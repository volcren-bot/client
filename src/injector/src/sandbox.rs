use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use mlua::{
    FromLuaMulti, IntoLua, IntoLuaMulti, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib,
    serde::SerializeOptions,
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
    draw, draw_batch, draw_lines, entry_swaps, enum_def, find_path, game_state, game_val_names,
    ground_items, in_instance, item_container, item_names, item_params, item_stackable, landmasses,
    loc_model, loc_shown, locs, map_regions, menu, menu_click, menu_layout, menu_open, movement,
    nav_plan, npc_hitsplats, npc_model, npc_sizes, npc_type_model, npcs, player_hitsplats,
    player_model, player_plane, players, project, project_moved, projectiles, scene_collision,
    scene_route, selected_widget, send_input, send_input_with, server_tile, spot_anims,
    sub_interfaces, varbit, varc, varp_value, varps, wait_watchers, widget, widget_children,
    widget_model, world_route, world_select,
};
#[cfg(not(windows))]
use offline::*;

#[cfg(not(windows))]
mod offline {
    use std::collections::BTreeMap;

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

    macro_rules! unreachable_clients {
        ($(fn $name:ident($($arg:ty),*) -> $value:ty;)*) => {
            $(pub fn $name($(_: $arg),*) -> Result<$value, String> {
                Err(NO_WIN32.into())
            })*
        };
    }

    unreachable_clients! {
        fn game_state(u32) -> GameState;
        fn widget(u32, u16, u16) -> Option<Widget>;
        fn sub_interfaces(u32) -> Option<Vec<SubInterface>>;
        fn menu(u32) -> Vec<MenuEntry>;
        fn menu_open(u32) -> bool;
        fn menu_click(u32) -> Option<MenuClick>;
        fn entry_swaps(u32) -> crate::EntrySwaps;
        fn menu_layout(u32) -> Option<MenuLayout>;
        fn selected_widget(u32) -> Option<crate::SelectedWidget>;
        fn project(u32, &[crate::projection::Point], crate::projection::Via) -> Vec<[f32; 2]>;
        fn project_moved(u32, &[crate::projection::Point], [i32; 3]) -> Vec<[f32; 2]>;
        fn game_val_names(u32, u32, &[i32]) -> Vec<Option<String>>;
        fn world_route(u32, crate::nav::Point, crate::nav::Point, &[String])
            -> crate::landmass::Route;
        fn widget_model(u32, u16, u16) -> Option<decode::widgets::WidgetModel>;
        fn item_names(u32, &[i32]) -> Vec<Option<String>>;
        fn item_stackable(u32, &[i32]) -> Vec<Option<bool>>;
        fn item_container(u32, i32) -> Option<Vec<Item>>;
        fn varc(u32, i32) -> Option<i32>;
        fn varp_value(u32, usize) -> Option<i32>;
        fn varps(u32) -> Option<Vec<i32>>;
        fn varbit(u32, i32) -> Option<i32>;
        fn landmasses(u32, &[[i32; 3]]) -> Vec<u32>;
        fn cache_files(u32, u32, &[[i32; 2]]) -> Vec<Option<Vec<u8>>>;
        fn enum_def(u32, i32) -> Option<crate::config::EnumDef>;
        fn item_params(u32, &[i32]) -> Vec<Option<BTreeMap<u32, crate::config::Value>>>;
        fn npcs(u32) -> Vec<Npc>;
        fn npc_model(u32, i32) -> Option<EntityModel>;
        fn loc_model(u32, u8, i32, i32, i32) -> Option<EntityModel>;
        fn ground_items(u32) -> Vec<GroundItem>;
        fn projectiles(u32) -> Vec<Projectile>;
        fn spot_anims(u32) -> Vec<SpotAnim>;
        fn chat_messages(u32) -> Vec<ChatMessage>;
        fn locs(u32, Option<i32>) -> Vec<Loc>;
        fn loc_shown(u32, &[u32]) -> Vec<crate::LocView>;
        fn cache_locs(u32, [i32; 4], &crate::CacheLocFilter) -> Vec<crate::CacheLoc>;
        fn find_path(u32, i32, [i32; 2], [i32; 2], Option<usize>) -> Vec<[i32; 2]>;
        fn server_tile(u32) -> Option<crate::ServerTile>;
        fn movement(u32) -> Option<crate::Movement>;
        fn player_plane(u32) -> i32;
        fn map_regions(u32) -> Vec<[i32; 2]>;
        fn in_instance(u32) -> bool;
        fn npc_sizes(u32, &[i32]) -> BTreeMap<i32, i32>;
        fn npc_hitsplats(u32, &[i32]) -> BTreeMap<i32, Vec<decode::world::Hitsplat>>;
        fn player_hitsplats(u32, &[i32]) -> BTreeMap<i32, Vec<decode::world::Hitsplat>>;
        fn client_cycle(u32) -> i32;
        fn scene_collision(u32) -> (i32, i32, i32, Vec<Vec<u32>>);
        fn scene_route(u32, [i32; 2], [i32; 2], Option<crate::scene_graph::Options>)
            -> crate::pathfinding::Route;
        fn nav_plan(u32, crate::nav::Point, crate::nav::Point, &crate::nav::Request)
            -> crate::nav::NavPlan;
        fn npc_type_model(u32, i32, i32, i32, (i32, i32), i32) -> Option<EntityModel>;
        fn player_model(u32, i32) -> Option<EntityModel>;
        fn players(u32) -> Vec<Player>;
        fn widget_children(u32, u16, u16) -> Option<Vec<Widget>>;
        fn world_select(u32) -> Option<WorldSelect>;
        fn send_input(u32, &Input) -> ();
        fn send_input_with(u32, &Input, &dyn super::Pace) -> ();
        fn bank_pin_enter(u32, &(dyn super::Pace + Sync)) -> crate::bank_pin::Outcome;
        fn draw(u32, &Overlay) -> ();
        fn draw_batch(u32, &[Overlay]) -> ();
        fn draw_lines(u32, &[[i32; 4]], crate::Rgba) -> ();
    }
}

const NO_CLIENT: &str = "no client selected";
const NO_AGENT: &str = "agent isn't loaded in the client";
const ITEM_CHANGES_PER_WAKE: usize = 2000;
const PACKETS_PER_WAKE: usize = ipc::packets::SLOT_COUNT;
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(u32::MAX as u64);

#[derive(Serialize)]
struct ScreenPoint {
    x: f32,
    y: f32,
}

impl From<[f32; 2]> for ScreenPoint {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

fn screen_points(points: Vec<[f32; 2]>) -> Vec<ScreenPoint> {
    points.into_iter().map(ScreenPoint::from).collect()
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
    ScriptModel {
        bounding_box: m.bounding_box,
        orientation: m.orientation,
        vertex_count: m.vertex_count,
        corners: screen_points(m.corners),
        hull: screen_points(m.hull),
        faces: m
            .faces
            .into_iter()
            .map(|face| face.map(ScreenPoint::from))
            .collect(),
        frame_count: m.frame_count,
    }
}

#[derive(Serialize)]
struct ScriptResponse {
    ok: bool,
    #[serde(flatten)]
    response: crate::http::Response,
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

#[derive(Serialize)]
struct Collision {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    plane: i32,
    flags: Vec<u32>,
}

#[derive(Serialize)]
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

fn to_lua<T: Serialize>(lua: &Lua, value: &T) -> mlua::Result<mlua::Value> {
    lua.to_value_with(
        value,
        SerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

fn need_client(pid: Option<u32>) -> mlua::Result<u32> {
    pid.ok_or_else(|| mlua::Error::RuntimeError(NO_CLIENT.into()))
}

fn get<T: mlua::FromLua>(table: &mlua::Table, key: &str) -> Result<T, String> {
    table.get(key).map_err(|e| e.to_string())
}

fn tile_arg(table: &mlua::Table) -> Result<[i32; 2], String> {
    Ok([get(table, "x")?, get(table, "y")?])
}

fn area_arg(table: &mlua::Table) -> Result<[i32; 4], String> {
    Ok([
        get(table, "x1")?,
        get(table, "y1")?,
        get(table, "x2")?,
        get(table, "y2")?,
    ])
}

fn point_arg(table: &mlua::Table, what: &str) -> Result<crate::nav::Point, String> {
    let read = |field: &str| {
        table
            .get::<i32>(field)
            .map_err(|e| format!("{what}'s {field}: {e}"))
    };
    Ok((read("x")?, read("y")?, read("plane")?))
}

fn rounded<K: IntoLua>(table: &mlua::Table, keys: [K; 4]) -> mlua::Result<[i32; 4]> {
    let [a, b, c, d] = keys.map(|key| table.get::<f64>(key).map(|v| v.round() as i32));
    Ok([a?, b?, c?, d?])
}

fn rgba(r: u8, g: u8, b: u8, a: Option<u8>) -> Rgba {
    Rgba {
        r,
        g,
        b,
        a: a.unwrap_or(255),
    }
}

fn shape(line: bool, [x1, y1, x2, y2]: [i32; 4], color: Rgba) -> Overlay {
    if line {
        Overlay::Line {
            x1,
            y1,
            x2,
            y2,
            color,
        }
    } else {
        Overlay::Rect {
            x1,
            y1,
            x2,
            y2,
            color,
        }
    }
}

type ShapeArgs = (i32, i32, i32, i32, u8, u8, u8, Option<u8>);

fn shape_args(line: bool, (x1, y1, x2, y2, r, g, b, a): ShapeArgs) -> Overlay {
    shape(line, [x1, y1, x2, y2], rgba(r, g, b, a))
}

fn overlay_arg(table: &mlua::Table) -> mlua::Result<Overlay> {
    let kind: String = table.get("kind")?;
    if kind == "clear" {
        return Ok(Overlay::Clear);
    }
    let corners = rounded(table, ["x1", "y1", "x2", "y2"])?;
    let color = rgba(
        table.get("r")?,
        table.get("g")?,
        table.get("b")?,
        table.get("a")?,
    );
    match kind.as_str() {
        "rect" => Ok(shape(false, corners, color)),
        "line" => Ok(shape(true, corners, color)),
        _ => Err(mlua::Error::RuntimeError(format!(
            "unknown shape kind: {kind}"
        ))),
    }
}

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

fn scene_options(t: &mlua::Table) -> Result<crate::scene_graph::Options, String> {
    let default = crate::scene_graph::Options::default();
    let field = |name: &str| get::<Option<f64>>(t, name);
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
}

fn nav_request(t: &mlua::Table) -> Result<crate::nav::Request, String> {
    let mut request = crate::nav::Request::default();
    let o = &mut request.options;
    o.near = get::<Option<i32>>(t, "near")?.unwrap_or(o.near);
    o.door_cost = get::<Option<u32>>(t, "door_cost")?.unwrap_or(o.door_cost);
    o.climb_cost = get::<Option<u32>>(t, "climb_cost")?.unwrap_or(o.climb_cost);
    o.shortcut_saving = get::<Option<f64>>(t, "shortcut_saving")?
        .map_or(o.shortcut_saving, |tiles| (tiles.max(0.0) * 10.0) as u32);
    o.rnd = get::<Option<f64>>(t, "rnd")?.unwrap_or(o.rnd);
    o.seed = get::<Option<f64>>(t, "seed")?.map(|s| s as u64);
    if let Some(options) = get(t, "door_options")? {
        request.door_options = options;
    }
    if let Some(climbs) = get(t, "climbs")? {
        request.climbs = climbs;
    }
    if let Some(keys) = get(t, "allow_climbs")? {
        request.allow_climbs = keys;
    }
    if let Some(names) = get(t, "portal_names")? {
        request.portal_names = names;
    }
    for loc in get::<Option<Vec<mlua::Table>>>(t, "avoid")?.unwrap_or_default() {
        request.avoid.push((
            get(&loc, "id")?,
            get(&loc, "x")?,
            get(&loc, "y")?,
            get(&loc, "plane")?,
        ));
    }
    Ok(request)
}

#[derive(Default)]
struct Callbacks {
    items: Option<ItemWatcher>,
    item_callbacks: Vec<(u64, mlua::Function)>,
    packets: Option<PacketWatcher>,
    packet_callbacks: Vec<(u64, Filter, mlua::Function)>,
    waiting: Option<Filter>,
    last_id: u64,
}

impl Callbacks {
    fn watching(&self) -> bool {
        self.items.is_some() || self.packets.is_some()
    }

    fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

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
        let packets = match &mut self.packets {
            Some(packets) => packets,
            None => self
                .packets
                .insert(PacketWatcher::new(need_client(pid)?).map_err(mlua::Error::RuntimeError)?),
        };
        packets.set_filter(filter);
        Ok(())
    }
}

enum Until {
    Time,
    ItemChange,
    Packet(Filter, u64),
}

fn pump_events(
    lua: &Lua,
    callbacks: &Rc<RefCell<Callbacks>>,
    time: std::time::Duration,
    until: &Until,
) -> mlua::Result<Option<mlua::Value>> {
    let now = std::time::Instant::now();
    let deadline = now.checked_add(time).unwrap_or(now + MAX_WAIT);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let (changes, packets) = {
            let mut callbacks = callbacks.borrow_mut();
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

fn packet_filter(name: &str, opcodes: &[u16], sent: Option<bool>) -> mlua::Result<Filter> {
    if opcodes.is_empty() {
        return Err(mlua::Error::RuntimeError(format!(
            "{name}: no packets given"
        )));
    }
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
    Ok(filter)
}

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

type InputWorker = Option<Rc<Worker<Job>>>;

fn cancel_input(worker: Option<&Worker<Job>>) {
    if let Some(worker) = worker {
        worker.cancel();
    }
}

fn queue(worker: &InputWorker, job: Job) -> Result<(), String> {
    worker
        .as_ref()
        .map_or(Err(NO_CLIENT.into()), |worker| worker.push(job))
}

fn button_arg(name: Option<String>) -> mlua::Result<MouseButton> {
    name.map_or(Ok(MouseButton::Left), |name| {
        MouseButton::parse(&name).map_err(mlua::Error::RuntimeError)
    })
}

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

fn button_input(down: bool) -> impl Fn(Option<String>) -> mlua::Result<Input> {
    move |button| {
        Ok(Input::Button {
            button: button_arg(button)?,
            down,
        })
    }
}

fn scroll_input(notches: i32) -> mlua::Result<Input> {
    Ok(Input::Scroll { notches })
}

fn key_input(down: bool) -> impl Fn(u8) -> mlua::Result<Input> {
    move |key| Ok(Input::Key { key, down })
}

fn file_value(lua: &Lua, file: Option<Vec<u8>>, missing: mlua::Value) -> mlua::Result<mlua::Value> {
    Ok(match file {
        Some(bytes) => mlua::Value::String(lua.create_string(&bytes)?),
        None => missing,
    })
}

type Print = Rc<RefCell<dyn FnMut(String)>>;

fn without_separators(text: &str) -> String {
    text.replace('\u{1e}', "")
}

struct Sandbox<'a> {
    lua: &'a Lua,
    globals: mlua::Table,
    pid: Option<u32>,
    worker: InputWorker,
}

impl Sandbox<'_> {
    fn set(&self, name: &str, function: mlua::Function) -> mlua::Result<()> {
        self.globals.set(name, function)
    }

    fn printing<A: FromLuaMulti>(
        &self,
        name: &str,
        print: &Print,
        line: impl Fn(&Lua, A) -> mlua::Result<String> + 'static,
    ) -> mlua::Result<()> {
        let print = print.clone();
        self.set(
            name,
            self.lua.create_function(move |lua, args: A| {
                let line = line(lua, args)?;
                (print.borrow_mut())(line);
                Ok(())
            })?,
        )
    }

    fn value<T: Serialize + 'static>(&self, name: &str, read: fn() -> T) -> mlua::Result<()> {
        self.set(
            name,
            self.lua
                .create_function(move |lua, (): ()| to_lua(lua, &read()))?,
        )
    }

    fn query_with<A: FromLuaMulti, T>(
        &self,
        name: &str,
        read: impl Fn(u32, A) -> Result<T, String> + 'static,
        convert: impl Fn(&Lua, T) -> mlua::Result<mlua::Value> + 'static,
    ) -> mlua::Result<()> {
        let pid = self.pid;
        self.set(
            name,
            self.lua.create_function(
                move |lua, args: A| -> mlua::Result<(mlua::Value, Option<String>)> {
                    let Some(pid) = pid else {
                        return Ok((mlua::Value::Nil, Some(NO_CLIENT.into())));
                    };
                    match read(pid, args) {
                        Ok(value) => Ok((convert(lua, value)?, None)),
                        Err(error) => Ok((mlua::Value::Nil, Some(error))),
                    }
                },
            )?,
        )
    }

    fn query<A: FromLuaMulti, T: Serialize>(
        &self,
        name: &str,
        read: impl Fn(u32, A) -> Result<T, String> + 'static,
    ) -> mlua::Result<()> {
        self.query_with(name, read, |lua, value| to_lua(lua, &value))
    }

    fn read<T: Serialize + 'static>(
        &self,
        name: &str,
        read: fn(u32) -> Result<T, String>,
    ) -> mlua::Result<()> {
        self.query(name, move |pid, ()| read(pid))
    }

    fn menu_query<T: Serialize + 'static>(
        &self,
        name: &str,
        read: fn(u32) -> Result<T, String>,
    ) -> mlua::Result<()> {
        let worker = self.worker.clone();
        self.query(name, move |pid, ()| {
            cancel_input(worker.as_deref());
            read(pid)
        })
    }

    fn action<A: FromLuaMulti>(
        &self,
        name: &str,
        act: impl Fn(u32, A) -> mlua::Result<Result<(), String>> + 'static,
    ) -> mlua::Result<()> {
        let pid = self.pid;
        self.set(
            name,
            self.lua
                .create_function(move |_, args: A| -> mlua::Result<()> {
                    act(need_client(pid)?, args)?.map_err(mlua::Error::RuntimeError)
                })?,
        )
    }

    fn input<A: FromLuaMulti + 'static>(
        &self,
        name: &str,
        make: impl Fn(A) -> mlua::Result<Input> + 'static,
    ) -> mlua::Result<()> {
        let worker = self.worker.clone();
        self.action(name, move |pid, args| {
            let input = make(args)?;
            cancel_input(worker.as_deref());
            Ok(send_input(pid, &input))
        })
    }

    fn input_async<A: FromLuaMulti + 'static>(
        &self,
        name: &str,
        make: impl Fn(A) -> mlua::Result<Input> + 'static,
    ) -> mlua::Result<()> {
        let worker = self.worker.clone();
        self.action(name, move |_, args| {
            Ok(queue(&worker, Job::Input(make(args)?)))
        })
    }

    fn with_worker<A: FromLuaMulti, R: IntoLuaMulti>(
        &self,
        name: &str,
        call: impl Fn(Option<&Worker<Job>>, A) -> R + 'static,
    ) -> mlua::Result<()> {
        let worker = self.worker.clone();
        self.set(
            name,
            self.lua
                .create_function(move |_, args: A| Ok(call(worker.as_deref(), args)))?,
        )
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

#[cfg(feature = "embedded-lib")]
const LIB_MODULES: &[(&str, &str, &str)] = &[
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
    lib_module!("handoff", "main/handoff.luau"),
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
    lib_module!("anvil", "main/interfaces/mainscreen/anvil.luau"),
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
    lib_module!("shopmain", "main/interfaces/mainscreen/shopmain.luau"),
    lib_module!("boltpouch", "main/interfaces/mainscreen/boltpouch.luau"),
    lib_module!("runepouch", "main/interfaces/mainscreen/runepouch.luau"),
    lib_module!("bankpouch", "main/interfaces/mainscreen/bankpouch.luau"),
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
    lib_module!("item_data", "utils/item_data.luau"),
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
    lib_module!("consumables", "main/consumables.luau"),
];

#[cfg(not(feature = "embedded-lib"))]
const LIB_MODULES: &[(&str, &str, &str)] = &[];

fn module_name(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".luau")
        .or_else(|| file.strip_suffix(".lua"))
        .unwrap_or(file)
}

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

fn embedded_module(path: &str) -> Result<(&'static str, String), String> {
    let name = module_name(path);
    let &(_, file, source) = LIB_MODULES
        .iter()
        .find(|&&(n, _, _)| n == name)
        .ok_or_else(|| {
            let known: Vec<&str> = LIB_MODULES.iter().map(|&(n, _, _)| n).collect();
            format!("module '{path}' not found, lib has: {}", known.join(", "))
        })?;
    let source = match std::env::var_os("VOLCREN_LIB_DIR") {
        Some(dir) => std::fs::read_to_string(Path::new(&dir).join(file))
            .map_err(|e| format!("VOLCREN_LIB_DIR: cannot read {file}: {e}"))?,
        None => source.to_string(),
    };
    Ok((file, source))
}

fn lib_alias(path: &str) -> Option<&str> {
    path.strip_prefix("@lib/").filter(|rest| !rest.is_empty())
}

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

#[allow(clippy::too_many_arguments)]
pub fn run_with_modules(
    name: &str,
    source: &str,
    pid: Option<u32>,
    print: impl FnMut(String) + 'static,
    script_path: Option<&Path>,
    lib_dir: Option<&Path>,
    modules: HashMap<String, String>,
) -> mlua::Result<()> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT,
        LuaOptions::default(),
    )?;
    let print: Print = Rc::new(RefCell::new(print));
    let s = Sandbox {
        lua: &lua,
        globals: lua.globals(),
        pid,
        worker: pid.map(|pid| {
            Rc::new(input_jobs::worker(move |input: &Input, pace: &dyn Pace| {
                send_input_with(pid, input, pace)
            }))
        }),
    };
    s.globals.set("dofile", mlua::Nil)?;
    s.globals.set("loadfile", mlua::Nil)?;

    let script_dir = script_path.and_then(Path::parent);
    let dirs: Rc<RefCell<HashMap<String, PathBuf>>> = Rc::default();
    if let Some(dir) = script_dir {
        dirs.borrow_mut()
            .insert(format!("={name}"), dir.to_path_buf());
    }
    let memory: HashMap<PathBuf, String> = match script_dir {
        Some(dir) => modules
            .into_iter()
            .map(|(file, text)| (normalize(&dir.join(file)), text))
            .collect(),
        None => HashMap::new(),
    };
    let script_chunk = script_path.map(|_| format!("={name}"));
    let lib_root = lib_dir
        .map(Path::to_path_buf)
        .or_else(|| script_dir.map(|d| d.join("../lib")));
    let lib_dir = lib_dir.map(Path::to_path_buf);

    let loaded = lua.create_table()?;
    s.set(
        "require",
        lua.create_function(move |lua, path: String| {
            let alias = lib_alias(&path).map(str::to_string);
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

            let found = match (&alias, &lib_root, &lib_dir, lib_relative(&path)) {
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

    s.printing("print", &print, |_, args: MultiValue| {
        let line: Vec<String> = args
            .iter()
            .map(|v| v.to_string().unwrap_or_else(|_| "?".into()))
            .collect();
        Ok(without_separators(&line.join("\t")))
    })?;
    s.printing("report_counters", &print, |lua, counters: mlua::Value| {
        let counters: Vec<crate::form::Counter> = lua
            .from_value(counters)
            .map_err(|e| mlua::Error::RuntimeError(format!("report_counters: {e}")))?;
        if counters.iter().any(|c| !c.value.is_finite()) {
            return Err(mlua::Error::RuntimeError(
                "report_counters: values must be finite numbers".into(),
            ));
        }
        Ok(crate::form::counters_line(&counters))
    })?;
    s.set(
        "murmur2a",
        lua.create_function(|_, (text, seed): (mlua::LuaString, Option<f64>)| {
            Ok(crate::biometrics::murmur2a(
                &text.as_bytes(),
                seed.unwrap_or(0.0).rem_euclid(4294967296.0) as u32,
            ))
        })?,
    )?;
    s.set(
        "biometrics_profile",
        lua.create_function(|_, name: mlua::LuaString| {
            Ok(crate::biometrics::profile(&name.as_bytes()).to_vec())
        })?,
    )?;
    s.printing("report_breaks", &print, |lua, plan: mlua::Value| {
        let plan: crate::form::BreakPlan = lua
            .from_value(plan)
            .map_err(|e| mlua::Error::RuntimeError(format!("report_breaks: {e}")))?;
        Ok(crate::form::breaks_line(&plan))
    })?;
    s.printing(
        "report_exit",
        &print,
        |lua, (status, reason): (mlua::Value, Option<String>)| {
            let status: crate::form::ExitStatus = lua.from_value(status).map_err(|_| {
                mlua::Error::RuntimeError(
                    "report_exit: status is \"done\" or \"blocked\"".into(),
                )
            })?;
            let reason = reason.map(|r| without_separators(&r));
            Ok(crate::form::exit_line(&crate::form::Exit { status, reason }))
        },
    )?;
    s.printing("report_warning", &print, |_, text: mlua::LuaString| {
        let text = without_separators(&String::from_utf8_lossy(&text.as_bytes()));
        Ok(format!("{}{text}", crate::form::WARN_PREFIX))
    })?;
    s.printing(
        "report_stats",
        &print,
        |_, (experience, gold, custom): (f64, f64, Option<Vec<f64>>)| {
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
            Ok(crate::form::stats_line(
                experience.round() as i64,
                gold.round() as i64,
                &custom,
            ))
        },
    )?;

    let started = std::time::Instant::now();
    s.set(
        "time",
        lua.create_function(move |_, (): ()| Ok(started.elapsed().as_secs_f64() * 1000.0))?,
    )?;
    s.set(
        "clock",
        lua.create_function(|_, (): ()| Ok(local_time_of_day()))?,
    )?;
    s.set(
        "unix_time",
        lua.create_function(|_, (): ()| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64()))
        })?,
    )?;

    let events = Rc::new(RefCell::new(Callbacks::default()));
    let registry = events.clone();
    s.set(
        "on_item_change",
        lua.create_function(move |_, callback: mlua::Function| {
            let pid = need_client(pid)?;
            let mut events = registry.borrow_mut();
            if events.items.is_none() {
                events.items = Some(ItemWatcher::new(pid).map_err(mlua::Error::RuntimeError)?);
            }
            let id = events.next_id();
            events.item_callbacks.push((id, callback));
            Ok(id)
        })?,
    )?;
    let registry = events.clone();
    s.set(
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
    s.set(
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
    s.set(
        "on_packet",
        lua.create_function(
            move |_, (opcodes, callback, sent): (Vec<u16>, mlua::Function, Option<bool>)| {
                let filter = packet_filter("on_packet", &opcodes, sent)?;
                let mut events = registry.borrow_mut();
                let id = events.next_id();
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
    s.set(
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
    s.set(
        "wait_packet",
        lua.create_function(
            move |lua, (opcodes, ms, sent): (Vec<u16>, u64, Option<bool>)| {
                let filter = packet_filter("wait_packet", &opcodes, sent)?;
                let (previous, since) = {
                    let mut events = pump.borrow_mut();
                    let previous = events.waiting.replace(filter);
                    if let Err(error) = events.refresh_packets(pid) {
                        events.waiting = previous;
                        return Err(error);
                    }
                    (
                        previous,
                        events.packets.as_ref().map_or(0, PacketWatcher::written),
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
    s.set(
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

    s.set(
        "game_state",
        lua.create_function(move |lua, (): ()| -> mlua::Result<mlua::Value> {
            let state = game_state(need_client(pid)?).map_err(mlua::Error::RuntimeError)?;
            to_lua(lua, &state)
        })?,
    )?;
    s.set(
        "connection",
        lua.create_function(move |lua, (): ()| -> mlua::Result<mlua::Value> {
            let conn = connection(need_client(pid)?)
                .ok_or_else(|| mlua::Error::RuntimeError(NO_AGENT.into()))?;
            if let Some(problem) = conn.problem() {
                return Err(mlua::Error::RuntimeError(problem));
            }
            to_lua(lua, &conn)
        })?,
    )?;
    s.query("widget", |pid, (group, child): (u16, u16)| {
        widget(pid, group, child)
    })?;
    s.query("widget_model", |pid, (group, child): (u16, u16)| {
        widget_model(pid, group, child)
    })?;
    s.query("item_container", item_container)?;
    s.read("sub_interfaces", sub_interfaces)?;
    s.value("settings", crate::form::settings)?;
    s.value("finish_requested", crate::form::finish_requested)?;
    s.value("antiban_resume", crate::form::take_resume)?;
    s.value("form_update", crate::form::take_update)?;
    s.value("antiban_config", crate::antiban::current_value)?;
    s.value("transport_settings", crate::transports::current_settings)?;
    s.value("transports", crate::transports::current_value)?;
    #[cfg(test)]
    s.set(
        "test_set_transports",
        lua.create_function(|_, json: String| {
            crate::transports::set(
                crate::transports::parse(&json)
                    .ok_or_else(|| mlua::Error::RuntimeError("bad transports".into()))?,
            );
            Ok(())
        })?,
    )?;
    s.value("antiban_skips", crate::antiban::take_skips)?;
    #[cfg(test)]
    s.set(
        "test_skip",
        lua.create_function(|_, (kind, index): (String, u32)| {
            crate::antiban::push_skip(crate::antiban::Skip { kind, index });
            Ok(())
        })?,
    )?;
    #[cfg(test)]
    s.set(
        "test_set_antiban",
        lua.create_function(|_, json: String| {
            crate::antiban::set(
                crate::antiban::parse(&json)
                    .ok_or_else(|| mlua::Error::RuntimeError("bad config".into()))?,
            );
            Ok(())
        })?,
    )?;
    s.menu_query("menu", menu)?;
    s.read("menu_click", menu_click)?;
    s.read("entry_swaps", entry_swaps)?;
    s.menu_query("menu_open", menu_open)?;
    s.menu_query("menu_layout", menu_layout)?;
    s.read("selected_widget", selected_widget)?;
    s.query(
        "project",
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
                        get(&s, "x")?,
                        get(&s, "y")?,
                        get::<Option<i32>>(&s, "plane")?.unwrap_or(-1),
                    ],
                )?,
            };
            Ok(screen_points(screen))
        },
    )?;
    s.query("game_val_name", |pid, (kind, id): (u32, i32)| {
        Ok(game_val_names(pid, kind, &[id])?.pop().flatten())
    })?;
    s.query("game_val_names", |pid, (kind, ids): (u32, Vec<i32>)| {
        game_val_names(pid, kind, &ids)
    })?;
    s.query("item_name", |pid, id: i32| {
        Ok(item_names(pid, &[id])?.pop().flatten())
    })?;
    s.query("item_names", |pid, ids: Vec<i32>| item_names(pid, &ids))?;
    s.query("item_stackable", |pid, ids: Vec<i32>| {
        item_stackable(pid, &ids)
    })?;
    s.query("varc", varc)?;
    s.query("varp", varp_value)?;
    s.read("varps", varps)?;
    s.query("varbit", varbit)?;
    s.query("enum_def", enum_def)?;
    s.query_with(
        "cache_files",
        |pid, (index, wanted): (u32, Vec<[i32; 2]>)| cache_files(pid, index, &wanted),
        |lua, files| {
            let list = lua.create_table()?;
            for (i, file) in files.into_iter().enumerate() {
                list.set(i + 1, file_value(lua, file, mlua::Value::Boolean(false))?)?;
            }
            Ok(mlua::Value::Table(list))
        },
    )?;
    s.query_with(
        "cache_file",
        |pid, (index, archive, file): (u32, i32, i32)| {
            Ok(cache_files(pid, index, &[[archive, file]])?.pop().flatten())
        },
        |lua, file| file_value(lua, file, mlua::Value::Nil),
    )?;
    s.query("landmasses", |pid, points: Vec<mlua::Table>| {
        let points = points
            .iter()
            .map(|t| point_arg(t, "a point").map(|(x, y, plane)| [x, y, plane]))
            .collect::<Result<Vec<[i32; 3]>, String>>()?;
        landmasses(pid, &points)
    })?;
    s.query(
        "world_route",
        |pid, (from, to, allow): (mlua::Table, mlua::Table, Option<Vec<String>>)| {
            let route = world_route(
                pid,
                point_arg(&from, "a tile")?,
                point_arg(&to, "a tile")?,
                &allow.unwrap_or_default(),
            )?;
            Ok(WorldRoute::from(route))
        },
    )?;
    s.query("item_params", |pid, ids: Vec<i32>| item_params(pid, &ids))?;
    s.read("npcs", npcs)?;
    s.query("npc_model", |pid, index: i32| {
        Ok(npc_model(pid, index)?.map(script_model))
    })?;
    s.query("npc_sizes", |pid, indices: Vec<i32>| {
        npc_sizes(pid, &indices)
    })?;
    s.query("npc_hitsplats", |pid, indices: Vec<i32>| {
        npc_hitsplats(pid, &indices)
    })?;
    s.query("player_hitsplats", |pid, indices: Vec<i32>| {
        player_hitsplats(pid, &indices)
    })?;
    s.read("client_cycle", client_cycle)?;
    s.query("locs", locs)?;
    s.read("map_regions", map_regions)?;
    s.read("in_instance", in_instance)?;
    s.read("server_tile", server_tile)?;
    s.read("movement", movement)?;
    s.read("ground_items", ground_items)?;
    s.read("projectiles", projectiles)?;
    s.read("spot_anims", spot_anims)?;
    s.read("chat_messages", chat_messages)?;
    s.query(
        "find_path",
        |pid,
         (from, to, plane, max_tiles): (
            mlua::Table,
            mlua::Table,
            Option<i32>,
            Option<usize>,
        )| {
            let plane = match plane {
                Some(plane) => plane,
                None => player_plane(pid)?,
            };
            Ok(
                find_path(pid, plane, tile_arg(&from)?, tile_arg(&to)?, max_tiles)?
                    .into_iter()
                    .map(|[x, y]| PathTile { x, y })
                    .collect::<Vec<_>>(),
            )
        },
    )?;
    s.query("loc_shown", |pid, ids: Vec<u32>| loc_shown(pid, &ids))?;
    s.query(
        "cache_locs",
        |pid, (area, filter): (mlua::Table, Option<mlua::Table>)| {
            let area = area_arg(&area)?;
            let filter = match filter {
                Some(t) => crate::CacheLocFilter {
                    plane: get(&t, "plane")?,
                    names: get(&t, "names")?,
                    actions: get(&t, "actions")?,
                },
                None => crate::CacheLocFilter::default(),
            };
            cache_locs(pid, area, &filter)
        },
    )?;
    s.query("collision", |pid, area: Option<mlua::Table>| {
        let area = area.as_ref().map(area_arg).transpose()?;
        collision_cut(scene_collision(pid)?, area)
    })?;
    s.query(
        "scene_route",
        |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
            let options = options.as_ref().map(scene_options);
            let route = scene_route(pid, tile_arg(&from)?, tile_arg(&to)?, options.transpose()?)?;
            Ok(SceneRoute {
                tiles: route
                    .tiles
                    .into_iter()
                    .map(|(x, y)| PathTile { x, y })
                    .collect(),
                reached: route.reached,
            })
        },
    )?;
    s.query(
        "nav_plan",
        |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
            let point = |t: &mlua::Table| -> Result<crate::nav::Point, String> {
                let plane = match get(t, "plane")? {
                    Some(plane) => plane,
                    None => player_plane(pid)?,
                };
                Ok((get(t, "x")?, get(t, "y")?, plane))
            };
            let request = match options {
                Some(t) => nav_request(&t)?,
                None => crate::nav::Request::default(),
            };
            nav_plan(pid, point(&from)?, point(&to)?, &request)
        },
    )?;
    s.query("loc_model", |pid, loc: mlua::Table| {
        let field = |name: &str| get::<i32>(&loc, name);
        Ok(loc_model(
            pid,
            field("kind")? as u8,
            field("scene_x")?,
            field("scene_y")?,
            field("id")?,
        )?
        .map(script_model))
    })?;
    s.query(
        "npc_type_model",
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
    )?;
    s.read("players", players)?;
    s.query("player_model", |pid, index: i32| {
        Ok(player_model(pid, index)?.map(script_model))
    })?;
    s.read("world_select", world_select)?;
    s.query("widget_children", |pid, (group, child): (u16, u16)| {
        widget_children(pid, group, child)
    })?;

    let worker = s.worker.clone();
    s.set(
        "bank_pin_enter",
        lua.create_function(move |_, (): ()| -> mlua::Result<&'static str> {
            let pid = need_client(pid)?;
            cancel_input(worker.as_deref());
            bank_pin_enter(pid, &crate::async_input::Blocking)
                .map(crate::bank_pin::Outcome::name)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    s.read("lamp_skill", |pid| match game_state(pid)?.account_name {
        Some(account) => crate::lamp::skill_for(&account),
        None => Ok(None),
    })?;
    s.set(
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
    let client = s.pid;
    s.set(
        "item_prices",
        lua.create_function(
            move |lua, items: Vec<mlua::Value>| -> mlua::Result<(mlua::Value, Option<String>)> {
                let items = items
                    .into_iter()
                    .map(|item| match item {
                        mlua::Value::String(name) => {
                            Ok(crate::prices::Item::Name(name.to_str()?.to_owned()))
                        }
                        other => <i32 as mlua::FromLua>::from_lua(other, lua)
                            .map(crate::prices::Item::Id),
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                let names_of = |ids: &[i32]| match client {
                    Some(pid) => item_names(pid, ids),
                    None => Ok(vec![None; ids.len()]),
                };
                Ok(match crate::prices::lookup(&items, names_of) {
                    Ok(prices) => (to_lua(lua, &prices)?, None),
                    Err(error) => (mlua::Value::Nil, Some(error)),
                })
            },
        )?,
    )?;

    s.input("click", click_input)?;
    s.input("move", move_input)?;
    s.input("wind_mouse", wind_mouse_input)?;
    s.input("type_text", type_input)?;
    s.input("mouse_down", button_input(true))?;
    s.input("mouse_up", button_input(false))?;
    s.input("scroll", scroll_input)?;
    s.input("key_down", key_input(true))?;
    s.input("key_up", key_input(false))?;
    s.input_async("click_async", click_input)?;
    s.input_async("move_async", move_input)?;
    s.input_async("wind_mouse_async", wind_mouse_input)?;
    s.input_async("type_text_async", type_input)?;
    s.input_async("mouse_down_async", button_input(true))?;
    s.input_async("mouse_up_async", button_input(false))?;
    s.input_async("scroll_async", scroll_input)?;
    s.input_async("key_down_async", key_input(true))?;
    s.input_async("key_up_async", key_input(false))?;
    let worker = s.worker.clone();
    s.action("sleep_async", move |_, ms: u32| {
        Ok(queue(&worker, Job::Wait(ms)))
    })?;
    s.with_worker("input_busy", |worker, (): ()| {
        worker.is_some_and(Worker::busy)
    })?;
    s.with_worker("input_wait", |worker, (): ()| {
        worker.and_then(|worker| {
            worker.wait();
            worker.take_error()
        })
    })?;
    s.with_worker("input_error", |worker, (): ()| {
        worker.and_then(Worker::take_error)
    })?;
    s.with_worker("input_cancel", |worker, (): ()| cancel_input(worker))?;

    s.action("draw_rect", |pid, args: ShapeArgs| {
        Ok(draw(pid, &shape_args(false, args)))
    })?;
    s.action("draw_line", |pid, args: ShapeArgs| {
        Ok(draw(pid, &shape_args(true, args)))
    })?;
    s.action(
        "draw_lines",
        |pid, (lines, r, g, b, a): (Vec<mlua::Table>, u8, u8, u8, Option<u8>)| {
            let lines = lines
                .iter()
                .map(|line| rounded(line, [1, 2, 3, 4]))
                .collect::<mlua::Result<Vec<_>>>()?;
            Ok(draw_lines(pid, &lines, rgba(r, g, b, a)))
        },
    )?;
    s.action("clear_overlay", |pid, (): ()| {
        Ok(draw(pid, &Overlay::Clear))
    })?;
    s.action("draw_batch", |pid, shapes: Vec<mlua::Table>| {
        let overlays = shapes
            .iter()
            .map(overlay_arg)
            .collect::<mlua::Result<Vec<_>>>()?;
        Ok(draw_batch(pid, &overlays))
    })?;

    lua.sandbox(true)?;
    let result = lua.load(source).set_name(format!("={name}")).exec();
    if let Some(worker) = &s.worker {
        worker.shutdown();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_collect(
        name: &str,
        source: &str,
        script_path: Option<&Path>,
        lib_dir: Option<&Path>,
        modules: HashMap<String, String>,
    ) -> (Vec<String>, mlua::Result<()>) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let result = run_with_modules(
            name,
            source,
            None,
            move |line| collected.borrow_mut().push(line),
            script_path,
            lib_dir,
            modules,
        );
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    fn run_lines(source: &str) -> (Vec<String>, mlua::Result<()>) {
        run_collect("test.luau", source, None, None, HashMap::new())
    }

    fn run_script_at(
        path: &Path,
        source: &str,
        lib_dir: Option<&Path>,
    ) -> (Vec<String>, mlua::Result<()>) {
        let name = path.file_name().unwrap().to_string_lossy();
        run_collect(&name, source, Some(path), lib_dir, HashMap::new())
    }

    #[test]
    fn collision_cut_clips_the_area_to_the_scene() {
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
    fn handoff_globals_read_what_the_loader_sent() {
        crate::form::set_settings(serde_json::json!({ "eat_at": 40 }));
        crate::form::set_resume(crate::form::BreakPlan {
            breaks: vec![crate::form::PlannedBreak {
                remaining: 5000.0,
                length: 60000.0,
                at: None,
                index: 1,
            }],
            ..Default::default()
        });
        let (lines, result) = run_lines(
            "local s = settings() print(s.eat_at) print(finish_requested()) local r = antiban_resume() print(r.breaks[1].index, r.breaks[1].remaining) print(antiban_resume() == nil)",
        );
        result.unwrap();
        assert_eq!(lines, ["40", "false", "1\t5000", "true"]);
        crate::form::request_finish();
        let (lines, result) = run_lines("print(finish_requested())");
        result.unwrap();
        assert_eq!(lines, ["true"]);
    }

    #[test]
    fn report_exit_writes_an_exit_line() {
        let (lines, result) = run_lines("report_exit('blocked', 'out of food')");
        result.unwrap();
        let exit =
            crate::form::parse_exit(lines[0].strip_prefix(crate::form::EXIT_PREFIX).unwrap())
                .unwrap();
        assert_eq!(exit.status, crate::form::ExitStatus::Blocked);
        assert_eq!(exit.reason.as_deref(), Some("out of food"));
        assert!(run_lines("report_exit('done')").1.is_ok());
        assert!(run_lines("report_exit('failed')").1.is_err());
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
        let source = "local Util = require('./util')\nprint(Util.name, Util.other, require('../lib/host').name, require('./util') == Util)";
        let (lines, result) = run_collect("run.luau", source, Some(&script), None, modules);
        result.unwrap();
        assert_eq!(lines, ["util\tdeeper\thost\ttrue"]);
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
            None,
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
            None,
        );
        result.unwrap();
        assert_eq!(lines, ["true\thost\tother"]);
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

        let (lines, result) = run_script_at(&script, source, Some(&dir.join("mine")));
        result.unwrap();
        assert_eq!(lines, ["mine\tmine"]);

        let (lines, result) = run_script_at(&script, "print(require('../lib/host').name)", None);
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
        let (_, result) = run_script_at(
            &dir.join("scripts/run.luau"),
            "require('../lib/nope')",
            Some(&dir.join("mine")),
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
        let (_, result) = run_script_at(&dir.join("run.luau"), "require('../lib/nope')", None);
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
        let (_, result) = run_script_at(&dir.join("run.luau"), "require('./lib/bad')", None);
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
             plan('{\"breaks\":[{\"interval\":600000,\"length\":90000,\"randomness\":0.0,\"logout_chance\":0.0}],\"sleeps\":[{}]}')\n\
             assert(A.breaks[1].length == 90000 and A.breaks[1].next == due)\n\
             assert(A.sleeps[1].next == sleep_due)\n\
             plan('{\"breaks\":[{\"interval\":300000,\"length\":90000,\"randomness\":0.0,\"logout_chance\":0.5},{\"interval\":60000,\"length\":10000,\"randomness\":0.0}],\"sleeps\":[]}')\n\
             assert(#A.breaks == 2 and #A.sleeps == 0)\n\
             assert(A.breaks[1].next ~= due and A.breaks[1].logout_chance == 0.5)\n\
             assert(A.breaks[2].logout_chance == 0.01)\n\
             local second = A.breaks[2].next\n\
             test_skip('break', 1); test_skip('break', 7); test_skip('sleep', 0)\n\
             assert(A.sync_config())\n\
             assert(A.breaks[2].next == second + 60000 and A.breaks[1].next < second + 400000)\n\
             assert(not A.sync_config())\n\
             plan('{\"breaks_enabled\":false,\"sleeps_enabled\":false}')\n\
             assert(#A.breaks == 0 and #A.sleeps == 0 and not A.break_due())\n\
             print('ok')",
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

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

    #[cfg(feature = "embedded-lib")]
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
Randoms.LAMP_WAIT_MS = 50
local function lamp_index()
    for i, h in stage.inventory do
        if h.id == Randoms.GENIE_LAMP then return i end
    end
    return nil
end
Lamp.slot = function() local i = lamp_index() return if i then stage.inventory[i].slot else nil end
Lamp.use = function(skill) stage.used = skill table.remove(stage.inventory, lamp_index()) return true end
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
local oasis = tile(3050, 3000)
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
stage.inventory = {}
c = T.choose(home, inside)
assert(c and c.kind == 'fairy_rings' and c.transport.label == 'DIQ', 'a ring underfoot goes into the house once it is known')
stage.inventory = { { id = 8013, quantity = 1, slot = 0 } }
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
    fn consumables_work_out_heals_boosts_drains_and_ranges_from_given_levels() {
        let (lines, result) = run_lines(
            "local C = require('consumables')\n\
             local function at(hp, real)\n\
               local l = {}\n\
               for _, s in ipairs({ 'Attack', 'Strength', 'Defence', 'Ranged', 'Magic', 'Prayer', 'Herblore', 'Cooking', 'Fishing' }) do\n\
                 l[s] = { level = 99, real = 99 }\n\
               end\n\
               l.Hitpoints = { level = hp, real = real or 99 }\n\
               l['Run energy'] = { level = 50, real = 100 }\n\
               return l\n\
             end\n\
             local shark = C.get(385)\n\
             assert(shark and shark.name == 'Shark' and shark.option == 'Eat' and not shark.combo)\n\
             assert(C.get(1) == nil and C.result(1, at(50)) == nil)\n\
             assert(C.result(385, at(50)).Hitpoints.low == 70, 'shark heals 20')\n\
             assert(C.result(385, at(95)).Hitpoints.high == 99, 'food stops at the real level')\n\
             assert(C.result(13441, at(99)).Hitpoints.low == 121, 'anglerfish overheals 22 at 99')\n\
             local brew = C.result(6685, at(99))\n\
             assert(brew.Hitpoints.low == 115 and brew.Defence.low == 120 and brew.Attack.low == 88, 'saradomin brew')\n\
             assert(C.result(2450, at(99)).Hitpoints.low == 88, 'zamorak brew takes 12% of current hitpoints')\n\
             assert(C.result(29149, at(50)).Hitpoints.low == 64, 'graahk heals 8, then 6')\n\
             local snail = C.result(3369, at(50)).Hitpoints\n\
             assert(snail.from == 50 and snail.low == 55 and snail.high == 57, 'thin snail heals 5 to 7')\n\
             local l = at(99)\n\
             l.Prayer = { level = 0, real = 70 }\n\
             assert(C.result(3024, l).Prayer.low == 25, 'super restore: 8 + 25% of 70')\n\
             assert(C.get(3144).combo and C.get(2452).option == 'Drink')\n\
             local names = {}\n\
             for _, c in ipairs(C.raising('Hitpoints', 1, at(1))) do names[c.name] = true end\n\
             assert(names.Shark and names['Saradomin brew'] and not names['Prayer potion'], 'raising hitpoints')\n\
             local fire = {}\n\
             for _, c in ipairs(C.with_special('antifire')) do fire[c.name] = c.special[1].ticks end\n\
             assert(fire['Antifire potion'] == 600 and fire['Extended antifire mix'] == 1200, 'antifire specials')\n\
             print('ok')",
        );
        assert!(result.is_ok(), "{result:?} {lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("ok"), "{lines:?}");
    }

    #[test]
    #[cfg(feature = "embedded-lib")]
    fn cs2_scripts_and_database_rows_decode_and_bad_bytes_are_refused() {
        let (lines, result) = run_lines(
            "local Cs2 = require('cs2')\n\
             local code = '\\0' .. string.pack('>I2i4', 0, 7) .. string.pack('>I2i4', 60, 0) .. string.pack('>I2i4', 25, 99) .. string.pack('>I2B', 21, 0)\n\
             local tables = string.char(1) .. string.pack('>I2', 1) .. string.pack('>i4i4', 5, 1)\n\
             local bytes = code .. string.pack('>i4', 4) .. string.rep('\\0', 12) .. tables .. string.pack('>I2', #tables)\n\
             local s = Cs2.script(bytes)\n\
             assert(s and #s.instructions == 4, 'script')\n\
             assert(s.instructions[3].op == Cs2.GET_VARBIT and s.instructions[3].value == 99)\n\
             assert(s.tables[1][5] == 1)\n\
             assert(Cs2.script('not a script at all, just text') == nil)\n\
             assert(Cs2.script(string.sub(bytes, 1, #bytes - 5)) == nil)\n\
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
