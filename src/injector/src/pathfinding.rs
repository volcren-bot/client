
use std::{
    cmp::Reverse,
    collections::{hash_map::Entry, BinaryHeap, HashMap},
    hash::{BuildHasherDefault, Hasher},
};

use crate::collision::{flag, Movement, WorldCollision};

const BLOCKS: u16 = Movement::BLOCKS;
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 11;

// Tile keys are never iterated, so a multiplicative hash is much cheaper than SipHash on the hot path.
#[derive(Default)]
struct TileHasher(u64);

impl Hasher for TileHasher {
    fn finish(&self) -> u64 {
        let mut z = self.0;
        z ^= z >> 33;
        z = z.wrapping_mul(0xff51_afd7_ed55_8ccd);
        z ^ (z >> 33)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }

    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type TileMap<V> = HashMap<u32, V, BuildHasherDefault<TileHasher>>;

const FROM_EAST: u16 = BLOCKS | flag::WALL_EAST as u16;
const FROM_WEST: u16 = BLOCKS | flag::WALL_WEST as u16;
const FROM_NORTH: u16 = BLOCKS | flag::WALL_NORTH as u16;
const FROM_SOUTH: u16 = BLOCKS | flag::WALL_SOUTH as u16;

pub type Tile = (i32, i32);

fn walkable(world: &WorldCollision<Movement>, plane: i32, x: i32, y: i32, mask: u16) -> bool {
    world.flags(plane, x, y).is_some_and(|m| m.0 & mask == 0)
}

pub fn can_step(world: &WorldCollision<Movement>, plane: i32, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    use flag::*;
    let straight_x = |dx: i32| walkable(world, plane, x + dx, y, if dx < 0 { FROM_EAST } else { FROM_WEST });
    let straight_y = |dy: i32| walkable(world, plane, x, y + dy, if dy < 0 { FROM_NORTH } else { FROM_SOUTH });
    match (dx, dy) {
        (0, 0) => true,
        (dx, 0) => straight_x(dx),
        (0, dy) => straight_y(dy),
        (dx, dy) => {
            let corner = match (dx < 0, dy < 0) {
                (true, true) => WALL_NORTH | WALL_NORTH_EAST | WALL_EAST,
                (false, true) => WALL_NORTH | WALL_NORTH_WEST | WALL_WEST,
                (true, false) => WALL_SOUTH | WALL_SOUTH_EAST | WALL_EAST,
                (false, false) => WALL_SOUTH | WALL_SOUTH_WEST | WALL_WEST,
            };
            walkable(world, plane, x + dx, y + dy, BLOCKS | corner as u16) && straight_x(dx) && straight_y(dy)
        }
    }
}

fn key((x, y): Tile) -> u32 {
    ((x as u32) << 16) | (y as u32 & 0xffff)
}

fn tile(key: u32) -> Tile {
    ((key >> 16) as i32, (key & 0xffff) as i32)
}

fn estimate((x, y): Tile, (tx, ty): Tile) -> u32 {
    let (dx, dy) = ((x - tx).unsigned_abs(), (y - ty).unsigned_abs());
    STRAIGHT * dx.max(dy) + (DIAGONAL - STRAIGHT) * dx.min(dy)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    Blocked(Tile),
    NoPath,
    TooFar(usize),
}

pub fn find_path(world: &WorldCollision<Movement>, plane: i32, from: Tile, to: Tile, max_tiles: usize) -> Result<Vec<Tile>, PathError> {
    for t in [from, to] {
        if !walkable(world, plane, t.0, t.1, BLOCKS) {
            return Err(PathError::Blocked(t));
        }
    }
    let goal = key(to);
    // Best cost so far and the tile it came from.
    let mut seen: TileMap<(u32, u32)> = TileMap::default();
    let mut open = BinaryHeap::new();
    seen.insert(key(from), (0, key(from)));
    open.push((Reverse(estimate(from, to)), Reverse(0u32), key(from)));
    let mut expanded = 0;
    while let Some((_, Reverse(cost), at)) = open.pop() {
        if at == goal {
            let mut path = vec![tile(at)];
            let mut step = at;
            while step != key(from) {
                step = seen[&step].1;
                path.push(tile(step));
            }
            path.reverse();
            return Ok(path);
        }
        if seen.get(&at).is_some_and(|&(best, _)| best < cost) {
            continue;
        }
        expanded += 1;
        if expanded > max_tiles {
            return Err(PathError::TooFar(max_tiles));
        }
        let (x, y) = tile(at);
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
            if !can_step(world, plane, x, y, dx, dy) {
                continue;
            }
            let next = (x + dx, y + dy);
            let cost = cost + if dx != 0 && dy != 0 { DIAGONAL } else { STRAIGHT };
            match seen.entry(key(next)) {
                Entry::Occupied(mut e) if e.get().0 > cost => {
                    e.insert((cost, at));
                }
                Entry::Vacant(e) => {
                    e.insert((cost, at));
                }
                _ => continue,
            }
            open.push((Reverse(cost + estimate(next, to)), Reverse(cost), key(next)));
        }
    }
    Err(PathError::NoPath)
}

// The game's route finder for a walk click (breadth first over 128 x 128 around `from`, west/east/south/north first); unreachable targets end at the nearest tile within 10.
pub mod route_mask {
    pub const WEST: u32 = 0x4024_0108;
    pub const EAST: u32 = 0x4024_0180;
    pub const SOUTH: u32 = 0x4024_0102;
    pub const NORTH: u32 = 0x4024_0120;
    pub const SOUTH_WEST: u32 = 0x4024_010e;
    pub const SOUTH_EAST: u32 = 0x4024_0183;
    pub const NORTH_WEST: u32 = 0x4024_0138;
    pub const NORTH_EAST: u32 = 0x4024_01e0;
}

// Whether the game's route finder can step from (x, y) by (dx, dy), as `game_route` checks inline.
pub fn step_open(flags: &impl Fn(i32, i32) -> Option<u32>, x: i32, y: i32, dx: i32, dy: i32) -> bool {
    use route_mask::*;
    let free = |x: i32, y: i32, mask: u32| flags(x, y).is_some_and(|f| f & mask == 0);
    let side = if dx < 0 { WEST } else { EAST };
    let end = if dy < 0 { SOUTH } else { NORTH };
    match (dx, dy) {
        (0, 0) => false,
        (_, 0) => free(x + dx, y, side),
        (0, _) => free(x, y + dy, end),
        _ => {
            let diagonal = match (dx < 0, dy < 0) {
                (true, true) => SOUTH_WEST,
                (false, true) => SOUTH_EAST,
                (true, false) => NORTH_WEST,
                (false, false) => NORTH_EAST,
            };
            free(x + dx, y + dy, diagonal) && free(x + dx, y, side) && free(x, y + dy, end)
        }
    }
}

const GRAPH: i32 = 128;
const ALTERNATIVE_RADIUS: i32 = 10;
const ALTERNATIVE_MAX_STEPS: u32 = 100;
const START: u8 = 99;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Route {
    // every tile walked, `from` first
    pub tiles: Vec<Tile>,
    // false when the route ends elsewhere because `to` can't be reached
    pub reached: bool,
}

// `flags(x, y)` is the collision at a world tile, None (blocked) off the map.
pub fn game_route(flags: impl Fn(i32, i32) -> Option<u32>, from: Tile, to: Tile) -> Option<Route> {
    use route_mask::*;
    let free = |x: i32, y: i32, mask: u32| flags(x, y).is_some_and(|f| f & mask == 0);
    let (ox, oy) = (from.0 - GRAPH / 2, from.1 - GRAPH / 2);
    let index = |x: i32, y: i32| ((x - ox) * GRAPH + (y - oy)) as usize;
    let size = (GRAPH * GRAPH) as usize;
    // the direction back to where the tile was reached from (Java's bits: 1 north, 2 east, 4 south, 8 west), and its steps
    let mut via = vec![0u8; size];
    let mut steps = vec![u32::MAX; size];
    via[index(from.0, from.1)] = START;
    steps[index(from.0, from.1)] = 0;
    let mut queue = std::collections::VecDeque::from([from]);
    let mut reached = false;
    while let Some((x, y)) = queue.pop_front() {
        if (x, y) == to {
            reached = true;
            break;
        }
        let (gx, gy) = (x - ox, y - oy);
        let next = steps[index(x, y)] + 1;
        let (west, east, south, north) = (gx > 0, gx < GRAPH - 1, gy > 0, gy < GRAPH - 1);
        // Moves are checked only when on the graph and not yet reached; `free` only reads, so skipping reached tiles changes nothing.
        macro_rules! step {
            ($inside:expr, $dx:expr, $dy:expr, $back:expr, $open:expr) => {
                if $inside {
                    let i = index(x + $dx, y + $dy);
                    if via[i] == 0 && $open {
                        via[i] = $back;
                        steps[i] = next;
                        queue.push_back((x + $dx, y + $dy));
                    }
                }
            };
        }
        step!(west, -1, 0, 2, free(x - 1, y, WEST));
        step!(east, 1, 0, 8, free(x + 1, y, EAST));
        step!(south, 0, -1, 1, free(x, y - 1, SOUTH));
        step!(north, 0, 1, 4, free(x, y + 1, NORTH));
        step!(west && south, -1, -1, 3, free(x - 1, y - 1, SOUTH_WEST) && free(x - 1, y, WEST) && free(x, y - 1, SOUTH));
        step!(east && south, 1, -1, 9, free(x + 1, y - 1, SOUTH_EAST) && free(x + 1, y, EAST) && free(x, y - 1, SOUTH));
        step!(west && north, -1, 1, 6, free(x - 1, y + 1, NORTH_WEST) && free(x - 1, y, WEST) && free(x, y + 1, NORTH));
        step!(east && north, 1, 1, 12, free(x + 1, y + 1, NORTH_EAST) && free(x + 1, y, EAST) && free(x, y + 1, NORTH));
    }
    let mut end = to;
    if !reached {
        let mut best: Option<(i32, u32)> = None;
        for x in to.0 - ALTERNATIVE_RADIUS..=to.0 + ALTERNATIVE_RADIUS {
            for y in to.1 - ALTERNATIVE_RADIUS..=to.1 + ALTERNATIVE_RADIUS {
                let (gx, gy) = (x - ox, y - oy);
                if !(0..GRAPH).contains(&gx) || !(0..GRAPH).contains(&gy) || steps[index(x, y)] >= ALTERNATIVE_MAX_STEPS {
                    continue;
                }
                let (dx, dy) = ((x - to.0).abs(), (y - to.1).abs());
                let score = (dx * dx + dy * dy, steps[index(x, y)]);
                if best.is_none_or(|b| score < b) {
                    best = Some(score);
                    end = (x, y);
                }
            }
        }
        best?;
    }
    let mut tiles = vec![end];
    let (mut x, mut y) = end;
    while (x, y) != from {
        let back = via[index(x, y)];
        if back & 2 != 0 {
            x += 1;
        } else if back & 8 != 0 {
            x -= 1;
        }
        if back & 1 != 0 {
            y += 1;
        } else if back & 4 != 0 {
            y -= 1;
        }
        tiles.push((x, y));
    }
    tiles.reverse();
    Some(Route { tiles, reached })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{region_id, Square};

    fn square(rows: &[&str]) -> (WorldCollision<Movement>, i32, i32) {
        let mut world = WorldCollision::<Movement>::default();
        let region = region_id(50, 50);
        world.add_square(region);
        let mut tiles = Box::new([Movement::default(); 64 * 64]);
        for (row, line) in rows.iter().enumerate() {
            let y = rows.len() - 1 - row;
            for (x, c) in line.chars().enumerate() {
                if c == '#' {
                    tiles[x * 64 + y] = Movement(BLOCKS);
                }
            }
        }
        world.set_square(0, region, Square::Tiles(tiles));
        (world, 50 * 64, 50 * 64)
    }

    #[test]
    fn walks_around_a_wall() {
        let (world, bx, by) = square(&[
            ".....", //
            ".###.",
            ".#...",
            ".#...",
        ]);
        let path = find_path(&world, 0, (bx, by), (bx + 2, by), 10_000).unwrap();
        assert_eq!(path.first(), Some(&(bx, by)));
        assert_eq!(path.last(), Some(&(bx + 2, by)));
        for w in path.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            assert!(can_step(&world, 0, w[0].0, w[0].1, dx, dy), "illegal step {w:?}");
        }
        // Up the west side to (1, 3), round the east end of the row at y 2, down to (2, 0): 11 steps, no corner cutting.
        assert_eq!(path.len(), 12, "{path:?}");
    }

    #[test]
    fn no_diagonal_through_a_corner() {
        let (world, bx, by) = square(&[
            "#.", //
            "..",
        ]);
        assert!(!can_step(&world, 0, bx, by, 1, 1));
        assert!(can_step(&world, 0, bx, by, 1, 0));
        let (world, bx, by) = square(&[
            "#.", //
            ".#",
        ]);
        assert!(!can_step(&world, 0, bx, by, 1, 1));
    }

    #[test]
    fn missing_squares_block() {
        let (world, bx, by) = square(&["."]);
        assert!(!can_step(&world, 0, bx, by, -1, 0));
        assert_eq!(find_path(&world, 0, (bx, by), (bx - 5, by), 1000), Err(PathError::Blocked((bx - 5, by))));
    }

    fn route(rows: &[&str], from: Tile, to: Tile) -> Option<Route> {
        let scene = crate::scene_collision::from_rows(0, 0, rows);
        game_route(|x, y| scene.flags(x, y), from, to)
    }

    #[test]
    fn game_route_prefers_west_east_south_north_then_diagonals() {
        let open = [".....", ".....", ".....", ".....", "....."];
        // straight east: no tie to break
        assert_eq!(route(&open, (0, 2), (3, 2)).unwrap().tiles, vec![(0, 2), (1, 2), (2, 2), (3, 2)]);
        // two steps north-east and one east: the BFS takes the diagonals last, so the east step first
        let r = route(&open, (0, 0), (3, 2)).unwrap();
        assert!(r.reached);
        assert_eq!(r.tiles, vec![(0, 0), (1, 0), (2, 1), (3, 2)]);
    }

    #[test]
    fn game_route_goes_around_and_ends_near_unreachable_tiles() {
        let rows = [
            "..#..", //
            "..#..",
            "..#..",
            ".....",
        ];
        let r = route(&rows, (0, 3), (4, 3)).unwrap();
        assert!(r.reached);
        // down the west side, under the wall and up: 8 steps (no diagonal past the wall's end)
        assert_eq!(r.tiles, vec![(0, 3), (0, 2), (0, 1), (1, 0), (2, 0), (3, 0), (3, 1), (3, 2), (4, 3)]);
        // a closed box: the nearest tile outside it
        let rows = [
            ".....", //
            ".###.",
            ".#.#.",
            ".###.",
            ".....",
        ];
        let r = route(&rows, (0, 0), (2, 2)).unwrap();
        assert!(!r.reached);
        // (0, 2) and (2, 0) tie (4 away, 2 steps): the lower x wins
        assert_eq!(r.tiles.last(), Some(&(0, 2)));
        // a blocked click: the nearest free tile
        let r = route(&rows, (0, 2), (1, 2)).unwrap();
        assert_eq!(r.tiles, vec![(0, 2)]);
    }

    #[test]
    fn walls_block_both_sides() {
        let mut world = WorldCollision::<Movement>::default();
        let region = region_id(50, 50);
        world.add_square(region);
        let mut tiles = Box::new([Movement::default(); 64 * 64]);
        // A wall on the east edge of (0, 0), as the client marks it on both tiles.
        tiles[0] = Movement(flag::WALL_EAST as u16);
        tiles[64] = Movement(flag::WALL_WEST as u16);
        world.set_square(0, region, Square::Tiles(tiles));
        let (bx, by) = (50 * 64, 50 * 64);
        assert!(!can_step(&world, 0, bx, by, 1, 0));
        assert!(!can_step(&world, 0, bx + 1, by, -1, 0));
        assert!(can_step(&world, 0, bx, by, 0, 1));
        let path = find_path(&world, 0, (bx, by), (bx + 1, by), 1000).unwrap();
        assert_eq!(path.len(), 4, "{path:?}");
    }
}
