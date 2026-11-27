use std::{
    cell::OnceCell,
    collections::{HashMap, HashSet},
};

mod actor_path;
mod camera;
mod chat;
mod frame;
mod ground_items;
mod hider;
#[cfg(test)]
mod highlights;
mod item_icon;
mod item_setter;
mod items;
mod local_projection;
mod login_provider;
mod menu;
mod minimap;
mod model;
mod packet_layouts;
mod packet_names;
mod packets;
mod projectiles;
mod projection;
mod render;
mod scene;
mod scene_loader;
mod script_runs;
mod scripts;
mod spot_anims;
mod values;
mod varc_setters;
mod world_select;
pub use actor_path::find_actor_path;
pub use camera::find_camera;
pub use chat::find_chat;
pub use frame::{find_frame, Frame};
pub use ground_items::{find_ground_item_ownership, find_ground_items, find_item_pile};
pub use hider::{find_hider, Hider};
pub use item_icon::find_item_icon;
pub use item_setter::find_item_setter;
pub use local_projection::find_local_projection;
pub use login_provider::find_login_provider;
pub use menu::{
    find_menu, find_menu_click, find_menu_layout, find_menu_open, find_menu_order,
    find_menu_settings, find_selection, MenuLayout,
};
pub use minimap::{find_destination, find_minimap};
pub use model::{
    find_animation_height, find_model_faces, find_model_vertices, find_npc_model, render_slots,
    ModelFaces, ModelVertices, NpcModel,
};
pub use packet_layouts::{find_packet_layouts, PacketLayouts};
pub use packet_names::{find_packet_names, NamedPacket, PacketAnchors};
pub use packets::{find_packets, Packets, BUFFER_SIZE};
pub use projectiles::find_projectiles;
pub use projection::find_projection;
pub use render::{find_render, Render};
pub use scene::{
    find_collision_maps, find_decoration_layout, find_game_object_layout, find_loc_configs,
    find_scene, loc_kind, GameObjectLayout, LocConfigs, Scene,
};
pub use scene_loader::find_instance_flag;
pub use script_runs::{find_script_runs, ScriptRuns, ARG_KIND, ARG_SIZE, KIND_INT};
pub use scripts::{
    find_component_fields, find_component_invalidate, find_component_sprite, ComponentInvalidate,
};
pub use spot_anims::find_spot_anims;
pub use values::{
    find_actor_lists, find_actor_names, find_appearance_decoder, find_item_containers,
    find_npc_head_icons, find_run_energy, find_varcs, AppearanceDecoder, ItemContainers, World,
};
pub use varc_setters::{find_varc_setters, VarcSetters, VARIANT_KIND, VARIANT_STRING};
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

use ipc::state::array;

pub mod hash_table {
    pub const BUCKETS: usize = 0x8;
    pub const BUCKET_COUNT: usize = 0x10;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    InvalidImage,
    NotFound,
    Ambiguous(usize),
    Panicked,
}

pub fn guarded<T>(rule: impl FnOnce() -> Result<T, ScanError>) -> Result<T, ScanError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(rule)).unwrap_or(Err(ScanError::Panicked))
}

struct Section {
    rva: usize,
    size: usize,
}

pub struct Pe<'a> {
    image: &'a [u8],
    code: Vec<Section>,
    functions: Vec<(u32, u32)>,
    unwinds: Vec<Option<u32>>,
    calls: OnceCell<Vec<(u32, u32)>>,
    rdx_leas: OnceCell<Vec<(u32, u32)>>,
}

const UNW_FLAG_CHAININFO: u8 = 4;
const LEAF_ALIGN: u32 = 16;
const MAX_LEAF: u32 = 0x200;
const INT3: u8 = 0xcc;
const RET: u8 = 0xc3;

fn chunk_root(pe: &Pe, begin: u32) -> Option<u32> {
    let mut begin = begin;
    for _ in 0..8 {
        let index = pe.functions.partition_point(|&(b, _)| b < begin);
        let &(b, _) = pe.functions.get(index)?;
        if b != begin {
            return None;
        }
        let Some(unwind) = pe.unwinds[index] else {
            return Some(begin);
        };
        let unwind = unwind as usize;
        let flags = *pe.image.get(unwind)? >> 3;
        if flags & UNW_FLAG_CHAININFO == 0 {
            return Some(begin);
        }
        let codes = *pe.image.get(unwind + 2)? as usize;
        begin = u32_at(pe.image, unwind + 4 + ((codes + 1) & !1) * 2)?;
    }
    None
}

fn function_head(pe: &Pe, begin: u32, end: u32, limit: usize) -> Vec<Instruction> {
    instructions(pe.bytes(begin, end), begin)
        .take(limit)
        .collect()
}

fn function_instrs(pe: &Pe, begin: u32, end: u32) -> Vec<Instruction> {
    function_head(pe, begin, end, usize::MAX)
}

fn before_padding(mut instrs: Vec<Instruction>) -> Vec<Instruction> {
    if let Some(padding) = instrs
        .windows(2)
        .position(|w| w.iter().all(|i| i.mnemonic() == Mnemonic::Int3))
    {
        instrs.truncate(padding);
    }
    instrs
}

fn whole_function(pe: &Pe, begin: u32, max: u32) -> Vec<Instruction> {
    image_function(pe.image, begin, max)
}

fn image_function(image: &[u8], begin: u32, max: u32) -> Vec<Instruction> {
    let end = (begin as usize + max as usize).min(image.len());
    before_padding(decode(&image[(begin as usize).min(end)..end], begin))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

fn calls_to<'p>(pe: &'p Pe, target: u32) -> impl Iterator<Item = u32> + 'p {
    direct_calls(pe)
        .iter()
        .filter(move |&&(_, t)| t == target)
        .map(|&(site, _)| site)
}

fn caller_functions<'p>(pe: &'p Pe, target: u32) -> impl Iterator<Item = (u32, u32)> + 'p {
    calls_to(pe, target).filter_map(|site| pe.function_containing(site))
}

struct Caller {
    begin: u32,
    end: u32,
    instrs: Vec<Instruction>,
    at: usize,
}

fn callers<'p>(pe: &'p Pe, target: u32) -> impl Iterator<Item = Caller> + 'p {
    calls_to(pe, target).filter_map(|site| {
        let (begin, end) = pe.function_containing(site)?;
        let instrs = function_instrs(pe, begin, end);
        let at = instrs.iter().position(|i| i.ip32() == site)?;
        Some(Caller {
            begin,
            end,
            instrs,
            at,
        })
    })
}

fn functions_containing(pe: &Pe, hits: impl IntoIterator<Item = u32>) -> Vec<(u32, u32)> {
    let mut functions: Vec<(u32, u32)> = hits
        .into_iter()
        .filter_map(|hit| pe.function_containing(hit))
        .collect();
    functions.sort_unstable();
    functions.dedup();
    functions
}

fn functions_with_dword(pe: &Pe, value: u32) -> Vec<(u32, u32)> {
    functions_containing(pe, dword_hits(pe, value))
}

fn string_rvas(pe: &Pe, name: &str) -> HashSet<u32> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(name.as_bytes());
    needle.push(0);
    pe.image
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_slice())
        .map(|(at, _)| at as u32 + 1)
        .collect()
}

fn rip_lea(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand())
        .then(|| i.ip_rel_memory_address() as u32)
}

fn dword_store(i: &Instruction, base: Register) -> bool {
    i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.memory_base() == base
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt32
        && i.op1_kind() == OpKind::Register
}

fn client_registers(instrs: &[Instruction], client: u32) -> HashSet<Register> {
    instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.is_ip_rel_memory_operand()
                && i.ip_rel_memory_address() as u32 == client
        })
        .map(|i| i.op0_register())
        .collect()
}

fn client_field(i: &Instruction, clients: &HashSet<Register>) -> Option<u32> {
    (i.op1_kind() == OpKind::Memory
        && clients.contains(&i.memory_base())
        && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

fn calls(instrs: &[Instruction]) -> impl Iterator<Item = (usize, u32)> + '_ {
    instrs
        .iter()
        .enumerate()
        .filter_map(|(n, i)| Some((n, call_target(i)?)))
}

fn has_dword(code: &[u8], value: u32) -> bool {
    code.windows(4).any(|w| w == value.to_le_bytes())
}

fn site_function(pe: &Pe, site: u32) -> Option<(Vec<Instruction>, usize)> {
    let (begin, end) = pe.function_containing(site)?;
    let instrs = whole_function(pe, begin, end - begin);
    let at = instrs.iter().position(|i| i.ip() == site as u64)?;
    Some((instrs, at))
}

fn rel32_branches<'p>(pe: &'p Pe, opcode: u8) -> impl Iterator<Item = (i64, i64)> + 'p {
    pe.code_sections().flat_map(move |(rva, data)| {
        data.windows(5)
            .enumerate()
            .filter(move |(_, window)| window[0] == opcode)
            .map(move |(offset, window)| {
                let site = (rva + offset) as i64;
                let target = site + 5 + i32::from_le_bytes(window[1..5].try_into().unwrap()) as i64;
                (site, target)
            })
    })
}

fn direct_jumps<'p>(pe: &'p Pe) -> impl Iterator<Item = (u32, u32)> + 'p {
    rel32_branches(pe, 0xe9).map(|(site, target)| (site as u32, target as u32))
}

fn memory_field(i: &Instruction, base: Register) -> Option<u32> {
    (i.op1_kind() == OpKind::Memory
        && i.memory_base() == base
        && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

pub(crate) fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

pub(crate) fn u32_at(data: &[u8], at: usize) -> Option<u32> {
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

        let entries = image.get(exception_rva..exception_rva.checked_add(exception_size)?)?;
        let mut entries: Vec<(u32, u32, u32)> = entries
            .as_chunks::<12>()
            .0
            .iter()
            .map(|entry| Some((u32_at(entry, 0)?, u32_at(entry, 4)?, u32_at(entry, 8)?)))
            .collect::<Option<_>>()?;
        entries.retain(|&(begin, end, _)| end > begin && end as usize <= image.len());
        entries.sort_unstable();

        let mut pe = Self {
            image,
            code,
            functions: Vec::new(),
            unwinds: Vec::new(),
            calls: OnceCell::new(),
            rdx_leas: OnceCell::new(),
        };
        let mut all: Vec<(u32, u32, Option<u32>)> = entries
            .iter()
            .map(|&(begin, end, unwind)| (begin, end, Some(unwind)))
            .collect();
        pe.functions = entries.iter().map(|&(begin, end, _)| (begin, end)).collect();
        all.extend(pe.leaf_functions().into_iter().map(|(begin, end)| (begin, end, None)));
        all.sort_unstable();
        pe.functions = all.iter().map(|&(begin, end, _)| (begin, end)).collect();
        pe.unwinds = all.iter().map(|&(_, _, unwind)| unwind).collect();
        Some(pe)
    }

    fn leaf_functions(&self) -> Vec<(u32, u32)> {
        let mut targets: Vec<u32> = rel32_branches(self, 0xe8)
            .filter_map(|(_, target)| u32::try_from(target).ok())
            .filter(|&target| target % LEAF_ALIGN == 0 && self.in_code(target as usize))
            .filter(|&target| self.function_containing(target).is_none())
            .filter(|&target| {
                target
                    .checked_sub(1)
                    .and_then(|before| self.image.get(before as usize))
                    .is_some_and(|&byte| byte == INT3 || byte == RET)
            })
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
            .into_iter()
            .filter_map(|begin| {
                let instrs = image_function(self.image, begin, MAX_LEAF);
                let last = instrs.last()?;
                let ends = matches!(last.mnemonic(), Mnemonic::Ret | Mnemonic::Jmp);
                let valid = instrs.iter().all(|i| !i.is_invalid());
                (ends && valid).then(|| (begin, last.next_ip() as u32))
            })
            .collect()
    }

    fn function_containing(&self, rva: u32) -> Option<(u32, u32)> {
        let index = self.functions.partition_point(|&(begin, _)| begin <= rva);
        let &(begin, end) = self.functions.get(index.checked_sub(1)?)?;
        (rva < end).then_some((begin, end))
    }

    fn bytes(&self, begin: u32, end: u32) -> &[u8] {
        let end = (end as usize).min(self.image.len());
        &self.image[(begin as usize).min(end)..end]
    }

    fn code_sections(&self) -> impl Iterator<Item = (usize, &'a [u8])> + '_ {
        self.code
            .iter()
            .map(|s| (s.rva, &self.image[s.rva..s.rva + s.size]))
    }

    fn section_containing(&self, rva: usize) -> Option<&Section> {
        self.code
            .iter()
            .find(|s| (s.rva..s.rva + s.size).contains(&rva))
    }

    fn in_code(&self, rva: usize) -> bool {
        self.section_containing(rva).is_some()
    }
}

fn instructions(code: &[u8], rva: u32) -> impl Iterator<Item = Instruction> + '_ {
    Decoder::with_ip(64, code, rva as u64, DecoderOptions::NONE).into_iter()
}

fn decode(code: &[u8], rva: u32) -> Vec<Instruction> {
    instructions(code, rva).collect()
}

fn call_target(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)
        .then(|| i.near_branch_target() as u32)
}

fn immediate(i: &Instruction) -> Option<u64> {
    matches!(
        i.op1_kind(),
        OpKind::Immediate8
            | OpKind::Immediate8to32
            | OpKind::Immediate8to64
            | OpKind::Immediate32
            | OpKind::Immediate32to64
    )
    .then(|| i.immediate(1))
}

fn imm32(i: &Instruction) -> Option<u32> {
    matches!(i.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
        .then(|| i.immediate(1) as u32)
}

fn base_field(i: &Instruction) -> Option<Load> {
    (i.memory_index() == Register::None
        && !matches!(i.memory_base(), Register::None | Register::RIP))
    .then(|| (i.memory_base(), i.memory_displacement64() as i64))
}

fn global_load(i: &Instruction) -> Option<(Register, u32)> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Register
        && i.op1_kind() == OpKind::Memory
        && i.is_ip_rel_memory_operand())
    .then(|| (i.op0_register(), i.ip_rel_memory_address() as u32))
}

fn global_store(i: &Instruction) -> Option<(Register, u32)> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.is_ip_rel_memory_operand()
        && i.op1_kind() == OpKind::Register)
        .then(|| (i.op1_register(), i.ip_rel_memory_address() as u32))
}

fn dword_hits(pe: &Pe, value: u32) -> Vec<u32> {
    let needle = value.to_le_bytes();
    let mut hits = Vec::new();
    for (rva, data) in pe.code_sections() {
        for (offset, window) in data.windows(4).enumerate() {
            if window == needle {
                hits.push((rva + offset) as u32);
            }
        }
    }
    hits
}

type Load = (Register, i64);

fn compared_offsets(code: &[u8], rva: u32) -> (Vec<Load>, Vec<Load>) {
    let mut loaded: HashMap<Register, Load> = HashMap::new();
    let (mut mins, mut maxs) = (Vec::new(), Vec::new());

    for instr in instructions(code, rva).filter(|i| !i.is_invalid()) {
        let reg = instr.op0_register().full_register32();
        match instr.mnemonic() {
            Mnemonic::Call => loaded.clear(),
            Mnemonic::Cmp if instr.op0_kind() == OpKind::Register => {
                let Some(value) = immediate(&instr).map(|v| v as u32) else {
                    continue;
                };
                if let Some(&load) = loaded.get(&reg) {
                    if value == LOCAL_MIN {
                        mins.push(load);
                    } else if value == LOCAL_MAX {
                        maxs.push(load);
                    }
                }
            }
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register && instr.op1_kind() == OpKind::Memory =>
            {
                match base_field(&instr) {
                    Some(load) => loaded.insert(reg, load),
                    None => loaded.remove(&reg),
                };
            }
            _ if instr.op0_kind() == OpKind::Register => {
                loaded.remove(&reg);
            }
            _ => {}
        }
    }

    (mins, maxs)
}

fn position_in(mins: &[Load], maxs: &[Load]) -> Option<(u32, u32)> {
    for (i, &(base, disp)) in mins.iter().enumerate() {
        let y = (base, disp + 4);
        if disp >= 0
            && mins[i + 1..].contains(&y)
            && maxs.contains(&(base, disp))
            && maxs.contains(&y)
        {
            return Some((disp as u32, disp as u32 + 4));
        }
    }
    None
}

fn world_base_in(code: &[u8], rva: u32) -> Option<u32> {
    let subs: Vec<Load> = instructions(code, rva)
        .filter(|i| {
            i.mnemonic() == Mnemonic::Sub
                && i.op0_kind() == OpKind::Register
                && i.op1_kind() == OpKind::Memory
                && i.memory_index() == Register::None
                && !matches!(i.memory_base(), Register::None | Register::RIP | Register::RSP)
        })
        .map(|i| (i.memory_base(), i.memory_displacement64() as i64))
        .collect();

    let mut pairs: Vec<i64> = subs
        .iter()
        .filter(|&&(base, d)| d >= 0 && subs.contains(&(base, d + 4)))
        .map(|&(_, d)| d)
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    match pairs.as_slice() {
        [only] => Some(*only as u32),
        _ => None,
    }
}

pub fn find_actor_position(pe: &Pe) -> Result<ActorPosition, ScanError> {
    let mut found = Vec::new();
    for (begin, end) in functions_with_dword(pe, LOCAL_MIN) {
        let code = pe.bytes(begin, end);
        if !has_dword(code, LOCAL_MAX) {
            continue;
        }

        let (mins, maxs) = compared_offsets(code, begin);
        if let Some((x, y)) = position_in(&mins, &maxs) {
            let Some(base_x) = world_base_in(code, begin) else {
                continue;
            };
            found.push(ActorPosition {
                x,
                y,
                function: begin,
                base_x,
            });
        }
    }

    single(&found)
}

fn field_compares(code: &[u8], rva: u32) -> HashMap<Load, HashSet<u32>> {
    let mut compares: HashMap<Load, HashSet<u32>> = HashMap::new();
    let mut loaded: HashMap<Register, Load> = HashMap::new();

    for instr in instructions(code, rva) {
        if let Some(value) = imm32(&instr).filter(|_| instr.mnemonic() == Mnemonic::Cmp) {
            let compared = match instr.op0_kind() {
                OpKind::Memory => base_field(&instr),
                OpKind::Register => loaded.get(&instr.op0_register().full_register32()).copied(),
                _ => None,
            };
            if let Some(load) = compared {
                compares.entry(load).or_default().insert(value);
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
                match base_field(&instr) {
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
    single(&found)
}

pub fn find_client(pe: &Pe, game_state: u32) -> Result<u32, ScanError> {
    client_global(pe, &[game_state])
}

fn decode_at(pe: &Pe, rva: u32, limit: usize) -> Vec<Instruction> {
    let Some(section) = pe.section_containing(rva as usize) else {
        return Vec::new();
    };
    let end = (section.rva + section.size).min(rva as usize + limit * 15);
    instructions(&pe.image[rva as usize..end], rva)
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
        && matches!(
            add.op1_kind(),
            OpKind::Immediate32to64 | OpKind::Immediate8to64
        );
    let is_jmp = jmp.mnemonic() == Mnemonic::Jmp && jmp.op0_kind() == OpKind::NearBranch64;
    (is_add && is_jmp).then(|| (id, add.immediate(1) as u32, jmp.near_branch_target() as u32))
}

const MAX_THUNKS: u32 = 8;

fn hash_table_offset(pe: &Pe, rva: u32, thunks: u32) -> Option<u32> {
    let instrs = decode_at(pe, rva, 8);
    if let [add, jmp, ..] = instrs.as_slice() {
        if add.mnemonic() == Mnemonic::Add
            && add.op0_register() == Register::RCX
            && jmp.mnemonic() == Mnemonic::Jmp
            && jmp.op0_kind() == OpKind::NearBranch64
        {
            let inner =
                hash_table_offset(pe, jmp.near_branch_target() as u32, thunks.checked_sub(1)?)?;
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
        if instr.op0_kind() == OpKind::Register
            && instr.op0_register().full_register() == Register::RCX
        {
            break;
        }
    }

    let offset = |disp: &u32| disp.checked_sub(hash_table::BUCKETS as u32);
    let mut tables = buckets
        .iter()
        .filter(|&&b| {
            counts.contains(&(b + (hash_table::BUCKET_COUNT - hash_table::BUCKETS) as u32))
        })
        .filter_map(offset);
    let table = tables.next()?;
    tables.next().is_none().then_some(table)
}

fn direct_calls<'p>(pe: &'p Pe) -> &'p [(u32, u32)] {
    pe.calls.get_or_init(|| {
        rel32_branches(pe, 0xe8)
            .filter(|&(_, target)| usize::try_from(target).is_ok_and(|target| pe.in_code(target)))
            .map(|(site, target)| (site as u32, target as u32))
            .collect()
    })
}

fn rdx_leas<'p>(pe: &'p Pe) -> &'p [(u32, u32)] {
    pe.rdx_leas.get_or_init(|| {
        let mut found = Vec::new();
        for (rva, data) in pe.code_sections() {
            for (offset, w) in data.windows(7).enumerate() {
                if w[..3] != [0x48, 0x8d, 0x15] {
                    continue;
                }
                let site = (rva + offset) as u32;
                let target =
                    (site as i64 + 7 + i32::from_le_bytes(w[3..7].try_into().unwrap()) as i64)
                        as u32;
                found.push((site, target));
            }
        }
        found
    })
}

fn call_targets(pe: &Pe) -> Vec<u32> {
    let mut targets: Vec<u32> = direct_calls(pe).iter().map(|&(_, target)| target).collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn player_getter(pe: &Pe, call: u32) -> Option<(u32, u32)> {
    let instrs = decode_at(pe, call, 10);
    let [_call, after @ ..] = instrs.as_slice() else {
        return None;
    };

    let [test, je, rest @ ..] = after else {
        return None;
    };
    if test.mnemonic() != Mnemonic::Test
        || test.op0_register() != Register::RAX
        || je.mnemonic() != Mnemonic::Je
    {
        return None;
    }

    let [load, mov, rest @ ..] = rest else {
        return None;
    };
    let index = load_from(load, load.memory_base())
        .filter(|_| load.op0_register() == Register::EDX && load.memory_base() != Register::RAX)?;
    if mov.mnemonic() != Mnemonic::Mov
        || mov.op0_register() != Register::RCX
        || mov.op1_register() != Register::RAX
    {
        return None;
    }

    let jmp = rest.iter().find(|i| i.mnemonic() == Mnemonic::Jmp)?;
    (jmp.op0_kind() == OpKind::NearBranch64).then(|| (index, jmp.near_branch_target() as u32))
}

fn client_global(pe: &Pe, offsets: &[u32]) -> Result<u32, ScanError> {
    let functions = functions_containing(
        pe,
        offsets.iter().flat_map(|&offset| dword_hits(pe, offset)),
    );

    const VOLATILE: [Register; 7] = [
        Register::RAX,
        Register::RCX,
        Register::RDX,
        Register::R8,
        Register::R9,
        Register::R10,
        Register::R11,
    ];

    let mut globals: HashMap<u32, usize> = HashMap::new();
    for (begin, end) in functions {
        let mut loaded: HashMap<Register, u32> = HashMap::new();
        for instr in instructions(pe.bytes(begin, end), begin).filter(|i| !i.is_invalid()) {
            if instr.op_kinds().any(|kind| kind == OpKind::Memory) {
                let disp = instr.memory_displacement64() as u32;
                if let Some(&global) = loaded.get(&instr.memory_base()) {
                    if instr.memory_index() == Register::None && offsets.contains(&disp) {
                        *globals.entry(global).or_default() += 1;
                    }
                }
            }

            if let Some((reg, global)) = global_load(&instr).filter(|&(reg, _)| reg.size() == 8) {
                loaded.insert(reg, global);
                continue;
            }
            match instr.mnemonic() {
                Mnemonic::Call => VOLATILE.iter().for_each(|r| {
                    loaded.remove(r);
                }),
                Mnemonic::Ret | Mnemonic::Jmp => loaded.clear(),
                _ if instr.op0_kind() == OpKind::Register => {
                    loaded.remove(&instr.op0_register().full_register());
                }
                _ => {}
            }
        }
    }

    single(&globals.into_keys().collect::<Vec<_>>())
}

pub fn find_local_player(pe: &Pe) -> Result<LocalPlayer, ScanError> {
    let getters: Vec<(u32, (u32, u32, u32))> = call_targets(pe)
        .into_iter()
        .filter_map(|target| Some((target, world_view_getter(&decode_at(pe, target, 3))?)))
        .collect();
    let (getter, (world_view_id, manager, lookup)) = single(&getters)?;
    let world_views = manager
        .checked_add(hash_table_offset(pe, lookup, MAX_THUNKS).ok_or(ScanError::NotFound)?)
        .ok_or(ScanError::NotFound)?;

    let mut player = None;
    for site in calls_to(pe, getter) {
        let Some((index, lookup)) = player_getter(pe, site) else {
            continue;
        };
        let Some(table) = hash_table_offset(pe, lookup, MAX_THUNKS) else {
            continue;
        };
        let found = (index, table);
        if player.is_some_and(|p| p != found) {
            return Err(ScanError::Ambiguous(2));
        }
        player = Some(found);
    }
    let (player_index, players) = player.ok_or(ScanError::NotFound)?;

    Ok(LocalPlayer {
        world_view_id,
        player_index,
        world_views,
        players,
    })
}

fn registered_code(pe: &Pe, site: u32) -> Option<u32> {
    let code_lea = |i: &Instruction| rip_lea(i).filter(|&rva| pe.in_code(rva as usize));
    for instr in decode_at(pe, site, 12) {
        if let Some(code) = code_lea(&instr) {
            return Some(code);
        }
        if let Some(target) = call_target(&instr) {
            let callee = decode_at(pe, target, 8);
            if let Some(code) = callee.iter().find_map(code_lea) {
                return Some(code);
            }
        }
    }
    None
}

fn paired_code(pe: &Pe, site: u32) -> Option<u32> {
    let (begin, end) = pe.function_containing(site)?;
    let instrs = function_instrs(pe, begin, end);
    let at = instrs.iter().position(|i| i.ip32() == site)?;
    if instrs[at].op0_register() == Register::RDX {
        return None;
    }
    let statement = instrs[..at]
        .iter()
        .rposition(|i| i.mnemonic() == Mnemonic::Call)
        .map_or(0, |call| call + 1);
    let codes: Vec<u32> = instrs[statement..at]
        .iter()
        .filter_map(|i| rip_lea(i).filter(|&rva| pe.in_code(rva as usize)))
        .collect();
    match codes.as_slice() {
        [code] => Some(*code),
        _ => None,
    }
}

fn lua_binding(pe: &Pe, name: &str) -> Result<u32, ScanError> {
    let strings = string_rvas(pe, name);
    let registered = rdx_leas(pe)
        .iter()
        .filter(|(_, target)| strings.contains(target))
        .filter_map(|&(site, _)| registered_code(pe, site));
    let paired = strings
        .iter()
        .flat_map(|&string| menu::rip_lea_sites(pe, string))
        .filter_map(|site| paired_code(pe, site));
    let implementations: HashSet<u32> = registered.chain(paired).collect();
    single(&implementations.into_iter().collect::<Vec<_>>())
}

fn global_field_getter(instrs: &[Instruction]) -> Option<(u32, u32)> {
    let [global, load, ret, ..] = instrs else {
        return None;
    };
    let (_, global) = global_load(global).filter(|&(reg, _)| reg == Register::RAX)?;
    let disp = load_from(load, Register::RAX).filter(|_| load.op0_register() == Register::EAX)?;
    (ret.mnemonic() == Mnemonic::Ret).then_some((global, disp))
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
    (data.ip_rel_memory_address() as u32)
        .checked_sub(array::DATA as u32)
        .ok_or(ScanError::NotFound)
}

fn client_array_getter(instrs: &[Instruction], client: u32) -> Option<u32> {
    let mut client_register = None;
    for instr in instrs {
        match instr.mnemonic() {
            Mnemonic::Ret => return None,
            Mnemonic::Mov if global_load(instr).is_some_and(|(_, global)| global == client) => {
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
    let evenly_spaced =
        spacing == arrays[2] - arrays[1] && spacing % 4 == 0 && (20..=64).contains(&(spacing / 4));
    if !evenly_spaced {
        return Err(ScanError::NotFound);
    }
    Ok(Skills {
        levels,
        real_levels,
        xp,
        count: spacing / 4,
    })
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

fn login_box_writes(instrs: &[Instruction], panel_x: u32) -> Vec<(u32, u32)> {
    let adds_immediate = |i: &Instruction, reg: Register| {
        i.mnemonic() == Mnemonic::Add
            && i.op0_kind() == OpKind::Register
            && i.op0_register() == reg
            && imm32(i).is_some()
    };
    let stores = |i: &Instruction, reg: Register| {
        global_store(i)
            .filter(|&(from, _)| from == reg)
            .map(|(_, global)| global)
    };

    instrs
        .windows(5)
        .filter_map(|w| {
            let [load, add_box, box_write, add_center, center_write] = w else {
                return None;
            };
            let (reg, _) =
                global_load(load).filter(|&(reg, global)| reg.size() == 4 && global == panel_x)?;
            if !(adds_immediate(add_box, reg) && adds_immediate(add_center, reg)) {
                return None;
            }
            Some((stores(box_write, reg)?, stores(center_write, reg)?))
        })
        .collect()
}

fn login_y_write(instrs: &[Instruction]) -> Option<u32> {
    let target = instrs.windows(2).find_map(|w| {
        let [set, store] = w else {
            return None;
        };
        let sets_default = set.mnemonic() == Mnemonic::Mov
            && set.op0_kind() == OpKind::Register
            && set.op0_register().size() == 4
            && matches!(set.op1_kind(), OpKind::Immediate32)
            && set.immediate(1) as u32 == LOGIN_Y_DEFAULT;
        let (from, global) = global_store(store)?;
        (sets_default && from == set.op0_register()).then_some(global)
    })?;

    instrs.iter().enumerate().find_map(|(i, convert)| {
        if convert.mnemonic() != Mnemonic::Cvttss2si || convert.op0_register().size() != 4 {
            return None;
        }
        let reg = convert.op0_register();
        instrs[i + 1..].iter().take(3).find_map(|store| {
            global_store(store)
                .filter(|&(from, y)| from == reg && y != target)
                .map(|(_, y)| y)
        })
    })
}

fn panel_x_write(instrs: &[Instruction]) -> Option<u32> {
    const NEG_PANEL_WIDTH: u32 = LOGIN_PANEL_WIDTH.wrapping_neg();
    let (mut since_subtract, mut halved) = (None::<u32>, false);

    for instr in instrs.iter().filter(|i| !i.is_invalid()) {
        if halved {
            if let Some((_, global)) = global_store(instr).filter(|&(from, _)| from.size() == 4) {
                return Some(global);
            }
        }

        if let Some(n) = since_subtract {
            let halves = matches!(instr.mnemonic(), Mnemonic::Sar | Mnemonic::Shr)
                && instr.op1_kind() == OpKind::Immediate8
                && instr.immediate(1) == 1;
            since_subtract = if halves {
                halved = true;
                None
            } else {
                (n < 10).then_some(n + 1)
            };
        }

        let subtracts_panel_width = match instr.mnemonic() {
            Mnemonic::Add => imm32(instr) == Some(NEG_PANEL_WIDTH),
            Mnemonic::Sub => imm32(instr) == Some(LOGIN_PANEL_WIDTH),
            _ => false,
        };
        if subtracts_panel_width {
            since_subtract = Some(0);
        }
    }
    None
}

fn login_state_reads(instrs: &[Instruction], game_state: u32) -> Vec<u32> {
    let mut loaded: HashMap<Register, Load> = HashMap::new();
    let mut gated: Option<Register> = None;
    let mut window: Option<u32> = None;
    let mut pending_global: Option<(Register, u32)> = None;
    let mut found = Vec::new();

    for instr in instrs.iter().filter(|i| !i.is_invalid()) {
        if let Some((reg, target)) = pending_global.take() {
            let tests_itself = instr.mnemonic() == Mnemonic::Test
                && instr.op0_kind() == OpKind::Register
                && instr.op1_kind() == OpKind::Register
                && instr.op0_register() == reg
                && instr.op1_register() == reg;
            if tests_itself {
                found.push(target);
                window = None;
            }
        }

        if let Some(n) = window {
            pending_global = global_load(instr).filter(|&(reg, _)| reg.size() == 4);
            window = (n < 15).then_some(n + 1);
        }

        let reg0 =
            (instr.op0_kind() == OpKind::Register).then(|| instr.op0_register().full_register32());

        match instr.mnemonic() {
            Mnemonic::Call => loaded.clear(),
            Mnemonic::Sub
                if imm32(instr) == Some(10)
                    && reg0.is_some_and(|reg| {
                        loaded.get(&reg).map(|&(_, disp)| disp) == Some(game_state as i64)
                    }) =>
            {
                gated = reg0;
            }
            Mnemonic::Cmp if imm32(instr) == Some(1) && reg0.is_some() && gated == reg0 => {
                window.get_or_insert(0);
                gated = None;
            }
            Mnemonic::Mov
                if instr.op0_kind() == OpKind::Register && instr.op1_kind() == OpKind::Memory =>
            {
                let reg = instr.op0_register().full_register32();
                match base_field(instr) {
                    Some(load) => loaded.insert(reg, load),
                    None => loaded.remove(&reg),
                };
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

fn login_screen_compares(instrs: &[Instruction], game_state: u32) -> Vec<u32> {
    const LOGGING_IN: u64 = 20;
    let compares_dword = |i: &Instruction| {
        i.mnemonic() == Mnemonic::Cmp
            && i.op0_kind() == OpKind::Memory
            && i.memory_size() == iced_x86::MemorySize::UInt32
            && immediate(i) == Some(LOGGING_IN)
    };
    instrs
        .windows(3)
        .filter_map(|w| {
            let [state, jump, screen] = w else {
                return None;
            };
            let compares_state = compares_dword(state)
                && base_field(state).is_some_and(|(_, disp)| disp == game_state as i64);
            let compares_screen = compares_dword(screen) && screen.is_ip_rel_memory_operand();
            (compares_state && jump.mnemonic() == Mnemonic::Je && compares_screen)
                .then(|| screen.ip_rel_memory_address() as u32)
        })
        .collect()
}

pub fn find_login_panel(pe: &Pe, game_state: u32) -> Result<LoginPanel, ScanError> {
    let (mut panel_x_candidates, mut screen_candidates, mut y_candidates) =
        (Vec::new(), Vec::new(), Vec::new());
    for &(begin, end) in &pe.functions {
        let instrs = function_instrs(pe, begin, end);
        panel_x_candidates.extend(panel_x_write(&instrs));
        screen_candidates.extend(login_state_reads(&instrs, game_state));
        screen_candidates.extend(login_screen_compares(&instrs, game_state));
        y_candidates.extend(login_y_write(&instrs));
    }

    let panel_x = one(panel_x_candidates)?;
    let box_candidates = pe
        .functions
        .iter()
        .flat_map(|&(begin, end)| login_box_writes(&function_instrs(pe, begin, end), panel_x))
        .collect();
    let (box_x, center_x) = one(box_candidates)?;

    Ok(LoginPanel {
        panel_x,
        box_x,
        center_x,
        y: one(y_candidates)?,
        screen: one(screen_candidates)?,
    })
}

fn single<T: Copy>(found: &[T]) -> Result<T, ScanError> {
    match found {
        [] => Err(ScanError::NotFound),
        [only] => Ok(*only),
        many => Err(ScanError::Ambiguous(many.len())),
    }
}

fn one<T: Ord + Copy>(mut candidates: Vec<T>) -> Result<T, ScanError> {
    candidates.sort_unstable();
    candidates.dedup();
    single(&candidates)
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
        && !matches!(
            instr.memory_base(),
            Register::None | Register::RIP | Register::RSP | Register::RBP
        ))
    .then(|| instr.memory_displacement64() as u32)
}

fn component_layout_function(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
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
            Mnemonic::Sub
                if instr.op0_kind() == OpKind::Register && instr.op0_register().size() == 4 =>
            {
                subtracted.extend(field_operand(instr, 1));
            }
            Mnemonic::Mov
                if instr.op1_kind() == OpKind::Register && instr.op1_register().size() == 4 =>
            {
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
    let width = pair(
        subtracted
            .into_iter()
            .filter(|&d| d != original_x && d != original_x + 4)
            .collect(),
    )?;
    (x != width && x != original_x).then_some((original_x, x, width))
}

fn component_tree_loop(instrs: &[Instruction], width: u32) -> Option<(u32, u32)> {
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
        (instr.mnemonic() == Mnemonic::Cmp
            && instr.op1_kind() == OpKind::Register
            && instr.op1_register().size() == 4)
            .then(|| field_operand(instr, 0))
            .flatten()
            .filter(|&d| d != scroll_x && d != scroll_x + 4)
    })?;
    Some((parent, scroll_x))
}

fn sub_interface_lookup(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
    instrs.windows(9).find_map(|win| {
        let [cmp, jne, load_id, rest @ ..] = win else {
            return None;
        };
        let kind = (cmp.mnemonic() == Mnemonic::Cmp && imm32(cmp) == Some(0))
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
        let buckets = loads
            .iter()
            .find(|&&(size, d)| size == 8 && loads.contains(&(4, d + 8)))?
            .1;
        Some((kind, id, buckets.checked_sub(hash_table::BUCKETS as u32)?))
    })
}

fn pointer_getter(instrs: &[Instruction]) -> Option<u32> {
    let [load, ret, ..] = instrs else { return None };
    (load_from(load, Register::RCX).is_some()
        && load.op0_register() == Register::RAX
        && ret.mnemonic() == Mnemonic::Ret)
        .then(|| load.memory_displacement64() as u32)
}

fn group_getter(instrs: &[Instruction]) -> Option<u32> {
    let [index, triple, load, scale, ret, ..] = instrs else {
        return None;
    };
    let ok = index.mnemonic() == Mnemonic::Movsxd
        && triple.mnemonic() == Mnemonic::Lea
        && triple.memory_index_scale() == 2
        && scale.mnemonic() == Mnemonic::Lea
        && scale.memory_index_scale() == 8
        && ret.mnemonic() == Mnemonic::Ret
        && load.op0_register() == Register::RAX;
    ok.then(|| load_from(load, Register::RCX)).flatten()
}

pub const UNSCALED: u32 = 0;

pub fn find_interfaces(pe: &Pe) -> Result<Interfaces, ScanError> {
    let mut layouts = Vec::new();
    let mut lookups = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs = function_instrs(pe, begin, end);
        if let Some(found) = component_layout_function(&instrs) {
            layouts.push((begin, found));
        }
        lookups.extend(sub_interface_lookup(&instrs));
    }
    let (layout_rva, (_, x, width)) = one(layouts)?;
    let (kind, id, sub_interfaces) = one(lookups)?;

    let calls = direct_calls(pe);
    let mut loops = Vec::new();
    for (begin, end) in caller_functions(pe, layout_rva) {
        if let Some(found) = component_tree_loop(&function_instrs(pe, begin, end), width) {
            loops.push((begin, found));
        }
    }
    let (loop_rva, (parent, scroll_x)) = one(loops)?;

    let loop_entries: HashSet<u32> = calls
        .iter()
        .map(|&(_, t)| t)
        .filter(|&t| t <= loop_rva && loop_rva - t <= 0x20)
        .collect();
    let mut roots = Vec::new();
    for (begin, end) in loop_entries
        .iter()
        .flat_map(|&entry| caller_functions(pe, entry))
    {
        let targets: Vec<u32> = calls
            .iter()
            .filter(|&&(site, _)| (begin..end).contains(&site))
            .map(|&(_, target)| target)
            .collect();
        let manager = targets
            .iter()
            .find_map(|&t| pointer_getter(&decode_at(pe, t, 2)));
        let groups = targets
            .iter()
            .find_map(|&t| group_getter(&decode_at(pe, t, 5)));
        if let (Some(manager), Some(groups)) = (manager, groups) {
            roots.push((begin, manager, groups));
        }
    }
    let (resize_root, manager, groups) = one(roots)?;

    let mut root_candidates = Vec::new();
    for site in calls_to(pe, resize_root) {
        let before = (8..=40).rev().find_map(|back| {
            let instrs = decode_at(pe, site.saturating_sub(back), 16);
            instrs
                .iter()
                .any(|i| i.ip() as u32 == site)
                .then_some(instrs)
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

    let scales = call_targets(pe).into_iter().filter_map(|t| {
        let instrs = decode_at(pe, t, 6);
        let loads_canvas = instrs.first().is_some_and(|i| {
            i.op0_kind() == OpKind::Register
                && i.op0_register() == Register::RAX
                && load_from(i, Register::RCX) == Some(canvas)
        });
        loads_canvas.then(|| {
            instrs.iter().take(4).find_map(|i| {
                (i.mnemonic() == Mnemonic::Divss
                    && i.op1_kind() == OpKind::Memory
                    && i.memory_base() == Register::RAX)
                    .then(|| i.memory_displacement64() as u32)
            })
        })?
    });
    let ui_scale = match one(scales.collect()) {
        Ok(offset) => offset,
        Err(ScanError::NotFound) => UNSCALED,
        Err(error) => return Err(error),
    };

    Ok(Interfaces {
        root,
        manager,
        groups,
        sub_interfaces,
        component: ComponentLayout {
            id,
            kind,
            parent,
            x,
            width,
            scroll_x,
        },
        canvas,
        ui_width,
        ui_scale,
    })
}

#[cfg(test)]
mod tests {
    use super::menu::{MenuClick, Selection};
    use super::projection::Projection;
    use super::spot_anims::SpotAnims;
    use super::values::{
        ActorLists, ActorNames, AppearanceDecoder, ItemContainers, RunEnergy, Varcs,
    };
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
        let code = [
            0x05, 0x03, 0xfd, 0xff, 0xff, 0xd1, 0xf8, 0x89, 0x05, 0x00, 0x01, 0x00, 0x00,
        ];
        assert_eq!(
            panel_x_write(&decode(&code, 0x1000)),
            Some(0x1000 + 13 + 0x100)
        );
    }

    #[test]
    fn matches_login_state_selector() {
        let code = [
            0x8b, 0x83, 0x60, 0x21, 0x00, 0x00, 0x83, 0xe8, 0x0a, 0x83, 0xf8, 0x01, 0x8b, 0x53,
            0x10, 0x8b, 0x0d, 0x50, 0x00, 0x00, 0x00, 0x85, 0xc9,
        ];
        assert_eq!(
            login_state_reads(&decode(&code, 0x1000), 0x2160),
            vec![0x1000 + 21 + 0x50]
        );
    }

    #[test]
    fn login_state_selector_ignores_an_unrelated_gate() {
        let code = [
            0x8b, 0x83, 0x10, 0x00, 0x00, 0x00, 0x83, 0xe8, 0x0a, 0x83, 0xf8, 0x01, 0x8b, 0x0d,
            0x50, 0x00, 0x00, 0x00, 0x85, 0xc9,
        ];
        assert_eq!(
            login_state_reads(&decode(&code, 0x1000), 0x2160),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn matches_login_box_derivation() {
        let code = [
            0x8b, 0x05, 0x00, 0x01, 0x00, 0x00, 0x05, 0xca, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00,
            0x02, 0x00, 0x00, 0x05, 0xb4, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00, 0x03, 0x00, 0x00,
        ];
        let instrs = decode(&code, 0x1000);
        assert_eq!(login_box_writes(&instrs, 0x1106), vec![(0x1211, 0x131c)]);
        assert_eq!(login_box_writes(&instrs, 0x1107), vec![]);
    }

    #[test]
    fn matches_login_y_easing() {
        let code = [
            0xb8, 0xab, 0x00, 0x00, 0x00, 0x89, 0x05, 0x00, 0x01, 0x00, 0x00, 0xf3, 0x0f, 0x2c,
            0xda, 0xf3, 0x0f, 0x11, 0x15, 0x50, 0x00, 0x00, 0x00, 0x89, 0x1d, 0x00, 0x02, 0x00,
            0x00,
        ];
        assert_eq!(login_y_write(&decode(&code, 0x1000)), Some(0x121d));
        assert_eq!(login_y_write(&decode(&code[11..], 0x100b)), None);
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
    fn finds_exact_offsets_in_243_1() {
        let Some(image) = archived_image("osclient-243.1") else {
            eprintln!("skipped, build 243.1 isn't archived");
            return;
        };
        let pe = Pe::parse(&image).unwrap();
        let position = find_actor_position(&pe).expect("actor position not found");
        println!("{position:x?}");
        assert_eq!(position.y, position.x + 4);

        let player = find_local_player(&pe).expect("local player not found");
        println!("{player:x?}");
        assert_ne!(player.world_view_id, player.player_index);

        let game_state = find_game_state(&pe).expect("game state not found");
        println!("game state +{game_state:#x}");
        let client = find_client(&pe, game_state).expect("client not found");

        let skills = find_skills(&pe, client).expect("skills not found");
        println!("{skills:x?}");
        assert_eq!(skills.count, 25);

        let tick_count = find_tick_count(&pe, client).expect("tick count not found");
        println!("tick count +{tick_count:#x}");
        let varps = find_varps(&pe).expect("varps not found");
        println!("varps RVA {varps:#x}");

        let interfaces = find_interfaces(&pe).expect("interfaces not found");
        println!("{interfaces:x?}");
        assert_eq!(
            interfaces,
            Interfaces {
                root: 0x413c08,
                manager: 0x413c18,
                groups: 0x6890,
                sub_interfaces: 0x413bd8,
                component: ComponentLayout {
                    id: 0x4,
                    kind: 0x28,
                    parent: 0x74,
                    x: 0x5c,
                    width: 0x64,
                    scroll_x: 0x84
                },
                canvas: 0x90,
                ui_width: 0x6c,
                ui_scale: 0x68,
            }
        );

        let containers = find_item_containers(&pe).expect("item containers not found");
        println!("{containers:x?}");
        assert_eq!(
            containers,
            ItemContainers {
                table: 0x15a7940,
                ids: 0x8,
                quantities: 0x20,
                next: 0x38
            }
        );
        let varcs = find_varcs(&pe, client).expect("varcs not found");
        println!("{varcs:x?}");
        assert_eq!(
            varcs,
            Varcs {
                object: 0x417ce8,
                begin: 0x48,
                entry_size: 0x30,
                value: 0x8,
                kind: 0x28
            }
        );
        let energy = find_run_energy(&pe).expect("run energy not found");
        println!("{energy:x?}");
        assert_eq!(
            energy,
            RunEnergy {
                energy: 0x3528,
                weight: 0x352c
            }
        );
        let actors = find_actor_lists(&pe).expect("actor lists not found");
        println!("{actors:x?}");
        assert_eq!(
            actors,
            ActorLists {
                npc_indices: 0xd0,
                npc_table: 0x90,
                player_count: 0xcd10
            }
        );
        let names = find_actor_names(&pe).expect("actor names not found");
        println!("{names:x?}");
        assert_eq!(
            names,
            ActorNames {
                npc_name_override: 0x710,
                npc_definition: 0x730,
                definition_id: 0,
                definition_name: 0x8,
                player_name: 0x718,
                npc_animation: 0x4d8
            }
        );
        let decoder = find_appearance_decoder(&pe, names.player_name, player.player_index)
            .expect("appearance decoder not found");
        println!("{decoder:x?}");
        assert_eq!(
            decoder,
            AppearanceDecoder {
                function: 0xa3860,
                actor_id: 0x79c
            }
        );
        let invalidate = find_component_invalidate(&pe).expect("component invalidation not found");
        println!("{invalidate:x?}");
        assert_eq!(
            invalidate,
            ComponentInvalidate {
                function: 0x7dff0,
                holder_component: 0x8,
                text: 0x158
            }
        );
        let fields = find_component_fields(&pe).expect("component fields not found");
        println!("{fields:x?}");
        assert_eq!(
            fields,
            scripts::ComponentFields {
                hidden: 0x78,
                text_color: 0x98,
                scroll_width: 0x8c,
                index: 0x8,
                children: 0xb50
            }
        );
        let sprite = find_component_sprite(&pe).expect("component sprite not found");
        println!("component sprite +{sprite:#x}");
        assert_eq!(sprite, 0xc8);
        let projection = find_projection(&pe, client).expect("projection not found");
        println!("{projection:x?}");
        let menu = find_menu(&pe).expect("menu not found");
        println!("{menu:x?}");
        assert_eq!(menu.object, 0x413430);
        assert_eq!(
            (
                menu.entry_size,
                menu.target,
                menu.option,
                menu.ids,
                menu.priority,
                menu.kind,
                menu.category,
                menu.cancel
            ),
            (0x150, 0x50, 0xb8, 0x128, 0x134, 0x146, 0x147, 0x14d)
        );
        assert_eq!(find_menu_order(&pe, &menu), Ok(0x120));
        let click = find_menu_click(&pe, &menu).expect("menu click not found");
        println!("{click:x?}");
        assert_eq!(
            click,
            MenuClick {
                minimenu_vtable: 0xbd8ab8,
                entry: 0x2a0,
                click_x: 0x3f0,
                counter: 0x3fc
            }
        );
        assert_eq!(find_menu_open(&pe, &click), Ok(0x800));
        assert_eq!(
            find_menu_layout(&pe, &click),
            Ok(MenuLayout {
                view: 0x158,
                function: 0x623df0,
                rows: 0x140,
                margin: 10,
                path: 0x10,
                open_row: 0x58,
                submenu: 0x10
            })
        );
        let selection = find_selection(&pe, &menu).expect("selection not found");
        println!("{selection:x?}");
        assert_eq!(
            selection,
            Selection {
                offset: 0x810,
                verb: 0,
                name: 0x48,
                id: 0x90,
                child: 0x94,
                item: 0x98,
                flags: 0x9e,
                selected: 0x9f
            }
        );
        assert_eq!(
            projection,
            Projection {
                world_to_screen: 0x180880,
                tile_height: 0x99d90,
                top_world_view: 0xcac0,
                plane: 0x18
            }
        );
        let minimap = find_minimap(&pe, client).expect("minimap not found");
        println!("{minimap:x?}");
        assert_eq!(
            minimap,
            minimap::Minimap {
                object: 0x40eaf0,
                zoom: 0x6470,
                angle: 0x89620,
                focal_x: 0x8968c,
            }
        );
        assert_eq!(find_destination(&pe, minimap.object), Ok(0));
        let chat = find_chat(&pe);
        println!("{chat:x?}");
        assert_eq!(
            chat,
            Ok(chat::Chat {
                messages: 0x15a78d0,
                cycle: 0xeaff2c,
                message_cycle: 0x8,
                kind: 0x40,
                name: 0x48,
                sender: 0x70,
                text: 0xa0,
                time_utc: 0x28,
                time_local: 0x10
            })
        );
        let projectiles = find_projectiles(&pe).expect("projectiles not found");
        println!("{projectiles:x?}");
        assert_eq!(
            (projectiles.list, projectiles.node, projectiles.cycle),
            (0xccb0, 0x18, 0xeaff2c)
        );
        assert_eq!(
            (
                projectiles.source_level,
                projectiles.target_actor,
                projectiles.start_cycle,
                projectiles.end_cycle,
                projectiles.id
            ),
            (0x260, 0x284, 0x288, 0x28c, 0x298)
        );
        assert_eq!(
            (
                projectiles.x,
                projectiles.z,
                projectiles.y,
                projectiles.orientation,
                projectiles.moving
            ),
            (0x2a0, 0x2a8, 0x2b0, 0x2b8, 0x2c0)
        );
        let spot_anims = find_spot_anims(&pe).expect("spot animations not found");
        println!("{spot_anims:x?}");
        assert_eq!(
            spot_anims,
            SpotAnims {
                list: 0x160,
                cycle: 0xeaff2c,
                start_cycle: 0x260,
                level: 0x264,
                x: 0x268,
                y: 0x26c,
                z: 0x270,
                id: 0x274
            }
        );
        let ground_items = find_ground_items(&pe);
        println!("{ground_items:x?}");
        assert_eq!(
            ground_items,
            Ok(ground_items::GroundItems {
                items: 0x140,
                level: 0x10,
                node_item: 0x18,
                id: 0x20
            })
        );
        let ownership = find_ground_item_ownership(&pe, tick_count, &ground_items.unwrap());
        println!("{ownership:x?}");
        assert_eq!(
            ownership,
            Ok(ground_items::GroundItemOwnership {
                public_tick: 0x28,
                despawn_tick: 0x2c,
                ownership: 0x30,
                never_public: 0x34,
                op_flags: 0x40
            })
        );
        let camera = find_camera(&pe, client).expect("camera not found");
        println!("{camera:x?}");
        assert_eq!(
            camera,
            camera::Camera {
                position: 0x89608,
                pitch: 0x8961c,
                yaw: 0x89620
            }
        );
        let scene = find_scene(&pe).expect("scene not found");
        let game = find_game_object_layout(&pe, &scene).expect("game object layout not found");
        assert_eq!(
            find_loc_configs(&pe, &scene, &game),
            Ok(scene::LocConfigs {
                wall: 0x48,
                wall_decoration: 0x428,
                ground_decoration: 0x228
            })
        );
        let decoration = find_decoration_layout(&pe, &scene).expect("decoration layout not found");
        assert_eq!(find_item_pile(&pe, &decoration), Ok(0x120));
        let local =
            find_local_projection(&pe, client, interfaces.canvas, &projection, &scene)
                .expect("local projection not found");
        println!("{local:x?}");
        assert_eq!(
            local,
            local_projection::LocalProjection {
                zoom: 0x4878,
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
            LoginPanel {
                panel_x: 0xeafef8,
                box_x: 0x15a7aec,
                center_x: 0xeafef0,
                y: 0xd11090,
                screen: 0xeaff28
            }
        );

        let logged_in = decode_at(&pe, lua_binding(&pe, "isLoggedIn").unwrap(), 6);
        let state_compare = logged_in
            .iter()
            .find(|i| {
                i.mnemonic() == Mnemonic::Cmp
                    && i.op0_kind() == OpKind::Memory
                    && i.immediate(1) == 30
            })
            .expect("isLoggedIn doesn't compare with 30");
        assert_eq!(state_compare.memory_displacement64() as u32, game_state);
    }

    pub(super) fn found<T: std::fmt::Debug, E: std::fmt::Debug>(
        what: &str,
        build: &std::path::Path,
        result: Result<T, E>,
    ) -> T {
        let value = result
            .unwrap_or_else(|error| panic!("{what} not found in {}: {error:?}", build.display()));
        println!("{}: {what} {value:x?}", build.display());
        value
    }

    fn archived_image(build: &str) -> Option<Vec<u8>> {
        let mut image = None;
        for_each_archived_build(|path, pe| {
            if path.to_string_lossy().contains(build) {
                image = Some(pe.image.to_vec());
            }
        });
        image
    }

    fn for_each_build_in(dir: &str, mut check: impl FnMut(&std::path::Path, &Pe)) {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') {
            format!("Z:{manifest}")
        } else {
            manifest.into()
        };
        let archive = std::path::Path::new(&manifest).join("../..").join(dir);
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
            check(&build, &pe);
        }
    }

    pub(super) fn for_each_archived_build(check: impl FnMut(&std::path::Path, &Pe)) {
        for_each_build_in("archive", check);
    }

    pub(super) fn for_each_legacy_build(check: impl FnMut(&std::path::Path, &Pe)) {
        for_each_build_in("archive/legacy", check);
    }

    #[test]
    fn finds_npc_head_icons_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let icons = found("NPC head icons", build, find_npc_head_icons(pe));
            assert_eq!(
                (icons.field, icons.sprites),
                (0x760, 0x48),
                "{}",
                build.display()
            );
        });
    }

    #[test]
    fn finds_menu_order_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let menu = found("menu", build, find_menu(pe));
            let key = found("menu order", build, find_menu_order(pe, &menu));
            assert_eq!(
                (menu.category, key),
                (menu.kind + 1, menu.ids - 8),
                "{}",
                build.display()
            );
            let settings = found("menu settings", build, find_menu_settings(pe, &menu));
            assert_eq!(
                settings,
                menu::MenuSettings {
                    player_attack: 0x278,
                    npc_attack: 0x279,
                    follower: 0x27a,
                    team: 0x148,
                    flag: 0x149,
                    npc_level: 0x11c,
                    npc_follower: 0x15b,
                    player_level: 0x780,
                    player_team: 0x790,
                },
                "{}",
                build.display()
            );
        });
    }

    #[test]
    fn finds_menu_open_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let menu = found("menu", build, find_menu(pe));
            let click = found("menu click", build, find_menu_click(pe, &menu));
            let open = found("menu open", build, find_menu_open(pe, &click));
            let name = build.display().to_string();
            if name.contains("osclient-243.1") {
                assert_eq!(open, 0x800, "{name}");
            }
        });
    }

    #[test]
    fn finds_instance_flag_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            found("instance flag", build, find_instance_flag(pe));
        });
    }

    #[test]
    fn finds_actor_path_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let position = find_actor_position(pe).unwrap();
            let path = found("actor path", build, find_actor_path(pe, position.x));
            assert_eq!(
                (path.length, path.x, path.y),
                (0x3ec, 0x3f0, 0x418),
                "{}",
                build.display()
            );
        });
    }

    #[test]
    fn finds_packets_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let packets = found("packets", build, find_packets(pe));
            assert_eq!(
                packets,
                Packets {
                    receive_opcode: 0,
                    receive_buffer: 8,
                    send_type: 0,
                    send_buffer: 8,
                    buffer_data: 0x10,
                    buffer_cursor: 0x18,
                    ..packets
                },
                "{}",
                build.display()
            );
            for function in [packets.receive, packets.send] {
                assert_eq!(
                    pe.function_containing(function).map(|(begin, _)| begin),
                    Some(function),
                    "{}",
                    build.display()
                );
            }
        });
    }

    #[test]
    fn finds_script_runs_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let runs = found("script runs", build, find_script_runs(pe));
            let name = build.file_name().unwrap().to_string_lossy();
            let function = if name.contains("osclient-243.1") {
                0x124020
            } else {
                runs.function
            };
            assert_eq!(
                runs,
                ScriptRuns {
                    function,
                    event_type: 0x8c,
                    args: 0x10,
                    arg_count: 8
                },
                "{}",
                build.display()
            );
        });
    }

    #[test]
    fn finds_varc_setters_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let setters = found("varc setters", build, find_varc_setters(pe));
            if build.to_string_lossy().contains("osclient-243.1") {
                assert_eq!(
                    setters,
                    VarcSetters {
                        get_int: 0x33df0,
                        set_int: 0x33d00,
                        set_object: 0x33fc0
                    },
                    "{}",
                    build.display()
                );
            }
        });
    }

    #[test]
    fn finds_item_setter_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let containers = found("item containers", build, find_item_containers(pe));
            let setter = found("item setter", build, find_item_setter(pe, &containers));
            if build.to_string_lossy().contains("osclient-243.1") {
                assert_eq!(setter, 0x31b00, "{}", build.display());
            }
        });
    }

    #[test]
    fn finds_render_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            found("render", build, find_render(pe));
        });
    }

    #[test]
    fn finds_every_offset_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let game_state = found("game state", build, find_game_state(pe));
            let client = found("client", build, find_client(pe, game_state));
            found("login panel", build, find_login_panel(pe, game_state));
            let interfaces = found("interfaces", build, find_interfaces(pe));
            let player = found("local player", build, find_local_player(pe));
            found("item containers", build, find_item_containers(pe));
            found("varcs", build, find_varcs(pe, client));
            found("run energy", build, find_run_energy(pe));
            found("actor lists", build, find_actor_lists(pe));
            let names = found("actor names", build, find_actor_names(pe));
            found("component invalidate", build, find_component_invalidate(pe));
            found("component sprite", build, find_component_sprite(pe));
            found("component fields", build, find_component_fields(pe));
            let projection = found("projection", build, find_projection(pe, client));
            let menu = found("menu", build, find_menu(pe));
            found("menu order", build, find_menu_order(pe, &menu));
            let click = found("menu click", build, find_menu_click(pe, &menu));
            found("menu layout", build, find_menu_layout(pe, &click));
            found("selection", build, find_selection(pe, &menu));
            let minimap = found("minimap", build, find_minimap(pe, client));
            found("destination", build, find_destination(pe, minimap.object));
            let ground_items = found("ground items", build, find_ground_items(pe));
            let tick_count = found("tick count", build, find_tick_count(pe, client));
            found(
                "ground item ownership",
                build,
                find_ground_item_ownership(pe, tick_count, &ground_items),
            );
            found("projectiles", build, find_projectiles(pe));
            found("spot animations", build, find_spot_anims(pe));
            found("chat", build, find_chat(pe));
            let camera = found("camera", build, find_camera(pe, client));
            assert_eq!(camera.yaw, minimap.angle);
            if build.to_string_lossy().contains("osclient-243.1") {
                found("world select", build, find_world_select(pe, client));
            }
            found(
                "appearance decoder",
                build,
                find_appearance_decoder(pe, names.player_name, player.player_index),
            );
            let model = found("NPC model", build, find_npc_model(pe));
            assert_eq!(model.definition, names.npc_definition, "{}", build.display());
            let vertices = found("model vertices", build, find_model_vertices(pe, &model));
            found("model faces", build, find_model_faces(pe, &vertices));
            found(
                "animation height",
                build,
                find_animation_height(pe, model.definition),
            );
            let scene = found("scene", build, find_scene(pe));
            let collision = found("collision maps", build, find_collision_maps(pe, &scene));
            assert_eq!(
                collision,
                scene::CollisionMaps {
                    maps: 0x2f8,
                    stride: 0x28
                }
            );
            let game = found(
                "game object layout",
                build,
                find_game_object_layout(pe, &scene),
            );
            found("loc configs", build, find_loc_configs(pe, &scene, &game));
            let decoration = found(
                "decoration layout",
                build,
                find_decoration_layout(pe, &scene),
            );
            found("item pile", build, find_item_pile(pe, &decoration));
            found(
                "local projection",
                build,
                find_local_projection(pe, client, interfaces.canvas, &projection, &scene),
            );
        });
    }
}

#[cfg(test)]
mod no_panics {
    use super::tests::{for_each_archived_build, for_each_legacy_build};
    use super::*;

    fn r<T>(v: &Result<T, String>) -> String {
        match v {
            Ok(_) => "ok".into(),
            Err(e) => e.clone(),
        }
    }

    fn guard<T>(f: impl FnOnce() -> Result<T, ScanError>) -> Result<T, String> {
        guarded(f).map_err(|e| format!("{e:?}"))
    }

    #[test]
    fn no_rule_panics_in_any_build() {
        let mut table: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut survey = |build: &std::path::Path, pe: &Pe| {
            let mut row: Vec<(String, String)> = Vec::new();
            let mut put = |name: &str, value: String| row.push((name.to_string(), value));
            let skip = || "skip".to_string();
            let actor = guard(|| find_actor_position(pe));
            put("actor position", r(&actor));
            put(
                "actor path",
                actor
                    .as_ref()
                    .map_or_else(|_| skip(), |a| r(&guard(|| find_actor_path(pe, a.x)))),
            );
            let player = guard(|| find_local_player(pe));
            put("local player", r(&player));
            let game_state = guard(|| find_game_state(pe));
            put("game state", r(&game_state));
            let client = game_state
                .as_ref()
                .ok()
                .map(|&g| guard(|| find_client(pe, g)));
            put("client", client.as_ref().map_or_else(skip, r));
            let client = client.and_then(|c| c.ok());
            put(
                "skills",
                client.map_or_else(skip, |c| r(&guard(|| find_skills(pe, c)))),
            );
            let tick = client.map(|c| guard(|| find_tick_count(pe, c)));
            put("tick count", tick.as_ref().map_or_else(skip, r));
            let varps = guard(|| find_varps(pe));
            put("varps", r(&varps));
            put(
                "login panel",
                game_state
                    .as_ref()
                    .map_or_else(|_| skip(), |g| r(&guard(|| find_login_panel(pe, *g)))),
            );
            put(
                "login provider",
                client.map_or_else(skip, |c| r(&guard(|| find_login_provider(pe, c)))),
            );
            let interfaces = guard(|| find_interfaces(pe));
            put("interfaces", r(&interfaces));
            put("component sprite", r(&guard(|| find_component_sprite(pe))));
            put(
                "component invalidate",
                r(&guard(|| find_component_invalidate(pe))),
            );
            put("component fields", r(&guard(|| find_component_fields(pe))));
            put("hider", r(&guard(|| find_hider(pe))));
            put(
                "world select",
                client.map_or_else(skip, |c| r(&guard(|| find_world_select(pe, c)))),
            );
            let minimap = client.map(|c| guard(|| find_minimap(pe, c)));
            put("minimap", minimap.as_ref().map_or_else(skip, r));
            put(
                "destination",
                match &minimap {
                    Some(Ok(m)) => r(&guard(|| find_destination(pe, m.object))),
                    _ => skip(),
                },
            );
            let ground = guard(|| find_ground_items(pe));
            put("ground items", r(&ground));
            put(
                "ground item ownership",
                match (&tick, &ground) {
                    (Some(Ok(t)), Ok(g)) => r(&guard(|| find_ground_item_ownership(pe, *t, g))),
                    _ => skip(),
                },
            );
            let chat = guard(|| find_chat(pe));
            put("chat", r(&chat));
            put("projectiles", r(&guard(|| find_projectiles(pe))));
            put("spot anims", r(&guard(|| find_spot_anims(pe))));
            let packets = guard(|| find_packets(pe));
            put("packets", r(&packets));
            let layouts = packets
                .as_ref()
                .ok()
                .map(|p| guard(|| find_packet_layouts(pe, p)));
            put("packet layouts", layouts.as_ref().map_or_else(skip, r));
            if let (Ok(p), Some(Ok(l))) = (&packets, &layouts) {
                let anchors = PacketAnchors {
                    tick_count: tick.as_ref().and_then(|t| t.as_ref().ok().copied()),
                    varps: varps.as_ref().ok().copied(),
                    chat_messages: chat.as_ref().ok().map(|c| c.messages),
                };
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    find_packet_names(pe, p, l, anchors)
                })) {
                    Ok(names) => {
                        for (name, result) in names {
                            put(
                                &format!("packet {}", name.spelling()),
                                r(&result.map_err(|e| format!("{e:?}"))),
                            );
                        }
                    }
                    Err(_) => put("packet names", format!("{:?}", ScanError::Panicked)),
                }
            }
            put("script runs", r(&guard(|| find_script_runs(pe))));
            put("varc setters", r(&guard(|| find_varc_setters(pe))));
            let containers = guard(|| find_item_containers(pe));
            put("item containers", r(&containers));
            put(
                "item setter",
                containers
                    .as_ref()
                    .map_or_else(|_| skip(), |c| r(&guard(|| find_item_setter(pe, c)))),
            );
            put("instance flag", r(&guard(|| find_instance_flag(pe))));
            put(
                "varcs",
                client.map_or_else(skip, |c| r(&guard(|| find_varcs(pe, c)))),
            );
            put("run energy", r(&guard(|| find_run_energy(pe))));
            put("actor lists", r(&guard(|| find_actor_lists(pe))));
            let names = guard(|| find_actor_names(pe));
            put("actor names", r(&names));
            put(
                "camera",
                client.map_or_else(skip, |c| r(&guard(|| find_camera(pe, c)))),
            );
            let projection = client.map(|c| guard(|| find_projection(pe, c)));
            put("projection", projection.as_ref().map_or_else(skip, r));
            let menu = guard(|| find_menu(pe));
            put("menu", r(&menu));
            if let Ok(m) = &menu {
                put("menu order", r(&guard(|| find_menu_order(pe, m))));
                put("menu settings", r(&guard(|| find_menu_settings(pe, m))));
                put("selection", r(&guard(|| find_selection(pe, m))));
                let click = guard(|| find_menu_click(pe, m));
                put("menu click", r(&click));
                if let Ok(c) = &click {
                    put("menu open", r(&guard(|| find_menu_open(pe, c))));
                    put("menu layout", r(&guard(|| find_menu_layout(pe, c))));
                }
            }
            put("npc head icons", r(&guard(|| find_npc_head_icons(pe))));
            put(
                "appearance decoder",
                match (&names, &player) {
                    (Ok(n), Ok(p)) => r(&guard(|| {
                        find_appearance_decoder(pe, n.player_name, p.player_index)
                    })),
                    _ => skip(),
                },
            );
            let model = guard(|| find_npc_model(pe));
            put("npc model", r(&model));
            if let Ok(m) = &model {
                let vertices = guard(|| find_model_vertices(pe, m));
                put("model vertices", r(&vertices));
                if let Ok(v) = &vertices {
                    put("model faces", r(&guard(|| find_model_faces(pe, v))));
                }
                put(
                    "animation height",
                    r(&guard(|| find_animation_height(pe, m.definition))),
                );
            }
            let scene = guard(|| find_scene(pe));
            put("scene", r(&scene));
            if let Ok(s) = &scene {
                put("collision maps", r(&guard(|| find_collision_maps(pe, s))));
                let game = guard(|| find_game_object_layout(pe, s));
                put("game object layout", r(&game));
                if let Ok(g) = &game {
                    put("loc configs", r(&guard(|| find_loc_configs(pe, s, g))));
                }
                let decoration = guard(|| find_decoration_layout(pe, s));
                put("decoration layout", r(&decoration));
                if let Ok(d) = &decoration {
                    put("item pile", r(&guard(|| find_item_pile(pe, d))));
                }
                if let (Some(c), Ok(i), Some(Ok(p))) = (client, &interfaces, &projection) {
                    put(
                        "local projection",
                        r(&guard(|| find_local_projection(pe, c, i.canvas, p, s))),
                    );
                }
            }
            put("item icon", r(&guard(|| find_item_icon(pe))));
            put("render", r(&guard(|| find_render(pe))));
            put("frame", r(&guard(|| find_frame(pe))));
            let name = build.file_name().unwrap().to_string_lossy().to_string();
            table.push((name, row));
        };
        for_each_archived_build(&mut survey);
        for_each_legacy_build(&mut survey);
        for (build, row) in &table {
            for (rule, value) in row {
                println!("SURVEY {build} {rule} {value}");
            }
        }
        let panics: Vec<String> = table
            .iter()
            .flat_map(|(build, row)| {
                row.iter()
                    .filter(|(_, value)| value == "Panicked")
                    .map(move |(rule, _)| format!("{rule} in {build}"))
            })
            .collect();
        assert!(panics.is_empty(), "rules panicked: {panics:?}");
    }
}
