use std::{
    sync::{
        Arc, Mutex,
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
use crate::api::{Api, ApiError, Config, Me, Redeemed, Removed, StoreScript, models::{API_VERSION, Ended}, Heartbeat, Stats};
use crate::highlight::Highlighter;
use crate::script::{Script, ScriptEvent};
use crate::session::{self, Beat, Prepared, Verdict};
use crate::store::{self, Confirmed, Store};
use crate::update::{self, Release};

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
    /// A read for the XP tracker, taken while a script runs.
    XpState(Result<GameState, String>),
    /// The account name of the selected client, for the Accounts page.
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
    /// A read of the packet log for the packet monitor.
    Packets {
        pid: u32,
        result: Result<injector::PacketPage, String>,
    },
    /// What the agent read about the client's packets, for the packet monitor.
    PacketLayouts {
        pid: u32,
        result: Result<Option<Vec<injector::PacketLayout>>, String>,
    },
    /// The script names the Script Inspector's Load names button downloaded.
    ScriptNames(Result<crate::script_names::Names, String>),
    /// A read of the script log for the script inspector.
    ScriptRuns {
        pid: u32,
        result: Result<crate::script_log::Poll, String>,
    },
    /// A read of the varps for the var inspector.
    Vars {
        pid: u32,
        result: Result<crate::vars::Poll, String>,
    },
    /// A read of the item containers for the inventory inspector.
    Inventory {
        pid: u32,
        result: Result<crate::inventory::Poll, String>,
    },
    /// A read of the open interfaces for the widget inspector.
    Widgets {
        pid: u32,
        result: Result<crate::widgets::Poll, String>,
    },
    /// Where the client's mouse is, for the widget inspector's "Follow mouse".
    WidgetMouse {
        pid: u32,
        result: Result<Option<(i32, i32)>, String>,
    },
    Restored(Result<Option<Me>, ApiError>),
    SignedIn(Result<Me, String>),
    Me(Result<Me, ApiError>),
    Scripts(Result<Vec<StoreScript>, ApiError>),
    Redeemed(Result<Redeemed, ApiError>),
    GrantRemoved {
        id: String,
        result: Result<Removed, ApiError>,
    },
    StoreStarted {
        pid: u32,
        title: String,
        /// The script is free, so its source is shown in the editor; premium scripts stay closed.
        open: bool,
        result: Result<Prepared, String>,
    },
    Heartbeat {
        session_id: String,
        sent: Option<Stats>,
        result: Result<Heartbeat, ApiError>,
    },
    SessionEnded(Result<Ended, ApiError>),
    Update(Result<Option<Release>, String>),
}

/// A store script that is running, and the session that is billing it.
struct StoreRun {
    session_id: String,
    /// The script's name, for its page in the sidebar.
    title: String,
    beat: Beat,
    /// The script is being stopped for a reason already reported; the end of the session follows its exit.
    stopping: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Clients,
    Store,
    /// The page of the store script that is running; it replaces `Store` in the sidebar until the script ends.
    Running,
    Scripts,
    Development,
    Terminal,
    Accounts,
    Settings,
}

/// The tabs inside the Development page.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DevTab {
    Tools,
    Inspectors,
}

impl DevTab {
    const ALL: [(DevTab, &'static str); 2] = [(DevTab::Tools, "Tools"), (DevTab::Inspectors, "Inspectors")];
}

/// The tabs inside the Inspectors tab.
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

/// A script's title as a sidebar row: lowercase like the other pages, cut to fit.
fn sidebar_name(title: &str) -> String {
    const MAX: usize = 16;
    let name = title.to_lowercase();
    if name.chars().count() <= MAX {
        return name;
    }
    name.chars().take(MAX - 1).chain(['\u{2026}']).collect()
}

/// Window width from which the sidebar is always shown.
const SIDEBAR_MIN_WINDOW: f32 = 760.0;
const SIDEBAR_WIDTH: f32 = 190.0;
/// Width of the strip that toggles the sidebar on narrow windows.
const SIDEBAR_HANDLE_WIDTH: f32 = 18.0;

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
/// Below this width the script tab stacks the output and the tracker rather than putting them side by side.
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 560.0;

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
// How long the scene tiles may keep failing before the worker gives up and reports why.
const TILES_GIVE_UP: Duration = Duration::from_secs(10);
// The pause after a failed paint: the frame it waited for is spent, so the next try wouldn't wait for one.
const TILES_RETRY: Duration = Duration::from_millis(50);

// Paints scene tiles on its own thread once per presented frame; dropping it stops and clears.
struct TilesWorker {
    pid: u32,
    layers: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TilesWorker {
    fn start(ctx: &egui::Context, tx: Sender<Event>, target: Target, settings: Settings, layers: Vec<String>) -> Self {
        let pid = target.client.pid;
        let worker = Self { pid, layers: Arc::new(Mutex::new(layers)), stop: Arc::new(AtomicBool::new(false)), thread: None };
        let (layers, stop, ctx) = (worker.layers.clone(), worker.stop.clone(), ctx.clone());
        let mut worker = worker;
        worker.thread = Some(thread::spawn(move || {
            // the frame last painted after; the first paint waits for any frame
            let mut frame = 0;
            // since when the paints have been failing
            let mut failing: Option<Instant> = None;
            while !stop.load(Ordering::Relaxed) {
                let current = layers.lock().map(|l| l.clone()).unwrap_or_default();
                match backend::scene_tiles_next_frame(&target, &settings, &current.iter().map(String::as_str).collect::<Vec<_>>(), frame) {
                    Ok(Some(painted)) => {
                        frame = painted;
                        failing = None;
                    }
                    // no frame came (minimized, loading): look again once it's clear the worker isn't stopping
                    Ok(None) => {}
                    // A scene change, hop or logout fails the paints: clear the boxes at once and resume when possible.
                    Err(error) => {
                        let since = *failing.get_or_insert_with(|| {
                            let _ = backend::draw(&target, &settings, &Overlay::Clear);
                            Instant::now()
                        });
                        if since.elapsed() > TILES_GIVE_UP {
                            let _ = tx.send(Event::Tiles { pid, result: Err(error) });
                            ctx.request_repaint();
                            break;
                        }
                        thread::sleep(TILES_RETRY);
                    }
                }
            }
            // Take the boxes off the client once nothing can paint them again.
            let _ = backend::draw(&target, &settings, &Overlay::Clear);
        }));
        worker
    }

    /// Stops painting and waits until the boxes are off the client; the loader is about to exit, which would cut the clear short.
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
const FORMS_KEY: &str = "forms";
const CURSOR_KEY: &str = "cursor";
const CONFIRMED_KEY: &str = "community_confirmed";
/// The hosts the user added to the HTTP whitelist (the built-in ones are not stored).
const HTTP_WHITELIST_KEY: &str = "http_whitelist";

/// How often the XP tracker reads the client while a script runs.
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

/// A Development inspector's log as a table: a header that stays put, columns the user can resize, rows drawn only while visible.
/// `columns` are (title, column); the last one should be `remainder`. `row` fills one row's cells, given the row.
struct LogTable<'a> {
    id: &'a str,
    follow: bool,
    row_height: f32,
    max_height: Option<f32>,
    clickable: bool,
}

impl LogTable<'_> {
    fn show(&self, ui: &mut egui::Ui, columns: Vec<(&str, egui_extras::Column)>, rows: usize, row: impl FnMut(egui_extras::TableRow<'_, '_>)) {
        // text that can be selected takes the clicks a clickable row wants
        ui.scope(|ui| {
            ui.style_mut().interaction.selectable_labels = !self.clickable;
            self.table(ui, columns, rows, row);
        });
    }

    fn table(&self, ui: &mut egui::Ui, columns: Vec<(&str, egui_extras::Column)>, rows: usize, row: impl FnMut(egui_extras::TableRow<'_, '_>)) {
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
        let titles: Vec<&str> = columns.iter().map(|(title, _)| *title).collect();
        for (_, column) in columns {
            table = table.column(column);
        }
        table
            .header(self.row_height, |mut header| {
                for title in titles {
                    header.col(|ui| {
                        ui.strong(title);
                    });
                }
            })
            .body(|body| body.rows(self.row_height, rows, row));
    }
}

fn narrow(width: f32) -> egui_extras::Column {
    egui_extras::Column::initial(width).at_least(24.0).clip(true)
}

/// The Names menu of the packet monitor: every packet the table knows by name, searchable, each click cycling it through
/// show only, hide and off by rewriting the filter text.
fn packet_names_menu(ui: &mut egui::Ui, monitor: &mut crate::packets::Monitor) {
    use crate::packets::{name_state, with_name, NameState, UNKNOWN};

    let active = crate::packets::named_words(&monitor.filter, monitor.table()) + usize::from(crate::packets::name_state(&monitor.filter, UNKNOWN) != NameState::Off);
    let title = if active > 0 { format!("Names ({active}) ⏷") } else { "Names ⏷".to_owned() };
    let config = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new(title).config(config).ui(ui, |ui| {
        ui.set_width(300.0);
        ui.add(egui::TextEdit::singleline(&mut monitor.name_search).desired_width(f32::INFINITY).hint_text("🔍 Search names"));
        ui.add_space(2.0);
        ui.weak("Click a name: show only → hide → off.");
        ui.separator();

        let search = monitor.name_search.trim().to_ascii_lowercase();
        let mut rows: Vec<(bool, u16, String)> = monitor.table().named().map(|(sent, opcode, name)| (sent, opcode, name.to_owned())).filter(|(_, _, name)| name.to_ascii_lowercase().contains(&search)).collect();
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
                    ui.add_sized([20.0, 18.0], egui::Label::new(egui::RichText::new(glyph).color(color).strong()));
                    let text = egui::RichText::new(name).monospace();
                    let text = if state == NameState::Off { text } else { text.color(color) };
                    ui.label(text);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(detail));
                })
                .response
                .interact(egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(response.rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
            }
            if response.clicked() {
                clicked = Some((name.to_owned(), state.next()));
            }
        };
        if UNKNOWN.to_ascii_lowercase().contains(&search) {
            row(ui, UNKNOWN, "no name".to_owned());
            ui.separator();
        }
        egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            if rows.is_empty() && !UNKNOWN.to_ascii_lowercase().contains(&search) {
                ui.weak("No name matches.");
            }
            for (sent, opcode, name) in &rows {
                row(ui, name, format!("{} {opcode}", if *sent { "→" } else { "←" }));
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(!monitor.filter.is_empty(), egui::Button::new("Clear filter")).clicked() {
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

fn known_varp_name(id: usize) -> Option<String> {
    KNOWN_VARPS.iter().find(|&&(_, known)| known == id).map(|&(name, _)| name.to_owned())
}

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

/// The lines a log tab keeps; older ones are dropped, since the whole output is laid out again whenever a line is added.
const MAX_LOG_LINES: usize = 2000;

/// One log tab's output; `view` and `text` hold only lines the tag filter lets through and are rebuilt only when it changes.
#[derive(Default)]
struct Log {
    lines: Vec<LogLine>,
    tags: Vec<String>,
    filter: Option<usize>,
    view: Vec<usize>,
    text: String,
    /// Bumped whenever `view` changes, so the laid out output is only rebuilt then.
    revision: u64,
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
        // dropped a quarter at a time, so the view is rebuilt once every few hundred lines rather than on each
        if self.lines.len() > MAX_LOG_LINES + MAX_LOG_LINES / 4 {
            self.lines.drain(..self.lines.len() - MAX_LOG_LINES);
            self.rebuild();
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

    /// The shown lines in their colors: one section per run of lines of the same color.
    fn layout_job(&self, font: egui::FontId, normal: egui::Color32, error: egui::Color32) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob { text: self.text.clone(), ..Default::default() };
        let mut section = |range: std::ops::Range<usize>, is_error: bool| {
            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(range.start)..egui::text::ByteIndex(range.end),
                format: egui::TextFormat::simple(font.clone(), if is_error { error } else { normal }),
            });
        };
        let mut start = 0;
        let mut run: Option<(usize, bool)> = None;
        for (i, &index) in self.view.iter().enumerate() {
            let line = &self.lines[index];
            if let Some((from, was_error)) = run.filter(|&(_, was_error)| was_error != line.error) {
                section(from..start, was_error);
                run = None;
            }
            run.get_or_insert((start, line.error));
            // each line but the last is followed by the newline that joins it to the next
            start += line.text.len() + usize::from(i + 1 < self.view.len());
        }
        if let Some((from, was_error)) = run {
            section(from..self.text.len(), was_error);
        }
        job
    }

    fn clear(&mut self) {
        // the revision keeps counting, so a job laid out before the clear can't pass for the new output
        *self = Self { revision: self.revision + 1, ..Self::default() };
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

/// The tabs of the panel beside the script output.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolsTab {
    Xp,
    Breaks,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Loader,
    Script,
}

type LogJobKey = (LogTab, u64, egui::FontId, egui::Color32, egui::Color32);

pub struct LoaderApp {
    settings: Settings,
    tab: Tab,
    /// The sidebar drawer is open (only used when the window is too narrow to show it permanently).
    sidebar_open: bool,
    /// The shell of the Terminal page, started the first time it is opened and kept until the app closes.
    terminal: Option<crate::terminal::Terminal>,
    display_name: String,
    varp: usize,
    varps: Option<Vec<i32>>,
    script_file: ScriptFile,
    /// A store script's source is open in the editor, read-only: its name and revision. The user's own file waits in `parked`.
    viewing: Option<String>,
    parked: Option<ScriptFile>,
    find: Find,
    highlighter: Highlighter,
    fold: crate::fold::Folds,
    script: Option<Script>,
    form: Option<crate::form::Form>,
    /// The file's new contents, while asking whether to reload it before running.
    reload_prompt: Option<String>,
    remembered_forms: std::collections::HashMap<String, serde_json::Map<String, serde_json::Value>>,
    dev_toggles: [bool; DEV_TOGGLES.len()],
    cursor: Cursor,
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
    accounts: crate::accounts::Accounts,
    /// The hosts scripts may send HTTP requests to.
    http_whitelist: crate::whitelist::Whitelist,
    whitelist_page: crate::whitelist::Page,
    /// Community scripts the user agreed to run, by revision (persisted).
    confirmed: Confirmed,
    /// A community script waiting for the user to accept the warning before it starts.
    warning: Option<(StoreScript, Target)>,
    store_run: Option<StoreRun>,
    /// Whether the latest script run came from the editor, so its page hands back to the editor when it ends.
    run_was_local: bool,
    /// A newer launcher release, until the user dismisses it.
    update: Option<Release>,
    /// Title of the store script being prepared (session start and download).
    starting: Option<String>,
    targets: Vec<Target>,
    dll_version: Option<String>,
    selected: Option<String>,
    busy: Option<&'static str>,
    log_tab: LogTab,
    /// The main tab the output tab last followed, so switching main tabs switches the output but the output can still be changed on its own.
    followed_tab: Tab,
    script_log: Log,
    loader_log: Log,
    /// The output tab's layout job, for the tab and revision it was built from.
    log_job: Option<(LogJobKey, egui::text::LayoutJob)>,
    xp: crate::xp::XpTracker,
    breaks: crate::breaks::BreaksTracker,
    counters: crate::counters::CountersTracker,
    /// When the running script started, and when it ended; the XP Tracker's clock.
    run_started: Option<std::time::Instant>,
    run_ended: Option<std::time::Instant>,
    tools_tab: ToolsTab,
    /// The client the XP tracker reads while the script runs, when one was chosen.
    xp_target: Option<Target>,
    xp_next: std::time::Instant,
    xp_reading: bool,
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
        let config = Config::from_env();
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
            viewing: None,
            parked: None,
            find: Find::default(),
            highlighter: Highlighter::default(),
            fold: crate::fold::Folds::default(),
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
            accounts: crate::accounts::Accounts::load(),
            http_whitelist: crate::whitelist::Whitelist::new(
                cc.storage.and_then(|s| eframe::get_value(s, HTTP_WHITELIST_KEY)).unwrap_or_default(),
                &config,
            ),
            whitelist_page: Default::default(),
            confirmed: cc
                .storage
                .and_then(|s| eframe::get_value(s, CONFIRMED_KEY))
                .unwrap_or_default(),
            warning: None,
            store_run: None,
            run_was_local: false,
            update: None,
            starting: None,
            targets: Vec::new(),
            dll_version: None,
            selected: None,
            busy: None,
            log_tab: LogTab::Loader,
            followed_tab: Tab::Clients,
            script_log: Log::default(),
            xp: Default::default(),
            breaks: Default::default(),
            counters: Default::default(),
            run_started: None,
            run_ended: None,
            tools_tab: ToolsTab::Xp,
            xp_target: None,
            xp_next: std::time::Instant::now(),
            xp_reading: false,
            loader_log: Log::default(),
            log_job: None,
            tx,
            rx,
        };
        app.scan(&cc.egui_ctx);
        app.restore_sign_in(&cc.egui_ctx);
        app.api_job(&cc.egui_ctx, |api| Event::Update(update::check(&api)));
        app
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
        self.info("Waiting for you to sign in in the browser…");
        self.api_job(ctx, move |api| Event::SignedIn(account::sign_in(&api, &cancel)));
    }

    fn sign_out(&mut self, why: Option<String>) {
        self.api.sign_out();
        self.auth = Auth::SignedOut;
        self.store = Store::default();
        self.warning = None;
        match why {
            Some(why) => self.error(format!("Signed out: {why}")),
            None => self.info("Signed out."),
        }
    }

    fn signed_in(&mut self, ctx: &egui::Context, me: Me) {
        self.info(format!("Signed in as {}.", me.username));
        if me.api_version > API_VERSION {
            self.error("This launcher is too old for the server. Update it to run store scripts.");
        }
        self.auth = Auth::SignedIn(me);
        self.fetch_scripts(ctx);
    }

    fn fetch_scripts(&mut self, ctx: &egui::Context) {
        self.store.loading = true;
        self.store.error = None;
        let community = self.store.community;
        self.api_job(ctx, move |api| Event::Scripts(api.scripts("", "", community)));
    }

    /// Why a store script can't start right now, apart from the script itself.
    fn run_block(&self, target: Option<&Target>) -> Option<String> {
        if self.auth.me().is_some_and(|me| me.api_version > API_VERSION) {
            return Some("Update the launcher to run store scripts.".into());
        }
        if self.starting.is_some() {
            return Some("A script is starting.".into());
        }
        if self.script.as_ref().is_some_and(Script::is_running) {
            return Some("A script is already running. Stop it first.".into());
        }
        target.is_none().then(|| "Select a client with agent loaded in the Clients tab.".into())
    }

    fn start_store_script(&mut self, ctx: &egui::Context, script: StoreScript, target: Target) {
        self.starting = Some(script.title.clone());
        self.info(format!("Starting {}…", script.title));
        let (pid, title) = (target.client.pid, script.title.clone());
        // A community script only runs at the revision the user was warned about
        let warned = script.community.then_some(script.revision).flatten();
        let open = script.price <= 0;
        self.api_job(ctx, move |api| Event::StoreStarted { pid, title, open, result: session::prepare(&api, &script, warned) });
    }

    /// Ends the session of the store script that just stopped. Idempotent on the server, so it is safe to call twice.
    fn end_store_session(&mut self, ctx: &egui::Context) {
        if let Some(run) = self.store_run.take() {
            self.api_job(ctx, move |api| Event::SessionEnded(api.end_session(&run.session_id)));
        }
    }

    fn start_xp_tracking(&mut self, target: Option<Target>) {
        self.xp.reset();
        self.breaks.reset();
        self.counters.reset();
        self.run_started = Some(std::time::Instant::now());
        self.run_ended = None;
        self.xp_target = target;
        self.xp_next = std::time::Instant::now();
    }

    /// While a script runs, reads the client's skills every few seconds for the XP tracker.
    fn drive_xp(&mut self, ctx: &egui::Context) {
        if !self.script.as_ref().is_some_and(Script::is_running) {
            return;
        }
        // the run time on screen ticks every second
        ctx.request_repaint_after(Duration::from_secs(1));
        let Some(target) = self.xp_target.clone().filter(|t| t.client.loaded.is_some()) else { return };
        let now = std::time::Instant::now();
        if self.xp_reading || now < self.xp_next {
            ctx.request_repaint_after(self.xp_next.saturating_duration_since(now).max(Duration::from_millis(250)));
            return;
        }
        self.xp_reading = true;
        self.background(ctx, move |settings| Event::XpState(backend::state(&target, &settings)));
    }

    /// Sends a heartbeat when one is due, and asks for a repaint when the next one will be.
    fn drive_session(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        let Some(run) = self.store_run.as_mut().filter(|run| !run.stopping) else { return };
        if run.beat.due(now) {
            let sent = run.beat.take(now);
            let session_id = run.session_id.clone();
            self.api_job(ctx, move |api| {
                let result = api.heartbeat(&session_id, sent.as_ref());
                Event::Heartbeat { session_id, sent, result }
            });
        } else {
            ctx.request_repaint_after(run.beat.next_due().saturating_duration_since(now).max(Duration::from_millis(250)));
        }
    }

    fn store_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(me) = self.auth.me().cloned() else {
            match self.auth {
                Auth::Restoring => {
                    ui.label("Checking for a saved sign-in…");
                }
                Auth::SigningIn(_) => {
                    ui.label("Waiting for you to sign in in the browser…");
                }
                _ => {
                    ui.label("Sign in with Discord to browse and run scripts.");
                    if ui.button("Sign in").clicked() {
                        self.start_sign_in(&ctx);
                    }
                }
            }
            return;
        };
        if !self.store.loaded && !self.store.loading && self.store.error.is_none() {
            self.fetch_scripts(&ctx);
        }
        let target = self.loaded_target_ui(ui);
        let block = self.run_block(target.as_ref());
        match self.store.ui(ui, &me, block.as_deref()) {
            Some(store::Action::Refresh) => self.fetch_scripts(&ctx),
            Some(store::Action::Run(script)) => {
                if let Some(target) = target {
                    // Community scripts are not reviewed by staff: warn before the first run of each revision
                    let confirmed = script.revision.is_some_and(|revision| self.confirmed.has(&script.id, revision));
                    if script.community && !confirmed {
                        self.warning = Some((*script, target));
                    } else {
                        self.start_store_script(&ctx, *script, target);
                    }
                }
            }
            Some(store::Action::Redeem(code)) => {
                self.store.redeeming = true;
                self.store.redeem_result = None;
                self.api_job(&ctx, move |api| Event::Redeemed(api.redeem(&code)));
            }
            Some(store::Action::RemoveGrant(script)) => {
                let id = script.id.clone();
                self.api_job(&ctx, move |api| Event::GrantRemoved { result: api.remove_grant(&id), id });
            }
            None => {}
        }
        self.warning_ui(&ctx);
    }

    /// The warning shown before a community script runs. Accepting it is remembered for that revision of that script.
    fn warning_ui(&mut self, ctx: &egui::Context) {
        let Some((script, target)) = self.warning.take() else { return };
        let mut decision = None;
        egui::Window::new("Community script")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label(egui::RichText::new(&script.title).strong());
                ui.label(format!("by {}", script.author));
                ui.add_space(6.0);
                ui.label(
                    "This is a community script. It was written by a community member and has not been reviewed by the Volcren team. \
                     It runs on your computer and controls your game client, so only run scripts from people you trust.",
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("You won't be asked again until a new revision of this script comes out.").weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Run anyway").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                if let Some(revision) = script.revision {
                    self.confirmed.confirm(&script.id, revision);
                }
                self.start_store_script(ctx, script, target);
            }
            Some(false) => {}
            None => self.warning = Some((script, target)),
        }
    }

    /// The name of the script that is running, a store script's title or the editor's file: the page that replaces Scripts and Editor.
    fn running_title(&self) -> Option<String> {
        match &self.store_run {
            Some(run) => Some(run.title.clone()),
            None if self.script.as_ref().is_some_and(Script::is_running) => Some(self.script_file.name()),
            None => None,
        }
    }

    /// The page tree on the left, after the website's neo-tree. Picking a page closes the drawer on narrow windows.
    fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        crate::theme::sidebar_header(ui, "explorer");
        crate::theme::sidebar_root(ui, "\u{f233}", "volcren");
        let before = self.tab;
        let running = self.running_title().map(|title| sidebar_name(&title));
        // a running script's page takes the place of both Scripts and Editor
        let items = [
            (Tab::Clients, "\u{f108}", "client".to_owned()),
        ]
        .into_iter()
        .chain(match running {
            Some(name) => vec![(Tab::Running, "\u{f04b}", name)],
            None => vec![
                (Tab::Store, "\u{f489}", "scripts".to_owned()),
                (Tab::Scripts, "\u{f044}", "editor".to_owned()),
            ],
        })
        .chain([
            (Tab::Development, "\u{f188}", "development".to_owned()),
            (Tab::Terminal, "\u{f120}", "terminal".to_owned()),
            (Tab::Accounts, "\u{f007}", "accounts".to_owned()),
            (Tab::Settings, "\u{f013}", "settings".to_owned()),
        ])
        .collect::<Vec<_>>();
        for (i, (tab, icon, name)) in items.iter().enumerate() {
            crate::theme::sidebar_item(ui, &mut self.tab, *tab, icon, name, i == items.len() - 1);
        }
        if self.tab != before {
            self.sidebar_open = false;
        }
    }

    fn update_banner_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let Some(release) = self.update.clone() else { return };
        // Added before the log panel so it takes the very bottom of the window.
        egui::Panel::bottom("update").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(format!("Version {} is available", release.version));
                    ui.label(format!("You have version {}", update::VERSION));
                });
                // Each button is as tall as a text line, in a slightly smaller font, and as wide as the longer label.
                let row = ui.text_style_height(&egui::TextStyle::Body);
                let font = egui::FontId::monospace(12.0);
                let width = ui.painter().layout_no_wrap("Download".into(), font.clone(), egui::Color32::WHITE).size().x
                    + 2.0 * ui.spacing().button_padding.x;
                ui.add_space((ui.available_width() - width).max(0.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().button_padding.y = 0.0;
                    let size = egui::vec2(width, row);
                    let button = |text: &str| egui::Button::new(egui::RichText::new(text).font(font.clone())).min_size(size);
                    if let Some(url) = release.download_url(&self.api)
                        && ui.add(button("Download")).clicked()
                    {
                        let _ = webbrowser::open(&url);
                    }
                    if ui.add(button("Dismiss")).clicked() {
                        self.update = None;
                        ctx.request_repaint();
                    }
                });
            });
        });
    }

    fn account_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        match &self.auth {
            Auth::SignedIn(me) => {
                let (name, balance) = (me.username.clone(), store::balance_label(me));
                if ui.button("Sign out").clicked() {
                    self.sign_out(None);
                    return;
                }
                if !self.api.config().site_url.is_empty() && ui.button("Buy credits").clicked() {
                    let _ = webbrowser::open(&format!("{}/credits", self.api.config().site_url));
                }
                ui.label(balance);
                ui.label(egui::RichText::new(name).strong());
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

    /// Shown instead of the app until the account is signed in: the loader doesn't start without a Discord login.
    fn login_gate_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(16.0));
            ui.label(egui::RichText::new("Sign in to continue").size(20.0).strong());
            ui.add_space(8.0);
            match &self.auth {
                Auth::Restoring => {
                    ui.spinner();
                    ui.label("Checking for a saved sign-in…");
                }
                Auth::SigningIn(cancel) => {
                    ui.spinner();
                    ui.label("Waiting for you to sign in in the browser…");
                    ui.add_space(8.0);
                    if ui.button("Cancel").clicked() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                _ => {
                    ui.label("Volcren needs a Discord account. Your browser will open to sign in.");
                    ui.add_space(8.0);
                    if ui.button(egui::RichText::new("Sign in with Discord").size(16.0)).clicked() {
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
        let mut layers: Vec<String> = DEV_LAYERS
            .iter()
            .zip(&self.dev_toggles)
            .filter(|(_, on)| **on)
            .filter_map(|(layer, _)| layer.map(str::to_owned))
            .collect();
        // the widget inspector's selection is boxed on the client while its tab is showing
        if self.widget_inspector_showing() {
            layers.extend(self.widgets.highlight_layers());
        }
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

    // Reads the selected client's packet log while the monitor is capturing, a page at a time.
    fn poll_packets(&mut self, ctx: &egui::Context) {
        if !self.packets.capture {
            return;
        }
        let Some(target) = self.selected_target().filter(|t| t.client.loaded.is_some()) else { return };
        let now = Instant::now();
        // look again when the next read is due, even if nothing else repaints
        ctx.request_repaint_after(crate::packets::POLL_EVERY);
        if self.packets.wants_layouts(target.client.pid, now) {
            self.packets.start_layouts(target.client.pid);
            let (pid, target) = (target.client.pid, target.clone());
            self.background(ctx, move |settings| Event::PacketLayouts { pid, result: backend::packet_layouts(&target, &settings) });
        }
        if !self.packets.due(target.client.pid, now) {
            return;
        }
        let (pid, since) = (target.client.pid, self.packets.start(target.client.pid));
        self.background(ctx, move |settings| Event::Packets { pid, result: backend::packets(&target, &settings, since) });
    }

    // The Packet Inspector tab: reads the selected client's packets while Enable is on.
    // Reads the selected client's varps while the var inspector is enabled; the job also looks up the varbits of varps that changed.
    fn poll_vars(&mut self, ctx: &egui::Context) {
        if !self.vars.enabled {
            return;
        }
        let Some(target) = self.selected_target().filter(|t| t.client.loaded.is_some()) else { return };
        let now = Instant::now();
        ctx.request_repaint_after(crate::vars::POLL_EVERY);
        if !self.vars.due(target.client.pid, now) {
            return;
        }
        let pid = target.client.pid;
        let crate::vars::Request { previous, known, varc_since } = self.vars.start(pid);
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::vars::Poll, String> {
                let varps = backend::varps(&target, &settings)?;
                let (mut tick, mut varbits) = (None, Vec::new());
                if let (Some(varps), Some(previous)) = (&varps, &previous) {
                    let changed = crate::vars::Inspector::unknown(previous, varps, &known);
                    if previous != varps {
                        tick = backend::state(&target, &settings).ok().and_then(|state| state.tick_count);
                    }
                    for varp in changed {
                        varbits.push((varp, backend::varbits_of(&target, &settings, varp)?));
                    }
                }
                let (varcs, varc_error) = match backend::varc_changes(&target, &settings, varc_since) {
                    Ok(page) => (page, None),
                    Err(error) => (injector::VarcPage { next: varc_since, ..Default::default() }, Some(error)),
                };
                // the game tick the changes are shown under, read when there is something to show
                if tick.is_none() && !varcs.changes.is_empty() {
                    tick = backend::state(&target, &settings).ok().and_then(|state| state.tick_count);
                }
                Ok(crate::vars::Poll { varps, tick, varbits, varcs, varc_error })
            };
            Event::Vars { pid, result: read() }
        });
    }

    // The widget inspector's "Follow mouse": looks at where the client's mouse is every so often and selects the widget there.
    fn poll_widget_mouse(&mut self, ctx: &egui::Context) {
        if !self.widgets.follow_mouse || !self.widget_inspector_showing() {
            return;
        }
        let Some(target) = self.selected_target().filter(|t| t.client.loaded.is_some()) else { return };
        ctx.request_repaint_after(crate::widgets::FOLLOW_EVERY);
        if !self.widgets.follow_due(Instant::now()) {
            return;
        }
        let pid = target.client.pid;
        self.background(ctx, move |settings| {
            let result = backend::connection(&target, &settings).map(|connection| connection.filter(|c| c.mouse_x >= 0 && c.mouse_y >= 0).map(|c| (c.mouse_x, c.mouse_y)));
            Event::WidgetMouse { pid, result }
        });
    }

    // Reads every open interface of the selected client for the widget inspector, and the names of the groups it has not named yet.
    fn refresh_widgets(&mut self, ctx: &egui::Context, target: Target) {
        let pid = target.client.pid;
        self.widgets.start(pid);
        let known = self.widgets.known_groups();
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::widgets::Poll, String> {
                let tree = backend::widget_tree(&target, &settings)?;
                let mut groups: Vec<i32> = tree.widgets.iter().map(|w| w.widget.group).filter(|group| !known.contains(group)).collect();
                groups.sort_unstable();
                groups.dedup();
                let names = if groups.is_empty() {
                    Vec::new()
                } else {
                    let names = backend::game_val_names(&target, &settings, crate::widgets::INTERFACE_GAME_VAL, &groups)?;
                    groups.into_iter().zip(names).filter_map(|(group, name)| Some((group, name?))).collect()
                };
                Ok(crate::widgets::Poll { tree, names })
            };
            Event::Widgets { pid, result: read() }
        });
    }

    // The Widget Inspector tab of the Development page: the open interfaces as a tree and the fields of the selected widget.
    fn widgets_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        ui.horizontal_wrapped(|ui| {
            let busy = self.widgets.reading();
            if ui.add_enabled(target.is_some() && !busy, egui::Button::new("Refresh")).clicked()
                && let Some(target) = target.clone()
            {
                self.refresh_widgets(&ctx, target);
            }
            ui.checkbox(&mut self.widgets.hide_hidden, "Hide hidden");
            ui.checkbox(&mut self.widgets.follow_mouse, "Follow mouse")
                .on_hover_text("Selects the widget under the client's mouse, looking about ten times a second. Move the mouse over the client and the tree follows; untick it to keep the selection.");
        });
        match &target {
            Some(target) => ui.weak(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        // the first look at the tab reads the interfaces without being asked
        if let Some(target) = target
            && !self.widgets.has_read()
            && !self.widgets.reading()
            && self.widgets.error.is_none()
        {
            self.refresh_widgets(&ctx, target);
        }
        if let Some(error) = &self.widgets.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        let inspector = &mut self.widgets;
        let mut picked = inspector.selected;
        let open = inspector.open_path();
        let reveal = std::mem::take(&mut inspector.reveal);
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

    // Reads the selected client's script log while the script inspector is enabled.
    fn poll_script_log(&mut self, ctx: &egui::Context) {
        if !self.script_inspector.enabled {
            return;
        }
        let Some(target) = self.selected_target().filter(|t| t.client.loaded.is_some()) else { return };
        let now = Instant::now();
        ctx.request_repaint_after(crate::script_log::POLL_EVERY);
        if !self.script_inspector.due(target.client.pid, now) {
            return;
        }
        let (pid, since) = (target.client.pid, self.script_inspector.start(target.client.pid));
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::script_log::Poll, String> {
                let page = backend::script_runs(&target, &settings, since)?;
                Ok(crate::script_log::Poll { page })
            };
            Event::ScriptRuns { pid, result: read() }
        });
    }

    // Reads the selected client's item containers while the inventory inspector is enabled; the job also looks up the names it hasn't got.
    fn poll_inventory(&mut self, ctx: &egui::Context) {
        if !self.inventory.enabled {
            return;
        }
        let Some(target) = self.selected_target().filter(|t| t.client.loaded.is_some()) else { return };
        let now = Instant::now();
        if let Some(wait) = self.inventory.retry_in(now) {
            ctx.request_repaint_after(wait);
        }
        if !self.inventory.due(target.client.pid, now) {
            return;
        }
        let pid = target.client.pid;
        let crate::inventory::Request { since, known_containers, known_items } = self.inventory.start(pid);
        self.background(ctx, move |settings| {
            let read = || -> Result<crate::inventory::Poll, String> {
                // the first read marks the place in the log, then reads the containers: a change in between is applied twice, harmlessly
                let (page, baseline) = match since {
                    Some(since) => (backend::item_changes(&target, &settings, since, crate::inventory::WAIT_FOR_CHANGE)?, None),
                    None => {
                        let page = backend::item_changes(&target, &settings, injector::ITEM_CHANGES_NOW, std::time::Duration::ZERO)?;
                        (page, Some(backend::item_containers(&target, &settings)?))
                    }
                };
                let missing = |known: &[i32], mut ids: Vec<i32>| {
                    ids.retain(|id| *id >= 0 && !known.contains(id));
                    ids.sort_unstable();
                    ids.dedup();
                    ids
                };
                let named = |ids: &[i32], names: Vec<Option<String>>| -> Vec<(i32, String)> {
                    ids.iter().zip(names).filter_map(|(id, name)| Some((*id, name?))).collect()
                };
                let mut containers: Vec<i32> = page.changes.iter().map(|change| change.container).collect();
                let mut items: Vec<i32> = page.changes.iter().flat_map(|change| [change.old_item, change.new_item]).collect();
                for (id, held) in baseline.iter().flatten() {
                    containers.push(*id);
                    items.extend(held.iter().map(|item| item.id));
                }
                let new_containers = missing(&known_containers, containers);
                let new_items = missing(&known_items, items);
                let container_names = if new_containers.is_empty() {
                    Vec::new()
                } else {
                    named(&new_containers, backend::game_val_names(&target, &settings, crate::inventory::INV_GAME_VAL, &new_containers)?)
                };
                let (item_names, stackable) = if new_items.is_empty() {
                    (Vec::new(), Vec::new())
                } else {
                    let stacks = backend::item_stackable(&target, &settings, &new_items)?;
                    (named(&new_items, backend::item_names(&target, &settings, &new_items)?), new_items.iter().zip(stacks).filter_map(|(id, stacks)| Some((*id, stacks?))).collect())
                };
                Ok(crate::inventory::Poll { page, baseline, container_names, item_names, stackable })
            };
            Event::Inventory { pid, result: read() }
        });
    }

    // The Inventory Inspector tab of the Development page: snapshots of the client's item containers while Enable is on.
    fn inventory_ui(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        let inspector = &mut self.inventory;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut inspector.enabled, "Enable");
            if ui.button(format!("Refresh ({})", inspector.pending())).on_hover_text("Adds the changes read since the last refresh to the list.").clicked() {
                inspector.refresh();
            }
            if ui.button("Clear").clicked() {
                inspector.clear();
            }
        });
        match &target {
            Some(target) => ui.weak(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        if let Some(error) = &inspector.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        let mut picked = inspector.selected;
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical().id_salt("inventory_tree").auto_shrink([false, false]).show(&mut columns[0], |ui| {
                for (id, log) in inspector.logs() {
                    egui::CollapsingHeader::new(inspector.container_label(*id)).id_salt(("inventory_container", *id)).show(ui, |ui| {
                        for (index, snapshot) in log.iter().enumerate() {
                            let label = snapshot.tick.map_or("Tick: ?".to_owned(), |tick| format!("Tick: {tick}"));
                            if ui.selectable_label(picked == Some((*id, index)), label).clicked() {
                                picked = Some((*id, index));
                            }
                        }
                    });
                }
            });
            egui::ScrollArea::vertical().id_salt("inventory_items").auto_shrink([false, false]).show(&mut columns[1], |ui| match inspector.picked() {
                Some((snapshot, before)) => inventory_items_ui(ui, inspector, snapshot, before),
                None => {
                    ui.weak("Pick a tick to see the items.");
                }
            });
        });
        inspector.selected = picked;
    }

    // The Script Inspector tab of the Development page: a log of the scripts the client runs for events while Enable is on.
    fn script_log_ui(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        let mut load_names = false;
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
        });
        match &target {
            Some(target) => ui.weak(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        if let Some(error) = &inspector.names_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let visible = inspector.visible();
        let names = &inspector.names;
        if !names.is_empty() {
            ui.weak(format!("{} script names, from RuneLite's cs2-scripts at {}.", names.len(), names.source));
        }
        ui.weak(format!(
            "{} shown of {} kept{}. Scripts started by trigger lookup aren't listed.",
            visible.len(),
            inspector.kept(),
            if inspector.dropped > 0 { format!(", {} missed", inspector.dropped) } else { String::new() }
        ));
        if let Some(error) = &inspector.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().item_spacing.y;
        let table = LogTable { id: "script_log", follow: inspector.follow, row_height, max_height: None, clickable: false };
        table.show(ui, vec![("Tick", narrow(60.0)), ("Script", narrow(240.0)), ("Arguments", narrow(260.0)), ("Time", egui_extras::Column::remainder().at_least(60.0).clip(true))], visible.len(), |mut row| {
            let index = row.index();
            let entry = &visible[index];
            // the tick is shown on the first run of each tick
            let first = index == 0 || visible[index - 1].run.tick != entry.run.tick;
            row.col(|ui| {
                if first {
                    ui.label(mono(entry.run.tick.map_or("?".to_owned(), |tick| tick.to_string())));
                }
            });
            row.col(|ui| {
                ui.label(mono(format!("{}{}", "  ".repeat(entry.run.depth as usize), entry.name(names))));
            });
            row.col(|ui| {
                ui.label(mono(entry.args_text()));
            });
            row.col(|ui| {
                ui.label(mono(entry.took()));
            });
        });
        drop(visible);
        if load_names {
            inspector.loading_names = true;
            inspector.names_error = None;
            self.background(ui.ctx(), |_| Event::ScriptNames(crate::script_names::Names::download()));
        }
    }

    // The Var Inspector tab of the Development page: a log of the varps and varbits that change while Enable is on.
    fn vars_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Varps").default_open(false).show(ui, |ui| self.varps_ui(ui));
        ui.separator();
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
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
        });
        match &target {
            Some(target) => ui.weak(format!("Client {}.", target.client.pid)),
            None => ui.label("Select a client with agent loaded in the Clients tab."),
        };
        let visible = inspector.visible();
        ui.weak(format!("{} shown of {} kept. Client variables are listed as their index.", visible.len(), inspector.kept()));
        if let Some(error) = &inspector.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(error) = &inspector.varc_error {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Client variables: {error}"));
        }
        ui.separator();
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().item_spacing.y;
        let table = LogTable { id: "vars", follow: inspector.follow, row_height, max_height: None, clickable: false };
        table.show(ui, vec![("Tick", narrow(60.0)), ("Kind", narrow(100.0)), ("Name", narrow(220.0)), ("Old", narrow(110.0)), ("New", egui_extras::Column::remainder().at_least(60.0).clip(true))], visible.len(), |mut row| {
            let index = row.index();
            let entry = &visible[index];
            // the tick is shown on the first change of each tick
            let first = index == 0 || visible[index - 1].tick != entry.tick;
            let kind = match entry.kind {
                crate::vars::Kind::Varbit => "Varbit",
                crate::vars::Kind::Varp => "VarPlayer",
                crate::vars::Kind::VarcInt => "VarClientInt",
                crate::vars::Kind::VarcStr => "VarClientStr",
            };
            row.col(|ui| {
                if first {
                    ui.label(mono(entry.tick.map_or("?".to_owned(), |tick| tick.to_string())));
                }
            });
            row.col(|ui| {
                ui.label(mono(kind));
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
        });
    }

    fn packets_ui(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        let monitor = &mut self.packets;
        {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut monitor.capture, "Enable");
                ui.checkbox(&mut monitor.received, "Received");
                ui.checkbox(&mut monitor.sent, "Sent");
                ui.checkbox(&mut monitor.follow, "Follow");
                if ui.button("Clear").on_hover_text("Forget the packets read so far.").clicked() {
                    monitor.clear();
                }
            });
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
            match &target {
                Some(target) => ui.weak(format!("Client {}.", target.client.pid)),
                None => ui.label("Select a client with agent loaded in the Clients tab."),
            };
            let visible = monitor.visible();
            let table = monitor.table();
            ui.weak(format!(
                "{} shown of {} kept{}",
                visible.len(),
                monitor.kept(),
                if monitor.dropped > 0 { format!(", {} missed", monitor.dropped) } else { String::new() }
            ));
            if let Some(error) = &monitor.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();

            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + ui.spacing().item_spacing.y;
            let sent_color = ui.visuals().hyperlink_color;
            let detail_height = 150.0;
            let mut picked = None;
            let height = (ui.available_height() - detail_height - 12.0).max(row_height * 4.0);
            let log = LogTable { id: "packets", follow: monitor.follow, row_height, max_height: Some(height), clickable: true };
            let selected = monitor.selected;
            log.show(
                ui,
                vec![
                    ("Time", narrow(70.0)),
                    ("", narrow(24.0)),
                    ("Opcode", narrow(56.0)),
                    ("Length", narrow(50.0)),
                    ("Name", narrow(120.0)),
                    ("Fields", narrow(240.0)),
                    ("Hex", egui_extras::Column::remainder().at_least(80.0).clip(true)),
                ],
                visible.len(),
                |mut row| {
                    let packet = &visible[row.index()];
                    let def = table.lookup(packet.sent, packet.opcode);
                    let colored = |text: String| {
                        let text = mono(text);
                        if packet.sent { text.color(sent_color) } else { text }
                    };
                    row.set_selected(selected == Some(packet.sequence));
                    row.col(|ui| {
                        ui.label(colored(format!("{:.3}", packet.time_ms as f64 / 1000.0)));
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
                        let fields = def.map(|def| crate::packet_defs::decode(def, &crate::packets::payload_bytes(packet), packet.length).fields.into_iter().map(|(_, value)| value).collect::<Vec<_>>().join(", "));
                        ui.label(colored(fields.unwrap_or_default()));
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
                // looking at a packet while the list keeps scrolling is no use
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
                        ui.label(egui::RichText::new(if packet.sent { "Sent" } else { "Received" }).strong());
                        ui.label(format!("opcode {}, {} bytes, at {:.3}s", packet.opcode, packet.length, packet.time_ms as f64 / 1000.0));
                        if (bytes.len() as u32) < packet.length {
                            ui.weak(format!("(first {} kept)", bytes.len()));
                        }
                        if ui.small_button("Copy hex").clicked() {
                            ui.ctx().copy_text(crate::packets::payload_text(packet));
                        }
                    });
                    let values = crate::packets::leading_values(&bytes);
                    if !values.is_empty() {
                        ui.weak(values.iter().map(|(kind, value)| format!("{kind} {value}")).collect::<Vec<_>>().join("  ·  ") + "  (big endian, from the start)");
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

    fn accounts_ui(&mut self, ui: &mut egui::Ui) {
        let target = self.selected_target().filter(|t| t.client.loaded.is_some());
        if let Some(crate::accounts::Action::ReadClientName) = self.accounts.ui(ui, target.is_some())
            && let Some(target) = target
        {
            let ctx = ui.ctx().clone();
            self.background(&ctx, move |settings| {
                Event::AccountName(backend::state(&target, &settings).map(|state| state.account_name))
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
            // Tiles and the XP tracker's reads run in the background, so they must not clear another job's busy label.
            if !matches!(event, Event::Tiles { .. } | Event::XpState(_) | Event::AccountName(_) | Event::Packets { .. } | Event::PacketLayouts { .. } | Event::Vars { .. } | Event::ScriptRuns { .. } | Event::Inventory { .. } | Event::Widgets { .. } | Event::WidgetMouse { .. }) {
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
                Event::Restored(result) => match result {
                    Ok(Some(me)) => self.signed_in(ctx, me),
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
                    Ok(me) => self.signed_in(ctx, me),
                    Err(error) => {
                        self.auth = Auth::SignedOut;
                        self.error(error);
                    }
                },
                Event::Update(result) => match result {
                    Ok(release) => self.update = release,
                    // Offline or no release published yet: not worth an error line.
                    Err(error) => self.info(format!("Couldn't check for updates: {error}")),
                },
                Event::Me(result) => match result {
                    Ok(me) => self.auth = Auth::SignedIn(me),
                    Err(ApiError::Unauthorized | ApiError::SignedOut(_)) => self.sign_out(Some("your session expired.".into())),
                    Err(error) => self.error(format!("Couldn't refresh your account: {error}")),
                },
                Event::Scripts(result) => {
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
                Event::Redeemed(result) => {
                    self.store.redeeming = false;
                    match result {
                        Ok(_) => {
                            self.store.code.clear();
                            self.store.redeem_result = Some((true, "Added to your scripts.".into()));
                            self.fetch_scripts(ctx);
                        }
                        Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
                        Err(error) => self.store.redeem_result = Some((false, error.to_string())),
                    }
                }
                Event::GrantRemoved { id, result } => {
                    self.store.removing = None;
                    match result {
                        Ok(_) => {
                            self.confirmed.forget(&id);
                            self.store.redeem_result = Some((true, "Removed from your account.".into()));
                            self.fetch_scripts(ctx);
                        }
                        Err(ApiError::SignedOut(why)) => self.sign_out(Some(why)),
                        Err(error) => self.store.redeem_result = Some((false, error.to_string())),
                    }
                }
                Event::StoreStarted { pid, title, open, result } => {
                    self.starting = None;
                    match result {
                        Ok(prepared) => self.run_prepared(ctx, pid, &title, open, prepared),
                        Err(error) => {
                            self.script_error(format!("Couldn't start {title}: {error}"));
                            // The list may be stale (a new revision, a grant that was revoked)
                            self.fetch_scripts(ctx);
                        }
                    }
                }
                Event::Heartbeat { session_id, sent, result } => {
                    let now = std::time::Instant::now();
                    if let (Ok(beat), Some(me)) = (&result, self.auth.me_mut())
                        && let Some(credits) = beat.credits
                    {
                        me.credits = credits;
                    }
                    let verdict = match self.store_run.as_mut().filter(|run| run.session_id == session_id) {
                        Some(run) => run.beat.on_result(now, sent, result),
                        None => Verdict::Continue,
                    };
                    if let Verdict::Stop { reason, sign_out } = verdict {
                        if let Some(run) = &mut self.store_run {
                            run.stopping = true;
                        }
                        if let Some(script) = &mut self.script {
                            script.stop();
                        }
                        self.script_error(reason.clone());
                        if sign_out {
                            self.sign_out(Some(reason));
                        }
                    }
                }
                Event::SessionEnded(result) => match result {
                    Ok(ended) => {
                        self.script_info(format!("Session ended, {} credits charged for the last minutes.", ended.charged));
                        self.api_job(ctx, |api| Event::Me(api.me()));
                    }
                    Err(error) => self.script_error(format!("Couldn't end the session: {error}. The server will end it within 5 minutes.")),
                },
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
                Event::AccountName(result) => self.accounts.client_name(result),
                Event::XpState(result) => {
                    self.xp_reading = false;
                    self.xp_next = std::time::Instant::now() + XP_READ_EVERY;
                    // a failed or empty read (logged out, client closing) is skipped; the baseline stays
                    let skills = result.ok().and_then(|state| state.skills).filter(|skills| skills.iter().any(|s| s.xp > 0));
                    if let Some(skills) = skills.filter(|_| self.script.as_ref().is_some_and(Script::is_running)) {
                        let skills = skills
                            .into_iter()
                            .map(|s| crate::xp::SkillXp { name: s.name, level: s.real_level as i64, xp: s.xp as i64 })
                            .collect();
                        self.xp.update(skills, self.run_time());
                    }
                }
                Event::Packets { pid, result } => self.packets.finish(pid, Instant::now(), result),
                Event::PacketLayouts { pid, result } => self.packets.finish_layouts(pid, result),
                Event::ScriptNames(result) => self.script_inspector.finish_names(result),
                Event::ScriptRuns { pid, result } => self.script_inspector.finish(pid, Instant::now(), result),
                Event::Inventory { pid, result } => self.inventory.finish(pid, Instant::now(), result),
                Event::Widgets { pid, result } => self.widgets.finish(pid, result),
                Event::WidgetMouse { pid, result } => {
                    self.widgets.mouse_done();
                    match result {
                        Ok(Some((x, y))) if self.widgets.follow_mouse && self.widgets.has_pid(pid) => self.widgets.pick_at(x, y),
                        Ok(_) => {}
                        Err(error) => self.widgets.error = Some(error),
                    }
                }
                Event::Vars { pid, result } => self.vars.finish(pid, Instant::now(), result, known_varp_name),
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

            ui.label("lib folder").on_hover_text("Optional. Scripts you run from a file read require(\"../lib/...\") from this folder, e.g. your lib checkout. Store scripts always use the downloaded lib.");
            ui.add(egui::TextEdit::singleline(&mut self.settings.lib_dir).desired_width(460.0).hint_text("empty: the lib beside the script, or the built-in one"));
            if ui.button("Browse…").clicked() {
                pick_folder(&mut self.settings.lib_dir);
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
        // The entries are saved with the rest of the settings (`save`).
        self.whitelist_page.ui(ui, &mut self.http_whitelist);
    }

    fn clients_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        ui.horizontal(|ui| {
            ui.heading("Clients");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.busy.is_none(), egui::Button::new("Refresh")).clicked() {
                    self.scan(&ctx);
                }
            });
        });
        ui.add_space(4.0);

        if self.targets.is_empty() {
            if self.busy.is_none() {
                ui.label(egui::RichText::new("No game clients found.").color(crate::theme::COMMENT));
            }
        } else {
            self.step_selected_client(&ctx);
            let show_prefix = !cfg!(windows);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height((ui.available_height() - 120.0).max(60.0))
                .show(ui, |ui| {
                    egui::Grid::new("clients")
                        .num_columns(if show_prefix { 4 } else { 3 })
                        .striped(true)
                        .min_col_width(60.0)
                        .spacing(egui::vec2(16.0, 6.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("PID").color(crate::theme::COMMENT));
                            ui.label(egui::RichText::new("Title").color(crate::theme::COMMENT));
                            if show_prefix {
                                ui.label(egui::RichText::new("Prefix").color(crate::theme::COMMENT));
                            }
                            ui.label(egui::RichText::new("Status").color(crate::theme::COMMENT));
                            ui.end_row();

                            for target in &self.targets {
                                let client = &target.client;
                                let enabled = client.foreign_bot.is_none();
                                // the whole row, gaps included, is the click target; the highlight is painted behind the cells afterwards
                                let highlight = ui.painter().add(egui::Shape::Noop);
                                let mut row = egui::Rect::NOTHING;
                                let mut cell = |ui: &mut egui::Ui, text: egui::RichText| {
                                    let response = ui.add(egui::Label::new(text).selectable(false));
                                    row = row.union(response.rect);
                                    response
                                };
                                let is_selected = self.selected.as_ref() == Some(&target.key());
                                let pid = egui::RichText::new(client.pid.to_string());
                                cell(ui, if is_selected { pid.strong().color(crate::theme::GREEN) } else { pid });
                                cell(ui, egui::RichText::new(&client.title)).on_hover_text(&client.exe);
                                if show_prefix {
                                    cell(ui, egui::RichText::new(target.location.label().unwrap_or_default()));
                                }
                                if let Some(bot) = &client.foreign_bot {
                                    cell(ui, egui::RichText::new(format!("Controlled by {bot}")).color(crate::theme::RED));
                                } else if let Some(module) = &client.loaded {
                                    let version = client.version.as_deref().unwrap_or("unknown version");
                                    let (text, color) = if self.dll_version.as_deref().is_some_and(|v| v != version) {
                                        (format!("Loaded {version}, outdated"), crate::theme::YELLOW)
                                    } else {
                                        (format!("Loaded {version}"), crate::theme::GREEN)
                                    };
                                    cell(ui, egui::RichText::new(text).color(color)).on_hover_text(module);
                                } else if !client.is_64bit {
                                    cell(ui, egui::RichText::new("32-bit, unsupported").color(crate::theme::RED));
                                } else {
                                    cell(ui, egui::RichText::new("Not loaded").color(crate::theme::COMMENT));
                                }
                                ui.end_row();
                                let row = row.expand2(egui::vec2(8.0, 3.0));
                                let hovered = enabled
                                    && ui.is_rect_visible(row)
                                    && ui.input(|i| i.pointer.hover_pos()).is_some_and(|pos| row.contains(pos) && ui.clip_rect().contains(pos));
                                if hovered {
                                    ui.painter().set(highlight, egui::Shape::rect_filled(row, 3.0, ui.visuals().widgets.hovered.weak_bg_fill));
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    if ui.input(|i| i.pointer.primary_clicked()) {
                                        self.selected = Some(target.key());
                                    }
                                }
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

        ui.add_space(8.0);
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
        });

        ui.add_space(8.0);
        egui::CollapsingHeader::new("Client options").default_open(false).show(ui, |ui| {
            let mut changed = false;
            egui::Grid::new("client_options").num_columns(2).spacing(egui::vec2(16.0, 6.0)).show(ui, |ui| {
                ui.label("Mouse cursor");
                ui.horizontal(|ui| {
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
                    if (ui.add_enabled(can_unload, egui::Button::new(label)).on_hover_text(hint).clicked() || submitted)
                        && let Some(target) = selected.clone()
                    {
                        self.set_display_name(&ctx, target);
                    }
                });
                ui.end_row();
            });
            if changed && let Some(target) = selected {
                self.set_cursor(&ctx, target);
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
        let keys: Vec<String> = self.targets.iter().filter(|t| t.client.foreign_bot.is_none()).map(Target::key).collect();
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
    }

    fn selected_target(&self) -> Option<Target> {
        self.selected
            .as_ref()
            .and_then(|key| self.targets.iter().find(|t| &t.key() == key && t.client.foreign_bot.is_none()))
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
        if let Some(script) = &mut self.script {
            if script.stop_ignored(Duration::from_secs(2)) {
                self.script_error("The script's process didn't exit after being killed. It will end when the loader closes.");
            } else if script.is_running() && script.stop_requested() {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        let events = self.script.as_mut().map(Script::poll).unwrap_or_default();
        for event in events {
            let finished = matches!(event, ScriptEvent::Finished { .. });
            match event {
                ScriptEvent::Output(line) => self.script_info(line),
                ScriptEvent::Form(spec) => {
                    self.tab = Tab::Running;
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
                // Only a store run has a session to send them to; stats from a local run are dropped.
                ScriptEvent::Stats(experience, gold, custom) => {
                    if let Some(run) = &mut self.store_run {
                        run.beat.report(experience, gold, &custom);
                    }
                }
                ScriptEvent::Counters(counters) => self.counters.update(counters, self.run_time()),
                ScriptEvent::PinRequest(hash) => {
                    let answer = self.accounts.pin_for(&hash);
                    if let Some(script) = &mut self.script
                        && let Err(error) = script.answer_pin(answer)
                    {
                        self.script_error(error);
                    }
                }
                ScriptEvent::HttpRequest(payload) => {
                    if let Some(script) = &self.script {
                        script.answer_http(&payload, self.http_whitelist.clone());
                    }
                }
                ScriptEvent::Breaks(plan) => self.breaks.update(plan, std::time::Instant::now()),
                ScriptEvent::Finished { stopped: true, .. } => self.script_info("Script stopped."),
                ScriptEvent::Finished { result: Ok(()), .. } => self.script_info("Script finished."),
                ScriptEvent::Finished {
                    result: Err(error), ..
                } => self.script_error(format!("Script failed: {error}")),
            }
            if finished {
                self.run_ended = Some(std::time::Instant::now());
                self.end_store_session(ctx);
            }
        }
    }

    /// Drops a form whose script is gone. The form itself is shown on the script's page (`running_ui`, or the editor's `scripts_ui` for a local run).
    fn form_ui(&mut self) {
        if self.script.as_ref().is_none_or(|s| !s.is_running()) {
            self.form = None;
        }
    }

    /// Sends the script what the user did with its form, and remembers the values for next time.
    fn answer_form(&mut self, outcome: crate::form::Outcome) {
        let answer = match outcome {
            crate::form::Outcome::Open => return,
            crate::form::Outcome::Submitted(values) => {
                if let (serde_json::Value::Object(map), Some(form)) = (&values, &self.form) {
                    self.remembered_forms.insert(form.title.clone(), map.clone());
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

    /// The page of the running script, store or local: its controls, and its settings form while it is waiting for one.
    fn running_ui(&mut self, ui: &mut egui::Ui) {
        let Some(title) = self.running_title() else { return };
        let running = self.script.as_ref().is_some_and(Script::is_running);
        let paused = running && self.script.as_ref().is_some_and(Script::is_paused);
        let stopping = self.script.as_ref().is_some_and(Script::stop_requested);
        ui.heading(&title);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(running && !stopping, egui::Button::new(if paused { "Resume" } else { "Pause" }))
                .on_hover_text(if self.store_run.is_some() {
                    "Suspends the script's process. It is still billed while paused: stop it to stop billing."
                } else {
                    "Suspends the script's process, so it takes effect immediately, even in a tight loop."
                })
                .clicked()
            {
                self.toggle_pause();
            }
            if ui.add_enabled(running && !stopping, egui::Button::new("Stop")).clicked()
                && let Some(script) = &mut self.script
            {
                script.stop();
            }
            ui.separator();
            ui.label(match (running, stopping, paused) {
                (false, ..) => "ended",
                (_, true, _) => "stopping",
                (_, _, true) => "paused",
                _ => "running",
            });
        });
        ui.add_space(8.0);
        ui.separator();
        match &mut self.form {
            Some(form) => {
                ui.label(egui::RichText::new(&form.title).strong());
                let outcome = form.show_inline(ui);
                self.answer_form(outcome);
            }
            None => {
                ui.label(egui::RichText::new("The script isn't asking for settings.").weak());
            }
        }
    }

    fn toggle_pause(&mut self) {
        let Some(script) = &mut self.script else { return };
        let paused = script.is_paused();
        script.set_paused(!paused);
        self.script_info(if paused { "Script resumed." } else { "Script paused." });
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
                self.viewing = None;
                self.parked = None;
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

    /// Opens a store script's entry file in the editor, read-only, so every script can be read. The user's own file is parked until they leave the view.
    fn view_store_source(&mut self, title: &str, revision: i64, entry: &std::path::Path, source: &str) {
        if self.viewing.is_none() {
            self.parked = Some(std::mem::take(&mut self.script_file));
        }
        self.script_file = ScriptFile {
            path: entry.display().to_string(),
            text: source.to_owned(),
            disk: Some(source.to_owned()),
        };
        self.viewing = Some(format!("{title} (revision {revision})"));
    }

    /// Closes the read-only view and brings back the file that was open before it.
    fn leave_view(&mut self) {
        if self.viewing.take().is_some() {
            self.script_file = self.parked.take().unwrap_or_default();
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

    fn run_prepared(&mut self, ctx: &egui::Context, pid: u32, title: &str, open: bool, prepared: Prepared) {
        let session_id = prepared.session.session_id.clone();
        let Some(target) = self.targets.iter().find(|t| t.client.pid == pid && t.client.loaded.is_some()).cloned() else {
            self.script_error(format!("The client {pid} is gone, so {title} wasn't started."));
            self.api_job(ctx, move |api| Event::SessionEnded(api.end_session(&session_id)));
            return;
        };
        match Script::start_with(ctx, &prepared.source, &prepared.modules, Some(&prepared.entry), None, Some(&target), &self.settings) {
            Ok(script) => {
                if let Some(warning) = &prepared.warning {
                    self.script_info(format!("Warning: {warning}"));
                }
                self.script_info(format!("Running {title} (revision {}).", prepared.session.revision));
                self.script = Some(script);
                if open && !prepared.premium {
                    self.view_store_source(title, prepared.session.revision, &prepared.entry, &prepared.source);
                } else {
                    // a premium script never replaces what the editor shows, not even the free script viewed before it
                    self.leave_view();
                }
                self.start_xp_tracking(Some(target.clone()));
                let interval = Duration::from_secs(prepared.session.heartbeat_seconds.clamp(10, 120));
                self.store_run = Some(StoreRun { session_id, title: title.to_owned(), beat: Beat::new(std::time::Instant::now(), interval), stopping: false });
                self.run_was_local = false;
                self.tab = Tab::Running;
                self.sidebar_open = false;
            }
            Err(error) => {
                self.script_error(format!("Couldn't start {title}: {error}"));
                self.api_job(ctx, move |api| Event::SessionEnded(api.end_session(&session_id)));
            }
        }
    }

    fn run_script(&mut self, ctx: &egui::Context, target: Option<&Target>) {
        let name = self.script_file.name();
        let path = Some(std::path::Path::new(&self.script_file.path)).filter(|p| p.is_file());
        // Developing: `require("../lib/...")` reads from your own lib folder when one is set.
        let lib = Some(std::path::PathBuf::from(self.settings.lib_dir.trim())).filter(|l| path.is_some() && !l.as_os_str().is_empty());
        if let Some(lib) = lib.as_ref().filter(|l| !l.is_dir()) {
            self.script_error(format!("Couldn't start the script: the lib folder {} doesn't exist. Fix it in the settings.", lib.display()));
            return;
        }
        match Script::start(ctx, &self.script_file.text, path, lib.as_deref(), target, &self.settings) {
            Ok(script) => {
                self.script_info(format!("Running {name}…"));
                self.script = Some(script);
                self.run_was_local = true;
                self.tab = Tab::Running;
                self.sidebar_open = false;
                self.start_xp_tracking(target.cloned());
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

        // A store script is read-only here and can't be run from the editor, which would skip its billing.
        let viewing = self.viewing.is_some();
        if let Some(label) = self.viewing.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Viewing {label}, read-only.")).strong());
                if ui.add_enabled(!running, egui::Button::new("Back to my script")).clicked() {
                    self.leave_view();
                }
            });
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!running, egui::Button::new("Open…")).clicked() {
                self.open_script();
            }
            if ui.add_enabled(!running && !viewing && !self.script_file.path.is_empty(), egui::Button::new("Save")).clicked() {
                self.save_script();
            }
            if ui.add_enabled(!running, egui::Button::new("Save as…")).on_hover_text("Saves a copy of what's in the editor.").clicked() {
                self.save_script_as();
            }
            ui.add_space(16.0);
            let can_run = !running && !viewing && !self.script_file.text.trim().is_empty();
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
                .on_hover_text(if self.store_run.is_some() {
                    "Suspends the script's process. A store script is still billed while paused: stop it to stop billing."
                } else {
                    "Suspends the script's process, so it takes effect immediately, even in a tight loop."
                })
                .clicked()
            {
                self.toggle_pause();
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
        let (row_height, digit_width) = ui.fonts_mut(|fonts| (fonts.row_height(&font), fonts.glyph_width(&font, '0')));
        // only scans the text when it changed since the last frame
        self.fold.sync(&self.script_file.path, &self.script_file.text);
        let lines = self.fold.line_count();
        let arrow_width = row_height;
        let gutter_width = digit_width * lines.to_string().len() as f32 + 3.0 * margin.leftf() + arrow_width;
        let gutter_color = ui.visuals().weak_text_color();
        if self.find.open
            && self.find.jump
            && let Some(&at) = matches.get(self.find.current)
        {
            self.fold.reveal(self.script_file.text[..at].matches('\n').count());
        }
        let view = self.fold.view(&self.script_file.text);
        // wide enough for the longest line (a tab counts as four columns) so the area scrolls sideways; the text fills what's left
        let text_width = (self.fold.longest_line() as f32 * digit_width + margin.sum().x + 8.0)
            .max(size.x - gutter_width - ui.spacing().item_spacing.x);
        let gutter_height = size.y.max(view.visible.len() as f32 * row_height + margin.sum().y);
        let highlighter = &mut self.highlighter;
        let hidden = &view.hidden;
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap_width: f32| {
            // never wrap: the gutter counts one row per line, and the area scrolls sideways instead
            let mut job = highlighter.layout(ui, text.as_str(), hidden);
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let mut select_line = None;
                    let (gutter, response) =
                        ui.allocate_exact_size(egui::vec2(gutter_width, gutter_height), egui::Sense::click());
                    ui.painter().rect_filled(gutter, 0.0, ui.visuals().faint_bg_color);
                    let row_top = |row: usize| gutter.top() + margin.topf() + row as f32 * row_height;
                    let first = ((ui.clip_rect().top() - gutter.top() - margin.topf()) / row_height).floor().max(0.0) as usize;
                    let last = ((ui.clip_rect().bottom() - gutter.top() - margin.topf()) / row_height).ceil().max(0.0) as usize;
                    let arrow_center = |row: usize| egui::pos2(gutter.right() - arrow_width / 2.0, row_top(row) + row_height / 2.0);
                    // the row under the pointer: its number lights up, and so does the line in the text
                    let hover_row = response
                        .hover_pos()
                        .map(|pos| ((pos.y - gutter.top() - margin.topf()) / row_height).floor())
                        .filter(|&row| row >= 0.0 && (row as usize) < view.visible.len())
                        .map(|row| row as usize);
                    let hover_fill = ui.visuals().widgets.hovered.bg_fill;
                    if let Some(row) = hover_row {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        let rect = egui::Rect::from_min_size(egui::pos2(gutter.left(), row_top(row)), egui::vec2(gutter_width, row_height));
                        ui.painter().rect_filled(rect, 0.0, hover_fill);
                    }
                    for row in first..last.min(view.visible.len()) {
                        let line = view.visible[row];
                        let number_color = if hover_row == Some(row) { ui.visuals().strong_text_color() } else { gutter_color };
                        ui.painter().text(
                            egui::pos2(gutter.right() - arrow_width - 2.0 * margin.leftf(), row_top(row)),
                            egui::Align2::RIGHT_TOP,
                            line + 1,
                            font.clone(),
                            number_color,
                        );
                        if self.fold.is_foldable(line) {
                            paint_arrow(ui.painter(), arrow_center(row), row_height * 0.3, self.fold.is_folded(line), gutter_color);
                        }
                    }
                    if response.clicked()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let row = ((pos.y - gutter.top() - margin.topf()) / row_height).floor();
                        if row >= 0.0
                            && let Some(&line) = view.visible.get(row as usize)
                        {
                            // the arrow folds; the number selects the line's text
                            if pos.x >= gutter.right() - arrow_width - margin.leftf() && self.fold.is_foldable(line) {
                                self.fold.toggle(line);
                            } else {
                                select_line = Some(line);
                            }
                        }
                    }
                    // read-only: a str is a TextBuffer that refuses edits but still lets the text be selected and copied
                    let frozen = viewing.then(|| self.script_file.text.clone());
                    let mut frozen_ref = frozen.as_deref().unwrap_or_default();
                    let buffer: &mut dyn egui::TextBuffer = if frozen.is_some() { &mut frozen_ref } else { &mut self.script_file.text };
                    let mut output = egui::TextEdit::multiline(buffer)
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
                        ui.painter().rect_filled(rect, 0.0, hover_fill.gamma_multiply(0.4));
                    }
                    if let Some(line) = select_line {
                        let (start, end) = line_char_range(&self.script_file.text, line);
                        let range = egui::text::CCursorRange::two(egui::text::CCursor::new(start), egui::text::CCursor::new(end));
                        output.state.cursor.set_char_range(Some(range));
                        output.state.clone().store(ui.ctx(), output.response.id);
                        output.response.request_focus();
                    }
                    if let Some(range) = output.state.cursor.char_range() {
                        self.fold.follow_cursor(&self.script_file.text, range.primary.index.0);
                    }
                    // the rows on screen, with one to spare either side
                    let on_screen = first.saturating_sub(1)..last + 1;
                    if self.find.open {
                        paint_matches(ui, &output, &self.script_file.text, &matches, &mut self.find, &view.visible, on_screen.clone());
                    }
                    let mut walk = TextWalk::new(&self.script_file.text);
                    for &(line, header_end) in &view.headers {
                        if !view.visible.binary_search(&line).is_ok_and(|row| on_screen.contains(&row)) {
                            continue;
                        }
                        let at = output.galley.pos_from_cursor(egui::text::CCursor::new(walk.advance(header_end).0));
                        let badge = egui::Rect::from_min_size(
                            egui::pos2(output.galley_pos.x + at.max.x + 6.0, output.galley_pos.y + at.min.y + 1.0),
                            egui::vec2(2.0 * digit_width + 8.0, row_height - 2.0),
                        );
                        let click = ui.interact(badge, ui.id().with(("fold_badge", line)), egui::Sense::click());
                        let fill = if click.hovered() { ui.visuals().widgets.hovered.bg_fill } else { ui.visuals().widgets.inactive.bg_fill };
                        ui.painter().rect_filled(badge, 3.0, fill);
                        ui.painter().text(badge.center(), egui::Align2::CENTER_CENTER, "...", font.clone(), gutter_color);
                        if click.on_hover_text("Click to unfold").clicked() {
                            self.fold.toggle(line);
                        }
                    }
                });
            });
    }

    /// The find bar; returns the matches in the text as it stands after the bar's own edits.
    fn find_ui(&mut self, ui: &mut egui::Ui) -> Vec<usize> {
        let matches = find_matches(&self.script_file.text, &self.find.query, self.find.case_sensitive);
        let count = matches.len();
        self.find.current = self.find.current.min(count.saturating_sub(1));

        // `Some(true)` steps back, `Some(false)` steps forward.
        let mut step = None;
        let mut replace_current = false;
        let mut replace_all = false;
        let mut query_changed = false;

        ui.horizontal(|ui| {
            let query = ui.add(egui::TextEdit::singleline(&mut self.find.query).hint_text("Find").desired_width(200.0));
            if std::mem::take(&mut self.find.focus) {
                query.request_focus();
            }
            if query.changed() {
                self.find.current = 0;
                self.find.jump = true;
                query_changed = true;
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
                query_changed = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").on_hover_text("Escape").clicked() {
                    self.find.open = false;
                }
            });
        });

        if self.find.replace && self.viewing.is_none() {
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
            find_matches(&self.script_file.text, &self.find.query, self.find.case_sensitive)
        } else if let (true, Some(&at)) = (replace_current, matches.get(self.find.current)) {
            let replacement = self.find.replacement.clone();
            let text = &mut self.script_file.text;
            text.replace_range(at..at + self.find.query.len(), &replacement);
            // Land on the first match after the replacement, so repeated clicks walk through the rest.
            let after = at + replacement.len();
            let next = find_matches(text, &self.find.query, self.find.case_sensitive);
            self.find.current = next.iter().position(|&start| start >= after).unwrap_or(0);
            self.find.jump = true;
            next
        } else if query_changed {
            find_matches(&self.script_file.text, &self.find.query, self.find.case_sensitive)
        } else {
            matches
        }
    }

    fn terminal_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.terminal.get_or_insert_with(|| crate::terminal::Terminal::new(&ctx)).ui(ui);
    }

    fn development_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in DevTab::ALL {
                ui.selectable_value(&mut self.dev_tab, tab, label);
            }
        });
        ui.separator();
        match self.dev_tab {
            DevTab::Tools => self.dev_tools_ui(ui),
            DevTab::Inspectors => self.inspectors_ui(ui),
        }
    }

    fn widget_inspector_showing(&self) -> bool {
        self.tab == Tab::Development && self.dev_tab == DevTab::Inspectors && self.inspector_tab == InspectorTab::Widget
    }

    fn inspectors_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in InspectorTab::ALL {
                ui.selectable_value(&mut self.inspector_tab, tab, label);
            }
        });
        ui.separator();
        match self.inspector_tab {
            InspectorTab::Widget => self.widgets_ui(ui),
            InspectorTab::Var => self.vars_ui(ui),
            InspectorTab::Script => self.script_log_ui(ui),
            InspectorTab::Inventory => self.inventory_ui(ui),
            InspectorTab::Packet => self.packets_ui(ui),
        }
    }

    fn dev_tools_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Game state").default_open(true).show(ui, |ui| self.state_ui(ui));
        ui.separator();
        // Drop columns as the window narrows, down to a single vertical stack.
        let gap = 24.0;
        let font = egui::TextStyle::Body.resolve(ui.style());
        let widest = DEV_TOGGLES
            .iter()
            .map(|label| ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), egui::Color32::WHITE).size().x)
            .fold(0.0_f32, f32::max)
            + ui.spacing().icon_width
            + ui.spacing().icon_spacing;
        let columns = (((ui.available_width() + gap) / (widest + gap)).floor() as usize).clamp(1, DEV_COLUMNS);
        egui::Grid::new(("development_toggles", columns))
            .num_columns(columns)
            .spacing([gap, 6.0])
            .show(ui, |ui| {
                for (index, (label, on)) in DEV_TOGGLES
                    .iter()
                    .zip(self.dev_toggles.iter_mut())
                    .enumerate()
                {
                    ui.add_enabled(DEV_LAYERS[index].is_some(), egui::Checkbox::new(on, *label))
                        .on_disabled_hover_text("Not ported yet.");
                    if (index + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    /// How long the script has run: it keeps counting while the script does and stops where the script ended.
    fn run_time(&self) -> Duration {
        let Some(started) = self.run_started else { return Duration::ZERO };
        self.run_ended.unwrap_or_else(std::time::Instant::now).saturating_duration_since(started)
    }

    fn xp_tracker_ui(&self, ui: &mut egui::Ui) {
        use crate::{counters::number, xp::{duration, thousands}};
        if self.run_started.is_none() || (self.xp.is_empty() && self.counters.is_empty()) {
            ui.weak("Waiting for the first read of the client's skills.");
            return;
        }
        let elapsed = self.run_time();
        ui.label(format!("{} · {} xp", duration(elapsed), thousands(self.xp.total())));
        let rows = self.xp.rows();
        egui::ScrollArea::both().min_scrolled_height(0.0).id_salt("xp_rows").auto_shrink([false, false]).show(ui, |ui| {
            if rows.is_empty() {
                ui.weak("No experience gained yet.");
            } else {
                egui::Grid::new("xp_grid").num_columns(5).spacing([12.0, 2.0]).show(ui, |ui| {
                    for head in ["Skill", "Gained", "XP/hr", "Level", "To level"] {
                        ui.weak(head);
                    }
                    ui.end_row();
                    for row in &rows {
                        ui.label(&row.name);
                        ui.label(thousands(row.gained));
                        ui.label(row.per_hour.map_or("-".into(), thousands));
                        ui.label(row.level.to_string());
                        ui.label(row.to_level.map_or("-".into(), duration));
                        ui.end_row();
                    }
                });
            }
            let counters = self.counters.rows();
            if !counters.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                egui::Grid::new("counters_grid").num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
                    for head in ["", "Total", "Per hour", ""] {
                        ui.weak(head);
                    }
                    ui.end_row();
                    for row in &counters {
                        ui.label(&row.label);
                        ui.label(number(row.total));
                        ui.label(row.per_hour.map_or("-".into(), number));
                        ui.label(row.ratio.as_deref().unwrap_or(""));
                        ui.end_row();
                    }
                });
            }
        });
    }

    fn breaks_ui(&self, ui: &mut egui::Ui) {
        use crate::{breaks::clock, xp::duration};
        let running = self.script.as_ref().is_some_and(Script::is_running);
        // counts down every second while the script runs, and stands at the last report once it has ended
        let at = if running { std::time::Instant::now() } else { self.breaks.received().unwrap_or_else(std::time::Instant::now) };
        let Some(view) = self.breaks.view(at) else {
            ui.weak("The script hasn't reported any breaks.");
            return;
        };
        if running {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        if let Some(active) = &view.active {
            ui.label(egui::RichText::new(format!("On a {} · {} left", active.kind, duration(active.remaining))).strong());
            ui.add_space(4.0);
        }
        egui::ScrollArea::both().min_scrolled_height(0.0).id_salt("breaks_rows").auto_shrink([false, false]).show(ui, |ui| {
            if !view.breaks.is_empty() {
                ui.label(egui::RichText::new("Breaks").strong());
                egui::Grid::new("breaks_grid").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.weak("In");
                    ui.weak("Length");
                    ui.end_row();
                    for row in &view.breaks {
                        ui.label(duration(row.remaining));
                        ui.label(format!("~{}", duration(row.length)));
                        ui.end_row();
                    }
                });
            }
            if !view.sleeps.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Sleeps").strong());
                egui::Grid::new("sleeps_grid").num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
                    ui.weak("At");
                    ui.weak("In");
                    ui.weak("Length");
                    ui.end_row();
                    for row in &view.sleeps {
                        ui.label(row.at.map_or("-".into(), clock));
                        ui.label(duration(row.remaining));
                        ui.label(format!("~{}", duration(row.length)));
                        ui.end_row();
                    }
                });
            }
            if view.breaks.is_empty() && view.sleeps.is_empty() {
                ui.weak("Nothing is scheduled.");
            }
        });
    }

    /// The tag filter and the Copy all / Clear buttons above the output (the filter only for the script's); meant to run inside a horizontal row.
    fn log_controls_ui(&mut self, ui: &mut egui::Ui) {
        if self.log_tab == LogTab::Script {
            let log = &mut self.script_log;
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
        // the buttons sit at the right edge; when the row is too narrow they wrap onto the next line after the filter
        let narrow = ui.available_width() < 150.0;
        let layout = if narrow { egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true) } else { egui::Layout::right_to_left(egui::Align::Center) };
        ui.with_layout(layout, |ui| {
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
    }

    fn log_panel_ui(&mut self, ui: &mut egui::Ui) {
        ui.shrink_clip_rect(ui.available_rect_before_wrap());
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.horizontal_wrapped(|ui| {
            crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Loader, "Loader");
            crate::theme::tab_value(ui, &mut self.log_tab, LogTab::Script, "Script");
        });
        // the line under the tabs, across the whole width, so they read as tabs rather than buttons
        let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(line, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
        ui.spacing_mut().item_spacing.y = 4.0;
        if self.log_tab == LogTab::Script {
            // the output keeps the left half and the XP tracker the right; when too narrow the tracker goes under the output
            let stacked = ui.available_width() < SIDE_BY_SIDE_MIN_WIDTH;
            let panel = if stacked {
                egui::Panel::bottom("script_side").resizable(false).exact_size(ui.available_height() * 0.4)
            } else {
                egui::Panel::right("script_side").resizable(false).exact_size(ui.available_width() * 0.5)
            };
            panel.show(ui, |ui| {
                ui.shrink_clip_rect(ui.max_rect());
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal_wrapped(|ui| {
                    crate::theme::tab_value(ui, &mut self.tools_tab, ToolsTab::Xp, "XP Tracker");
                    crate::theme::tab_value(ui, &mut self.tools_tab, ToolsTab::Breaks, "Breaks");
                });
                let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().rect_filled(line, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
                ui.spacing_mut().item_spacing.y = 4.0;
                match self.tools_tab {
                    ToolsTab::Xp => self.xp_tracker_ui(ui),
                    ToolsTab::Breaks => self.breaks_ui(ui),
                }
            });
        }
        // what's left once the side panel took its share; the output is cropped to it rather than drawn over the panel
        ui.shrink_clip_rect(ui.available_rect_before_wrap());
        // what is left after the tracker: anything that doesn't fit there is cut off rather than drawn over it
        ui.shrink_clip_rect(ui.available_rect_before_wrap());
        ui.horizontal_wrapped(|ui| self.log_controls_ui(ui));
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
        // the job is only rebuilt when the shown lines change; egui's own cache then skips the layout too
        let key = (self.log_tab, log.revision, font.clone(), normal, error);
        if self.log_job.as_ref().is_none_or(|(cached, _)| *cached != key) {
            self.log_job = Some((key, log.layout_job(font, normal, error)));
        }
        let cached_job = self.log_job.as_ref().map(|(_, job)| job);
        let mut layouter = |ui: &egui::Ui, _: &dyn egui::TextBuffer, wrap_width: f32| {
            let mut job = cached_job.cloned().unwrap_or_default();
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        };
        egui::ScrollArea::vertical()
            .min_scrolled_height(0.0)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let mut read_only = text;
                let output = egui::TextEdit::multiline(&mut read_only)
                    .font(egui::TextStyle::Body)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .layouter(&mut layouter)
                    .show(ui);

                let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|pos| output.response.rect.contains(*pos)) else {
                    return;
                };
                // Rows of one galley map to text lines (a row that ends with a newline closes one); an entry can span several.
                let entry_of_line: Vec<usize> = log
                    .view
                    .iter()
                    .enumerate()
                    .flat_map(|(entry, &index)| std::iter::repeat_n(entry, log.lines[index].text.matches('\n').count() + 1))
                    .collect();
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
                    egui::pos2(output.response.rect.left(), span.top()),
                    egui::pos2(output.response.rect.right(), span.bottom()),
                );
                ui.painter().rect_filled(band, 0.0, ui.visuals().text_color().gamma_multiply(0.08));
                let line_rect = egui::Rect::from_min_max(
                    egui::pos2(output.response.rect.left(), rect.top()),
                    egui::pos2(output.response.rect.right(), rect.bottom()),
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

/// A fold arrow: pointing right when folded, down when open.
fn paint_arrow(painter: &egui::Painter, center: egui::Pos2, size: f32, folded: bool, color: egui::Color32) {
    let points = if folded {
        vec![center + egui::vec2(-size * 0.6, -size), center + egui::vec2(-size * 0.6, size), center + egui::vec2(size * 0.9, 0.0)]
    } else {
        vec![center + egui::vec2(-size, -size * 0.6), center + egui::vec2(size, -size * 0.6), center + egui::vec2(0.0, size * 0.9)]
    };
    painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
}

/// The char range of `line` (0-based) without its line break.
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

/// Walks forward through a text, turning increasing byte offsets into char indices and line numbers in one pass.
struct TextWalk<'a> {
    text: &'a str,
    byte: usize,
    char: usize,
    line: usize,
}

impl<'a> TextWalk<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, byte: 0, char: 0, line: 0 }
    }

    /// The char index and line of byte `to`. An offset behind the walk, or not in the text (it was edited this frame), stays where the walk is.
    fn advance(&mut self, to: usize) -> (usize, usize) {
        if let Some(part) = self.text.get(self.byte..to) {
            self.char += part.chars().count();
            self.line += part.bytes().filter(|&b| b == b'\n').count();
            self.byte = to;
        }
        (self.char, self.line)
    }
}

/// Paints the find matches on the editor rows `on_screen` (indices into `visible`), and scrolls to the current one when asked.
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
        // laying out a match's place costs a pass over the galley's rows, so only the ones that can be seen get one
        let shown = visible.binary_search(&line).is_ok_and(|row| on_screen.contains(&row));
        if !shown && !(current && find.jump) {
            continue;
        }
        let Some(query_chars) = text.get(at..at + query_len).map(|q| q.chars().count()) else { continue };
        let start = output.galley.pos_from_cursor(egui::text::CCursor::new(first_char));
        let end = output.galley.pos_from_cursor(egui::text::CCursor::new(first_char + query_chars));
        // A match split across wrapped rows is skipped rather than drawn wrong.
        if (start.top() - end.top()).abs() > 1.0 {
            continue;
        }
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
        self.drive_session(&ctx);
        self.drive_xp(&ctx);
        self.form_ui();
        self.reload_prompt_ui(&ctx);
        self.tiles_ui(&ctx);
        self.poll_packets(&ctx);
        self.poll_vars(&ctx);
        self.poll_script_log(&ctx);
        self.poll_inventory(&ctx);
        self.poll_widget_mouse(&ctx);

        let narrow = ctx.content_rect().width() < SIDEBAR_MIN_WINDOW;
        egui::Panel::top("settings").show(ui, |ui| {
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let logo = egui::Image::new(egui::include_image!("../assets/volcren.svg")).fit_to_exact_size(egui::vec2(32.0, 32.0));
                    ui.add(logo);
                    ui.label(egui::RichText::new("VOLCREN").font(egui::FontId::new(22.0, crate::theme::bold_family())));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.account_ui(ui));
                },
            );
            ui.add_space(6.0);
        });
        self.update_banner_ui(&ctx, ui);
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(140.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                self.log_panel_ui(ui);
            });
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
            // Below the breakpoint the sidebar is a sliding drawer with a skinny handle that stays visible when closed.
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
            // Closed by a click outside, Esc or the handle; the drawer sits above the backdrop so clicks on it aren't taken.
            if slide > 0.0 {
                let backdrop = egui::Area::new("sidebar_backdrop".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(area.min)
                    .show(&ctx, |ui| {
                        let (_, response) = ui.allocate_exact_size(area.size(), egui::Sense::click());
                        let shade = egui::Color32::from_black_alpha((130.0 * slide) as u8);
                        ui.painter().rect_filled(area, egui::CornerRadius::ZERO, shade);
                        response
                    })
                    .inner;
                if backdrop.clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.sidebar_open = false;
                }
            }
            egui::Area::new("sidebar_drawer".into())
                .order(egui::Order::Tooltip)
                // Closed, it sits left of the window so only the handle shows; areas are otherwise kept on screen.
                .constrain(false)
                .fixed_pos(egui::pos2(area.min.x - width * (1.0 - slide), area.min.y))
                .show(&ctx, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    ui.horizontal_top(|ui| {
                        crate::theme::sidebar_frame().show(ui, |ui| {
                            ui.set_width(width - f32::from(crate::theme::SIDEBAR_INSET));
                            ui.set_min_height(area.height() - 12.0);
                            // The frame inherits the row's horizontal layout; the page tree stacks top to bottom.
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
        egui::CentralPanel::default_margins().show(ui, |ui| {
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
            // the script's page goes away with the script
            if self.tab == Tab::Running && self.running_title().is_none() {
                self.tab = if self.run_was_local { Tab::Scripts } else { Tab::Store };
            }
            if self.tab != self.followed_tab {
                self.followed_tab = self.tab;
                self.log_tab = if matches!(self.tab, Tab::Scripts | Tab::Store | Tab::Running) { LogTab::Script } else { LogTab::Loader };
            }
            match self.tab {
                Tab::Clients => self.clients_ui(ui),
                Tab::Store => self.store_ui(ui),
                Tab::Running => self.running_ui(ui),
                Tab::Scripts => self.scripts_ui(ui),
                Tab::Development => self.development_ui(ui),
                Tab::Terminal => self.terminal_ui(ui),
                Tab::Accounts => self.accounts_ui(ui),
                Tab::Settings => self.settings_ui(ui),
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
        eframe::set_value(storage, SCRIPT_KEY, self.parked.as_ref().unwrap_or(&self.script_file));
        eframe::set_value(storage, FORMS_KEY, &self.remembered_forms);
        eframe::set_value(storage, CURSOR_KEY, &self.cursor);
        eframe::set_value(storage, CONFIRMED_KEY, &self.confirmed);
        eframe::set_value(storage, HTTP_WHITELIST_KEY, &self.http_whitelist.user());
    }

    fn on_exit(&mut self) {
        if let Some(tiles) = self.tiles.take() {
            tiles.finish();
        }
        if let Some(terminal) = &mut self.terminal {
            terminal.close();
        }
        if let Some(script) = &mut self.script {
            script.stop();
        }
        // Closing must not wait on the network. If this doesn't land, the server ends the session within 5 minutes.
        if let Some(run) = self.store_run.take() {
            let (done, wait) = channel();
            let api = self.api.clone();
            thread::spawn(move || {
                let _ = api.end_session(&run.session_id);
                let _ = done.send(());
            });
            let _ = wait.recv_timeout(Duration::from_secs(3));
        }
        backend::shutdown();
    }
}

// What changed in a container since the snapshot before it, then every slot, as RuneLite's inventory delta panel shows them.
fn inventory_items_ui(ui: &mut egui::Ui, inspector: &crate::inventory::Inspector, snapshot: &crate::inventory::Snapshot, before: Option<&crate::inventory::Snapshot>) {
    let line = |ui: &mut egui::Ui, id: i32, quantity: i32, color: egui::Color32| {
        ui.colored_label(color, format!("{} x{quantity}  (id {id})", inspector.item_name(id)));
    };
    if let Some(before) = before {
        let (added, removed) = crate::inventory::compare(&before.items, &snapshot.items, |id| inspector.is_stackable(id));
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
    for item in &snapshot.items {
        if item.id < 0 {
            ui.weak(format!("{:>3}  EMPTY", item.slot));
        } else {
            ui.label(format!("{:>3}  {} x{}  (id {})", item.slot, inspector.item_name(item.id), item.quantity, item.id));
        }
    }
}

// One node of the widget tree and, when it is open, its children. Hidden widgets are left out when the inspector hides them.
fn widget_node_ui(ui: &mut egui::Ui, inspector: &crate::widgets::Inspector, index: usize, open: &std::collections::HashSet<usize>, reveal: bool, picked: &mut Option<(i32, i32)>) {
    let node = &inspector.built().nodes[index];
    if inspector.hide_hidden && node.widget.hidden {
        return;
    }
    let key = (node.widget.id, node.widget.index);
    let label = format!("{} {}", node.kind.letter(), inspector.identifier(&node.widget));
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
    let mut header = egui::CollapsingHeader::new(egui::RichText::new(label).color(if selected { ui.visuals().selection.stroke.color } else { ui.visuals().text_color() }))
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
        shown.header_response.scroll_to_me(Some(egui::Align::Center));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_the_newest_lines_and_its_filter() {
        let mut log = Log::default();
        for i in 0..MAX_LOG_LINES + MAX_LOG_LINES / 4 + 1 {
            log.push(false, if i % 2 == 0 { format!("[even] {i}") } else { format!("{i}") });
        }
        assert_eq!(log.lines.len(), MAX_LOG_LINES);
        assert_eq!(log.lines.last().unwrap().text, format!("[even] {}", MAX_LOG_LINES + MAX_LOG_LINES / 4));
        assert_eq!(log.text.lines().count(), MAX_LOG_LINES);
        log.set_filter(Some(0));
        assert_eq!(log.view.len(), MAX_LOG_LINES / 2);
        assert!(log.text.lines().all(|line| line.starts_with("[even]")));
        let revision = log.revision;
        log.clear();
        assert!(log.revision > revision);
    }

    #[test]
    fn log_sections_cover_runs_of_one_color() {
        let mut log = Log::default();
        for (error, text) in [(false, "a"), (false, "bb"), (true, "c\nd"), (false, "e")] {
            log.push(error, text.into());
        }
        let (normal, error) = (egui::Color32::WHITE, egui::Color32::RED);
        let job = log.layout_job(egui::FontId::monospace(13.0), normal, error);
        let sections: Vec<_> = job.sections.iter().map(|s| (&job.text[s.byte_range.start.0..s.byte_range.end.0], s.format.color)).collect();
        assert_eq!(sections, [("a\nbb\n", normal), ("c\nd\n", error), ("e", normal)]);
    }

    #[test]
    fn the_text_walk_counts_chars_and_lines() {
        let text = "é\nab\nc";
        let mut walk = TextWalk::new(text);
        assert_eq!(walk.advance(3), (2, 1));
        assert_eq!(walk.advance(7), (6, 2));
        // behind the walk, or past the end of a text edited this frame: it stays put
        assert_eq!(walk.advance(1), (6, 2));
        assert_eq!(walk.advance(99), (6, 2));
    }
}
