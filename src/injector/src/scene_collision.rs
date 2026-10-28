// The collision map the client keeps for the loaded scene (Java's `CollisionMap`), with instances, opened doors and spawned locs; edge ring 0xFFFFFF.

use ipc::{
    read::{local_player, Memory},
    state::{array, collision_map, part, Layout},
};

const MAX_SIZE: i32 = 256;
const MAX_MAPS: u64 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCollision {
    // the world tile of the scene's (0, 0), which is the map's (1, 1)
    pub base_x: i32,
    pub base_y: i32,
    pub plane: i32,
    width: i32,
    height: i32,
    // column major, edge ring included
    flags: Vec<u32>,
}

impl SceneCollision {
    pub fn flags(&self, x: i32, y: i32) -> Option<u32> {
        let border = collision_map::BORDER as i32;
        let (mx, my) = (x - self.base_x + border, y - self.base_y + border);
        ((0..self.width).contains(&mx) && (0..self.height).contains(&my)).then(|| self.flags[(mx * self.height + my) as usize])
    }

    // The world tiles the map covers, edge ring included: (x1, y1, x2, y2).
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let border = collision_map::BORDER as i32;
        (self.base_x - border, self.base_y - border, self.base_x - border + self.width - 1, self.base_y - border + self.height - 1)
    }
}

pub fn read(memory: &impl Memory, layout: &Layout) -> Option<SceneCollision> {
    let (plane, maps) = maps(memory, layout)?;
    maps.read(memory, plane)
}

/// Every plane's collision map (the loaded scene has four), with the player's plane first in the tuple.
pub fn read_all(memory: &impl Memory, layout: &Layout) -> Option<(i32, Vec<Option<SceneCollision>>)> {
    let (plane, maps) = maps(memory, layout)?;
    Some((plane, (0..maps.count.min(PLANES)).map(|p| maps.read(memory, p)).collect()))
}

const PLANES: i32 = 4;

struct Maps {
    begin: u64,
    count: i32,
    stride: u64,
    base_x: i32,
    base_y: i32,
}

fn maps(memory: &impl Memory, layout: &Layout) -> Option<(i32, Maps)> {
    if !layout.has(part::COLLISION | part::PROJECTION | part::PLAYER) {
        return None;
    }
    let (view, _) = local_player(memory, layout)?;
    let p = &layout.projection;
    let plane = memory.i32(view + p.plane as u64)?;
    let base = view + layout.player.base_x as u64;
    let (base_x, base_y) = (memory.i32(base)?, memory.i32(base + 4)?);
    let scene = memory.ptr(view + p.scene as u64)?;
    let c = &layout.collision;
    let begin = memory.ptr(scene + c.maps as u64)?;
    let end = memory.ptr(scene + c.maps as u64 + 8)?;
    let count = (end.checked_sub(begin)? / c.stride.max(1) as u64).min(MAX_MAPS) as i32;
    Some((plane, Maps { begin, count, stride: c.stride as u64, base_x, base_y }))
}

impl Maps {
    fn read(&self, memory: &impl Memory, plane: i32) -> Option<SceneCollision> {
        if !(0..self.count).contains(&plane) {
            return None;
        }
        let map = self.begin + plane as u64 * self.stride;
        let width = memory.i32(map + collision_map::WIDTH as u64)?;
        let height = memory.i32(map + collision_map::HEIGHT as u64)?;
        if !(1..=MAX_SIZE).contains(&width) || !(1..=MAX_SIZE).contains(&height) {
            return None;
        }
        let columns = map + collision_map::COLUMNS as u64;
        if memory.u64(columns + array::COUNT as u64)? != width as u64 {
            return None;
        }
        let data = memory.ptr(columns + array::DATA as u64)?;
        let mut flags = Vec::with_capacity((width * height) as usize);
        for x in 0..width as u64 {
            let column = data + x * collision_map::COLUMN_SIZE as u64;
            if memory.u64(column + array::COUNT as u64)? != height as u64 {
                return None;
            }
            let values = memory.i32s(memory.ptr(column + array::DATA as u64)?, height as usize)?;
            flags.extend(values.into_iter().map(|v| v as u32));
        }
        Some(SceneCollision { base_x: self.base_x, base_y: self.base_y, plane, width, height, flags })
    }
}

#[cfg(test)]
pub fn from_rows(base_x: i32, base_y: i32, rows: &[&str]) -> SceneCollision {
    // `rows` top (north) first, '#' blocked; the edge ring is added around them
    let (inner_w, inner_h) = (rows[0].len() as i32, rows.len() as i32);
    let (width, height) = (inner_w + 2, inner_h + 2);
    let mut flags = vec![0xFFFFFFu32; (width * height) as usize];
    for (row, line) in rows.iter().enumerate() {
        let y = inner_h - 1 - row as i32;
        for (x, ch) in line.chars().enumerate() {
            let x = x as i32;
            flags[((x + 1) * height + y + 1) as usize] = if ch == '#' { crate::collision::flag::OBJECT } else { 0 };
        }
    }
    SceneCollision { base_x, base_y, plane: 0, width, height, flags }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;
    use ipc::state::{CollisionLayout, PlayerLayout, ProjectionLayout};

    #[test]
    fn reads_the_players_plane() {
        const CLIENT: u64 = 0x1000;
        const VIEW: u64 = 0x3000;
        const SCENE: u64 = 0x4000;
        const MAPS: u64 = 0x5000;
        const COLUMNS: u64 = 0x6000;
        const DATA: u64 = 0x7000;
        let layout = Layout {
            found: part::COLLISION | part::PROJECTION | part::PLAYER,
            module_base: 0x100,
            client: 0x10,
            player: PlayerLayout { world_views: 0x20, world_view_id: 0x70, player_index: 0x74, players: 0x40, base_x: 0x80, local_x: 0 },
            projection: ProjectionLayout { plane: 0x60, scene: 0x10, ..Default::default() },
            collision: CollisionLayout { maps: 0x2f8, stride: 0x28 },
            ..Default::default()
        };

        let mut m = Fake::default();
        m.put_u64(0x100 + 0x10, CLIENT);
        m.put_i32(CLIENT + 0x70, 0);
        m.put_i32(CLIENT + 0x74, 0);
        // one world view (id 0) and one player (index 0) in the hash tables `local_player` reads
        for (table, value) in [(CLIENT + 0x20, VIEW), (VIEW + 0x40, 0x9000)] {
            let buckets = table + 0x100;
            m.put_u64(table + 0x8, buckets);
            m.put_u64(table + 0x10, 1);
            let node = buckets + 0x40;
            m.put_u64(buckets, node);
            m.put_u64(buckets + 8, 0xdead);
            m.put(node, &[0; 0x20]);
            m.put_u64(node + 0x10, value);
            m.put_u64(node + 0x18, 0xdead);
        }
        m.put_i32(VIEW + 0x60, 1);
        m.put_i32(VIEW + 0x80, 3200);
        m.put_i32(VIEW + 0x84, 3100);
        m.put_u64(VIEW + 0x10, SCENE);
        m.put_u64(SCENE + 0x2f8, MAPS);
        m.put_u64(SCENE + 0x300, MAPS + 2 * 0x28);
        let map = MAPS + 0x28;
        m.put_i32(map + 0x8, 3);
        m.put_i32(map + 0xc, 2);
        m.put_u64(map + 0x10 + 0x8, 3);
        m.put_u64(map + 0x10 + 0x10, COLUMNS);
        for x in 0..3u64 {
            let column = COLUMNS + x * 0x18;
            m.put_u64(column + 0x8, 2);
            m.put_u64(column + 0x10, DATA + x * 8);
            m.put_i32(DATA + x * 8, (x * 10) as i32);
            m.put_i32(DATA + x * 8 + 4, (x * 10 + 1) as i32);
        }
        let read = read(&m, &layout).expect("read");
        assert_eq!((read.base_x, read.base_y, read.plane), (3200, 3100, 1));
        // the map's (1, 1) is the scene's (0, 0)
        assert_eq!(read.flags(3200, 3100), Some(11));
        assert_eq!(read.flags(3199, 3099), Some(0));
        assert_eq!(read.flags(3201, 3099), Some(20));
        assert_eq!(read.flags(3202, 3100), None);
        assert_eq!(read.bounds(), (3199, 3099, 3201, 3100));
        let (plane, all) = read_all(&m, &layout).expect("read all");
        assert_eq!(plane, 1);
        assert_eq!(all.len(), 2);
        // plane 0's map has no size
        assert_eq!(all[0], None);
        assert_eq!(all[1].as_ref(), Some(&read));
    }
}
