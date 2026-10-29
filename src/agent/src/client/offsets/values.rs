use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemContainers {
    pub table: u32,
    pub ids: u32,
    pub quantities: u32,
    pub next: u32,
}

fn container_slot_getter(instrs: &[Instruction]) -> Option<(u32, u32, u32, u32, bool)> {
    let global = |i: &Instruction| {
        (i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 8 && i.is_ip_rel_memory_operand())
            .then(|| i.ip_rel_memory_address() as u32)
    };
    let globals: Vec<u32> = instrs.iter().take(8).filter_map(global).collect();
    let [count, buckets] = globals.as_slice() else { return None };
    if *count != buckets + 8 {
        return None;
    }
    let compares_key = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Cmp && i.op1_kind() == OpKind::Memory && i.memory_base() == Register::RAX && i.memory_displacement64() == 0
    });
    let next = instrs.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::RAX && i.memory_base() == Register::RAX && i.op1_kind() == OpKind::Memory)
            .then(|| i.memory_displacement64() as u32)
    })?;
    let vector = instrs.windows(2).find_map(|w| {
        let [begin, end] = w else { return None };
        let b = load_from(begin, Register::RAX).filter(|_| begin.op0_register().size() == 8)?;
        let e = load_from(end, Register::RAX).filter(|_| end.op0_register().size() == 8)?;
        (e == b + 8).then_some(b)
    })?;
    let miss = instrs.iter().find_map(|i| match i.mnemonic() {
        Mnemonic::Mov if i.op0_register() == Register::EAX && i.op1_kind() == OpKind::Immediate32 => Some(i.immediate(1) as u32 == u32::MAX),
        Mnemonic::Xor if i.op0_register() == Register::EAX && i.op1_register() == Register::EAX => Some(false),
        _ => None,
    })?;
    compares_key.then_some((*count, *buckets, next, vector, miss))
}

pub fn find_item_containers(pe: &Pe) -> Result<ItemContainers, ScanError> {
    const INTERFACE_THRESHOLD: u32 = -70000i32 as u32;

    let handlers: HashSet<(u32, u32)> = dword_hits(pe, INTERFACE_THRESHOLD)
        .into_iter()
        .filter_map(|hit| pe.function_containing(hit))
        .collect();
    let calls = direct_calls(pe);
    let mut getters = Vec::new();
    for &(site, target) in calls {
        if handlers.iter().any(|&(begin, end)| (begin..end).contains(&site)) {
            getters.extend(container_slot_getter(&decode_at(pe, target, 30)));
        }
    }
    let ids = one(getters.iter().filter(|g| g.4).map(|g| (g.1, g.2, g.3)).collect())?;
    let quantities = one(getters.iter().filter(|g| !g.4).map(|g| (g.1, g.2, g.3)).collect())?;
    if (ids.0, ids.1) != (quantities.0, quantities.1) {
        return Err(ScanError::Ambiguous(2));
    }
    let table = ids.0.checked_sub(hash_table::BUCKETS as u32).ok_or(ScanError::NotFound)?;
    Ok(ItemContainers { table, ids: ids.2, quantities: quantities.2, next: ids.1 })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Varcs {
    pub object: u32,
    pub begin: u32,
    pub entry_size: u32,
    pub value: u32,
    pub kind: u32,
}

pub fn find_varcs(pe: &Pe, client: u32) -> Result<Varcs, ScanError> {
    let getter = lua_binding(pe, "getIntegerVarc")?;
    let instrs = decode_at(pe, getter, 3);
    let [global, object, jmp, ..] = instrs.as_slice() else { return Err(ScanError::NotFound) };
    let reads_client = global.is_ip_rel_memory_operand() && global.ip_rel_memory_address() as u32 == client;
    let object = load_from(object, Register::RAX).filter(|_| object.op0_register() == Register::RCX);
    let (true, Some(object), Mnemonic::Jmp) = (reads_client, object, jmp.mnemonic()) else {
        return Err(ScanError::NotFound);
    };

    let lookup = decode_at(pe, jmp.near_branch_target() as u32, 24);
    let begin = lookup.iter().find_map(|i| load_from(i, Register::RCX).filter(|_| i.op0_register() == Register::RCX));
    let end = lookup.iter().find_map(|i| load_from(i, Register::RCX).filter(|_| i.op0_register() == Register::RDX));
    let entry_size = lookup.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Lea && i.memory_base() == Register::RAX && i.memory_index() == Register::None)
            .then(|| i.memory_displacement64() as u32)
    });
    let kind = lookup.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Cmp && i.memory_size() == iced_x86::MemorySize::UInt8 && i.memory_base() == Register::RAX && i.immediate(1) == 1)
            .then(|| i.memory_displacement64() as u32)
    });
    let value = lookup.iter().find_map(|i| load_from(i, Register::RAX).filter(|_| i.op0_register() == Register::EAX));
    match (begin, end, entry_size, kind, value) {
        (Some(begin), Some(end), Some(entry_size), Some(kind), Some(value)) if end == begin + 8 => {
            Ok(Varcs { object, begin, entry_size, value, kind })
        }
        _ => Err(ScanError::NotFound),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunEnergy {
    pub energy: u32,
    pub weight: u32,
}

pub fn find_run_energy(pe: &Pe) -> Result<RunEnergy, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> =
            Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        let extends = |i: &Instruction, mnemonic: Mnemonic| {
            i.mnemonic() == mnemonic && i.op1_kind() == OpKind::Register && i.op1_register() == Register::AX
        };
        for (n, instr) in instrs.iter().enumerate() {
            if !extends(instr, Mnemonic::Movzx) {
                continue;
            }
            let reg = instr.op0_register();
            let store = |i: &Instruction, reg: Register| {
                (i.mnemonic() == Mnemonic::Mov && i.op1_kind() == OpKind::Register && i.op1_register() == reg)
                    .then(|| field_operand(i, 0))
                    .flatten()
            };
            let Some(energy) = instrs[n + 1..].iter().take(3).find_map(|i| store(i, reg)) else { continue };
            let weight = instrs[n + 1..].iter().take(16).enumerate().find_map(|(k, i)| {
                extends(i, Mnemonic::Movsx).then(|| instrs.get(n + 2 + k).and_then(|next| store(next, i.op0_register())))?
            });
            if weight == Some(energy + 4) {
                found.push(RunEnergy { energy, weight: energy + 4 });
            }
        }
    }
    one(found.into_iter().map(|r| (r.energy, r.weight)).collect()).map(|(energy, weight)| RunEnergy { energy, weight })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorLists {
    pub npc_indices: u32,
    pub npc_table: u32,
    pub player_count: u32,
}

fn object_operand(instr: &Instruction, operand: u32) -> Option<u32> {
    (instr.op_kind(operand) == OpKind::Memory
        && instr.memory_index() == Register::None
        && !matches!(instr.memory_base(), Register::None | Register::RIP | Register::RSP))
        .then(|| instr.memory_displacement64() as u32)
}

pub fn find_actor_lists(pe: &Pe) -> Result<ActorLists, ScanError> {

    let npc_all = decode_at(pe, lua_binding(pe, "getNpcIdAll")?, 80);
    let counts: Vec<u32> = npc_all
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Cmp && i.op1_kind() == OpKind::Memory && i.op0_register().size() == 4)
        .filter_map(|i| object_operand(i, 1))
        .collect();
    let lists: Vec<u32> = npc_all
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_register().size() == 8)
        .filter_map(|i| object_operand(i, 1))
        .collect();
    let npc_indices = one(counts.iter().filter(|&&c| c >= 8 && lists.contains(&(c - 8))).map(|&c| c - 8).collect())?;

    let player_all = decode_at(pe, lua_binding(pe, "getPlayerIdAll")?, 30);
    let player_count = player_all
        .iter()
        .find_map(|i| (i.mnemonic() == Mnemonic::Movsxd).then(|| object_operand(i, 1)).flatten())
        .ok_or(ScanError::NotFound)?;
    let adds_list = player_all.iter().any(|i| {
        i.mnemonic() == Mnemonic::Add && matches!(i.op1_kind(), OpKind::Immediate32to64 | OpKind::Immediate32) && i.immediate(1) as u32 == player_count + 4
    });
    if !adds_list {
        return Err(ScanError::NotFound);
    }

    let npc_count = npc_indices + 8;
    let mut scores: HashMap<u32, usize> = HashMap::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> =
            Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        for (n, i) in instrs.iter().enumerate() {
            let reads_count = matches!(i.mnemonic(), Mnemonic::Mov | Mnemonic::Cmp | Mnemonic::Movsxd)
                && (object_operand(i, 1) == Some(npc_count) || object_operand(i, 0) == Some(npc_count));
            if !reads_count {
                continue;
            }
            let base = i.memory_base();
            let window = &instrs[n.saturating_sub(60)..(n + 60).min(instrs.len())];
            if !window.iter().any(|w| w.mnemonic() == Mnemonic::Div) {
                continue;
            }
            let qwords: HashSet<u32> = window
                .iter()
                .filter(|w| w.op0_kind() == OpKind::Register && w.op0_register().size() == 8 && w.memory_base() == base)
                .filter_map(|w| object_operand(w, 1))
                .collect();
            let dwords: HashSet<u32> = window
                .iter()
                .filter(|w| w.op0_kind() == OpKind::Register && w.op0_register().size() == 4 && w.memory_base() == base)
                .filter_map(|w| object_operand(w, 1))
                .collect();
            for &buckets in &qwords {
                let table = buckets.wrapping_sub(hash_table::BUCKETS as u32);
                if dwords.contains(&(buckets + 8)) && table != npc_indices - 8 && buckets != npc_indices {
                    *scores.entry(table).or_default() += 1;
                }
            }
        }
    }
    let mut ranked: Vec<(u32, usize)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let npc_table = match ranked.as_slice() {
        [] => return Err(ScanError::NotFound),
        [(table, _)] => *table,
        [(table, best), (_, second), ..] if *best >= 2 * *second => *table,
        _ => return Err(ScanError::Ambiguous(ranked.len())),
    };
    Ok(ActorLists { npc_indices, npc_table, player_count })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorNames {
    pub npc_name_override: u32,
    pub npc_definition: u32,
    pub definition_id: u32,
    pub definition_name: u32,
    pub player_name: u32,
    pub npc_animation: u32,
}

// The NPC actor's animation id, read off a lightning cloud's actor (see `injector npc-words`), not found by a rule; redo if the client changes.
pub const NPC_ANIMATION: u32 = 0x4d8;

fn npc_name_getter(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
    let flag = instrs.iter().take(12).find_map(|i| {
        (i.mnemonic() == Mnemonic::Movsx && i.memory_size() == iced_x86::MemorySize::Int8 && i.memory_base() == Register::RSI)
            .then(|| (i.memory_displacement64() as u32).checked_sub(0x17))
            .flatten()
    })?;
    instrs.windows(2).find_map(|w| {
        let [load, add] = w else { return None };
        let definition = load_from(load, Register::RSI).filter(|_| load.op0_register().size() == 8)?;
        let adds = add.mnemonic() == Mnemonic::Add
            && add.op0_kind() == OpKind::Register
            && add.op0_register() == load.op0_register()
            && matches!(add.op1_kind(), OpKind::Immediate8to64 | OpKind::Immediate32to64);
        adds.then(|| (flag, definition, add.immediate(1) as u32))
    })
}

pub fn find_actor_names(pe: &Pe) -> Result<ActorNames, ScanError> {
    let by_name = lua_binding(pe, "getNpcIdByName")?;
    let calls = direct_calls(pe);
    let (begin, end) = (by_name, by_name + 0x800);
    let getters: Vec<(u32, u32, u32)> = calls
        .iter()
        .filter(|&&(site, _)| (begin..end).contains(&site))
        .filter_map(|&(_, target)| npc_name_getter(&decode_at(pe, target, 120)))
        .collect();
    let (npc_name_override, npc_definition, definition_name) = one(getters)?;

    let mut id_reads: HashMap<u32, usize> = HashMap::new();
    let mut player_names = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> =
            Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        for (n, load) in instrs.iter().enumerate() {
            if load.mnemonic() != Mnemonic::Mov || load.op0_kind() != OpKind::Register || load.op0_register().size() != 8 {
                continue;
            }
            let Some(disp) = field_operand(load, 1) else { continue };
            let reg = load.op0_register();
            let after = &instrs[n + 1..(n + 4).min(instrs.len())];
            if disp == npc_definition {
                if let Some(read) = after.iter().find(|i| {
                    i.mnemonic() == Mnemonic::Mov && i.op0_register().size() == 4 && i.op1_kind() == OpKind::Memory && i.memory_base() == reg
                }) {
                    *id_reads.entry(read.memory_displacement64() as u32).or_default() += 1;
                }
            } else if (0x700..0x800).contains(&disp) {
                let reads_string = after.iter().take(1).any(|i| {
                    i.mnemonic() == Mnemonic::Movsx && i.memory_base() == reg && i.memory_displacement64() == 0x17
                });
                if reads_string {
                    player_names.push(disp);
                }
            }
        }
    }
    let definition_id = id_reads.into_iter().max_by_key(|&(disp, count)| (count, std::cmp::Reverse(disp))).ok_or(ScanError::NotFound)?.0;
    let player_name = one(player_names)?;
    Ok(ActorNames { npc_name_override, npc_definition, definition_id, definition_name, player_name, npc_animation: NPC_ANIMATION })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppearanceDecoder {
    pub function: u32,
    pub actor_id: u32,
}

pub fn find_appearance_decoder(pe: &Pe, player_name: u32, player_index: u32) -> Result<AppearanceDecoder, ScanError> {
    let pointer = player_name.checked_sub(8).ok_or(ScanError::NotFound)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> =
            Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        let checks = instrs
            .iter()
            .filter(|i| {
                i.mnemonic() == Mnemonic::Cmp
                    && i.op0_kind() == OpKind::Register
                    && i.op0_register().size() == 4
                    && i.op1_kind() == OpKind::Immediate32
                    && i.immediate(1) == 0xffff
            })
            .count();
        if checks < 7 {
            continue;
        }
        let Some(player) = instrs.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Lea && i.op0_register() == Register::RCX && object_operand(i, 1) == Some(pointer))
                .then(|| i.memory_base())
        }) else {
            continue;
        };
        let ids: HashSet<u32> = instrs
            .windows(3)
            .filter_map(|w| {
                let load = &w[0];
                let index = load.mnemonic() == Mnemonic::Mov
                    && load.op0_kind() == OpKind::Register
                    && load.op0_register().size() == 4
                    && object_operand(load, 1) == Some(player_index);
                if !index {
                    return None;
                }
                w[1..].iter().find_map(|cmp| {
                    (cmp.mnemonic() == Mnemonic::Cmp
                        && cmp.memory_base() == player
                        && cmp.op1_kind() == OpKind::Register
                        && cmp.op1_register() == load.op0_register())
                    .then(|| object_operand(cmp, 0))
                    .flatten()
                })
            })
            .collect();
        found.extend(ids.into_iter().map(|actor_id| (begin, actor_id)));
    }
    one(found).map(|(function, actor_id)| AppearanceDecoder { function, actor_id })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct World {
    pub containers: ItemContainers,
    pub varcs: Varcs,
    pub run_energy: RunEnergy,
    pub lists: ActorLists,
    pub names: ActorNames,
    pub player: LocalPlayer,
    pub actor: ActorPosition,
}
