//! Decoders over the client's memory and their data, run by the injector via `ReadProcessMemory` and in-process by the agent.

pub use postcard;

use ipc::state::game_state;
use serde::{Deserialize, Serialize};

pub mod chat;
pub mod memory;
pub mod state;
pub mod widgets;
pub mod worker;
pub mod world;
pub mod world_select;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub local_x: i32,
    pub local_y: i32,
    pub x: i32,
    pub y: i32,
    /// 0 is the ground floor; `None` when the projection offsets weren't found.
    pub plane: Option<i32>,
}

impl Position {
    pub fn chunk(&self) -> Chunk {
        Chunk { x: self.x >> 6, y: self.y >> 6 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub x: i32,
    pub y: i32,
}

impl Chunk {
    pub fn id(&self) -> i32 {
        self.x << 8 | self.y
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub level: i32,
    pub real_level: i32,
    pub xp: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Login {
    pub screen: i32,
    pub panel_x: i32,
    pub box_x: i32,
    pub center_x: i32,
    pub y: i32,
    pub scale: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    pub position: Option<Position>,
    #[serde(default)]
    pub player_name: Option<String>,
    /// The logged in account's normalized name; unlike `player_name` it is never the overridden display name.
    #[serde(default)]
    pub account_name: Option<String>,
    pub game_state: Option<i32>,
    #[serde(default)]
    pub skills: Option<Vec<Skill>>,
    #[serde(default)]
    pub tick_count: Option<i32>,
    #[serde(default)]
    pub login: Option<Login>,
    #[serde(default)]
    pub root_interface: Option<i32>,
    #[serde(default)]
    pub ui_scale: Option<f32>,
    #[serde(default)]
    pub run_energy: Option<i32>,
    #[serde(default)]
    pub weight: Option<i32>,
    #[serde(default)]
    pub camera: Option<Camera>,
    #[serde(default)]
    pub minimap: Option<Minimap>,
    #[serde(default)]
    pub destination: Option<Tile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub pitch: i32,
    pub yaw: i32,
    pub focal_x: i32,
    pub focal_y: i32,
    pub focal_z: i32,
    pub zoom: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Minimap {
    pub zoom: i32,
}

impl GameState {
    pub fn logged_in(&self) -> Option<bool> {
        self.game_state.map(|state| state == game_state::LOGGED_IN)
    }
}

pub fn game_state_name(state: i32) -> String {
    game_state::name(state).map_or_else(|| format!("unknown state {state}"), str::to_owned)
}

pub fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(if c == '\u{a0}' { ' ' } else { c }),
            _ => {}
        }
    }
    out
}
