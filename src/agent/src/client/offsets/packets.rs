use super::{model::whole_function, *};

/// The receive and send functions that see every packet, and where a packet keeps what the packet log records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Packets {
    /// RVA of the function that pops the next received packet (the game thread calls it from the packet dispatcher)
    pub receive: u32,
    /// the packet id in the `out` argument
    pub receive_opcode: u32,
    /// the buffer object in the `out` argument
    pub receive_buffer: u32,
    /// RVA of the function that sends a queued packet (on the network thread)
    pub send: u32,
    /// the client packet id in the packet
    pub send_type: u32,
    /// the buffer object in the packet
    pub send_buffer: u32,
    /// the data pointer and the written length in a buffer object
    pub buffer_data: u32,
    pub buffer_cursor: u32,
}

// Where a buffer object keeps the length a payload was allocated with; no code shape names it, checked against the packet reader's size table.
pub const BUFFER_SIZE: u32 = 8;

fn imm1(i: &Instruction) -> Option<u64> {
    matches!(
        i.op1_kind(),
        OpKind::Immediate8 | OpKind::Immediate8to16 | OpKind::Immediate8to32 | OpKind::Immediate8to64 | OpKind::Immediate16 | OpKind::Immediate32 | OpKind::Immediate32to64 | OpKind::Immediate64
    )
    .then(|| i.immediate(1))
}

// The opcode writer `(buffer, opcode in dl, cipher)`: adds the cipher byte to the opcode and appends it to the buffer.
struct Writer {
    function: u32,
    data: u32,
    cursor: u32,
}

fn opcode_writer(function: u32, instrs: &[Instruction]) -> Option<Writer> {
    let calls = instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Call).count();
    let adds_byte = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Add && i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op0_register().size() == 1 && i.op1_register().size() == 1
    });
    let refills = instrs.iter().any(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && imm1(i) == Some(0x100));
    let stores_byte = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt8 && i.memory_index() != Register::None && i.op1_kind() == OpKind::Register
    });
    if calls != 1 || !adds_byte || !refills || !stores_byte {
        return None;
    }

    let buffer = instrs.iter().take(10).find(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 8 && i.op1_register() == Register::RCX)?.op0_register();
    let store = |i: &Instruction| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == buffer && i.memory_index() == Register::None && i.op1_kind() == OpKind::Register;
    let cursor = one(instrs.iter().filter(|i| store(i)).map(|i| i.memory_displacement64() as u32).collect()).ok()?;
    let data = one(instrs.iter().filter_map(|i| load_from(i, buffer)).filter(|&d| d != cursor).collect()).ok()?;
    Some(Writer { function, data, cursor })
}

// The packet constructor calls the writer with `lea rcx, [packet + buffer]` and stores the packet id.
fn constructor_fields(instrs: &[Instruction], call: usize) -> Option<(u32, u32)> {
    let buffer = instrs[call.saturating_sub(6)..call]
        .iter()
        .rev()
        .find(|i| i.mnemonic() == Mnemonic::Lea && i.op0_register() == Register::RCX && i.memory_index() == Register::None)?
        .memory_displacement64() as u32;

    let packet = instrs.iter().take(12).filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_register() == Register::RCX).map(|i| i.op0_register());
    let bases: Vec<Register> = core::iter::once(Register::RCX).chain(packet).collect();
    let id = instrs.iter().find(|i| i.mnemonic() == Mnemonic::Movsxd && i.op1_register() == Register::EDX)?.op0_register().full_register32();
    let kind = one(
        instrs
            .iter()
            .filter(|i| {
                i.mnemonic() == Mnemonic::Mov
                    && i.op0_kind() == OpKind::Memory
                    && i.memory_size() == iced_x86::MemorySize::UInt32
                    && bases.contains(&i.memory_base())
                    && i.memory_index() == Register::None
                    && i.op1_kind() == OpKind::Register
                    && i.op1_register() == id
            })
            .map(|i| i.memory_displacement64() as u32)
            .collect(),
    )
    .ok()?;
    Some((kind, buffer))
}

// The send function passes the packet's buffer data pointer and length to the socket write.
fn is_sender(instrs: &[Instruction], data: u32, cursor: u32) -> bool {
    if instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Call).count() != 1 {
        return false;
    }
    let copies = instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 8 && i.op1_register() == Register::R8).map(|i| i.op0_register());
    let packet: Vec<Register> = core::iter::once(Register::R8).chain(copies).collect();
    // they are the arguments of the socket write (data in `rdx`, length in `r8`), which sets the sender apart from a copy of the packet
    let loads = |disp: u32, into: Register| instrs.iter().any(|i| i.op0_kind() == OpKind::Register && i.op0_register().full_register() == into && packet.iter().any(|&base| load_from(i, base) == Some(disp)));
    loads(data, Register::RDX) && loads(cursor, Register::R8)
}

// The receive function: checks the connection state, compares the queue node's kind with 3 and 4, stores the packet id at the start of `out`.
fn receive_fields(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let state_is_3 = instrs.iter().take(6).any(|i| {
        i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RCX && i.memory_size() == iced_x86::MemorySize::UInt32 && imm1(i) == Some(3)
    });
    let kinds = |value: u64| instrs.iter().any(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Register && imm1(i) == Some(value));
    if !state_is_3 || !kinds(3) || !kinds(4) {
        return None;
    }
    let out = instrs.iter().take(8).find(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 8 && i.op1_register() == Register::RDX)?.op0_register();
    let opcode = one(
        instrs
            .iter()
            .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == out && i.memory_index() == Register::None && i.memory_size() == iced_x86::MemorySize::UInt32 && i.op1_kind() == OpKind::Register)
            .map(|i| i.memory_displacement64() as u32)
            .collect(),
    )
    .ok()?;
    let buffer = one(
        instrs
            .windows(6)
            .filter(|w| {
                w[0].mnemonic() == Mnemonic::Lea && w[0].op0_register() == Register::RCX && w[0].memory_base() == out && w[0].memory_index() == Register::None && w[1..].iter().any(|i| i.mnemonic() == Mnemonic::Call)
            })
            .map(|w| w[0].memory_displacement64() as u32)
            .collect(),
    )
    .ok()?;
    Some((opcode, buffer))
}

pub fn find_packets(pe: &Pe) -> Result<Packets, ScanError> {

    let mut writers = Vec::new();
    let mut receivers = Vec::new();
    let mut small = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x200 {
            continue;
        }
        let instrs = whole_function(pe, begin, end - begin);
        if end - begin <= 0x80 {
            writers.extend(opcode_writer(begin, &instrs).map(|w| (w.function, w.data, w.cursor)));
        }
        if end - begin <= 0x60 {
            small.push((begin, instrs));
            continue;
        }
        receivers.extend(receive_fields(&instrs).map(|(opcode, buffer)| (begin, opcode, buffer)));
    }
    let (writer, data, cursor) = one(writers)?;

    // the one place that builds a packet: it writes the opcode through the writer
    let sites: Vec<u32> = direct_calls(pe).iter().copied().filter(|&(_, target)| target == writer).map(|(site, _)| site).collect();
    let site = one(sites)?;
    let (begin, end) = pe.function_containing(site).ok_or(ScanError::NotFound)?;
    let instrs = whole_function(pe, begin, end - begin);
    let call = instrs.iter().position(|i| i.ip() == site as u64).ok_or(ScanError::NotFound)?;
    let (send_type, send_buffer) = constructor_fields(&instrs, call).ok_or(ScanError::NotFound)?;

    let send = one(small.iter().filter(|(_, instrs)| is_sender(instrs, send_buffer + data, send_buffer + cursor)).map(|&(begin, _)| begin).collect())?;
    let (receive, receive_opcode, receive_buffer) = one(receivers)?;
    Ok(Packets { receive, receive_opcode, receive_buffer, send, send_type, send_buffer, buffer_data: data, buffer_cursor: cursor })
}
