use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use ipc::{
    read::{self, Memory},
    state::{
        game_state, loc_status, locs_request, LocInfo, LocsReply, LOC_CLICKABLE, LOC_NAME_SIZE,
        LOC_PLANE_MASK, LOC_PLANE_SHIFT, MAX_LOCS, MAX_LOC_NAMES, NO_NAME,
    },
};

use super::{
    detour::function,
    hooks::client_image,
    model::Placement,
    offsets::{
        loc_kind, render_slots, DecorationLayout, GameObjectLayout, LocConfigs, LocNames, Scene,
    },
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::mapping,
};

const TAG_TYPE_SHIFT: u64 = 16;
const TAG_LOC: u64 = 2;
const TAG_NOT_CLICKABLE: u64 = 1 << 19;
const ENTRY_SIZE: u64 = 0x10;
const ENTRY_OBJECT: u64 = 8;
const MAX_GAME_OBJECTS: i32 = 64;
const MAX_SCENE_SIZE: i32 = 1024;
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
        scene: memory.ptr(world_view + rules.scene.scene as u64)?,
        plane: memory.i32(world_view + rules.plane as u64)?,
        base_x: memory.i32(world_view + base)?,
        base_y: memory.i32(world_view + base + 4)?,
    })
}

struct Tiles {
    plane: i32,
    size_x: i32,
    size_y: i32,
    base: u64,
}

impl Tiles {
    fn new(memory: &OwnMemory, rules: &Rules, scene: u64, plane: i32) -> Option<Self> {
        let s = &rules.scene;
        let (size_x, size_y) = (
            memory.i32(scene + s.size_x as u64)?,
            memory.i32(scene + s.size_y as u64)?,
        );
        let base = memory.ptr(scene + s.tiles as u64)?;
        Some(Self {
            plane,
            size_x,
            size_y,
            base,
        })
    }

    fn at(&self, memory: &OwnMemory, x: i32, y: i32) -> Option<u64> {
        if !(0..4).contains(&self.plane)
            || !(0..self.size_x).contains(&x)
            || !(0..self.size_y).contains(&y)
        {
            return None;
        }
        let index = ((self.plane * self.size_x + x) * self.size_y + y) as u64;
        memory.ptr(self.base + index * ENTRY_SIZE + ENTRY_OBJECT)
    }

    fn objects(&self, memory: &OwnMemory) -> Option<Vec<u64>> {
        if !(0..4).contains(&self.plane)
            || !(0..=MAX_SCENE_SIZE).contains(&self.size_x)
            || !(0..=MAX_SCENE_SIZE).contains(&self.size_y)
        {
            return None;
        }
        let count = (self.size_x * self.size_y) as usize;
        let mut entries = vec![0u8; count * ENTRY_SIZE as usize];
        let first = self.base + self.plane as u64 * count as u64 * ENTRY_SIZE;
        if count > 0 && !memory.read(first, &mut entries) {
            return None;
        }
        Some(
            entries
                .as_chunks::<{ ENTRY_SIZE as usize }>()
                .0
                .iter()
                .map(|entry| Fields(entry).u64(ENTRY_OBJECT as u32).unwrap_or(0))
                .collect(),
        )
    }
}

struct Fields<'a>(&'a [u8]);

impl Fields<'_> {
    fn bytes<const N: usize>(&self, at: u32) -> Option<[u8; N]> {
        self.0.get(at as usize..at as usize + N)?.try_into().ok()
    }

    fn u64(&self, at: u32) -> Option<u64> {
        self.bytes(at).map(u64::from_le_bytes)
    }

    fn ptr(&self, at: u32) -> Option<u64> {
        self.u64(at).filter(|&p| p != 0)
    }

    fn i32(&self, at: u32) -> Option<i32> {
        self.bytes(at).map(i32::from_le_bytes)
    }

    fn u8(&self, at: u32) -> Option<u8> {
        self.bytes(at).map(|[b]| b)
    }
}

fn read_fields<'a>(memory: &OwnMemory, at: u64, block: &'a mut [u8]) -> Option<Fields<'a>> {
    memory.read(at, block).then_some(Fields(block))
}

struct Blocks {
    tile: Vec<u8>,
    slot: Vec<u8>,
    game: Vec<u8>,
}

impl Blocks {
    fn new(rules: &Rules) -> Self {
        let s = &rules.scene;
        let span = |ends: &[Option<u32>]| {
            vec![0u8; ends.iter().flatten().max().copied().unwrap_or(0) as usize]
        };
        Self {
            tile: span(&[
                Some(s.wall + 8),
                Some(s.wall_decoration + 8),
                Some(s.ground_decoration + 8),
                Some(s.game_count + 4),
                Some(s.game_array + 8),
                rules.item_pile.map(|pile| pile + 8),
            ]),
            slot: span(&single_slots(rules).map(|(_, _, config)| Some(slot_span(s, config)))),
            game: span(&[
                Some(s.game_tag + 8),
                Some(s.game_origin_x + 8),
                Some(s.game_origin_y + 8),
                Some(rules.game.config + 4),
            ]),
        }
    }
}

fn shape_rotation(config: i32) -> (u8, u8) {
    (
        (config & SHAPE_MASK) as u8,
        ((config >> ROTATION_SHIFT) & 3) as u8,
    )
}

fn loc_id(rules: &Rules, tag: u64) -> i32 {
    ((tag >> rules.scene.id_shift) & 0xffff) as i32
}

fn slot_span(scene: &Scene, config: Option<u32>) -> u32 {
    (scene.tag + 8).max(config.map_or(0, |c| c + 1))
}

fn single_slots(rules: &Rules) -> [(u32, u8, Option<u32>); 3] {
    let s = &rules.scene;
    let c = rules.configs;
    [
        (s.wall, loc_kind::WALL, c.map(|c| c.wall)),
        (
            s.wall_decoration,
            loc_kind::WALL_DECORATION,
            c.map(|c| c.wall_decoration),
        ),
        (
            s.ground_decoration,
            loc_kind::GROUND_DECORATION,
            c.map(|c| c.ground_decoration),
        ),
    ]
}

struct GameObject {
    object: u64,
    tag: u64,
    x1: i32,
    x2: i32,
    y1: i32,
    y2: i32,
    config: i32,
}

fn game_objects(
    memory: &OwnMemory,
    rules: &Rules,
    tile: &Fields,
    block: &mut [u8],
) -> Vec<GameObject> {
    let s = &rules.scene;
    let count = tile
        .i32(s.game_count)
        .unwrap_or(0)
        .clamp(0, MAX_GAME_OBJECTS) as usize;
    let Some(array) = tile.ptr(s.game_array) else {
        return Vec::new();
    };
    let mut entries = vec![0u8; count * ENTRY_SIZE as usize];
    if count > 0 && !memory.read(array, &mut entries) {
        return Vec::new();
    }
    entries
        .as_chunks::<{ ENTRY_SIZE as usize }>()
        .0
        .iter()
        .filter_map(|entry| Fields(entry).ptr(ENTRY_OBJECT as u32))
        .filter_map(|object| {
            let fields = read_fields(memory, object, block)?;
            let tag = fields.u64(s.game_tag)?;
            if (tag >> TAG_TYPE_SHIFT) & 7 != TAG_LOC {
                return None;
            }
            Some(GameObject {
                object,
                tag,
                x1: fields.i32(s.game_origin_x)?,
                x2: fields.i32(s.game_origin_x + 4)?,
                y1: fields.i32(s.game_origin_y)?,
                y2: fields.i32(s.game_origin_y + 4)?,
                config: fields.i32(rules.game.config).unwrap_or(0),
            })
        })
        .collect()
}

unsafe fn definition(memory: &OwnMemory, rules: &Rules, id: i32) -> Option<u64> {
    let lookup: Lookup = function(rules.module_base + rules.names.lookup as u64);
    memory.ptr(lookup(id) as u64 + 8)
}

unsafe fn decoration_height(memory: &OwnMemory, rules: &Rules, id: i32) -> i16 {
    let height = rules
        .decoration_height
        .and_then(|field| memory.i32(definition(memory, rules, id)? + field as u64));
    height.unwrap_or(0).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

unsafe fn name(memory: &OwnMemory, rules: &Rules, id: i32) -> Option<String> {
    let n = &rules.names;
    let transform: Transform = function(rules.module_base + n.transform as u64);
    let mut definition = definition(memory, rules, id)?;
    if memory
        .u64(definition + n.transforms as u64)
        .is_some_and(|t| t != 0)
    {
        let pair = transform(definition);
        definition = memory.ptr(pair as u64 + 8)?;
    }
    read::read_string(memory, definition + n.name as u64)
        .filter(|name| !name.is_empty() && name != NULL_NAME)
}

fn fill_locs(reply: &mut LocsReply, request: i32) -> i32 {
    let memory = OwnMemory;
    let Some(layout) = LAYOUT.get() else {
        return loc_status::NOT_FOUND;
    };
    let Some(rules) = RULES.get() else {
        return loc_status::NOT_FOUND;
    };
    if on_other_thread() {
        return loc_status::OTHER_THREAD;
    }
    if read::current_game_state(&memory, layout) != Some(game_state::LOGGED_IN) {
        return loc_status::NOT_IN_GAME;
    }
    let Some(view) = view(&memory, rules) else {
        return loc_status::NOT_IN_GAME;
    };
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
        info.flags = if clickable { LOC_CLICKABLE } else { 0 }
            | (plane as u8 & LOC_PLANE_MASK) << LOC_PLANE_SHIFT;
        info.name = *names.entry(info.id).or_insert_with(|| {
            match unsafe { name(&memory, rules, info.id) } {
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
            }
        });
        reply.locs[count] = info;
        count += 1;
    };
    let mut blocks = Blocks::new(rules);
    for plane in planes {
        let Some(tiles) = Tiles::new(&memory, rules, view.scene, plane) else {
            continue;
        };
        let Some(objects) = tiles.objects(&memory) else {
            continue;
        };
        for x in 0..tiles.size_x {
            for y in 0..tiles.size_y {
                let tile = objects[(x * tiles.size_y + y) as usize];
                let Some(tile) = (tile != 0)
                    .then(|| read_fields(&memory, tile, &mut blocks.tile))
                    .flatten()
                else {
                    continue;
                };
                let base = LocInfo {
                    x: x as u8,
                    y: y as u8,
                    size_x: 1,
                    size_y: 1,
                    shape: UNKNOWN,
                    rotation: UNKNOWN,
                    ..LocInfo::default()
                };
                for (slot, kind, config) in single_slots(rules) {
                    let Some(object) = tile.ptr(slot) else {
                        continue;
                    };
                    let block = &mut blocks.slot[..slot_span(s, config) as usize];
                    let Some(fields) = read_fields(&memory, object, block) else {
                        continue;
                    };
                    let Some(tag) = fields.u64(s.tag) else {
                        continue;
                    };
                    let id = loc_id(rules, tag);
                    let decoration_height = match kind {
                        loc_kind::GROUND_DECORATION => *decoration_heights
                            .entry(id)
                            .or_insert_with(|| unsafe { decoration_height(&memory, rules, id) }),
                        _ => 0,
                    };
                    let config = config.and_then(|c| fields.u8(c));
                    let (shape, rotation) =
                        config.map_or((UNKNOWN, UNKNOWN), |c| shape_rotation(c as i32));
                    push(
                        reply,
                        LocInfo {
                            kind,
                            decoration_height,
                            shape,
                            rotation,
                            ..base
                        },
                        tag,
                        plane,
                    );
                }
                for object in game_objects(&memory, rules, &tile, &mut blocks.game) {
                    if object.x1 != x || object.y1 != y {
                        continue;
                    }
                    let (shape, rotation) = shape_rotation(object.config);
                    push(
                        reply,
                        LocInfo {
                            kind: loc_kind::GAME,
                            size_x: (object.x2 - object.x1 + 1).clamp(1, 255) as u8,
                            size_y: (object.y2 - object.y1 + 1).clamp(1, 255) as u8,
                            shape,
                            rotation,
                            ..base
                        },
                        object.tag,
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
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_locs_request() else {
        return;
    };
    shared.write_locs_reply_with(|reply| {
        reply.sequence = sequence;
        reply.count = 0;
        reply.name_count = 0;
        reply.status = fill_locs(reply, request);
    });
}

fn slots(memory: &OwnMemory, rules: &Rules, object: u64) -> Vec<u32> {
    let Some(vtable) = memory.ptr(object) else {
        return Vec::new();
    };
    let mut cache = SLOTS.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    cache
        .entry(vtable)
        .or_insert_with(|| {
            let first = memory.ptr(vtable).unwrap_or(0);
            match unsafe { client_image() } {
                Some(image) if first > rules.module_base => {
                    render_slots(image, (first - rules.module_base) as u32)
                }
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
    let mut blocks = Blocks::new(rules);
    let tile = read_fields(&memory, tile, &mut blocks.tile)?;
    let s = &rules.scene;
    if kind == loc_kind::GAME {
        let g = &rules.game;
        let object = game_objects(&memory, rules, &tile, &mut blocks.game)
            .into_iter()
            .find(|object| loc_id(rules, object.tag) == id)?
            .object;
        let field = |offset: u32| memory.i32(object + offset as u64);
        let at = Placement {
            x: field(g.x)?,
            y: field(g.y)?,
            height: field(g.height)?,
            orientation: field(g.orientation)?,
        };
        let renderable = memory.ptr(object + g.renderable as u64)?;
        return Some(vec![(renderable, at)]);
    }
    let object = if kind == loc_kind::ITEM_PILE {
        tile.ptr(rules.item_pile?)?
    } else {
        let (slot, _, _) = single_slots(rules)
            .into_iter()
            .find(|&(_, k, _)| k == kind)?;
        let object = tile.ptr(slot)?;
        if memory
            .u64(object + s.tag as u64)
            .map(|tag| loc_id(rules, tag))
            != Some(id)
        {
            return None;
        }
        object
    };
    let d = &rules.decoration;
    let field = |offset: u32| memory.i32(object + offset as u64);
    let at = Placement {
        x: field(d.x)?,
        y: field(d.y)?,
        height: field(d.height)?,
        orientation: 0,
    };
    let renderables = slots(&memory, rules, object)
        .into_iter()
        .enumerate()
        .filter_map(|(i, offset)| {
            let renderable = memory.ptr(object + offset as u64)?;
            let at = if kind == loc_kind::WALL_DECORATION {
                let shift = i as u32 * 8;
                Placement {
                    x: at.x + field(d.x_offset + shift)?,
                    y: at.y + field(d.y_offset + shift)?,
                    ..at
                }
            } else {
                at
            };
            Some((renderable, at))
        })
        .collect();
    Some(renderables)
}
