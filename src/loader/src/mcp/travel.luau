--!strict
local Host = require("@volcren/host")
local Transports = require("@volcren/transports")
local Walker = require("@volcren/walker")

local args: { [string]: any } = Host.settings() or {}
local here = Walker.here()
if here == nil then
	print("The player isn't logged in.")
	return
end
local goal = { x = args.x :: number, y = args.y :: number, plane = (args.plane or here.plane) :: number }
local near: number = args.near or 2

local function tile(t: { x: number, y: number, plane: number }?): string
	return if t then `{t.x}, {t.y}, plane {t.plane}` else "anywhere"
end

if not args.plan then
	print(`Travelling from {tile(here)} to {tile(goal)}.`)
	local ok, arrived = pcall(function()
		return Walker.walk({ x = goal.x, y = goal.y }, { plane = goal.plane, near = near })
	end)
	local at = Walker.here()
	if not ok then
		print(`Couldn't travel: {arrived}`)
	else
		print(if arrived then "Arrived." else "Didn't arrive.")
	end
	print(`You are at {tile(at)}.`)
	return
end

print(`Plan from {tile(here)} to {tile(goal)}:`)
local at: { x: number, y: number, plane: number } = here
local estimate: number? = nil
for hop = 1, Walker.TRANSPORT_HOPS do
	local choice = Transports.choose(at, goal, { prefer_house = Walker.PREFER_HOUSE })
	if choice == nil then
		break
	end
	local t = choice.transport
	estimate = estimate or choice.ticks
	local saving = if choice.saved >= 1e9 then "the goal can't be walked to" else `saves {choice.saved} ticks over walking`
	print(`{hop}. {t.kind}: {t.name} ({t.menu}) from {tile(t.origin)} to {tile(t.destination)}, {saving}`)
	if t.items ~= "" then
		print(`   needs {t.items}`)
	end
	if t.destination == nil then
		break
	end
	at = t.destination
end
if not Transports.walkable(at, goal) then
	print(`No way on foot from {tile(at)}: the goal can't be reached with what this account can use.`)
	return
end
local plan, why = Host.nav_plan(at, goal, { near = near })
if plan == nil then
	print(`Walk from {tile(at)}: the planner found no route ({why or "no reason given"}).`)
else
	print(`Walk from {tile(at)}: {#plan.tiles} tiles{if plan.reached then "" else ", not quite reaching the goal"}.`)
	for _, action in plan.actions do
		print(`   {action.kind}: {action.loc.option} at {action.loc.x}, {action.loc.y} to plane {action.to_plane}`)
	end
end
print(`About {estimate or Transports.walk_ticks(at, goal)} ticks in all.`)
