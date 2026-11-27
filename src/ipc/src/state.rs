use alloc::{format, string::String, vec::Vec};
use core::{
    marker::PhantomData,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{fence, AtomicU32, Ordering},
};

use crate::PROTOCOL_VERSION;

pub fn state_memory_name(pid: u32) -> String {
    format!("AGENT_STATE_{pid}")
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
    pub const PATH: u64 = 1 << 19;
    pub const MENU: u64 = 1 << 20;
    pub const MENU_CLICK: u64 = 1 << 21;
    pub const MENU_OPEN: u64 = 1 << 22;
    pub const SELECTION: u64 = 1 << 23;
    pub const LOGIN_PROVIDER: u64 = 1 << 24;
    pub const MENU_ORDER: u64 = 1 << 25;
    pub const MENU_SETTINGS: u64 = 1 << 26;
    pub const GROUND_ITEM_OWNERSHIP: u64 = 1 << 27;
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
    const STARTING: i32 = 0;
    pub const LOGIN_SCREEN: i32 = 10;
    pub const LOGIN_SCREEN_AUTHENTICATOR: i32 = 11;
    const LOGGING_IN: i32 = 20;
    pub const LOADING: i32 = 25;
    pub const LOGGED_IN: i32 = 30;
    const CONNECTION_LOST: i32 = 40;
    const HOPPING: i32 = 45;

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
    pub provider_holder: u32,
    pub provider: u32,
    pub steam_provider_vtable: u32,
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
    pub const ENTRY_SIZE: usize = 0x10;
    pub const ENTRY_COMPONENT: usize = 0x8;
    pub const CHILDREN_DATA: usize = 0x8;
    pub const ROOT_FIXED: i32 = 548;
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
    pub npc_animation: u32,
    pub npc_head_icons: u32,
    pub head_icon_sprites: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldSelectLayout {
    pub switcher: u32,
    pub current_world: u32,
    pub master_count: u32,
    pub master_worlds: u32,
    pub open: u32,
    pub list: u32,
    pub state: u32,
    pub layout: u32,
    pub selected: u32,
    pub hovered: u32,
    pub page: u32,
    pub compact: u32,
    pub list_rect: u32,
    pub row_offset: u32,
    pub row_height: u32,
    pub rows: u32,
    pub columns: u32,
    pub compact_columns: u32,
    pub previous: u32,
    pub next: u32,
    pub id: u32,
    pub types: u32,
    pub players: u32,
    pub activity: u32,
    pub location: u32,
    pub reserved: u32,
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
    pub public_tick: u32,
    pub despawn_tick: u32,
    pub ownership: u32,
    pub never_public: u32,
    pub op_flags: u32,
}

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollisionLayout {
    pub maps: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneLoaderLayout {
    pub instance_flag: u32,
}

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MenuStateLayout {
    pub object: u32,
    pub entry_vtable: u32,
    pub entry_size: u32,
    pub target: u32,
    pub option: u32,
    pub ids: u32,
    pub priority: u32,
    pub kind: u32,
    pub category: u32,
    pub cancel: u32,
    pub order_key: u32,
    pub player_attack: u32,
    pub npc_attack: u32,
    pub follower: u32,
    pub team: u32,
    pub flag: u32,
    pub npc_level: u32,
    pub npc_follower: u32,
    pub player_level: u32,
    pub player_team: u32,
    pub minimenu_vtable: u32,
    pub click_entry: u32,
    pub click_x: u32,
    pub click_counter: u32,
    pub open: u32,
    pub selection: u32,
    pub selection_verb: u32,
    pub selection_name: u32,
    pub selection_id: u32,
    pub selection_child: u32,
    pub selection_item: u32,
    pub selection_flags: u32,
    pub selection_selected: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathLayout {
    pub x: u32,
    pub y: u32,
    pub length: u32,
    pub reserved: u32,
}

pub mod collision_map {
    pub const WIDTH: usize = 0x8;
    pub const HEIGHT: usize = 0xc;
    pub const COLUMNS: usize = 0x10;
    pub const COLUMN_SIZE: usize = 0x18;
    pub const BORDER: usize = 1;
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
    pub path: PathLayout,
    pub menu: MenuStateLayout,
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

pub mod hide {
    pub const OTHERS: u64 = 1 << 0;
    pub const OTHERS_2D: u64 = 1 << 1;
    pub const FRIENDS: u64 = 1 << 2;
    pub const FRIENDS_CHAT: u64 = 1 << 3;
    pub const CLAN: u64 = 1 << 4;
    pub const IGNORED: u64 = 1 << 5;
    pub const LOCAL: u64 = 1 << 6;
    pub const LOCAL_2D: u64 = 1 << 7;
    pub const NPCS: u64 = 1 << 8;
    pub const NPCS_2D: u64 = 1 << 9;
    pub const PETS: u64 = 1 << 10;
    pub const THRALLS: u64 = 1 << 11;
    pub const DEAD_NPCS: u64 = 1 << 12;
    pub const RANDOM_EVENTS: u64 = 1 << 13;
    pub const ATTACKERS: u64 = 1 << 14;
    pub const PROJECTILES: u64 = 1 << 15;
    pub const SPOT_ANIMS: u64 = 1 << 16;
    pub const WALLS: u64 = 1 << 17;
    pub const WALL_DECORATIONS: u64 = 1 << 18;
    pub const GROUND_DECORATIONS: u64 = 1 << 19;
    pub const OBJECTS: u64 = 1 << 20;
    pub const GROUND_ITEMS: u64 = 1 << 21;
    pub const ALL: u64 = (1 << 22) - 1;
}

pub mod hider_status {
    pub const OFF: i32 = 0;
    pub const ON: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HiderRequest {
    pub mask: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HiderStatus {
    pub sequence: u32,
    pub status: i32,
    pub mask: u64,
}

pub mod game_val {
    pub const OBJ: u32 = 0;
    pub const VARBIT: u32 = 4;
    pub const TABLE: u32 = 10;
    pub const INTERFACE: u32 = 14;
}

pub const MENU_OPTION_SIZE: usize = 40;
const MENU_TARGET_SIZE: usize = 256;

pub mod menu_kind {
    pub const TILE: u8 = 1;
    pub const NPC: u8 = 2;
    pub const PLAYER: u8 = 6;
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
    pub x: i32,
    pub y: i32,
    pub kind: u8,
    pub category: u8,
    pub cancel: u8,
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
            x: -1,
            y: -1,
            kind: 0,
            category: 0,
            cancel: 0,
            submenu: 0,
        }
    }
}

pub mod model_status {
    pub const NOT_FOUND: i32 = -1;
    pub const NOT_IN_GAME: i32 = -4;
    pub const NO_NPC: i32 = -5;
    pub const NO_MODEL: i32 = -6;
}

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
    pub const MENU: u32 = 14;
    pub const MODEL: u32 = 15;
    pub const LOCS: u32 = 16;
}

pub mod worker_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const TOO_BIG: i32 = -3;
}

const WORKER_DATA_SIZE: usize = 512 * 1024;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkerRequest {
    pub kind: u32,
    pub args: [i32; 4],
    pub reserved: u32,
}

#[repr(C)]
pub struct WorkerReply {
    pub sequence: u32,
    pub status: i32,
    pub length: u32,
    pub reserved: u32,
    pub data: [u8; WORKER_DATA_SIZE],
}

pub const ACT_OPTIONS: usize = 4;
pub const ACT_BLOCKERS: usize = 16;

pub mod act_command {
    pub const ARM: u32 = 1;
    pub const CANCEL: u32 = 2;
}

pub mod act_status {
    pub(crate) const IDLE: i32 = 0;
    pub const TRACKING: i32 = 1;
    pub const LEFT: i32 = 2;
    pub const RIGHT: i32 = 3;
    pub const NO_OPTION: i32 = -1;
    pub const GONE: i32 = -2;
    pub const MENU_OPEN: i32 = -3;
    pub const TIMEOUT: i32 = -4;
    pub const CANCELLED: i32 = -5;
    pub const NOT_FOUND: i32 = -6;
    pub const OTHER_THREAD: i32 = -7;
    pub const NOT_IN_GAME: i32 = -8;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActRequest {
    pub command: u32,
    pub right: u32,
    pub model: ModelRequest,
    pub entry_kind: i32,
    pub entry_id: i32,
    pub entry_index: i32,
    pub entry_x: i32,
    pub entry_y: i32,
    pub option_count: u32,
    pub options: [[u8; MENU_OPTION_SIZE]; ACT_OPTIONS],
    pub near_px: f32,
    pub spread: f32,
    pub min_area: f32,
    pub timeout_ms: u32,
    pub viewport: [i32; 4],
    pub blocker_count: u32,
    pub blockers: [[i32; 4]; ACT_BLOCKERS],
}

impl Default for ActRequest {
    fn default() -> Self {
        Self {
            command: 0,
            right: 0,
            model: ModelRequest::default(),
            entry_kind: -1,
            entry_id: -1,
            entry_index: -1,
            entry_x: -1,
            entry_y: -1,
            option_count: 0,
            options: [[0; MENU_OPTION_SIZE]; ACT_OPTIONS],
            near_px: 0.0,
            spread: 0.0,
            min_area: 0.0,
            timeout_ms: 0,
            viewport: [0; 4],
            blocker_count: 0,
            blockers: [[0; 4]; ACT_BLOCKERS],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActState {
    pub request: u32,
    pub status: i32,
    pub frame: u32,
    pub has_spot: u32,
    pub spot: [f32; 2],
    pub face: i32,
    pub face_count: u32,
    pub mouse: [i32; 2],
    pub entry: MenuEntryInfo,
}

impl Default for ActState {
    fn default() -> Self {
        Self {
            request: 0,
            status: act_status::IDLE,
            frame: 0,
            has_spot: 0,
            spot: [0.0; 2],
            face: -1,
            face_count: 0,
            mouse: [0; 2],
            entry: MenuEntryInfo::default(),
        }
    }
}

pub const ITEM_ICON_WIDTH: u32 = 36;
pub const ITEM_ICON_HEIGHT: u32 = 32;
pub const ITEM_ICON_SHOW_COUNT: i32 = 1 << 24;
pub const ITEM_ICON_ID_MASK: i32 = ITEM_ICON_SHOW_COUNT - 1;

pub mod cache_status {
    pub const OK: i32 = 1;
    pub const NOT_FOUND: i32 = -1;
    pub const OTHER_THREAD: i32 = -2;
    pub const TOO_MANY: i32 = -3;
}

pub const CACHE_FILE_MISSING: i32 = -1;
pub const CACHE_FILE_NO_ROOM: i32 = -2;

pub const CACHE_BATCH: usize = 256;
pub const CACHE_DATA_SIZE: usize = 4 << 20;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheRequest {
    pub count: u32,
    pub reserved: u32,
    pub files: [[i32; 2]; CACHE_BATCH],
}

impl Default for CacheRequest {
    fn default() -> Self {
        Self {
            count: 0,
            reserved: 0,
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
    pub act_request_sequence: u32,
    pub reserved21: u32,
    pub act_request: ActRequest,
    pub act_state_sequence: u32,
    pub reserved22: u32,
    pub act_state: ActState,
    pub hider_request_sequence: u32,
    pub reserved23: u32,
    pub hider_request: HiderRequest,
    pub hider_status_sequence: u32,
    pub reserved24: u32,
    pub hider_status: HiderStatus,
}

pub const LAYOUT_SIZE: usize = core::mem::size_of::<LayoutMemory>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    Incompatible(u32),
    Busy,
}

#[derive(Clone, Copy)]
pub struct Shared<'a> {
    ptr: NonNull<LayoutMemory>,
    view: PhantomData<&'a LayoutMemory>,
}

macro_rules! seqlocked {
    ($sequence:ident, $field:ident: $ty:ty; write $write:ident) => {
        pub fn $write(self, value: &$ty) -> u32 {
            let this = self.this();
            unsafe {
                seqlock_write(
                    addr_of_mut!((*this).$sequence),
                    addr_of_mut!((*this).$field),
                    value,
                )
            }
        }
    };
    ($sequence:ident, $field:ident: $ty:ty; read $read:ident) => {
        pub fn $read(self, attempts: usize) -> Option<$ty> {
            let this = self.this();
            unsafe {
                seqlock_read(
                    addr_of!((*this).$sequence),
                    addr_of!((*this).$field),
                    attempts,
                )
                .map(|(_, value)| value)
            }
        }
    };
    ($sequence:ident, $field:ident: $ty:ty; fill $write:ident) => {
        pub fn $write(self, fill: impl FnOnce(&mut $ty)) {
            let this = self.this();
            unsafe {
                seqlock_write_with(
                    addr_of_mut!((*this).$sequence),
                    addr_of_mut!((*this).$field),
                    |data| fill(&mut *data),
                );
            }
        }
    };
    ($sequence:ident, $field:ident: $ty:ty; request $write:ident, $read:ident($attempts:expr)) => {
        seqlocked!($sequence, $field: $ty; write $write);

        pub fn $read(self) -> Option<(u32, $ty)> {
            let this = self.this();
            unsafe {
                seqlock_read(
                    addr_of!((*this).$sequence),
                    addr_of!((*this).$field),
                    $attempts,
                )
            }
        }
    };
    ($sequence:ident, $field:ident: $ty:ty; reply $write:ident, $read:ident) => {
        seqlocked!($sequence, $field: $ty; write $write);
        seqlocked!($sequence, $field: $ty; read $read);
    };
}

impl<'a> Shared<'a> {
    pub unsafe fn create(ptr: NonNull<LayoutMemory>) -> Self {
        let this = ptr.as_ptr();
        unsafe {
            core::ptr::write_bytes(this, 0, 1);
            addr_of_mut!((*this).protocol).write(PROTOCOL_VERSION as u32);
        }
        Self::attach(ptr)
    }

    pub unsafe fn attach(ptr: NonNull<LayoutMemory>) -> Self {
        Self {
            ptr,
            view: PhantomData,
        }
    }

    fn this(self) -> *mut LayoutMemory {
        self.ptr.as_ptr()
    }

    seqlocked!(sequence, layout: Layout; write write);
    seqlocked!(request_sequence, name_request: NameRequest; request write_name_request, read_name_request(1));
    seqlocked!(status_sequence, name_status: NameStatus; reply write_name_status, read_name_status);
    seqlocked!(xp_request_sequence, total_xp_request: TotalXpRequest; request write_total_xp_request, read_total_xp_request(1));
    seqlocked!(xp_status_sequence, total_xp_status: TotalXpStatus; reply write_total_xp_status, read_total_xp_status);
    seqlocked!(cache_request_sequence, cache_request: CacheRequest; request write_cache_request, read_cache_request(1));
    seqlocked!(cache_reply_sequence, cache_reply: CacheReply; fill write_cache_reply_with);
    seqlocked!(worker_request_sequence, worker_request: WorkerRequest; request write_worker_request, read_worker_request(100));
    seqlocked!(worker_reply_sequence, worker_reply: WorkerReply; fill write_worker_reply_with);
    seqlocked!(act_request_sequence, act_request: ActRequest; request write_act_request, read_act_request(1));
    seqlocked!(act_state_sequence, act_state: ActState; reply write_act_state, read_act_state);
    seqlocked!(hider_request_sequence, hider_request: HiderRequest; request write_hider_request, read_hider_request(1));
    seqlocked!(hider_status_sequence, hider_status: HiderStatus; reply write_hider_status, read_hider_status);

    pub fn worker_reply_answers(self) -> Option<u32> {
        let this = self.this();
        unsafe {
            seqlock_read_with(addr_of!((*this).worker_reply_sequence), 1, || {
                addr_of!((*this).worker_reply.sequence).read_volatile()
            })
            .map(|(_, answers)| answers)
        }
    }

    pub fn read_worker_reply(self, sequence: u32, data: &mut Vec<u8>) -> Option<i32> {
        let this = self.this();
        unsafe {
            seqlock_read_with(addr_of!((*this).worker_reply_sequence), 1, || {
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
                core::ptr::copy_nonoverlapping(
                    addr_of!((*reply).data) as *const u8,
                    data.as_mut_ptr(),
                    length,
                );
                data.set_len(length);
                Some(status)
            })
            .and_then(|(_, status)| status)
        }
    }

    pub fn read_cache_reply_into(self, out: &mut CacheReply, attempts: usize) -> bool {
        let this = self.this();
        unsafe {
            let reply = addr_of!((*this).cache_reply);
            seqlock_read_with(addr_of!((*this).cache_reply_sequence), attempts, || {
                core::ptr::copy_nonoverlapping(
                    reply as *const u8,
                    out as *mut CacheReply as *mut u8,
                    core::mem::offset_of!(CacheReply, data),
                );
                let used = (out.used as usize).min(CACHE_DATA_SIZE);
                core::ptr::copy_nonoverlapping(
                    addr_of!((*reply).data) as *const u8,
                    out.data.as_mut_ptr(),
                    used,
                );
            })
            .is_some()
        }
    }
}

#[derive(Clone, Copy)]
pub struct LayoutView<'a> {
    ptr: NonNull<LayoutMemory>,
    view: PhantomData<&'a LayoutMemory>,
}

impl<'a> LayoutView<'a> {
    pub unsafe fn attach(ptr: NonNull<LayoutMemory>) -> Self {
        Self {
            ptr,
            view: PhantomData,
        }
    }

    pub fn read(self, attempts: usize) -> Result<Layout, ReadError> {
        let this = self.ptr.as_ptr();
        unsafe {
            let protocol = addr_of!((*this).protocol).read_volatile();
            if protocol != PROTOCOL_VERSION as u32 {
                return Err(ReadError::Incompatible(protocol));
            }
            seqlock_read(
                addr_of!((*this).sequence),
                addr_of!((*this).layout),
                attempts,
            )
            .map(|(_, layout)| layout)
            .ok_or(ReadError::Busy)
        }
    }
}

pub(crate) unsafe fn seqlock_write_with<T>(
    sequence: *mut u32,
    data: *mut T,
    fill: impl FnOnce(*mut T),
) -> u32 {
    let sequence = AtomicU32::from_ptr(sequence);
    let start = sequence.load(Ordering::Relaxed) & !1;
    sequence.store(start.wrapping_add(1), Ordering::Relaxed);
    fence(Ordering::Release);
    fill(data);
    sequence.store(start.wrapping_add(2), Ordering::Release);
    start.wrapping_add(2)
}

pub(crate) unsafe fn seqlock_write<T: Copy>(sequence: *mut u32, data: *mut T, value: &T) -> u32 {
    seqlock_write_with(sequence, data, |data| data.write_volatile(*value))
}

pub(crate) unsafe fn seqlock_read_with<R>(
    sequence: *const u32,
    attempts: usize,
    mut read: impl FnMut() -> R,
) -> Option<(u32, R)> {
    let sequence = AtomicU32::from_ptr(sequence as *mut u32);
    for _ in 0..attempts {
        let before = sequence.load(Ordering::Acquire);
        if before % 2 == 1 {
            core::hint::spin_loop();
            continue;
        }
        let value = read();
        fence(Ordering::Acquire);
        if sequence.load(Ordering::Relaxed) == before {
            return Some((before, value));
        }
    }
    None
}

pub(crate) unsafe fn seqlock_read<T: Copy>(
    sequence: *const u32,
    data: *const T,
    attempts: usize,
) -> Option<(u32, T)> {
    seqlock_read_with(sequence, attempts, || data.read_volatile())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use core::mem::{offset_of, size_of, MaybeUninit};

    fn memory() -> Box<MaybeUninit<LayoutMemory>> {
        Box::new_uninit()
    }

    #[test]
    fn layout() {
        assert_eq!(offset_of!(LayoutMemory, protocol), 0);
        assert_eq!(offset_of!(LayoutMemory, sequence), 4);
        assert_eq!(offset_of!(LayoutMemory, layout), 8);
        assert_eq!(size_of::<PlayerLayout>(), 6 * 4);
        assert_eq!(size_of::<SkillsLayout>(), 4 * 4);
        assert_eq!(size_of::<LoginLayout>(), 8 * 4);
        assert_eq!(size_of::<InterfaceLayout>(), 8 + 22 * 4);
        assert_eq!(size_of::<WorldLayout>(), 128);
        assert_eq!(size_of::<WorldSelectLayout>(), 26 * 4);
        assert_eq!(offset_of!(Layout, interfaces), 16 + 16 + 24 + 16 + 32);
        assert_eq!(size_of::<NameRequest>(), 8 + 48);
        assert_eq!(size_of::<NameStatus>(), 8 + 24);
        assert_eq!(size_of::<TotalXpRequest>(), 16);
        assert_eq!(size_of::<TotalXpStatus>(), 8);
        assert_eq!(
            size_of::<CameraLayout>() + size_of::<MinimapLayout>(),
            7 * 4
        );
        assert_eq!(size_of::<ProjectionLayout>(), 24 * 4);
        assert_eq!(size_of::<GroundItemsLayout>(), 9 * 4);
        assert_eq!(size_of::<ChatLayout>(), 10 * 4);
        assert_eq!(size_of::<ProjectileLayout>(), 24 * 4);
        assert_eq!(size_of::<SpotAnimLayout>(), 8 * 4);
        assert_eq!(size_of::<PathLayout>(), 4 * 4);
        assert_eq!(size_of::<MenuStateLayout>(), 33 * 4);
        assert_eq!(
            offset_of!(LayoutMemory, request_sequence),
            8 + 104 + 96 + 128 + 104 + 28 + 96 + 36 + 40 + 8 + 4 + 96 + 32 + 16 + 132
        );
        let entry = MENU_OPTION_SIZE + MENU_TARGET_SIZE + 28;
        assert_eq!(size_of::<MenuEntryInfo>(), entry);
        assert_eq!(size_of::<CacheRequest>(), 8 + CACHE_BATCH * 8);
        assert_eq!(
            size_of::<CacheReply>(),
            16 + CACHE_BATCH * 8 + CACHE_DATA_SIZE
        );
        assert_eq!(size_of::<WorkerRequest>(), 24);
        assert_eq!(size_of::<WorkerReply>(), 16 + WORKER_DATA_SIZE);
        assert_eq!(
            size_of::<ActRequest>(),
            8 + size_of::<ModelRequest>()
                + 24
                + ACT_OPTIONS * MENU_OPTION_SIZE
                + 16
                + 16
                + 4
                + ACT_BLOCKERS * 16
        );
        assert_eq!(size_of::<ActState>(), 40 + entry);
        assert_eq!(size_of::<HiderRequest>(), 8);
        assert_eq!(size_of::<HiderStatus>(), 16);
        assert_eq!(
            offset_of!(LayoutMemory, hider_request),
            offset_of!(LayoutMemory, act_state) + size_of::<ActState>() + 8
        );
    }

    #[test]
    fn hider_requests_and_status_round_trip() {
        let mut memory = memory();
        let ptr = memory.as_mut_ptr();
        unsafe {
            let shared = Shared::create(NonNull::new(ptr).unwrap());
            assert_eq!(
                shared.read_hider_request(),
                Some((0, HiderRequest::default()))
            );
            let request = HiderRequest {
                mask: hide::OTHERS | hide::WALLS,
            };
            assert_eq!(shared.write_hider_request(&request), 2);
            assert_eq!(shared.read_hider_request(), Some((2, request)));
            let status = HiderStatus {
                sequence: 2,
                status: hider_status::ON,
                mask: hide::OTHERS,
            };
            shared.write_hider_status(&status);
            assert_eq!(shared.read_hider_status(1), Some(status));
        }
    }

    #[test]
    fn write_then_read() {
        let mut memory = memory();
        let ptr = memory.as_mut_ptr();
        unsafe {
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
        let mut memory = memory();
        let ptr = memory.as_mut_ptr();
        unsafe {
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
    fn name_requests_and_status_round_trip() {
        let mut memory = memory();
        let ptr = memory.as_mut_ptr();
        unsafe {
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
        let mut memory = memory();
        let ptr = memory.as_mut_ptr();
        unsafe {
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
}
