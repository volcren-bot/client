// Randomized walking routes: a jittered sparse lattice over the scene's walkable tiles, routed with per-edge noise and cached per scene.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crate::pathfinding::{game_route, step_open, Route, Tile};

const DIRECTIONS: [(i32, i32); 8] = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)];
// the lattice edges: each node to the next cell east, north, north-east and south-east
const FORWARD: [(i32, i32); 4] = [(1, 0), (0, 1), (1, 1), (1, -1)];
const ATTACH_CANDIDATES: usize = 8;
const ATTACH_KEPT: usize = 4;
const RETRIES: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    // the walk's noise; the same seed gives the same route
    pub seed: u64,
    // edge costs are scaled by 1 +- rnd / 2
    pub rnd: f64,
    // the cells' size in tiles
    pub spacing: i32,
    // how far from its cell's centre a node can be
    pub jitter: i32,
    // routes longer than this times the game's own route are dropped for the game's
    pub max_detour: f64,
    // each call moves the route's nodes by up to this many tiles on each axis, whatever the seed
    pub offset: i32,
}

impl Default for Options {
    fn default() -> Self {
        Options { seed: 0, rnd: 0.8, spacing: 8, jitter: 2, max_detour: 1.3, offset: 2 }
    }
}

pub fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn key_of((x, y): Tile) -> u64 {
    (x as u32 as u64) << 32 | y as u32 as u64
}

fn unit(seed: u64, a: u64, b: u64) -> f64 {
    (mix(seed ^ mix(a ^ mix(b))) >> 11) as f64 / (1u64 << 53) as f64
}

// Picked once per process, so a run keeps its lattice while every run has its own.
fn run_seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| mix(SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) ^ u64::from(std::process::id())))
}

// A new value on every call: the offsets of a route are not the walk's seed, they are re-rolled each time.
fn call_nonce() -> u64 {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    mix(run_seed() ^ mix(CALLS.fetch_add(1, Ordering::Relaxed)))
}

type Flags<'a> =&'a dyn Fn(i32, i32) -> Option<u32>;

// Shortest walk between two tiles inside their bounding box grown by `pad`, as the game steps, `a` first.
fn bfs(flags: Flags, a: Tile, b: Tile, pad: i32, max_len: usize) -> Option<Vec<Tile>> {
    let (x1, y1) = (a.0.min(b.0) - pad, a.1.min(b.1) - pad);
    let (w, h) = (a.0.max(b.0) + pad - x1 + 1, a.1.max(b.1) + pad - y1 + 1);
    let index = |(x, y): Tile| ((x - x1) * h + (y - y1)) as usize;
    let mut from = vec![u32::MAX; (w * h) as usize];
    let mut steps = vec![0u32; (w * h) as usize];
    from[index(a)] = index(a) as u32;
    let mut queue = VecDeque::from([a]);
    while let Some((x, y)) = queue.pop_front() {
        if (x, y) == b {
            let mut path = vec![b];
            let mut at = b;
            while at != a {
                let i = from[index(at)] as i32;
                at = (x1 + i / h, y1 + i % h);
                path.push(at);
            }
            path.reverse();
            return Some(path);
        }
        let here = index((x, y));
        if steps[here] as usize + 1 >= max_len {
            continue;
        }
        for (dx, dy) in DIRECTIONS {
            let n = (x + dx, y + dy);
            if !(x1..x1 + w).contains(&n.0) || !(y1..y1 + h).contains(&n.1) || from[index(n)] != u32::MAX {
                continue;
            }
            if step_open(&flags, x, y, dx, dy) {
                from[index(n)] = here as u32;
                steps[index(n)] = steps[here] + 1;
                queue.push_back(n);
            }
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Key {
    bounds: (i32, i32, i32, i32),
    flags_hash: u64,
    spacing: i32,
    jitter: i32,
}

struct Lattice {
    key: Key,
    // the tiles reachable from where it was built, over the bounds
    inside: Vec<bool>,
    nodes: Vec<Tile>,
    // per node: (neighbour, edge)
    adj: Vec<Vec<(usize, usize)>>,
    // (a, b, the walk from a to b)
    edges: Vec<(usize, usize, Vec<Tile>)>,
}

impl Lattice {
    fn contains(&self, (x, y): Tile) -> bool {
        let (x1, y1, x2, y2) = self.key.bounds;
        (x1..=x2).contains(&x) && (y1..=y2).contains(&y) && self.inside[((x - x1) * (y2 - y1 + 1) + (y - y1)) as usize]
    }

    fn walk(&self, edge: usize, from: usize) -> Vec<Tile> {
        let (a, _, path) = &self.edges[edge];
        if *a == from {
            path.clone()
        } else {
            path.iter().rev().copied().collect()
        }
    }
}

fn flags_hash(flags: Flags, (x1, y1, x2, y2): (i32, i32, i32, i32)) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for x in x1..=x2 {
        for y in y1..=y2 {
            hash = (hash ^ u64::from(flags(x, y).unwrap_or(u32::MAX))).wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

// A tile something can stand on, by the game's masks for an object, a full block and a blocked floor.
fn standable(flags: Flags, x: i32, y: i32) -> bool {
    flags(x, y).is_some_and(|f| f & 0x0024_0100 == 0)
}

fn build(flags: Flags, key: Key) -> Lattice {
    let (x1, y1, x2, y2) = key.bounds;
    let h = y2 - y1 + 1;
    let index = |(x, y): Tile| ((x - x1) * h + (y - y1)) as usize;
    // Nodes go on any standable tile, not only reachable ones, so the same ground gives the same lattice.
    let mut inside = vec![false; ((x2 - x1 + 1) * h) as usize];
    for x in x1..=x2 {
        for y in y1..=y2 {
            inside[index((x, y))] = standable(flags, x, y);
        }
    }

    let seed = run_seed();
    let (spacing, jitter) = (key.spacing, key.jitter);
    // cells are laid out on the world's tiles, so two windows over the same ground get the same nodes
    let (first_x, first_y) = (x1.div_euclid(spacing), y1.div_euclid(spacing));
    let cells_x = x2.div_euclid(spacing) - first_x + 1;
    let cells_y = y2.div_euclid(spacing) - first_y + 1;
    let mut cell_node = vec![usize::MAX; (cells_x * cells_y) as usize];
    let mut nodes = Vec::new();
    for cx in 0..cells_x {
        for cy in 0..cells_y {
            let (ox, oy) = ((first_x + cx) * spacing, (first_y + cy) * spacing);
            let (mx, my) = (ox + spacing / 2, oy + spacing / 2);
            let tiles: Vec<Tile> = (ox.max(x1)..(ox + spacing).min(x2 + 1))
                .flat_map(|x| (oy.max(y1)..(oy + spacing).min(y2 + 1)).map(move |y| (x, y)))
                .filter(|&t| inside[index(t)])
                .collect();
            let near: Vec<Tile> = tiles.iter().copied().filter(|t| (t.0 - mx).abs() <= jitter && (t.1 - my).abs() <= jitter).collect();
            let pool = if near.is_empty() { &tiles } else { &near };
            if pool.is_empty() {
                continue;
            }
            let pick = mix(seed ^ mix(key_of(((first_x + cx) * spacing, (first_y + cy) * spacing)))) as usize % pool.len();
            cell_node[(cx * cells_y + cy) as usize] = nodes.len();
            nodes.push(pool[pick]);
        }
    }

    let mut adj = vec![Vec::new(); nodes.len()];
    let mut edges = Vec::new();
    let max_len = (spacing as f64 * 2.2) as usize;
    for cx in 0..cells_x {
        for cy in 0..cells_y {
            let a = cell_node[(cx * cells_y + cy) as usize];
            if a == usize::MAX {
                continue;
            }
            for (dx, dy) in FORWARD {
                let (nx, ny) = (cx + dx, cy + dy);
                if !(0..cells_x).contains(&nx) || !(0..cells_y).contains(&ny) {
                    continue;
                }
                let b = cell_node[(nx * cells_y + ny) as usize];
                if b == usize::MAX {
                    continue;
                }
                if let Some(path) = bfs(flags, nodes[a], nodes[b], 3, max_len) {
                    adj[a].push((b, edges.len()));
                    adj[b].push((a, edges.len()));
                    edges.push((a, b, path));
                }
            }
        }
    }
    Lattice { key, inside, nodes, adj, edges }
}

// the lattices of the last few queries: a walk's own and the scene's don't push each other out
static CACHE: Mutex<Vec<Arc<Lattice>>> = Mutex::new(Vec::new());
const CACHED: usize = 3;

fn lattice(flags: Flags, bounds: (i32, i32, i32, i32), options: &Options) -> Arc<Lattice> {
    let key = Key { bounds, flags_hash: flags_hash(flags, bounds), spacing: options.spacing, jitter: options.jitter };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = cache.iter().position(|c| c.key == key) {
        let cached = cache.remove(at);
        cache.insert(0, cached.clone());
        return cached;
    }
    let built = Arc::new(build(flags, key));
    cache.insert(0, built.clone());
    cache.truncate(CACHED);
    built
}

// The nearest few nodes to `tile` by walking distance, each with the walk from `tile`.
fn attach(flags: Flags, lattice: &Lattice, tile: Tile) -> Vec<(usize, Vec<Tile>)> {
    let mut near: Vec<usize> = (0..lattice.nodes.len()).collect();
    let distance = |n: usize| (lattice.nodes[n].0 - tile.0).abs().max((lattice.nodes[n].1 - tile.1).abs());
    near.sort_by_key(|&n| distance(n));
    let max_len = lattice.key.spacing as usize * 4;
    let mut found: Vec<(usize, Vec<Tile>)> = near
        .into_iter()
        .take(ATTACH_CANDIDATES)
        .filter_map(|n| bfs(flags, tile, lattice.nodes[n], 6, max_len).map(|path| (n, path)))
        .collect();
    found.sort_by_key(|(_, path)| path.len());
    found.truncate(ATTACH_KEPT);
    found
}

// A walk can't turn back on itself: cut every loop out of it.
fn without_loops(tiles: Vec<Tile>) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if let Some(at) = out.iter().position(|&t| t == tile) {
            out.truncate(at + 1);
        } else {
            out.push(tile);
        }
    }
    out
}

// How far a target can be for `route` to look past the game's 128 x 128 route finder: the search box and longest walk.
const FAR_PAD: i32 = 64;
const FAR_MAX_LEN: usize = 1000;

// The game's route from `from` to `to`, or one over the lattice when the target is beyond ~60 tiles, searched inside `bounds`.
pub fn route(flags: Flags, bounds: (i32, i32, i32, i32), from: Tile, to: Tile, options: &Options) -> Option<Route> {
    let plain = game_route(flags, from, to);
    let (to, baseline) = match &plain {
        Some(plain) if plain.reached => (*plain.tiles.last()?, plain.tiles.len()),
        // out of the game's sight: the shortest walk there is the baseline, or it can't be reached
        _ => match bfs(flags, from, to, FAR_PAD, FAR_MAX_LEN) {
            Some(walk) => {
                if options.spacing < 4 || walk.len() < options.spacing as usize * 2 {
                    return Some(Route { tiles: walk, reached: true });
                }
                (to, walk.len())
            }
            None => return plain,
        },
    };
    if options.spacing < 4 || baseline < options.spacing as usize * 2 {
        return plain;
    }
    // a route over the cap is tried again with other noise before giving up on the lattice
    let routed = (0..RETRIES).find_map(|attempt| {
        let options = Options { seed: mix(options.seed.wrapping_add(attempt)), ..*options };
        over_lattice(flags, bounds, from, to, &options, baseline)
    });
    match routed {
        Some(tiles) => Some(Route { tiles, reached: true }),
        None => plain.filter(|p| p.reached).or_else(|| bfs(flags, from, to, FAR_PAD, FAR_MAX_LEN).map(|tiles| Route { tiles, reached: true })),
    }
}

// The route through `nodes` with each node moved to a random standable tile within `options.offset`; None when the last leg can't be found.
fn offset_route(flags: Flags, lattice: &Lattice, from: Tile, to: Tile, nodes: &[Tile], options: &Options) -> Option<Vec<Tile>> {
    const DRAWS: usize = 6;
    let mut state = call_nonce() ^ options.seed;
    let mut next = move || {
        state = mix(state);
        state
    };
    let span = (options.offset * 2 + 1) as u64;
    let max_len = lattice.key.spacing as usize * 4;
    let mut tiles = vec![from];
    let mut at = from;
    for &node in nodes {
        let pad = if at == from { 6 } else { 3 };
        let mut moved = None;
        for _ in 0..DRAWS {
            let r = next();
            let candidate = (node.0 + (r % span) as i32 - options.offset, node.1 + ((r >> 32) % span) as i32 - options.offset);
            if candidate == node || !lattice.contains(candidate) || !standable(flags, candidate.0, candidate.1) {
                continue;
            }
            if let Some(walk) = bfs(flags, at, candidate, pad, max_len) {
                moved = Some(walk);
                break;
            }
        }
        // the node itself is what the lattice already walks to, so it's the safe draw
        let walk = moved.or_else(|| bfs(flags, at, node, pad, max_len))?;
        at = *walk.last()?;
        tiles.extend(walk.into_iter().skip(1));
    }
    tiles.extend(bfs(flags, at, to, 6, max_len)?.into_iter().skip(1));
    Some(tiles)
}

fn over_lattice(flags: Flags, bounds: (i32, i32, i32, i32), from: Tile, to: Tile, options: &Options, plain_len: usize) -> Option<Vec<Tile>> {
    let lattice = lattice(flags, bounds, options);
    if !lattice.contains(to) {
        return None;
    }
    let starts = attach(flags, &lattice, from);
    let ends = attach(flags, &lattice, to);
    if starts.is_empty() || ends.is_empty() {
        return None;
    }
    // per walk and per pair of places, so the same walk sees the same noise from another window
    let noise = |a: Tile, b: Tile| (1.0 + (unit(options.seed, key_of(a.min(b)), key_of(a.max(b))) - 0.5) * options.rnd).max(0.2);
    let n = lattice.nodes.len();
    let mut cost = vec![f64::INFINITY; n];
    let mut previous = vec![(usize::MAX, usize::MAX); n];
    let mut queue = BinaryHeap::new();
    for (node, path) in &starts {
        let c = (path.len() - 1) as f64 * noise(lattice.nodes[*node], from);
        if c < cost[*node] {
            cost[*node] = c;
            queue.push(Reverse(((c * 100.0) as u64, *node)));
        }
    }
    while let Some(Reverse((c, node))) = queue.pop() {
        if c as f64 / 100.0 > cost[node] + 0.01 {
            continue;
        }
        for &(next, edge) in &lattice.adj[node] {
            let step = (lattice.edges[edge].2.len() - 1) as f64 * noise(lattice.nodes[node], lattice.nodes[next]);
            if cost[node] + step < cost[next] {
                cost[next] = cost[node] + step;
                previous[next] = (node, edge);
                queue.push(Reverse(((cost[next] * 100.0) as u64, next)));
            }
        }
    }
    let (last, tail) = ends
        .iter()
        .filter(|(node, _)| cost[*node].is_finite())
        .min_by(|(a, pa), (b, pb)| {
            let score = |node: usize, path: &Vec<Tile>| cost[node] + (path.len() - 1) as f64 * noise(lattice.nodes[node], to);
            score(*a, pa).total_cmp(&score(*b, pb))
        })?;

    let mut chain = vec![*last];
    let mut walked = Vec::new();
    let mut at = *last;
    while previous[at].0 != usize::MAX {
        let (before, edge) = previous[at];
        walked.push((before, edge));
        chain.push(before);
        at = before;
    }
    chain.reverse();
    walked.reverse();
    let (_, head) = starts.iter().find(|(node, _)| *node == chain[0])?;

    let mut tiles = head.clone();
    for (node, edge) in walked {
        tiles.extend(lattice.walk(edge, node).into_iter().skip(1));
    }
    tiles.extend(tail.iter().rev().copied().skip(1));
    let tiles = if options.offset > 0 {
        let nodes: Vec<Tile> = chain.iter().map(|&n| lattice.nodes[n]).collect();
        offset_route(flags, &lattice, from, to, &nodes, options).unwrap_or(tiles)
    } else {
        tiles
    };
    let tiles = without_loops(tiles);
    debug_assert_eq!((tiles.first(), tiles.last()), (Some(&from), Some(&to)));
    (tiles.len() as f64 <= plain_len as f64 * options.max_detour).then_some(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_collision::{from_rows, SceneCollision};

    fn open(size: usize, walls: &[(usize, usize)]) -> SceneCollision {
        // `walls` as (x, y) inner tiles, y up
        let mut rows = vec![vec!['.'; size]; size];
        for &(x, y) in walls {
            rows[size - 1 - y][x] = '#';
        }
        let rows: Vec<String> = rows.into_iter().map(|r| r.into_iter().collect()).collect();
        from_rows(0, 0, &rows.iter().map(String::as_str).collect::<Vec<_>>())
    }

    fn walk(scene: &SceneCollision, from: Tile, to: Tile, seed: u64) -> Route {
        let flags = |x, y| scene.flags(x, y);
        // no offsets: those are new on every call, and these tests compare calls
        let options = Options { seed, offset: 0, ..Options::default() };
        route(&flags, scene.bounds(), from, to, &options).unwrap()
    }

    #[test]
    fn offsets_make_every_call_different_and_legal() {
        let scene = open(60, &[]);
        let flags = |x, y| scene.flags(x, y);
        let (from, to) = ((2, 2), (57, 57));
        let plain = game_route(flags, from, to).unwrap().tiles.len();
        let options = Options { seed: 3, ..Options::default() };
        let routes: Vec<Vec<Tile>> = (0..8)
            .map(|_| {
                let r = route(&flags, scene.bounds(), from, to, &options).unwrap();
                assert_legal(&scene, &r, from, to);
                assert!(r.tiles.len() as f64 <= plain as f64 * 1.3);
                r.tiles
            })
            .collect();
        let mut distinct = routes.clone();
        distinct.sort();
        distinct.dedup();
        // the same seed, and still every call has its own route
        assert!(distinct.len() >= 7, "only {} different routes", distinct.len());
        // without offsets the seed is the route
        let still = Options { offset: 0, ..options };
        let first = route(&flags, scene.bounds(), from, to, &still).unwrap();
        assert_eq!(route(&flags, scene.bounds(), from, to, &still).unwrap(), first);
    }

    #[test]
    fn offsets_keep_to_the_gap() {
        let walls: Vec<(usize, usize)> = (0..60).filter(|&y| y != 30).map(|y| (30, y)).collect();
        let scene = open(60, &walls);
        let flags = |x, y| scene.flags(x, y);
        let (from, to) = ((2, 30), (57, 30));
        for seed in 0..10 {
            let options = Options { seed, offset: 0, ..Options::default() };
            let r = route(&flags, scene.bounds(), from, to, &options).unwrap();
            assert_legal(&scene, &r, from, to);
            assert!(r.tiles.contains(&(30, 30)), "seed {seed} missed the gap");
        }
    }

    fn assert_legal(scene: &SceneCollision, route: &Route, from: Tile, to: Tile) {
        let flags = |x, y| scene.flags(x, y);
        assert_eq!(route.tiles.first(), Some(&from));
        assert_eq!(route.tiles.last(), Some(&to));
        for w in route.tiles.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            assert!(step_open(&flags, w[0].0, w[0].1, dx, dy), "illegal step {w:?}");
        }
    }

    #[test]
    fn open_ground_takes_different_routes_no_longer_than_the_cap() {
        let scene = open(60, &[]);
        let (from, to) = ((2, 2), (57, 57));
        let plain = game_route(|x, y| scene.flags(x, y), from, to).unwrap().tiles.len();
        let routes: Vec<Vec<Tile>> = (0..8)
            .map(|seed| {
                let r = walk(&scene, from, to, seed);
                assert_legal(&scene, &r, from, to);
                assert!(r.tiles.len() as f64 <= plain as f64 * 1.3, "{} vs {plain}", r.tiles.len());
                r.tiles
            })
            .collect();
        let mut distinct = routes.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() >= 4, "only {} different routes", distinct.len());
        // the same seed is the same route
        assert_eq!(walk(&scene, from, to, 3).tiles, routes[3]);
    }

    #[test]
    fn a_wall_with_a_gap_forces_the_corridor() {
        let walls: Vec<(usize, usize)> = (0..60).filter(|&y| y != 30).map(|y| (30, y)).collect();
        let scene = open(60, &walls);
        let (from, to) = ((2, 30), (57, 30));
        for seed in 0..6 {
            let r = walk(&scene, from, to, seed);
            assert_legal(&scene, &r, from, to);
            assert!(r.tiles.contains(&(30, 30)), "seed {seed} missed the gap");
        }
    }

    #[test]
    fn unreachable_targets_use_the_games_route() {
        let rows = [
            "..........", //
            ".########.",
            ".#......#.",
            ".#......#.",
            ".########.",
            "..........",
        ];
        let scene = from_rows(0, 0, &rows);
        let flags = |x, y| scene.flags(x, y);
        let plain = game_route(flags, (0, 0), (4, 3)).unwrap();
        assert!(!plain.reached);
        let routed = route(&flags, scene.bounds(), (0, 0), (4, 3), &Options::default()).unwrap();
        assert_eq!(routed, plain);
    }

    #[test]
    fn short_walks_are_the_games() {
        let scene = open(60, &[]);
        let flags = |x, y| scene.flags(x, y);
        let plain = game_route(flags, (2, 2), (9, 9)).unwrap();
        assert_eq!(route(&flags, scene.bounds(), (2, 2), (9, 9), &Options::default()).unwrap(), plain);
    }

    #[test]
    fn targets_past_the_games_sight_route_over_the_lattice() {
        let scene = open(200, &[]);
        let flags = |x, y| scene.flags(x, y);
        let (from, to) = ((2, 2), (190, 190));
        assert!(game_route(flags, from, to).is_none_or(|r| !r.reached));
        let baseline = bfs(&flags, from, to, FAR_PAD, FAR_MAX_LEN).unwrap().len();
        let mut distinct = std::collections::HashSet::new();
        for seed in 0..6 {
            let options = Options { seed, offset: 0, ..Options::default() };
            let r = route(&flags, scene.bounds(), from, to, &options).unwrap();
            assert!(r.reached);
            assert_legal(&scene, &r, from, to);
            assert!(r.tiles.len() as f64 <= baseline as f64 * 1.3, "{} vs {baseline}", r.tiles.len());
            distinct.insert(r.tiles);
        }
        assert!(distinct.len() >= 3, "only {} different routes", distinct.len());
    }

    #[test]
    fn routes_cross_the_seam_of_two_sources() {
        // a loaded scene on the left with an open door, "cache" ground on the right with it closed: the first source answers west of x 60
        let live = open(60, &(0..60).filter(|&y| y != 30).map(|y| (30, y)).collect::<Vec<_>>());
        let cache = from_rows(60, 0, &vec![&".".repeat(60)[..]; 60]);
        let flags = |x: i32, y: i32| if x < 60 { live.flags(x, y) } else { cache.flags(x, y) };
        let (from, to) = ((2, 30), (115, 30));
        let bounds = (-1, -1, 120, 61);
        for seed in 0..5 {
            let options = Options { seed, offset: 0, ..Options::default() };
            let r = route(&flags, bounds, from, to, &options).unwrap();
            assert_eq!((r.tiles.first(), r.tiles.last()), (Some(&from), Some(&to)));
            assert!(r.tiles.contains(&(30, 30)), "seed {seed} missed the gap");
            for w in r.tiles.windows(2) {
                let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                assert!(step_open(&flags, w[0].0, w[0].1, dx, dy), "illegal step {w:?}");
            }
        }
    }

    #[test]
    fn a_route_doesnt_depend_on_the_window() {
        let scene = open(200, &[(100, 100), (101, 100), (100, 101), (101, 101)]);
        let flags = |x, y| scene.flags(x, y);
        let (from, to) = ((50, 50), (150, 150));
        for seed in 0..4 {
            let options = Options { seed, offset: 0, ..Options::default() };
            let whole = route(&flags, scene.bounds(), from, to, &options).unwrap();
            let part = route(&flags, (30, 33, 171, 169), from, to, &options).unwrap();
            assert_eq!(whole.tiles, part.tiles, "seed {seed}");
        }
    }
}
