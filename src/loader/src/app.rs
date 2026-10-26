use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};

use eframe::egui;
use injector::{Connection, Cursor, CursorStyle, DisplayName, GameState, Input, LoadReport, MouseButton, Overlay, varp};
use serde::{Deserialize, Serialize};

use loader::backend::{self, Scan, Settings, Target};

use crate::highlight::Highlighter;
use crate::script::{Script, ScriptEvent};

enum Event {
    Scanned(Scan),
    Loaded {
        pid: u32,
        result: Result<LoadReport, String>,
    },
    Unloaded {
        pid: u32,
        result: Result<(), String>,
    },
    Console {
        pid: u32,
        result: Result<(), String>,
    },
    Cursor {
        pid: u32,
        result: Result<(), String>,
    },
    State {
        pid: u32,
        read: Read,
        result: Result<GameState, String>,
    },
    Varps {
        pid: u32,
        read: Read,
        result: Result<Option<Vec<i32>>, String>,
    },
    Input {
        pid: u32,
        input: Input,
        result: Result<(), String>,
    },
    Mouse {
        pid: u32,
        result: Result<Option<Connection>, String>,
    },
    Name {
        pid: u32,
        name: String,
        result: Result<DisplayName, String>,
    },
    Tiles {
        pid: u32,
        result: Result<usize, String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Clients,
    State,
    Varps,
    Input,
    Scripts,
    Development,
    Settings,
}

const DEV_TOGGLES: [&str; 33] = [
    "Players",
    "NPCs",
    "Ground Items",
    "Ground Objects",
    "Game Objects",
    "Grid",
    "Graphics Objects",
    "Walls",
    "Decorations",
    "Projectiles",
    "Location",
    "World Map Location",
    "Tile Location",
    "Camera Position",
    "Zone Borders",
    "Map Squares",
    "Loading Lines",
    "Line Of Sight",
    "Valid Movement",
    "Movement Flags",
    "Interacting",
    "Examine",
    "Detached Camera",
    "Widget Inspector",
    "Var Inspector",
    "Sound Effects",
    "Script Inspector",
    "Inventory Inspector",
    "Tile flags",
    "Shell",
    "Menus",
    "Swing Defaults",
    "World Entities",
];

const DEV_COLUMNS: usize = 4;

const CURSOR_STYLES: [CursorStyle; 4] = [CursorStyle::Off, CursorStyle::Cross, CursorStyle::X, CursorStyle::Dot];

const DEFAULT_CURSOR: Cursor = Cursor {
    style: CursorStyle::Cross,
    color: [255, 0, 0],
    size: 20,
};

fn cursor_style_label(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::Off => "Off",
        CursorStyle::Cross => "Cross",
        CursorStyle::Dot => "Dot",
        CursorStyle::X => "X",
    }
}

// The scene layer each checkbox draws (see `injector::scene_tiles`), by index in `DEV_TOGGLES`; unported ones have none.
const DEV_LAYERS: [Option<&str>; DEV_TOGGLES.len()] = [
    Some("players"),
    Some("npcs"),
    Some("ground-items"),
    Some("ground-objects"),
    Some("objects"),
    Some("grid"),
    None,
    Some("walls"),
    Some("decorations"),
    None,
    Some("location"),
    None,
    Some("tile-location"),
    Some("camera"),
    Some("zone-borders"),
    Some("map-squares"),
    Some("loading-lines"),
    Some("line-of-sight"),
    Some("valid-movement"),
    Some("movement-flags"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("tile-flags"),
    None,
    None,
    None,
    None,
];
// The pause between scene tile paints so the worker yields; the injector skips repeats cheaply.
const TILES_PAUSE: Duration = Duration::from_millis(1);

// Paints scene tiles back to back on its own thread so the overlay trails by one paint; dropping it stops and clears.
struct TilesWorker {
    pid: u32,
    layers: Arc<Mutex<Vec<&'static str>>>,
    stop: Arc<AtomicBool>,
}

impl TilesWorker {
    fn start(ctx: &egui::Context, tx: Sender<Event>, target: Target, settings: Settings, layers: Vec<&'static str>) -> Self {
        let pid = target.client.pid;
        let worker = Self { pid, layers: Arc::new(Mutex::new(layers)), stop: Arc::new(AtomicBool::new(false)) };
        let (layers, stop, ctx) = (worker.layers.clone(), worker.stop.clone(), ctx.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let current = layers.lock().map(|l| l.clone()).unwrap_or_default();
                if let Err(error) = backend::scene_tiles(&target, &settings, &current) {
                    let _ = tx.send(Event::Tiles { pid, result: Err(error) });
                    ctx.request_repaint();
                    return;
                }
                thread::sleep(TILES_PAUSE);
            }
            // Take the boxes off the client once nothing can paint them again.
            let _ = backend::draw(&target, &settings, &Overlay::Clear);
        });
        worker
    }
}

impl Drop for TilesWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const SCRIPT_KEY: &str = "script";
const FORMS_KEY: &str = "forms";
const CURSOR_KEY: &str = "cursor";

const EXAMPLE_SCRIPT: &str = r#"local GameState = require("game_state")
local Login = require("login")

local function current_state()
  local state, err = game_state()
  assert(state ~= nil, err)
  return state
end

local state = current_state()
while state.game_state ~= GameState.LOGGED_IN do
  local ok, err = Login.login()
  assert(ok ~= nil, err)
  sleep(1000)
  state = current_state()
end

local total_xp = 0
local hp = nil
for _, skill in ipairs(state.skills) do
  total_xp += skill.xp
  if skill.name == "Hitpoints" then
    hp = skill.level
  end
end
print("total xp: ", total_xp)
print("hp: ", hp)
"#;

/// The script editor's find bar (Ctrl+F) and replace row (Ctrl+R).
#[derive(Default)]
struct Find {
    open: bool,
    replace: bool,
    query: String,
    replacement: String,
    case_sensitive: bool,
    /// Index into the matches of the query, for the one that is current.
    current: usize,
    /// Scroll the editor to the current match on this frame.
    jump: bool,
    /// Put the cursor in the find field on this frame.
    focus: bool,
}

/// Byte offsets of every non-overlapping match of `query` in `text`.
fn find_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        text.match_indices(query).map(|(at, _)| at).collect()
    } else {
        // ASCII lowercasing keeps byte offsets, so the matches still index into `text`.
        let lower = text.to_ascii_lowercase();
        lower.match_indices(&query.to_ascii_lowercase()).map(|(at, _)| at).collect()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct ScriptFile {
    path: String,
    text: String,
    /// What the file held on disk when the editor last synced with it (open, save, startup).
    disk: Option<String>,
}

impl Default for ScriptFile {
    fn default() -> Self {
        Self {
            path: String::new(),
            text: EXAMPLE_SCRIPT.into(),
            disk: None,
        }
    }
}

impl ScriptFile {
    fn name(&self) -> String {
        std::path::Path::new(&self.path).file_name().map_or_else(
            || "script".into(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

fn default_script_path() -> Option<std::path::PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        exe_dir.join("scripts/default.luau"),
        exe_dir.join("../../../scripts/scripts/default.luau"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn load_default_script() -> Option<ScriptFile> {
    let path = default_script_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    Some(ScriptFile {
        path: path.display().to_string(),
        disk: Some(text.clone()),
        text,
    })
}

const KNOWN_VARPS: [(&str, usize); 11] = [
    ("Attack style", varp::ATTACK_STYLE),
    ("Active prayers", 83),
    ("Quest points", varp::QUEST_POINTS),
    ("Poison", varp::POISON),
    ("Run", varp::RUN),
    ("Tutorial progress", 281),
    ("Special attack", varp::SPECIAL_ATTACK),
    ("Special attack selected", varp::SPECIAL_ATTACK_ENABLED),
    ("Slayer task size", 394),
    ("Slayer task creature", 395),
    ("Last home teleport", 892),
];

fn describe_varp(id: usize, value: i32) -> Option<String> {
    Some(match id {
        varp::RUN | varp::SPECIAL_ATTACK_ENABLED => (if value == 1 { "on" } else { "off" }).into(),
        varp::SPECIAL_ATTACK => format!("{}%", value / 10),
        varp::POISON if value >= 1_000_000 => "envenomed".into(),
        varp::POISON if value > 0 => "poisoned".into(),
        varp::POISON => "healthy".into(),
        83 => format!("bits {value:#b}"),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Read {
    GameState,
    Position,
    Chunk,
    Skills,
    TickCount,
    Login,
    RootInterface,
    Varp(usize),
    KnownVarps,
    VarpChanges,
}

struct LogLine {
    error: bool,
    text: String,
    /// Index into `Log::tags` of the line's leading `[tag]`.
    tag: Option<usize>,
}

/// One log tab's output; `view` and `text` hold only lines the tag filter lets through and are rebuilt only when it changes.
#[derive(Default)]
struct Log {
    lines: Vec<LogLine>,
    tags: Vec<String>,
    filter: Option<usize>,
    view: Vec<usize>,
    text: String,
}

impl Log {
    fn push(&mut self, error: bool, text: String) {
        let tag = line_tag(&text).map(|name| match self.tags.iter().position(|known| known == name) {
            Some(index) => index,
            None => {
                self.tags.push(name.to_owned());
                self.tags.len() - 1
            }
        });
        if self.filter.is_none() || self.filter == tag {
            self.show(self.lines.len(), &text);
        }
        self.lines.push(LogLine { error, text, tag });
    }

    fn show(&mut self, index: usize, text: &str) {
        if !self.view.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(text);
        self.view.push(index);
    }

    fn set_filter(&mut self, filter: Option<usize>) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        self.view.clear();
        self.text.clear();
        let lines = std::mem::take(&mut self.lines);
        for (index, line) in lines.iter().enumerate() {
            if filter.is_none() || filter == line.tag {
                self.show(index, &line.text);
            }
        }
        self.lines = lines;
    }

    fn clear(&mut self) {
        *self = Self::default();
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// The `tag` of a line that starts with `[tag]`.
fn line_tag(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('[')?;
    let end = rest.find(']')?;
    (end > 0 && end <= 32 && !rest[..end].contains('[')).then(|| &rest[..end])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Loader,
    Script,
}

pub struct LoaderApp {
    settings: Settings,
    tab: Tab,
    text: String,
    mouse: (i32, i32),
    display_name: String,
    varp: usize,
    varps: Option<Vec<i32>>,
    script_file: ScriptFile,
    find: Find,
    highlighter: Highlighter,
    script: Option<Script>,
    form: Option<crate::form::Form>,
    /// The file's new contents, while asking whether to reload it before running.
    reload_prompt: Option<String>,
    remembered_forms: std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>>,
    dev_toggles: [bool; DEV_TOGGLES.len()],
    cursor: Cursor,
    tiles: Option<TilesWorker>,
    targets: Vec<Target>,
    dll_version: Option<String>,
    selected: Option<String>,
    busy: Option<&'static str>,
    log_tab: LogTab,
    /// The main tab the output tab last followed, so switching main tabs switches the output but the output can still be changed on its own.
    followed_tab: Tab,
    script_log: Log,
    loader_log: Log,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl LoaderApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        settings.fill_missing();
        crate::theme::apply(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let (tx, rx) = channel();
        let mut app = Self {
            settings,
            tab: Tab::Clients,
            text: String::new(),
            mouse: (0, 0),
            display_name: String::new(),
            varp: varp::RUN,
            varps: None,
            script_file: load_default_script()
                .or_else(|| cc.storage.and_then(|s| eframe::get_value(s, SCRIPT_KEY)))
                .unwrap_or_default(),
            find: Find::default(),
            highlighter: Highlighter::default(),
            script: None,
            form: None,
            reload_prompt: None,
            remembered_forms: cc
                .storage
                .and_then(|s| eframe::get_value(s, FORMS_KEY))
                .unwrap_or_default(),
            dev_toggles: [false; DEV_TOGGLES.len()],
            cursor: cc
                .storage
                .and_then(|s| eframe::get_value(s, CURSOR_KEY))
                .unwrap_or(DEFAULT_CURSOR),
            tiles: None,
            targets: Vec::new(),
            dll_version: None,
            selected: None,
            busy: None,
            log_tab: LogTab::Loader,
            followed_tab: Tab::Clients,
            script_log: Log::default(),
            loader_log: Log::default(),
            tx,
            rx,
        };
        app.scan(&cc.egui_ctx);
        app
    }

    fn info(&mut self, text: impl Into<String>) {
        self.loader_log.push(false, text.into());
    }

    fn error(&mut self, text: impl Into<String>) {
        self.loader_log.push(true, text.into());
    }

    fn script_info(&mut self, text: impl Into<String>) {
        self.script_log.push(false, text.into());
    }

    fn script_error(&mut self, text: impl Into<String>) {
        self.script_log.push(true, text.into());
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        label: &'static str,
        job: impl FnOnce(Settings) -> Event + Send + 'static,
    ) {
        self.busy = Some(label);
        self.background(ctx, job);
    }

    // Like `spawn`, but leaves the busy label alone.
    fn background(
        &self,
        ctx: &egui::Context,
        job: impl FnOnce(Settings) -> Event + Send + 'static,
    ) {
        let (tx, ctx, settings) = (self.tx.clone(), ctx.clone(), self.settings.clone());
        thread::spawn(move || {
            let _ = tx.send(job(settings));
            ctx.request_repaint();
        });
    }

    // Keeps the scene tiles on while a layer checkbox is checked, refreshing as actors move; layers share one overlay.
    fn tiles_ui(&mut self, ctx: &egui::Context) {
        let layers: Vec<&'static str> = DEV_LAYERS
            .iter()
            .zip(&self.dev_toggles)
            .filter(|(_, on)| **on)
            .filter_map(|(layer, _)| *layer)
            .collect();
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        let target = if layers.is_empty() { None } else { target };

        let Some(target) = target else {
            // Switched off or another client selected: dropping the worker takes the boxes off.
            self.tiles = None;
            return;
        };
        if self.tiles.as_ref().is_some_and(|worker| worker.pid != target.client.pid) {
            self.tiles = None;
        }
        match &self.tiles {
            Some(worker) => {
                if let Ok(mut current) = worker.layers.lock() {
                    *current = layers;
                }
            }
            None => self.tiles = Some(TilesWorker::start(ctx, self.tx.clone(), target.clone(), self.settings.clone(), layers)),
        }
    }

    fn scan(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, "Looking for clients…", |settings| {
            let mut scan = backend::scan(&settings);
            match settings.dll_version() {
                Ok(version) => scan.dll_version = Some(version),
                Err(error) if !settings.dll.is_empty() => scan.errors.push(error),
                Err(_) => {}
            }
            Event::Scanned(scan)
        });
    }

    fn unload(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.info(format!("Unloading agent from {pid}…"));
        self.spawn(ctx, "Unloading…", move |settings| Event::Unloaded {
            pid,
            result: backend::unload(&target, &settings),
        });
    }

    fn console(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.spawn(ctx, "Opening console…", move |settings| Event::Console {
            pid,
            result: backend::console(&target, &settings),
        });
    }

    fn set_cursor(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        let cursor = self.cursor;
        self.background(ctx, move |settings| Event::Cursor {
            pid,
            result: backend::set_cursor(&target, &settings, &cursor),
        });
    }

    fn load(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.info(format!("Loading into {pid}…"));
        let cursor = self.cursor;
        self.spawn(ctx, "Loading…", move |settings| Event::Loaded {
            pid,
            result: backend::load(&target, &settings, &cursor),
        });
    }

    fn read_state(&mut self, ctx: &egui::Context, target: Target, read: Read) {
        let pid = target.client.pid;
        if matches!(read, Read::Varp(_) | Read::KnownVarps | Read::VarpChanges) {
            self.spawn(ctx, "Reading…", move |settings| Event::Varps {
                pid,
                read,
                result: backend::varps(&target, &settings),
            });
            return;
        }
        self.spawn(ctx, "Reading…", move |settings| Event::State {
            pid,
            read,
            result: backend::state(&target, &settings),
        });
    }

    fn send_input(&mut self, ctx: &egui::Context, target: Target, input: Input) {
        let pid = target.client.pid;
        self.spawn(ctx, "Sending input…", move |settings| {
            let result = backend::input(&target, &settings, &input);
            Event::Input { pid, input, result }
        });
    }

    fn set_display_name(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        let name = self.display_name.trim().to_owned();
        self.spawn(ctx, "Setting the name…", move |settings| {
            let result = backend::set_display_name(&target, &settings, &name);
            Event::Name { pid, name, result }
        });
    }

    fn read_mouse(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.spawn(ctx, "Reading…", move |settings| Event::Mouse {
            pid,
            result: backend::connection(&target, &settings),
        });
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            // Tiles run in the background, so they must not clear another job's busy label.
            if !matches!(event, Event::Tiles { .. }) {
                self.busy = None;
            }
            match event {
                Event::Scanned(scan) => {
                    self.targets = scan.targets;
                    self.dll_version = scan.dll_version;
                    for error in scan.errors {
                        self.error(error);
                    }
                    if !self.targets.iter().any(|t| Some(t.key()) == self.selected) {
                        self.selected = (self.targets.len() == 1).then(|| self.targets[0].key());
                    }
                }
                Event::Loaded { pid, result } => {
                    match result {
                        Ok(report) => {
                            self.report(pid, report);
                        }
                        Err(error) => self.error(format!("Loading into {pid} failed: {error}")),
                    }
                    self.scan(ctx);
                }
                Event::Unloaded { pid, result } => {
                    match result {
                        Ok(()) => self.info(format!("Unloaded agent from {pid}.")),
                        Err(error) => self.error(format!("Unloading agent from {pid} failed: {error}")),
                    }
                    self.scan(ctx);
                }
                Event::Tiles { pid, result } => {
                    if let Err(error) = result {
                        self.tiles = None;
                        for (on, layer) in self.dev_toggles.iter_mut().zip(DEV_LAYERS) {
                            if layer.is_some() {
                                *on = false;
                            }
                        }
                        self.error(format!("Scene tiles in {pid} failed: {error}"));
                    }
                }
                Event::Console { pid, result } => {
                    if let Err(error) = result {
                        self.error(format!("Opening the console of {pid} failed: {error}"));
                    }
                }
                Event::Cursor { pid, result } => {
                    if let Err(error) = result {
                        self.error(format!("Setting the cursor of {pid} failed: {error}"));
                    }
                }
                Event::Input { pid, input, result } => match result {
                    Ok(()) => self.info(format!("{pid}: sent {}.", describe(&input))),
                    Err(error) => self.error(format!("Sending input to {pid} failed: {error}")),
                },
                Event::Mouse { pid, result } => match result {
                    Ok(Some(c)) if c.mouse_x < 0 => self.info(format!("{pid}: agent hasn't seen the mouse move yet.")),
                    Ok(Some(c)) => self.info(format!("{pid}: Mouse at ({}, {}).", c.mouse_x, c.mouse_y)),
                    Ok(None) => self.error(format!("Reading the mouse of {pid} failed: agent isn't loaded.")),
                    Err(error) => self.error(format!("Reading the mouse of {pid} failed: {error}")),
                },
                Event::Name { pid, name, result } => match result {
                    Ok(DisplayName::Kept { real }) => self.info(format!(
                        "{pid}: showing \"{name}\" instead of {}, kept across logins. Text already on screen changes when the client redraws it (the chatbox's input line soon, the quest tab's summary after switching tabs).",
                        real.map_or_else(|| "the real name".to_owned(), |real| format!("\"{real}\""))
                    )),
                    Ok(DisplayName::Waiting) => {
                        self.info(format!("{pid}: \"{name}\" will show once that account is logged in."))
                    }
                    Ok(DisplayName::Off) => self.info(format!("{pid}: the real display name shows.")),
                    Err(error) => self.error(format!("Setting the display name in {pid} failed: {error}")),
                },
                Event::State { pid, read, result } => match result {
                    Ok(state) => self.report_state(pid, read, &state),
                    Err(error) => self.error(format!("Reading the game state of {pid} failed: {error}")),
                },
                Event::Varps { pid, read, result } => match result {
                    Ok(varps) => {
                        let previous = match &varps {
                            Some(varps) => self.varps.replace(varps.clone()),
                            None => self.varps.clone(),
                        };
                        self.report_varps(pid, read, varps.as_deref(), previous.as_deref());
                    }
                    Err(error) => self.error(format!("Reading the varps of {pid} failed: {error}")),
                },
            }
        }
    }

    fn report(&mut self, pid: u32, report: LoadReport) {
        if report.already_loaded {
            self.info(format!("This build of agent was already loaded in {pid}."));
        }
        if let Some(old) = &report.replaced {
            self.info(format!("Unloaded agent {old} from {pid}."));
        }
        match report.connection {
            Some(c) if !c.is_compatible() => self.error(format!(
                "agent {} in {pid} uses shared memory protocol {}, this loader uses {}. Load a matching agent.",
                c.version,
                c.protocol,
                injector::PROTOCOL_VERSION
            )),
            Some(c) if c.width > 0 => {
                self.info(format!("Connected to agent {} in {pid} ({}x{}).", c.version, c.width, c.height));
                self.report_offsets(pid, c.offsets_found);
            }
            Some(c) => {
                self.info(format!(
                    "Connected to agent {} in {pid}, waiting for the client to render a frame.",
                    c.version
                ));
                self.report_offsets(pid, c.offsets_found);
            }
            None => self.error(format!(
                "Loaded into {pid}, but agent's shared memory never appeared. It may have failed to start."
            )),
        }
    }

    fn report_state(&mut self, pid: u32, read: Read, state: &GameState) {
        let text = match (read, state.game_state, state.position) {
            (Read::GameState, Some(game_state), _) => {
                format!(
                    "Game state: {} ({game_state}).",
                    injector::game_state_name(game_state)
                )
            }
            (Read::GameState, None, _) => "Game state: unknown (offset not found).".into(),
            (Read::Position, _, Some(p)) => {
                let plane = p
                    .plane
                    .map_or_else(|| "?".into(), |plane| plane.to_string());
                format!(
                    "Position: tile ({}, {}, plane {plane}), local ({}, {}).",
                    p.x, p.y, p.local_x, p.local_y
                )
            }
            (Read::Chunk, _, Some(p)) => {
                let chunk = p.chunk();
                format!(
                    "Chunk: ({}, {}), region id {}.",
                    chunk.x,
                    chunk.y,
                    chunk.id()
                )
            }
            (Read::Position | Read::Chunk, _, None) => "No local player (logged out?).".into(),
            (Read::Skills, ..) => match &state.skills {
                Some(skills) => {
                    let total: i32 = skills.iter().map(|s| s.real_level).sum();
                    let lines: Vec<String> = skills
                        .iter()
                        .map(|s| format!("  {}: {}/{}, {} xp", s.name, s.level, s.real_level, s.xp))
                        .collect();
                    format!(
                        "Skills (current/real), total level {total}:\n{}",
                        lines.join("\n")
                    )
                }
                None => "No skills (logged out, or the offsets weren't found).".into(),
            },
            (Read::Login, ..) => match state.login {
                Some(l) => format!(
                    "Login screen {}: panel x {}, box x {}, center x {}, y {}.",
                    l.screen, l.panel_x, l.box_x, l.center_x, l.y
                ),
                None => "No login layout (not on the login screen, or the offsets weren't found)."
                    .into(),
            },
            (Read::RootInterface, ..) => match state.root_interface {
                Some(165) => "Top-level interface: 165 (lobby).".into(),
                Some(root) => format!("Top-level interface: {root}."),
                None => {
                    "No top-level interface (not logged in, or the offsets weren't found).".into()
                }
            },
            (Read::TickCount, ..) => match state.tick_count {
                Some(ticks) => format!("Tick count: {ticks} ({:.1} s).", ticks as f64 * 0.6),
                None => "Tick count: unknown (offset not found).".into(),
            },
            (Read::Varp(_) | Read::KnownVarps | Read::VarpChanges, ..) => return,
        };
        self.info(format!("{pid}: {text}"));
    }

    fn report_varps(
        &mut self,
        pid: u32,
        read: Read,
        varps: Option<&[i32]>,
        previous: Option<&[i32]>,
    ) {
        let Some(varps) = varps else {
            self.info(format!("{pid}: No varps (not logged in)."));
            return;
        };
        let varp = |id: usize| varps.get(id).copied();
        let text = match read {
            Read::Varp(id) => match varp(id) {
                Some(value) => format!("{}.", varp_text(None, id, value)),
                None => format!("Varp {id}: no such varp, the client has {}.", varps.len()),
            },
            Read::KnownVarps => {
                let lines: Vec<String> = KNOWN_VARPS
                    .iter()
                    .map(|&(name, id)| match varp(id) {
                        Some(value) => format!("  {}", varp_text(Some(name), id, value)),
                        None => format!("  {name} ({id}): missing"),
                    })
                    .collect();
                format!("Known varps:\n{}", lines.join("\n"))
            }
            Read::VarpChanges => match previous {
                Some(previous) => {
                    let changes: Vec<String> = varps
                        .iter()
                        .enumerate()
                        .filter(|&(id, value)| previous.get(id) != Some(value))
                        .map(|(id, value)| {
                            let name = KNOWN_VARPS
                                .iter()
                                .find(|&&(_, known)| known == id)
                                .map(|&(name, _)| name);
                            let before = previous.get(id).map_or("none".to_owned(), i32::to_string);
                            format!("  {} (was {before})", varp_text(name, id, *value))
                        })
                        .collect();
                    match changes.len() {
                        0 => "No varps changed since the previous read.".into(),
                        n => format!(
                            "{n} varps changed since the previous read:\n{}",
                            changes.join("\n")
                        ),
                    }
                }
                None => {
                    "Varps read; click again after changing something in game to see what changed."
                        .into()
                }
            },
            _ => return,
        };
        self.info(format!("{pid}: {text}"));
    }

    fn report_offsets(&mut self, pid: u32, found: Option<bool>) {
        match found {
            Some(true) => self.info(format!("Game state offsets found in {pid}.")),
            Some(false) => self.error(format!(
                "Game state offsets were not found in {pid}: this client build isn't supported yet."
            )),
            None => self.info(format!("Still looking for game state offsets in {pid}.")),
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("settings").num_columns(3).show(ui, |ui| {
            ui.label("agent DLL");
            ui.add(egui::TextEdit::singleline(&mut self.settings.dll).desired_width(460.0));
            if ui.button("Browse…").clicked() {
                pick_file(&mut self.settings.dll, "DLL", "dll");
            }
            ui.end_row();

            if !cfg!(windows) {
                ui.label("injector.exe");
                ui.add(egui::TextEdit::singleline(&mut self.settings.helper).desired_width(460.0));
                if ui.button("Browse…").clicked() {
                    pick_file(&mut self.settings.helper, "Executable", "exe");
                }
                ui.end_row();
            }
        });
    }

    fn clients_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        if ui
            .add_enabled(self.busy.is_none(), egui::Button::new("Refresh"))
            .clicked()
        {
            self.scan(&ctx);
        }

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Mouse cursor");
            egui::ComboBox::from_id_salt("cursor_style")
                .selected_text(cursor_style_label(self.cursor.style))
                .show_ui(ui, |ui| {
                    for style in CURSOR_STYLES {
                        changed |= ui.selectable_value(&mut self.cursor.style, style, cursor_style_label(style)).changed();
                    }
                });
            ui.add_enabled_ui(self.cursor.style != CursorStyle::Off, |ui| {
                changed |= ui.color_edit_button_srgb(&mut self.cursor.color).changed();
                ui.label("Size");
                changed |= ui.add(egui::DragValue::new(&mut self.cursor.size).range(4..=64).suffix(" px")).changed();
            });
        });
        if changed && let Some(target) = self.selected_target() {
            self.set_cursor(&ctx, target);
        }
        ui.add_space(6.0);
        if self.targets.is_empty() {
            if self.busy.is_none() {
                ui.label("No game clients found.");
            }
        } else {
            self.step_selected_client(&ctx);
            let show_prefix = !cfg!(windows);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    egui::Grid::new("clients")
                        .num_columns(6)
                        .striped(true)
                        .show(ui, |ui| {
                            for header in [
                                "",
                                "PID",
                                "Title",
                                "Executable",
                                if show_prefix { "Prefix" } else { "" },
                                "Status",
                            ] {
                                ui.strong(header);
                            }
                            ui.end_row();

                            for target in &self.targets {
                                let client = &target.client;
                                ui.radio_value(&mut self.selected, Some(target.key()), "");
                                ui.label(client.pid.to_string());
                                ui.label(&client.title);
                                ui.label(&client.exe);
                                ui.label(if show_prefix {
                                    target.location.label().unwrap_or_default()
                                } else {
                                    String::new()
                                });
                                if let Some(module) = &client.loaded {
                                    let version =
                                        client.version.as_deref().unwrap_or("unknown version");
                                    let text = if self
                                        .dll_version
                                        .as_deref()
                                        .is_some_and(|v| v != version)
                                    {
                                        format!("Loaded {version}, outdated")
                                    } else {
                                        format!("Loaded {version}")
                                    };
                                    ui.label(text).on_hover_text(module);
                                } else if !client.is_64bit {
                                    ui.label("32-bit, unsupported");
                                } else {
                                    ui.label("Not loaded");
                                }
                                ui.end_row();
                            }
                        });
                });
        }

        let selected = self.selected_target();
        let loaded = selected.as_ref().is_some_and(|t| t.client.loaded.is_some());
        let outdated = selected
            .as_ref()
            .is_some_and(|t| t.client.version != self.dll_version);
        let can_load = self.busy.is_none()
            && !self.settings.dll.is_empty()
            && selected.as_ref().is_some_and(|t| t.client.is_64bit)
            && (!loaded || outdated);
        let can_unload = self.busy.is_none() && loaded;

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let load = if loaded { "Reload" } else { "Load" };
            if ui
                .add_enabled(can_load, egui::Button::new(load))
                .on_hover_text("Loads the DLL from the settings, unloading another build first.")
                .clicked()
                && let Some(target) = selected.clone()
            {
                self.load(&ctx, target);
            }
            if ui.add_enabled(can_unload, egui::Button::new("Unload")).clicked()
                && let Some(target) = selected.clone()
            {
                self.unload(&ctx, target);
            }
            if ui
                .add_enabled(can_unload, egui::Button::new("Console"))
                .on_hover_text("Opens agent's console window in the client.")
                .clicked()
                && let Some(target) = selected.clone()
            {
                self.console(&ctx, target);
            }

            ui.separator();
            let hint = "Shown instead of your display name, in this client only, and kept across logins until reset. Empty puts the real name back.";
            let field = ui
                .add_enabled(
                    can_unload,
                    egui::TextEdit::singleline(&mut self.display_name)
                        .hint_text("Display name")
                        .char_limit(injector::state::MAX_NAME_BYTES)
                        .desired_width(140.0),
                )
                .on_hover_text(hint);
            let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let label = if self.display_name.trim().is_empty() { "Reset name" } else { "Set name" };
            if (ui.add_enabled(can_unload, egui::Button::new(label)).on_hover_text(hint).clicked() || submitted)
                && let Some(target) = selected
            {
                self.set_display_name(&ctx, target);
            }
        });
    }

    /// Moves the client selection with the up and down arrows, unless a text field has the keyboard.
    fn step_selected_client(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (up, down) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            )
        });
        if !up && !down {
            return;
        }
        let current = self.targets.iter().position(|t| Some(t.key()) == self.selected);
        let next = match (current, down) {
            (None, true) => 0,
            (None, false) => self.targets.len() - 1,
            (Some(i), true) => (i + 1).min(self.targets.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.selected = Some(self.targets[next].key());
    }

    fn selected_target(&self) -> Option<Target> {
        self.selected
            .as_ref()
            .and_then(|key| self.targets.iter().find(|t| &t.key() == key))
            .cloned()
    }

    fn loaded_target_ui(&self, ui: &mut egui::Ui) -> Option<Target> {
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        match &target {
            Some(target) => ui.label(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        ui.add_space(6.0);
        target
    }

    fn state_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let target = self.loaded_target_ui(ui);

        let enabled = self.busy.is_none() && target.is_some();
        ui.horizontal_wrapped(|ui| {
            for (label, read) in [
                ("Game state", Read::GameState),
                ("Position", Read::Position),
                ("Chunk", Read::Chunk),
                ("Skills", Read::Skills),
                ("Tick count", Read::TickCount),
                ("Login screen", Read::Login),
                ("Top-level interface", Read::RootInterface),
            ] {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked()
                    && let Some(target) = target.clone()
                {
                    self.read_state(&ctx, target, read);
                }
            }
        });
    }

    fn varps_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let target = self.loaded_target_ui(ui);
        let enabled = self.busy.is_none() && target.is_some();
        let mut read = None;

        ui.horizontal(|ui| {
            ui.label("Varp");
            ui.add(egui::DragValue::new(&mut self.varp).range(0..=injector::MAX_VARPS - 1));
            if ui.add_enabled(enabled, egui::Button::new("Read")).clicked() {
                read = Some(Read::Varp(self.varp));
            }
            ui.separator();
            if ui.add_enabled(enabled, egui::Button::new("Read all known")).clicked() {
                read = Some(Read::KnownVarps);
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Changes"))
                .on_hover_text("Lists every varp that changed since the previous read. Read, change something in game, then click this to find its varp.")
                .clicked()
            {
                read = Some(Read::VarpChanges);
            }
        });
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("known varps")
                    .num_columns(4)
                    .striped(true)
                    .show(ui, |ui| {
                        for header in ["Varp", "Id", "Last read", ""] {
                            ui.strong(header);
                        }
                        ui.end_row();
                        for &(name, id) in &KNOWN_VARPS {
                            ui.label(name);
                            ui.label(id.to_string());
                            match self.varps.as_ref().and_then(|varps| varps.get(id)) {
                                Some(&value) => ui.label(match describe_varp(id, value) {
                                    Some(text) => format!("{value} ({text})"),
                                    None => value.to_string(),
                                }),
                                None => ui.label("-"),
                            };
                            if ui.add_enabled(enabled, egui::Button::new("Read")).clicked() {
                                read = Some(Read::Varp(id));
                            }
                            ui.end_row();
                        }
                    });
            });

        if let (Some(read), Some(target)) = (read, target) {
            self.read_state(&ctx, target, read);
        }
    }

    fn handle_script_events(&mut self, ctx: &egui::Context) {
        if let Some(script) = &mut self.script {
            if script.stop_ignored(Duration::from_secs(2)) {
                self.script_error("The script's process didn't exit after being killed. It will end when the loader closes.");
            } else if script.is_running() && script.stop_requested() {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        let events = self.script.as_mut().map(Script::poll).unwrap_or_default();
        for event in events {
            match event {
                ScriptEvent::Output(line) => self.script_info(line),
                ScriptEvent::Form(spec) => {
                    let title = spec
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Script settings")
                        .to_owned();
                    self.form = Some(crate::form::Form::new(
                        spec,
                        self.remembered_forms.get(&title),
                    ));
                    ctx.request_repaint();
                }
                ScriptEvent::Finished { stopped: true, .. } => self.script_info("Script stopped."),
                ScriptEvent::Finished { result: Ok(()), .. } => self.script_info("Script finished."),
                ScriptEvent::Finished {
                    result: Err(error), ..
                } => self.script_error(format!("Script failed: {error}")),
            }
        }
    }

    fn form_ui(&mut self, ctx: &egui::Context) {
        if self.script.as_ref().is_none_or(|s| !s.is_running()) {
            self.form = None;
        }
        let Some(form) = &mut self.form else { return };
        let answer = match form.show(ctx) {
            crate::form::Outcome::Open => return,
            crate::form::Outcome::Submitted(values) => {
                if let serde_json::Value::Object(map) = &values {
                    self.remembered_forms
                        .insert(form.title.clone(), map.clone());
                }
                Some(values)
            }
            crate::form::Outcome::Cancelled => None,
        };
        self.form = None;
        if let Some(script) = &mut self.script
            && let Err(error) = script.answer_form(answer) {
                self.script_error(error);
            }
    }

    fn open_script(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Luau script", &["luau", "lua"]);
        if let Some(dir) = std::path::Path::new(&self.script_file.path)
            .parent()
            .filter(|d| d.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.script_file = ScriptFile {
                    path: path.display().to_string(),
                    disk: Some(text.clone()),
                    text,
                };
                self.info(format!("Opened {}.", path.display()));
            }
            Err(error) => self.error(format!("Cannot open {}: {error}", path.display())),
        }
    }

    /// The file's contents if they differ from what the editor last synced with.
    fn changed_on_disk(&self) -> Option<String> {
        let disk = std::fs::read_to_string(&self.script_file.path).ok()?;
        match &self.script_file.disk {
            Some(known) => (&disk != known).then_some(disk),
            None => (disk != self.script_file.text).then_some(disk),
        }
    }

    fn run_script(&mut self, ctx: &egui::Context, target: Option<&Target>) {
        let name = self.script_file.name();
        let path = Some(std::path::Path::new(&self.script_file.path)).filter(|p| p.is_file());
        match Script::start(ctx, &self.script_file.text, path, target, &self.settings) {
            Ok(script) => {
                self.script_info(format!("Running {name}…"));
                self.script = Some(script);
            }
            Err(error) => self.script_error(format!("Couldn't start the script: {error}")),
        }
    }

    fn reload_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(disk) = self.reload_prompt.clone() else { return };
        let name = self.script_file.name();
        let unsaved = self.script_file.disk.as_ref().is_some_and(|known| known != &self.script_file.text);
        let mut choice: Option<Option<bool>> = None;
        egui::Window::new("Script changed on disk")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("{name} changed on disk since it was loaded. Reload it before running?"));
                if unsaved {
                    ui.colored_label(ui.visuals().warn_fg_color, "Reloading discards your unsaved edits.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reload and run").clicked() {
                        choice = Some(Some(true));
                    }
                    if ui.button("Run editor version").clicked() {
                        choice = Some(Some(false));
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(None);
                    }
                });
            });
        let Some(choice) = choice else { return };
        self.reload_prompt = None;
        let Some(reload) = choice else { return };
        self.script_file.disk = Some(disk.clone());
        if reload {
            self.script_file.text = disk;
        }
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        self.run_script(ctx, target.as_ref());
    }

    fn save_script(&mut self) {
        let path = self.script_file.path.clone();
        match std::fs::write(&path, &self.script_file.text) {
            Ok(()) => {
                self.script_file.disk = Some(self.script_file.text.clone());
                self.info(format!("Saved {path}."));
            }
            Err(error) => self.error(format!("Cannot save {path}: {error}")),
        }
    }

    fn save_script_as(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Luau script", &["luau", "lua"])
            .set_file_name(self.script_file.name());
        if let Some(dir) = std::path::Path::new(&self.script_file.path)
            .parent()
            .filter(|d| d.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        self.script_file.path = path.display().to_string();
        self.save_script();
    }

    fn scripts_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.script.as_ref().is_some_and(Script::is_running);
        let paused = running && self.script.as_ref().is_some_and(Script::is_paused);

        let (find, replace) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::F),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::R),
            )
        });
        if find || replace {
            self.find.open = true;
            self.find.replace |= replace;
            self.find.focus = true;
        }

        let target = self.loaded_target_ui(ui);

        ui.horizontal(|ui| {
            if ui.add_enabled(!running, egui::Button::new("Open…")).clicked() {
                self.open_script();
            }
            if ui.add_enabled(!running && !self.script_file.path.is_empty(), egui::Button::new("Save")).clicked() {
                self.save_script();
            }
            if ui.add_enabled(!running, egui::Button::new("Save as…")).clicked() {
                self.save_script_as();
            }
            let can_run = !running && !self.script_file.text.trim().is_empty();
            if ui
                .add_enabled(can_run, egui::Button::new("Run"))
                .on_hover_text("Runs what's in the editor, against the client selected above if any.")
                .clicked()
            {
                match self.changed_on_disk() {
                    Some(disk) => self.reload_prompt = Some(disk),
                    None => self.run_script(ui.ctx(), target.as_ref()),
                }
            }
            if ui
                .add_enabled(running, egui::Button::new(if paused { "Resume" } else { "Pause" }))
                .on_hover_text("Suspends the script's process, so it takes effect immediately, even in a tight loop.")
                .clicked()
                && let Some(script) = &mut self.script
            {
                script.set_paused(!paused);
                self.script_info(if paused { "Script resumed." } else { "Script paused." });
            }
            if ui.add_enabled(running, egui::Button::new("Stop")).clicked()
                && let Some(script) = &mut self.script
            {
                script.stop();
            }
            ui.separator();
            let status = match (running, paused) {
                (true, true) => "paused",
                (true, false) => "running",
                (false, _) => "not running",
            };
            ui.label(format!("{}, {status}", self.script_file.name())).on_hover_text(&self.script_file.path);
        });
        ui.add_space(4.0);
        if self.find.open {
            self.find_ui(ui);
            ui.add_space(4.0);
        }

        let size = ui.available_size();
        let margin = egui::Margin::symmetric(4, 2);
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let (row_height, digit_width) = ui.fonts_mut(|fonts| (fonts.row_height(&font), fonts.glyph_width(&font, '0')));
        let lines = self.script_file.text.matches('\n').count() + 1;
        let gutter_width = digit_width * lines.to_string().len() as f32 + 3.0 * margin.leftf();
        let gutter_height = size.y.max(lines as f32 * row_height + margin.sum().y);
        let gutter_color = ui.visuals().weak_text_color();
        let matches = if self.find.open {
            find_matches(&self.script_file.text, &self.find.query, self.find.case_sensitive)
        } else {
            Vec::new()
        };
        let highlighter = &mut self.highlighter;
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = highlighter.layout(ui, text.as_str());
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let (gutter, _) = ui.allocate_exact_size(egui::vec2(gutter_width, gutter_height), egui::Sense::hover());
                    ui.painter().rect_filled(gutter, 0.0, ui.visuals().faint_bg_color);
                    let first = ((ui.clip_rect().top() - gutter.top() - margin.topf()) / row_height).floor().max(0.0) as usize;
                    let last = ((ui.clip_rect().bottom() - gutter.top() - margin.topf()) / row_height).ceil().max(0.0) as usize;
                    for line in first..last.min(lines) {
                        let y = gutter.top() + margin.topf() + line as f32 * row_height;
                        ui.painter().text(
                            egui::pos2(gutter.right() - 2.0 * margin.leftf(), y),
                            egui::Align2::RIGHT_TOP,
                            line + 1,
                            font.clone(),
                            gutter_color,
                        );
                    }
                    let output = egui::TextEdit::multiline(&mut self.script_file.text)
                        .code_editor()
                        .margin(margin)
                        .desired_width(f32::INFINITY)
                        .min_size(size)
                        .layouter(&mut layouter)
                        .show(ui);
                    if self.find.open {
                        paint_matches(ui, &output, &self.script_file.text, &matches, &mut self.find);
                    }
                });
            });
    }

    fn find_ui(&mut self, ui: &mut egui::Ui) {
        let matches = find_matches(&self.script_file.text, &self.find.query, self.find.case_sensitive);
        let count = matches.len();
        self.find.current = self.find.current.min(count.saturating_sub(1));

        // `Some(true)` steps back, `Some(false)` steps forward.
        let mut step = None;
        let mut replace_current = false;
        let mut replace_all = false;

        ui.horizontal(|ui| {
            let query = ui.add(egui::TextEdit::singleline(&mut self.find.query).hint_text("Find").desired_width(200.0));
            if std::mem::take(&mut self.find.focus) {
                query.request_focus();
            }
            if query.changed() {
                self.find.current = 0;
                self.find.jump = true;
            }
            if (query.has_focus() || query.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                step = Some(ui.input(|i| i.modifiers.shift));
            }
            if ui.add_enabled(count > 0, egui::Button::new("Previous")).on_hover_text("Shift+Enter").clicked() {
                step = Some(true);
            }
            if ui.add_enabled(count > 0, egui::Button::new("Next")).on_hover_text("Enter").clicked() {
                step = Some(false);
            }
            let status = if self.find.query.is_empty() {
                String::new()
            } else if count == 0 {
                "No matches".into()
            } else {
                format!("{} of {count}", self.find.current + 1)
            };
            ui.label(status);
            if ui.checkbox(&mut self.find.case_sensitive, "Match case").changed() {
                self.find.current = 0;
                self.find.jump = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").on_hover_text("Escape").clicked() {
                    self.find.open = false;
                }
            });
        });

        if self.find.replace {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.find.replacement).hint_text("Replace with").desired_width(200.0));
                if ui.add_enabled(count > 0, egui::Button::new("Replace")).clicked() {
                    replace_current = true;
                }
                if ui.add_enabled(count > 0, egui::Button::new("Replace all")).clicked() {
                    replace_all = true;
                }
            });
        }

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.find.open = false;
        }
        if let Some(back) = step.filter(|_| count > 0) {
            let current = self.find.current;
            self.find.current = if back { (current + count - 1) % count } else { (current + 1) % count };
            self.find.jump = true;
        }
        if replace_all {
            let query_len = self.find.query.len();
            let mut out = String::with_capacity(self.script_file.text.len());
            let mut last = 0;
            for &at in &matches {
                out.push_str(&self.script_file.text[last..at]);
                out.push_str(&self.find.replacement);
                last = at + query_len;
            }
            out.push_str(&self.script_file.text[last..]);
            self.script_file.text = out;
            self.find.current = 0;
        } else if let (true, Some(&at)) = (replace_current, matches.get(self.find.current)) {
            let replacement = self.find.replacement.clone();
            let text = &mut self.script_file.text;
            text.replace_range(at..at + self.find.query.len(), &replacement);
            // Land on the first match after the replacement, so repeated clicks walk through the rest.
            let after = at + replacement.len();
            let next = find_matches(text, &self.find.query, self.find.case_sensitive);
            self.find.current = next.iter().position(|&start| start >= after).unwrap_or(0);
            self.find.jump = true;
        }
    }

    fn input_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let target = self.loaded_target_ui(ui);
        let enabled = self.busy.is_none() && target.is_some();
        let mut send = None;

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .hint_text("Text to type")
                    .desired_width(300.0),
            );
            if ui
                .add_enabled(enabled && !self.text.is_empty(), egui::Button::new("Type"))
                .on_hover_text("Types the text without pressing enter.")
                .clicked()
            {
                send = Some(Input::Type {
                    text: self.text.clone(),
                    hold_ms: injector::KEY_HOLD_MS,
                });
            }
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let (x, y) = &mut self.mouse;
            ui.label("x");
            ui.add(egui::DragValue::new(x).range(0..=7680));
            ui.label("y");
            ui.add(egui::DragValue::new(y).range(0..=4320));
            let (x, y) = (*x, *y);
            if ui
                .add_enabled(enabled, egui::Button::new("Move mouse"))
                .clicked()
            {
                send = Some(Input::Move { x, y });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Left click"))
                .on_hover_text("Clicks at x, y.")
                .clicked()
            {
                send = Some(Input::Click {
                    button: MouseButton::Left,
                    x,
                    y,
                    hold_ms: injector::CLICK_HOLD_MS,
                });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Right click"))
                .on_hover_text("Clicks at x, y.")
                .clicked()
            {
                send = Some(Input::Click {
                    button: MouseButton::Right,
                    x,
                    y,
                    hold_ms: injector::CLICK_HOLD_MS,
                });
            }
        });
        ui.add_space(6.0);

        if ui
            .add_enabled(enabled, egui::Button::new("Mouse position"))
            .on_hover_text("Prints where agent last saw the mouse, in client coordinates.")
            .clicked()
            && let Some(target) = target.clone()
        {
            self.read_mouse(&ctx, target);
        }

        if let (Some(input), Some(target)) = (send, target) {
            self.send_input(&ctx, target, input);
        }
    }

    fn development_ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("development_toggles")
            .num_columns(DEV_COLUMNS)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                for (index, (label, on)) in DEV_TOGGLES
                    .iter()
                    .zip(self.dev_toggles.iter_mut())
                    .enumerate()
                {
                    ui.add_enabled(DEV_LAYERS[index].is_some(), egui::Checkbox::new(on, *label))
                        .on_disabled_hover_text("Not ported yet.");
                    if (index + 1) % DEV_COLUMNS == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn log_panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Loader, "Loader");
            crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Script, "Script");
            let log = match self.log_tab {
                LogTab::Script => &mut self.script_log,
                LogTab::Loader => &mut self.loader_log,
            };
            let mut filter = log.filter;
            egui::ComboBox::from_id_salt("log_tag")
                .selected_text(filter.map_or("All", |index| log.tags[index].as_str()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut filter, None, "All");
                    for (index, tag) in log.tags.iter().enumerate() {
                        ui.selectable_value(&mut filter, Some(index), tag);
                    }
                });
            log.set_filter(filter);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let log = match self.log_tab {
                    LogTab::Script => &mut self.script_log,
                    LogTab::Loader => &mut self.loader_log,
                };
                if ui
                    .add_enabled(!log.is_empty(), egui::Button::new("Clear"))
                    .on_hover_text("Clears the output of the open tab.")
                    .clicked()
                {
                    log.clear();
                }
                if ui
                    .add_enabled(!log.is_empty(), egui::Button::new("Copy all"))
                    .on_hover_text("Copies the shown output of the open tab.")
                    .clicked()
                {
                    ui.ctx().copy_text(log.text.clone());
                }
            });
        });
        ui.separator();
        self.log_ui(ui);
    }

    fn log_ui(&mut self, ui: &mut egui::Ui) {
        let log = match self.log_tab {
            LogTab::Script => &self.script_log,
            LogTab::Loader => &self.loader_log,
        };
        // One text widget for the whole output, so Ctrl+A and Ctrl+C cover every line.
        let text = log.text.as_str();
        let font = egui::TextStyle::Body.resolve(ui.style());
        let normal = ui.visuals().text_color();
        let error = ui.visuals().error_fg_color;
        let mut layouter = |ui: &egui::Ui, _: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = egui::text::LayoutJob::default();
            for (i, &index) in log.view.iter().enumerate() {
                let line = &log.lines[index];
                let color = if line.error { error } else { normal };
                let separator = if i > 0 { "\n" } else { "" };
                job.append(&format!("{separator}{}", line.text), 0.0, egui::TextFormat::simple(font.clone(), color));
            }
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let mut read_only = text;
                let output = egui::TextEdit::multiline(&mut read_only)
                    .font(egui::TextStyle::Body)
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY)
                    .layouter(&mut layouter)
                    .show(ui);

                // Rows of one galley map to text lines (a row that ends with a newline closes one); an entry can span several.
                let entry_of_line: Vec<usize> = log
                    .view
                    .iter()
                    .enumerate()
                    .flat_map(|(entry, &index)| std::iter::repeat_n(entry, log.lines[index].text.matches('\n').count() + 1))
                    .collect();
                let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|pos| output.response.rect.contains(*pos)) else {
                    return;
                };
                let origin = output.galley_pos.to_vec2();
                let mut paragraph = 0;
                let mut hovered = None;
                for row in &output.galley.rows {
                    let rect = row.rect().translate(origin);
                    if rect.y_range().contains(pointer.y) {
                        hovered = Some(paragraph);
                        break;
                    }
                    if row.ends_with_newline {
                        paragraph += 1;
                    }
                }
                let Some(entry) = hovered.and_then(|line| entry_of_line.get(line).copied()) else {
                    return;
                };
                // A wrapped line spans several rows: highlight them all, keep the button on the first.
                let mut index = 0;
                let mut first = None;
                let mut span = egui::Rect::NOTHING;
                for row in &output.galley.rows {
                    let rect = row.rect().translate(origin);
                    if entry_of_line.get(index) == Some(&entry) {
                        first.get_or_insert(rect);
                        span = span.union(rect);
                    }
                    if row.ends_with_newline {
                        index += 1;
                    }
                }
                let Some(rect) = first else {
                    return;
                };
                let band = egui::Rect::from_min_max(
                    egui::pos2(output.text_clip_rect.left(), span.top()),
                    egui::pos2(output.text_clip_rect.right(), span.bottom()),
                );
                ui.painter().rect_filled(band, 0.0, ui.visuals().text_color().gamma_multiply(0.08));
                let line_rect = egui::Rect::from_min_max(
                    egui::pos2(output.text_clip_rect.left(), rect.top()),
                    egui::pos2(output.text_clip_rect.right(), rect.bottom()),
                );
                let copy = ui.put(line_rect, |ui: &mut egui::Ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.small_button("Copy"))
                        .inner
                })
                .on_hover_cursor(egui::CursorIcon::PointingHand);
                if copy.clicked() {
                    ui.ctx().copy_text(log.lines[log.view[entry]].text.clone());
                }
            });
    }
}

/// Paints the find matches over the editor, and scrolls to the current one when asked.
fn paint_matches(ui: &egui::Ui, output: &egui::text_edit::TextEditOutput, text: &str, matches: &[usize], find: &mut Find) {
    let query_len = find.query.len();
    let origin = output.galley_pos.to_vec2();
    for (i, &at) in matches.iter().enumerate() {
        let start = output.galley.pos_from_cursor(egui::text::CCursor::new(text[..at].chars().count()));
        let end = output.galley.pos_from_cursor(egui::text::CCursor::new(text[..at + query_len].chars().count()));
        // A match split across wrapped rows is skipped rather than drawn wrong.
        if (start.top() - end.top()).abs() > 1.0 {
            continue;
        }
        let current = i == find.current;
        let rect = egui::Rect::from_min_max(start.min, egui::pos2(end.max.x, start.max.y)).translate(origin);
        let color = if current {
            egui::Color32::from_rgba_unmultiplied(255, 140, 0, 150)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 220, 0, 70)
        };
        ui.painter().rect_filled(rect, 2.0, color);
        if current && find.jump {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
        }
    }
    find.jump = false;
}

fn varp_text(name: Option<&str>, id: usize, value: i32) -> String {
    let mut text = match name.or_else(|| {
        KNOWN_VARPS
            .iter()
            .find(|&&(_, known)| known == id)
            .map(|&(name, _)| name)
    }) {
        Some(name) => format!("{name} ({id}): {value}"),
        None => format!("Varp {id}: {value}"),
    };
    if let Some(meaning) = describe_varp(id, value) {
        text.push_str(&format!(" ({meaning})"));
    }
    text
}

fn describe(input: &Input) -> String {
    match input {
        Input::Move { x, y } => format!("a mouse move to ({x}, {y})"),
        Input::WindMouse { x, y, .. } => format!("a WindMouse move to ({x}, {y})"),
        Input::Click { button, x, y, .. } => format!("a {} click at ({x}, {y})", button.name()),
        Input::Button { button, down } => format!(
            "a {} button {}",
            button.name(),
            if *down { "press" } else { "release" }
        ),
        Input::Scroll { notches } => format!("a scroll of {notches} notches"),
        Input::Key { key, down } => format!("key {key} {}", if *down { "down" } else { "up" }),
        Input::Type { text, .. } => format!("{:?}", text),
    }
}

fn pick_file(path: &mut String, name: &str, extension: &str) {
    let mut dialog = rfd::FileDialog::new().add_filter(name, &[extension]);
    if let Some(dir) = std::path::Path::new(path.as_str())
        .parent()
        .filter(|d| d.is_dir())
    {
        dialog = dialog.set_directory(dir);
    }
    if let Some(picked) = dialog.pick_file() {
        *path = picked.display().to_string();
    }
}

impl eframe::App for LoaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        self.handle_script_events(&ctx);
        self.form_ui(&ctx);
        self.reload_prompt_ui(&ctx);
        self.tiles_ui(&ctx);

        egui::Panel::top("settings").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../assets/volcren.svg")).fit_to_exact_size(egui::vec2(32.0, 32.0)));
                ui.label(egui::RichText::new("Volcren").size(22.0).strong());
            });
            ui.add_space(6.0);
        });
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(140.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                self.log_panel_ui(ui);
            });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.horizontal(|ui| {
                crate::theme::tab_value(ui, &mut self.tab, Tab::Clients, "Clients");
                crate::theme::tab_value(ui, &mut self.tab, Tab::State, "Game state");
                crate::theme::tab_value(ui, &mut self.tab, Tab::Varps, "Varps");
                crate::theme::tab_value(ui, &mut self.tab, Tab::Input, "Input");
                crate::theme::tab_value(ui, &mut self.tab, Tab::Scripts, "Scripts");
                crate::theme::tab_value(ui, &mut self.tab, Tab::Development, "Development");
                if let Some(label) = self.busy {
                    ui.separator();
                    ui.spinner();
                    ui.label(label);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::theme::tab_value(ui, &mut self.tab, Tab::Settings, "Settings");
                });
            });
            ui.separator();
            if self.tab != self.followed_tab {
                self.followed_tab = self.tab;
                self.log_tab = if self.tab == Tab::Scripts { LogTab::Script } else { LogTab::Loader };
            }
            match self.tab {
                Tab::Clients => self.clients_ui(ui),
                Tab::State => self.state_ui(ui),
                Tab::Varps => self.varps_ui(ui),
                Tab::Input => self.input_ui(ui),
                Tab::Scripts => self.scripts_ui(ui),
                Tab::Development => self.development_ui(ui),
                Tab::Settings => self.settings_ui(ui),
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        eframe::set_value(storage, SCRIPT_KEY, &self.script_file);
        eframe::set_value(storage, FORMS_KEY, &self.remembered_forms);
        eframe::set_value(storage, CURSOR_KEY, &self.cursor);
    }

    fn on_exit(&mut self) {
        if let Some(script) = &mut self.script {
            script.stop();
        }
        backend::shutdown();
    }
}
