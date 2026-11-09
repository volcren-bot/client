use ipc::read::{Table, local_player, table_get};
use ipc::state::{Layout, WorldLayout};
use serde::{Deserialize, Serialize};

use crate::memory::{Memory, Snapshot, TABLE_BUCKETS, TABLE_COUNT, byte, i32_pair, read_string};
use crate::state::view_base;

const MAX_CHAIN: usize = 64;
const MAX_ITEMS: u64 = 4096;
const MAX_ACTORS: i32 = 4096;
const MAX_VARCS: u64 = 1 << 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub slot: i32,
    pub id: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundItem {
    pub id: i32,
    pub quantity: i32,
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub scene_x: i32,
    pub scene_y: i32,
}

const SCENE_TILES: i32 = 104;
const LIST_SIZE: u64 = 24;
const MAX_PILE: usize = 256;

pub fn ground_items(memory: &impl Memory, layout: &Layout) -> Option<Vec<GroundItem>> {
    let (view, _) = local_player(memory, layout)?;
    ground_items_in(memory, layout, view)
}

fn ground_items_in(memory: &impl Memory, layout: &Layout, view: u64) -> Option<Vec<GroundItem>> {
    let g = &layout.ground_items;
    let p = &layout.projection;
    let plane = memory.i32(view + p.plane as u64)?;
    let (base_x, base_y) = view_base(memory, layout, view)?;
    let scene = memory.ptr(view + p.scene as u64);
    let size = |offset: u32| {
        scene
            .and_then(|s| memory.i32(s + offset as u64))
            .filter(|n| (1..=256).contains(n))
            .unwrap_or(SCENE_TILES)
    };
    let (size_x, size_y) = (size(p.size_x), size(p.size_y));
    if !(0..4).contains(&plane) {
        return None;
    }
    let level =
        |array: u64, index: i32| memory.ptr(array + index as u64 * LIST_SIZE + g.level as u64);
    let columns = level(memory.ptr(view + g.items as u64)?, plane)?;
    let mut items = Vec::new();
    let mut heads = vec![0u8; size_y as usize * LIST_SIZE as usize];
    for x in 0..size_x {
        let Some(column) = level(columns, x) else {
            continue;
        };
        if !memory.read(column, &mut heads) {
            continue;
        }
        for y in 0..size_y {
            let list = column + y as u64 * LIST_SIZE;
            let at = y as usize * LIST_SIZE as usize;
            let mut node = u64::from_le_bytes(heads[at..at + 8].try_into().unwrap());
            for _ in 0..MAX_PILE {
                if node == 0 || node == list {
                    break;
                }
                let Some(item) = memory.ptr(node + g.node_item as u64) else {
                    break;
                };
                let Some((id, quantity)) = i32_pair(memory, item + g.id as u64) else {
                    break;
                };
                items.push(GroundItem {
                    id,
                    quantity,
                    name: None,
                    x: base_x + x,
                    y: base_y + y,
                    plane,
                    scene_x: x,
                    scene_y: y,
                });
                let Some(next) = memory.u64(node) else { break };
                node = next;
            }
        }
    }
    Some(items)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Npc {
    pub index: i32,
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub local_x: i32,
    pub local_y: i32,
    pub animation: i32,
    pub height: i32,
    pub pose: i32,
    pub graphic: i32,
    pub interacting_npc: i32,
    pub interacting_player: i32,
    pub head_icons: Vec<HeadIcon>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadIcon {
    pub archive: i32,
    pub sprite: i32,
}

const MAX_HEAD_ICONS: u64 = 8;

fn head_icons(memory: &impl Memory, layout: &WorldLayout, object: u64) -> Vec<HeadIcon> {
    let sprites = object + layout.head_icon_sprites as u64;
    let (Some(archives), Some(sprites)) = (
        ints(memory, object, MAX_HEAD_ICONS),
        ints(memory, sprites, MAX_HEAD_ICONS),
    ) else {
        return Vec::new();
    };
    archives
        .into_iter()
        .zip(sprites)
        .filter(|&(archive, sprite)| archive >= 0 && sprite >= 0)
        .map(|(archive, sprite)| HeadIcon { archive, sprite })
        .collect()
}

const ACTOR_HEIGHT: u64 = 0x18;
const ACTOR_POSE: u64 = 0x4b8;
const ACTOR_SPOT_ANIM: u64 = 0x4c8;
const SPOT_ANIM_KIND: u64 = 0x8;
const SPOT_ANIM_ID: u64 = 0x20;
const SPOT_ANIM_KIND_VALUE: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub index: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub local_x: i32,
    pub local_y: i32,
    pub height: i32,
    pub animation: i32,
    pub pose: i32,
    pub graphic: i32,
    pub target_state: i32,
    pub target_npc: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub interacting_npc: i32,
    pub interacting_player: i32,
}

const PLAYER_TARGET_STATE: u64 = 0x3a4;
const PLAYER_TARGET_NPC: u64 = 0x3a8;
const PLAYER_TARGET_TILE: u64 = 0x3b0;

fn interacting(memory: &impl Memory, actor: u64) -> (i32, i32) {
    let kind = memory
        .i32(actor + PLAYER_TARGET_STATE)
        .map_or(0, |s| s >> 24);
    let index = memory
        .i32(actor + PLAYER_TARGET_NPC)
        .filter(|&i| i >= 0)
        .unwrap_or(-1);
    match kind {
        1 => (index, -1),
        2 => (-1, index),
        _ => (-1, -1),
    }
}

fn player_target(memory: &impl Memory, player: u64) -> (i32, i32, i32, i32) {
    let state = memory
        .i32(player + PLAYER_TARGET_STATE)
        .filter(|s| s >> 24 == 1 && s & 0xff == 0)
        .map(|s| (s >> 8) & 0xffff)
        .unwrap_or(-1);
    let npc = memory.i32(player + PLAYER_TARGET_NPC).unwrap_or(-1);
    let (x, y) = match state {
        1 => (
            memory.i32(player + PLAYER_TARGET_TILE).unwrap_or(-1),
            memory.i32(player + PLAYER_TARGET_TILE + 4).unwrap_or(-1),
        ),
        _ => (-1, -1),
    };
    (state, npc, x, y)
}

const MAX_OBJECT_SPAN: u64 = 0x1000;

fn span(fields: &[(u64, u64)]) -> usize {
    let end = fields
        .iter()
        .map(|&(offset, size)| offset + size)
        .max()
        .unwrap_or(0);
    if end > MAX_OBJECT_SPAN {
        0
    } else {
        end as usize
    }
}

fn actor_span(layout: &WorldLayout, own: &[(u64, u64)]) -> usize {
    let shared = [
        (ACTOR_HEIGHT, 4),
        (ACTOR_POSE, 4),
        (ACTOR_SPOT_ANIM, 8),
        (PLAYER_TARGET_STATE, 4),
        (PLAYER_TARGET_NPC, 4),
        (layout.npc_animation as u64, 4),
        (layout.local_x as u64, 8),
    ];
    span(&[&shared[..], own].concat())
}

fn npc_fields(layout: &WorldLayout) -> [(u64, u64); 3] {
    [
        (layout.npc_definition as u64, 8),
        (layout.npc_name_override as u64, 24),
        (layout.npc_head_icons as u64, 8),
    ]
}

fn player_fields(layout: &WorldLayout) -> [(u64, u64); 2] {
    [(layout.player_name as u64, 8), (PLAYER_TARGET_TILE, 8)]
}

fn int_vector(memory: &impl Memory, at: u64) -> Option<(u64, u64)> {
    let (begin, end) = (memory.u64(at)?, memory.u64(at + 8)?);
    Some((begin, end.checked_sub(begin)? / 4))
}

fn ints(memory: &impl Memory, at: u64, max: u64) -> Option<Vec<i32>> {
    let (begin, count) = int_vector(memory, at)?;
    if count > max {
        return None;
    }
    memory.i32s(begin, count as usize)
}

fn client(memory: &impl Memory, layout: &WorldLayout) -> Option<u64> {
    memory.ptr(layout.module_base + layout.client as u64)
}

fn container_items(memory: &impl Memory, layout: &WorldLayout, node: u64) -> Option<Vec<Item>> {
    let ids = ints(memory, node + layout.container_ids as u64, MAX_ITEMS)?;
    let quantities = ints(memory, node + layout.container_quantities as u64, MAX_ITEMS)?;
    Some(
        ids.iter()
            .enumerate()
            .map(|(slot, &item)| Item {
                slot: slot as i32,
                id: item,
                quantity: quantities.get(slot).copied().unwrap_or(0),
            })
            .collect(),
    )
}

fn containers(memory: &impl Memory, layout: &WorldLayout) -> Option<(u64, u64, u64)> {
    let table = layout.module_base + layout.containers as u64;
    let buckets = memory.ptr(table + TABLE_BUCKETS)?;
    let count = memory.u64(table + TABLE_COUNT)?;
    if count == 0 || count > MAX_ITEMS {
        return None;
    }
    Some((buckets, count, memory.u64(buckets + count * 8)?))
}

fn container_node(memory: &impl Memory, layout: &WorldLayout, id: i32) -> Option<u64> {
    if id < 0 {
        return None;
    }
    let (buckets, count, end) = containers(memory, layout)?;
    let mut node = memory.u64(buckets + (id as u64 % count) * 8)?;
    for _ in 0..MAX_CHAIN {
        if node == 0 || node == end {
            return None;
        }
        if memory.i32(node)? == id {
            return Some(node);
        }
        node = memory.u64(node + layout.container_next as u64)?;
    }
    None
}

pub fn item_container(memory: &impl Memory, layout: &WorldLayout, id: i32) -> Option<Vec<Item>> {
    container_items(memory, layout, container_node(memory, layout, id)?)
}

pub fn item_slot(memory: &impl Memory, layout: &WorldLayout, id: i32, slot: i32) -> Option<Item> {
    let node = container_node(memory, layout, id)?;
    let read = |vector: u32| -> Option<i32> {
        let (begin, length) = int_vector(memory, node + vector as u64)?;
        let index = u64::try_from(slot)
            .ok()
            .filter(|&index| index < length && index < MAX_ITEMS)?;
        memory.i32(begin + index * 4)
    };
    Some(Item {
        slot,
        id: read(layout.container_ids).unwrap_or(-1),
        quantity: read(layout.container_quantities).unwrap_or(0),
    })
}

pub fn item_containers(
    memory: &impl Memory,
    layout: &WorldLayout,
) -> Option<Vec<(i32, Vec<Item>)>> {
    let (buckets, count, end) = containers(memory, layout)?;
    let mut seen = Vec::new();
    let mut found = Vec::new();
    for bucket in 0..count {
        let mut node = memory.u64(buckets + bucket * 8)?;
        for _ in 0..MAX_CHAIN {
            if node == 0 || node == end || seen.contains(&node) {
                break;
            }
            seen.push(node);
            let id = memory.i32(node)?;
            if !found.iter().any(|(known, _)| *known == id) {
                found.push((id, container_items(memory, layout, node)?));
            }
            node = memory.u64(node + layout.container_next as u64)?;
        }
    }
    found.sort_by_key(|(id, _)| *id);
    Some(found)
}

pub fn varc(memory: &impl Memory, layout: &WorldLayout, id: i32) -> Option<i32> {
    let varcs = memory.ptr(client(memory, layout)? + layout.varcs as u64)?;
    let begin = memory.u64(varcs + layout.varc_begin as u64)?;
    let end = memory.u64(varcs + layout.varc_begin as u64 + 8)?;
    let size = layout.varc_entry_size as u64;
    if size == 0 {
        return None;
    }
    let count = end.checked_sub(begin)? / size;
    if count > MAX_VARCS {
        return None;
    }
    let (mut low, mut high) = (0u64, count);
    while low < high {
        let mid = (low + high) / 2;
        let entry = begin + mid * size;
        let key = memory.i32(entry)?;
        if key < id {
            low = mid + 1;
        } else if key > id {
            high = mid;
        } else {
            let kind = byte(memory, entry + layout.varc_kind as u64)?;
            return (kind == 1)
                .then(|| memory.i32(entry + layout.varc_value as u64))
                .flatten();
        }
    }
    None
}

fn world_view(
    memory: &impl Memory,
    layout: &WorldLayout,
    client: u64,
) -> Option<(u64, (i32, i32))> {
    let id = memory.i32(client + layout.world_view_id as u64)?;
    let view = table_get(memory, client + layout.world_views as u64, id)?;
    Some((view, i32_pair(memory, view + layout.base_x as u64)?))
}

struct Actor {
    index: i32,
    x: i32,
    y: i32,
    local_x: i32,
    local_y: i32,
    animation: i32,
    height: i32,
    pose: i32,
    graphic: i32,
    interacting_npc: i32,
    interacting_player: i32,
}

fn read_actor(
    memory: &impl Memory,
    layout: &WorldLayout,
    actor: u64,
    index: i32,
    (base_x, base_y): (i32, i32),
) -> Option<Actor> {
    let (local_x, local_y) = i32_pair(memory, actor + layout.local_x as u64)?;
    let graphic = memory
        .ptr(actor + ACTOR_SPOT_ANIM)
        .filter(|&spot| memory.i32(spot + SPOT_ANIM_KIND) == Some(SPOT_ANIM_KIND_VALUE))
        .and_then(|spot| memory.i32(spot + SPOT_ANIM_ID))
        .unwrap_or(-1);
    let (interacting_npc, interacting_player) = interacting(memory, actor);
    Some(Actor {
        index,
        x: base_x + (local_x >> 7),
        y: base_y + (local_y >> 7),
        local_x,
        local_y,
        animation: memory
            .i32(actor + layout.npc_animation as u64)
            .unwrap_or(-1),
        height: memory
            .i32(actor + ACTOR_HEIGHT)
            .filter(|h| (1..=1000).contains(h))
            .unwrap_or(-1),
        pose: memory.i32(actor + ACTOR_POSE).unwrap_or(-1),
        graphic,
        interacting_npc,
        interacting_player,
    })
}

fn actors<M: Memory, T>(
    memory: &M,
    layout: &WorldLayout,
    (table, indices, count): (u64, u64, i32),
    base: (i32, i32),
    own: &[(u64, u64)],
    mut decode: impl FnMut(&Snapshot<'_, M>, u64, Actor) -> T,
) -> Option<Vec<T>> {
    if !(0..=MAX_ACTORS).contains(&count) {
        return None;
    }
    let table = Table::open(memory, table);
    let mut list = Snapshot::new(memory, count as usize * 4);
    list.load(indices);
    let mut snapshot = Snapshot::new(memory, actor_span(layout, own));
    let mut found = Vec::with_capacity(count as usize);
    for i in 0..count as u64 {
        let Some(index) = list.i32(indices + i * 4) else {
            continue;
        };
        let Some(address) = table.and_then(|t| t.get(memory, index)) else {
            continue;
        };
        snapshot.load(address);
        if let Some(actor) = read_actor(&snapshot, layout, address, index, base) {
            found.push(decode(&snapshot, address, actor));
        }
    }
    Some(found)
}

pub fn npcs(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<Npc>> {
    let (view, base) = world_view(memory, layout, client(memory, layout)?)?;
    let indices = memory.ptr(view + layout.npc_indices as u64)?;
    let count = memory.i32(view + layout.npc_indices as u64 + 8)?;
    let list = (view + layout.npc_table as u64, indices, count);
    actors(
        memory,
        layout,
        list,
        base,
        &npc_fields(layout),
        |m, npc, a| {
            let definition = m.ptr(npc + layout.npc_definition as u64);
            let id = definition
                .and_then(|d| m.i32(d + layout.definition_id as u64))
                .unwrap_or(-1);
            let name = read_string(m, npc + layout.npc_name_override as u64)
                .filter(|n| !n.is_empty())
                .or_else(|| {
                    definition.and_then(|d| read_string(m, d + layout.definition_name as u64))
                })
                .unwrap_or_default();
            let head_icons = (layout.npc_head_icons != 0)
                .then(|| m.ptr(npc + layout.npc_head_icons as u64))
                .flatten()
                .map(|object| head_icons(memory, layout, object))
                .unwrap_or_default();
            Npc {
                index: a.index,
                id,
                name,
                x: a.x,
                y: a.y,
                local_x: a.local_x,
                local_y: a.local_y,
                animation: a.animation,
                height: a.height,
                pose: a.pose,
                graphic: a.graphic,
                interacting_npc: a.interacting_npc,
                interacting_player: a.interacting_player,
                head_icons,
            }
        },
    )
}

const DEFINITION_SIZE: u64 = 0x20;
const MAX_NPC_SIZE: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hitsplat {
    pub slot: i32,
    pub kind: i32,
    pub damage: i32,
    pub cycle: i32,
}

const ACTOR_HITSPLATS: u64 = 0x358;
const HITSPLAT_SIZE: u64 = 12;
const HITSPLAT_SLOTS: u64 = 4;

fn actor_at(memory: &impl Memory, layout: &WorldLayout, table: u32, index: i32) -> Option<u64> {
    let (view, _) = world_view(memory, layout, client(memory, layout)?)?;
    table_get(memory, view + table as u64, index)
}

pub fn npc_hitsplats(
    memory: &impl Memory,
    layout: &WorldLayout,
    index: i32,
) -> Option<Vec<Hitsplat>> {
    actor_hitsplats(memory, actor_at(memory, layout, layout.npc_table, index)?)
}

pub fn player_hitsplats(
    memory: &impl Memory,
    layout: &WorldLayout,
    index: i32,
) -> Option<Vec<Hitsplat>> {
    actor_hitsplats(memory, actor_at(memory, layout, layout.players, index)?)
}

fn actor_hitsplats(memory: &impl Memory, actor: u64) -> Option<Vec<Hitsplat>> {
    let mut splats = Vec::new();
    for slot in 0..HITSPLAT_SLOTS {
        let at = actor + ACTOR_HITSPLATS + slot * HITSPLAT_SIZE;
        let (kind, damage, cycle) = (memory.i32(at)?, memory.i32(at + 4)?, memory.i32(at + 8)?);
        if cycle != -1 && kind != -1 {
            splats.push(Hitsplat {
                slot: slot as i32,
                kind,
                damage,
                cycle,
            });
        }
    }
    Some(splats)
}

pub fn npc_size(memory: &impl Memory, layout: &WorldLayout, index: i32) -> Option<i32> {
    let npc = actor_at(memory, layout, layout.npc_table, index)?;
    let definition = memory.ptr(npc + layout.npc_definition as u64)?;
    memory
        .i32(definition + DEFINITION_SIZE)
        .filter(|s| (1..=MAX_NPC_SIZE).contains(s))
}

pub fn npc_words(
    memory: &impl Memory,
    layout: &WorldLayout,
    index: i32,
    count: usize,
    definition: bool,
) -> Option<(u64, Vec<i32>)> {
    let npc = actor_at(memory, layout, layout.npc_table, index)?;
    let at = if definition {
        memory.ptr(npc + layout.npc_definition as u64)?
    } else {
        npc
    };
    Some((at, memory.i32s(at, count)?))
}

pub fn player_words(
    memory: &impl Memory,
    layout: &WorldLayout,
    index: i32,
    count: usize,
) -> Option<(u64, Vec<i32>)> {
    let player = actor_at(memory, layout, layout.players, index)?;
    Some((player, memory.i32s(player, count)?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldEntity {
    pub view_id: i32,
    pub config_id: i32,
    pub local_x: i32,
    pub local_y: i32,
}

const TOP_LEVEL_VIEW: i32 = 0;
const SCENE_LIMIT: i32 = SCENE_TILES << 7;
const VIEW_WORLD_ENTITIES: u64 = 0xe0;
const ENTITY_VIEW_ID: u64 = 0x2c;
const ENTITY_CONFIG: u64 = 0x38;
const ENTITY_VIEW: u64 = 0x80;
const ENTITY_LOCAL_X: u64 = 0x8c;

pub fn world_entities(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<WorldEntity>> {
    let client = client(memory, layout)?;
    let views = client + layout.world_views as u64;
    let top = table_get(memory, views, TOP_LEVEL_VIEW)?;
    Some(
        ipc::read::table_entries(memory, top + VIEW_WORLD_ENTITIES)
            .into_iter()
            .filter_map(|(view_id, entity)| {
                if memory.i32(entity + ENTITY_VIEW_ID)? != view_id
                    || memory.ptr(entity + ENTITY_VIEW)? != table_get(memory, views, view_id)?
                {
                    return None;
                }
                let (local_x, local_y) = i32_pair(memory, entity + ENTITY_LOCAL_X)?;
                if !(0..SCENE_LIMIT).contains(&local_x) || !(0..SCENE_LIMIT).contains(&local_y) {
                    return None;
                }
                let config_id = memory
                    .ptr(entity + ENTITY_CONFIG)
                    .and_then(|config| memory.i32(config))
                    .unwrap_or(-1);
                Some(WorldEntity {
                    view_id,
                    config_id,
                    local_x,
                    local_y,
                })
            })
            .collect(),
    )
}

pub fn world_debug(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<String>> {
    let client = client(memory, layout)?;
    let mut lines = Vec::new();
    let mine = memory.i32(client + layout.world_view_id as u64)?;
    lines.push(format!("own world view id {mine}"));
    for (id, view) in ipc::read::table_entries(memory, client + layout.world_views as u64) {
        let (bx, by) = (
            memory.i32(view + layout.base_x as u64),
            memory.i32(view + layout.base_x as u64 + 4),
        );
        lines.push(format!("view id {id} at {view:#x} base {bx:?} {by:?}"));
    }
    let view = table_get(memory, client + layout.world_views as u64, TOP_LEVEL_VIEW)?;
    for offset in (0..0x4000u64).step_by(8) {
        let entries = ipc::read::table_entries(memory, view + offset);
        if !entries.is_empty() {
            let keys: Vec<_> = entries
                .iter()
                .take(4)
                .map(|e| (e.0, format!("{:#x}", e.1)))
                .collect();
            lines.push(format!(
                "table at +{offset:#x}: {} entries, keys {keys:?}",
                entries.len()
            ));
        }
    }
    Some(lines)
}

pub fn players(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<Player>> {
    let client = client(memory, layout)?;
    let (view, base) = world_view(memory, layout, client)?;
    let count = memory.i32(client + layout.player_count as u64)?;
    let list = (
        view + layout.players as u64,
        client + layout.player_count as u64 + 4,
        count,
    );
    actors(
        memory,
        layout,
        list,
        base,
        &player_fields(layout),
        |m, player, a| {
            let name = m
                .ptr(player + layout.player_name as u64)
                .and_then(|n| read_string(m, n))
                .unwrap_or_default()
                .replace('\u{a0}', " ");
            let (target_state, target_npc, target_x, target_y) = player_target(m, player);
            Player {
                index: a.index,
                name,
                x: a.x,
                y: a.y,
                local_x: a.local_x,
                local_y: a.local_y,
                height: a.height,
                animation: a.animation,
                pose: a.pose,
                graphic: a.graphic,
                target_state,
                target_npc,
                target_x,
                target_y,
                interacting_npc: a.interacting_npc,
                interacting_player: a.interacting_player,
            }
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projectile {
    pub id: i32,
    pub source_x: i32,
    pub source_y: i32,
    pub source_plane: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub target_plane: i32,
    pub source_npc: i32,
    pub source_player: i32,
    pub target_npc: i32,
    pub target_player: i32,
    pub start_height: i32,
    pub end_height: i32,
    pub slope: i32,
    pub start_pos: i32,
    pub start_cycle: i32,
    pub end_cycle: i32,
    pub cycle: i32,
    pub moving: bool,
    pub x: i32,
    pub y: i32,
    pub local_x: i32,
    pub local_y: i32,
    pub z: i32,
    pub orientation: i32,
    pub height: i32,
}

const MAX_PROJECTILES: u64 = 512;

fn actor_ref(value: i32) -> (i32, i32) {
    match value {
        v if v > 0 => (v - 1, -1),
        v if v < 0 => (-1, -v - 1),
        _ => (-1, -1),
    }
}

const LIST_COUNT: u64 = 0x10;

fn list_objects(
    memory: &impl Memory,
    head: u64,
    object: u64,
    max: u64,
    mut visit: impl FnMut(u64),
) -> Option<()> {
    if memory.u64(head + LIST_COUNT)? > max {
        return None;
    }
    let mut node = memory.u64(head)?;
    for _ in 0..max {
        if node == 0 || node == head {
            break;
        }
        let next = memory.u64(node)?;
        if let Some(at) = memory.ptr(node + object) {
            visit(at);
        }
        node = next;
    }
    Some(())
}

pub fn projectiles(memory: &impl Memory, layout: &Layout) -> Option<Vec<Projectile>> {
    let w = &layout.world;
    let p = &layout.projectiles;
    let client = client(memory, w)?;
    let (_, (base_x, base_y)) = world_view(memory, w, client)?;
    let cycle = memory.i32(layout.module_base + p.cycle as u64)?;
    let fields = [
        p.id,
        p.source_x,
        p.source_y,
        p.source_level,
        p.source_actor,
        p.start_height,
        p.target_x,
        p.target_y,
        p.target_level,
        p.target_actor,
        p.end_height,
        p.start_cycle,
        p.end_cycle,
        p.slope,
        p.start_pos,
        p.x,
        p.y,
        p.z,
        p.orientation,
        p.moving,
    ];
    let mut object = Snapshot::new(memory, span(&fields.map(|offset| (offset as u64, 8))));
    let mut projectiles = Vec::new();
    list_objects(
        memory,
        client + p.list as u64,
        p.node as u64,
        MAX_PROJECTILES,
        |at| {
            object.load(at);
            let m = &object;
            let int = |offset: u32| m.i32(at + offset as u64);
            let double = |offset: u32| {
                m.u64(at + offset as u64)
                    .map(f64::from_bits)
                    .filter(|v| v.is_finite())
                    .map(|v| v as i32)
            };
            let (Some(id), Some(source_x), Some(source_y), Some(target_x), Some(target_y)) = (
                int(p.id),
                int(p.source_x),
                int(p.source_y),
                int(p.target_x),
                int(p.target_y),
            ) else {
                return;
            };
            let (source_npc, source_player) = actor_ref(int(p.source_actor).unwrap_or(0));
            let (target_npc, target_player) = actor_ref(int(p.target_actor).unwrap_or(0));
            let moving = byte(m, at + p.moving as u64).is_some_and(|b| b != 0);
            let (local_x, local_y, z) = match (moving, double(p.x), double(p.y), double(p.z)) {
                (true, Some(x), Some(y), Some(z)) => (x, y, z),
                _ => (
                    ((source_x - base_x) << 7) + 64,
                    ((source_y - base_y) << 7) + 64,
                    0,
                ),
            };
            projectiles.push(Projectile {
                id,
                source_x,
                source_y,
                source_plane: int(p.source_level).unwrap_or(0),
                target_x,
                target_y,
                target_plane: int(p.target_level).unwrap_or(0),
                source_npc,
                source_player,
                target_npc,
                target_player,
                start_height: int(p.start_height).unwrap_or(0),
                end_height: int(p.end_height).unwrap_or(0),
                slope: int(p.slope).unwrap_or(0),
                start_pos: int(p.start_pos).unwrap_or(0),
                start_cycle: int(p.start_cycle).unwrap_or(0),
                end_cycle: int(p.end_cycle).unwrap_or(0),
                cycle,
                moving,
                x: base_x + (local_x >> 7),
                y: base_y + (local_y >> 7),
                local_x,
                local_y,
                z,
                orientation: int(p.orientation).unwrap_or(0),
                height: int(p.start_height).unwrap_or(0),
            });
        },
    )?;
    Some(projectiles)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpotAnim {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub local_x: i32,
    pub local_y: i32,
    pub z: i32,
    pub start_cycle: i32,
    pub cycle: i32,
    pub view_id: i32,
}

const MAX_SPOT_ANIMS: u64 = 512;
const SPOT_ANIM_NODE: u64 = 0x18;

pub fn spot_anims(memory: &impl Memory, layout: &Layout) -> Option<Vec<SpotAnim>> {
    let (own, _) = local_player(memory, layout)?;
    let client = ipc::read::client(memory, layout)?;
    let own_id = memory.i32(client + layout.player.world_view_id as u64)?;
    let mut objects = Vec::new();
    if own_id != TOP_LEVEL_VIEW
        && let Some(top) = table_get(
            memory,
            client + layout.player.world_views as u64,
            TOP_LEVEL_VIEW,
        )
    {
        objects.extend(spot_anims_in(memory, layout, top, TOP_LEVEL_VIEW).unwrap_or_default());
    }
    objects.extend(spot_anims_in(memory, layout, own, own_id)?);
    Some(objects)
}

fn spot_anims_in(
    memory: &impl Memory,
    layout: &Layout,
    view: u64,
    view_id: i32,
) -> Option<Vec<SpotAnim>> {
    let g = &layout.spot_anims;
    let (base_x, base_y) = view_base(memory, layout, view)?;
    let cycle = memory.i32(layout.module_base + g.cycle as u64)?;
    let mut objects = Vec::new();
    list_objects(
        memory,
        view + g.list as u64,
        SPOT_ANIM_NODE,
        MAX_SPOT_ANIMS,
        |at| {
            let int = |offset: u32| memory.i32(at + offset as u64);
            let (Some(id), Some(local_x), Some(local_y)) = (int(g.id), int(g.x), int(g.y)) else {
                return;
            };
            objects.push(SpotAnim {
                id,
                x: base_x + (local_x >> 7),
                y: base_y + (local_y >> 7),
                plane: int(g.level).unwrap_or(0),
                local_x,
                local_y,
                z: int(g.z).unwrap_or(0),
                start_cycle: int(g.start_cycle).unwrap_or(0),
                cycle,
                view_id,
            });
        },
    )?;
    Some(objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;

    const LAYOUT: WorldLayout = WorldLayout {
        module_base: 0x1_4000_0000,
        client: 0x100,
        world_views: 0xc9d0,
        world_view_id: 0xcc50,
        base_x: 0x24,
        players: 0x60,
        npc_table: 0x90,
        npc_indices: 0xd0,
        player_count: 0xccd0,
        local_x: 0x268,
        npc_name_override: 0x710,
        npc_definition: 0x730,
        player_name: 0x718,
        npc_animation: 0x4d8,
        definition_id: 0,
        definition_name: 0x8,
        containers: 0x200,
        container_ids: 0x8,
        container_quantities: 0x20,
        container_next: 0x38,
        varcs: 0x417ca8,
        varc_begin: 0x48,
        varc_entry_size: 0x30,
        varc_value: 0x8,
        varc_kind: 0x28,
        run_energy: 0x34e0,
        weight: 0x34e4,
        npc_head_icons: 0x760,
        head_icon_sprites: 0x48,
        reserved: 0,
    };
    const CLIENT: u64 = 0x10_0000;

    fn grand_exchange() -> Fake {
        let mut m = Fake::default();
        m.put_u64(LAYOUT.module_base + LAYOUT.client as u64, CLIENT);

        let containers = LAYOUT.module_base + LAYOUT.containers as u64;
        let buckets = 0x20_0000;
        m.put_u64(containers + TABLE_BUCKETS, buckets);
        m.put_u64(containers + TABLE_COUNT, 1);
        let (inventory, equipment) = (0x21_0000u64, 0x22_0000u64);
        m.put_u64(buckets, inventory);
        m.put_u64(buckets + 8, 0xdead);
        let vector = |m: &mut Fake, at: u64, data: u64, values: &[i32]| {
            for (i, v) in values.iter().enumerate() {
                m.put_i32(data + i as u64 * 4, *v);
            }
            m.put_u64(at, data);
            m.put_u64(at + 8, data + values.len() as u64 * 4);
        };
        m.put_i32(inventory, 93);
        vector(&mut m, inventory + 8, 0x23_0000, &[-1, 21143, 21143, 27281]);
        vector(&mut m, inventory + 0x20, 0x23_1000, &[0, 1, 1, 1]);
        m.put_u64(inventory + 0x38, equipment);
        m.put_i32(equipment, 94);
        vector(&mut m, equipment + 8, 0x23_2000, &[5554, 13342]);
        vector(&mut m, equipment + 0x20, 0x23_3000, &[1, 1]);
        m.put_u64(equipment + 0x38, 0);

        let varcs = 0x30_0000;
        m.put_u64(CLIENT + LAYOUT.varcs as u64, varcs);
        let entries = 0x31_0000;
        m.put_u64(varcs + 0x48, entries);
        m.put_u64(varcs + 0x50, entries + 3 * 0x30);
        for (i, (id, value, kind)) in [(3, -1, 1u8), (171, 3, 1), (200, 7, 2)]
            .into_iter()
            .enumerate()
        {
            let e = entries + i as u64 * 0x30;
            m.put_i32(e, id);
            m.put_i32(e + 8, value);
            m.put(e + 0x28, &[kind]);
        }

        let view = 0x40_0000;
        m.put_i32(CLIENT + LAYOUT.world_view_id as u64, 0);
        m.put_table(CLIENT + LAYOUT.world_views as u64, 0x41_0000, &[(0, view)]);
        m.put_i32(view + 0x24, 3112);
        m.put_i32(view + 0x28, 3440);

        let (clerk, banker) = (0x50_0000u64, 0x51_0000u64);
        m.put_table(
            view + LAYOUT.npc_table as u64,
            0x52_0000,
            &[(25400, clerk), (25399, banker)],
        );
        let indices = 0x58_0000;
        m.put_u64(view + LAYOUT.npc_indices as u64, indices);
        m.put_i32(view + LAYOUT.npc_indices as u64 + 8, 2);
        m.put_i32(indices, 25400);
        m.put_i32(indices + 4, 25399);
        for (npc, (id, name, x, y)) in [
            (clerk, (2149, "", 6720, 6208)),
            (banker, (6528, "", 6592, 6464)),
        ] {
            m.put_i32(npc + LAYOUT.local_x as u64, x);
            m.put_i32(npc + LAYOUT.local_x as u64 + 4, y);
            m.put_short_string(npc + LAYOUT.npc_name_override as u64, name);
            let definition = npc + 0x8000;
            m.put_u64(npc + LAYOUT.npc_definition as u64, definition);
            m.put_i32(definition, id);
        }
        m.put_short_string(
            clerk + 0x8000 + 8,
            "Grand Exchange Clerk".get(..20).unwrap_or_default(),
        );
        m.put_short_string(banker + 0x8000 + 8, "Banker");
        let long = 0x54_0000;
        m.put(long, b"Grand Exchange Clerk");
        m.put_u64(clerk + 0x8000 + 8, long);
        m.put_u64(clerk + 0x8000 + 0x10, 20);
        m.put_u64(clerk + 0x8000 + 0x18, 0x8000_0000_0000_0020);

        let player = 0x60_0000;
        m.put_table(view + LAYOUT.players as u64, 0x61_0000, &[(780, player)]);
        m.put_i32(CLIENT + LAYOUT.player_count as u64, 1);
        m.put_i32(CLIENT + LAYOUT.player_count as u64 + 4, 780);
        m.put_i32(player + LAYOUT.local_x as u64, 6464);
        m.put_i32(player + LAYOUT.local_x as u64 + 4, 6208);
        let name = 0x68_0000;
        m.put_u64(player + LAYOUT.player_name as u64, name);
        m.put_short_string(name, "gambri\u{a0}jani5");
        m
    }

    #[test]
    fn reads_item_containers() {
        let m = grand_exchange();
        let inventory = item_container(&m, &LAYOUT, 93).unwrap();
        assert_eq!(inventory.len(), 4);
        assert_eq!(
            inventory[0],
            Item {
                slot: 0,
                id: -1,
                quantity: 0
            }
        );
        assert_eq!(
            inventory[3],
            Item {
                slot: 3,
                id: 27281,
                quantity: 1
            }
        );
        assert_eq!(item_container(&m, &LAYOUT, 94).unwrap()[1].id, 13342);
        assert_eq!(item_container(&m, &LAYOUT, 95), None);
        assert_eq!(
            item_slot(&m, &LAYOUT, 93, 3),
            Some(Item {
                slot: 3,
                id: 27281,
                quantity: 1
            })
        );
        assert_eq!(
            item_slot(&m, &LAYOUT, 93, 4),
            Some(Item {
                slot: 4,
                id: -1,
                quantity: 0
            })
        );
        assert_eq!(
            item_slot(&m, &LAYOUT, 93, -1),
            Some(Item {
                slot: -1,
                id: -1,
                quantity: 0
            })
        );
        assert_eq!(item_slot(&m, &LAYOUT, 95, 0), None);
        let all = item_containers(&m, &LAYOUT).unwrap();
        assert_eq!(all.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [93, 94]);
        assert_eq!(all[1].1[1].id, 13342);
    }

    #[test]
    fn reads_varcs() {
        let m = grand_exchange();
        assert_eq!(varc(&m, &LAYOUT, 171), Some(3));
        assert_eq!(varc(&m, &LAYOUT, 3), Some(-1));
        assert_eq!(varc(&m, &LAYOUT, 200), None, "not an integer");
        assert_eq!(varc(&m, &LAYOUT, 172), None, "not set");
    }

    #[test]
    fn reads_npcs_and_players() {
        let m = grand_exchange();
        let npcs = npcs(&m, &LAYOUT).unwrap();
        assert_eq!(npcs.len(), 2);
        assert_eq!(
            npcs[0],
            Npc {
                index: 25400,
                id: 2149,
                name: "Grand Exchange Clerk".into(),
                x: 3164,
                y: 3488,
                local_x: 6720,
                local_y: 6208,
                animation: -1,
                height: -1,
                pose: -1,
                graphic: -1,
                interacting_npc: -1,
                interacting_player: -1,
                head_icons: Vec::new(),
            }
        );
        assert_eq!(
            (npcs[1].id, npcs[1].name.as_str(), npcs[1].x, npcs[1].y),
            (6528, "Banker", 3163, 3490)
        );
        let players = players(&m, &LAYOUT).unwrap();
        assert_eq!(
            players,
            [Player {
                index: 780,
                name: "gambri jani5".into(),
                x: 3162,
                y: 3488,
                local_x: 6464,
                local_y: 6208,
                height: -1,
                animation: -1,
                pose: -1,
                graphic: -1,
                target_state: -1,
                target_npc: -1,
                target_x: -1,
                target_y: -1,
                interacting_npc: -1,
                interacting_player: -1
            }]
        );
    }

    struct Recording<'a> {
        memory: &'a Fake,
        short: bool,
        reads: std::cell::RefCell<Vec<u64>>,
    }

    impl Memory for Recording<'_> {
        fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
            self.reads.borrow_mut().push(address);
            (!self.short || buffer.len() <= 64) && self.memory.read(address, buffer)
        }
    }

    #[test]
    fn actors_are_read_in_one_go_and_decode_the_same() {
        let mut m = grand_exchange();
        let client = m.ptr(LAYOUT.module_base + LAYOUT.client as u64).unwrap();
        let (view, _) = world_view(&m, &LAYOUT, client).unwrap();
        let npc_span = actor_span(&LAYOUT, &npc_fields(&LAYOUT));
        let player_span = actor_span(&LAYOUT, &player_fields(&LAYOUT));
        let npcs_in = ipc::read::table_entries(&m, view + LAYOUT.npc_table as u64)
            .into_iter()
            .map(|(_, npc)| (npc, npc_span));
        let players_in = ipc::read::table_entries(&m, view + LAYOUT.players as u64)
            .into_iter()
            .map(|(_, player)| (player, player_span));
        let actors: Vec<(u64, u64)> = npcs_in
            .chain(players_in)
            .map(|(actor, span)| (actor, actor + span as u64))
            .collect();
        assert_eq!(actors.len(), 3);
        for &(begin, end) in &actors {
            for address in begin..end {
                m.0.get_mut().entry(address).or_insert(0);
            }
        }
        let decode = |short: bool| {
            let memory = Recording {
                memory: &m,
                short,
                reads: Default::default(),
            };
            let decoded = (
                npcs(&memory, &LAYOUT).unwrap(),
                players(&memory, &LAYOUT).unwrap(),
            );
            let reads = memory.reads.into_inner();
            let per_actor: Vec<usize> = actors
                .iter()
                .map(|&(begin, end)| {
                    reads
                        .iter()
                        .filter(|&&at| (begin..end).contains(&at))
                        .count()
                })
                .collect();
            (decoded, per_actor)
        };

        let (at_once, reads_at_once) = decode(false);
        let (field_by_field, reads_field_by_field) = decode(true);
        assert_eq!(at_once, field_by_field);
        assert_eq!((at_once.0.len(), at_once.1.len()), (2, 1));
        assert_eq!(reads_at_once, [1, 1, 1]);
        assert!(
            reads_field_by_field.iter().all(|&reads| reads >= 10),
            "{reads_field_by_field:?}"
        );
    }

    #[test]
    fn reads_projectiles() {
        use ipc::state::ProjectileLayout;
        let mut m = grand_exchange();
        let p = ProjectileLayout {
            list: 0xcc70,
            node: 0x18,
            cycle: 0xe98c14,
            source_level: 0x260,
            source_x: 0x264,
            source_y: 0x268,
            start_height: 0x26c,
            source_actor: 0x270,
            target_level: 0x274,
            target_x: 0x278,
            target_y: 0x27c,
            end_height: 0x280,
            target_actor: 0x284,
            start_cycle: 0x288,
            end_cycle: 0x28c,
            slope: 0x290,
            start_pos: 0x294,
            id: 0x298,
            x: 0x2a0,
            z: 0x2a8,
            y: 0x2b0,
            orientation: 0x2b8,
            moving: 0x2c0,
            reserved: 0,
        };
        let layout = Layout {
            module_base: LAYOUT.module_base,
            world: LAYOUT,
            projectiles: p,
            ..Layout::default()
        };
        m.put_i32(LAYOUT.module_base + 0xe98c14, 1000);
        let head = CLIENT + 0xcc70;
        let (bolt_node, spell_node, bolt, spell) =
            (0x70_0000u64, 0x70_1000u64, 0x71_0000u64, 0x72_0000u64);
        m.put_u64(head, bolt_node);
        m.put_u64(head + 0x10, 2);
        m.put_u64(bolt_node, spell_node);
        m.put_u64(bolt_node + 0x18, bolt);
        m.put_u64(spell_node, head);
        m.put_u64(spell_node + 0x18, spell);
        for (at, id, source, target, actor, cycles, moving) in [
            (
                bolt,
                27,
                (3162, 3488),
                (3164, 3488),
                25401,
                (990, 1010),
                true,
            ),
            (
                spell,
                91,
                (3164, 3488),
                (3162, 3488),
                -781,
                (1015, 1040),
                false,
            ),
        ] {
            let ints = [
                0, source.0, source.1, 155, 0, 0, target.0, target.1, 146, actor, cycles.0,
                cycles.1, 5, 11, id,
            ];
            for (i, v) in ints.into_iter().enumerate() {
                m.put_i32(at + 0x260 + i as u64 * 4, v);
            }
            m.put(at + 0x2c0, &[moving as u8]);
        }
        for (offset, value) in [(0x2a0u64, 6592.5f64), (0x2a8, -420.0), (0x2b0, 6208.0)] {
            m.put_u64(bolt + offset, value.to_bits());
        }
        m.put_i32(bolt + 0x2b8, 512);

        let projectiles = projectiles(&m, &layout).unwrap();
        assert_eq!(projectiles.len(), 2);
        let (bolt, spell) = (&projectiles[0], &projectiles[1]);
        assert_eq!(
            (
                bolt.id,
                bolt.target_npc,
                bolt.target_player,
                bolt.source_npc,
                bolt.cycle
            ),
            (27, 25400, -1, -1, 1000)
        );
        assert_eq!(
            (
                bolt.moving,
                bolt.x,
                bolt.y,
                bolt.local_x,
                bolt.local_y,
                bolt.z,
                bolt.orientation
            ),
            (true, 3163, 3488, 6592, 6208, -420, 512)
        );
        assert_eq!(
            (
                spell.id,
                spell.target_npc,
                spell.target_player,
                spell.start_cycle,
                spell.end_cycle
            ),
            (91, -1, 780, 1015, 1040)
        );
        assert_eq!(
            (
                spell.moving,
                spell.x,
                spell.y,
                spell.local_x,
                spell.local_y,
                spell.height
            ),
            (false, 3164, 3488, 6720, 6208, 155)
        );
    }

    #[test]
    fn walks_the_ground_item_lists() {
        use ipc::state::{GroundItemsLayout, PlayerLayout, ProjectionLayout};
        let layout = Layout {
            player: PlayerLayout {
                base_x: 0x24,
                ..Default::default()
            },
            projection: ProjectionLayout {
                plane: 0x18,
                scene: 0x10,
                size_x: 0x948,
                size_y: 0x94c,
                ..Default::default()
            },
            ground_items: GroundItemsLayout {
                items: 0x140,
                level: 0x10,
                node_item: 0x18,
                id: 0x20,
            },
            ..Layout::default()
        };
        let (view, scene, planes, columns, rows) =
            (0x1000u64, 0x2000u64, 0x3000u64, 0x4000u64, 0x10_0000u64);
        let mut m = Fake::default();
        m.put_i32(view + 0x18, 1);
        m.put_i32(view + 0x24, 3136);
        m.put_i32(view + 0x28, 2784);
        m.put_u64(view + 0x10, scene);
        m.put_i32(scene + 0x948, 4);
        m.put_i32(scene + 0x94c, 3);
        m.put_u64(view + 0x140, planes);
        m.put_u64(planes + 24 + 0x10, columns);
        for x in 0..4u64 {
            let row = rows + x * 0x100;
            m.put_u64(columns + x * 24 + 0x10, row);
            for y in 0..3u64 {
                m.put_u64(row + y * 24, row + y * 24);
                m.put_u64(row + y * 24 + 8, row + y * 24);
                m.put_u64(row + y * 24 + 16, 0);
            }
        }
        let list = rows + 2 * 0x100 + 24;
        let (first, second) = (0x20_0000u64, 0x20_1000u64);
        m.put_u64(list, first);
        m.put_u64(first, second);
        m.put_u64(second, list);
        for (node, item, id, quantity) in [
            (first, 0x30_0000u64, 311, 1),
            (second, 0x30_1000u64, 995, 2500),
        ] {
            m.put_u64(node + 0x18, item);
            m.put_i32(item + 0x20, id);
            m.put_i32(item + 0x24, quantity);
        }
        let items = ground_items_in(&m, &layout, view).unwrap();
        let tile = |i: &GroundItem| (i.id, i.quantity, i.x, i.y, i.plane, i.scene_x, i.scene_y);
        assert_eq!(
            items.iter().map(tile).collect::<Vec<_>>(),
            vec![
                (311, 1, 3138, 2785, 1, 2, 1),
                (995, 2500, 3138, 2785, 1, 2, 1)
            ]
        );
    }

    #[test]
    fn reads_spot_anims() {
        use ipc::state::{PlayerLayout, SpotAnimLayout};
        let layout = Layout {
            module_base: LAYOUT.module_base,
            player: PlayerLayout {
                base_x: 0x24,
                ..Default::default()
            },
            spot_anims: SpotAnimLayout {
                list: 0x160,
                cycle: 0xe98c14,
                start_cycle: 0x260,
                level: 0x264,
                x: 0x268,
                y: 0x26c,
                z: 0x270,
                id: 0x274,
            },
            ..Layout::default()
        };
        let (view, first, second, a, b) = (
            0x1000u64,
            0x20_0000u64,
            0x20_1000u64,
            0x30_0000u64,
            0x31_0000u64,
        );
        let mut m = Fake::default();
        m.put_i32(LAYOUT.module_base + 0xe98c14, 500);
        m.put_i32(view + 0x24, 3136);
        m.put_i32(view + 0x28, 2784);
        let head = view + 0x160;
        m.put_u64(head, first);
        m.put_u64(head + 0x10, 2);
        m.put_u64(first, second);
        m.put_u64(first + 0x18, a);
        m.put_u64(second, head);
        m.put_u64(second + 0x18, b);
        for (at, start, level, x, y, z, id) in [
            (a, 510, 1, 2 * 128 + 64, 3 * 128 + 64, -100, 308),
            (b, 495, 1, 70 * 128 + 64, 5 * 128 + 64, 0, 86),
        ] {
            for (i, v) in [start, level, x, y, z, id].into_iter().enumerate() {
                m.put_i32(at + 0x260 + i as u64 * 4, v);
            }
        }
        let objects = spot_anims_in(&m, &layout, view, 0).unwrap();
        let tile = |o: &SpotAnim| (o.id, o.x, o.y, o.plane, o.z, o.start_cycle, o.cycle);
        assert_eq!(
            objects.iter().map(tile).collect::<Vec<_>>(),
            vec![
                (308, 3138, 2787, 1, -100, 510, 500),
                (86, 3206, 2789, 1, 0, 495, 500)
            ]
        );
    }

    #[test]
    fn reads_the_main_worlds_spot_anims_when_the_player_is_on_a_boat() {
        use ipc::state::{PlayerLayout, SpotAnimLayout, part};
        let client = 0x2000u64;
        let layout = Layout {
            found: part::PLAYER | part::SPOT_ANIMS,
            module_base: LAYOUT.module_base,
            client: LAYOUT.client,
            player: PlayerLayout {
                world_views: 0x300,
                world_view_id: 0x320,
                player_index: 0x324,
                players: 0x60,
                base_x: 0x24,
                local_x: 0x268,
            },
            spot_anims: SpotAnimLayout {
                list: 0x160,
                cycle: 0xe98c14,
                start_cycle: 0x260,
                level: 0x264,
                x: 0x268,
                y: 0x26c,
                z: 0x270,
                id: 0x274,
            },
            ..Layout::default()
        };
        let (top, boat, player) = (0x1000u64, 0x5000u64, 0x6000u64);
        let mut m = Fake::default();
        m.put_u64(LAYOUT.module_base + LAYOUT.client as u64, client);
        m.put_i32(LAYOUT.module_base + 0xe98c14, 500);
        m.put_table(client + 0x300, 0x40_0000, &[(0, top), (5, boat)]);
        m.put_i32(client + 0x320, 5);
        m.put_i32(client + 0x324, 7);
        m.put_table(boat + 0x60, 0x42_0000, &[(7, player)]);
        for (view, base) in [(top, 3136), (boat, 0)] {
            m.put_i32(view + 0x24, base);
            m.put_i32(view + 0x28, base);
            m.put_u64(view + 0x160, view + 0x160);
        }
        for (view, node, at, id) in [
            (top, 0x50_0000u64, 0x51_0000u64, 308),
            (boat, 0x52_0000, 0x53_0000, 86),
        ] {
            let head = view + 0x160;
            m.put_u64(head, node);
            m.put_u64(head + 0x10, 1);
            m.put_u64(node, head);
            m.put_u64(node + 0x18, at);
            for (i, v) in [500, 0, 2 * 128 + 64, 3 * 128 + 64, 0, id]
                .into_iter()
                .enumerate()
            {
                m.put_i32(at + 0x260 + i as u64 * 4, v);
            }
        }
        let found = spot_anims(&m, &layout).unwrap();
        assert_eq!(
            found.iter().map(|o| (o.view_id, o.id)).collect::<Vec<_>>(),
            vec![(0, 308), (5, 86)]
        );
        assert_eq!((found[0].x, found[0].y), (3138, 3139));
    }
}
