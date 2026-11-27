--!strict
local Host = require("@volcren/host")
local widget_children = Host.widget_children
type Widget = Host.Widget
local Item = require("@volcren/item")
type Item = Item.Item
local BankList = require("@volcren/bank_list")
local BankpinKeypad = require("@volcren/bankpin_keypad")
local Biometrics = require("@volcren/biometrics")
local Chat = require("@volcren/chat")
local Clickbox = require("@volcren/clickbox")
local Interact = require("@volcren/interact")
local Inv = require("@volcren/inv")
local Inventory = require("@volcren/inventory")
local Log = require("@volcren/log")
local Minimenu = require("@volcren/minimenu")
local Mouse = require("@volcren/mouse")
local Npcs = require("@volcren/npcs")
local Objects = require("@volcren/objects")
local Random = require("@volcren/random")
local SafeSpot = require("@volcren/safespot")
local Toplevel = require("@volcren/toplevel")
local Walker = require("@volcren/walker")
local Widgets = require("@volcren/widgets")
type Loc = Host.Loc
type Npc = Host.Npc
type Tile = Host.Tile

local CONTAINER = 95
local GROUP_ID = 12
local FRAME_CHILD = 2
local CLOSE_INDEX = 11
local CLOSE_MAX_SIZE = 60
local GRID_CHILD = 12
local SCROLL_NOTCH = 79
local MIN_SHOWN = 0.5

local function grid(): Widget?
	local w = Host.widget(GROUP_ID, GRID_CHILD)
	if w == nil or w.hidden then
		return nil
	end
	return w
end

local function shown_slots(): { [number]: Widget }?
	local g = grid()
	if g == nil then
		return nil
	end
	local kids = widget_children(GROUP_ID, GRID_CHILD)
	if kids == nil then
		return nil
	end
	local top, bottom = g.y, g.y + g.height
	local found: { [number]: Widget } = {}
	for _, k in ipairs(kids) do
		if not k.hidden then
			local visible = math.min(k.y + k.height, bottom) - math.max(k.y, top)
			if visible >= k.height * MIN_SHOWN then
				local clipped = table.clone(k)
				clipped.y = math.max(k.y, top)
				clipped.height = visible
				found[k.index] = clipped
			end
		end
	end
	return found
end

local placeholder_ids: { [number]: boolean } = {}

local function placeholders(): { [number]: boolean }?
	local slots = Host.item_container(CONTAINER)
	if slots == nil then
		return nil
	end
	local unknown: { number } = {}
	for _, s in ipairs(slots) do
		if s.id ~= -1 and placeholder_ids[s.id] == nil and not table.find(unknown, s.id) then
			table.insert(unknown, s.id)
		end
	end
	if #unknown > 0 then
		local flags = Host.item_placeholder(unknown)
		if flags == nil then
			return nil
		end
		for i, id in ipairs(unknown) do
			placeholder_ids[id] = flags[i] == true
		end
	end
	local found: { [number]: boolean } = {}
	for _, s in ipairs(slots) do
		if placeholder_ids[s.id] then
			found[s.slot] = true
		end
	end
	return found
end

local INVENTORY_CONTAINER = 93
local INVENTORY_SIZE = 28
local INVENTORY_GROUP = 15
local INVENTORY_CHILD = 3

local function bank_shown(): boolean
	return Toplevel.is_open(GROUP_ID)
end

local function owned_by(group: number, child: number): (number) -> (Host.MenuEntry) -> boolean
	local id = group * 65536 + child
	return function(slot: number)
		return function(entry: Host.MenuEntry): boolean
			return entry.id == id and entry.index == slot
		end
	end
end

local inventory = Inv.new(INVENTORY_CONTAINER, INVENTORY_SIZE, {
	open = bank_shown,
	boxes = function(): { [number]: Widget }?
		local kids = widget_children(INVENTORY_GROUP, INVENTORY_CHILD)
		if kids == nil then
			return nil
		end
		local by_slot: { [number]: Widget } = {}
		for _, k in ipairs(kids) do
			if k.index >= 0 and not k.hidden then
				by_slot[k.index] = k
			end
		end
		return by_slot
	end,
	owns = owned_by(INVENTORY_GROUP, INVENTORY_CHILD),
})

local items = Inv.new(CONTAINER, nil, {
	open = bank_shown,
	boxes = shown_slots,
	empty = placeholders,
	owns = owned_by(GROUP_ID, GRID_CHILD),
})

local OPEN_TIMEOUT_MS = 2400
local POLL_MS = 50
local OBJECT_OPTIONS = table.freeze({ "Bank", "Use" })
local NPC_OPTIONS = table.freeze({ "Bank" })
local SIDES = table.freeze({
	{ x = 1, y = 0, wall = SafeSpot.FLAG.WALL_EAST },
	{ x = -1, y = 0, wall = SafeSpot.FLAG.WALL_WEST },
	{ x = 0, y = 1, wall = SafeSpot.FLAG.WALL_NORTH },
	{ x = 0, y = -1, wall = SafeSpot.FLAG.WALL_SOUTH },
})

export type Button =
	"ARRANGEMENT"
	| "NOTES"
	| "PLACEHOLDERS"
	| "SEARCH"
	| "DEPOSIT_CONTAINERS"
	| "DEPOSIT_INVENTORY"
	| "DEPOSIT_WORN"
	| "POTION_STORAGE"
	| "TUTORIAL"
export type Quantity = "ONE" | "FIVE" | "TEN" | "CUSTOM" | "ALL"
export type Arrangement = "SWAP" | "INSERT"

local BUTTONS: { [Button]: number } = {
	ARRANGEMENT = 23,
	NOTES = 25,
	PLACEHOLDERS = 40,
	SEARCH = 42,
	DEPOSIT_CONTAINERS = 45,
	DEPOSIT_INVENTORY = 47,
	DEPOSIT_WORN = 49,
	POTION_STORAGE = 53,
	TUTORIAL = 4,
}
local QUANTITY_BUTTONS: { [Quantity]: number } = { ONE = 29, FIVE = 31, TEN = 33, CUSTOM = 35, ALL = 37 }
local ARRANGEMENT_VARBIT = 3959
local NOTES_VARBIT = 3958
local PLACEHOLDERS_VARBIT = 3755
local QUANTITY_VARBIT = 6590
local CUSTOM_QUANTITY_VARBIT = 3960
local QUANTITIES: { [number]: Quantity } = { [0] = "ONE", [1] = "FIVE", [2] = "TEN", [3] = "CUSTOM", [4] = "ALL" }
local QUANTITY_VALUES: { [Quantity]: number } = { ONE = 0, FIVE = 1, TEN = 2, CUSTOM = 3, ALL = 4 }
local TITLE_CHILD = 3
local POTION_STORAGE_TITLE = "potion storage"
local SEARCH_QUERY = "Show items whose names contain"
local CUSTOM_QUANTITY_QUERY = "Enter amount"
local SETTING_TIMEOUT_MS = 2000
local SEARCH_TIMEOUT_MS = 2000
local ENTER_AMOUNT_TIMEOUT_MS = 2500
local MAX_TABS = 9
local SCROLL_TIMEOUT_MS = 8000
local SCROLL_FAR = 3
local SCROLL_BURST_MIN = 2
local SCROLL_BURST_MAX = 6
local SCROLL_PAUSE_MIN_MS = 60
local SCROLL_PAUSE_MAX_MS = 220
local SCROLL_SETTLE_MS = 250
local WITHDRAW_TIMEOUT_MS = 2500
local DEPOSIT_TIMEOUT_MS = 2500
local TAB_SIZE_VARBIT = 4171

export type Source = { loc: Loc?, npc: Npc? }

local Bankmain = {
	inventory = inventory,
	CONTAINER = CONTAINER,
	GROUP = GROUP_ID,
	TABS_CHILD = 10,
	TAB_VARBIT = 4150,
	PLAN_CANDIDATES = 6,
	slots = items.slots,
	items = items.items,
	indices_of = items.indices_of,
	index_of = items.index_of,
	find = items.find,
	contains = items.contains,
	contains_any = items.contains_any,
	contains_all = items.contains_all,
	box = items.box,
	boxes = items.boxes,
	slot_at = items.slot_at,
	hover_slot = items.hover_slot,
	click_slot = items.click_slot,
	interact_slot = items.interact_slot,
	slot_quantity = items.slot_quantity,
	quantity_of_item = items.quantity,
	count = items.count,
	count_ex = items.count_ex,
	wait_count = items.wait_count,
}

Bankmain.inventory = inventory

function Bankmain.is_open(): boolean
	if BankpinKeypad.is_open() then
		local entered = BankpinKeypad.enter()
		if entered ~= "entered" and entered ~= "no_pin" then
			Log.log("Bankmain", `the bank PIN wasn't entered: {entered}`)
			return false
		end
	end
	return Toplevel.is_open(Bankmain.GROUP)
end

function Bankmain.close_button(): Widget?
	local kids = widget_children(Bankmain.GROUP, FRAME_CHILD)
	if kids == nil then
		return nil
	end
	for _, k in ipairs(kids) do
		if k.index == CLOSE_INDEX then
			if k.hidden or k.width > CLOSE_MAX_SIZE or k.height > CLOSE_MAX_SIZE then
				return nil
			end
			return k
		end
	end
	return nil
end

function Bankmain.tab(): number?
	return Host.varbit(Bankmain.TAB_VARBIT)
end

function Bankmain.tab_button(tab: number): Widget?
	for _, k in ipairs(widget_children(Bankmain.GROUP, Bankmain.TABS_CHILD) or {}) do
		if k.index == tab and not k.hidden then
			return k
		end
	end
	return nil
end

function Bankmain.open_tab(tab: number, timeout_ms: number?): boolean
	if Bankmain.tab() == tab then
		return true
	end
	local button = Bankmain.tab_button(tab)
	if button == nil then
		return false
	end
	Mouse.click(button)
	local deadline = Host.time() + (timeout_ms or 2000)
	while Host.time() < deadline do
		if Bankmain.tab() == tab then
			return true
		end
		Host.sleep(100)
	end
	return Bankmain.tab() == tab
end

function Bankmain.wait_open(timeout_ms: number?): boolean
	return Widgets.wait_for(Bankmain.is_open, timeout_ms or OPEN_TIMEOUT_MS, POLL_MS)
end

local function walk_cost(at: Tile, plane: number?): number?
	local plan = Walker.plan(at, plane, { near = Clickbox.APPROACH_NEAR })
	if plan == nil then
		return nil
	end
	local cost = #plan.tiles
	if not plan.reached then
		local last = plan.tiles[#plan.tiles]
		if last then
			cost += Walker.distance(last, at)
		end
	end
	return cost
end

function Bankmain.is_banker(npc: Npc): boolean
	for _, option in ipairs(Npcs.options(npc.id) or {}) do
		if table.find(NPC_OPTIONS, option) then
			return true
		end
	end
	return false
end

local function is_bank(view: Host.LocView): boolean
	local actions = view.actions
	return table.find(actions, "Bank") ~= nil
		or (table.find(actions, "Use") ~= nil and table.find(actions, "Collect") ~= nil)
end

local function bank_locs(): { Loc }
	local locs = Objects.find() or {}
	local ids: { number } = {}
	local index: { [number]: number } = {}
	for _, loc in ipairs(locs) do
		if index[loc.id] == nil then
			table.insert(ids, loc.id)
			index[loc.id] = #ids
		end
	end
	local views = if #ids > 0 then Host.loc_shown(ids) else nil
	local banks: { Loc } = {}
	if views == nil then
		return banks
	end
	for _, loc in ipairs(locs) do
		local view = views[index[loc.id]]
		if view and is_bank(view) then
			table.insert(banks, loc)
		end
	end
	return banks
end

local function covers(loc: Loc, x: number, y: number): boolean
	return x >= loc.x and x < loc.x + loc.size_x and y >= loc.y and y < loc.y + loc.size_y
end

local function has_tile(tiles: { Tile }, x: number, y: number): boolean
	for _, t in ipairs(tiles) do
		if t.x == x and t.y == y then
			return true
		end
	end
	return false
end

function Bankmain.counter_tiles(npc: Npc): { Tile }
	local banker = Npcs.reach(npc)
	local tiles: { Tile } = {}
	local grid = Host.collision({ x1 = banker.x, y1 = banker.y, x2 = banker.x, y2 = banker.y })
	if grid == nil then
		return tiles
	end
	local flags = grid.flags[1]
	for _, loc in ipairs(bank_locs()) do
		for _, d in ipairs(SIDES) do
			local x, y = banker.x + d.x, banker.y + d.y
			if covers(loc, x, y) and bit32.band(flags, d.wall) == 0 then
				while covers(loc, x, y) do
					x, y = x + d.x, y + d.y
				end
				if has_tile(Objects.stand_tiles(loc) or {}, x, y) then
					table.insert(tiles, { x = x, y = y })
				end
			end
		end
	end
	return tiles
end

local function banker_cost(npc: Npc): number?
	local me = Walker.position()
	if me == nil then
		return nil
	end
	local tiles: { Tile } = Bankmain.counter_tiles(npc)
	for _, t in ipairs(Host.reach_tiles(Npcs.reach(npc)) or {}) do
		table.insert(tiles, t)
	end
	table.sort(tiles, function(a: Tile, b: Tile): boolean
		return Walker.distance(me, a) < Walker.distance(me, b)
	end)
	local best: number? = nil
	for i = 1, math.min(#tiles, Clickbox.STAND_CANDIDATES) do
		local plan = Walker.plan(tiles[i], nil, { near = 0 })
		if plan and plan.reached and (best == nil or #plan.tiles < best) then
			best = #plan.tiles
		end
	end
	return best
end

function Bankmain.nearest(): Source?
	local me = Walker.position()
	if me == nil then
		return nil
	end
	type Candidate = { source: Source, straight: number, plane: number? }
	local candidates: { Candidate } = {}
	for _, loc in ipairs(bank_locs()) do
		table.insert(
			candidates,
			{ source = { loc = loc, npc = nil }, straight = Objects.distance(loc, me), plane = loc.plane }
		)
	end
	for _, npc in ipairs(Npcs.find() or {}) do
		if Bankmain.is_banker(npc) then
			table.insert(
				candidates,
				{ source = { loc = nil, npc = npc }, straight = Npcs.distance(npc, me), plane = nil }
			)
		end
	end
	table.sort(candidates, function(a: Candidate, b: Candidate)
		return a.straight < b.straight
	end)
	local best: Source? = nil
	local best_cost = math.huge
	for i = 1, math.min(#candidates, Bankmain.PLAN_CANDIDATES) do
		local c = candidates[i]
		local cost = if c.source.npc then banker_cost(c.source.npc) else walk_cost(c.source.loc :: Tile, c.plane)
		if cost and cost < best_cost then
			best, best_cost = c.source, cost
		end
	end
	if best == nil and candidates[1] then
		return candidates[1].source
	end
	return best
end

export type Bank = { x: number, y: number, plane: number, name: string }
export type Requirement = {
	area: { x1: number, y1: number, x2: number, y2: number },
	quests: { string }?,
	skills: { { name: string, level: number } }?,
}

Bankmain.BANKS = BankList :: { Bank }
Bankmain.TRAVEL_NEAR = 8
Bankmain.TRAVEL_TRIES = 3
Bankmain.REQUIREMENTS = {
	{ area = { x1 = 3200, y1 = 6030, x2 = 3330, y2 = 6150 }, quests = { "Song of the Elves" } },
	{ area = { x1 = 2455, y1 = 2835, x2 = 2475, y2 = 2860 }, quests = { "Dragon Slayer II" } },
	{ area = { x1 = 3590, y1 = 3355, x2 = 3620, y2 = 3380 }, quests = { "Sins of the Father" } },
	{ area = { x1 = 2575, y1 = 3410, x2 = 2595, y2 = 3430 }, skills = { { name = "Fishing", level = 68 } } },
	{ area = { x1 = 1580, y1 = 3465, x2 = 1600, y2 = 3485 }, skills = { { name = "Woodcutting", level = 60 } } },
	{ area = { x1 = 1235, y1 = 3745, x2 = 1260, y2 = 3770 }, skills = { { name = "Farming", level = 45 } } },
	{ area = { x1 = 3000, y1 = 9705, x2 = 3025, y2 = 9730 }, skills = { { name = "Mining", level = 60 } } },
	{ area = { x1 = 2925, y1 = 3270, x2 = 2945, y2 = 3290 }, skills = { { name = "Crafting", level = 40 } } },
} :: { Requirement }

local left_out: { [number]: boolean } = {}
local travelled: number? = nil

local function transports(): any
	return (require :: any)("@volcren/transports")
end

local function within(area: { x1: number, y1: number, x2: number, y2: number }, bank: Bank): boolean
	return bank.x >= area.x1 and bank.x <= area.x2 and bank.y >= area.y1 and bank.y <= area.y2
end

function Bankmain.usable(bank: Bank, wilderness: boolean?): boolean
	local Transports = transports()
	if not wilderness and Transports.wilderness_level(bank.x, bank.y) > 0 then
		return false
	end
	for _, need in ipairs(Bankmain.REQUIREMENTS) do
		if within(need.area, bank) then
			local row = { quests = need.quests or {}, skills = need.skills or {} }
			if not Transports.quests_done(row) or not Transports.skills_met(row) then
				return false
			end
		end
	end
	return true
end

function Bankmain.travel(walk: Walker.WalkOptions?): boolean
	local opts: Walker.WalkOptions = walk or {}
	if opts.transports == false then
		return false
	end
	local here = Walker.position()
	if here == nil then
		return false
	end
	local plan: { [string]: any } = if type(opts.transports) == "table"
		then table.clone(opts.transports) :: { [string]: any }
		else {}
	plan.shortcuts = opts.shortcuts
	plan.prefer_house = if opts.prefer_house ~= nil then opts.prefer_house else Walker.PREFER_HOUSE
	local indices: { number } = {}
	local tiles: { Tile } = {}
	for i, bank in ipairs(Bankmain.BANKS) do
		if not left_out[i] and Bankmain.usable(bank, plan.wilderness) then
			table.insert(indices, i)
			table.insert(tiles, { x = bank.x, y = bank.y, plane = bank.plane })
		end
	end
	local costs = transports().costs({ x = here.x, y = here.y, plane = here.plane or 0 }, tiles, plan)
	local order: { number } = {}
	for k = 1, #indices do
		if costs[k] < math.huge then
			table.insert(order, k)
		end
	end
	table.sort(order, function(a: number, b: number): boolean
		return costs[a] < costs[b]
	end)
	for attempt = 1, math.min(#order, Bankmain.TRAVEL_TRIES) do
		local k = order[attempt]
		local i, bank = indices[k], Bankmain.BANKS[indices[k]]
		Log.log("Bankmain", `travelling to the bank at {bank.x}, {bank.y} (about {math.floor(costs[k])} ticks)`)
		local go = table.clone(opts) :: Walker.WalkOptions
		go.near = Bankmain.TRAVEL_NEAR
		go.plane = bank.plane
		local ok, reached = pcall(Walker.walk, { x = bank.x, y = bank.y }, go)
		if ok and reached and Bankmain.nearest() ~= nil then
			travelled = i
			return true
		end
		left_out[i] = true
		Log.log("Bankmain", `couldn't get to the bank at {bank.x}, {bank.y}: left out for this run`)
	end
	return false
end

local function resolve(source: Source?): Source?
	if source and (source.loc or source.npc) then
		return source
	end
	return Bankmain.nearest()
end

local function walk_options(walk: (Walker.WalkOptions | false)?): Walker.WalkOptions
	return if type(walk) == "table" then walk else {}
end

local function stand_near(source: Source, walk: Walker.WalkOptions): boolean
	local at: Tile = (source.loc or source.npc) :: Tile
	local go = table.clone(walk) :: Walker.WalkOptions
	go.near = Clickbox.APPROACH_NEAR
	return Walker.walk({ x = math.floor(at.x), y = math.floor(at.y) }, go)
end

local function refused_here()
	local i = travelled
	local here = Walker.position()
	if i == nil or here == nil then
		return
	end
	local bank = Bankmain.BANKS[i]
	if math.max(math.abs(bank.x - here.x), math.abs(bank.y - here.y)) <= Bankmain.TRAVEL_NEAR * 2 then
		left_out[i] = true
		travelled = nil
		Log.log("Bankmain", `the bank at {bank.x}, {bank.y} didn't open: left out for this run`)
	end
end

function Bankmain.hover(source: Source?, walk: (Walker.WalkOptions | false)?): boolean
	local target = resolve(source)
	if target == nil and source == nil and walk ~= false and Bankmain.travel(walk_options(walk)) then
		target = resolve(nil)
	end
	if target == nil then
		return false
	end
	if walk ~= false and not stand_near(target, walk_options(walk)) then
		return false
	end
	if target.loc then
		return Objects.hover(target.loc)
	end
	return Npcs.hover(target.npc :: Npc)
end

function Bankmain.open(source: Source?, walk: (Walker.WalkOptions | false)?): boolean
	if BankpinKeypad.is_open() then
		return Bankmain.wait_open()
	end
	if Toplevel.is_open(Bankmain.GROUP) then
		return Bankmain.is_open()
	end
	if not Toplevel.escape_clear() then
		return false
	end
	local target = resolve(source)
	if target == nil and source == nil and walk ~= false and Bankmain.travel(walk_options(walk)) then
		target = resolve(nil)
	end
	if target == nil then
		Log.log("Bankmain", "no bank or banker around")
		return false
	end
	local picked: boolean
	if target.loc then
		if walk ~= false then
			picked = Objects.approach(target.loc, OBJECT_OPTIONS, { walk = walk_options(walk) })
		else
			picked = Objects.interact(target.loc, OBJECT_OPTIONS)
		end
	else
		local npc = target.npc :: Npc
		if walk ~= false then
			picked = Npcs.approach(npc, NPC_OPTIONS, { stand = Bankmain.counter_tiles(npc), walk = walk_options(walk) })
		else
			picked = Npcs.interact(npc, NPC_OPTIONS)
		end
	end
	if not picked then
		local uptext = Minimenu.uptext()
		if uptext == nil or not string.find(string.lower(uptext), "bank", 1, true) then
			return false
		end
		if not Minimenu.select("Bank") then
			refused_here()
			return false
		end
	end
	if not Bankmain.wait_open() then
		refused_here()
		return false
	end
	return true
end

local function wait_varbit(varbit: number, value: number, timeout_ms: number): boolean
	return Widgets.wait_for(function()
		return Host.varbit(varbit) == value
	end, timeout_ms, POLL_MS)
end

function Bankmain.button(button: Button): Widget?
	local w = Host.widget(Bankmain.GROUP, BUTTONS[button])
	if w == nil or w.hidden then
		return nil
	end
	return w
end

function Bankmain.click_button(button: Button): boolean
	if not Toplevel.is_open(Bankmain.GROUP) then
		return false
	end
	local w = Bankmain.button(button)
	if w == nil then
		return false
	end
	Mouse.click(w)
	return true
end

local function set_setting(button: Button, varbit: number, value: number, timeout_ms: number?): boolean
	local now = Host.varbit(varbit)
	if now == nil then
		return false
	end
	if now == value then
		return true
	end
	if not Bankmain.click_button(button) then
		return false
	end
	return wait_varbit(varbit, value, timeout_ms or SETTING_TIMEOUT_MS)
end

function Bankmain.arrangement(): Arrangement?
	local v = Host.varbit(ARRANGEMENT_VARBIT)
	if v == nil then
		return nil
	end
	return if v == 1 then "INSERT" else "SWAP"
end

function Bankmain.set_arrangement(mode: Arrangement, timeout_ms: number?): boolean
	return set_setting("ARRANGEMENT", ARRANGEMENT_VARBIT, if mode == "INSERT" then 1 else 0, timeout_ms)
end

local function flag(varbit: number): boolean?
	local v = Host.varbit(varbit)
	if v == nil then
		return nil
	end
	return v == 1
end

function Bankmain.notes(): boolean?
	return flag(NOTES_VARBIT)
end

function Bankmain.set_notes(on: boolean, timeout_ms: number?): boolean
	return set_setting("NOTES", NOTES_VARBIT, if on then 1 else 0, timeout_ms)
end

function Bankmain.placeholders(): boolean?
	return flag(PLACEHOLDERS_VARBIT)
end

function Bankmain.set_placeholders(on: boolean, timeout_ms: number?): boolean
	return set_setting("PLACEHOLDERS", PLACEHOLDERS_VARBIT, if on then 1 else 0, timeout_ms)
end

function Bankmain.quantity(): Quantity?
	local v = Host.varbit(QUANTITY_VARBIT)
	if v == nil then
		return nil
	end
	return QUANTITIES[v]
end

function Bankmain.custom_quantity(): number?
	return Host.varbit(CUSTOM_QUANTITY_VARBIT)
end

function Bankmain.quantity_of(amount: number): Quantity
	if amount == 1 then
		return "ONE"
	elseif amount == 5 then
		return "FIVE"
	elseif amount == 10 then
		return "TEN"
	elseif amount <= 0 then
		return "ALL"
	end
	return "CUSTOM"
end

local function quantity_button(quantity: Quantity): Widget?
	if not Toplevel.is_open(Bankmain.GROUP) then
		return nil
	end
	local w = Host.widget(Bankmain.GROUP, QUANTITY_BUTTONS[quantity])
	if w == nil or w.hidden then
		return nil
	end
	return w
end

function Bankmain.select_quantity(quantity: Quantity, timeout_ms: number?): boolean
	local value = QUANTITY_VALUES[quantity] or 0
	if Host.varbit(QUANTITY_VARBIT) == value then
		return true
	end
	local w = quantity_button(quantity)
	if w == nil then
		return false
	end
	Mouse.click(w)
	return wait_varbit(QUANTITY_VARBIT, value, timeout_ms or SETTING_TIMEOUT_MS)
end

function Bankmain.set_custom_quantity(amount: number): boolean
	if amount < 1 then
		return false
	end
	if Bankmain.custom_quantity() == amount then
		return Bankmain.select_quantity("CUSTOM")
	end
	local w = quantity_button("CUSTOM")
	if w == nil then
		return false
	end
	if not Interact.with(w, "Set custom quantity", nil) then
		return false
	end
	if not Chat.answer_query(CUSTOM_QUANTITY_QUERY, tostring(math.floor(amount)), ENTER_AMOUNT_TIMEOUT_MS) then
		return false
	end
	return wait_varbit(CUSTOM_QUANTITY_VARBIT, math.floor(amount), SETTING_TIMEOUT_MS)
end

function Bankmain.set_quantity(amount: number): boolean
	local quantity = Bankmain.quantity_of(amount)
	if quantity == "CUSTOM" then
		return Bankmain.set_custom_quantity(amount)
	end
	return Bankmain.select_quantity(quantity)
end

function Bankmain.lock_slot(slot: number, locked: boolean): boolean
	local slots = Inventory.locked_slots()
	if slots == nil then
		return false
	end
	if (slots[slot] == true) == locked then
		return true
	end
	if not inventory.interact_slot(slot, if locked then "Lock-slot" else "Unlock-slot") then
		return false
	end
	return Inventory.wait_slot_locked(slot, locked)
end

function Bankmain.deposit_inventory(): boolean
	return Bankmain.click_button("DEPOSIT_INVENTORY")
end

function Bankmain.deposit_worn(): boolean
	return Bankmain.click_button("DEPOSIT_WORN")
end

function Bankmain.deposit_containers(): boolean
	return Bankmain.click_button("DEPOSIT_CONTAINERS")
end

function Bankmain.is_search_open(): boolean
	return Chat.find_query(SEARCH_QUERY)
end

function Bankmain.open_search(timeout_ms: number?): boolean
	if Bankmain.is_search_open() then
		return true
	end
	if not Bankmain.click_button("SEARCH") then
		return false
	end
	return Chat.wait_query(SEARCH_QUERY, timeout_ms or SEARCH_TIMEOUT_MS)
end

function Bankmain.search(text: string): boolean
	if not Bankmain.open_search() then
		return false
	end
	return Chat.answer_query(SEARCH_QUERY, text, SEARCH_TIMEOUT_MS)
end

function Bankmain.has_potion_storage(): boolean
	return Bankmain.button("POTION_STORAGE") ~= nil
end

function Bankmain.potion_storage_is_open(): boolean
	local w = Host.widget(Bankmain.GROUP, TITLE_CHILD)
	if w == nil or w.text == nil then
		return false
	end
	return string.find(string.lower(w.text), POTION_STORAGE_TITLE, 1, true) ~= nil
end

function Bankmain.toggle_potion_storage(timeout_ms: number?): boolean
	local before = Bankmain.potion_storage_is_open()
	if not Bankmain.click_button("POTION_STORAGE") then
		return false
	end
	return Widgets.wait_within(function()
		return Bankmain.potion_storage_is_open() ~= before
	end, timeout_ms or SETTING_TIMEOUT_MS, POLL_MS)
end

function Bankmain.open_potion_storage(timeout_ms: number?): boolean
	return Bankmain.potion_storage_is_open() or Bankmain.toggle_potion_storage(timeout_ms)
end

function Bankmain.close_potion_storage(timeout_ms: number?): boolean
	return not Bankmain.potion_storage_is_open() or Bankmain.toggle_potion_storage(timeout_ms)
end

function Bankmain.tab_sizes(): { number }?
	local sizes: { number } = {}
	for tab = 1, MAX_TABS do
		local size = Host.varbit(TAB_SIZE_VARBIT + tab - 1)
		if size == nil then
			return nil
		end
		sizes[tab] = size
	end
	return sizes
end

function Bankmain.count_tabs(): number?
	local sizes = Bankmain.tab_sizes()
	if sizes == nil then
		return nil
	end
	local count = 0
	for _, size in ipairs(sizes) do
		if size > 0 then
			count += 1
		end
	end
	return count
end

function Bankmain.tab_of_slot(slot: number): number?
	local sizes = Bankmain.tab_sizes()
	if sizes == nil then
		return nil
	end
	local first = 0
	for tab, size in ipairs(sizes) do
		first += size
		if slot < first then
			return tab
		end
	end
	return 0
end

function Bankmain.find_tab(item: Item | { Item }): (number?, number?)
	local slot = items.index_of(item)
	if slot == nil then
		return nil, nil
	end
	return Bankmain.tab_of_slot(slot), slot
end

local function slot_widget(slot: number): Widget?
	for _, k in ipairs(widget_children(GROUP_ID, GRID_CHILD) or {}) do
		if k.index == slot then
			return if k.hidden then nil else k
		end
	end
	return nil
end

function Bankmain.is_slot_shown(slot: number): boolean
	local shown = shown_slots()
	return shown ~= nil and shown[slot] ~= nil
end

local function distance_of(g: Widget, w: Widget): number
	local top, bottom = g.y, g.y + g.height
	if w.y >= top and w.y + w.height <= bottom then
		return 0
	end
	local delta = if w.y < top then w.y - top else w.y + w.height - bottom
	return if delta > 0 then math.ceil(delta / SCROLL_NOTCH) else math.floor(delta / SCROLL_NOTCH)
end

function Bankmain.scroll_distance(slot: number): number?
	local g, w = grid(), slot_widget(slot)
	if g == nil or w == nil then
		return nil
	end
	return distance_of(g, w)
end

function Bankmain.scroll_to(slot: number, timeout_ms: number?): boolean
	local deadline = Host.time() + (timeout_ms or SCROLL_TIMEOUT_MS)
	local first = true
	while Host.time() < deadline do
		local g, w = grid(), slot_widget(slot)
		if g == nil or w == nil then
			return false
		end
		local need = distance_of(g, w)
		if need == 0 then
			return true
		end
		local notches = need
		if first and math.abs(need) > SCROLL_FAR then
			local aim = g.y + Random.float(0.15, 0.85) * (g.height - w.height)
			notches = math.round((w.y - aim) / SCROLL_NOTCH)
			if notches == 0 then
				notches = need
			end
		end
		first = false
		local direction = notches > 0
		local left = math.abs(notches)
		while left > 0 do
			local burst = math.min(left, Random.int(SCROLL_BURST_MIN, SCROLL_BURST_MAX))
			Mouse.scroll(burst, direction, if left == math.abs(notches) then g else nil)
			left -= burst
			if left > 0 then
				Random.sleep(SCROLL_PAUSE_MIN_MS, SCROLL_PAUSE_MAX_MS)
			end
		end
		Host.sleep(SCROLL_SETTLE_MS)
	end
	return Bankmain.is_slot_shown(slot)
end

function Bankmain.show_slot(slot: number): boolean
	local tab = Bankmain.tab_of_slot(slot)
	if tab == nil then
		return false
	end
	if not Bankmain.open_tab(tab) then
		return false
	end
	return Bankmain.scroll_to(slot)
end

Bankmain.ALL_BUT_ONE = -1
Bankmain.CUSTOM_BUTTON_CHANCE = 0.005

local function enable_custom(amount: number): (boolean, boolean)
	local custom = QUANTITY_VALUES.CUSTOM
	if Host.varbit(QUANTITY_VARBIT) == custom then
		return true, false
	end
	local w = quantity_button("CUSTOM")
	if w == nil then
		return false, false
	end
	Mouse.click(w)
	local prompted = false
	local selected = Widgets.wait_within(function(): boolean
		prompted = Chat.find_query(CUSTOM_QUANTITY_QUERY)
		return prompted or Host.varbit(QUANTITY_VARBIT) == custom
	end, SETTING_TIMEOUT_MS, POLL_MS)
	if prompted then
		local typed = Chat.answer_query(CUSTOM_QUANTITY_QUERY, tostring(math.floor(amount)), ENTER_AMOUNT_TIMEOUT_MS)
		return typed and wait_varbit(CUSTOM_QUANTITY_VARBIT, math.floor(amount), SETTING_TIMEOUT_MS), true
	end
	return selected, false
end

local function stocked_slot(item: Item | { Item }): (number?, number?)
	local matcher = Item.matcher(item)
	local all = items.items()
	if all == nil then
		return nil, nil
	end
	for _, s in ipairs(all) do
		if s.quantity > 0 and Item.matches(matcher, s.id, s.name) then
			return s.slot, s.quantity
		end
	end
	return nil, nil
end

local function withdraw_option(amount: number): (string, boolean)
	if amount == Bankmain.ALL_BUT_ONE then
		return "Withdraw-All-but-1", false
	elseif amount <= 0 then
		return "Withdraw-All", false
	elseif amount == 1 or amount == 5 or amount == 10 or amount == Bankmain.custom_quantity() then
		return `Withdraw-{amount}`, false
	end
	return "Withdraw-X", true
end

function Bankmain.withdraw(
	item: Item | { Item },
	amount: number?,
	noted: boolean?,
	use_quantity_buttons: boolean?
): boolean
	if not Bankmain.is_open() then
		return false
	end
	local slot, stock = stocked_slot(item)
	if slot == nil or stock == nil then
		Log.log("Bankmain", "withdraw: the bank has none of it")
		return false
	end
	local wanted = amount or 1
	if wanted > 0 and wanted >= stock then
		wanted = 0
	end
	if noted ~= nil and not Bankmain.set_notes(noted) then
		return false
	end
	local before = items.count_ex(item) or 0
	local new_x = wanted > 0 and Bankmain.quantity_of(wanted) == "CUSTOM" and Bankmain.custom_quantity() ~= wanted
	if use_quantity_buttons ~= false and new_x and not Biometrics.random_boolean(Bankmain.CUSTOM_BUTTON_CHANCE) then
		local selected, prompted = enable_custom(wanted)
		if not selected then
			Log.log("Bankmain", "withdraw: couldn't select the X quantity button")
			return false
		end
		local current = stocked_slot(item)
		if current ~= nil then
			slot = current
		end
		if not Bankmain.show_slot(slot) then
			Log.log("Bankmain", `withdraw: couldn't show bank slot {slot}`)
			return false
		end
		if prompted then
			if not items.click_slot(slot) then
				Log.log("Bankmain", `withdraw: couldn't click bank slot {slot}`)
				return false
			end
		else
			if not items.interact_slot(slot, "Withdraw-X") then
				Log.log("Bankmain", `withdraw: couldn't pick Withdraw-X on bank slot {slot}`)
				return false
			end
			if not Chat.answer_query(CUSTOM_QUANTITY_QUERY, tostring(math.floor(wanted)), ENTER_AMOUNT_TIMEOUT_MS) then
				Log.log("Bankmain", `withdraw: couldn't type {wanted} for Withdraw-X`)
				return false
			end
		end
	elseif use_quantity_buttons ~= false and wanted ~= Bankmain.ALL_BUT_ONE then
		if not Bankmain.set_quantity(wanted) then
			Log.log("Bankmain", `withdraw: couldn't set the quantity to {wanted}`)
			return false
		end
		local current = stocked_slot(item)
		if current ~= nil then
			slot = current
		end
		if not Bankmain.show_slot(slot) then
			Log.log("Bankmain", `withdraw: couldn't show bank slot {slot}`)
			return false
		end
		if not items.click_slot(slot) then
			Log.log("Bankmain", `withdraw: couldn't click bank slot {slot}`)
			return false
		end
	else
		if not Bankmain.show_slot(slot) then
			return false
		end
		local option, typed = withdraw_option(wanted)
		if not items.interact_slot(slot, option) then
			Log.log("Bankmain", `withdraw: couldn't pick {option} on bank slot {slot}`)
			return false
		end
		if
			typed
			and not Chat.answer_query(CUSTOM_QUANTITY_QUERY, tostring(math.floor(wanted)), ENTER_AMOUNT_TIMEOUT_MS)
		then
			return false
		end
	end
	if
		Widgets.wait_within(function()
			return (items.count_ex(item) or before) < before
		end, WITHDRAW_TIMEOUT_MS, POLL_MS)
	then
		return true
	end
	Log.log(
		"Bankmain",
		`withdraw: the bank still held {items.count_ex(item) or "?"} of it after {WITHDRAW_TIMEOUT_MS} ms (had {before})`
	)
	return false
end

function Bankmain.selected_amount(): number?
	local quantity = Bankmain.quantity()
	if quantity == "ONE" then
		return 1
	elseif quantity == "FIVE" then
		return 5
	elseif quantity == "TEN" then
		return 10
	elseif quantity == "CUSTOM" then
		return Bankmain.custom_quantity()
	elseif quantity == "ALL" then
		return math.huge
	end
	return nil
end

function Bankmain.withdraw_selected(item: Item | { Item }, wait: boolean?): boolean
	if not Bankmain.is_open() then
		return false
	end
	local slot = stocked_slot(item)
	if slot == nil then
		Log.log("Bankmain", "withdraw_selected: the bank has none of it")
		return false
	end
	local before = items.count_ex(item) or 0
	if not Bankmain.show_slot(slot) then
		Log.log("Bankmain", `withdraw_selected: couldn't show bank slot {slot}`)
		return false
	end
	if not items.click_slot(slot) then
		Log.log("Bankmain", `withdraw_selected: couldn't click bank slot {slot}`)
		return false
	end
	if wait == false then
		return true
	end
	return Widgets.wait_within(function()
		return (items.count_ex(item) or before) < before
	end, WITHDRAW_TIMEOUT_MS, POLL_MS)
end

export type Deposit = { item: Item | { Item }, quantity: number? }
export type Entry = Item | Deposit

local function deposit_option(amount: number): (string, boolean)
	if amount <= 0 then
		return "Deposit-All", false
	elseif amount == 1 or amount == 5 or amount == 10 or amount == Bankmain.custom_quantity() then
		return `Deposit-{amount}`, false
	end
	return "Deposit-X", true
end

local function deposit_slot(slot: number, amount: number, use_quantity_buttons: boolean): boolean
	if use_quantity_buttons then
		if not Bankmain.set_quantity(amount) then
			return false
		end
		return inventory.click_slot(slot)
	end
	local option, typed = deposit_option(amount)
	if not inventory.interact_slot(slot, option) then
		return false
	end
	if typed then
		return Chat.answer_query(CUSTOM_QUANTITY_QUERY, tostring(math.floor(amount)), ENTER_AMOUNT_TIMEOUT_MS)
	end
	return true
end

function Bankmain.deposit(item: Item | { Item }, amount: number?, use_quantity_buttons: boolean?): boolean
	if not Bankmain.is_open() then
		return false
	end
	local slots = inventory.indices_of(item)
	if slots == nil or #slots == 0 then
		Log.log("Bankmain", "deposit: the inventory has none of it")
		return false
	end
	local have = inventory.count_ex(item) or 0
	local wanted = amount or 0
	if wanted > 0 and wanted >= have then
		wanted = 0
	end
	if not deposit_slot(slots[Random.int(1, #slots)], wanted, use_quantity_buttons ~= false) then
		return false
	end
	return Widgets.wait_within(function()
		return (inventory.count_ex(item) or have) < have
	end, DEPOSIT_TIMEOUT_MS, POLL_MS)
end

function Bankmain.deposit_array(entries: { Entry }, use_quantity_buttons: boolean?): boolean
	local all_ok = true
	for _, entry in ipairs(entries) do
		local e: Deposit = if type(entry) == "table" and (entry :: any).item ~= nil
			then entry :: Deposit
			else { item = entry :: Item }
		if not Bankmain.deposit(e.item, e.quantity, use_quantity_buttons) then
			all_ok = false
		end
	end
	return all_ok
end

function Bankmain.deposit_different(keep: Item | { Item }, use_quantity_buttons: boolean?): boolean
	local matcher = Item.matcher(keep)
	local all = inventory.items()
	if all == nil then
		return false
	end
	local done: { [number]: boolean } = {}
	for _, s in ipairs(all) do
		if not Item.matches(matcher, s.id, s.name) and not done[s.id] then
			done[s.id] = true
			if not Bankmain.deposit(s.id, 0, use_quantity_buttons) then
				return false
			end
		end
	end
	return true
end

function Bankmain.close(key_probability: number?, timeout_ms: number?): boolean
	return Toplevel.close(Bankmain.GROUP, Bankmain.close_button, key_probability, timeout_ms)
end

return Bankmain
