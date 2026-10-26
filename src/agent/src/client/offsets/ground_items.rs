use super::{model::whole_function, scene::DecorationLayout, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroundItems {
    pub items: u32,
    pub level: u32,
    pub node_item: u32,
    pub id: u32,
}

fn tile_lookup(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let first = instrs.first()?;
    if !(first.mnemonic() == Mnemonic::Mov
        && first.op0_register() == Register::RCX
        && first.op1_kind() == OpKind::Memory
        && first.memory_base() == Register::RDX
        && first.memory_index() == Register::None)
    {
        return None;
    }
    let tripled = instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Lea && i.memory_index_scale() == 2 && i.memory_base() == i.memory_index()).count();
    let levels: Vec<u32> = instrs
        .iter()
        .take_while(|i| i.mnemonic() != Mnemonic::Ret)
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op1_kind() == OpKind::Memory && i.memory_index_scale() == 8 && i.memory_index() != Register::None)
        .map(|i| i.memory_displacement64() as u32)
        .collect();
    (tripled == 3 && levels.len() == 2 && levels[0] == levels[1]).then_some((first.memory_displacement64() as u32, levels[0]))
}

fn id_compares(instrs: &[Instruction]) -> Vec<(u32, u32)> {
    instrs
        .windows(2)
        .filter_map(|w| {
            let load = &w[0];
            let compare = &w[1];
            (load.mnemonic() == Mnemonic::Mov
                && load.op0_kind() == OpKind::Register
                && load.op0_register().size() == 8
                && load.op1_kind() == OpKind::Memory
                && load.memory_index() == Register::None
                && compare.mnemonic() == Mnemonic::Cmp
                && compare.op0_kind() == OpKind::Memory
                && compare.memory_base() == load.op0_register()
                && compare.memory_size() == iced_x86::MemorySize::UInt32
                && compare.op1_kind() == OpKind::Register)
                .then(|| (load.memory_displacement64() as u32, compare.memory_displacement64() as u32))
        })
        .collect()
}

fn direct_targets(instrs: &[Instruction]) -> Vec<u32> {
    instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64).map(|i| i.near_branch_target() as u32).collect()
}

pub fn find_ground_items(image: &[u8]) -> Result<GroundItems, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let implementation = lua_binding(&pe, "objFind")?;
    let mut found = Vec::new();
    for find in direct_targets(&whole_function(&pe, implementation, 0x800)) {
        let instrs = whole_function(&pe, find, 0x200);
        let compares = id_compares(&instrs);
        for lookup in direct_targets(&instrs) {
            if let Some((items, level)) = tile_lookup(&whole_function(&pe, lookup, 0x80)) {
                found.extend(compares.iter().map(|&(node_item, id)| GroundItems { items, level, node_item, id }));
            }
        }
    }
    one(found)
}

fn pile_store(instrs: &[Instruction], decoration: &DecorationLayout) -> Option<u32> {
    let mut slots: HashMap<(u32, Register), HashSet<u32>> = HashMap::new();
    for w in instrs.windows(2) {
        if w[0].mnemonic() == Mnemonic::Lea
            && w[0].op0_register() == Register::RCX
            && w[0].memory_index() == Register::None
            && w[1].mnemonic() == Mnemonic::Call
            && w[1].op0_kind() == OpKind::NearBranch64
        {
            slots.entry((w[1].near_branch_target() as u32, w[0].memory_base())).or_default().insert(w[0].memory_displacement64() as u32);
        }
    }
    let stores = |pile: Register, field: u32| {
        instrs.iter().any(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == pile && i.memory_displacement64() == field as u64)
    };
    // the old pile's slots are destroyed three times too: keep the base that gets placed
    slots.iter().filter(|(_, offsets)| offsets.len() == 3).find(|(&(_, base), _)| stores(base, decoration.x) && stores(base, decoration.y))?;
    instrs.iter().enumerate().find_map(|(at, i)| {
        let (true, OpKind::Register, OpKind::Immediate32to64) = (i.mnemonic() == Mnemonic::Add, i.op0_kind(), i.op1_kind()) else { return None };
        let tile = i.op0_register();
        instrs[at + 1..(at + 8).min(instrs.len())]
            .iter()
            .any(|s| {
                s.mnemonic() == Mnemonic::Mov
                    && s.op0_kind() == OpKind::Memory
                    && s.memory_base() == tile
                    && s.memory_displacement64() == 0
                    && s.op1_kind() == OpKind::Register
            })
            .then(|| i.immediate(1) as u32)
    })
}

pub fn find_item_pile(image: &[u8], decoration: &DecorationLayout) -> Result<u32, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x800 {
            continue;
        }
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        found.extend(pile_store(&instrs, decoration));
    }
    one(found)
}
