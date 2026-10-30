use super::{model::whole_function, *};

/// Where an actor keeps its path (Java `Actor.pathX`, `pathY`, `pathLength`): scene tiles, newest first, so entry 0 is the tile the server last put the actor on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActorPath {
    pub x: u32,
    pub y: u32,
    pub length: u32,
    pub function: u32,
}

fn is_store(i: &Instruction) -> bool {
    i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.memory_base() == Register::RCX
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt32
        && i.op1_kind() == OpKind::Register
}

// The path reset `(actor, x in edx, y in r8d)` (Java `Player.ax`): zeroes the length, stores the tile as entry 0 and moves the actor onto it, `local = (size + 2 * tile) << 6` (`tile * 128 + size * 64`).
fn path_reset(function: u32, instrs: &[Instruction], local_x: u32) -> Option<ActorPath> {
    if instrs.last()?.mnemonic() != Mnemonic::Ret || instrs.iter().any(|i| matches!(i.mnemonic(), Mnemonic::Call | Mnemonic::Jmp)) {
        return None;
    }
    let stores_of = |register: Register| -> Vec<u32> {
        instrs.iter().filter(|i| is_store(i) && i.op1_register().full_register() == register).map(|i| i.memory_displacement64() as u32).collect()
    };
    let x = one(stores_of(Register::RDX)).ok()?;
    let y = one(stores_of(Register::R8)).ok()?;

    // the length is the dword stored from a register while it holds the zero of `xor r, r`
    let mut zero: Vec<Register> = Vec::new();
    let mut zeroed = Vec::new();
    for i in instrs {
        if is_store(i) && zero.contains(&i.op1_register().full_register()) {
            zeroed.push(i.memory_displacement64() as u32);
        }
        if i.op0_kind() == OpKind::Register {
            let register = i.op0_register().full_register();
            zero.retain(|&r| r != register);
            if i.mnemonic() == Mnemonic::Xor && i.op1_kind() == OpKind::Register && i.op1_register() == i.op0_register() {
                zero.push(register);
            }
        }
    }
    let length = one(zeroed).ok()?;

    // both fine coordinates are rebuilt from the new tile: `lea r, [size + tile*2]`, `shl r, 6`, stored at local_x and local_x + 4
    let doubles = |tile: Register| instrs.iter().any(|i| i.mnemonic() == Mnemonic::Lea && i.memory_index().full_register() == tile && i.memory_index_scale() == 2);
    let shifts = instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Shl && i.op1_kind() == OpKind::Immediate8 && i.immediate(1) == 6).count();
    let stored = |disp: u32| instrs.iter().any(|i| is_store(i) && i.memory_displacement64() as u32 == disp);
    (doubles(Register::RDX) && doubles(Register::R8) && shifts == 2 && stored(local_x) && stored(local_x + 4) && x < y).then_some(ActorPath { x, y, length, function })
}

/// `local_x` is the actor's fine position field (`ActorPosition::x`).
pub fn find_actor_path(pe: &Pe, local_x: u32) -> Result<ActorPath, ScanError> {
    // the reset is a leaf with no `.pdata` entry, so the candidates are the targets of direct calls, each up to its first `ret`
    let mut targets: Vec<u32> = direct_calls(pe).iter().map(|&(_, target)| target).collect();
    targets.sort_unstable();
    targets.dedup();
    let found = targets
        .into_iter()
        .filter_map(|target| {
            let mut instrs = whole_function(pe, target, 0x60);
            instrs.truncate(instrs.iter().position(|i| i.mnemonic() == Mnemonic::Ret)? + 1);
            path_reset(target, &instrs, local_x)
        })
        .collect();
    one(found)
}
