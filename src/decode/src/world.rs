use serde::{Deserialize, Serialize};
use ipc::state::{Layout, WorldLayout};

use ipc::read::{local_player, table_get};

use crate::memory::{Memory, read_string};

const MAX_CHAIN: usize = 64;
const MAX_ITEMS: u64 = 4096;
const MAX_ACTORS: i32 = 4096;
const MAX_VARCS: u64 = 1 << 16;

#[cfg(test)]
const NODE_VALUE: u64 = 0x10;
#[cfg(test)]
const NODE_NEXT: u64 = 0x18;
const TABLE_BUCKETS: u64 = 0x8;
const TABLE_COUNT: u64 = 0x10;

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
    let base = view + layout.player.base_x as u64;
    let (base_x, base_y) = (memory.i32(base)?, memory.i32(base + 4)?);
    let scene = memory.ptr(view + p.scene as u64);
    let size = |offset: u32| scene.and_then(|s| memory.i32(s + offset as u64)).filter(|n| (1..=256).contains(n)).unwrap_or(SCENE_TILES);
    let (size_x, size_y) = (size(p.size_x), size(p.size_y));
    if !(0..4).contains(&plane) {
        return None;
    }
    let level = |array: u64, index: i32| memory.ptr(array + index as u64 * LIST_SIZE + g.level as u64);
    let columns = level(memory.ptr(view + g.items as u64)?, plane)?;
    let mut items = Vec::new();
    let mut heads = vec![0u8; size_y as usize * LIST_SIZE as usize];
    for x in 0..size_x {
        let Some(column) = level(columns, x) else { continue };
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
                let Some(item) = memory.ptr(node + g.node_item as u64) else { break };
                let (Some(id), Some(quantity)) = (memory.i32(item + g.id as u64), memory.i32(item + g.id as u64 + 4)) else { break };
                items.push(GroundItem { id, quantity, name: None, x: base_x + x, y: base_y + y, plane, scene_x: x, scene_y: y });
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
    // the animation id, -1 when it has none
    pub animation: i32,
    // the model's height in units (the actor's logical height), -1 when it can't be read
    pub height: i32,
    // the pose animation: the walk animation while the NPC moves, the stand one otherwise
    pub pose: i32,
    // the spot animation (graphic) on the NPC, -1 when it has none
    pub graphic: i32,
}

// Byte offsets in an actor: model height, pose animation, and a pointer to the spot animation node (kind at 0x8, id at 0x20).
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
    // As for `Npc`: the actor's model height (-1 when unknown), animation, pose and spot animation.
    pub height: i32,
    pub animation: i32,
    pub pose: i32,
    pub graphic: i32,
    // What the player is interacting with: state (0 NPC, 1 loc, 2 gone, 3 done, -1 unknown), NPC index, loc world tile (-1 if none).
    pub target_state: i32,
    pub target_npc: i32,
    pub target_x: i32,
    pub target_y: i32,
}

// The interaction block in a player actor: state word, target NPC index and loc tile; it names the target, not the action.
const PLAYER_TARGET_STATE: u64 = 0x3a4;
const PLAYER_TARGET_NPC: u64 = 0x3a8;
const PLAYER_TARGET_TILE: u64 = 0x3b0;

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

fn read_ints(memory: &impl Memory, address: u64) -> Option<Vec<i32>> {
    let begin = memory.u64(address)?;
    let end = memory.u64(address + 8)?;
    let count = end.checked_sub(begin)? / 4;
    if count > MAX_ITEMS {
        return None;
    }
    let mut bytes = vec![0u8; count as usize * 4];
    if count > 0 && !memory.read(begin, &mut bytes) {
        return None;
    }
    Some(bytes.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect())
}

fn client(memory: &impl Memory, layout: &WorldLayout) -> Option<u64> {
    memory.ptr(layout.module_base + layout.client as u64)
}

pub fn item_container(memory: &impl Memory, layout: &WorldLayout, id: i32) -> Option<Vec<Item>> {
    let table = layout.module_base + layout.containers as u64;
    let buckets = memory.ptr(table + TABLE_BUCKETS)?;
    let count = memory.u64(table + TABLE_COUNT)?;
    if count == 0 || count > MAX_ITEMS || id < 0 {
        return None;
    }
    let end = memory.u64(buckets + count * 8)?;
    let mut node = memory.u64(buckets + (id as u64 % count) * 8)?;
    for _ in 0..MAX_CHAIN {
        if node == 0 || node == end {
            return None;
        }
        if memory.i32(node)? == id {
            let ids = read_ints(memory, node + layout.container_ids as u64)?;
            let quantities = read_ints(memory, node + layout.container_quantities as u64)?;
            return Some(
                ids.iter()
                    .enumerate()
                    .map(|(slot, &item)| Item { slot: slot as i32, id: item, quantity: quantities.get(slot).copied().unwrap_or(0) })
                    .collect(),
            );
        }
        node = memory.u64(node + layout.container_next as u64)?;
    }
    None
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
            let kind = {
                let mut b = [0u8];
                memory.read(entry + layout.varc_kind as u64, &mut b).then_some(b[0])?
            };
            return (kind == 1).then(|| memory.i32(entry + layout.varc_value as u64)).flatten();
        }
    }
    None
}

fn world_view(memory: &impl Memory, layout: &WorldLayout, client: u64) -> Option<(u64, i32, i32)> {
    let id = memory.i32(client + layout.world_view_id as u64)?;
    let view = table_get(memory, client + layout.world_views as u64, id)?;
    let base_x = memory.i32(view + layout.base_x as u64)?;
    let base_y = memory.i32(view + layout.base_x as u64 + 4)?;
    Some((view, base_x, base_y))
}

fn position(memory: &impl Memory, layout: &WorldLayout, actor: u64, base: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let local_x = memory.i32(actor + layout.local_x as u64)?;
    let local_y = memory.i32(actor + layout.local_x as u64 + 4)?;
    Some((base.0 + (local_x >> 7), base.1 + (local_y >> 7), local_x, local_y))
}

// The fields NPCs and players share, all read from the actor itself.
fn actor_fields(memory: &impl Memory, layout: &WorldLayout, actor: u64) -> (i32, i32, i32, i32) {
    let animation = memory.i32(actor + layout.npc_animation as u64).unwrap_or(-1);
    let height = memory.i32(actor + ACTOR_HEIGHT).filter(|h| (1..=1000).contains(h)).unwrap_or(-1);
    let pose = memory.i32(actor + ACTOR_POSE).unwrap_or(-1);
    let graphic = memory
        .ptr(actor + ACTOR_SPOT_ANIM)
        .filter(|&spot| memory.i32(spot + SPOT_ANIM_KIND) == Some(SPOT_ANIM_KIND_VALUE))
        .and_then(|spot| memory.i32(spot + SPOT_ANIM_ID))
        .unwrap_or(-1);
    (animation, height, pose, graphic)
}

pub fn npcs(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<Npc>> {
    let client = client(memory, layout)?;
    let (view, base_x, base_y) = world_view(memory, layout, client)?;
    let indices = memory.ptr(view + layout.npc_indices as u64)?;
    let count = memory.i32(view + layout.npc_indices as u64 + 8)?;
    if !(0..=MAX_ACTORS).contains(&count) {
        return None;
    }
    let table = view + layout.npc_table as u64;
    let mut npcs = Vec::with_capacity(count as usize);
    for i in 0..count as u64 {
        let Some(index) = memory.i32(indices + i * 4) else { continue };
        let Some(npc) = table_get(memory, table, index) else { continue };
        let Some((x, y, local_x, local_y)) = position(memory, layout, npc, (base_x, base_y)) else { continue };
        let definition = memory.ptr(npc + layout.npc_definition as u64);
        let id = definition.and_then(|d| memory.i32(d + layout.definition_id as u64)).unwrap_or(-1);
        let name = read_string(memory, npc + layout.npc_name_override as u64)
            .filter(|n| !n.is_empty())
            .or_else(|| definition.and_then(|d| read_string(memory, d + layout.definition_name as u64)))
            .unwrap_or_default();
        let (animation, height, pose, graphic) = actor_fields(memory, layout, npc);
        npcs.push(Npc { index, id, name, x, y, local_x, local_y, animation, height, pose, graphic });
    }
    Some(npcs)
}

// The NPC definition's size in tiles (its side), checked against NPCs of size 1, 2 and 3.
const DEFINITION_SIZE: u64 = 0x20;
const MAX_NPC_SIZE: i32 = 10;

/// The size in tiles of the NPC with `index`, from its definition; None when it's gone or the value is off.
pub fn npc_size(memory: &impl Memory, layout: &WorldLayout, index: i32) -> Option<i32> {
    let client = client(memory, layout)?;
    let (view, _, _) = world_view(memory, layout, client)?;
    let npc = table_get(memory, view + layout.npc_table as u64, index)?;
    let definition = memory.ptr(npc + layout.npc_definition as u64)?;
    memory.i32(definition + DEFINITION_SIZE).filter(|s| (1..=MAX_NPC_SIZE).contains(s))
}

// Debug: the NPC actor with `index`, or its definition with `definition` (its address and first `count`
// 4-byte words), to find fields.
pub fn npc_words(memory: &impl Memory, layout: &WorldLayout, index: i32, count: usize, definition: bool) -> Option<(u64, Vec<i32>)> {
    let client = client(memory, layout)?;
    let (view, _, _) = world_view(memory, layout, client)?;
    let npc = table_get(memory, view + layout.npc_table as u64, index)?;
    let at = if definition { memory.ptr(npc + layout.npc_definition as u64)? } else { npc };
    let mut bytes = vec![0u8; count * 4];
    if !memory.read(at, &mut bytes) {
        return None;
    }
    Some((at, bytes.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect()))
}

pub fn players(memory: &impl Memory, layout: &WorldLayout) -> Option<Vec<Player>> {
    let client = client(memory, layout)?;
    let (view, base_x, base_y) = world_view(memory, layout, client)?;
    let count = memory.i32(client + layout.player_count as u64)?;
    if !(0..=MAX_ACTORS).contains(&count) {
        return None;
    }
    let table = view + layout.players as u64;
    let mut players = Vec::with_capacity(count as usize);
    for i in 0..count as u64 {
        let Some(index) = memory.i32(client + layout.player_count as u64 + 4 + i * 4) else { continue };
        let Some(player) = table_get(memory, table, index) else { continue };
        let Some((x, y, local_x, local_y)) = position(memory, layout, player, (base_x, base_y)) else { continue };
        let name = memory
            .ptr(player + layout.player_name as u64)
            .and_then(|n| read_string(memory, n))
            .unwrap_or_default()
            .replace('\u{a0}', " ");
        let (animation, height, pose, graphic) = actor_fields(memory, layout, player);
        let (target_state, target_npc, target_x, target_y) = player_target(memory, player);
        players.push(Player {
            index, name, x, y, local_x, local_y, height, animation, pose, graphic, target_state, target_npc, target_x, target_y,
        });
    }
    Some(players)
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
        reserved: 0,
    };
    const CLIENT: u64 = 0x10_0000;

    fn table(m: &mut Fake, table: u64, nodes: u64, entries: &[(i32, u64)]) {
        let buckets = nodes + 0x10000;
        m.put_u64(table + TABLE_BUCKETS, buckets);
        m.put_i32(table + TABLE_COUNT, 1);
        m.put_u64(buckets + 8, 0xdead);
        let mut next = 0;
        for (i, &(key, value)) in entries.iter().enumerate().rev() {
            let node = nodes + i as u64 * 0x20;
            m.put(node, &[0; 0x20]);
            m.put_i32(node, key);
            m.put_u64(node + NODE_VALUE, value);
            m.put_u64(node + NODE_NEXT, next);
            next = node;
        }
        m.put_u64(buckets, next);
    }

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
        for (i, (id, value, kind)) in [(3, -1, 1u8), (171, 3, 1), (200, 7, 2)].into_iter().enumerate() {
            let e = entries + i as u64 * 0x30;
            m.put_i32(e, id);
            m.put_i32(e + 8, value);
            m.put(e + 0x28, &[kind]);
        }

        let view = 0x40_0000;
        m.put_i32(CLIENT + LAYOUT.world_view_id as u64, 0);
        table(&mut m, CLIENT + LAYOUT.world_views as u64, 0x41_0000, &[(0, view)]);
        m.put_i32(view + 0x24, 3112);
        m.put_i32(view + 0x28, 3440);

        let (clerk, banker) = (0x50_0000u64, 0x51_0000u64);
        table(&mut m, view + LAYOUT.npc_table as u64, 0x52_0000, &[(25400, clerk), (25399, banker)]);
        let indices = 0x58_0000;
        m.put_u64(view + LAYOUT.npc_indices as u64, indices);
        m.put_i32(view + LAYOUT.npc_indices as u64 + 8, 2);
        m.put_i32(indices, 25400);
        m.put_i32(indices + 4, 25399);
        for (npc, (id, name, x, y)) in [(clerk, (2149, "", 6720, 6208)), (banker, (6528, "", 6592, 6464))] {
            m.put_i32(npc + LAYOUT.local_x as u64, x);
            m.put_i32(npc + LAYOUT.local_x as u64 + 4, y);
            m.put_short_string(npc + LAYOUT.npc_name_override as u64, name);
            let definition = npc + 0x8000;
            m.put_u64(npc + LAYOUT.npc_definition as u64, definition);
            m.put_i32(definition, id);
        }
        m.put_short_string(clerk + 0x8000 + 8, "Grand Exchange Clerk".get(..20).unwrap_or_default());
        m.put_short_string(banker + 0x8000 + 8, "Banker");
        let long = 0x54_0000;
        m.put(long, b"Grand Exchange Clerk");
        m.put_u64(clerk + 0x8000 + 8, long);
        m.put_u64(clerk + 0x8000 + 0x10, 20);
        m.put_u64(clerk + 0x8000 + 0x18, 0x8000_0000_0000_0020);

        let player = 0x60_0000;
        table(&mut m, view + LAYOUT.players as u64, 0x61_0000, &[(780, player)]);
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
        assert_eq!(inventory[0], Item { slot: 0, id: -1, quantity: 0 });
        assert_eq!(inventory[3], Item { slot: 3, id: 27281, quantity: 1 });
        assert_eq!(item_container(&m, &LAYOUT, 94).unwrap()[1].id, 13342);
        assert_eq!(item_container(&m, &LAYOUT, 95), None);
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
        assert_eq!(npcs[0], Npc { index: 25400, id: 2149, name: "Grand Exchange Clerk".into(), x: 3164, y: 3488, local_x: 6720, local_y: 6208, animation: -1, height: -1, pose: -1, graphic: -1 });
        assert_eq!((npcs[1].id, npcs[1].name.as_str(), npcs[1].x, npcs[1].y), (6528, "Banker", 3163, 3490));
        let players = players(&m, &LAYOUT).unwrap();
        assert_eq!(
            players,
            [Player { index: 780, name: "gambri jani5".into(), x: 3162, y: 3488, local_x: 6464, local_y: 6208, height: -1, animation: -1, pose: -1, graphic: -1, target_state: -1, target_npc: -1, target_x: -1, target_y: -1 }]
        );
    }

    #[test]
    fn walks_the_ground_item_lists() {
        use ipc::state::{GroundItemsLayout, PlayerLayout, ProjectionLayout};
        let layout = Layout {
            player: PlayerLayout { base_x: 0x24, ..Default::default() },
            projection: ProjectionLayout { plane: 0x18, scene: 0x10, size_x: 0x948, size_y: 0x94c, ..Default::default() },
            ground_items: GroundItemsLayout { items: 0x140, level: 0x10, node_item: 0x18, id: 0x20 },
            ..Layout::default()
        };
        let (view, scene, planes, columns, rows) = (0x1000u64, 0x2000u64, 0x3000u64, 0x4000u64, 0x10_0000u64);
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
        // tile (2, 1): a pile of two
        let list = rows + 2 * 0x100 + 24;
        let (first, second) = (0x20_0000u64, 0x20_1000u64);
        m.put_u64(list, first);
        m.put_u64(first, second);
        m.put_u64(second, list);
        for (node, item, id, quantity) in [(first, 0x30_0000u64, 311, 1), (second, 0x30_1000u64, 995, 2500)] {
            m.put_u64(node + 0x18, item);
            m.put_i32(item + 0x20, id);
            m.put_i32(item + 0x24, quantity);
        }
        let items = ground_items_in(&m, &layout, view).unwrap();
        let tile = |i: &GroundItem| (i.id, i.quantity, i.x, i.y, i.plane, i.scene_x, i.scene_y);
        assert_eq!(items.iter().map(tile).collect::<Vec<_>>(), vec![(311, 1, 3138, 2785, 1, 2, 1), (995, 2500, 3138, 2785, 1, 2, 1)]);
    }
}
