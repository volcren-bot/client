use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Write as _,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use injector::{Cursor, CursorStyle, DisplayName, GameState, LoadReport, Overlay, varp};
use serde::{Deserialize, Serialize};

use loader::backend::{self, Scan, Settings, Target};

use crate::account::{self, Auth};
use crate::api::{
    Api, ApiError, Config, Heartbeat, Me, Redeemed, Removed, StoreScript,
    models::{API_VERSION, Ended, Quote, RecentSession, Scored},
};
use crate::feedback;
use crate::highlight::Highlighter;
use crate::libs::Lib;
use crate::pipe::Rows;
use crate::script::{Launch, Script, ScriptEvent};
use crate::session::{self, Kept, Prepared};
use crate::store::{self, Confirmed, Store};
use crate::theme::{icon_button, icon_button_width, slide_button};
use crate::update;

mod queue_page;

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
    Done {
        pid: u32,
        what: &'static str,
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
    XpState {
        run: u64,
        result: Result<GameState, String>,
    },
    AccountName(Result<Option<String>, String>),
    Name {
        pid: u32,
        name: String,
        result: Result<DisplayName, String>,
    },
    Tiles {
        pid: u32,
        result: Result<usize, String>,
    },
    Packets {
        pid: u32,
        result: Result<injector::PacketPage, String>,
    },
    PacketLayouts {
        pid: u32,
        result: Result<Option<Vec<injector::PacketLayout>>, String>,
    },
    ScriptNames(Result<crate::script_names::Names, String>),
    ScriptRuns {
        pid: u32,
        result: Result<crate::script_log::Poll, String>,
    },
    Vars {
        pid: u32,
        result: Result<crate::vars::Poll, String>,
    },
    Inventory {
        pid: u32,
        result: Result<crate::inventory::Poll, String>,
    },
    Icons {
        pictures: Vec<(crate::icons::Key, Option<injector::Image>)>,
        error: Option<String>,
    },
    LayoutRead {
        run: u64,
        kind: crate::layouts::Kind,
        result: Result<crate::layouts::Read, String>,
    },
    ItemsKnown(Result<Vec<(i32, String, bool)>, String>),
    Widgets {
        pid: u32,
        result: Result<crate::widgets::Poll, String>,
    },
    WidgetMouse {
        pid: u32,
        result: Result<Option<(i32, i32)>, String>,
    },
    Restored(Result<Option<account::Account>, ApiError>),
    SignedIn(Result<account::Account, String>),
    Me(Result<Me, ApiError>),
    Scripts(Result<Vec<StoreScript>, ApiError>),
    Redeemed(Result<Redeemed, ApiError>),
    GrantRemoved {
        id: String,
        result: Result<Removed, ApiError>,
    },
    StoreStarted {
        pid: u32,
        script: Box<StoreScript>,
        replaces: Option<u64>,
        run: Option<u64>,
        launch: Box<Launch>,
        result: Result<Prepared, String>,
    },
    RunQuoted {
        script: Box<StoreScript>,
        target: Box<Target>,
        run: Option<u64>,
        values: serde_json::Value,
        result: Result<session::Quoted, ApiError>,
    },
    ManifestFetched {
        script: Box<StoreScript>,
        target: Box<Target>,
        result: Result<crate::api::models::ScriptManifest, ApiError>,
    },
    QueueManifest {
        script_id: String,
        result: Result<crate::api::models::ScriptManifest, ApiError>,
    },
    QueueChecked {
        pid: u32,
        entry: bool,
        result: Result<Box<queue_page::Check>, ApiError>,
    },
    RestartQuoted {
        run: u64,
        result: Result<Option<(StoreScript, session::Quoted)>, ApiError>,
    },
    Heartbeat {
        session_id: String,
        beat: Heartbeat,
    },
    SessionStopped {
        session_id: String,
        reason: String,
        sign_out: bool,
    },
    SessionEnded(Result<Ended, ApiError>),
    Update(Result<Option<update::Available>, update::CheckError>),
    UpdatePrepared {
        progress: Arc<update::Progress>,
        result: Result<update::Prepared, String>,
    },
    RecentSessions(Result<Vec<RecentSession>, ApiError>),
    Scored {
        session_id: String,
        result: Result<Scored, ApiError>,
    },
    Reported {
        session_id: String,
        result: Result<(), String>,
    },
    RunKept(Result<(), String>),
}

struct StoreRun {
    session_id: String,
    script_id: String,
    community: bool,
    closed: Receiver<()>,
    lib_version: String,
    price: i64,
    rank: i64,
    fee: i64,
    fee_charged: i64,
}

impl StoreRun {
    fn rate_per_hour(&self, minutes_per_month: i64) -> f64 {
        self.price as f64 + crate::api::models::fee_per_hour(self.fee, minutes_per_month)
    }
}

enum UpdateState {
    Offered,
    Downloading(Arc<update::Progress>),
    Installing,
    Failed(String),
}

fn progress_text(progress: &update::Progress) -> String {
    const MIB: u64 = 1024 * 1024;
    let done = progress.done.load(Ordering::Relaxed) / MIB;
    match progress.total.load(Ordering::Relaxed) / MIB {
        0 => format!("Downloading: {done} MB"),
        total => format!("Downloading: {done} of {total} MB"),
    }
}

#[derive(Clone)]
enum Source {
    File(ScriptFile),
    Store(StoreScript),
}

struct Run {
    id: u64,
    title: String,
    script: Option<Script>,
    source: Option<Source>,
    target: Option<Target>,
    store: Option<StoreRun>,
    file: Option<ScriptFile>,
    form: Option<crate::form::Form>,
    form_sent: Option<serde_json::Value>,
    last_breaks: Option<(injector::form::BreakPlan, Instant)>,
    exit: Option<injector::form::Exit>,
    finished: Option<(Result<(), String>, bool)>,
    antiban_sent: injector::antiban::Config,
    log: Log,
    xp: crate::xp::XpTracker,
    breaks: crate::breaks::BreaksTracker,
    counters: crate::counters::CountersTracker,
    started: Instant,
    ended: Option<Instant>,
    tools_tab: ToolsTab,
    names: Vec<String>,
    xp_next: Instant,
    xp_reading: bool,
    editors: crate::layouts::Editors,
}

impl Run {
    fn new(
        id: u64,
        title: String,
        script: Option<Script>,
        target: Option<Target>,
        store: Option<StoreRun>,
        settings: &Settings,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            title,
            script,
            source: None,
            target,
            store,
            file: None,
            form: None,
            form_sent: None,
            last_breaks: None,
            exit: None,
            finished: None,
            antiban_sent: settings.antiban.clone(),
            log: Log::new(settings.log_lines),
            xp: Default::default(),
            breaks: Default::default(),
            counters: Default::default(),
            started: now,
            ended: None,
            tools_tab: ToolsTab::Settings,
            names: Vec::new(),
            xp_next: now,
            xp_reading: false,
            editors: Default::default(),
        }
    }

    fn is_running(&self) -> bool {
        self.script.as_ref().is_some_and(Script::is_running)
    }

    fn is_configuring(&self) -> bool {
        self.script.is_none()
    }

    fn stop(&mut self) {
        if let Some(script) = self.script.as_mut() {
            script.stop();
        }
    }

    fn stop_requested(&self) -> bool {
        self.script.as_ref().is_some_and(Script::stop_requested)
    }

    fn is_paused(&self) -> bool {
        self.script.as_ref().is_some_and(Script::is_paused)
    }

    fn send(&self, line: impl FnOnce(&Script) -> Result<(), String>) -> Result<(), String> {
        self.script
            .as_ref()
            .map_or_else(|| Err("The script isn't running yet.".into()), line)
    }

    fn run_time(&self) -> Duration {
        self.ended
            .unwrap_or_else(Instant::now)
            .saturating_duration_since(self.started)
    }

    fn loaded_target(&self) -> Option<&Target> {
        self.target.as_ref().filter(|t| t.client.loaded.is_some())
    }

    fn session_id(&self) -> Option<&str> {
        self.store.as_ref().map(|s| s.session_id.as_str())
    }

    fn info(&mut self, text: impl Into<String>) {
        self.log.push(false, text.into());
    }

    fn error(&mut self, text: impl Into<String>) {
        self.log.push(true, text.into());
    }

    fn report(&mut self, result: Result<(), String>, done: &str) {
        match result {
            Ok(()) => self.info(done),
            Err(error) => self.error(error),
        }
    }

    fn state(&self) -> RunState {
        if self.is_configuring() {
            RunState::Waiting
        } else if !self.is_running() {
            RunState::Ended
        } else if self.stop_requested() {
            RunState::Stopping
        } else if self.is_paused() {
            RunState::Paused
        } else {
            RunState::Running
        }
    }
}

#[derive(Clone, Copy)]
enum RunState {
    Running,
    Waiting,
    Paused,
    Stopping,
    Ended,
}

impl RunState {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Ended => "ended",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Running => crate::theme::GREEN,
            Self::Waiting => crate::theme::BLUE,
            Self::Paused => crate::theme::YELLOW,
            Self::Stopping => crate::theme::RED,
            Self::Ended => crate::theme::COMMENT,
        }
    }
}

const EXTRA_SESSION_FEE_HINT: &str = "Each script runs on its own client. Every script past the oldest pays the extra-session fee; the oldest runs without one.";

fn restart_button(ui: &mut egui::Ui, width: f32, store: bool, refusal: Option<&str>) -> bool {
    let button = icon_button(ui, ICON_RESTART, "Restart", width, refusal.is_none()).on_hover_text(
        if store {
            "Closes this session and starts the script again on the same client."
        } else {
            "Runs the file again on the same client, asking first if it changed on disk."
        },
    );
    match refusal {
        Some(why) => button.on_disabled_hover_text(why).clicked(),
        None => button.clicked(),
    }
}

fn decision_window(
    ctx: &egui::Context,
    title: &str,
    width: f32,
    confirm: &str,
    body: impl FnOnce(&mut egui::Ui),
) -> Option<bool> {
    let mut decision = None;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_max_width(width);
            body(ui);
            ui.horizontal(|ui| {
                if ui.button(confirm).clicked() {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });
    decision
}

fn client_caption(ui: &mut egui::Ui, pid: Option<u32>) {
    match pid {
        Some(pid) => {
            ui.weak(format!("Client {pid}."));
        }
        None => {
            ui.label("Select a client with agent loaded in the Clients tab.");
        }
    }
}

fn divider(ui: &mut egui::Ui) {
    let (line, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(
        line,
        0.0,
        ui.visuals().widgets.noninteractive.bg_stroke.color,
    );
}

fn fit_row(ui: &mut egui::Ui, fits: bool, add: impl FnOnce(&mut egui::Ui)) {
    if fits {
        ui.horizontal(add);
    } else {
        ui.vertical(add);
    }
}

fn start_dir(path: &str) -> Option<&std::path::Path> {
    std::path::Path::new(path).parent().filter(|d| d.is_dir())
}

pub(crate) const ICON_START: &str = "\u{f04b}";
const ICON_PAUSE: &str = "\u{f04c}";
const ICON_STOP: &str = "\u{f04d}";
const ICON_CANCEL: &str = "\u{f00d}";
const ICON_RESTART: &str = "\u{f01e}";
const ICON_UPDATE: &str = "\u{f021}";
const ICON_EXPAND: &str = "\u{f077}";
const ICON_COLLAPSE: &str = "\u{f078}";
const ICON_CLEAR: &str = "\u{f1f8}";
const ICON_COPY: &str = "\u{f0c5}";
const ICON_SAVE: &str = "\u{f0c7}";
const ICON_SAVE_AS: &str = "\u{f0cfb}";
const ICON_OPEN: &str = "\u{f07c}";
const ICON_OPEN_LAST: &str = "\u{f1da}";
const ICON_PIPE: &str = "\u{f07e5}";
const ICON_OPEN_RUN: &str = "\u{f08e}";
const ICON_LOAD: &str = "\u{f1e6}";
const ICON_UNLOAD: &str = "\u{f052}";
const ICON_CONSOLE: &str = "\u{f120}";
const ICON_SIGN_OUT: &str = "\u{f08b}";

const PAUSE_BUTTONS: [(&str, &str); 2] = [(ICON_PAUSE, "Pause"), (ICON_START, "Resume")];

fn run_pause_label(paused: bool) -> (&'static str, &'static str) {
    PAUSE_BUTTONS[usize::from(paused)]
}

const RESTORING_SIGN_IN: &str = "Checking for a saved sign-in…";
const WAITING_FOR_BROWSER: &str = "Waiting for you to sign in in the browser…";

const SCRIPT_FILTER: &[&str] = &["luau", "lua"];

struct PendingStart {
    script: StoreScript,
    target: Target,
    replaces: Option<u64>,
    lib: Lib,
    run: Option<u64>,
    launch: Launch,
}

struct StartPrompt {
    start: PendingStart,
    price: i64,
    fee: i64,
    nth: i64,
    old_price: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Clients,
    Store,
    Sessions,
    Queue,
    Run,
    Scripts,
    Development,
    Terminal,
    Accounts,
    Feedback,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DevTab {
    Tools,
    Inspectors,
}

impl DevTab {
    const ALL: [(DevTab, &'static str); 2] =
        [(DevTab::Tools, "Tools"), (DevTab::Inspectors, "Inspectors")];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Widget,
    Var,
    Script,
    Inventory,
    Packet,
}

impl InspectorTab {
    const ALL: [(InspectorTab, &'static str); 5] = [
        (InspectorTab::Widget, "Widget"),
        (InspectorTab::Var, "Var"),
        (InspectorTab::Script, "Script"),
        (InspectorTab::Inventory, "Inventory"),
        (InspectorTab::Packet, "Packet"),
    ];
}

fn sidebar_name(title: &str) -> String {
    const MAX: usize = 16;
    let name = title.to_lowercase();
    if name.chars().count() <= MAX {
        return name;
    }
    name.chars().take(MAX - 1).chain(['\u{2026}']).collect()
}

fn open_site(site_url: &str, path: &str) {
    if !site_url.is_empty() {
        let _ = webbrowser::open(&format!("{site_url}{path}"));
    }
}

fn sidebar_tree(
    ui: &mut egui::Ui,
    tab: Tab,
    active_run: Option<u64>,
    runs: &[(u64, String, bool)],
    site_url: &str,
) -> Option<(Tab, Option<u64>)> {
    ui.spacing_mut().item_spacing.y = 0.0;
    crate::theme::sidebar_header(ui, "explorer");
    if crate::theme::sidebar_root(ui, "\u{f233}", "volcren", !site_url.is_empty()).clicked() {
        open_site(site_url, "");
    }
    let items = [
        (Tab::Clients, "\u{f108}", "client"),
        (Tab::Store, "\u{f489}", "scripts"),
        (Tab::Scripts, "\u{f044}", "editor"),
        (Tab::Sessions, "\u{f04b}", "sessions"),
        (Tab::Queue, "\u{f0cb}", "queue"),
        (Tab::Development, "\u{f188}", "development"),
        (Tab::Terminal, "\u{f120}", "terminal"),
        (Tab::Accounts, "\u{f007}", "accounts"),
        (Tab::Feedback, "\u{f075}", "feedback"),
        (Tab::Settings, "\u{f013}", "settings"),
    ];
    let mut current = if tab == Tab::Run {
        (Tab::Run, active_run)
    } else {
        (tab, None)
    };
    let before = current;
    for (i, (tab, icon, name)) in items.iter().enumerate() {
        crate::theme::sidebar_item(
            ui,
            &mut current,
            (*tab, None),
            icon,
            name,
            i == items.len() - 1,
        );
        if *tab == Tab::Sessions {
            for (j, (id, name, ended)) in runs.iter().enumerate() {
                crate::theme::sidebar_child(
                    ui,
                    &mut current,
                    (Tab::Run, Some(*id)),
                    name,
                    *ended,
                    j == runs.len() - 1,
                );
            }
        }
    }
    (current != before).then_some(current)
}

const SIDEBAR_MIN_WINDOW: f32 = 760.0;
const SIDEBAR_WIDTH: f32 = 190.0;
const HEADER_COMPACT_WINDOW: f32 = 640.0;
const HEADER_STACK_WINDOW: f32 = 400.0;
const HEADER_TINY_WINDOW: f32 = 200.0;
const SIDEBAR_HANDLE_WIDTH: f32 = 18.0;
const CLIENT_PREFIX_WIDTH: f32 = 130.0;
const CLIENT_STATUS_WIDTH: f32 = 220.0;

fn centered_row(
    ui: &mut egui::Ui,
    id: &str,
    layout: egui::Layout,
    height: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let id = ui.id().with(id);
    let known: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    let available = ui.available_width();
    let offset = ((available - known) / 2.0).max(0.0);
    let spacing = ui.spacing().item_spacing.x;
    let measured = ui
        .allocate_ui_with_layout(egui::vec2(available, height), layout, |ui| {
            ui.add_space(offset);
            add(ui);
            (ui.min_rect().width() - offset - spacing).max(0.0)
        })
        .inner;
    if (measured - known).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(id, measured));
        ui.ctx().request_repaint();
    }
}

fn client_cell<R>(
    ui: &mut egui::Ui,
    width: f32,
    top: f32,
    spacing: egui::Vec2,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = spacing;
            ui.add_space(top);
            add(ui)
        },
    )
}

#[derive(Clone, Copy)]
enum ClientAction {
    Load,
    Unload,
    Console,
}

const DEV_TOGGLES: [&str; 23] = [
    "Players",
    "NPCs",
    "Ground Items",
    "Ground Objects",
    "Game Objects",
    "Grid",
    "Spot Animations",
    "Walls",
    "Decorations",
    "Projectiles",
    "Location",
    "Tile Location",
    "Camera Position",
    "Zone Borders",
    "Map Squares",
    "Loading Lines",
    "Line Of Sight",
    "Valid Movement",
    "Movement Flags",
    "Interacting",
    "Sound Effects",
    "Tile flags",
    "World Entities",
];

const DEV_COLUMNS: usize = 4;

const CURSOR_STYLES: [CursorStyle; 4] = [
    CursorStyle::Off,
    CursorStyle::Cross,
    CursorStyle::X,
    CursorStyle::Dot,
];

pub(crate) const DEFAULT_CURSOR: Cursor = Cursor {
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

const DEV_LAYERS: [Option<&str>; DEV_TOGGLES.len()] = [
    Some("players"),
    Some("npcs"),
    Some("ground-items"),
    Some("ground-objects"),
    Some("objects"),
    Some("grid"),
    Some("spot-anims"),
    Some("walls"),
    Some("decorations"),
    Some("projectiles"),
    Some("location"),
    Some("tile-location"),
    Some("camera"),
    Some("zone-borders"),
    Some("map-squares"),
    Some("loading-lines"),
    Some("line-of-sight"),
    Some("valid-movement"),
    Some("movement-flags"),
    Some("interacting"),
    None,
    Some("tile-flags"),
    Some("world-entities"),
];
const TILES_GIVE_UP: Duration = Duration::from_secs(10);
const TILES_RETRY: Duration = Duration::from_millis(50);

struct TilesWorker {
    pid: u32,
    layers: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TilesWorker {
    fn start(
        ctx: &egui::Context,
        tx: Sender<Event>,
        target: Target,
        settings: Settings,
        layers: Vec<String>,
    ) -> Self {
        let pid = target.client.pid;
        let mut worker = Self {
            pid,
            layers: Arc::new(Mutex::new(layers)),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        };
        let (layers, stop, ctx) = (worker.layers.clone(), worker.stop.clone(), ctx.clone());
        worker.thread = Some(thread::spawn(move || {
            let mut frame = 0;
            let mut failing: Option<Instant> = None;
            while !stop.load(Ordering::Relaxed) {
                let current = layers.lock().map(|l| l.clone()).unwrap_or_default();
                match backend::scene_tiles_next_frame(
                    &target,
                    &settings,
                    &current.iter().map(String::as_str).collect::<Vec<_>>(),
                    frame,
                ) {
                    Ok(Some(painted)) => {
                        frame = painted;
                        failing = None;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let since = *failing.get_or_insert_with(|| {
                            let _ = backend::draw(&target, &settings, &Overlay::Clear);
                            Instant::now()
                        });
                        if since.elapsed() > TILES_GIVE_UP {
                            let _ = tx.send(Event::Tiles {
                                pid,
                                result: Err(error),
                            });
                            ctx.request_repaint();
                            break;
                        }
                        thread::sleep(TILES_RETRY);
                    }
                }
            }
            let _ = backend::draw(&target, &settings, &Overlay::Clear);
        }));
        worker
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TilesWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const SCRIPT_KEY: &str = "script";
const LAST_SCRIPT_KEY: &str = "last_script";
const FORMS_KEY: &str = "forms";
const LAYOUTS_KEY: &str = "layouts";
pub(crate) const CURSOR_KEY: &str = "cursor";
const UI_FONT_KEY: &str = "ui_font";
const CONFIRMED_KEY: &str = "community_confirmed";
pub(crate) const HTTP_WHITELIST_KEY: &str = "http_whitelist";

const XP_READ_EVERY: Duration = Duration::from_secs(5);

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

#[derive(Default)]
struct Find {
    open: bool,
    replace: bool,
    query: String,
    replacement: String,
    case_sensitive: bool,
    current: usize,
    jump: bool,
    focus: bool,
}

fn find_matches(text: &str, query: &str, case_sensitive: bool) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    if case_sensitive {
        return text.match_indices(query).map(|(at, _)| at).collect();
    }
    let (text, query) = (text.as_bytes(), query.as_bytes());
    let mut found = Vec::new();
    let mut at = 0;
    while at + query.len() <= text.len() {
        if text[at..at + query.len()].eq_ignore_ascii_case(query) {
            found.push(at);
            at += query.len();
        } else {
            at += 1;
        }
    }
    found
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
#[derive(Clone)]
struct ScriptFile {
    path: String,
    text: String,
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

    fn changed_on_disk(&self) -> Option<String> {
        let disk = std::fs::read_to_string(&self.path).ok()?;
        match &self.disk {
            Some(known) => (&disk != known).then_some(disk),
            None => (disk != self.text).then_some(disk),
        }
    }
}

struct ReloadPrompt {
    disk: String,
    restart: Option<u64>,
}

fn default_script_path() -> Option<std::path::PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let sibling =
        update::is_dev_install().then(|| exe_dir.join("../../../scripts/scripts/default.luau"));
    std::iter::once(exe_dir.join("scripts/default.luau"))
        .chain(sibling)
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

const ACTIVE_PRAYERS: usize = 83;

const KNOWN_VARPS: [(&str, usize); 11] = [
    ("Attack style", varp::ATTACK_STYLE),
    ("Active prayers", ACTIVE_PRAYERS),
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

struct LogTable<'a> {
    id: &'a str,
    follow: bool,
    row_height: f32,
    max_height: Option<f32>,
    clickable: bool,
}

impl LogTable<'_> {
    fn show(
        &self,
        ui: &mut egui::Ui,
        columns: &[(&str, egui_extras::Column)],
        rows: usize,
        row: impl FnMut(egui_extras::TableRow<'_, '_>),
    ) {
        ui.scope(|ui| {
            ui.style_mut().interaction.selectable_labels = !self.clickable;
            let mut table = egui_extras::TableBuilder::new(ui)
                .id_salt(self.id)
                .striped(true)
                .resizable(true)
                .stick_to_bottom(self.follow)
                .auto_shrink([false, false])
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            if let Some(height) = self.max_height {
                table = table.max_scroll_height(height);
            }
            if self.clickable {
                table = table.sense(egui::Sense::click());
            }
            for (_, column) in columns {
                table = table.column(*column);
            }
            table
                .header(self.row_height, |header| {
                    crate::theme::table_header(header, columns.iter().map(|(title, _)| *title));
                })
                .body(|body| body.rows(self.row_height, rows, row));
        });
    }
}

fn narrow(width: f32) -> egui_extras::Column {
    egui_extras::Column::initial(width)
        .at_least(24.0)
        .clip(true)
}

fn paired<K: Copy, V, C: FromIterator<(K, V)>>(ids: &[K], values: Vec<Option<V>>) -> C {
    ids.iter()
        .zip(values)
        .filter_map(|(&id, value)| Some((id, value?)))
        .collect()
}

fn item_info<N, S>(target: &Target, settings: &Settings, ids: &[i32]) -> Result<(N, S), String>
where
    N: FromIterator<(i32, String)>,
    S: FromIterator<(i32, bool)>,
{
    let names = backend::item_names(target, settings, ids)?;
    let stacks = backend::item_stackable(target, settings, ids)?;
    Ok((paired(ids, names), paired(ids, stacks)))
}

fn rest(min: f32) -> egui_extras::Column {
    egui_extras::Column::remainder().at_least(min).clip(true)
}

fn mono_row(ui: &egui::Ui) -> (egui::FontId, f32) {
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().item_spacing.y;
    (font, row_height)
}

fn missed(dropped: impl Into<u64>) -> String {
    let dropped = dropped.into();
    if dropped > 0 {
        format!(", {dropped} missed")
    } else {
        String::new()
    }
}

fn error_label(ui: &mut egui::Ui, error: impl Into<egui::RichText>) {
    ui.colored_label(ui.visuals().error_fg_color, error);
}

fn packet_names_menu(ui: &mut egui::Ui, monitor: &mut crate::packets::Monitor) {
    use crate::packets::{NameState, UNKNOWN, name_state, with_name};

    let active = crate::packets::named_words(&monitor.filter, monitor.table())
        + usize::from(crate::packets::name_state(&monitor.filter, UNKNOWN) != NameState::Off);
    let title = if active > 0 {
        format!("Names ({active}) ⏷")
    } else {
        "Names ⏷".to_owned()
    };
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(title)
        .config(config)
        .ui(ui, |ui| {
            ui.set_width(300.0);
            ui.add(
                egui::TextEdit::singleline(&mut monitor.name_search)
                    .desired_width(f32::INFINITY)
                    .hint_text("🔍 Search names"),
            );
            ui.add_space(2.0);
            ui.weak("Click a name: show only → hide → off.");
            ui.separator();

            let search = monitor.name_search.trim().to_ascii_lowercase();
            let unknown_matches = UNKNOWN.to_ascii_lowercase().contains(&search);
            let mut rows: Vec<(bool, u16, String)> = monitor
                .table()
                .named()
                .map(|(sent, opcode, name)| (sent, opcode, name.to_owned()))
                .filter(|(_, _, name)| name.to_ascii_lowercase().contains(&search))
                .collect();
            rows.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
            let mut clicked = None;
            let mut row = |ui: &mut egui::Ui, name: &str, detail: String| {
                let state = name_state(&monitor.filter, name);
                let (glyph, color) = match state {
                    NameState::Off => ("", ui.visuals().weak_text_color()),
                    NameState::Only => ("✔", ui.visuals().hyperlink_color),
                    NameState::Hidden => ("✖", ui.visuals().error_fg_color),
                };
                let response = ui
                    .horizontal(|ui| {
                        ui.add_sized(
                            [20.0, 18.0],
                            egui::Label::new(egui::RichText::new(glyph).color(color).strong()),
                        );
                        let text = egui::RichText::new(name).monospace();
                        let text = if state == NameState::Off {
                            text
                        } else {
                            text.color(color)
                        };
                        ui.label(text);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(detail)
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(
                        response.rect,
                        3.0,
                        ui.visuals().widgets.hovered.weak_bg_fill,
                    );
                }
                if response.clicked() {
                    clicked = Some((name.to_owned(), state.next()));
                }
            };
            if unknown_matches {
                row(ui, UNKNOWN, "no name".to_owned());
                ui.separator();
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if rows.is_empty() && !unknown_matches {
                        ui.weak("No name matches.");
                    }
                    for (sent, opcode, name) in &rows {
                        row(
                            ui,
                            name,
                            format!("{} {opcode}", if *sent { "→" } else { "←" }),
                        );
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !monitor.filter.is_empty(),
                        egui::Button::new("Clear filter"),
                    )
                    .clicked()
                {
                    monitor.filter.clear();
                }
                ui.weak(format!("{} named", monitor.table().named().count()));
            });
            if let Some((name, state)) = clicked {
                monitor.filter = with_name(&monitor.filter, &name, state);
            }
        });
}

fn mono(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).monospace()
}

fn known_varp(id: usize) -> Option<&'static str> {
    KNOWN_VARPS
        .iter()
        .find(|&&(_, known)| known == id)
        .map(|&(name, _)| name)
}

fn describe_varp(id: usize, value: i32) -> Option<String> {
    Some(match id {
        varp::RUN | varp::SPECIAL_ATTACK_ENABLED => (if value == 1 { "on" } else { "off" }).into(),
        varp::SPECIAL_ATTACK => format!("{}%", value / 10),
        varp::POISON if value >= 1_000_000 => "envenomed".into(),
        varp::POISON if value > 0 => "poisoned".into(),
        varp::POISON => "healthy".into(),
        ACTIVE_PRAYERS => format!("bits {value:#b}"),
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

impl Read {
    fn is_varp(self) -> bool {
        matches!(self, Read::Varp(_) | Read::KnownVarps | Read::VarpChanges)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Normal,
    Warning,
    Error,
}

struct LogLine {
    tone: Tone,
    text: String,
    tag: Option<usize>,
}

#[derive(Default)]
struct Log {
    lines: Vec<LogLine>,
    tags: Vec<String>,
    tag_index: HashMap<String, usize>,
    filter: Option<usize>,
    view: Vec<usize>,
    text: String,
    revision: u64,
    keep: usize,
    pipe: Option<crate::pipe::Pipe>,
}

impl Log {
    fn new(keep: usize) -> Self {
        Self {
            keep,
            ..Self::default()
        }
    }

    fn set_keep(&mut self, keep: usize) {
        self.keep = keep;
        if self.lines.len() > keep {
            self.lines.drain(..self.lines.len() - keep);
            self.rebuild();
        }
    }

    fn push(&mut self, error: bool, text: String) {
        self.push_tone(if error { Tone::Error } else { Tone::Normal }, text);
    }

    fn push_tone(&mut self, tone: Tone, text: String) {
        let tag = line_tag(&text).map(|name| match self.tag_index.get(name) {
            Some(&index) => index,
            None => {
                let index = self.tags.len();
                self.tags.push(name.to_owned());
                self.tag_index.insert(name.to_owned(), index);
                index
            }
        });
        if self.filter.is_none() || self.filter == tag {
            self.show(self.lines.len(), &text);
        }
        let piped = crate::pipe::write(&mut self.pipe, [text.as_str()]);
        self.lines.push(LogLine { tone, text, tag });
        if self.lines.len() > self.keep + self.keep / 4 {
            self.lines.drain(..self.lines.len() - self.keep);
            self.rebuild();
        }
        if let Err(error) = piped {
            self.push_tone(Tone::Error, error);
        }
    }

    fn show(&mut self, index: usize, text: &str) {
        if !self.view.is_empty() {
            self.text.push('\n');
        }
        self.text.push_str(text);
        self.view.push(index);
        self.revision += 1;
    }

    fn last(&self) -> Option<&LogLine> {
        self.view.last().map(|&index| &self.lines[index])
    }

    fn texts(&self) -> Vec<String> {
        self.lines.iter().map(|line| line.text.clone()).collect()
    }

    fn set_filter(&mut self, filter: Option<usize>) {
        if self.filter == filter {
            return;
        }
        self.filter = filter;
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.view.clear();
        self.text.clear();
        self.revision += 1;
        let lines = std::mem::take(&mut self.lines);
        for (index, line) in lines.iter().enumerate() {
            if self.filter.is_none() || self.filter == line.tag {
                self.show(index, &line.text);
            }
        }
        self.lines = lines;
    }

    fn layout_job(
        &self,
        font: egui::FontId,
        normal: egui::Color32,
        warning: egui::Color32,
        error: egui::Color32,
    ) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob {
            text: self.text.clone(),
            ..Default::default()
        };
        let mut section = |range: std::ops::Range<usize>, tone: Tone| {
            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(range.start)..egui::text::ByteIndex(range.end),
                format: egui::TextFormat::simple(
                    font.clone(),
                    match tone {
                        Tone::Normal => normal,
                        Tone::Warning => warning,
                        Tone::Error => error,
                    },
                ),
            });
        };
        let mut start = 0;
        let mut run: Option<(usize, Tone)> = None;
        for (i, &index) in self.view.iter().enumerate() {
            let line = &self.lines[index];
            if let Some((from, was)) = run.filter(|&(_, was)| was != line.tone) {
                section(from..start, was);
                run = None;
            }
            run.get_or_insert((start, line.tone));
            start += line.text.len() + usize::from(i + 1 < self.view.len());
        }
        if let Some((from, was)) = run {
            section(from..self.text.len(), was);
        }
        job
    }

    fn entry_of_line(&self) -> Vec<usize> {
        self.view
            .iter()
            .enumerate()
            .flat_map(|(entry, &index)| {
                std::iter::repeat_n(entry, self.lines[index].text.matches('\n').count() + 1)
            })
            .collect()
    }

    fn clear(&mut self) {
        *self = Self {
            revision: self.revision + 1,
            pipe: self.pipe.take(),
            ..Self::new(self.keep)
        };
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

fn line_tag(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('[')?;
    let end = rest.find(']')?;
    (end > 0 && end <= 32 && !rest[..end].contains('[')).then(|| &rest[..end])
}

const SELECTION_SCROLL_MAX: f32 = 2400.0;
const SELECTION_SCROLL_RAMP: f32 = 200.0;

fn scroll_selection_past_edge(ui: &egui::Ui, text: &egui::Response) {
    let Some(pointer) = ui
        .input(|i| i.pointer.interact_pos())
        .filter(|_| text.dragged())
    else {
        return;
    };
    let speed = selection_scroll_speed(
        pointer.y,
        ui.clip_rect().y_range(),
        ui.ctx().viewport_rect().y_range(),
    );
    if speed != 0.0 {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        ui.scroll_with_delta(egui::vec2(0.0, -speed * dt));
        ui.ctx().request_repaint();
    }
}

fn selection_scroll_speed(pointer: f32, visible: egui::Rangef, window: egui::Rangef) -> f32 {
    let past = pointer - pointer.clamp(visible.min, visible.max);
    let room = if past < 0.0 {
        visible.min - window.min
    } else {
        window.max - visible.max
    };
    let share = (past.abs() / room.clamp(1.0, SELECTION_SCROLL_RAMP)).min(1.0);
    past.signum() * share * SELECTION_SCROLL_MAX
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolsTab {
    Settings,
    Equipment,
    Inventory,
    Prayer,
    Xp,
    Breaks,
    Feedback,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BreaksTab {
    Breaks,
    Sleeps,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Loader,
    Run(u64),
}

type LogJobKey = (
    LogTab,
    u64,
    egui::FontId,
    egui::Color32,
    egui::Color32,
    egui::Color32,
);

struct LaidOut {
    wrap_width: f32,
    pixels_per_point: f32,
    probe: Arc<egui::Galley>,
    galley: Arc<egui::Galley>,
}

struct LogJob {
    key: LogJobKey,
    job: egui::text::LayoutJob,
    entry_of_line: Vec<usize>,
    laid_out: RefCell<Option<LaidOut>>,
}

pub struct LoaderApp {
    settings: Settings,
    tab: Tab,
    sidebar_open: bool,
    terminal: Option<crate::terminal::Terminal>,
    display_name: String,
    varp: usize,
    varps: Option<Vec<i32>>,
    script_file: ScriptFile,
    last_script: Option<String>,
    find: Find,
    highlighter: Highlighter,
    fold: crate::fold::Folds,
    runs: Vec<Run>,
    active_run: Option<u64>,
    next_run: u64,
    run_target: Option<String>,
    start_prompt: Option<StartPrompt>,
    reload_prompt: Option<ReloadPrompt>,
    remembered_forms: HashMap<String, serde_json::Map<String, serde_json::Value>>,
    manifests: HashMap<String, (i64, injector::manifest::Manifest)>,
    queues: crate::queue::Saved,
    queue_runners: Vec<crate::queue::Runner>,
    queue_page: queue_page::Page,
    dev_toggles: [bool; DEV_TOGGLES.len()],
    cursor: Cursor,
    ui_font: crate::theme::UiFont,
    dev_tab: DevTab,
    inspector_tab: InspectorTab,
    tiles: Option<TilesWorker>,
    packets: crate::packets::Monitor,
    vars: crate::vars::Inspector,
    script_inspector: crate::script_log::Inspector,
    inventory: crate::inventory::Inspector,
    widgets: crate::widgets::Inspector,
    api: Api,
    auth: Auth,
    store: Store,
    accounts: Arc<RwLock<crate::accounts::Accounts>>,
    http_whitelist: Arc<RwLock<crate::whitelist::Whitelist>>,
    whitelist_page: crate::whitelist::Page,
    confirmed: Confirmed,
    warning: Option<PendingStart>,
    update: Option<update::Available>,
    update_state: UpdateState,
    instance: update::Instance,
    starting: Option<u32>,
    targets: Vec<Target>,
    dll_version: Option<String>,
    selected: Option<String>,
    busy: Option<&'static str>,
    log_tab: LogTab,
    log_collapsed: bool,
    followed_tab: (Tab, Option<u64>),
    loader_log: Log,
    log_job: Option<LogJob>,
    breaks_tab: BreaksTab,
    feedback: feedback::Page,
    gpu: Option<String>,
    icons: crate::icons::Icons,
    layouts: crate::layouts::AllLayouts,
    layouts_unreadable: bool,
    known_items: crate::layouts::Known,
    asking_items: Vec<i32>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

fn selected_in<'a>(targets: &'a [Target], selected: Option<&str>) -> Option<&'a Target> {
    let key = selected?;
    targets
        .iter()
        .find(|t| t.key() == key && t.client.foreign_bot.is_none())
}

fn loaded_in<'a>(targets: &'a [Target], selected: Option<&str>) -> Option<&'a Target> {
    selected_in(targets, selected).filter(|t| t.client.loaded.is_some())
}

fn run_in(runs: &mut [Run], id: Option<u64>) -> Option<&mut Run> {
    runs.iter_mut().find(|r| Some(r.id) == id)
}

fn editors_of(runs: &mut [Run], id: Option<u64>) -> Option<&mut crate::layouts::Editors> {
    run_in(runs, id).map(|r| &mut r.editors)
}

static EMPTY_LAYOUTS: std::sync::LazyLock<crate::layouts::ScriptLayouts> =
    std::sync::LazyLock::new(Default::default);

static EMPTY_RULES: crate::layouts::Rules = crate::layouts::Rules {
    required: Vec::new(),
    locked: Vec::new(),
};

fn rules_in(
    form: &Option<crate::form::Form>,
    kind: crate::layouts::Kind,
) -> &crate::layouts::Rules {
    form.as_ref().map_or(&EMPTY_RULES, |form| form.rules(kind))
}

impl LoaderApp {
    pub fn new(cc: &eframe::CreationContext<'_>, instance: update::Instance) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        settings.fill_missing(update::is_dev_install());
        let data_dir = crate::paths::project_dirs().map(|dirs| dirs.data_dir().to_path_buf());
        let (layouts, layouts_unreadable) =
            crate::layouts::load(cc.storage, LAYOUTS_KEY, data_dir.as_deref());
        let ui_font = cc
            .storage
            .and_then(|s| eframe::get_value(s, UI_FONT_KEY))
            .unwrap_or_default();
        crate::theme::apply(&cc.egui_ctx, ui_font);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let (tx, rx) = channel();
        let config = Config::from_env();
        let loader_log = Log::new(settings.log_lines);
        let gpu = cc.wgpu_render_state.as_ref().map(|state| {
            let info = state.adapter.get_info();
            format!("{} ({:?}, {})", info.name, info.backend, info.driver)
        });
        let mut app = Self {
            settings,
            tab: Tab::Clients,
            sidebar_open: false,
            terminal: None,
            display_name: String::new(),
            varp: varp::RUN,
            varps: None,
            script_file: load_default_script()
                .or_else(|| cc.storage.and_then(|s| eframe::get_value(s, SCRIPT_KEY)))
                .unwrap_or_default(),
            last_script: cc
                .storage
                .and_then(|s| eframe::get_value(s, LAST_SCRIPT_KEY))
                .flatten(),
            find: Find::default(),
            highlighter: Highlighter::default(),
            fold: crate::fold::Folds::default(),
            reload_prompt: None,
            remembered_forms: cc
                .storage
                .and_then(|s| eframe::get_value(s, FORMS_KEY))
                .unwrap_or_default(),
            manifests: HashMap::new(),
            queues: cc
                .storage
                .and_then(|s| eframe::get_value(s, crate::queue::STORAGE_KEY))
                .unwrap_or_default(),
            queue_runners: Vec::new(),
            queue_page: queue_page::Page::default(),
            dev_toggles: [false; DEV_TOGGLES.len()],
            cursor: cc
                .storage
                .and_then(|s| eframe::get_value(s, CURSOR_KEY))
                .unwrap_or(DEFAULT_CURSOR),
            ui_font,
            dev_tab: DevTab::Tools,
            inspector_tab: InspectorTab::Widget,
            tiles: None,
            packets: crate::packets::Monitor::default(),
            vars: crate::vars::Inspector::default(),
            script_inspector: crate::script_log::Inspector::default(),
            inventory: crate::inventory::Inspector::default(),
            widgets: crate::widgets::Inspector::new(),
            api: Api::new(config.clone()),
            auth: Auth::Restoring,
            store: Store::default(),
            accounts: Arc::new(RwLock::new(crate::accounts::Accounts::load())),
            http_whitelist: Arc::new(RwLock::new(crate::whitelist::Whitelist::new(
                cc.storage
                    .and_then(|s| eframe::get_value(s, HTTP_WHITELIST_KEY))
                    .unwrap_or_default(),
                &config,
            ))),
            whitelist_page: Default::default(),
            confirmed: cc
                .storage
                .and_then(|s| eframe::get_value(s, CONFIRMED_KEY))
                .unwrap_or_default(),
            warning: None,
            runs: Vec::new(),
            active_run: None,
            next_run: 1,
            run_target: None,
            start_prompt: None,
            update: None,
            update_state: UpdateState::Offered,
            instance,
            starting: None,
            targets: Vec::new(),
            dll_version: None,
            selected: None,
            busy: None,
            log_tab: LogTab::Loader,
            log_collapsed: false,
            followed_tab: (Tab::Clients, None),
            breaks_tab: BreaksTab::Breaks,
            icons: Default::default(),
            layouts,
            layouts_unreadable: layouts_unreadable.is_some(),
            known_items: Default::default(),
            asking_items: Vec::new(),
            feedback: feedback::Page::default(),
            gpu,
            loader_log,
            log_job: None,
            tx,
            rx,
        };
        app.scan(&cc.egui_ctx);
        thread::spawn(feedback::prune);
        app.restore_sign_in(&cc.egui_ctx);
        app.api_job(&cc.egui_ctx, |api| Event::Update(update::check(&api)));
        if let Some(failure) = update::failure_from_args() {
            app.error(failure.clone());
            app.update_state = UpdateState::Failed(failure);
        }
        if let Some(message) = layouts_unreadable {
            app.error(message);
        }
        app
    }

    fn end_session_job(&self, ctx: &egui::Context, session_id: String) {
        self.api_job(ctx, move |api| {
            Event::SessionEnded(api.end_session(&session_id))
        });
    }

    fn next_run_id(&mut self) -> u64 {
        let id = self.next_run;
        self.next_run += 1;
        id
    }

    fn api_job(&self, ctx: &egui::Context, job: impl FnOnce(Api) -> Event + Send + 'static) {
        let (tx, ctx, api) = (self.tx.clone(), ctx.clone(), self.api.clone());
        thread::spawn(move || {
            let _ = tx.send(job(api));
            ctx.request_repaint();
        });
    }

    fn restore_sign_in(&mut self, ctx: &egui::Context) {
        self.auth = Auth::Restoring;
        self.api_job(ctx, |api| Event::Restored(account::restore(&api)));
    }

    fn start_sign_in(&mut self, ctx: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.auth = Auth::SigningIn(cancel.clone());
        self.info(WAITING_FOR_BROWSER);
        self.api_job(ctx, move |api| {
            Event::SignedIn(account::sign_in(&api, &cancel))
        });
    }

    fn stop_all(&mut self) {
        self.queue_runners.clear();
        let deadline = Instant::now() + Duration::from_secs(3);
        for run in &mut self.runs {
            run.stop();
        }
        for store in self.runs.iter().filter_map(|r| r.store.as_ref()) {
            let left = deadline.saturating_duration_since(Instant::now());
            let _ = store.closed.recv_timeout(left);
        }
    }

    fn sign_out(&mut self, why: Option<String>) {
        self.stop_all();
        self.api.sign_out();
        self.auth = Auth::SignedOut;
        self.store = Store::default();
        self.warning = None;
        match why {
            Some(why) => self.error(format!("Signed out: {why}")),
            None => self.info("Signed out."),
        }
    }

    fn signed_in(&mut self, account: account::Account) {
        let me = account.me;
        self.info(format!("Signed in as {}.", me.username));
        if me.api_version > API_VERSION {
            self.error("This launcher is too old for the server. Update it to run store scripts.");
        }
        self.auth = Auth::SignedIn(me);
        self.scripts_loaded(account.scripts);
    }

    fn scripts_loaded(&mut self, result: Result<Vec<StoreScript>, ApiError>) {
        self.store.loading = false;
        match result {
            Ok(scripts) => {
                self.store.scripts = scripts;
                self.store.loaded = true;
            }
            Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
            Err(error) => self.store.error = Some(format!("Couldn't load the scripts: {error}")),
        }
    }

    fn store_changed<T>(
        &mut self,
        ctx: &egui::Context,
        result: Result<T, ApiError>,
        done: &str,
    ) -> bool {
        match result {
            Ok(_) => {
                self.store.redeem_result = Some((true, done.into()));
                self.fetch_scripts(ctx);
                true
            }
            Err(ApiError::SignedOut(why)) => {
                self.sign_out(Some(why));
                false
            }
            Err(error) => {
                self.store.redeem_result = Some((false, error.to_string()));
                false
            }
        }
    }

    fn start_failed(&mut self, ctx: &egui::Context, title: &str, error: impl std::fmt::Display) {
        self.script_error(format!("Couldn't start {title}: {error}"));
        self.fetch_scripts(ctx);
    }

    fn fetch_scripts(&mut self, ctx: &egui::Context) {
        self.store.loading = true;
        self.store.error = None;
        let community = self.store.community;
        self.api_job(ctx, move |api| {
            Event::Scripts(api.scripts("", "", community))
        });
    }

    fn run_block(&self, target: Option<&Target>) -> Option<String> {
        if self
            .auth
            .me()
            .is_some_and(|me| me.api_version > API_VERSION)
        {
            return Some("Update the launcher to run store scripts.".into());
        }
        if self.starting.is_some() {
            return Some("A script is starting.".into());
        }
        if self.start_prompt.is_some() || self.warning.is_some() {
            return Some("Answer the open dialog first.".into());
        }
        target
            .is_none()
            .then(|| "Pick a client with the agent loaded that isn't running a script.".into())
    }

    fn client_free(&self, target: &Target) -> bool {
        target.client.loaded.is_some()
            && target.client.foreign_bot.is_none()
            && target.client.is_64bit
            && !self.client_in_use(target.client.pid)
    }

    fn free_client(&self, pid: u32) -> Option<&Target> {
        self.targets
            .iter()
            .find(|t| t.client.pid == pid && self.client_free(t))
    }

    fn restart_refusal(&self, run: &Run) -> Option<String> {
        if run.store.is_none() {
            return self.file_restart_refusal(run);
        }
        if self.auth.me().is_none() {
            return Some("Sign in to run store scripts.".into());
        }
        if let Some(why) = self.run_block(run.target.as_ref()) {
            return Some(why);
        }
        run.target
            .as_ref()
            .and_then(|t| self.free_client(t.client.pid))
            .is_none()
            .then(|| "Its client is gone or busy. Start the script from the Scripts page.".into())
    }

    fn file_restart_refusal(&self, run: &Run) -> Option<String> {
        if self.reload_prompt.is_some() || self.start_prompt.is_some() || self.warning.is_some() {
            return Some("Answer the open dialog first.".into());
        }
        if self
            .runs
            .iter()
            .any(|r| r.store.is_none() && r.is_running())
        {
            return Some("A script from the editor is running.".into());
        }
        run.target
            .as_ref()
            .is_some_and(|t| self.free_client(t.client.pid).is_none())
            .then(|| "Its client is gone or busy. Run the script from the editor.".into())
    }

    fn restart_run(&mut self, ctx: &egui::Context, id: u64) {
        let Some(run) = self.run_by_id(id) else {
            return;
        };
        let Some(store) = &run.store else {
            self.restart_file(ctx, id);
            return;
        };
        let (session_id, script_id, community) = (
            store.session_id.clone(),
            store.script_id.clone(),
            store.community,
        );
        self.starting = run.target.as_ref().map(|t| t.client.pid);
        self.active_run = Some(id);
        self.log_tab = LogTab::Run(id);
        self.api_job(ctx, move |api| Event::RestartQuoted {
            run: id,
            result: session::restart_quote(
                &api,
                session::cache_root().as_deref(),
                &session_id,
                &script_id,
                community,
            ),
        });
    }

    fn restart_quoted(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        script: StoreScript,
        quote: Quote,
        lib: Result<Lib, String>,
    ) {
        let lib = match lib {
            Ok(lib) => lib,
            Err(error) => return self.script_error(format!("Couldn't restart: {error}")),
        };
        let Some(run) = self.run_by_id(id) else {
            return;
        };
        let (Some(old_price), Some(target)) =
            (run.store.as_ref().map(|s| s.price), run.target.clone())
        else {
            return;
        };
        let values = run
            .form_sent
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        let start = PendingStart {
            script,
            target,
            replaces: Some(id),
            lib,
            run: None,
            launch: Launch::with(values),
        };
        self.start_at_quote(ctx, start, Some(old_price), quote);
    }

    fn quote_store_script(
        &mut self,
        ctx: &egui::Context,
        script: StoreScript,
        target: Target,
        run: Option<u64>,
        values: serde_json::Value,
    ) {
        self.starting = Some(target.client.pid);
        self.api_job(ctx, move |api| Event::RunQuoted {
            result: session::quote(&api, session::cache_root().as_deref(), &script.id),
            script: Box::new(script),
            target: Box::new(target),
            run,
            values,
        });
    }

    fn open_store_script(&mut self, ctx: &egui::Context, script: StoreScript, target: Target) {
        match script.revision.and_then(|revision| {
            self.manifests
                .get(&script.id)
                .filter(|(r, _)| *r == revision)
        }) {
            Some((_, manifest)) => {
                let manifest = manifest.clone();
                self.configure_store_script(ctx, script, target, &manifest);
            }
            None => {
                self.starting = Some(target.client.pid);
                let id = script.id.clone();
                self.api_job(ctx, move |api| Event::ManifestFetched {
                    result: api.manifest(&id),
                    script: Box::new(script),
                    target: Box::new(target),
                });
            }
        }
    }

    fn manifest_fetched(
        &mut self,
        ctx: &egui::Context,
        script: StoreScript,
        target: Target,
        fetched: crate::api::models::ScriptManifest,
    ) {
        match injector::manifest::Manifest::parse(fetched.manifest) {
            Ok(manifest) => {
                self.manifests
                    .insert(script.id.clone(), (fetched.revision, manifest.clone()));
                let script = StoreScript {
                    revision: Some(fetched.revision),
                    ..script
                };
                self.configure_store_script(ctx, script, target, &manifest);
            }
            Err(error) => self.start_failed(ctx, &script.title, error),
        }
    }

    fn configure_store_script(
        &mut self,
        ctx: &egui::Context,
        script: StoreScript,
        target: Target,
        manifest: &injector::manifest::Manifest,
    ) {
        let Some(spec) = manifest.settings.clone() else {
            return self.quote_store_script(ctx, script, target, None, serde_json::json!({}));
        };
        let title = script.title.clone();
        let id = self.next_run_id();
        let mut run = Run::new(id, title.clone(), None, Some(target), None, &self.settings);
        run.source = Some(Source::Store(script));
        run.form = Some(self.settings_form(spec, &title));
        run.info(format!(
            "Set up {title}, then press Start. Nothing is billed until it starts."
        ));
        self.add_run(run);
    }

    fn settings_form(&self, spec: serde_json::Value, title: &str) -> crate::form::Form {
        let mut form = crate::form::Form::new(spec, self.remembered_forms.get(title));
        form.title = title.to_owned();
        form
    }

    fn refresh_me(&mut self, me: Me) {
        if let Some(current) = self.auth.me_mut() {
            *current = me;
        }
    }

    fn start_at_quote(
        &mut self,
        ctx: &egui::Context,
        start: PendingStart,
        old_price: Option<i64>,
        quote: Quote,
    ) {
        let microcredits = self.auth.me().map_or(0, |me| me.microcredits);
        let target = self.free_client(start.target.client.pid).cloned();
        let refusal = session::start_block(&start.script, &quote, microcredits);
        let (Some(target), None) = (target, refusal.as_ref()) else {
            let why = refusal.unwrap_or_else(|| "Its client is gone or busy.".into());
            self.script_error(format!("Couldn't start {}: {why}", start.script.title));
            return;
        };
        let asks = session::asks_first(old_price, &quote);
        let start = PendingStart { target, ..start };
        if asks {
            self.start_prompt = Some(StartPrompt {
                start,
                price: quote.price,
                fee: quote.fee,
                nth: quote.rank + 1,
                old_price: old_price.filter(|old| *old != quote.price),
            });
        } else {
            self.confirm_store_script(ctx, start);
        }
    }

    fn start_store_script(&mut self, ctx: &egui::Context, start: PendingStart) {
        let PendingStart {
            script,
            target,
            replaces,
            lib,
            run,
            launch,
        } = start;
        let pid = target.client.pid;
        self.starting = Some(pid);
        self.info(format!("Starting {}…", script.title));
        let warned = script.community.then_some(script.revision).flatten();
        self.api_job(ctx, move |api| {
            let result = session::prepare(&api, &script, lib, warned);
            Event::StoreStarted {
                pid,
                script: Box::new(script),
                replaces,
                run,
                launch: Box::new(launch),
                result,
            }
        });
    }

    fn run(&self) -> Option<&Run> {
        self.active_run.and_then(|id| self.run_by_id(id))
    }

    fn run_mut(&mut self) -> Option<&mut Run> {
        run_in(&mut self.runs, self.active_run)
    }

    fn run_by_id(&self, id: u64) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }

    fn run_by_id_mut(&mut self, id: u64) -> Option<&mut Run> {
        run_in(&mut self.runs, Some(id))
    }

    fn run_by_session(&mut self, session_id: &str) -> Option<&mut Run> {
        self.runs
            .iter_mut()
            .find(|r| r.session_id() == Some(session_id))
    }

    fn store_runs(&self) -> impl Iterator<Item = &StoreRun> {
        self.runs
            .iter()
            .filter(|r| r.is_running())
            .filter_map(|r| r.store.as_ref())
    }

    fn client_in_use(&self, pid: u32) -> bool {
        let pending = [
            self.start_prompt.as_ref().map(|prompt| &prompt.start),
            self.warning.as_ref(),
        ];
        self.starting == Some(pid)
            || self.queue_runners.iter().any(|r| r.pid == pid)
            || pending
                .into_iter()
                .flatten()
                .any(|start| start.target.client.pid == pid)
            || self
                .runs
                .iter()
                .any(|r| r.is_running() && r.target.as_ref().is_some_and(|t| t.client.pid == pid))
    }

    fn add_run(&mut self, run: Run) {
        self.active_run = Some(run.id);
        self.log_tab = LogTab::Run(run.id);
        self.runs.push(run);
        self.tab = Tab::Run;
        self.sidebar_open = false;
    }

    fn drive_xp(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let mut reads = Vec::new();
        for run in &mut self.runs {
            if !run.is_running() {
                continue;
            }
            ctx.request_repaint_after(Duration::from_secs(1));
            let Some(target) = run.loaded_target() else {
                continue;
            };
            if run.xp_reading || now < run.xp_next {
                ctx.request_repaint_after(
                    run.xp_next
                        .saturating_duration_since(now)
                        .max(Duration::from_millis(250)),
                );
                continue;
            }
            reads.push((run.id, target.clone()));
            run.xp_reading = true;
        }
        for (id, target) in reads {
            self.background(ctx, move |settings| Event::XpState {
                run: id,
                result: backend::state(&target, &settings),
            });
        }
    }

    fn script_host(&self, ctx: &egui::Context) -> crate::script::Host {
        crate::script::Host {
            ctx: ctx.clone(),
            accounts: self.accounts.clone(),
            whitelist: self.http_whitelist.clone(),
            prices_url: self.api.config().prices_url.clone(),
        }
    }

    fn keep_session(
        &self,
        ctx: &egui::Context,
        session_id: String,
        process: crate::script::Process,
        interval: Duration,
    ) -> Receiver<()> {
        let (tx, ctx, api) = (self.tx.clone(), ctx.clone(), self.api.clone());
        let (closing, closed) = channel();
        thread::spawn(move || {
            let _closing: Sender<()> = closing;
            session::keep(&api, &session_id, &process, interval, |kept| {
                let session_id = session_id.clone();
                let _ = tx.send(match kept {
                    Kept::Beat(beat) => Event::Heartbeat { session_id, beat },
                    Kept::Stop { reason, sign_out } => Event::SessionStopped {
                        session_id,
                        reason,
                        sign_out,
                    },
                    Kept::Ended(result) => Event::SessionEnded(result),
                });
                ctx.request_repaint();
            });
        });
        closed
    }

    fn store_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.auth.me().is_none() {
            match self.auth {
                Auth::Restoring => {
                    ui.label(RESTORING_SIGN_IN);
                }
                Auth::SigningIn(_) => {
                    ui.label(WAITING_FOR_BROWSER);
                }
                _ => {
                    ui.label("Sign in with Discord to browse and run scripts.");
                    if ui.button("Sign in").clicked() {
                        self.start_sign_in(&ctx);
                    }
                }
            }
            return;
        }
        if !self.store.loaded && !self.store.loading && self.store.error.is_none() {
            self.fetch_scripts(&ctx);
        }
        let target = self.run_target_ui(ui);
        let block = self.run_block(target.as_ref());
        let Some(me) = self.auth.me() else {
            return;
        };
        let has_site = !self.api.config().site_url.is_empty();
        match self.store.ui(ui, me, block.as_deref(), has_site) {
            Some(store::Action::OpenSite(path)) => open_site(&self.api.config().site_url, &path),
            Some(store::Action::Refresh) => {
                self.fetch_scripts(&ctx);
                self.api_job(&ctx, |api| Event::Me(api.me()));
            }
            Some(store::Action::Run(script)) => {
                if let Some(target) = target {
                    self.open_store_script(&ctx, *script, target);
                }
            }
            Some(store::Action::Redeem(code)) => {
                self.store.redeeming = true;
                self.store.redeem_result = None;
                self.api_job(&ctx, move |api| Event::Redeemed(api.redeem(&code)));
            }
            Some(store::Action::RemoveGrant(script)) => {
                let id = script.id.clone();
                self.api_job(&ctx, move |api| Event::GrantRemoved {
                    result: api.remove_grant(&id),
                    id,
                });
            }
            None => {}
        }
    }

    fn confirm_store_script(&mut self, ctx: &egui::Context, start: PendingStart) {
        let script = &start.script;
        let confirmed = script
            .revision
            .is_some_and(|revision| self.confirmed.has(&script.id, revision));
        if script.community && !confirmed {
            self.warning = Some(start);
        } else {
            self.start_store_script(ctx, start);
        }
    }

    fn start_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(prompt) = self.start_prompt.take() else {
            return;
        };
        let Some(me) = self.auth.me() else {
            return;
        };
        use crate::api::models::{credits_label, fee_per_hour, per_month, rate_label};
        let fee_hour = fee_per_hour(prompt.fee, me.minutes_per_month);
        let fee_minute = fee_hour / 60.0;
        let price_hour = prompt.price as f64;
        let total_hour = price_hour + fee_hour;
        let total_month = per_month(prompt.price, prompt.fee, me.minutes_per_month);
        let extra = prompt.fee > 0;
        let nth = match prompt.nth {
            2 => "2nd".to_owned(),
            3 => "3rd".to_owned(),
            n => format!("{n}th"),
        };
        let heading = if prompt.start.replaces.is_some() {
            "Restart session"
        } else {
            "Extra session"
        };
        let decision = decision_window(ctx, heading, 440.0, "Confirm", |ui| {
            ui.label(egui::RichText::new(&prompt.start.script.title).strong());
            ui.add_space(6.0);
            if prompt.old_price.is_some() {
                ui.label("The price of this script changed since the session started.");
            }
            if extra {
                ui.label(format!(
                    "This would be your {nth} script running at once. Every script past the first adds a fee on top of its own price:"
                ));
            }
            ui.add_space(4.0);
            egui::Grid::new("start_cost")
                .num_columns(2)
                .spacing(egui::vec2(16.0, 4.0))
                .show(ui, |ui| {
                    if extra {
                        ui.label("extra fee");
                        ui.horizontal(|ui| {
                            crate::theme::credits(
                                ui,
                                egui::RichText::new(credits_label(fee_hour)).strong(),
                                "/h",
                            );
                            crate::theme::credits(
                                ui,
                                egui::RichText::new(credits_label(fee_minute)).small(),
                                "/min",
                            );
                        });
                        ui.end_row();
                    }
                    if let Some(old_price) = prompt.old_price {
                        ui.label("old price");
                        crate::theme::credits(
                            ui,
                            egui::RichText::new(rate_label(old_price as f64)),
                            "/h",
                        );
                        ui.end_row();
                    }
                    ui.label("script price");
                    crate::theme::credits(
                        ui,
                        egui::RichText::new(rate_label(price_hour)).strong(),
                        "/h",
                    );
                    ui.end_row();
                    if extra {
                        ui.label("in total");
                        ui.horizontal(|ui| {
                            crate::theme::credits(
                                ui,
                                egui::RichText::new(rate_label(total_hour)).strong(),
                                "/h",
                            );
                            crate::theme::credits(
                                ui,
                                egui::RichText::new(rate_label(total_month)).small(),
                                " per 30 days",
                            );
                        });
                        ui.end_row();
                    }
                });
            if extra {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("The fee follows the order the scripts started in: when an older one stops, this one moves up and the oldest runs without a fee.").weak());
            }
            ui.add_space(8.0);
        });
        match decision {
            Some(true) => self.confirm_store_script(ctx, prompt.start),
            Some(false) => {}
            None => self.start_prompt = Some(prompt),
        }
    }

    fn warning_ui(&mut self, ctx: &egui::Context) {
        let Some(start) = self.warning.take() else {
            return;
        };
        let script = &start.script;
        let decision = decision_window(ctx, "Community script", 420.0, "Run anyway", |ui| {
            ui.label(egui::RichText::new(&script.title).strong());
            ui.label(format!("by {}", script.author));
            ui.add_space(6.0);
            ui.label(
                "This is a community script. It was written by a community member and has not been reviewed by the Volcren team. \
                 It runs on your computer and controls your game client, so only run scripts from people you trust.",
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "You won't be asked again until a new revision of this script comes out.",
                )
                .weak(),
            );
            ui.add_space(6.0);
        });
        match decision {
            Some(true) => {
                if let Some(revision) = script.revision {
                    self.confirmed.confirm(&script.id, revision);
                }
                self.start_store_script(ctx, start);
            }
            Some(false) => {}
            None => self.warning = Some(start),
        }
    }

    fn feedback_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.heading("Feedback");
        if self.auth.me().is_none() {
            ui.label("Sign in to score scripts and report bugs.");
            return;
        }
        self.fetch_sessions_if_stale(&ctx);
        let running: Vec<String> = self.store_runs().map(|s| s.session_id.clone()).collect();
        let actions = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.feedback
                    .ui(ui, &mut self.settings.include_system_info, &running)
            })
            .inner;
        self.apply_feedback_actions(&ctx, actions);
    }

    fn run_feedback_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(session_id) = self.run().and_then(|r| r.session_id().map(str::to_owned)) else {
            return;
        };
        self.fetch_sessions_if_stale(&ctx);
        let Some(session) = self
            .feedback
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .cloned()
        else {
            ui.label(
                egui::RichText::new(if self.feedback.loading {
                    "Loading…"
                } else {
                    "This run isn't listed yet; it is once the server has it."
                })
                .weak(),
            );
            if let Some(error) = &self.feedback.error {
                error_label(ui, error);
            }
            return;
        };
        let draft = self.feedback.draft(&session_id);
        let actions =
            feedback::session_ui(ui, &session, draft, &mut self.settings.include_system_info);
        self.apply_feedback_actions(&ctx, actions);
    }

    fn fetch_sessions_if_stale(&mut self, ctx: &egui::Context) {
        if self.feedback.stale(Instant::now()) {
            self.fetch_sessions(ctx);
        }
    }

    fn fetch_sessions(&mut self, ctx: &egui::Context) {
        self.feedback.loading = true;
        self.api_job(ctx, |api| Event::RecentSessions(api.recent_sessions()));
    }

    fn apply_feedback_actions(&mut self, ctx: &egui::Context, actions: Vec<feedback::Action>) {
        for action in actions {
            match action {
                feedback::Action::Refresh => self.fetch_sessions(ctx),
                feedback::Action::Score { session_id, score } => self.api_job(ctx, move |api| {
                    let result = api.score_session(&session_id, score);
                    Event::Scored { session_id, result }
                }),
                feedback::Action::Report {
                    session_id,
                    description,
                } => self.send_report(ctx, session_id, description),
            }
        }
    }

    fn versions(&self, run: Option<&Run>) -> feedback::Versions {
        feedback::Versions {
            agent: run
                .and_then(|r| r.target.as_ref())
                .and_then(|t| t.client.version.clone())
                .or_else(|| self.dll_version.clone()),
            lib: run
                .and_then(|r| r.store.as_ref())
                .map(|s| s.lib_version.clone()),
            gpu: self.gpu.clone(),
        }
    }

    fn send_report(&mut self, ctx: &egui::Context, session_id: String, description: String) {
        let run = self
            .runs
            .iter()
            .find(|r| r.session_id() == Some(session_id.as_str()));
        let versions = self.versions(run);
        let include_system = self.settings.include_system_info;
        let (tx, ctx, api, settings) = (
            self.tx.clone(),
            ctx.clone(),
            self.api.clone(),
            self.settings.clone(),
        );
        let live = run
            .filter(|r| r.is_running())
            .map(|r| (r.log.texts(), r.names.clone(), r.loaded_target().cloned()));
        thread::spawn(move || {
            let diagnostics = feedback::diagnostics(&versions, include_system);
            let report = match live {
                Some((log, mut names, target)) => {
                    let snapshot = target
                        .and_then(|target| feedback::read_snapshot(&target, &settings, &mut names));
                    feedback::report(description, &log, &names, snapshot, diagnostics)
                }
                None => {
                    let record = feedback::load_record(&session_id);
                    crate::api::models::Report {
                        description,
                        log: record
                            .as_ref()
                            .map(|r| r.log.clone())
                            .filter(|log| !log.is_empty()),
                        snapshot: record.and_then(|r| r.snapshot),
                        diagnostics,
                    }
                }
            };
            let result = api
                .report_session(&session_id, &report)
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Reported { session_id, result });
            ctx.request_repaint();
        });
    }

    fn keep_run_record(&mut self, ctx: &egui::Context, index: usize) {
        let run = &self.runs[index];
        let Some(session_id) = run.session_id().map(str::to_owned) else {
            return;
        };
        let title = run.title.clone();
        let (log, mut names, target) = (
            run.log.texts(),
            run.names.clone(),
            run.loaded_target().cloned(),
        );
        self.feedback.fetched_at = None;
        self.background(ctx, move |settings| {
            let snapshot =
                target.and_then(|target| feedback::read_snapshot(&target, &settings, &mut names));
            let log = feedback::scrubbed_log(&log, &names);
            let record = feedback::Record {
                session_id,
                title,
                log,
                snapshot,
                ended_at: crate::paths::unix_secs(),
            };
            Event::RunKept(feedback::store_record(&record))
        });
    }

    fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<(u64, String, bool)> = self
            .runs
            .iter()
            .map(|r| (r.id, sidebar_name(&r.title), !r.is_running()))
            .collect();
        let picked = egui::ScrollArea::vertical()
            .id_salt("sidebar")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                sidebar_tree(
                    ui,
                    self.tab,
                    self.active_run,
                    &rows,
                    &self.api.config().site_url,
                )
            })
            .inner;
        if let Some((tab, run)) = picked {
            self.tab = tab;
            if run.is_some() {
                self.active_run = run;
            }
            self.sidebar_open = false;
        }
    }

    fn start_in_progress(&self) -> bool {
        self.runs.iter().any(|run| run.ended.is_none()) || self.start_pending()
    }

    fn start_pending(&self) -> bool {
        self.starting.is_some() || self.start_prompt.is_some() || self.warning.is_some()
    }

    fn update_failed(&mut self, error: String) {
        self.error(error.clone());
        self.update_state = UpdateState::Failed(error);
    }

    fn can_update(&mut self) -> bool {
        let alone = self.instance.alone();
        if !alone {
            self.update_failed(
                "Another VOLCREN window is open. Close it to update, so the update doesn't replace the files it runs from.".into(),
            );
        }
        alone
    }

    fn start_update(&mut self, ctx: &egui::Context, release: update::Release) {
        if !self.can_update() {
            return;
        }
        let progress = Arc::new(update::Progress::default());
        self.update_state = UpdateState::Downloading(progress.clone());
        let repaint = ctx.clone();
        self.api_job(ctx, move |api| {
            let result = update::prepare(&api, &release, &progress, &|| {
                repaint.request_repaint();
            });
            Event::UpdatePrepared { progress, result }
        });
    }

    fn apply_update(&mut self, ctx: &egui::Context, prepared: &update::Prepared) {
        if !self.can_update() {
            return;
        }
        match update::apply(prepared) {
            Ok(()) => {
                self.update_state = UpdateState::Installing;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(error) => self.update_failed(error),
        }
    }

    fn updating_ui(&mut self, ui: &mut egui::Ui) {
        let status = match &self.update_state {
            UpdateState::Downloading(progress) => progress_text(progress),
            _ => "Installing...".to_owned(),
        };
        let version = self.update.as_ref().map(|a| a.release.version.clone());
        let mut skipped = false;
        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                ui.heading("Updating VOLCREN");
                if let Some(version) = version {
                    ui.label(format!("Version {version}"));
                }
                ui.label(status);
                if matches!(self.update_state, UpdateState::Downloading(_)) {
                    ui.add_space(8.0);
                    skipped = ui.button("Skip for now").clicked();
                }
            });
        });
        if skipped {
            if let UpdateState::Downloading(progress) = &self.update_state {
                progress.cancel();
            }
            self.update_state = UpdateState::Offered;
        }
    }

    fn update_banner_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let Some(available) = &self.update else {
            return;
        };
        let (release, kind) = (&available.release, available.kind);
        if kind == update::Kind::Dev {
            return;
        }
        let running = self.start_in_progress();
        let failure = match &self.update_state {
            UpdateState::Failed(error) => Some(error.as_str()),
            _ => None,
        };
        let main_button = match (failure, kind) {
            (_, update::Kind::Manual) => "Download",
            (Some(_), _) => "Retry",
            _ => "Update and restart",
        };
        let mut clicked = false;
        let mut dismissed = false;
        egui::Panel::bottom("update").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dismissed = ui
                        .add(egui::Button::new(ICON_CANCEL).frame(false))
                        .on_hover_text("Dismiss")
                        .clicked();
                    let enabled = kind == update::Kind::Manual || !running;
                    let button = ui.add_enabled(enabled, egui::Button::new(main_button));
                    let button = button.on_disabled_hover_text("Stop running scripts to update.");
                    let button = if kind == (update::Kind::Auto { admin: true }) {
                        button.on_hover_text("An administrator has to approve the update.")
                    } else {
                        button
                    };
                    clicked = button.clicked();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!(
                                    "Version {} is available",
                                    release.version
                                ))
                                .color(crate::theme::BLUE),
                            )
                            .truncate(),
                        )
                        .on_hover_text(format!("You have version {}", update::VERSION));
                        if let Some(error) = failure {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(error).color(ui.visuals().error_fg_color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(error);
                        }
                    });
                });
            });
            ui.add_space(2.0);
        });
        if clicked {
            if kind == update::Kind::Manual {
                if let Some(url) = release.download_url(&self.api) {
                    let _ = webbrowser::open(&url);
                }
            } else {
                let release = release.clone();
                self.start_update(ctx, release);
            }
        }
        if dismissed {
            self.update = None;
            ctx.request_repaint();
        }
    }

    fn version_text(&self) -> String {
        let mut text = format!("Version {}", update::VERSION);
        if let Some(available) = &self.update
            && available.kind == update::Kind::Dev
        {
            text.push_str(&format!(
                "\nVersion {} is available. Development build: updates aren't installed here.",
                available.release.version
            ));
        }
        text
    }

    fn account_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.auth {
            Auth::SignedIn(me) => {
                let name = me.username.clone();
                let balance = crate::api::models::balance_label(me.microcredits as f64);
                let minutes_per_month = me.minutes_per_month;
                let rate: f64 = self
                    .store_runs()
                    .map(|s| s.rate_per_hour(minutes_per_month))
                    .sum();
                let sign_out_width = icon_button_width(ui, &[(ICON_SIGN_OUT, "Sign out")]);
                if icon_button(ui, ICON_SIGN_OUT, "Sign out", sign_out_width, true).clicked() {
                    self.sign_out(None);
                    return;
                }
                if rate > 0.0 {
                    crate::theme::credits(ui, egui::RichText::new(crate::api::models::rate_label(rate)).weak(), "/h")
                        .on_hover_text("What the scripts running from this loader cost an hour, prices and extra-session fees together.");
                }
                let site_url = &self.api.config().site_url;
                let balance = egui::RichText::new(balance);
                if site_url.is_empty() {
                    crate::theme::credits(ui, balance, "").on_hover_text("Your balance.");
                } else if crate::theme::credits_link(ui, balance, "")
                    .on_hover_text("Your balance. Click to add funds on the website.")
                    .clicked()
                {
                    open_site(site_url, "/credits");
                }
                ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
            }
            Auth::SigningIn(cancel) => {
                if ui.button("Cancel").clicked() {
                    cancel.store(true, Ordering::Relaxed);
                }
                ui.spinner();
                ui.label("Signing in…");
            }
            Auth::Restoring => {
                ui.spinner();
            }
            Auth::SignedOut => {
                if ui.button("Sign in").clicked() {
                    self.start_sign_in(&ctx);
                }
            }
        }
    }

    fn login_gate_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(16.0));
            ui.label(
                egui::RichText::new("Sign in to continue")
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(8.0);
            match &self.auth {
                Auth::Restoring => {
                    ui.spinner();
                    ui.label(RESTORING_SIGN_IN);
                }
                Auth::SigningIn(cancel) => {
                    ui.spinner();
                    ui.label(WAITING_FOR_BROWSER);
                    ui.add_space(8.0);
                    if ui.button("Cancel").clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                _ => {
                    ui.label("Volcren needs a Discord account. Your browser will open to sign in.");
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("Sign in with Discord").size(16.0))
                        .clicked()
                    {
                        self.start_sign_in(&ctx);
                    }
                }
            }
        });
    }

    fn info(&mut self, text: impl Into<String>) {
        self.loader_log.push(false, text.into());
    }

    fn error(&mut self, text: impl Into<String>) {
        self.loader_log.push(true, text.into());
    }

    fn script_log(&mut self, error: bool, text: impl Into<String>) {
        match self.run_mut() {
            Some(run) => run.log.push(error, text.into()),
            None => self.loader_log.push(error, text.into()),
        }
    }

    fn script_info(&mut self, text: impl Into<String>) {
        self.script_log(false, text);
    }

    fn script_error(&mut self, text: impl Into<String>) {
        self.script_log(true, text);
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

    fn tiles_ui(&mut self, ctx: &egui::Context) {
        let mut layers: Vec<String> = DEV_LAYERS
            .iter()
            .zip(&self.dev_toggles)
            .filter(|(_, on)| **on)
            .filter_map(|(layer, _)| layer.map(str::to_owned))
            .collect();
        if self.widget_inspector_showing() {
            layers.extend(self.widgets.highlight_layers());
        }
        let Some(target) = (!layers.is_empty())
            .then(|| loaded_in(&self.targets, self.selected.as_deref()))
            .flatten()
        else {
            self.tiles = None;
            return;
        };
        if self
            .tiles
            .as_ref()
            .is_some_and(|worker| worker.pid != target.client.pid)
        {
            self.tiles = None;
        }
        match &self.tiles {
            Some(worker) => {
                if let Ok(mut current) = worker.layers.lock()
                    && *current != layers
                {
                    *current = layers;
                }
            }
            None => {
                self.tiles = Some(TilesWorker::start(
                    ctx,
                    self.tx.clone(),
                    target.clone(),
                    self.settings.clone(),
                    layers,
                ))
            }
        }
    }

    fn poll_packets(&mut self, ctx: &egui::Context) {
        if !self.packets.capture {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        let now = Instant::now();
        ctx.request_repaint_after(crate::packets::POLL_EVERY);
        if self.packets.wants_layouts(target.client.pid, now) {
            self.packets.start_layouts(target.client.pid);
            let (pid, target) = (target.client.pid, target.clone());
            self.background(ctx, move |settings| Event::PacketLayouts {
                pid,
                result: backend::packet_layouts(&target, &settings),
            });
        }
        if !self.packets.due(target.client.pid, now) {
            return;
        }
        let (pid, since) = (target.client.pid, self.packets.start(target.client.pid));
        let target = target.clone();
        self.background(ctx, move |settings| Event::Packets {
            pid,
            result: backend::packets(&target, &settings, since),
        });
    }

    fn poll_vars(&mut self, ctx: &egui::Context) {
        if !self.vars.enabled {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        let now = Instant::now();
        ctx.request_repaint_after(crate::vars::POLL_EVERY);
        if !self.vars.due(target.client.pid, now) {
            return;
        }
        let pid = target.client.pid;
        let crate::vars::Request {
            previous,
            known,
            varc_since,
        } = self.vars.start(pid);
        let target = target.clone();
        self.background(ctx, move |settings| {
            let tick_count = || {
                backend::state(&target, &settings)
                    .ok()
                    .and_then(|state| state.tick_count)
            };
            let read = || -> Result<crate::vars::Poll, String> {
                let varps = backend::varps(&target, &settings)?;
                let (mut tick, mut varbits) = (None, Vec::new());
                if let (Some(varps), Some(previous)) = (&varps, &previous) {
                    let changed = crate::vars::Inspector::unknown(previous, varps, &known);
                    if previous != varps {
                        tick = tick_count();
                    }
                    for varp in changed {
                        varbits.push((varp, backend::varbits_of(&target, &settings, varp)?));
                    }
                }
                let (varcs, varc_error) =
                    match backend::varc_changes(&target, &settings, varc_since) {
                        Ok(page) => (page, None),
                        Err(error) => (
                            injector::VarcPage {
                                next: varc_since,
                                ..Default::default()
                            },
                            Some(error),
                        ),
                    };
                if tick.is_none() && !varcs.changes.is_empty() {
                    tick = tick_count();
                }
                Ok(crate::vars::Poll {
                    varps,
                    tick,
                    varbits,
                    varcs,
                    varc_error,
                })
            };
            Event::Vars {
                pid,
                result: read(),
            }
        });
    }

    fn poll_widget_mouse(&mut self, ctx: &egui::Context) {
        if !self.widgets.follow_mouse || !self.widget_inspector_showing() {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        ctx.request_repaint_after(crate::widgets::FOLLOW_EVERY);
        if !self.widgets.follow_due(Instant::now()) {
            return;
        }
        let (pid, target) = (target.client.pid, target.clone());
        self.background(ctx, move |settings| {
            let result = backend::connection(&target, &settings).map(|connection| {
                connection
                    .filter(|c| c.mouse_x >= 0 && c.mouse_y >= 0)
                    .map(|c| (c.mouse_x, c.mouse_y))
            });
            Event::WidgetMouse { pid, result }
        });
    }

    fn refresh_widgets(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.widgets.start(pid);
        let known = self.widgets.known_groups();
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::widgets::Poll, String> {
                let tree = backend::widget_tree(&target, &settings)?;
                let mut groups: Vec<i32> = tree
                    .widgets
                    .iter()
                    .map(|w| w.widget.group)
                    .filter(|group| !known.contains(group))
                    .collect();
                groups.sort_unstable();
                groups.dedup();
                let names = if groups.is_empty() {
                    Vec::new()
                } else {
                    let names = backend::game_val_names(
                        &target,
                        &settings,
                        crate::widgets::INTERFACE_GAME_VAL,
                        &groups,
                    )?;
                    paired(&groups, names)
                };
                Ok(crate::widgets::Poll { tree, names })
            };
            Event::Widgets {
                pid,
                result: read(),
            }
        });
    }

    fn widgets_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let pid = self.selected_loaded_target().map(|t| t.client.pid);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            let busy = self.widgets.reading();
            refresh = ui.add_enabled(pid.is_some() && !busy, egui::Button::new("Refresh")).clicked();
            ui.checkbox(&mut self.widgets.hide_hidden, "Hide hidden");
            ui.checkbox(&mut self.widgets.follow_mouse, "Follow mouse")
                .on_hover_text("Selects the widget under the client's mouse, looking about ten times a second. Move the mouse over the client and the tree follows; untick it to keep the selection.");
        });
        client_caption(ui, pid);
        refresh |=
            !self.widgets.has_read() && !self.widgets.reading() && self.widgets.error.is_none();
        if refresh && let Some(target) = self.selected_loaded_target().cloned() {
            self.refresh_widgets(&ctx, target);
        }
        if let Some(error) = &self.widgets.error {
            error_label(ui, error);
        }
        ui.separator();
        let inspector = &mut self.widgets;
        let mut picked = inspector.selected;
        let reveal = std::mem::take(&mut inspector.reveal);
        let open = if reveal {
            inspector.open_path()
        } else {
            HashSet::new()
        };
        ui.columns(2, |columns| {
            egui::ScrollArea::both().id_salt("widget_tree").auto_shrink([false, false]).show(&mut columns[0], |ui| {
                for &root in &inspector.built().roots {
                    widget_node_ui(ui, inspector, root, &open, reveal, &mut picked);
                }
            });
            egui::ScrollArea::vertical().id_salt("widget_fields").auto_shrink([false, false]).show(&mut columns[1], |ui| match inspector.selected_node() {
                Some(node) => {
                    egui::Grid::new("widget_field_grid").num_columns(2).striped(true).show(ui, |ui| {
                        for (name, value) in inspector.fields(node) {
                            ui.label(name);
                            ui.add(egui::Label::new(value).selectable(true));
                            ui.end_row();
                        }
                    });
                }
                None => {
                    ui.weak("Pick a widget in the tree, or tick Follow mouse and point at one in the client.");
                }
            });
        });
        inspector.selected = picked;
    }

    fn poll_script_log(&mut self, ctx: &egui::Context) {
        if !self.script_inspector.enabled {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        let now = Instant::now();
        ctx.request_repaint_after(crate::script_log::POLL_EVERY);
        if !self.script_inspector.due(target.client.pid, now) {
            return;
        }
        let (pid, since) = (
            target.client.pid,
            self.script_inspector.start(target.client.pid),
        );
        let target = target.clone();
        self.background(ctx, move |settings| Event::ScriptRuns {
            pid,
            result: backend::script_runs(&target, &settings, since)
                .map(|page| crate::script_log::Poll { page }),
        });
    }

    fn poll_inventory(&mut self, ctx: &egui::Context) {
        if !self.inventory.enabled {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        let now = Instant::now();
        if let Some(wait) = self.inventory.retry_in(now) {
            ctx.request_repaint_after(wait);
        }
        if !self.inventory.due(target.client.pid, now) {
            return;
        }
        let pid = target.client.pid;
        let crate::inventory::Request {
            since,
            known_containers,
            known_items,
        } = self.inventory.start(pid);
        let target = target.clone();
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::inventory::Poll, String> {
                let (page, baseline) = match since {
                    Some(since) => (
                        backend::item_changes(
                            &target,
                            &settings,
                            since,
                            crate::inventory::WAIT_FOR_CHANGE,
                        )?,
                        None,
                    ),
                    None => {
                        let page = backend::item_changes(
                            &target,
                            &settings,
                            injector::ITEM_CHANGES_NOW,
                            Duration::ZERO,
                        )?;
                        (page, Some(backend::item_containers(&target, &settings)?))
                    }
                };
                let missing = |known: &[i32], mut ids: Vec<i32>| {
                    ids.retain(|id| *id >= 0 && !known.contains(id));
                    ids.sort_unstable();
                    ids.dedup();
                    ids
                };
                let mut containers: Vec<i32> =
                    page.changes.iter().map(|change| change.container).collect();
                let mut items: Vec<i32> = page
                    .changes
                    .iter()
                    .flat_map(|change| [change.old_item, change.new_item])
                    .collect();
                for (id, held) in baseline.iter().flatten() {
                    containers.push(*id);
                    items.extend(held.iter().map(|item| item.id));
                }
                let new_containers = missing(&known_containers, containers);
                let new_items = missing(&known_items, items);
                let container_names = if new_containers.is_empty() {
                    Vec::new()
                } else {
                    paired(
                        &new_containers,
                        backend::game_val_names(
                            &target,
                            &settings,
                            crate::inventory::INV_GAME_VAL,
                            &new_containers,
                        )?,
                    )
                };
                let (item_names, stackable) = if new_items.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    item_info(&target, &settings, &new_items)?
                };
                Ok(crate::inventory::Poll {
                    page,
                    baseline,
                    container_names,
                    item_names,
                    stackable,
                })
            };
            Event::Inventory {
                pid,
                result: read(),
            }
        });
    }

    fn inventory_ui(&mut self, ui: &mut egui::Ui) {
        let pid = self.selected_loaded_target().map(|t| t.client.pid);
        let inspector = &mut self.inventory;
        let icons = &mut self.icons;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut inspector.enabled, "Enable");
            if ui
                .button(format!("Refresh ({})", inspector.pending()))
                .on_hover_text("Adds the changes read since the last refresh to the list.")
                .clicked()
            {
                inspector.refresh();
            }
            if ui.button("Clear").clicked() {
                inspector.clear();
            }
        });
        client_caption(ui, pid);
        if let Some(error) = inspector.error.iter().chain(&icons.error).next() {
            error_label(ui, error);
        }
        ui.separator();
        let mut picked = inspector.selected;
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical()
                .id_salt("inventory_tree")
                .auto_shrink([false, false])
                .show(&mut columns[0], |ui| {
                    for (id, log) in inspector.logs() {
                        egui::CollapsingHeader::new(inspector.container_label(*id))
                            .id_salt(("inventory_container", *id))
                            .show(ui, |ui| {
                                for (index, snapshot) in log.iter().enumerate() {
                                    let label = snapshot.tick.map_or_else(
                                        || "Tick: ?".to_owned(),
                                        |tick| format!("Tick: {tick}"),
                                    );
                                    if ui
                                        .selectable_label(picked == Some((*id, index)), label)
                                        .clicked()
                                    {
                                        picked = Some((*id, index));
                                    }
                                }
                            });
                    }
                });
            egui::ScrollArea::vertical()
                .id_salt("inventory_items")
                .auto_shrink([false, false])
                .show(&mut columns[1], |ui| match inspector.picked() {
                    Some((snapshot, before)) => {
                        inventory_items_ui(ui, inspector, icons, snapshot, before)
                    }
                    None => {
                        ui.weak("Pick a tick to see the items.");
                    }
                });
        });
        inspector.selected = picked;
    }

    fn poll_icons(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(wait) = self.icons.retry_in(now) {
            ctx.request_repaint_after(wait);
        }
        if !self.icons.due(now) {
            return;
        }
        let Some(target) = loaded_in(&self.targets, self.selected.as_deref()) else {
            return;
        };
        let (batch, target) = (self.icons.start(), target.clone());
        self.background(ctx, move |settings| {
            let mut pictures = Vec::new();
            let mut read = || -> Result<(), String> {
                if !batch.items.is_empty() {
                    let images = backend::item_icons(&target, &settings, &batch.items)?;
                    pictures.extend(
                        batch
                            .items
                            .iter()
                            .map(|r| crate::icons::Key::Item(*r))
                            .zip(images),
                    );
                }
                if !batch.sprites.is_empty() {
                    let images = backend::sprites(&target, &settings, &batch.sprites)?;
                    pictures.extend(
                        batch
                            .sprites
                            .iter()
                            .map(|id| crate::icons::Key::Sprite(*id))
                            .zip(images),
                    );
                }
                Ok(())
            };
            let error = read().err();
            Event::Icons { pictures, error }
        });
    }

    fn script_log_ui(&mut self, ui: &mut egui::Ui) {
        let pid = self.selected_loaded_target().map(|t| t.client.pid);
        let mut load_names = false;
        let mut action = None;
        let inspector = &mut self.script_inspector;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut inspector.enabled, "Enable");
            ui.checkbox(&mut inspector.triggers, "Trigger events").on_hover_text("Component and menu events, which find their script by trigger so their script id isn't known.");
            ui.checkbox(&mut inspector.follow, "Follow");
            ui.label("Blacklist");
            ui.add(egui::TextEdit::singleline(&mut inspector.blacklist).desired_width(140.0).hint_text("6697, 4730"))
                .on_hover_text("Scripts to leave out: ids, or names once the names are loaded. Separate with commas or spaces. A client runs a few scripts every frame; the default names the ones seen on an idle account.");
            let load = ui.add_enabled(!inspector.loading_names, egui::Button::new(if inspector.loading_names { "Loading names…" } else { "Load names" })).on_hover_text(
                "Downloads RuneLite's names for the client's scripts from github.com/runelite/cs2-scripts (about 10 MB) and shows them in the list. The client has none of its own. They match the game build the repository was last decompiled from.",
            );
            if load.clicked() {
                load_names = true;
            }
            if ui.button("Clear").clicked() {
                inspector.clear();
            }
            action = rows_buttons(ui, inspector.kept() == 0, inspector.pipe.as_ref());
        });
        self.rows_action(ui.ctx(), action, "script runs", "scripts.txt", |app| {
            &mut app.script_inspector
        });
        let inspector = &mut self.script_inspector;
        client_caption(ui, pid);
        if let Some(error) = &inspector.names_error {
            error_label(ui, error);
        }
        let visible = inspector.visible();
        let names = &inspector.names;
        if !names.is_empty() {
            ui.weak(format!(
                "{} script names, from RuneLite's cs2-scripts at {}.",
                names.len(),
                names.source
            ));
        }
        ui.weak(format!(
            "{} shown of {} kept{}. Scripts started by trigger lookup aren't listed.",
            visible.len(),
            inspector.kept(),
            missed(inspector.dropped)
        ));
        if let Some(error) = &inspector.error {
            error_label(ui, error);
        }
        ui.separator();
        let (_, row_height) = mono_row(ui);
        let table = LogTable {
            id: "script_log",
            follow: inspector.follow,
            row_height,
            max_height: None,
            clickable: false,
        };
        table.show(
            ui,
            &[
                ("Tick", narrow(60.0)),
                ("Script", narrow(240.0)),
                ("Arguments", narrow(260.0)),
                ("Time", rest(60.0)),
            ],
            visible.len(),
            |mut row| {
                let index = row.index();
                let entry = &visible[index];
                let first = index == 0 || visible[index - 1].run.tick != entry.run.tick;
                row.col(|ui| {
                    if first {
                        ui.label(mono(entry.tick_text()));
                    }
                });
                row.col(|ui| {
                    ui.label(mono(entry.indented_name(names)));
                });
                row.col(|ui| {
                    ui.label(mono(entry.args_text()));
                });
                row.col(|ui| {
                    ui.label(mono(entry.took()));
                });
            },
        );
        drop(visible);
        if load_names {
            inspector.loading_names = true;
            inspector.names_error = None;
            self.background(ui.ctx(), |_| {
                Event::ScriptNames(crate::script_names::Names::download())
            });
        }
    }

    fn vars_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Varps")
            .default_open(false)
            .show(ui, |ui| self.varps_ui(ui));
        ui.separator();
        let pid = self.selected_loaded_target().map(|t| t.client.pid);
        let mut action = None;
        let inspector = &mut self.vars;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut inspector.enabled, "Enable");
            ui.checkbox(&mut inspector.varbits, "Varbit");
            ui.checkbox(&mut inspector.varps, "VarPlayer");
            ui.checkbox(&mut inspector.varc_ints, "VarClientInt");
            ui.checkbox(&mut inspector.varc_strings, "VarClientStr");
            ui.checkbox(&mut inspector.follow, "Follow");
            if ui.button("Clear").clicked() {
                inspector.clear();
            }
            action = rows_buttons(ui, inspector.kept() == 0, inspector.pipe.as_ref());
        });
        self.rows_action(ui.ctx(), action, "var changes", "vars.txt", |app| {
            &mut app.vars
        });
        let inspector = &mut self.vars;
        client_caption(ui, pid);
        let visible = inspector.visible();
        ui.weak(format!(
            "{} shown of {} kept. Client variables are listed as their index.",
            visible.len(),
            inspector.kept()
        ));
        if let Some(error) = &inspector.error {
            error_label(ui, error);
        }
        if let Some(error) = &inspector.varc_error {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Client variables: {error}"),
            );
        }
        ui.separator();
        let (_, row_height) = mono_row(ui);
        let table = LogTable {
            id: "vars",
            follow: inspector.follow,
            row_height,
            max_height: None,
            clickable: false,
        };
        table.show(
            ui,
            &[
                ("Tick", narrow(60.0)),
                ("Kind", narrow(100.0)),
                ("Name", narrow(220.0)),
                ("Old", narrow(110.0)),
                ("New", rest(60.0)),
            ],
            visible.len(),
            |mut row| {
                let index = row.index();
                let entry = &visible[index];
                let first = index == 0 || visible[index - 1].tick != entry.tick;
                row.col(|ui| {
                    if first {
                        ui.label(mono(entry.tick_text()));
                    }
                });
                row.col(|ui| {
                    ui.label(mono(entry.kind.label()));
                });
                row.col(|ui| {
                    ui.label(mono(entry.name.as_str()));
                });
                row.col(|ui| {
                    ui.label(mono(entry.old.as_str()));
                });
                row.col(|ui| {
                    ui.label(mono(entry.new.as_str()));
                });
            },
        );
    }

    fn packets_ui(&mut self, ui: &mut egui::Ui) {
        let pid = self.selected_loaded_target().map(|t| t.client.pid);
        let mut action = None;
        let monitor = &mut self.packets;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut monitor.capture, "Enable");
            ui.checkbox(&mut monitor.received, "Received");
            ui.checkbox(&mut monitor.sent, "Sent");
            ui.checkbox(&mut monitor.follow, "Follow");
            if ui
                .button("Clear")
                .on_hover_text("Forget the packets read so far.")
                .clicked()
            {
                monitor.clear();
            }
            action = rows_buttons(ui, monitor.kept() == 0, monitor.pipe.as_ref());
        });
        self.rows_action(ui.ctx(), action, "packets", "packets.txt", |app| {
            &mut app.packets
        });
        let monitor = &mut self.packets;
        ui.horizontal(|ui| {
            ui.label("Filter");
            let menu_width = 170.0;
            ui.add(egui::TextEdit::singleline(&mut monitor.filter).desired_width((ui.available_width() - menu_width).max(120.0)).hint_text("40, VARP_LARGE or !PLAYER_INFO"))
                .on_hover_text("Only these packets, or with a ! in front, all but these. Opcodes or names (see the Names menu), separated by commas or spaces. UNKNOWN stands for the packets without a name.");
            packet_names_menu(ui, monitor);
            if ui.add_enabled(!monitor.filter.is_empty(), egui::Button::new("✖").small()).on_hover_text("Clear the filter").clicked() {
                monitor.filter.clear();
            }
        });
        client_caption(ui, pid);
        let visible = monitor.visible();
        let table = monitor.table();
        ui.weak(format!(
            "{} shown of {} kept{}",
            visible.len(),
            monitor.kept(),
            missed(monitor.dropped)
        ));
        if let Some(error) = &monitor.error {
            error_label(ui, error);
        }
        ui.separator();

        let (font, row_height) = mono_row(ui);
        let sent_color = ui.visuals().hyperlink_color;
        let detail_height = 150.0;
        let mut picked = None;
        let height = (ui.available_height() - detail_height - 12.0).max(row_height * 4.0);
        let log = LogTable {
            id: "packets",
            follow: monitor.follow,
            row_height,
            max_height: Some(height),
            clickable: true,
        };
        let selected = monitor.selected;
        log.show(
            ui,
            &[
                ("Time", narrow(70.0)),
                ("", narrow(24.0)),
                ("Opcode", narrow(56.0)),
                ("Length", narrow(50.0)),
                ("Name", narrow(120.0)),
                ("Fields", narrow(240.0)),
                ("Hex", rest(80.0)),
            ],
            visible.len(),
            |mut row| {
                let packet = &visible[row.index()];
                let def = table.lookup(packet.sent, packet.opcode);
                let colored = |text: String| {
                    let text = mono(text);
                    if packet.sent {
                        text.color(sent_color)
                    } else {
                        text
                    }
                };
                row.set_selected(selected == Some(packet.sequence));
                row.col(|ui| {
                    ui.label(colored(crate::packets::time_text(packet)));
                });
                row.col(|ui| {
                    ui.label(colored(if packet.sent { "→" } else { "←" }.into()));
                });
                row.col(|ui| {
                    ui.label(colored(packet.opcode.to_string()));
                });
                row.col(|ui| {
                    ui.label(colored(packet.length.to_string()));
                });
                row.col(|ui| {
                    ui.label(colored(def.map_or(String::new(), |def| def.name.clone())));
                });
                row.col(|ui| {
                    ui.label(colored(crate::packets::fields_text(def, packet)));
                });
                row.col(|ui| {
                    ui.label(colored(crate::packets::payload_text(packet)));
                });
                if row.response().clicked() {
                    picked = Some(packet.sequence);
                }
            },
        );
        drop(visible);
        if picked.is_some() {
            monitor.selected = picked;
            monitor.follow = false;
        }

        ui.separator();
        match monitor.selected_packet() {
            None => {
                ui.weak("Select a packet to see it in full.");
            }
            Some(packet) => {
                let bytes = crate::packets::payload_bytes(packet);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(if packet.sent { "Sent" } else { "Received" }).strong(),
                    );
                    ui.label(format!(
                        "opcode {}, {} bytes, at {:.3}s",
                        packet.opcode,
                        packet.length,
                        packet.time_ms as f64 / 1000.0
                    ));
                    if (bytes.len() as u32) < packet.length {
                        ui.weak(format!("(first {} kept)", bytes.len()));
                    }
                    if ui.small_button("Copy hex").clicked() {
                        ui.ctx().copy_text(crate::packets::payload_text(packet));
                    }
                });
                let values = crate::packets::leading_values(&bytes);
                if !values.is_empty() {
                    ui.weak(
                        values
                            .iter()
                            .map(|(kind, value)| format!("{kind} {value}"))
                            .collect::<Vec<_>>()
                            .join("  ·  ")
                            + "  (big endian, from the start)",
                    );
                }
                let def = monitor.table().lookup(packet.sent, packet.opcode);
                let decoded = def.map(|def| crate::packet_defs::decode(def, &bytes, packet.length));
                egui::ScrollArea::vertical().id_salt("packet_detail").auto_shrink([false, false]).show(ui, |ui| {
                    if let (Some(def), Some(decoded)) = (def, &decoded) {
                        let fields: Vec<String> = decoded.fields.iter().map(|(label, value)| format!("{label} = {value}")).collect();
                        let title = if def.name.is_empty() { format!("opcode {}", def.opcode) } else { def.name.clone() };
                        ui.label(egui::RichText::new(format!("{title}  {}", fields.join("  ·  "))).font(font.clone()).strong())
                            .on_hover_text(format!("Layout: {}.", monitor.table().source));
                        if !def.has_fields() {
                            ui.weak("The layout of this packet isn't known (its handler reads conditionally or in a loop).");
                        }
                        if !def.touches.is_empty() {
                            ui.weak(format!("the client's handler touches: {}", def.touches.join(", ")));
                        }
                        if let Some(note) = &decoded.note {
                            ui.colored_label(ui.visuals().warn_fg_color, note);
                        }
                    }
                    for line in crate::packets::hex_dump(&bytes) {
                        ui.label(egui::RichText::new(line).font(font.clone()));
                    }
                });
            }
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
        self.spawn(ctx, "Opening console…", move |settings| Event::Done {
            pid,
            what: "Opening the console",
            result: backend::console(&target, &settings),
        });
    }

    fn set_cursor(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        let cursor = self.cursor;
        self.background(ctx, move |settings| Event::Done {
            pid,
            what: "Setting the cursor",
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
        if read.is_varp() {
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

    fn accounts_ui(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_loaded_target();
        let action = crate::script::write(&self.accounts).ui(ui, target.is_some());
        if let Some(crate::accounts::Action::ReadClientName) = action
            && let Some(target) = target.cloned()
        {
            let ctx = ui.ctx().clone();
            self.background(&ctx, move |settings| {
                Event::AccountName(
                    backend::state(&target, &settings).map(|state| state.account_name),
                )
            });
        }
    }

    fn set_display_name(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        let name = self.display_name.trim().to_owned();
        self.spawn(ctx, "Setting the name…", move |settings| {
            let result = backend::set_display_name(&target, &settings, &name);
            Event::Name { pid, name, result }
        });
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            if !matches!(
                event,
                Event::Tiles { .. }
                    | Event::XpState { .. }
                    | Event::AccountName(_)
                    | Event::Packets { .. }
                    | Event::PacketLayouts { .. }
                    | Event::Vars { .. }
                    | Event::ScriptRuns { .. }
                    | Event::Inventory { .. }
                    | Event::Icons { .. }
                    | Event::LayoutRead { .. }
                    | Event::Widgets { .. }
                    | Event::WidgetMouse { .. }
            ) {
                self.busy = None;
            }
            match event {
                Event::Scanned(scan) => {
                    self.targets = scan.targets;
                    self.dll_version = scan.dll_version;
                    for error in scan.errors {
                        self.error(error);
                    }
                    let usable = |t: &&Target| t.client.foreign_bot.is_none() && t.client.is_64bit;
                    if !self.targets.iter().filter(usable).any(|t| Some(t.key()) == self.selected) {
                        self.selected = self.targets.iter().find(usable).map(Target::key);
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
                Event::Restored(result) => match result {
                    Ok(Some(account)) => self.signed_in(account),
                    Ok(None) => self.auth = Auth::SignedOut,
                    Err(error) => {
                        self.auth = Auth::SignedOut;
                        if let ApiError::SignedOut(why) = &error {
                            self.error(format!("Your saved sign-in is no longer valid: {why}"));
                        } else {
                            self.error(format!("Couldn't sign in with the saved session: {error}"));
                        }
                    }
                },
                Event::SignedIn(result) => match result {
                    Ok(account) => self.signed_in(account),
                    Err(error) => {
                        self.auth = Auth::SignedOut;
                        self.error(error);
                    }
                },
                Event::Update(result) => match result {
                    Ok(available) => {
                        let failed = matches!(self.update_state, UpdateState::Failed(_));
                        let busy = self.start_in_progress() || !self.instance.alone();
                        let automatic = available
                            .as_ref()
                            .filter(|a| update::installs_by_itself(a.kind, failed, busy))
                            .map(|a| a.release.clone());
                        self.update = available;
                        if let Some(release) = automatic {
                            self.start_update(ctx, release);
                        }
                    }
                    Err(update::CheckError::Unavailable(error)) => {
                        self.info(format!("Couldn't check for updates: {error}"));
                    }
                    Err(update::CheckError::BadSignature) => {
                        self.error("Update manifest signature is invalid");
                    }
                },
                Event::UpdatePrepared { progress, result } => {
                    let current = matches!(&self.update_state, UpdateState::Downloading(now) if Arc::ptr_eq(now, &progress));
                    match result {
                        Ok(prepared) if current => self.apply_update(ctx, &prepared),
                        Err(error) if current => self.update_failed(error),
                        _ => {}
                    }
                }
                Event::Me(result) => match result {
                    Ok(me) => self.refresh_me(me),
                    Err(ApiError::Unauthorized | ApiError::SignedOut(_)) => {
                        if self.auth.me().is_some() {
                            self.sign_out(Some("your session expired.".into()));
                        }
                    }
                    Err(error) => self.error(format!("Couldn't refresh your account: {error}")),
                },
                Event::Scripts(result) => self.scripts_loaded(result),
                Event::Redeemed(result) => {
                    self.store.redeeming = false;
                    if self.store_changed(ctx, result, "Added to your scripts.") {
                        self.store.code.clear();
                    }
                }
                Event::GrantRemoved { id, result } => {
                    self.store.removing = None;
                    if self.store_changed(ctx, result, "Removed from your account.") {
                        self.confirmed.forget(&id);
                    }
                }
                Event::StoreStarted { pid, script, replaces, run, launch, result } => {
                    self.starting = None;
                    let started = match result {
                        Ok(prepared) => {
                            let started = self.run_prepared(ctx, pid, &script, prepared, run, *launch);
                            if started.is_some()
                                && let Some(old) = replaces
                            {
                                self.close_run(old);
                            }
                            started
                        }
                        Err(error) => {
                            self.start_failed(ctx, &script.title, error);
                            None
                        }
                    };
                    self.queue_entry_started(pid, started);
                }
                Event::QueueManifest { script_id, result } => self.queue_manifest(script_id, result),
                Event::QueueChecked { pid, entry, result } => self.queue_checked(ctx, pid, entry, result),
                Event::ManifestFetched { script, target, result } => {
                    self.starting = None;
                    match result {
                        Ok(fetched) => self.manifest_fetched(ctx, *script, *target, fetched),
                        Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
                        Err(error) => self.start_failed(ctx, &script.title, error),
                    }
                }
                Event::RunQuoted { script, target, run, values, result } => {
                    self.starting = None;
                    match result {
                        Ok(session::Quoted { me, quote, lib }) => {
                            self.refresh_me(me);
                            match lib {
                                Ok(lib) => {
                                    let start = PendingStart { script: *script, target: *target, replaces: None, lib, run, launch: Launch::with(values) };
                                    self.start_at_quote(ctx, start, None, quote);
                                }
                                Err(error) => self.start_failed(ctx, &script.title, error),
                            }
                        }
                        Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
                        Err(error) => self.start_failed(ctx, &script.title, error),
                    }
                }
                Event::RestartQuoted { run, result } => {
                    self.starting = None;
                    match result {
                        Ok(Some((script, session::Quoted { me, quote, lib }))) => {
                            self.refresh_me(me);
                            self.restart_quoted(ctx, run, script, quote, lib);
                        }
                        Ok(None) => self.script_error("Couldn't restart: the script is no longer available to this account."),
                        Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
                        Err(error) => self.script_error(format!("Couldn't restart: {error}")),
                    }
                }
                Event::Heartbeat { session_id, beat } => {
                    if let (Some(microcredits), Some(me)) = (beat.microcredits, self.auth.me_mut()) {
                        me.microcredits = microcredits;
                    }
                    if let Some(store) = self.run_by_session(&session_id).and_then(|run| run.store.as_mut()) {
                        store.rank = beat.rank.unwrap_or(store.rank);
                        store.fee = beat.fee.unwrap_or(store.fee);
                        store.fee_charged = beat.fee_charged.unwrap_or(store.fee_charged);
                    }
                }
                Event::SessionStopped { session_id, reason, sign_out } => {
                    if let Some(run) = self.run_by_session(&session_id) {
                        run.stop();
                        run.error(reason.clone());
                    }
                    if sign_out {
                        self.sign_out(Some(reason));
                    }
                }
                Event::SessionEnded(result) => match result {
                    Ok(ended) => {
                        self.script_info(format!("Session ended, {} charged for the last minutes.", crate::api::models::credits_label(ended.charged as f64)));
                        if self.auth.me().is_some() {
                            self.api_job(ctx, |api| Event::Me(api.me()));
                        }
                    }
                    Err(error) => self.script_error(format!("Couldn't end the session: {error}. The server will end it within 5 minutes.")),
                },
                Event::Done { pid, what, result } => {
                    if let Err(error) = result {
                        self.error(format!("{what} of {pid} failed: {error}"));
                    }
                }
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
                Event::AccountName(result) => crate::script::write(&self.accounts).client_name(result),
                Event::XpState { run: id, result } => {
                    if let Some(run) = self.run_by_id_mut(id) {
                        run.xp_reading = false;
                        run.xp_next = Instant::now() + XP_READ_EVERY;
                        if let Ok(state) = &result {
                            for name in feedback::names_of(state) {
                                if !run.names.contains(&name) {
                                    run.names.push(name);
                                }
                            }
                        }
                        let skills = result.ok().and_then(|state| state.skills).filter(|skills| skills.iter().any(|s| s.xp > 0));
                        if let Some(skills) = skills.filter(|_| run.is_running()) {
                            let skills = skills
                                .into_iter()
                                .map(|s| crate::xp::SkillXp { name: s.name, level: s.real_level as i64, xp: s.xp as i64 })
                                .collect();
                            let elapsed = run.run_time();
                            run.xp.update(skills, elapsed);
                        }
                    }
                }
                Event::RecentSessions(result) => self
                    .feedback
                    .finish(result.map_err(|e| e.to_string()), Instant::now()),
                Event::Scored { session_id, result } => self
                    .feedback
                    .scored(&session_id, result.map(|s| s.score).map_err(|e| e.to_string())),
                Event::Reported { session_id, result } => self.feedback.reported(&session_id, result),
                Event::RunKept(Ok(())) => {}
                Event::RunKept(Err(error)) => {
                    self.error(format!("Couldn't keep the run for bug reports: {error}"))
                }
                Event::Packets { pid, result } => {
                    self.packets.finish(pid, Instant::now(), result);
                    let piped = self.packets.write_pipe();
                    self.report_pipe(piped.map(|()| None));
                }
                Event::PacketLayouts { pid, result } => self.packets.finish_layouts(pid, result),
                Event::ScriptNames(result) => self.script_inspector.finish_names(result),
                Event::ScriptRuns { pid, result } => {
                    self.script_inspector.finish(pid, Instant::now(), result);
                    let piped = self.script_inspector.write_pipe();
                    self.report_pipe(piped.map(|()| None));
                }
                Event::Inventory { pid, result } => self.inventory.finish(pid, Instant::now(), result),
                Event::Icons { pictures, error } => self.icons.finish(ctx, Instant::now(), pictures, error),
                Event::LayoutRead { run, kind, result } => {
                    if let Some(editors) = editors_of(&mut self.runs, Some(run)) {
                        match kind {
                            crate::layouts::Kind::Equipment => editors
                                .equipment
                                .finish_read(result, crate::layouts::apply_equipment),
                            crate::layouts::Kind::Inventory => editors
                                .inventory
                                .finish_read(result, crate::layouts::apply_inventory),
                            crate::layouts::Kind::Prayer => editors
                                .prayer
                                .finish_read(result, crate::layouts::apply_prayers),
                        }
                    }
                }
                Event::ItemsKnown(result) => match result {
                    Ok(items) => {
                        self.asking_items.clear();
                        self.known_items
                            .extend(items.into_iter().map(|(id, name, stacks)| (id, (name, stacks))));
                    }
                    Err(error) => self.error(format!("Couldn't read the script's required items: {error}")),
                },
                Event::Widgets { pid, result } => self.widgets.finish(pid, result),
                Event::WidgetMouse { pid, result } => {
                    self.widgets.mouse_done();
                    match result {
                        Ok(Some((x, y))) if self.widgets.follow_mouse && self.widgets.has_pid(pid) => self.widgets.pick_at(x, y),
                        Ok(_) => {}
                        Err(error) => self.widgets.error = Some(error),
                    }
                }
                Event::Vars { pid, result } => {
                    self.vars.finish(pid, Instant::now(), result, |id| known_varp(id).map(str::to_owned));
                    let piped = self.vars.write_pipe();
                    self.report_pipe(piped.map(|()| None));
                }
                Event::State { pid, read, result } => match result {
                    Ok(state) => self.report_state(pid, read, &state),
                    Err(error) => self.error(format!("Reading the game state of {pid} failed: {error}")),
                },
                Event::Varps { pid, read, result } => match result {
                    Ok(None) => self.info(format!("{pid}: No varps (not logged in).")),
                    Ok(Some(varps)) => {
                        let text = varps_report(read, &varps, self.varps.as_deref());
                        self.varps = Some(varps);
                        if let Some(text) = text {
                            self.info(format!("{pid}: {text}"));
                        }
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
        let field = (ui.available_width() - 190.0).clamp(120.0, 760.0);
        let mut log_lines_changed = false;
        egui::ScrollArea::both().id_salt("settings_page").auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("settings").num_columns(3).max_col_width(field).show(ui, |ui| {
                ui.label("agent DLL");
                ui.add(egui::TextEdit::singleline(&mut self.settings.dll).desired_width(field));
                if ui.button("Browse…").clicked() {
                    pick_file(&mut self.settings.dll, "DLL", "dll");
                }
                ui.end_row();

                ui.label("lib folder").on_hover_text("Optional. Scripts you run from a file read require(\"../lib/...\") from this folder, e.g. your lib checkout. Store scripts always use the downloaded lib.");
                ui.add(egui::TextEdit::singleline(&mut self.settings.lib_dir).desired_width(field).hint_text("empty: the lib beside the script, or the built-in one"));
                if ui.button("Browse…").clicked() {
                    pick_folder(&mut self.settings.lib_dir);
                }
                ui.end_row();

                if !cfg!(windows) {
                    ui.label("injector.exe");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.helper).desired_width(field));
                    if ui.button("Browse…").clicked() {
                        pick_file(&mut self.settings.helper, "Executable", "exe");
                    }
                    ui.end_row();
                }

                ui.label("output lines").on_hover_text("How many lines each output tab keeps. A tab lays out all of its lines again whenever one is added, so more lines cost more CPU and memory.");
                log_lines_changed = ui.add(egui::DragValue::new(&mut self.settings.log_lines).range(backend::LOG_LINES).speed(50.0)).changed();
                ui.end_row();

                ui.label("interface font").on_hover_text("The font of the pages, buttons and labels. The output, the editor, the terminal and the inspectors stay monospace.");
                let before = self.ui_font;
                egui::ComboBox::from_id_salt("ui_font")
                    .selected_text(self.ui_font.label())
                    .show_ui(ui, |ui| {
                        for font in crate::theme::UiFont::ALL {
                            ui.selectable_value(&mut self.ui_font, font, font.label());
                        }
                    });
                if self.ui_font != before {
                    crate::theme::set_ui_font(ui.ctx(), self.ui_font);
                }
                ui.end_row();
            });
            self.whitelist_page
                .ui(ui, &mut crate::script::write(&self.http_whitelist));
            self.transport_settings_ui(ui);
        });
        if log_lines_changed {
            let keep = self.settings.log_lines;
            self.loader_log.set_keep(keep);
            for run in &mut self.runs {
                run.log.set_keep(keep);
            }
        }
    }

    fn transport_settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.strong("Transports");
        ui.weak("What scripts may use to get around. A script that needs one that is off stops with an error saying so.");
        let mut changed = false;
        egui::Grid::new("transport_types")
            .num_columns(2)
            .show(ui, |ui| {
                for (i, (name, label)) in injector::transports::TYPES.iter().enumerate() {
                    let off = &mut self.settings.transports.off;
                    let mut on = !off.iter().any(|o| o == name);
                    if ui.checkbox(&mut on, *label).changed() {
                        if on {
                            off.retain(|o| o != name);
                        } else {
                            off.push((*name).to_owned());
                        }
                        changed = true;
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(6.0);
        let data = crate::transports::status();
        ui.label(format!("Data: {}", data.version));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!data.busy, egui::Button::new("Check for update"))
                .on_hover_text(
                    "Asks for the newest data now. The loader also does this once a day.",
                )
                .clicked()
            {
                crate::transports::update_now();
            }
            if ui
                .add_enabled(
                    data.can_roll_back && !data.busy,
                    egui::Button::new("Roll back"),
                )
                .on_hover_text(
                    "Goes back to the version before this one, if this one causes trouble.",
                )
                .clicked()
            {
                crate::transports::roll_back_now();
            }
            if data.busy {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        });
        if !data.message.is_empty() {
            ui.weak(&data.message);
        }
        if changed {
            let settings = self.settings.transports.clone();
            for run in self.runs.iter_mut().filter(|r| r.is_running()) {
                if let Err(error) = run.send(|s| s.push_transport_settings(&settings)) {
                    run.error(error);
                }
            }
        }
    }

    fn clients_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        ui.horizontal(|ui| {
            ui.heading("Clients");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(self.busy.is_none(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.scan(&ctx);
                }
            });
        });
        ui.add_space(4.0);

        if self.targets.is_empty() {
            if self.busy.is_none() {
                ui.label(
                    egui::RichText::new("No game clients found.").color(crate::theme::COMMENT),
                );
            }
        } else {
            self.step_selected_client(&ctx);
            let show_prefix = !cfg!(windows);
            let gap = 16.0;
            let spacing = ui.spacing().item_spacing;
            let body = egui::TextStyle::Body.resolve(ui.style()).size;
            let bold = egui::FontId::new(body, crate::theme::bold_family());
            let measure = |ui: &egui::Ui, text: &str| crate::theme::text_width(ui, &bold, text);
            let pid_width = self
                .targets
                .iter()
                .map(|t| measure(ui, &t.client.pid.to_string()))
                .fold(measure(ui, "PID"), f32::max);
            let title_width = self
                .targets
                .iter()
                .map(|t| measure(ui, &t.client.title))
                .fold(measure(ui, "Title"), f32::max);
            let mut flex = vec![("Title", title_width)];
            if show_prefix {
                flex.push(("Prefix", CLIENT_PREFIX_WIDTH));
            }
            flex.push(("Status", CLIENT_STATUS_WIDTH));
            let columns = flex.len() + 1;
            let natural: f32 = flex.iter().map(|(_, w)| w).sum();
            let other = pid_width + gap * (columns - 1) as f32 + 16.0;
            let room = ui.available_width() - other;
            let narrow = room < natural;
            if narrow {
                let scale = (room / natural).max(0.0);
                for (_, width) in &mut flex {
                    *width = (*width * scale).max(48.0);
                }
            }
            let widths: Vec<f32> = flex.iter().map(|(_, w)| *w).collect();
            egui::ScrollArea::both()
                .auto_shrink([false, true])
                .max_height((ui.available_height() - 120.0).max(60.0))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        let header = |ui: &mut egui::Ui, width: f32, text: &str| {
                            client_cell(ui, width, 0.0, spacing, |ui| {
                                ui.label(egui::RichText::new(text).color(crate::theme::COMMENT));
                            });
                        };
                        header(ui, pid_width, "PID");
                        for (i, (title, _)) in flex.iter().enumerate() {
                            header(ui, widths[i], title);
                        }
                    });
                    ui.add_space(spacing.y);

                    for (index, target) in self.targets.iter().enumerate() {
                        let client = &target.client;
                        let enabled = client.foreign_bot.is_none();
                        let is_selected = self.selected.as_ref() == Some(&target.key());
                        let active = is_selected && enabled;
                        let background = ui.painter().add(egui::Shape::Noop);
                        let text_color = if active {
                            crate::theme::GREEN
                        } else if !enabled {
                            crate::theme::COMMENT
                        } else {
                            ui.visuals().text_color()
                        };
                        let styled = |text: &str| {
                            let text = egui::RichText::new(text).color(text_color);
                            if active {
                                text.font(bold.clone())
                            } else {
                                text
                            }
                        };
                        let (status, color, tip) = if client.foreign_bot.is_some() {
                            (
                                "Unsupported".to_string(),
                                crate::theme::RED,
                                "Being used by another platform".to_string(),
                            )
                        } else if let Some(module) = &client.loaded {
                            let version = client.version.as_deref().unwrap_or("unknown version");
                            let (text, color) =
                                if self.dll_version.as_deref().is_some_and(|v| v != version) {
                                    (format!("Loaded {version}, outdated"), crate::theme::YELLOW)
                                } else {
                                    (format!("Loaded {version}"), crate::theme::GREEN)
                                };
                            (text.clone(), color, format!("{text}\n{module}"))
                        } else if !client.is_64bit {
                            let text = "32-bit, unsupported".to_string();
                            (text.clone(), crate::theme::RED, text)
                        } else {
                            let text = "Not loaded".to_string();
                            (text.clone(), crate::theme::COMMENT, text)
                        };
                        let bold_status = active || !enabled;
                        let prefix = target.location.label().unwrap_or_default();

                        let painter = ui.painter().clone();
                        let normal = egui::TextStyle::Body.resolve(ui.style());
                        let font = if active { bold.clone() } else { normal.clone() };
                        let title_galley = painter.layout(
                            client.title.clone(),
                            font.clone(),
                            text_color,
                            widths[0],
                        );
                        let status_index = flex.len() - 1;
                        let status_font = if bold_status {
                            bold.clone()
                        } else {
                            normal.clone()
                        };
                        let status_galley = painter.layout(
                            status.clone(),
                            status_font,
                            color,
                            widths[status_index],
                        );
                        let pid_galley = painter.layout_no_wrap(
                            client.pid.to_string(),
                            font.clone(),
                            text_color,
                        );
                        let prefix_galley = show_prefix.then(|| {
                            let lines =
                                title_galley.rows.len().max(status_galley.rows.len()).max(1);
                            let mut job = egui::text::LayoutJob::simple(
                                prefix.clone(),
                                font.clone(),
                                text_color,
                                widths[1],
                            );
                            job.wrap.max_rows = lines;
                            job.wrap.break_anywhere = true;
                            painter.layout_job(job)
                        });
                        let row_height = [
                            pid_galley.size().y,
                            title_galley.size().y,
                            status_galley.size().y,
                            prefix_galley.as_ref().map_or(0.0, |g| g.size().y),
                        ]
                        .into_iter()
                        .fold(0.0, f32::max);
                        let pad = |height: f32| (row_height - height) / 2.0;

                        let row = ui
                            .horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                client_cell(
                                    ui,
                                    pid_width,
                                    pad(pid_galley.size().y),
                                    spacing,
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(styled(&client.pid.to_string()))
                                                .selectable(false),
                                        );
                                    },
                                );
                                client_cell(
                                    ui,
                                    widths[0],
                                    pad(title_galley.size().y),
                                    spacing,
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(styled(&client.title))
                                                .selectable(false),
                                        )
                                        .on_hover_text(&client.exe);
                                    },
                                );
                                if let Some(galley) = prefix_galley {
                                    client_cell(
                                        ui,
                                        widths[1],
                                        pad(galley.size().y),
                                        spacing,
                                        |ui| {
                                            let (rect, response) = ui.allocate_exact_size(
                                                egui::vec2(widths[1], galley.size().y),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().galley(rect.min, galley, text_color);
                                            response.on_hover_text(&prefix);
                                        },
                                    );
                                }
                                client_cell(
                                    ui,
                                    widths[status_index],
                                    pad(status_galley.size().y),
                                    spacing,
                                    |ui| {
                                        let text = egui::RichText::new(&status).color(color);
                                        let text = if bold_status {
                                            text.font(bold.clone())
                                        } else {
                                            text
                                        };
                                        ui.add(egui::Label::new(text).selectable(false))
                                            .on_hover_text(&tip);
                                    },
                                );
                            })
                            .response
                            .rect
                            .expand2(egui::vec2(8.0, 2.0));
                        ui.add_space(spacing.y);
                        let hovered = enabled && ui.rect_contains_pointer(row);
                        if hovered {
                            ui.painter().set(
                                background,
                                egui::Shape::rect_filled(
                                    row,
                                    3.0,
                                    ui.visuals().widgets.hovered.weak_bg_fill,
                                ),
                            );
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            if ui.input(|i| i.pointer.primary_clicked()) {
                                self.selected = Some(target.key());
                                self.run_target.clone_from(&self.selected);
                            }
                        } else if index % 2 == 1 {
                            ui.painter().set(
                                background,
                                egui::Shape::rect_filled(row, 3.0, ui.visuals().faint_bg_color),
                            );
                        }
                    }
                });
        }

        let loaded = self
            .selected_target()
            .is_some_and(|t| t.client.loaded.is_some());
        let can_unload = self.busy.is_none() && loaded;

        ui.add_space(8.0);
        ui.heading("Client options");
        ui.add_space(4.0);
        self.client_buttons_ui(ui);
        ui.add_space(6.0);
        let mut changed = false;
        let mut set_name = false;
        ui.vertical(|ui| {
            egui::Grid::new("client_options").num_columns(2).spacing(egui::vec2(16.0, 6.0)).show(ui, |ui| {
                ui.label("Mouse cursor");
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("cursor_style")
                        .selected_text(cursor_style_label(self.cursor.style))
                        .show_ui(ui, |ui| {
                            for style in CURSOR_STYLES {
                                changed |= ui.selectable_value(&mut self.cursor.style, style, cursor_style_label(style)).changed();
                            }
                        });
                    ui.add_enabled_ui(self.cursor.style != CursorStyle::Off, |ui| {
                        changed |= ui.color_edit_button_srgb(&mut self.cursor.color).changed();
                        changed |= ui.add(egui::DragValue::new(&mut self.cursor.size).range(4..=64).suffix(" px")).changed();
                    });
                });
                ui.end_row();

                ui.label("Display name");
                ui.horizontal(|ui| {
                    let hint = "Shown instead of your display name, in this client only, and kept across logins until reset. Empty puts the real name back.";
                    let field = ui
                        .add_enabled(
                            can_unload,
                            egui::TextEdit::singleline(&mut self.display_name)
                                .hint_text("Real name")
                                .char_limit(injector::state::MAX_NAME_BYTES)
                                .desired_width(140.0),
                        )
                        .on_hover_text(hint);
                    let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let label = if self.display_name.trim().is_empty() { "Reset" } else { "Set" };
                    set_name = ui.add_enabled(can_unload, egui::Button::new(label)).on_hover_text(hint).clicked() || submitted;
                });
                ui.end_row();
            });
        });
        if (set_name || changed)
            && let Some(target) = self.selected_target().cloned()
        {
            if set_name {
                self.set_display_name(&ctx, target.clone());
            }
            if changed {
                self.set_cursor(&ctx, target);
            }
        }
    }
    fn client_buttons_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let target = self
            .selected_target()
            .filter(|t| t.client.foreign_bot.is_none())
            .cloned();
        let client = target.as_ref().map(|t| &t.client);
        let idle = self.busy.is_none();
        let loaded = client.is_some_and(|c| c.loaded.is_some());
        let can_load = idle
            && !self.settings.dll.is_empty()
            && client.is_some_and(|c| {
                c.is_64bit && (c.loaded.is_none() || c.version != self.dll_version)
            });
        let can_unload = idle && loaded;
        let (load_icon, load_label) = if loaded {
            (ICON_UPDATE, "Reload")
        } else {
            (ICON_LOAD, "Load")
        };
        let mut action = None;
        crate::theme::button_group(ui, |ui| {
            let width = icon_button_width(
                ui,
                &[
                    (ICON_LOAD, "Load"),
                    (ICON_UPDATE, "Reload"),
                    (ICON_UNLOAD, "Unload"),
                    (ICON_CONSOLE, "Console"),
                ],
            );
            let load = icon_button(ui, load_icon, load_label, width, can_load)
                .on_hover_text("Loads the DLL from the settings, unloading another build first.");
            if load.clicked() {
                action = Some(ClientAction::Load);
            }
            if icon_button(ui, ICON_UNLOAD, "Unload", width, can_unload).clicked() {
                action = Some(ClientAction::Unload);
            }
            if icon_button(ui, ICON_CONSOLE, "Console", width, can_unload)
                .on_hover_text("Opens agent's console window in the client.")
                .clicked()
            {
                action = Some(ClientAction::Console);
            }
        });
        match (action, target) {
            (Some(ClientAction::Load), Some(target)) => self.load(&ctx, target),
            (Some(ClientAction::Unload), Some(target)) => self.unload(&ctx, target),
            (Some(ClientAction::Console), Some(target)) => self.console(&ctx, target),
            _ => {}
        }
    }

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
        let keys: Vec<String> = self
            .targets
            .iter()
            .filter(|t| t.client.foreign_bot.is_none())
            .map(Target::key)
            .collect();
        if keys.is_empty() {
            return;
        }
        let current = keys.iter().position(|k| Some(k) == self.selected.as_ref());
        let next = match (current, down) {
            (None, true) => 0,
            (None, false) => keys.len() - 1,
            (Some(i), true) => (i + 1).min(keys.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.selected = Some(keys[next].clone());
        self.run_target.clone_from(&self.selected);
    }

    fn selected_target(&self) -> Option<&Target> {
        selected_in(&self.targets, self.selected.as_deref())
    }

    fn selected_loaded_target(&self) -> Option<&Target> {
        loaded_in(&self.targets, self.selected.as_deref())
    }

    fn loaded_target_ui(&self, ui: &mut egui::Ui) -> Option<Target> {
        let target = self.selected_loaded_target();
        match target {
            Some(target) => ui.label(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        ui.add_space(6.0);
        target.cloned()
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
            .max_height(240.0)
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
        for index in 0..self.runs.len() {
            let events = {
                let run = &mut self.runs[index];
                let Some(script) = run.script.as_mut() else {
                    continue;
                };
                if script.stop_ignored(Duration::from_secs(2)) {
                    run.error("The script's process didn't exit after being killed. It will end when the loader closes.");
                } else if script.is_running() && script.stop_requested() {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
                match run.script.as_mut() {
                    Some(script) => script.poll(),
                    None => continue,
                }
            };
            for event in events {
                let finished = matches!(event, ScriptEvent::Finished { .. });
                let run = &mut self.runs[index];
                if let ScriptEvent::Finished { result, stopped } = &event {
                    run.finished = Some((result.clone(), *stopped));
                }
                match event {
                    ScriptEvent::Output(line) => run.info(line),
                    ScriptEvent::Warning(line) => run.log.push_tone(Tone::Warning, line),
                    ScriptEvent::Exit(exit) => {
                        match (&exit.status, &exit.reason) {
                            (injector::form::ExitStatus::Blocked, Some(reason)) => {
                                run.info(format!("The script can't go on: {reason}"));
                            }
                            (injector::form::ExitStatus::Blocked, None) => {
                                run.info("The script can't go on.");
                            }
                            (injector::form::ExitStatus::Done, Some(reason)) => {
                                run.info(reason.clone())
                            }
                            (injector::form::ExitStatus::Done, None) => {}
                        }
                        run.exit = Some(exit);
                    }
                    ScriptEvent::Counters(counters) => {
                        let elapsed = run.run_time();
                        run.counters.update(counters, elapsed);
                    }
                    ScriptEvent::Breaks(plan) => {
                        let now = Instant::now();
                        run.last_breaks = Some((plan.clone(), now));
                        run.breaks.update(plan, now);
                    }
                    ScriptEvent::Finished { stopped: true, .. } => run.info("Script stopped."),
                    ScriptEvent::Finished { result: Ok(()), .. } => run.info("Script finished."),
                    ScriptEvent::Finished {
                        result: Err(error), ..
                    } => run.error(format!("Script failed: {error}")),
                }
                if finished {
                    self.runs[index].ended = Some(Instant::now());
                    self.keep_run_record(ctx, index);
                }
            }
        }
    }

    fn close_run(&mut self, id: u64) {
        self.runs.retain(|r| r.id != id || r.is_running());
        if self.active_run == Some(id) && self.run().is_none() {
            self.active_run = None;
            if self.tab == Tab::Run {
                self.tab = Tab::Sessions;
            }
        }
        self.repair_log_tab();
    }

    fn repair_log_tab(&mut self) {
        if let LogTab::Run(id) = self.log_tab
            && self.run_by_id(id).is_none()
        {
            self.log_tab = LogTab::Loader;
        }
    }

    fn sessions_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sessions");
        if self.runs.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Nothing is running. Start a script from the Scripts page, or the editor.",
                )
                .weak(),
            );
            return;
        }
        let minutes_per_month = self.auth.me().map_or(43_200, |me| me.minutes_per_month);
        ui.add_space(6.0);
        let mut open = None;
        let mut pause = None;
        let mut stop = None;
        let mut close = None;
        let mut restart = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for run in &self.runs {
                    ui.add_space(8.0);
                    crate::theme::card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let client = run.target.as_ref().map_or("no client".to_owned(), |t| {
                            format!("client {}", t.client.pid)
                        });
                        let rate = run
                            .store
                            .as_ref()
                            .map_or(0.0, |store| store.rate_per_hour(minutes_per_month));
                        let billed = run
                            .store
                            .as_ref()
                            .filter(|store| rate > 0.0 || store.fee_charged > 0);
                        ui.horizontal_wrapped(|ui| {
                            let state = run.state();
                            let about = format!("{}, {client}", state.label());
                            crate::theme::status_dot(ui, state.color()).on_hover_text(&about);
                            ui.label(egui::RichText::new(&run.title).strong().size(16.0))
                                .on_hover_text(&about);
                            ui.label(
                                egui::RichText::new(crate::xp::duration(run.run_time())).weak(),
                            );
                            if billed.is_none() {
                                ui.label(egui::RichText::new("free").weak().small())
                                    .on_hover_text(if run.store.is_some() {
                                        "This session costs nothing."
                                    } else {
                                        "Run from the editor, not billed."
                                    });
                            }
                        });
                        if let Some(store) = billed {
                            ui.horizontal_wrapped(|ui| {
                                crate::theme::credits(
                                    ui,
                                    egui::RichText::new(crate::api::models::rate_label(rate))
                                        .strong(),
                                    "/h",
                                )
                                .on_hover_text(EXTRA_SESSION_FEE_HINT);
                                if store.fee > 0 {
                                    ui.label(
                                        egui::RichText::new("includes the extra-session fee")
                                            .weak()
                                            .small(),
                                    )
                                    .on_hover_text(EXTRA_SESSION_FEE_HINT);
                                }
                                if store.fee_charged > 0 {
                                    crate::theme::credits(
                                        ui,
                                        egui::RichText::new(crate::api::models::credits_label(
                                            store.fee_charged as f64,
                                        ))
                                        .small(),
                                        " in fees so far",
                                    );
                                }
                            });
                        }
                        ui.add_space(4.0);
                        crate::theme::button_group(ui, |ui| {
                            let width = icon_button_width(
                                ui,
                                &[
                                    (ICON_OPEN_RUN, "Open"),
                                    PAUSE_BUTTONS[0],
                                    PAUSE_BUTTONS[1],
                                    (ICON_STOP, "Stop"),
                                    (ICON_CANCEL, "Close"),
                                    (ICON_RESTART, "Restart"),
                                ],
                            );
                            if icon_button(ui, ICON_OPEN_RUN, "Open", width, true).clicked() {
                                open = Some(run.id);
                            }
                            let running = run.is_running();
                            let stopping = run.stop_requested();
                            let (pause_icon, pause_text) = run_pause_label(run.is_paused());
                            if icon_button(ui, pause_icon, pause_text, width, running && !stopping)
                                .clicked()
                            {
                                pause = Some(run.id);
                            }
                            if icon_button(ui, ICON_STOP, "Stop", width, running && !stopping)
                                .clicked()
                            {
                                stop = Some(run.id);
                            }
                            if icon_button(ui, ICON_CANCEL, "Close", width, !running)
                                .on_hover_text("Takes the ended run and its log off the list.")
                                .clicked()
                            {
                                close = Some(run.id);
                            }
                            if !running
                                && !run.is_configuring()
                                && restart_button(
                                    ui,
                                    width,
                                    run.store.is_some(),
                                    self.restart_refusal(run).as_deref(),
                                )
                            {
                                restart = Some(run.id);
                            }
                        });
                    });
                }
            });
        if let Some(id) = open {
            self.active_run = Some(id);
            self.tab = Tab::Run;
        }
        if let Some(id) = pause {
            self.toggle_pause_of(id);
        }
        if let Some(id) = stop
            && let Some(run) = self.run_by_id_mut(id)
        {
            run.stop();
        }
        if let Some(id) = close {
            self.close_run(id);
        }
        if let Some(id) = restart {
            let ctx = ui.ctx().clone();
            self.restart_run(&ctx, id);
        }
    }

    fn run_target_ui(&mut self, ui: &mut egui::Ui) -> Option<Target> {
        let free: Vec<(&Target, String)> = self
            .targets
            .iter()
            .filter(|t| self.client_free(t))
            .map(|t| (t, t.key()))
            .collect();
        if !free
            .iter()
            .any(|(_, key)| Some(key) == self.run_target.as_ref())
        {
            self.run_target = free
                .iter()
                .find(|(_, key)| Some(key) == self.selected.as_ref())
                .or_else(|| free.first())
                .map(|(_, key)| key.clone());
        }
        ui.vertical(|ui| {
            ui.label("Client:");
            let label = |t: &Target| format!("client {} ({})", t.client.pid, t.client.title);
            let current = free.iter().find(|(_, key)| Some(key) == self.run_target.as_ref());
            egui::ComboBox::from_id_salt("run_target")
                .selected_text(current.map_or_else(|| "no free client".to_owned(), |(t, _)| label(t)))
                .show_ui(ui, |ui| {
                    for (t, key) in &free {
                        ui.selectable_value(&mut self.run_target, Some(key.clone()), label(t));
                    }
                });
            if free.is_empty() {
                ui.label(egui::RichText::new("Load the agent into a client on the Client page; a client runs one script at a time.").weak());
            }
        });
        ui.add_space(6.0);
        self.run_target_client()
    }

    fn run_target_client(&self) -> Option<Target> {
        self.targets
            .iter()
            .find(|t| Some(t.key()) == self.run_target && self.client_free(t))
            .cloned()
    }

    fn drop_ended_forms(&mut self) {
        for run in self
            .runs
            .iter_mut()
            .filter(|r| !r.is_running() && !r.is_configuring())
        {
            run.form = None;
            run.form_sent = None;
        }
    }

    fn remember_form(&mut self) {
        if let Some(form) = self.run().and_then(|r| r.form.as_ref()) {
            let (title, remembered) = (form.title.clone(), form.remembered());
            self.remembered_forms.insert(title, remembered);
        }
    }

    fn answer_form(&mut self, ctx: &egui::Context, id: u64, outcome: crate::form::Outcome) {
        match outcome {
            crate::form::Outcome::Open => {}
            crate::form::Outcome::Submitted(values) => {
                self.remember_form();
                self.launch_run(ctx, id, values);
            }
            crate::form::Outcome::Cancelled => self.close_run(id),
        }
    }

    fn launch_run(&mut self, ctx: &egui::Context, id: u64, values: serde_json::Value) {
        let Some(run) = self.run_by_id(id).filter(|r| r.is_configuring()) else {
            return;
        };
        let target = run.target.clone();
        match run.source.clone() {
            Some(Source::File(file)) => self.start_file(ctx, id, &file, target.as_ref(), values),
            Some(Source::Store(script)) => match target {
                Some(target) => self.quote_store_script(ctx, script, target, Some(id), values),
                None => self.script_error("Pick a client to run the script on."),
            },
            None => {}
        }
    }

    fn push_form_update(&mut self) {
        let Some(form) = self.run().and_then(|r| r.form.as_ref()) else {
            return;
        };
        let crate::form::Outcome::Submitted(values) =
            form.submitted(self.script_layouts(), &self.known_items)
        else {
            return;
        };
        self.remember_form();
        if let Some(run) = self.run_mut() {
            match run.send(|s| s.push_update(&values)) {
                Ok(()) => {
                    run.form_sent = Some(values);
                    run.info("Settings updated.");
                }
                Err(error) => run.error(error),
            }
        }
    }

    fn running_ui(&mut self, ui: &mut egui::Ui) {
        let Some(run) = self.run() else {
            return;
        };
        let running = run.is_running();
        let paused = running && run.is_paused();
        let stopping = run.stop_requested();
        let is_store = run.store.is_some();
        let waiting = run.is_configuring();
        let form_sent = !waiting && run.form_sent.is_some();
        let can_start = waiting && !self.start_pending();
        let changed_form = form_sent && run.form.as_ref().is_some_and(|f| {
            matches!(f.submitted(self.script_layouts(), &self.known_items), crate::form::Outcome::Submitted(v) if Some(&v) != run.form_sent.as_ref())
        });
        let submit_label = run
            .form
            .as_ref()
            .map_or("Start", |f| f.submit_label())
            .to_owned();
        let has =
            |kind: crate::layouts::Kind| run.form.as_ref().is_some_and(|f| f.has_layout(kind));
        let has_equipment = has(crate::layouts::Kind::Equipment);
        let has_inventory = has(crate::layouts::Kind::Inventory);
        let has_prayer = has(crate::layouts::Kind::Prayer);
        let mut tools_tab = run.tools_tab;
        let run_id = run.id;
        let restartable = !running && !waiting;
        let restart_refusal = self.restart_refusal(run);
        let title_font = egui::FontId::new(
            egui::TextStyle::Heading.resolve(ui.style()).size,
            crate::theme::bold_family(),
        );
        ui.add_space(4.0);
        ui.add(egui::Label::new(egui::RichText::new(&run.title).font(title_font)).truncate());
        if let Some(progress) = self.queue_banner(run_id) {
            ui.label(egui::RichText::new(format!("Queue {progress}")).color(crate::theme::BLUE));
        }
        ui.add_space(8.0);
        let mut outcome = crate::form::Outcome::Open;
        let mut update = false;
        let mut restart = false;
        let width = icon_button_width(
            ui,
            &[
                (ICON_START, &submit_label),
                (ICON_UPDATE, "Update"),
                PAUSE_BUTTONS[0],
                PAUSE_BUTTONS[1],
                (ICON_STOP, "Stop"),
                (ICON_CANCEL, "Cancel"),
                (ICON_RESTART, "Restart"),
            ],
        );
        let buttons = 3 + usize::from(restartable);
        let stack_buttons = crate::theme::button_group_width(buttons, width) > ui.available_width();
        crate::theme::button_group_in(ui, stack_buttons, |ui| {
            if form_sent {
                update = icon_button(
                    ui,
                    ICON_UPDATE,
                    "Update",
                    width,
                    running && !stopping && changed_form,
                )
                .clicked();
            } else if icon_button(ui, ICON_START, &submit_label, width, can_start).clicked() {
                outcome = self.run().and_then(|r| r.form.as_ref()).map_or_else(
                    || crate::form::Outcome::Submitted(serde_json::json!({})),
                    |form| form.submitted(self.script_layouts(), &self.known_items),
                );
            }
            let (pause_icon, pause_text) = run_pause_label(paused);
            let pause = icon_button(
                ui,
                pause_icon,
                pause_text,
                width,
                running && !stopping && !waiting,
            );
            let pause = if is_store {
                pause.on_hover_text("Still billed while paused: stop the script to stop billing.")
            } else {
                pause
            };
            if pause.clicked()
                && let Some(id) = self.active_run
            {
                self.toggle_pause_of(id);
            }
            let (stop_icon, stop_text) = if waiting {
                (ICON_CANCEL, "Cancel")
            } else {
                (ICON_STOP, "Stop")
            };
            if icon_button(
                ui,
                stop_icon,
                stop_text,
                width,
                waiting || (running && !stopping),
            )
            .clicked()
            {
                if waiting {
                    outcome = crate::form::Outcome::Cancelled;
                } else if let Some(run) = self.run_mut() {
                    run.stop();
                }
            }
            if restartable {
                let button = icon_button(
                    ui,
                    ICON_RESTART,
                    "Restart",
                    width,
                    restart_refusal.is_none(),
                );
                restart = match restart_refusal.as_deref() {
                    Some(why) => button.on_disabled_hover_text(why),
                    None => button,
                }
                .clicked();
            }
        });
        let state = self.run().map_or(RunState::Ended, Run::state);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            crate::theme::status_dot(ui, state.color());
            ui.label(egui::RichText::new(state.label()).color(state.color()));
        });
        self.answer_form(ui.ctx(), run_id, outcome);
        if update {
            self.push_form_update();
        }
        if restart {
            let ctx = ui.ctx().clone();
            self.restart_run(&ctx, run_id);
            return;
        }
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        if (tools_tab == ToolsTab::Equipment && !has_equipment)
            || (tools_tab == ToolsTab::Inventory && !has_inventory)
            || (tools_tab == ToolsTab::Prayer && !has_prayer)
            || (tools_tab == ToolsTab::Feedback && !is_store)
        {
            tools_tab = ToolsTab::Settings;
        }
        ui.horizontal_wrapped(|ui| {
            crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Settings, "Settings");
            if has_equipment {
                crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Equipment, "Equipment");
            }
            if has_inventory {
                crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Inventory, "Inventory");
            }
            if has_prayer {
                crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Prayer, "Prayer");
            }
            crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Xp, "Stats");
            crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Breaks, "Breaks");
            if is_store {
                crate::theme::tab_value(ui, &mut tools_tab, ToolsTab::Feedback, "Feedback");
            }
        });
        if let Some(run) = self.run_mut() {
            run.tools_tab = tools_tab;
        }
        divider(ui);
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.add_space(8.0);
        let can_read = self.layout_target().is_some();
        let form_title = self
            .run()
            .and_then(|r| r.form.as_ref())
            .map(|form| form.title.clone())
            .unwrap_or_default();
        let mut read: Option<(crate::layouts::Kind, bool)> = None;
        let unknown: Vec<i32> = self
            .run()
            .and_then(|r| r.form.as_ref())
            .map(|form| {
                [
                    crate::layouts::Kind::Equipment,
                    crate::layouts::Kind::Inventory,
                    crate::layouts::Kind::Prayer,
                ]
                .into_iter()
                .flat_map(|kind| form.rules(kind).unknown(&self.known_items))
                .filter(|id| !self.asking_items.contains(id))
                .collect()
            })
            .unwrap_or_default();
        egui::ScrollArea::both()
            .min_scrolled_height(0.0)
            .id_salt(("tools_tab", tools_tab as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                crate::theme::tab_content(ui, |ui| match tools_tab {
                    ToolsTab::Settings => match run_in(&mut self.runs, self.active_run)
                        .and_then(|r| r.form.as_mut())
                    {
                        Some(form) => {
                            let layouts = self.layouts.get(&form.title).unwrap_or(&EMPTY_LAYOUTS);
                            form.show_inline(ui, layouts);
                        }
                        None => {
                            ui.label(
                                egui::RichText::new("The script isn't asking for settings.").weak(),
                            );
                        }
                    },
                    ToolsTab::Equipment => {
                        let saved = self.layouts.entry(form_title).or_default();
                        if let Some(Run { editors, form, .. }) =
                            run_in(&mut self.runs, self.active_run)
                            && crate::layouts::equipment_ui(
                                ui,
                                &mut editors.equipment,
                                &mut saved.equipment,
                                &mut self.icons,
                                can_read,
                                rules_in(form, crate::layouts::Kind::Equipment),
                                &self.known_items,
                            ) == Some(crate::layouts::Action::Read)
                        {
                            read = Some((crate::layouts::Kind::Equipment, false));
                        }
                    }
                    ToolsTab::Inventory => {
                        let saved = self.layouts.entry(form_title).or_default();
                        if let Some(Run { editors, form, .. }) =
                            run_in(&mut self.runs, self.active_run)
                            && crate::layouts::inventory_ui(
                                ui,
                                &mut editors.inventory,
                                &mut saved.inventory,
                                &mut self.icons,
                                can_read,
                                rules_in(form, crate::layouts::Kind::Inventory),
                                &self.known_items,
                            ) == Some(crate::layouts::Action::Read)
                        {
                            read = Some((crate::layouts::Kind::Inventory, false));
                        }
                    }
                    ToolsTab::Prayer => {
                        let level = self
                            .run()
                            .and_then(|r| r.xp.rows().into_iter().find(|row| row.name == "Prayer"))
                            .and_then(|row| u32::try_from(row.level).ok());
                        let saved = self.layouts.entry(form_title).or_default();
                        let action = run_in(&mut self.runs, self.active_run).and_then(
                            |Run { editors, form, .. }| {
                                crate::layouts::prayer_ui(
                                    ui,
                                    &mut editors.prayer,
                                    &mut saved.prayer,
                                    &mut self.icons,
                                    can_read,
                                    level,
                                    rules_in(form, crate::layouts::Kind::Prayer),
                                )
                            },
                        );
                        match action {
                            Some(crate::layouts::Action::Read) => {
                                read = Some((crate::layouts::Kind::Prayer, false))
                            }
                            Some(crate::layouts::Action::ReadQuick) => {
                                read = Some((crate::layouts::Kind::Prayer, true))
                            }
                            None => {}
                        }
                    }
                    ToolsTab::Xp => self.xp_tracker_ui(ui),
                    ToolsTab::Breaks => self.breaks_ui(ui),
                    ToolsTab::Feedback => self.run_feedback_ui(ui),
                })
            });
        if let Some((kind, quick)) = read {
            self.read_layout(ui.ctx(), kind, quick);
        }
        if !unknown.is_empty() {
            self.describe_items(ui.ctx(), unknown);
        }
    }

    fn describe_items(&mut self, ctx: &egui::Context, ids: Vec<i32>) {
        let Some(target) = self.layout_target().cloned() else {
            return;
        };
        self.asking_items.extend(&ids);
        self.background(ctx, move |settings| {
            Event::ItemsKnown((|| {
                let names = backend::item_names(&target, &settings, &ids)?;
                let stacks = backend::item_stackable(&target, &settings, &ids)?;
                Ok(ids
                    .iter()
                    .zip(names)
                    .zip(stacks)
                    .map(|((&id, name), stacks)| {
                        (id, name.unwrap_or_default(), stacks.unwrap_or(false))
                    })
                    .collect())
            })())
        });
    }

    fn script_layouts(&self) -> &crate::layouts::ScriptLayouts {
        self.run()
            .and_then(|r| r.form.as_ref())
            .and_then(|form| self.layouts.get(&form.title))
            .unwrap_or(&EMPTY_LAYOUTS)
    }

    fn layout_target(&self) -> Option<&Target> {
        self.run()
            .and_then(Run::loaded_target)
            .or_else(|| self.selected_loaded_target())
    }

    fn read_layout(&mut self, ctx: &egui::Context, kind: crate::layouts::Kind, quick: bool) {
        use crate::layouts::{self, Kind, Read};
        let Some(run) = self.active_run else {
            return;
        };
        let Some(target) = self.layout_target().cloned() else {
            if let Some(editors) = editors_of(&mut self.runs, Some(run)) {
                editors.equipment.reading = false;
                editors.inventory.reading = false;
                editors.prayer.reading = false;
            }
            return;
        };
        self.background(ctx, move |settings| {
            let read = || -> Result<Read, String> {
                let container = match kind {
                    Kind::Equipment => crate::layouts::EQUIPMENT_CONTAINER,
                    Kind::Inventory => crate::layouts::INVENTORY_CONTAINER,
                    Kind::Prayer => {
                        let mut keys = Vec::new();
                        if quick {
                            let mask = backend::varbit(
                                &target,
                                &settings,
                                layouts::QUICK_SELECTED_VARBIT,
                            )?
                            .ok_or("the client has no quick prayer selection")?;
                            let selected: Vec<&layouts::PrayerInfo> = layouts::PRAYERS
                                .iter()
                                .filter(|p| mask >> p.quick_bit & 1 == 1)
                                .collect();
                            let mut unlocked = Vec::new();
                            for p in &selected {
                                if let Some(unlock) = p.unlock
                                    && backend::varbit(&target, &settings, unlock)? == Some(1)
                                {
                                    unlocked.push(p.key);
                                }
                            }
                            for p in selected {
                                let replaced = layouts::PRAYERS.iter().any(|u| {
                                    u.replaces == Some(p.key) && unlocked.contains(&u.key)
                                });
                                let locked_upgrade =
                                    p.replaces.is_some() && !unlocked.contains(&p.key);
                                if !replaced && !locked_upgrade {
                                    keys.push(p.key.to_owned());
                                }
                            }
                        } else {
                            for p in &layouts::PRAYERS {
                                if backend::varbit(&target, &settings, p.varbit)? == Some(1) {
                                    keys.push(p.key.to_owned());
                                }
                            }
                        }
                        return Ok(Read::Prayers(keys));
                    }
                };
                let items = backend::item_container(&target, &settings, container)?
                    .ok_or("the client holds no such container")?;
                let describe = |ids: &[i32]| -> Result<crate::layouts::Described, String> {
                    let mut ids: Vec<i32> = ids.iter().copied().filter(|&id| id >= 0).collect();
                    ids.sort_unstable();
                    ids.dedup();
                    if ids.is_empty() {
                        return Ok(Default::default());
                    }
                    item_info(&target, &settings, &ids)
                };
                let ids: Vec<i32> = items.iter().map(|item| item.id).collect();
                let (names, stackable) = describe(&ids)?;
                let mut slots = layouts::items_of(&items, &names, &stackable);
                let mut single: Vec<i32> = slots
                    .iter()
                    .flatten()
                    .filter(|item| !item.stackable)
                    .map(|item| item.id)
                    .collect();
                single.sort_unstable();
                single.dedup();
                if !single.is_empty() {
                    let families: HashMap<i32, injector::config::ItemFamily> = single
                        .iter()
                        .copied()
                        .zip(backend::item_families(&target, &settings, &single)?)
                        .collect();
                    layouts::add_families(slots.iter_mut().flatten(), &families);
                }
                match kind {
                    Kind::Prayer => unreachable!("read above"),
                    Kind::Equipment => {
                        let mut slots: Vec<layouts::EquipmentSlot> = slots
                            .into_iter()
                            .map(|item| {
                                item.map_or(
                                    layouts::EquipmentSlot::Empty,
                                    layouts::EquipmentSlot::Item,
                                )
                            })
                            .collect();
                        slots.resize(layouts::SECONDARY_AMMO_SLOT, layouts::EquipmentSlot::Empty);
                        slots.push(
                            match layouts::read_secondary_ammo(&target, &settings, &items)? {
                                None => layouts::EquipmentSlot::Any,
                                Some(None) => layouts::EquipmentSlot::Empty,
                                Some(Some((id, quantity))) => {
                                    let (names, stacks) = describe(&[id])?;
                                    layouts::EquipmentSlot::Item(layouts::Item::read(
                                        id,
                                        quantity,
                                        names.get(&id).cloned().unwrap_or_default(),
                                        stacks.get(&id).copied().unwrap_or(true),
                                    ))
                                }
                            },
                        );
                        Ok(Read::Equipment {
                            autocast: backend::varbit(
                                &target,
                                &settings,
                                layouts::WEAPON_CATEGORY_VARBIT,
                            )?
                            .is_some_and(|category| {
                                layouts::AUTOCAST_WEAPON_CATEGORIES.contains(&category)
                            }),
                            slots,
                        })
                    }
                    Kind::Inventory => {
                        let has_pouch = slots.iter().flatten().any(layouts::Item::is_rune_pouch);
                        let pouch = if has_pouch {
                            let runes =
                                backend::enum_def(&target, &settings, layouts::POUCH_RUNES_ENUM)?
                                    .ok_or("the cache has no rune pouch enum")?;
                            let mut held = Vec::new();
                            for (rune, amount) in layouts::POUCH_RUNE_VARBITS
                                .iter()
                                .zip(layouts::POUCH_AMOUNT_VARBITS)
                            {
                                let index =
                                    backend::varbit(&target, &settings, *rune)?.unwrap_or(0);
                                let amount =
                                    backend::varbit(&target, &settings, amount)?.unwrap_or(0);
                                let id =
                                    runes.entries.iter().find(|e| e.key == index).and_then(|e| {
                                        match &e.value {
                                            injector::config::Value::Int(id) => {
                                                i32::try_from(*id).ok()
                                            }
                                            injector::config::Value::Str(_) => None,
                                        }
                                    });
                                if let Some(id) = id.filter(|&id| id > 0 && amount > 0) {
                                    held.push((id, amount));
                                }
                            }
                            let ids: Vec<i32> = held.iter().map(|(id, _)| *id).collect();
                            let (names, _) = describe(&ids)?;
                            Some(
                                held.into_iter()
                                    .map(|(id, quantity)| {
                                        layouts::Item::read(
                                            id,
                                            quantity,
                                            names.get(&id).cloned().unwrap_or_default(),
                                            true,
                                        )
                                    })
                                    .collect(),
                            )
                        } else {
                            None
                        };
                        Ok(Read::Inventory { slots, pouch })
                    }
                }
            };
            Event::LayoutRead {
                run,
                kind,
                result: read(),
            }
        });
    }

    fn toggle_pause_of(&mut self, id: u64) {
        let Some(run) = self.run_by_id_mut(id) else {
            return;
        };
        let paused = run.is_paused();
        if let Some(script) = run.script.as_mut() {
            script.set_paused(!paused);
        }
        run.info(if paused {
            "Script resumed."
        } else {
            "Script paused."
        });
    }

    fn editor_run_mut(&mut self) -> Option<&mut Run> {
        self.runs
            .iter_mut()
            .find(|r| r.store.is_none() && r.is_running())
    }

    fn open_script(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Luau script", SCRIPT_FILTER);
        if let Some(dir) = start_dir(&self.script_file.path) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
    }

    fn open_path(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                self.script_file = ScriptFile {
                    path: path.display().to_string(),
                    disk: Some(text.clone()),
                    text,
                };
                self.last_script = Some(self.script_file.path.clone());
                self.info(format!("Opened {}.", path.display()));
            }
            Err(error) => {
                if self.last_script.as_deref() == path.to_str() {
                    self.last_script = None;
                }
                self.error(format!("Cannot open {}: {error}", path.display()));
            }
        }
    }

    fn run_prepared(
        &mut self,
        ctx: &egui::Context,
        pid: u32,
        script: &StoreScript,
        prepared: Prepared,
        configured: Option<u64>,
        launch: Launch,
    ) -> Option<u64> {
        let title = script.title.as_str();
        let session_id = prepared.session.session_id.clone();
        let Some(target) = self
            .targets
            .iter()
            .find(|t| t.client.pid == pid && t.client.loaded.is_some())
            .cloned()
        else {
            self.script_error(format!(
                "The client {pid} is gone, so {title} wasn't started."
            ));
            self.end_session_job(ctx, session_id);
            return None;
        };
        match Script::start_with(
            &self.script_host(ctx),
            &prepared.source,
            &prepared.modules,
            Some(&prepared.entry),
            None,
            Some(&target),
            &self.settings,
            &launch,
        ) {
            Ok(process) => {
                let interval =
                    Duration::from_secs(prepared.session.heartbeat_seconds.clamp(10, 120));
                let closed =
                    self.keep_session(ctx, session_id.clone(), process.process(), interval);
                let store = StoreRun {
                    session_id,
                    script_id: script.id.clone(),
                    community: script.community,
                    closed,
                    lib_version: prepared.lib_version.clone(),
                    price: prepared.session.price,
                    rank: prepared.session.rank,
                    fee: prepared.session.fee,
                    fee_charged: 0,
                };
                let id = match configured
                    .filter(|id| self.run_by_id(*id).is_some_and(Run::is_configuring))
                {
                    Some(id) => id,
                    None => {
                        let id = self.next_run_id();
                        let run = Run::new(id, title.to_owned(), None, None, None, &self.settings);
                        if self.queue_runners.iter().any(|r| r.pid == pid) {
                            self.runs.push(run);
                        } else {
                            self.add_run(run);
                        }
                        id
                    }
                };
                let quiet = self.queue_runners.iter().any(|r| r.pid == pid);
                let run = self.run_by_id_mut(id)?;
                run.script = Some(process);
                run.target = Some(target);
                run.store = Some(store);
                run.source = Some(Source::Store(script.clone()));
                run.form_sent = Some(launch.values);
                run.started = Instant::now();
                if let Some(warning) = &prepared.warning {
                    run.info(format!("Warning: {warning}"));
                }
                run.info(format!(
                    "Running {title} (revision {}).",
                    prepared.session.revision
                ));
                if !quiet {
                    self.active_run = Some(id);
                    self.log_tab = LogTab::Run(id);
                }
                self.feedback.fetched_at = None;
                Some(id)
            }
            Err(error) => {
                self.script_error(format!("Couldn't start {title}: {error}"));
                self.end_session_job(ctx, session_id);
                None
            }
        }
    }

    fn restart_file(&mut self, ctx: &egui::Context, id: u64) {
        let Some(file) = self.run_by_id(id).and_then(|r| r.file.clone()) else {
            return;
        };
        match file.changed_on_disk() {
            Some(disk) => {
                self.reload_prompt = Some(ReloadPrompt {
                    disk,
                    restart: Some(id),
                })
            }
            None => self.rerun_file(ctx, id, file),
        }
    }

    fn rerun_file(&mut self, ctx: &egui::Context, id: u64, file: ScriptFile) {
        let Some(run) = self.run_by_id(id) else {
            return;
        };
        let target = match &run.target {
            Some(old) => match self.free_client(old.client.pid) {
                Some(target) => Some(target.clone()),
                None => {
                    self.script_error(format!(
                        "Couldn't restart {}: its client is gone or busy.",
                        file.name()
                    ));
                    return;
                }
            },
            None => None,
        };
        let values = run.form_sent.clone();
        self.run_file(ctx, file, target.as_ref(), Some(id), values);
    }

    fn run_file(
        &mut self,
        ctx: &egui::Context,
        file: ScriptFile,
        target: Option<&Target>,
        replaces: Option<u64>,
        values: Option<serde_json::Value>,
    ) {
        let name = file.name();
        let path = std::path::Path::new(&file.path);
        let manifest = if path.is_file() {
            match injector::manifest::Manifest::beside(path) {
                Ok(manifest) => manifest.unwrap_or_default(),
                Err(error) => {
                    return self.script_error(format!("Couldn't start the script: {error}"));
                }
            }
        } else {
            injector::manifest::Manifest::default()
        };
        let id = self.next_run_id();
        let mut run = Run::new(
            id,
            name.clone(),
            None,
            target.cloned(),
            None,
            &self.settings,
        );
        run.source = Some(Source::File(file.clone()));
        run.file = Some(file.clone());
        let spec = manifest.settings.clone().filter(|_| values.is_none());
        if let Some(spec) = spec {
            run.form = Some(self.settings_form(spec, &name));
            run.info(format!("Set up {name}, then press Start."));
        } else if let Some(spec) = manifest.settings.clone() {
            run.form = Some(self.settings_form(spec, &name));
        }
        self.add_run(run);
        if let Some(old) = replaces {
            self.close_run(old);
        }
        if manifest.settings.is_none() || values.is_some() {
            let values = values.unwrap_or_else(|| manifest.defaults());
            self.start_file(ctx, id, &file, target, values);
        }
    }

    fn start_file(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        file: &ScriptFile,
        target: Option<&Target>,
        values: serde_json::Value,
    ) {
        let name = file.name();
        let path = Some(std::path::Path::new(&file.path)).filter(|p| p.is_file());
        let lib = Some(std::path::PathBuf::from(self.settings.lib_dir.trim()))
            .filter(|l| path.is_some() && !l.as_os_str().is_empty());
        if let Some(lib) = lib.as_ref().filter(|l| !l.is_dir()) {
            self.script_error(format!("Couldn't start the script: the lib folder {} doesn't exist. Fix it in the settings.", lib.display()));
            return;
        }
        let launch = Launch::with(values.clone());
        match Script::start(
            &self.script_host(ctx),
            &file.text,
            path,
            lib.as_deref(),
            target,
            &self.settings,
            &launch,
        ) {
            Ok(script) => {
                if let Some(run) = self.run_by_id_mut(id) {
                    run.script = Some(script);
                    run.form_sent = Some(values);
                    run.started = Instant::now();
                    run.info(format!("Running {name}…"));
                }
            }
            Err(error) => {
                if let Some(run) = self.run_by_id_mut(id) {
                    run.error(format!("Couldn't start the script: {error}"));
                }
            }
        }
    }

    fn reload_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &self.reload_prompt else {
            return;
        };
        let restart = prompt.restart;
        let file = match restart {
            Some(id) => match self.run_by_id(id).and_then(|r| r.file.clone()) {
                Some(file) => file,
                None => {
                    self.reload_prompt = None;
                    return;
                }
            },
            None => self.script_file.clone(),
        };
        let name = file.name();
        let unsaved =
            restart.is_none() && file.disk.as_ref().is_some_and(|known| known != &file.text);
        let mut choice: Option<Option<bool>> = None;
        egui::Window::new("Script changed on disk")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(if restart.is_some() {
                    format!(
                        "{name} changed on disk since it last ran. Reload it before restarting?"
                    )
                } else {
                    format!("{name} changed on disk since it was loaded. Reload it before running?")
                });
                if unsaved {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Reloading discards your unsaved edits.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Reload and run").clicked() {
                        choice = Some(Some(true));
                    }
                    if ui
                        .button(if restart.is_some() {
                            "Run previous version"
                        } else {
                            "Run editor version"
                        })
                        .clicked()
                    {
                        choice = Some(Some(false));
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(None);
                    }
                });
            });
        let Some(choice) = choice else { return };
        let Some(ReloadPrompt { disk, .. }) = self.reload_prompt.take() else {
            return;
        };
        let Some(reload) = choice else { return };
        let mut file = file;
        file.disk = Some(disk.clone());
        if reload {
            file.text = disk;
        }
        match restart {
            Some(id) => self.rerun_file(ctx, id, file),
            None => {
                self.script_file = file.clone();
                let target = self.run_target_client();
                self.run_file(ctx, file, target.as_ref(), None, None);
            }
        }
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
            .add_filter("Luau script", SCRIPT_FILTER)
            .set_file_name(self.script_file.name());
        if let Some(dir) = start_dir(&self.script_file.path) {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        self.script_file.path = path.display().to_string();
        self.last_script = Some(self.script_file.path.clone());
        self.save_script();
    }

    fn file_buttons(&mut self, ui: &mut egui::Ui, running: bool, width: f32) {
        if icon_button(ui, ICON_OPEN, "Open…", width, !running).clicked() {
            self.open_script();
        }
        let last = self.last_script.as_deref();
        let open_last = icon_button(
            ui,
            ICON_OPEN_LAST,
            "Open last",
            width,
            !running && last.is_some(),
        );
        let open_last = match last {
            Some(path) => open_last.on_hover_text(path),
            None => open_last,
        };
        if open_last.clicked()
            && let Some(path) = self.last_script.clone()
        {
            self.open_path(std::path::Path::new(&path));
        }
        if icon_button(
            ui,
            ICON_SAVE,
            "Save",
            width,
            !running && !self.script_file.path.is_empty(),
        )
        .clicked()
        {
            self.save_script();
        }
        if icon_button(ui, ICON_SAVE_AS, "Save as…", width, !running).clicked() {
            self.save_script_as();
        }
    }

    fn run_buttons(
        &mut self,
        ui: &mut egui::Ui,
        running: bool,
        paused: bool,
        target: Option<&Target>,
        width: f32,
    ) {
        let can_run = !running && !self.script_file.text.trim().is_empty();
        if icon_button(ui, ICON_START, "Run", width, can_run).clicked() {
            match self.script_file.changed_on_disk() {
                Some(disk) => {
                    self.reload_prompt = Some(ReloadPrompt {
                        disk,
                        restart: None,
                    })
                }
                None => self.run_file(ui.ctx(), self.script_file.clone(), target, None, None),
            }
        }
        let (pause_icon, pause_text) = run_pause_label(paused);
        if icon_button(ui, pause_icon, pause_text, width, running).clicked()
            && let Some(id) = self.editor_run_mut().map(|r| r.id)
        {
            self.toggle_pause_of(id);
        }
        if icon_button(ui, ICON_STOP, "Stop", width, running).clicked()
            && let Some(run) = self.editor_run_mut()
        {
            run.stop();
        }
    }

    fn scripts_ui(&mut self, ui: &mut egui::Ui) {
        let (running, paused) = self
            .editor_run_mut()
            .map_or((false, false), |r| (true, r.is_paused()));

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

        let target = self.run_target_ui(ui);

        let width = icon_button_width(
            ui,
            &[
                (ICON_START, "Run"),
                PAUSE_BUTTONS[0],
                PAUSE_BUTTONS[1],
                (ICON_STOP, "Stop"),
                (ICON_OPEN, "Open…"),
                (ICON_OPEN_LAST, "Open last"),
                (ICON_SAVE, "Save"),
                (ICON_SAVE_AS, "Save as…"),
            ],
        );
        let group_gap = 8.0;
        let available = ui.available_width();
        let run_group = crate::theme::button_group_width(3, width);
        let file_group = crate::theme::button_group_width(4, width);
        let stack_groups =
            run_group + ui.spacing().item_spacing.x + group_gap + file_group > available;
        let stack_buttons = run_group.max(file_group) > available;
        fit_row(ui, !stack_groups, |ui| {
            crate::theme::button_group_in(ui, stack_buttons, |ui| {
                self.run_buttons(ui, running, paused, target.as_ref(), width)
            });
            ui.add_space(group_gap);
            crate::theme::button_group_in(ui, stack_buttons, |ui| {
                self.file_buttons(ui, running, width)
            });
        });
        let status = match (running, paused) {
            (true, true) => "paused",
            (true, false) => "running",
            (false, _) => "not running",
        };
        ui.label(format!("{}, {status}", self.script_file.name()))
            .on_hover_text(&self.script_file.path);
        ui.add_space(4.0);
        let matches = if self.find.open {
            let matches = self.find_ui(ui);
            ui.add_space(4.0);
            matches
        } else {
            Vec::new()
        };

        let size = ui.available_size();
        let margin = egui::Margin::symmetric(4, 2);
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let (row_height, digit_width) =
            ui.fonts_mut(|fonts| (fonts.row_height(&font), fonts.glyph_width(&font, '0')));
        self.fold
            .sync(&self.script_file.path, &self.script_file.text);
        let lines = self.fold.line_count();
        let arrow_width = row_height;
        let gutter_width =
            digit_width * lines.to_string().len() as f32 + 3.0 * margin.leftf() + arrow_width;
        let gutter_color = ui.visuals().weak_text_color();
        if self.find.open
            && self.find.jump
            && let Some(&at) = matches.get(self.find.current)
        {
            self.fold
                .reveal(self.script_file.text[..at].matches('\n').count());
        }
        let view = self.fold.view(&self.script_file.text);
        let text_width = (self.fold.longest_line() as f32 * digit_width + margin.sum().x + 8.0)
            .max(size.x - gutter_width - ui.spacing().item_spacing.x);
        let gutter_height = size
            .y
            .max(view.visible.len() as f32 * row_height + margin.sum().y);
        let highlighter = &mut self.highlighter;
        let hidden = &view.hidden;
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap_width: f32| {
            let mut job = highlighter.layout(ui, text.as_str(), hidden);
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let mut select_line = None;
                    let (gutter, response) = ui.allocate_exact_size(
                        egui::vec2(gutter_width, gutter_height),
                        egui::Sense::click(),
                    );
                    ui.painter()
                        .rect_filled(gutter, 0.0, ui.visuals().faint_bg_color);
                    let row_top =
                        |row: usize| gutter.top() + margin.topf() + row as f32 * row_height;
                    let first = ((ui.clip_rect().top() - gutter.top() - margin.topf()) / row_height)
                        .floor()
                        .max(0.0) as usize;
                    let last = ((ui.clip_rect().bottom() - gutter.top() - margin.topf())
                        / row_height)
                        .ceil()
                        .max(0.0) as usize;
                    let arrow_center = |row: usize| {
                        egui::pos2(
                            gutter.right() - arrow_width / 2.0,
                            row_top(row) + row_height / 2.0,
                        )
                    };
                    let hover_row = response
                        .hover_pos()
                        .map(|pos| ((pos.y - gutter.top() - margin.topf()) / row_height).floor())
                        .filter(|&row| row >= 0.0 && (row as usize) < view.visible.len())
                        .map(|row| row as usize);
                    let hover_fill = ui.visuals().widgets.hovered.bg_fill;
                    if let Some(row) = hover_row {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(gutter.left(), row_top(row)),
                            egui::vec2(gutter_width, row_height),
                        );
                        ui.painter().rect_filled(rect, 0.0, hover_fill);
                    }
                    for row in first..last.min(view.visible.len()) {
                        let line = view.visible[row];
                        let number_color = if hover_row == Some(row) {
                            ui.visuals().text_color()
                        } else {
                            gutter_color
                        };
                        ui.painter().text(
                            egui::pos2(
                                gutter.right() - arrow_width - 2.0 * margin.leftf(),
                                row_top(row),
                            ),
                            egui::Align2::RIGHT_TOP,
                            line + 1,
                            font.clone(),
                            number_color,
                        );
                        if self.fold.is_foldable(line) {
                            paint_arrow(
                                ui.painter(),
                                arrow_center(row),
                                row_height * 0.3,
                                self.fold.is_folded(line),
                                gutter_color,
                            );
                        }
                    }
                    if response.clicked()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let row = ((pos.y - gutter.top() - margin.topf()) / row_height).floor();
                        if row >= 0.0
                            && let Some(&line) = view.visible.get(row as usize)
                        {
                            if pos.x >= gutter.right() - arrow_width - margin.leftf()
                                && self.fold.is_foldable(line)
                            {
                                self.fold.toggle(line);
                            } else {
                                select_line = Some(line);
                            }
                        }
                    }
                    let mut output = egui::TextEdit::multiline(&mut self.script_file.text)
                        .code_editor()
                        .margin(margin)
                        .desired_width(text_width)
                        .min_size(egui::vec2(text_width, size.y))
                        .layouter(&mut layouter)
                        .show(ui);
                    if let Some(row) = hover_row {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(output.response.rect.left(), row_top(row)),
                            egui::vec2(output.response.rect.width(), row_height),
                        );
                        ui.painter()
                            .rect_filled(rect, 0.0, hover_fill.gamma_multiply(0.4));
                    }
                    if let Some(line) = select_line {
                        let (start, end) = line_char_range(&self.script_file.text, line);
                        let range = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(start),
                            egui::text::CCursor::new(end),
                        );
                        output.state.cursor.set_char_range(Some(range));
                        output.state.clone().store(ui.ctx(), output.response.id);
                        output.response.request_focus();
                    }
                    if let Some(range) = output.state.cursor.char_range() {
                        self.fold
                            .follow_cursor(&self.script_file.text, range.primary.index.0);
                    }
                    let on_screen = first.saturating_sub(1)..last + 1;
                    if self.find.open {
                        paint_matches(
                            ui,
                            &output,
                            &self.script_file.text,
                            &matches,
                            &mut self.find,
                            &view.visible,
                            on_screen.clone(),
                        );
                    }
                    let mut walk = TextWalk::new(&self.script_file.text);
                    for &(line, header_end) in &view.headers {
                        if !view
                            .visible
                            .binary_search(&line)
                            .is_ok_and(|row| on_screen.contains(&row))
                        {
                            continue;
                        }
                        let at = output
                            .galley
                            .pos_from_cursor(egui::text::CCursor::new(walk.advance(header_end).0));
                        let badge = egui::Rect::from_min_size(
                            egui::pos2(
                                output.galley_pos.x + at.max.x + 6.0,
                                output.galley_pos.y + at.min.y + 1.0,
                            ),
                            egui::vec2(2.0 * digit_width + 8.0, row_height - 2.0),
                        );
                        let click = ui.interact(
                            badge,
                            ui.id().with(("fold_badge", line)),
                            egui::Sense::click(),
                        );
                        let fill = if click.hovered() {
                            ui.visuals().widgets.hovered.bg_fill
                        } else {
                            ui.visuals().widgets.inactive.bg_fill
                        };
                        ui.painter().rect_filled(badge, 3.0, fill);
                        ui.painter().text(
                            badge.center(),
                            egui::Align2::CENTER_CENTER,
                            "...",
                            font.clone(),
                            gutter_color,
                        );
                        if click.on_hover_text("Click to unfold").clicked() {
                            self.fold.toggle(line);
                        }
                    }
                });
            });
    }

    fn find_ui(&mut self, ui: &mut egui::Ui) -> Vec<usize> {
        let matches = find_matches(
            &self.script_file.text,
            &self.find.query,
            self.find.case_sensitive,
        );
        let count = matches.len();
        self.find.current = self.find.current.min(count.saturating_sub(1));

        let mut step = None;
        let mut replace_current = false;
        let mut replace_all = false;
        let mut query_changed = false;

        ui.horizontal(|ui| {
            let query = ui.add(
                egui::TextEdit::singleline(&mut self.find.query)
                    .hint_text("Find")
                    .desired_width(200.0),
            );
            if std::mem::take(&mut self.find.focus) {
                query.request_focus();
            }
            if query.changed() {
                self.find.current = 0;
                self.find.jump = true;
                query_changed = true;
            }
            if (query.has_focus() || query.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                step = Some(ui.input(|i| i.modifiers.shift));
            }
            if ui
                .add_enabled(count > 0, egui::Button::new("Previous"))
                .on_hover_text("Shift+Enter")
                .clicked()
            {
                step = Some(true);
            }
            if ui
                .add_enabled(count > 0, egui::Button::new("Next"))
                .on_hover_text("Enter")
                .clicked()
            {
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
            if ui
                .checkbox(&mut self.find.case_sensitive, "Match case")
                .changed()
            {
                self.find.current = 0;
                self.find.jump = true;
                query_changed = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").on_hover_text("Escape").clicked() {
                    self.find.open = false;
                }
            });
        });

        if self.find.replace {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.find.replacement)
                        .hint_text("Replace with")
                        .desired_width(200.0),
                );
                if ui
                    .add_enabled(count > 0, egui::Button::new("Replace"))
                    .clicked()
                {
                    replace_current = true;
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Replace all"))
                    .clicked()
                {
                    replace_all = true;
                }
            });
        }

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.find.open = false;
        }
        if let Some(back) = step.filter(|_| count > 0) {
            let current = self.find.current;
            self.find.current = if back {
                (current + count - 1) % count
            } else {
                (current + 1) % count
            };
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
            find_matches(
                &self.script_file.text,
                &self.find.query,
                self.find.case_sensitive,
            )
        } else if let (true, Some(&at)) = (replace_current, matches.get(self.find.current)) {
            let replacement = self.find.replacement.clone();
            let text = &mut self.script_file.text;
            text.replace_range(at..at + self.find.query.len(), &replacement);
            let after = at + replacement.len();
            let next = find_matches(text, &self.find.query, self.find.case_sensitive);
            self.find.current = next.iter().position(|&start| start >= after).unwrap_or(0);
            self.find.jump = true;
            next
        } else if query_changed {
            find_matches(
                &self.script_file.text,
                &self.find.query,
                self.find.case_sensitive,
            )
        } else {
            matches
        }
    }

    fn terminal_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.terminal.is_none() {
            let dir = crate::workspace::prepare(&self.settings)
                .map_err(|error| {
                    self.error(format!("Couldn't prepare the agent workspace: {error}"))
                })
                .ok();
            self.terminal = Some(crate::terminal::Terminal::new(&ctx, dir));
        }
        if let Some(terminal) = &mut self.terminal {
            terminal.ui(ui);
        }
    }

    fn development_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in DevTab::ALL {
                ui.selectable_value(&mut self.dev_tab, tab, label);
            }
        });
        ui.separator();
        crate::theme::tab_content(ui, |ui| match self.dev_tab {
            DevTab::Tools => self.dev_tools_ui(ui),
            DevTab::Inspectors => self.inspectors_ui(ui),
        });
    }

    fn widget_inspector_showing(&self) -> bool {
        self.tab == Tab::Development
            && self.dev_tab == DevTab::Inspectors
            && self.inspector_tab == InspectorTab::Widget
    }

    fn inspectors_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in InspectorTab::ALL {
                ui.selectable_value(&mut self.inspector_tab, tab, label);
            }
        });
        ui.separator();
        crate::theme::tab_content(ui, |ui| match self.inspector_tab {
            InspectorTab::Widget => self.widgets_ui(ui),
            InspectorTab::Var => self.vars_ui(ui),
            InspectorTab::Script => self.script_log_ui(ui),
            InspectorTab::Inventory => self.inventory_ui(ui),
            InspectorTab::Packet => self.packets_ui(ui),
        });
    }

    fn dev_tools_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Game state");
        ui.add_space(4.0);
        self.state_ui(ui);
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);
        ui.heading("Overlays");
        ui.add_space(4.0);
        let gap = 24.0;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let widest = crate::theme::widest(ui, &font, &DEV_TOGGLES)
            + ui.spacing().icon_width
            + ui.spacing().icon_spacing;
        let per_group = DEV_TOGGLES.len().div_ceil(DEV_COLUMNS);
        let groups: Vec<std::ops::Range<usize>> = (0..DEV_TOGGLES.len())
            .step_by(per_group)
            .map(|start| start..(start + per_group).min(DEV_TOGGLES.len()))
            .collect();
        let beside = (((ui.available_width() + gap) / (widest + gap)).floor() as usize)
            .clamp(1, groups.len());
        for (row, row_groups) in groups.chunks(beside).enumerate() {
            if row > 0 {
                ui.add_space(8.0);
            }
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for group in row_groups {
                    ui.vertical(|ui| {
                        ui.set_width(widest);
                        for index in group.clone() {
                            ui.add_enabled(
                                DEV_LAYERS[index].is_some(),
                                egui::Checkbox::new(
                                    &mut self.dev_toggles[index],
                                    DEV_TOGGLES[index],
                                ),
                            )
                            .on_disabled_hover_text("Not ported yet.");
                        }
                    });
                }
            });
        }
    }

    fn xp_tracker_ui(&self, ui: &mut egui::Ui) {
        use crate::{
            counters::number,
            xp::{duration, thousands},
        };
        let Some(run) = self
            .run()
            .filter(|r| !(r.xp.is_empty() && r.counters.is_empty()))
        else {
            ui.weak("Waiting for the first read of the client's skills.");
            return;
        };
        let elapsed = run.run_time();
        ui.label(format!(
            "{} · {} xp",
            duration(elapsed),
            thousands(run.xp.total())
        ));
        let rows = run.xp.rows();
        ui.vertical(|ui| {
            if rows.is_empty() {
                ui.weak("No experience gained yet.");
            } else {
                egui::Grid::new("xp_grid")
                    .num_columns(5)
                    .spacing([12.0, 2.0])
                    .show(ui, |ui| {
                        for head in ["Skill", "Gained", "XP/hr", "Level", "To level"] {
                            ui.weak(head);
                        }
                        ui.end_row();
                        for row in &rows {
                            ui.label(&row.name);
                            ui.label(thousands(row.gained));
                            ui.label(row.per_hour.map_or_else(|| "-".into(), thousands));
                            ui.label(row.level.to_string());
                            ui.label(row.to_level.map_or_else(|| "-".into(), duration));
                            ui.end_row();
                        }
                    });
            }
            let counters = run.counters.rows();
            if !counters.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                egui::Grid::new("counters_grid")
                    .num_columns(4)
                    .spacing([12.0, 2.0])
                    .show(ui, |ui| {
                        for head in ["", "Total", "Per hour", ""] {
                            ui.weak(head);
                        }
                        ui.end_row();
                        for row in &counters {
                            ui.label(&row.label);
                            ui.label(number(row.total));
                            ui.label(row.per_hour.map_or_else(|| "-".into(), number));
                            ui.label(row.ratio.as_deref().unwrap_or(""));
                            ui.end_row();
                        }
                    });
            }
        });
    }

    fn breaks_ui(&mut self, ui: &mut egui::Ui) {
        use crate::{breaks::clock, xp::duration};
        let running = self.run().is_some_and(Run::is_running);
        let mut skip: Option<(&'static str, u32)> = None;
        let mut update = false;
        ui.vertical(|ui| {
            crate::breaks::header(ui);
            ui.add_space(4.0);
            let spacing = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal(|ui| {
                crate::theme::tab_value(ui, &mut self.breaks_tab, BreaksTab::Breaks, "Breaks");
                crate::theme::tab_value(ui, &mut self.breaks_tab, BreaksTab::Sleeps, "Sleeps");
            });
            let sleeps = self.breaks_tab == BreaksTab::Sleeps;
            crate::theme::tab_panel(ui, |ui| {
                ui.spacing_mut().item_spacing.y = spacing;
                if sleeps {
                    crate::breaks::sleeps_editor(ui, &mut self.settings.antiban);
                } else {
                    crate::breaks::breaks_editor(ui, &mut self.settings.antiban);
                }
            });
            ui.spacing_mut().item_spacing.y = spacing;
            ui.add_space(6.0);
            let valid = self.settings.antiban.validate();
            if let Err(why) = &valid {
                ui.colored_label(egui::Color32::LIGHT_RED, why);
            }
            if running {
                let dirty = self
                    .run()
                    .is_some_and(|r| r.antiban_sent != self.settings.antiban);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    update = ui
                        .add_enabled(dirty && valid.is_ok(), egui::Button::new("Update"))
                        .on_hover_text("Send these to the running script")
                        .clicked();
                    if dirty {
                        ui.weak("Not sent to the script yet");
                    }
                });
                ui.add_space(6.0);
            } else {
                ui.weak("Used by the scripts you start. It is saved as you edit.");
                return;
            }
            let Some(view) = self.run().and_then(|r| r.breaks.view(Instant::now())) else {
                ui.add_space(6.0);
                ui.weak("The script hasn't reported any breaks yet.");
                return;
            };
            ui.ctx().request_repaint_after(Duration::from_secs(1));
            ui.separator();
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Queued").strong());
            ui.add_space(4.0);
            if let Some(active) = &view.active {
                ui.label(
                    egui::RichText::new(format!(
                        "On a {} · {} left",
                        active.kind,
                        duration(active.remaining)
                    ))
                    .strong(),
                );
            }
            if !sleeps && !view.breaks.is_empty() {
                ui.add_space(4.0);
                egui::Grid::new("breaks_grid")
                    .num_columns(3)
                    .spacing([12.0, 2.0])
                    .show(ui, |ui| {
                        for row in &view.breaks {
                            ui.label(format!("in {}", duration(row.remaining)));
                            ui.label(format!("~{}", duration(row.length)));
                            if ui.small_button("Skip next").clicked() {
                                skip = Some(("break", row.index));
                            }
                            ui.end_row();
                        }
                    });
            }
            if sleeps && !view.sleeps.is_empty() {
                ui.add_space(4.0);
                egui::Grid::new("sleeps_grid")
                    .num_columns(4)
                    .spacing([12.0, 2.0])
                    .show(ui, |ui| {
                        for row in &view.sleeps {
                            ui.label(row.at.map_or_else(|| "-".into(), clock));
                            ui.label(format!("in {}", duration(row.remaining)));
                            ui.label(format!("~{}", duration(row.length)));
                            if ui.small_button("Skip next").clicked() {
                                skip = Some(("sleep", row.index));
                            }
                            ui.end_row();
                        }
                    });
            }
            if (sleeps && view.sleeps.is_empty()) || (!sleeps && view.breaks.is_empty()) {
                ui.weak("Nothing is scheduled.");
            }
        });
        if update {
            let config = self.settings.antiban.clone();
            if let Some(run) = self.run_mut() {
                match run.send(|s| s.push_antiban(&config)) {
                    Ok(()) => {
                        run.antiban_sent = config;
                        run.info("Breaks updated.");
                    }
                    Err(error) => run.error(error),
                }
            }
        }
        if let Some((kind, index)) = skip
            && let Some(run) = self.run_mut()
        {
            let result = run.send(|s| s.skip_next(kind, index));
            run.report(result, &format!("Skipping the next {kind}."));
        }
    }

    fn log_ref(&self, tab: LogTab) -> &Log {
        match tab {
            LogTab::Run(id) => self.run_by_id(id).map_or(&self.loader_log, |run| &run.log),
            LogTab::Loader => &self.loader_log,
        }
    }

    fn log_of(&mut self, tab: LogTab) -> &mut Log {
        match tab {
            LogTab::Run(id) => match run_in(&mut self.runs, Some(id)) {
                Some(run) => &mut run.log,
                None => &mut self.loader_log,
            },
            LogTab::Loader => &mut self.loader_log,
        }
    }

    fn log_controls_ui(&mut self, ui: &mut egui::Ui) {
        if let LogTab::Run(_) = self.log_tab {
            let tab = self.log_tab;
            let log = self.log_of(tab);
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
        }
        let tab = self.log_tab;
        let (mut save, mut pipe) = (false, false);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::theme::button_group(ui, |ui| {
                let log = self.log_of(tab);
                let filled = !log.is_empty();
                if slide_button(ui, ICON_CLEAR, "Clear", filled, false).clicked() {
                    log.clear();
                }
                if slide_button(ui, ICON_COPY, "Copy all", filled, false).clicked() {
                    ui.ctx().copy_text(log.text.clone());
                }
                save = slide_button(ui, ICON_SAVE, "Save", filled, false).clicked();
                let piping = log
                    .pipe
                    .as_ref()
                    .map(|pipe| pipe.path().display().to_string());
                let button = slide_button(ui, ICON_PIPE, "Pipe", true, piping.is_some());
                pipe = match piping {
                    Some(path) => button.on_hover_text(format!("Appending to {path}")),
                    None => button,
                }
                .clicked();
            });
        });
        if save {
            let text = self.log_of(tab).text.clone();
            self.save_text("output", "output.txt", text);
        }
        if pipe {
            let toggled = crate::pipe::toggle(&mut self.log_of(tab).pipe, "output.txt", 0);
            self.report_pipe(toggled);
        }
    }

    fn save_text(&mut self, what: &str, file_name: &str, text: String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text", &["txt"])
            .set_file_name(file_name)
            .save_file()
        else {
            return;
        };
        let shown = path.display().to_string();
        match std::fs::write(&path, text) {
            Ok(()) => self.info(format!("Saved {what} to {shown}.")),
            Err(error) => self.error(format!("Cannot save {what} to {shown}: {error}")),
        }
    }

    fn rows_action<T: crate::pipe::Rows>(
        &mut self,
        ctx: &egui::Context,
        action: Option<RowsAction>,
        what: &str,
        file_name: &str,
        rows: fn(&mut Self) -> &mut T,
    ) {
        match action {
            None => {}
            Some(RowsAction::Copy) => ctx.copy_text(rows(self).shown_text()),
            Some(RowsAction::Save) => {
                let text = rows(self).shown_text();
                self.save_text(what, file_name, text);
            }
            Some(RowsAction::Pipe) => {
                let rows = rows(self);
                let next = rows.end();
                let toggled = crate::pipe::toggle(rows.pipe(), file_name, next);
                self.report_pipe(toggled);
            }
        }
    }

    fn report_pipe(&mut self, toggled: Result<Option<String>, String>) {
        match toggled {
            Ok(Some(message)) => self.info(message),
            Ok(None) => {}
            Err(error) => self.error(error),
        }
    }

    fn shown_run_log(&self) -> Option<&Run> {
        let id = match self.log_tab {
            LogTab::Run(id) => Some(id),
            LogTab::Loader => self.active_run,
        };
        id.and_then(|id| self.run_by_id(id))
    }

    fn log_line_ui(&mut self, ui: &mut egui::Ui) {
        let tab = self.log_tab;
        let shown_run = self.shown_run_log().map(|run| (run.id, run.title.clone()));
        let title = match (tab, &shown_run) {
            (LogTab::Run(_), Some((_, title))) => title.clone(),
            _ => "Loader".to_owned(),
        };
        let last = self.log_ref(tab).last();
        let (text, tone) = last.map_or(("", Tone::Normal), |line| {
            (line.text.lines().last().unwrap_or_default(), line.tone)
        });
        let color = match tone {
            Tone::Normal => ui.visuals().text_color(),
            Tone::Warning => ui.visuals().warn_fg_color,
            Tone::Error => ui.visuals().error_fg_color,
        };
        let mut expand = false;
        let mut picked = tab;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                expand = log_collapse_button(ui, true);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let title = egui::RichText::new(title).color(crate::theme::COMMENT);
                    match &shown_run {
                        Some((id, run_title)) => {
                            egui::ComboBox::from_id_salt("log_line_tab")
                                .selected_text(title)
                                .width(0.0)
                                .truncate()
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut picked, LogTab::Loader, "Loader");
                                    ui.selectable_value(&mut picked, LogTab::Run(*id), run_title);
                                });
                        }
                        None => {
                            ui.label(title);
                        }
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().color(color))
                            .truncate(),
                    );
                });
            });
        });
        if expand {
            self.log_collapsed = false;
        }
        self.log_tab = picked;
    }

    fn log_panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.shrink_clip_rect(ui.available_rect_before_wrap());
        ui.spacing_mut().item_spacing.y = 0.0;
        let shown_run = self.shown_run_log().map(|run| (run.id, run.title.clone()));
        let mut collapse = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                collapse = log_collapse_button(ui, false);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Loader, "Loader");
                    if let Some((id, title)) = &shown_run {
                        crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Run(*id), title);
                    }
                });
            });
        });
        if collapse {
            self.log_collapsed = true;
        }
        divider(ui);
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal_wrapped(|ui| self.log_controls_ui(ui));
        self.log_ui(ui);
    }

    fn log_ui(&mut self, ui: &mut egui::Ui) {
        let tab = self.log_tab;
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let normal = ui.visuals().text_color();
        let error = ui.visuals().error_fg_color;
        let warning = ui.visuals().warn_fg_color;
        let key = (
            tab,
            self.log_ref(tab).revision,
            font.clone(),
            normal,
            warning,
            error,
        );
        if self.log_job.as_ref().is_none_or(|cached| cached.key != key) {
            let log = self.log_ref(tab);
            self.log_job = Some(LogJob {
                job: log.layout_job(font.clone(), normal, warning, error),
                entry_of_line: log.entry_of_line(),
                laid_out: RefCell::new(None),
                key,
            });
        }
        let Some(log_job) = self.log_job.as_ref() else {
            return;
        };
        let entry_of_line = &log_job.entry_of_line;
        let log = self.log_ref(tab);
        let text = log.text.as_str();
        let probe = ui.fonts_mut(|fonts| fonts.layout_no_wrap(" ".into(), font, normal));
        let pixels_per_point = ui.ctx().pixels_per_point();
        let mut layouter = |ui: &egui::Ui, _: &dyn egui::TextBuffer, wrap_width: f32| {
            let wrap_width = wrap_width.round();
            let mut laid_out = log_job.laid_out.borrow_mut();
            if let Some(kept) = laid_out.as_ref()
                && kept.wrap_width == wrap_width
                && kept.pixels_per_point == pixels_per_point
                && Arc::ptr_eq(&kept.probe, &probe)
            {
                return kept.galley.clone();
            }
            let mut job = log_job.job.clone();
            job.wrap.max_width = wrap_width;
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            *laid_out = Some(LaidOut {
                wrap_width,
                pixels_per_point,
                probe: probe.clone(),
                galley: galley.clone(),
            });
            galley
        };
        egui::ScrollArea::vertical()
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let mut read_only = text;
                let output = egui::TextEdit::multiline(&mut read_only)
                    .font(egui::TextStyle::Monospace)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .layouter(&mut layouter)
                    .show(ui);
                scroll_selection_past_edge(ui, &output.response);

                let Some(pointer) = ui
                    .input(|i| i.pointer.hover_pos())
                    .filter(|pos| output.response.rect.contains(*pos))
                else {
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
                    egui::pos2(output.response.rect.left(), span.top()),
                    egui::pos2(output.response.rect.right(), span.bottom()),
                );
                ui.painter()
                    .rect_filled(band, 0.0, ui.visuals().text_color().gamma_multiply(0.08));
                let line_rect = egui::Rect::from_min_max(
                    egui::pos2(output.response.rect.left(), rect.top()),
                    egui::pos2(output.response.rect.right(), rect.bottom()),
                );
                if line_copy_button(ui, line_rect).clicked() {
                    ui.ctx().copy_text(log.lines[log.view[entry]].text.clone());
                }
            });
    }
}

fn line_copy_button(ui: &mut egui::Ui, line: egui::Rect) -> egui::Response {
    let height = (line.height() - 2.0).max(1.0);
    let rect = egui::Rect::from_min_max(
        egui::pos2(line.right() - height * 1.6, line.top() + 1.0),
        egui::pos2(line.right(), line.top() + 1.0 + height),
    );
    let response = ui
        .interact(rect, ui.id().with("copy_line"), egui::Sense::click())
        .on_hover_text("Copy this line")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::ZERO,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ICON_COPY,
        egui::FontId::monospace(height * 0.7),
        visuals.text_color(),
    );
    response
}

fn paint_arrow(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    folded: bool,
    color: egui::Color32,
) {
    let points = if folded {
        vec![
            center + egui::vec2(-size * 0.6, -size),
            center + egui::vec2(-size * 0.6, size),
            center + egui::vec2(size * 0.9, 0.0),
        ]
    } else {
        vec![
            center + egui::vec2(-size, -size * 0.6),
            center + egui::vec2(size, -size * 0.6),
            center + egui::vec2(0.0, size * 0.9),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

fn line_char_range(text: &str, line: usize) -> (usize, usize) {
    let mut start = 0;
    for (i, content) in text.split('\n').enumerate() {
        let len = content.trim_end_matches('\r').chars().count();
        if i == line {
            return (start, start + len);
        }
        start += content.chars().count() + 1;
    }
    (start, start)
}

struct TextWalk<'a> {
    text: &'a str,
    byte: usize,
    char: usize,
    line: usize,
}

impl<'a> TextWalk<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            char: 0,
            line: 0,
        }
    }

    fn advance(&mut self, to: usize) -> (usize, usize) {
        if let Some(part) = self.text.get(self.byte..to) {
            self.char += part.chars().count();
            self.line += part.bytes().filter(|&b| b == b'\n').count();
            self.byte = to;
        }
        (self.char, self.line)
    }
}

fn paint_matches(
    ui: &egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    text: &str,
    matches: &[usize],
    find: &mut Find,
    visible: &[usize],
    on_screen: std::ops::Range<usize>,
) {
    let query_len = find.query.len();
    let origin = output.galley_pos.to_vec2();
    let mut walk = TextWalk::new(text);
    for (i, &at) in matches.iter().enumerate() {
        let (first_char, line) = walk.advance(at);
        let current = i == find.current;
        let shown = visible
            .binary_search(&line)
            .is_ok_and(|row| on_screen.contains(&row));
        if !shown && !(current && find.jump) {
            continue;
        }
        let Some(query_chars) = text.get(at..at + query_len).map(|q| q.chars().count()) else {
            continue;
        };
        let start = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(first_char));
        let end = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(first_char + query_chars));
        if (start.top() - end.top()).abs() > 1.0 {
            continue;
        }
        let rect = egui::Rect::from_min_max(start.min, egui::pos2(end.max.x, start.max.y))
            .translate(origin);
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

fn varps_report(read: Read, varps: &[i32], previous: Option<&[i32]>) -> Option<String> {
    let varp = |id: usize| varps.get(id).copied();
    Some(match read {
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
                        let before = previous
                            .get(id)
                            .map_or_else(|| "none".to_owned(), i32::to_string);
                        format!("  {} (was {before})", varp_text(None, id, *value))
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
            None => "Varps read; click again after changing something in game to see what changed."
                .into(),
        },
        _ => return None,
    })
}

fn varp_text(name: Option<&str>, id: usize, value: i32) -> String {
    let mut text = match name.or_else(|| known_varp(id)) {
        Some(name) => format!("{name} ({id}): {value}"),
        None => format!("Varp {id}: {value}"),
    };
    if let Some(meaning) = describe_varp(id, value) {
        let _ = write!(text, " ({meaning})");
    }
    text
}

#[derive(Clone, Copy)]
enum RowsAction {
    Copy,
    Save,
    Pipe,
}

fn log_collapse_button(ui: &mut egui::Ui, collapsed: bool) -> bool {
    let (icon, hint) = if collapsed {
        (ICON_EXPAND, "Show the output")
    } else {
        (ICON_COLLAPSE, "Hide the output, keeping its last line")
    };
    ui.add(egui::Button::new(icon).frame(false))
        .on_hover_text(hint)
        .clicked()
}

fn rows_buttons(
    ui: &mut egui::Ui,
    empty: bool,
    pipe: Option<&crate::pipe::Pipe>,
) -> Option<RowsAction> {
    let mut action = None;
    if ui
        .add_enabled(!empty, egui::Button::new("Copy all"))
        .on_hover_text("Copies the shown rows, a tab between columns.")
        .clicked()
    {
        action = Some(RowsAction::Copy);
    }
    if ui
        .add_enabled(!empty, egui::Button::new("Save"))
        .on_hover_text("Saves the shown rows to a text file, a tab between columns.")
        .clicked()
    {
        action = Some(RowsAction::Save);
    }
    if crate::pipe::button(ui, pipe) {
        action = Some(RowsAction::Pipe);
    }
    action
}

fn pick_file(path: &mut String, name: &str, extension: &str) {
    let mut dialog = rfd::FileDialog::new().add_filter(name, &[extension]);
    if let Some(dir) = start_dir(path.as_str()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(picked) = dialog.pick_file() {
        *path = picked.display().to_string();
    }
}

fn pick_folder(path: &mut String) {
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = Some(std::path::Path::new(path.as_str())).filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(picked) = dialog.pick_folder() {
        *path = picked.display().to_string();
    }
}

impl eframe::App for LoaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        self.handle_script_events(&ctx);
        self.drive_queues(&ctx);
        self.drive_xp(&ctx);
        if matches!(
            self.update_state,
            UpdateState::Downloading(_) | UpdateState::Installing
        ) {
            self.updating_ui(ui);
            return;
        }
        self.drop_ended_forms();
        self.reload_prompt_ui(&ctx);
        self.tiles_ui(&ctx);
        self.poll_packets(&ctx);
        self.poll_vars(&ctx);
        self.poll_script_log(&ctx);
        self.poll_inventory(&ctx);
        self.poll_icons(&ctx);
        self.poll_widget_mouse(&ctx);

        let window = ctx.content_rect();
        let narrow = window.width() < SIDEBAR_MIN_WINDOW;
        let wrap_account = window.width() < HEADER_COMPACT_WINDOW;
        let stack_account = window.width() < HEADER_STACK_WINDOW;
        let stack_brand = window.width() < HEADER_TINY_WINDOW;
        egui::Panel::top("settings").show(ui, |ui| {
            ui.add_space(6.0);
            let version = self.version_text();
            let site_url = self.api.config().site_url.clone();
            let brand = |ui: &mut egui::Ui| {
                let id = egui::Id::new("header_brand");
                let link = !site_url.is_empty();
                let hovered = link && crate::theme::link_hovered(ui, id);
                let logo = egui::Image::new(egui::include_image!("../assets/volcren.svg"))
                    .fit_to_exact_size(egui::vec2(32.0, 32.0));
                let logo = ui.add(logo);
                let title = ui.label(
                    egui::RichText::new("VOLCREN")
                        .font(egui::FontId::new(22.0, crate::theme::bold_family()))
                        .color(if hovered {
                            crate::theme::BLUE
                        } else {
                            ui.visuals().text_color()
                        }),
                );
                let sense = if link {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let brand = ui
                    .interact(logo.rect.union(title.rect), id, sense)
                    .on_hover_text(&version);
                let brand = if link {
                    brand.on_hover_cursor(egui::CursorIcon::PointingHand)
                } else {
                    brand
                };
                if brand.clicked() {
                    open_site(&site_url, "");
                }
            };
            let brand_ui = |ui: &mut egui::Ui| fit_row(ui, !stack_brand, brand);
            if !wrap_account {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 32.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        brand_ui(ui);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                            self.account_ui(ui)
                        });
                    },
                );
            } else {
                ui.vertical(|ui| {
                    if stack_brand {
                        ui.vertical_centered(brand);
                    } else {
                        centered_row(
                            ui,
                            "header_brand",
                            egui::Layout::left_to_right(egui::Align::Center),
                            32.0,
                            brand,
                        );
                    }
                    ui.add_space(4.0);
                    if stack_account {
                        ui.vertical_centered(|ui| self.account_ui(ui));
                    } else {
                        let height = ui.spacing().interact_size.y;
                        centered_row(
                            ui,
                            "header_account",
                            egui::Layout::right_to_left(egui::Align::Center),
                            height,
                            |ui| self.account_ui(ui),
                        );
                    }
                });
            }
            ui.add_space(6.0);
        });
        self.update_banner_ui(&ctx, ui);
        let log_max = (window.height() * 0.5).max(60.0);
        if self.log_collapsed {
            egui::Panel::bottom("log_collapsed")
                .resizable(false)
                .show(ui, |ui| self.log_line_ui(ui));
        } else {
            egui::Panel::bottom("log")
                .resizable(true)
                .default_size((window.height() * 0.22).clamp(70.0, 180.0))
                .min_size(48.0)
                .max_size(log_max)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    self.log_panel_ui(ui);
                });
        }
        if self.auth.me().is_none() {
            egui::CentralPanel::default_margins().show(ui, |ui| {
                self.login_gate_ui(ui);
            });
            return;
        }
        if !narrow {
            egui::Panel::left("sidebar")
                .resizable(false)
                .exact_size(SIDEBAR_WIDTH)
                .frame(crate::theme::sidebar_frame())
                .show(ui, |ui| self.sidebar_ui(ui));
        } else {
            let area = ui.available_rect_before_wrap();
            egui::Panel::left("sidebar_handle_space")
                .resizable(false)
                .exact_size(SIDEBAR_HANDLE_WIDTH)
                .frame(egui::Frame::NONE)
                .show(ui, |_| {});
            let slide = ctx.animate_bool_with_time_and_easing(
                "sidebar_slide".into(),
                self.sidebar_open,
                0.2,
                egui::emath::easing::cubic_out,
            );
            let width = SIDEBAR_WIDTH.min(area.width() * 0.85);
            if slide > 0.0 {
                let backdrop = egui::Area::new("sidebar_backdrop".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(area.min)
                    .show(&ctx, |ui| {
                        let (_, response) =
                            ui.allocate_exact_size(area.size(), egui::Sense::click());
                        let shade = egui::Color32::from_black_alpha((130.0 * slide) as u8);
                        ui.painter()
                            .rect_filled(area, egui::CornerRadius::ZERO, shade);
                        response
                    })
                    .inner;
                if backdrop.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.sidebar_open = false;
                }
            }
            egui::Area::new("sidebar_drawer".into())
                .order(egui::Order::Tooltip)
                .constrain(false)
                .fixed_pos(egui::pos2(area.min.x - width * (1.0 - slide), area.min.y))
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    ui.horizontal_top(|ui| {
                        crate::theme::sidebar_frame().show(ui, |ui| {
                            ui.set_width(width - f32::from(crate::theme::SIDEBAR_INSET));
                            ui.set_height(area.height() - 12.0);
                            ui.vertical(|ui| self.sidebar_ui(ui));
                        });
                        ui.allocate_ui(egui::vec2(SIDEBAR_HANDLE_WIDTH, area.height()), |ui| {
                            if crate::theme::sidebar_handle(ui, slide).clicked() {
                                self.sidebar_open = !self.sidebar_open;
                            }
                        });
                    });
                });
        }
        let frame = egui::Frame::central_panel(ui.style())
            .inner_margin((window.width() * 0.02).clamp(8.0, 16.0));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            if self.tab == Tab::Clients {
                crate::logo::paint_background(ui, ui.max_rect());
            }
            if let Some(label) = self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(label);
                });
                ui.separator();
            }
            if self.tab == Tab::Run && self.run().is_none() {
                self.tab = Tab::Sessions;
            }
            self.repair_log_tab();
            let followed = (self.tab, self.active_run);
            if followed != self.followed_tab {
                self.followed_tab = followed;
                self.log_collapsed =
                    matches!(self.tab, Tab::Accounts | Tab::Settings | Tab::Feedback);
                self.log_tab = match self.tab {
                    Tab::Run | Tab::Sessions | Tab::Scripts | Tab::Store => {
                        self.active_run.map_or(LogTab::Loader, LogTab::Run)
                    }
                    _ => LogTab::Loader,
                };
            }
            match self.tab {
                Tab::Clients => self.clients_ui(ui),
                Tab::Store => self.store_ui(ui),
                Tab::Sessions => self.sessions_ui(ui),
                Tab::Queue => self.queue_ui(ui),
                Tab::Run => self.running_ui(ui),
                Tab::Scripts => self.scripts_ui(ui),
                Tab::Development => self.development_ui(ui),
                Tab::Terminal => self.terminal_ui(ui),
                Tab::Accounts => self.accounts_ui(ui),
                Tab::Feedback => self.feedback_ui(ui),
                Tab::Settings => self.settings_ui(ui),
            }
            let ctx = ui.ctx().clone();
            self.start_prompt_ui(&ctx);
            self.warning_ui(&ctx);
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        eframe::set_value(storage, SCRIPT_KEY, &self.script_file);
        eframe::set_value(storage, LAST_SCRIPT_KEY, &self.last_script);
        eframe::set_value(storage, FORMS_KEY, &self.remembered_forms);
        self.sync_queue_form();
        eframe::set_value(storage, crate::queue::STORAGE_KEY, &self.queues);
        if !self.layouts_unreadable {
            eframe::set_value(storage, LAYOUTS_KEY, &self.layouts);
        }
        eframe::set_value(storage, CURSOR_KEY, &self.cursor);
        eframe::set_value(storage, UI_FONT_KEY, &self.ui_font);
        eframe::set_value(storage, CONFIRMED_KEY, &self.confirmed);
        eframe::set_value(
            storage,
            HTTP_WHITELIST_KEY,
            &crate::script::read(&self.http_whitelist).user(),
        );
    }

    fn on_exit(&mut self) {
        if let Some(tiles) = self.tiles.take() {
            tiles.finish();
        }
        if let Some(terminal) = &mut self.terminal {
            terminal.close();
        }
        self.stop_all();
        backend::shutdown();
    }
}

fn inventory_items_ui(
    ui: &mut egui::Ui,
    inspector: &crate::inventory::Inspector,
    icons: &mut crate::icons::Icons,
    snapshot: &crate::inventory::Snapshot,
    before: Option<&crate::inventory::Snapshot>,
) {
    let mut icon = |ui: &mut egui::Ui, id: i32, quantity: i32| {
        let key = crate::icons::Key::item(id, quantity, inspector.is_stackable(id));
        let texture = icons.texture(ui.ctx(), key);
        crate::icons::slot(ui, texture, inspector.item_name(id))
    };
    let mut line = |ui: &mut egui::Ui, id: i32, quantity: i32, color: egui::Color32| {
        ui.horizontal(|ui| {
            icon(ui, id, quantity);
            ui.colored_label(
                color,
                format!("{} x{quantity}  (id {id})", inspector.item_name(id)),
            );
        });
    };
    if let Some(before) = before {
        let (added, removed) = crate::inventory::compare(&before.items, &snapshot.items, |id| {
            inspector.is_stackable(id)
        });
        if !added.is_empty() {
            ui.strong("Items Added:");
            for (id, quantity) in added {
                line(ui, id, quantity, egui::Color32::from_rgb(90, 200, 90));
            }
        }
        if !removed.is_empty() {
            ui.strong("Items Removed:");
            for (id, quantity) in removed {
                line(ui, id, quantity, egui::Color32::from_rgb(230, 100, 100));
            }
        }
    }
    ui.strong("Items in Container:");
    let columns = if snapshot.items.len() == 28 {
        4
    } else {
        ((ui.available_width() / (crate::icons::SLOT_SIZE.x + 8.0)) as usize).max(1)
    };
    egui::Grid::new("inventory_slots")
        .spacing(egui::vec2(8.0, 6.0))
        .show(ui, |ui| {
            for (i, item) in snapshot.items.iter().enumerate() {
                if item.id < 0 {
                    crate::icons::slot(ui, None, "")
                        .on_hover_text(format!("Slot {}: empty", item.slot));
                } else {
                    icon(ui, item.id, item.quantity).on_hover_text(format!(
                        "Slot {}: {} x{}  (id {})",
                        item.slot,
                        inspector.item_name(item.id),
                        item.quantity,
                        item.id
                    ));
                }
                if (i + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
}

fn widget_node_ui(
    ui: &mut egui::Ui,
    inspector: &crate::widgets::Inspector,
    index: usize,
    open: &HashSet<usize>,
    reveal: bool,
    picked: &mut Option<(i32, i32)>,
) {
    let node = &inspector.built().nodes[index];
    if inspector.hide_hidden && node.widget.hidden {
        return;
    }
    let key = (node.widget.id, node.widget.index);
    let label = format!(
        "{} {}",
        node.kind.letter(),
        inspector.identifier(&node.widget)
    );
    let selected = *picked == Some(key);
    if node.children.is_empty() {
        let response = ui.selectable_label(selected, label);
        if response.clicked() {
            *picked = Some(key);
        }
        if selected && reveal {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        return;
    }
    let mut header = egui::CollapsingHeader::new(egui::RichText::new(label).color(if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    }))
    .id_salt(("widget", node.widget.id, node.widget.index, index));
    if reveal && open.contains(&index) {
        header = header.open(Some(true));
    }
    let shown = header.show(ui, |ui| {
        for &child in &node.children {
            widget_node_ui(ui, inspector, child, open, reveal, picked);
        }
    });
    if shown.header_response.clicked() {
        *picked = Some(key);
    }
    if selected && reveal {
        shown
            .header_response
            .scroll_to_me(Some(egui::Align::Center));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ignores_ascii_case_like_lowercasing_does() {
        let old = |text: &str, query: &str| -> Vec<usize> {
            text.to_ascii_lowercase()
                .match_indices(&query.to_ascii_lowercase())
                .map(|(at, _)| at)
                .collect()
        };
        for (text, query) in [
            ("Foo foo FOO", "foo"),
            ("aaaa", "aa"),
            ("ÄpfelÄ äpfel", "ä"),
            ("ÄpfelÄ äpfel", "PFEL"),
            ("über Über", "Über"),
            ("", "x"),
            ("short", "longer query"),
        ] {
            assert_eq!(
                find_matches(text, query, false),
                old(text, query),
                "{text:?} / {query:?}"
            );
        }
        assert_eq!(find_matches("Foo foo", "foo", true), [4]);
        assert!(find_matches("anything", "", false).is_empty());
    }

    fn sidebar_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        tab: Tab,
        active_run: Option<u64>,
    ) -> Option<(Tab, Option<u64>)> {
        let runs = [
            (1, "woodcutter".to_string(), false),
            (2, "old".to_string(), true),
        ];
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut picked = None;
        ctx.run_ui(input, |ui| {
            picked = sidebar_tree(ui, tab, active_run, &runs, "");
        })
        .textures_delta
        .clear();
        picked
    }

    #[test]
    fn the_sidebar_reports_no_pick_without_a_click_while_a_run_is_active() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx, crate::theme::UiFont::default());
        for tab in [
            Tab::Clients,
            Tab::Store,
            Tab::Sessions,
            Tab::Settings,
            Tab::Run,
        ] {
            for _ in 0..3 {
                assert!(sidebar_frame(&ctx, Vec::new(), tab, Some(1)).is_none());
            }
        }
    }

    #[test]
    fn a_click_on_a_sidebar_row_picks_its_page() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx, crate::theme::UiFont::default());
        let mut picks = Vec::new();
        for y in (0..400).step_by(5) {
            let pos = egui::pos2(40.0, y as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let frames = [
                vec![egui::Event::PointerMoved(pos)],
                vec![button(true)],
                vec![button(false)],
            ];
            for events in frames {
                if let Some(pick) = sidebar_frame(&ctx, events, Tab::Sessions, Some(1)) {
                    picks.push(pick);
                }
            }
        }
        for page in [
            (Tab::Clients, None),
            (Tab::Settings, None),
            (Tab::Run, Some(1)),
            (Tab::Run, Some(2)),
        ] {
            assert!(picks.contains(&page));
        }
        assert!(!picks.contains(&(Tab::Sessions, None)));
    }

    #[test]
    fn the_log_keeps_the_newest_lines_and_its_filter() {
        let keep = *backend::LOG_LINES.start() * 2;
        let mut log = Log::new(keep);
        for i in 0..keep + keep / 4 + 1 {
            log.push(
                false,
                if i % 2 == 0 {
                    format!("[even] {i}")
                } else {
                    format!("{i}")
                },
            );
        }
        assert_eq!(log.lines.len(), keep);
        assert_eq!(
            log.lines.last().unwrap().text,
            format!("[even] {}", keep + keep / 4)
        );
        assert_eq!(log.text.lines().count(), keep);
        log.set_filter(Some(0));
        assert_eq!(log.view.len(), keep / 2);
        assert!(log.text.lines().all(|line| line.starts_with("[even]")));
        log.set_keep(keep / 2);
        assert_eq!(log.lines.len(), keep / 2);
        assert_eq!(log.view.len(), keep / 4);
        let revision = log.revision;
        log.clear();
        assert!(log.revision > revision);
        assert_eq!(log.keep, keep / 2);
    }

    #[test]
    fn a_dragged_selection_scrolls_as_fast_down_as_up_for_the_same_share_of_room() {
        let visible = egui::Rangef::new(300.0, 590.0);
        let window = egui::Rangef::new(0.0, 600.0);
        let speed = |y| selection_scroll_speed(y, visible, window);
        assert_eq!(speed(400.0), 0.0);
        assert_eq!(
            speed(300.0 - SELECTION_SCROLL_RAMP / 2.0),
            -SELECTION_SCROLL_MAX / 2.0
        );
        assert_eq!(speed(595.0), SELECTION_SCROLL_MAX / 2.0);
        assert_eq!(speed(0.0), -SELECTION_SCROLL_MAX);
        assert_eq!(speed(600.0), SELECTION_SCROLL_MAX);
        assert_eq!(speed(700.0), SELECTION_SCROLL_MAX);
    }

    #[test]
    fn a_selection_dragged_above_the_output_keeps_scrolling_up() {
        let ctx = egui::Context::default();
        let text: String = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut time = 0.0;
        let mut frame = |events: Vec<egui::Event>| {
            time += 0.016;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                time: Some(time),
                ..Default::default()
            };
            let mut offset = 0.0;
            let mut output = ctx.run_ui(input, |ui| {
                ui.add_space(300.0);
                offset = egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let mut read_only = text.as_str();
                        let output = egui::TextEdit::multiline(&mut read_only).show(ui);
                        scroll_selection_past_edge(ui, &output.response);
                    })
                    .state
                    .offset
                    .y;
            });
            output.textures_delta.clear();
            offset
        };
        for _ in 0..3 {
            frame(Vec::new());
        }
        let inside = egui::pos2(50.0, 400.0);
        frame(vec![egui::Event::PointerMoved(inside)]);
        let bottom = frame(vec![egui::Event::PointerButton {
            pos: inside,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        for y in [380.0, 340.0, 300.0, 250.0] {
            frame(vec![egui::Event::PointerMoved(egui::pos2(50.0, y))]);
        }
        let moved = frame(Vec::new());
        let held = (0..10).map(|_| frame(Vec::new())).last().unwrap();
        assert!(moved < bottom && held < moved, "{bottom} {moved} {held}");
    }

    #[test]
    fn log_sections_cover_runs_of_one_color() {
        let mut log = Log::new(*backend::LOG_LINES.start());
        for (tone, text) in [
            (Tone::Normal, "a"),
            (Tone::Normal, "bb"),
            (Tone::Error, "c\nd"),
            (Tone::Warning, "w"),
            (Tone::Normal, "e"),
        ] {
            log.push_tone(tone, text.into());
        }
        let (normal, warning, error) = (
            egui::Color32::WHITE,
            egui::Color32::YELLOW,
            egui::Color32::RED,
        );
        let job = log.layout_job(egui::FontId::monospace(13.0), normal, warning, error);
        let sections: Vec<_> = job
            .sections
            .iter()
            .map(|s| {
                (
                    &job.text[s.byte_range.start.0..s.byte_range.end.0],
                    s.format.color,
                )
            })
            .collect();
        assert_eq!(
            sections,
            [
                ("a\nbb\n", normal),
                ("c\nd\n", error),
                ("w\n", warning),
                ("e", normal)
            ]
        );
    }

    #[test]
    fn the_text_walk_counts_chars_and_lines() {
        let text = "é\nab\nc";
        let mut walk = TextWalk::new(text);
        assert_eq!(walk.advance(3), (2, 1));
        assert_eq!(walk.advance(7), (6, 2));
        assert_eq!(walk.advance(1), (6, 2));
        assert_eq!(walk.advance(99), (6, 2));
    }
}
