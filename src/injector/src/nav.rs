use std::{
    cell::Cell,
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
};

use crate::{
    Loc,
    collision::{self, LocDef, LocText, flag, turned, wall_marks},
    pathfinding::{DIRECTIONS, step_cost, step_open},
    projection::{GAME_OBJECT, WALL},
    scene_graph::mix,
};

pub type Point = (i32, i32, i32);

pub(crate) fn fold((x, y, plane): Point) -> Point {
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

pub const PLANES: i32 = collision::PLANES as i32;
pub const UNDERGROUND: i32 = 6400;
const WORLDS: i32 = 2;

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
    pub blocks: Vec<(i32, i32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climb {
    pub loc: Target,
    pub to_plane: i32,
    pub reach: (i32, i32),
    pub land: (i32, i32),
    pub shortcut_ticks: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Climbable {
    pub loc: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClimbRow {
    pub origin: Point,
    pub destination: Point,
    pub option: String,
    pub id: i32,
    pub shortcut_ticks: Option<u32>,
    pub portal: bool,
}

pub type Flags<'a> = &'a dyn Fn(i32, i32, i32) -> Option<u32>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub near: i32,
    pub door_cost: u32,
    pub climb_cost: u32,
    pub shortcut_saving: u32,
    pub seed: Option<u64>,
    pub rnd: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            near: 0,
            door_cost: 8,
            climb_cost: 50,
            shortcut_saving: 100,
            seed: None,
            rnd: 0.6,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub options: Options,
    pub door_options: Vec<String>,
    pub climbs: bool,
    pub allow_climbs: Vec<String>,
    pub climb_options: Vec<String>,
    pub portal_names: Vec<String>,
    pub avoid: Vec<(i32, i32, i32, i32)>,
}

impl Default for Request {
    fn default() -> Self {
        Request {
            options: Options::default(),
            door_options: vec!["Open".into()],
            climbs: true,
            allow_climbs: Vec::new(),
            climb_options: Vec::new(),
            portal_names: ["trapdoor", "manhole"].map(String::from).to_vec(),
            avoid: Vec::new(),
        }
    }
}

pub fn scene_locs<'a>(
    flags: Flags,
    locs: &[Loc],
    defs: &dyn Fn(i32) -> Option<&'a (LocDef, LocText)>,
    request: &Request,
) -> (Vec<Door>, Vec<Climbable>) {
    let (mut doors, mut climbables) = (Vec::new(), Vec::new());
    for loc in locs
        .iter()
        .filter(|l| l.clickable && !request.avoid.contains(&(l.id, l.x, l.y, l.plane)))
    {
        let Some((def, text)) = defs(loc.id) else {
            continue;
        };
        let offered = |wanted: &str| {
            text.actions
                .iter()
                .find(|a| a.eq_ignore_ascii_case(wanted))
                .cloned()
        };
        let target = |option: String| Target {
            id: loc.id,
            x: loc.x,
            y: loc.y,
            plane: loc.plane,
            kind: loc.kind,
            size_x: loc.size_x,
            size_y: loc.size_y,
            option,
        };
        let on = |x: i32, y: i32| flags(x, y, loc.plane);
        let door = request.door_options.iter().find_map(|o| offered(o));
        if let Some(option) = door.filter(|_| def.interact_type != 0) {
            let shape = match (loc.kind, loc.shape, loc.rotation) {
                (WALL, Some(shape @ (0 | 2)), Some(rotation)) => Some((shape, rotation)),
                (WALL, None, _) => guess_wall(&on, loc.x, loc.y),
                (GAME_OBJECT, Some(9), Some(rotation)) => Some((9, rotation)),
                _ => None,
            };
            let loc = target(option);
            let blocks = shape.map_or_else(Vec::new, |(shape, rotation)| {
                door_blocks(&on, &loc, shape, rotation)
            });
            if blocks.is_empty() {
                climbables.push(Climbable { loc });
            } else {
                doors.push(Door { loc, blocks });
            }
            continue;
        }
        climbables.extend(
            request
                .climb_options
                .iter()
                .filter_map(|option| offered(option))
                .map(|option| Climbable {
                    loc: target(option),
                }),
        );
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
    pub at: usize,
    pub what: Use,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub tiles: Vec<Point>,
    pub actions: Vec<Action>,
    pub reached: bool,
}

fn footprint_distance((x, y): (i32, i32), loc: &Target) -> i32 {
    let gap = |t: i32, start: i32, size: u8| (start - t).max(t - (start + size as i32 - 1)).max(0);
    gap(x, loc.x, loc.size_x).max(gap(y, loc.y, loc.size_y))
}

const ROW_REACH: i32 = 2;

pub fn data_climbs(
    flags: Flags,
    locs: &[Climbable],
    rows: &[ClimbRow],
    door_options: &[String],
) -> Vec<Climb> {
    let mut by_plane: HashMap<i32, Vec<&Climbable>> = HashMap::new();
    for loc in locs {
        by_plane.entry(loc.loc.plane).or_default().push(loc);
    }
    let mut out = Vec::new();
    for row in rows {
        let (x, y, plane) = row.origin;
        let Some(candidates) = by_plane.get(&plane) else {
            continue;
        };
        let span = if row.destination.2 == plane {
            (row.destination.0 - x)
                .abs()
                .max((row.destination.1 - y).abs())
        } else {
            0
        };
        let Some(found) = candidates
            .iter()
            .filter(|c| {
                c.loc.option.eq_ignore_ascii_case(&row.option)
                    || (row.portal
                        && door_options
                            .iter()
                            .any(|d| d.eq_ignore_ascii_case(&c.loc.option)))
            })
            .map(|c| (footprint_distance((x, y), &c.loc), *c))
            .filter(|(d, c)| *d <= ROW_REACH || (c.loc.id == row.id && *d <= span))
            .min_by_key(|(d, c)| (c.loc.id != row.id, *d, c.loc.x, c.loc.y))
            .map(|(_, c)| c)
        else {
            continue;
        };
        if !flags(x, y, plane).is_some_and(|f| f & flag::BLOCKED == 0) {
            continue;
        }
        out.push(Climb {
            loc: found.loc.clone(),
            to_plane: row.destination.2,
            reach: (x, y),
            land: (row.destination.0, row.destination.1),
            shortcut_ticks: row.shortcut_ticks,
        });
    }
    out
}

pub fn door_blocks(
    flags: &dyn Fn(i32, i32) -> Option<u32>,
    loc: &Target,
    shape: u8,
    rotation: u8,
) -> Vec<(i32, i32, u32)> {
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
            let (w, h) = turned(loc.size_x, loc.size_y, rotation);
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

fn guess_wall(flags: &dyn Fn(i32, i32) -> Option<u32>, x: i32, y: i32) -> Option<(u8, u8)> {
    let mut found = None;
    for rotation in 0..4u8 {
        if wall_marks(0, rotation)
            .iter()
            .all(|&(dx, dy, wall)| flags(x + dx, y + dy).unwrap_or(0) & wall != 0)
        {
            if found.is_some() {
                return None;
            }
            found = Some((0, rotation));
        }
    }
    found
}

const WALK: u32 = 0;
const DOOR: u32 = 1 << 30;
const CLIMB: u32 = 2 << 30;
const INDEX: u32 = (1 << 30) - 1;

const SHORTCUT_TICK_COST: u32 = 4;

fn shortcut_extra(climb: &Climb, options: &Options) -> u32 {
    climb.shortcut_ticks.map_or(0, |ticks| {
        options.shortcut_saving + ticks * SHORTCUT_TICK_COST
    })
}

pub type Closed = HashSet<(Point, (i32, i32))>;

pub fn step_closed(closed: &Closed, (x, y, plane): Point, dx: i32, dy: i32) -> bool {
    let shut = |x: i32, y: i32, dx: i32, dy: i32| closed.contains(&((x, y, plane), (dx, dy)));
    !closed.is_empty()
        && if dx != 0 && dy != 0 {
            shut(x, y, dx, 0)
                || shut(x, y, 0, dy)
                || shut(x + dx, y, 0, dy)
                || shut(x, y + dy, dx, 0)
        } else {
            shut(x, y, dx, dy)
        }
}

#[allow(clippy::too_many_arguments)]
pub fn plan(
    flags: Flags,
    bounds: (i32, i32, i32, i32),
    doors: &[Door],
    climbs: &[Climb],
    from: Point,
    to: Point,
    options: &Options,
    closed: &Closed,
) -> Option<Plan> {
    let (from, to) = (fold(from), fold(to));
    let (x1, y1, x2, y2) = bounds;
    let (w, h) = (x2 - x1 + 1, y2 - y1 + 1);
    if w <= 0 || h <= 0 {
        return None;
    }
    let inside = |x: i32, y: i32, plane: i32| {
        (x1..=x2).contains(&x) && (y1..=y2).contains(&y) && (0..PLANES * WORLDS).contains(&plane)
    };
    let closed: Closed = closed
        .iter()
        .map(|&(tile, step)| (fold(tile), step))
        .filter(|&((x, y, plane), _)| inside(x, y, plane))
        .collect();
    let index = |x: i32, y: i32, plane: i32| ((plane * w + (x - x1)) * h + (y - y1)) as usize;
    let index_in = |(x, y, plane): Point| inside(x, y, plane).then(|| index(x, y, plane));
    let point = |i: usize| {
        let i = i as i32;
        let (plane, rest) = (i / (w * h), i % (w * h));
        (x1 + rest / h, y1 + rest % h, plane)
    };
    if !inside(from.0, from.1, from.2) {
        return None;
    }
    let size = (PLANES * WORLDS * w * h) as usize;

    let mut opened = vec![0u32; size];
    let mut door_at = vec![0u32; size];
    for (n, door) in doors.iter().enumerate() {
        for &(x, y, bits) in &door.blocks {
            if let Some(i) = index_in(fold((x, y, door.loc.plane))) {
                opened[i] |= bits;
                door_at[i] = n as u32 + 1;
            }
        }
    }
    let mut climb_at = vec![0u32; size];
    let mut climb_lists: Vec<Vec<u32>> = Vec::new();
    for (n, climb) in climbs.iter().enumerate() {
        if let Some(i) = index_in(fold((climb.reach.0, climb.reach.1, climb.loc.plane))) {
            if climb_at[i] == 0 {
                climb_lists.push(Vec::new());
                climb_at[i] = climb_lists.len() as u32;
            }
            climb_lists[climb_at[i] as usize - 1].push(n as u32);
        }
    }
    let read = vec![Cell::new(None); size];
    let original = |x: i32, y: i32, plane: i32| {
        if !inside(x, y, plane) {
            return None;
        }
        let kept = &read[index(x, y, plane)];
        kept.get().unwrap_or_else(|| {
            let (rx, ry, rplane) = unfold((x, y, plane));
            let found = flags(rx, ry, rplane);
            kept.set(Some(found));
            found
        })
    };
    let open =
        |x: i32, y: i32, plane: i32| original(x, y, plane).map(|f| f & !opened[index(x, y, plane)]);
    let noise = |i: usize, cost: u32| match options.seed {
        Some(seed) => {
            let unit = (mix(seed ^ mix(i as u64)) >> 11) as f64 / (1u64 << 53) as f64;
            (cost as f64 * (1.0 + options.rnd * (unit - 0.5)))
                .round()
                .max(1.0) as u32
        }
        None => cost,
    };
    let chebyshev = |(x, y, _): Point| (x - to.0).abs().max((y - to.1).abs());

    let mut cost = vec![u32::MAX; size];
    let mut back = vec![0u32; size];
    let mut via = vec![WALK; size];
    let start = index(from.0, from.1, from.2);
    cost[start] = 0;
    let mut heap = BinaryHeap::from([(Reverse(0u32), start)]);
    let mut end: Option<usize> = None;
    let mut closest: Option<(i32, u32, usize)> = None;
    while let Some((Reverse(c), at)) = heap.pop() {
        if c > cost[at] {
            continue;
        }
        let here = point(at);
        if here.2 == to.2 {
            let d = chebyshev(here);
            if d <= options.near {
                end = Some(at);
                break;
            }
            if closest.is_none_or(|(best, best_cost, _)| (d, c) < (best, best_cost)) {
                closest = Some((d, c, at));
            }
        }
        let (x, y, plane) = here;
        let mut relax =
            |next: usize, step: u32, how: u32, heap: &mut BinaryHeap<(Reverse<u32>, usize)>| {
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
            if !inside(nx, ny, plane) || step_closed(&closed, here, dx, dy) {
                continue;
            }
            let step = step_cost(dx, dy);
            let next = index(nx, ny, plane);
            if step_open(&walked, x, y, dx, dy) {
                relax(next, step, WALK, &mut heap);
            } else if step_open(&through, x, y, dx, dy)
                && let Some(door) = [(nx, ny), (nx, y), (x, ny), (x, y)]
                    .iter()
                    .map(|&(tx, ty)| door_at[index(tx, ty, plane)])
                    .find(|&d| d != 0)
            {
                relax(next, step + options.door_cost, DOOR | (door - 1), &mut heap);
            }
        }
        if climb_at[at] != 0 {
            for &n in &climb_lists[climb_at[at] as usize - 1] {
                let climb = &climbs[n as usize];
                if let Some(next) = index_in(fold((climb.land.0, climb.land.1, climb.to_plane))) {
                    relax(
                        next,
                        options.climb_cost + shortcut_extra(climb, options),
                        CLIMB | n,
                        &mut heap,
                    );
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
    Some(Plan {
        tiles: nodes.into_iter().map(|i| unfold(point(i))).collect(),
        actions,
        reached,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NavPlan {
    pub tiles: Vec<NavTile>,
    pub actions: Vec<NavAction>,
    pub reached: bool,
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
    pub at: usize,
    pub kind: &'static str,
    pub loc: Target,
    pub to_plane: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        move |x, y, plane| {
            ((0..w).contains(&x) && (0..h).contains(&y))
                .then(|| planes.get(plane as usize).map(|p| p[(x * h + y) as usize]))
                .flatten()
        }
    }

    fn target(x: i32, y: i32, plane: i32, option: &str) -> Target {
        Target {
            id: 1,
            x,
            y,
            plane,
            kind: 0,
            size_x: 1,
            size_y: 1,
            option: option.into(),
        }
    }

    fn east_door(flags: &dyn Fn(i32, i32) -> Option<u32>, x: i32, y: i32, plane: i32) -> Door {
        let loc = target(x, y, plane, "Open");
        Door {
            blocks: door_blocks(flags, &loc, 0, 2),
            loc,
        }
    }

    #[test]
    fn plans_through_a_closed_door_and_says_where() {
        let rows: &[&str] = &["...|...", "...D...", "...|..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        assert_eq!(
            door.blocks,
            vec![(3, 1, flag::WALL_EAST), (4, 1, flag::WALL_WEST)]
        );
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            std::slice::from_ref(&door),
            &[],
            (0, 1, 0),
            (6, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        assert_eq!(plan.tiles.first(), Some(&(0, 1, 0)));
        assert_eq!(plan.tiles.last(), Some(&(6, 1, 0)));
        assert_eq!(plan.actions.len(), 1);
        let action = &plan.actions[0];
        assert_eq!(action.what, Use::Door(door.loc));
        assert_eq!(
            (plan.tiles[action.at], plan.tiles[action.at + 1]),
            ((3, 1, 0), (4, 1, 0))
        );
        let stuck = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &[],
            (0, 1, 0),
            (6, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(!stuck.reached);
        assert_eq!(stuck.tiles.last().map(|t| t.0), Some(3));
    }

    #[test]
    fn an_open_door_is_just_walked() {
        let rows: &[&str] = &["...|...", ".......", "...|..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        assert!(door_blocks(&on0, &target(3, 1, 0, "Open"), 0, 2).is_empty());
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &[],
            (0, 1, 0),
            (6, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached && plan.actions.is_empty());
        assert_eq!(plan.tiles.len(), 7);
    }

    #[test]
    fn prefers_a_door_to_a_long_way_round() {
        let rows: &[&str] = &[
            ".......", "...|...", "...|...", "...D...", "...|...", "...|...",
        ];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 2, 0);
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[door],
            &[],
            (2, 2, 0),
            (4, 2, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 1);
        let rows: &[&str] = &[".......", ".......", "...D...", "......."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        let plan = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            std::slice::from_ref(&door),
            &[],
            (3, 1, 0),
            (4, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert_eq!(
            plan.actions.len(),
            1,
            "a cheap door beats a step round: {plan:?}"
        );
        let costly = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[door],
            &[],
            (3, 1, 0),
            (4, 1, 0),
            &Options {
                door_cost: 30,
                ..Options::default()
            },
            &Closed::default(),
        )
        .unwrap();
        assert!(
            costly.actions.is_empty(),
            "a dear door loses to a step round: {costly:?}"
        );
    }

    #[test]
    fn a_step_can_be_closed_one_way_and_diagonally() {
        let mut closed = Closed::default();
        closed.insert(((5, 5, 0), (1, 0)));
        assert!(step_closed(&closed, (5, 5, 0), 1, 0));
        assert!(
            !step_closed(&closed, (6, 5, 0), -1, 0),
            "the way back is free"
        );
        assert!(
            step_closed(&closed, (5, 5, 0), 1, 1),
            "past it, from its tile"
        );
        assert!(
            step_closed(&closed, (5, 4, 0), 1, 1),
            "and into the row beyond"
        );
        assert!(!step_closed(&closed, (5, 5, 1), 1, 0), "another plane");
    }

    #[test]
    fn climbs_between_planes_through_doors() {
        let ground: &[&str] = &["...|...", "...D.L.", "...|..."];
        let upper: &[&str] = &[".......", ".....L.", "......."];
        let (planes, w, h) = world(&[ground, upper]);
        let flags = lookup(&planes, w, h);
        let on0 = |x: i32, y: i32| flags(x, y, 0);
        let door = east_door(&on0, 3, 1, 0);
        let ladder = |plane: i32, option: &str| Climbable {
            loc: Target {
                kind: 2,
                ..target(5, 1, plane, option)
            },
        };
        let rows = [
            row((4, 1, 0), (4, 1, 1), "Climb-up", 1),
            row((6, 1, 0), (6, 1, 1), "Climb-up", 1),
            row((4, 1, 1), (4, 1, 0), "Climb-down", 1),
        ];
        let found = data_climbs(
            &flags,
            &[ladder(0, "Climb-up"), ladder(1, "Climb-down")],
            &rows,
        );
        assert_eq!(found.len(), 3);
        let up = &found[0];
        assert_eq!(up.to_plane, 1);
        assert_eq!(up.reach, (4, 1));
        assert_eq!(up.land, (4, 1));
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            std::slice::from_ref(&door),
            &found,
            (0, 1, 0),
            (1, 1, 1),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        let kinds: Vec<&Use> = plan.actions.iter().map(|a| &a.what).collect();
        assert_eq!(
            kinds,
            vec![
                &Use::Door(door.loc),
                &Use::Climb(ladder(0, "Climb-up").loc, 1)
            ]
        );
        let climb = &plan.actions[1];
        assert_eq!(plan.tiles[climb.at].2, 0);
        assert_eq!(plan.tiles[climb.at + 1].2, 1);
        assert_eq!(plan.tiles.last(), Some(&(1, 1, 1)));
    }

    fn loc(
        id: i32,
        kind: u8,
        x: i32,
        y: i32,
        shape: Option<u8>,
        rotation: Option<u8>,
        clickable: bool,
    ) -> Loc {
        Loc {
            id,
            name: None,
            kind,
            x,
            y,
            plane: 0,
            scene_x: x,
            scene_y: y,
            size_x: 1,
            size_y: 1,
            shape,
            rotation,
            clickable,
        }
    }

    fn def(interact_type: u8, actions: &[&str]) -> (LocDef, LocText) {
        let mut text = LocText::default();
        for (i, a) in actions.iter().enumerate() {
            text.actions[i] = a.to_string();
        }
        (
            LocDef {
                interact_type,
                ..LocDef::default()
            },
            text,
        )
    }

    #[test]
    fn finds_closed_doors_and_climbables() {
        let rows: &[&str] = &[".....", ".D.|.", "..L.."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let defs = [
            def(2, &["Open"]),
            def(2, &["Open"]),
            def(2, &["Climb-up", "Climb-down"]),
            def(2, &["Search"]),
            def(0, &["Open"]),
        ];
        let locs = [
            loc(0, WALL, 1, 1, Some(0), Some(2), true),
            loc(1, WALL, 3, 1, None, None, true),
            loc(2, GAME_OBJECT, 2, 0, Some(10), Some(0), true),
            loc(3, WALL, 1, 1, Some(0), Some(2), true),
            loc(4, WALL, 1, 1, Some(0), Some(2), true),
            loc(0, WALL, 0, 2, Some(0), Some(2), false),
        ];
        let lookup_def = |id: i32| defs.get(id as usize);
        let request = Request {
            climb_options: vec!["Climb-up".into(), "Climb-down".into()],
            ..Request::default()
        };
        let (doors, climbables) = scene_locs(&flags, &locs, &lookup_def, &request);
        assert_eq!(
            doors.iter().map(|d| d.loc.id).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            doors[1].blocks,
            vec![(3, 1, flag::WALL_EAST), (4, 1, flag::WALL_WEST)]
        );
        assert_eq!(
            climbables
                .iter()
                .map(|c| c.loc.option.as_str())
                .collect::<Vec<_>>(),
            vec!["Climb-up", "Climb-down"]
        );
        let request = Request {
            avoid: vec![(0, 1, 1, 0)],
            ..request
        };
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
        let options = Options {
            near: 2,
            ..Options::default()
        };
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &[],
            (0, 0, 0),
            (8, 3, 0),
            &options,
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        assert_eq!(plan.tiles.last().map(|t| t.0), Some(6));
        let a = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &[],
            (0, 0, 0),
            (8, 3, 0),
            &Options {
                seed: Some(1),
                ..Options::default()
            },
            &Closed::default(),
        )
        .unwrap();
        let b = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &[],
            (0, 0, 0),
            (8, 3, 0),
            &Options {
                seed: Some(1),
                ..Options::default()
            },
            &Closed::default(),
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.tiles.last(), Some(&(8, 3, 0)));
    }

    fn two_worlds<'a>(
        surface: &'a [Vec<u32>],
        under: &'a [Vec<u32>],
        w: i32,
        h: i32,
    ) -> impl Fn(i32, i32, i32) -> Option<u32> + 'a {
        move |x, y, plane| {
            if y >= UNDERGROUND {
                lookup(under, w, h)(x, y - UNDERGROUND, plane)
            } else {
                lookup(surface, w, h)(x, y, plane)
            }
        }
    }

    fn data_climbs(flags: Flags, locs: &[Climbable], rows: &[ClimbRow]) -> Vec<Climb> {
        super::data_climbs(flags, locs, rows, &[])
    }

    fn climbable(x: i32, y: i32, plane: i32, option: &str) -> Climbable {
        Climbable {
            loc: target(x, y, plane, option),
        }
    }

    fn with_id(id: i32, x: i32, y: i32, plane: i32, option: &str) -> Climbable {
        Climbable {
            loc: Target {
                id,
                ..target(x, y, plane, option)
            },
        }
    }

    fn row(origin: Point, destination: Point, option: &str, id: i32) -> ClimbRow {
        ClimbRow {
            origin,
            destination,
            option: option.into(),
            id,
            shortcut_ticks: None,
            portal: false,
        }
    }

    #[test]
    fn plans_down_a_ladder_into_the_underground() {
        let surface: &[&str] = &[".....", "...L.", "....."];
        let (surface, w, h) = world(&[surface]);
        let (under, _, _) = world(&[&[".....", "...L.", "....."]]);
        let flags = two_worlds(&surface, &under, w, h);
        let down = climbable(3, 1, 0, "Climb-down");
        let up = climbable(3, 1 + UNDERGROUND, 0, "Climb-up");
        let rows = [
            row((2, 1, 0), (2, 1 + UNDERGROUND, 0), "Climb-down", 1),
            row((2, 1 + UNDERGROUND, 0), (2, 1, 0), "Climb-up", 1),
        ];
        let found = data_climbs(&flags, &[down.clone(), up], &rows);
        assert_eq!(found.len(), 2);
        let into = found.iter().find(|c| c.loc == down.loc).unwrap();
        assert_eq!(into.to_plane, 0);
        assert_eq!(into.land, (2, 1 + UNDERGROUND));
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (0, 1, 0),
            (1, 1 + UNDERGROUND, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].what, Use::Climb(down.loc, 0));
        assert_eq!(plan.tiles[plan.actions[0].at].1, 1);
        assert!(plan.tiles[plan.actions[0].at + 1].1 >= UNDERGROUND);
        assert_eq!(plan.tiles.last(), Some(&(1, 1 + UNDERGROUND, 0)));
        let back = super::plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (1, 1 + UNDERGROUND, 0),
            (0, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(back.reached);
        assert_eq!(back.tiles.last(), Some(&(0, 1, 0)));
    }

    #[test]
    fn goes_from_one_dungeon_to_another_through_the_surface() {
        let surface: &[&str] = &["...........", "..L.....L..", "..........."];
        let (surface, w, h) = world(&[surface]);
        let (under, _, _) = world(&[&["....#......", "..L.#...L..", "....#......"]]);
        let flags = two_worlds(&surface, &under, w, h);
        let sources = [
            climbable(2, 1, 0, "Climb-down"),
            climbable(2, 1 + UNDERGROUND, 0, "Climb-up"),
            with_id(9, 8, 1, 0, "Open"),
            climbable(8, 1 + UNDERGROUND, 0, "Climb-up"),
        ];
        let found = data_climbs(
            &flags,
            &sources,
            &[
                row((3, 1 + UNDERGROUND, 0), (3, 1, 0), "Climb-up", 1),
                row((7, 1, 0), (7, 1 + UNDERGROUND, 0), "Open", 9),
                row((7, 1 + UNDERGROUND, 0), (7, 1, 0), "Climb-up", 1),
            ],
        );
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (3, 1 + UNDERGROUND, 0),
            (7, 1 + UNDERGROUND, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        let used: Vec<&str> = plan
            .actions
            .iter()
            .map(|a| match &a.what {
                Use::Climb(loc, _) => loc.option.as_str(),
                Use::Door(_) => "door",
            })
            .collect();
        assert_eq!(used, vec!["Climb-up", "Open"]);
    }

    #[test]
    fn a_rope_can_climb_two_planes() {
        let (planes, w, h) = world(&[
            &[".....", "..L..", "....."],
            &[".....", "..L..", "....."],
            &[".....", "..L..", "....."],
        ]);
        let flags = lookup(&planes, w, h);
        let ropes = [
            climbable(2, 1, 0, "Climb-up"),
            climbable(2, 1, 1, "Climb-up"),
            climbable(2, 1, 2, "Climb-down"),
        ];
        let rows = [
            row((2, 0, 0), (0, 1, 2), "Climb-up", 5),
            row((0, 1, 2), (2, 0, 0), "Climb-down", 6),
        ];
        let found = data_climbs(&flags, &ropes, &rows);
        assert_eq!(
            found
                .iter()
                .map(|c| (c.loc.plane, c.to_plane))
                .collect::<Vec<_>>(),
            vec![(0, 2), (2, 0)]
        );
        assert_eq!(found[0].land, (0, 1));
        let up = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (2, 0, 0),
            (0, 1, 2),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(up.reached);
        assert_eq!(up.actions.len(), 1);
        assert_eq!(up.tiles.last(), Some(&(0, 1, 2)));
        let down = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (0, 1, 2),
            (2, 0, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(down.reached);
        assert_eq!(down.tiles.last(), Some(&(2, 0, 0)));
    }

    #[test]
    fn a_stepping_stone_mid_crossing_links_its_row() {
        let (planes, w, h) = world(&[&["...##...", "...##...", "...##..."]]);
        let flags = lookup(&planes, w, h);
        let stone = with_id(5, 3, 1, 0, "Cross");
        let rows = [
            row((0, 1, 0), (6, 1, 0), "Cross", 5),
            row((6, 1, 0), (0, 1, 0), "Cross", 5),
        ];
        let found = data_climbs(&flags, std::slice::from_ref(&stone), &rows);
        assert_eq!(found.len(), 2);
        let plan = plan(
            &flags,
            (0, 0, w - 1, h - 1),
            &[],
            &found,
            (0, 1, 0),
            (7, 1, 0),
            &Options::default(),
            &Closed::default(),
        )
        .unwrap();
        assert!(plan.reached);
        assert_eq!(plan.tiles.last(), Some(&(7, 1, 0)));
        let lookalike = with_id(6, 3, 1, 0, "Cross");
        assert!(data_climbs(&flags, &[lookalike], &rows).is_empty());
    }

    #[test]
    fn a_row_picks_its_loc_by_id_then_by_distance() {
        let (planes, w, h) = world(&[&[".......", ".......", "......."]]);
        let flags = lookup(&planes, w, h);
        let near = with_id(1, 1, 1, 0, "Climb-up");
        let same_id = with_id(7, 3, 1, 0, "Climb-up");
        let other = with_id(7, 2, 1, 0, "Climb-down");
        let from = (2, 0, 0);
        let to = (0, 0, 1);
        let found = data_climbs(
            &flags,
            &[near.clone(), same_id.clone(), other],
            &[row(from, to, "Climb-up", 7)],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].loc, same_id.loc);
        let found = data_climbs(
            &flags,
            &[same_id, near.clone()],
            &[row(from, to, "Climb-up", 99)],
        );
        assert_eq!(found[0].loc, near.loc);
        assert!(
            data_climbs(
                &flags,
                std::slice::from_ref(&near),
                &[row((6, 2, 0), to, "Climb-up", 1)]
            )
            .is_empty()
        );
        assert!(
            data_climbs(
                &flags,
                std::slice::from_ref(&near),
                &[row((1, 0, 1), to, "Climb-up", 1)]
            )
            .is_empty()
        );
        assert!(
            data_climbs(
                &flags,
                std::slice::from_ref(&near),
                &[row(from, to, "Enter", 1)]
            )
            .is_empty()
        );
    }

    #[test]
    fn a_row_used_from_a_tile_you_cannot_stand_on_is_left_out() {
        let (planes, w, h) = world(&[&["...", ".L#", "..."]]);
        let flags = lookup(&planes, w, h);
        let ladder = climbable(1, 1, 0, "Climb-up");
        assert!(
            !data_climbs(
                &flags,
                std::slice::from_ref(&ladder),
                &[row((0, 1, 0), (0, 1, 1), "Climb-up", 1)]
            )
            .is_empty()
        );
        assert!(
            data_climbs(
                &flags,
                std::slice::from_ref(&ladder),
                &[row((2, 1, 0), (0, 1, 1), "Climb-up", 1)]
            )
            .is_empty()
        );
    }

    #[test]
    fn a_shortcut_is_used_only_for_a_real_saving() {
        let rows: &[&str] = &[
            ".......", "...#...", "...#...", "...#...", "...#...", "...#...", "...#...", "...#...",
            "...#...", "...#...",
        ];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let hop = with_id(5, 3, 2, 0, "Jump");
        let over = ClimbRow {
            shortcut_ticks: Some(4),
            ..row((2, 2, 0), (4, 2, 0), "Jump", 5)
        };
        let found = super::data_climbs(&flags, std::slice::from_ref(&hop), &[over], &[]);
        assert_eq!(found[0].shortcut_ticks, Some(4));
        let go = |options: Options, climbs: &[Climb]| {
            plan(
                &flags,
                (0, 0, w - 1, h - 1),
                &[],
                climbs,
                (2, 2, 0),
                (4, 2, 0),
                &options,
                &Closed::default(),
            )
            .unwrap()
        };
        let wanted = go(Options::default(), &found);
        assert!(
            wanted.actions.is_empty(),
            "10 tiles of saving asked, 7.4 offered: walks round"
        );
        let cheap = go(
            Options {
                shortcut_saving: 0,
                ..Options::default()
            },
            &found,
        );
        assert_eq!(
            cheap.actions.len(),
            1,
            "no saving asked: takes the shortcut"
        );
        let ladder = Climb {
            shortcut_ticks: None,
            ..found[0].clone()
        };
        assert_eq!(go(Options::default(), &[ladder]).actions.len(), 1);
    }

    #[test]
    fn a_row_through_a_trapdoor_is_used_on_the_closed_one() {
        let (planes, w, h) = world(&[&[".....", "..L..", "....."]]);
        let flags = lookup(&planes, w, h);
        let closed = with_id(1579, 2, 1, 0, "Open");
        let trapdoor = ClimbRow {
            portal: true,
            ..row((2, 0, 0), (2, 1 + UNDERGROUND, 0), "Climb-down", 1581)
        };
        let doors = vec!["Open".to_owned()];
        let found = super::data_climbs(
            &flags,
            std::slice::from_ref(&closed),
            std::slice::from_ref(&trapdoor),
            &doors,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].loc, closed.loc);
        assert_eq!(found[0].land, (2, 1 + UNDERGROUND));
        let ladder = ClimbRow {
            portal: false,
            ..trapdoor.clone()
        };
        assert!(
            super::data_climbs(&flags, std::slice::from_ref(&closed), &[ladder], &doors).is_empty()
        );
        assert!(
            super::data_climbs(&flags, std::slice::from_ref(&closed), &[trapdoor], &[]).is_empty()
        );
    }

    #[test]
    fn a_loc_with_a_door_option_that_is_not_a_wall_door_is_a_candidate_for_a_row() {
        let rows: &[&str] = &["...", "...", "..."];
        let (planes, w, h) = world(&[rows]);
        let flags = lookup(&planes, w, h);
        let defs = [def(2, &["Open"]), def(2, &["Open"])];
        let locs = [
            loc(0, GAME_OBJECT, 1, 1, Some(10), Some(0), true),
            loc(1, GAME_OBJECT, 2, 2, Some(10), Some(0), true),
        ];
        let lookup_def = |id: i32| defs.get(id as usize);
        let (doors, climbables) = scene_locs(&flags, &locs, &lookup_def, &Request::default());
        assert!(doors.is_empty());
        assert_eq!(
            climbables
                .iter()
                .map(|c| (c.loc.id, c.loc.option.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "Open"), (1, "Open")]
        );
        let row = ClimbRow {
            portal: false,
            ..row((1, 0, 0), (1, 2, 0), "Open", 0)
        };
        let found = super::data_climbs(&flags, &climbables, &[row], &[]);
        assert_eq!(found.len(), 1);
    }
}
