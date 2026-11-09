use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Packets {
    pub receive: u32,
    pub receive_opcode: u32,
    pub receive_buffer: u32,
    pub send: u32,
    pub send_type: u32,
    pub send_buffer: u32,
    pub buffer_data: u32,
    pub buffer_cursor: u32,
}

pub const BUFFER_SIZE: u32 = 8;

fn imm1(i: &Instruction) -> Option<u64> {
    matches!(
        i.op1_kind(),
        OpKind::Immediate8
            | OpKind::Immediate8to16
            | OpKind::Immediate8to32
            | OpKind::Immediate8to64
            | OpKind::Immediate16
            | OpKind::Immediate32
            | OpKind::Immediate32to64
            | OpKind::Immediate64
    )
    .then(|| i.immediate(1))
}

fn opcode_writer(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let calls = instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Call)
        .count();
    let adds_byte = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Add
            && i.op0_kind() == OpKind::Register
            && i.op1_kind() == OpKind::Register
            && i.op0_register().size() == 1
            && i.op1_register().size() == 1
    });
    let refills = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && imm1(i) == Some(0x100)
    });
    let stores_byte = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Memory
            && i.memory_size() == iced_x86::MemorySize::UInt8
            && i.memory_index() != Register::None
            && i.op1_kind() == OpKind::Register
    });
    if calls != 1 || !adds_byte || !refills || !stores_byte {
        return None;
    }

    let buffer = instrs
        .iter()
        .take(10)
        .find(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_register() == Register::RCX
        })?
        .op0_register();
    let store = |i: &Instruction| {
        i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Memory
            && i.memory_base() == buffer
            && i.memory_index() == Register::None
            && i.op1_kind() == OpKind::Register
    };
    let cursor = one(instrs
        .iter()
        .filter(|i| store(i))
        .map(|i| i.memory_displacement64() as u32)
        .collect())
    .ok()?;
    let data = one(instrs
        .iter()
        .filter_map(|i| load_from(i, buffer))
        .filter(|&d| d != cursor)
        .collect())
    .ok()?;
    Some((data, cursor))
}

fn constructor_fields(instrs: &[Instruction], call: usize) -> Option<(u32, u32)> {
    let buffer = instrs[call.saturating_sub(6)..call]
        .iter()
        .rev()
        .find(|i| {
            i.mnemonic() == Mnemonic::Lea
                && i.op0_register() == Register::RCX
                && i.memory_index() == Register::None
        })?
        .memory_displacement64() as u32;

    let packet = instrs
        .iter()
        .take(12)
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op1_register() == Register::RCX
        })
        .map(|i| i.op0_register());
    let bases: Vec<Register> = core::iter::once(Register::RCX).chain(packet).collect();
    let id = instrs
        .iter()
        .find(|i| i.mnemonic() == Mnemonic::Movsxd && i.op1_register() == Register::EDX)?
        .op0_register()
        .full_register32();
    let kind = one(instrs
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
        .collect())
    .ok()?;
    Some((kind, buffer))
}

fn is_sender(instrs: &[Instruction], data: u32, cursor: u32) -> bool {
    if instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Call)
        .count()
        != 1
    {
        return false;
    }
    let copies = instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_register() == Register::R8
        })
        .map(|i| i.op0_register());
    let packet: Vec<Register> = core::iter::once(Register::R8).chain(copies).collect();
    let loads = |disp: u32, into: Register| {
        instrs.iter().any(|i| {
            i.op0_kind() == OpKind::Register
                && i.op0_register().full_register() == into
                && packet.iter().any(|&base| load_from(i, base) == Some(disp))
        })
    };
    loads(data, Register::RDX) && loads(cursor, Register::R8)
}

fn receive_fields(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let state_is_3 = instrs.iter().take(6).any(|i| {
        i.mnemonic() == Mnemonic::Cmp
            && i.op0_kind() == OpKind::Memory
            && i.memory_base() == Register::RCX
            && i.memory_size() == iced_x86::MemorySize::UInt32
            && imm1(i) == Some(3)
    });
    let kinds = |value: u64| {
        instrs.iter().any(|i| {
            i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && imm1(i) == Some(value)
        })
    };
    if !state_is_3 || !kinds(3) || !kinds(4) {
        return None;
    }
    let out = instrs
        .iter()
        .take(8)
        .find(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_register() == Register::RDX
        })?
        .op0_register();
    let opcode = one(instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == out
                && i.memory_index() == Register::None
                && i.memory_size() == iced_x86::MemorySize::UInt32
                && i.op1_kind() == OpKind::Register
        })
        .map(|i| i.memory_displacement64() as u32)
        .collect())
    .ok()?;
    let buffer = one(instrs
        .windows(6)
        .filter(|w| {
            w[0].mnemonic() == Mnemonic::Lea
                && w[0].op0_register() == Register::RCX
                && w[0].memory_base() == out
                && w[0].memory_index() == Register::None
                && w[1..].iter().any(|i| i.mnemonic() == Mnemonic::Call)
        })
        .map(|w| w[0].memory_displacement64() as u32)
        .collect())
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
            writers.extend(opcode_writer(&instrs).map(|(data, cursor)| (begin, data, cursor)));
        }
        if end - begin <= 0x60 {
            small.push((begin, instrs));
            continue;
        }
        receivers.extend(receive_fields(&instrs).map(|(opcode, buffer)| (begin, opcode, buffer)));
    }
    let (writer, data, cursor) = one(writers)?;

    let site = one(calls_to(pe, writer).collect())?;

    let (instrs, call) = site_function(pe, site).ok_or(ScanError::NotFound)?;
    let (send_type, send_buffer) = constructor_fields(&instrs, call).ok_or(ScanError::NotFound)?;

    let send = one(small
        .iter()
        .filter(|(_, instrs)| is_sender(instrs, send_buffer + data, send_buffer + cursor))
        .map(|&(begin, _)| begin)
        .collect())?;
    let (receive, receive_opcode, receive_buffer) = one(receivers)?;
    Ok(Packets {
        receive,
        receive_opcode,
        receive_buffer,
        send,
        send_type,
        send_buffer,
        buffer_data: data,
        buffer_cursor: cursor,
    })
}
