use alloc::{format, string::String};
use core::{
    marker::PhantomData,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{fence, AtomicU32, Ordering},
};

use crate::PROTOCOL_VERSION;

pub const STATE_MEMORY_PREFIX: &str = "AGENT_STATE_";

pub fn state_memory_name(pid: u32) -> String {
    format!("{STATE_MEMORY_PREFIX}{pid}")
}

pub mod part {
    pub const GAME_STATE: u64 = 1 << 0;
    pub const TICK_COUNT: u64 = 1 << 1;
    pub const PLAYER: u64 = 1 << 2;
    pub const SKILLS: u64 = 1 << 3;
    pub const VARPS: u64 = 1 << 4;
    pub const LOGIN: u64 = 1 << 5;
    pub const INTERFACES: u64 = 1 << 6;
    pub const WORLD: u64 = 1 << 7;
    pub const WORLD_SELECT: u64 = 1 << 8;
    pub const CAMERA: u64 = 1 << 9;
    pub const MINIMAP: u64 = 1 << 10;
    pub const PROJECTION: u64 = 1 << 11;
    pub const DESTINATION: u64 = 1 << 12;
    pub const GROUND_ITEMS: u64 = 1 << 13;
    pub const CHAT: u64 = 1 << 14;
    pub const COLLISION: u64 = 1 << 15;
    pub const SCENE_LOADER: u64 = 1 << 16;
    pub const PROJECTILES: u64 = 1 << 17;
    pub const SPOT_ANIMS: u64 = 1 << 18;
}

pub mod varp {
    pub const ATTACK_STYLE: usize = 43;
    pub const QUEST_POINTS: usize = 101;
    pub const POISON: usize = 102;
    pub const RUN: usize = 173;
    pub const SPECIAL_ATTACK: usize = 300;
    pub const SPECIAL_ATTACK_ENABLED: usize = 301;
}

pub const SKILL_NAMES: [&str; 24] = [
    "Attack",
    "Defence",
    "Strength",
    "Hitpoints",
    "Ranged",
    "Prayer",
    "Magic",
    "Cooking",
    "Woodcutting",
    "Fletching",
    "Fishing",
    "Firemaking",
    "Crafting",
    "Smithing",
    "Mining",
    "Herblore",
    "Agility",
    "Thieving",
    "Slayer",
    "Farming",
    "Runecraft",
    "Hunter",
    "Construction",
    "Sailing",
];

pub mod game_state {
    pub const STARTING: i32 = 0;
    pub const LOGIN_SCREEN: i32 = 10;
    pub const LOGIN_SCREEN_AUTHENTICATOR: i32 = 11;
    pub const LOGGING_IN: i32 = 20;
    pub const LOADING: i32 = 25;
    pub const LOGGED_IN: i32 = 30;
    pub const CONNECTION_LOST: i32 = 40;
    pub const HOPPING: i32 = 45;

    pub fn name(state: i32) -> Option<&'static str> {
        Some(match state {
            STARTING => "starting",
            LOGIN_SCREEN => "login screen",
            LOGIN_SCREEN_AUTHENTICATOR => "authenticator",
            LOGGING_IN => "logging in",
            LOADING => "loading",
            LOGGED_IN => "logged in",
            CONNECTION_LOST => "connection lost",
            HOPPING => "hopping",
            _ => return None,
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlayerLayout {
    pub world_views: u32,
    pub world_view_id: u32,
    pub player_index: u32,
    pub players: u32,
    pub base_x: u32,
    pub local_x: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkillsLayout {
    pub levels: u32,
    pub real_levels: u32,
    pub xp: u32,
    pub count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoginLayout {
    pub screen: u32,
    pub panel_x: u32,
    pub box_x: u32,
    pub center_x: u32,
    pub y: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InterfaceLayout {
    pub module_base: u64,
    pub client: u32,
    pub root: u32,
    pub manager: u32,
    pub groups: u32,
    pub sub_interfaces: u32,
    pub canvas: u32,
    pub ui_width: u32,
    pub ui_scale: u32,
    pub id: u32,
    pub kind: u32,
    pub parent: u32,
    pub x: u32,
    pub width: u32,
    pub scroll_x: u32,
    pub sprite: u32,
    pub text: u32,
    pub hidden: u32,
    pub text_color: u32,
    pub scroll_width: u32,
    pub index: u32,
    pub children: u32,
    pub reserved: u32,
}

pub mod interfaces {
    pub const GROUP_SIZE: usize = 0x18;
    pub const ARRAY_COUNT: usize = 0x8;
    pub const ARRAY_DATA: usize = 0x10;
    pub const ENTRY_SIZE: usize = 0x10;
    pub const ENTRY_COMPONENT: usize = 0x8;
    pub const CHILDREN_DATA: usize = 0x8;
    pub const TABLE_BUCKETS: usize = 0x8;
    pub const TABLE_BUCKET_COUNT: usize = 0x10;
    pub const NODE_KEY: usize = 0x0;
    pub const NODE_GROUP: usize = 0x4;
    pub const NODE_NEXT: usize = 0x10;
    pub const ROOT_LOBBY: i32 = 165;
    pub const ROOT_FIXED: i32 = 548;
    pub const ROOT_RESIZABLE: i32 = 161;
    pub const ROOT_RESIZABLE_MODERN: i32 = 164;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldLayout {
    pub module_base: u64,
    pub client: u32,
    pub world_views: u32,
    pub world_view_id: u32,
    pub base_x: u32,
    pub players: u32,
    pub npc_table: u32,
    pub npc_indices: u32,
    pub player_count: u32,
    pub local_x: u32,
    pub npc_name_override: u32,
    pub npc_definition: u32,
    pub player_name: u32,
    pub definition_id: u32,
    pub definition_name: u32,
    pub containers: u32,
    pub container_ids: u32,
    pub container_quantities: u32,
    pub container_next: u32,
    pub varcs: u32,
    pub varc_begin: u32,
    pub varc_entry_size: u32,
    pub varc_value: u32,
    pub varc_kind: u32,
    pub run_energy: u32,
    pub weight: u32,
    // the NPC actor's animation id (-1 when none), `Npc::animation`
    pub npc_animation: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldSelectLayout {
    pub open: u32,
    pub count: u32,
    pub worlds: u32,
    pub offset: u32,
    pub max_offset: u32,
    pub current_world: u32,
    pub background: u32,
    pub background_height: u32,
    pub id: u32,
    pub types: u32,
    pub players: u32,
    pub activity: u32,
    pub location: u32,
    pub hovered: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CameraLayout {
    pub position: u32,
    pub pitch: u32,
    pub yaw: u32,
    pub focal: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinimapLayout {
    pub object: u32,
    pub zoom: u32,
    pub destination: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroundItemsLayout {
    pub items: u32,
    pub level: u32,
    pub node_item: u32,
    pub id: u32,
}

// The chat history: `messages` and `cycle` are module RVAs (uid hash table, client cycle at 50 per second), the rest message fields.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChatLayout {
    pub messages: u32,
    pub cycle: u32,
    pub message_cycle: u32,
    pub kind: u32,
    pub name: u32,
    pub sender: u32,
    pub text: u32,
    pub time_utc: u32,
    pub time_local: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionLayout {
    pub top_world_view: u32,
    pub plane: u32,
    pub scene: u32,
    pub size_x: u32,
    pub size_y: u32,
    pub tiles: u32,
    pub zoom: u32,
    pub view_holder: u32,
    pub view: u32,
    pub view_center: u32,
    pub view_offset_x: u32,
    pub view_offset_y: u32,
    pub trig: u32,
    pub canvas_size: u32,
    pub ui_size: u32,
    pub heights: u32,
    pub settings: u32,
    pub tile_x: u32,
    pub tile_model: u32,
    pub tile_paint: u32,
    pub ground_decoration: u32,
    pub model_vertices: u32,
    pub model_triangles: u32,
    pub decoration_height: u32,
}

// The scene's collision maps (Java's `CollisionMap[]`): a vector at `scene + maps`, `stride` apart, tile (x, y) at `[x + 1][y + 1]`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollisionLayout {
    pub maps: u32,
    pub stride: u32,
}

// The scene loader the client's packet handler keeps: the byte at `loader + instance_flag` is the client's instance flag, 1 or 0.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneLoaderLayout {
    pub instance_flag: u32,
}

// The projectiles in flight: a circular list at `client + list` of nodes holding projectile pointers, plus the projectile field offsets.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectileLayout {
    pub list: u32,
    pub node: u32,
    pub cycle: u32,
    pub source_level: u32,
    pub source_x: u32,
    pub source_y: u32,
    pub source_actor: u32,
    pub start_height: u32,
    pub target_level: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub target_actor: u32,
    pub end_height: u32,
    pub start_cycle: u32,
    pub end_cycle: u32,
    pub slope: u32,
    pub start_pos: u32,
    pub id: u32,
    pub x: u32,
    pub z: u32,
    pub y: u32,
    pub orientation: u32,
    pub moving: u32,
    pub reserved: u32,
}

// The spot animations on tiles: a circular list at `world view + list` of nodes holding object pointers, plus the object field offsets.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpotAnimLayout {
    pub list: u32,
    pub cycle: u32,
    pub start_cycle: u32,
    pub level: u32,
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub id: u32,
}

pub mod collision_map {
    pub const WIDTH: usize = 0x8;
    pub const HEIGHT: usize = 0xc;
    // an `array` of columns, each an inline 0x18 byte `array` of i32 flags
    pub const COLUMNS: usize = 0x10;
    pub const COLUMN_SIZE: usize = 0x18;
    pub const BORDER: usize = 1;
}

pub mod world {
    pub const INVENTORY: i32 = 93;
    pub const EQUIPMENT: i32 = 94;
    pub const BANK: i32 = 95;
    pub const VARC_GAME_TAB: i32 = 171;
}

pub mod array {
    pub const COUNT: usize = 0x8;
    pub const DATA: usize = 0x10;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layout {
    pub found: u64,
    pub module_base: u64,
    pub client: u32,
    pub game_state: u32,
    pub tick_count: u32,
    pub varps: u32,
    pub player: PlayerLayout,
    pub skills: SkillsLayout,
    pub login: LoginLayout,
    pub interfaces: InterfaceLayout,
    pub world: WorldLayout,
    pub world_select: WorldSelectLayout,
    pub camera: CameraLayout,
    pub minimap: MinimapLayout,
    pub projection: ProjectionLayout,
    pub ground_items: GroundItemsLayout,
    pub chat: ChatLayout,
    pub collision: CollisionLayout,
    pub scene_loader: SceneLoaderLayout,
    pub projectiles: ProjectileLayout,
    pub spot_anims: SpotAnimLayout,
}

impl Layout {
    pub fn has(&self, parts: u64) -> bool {
        self.found & parts == parts
    }
}

pub mod name_status {
    pub const OFF: i32 = 0;
    pub const KEPT: i32 = 1;
    pub const WAITING: i32 = 2;
    pub const LONG_NAME: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const NOT_FOUND: i32 = -3;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NameRequest {
    pub enabled: u32,
    pub reserved: u32,
    pub display: [u8; 24],
    pub account: [u8; 24],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NameStatus {
    pub sequence: u32,
    pub status: i32,
    pub real: [u8; 24],
}

pub mod total_xp_status {
    pub const OFF: i32 = 0;
    pub const ON: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TotalXpRequest {
    pub enabled: u32,
    pub reserved: u32,
    pub offset: i64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TotalXpStatus {
    pub sequence: u32,
    pub status: i32,
}

pub mod item_name_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const TOO_MANY: i32 = -3;
}

pub const ITEM_NAME_BATCH: usize = 64;
pub const ITEM_NAME_SIZE: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemNameRequest {
    pub count: u32,
    pub reserved: u32,
    pub ids: [i32; ITEM_NAME_BATCH],
}

impl Default for ItemNameRequest {
    fn default() -> Self {
        Self {
            count: 0,
            reserved: 0,
            ids: [0; ITEM_NAME_BATCH],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameValRequest {
    pub kind: u32,
    pub count: u32,
    pub ids: [i32; ITEM_NAME_BATCH],
}

impl Default for GameValRequest {
    fn default() -> Self {
        Self {
            kind: 0,
            count: 0,
            ids: [0; ITEM_NAME_BATCH],
        }
    }
}

pub mod game_val {
    pub const OBJ: u32 = 0;
    pub const NPC: u32 = 1;
    pub const INV: u32 = 2;
    pub const VARP: u32 = 3;
    pub const VARBIT: u32 = 4;
    pub const LOC: u32 = 6;
    pub const SEQ: u32 = 7;
    pub const SPOT: u32 = 8;
    pub const ROW: u32 = 9;
    pub const TABLE: u32 = 10;
    pub const JINGLE: u32 = 11;
    pub const SPRITE: u32 = 12;
    pub const INTERFACE: u32 = 14;
    pub const VARC: u32 = 15;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamesReply {
    pub sequence: u32,
    pub status: i32,
    pub count: u32,
    pub reserved: u32,
    pub names: [[u8; ITEM_NAME_SIZE]; ITEM_NAME_BATCH],
}

impl Default for NamesReply {
    fn default() -> Self {
        Self {
            sequence: 0,
            status: 0,
            count: 0,
            reserved: 0,
            names: [[0; ITEM_NAME_SIZE]; ITEM_NAME_BATCH],
        }
    }
}

impl NamesReply {
    pub fn name(&self, index: usize) -> Option<&[u8]> {
        let name = self
            .names
            .get(index)
            .filter(|_| index < self.count as usize)?;
        Some(&name[..name.iter().position(|&b| b == 0).unwrap_or(ITEM_NAME_SIZE)])
    }

    pub fn set_name(&mut self, index: usize, name: &[u8]) {
        let slot = &mut self.names[index];
        let len = name.len().min(ITEM_NAME_SIZE);
        slot.fill(0);
        slot[..len].copy_from_slice(&name[..len]);
    }
}

pub mod project_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const TOO_MANY: i32 = -3;
    pub const NOT_IN_GAME: i32 = -4;
}

pub const PROJECT_BATCH: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectRequest {
    pub count: u32,
    pub reserved: u32,
    pub points: [[i32; 3]; PROJECT_BATCH],
}

impl Default for ProjectRequest {
    fn default() -> Self {
        Self {
            count: 0,
            reserved: 0,
            points: [[0; 3]; PROJECT_BATCH],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectReply {
    pub sequence: u32,
    pub status: i32,
    pub count: u32,
    pub reserved: u32,
    pub points: [[f32; 2]; PROJECT_BATCH],
}

impl Default for ProjectReply {
    fn default() -> Self {
        Self {
            sequence: 0,
            status: 0,
            count: 0,
            reserved: 0,
            points: [[0.0; 2]; PROJECT_BATCH],
        }
    }
}

pub mod menu_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
}

// The client's own cap (Java's 500). A crowded tile easily has 80 entries.
pub const MENU_ENTRIES: usize = 500;
pub const MENU_OPTION_SIZE: usize = 40;
// Targets keep colour tags, and a "Use" target naming both items already passes 80 bytes.
pub const MENU_TARGET_SIZE: usize = 256;
// The client lays out at most 4 submenus deep (it refuses paths of 5 rows or more).
pub const MENU_SUBMENUS: usize = 4;
// Java's cap for a submenu (`new Menu(false)`).
pub const MENU_SUBMENU_ENTRIES: usize = 64;

pub mod menu_open {
    pub const CLOSED: u32 = 0;
    pub const OPEN: u32 = 1;
    pub const UNKNOWN: u32 = 2;
}

pub mod menu_kind {
    pub const NONE: u8 = 0;
    pub const TILE: u8 = 1;
    pub const NPC: u8 = 2;
    pub const WIDGET: u8 = 7;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuEntryInfo {
    pub option: [u8; MENU_OPTION_SIZE],
    pub target: [u8; MENU_TARGET_SIZE],
    pub id: i32,
    pub index: i32,
    pub action: i32,
    pub priority: i32,
    pub kind: u8,
    pub category: u8,
    pub cancel: u8,
    /// 1 when the row opens a submenu. Only known for the rows of an open menu.
    pub submenu: u8,
}

impl Default for MenuEntryInfo {
    fn default() -> Self {
        Self {
            option: [0; MENU_OPTION_SIZE],
            target: [0; MENU_TARGET_SIZE],
            id: -1,
            index: -1,
            action: -1,
            priority: 0,
            kind: 0,
            category: 0,
            cancel: 0,
            submenu: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuReply {
    pub sequence: u32,
    pub status: i32,
    pub count: u32,
    /// Whether the client's choose-option menu is open: `menu_open::CLOSED`, `OPEN` or `UNKNOWN`.
    pub open: u32,
    pub entries: [MenuEntryInfo; MENU_ENTRIES],
    pub clicked: MenuEntryInfo,
    pub click_x: i32,
    pub click_y: i32,
    pub click_age: i32,
    pub has_click: u32,
    /// The component waiting for a target: verb `option`, name `target`, component `id`, child `index`, item id `action` and mask `kind`; valid when `selected` is 1.
    pub selection: MenuEntryInfo,
    /// 1 when agent knows where the client keeps the selected component.
    pub has_selection: u32,
    pub selected: u32,
    /// The menu as the client lays it out. Only meaningful when `has_geometry` is 1.
    pub geometry: MenuGeometry,
    pub has_geometry: u32,
    /// The open submenus, the one opened from the menu first; valid when `has_geometry` is 1.
    pub submenu_count: u32,
    pub submenus: [MenuSubmenu; MENU_SUBMENUS],
}

/// An open submenu: the row above that opened it (0 is the top), its rows top to bottom, and its layout.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuSubmenu {
    pub row: u32,
    pub count: u32,
    pub geometry: MenuGeometry,
    pub entries: [MenuEntryInfo; MENU_SUBMENU_ENTRIES],
}

impl Default for MenuSubmenu {
    fn default() -> Self {
        Self {
            row: 0,
            count: 0,
            geometry: MenuGeometry::default(),
            entries: [MenuEntryInfo::default(); MENU_SUBMENU_ENTRIES],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MenuGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub rows_x: i32,
    pub rows_y: i32,
    pub rows_width: i32,
    pub rows_height: i32,
    pub header_height: i32,
    pub row_height: i32,
    pub scroll: i32,
    pub margin: i32,
}

impl Default for MenuReply {
    fn default() -> Self {
        Self {
            sequence: 0,
            status: 0,
            count: 0,
            open: 0,
            entries: [MenuEntryInfo::default(); MENU_ENTRIES],
            clicked: MenuEntryInfo::default(),
            click_x: 0,
            click_y: 0,
            click_age: 0,
            has_click: 0,
            selection: MenuEntryInfo::default(),
            has_selection: 0,
            selected: 0,
            geometry: MenuGeometry::default(),
            has_geometry: 0,
            submenu_count: 0,
            submenus: [MenuSubmenu::default(); MENU_SUBMENUS],
        }
    }
}

pub mod model_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const NOT_IN_GAME: i32 = -4;
    pub const NO_NPC: i32 = -5;
    pub const NO_MODEL: i32 = -6;
}

pub const MODEL_HULL: usize = 128;
pub const MODEL_FACES: usize = 8192;

pub mod model_target {
    pub const NPC: u32 = 0;
    pub const LOC: u32 = 1;
    pub const PLAYER: u32 = 2;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelRequest {
    pub index: i32,
    pub target: u32,
    pub loc_kind: u32,
    pub loc_x: i32,
    pub loc_y: i32,
    pub loc_id: i32,
}

pub mod loc_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const NOT_IN_GAME: i32 = -4;
}

pub const MAX_LOCS: usize = 8192;
pub const MAX_LOC_NAMES: usize = 1024;
pub const LOC_NAME_SIZE: usize = 32;
// `LocInfo::flags`: whether the client lets the loc be clicked (the minimap draws such walls red: doors), and its plane
pub const LOC_CLICKABLE: u8 = 1;
pub const LOC_PLANE_SHIFT: u8 = 1;
pub const LOC_PLANE_MASK: u8 = 3;
pub const NO_NAME: u16 = 0xffff;

/// What a locs request asks for: a plane (0 to 3) or `PLAYER_PLANE` or `ALL_PLANES`, optionally `| CLICKABLE_ONLY`.
pub mod locs_request {
    pub const PLAYER_PLANE: i32 = -1;
    pub const ALL_PLANES: i32 = 4;
    pub const CLICKABLE_ONLY: i32 = 0x100;
    pub const PLANES: i32 = 0xff;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LocInfo {
    pub id: i32,
    pub x: u8,
    pub y: u8,
    pub size_x: u8,
    pub size_y: u8,
    pub kind: u8,
    pub shape: u8,
    pub rotation: u8,
    pub flags: u8,
    pub name: u16,
    pub decoration_height: i16,
}

/// One request slot and one reply buffer for read-only requests the worker answers; the injector holds `worker_mutex_name` while waiting.
pub mod worker_kind {
    pub const NPCS: u32 = 1;
    pub const PLAYERS: u32 = 2;
    pub const GROUND_ITEMS: u32 = 3;
    pub const WIDGET: u32 = 4;
    pub const WIDGET_CHILDREN: u32 = 5;
    pub const SUB_INTERFACES: u32 = 6;
    pub const GAME_STATE: u32 = 7;
    pub const ITEM_CONTAINER: u32 = 8;
    pub const VARC: u32 = 9;
    pub const CHAT: u32 = 10;
    pub const WORLD_SELECT: u32 = 11;
    pub const PROJECTILES: u32 = 12;
    pub const SPOT_ANIMS: u32 = 13;
}

pub mod worker_status {
    pub const OK: i32 = 1;
    /// The agent hasn't found what the request needs, or doesn't know the kind.
    pub const NOT_FOUND: i32 = -1;
    /// The answer doesn't fit the reply buffer: read the memory through the client instead.
    pub const TOO_BIG: i32 = -3;
}

pub const WORKER_DATA_SIZE: usize = 512 * 1024;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkerRequest {
    pub kind: u32,
    pub args: [i32; 4],
    pub reserved: u32,
}

/// `data[..length]` is the answer, encoded by the type the kind names. `sequence` echoes the request's.
#[repr(C)]
pub struct WorkerReply {
    pub sequence: u32,
    pub status: i32,
    pub length: u32,
    pub reserved: u32,
    pub data: [u8; WORKER_DATA_SIZE],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocsReply {
    pub sequence: u32,
    pub status: i32,
    pub count: u32,
    pub name_count: u32,
    pub plane: i32,
    pub base_x: i32,
    pub base_y: i32,
    pub reserved: u32,
    pub locs: [LocInfo; MAX_LOCS],
    pub names: [[u8; LOC_NAME_SIZE]; MAX_LOC_NAMES],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelReply {
    pub sequence: u32,
    pub status: i32,
    pub bounding_box: u32,
    pub orientation: i32,
    pub vertex_count: u32,
    pub corner_count: u32,
    pub hull_count: u32,
    pub face_count: u32,
    pub corners: [[f32; 2]; 8],
    pub hull: [[f32; 2]; MODEL_HULL],
    pub faces: [[[f32; 2]; 3]; MODEL_FACES],
}

impl Default for ModelReply {
    fn default() -> Self {
        Self {
            sequence: 0,
            status: 0,
            bounding_box: 0,
            orientation: 0,
            vertex_count: 0,
            corner_count: 0,
            hull_count: 0,
            face_count: 0,
            corners: [[0.0; 2]; 8],
            hull: [[0.0; 2]; MODEL_HULL],
            faces: [[[0.0; 2]; 3]; MODEL_FACES],
        }
    }
}

pub mod cache_kind {
    pub const FILES: u32 = 0;
    pub const ARCHIVES: u32 = 1;
    pub const ARCHIVE_FILES: u32 = 2;
}

pub mod cache_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const TOO_MANY: i32 = -3;
    pub const NO_INDEX: i32 = -5;
    pub const NO_ARCHIVE: i32 = -6;
}

pub const CACHE_FILE_MISSING: i32 = -1;
pub const CACHE_FILE_NO_ROOM: i32 = -2;

pub const CACHE_BATCH: usize = 256;
pub const CACHE_DATA_SIZE: usize = 4 << 20;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheRequest {
    pub kind: u32,
    pub index: u32,
    pub archive: i32,
    pub count: u32,
    pub files: [[i32; 2]; CACHE_BATCH],
}

impl Default for CacheRequest {
    fn default() -> Self {
        Self {
            kind: 0,
            index: 0,
            archive: 0,
            count: 0,
            files: [[0; 2]; CACHE_BATCH],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheFile {
    pub offset: u32,
    pub length: i32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CacheReply {
    pub sequence: u32,
    pub status: i32,
    pub count: u32,
    pub used: u32,
    pub fingerprint: u64,
    pub files: [CacheFile; CACHE_BATCH],
    pub data: [u8; CACHE_DATA_SIZE],
}

pub fn text(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

#[repr(C)]
pub struct LayoutMemory {
    pub protocol: u32,
    pub sequence: u32,
    pub layout: Layout,
    pub request_sequence: u32,
    pub reserved: u32,
    pub name_request: NameRequest,
    pub status_sequence: u32,
    pub reserved2: u32,
    pub name_status: NameStatus,
    pub xp_request_sequence: u32,
    pub reserved3: u32,
    pub total_xp_request: TotalXpRequest,
    pub xp_status_sequence: u32,
    pub reserved4: u32,
    pub total_xp_status: TotalXpStatus,
    pub item_request_sequence: u32,
    pub reserved5: u32,
    pub item_name_request: ItemNameRequest,
    pub item_status_sequence: u32,
    pub reserved6: u32,
    pub item_name_status: NamesReply,
    pub game_val_request_sequence: u32,
    pub reserved7: u32,
    pub game_val_request: GameValRequest,
    pub game_val_status_sequence: u32,
    pub reserved8: u32,
    pub game_val_status: NamesReply,
    pub project_request_sequence: u32,
    pub reserved9: u32,
    pub project_request: ProjectRequest,
    pub project_reply_sequence: u32,
    pub reserved10: u32,
    pub project_reply: ProjectReply,
    pub menu_request_sequence: u32,
    pub reserved11: u32,
    pub menu_reply_sequence: u32,
    pub reserved12: u32,
    pub menu_reply: MenuReply,
    pub model_request_sequence: u32,
    pub reserved13: u32,
    pub model_request: ModelRequest,
    pub model_reply_sequence: u32,
    pub reserved14: u32,
    pub model_reply: ModelReply,
    pub locs_request_sequence: u32,
    pub locs_request: i32,
    pub locs_reply_sequence: u32,
    pub reserved16: u32,
    pub locs_reply: LocsReply,
    pub cache_request_sequence: u32,
    pub reserved17: u32,
    pub cache_request: CacheRequest,
    pub cache_reply_sequence: u32,
    pub reserved18: u32,
    pub cache_reply: CacheReply,
    pub worker_request_sequence: u32,
    pub reserved19: u32,
    pub worker_request: WorkerRequest,
    pub worker_reply_sequence: u32,
    pub reserved20: u32,
    pub worker_reply: WorkerReply,
}

pub const LAYOUT_SIZE: usize = core::mem::size_of::<LayoutMemory>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    Incompatible(u32),
    Busy,
}

/// A handle to the shared memory block, valid for `'a`; the one place that vouches the pointer is a live mapping.
#[derive(Clone, Copy)]
pub struct Shared<'a> {
    ptr: NonNull<LayoutMemory>,
    view: PhantomData<&'a LayoutMemory>,
}

impl<'a> Shared<'a> {
    /// Zeroes the block and stamps the protocol version; `ptr` must be a writable, aligned mapping of `LAYOUT_SIZE` bytes nobody else uses yet.
    pub unsafe fn create(ptr: NonNull<LayoutMemory>) -> Self {
        let this = ptr.as_ptr();
        unsafe {
            core::ptr::write_bytes(this, 0, 1);
            addr_of_mut!((*this).protocol).write(PROTOCOL_VERSION as u32);
        }
        Self {
            ptr,
            view: PhantomData,
        }
    }

    /// `ptr` must be a writable, aligned mapping of `LAYOUT_SIZE` bytes set up by `create`.
    pub unsafe fn attach(ptr: NonNull<LayoutMemory>) -> Self {
        Self {
            ptr,
            view: PhantomData,
        }
    }

    fn this(self) -> *mut LayoutMemory {
        self.ptr.as_ptr()
    }

    pub fn write(self, layout: &Layout) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).sequence));
            let start = sequence.load(Ordering::Relaxed);
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            addr_of_mut!((*this).layout).write_volatile(*layout);
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    pub fn write_name_request(self, request: &NameRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).request_sequence),
                addr_of_mut!((*this).name_request),
                request,
            )
        }
    }

    pub fn read_name_request(self) -> Option<(u32, NameRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).request_sequence),
                addr_of!((*this).name_request),
                1,
            )
        }
    }

    pub fn write_name_status(self, status: &NameStatus) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).status_sequence),
                addr_of_mut!((*this).name_status),
                status,
            );
        }
    }

    pub fn read_name_status(self, attempts: usize) -> Option<NameStatus> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).status_sequence),
                addr_of!((*this).name_status),
                attempts,
            )
            .map(|(_, s)| s)
        }
    }

    pub fn write_total_xp_request(self, request: &TotalXpRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).xp_request_sequence),
                addr_of_mut!((*this).total_xp_request),
                request,
            )
        }
    }

    pub fn read_total_xp_request(self) -> Option<(u32, TotalXpRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).xp_request_sequence),
                addr_of!((*this).total_xp_request),
                1,
            )
        }
    }

    pub fn write_total_xp_status(self, status: &TotalXpStatus) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).xp_status_sequence),
                addr_of_mut!((*this).total_xp_status),
                status,
            );
        }
    }

    pub fn read_total_xp_status(self, attempts: usize) -> Option<TotalXpStatus> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).xp_status_sequence),
                addr_of!((*this).total_xp_status),
                attempts,
            )
            .map(|(_, s)| s)
        }
    }

    pub fn write_item_name_request(self, request: &ItemNameRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).item_request_sequence),
                addr_of_mut!((*this).item_name_request),
                request,
            )
        }
    }

    pub fn read_item_name_request(self) -> Option<(u32, ItemNameRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).item_request_sequence),
                addr_of!((*this).item_name_request),
                1,
            )
        }
    }

    pub fn write_item_name_status(self, status: &NamesReply) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).item_status_sequence),
                addr_of_mut!((*this).item_name_status),
                status,
            );
        }
    }

    pub fn read_item_name_status(self, attempts: usize) -> Option<NamesReply> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).item_status_sequence),
                addr_of!((*this).item_name_status),
                attempts,
            )
            .map(|(_, s)| s)
        }
    }

    pub fn write_game_val_request(self, request: &GameValRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).game_val_request_sequence),
                addr_of_mut!((*this).game_val_request),
                request,
            )
        }
    }

    pub fn read_game_val_request(self) -> Option<(u32, GameValRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).game_val_request_sequence),
                addr_of!((*this).game_val_request),
                1,
            )
        }
    }

    pub fn write_game_val_status(self, status: &NamesReply) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).game_val_status_sequence),
                addr_of_mut!((*this).game_val_status),
                status,
            );
        }
    }

    pub fn read_game_val_status(self, attempts: usize) -> Option<NamesReply> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).game_val_status_sequence),
                addr_of!((*this).game_val_status),
                attempts,
            )
            .map(|(_, s)| s)
        }
    }

    pub fn write_project_request(self, request: &ProjectRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).project_request_sequence),
                addr_of_mut!((*this).project_request),
                request,
            )
        }
    }

    pub fn read_project_request(self) -> Option<(u32, ProjectRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).project_request_sequence),
                addr_of!((*this).project_request),
                1,
            )
        }
    }

    pub fn write_project_reply(self, reply: &ProjectReply) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).project_reply_sequence),
                addr_of_mut!((*this).project_reply),
                reply,
            );
        }
    }

    pub fn read_project_reply(self, attempts: usize) -> Option<ProjectReply> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).project_reply_sequence),
                addr_of!((*this).project_reply),
                attempts,
            )
            .map(|(_, r)| r)
        }
    }

    pub fn write_menu_request(self) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).menu_request_sequence),
                addr_of_mut!((*this).reserved11),
                &0,
            )
        }
    }

    pub fn read_menu_request(self) -> Option<u32> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).menu_request_sequence),
                addr_of!((*this).reserved11),
                1,
            )
            .map(|(s, _)| s)
        }
    }

    /// Fills the menu reply in place, as it is too big for the window procedure's stack; `fill` must set every field.
    pub fn write_menu_reply_with(self, fill: impl FnOnce(&mut MenuReply)) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).menu_reply_sequence));
            let start = sequence.load(Ordering::Relaxed) & !1;
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            fill(&mut *addr_of_mut!((*this).menu_reply));
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    pub fn read_menu_reply(self, attempts: usize) -> Option<MenuReply> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).menu_reply_sequence),
                addr_of!((*this).menu_reply),
                attempts,
            )
            .map(|(_, r)| r)
        }
    }

    pub fn write_model_request(self, request: &ModelRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).model_request_sequence),
                addr_of_mut!((*this).model_request),
                request,
            )
        }
    }

    pub fn read_model_request(self) -> Option<(u32, ModelRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).model_request_sequence),
                addr_of!((*this).model_request),
                1,
            )
        }
    }

    pub fn write_model_reply_with(self, fill: impl FnOnce(&mut ModelReply)) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).model_reply_sequence));
            let start = sequence.load(Ordering::Relaxed) & !1;
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            fill(&mut *addr_of_mut!((*this).model_reply));
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    pub fn read_model_reply(self, attempts: usize) -> Option<ModelReply> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).model_reply_sequence),
                addr_of!((*this).model_reply),
                attempts,
            )
            .map(|(_, r)| r)
        }
    }

    /// `request` is a `locs_request` value.
    pub fn write_locs_request(self, request: i32) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).locs_request_sequence),
                addr_of_mut!((*this).locs_request),
                &request,
            )
        }
    }

    pub fn read_locs_request(self) -> Option<(u32, i32)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).locs_request_sequence),
                addr_of!((*this).locs_request),
                1,
            )
        }
    }

    pub fn write_locs_reply_with(self, fill: impl FnOnce(&mut LocsReply)) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).locs_reply_sequence));
            let start = sequence.load(Ordering::Relaxed) & !1;
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            fill(&mut *addr_of_mut!((*this).locs_reply));
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    pub fn read_locs_reply_into(self, out: &mut LocsReply, attempts: usize) -> bool {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of!((*this).locs_reply_sequence) as *mut u32);
            for _ in 0..attempts {
                let before = sequence.load(Ordering::Acquire);
                if before % 2 == 1 {
                    core::hint::spin_loop();
                    continue;
                }
                core::ptr::copy_nonoverlapping(addr_of!((*this).locs_reply), out, 1);
                fence(Ordering::Acquire);
                if sequence.load(Ordering::Relaxed) == before {
                    return true;
                }
            }
            false
        }
    }

    pub fn write_worker_request(self, request: &WorkerRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).worker_request_sequence),
                addr_of_mut!((*this).worker_request),
                request,
            )
        }
    }

    pub fn read_worker_request(self) -> Option<(u32, WorkerRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).worker_request_sequence),
                addr_of!((*this).worker_request),
                100,
            )
        }
    }

    pub fn write_worker_reply_with(self, fill: impl FnOnce(&mut WorkerReply)) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).worker_reply_sequence));
            let start = sequence.load(Ordering::Relaxed) & !1;
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            fill(&mut *addr_of_mut!((*this).worker_reply));
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    /// The request sequence the reply answers, or `None` while the agent is writing it.
    pub fn worker_reply_answers(self) -> Option<u32> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of!((*this).worker_reply_sequence) as *mut u32);
            let before = sequence.load(Ordering::Acquire);
            if before % 2 == 1 {
                return None;
            }
            let answers = addr_of!((*this).worker_reply.sequence).read_volatile();
            fence(Ordering::Acquire);
            (sequence.load(Ordering::Relaxed) == before).then_some(answers)
        }
    }

    /// Copies the answer to request `sequence` into `data` and returns its status; `None` if another request's or changed.
    pub fn read_worker_reply(self, sequence: u32, data: &mut alloc::vec::Vec<u8>) -> Option<i32> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let counter = AtomicU32::from_ptr(addr_of!((*this).worker_reply_sequence) as *mut u32);
            let before = counter.load(Ordering::Acquire);
            if before % 2 == 1 {
                return None;
            }
            let reply = addr_of!((*this).worker_reply);
            let (answers, status, length) = (
                addr_of!((*reply).sequence).read_volatile(),
                addr_of!((*reply).status).read_volatile(),
                (addr_of!((*reply).length).read_volatile() as usize).min(WORKER_DATA_SIZE),
            );
            if answers != sequence {
                return None;
            }
            data.clear();
            data.reserve(length);
            core::ptr::copy_nonoverlapping(addr_of!((*reply).data) as *const u8, data.as_mut_ptr(), length);
            data.set_len(length);
            fence(Ordering::Acquire);
            (counter.load(Ordering::Relaxed) == before).then_some(status)
        }
    }

    pub fn write_cache_request(self, request: &CacheRequest) -> u32 {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_write(
                addr_of_mut!((*this).cache_request_sequence),
                addr_of_mut!((*this).cache_request),
                request,
            )
        }
    }

    pub fn read_cache_request(self) -> Option<(u32, CacheRequest)> {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            seqlock_read(
                addr_of!((*this).cache_request_sequence),
                addr_of!((*this).cache_request),
                1,
            )
        }
    }

    pub fn write_cache_reply_with(self, fill: impl FnOnce(&mut CacheReply)) {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of_mut!((*this).cache_reply_sequence));
            let start = sequence.load(Ordering::Relaxed) & !1;
            sequence.store(start.wrapping_add(1), Ordering::Relaxed);
            fence(Ordering::Release);
            fill(&mut *addr_of_mut!((*this).cache_reply));
            sequence.store(start.wrapping_add(2), Ordering::Release);
        }
    }

    pub fn read_cache_reply_into(self, out: &mut CacheReply, attempts: usize) -> bool {
        let this = self.this();
        // SAFETY: `Shared` only holds pointers that `create`/`attach` promised are live mappings.
        unsafe {
            let sequence = AtomicU32::from_ptr(addr_of!((*this).cache_reply_sequence) as *mut u32);
            let reply = addr_of!((*this).cache_reply);
            for _ in 0..attempts {
                let before = sequence.load(Ordering::Acquire);
                if before % 2 == 1 {
                    core::hint::spin_loop();
                    continue;
                }
                let header = core::mem::offset_of!(CacheReply, data);
                core::ptr::copy_nonoverlapping(
                    reply as *const u8,
                    out as *mut CacheReply as *mut u8,
                    header,
                );
                let used = (out.used as usize).min(CACHE_DATA_SIZE);
                core::ptr::copy_nonoverlapping(
                    addr_of!((*reply).data) as *const u8,
                    out.data.as_mut_ptr(),
                    used,
                );
                fence(Ordering::Acquire);
                if sequence.load(Ordering::Relaxed) == before {
                    return true;
                }
            }
            false
        }
    }
}

/// A read-only handle, for mappings opened without write access.
#[derive(Clone, Copy)]
pub struct LayoutView<'a> {
    ptr: NonNull<LayoutMemory>,
    view: PhantomData<&'a LayoutMemory>,
}

impl<'a> LayoutView<'a> {
    /// `ptr` must be a readable, aligned mapping of `LAYOUT_SIZE` bytes.
    pub unsafe fn attach(ptr: NonNull<LayoutMemory>) -> Self {
        Self {
            ptr,
            view: PhantomData,
        }
    }

    pub fn read(self, attempts: usize) -> Result<Layout, ReadError> {
        let this = self.ptr.as_ptr();
        // SAFETY: `attach` promised a live mapping.
        unsafe {
            let protocol = addr_of!((*this).protocol).read_volatile();
            if protocol != PROTOCOL_VERSION as u32 {
                return Err(ReadError::Incompatible(protocol));
            }

            let sequence = AtomicU32::from_ptr(addr_of!((*this).sequence) as *mut u32);
            for _ in 0..attempts {
                let before = sequence.load(Ordering::Acquire);
                if before % 2 == 1 {
                    core::hint::spin_loop();
                    continue;
                }
                let layout = addr_of!((*this).layout).read_volatile();
                fence(Ordering::Acquire);
                if sequence.load(Ordering::Relaxed) == before {
                    return Ok(layout);
                }
            }
            Err(ReadError::Busy)
        }
    }
}

unsafe fn seqlock_write<T: Copy>(sequence: *mut u32, data: *mut T, value: &T) -> u32 {
    let sequence = AtomicU32::from_ptr(sequence);
    let start = sequence.load(Ordering::Relaxed) & !1;
    sequence.store(start.wrapping_add(1), Ordering::Relaxed);
    fence(Ordering::Release);
    data.write_volatile(*value);
    sequence.store(start.wrapping_add(2), Ordering::Release);
    start.wrapping_add(2)
}

unsafe fn seqlock_read<T: Copy>(
    sequence: *const u32,
    data: *const T,
    attempts: usize,
) -> Option<(u32, T)> {
    let sequence = AtomicU32::from_ptr(sequence as *mut u32);
    for _ in 0..attempts {
        let before = sequence.load(Ordering::Acquire);
        if before % 2 == 1 {
            core::hint::spin_loop();
            continue;
        }
        let value = data.read_volatile();
        fence(Ordering::Acquire);
        if sequence.load(Ordering::Relaxed) == before {
            return Some((before, value));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn layout() {
        assert_eq!(offset_of!(LayoutMemory, protocol), 0);
        assert_eq!(offset_of!(LayoutMemory, sequence), 4);
        assert_eq!(offset_of!(LayoutMemory, layout), 8);
        assert_eq!(size_of::<PlayerLayout>(), 6 * 4);
        assert_eq!(size_of::<SkillsLayout>(), 4 * 4);
        assert_eq!(size_of::<LoginLayout>(), 5 * 4);
        assert_eq!(size_of::<InterfaceLayout>(), 8 + 22 * 4);
        // 27 u32s after the u64 module base, padded to 8 bytes
        assert_eq!(size_of::<WorldLayout>(), 120);
        assert_eq!(size_of::<WorldSelectLayout>(), 14 * 4);
        assert_eq!(offset_of!(Layout, interfaces), 16 + 16 + 24 + 16 + 20 + 4);
        assert_eq!(size_of::<NameRequest>(), 8 + 48);
        assert_eq!(size_of::<NameStatus>(), 8 + 24);
        assert_eq!(size_of::<TotalXpRequest>(), 16);
        assert_eq!(size_of::<TotalXpStatus>(), 8);
        assert_eq!(
            size_of::<CameraLayout>() + size_of::<MinimapLayout>(),
            7 * 4
        );
        assert_eq!(size_of::<ProjectionLayout>(), 24 * 4);
        assert_eq!(size_of::<ChatLayout>(), 10 * 4);
        assert_eq!(size_of::<ProjectileLayout>(), 24 * 4);
        assert_eq!(size_of::<SpotAnimLayout>(), 8 * 4);
        assert_eq!(
            offset_of!(LayoutMemory, request_sequence),
            8 + 96 + 96 + 120 + 56 + 28 + 96 + 16 + 40 + 8 + 4 + 96 + 32
        );
        let entry = MENU_OPTION_SIZE + MENU_TARGET_SIZE + 20;
        assert_eq!(size_of::<MenuEntryInfo>(), entry);
        assert_eq!(size_of::<MenuGeometry>(), 12 * 4);
        // the menu geometry and its flag, then the submenus
        let submenu = 8 + 48 + MENU_SUBMENU_ENTRIES * entry;
        assert_eq!(size_of::<MenuSubmenu>(), submenu);
        assert_eq!(
            size_of::<MenuReply>(),
            16 + MENU_ENTRIES * entry + entry + 16 + entry + 8 + 52 + 4 + MENU_SUBMENUS * submenu
        );
        assert_eq!(size_of::<ItemNameRequest>(), 8 + 64 * 4);
        assert_eq!(size_of::<NamesReply>(), 16 + 64 * 64);
        assert_eq!(
            LAYOUT_SIZE,
            8 + 96
                + 96
                + 120
                + 56
                + 28
                + 96
                + 16
                + 40
                + 8
                + 4
                + 96
                + 32
                + 8
                + 56
                + 8
                + 32
                + 8
                + 16
                + 8
                + 8
                + 8
                + 264
                + 8
                + 4112
                + 8
                + 264
                + 8
                + 4112
                + 8
                + 8
                + 768
                + 8
                + 16
                + 512
                + 8
                + 8
                + (16
                    + MENU_ENTRIES * entry
                    + entry
                    + 16
                    + entry
                    + 8
                    + 52
                    + 4
                    + MENU_SUBMENUS * submenu)
                + 8
                + 24
                + 8
                + 1120
                + MODEL_FACES * 24
                + 16
                + size_of::<LocsReply>()
                + 8
                + size_of::<CacheRequest>()
                + 8
                + size_of::<CacheReply>()
                + 8
                + size_of::<WorkerRequest>()
                + 8
                + size_of::<WorkerReply>()
        );
        assert_eq!(size_of::<CacheRequest>(), 16 + CACHE_BATCH * 8);
        assert_eq!(
            size_of::<CacheReply>(),
            24 + CACHE_BATCH * 8 + CACHE_DATA_SIZE
        );
        assert_eq!(size_of::<LocInfo>(), 16);
        assert_eq!(size_of::<WorkerRequest>(), 24);
        assert_eq!(size_of::<WorkerReply>(), 16 + WORKER_DATA_SIZE);
        assert_eq!(
            size_of::<LocsReply>(),
            32 + MAX_LOCS * 16 + MAX_LOC_NAMES * LOC_NAME_SIZE
        );
        assert_eq!(
            size_of::<ModelReply>(),
            32 + 8 * 8 + MODEL_HULL * 8 + MODEL_FACES * 24
        );
    }

    #[test]
    fn write_then_read() {
        // On the heap: the layout is megabytes, more than a test thread's stack.
        let mut memory = alloc::boxed::Box::<LayoutMemory>::new_uninit();
        let shared = (*memory).as_mut_ptr();
        unsafe {
            let ptr = shared;
            let shared = Shared::create(NonNull::new(ptr).unwrap());
            let view = LayoutView::attach(NonNull::new(ptr).unwrap());
            assert_eq!(view.read(1), Ok(Layout::default()));

            let layout = Layout {
                found: part::GAME_STATE | part::SKILLS,
                module_base: 0x1_4000_0000,
                client: 0xe98c18,
                game_state: 0x2160,
                skills: SkillsLayout {
                    levels: 0x3350,
                    real_levels: 0x33b4,
                    xp: 0x3418,
                    count: 25,
                },
                ..Layout::default()
            };
            shared.write(&layout);
            assert_eq!((*ptr).sequence, 2);
            let read = view.read(1).unwrap();
            assert!(read.has(part::GAME_STATE | part::SKILLS));
            assert!(!read.has(part::GAME_STATE | part::VARPS));
            assert_eq!(read, layout);
        }
    }

    #[test]
    fn total_xp_requests_and_status_round_trip() {
        // On the heap: the layout is megabytes, more than a test thread's stack.
        let mut memory = alloc::boxed::Box::<LayoutMemory>::new_uninit();
        let shared = (*memory).as_mut_ptr();
        unsafe {
            let ptr = shared;
            let shared = Shared::create(NonNull::new(ptr).unwrap());
            assert_eq!(
                shared.read_total_xp_request(),
                Some((0, TotalXpRequest::default()))
            );
            let request = TotalXpRequest {
                enabled: 1,
                offset: -1_234_567,
                ..TotalXpRequest::default()
            };
            assert_eq!(shared.write_total_xp_request(&request), 2);
            assert_eq!(shared.read_total_xp_request(), Some((2, request)));
            let status = TotalXpStatus {
                sequence: 2,
                status: total_xp_status::ON,
            };
            shared.write_total_xp_status(&status);
            assert_eq!(shared.read_total_xp_status(1), Some(status));
            assert_eq!(
                shared.read_name_request(),
                Some((0, NameRequest::default()))
            );
        }
    }

    #[test]
    fn item_name_requests_and_status_round_trip() {
        // On the heap: the layout is megabytes, more than a test thread's stack.
        let mut memory = alloc::boxed::Box::<LayoutMemory>::new_uninit();
        let shared = (*memory).as_mut_ptr();
        unsafe {
            let ptr = shared;
            let shared = Shared::create(NonNull::new(ptr).unwrap());
            let mut request = ItemNameRequest {
                count: 2,
                ..ItemNameRequest::default()
            };
            request.ids[..2].copy_from_slice(&[1925, 13241]);
            assert_eq!(shared.write_item_name_request(&request), 2);
            assert_eq!(shared.read_item_name_request(), Some((2, request)));

            let mut status = NamesReply {
                sequence: 2,
                status: item_name_status::OK,
                count: 2,
                ..NamesReply::default()
            };
            status.set_name(0, b"Bucket");
            status.set_name(1, &[b'a'; 70]);
            shared.write_item_name_status(&status);
            let read = shared.read_item_name_status(1).unwrap();
            assert_eq!(read.name(0), Some(&b"Bucket"[..]));
            assert_eq!(read.name(1).unwrap().len(), ITEM_NAME_SIZE);
            assert_eq!(read.name(2), None);
        }
    }

    #[test]
    fn name_requests_and_status_round_trip() {
        // On the heap: the layout is megabytes, more than a test thread's stack.
        let mut memory = alloc::boxed::Box::<LayoutMemory>::new_uninit();
        let shared = (*memory).as_mut_ptr();
        unsafe {
            let ptr = shared;
            let shared = Shared::create(NonNull::new(ptr).unwrap());
            let view = LayoutView::attach(NonNull::new(ptr).unwrap());
            assert_eq!(
                shared.read_name_request(),
                Some((0, NameRequest::default()))
            );
            let request = NameRequest {
                enabled: 1,
                display: crate::read::short_string(b"Test\xc2\xa0Player").unwrap(),
                account: crate::read::short_string(b"p_4i_n").unwrap(),
                ..NameRequest::default()
            };
            let sequence = shared.write_name_request(&request);
            assert_eq!(sequence, 2);
            assert_eq!(shared.read_name_request(), Some((2, request)));
            assert_eq!(shared.write_name_request(&request), 4);

            let status = NameStatus {
                sequence: 4,
                status: name_status::KEPT,
                real: [1; 24],
            };
            shared.write_name_status(&status);
            assert_eq!(shared.read_name_status(1), Some(status));
            assert_eq!(view.read(1), Ok(Layout::default()));
        }
    }

    #[test]
    fn rejects_busy_and_incompatible() {
        // On the heap: the layout is megabytes, more than a test thread's stack.
        let mut memory = alloc::boxed::Box::<LayoutMemory>::new_uninit();
        let shared = (*memory).as_mut_ptr();
        unsafe {
            let ptr = shared;
            Shared::create(NonNull::new(ptr).unwrap());
            let view = LayoutView::attach(NonNull::new(ptr).unwrap());
            (*ptr).sequence = 1;
            assert_eq!(view.read(3), Err(ReadError::Busy));

            (*ptr).protocol = PROTOCOL_VERSION as u32 + 1;
            assert_eq!(
                view.read(3),
                Err(ReadError::Incompatible(PROTOCOL_VERSION as u32 + 1))
            );
        }
    }

    #[test]
    fn names_are_per_process() {
        assert_eq!(state_memory_name(32), "AGENT_STATE_32");
    }
}
