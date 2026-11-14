pub use decode::{
    GameState, Position, ServerTile,
    chat::ChatMessage,
    game_state_name, memory, state,
    state::MAX_VARPS,
    strip_tags, widgets,
    widgets::{SubInterface, TreeWidget, Widget, WidgetDetail, WidgetTree},
    worker, world,
    world::{GroundItem, Item, Npc, Player, Projectile, SpotAnim},
    world_select,
    world_select::WorldSelect,
};
use ipc::{Header, Incompatible};
pub use ipc::{PROTOCOL_VERSION, state::varp};
use serde::{Deserialize, Serialize};

pub const HOST_API: u32 = 20;

pub mod antiban;
pub mod async_input;
pub mod bank_pin;
mod bank_pin_overlay;
pub mod biometrics;
pub mod cache;
pub mod collision;
pub mod config;
pub mod devtools;
pub mod entry_swaps;
pub use entry_swaps::{EntrySwaps, Swap};
pub mod font;
pub mod form;
pub mod http;
pub mod input_jobs;
pub mod lamp;
pub mod manifest;
pub mod landmass;
pub mod nav;
pub mod pathfinding;
pub mod prices;
pub mod projection;
pub mod sandbox;
pub mod scene_collision;
pub mod scene_graph;
pub mod transports;
pub mod us_keys;
pub mod wind_mouse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedWidget {
    pub verb: String,
    pub name: String,
    pub raw_name: String,
    pub id: i32,
    pub group: i32,
    pub child: i32,
    pub index: i32,
    pub item: i32,
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
    pub frame_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheLoc {
    pub id: i32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub size_x: u8,
    pub size_y: u8,
    pub shape: u8,
    pub rotation: u8,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocView {
    pub name: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CacheLocFilter {
    pub plane: Option<i32>,
    pub names: Option<Vec<String>>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    pub sequence: u64,
    pub time_ms: u64,
    pub sent: bool,
    pub opcode: u16,
    pub length: u32,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketLayout {
    pub opcode: u16,
    pub size: i32,
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Movement {
    pub x: i32,
    pub y: i32,
    pub plane: Option<i32>,
    pub from_x: i32,
    pub from_y: i32,
    pub from_plane: Option<i32>,
    pub moving: bool,
    pub moved_tick: Option<i32>,
    pub tick: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketPage {
    pub packets: Vec<Packet>,
    pub next: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptRun {
    pub sequence: u64,
    pub time_us: u64,
    pub tick: Option<i32>,
    pub end: bool,
    pub event_type: i32,
    pub arg_count: u16,
    pub depth: u8,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPage {
    pub runs: Vec<ScriptRun>,
    pub next: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarcChange {
    pub sequence: u64,
    pub time_ms: u64,
    pub index: i32,
    pub string: bool,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarcPage {
    pub changes: Vec<VarcChange>,
    pub next: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemChange {
    pub sequence: u64,
    pub time_ms: u64,
    pub tick: Option<i32>,
    pub container: i32,
    pub slot: i32,
    pub old_known: bool,
    pub old_item: i32,
    pub old_quantity: i32,
    pub new_item: i32,
    pub new_quantity: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemPage {
    pub changes: Vec<ItemChange>,
    pub next: u64,
    pub dropped: u64,
}

pub const ITEM_CHANGES_NOW: u64 = u64::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemIconRequest {
    pub id: i32,
    pub quantity: i32,
    pub show_count: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: String,
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = Vec::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        hex.push(DIGITS[usize::from(byte >> 4)]);
        hex.push(DIGITS[usize::from(byte & 0xf)]);
    }
    String::from_utf8(hex).expect("hex digits are ASCII")
}

impl Image {
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        Self {
            width,
            height,
            rgba: hex_lower(rgba),
        }
    }

    pub fn pixels(&self) -> Option<Vec<u8>> {
        let bytes = self.rgba.as_bytes();
        if bytes.len() != self.width as usize * self.height as usize * 8 {
            return None;
        }
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
            .collect()
    }
}

#[cfg(windows)]
mod input;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use input::{open_console, send as send_input, send_with as send_input_with};
#[cfg(windows)]
pub(crate) use win::paint_overlay;
#[cfg(windows)]
pub use win::{
    ItemWatcher, PacketWatcher, cache_archive_files, cache_archives, cache_files, cache_locs,
    chat_messages, client_cycle, collision_area, connection, draw, draw_batch, draw_lines,
    entry_swaps, enum_def, find_path, game_state, game_val_names, ground_items, in_instance,
    item_changes, item_changes_wait, item_container, item_containers, item_families, item_icons,
    item_names, item_params, item_stackable, landmasses, list_clients, load, loc_model, loc_shown,
    locs, map_regions, memory_words, menu, menu_click, menu_layout, menu_open, movement, nav_plan,
    npc_hitsplats, npc_model, npc_sizes, npc_type_model, npc_words, npcs, packet_layouts, packets,
    player_hitsplats, player_model, player_plane, player_words, players, project, project_moved,
    projectiles, scene_collision, scene_route, scene_tiles, scene_tiles_next_frame, screenshot,
    script_runs, selected_widget, server_tile, set_cursor, set_display_name, set_total_xp,
    spot_anims, sprites, sub_interfaces, unload, varbit, varbits_of, varc, varc_changes,
    varp_value, varps, wait_watchers, watch_client, widget, widget_children, widget_dump,
    widget_model, widget_tree, widget_words, world_debug, world_entities, world_route,
    world_select,
};

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
    #[serde(default)]
    pub foreign_bot: Option<String>,
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
            Err(Incompatible { version, protocol }) => Self {
                version,
                protocol,
                width: -1,
                height: -1,
                mouse_x: -1,
                mouse_y: -1,
                offsets_found: None,
            },
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.protocol == ipc::PROTOCOL_VERSION
    }

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

fn parse_pid(pid: &str) -> Result<u32, String> {
    pid.parse().map_err(|_| format!("Invalid pid: {pid}"))
}

fn number(s: &str) -> Result<i32, String> {
    s.parse().map_err(|_| format!("Invalid number: {s}"))
}

fn real(s: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Invalid number: {s}"))
}

fn byte(s: &str) -> Result<u8, String> {
    s.parse()
        .map_err(|_| format!("Invalid color component: {s}"))
}

fn hold(s: &str) -> Result<u32, String> {
    s.parse().map_err(|_| format!("Invalid hold time: {s}"))
}

fn down(s: &str) -> Result<bool, String> {
    match s {
        "down" => Ok(true),
        "up" => Ok(false),
        other => Err(format!("Invalid button/key state: {other} (down or up)")),
    }
}

pub const KEY_HOLD_MS: u32 = 60;
pub const CLICK_HOLD_MS: u32 = 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Input {
    Move {
        x: i32,
        y: i32,
    },
    WindMouse {
        x: i32,
        y: i32,
        options: wind_mouse::Options,
    },
    Click {
        button: MouseButton,
        x: i32,
        y: i32,
        hold_ms: u32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Scroll {
        notches: i32,
    },
    Key {
        key: u8,
        down: bool,
    },
    Type {
        text: String,
        hold_ms: u32,
    },
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
                options.pace.to_string(),
            ],
            Self::Click {
                button,
                x,
                y,
                hold_ms,
            } => {
                vec![
                    "click".into(),
                    pid,
                    button.name().into(),
                    x.to_string(),
                    y.to_string(),
                    hold_ms.to_string(),
                ]
            }
            Self::Button { button, down } => {
                vec!["button".into(), pid, button.name().into(), state(down)]
            }
            Self::Scroll { notches } => vec!["scroll".into(), pid, notches.to_string()],
            Self::Key { key, down } => vec!["key".into(), pid, key.to_string(), state(down)],
            Self::Type { text, hold_ms } => {
                vec!["type".into(), pid, text.clone(), hold_ms.to_string()]
            }
        }
    }

    pub fn from_args(args: &[&str]) -> Option<Result<(u32, Self), String>> {
        let [command, pid, rest @ ..] = args else {
            return None;
        };
        let input = Self::parse(command, rest).transpose()?;
        Some(parse_pid(pid).and_then(|pid| Ok((pid, input?))))
    }

    fn parse(command: &str, args: &[&str]) -> Result<Option<Self>, String> {
        Ok(Some(match (command, args) {
            ("move", [x, y]) => Self::Move {
                x: number(x)?,
                y: number(y)?,
            },
            ("wind_mouse", [x, y, speed, wind, gravity, timeout_ms, pace]) => {
                let options = wind_mouse::Options {
                    speed: real(speed)?,
                    wind: real(wind)?,
                    gravity: real(gravity)?,
                    timeout_ms: timeout_ms
                        .parse()
                        .map_err(|_| format!("Invalid timeout: {timeout_ms}"))?,
                    pace: real(pace)?,
                };
                Self::WindMouse {
                    x: number(x)?,
                    y: number(y)?,
                    options,
                }
            }
            ("click", [button, x, y]) => Self::Click {
                button: MouseButton::parse(button)?,
                x: number(x)?,
                y: number(y)?,
                hold_ms: CLICK_HOLD_MS,
            },
            ("click", [button, x, y, hold_ms]) => {
                let hold_ms = hold(hold_ms)?;
                Self::Click {
                    button: MouseButton::parse(button)?,
                    x: number(x)?,
                    y: number(y)?,
                    hold_ms,
                }
            }
            ("button", [button, state]) => Self::Button {
                button: MouseButton::parse(button)?,
                down: down(state)?,
            },
            ("scroll", [notches]) => Self::Scroll {
                notches: number(notches)?,
            },
            ("key", [key, state]) => Self::Key {
                key: key
                    .parse()
                    .map_err(|_| format!("Invalid key code: {key} (0-255)"))?,
                down: down(state)?,
            },
            ("type", [text]) => Self::Type {
                text: text.to_string(),
                hold_ms: KEY_HOLD_MS,
            },
            ("type", [text, hold_ms]) => Self::Type {
                text: text.to_string(),
                hold_ms: hold(hold_ms)?,
            },
            _ => return Ok(None),
        }))
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
    Rect {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: Rgba,
    },
    Line {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: Rgba,
    },
    Clear,
}

impl Overlay {
    pub fn args(&self, pid: u32) -> Vec<String> {
        let pid = pid.to_string();
        match self {
            Self::Rect {
                x1,
                y1,
                x2,
                y2,
                color: Rgba { r, g, b, a },
            }
            | Self::Line {
                x1,
                y1,
                x2,
                y2,
                color: Rgba { r, g, b, a },
            } => vec![
                "overlay".into(),
                pid,
                if matches!(self, Self::Line { .. }) {
                    "line"
                } else {
                    "rect"
                }
                .into(),
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
        let ["overlay", pid, rest @ ..] = args else {
            return None;
        };
        let overlay = Self::parse(rest).transpose()?;
        Some(parse_pid(pid).and_then(|pid| Ok((pid, overlay?))))
    }

    fn parse(args: &[&str]) -> Result<Option<Self>, String> {
        Ok(Some(match args {
            [kind @ ("rect" | "line"), x1, y1, x2, y2, r, g, b, a] => {
                let (x1, y1, x2, y2) = (number(x1)?, number(y1)?, number(x2)?, number(y2)?);
                let color = Rgba {
                    r: byte(r)?,
                    g: byte(g)?,
                    b: byte(b)?,
                    a: byte(a)?,
                };
                if *kind == "line" {
                    Self::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    }
                } else {
                    Self::Rect {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    }
                }
            }
            ["clear"] => Self::Clear,
            _ => return Ok(None),
        }))
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
    const ALL: [Self; 4] = [Self::Off, Self::Cross, Self::Dot, Self::X];

    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Cross => "cross",
            Self::Dot => "dot",
            Self::X => "x",
        }
    }

    pub fn value(self) -> u8 {
        match self {
            Self::Off => ipc::cursor_style::OFF,
            Self::Cross => ipc::cursor_style::CROSS,
            Self::Dot => ipc::cursor_style::DOT,
            Self::X => ipc::cursor_style::X,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub style: CursorStyle,
    pub color: [u8; 3],
    pub size: u8,
}

impl Cursor {
    pub fn args(&self, pid: u32) -> Vec<String> {
        let [r, g, b] = self.color;
        vec![
            "cursor".into(),
            pid.to_string(),
            self.style.name().into(),
            r.to_string(),
            g.to_string(),
            b.to_string(),
            self.size.to_string(),
        ]
    }

    pub fn from_args(args: &[&str]) -> Option<Result<(u32, Self), String>> {
        let ["cursor", pid, style, r, g, b, size] = args else {
            return None;
        };
        let Some(style) = CursorStyle::ALL.into_iter().find(|s| s.name() == *style) else {
            return Some(Err(format!(
                "Invalid cursor style: {style} (off, cross, dot or x)"
            )));
        };
        Some(parse_pid(pid).and_then(|pid| {
            Ok((
                pid,
                Self {
                    style,
                    color: [byte(r)?, byte(g)?, byte(b)?],
                    size: byte(size)?,
                },
            ))
        }))
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
    use decode::Chunk;

    use super::*;

    #[test]
    fn input_round_trips_through_args() {
        for input in [
            Input::Move { x: 10, y: 20 },
            Input::WindMouse {
                x: -3,
                y: 400,
                options: wind_mouse::Options {
                    speed: 12.5,
                    wind: 0.1,
                    gravity: 9.0,
                    timeout_ms: 2500,
                    pace: 1.25,
                },
            },
            Input::Click {
                button: MouseButton::Right,
                x: 0,
                y: 5,
                hold_ms: 90,
            },
            Input::Button {
                button: MouseButton::Middle,
                down: true,
            },
            Input::Button {
                button: MouseButton::Left,
                down: false,
            },
            Input::Scroll { notches: -3 },
            Input::Key {
                key: 37,
                down: true,
            },
            Input::Key {
                key: 160,
                down: false,
            },
            Input::Type {
                text: "Hello \"world\"".into(),
                hold_ms: 45,
            },
        ] {
            let args = input.args(32);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            assert_eq!(Input::from_args(&args), Some(Ok((32, input))));
        }
        assert_eq!(Input::from_args(&["list"]), None);
        assert_eq!(
            Input::from_args(&["type", "1", "hi"]),
            Some(Ok((
                1,
                Input::Type {
                    text: "hi".into(),
                    hold_ms: KEY_HOLD_MS
                }
            )))
        );
        assert!(matches!(
            Input::from_args(&["key", "1", "256", "down"]),
            Some(Err(_))
        ));
        assert!(matches!(
            Input::from_args(&["button", "1", "left", "held"]),
            Some(Err(_))
        ));
    }

    #[test]
    fn cursor_round_trips_through_args() {
        for style in CursorStyle::ALL {
            let cursor = Cursor {
                style,
                color: [1, 2, 3],
                size: 9,
            };
            let args = cursor.args(7);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            assert_eq!(Cursor::from_args(&args), Some(Ok((7, cursor))));
        }
        assert_eq!(
            Cursor::from_args(&["cursor", "7", "ring", "1", "2", "3", "9"]),
            Some(Err(
                "Invalid cursor style: ring (off, cross, dot or x)".into()
            ))
        );
    }

    #[test]
    fn chunk_of_a_position() {
        let position = Position {
            local_x: 0,
            local_y: 0,
            x: 3213,
            y: 3428,
            plane: Some(0),
        };
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
                assert!(
                    size as f64 <= (real as f64 * 0.3).max(1_000.0) + 1.0,
                    "{real} {offset}"
                );
            }
        }
        assert_eq!(
            (6_019_725 + random_total_xp_offset(6_019_725, 7))
                .to_string()
                .len(),
            7
        );
    }
}
