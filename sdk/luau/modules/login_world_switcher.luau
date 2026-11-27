--!strict
local Host = require("@volcren/host")
local game_state = Host.game_state
local world_select = Host.world_select
type World = Host.World
type WorldSelect = Host.WorldSelect
type WorldPanel = Host.WorldPanel
type PanelRect = Host.PanelRect
local GameState = require("@volcren/game_state")
local Geometry = require("@volcren/geometry")
local Keyboard = require("@volcren/keyboard")
local Log = require("@volcren/log")
local Mouse = require("@volcren/mouse")
local Random = require("@volcren/random")
local Widgets = require("@volcren/widgets")
local Box = Geometry.Box
type Box = Geometry.Box

local LoginWorldSwitcher = {}

LoginWorldSwitcher.MEMBERS = 0x1
LoginWorldSwitcher.PVP = 0x4
LoginWorldSwitcher.BOUNTY = 0x20
LoginWorldSwitcher.PVP_ARENA = 0x40
LoginWorldSwitcher.SKILL_TOTAL = 0x80
LoginWorldSwitcher.QUEST_SPEEDRUNNING = 0x100
LoginWorldSwitcher.HIGH_RISK = 0x400
LoginWorldSwitcher.HARDCORE_WORLD = 0x800
LoginWorldSwitcher.LAST_MAN_STANDING = 0x4000
LoginWorldSwitcher.BETA_WORLD = 0x10000
LoginWorldSwitcher.LEGACY_ONLY = 0x400000
LoginWorldSwitcher.EOC_ONLY = 0x800000
LoginWorldSwitcher.NOSAVE_MODE = 0x2000000
LoginWorldSwitcher.TOURNAMENT_WORLD = 0x4000000
LoginWorldSwitcher.FRESH_START_WORLD = 0x8000000
LoginWorldSwitcher.DEADMAN = 0x20000000
LoginWorldSwitcher.SEASONAL = 0x40000000

LoginWorldSwitcher.DANGEROUS = bit32.bor(
	LoginWorldSwitcher.PVP,
	LoginWorldSwitcher.BOUNTY,
	LoginWorldSwitcher.PVP_ARENA,
	LoginWorldSwitcher.QUEST_SPEEDRUNNING,
	LoginWorldSwitcher.HIGH_RISK,
	LoginWorldSwitcher.HARDCORE_WORLD,
	LoginWorldSwitcher.LAST_MAN_STANDING,
	LoginWorldSwitcher.BETA_WORLD,
	LoginWorldSwitcher.NOSAVE_MODE,
	LoginWorldSwitcher.TOURNAMENT_WORLD,
	LoginWorldSwitcher.FRESH_START_WORLD,
	LoginWorldSwitcher.DEADMAN,
	LoginWorldSwitcher.SEASONAL
)

LoginWorldSwitcher.allow_dangerous_worlds = false

LoginWorldSwitcher.OFFLINE = -1
LoginWorldSwitcher.FULL = 1980

export type WorldBox = {
	index: number,
	slot: number,
	world: World,
	box: Box,
}

export type Buttons = {
	world: Box,
	previous: Box?,
	next: Box?,
}

type Screen = {
	select: WorldSelect,
	panel_x: number,
	scale: number,
}

local TITLE_HEIGHT = 671
local WORLD_BUTTON = { x1 = 20, y1 = -55, x2 = 120, y2 = -20 }

local OPEN_TIMEOUT_MS = 5000
local CLOSE_TIMEOUT_MS = 1500
local PAGE_TIMEOUT_MS = 1500
local HOVER_TIMEOUT_MS = 600
local SWITCH_TIMEOUT_MS = 1500
local SWITCH_ATTEMPTS = 5
local MAX_PAGE_STEPS = 32

local function screen(): Screen?
	local state = game_state()
	if state.game_state ~= GameState.LOGIN_SCREEN and state.game_state ~= GameState.LOGIN_SCREEN_AUTHENTICATOR then
		return nil
	end
	local login = state.login
	if login == nil then
		Log.log("LoginWorldSwitcher", "login screen layout unknown")
		return nil
	end
	local select = world_select()
	if select == nil then
		return nil
	end
	return {
		select = select,
		panel_x = login.panel_x,
		scale = login.scale or 1,
	}
end

local function scaled(s: Screen, x: number, y: number, width: number, height: number): Box
	local function px(v: number): number
		return math.floor(v * s.scale)
	end
	return Box.new(px(s.panel_x + x), px(y), px(s.panel_x + x + width) - 1, px(y + height) - 1)
end

local function rect(s: Screen, r: PanelRect): Box
	return scaled(s, r.x, r.y, r.width, r.height)
end

local function per_page(panel: WorldPanel): number
	return math.max(1, panel.rows * panel.columns)
end

local function last_page(panel: WorldPanel): number
	return math.max(0, (#panel.shown - 1) // per_page(panel))
end

function LoginWorldSwitcher.state(): WorldSelect?
	local s = screen()
	if s == nil then
		return nil
	end
	return s.select
end

function LoginWorldSwitcher.is_open(): boolean
	local select = LoginWorldSwitcher.state()
	return select ~= nil and select.open
end

function LoginWorldSwitcher.wait_open(timeout_ms: number?): boolean
	return Widgets.wait_until(function()
		return LoginWorldSwitcher.is_open() == true
	end, timeout_ms or OPEN_TIMEOUT_MS)
end

function LoginWorldSwitcher.current(): number?
	local select = LoginWorldSwitcher.state()
	if select == nil then
		return nil
	end
	return select.current
end

function LoginWorldSwitcher.worlds(): { World }?
	local select = LoginWorldSwitcher.state()
	if select == nil then
		return nil
	end
	return select.worlds
end

function LoginWorldSwitcher.world(id: number): World?
	local worlds = LoginWorldSwitcher.worlds()
	if worlds == nil then
		return nil
	end
	for _, world in worlds do
		if world.id == id then
			return world
		end
	end
	return nil
end

function LoginWorldSwitcher.has_type(world: World, types: number): boolean
	return bit32.band(world.types, types) ~= 0
end

function LoginWorldSwitcher.is_dangerous(world: World): boolean
	return LoginWorldSwitcher.has_type(world, LoginWorldSwitcher.DANGEROUS)
end

function LoginWorldSwitcher.is_available(world: World): boolean
	return world.players ~= LoginWorldSwitcher.OFFLINE and world.players <= LoginWorldSwitcher.FULL
end

local function world_button(s: Screen): Box
	local b = WORLD_BUTTON
	return scaled(s, b.x1, TITLE_HEIGHT + b.y1, b.x2 - b.x1, b.y2 - b.y1)
end

local function master_index(s: Screen, id: number): number?
	for i, world in s.select.worlds do
		if world.id == id then
			return i
		end
	end
	return nil
end

local function boxes(s: Screen): { WorldBox }?
	local panel = s.select.panel
	if not s.select.open or panel == nil then
		return nil
	end
	local size = per_page(panel)
	local column_width = panel.list.width // math.max(1, panel.columns)
	local result: { WorldBox } = {}
	for slot = 0, size - 1 do
		local shown = panel.page * size + slot
		local world = panel.shown[shown + 1]
		if world == nil then
			break
		end
		local column, row = slot // panel.rows, slot % panel.rows
		table.insert(result, {
			index = master_index(s, world.id) or 0,
			slot = slot + 1,
			world = world,
			box = scaled(
				s,
				panel.list.x + column * column_width,
				panel.list.y + panel.row_offset + row * panel.row_height,
				column_width,
				panel.row_height
			),
		})
	end
	return result
end

function LoginWorldSwitcher.buttons(): Buttons?
	local s = screen()
	if s == nil then
		return nil
	end
	local panel = s.select.panel
	local open = s.select.open and panel ~= nil
	return {
		world = world_button(s),
		previous = if open and panel and panel.page > 0 then rect(s, panel.previous) else nil,
		next = if open and panel and panel.page < last_page(panel) then rect(s, panel.next) else nil,
	}
end

function LoginWorldSwitcher.boxes(): { WorldBox }?
	local s = screen()
	if s == nil then
		return nil
	end
	return boxes(s)
end

function LoginWorldSwitcher.read(slot: number): number?
	local shown = LoginWorldSwitcher.boxes()
	if shown == nil then
		return nil
	end
	local b = shown[slot]
	return b and b.world.id
end

function LoginWorldSwitcher.open(): boolean
	local s = screen()
	if s == nil then
		return false
	end
	if s.select.open then
		return true
	end
	Mouse.click(world_button(s))
	if not LoginWorldSwitcher.wait_open() then
		Log.log("LoginWorldSwitcher", `the world switcher didn't open within {OPEN_TIMEOUT_MS} ms`)
		return false
	end
	return true
end

function LoginWorldSwitcher.close(): boolean
	if not LoginWorldSwitcher.is_open() then
		return true
	end
	Keyboard.key_press(Keyboard.keys.ESCAPE)
	local closed = Widgets.wait_until(function()
		return LoginWorldSwitcher.is_open() == false
	end, CLOSE_TIMEOUT_MS)
	if not closed then
		Log.log("LoginWorldSwitcher", `the world switcher didn't close within {CLOSE_TIMEOUT_MS} ms`)
		return false
	end
	return true
end

local function turn(step: number): boolean
	local s = screen()
	if s == nil then
		return false
	end
	local panel = s.select.panel
	if not s.select.open or panel == nil then
		return false
	end
	local from = panel.page
	if (step < 0 and from <= 0) or (step > 0 and from >= last_page(panel)) then
		return false
	end
	Mouse.click(rect(s, if step < 0 then panel.previous else panel.next))
	local moved = Widgets.wait_until(function()
		local select = LoginWorldSwitcher.state()
		return select ~= nil and select.panel ~= nil and select.panel.page ~= from
	end, PAGE_TIMEOUT_MS)
	if not moved then
		Log.log("LoginWorldSwitcher", `the world list didn't turn the page within {PAGE_TIMEOUT_MS} ms`)
		return false
	end
	Random.sleep(400, 1200)
	return true
end

function LoginWorldSwitcher.previous_page(): boolean
	return turn(-1)
end

function LoginWorldSwitcher.next_page(): boolean
	return turn(1)
end

function LoginWorldSwitcher.find(id: number): WorldBox?
	if not LoginWorldSwitcher.open() then
		return nil
	end
	for _ = 1, MAX_PAGE_STEPS do
		local s = screen()
		local panel = s and s.select.panel
		if s == nil or panel == nil then
			return nil
		end
		local position: number? = nil
		for i, world in panel.shown do
			if world.id == id then
				position = i - 1
				break
			end
		end
		if position == nil then
			Log.log("LoginWorldSwitcher", `world {id} isn't in the list: its filters hide it`)
			return nil
		end
		local page = position // per_page(panel)
		if page ~= panel.page then
			if not turn(if page < panel.page then -1 else 1) then
				return nil
			end
		else
			for _, b in boxes(s) or {} do
				if b.world.id == id then
					return b
				end
			end
			return nil
		end
	end
	return nil
end

local function hovering(index: number): boolean
	return Widgets.wait_until(function()
		local select = LoginWorldSwitcher.state()
		local panel = select and select.panel
		return panel ~= nil and panel.hovered == index - 1
	end, HOVER_TIMEOUT_MS)
end

function LoginWorldSwitcher.switch(id: number): boolean
	local current = LoginWorldSwitcher.current()
	if current == nil then
		return false
	end
	if current == id then
		return true
	end

	local found = LoginWorldSwitcher.find(id)
	if found == nil then
		return false
	end
	local world = found.world
	if not LoginWorldSwitcher.allow_dangerous_worlds and LoginWorldSwitcher.is_dangerous(world) then
		Log.log(
			"LoginWorldSwitcher",
			`world {id} is a dangerous world ({world.activity}): set LoginWorldSwitcher.allow_dangerous_worlds if this is intentional`
		)
		return false
	end
	if not LoginWorldSwitcher.is_available(world) then
		return false
	end

	Mouse.move(found.box)
	if not hovering(found.index) then
		return false
	end
	Mouse.click()
	local switched = Widgets.wait_until(function()
		local select = LoginWorldSwitcher.state()
		return select ~= nil and not select.open and select.current == id
	end, SWITCH_TIMEOUT_MS)
	if not switched then
		Log.log("LoginWorldSwitcher", `didn't switch to world {id} within {SWITCH_TIMEOUT_MS} ms`)
		return false
	end
	return true
end

function LoginWorldSwitcher.switch_random(ids: { number }): boolean
	if #ids == 0 then
		return false
	end
	local current = LoginWorldSwitcher.current()
	if current == nil then
		return false
	end
	if table.find(ids, current) then
		return true
	end
	for _ = 1, SWITCH_ATTEMPTS do
		if LoginWorldSwitcher.switch(ids[math.random(#ids)]) then
			return true
		end
	end
	Log.log("LoginWorldSwitcher", `couldn't switch to any of the worlds after {SWITCH_ATTEMPTS} attempts`)
	return false
end

return LoginWorldSwitcher
