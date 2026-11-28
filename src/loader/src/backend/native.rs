use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::SystemTime,
};

use injector::{Connection, Cursor, DisplayName, GameState, Item, LoadReport, Overlay, PacketPage};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{Scan, Settings, Target};

#[derive(Clone, Serialize, Deserialize)]
pub struct Location;

impl Location {
    pub fn label(&self) -> Option<String> {
        None
    }
}

pub fn scan(settings: &Settings) -> Scan {
    let (agent_version, errors) = match helper_agent_version(settings) {
        Ok(version) => (Some(version), Vec::new()),
        Err(error) => (None, vec![error]),
    };
    Scan {
        targets: injector::list_clients()
            .into_iter()
            .map(|client| Target {
                client,
                location: Location,
            })
            .collect(),
        errors,
        agent_version,
    }
}

type HelperStamp = (PathBuf, SystemTime, u64);

static AGENT_VERSION: Mutex<Option<(HelperStamp, String)>> = Mutex::new(None);

pub fn agent_version(_target: &Target, settings: &Settings) -> Result<String, String> {
    helper_agent_version(settings)
}

fn helper_agent_version(settings: &Settings) -> Result<String, String> {
    let helper = PathBuf::from(&settings.helper);
    let metadata = helper
        .metadata()
        .map_err(|e| format!("Cannot read {}: {e}", helper.display()))?;
    let stamp = (
        helper,
        metadata.modified().map_err(|e| e.to_string())?,
        metadata.len(),
    );
    let mut cached = AGENT_VERSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((seen, version)) = cached.as_ref()
        && *seen == stamp
    {
        return Ok(version.clone());
    }
    let version: String = helper_call(settings, &["agent-version"])?;
    *cached = Some((stamp, version.clone()));
    Ok(version)
}

fn helper_call<T: DeserializeOwned>(settings: &Settings, args: &[&str]) -> Result<T, String> {
    if !Path::new(&settings.helper).is_file() {
        return Err("Set the path to injector.exe, it loads the agent.".into());
    }
    let mut command = Command::new(&settings.helper);
    super::no_window(&mut command);
    let output = command
        .args(args)
        .output()
        .map_err(|e| format!("Couldn't run {}: {e}", settings.helper))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<Result<T, String>>(stdout.trim()).map_err(|e| {
        format!(
            "Unexpected output from injector.exe ({e}): {}{}",
            stdout.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
    })?
}

pub fn load(target: &Target, settings: &Settings, cursor: &Cursor) -> Result<LoadReport, String> {
    let cursor = cursor.args(target.client.pid);
    let pid = target.client.pid.to_string();
    let mut args = vec!["load", pid.as_str()];
    args.extend(cursor[2..].iter().map(String::as_str));
    helper_call(settings, &args)
}

pub fn unload(target: &Target, _settings: &Settings) -> Result<(), String> {
    injector::unload(target.client.pid)
}

pub fn console(target: &Target, _settings: &Settings) -> Result<(), String> {
    injector::open_console(target.client.pid)
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

pub fn hider(target: &Target, _settings: &Settings) -> Result<u64, String> {
    injector::hider(target.client.pid)
}

pub fn set_hider(target: &Target, _settings: &Settings, mask: u64) -> Result<u64, String> {
    injector::set_hider(target.client.pid, mask)
}

pub fn proxy(target: &Target, _settings: &Settings) -> Result<injector::ProxyStatus, String> {
    injector::proxy(target.client.pid)
}

pub fn set_proxy(
    target: &Target,
    _settings: &Settings,
    proxy: Option<&injector::Proxy>,
) -> Result<injector::ProxyStatus, String> {
    injector::set_proxy(target.client.pid, proxy)
}

pub fn proxy_version(_target: &Target, settings: &Settings) -> Result<u32, String> {
    helper_call(settings, &["proxy-version"])
}

pub fn varps(target: &Target, _settings: &Settings) -> Result<Option<Vec<i32>>, String> {
    injector::varps(target.client.pid)
}

pub fn house(target: &Target, _settings: &Settings) -> Result<injector::house::Survey, String> {
    injector::house(target.client.pid)
}

pub fn varbit(target: &Target, _settings: &Settings, id: i32) -> Result<Option<i32>, String> {
    injector::varbit(target.client.pid, id)
}

pub fn enum_def(
    target: &Target,
    _settings: &Settings,
    id: i32,
) -> Result<Option<injector::config::EnumDef>, String> {
    injector::enum_def(target.client.pid, id)
}

pub fn varp(target: &Target, _settings: &Settings, id: usize) -> Result<Option<i32>, String> {
    injector::varp_value(target.client.pid, id)
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

pub fn item_families(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<injector::config::ItemFamily>, String> {
    injector::item_families(target.client.pid, ids)
}

pub fn item_icons(
    target: &Target,
    _settings: &Settings,
    icons: &[injector::ItemIconRequest],
) -> Result<Vec<Option<injector::Image>>, String> {
    injector::item_icons(target.client.pid, icons)
}

pub fn sprites(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<injector::Image>>, String> {
    injector::sprites(target.client.pid, ids)
}

pub fn item_changes(
    target: &Target,
    _settings: &Settings,
    since: u64,
    wait: std::time::Duration,
) -> Result<injector::ItemPage, String> {
    injector::item_changes_wait(target.client.pid, since, 2000, wait)
}

pub fn item_params(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<std::collections::BTreeMap<u32, injector::config::Value>>>, String> {
    injector::item_params(target.client.pid, ids)
}

pub fn item_names(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    injector::item_names(target.client.pid, ids)
}

pub fn npcs(target: &Target, _settings: &Settings) -> Result<Vec<injector::Npc>, String> {
    injector::npcs(target.client.pid)
}

pub fn players(target: &Target, _settings: &Settings) -> Result<Vec<injector::Player>, String> {
    injector::players(target.client.pid)
}

pub fn locs(target: &Target, _settings: &Settings) -> Result<Vec<injector::Loc>, String> {
    injector::locs(target.client.pid, None)
}

pub fn ground_items(
    target: &Target,
    _settings: &Settings,
) -> Result<Vec<injector::GroundItem>, String> {
    injector::ground_items(target.client.pid)
}

pub fn sub_interfaces(
    target: &Target,
    _settings: &Settings,
) -> Result<Option<Vec<injector::SubInterface>>, String> {
    injector::sub_interfaces(target.client.pid)
}

pub fn chat_messages(
    target: &Target,
    _settings: &Settings,
) -> Result<Vec<injector::ChatMessage>, String> {
    injector::chat_messages(target.client.pid)
}

pub fn npc_actions(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<std::collections::BTreeMap<i32, Vec<String>>, String> {
    injector::npc_actions(target.client.pid, ids)
}

pub fn loc_shown(
    target: &Target,
    _settings: &Settings,
    ids: &[i32],
) -> Result<Vec<injector::LocView>, String> {
    let ids = ids
        .iter()
        .map(|id| u32::try_from(*id).map_err(|_| format!("{id} isn't an object id")))
        .collect::<Result<Vec<u32>, String>>()?;
    injector::loc_shown(target.client.pid, &ids)
}

pub fn varbits_of(
    target: &Target,
    _settings: &Settings,
    varp: u16,
) -> Result<Vec<(i32, decode::definitions::VarbitDef, Option<String>)>, String> {
    injector::varbits_of(target.client.pid, varp)
}

pub fn scene_tiles_next_frame(
    target: &Target,
    _settings: &Settings,
    layers: &[&str],
    after: u32,
) -> Result<Option<u32>, String> {
    injector::scene_tiles_next_frame(target.client.pid, layers, after)
}

pub fn packets(target: &Target, _settings: &Settings, since: u64) -> Result<PacketPage, String> {
    injector::packets(target.client.pid, since, 1000)
}

pub fn packet_layouts(
    target: &Target,
    _settings: &Settings,
) -> Result<Option<Vec<injector::PacketLayout>>, String> {
    injector::packet_layouts(target.client.pid)
}

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

pub fn script_command(
    target: &Target,
    settings: &Settings,
    script: Option<&Path>,
    sdk: Option<&Path>,
) -> Result<Command, String> {
    if !Path::new(&settings.helper).is_file() {
        return Err("Set the path to injector.exe, scripts run in it.".into());
    }
    let mut command = Command::new(&settings.helper);
    super::no_window(&mut command);
    super::run_args(&mut command, script, sdk);
    command.arg(target.client.pid.to_string());
    Ok(command)
}

pub fn shutdown() {}
