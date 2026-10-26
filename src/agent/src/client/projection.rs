use std::sync::Arc;
use std::sync::OnceLock;

use ipc::{
    read::{self, Memory},
    state::{game_state, part, project_status, ProjectReply, PROJECT_BATCH},
};

use super::{
    offsets::Projection,
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
};

type WorldToScreen = unsafe extern "system" fn(u64, *mut [f32; 2], *const [i32; 3]) -> *mut [f32; 2];
type TileHeight = unsafe extern "system" fn(u64, u64, i32, i32, i32) -> i32;

static PROJECTION: OnceLock<(WorldToScreen, TileHeight, Projection)> = OnceLock::new();

pub unsafe fn set_projection(module_base: u64, rule: Projection) {
    let world_to_screen: WorldToScreen = std::mem::transmute((module_base + rule.world_to_screen as u64) as *const ());
    let tile_height: TileHeight = std::mem::transmute((module_base + rule.tile_height as u64) as *const ());
    let _ = PROJECTION.set((world_to_screen, tile_height, rule));
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

pub struct Scene {
    world_to_screen: WorldToScreen,
    tile_height: TileHeight,
    world_view: u64,
    plane: i32,
    scale: f32,
}

const OFF_SCREEN: f32 = 100_000.0;

impl Scene {
    pub fn current() -> Result<Self, i32> {
        let memory = OwnMemory;
        let layout = LAYOUT.get().ok_or(project_status::NOT_FOUND)?;
        let &(world_to_screen, tile_height, rule) = PROJECTION.get().ok_or(project_status::NOT_FOUND)?;
        if on_other_thread() {
            return Err(project_status::OTHER_THREAD);
        }
        if read::current_game_state(&memory, layout) != Some(game_state::LOGGED_IN) {
            return Err(project_status::NOT_IN_GAME);
        }
        let interfaces = &layout.interfaces;
        let client = read::client(&memory, layout).ok_or(project_status::NOT_FOUND)?;
        let scale = layout
            .has(part::INTERFACES)
            .then_some(client)
            .and_then(|client| memory.ptr(client + interfaces.canvas as u64))
            .and_then(|canvas| memory.f32(canvas + interfaces.ui_scale as u64))
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .ok_or(project_status::NOT_FOUND)?;
        let world_view = memory.ptr(client + rule.top_world_view as u64).filter(|&wv| wv != 0).ok_or(project_status::NOT_FOUND)?;
        let plane = memory.i32(world_view + rule.plane as u64).ok_or(project_status::NOT_FOUND)?;
        Ok(Self { world_to_screen, tile_height, world_view, plane, scale })
    }

    pub fn ground(&self, x: i32, y: i32) -> i32 {
        unsafe { (self.tile_height)(0, self.world_view, x, y, self.plane) }
    }

    pub fn project_raw(&self, x: i32, y: i32, height: i32) -> [f32; 2] {
        let mut screen = [0f32; 2];
        unsafe { (self.world_to_screen)(0, &mut screen, &[x, height, y]) };
        screen.map(|v| v * self.scale)
    }

    pub fn project(&self, x: i32, y: i32, height: i32) -> Option<[f32; 2]> {
        let screen = self.project_raw(x, y, height);
        screen.iter().all(|v| v.is_finite() && v.abs() < OFF_SCREEN * self.scale).then_some(screen)
    }
}

pub fn on_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_project_request() else { return };
    let mut reply = ProjectReply { sequence, ..ProjectReply::default() };
    let count = request.count as usize;
    reply.status = if count > PROJECT_BATCH {
        project_status::TOO_MANY
    } else {
        match Scene::current() {
            Ok(scene) => {
                reply.count = count as u32;
                for (i, &[x, y, offset]) in request.points[..count].iter().enumerate() {
                    reply.points[i] = scene.project_raw(x, y, scene.ground(x, y) + offset);
                }
                project_status::OK
            }
            Err(status) => status,
        }
    };
    shared.write_project_reply(&reply);
}
