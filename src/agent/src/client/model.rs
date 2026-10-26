use std::sync::Arc;
use std::sync::{
    atomic::{AtomicI32, Ordering},
    OnceLock,
};

use ipc::{
    read::{self, Memory},
    state::{game_state, model_status, model_target, project_status, ModelReply, ModelRequest, MODEL_FACES, MODEL_HULL},
};

use super::{
    offsets::{ActorOrientation, ModelFaces, ModelVertices, NpcModel},
    own::{on_other_thread, OwnMemory, LAYOUT},
    projection::Scene,
    state::{Mapping, LAYOUT_MAP},
};

type GetModel = unsafe extern "system" fn(u64, *mut [u64; 2], i32) -> *mut [u64; 2];
type AnimationHeight = unsafe extern "system" fn(u64) -> i32;

#[derive(Clone, Copy)]
struct Rules {
    module_base: u64,
    animation_slot: Option<u32>,
    npc: NpcModel,
    vertices: ModelVertices,
    faces: ModelFaces,
    orientation: ActorOrientation,
}

static RULES: OnceLock<Rules> = OnceLock::new();

const MAX_VERTICES: usize = 20_000;
const MAX_FACES: usize = 40_000;
const HIDDEN_FACE: i32 = -2;
const MIN_EXTREME: i32 = 32;
const BOUNDING_BOX_MARGIN: i32 = 8;
const ANGLE_STEP: f64 = 0.0030679615;

fn section(image: &[u8], name: &[u8]) -> Option<std::ops::Range<usize>> {
    let u16_at = |at: usize| image.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| image.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let nt = u32_at(0x3c)? as usize;
    let count = u16_at(nt + 6)? as usize;
    let table = nt + 24 + u16_at(nt + 20)? as usize;
    (0..count).find_map(|i| {
        let header = table + i * 40;
        let section_name = image.get(header..header + 8)?;
        if !section_name.starts_with(name) {
            return None;
        }
        let (size, rva) = (u32_at(header + 8)? as usize, u32_at(header + 12)? as usize);
        Some(rva..(rva + size).min(image.len()) / 8 * 8)
    })
}

fn animation_slot(image: &[u8], module_base: u64, get_model: u32, animation_height: u32) -> Option<u32> {
    let (get_model, height) = (module_base + get_model as u64, module_base + animation_height as u64);
    let rdata = section(image, b".rdata")?;
    let qwords: Vec<u64> = image.get(rdata)?.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
    qwords.iter().enumerate().filter(|&(_, &q)| q == get_model).find_map(|(at, _)| {
        qwords[at..(at + 64).min(qwords.len())].iter().position(|&q| q == height).map(|slot| slot as u32 * 8)
    })
}

pub unsafe fn set_npc_model(
    module_base: u64,
    image: &[u8],
    npc: NpcModel,
    vertices: ModelVertices,
    faces: ModelFaces,
    orientation: ActorOrientation,
    animation_height: u32,
) {
    let animation_slot = animation_slot(image, module_base, npc.get_model, animation_height);
    println!("[Agent]: Renderable animation height slot: {animation_slot:x?}.\r\n");
    let _ = RULES.set(Rules { module_base, animation_slot, npc, vertices, faces, orientation });
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

unsafe fn release(control: u64) {
    if control == 0 {
        return;
    }
    let vtable = *(control as *const u64);
    let uses = &*((control + 8) as *const AtomicI32);
    if uses.fetch_sub(1, Ordering::AcqRel) == 1 {
        let destroy: unsafe extern "system" fn(u64) = std::mem::transmute(*((vtable + 8) as *const u64));
        destroy(control);
        let weaks = &*((control + 0xc) as *const AtomicI32);
        if weaks.fetch_sub(1, Ordering::AcqRel) == 1 {
            let delete: unsafe extern "system" fn(u64) = std::mem::transmute(*((vtable + 16) as *const u64));
            delete(control);
        }
    }
}

fn sin_cos(orientation: i32) -> (i32, i32) {
    let angle = (orientation & 2047) as f64 * ANGLE_STEP;
    ((65536.0 * angle.sin()) as i32, (65536.0 * angle.cos()) as i32)
}

fn rotate([x, y, z]: [i32; 3], sin: i32, cos: i32) -> [i32; 3] {
    let rx = z.wrapping_mul(sin).wrapping_add(x.wrapping_mul(cos)) >> 16;
    let rz = z.wrapping_mul(cos).wrapping_sub(x.wrapping_mul(sin)) >> 16;
    [rx, y, rz]
}

fn bounding_box(vertices: &[[i32; 3]], bounding_box: bool) -> ([i32; 3], [i32; 3]) {
    let mut min = [0i32; 3];
    let mut max = [0i32; 3];
    for v in vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(v[axis]);
            max[axis] = max[axis].max(v[axis]);
        }
    }
    let center = [0, 1, 2].map(|a| (max[a] + min[a]) / 2);
    let mut extreme = [0, 1, 2].map(|a| (max[a] - min[a] + 1) / 2);
    for axis in [0, 2] {
        extreme[axis] = extreme[axis].max(MIN_EXTREME);
        if bounding_box {
            extreme[axis] += BOUNDING_BOX_MARGIN;
        }
    }
    (center, extreme)
}

fn convex_hull(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut hull: Vec<[f32; 2]> = Vec::with_capacity(points.len() * 2);
    let n = points.len();
    for pass in 0..2 {
        let start = hull.len();
        for i in 0..n {
            let p = if pass == 0 { points[i] } else { points[n - 1 - i] };
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

fn thin(hull: Vec<[f32; 2]>, limit: usize) -> Vec<[f32; 2]> {
    if hull.len() <= limit {
        return hull;
    }
    (0..limit).map(|i| hull[i * hull.len() / limit]).collect()
}

fn read_vertices(memory: &OwnMemory, model: u64, layout: &ModelVertices) -> Option<Vec<[i32; 3]>> {
    let count = memory.i32(model + layout.count as u64)?;
    if count <= 0 || count as usize > MAX_VERTICES {
        return None;
    }
    let axis = |offset: u32| memory.ptr(model + offset as u64).filter(|&p| p != 0).and_then(|p| memory.i32s(p, count as usize));
    let (xs, ys, zs) = (axis(layout.x)?, axis(layout.y)?, axis(layout.z)?);
    Some((0..count as usize).map(|i| [xs[i], ys[i], zs[i]]).collect())
}

fn read_faces(memory: &OwnMemory, model: u64, layout: &ModelFaces, vertex_count: usize) -> Option<Vec<[usize; 3]>> {
    let count = memory.i32(model + layout.count as u64)?;
    if count <= 0 || count as usize > MAX_FACES {
        return None;
    }
    let array = |offset: u32| memory.ptr(model + offset as u64).filter(|&p| p != 0).and_then(|p| memory.i32s(p, count as usize));
    let (a, b, c) = (array(layout.a)?, array(layout.b)?, array(layout.c)?);
    let colors = array(layout.color);
    Some(
        (0..count as usize)
            .filter(|&i| colors.as_ref().is_none_or(|colors| colors[i] != HIDDEN_FACE))
            .map(|i| [a[i] as usize, b[i] as usize, c[i] as usize])
            .filter(|f| f.iter().all(|&v| v < vertex_count))
            .collect(),
    )
}

#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub height: i32,
    pub orientation: i32,
}

struct Projected {
    bounding_box: bool,
    corners: Option<[[f32; 2]; 8]>,
    points: Vec<[f32; 2]>,
    faces: Vec<[[f32; 2]; 3]>,
    vertex_count: usize,
}

fn project(scene: &Scene, rules: &Rules, model: u64, at: &Placement) -> Option<Projected> {
    let memory = OwnMemory;
    let vertices = read_vertices(&memory, model, &rules.vertices)?;
    let bounding_box = memory.u64(model + rules.npc.bounding_box as u64).is_some_and(|v| v as u8 != 0);
    let (sin, cos) = sin_cos(at.orientation);
    let turned: Vec<[i32; 3]> = vertices.iter().map(|&v| rotate(v, sin, cos)).collect();
    let (x, y, height) = (at.x, at.y, at.height);

    let (center, extreme) = self::bounding_box(&turned, bounding_box);
    let corners: [Option<[f32; 2]>; 8] = std::array::from_fn(|i| {
        let sign = |bit: i32| if i as i32 & bit == 0 { -1 } else { 1 };
        scene.project(x + center[0] + sign(1) * extreme[0], y + center[2] + sign(4) * extreme[2], height + center[1] + sign(2) * extreme[1])
    });
    let corners = corners.iter().all(Option::is_some).then(|| corners.map(|corner| corner.unwrap_or_default()));

    let screen: Vec<Option<[f32; 2]>> = turned.iter().map(|v| scene.project(x + v[0], y + v[2], height + v[1])).collect();
    // box-picked models send their faces too: any point on them is inside the box
    let mut faces = Vec::new();
    for face in read_faces(&memory, model, &rules.faces, vertices.len()).unwrap_or_default() {
        let (Some(a), Some(b), Some(c)) = (screen[face[0]], screen[face[1]], screen[face[2]]) else { continue };
        faces.push([a, b, c]);
    }
    Some(Projected { bounding_box, corners, points: screen.into_iter().flatten().collect(), faces, vertex_count: vertices.len() })
}

fn write(reply: &mut ModelReply, models: &[Projected], orientation: i32) {
    let Some(first) = models.first() else { return };
    reply.bounding_box = first.bounding_box as u32;
    reply.orientation = orientation & 2047;
    reply.corner_count = 0;
    if let Some(corners) = first.corners {
        reply.corner_count = 8;
        reply.corners = corners;
    }
    let hull = thin(convex_hull(models.iter().flat_map(|m| m.points.iter().copied()).collect()), MODEL_HULL);
    reply.hull_count = hull.len() as u32;
    reply.hull[..hull.len()].copy_from_slice(&hull);
    let mut count = 0;
    for face in models.iter().flat_map(|m| m.faces.iter()) {
        if count < MODEL_FACES {
            reply.faces[count] = *face;
        }
        count += 1;
    }
    reply.face_count = count as u32;
    reply.vertex_count = models.iter().map(|m| m.vertex_count as u32).sum();
}

fn scene_status(status: i32) -> i32 {
    match status {
        project_status::NOT_IN_GAME => model_status::NOT_IN_GAME,
        project_status::OTHER_THREAD => model_status::OTHER_THREAD,
        _ => model_status::NOT_FOUND,
    }
}

struct Built {
    model: u64,
    control: u64,
    lift: i32,
}

unsafe fn build(rules: &Rules, renderable: u64) -> Option<Built> {
    let memory = OwnMemory;
    let vtable = memory.ptr(renderable).filter(|&v| v != 0)?;
    let bounds = memory.u64(vtable + rules.vertices.bounds_slot as u64).unwrap_or(0);
    if rules.vertices.bounds.iter().any(|&b| b != 0 && bounds == rules.module_base + b as u64) {
        return Some(Built { model: renderable, control: 0, lift: 0 });
    }
    let get_model: GetModel = std::mem::transmute(memory.ptr(vtable).filter(|&f| f != 0)? as *const ());
    let mut model = [0u64; 2];
    get_model(renderable, &mut model, 0);
    if model[1] == 0 {
        release(model[0]);
        return None;
    }
    let lift = match rules.animation_slot {
        Some(slot) => match memory.ptr(vtable + slot as u64).filter(|&f| f != 0) {
            Some(f) => {
                let height: AnimationHeight = std::mem::transmute(f as *const ());
                height(renderable)
            }
            None => 0,
        },
        None => 0,
    };
    Some(Built { model: model[1], control: model[0], lift })
}

fn project_renderables(reply: &mut ModelReply, renderables: &[(u64, Placement)]) -> i32 {
    let Some(rules) = RULES.get() else { return model_status::NOT_FOUND };
    let scene = match Scene::current() {
        Ok(scene) => scene,
        Err(status) => return scene_status(status),
    };
    let mut models = Vec::new();
    for &(renderable, at) in renderables {
        let Some(built) = (unsafe { build(rules, renderable) }) else { continue };
        let at = Placement { height: at.height - built.lift, ..at };
        models.extend(project(&scene, rules, built.model, &at));
        unsafe { release(built.control) };
    }
    if models.is_empty() {
        return model_status::NO_MODEL;
    }
    write(reply, &models, renderables[0].1.orientation);
    model_status::OK
}

fn actor_request(reply: &mut ModelReply, target: u32, index: i32) -> i32 {
    let memory = OwnMemory;
    let Some(layout) = LAYOUT.get() else { return model_status::NOT_FOUND };
    let Some(rules) = RULES.get() else { return model_status::NOT_FOUND };
    let Some((view, _)) = read::local_player(&memory, layout) else { return model_status::NOT_IN_GAME };
    let table = if target == model_target::PLAYER { layout.player.players } else { layout.world.npc_table };
    let Some(actor) = read::table_get(&memory, view + table as u64, index) else {
        return model_status::NO_NPC;
    };
    let local = layout.world.local_x as u64;
    let (Some(x), Some(y), Some(orientation)) =
        (memory.i32(actor + local), memory.i32(actor + local + 4), memory.i32(actor + rules.orientation.current as u64))
    else {
        return model_status::NO_NPC;
    };
    let height = match Scene::current() {
        Ok(scene) => scene.ground(x, y),
        Err(status) => return scene_status(status),
    };
    project_renderables(reply, &[(actor, Placement { x, y, height, orientation })])
}

fn loc_request(reply: &mut ModelReply, request: &ModelRequest) -> i32 {
    match super::scene::loc_renderables(request.loc_kind as u8, request.loc_x, request.loc_y, request.loc_id) {
        Some(renderables) if !renderables.is_empty() => project_renderables(reply, &renderables),
        Some(_) => model_status::NO_MODEL,
        None => model_status::NO_NPC,
    }
}

pub fn on_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_model_request() else { return };
    let memory = OwnMemory;
    shared.write_model_reply_with(|reply| {
        reply.sequence = sequence;
        reply.corner_count = 0;
        reply.hull_count = 0;
        reply.face_count = 0;
        reply.status = if RULES.get().is_none() || LAYOUT.get().is_none() {
            model_status::NOT_FOUND
        } else if on_other_thread() {
            model_status::OTHER_THREAD
        } else if LAYOUT.get().and_then(|layout| read::current_game_state(&memory, layout)) != Some(game_state::LOGGED_IN) {
            model_status::NOT_IN_GAME
        } else if request.target == model_target::LOC {
            loc_request(reply, &request)
        } else {
            actor_request(reply, request.target, request.index)
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_drops_inner_points() {
        let hull = convex_hull(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [5.0, 5.0], [5.0, 0.0]]);
        assert_eq!(hull.len(), 4);
        assert!(!hull.contains(&[5.0, 5.0]));
    }

    #[test]
    fn box_matches_java_rounding_and_minimum() {
        let (center, extreme) = bounding_box(&[[-10, -200, 4], [12, 3, 20]], false);
        assert_eq!(center, [1, -98, 10]);
        assert_eq!(extreme, [32, 102, 32]);
        let (_, boxed) = bounding_box(&[[-10, -200, 4], [12, 3, 20]], true);
        assert_eq!(boxed, [40, 102, 40]);
    }

    #[test]
    fn rotation_is_a_quarter_turn_at_512() {
        let (sin, cos) = sin_cos(512);
        assert_eq!(rotate([100, -5, 0], sin, cos), [0, -5, -100]);
    }
}
