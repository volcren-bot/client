use std::sync::Arc;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use ipc::{
    read::{self, Memory},
    state::{game_state, loc_status, locs_request, LocInfo, LocsReply, LOC_CLICKABLE, LOC_NAME_SIZE, LOC_PLANE_MASK, LOC_PLANE_SHIFT, MAX_LOCS, MAX_LOC_NAMES, NO_NAME},
};

use super::{
    hooks::client_image,
    model::Placement,
    offsets::{loc_kind, render_slots, DecorationLayout, GameObjectLayout, LocConfigs, LocNames, Scene},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
};

const TAG_TYPE_SHIFT: u64 = 16;
const TAG_LOC: u64 = 2;
const TAG_NOT_CLICKABLE: u64 = 1 << 19;
const ENTRY_SIZE: u64 = 0x10;
const ENTRY_OBJECT: u64 = 8;
const MAX_GAME_OBJECTS: i32 = 64;
const SHAPE_MASK: i32 = 0x3f;
const ROTATION_SHIFT: i32 = 6;
const UNKNOWN: u8 = 0xff;
const NULL_NAME: &str = "null";

type Lookup = unsafe extern "system" fn(i32) -> *const [u64; 2];
type Transform = unsafe extern "system" fn(u64) -> *const [u64; 2];

#[derive(Clone, Copy)]
pub struct Rules {
    pub module_base: u64,
    pub scene: Scene,
    pub game: GameObjectLayout,
    pub decoration: DecorationLayout,
    pub names: LocNames,
    // where walls and decorations keep their shape and rotation; unknown (0xff) without it
    pub configs: Option<LocConfigs>,
    pub plane: u32,
    pub decoration_height: Option<u32>,
    pub item_pile: Option<u32>,
}

static RULES: OnceLock<Rules> = OnceLock::new();
static SLOTS: Mutex<Option<HashMap<u64, Vec<u32>>>> = Mutex::new(None);

pub fn set_scene(rules: Rules) {
    let _ = RULES.set(rules);
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

struct View {
    scene: u64,
    plane: i32,
    base_x: i32,
    base_y: i32,
}

fn view(memory: &OwnMemory, rules: &Rules) -> Option<View> {
    let layout = LAYOUT.get()?;
    let (world_view, _) = read::local_player(memory, layout)?;
    let base = layout.player.base_x as u64;
    Some(View {
        scene: memory.ptr(world_view + rules.scene.scene as u64).filter(|&s| s != 0)?,
        plane: memory.i32(world_view + rules.plane as u64)?,
        base_x: memory.i32(world_view + base)?,
        base_y: memory.i32(world_view + base + 4)?,
    })
}

/// The tile grid of one plane, read once so that walking every tile costs one read per tile.
struct Tiles {
    plane: i32,
    size_x: i32,
    size_y: i32,
    base: u64,
}

impl Tiles {
    fn new(memory: &OwnMemory, rules: &Rules, scene: u64, plane: i32) -> Option<Self> {
        let s = &rules.scene;
        let (size_x, size_y) = (memory.i32(scene + s.size_x as u64)?, memory.i32(scene + s.size_y as u64)?);
        let base = memory.ptr(scene + s.tiles as u64)?;
        Some(Self { plane, size_x, size_y, base })
    }

    fn at(&self, memory: &OwnMemory, x: i32, y: i32) -> Option<u64> {
        if !(0..4).contains(&self.plane) || !(0..self.size_x).contains(&x) || !(0..self.size_y).contains(&y) {
            return None;
        }
        let index = ((self.plane * self.size_x + x) * self.size_y + y) as u64;
        memory.ptr(self.base + index * ENTRY_SIZE + ENTRY_OBJECT).filter(|&t| t != 0)
    }
}

fn loc_id(rules: &Rules, tag: u64) -> i32 {
    ((tag >> rules.scene.id_shift) & 0xffff) as i32
}

// Each slot, its kind and where its object keeps its config.
fn single_slots(rules: &Rules) -> [(u32, u8, Option<u32>); 3] {
    let s = &rules.scene;
    let c = rules.configs;
    [
        (s.wall, loc_kind::WALL, c.map(|c| c.wall)),
        (s.wall_decoration, loc_kind::WALL_DECORATION, c.map(|c| c.wall_decoration)),
        (s.ground_decoration, loc_kind::GROUND_DECORATION, c.map(|c| c.ground_decoration)),
    ]
}

fn game_objects(memory: &OwnMemory, rules: &Rules, tile: u64) -> Vec<(u64, u64)> {
    let s = &rules.scene;
    let count = memory.i32(tile + s.game_count as u64).unwrap_or(0).clamp(0, MAX_GAME_OBJECTS);
    let Some(array) = memory.ptr(tile + s.game_array as u64).filter(|&a| a != 0) else { return Vec::new() };
    (0..count as u64)
        .filter_map(|i| memory.ptr(array + i * ENTRY_SIZE + ENTRY_OBJECT).filter(|&o| o != 0))
        .filter_map(|o| Some((o, memory.u64(o + s.game_tag as u64)?)))
        .filter(|&(_, tag)| (tag >> TAG_TYPE_SHIFT) & 7 == TAG_LOC)
        .collect()
}

unsafe fn definition(memory: &OwnMemory, rules: &Rules, id: i32) -> Option<u64> {
    let lookup: Lookup = std::mem::transmute((rules.module_base + rules.names.lookup as u64) as *const ());
    memory.ptr(lookup(id) as u64 + 8).filter(|&d| d != 0)
}

unsafe fn decoration_height(memory: &OwnMemory, rules: &Rules, id: i32) -> i16 {
    let height = rules.decoration_height.and_then(|field| memory.i32(definition(memory, rules, id)? + field as u64));
    height.unwrap_or(0).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

unsafe fn name(memory: &OwnMemory, rules: &Rules, id: i32) -> Option<String> {
    let n = &rules.names;
    let transform: Transform = std::mem::transmute((rules.module_base + n.transform as u64) as *const ());
    let mut definition = definition(memory, rules, id)?;
    if memory.u64(definition + n.transforms as u64).is_some_and(|t| t != 0) {
        let pair = transform(definition);
        definition = memory.ptr(pair as u64 + 8).filter(|&d| d != 0)?;
    }
    read::read_string(memory, definition + n.name as u64).filter(|name| !name.is_empty() && name != NULL_NAME)
}

fn fill_locs(reply: &mut LocsReply, request: i32) -> i32 {
    let memory = OwnMemory;
    let Some(layout) = LAYOUT.get() else { return loc_status::NOT_FOUND };
    let Some(rules) = RULES.get() else { return loc_status::NOT_FOUND };
    if on_other_thread() {
        return loc_status::OTHER_THREAD;
    }
    if read::current_game_state(&memory, layout) != Some(game_state::LOGGED_IN) {
        return loc_status::NOT_IN_GAME;
    }
    let Some(view) = view(&memory, rules) else { return loc_status::NOT_IN_GAME };
    let s = &rules.scene;
    let planes = match request & locs_request::PLANES {
        locs_request::ALL_PLANES => 0..4,
        plane @ 0..=3 => plane..plane + 1,
        _ => view.plane..view.plane + 1,
    };
    let clickable_only = request & locs_request::CLICKABLE_ONLY != 0;
    reply.plane = view.plane;
    reply.base_x = view.base_x;
    reply.base_y = view.base_y;
    reply.count = 0;
    reply.name_count = 0;

    let mut names: HashMap<i32, u16> = HashMap::new();
    let mut decoration_heights: HashMap<i32, i16> = HashMap::new();
    let mut name_count = 0usize;
    let mut count = 0usize;
    let mut push = |reply: &mut LocsReply, mut info: LocInfo, tag: u64, plane: i32| {
        let clickable = tag & TAG_NOT_CLICKABLE == 0;
        if count >= MAX_LOCS || (clickable_only && !clickable) {
            return;
        }
        info.id = loc_id(rules, tag);
        info.flags = if clickable { LOC_CLICKABLE } else { 0 } | (plane as u8 & LOC_PLANE_MASK) << LOC_PLANE_SHIFT;
        info.name = *names.entry(info.id).or_insert_with(|| match unsafe { name(&memory, rules, info.id) } {
            Some(name) if name_count < MAX_LOC_NAMES => {
                let slot = &mut reply.names[name_count];
                let bytes = name.as_bytes();
                let len = bytes.len().min(LOC_NAME_SIZE - 1);
                slot.fill(0);
                slot[..len].copy_from_slice(&bytes[..len]);
                name_count += 1;
                (name_count - 1) as u16
            }
            _ => NO_NAME,
        });
        reply.locs[count] = info;
        count += 1;
    };
    for plane in planes {
    let Some(tiles) = Tiles::new(&memory, rules, view.scene, plane) else { continue };
    for x in 0..tiles.size_x {
        for y in 0..tiles.size_y {
            let Some(tile) = tiles.at(&memory, x, y) else { continue };
            let base = LocInfo { x: x as u8, y: y as u8, size_x: 1, size_y: 1, shape: UNKNOWN, rotation: UNKNOWN, ..LocInfo::default() };
            for (slot, kind, config) in single_slots(rules) {
                let Some(object) = memory.ptr(tile + slot as u64).filter(|&o| o != 0) else { continue };
                let Some(tag) = memory.u64(object + s.tag as u64) else { continue };
                let id = loc_id(rules, tag);
                let decoration_height = match kind {
                    loc_kind::GROUND_DECORATION => *decoration_heights.entry(id).or_insert_with(|| unsafe { decoration_height(&memory, rules, id) }),
                    _ => 0,
                };
                let config = config.and_then(|c| { let mut byte = [0u8]; memory.read(object + c as u64, &mut byte).then_some(byte[0]) });
                let (shape, rotation) = config.map_or((UNKNOWN, UNKNOWN), |c| (c & SHAPE_MASK as u8, (c >> ROTATION_SHIFT) & 3));
                push(reply, LocInfo { kind, decoration_height, shape, rotation, ..base }, tag, plane);
            }
            for (object, tag) in game_objects(&memory, rules, tile) {
                let origin = |offset: u32| memory.i32(object + offset as u64);
                let (Some(x1), Some(x2), Some(y1), Some(y2)) =
                    (origin(s.game_origin_x), origin(s.game_origin_x + 4), origin(s.game_origin_y), origin(s.game_origin_y + 4))
                else {
                    continue;
                };
                // listed on every tile it covers: keep its south-west one
                if x1 != x || y1 != y {
                    continue;
                }
                let config = memory.i32(object + rules.game.config as u64).unwrap_or(0);
                push(
                    reply,
                    LocInfo {
                        kind: loc_kind::GAME,
                        size_x: (x2 - x1 + 1).clamp(1, 255) as u8,
                        size_y: (y2 - y1 + 1).clamp(1, 255) as u8,
                        shape: (config & SHAPE_MASK) as u8,
                        rotation: ((config >> ROTATION_SHIFT) & 3) as u8,
                        ..base
                    },
                    tag,
                    plane,
                );
            }
        }
    }
    }
    reply.count = count as u32;
    reply.name_count = name_count as u32;
    loc_status::OK
}

pub fn on_locs_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_locs_request() else { return };
    shared.write_locs_reply_with(|reply| {
        reply.sequence = sequence;
        reply.count = 0;
        reply.name_count = 0;
        reply.status = fill_locs(reply, request);
    });
}

fn slots(memory: &OwnMemory, rules: &Rules, object: u64) -> Vec<u32> {
    let Some(vtable) = memory.ptr(object).filter(|&v| v != 0) else { return Vec::new() };
    let mut cache = SLOTS.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    cache
        .entry(vtable)
        .or_insert_with(|| {
            let first = memory.ptr(vtable).unwrap_or(0);
            match unsafe { client_image() } {
                Some(image) if first > rules.module_base => render_slots(image, (first - rules.module_base) as u32),
                _ => Vec::new(),
            }
        })
        .clone()
}

pub fn loc_renderables(kind: u8, x: i32, y: i32, id: i32) -> Option<Vec<(u64, Placement)>> {
    let memory = OwnMemory;
    let rules = RULES.get()?;
    let view = view(&memory, rules)?;
    let tile = Tiles::new(&memory, rules, view.scene, view.plane)?.at(&memory, x, y)?;
    let s = &rules.scene;
    if kind == loc_kind::GAME {
        let g = &rules.game;
        let (object, _) = game_objects(&memory, rules, tile).into_iter().find(|&(_, tag)| loc_id(rules, tag) == id)?;
        let field = |offset: u32| memory.i32(object + offset as u64);
        let at = Placement { x: field(g.x)?, y: field(g.y)?, height: field(g.height)?, orientation: field(g.orientation)? };
        let renderable = memory.ptr(object + g.renderable as u64).filter(|&r| r != 0)?;
        return Some(vec![(renderable, at)]);
    }
    // an item pile is placed like a decoration with no id in its tag (items are in the tile's ground item list)
    let (object, pile) = if kind == loc_kind::ITEM_PILE {
        (memory.ptr(tile + rules.item_pile? as u64).filter(|&o| o != 0)?, true)
    } else {
        let (slot, _, _) = single_slots(rules).into_iter().find(|&(_, k, _)| k == kind)?;
        (memory.ptr(tile + slot as u64).filter(|&o| o != 0)?, false)
    };
    if !pile && memory.u64(object + s.tag as u64).map(|tag| loc_id(rules, tag)) != Some(id) {
        return None;
    }
    let d = &rules.decoration;
    let field = |offset: u32| memory.i32(object + offset as u64);
    let at = Placement { x: field(d.x)?, y: field(d.y)?, height: field(d.height)?, orientation: 0 };
    let renderables = slots(&memory, rules, object)
        .into_iter()
        .enumerate()
        .filter_map(|(i, offset)| {
            let renderable = memory.ptr(object + offset as u64).filter(|&r| r != 0)?;
            // wall decorations are drawn moved by their offsets, the second model by the second pair
            let at = if kind == loc_kind::WALL_DECORATION {
                let shift = i as u32 * 8;
                Placement { x: at.x + field(d.x_offset + shift)?, y: at.y + field(d.y_offset + shift)?, ..at }
            } else {
                at
            };
            Some((renderable, at))
        })
        .collect();
    Some(renderables)
}
