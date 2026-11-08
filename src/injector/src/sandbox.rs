use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use mlua::{
    FromLuaMulti, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib, serde::SerializeOptions,
};
use serde::Serialize;

use crate::{
    Input, MouseButton, Overlay, Rgba,
    async_input::{Pace, Worker},
    input_jobs::{self, Job},
    projection::Via,
    wind_mouse,
};
use ipc::packets::{Filter, filter_add, filter_has};

#[cfg(windows)]
use crate::bank_pin::enter as bank_pin_enter;
#[cfg(windows)]
use crate::{
    ItemWatcher, PacketWatcher, cache_files, cache_locs, chat_messages, client_cycle, connection,
    draw, draw_batch, draw_lines, enum_def, find_path, game_state, game_val_names, ground_items,
    in_instance, item_container, item_names, item_params, item_stackable, landmasses, loc_model,
    loc_shown, locs, map_regions, menu, menu_click, menu_layout, menu_open, movement, nav_plan,
    npc_hitsplats, npc_model, npc_sizes, npc_type_model, npcs, player_hitsplats, player_model,
    player_plane, players, project, project_moved, projectiles, scene_collision, scene_route,
    selected_widget, send_input, send_input_with, server_tile, spot_anims, sub_interfaces, varbit,
    varc, varp_value, varps, wait_watchers, widget, widget_children, widget_model, world_route,
    world_select,
};
#[cfg(not(windows))]
use offline::*;

#[cfg(not(windows))]
mod offline {
    use crate::{
        ChatMessage, Connection, EntityModel, GameState, GroundItem, Input, Item, Loc, MenuClick,
        MenuEntry, MenuLayout, Npc, Overlay, Player, Projectile, SpotAnim, SubInterface, Widget,
        WorldSelect,
    };

    const NO_WIN32: &str = "clients can only be reached from Windows or Wine";

    pub struct ItemWatcher;

    impl ItemWatcher {
        pub fn new(_: u32) -> Result<Self, String> {
            Err(NO_WIN32.into())
        }
        pub fn read(&mut self, _: usize) -> crate::ItemPage {
            crate::ItemPage::default()
        }
    }

    pub struct PacketWatcher;

    impl PacketWatcher {
        pub fn new(_: u32) -> Result<Self, String> {
            Err(NO_WIN32.into())
        }
        pub fn set_filter(&mut self, _: ipc::packets::Filter) {}
        pub fn written(&self) -> u64 {
            0
        }
        pub fn read(&mut self, _: usize) -> ipc::packets::Page {
            ipc::packets::Page::default()
        }
    }

    pub fn wait_watchers(
        _: Option<&ItemWatcher>,
        _: Option<&PacketWatcher>,
        timeout: std::time::Duration,
    ) -> bool {
        std::thread::sleep(timeout);
        false
    }

    pub fn connection(_: u32) -> Option<Connection> {
        None
    }
    pub fn game_state(_: u32) -> Result<GameState, String> {
        Err(NO_WIN32.into())
    }
    pub fn widget(_: u32, _: u16, _: u16) -> Result<Option<Widget>, String> {
        Err(NO_WIN32.into())
    }
    pub fn sub_interfaces(_: u32) -> Result<Option<Vec<SubInterface>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn menu(_: u32) -> Result<Vec<MenuEntry>, String> {
        Err(NO_WIN32.into())
    }
    pub fn menu_open(_: u32) -> Result<bool, String> {
        Err(NO_WIN32.into())
    }
    pub fn menu_click(_: u32) -> Result<Option<MenuClick>, String> {
        Err(NO_WIN32.into())
    }
    pub fn menu_layout(_: u32) -> Result<Option<MenuLayout>, String> {
        Err(NO_WIN32.into())
    }
    pub fn selected_widget(_: u32) -> Result<Option<crate::SelectedWidget>, String> {
        Err(NO_WIN32.into())
    }
    pub fn project(
        _: u32,
        _: &[crate::projection::Point],
        _: crate::projection::Via,
    ) -> Result<Vec<[f32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn project_moved(
        _: u32,
        _: &[crate::projection::Point],
        _: [i32; 3],
    ) -> Result<Vec<[f32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn game_val_names(_: u32, _: u32, _: &[i32]) -> Result<Vec<Option<String>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn world_route(
        _: u32,
        _: crate::nav::Point,
        _: crate::nav::Point,
        _: &[String],
    ) -> Result<crate::landmass::Route, String> {
        Err(NO_WIN32.into())
    }
    pub fn widget_model(
        _: u32,
        _: u16,
        _: u16,
    ) -> Result<Option<decode::widgets::WidgetModel>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_names(_: u32, _: &[i32]) -> Result<Vec<Option<String>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_stackable(_: u32, _: &[i32]) -> Result<Vec<Option<bool>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_container(_: u32, _: i32) -> Result<Option<Vec<Item>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn varc(_: u32, _: i32) -> Result<Option<i32>, String> {
        Err(NO_WIN32.into())
    }
    pub fn varp_value(_: u32, _: usize) -> Result<Option<i32>, String> {
        Err(NO_WIN32.into())
    }
    pub fn varps(_: u32) -> Result<Option<Vec<i32>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn varbit(_: u32, _: i32) -> Result<Option<i32>, String> {
        Err(NO_WIN32.into())
    }
    pub fn landmasses(_: u32, _: &[[i32; 3]]) -> Result<Vec<u32>, String> {
        Err(NO_WIN32.into())
    }
    pub fn cache_files(_: u32, _: u32, _: &[[i32; 2]]) -> Result<Vec<Option<Vec<u8>>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn enum_def(_: u32, _: i32) -> Result<Option<crate::config::EnumDef>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_params(
        _: u32,
        _: &[i32],
    ) -> Result<Vec<Option<std::collections::BTreeMap<u32, crate::config::Value>>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn npcs(_: u32) -> Result<Vec<Npc>, String> {
        Err(NO_WIN32.into())
    }
    pub fn npc_model(_: u32, _: i32) -> Result<Option<EntityModel>, String> {
        Err(NO_WIN32.into())
    }
    pub fn loc_model(_: u32, _: u8, _: i32, _: i32, _: i32) -> Result<Option<EntityModel>, String> {
        Err(NO_WIN32.into())
    }
    pub fn ground_items(_: u32) -> Result<Vec<GroundItem>, String> {
        Err(NO_WIN32.into())
    }
    pub fn projectiles(_: u32) -> Result<Vec<Projectile>, String> {
        Err(NO_WIN32.into())
    }
    pub fn spot_anims(_: u32) -> Result<Vec<SpotAnim>, String> {
        Err(NO_WIN32.into())
    }
    pub fn chat_messages(_: u32) -> Result<Vec<ChatMessage>, String> {
        Err(NO_WIN32.into())
    }
    pub fn locs(_: u32, _: Option<i32>) -> Result<Vec<Loc>, String> {
        Err(NO_WIN32.into())
    }
    pub fn loc_shown(_: u32, _: &[u32]) -> Result<Vec<crate::LocView>, String> {
        Err(NO_WIN32.into())
    }
    pub fn cache_locs(
        _: u32,
        _: [i32; 4],
        _: &crate::CacheLocFilter,
    ) -> Result<Vec<crate::CacheLoc>, String> {
        Err(NO_WIN32.into())
    }
    pub fn find_path(
        _: u32,
        _: i32,
        _: [i32; 2],
        _: [i32; 2],
        _: Option<usize>,
    ) -> Result<Vec<[i32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn server_tile(_: u32) -> Result<Option<crate::ServerTile>, String> {
        Err(NO_WIN32.into())
    }
    pub fn movement(_: u32) -> Result<Option<crate::Movement>, String> {
        Err(NO_WIN32.into())
    }
    pub fn player_plane(_: u32) -> Result<i32, String> {
        Err(NO_WIN32.into())
    }
    pub fn map_regions(_: u32) -> Result<Vec<[i32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn in_instance(_: u32) -> Result<bool, String> {
        Err(NO_WIN32.into())
    }
    pub fn npc_sizes(_: u32, _: &[i32]) -> Result<std::collections::BTreeMap<i32, i32>, String> {
        Err(NO_WIN32.into())
    }
    pub fn npc_hitsplats(
        _: u32,
        _: &[i32],
    ) -> Result<std::collections::BTreeMap<i32, Vec<decode::world::Hitsplat>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn player_hitsplats(
        _: u32,
        _: &[i32],
    ) -> Result<std::collections::BTreeMap<i32, Vec<decode::world::Hitsplat>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn client_cycle(_: u32) -> Result<i32, String> {
        Err(NO_WIN32.into())
    }
    pub fn scene_collision(_: u32) -> Result<(i32, i32, i32, Vec<Vec<u32>>), String> {
        Err(NO_WIN32.into())
    }
    pub fn scene_route(
        _: u32,
        _: [i32; 2],
        _: [i32; 2],
        _: Option<crate::scene_graph::Options>,
    ) -> Result<crate::pathfinding::Route, String> {
        Err(NO_WIN32.into())
    }
    pub fn nav_plan(
        _: u32,
        _: crate::nav::Point,
        _: crate::nav::Point,
        _: &crate::nav::Request,
    ) -> Result<crate::nav::NavPlan, String> {
        Err(NO_WIN32.into())
    }
    pub fn npc_type_model(
        _: u32,
        _: i32,
        _: i32,
        _: i32,
        _: (i32, i32),
        _: i32,
    ) -> Result<Option<EntityModel>, String> {
        Err(NO_WIN32.into())
    }
    pub fn player_model(_: u32, _: i32) -> Result<Option<EntityModel>, String> {
        Err(NO_WIN32.into())
    }
    pub fn players(_: u32) -> Result<Vec<Player>, String> {
        Err(NO_WIN32.into())
    }
    pub fn widget_children(_: u32, _: u16, _: u16) -> Result<Option<Vec<Widget>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn world_select(_: u32) -> Result<Option<WorldSelect>, String> {
        Err(NO_WIN32.into())
    }
    pub fn send_input(_: u32, _: &Input) -> Result<(), String> {
        Err(NO_WIN32.into())
    }
    pub fn send_input_with(_: u32, _: &Input, _: &dyn super::Pace) -> Result<(), String> {
        Err(NO_WIN32.into())
    }
    pub fn bank_pin_enter(
        _: u32,
        _: &(dyn super::Pace + Sync),
    ) -> Result<crate::bank_pin::Outcome, String> {
        Err(NO_WIN32.into())
    }
    pub fn draw(_: u32, _: &Overlay) -> Result<(), String> {
        Err(NO_WIN32.into())
    }
    pub fn draw_batch(_: u32, _: &[Overlay]) -> Result<(), String> {
        Err(NO_WIN32.into())
    }
    pub fn draw_lines(_: u32, _: &[[i32; 4]], _: crate::Rgba) -> Result<(), String> {
        Err(NO_WIN32.into())
    }
}

const NO_CLIENT: &str = "no client selected";
const NO_AGENT: &str = "agent isn't loaded in the client";

#[derive(Serialize)]
struct ScreenPoint {
    x: f32,
    y: f32,
}

#[derive(Serialize)]
struct ScriptModel {
    bounding_box: bool,
    orientation: i32,
    vertex_count: u32,
    corners: Vec<ScreenPoint>,
    hull: Vec<ScreenPoint>,
    faces: Vec<[ScreenPoint; 3]>,
    frame_count: u32,
}

fn script_model(m: crate::EntityModel) -> ScriptModel {
    let points = |points: Vec<[f32; 2]>| {
        points
            .into_iter()
            .map(|[x, y]| ScreenPoint { x, y })
            .collect()
    };
    ScriptModel {
        bounding_box: m.bounding_box,
        orientation: m.orientation,
        vertex_count: m.vertex_count,
        corners: points(m.corners),
        hull: points(m.hull),
        faces: m
            .faces
            .into_iter()
            .map(|face| face.map(|[x, y]| ScreenPoint { x, y }))
            .collect(),
        frame_count: m.frame_count,
    }
}

// An HTTP response as the script sees it.
#[derive(Serialize)]
struct ScriptResponse {
    ok: bool,
    #[serde(flatten)]
    response: crate::http::Response,
}

fn to_lua<T: Serialize>(lua: &Lua, value: &T) -> mlua::Result<mlua::Value> {
    lua.to_value_with(
        value,
        SerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

#[derive(Serialize)]
struct PathTile {
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct SceneRoute {
    tiles: Vec<PathTile>,
    reached: bool,
}

// A part of the scene's collision map: `flags` column by column (west first), each column south first.
#[derive(Serialize)]
struct Collision {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    plane: i32,
    flags: Vec<u32>,
}

/// The registered item change and packet callbacks and the watchers that wake the script for them.
#[derive(Default)]
struct Callbacks {
    items: Option<ItemWatcher>,
    item_callbacks: Vec<(u64, mlua::Function)>,
    packets: Option<PacketWatcher>,
    packet_callbacks: Vec<(u64, Filter, mlua::Function)>,
    /// the packets a running `wait_packet` waits for, on top of the callbacks'
    waiting: Option<Filter>,
    last_id: u64,
}

impl Callbacks {
    fn watching(&self) -> bool {
        self.items.is_some() || self.packets.is_some()
    }

    /// Makes the packet watcher filter for what the callbacks and a running wait want, and keeps it only while something does.
    fn refresh_packets(&mut self, pid: Option<u32>) -> mlua::Result<()> {
        let mut filter: Filter = self.waiting.unwrap_or_default();
        for (_, wanted, _) in &self.packet_callbacks {
            filter
                .iter_mut()
                .zip(wanted)
                .for_each(|(word, bits)| *word |= bits);
        }
        if filter == Filter::default() {
            self.packets = None;
            return Ok(());
        }
        if self.packets.is_none() {
            let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
            self.packets = Some(PacketWatcher::new(pid).map_err(mlua::Error::RuntimeError)?);
        }
        self.packets.as_mut().unwrap().set_filter(filter);
        Ok(())
    }
}

/// What a pump waits for besides the time running out.
enum Until {
    Time,
    ItemChange,
    /// a packet in the filter, from this sequence number on
    Packet(Filter, u64),
}

/// Waits up to `time`, calling every callback for each item change and packet logged meanwhile; returns the first item change or packet `until` asks for.
fn pump_events(
    lua: &Lua,
    callbacks: &Rc<RefCell<Callbacks>>,
    time: std::time::Duration,
    until: &Until,
) -> mlua::Result<Option<mlua::Value>> {
    let deadline = std::time::Instant::now() + time;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        // the borrow ends before any callback runs, so a callback may register or remove callbacks
        let (changes, packets) = {
            let mut callbacks = callbacks.borrow_mut();
            let callbacks = &mut *callbacks;
            if !callbacks.watching() {
                std::thread::sleep(left);
                return Ok(None);
            }
            if !wait_watchers(callbacks.items.as_ref(), callbacks.packets.as_ref(), left) {
                return Ok(None);
            }
            let changes = callbacks
                .items
                .as_mut()
                .map(|watcher| watcher.read(ITEM_CHANGES_PER_WAKE).changes)
                .unwrap_or_default();
            let packets = callbacks
                .packets
                .as_mut()
                .map(|watcher| watcher.read(PACKETS_PER_WAKE).entries)
                .unwrap_or_default();
            (changes, packets)
        };
        if changes.is_empty() && packets.is_empty() {
            if left.is_zero() {
                return Ok(None);
            }
            continue;
        }
        let mut found = None;
        let item_callbacks: Vec<mlua::Function> = callbacks
            .borrow()
            .item_callbacks
            .iter()
            .map(|(_, callback)| callback.clone())
            .collect();
        for change in &changes {
            let value = to_lua(lua, change)?;
            for callback in &item_callbacks {
                callback.call::<()>(value.clone())?;
            }
            if matches!(until, Until::ItemChange) && found.is_none() {
                found = Some(value);
            }
        }
        let packet_callbacks: Vec<(Filter, mlua::Function)> = callbacks
            .borrow()
            .packet_callbacks
            .iter()
            .map(|(_, filter, callback)| (*filter, callback.clone()))
            .collect();
        for packet in &packets {
            let value = packet_to_lua(lua, packet)?;
            for (filter, callback) in &packet_callbacks {
                if filter_has(filter, packet.direction, packet.opcode) {
                    callback.call::<()>(value.clone())?;
                }
            }
            if let Until::Packet(filter, since) = until
                && found.is_none()
                && packet.sequence >= *since
                && filter_has(filter, packet.direction, packet.opcode)
            {
                found = Some(value);
            }
        }
        if found.is_some() {
            return Ok(found);
        }
    }
}

/// Changes a callback pass handles at most per read: a bank opening logs a few hundred in one tick.
const ITEM_CHANGES_PER_WAKE: usize = 2000;

/// Log entries a callback pass reads at most, before filtering: the whole ring.
const PACKETS_PER_WAKE: usize = ipc::packets::SLOT_COUNT;

fn packet_to_lua(lua: &Lua, packet: &ipc::packets::Entry) -> mlua::Result<mlua::Value> {
    let table = lua.create_table()?;
    table.set("sequence", packet.sequence)?;
    table.set("time_ms", packet.time_ms)?;
    table.set("sent", packet.direction == ipc::packets::direction::SENT)?;
    table.set("opcode", packet.opcode)?;
    table.set("length", packet.length)?;
    table.set("payload", lua.create_string(&packet.payload)?)?;
    Ok(mlua::Value::Table(table))
}

/// The filter for a script's list of packet ids, received ones or, with `sent`, the ones the client sends.
fn packet_filter(name: &str, opcodes: &[u16], sent: Option<bool>) -> mlua::Result<Filter> {
    let direction = if sent.unwrap_or(false) {
        ipc::packets::direction::SENT
    } else {
        ipc::packets::direction::RECEIVED
    };
    let mut filter = Filter::default();
    for &opcode in opcodes {
        if !filter_add(&mut filter, direction, opcode) {
            return Err(mlua::Error::RuntimeError(format!(
                "{name}: packet {opcode} can't be listened for, only 0 to 255"
            )));
        }
    }
    if opcodes.is_empty() {
        return Err(mlua::Error::RuntimeError(format!(
            "{name}: no packets given"
        )));
    }
    Ok(filter)
}

// `scene` cut to `area` (x1, y1, x2, y2, corners included), or whole without one.
fn collision_cut(
    scene: (i32, i32, i32, Vec<Vec<u32>>),
    area: Option<[i32; 4]>,
) -> Result<Collision, String> {
    let (x0, y0, plane, columns) = scene;
    let height = columns.first().map_or(0, |c| c.len() as i32);
    let (x2_max, y2_max) = (x0 + columns.len() as i32 - 1, y0 + height - 1);
    let [x1, y1, x2, y2] = area.unwrap_or([x0, y0, x2_max, y2_max]);
    let (x1, y1, x2, y2) = (
        x1.min(x2).max(x0),
        y1.min(y2).max(y0),
        x1.max(x2).min(x2_max),
        y1.max(y2).min(y2_max),
    );
    if x1 > x2 || y1 > y2 {
        return Err("the area is outside the loaded scene".into());
    }
    let flags = (x1..=x2)
        .flat_map(|x| {
            columns[(x - x0) as usize][(y1 - y0) as usize..=(y2 - y0) as usize]
                .iter()
                .copied()
        })
        .collect();
    Ok(Collision {
        x: x1,
        y: y1,
        width: x2 - x1 + 1,
        height: y2 - y1 + 1,
        plane,
        flags,
    })
}

fn query<A: FromLuaMulti, T: Serialize>(
    lua: &Lua,
    pid: Option<u32>,
    read: impl Fn(u32, A) -> Result<T, String> + Send + 'static,
) -> mlua::Result<mlua::Function> {
    lua.create_function(
        move |lua, args: A| -> mlua::Result<(mlua::Value, Option<String>)> {
            let Some(pid) = pid else {
                return Ok((mlua::Value::Nil, Some(NO_CLIENT.into())));
            };
            match read(pid, args) {
                Ok(value) => Ok((to_lua(lua, &value)?, None)),
                Err(error) => Ok((mlua::Value::Nil, Some(error))),
            }
        },
    )
}

fn action<A: FromLuaMulti>(
    lua: &Lua,
    pid: Option<u32>,
    act: impl Fn(u32, A) -> mlua::Result<Result<(), String>> + Send + 'static,
) -> mlua::Result<mlua::Function> {
    lua.create_function(move |_, args: A| -> mlua::Result<()> {
        let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
        act(pid, args)?.map_err(mlua::Error::RuntimeError)
    })
}

// Input the script started without waiting (`*_async`), run one at a time on a thread of its own.
type InputWorker = Option<Arc<Worker<Job>>>;

// Takes the mouse and keyboard back: stops async input, releases what is held, drops the queue and returns once idle.
fn cancel_input(worker: &InputWorker) {
    if let Some(worker) = worker {
        worker.cancel();
    }
}

// Blocking input first takes the input back from any async input.
fn input<A: FromLuaMulti>(
    lua: &Lua,
    pid: Option<u32>,
    worker: &InputWorker,
    make: impl Fn(A) -> mlua::Result<Input> + Send + 'static,
) -> mlua::Result<mlua::Function> {
    let worker = worker.clone();
    action(lua, pid, move |pid, args| {
        let input = make(args)?;
        cancel_input(&worker);
        Ok(send_input(pid, &input))
    })
}

// Returns right away; the input runs after what is already queued.
fn input_async<A: FromLuaMulti>(
    lua: &Lua,
    pid: Option<u32>,
    worker: &InputWorker,
    make: impl Fn(A) -> mlua::Result<Input> + Send + 'static,
) -> mlua::Result<mlua::Function> {
    let worker = worker.clone();
    action(lua, pid, move |_, args| {
        let input = make(args)?;
        Ok(worker.as_ref().map_or(Err(NO_CLIENT.into()), |worker| {
            worker.push(Job::Input(input))
        }))
    })
}

// A query of the live menu, run after taking the input back.
fn menu_read<T: 'static>(
    worker: &InputWorker,
    read: fn(u32) -> Result<T, String>,
) -> impl Fn(u32, ()) -> Result<T, String> + Send + 'static {
    let worker = worker.clone();
    move |pid, ()| {
        cancel_input(&worker);
        read(pid)
    }
}

fn button_arg(name: Option<String>) -> mlua::Result<MouseButton> {
    name.map_or(Ok(MouseButton::Left), |name| {
        MouseButton::parse(&name).map_err(mlua::Error::RuntimeError)
    })
}

// A `draw_batch` shape: `{ kind = "rect" | "line", x1, y1, x2, y2, r, g, b, a? }` or `{ kind = "clear" }`.
fn overlay_arg(shape: &mlua::Table) -> mlua::Result<Overlay> {
    let kind: String = shape.get("kind")?;
    if kind == "clear" {
        return Ok(Overlay::Clear);
    }
    let [x1, y1, x2, y2] =
        ["x1", "y1", "x2", "y2"].map(|k| shape.get::<f64>(k).map(|v| v.round() as i32));
    let (x1, y1, x2, y2) = (x1?, y1?, x2?, y2?);
    let color = Rgba {
        r: shape.get("r")?,
        g: shape.get("g")?,
        b: shape.get("b")?,
        a: shape.get::<Option<u8>>("a")?.unwrap_or(255),
    };
    match kind.as_str() {
        "rect" => Ok(Overlay::Rect {
            x1,
            y1,
            x2,
            y2,
            color,
        }),
        "line" => Ok(Overlay::Line {
            x1,
            y1,
            x2,
            y2,
            color,
        }),
        _ => Err(mlua::Error::RuntimeError(format!(
            "unknown shape kind: {kind}"
        ))),
    }
}

/// What `world_route` tells a script.
#[derive(serde::Serialize)]
struct WorldRoute {
    known: bool,
    reachable: bool,
    from: Option<crate::nav::NavTile>,
    to: Option<crate::nav::NavTile>,
}

impl From<crate::landmass::Route> for WorldRoute {
    fn from(route: crate::landmass::Route) -> Self {
        use crate::landmass::Route;
        let tile = |(x, y, plane): crate::nav::Point| Some(crate::nav::NavTile { x, y, plane });
        let (known, reachable, from, to) = match route {
            Route::Unknown => (false, true, None, None),
            Route::None => (true, false, None, None),
            Route::Walk => (true, true, None, None),
            Route::Through(from, to) => (true, true, tile(from), tile(to)),
        };
        Self {
            known,
            reachable,
            from,
            to,
        }
    }
}

#[cfg(feature = "embedded-lib")]
macro_rules! lib_module {
    ($name:literal, $path:literal) => {
        (
            $name,
            $path,
            include_str!(concat!("../../../../lib/", $path)),
        )
    };
}

// The lib/ shipped inside the injector; scripts that know their own path read lib/ from disk instead.
#[cfg(feature = "embedded-lib")]
pub const LIB_MODULES: &[(&str, &str, &str)] = &[
    lib_module!("host", "host.luau"),
    lib_module!("game_state", "main/game_state.luau"),
    lib_module!("position", "main/position.luau"),
    lib_module!("titlescreen", "main/interfaces/login/titlescreen.luau"),
    lib_module!("login", "main/interfaces/login/login.luau"),
    lib_module!(
        "login_world_switcher",
        "main/interfaces/login/login_world_switcher.luau"
    ),
    lib_module!("logout", "main/interfaces/gametabs/logout.luau"),
    lib_module!(
        "worldswitcher",
        "main/interfaces/gametabs/worldswitcher.luau"
    ),
    lib_module!("stats", "main/interfaces/gametabs/stats.luau"),
    lib_module!(
        "welcome_screen",
        "main/interfaces/login/welcome_screen.luau"
    ),
    lib_module!("inventory", "main/interfaces/gametabs/inventory.luau"),
    lib_module!("wornitems", "main/interfaces/gametabs/wornitems.luau"),
    lib_module!("side_panels", "main/interfaces/gametabs/side_panels.luau"),
    lib_module!("combat_achievements", "main/combat_achievements.luau"),
    lib_module!("magic", "main/interfaces/gametabs/magic.luau"),
    lib_module!("journal", "main/interfaces/gametabs/journal.luau"),
    lib_module!("prayer", "main/interfaces/gametabs/prayer.luau"),
    lib_module!("npcs", "main/entities/npcs.luau"),
    lib_module!("objects", "main/entities/objects.luau"),
    lib_module!("ground_items", "main/entities/ground_items.luau"),
    lib_module!("projectiles", "main/entities/projectiles.luau"),
    lib_module!("spot_anims", "main/entities/spot_anims.luau"),
    lib_module!("clickbox", "main/clickbox.luau"),
    lib_module!("varps", "main/varps.luau"),
    lib_module!("varbits", "main/varbits.luau"),
    lib_module!("player", "main/player.luau"),
    lib_module!("projection", "main/projection/projection.luau"),
    lib_module!("camera", "main/camera.luau"),
    lib_module!("walker", "main/walker.luau"),
    lib_module!("safespot", "main/safespot.luau"),
    lib_module!("combat_safespot", "main/combat_safespot.luau"),
    lib_module!("players", "main/entities/players.luau"),
    lib_module!("bankmain", "main/interfaces/mainscreen/bankmain.luau"),
    lib_module!(
        "bankpin_keypad",
        "main/interfaces/mainscreen/bankpin_keypad.luau"
    ),
    lib_module!("depositbox", "main/interfaces/mainscreen/depositbox.luau"),
    lib_module!("toplevel", "main/interfaces/mainscreen/toplevel.luau"),
    lib_module!("chartering", "main/interfaces/mainscreen/chartering.luau"),
    lib_module!("mushtree", "main/interfaces/mainscreen/mushtree.luau"),
    lib_module!("quetzal", "main/interfaces/mainscreen/quetzal.luau"),
    lib_module!("spirittree", "main/interfaces/mainscreen/spirittree.luau"),
    lib_module!("minecart", "main/interfaces/mainscreen/minecart.luau"),
    lib_module!("gnomeglider", "main/interfaces/mainscreen/gnomeglider.luau"),
    lib_module!("balloon", "main/interfaces/mainscreen/balloon.luau"),
    lib_module!("obelisk", "main/interfaces/mainscreen/obelisk.luau"),
    lib_module!("nexus", "main/interfaces/mainscreen/nexus.luau"),
    lib_module!("house", "main/house.luau"),
    lib_module!("mounted", "main/interfaces/mainscreen/mounted.luau"),
    lib_module!("canoe", "main/interfaces/mainscreen/canoe.luau"),
    lib_module!("fairyring", "main/interfaces/mainscreen/fairyring.luau"),
    lib_module!("certer", "main/interfaces/mainscreen/certer.luau"),
    lib_module!("minigames", "main/interfaces/mainscreen/minigames.luau"),
    lib_module!(
        "jewellerybox",
        "main/interfaces/mainscreen/jewellerybox.luau"
    ),
    lib_module!("lamp", "main/interfaces/mainscreen/lamp.luau"),
    lib_module!(
        "surprise_exam",
        "main/interfaces/mainscreen/surprise_exam.luau"
    ),
    lib_module!(
        "itemretrieval",
        "main/interfaces/mainscreen/itemretrieval.luau"
    ),
    lib_module!("occult_altar", "main/occult_altar.luau"),
    lib_module!("randoms", "main/antiban/randoms.luau"),
    lib_module!("scrollbook", "main/interfaces/mainscreen/scrollbook.luau"),
    lib_module!("boltpouch", "main/interfaces/mainscreen/boltpouch.luau"),
    lib_module!("runepouch", "main/interfaces/mainscreen/runepouch.luau"),
    lib_module!("widgets", "main/interfaces/widgets.luau"),
    lib_module!("chatbox", "main/interfaces/chatbox/chatbox.luau"),
    lib_module!(
        "recommend_survey",
        "main/interfaces/chatbox/recommend_survey.luau"
    ),
    lib_module!("skillmulti", "main/interfaces/chatbox/skillmulti.luau"),
    lib_module!("make", "main/interfaces/chatbox/make.luau"),
    lib_module!("chat", "main/interfaces/chatbox/chat.luau"),
    lib_module!("chat_tabs", "main/interfaces/chatbox/chat_tabs.luau"),
    lib_module!("chat_options", "main/interfaces/chatbox/chat_options.luau"),
    lib_module!("minimenu", "main/interfaces/minimenu.luau"),
    lib_module!("minimap", "main/interfaces/minimap.luau"),
    lib_module!("interact", "main/interact.luau"),
    lib_module!("inv", "main/interfaces/inv.luau"),
    lib_module!("item", "utils/item.luau"),
    lib_module!("game_val", "utils/game_val.luau"),
    lib_module!("draw", "utils/draw.luau"),
    lib_module!("random", "utils/math/random.luau"),
    lib_module!("biometrics", "main/antiban/biometrics.luau"),
    lib_module!("antiban", "main/antiban/antiban.luau"),
    lib_module!("antiban_tasks", "main/antiban/antiban_tasks.luau"),
    lib_module!("transports", "main/transports.luau"),
    lib_module!("quests", "main/quests.luau"),
    lib_module!("diaries", "main/diaries.luau"),
    lib_module!("cs2", "utils/cs2.luau"),
    lib_module!("geometry", "utils/math/geometry.luau"),
    lib_module!("mouse", "utils/input/mouse.luau"),
    lib_module!("track", "utils/input/track.luau"),
    lib_module!("keyboard", "utils/input/keyboard.luau"),
    lib_module!("ticks", "utils/ticks.luau"),
    lib_module!("stats_report", "utils/stats_report.luau"),
    lib_module!("log", "utils/log.luau"),
    lib_module!("npc_health", "main/entities/npc_health.luau"),
    lib_module!("special_attack", "main/interfaces/special_attack.luau"),
    lib_module!("combat", "main/interfaces/gametabs/combat.luau"),
    lib_module!("ferox", "main/ferox.luau"),
    lib_module!("layout_item", "main/layout_item.luau"),
    lib_module!("equipment_layout", "main/equipment_layout.luau"),
    lib_module!("inventory_layout", "main/inventory_layout.luau"),
    lib_module!("prayer_layout", "main/prayer_layout.luau"),
];

#[cfg(not(feature = "embedded-lib"))]
pub const LIB_MODULES: &[(&str, &str, &str)] = &[];

// A point for `project`: optional `z` and a `tile` (table with `x` and `y`) naming the tile it stands on.
fn fine_point(p: &mlua::Table) -> mlua::Result<crate::projection::Point> {
    let tile = match p.get::<Option<mlua::Table>>("tile")? {
        Some(t) => Some([t.get::<i32>("x")?, t.get::<i32>("y")?]),
        None => None,
    };
    Ok(crate::projection::Point {
        tile,
        ..crate::projection::Point::new(
            p.get("x")?,
            p.get("y")?,
            p.get::<Option<i32>>("z")?.unwrap_or(0),
        )
    })
}

fn module_name(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".luau")
        .or_else(|| file.strip_suffix(".lua"))
        .unwrap_or(file)
}

// Seconds since local midnight, with the milliseconds as the fraction.
#[cfg(windows)]
fn local_time_of_day() -> f64 {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    f64::from(now.wHour) * 3600.0
        + f64::from(now.wMinute) * 60.0
        + f64::from(now.wSecond)
        + f64::from(now.wMilliseconds) / 1000.0
}

#[cfg(not(windows))]
fn local_time_of_day() -> f64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = since_epoch.as_secs() as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut local) };
    f64::from(local.tm_hour) * 3600.0
        + f64::from(local.tm_min) * 60.0
        + f64::from(local.tm_sec)
        + f64::from(since_epoch.subsec_millis()) / 1000.0
}

pub fn run(
    name: &str,
    source: &str,
    pid: Option<u32>,
    print: impl FnMut(String) + 'static,
) -> mlua::Result<()> {
    run_with_form(name, source, pid, print, None, None, None)
}

// Resolves `.` and `..` by name only, so a module is cached under one path however it was reached.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// The file `require("../lib/host")` names from `dir`; `memory` holds in-memory-only files by normalized path.
fn find_module(
    dir: &Path,
    path: &str,
    memory: &HashMap<PathBuf, String>,
) -> Result<PathBuf, Vec<PathBuf>> {
    let base = normalize(&dir.join(path));
    let with = |suffix: &str| {
        let mut file = base.clone().into_os_string();
        file.push(suffix);
        PathBuf::from(file)
    };
    let candidates = [with(".luau"), with(".lua"), base.join("init.luau")];
    candidates
        .iter()
        .find(|c| memory.contains_key(*c) || c.is_file())
        .cloned()
        .ok_or_else(|| candidates.into())
}

// A module that ships inside the injector (`LIB_MODULES`), looked up by its bare name.
fn embedded_module(path: &str) -> Result<(&'static str, String), String> {
    let name = module_name(path);
    let &(_, file, source) = LIB_MODULES
        .iter()
        .find(|&&(n, _, _)| n == name)
        .ok_or_else(|| {
            let known: Vec<&str> = LIB_MODULES.iter().map(|&(n, _, _)| n).collect();
            format!("module '{path}' not found, lib has: {}", known.join(", "))
        })?;
    // For working on lib/ without rebuilding: VOLCREN_LIB_DIR points at a lib/ to read instead.
    let source = match std::env::var_os("VOLCREN_LIB_DIR") {
        Some(dir) => std::fs::read_to_string(Path::new(&dir).join(file))
            .map_err(|e| format!("VOLCREN_LIB_DIR: cannot read {file}: {e}"))?,
        None => source.to_string(),
    };
    Ok((file, source))
}

// `require("@lib/main/walker")` names `main/walker` in the lib root, matching the `lib` alias in the `.luaurc` files.
fn lib_alias(path: &str) -> Option<&str> {
    path.strip_prefix("@lib/").filter(|rest| !rest.is_empty())
}

// What `require("../lib/main/walker")` asks for inside the lib, when the path names a `lib` folder: `main/walker`.
fn lib_relative(path: &str) -> Option<&str> {
    let mut rest = path;
    let mut stepped = false;
    while let Some(next) = rest.strip_prefix("../").or_else(|| rest.strip_prefix("./")) {
        rest = next;
        stepped = true;
    }
    rest.strip_prefix("lib/")
        .filter(|r| stepped && !r.is_empty())
}

// `script_path` is the script's file; `lib_dir` is the developer's lib folder, used instead of the one beside the script.
pub fn run_with_form(
    name: &str,
    source: &str,
    pid: Option<u32>,
    print: impl FnMut(String) + 'static,
    form: Option<crate::form::FormHandler>,
    script_path: Option<&Path>,
    lib_dir: Option<&Path>,
) -> mlua::Result<()> {
    run_with_modules(
        name,
        source,
        pid,
        print,
        form,
        script_path,
        lib_dir,
        HashMap::new(),
    )
}

// `modules` are the script's other files kept in memory, found by `require("./name")` as if beside `script_path`.
#[allow(clippy::too_many_arguments)]
pub fn run_with_modules(
    name: &str,
    source: &str,
    pid: Option<u32>,
    print: impl FnMut(String) + 'static,
    form: Option<crate::form::FormHandler>,
    script_path: Option<&Path>,
    lib_dir: Option<&Path>,
    modules: HashMap<String, String>,
) -> mlua::Result<()> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT,
        LuaOptions::default(),
    )?;
    let print: Rc<RefCell<dyn FnMut(String)>> = Rc::new(RefCell::new(print));
    let globals = lua.globals();
    let worker: InputWorker = pid.map(|pid| {
        Arc::new(input_jobs::worker(move |input: &Input, pace: &dyn Pace| {
            send_input_with(pid, input, pace)
        }))
    });
    globals.set("dofile", mlua::Nil)?;
    globals.set("loadfile", mlua::Nil)?;

    // Chunk name -> the directory its relative requires start from, for the script and every module read from disk.
    let dirs: Rc<RefCell<HashMap<String, PathBuf>>> = Rc::default();
    if let Some(dir) = script_path.and_then(Path::parent) {
        dirs.borrow_mut()
            .insert(format!("={name}"), dir.to_path_buf());
    }

    let memory: HashMap<PathBuf, String> = match script_path.and_then(Path::parent) {
        Some(dir) => modules
            .into_iter()
            .map(|(file, text)| (normalize(&dir.join(file)), text))
            .collect(),
        None => HashMap::new(),
    };
    let script_chunk = script_path.map(|_| format!("={name}"));
    // Where `@lib/...` points from any file: the configured lib, else the one beside the script.
    let lib_root = lib_dir
        .map(Path::to_path_buf)
        .or_else(|| script_path.and_then(Path::parent).map(|d| d.join("../lib")));
    let lib_dir = lib_dir.map(Path::to_path_buf);

    let loaded = lua.create_table()?;
    globals.set(
        "require",
        lua.create_function(move |lua, path: String| {
            let alias = lib_alias(&path).map(str::to_string);
            // Embedded lookups and error messages still see the `../lib/...` spelling.
            let path = match &alias {
                Some(rest) => format!("../lib/{rest}"),
                None => path,
            };
            let relative = path.starts_with("./") || path.starts_with("../");
            let from = lua
                .inspect_stack(1, |d| d.source().source.map(|s| s.into_owned()))
                .flatten();
            let from_script = from.is_some() && from == script_chunk;
            let dir = from
                .and_then(|chunk| dirs.borrow().get(&chunk).cloned())
                .filter(|_| relative);

            // The script's own `../lib/...` requires read from the configured lib folder; the modules inside it find each other as usual.
            let found = match (&alias, &lib_root, &lib_dir, lib_relative(&path)) {
                // `@lib/...` means the same lib root whichever file asks, the script or a lib module.
                (Some(rest), Some(root), _, _) => Some(find_module(root, rest, &memory)),
                (_, _, Some(lib), Some(rest)) if from_script => {
                    Some(find_module(lib, rest, &memory))
                }
                _ => dir.map(|dir| find_module(&dir, &path, &memory)),
            };
            let key = match &found {
                Some(Ok(file)) => file.display().to_string(),
                _ => module_name(&path).to_string(),
            };
            let cached: mlua::Value = loaded.get(key.as_str())?;
            if !cached.is_nil() {
                return Ok(cached);
            }

            let (chunk, source) = match found {
                Some(Ok(file)) => {
                    let source = match memory.get(&file) {
                        Some(text) => text.clone(),
                        None => std::fs::read_to_string(&file).map_err(|e| {
                            mlua::Error::RuntimeError(format!("cannot read {key}: {e}"))
                        })?,
                    };
                    dirs.borrow_mut().insert(
                        format!("={key}"),
                        file.parent().map(Path::to_path_buf).unwrap_or_default(),
                    );
                    (key.clone(), source)
                }
                found => {
                    let (file, source) =
                        embedded_module(&path).map_err(|embedded| match found {
                            Some(Err(tried)) => {
                                let tried: Vec<String> =
                                    tried.iter().map(|p| p.display().to_string()).collect();
                                mlua::Error::RuntimeError(format!(
                                    "module '{path}' not found, tried: {}",
                                    tried.join(", ")
                                ))
                            }
                            _ => mlua::Error::RuntimeError(embedded),
                        })?;
                    (format!("lib/{file}"), source)
                }
            };
            let value: mlua::Value = lua.load(source).set_name(format!("={chunk}")).eval()?;
            loaded.set(key, value.clone())?;
            Ok(value)
        })?,
    )?;

    let printer = print.clone();
    globals.set(
        "print",
        lua.create_function_mut(move |_, args: MultiValue| {
            let line: Vec<String> = args
                .iter()
                .map(|v| v.to_string().unwrap_or_else(|_| "?".into()))
                .collect();
            // U+001E starts the loader's own lines (`FORM`, `STATS`, `PINREQ`, ...); a script must not be able to write one.
            (printer.borrow_mut())(line.join("\t").replace('\u{1e}', ""));
            Ok(())
        })?,
    )?;
    // The loader shows these under the skills in its XP Tracker tab and works out the rates itself; each call replaces the last.
    let counters_print = print.clone();
    globals.set(
        "report_counters",
        lua.create_function_mut(move |lua, counters: mlua::Value| {
            let counters: Vec<crate::form::Counter> = lua
                .from_value(counters)
                .map_err(|e| mlua::Error::RuntimeError(format!("report_counters: {e}")))?;
            if counters.iter().any(|c| !c.value.is_finite()) {
                return Err(mlua::Error::RuntimeError(
                    "report_counters: values must be finite numbers".into(),
                ));
            }
            (counters_print.borrow_mut())(crate::form::counters_line(&counters));
            Ok(())
        })?,
    )?;
    // Biometrics: the hash and the per-account profile are computed here; the lib caches the profile.
    globals.set(
        "murmur2a",
        lua.create_function(|_, (text, seed): (mlua::LuaString, Option<f64>)| {
            Ok(crate::biometrics::murmur2a(
                &text.as_bytes(),
                seed.unwrap_or(0.0).rem_euclid(4294967296.0) as u32,
            ))
        })?,
    )?;
    globals.set(
        "biometrics_profile",
        lua.create_function(|_, name: mlua::LuaString| {
            Ok(crate::biometrics::profile(&name.as_bytes()).to_vec())
        })?,
    )?;
    // The loader shows these in its Breaks tab, in any run, and counts the times down itself.
    let breaks_print = print.clone();
    globals.set(
        "report_breaks",
        lua.create_function_mut(move |lua, plan: mlua::Value| {
            let plan: crate::form::BreakPlan = lua
                .from_value(plan)
                .map_err(|e| mlua::Error::RuntimeError(format!("report_breaks: {e}")))?;
            (breaks_print.borrow_mut())(crate::form::breaks_line(&plan));
            Ok(())
        })?,
    )?;
    // The loader shows these in its log in the warning color (yellow), for things the script wants noticed that aren't errors.
    let warn_print = print.clone();
    globals.set(
        "report_warning",
        lua.create_function_mut(move |_, text: mlua::LuaString| {
            let text = String::from_utf8_lossy(&text.as_bytes()).replace('\u{1e}', "");
            (warn_print.borrow_mut())(format!("{}{text}", crate::form::WARN_PREFIX));
            Ok(())
        })?,
    )?;
    // The loader sums these between heartbeats. Outside a store run it has nowhere to send them and drops them.
    globals.set(
        "report_stats",
        lua.create_function_mut(
            move |_, (experience, gold, custom): (f64, f64, Option<Vec<f64>>)| {
                if !experience.is_finite() || !gold.is_finite() || experience < 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "report_stats: experience must be a number of 0 or more, and gold a number"
                            .into(),
                    ));
                }
                let custom: Vec<i64> = custom
                    .unwrap_or_default()
                    .into_iter()
                    .map(|value| value.round() as i64)
                    .collect();
                (print.borrow_mut())(crate::form::stats_line(
                    experience.round() as i64,
                    gold.round() as i64,
                    &custom,
                ));
                Ok(())
            },
        )?,
    )?;

    let started = std::time::Instant::now();
    globals.set(
        "time",
        lua.create_function(move |_, (): ()| Ok(started.elapsed().as_secs_f64() * 1000.0))?,
    )?;
    globals.set(
        "clock",
        lua.create_function(|_, (): ()| Ok(local_time_of_day()))?,
    )?;
    globals.set(
        "unix_time",
        lua.create_function(|_, (): ()| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64()))
        })?,
    )?;

    // Item change and packet callbacks; they run inside `sleep`, `wait_item_change` and `wait_packet`, not interrupting the script.
    let events = Rc::new(RefCell::new(Callbacks::default()));
    let registry = events.clone();
    globals.set(
        "on_item_change",
        lua.create_function(move |_, callback: mlua::Function| {
            let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
            let mut events = registry.borrow_mut();
            if events.items.is_none() {
                events.items = Some(ItemWatcher::new(pid).map_err(mlua::Error::RuntimeError)?);
            }
            events.last_id += 1;
            let id = events.last_id;
            events.item_callbacks.push((id, callback));
            Ok(id)
        })?,
    )?;
    let registry = events.clone();
    globals.set(
        "off_item_change",
        lua.create_function(move |_, id: u64| {
            let mut events = registry.borrow_mut();
            let before = events.item_callbacks.len();
            events.item_callbacks.retain(|(known, _)| *known != id);
            if events.item_callbacks.is_empty() {
                events.items = None;
            }
            Ok(events.item_callbacks.len() != before)
        })?,
    )?;
    let pump = events.clone();
    globals.set(
        "wait_item_change",
        lua.create_function(move |lua, ms: u64| {
            Ok(pump_events(
                lua,
                &pump,
                std::time::Duration::from_millis(ms),
                &Until::ItemChange,
            )?
            .is_some())
        })?,
    )?;
    let registry = events.clone();
    globals.set(
        "on_packet",
        lua.create_function(
            move |_, (opcodes, callback, sent): (Vec<u16>, mlua::Function, Option<bool>)| {
                let filter = packet_filter("on_packet", &opcodes, sent)?;
                let mut events = registry.borrow_mut();
                events.last_id += 1;
                let id = events.last_id;
                events.packet_callbacks.push((id, filter, callback));
                if let Err(error) = events.refresh_packets(pid) {
                    events.packet_callbacks.pop();
                    return Err(error);
                }
                Ok(id)
            },
        )?,
    )?;
    let registry = events.clone();
    globals.set(
        "off_packet",
        lua.create_function(move |_, id: u64| {
            let mut events = registry.borrow_mut();
            let before = events.packet_callbacks.len();
            events.packet_callbacks.retain(|(known, _, _)| *known != id);
            events.refresh_packets(pid)?;
            Ok(events.packet_callbacks.len() != before)
        })?,
    )?;
    let pump = events.clone();
    globals.set(
        "wait_packet",
        lua.create_function(
            move |lua, (opcodes, ms, sent): (Vec<u16>, u64, Option<bool>)| {
                let filter = packet_filter("wait_packet", &opcodes, sent)?;
                // only a packet logged from now on counts, not one waiting from before the call
                let (previous, since) = {
                    let mut events = pump.borrow_mut();
                    let previous = events.waiting.replace(filter);
                    if let Err(error) = events.refresh_packets(pid) {
                        events.waiting = previous;
                        return Err(error);
                    }
                    (
                        previous,
                        events
                            .packets
                            .as_ref()
                            .map_or(0, |watcher| watcher.written()),
                    )
                };
                let found = pump_events(
                    lua,
                    &pump,
                    std::time::Duration::from_millis(ms),
                    &Until::Packet(filter, since),
                );
                let mut events = pump.borrow_mut();
                events.waiting = previous;
                events.refresh_packets(pid)?;
                Ok(found?.unwrap_or(mlua::Value::Nil))
            },
        )?,
    )?;
    let pump = events;
    globals.set(
        "sleep",
        lua.create_function(move |lua, ms: u64| {
            let time = std::time::Duration::from_millis(ms);
            if pump.borrow().watching() {
                pump_events(lua, &pump, time, &Until::Time)?;
            } else {
                std::thread::sleep(time);
            }
            Ok(())
        })?,
    )?;

    // Like `connection`, it needs the agent loaded and raises without it; what it returns always has a table.
    globals.set(
        "game_state",
        lua.create_function(move |lua, (): ()| -> mlua::Result<mlua::Value> {
            let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
            let state = game_state(pid).map_err(mlua::Error::RuntimeError)?;
            to_lua(lua, &state)
        })?,
    )?;
    // Scripts need the agent loaded, so a missing client or agent is an error and `connection()` always returns one.
    globals.set(
        "connection",
        lua.create_function(move |lua, (): ()| -> mlua::Result<mlua::Value> {
            let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
            let conn = connection(pid).ok_or_else(|| mlua::Error::RuntimeError(NO_AGENT.into()))?;
            if let Some(problem) = conn.problem() {
                return Err(mlua::Error::RuntimeError(problem));
            }
            to_lua(lua, &conn)
        })?,
    )?;
    globals.set(
        "widget",
        query(&lua, pid, |pid, (group, child): (u16, u16)| {
            widget(pid, group, child)
        })?,
    )?;
    globals.set(
        "widget_model",
        query(&lua, pid, |pid, (group, child): (u16, u16)| {
            widget_model(pid, group, child)
        })?,
    )?;
    globals.set(
        "item_container",
        query(&lua, pid, |pid, id: i32| item_container(pid, id))?,
    )?;
    globals.set(
        "sub_interfaces",
        query(&lua, pid, |pid, (): ()| sub_interfaces(pid))?,
    )?;
    let form = std::cell::RefCell::new(form);
    globals.set(
        "form",
        lua.create_function(
            move |lua, spec: mlua::Value| -> mlua::Result<(mlua::Value, Option<String>)> {
                let spec: serde_json::Value = lua.from_value(spec)?;
                crate::form::validate(&spec).map_err(mlua::Error::RuntimeError)?;
                let values = match form.borrow_mut().as_mut() {
                    Some(handler) => handler(&spec).map_err(mlua::Error::RuntimeError)?,
                    None => Some(crate::form::defaults(&spec)),
                };
                match values {
                    Some(values) => Ok((to_lua(lua, &values)?, None)),
                    None => Ok((mlua::Value::Nil, Some("cancelled".into()))),
                }
            },
        )?,
    )?;
    // The values the user last updated the settings to while the script runs, or nil when there is nothing new.
    globals.set(
        "form_update",
        lua.create_function(|lua, (): ()| match crate::form::take_update() {
            Some(values) => to_lua(lua, &values),
            None => Ok(mlua::Value::Nil),
        })?,
    )?;
    // The global break and sleep config, with a `version` that changes whenever the loader sends a new one.
    globals.set(
        "antiban_config",
        lua.create_function(|lua, (): ()| to_lua(lua, &crate::antiban::current_value()))?,
    )?;
    // Which types of transport the user turned off in the loader: `{ off = { names } }`.
    globals.set(
        "transport_settings",
        lua.create_function(|lua, (): ()| to_lua(lua, &crate::transports::current_settings()))?,
    )?;
    // The transports (teleport spells, ...) the loader downloaded and parsed: `{ source, transports, version }`.
    globals.set(
        "transports",
        lua.create_function(|lua, (): ()| to_lua(lua, &crate::transports::current_value()))?,
    )?;
    #[cfg(test)]
    globals.set(
        "test_set_transports",
        lua.create_function(|_, json: String| {
            crate::transports::set(
                crate::transports::parse(&json)
                    .ok_or_else(|| mlua::Error::RuntimeError("bad transports".into()))?,
            );
            Ok(())
        })?,
    )?;
    // The queued breaks and sleeps the user asked to skip once since the last call, as `{ kind, index }` (index from 0).
    globals.set(
        "antiban_skips",
        lua.create_function(|lua, (): ()| to_lua(lua, &crate::antiban::take_skips()))?,
    )?;
    #[cfg(test)]
    globals.set(
        "test_skip",
        lua.create_function(|_, (kind, index): (String, u32)| {
            crate::antiban::push_skip(crate::antiban::Skip { kind, index });
            Ok(())
        })?,
    )?;
    #[cfg(test)]
    globals.set(
        "test_set_antiban",
        lua.create_function(|_, json: String| {
            crate::antiban::set(
                crate::antiban::parse(&json)
                    .ok_or_else(|| mlua::Error::RuntimeError("bad config".into()))?,
            );
            Ok(())
        })?,
    )?;
    // The live menu has to be read with the mouse still, so these take the input back first.
    globals.set("menu", query(&lua, pid, menu_read(&worker, menu))?)?;
    globals.set(
        "menu_click",
        query(&lua, pid, |pid, (): ()| menu_click(pid))?,
    )?;
    globals.set(
        "menu_open",
        query(&lua, pid, menu_read(&worker, menu_open))?,
    )?;
    globals.set(
        "menu_layout",
        query(&lua, pid, menu_read(&worker, menu_layout))?,
    )?;
    globals.set(
        "selected_widget",
        query(&lua, pid, |pid, (): ()| selected_widget(pid))?,
    )?;
    globals.set(
        "project",
        query(
            &lua,
            pid,
            |pid, (points, via, shift): (Vec<mlua::Table>, Option<String>, Option<mlua::Table>)| {
                let via = Via::parse(via.as_deref())?;
                let points = points
                    .iter()
                    .map(fine_point)
                    .collect::<mlua::Result<Vec<_>>>()
                    .map_err(|e| e.to_string())?;
                let screen = match shift {
                    None => project(pid, &points, via)?,
                    Some(_) if via == Via::Client => {
                        return Err("the client's projection can't move the camera".into());
                    }
                    Some(s) => project_moved(
                        pid,
                        &points,
                        [
                            s.get("x").map_err(|e| e.to_string())?,
                            s.get("y").map_err(|e| e.to_string())?,
                            s.get::<Option<i32>>("plane")
                                .map_err(|e| e.to_string())?
                                .unwrap_or(-1),
                        ],
                    )?,
                };
                Ok(screen
                    .into_iter()
                    .map(|[x, y]| ScreenPoint { x, y })
                    .collect::<Vec<_>>())
            },
        )?,
    )?;
    globals.set(
        "game_val_name",
        query(&lua, pid, |pid, (kind, id): (u32, i32)| {
            Ok(game_val_names(pid, kind, &[id])?.pop().flatten())
        })?,
    )?;
    globals.set(
        "game_val_names",
        query(&lua, pid, |pid, (kind, ids): (u32, Vec<i32>)| {
            game_val_names(pid, kind, &ids)
        })?,
    )?;
    globals.set(
        "item_name",
        query(&lua, pid, |pid, id: i32| {
            Ok(item_names(pid, &[id])?.pop().flatten())
        })?,
    )?;
    globals.set(
        "item_names",
        query(&lua, pid, |pid, ids: Vec<i32>| item_names(pid, &ids))?,
    )?;
    globals.set(
        "item_stackable",
        query(&lua, pid, |pid, ids: Vec<i32>| item_stackable(pid, &ids))?,
    )?;
    globals.set("varc", query(&lua, pid, |pid, id: i32| varc(pid, id))?)?;
    globals.set(
        "varp",
        query(&lua, pid, |pid, id: usize| varp_value(pid, id))?,
    )?;
    globals.set("varps", query(&lua, pid, |pid, (): ()| varps(pid))?)?;
    globals.set("varbit", query(&lua, pid, |pid, id: i32| varbit(pid, id))?)?;
    globals.set(
        "enum_def",
        query(&lua, pid, |pid, id: i32| enum_def(pid, id))?,
    )?;
    // Several files of one cache index at once, `cache_files(index, { { archive, file }, ... })`: a list with a string of bytes for each file that exists and `false` for each that doesn't.
    globals.set(
        "cache_files",
        lua.create_function(
            move |lua,
                  (index, wanted): (u32, Vec<[i32; 2]>)|
                  -> mlua::Result<(mlua::Value, Option<String>)> {
                let Some(pid) = pid else {
                    return Ok((mlua::Value::Nil, Some(NO_CLIENT.into())));
                };
                match cache_files(pid, index, &wanted) {
                    Ok(files) => {
                        let list = lua.create_table()?;
                        for (i, file) in files.into_iter().enumerate() {
                            list.set(
                                i + 1,
                                match file {
                                    Some(bytes) => mlua::Value::String(lua.create_string(&bytes)?),
                                    None => mlua::Value::Boolean(false),
                                },
                            )?;
                        }
                        Ok((mlua::Value::Table(list), None))
                    }
                    Err(error) => Ok((mlua::Value::Nil, Some(error))),
                }
            },
        )?,
    )?;
    // One file of the client's cache, as a Lua string of its bytes (nil when it doesn't exist): `cache_file(index, archive, file)`.
    globals.set(
        "cache_file",
        lua.create_function(
            move |lua,
                  (index, archive, file): (u32, i32, i32)|
                  -> mlua::Result<(mlua::Value, Option<String>)> {
                let Some(pid) = pid else {
                    return Ok((mlua::Value::Nil, Some(NO_CLIENT.into())));
                };
                match cache_files(pid, index, &[[archive, file]]) {
                    Ok(mut files) => match files.pop().flatten() {
                        Some(bytes) => Ok((mlua::Value::String(lua.create_string(&bytes)?), None)),
                        None => Ok((mlua::Value::Nil, None)),
                    },
                    Err(error) => Ok((mlua::Value::Nil, Some(error))),
                }
            },
        )?,
    )?;
    // The landmass number of each `{ x, y, plane }` (0: none, or not the surface): tiles with the same number can be walked between.
    globals.set(
        "landmasses",
        query(&lua, pid, |pid, points: Vec<mlua::Table>| {
            let read = |t: &mlua::Table, field: &str| -> Result<i32, String> {
                t.get::<i32>(field)
                    .map_err(|e| format!("a point's {field}: {e}"))
            };
            let points = points
                .iter()
                .map(|t| Ok([read(t, "x")?, read(t, "y")?, read(t, "plane")?]))
                .collect::<Result<Vec<[i32; 3]>, String>>()?;
            landmasses(pid, &points)
        })?,
    )?;
    // Whether `to` can be reached from `from` by walking and climbing: `{ known, reachable, from, to }`, the last two being the first climb on the way when one is needed.
    globals.set(
        "world_route",
        query(
            &lua,
            pid,
            |pid, (from, to, allow): (mlua::Table, mlua::Table, Option<Vec<String>>)| {
                let tile = |t: &mlua::Table| -> Result<crate::nav::Point, String> {
                    let read = |field: &str| {
                        t.get::<i32>(field)
                            .map_err(|e| format!("a tile's {field}: {e}"))
                    };
                    Ok((read("x")?, read("y")?, read("plane")?))
                };
                let route = world_route(pid, tile(&from)?, tile(&to)?, &allow.unwrap_or_default())?;
                Ok(WorldRoute::from(route))
            },
        )?,
    )?;
    globals.set(
        "item_params",
        query(&lua, pid, |pid, ids: Vec<i32>| item_params(pid, &ids))?,
    )?;
    globals.set("npcs", query(&lua, pid, |pid, (): ()| npcs(pid))?)?;
    globals.set(
        "npc_model",
        query(&lua, pid, |pid, index: i32| {
            Ok(npc_model(pid, index)?.map(script_model))
        })?,
    )?;
    globals.set(
        "npc_sizes",
        query(&lua, pid, |pid, indices: Vec<i32>| npc_sizes(pid, &indices))?,
    )?;
    globals.set(
        "npc_hitsplats",
        query(&lua, pid, |pid, indices: Vec<i32>| {
            npc_hitsplats(pid, &indices)
        })?,
    )?;
    globals.set(
        "player_hitsplats",
        query(&lua, pid, |pid, indices: Vec<i32>| {
            player_hitsplats(pid, &indices)
        })?,
    )?;
    globals.set(
        "client_cycle",
        query(&lua, pid, |pid, (): ()| client_cycle(pid))?,
    )?;
    globals.set(
        "locs",
        query(&lua, pid, |pid, plane: Option<i32>| locs(pid, plane))?,
    )?;
    globals.set(
        "map_regions",
        query(&lua, pid, |pid, (): ()| map_regions(pid))?,
    )?;
    globals.set(
        "in_instance",
        query(&lua, pid, |pid, (): ()| in_instance(pid))?,
    )?;
    globals.set(
        "server_tile",
        query(&lua, pid, |pid, (): ()| server_tile(pid))?,
    )?;
    globals.set("movement", query(&lua, pid, |pid, (): ()| movement(pid))?)?;
    globals.set(
        "ground_items",
        query(&lua, pid, |pid, (): ()| ground_items(pid))?,
    )?;
    globals.set(
        "projectiles",
        query(&lua, pid, |pid, (): ()| projectiles(pid))?,
    )?;
    globals.set(
        "spot_anims",
        query(&lua, pid, |pid, (): ()| spot_anims(pid))?,
    )?;
    globals.set(
        "chat_messages",
        query(&lua, pid, |pid, (): ()| chat_messages(pid))?,
    )?;
    globals.set(
        "find_path",
        query(
            &lua,
            pid,
            |pid,
             (from, to, plane, max_tiles): (
                mlua::Table,
                mlua::Table,
                Option<i32>,
                Option<usize>,
            )| {
                let tile = |t: &mlua::Table| -> Result<[i32; 2], String> {
                    Ok([
                        t.get("x").map_err(|e| e.to_string())?,
                        t.get("y").map_err(|e| e.to_string())?,
                    ])
                };
                let plane = match plane {
                    Some(plane) => plane,
                    None => player_plane(pid)?,
                };
                Ok(find_path(pid, plane, tile(&from)?, tile(&to)?, max_tiles)?
                    .into_iter()
                    .map(|[x, y]| PathTile { x, y })
                    .collect::<Vec<_>>())
            },
        )?,
    )?;
    // How objects look to the player now, by id: a multi-state object (a farming patch with a grown tree, say) shows the form its varbit picks, with that form's name and options
    globals.set(
        "loc_shown",
        query(&lua, pid, |pid, ids: Vec<u32>| loc_shown(pid, &ids))?,
    )?;
    globals.set(
        "cache_locs",
        query(
            &lua,
            pid,
            |pid, (area, filter): (mlua::Table, Option<mlua::Table>)| {
                let corner = |name: &str| area.get::<i32>(name).map_err(|e| e.to_string());
                let area = [corner("x1")?, corner("y1")?, corner("x2")?, corner("y2")?];
                let filter = match filter {
                    Some(t) => crate::CacheLocFilter {
                        plane: t.get::<Option<i32>>("plane").map_err(|e| e.to_string())?,
                        names: t
                            .get::<Option<Vec<String>>>("names")
                            .map_err(|e| e.to_string())?,
                        actions: t
                            .get::<Option<Vec<String>>>("actions")
                            .map_err(|e| e.to_string())?,
                    },
                    None => crate::CacheLocFilter::default(),
                };
                cache_locs(pid, area, &filter)
            },
        )?,
    )?;
    globals.set(
        "collision",
        query(&lua, pid, |pid, area: Option<mlua::Table>| {
            let area = match area {
                Some(t) => {
                    let corner = |name: &str| t.get::<i32>(name).map_err(|e| e.to_string());
                    Some([corner("x1")?, corner("y1")?, corner("x2")?, corner("y2")?])
                }
                None => None,
            };
            collision_cut(scene_collision(pid)?, area)
        })?,
    )?;
    globals.set(
        "scene_route",
        query(
            &lua,
            pid,
            |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
                let tile = |t: &mlua::Table| -> Result<[i32; 2], String> {
                    Ok([
                        t.get("x").map_err(|e| e.to_string())?,
                        t.get("y").map_err(|e| e.to_string())?,
                    ])
                };
                // a table asks for a randomized route, its fields override the defaults
                let options = options.map(|t| -> Result<crate::scene_graph::Options, String> {
                    let default = crate::scene_graph::Options::default();
                    let field = |name: &str| t.get::<Option<f64>>(name).map_err(|e| e.to_string());
                    Ok(crate::scene_graph::Options {
                        seed: match field("seed")? {
                            Some(seed) => seed as u64,
                            None => crate::scene_graph::mix(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map_or(0, |d| d.as_nanos() as u64),
                            ),
                        },
                        rnd: field("rnd")?.unwrap_or(default.rnd),
                        spacing: field("spacing")?.map_or(default.spacing, |v| v as i32),
                        jitter: field("jitter")?.map_or(default.jitter, |v| v as i32),
                        max_detour: field("max_detour")?.unwrap_or(default.max_detour),
                        offset: field("offset")?.map_or(default.offset, |v| v as i32),
                    })
                });
                let route = scene_route(pid, tile(&from)?, tile(&to)?, options.transpose()?)?;
                Ok(SceneRoute {
                    tiles: route
                        .tiles
                        .into_iter()
                        .map(|(x, y)| PathTile { x, y })
                        .collect(),
                    reached: route.reached,
                })
            },
        )?,
    )?;
    globals.set(
        "nav_plan",
        query(
            &lua,
            pid,
            |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
                let err = |e: mlua::Error| e.to_string();
                let point = |t: &mlua::Table| -> Result<crate::nav::Point, String> {
                    let plane = match t.get::<Option<i32>>("plane").map_err(err)? {
                        Some(plane) => plane,
                        None => player_plane(pid)?,
                    };
                    Ok((t.get("x").map_err(err)?, t.get("y").map_err(err)?, plane))
                };
                let mut request = crate::nav::Request::default();
                if let Some(t) = options {
                    let o = &mut request.options;
                    o.near = t.get::<Option<i32>>("near").map_err(err)?.unwrap_or(o.near);
                    o.door_cost = t
                        .get::<Option<u32>>("door_cost")
                        .map_err(err)?
                        .unwrap_or(o.door_cost);
                    o.climb_cost = t
                        .get::<Option<u32>>("climb_cost")
                        .map_err(err)?
                        .unwrap_or(o.climb_cost);
                    // the least (in tiles) an agility shortcut has to save over walking round
                    o.shortcut_saving = t
                        .get::<Option<f64>>("shortcut_saving")
                        .map_err(err)?
                        .map_or(o.shortcut_saving, |tiles| (tiles.max(0.0) * 10.0) as u32);
                    o.rnd = t.get::<Option<f64>>("rnd").map_err(err)?.unwrap_or(o.rnd);
                    o.seed = t.get::<Option<f64>>("seed").map_err(err)?.map(|s| s as u64);
                    if let Some(options) =
                        t.get::<Option<Vec<String>>>("door_options").map_err(err)?
                    {
                        request.door_options = options;
                    }
                    if let Some(climbs) = t.get::<Option<bool>>("climbs").map_err(err)? {
                        request.climbs = climbs;
                    }
                    if let Some(keys) = t.get::<Option<Vec<String>>>("allow_climbs").map_err(err)? {
                        request.allow_climbs = keys;
                    }
                    if let Some(names) =
                        t.get::<Option<Vec<String>>>("portal_names").map_err(err)?
                    {
                        request.portal_names = names;
                    }
                    for loc in t
                        .get::<Option<Vec<mlua::Table>>>("avoid")
                        .map_err(err)?
                        .unwrap_or_default()
                    {
                        let field = |name: &str| loc.get::<i32>(name).map_err(err);
                        request.avoid.push((
                            field("id")?,
                            field("x")?,
                            field("y")?,
                            field("plane")?,
                        ));
                    }
                }
                nav_plan(pid, point(&from)?, point(&to)?, &request)
            },
        )?,
    )?;
    globals.set(
        "loc_model",
        query(&lua, pid, |pid, loc: mlua::Table| {
            let field = |name: &str| loc.get::<i32>(name).map_err(|e| e.to_string());
            Ok(loc_model(
                pid,
                field("kind")? as u8,
                field("scene_x")?,
                field("scene_y")?,
                field("id")?,
            )?
            .map(script_model))
        })?,
    )?;
    globals.set(
        "npc_type_model",
        query(
            &lua,
            pid,
            |pid,
             (id, animation, frame, fine_x, fine_y, orientation): (
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
            )| {
                Ok(
                    npc_type_model(pid, id, animation, frame, (fine_x, fine_y), orientation)?
                        .map(script_model),
                )
            },
        )?,
    )?;
    globals.set("players", query(&lua, pid, |pid, (): ()| players(pid))?)?;
    globals.set(
        "player_model",
        query(&lua, pid, |pid, index: i32| {
            Ok(player_model(pid, index)?.map(script_model))
        })?,
    )?;
    globals.set(
        "world_select",
        query(&lua, pid, |pid, (): ()| world_select(pid))?,
    )?;
    globals.set(
        "widget_children",
        query(&lua, pid, |pid, (group, child): (u16, u16)| {
            widget_children(pid, group, child)
        })?,
    )?;

    fn click_input(
        (x, y, button, hold_ms): (i32, i32, Option<String>, Option<u32>),
    ) -> mlua::Result<Input> {
        Ok(Input::Click {
            button: button_arg(button)?,
            x,
            y,
            hold_ms: hold_ms.unwrap_or(crate::CLICK_HOLD_MS),
        })
    }
    fn move_input((x, y): (i32, i32)) -> mlua::Result<Input> {
        Ok(Input::Move { x, y })
    }
    fn wind_mouse_input((x, y, options): (i32, i32, Option<mlua::Table>)) -> mlua::Result<Input> {
        let mut opts = wind_mouse::Options::default();
        if let Some(options) = options {
            opts.speed = options.get::<Option<f64>>("speed")?.unwrap_or(opts.speed);
            opts.wind = options.get::<Option<f64>>("wind")?.unwrap_or(opts.wind);
            opts.gravity = options
                .get::<Option<f64>>("gravity")?
                .unwrap_or(opts.gravity);
            opts.timeout_ms = options
                .get::<Option<u32>>("timeout")?
                .unwrap_or(opts.timeout_ms);
            opts.pace = options.get::<Option<f64>>("pace")?.unwrap_or(opts.pace);
        }
        Ok(Input::WindMouse {
            x,
            y,
            options: opts,
        })
    }
    fn type_input((text, hold_ms): (String, Option<u32>)) -> mlua::Result<Input> {
        Ok(Input::Type {
            text,
            hold_ms: hold_ms.unwrap_or(crate::KEY_HOLD_MS),
        })
    }
    // Enters the PIN the loader gave this run on the open bank PIN keypad; it takes no PIN and returns only how it went.
    globals.set(
        "bank_pin_enter",
        lua.create_function({
            let worker = worker.clone();
            move |_, (): ()| -> mlua::Result<&'static str> {
                let pid = pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))?;
                cancel_input(&worker);
                bank_pin_enter(pid, &crate::async_input::Blocking)
                    .map(crate::bank_pin::Outcome::name)
                    .map_err(mlua::Error::RuntimeError)
            }
        })?,
    )?;
    // The skill the loader's Accounts page has set for spending the logged in account's lamps. The account is read here, as for the bank PIN: a script that named it could ask about any saved account.
    globals.set(
        "lamp_skill",
        lua.create_function(move |_, (): ()| {
            let skill = || -> Result<Option<String>, String> {
                let pid = pid.ok_or(NO_CLIENT)?;
                match game_state(pid)?.account_name {
                    Some(account) => crate::lamp::skill_for(&account),
                    None => Ok(None),
                }
            };
            Ok(match skill() {
                Ok(skill) => (skill, None),
                Err(error) => (None, Some(error)),
            })
        })?,
    )?;
    // Makes the request through the loader, which only reaches the hosts the user whitelisted. Needs no client.
    globals.set(
        "http_request",
        lua.create_function(
            |lua, options: mlua::Value| -> mlua::Result<(mlua::Value, Option<String>)> {
                let request: crate::http::Request = match lua.from_value(options) {
                    Ok(request) => request,
                    Err(error) => {
                        return Ok((mlua::Value::Nil, Some(format!("http_request: {error}"))));
                    }
                };
                Ok(match crate::http::request(&request) {
                    Ok(response) => (
                        to_lua(
                            lua,
                            &ScriptResponse {
                                ok: (200..300).contains(&response.status),
                                response,
                            },
                        )?,
                        None,
                    ),
                    Err(error) => (mlua::Value::Nil, Some(error)),
                })
            },
        )?,
    )?;
    globals.set("click", input(&lua, pid, &worker, click_input)?)?;
    globals.set("move", input(&lua, pid, &worker, move_input)?)?;
    globals.set("wind_mouse", input(&lua, pid, &worker, wind_mouse_input)?)?;
    globals.set("type_text", input(&lua, pid, &worker, type_input)?)?;
    fn mouse_down_input(button: Option<String>) -> mlua::Result<Input> {
        Ok(Input::Button {
            button: button_arg(button)?,
            down: true,
        })
    }
    fn mouse_up_input(button: Option<String>) -> mlua::Result<Input> {
        Ok(Input::Button {
            button: button_arg(button)?,
            down: false,
        })
    }
    fn scroll_input(notches: i32) -> mlua::Result<Input> {
        Ok(Input::Scroll { notches })
    }
    fn key_down_input(key: u8) -> mlua::Result<Input> {
        Ok(Input::Key { key, down: true })
    }
    fn key_up_input(key: u8) -> mlua::Result<Input> {
        Ok(Input::Key { key, down: false })
    }
    globals.set("mouse_down", input(&lua, pid, &worker, mouse_down_input)?)?;
    globals.set("mouse_up", input(&lua, pid, &worker, mouse_up_input)?)?;
    globals.set("scroll", input(&lua, pid, &worker, scroll_input)?)?;
    globals.set("key_down", input(&lua, pid, &worker, key_down_input)?)?;
    globals.set("key_up", input(&lua, pid, &worker, key_up_input)?)?;

    // The same inputs without waiting; they queue in order and stop on the next blocking input, `input_cancel`, a live menu read or script end.
    globals.set("click_async", input_async(&lua, pid, &worker, click_input)?)?;
    globals.set("move_async", input_async(&lua, pid, &worker, move_input)?)?;
    globals.set(
        "wind_mouse_async",
        input_async(&lua, pid, &worker, wind_mouse_input)?,
    )?;
    globals.set(
        "type_text_async",
        input_async(&lua, pid, &worker, type_input)?,
    )?;
    // Buttons and keys pressed this way stay down until released, cancelled or the script ends, last pressed released first.
    globals.set(
        "mouse_down_async",
        input_async(&lua, pid, &worker, mouse_down_input)?,
    )?;
    globals.set(
        "mouse_up_async",
        input_async(&lua, pid, &worker, mouse_up_input)?,
    )?;
    globals.set(
        "scroll_async",
        input_async(&lua, pid, &worker, scroll_input)?,
    )?;
    globals.set(
        "key_down_async",
        input_async(&lua, pid, &worker, key_down_input)?,
    )?;
    globals.set(
        "key_up_async",
        input_async(&lua, pid, &worker, key_up_input)?,
    )?;
    // A pause in the queue, for holding a key or a button for a while.
    globals.set(
        "sleep_async",
        action(&lua, pid, {
            let worker = worker.clone();
            move |_, ms: u32| {
                Ok(worker
                    .as_ref()
                    .map_or(Err(NO_CLIENT.into()), |worker| worker.push(Job::Wait(ms))))
            }
        })?,
    )?;
    globals.set(
        "input_busy",
        lua.create_function({
            let worker = worker.clone();
            move |_, (): ()| Ok(worker.as_ref().is_some_and(|worker| worker.busy()))
        })?,
    )?;
    // Returns the error of an async input that failed, if any.
    globals.set(
        "input_wait",
        lua.create_function({
            let worker = worker.clone();
            move |_, (): ()| {
                Ok(worker.as_ref().and_then(|worker| {
                    worker.wait();
                    worker.take_error()
                }))
            }
        })?,
    )?;
    globals.set(
        "input_error",
        lua.create_function({
            let worker = worker.clone();
            move |_, (): ()| Ok(worker.as_ref().and_then(|worker| worker.take_error()))
        })?,
    )?;
    globals.set(
        "input_cancel",
        lua.create_function({
            let worker = worker.clone();
            move |_, (): ()| {
                cancel_input(&worker);
                Ok(())
            }
        })?,
    )?;

    globals.set(
        "draw_rect",
        action(
            &lua,
            pid,
            |pid, (x1, y1, x2, y2, r, g, b, a): (i32, i32, i32, i32, u8, u8, u8, Option<u8>)| {
                let color = Rgba {
                    r,
                    g,
                    b,
                    a: a.unwrap_or(255),
                };
                Ok(draw(
                    pid,
                    &Overlay::Rect {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    },
                ))
            },
        )?,
    )?;
    globals.set(
        "draw_line",
        action(
            &lua,
            pid,
            |pid, (x1, y1, x2, y2, r, g, b, a): (i32, i32, i32, i32, u8, u8, u8, Option<u8>)| {
                let color = Rgba {
                    r,
                    g,
                    b,
                    a: a.unwrap_or(255),
                };
                Ok(draw(
                    pid,
                    &Overlay::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    },
                ))
            },
        )?,
    )?;
    globals.set(
        "draw_lines",
        action(
            &lua,
            pid,
            |pid, (lines, r, g, b, a): (Vec<mlua::Table>, u8, u8, u8, Option<u8>)| {
                let lines = lines
                    .iter()
                    .map(|l| {
                        Ok([1, 2, 3, 4]
                            .map(|i| l.get::<f64>(i).map(|v| v.round() as i32))
                            .into_iter()
                            .collect::<mlua::Result<Vec<_>>>()?
                            .try_into()
                            .unwrap())
                    })
                    .collect::<mlua::Result<Vec<[i32; 4]>>>()?;
                Ok(draw_lines(
                    pid,
                    &lines,
                    Rgba {
                        r,
                        g,
                        b,
                        a: a.unwrap_or(255),
                    },
                ))
            },
        )?,
    )?;
    globals.set(
        "clear_overlay",
        action(&lua, pid, |pid, (): ()| Ok(draw(pid, &Overlay::Clear)))?,
    )?;
    globals.set(
        "draw_batch",
        action(&lua, pid, |pid, shapes: Vec<mlua::Table>| {
            let overlays = shapes
                .iter()
                .map(overlay_arg)
                .collect::<mlua::Result<Vec<_>>>()?;
            Ok(draw_batch(pid, &overlays))
        })?,
    )?;

    lua.sandbox(true)?;
    let result = lua.load(source).set_name(format!("={name}")).exec();
    if let Some(worker) = &worker {
        worker.shutdown();
    }
    result
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    fn run_lines(source: &str) -> (Vec<String>, mlua::Result<()>) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let result = run("test.luau", source, None, move |line| {
            collected.borrow_mut().push(line)
        });
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn collision_cut_clips_the_area_to_the_scene() {
        // 3 columns of 2, the flag being x * 10 + y
        let scene = (
            100,
            200,
            0,
            (0..3)
                .map(|x| (0..2).map(|y| x * 10 + y).collect())
                .collect(),
        );
        let cut = collision_cut(scene.clone(), Some([101, 150, 105, 201])).unwrap();
        assert_eq!((cut.x, cut.y, cut.width, cut.height), (101, 200, 2, 2));
        assert_eq!(cut.flags, vec![10, 11, 20, 21]);
        assert_eq!(collision_cut(scene.clone(), None).unwrap().flags.len(), 6);
        assert!(collision_cut(scene, Some([90, 190, 95, 195])).is_err());
    }

    #[test]
    fn packet_filters_check_their_ids_and_need_a_client() {
        let error = |source: &str| run_lines(source).1.unwrap_err().to_string();
        assert!(
            error("on_packet({ 2, 300 }, function() end)")
                .contains("packet 300 can't be listened for")
        );
        assert!(error("wait_packet({}, 10)").contains("no packets given"));
        assert!(error("on_packet({ 2 }, function() end)").contains(NO_CLIENT));
        // the failed registration left nothing behind, so sleep is a plain sleep and off_packet knows no id
        let (lines, result) =
            run_lines("pcall(on_packet, { 2 }, function() end) sleep(1) print(off_packet(1))");
        result.unwrap();
        assert_eq!(lines, ["false"]);
    }

    #[test]
    fn a_packet_reaches_luau_with_its_payload_as_bytes() {
        let lua = Lua::new();
        let entry = ipc::packets::Entry {
            sequence: 7,
            time_ms: 1200,
            direction: ipc::packets::direction::SENT,
            opcode: 75,
            length: 300,
            payload: vec![0, 255, 10],
        };
        lua.globals()
            .set("packet", packet_to_lua(&lua, &entry).unwrap())
            .unwrap();
        let seen: String = lua.load("return `{packet.sequence} {packet.time_ms} {packet.sent} {packet.opcode} {packet.length} {#packet.payload} {string.byte(packet.payload, 2)}`").eval().unwrap();
        assert_eq!(seen, "7 1200 true 75 300 3 255");
    }

    #[test]
    fn packet_filters_pick_the_direction() {
        let received = packet_filter("t", &[2, 40], None).unwrap();
        assert!(filter_has(&received, ipc::packets::direction::RECEIVED, 40));
        assert!(!filter_has(&received, ipc::packets::direction::SENT, 40));
        let sent = packet_filter("t", &[75], Some(true)).unwrap();
        assert!(filter_has(&sent, ipc::packets::direction::SENT, 75));
        assert!(!filter_has(&sent, ipc::packets::direction::RECEIVED, 75));
    }

    #[test]
    fn report_stats_writes_a_stats_line_and_rejects_bad_values() {
        let (lines, result) = run_lines("report_stats(100.4, -20, { 1, 2.6 }) print('after')");
        result.unwrap();
        assert_eq!(lines[0], crate::form::stats_line(100, -20, &[1, 3]));
        assert_eq!(lines[1], "after");
        assert!(run_lines("report_stats(-1, 0)").1.is_err());
        assert!(run_lines("report_stats(0/0, 0)").1.is_err());
    }

    #[test]
    fn report_breaks_writes_a_breaks_line() {
        let (lines, result) = run_lines(
            "report_breaks({ breaks = { { remaining = 1500.5, length = 60000 } }, sleeps = {}, active = { kind = 'break', remaining = 10 } })",
        );
        result.unwrap();
        let plan =
            crate::form::parse_breaks(lines[0].strip_prefix(crate::form::BREAKS_PREFIX).unwrap())
                .unwrap();
        assert_eq!(plan.breaks[0].remaining, 1500.5);
        assert!(plan.sleeps.is_empty());
        assert_eq!(plan.active.unwrap().kind, "break");
        assert!(run_lines("report_breaks({ breaks = 5 })").1.is_err());
    }

    #[test]
    fn report_warning_writes_a_warn_line() {
        let (lines, result) = run_lines("report_warning('careful\u{1e}!')");
        result.unwrap();
        assert_eq!(lines[0], format!("{}careful!", crate::form::WARN_PREFIX));
        assert!(run_lines("report_warning(5)").1.is_ok());
        assert!(run_lines("report_warning()").1.is_err());
    }

    #[test]
    fn report_counters_writes_a_counters_line() {
        let (lines, result) = run_lines(
            "report_counters({ { id = 'wins', label = 'Wins', value = 3, percent_of = 'games' }, { id = 'games', label = 'Games', value = 4, hourly = false } })",
        );
        result.unwrap();
        let counters = crate::form::parse_counters(
            lines[0].strip_prefix(crate::form::COUNTERS_PREFIX).unwrap(),
        )
        .unwrap();
        assert_eq!(counters[0].percent_of.as_deref(), Some("games"));
        assert!(counters[0].hourly && !counters[1].hourly);
        assert!(
            run_lines("report_counters({ { label = 'No id', value = 1 } })")
                .1
                .is_err()
        );
        assert!(
            run_lines("report_counters({ { id = 'a', label = 'A', value = 0/0 } })")
                .1
                .is_err()
        );
    }

    // A scratch directory with `files` (path, contents) written under it.
    fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, contents).unwrap();
        }
        dir
    }

    fn run_script_at(path: &Path, source: &str) -> (Vec<String>, mlua::Result<()>) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let name = path.file_name().unwrap().to_string_lossy();
        let result = run_with_form(
            &name,
            source,
            None,
            move |line| collected.borrow_mut().push(line),
            None,
            Some(path),
            None,
        );
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn modules_given_in_memory_are_required_without_files() {
        let dir = scratch(
            "requires_memory",
            &[("lib/host.luau", "return { name = 'host' }")],
        );
        let script = dir.join("scripts/run.luau");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        let modules = HashMap::from([
            (
                "util.luau".to_owned(),
                "local Other = require('./deeper')\nreturn { name = 'util', other = Other.name }"
                    .to_owned(),
            ),
            (
                "deeper.luau".to_owned(),
                "return { name = 'deeper' }".to_owned(),
            ),
        ]);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let source = "local Util = require('./util')\nprint(Util.name, Util.other, require('../lib/host').name, require('./util') == Util)";
        let result = run_with_modules(
            "run.luau",
            source,
            None,
            move |line| collected.borrow_mut().push(line),
            None,
            Some(&script),
            None,
            modules,
        );
        result.unwrap();
        assert_eq!(
            *lines.borrow(),
            vec!["util\tdeeper\thost\ttrue".to_string()]
        );
        assert!(
            !dir.join("scripts/util.luau").exists(),
            "nothing is written out"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn requires_resolve_from_the_requiring_file() {
        let dir = scratch(
            "requires_resolve",
            &[
                ("lib/host.luau", "return { name = 'host' }"),
                (
                    "lib/main/walker.luau",
                    "local Host = require('../host')\nreturn { host = Host, lazy = function() return require('./extra') end }",
                ),
                ("lib/main/extra/init.luau", "return { name = 'extra' }"),
            ],
        );
        let script = dir.join("scripts/run.luau");
        let (lines, result) = run_script_at(
            &script,
            "local Walker = require('../lib/main/walker')\n\
             print(Walker.host == require('../lib/host'), Walker.host.name, Walker.lazy().name)",
        );
        result.unwrap();
        assert_eq!(lines, ["true\thost\textra"]);
    }

    #[test]
    fn lib_modules_require_each_other_through_the_alias() {
        let dir = scratch(
            "lib_alias_from_lib",
            &[
                ("lib/host.luau", "return { name = 'host' }"),
                (
                    "lib/main/deep/walker.luau",
                    "return { host = require('@lib/host'), same = require('@lib/main/other') }",
                ),
                ("lib/main/other.luau", "return { name = 'other' }"),
            ],
        );
        let script = dir.join("scripts/run.luau");
        let (lines, result) = run_script_at(
            &script,
            "local W = require('@lib/main/deep/walker')\n\
             print(W.host == require('../lib/host'), W.host.name, W.same.name)",
        );
        result.unwrap();
        assert_eq!(lines, ["true\thost\tother"]);
    }

    fn run_script_with_lib(
        path: &Path,
        source: &str,
        lib: &Path,
    ) -> (Vec<String>, mlua::Result<()>) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let name = path.file_name().unwrap().to_string_lossy();
        let result = run_with_form(
            &name,
            source,
            None,
            move |line| collected.borrow_mut().push(line),
            None,
            Some(path),
            Some(lib),
        );
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn lib_alias_names_the_path_inside_the_lib() {
        assert_eq!(lib_alias("@lib/main/walker"), Some("main/walker"));
        assert_eq!(lib_alias("../lib/host"), None);
        assert_eq!(lib_alias("@library/host"), None);
        assert_eq!(lib_alias("@lib/"), None);
    }

    #[test]
    fn lib_relative_names_the_path_inside_the_lib() {
        assert_eq!(lib_relative("../lib/main/walker"), Some("main/walker"));
        assert_eq!(lib_relative("./lib/host"), Some("host"));
        assert_eq!(lib_relative("../../lib/host"), Some("host"));
        assert_eq!(lib_relative("lib/host"), None);
        assert_eq!(lib_relative("../library/host"), None);
        assert_eq!(lib_relative("../lib/"), None);
        assert_eq!(lib_relative("./extra"), None);
    }

    #[test]
    fn a_lib_folder_replaces_the_lib_beside_the_script() {
        let dir = scratch(
            "requires_lib_dir",
            &[
                ("lib/host.luau", "return { name = 'beside' }"),
                ("mine/host.luau", "return { name = 'mine' }"),
                (
                    "mine/main/walker.luau",
                    "local Host = require('../host')\nreturn { host = Host }",
                ),
            ],
        );
        let source = "print(require('../lib/host').name, require('../lib/main/walker').host.name)";
        let script = dir.join("scripts/run.luau");

        let (lines, result) = run_script_with_lib(&script, source, &dir.join("mine"));
        result.unwrap();
        assert_eq!(lines, ["mine\tmine"]);

        // Without a lib folder, the lib beside the script is used as before.
        let (lines, result) = run_script_at(&script, "print(require('../lib/host').name)");
        result.unwrap();
        assert_eq!(lines, ["beside"]);
    }

    #[test]
    fn a_module_missing_from_the_lib_folder_is_an_error_naming_it() {
        let dir = scratch(
            "requires_lib_dir_missing",
            &[
                ("mine/host.luau", "return {}"),
                ("lib/nope.luau", "return {}"),
            ],
        );
        let (_, result) = run_script_with_lib(
            &dir.join("scripts/run.luau"),
            "require('../lib/nope')",
            &dir.join("mine"),
        );
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("module '../lib/nope' not found, tried:") && error.contains("mine"),
            "{error}"
        );
    }

    #[test]
    fn missing_relative_requires_name_what_was_tried() {
        let dir = scratch("requires_missing", &[]);
        let (_, result) = run_script_at(&dir.join("run.luau"), "require('../lib/nope')");
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("module '../lib/nope' not found, tried:"),
            "{error}"
        );
        assert!(error.contains("nope.luau"), "{error}");
    }

    #[test]
    fn errors_in_disk_modules_name_their_file() {
        let dir = scratch("requires_errors", &[("lib/bad.luau", "\nerror('boom')")]);
        let (_, result) = run_script_at(&dir.join("run.luau"), "require('./lib/bad')");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("bad.luau:2: boom"), "{error}");
    }

    #[test]
    fn prints_and_finishes() {
        let (lines, result) = run_lines("print('a', 1, nil, true) print(2 + 3)");
        assert_eq!(lines, ["a\t1\tnil\ttrue", "5"]);
        assert!(result.is_ok());
    }

    #[test]
    fn a_script_cannot_write_the_loaders_lines() {
        let (lines, result) = run_lines("print('\\x1ePINREQ abc', 'x\\x1eFORM {}')");
        result.unwrap();
        assert_eq!(lines, ["PINREQ abc\txFORM {}"]);
    }

    #[test]
    fn errors_name_the_script_and_line() {
        let (_, result) = run_lines("print('ok')\nerror('boom')");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("test.luau:2: boom"), "{error}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn requires_lib_modules() {
        let (lines, result) = run_lines(
            "local GameState = require('game_state')\n\
             print(GameState.LOGGED_IN, GameState.name(GameState.LOGGED_IN))\n\
             local Position = require('position')\n\
             print(Position.new(0, 0, 3213, 3428):chunk():id())\n\
             print(require('game_state') == GameState)",
        );
        assert_eq!(lines, ["30\tlogged in", "12853", "true"]);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn skills_sums_experience() {
        let (lines, result) = run_lines(
            "local Stats = require('stats')\n\
             print(Stats.total_experience(nil))\n\
             print(Stats.total_experience({ { name = 'Attack', level = 1, real_level = 1, xp = 0 } }))\n\
             print(Stats.total_experience({\n\
                 { name = 'Attack', level = 99, real_level = 99, xp = 13034431 },\n\
                 { name = 'Strength', level = 1, real_level = 1, xp = 0 },\n\
             }))",
        );
        assert_eq!(lines, ["nil", "0", "13034431"]);
        assert!(result.is_ok());
    }

    #[test]
    fn unix_time_is_seconds_since_the_epoch() {
        let (lines, result) = run_lines("print(unix_time() > 1700000000 and unix_time() < 1e11)");
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(lines, vec!["true"]);
    }

    #[test]
    fn clock_is_local_seconds_since_midnight() {
        let (lines, result) = run_lines("local c = clock() print(c >= 0 and c < 86400)");
        assert_eq!(lines, ["true"]);
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn login_screen_buttons_follow_the_clients_layout() {
        let (lines, result) = run_lines(
            "local TitleScreen = require('titlescreen')\n\
             local login = { screen = 24, panel_x = 318, box_x = 520, center_x = 700, y = 171 }\n\
             local b = TitleScreen.button(login, 'try_again')\n\
             print(b.label, b.x1, b.y1, b.x2, b.y2, b.center_x, b.center_y)\n\
             login.screen = TitleScreen.PLAY_NOW\n\
             print(TitleScreen.button(login, 'try_again'), TitleScreen.name(login.screen))\n\
             b = TitleScreen.buttons(login).play_now\n\
             print(b.x1, b.y1, b.x2, b.y2)\n\
             login.y = 100\n\
             b = TitleScreen.button(login, 'play_now')\n\
             print(b.y1, b.y2)\n\
             print(next(TitleScreen.buttons({ screen = 1000, panel_x = 0, box_x = 0, center_x = 0, y = 0 })))",
        );
        assert_eq!(
            lines,
            [
                "Try again\t625\t291\t775\t331\t700\t311",
                "nil\tplay now",
                "591\t209\t809\t277",
                "138\t206",
                "nil"
            ]
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn lobby_needs_a_client_and_knows_its_screen() {
        let (lines, result) = run_lines(
            "local WelcomeScreen = require('welcome_screen')\n\
             print(WelcomeScreen.is_open({ game_state = 30, root_interface = 165 }), WelcomeScreen.is_open({ game_state = 30, root_interface = 161 }))\n\
             print(pcall(WelcomeScreen.play))\n\
             print(widget(378, 72))",
        );
        assert_eq!(lines[0], "true\tfalse");
        assert!(
            lines[1].starts_with("false") && lines[1].contains("no client selected"),
            "{lines:?}"
        );
        assert_eq!(lines[2], "nil\tno client selected");
        assert_eq!(lines.len(), 3);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn every_lib_module_loads() {
        let source: String = LIB_MODULES
            .iter()
            .map(|(name, _, _)| format!("assert(type(require('{name}')) == 'table', '{name}')\n"))
            .collect();
        let (_, result) = run_lines(&source);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn every_lib_file_is_a_lib_module() {
        fn files(dir: &std::path::Path, found: &mut Vec<String>, root: &std::path::Path) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files(&path, found, root);
                } else if path.extension().is_some_and(|e| e == "luau") {
                    let relative = path.strip_prefix(root).unwrap();
                    found.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib");
        let mut found = vec!["host.luau".to_owned()];
        files(&lib.join("main"), &mut found, &lib);
        files(&lib.join("utils"), &mut found, &lib);
        let missing: Vec<&String> = found
            .iter()
            .filter(|file| !LIB_MODULES.iter().any(|(_, path, _)| path == file))
            .collect();
        assert!(missing.is_empty(), "not in LIB_MODULES: {missing:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn items_match_by_id_or_name() {
        let (lines, result) = run_lines(
            "local Item = require('item')\n\
             print(table.concat(Item.spread('Prayer potion(4..2)'), ','))\n\
             print(#Item.spread('Bucket'), Item.spread(1925)[1], Item.has_amount('Amulet of glory(6)'))\n\
             local m = Item.matcher({ 'bucket', 1929, '954', 'Prayer potion(1..2)' })\n\
             print(Item.matches(m, 1925, 'Bucket'), Item.matches(m, 1929, nil), Item.matches(m, 954, nil), Item.matches(m, 311, 'Harpoon'))\n\
             print(Item.matches(m, 1, 'PRAYER POTION(2)'), Item.needs_names(m), Item.needs_names(Item.matcher(311)))\n\
             print(require('inventory').count('Bucket'))\n\
             print(pcall(require('bankmain').is_open))\n\
             print(pcall(require('walker').click_tile, { x = 3156, y = 2841 }))",
        );
        assert_eq!(
            lines[..5],
            [
                "Prayer potion(4),Prayer potion(3),Prayer potion(2)",
                "1\t1925\ttrue",
                "true\ttrue\ttrue\tfalse",
                "true\ttrue\tfalse",
                "0",
            ]
        );
        assert_eq!(lines.len(), 7);
        for line in &lines[5..] {
            assert!(
                line.starts_with("false") && line.contains("no client selected"),
                "{lines:?}"
            );
        }
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn pure_lib_helpers() {
        let (lines, result) = run_lines(
            "local SidePanels = require('side_panels')\n\
             print(SidePanels.resolve('Inventory'), SidePanels.resolve(13), SidePanels.resolve('bank'))\n\
             local Varps = require('varps')\n\
             print(Varps.poison_state(0), Varps.poison_state(6), Varps.poison_state(1000000))\n\
             local c = Varps.changes({ [0] = 1, [173] = 0 }, { [0] = 1, [173] = 1 })\n\
             print(c[173].before, c[173].after, c[0])\n\
             local Widgets = require('widgets')\n\
             local w = { x = 565, y = 292, width = 229, height = 90 }\n\
             print(Widgets.center(w))\n\
             print(Widgets.contains(w, 565, 292), Widgets.contains(w, 794, 292))\n\
             local Stats = require('stats')\n\
             print(Stats.total_level({ { real_level = 99 }, { real_level = 1 } }))\n\
             local Players = require('players')\n\
             print(Players.distance({ x = 0, y = 0 }, { x = 3, y = -5 }))",
        );
        // The log line carries the time the script has been running, so only its prefix and message are fixed.
        assert!(
            lines[0].starts_with("[SidePanels][")
                && lines[0].ends_with("]: unknown side panel: bank"),
            "{lines:?}"
        );
        assert_eq!(
            lines[1..],
            [
                "3\t13\tnil",
                "healthy\tpoisoned\tenvenomed",
                "0\t1\tnil",
                "679\t337",
                "true\tfalse",
                "100",
                "5",
            ]
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn lib_readers_need_a_client() {
        let (lines, result) = run_lines(
            "print(pcall(require('player').name))\n\
             print(require('varps').get(173))\n\
             print(pcall(require('side_panels').open, 'stats'))\n\
             print(pcall(require('ticks').wait, 1))\n\
             print(pcall(require('mouse').teleport, { x = 1, y = 2 }))\n\
             print(pcall(require('mouse').move, { x = 1, y = 2 }))\n\
             print(pcall(require('mouse').hold))\n\
             print(pcall(require('mouse').scroll, 2, true))\n\
             print(pcall(require('keyboard').key_press, require('keyboard').keys.RETURN))\n\
             print(pcall(require('keyboard').send, 'hi'))",
        );
        assert_eq!(lines[1], "nil\tno client selected");
        for line in lines
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != 1)
            .map(|(_, line)| line)
        {
            assert!(
                line.starts_with("false") && line.contains("no client selected"),
                "{lines:?}"
            );
        }
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn biometrics_follow_the_username() {
        let (lines, result) = run_lines(
            "local B = require('biometrics')\n\
             print(B.hash_string('player'), B.murmur2a('abcd', 7), B.murmur2a('', 0), B.murmur2a('abcdefghijklmnopqrs', 0))\n\
             B.setup('player')\n\
             local h = B.hash()\n\
             print(#h, h[1], h[2], h[3], h[4])\n\
             local first = B.sleep_hour()\n\
             B.setup('player')\n\
             assert(B.sleep_hour() == first)\n\
             assert(B.sleep_length() >= 6 and B.sleep_length() <= 10)\n\
             for _ = 1, 500 do\n\
                 local v = B.random_mode_integer(7, 3, 15)\n\
                 assert(v >= 3 and v <= 15, v)\n\
             end\n\
             print(first)",
        );
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines[0], "3594132001\t139965599\t0\t1770672524");
        assert_eq!(lines[2], "24\t33\t26\t58\t214");
        assert_eq!(lines[4], "23:38:57");
        assert!(result.is_ok(), "{result:?}");
    }

    // The only test that sets the global break config, so it can't race another.
    #[test]
    #[cfg(feature = "embedded-lib")]
    fn breaks_follow_the_loaders_config_and_keep_their_time_when_it_changes() {
        let (lines, result) = run_lines(
            "local A = require('antiban')\n\
             local function plan(json) test_set_antiban(json); assert(A.sync_config()) end\n\
             plan('{\"breaks\":[{\"interval\":600000,\"length\":60000,\"randomness\":0.0,\"logout_chance\":0.0}],\"sleeps\":[{}]}')\n\
             assert(not A.sync_config())\n\
             assert(#A.breaks == 1 and #A.sleeps == 1)\n\
             local first, sleep_due = A.breaks[1], A.sleeps[1].next\n\
             assert(first.length == 60000 and first.logout_chance == 0)\n\
             assert(A.sleeps[1].length >= 6 * A.HOUR and A.sleeps[1].length <= 10 * A.HOUR)\n\
             local due = first.next\n\
             -- only the length changed: the due time stays\n\
             plan('{\"breaks\":[{\"interval\":600000,\"length\":90000,\"randomness\":0.0,\"logout_chance\":0.0}],\"sleeps\":[{}]}')\n\
             assert(A.breaks[1].length == 90000 and A.breaks[1].next == due)\n\
             assert(A.sleeps[1].next == sleep_due)\n\
             -- a new interval re-plans, and a second break gets its own plan\n\
             plan('{\"breaks\":[{\"interval\":300000,\"length\":90000,\"randomness\":0.0,\"logout_chance\":0.5},{\"interval\":60000,\"length\":10000}],\"sleeps\":[]}')\n\
             assert(#A.breaks == 2 and #A.sleeps == 0)\n\
             assert(A.breaks[1].next ~= due and A.breaks[1].logout_chance == 0.5)\n\
             assert(A.breaks[2].logout_chance == 0.01)\n\
             -- skipping: the next occurrence moves one interval on, and an unknown one is ignored\n\
             local second = A.breaks[2].next\n\
             test_skip('break', 1); test_skip('break', 7); test_skip('sleep', 0)\n\
             assert(A.sync_config())\n\
             assert(A.breaks[2].next > second + 40000 and A.breaks[1].next < second + 400000)\n\
             assert(A.breaks[2].next < second + 80000)\n\
             assert(not A.sync_config())\n\
             -- off: nothing is ever due\n\
             plan('{\"breaks_enabled\":false,\"sleeps_enabled\":false}')\n\
             assert(#A.breaks == 0 and #A.sleeps == 0 and not A.break_due())\n\
             print('ok')",
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    // The only test that sets the transports, so it can't race another.
    #[test]
    #[cfg(feature = "embedded-lib")]
    fn transports_come_from_the_loader_and_spell_names_split_into_option() {
        let (lines, result) = run_lines(
            "assert(transports().source == 'none' and #transports().transports == 0)\n\
             test_set_transports('{\"source\":\"cache\",\"transports\":[{\"kind\":\"teleportation_spells\",\"destination\":{\"x\":3213,\"y\":3424,\"plane\":0},\"name\":\"Varrock Teleport\",\"duration\":4,\"skills\":[],\"quests\":[],\"varbits\":[{\"id\":4070,\"op\":\"=\",\"value\":0}],\"varps\":[],\"items\":\"\",\"consumable\":false}],\"items\":[{\"name\":\"COINS\",\"ids\":[995]},{\"name\":\"AXE\",\"ids\":[1351,1349]}]}')\n\
             local data = transports()\n\
             assert(data.source == 'cache' and data.version == 1 and #data.transports == 1)\n\
             assert(data.transports[1].varbits[1].id == 4070 and data.transports[1].origin == nil)\n\
             local T = require('transports')\n\
             local a, ao = T.split('Varrock Teleport: Grand Exchange')\n\
             assert(a == 'Varrock Teleport' and ao == 'Grand Exchange')\n\
             local b, bo = T.split('Teleport to House (Inside)')\n\
             assert(b == 'Teleport to House' and bo == 'Inside')\n\
             local c, co = T.split('Falador Teleport')\n\
             assert(c == 'Falador Teleport' and co == nil)\n\
             assert(T.wilderness_level(3100, 3600) == 11 and T.wilderness_level(3200, 3200) == 0)\n\
             local function need(...) local list = {} for _, s in ipairs({ ... }) do list[#list + 1] = { name = s[1], level = s[2] } end return { skills = list } end\n\
             local have = { magic = 60, total = 1500, combat = 80 }\n\
             assert(T.skills_met(need(), have))\n\
             assert(T.skills_met(need({ 'Magic', 60 }), have) and not T.skills_met(need({ 'Magic', 61 }), have))\n\
             assert(T.skills_met(need({ 'Total', 1500 }, { 'Combat', 70 }), have))\n\
             assert(not T.skills_met(need({ 'Magic', 25 }, { 'Sailing', 1 }), have), 'an unknown skill is not met')\n\
             assert(T.walk_ticks({ x = 0, y = 0 }, { x = 100, y = 40 }) == 60)\n\
             -- every kind of transport is tried 3 times, then left out\n\
             T.RETRY_PAUSE_MS = 0\n\
             local calls = 0\n\
             T.runners.spell = function() calls += 1 return false end\n\
             local fake = { kind = 'spell', transport = { name = 'Fake Teleport' }, spell = 'Fake', ticks = 1, saved = 1 }\n\
             assert(not T.failed_lately(fake))\n\
             assert(not T.use(fake) and calls == 3, 'three attempts')\n\
             assert(T.failed_lately(fake), 'then marked failed')\n\
             calls = 0\n\
             T.runners.charter = function() calls += 1 return calls == 2 end\n\
             local ship = { kind = 'charter', transport = { name = '1: Somewhere', origin = { x = 1, y = 2, plane = 0 } }, spell = '', ticks = 1, saved = 1 }\n\
             assert(T.use(ship) and calls == 2, 'stops at the first success')\n\
             assert(not T.failed_lately(ship), 'a success is not marked')\n\
             calls = 0\n\
             T.runners.item = function() calls += 1 return false, true end\n\
             local piece = { kind = 'item', transport = { name = 'Games necklace: Burthorpe' }, spell = 'Games necklace', option = 'Burthorpe', ticks = 1, saved = 1 }\n\
             assert(not T.use(piece) and calls == 1 and T.failed_lately(piece), 'an item that is not offered is not tried again')\n\
             calls = 0\n\
             T.runners.magic_mushtrees = function() calls += 1 return false end\n\
             local ride = { kind = 'magic_mushtrees', id = 'magic_mushtrees 3764,3879 Sticky Swamp', transport = { name = '3. Sticky Swamp', origin = { x = 3764, y = 3879, plane = 1 } }, spell = '', ticks = 1, saved = 1 }\n\
             assert(not T.use(ride) and calls == 3 and T.failed_lately(ride), 'a mushtree ride is tried 3 times, then left out')\n\
             local menu = { { text = 'Brimhaven' }, { text = 'Rimmington' }, { text = 'No thanks' } }\n\
             assert(T.dialogue_pick(menu, { 'brimhaven' }) == 'Brimhaven', 'the option that names the destination')\n\
             assert(T.dialogue_pick({ { text = 'Yes please' }, { text = 'No' } }, { 'musa point' }) == 'Yes please', 'else a yes')\n\
             assert(T.dialogue_pick({ { text = 'What is this place?' }, { text = 'Bye' } }, { 'musa point' }) == nil, 'else nothing')\n\
             assert(T.dialogue_pick({ { text = 'Not now' } }, { 'no' }) == nil, 'a hint shorter than four letters is not matched')\n\
             local coins = { items = 'COINS=2500' }\n\
             local pay = { [995] = 2500 }\n\
             assert(T.items_met(coins, pay) and not T.items_met(coins, { [995] = 2499 }) and not T.items_met(coins, {}))\n\
             assert(T.items_met({ items = '' }, {}), 'no items needed')\n\
             assert(T.items_met({ items = 'AXE=1' }, { [1351] = 1 }), 'a named group counts any of its ids')\n\
             assert(T.items_met({ items = '1351=1&&995=10' }, { [1351] = 1, [995] = 10 }) and not T.items_met({ items = '1351=1&&995=10' }, { [1351] = 1 }))\n\
             assert(T.items_met({ items = '1351=1||995=10' }, { [995] = 10 }) and not T.items_met({ items = '1351=1||995=10' }, {}))\n\
             assert(not T.items_met({ items = 'HEADSLOT=0' }, {}) and not T.items_met({ items = 'UNLOCK_CANOE_AXE=1' }, {}), 'slots and unlocks are not counted')\n\
             assert(T.items_met({ items = 'AXE=1|UNLOCK_CANOE_AXE=1' }, { [1351] = 1 }), 'an axe carried makes the unlock unnecessary')\n\
             print('ok')",
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    // The game stood in for: rows, levels, what is held, and two landmasses split at x = 5000.
    #[cfg(feature = "embedded-lib")]
    const CHOOSER_STAGE: &str = r#"
local Host = require('@lib/host')
local T = require('@lib/main/transports')
local Magic = require('@lib/main/interfaces/gametabs/magic')
local Journal = require('@lib/main/interfaces/gametabs/journal')
local Player = require('@lib/main/player')
Player.position = function() return { x = 1, y = 1, plane = 0 } end
Magic.spell = function(name) return name end
Magic.can_cast = function() return true end
Journal.is_quest_complete = function() return true end
local function row(kind, name, origin, destination, more)
    local t = { kind = kind, name = name, origin = origin, destination = destination, duration = 4, skills = {}, quests = {}, varbits = {}, varps = {}, items = '', consumable = false, menu = '' }
    for k, v in pairs(more or {}) do t[k] = v end
    return t
end
local function tile(x, y, plane) return { x = x, y = y, plane = plane or 0 } end
local stage = { rows = {}, inventory = {}, worn = {}, plan_reaches = false, nav_plans = 0 }
Host.transports = function() return { source = 'test', version = 1, transports = stage.rows, items = { { name = 'ROPE', ids = { 954 } }, { name = 'CROSSBOW', ids = { 837 } }, { name = 'MITH_GRAPPLE', ids = { 9419 } }, { name = 'AXE', ids = { 1351 } }, { name = 'COINS', ids = { 995 } } } } end
Host.game_state = function() return { game_state = 30, skills = { { name = 'Magic', level = 60, real_level = 60 }, { name = 'Agility', level = 55, real_level = 50 } } } end
Host.varbit = function() return 0 end
Host.varp = function() return 0 end
Host.item_container = function(id) return if id == T.INVENTORY then stage.inventory elseif id == T.WORN then stage.worn else {} end
Host.landmasses = function(points)
    local out = {}
    for i, p in ipairs(points) do out[i] = if p.x >= 5000 then 2 else 1 end
    return out
end
Host.nav_plan = function() stage.nav_plans += 1 return { reached = stage.plan_reaches, tiles = {}, actions = {} } end
Host.world_route = function(a, b) return { known = true, reachable = (a.x >= 5000) == (b.x >= 5000) } end
local home = tile(3000, 3000)
local far = tile(3800, 3000)
local island = tile(5400, 3000)
"#;

    #[cfg(feature = "embedded-lib")]
    fn chooser_test(body: &str) {
        let (lines, result) = run_lines(&format!("{CHOOSER_STAGE}\n{body}\nprint('ok')"));
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_chooser_walks_when_near_teleports_when_far_and_skips_unmet_rows() {
        chooser_test(
            r#"
stage.rows = {
    row('teleportation_spells', 'Good Teleport', nil, tile(3805, 3000)),
    row('teleportation_spells', 'Too Hard Teleport', nil, tile(3800, 3001), { skills = { { name = 'Magic', level = 61 } } }),
}
assert(T.choose(home, tile(3010, 3000)) == nil, 'near: walk')
local c = T.choose(home, far)
assert(c and c.kind == 'spell' and c.spell == 'Good Teleport', 'far: the spell whose skills are met')
assert(c.id == 'Good Teleport' and c.saved > 30)
assert(T.choose(home, far, { exclude = { spells = true } }) == nil, 'a group can be excluded')
assert(T.choose(home, far, { exclude = { ['Good Teleport'] = true } }) == nil, 'and a name')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn water_is_crossed_by_a_ship_and_a_goal_the_planner_reaches_is_walked() {
        chooser_test(
            r#"
assert(T.choose(home, island) == nil, 'no way at all: nothing to choose')
local ship = row('charter_ships', '1: Isle', tile(3010, 3000), tile(5401, 3000), { items = 'COINS=100' })
stage.rows = { ship }
assert(T.choose(home, island) == nil, 'the fare is not held')
stage.inventory = { { id = 995, quantity = 100, slot = 0 } }
local c = T.choose(home, island)
assert(c and c.kind == 'charter' and c.transport.origin.x == 3010 and c.saved == 1e9, 'a ship across the water')
assert(stage.nav_plans == 0, 'an island this far is not asked of the planner')
local shore = tile(4990, 3000)
local across = tile(5010, 3000)
stage.plan_reaches = false
assert(T.choose(shore, across) ~= nil and stage.nav_plans == 1, 'cut off and the planner agrees: the ship')
stage.plan_reaches = true
assert(T.choose(shore, across) == nil, 'cut off by landmass but the planner gets there: walk')
"#,
        );
    }

    const RANDOMS_STAGE: &str = r#"
local Host = require('@lib/host')
local ChatOptions = require('@lib/main/interfaces/chatbox/chat_options')
local Lamp = require('@lib/main/interfaces/mainscreen/lamp')
local Npcs = require('@lib/main/entities/npcs')
local Randoms = require('@lib/main/antiban/randoms')
local Tasks = require('@lib/main/antiban/antiban_tasks')
Randoms.GENIE_SETTLE_MS = 0
Randoms.GENIE_TIMEOUT_MS = 50
Randoms.POLL_MS = 1
local stage = { npcs = {}, inventory = {}, skill = nil, why = nil, talked = 0, used = nil, continued = 0, message = false, gives = true }
local ME = 362
Host.game_state = function() return { game_state = 30, player_name = 'Me', account_name = 'me' } end
Host.players = function() return { { name = 'Other', index = 7 }, { name = 'Me', index = ME } } end
Host.npcs = function() return stage.npcs end
Host.item_container = function() return stage.inventory end
Host.lamp_skill = function() return stage.skill, stage.why end
Npcs.interact = function(npc, option)
    assert(option == 'Talk-to')
    stage.talked += 1
    if stage.gives then table.insert(stage.inventory, { id = Randoms.GENIE_LAMP, quantity = 1, slot = 0 }) end
    return true
end
ChatOptions.contains_continue = function() return stage.message end
ChatOptions.continue_chat = function() stage.continued += 1 stage.message = false return true end
Lamp.use = function(skill, item) stage.used = skill assert(item == Randoms.GENIE_LAMP) return true end
local function genie(player) return { id = Randoms.GENIE, index = 1, name = 'Genie', interacting_player = player } end
"#;

    #[cfg(feature = "embedded-lib")]
    fn randoms_test(body: &str) {
        let (lines, result) = run_lines(&format!("{RANDOMS_STAGE}\n{body}\nprint('ok')"));
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_random_event_is_only_handled_when_it_is_ours() {
        randoms_test(
            r#"
assert(Randoms.handle() == false, 'nothing around')
stage.npcs = { { id = 3106, index = 2, name = 'Man', interacting_player = ME } }
assert(Randoms.handle() == false, 'an NPC that is no event')
stage.npcs = { genie(7) }
assert(Randoms.handle() == false and stage.talked == 0, "another player's genie")
stage.npcs = { genie(-1) }
assert(Randoms.handle() == false and stage.talked == 0, 'a genie talking to nobody')
stage.npcs = { genie(ME) }
Tasks.handle_randoms()
assert(stage.talked == 1, 'the task handles ours')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn giles_is_handled_by_the_certers_quiz() {
        randoms_test(
            r#"
local Certer = require('@lib/main/interfaces/mainscreen/certer')
local opened, solved = 0, 0
Certer.open = function(npc) assert(npc.id == Randoms.GILES) opened += 1 return true end
Certer.solve = function() solved += 1 return true end
stage.npcs = { { id = Randoms.GILES, index = 1, name = 'Giles', interacting_player = 7 } }
assert(Randoms.handle() == false and opened == 0, "another player's Giles")
stage.npcs[1].interacting_player = ME
assert(Randoms.handle() == true and opened == 1 and solved == 1, 'ours is answered')
for _, id in { Randoms.NILES, Randoms.MILES } do
    stage.npcs[1].id = id
    Certer.open = function(npc) assert(npc.id == id) opened += 1 return true end
    assert(Randoms.handle() == true and opened >= 2, 'and so are the other certers')
end
Certer.solve = function() return false, 'no answer fits the picture' end
assert(Randoms.handle() == false, 'and it says so when it could not be')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_certers_answer_comes_from_the_pictures_model() {
        let (lines, result) = run_lines(
            r#"
local Host = require('@lib/host')
local Toplevel = require('@lib/main/interfaces/mainscreen/toplevel')
Toplevel.is_open = function() return true end
local model = 8833
local texts = { 'A helmet.', 'A fish.', 'A bowl.' }
Host.widget_model = function(group, child) assert(group == 184 and child == 7) return { kind = 1, id = model } end
Host.widget = function(_, child)
    if child >= 1 and child <= 3 then return { text = texts[child], hidden = false } end
    if child >= 8 and child <= 10 then return { hidden = false } end
end
local Certer = require('@lib/main/interfaces/mainscreen/certer')
assert(Certer.correct().letter == 'A', 'armour is the helmet')
model = 8829
assert(Certer.correct().letter == 'B', 'food is the fish')
model = 8834
assert(Certer.correct() == nil, 'nothing fits a jewel: no guess')
model = 1
assert(Certer.correct() == nil and Certer.category() == nil, 'an unknown picture: no guess')
texts = { 'A ring.', 'A necklace.', 'A bowl.' }
model = 8834
assert(Certer.correct() == nil, 'two fit: no guess')
texts = { 'A string.', 'A ring.', 'A bowl.' }
assert(Certer.correct().letter == 'B', 'a word, not part of one')
print('ok')
"#,
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_genies_lamp_goes_on_the_accounts_skill_or_is_kept() {
        randoms_test(
            r#"
stage.npcs = { genie(ME) }
stage.skill = 'Slayer'
stage.message = true
assert(Randoms.handle() == true and stage.used == 'Slayer' and stage.continued == 1, 'spent, after the dialogue')
stage.used, stage.skill = nil, nil
assert(Randoms.handle() == true and stage.used == nil, 'no skill set: the lamp is kept')
stage.skill = 'Nonsense'
assert(Randoms.handle() == true and stage.used == nil, 'an unknown skill: kept')
stage.skill, stage.why = nil, 'locked'
assert(Randoms.handle() == true and stage.used == nil, 'the loader cannot say: kept')
stage.why, stage.gives, stage.skill = nil, false, 'Slayer'
assert(Randoms.handle() == false and stage.used == nil, 'no lamp came')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn carpets_are_preferred_and_free_with_the_desert_hard_diary() {
        chooser_test(
            r#"
local Diaries = require('@lib/main/diaries')
local hard = false
Diaries.tier = function(region, tier) assert(region == 'desert' and tier == 'hard') return hard end
stage.rows = { row('magic_carpets', '1: Oasis', tile(3000, 3000), tile(3060, 3000), { menu = 'Travel Rug Merchant 17', duration = 30, items = 'COINS=200' }) }
local oasis = tile(3060, 3000)
assert(T.choose(home, oasis) == nil, 'no fare and no diary')
hard = true
local c = T.choose(home, oasis)
assert(c and c.transport.kind == 'magic_carpets', 'the diary makes it free')
hard = false
stage.inventory = { { id = 995, quantity = 200, slot = 0 } }
c = T.choose(home, oasis)
assert(c and c.saved < T.MIN_SAVING_TICKS, 'the fare will do, and it needs less than the usual saving')
assert(T.choose(home, oasis, { min_saving_ticks = 30 }) == nil, 'unless a plan asks for one')
assert(T.choose(home, tile(3015, 3000)) == nil, 'a short walk is still walked')
assert(T.choose(home, oasis, { exclude = { ride = true } }) == nil, 'and it can be left out')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn types_the_user_turned_off_are_left_out_and_can_be_required() {
        chooser_test(
            r#"
stage.rows = {
    row('teleportation_spells', 'Good Teleport', nil, far),
    row('teleportation_spells_home', 'Lumbridge Home Teleport', nil, tile(3900, 3000)),
}
local off = {}
Host.transport_settings = function() return { off = off } end
assert(T.is_enabled('spells'))
assert(T.choose(home, far) ~= nil, 'on')
off = { 'spells' }
assert(not T.is_enabled('spells') and T.is_enabled('home_teleports'))
assert(T.choose(home, far, { exclude = { ['Lumbridge Home Teleport'] = true } }) == nil, 'off')
assert(T.choose(home, tile(3900, 3000)) ~= nil, 'home teleports are their own type')
local ok, why = pcall(T.require_enabled, 'spells')
assert(not ok and string.find(tostring(why), 'turned off', 1, true), 'a script that needs it stops')
assert(pcall(T.require_enabled, 'home_teleports'), 'and one that is on is fine')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_goal_with_no_way_stops_the_walk() {
        chooser_test(
            r#"
local Walker = require('@lib/main/walker')
Walker.here = function() return home end
local walked = 0
stage.plan_reaches = false
local ok, why = pcall(Walker.walk, island, { plane = 0 })
assert(not ok and string.find(tostring(why), 'no way from', 1, true), 'it raises, naming the tiles: ' .. tostring(why))
assert(T.walkable(home, far), 'a way on foot is fine')
assert(not T.walkable(home, island), 'and none is not')
stage.plan_reaches = true
assert(T.walkable(home, tile(5010, 3000)) == false, 'too far for the planner to say otherwise')
local shore = tile(4990, 3000)
assert(T.walkable(shore, tile(5010, 3000)), 'the planner can find a close way the map misses')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_comments_requirement_binds_by_its_variable_and_unlocks_are_read() {
        chooser_test(
            r#"
local vars = {}
Host.varbit = function(id) return vars[id] or 0 end
local karamja = 'Karamja (varbit:4566=0?COINS=200)'
local toll = 'Port Phasmatys (varbit:217=7?ECTO_TOKEN=2)'
local carts = 'Carts (varbit:4567=0?COINS=300)'
local stage_rows = {
    row('link_gated', '', tile(5200, 3000), tile(5210, 3000), { note = carts }),
    row('transports', '', tile(5220, 3000), tile(5900, 3000), { note = carts, items = 'COINS=300' }),
    row('link_gated', '', tile(5100, 3000), tile(5110, 3000), { note = karamja }),
    row('link_gated', '', tile(5120, 3000), tile(5130, 3000), { note = karamja, items = 'COINS=200' }),
    row('link_gated', '', tile(3010, 3000), tile(3020, 3000), { note = toll }),
    row('link_gated', '', tile(3030, 3000), tile(3040, 3000)),
    row('link_gated', '', tile(3050, 3000), tile(3060, 3000), { note = 'What (questpoints>5?COINS=1)' }),
    row('link_gated', '', tile(3070, 3000), tile(3080, 3000), { items = 'UNLOCK_KARAMJA_ELITE=1||6306=100' }),
    row('link_gated', '', tile(3090, 3000), tile(3100, 3000), { items = '6570=1|UNLOCK_MOR_UL_REK=1' }),
    row('link_gated', '', tile(3110, 3000), tile(3120, 3000), { items = 'UNLOCK_CANOE_AXE=1' }),
}
stage.rows = stage_rows
local function allowed()
    local keys = {}
    for _, k in ipairs(T.allowed_climbs()) do keys[string.match(k, '^[^|]+')] = true end
    return function(x) return keys[x .. ',3000,0>' .. (x + 10) .. ',3000,0'] == true end
end
local has = allowed()
assert(has(5100), 'a free row beside paying ones is not bound')
assert(has(5200), 'nor beside paying far hops, which are no climbs or links')
assert(not has(5120), 'the paying row owes its coins before the diary')
assert(not has(3010), 'when no row names them the comment binds every row')
assert(has(3030), 'a row with no comment is unaffected')
assert(not has(3050), 'a form that cannot be read is left out')
assert(not has(3070) and not has(3090), 'unlocks not met')
vars[4566] = 1
vars[217] = 8
vars[5646] = 1
T.VALID_FOR_MS = -1
has = allowed()
assert(has(5120), 'the elite diary makes the cart free, with no coins held')
assert(has(3010), 'the quest passed, the toll is free')
assert(has(3070), 'the Karamja unlock reads the diary')
assert(has(3090), 'and the guard gave permission')
assert(not has(3110), 'an unlock with nothing to read stays unmet')
vars[217] = 7
has = allowed()
assert(not has(3010), 'at the value it is still owed')
assert(T.items_met({ items = 'ECTO_TOKEN=25|UNLOCK_DRAGONTOOTH=1' }, {}), 'taken as met until refused')
assert(not T.items_met({ items = '13393=1&UNLOCK_XERICS_HONOUR=1' }, {}), 'and the talisman is still needed')
assert(T.items_met({ items = '13393=1&UNLOCK_XERICS_HONOUR=1' }, { [13393] = 1 }))
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_far_hop_is_a_trip_with_its_requirements() {
        chooser_test(
            r#"
local cart = row('transports', '', home, far, { menu = 'Board Travel cart 2230', items = 'COINS=200', note = 'Karamja (varbit:4566=0?COINS=200)' })
local toll = row('transports', '', tile(3010, 3000), tile(3010, 3002), { menu = 'Pay-toll(2-Ecto) Energy Barrier 16105', note = 'Port Phasmatys (varbit:217=7?ECTO_TOKEN=2)' })
stage.rows = { cart, toll }
local vars = {}
Host.varbit = function(id) return vars[id] or 0 end
assert(T.choose(home, far) == nil, 'the fare is owed')
vars[4566] = 1
T.VALID_FOR_MS = -1
local c = T.choose(home, far)
assert(c and c.kind == 'ride', 'a trip, free with the diary')
assert(T.choose(home, far, { exclude = { passages = true } }) == nil, 'a script can leave them out')
assert(T.runners[c.kind] == T.runners.ride)
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_way_out_through_a_one_way_door_is_a_walk() {
        chooser_test(
            r#"
local inside, outside = tile(4990, 3000), tile(5010, 3000)
stage.rows = { row('teleportation_spells', 'Good Teleport', nil, outside) }
local way_out = true
Host.world_route = function() return { known = true, reachable = way_out } end
assert(T.choose(inside, outside) == nil, 'the map says it can be walked, and walking is shorter')
way_out = false
assert(T.choose(inside, outside) ~= nil, 'with no way on foot the spell is the way')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn an_item_on_an_object_needs_the_item_and_climbs_put_on_what_they_need() {
        chooser_test(
            r#"
local rope = row('agility_shortcuts', '', home, far, { menu = 'Use Rope -> Boulder 5842', items = 'ROPE=1', duration = 4 })
local grapple = row('shortcut_gated', '', tile(3010, 3000), tile(3016, 3000), { menu = 'Grapple Rocks 17042', items = 'CROSSBOW=1&MITH_GRAPPLE=1' })
stage.rows = { rope, grapple }
assert(T.choose(home, far) == nil, 'no rope')
stage.inventory = { { id = 954, quantity = 1, slot = 0 } }
local c = T.choose(home, far)
assert(c and c.kind == 'item_on', 'with one it is a trip')
assert(T.runners.item_on ~= nil)
local Inventory = require('@lib/main/interfaces/gametabs/inventory')
local Minimenu = require('@lib/main/interfaces/minimenu')
Inventory.hover_slot = function() end
Minimenu.entries = function() return { { option = 'Wield' }, { option = 'Drop' } } end
local worn_now = {}
Inventory.interact_slot = function(slot, option)
    assert(option == 'Wield', option)
    for i, item in ipairs(stage.inventory) do
        if item.slot == slot then table.insert(worn_now, table.remove(stage.inventory, i)) break end
    end
    stage.worn = worn_now
    return true
end
stage.inventory = { { id = 837, quantity = 1, slot = 3 }, { id = 9419, quantity = 1, slot = 4 } }
T.VALID_FOR_MS = -1
local grappling = '3010,3000,0>3016,3000,0|Grapple Rocks 17042'
local climbing = '3010,3000,0>3016,3000,0|Climb Rocks 62423'
local function allowed(key) return table.find(T.allowed_climbs(), key) ~= nil end
assert(allowed(grappling), 'a row of its own')
assert(T.wear_for_climb(tile(3010, 3000), tile(3016, 3000), 'Grapple'), 'both put on')
assert(#worn_now == 2 and #stage.inventory == 0)
assert(T.wear_for_climb(tile(3010, 3000), tile(3016, 3000), 'Grapple'), 'already worn')
assert(T.wear_for_climb(tile(1, 1), tile(2, 2), 'Climb'), 'a climb that needs nothing')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_row_that_needs_nothing_put_on_wins_on_the_same_tiles() {
        chooser_test(
            r#"
local a, b = tile(3010, 3000), tile(3016, 3000)
stage.rows = {
    row('shortcut_gated', '', a, b, { menu = 'Grapple Rocks 17042', items = 'CROSSBOW=1&MITH_GRAPPLE=1' }),
    row('shortcut_gated', '', a, b, { menu = 'Climb Rocks 62423', skills = { { name = 'Agility', level = 52 } } }),
}
stage.inventory = { { id = 837, quantity = 1, slot = 3 }, { id = 9419, quantity = 1, slot = 4 } }
local grappling, climbing = '3010,3000,0>3016,3000,0|Grapple Rocks 17042', '3010,3000,0>3016,3000,0|Climb Rocks 62423'
local function allowed(key) return table.find(T.allowed_climbs(), key) ~= nil end
T.VALID_FOR_MS = -1
assert(allowed(grappling) and not allowed(climbing), 'too low for the climb: the grapple')
local levels = T.levels
T.levels = function(real) local have = levels(real) have.agility = 60 return have end
assert(allowed(climbing) and not allowed(grappling), 'high enough: the climb, with no gear swapped')
stage.inventory = {}
assert(allowed(climbing), 'and with no grapple held')
T.levels = levels
assert(#T.allowed_climbs() == 0, 'with neither there is no way')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn portals_are_trips_but_not_the_house_or_the_pvp_arena() {
        chooser_test(
            r#"
stage.rows = { row('teleportation_portals', '', home, tile(1858, 7051), { menu = 'Home Portal 15478' }) }
assert(T.choose(home, far) == nil, 'the house leads nowhere on its own')
"#,
        );
        chooser_test(
            r#"
stage.rows = { row('teleportation_portals', '', home, far, { menu = 'Enter Free-for-all portal 26645' }) }
assert(T.choose(home, far) == nil, 'the arena')
"#,
        );
        chooser_test(
            r#"
stage.rows = { row('teleportation_portals', '', home, far, { menu = 'Enter Soul Wars Portal 40474' }) }
local c = T.choose(home, far)
assert(c and c.kind == 'ride', 'a portal is a trip')
assert(T.choose(home, far, { exclude = { portals = true } }) == nil, 'a script can leave them out')
Host.transport_settings = function() return { off = { 'portals' } } end
assert(T.choose(home, far) == nil, 'and so can the user')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn obelisks_need_the_diary_and_the_wilderness() {
        chooser_test(
            r#"
local Diaries = require('@lib/main/diaries')
local here, there = tile(3156, 3620), tile(3307, 3916)
local start = row('wilderness_obelisks', '', tile(3155, 3620), nil, { menu = 'Activate Obelisk 14829' })
stage.rows = { start, row('wilderness_obelisks', '6: Level 50 Wilderness', nil, there) }
local hard = false
Diaries.tier = function() return hard end
assert(T.choose(here, there) == nil, 'without the diary the landing is random')
hard = true
T.VALID_FOR_MS = -1
local c = T.choose(here, there)
assert(c and c.kind == 'wilderness_obelisks' and c.transport.label == 'Level 50 Wilderness', 'with it, a trip')
assert(T.choose(home, there) == nil, 'from safety, no')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_house_is_a_place_on_the_way_and_what_is_missing_is_dropped() {
        chooser_test(
            r#"
local scene = {}
Host.locs = function(plane) return if plane == 0 then scene else {} end
local House = require('@lib/main/house')
local inside = tile(1858, 7051)
local tablet = row('teleportation_items', 'Teleport to House tablet: Inside the house', nil, inside, { items = '8013=1', consumable = true })
local barrows = row('teleportation_portals_poh', 'Barrows Portal', inside, far, { menu = 'Enter Barrows Portal 37591' })
local chest = row('teleportation_boxes', 'E: Fishing Guild', inside, tile(3000, 2100), { menu = 'Teleport Menu Fancy Jewellery Box 37501' })
local beast = row('teleportation_boxes', '7: Corporeal Beast', inside, tile(3000, 1200), { menu = 'Teleport Menu Basic Jewellery Box 37492' })
local glory = row('teleportation_boxes', 'Edgeville', inside, tile(3000, 500), { menu = 'Edgeville Amulet of Glory 13523' })
stage.rows = { tablet, barrows, chest, beast, glory }
stage.inventory = { { id = 8013, quantity = 1, slot = 0 } }
local vars = {}
Host.varbit = function(id) return vars[id] or 0 end
assert(T.choose(home, far) == nil, 'nothing says a Nexus is built')
assert(T.choose(home, tile(3000, 500)) == nil, 'and nothing can say a glory is, before a visit')
-- the Nexus's first slot holds teleport 10, which the cache names Barrows
Host.enum_def = function(id) return if id == House.NEXUS_ENUM then { entries = { { key = 10, value = 500 } } } else nil end
Host.cache_files = function() return { '\249\1\1' .. string.pack('>I3', House.NEXUS_NAME_PARAM) .. 'Barrows\0' } end
vars[House.NEXUS_SLOT_VARBITS[1]] = 10
local c = T.choose(home, far)
assert(c and c.kind == 'item', 'the tablet first')
Host.in_instance = function() return true end
local function built(id, name) return { id = id, name = name, x = 11670, y = 5270, plane = 0, clickable = true, size_x = 1, size_y = 1 } end
scene = { built(House.EXIT_PORTAL, 'Portal'), built(33410, 'Portal Nexus') }
assert(House.inside())
local from_inside = tile(11675, 5277)
c = T.choose(from_inside, far)
assert(c and c.kind == 'house' and c.option == 'Barrows' and c.spell == 'nexus', 'then the Nexus, whatever the tile inside')
assert(T.choose(from_inside, tile(11680, 5280)) == nil, 'a goal in the house is a walk')
vars[House.BOX_VARBIT] = 2
c = T.choose(from_inside, tile(3000, 2100))
assert(c == nil or c.spell ~= 'box', 'a visit beats the variable: no box seen')
c = T.choose(from_inside, tile(3000, 500))
assert(c == nil or c.spell ~= 'glory', 'nor a glory')
table.insert(scene, built(29156, 'Ornate Jewellery Box'))
table.insert(scene, built(13523, 'Amulet of Glory'))
c = T.choose(from_inside, tile(3000, 2100))
assert(c and c.kind == 'house' and c.option == 'Fishing Guild' and c.spell == 'box', 'the box, with the label bare')
c = T.choose(from_inside, tile(3000, 1200))
assert(c == nil or c.option ~= 'Corporeal Beast', 'not a boss room')
c = T.choose(from_inside, tile(3000, 500))
assert(c and c.kind == 'house' and c.spell == 'glory' and c.option == 'Edgeville', 'and the glory it now sees')
local Nexus = require('@lib/main/interfaces/mainscreen/nexus')
Nexus.open = function() return true end
Nexus.find = function() return nil end
Nexus.close = function() return true end
c = T.choose(from_inside, far)
local ok, final = T.house(c)
assert(not ok and final, 'not in this Nexus')
c = T.choose(from_inside, far)
assert(c == nil or c.option ~= 'Barrows', 'so it is left out')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_house_ring_and_tree_count_once_a_visit_has_shown_them() {
        chooser_test(
            r#"
local scene = {}
Host.locs = function(plane) return if plane == 0 then scene else {} end
local House = require('@lib/main/house')
local inside = tile(1858, 7051)
local Fairyring = require('@lib/main/interfaces/mainscreen/fairyring')
Fairyring.can_use = function() return true end
stage.rows = {
    row('teleportation_items', 'Teleport to House tablet: Inside the house', nil, inside, { items = '8013=1', consumable = true }),
    row('fairy_rings', '', inside, nil, { menu = 'Configure Fairy ring 29228' }),
    row('fairy_rings', 'A I Q', nil, far),
    row('fairy_rings', 'D I Q', nil, inside),
    row('fairy_rings', '', home, nil, { menu = 'Configure Fairy ring 29495' }),
    row('spirit_trees', '', inside, nil, { menu = 'Travel Spirit Tree 29227' }),
    row('spirit_trees', 'A: Hosidius', nil, tile(3000, 500)),
    row('spirit_trees', 'C: Your house', nil, inside),
}
stage.inventory = { { id = 8013, quantity = 1, slot = 0 } }
Host.varbit = function() return 0 end
local before = T.choose(home, far, { exclude = { fairy_rings = true } })
assert(before == nil, 'no visit yet: no ring to teleport in for, even holding a tablet')
local c_tree = T.choose(home, tile(3000, 500), { exclude = { fairy_rings = true } })
assert(c_tree == nil, 'nor a tree')
Host.in_instance = function() return true end
local function built(id, name) return { id = id, name = name, x = 11670, y = 5270, plane = 0, clickable = true, size_x = 1, size_y = 1 } end
scene = { built(House.EXIT_PORTAL, 'Portal') }
local here = tile(11675, 5277)
assert(T.choose(here, far) == nil, 'no ring seen')
assert(T.choose(here, tile(3000, 500)) == nil, 'no tree seen')
scene = { built(House.EXIT_PORTAL, 'Portal'), built(29229, 'Spiritual Fairy Tree') }
local c = T.choose(here, far)
assert(c and c.kind == 'fairy_rings' and c.transport.label == 'AIQ', 'the one tree is a ring')
c = T.choose(here, tile(3000, 500))
assert(c and c.kind == 'spirit_trees' and c.transport.label == 'Hosidius', 'and a tree, with the label bare')
Host.in_instance = function() return false end
c = T.choose(home, inside)
assert(c and c.kind == 'fairy_rings' and c.transport.label == 'DIQ', 'a ring underfoot goes into the house once it is known')
c = T.choose(home, tile(3000, 500), { exclude = { fairy_rings = true } })
assert(c and c.kind == 'item', 'from outside a visit makes a tablet and the tree a way')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_ferox_enclave_can_be_a_goal_but_not_a_way_through_the_wilderness() {
        chooser_test(
            r#"
local enclave = tile(3150, 3635)
stage.rows = { row('teleportation_spells', 'Good Teleport', nil, enclave) }
local Ferox = require('@lib/main/ferox')
assert(Ferox.contains(3150, 3635) and not Ferox.contains(3150, 3700))
assert(T.choose(home, enclave) ~= nil, 'to go to the Enclave')
assert(T.choose(home, tile(3148, 3630)) ~= nil, 'or inside it')
assert(T.choose(home, tile(3150, 3660)) == nil, 'a goal north of it is the Wilderness')
assert(T.choose(home, tile(3100, 3500)) == nil, 'and it is no way through to somewhere else')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn canoes_are_dropped_when_a_station_says_there_is_no_axe() {
        chooser_test(
            r#"
local Canoe = require('@lib/main/interfaces/mainscreen/canoe')
local Log_row = row('canoes', 'The Clock Tower', tile(3000, 3000), far, { skills = { { name = 'Woodcutting', level = 27 } }, duration = 30 })
stage.rows = { Log_row }
Player.position = function() return tile(3000, 3000) end
Host.game_state = function() return { game_state = 30, skills = { { name = 'Woodcutting', level = 60, real_level = 60 } } } end
assert(Canoe.kind_for_level(12) == 'Log' and Canoe.kind_for_level(27) == 'Dugout')
assert(Canoe.kind_for_level(42) == 'Stable dugout' and Canoe.kind_for_level(57) == 'Waka' and Canoe.kind_for_level(nil) == 'Waka')
local c = T.choose(home, far)
assert(c and c.kind == 'canoe', 'with no axe held, one is assumed to be kept')
local kind
Canoe.travel = function(place, k) kind = k return false, Canoe.NO_AXE end
local ok, final = T.canoe(c)
assert(not ok and final, 'pointless to try again')
assert(kind == 'Dugout', 'the smallest for the trip')
assert(T.choose(home, far) == nil, 'the station said there is none')
stage.inventory = { { id = 1351, quantity = 1, slot = 0 } }
c = T.choose(home, far)
assert(c ~= nil, 'an axe held brings them back')
assert(not select(1, T.canoe(c)), 'but it may be too high a level')
assert(T.choose(home, far) == nil, 'and then the same axe does not bring them back')
stage.inventory = {}
assert(T.choose(home, far) ~= nil, 'with the axes changed, one may have gone to a keeper')
assert(T.choose(home, far, { exclude = { canoes = true } }) == nil, 'and a script can leave them out')
Host.transport_settings = function() return { off = { 'canoes' } } end
assert(T.choose(home, far) == nil, 'and so can the user')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn balloons_need_a_log_held_or_stored() {
        chooser_test(
            r#"
local Balloon = require('@lib/main/interfaces/mainscreen/balloon')
local chat = {}
Host.chat_messages = function() return chat end
assert(next(Balloon.stored()) == nil, 'nothing said yet')
chat = { { text = 'This crate currently contains 30 logs, 12 oak logs and 3 yew logs.' } }
local s = Balloon.stored()
assert(s['logs'] == 30 and s['oak logs'] == 12 and s['yew logs'] == 3, 'the crate says')
assert(s['magic logs'] == 0 and s['willow logs'] == 0, 'and a kind it leaves out is none')
chat = {
    { text = 'This crate currently contains 30 logs and 30 magic logs.' },
    { text = 'You have 29 sets of Magic logs left in storage.' },
    { text = 'You board the balloon and fly to the Gnome Stronghold.' },
}
s = Balloon.stored()
assert(s['magic logs'] == 29 and s['logs'] == 30, 'a flight since then is newer')
assert(Balloon.has_log('Grand Tree') and Balloon.has_log('Entrana'), 'in storage')
assert(not Balloon.has_log('Varrock'), 'no willow logs anywhere')
stage.inventory = { { id = 1519, quantity = 1, slot = 0 } }
assert(Balloon.has_log('Varrock'), 'but one held will do')
stage.inventory = {}
local basket = { menu = 'Use Basket 19129', varbits = { { id = 2869, op = '=', value = 1 } } }
stage.rows = {
    row('hot_air_balloons', '', tile(3010, 3000), nil, basket),
    row('hot_air_balloons', 'Grand Tree', nil, tile(3800, 3000)),
    row('hot_air_balloons', 'Varrock', nil, tile(3000, 2200)),
}
Host.varbit = function(id) return if id == 2869 then 1 else 0 end
assert(T.choose(home, far) == nil, 'a stored log is not counted on')
stage.inventory = { { id = 1513, quantity = 1, slot = 0 } }
local c = T.choose(home, far)
assert(c and c.kind == 'hot_air_balloons' and c.transport.label == 'Grand Tree', 'one held is')
assert(T.choose(home, tile(3000, 2200)) == nil, 'and only for its own place')
stage.inventory = {}
T.BALLOON_STORAGE = true
c = T.choose(home, far)
assert(c and c.transport.label == 'Grand Tree', 'a script can count on storage')
assert(T.choose(home, tile(3000, 2200)) == nil, 'and still not a kind said to be gone')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn minigame_teleports_wait_for_their_cooldown() {
        chooser_test(
            r#"
local cooldown = { { id = 888, op = '@', value = 20 } }
stage.rows = {
    row('teleportation_minigames', 'Castle Wars Minigame Teleport', nil, tile(3800, 3000), { varps = cooldown }),
    row('teleportation_minigames', 'Rat Pits Minigame Teleport: 2. Varrock', nil, tile(3000, 2200), { varps = cooldown }),
    row('teleportation_minigames', 'Clan Wars Minigame Teleport', nil, tile(3150, 3635), { varps = cooldown }),
}
local c = T.choose(home, far)
assert(c and c.kind == 'minigame' and c.spell == 'Castle Wars' and c.option == nil, 'off cooldown')
c = T.choose(home, tile(3000, 2200))
assert(c and c.spell == 'Rat Pits' and c.option == 'Varrock', 'the activity and which pit')
assert(T.choose(home, far, { exclude = { minigames = true } }) == nil, 'a group can be excluded')
assert(T.choose(home, far, { exclude = { ['Castle Wars'] = true } }) == nil, 'and an activity')
assert(T.choose(home, tile(3150, 3640)) == nil, 'nothing into the wilderness')
Host.varp = function() return math.floor(Host.unix_time() / 60) - 5 end
assert(T.choose(home, far) == nil, 'used 5 minutes ago')
Host.varp = function() return math.floor(Host.unix_time() / 60) - 21 end
assert(T.choose(home, far) ~= nil, 'and again after 20')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn the_wilderness_is_only_entered_when_asked() {
        chooser_test(
            r#"
local deep = tile(3153, 3923)
stage.rows = {
    row('teleportation_spells', 'Wild Teleport', nil, tile(3100, 3700)),
    row('teleportation_levers', '', deep, tile(2560, 3000), { menu = 'Ardougne Lever 1815' }),
    row('teleportation_levers', '', tile(3000, 3010), tile(3154, 3924), { menu = 'Pull Lever 26761' }),
}
assert(T.choose(tile(3160, 3400), tile(2560, 3010)) == nil, 'no walking to a lever deep in the wilderness')
assert(T.choose(tile(3000, 3000), tile(3160, 3930)) == nil, 'no spell or lever into it')
local c = T.choose(tile(3150, 3920), tile(2560, 3010))
assert(c and c.kind == 'lever' and c.transport.menu == 'Ardougne Lever 1815', 'but one that gets us out when we are in it')
local inside = T.choose(tile(3100, 3530), tile(3100, 3705))
assert(inside and inside.kind == 'spell', 'moving within it once we are in it')
assert(T.choose(tile(3000, 3600), tile(3160, 3930), { exclude = { spells = true } }) == nil, 'no walking out to a lever that leads back in')
assert(T.choose(tile(3000, 3000), tile(3160, 3930)) == nil, 'not from safety')
assert(T.choose(tile(3000, 3000), tile(3160, 3930), { wilderness = true }) ~= nil, 'one plan can ask for it')
T.WILDERNESS = true
assert(T.choose(tile(3000, 3000), tile(3160, 3930)) ~= nil, 'or every plan')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn items_count_when_worn() {
        chooser_test(
            r#"
stage.rows = {
    row('teleportation_items', 'Ring: Far', nil, tile(3801, 3000), { items = '2552=1' }),
}
assert(T.choose(home, far) == nil, 'not held')
stage.worn = { { id = 2552, quantity = 1, slot = 12 } }
local c = T.choose(home, far)
assert(c and c.kind == 'item' and c.spell == 'Ring' and c.option == 'Far', 'a worn ring counts')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn gated_rows_are_told_to_the_planner_and_ways_across_join_landmasses() {
        chooser_test(
            r#"
local chop = row('climb_gated', '', tile(1, 1), tile(1, 1, 1), { items = 'AXE=1' })
local stile = row('link_gated', '', tile(4999, 3000), tile(5001, 3000), { items = 'AXE=1' })
local hop = row('shortcut_gated', '', tile(4999, 3100), tile(5001, 3100), { skills = { { name = 'Agility', level = 55 } } })
local wild_hop = row('shortcut_gated', '', tile(3100, 3600), tile(3100, 3610), { skills = { { name = 'Agility', level = 1 } } })
stage.rows = { chop, stile, hop, wild_hop, row('teleportation_spells', 'Isle Teleport', nil, tile(5001, 3010)) }
local from, to = tile(4900, 3000), tile(5060, 3000)
assert(#T.allowed_climbs() == 0, 'no axe yet, and the shortcut is judged on the real level')
assert(T.choose(from, to) ~= nil, 'no way across yet: the spell')
T.VALID_FOR_MS = 0
stage.worn = { { id = 1351, quantity = 1, slot = 3 } }
local keys = {}
for _, k in ipairs(T.allowed_climbs()) do keys[string.match(k, '^[^|]+')] = true end
assert(keys['1,1,0>1,1,1'] and keys['4999,3000,0>5001,3000,0'], 'a worn axe counts for gated rows')
assert(not keys['4999,3100,0>5001,3100,0'], 'a boost does not count for a shortcut')
assert(not keys['3100,3600,0>3100,3610,0'], 'and a shortcut in the wilderness never does')
T.MIN_SAVING_TICKS = 1000
assert(T.choose(from, to) == nil, 'the stile joins the landmasses: walking is possible')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_network_pairs_its_starts_with_its_ends() {
        chooser_test(
            r#"
stage.rows = {
    row('quetzals', '', tile(3005, 3000), nil, { menu = 'Travel Quetzal 13350' }),
    row('quetzals', '', tile(3006, 3000), nil, { menu = 'Travel Quetzal 13350' }),
    row('quetzals', '1: Here', nil, tile(3004, 3000), { duration = 6 }),
    row('quetzals', '2: Far', nil, tile(3800, 3000), { duration = 6 }),
    row('quetzals', '3: Quest', nil, tile(3801, 3000), { duration = 1, varbits = { { id = 1, op = '=', value = 9 } } }),
}
local c = T.choose(home, far)
assert(c and c.kind == 'quetzals', 'by the network')
assert(c.transport.label == 'Far' and c.transport.origin.x == 3005, 'the first start of the structure, the end whose test holds')
assert(c.id == 'quetzals 3005,3000 Far')
T.RETRY_PAUSE_MS = 0
T.runners.quetzals = function() return false end
assert(not T.use(c) and T.failed_lately(c))
assert(T.choose(home, far) == nil, 'a failed trip is out of the next plan')
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn a_retry_never_repeats_a_trip_that_landed_late() {
        chooser_test(
            r#"
local at = tile(100, 100)
Player.position = function() return at end
T.RETRY_PAUSE_MS = 0
local calls = 0
T.runners.item = function() calls += 1 at = tile(3800, 3000) return false end
local piece = { kind = 'item', id = 'piece', transport = { name = 'Piece', destination = tile(3800, 3000) }, spell = 'Piece', ticks = 1, saved = 1 }
assert(T.use(piece) and calls == 1, 'landed during the pause: no second charge')
assert(not T.failed_lately(piece))
"#,
        );
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn cs2_scripts_and_database_rows_decode_and_bad_bytes_are_refused() {
        let (lines, result) = run_lines(
            "local Cs2 = require('cs2')\n\
             -- a script named '' with `push 7; switch 0; get_varbit 99; return`, and one switch table { [5] = 1 }\n\
             local code = '\\0' .. string.pack('>I2i4', 0, 7) .. string.pack('>I2i4', 60, 0) .. string.pack('>I2i4', 25, 99) .. string.pack('>I2B', 21, 0)\n\
             local tables = string.char(1) .. string.pack('>I2', 1) .. string.pack('>i4i4', 5, 1)\n\
             local bytes = code .. string.pack('>i4', 4) .. string.rep('\\0', 12) .. tables .. string.pack('>I2', #tables)\n\
             local s = Cs2.script(bytes)\n\
             assert(s and #s.instructions == 4, 'script')\n\
             assert(s.instructions[3].op == Cs2.GET_VARBIT and s.instructions[3].value == 99)\n\
             assert(s.tables[1][5] == 1)\n\
             assert(Cs2.script('not a script at all, just text') == nil)\n\
             assert(Cs2.script(string.sub(bytes, 1, #bytes - 5)) == nil)\n\
             -- a row: table 0, columns 2 (a string) and 19 (an int)\n\
             local row = '\\4\\0' .. '\\3\\50' .. string.char(2, 1, 36, 1) .. 'Demon Slayer\\0' .. string.char(19, 1, 0, 1) .. string.pack('>i4', 3) .. string.char(255, 0)\n\
             local r = Cs2.dbrow(row)\n\
             assert(r and r[2].values[1][1] == 'Demon Slayer' and r[19].values[1][1] == 3, 'row')\n\
             assert(Cs2.dbrow('\\9\\9\\9') == nil and Cs2.dbrow('') == nil)\n\
             print('ok')",
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn random_draws_stay_in_range() {
        let (lines, result) = run_lines(
            "local Random = require('random')\n\
             for _ = 1, 20000 do\n\
                 local l, m, r = Random.left(10, 20), Random.mean(10, 20), Random.right(10, 20)\n\
                 assert(l >= 10 and l < 20 and m >= 10 and m <= 20 and r > 10 and r <= 20, `{l} {m} {r}`)\n\
                 local mode = Random.mode(0.9, 0.1, 1.5)\n\
                 assert(mode >= 0.1 and mode <= 1.5, mode)\n\
                 local i = Random.int(3, 7)\n\
                 assert(i >= 3 and i < 7 and i == math.floor(i), i)\n\
                 assert(Random.range(5, 5) == 5)\n\
             end\n\
             assert(not Random.chance(0))\n\
             local low, high = 0, 0\n\
             for _ = 1, 2000 do low += Random.left(0, 100); high += Random.right(0, 100) end\n\
             print(low < high)",
        );
        assert_eq!(lines, ["true"]);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn shape_points_land_inside() {
        let (lines, result) = run_lines(
            "local G = require('geometry')\n\
             local box = G.Box.new(100, 200, 160, 230)\n\
             local circle = G.Circle.new(400, 300, 25)\n\
             local quad = G.Quad.new({ x = 500, y = 100 }, { x = 560, y = 130 }, { x = 530, y = 190 }, { x = 470, y = 160 })\n\
             local function near_box(b, p) return p.x >= b.x1 - 2 and p.x <= b.x2 + 2 and p.y >= b.y1 - 2 and p.y <= b.y2 + 2 end\n\
             local quad_bounds = G.Box.new(470, 100, 560, 190)\n\
             local froms = { { x = 130, y = 215 }, { x = 0, y = 0 }, { x = 900, y = 600 }, { x = 400, y = 300 }, { x = 515, y = 145 } }\n\
             for _ = 1, 500 do\n\
                 for _, from in froms do\n\
                     for _, p in { G.Box.random_point(box), G.Box.random_point_center(box), G.Box.random_skewed_point(box, from), G.Box.random_weighted_point(box, from) } do\n\
                         assert(near_box(box, p), `box {p.x},{p.y} from {from.x},{from.y}`)\n\
                     end\n\
                     for _, p in { G.Circle.random_point(circle), G.Circle.random_point_center(circle), G.Circle.random_skewed_point(circle, from), G.Circle.random_weighted_point(circle, from) } do\n\
                         assert(G.Circle.contains(G.Circle.new(400, 300, 27), p), `circle {p.x},{p.y} from {from.x},{from.y}`)\n\
                     end\n\
                     for _, p in { G.Quad.random_point(quad), G.Quad.random_point_center(quad), G.Quad.random_skewed_point(quad, from), G.Quad.random_weighted_point(quad, from) } do\n\
                         assert(near_box(quad_bounds, p), `quad {p.x},{p.y} from {from.x},{from.y}`)\n\
                     end\n\
                 end\n\
             end\n\
             local thin = G.Box.new(50, 60, 50, 90)\n\
             local p = G.Box.random_weighted_point(thin, { x = 300, y = 300 })\n\
             assert(near_box(thin, p), `thin {p.x},{p.y}`)\n\
             print(G.Box.contains(box, { x = 100, y = 230 }), G.Box.contains(box, { x = 161, y = 230 }))\n\
             print(G.Quad.contains(quad, { x = 515, y = 145 }), G.Quad.contains(quad, { x = 470, y = 100 }))\n\
             local w = G.Box.from_area({ x = 10, y = 20, width = 5, height = 3 })\n\
             print(w.x1, w.y1, w.x2, w.y2)",
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(lines, ["true\tfalse", "true\tfalse", "10\t20\t14\t22"]);
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn keyboard_keys_match_simba() {
        let (lines, result) = run_lines(
            "local K = require('keyboard')\n\
             print(K.keys.RETURN, K.keys.LEFT, K.keys.F12, K.keys.LSHIFT, K.keys.OEM_102, #K.ARROWS, #K.FKEYS)\n\
             print(K.is_key_down(K.keys.SHIFT), K.interval_min(), K.interval_max())",
        );
        assert_eq!(lines, ["13\t37\t123\t160\t226\t4\t12", "false\t40\t85"]);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn require_rejects_unknown_modules() {
        let (_, result) = run_lines("require('os')");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("module 'os' not found"), "{error}");
    }

    #[test]
    fn unsafe_libraries_are_missing() {
        let (lines, result) = run_lines(
            "print(io, os, package, ffi, debug, dofile, loadfile, type(bit32), type(jit))",
        );
        assert_eq!(lines, ["nil\tnil\tnil\tnil\tnil\tnil\tnil\ttable\tnil"]);
        assert!(result.is_ok());
    }

    #[test]
    fn sleep_actually_sleeps() {
        let start = std::time::Instant::now();
        let (_, result) = run_lines("sleep(50)");
        assert!(result.is_ok());
        assert!(start.elapsed() >= std::time::Duration::from_millis(50));
    }

    #[test]
    fn client_functions_need_a_client() {
        let (lines, result) = run_lines(
            "local ok_state, err = pcall(game_state)\n\
             print(ok_state, err)\n\
             local ok_conn, err2 = pcall(connection)\n\
             print(ok_conn, err2)\n\
             print(varp(173))\n\
             print(varps())\n\
             local ok, err3 = pcall(click, 1, 2)\n\
             print(ok)\n\
             print(err3)\n\
             local ok_rect, err4 = pcall(draw_rect, 100, 100, 200, 200, 255, 0, 0)\n\
             print(ok_rect, err4)\n\
             local ok_clear, err5 = pcall(clear_overlay)\n\
             print(ok_clear, err5)\n\
             local ok_batch, err6 = pcall(draw_batch, { { kind = 'clear' } })\n\
             print(ok_batch, err6)",
        );
        let none = "nil\tno client selected";
        let refused =
            |line: &String| line.starts_with("false") && line.contains("no client selected");
        for line in lines[..2].iter().chain(&lines[6..]) {
            assert!(refused(line), "{lines:?}");
        }
        assert_eq!(lines[2..5], [none, none, "false"]);
        assert!(lines[5].contains("no client selected"), "{lines:?}");
        assert!(result.is_ok());
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn login_needs_a_client_too() {
        let (lines, result) = run_lines(
            "local Login = require('login')\n\
             print(pcall(Login.login))",
        );
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("false") && lines[0].contains("no client selected"),
            "{lines:?}"
        );
        assert!(result.is_ok());
    }
}
