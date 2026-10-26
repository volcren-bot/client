use serde::{Deserialize, Serialize};
use ipc::state::Layout;

use crate::memory::{Memory, read_string};

const MAX_WORLDS: i32 = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    pub id: i32,
    pub types: u32,
    pub players: i32,
    pub location: i32,
    pub activity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldSelect {
    pub open: bool,
    pub current: i32,
    pub offset: i32,
    pub max_offset: i32,
    pub hovered: i32,
    pub background_height: Option<i32>,
    pub worlds: Vec<World>,
}

fn world(memory: &impl Memory, layout: &Layout, address: u64) -> Option<World> {
    let w = &layout.world_select;
    let mut types = [0u8; 4];
    memory.read(address + w.types as u64, &mut types).then_some(())?;
    Some(World {
        id: memory.i32(address + w.id as u64)?,
        types: u32::from_le_bytes(types),
        players: memory.i32(address + w.players as u64)?,
        location: memory.i32(address + w.location as u64)?,
        activity: read_string(memory, address + w.activity as u64).unwrap_or_default(),
    })
}

fn worlds(memory: &impl Memory, layout: &Layout) -> Option<Vec<World>> {
    let w = &layout.world_select;
    let count = memory.i32(layout.module_base + w.count as u64)?;
    let Some(array) = memory.ptr(layout.module_base + w.worlds as u64).filter(|_| (1..=MAX_WORLDS).contains(&count)) else {
        return Some(Vec::new());
    };
    (0..count as u64).map(|i| world(memory, layout, memory.ptr(array + i * 8)?)).collect()
}

pub fn world_select(memory: &impl Memory, layout: &Layout, client: u64) -> Option<WorldSelect> {
    let w = &layout.world_select;
    let global = |rva: u32| memory.i32(layout.module_base + rva as u64);
    let mut open = [0u8];
    memory.read(layout.module_base + w.open as u64, &mut open).then_some(())?;
    let background_height = memory
        .ptr(layout.module_base + w.background as u64)
        .and_then(|background| memory.i32(background + w.background_height as u64));
    Some(WorldSelect {
        open: open[0] != 0,
        current: memory.i32(client + w.current_world as u64)?,
        offset: global(w.offset)?,
        max_offset: global(w.max_offset)?,
        hovered: global(w.hovered)?,
        background_height,
        worlds: worlds(memory, layout)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;
    use ipc::state::WorldSelectLayout;

    const BASE: u64 = 0x1_4000_0000;
    const CLIENT: u64 = 0x5000_0000;

    fn layout() -> Layout {
        Layout {
            module_base: BASE,
            world_select: WorldSelectLayout {
                open: 0xe98bd2,
                count: 0xe98c00,
                worlds: 0x154fbb8,
                offset: 0xe98b74,
                max_offset: 0xe98b78,
                current_world: 0x32b4,
                background: 0x154fc18,
                background_height: 0x1c,
                id: 0,
                types: 0x4,
                players: 0x8,
                activity: 0x28,
                location: 0x40,
                hovered: 0xcfb060,
            },
            ..Layout::default()
        }
    }

    fn put_world(m: &mut Fake, address: u64, id: i32, types: u32, players: i32, location: i32, activity: &str) {
        m.put_i32(address, id);
        m.put(address + 4, &types.to_le_bytes());
        m.put_i32(address + 8, players);
        m.put_short_string(address + 0x28, activity);
        m.put_i32(address + 0x40, location);
    }

    fn client(m: &mut Fake, open: u8, count: i32) {
        m.put(BASE + 0xe98bd2, &[open]);
        m.put_i32(BASE + 0xe98c00, count);
        m.put_i32(BASE + 0xe98b74, 1);
        m.put_i32(BASE + 0xe98b78, 2);
        m.put_i32(BASE + 0xcfb060, -1);
        m.put_i32(CLIENT + 0x32b4, 304);
    }

    #[test]
    fn reads_the_world_list_in_order() {
        let mut m = Fake::default();
        client(&mut m, 1, 2);
        m.put_u64(BASE + 0x154fbb8, 0x7000);
        m.put_u64(0x7000, 0x8000);
        m.put_u64(0x7008, 0x9000);
        put_world(&mut m, 0x8000, 301, 0, 796, 0, "Trade - Free");
        put_world(&mut m, 0x9000, 345, 0x2000_0001, 59, 0, "Deadman");
        m.put_u64(BASE + 0x154fc18, 0xa000);
        m.put_i32(0xa000 + 0x1c, 671);

        let select = world_select(&m, &layout(), CLIENT).unwrap();
        assert_eq!(
            select,
            WorldSelect {
                open: true,
                current: 304,
                offset: 1,
                max_offset: 2,
                hovered: -1,
                background_height: Some(671),
                worlds: vec![
                    World { id: 301, types: 0, players: 796, location: 0, activity: "Trade - Free".into() },
                    World { id: 345, types: 0x2000_0001, players: 59, location: 0, activity: "Deadman".into() },
                ],
            }
        );
    }

    #[test]
    fn no_worlds_before_the_list_is_loaded() {
        let mut m = Fake::default();
        client(&mut m, 0, 0);
        m.put_u64(BASE + 0x154fbb8, 0);
        let select = world_select(&m, &layout(), CLIENT).unwrap();
        assert!(!select.open);
        assert_eq!(select.worlds, Vec::new());
        assert_eq!(select.background_height, None);
    }
}
