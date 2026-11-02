//! Which tiles can be walked between: every walkable tile of a plane gets the number of the landmass it is on, found by flood fill over the world collision from the cache.
//!
//! Closed doors count as open (a door is a way through, and the walker opens it), and diagonal steps add nothing a straight one doesn't, so only straight steps are followed.
//! Stairs, ladders and other shortcuts are not followed: a tile on another plane or in a dungeon is a different landmass from the surface, so use this for the surface.

use crate::{
    collision::{Movement, REGION_SIZE, Square, WorldCollision},
    pathfinding::can_step_with,
};

const SQUARES: usize = 1 << 16;
const TILES: usize = (REGION_SIZE * REGION_SIZE) as usize;

/// The landmass of each walkable tile of one plane; 0 is "no landmass" (a tile that can't be stood on, or outside the map).
pub struct Landmasses {
    squares: Vec<Option<Box<[u32; TILES]>>>,
    /// How many landmasses there are (their numbers are 1 to this).
    pub count: u32,
    /// Landmasses joined into another by `join`: its number, by the number it was joined into.
    merged: std::collections::HashMap<u32, u32>,
}

fn square_of(x: i32, y: i32) -> Option<(usize, usize)> {
    if !(0..REGION_SIZE * 256).contains(&x) || !(0..REGION_SIZE * 256).contains(&y) {
        return None;
    }
    Some((
        (((x / REGION_SIZE) << 8) | (y / REGION_SIZE)) as usize,
        ((x % REGION_SIZE) * REGION_SIZE + y % REGION_SIZE) as usize,
    ))
}

fn standable(world: &WorldCollision<Movement>, plane: i32, x: i32, y: i32) -> bool {
    world
        .flags(plane, x, y)
        .is_some_and(|m| m.opened().0 & Movement::BLOCKS == 0)
}

impl Landmasses {
    /// Labels the walkable tiles of `plane`.
    pub fn build(world: &WorldCollision<Movement>, plane: i32) -> Self {
        let mut squares: Vec<Option<Box<[u32; TILES]>>> = (0..SQUARES).map(|_| None).collect();
        for (region, square) in squares.iter_mut().enumerate() {
            if !matches!(world.square(plane as usize, region as u32), Square::Missing) {
                *square = Some(Box::new([0; TILES]));
            }
        }
        let mut count = 0;
        let mut stack: Vec<(i32, i32)> = Vec::new();
        for region in 0..SQUARES {
            if squares[region].is_none() {
                continue;
            }
            let (rx, ry) = (
                (region >> 8) as i32 * REGION_SIZE,
                (region & 0xff) as i32 * REGION_SIZE,
            );
            for dx in 0..REGION_SIZE {
                for dy in 0..REGION_SIZE {
                    let (x, y) = (rx + dx, ry + dy);
                    let tile = (dx * REGION_SIZE + dy) as usize;
                    if squares[region].as_ref().is_some_and(|s| s[tile] != 0)
                        || !standable(world, plane, x, y)
                    {
                        continue;
                    }
                    count += 1;
                    squares[region].as_mut().expect("checked")[tile] = count;
                    stack.push((x, y));
                    while let Some((x, y)) = stack.pop() {
                        for (sx, sy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let (nx, ny) = (x + sx, y + sy);
                            let Some((square, index)) = square_of(nx, ny) else {
                                continue;
                            };
                            let Some(labels) = squares[square].as_mut() else {
                                continue;
                            };
                            if labels[index] != 0
                                || !standable(world, plane, nx, ny)
                                || !can_step_with(world, plane, x, y, sx, sy, true)
                            {
                                continue;
                            }
                            labels[index] = count;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        Self {
            squares,
            count,
            merged: Default::default(),
        }
    }

    /// The landmass of a tile, 0 when it has none. Landmasses joined by `join` have one number.
    pub fn at(&self, x: i32, y: i32) -> u32 {
        square_of(x, y)
            .and_then(|(square, index)| self.squares[square].as_ref().map(|s| s[index]))
            .map_or(0, |landmass| self.root(landmass))
    }

    fn root(&self, mut landmass: u32) -> u32 {
        while let Some(&into) = self.merged.get(&landmass) {
            landmass = into;
        }
        landmass
    }

    /// Makes two landmasses one: a way between them that is always open (a stile, a door that is an object, a log across a stream).
    pub fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.root(a), self.root(b));
        if a != 0 && b != 0 && a != b {
            self.merged.insert(a, b);
        }
    }

    /// The landmass of a tile or, when it has none (an object stands on it, say), of the nearest tile that has one within `radius`.
    pub fn near(&self, x: i32, y: i32, radius: i32) -> u32 {
        for r in 0..=radius {
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx.abs().max(dy.abs()) == r {
                        let landmass = self.at(x + dx, y + dy);
                        if landmass != 0 {
                            return landmass;
                        }
                    }
                }
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::flag;

    /// A world of one open map square (64x64) with a wall of blocked tiles at x = 20 and, when asked, a door in it.
    fn world(door_at: Option<i32>) -> WorldCollision<Movement> {
        let mut w = WorldCollision::<Movement>::default();
        w.add_square(0x0101);
        let (rx, ry) = (64, 64);
        for y in 0..64 {
            let blocked = if Some(y) == door_at {
                // a closed door in the wall: its tile blocks until opened
                flag::OBJECT | flag::DOOR_OBJECT
            } else {
                flag::OBJECT
            };
            w.add_for_test(0, rx + 20, ry + y, blocked);
        }
        w
    }

    #[test]
    fn a_wall_splits_a_square_in_two_and_the_sides_are_numbered() {
        let l = Landmasses::build(&world(None), 0);
        let (west, east) = (l.at(64 + 5, 64 + 5), l.at(64 + 40, 64 + 5));
        assert!(west != 0 && east != 0 && west != east);
        assert_eq!(l.count, 2);
        assert_eq!(l.at(64 + 20, 64 + 5), 0, "the wall itself has none");
        assert_eq!(l.at(10, 10), 0, "outside the map has none");
    }

    #[test]
    fn a_join_makes_two_landmasses_one() {
        // a wall with no door: two landmasses, west and east of it
        let mut masses = Landmasses::build(&world(None), 0);
        let (west, east) = (masses.at(64 + 5, 64 + 5), masses.at(64 + 40, 64 + 5));
        assert_eq!(masses.count, 2);
        assert_ne!(west, east);
        masses.join(west, east);
        assert_eq!(masses.at(64 + 5, 64 + 5), masses.at(64 + 40, 64 + 5));
        assert_eq!(
            masses.near(64 + 5, 64 + 5, 0),
            masses.near(64 + 40, 64 + 5, 0)
        );
        // joining the same pair again, or one with nothing, changes nothing
        masses.join(west, east);
        masses.join(0, east);
        assert_eq!(masses.at(64 + 5, 64 + 5), masses.at(64 + 40, 64 + 5));
    }

    #[test]
    fn a_closed_door_joins_the_sides() {
        let l = Landmasses::build(&world(Some(30)), 0);
        assert_eq!(l.at(64 + 5, 64 + 5), l.at(64 + 40, 64 + 5));
        assert_eq!(l.count, 1);
    }

    #[test]
    fn near_finds_the_closest_landmass() {
        let l = Landmasses::build(&world(None), 0);
        assert_eq!(
            l.near(64 + 20, 64 + 5, 0),
            0,
            "the wall has none of its own"
        );
        assert_ne!(l.near(64 + 20, 64 + 5, 2), 0);
        assert_eq!(l.near(10, 10, 3), 0);
    }
}
