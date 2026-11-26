--!strict
local Host = require("@volcren/host")
type InventoryLayout = Host.InventoryLayout
type InventoryLayoutSlot = Host.InventoryLayoutSlot
type LayoutItem = Host.LayoutItem
local BankPouch = require("@volcren/bankpouch")
local Bankmain = require("@volcren/bankmain")
local Biometrics = require("@volcren/biometrics")
local Inv = require("@volcren/inv")
local Inventory = require("@volcren/inventory")
local Item = require("@volcren/item")
local LayoutItems = require("@volcren/layout_item")
local Log = require("@volcren/log")
local Random = require("@volcren/random")
local Runepouch = require("@volcren/runepouch")
local Widgets = require("@volcren/widgets")

local InventoryLayout = {}

export type Layout = InventoryLayout
export type Slot = InventoryLayoutSlot
export type Rune = LayoutItem

export type Entry = {
	items: { Item.Item },
	stackable: boolean,
	min: number,
	max: number,
	copies: number,
	label: string,
}

InventoryLayout.SIZE = 28
InventoryLayout.POUCH_TIMEOUT_MS = 2500
InventoryLayout.POUCH_LOAD_BASE = 0.7
InventoryLayout.POUCH_SLOTS = 3
InventoryLayout.DIVINE_POUCH_SLOTS = 4
InventoryLayout.DIVINE_POUCHES = table.freeze({ "Divine rune pouch", "Divine rune pouch (l)" } :: { Item.Item })
InventoryLayout.POLL_MS = 50

function InventoryLayout.from_client(name: string?): Layout?
	local items = Inventory.items()
	if items == nil then
		return nil
	end
	local slots: { Slot } = {}
	for _, s in ipairs(items) do
		if s.id ~= -1 then
			local stackable = s.quantity > 1
			local amount = if stackable then s.quantity else 1
			table.insert(slots, {
				slot = s.slot,
				id = s.id,
				ids = { s.id },
				name = s.name or "",
				stackable = stackable,
				min = amount,
				max = amount,
				locked = false,
			})
		end
	end
	local pouch: { Rune }? = nil
	local held = if Runepouch.held() then Runepouch.runes() else nil
	if held then
		local runes: { Rune } = {}
		for _, rune in ipairs(held) do
			table.insert(runes, {
				id = rune.id,
				ids = { rune.id },
				name = rune.name,
				stackable = true,
				min = rune.quantity,
				max = rune.quantity,
				locked = false,
			})
		end
		pouch = runes
	end
	return { name = name or "From client", slots = slots, pouch = pouch }
end

function InventoryLayout.entries(layout: Layout): { Entry }
	local by_key: { [string]: Entry } = {}
	local list: { Entry } = {}
	for _, slot in ipairs(layout.slots) do
		local items = LayoutItems.items(slot)
		local parts: { string } = {}
		for i, item in ipairs(items) do
			parts[i] = tostring(item)
		end
		local key = (if slot.stackable then "stack:" else "copies:") .. table.concat(parts, ",")
		local entry = by_key[key]
		if entry == nil then
			entry = {
				items = items,
				stackable = slot.stackable,
				min = slot.min,
				max = slot.max,
				copies = 0,
				label = if slot.name ~= "" then slot.name else `item {slot.id}`,
			}
			by_key[key] = entry
			table.insert(list, entry)
		end
		entry.copies += 1
	end
	return list
end

local function pouch_missing(layout: Layout): { [number]: number }?
	local missing: { [number]: number } = {}
	if layout.pouch == nil then
		return missing
	end
	local held = Runepouch.runes()
	if held == nil then
		return nil
	end
	local have: { [number]: number } = {}
	for _, rune in ipairs(held) do
		have[rune.id] = rune.quantity
	end
	for _, rune in ipairs(layout.pouch :: { Rune }) do
		local need = LayoutItems.top_up(rune, have[rune.id] or 0)
		if need then
			missing[rune.id] = need
		end
	end
	return missing
end

local function entry_matches(entry: Entry): boolean?
	if entry.stackable then
		local have = Inventory.count_ex(entry.items)
		return if have == nil then nil else LayoutItems.in_range(entry, have)
	end
	local have = Inventory.count(entry.items)
	return if have == nil then nil else have >= entry.copies
end

function InventoryLayout.matches(layout: Layout): boolean?
	for _, entry in ipairs(InventoryLayout.entries(layout)) do
		local ok = entry_matches(entry)
		if ok ~= true then
			return ok
		end
	end
	local missing = pouch_missing(layout)
	if missing == nil then
		return nil
	end
	return next(missing) == nil
end

local function deposit_extra(entry: Entry): boolean
	if entry.stackable then
		local extra = LayoutItems.excess(entry, Inventory.count_ex(entry.items) or 0)
		return extra == 0 or Bankmain.deposit(entry.items, extra)
	end
	local extra = (Inventory.count(entry.items) or 0) - entry.copies
	for i = #entry.items, 1, -1 do
		if extra <= 0 then
			break
		end
		local held = Inventory.count(entry.items[i]) or 0
		if held > 0 then
			if not Bankmain.deposit(entry.items[i], math.min(held, extra)) then
				return false
			end
			extra = (Inventory.count(entry.items) or 0) - entry.copies
		end
	end
	return true
end

function InventoryLayout.deposit_diff(layout: Layout): boolean
	if not Bankmain.is_open() then
		Log.log("InventoryLayout", "deposit_diff: the bank isn't open")
		return false
	end
	local entries = InventoryLayout.entries(layout)
	local keep: { Item.Item } = {}
	for _, entry in ipairs(entries) do
		for _, item in ipairs(entry.items) do
			table.insert(keep, item)
		end
	end
	if #keep == 0 then
		return Bankmain.deposit_inventory()
	end
	if not Bankmain.deposit_different(keep) then
		return false
	end
	for _, entry in ipairs(entries) do
		if not deposit_extra(entry) then
			return false
		end
	end
	return true
end

local function take(entry: Entry, buttons: boolean): boolean
	if entry.stackable then
		local amount = LayoutItems.top_up(entry, Inventory.count_ex(entry.items) or 0)
		return amount == nil or Bankmain.withdraw(entry.items, amount, false, buttons)
	end
	local need = entry.copies - (Inventory.count(entry.items) or 0)
	for _, item in ipairs(entry.items) do
		if need <= 0 then
			break
		end
		local stock = Bankmain.count_ex(item) or 0
		if stock > 0 then
			if not Bankmain.withdraw(item, math.min(need, stock), false, buttons) then
				return false
			end
			need = entry.copies - (Inventory.count(entry.items) or 0)
		end
	end
	return need <= 0
end

local function custom_amount(entry: Entry): number?
	local amount: number?
	if entry.stackable then
		amount = LayoutItems.top_up(entry, Inventory.count_ex(entry.items) or 0)
	else
		amount = entry.copies - (Inventory.count(entry.items) or 0)
	end
	if amount == nil or amount <= 0 or Bankmain.quantity_of(amount) ~= "CUSTOM" then
		return nil
	end
	return if amount < (Bankmain.count_ex(entry.items) or 0) then amount else nil
end

local function withdraw_plan(entries: { Entry }): { { entry: Entry, shared: boolean } }
	local amounts: { [number]: number } = {}
	local uses: { [number]: number } = {}
	for i, entry in ipairs(entries) do
		local amount = custom_amount(entry)
		if amount then
			amounts[i] = amount
			uses[amount] = (uses[amount] or 0) + 1
		end
	end
	local groups: { { number } } = {}
	local group_of: { [number]: number } = {}
	local rest: { number } = {}
	for i, _ in ipairs(entries) do
		local amount = amounts[i]
		if amount and uses[amount] > 1 then
			if group_of[amount] == nil then
				local group: { number } = {}
				table.insert(groups, group)
				group_of[amount] = #groups
			end
			table.insert(groups[group_of[amount]], i)
		else
			table.insert(rest, i)
		end
	end
	local plan: { { entry: Entry, shared: boolean } } = {}
	for _, group in ipairs(groups) do
		for _, i in ipairs(group) do
			table.insert(plan, { entry = entries[i], shared = true })
		end
	end
	for _, i in ipairs(rest) do
		table.insert(plan, { entry = entries[i], shared = false })
	end
	return plan
end

local function missing(entry: Entry): boolean
	if entry.stackable then
		return LayoutItems.top_up(entry, Inventory.count_ex(entry.items) or 0) ~= nil
	end
	return entry.copies > (Inventory.count(entry.items) or 0)
end

local function fills_with_click(entry: Entry): boolean
	if entry.stackable or Bankmain.notes() ~= false then
		return false
	end
	local free = InventoryLayout.SIZE - (Inventory.count_used() or InventoryLayout.SIZE)
	local need = entry.copies - (Inventory.count(entry.items) or 0)
	local selected = Bankmain.selected_amount()
	return free > 0
		and need >= free
		and selected ~= nil
		and selected >= free
		and (Bankmain.count_ex(entry.items) or 0) >= free
end

local function pouch_slot(layout: Layout): Slot?
	for _, slot in ipairs(layout.slots) do
		if table.find(Runepouch.POUCHES, slot.name) then
			return slot
		end
	end
	return nil
end

local function pouch_filled(layout: Layout): boolean
	local left = pouch_missing(layout)
	return left ~= nil and next(left) == nil
end

local function pouch_ids(layout: Layout): { number }
	local ids: { number } = {}
	for _, rune in ipairs(layout.pouch or {}) do
		table.insert(ids, rune.id)
	end
	return ids
end

local function same_runes(first: { number }, second: { number }): boolean
	if #first ~= #second then
		return false
	end
	for _, id in ipairs(first) do
		if not table.find(second, id) then
			return false
		end
	end
	return true
end

local function saved_loadout(layout: Layout): BankPouch.Loadout?
	local wanted = pouch_ids(layout)
	for _, loadout in ipairs(BankPouch.loadouts() or {}) do
		if #loadout.runes > 0 and same_runes(loadout.runes, wanted) then
			return loadout
		end
	end
	return nil
end

local function prefers_loadout(): boolean
	local base = InventoryLayout.POUCH_LOAD_BASE
	return Random.chance(base + (1 - base) * Biometrics.get_probability(Biometrics.CHOOSEOPTION))
end

local function load_saved(layout: Layout): boolean
	local loadout = saved_loadout(layout)
	if loadout == nil or not prefers_loadout() then
		return false
	end
	local label = if loadout.name then `{loadout.number} ({loadout.name})` else tostring(loadout.number)
	Log.log("InventoryLayout", `withdraw: loading the pouch's saved loadout {label}`)
	if not BankPouch.load(loadout.number) then
		Log.log("InventoryLayout", "withdraw: the saved loadout didn't load, filling the pouch by hand")
		return false
	end
	Widgets.wait_within(function(): boolean
		return pouch_filled(layout)
	end, InventoryLayout.POUCH_TIMEOUT_MS, InventoryLayout.POLL_MS)
	return true
end

local function make_room(layout: Layout): boolean
	local missing = pouch_missing(layout)
	local held = Runepouch.runes()
	if missing == nil or held == nil then
		return false
	end
	local wanted: { [number]: boolean } = {}
	for _, id in ipairs(pouch_ids(layout)) do
		wanted[id] = true
	end
	local held_ids: { [number]: boolean } = {}
	local unwanted: { Runepouch.Rune } = {}
	for _, rune in ipairs(held) do
		held_ids[rune.id] = true
		if not wanted[rune.id] then
			table.insert(unwanted, rune)
		end
	end
	local new_count = 0
	for id in pairs(missing) do
		if not held_ids[id] then
			new_count += 1
		end
	end
	local slots = if Inventory.contains_any(InventoryLayout.DIVINE_POUCHES)
		then InventoryLayout.DIVINE_POUCH_SLOTS
		else InventoryLayout.POUCH_SLOTS
	local bank_count = math.clamp(new_count - (slots - #held), 0, #unwanted)
	if bank_count == 0 then
		return true
	end
	local banked: { Runepouch.Rune } = {}
	local names: { string } = {}
	for i = 1, bank_count do
		local rune = unwanted[i]
		table.insert(banked, rune)
		table.insert(names, if rune.name ~= "" then rune.name else `rune {rune.id}`)
	end
	Log.log("InventoryLayout", `withdraw: banking {table.concat(names, ", ")} from the pouch to make room`)
	if #banked == #held then
		return BankPouch.deposit_runes()
	end
	for _, rune in ipairs(banked) do
		if not BankPouch.deposit(rune.id) then
			return false
		end
	end
	return true
end

local function fill_pouch(layout: Layout): boolean
	local missing = pouch_missing(layout)
	if missing == nil then
		return false
	end
	if next(missing) == nil then
		return true
	end
	if not Runepouch.held() then
		local pouch = pouch_slot(layout)
		if pouch == nil or not Bankmain.withdraw(pouch.id, 1, false) then
			Log.log(
				"InventoryLayout",
				"withdraw: the layout has runes for a pouch, but no pouch is in the inventory or the bank"
			)
			return false
		end
	end
	if not BankPouch.open() then
		Log.log("InventoryLayout", "withdraw: couldn't open the rune pouch in the bank")
		return false
	end
	if not load_saved(layout) and not make_room(layout) then
		Log.log("InventoryLayout", "withdraw: couldn't bank the pouch runes the layout doesn't list")
		BankPouch.close()
		return false
	end
	local left = pouch_missing(layout)
	if left == nil then
		BankPouch.close()
		return false
	end
	for id, need in pairs(left) do
		if not BankPouch.withdraw(id, need) then
			BankPouch.close()
			return false
		end
	end
	local filled = Widgets.wait_within(function(): boolean
		return pouch_filled(layout)
	end, InventoryLayout.POUCH_TIMEOUT_MS, InventoryLayout.POLL_MS)
	if not BankPouch.close() then
		return false
	end
	if not filled then
		Log.log("InventoryLayout", "withdraw: the pouch didn't take every rune")
	end
	return filled
end

function InventoryLayout.withdraw(layout: Layout): boolean
	if not Bankmain.is_open() then
		Log.log("InventoryLayout", "withdraw: the bank isn't open")
		return false
	end
	if not BankPouch.close() then
		return false
	end
	if InventoryLayout.matches(layout) then
		return true
	end
	if (Inventory.count_used() or 0) > 0 and not InventoryLayout.deposit_diff(layout) then
		return false
	end
	if not fill_pouch(layout) then
		return false
	end
	local plan = withdraw_plan(InventoryLayout.entries(layout))
	local last = 0
	for i, step in ipairs(plan) do
		if missing(step.entry) then
			last = i
		end
	end
	for i, step in ipairs(plan) do
		if i == last and fills_with_click(step.entry) then
			if not Bankmain.withdraw_selected(step.entry.items) then
				Log.log("InventoryLayout", `withdraw: couldn't take {step.entry.label}`)
				return false
			end
			continue
		end
		local amount = custom_amount(step.entry)
		local buttons = step.shared or amount == nil or Bankmain.custom_quantity() == amount
		if not take(step.entry, buttons) then
			Log.log("InventoryLayout", `withdraw: couldn't take {step.entry.label}`)
			return false
		end
	end
	return InventoryLayout.matches(layout) == true
end

function InventoryLayout.reorder(layout: Layout): boolean
	local by_slot: { [number]: Slot } = {}
	for _, slot in ipairs(layout.slots) do
		by_slot[slot.slot] = slot
	end
	for destination = 0, InventoryLayout.SIZE - 1 do
		local wanted = by_slot[destination]
		if wanted == nil then
			continue
		end
		local held = Inventory.items()
		if held == nil then
			return false
		end
		local at_slot: { [number]: Inv.Slot } = {}
		for _, s in ipairs(held) do
			at_slot[s.slot] = s
		end
		local function settled(at: number): boolean
			local here, now = by_slot[at], at_slot[at]
			return here ~= nil and now ~= nil and LayoutItems.counts_as(here, now.id, now.name)
		end
		if settled(destination) then
			continue
		end
		local from: number? = nil
		for _, at in ipairs(Inventory.indices_of(LayoutItems.items(wanted)) or {}) do
			if at ~= destination and not settled(at) then
				from = at
				break
			end
		end
		if from == nil then
			Log.log("InventoryLayout", `reorder: nothing to put in slot {destination} ({wanted.name})`)
			return false
		end
		if not Inventory.move_slot(from, destination) then
			return false
		end
	end
	return true
end

return InventoryLayout
