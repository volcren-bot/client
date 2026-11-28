--!strict
local ChatOptions = require("@volcren/chat_options")
local GroundItems = require("@volcren/ground_items")
local Host = require("@volcren/host")
local Inventory = require("@volcren/inventory")
local Npcs = require("@volcren/npcs")
local Objects = require("@volcren/objects")
local Walker = require("@volcren/walker")

local SETTLE_MS = 6000
local POLL_MS = 100

local args: { [string]: any } = Host.settings() or {}
local kind: string = args.kind or ""
local target: string = args.target or ""
local using: string? = args.item
local option: string = if using then "Use" else args.option or ""

local function what(text: string): string | number
	return tonumber(text) or text
end

local function settle()
	local started = Host.time()
	local last = Walker.here()
	local still_since = started
	while Host.time() - started < SETTLE_MS do
		Host.sleep(POLL_MS)
		local now = Walker.here()
		if now == nil or last == nil or now.x ~= last.x or now.y ~= last.y or now.plane ~= last.plane then
			still_since = Host.time()
		elseif Host.time() - still_since >= 1200 then
			break
		end
		if ChatOptions.contains_continue() or #(ChatOptions.get() or {}) > 0 then
			break
		end
		last = now
	end
	local at = Walker.here()
	if at then
		print(`You are at {at.x}, {at.y}, plane {at.plane}.`)
	end
	local options = ChatOptions.get()
	if options and #options > 0 then
		print(`Dialogue options: {ChatOptions.to_string(options)}`)
	elseif ChatOptions.contains_continue() then
		local speech = ChatOptions.speech()
		print(if speech then `Dialogue: {speech.name}: {speech.text}` else "A message is waiting to continue.")
	end
end

local here = Walker.here()
if here == nil then
	print("The player isn't logged in.")
	return
end
if using and not Inventory.contains(what(using)) then
	print(`There's no {using} in the inventory.`)
	return
end

local function select_item(): boolean
	if using == nil then
		return true
	end
	if Inventory.select(what(using)) then
		return true
	end
	print(`Couldn't select the {using}.`)
	return false
end

local done = false
local name = target
if kind == "item" then
	if not Inventory.contains(what(target)) then
		print(`There's no {target} in the inventory.`)
		return
	end
	done = if using then Inventory.combine(what(using), what(target)) else Inventory.interact(what(target), option)
elseif kind == "npc" then
	local npc = Npcs.nearest(what(target), here)
	if npc == nil then
		print(`No NPC matching "{target}" nearby.`)
		return
	end
	name = npc.name
	print(`Found {npc.name} (id {npc.id}) at {npc.x}, {npc.y}.`)
	done = select_item() and Npcs.interact(npc, option)
elseif kind == "object" then
	local loc = Objects.nearest(what(target), here)
	if loc == nil then
		print(`No object matching "{target}" nearby.`)
		return
	end
	name = loc.name or target
	print(`Found {name} (id {loc.id}) at {loc.x}, {loc.y}, plane {loc.plane}.`)
	done = select_item() and Objects.interact(loc, option)
elseif kind == "ground_item" then
	local item = GroundItems.nearest(what(target), here)
	if item == nil then
		print(`No ground item matching "{target}" nearby.`)
		return
	end
	name = item.name or target
	print(`Found {name} x{item.quantity} (id {item.id}) at {item.x}, {item.y}.`)
	done = select_item() and GroundItems.interact(item, option)
else
	print(`Unknown kind "{kind}": use npc, object, ground_item or item.`)
	return
end

local action = if using then `Use {using} on {name}` else `{option} on {name}`
if not done then
	if using and Inventory.selected() ~= nil then
		Inventory.deselect()
	end
	print(`{action} didn't go through.`)
	return
end
print(`{action}: clicked.`)
settle()
