use std::{path::Path, process::Command};

use injector::{
    Connection, Cursor, DisplayName, GameState, Input, Item, LoadReport, Npc, Overlay, PacketPage,
    Player, Widget,
};
use serde::{Deserialize, Serialize};

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
            .map(|client| Target {
                client,
                location: Location,
            })
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

pub fn set_display_name(
    target: &Target,
    _settings: &Settings,
    name: &str,
) -> Result<DisplayName, String> {
    injector::set_display_name(target.client.pid, name)
}

pub fn varps(target: &Target, _settings: &Settings) -> Result<Option<Vec<i32>>, String> {
    injector::varps(target.client.pid)
}

pub fn varp(target: &Target, _settings: &Settings, id: usize) -> Result<Option<i32>, String> {
    injector::varp_value(target.client.pid, id)
}

pub fn widget(
    target: &Target,
    _settings: &Settings,
    group: u16,
    child: u16,
) -> Result<Option<Widget>, String> {
    injector::widget(target.client.pid, group, child)
}

pub fn item_container(
    target: &Target,
    _settings: &Settings,
    id: i32,
) -> Result<Option<Vec<Item>>, String> {
    injector::item_container(target.client.pid, id)
}

pub fn item_containers(
    target: &Target,
    _settings: &Settings,
) -> Result<Vec<(i32, Vec<Item>)>, String> {
    injector::item_containers(target.client.pid)
}

pub fn widget_tree(target: &Target, _settings: &Settings) -> Result<injector::WidgetTree, String> {
    injector::widget_tree(target.client.pid)
}

pub fn game_val_names(
    target: &Target,
    _settings: &Settings,
    kind: u32,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    injector::game_val_names(target.client.pid, kind, ids)
}

pub fn item_stackable(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<bool>>, String> {
    injector::item_stackable(target.client.pid, ids)
}

/// One read of the item log is at most `ITEM_CHANGES_PER_READ` changes; with nothing new it blocks up to `wait`.
pub fn item_changes(
    target: &Target,
    _settings: &Settings,
    since: u64,
    wait: std::time::Duration,
) -> Result<injector::ItemPage, String> {
    injector::item_changes_wait(target.client.pid, since, 2000, wait)
}

pub fn item_names(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    injector::item_names(target.client.pid, ids)
}

pub fn varbits_of(
    target: &Target,
    _settings: &Settings,
    varp: u16,
) -> Result<Vec<(i32, injector::config::VarbitDef, Option<String>)>, String> {
    injector::varbits_of(target.client.pid, varp)
}

pub fn varc(target: &Target, _settings: &Settings, id: i32) -> Result<Option<i32>, String> {
    injector::varc(target.client.pid, id)
}

pub fn scene_tiles_next_frame(
    target: &Target,
    _settings: &Settings,
    layers: &[&str],
    after: u32,
) -> Result<Option<u32>, String> {
    injector::scene_tiles_next_frame(target.client.pid, layers, after)
}

pub fn npcs(target: &Target, _settings: &Settings) -> Result<Vec<Npc>, String> {
    injector::npcs(target.client.pid)
}

pub fn players(target: &Target, _settings: &Settings) -> Result<Vec<Player>, String> {
    injector::players(target.client.pid)
}

/// One read of the packet log is at most `PACKETS_PER_READ` packets, as through the helper.
pub fn packets(target: &Target, _settings: &Settings, since: u64) -> Result<PacketPage, String> {
    injector::packets(target.client.pid, since, 1000)
}

/// What the agent read about every server packet from the client's code, once it has.
pub fn packet_layouts(
    target: &Target,
    _settings: &Settings,
) -> Result<Option<Vec<injector::PacketLayout>>, String> {
    injector::packet_layouts(target.client.pid)
}

/// One read of the script log is at most `SCRIPT_RUNS_PER_READ` runs, as through the helper.
pub fn script_runs(
    target: &Target,
    _settings: &Settings,
    since: u64,
) -> Result<injector::ScriptPage, String> {
    injector::script_runs(target.client.pid, since, 2000)
}

pub fn varc_changes(
    target: &Target,
    _settings: &Settings,
    since: u64,
) -> Result<injector::VarcPage, String> {
    injector::varc_changes(target.client.pid, since, 1000)
}

pub fn connection(target: &Target, _settings: &Settings) -> Result<Option<Connection>, String> {
    Ok(injector::connection(target.client.pid))
}

/// `lib` is only for scripts run from a local file: store scripts get the downloaded lib beside them.
pub fn script_command(
    target: Option<&Target>,
    settings: &Settings,
    script: Option<&Path>,
    lib: Option<&Path>,
) -> Result<Command, String> {
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
