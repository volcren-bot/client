--!strict

export type PositionData = {
	local_x: number,
	local_y: number,
	x: number,
	y: number,
	plane: number?,
}

export type Skill = {
	name: string,
	level: number,
	real_level: number,
	xp: number,
}

export type Counter = {
	id: string,
	label: string,
	value: number,
	hourly: boolean?,
	per: string?,
	percent_of: string?,
}

export type PlannedBreak = {
	remaining: number,
	length: number,
	at: number?,
	index: number?,
}

export type ActiveBreak = {
	kind: "break" | "sleep",
	remaining: number,
}

export type BreakPlan = {
	breaks: { PlannedBreak },
	sleeps: { PlannedBreak },
	active: ActiveBreak?,
}

export type BreakConfig = {
	interval: number,
	length: number,
	randomness: number?,
	logout_chance: number?,
}

export type SleepConfig = {
	time: string?,
	length: number?,
	randomness: number?,
	logout_chance: number?,
}

export type AntibanConfig = {
	version: number,
	breaks_enabled: boolean,
	sleeps_enabled: boolean,
	breaks: { BreakConfig },
	sleeps: { SleepConfig },
}

export type TransportSkill = { name: string, level: number }

export type TransportClause = { id: number, op: string, value: number }

export type TransportTile = { x: number, y: number, plane: number }

export type Transport = {
	kind: string,
	origin: TransportTile?,
	destination: TransportTile?,
	name: string,
	duration: number,
	wilderness: number?,
	skills: { TransportSkill },
	quests: { string },
	varbits: { TransportClause },
	varps: { TransportClause },
	items: string,
	consumable: boolean,
	menu: string,
	note: string?,
}

export type TransportItemName = { name: string, ids: { number } }

export type TransportsData = {
	version: number,
	source: string,
	transports: { Transport },
	items: { TransportItemName },
}

export type LoginLayout = {
	screen: number,
	panel_x: number,
	box_x: number,
	center_x: number,
	y: number,
	scale: number,
	steam: boolean?,
}

export type CameraState = {
	x: number,
	y: number,
	z: number,
	pitch: number,
	yaw: number,
	focal_x: number,
	focal_y: number,
	focal_z: number,
	zoom: number?,
}

export type MinimapState = {
	zoom: number,
}

export type ClientState = {
	position: PositionData?,
	player_name: string?,
	game_state: number?,
	skills: { Skill }?,
	tick_count: number?,
	login: LoginLayout?,
	root_interface: number?,
	ui_scale: number?,
	run_energy: number?,
	weight: number?,
	camera: CameraState?,
	minimap: MinimapState?,
	destination: Tile?,
}

export type World = {
	id: number,
	types: number,
	players: number,
	location: number,
	activity: string,
}

export type PanelRect = {
	x: number,
	y: number,
	width: number,
	height: number,
}

export type WorldPanel = {
	page: number,
	compact: boolean,
	selected: number,
	hovered: number,
	rows: number,
	columns: number,
	list: PanelRect,
	row_offset: number,
	row_height: number,
	previous: PanelRect,
	next: PanelRect,
	shown: { World },
}

export type WorldSelect = {
	open: boolean,
	current: number,
	worlds: { World },
	panel: WorldPanel?,
}

export type Connection = {
	version: string,
	protocol: number,
	width: number,
	height: number,
	mouse_x: number,
	mouse_y: number,
	offsets_found: boolean?,
}

export type ScreenPoint = {
	x: number,
	y: number,
}

export type FinePoint = {
	x: number,
	y: number,
	z: number?,
	tile: { x: number, y: number }?,
}

export type CameraView = { yaw: number?, zoom: number? }

export type LayoutItem = {
	id: number,
	ids: { number },
	name: string,
	stackable: boolean,
	min: number,
	max: number,
	doses: { number }?,
	locked: boolean,
}

export type EquipmentLayoutSlot = {
	slot: number,
	mode: "item" | "empty" | "any",
	id: number?,
	ids: { number }?,
	name: string?,
	stackable: boolean?,
	min: number?,
	max: number?,
	doses: { number }?,
	locked: boolean?,
}

export type EquipmentSpell = {
	key: string,
	name: string,
	book: string,
	level: number,
	autocast: boolean,
}

export type EquipmentLayout = {
	name: string,
	slots: { EquipmentLayoutSlot },
	spell: EquipmentSpell?,
}

export type InventoryLayoutSlot = {
	slot: number,
	id: number,
	ids: { number },
	name: string,
	stackable: boolean,
	min: number,
	max: number,
	doses: { number }?,
	locked: boolean,
}

export type InventoryLayout = {
	name: string,
	slots: { InventoryLayoutSlot },
	pouch: { LayoutItem }?,
}

export type PrayerLayout = {
	name: string,
	prayers: { string },
	locked: { string }?,
}

export type LayoutRequirement = {
	ids: { number },
	slot: number?,
	min: number?,
	max: number?,
	doses: { number }?,
}

export type GridColumn = {
	id: string,
	label: string?,
	summary: string?,
	color: string?,
	max: number?,
}

export type TaskType = {
	id: string,
	label: string?,
	fields: { FormField }?,
}

export type VisibleWhen = { id: string, equals: any } | { VisibleWhen } | { any: { VisibleWhen } }

export type FormField = {
	id: string?,
	kind: (
		"label"
		| "check"
		| "number"
		| "text"
		| "choice"
		| "list"
		| "grid"
		| "tasks"
		| "equipment"
		| "inventory"
		| "prayer"
	)?,
	label: string?,
	text: string?,
	default: any?,
	options: { string }?,
	min: number?,
	max: number?,
	step: number?,
	integer: boolean?,
	items: { { id: string, label: string?, detail: string?, default: (boolean | string)? } }?,
	columns: { GridColumn }?,
	types: { TaskType }?,
	colors: boolean?,
	search: string?,
	note: string?,
	enabled_by: string?,
	disabled_by: string?,
	row: string?,
	box_first: boolean?,
	heading: number?,
	required: { LayoutRequirement }?,
	locked: { string }?,
	visible_when: VisibleWhen?,
}

export type FormSpec = {
	title: string?,
	submit: string?,
	columns: boolean?,
	row_gap: number?,
	heading_gap: number?,
	fields: { FormField },
}

export type MenuEntry = {
	option: string,
	target: string,
	raw_target: string,
	id: number,
	index: number,
	action: number,
	priority: number,
	x: number,
	y: number,
	kind: number,
	submenu: boolean,
}

export type EntrySwap = {
	target: string,
	options: { string },
}

export type EntrySwaps = {
	swaps: { EntrySwap },
	alt: { EntrySwap },
}

export type MenuClick = {
	entry: MenuEntry,
	x: number,
	y: number,
	age: number,
}

export type MenuRect = {
	x: number,
	y: number,
	width: number,
	height: number,
}

export type MenuLayout = {
	x: number,
	y: number,
	width: number,
	height: number,
	rows: MenuRect,
	header_height: number,
	row_height: number,
	scroll: number,
	margin: number,
	submenus: { MenuSubmenu },
}

export type MenuSubmenu = {
	row: number,
	entries: { MenuEntry },
	x: number,
	y: number,
	width: number,
	height: number,
	rows: MenuRect,
	header_height: number,
	row_height: number,
	scroll: number,
	margin: number,
}

export type SelectedWidget = {
	verb: string,
	name: string,
	raw_name: string,
	id: number,
	group: number,
	child: number,
	index: number,
	item: number,
	targets: number,
}

export type CacheValue = number | string

export type EnumDef = {
	key_type: string,
	value_type: string,
	default: CacheValue?,
	entries: { { key: number, value: CacheValue } },
}

export type SubInterface = {
	group: number,
	parent: number,
	parent_group: number,
	parent_child: number,
}

export type WorldRoute = {
	known: boolean,
	reachable: boolean,
	from: NavTile?,
	to: NavTile?,
}

export type Widget = {
	id: number,
	group: number,
	child: number,
	kind: number,
	x: number,
	y: number,
	width: number,
	height: number,
	parent: number,
	sprite: number,
	index: number,
	hidden: boolean,
	text: string?,
	text_color: number,
	scroll_x: number,
	scroll_y: number,
	scroll_width: number,
	scroll_height: number,
	action: string?,
	item_id: number,
	item_quantity: number,
}

export type ItemSlot = {
	slot: number,
	id: number,
	quantity: number,
}

export type ItemChange = {
	sequence: number,
	time_ms: number,
	tick: number?,
	container: number,
	slot: number,
	old_known: boolean,
	old_item: number,
	old_quantity: number,
	new_item: number,
	new_quantity: number,
}

export type PacketName = "TICK" | "VARP_SMALL" | "VARP_LARGE" | "SYNTH_SOUND" | "MESSAGE_GAME"

export type Packet = {
	sequence: number,
	time_ms: number,
	sent: boolean,
	opcode: number,
	length: number,
	payload: string,
	name: PacketName?,
	fields: { [string]: number | string }?,
}

export type AccountSettings = {
	off: { string },
	house: { string },
}

export type ServerTile = {
	x: number,
	y: number,
	plane: number?,
	steps: number,
}

export type MovementState = {
	x: number,
	y: number,
	plane: number?,
	from_x: number,
	from_y: number,
	from_plane: number?,
	moving: boolean,
	moved_tick: number?,
	tick: number,
}

export type Npc = {
	index: number,
	id: number,
	name: string,
	x: number,
	y: number,
	local_x: number,
	local_y: number,
	animation: number,
	interacting_npc: number,
	interacting_player: number,
	head_icons: { HeadIcon },
}

export type HeadIcon = {
	archive: number,
	sprite: number,
}

export type Hitsplat = {
	slot: number,
	kind: number,
	damage: number,
	cycle: number,
}

export type NpcModel = {
	bounding_box: boolean,
	orientation: number,
	vertex_count: number,
	corners: { ScreenPoint },
	hull: { ScreenPoint },
	faces: { { ScreenPoint } },
	frame_count: number,
}

export type Loc = {
	id: number,
	name: string?,
	kind: number,
	x: number,
	y: number,
	plane: number,
	scene_x: number,
	scene_y: number,
	size_x: number,
	size_y: number,
	shape: number?,
	rotation: number?,
	clickable: boolean,
}

export type LocView = {
	name: string,
	actions: { string },
}

export type CacheLoc = {
	id: number,
	name: string,
	x: number,
	y: number,
	plane: number,
	size_x: number,
	size_y: number,
	shape: number,
	rotation: number,
	actions: { string },
}

export type CacheLocFilter = {
	plane: number?,
	names: { string }?,
	actions: { string }?,
}

export type GroundItem = {
	id: number,
	quantity: number,
	name: string?,
	x: number,
	y: number,
	plane: number,
	scene_x: number,
	scene_y: number,
	public_tick: number?,
	despawn_tick: number?,
	ownership: number?,
	never_public: boolean?,
	op_flags: number?,
}

export type ChatMessage = {
	uid: number,
	kind: number,
	name: string,
	sender: string,
	text: string,
	cycle: number,
	age: number,
	time: number,
	time_utc: string,
	time_local: string,
}

export type PlayerInfo = {
	index: number,
	name: string,
	x: number,
	y: number,
	local_x: number,
	local_y: number,
	animation: number,
	pose: number,
	graphic: number,
	target_state: number,
	target_npc: number,
	target_x: number,
	target_y: number,
	interacting_npc: number,
	interacting_player: number,
	movement: { number },
}

export type Projectile = {
	id: number,
	source_x: number,
	source_y: number,
	source_plane: number,
	target_x: number,
	target_y: number,
	target_plane: number,
	source_npc: number,
	source_player: number,
	target_npc: number,
	target_player: number,
	start_height: number,
	end_height: number,
	slope: number,
	start_pos: number,
	start_cycle: number,
	end_cycle: number,
	cycle: number,
	moving: boolean,
	x: number,
	y: number,
	local_x: number,
	local_y: number,
	z: number,
	orientation: number,
	height: number,
}

export type SpotAnim = {
	id: number,
	x: number,
	y: number,
	plane: number,
	view_id: number,
	local_x: number,
	local_y: number,
	z: number,
	start_cycle: number,
	cycle: number,
}

export type WindMouseOptions = {
	speed: number?,
	wind: number?,
	gravity: number?,
	timeout: number?,
	pace: number?,
}

export type ActModel = {
	target: "loc" | "npc" | "player",
	kind: number?,
	scene_x: number?,
	scene_y: number?,
	id: number?,
	index: number?,
}

export type ActRect = { x1: number, y1: number, x2: number, y2: number }

export type ActSpec = {
	model: ActModel,
	entry: { kind: number?, id: number?, index: number?, x: number?, y: number? },
	options: { string },
	right: boolean?,
	near: number?,
	spread: number?,
	min_area: number?,
	timeout: number?,
	viewport: ActRect,
	blockers: { ActRect }?,
	mouse: WindMouseOptions?,
	hold: number?,
}

export type ActStatus =
	"left"
	| "right"
	| "no_option"
	| "gone"
	| "menu_open"
	| "timeout"
	| "cancelled"
	| "lost"
	| "other_thread"
	| "not_in_game"
	| "not_found"

export type ActResult = { status: ActStatus, x: number, y: number, entry: MenuEntry? }

export type Tile = {
	x: number,
	y: number,
}

export type SceneRoute = {
	tiles: { Tile },
	reached: boolean,
}

export type ReachTarget = {
	kind: "tile" | "npc" | "loc",
	x: number,
	y: number,
	size: number?,
	size_x: number?,
	size_y: number?,
	shape: number?,
	rotation: number?,
}

export type Collision = {
	x: number,
	y: number,
	width: number,
	height: number,
	plane: number,
	flags: { number },
}

export type NavTile = {
	x: number,
	y: number,
	plane: number,
}

export type NavLoc = {
	id: number,
	x: number,
	y: number,
	plane: number,
	kind: number,
	size_x: number,
	size_y: number,
	option: string,
}

export type NavAction = {
	at: number,
	kind: "door" | "climb",
	loc: NavLoc,
	to_plane: number,
}

export type NavPlan = {
	tiles: { NavTile },
	actions: { NavAction },
	reached: boolean,
	goal: NavTile,
}

export type NavOptions = {
	near: number?,
	door_options: { string }?,
	climbs: boolean?,
	allow_climbs: { string }?,
	shortcut_saving: number?,
	avoid: { NavLoc }?,
	seed: number?,
	rnd: number?,
	door_cost: number?,
	climb_cost: number?,
}

export type SceneRouteOptions = {
	seed: number?,
	rnd: number?,
	spacing: number?,
	jitter: number?,
	max_detour: number?,
	offset: number?,
}

export type Area = {
	x: number,
	y: number,
	width: number,
	height: number,
}

export type Corners = {
	x1: number,
	y1: number,
	x2: number,
	y2: number,
}

export type Layer = "world" | "ui"

export type Shape =
	{
		kind: "rect" | "line",
		x1: number,
		y1: number,
		x2: number,
		y2: number,
		r: number,
		g: number,
		b: number,
		a: number?,
		layer: Layer?,
	}
	| { kind: "clear" }

export type DebugShape =
	{
		kind: "path",
		points: { FinePoint },
		r: number,
		g: number,
		b: number,
		a: number?,
		width: number?,
		closed: boolean?,
	}
	| {
		kind: "label",
		at: FinePoint,
		text: string,
		r: number,
		g: number,
		b: number,
		a: number?,
		bold: boolean?,
	}
	| {
		kind: "loc",
		loc_kind: number,
		x: number,
		y: number,
		id: number,
		faces: { r: number, g: number, b: number, a: number? }?,
		hull: { r: number, g: number, b: number, a: number? }?,
	}
	| {
		kind: "screen_label",
		x: number,
		y: number,
		text: string,
		r: number,
		g: number,
		b: number,
		a: number?,
		bold: boolean?,
	}

export type HttpRequest = {
	url: string,
	method: string?,
	headers: { [string]: string }?,
	body: string?,
	timeout: number?,
}

export type HttpResponse = {
	ok: boolean,
	status: number,
	headers: { [string]: string },
	body: string,
}

export type ItemPrice = {
	id: number,
	name: string,
	members: boolean,
	value: number?,
	high_alch: number?,
	low_alch: number?,
	limit: number?,
	high: number?,
	high_time: number?,
	low: number?,
	low_time: number?,
}

local g = _G :: any

return {
	game_state = g.game_state :: () -> ClientState,
	connection = g.connection :: () -> Connection,
	widget = g.widget :: (group: number, child: number) -> Widget?,
	widget_model = g.widget_model :: (group: number, child: number) -> { kind: number, id: number }?,
	widget_children = g.widget_children :: (group: number, child: number) -> { Widget }?,
	item_container = g.item_container :: (id: number) -> { ItemSlot }?,
	on_item_change = g.on_item_change :: ((change: ItemChange) -> ()) -> number,
	off_item_change = g.off_item_change :: (id: number) -> boolean,
	wait_item_change = g.wait_item_change :: (ms: number) -> boolean,
	on_packet = g.on_packet :: (
		packets: { number | PacketName },
		callback: (packet: Packet) -> (),
		sent: boolean?
	) -> number,
	off_packet = g.off_packet :: (id: number) -> boolean,
	wait_packet = g.wait_packet :: (packets: { number | PacketName }, ms: number, sent: boolean?) -> Packet?,
	packet_opcode = g.packet_opcode :: (name: PacketName) -> number?,
	project = g.project :: (
		points: { FinePoint },
		shift: { x: number, y: number, plane: number? }?,
		view: CameraView?
	) -> { ScreenPoint }?,
	settings = g.settings :: () -> { [string]: any },
	finish_requested = g.finish_requested :: () -> boolean,
	script_debugging = g.script_debugging :: () -> boolean,
	form_update = g.form_update :: () -> { [string]: any }?,
	menu = g.menu :: () -> { MenuEntry }?,
	menu_click = g.menu_click :: () -> MenuClick?,
	entry_swaps = g.entry_swaps :: () -> EntrySwaps?,
	menu_open = g.menu_open :: () -> boolean,
	menu_layout = g.menu_layout :: () -> MenuLayout?,
	sub_interfaces = g.sub_interfaces :: () -> { SubInterface }?,
	game_val_name = g.game_val_name :: (kind: number, id: number) -> string?,
	game_val_names = g.game_val_names :: (kind: number, ids: { number }) -> { string? }?,
	item_name = g.item_name :: (id: number) -> string?,
	item_names = g.item_names :: (ids: { number }) -> { string? }?,
	varc = g.varc :: (id: number) -> number?,
	varp = g.varp :: (id: number) -> number?,
	varps = g.varps :: () -> { number }?,
	varbit = g.varbit :: (id: number) -> number?,
	enum_def = g.enum_def :: (id: number) -> EnumDef?,
	landmasses = g.landmasses :: (points: { { x: number, y: number, plane: number } }) -> ({ number }?, string?),
	cache_files = g.cache_files :: (index: number, files: { { number } }) -> ({ string | boolean }?, string?),
	cache_file = g.cache_file :: (index: number, archive: number, file: number) -> (string?, string?),
	item_params = g.item_params :: (ids: { number }) -> { { [number]: CacheValue }? }?,
	selected_widget = g.selected_widget :: () -> SelectedWidget?,
	npcs = g.npcs :: () -> { Npc }?,
	npc_model = g.npc_model :: (index: number) -> NpcModel?,
	npc_type_model = g.npc_type_model :: (
		id: number,
		animation: number,
		frame: number,
		fine_x: number,
		fine_y: number,
		orientation: number
	) -> NpcModel?,
	npc_sizes = g.npc_sizes :: (indices: { number }) -> { [number]: number }?,
	npc_actions = g.npc_actions :: (ids: { number }) -> { [number]: { string } }?,
	npc_hitsplats = g.npc_hitsplats :: (indices: { number }) -> { [number]: { Hitsplat } }?,
	player_hitsplats = g.player_hitsplats :: (indices: { number }) -> { [number]: { Hitsplat } }?,
	npc_overhead_text = g.npc_overhead_text :: (indices: { number }) -> { [number]: string }?,
	player_overhead_text = g.player_overhead_text :: (indices: { number }) -> { [number]: string }?,
	client_cycle = g.client_cycle :: () -> number?,
	item_stackable = g.item_stackable :: (ids: { number }) -> { boolean? }?,
	item_placeholder = g.item_placeholder :: (ids: { number }) -> { boolean? }?,
	locs = g.locs :: (plane: number?) -> { Loc }?,
	loc_shown = g.loc_shown :: (ids: { number }) -> { LocView }?,
	cache_locs = g.cache_locs :: (area: Corners, filter: CacheLocFilter?) -> { CacheLoc }?,
	ground_items = g.ground_items :: () -> { GroundItem }?,
	projectiles = g.projectiles :: () -> { Projectile }?,
	spot_anims = g.spot_anims :: () -> { SpotAnim }?,
	map_regions = g.map_regions :: () -> { { number } }?,
	in_instance = g.in_instance :: () -> boolean?,
	server_tile = g.server_tile :: () -> ServerTile?,
	movement = g.movement :: () -> MovementState?,
	chat_messages = g.chat_messages :: () -> { ChatMessage }?,
	find_path = g.find_path :: (from: Tile, to: Tile, plane: number?, max_tiles: number?) -> { Tile }?,
	collision = g.collision :: (area: Corners?) -> (Collision?, string?),
	scene_route = g.scene_route :: (from: Tile, to: Tile, options: SceneRouteOptions?) -> SceneRoute?,
	reach_route = g.reach_route :: (from: Tile, to: ReachTarget) -> (SceneRoute?, string?),
	reach_tiles = g.reach_tiles :: (to: ReachTarget) -> ({ Tile }?, string?),
	world_route = g.world_route :: (from: NavTile, to: NavTile, allow_climbs: { string }?) -> WorldRoute?,
	nav_plan = g.nav_plan :: (from: Tile | NavTile, to: Tile | NavTile, options: NavOptions?) -> (NavPlan?, string?),
	loc_model = g.loc_model :: (loc: Loc) -> NpcModel?,
	players = g.players :: () -> { PlayerInfo }?,
	player_model = g.player_model :: (index: number) -> NpcModel?,
	world_select = g.world_select :: () -> WorldSelect?,

	click = g.click :: (x: number, y: number, button: string?, hold_ms: number?) -> (),
	move = g.move :: (x: number, y: number) -> (),
	wind_mouse = g.wind_mouse :: (x: number, y: number, options: WindMouseOptions?) -> (),
	mouse_down = g.mouse_down :: (button: string?) -> (),
	mouse_up = g.mouse_up :: (button: string?) -> (),
	scroll = g.scroll :: (notches: number) -> (),
	key_down = g.key_down :: (key: number) -> (),
	key_up = g.key_up :: (key: number) -> (),
	type_text = g.type_text :: (text: string, hold_ms: number?) -> (),
	click_async = g.click_async :: (x: number, y: number, button: string?, hold_ms: number?) -> (),
	move_async = g.move_async :: (x: number, y: number) -> (),
	wind_mouse_async = g.wind_mouse_async :: (x: number, y: number, options: WindMouseOptions?) -> (),
	type_text_async = g.type_text_async :: (text: string, hold_ms: number?) -> (),
	mouse_down_async = g.mouse_down_async :: (button: string?) -> (),
	mouse_up_async = g.mouse_up_async :: (button: string?) -> (),
	scroll_async = g.scroll_async :: (notches: number) -> (),
	key_down_async = g.key_down_async :: (key: number) -> (),
	key_up_async = g.key_up_async :: (key: number) -> (),
	sleep_async = g.sleep_async :: (ms: number) -> (),
	input_busy = g.input_busy :: () -> boolean,
	input_wait = g.input_wait :: () -> string?,
	input_error = g.input_error :: () -> string?,
	input_cancel = g.input_cancel :: () -> (),
	act = g.act :: (spec: ActSpec) -> ActResult,
	bank_pin_enter = g.bank_pin_enter :: () -> string,
	account_settings = g.account_settings :: () -> (AccountSettings?, string?),
	lamp_skill = g.lamp_skill :: () -> (string?, string?),
	http_request = g.http_request :: (options: HttpRequest) -> (HttpResponse?, string?),
	item_prices = g.item_prices :: (items: { number | string }) -> ({ ItemPrice? }?, string?),
	draw_rect = g.draw_rect :: (
		x1: number,
		y1: number,
		x2: number,
		y2: number,
		r: number,
		g: number,
		b: number,
		a: number?,
		layer: Layer?
	) -> (),
	draw_line = g.draw_line :: (
		x1: number,
		y1: number,
		x2: number,
		y2: number,
		r: number,
		g: number,
		b: number,
		a: number?,
		layer: Layer?
	) -> (),
	draw_lines = g.draw_lines :: (
		lines: { { number } },
		r: number,
		g: number,
		b: number,
		a: number?,
		layer: Layer?
	) -> (),
	clear_overlay = g.clear_overlay :: () -> (),
	draw_batch = g.draw_batch :: (shapes: { Shape }) -> (),
	debug_shapes = g.debug_shapes :: (shapes: { DebugShape }) -> (),
	sleep = g.sleep :: (ms: number) -> (),
	time = g.time :: () -> number,
	clock = g.clock :: () -> number,
	unix_time = g.unix_time :: () -> number,
	antiban_skips = g.antiban_skips :: () -> { { kind: string, index: number } },
	antiban_config = g.antiban_config :: () -> AntibanConfig,
	antiban_resume = g.antiban_resume :: () -> BreakPlan?,
	transports = g.transports :: () -> TransportsData,
	murmur2a = g.murmur2a :: (text: string, seed: number?) -> number,
	biometrics_profile = g.biometrics_profile :: (name: string) -> { number },
	report_counters = g.report_counters :: (counters: { Counter }) -> (),
	report_warning = g.report_warning :: (text: string) -> (),
	report_breaks = g.report_breaks :: (plan: BreakPlan) -> (),
	report_exit = g.report_exit :: (status: "done" | "blocked", reason: string?) -> (),
	report_stats = g.report_stats :: (experience: number, gold: number, custom: { number }?) -> (),
}
