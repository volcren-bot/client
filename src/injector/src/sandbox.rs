use std::{cell::RefCell, collections::HashMap, path::{Component, Path, PathBuf}, rc::Rc, sync::Arc};

use mlua::{FromLuaMulti, Lua, LuaOptions, LuaSerdeExt, MultiValue, StdLib, serde::SerializeOptions};
use serde::Serialize;

use crate::{Input, MouseButton, Overlay, Rgba, async_input::{Pace, Worker}, input_jobs::{self, Job}, projection::Via, wind_mouse};

#[cfg(windows)]
use crate::{cache_locs, nav_plan, chat_messages, connection, draw, draw_batch, draw_lines, enum_def, selected_widget, find_path, item_params, varbit, game_state, game_val_names, ground_items, in_instance, map_regions, player_plane, scene_collision, scene_route, menu, menu_click, menu_layout, menu_open, project, project_moved, item_container, item_names, loc_model, locs, npc_hitsplats, npc_model, npc_sizes, npcs, sub_interfaces, player_model, players, projectiles, send_input, send_input_with, spot_anims, varc, varp_value, varps, widget, widget_children, world_select};
#[cfg(not(windows))]
use offline::*;

#[cfg(not(windows))]
mod offline {
    use crate::{ChatMessage, Connection, EntityModel, GameState, GroundItem, Input, Item, Loc, MenuClick, MenuEntry, MenuLayout, Npc, Overlay, Player, Projectile, SpotAnim, SubInterface, Widget, WorldSelect};

    const NO_WIN32: &str = "clients can only be reached from Windows or Wine";

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
    pub fn project(_: u32, _: &[crate::projection::Point], _: crate::projection::Via) -> Result<Vec<[f32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn project_moved(_: u32, _: &[crate::projection::Point], _: [i32; 2]) -> Result<Vec<[f32; 2]>, String> {
        Err(NO_WIN32.into())
    }
    pub fn game_val_names(_: u32, _: u32, _: &[i32]) -> Result<Vec<Option<String>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_names(_: u32, _: &[i32]) -> Result<Vec<Option<String>>, String> {
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
    pub fn enum_def(_: u32, _: i32) -> Result<Option<crate::config::EnumDef>, String> {
        Err(NO_WIN32.into())
    }
    pub fn item_params(_: u32, _: &[i32]) -> Result<Vec<Option<std::collections::BTreeMap<u32, crate::config::Value>>>, String> {
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
    pub fn cache_locs(_: u32, _: [i32; 4], _: &crate::CacheLocFilter) -> Result<Vec<crate::CacheLoc>, String> {
        Err(NO_WIN32.into())
    }
    pub fn find_path(_: u32, _: i32, _: [i32; 2], _: [i32; 2], _: Option<usize>) -> Result<Vec<[i32; 2]>, String> {
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
    pub fn npc_hitsplats(_: u32, _: &[i32]) -> Result<std::collections::BTreeMap<i32, Vec<decode::world::Hitsplat>>, String> {
        Err(NO_WIN32.into())
    }
    pub fn scene_collision(_: u32) -> Result<(i32, i32, i32, Vec<Vec<u32>>), String> {
        Err(NO_WIN32.into())
    }
    pub fn scene_route(_: u32, _: [i32; 2], _: [i32; 2], _: Option<crate::scene_graph::Options>) -> Result<crate::pathfinding::Route, String> {
        Err(NO_WIN32.into())
    }
    pub fn nav_plan(_: u32, _: crate::nav::Point, _: crate::nav::Point, _: &crate::nav::Request) -> Result<crate::nav::NavPlan, String> {
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
}

fn script_model(m: crate::EntityModel) -> ScriptModel {
    let points = |points: Vec<[f32; 2]>| points.into_iter().map(|[x, y]| ScreenPoint { x, y }).collect();
    ScriptModel {
        bounding_box: m.bounding_box,
        orientation: m.orientation,
        vertex_count: m.vertex_count,
        corners: points(m.corners),
        hull: points(m.hull),
        faces: m.faces.into_iter().map(|face| face.map(|[x, y]| ScreenPoint { x, y })).collect(),
    }
}


fn to_lua<T: Serialize>(lua: &Lua, value: &T) -> mlua::Result<mlua::Value> {
    lua.to_value_with(value, SerializeOptions::new().serialize_none_to_null(false).serialize_unit_to_null(false))
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

// `scene` cut to `area` (x1, y1, x2, y2, corners included), or whole without one.
fn collision_cut(scene: (i32, i32, i32, Vec<Vec<u32>>), area: Option<[i32; 4]>) -> Result<Collision, String> {
    let (x0, y0, plane, columns) = scene;
    let height = columns.first().map_or(0, |c| c.len() as i32);
    let (x2_max, y2_max) = (x0 + columns.len() as i32 - 1, y0 + height - 1);
    let [x1, y1, x2, y2] = area.unwrap_or([x0, y0, x2_max, y2_max]);
    let (x1, y1, x2, y2) = (x1.min(x2).max(x0), y1.min(y2).max(y0), x1.max(x2).min(x2_max), y1.max(y2).min(y2_max));
    if x1 > x2 || y1 > y2 {
        return Err("the area is outside the loaded scene".into());
    }
    let flags = (x1..=x2).flat_map(|x| columns[(x - x0) as usize][(y1 - y0) as usize..=(y2 - y0) as usize].iter().copied()).collect();
    Ok(Collision { x: x1, y: y1, width: x2 - x1 + 1, height: y2 - y1 + 1, plane, flags })
}

fn query<A: FromLuaMulti, T: Serialize>(
    lua: &Lua,
    pid: Option<u32>,
    read: impl Fn(u32, A) -> Result<T, String> + Send + 'static,
) -> mlua::Result<mlua::Function> {
    lua.create_function(move |lua, args: A| -> mlua::Result<(mlua::Value, Option<String>)> {
        let Some(pid) = pid else {
            return Ok((mlua::Value::Nil, Some(NO_CLIENT.into())));
        };
        match read(pid, args) {
            Ok(value) => Ok((to_lua(lua, &value)?, None)),
            Err(error) => Ok((mlua::Value::Nil, Some(error))),
        }
    })
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
        Ok(worker.as_ref().map_or(Err(NO_CLIENT.into()), |worker| worker.push(Job::Input(input))))
    })
}

// A query of the live menu, run after taking the input back.
fn menu_read<T: 'static>(worker: &InputWorker, read: fn(u32) -> Result<T, String>) -> impl Fn(u32, ()) -> Result<T, String> + Send + 'static {
    let worker = worker.clone();
    move |pid, ()| {
        cancel_input(&worker);
        read(pid)
    }
}

fn button_arg(name: Option<String>) -> mlua::Result<MouseButton> {
    name.map_or(Ok(MouseButton::Left), |name| MouseButton::parse(&name).map_err(mlua::Error::RuntimeError))
}

// A `draw_batch` shape: `{ kind = "rect" | "line", x1, y1, x2, y2, r, g, b, a? }` or `{ kind = "clear" }`.
fn overlay_arg(shape: &mlua::Table) -> mlua::Result<Overlay> {
    let kind: String = shape.get("kind")?;
    if kind == "clear" {
        return Ok(Overlay::Clear);
    }
    let [x1, y1, x2, y2] = ["x1", "y1", "x2", "y2"].map(|k| shape.get::<f64>(k).map(|v| v.round() as i32));
    let (x1, y1, x2, y2) = (x1?, y1?, x2?, y2?);
    let color = Rgba { r: shape.get("r")?, g: shape.get("g")?, b: shape.get("b")?, a: shape.get::<Option<u8>>("a")?.unwrap_or(255) };
    match kind.as_str() {
        "rect" => Ok(Overlay::Rect { x1, y1, x2, y2, color }),
        "line" => Ok(Overlay::Line { x1, y1, x2, y2, color }),
        _ => Err(mlua::Error::RuntimeError(format!("unknown shape kind: {kind}"))),
    }
}

#[cfg(feature = "embedded-lib")]
macro_rules! lib_module {
    ($name:literal, $path:literal) => {
        ($name, $path, include_str!(concat!("../../../../lib/", $path)))
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
    lib_module!("login_world_switcher", "main/interfaces/login/login_world_switcher.luau"),
    lib_module!("logout", "main/interfaces/gametabs/logout.luau"),
    lib_module!("worldswitcher", "main/interfaces/gametabs/worldswitcher.luau"),
    lib_module!("stats", "main/interfaces/gametabs/stats.luau"),
    lib_module!("welcome_screen", "main/interfaces/login/welcome_screen.luau"),
    lib_module!("inventory", "main/interfaces/gametabs/inventory.luau"),
    lib_module!("wornitems", "main/interfaces/gametabs/wornitems.luau"),
    lib_module!("side_panels", "main/interfaces/gametabs/side_panels.luau"),
    lib_module!("magic", "main/interfaces/gametabs/magic.luau"),
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
    lib_module!("bankpin_keypad", "main/interfaces/mainscreen/bankpin_keypad.luau"),
    lib_module!("depositbox", "main/interfaces/mainscreen/depositbox.luau"),
    lib_module!("toplevel", "main/interfaces/mainscreen/toplevel.luau"),
    lib_module!("widgets", "main/interfaces/widgets.luau"),
    lib_module!("chatbox", "main/interfaces/chatbox/chatbox.luau"),
    lib_module!("recommend_survey", "main/interfaces/chatbox/recommend_survey.luau"),
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
    lib_module!("geometry", "utils/math/geometry.luau"),
    lib_module!("mouse", "utils/input/mouse.luau"),
    lib_module!("keyboard", "utils/input/keyboard.luau"),
    lib_module!("ticks", "utils/ticks.luau"),
    lib_module!("stats_report", "utils/stats_report.luau"),
    lib_module!("log", "utils/log.luau"),
];

#[cfg(not(feature = "embedded-lib"))]
pub const LIB_MODULES: &[(&str, &str, &str)] = &[];

// A point for `project`: optional `z` and a `tile` (table with `x` and `y`) naming the tile it stands on.
fn fine_point(p: &mlua::Table) -> mlua::Result<crate::projection::Point> {
    let tile = match p.get::<Option<mlua::Table>>("tile")? {
        Some(t) => Some([t.get::<i32>("x")?, t.get::<i32>("y")?]),
        None => None,
    };
    Ok(crate::projection::Point { tile, ..crate::projection::Point::new(p.get("x")?, p.get("y")?, p.get::<Option<i32>>("z")?.unwrap_or(0)) })
}

fn module_name(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".luau").or_else(|| file.strip_suffix(".lua")).unwrap_or(file)
}

// Seconds since local midnight, with the milliseconds as the fraction.
#[cfg(windows)]
fn local_time_of_day() -> f64 {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    f64::from(now.wHour) * 3600.0 + f64::from(now.wMinute) * 60.0 + f64::from(now.wSecond) + f64::from(now.wMilliseconds) / 1000.0
}

#[cfg(not(windows))]
fn local_time_of_day() -> f64 {
    let since_epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let seconds = since_epoch.as_secs() as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut local) };
    f64::from(local.tm_hour) * 3600.0 + f64::from(local.tm_min) * 60.0 + f64::from(local.tm_sec) + f64::from(since_epoch.subsec_millis()) / 1000.0
}

pub fn run(name: &str, source: &str, pid: Option<u32>, print: impl FnMut(String) + 'static) -> mlua::Result<()> {
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

// The file `require("../lib/host")` names from `dir`: `host.luau`, `host.lua` or `host/init.luau`.
// `memory` holds files that exist only in memory (a premium script's own files), keyed by their normalized path.
fn find_module(dir: &Path, path: &str, memory: &HashMap<PathBuf, String>) -> Result<PathBuf, Vec<PathBuf>> {
    let base = normalize(&dir.join(path));
    let with = |suffix: &str| {
        let mut file = base.clone().into_os_string();
        file.push(suffix);
        PathBuf::from(file)
    };
    let candidates = [with(".luau"), with(".lua"), base.join("init.luau")];
    candidates.iter().find(|c| memory.contains_key(*c) || c.is_file()).cloned().ok_or_else(|| candidates.into())
}

// A module that ships inside the injector (`LIB_MODULES`), looked up by its bare name.
fn embedded_module(path: &str) -> Result<(&'static str, String), String> {
    let name = module_name(path);
    let &(_, file, source) = LIB_MODULES.iter().find(|&&(n, _, _)| n == name).ok_or_else(|| {
        let known: Vec<&str> = LIB_MODULES.iter().map(|&(n, _, _)| n).collect();
        format!("module '{path}' not found, lib has: {}", known.join(", "))
    })?;
    // For working on lib/ without rebuilding: VOLCREN_LIB_DIR points at a lib/ to read instead.
    let source = match std::env::var_os("VOLCREN_LIB_DIR") {
        Some(dir) => std::fs::read_to_string(Path::new(&dir).join(file)).map_err(|e| format!("VOLCREN_LIB_DIR: cannot read {file}: {e}"))?,
        None => source.to_string(),
    };
    Ok((file, source))
}

// `require("@lib/main/walker")` is `require("../lib/main/walker")`, so a script can name the lib without counting folders.
// It matches the `lib` alias in a script's `.luaurc`, which is what `luau-analyze` reads.
fn alias_lib(path: String) -> String {
    match path.strip_prefix("@lib/") {
        Some(rest) => format!("../lib/{rest}"),
        None => path,
    }
}

// What `require("../lib/main/walker")` asks for inside the lib, when the path names a `lib` folder: `main/walker`.
fn lib_relative(path: &str) -> Option<&str> {
    let mut rest = path;
    let mut stepped = false;
    while let Some(next) = rest.strip_prefix("../").or_else(|| rest.strip_prefix("./")) {
        rest = next;
        stepped = true;
    }
    rest.strip_prefix("lib/").filter(|r| stepped && !r.is_empty())
}

// `script_path` is the file the script was read from; without it only modules embedded in the injector can be required.
// `lib_dir` is the developer's own lib folder: the script's `require("../lib/...")` reads from it instead of from beside the script.
pub fn run_with_form(
    name: &str,
    source: &str,
    pid: Option<u32>,
    print: impl FnMut(String) + 'static,
    form: Option<crate::form::FormHandler>,
    script_path: Option<&Path>,
    lib_dir: Option<&Path>,
) -> mlua::Result<()> {
    run_with_modules(name, source, pid, print, form, script_path, lib_dir, HashMap::new())
}

// `modules` are the script's other files by name, kept in memory instead of on disk: `require("./name")` finds them
// beside `script_path` as if they were there, so a script's source never has to be written out.
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
    let worker: InputWorker = pid.map(|pid| Arc::new(input_jobs::worker(move |input: &Input, pace: &dyn Pace| send_input_with(pid, input, pace))));
    globals.set("dofile", mlua::Nil)?;
    globals.set("loadfile", mlua::Nil)?;

    // Chunk name -> the directory its relative requires start from, for the script and every module read from disk.
    let dirs: Rc<RefCell<HashMap<String, PathBuf>>> = Rc::default();
    if let Some(dir) = script_path.and_then(Path::parent) {
        dirs.borrow_mut().insert(format!("={name}"), dir.to_path_buf());
    }

    let memory: HashMap<PathBuf, String> = match script_path.and_then(Path::parent) {
        Some(dir) => modules.into_iter().map(|(file, text)| (normalize(&dir.join(file)), text)).collect(),
        None => HashMap::new(),
    };
    let script_chunk = script_path.map(|_| format!("={name}"));
    let lib_dir = lib_dir.map(Path::to_path_buf);

    let loaded = lua.create_table()?;
    globals.set(
        "require",
        lua.create_function(move |lua, path: String| {
            let path = alias_lib(path);
            let relative = path.starts_with("./") || path.starts_with("../");
            let from = lua.inspect_stack(1, |d| d.source().source.map(|s| s.into_owned())).flatten();
            let from_script = from.is_some() && from == script_chunk;
            let dir = from.and_then(|chunk| dirs.borrow().get(&chunk).cloned()).filter(|_| relative);

            // The script's own `../lib/...` requires read from the configured lib folder; the modules inside it find each other as usual.
            let found = match (&lib_dir, lib_relative(&path)) {
                (Some(lib), Some(rest)) if from_script => Some(find_module(lib, rest, &memory)),
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
                        None => std::fs::read_to_string(&file).map_err(|e| mlua::Error::RuntimeError(format!("cannot read {key}: {e}")))?,
                    };
                    dirs.borrow_mut().insert(format!("={key}"), file.parent().map(Path::to_path_buf).unwrap_or_default());
                    (key.clone(), source)
                }
                found => {
                    let (file, source) = embedded_module(&path).map_err(|embedded| match found {
                        Some(Err(tried)) => {
                            let tried: Vec<String> = tried.iter().map(|p| p.display().to_string()).collect();
                            mlua::Error::RuntimeError(format!("module '{path}' not found, tried: {}", tried.join(", ")))
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
            let line: Vec<String> = args.iter().map(|v| v.to_string().unwrap_or_else(|_| "?".into())).collect();
            (printer.borrow_mut())(line.join("\t"));
            Ok(())
        })?,
    )?;
    // The loader shows these under the skills in its XP Tracker tab and works out the rates itself; each call replaces the last.
    let counters_print = print.clone();
    globals.set(
        "report_counters",
        lua.create_function_mut(move |lua, counters: mlua::Value| {
            let counters: Vec<crate::form::Counter> =
                lua.from_value(counters).map_err(|e| mlua::Error::RuntimeError(format!("report_counters: {e}")))?;
            if counters.iter().any(|c| !c.value.is_finite()) {
                return Err(mlua::Error::RuntimeError("report_counters: values must be finite numbers".into()));
            }
            (counters_print.borrow_mut())(crate::form::counters_line(&counters));
            Ok(())
        })?,
    )?;
    // The loader shows these in its Breaks tab, in any run, and counts the times down itself.
    let breaks_print = print.clone();
    globals.set(
        "report_breaks",
        lua.create_function_mut(move |lua, plan: mlua::Value| {
            let plan: crate::form::BreakPlan = lua.from_value(plan).map_err(|e| mlua::Error::RuntimeError(format!("report_breaks: {e}")))?;
            (breaks_print.borrow_mut())(crate::form::breaks_line(&plan));
            Ok(())
        })?,
    )?;
    // The loader sums these between heartbeats. Outside a store run it has nowhere to send them and drops them.
    globals.set(
        "report_stats",
        lua.create_function_mut(move |_, (experience, gold, custom): (f64, f64, Option<Vec<f64>>)| {
            if !experience.is_finite() || !gold.is_finite() || experience < 0.0 {
                return Err(mlua::Error::RuntimeError("report_stats: experience must be a number of 0 or more, and gold a number".into()));
            }
            let custom: Vec<i64> = custom.unwrap_or_default().into_iter().map(|value| value.round() as i64).collect();
            (print.borrow_mut())(crate::form::stats_line(experience.round() as i64, gold.round() as i64, &custom));
            Ok(())
        })?,
    )?;

    let started = std::time::Instant::now();
    globals.set("time", lua.create_function(move |_, (): ()| Ok(started.elapsed().as_secs_f64() * 1000.0))?)?;
    globals.set("clock", lua.create_function(|_, (): ()| Ok(local_time_of_day()))?)?;

    globals.set(
        "sleep",
        lua.create_function(|_, ms: u64| {
            std::thread::sleep(std::time::Duration::from_millis(ms));
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
    globals.set("widget", query(&lua, pid, |pid, (group, child): (u16, u16)| widget(pid, group, child))?)?;
    globals.set("item_container", query(&lua, pid, |pid, id: i32| item_container(pid, id))?)?;
    globals.set("sub_interfaces", query(&lua, pid, |pid, (): ()| sub_interfaces(pid))?)?;
    let form = std::cell::RefCell::new(form);
    globals.set(
        "form",
        lua.create_function(move |lua, spec: mlua::Value| -> mlua::Result<(mlua::Value, Option<String>)> {
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
        })?,
    )?;
    // The live menu has to be read with the mouse still, so these take the input back first.
    globals.set("menu", query(&lua, pid, menu_read(&worker, menu))?)?;
    globals.set("menu_click", query(&lua, pid, |pid, (): ()| menu_click(pid))?)?;
    globals.set("menu_open", query(&lua, pid, menu_read(&worker, menu_open))?)?;
    globals.set("menu_layout", query(&lua, pid, menu_read(&worker, menu_layout))?)?;
    globals.set("selected_widget", query(&lua, pid, |pid, (): ()| selected_widget(pid))?)?;
    globals.set(
        "project",
        query(&lua, pid, |pid, (points, via, shift): (Vec<mlua::Table>, Option<String>, Option<mlua::Table>)| {
            let via = Via::parse(via.as_deref())?;
            let points = points.iter().map(fine_point).collect::<mlua::Result<Vec<_>>>().map_err(|e| e.to_string())?;
            let screen = match shift {
                None => project(pid, &points, via)?,
                Some(_) if via == Via::Client => return Err("the client's projection can't move the camera".into()),
                Some(s) => project_moved(pid, &points, [s.get("x").map_err(|e| e.to_string())?, s.get("y").map_err(|e| e.to_string())?])?,
            };
            Ok(screen.into_iter().map(|[x, y]| ScreenPoint { x, y }).collect::<Vec<_>>())
        })?,
    )?;
    globals.set("game_val_name", query(&lua, pid, |pid, (kind, id): (u32, i32)| Ok(game_val_names(pid, kind, &[id])?.pop().flatten()))?)?;
    globals.set("game_val_names", query(&lua, pid, |pid, (kind, ids): (u32, Vec<i32>)| game_val_names(pid, kind, &ids))?)?;
    globals.set("item_name", query(&lua, pid, |pid, id: i32| Ok(item_names(pid, &[id])?.pop().flatten()))?)?;
    globals.set("item_names", query(&lua, pid, |pid, ids: Vec<i32>| item_names(pid, &ids))?)?;
    globals.set("varc", query(&lua, pid, |pid, id: i32| varc(pid, id))?)?;
    globals.set("varp", query(&lua, pid, |pid, id: usize| varp_value(pid, id))?)?;
    globals.set("varps", query(&lua, pid, |pid, (): ()| varps(pid))?)?;
    globals.set("varbit", query(&lua, pid, |pid, id: i32| varbit(pid, id))?)?;
    globals.set("enum_def", query(&lua, pid, |pid, id: i32| enum_def(pid, id))?)?;
    globals.set("item_params", query(&lua, pid, |pid, ids: Vec<i32>| item_params(pid, &ids))?)?;
    globals.set("npcs", query(&lua, pid, |pid, (): ()| npcs(pid))?)?;
    globals.set("npc_model", query(&lua, pid, |pid, index: i32| Ok(npc_model(pid, index)?.map(script_model)))?)?;
    globals.set("npc_sizes", query(&lua, pid, |pid, indices: Vec<i32>| npc_sizes(pid, &indices))?)?;
    globals.set("npc_hitsplats", query(&lua, pid, |pid, indices: Vec<i32>| npc_hitsplats(pid, &indices))?)?;
    globals.set("locs", query(&lua, pid, |pid, plane: Option<i32>| locs(pid, plane))?)?;
    globals.set("map_regions", query(&lua, pid, |pid, (): ()| map_regions(pid))?)?;
    globals.set("in_instance", query(&lua, pid, |pid, (): ()| in_instance(pid))?)?;
    globals.set("ground_items", query(&lua, pid, |pid, (): ()| ground_items(pid))?)?;
    globals.set("projectiles", query(&lua, pid, |pid, (): ()| projectiles(pid))?)?;
    globals.set("spot_anims", query(&lua, pid, |pid, (): ()| spot_anims(pid))?)?;
    globals.set("chat_messages", query(&lua, pid, |pid, (): ()| chat_messages(pid))?)?;
    globals.set(
        "find_path",
        query(&lua, pid, |pid, (from, to, plane, max_tiles): (mlua::Table, mlua::Table, Option<i32>, Option<usize>)| {
            let tile = |t: &mlua::Table| -> Result<[i32; 2], String> { Ok([t.get("x").map_err(|e| e.to_string())?, t.get("y").map_err(|e| e.to_string())?]) };
            let plane = match plane {
                Some(plane) => plane,
                None => player_plane(pid)?,
            };
            Ok(find_path(pid, plane, tile(&from)?, tile(&to)?, max_tiles)?.into_iter().map(|[x, y]| PathTile { x, y }).collect::<Vec<_>>())
        })?,
    )?;
    globals.set(
        "cache_locs",
        query(&lua, pid, |pid, (area, filter): (mlua::Table, Option<mlua::Table>)| {
            let corner = |name: &str| area.get::<i32>(name).map_err(|e| e.to_string());
            let area = [corner("x1")?, corner("y1")?, corner("x2")?, corner("y2")?];
            let filter = match filter {
                Some(t) => crate::CacheLocFilter {
                    plane: t.get::<Option<i32>>("plane").map_err(|e| e.to_string())?,
                    names: t.get::<Option<Vec<String>>>("names").map_err(|e| e.to_string())?,
                    actions: t.get::<Option<Vec<String>>>("actions").map_err(|e| e.to_string())?,
                },
                None => crate::CacheLocFilter::default(),
            };
            cache_locs(pid, area, &filter)
        })?,
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
        query(&lua, pid, |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
            let tile = |t: &mlua::Table| -> Result<[i32; 2], String> { Ok([t.get("x").map_err(|e| e.to_string())?, t.get("y").map_err(|e| e.to_string())?]) };
            // a table asks for a randomized route, its fields override the defaults
            let options = options.map(|t| -> Result<crate::scene_graph::Options, String> {
                let default = crate::scene_graph::Options::default();
                let field = |name: &str| t.get::<Option<f64>>(name).map_err(|e| e.to_string());
                Ok(crate::scene_graph::Options {
                    seed: match field("seed")? {
                        Some(seed) => seed as u64,
                        None => crate::scene_graph::mix(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)),
                    },
                    rnd: field("rnd")?.unwrap_or(default.rnd),
                    spacing: field("spacing")?.map_or(default.spacing, |v| v as i32),
                    jitter: field("jitter")?.map_or(default.jitter, |v| v as i32),
                    max_detour: field("max_detour")?.unwrap_or(default.max_detour),
                    offset: field("offset")?.map_or(default.offset, |v| v as i32),
                })
            });
            let route = scene_route(pid, tile(&from)?, tile(&to)?, options.transpose()?)?;
            Ok(SceneRoute { tiles: route.tiles.into_iter().map(|(x, y)| PathTile { x, y }).collect(), reached: route.reached })
        })?,
    )?;
    globals.set(
        "nav_plan",
        query(&lua, pid, |pid, (from, to, options): (mlua::Table, mlua::Table, Option<mlua::Table>)| {
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
                o.door_cost = t.get::<Option<u32>>("door_cost").map_err(err)?.unwrap_or(o.door_cost);
                o.climb_cost = t.get::<Option<u32>>("climb_cost").map_err(err)?.unwrap_or(o.climb_cost);
                o.rnd = t.get::<Option<f64>>("rnd").map_err(err)?.unwrap_or(o.rnd);
                o.seed = t.get::<Option<f64>>("seed").map_err(err)?.map(|s| s as u64);
                if let Some(options) = t.get::<Option<Vec<String>>>("door_options").map_err(err)? {
                    request.door_options = options;
                }
                if let Some(options) = t.get::<Option<std::collections::BTreeMap<String, i32>>>("climb_options").map_err(err)? {
                    request.climb_options = options.into_iter().collect();
                }
                for loc in t.get::<Option<Vec<mlua::Table>>>("avoid").map_err(err)?.unwrap_or_default() {
                    let field = |name: &str| loc.get::<i32>(name).map_err(err);
                    request.avoid.push((field("id")?, field("x")?, field("y")?, field("plane")?));
                }
            }
            nav_plan(pid, point(&from)?, point(&to)?, &request)
        })?,
    )?;
    globals.set(
        "loc_model",
        query(&lua, pid, |pid, loc: mlua::Table| {
            let field = |name: &str| loc.get::<i32>(name).map_err(|e| e.to_string());
            Ok(loc_model(pid, field("kind")? as u8, field("scene_x")?, field("scene_y")?, field("id")?)?.map(script_model))
        })?,
    )?;
    globals.set("players", query(&lua, pid, |pid, (): ()| players(pid))?)?;
    globals.set("player_model", query(&lua, pid, |pid, index: i32| Ok(player_model(pid, index)?.map(script_model)))?)?;
    globals.set("world_select", query(&lua, pid, |pid, (): ()| world_select(pid))?)?;
    globals.set("widget_children", query(&lua, pid, |pid, (group, child): (u16, u16)| widget_children(pid, group, child))?)?;

    fn click_input((x, y, button, hold_ms): (i32, i32, Option<String>, Option<u32>)) -> mlua::Result<Input> {
        Ok(Input::Click { button: button_arg(button)?, x, y, hold_ms: hold_ms.unwrap_or(crate::CLICK_HOLD_MS) })
    }
    fn move_input((x, y): (i32, i32)) -> mlua::Result<Input> {
        Ok(Input::Move { x, y })
    }
    fn wind_mouse_input((x, y, options): (i32, i32, Option<mlua::Table>)) -> mlua::Result<Input> {
        let mut opts = wind_mouse::Options::default();
        if let Some(options) = options {
            opts.speed = options.get::<Option<f64>>("speed")?.unwrap_or(opts.speed);
            opts.wind = options.get::<Option<f64>>("wind")?.unwrap_or(opts.wind);
            opts.gravity = options.get::<Option<f64>>("gravity")?.unwrap_or(opts.gravity);
            opts.timeout_ms = options.get::<Option<u32>>("timeout")?.unwrap_or(opts.timeout_ms);
        }
        Ok(Input::WindMouse { x, y, options: opts })
    }
    fn type_input((text, hold_ms): (String, Option<u32>)) -> mlua::Result<Input> {
        Ok(Input::Type { text, hold_ms: hold_ms.unwrap_or(crate::KEY_HOLD_MS) })
    }
    globals.set("click", input(&lua, pid, &worker, click_input)?)?;
    globals.set("move", input(&lua, pid, &worker, move_input)?)?;
    globals.set("wind_mouse", input(&lua, pid, &worker, wind_mouse_input)?)?;
    globals.set("type_text", input(&lua, pid, &worker, type_input)?)?;
    fn mouse_down_input(button: Option<String>) -> mlua::Result<Input> {
        Ok(Input::Button { button: button_arg(button)?, down: true })
    }
    fn mouse_up_input(button: Option<String>) -> mlua::Result<Input> {
        Ok(Input::Button { button: button_arg(button)?, down: false })
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
    globals.set("wind_mouse_async", input_async(&lua, pid, &worker, wind_mouse_input)?)?;
    globals.set("type_text_async", input_async(&lua, pid, &worker, type_input)?)?;
    // Buttons and keys pressed this way stay down until released, cancelled or the script ends, last pressed released first.
    globals.set("mouse_down_async", input_async(&lua, pid, &worker, mouse_down_input)?)?;
    globals.set("mouse_up_async", input_async(&lua, pid, &worker, mouse_up_input)?)?;
    globals.set("scroll_async", input_async(&lua, pid, &worker, scroll_input)?)?;
    globals.set("key_down_async", input_async(&lua, pid, &worker, key_down_input)?)?;
    globals.set("key_up_async", input_async(&lua, pid, &worker, key_up_input)?)?;
    // A pause in the queue, for holding a key or a button for a while.
    globals.set(
        "sleep_async",
        action(&lua, pid, {
            let worker = worker.clone();
            move |_, ms: u32| Ok(worker.as_ref().map_or(Err(NO_CLIENT.into()), |worker| worker.push(Job::Wait(ms))))
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
        action(&lua, pid, |pid, (x1, y1, x2, y2, r, g, b, a): (i32, i32, i32, i32, u8, u8, u8, Option<u8>)| {
            let color = Rgba { r, g, b, a: a.unwrap_or(255) };
            Ok(draw(pid, &Overlay::Rect { x1, y1, x2, y2, color }))
        })?,
    )?;
    globals.set(
        "draw_line",
        action(&lua, pid, |pid, (x1, y1, x2, y2, r, g, b, a): (i32, i32, i32, i32, u8, u8, u8, Option<u8>)| {
            let color = Rgba { r, g, b, a: a.unwrap_or(255) };
            Ok(draw(pid, &Overlay::Line { x1, y1, x2, y2, color }))
        })?,
    )?;
    globals.set(
        "draw_lines",
        action(&lua, pid, |pid, (lines, r, g, b, a): (Vec<mlua::Table>, u8, u8, u8, Option<u8>)| {
            let lines = lines
                .iter()
                .map(|l| Ok([1, 2, 3, 4].map(|i| l.get::<f64>(i).map(|v| v.round() as i32)).into_iter().collect::<mlua::Result<Vec<_>>>()?.try_into().unwrap()))
                .collect::<mlua::Result<Vec<[i32; 4]>>>()?;
            Ok(draw_lines(pid, &lines, Rgba { r, g, b, a: a.unwrap_or(255) }))
        })?,
    )?;
    globals.set("clear_overlay", action(&lua, pid, |pid, (): ()| Ok(draw(pid, &Overlay::Clear)))?)?;
    globals.set(
        "draw_batch",
        action(&lua, pid, |pid, shapes: Vec<mlua::Table>| {
            let overlays = shapes.iter().map(overlay_arg).collect::<mlua::Result<Vec<_>>>()?;
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
        let result = run("test.luau", source, None, move |line| collected.borrow_mut().push(line));
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn collision_cut_clips_the_area_to_the_scene() {
        // 3 columns of 2, the flag being x * 10 + y
        let scene = (100, 200, 0, (0..3).map(|x| (0..2).map(|y| x * 10 + y).collect()).collect());
        let cut = collision_cut(scene.clone(), Some([101, 150, 105, 201])).unwrap();
        assert_eq!((cut.x, cut.y, cut.width, cut.height), (101, 200, 2, 2));
        assert_eq!(cut.flags, vec![10, 11, 20, 21]);
        assert_eq!(collision_cut(scene.clone(), None).unwrap().flags.len(), 6);
        assert!(collision_cut(scene, Some([90, 190, 95, 195])).is_err());
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
        let plan = crate::form::parse_breaks(lines[0].strip_prefix(crate::form::BREAKS_PREFIX).unwrap()).unwrap();
        assert_eq!(plan.breaks[0].remaining, 1500.5);
        assert!(plan.sleeps.is_empty());
        assert_eq!(plan.active.unwrap().kind, "break");
        assert!(run_lines("report_breaks({ breaks = 5 })").1.is_err());
    }

    #[test]
    fn report_counters_writes_a_counters_line() {
        let (lines, result) = run_lines(
            "report_counters({ { id = 'wins', label = 'Wins', value = 3, percent_of = 'games' }, { id = 'games', label = 'Games', value = 4, hourly = false } })",
        );
        result.unwrap();
        let counters = crate::form::parse_counters(lines[0].strip_prefix(crate::form::COUNTERS_PREFIX).unwrap()).unwrap();
        assert_eq!(counters[0].percent_of.as_deref(), Some("games"));
        assert!(counters[0].hourly && !counters[1].hourly);
        assert!(run_lines("report_counters({ { label = 'No id', value = 1 } })").1.is_err());
        assert!(run_lines("report_counters({ { id = 'a', label = 'A', value = 0/0 } })").1.is_err());
    }

    // A scratch directory with `files` (path, contents) written under it.
    fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
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
        let result = run_with_form(&name, source, None, move |line| collected.borrow_mut().push(line), None, Some(path), None);
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn modules_given_in_memory_are_required_without_files() {
        let dir = scratch("requires_memory", &[("lib/host.luau", "return { name = 'host' }")]);
        let script = dir.join("scripts/run.luau");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        let modules = HashMap::from([
            ("util.luau".to_owned(), "local Other = require('./deeper')\nreturn { name = 'util', other = Other.name }".to_owned()),
            ("deeper.luau".to_owned(), "return { name = 'deeper' }".to_owned()),
        ]);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let source = "local Util = require('./util')\nprint(Util.name, Util.other, require('../lib/host').name, require('./util') == Util)";
        let result = run_with_modules("run.luau", source, None, move |line| collected.borrow_mut().push(line), None, Some(&script), None, modules);
        result.unwrap();
        assert_eq!(*lines.borrow(), vec!["util\tdeeper\thost\ttrue".to_string()]);
        assert!(!dir.join("scripts/util.luau").exists(), "nothing is written out");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn requires_resolve_from_the_requiring_file() {
        let dir = scratch(
            "requires_resolve",
            &[
                ("lib/host.luau", "return { name = 'host' }"),
                ("lib/main/walker.luau", "local Host = require('../host')\nreturn { host = Host, lazy = function() return require('./extra') end }"),
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

    fn run_script_with_lib(path: &Path, source: &str, lib: &Path) -> (Vec<String>, mlua::Result<()>) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = lines.clone();
        let name = path.file_name().unwrap().to_string_lossy();
        let result = run_with_form(&name, source, None, move |line| collected.borrow_mut().push(line), None, Some(path), Some(lib));
        (Rc::try_unwrap(lines).unwrap().into_inner(), result)
    }

    #[test]
    fn alias_lib_points_at_the_lib_beside_the_script() {
        assert_eq!(alias_lib("@lib/main/walker".into()), "../lib/main/walker");
        assert_eq!(alias_lib("../lib/host".into()), "../lib/host");
        assert_eq!(alias_lib("@library/host".into()), "@library/host");
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
                ("mine/main/walker.luau", "local Host = require('../host')\nreturn { host = Host }"),
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
        let dir = scratch("requires_lib_dir_missing", &[("mine/host.luau", "return {}"), ("lib/nope.luau", "return {}")]);
        let (_, result) = run_script_with_lib(&dir.join("scripts/run.luau"), "require('../lib/nope')", &dir.join("mine"));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("module '../lib/nope' not found, tried:") && error.contains("mine"), "{error}");
    }

    #[test]
    fn missing_relative_requires_name_what_was_tried() {
        let dir = scratch("requires_missing", &[]);
        let (_, result) = run_script_at(&dir.join("run.luau"), "require('../lib/nope')");
        let error = result.unwrap_err().to_string();
        assert!(error.contains("module '../lib/nope' not found, tried:"), "{error}");
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
            ["Try again\t625\t291\t775\t331\t700\t311", "nil\tplay now", "591\t209\t809\t277", "138\t206", "nil"]
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
        assert!(lines[1].starts_with("false") && lines[1].contains("no client selected"), "{lines:?}");
        assert_eq!(lines[2], "nil\tno client selected");
        assert_eq!(lines.len(), 3);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn every_lib_module_loads() {
        let source: String = LIB_MODULES.iter().map(|(name, _, _)| format!("assert(type(require('{name}')) == 'table', '{name}')\n")).collect();
        let (_, result) = run_lines(&source);
        assert!(result.is_ok(), "{result:?}");
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
            assert!(line.starts_with("false") && line.contains("no client selected"), "{lines:?}");
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
        assert!(lines[0].starts_with("[SidePanels][") && lines[0].ends_with("]: unknown side panel: bank"), "{lines:?}");
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
        for line in lines.iter().enumerate().filter(|&(i, _)| i != 1).map(|(_, line)| line) {
            assert!(line.starts_with("false") && line.contains("no client selected"), "{lines:?}");
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
        assert!(result.is_ok(), "{result:?}");
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
        let (lines, result) = run_lines("print(io, os, package, ffi, debug, dofile, loadfile, type(bit32), type(jit))");
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
    fn game_state_connection_and_click_need_a_client() {
        let (lines, result) = run_lines(
            "local ok_state, err = pcall(game_state)\n\
             print(ok_state, err)\n\
             local ok_conn, err2 = pcall(connection)\n\
             print(ok_conn, err2)\n\
             print(varp(173))\n\
             print(varps())\n\
             local ok, err3 = pcall(click, 1, 2)\n\
             print(ok)\n\
             print(err3)",
        );
        let none = "nil\tno client selected";
        for line in &lines[..2] {
            assert!(line.starts_with("false") && line.contains("no client selected"), "{lines:?}");
        }
        assert_eq!(lines[2..5], [none, none, "false"]);
        assert!(lines[5].contains("no client selected"), "{lines:?}");
        assert!(result.is_ok());
    }

    #[test]
    fn draw_rect_and_clear_overlay_need_a_client() {
        let (lines, result) = run_lines(
            "local ok, err = pcall(draw_rect, 100, 100, 200, 200, 255, 0, 0)\n\
             print(ok, err)\n\
             local ok2, err2 = pcall(clear_overlay)\n\
             print(ok2, err2)",
        );
        assert!(lines[0].starts_with("false"), "{lines:?}");
        assert!(lines[0].contains("no client selected"), "{lines:?}");
        assert!(lines[1].starts_with("false"), "{lines:?}");
        assert!(lines[1].contains("no client selected"), "{lines:?}");
        assert!(result.is_ok());
    }

    #[test]
    fn draw_batch_needs_a_client() {
        let (lines, result) = run_lines(
            "local ok, err = pcall(draw_batch, { { kind = 'clear' }, { kind = 'rect', x1 = 1, y1 = 1, x2 = 9, y2 = 9, r = 255, g = 0, b = 0 } })\n\
             print(ok, err)",
        );
        assert!(lines[0].starts_with("false") && lines[0].contains("no client selected"), "{lines:?}");
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
        assert!(lines[0].starts_with("false") && lines[0].contains("no client selected"), "{lines:?}");
        assert!(result.is_ok());
    }
}
