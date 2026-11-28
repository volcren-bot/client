--!strict
local Draw = require("@volcren/draw")
local Geometry = require("@volcren/geometry")
local Host = require("@volcren/host")
local Projection = require("@volcren/projection")
type Tile = Projection.Tile
type Point = Geometry.Point
export type Spot = { x: number, y: number }

local DebugDraw = {}

local TILE = Projection.TILE

local layers: { [string]: () -> () } = {}
local kinds: { [string]: Draw.Layer } = {}
local order: { string } = {}
local recording: { Host.DebugShape }? = nil
local screen_shown = false
local world_shown = false

function DebugDraw.on(): boolean
	return Host.script_debugging()
end

function DebugDraw.render()
	local world: { Host.DebugShape } = {}
	local screen: { Host.Shape } = {}
	if DebugDraw.on() then
		for _, name in ipairs(order) do
			recording = world
			local ok, shapes = pcall(Draw.record, layers[name], kinds[name])
			recording = nil
			if not ok then
				error(shapes, 0)
			end
			table.move(shapes, 1, #shapes, #screen + 1, screen)
		end
	end
	if #screen > 0 or screen_shown then
		screen_shown = #screen > 0
		local clear: Host.Shape = { kind = "clear" }
		table.insert(screen, 1, clear)
		Host.draw_batch(screen)
	end
	if #world > 0 or world_shown then
		world_shown = #world > 0
		Host.debug_shapes(world)
	end
end

function DebugDraw.set(name: string, paint: (() -> ())?, layer: Draw.Layer?)
	if paint == nil then
		if layers[name] == nil then
			return
		end
		layers[name] = nil
		kinds[name] = nil
		table.remove(order, table.find(order, name) :: number)
	else
		if layers[name] == nil then
			table.insert(order, name)
		end
		layers[name] = paint
		kinds[name] = layer or "world"
	end
	DebugDraw.render()
end

local function shapes(): { Host.DebugShape }
	local into = recording
	if into == nil then
		error("DebugDraw world drawings only work inside a DebugDraw.set paint function")
	end
	return into
end

local function ground(spot: Spot): Host.FinePoint
	local x, y = math.round(spot.x * TILE + TILE / 2), math.round(spot.y * TILE + TILE / 2)
	return { x = x, y = y, tile = { x = x // TILE, y = y // TILE } }
end

local function path(points: { Host.FinePoint }, color: Draw.Color, width: number?)
	local shape: Host.DebugShape = {
		kind = "path",
		points = points,
		r = color.r,
		g = color.g,
		b = color.b,
		a = color.a,
		width = math.round(width or 1),
	}
	table.insert(shapes(), shape)
end

local function key(x: number, y: number): string
	return `{x},{y}`
end

function DebugDraw.outline(tiles: { Tile }, color: Draw.Color, width: number?)
	local inside: { [string]: boolean } = {}
	for _, t in ipairs(tiles) do
		inside[key(t.x, t.y)] = true
	end
	for _, t in ipairs(tiles) do
		local tile = { x = t.x, y = t.y }
		local function corner(dx: number, dy: number): Host.FinePoint
			return { x = (t.x + dx) * TILE, y = (t.y + dy) * TILE, tile = tile }
		end
		local sw, nw, ne, se = corner(0, 0), corner(0, 1), corner(1, 1), corner(1, 0)
		if not inside[key(t.x - 1, t.y)] then
			path({ sw, nw }, color, width)
		end
		if not inside[key(t.x, t.y + 1)] then
			path({ nw, ne }, color, width)
		end
		if not inside[key(t.x + 1, t.y)] then
			path({ ne, se }, color, width)
		end
		if not inside[key(t.x, t.y - 1)] then
			path({ se, sw }, color, width)
		end
	end
end

function DebugDraw.line(from: Spot, to: Spot, color: Draw.Color, width: number?)
	path({ ground(from), ground(to) }, color, width)
end

function DebugDraw.label(at: Spot, text: string, color: Draw.Color, bold: boolean?)
	local shape: Host.DebugShape = {
		kind = "label",
		at = ground(at),
		text = text,
		r = color.r,
		g = color.g,
		b = color.b,
		a = color.a,
		bold = bold,
	}
	table.insert(shapes(), shape)
end

function DebugDraw.screen_label(at: Point, text: string, color: Draw.Color, bold: boolean?)
	local shape: Host.DebugShape = {
		kind = "screen_label",
		x = math.round(at.x),
		y = math.round(at.y),
		text = text,
		r = color.r,
		g = color.g,
		b = color.b,
		a = color.a,
		bold = bold,
	}
	table.insert(shapes(), shape)
end

function DebugDraw.loc(loc: Host.Loc, faces: Draw.Color?, hull: Draw.Color?)
	local shape: Host.DebugShape = {
		kind = "loc",
		loc_kind = loc.kind,
		x = loc.x,
		y = loc.y,
		id = loc.id,
		faces = faces,
		hull = hull,
	}
	table.insert(shapes(), shape)
end

return DebugDraw
