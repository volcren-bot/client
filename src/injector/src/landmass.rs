use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
};

use crate::{
    collision::{Movement, REGION_SIZE, SQUARES, Square, TILES, WorldCollision, square_index},
    nav::{Closed, PLANES, Point, fold},
    pathfinding::{DIRECTIONS, can_step_with},
};

type Labels = Option<Box<[u32; TILES]>>;

pub struct Landmasses {
    planes: Vec<Vec<Labels>>,
    pub count: u32,
    merged: HashMap<u32, u32>,
}

fn standable(world: &WorldCollision<Movement>, plane: i32, x: i32, y: i32) -> bool {
    world
        .flags(plane, x, y)
        .is_some_and(|m| m.opened().0 & (Movement::BLOCKS | Movement::NO_FLOOR) == 0)
}

fn label_at(squares: &[Labels], square: usize, tile: usize) -> u32 {
    squares[square].as_ref().map_or(0, |labels| labels[tile])
}

fn set_label(squares: &mut [Labels], square: usize, tile: usize, label: u32) {
    squares[square].get_or_insert_with(|| Box::new([0; TILES]))[tile] = label;
}

impl Landmasses {
    pub fn build(world: &WorldCollision<Movement>, closed: &Closed) -> Self {
        let mut count = 0;
        let planes = (0..PLANES)
            .map(|plane| Self::label(world, plane, closed, &mut count))
            .collect();
        Self {
            planes,
            count,
            merged: HashMap::new(),
        }
    }

    fn label(
        world: &WorldCollision<Movement>,
        plane: i32,
        closed: &Closed,
        count: &mut u32,
    ) -> Vec<Labels> {
        let mut squares: Vec<Labels> = (0..SQUARES).map(|_| None).collect();
        let mut stack: Vec<(i32, i32)> = Vec::new();
        for region in 0..SQUARES {
            if matches!(world.square(plane as usize, region as u32), Square::Missing) {
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
                    if label_at(&squares, region, tile) != 0 || !standable(world, plane, x, y) {
                        continue;
                    }
                    *count += 1;
                    set_label(&mut squares, region, tile, *count);
                    stack.push((x, y));
                    while let Some((x, y)) = stack.pop() {
                        for &(sx, sy) in &DIRECTIONS[..4] {
                            let (nx, ny) = (x + sx, y + sy);
                            let Some((square, index)) = square_index(nx, ny) else {
                                continue;
                            };
                            if label_at(&squares, square, index) != 0
                                || !standable(world, plane, nx, ny)
                                || !can_step_with(world, plane, x, y, sx, sy, true)
                                || closed.contains(&((x, y, plane), (sx, sy)))
                            {
                                continue;
                            }
                            set_label(&mut squares, square, index, *count);
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        squares
    }

    fn raw(&self, x: i32, y: i32, plane: i32) -> u32 {
        let Some(squares) = usize::try_from(plane).ok().and_then(|p| self.planes.get(p)) else {
            return 0;
        };
        square_index(x, y).map_or(0, |(square, index)| label_at(squares, square, index))
    }

    pub fn at(&self, x: i32, y: i32, plane: i32) -> u32 {
        self.root(self.raw(x, y, plane))
    }

    fn root(&self, mut landmass: u32) -> u32 {
        while let Some(&into) = self.merged.get(&landmass) {
            landmass = into;
        }
        landmass
    }

    pub fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.root(a), self.root(b));
        if a != 0 && b != 0 && a != b {
            self.merged.insert(a, b);
        }
    }

    pub fn near(&self, x: i32, y: i32, plane: i32, radius: i32) -> u32 {
        self.root(self.raw_near(x, y, plane, radius))
    }

    fn raw_near(&self, x: i32, y: i32, plane: i32, radius: i32) -> u32 {
        for r in 0..=radius {
            for dx in -r..=r {
                for dy in -r..=r {
                    if dx.abs().max(dy.abs()) == r {
                        let landmass = self.raw(x + dx, y + dy, plane);
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

const CLIMB_COST: u32 = 20;

struct Link {
    from: Point,
    to: Point,
    to_mass: u32,
    key: Option<String>,
    wilderness: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Unknown,
    None,
    Walk,
    Through(Point, Point),
}

fn distance(a: Point, b: Point) -> u32 {
    (a.0 - b.0)
        .unsigned_abs()
        .max((fold(a).1 - fold(b).1).unsigned_abs())
}

pub struct WorldMap {
    pub masses: Landmasses,
    links: Vec<Link>,
    by_mass: HashMap<u32, Vec<usize>>,
    radius: i32,
}

impl WorldMap {
    pub fn new(
        mut masses: Landmasses,
        rows: impl IntoIterator<Item = (Point, Point, Option<String>, bool)>,
        exits: impl IntoIterator<Item = (Point, Point)>,
        radius: i32,
    ) -> Self {
        let mut links = Vec::new();
        let mut by_mass: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut link = |from: Point, to: Point, key: Option<String>, wilderness: bool| {
            let from_mass = masses.raw_near(from.0, from.1, from.2, radius);
            let to_mass = masses.raw_near(to.0, to.1, to.2, radius);
            if from_mass == 0 || to_mass == 0 || from_mass == to_mass {
                return None;
            }
            by_mass.entry(from_mass).or_default().push(links.len());
            links.push(Link {
                from,
                to,
                to_mass,
                key,
                wilderness,
            });
            Some((from_mass, to_mass))
        };
        let joins: Vec<(u32, u32)> = rows
            .into_iter()
            .filter_map(|(from, to, key, wilderness)| {
                let joins = key.is_none();
                link(from, to, key, wilderness).filter(|_| joins)
            })
            .collect();
        for (from, to) in exits {
            link(from, to, None, false);
        }
        for (a, b) in joins {
            masses.join(a, b);
        }
        Self {
            masses,
            links,
            by_mass,
            radius,
        }
    }

    pub fn route(
        &self,
        from: Point,
        to: Point,
        allowed: &HashSet<&str>,
        wilderness: bool,
    ) -> Route {
        let start = self.masses.raw_near(from.0, from.1, from.2, self.radius);
        let goal = self.masses.raw_near(to.0, to.1, to.2, self.radius);
        if start == 0 || goal == 0 {
            return Route::Unknown;
        }
        if start == goal {
            return Route::Walk;
        }
        let usable = |link: &Link| {
            (wilderness || !link.wilderness)
                && link.key.as_deref().is_none_or(|k| allowed.contains(k))
        };
        let mut heap = BinaryHeap::new();
        let mut seen = vec![false; self.links.len()];
        for &i in self.by_mass.get(&start).into_iter().flatten() {
            if usable(&self.links[i]) {
                heap.push(Reverse((
                    distance(from, self.links[i].from) + CLIMB_COST,
                    i,
                    i,
                )));
            }
        }
        while let Some(Reverse((cost, i, first))) = heap.pop() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            let link = &self.links[i];
            if link.to_mass == goal {
                let f = &self.links[first];
                return Route::Through(f.from, f.to);
            }
            for &next in self.by_mass.get(&link.to_mass).into_iter().flatten() {
                if !seen[next] && usable(&self.links[next]) {
                    let step = distance(link.to, self.links[next].from) + CLIMB_COST;
                    heap.push(Reverse((cost + step, next, first)));
                }
            }
        }
        Route::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::flag;

    fn world(door_at: Option<i32>) -> WorldCollision<Movement> {
        let mut w = WorldCollision::<Movement>::default();
        w.add_square(0x0101);
        let (rx, ry) = (64, 64);
        for y in 0..64 {
            let blocked = if Some(y) == door_at {
                flag::OBJECT | flag::DOOR_OBJECT
            } else {
                flag::OBJECT
            };
            w.add(0, rx + 20, ry + y, blocked);
        }
        w
    }

    #[test]
    fn a_wall_splits_a_square_in_two_and_the_sides_are_numbered() {
        let l = Landmasses::build(&world(None), &Default::default());
        let (west, east) = (l.at(64 + 5, 64 + 5, 0), l.at(64 + 40, 64 + 5, 0));
        assert!(west != 0 && east != 0 && west != east);
        assert_eq!(l.count, 2 + 3, "and the three other planes are open");
        assert_eq!(l.at(64 + 20, 64 + 5, 0), 0, "the wall itself has none");
        assert_eq!(l.at(10, 10, 0), 0, "outside the map has none");
    }

    #[test]
    fn a_join_makes_two_landmasses_one() {
        let mut masses = Landmasses::build(&world(None), &Default::default());
        let (west, east) = (masses.at(64 + 5, 64 + 5, 0), masses.at(64 + 40, 64 + 5, 0));
        assert_eq!(masses.count, 2 + 3);
        assert_ne!(west, east);
        masses.join(west, east);
        assert_eq!(masses.at(64 + 5, 64 + 5, 0), masses.at(64 + 40, 64 + 5, 0));
        assert_eq!(
            masses.near(64 + 5, 64 + 5, 0, 0),
            masses.near(64 + 40, 64 + 5, 0, 0)
        );
        masses.join(west, east);
        masses.join(0, east);
        assert_eq!(masses.at(64 + 5, 64 + 5, 0), masses.at(64 + 40, 64 + 5, 0));
    }

    #[test]
    fn a_closed_door_joins_the_sides() {
        let l = Landmasses::build(&world(Some(30)), &Default::default());
        assert_eq!(l.at(64 + 5, 64 + 5, 0), l.at(64 + 40, 64 + 5, 0));
        assert_eq!(l.count, 1 + 3);
    }

    #[test]
    fn near_finds_the_closest_landmass() {
        let l = Landmasses::build(&world(None), &Default::default());
        assert_eq!(
            l.near(64 + 20, 64 + 5, 0, 0),
            0,
            "the wall has none of its own"
        );
        assert_ne!(l.near(64 + 20, 64 + 5, 0, 2), 0);
        assert_eq!(l.near(10, 10, 0, 3), 0);
    }

    fn map(rows: Vec<(Point, Point, Option<String>, bool)>) -> WorldMap {
        WorldMap::new(
            Landmasses::build(&world(None), &Default::default()),
            rows,
            [],
            0,
        )
    }

    #[test]
    fn a_door_that_asks_splits_the_map_and_only_its_row_and_its_exit_cross() {
        let mut closed = Closed::default();
        let (inside, outside): (Point, Point) = ((64 + 20, 64 + 30, 0), (64 + 21, 64 + 30, 0));
        closed.insert((inside, (1, 0)));
        closed.insert((outside, (-1, 0)));
        let masses = Landmasses::build(&world(Some(30)), &closed);
        let (west, east) = (masses.at(64 + 5, 64 + 5, 0), masses.at(64 + 40, 64 + 5, 0));
        assert!(west != 0 && east != 0 && west != east, "{west} {east}");
        let key = "door".to_owned();
        let map = WorldMap::new(
            masses,
            [(inside, outside, Some(key.clone()), false)],
            [(outside, inside)],
            0,
        );
        let (w, e) = ((64 + 5, 64 + 5, 0), (64 + 40, 64 + 5, 0));
        let none = HashSet::new();
        let allowed = HashSet::from([key.as_str()]);
        assert_eq!(
            map.route(w, e, &none, false),
            Route::None,
            "in needs the row"
        );
        assert_eq!(
            map.route(w, e, &allowed, false),
            Route::Through(inside, outside)
        );
        assert_eq!(
            map.route(e, w, &none, false),
            Route::Through(outside, inside),
            "out is free"
        );
        assert_ne!(
            map.masses.at(w.0, w.1, 0),
            map.masses.at(e.0, e.1, 0),
            "and doesn't join them"
        );
    }

    const WEST: Point = (64 + 5, 64 + 5, 0);
    const EAST: Point = (64 + 40, 64 + 5, 0);
    const WEST_DOOR: Point = (64 + 19, 64 + 9, 0);
    const EAST_DOOR: Point = (64 + 21, 64 + 9, 0);

    #[test]
    fn the_same_landmass_is_walked_and_nothing_between_two_is_no_way() {
        let m = map(vec![]);
        let none = Default::default();
        assert_eq!(
            m.route(WEST, (64 + 9, 64 + 9, 0), &none, false),
            Route::Walk
        );
        assert_eq!(m.route(WEST, EAST, &none, false), Route::None);
        assert_eq!(m.route(WEST, (10, 10, 0), &none, false), Route::Unknown);
    }

    #[test]
    fn a_route_takes_a_climb_in_its_direction_only() {
        let m = map(vec![(WEST_DOOR, EAST_DOOR, None, false)]);
        let none = Default::default();
        assert_eq!(
            m.route(WEST, EAST, &none, false),
            Route::Through(WEST_DOOR, EAST_DOOR)
        );
        assert_eq!(
            m.route(EAST, WEST, &none, false),
            Route::None,
            "it is one way"
        );
        assert_eq!(
            m.masses.at(WEST.0, WEST.1, 0),
            m.masses.at(EAST.0, EAST.1, 0),
            "but it joins the landmasses"
        );
    }

    #[test]
    fn a_climb_that_asks_for_something_needs_allowing_and_doesnt_join() {
        let m = map(vec![(WEST_DOOR, EAST_DOOR, Some("key".to_owned()), false)]);
        assert_eq!(m.route(WEST, EAST, &Default::default(), false), Route::None);
        let allowed = HashSet::from(["key"]);
        assert_eq!(
            m.route(WEST, EAST, &allowed, false),
            Route::Through(WEST_DOOR, EAST_DOOR)
        );
        assert_ne!(
            m.masses.at(WEST.0, WEST.1, 0),
            m.masses.at(EAST.0, EAST.1, 0)
        );
    }

    #[test]
    fn a_wilderness_climb_is_only_for_a_walk_there() {
        let m = map(vec![(WEST_DOOR, EAST_DOOR, None, true)]);
        assert_eq!(m.route(WEST, EAST, &Default::default(), false), Route::None);
        assert_eq!(
            m.route(WEST, EAST, &Default::default(), true),
            Route::Through(WEST_DOOR, EAST_DOOR)
        );
    }
}
