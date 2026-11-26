--!strict
local Host = require("@volcren/host")
local widget = Host.widget
local widget_children = Host.widget_children
type Widget = Host.Widget
local Bankmain = require("@volcren/bankmain")
local Chat = require("@volcren/chat")
local Interact = require("@volcren/interact")
local Item = require("@volcren/item")
type Item = Item.Item
local Log = require("@volcren/log")
local Mouse = require("@volcren/mouse")
local Random = require("@volcren/random")
local Runepouch = require("@volcren/runepouch")
local Ticks = require("@volcren/ticks")
local Widgets = require("@volcren/widgets")

local BankPouch = {}

export type Loadout = { number: number, name: string?, runes: { number } }

BankPouch.GROUP = 15
BankPouch.PANEL = 19
BankPouch.DEPOSIT_RUNES = 20
BankPouch.DISMISS = 22
BankPouch.RUNES = 26
BankPouch.LIST = 32
BankPouch.LOADOUTS = table.freeze({ 33, 37, 40, 43, 45, 47, 49, 51, 53, 55 })
BankPouch.NAMES = table.freeze({ 34, 38, 41, 44, 46, 48, 50, 52, 54, 56 })
BankPouch.LAST_CHILD = 120
BankPouch.SLOTS = 4
BankPouch.EMPTY_SLOT = 11526
BankPouch.UNNAMED = "-"
BankPouch.OPTION = "Configure"
BankPouch.DISMISS_OPTION = "Dismiss"
BankPouch.DEPOSIT_RUNES_OPTION = "Deposit runes"
BankPouch.LOAD_OPTION = "Load"
BankPouch.AMOUNT_QUERY = "Enter amount"
BankPouch.MAX_SCROLLS = 12
BankPouch.TIMEOUT_MS = 2500
BankPouch.POLL_MS = 50
BankPouch.SETTLE_TICKS = 1

function BankPouch.is_open(): boolean
	local panel = widget(BankPouch.GROUP, BankPouch.PANEL)
	return panel ~= nil and not panel.hidden and Bankmain.is_open()
end

local function wait(check: () -> boolean, timeout_ms: number?): boolean
	return Widgets.wait_within(check, timeout_ms or BankPouch.TIMEOUT_MS, BankPouch.POLL_MS)
end

function BankPouch.open(timeout_ms: number?): boolean
	if BankPouch.is_open() then
		return true
	end
	if not Bankmain.is_open() then
		Log.log("BankPouch", "open: the bank isn't open")
		return false
	end
	local slots = Bankmain.inventory.indices_of(Runepouch.POUCHES)
	if slots == nil or #slots == 0 then
		Log.log("BankPouch", "open: the inventory holds no rune pouch")
		return false
	end
	if not Bankmain.inventory.interact_slot(slots[1], BankPouch.OPTION) then
		return false
	end
	if not wait(BankPouch.is_open, timeout_ms) then
		return false
	end
	Ticks.wait(BankPouch.SETTLE_TICKS)
	return true
end

local function click(child: number, option: string, what: string): boolean
	local w = widget(BankPouch.GROUP, child)
	if w == nil or w.hidden or not Interact.with(w, option, nil, false) then
		Log.log("BankPouch", `{what}: couldn't click {option}`)
		return false
	end
	return true
end

function BankPouch.close(timeout_ms: number?): boolean
	if not BankPouch.is_open() then
		return true
	end
	if not click(BankPouch.DISMISS, BankPouch.DISMISS_OPTION, "close") then
		return false
	end
	return wait(function()
		return not BankPouch.is_open()
	end, timeout_ms)
end

local function require_open(what: string): boolean
	if BankPouch.is_open() then
		return true
	end
	Log.log("BankPouch", `{what}: the pouch panel isn't open`)
	return false
end

local function held_rune(rune: Item): Runepouch.Rune?
	for _, held in ipairs(Runepouch.runes() or {}) do
		if held.id == rune or (type(rune) == "string" and string.lower(held.name) == string.lower(rune)) then
			return held
		end
	end
	return nil
end

local function slot_child(child: number, index: number): Widget?
	for _, k in ipairs(widget_children(BankPouch.GROUP, child) or {}) do
		if k.index == index then
			return k
		end
	end
	return nil
end

local function rune_cell(id: number): Widget?
	for index = 1, BankPouch.SLOTS do
		local cell = slot_child(BankPouch.RUNES, index)
		if cell and not cell.hidden and cell.item_id == id then
			return cell
		end
	end
	return nil
end

function BankPouch.deposit_runes(): boolean
	if not require_open("deposit_runes") then
		return false
	end
	local held = Runepouch.runes()
	if held == nil then
		return false
	end
	if #held == 0 then
		return true
	end
	if not click(BankPouch.DEPOSIT_RUNES, BankPouch.DEPOSIT_RUNES_OPTION, "deposit_runes") then
		return false
	end
	return wait(function()
		local now = Runepouch.runes()
		return now ~= nil and #now == 0
	end)
end

local function deposit_option(amount: number, held: number): (string, boolean)
	if amount <= 0 or amount >= held then
		return "Deposit-All", false
	elseif amount == 1 or amount == 5 or amount == 10 or amount == Bankmain.custom_quantity() then
		return `Deposit-{amount}`, false
	end
	return "Deposit-X", true
end

function BankPouch.deposit(rune: Item, amount: number?): boolean
	if not require_open("deposit") then
		return false
	end
	local held = held_rune(rune)
	if held == nil then
		Log.log("BankPouch", "deposit: the pouch holds none of it")
		return false
	end
	local cell = rune_cell(held.id)
	if cell == nil then
		Log.log("BankPouch", `deposit: no pouch slot shows {held.name}`)
		return false
	end
	local wanted = amount or 0
	local option, typed = deposit_option(wanted, held.quantity)
	if not Interact.with(cell, option, nil, false) then
		Log.log("BankPouch", `deposit: couldn't pick {option} on {held.name}`)
		return false
	end
	if typed and not Chat.answer_query(BankPouch.AMOUNT_QUERY, tostring(math.floor(wanted)), BankPouch.TIMEOUT_MS) then
		return false
	end
	return wait(function()
		return Runepouch.quantity(held.id) < held.quantity
	end)
end

local function rune_name(rune: Item): string
	if type(rune) == "string" then
		return rune
	end
	return Item.name(rune) or `rune {rune}`
end

function BankPouch.withdraw(rune: Item, amount: number?): boolean
	if not require_open("withdraw") then
		return false
	end
	local before = Runepouch.quantity(rune)
	local wanted = if amount == nil then "1" elseif amount <= 0 then "all" else tostring(amount)
	if not Bankmain.withdraw(rune, amount, false) then
		Log.log("BankPouch", `withdraw: couldn't take {wanted} of {rune_name(rune)} from the bank`)
		return false
	end
	local taken = wait(function()
		return Runepouch.quantity(rune) > before
	end)
	if not taken then
		Log.log("BankPouch", `withdraw: the pouch didn't take {rune_name(rune)}`)
	end
	return taken
end

function BankPouch.loadouts(): { Loadout }?
	if not require_open("loadouts") then
		return nil
	end
	local list: { Loadout } = {}
	for number, row in ipairs(BankPouch.LOADOUTS) do
		local runes: { number } = {}
		for index = 1, BankPouch.SLOTS do
			local cell = slot_child(row, index)
			if cell and cell.item_id > 0 and cell.item_id ~= BankPouch.EMPTY_SLOT then
				table.insert(runes, cell.item_id)
			end
		end
		local label = slot_child(BankPouch.NAMES[number], 1)
		local text = label and label.text
		local name = if text and text ~= "" and text ~= BankPouch.UNNAMED then text else nil
		table.insert(list, { number = number, name = name, runes = runes })
	end
	return list
end

local function loadout_number(which: number | string): number?
	if type(which) == "number" then
		return if BankPouch.LOADOUTS[which] then which else nil
	end
	for _, loadout in ipairs(BankPouch.loadouts() or {}) do
		if loadout.name and string.lower(loadout.name) == string.lower(which) then
			return loadout.number
		end
	end
	return nil
end

local function in_view(w: Widget, view: Widget): boolean
	return w.y >= view.y and w.y + w.height <= view.y + view.height
end

local function scroll_into_view(child: number): boolean
	local last: number? = nil
	for _ = 1, BankPouch.MAX_SCROLLS do
		local view, w = widget(BankPouch.GROUP, BankPouch.LIST), widget(BankPouch.GROUP, child)
		if view == nil or w == nil then
			return false
		end
		if in_view(w, view) then
			return true
		end
		if last == w.y then
			return false
		end
		last = w.y
		Mouse.scroll(Random.int(1, 3), w.y > view.y, view)
		Random.sleep(80, 250)
	end
	return false
end

local function load_button(row: number): Widget?
	local id = BankPouch.GROUP * 65536 + row
	for child = BankPouch.PANEL, BankPouch.LAST_CHILD do
		local w = widget(BankPouch.GROUP, child)
		if w and w.parent == id and w.action == BankPouch.LOAD_OPTION and not w.hidden then
			return w
		end
	end
	return nil
end

local function holds_only(runes: { number }): boolean
	local held = Runepouch.runes()
	if held == nil or #held ~= #runes then
		return false
	end
	for _, rune in ipairs(held) do
		if not table.find(runes, rune.id) then
			return false
		end
	end
	return true
end

function BankPouch.load(which: number | string): boolean
	if not require_open("load") then
		return false
	end
	local number = loadout_number(which)
	local loadouts = BankPouch.loadouts()
	if number == nil or loadouts == nil then
		Log.log("BankPouch", `load: no loadout {which}`)
		return false
	end
	local runes = loadouts[number].runes
	if #runes == 0 then
		Log.log("BankPouch", `load: loadout {number} is empty`)
		return false
	end
	local row = BankPouch.LOADOUTS[number]
	if not scroll_into_view(row) then
		Log.log("BankPouch", `load: couldn't scroll loadout {number} into view`)
		return false
	end
	local button = load_button(row)
	if button == nil or not Interact.with(button, BankPouch.LOAD_OPTION, nil, false) then
		Log.log("BankPouch", `load: couldn't click Load on loadout {number}`)
		return false
	end
	return wait(function()
		return holds_only(runes)
	end)
end

return BankPouch
