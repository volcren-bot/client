use std::sync::OnceLock;

use ipc::{
    read::{self, Memory},
    state::{
        game_state, model_status, model_target, project_status, ModelReply, ModelRequest,
        MODEL_HULL,
    },
};

use super::{
    detour::function,
    items::release,
    offsets::{u16_at, u32_at, ActorOrientation, ModelFaces, ModelVertices, NpcModel, TypeModels},
    own::{on_other_thread, OwnMemory, LAYOUT},
    projection::Scene,
    state::mapping,
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
static TYPES: OnceLock<TypeModels> = OnceLock::new();

type Lookup = unsafe extern "system" fn(*mut [u64; 2], i32) -> *mut [u64; 2];
type NpcGetModel =
    unsafe extern "system" fn(u64, *mut [u64; 2], u64, i32, u64, i32, u64) -> *mut [u64; 2];
type IsSkeletal = unsafe extern "system" fn(u64) -> u8;

const MAX_TYPE_ID: i32 = 0x10000;

pub fn set_type_models(types: TypeModels) {
    let _ = TYPES.set(types);
}

const MAX_VERTICES: usize = 20_000;
const MAX_FACES: usize = 40_000;
const HIDDEN_FACE: i32 = -2;
const MIN_EXTREME: i32 = 32;
const BOUNDING_BOX_MARGIN: i32 = 8;
const ANGLE_STEP: f64 = 0.0030679615;

fn section(image: &[u8], name: &[u8]) -> Option<std::ops::Range<usize>> {
    let nt = u32_at(image, 0x3c)? as usize;
    let count = u16_at(image, nt + 6)? as usize;
    let table = nt + 24 + u16_at(image, nt + 20)? as usize;
    (0..count).find_map(|i| {
        let header = table + i * 40;
        let section_name = image.get(header..header + 8)?;
        if !section_name.starts_with(name) {
            return None;
        }
        let (size, rva) = (
            u32_at(image, header + 8)? as usize,
            u32_at(image, header + 12)? as usize,
        );
        Some(rva..(rva + size).min(image.len()) / 8 * 8)
    })
}

fn animation_slot(
    image: &[u8],
    module_base: u64,
    get_model: u32,
    animation_height: u32,
) -> Option<u32> {
    let (get_model, height) = (
        module_base + get_model as u64,
        module_base + animation_height as u64,
    );
    let rdata = section(image, b".rdata")?;
    let qwords: Vec<u64> = image
        .get(rdata)?
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_le_bytes(*c))
        .collect();
    qwords
        .iter()
        .enumerate()
        .filter(|&(_, &q)| q == get_model)
        .find_map(|(at, _)| {
            qwords[at..(at + 64).min(qwords.len())]
                .iter()
                .position(|&q| q == height)
                .map(|slot| slot as u32 * 8)
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
    let _ = RULES.set(Rules {
        module_base,
        animation_slot,
        npc,
        vertices,
        faces,
        orientation,
    });
}

fn sin_cos(orientation: i32) -> (i32, i32) {
    let angle = (orientation & 2047) as f64 * ANGLE_STEP;
    (
        (65536.0 * angle.sin()) as i32,
        (65536.0 * angle.cos()) as i32,
    )
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
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f32; 2]> = Vec::with_capacity(points.len() * 2);
    let n = points.len();
    for pass in 0..2 {
        let start = hull.len();
        for i in 0..n {
            let p = if pass == 0 {
                points[i]
            } else {
                points[n - 1 - i]
            };
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
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

fn count(memory: &OwnMemory, at: u64, max: usize) -> Option<usize> {
    let count = memory.i32(at)?;
    (count > 0 && count as usize <= max).then_some(count as usize)
}

fn array(memory: &OwnMemory, model: u64, offset: u32, count: usize) -> Option<Vec<i32>> {
    memory
        .ptr(model + offset as u64)
        .and_then(|p| memory.i32s(p, count))
}

fn read_vertices(memory: &OwnMemory, model: u64, layout: &ModelVertices) -> Option<Vec<[i32; 3]>> {
    let count = count(memory, model + layout.count as u64, MAX_VERTICES)?;
    let field = |offset| array(memory, model, offset, count);
    let (xs, ys, zs) = (field(layout.x)?, field(layout.y)?, field(layout.z)?);
    Some((0..count).map(|i| [xs[i], ys[i], zs[i]]).collect())
}

fn read_faces(
    memory: &OwnMemory,
    model: u64,
    layout: &ModelFaces,
    vertex_count: usize,
) -> Option<Vec<[usize; 3]>> {
    let count = count(memory, model + layout.count as u64, MAX_FACES)?;
    let field = |offset| array(memory, model, offset, count);
    let (a, b, c) = (field(layout.a)?, field(layout.b)?, field(layout.c)?);
    let colors = field(layout.color);
    Some(
        (0..count)
            .filter(|&i| {
                colors
                    .as_ref()
                    .is_none_or(|colors| colors[i] != HIDDEN_FACE)
            })
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
    let bounding_box = memory
        .u64(model + rules.npc.bounding_box as u64)
        .is_some_and(|v| v as u8 != 0);
    let (sin, cos) = sin_cos(at.orientation);
    let turned: Vec<[i32; 3]> = vertices.iter().map(|&v| rotate(v, sin, cos)).collect();
    let (x, y, height) = (at.x, at.y, at.height);

    let (center, extreme) = self::bounding_box(&turned, bounding_box);
    let corners: [Option<[f32; 2]>; 8] = std::array::from_fn(|i| {
        let sign = |bit: i32| if i as i32 & bit == 0 { -1 } else { 1 };
        scene.project(
            x + center[0] + sign(1) * extreme[0],
            y + center[2] + sign(4) * extreme[2],
            height + center[1] + sign(2) * extreme[1],
        )
    });
    let corners = corners
        .iter()
        .all(Option::is_some)
        .then(|| corners.map(|corner| corner.unwrap_or_default()));

    let screen: Vec<Option<[f32; 2]>> = turned
        .iter()
        .map(|v| scene.project(x + v[0], y + v[2], height + v[1]))
        .collect();
    let faces = read_faces(&memory, model, &rules.faces, vertices.len())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|[a, b, c]| Some([screen[a]?, screen[b]?, screen[c]?]))
        .collect();
    Some(Projected {
        bounding_box,
        corners,
        points: screen.into_iter().flatten().collect(),
        faces,
        vertex_count: vertices.len(),
    })
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
    let hull = thin(
        convex_hull(
            models
                .iter()
                .flat_map(|m| m.points.iter().copied())
                .collect(),
        ),
        MODEL_HULL,
    );
    reply.hull_count = hull.len() as u32;
    reply.hull[..hull.len()].copy_from_slice(&hull);
    let faces = models.iter().flat_map(|m| m.faces.iter());
    for (slot, face) in reply.faces.iter_mut().zip(faces) {
        *slot = *face;
    }
    reply.face_count = models.iter().map(|m| m.faces.len() as u32).sum();
    reply.vertex_count = models.iter().map(|m| m.vertex_count as u32).sum();
}

type Status = Result<(), i32>;

fn scene() -> Result<Scene, i32> {
    Scene::current().map_err(|status| match status {
        project_status::NOT_IN_GAME => model_status::NOT_IN_GAME,
        project_status::OTHER_THREAD => model_status::OTHER_THREAD,
        _ => model_status::NOT_FOUND,
    })
}

fn rules() -> Result<&'static Rules, i32> {
    RULES.get().ok_or(model_status::NOT_FOUND)
}

struct Built {
    model: u64,
    control: u64,
    lift: i32,
}

unsafe fn build(rules: &Rules, renderable: u64) -> Option<Built> {
    let memory = OwnMemory;
    let vtable = memory.ptr(renderable)?;
    let bounds = memory
        .u64(vtable + rules.vertices.bounds_slot as u64)
        .unwrap_or(0);
    if rules
        .vertices
        .bounds
        .iter()
        .any(|&b| b != 0 && bounds == rules.module_base + b as u64)
    {
        return Some(Built {
            model: renderable,
            control: 0,
            lift: 0,
        });
    }
    let get_model: GetModel = function(memory.ptr(vtable)?);
    let mut model = [0u64; 2];
    get_model(renderable, &mut model, 0);
    if model[1] == 0 {
        release(model[0]);
        return None;
    }
    let lift = rules
        .animation_slot
        .and_then(|slot| memory.ptr(vtable + slot as u64))
        .map_or(0, |height| function::<AnimationHeight>(height)(renderable));
    Some(Built {
        model: model[1],
        control: model[0],
        lift,
    })
}

fn project_renderables(
    reply: &mut ModelReply,
    rules: &Rules,
    scene: &Scene,
    renderables: &[(u64, Placement)],
) -> Status {
    let mut models = Vec::new();
    for &(renderable, at) in renderables {
        let Some(built) = (unsafe { build(rules, renderable) }) else {
            continue;
        };
        let at = Placement {
            height: at.height - built.lift,
            ..at
        };
        models.extend(project(scene, rules, built.model, &at));
        unsafe { release(built.control) };
    }
    if models.is_empty() {
        return Err(model_status::NO_MODEL);
    }
    write(reply, &models, renderables[0].1.orientation);
    Ok(())
}

fn actor_request(reply: &mut ModelReply, target: u32, index: i32) -> Status {
    let memory = OwnMemory;
    let layout = LAYOUT.get().ok_or(model_status::NOT_FOUND)?;
    let rules = rules()?;
    let (view, _) = read::local_player(&memory, layout).ok_or(model_status::NOT_IN_GAME)?;
    let table = if target == model_target::PLAYER {
        layout.player.players
    } else {
        layout.world.npc_table
    };
    let actor = read::table_get(&memory, view + table as u64, index).ok_or(model_status::NO_NPC)?;
    let local = layout.world.local_x as u64;
    let (Some(x), Some(y), Some(orientation)) = (
        memory.i32(actor + local),
        memory.i32(actor + local + 4),
        memory.i32(actor + rules.orientation.current as u64),
    ) else {
        return Err(model_status::NO_NPC);
    };
    let scene = scene()?;
    let height = scene.ground(x, y);
    project_renderables(
        reply,
        rules,
        &scene,
        &[(
            actor,
            Placement {
                x,
                y,
                height,
                orientation,
            },
        )],
    )
}

struct Pair([u64; 2]);

impl Pair {
    unsafe fn lookup(rva: u32, id: i32) -> Option<Self> {
        let lookup: Lookup = function(RULES.get()?.module_base + rva as u64);
        let mut pair = Pair([0; 2]);
        lookup(&mut pair.0, id);
        (pair.0[1] != 0).then_some(pair)
    }

    fn object(&self) -> u64 {
        self.0[1]
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        unsafe { release(self.0[0]) };
    }
}

unsafe fn checked_frames(types: &TypeModels, rules: &Rules, seq: &Pair, frame: i32) -> Option<u32> {
    let memory = OwnMemory;
    let skeletal: IsSkeletal = function(rules.module_base + types.seq_skeletal as u64);
    if skeletal(seq.object()) != 0 {
        return None;
    }
    let count = memory.i32(seq.object() + types.seq_frame_count as u64)?;
    let frames = memory.ptr(seq.object() + types.seq_frames as u64)?;
    (count > 0 && frames != 0 && frame >= 0 && frame < count).then_some(count as u32)
}

fn npc_type_request(reply: &mut ModelReply, request: &ModelRequest) -> Status {
    let types = TYPES.get().ok_or(model_status::NOT_FOUND)?;
    let rules = rules()?;
    if !(0..MAX_TYPE_ID).contains(&request.type_id) {
        return Err(model_status::NO_NPC);
    }
    let scene = scene()?;
    let npc =
        unsafe { Pair::lookup(types.npc_lookup, request.type_id) }.ok_or(model_status::NO_NPC)?;
    let mut frame_count = 0;
    let seq = if request.animation >= 0 {
        let seq = (request.animation < MAX_TYPE_ID)
            .then(|| unsafe { Pair::lookup(types.seq_lookup, request.animation) })
            .flatten()
            .ok_or(model_status::BAD_ANIMATION)?;
        frame_count = unsafe { checked_frames(types, rules, &seq, request.frame) }
            .ok_or(model_status::BAD_ANIMATION)?;
        Some(seq)
    } else {
        None
    };
    let get_model: NpcGetModel =
        unsafe { function(rules.module_base + types.npc_get_model as u64) };
    let mut model = Pair([0; 2]);
    unsafe {
        get_model(
            npc.object(),
            &mut model.0,
            seq.as_ref().map_or(0, Pair::object),
            if seq.is_some() { request.frame } else { -1 },
            0,
            -1,
            0,
        )
    };
    if model.object() == 0 {
        return Err(model_status::NO_MODEL);
    }
    let (x, y) = (request.fine_x, request.fine_y);
    let at = Placement {
        x,
        y,
        height: scene.ground(x, y),
        orientation: request.orientation,
    };
    let projected = project(&scene, rules, model.object(), &at).ok_or(model_status::NO_MODEL)?;
    write(reply, &[projected], request.orientation);
    reply.frame_count = frame_count;
    Ok(())
}

fn loc_request(reply: &mut ModelReply, request: &ModelRequest) -> Status {
    match super::scene::loc_renderables(
        request.loc_kind as u8,
        request.loc_x,
        request.loc_y,
        request.loc_id,
    ) {
        Some(renderables) if !renderables.is_empty() => {
            project_renderables(reply, rules()?, &scene()?, &renderables)
        }
        Some(_) => Err(model_status::NO_MODEL),
        None => Err(model_status::NO_NPC),
    }
}

pub fn on_request() {
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_model_request() else {
        return;
    };
    let memory = OwnMemory;
    shared.write_model_reply_with(|reply| {
        reply.sequence = sequence;
        reply.corner_count = 0;
        reply.hull_count = 0;
        reply.face_count = 0;
        reply.frame_count = 0;
        let status = match LAYOUT.get().filter(|_| RULES.get().is_some()) {
            None => Err(model_status::NOT_FOUND),
            Some(_) if on_other_thread() => Err(model_status::OTHER_THREAD),
            Some(layout)
                if read::current_game_state(&memory, layout) != Some(game_state::LOGGED_IN) =>
            {
                Err(model_status::NOT_IN_GAME)
            }
            _ => match request.target {
                model_target::LOC => loc_request(reply, &request),
                model_target::NPC_TYPE => npc_type_request(reply, &request),
                target => actor_request(reply, target, request.index),
            },
        };
        reply.status = status.map_or_else(|status| status, |()| model_status::OK);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_drops_inner_points() {
        let hull = convex_hull(vec![
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 10.0],
            [0.0, 10.0],
            [5.0, 5.0],
            [5.0, 0.0],
        ]);
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
