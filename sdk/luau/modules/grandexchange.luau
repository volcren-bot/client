--!strict
local Host = require("@volcren/host")
local widget = Host.widget
local widget_children = Host.widget_children
local sleep = Host.sleep
type Widget = Host.Widget
local Interact = require("@volcren/interact")
local Inventory = require("@volcren/inventory")
local Keyboard = require("@volcren/keyboard")
local Log = require("@volcren/log")
local Npcs = require("@volcren/npcs")
local Random = require("@volcren/random")
local Toplevel = require("@volcren/toplevel")
local Widgets = require("@volcren/widgets")

local NAME = "GrandExchange"
local GROUP = 465
local FRAME_CHILD = 2
local CLOSE_INDEX = 11
local TOP_BAR_CHILD = 6
local COLLECT_INDEX = 0
local FIRST_SLOT_CHILD = 7
local SLOTS = 8
local BACK_CHILD = 4
local SETUP_CHILD = 26
local CONFIRM_CHILD = 30
local SLOT_PART = table.freeze({
	TITLE = 16,
	ITEM = 18,
	NAME = 19,
	BAR_BACK = 21,
	BAR = 22,
	PRICE = 25,
	BUY = 26,
	SELL = 27,
})
local SETUP_PART = table.freeze({
	PLUS_ONE = 3,
	PLUS_TEN = 4,
	PLUS_HUNDRED = 5,
	PLUS_THOUSAND = 6,
	ENTER_QUANTITY = 7,
	MINUS_FIVE = 10,
	PLUS_FIVE = 13,
	ITEM = 23,
	QUANTITY = 34,
	PRICE = 41,
	TOTAL = 45,
})
local CHATBOX_GROUP = 162
local SEARCH_RESULTS_CHILD = 53
local PROMPT_CHILD = 44
local COMPLETE_COLOR = 0x005f00
local ABORTED_COLOR = 0x8f0000
local CLERK = "Grand Exchange Clerk"
local OPTION = "Exchange"
local BUY_OPTION = "Create Buy offer"
local POLL_MS = 50
local OPEN_TIMEOUT_MS = 4000
local STEP_TIMEOUT_MS = 2400
local SEARCH_TIMEOUT_MS = 3000
local SEARCH_SETTLE_MS = 450
local CLICK_GAP_MS = { 180, 420 }
local MAX_BUTTON_CLICKS = { 4, 7 }
local PERCENT_STEP = 5
local MAX_PERCENT = 100

export type State = "empty" | "buying" | "selling" | "bought" | "sold" | "aborted"

export type Offer = {
	slot: number,
	state: State,
	id: number?,
	name: string?,
	quantity: number,
	price: number?,
	progress: number,
}

local GrandExchange = {
	GROUP = GROUP,
	SLOTS = SLOTS,
	CLERK = CLERK,
	OPTION = OPTION,
}

local function part(child: number, index: number): Widget?
	for _, k in ipairs(widget_children(GROUP, child) or {}) do
		if k.index == index then
			return k
		end
	end
	return nil
end

local function text_at(child: number, index: number): string?
	local w = part(child, index)
	return if w then w.text else nil
end

local function shown(w: Widget?): Widget?
	if w == nil or w.hidden then
		return nil
	end
	return w
end

local function number_in(text: string?): number?
	if text == nil then
		return nil
	end
	local digits = string.gsub(string.match(text, "[%d,]+") or "", ",", "")
	return tonumber(digits)
end

local function pause()
	sleep(Random.int(CLICK_GAP_MS[1], CLICK_GAP_MS[2]))
end

function GrandExchange.is_open(): boolean
	return Toplevel.is_open(GROUP)
end

function GrandExchange.wait_open(timeout_ms: number?): boolean
	return Widgets.wait_for(GrandExchange.is_open, timeout_ms or OPEN_TIMEOUT_MS, POLL_MS)
end

function GrandExchange.close_button(): Widget?
	return shown(part(FRAME_CHILD, CLOSE_INDEX))
end

function GrandExchange.close(key_probability: number?, timeout_ms: number?): boolean
	if not GrandExchange.is_open() then
		return true
	end
	return Toplevel.close(GROUP, GrandExchange.close_button, key_probability, timeout_ms)
end

function GrandExchange.open(): (boolean, string?)
	if GrandExchange.is_open() then
		return true, nil
	end
	local me = Host.game_state().position
	local clerk = if me then Npcs.nearest(CLERK, me) else nil
	if clerk == nil then
		return false, "no Grand Exchange Clerk nearby"
	end
	if not Npcs.interact(clerk, OPTION) then
		return false, "couldn't pick Exchange on the clerk"
	end
	if not GrandExchange.wait_open() then
		return false, "the Grand Exchange didn't open"
	end
	return true, nil
end

function GrandExchange.is_overview(): boolean
	return GrandExchange.is_open() and shown(widget(GROUP, FIRST_SLOT_CHILD)) ~= nil
end

function GrandExchange.is_setup(): boolean
	return GrandExchange.is_open() and shown(widget(GROUP, SETUP_CHILD)) ~= nil
end

local function state_of(title: string, bar: Widget?): State
	if title == "Empty" then
		return "empty"
	end
	local buying = title == "Buy"
	if bar and bar.text_color == ABORTED_COLOR then
		return "aborted"
	end
	if bar and bar.text_color == COMPLETE_COLOR then
		return if buying then "bought" else "sold"
	end
	return if buying then "buying" else "selling"
end

function GrandExchange.offer(slot: number): Offer?
	if not GrandExchange.is_overview() or slot < 0 or slot >= SLOTS then
		return nil
	end
	local child = FIRST_SLOT_CHILD + slot
	local title = part(child, SLOT_PART.TITLE)
	if title == nil or title.text == nil then
		return nil
	end
	local item, bar, back = part(child, SLOT_PART.ITEM), part(child, SLOT_PART.BAR), part(child, SLOT_PART.BAR_BACK)
	local state = state_of(title.text, bar)
	local filled = if bar and back and back.width > 0 then bar.width / back.width else 0
	return {
		slot = slot,
		state = state,
		id = if item and item.item_id ~= -1 and state ~= "empty" then item.item_id else nil,
		name = if state ~= "empty" then text_at(child, SLOT_PART.NAME) else nil,
		quantity = if item and state ~= "empty" then item.item_quantity else 0,
		price = if state ~= "empty" then number_in(text_at(child, SLOT_PART.PRICE)) else nil,
		progress = if state == "empty" then 0 else math.clamp(filled, 0, 1),
	}
end

function GrandExchange.offers(): { Offer }?
	if not GrandExchange.is_overview() then
		return nil
	end
	local found: { Offer } = {}
	for slot = 0, SLOTS - 1 do
		local offer = GrandExchange.offer(slot)
		if offer then
			table.insert(found, offer)
		end
	end
	return found
end

function GrandExchange.free_slot(): number?
	for _, offer in ipairs(GrandExchange.offers() or {}) do
		if offer.state == "empty" and shown(part(FIRST_SLOT_CHILD + offer.slot, SLOT_PART.BUY)) then
			return offer.slot
		end
	end
	return nil
end

function GrandExchange.can_collect(): boolean
	return GrandExchange.is_overview() and shown(part(TOP_BAR_CHILD, COLLECT_INDEX)) ~= nil
end

function GrandExchange.collect(to_bank: boolean?): boolean
	local button = if GrandExchange.can_collect() then part(TOP_BAR_CHILD, COLLECT_INDEX) else nil
	if button == nil then
		return false
	end
	local option = if to_bank then "Collect to bank" else "Collect to inventory"
	if not Interact.with(button, option, nil, false) then
		return false
	end
	return Widgets.wait_for(function(): boolean
		return not GrandExchange.can_collect()
	end, STEP_TIMEOUT_MS, POLL_MS)
end

local function prompt(): string?
	local w = shown(widget(CHATBOX_GROUP, PROMPT_CHILD))
	return if w then w.text else nil
end

function GrandExchange.start_buy(slot: number?): boolean
	if not GrandExchange.is_overview() then
		return false
	end
	local free = slot or GrandExchange.free_slot()
	local button = if free then shown(part(FIRST_SLOT_CHILD + free, SLOT_PART.BUY)) else nil
	if button == nil then
		Log.log(NAME, "start_buy: no free slot to buy in")
		return false
	end
	if not Interact.with(button, BUY_OPTION, nil, false) then
		return false
	end
	return Widgets.wait_for(function(): boolean
		return GrandExchange.is_setup() and prompt() ~= nil
	end, STEP_TIMEOUT_MS, POLL_MS)
end

local function search_result(name: string): (Widget?, number?)
	local wanted = string.lower(name)
	local kids: { Widget } = widget_children(CHATBOX_GROUP, SEARCH_RESULTS_CHILD) or {}
	for i = 2, #kids - 1 do
		local select, label, icon = kids[i - 1], kids[i], kids[i + 1]
		if label.text and string.lower(label.text) == wanted and not select.hidden and icon.item_id ~= -1 then
			return select, icon.item_id
		end
	end
	return nil, nil
end

local function settled_result(name: string): (Widget?, number?)
	local result: Widget?, id: number? = nil, nil
	local seen, since = "", Host.time()
	Widgets.wait_for(function(): boolean
		result, id = search_result(name)
		local key = if result then `{id}@{result.x},{result.y}` else ""
		if key ~= seen then
			seen, since = key, Host.time()
		end
		return result ~= nil and Host.time() - since >= SEARCH_SETTLE_MS
	end, SEARCH_TIMEOUT_MS, POLL_MS)
	if result == nil or Host.time() - since < SEARCH_SETTLE_MS then
		return nil, nil
	end
	return result, id
end

function GrandExchange.setup_item(): number?
	local w = if GrandExchange.is_setup() then part(SETUP_CHILD, SETUP_PART.ITEM) else nil
	return if w and w.item_id ~= -1 then w.item_id else nil
end

function GrandExchange.choose(name: string): boolean
	if not GrandExchange.is_setup() or prompt() == nil then
		return false
	end
	Keyboard.send(string.lower(name))
	local result, id = settled_result(name)
	if result == nil then
		Log.log(NAME, `choose: no search result called {name}`)
		return false
	end
	if not Interact.with(result, "Select", nil, false) then
		return false
	end
	local ready = Widgets.wait_for(function(): boolean
		return GrandExchange.setup_item() ~= nil and GrandExchange.quantity() ~= nil and GrandExchange.price() ~= nil
	end, STEP_TIMEOUT_MS, POLL_MS)
	if ready and GrandExchange.setup_item() ~= id then
		Log.log(NAME, `choose: selected item {GrandExchange.setup_item()} instead of {name} ({id})`)
		return false
	end
	return ready
end

function GrandExchange.quantity(): number?
	return if GrandExchange.is_setup() then number_in(text_at(SETUP_CHILD, SETUP_PART.QUANTITY)) else nil
end

function GrandExchange.price(): number?
	return if GrandExchange.is_setup() then number_in(text_at(SETUP_CHILD, SETUP_PART.PRICE)) else nil
end

function GrandExchange.total(): number?
	return if GrandExchange.is_setup() then number_in(text_at(SETUP_CHILD, SETUP_PART.TOTAL)) else nil
end

local function click_setup(index: number, option: string): boolean
	local button = shown(part(SETUP_CHILD, index))
	if button == nil then
		return false
	end
	local before = GrandExchange.total()
	if not Interact.with(button, option, nil, false) then
		return false
	end
	Widgets.wait_for(function(): boolean
		return GrandExchange.total() ~= before
	end, STEP_TIMEOUT_MS, POLL_MS)
	pause()
	return true
end

local BUTTONS = table.freeze({
	{ step = 1000, index = SETUP_PART.PLUS_THOUSAND, option = "+1K" },
	{ step = 100, index = SETUP_PART.PLUS_HUNDRED, option = "+100" },
	{ step = 10, index = SETUP_PART.PLUS_TEN, option = "+10" },
	{ step = 1, index = SETUP_PART.PLUS_ONE, option = "+1" },
})

local function button_clicks(from: number, to: number): { typeof(BUTTONS[1]) }?
	if to < from then
		return nil
	end
	local clicks = {}
	local left = to - from
	for _, button in ipairs(BUTTONS) do
		while left >= button.step do
			table.insert(clicks, button)
			left -= button.step
		end
	end
	return clicks
end

function GrandExchange.set_quantity(amount: number, typed: boolean?): boolean
	local current = GrandExchange.quantity()
	if current == nil or amount < 1 or amount ~= math.floor(amount) then
		return false
	end
	if current == amount then
		return true
	end
	local clicks = button_clicks(current, amount)
	local typing = typed
	if typing == nil then
		typing = clicks == nil or #clicks > Random.int(MAX_BUTTON_CLICKS[1], MAX_BUTTON_CLICKS[2])
	end
	if not typing and clicks then
		for _, button in ipairs(clicks) do
			if not click_setup(button.index, button.option) then
				return false
			end
		end
		return GrandExchange.quantity() == amount
	end
	local before = prompt()
	local button = shown(part(SETUP_CHILD, SETUP_PART.ENTER_QUANTITY))
	if button == nil or not Interact.with(button, "Enter quantity", nil, false) then
		return false
	end
	if
		not Widgets.wait_for(function(): boolean
			return prompt() ~= nil and prompt() ~= before
		end, STEP_TIMEOUT_MS, POLL_MS)
	then
		Log.log(NAME, "set_quantity: the quantity prompt didn't open")
		return false
	end
	pause()
	Keyboard.send(tostring(amount), Keyboard.keys.RETURN)
	return Widgets.wait_for(function(): boolean
		return GrandExchange.quantity() == amount
	end, STEP_TIMEOUT_MS, POLL_MS)
end

function GrandExchange.adjust_price(percent: number): boolean
	if not GrandExchange.is_setup() or percent % PERCENT_STEP ~= 0 or math.abs(percent) > MAX_PERCENT then
		return false
	end
	local index = if percent > 0 then SETUP_PART.PLUS_FIVE else SETUP_PART.MINUS_FIVE
	local option = if percent > 0 then "+5%" else "-5%"
	for _ = 1, math.abs(percent) // PERCENT_STEP do
		if not click_setup(index, option) then
			return false
		end
	end
	return true
end

function GrandExchange.confirm(): boolean
	local button = if GrandExchange.is_setup() then shown(widget(GROUP, CONFIRM_CHILD)) else nil
	if button == nil then
		return false
	end
	if not Interact.with(button, "Confirm", nil, false) then
		return false
	end
	return Widgets.wait_for(GrandExchange.is_overview, STEP_TIMEOUT_MS, POLL_MS)
end

function GrandExchange.back(): boolean
	if not GrandExchange.is_setup() then
		return GrandExchange.is_overview()
	end
	local button = shown(widget(GROUP, BACK_CHILD))
	if button == nil or not Interact.with(button, "Back", nil, false) then
		return false
	end
	return Widgets.wait_for(GrandExchange.is_overview, STEP_TIMEOUT_MS, POLL_MS)
end

function GrandExchange.buy(name: string, amount: number, percent: number?): (number?, string?)
	if not GrandExchange.is_overview() then
		return nil, "the Grand Exchange isn't open on its offers"
	end
	local slot = GrandExchange.free_slot()
	if slot == nil then
		return nil, "no free offer slot"
	end
	local function fail(reason: string): (number?, string?)
		GrandExchange.back()
		return nil, reason
	end
	if not GrandExchange.start_buy(slot) then
		return fail("couldn't start a buy offer")
	end
	if not GrandExchange.choose(name) then
		return fail(`couldn't choose {name}`)
	end
	if not GrandExchange.set_quantity(amount) then
		return fail(`couldn't set the quantity to {amount}`)
	end
	if percent and percent ~= 0 and not GrandExchange.adjust_price(percent) then
		return fail(`couldn't move the price by {percent}%`)
	end
	local total = GrandExchange.total()
	if total == nil or total > (Inventory.count_ex("Coins") or 0) then
		return fail(`not enough coins in the inventory for {total or "the offer"}`)
	end
	if not GrandExchange.confirm() then
		return fail("the offer wasn't confirmed")
	end
	local placed = Widgets.wait_for(function(): boolean
		local offer = GrandExchange.offer(slot)
		return offer ~= nil and offer.state ~= "empty"
	end, STEP_TIMEOUT_MS, POLL_MS)
	if not placed then
		return nil, `slot {slot} didn't show the offer`
	end
	return slot, nil
end

function GrandExchange.wait_done(slot: number, timeout_ms: number): Offer?
	local offer: Offer? = nil
	Widgets.wait_for(function(): boolean
		offer = GrandExchange.offer(slot)
		return offer ~= nil and (offer.state == "bought" or offer.state == "sold" or offer.state == "aborted")
	end, timeout_ms, POLL_MS * 4)
	return offer
end

return table.freeze(GrandExchange)
