use std::{
    cell::OnceCell,
    collections::{HashMap, HashSet},
};

mod camera;
mod chat;
mod ground_items;
mod item_setter;
mod items;
mod local_projection;
mod menu;
mod minimap;
mod model;
mod packets;
mod projection;
mod projectiles;
mod scene;
mod scene_loader;
mod script_runs;
mod scripts;
mod spot_anims;
mod values;
mod varc_setters;
mod world_select;
pub use values::{
    find_actor_lists, find_actor_names, find_appearance_decoder, find_item_containers, find_run_energy, find_varcs, AppearanceDecoder, ItemContainers, World,
};
pub use items::{find_game_vals, find_obj_type_lookup, CacheTable, GameVals, ObjTypeLookup};
pub use local_projection::find_local_projection;
pub use projection::{find_projection, Projection};
pub use projectiles::find_projectiles;
pub use scene_loader::find_instance_flag;
pub use scene::{
    find_collision_maps, find_decoration_layout, find_game_object_layout, find_loc_configs, find_loc_names, find_scene, loc_kind, DecorationLayout, GameObjectLayout,
    LocConfigs, LocNames, Scene,
};
pub use menu::{find_menu, find_menu_click, find_menu_layout, find_menu_open, find_selection, Menu, MenuClick, MenuLayout, Selection};
pub use camera::find_camera;
pub use chat::find_chat;
pub use packets::{find_packets, Packets, BUFFER_SIZE};
pub use item_setter::find_item_setter;
pub use varc_setters::{find_varc_setters, VarcSetters, VARIANT_KIND, VARIANT_STRING};
pub use script_runs::{find_script_runs, ScriptRuns, ARG_KIND, ARG_SIZE, KIND_INT};
pub use spot_anims::find_spot_anims;
pub use ground_items::{find_ground_items, find_item_pile};
pub use minimap::{find_destination, find_minimap};
pub use model::{render_slots, find_actor_orientation, find_animation_height, find_model_faces, find_model_vertices, find_npc_model, ActorOrientation, ModelFaces, ModelVertices, NpcModel};
pub use scripts::{find_component_fields, find_component_invalidate, find_component_sprite, ComponentInvalidate};
pub use world_select::find_world_select;

use iced_x86::{Decoder, DecoderOptions, Instruction, Mnemonic, OpKind, Register};

const GAME_STATES: [u32; 3] = [20, 25, 30];

const LOCAL_MIN: u32 = 1536;
const LOCAL_MAX: u32 = 11776;

const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const PE32_PLUS: u16 = 0x20b;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorPosition {
    pub x: u32,
    pub y: u32,
    pub function: u32,
    pub base_x: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalPlayer {
    pub client: u32,
    pub world_view_id: u32,
    pub player_index: u32,
    pub world_views: u32,
    pub players: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skills {
    pub levels: u32,
    pub real_levels: u32,
    pub xp: u32,
    pub count: u32,
}

pub use ipc::state::array;

pub mod hash_table {
    pub const BUCKETS: usize = 0x8;
    pub const BUCKET_COUNT: usize = 0x10;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    InvalidImage,
    NotFound,
    Ambiguous(usize),
}

struct Section {
    rva: usize,
    size: usize,
}

// The client image parsed once for every rule: its code sections, functions and, scanned on first use, its direct calls.
pub struct Pe<'a> {
    image: &'a [u8],
    code: Vec<Section>,
    functions: Vec<(u32, u32)>,
    raw_functions: Vec<(u32, u32, u32)>,
    calls: OnceCell<Vec<(u32, u32)>>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Pe<'a> {
    pub fn parse(image: &'a [u8]) -> Option<Self> {
        let nt = u32_at(image, 0x3c)? as usize;
        if image.get(nt..nt + 4)? != b"PE\0\0" || u16_at(image, nt + 24)? != PE32_PLUS {
            return None;
        }

        let section_count = u16_at(image, nt + 6)? as usize;
        let optional_size = u16_at(image, nt + 20)? as usize;
        let optional = nt + 24;
        let exception_rva = u32_at(image, optional + 112 + 3 * 8)? as usize;
        let exception_size = u32_at(image, optional + 112 + 3 * 8 + 4)? as usize;

        let mut code = Vec::new();
        let table = optional + optional_size;
        for i in 0..section_count {
            let header = table + i * 40;
            let size = u32_at(image, header + 8)? as usize;
            let rva = u32_at(image, header + 12)? as usize;
            let flags = u32_at(image, header + 36)?;
            if flags & IMAGE_SCN_MEM_EXECUTE != 0 && rva.checked_add(size)? <= image.len() {
                code.push(Section { rva, size });
            }
        }

        let mut functions = Vec::with_capacity(exception_size / 12);
        let mut raw_functions = Vec::with_capacity(exception_size / 12);
        let entries = image.get(exception_rva..exception_rva.checked_add(exception_size)?)?;
        for entry in entries.as_chunks::<12>().0 {
            let begin = u32::from_le_bytes(entry[0..4].try_into().ok()?);
            let end = u32::from_le_bytes(entry[4..8].try_into().ok()?);
            let unwind = u32::from_le_bytes(entry[8..12].try_into().ok()?);
            if end > begin {
                functions.push((begin, end));
                raw_functions.push((begin, end, unwind));
            }
        }
        functions.sort_unstable();
        raw_functions.sort_unstable();

        Some(Self { image, code, functions, raw_functions, calls: OnceCell::new() })
    }

    fn function_containing(&self, rva: u32) -> Option<(u32, u32)> {
        let index = self.functions.partition_point(|&(begin, _)| begin <= rva);
        let &(begin, end) = self.functions.get(index.checked_sub(1)?)?;
        (rva < end).then_some((begin, end))
    }

    fn bytes(&self, begin: u32, end: u32) -> &[u8] {
        &self.image[begin as usize..end as usize]
    }
}

fn dword_hits(pe: &Pe, value: u32) -> Vec<u32> {
    let needle = value.to_le_bytes();
    let mut hits = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(4).enumerate() {
            if window == needle {
                hits.push((section.rva + offset) as u32);
            }
        }
    }
    hits
}

type Load = (Register, i64);

fn compared_offsets(code: &[u8], rva: u32) -> (Vec<Load>, Vec<Load>) {
    let mut decoder = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE);
    let mut loaded: HashMap<Register, Load> = HashMap::new();
    let (mut mins, mut maxs) = (Vec::new(), Vec::new());

    while decoder.can_decode() {
        let instr = decoder.decode();
        if instr.is_invalid() {
            continue;
        }

        match instr.mnemonic() {
            Mnemonic::Call => loaded.clear(),
            Mnemonic::Cmp
                if instr.op0_kind() == OpKind::Register && instr.op1_kind() != OpKind::Register =>
            {
                let value = instr.immediate(1) as u32;
                let reg = instr.op0_register().full_register32();
                if let Some(&load) = loaded.get(&reg) {
                    if value == LOCAL_MIN {
                        mins.push(load);
                    } else if value == LOCAL_MAX {
                        maxs.push(load);
                    }
                }
            }
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register
                    && instr.op1_kind() == OpKind::Memory
                    && instr.memory_index() == Register::None
                    && instr.memory_base() != Register::None
                    && instr.memory_base() != Register::RIP =>
            {
                let reg = instr.op0_register().full_register32();
                loaded.insert(reg, (instr.memory_base(), instr.memory_displacement64() as i64));
            }
            _ => {
                if instr.op0_kind() == OpKind::Register {
                    loaded.remove(&instr.op0_register().full_register32());
                }
            }
        }
    }

    (mins, maxs)
}

fn position_in(mins: &[Load], maxs: &[Load]) -> Option<(u32, u32)> {
    for (i, &(base, disp)) in mins.iter().enumerate() {
        let y = (base, disp + 4);
        if disp >= 0 && mins[i + 1..].contains(&y) && maxs.contains(&(base, disp)) && maxs.contains(&y) {
            return Some((disp as u32, disp as u32 + 4));
        }
    }
    None
}

fn world_base_in(code: &[u8], rva: u32) -> Option<u32> {
    let mut subs = Vec::new();
    let mut decoder = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE);
    while decoder.can_decode() {
        let instr = decoder.decode();
        if instr.mnemonic() == Mnemonic::Sub
            && instr.op0_kind() == OpKind::Register
            && instr.op1_kind() == OpKind::Memory
            && instr.memory_base() == Register::RAX
            && instr.memory_index() == Register::None
        {
            subs.push(instr.memory_displacement64() as i64);
        }
    }

    let pairs: Vec<i64> = subs.iter().copied().filter(|&d| d >= 0 && subs.contains(&(d + 4))).collect();
    match pairs.as_slice() {
        [only] => Some(*only as u32),
        _ => None,
    }
}

pub fn find_actor_position(pe: &Pe) -> Result<ActorPosition, ScanError> {

    let mut candidates: Vec<(u32, u32)> = dword_hits(pe, LOCAL_MIN)
        .into_iter()
        .filter_map(|hit| pe.function_containing(hit))
        .collect();
    candidates.sort_unstable();
    candidates.dedup();

    let max_bytes = LOCAL_MAX.to_le_bytes();
    let mut found = Vec::new();
    for (begin, end) in candidates {
        let code = pe.bytes(begin, end);
        if !code.windows(4).any(|w| w == max_bytes) {
            continue;
        }

        let (mins, maxs) = compared_offsets(code, begin);
        if let Some((x, y)) = position_in(&mins, &maxs) {
            let Some(base_x) = world_base_in(code, begin) else {
                continue;
            };
            found.push(ActorPosition { x, y, function: begin, base_x });
        }
    }

    match found.as_slice() {
        [] => Err(ScanError::NotFound),
        [only] => Ok(*only),
        many => Err(ScanError::Ambiguous(many.len())),
    }
}

fn field_compares(code: &[u8], rva: u32) -> HashMap<Load, HashSet<u32>> {
    let mut compares: HashMap<Load, HashSet<u32>> = HashMap::new();
    let mut loaded: HashMap<Register, Load> = HashMap::new();
    let field = |instr: &Instruction| {
        (instr.memory_index() == Register::None
            && instr.memory_base() != Register::None
            && instr.memory_base() != Register::RIP)
            .then(|| (instr.memory_base(), instr.memory_displacement64() as i64))
    };

    for instr in Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE) {
        let compares_immediate = instr.mnemonic() == Mnemonic::Cmp
            && matches!(instr.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32);
        if compares_immediate {
            let compared = match instr.op0_kind() {
                OpKind::Memory => field(&instr),
                OpKind::Register => loaded.get(&instr.op0_register().full_register32()).copied(),
                _ => None,
            };
            if let Some(load) = compared {
                compares.entry(load).or_default().insert(instr.immediate(1) as u32);
            }
            continue;
        }

        match instr.mnemonic() {
            Mnemonic::Call => loaded.clear(),
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register
                    && instr.op0_register().size() == 4
                    && instr.op1_kind() == OpKind::Memory =>
            {
                let reg = instr.op0_register().full_register32();
                match field(&instr) {
                    Some(load) => loaded.insert(reg, load),
                    None => loaded.remove(&reg),
                };
            }
            _ if instr.op0_kind() == OpKind::Register => {
                loaded.remove(&instr.op0_register().full_register32());
            }
            _ => {}
        }
    }
    compares
}

pub fn find_game_state(pe: &Pe) -> Result<u32, ScanError> {

    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        for ((_, disp), values) in field_compares(pe.bytes(begin, end), begin) {
            if disp >= 0 && GAME_STATES.iter().all(|state| values.contains(state)) {
                found.push(disp as u32);
            }
        }
    }

    match found.as_slice() {
        [] => Err(ScanError::NotFound),
        [only] => Ok(*only),
        many => Err(ScanError::Ambiguous(many.len())),
    }
}

fn decode_at(pe: &Pe, rva: u32, limit: usize) -> Vec<Instruction> {
    let Some(section) = pe.code.iter().find(|s| (s.rva..s.rva + s.size).contains(&(rva as usize))) else {
        return Vec::new();
    };
    let end = (section.rva + section.size).min(rva as usize + limit * 15);
    Decoder::with_ip(64, &pe.image[rva as usize..end], rva as u64, DecoderOptions::NONE)
        .into_iter()
        .take(limit)
        .collect()
}

fn load_from(instr: &Instruction, base: Register) -> Option<u32> {
    (instr.mnemonic() == Mnemonic::Mov
        && instr.op0_kind() == OpKind::Register
        && instr.op1_kind() == OpKind::Memory
        && instr.memory_base() == base
        && instr.memory_index() == Register::None)
        .then(|| instr.memory_displacement64() as u32)
}

fn world_view_getter(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
    let [load, add, jmp, ..] = instrs else {
        return None;
    };
    let id = load_from(load, Register::RCX).filter(|_| load.op0_register() == Register::EDX)?;
    let is_add = add.mnemonic() == Mnemonic::Add
        && add.op0_register() == Register::RCX
        && matches!(add.op1_kind(), OpKind::Immediate32to64 | OpKind::Immediate8to64);
    let is_jmp = jmp.mnemonic() == Mnemonic::Jmp && jmp.op0_kind() == OpKind::NearBranch64;
    (is_add && is_jmp).then(|| (id, add.immediate(1) as u32, jmp.near_branch_target() as u32))
}

fn hash_table_offset(pe: &Pe, rva: u32) -> Option<u32> {
    let instrs = decode_at(pe, rva, 8);
    if let [add, jmp, ..] = instrs.as_slice() {
        if add.mnemonic() == Mnemonic::Add
            && add.op0_register() == Register::RCX
            && jmp.mnemonic() == Mnemonic::Jmp
            && jmp.op0_kind() == OpKind::NearBranch64
        {
            let inner = hash_table_offset(pe, jmp.near_branch_target() as u32)?;
            return (add.immediate(1) as u32).checked_add(inner);
        }
    }

    let mut buckets = HashSet::new();
    let mut counts = HashSet::new();
    for instr in &instrs {
        if let Some(disp) = load_from(instr, Register::RCX) {
            match instr.op0_register().size() {
                8 => buckets.insert(disp),
                4 => counts.insert(disp),
                _ => false,
            };
        }
        if instr.op0_kind() == OpKind::Register && instr.op0_register().full_register() == Register::RCX {
            break;
        }
    }

    let offset = |disp: &u32| disp.checked_sub(hash_table::BUCKETS as u32);
    let mut tables = buckets
        .iter()
        .filter(|&&b| counts.contains(&(b + (hash_table::BUCKET_COUNT - hash_table::BUCKETS) as u32)))
        .filter_map(offset);
    let table = tables.next()?;
    tables.next().is_none().then_some(table)
}

// Every `call rel32` into code, as (site, target), scanned once per image.
fn direct_calls<'p>(pe: &'p Pe) -> &'p [(u32, u32)] {
    pe.calls.get_or_init(|| scan_direct_calls(pe))
}

fn scan_direct_calls(pe: &Pe) -> Vec<(u32, u32)> {
    let mut calls = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(5).enumerate() {
            if window[0] != 0xe8 {
                continue;
            }
            let site = (section.rva + offset) as i64;
            let target = site + 5 + i32::from_le_bytes(window[1..5].try_into().unwrap()) as i64;
            if pe.code.iter().any(|s| (s.rva as i64..(s.rva + s.size) as i64).contains(&target)) {
                calls.push((site as u32, target as u32));
            }
        }
    }
    calls
}

fn player_getter(pe: &Pe, call: u32) -> Option<(u32, u32)> {
    let instrs = decode_at(pe, call, 10);
    let [_call, after @ ..] = instrs.as_slice() else {
        return None;
    };

    let [test, je, rest @ ..] = after else {
        return None;
    };
    if test.mnemonic() != Mnemonic::Test || test.op0_register() != Register::RAX || je.mnemonic() != Mnemonic::Je {
        return None;
    }

    let [load, mov, rest @ ..] = rest else {
        return None;
    };
    let index = load_from(load, load.memory_base())
        .filter(|_| load.op0_register() == Register::EDX && load.memory_base() != Register::RAX)?;
    if mov.mnemonic() != Mnemonic::Mov || mov.op0_register() != Register::RCX || mov.op1_register() != Register::RAX {
        return None;
    }

    let jmp = rest.iter().find(|i| i.mnemonic() == Mnemonic::Jmp)?;
    (jmp.op0_kind() == OpKind::NearBranch64).then(|| (index, jmp.near_branch_target() as u32))
}

fn client_global(pe: &Pe, offsets: &[u32]) -> Result<u32, ScanError> {
    let mut functions: Vec<(u32, u32)> = offsets
        .iter()
        .flat_map(|&offset| dword_hits(pe, offset))
        .filter_map(|hit| pe.function_containing(hit))
        .collect();
    functions.sort_unstable();
    functions.dedup();

    const VOLATILE: [Register; 7] =
        [Register::RAX, Register::RCX, Register::RDX, Register::R8, Register::R9, Register::R10, Register::R11];

    let mut globals: HashMap<u32, usize> = HashMap::new();
    for (begin, end) in functions {
        let mut decoder = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE);
        let mut loaded: HashMap<Register, u32> = HashMap::new();
        for instr in &mut decoder {
            if instr.is_invalid() {
                continue;
            }
            if instr.op_kinds().any(|kind| kind == OpKind::Memory) {
                let disp = instr.memory_displacement64() as u32;
                if let Some(&global) = loaded.get(&instr.memory_base()) {
                    if instr.memory_index() == Register::None && offsets.contains(&disp) {
                        *globals.entry(global).or_default() += 1;
                    }
                }
            }

            match instr.mnemonic() {
                Mnemonic::Call => VOLATILE.iter().for_each(|r| {
                    loaded.remove(r);
                }),
                Mnemonic::Ret | Mnemonic::Jmp => loaded.clear(),
                Mnemonic::Mov
                    if instr.op0_kind() == OpKind::Register
                        && instr.op0_register().size() == 8
                        && instr.op1_kind() == OpKind::Memory
                        && instr.is_ip_rel_memory_operand() =>
                {
                    loaded.insert(instr.op0_register(), instr.ip_rel_memory_address() as u32);
                }
                _ if instr.op0_kind() == OpKind::Register => {
                    loaded.remove(&instr.op0_register().full_register());
                }
                _ => {}
            }
        }
    }

    let mut globals = globals.into_iter();
    match (globals.next(), globals.next()) {
        (None, _) => Err(ScanError::NotFound),
        (Some((global, _)), None) => Ok(global),
        (Some(_), Some(_)) => Err(ScanError::Ambiguous(2 + globals.count())),
    }
}

pub fn find_local_player(pe: &Pe) -> Result<LocalPlayer, ScanError> {

    let calls = direct_calls(pe);
    let mut targets: Vec<u32> = calls.iter().map(|&(_, target)| target).collect();
    targets.sort_unstable();
    targets.dedup();

    let getters: Vec<(u32, (u32, u32, u32))> = targets
        .into_iter()
        .filter_map(|target| Some((target, world_view_getter(&decode_at(pe, target, 3))?)))
        .collect();
    let (getter, (world_view_id, manager, lookup)) = match getters.as_slice() {
        [] => return Err(ScanError::NotFound),
        [only] => *only,
        many => return Err(ScanError::Ambiguous(many.len())),
    };
    let world_views = manager
        .checked_add(hash_table_offset(pe, lookup).ok_or(ScanError::NotFound)?)
        .ok_or(ScanError::NotFound)?;

    let mut player = None;
    for &(site, _) in calls.iter().filter(|&&(_, target)| target == getter) {
        let Some((index, lookup)) = player_getter(pe, site) else {
            continue;
        };
        let found = (index, hash_table_offset(pe, lookup).ok_or(ScanError::NotFound)?);
        if player.is_some_and(|p| p != found) {
            return Err(ScanError::Ambiguous(2));
        }
        player = Some(found);
    }
    let (player_index, players) = player.ok_or(ScanError::NotFound)?;

    let client = client_global(pe, &[world_view_id, player_index, manager])?;

    Ok(LocalPlayer { client, world_view_id, player_index, world_views, players })
}

fn registered_code(pe: &Pe, site: u32) -> Option<u32> {
    let in_code = |rva: u32| pe.code.iter().any(|s| (s.rva..s.rva + s.size).contains(&(rva as usize)));
    let code_lea = |i: &Instruction| {
        (i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand())
            .then(|| i.ip_rel_memory_address() as u32)
            .filter(|&rva| in_code(rva))
    };
    for instr in decode_at(pe, site, 12) {
        if let Some(code) = code_lea(&instr) {
            return Some(code);
        }
        if instr.mnemonic() == Mnemonic::Call && instr.op0_kind() == OpKind::NearBranch64 {
            let callee = decode_at(pe, instr.near_branch_target() as u32, 8);
            if let Some(code) = callee.iter().find_map(code_lea) {
                return Some(code);
            }
        }
    }
    None
}

fn lua_binding(pe: &Pe, name: &str) -> Result<u32, ScanError> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(name.as_bytes());
    needle.push(0);
    let strings: HashSet<u32> = pe
        .image
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_slice())
        .map(|(at, _)| at as u32 + 1)
        .collect();

    let mut implementations = HashSet::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, w) in data.windows(7).enumerate() {
            if w[..3] != [0x48, 0x8d, 0x15] {
                continue;
            }
            let site = (section.rva + offset) as u32;
            let target = (site as i64 + 7 + i32::from_le_bytes(w[3..7].try_into().unwrap()) as i64) as u32;
            if !strings.contains(&target) {
                continue;
            }
            implementations.extend(registered_code(pe, site));
        }
    }

    let mut implementations = implementations.into_iter();
    match (implementations.next(), implementations.len()) {
        (None, _) => Err(ScanError::NotFound),
        (Some(only), 0) => Ok(only),
        (Some(_), more) => Err(ScanError::Ambiguous(1 + more)),
    }
}

fn global_field_getter(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let [global, load, ret, ..] = instrs else {
        return None;
    };
    let is_global = global.mnemonic() == Mnemonic::Mov
        && global.op0_register() == Register::RAX
        && global.op1_kind() == OpKind::Memory
        && global.is_ip_rel_memory_operand();
    let disp = load_from(load, Register::RAX).filter(|_| load.op0_register() == Register::EAX)?;
    (is_global && ret.mnemonic() == Mnemonic::Ret).then(|| (global.ip_rel_memory_address() as u32, disp))
}

pub fn find_tick_count(pe: &Pe, client: u32) -> Result<u32, ScanError> {
    let getter = lua_binding(pe, "getTickCount")?;
    match global_field_getter(&decode_at(pe, getter, 3)) {
        Some((global, disp)) if global == client => Ok(disp),
        _ => Err(ScanError::NotFound),
    }
}

pub fn find_varps(pe: &Pe) -> Result<u32, ScanError> {
    let getter = lua_binding(pe, "getVarp")?;
    let instrs = decode_at(pe, getter, 4);
    let [data, _index, load, ret, ..] = instrs.as_slice() else {
        return Err(ScanError::NotFound);
    };
    let reads_element = load.mnemonic() == Mnemonic::Mov
        && load.op0_register() == Register::EAX
        && load.memory_base() == Register::RAX
        && load.memory_index() != Register::None
        && load.memory_index_scale() == 4
        && load.memory_displacement64() == 0;
    let loads_data = data.mnemonic() == Mnemonic::Mov
        && data.op0_register() == Register::RAX
        && data.is_ip_rel_memory_operand();
    if !(reads_element && loads_data && ret.mnemonic() == Mnemonic::Ret) {
        return Err(ScanError::NotFound);
    }
    (data.ip_rel_memory_address() as u32).checked_sub(array::DATA as u32).ok_or(ScanError::NotFound)
}

fn client_array_getter(instrs: &[Instruction], client: u32) -> Option<u32> {
    let mut client_register = None;
    for instr in instrs {
        match instr.mnemonic() {
            Mnemonic::Ret => return None,
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register
                    && instr.op1_kind() == OpKind::Memory
                    && instr.is_ip_rel_memory_operand()
                    && instr.ip_rel_memory_address() as u32 == client =>
            {
                client_register = Some(instr.op0_register());
            }
            Mnemonic::Mov
                if instr.op0_register() == Register::EAX
                    && instr.op1_kind() == OpKind::Memory
                    && Some(instr.memory_base()) == client_register
                    && instr.memory_index() != Register::None
                    && instr.memory_index_scale() == 4 =>
            {
                return Some(instr.memory_displacement64() as u32);
            }
            _ => {}
        }
    }
    None
}

pub fn find_skills(pe: &Pe, client: u32) -> Result<Skills, ScanError> {
    let array = |name: &str| -> Result<u32, ScanError> {
        let getter = lua_binding(pe, name)?;
        client_array_getter(&decode_at(pe, getter, 12), client).ok_or(ScanError::NotFound)
    };
    let levels = array("getStatEffectiveLevel")?;
    let real_levels = array("getStatBaseLevel")?;
    let xp = array("getStatXP")?;

    let mut arrays = [levels, real_levels, xp];
    arrays.sort_unstable();
    let spacing = arrays[1] - arrays[0];
    let evenly_spaced = spacing == arrays[2] - arrays[1] && spacing % 4 == 0 && (20..=64).contains(&(spacing / 4));
    if !evenly_spaced {
        return Err(ScanError::NotFound);
    }
    Ok(Skills { levels, real_levels, xp, count: spacing / 4 })
}

const LOGIN_PANEL_WIDTH: u32 = 765;

const LOGIN_Y_DEFAULT: u32 = 171;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginPanel {
    pub panel_x: u32,
    pub box_x: u32,
    pub center_x: u32,
    pub y: u32,
    pub screen: u32,
}

fn login_box_writes(code: &[u8], rva: u32, panel_x: u32) -> Vec<(u32, u32)> {
    let instrs: Vec<Instruction> = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter().collect();
    let global_write = |instr: &Instruction, reg: Register| {
        (instr.mnemonic() == Mnemonic::Mov
            && instr.op0_kind() == OpKind::Memory
            && instr.is_ip_rel_memory_operand()
            && instr.op1_kind() == OpKind::Register
            && instr.op1_register() == reg)
            .then(|| instr.ip_rel_memory_address() as u32)
    };
    let add_immediate = |instr: &Instruction, reg: Register| {
        instr.mnemonic() == Mnemonic::Add
            && instr.op0_kind() == OpKind::Register
            && instr.op0_register() == reg
            && matches!(instr.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
    };

    instrs
        .windows(5)
        .filter_map(|w| {
            let [load, add_box, box_write, add_center, center_write] = w else {
                return None;
            };
            let reg = load.op0_register();
            let loads_panel_x = load.mnemonic() == Mnemonic::Mov
                && load.op0_kind() == OpKind::Register
                && reg.size() == 4
                && load.op1_kind() == OpKind::Memory
                && load.is_ip_rel_memory_operand()
                && load.ip_rel_memory_address() as u32 == panel_x;
            if !(loads_panel_x && add_immediate(add_box, reg) && add_immediate(add_center, reg)) {
                return None;
            }
            Some((global_write(box_write, reg)?, global_write(center_write, reg)?))
        })
        .collect()
}

fn login_y_write(code: &[u8], rva: u32) -> Option<u32> {
    let instrs: Vec<Instruction> = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter().collect();
    let target = instrs.windows(2).find_map(|w| {
        let [set, store] = w else {
            return None;
        };
        let sets_default = set.mnemonic() == Mnemonic::Mov
            && set.op0_kind() == OpKind::Register
            && set.op0_register().size() == 4
            && matches!(set.op1_kind(), OpKind::Immediate32)
            && set.immediate(1) as u32 == LOGIN_Y_DEFAULT;
        let stores = store.mnemonic() == Mnemonic::Mov
            && store.op0_kind() == OpKind::Memory
            && store.is_ip_rel_memory_operand()
            && store.op1_kind() == OpKind::Register
            && store.op1_register() == set.op0_register();
        (sets_default && stores).then(|| store.ip_rel_memory_address() as u32)
    })?;

    instrs.iter().enumerate().find_map(|(i, convert)| {
        if convert.mnemonic() != Mnemonic::Cvttss2si || convert.op0_register().size() != 4 {
            return None;
        }
        let reg = convert.op0_register();
        instrs[i + 1..].iter().take(3).find_map(|store| {
            let writes_global = store.mnemonic() == Mnemonic::Mov
                && store.op0_kind() == OpKind::Memory
                && store.is_ip_rel_memory_operand()
                && store.op1_kind() == OpKind::Register
                && store.op1_register() == reg;
            writes_global.then(|| store.ip_rel_memory_address() as u32).filter(|&y| y != target)
        })
    })
}

fn panel_x_write(code: &[u8], rva: u32) -> Option<u32> {
    const NEG_PANEL_WIDTH: u32 = LOGIN_PANEL_WIDTH.wrapping_neg();

    let mut decoder = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE);
    let (mut since_subtract, mut since_divide) = (None::<u32>, None::<u32>);

    while decoder.can_decode() {
        let instr = decoder.decode();
        if instr.is_invalid() {
            continue;
        }

        if let Some(n) = since_divide {
            let writes_global = instr.mnemonic() == Mnemonic::Mov
                && instr.op0_kind() == OpKind::Memory
                && instr.is_ip_rel_memory_operand()
                && instr.op1_kind() == OpKind::Register
                && instr.op1_register().size() == 4;
            if writes_global {
                return Some(instr.ip_rel_memory_address() as u32);
            }
            since_divide = (n < 20).then_some(n + 1);
        }

        if let Some(n) = since_subtract {
            let halves = matches!(instr.mnemonic(), Mnemonic::Sar | Mnemonic::Shr)
                && instr.op1_kind() == OpKind::Immediate8
                && instr.immediate(1) == 1;
            since_subtract = if halves {
                since_divide.get_or_insert(0);
                None
            } else {
                (n < 10).then_some(n + 1)
            };
        }

        let subtracts_panel_width = matches!(instr.mnemonic(), Mnemonic::Add | Mnemonic::Sub)
            && matches!(instr.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
            && match instr.mnemonic() {
                Mnemonic::Add => instr.immediate(1) as u32 == NEG_PANEL_WIDTH,
                Mnemonic::Sub => instr.immediate(1) as u32 == LOGIN_PANEL_WIDTH,
                _ => false,
            };
        if subtracts_panel_width {
            since_subtract = Some(0);
        }
    }
    None
}

fn login_state_reads(code: &[u8], rva: u32, game_state: u32) -> Vec<u32> {
    let mut decoder = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE);
    let mut loaded: HashMap<Register, Load> = HashMap::new();
    let mut gated: Option<Register> = None;
    let mut window: Option<u32> = None;
    let mut pending_global: Option<(Register, u32)> = None;
    let mut found = Vec::new();

    while decoder.can_decode() {
        let instr = decoder.decode();
        if instr.is_invalid() {
            continue;
        }

        if let Some((reg, target)) = pending_global {
            let tests_itself = instr.mnemonic() == Mnemonic::Test
                && instr.op0_kind() == OpKind::Register
                && instr.op1_kind() == OpKind::Register
                && instr.op0_register() == reg
                && instr.op1_register() == reg;
            if tests_itself {
                found.push(target);
                window = None;
            }
            pending_global = None;
        }

        if let Some(n) = window {
            let reads_global = instr.mnemonic() == Mnemonic::Mov
                && instr.op0_kind() == OpKind::Register
                && instr.op0_register().size() == 4
                && instr.op1_kind() == OpKind::Memory
                && instr.is_ip_rel_memory_operand();
            if reads_global {
                pending_global = Some((instr.op0_register(), instr.ip_rel_memory_address() as u32));
            }
            window = (n < 15).then_some(n + 1);
        }

        let reg0 = (instr.op0_kind() == OpKind::Register).then(|| instr.op0_register().full_register32());

        match instr.mnemonic() {
            Mnemonic::Call => loaded.clear(),
            Mnemonic::Sub
                if matches!(instr.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
                    && instr.immediate(1) as u32 == 10
                    && reg0.is_some_and(|reg| loaded.get(&reg).map(|&(_, disp)| disp) == Some(game_state as i64)) =>
            {
                gated = reg0;
            }
            Mnemonic::Cmp
                if matches!(instr.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
                    && instr.immediate(1) as u32 == 1
                    && reg0.is_some()
                    && gated == reg0 =>
            {
                window.get_or_insert(0);
                gated = None;
            }
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register
                    && instr.op1_kind() == OpKind::Memory
                    && instr.memory_index() == Register::None
                    && instr.memory_base() != Register::None
                    && instr.memory_base() != Register::RIP =>
            {
                let reg = instr.op0_register().full_register32();
                loaded.insert(reg, (instr.memory_base(), instr.memory_displacement64() as i64));
                gated = gated.filter(|&g| g != reg);
            }
            _ => {
                if let Some(reg) = reg0 {
                    loaded.remove(&reg);
                    gated = gated.filter(|&g| g != reg);
                }
            }
        }
    }
    found
}

pub fn find_login_panel(pe: &Pe, game_state: u32) -> Result<LoginPanel, ScanError> {

    let (mut panel_x_candidates, mut screen_candidates, mut y_candidates) = (Vec::new(), Vec::new(), Vec::new());
    for &(begin, end) in &pe.functions {
        let code = pe.bytes(begin, end);
        panel_x_candidates.extend(panel_x_write(code, begin));
        screen_candidates.extend(login_state_reads(code, begin, game_state));
        y_candidates.extend(login_y_write(code, begin));
    }

    let panel_x = one(panel_x_candidates)?;
    let box_candidates = pe
        .functions
        .iter()
        .flat_map(|&(begin, end)| login_box_writes(pe.bytes(begin, end), begin, panel_x))
        .collect();
    let (box_x, center_x) = one(box_candidates)?;

    Ok(LoginPanel { panel_x, box_x, center_x, y: one(y_candidates)?, screen: one(screen_candidates)? })
}

fn one<T: Ord + Copy>(mut candidates: Vec<T>) -> Result<T, ScanError> {
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [] => Err(ScanError::NotFound),
        [only] => Ok(*only),
        many => Err(ScanError::Ambiguous(many.len())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interfaces {
    pub root: u32,
    pub manager: u32,
    pub groups: u32,
    pub sub_interfaces: u32,
    pub component: ComponentLayout,
    pub canvas: u32,
    pub ui_width: u32,
    pub ui_scale: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentLayout {
    pub id: u32,
    pub kind: u32,
    pub parent: u32,
    pub x: u32,
    pub width: u32,
    pub scroll_x: u32,
}

fn field_operand(instr: &Instruction, operand: u32) -> Option<u32> {
    (instr.op_kind(operand) == OpKind::Memory
        && instr.memory_index() == Register::None
        && !matches!(instr.memory_base(), Register::None | Register::RIP | Register::RSP | Register::RBP))
        .then(|| instr.memory_displacement64() as u32)
}

fn component_layout_function(code: &[u8], rva: u32) -> Option<(u32, u32, u32)> {
    let instrs: Vec<Instruction> = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter().collect();
    let (mut scaled, mut subtracted, mut stored) = (Vec::new(), Vec::new(), Vec::new());
    for (i, instr) in instrs.iter().enumerate() {
        match instr.mnemonic() {
            Mnemonic::Imul if instr.op_count() == 2 && instr.op0_register().size() == 4 => {
                let shifted = instrs.get(i + 1).is_some_and(|next| {
                    next.mnemonic() == Mnemonic::Sar
                        && next.op0_kind() == OpKind::Register
                        && next.op0_register() == instr.op0_register()
                        && next.op1_kind() == OpKind::Immediate8
                        && next.immediate(1) == 14
                });
                if let Some(disp) = field_operand(instr, 1).filter(|_| shifted) {
                    scaled.push(disp);
                }
            }
            Mnemonic::Sub if instr.op0_kind() == OpKind::Register && instr.op0_register().size() == 4 => {
                subtracted.extend(field_operand(instr, 1));
            }
            Mnemonic::Mov if instr.op1_kind() == OpKind::Register && instr.op1_register().size() == 4 => {
                stored.extend(field_operand(instr, 0));
            }
            _ => {}
        }
    }
    let pair = |mut values: Vec<u32>| {
        values.sort_unstable();
        values.dedup();
        match values.as_slice() {
            &[a, b] if b == a + 4 => Some(a),
            _ => None,
        }
    };
    let original_x = pair(scaled)?;
    let x = pair(stored)?;
    let width = pair(subtracted.into_iter().filter(|&d| d != original_x && d != original_x + 4).collect())?;
    (x != width && x != original_x).then_some((original_x, x, width))
}

fn component_tree_loop(code: &[u8], rva: u32, width: u32) -> Option<(u32, u32)> {
    let instrs: Vec<Instruction> = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter().collect();
    let scroll = |w: u32| {
        instrs.windows(3).find_map(|win| {
            let [load, sub, cmp] = win else { return None };
            let reg = load.op0_register();
            let ok = load.mnemonic() == Mnemonic::Mov
                && load.op0_kind() == OpKind::Register
                && reg.size() == 4
                && field_operand(load, 1).is_some()
                && sub.mnemonic() == Mnemonic::Sub
                && sub.op0_kind() == OpKind::Register
                && sub.op0_register() == reg
                && field_operand(sub, 1) == Some(w)
                && cmp.mnemonic() == Mnemonic::Cmp
                && cmp.op1_kind() == OpKind::Register
                && cmp.op1_register() == reg;
            ok.then(|| field_operand(cmp, 0)).flatten()
        })
    };
    let scroll_x = scroll(width)?;
    (scroll(width + 4)? == scroll_x + 4).then_some(())?;

    let parent = instrs.iter().find_map(|instr| {
        (instr.mnemonic() == Mnemonic::Cmp && instr.op1_kind() == OpKind::Register && instr.op1_register().size() == 4)
            .then(|| field_operand(instr, 0))
            .flatten()
            .filter(|&d| d != scroll_x && d != scroll_x + 4)
    })?;
    Some((parent, scroll_x))
}

fn sub_interface_lookup(code: &[u8], rva: u32) -> Option<(u32, u32, u32)> {
    let instrs: Vec<Instruction> = Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter().collect();
    instrs.windows(9).find_map(|win| {
        let [cmp, jne, load_id, rest @ ..] = win else { return None };
        let kind = (cmp.mnemonic() == Mnemonic::Cmp
            && matches!(cmp.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
            && cmp.immediate(1) == 0)
            .then(|| field_operand(cmp, 0))
            .flatten()?;
        if jne.mnemonic() != Mnemonic::Jne || load_id.mnemonic() != Mnemonic::Movsxd {
            return None;
        }
        let id = field_operand(load_id, 1)?;
        if !rest.iter().any(|i| i.mnemonic() == Mnemonic::Div) {
            return None;
        }
        let loads: Vec<(u32, u32)> = rest
            .iter()
            .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register)
            .filter_map(|i| Some((i.op0_register().size() as u32, field_operand(i, 1)?)))
            .collect();
        let buckets = loads.iter().find(|&&(size, d)| size == 8 && loads.contains(&(4, d + 8)))?.1;
        Some((kind, id, buckets.checked_sub(hash_table::BUCKETS as u32)?))
    })
}

fn pointer_getter(instrs: &[Instruction]) -> Option<u32> {
    let [load, ret, ..] = instrs else { return None };
    (load_from(load, Register::RCX).is_some() && load.op0_register() == Register::RAX && ret.mnemonic() == Mnemonic::Ret)
        .then(|| load.memory_displacement64() as u32)
}

fn group_getter(instrs: &[Instruction]) -> Option<u32> {
    let [index, triple, load, scale, ret, ..] = instrs else { return None };
    let ok = index.mnemonic() == Mnemonic::Movsxd
        && triple.mnemonic() == Mnemonic::Lea
        && triple.memory_index_scale() == 2
        && scale.mnemonic() == Mnemonic::Lea
        && scale.memory_index_scale() == 8
        && ret.mnemonic() == Mnemonic::Ret
        && load.op0_register() == Register::RAX;
    ok.then(|| load_from(load, Register::RCX)).flatten()
}

pub fn find_interfaces(pe: &Pe) -> Result<Interfaces, ScanError> {

    let mut layouts = Vec::new();
    let mut lookups = Vec::new();
    for &(begin, end) in &pe.functions {
        let code = pe.bytes(begin, end);
        if let Some(found) = component_layout_function(code, begin) {
            layouts.push((begin, found));
        }
        lookups.extend(sub_interface_lookup(code, begin));
    }
    let (layout_rva, (_, x, width)) = one(layouts)?;
    let (kind, id, sub_interfaces) = one(lookups)?;

    let calls = direct_calls(pe);
    let callers_of = |target: u32| -> Vec<(u32, (u32, u32))> {
        calls
            .iter()
            .filter(|&&(_, t)| t == target)
            .filter_map(|&(site, _)| Some((site, pe.function_containing(site)?)))
            .collect()
    };

    let mut loops = Vec::new();
    for (_, (begin, end)) in callers_of(layout_rva) {
        if let Some(found) = component_tree_loop(pe.bytes(begin, end), begin, width) {
            loops.push((begin, found));
        }
    }
    let (loop_rva, (parent, scroll_x)) = one(loops)?;

    let loop_entries: HashSet<u32> =
        calls.iter().map(|&(_, t)| t).filter(|&t| t <= loop_rva && loop_rva - t <= 0x20).collect();
    let mut roots = Vec::new();
    let loop_callers: Vec<(u32, (u32, u32))> = loop_entries.iter().flat_map(|&entry| callers_of(entry)).collect();
    for (_, (begin, end)) in loop_callers {
        let targets: Vec<u32> = calls
            .iter()
            .filter(|&&(site, _)| (begin..end).contains(&site))
            .map(|&(_, target)| target)
            .collect();
        let manager = targets.iter().find_map(|&t| pointer_getter(&decode_at(pe, t, 2)));
        let groups = targets.iter().find_map(|&t| group_getter(&decode_at(pe, t, 5)));
        if let (Some(manager), Some(groups)) = (manager, groups) {
            roots.push((begin, manager, groups));
        }
    }
    let (resize_root, manager, groups) = one(roots)?;

    let mut root_candidates = Vec::new();
    for &(site, _) in calls.iter().filter(|&&(_, t)| t == resize_root) {
        let before = (8..=40).rev().find_map(|back| {
            let instrs = decode_at(pe, site.saturating_sub(back), 16);
            instrs.iter().any(|i| i.ip() as u32 == site).then_some(instrs)
        });
        let Some(before) = before else { continue };
        let before: Vec<&Instruction> = before.iter().filter(|i| (i.ip() as u32) < site).collect();
        let root = before.iter().rev().take(4).find_map(|i| {
            (i.op0_kind() == OpKind::Register && i.op0_register() == Register::EDX)
                .then(|| load_from(i, Register::RCX))
                .flatten()
                .filter(|&d| d >= 0x1000)
        });
        let canvas = before.iter().find_map(|i| {
            (i.op0_kind() == OpKind::Register && i.op0_register() == Register::RAX)
                .then(|| load_from(i, Register::RCX))
                .flatten()
        });
        let ui_width = before.iter().find_map(|i| {
            (i.op0_kind() == OpKind::Register && i.op0_register() == Register::R8D)
                .then(|| load_from(i, Register::RAX))
                .flatten()
        });
        if let (Some(root), Some(canvas), Some(ui_width)) = (root, canvas, ui_width) {
            root_candidates.push((root, canvas, ui_width));
        }
    }
    let (root, canvas, ui_width) = one(root_candidates)?;

    let mut targets: Vec<u32> = calls.iter().map(|&(_, t)| t).collect();
    targets.sort_unstable();
    targets.dedup();
    let scales = targets.into_iter().filter_map(|t| {
        let instrs = decode_at(pe, t, 6);
        let loads_canvas = instrs.first().is_some_and(|i| {
            i.op0_kind() == OpKind::Register && i.op0_register() == Register::RAX && load_from(i, Register::RCX) == Some(canvas)
        });
        loads_canvas.then(|| {
            instrs.iter().take(4).find_map(|i| {
                (i.mnemonic() == Mnemonic::Divss && i.op1_kind() == OpKind::Memory && i.memory_base() == Register::RAX)
                    .then(|| i.memory_displacement64() as u32)
            })
        })?
    });
    let ui_scale = one(scales.collect())?;

    Ok(Interfaces {
        root,
        manager,
        groups,
        sub_interfaces,
        component: ComponentLayout { id, kind, parent, x, width, scroll_x },
        canvas,
        ui_width,
        ui_scale,
    })
}

#[cfg(test)]
mod tests {
    use super::spot_anims::SpotAnims;
    use super::values::{ActorLists, ActorNames, AppearanceDecoder, ItemContainers, RunEnergy, Varcs};
    use super::*;

    pub(super) fn map_image(file: &[u8]) -> Vec<u8> {
        let nt = u32_at(file, 0x3c).unwrap() as usize;
        let optional = nt + 24;
        let size_of_image = u32_at(file, optional + 56).unwrap() as usize;
        let size_of_headers = u32_at(file, optional + 60).unwrap() as usize;
        let sections = u16_at(file, nt + 6).unwrap() as usize;
        let table = optional + u16_at(file, nt + 20).unwrap() as usize;

        let mut image = vec![0u8; size_of_image];
        image[..size_of_headers].copy_from_slice(&file[..size_of_headers]);
        for i in 0..sections {
            let header = table + i * 40;
            let rva = u32_at(file, header + 12).unwrap() as usize;
            let raw_size = u32_at(file, header + 16).unwrap() as usize;
            let raw = u32_at(file, header + 20).unwrap() as usize;
            image[rva..rva + raw_size].copy_from_slice(&file[raw..raw + raw_size]);
        }
        image
    }

    #[test]
    fn matches_player_update_pattern() {
        let code = [
            0x8b, 0x83, 0x68, 0x02, 0x00, 0x00, 0x3d, 0x00, 0x06, 0x00, 0x00, 0x7c, 0x1d, 0x8b,
            0x8b, 0x6c, 0x02, 0x00, 0x00, 0x81, 0xf9, 0x00, 0x06, 0x00, 0x00, 0x7c, 0x0f, 0x3d,
            0x00, 0x2e, 0x00, 0x00, 0x7d, 0x08, 0x81, 0xf9, 0x00, 0x2e, 0x00, 0x00, 0x7c, 0x28,
            0xc3,
        ];
        let (mins, maxs) = compared_offsets(&code, 0x1000);
        assert_eq!(position_in(&mins, &maxs), Some((0x268, 0x26c)));
    }

    #[test]
    fn matches_main_loop_game_state_branches() {
        let code = [
            0x8b, 0x8b, 0x60, 0x21, 0x00, 0x00, 0x83, 0xf9, 0x14, 0x7e, 0x10, 0x81, 0xf9, 0xe8,
            0x03, 0x00, 0x00, 0x74, 0x08, 0x83, 0xf9, 0x19, 0x75, 0x03, 0x83, 0xbb, 0x60, 0x21,
            0x00, 0x00, 0x1e, 0xc3,
        ];
        let compares = field_compares(&code, 0x1000);
        assert_eq!(
            compares[&(Register::RBX, 0x2160)],
            HashSet::from([20, 1000, 25, 30])
        );
    }

    #[test]
    fn matches_login_panel_x_computation() {
        let code = [0x05, 0x03, 0xfd, 0xff, 0xff, 0xd1, 0xf8, 0x89, 0x05, 0x00, 0x01, 0x00, 0x00];
        assert_eq!(panel_x_write(&code, 0x1000), Some(0x1000 + 13 + 0x100));
    }

    #[test]
    fn matches_login_state_selector() {
        let code = [
            0x8b, 0x83, 0x60, 0x21, 0x00, 0x00, 0x83, 0xe8, 0x0a, 0x83, 0xf8, 0x01, 0x8b, 0x53,
            0x10, 0x8b, 0x0d, 0x50, 0x00, 0x00, 0x00, 0x85, 0xc9,
        ];
        assert_eq!(login_state_reads(&code, 0x1000, 0x2160), vec![0x1000 + 21 + 0x50]);
    }

    #[test]
    fn login_state_selector_ignores_an_unrelated_gate() {
        let code = [
            0x8b, 0x83, 0x10, 0x00, 0x00, 0x00, 0x83, 0xe8, 0x0a, 0x83, 0xf8, 0x01, 0x8b, 0x0d,
            0x50, 0x00, 0x00, 0x00, 0x85, 0xc9,
        ];
        assert_eq!(login_state_reads(&code, 0x1000, 0x2160), Vec::<u32>::new());
    }

    #[test]
    fn matches_login_box_derivation() {
        let code = [
            0x8b, 0x05, 0x00, 0x01, 0x00, 0x00, 0x05, 0xca, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00,
            0x02, 0x00, 0x00, 0x05, 0xb4, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00, 0x03, 0x00, 0x00,
        ];
        assert_eq!(login_box_writes(&code, 0x1000, 0x1106), vec![(0x1211, 0x131c)]);
        assert_eq!(login_box_writes(&code, 0x1000, 0x1107), vec![]);
    }

    #[test]
    fn matches_login_y_easing() {
        let code = [
            0xb8, 0xab, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00, 0x01, 0x00, 0x00, 0xf3, 0x0f, 0x2c,
            0xda, 0xf3, 0x0f, 0x11, 0x15, 0x50, 0x00, 0x00, 0x00, 0x89, 0x1d, 0x00, 0x02, 0x00,
            0x00,
        ];
        assert_eq!(login_y_write(&code, 0x1000), Some(0x121d));
        assert_eq!(login_y_write(&code[11..], 0x100b), None);
    }

    #[test]
    fn ignores_unrelated_compares() {
        let code = [
            0x8b, 0x83, 0x68, 0x02, 0x00, 0x00, 0x3d, 0x00, 0x06, 0x00, 0x00, 0x8b, 0x8b, 0x00,
            0x03, 0x00, 0x00, 0x81, 0xf9, 0x00, 0x06, 0x00, 0x00, 0xc3,
        ];
        let (mins, maxs) = compared_offsets(&code, 0x1000);
        assert_eq!(position_in(&mins, &maxs), None);
    }

    #[test]
    fn finds_offsets_in_installed_client() {
        let path = std::env::var_os("GAME_CLIENT_EXE")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                Some(std::path::Path::new(&home).join(".local/share/bolt-launcher/osrswindows.exe"))
            })
            .unwrap();
        let Ok(file) = std::fs::read(&path) else {
            eprintln!("skipped, cannot read {}", path.display());
            return;
        };

        let image = map_image(&file);
        let pe = Pe::parse(&image).unwrap();
        let position = find_actor_position(&pe).expect("actor position not found");
        println!("{position:x?}");
        assert_eq!(position.y, position.x + 4);

        let player = find_local_player(&pe).expect("local player not found");
        println!("{player:x?}");
        assert_ne!(player.world_view_id, player.player_index);

        let game_state = find_game_state(&pe).expect("game state not found");
        println!("game state +{game_state:#x}");

        let skills = find_skills(&pe, player.client).expect("skills not found");
        println!("{skills:x?}");
        assert_eq!(skills.count, 25);

        let tick_count = find_tick_count(&pe, player.client).expect("tick count not found");
        println!("tick count +{tick_count:#x}");
        let varps = find_varps(&pe).expect("varps not found");
        println!("varps RVA {varps:#x}");

        let interfaces = find_interfaces(&pe).expect("interfaces not found");
        println!("{interfaces:x?}");
        assert_eq!(
            interfaces,
            Interfaces {
                root: 0x413bc8,
                manager: 0x413bd8,
                groups: 0x6890,
                sub_interfaces: 0x413b98,
                component: ComponentLayout { id: 0x4, kind: 0x28, parent: 0x74, x: 0x5c, width: 0x64, scroll_x: 0x84 },
                canvas: 0x90,
                ui_width: 0x6c,
                ui_scale: 0x68,
            }
        );

        let containers = find_item_containers(&pe).expect("item containers not found");
        println!("{containers:x?}");
        assert_eq!(containers, ItemContainers { table: 0x154fae0, ids: 0x8, quantities: 0x20, next: 0x38 });
        let varcs = find_varcs(&pe, player.client).expect("varcs not found");
        println!("{varcs:x?}");
        assert_eq!(varcs, Varcs { object: 0x417ca8, begin: 0x48, entry_size: 0x30, value: 0x8, kind: 0x28 });
        let energy = find_run_energy(&pe).expect("run energy not found");
        println!("{energy:x?}");
        assert_eq!(energy, RunEnergy { energy: 0x34e0, weight: 0x34e4 });
        let actors = find_actor_lists(&pe).expect("actor lists not found");
        println!("{actors:x?}");
        assert_eq!(actors, ActorLists { npc_indices: 0xd0, npc_table: 0x90, player_count: 0xccd0 });
        let names = find_actor_names(&pe).expect("actor names not found");
        println!("{names:x?}");
        assert_eq!(
            names,
            ActorNames { npc_name_override: 0x710, npc_definition: 0x730, definition_id: 0, definition_name: 0x8, player_name: 0x718, npc_animation: 0x4d8 }
        );
        let decoder = find_appearance_decoder(&pe, names.player_name, player.player_index).expect("appearance decoder not found");
        println!("{decoder:x?}");
        assert_eq!(decoder, AppearanceDecoder { function: 0xa1420, actor_id: 0x79c });
        let invalidate = find_component_invalidate(&pe).expect("component invalidation not found");
        println!("{invalidate:x?}");
        assert_eq!(invalidate, ComponentInvalidate { function: 0x7c270, holder_component: 0x8, text: 0x158 });
        let fields = find_component_fields(&pe).expect("component fields not found");
        println!("{fields:x?}");
        assert_eq!(fields, scripts::ComponentFields { hidden: 0x78, text_color: 0x98, scroll_width: 0x8c, index: 0x8, children: 0xb50 });
        let sprite = find_component_sprite(&pe).expect("component sprite not found");
        println!("component sprite +{sprite:#x}");
        assert_eq!(sprite, 0xc8);
        let lookup = find_obj_type_lookup(&pe).expect("obj type lookup not found");
        println!("{lookup:x?}");
        assert_eq!(lookup, ObjTypeLookup { function: 0x5f5e20, name: 0x8, desc: 0x20 });
        let game_vals = find_game_vals(&pe, player.client, &lookup).expect("game vals not found");
        println!("{game_vals:x?}");
        assert_eq!(
            game_vals,
            GameVals {
                index: 0x2490,
                reader: 0x625ba0,
                table: CacheTable {
                    archive_count: 0x28,
                    archives: 0x30,
                    archive_stride: 0x70,
                    archive_missing: 0x4,
                    file_count: 0x40,
                    files: 0x48,
                    file_stride: 0x20,
                    file_missing: 0x8,
                },
            }
        );
        let projection = find_projection(&pe, player.client).expect("projection not found");
        println!("{projection:x?}");
        let menu = find_menu(&pe).expect("menu not found");
        println!("{menu:x?}");
        assert_eq!(menu.object, 0x4133f0);
        assert_eq!((menu.entry_size, menu.target, menu.option, menu.ids, menu.priority, menu.kind, menu.cancel), (0x150, 0x50, 0xb8, 0x128, 0x134, 0x146, 0x14d));
        let click = find_menu_click(&pe, &menu).expect("menu click not found");
        println!("{click:x?}");
        assert_eq!(click, MenuClick { minimenu_vtable: 0xbcc438, entry: 0x298, click_x: 0x3e8, counter: 0x3f4 });
        assert_eq!(find_menu_open(&pe, &click), Ok(0x7c0));
        assert_eq!(find_menu_layout(&pe, &click), Ok(MenuLayout { view: 0x158, function: 0x612770, rows: 0x140, margin: 10, path: 0x10, open_row: 0x58, submenu: 0x10 }));
        let selection = find_selection(&pe, &menu).expect("selection not found");
        println!("{selection:x?}");
        assert_eq!(selection, Selection { offset: 0x7d0, verb: 0, name: 0x48, id: 0x90, child: 0x94, item: 0x98, flags: 0x9e, selected: 0x9f });
        assert_eq!(projection, Projection { world_to_screen: 0x21ec80, tile_height: 0x97ad0, top_world_view: 0xca80, plane: 0x18 });
        let minimap = find_minimap(&pe, player.client).expect("minimap not found");
        println!("{minimap:x?}");
        assert_eq!(minimap, minimap::Minimap { object: 0x40eab0, zoom: 0x6470, angle: 0x895e0, focal_x: 0x8964c, focal_y: 0x89654 });
        assert_eq!(find_destination(&pe, minimap.object), Ok(0));
        let chat = find_chat(&pe);
        println!("{chat:x?}");
        assert_eq!(
            chat,
            Ok(chat::Chat { messages: 0x154fa48, cycle: 0xe98c14, message_cycle: 0x8, kind: 0x40, name: 0x48, sender: 0x70, text: 0xa0, time_utc: 0x28, time_local: 0x10 })
        );
        let projectiles = find_projectiles(&pe).expect("projectiles not found");
        println!("{projectiles:x?}");
        assert_eq!((projectiles.list, projectiles.node, projectiles.cycle), (0xcc70, 0x18, 0xe98c14));
        assert_eq!((projectiles.source_level, projectiles.target_actor, projectiles.start_cycle, projectiles.end_cycle, projectiles.id), (0x260, 0x284, 0x288, 0x28c, 0x298));
        assert_eq!((projectiles.x, projectiles.z, projectiles.y, projectiles.orientation, projectiles.moving), (0x2a0, 0x2a8, 0x2b0, 0x2b8, 0x2c0));
        let spot_anims = find_spot_anims(&pe).expect("spot animations not found");
        println!("{spot_anims:x?}");
        assert_eq!(
            spot_anims,
            SpotAnims { list: 0x160, cycle: 0xe98c14, start_cycle: 0x260, level: 0x264, x: 0x268, y: 0x26c, z: 0x270, id: 0x274 }
        );
        let ground_items = find_ground_items(&pe);
        println!("{ground_items:x?}");
        assert_eq!(ground_items, Ok(ground_items::GroundItems { items: 0x140, level: 0x10, node_item: 0x18, id: 0x20 }));
        let camera = find_camera(&pe, player.client).expect("camera not found");
        println!("{camera:x?}");
        assert_eq!(camera, camera::Camera { position: 0x895c8, pitch: 0x895dc, yaw: 0x895e0 });
        let scene = find_scene(&pe).expect("scene not found");
        let game = find_game_object_layout(&pe, &scene).expect("game object layout not found");
        assert_eq!(find_loc_configs(&pe, &scene, &game), Ok(scene::LocConfigs { wall: 0x48, wall_decoration: 0x428, ground_decoration: 0x228 }));
        let decoration = find_decoration_layout(&pe, &scene).expect("decoration layout not found");
        assert_eq!(find_item_pile(&pe, &decoration), Ok(0x120));
        let local = find_local_projection(&pe, player.client, interfaces.canvas, &projection, &scene).expect("local projection not found");
        println!("{local:x?}");
        assert_eq!(
            local,
            local_projection::LocalProjection {
                zoom: 0x4830,
                view_holder: 0x1b0,
                view: 0x8,
                view_center: 0x14,
                view_offset_x: 0x58,
                view_offset_y: 0x50,
                trig: 0x418,
                canvas_size: 0x30,
                ui_size: 0x6c,
                heights: 0x3d0,
                settings: 0x400,
                tile_x: 0x20,
                tile_model: 0x130,
                tile_paint: 0x128,
                model_vertices: 0x8,
                model_triangles: 0x50,
                decoration_height: 0x134,
            }
        );

        let login_panel = find_login_panel(&pe, game_state).expect("login panel not found");
        println!("{login_panel:x?}");
        assert_eq!(
            login_panel,
            LoginPanel { panel_x: 0xe98bd8, box_x: 0x154fc64, center_x: 0xe98bdc, y: 0xcfb094, screen: 0xe98c10 }
        );

        let world_select = find_world_select(&pe, player.client).expect("world select not found");
        println!("{world_select:x?}");
        assert_eq!(
            world_select,
            world_select::WorldSelect {
                open: 0xe98bd2,
                count: 0xe98c00,
                worlds: 0x154fbb8,
                offset: 0xe98b74,
                max_offset: 0xe98b78,
                hovered: 0xcfb060,
                current_world: 0x32b4,
                background: 0x154fc18,
                background_height: 0x1c,
                world: world_select::WorldFields { id: 0, types: 0x4, players: 0x8, activity: 0x28, location: 0x40 },
            }
        );

        let logged_in = decode_at(&pe, lua_binding(&pe, "isLoggedIn").unwrap(), 6);
        let state_compare = logged_in
            .iter()
            .find(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.immediate(1) == 30)
            .expect("isLoggedIn doesn't compare with 30");
        assert_eq!(state_compare.memory_displacement64() as u32, game_state);
    }

    #[test]
    fn finds_instance_flag_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let flag = find_instance_flag(&pe).unwrap_or_else(|error| panic!("instance flag not found in {}: {error:?}", build.display()));
            println!("{}: instance flag {flag:x}", build.display());
        }
    }

    #[test]
    fn finds_packets_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let packets = find_packets(&pe).unwrap_or_else(|error| panic!("packets not found in {}: {error:?}", build.display()));
            println!("{}: {packets:x?}", build.display());
            // the layout is the same in every build; the addresses move with the code
            let name = build.file_name().unwrap().to_string_lossy();
            let (receive, send) = if name.contains("166231c8") { (0x202740, 0x204010) } else { (0x202840, 0x204110) };
            assert_eq!(
                packets,
                Packets { receive, receive_opcode: 0, receive_buffer: 8, send, send_type: 0, send_buffer: 8, buffer_data: 0x10, buffer_cursor: 0x18 },
                "{}",
                build.display()
            );
        }
    }

    #[test]
    fn finds_script_runs_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let runs = find_script_runs(&pe).unwrap_or_else(|error| panic!("script runs not found in {}: {error:?}", build.display()));
            println!("{}: {runs:x?}", build.display());
            // the layout is the same in every build; the address moves with the code
            let name = build.file_name().unwrap().to_string_lossy();
            let function = if name.contains("a2636a01") { 0x1995c0 } else { runs.function };
            assert_eq!(runs, ScriptRuns { function, event_type: 0x8c, args: 0x10, arg_count: 8 }, "{}", build.display());
        }
    }

    #[test]
    fn finds_varc_setters_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let setters = find_varc_setters(&pe).unwrap_or_else(|error| panic!("varc setters not found in {}: {error:?}", build.display()));
            println!("{}: {setters:x?}", build.display());
            if build.to_string_lossy().contains("a2636a01") {
                assert_eq!(setters, VarcSetters { get_int: 0x33220, set_int: 0x33130, set_object: 0x333f0 }, "{}", build.display());
            }
        }
    }

    #[test]
    fn finds_item_setter_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let containers = find_item_containers(&pe).unwrap_or_else(|error| panic!("item containers not found in {}: {error:?}", build.display()));
            let setter = find_item_setter(&pe, &containers).unwrap_or_else(|error| panic!("item setter not found in {}: {error:?}", build.display()));
            println!("{}: item setter {setter:#x}", build.display());
            if build.to_string_lossy().contains("a2636a01") {
                assert_eq!(setter, 0x30f60, "{}", build.display());
            }
        }
    }

    #[test]
    fn finds_login_panel_in_archived_builds() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') { format!("Z:{manifest}") } else { manifest.into() };
        let archive = std::path::Path::new(&manifest).join("../../archive");
        let Ok(entries) = std::fs::read_dir(&archive) else {
            eprintln!("skipped, no {}", archive.display());
            return;
        };
        let mut builds: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "exe"))
            .collect();
        builds.sort();
        for build in builds {
            let image = map_image(&std::fs::read(&build).unwrap());
            let pe = Pe::parse(&image).unwrap();
            let game_state = find_game_state(&pe).expect("game state not found");
            let panel = find_login_panel(&pe, game_state)
                .unwrap_or_else(|error| panic!("login panel not found in {}: {error:?}", build.display()));
            println!("{}: {panel:x?}", build.display());
            let interfaces = find_interfaces(&pe)
                .unwrap_or_else(|error| panic!("interfaces not found in {}: {error:?}", build.display()));
            println!("{}: {interfaces:x?}", build.display());
            let player = find_local_player(&pe).unwrap();
            println!(
                "{}: {:x?} {:x?} {:x?} {:x?}",
                build.display(),
                find_item_containers(&pe),
                find_varcs(&pe, player.client),
                find_run_energy(&pe),
                find_actor_lists(&pe)
            );
            let names = find_actor_names(&pe);
            println!("{}: {names:x?}", build.display());
            println!("{}: {:x?}", build.display(), find_component_invalidate(&pe));
            println!("{}: sprite {:x?}", build.display(), find_component_sprite(&pe));
            println!("{}: {:x?}", build.display(), find_component_fields(&pe));
            let lookup = find_obj_type_lookup(&pe);
            println!("{}: {lookup:x?}", build.display());
            println!("{}: {:x?}", build.display(), find_projection(&pe, player.client));
            let menu = find_menu(&pe);
            println!("{}: {menu:x?}", build.display());
            if let Ok(menu) = menu {
                let click = find_menu_click(&pe, &menu);
                println!("{}: {click:x?}", build.display());
                if let Ok(click) = click {
                    println!("{}: menu layout {:x?}", build.display(), find_menu_layout(&pe, &click));
                }
                println!("{}: {:x?}", build.display(), find_selection(&pe, &menu));
            }
            let minimap = find_minimap(&pe, player.client)
                .unwrap_or_else(|error| panic!("minimap not found in {}: {error:?}", build.display()));
            println!("{}: {minimap:x?}", build.display());
            println!("{}: destination {:x?}", build.display(), find_destination(&pe, minimap.object));
            println!("{}: {:x?}", build.display(), find_ground_items(&pe));
            println!("{}: {:x?}", build.display(), find_projectiles(&pe).unwrap_or_else(|error| panic!("projectiles not found in {}: {error:?}", build.display())));
            println!("{}: {:x?}", build.display(), find_spot_anims(&pe).unwrap_or_else(|error| panic!("spot animations not found in {}: {error:?}", build.display())));
            println!("{}: {:x?}", build.display(), find_chat(&pe).unwrap_or_else(|error| panic!("chat not found in {}: {error:?}", build.display())));
            let camera = find_camera(&pe, player.client)
                .unwrap_or_else(|error| panic!("camera not found in {}: {error:?}", build.display()));
            println!("{}: {camera:x?}", build.display());
            assert_eq!(camera.yaw, minimap.angle);
            let world_select = find_world_select(&pe, player.client)
                .unwrap_or_else(|error| panic!("world select not found in {}: {error:?}", build.display()));
            println!("{}: {world_select:x?}", build.display());
            if let Ok(lookup) = lookup {
                println!("{}: {:x?}", build.display(), find_game_vals(&pe, player.client, &lookup));
            }
            if let Ok(names) = names {
                println!("{}: {:x?}", build.display(), find_appearance_decoder(&pe, names.player_name, player.player_index));
                let model = find_npc_model(&pe, names.npc_definition)
                    .unwrap_or_else(|error| panic!("NPC model not found in {}: {error:?}", build.display()));
                println!("{}: {model:x?}", build.display());
                let vertices = find_model_vertices(&pe, &model)
                    .unwrap_or_else(|error| panic!("model vertices not found in {}: {error:?}", build.display()));
                println!("{}: {vertices:x?}", build.display());
                let faces = find_model_faces(&pe, &vertices)
                    .unwrap_or_else(|error| panic!("model faces not found in {}: {error:?}", build.display()));
                println!("{}: {faces:x?}", build.display());
                let orientation = find_actor_orientation(&pe)
                    .unwrap_or_else(|error| panic!("actor orientation not found in {}: {error:?}", build.display()));
                println!("{}: {orientation:x?}", build.display());
                let height = find_animation_height(&pe, names.npc_definition)
                    .unwrap_or_else(|error| panic!("animation height not found in {}: {error:?}", build.display()));
                println!("{}: animation height {height:x}", build.display());
            }
            let scene = find_scene(&pe).unwrap_or_else(|error| panic!("scene not found in {}: {error:?}", build.display()));
            println!("{}: {scene:x?}", build.display());
            let collision = find_collision_maps(&pe, &scene).unwrap_or_else(|error| panic!("collision maps not found in {}: {error:?}", build.display()));
            assert_eq!(collision, scene::CollisionMaps { maps: 0x2f8, stride: 0x28 });
            let game = find_game_object_layout(&pe, &scene).expect("game object layout");
            println!("{}: {game:x?}", build.display());
            println!("{}: {:x?}", build.display(), find_loc_configs(&pe, &scene, &game).expect("loc configs"));
            let decoration = find_decoration_layout(&pe, &scene).expect("decoration layout");
            println!("{}: {decoration:x?}", build.display());
            println!("{}: item pile {:x?}", build.display(), find_item_pile(&pe, &decoration));
            println!("{}: {:x?}", build.display(), find_loc_names(&pe).expect("loc names"));
            let interfaces = find_interfaces(&pe).unwrap();
            let projection = find_projection(&pe, player.client).unwrap();
            let local = find_local_projection(&pe, player.client, interfaces.canvas, &projection, &scene)
                .unwrap_or_else(|error| panic!("local projection not found in {}: {error:?}", build.display()));
            println!("{}: {local:x?}", build.display());
            {
            }
        }
    }
}
