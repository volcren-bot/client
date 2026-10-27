use std::{path::Path, process::Command};

use serde::{Deserialize, Serialize};
use injector::{Connection, Cursor, DisplayName, GameState, Input, Item, LoadReport, Npc, Overlay, Player, Widget};

use super::{Scan, Settings, Target};

#[derive(Clone, Serialize, Deserialize)]
pub struct Location;

impl Location {
    pub fn label(&self) -> Option<String> {
        None
    }
}

pub fn scan(_settings: &Settings) -> Scan {
    Scan {
        targets: injector::list_clients()
            .into_iter()
            .map(|client| Target { client, location: Location })
            .collect(),
        errors: Vec::new(),
        dll_version: None,
    }
}

pub fn load(target: &Target, settings: &Settings, cursor: &Cursor) -> Result<LoadReport, String> {
    injector::load(target.client.pid, Path::new(&settings.dll), Some(cursor))
}

pub fn unload(target: &Target, _settings: &Settings) -> Result<(), String> {
    injector::unload(target.client.pid)
}

pub fn console(target: &Target, _settings: &Settings) -> Result<(), String> {
    injector::open_console(target.client.pid)
}

pub fn input(target: &Target, _settings: &Settings, input: &Input) -> Result<(), String> {
    injector::send_input(target.client.pid, input)
}

pub fn draw(target: &Target, _settings: &Settings, overlay: &Overlay) -> Result<(), String> {
    injector::draw(target.client.pid, overlay)
}

pub fn set_cursor(target: &Target, _settings: &Settings, cursor: &Cursor) -> Result<(), String> {
    injector::set_cursor(target.client.pid, cursor)
}

pub fn state(target: &Target, _settings: &Settings) -> Result<GameState, String> {
    injector::game_state(target.client.pid)
}

pub fn set_display_name(target: &Target, _settings: &Settings, name: &str) -> Result<DisplayName, String> {
    injector::set_display_name(target.client.pid, name)
}

pub fn varps(target: &Target, _settings: &Settings) -> Result<Option<Vec<i32>>, String> {
    injector::varps(target.client.pid)
}

pub fn varp(target: &Target, _settings: &Settings, id: usize) -> Result<Option<i32>, String> {
    injector::varp_value(target.client.pid, id)
}

pub fn widget(target: &Target, _settings: &Settings, group: u16, child: u16) -> Result<Option<Widget>, String> {
    injector::widget(target.client.pid, group, child)
}

pub fn item_container(target: &Target, _settings: &Settings, id: i32) -> Result<Option<Vec<Item>>, String> {
    injector::item_container(target.client.pid, id)
}

pub fn varc(target: &Target, _settings: &Settings, id: i32) -> Result<Option<i32>, String> {
    injector::varc(target.client.pid, id)
}

pub fn scene_tiles_next_frame(target: &Target, _settings: &Settings, layers: &[&str], after: u32) -> Result<Option<u32>, String> {
    injector::scene_tiles_next_frame(target.client.pid, layers, after)
}

pub fn npcs(target: &Target, _settings: &Settings) -> Result<Vec<Npc>, String> {
    injector::npcs(target.client.pid)
}

pub fn players(target: &Target, _settings: &Settings) -> Result<Vec<Player>, String> {
    injector::players(target.client.pid)
}

pub fn connection(target: &Target, _settings: &Settings) -> Result<Option<Connection>, String> {
    Ok(injector::connection(target.client.pid))
}

/// `lib` is only for scripts run from a local file: store scripts get the downloaded lib beside them.
pub fn script_command(target: Option<&Target>, settings: &Settings, script: Option<&Path>, lib: Option<&Path>) -> Result<Command, String> {
    if settings.helper.is_empty() || !Path::new(&settings.helper).is_file() {
        return Err("Set the path to injector.exe, scripts run in it.".into());
    }
    let mut command = Command::new(&settings.helper);
    command.arg("run");
    if let Some(script) = script {
        command.arg("--script").arg(script);
        if let Some(lib) = lib {
            command.arg("--lib").arg(lib);
        }
    }
    if let Some(target) = target {
        command.arg(target.client.pid.to_string());
    }
    Ok(command)
}

pub fn shutdown() {}
