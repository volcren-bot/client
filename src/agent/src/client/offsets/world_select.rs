use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSelect {
    pub open: u32,
    pub count: u32,
    pub worlds: u32,
    pub offset: u32,
    pub max_offset: u32,
    pub hovered: u32,
    pub current_world: u32,
    pub background: u32,
    pub background_height: u32,
    pub world: WorldFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldFields {
    pub id: u32,
    pub types: u32,
    pub players: u32,
    pub activity: u32,
    pub location: u32,
}

const BOX_WIDTH: u32 = 88;
const BOX_HEIGHT: u32 = 19;
const PANEL_WIDTH: u32 = 765;
const PANEL_HEIGHT: u32 = 480;

fn function_instrs(pe: &Pe, begin: u32, end: u32) -> Vec<Instruction> {
    Decoder::with_ip(64, pe.bytes(begin, end.max(begin)), begin as u64, DecoderOptions::NONE).into_iter().collect()
}

fn immediate(instr: &Instruction) -> Option<u32> {
    matches!(
        instr.op1_kind(),
        OpKind::Immediate8 | OpKind::Immediate8to32 | OpKind::Immediate32 | OpKind::Immediate8to64 | OpKind::Immediate32to64
    )
    .then(|| instr.immediate(1) as u32)
}

fn global_load(instr: &Instruction) -> Option<u32> {
    (instr.mnemonic() == Mnemonic::Mov
        && instr.op0_kind() == OpKind::Register
        && instr.op1_kind() == OpKind::Memory
        && instr.is_ip_rel_memory_operand())
    .then(|| instr.ip_rel_memory_address() as u32)
}

fn global_store(instr: &Instruction) -> Option<u32> {
    (instr.mnemonic() == Mnemonic::Mov && instr.op0_kind() == OpKind::Memory && instr.is_ip_rel_memory_operand())
        .then(|| instr.ip_rel_memory_address() as u32)
}

fn field_of(instr: &Instruction, base: Register) -> Option<u32> {
    (instr.op1_kind() == OpKind::Memory && instr.memory_base() == base && instr.memory_index() == Register::None)
        .then(|| instr.memory_displacement64() as u32)
}

fn call_target(instr: &Instruction) -> Option<u32> {
    (instr.mnemonic() == Mnemonic::Call && instr.op0_kind() == OpKind::NearBranch64).then(|| instr.near_branch_target() as u32)
}

fn layout_function(pe: &Pe) -> Result<(u32, u32), ScanError> {
    let mut found = Vec::new();
    for site in dword_hits(pe, PANEL_WIDTH) {
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let stores = |value: u32| {
            instrs.iter().any(|i| {
                i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && !i.is_ip_rel_memory_operand() && immediate(i) == Some(value)
            })
        };
        let sets_height = instrs
            .iter()
            .any(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && immediate(i) == Some(PANEL_HEIGHT));
        if !(stores(BOX_WIDTH) && stores(BOX_HEIGHT) && sets_height) {
            continue;
        }
        let mut globals: Vec<u32> = instrs.iter().filter_map(global_load).collect();
        globals.sort_unstable();
        globals.dedup();
        if let [count] = globals.as_slice() {
            found.push((begin, *count));
        }
    }
    one(found)
}

fn draw_function(pe: &Pe, layout: u32) -> Result<(u32, u32), ScanError> {
    let mut found = Vec::new();
    for &(site, target) in direct_calls(pe) {
        if target != layout {
            continue;
        }
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else { continue };
        let passes_global = at > 0 && {
            let lea = &instrs[at - 1];
            lea.mnemonic() == Mnemonic::Lea && lea.op0_register() == Register::RCX && lea.is_ip_rel_memory_operand()
        };
        if passes_global {
            found.push((begin, end));
        }
    }
    one(found)
}

fn hovered(instrs: &[Instruction]) -> Result<u32, ScanError> {
    one(instrs
        .iter()
        .filter(|i| i.memory_size() == iced_x86::MemorySize::UInt32 && immediate(i) == Some(u32::MAX))
        .filter_map(global_store)
        .collect())
}

fn max_offset(instrs: &[Instruction]) -> Result<u32, ScanError> {
    let divisions: Vec<usize> = instrs.iter().enumerate().filter(|(_, i)| i.mnemonic() == Mnemonic::Idiv).map(|(at, _)| at).collect();
    one(divisions.into_iter().filter_map(|at| instrs[at + 1..].iter().take(6).find_map(global_store)).collect())
}

fn world_loads(instrs: &[Instruction]) -> Result<(u32, Vec<(usize, Register)>), ScanError> {
    let element = |at: usize| -> Option<Register> {
        let base = instrs[at].op0_register();
        instrs[at + 1..].iter().take(4).find_map(|i| {
            (i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_kind() == OpKind::Memory
                && i.memory_base() == base
                && i.memory_index() != Register::None)
                .then(|| i.op0_register())
        })
    };
    let loads: Vec<(u32, usize, Register)> = (0..instrs.len())
        .filter_map(|at| {
            let global = global_load(&instrs[at]).filter(|_| instrs[at].op0_register().size() == 8)?;
            Some((global, at, element(at)?))
        })
        .collect();
    let worlds = one(loads.iter().map(|&(global, ..)| global).collect())?;
    Ok((worlds, loads.into_iter().map(|(_, at, element)| (at, element)).collect()))
}

fn flag_accessor(pe: &Pe, function: u32) -> Option<u32> {
    let instrs = decode_at(pe, function, 4);
    let [load, test, ..] = instrs.as_slice() else { return None };
    let disp = field_of(load, Register::RCX)?;
    let same = test.op0_register().full_register() == load.op0_register().full_register();
    match (load.mnemonic(), test.mnemonic()) {
        (Mnemonic::Mov, Mnemonic::Shr) if same && load.memory_size() == iced_x86::MemorySize::UInt32 => Some(disp),
        (Mnemonic::Movzx, Mnemonic::And) if same && load.memory_size() == iced_x86::MemorySize::UInt8 && disp % 4 == 0 => Some(disp),
        _ => None,
    }
}

fn world_fields(pe: &Pe, instrs: &[Instruction], loads: &[(usize, Register)]) -> Result<(WorldFields, u32), ScanError> {
    let &(first, world) = loads.first().ok_or(ScanError::NotFound)?;
    let body = &instrs[first..];
    let field = |i: &Instruction| field_of(i, world);

    let players = one(body
        .windows(2)
        .filter(|w| w[0].mnemonic() == Mnemonic::Mov && w[1].mnemonic() == Mnemonic::Cmp)
        .filter(|w| w[1].op0_kind() == OpKind::Register && w[1].op0_register() == w[0].op0_register() && immediate(&w[1]) == Some(u32::MAX))
        .filter_map(|w| field(&w[0]))
        .collect())?;
    let location = one(body.iter().filter(|i| i.mnemonic() == Mnemonic::Movsxd).filter_map(field).collect())?;

    let (id, format) = one(body
        .iter()
        .enumerate()
        .filter(|(_, i)| i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::R9D)
        .filter_map(|(n, i)| Some((field(i).filter(|&disp| disp != players)?, body[n + 1..].iter().take(8).find_map(call_target)?)))
        .collect())?;

    let types = one(body
        .iter()
        .enumerate()
        .filter(|(_, i)| i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::RCX && i.op1_kind() == OpKind::Register && i.op1_register() == world)
        .filter_map(|(n, _)| flag_accessor(pe, body[n + 1..].iter().take(3).find_map(call_target)?))
        .collect())?;

    let activity = one(loads
        .iter()
        .skip(1)
        .filter_map(|&(at, element)| {
            instrs[at + 1..]
                .iter()
                .take(8)
                .find(|i| i.mnemonic() == Mnemonic::Add && i.op0_kind() == OpKind::Register && i.op0_register() == element)
                .and_then(immediate)
        })
        .collect())?;

    Ok((WorldFields { id, types, players, activity, location }, format))
}

fn open_function(pe: &Pe, max_offset: u32) -> Result<(u32, u32), ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x80 {
            continue;
        }
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| global_store(i) == Some(max_offset) && i.op1_kind() == OpKind::Register) else {
            continue;
        };
        let zero = instrs[at].op1_register();
        let before = &instrs[at.saturating_sub(4)..at];
        let offset = before.iter().rev().find_map(|i| global_store(i).filter(|_| i.op1_kind() == OpKind::Register && i.op1_register() == zero));
        let open = before
            .iter()
            .find_map(|i| global_store(i).filter(|_| i.memory_size() == iced_x86::MemorySize::UInt8 && immediate(i) == Some(1)));
        if let (Some(offset), Some(open)) = (offset, open) {
            found.push((open, offset));
        }
    }
    one(found)
}

fn login_draw(pe: &Pe, draw: u32, open: u32, client: u32, format: u32) -> Result<(u32, u32, u32), ScanError> {
    let mut found = Vec::new();
    for &(site, target) in direct_calls(pe) {
        if target != draw {
            continue;
        }
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else { continue };
        let gated = instrs[at.saturating_sub(8)..at].iter().any(|i| {
            i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.is_ip_rel_memory_operand() && i.ip_rel_memory_address() as u32 == open
        });
        if !gated {
            continue;
        }

        let mut client_regs: HashSet<Register> = HashSet::new();
        let mut world = Vec::new();
        for (n, i) in instrs.iter().enumerate() {
            let from_client = i.mnemonic() == Mnemonic::Mov
                && i.op0_register() == Register::R9D
                && i.op1_kind() == OpKind::Memory
                && i.memory_index() == Register::None
                && client_regs.contains(&i.memory_base());
            if from_client && instrs[n + 1..].iter().take(4).any(|c| call_target(c) == Some(format)) {
                world.push(i.memory_displacement64() as u32);
            }
            if i.mnemonic() == Mnemonic::Call {
                client_regs.clear();
            } else if global_load(i) == Some(client) {
                client_regs.insert(i.op0_register().full_register());
            } else if i.op0_kind() == OpKind::Register {
                client_regs.remove(&i.op0_register().full_register());
            }
        }

        let background = instrs
            .windows(3)
            .filter_map(|w| {
                let global = global_load(&w[0]).filter(|_| w[0].op0_register().size() == 8)?;
                let height = load_from(&w[1], w[0].op0_register()).filter(|_| w[1].op0_register().size() == 4)?;
                let clamps = w[2].mnemonic() == Mnemonic::Cmp && w[2].op1_kind() == OpKind::Register && w[2].op1_register() == w[1].op0_register();
                clamps.then_some((global, height))
            })
            .collect();
        let (background, height) = one(background)?;
        found.push((one(world)?, background, height));
    }
    one(found)
}

pub fn find_world_select(pe: &Pe, client: u32) -> Result<WorldSelect, ScanError> {
    let (layout, count) = layout_function(pe)?;
    let (begin, end) = draw_function(pe, layout)?;
    let instrs = function_instrs(pe, begin, end);
    let max_offset = max_offset(&instrs)?;
    let hovered = hovered(&instrs)?;
    let (worlds, loads) = world_loads(&instrs)?;
    let (world, format) = world_fields(pe, &instrs, &loads)?;
    let (open, offset) = open_function(pe, max_offset)?;
    let (current_world, background, background_height) = login_draw(pe, begin, open, client, format)?;
    Ok(WorldSelect { open, count, worlds, offset, max_offset, hovered, current_world, background, background_height, world })
}
