use std::time::Duration;

use serde::{Deserialize, Serialize};
use ipc::{Header, Incompatible};
pub use ipc::PROTOCOL_VERSION;

/// What the sandbox offers the Luau lib: bumped whenever a global is added or changed in a way a newer lib depends on.
/// A published lib states the `HOST_API` it needs, and a loader only installs libs it satisfies.
pub const HOST_API: u32 = 3;
pub use ipc::state::varp;

pub mod async_input;
pub mod input_jobs;
pub mod cache;
pub mod collision;
pub mod devtools;
pub mod config;
pub mod pathfinding;
pub mod font;
pub mod form;
pub mod projection;
pub mod sandbox;
pub mod nav;
pub mod scene_collision;
pub mod scene_graph;
pub mod wind_mouse;
pub use decode::{
    worker,
    Camera, Chunk, GameState, Login, Minimap, Position, Skill, Tile, chat, game_state_name, memory, state, strip_tags, widgets, world,
    world_select,
};
pub use decode::state::MAX_VARPS;
pub use decode::widgets::{SubInterface, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The open right-click menu as the client lays it out, in interface units, with its scroll, mouse margin and open `submenus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuLayout {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub rows: MenuRect,
    pub header_height: i32,
    pub row_height: i32,
    pub scroll: i32,
    pub margin: i32,
    pub submenus: Vec<MenuSubmenu>,
}

/// An open submenu: `row` is the row of the menu above that opened it (1 for the top), `entries` its rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuSubmenu {
    pub row: u32,
    pub entries: Vec<MenuEntry>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub rows: MenuRect,
    pub header_height: i32,
    pub row_height: i32,
    pub scroll: i32,
    pub margin: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuClick {
    pub entry: MenuEntry,
    pub x: i32,
    pub y: i32,
    pub age: i32,
}

/// The component waiting for a target (Java's `widgetSelected`): a selected spell or any op that asks for one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedWidget {
    pub verb: String,
    pub name: String,
    pub raw_name: String,
    /// The packed component id, `group << 16 | child`.
    pub id: i32,
    pub group: i32,
    pub child: i32,
    /// The dynamic child, -1 when the component itself was selected.
    pub index: i32,
    /// The item the component shows, -1 for spells.
    pub item: i32,
    /// What it can be used on: 1 ground items, 2 NPCs, 4 locs, 8 players, 16 self, 32 components.
    pub targets: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuEntry {
    pub option: String,
    pub target: String,
    pub raw_target: String,
    pub id: i32,
    pub index: i32,
    pub action: i32,
    pub priority: i32,
    pub kind: u8,
    /// Whether the row opens a submenu. Only known for the rows of an open menu.
    pub submenu: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityModel {
    pub bounding_box: bool,
    pub orientation: i32,
    pub vertex_count: u32,
    pub corners: Vec<[f32; 2]>,
    pub hull: Vec<[f32; 2]>,
    pub face_count: u32,
    pub faces: Vec<[[f32; 2]; 3]>,
}

/// A loc placed on the map in the game cache, whether or not its map square is loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheLoc {
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    /// The tiles it covers, turned with its rotation.
    pub size_x: u8,
    pub size_y: u8,
    pub shape: u8,
    pub rotation: u8,
    /// The right-click options it has, in menu order.
    pub actions: Vec<String>,
}

/// What `cache_locs` keeps; every field that is `None` keeps everything.
#[derive(Debug, Clone, Default)]
pub struct CacheLocFilter {
    pub plane: Option<i32>,
    /// Names to match (case-insensitive, part of the name).
    pub names: Option<Vec<String>>,
    /// Options to match (case-insensitive, the whole option): a loc with any of them is kept.
    pub actions: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loc {
    pub id: i32,
    pub name: Option<String>,
    pub kind: u8,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub scene_x: i32,
    pub scene_y: i32,
    pub size_x: u8,
    pub size_y: u8,
    pub shape: Option<u8>,
    pub rotation: Option<u8>,
    pub clickable: bool,
}

pub use decode::chat::ChatMessage;
pub use decode::world::{GroundItem, Item, Npc, Player};
pub use decode::world_select::{World, WorldSelect};

#[cfg(windows)]
mod input;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use input::{open_console, send as send_input, send_with as send_input_with};
#[cfg(windows)]
pub use win::{
    cache_archive_files, cache_archives, cache_files, cache_locs, chat_messages, collision_area, connection, enum_def, item_params, varbit, varbit_def, varbits_of, find_path, scene_collision, scene_route, player_plane, map_regions, in_instance, world_movement, draw, set_cursor, draw_batch, draw_lines, game_state, item_container, item_names, game_val_names, ground_items, list_clients, menu, menu_click, menu_layout, menu_open, selected_widget, scene_tiles, scene_tiles_next_frame, project, project_moved, sub_interfaces, load, loc_model, locs, clickable_locs, nav_plan, npc_model, npc_words, npcs, player_model, players, screenshot, set_display_name, set_total_xp,
    unload, varc, varp_value, varps, widget, widget_children, world_select,
};

pub const MODULE_PREFIX: &str = "agent";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const UNLOAD_TIMEOUT: Duration = Duration::from_secs(15);

pub fn dll_version(dll: &[u8]) -> String {
    String::from_utf8_lossy(&ipc::build_id(dll)).into_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Client {
    pub pid: u32,
    pub exe: String,
    pub title: String,
    pub is_64bit: bool,
    pub loaded: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub version: String,
    pub protocol: u8,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub mouse_x: i32,
    #[serde(default)]
    pub mouse_y: i32,
    pub offsets_found: Option<bool>,
}

impl Connection {
    pub fn from_header(header: Result<Header, Incompatible>) -> Self {
        match header {
            Ok(header) => {
                let shift = |pos: i32, offset: i32| if pos > -1 { pos - offset } else { pos };
                Self {
                    version: header.version(),
                    protocol: header.protocol,
                    width: header.width,
                    height: header.height,
                    mouse_x: shift(header.mouse_x, header.offset_x),
                    mouse_y: shift(header.mouse_y, header.offset_y_top_down()),
                    offsets_found: header.offsets_found(),
                }
            }
            Err(Incompatible { version, protocol }) => {
                Self { version, protocol, width: -1, height: -1, mouse_x: -1, mouse_y: -1, offsets_found: None }
            }
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.protocol == ipc::PROTOCOL_VERSION
    }

    /// Why a script can't use this agent, if it can't.
    pub fn problem(&self) -> Option<String> {
        (!self.is_compatible()).then(|| {
            format!("the agent runs protocol {}, this is {}: restart the client and load a matching agent", self.protocol, ipc::PROTOCOL_VERSION)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadReport {
    pub already_loaded: bool,
    #[serde(default)]
    pub replaced: Option<String>,
    pub connection: Option<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
        }
    }

    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "middle" => Ok(Self::Middle),
            other => Err(format!("Invalid button: {other} (left, right or middle)")),
        }
    }
}

pub const KEY_HOLD_MS: u32 = 60;
pub const CLICK_HOLD_MS: u32 = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Input {
    Move { x: i32, y: i32 },
    WindMouse { x: i32, y: i32, options: wind_mouse::Options },
    Click { button: MouseButton, x: i32, y: i32, hold_ms: u32 },
    Button { button: MouseButton, down: bool },
    Scroll { notches: i32 },
    Key { key: u8, down: bool },
    Type { text: String, hold_ms: u32 },
}

impl Input {
    pub fn args(&self, pid: u32) -> Vec<String> {
        let pid = pid.to_string();
        let state = |down: &bool| if *down { "down" } else { "up" }.to_string();
        match self {
            Self::Move { x, y } => vec!["move".into(), pid, x.to_string(), y.to_string()],
            Self::WindMouse { x, y, options } => vec![
                "wind_mouse".into(),
                pid,
                x.to_string(),
                y.to_string(),
                options.speed.to_string(),
                options.wind.to_string(),
                options.gravity.to_string(),
                options.timeout_ms.to_string(),
            ],
            Self::Click { button, x, y, hold_ms } => {
                vec!["click".into(), pid, button.name().into(), x.to_string(), y.to_string(), hold_ms.to_string()]
            }
            Self::Button { button, down } => vec!["button".into(), pid, button.name().into(), state(down)],
            Self::Scroll { notches } => vec!["scroll".into(), pid, notches.to_string()],
            Self::Key { key, down } => vec!["key".into(), pid, key.to_string(), state(down)],
            Self::Type { text, hold_ms } => vec!["type".into(), pid, text.clone(), hold_ms.to_string()],
        }
    }

    pub fn from_args(args: &[&str]) -> Option<Result<(u32, Self), String>> {
        let number = |s: &str| s.parse::<i32>().map_err(|_| format!("Invalid number: {s}"));
        let real = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("Invalid number: {s}"));
        let down = |s: &str| match s {
            "down" => Ok(true),
            "up" => Ok(false),
            other => Err(format!("Invalid button/key state: {other} (down or up)")),
        };
        let parse = |pid: &str, input: Result<Self, String>| -> Result<(u32, Self), String> {
            Ok((pid.parse().map_err(|_| format!("Invalid pid: {pid}"))?, input?))
        };
        Some(match args {
            ["move", pid, x, y] => parse(pid, (|| Ok(Self::Move { x: number(x)?, y: number(y)? }))()),
            ["wind_mouse", pid, x, y, speed, wind, gravity, timeout_ms] => parse(
                pid,
                (|| {
                    let options = wind_mouse::Options {
                        speed: real(speed)?,
                        wind: real(wind)?,
                        gravity: real(gravity)?,
                        timeout_ms: timeout_ms.parse().map_err(|_| format!("Invalid timeout: {timeout_ms}"))?,
                    };
                    Ok(Self::WindMouse { x: number(x)?, y: number(y)?, options })
                })(),
            ),
            ["click", pid, button, x, y] => parse(
                pid,
                (|| Ok(Self::Click { button: MouseButton::parse(button)?, x: number(x)?, y: number(y)?, hold_ms: CLICK_HOLD_MS }))(),
            ),
            ["click", pid, button, x, y, hold_ms] => parse(
                pid,
                (|| {
                    let hold_ms = hold_ms.parse().map_err(|_| format!("Invalid hold time: {hold_ms}"))?;
                    Ok(Self::Click { button: MouseButton::parse(button)?, x: number(x)?, y: number(y)?, hold_ms })
                })(),
            ),
            ["button", pid, button, state] => {
                parse(pid, (|| Ok(Self::Button { button: MouseButton::parse(button)?, down: down(state)? }))())
            }
            ["scroll", pid, notches] => parse(pid, (|| Ok(Self::Scroll { notches: number(notches)? }))()),
            ["key", pid, key, state] => parse(
                pid,
                (|| {
                    let key = key.parse().map_err(|_| format!("Invalid key code: {key} (0-255)"))?;
                    Ok(Self::Key { key, down: down(state)? })
                })(),
            ),
            ["type", pid, text] => parse(pid, Ok(Self::Type { text: text.to_string(), hold_ms: KEY_HOLD_MS })),
            ["type", pid, text, hold_ms] => parse(
                pid,
                (|| {
                    let hold_ms = hold_ms.parse().map_err(|_| format!("Invalid hold time: {hold_ms}"))?;
                    Ok(Self::Type { text: text.to_string(), hold_ms })
                })(),
            ),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Overlay {
    Rect { x1: i32, y1: i32, x2: i32, y2: i32, color: Rgba },
    Line { x1: i32, y1: i32, x2: i32, y2: i32, color: Rgba },
    Clear,
}

impl Overlay {
    pub fn args(&self, pid: u32) -> Vec<String> {
        let pid = pid.to_string();
        match self {
            Self::Rect { x1, y1, x2, y2, color: Rgba { r, g, b, a } } | Self::Line { x1, y1, x2, y2, color: Rgba { r, g, b, a } } => vec![
                "overlay".into(),
                pid,
                if matches!(self, Self::Line { .. }) { "line" } else { "rect" }.into(),
                x1.to_string(),
                y1.to_string(),
                x2.to_string(),
                y2.to_string(),
                r.to_string(),
                g.to_string(),
                b.to_string(),
                a.to_string(),
            ],
            Self::Clear => vec!["overlay".into(), pid, "clear".into()],
        }
    }

    pub fn from_args(args: &[&str]) -> Option<Result<(u32, Self), String>> {
        let number = |s: &str| s.parse::<i32>().map_err(|_| format!("Invalid number: {s}"));
        let byte = |s: &str| s.parse::<u8>().map_err(|_| format!("Invalid color component: {s}"));
        let parse = |pid: &str, overlay: Result<Self, String>| -> Result<(u32, Self), String> {
            Ok((pid.parse().map_err(|_| format!("Invalid pid: {pid}"))?, overlay?))
        };
        Some(match args {
            ["overlay", pid, "rect", x1, y1, x2, y2, r, g, b, a] => parse(
                pid,
                (|| {
                    Ok(Self::Rect {
                        x1: number(x1)?,
                        y1: number(y1)?,
                        x2: number(x2)?,
                        y2: number(y2)?,
                        color: Rgba { r: byte(r)?, g: byte(g)?, b: byte(b)?, a: byte(a)? },
                    })
                })(),
            ),
            ["overlay", pid, "line", x1, y1, x2, y2, r, g, b, a] => parse(
                pid,
                (|| {
                    Ok(Self::Line {
                        x1: number(x1)?,
                        y1: number(y1)?,
                        x2: number(x2)?,
                        y2: number(y2)?,
                        color: Rgba { r: byte(r)?, g: byte(g)?, b: byte(b)?, a: byte(a)? },
                    })
                })(),
            ),
            ["overlay", pid, "clear"] => parse(pid, Ok(Self::Clear)),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyle {
    Off,
    Cross,
    Dot,
    X,
}

impl CursorStyle {
    pub fn value(self) -> u8 {
        match self {
            Self::Off => ipc::cursor_style::OFF,
            Self::Cross => ipc::cursor_style::CROSS,
            Self::Dot => ipc::cursor_style::DOT,
            Self::X => ipc::cursor_style::X,
        }
    }
}

/// What the agent draws at the mouse position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub style: CursorStyle,
    pub color: [u8; 3],
    pub size: u8,
}

impl Cursor {
    pub fn args(&self, pid: u32) -> Vec<String> {
        let style = match self.style {
            CursorStyle::Off => "off",
            CursorStyle::Cross => "cross",
            CursorStyle::Dot => "dot",
            CursorStyle::X => "x",
        };
        let [r, g, b] = self.color;
        vec!["cursor".into(), pid.to_string(), style.into(), r.to_string(), g.to_string(), b.to_string(), self.size.to_string()]
    }

    pub fn from_args(args: &[&str]) -> Option<Result<(u32, Self), String>> {
        let ["cursor", pid, style, r, g, b, size] = args else { return None };
        let byte = |s: &str| s.parse::<u8>().map_err(|_| format!("Invalid color component: {s}"));
        Some((|| {
            let style = match *style {
                "off" => CursorStyle::Off,
                "cross" => CursorStyle::Cross,
                "dot" => CursorStyle::Dot,
                "x" => CursorStyle::X,
                other => return Err(format!("Invalid cursor style: {other} (off, cross, dot or x)")),
            };
            let pid = pid.parse().map_err(|_| format!("Invalid pid: {pid}"))?;
            Ok((pid, Self { style, color: [byte(r)?, byte(g)?, byte(b)?], size: byte(size)? }))
        })())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayName {
    Kept { real: Option<String> },
    Waiting,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TotalXp {
    Fake { offset: i64, real: Option<i64> },
    Off,
}

pub fn random_total_xp_offset(real: i64, seed: u64) -> i64 {
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let digits = |n: i64| n.max(0).to_string().len();
    let mut offset = 0;
    for _ in 0..64 {
        let fraction = 0.05 + (next() % 10_000) as f64 / 10_000.0 * 0.25;
        let size = ((real as f64 * fraction) as i64).max(1_000);
        offset = if next() % 2 == 0 { size } else { -size };
        if real + offset <= 0 {
            offset = size;
        }
        if digits(real + offset) == digits(real) {
            break;
        }
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_round_trips_through_args() {
        for input in [
            Input::Move { x: 10, y: 20 },
            Input::WindMouse {
                x: -3,
                y: 400,
                options: wind_mouse::Options { speed: 12.5, wind: 0.1, gravity: 9.0, timeout_ms: 2500 },
            },
            Input::Click { button: MouseButton::Right, x: 0, y: 5, hold_ms: 90 },
            Input::Button { button: MouseButton::Middle, down: true },
            Input::Button { button: MouseButton::Left, down: false },
            Input::Scroll { notches: -3 },
            Input::Key { key: 37, down: true },
            Input::Key { key: 160, down: false },
            Input::Type { text: "Hello \"world\"".into(), hold_ms: 45 },
        ] {
            let args = input.args(32);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            assert_eq!(Input::from_args(&args), Some(Ok((32, input))));
        }
        assert_eq!(Input::from_args(&["list"]), None);
        assert_eq!(Input::from_args(&["type", "1", "hi"]), Some(Ok((1, Input::Type { text: "hi".into(), hold_ms: KEY_HOLD_MS }))));
        assert!(matches!(Input::from_args(&["key", "1", "256", "down"]), Some(Err(_))));
        assert!(matches!(Input::from_args(&["button", "1", "left", "held"]), Some(Err(_))));
    }

    #[test]
    fn chunk_of_a_position() {
        let position = Position { local_x: 0, local_y: 0, x: 3213, y: 3428, plane: Some(0) };
        assert_eq!(position.chunk(), Chunk { x: 50, y: 53 });
        assert_eq!(position.chunk().id(), 12853);
    }

    #[test]
    fn random_total_xp_offsets_stay_in_range() {
        for real in [0i64, 54_321, 6_019_725, 509_552_478] {
            for seed in 1..500 {
                let offset = random_total_xp_offset(real, seed);
                assert!(real + offset > 0, "{real} {offset}");
                let size = offset.abs();
                assert!(size >= 1_000, "{real} {offset}");
                assert!(size as f64 <= (real as f64 * 0.3).max(1_000.0) + 1.0, "{real} {offset}");
            }
        }
        assert_eq!((6_019_725 + random_total_xp_offset(6_019_725, 7)).to_string().len(), 7);
    }
}
