use super::{items::string_rvas, model::whole_function, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scene {
    pub scene: u32,
    pub size_x: u32,
    pub size_y: u32,
    pub tiles: u32,
    pub wall: u32,
    pub wall_decoration: u32,
    pub ground_decoration: u32,
    pub tag: u32,
    pub game_count: u32,
    pub game_array: u32,
    pub game_tag: u32,
    pub game_origin_x: u32,
    pub game_origin_y: u32,
    pub id_shift: u32,
}

pub mod loc_kind {
    pub const WALL: u8 = 0;
    pub const WALL_DECORATION: u8 = 1;
    pub const GAME: u8 = 2;
    pub const GROUND_DECORATION: u8 = 3;
    pub const ITEM_PILE: u8 = 4;
}

fn mem_load(i: &Instruction, size: usize) -> Option<(Register, Register, u32)> {
    (i.mnemonic() == Mnemonic::Mov || i.mnemonic() == Mnemonic::Movsxd)
        .then_some(())
        .filter(|_| i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Memory && i.memory_base() != Register::RIP)
        .filter(|_| i.op0_register().size() == size || (i.mnemonic() == Mnemonic::Movsxd && size == 4))
        .map(|_| (i.op0_register(), i.memory_base(), i.memory_displacement64() as u32))
}

fn calls(instrs: &[Instruction]) -> impl Iterator<Item = (usize, u32)> + '_ {
    instrs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)
        .map(|(n, i)| (n, i.near_branch_target() as u32))
}

fn id_shift(pe: &Pe, decoder: u32) -> Option<u32> {
    let instrs = decode_at(pe, decoder, 8);
    instrs.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::DL && i.op1_kind() == OpKind::Immediate8).then(|| i.immediate8() as u32)
    })
}

struct Find {
    size_x: u32,
    size_y: u32,
    tiles: u32,
    slots: Vec<(u32, u32, u8)>,
    id_decoder: u32,
    game_find: u32,
}

fn scene_find(pe: &Pe, rva: u32) -> Option<Find> {
    let instrs = whole_function(pe, rva, 0x300);
    // sizes: the -1 plane path multiplies by size_y only, the normal one by size_x then size_y
    let imuls: Vec<(usize, u32)> = instrs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.mnemonic() == Mnemonic::Imul && i.op_count() == 2 && i.op1_kind() == OpKind::Memory && i.memory_base() == Register::RCX)
        .map(|(n, i)| (n, i.memory_displacement64() as u32))
        .collect();
    let (x_at, size_x) = *imuls.iter().find(|(_, d)| imuls.iter().filter(|(_, e)| e == d).count() == 1)?;
    let size_y = imuls.iter().map(|&(_, d)| d).find(|&d| d != size_x)?;
    let tiles = instrs[x_at..].iter().find_map(|i| mem_load(i, 8).filter(|l| l.1 == Register::RCX).map(|l| l.2))?;
    let mut slots = Vec::new();
    let mut id_decoder = None;
    for (n, call) in calls(&instrs) {
        let window = &instrs[n.saturating_sub(4)..n];
        let Some((_, _, tag)) = window.iter().rev().find_map(|i| mem_load(i, 8).filter(|l| l.0 == Register::RCX)) else { continue };
        let Some(slot) = instrs[n.saturating_sub(8)..n].iter().find_map(|i| mem_load(i, 8).filter(|l| l.0 == Register::RCX && l.2 != tag).map(|l| l.2))
        else {
            continue;
        };
        let kind = instrs[n..(n + 8).min(instrs.len())].iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt16 && i.immediate(1) & 0xff00 == 0x100)
                .then(|| i.immediate(1) as u8)
        });
        if let Some(kind) = kind {
            slots.push((slot, tag, kind));
            id_decoder.get_or_insert(call);
        }
    }
    // the game object lookup: the call taking (scene, plane, x, y) that isn't the id decoder
    let id_decoder = id_decoder?;
    let game_find = calls(&instrs).map(|(_, c)| c).find(|&c| c != id_decoder)?;
    Some(Find { size_x, size_y, tiles, slots, id_decoder, game_find })
}

fn game_find(pe: &Pe, rva: u32) -> Option<(u32, u32, u32, u32, u32)> {
    let instrs = whole_function(pe, rva, 0x200);
    let (count_at, count) = instrs.iter().enumerate().find_map(|(n, i)| {
        (i.mnemonic() == Mnemonic::Movsxd && i.op1_kind() == OpKind::Memory && i.memory_base() != Register::RIP && i.memory_base() != Register::RSP)
            .then(|| (n, i.memory_displacement64() as u32))
    })?;
    let array = instrs[count_at + 1..].iter().find_map(|i| mem_load(i, 8).filter(|_| i.mnemonic() == Mnemonic::Mov).map(|l| l.2))?;
    let (call_at, _) = calls(&instrs).find(|&(n, _)| n > count_at)?;
    let tag = instrs[call_at.saturating_sub(3)..call_at].iter().find_map(|i| mem_load(i, 8).filter(|l| l.0 == Register::RCX).map(|l| l.2))?;
    let compares: Vec<u32> = instrs[call_at..]
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_base() != Register::RIP)
        .map(|i| i.memory_displacement64() as u32)
        .take(2)
        .collect();
    let [x, y] = compares[..] else { return None };
    Some((count, array, tag, x, y))
}

pub fn find_scene(image: &[u8]) -> Result<Scene, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let loc_find = lua_binding(&pe, "locFind")?;
    let instrs = whole_function(&pe, loc_find, 0x400);
    let mut found = Vec::new();
    for (n, target) in calls(&instrs) {
        let Some(find) = scene_find(&pe, target) else { continue };
        let Some(scene) = instrs[n.saturating_sub(10)..n].iter().rev().find_map(|i| mem_load(i, 8).filter(|l| l.0 == Register::RCX).map(|l| l.2))
        else {
            continue;
        };
        let slot = |kind: u8| find.slots.iter().find(|s| s.2 == kind).copied();
        let (Some(wall), Some(decoration), Some(ground)) = (slot(loc_kind::WALL), slot(loc_kind::WALL_DECORATION), slot(loc_kind::GROUND_DECORATION))
        else {
            continue;
        };
        if wall.1 != decoration.1 || wall.1 != ground.1 {
            continue;
        }
        let (Some((game_count, game_array, game_tag, game_origin_x, game_origin_y)), Some(id_shift)) =
            (game_find(&pe, find.game_find), id_shift(&pe, find.id_decoder))
        else {
            continue;
        };
        found.push(Scene {
            scene,
            size_x: find.size_x,
            size_y: find.size_y,
            tiles: find.tiles,
            wall: wall.0,
            wall_decoration: decoration.0,
            ground_decoration: ground.0,
            tag: wall.1,
            game_count,
            game_array,
            game_tag,
            game_origin_x,
            game_origin_y,
            id_shift,
        });
    }
    one(found)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GameObjectLayout {
    pub renderable: u32,
    pub x: u32,
    pub height: u32,
    pub y: u32,
    pub orientation: u32,
    pub config: u32,
}

fn placement_call(instrs: &[Instruction], call: usize, tag: u32) -> Option<GameObjectLayout> {
    let window = &instrs[call.saturating_sub(14)..call];
    let mut loads: HashMap<Register, (Register, u32)> = HashMap::new();
    let mut stack: HashMap<u32, (Register, u32)> = HashMap::new();
    let (mut x, mut height) = (None, None);
    for i in window {
        if let Some((dst, base, d)) = mem_load(i, 4) {
            match dst {
                Register::R8D => x = Some((base, d)),
                Register::R9D => height = Some((base, d)),
                _ => {
                    loads.insert(dst, (base, d));
                }
            }
        } else if i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RSP && i.op1_kind() == OpKind::Register {
            if let Some(&load) = loads.get(&i.op1_register()) {
                stack.insert(i.memory_displacement64() as u32, load);
            }
        }
    }
    let (base, x) = x?;
    let (_, height) = height.filter(|h| h.0 == base)?;
    let (_, y) = stack.get(&0x20).filter(|l| l.0 == base).copied()?;
    let (_, orientation) = stack.get(&0x28).filter(|l| l.0 == base).copied()?;
    let renderable = instrs[call.saturating_sub(24)..call]
        .iter()
        .find_map(|i| mem_load(i, 8).filter(|l| l.1 == base && l.2 != tag).map(|l| l.2))?;
    // y, x and height are next to each other, orientation and config follow
    (x == height + 4 && y == x + 4 && orientation == y + 4)
        .then_some(GameObjectLayout { renderable, x, height, y, orientation, config: orientation + 4 })
}

pub fn find_game_object_layout(image: &[u8], scene: &Scene) -> Result<GameObjectLayout, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let code = pe.bytes(begin, end);
        if !code.windows(4).any(|w| w == scene.game_tag.to_le_bytes()) {
            continue;
        }
        let instrs: Vec<Instruction> = Decoder::with_ip(64, code, begin as u64, DecoderOptions::NONE).into_iter().collect();
        if !instrs.iter().any(|i| mem_load(i, 8).is_some_and(|l| l.2 == scene.game_tag)) {
            continue;
        }
        for (n, _) in calls(&instrs) {
            found.extend(placement_call(&instrs, n, scene.game_tag));
        }
    }
    one(found)
}

// Where walls and decorations keep their config byte (shape in the low 6 bits, rotation in the next 2), from Java's
// `Scene.getConfig(plane, x, y, tag)`: each slot's object is checked against the tag, then its config is the
// `movzx` that follows. The game object branch reads the config `find_game_object_layout` found, which makes the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocConfigs {
    pub wall: u32,
    pub wall_decoration: u32,
    pub ground_decoration: u32,
}

fn config_after(instrs: &[Instruction], from: usize, tag: u32) -> Option<u32> {
    let cmp = instrs[from..].iter().position(|i| {
        i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_base() != Register::RIP && i.memory_displacement64() as u32 == tag
    })?;
    let base = instrs[from + cmp].memory_base();
    instrs[from + cmp + 1..(from + cmp + 4).min(instrs.len())].iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Movzx && i.op1_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt8 && i.memory_base() == base)
            .then(|| i.memory_displacement64() as u32)
    })
}

pub fn find_loc_configs(image: &[u8], scene: &Scene, game: &GameObjectLayout) -> Result<LocConfigs, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let code = pe.bytes(begin, end);
        if code.len() > 0x200 || !code.windows(4).any(|w| w == scene.game_tag.to_le_bytes()) {
            continue;
        }
        let instrs: Vec<Instruction> = Decoder::with_ip(64, code, begin as u64, DecoderOptions::NONE).into_iter().collect();
        // `lea rcx,[tile + slot]` before each slot's check
        let slot_at = |slot: u32| {
            instrs.iter().position(|i| i.mnemonic() == Mnemonic::Lea && i.memory_base() != Register::RIP && i.memory_displacement64() as u32 == slot)
        };
        let (Some(wall), Some(decoration), Some(ground)) = (slot_at(scene.wall), slot_at(scene.wall_decoration), slot_at(scene.ground_decoration)) else {
            continue;
        };
        if !(wall < decoration && decoration < ground) {
            continue;
        }
        let configs = (config_after(&instrs, wall, scene.tag), config_after(&instrs, decoration, scene.tag), config_after(&instrs, ground, scene.tag));
        let (Some(wall), Some(wall_decoration), Some(ground_decoration)) = configs else { continue };
        let games = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_displacement64() as u32 == scene.game_tag);
        let game_config = games.and_then(|at| {
            instrs[at..].iter().find_map(|i| {
                (i.mnemonic() == Mnemonic::Movzx && i.op1_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt8).then(|| i.memory_displacement64() as u32)
            })
        });
        if game_config == Some(game.config) {
            found.push(LocConfigs { wall, wall_decoration, ground_decoration });
        }
    }
    one(found)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecorationLayout {
    pub x: u32,
    pub height: u32,
    pub y: u32,
    pub x_offset: u32,
    pub y_offset: u32,
}

pub fn find_decoration_layout(image: &[u8], scene: &Scene) -> Result<DecorationLayout, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        for w in instrs.windows(5) {
            let (Some((a, base, x_offset)), Some((_, b2, height))) = (mem_load(&w[0], 4), mem_load(&w[1], 4)) else { continue };
            let adds = |i: &Instruction, reg: Register| {
                (i.mnemonic() == Mnemonic::Add && i.op0_kind() == OpKind::Register && i.op0_register() == reg && i.op1_kind() == OpKind::Memory && i.memory_base() == base)
                    .then(|| i.memory_displacement64() as u32)
            };
            let Some(x) = adds(&w[2], a) else { continue };
            let Some((c, b3, y_offset)) = mem_load(&w[3], 4) else { continue };
            let Some(y) = adds(&w[4], c) else { continue };
            if b2 == base && b3 == base && height == x + 4 && y == x + 8 && y < scene.tag && y_offset == x_offset + 4 {
                found.push(DecorationLayout { x, height, y, x_offset, y_offset });
            }
        }
    }
    one(found)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocNames {
    pub lookup: u32,
    pub name: u32,
    pub transforms: u32,
    pub transform: u32,
}

pub fn find_loc_names(image: &[u8]) -> Result<LocNames, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let colors = string_rvas(&pe, "<col=00FFFF>");
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        for (n, i) in instrs.iter().enumerate() {
            if !(i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand() && colors.contains(&(i.ip_rel_memory_address() as u32))) {
                continue;
            }
            let before = &instrs[n.saturating_sub(20)..n];
            let Some(name) = before.iter().rev().find_map(|j| {
                (j.mnemonic() == Mnemonic::Lea && j.op0_register() == Register::R9 && j.memory_base() != Register::RIP && j.memory_index() == Register::None)
                    .then(|| j.memory_displacement64() as u32)
            }) else {
                continue;
            };
            let head = &instrs[n.saturating_sub(120)..n];
            // calls answering a `{control, definition}` pair: `mov a,[rax]; mov b,[rax+8]` right after
            let pair_calls: Vec<(usize, u32)> = calls(head)
                .filter(|&(c, _)| {
                    let after = &head[c + 1..(c + 4).min(head.len())];
                    let loads = |d: u64| after.iter().any(|j| mem_load(j, 8).is_some_and(|l| l.1 == Register::RAX && l.2 as u64 == d));
                    loads(0) && loads(8)
                })
                .collect();
            for pair in pair_calls.windows(2) {
                let ((lookup_at, lookup), (transform_at, transform)) = (pair[0], pair[1]);
                let transforms = head[lookup_at..transform_at].iter().find_map(|j| {
                    (j.mnemonic() == Mnemonic::Cmp
                        && j.op0_kind() == OpKind::Memory
                        && j.memory_base() != Register::RIP
                        && j.memory_size() == iced_x86::MemorySize::UInt64
                        && matches!(j.op1_kind(), OpKind::Immediate8to64 | OpKind::Immediate32to64)
                        && j.immediate(1) == 0)
                        .then(|| j.memory_displacement64() as u32)
                });
                if let Some(transforms) = transforms {
                    found.push(LocNames { lookup, name, transforms, transform });
                }
            }
        }
    }
    one(found)
}

// The scene's collision maps (Java's `CollisionMap[]`), from `getCollisionMap(plane)`: the scene's vector begin, indexed by `plane * 5 * 8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CollisionMaps {
    pub maps: u32,
    pub stride: u32,
}

pub fn find_collision_maps(image: &[u8], scene: &Scene) -> Result<CollisionMaps, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let instrs = whole_function(&pe, lua_binding(&pe, "getCollisionMap")?, 0x100);
    let mut found = Vec::new();
    for (n, i) in instrs.iter().enumerate() {
        let Some((holder, _, scene_field)) = mem_load(i, 8) else { continue };
        if scene_field != scene.scene {
            continue;
        }
        let Some((begin, maps)) = instrs[n + 1..].iter().find_map(|j| mem_load(j, 8).filter(|l| l.1 == holder).map(|l| (l.0, l.2))) else {
            continue;
        };
        // `lea r,[p+p*4]`, then `[begin + r*8]`
        let times_five = instrs.iter().any(|j| j.mnemonic() == Mnemonic::Lea && j.memory_base() == j.memory_index() && j.memory_index_scale() == 4);
        let times_eight = instrs.iter().any(|j| j.op_kinds().any(|k| k == OpKind::Memory) && j.memory_base() == begin && j.memory_index_scale() == 8);
        if times_five && times_eight {
            found.push(CollisionMaps { maps, stride: 5 * 8 });
        }
    }
    one(found)
}
