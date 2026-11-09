use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpotAnims {
    pub list: u32,
    pub cycle: u32,
    pub start_cycle: u32,
    pub level: u32,
    pub x: u32,
    pub y: u32,
    pub z: u32,
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

fn constructor_fields(instrs: &[Instruction]) -> Option<Fields> {
    let this = instrs
        .iter()
        .take(8)
        .find(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op1_register() == Register::RCX
                && i.op0_register().size() == 8
        })?
        .op0_register();
    let init = instrs.windows(5).position(|w| {
        call_target(&w[0]).is_some()
            && w[1..4]
                .iter()
                .any(|i| load_from(i, Register::RAX) == Some(8))
            && w[1..4]
                .iter()
                .any(|i| load_from(i, Register::RDX) == Some(8))
            && w[4].mnemonic() == Mnemonic::Call
    })?;
    let id_register = instrs[..init]
        .iter()
        .rev()
        .find(|i| i.op0_kind() == OpKind::Register && i.op0_register() == Register::EDX)?;
    if id_register.mnemonic() != Mnemonic::Mov || id_register.op1_kind() != OpKind::Register {
        return None;
    }
    let id_register = id_register.op1_register();
    let store_of = |register: Register| {
        one(instrs
            .iter()
            .filter(|i| is_store(i, this) && i.op1_register() == register)
            .map(|i| i.memory_displacement64() as u32)
            .collect())
        .ok()
    };
    let id = store_of(id_register)?;
    let level = store_of(Register::R9D)?;

    let start_cycle = instrs.iter().enumerate().find_map(|(at, add)| {
        if !(add.mnemonic() == Mnemonic::Add
            && add.op0_register() == Register::EDX
            && add.op1_kind() == OpKind::Memory
            && add.memory_base() == Register::RSP)
        {
            return None;
        }
        instrs[at + 1..(at + 16).min(instrs.len())]
            .iter()
            .find(|i| is_store(i, this) && i.op1_register() == Register::EDX)
            .map(|i| i.memory_displacement64() as u32)
    })?;

    let mut stack: Vec<(u32, u32)> = instrs
        .windows(2)
        .filter_map(|w| {
            let from = stack_load(&w[0])?;
            (is_store(&w[1], this) && w[1].op1_register() == w[0].op0_register())
                .then(|| (from, w[1].memory_displacement64() as u32))
        })
        .collect();
    stack.sort_unstable();
    let [(_, x), (_, y), (_, z)] = stack[..] else {
        return None;
    };
    Some(Fields {
        start_cycle,
        level,
        x,
        y,
        z,
        id,
    })
}

fn creation_site(instrs: &[Instruction], call: usize) -> Option<(u32, u32)> {
    let before = &instrs[call.saturating_sub(40)..call];
    let last = before
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RSP
                && i.op1_kind() == OpKind::Register
                && i.op1_register().size() == 8
        })
        .max_by_key(|i| i.memory_displacement64())?;
    let cycle = before
        .iter()
        .rev()
        .find(|i| {
            i.mnemonic() == Mnemonic::Lea
                && i.op0_register() == last.op1_register()
                && i.is_ip_rel_memory_operand()
        })?
        .ip_rel_memory_address() as u32;

    let after = &instrs[call + 1..];
    let list = after.iter().take(120).enumerate().find_map(|(at, i)| {
        let disp = match (i.mnemonic(), i.op1_kind()) {
            (Mnemonic::Lea, OpKind::Memory)
                if i.op0_register() == Register::RCX && i.memory_index() == Register::None =>
            {
                i.memory_displacement64()
            }
            (Mnemonic::Add, OpKind::Immediate32to64 | OpKind::Immediate8to64)
                if i.op0_register() == Register::RCX =>
            {
                i.immediate(1)
            }
            _ => return None,
        };
        let appended = after[at + 1..(at + 4).min(after.len())].iter().any(|n| {
            matches!(n.mnemonic(), Mnemonic::Call | Mnemonic::Jmp)
                && n.op0_kind() == OpKind::NearBranch64
        });
        (appended && LIST_RANGE.contains(&disp)).then_some(disp as u32)
    })?;
    Some((list, cycle))
}

pub fn find_spot_anims(pe: &Pe) -> Result<SpotAnims, ScanError> {
    let mut constructors = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x200 {
            continue;
        }
        let instrs = whole_function(pe, begin, end - begin);
        constructors.extend(constructor_fields(&instrs).map(|fields| (begin, fields)));
    }
    let (constructor, fields) = one(constructors)?;

    let calls = direct_calls(pe);
    let mut sites = Vec::new();
    for thunk in direct_jumps(pe)
        .filter(|&(_, target)| target == constructor)
        .filter_map(|(at, _)| pe.function_containing(at))
        .map(|(begin, _)| begin)
    {
        for &(site, _) in calls.iter().filter(|&&(_, target)| target == thunk) {
            let Some((begin, end)) = pe.function_containing(site) else {
                continue;
            };
            if end - begin <= 0x200 {
                sites.extend(
                    calls
                        .iter()
                        .filter(|&&(_, target)| target == begin)
                        .map(|&(caller, _)| caller),
                );
            } else {
                sites.push(site);
            }
        }
    }
    let mut found = Vec::new();
    for site in sites {
        if let Some((instrs, call)) = site_function(pe, site) {
            found.extend(creation_site(&instrs, call));
        }
    }
    let (list, cycle) = one(found)?;
    Ok(SpotAnims {
        list,
        cycle,
        start_cycle: fields.start_cycle,
        level: fields.level,
        x: fields.x,
        y: fields.y,
        z: fields.z,
        id: fields.id,
    })
}
