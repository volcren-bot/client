
use std::collections::HashMap;

use ipc::{
    read::{self, Memory},
    state::{part, Layout},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Via {
    #[default]
    Auto,
    Local,
    Client,
}

impl Via {
    pub fn parse(name: Option<&str>) -> Result<Self, String> {
        match name {
            None | Some("auto") => Ok(Self::Auto),
            Some("local") => Ok(Self::Local),
            Some("client") => Ok(Self::Client),
            Some(other) => Err(format!("unknown projection {other:?} (auto, local or client)")),
        }
    }
}

pub const WALL: u8 = 0;
pub const WALL_DECORATION: u8 = 1;
pub const GAME_OBJECT: u8 = 2;
pub const GROUND_DECORATION: u8 = 3;

pub const NEEDS: u64 = part::PROJECTION | part::CAMERA | part::INTERFACES;

const TILE_SHIFT: i32 = 7;
const TILE: i32 = 1 << TILE_SHIFT;
const PLANES: i32 = 4;
const BRIDGE: u8 = 2;
const TILE_ENTRY_SIZE: usize = 16;
const TILE_ENTRY_OBJECT: usize = 8;
const VERTEX_SIZE: usize = 12;
const MAX_TRIANGLES: usize = 4096;
const PAINT_CORNERS: [[i32; 2]; 4] = [[0, 0], [1, 0], [1, 1], [0, 1]];
const PAINT_TRIANGLES: [[usize; 3]; 2] = [[1, 2, 3], [0, 1, 3]];

type Vertex = [i32; 3];
type Triangle = [Vertex; 3];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Unavailable(String),
}

fn truncate(value: f32) -> i32 {
    if value.is_nan() || value >= 2_147_483_648.0 || value < -2_147_483_648.0 { i32::MIN } else { value as i32 }
}

fn i32s<const N: usize>(memory: &impl Memory, address: u64) -> Option<[i32; N]> {
    let mut bytes = [0u8; 16];
    let bytes = bytes.get_mut(..N * 4)?;
    memory.read(address, bytes).then(|| core::array::from_fn(|i| i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())))
}

const WINDOW_SIZE: usize = 512;

// The fields of one object that lie near each other, fetched in a single read; others are read on their own.
struct Window<'a, M: Memory> {
    memory: &'a M,
    base: u64,
    low: u64,
    high: u64,
    bytes: [u8; WINDOW_SIZE],
    filled: bool,
}

impl<'a, M: Memory> Window<'a, M> {
    // `fields` are the (offset, size) pairs the window will be asked for.
    fn new(memory: &'a M, base: u64, fields: &[(u64, u64)]) -> Self {
        let low = fields.iter().map(|f| f.0).min().unwrap_or(0);
        let high = fields.iter().map(|f| f.0 + f.1).max().unwrap_or(0);
        let mut window = Self { memory, base, low, high, bytes: [0; WINDOW_SIZE], filled: false };
        if high > low && ((high - low) as usize) <= WINDOW_SIZE {
            window.filled = memory.read(base + low, &mut window.bytes[..(high - low) as usize]);
        }
        window
    }

    fn get<const N: usize>(&self, offset: u64) -> Option<[u8; N]> {
        let end = offset + N as u64;
        if self.filled && offset >= self.low && end <= self.high {
            let at = (offset - self.low) as usize;
            return self.bytes[at..at + N].try_into().ok();
        }
        let mut bytes = [0; N];
        self.memory.read(self.base + offset, &mut bytes).then_some(bytes)
    }

    fn i32(&self, offset: u64) -> Option<i32> {
        self.get(offset).map(i32::from_le_bytes)
    }

    fn f32(&self, offset: u64) -> Option<f32> {
        self.get(offset).map(f32::from_le_bytes)
    }

    fn i32s<const N: usize>(&self, offset: u64) -> Option<[i32; N]> {
        let mut out = [0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.i32(offset + i as u64 * 4)?;
        }
        Some(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub position: [i32; 3],
    pub trig: [f32; 4],
    pub zoom: i32,
    pub center: [i32; 2],
    pub offset: [i32; 2],
    pub canvas: [i32; 2],
    pub ui: [i32; 2],
    pub scale: f32,
    // `ui / canvas` per axis, which every projected point scales by
    ratio: [f32; 2],
}

impl Camera {
    pub fn read(memory: &impl Memory, layout: &Layout, client: u64, scene: u64) -> Option<Self> {
        let mut trig = [0u8; 16];
        if !memory.read(scene + layout.projection.trig as u64, &mut trig) {
            return None;
        }
        Self::read_with(memory, layout, client, trig)
    }

    // `read` with the scene's trig bytes already in hand.
    fn read_with(memory: &impl Memory, layout: &Layout, client: u64, trig: [u8; 16]) -> Option<Self> {
        let p = &layout.projection;
        let canvas = memory.ptr(client + layout.interfaces.canvas as u64)?;
        let view = memory.ptr(canvas + p.view_holder as u64 + p.view as u64)?;
        let camera = Window::new(memory, client, &[(layout.camera.position as u64, 12), (p.zoom as u64, 4)]);
        let view_fields = Window::new(memory, view, &[(p.view_center as u64, 8), (p.view_offset_x as u64, 4), (p.view_offset_y as u64, 4)]);
        let canvas_fields = Window::new(memory, canvas, &[(p.canvas_size as u64, 8), (p.ui_size as u64, 8), (layout.interfaces.ui_scale as u64, 4)]);
        let canvas_size = canvas_fields.i32s::<2>(p.canvas_size as u64)?;
        let ui = canvas_fields.i32s::<2>(p.ui_size as u64)?;
        Some(Self {
            position: camera.i32s(layout.camera.position as u64)?,
            trig: core::array::from_fn(|i| f32::from_le_bytes(trig[i * 4..i * 4 + 4].try_into().unwrap())),
            zoom: camera.i32(p.zoom as u64)?,
            center: view_fields.i32s(p.view_center as u64)?,
            offset: [view_fields.i32(p.view_offset_x as u64)?, view_fields.i32(p.view_offset_y as u64)?],
            canvas: canvas_size,
            ui,
            scale: canvas_fields.f32(layout.interfaces.ui_scale as u64)?,
            ratio: [ui[0] as f32 / canvas_size[0] as f32, ui[1] as f32 / canvas_size[1] as f32],
        })
    }

    pub fn project(&self, x: i32, y: i32, height: i32) -> [f32; 2] {
        self.project_at(x, y, height).0
    }

    /// `project`, but `None` for a point closer to the camera than `NEAR_PLANE` or behind it, as RuneLite's `Perspective.localToCanvas`.
    pub fn project_near(&self, x: i32, y: i32, height: i32) -> Option<[f32; 2]> {
        let (screen, distance) = self.project_at(x, y, height);
        (distance >= NEAR_PLANE).then_some(screen)
    }

    // The screen position and the distance along the view direction.
    fn project_at(&self, x: i32, y: i32, height: i32) -> ([f32; 2], f32) {
        let [sin_pitch, cos_pitch, sin_yaw, cos_yaw] = self.trig;
        let dx = x.wrapping_sub(self.position[0]) as f32;
        let dz = height.wrapping_sub(self.position[1]) as f32;
        let dy = y.wrapping_sub(self.position[2]) as f32;
        let depth = cos_yaw * dy - sin_yaw * dx;
        let across = cos_yaw * dx + sin_yaw * dy;
        let up = cos_pitch * dz - sin_pitch * depth;
        let distance = sin_pitch * dz + cos_pitch * depth;
        let zoom = self.zoom as f32;
        let sx = truncate(zoom * across / distance).wrapping_add(self.offset[0]).wrapping_add(self.center[0]);
        let sy = truncate(zoom * up / distance).wrapping_add(self.offset[1]).wrapping_add(self.center[1]);
        ([self.canvas_units(sx, 0), self.canvas_units(sy, 1)], distance)
    }

    fn canvas_units(&self, value: i32, axis: usize) -> f32 {
        let units = self.ratio[axis] * value as f32;
        truncate(if units < 0.0 { units - 0.5 } else { units + 0.5 }) as f32 * self.scale
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneKey {
    pub scene: u64,
    pub plane: i32,
    pub base: [i32; 2],
    pub heights: u64,
    pub tiles: u64,
}

pub fn scene(memory: &impl Memory, layout: &Layout, client: u64) -> Option<SceneKey> {
    scene_and_trig(memory, layout, client).map(|(key, _)| key)
}

// The scene's key and its trig bytes, which sit next to the heights pointer and so share its read.
fn scene_and_trig(memory: &impl Memory, layout: &Layout, client: u64) -> Option<(SceneKey, [u8; 16])> {
    let p = &layout.projection;
    let world_view = memory.ptr(client + p.top_world_view as u64)?;
    let has_player = layout.has(part::PLAYER);
    let mut fields = vec![(p.scene as u64, 8), (p.plane as u64, 4)];
    if has_player {
        fields.push((layout.player.base_x as u64, 8));
    }
    let view = Window::new(memory, world_view, &fields);
    let scene = view.get(p.scene as u64).map(u64::from_le_bytes).filter(|&p| p != 0)?;
    let base = if has_player { view.i32s(layout.player.base_x as u64)? } else { [0; 2] };
    let plane = view.i32(p.plane as u64)?;
    let fields = Window::new(memory, scene, &[(p.heights as u64, 8), (p.tiles as u64, 8), (p.trig as u64, 16)]);
    let pointer = |offset: u64| fields.get(offset).map(u64::from_le_bytes).filter(|&p| p != 0);
    let key = SceneKey { scene, plane, base, heights: pointer(p.heights as u64)?, tiles: pointer(p.tiles as u64)? };
    Some((key, fields.get(p.trig as u64)?))
}

pub struct Terrain {
    key: SceneKey,
    size: [i32; 2],
    heights: Vec<i32>,
    settings: Vec<u8>,
    tiles: Vec<u64>,
    meshes: Vec<Option<Option<Vec<Triangle>>>>,
    decorations: HashMap<(i32, i32, i32), i32>,
}

impl Terrain {
    pub fn read(memory: &impl Memory, layout: &Layout, key: SceneKey) -> Option<Self> {
        let p = &layout.projection;
        let [size_x, size_y] = [memory.i32(key.scene + p.size_x as u64)?, memory.i32(key.scene + p.size_y as u64)?];
        if !(1..=1024).contains(&size_x) || !(1..=1024).contains(&size_y) {
            return None;
        }
        let tiles = (PLANES * size_x * size_y) as usize;
        let heights = memory.i32s(key.heights, (PLANES * (size_x + 1) * (size_y + 1)) as usize)?;
        let tile_count = tiles;
        let mut settings = vec![0u8; tiles];
        let settings_at = memory.ptr(key.scene + p.settings as u64)?;
        let mut entries = vec![0u8; tiles * TILE_ENTRY_SIZE];
        if !memory.read(settings_at, &mut settings) || !memory.read(key.tiles, &mut entries) {
            return None;
        }
        let tiles = entries
            .as_chunks::<TILE_ENTRY_SIZE>().0.iter()
            .map(|e| u64::from_le_bytes(e[TILE_ENTRY_OBJECT..TILE_ENTRY_OBJECT + 8].try_into().unwrap()))
            .collect();
        Some(Self { key, size: [size_x, size_y], heights, settings, tiles, meshes: vec![None; tile_count], decorations: HashMap::new() })
    }

    pub fn key(&self) -> SceneKey {
        self.key
    }

    // The client's tile settings of a scene tile (`x`, `y` in scene coordinates), `None` off the scene.
    pub fn setting(&self, plane: i32, x: i32, y: i32) -> Option<u8> {
        self.tile_index(plane, x, y).map(|i| self.settings[i])
    }

    // The size of the scene, in tiles.
    pub fn size(&self) -> [i32; 2] {
        self.size
    }

    pub fn set_decorations(&mut self, decorations: impl IntoIterator<Item = ((i32, i32, i32), i32)>) {
        self.decorations = decorations.into_iter().filter(|&(_, height)| height != 0).collect();
    }

    fn tile_index(&self, plane: i32, x: i32, y: i32) -> Option<usize> {
        let [size_x, size_y] = self.size;
        ((0..PLANES).contains(&plane) && (0..size_x).contains(&x) && (0..size_y).contains(&y)).then(|| ((plane * size_x + x) * size_y + y) as usize)
    }

    fn height(&self, plane: i32, x: i32, y: i32) -> i32 {
        let [size_x, size_y] = self.size;
        let index = (plane.wrapping_mul(size_x + 1).wrapping_add(x)).wrapping_mul(size_y + 1).wrapping_add(y);
        usize::try_from(index).ok().and_then(|i| self.heights.get(i)).copied().unwrap_or(0)
    }

    // The tile's heights come from its own corners, which an edge point shares with the tile beyond: `ax`/`ay` can be 0 or TILE.
    fn bilinear(&self, x: i32, y: i32, plane: i32, (tx, ty): (i32, i32)) -> i32 {
        let bridge = plane < 3 && self.tile_index(1, tx, ty).is_some_and(|i| self.settings[i] & BRIDGE != 0);
        let plane = if bridge { plane + 1 } else { plane };
        let (ax, ay) = (x - (tx << TILE_SHIFT), y - (ty << TILE_SHIFT));
        let h = |dx: i32, dy: i32| self.height(plane, tx + dx, ty + dy);
        let north = h(0, 1).wrapping_mul(TILE - ax).wrapping_add(h(1, 1).wrapping_mul(ax)) >> TILE_SHIFT;
        let south = h(1, 0).wrapping_mul(ax).wrapping_add(h(0, 0).wrapping_mul(TILE - ax)) >> TILE_SHIFT;
        south.wrapping_mul(TILE - ay).wrapping_add(north.wrapping_mul(ay)) >> TILE_SHIFT
    }

    fn mesh(&mut self, memory: &impl Memory, layout: &Layout, index: usize) -> Option<&[Triangle]> {
        if self.meshes[index].is_none() {
            self.meshes[index] = Some(self.read_mesh(memory, layout, self.tiles[index]));
        }
        self.meshes[index].as_ref().and_then(|mesh| mesh.as_deref())
    }

    fn read_mesh(&self, memory: &impl Memory, layout: &Layout, tile: u64) -> Option<Vec<Triangle>> {
        let p = &layout.projection;
        if let Some(model) = memory.ptr(tile + p.tile_model as u64) {
            let [begin, end] = [memory.ptr(model + p.model_triangles as u64)?, memory.ptr(model + p.model_triangles as u64 + 8)?];
            let count = (end.checked_sub(begin)? as usize / VERTEX_SIZE).min(MAX_TRIANGLES);
            let indices = memory.i32s(begin, count * 3)?;
            let vertex_count = usize::try_from(*indices.iter().max()?).ok()? + 1;
            // A scene still loading can hold garbage here; a real tile model has far fewer vertices than triangles allow.
            if vertex_count > MAX_TRIANGLES * 3 {
                return None;
            }
            let vertices = memory.i32s(memory.ptr(model + p.model_vertices as u64)?, vertex_count * 3)?;
            let vertex = |i: i32| -> Option<Vertex> { vertices.get(i as usize * 3..i as usize * 3 + 3)?.try_into().ok() };
            return indices.as_chunks::<3>().0.iter().map(|t| Some([vertex(t[0])?, vertex(t[1])?, vertex(t[2])?])).collect();
        }
        memory.ptr(tile + p.tile_paint as u64)?;
        // paint follows the heightmap of the tile's own plane, which differs under bridges
        let [x, y, plane] = i32s(memory, tile + p.tile_x as u64)?;
        let corners = PAINT_CORNERS.map(|[dx, dy]| [(x + dx) << TILE_SHIFT, self.height(plane, x + dx, y + dy), (y + dy) << TILE_SHIFT]);
        Some(PAINT_TRIANGLES.iter().map(|t| t.map(|i| corners[i])).collect())
    }

    // The heightmap's height under (x, y), without tile models or decorations: what the client flies projectiles over.
    pub fn heightmap(&self, x: i32, y: i32, plane: i32) -> i32 {
        self.bilinear(x, y, plane, (x >> TILE_SHIFT, y >> TILE_SHIFT))
    }

    // The heightmap under (x, y), 0 outside the scene.
    pub fn flat_ground(&self, x: i32, y: i32, plane: i32) -> i32 {
        let [size_x, size_y] = self.size;
        if (0..size_x).contains(&(x >> TILE_SHIFT)) && (0..size_y).contains(&(y >> TILE_SHIFT)) { self.heightmap(x, y, plane) } else { 0 }
    }

    pub fn ground(&mut self, memory: &impl Memory, layout: &Layout, x: i32, y: i32, plane: i32) -> i32 {
        self.ground_on(memory, layout, x, y, plane, None)
    }

    // The ground under (x, y), from tile `on` if given, else the tile the point falls in (an edge falls in the tile beyond).
    pub fn ground_on(&mut self, memory: &impl Memory, layout: &Layout, x: i32, y: i32, plane: i32, on: Option<[i32; 2]>) -> i32 {
        let [size_x, size_y] = self.size;
        let (tx, ty) = match on {
            Some([tx, ty]) if (0..size_x).contains(&tx) && (0..size_y).contains(&ty) => (tx, ty),
            _ => (x >> TILE_SHIFT, y >> TILE_SHIFT),
        };
        if tx < 0 || ty < 0 || tx > size_x - 1 || ty > size_y - 1 {
            return 0;
        }
        let Some(index) = self.tile_index(plane, tx, ty).filter(|&i| self.tiles[i] != 0) else {
            return self.bilinear(x, y, plane, (tx, ty));
        };
        let on_mesh = self.mesh(memory, layout, index).and_then(|triangles| triangles.iter().find(|t| inside(x, y, t)).map(|t| barycentric(x, y, t)));
        let height = on_mesh.unwrap_or_else(|| self.bilinear(x, y, plane, (tx, ty)));
        height.wrapping_sub(self.decorations.get(&(plane, tx, ty)).copied().unwrap_or(0))
    }
}

fn inside(px: i32, py: i32, [a, b, c]: &Triangle) -> bool {
    let (xs, ys) = ([a[0], b[0], c[0]], [a[2], b[2], c[2]]);
    if ys.iter().all(|&y| py < y) || ys.iter().all(|&y| py > y) || xs.iter().all(|&x| px < x) || xs.iter().all(|&x| px > x) {
        return false;
    }
    let side = |p: &Vertex, q: &Vertex| (py.wrapping_sub(p[2])).wrapping_mul(q[0].wrapping_sub(p[0])).wrapping_sub((q[2].wrapping_sub(p[2])).wrapping_mul(px.wrapping_sub(p[0])));
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    match d1.signum() {
        0 if d2 == 0 => true,
        0 if d2 < 0 => d3 <= 0,
        0 => d3 >= 0,
        -1 => d2 <= 0 && d3 <= 0,
        _ => d2 >= 0 && d3 >= 0,
    }
}

fn barycentric(px: i32, py: i32, [a, b, c]: &Triangle) -> i32 {
    let [(ax, ah, ay), (bx, bh, by), (cx, ch, cy)] = [a, b, c].map(|v| (v[0], v[1], v[2]));
    let (dx, dy) = (px.wrapping_sub(cx), py.wrapping_sub(cy));
    let den = ((ay - cy).wrapping_mul(cx - bx)).wrapping_add((by - cy).wrapping_mul(ax - cx)) as f32;
    let wa = ((by - cy).wrapping_mul(dx)).wrapping_add(dy.wrapping_mul(cx - bx)) as f32 / den;
    let wb = ((cy - ay).wrapping_mul(dx)).wrapping_add(dy.wrapping_mul(ax - cx)) as f32 / den;
    let wc = 1.0 - wa - wb;
    truncate(bh as f32 * wb + ah as f32 * wa + wc * ch as f32)
}

/// Nearer than this along the view direction a point is not drawn, as in RuneLite.
pub const NEAR_PLANE: f32 = 50.0;

pub type Decoration = ((i32, i32, i32), i32);

/// A point to project: `x`, `y`, `z` (height above ground, negative up) and the `tile` whose ground it stands on (default: the tile under it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub tile: Option<[i32; 2]>,
    /// Stand on the bare heightmap rather than the tile's model, as RuneLite's `Perspective.getTileHeight`.
    pub heightmap: bool,
}

impl Point {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z, tile: None, heightmap: false }
    }
}

// Reads the scene's terrain into `terrain` unless it already holds the shown scene; returns the scene's key.
pub fn ensure_terrain(
    memory: &impl Memory,
    layout: &Layout,
    terrain: &mut Option<Terrain>,
    decorations: impl FnOnce() -> Vec<Decoration>,
) -> Result<SceneKey, Failure> {
    if !layout.has(NEEDS) {
        return Err(Failure::Unavailable("agent didn't find what local projection reads".into()));
    }
    let client = read::client(memory, layout).ok_or_else(|| unavailable("client"))?;
    let (key, _) = scene_terrain(memory, layout, client, terrain, decorations)?;
    Ok(key)
}

fn unavailable(what: &str) -> Failure {
    Failure::Unavailable(format!("couldn't read the {what}"))
}

// Reads the scene's key, and its terrain when the client shows another scene than `terrain` holds.
fn scene_terrain(
    memory: &impl Memory,
    layout: &Layout,
    client: u64,
    terrain: &mut Option<Terrain>,
    decorations: impl FnOnce() -> Vec<Decoration>,
) -> Result<(SceneKey, [u8; 16]), Failure> {
    let (key, trig) = scene_and_trig(memory, layout, client).ok_or_else(|| unavailable("scene"))?;
    if terrain.as_ref().is_none_or(|t| t.key() != key) {
        let mut fresh = Terrain::read(memory, layout, key).ok_or_else(|| unavailable("terrain"))?;
        fresh.set_decorations(decorations());
        *terrain = Some(fresh);
    }
    Ok((key, trig))
}

/// Projects `points` with the client's camera, first moved by `shift` as it follows the player: fine x and y, and the plane the
/// player will stand on (negative: this one), since the camera rises or sinks with the ground there.
pub fn project(
    memory: &impl Memory,
    layout: &Layout,
    terrain: &mut Option<Terrain>,
    points: &[Point],
    shift: Option<[i32; 3]>,
    clip: bool,
    decorations: impl FnOnce() -> Vec<Decoration>,
) -> Result<Vec<[f32; 2]>, Failure> {
    if !layout.has(NEEDS) {
        return Err(Failure::Unavailable("agent didn't find what local projection reads".into()));
    }
    let client = read::client(memory, layout).ok_or_else(|| unavailable("client"))?;
    let (key, trig) = scene_terrain(memory, layout, client, terrain, decorations)?;
    let mut camera = Camera::read_with(memory, layout, client, trig).ok_or_else(|| unavailable("camera"))?;
    let terrain = terrain.as_mut().unwrap();
    if let Some([dx, dy, plane]) = shift {
        let [fx, _, fy] = i32s::<3>(memory, client + layout.camera.focal as u64).ok_or_else(|| unavailable("camera's focal point"))?;
        let from = terrain.ground_on(memory, layout, fx, fy, key.plane, None);
        let to = terrain.ground_on(memory, layout, fx.wrapping_add(dx), fy.wrapping_add(dy), if plane < 0 { key.plane } else { plane }, None);
        let [x, z, y] = camera.position;
        camera.position = [x.wrapping_add(dx), z.wrapping_add(to.wrapping_sub(from)), y.wrapping_add(dy)];
    }
    Ok(points
        .iter()
        .map(|p| {
            let ground = if p.heightmap { terrain.flat_ground(p.x, p.y, key.plane) } else { terrain.ground_on(memory, layout, p.x, p.y, key.plane, p.tile) };
            let height = ground.wrapping_add(p.z);
            // a clipped point has no position, which no on-screen test passes
            if clip { camera.project_near(p.x, p.y, height).unwrap_or([f32::NAN; 2]) } else { camera.project(p.x, p.y, height) }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;
    use ipc::state::ProjectionLayout;

    const CLIENT: u64 = 0x1000;
    const WORLD_VIEW: u64 = 0x2000;
    const SCENE: u64 = 0x3000;
    const HEIGHTS: u64 = 0x10_0000;
    const SETTINGS: u64 = 0x20_0000;
    const TILES: u64 = 0x30_0000;
    const TILE: u64 = 0x40_0000;
    const MODEL: u64 = 0x50_0000;
    const CANVAS: u64 = 0x6000;
    const SIZE: i32 = 4;

    fn layout() -> Layout {
        let mut layout = Layout { found: NEEDS | part::GAME_STATE, module_base: 0x10, client: 0x20, game_state: 0x8, ..Layout::default() };
        layout.camera.position = 0x100;
        layout.interfaces.canvas = 0x200;
        layout.interfaces.ui_scale = 0x68;
        layout.projection = ProjectionLayout {
            top_world_view: 0x300,
            plane: 0x18,
            scene: 0x10,
            size_x: 0x948,
            size_y: 0x94c,
            tiles: 0x968,
            zoom: 0x400,
            view_holder: 0x1b0,
            view: 0x8,
            view_center: 0x14,
            view_offset_x: 0x58,
            view_offset_y: 0x50,
            trig: 0x418,
            canvas_size: 0x30,
            ui_size: 0x6c,
            heights: 0x3d0,
            settings: 0x400,
            tile_x: 0x20,
            tile_model: 0x130,
            tile_paint: 0x128,
            ground_decoration: 0x118,
            model_vertices: 0x8,
            model_triangles: 0x50,
            decoration_height: 0x134,
        };
        layout
    }

    fn put_f32(memory: &mut Fake, address: u64, value: f32) {
        memory.put(address, &value.to_le_bytes());
    }

    fn world() -> (Fake, Layout) {
        let layout = layout();
        let p = layout.projection;
        let mut m = Fake::default();
        m.put_u64(0x10 + 0x20, CLIENT);
        m.put_u64(CLIENT + p.top_world_view as u64, WORLD_VIEW);
        m.put_u64(WORLD_VIEW + p.scene as u64, SCENE);
        m.put_i32(WORLD_VIEW + p.plane as u64, 0);
        m.put_i32(SCENE + p.size_x as u64, SIZE);
        m.put_i32(SCENE + p.size_y as u64, SIZE);
        m.put_u64(SCENE + p.heights as u64, HEIGHTS);
        m.put_u64(SCENE + p.settings as u64, SETTINGS);
        m.put_u64(SCENE + p.tiles as u64, TILES);
        for plane in 0..4 {
            for x in 0..=SIZE {
                for y in 0..=SIZE {
                    let index = (plane * (SIZE + 1) + x) * (SIZE + 1) + y;
                    m.put_i32(HEIGHTS + index as u64 * 4, -(plane * 1000 + x * 10 + y));
                }
            }
        }
        m.put(SETTINGS, &[0; (4 * SIZE * SIZE) as usize]);
        m.put(TILES, &[0; (4 * SIZE * SIZE) as usize * 16]);
        for (i, v) in [1.0f32, 0.0, 0.0, 1.0].into_iter().enumerate() {
            put_f32(&mut m, SCENE + p.trig as u64 + i as u64 * 4, v);
        }
        for (i, v) in [0, -1000, 0].into_iter().enumerate() {
            m.put_i32(CLIENT + 0x100 + i as u64 * 4, v);
        }
        m.put_i32(CLIENT + p.zoom as u64, 500);
        m.put_u64(CLIENT + 0x200, CANVAS);
        m.put_u64(CANVAS + 0x1b0 + 0x8, CANVAS + 0xf8);
        m.put_i32(CANVAS + 0xf8 + 0x14, 400);
        m.put_i32(CANVAS + 0xf8 + 0x18, 300);
        m.put_i32(CANVAS + 0xf8 + 0x58, 4);
        m.put_i32(CANVAS + 0xf8 + 0x50, 2);
        m.put_i32(CANVAS + 0x30, 800);
        m.put_i32(CANVAS + 0x34, 600);
        m.put_i32(CANVAS + 0x6c, 400);
        m.put_i32(CANVAS + 0x70, 300);
        put_f32(&mut m, CANVAS + 0x68, 2.0);
        (m, layout)
    }

    fn put_tile(m: &mut Fake, layout: &Layout, plane: i32, x: i32, y: i32, tile: u64) {
        let index = ((plane * SIZE + x) * SIZE + y) as u64;
        m.put_u64(TILES + index * 16 + 8, tile);
        let p = layout.projection;
        for (i, v) in [x, y, plane].into_iter().enumerate() {
            m.put_i32(tile + p.tile_x as u64 + i as u64 * 4, v);
        }
    }

    fn ground(m: &Fake, layout: &Layout, x: i32, y: i32) -> i32 {
        let key = scene(m, layout, CLIENT).unwrap();
        Terrain::read(m, layout, key).unwrap().ground(m, layout, x, y, 0)
    }

    #[test]
    fn heightmap_between_corners() {
        let (m, layout) = world();
        // tile (1, 2): corners -12 (SW), -22 (SE), -13 (NW), -23 (NE)
        assert_eq!(ground(&m, &layout, 128, 256), -12);
        assert_eq!(ground(&m, &layout, 192, 256), -17);
        assert_eq!(ground(&m, &layout, 192, 320), (((-17 * 64) + (-18 * 64)) >> 7));
        // outside the scene
        assert_eq!(ground(&m, &layout, -1, 0), 0);
        assert_eq!(ground(&m, &layout, SIZE * 128, 0), 0);
    }

    #[test]
    fn bridges_use_the_plane_above() {
        let (mut m, layout) = world();
        let bridge_setting = (SIZE + 1) * SIZE + 2;
        m.put(SETTINGS + bridge_setting as u64, &[BRIDGE]);
        assert_eq!(ground(&m, &layout, 128, 256), -1012);
    }

    #[test]
    fn an_edge_takes_the_ground_of_the_tile_it_outlines() {
        let (mut m, layout) = world();
        // tile (2, 2) is a bridge, so its heights come from plane 1; tile (1, 2) beside it is not
        m.put(SETTINGS + ((SIZE + 2) * SIZE + 2) as u64, &[BRIDGE]);
        let key = scene(&m, &layout, CLIENT).unwrap();
        let mut terrain = Terrain::read(&m, &layout, key).unwrap();
        // the edge x = 256, halfway up: the bridge's ground by default, which is a different plane
        assert_ne!(terrain.ground(&m, &layout, 256, 320, 0), -23);
        // named as tile (1, 2)'s, it is interpolated from that tile's own corners: -22 and -23
        assert_eq!(terrain.ground_on(&m, &layout, 256, 320, 0, Some([1, 2])), -23);
    }

    #[test]
    fn paint_follows_its_own_plane() {
        let (mut m, layout) = world();
        put_tile(&mut m, &layout, 0, 1, 2, TILE);
        m.put_u64(TILE + layout.projection.tile_paint as u64, 0x1);
        // a bridge tile's paint lies on plane 1 even when listed on plane 0
        m.put_i32(TILE + layout.projection.tile_x as u64 + 8, 1);
        assert_eq!(ground(&m, &layout, 128, 256), -1012);
        assert_eq!(ground(&m, &layout, 255, 383), -1022);
    }

    #[test]
    fn models_pick_the_first_triangle_holding_the_point() {
        let (mut m, layout) = world();
        let p = layout.projection;
        put_tile(&mut m, &layout, 0, 0, 0, TILE);
        m.put_u64(TILE + p.tile_model as u64, MODEL);
        let vertices: [Vertex; 4] = [[0, -100, 0], [128, -200, 0], [128, -300, 128], [0, -400, 128]];
        for (i, v) in vertices.iter().flatten().enumerate() {
            m.put_i32(MODEL + 0x1000 + i as u64 * 4, *v);
        }
        m.put_u64(MODEL + p.model_vertices as u64, MODEL + 0x1000);
        for (i, v) in [0, 1, 2, 0, 2, 3].into_iter().enumerate() {
            m.put_i32(MODEL + 0x2000 + i as u64 * 4, v);
        }
        m.put_u64(MODEL + p.model_triangles as u64, MODEL + 0x2000);
        m.put_u64(MODEL + p.model_triangles as u64 + 8, MODEL + 0x2000 + 24);
        let [first, second] = [[vertices[0], vertices[1], vertices[2]], [vertices[0], vertices[2], vertices[3]]];
        assert_eq!(ground(&m, &layout, 0, 0), -100);
        // on the diagonal both hold it: the first wins
        assert_eq!(ground(&m, &layout, 64, 64), -200);
        assert_eq!(ground(&m, &layout, 96, 32), barycentric(96, 32, &first));
        assert_eq!(ground(&m, &layout, 0, 127), barycentric(0, 127, &second));
        assert_ne!(barycentric(0, 127, &second), barycentric(0, 127, &first));
    }

    #[test]
    fn decorations_lower_their_tile() {
        let (mut m, layout) = world();
        put_tile(&mut m, &layout, 0, 1, 2, TILE);
        let key = scene(&m, &layout, CLIENT).unwrap();
        let mut terrain = Terrain::read(&m, &layout, key).unwrap();
        terrain.set_decorations([((0, 1, 2), 30), ((0, 2, 2), 0)]);
        assert_eq!(terrain.ground(&m, &layout, 128, 256, 0), -42);
        assert_eq!(terrain.ground(&m, &layout, 256, 256, 0), -22);
    }

    #[test]
    fn edges_count_as_inside() {
        let t: Triangle = [[0, 0, 0], [128, 0, 0], [0, 0, 128]];
        assert!(inside(0, 0, &t));
        assert!(inside(64, 64, &t));
        assert!(inside(64, 0, &t));
        assert!(!inside(65, 64, &t));
        assert!(!inside(-1, 0, &t));
        assert!(!inside(0, 129, &t));
        let reversed: Triangle = [t[0], t[2], t[1]];
        assert!(inside(64, 64, &reversed));
        assert!(!inside(65, 64, &reversed));
    }

    #[test]
    fn projects_like_the_client() {
        let (m, layout) = world();
        let mut terrain = None;
        let mut asked = 0;
        // straight down from 1000 above (0, 0): across = dx, up = -depth = -dy, distance = dz
        let points = [[0, 0, 0], [100, 0, 0], [0, 200, 0], [128, 256, -88]].map(|[x, y, z]| Point::new(x, y, z));
        let screen = project(&m, &layout, &mut terrain, &points, None, false, || {
            asked += 1;
            Vec::new()
        })
        .unwrap();
        // x: trunc(500 * dx / dz) + 4 + 400, halved to interface units, doubled back to pixels
        assert_eq!(screen[0], [404.0, 302.0]);
        assert_eq!(screen[1], [454.0, 302.0]);
        assert_eq!(screen[2], [404.0, 202.0]);
        // ground -12 and 88 up: 100 up, 900 below the camera
        let x = 500 * 128 / 900 + 404;
        let y = 2 + 300 - 500 * 256 / 900;
        assert_eq!(screen[3], [((x as f32 / 2.0 + 0.5) as i32 * 2) as f32, ((y as f32 / 2.0 + 0.5) as i32 * 2) as f32]);
        project(&m, &layout, &mut terrain, &points, None, false, || {
            asked += 1;
            Vec::new()
        })
        .unwrap();
        assert_eq!(asked, 1, "terrain and decorations are kept while the scene stays");
    }

    #[test]
    fn a_shift_moves_the_camera_with_the_ground_under_its_focal_point() {
        let (mut m, mut layout) = world();
        layout.camera.focal = 0x140;
        for (i, v) in [64, -5, 64].into_iter().enumerate() {
            m.put_i32(CLIENT + 0x140 + i as u64 * 4, v);
        }
        let mut terrain = None;
        let points = [Point::new(300, 200, -40)];
        let shifted = project(&m, &layout, &mut terrain, &points, Some([128, 0, -1]), false, Vec::new).unwrap();
        let t = terrain.as_mut().unwrap();
        let rise = t.ground_on(&m, &layout, 192, 64, 0, None) - t.ground_on(&m, &layout, 64, 64, 0, None);
        assert_eq!(rise, -10);
        let ground = t.ground_on(&m, &layout, 300, 200, 0, None);
        let mut camera = Camera::read(&m, &layout, CLIENT, SCENE).unwrap();
        camera.position = [camera.position[0] + 128, camera.position[1] + rise, camera.position[2]];
        assert_eq!(shifted[0], camera.project(300, 200, ground - 40));
        assert_ne!(shifted, project(&m, &layout, &mut terrain, &points, None, false, Vec::new).unwrap());
    }

    #[test]
    fn behind_the_camera_and_degenerate_values_match_cvttss2si() {
        assert_eq!(truncate(f32::NAN), i32::MIN);
        assert_eq!(truncate(3e9), i32::MIN);
        assert_eq!(truncate(-3e9), i32::MIN);
        assert_eq!(truncate(-2.7), -2);
        let (m, layout) = world();
        let camera = Camera::read(&m, &layout, CLIENT, SCENE).unwrap();
        // at the camera itself: 0 / 0, which the client turns into i32::MIN pixels
        let [x, y] = camera.project(0, 0, -1000);
        assert!(x < -1e9 && y < -1e9);
    }

    #[test]
    fn needs_the_parts() {
        let (m, mut layout) = world();
        layout.found &= !part::PROJECTION;
        assert!(matches!(project(&m, &layout, &mut None, &[Point::new(0, 0, 0)], None, false, Vec::new), Err(Failure::Unavailable(_))));
    }
}
