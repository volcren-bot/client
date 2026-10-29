use super::{model::whole_function, *};

/// The spot animations placed on tiles: a list in the world view and the object fields, from the constructor and the code that appends one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpotAnims {
    /// the list in the world view: a circular list whose head is the sentinel node (next, previous, size)
    pub list: u32,
    /// RVA of the client cycle the start cycle counts in (50 per second)
    pub cycle: u32,
    pub start_cycle: u32,
    pub level: u32,
    /// local fine x, y and z (height above the ground, as the packet handler works it out)
    pub x: u32,
    pub y: u32,
    pub z: u32,
    /// the spot animation id
    pub id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Fields {
    start_cycle: u32,
    level: u32,
    x: u32,
    y: u32,
    z: u32,
    id: u32,
}

// The list sits in a world view, a few hundred bytes in; the displacements below that are shared pointer fields.
const LIST_RANGE: std::ops::Range<u64> = 0x100..0x1000;

fn is_store(i: &Instruction, this: Register) -> bool {
    i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.memory_base() == this
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt32
        && i.op1_kind() == OpKind::Register
}

fn stack_load(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Register
        && i.op1_kind() == OpKind::Memory
        && i.memory_base() == Register::RSP
        && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

// The constructor `(this, world view, id in r8d, level in r9d, then x, y, z, delay, cycle on the stack)` looks up the id and stores the parameters.
fn constructor_fields(instrs: &[Instruction]) -> Option<Fields> {
    let this = instrs.iter().take(8).find(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_register() == Register::RCX && i.op0_register().size() == 8)?.op0_register();
    let init = instrs.windows(5).position(|w| {
        w[0].mnemonic() == Mnemonic::Call
            && w[0].op0_kind() == OpKind::NearBranch64
            && w[1..4].iter().any(|i| load_from(i, Register::RAX) == Some(8))
            && w[1..4].iter().any(|i| load_from(i, Register::RDX) == Some(8))
            && w[4].mnemonic() == Mnemonic::Call
    })?;
    let id_register = instrs[..init].iter().rev().find(|i| i.op0_kind() == OpKind::Register && i.op0_register() == Register::EDX)?;
    if id_register.mnemonic() != Mnemonic::Mov || id_register.op1_kind() != OpKind::Register {
        return None;
    }
    let id_register = id_register.op1_register();
    let store_of = |register: Register| one(instrs.iter().filter(|i| is_store(i, this) && i.op1_register() == register).map(|i| i.memory_displacement64() as u32).collect()).ok();
    let id = store_of(id_register)?;
    let level = store_of(Register::R9D)?;

    // `add edx, [rsp + delay]` (the start cycle is the delay plus the cycle), stored a few instructions later
    let start_cycle = instrs.iter().enumerate().find_map(|(at, add)| {
        if !(add.mnemonic() == Mnemonic::Add && add.op0_register() == Register::EDX && add.op1_kind() == OpKind::Memory && add.memory_base() == Register::RSP) {
            return None;
        }
        instrs[at + 1..(at + 16).min(instrs.len())].iter().find(|i| is_store(i, this) && i.op1_register() == Register::EDX).map(|i| i.memory_displacement64() as u32)
    })?;

    // `mov eax, [rsp + n]; mov [this + field], eax` for x, y and z, in stack order
    let mut stack: Vec<(u32, u32)> = instrs
        .windows(2)
        .filter_map(|w| {
            let from = stack_load(&w[0])?;
            (is_store(&w[1], this) && w[1].op1_register() == w[0].op0_register()).then(|| (from, w[1].memory_displacement64() as u32))
        })
        .collect();
    stack.sort_unstable();
    let [(_, x), (_, y), (_, z)] = stack[..] else { return None };
    Some(Fields { start_cycle, level, x, y, z, id })
}

// Calls and tail jumps to `target`.
fn jumps_to(pe: &Pe, target: u32) -> Vec<u32> {
    let mut found = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(5).enumerate() {
            if window[0] == 0xe9 && (section.rva + offset) as i64 + 5 + i32::from_le_bytes(window[1..5].try_into().unwrap()) as i64 == target as i64 {
                found.push((section.rva + offset) as u32);
            }
        }
    }
    found
}

// From a call that builds the object: the list offset in the world view and the client cycle, the last stack argument.
fn creation_site(instrs: &[Instruction], call: usize) -> Option<(u32, u32)> {
    let before = &instrs[call.saturating_sub(40)..call];
    let last = before
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RSP && i.op1_kind() == OpKind::Register && i.op1_register().size() == 8)
        .max_by_key(|i| i.memory_displacement64())?;
    let cycle = before.iter().rev().find(|i| i.mnemonic() == Mnemonic::Lea && i.op0_register() == last.op1_register() && i.is_ip_rel_memory_operand())?.ip_rel_memory_address() as u32;

    let after = &instrs[call + 1..];
    let list = after.iter().take(120).enumerate().find_map(|(at, i)| {
        let disp = match (i.mnemonic(), i.op1_kind()) {
            (Mnemonic::Lea, OpKind::Memory) if i.op0_register() == Register::RCX && i.memory_index() == Register::None => i.memory_displacement64(),
            (Mnemonic::Add, OpKind::Immediate32to64 | OpKind::Immediate8to64) if i.op0_register() == Register::RCX => i.immediate(1),
            _ => return None,
        };
        let appended = after[at + 1..(at + 4).min(after.len())].iter().any(|n| matches!(n.mnemonic(), Mnemonic::Call | Mnemonic::Jmp) && n.op0_kind() == OpKind::NearBranch64);
        (appended && LIST_RANGE.contains(&disp)).then_some(disp as u32)
    })?;
    Some((list, cycle))
}

pub fn find_spot_anims(image: &[u8]) -> Result<SpotAnims, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut constructors = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x200 {
            continue;
        }
        let instrs = whole_function(&pe, begin, end - begin);
        constructors.extend(constructor_fields(&instrs).map(|fields| (begin, fields)));
    }
    let (constructor, fields) = one(constructors)?;

// `std::make_shared` reaches the constructor through a thunk; the packet handler calls it directly or through a wrapper.
    let thunks = jumps_to(&pe, constructor);
    let calls = direct_calls(&pe);
    let mut sites = Vec::new();
    for thunk in thunks.iter().filter_map(|&at| pe.function_containing(at)).map(|(begin, _)| begin) {
        for &(site, _) in calls.iter().filter(|&&(_, target)| target == thunk) {
            let Some((begin, end)) = pe.function_containing(site) else { continue };
            if end - begin <= 0x200 {
                sites.extend(calls.iter().filter(|&&(_, target)| target == begin).map(|&(caller, _)| caller));
            } else {
                sites.push(site);
            }
        }
    }
    let mut found = Vec::new();
    for site in sites {
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = whole_function(&pe, begin, end - begin);
        let Some(call) = instrs.iter().position(|i| i.ip() == site as u64) else { continue };
        if let Some((list, cycle)) = creation_site(&instrs, call) {
            found.push((list, cycle));
        }
    }
    let (list, cycle) = one(found)?;
    Ok(SpotAnims { list, cycle, start_cycle: fields.start_cycle, level: fields.level, x: fields.x, y: fields.y, z: fields.z, id: fields.id })
}
