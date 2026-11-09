use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorPath {
    pub x: u32,
    pub y: u32,
    pub length: u32,
    pub function: u32,
}

fn is_store(i: &Instruction) -> bool {
    dword_store(i, Register::RCX)
}

fn path_reset(function: u32, instrs: &[Instruction], local_x: u32) -> Option<ActorPath> {
    if instrs
        .iter()
        .any(|i| matches!(i.mnemonic(), Mnemonic::Call | Mnemonic::Jmp))
    {
        return None;
    }
    let stores_of = |register: Register| -> Vec<u32> {
        instrs
            .iter()
            .filter(|i| is_store(i) && i.op1_register().full_register() == register)
            .map(|i| i.memory_displacement64() as u32)
            .collect()
    };
    let x = one(stores_of(Register::RDX)).ok()?;
    let y = one(stores_of(Register::R8)).ok()?;

    let mut zero: Vec<Register> = Vec::new();
    let mut zeroed = Vec::new();
    for i in instrs {
        if is_store(i) && zero.contains(&i.op1_register().full_register()) {
            zeroed.push(i.memory_displacement64() as u32);
        }
        if i.op0_kind() == OpKind::Register {
            let register = i.op0_register().full_register();
            zero.retain(|&r| r != register);
            if i.mnemonic() == Mnemonic::Xor
                && i.op1_kind() == OpKind::Register
                && i.op1_register() == i.op0_register()
            {
                zero.push(register);
            }
        }
    }
    let length = one(zeroed).ok()?;

    let doubles = |tile: Register| {
        instrs.iter().any(|i| {
            i.mnemonic() == Mnemonic::Lea
                && i.memory_index().full_register() == tile
                && i.memory_index_scale() == 2
        })
    };
    let shifts = instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Shl
                && i.op1_kind() == OpKind::Immediate8
                && i.immediate(1) == 6
        })
        .count();
    let stored = |disp: u32| {
        instrs
            .iter()
            .any(|i| is_store(i) && i.memory_displacement64() as u32 == disp)
    };
    (doubles(Register::RDX)
        && doubles(Register::R8)
        && shifts == 2
        && stored(local_x)
        && stored(local_x + 4)
        && x < y)
        .then_some(ActorPath {
            x,
            y,
            length,
            function,
        })
}

pub fn find_actor_path(pe: &Pe, local_x: u32) -> Result<ActorPath, ScanError> {
    let found = call_targets(pe)
        .into_iter()
        .filter_map(|target| {
            let mut instrs = whole_function(pe, target, 0x60);
            instrs.truncate(instrs.iter().position(|i| i.mnemonic() == Mnemonic::Ret)? + 1);
            path_reset(target, &instrs, local_x)
        })
        .collect();
    one(found)
}
