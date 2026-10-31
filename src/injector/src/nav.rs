// Walking plans over every plane: closed doors are opened by the walk, ladders and staircases join the planes; pure logic, tested natively.

use std::{cmp::Reverse, collections::BinaryHeap};

use crate::{
    collision::{flag, wall_marks, LocDef, LocText},
    pathfinding::step_open,
    scene_graph::mix,
    Loc,
};

/// A world tile and its plane: (x, y, plane).
pub type Point = (i32, i32, i32);

// The underground's tile laid over the surface: its planes come after the surface's.
fn fold((x, y, plane): Point) -> Point {
    if y >= UNDERGROUND {
        (x, y - UNDERGROUND, plane + PLANES)
    } else {
        (x, y, plane)
    }
}

fn unfold((x, y, plane): Point) -> Point {
    if plane >= PLANES {
        (x, y + UNDERGROUND, plane - PLANES)
    } else {
        (x, y, plane)
    }
}

pub const PLANES: i32 = 4;
// the underground (dungeons) is a copy of the map this many tiles north (100 map squares), entered from plane 0 by ladders, caves and the like
pub const UNDERGROUND: i32 = 6400;
// the surface and the underground: planned together, the underground folded onto `PLANES` more planes
const WORLDS: i32 = 2;
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 11;
// a tile of the scene's edge ring is all ones
const RING: u32 = 0xFF_FFFF;
// what can't be stood on
const STAND_BLOCKS: u32 = flag::OBJECT | flag::FLOOR | flag::FLOOR_DECORATION;
// how far apart (tiles between their footprints) the two ends of a ladder or staircase may be
pub const PAIR_RADIUS: i32 = 6;

/// A loc the plan uses: what the walk clicks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Target {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub kind: u8,
    pub size_x: u8,
    pub size_y: u8,
    pub option: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    pub loc: Target,
    // the bits it puts on tiles of its plane, as (x, y, bits): what planning takes away
    pub blocks: Vec<(i32, i32, u32)>,
}

/// A ladder, staircase or trapdoor: used from any `reach` tile, it puts the player on one of the `lands` of `to_plane`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climb {
    pub loc: Target,
    pub to_plane: i32,
    pub reach: Vec<(i32, i32)>,
    pub lands: Vec<(i32, i32)>,
}

/// A loc with a climbing option: `dir` is the planes it climbs (1 up, -1 down, 0 for a portal that keeps the plane), `solid` whether it can't be stood in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climbable {
    pub loc: Target,
    pub dir: i32,
    pub solid: bool,
}

/// The collision of (x, y, plane); `None` is off the map.
pub type Flags<'a> = &'a dyn Fn(i32, i32, i32) -> Option<u32>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    // the walk is done this close (tiles, any direction) to the goal on its plane
    pub near: i32,
    // what opening a door and climbing add, in tenths of a tile
    pub door_cost: u32,
    pub climb_cost: u32,
    // with a seed, each tile's cost is scaled by 1 +- rnd / 2: the same seed plans the same way
    pub seed: Option<u64>,
    pub rnd: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { near: 0, door_cost: 30, climb_cost: 50, seed: None, rnd: 0.6 }
    }
}

/// What a walk asks the planner for, besides where from and where to.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub options: Options,
    // the menu options that open a door, and those that climb with the planes they climb, any case
    pub door_options: Vec<String>,
    pub climb_options: Vec<(String, i32)>,
    // the menu options that lead into or out of the underground without climbing a plane (a cave's "Enter"), any case: used only with a loc at the other end
    pub portal_options: Vec<String>,
    // names (part of, any case) of the locs whose door option opens a way somewhere, not a door: a trapdoor
    pub portal_names: Vec<String>,
    // locs not to use, as (id, x, y, plane): a door that didn't open, say
    pub avoid: Vec<(i32, i32, i32, i32)>,
}

impl Default for Request {
    fn default() -> Self {
        Request {
            options: Options::default(),
            door_options: vec!["Open".into()],
            climb_options: vec![("Climb-up".into(), 1), ("Climb-down".into(), -1)],
            portal_options: ["Enter", "Exit", "Squeeze-through", "Climb"].map(String::from).to_vec(),
            portal_names: ["trapdoor", "manhole"].map(String::from).to_vec(),
            avoid: Vec::new(),
        }
    }
}

const WALL: u8 = 0;
const GAME: u8 = 2;

/// The closed doors and climbable locs of the scene.
pub fn scene_locs<'a>(flags: Flags, locs: &[Loc], defs: &dyn Fn(i32) -> Option<&'a (LocDef, LocText)>, request: &Request) -> (Vec<Door>, Vec<Climbable>) {
    let (mut doors, mut climbables) = (Vec::new(), Vec::new());
    for loc in locs.iter().filter(|l| l.clickable && !request.avoid.contains(&(l.id, l.x, l.y, l.plane))) {
        let Some((def, text)) = defs(loc.id) else { continue };
        let offered = |wanted: &str| text.actions.iter().find(|a| a.eq_ignore_ascii_case(wanted)).cloned();
        let target = |option: String| Target { id: loc.id, x: loc.x, y: loc.y, plane: loc.plane, kind: loc.kind, size_x: loc.size_x, size_y: loc.size_y, option };
        let on = |x: i32, y: i32| flags(x, y, loc.plane);
        let door = request.door_options.iter().find_map(|o| offered(o));
        if let Some(option) = door.filter(|_| def.interact_type != 0) {
            let shape = match (loc.kind, loc.shape, loc.rotation) {
                (WALL, Some(shape @ (0 | 2)), Some(rotation)) => Some((shape, rotation)),
                (WALL, None, _) => guess_wall(&on, loc.x, loc.y),
                (GAME, Some(9), Some(rotation)) => Some((9, rotation)),
                _ => None,
            };
            let mut made = false;
            if let Some((shape, rotation)) = shape {
                let loc = target(option.clone());
                let blocks = door_blocks(&on, &loc, shape, rotation);
                if !blocks.is_empty() {
                    doors.push(Door { loc, blocks });
                    made = true;
                }
            }
            // a closed trapdoor: opening it opens the way, the climb comes after
            let name = text.name.to_lowercase();
            if !made && request.portal_names.iter().any(|n| name.contains(&n.to_lowercase())) {
                climbables.push(Climbable { loc: target(option), dir: 0, solid: loc.kind == GAME && def.interact_type != 0 });
            }
            continue;
        }
        for (option, dir) in &request.climb_options {
            if let Some(option) = offered(option) {
                climbables.push(Climbable { loc: target(option), dir: *dir, solid: loc.kind == GAME && def.interact_type != 0 });
            }
        }
        for option in &request.portal_options {
            if let Some(option) = offered(option) {
                climbables.push(Climbable { loc: target(option), dir: 0, solid: loc.kind == GAME && def.interact_type != 0 });
            }
        }
    }
    (doors, climbables)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Use {
    Door(Target),
    Climb(Target, i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    // the index in `tiles` it is used from; the next tile is past it
    pub at: usize,
    pub what: Use,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    // every tile walked, `from` first
    pub tiles: Vec<Point>,
    pub actions: Vec<Action>,
    // false when it ends elsewhere, as close to the goal as it gets
    pub reached: bool,
}

fn standable(flags: u32) -> bool {
    flags != RING && flags & STAND_BLOCKS == 0
}

// The wall bit on `from`'s side of the edge it shares with the tile (dx, dy) away.
fn wall_toward(dx: i32, dy: i32) -> u32 {
    match (dx, dy) {
        (1, 0) => flag::WALL_EAST,
        (-1, 0) => flag::WALL_WEST,
        (0, 1) => flag::WALL_NORTH,
        _ => flag::WALL_SOUTH,
    }
}

/// The tiles a loc can be used from: beside its footprint without a wall between, and inside it when walkable.
pub fn reach(flags: &dyn Fn(i32, i32) -> Option<u32>, x: i32, y: i32, size_x: i32, size_y: i32, solid: bool) -> Vec<(i32, i32)> {
    let mut tiles = Vec::new();
    let inside = |tx: i32, ty: i32| (x..x + size_x).contains(&tx) && (y..y + size_y).contains(&ty);
    for tx in x - 1..=x + size_x {
        for ty in y - 1..=y + size_y {
            let Some(f) = flags(tx, ty).filter(|&f| standable(f)) else { continue };
            if inside(tx, ty) {
                if !solid {
                    tiles.push((tx, ty));
                }
                continue;
            }
            // the way into the footprint; only the sides count, a corner touches it diagonally
            let into = |t: i32, start: i32, size: i32| if t < start { 1 } else if t >= start + size { -1 } else { 0 };
            let (dx, dy) = (into(tx, x, size_x), into(ty, y, size_y));
            if dx != 0 && dy != 0 {
                continue;
            }
            let wall = wall_toward(dx, dy);
            let into = flags(tx + dx, ty + dy).unwrap_or(0);
            if f & wall == 0 && into & wall_toward(-dx, -dy) == 0 {
                tiles.push((tx, ty));
            }
        }
    }
    tiles
}

fn footprint_distance(a: &Target, b: &Target) -> i32 {
    let gap = |a1: i32, a_size: u8, b1: i32, b_size: u8| (b1 - (a1 + a_size as i32 - 1)).max(a1 - (b1 + b_size as i32 - 1)).max(0);
    gap(a.x, a.size_x, b.x, b.size_x).max(gap(a.y, a.size_y, b.y, b.size_y))
}

/// The nearest of `candidates` (within `PAIR_RADIUS` of `from` moved `shift` tiles north) on `plane` that `accepts`.
fn nearest<'a>(from: &Target, shift: i32, plane: i32, candidates: &'a [Climbable], accepts: impl Fn(&Climbable) -> bool) -> Option<&'a Climbable> {
    let from = Target { y: from.y + shift, ..from.clone() };
    candidates
        .iter()
        .filter(|o| o.loc.plane == plane && accepts(o))
        .map(|o| (footprint_distance(&from, &o.loc), o))
        .filter(|(d, _)| *d <= PAIR_RADIUS)
        .min_by_key(|(d, o)| (*d, o.loc.x, o.loc.y))
        .map(|(_, o)| o)
}

/// The climbs of `locs`, each paired with the nearest loc on the plane it leads to that climbs back.
/// A way up from the underground's plane 0 with no such loc, a way down from the surface's plane 0 and a portal lead to the nearest loc of the other world at the same spot 100 map squares away.
pub fn climbs(flags: Flags, locs: &[Climbable]) -> Vec<Climb> {
    let mut out = Vec::new();
    for c in locs {
        let to_plane = c.loc.plane + c.dir;
        let on = |plane: i32| move |tx: i32, ty: i32| flags(tx, ty, plane);
        let (x, y, w, h) = (c.loc.x, c.loc.y, c.loc.size_x as i32, c.loc.size_y as i32);
        let from_reach = reach(&on(c.loc.plane), x, y, w, h, c.solid);
        if from_reach.is_empty() {
            continue;
        }
        let lands_of = |p: &Climbable| reach(&on(p.loc.plane), p.loc.x, p.loc.y, p.loc.size_x as i32, p.loc.size_y as i32, p.solid);
        // where it can lead: the plane (and its lands) for each way
        let mut ways: Vec<(i32, Vec<(i32, i32)>)> = Vec::new();
        let stairs = c.dir != 0 && (0..PLANES).contains(&to_plane);
        let partner = if stairs { nearest(&c.loc, 0, to_plane, locs, |o| o.dir == -c.dir) } else { None };
        if let Some(p) = partner {
            ways.push((to_plane, lands_of(p)));
        }
        // a portal that keeps no direction ("Climb" on steps) leads to the plane above or below that has one beside it
        if c.dir == 0 {
            for plane in [c.loc.plane - 1, c.loc.plane + 1].into_iter().filter(|p| (0..PLANES).contains(p)) {
                if let Some(p) = nearest(&c.loc, 0, plane, locs, |o| o.dir == 0) {
                    ways.push((plane, lands_of(p)));
                }
            }
        }
        // across the worlds: from the surface to the underground, or back
        let underground = c.loc.y >= UNDERGROUND;
        let mirrored = c.loc.plane == 0 && (!stairs || partner.is_none()) && (c.dir == 0 || (c.dir > 0) == underground);
        if mirrored {
            let shift = if underground { -UNDERGROUND } else { UNDERGROUND };
            let leads_here = |o: &Climbable| if underground { o.dir <= 0 } else { o.dir >= 0 };
            if let Some(p) = nearest(&c.loc, shift, 0, locs, leads_here) {
                // a portal ("Squeeze-through", "Enter") doesn't take the other end of a ladder or stairs that has a closer one of its own on this side
                let claimed = c.dir == 0
                    && locs.iter().any(|q| {
                        q.dir != 0 && q.loc.plane == 0 && (q.loc.y >= UNDERGROUND) == underground && footprint_distance(&Target { y: q.loc.y + shift, ..q.loc.clone() }, &p.loc) < footprint_distance(&Target { y: c.loc.y + shift, ..c.loc.clone() }, &p.loc)
                    });
                if !claimed {
                    ways.push((0, lands_of(p)));
                }
            }
        }
        // no partner anywhere: straight up or down
        if ways.is_empty() && stairs {
            ways.push((to_plane, reach(&on(to_plane), x, y, w, h, false)));
        }
        for (to_plane, lands) in ways {
            if !lands.is_empty() {
                out.push(Climb { loc: c.loc.clone(), to_plane, reach: from_reach.clone(), lands });
            }
        }
    }
    out
}

/// The collision bits a closed door's walls or footprint set, from its shape and rotation (empty when open).
pub fn door_blocks(flags: &dyn Fn(i32, i32) -> Option<u32>, loc: &Target, shape: u8, rotation: u8) -> Vec<(i32, i32, u32)> {
    let mut blocks = Vec::new();
    match shape {
        0 | 2 => {
            for &(dx, dy, wall) in wall_marks(shape, rotation) {
                let (x, y) = (loc.x + dx, loc.y + dy);
                let there = flags(x, y).unwrap_or(0) & wall;
                if there != 0 {
                    blocks.push((x, y, there));
                }
            }
        }
        9 => {
            let (w, h) = if rotation & 1 == 1 { (loc.size_y, loc.size_x) } else { (loc.size_x, loc.size_y) };
            for x in loc.x..loc.x + w as i32 {
                for y in loc.y..loc.y + h as i32 {
                    if flags(x, y).unwrap_or(0) & flag::OBJECT != 0 {
                        blocks.push((x, y, flag::OBJECT));
                    }
                }
            }
        }
        _ => {}
    }
    blocks
}

/// The shape and rotation of a straight closed door from its tile's walls; `None` unless exactly one wall matches.
pub fn guess_wall(flags: &dyn Fn(i32, i32) -> Option<u32>, x: i32, y: i32) -> Option<(u8, u8)> {
    let mut found = None;
    for rotation in 0..4u8 {
        let marks = wall_marks(0, rotation);
        if marks.iter().all(|&(dx, dy, wall)| flags(x + dx, y + dy).unwrap_or(0) & wall != 0) {
            if found.is_some() {
                return None;
            }
            found = Some((0, rotation));
        }
    }
    found
}

const DIRECTIONS: [(i32, i32); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)];
const WALK: u32 = 0;
const DOOR: u32 = 1 << 30;
const CLIMB: u32 = 2 << 30;
const INDEX: u32 = (1 << 30) - 1;

/// Plans from `from` to `to` (planes included) inside `bounds` (x1, y1, x2, y2) on every plane.
/// The underground is planned together with the surface: `bounds` is a box of the surface's coordinates and holds the underground's copy of it too (its tiles are `UNDERGROUND` further north).
pub fn plan(flags: Flags, bounds: (i32, i32, i32, i32), doors: &[Door], climbs: &[Climb], from: Point, to: Point, options: &Options) -> Option<Plan> {
    let (from, to) = (fold(from), fold(to));
    let (x1, y1, x2, y2) = bounds;
    let (w, h) = (x2 - x1 + 1, y2 - y1 + 1);
    if w <= 0 || h <= 0 {
        return None;
    }
    let inside = |x: i32, y: i32, plane: i32| (x1..=x2).contains(&x) && (y1..=y2).contains(&y) && (0..PLANES * WORLDS).contains(&plane);
    let index = |x: i32, y: i32, plane: i32| ((plane * w + (x - x1)) * h + (y - y1)) as usize;
    let point = |i: usize| {
        let i = i as i32;
        let (plane, rest) = (i / (w * h), i % (w * h));
        (x1 + rest / h, y1 + rest % h, plane)
    };
    if !inside(from.0, from.1, from.2) {
        return None;
    }
    let size = (PLANES * WORLDS * w * h) as usize;

    // the doors' bits, taken away while planning, and which door put them there
    let mut opened = vec![0u32; size];
    let mut door_at = vec![0u32; size];
    for (n, door) in doors.iter().enumerate() {
        for &(x, y, bits) in &door.blocks {
            let (x, y, plane) = fold((x, y, door.loc.plane));
            if inside(x, y, plane) {
                let i = index(x, y, plane);
                opened[i] |= bits;
                door_at[i] = n as u32 + 1;
            }
        }
    }
    let mut climbs_at: std::collections::HashMap<usize, Vec<u32>> = std::collections::HashMap::new();
    for (n, climb) in climbs.iter().enumerate() {
        for &(x, y) in &climb.reach {
            let (x, y, plane) = fold((x, y, climb.loc.plane));
            if inside(x, y, plane) {
                climbs_at.entry(index(x, y, plane)).or_default().push(n as u32);
            }
        }
    }
    let original = |x: i32, y: i32, plane: i32| {
        let (rx, ry, rplane) = unfold((x, y, plane));
        inside(x, y, plane).then(|| flags(rx, ry, rplane)).flatten()
    };
    let open = |x: i32, y: i32, plane: i32| original(x, y, plane).map(|f| f & !opened[index(x, y, plane)]);
    let noise = |i: usize, cost: u32| match options.seed {
        Some(seed) => {
            let unit = (mix(seed ^ mix(i as u64)) >> 11) as f64 / (1u64 << 53) as f64;
            (cost as f64 * (1.0 + options.rnd * (unit - 0.5))).round().max(1.0) as u32
        }
        None => cost,
    };
    let near = |(x, y, plane): Point| plane == to.2 && (x - to.0).abs().max((y - to.1).abs()) <= options.near;

    let mut cost = vec![u32::MAX; size];
    let mut back = vec![u32::MAX; size];
    let mut via = vec![WALK; size];
    let start = index(from.0, from.1, from.2);
    cost[start] = 0;
    let mut heap = BinaryHeap::from([(Reverse(0u32), start)]);
    let mut end: Option<usize> = None;
    // the closest the search got to the goal on its plane, and how far that is
    let mut closest: Option<(i32, u32, usize)> = None;
    while let Some((Reverse(c), at)) = heap.pop() {
        if c > cost[at] {
            continue;
        }
        let here = point(at);
        if near(here) {
            end = Some(at);
            break;
        }
        if here.2 == to.2 {
            let d = (here.0 - to.0).abs().max((here.1 - to.1).abs());
            if closest.is_none_or(|(best, best_cost, _)| (d, c) < (best, best_cost)) {
                closest = Some((d, c, at));
            }
        }
        let (x, y, plane) = here;
        let mut relax = |next: usize, step: u32, how: u32, heap: &mut BinaryHeap<(Reverse<u32>, usize)>| {
            let total = c.saturating_add(noise(next, step));
            if total < cost[next] {
                cost[next] = total;
                back[next] = at as u32;
                via[next] = how;
                heap.push((Reverse(total), next));
            }
        };
        let walked = |x: i32, y: i32| original(x, y, plane);
        let through = |x: i32, y: i32| open(x, y, plane);
        for (dx, dy) in DIRECTIONS {
            let (nx, ny) = (x + dx, y + dy);
            if !inside(nx, ny, plane) || !step_open(&through, x, y, dx, dy) {
                continue;
            }
            let step = if dx != 0 && dy != 0 { DIAGONAL } else { STRAIGHT };
            let next = index(nx, ny, plane);
            if step_open(&walked, x, y, dx, dy) {
                relax(next, step, WALK, &mut heap);
                continue;
            }
            // the tiles whose flags the step looks at, the one it lands on first
            let door = [(nx, ny), (nx, y), (x, ny), (x, y)].iter().map(|&(tx, ty)| door_at[index(tx, ty, plane)]).find(|&d| d != 0);
            if let Some(door) = door {
                relax(next, step + options.door_cost, DOOR | (door - 1), &mut heap);
            }
        }
        if let Some(list) = climbs_at.get(&at) {
            for &n in list {
                let climb = &climbs[n as usize];
                for &land in &climb.lands {
                    let (lx, ly, lplane) = fold((land.0, land.1, climb.to_plane));
                    if inside(lx, ly, lplane) {
                        relax(index(lx, ly, lplane), options.climb_cost, CLIMB | n, &mut heap);
                    }
                }
            }
        }
    }

    let (end, reached) = match end {
        Some(end) => (end, true),
        None => (closest?.2, false),
    };
    let mut nodes = vec![end];
    while nodes[nodes.len() - 1] != start {
        nodes.push(back[nodes[nodes.len() - 1]] as usize);
    }
    nodes.reverse();
    let mut actions = Vec::new();
    for (n, &node) in nodes.iter().enumerate().skip(1) {
        let how = via[node];
        let what = match how & !INDEX {
            DOOR => Use::Door(doors[(how & INDEX) as usize].loc.clone()),
            CLIMB => {
                let climb = &climbs[(how & INDEX) as usize];
                Use::Climb(climb.loc.clone(), climb.to_plane)
            }
            _ => continue,
        };
        actions.push(Action { at: n - 1, what });
    }
    Some(Plan { tiles: nodes.into_iter().map(|i| unfold(point(i))).collect(), actions, reached })
}

/// A walking plan over every plane: what to walk, and the doors and climbs used on the way.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NavPlan {
    pub tiles: Vec<NavTile>,
    pub actions: Vec<NavAction>,
    pub reached: bool,
    // what it plans to: the goal, or a tile toward it when that is too far for one plan
    pub goal: NavTile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NavTile {
    pub x: i32,
    pub y: i32,
    pub plane: i32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NavAction {
    // the (1-based) index in `tiles` of the tile it is used from; the next tile is past it
    pub at: usize,
    // "door" or "climb"
    pub kind: &'static str,
    pub loc: Target,
    pub to_plane: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // `planes[p]` rows north first: '#' blocked, '|' east wall, '_' south wall, 'D'/'d' closed door east/south, 'L' ladder
    fn world(planes: &[&[&str]]) -> (Vec<Vec<u32>>, i32, i32) {
        let (w, h) = (planes[0][0].len() as i32, planes[0].len() as i32);
        let mut out = vec![vec![0u32; (w * h) as usize]; PLANES as usize];
        for (p, rows) in planes.iter().enumerate() {
            for (row, line) in rows.iter().enumerate() {
                let y = h - 1 - row as i32;
                for (x, ch) in line.chars().enumerate() {
                    let x = x as i32;
                    let mut set = |x: i32, y: i32, bits: u32| {
                        if (0..w).contains(&x) && (0..h).contains(&y) {
                            out[p][(x * h + y) as usize] |= bits;
                        }
                    };
                    match ch {
                        '#' | 'L' => set(x, y, flag::OBJECT),
                        '|' | 'D' => {
                            set(x, y, flag::WALL_EAST);
                            set(x + 1, y, flag::WALL_WEST);
                        }
                        '_' | 'd' => {
                            set(x, y, flag::WALL_SOUTH);
                            set(x, y - 1, flag::WALL_NORTH);
                        }
                        _ => {}
                    }
                }
            }
        }
        (out, w, h)
    }

    fn lookup(planes: &[Vec<u32>], w: i32, h: i32) -> impl Fn(i32, i32, i32) -> Option<u32> + '_ {
        move |x, y, plane| ((0..w).contains(&x) && (0..h).contains(&y)).then(|| planes.get(plane as usize).map(|p| p[(x * h + y) as usize])).flatten()
    }

    fn target(x: i32, y: i32, plane: i32, option: &str) -> Target {
        Target { id: 1, x, y, plane, kind: 0, size_x: 1, size_y: 1, option: option.into() }
    }

    // a straight door on the east edge of (x, y): rotation 2
    fn east_door(flags: &dyn Fn(i32, i32) -> Option<u32>, x: i32, y: i32, plane: i32) -> Door {
        let loc = target(x, y, plane, "Open");
        Door { blocks: door_blocks(flags, &loc, 0, 2), loc }
    }

    #[test]
    fn plans_through_a_closed_door_and_says_where() {
        let rows: &[&str] = &[
            "...|...", //
            "...D...",
            "...|...",
        ];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        assert_eq!(door.blocks, vec![(3, 1, flag::WALL_EAST), (4, 1, flag::WALL_WEST)]);
        let plan = plan(&flags, (0, 0, w - 1, h - 1), std::slice::from_ref(&door), &[], (0, 1, 0), (6, 1, 0), &Options::default()).unwrap();
        assert!(plan.reached);
        assert_eq!(plan.tiles.first(), Some(&(0, 1, 0)));
        assert_eq!(plan.tiles.last(), Some(&(6, 1, 0)));
        assert_eq!(plan.actions.len(), 1);
        let action = &plan.actions[0];
        assert_eq!(action.what, Use::Door(door.loc));
        assert_eq!((plan.tiles[action.at], plan.tiles[action.at + 1]), ((3, 1, 0), (4, 1, 0)));
        // without the door there is no way: the plan ends as close as it gets
        let stuck = super::plan(&flags, (0, 0, w - 1, h - 1), &[], &[], (0, 1, 0), (6, 1, 0), &Options::default()).unwrap();
        assert!(!stuck.reached);
        assert_eq!(stuck.tiles.last().map(|t| t.0), Some(3));
    }

    #[test]
    fn an_open_door_is_just_walked() {
        let rows: &[&str] = &["...|...", ".......", "...|..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        // the door's tile has no wall where a closed one would be
        assert!(door_blocks(&on0, &target(3, 1, 0, "Open"), 0, 2).is_empty());
        let plan = plan(&flags, (0, 0, w - 1, h - 1), &[], &[], (0, 1, 0), (6, 1, 0), &Options::default()).unwrap();
        assert!(plan.reached && plan.actions.is_empty());
        assert_eq!(plan.tiles.len(), 7);
    }

    #[test]
    fn prefers_a_door_to_a_long_way_round() {
        let rows: &[&str] = &[
            ".......", //
            "...|...",
            "...|...",
            "...D...",
            "...|...",
            "...|...",
        ];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 2, 0);
        let plan = plan(&flags, (0, 0, w - 1, h - 1), &[door], &[], (2, 2, 0), (4, 2, 0), &Options::default()).unwrap();
        assert_eq!(plan.actions.len(), 1);
        // a wide open way that is only a little longer is walked instead
        let rows: &[&str] = &[".......", ".......", "...D...", "......."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        let plan = super::plan(&flags, (0, 0, w - 1, h - 1), &[door], &[], (3, 1, 0), (4, 1, 0), &Options::default()).unwrap();
        assert!(plan.actions.is_empty(), "{plan:?}");
    }

    #[test]
    fn climbs_between_planes_through_doors() {
        // plane 0: a room with a door to the ladder; plane 1: the ladder's other end and the goal
        let ground: &[&str] = &[
            "...|...", //
            "...D.L.",
            "...|...",
        ];
        let upper: &[&str] = &[
            ".......", //
            ".....L.",
            ".......",
        ];
        let (planes, w, h) = world(&[ground, upper]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        let ladder = |plane: i32, dir: i32| Climbable {
            loc: Target { kind: 2, ..target(5, 1, plane, if dir > 0 { "Climb-up" } else { "Climb-down" }) },
            dir,
            solid: true,
        };
        let found = climbs(&flags, &[ladder(0, 1), ladder(1, -1)]);
        assert_eq!(found.len(), 2);
        let up = &found[0];
        assert_eq!(up.to_plane, 1);
        assert_eq!(up.reach, vec![(4, 1), (5, 0), (5, 2), (6, 1)]);
        assert_eq!(up.lands, up.reach);
        let plan = plan(&flags, (0, 0, w - 1, h - 1), std::slice::from_ref(&door), &found, (0, 1, 0), (1, 1, 1), &Options::default()).unwrap();
        assert!(plan.reached);
        let kinds: Vec<&Use> = plan.actions.iter().map(|a| &a.what).collect();
        assert_eq!(kinds, vec![&Use::Door(door.loc), &Use::Climb(ladder(0, 1).loc, 1)]);
        let climb = &plan.actions[1];
        assert_eq!(plan.tiles[climb.at].2, 0);
        assert_eq!(plan.tiles[climb.at + 1].2, 1);
        assert_eq!(plan.tiles.last(), Some(&(1, 1, 1)));
    }

    #[test]
    fn reach_stops_at_walls() {
        // a ladder against a wall on its west side: not used from the west
        let rows: &[&str] = &["...", ".|L", "..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        assert_eq!(reach(&on0, 2, 1, 1, 1, true), vec![(2, 0), (2, 2)]);
    }

    fn loc(id: i32, kind: u8, x: i32, y: i32, shape: Option<u8>, rotation: Option<u8>, clickable: bool) -> Loc {
        Loc { id, name: None, kind, x, y, plane: 0, scene_x: x, scene_y: y, size_x: 1, size_y: 1, shape, rotation, clickable }
    }

    fn def(interact_type: u8, actions: &[&str]) -> (LocDef, LocText) {
        let mut text = LocText::default();
        for (i, a) in actions.iter().enumerate() {
            text.actions[i] = a.to_string();
        }
        (LocDef { interact_type, ..LocDef::default() }, text)
    }

    #[test]
    fn finds_closed_doors_and_climbables() {
        let rows: &[&str] = &[
            ".....", //
            ".D.|.",
            "..L..",
        ];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let defs = [def(2, &["Open"]), def(2, &["Open"]), def(2, &["Climb-up", "Climb-down"]), def(2, &["Search"]), def(0, &["Open"])];
        let locs = [
            // a closed door (east edge of (1, 1)), one whose rotation isn't known, a ladder
            loc(0, WALL, 1, 1, Some(0), Some(2), true),
            loc(1, WALL, 3, 1, None, None, true),
            loc(2, GAME, 2, 0, Some(10), Some(0), true),
            // a wall with another option, a door that doesn't block, one that isn't clickable
            loc(3, WALL, 1, 1, Some(0), Some(2), true),
            loc(4, WALL, 1, 1, Some(0), Some(2), true),
            loc(0, WALL, 0, 2, Some(0), Some(2), false),
        ];
        let lookup_def = |id: i32| defs.get(id as usize);
        let (doors, climbables) = scene_locs(&flags, &locs, &lookup_def, &Request::default());
        assert_eq!(doors.iter().map(|d| d.loc.id).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(doors[1].blocks, vec![(3, 1, flag::WALL_EAST), (4, 1, flag::WALL_WEST)]);
        assert_eq!(climbables.iter().map(|c| (c.loc.option.as_str(), c.dir)).collect::<Vec<_>>(), vec![("Climb-up", 1), ("Climb-down", -1)]);
        // avoided locs are left out
        let request = Request { avoid: vec![(0, 1, 1, 0)], ..Request::default() };
        let (doors, _) = scene_locs(&flags, &locs, &lookup_def, &request);
        assert_eq!(doors.len(), 1);
    }

    #[test]
    fn guesses_a_doors_rotation_from_the_walls() {
        let rows: &[&str] = &["...", ".D.", "..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        assert_eq!(guess_wall(&on0, 1, 1), Some((0, 2)));
        assert_eq!(guess_wall(&on0, 0, 0), None);
    }

    #[test]
    fn ends_near_the_goal_and_seeds_change_the_way() {
        let rows: &[&str] = &[".........", ".........", ".........", "........."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let options = Options { near: 2, ..Options::default() };
        let plan = plan(&flags, (0, 0, w - 1, h - 1), &[], &[], (0, 0, 0), (8, 3, 0), &options).unwrap();
        assert!(plan.reached);
        assert_eq!(plan.tiles.last().map(|t| t.0), Some(6));
        let a = super::plan(&flags, (0, 0, w - 1, h - 1), &[], &[], (0, 0, 0), (8, 3, 0), &Options { seed: Some(1), ..Options::default() }).unwrap();
        let b = super::plan(&flags, (0, 0, w - 1, h - 1), &[], &[], (0, 0, 0), (8, 3, 0), &Options { seed: Some(1), ..Options::default() }).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.tiles.last(), Some(&(8, 3, 0)));
    }

    // the surface and the underground (rows north first, as `world`), as one map: the underground is `UNDERGROUND` tiles north
    fn two_worlds<'a>(surface: &'a [Vec<u32>], under: &'a [Vec<u32>], w: i32, h: i32) -> impl Fn(i32, i32, i32) -> Option<u32> + 'a {
        move |x, y, plane| if y >= UNDERGROUND { lookup(under, w, h)(x, y - UNDERGROUND, plane) } else { lookup(surface, w, h)(x, y, plane) }
    }

    fn climbable(x: i32, y: i32, plane: i32, option: &str, dir: i32) -> Climbable {
        Climbable { loc: target(x, y, plane, option), dir, solid: true }
    }

    #[test]
    fn plans_down_a_ladder_into_the_underground() {
        let surface: &[&str] = &[".....", "...L.", "....."];
        let (surface, w, h) = world(&[surface]);
        let (under, _, _) = world(&[&[".....", "...L.", "....."]]);
        let flags = two_worlds(&surface, &under, w, h);
        let down = climbable(3, 1, 0, "Climb-down", -1);
        let up = climbable(3, 1 + UNDERGROUND, 0, "Climb-up", 1);
        // both ends are sources, the one in the other world as far from the player as the cache is
        let found = climbs(&flags, &[down.clone(), up]);
        assert_eq!(found.len(), 2);
        let into = found.iter().find(|c| c.loc == down.loc).unwrap();
        assert_eq!(into.to_plane, 0);
        assert!(into.lands.iter().all(|&(_, y)| y >= UNDERGROUND));
        let plan = plan(&flags, (0, 0, w - 1, h - 1), &[], &found, (0, 1, 0), (1, 1 + UNDERGROUND, 0), &Options::default()).unwrap();
        assert!(plan.reached);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].what, Use::Climb(down.loc, 0));
        assert_eq!(plan.tiles[plan.actions[0].at].1, 1);
        assert!(plan.tiles[plan.actions[0].at + 1].1 >= UNDERGROUND);
        assert_eq!(plan.tiles.last(), Some(&(1, 1 + UNDERGROUND, 0)));
        // and back out
        let back = super::plan(&flags, (0, 0, w - 1, h - 1), &[], &found, (1, 1 + UNDERGROUND, 0), (0, 1, 0), &Options::default()).unwrap();
        assert!(back.reached);
        assert_eq!(back.tiles.last(), Some(&(0, 1, 0)));
    }

    #[test]
    fn goes_from_one_dungeon_to_another_through_the_surface() {
        // two dungeons that only meet above ground: the west one climbs out at the west ladder, the east one is entered by the east trapdoor
        let surface: &[&str] = &["...........", "..L.....L..", "..........."];
        let (surface, w, h) = world(&[surface]);
        let (under, _, _) = world(&[&["....#......", "..L.#...L..", "....#......"]]);
        let flags = two_worlds(&surface, &under, w, h);
        let sources = [
            climbable(2, 1, 0, "Climb-down", -1),
            climbable(2, 1 + UNDERGROUND, 0, "Climb-up", 1),
            Climbable { loc: Target { id: 9, ..target(8, 1, 0, "Open") }, dir: 0, solid: true },
            climbable(8, 1 + UNDERGROUND, 0, "Climb-up", 1),
        ];
        let found = climbs(&flags, &sources);
        let plan = plan(&flags, (0, 0, w - 1, h - 1), &[], &found, (3, 1 + UNDERGROUND, 0), (7, 1 + UNDERGROUND, 0), &Options::default()).unwrap();
        assert!(plan.reached);
        let used: Vec<&str> = plan.actions.iter().map(|a| match &a.what {
            Use::Climb(loc, _) => loc.option.as_str(),
            Use::Door(_) => "door",
        }).collect();
        assert_eq!(used, vec!["Climb-up", "Open"]);
    }

    #[test]
    fn a_way_up_tries_the_plane_above_before_the_surface() {
        let (surface, w, h) = world(&[&[".....", "...L.", "....."]]);
        let (under, _, _) = world(&[&[".....", "...L.", "....."], &[".....", "...L.", "....."]]);
        let flags = two_worlds(&surface, &under, w, h);
        let up = climbable(3, 1 + UNDERGROUND, 0, "Climb-up", 1);
        let above = climbable(3, 1 + UNDERGROUND, 1, "Climb-down", -1);
        let down = climbable(3, 1, 0, "Climb-down", -1);
        let of = |found: &[Climb], loc: &Target| found.iter().find(|c| &c.loc == loc).cloned();
        // a ladder on the plane above is where it leads, whatever is on the surface
        let found = climbs(&flags, &[up.clone(), above, down.clone()]);
        let to = of(&found, &up.loc).unwrap();
        assert_eq!(to.to_plane, 1);
        assert!(to.lands.iter().all(|&(_, y)| y >= UNDERGROUND));
        // without one it leads out, to the other end of the ladder
        let found = climbs(&flags, &[up.clone(), down]);
        let to = of(&found, &up.loc).unwrap();
        assert_eq!(to.to_plane, 0);
        assert!(to.lands.iter().all(|&(_, y)| y < UNDERGROUND));
    }

    #[test]
    fn portals_need_the_other_end() {
        let (surface, w, h) = world(&[&[".....", "...L.", "....."]]);
        let (under, _, _) = world(&[&[".....", "...L.", "....."]]);
        let flags = two_worlds(&surface, &under, w, h);
        let enter = climbable(3, 1, 0, "Enter", 0);
        // no loc at the other end: nothing to climb, and no guessing
        assert!(climbs(&flags, std::slice::from_ref(&enter)).is_empty());
        // the other end may be anywhere near, with any id, and has to lead out
        let exit = Climbable { loc: Target { id: 99, ..target(4, 2 + UNDERGROUND, 0, "Exit") }, dir: 0, solid: false };
        let found = climbs(&flags, &[enter.clone(), exit]);
        let into = found.iter().find(|c| c.loc == enter.loc).unwrap();
        assert!(into.lands.contains(&(4, 2 + UNDERGROUND)));
        // a way down at the other end doesn't lead back
        let wrong = climbable(3, 1 + UNDERGROUND, 0, "Climb-down", -1);
        assert!(!climbs(&flags, &[enter.clone(), wrong]).iter().any(|c| c.loc == enter.loc));
    }

    #[test]
    fn a_closed_trapdoor_is_a_portal_but_other_things_that_open_are_not() {
        let rows: &[&str] = &["...", "...", "..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let mut trapdoor = def(2, &["Open"]);
        trapdoor.1.name = "Trapdoor".into();
        let mut chest = def(2, &["Open"]);
        chest.1.name = "Chest".into();
        let defs = [trapdoor, chest];
        // game objects of shape 10: not a door's shape
        let locs = [loc(0, GAME, 1, 1, Some(10), Some(0), true), loc(1, GAME, 2, 2, Some(10), Some(0), true)];
        let lookup_def = |id: i32| defs.get(id as usize);
        let (doors, climbables) = scene_locs(&flags, &locs, &lookup_def, &Request::default());
        assert!(doors.is_empty());
        assert_eq!(climbables.iter().map(|c| (c.loc.id, c.loc.option.as_str(), c.dir)).collect::<Vec<_>>(), vec![(0, "Open", 0)]);
    }

    #[test]
    fn steps_that_only_say_climb_lead_to_the_planes_beside() {
        let (planes, w, h) = world(&[&[".....", "..L..", "....."], &[".....", "..L..", "....."], &[".....", "..L..", "....."]]);
        let flags = lookup(&planes, w, h);
        let steps = |plane: i32| climbable(2, 1, plane, "Climb", 0);
        // the middle one leads both ways, the ends one
        let found = climbs(&flags, &[steps(0), steps(1), steps(2)]);
        let leads = |plane: i32| found.iter().filter(|c| c.loc.plane == plane).map(|c| c.to_plane).collect::<Vec<_>>();
        assert_eq!(leads(0), vec![1]);
        assert_eq!(leads(1), vec![0, 2]);
        assert_eq!(leads(2), vec![1]);
        // one with nothing beside it leads nowhere
        assert!(climbs(&flags, &[steps(0)]).is_empty());
    }

    #[test]
    fn a_portal_leaves_the_other_end_of_a_ladder_to_the_ladder() {
        let (surface, w, h) = world(&[&[".......", "..L....", "......."]]);
        let (under, _, _) = world(&[&[".......", "..L.L..", "......."]]);
        let flags = two_worlds(&surface, &under, w, h);
        let down = climbable(2, 1, 0, "Climb-down", -1);
        let up = climbable(2, 1 + UNDERGROUND, 0, "Climb-up", 1);
        // a pipe a few tiles from the ladder, which has the same surface end within reach
        let pipe = climbable(4, 1 + UNDERGROUND, 0, "Squeeze-through", 0);
        let found = climbs(&flags, &[down, up, pipe.clone()]);
        assert!(!found.iter().any(|c| c.loc == pipe.loc));
        assert_eq!(found.len(), 2);
    }
}
