//! The reads the agent's worker answers, plain functions over a `Memory` so the injector can run them too; `serve` encodes results (postcard).

use ipc::state::{
    Layout, game_state::{LOGGED_IN, LOGIN_SCREEN, LOGIN_SCREEN_AUTHENTICATOR}, part, worker_kind as kind, worker_status as status,
};
use serde::Serialize;

use crate::{
    GameState,
    chat::{self, ChatMessage},
    memory::Memory,
    state,
    widgets::{self, SubInterface, Widget, WidgetTree, find_children, find_tree, find_widget},
    world::{self, SpotAnim, GroundItem, Item, Npc, Player, Projectile},
    world_select::{self, WorldSelect},
};

fn logged_in(memory: &impl Memory, layout: &Layout) -> bool {
    state::current(memory, layout) == Some(LOGGED_IN)
}

pub fn npcs(memory: &impl Memory, layout: &Layout) -> Vec<Npc> {
    if !logged_in(memory, layout) {
        return Vec::new();
    }
    world::npcs(memory, &layout.world).unwrap_or_default()
}

pub fn players(memory: &impl Memory, layout: &Layout) -> Vec<Player> {
    if !logged_in(memory, layout) {
        return Vec::new();
    }
    world::players(memory, &layout.world).unwrap_or_default()
}

pub const PROJECTILE_PARTS: u64 = part::PROJECTILES | part::WORLD;

pub fn projectiles(memory: &impl Memory, layout: &Layout) -> Vec<Projectile> {
    if !logged_in(memory, layout) {
        return Vec::new();
    }
    world::projectiles(memory, layout).unwrap_or_default()
}

pub const SPOT_ANIM_PARTS: u64 = part::SPOT_ANIMS | part::PLAYER;

pub fn spot_anims(memory: &impl Memory, layout: &Layout) -> Vec<SpotAnim> {
    if !logged_in(memory, layout) {
        return Vec::new();
    }
    world::spot_anims(memory, layout).unwrap_or_default()
}

pub const GROUND_ITEM_PARTS: u64 = part::GROUND_ITEMS | part::PROJECTION | part::PLAYER;

/// Ground items without their names, which the client's item definitions give.
pub fn ground_items(memory: &impl Memory, layout: &Layout) -> Vec<GroundItem> {
    if !logged_in(memory, layout) {
        return Vec::new();
    }
    world::ground_items(memory, layout).unwrap_or_default()
}

/// The interface root and the scale its widgets are drawn at; `None` while no interface is up.
fn interface_root(memory: &impl Memory, layout: &Layout) -> Option<(i32, f32)> {
    let root = state::root_interface(memory, layout, state::current(memory, layout))?;
    Some((root, state::ui_scale(memory, layout).unwrap_or(1.0)))
}

pub fn widget(memory: &impl Memory, layout: &Layout, group: u16, child: u16) -> Option<Widget> {
    let (root, scale) = interface_root(memory, layout)?;
    find_widget(memory, &layout.interfaces, root, group, child).map(|w| w.scaled(scale))
}

pub fn widget_children(memory: &impl Memory, layout: &Layout, group: u16, child: u16) -> Option<Vec<Widget>> {
    let (root, scale) = interface_root(memory, layout)?;
    find_children(memory, &layout.interfaces, root, group, child).map(|children| children.into_iter().map(|w| w.scaled(scale)).collect())
}

/// Every open interface with its components, read by the injector since the worker's reply buffer is small.
pub fn widget_tree(memory: &impl Memory, layout: &Layout) -> Option<WidgetTree> {
    let (root, scale) = interface_root(memory, layout)?;
    find_tree(memory, &layout.interfaces, root, scale)
}

pub fn sub_interfaces(memory: &impl Memory, layout: &Layout) -> Option<Vec<SubInterface>> {
    state::root_interface(memory, layout, state::current(memory, layout))?;
    widgets::sub_interfaces(memory, &layout.interfaces)
}

pub fn game_state(memory: &impl Memory, layout: &Layout) -> GameState {
    state::game_state(memory, layout)
}

pub fn item_container(memory: &impl Memory, layout: &Layout, id: i32) -> Option<Vec<Item>> {
    world::item_container(memory, &layout.world, id)
}

pub fn item_containers(memory: &impl Memory, layout: &Layout) -> Option<Vec<(i32, Vec<Item>)>> {
    world::item_containers(memory, &layout.world)
}

pub fn varc(memory: &impl Memory, layout: &Layout, id: i32) -> Option<i32> {
    world::varc(memory, &layout.world, id)
}

/// `now` is the host's clock in ms since the epoch, which the message ages are measured against.
pub fn chat(memory: &impl Memory, layout: &Layout, now: i64) -> Option<Vec<ChatMessage>> {
    chat::messages(memory, layout, now)
}

pub fn world_select(memory: &impl Memory, layout: &Layout) -> Option<WorldSelect> {
    if !matches!(state::current(memory, layout), Some(LOGIN_SCREEN | LOGIN_SCREEN_AUTHENTICATOR | LOGGED_IN)) {
        return None;
    }
    let client = ipc::read::client(memory, layout)?;
    world_select::world_select(memory, layout, client)
}

/// What `kind` needs the agent to have found; the worker answers `NOT_FOUND` without it.
pub fn parts(kind_id: u32) -> Option<u64> {
    Some(match kind_id {
        kind::NPCS | kind::PLAYERS | kind::ITEM_CONTAINER | kind::VARC => part::WORLD,
        kind::GROUND_ITEMS => GROUND_ITEM_PARTS,
        kind::WIDGET | kind::WIDGET_CHILDREN | kind::SUB_INTERFACES => part::INTERFACES,
        kind::GAME_STATE => 0,
        kind::CHAT => part::CHAT,
        kind::WORLD_SELECT => part::WORLD_SELECT,
        kind::PROJECTILES => PROJECTILE_PARTS,
        kind::SPOT_ANIMS => SPOT_ANIM_PARTS,
        _ => return None,
    })
}

fn put<T: Serialize>(value: &T, out: &mut [u8]) -> Result<usize, i32> {
    postcard::to_slice(value, out).map(|used| used.len()).map_err(|_| status::TOO_BIG)
}

/// Runs the read `kind_id` names with `args` and encodes the result into `out`: the length used or a status.
pub fn serve(memory: &impl Memory, layout: &Layout, kind_id: u32, args: [i32; 4], out: &mut [u8]) -> Result<usize, i32> {
    if !parts(kind_id).is_some_and(|needed| layout.has(needed)) {
        return Err(status::NOT_FOUND);
    }
    match kind_id {
        kind::NPCS => put(&npcs(memory, layout), out),
        kind::PLAYERS => put(&players(memory, layout), out),
        kind::GROUND_ITEMS => put(&ground_items(memory, layout), out),
        kind::WIDGET => put(&widget(memory, layout, args[0] as u16, args[1] as u16), out),
        kind::WIDGET_CHILDREN => put(&widget_children(memory, layout, args[0] as u16, args[1] as u16), out),
        kind::SUB_INTERFACES => put(&sub_interfaces(memory, layout), out),
        kind::GAME_STATE => put(&game_state(memory, layout), out),
        kind::ITEM_CONTAINER => put(&item_container(memory, layout, args[0]), out),
        kind::VARC => put(&varc(memory, layout, args[0]), out),
        kind::CHAT => put(&chat(memory, layout, (args[0] as u32 as i64) | ((args[1] as i64) << 32)), out),
        kind::WORLD_SELECT => put(&world_select(memory, layout), out),
        kind::PROJECTILES => put(&projectiles(memory, layout), out),
        kind::SPOT_ANIMS => put(&spot_anims(memory, layout), out),
        _ => Err(status::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;

    #[test]
    fn serve_encodes_what_the_injector_decodes() {
        let memory = Fake::default();
        let layout = Layout::default();
        let mut out = vec![0u8; 4096];
        let length = serve(&memory, &layout, kind::GAME_STATE, [0; 4], &mut out).unwrap();
        let decoded: GameState = postcard::from_bytes(&out[..length]).unwrap();
        assert_eq!(decoded, game_state(&memory, &layout));
    }

    #[test]
    fn serve_reports_what_it_cannot_answer() {
        let memory = Fake::default();
        let layout = Layout::default();
        let mut out = vec![0u8; 1];
        assert_eq!(serve(&memory, &layout, kind::GAME_STATE, [0; 4], &mut out), Err(status::TOO_BIG));
        assert_eq!(serve(&memory, &layout, 9999, [0; 4], &mut out), Err(status::NOT_FOUND));
        // the agent found no interfaces in a default layout
        assert_eq!(serve(&memory, &layout, kind::WIDGET, [1, 2, 0, 0], &mut out), Err(status::NOT_FOUND));
    }
}
