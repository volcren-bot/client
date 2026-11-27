--!strict
local Host = require("@volcren/host")
type Model = Host.NpcModel
type ScreenPoint = Host.ScreenPoint
type Tile = Host.Tile
local Geometry = require("@volcren/geometry")
local Point = Geometry.Point
local Polygon = Geometry.Polygon
local Quad = Geometry.Quad
local Triangle = Geometry.Triangle
type Point = Geometry.Point
type Polygon = Geometry.Polygon
type Quad = Geometry.Quad
local Box = Geometry.Box
type Box = Geometry.Box
local Camera = require("@volcren/camera")
local Draw = require("@volcren/draw")
local Interact = require("@volcren/interact")
local Log = require("@volcren/log")
local Minimenu = require("@volcren/minimenu")
local Mouse = require("@volcren/mouse")
local Player = require("@volcren/player")
local Projection = require("@volcren/projection")
local Random = require("@volcren/random")
local Toplevel = require("@volcren/toplevel")
local Track = require("@volcren/track")
local Walker = require("@volcren/walker")
local game_state = Host.game_state

export type Clickbox = {
	polygon: Polygon,
	bounding_box: boolean,
	box: { bottom: Quad, top: Quad }?,
	triangles: { Polygon }?,
	anchor: Point?,
}

export type Aim = (lead_ms: number?) -> Clickbox?

export type Anchor = { x: number, y: number, size: number?, height: number? }

export type ActTarget = {
	model: Host.ActModel,
	entry: { kind: number?, id: number?, index: number?, x: number?, y: number? },
}

export type Approach = {
	near: number?,
	walk: Walker.WalkOptions?,
	timeout_ms: number?,
	arrive_ms: (number | false)?,
	settle: boolean?,
	closer: number?,
	spam: number?,
	stand: { Tile }?,
}

local Clickbox = {}

Clickbox.TARGET_TRIES = 30

local function points(screen: { ScreenPoint }): { Point }
	local out: { Point } = {}
	for i, p in ipairs(screen) do
		out[i] = Point.new(math.round(p.x), math.round(p.y))
	end
	return out
end

local function triangles_of(model: Model): { Polygon }
	local triangles = table.create(#model.faces) :: { Polygon }
	for i, face in ipairs(model.faces) do
		triangles[i] = points(face)
	end
	return triangles
end

function Clickbox.from_model(model: Model, what: string): Clickbox?
	if model.bounding_box then
		if #model.corners < 8 then
			return nil
		end
		local c = points(model.corners)
		local has_faces = #model.faces > 0 and #model.hull >= 3
		return {
			polygon = if has_faces then points(model.hull) else Polygon.convex_hull(c),
			bounding_box = true,
			box = { bottom = Quad.new(c[3], c[4], c[8], c[7]), top = Quad.new(c[1], c[2], c[6], c[5]) },
			triangles = if has_faces then triangles_of(model) else nil,
		}
	end
	if #model.hull < 3 or #model.faces == 0 then
		return nil
	end
	return { polygon = points(model.hull), bounding_box = false, triangles = triangles_of(model) }
end

function Clickbox.from_cuboid(cuboid: { bottom: Quad, top: Quad }): Clickbox
	local corners: { Point } = {}
	for _, quad in ipairs({ cuboid.bottom, cuboid.top }) do
		for _, p in ipairs(Quad.corners(quad)) do
			table.insert(corners, p)
		end
	end
	return { polygon = Polygon.convex_hull(corners), bounding_box = true, box = cuboid, triangles = nil }
end

Clickbox.MIN_TRIANGLE_AREA = 4
Clickbox.CENTER_SPREAD = 0.45
Clickbox.MOUSE_PULL = 2

type Center = { x: number, y: number, sx: number, sy: number }

local function aimable_area(box: Clickbox, t: Polygon, visible: (Point) -> boolean): number?
	local area = Triangle.area(t)
	if area >= Clickbox.MIN_TRIANGLE_AREA then
		local center = Triangle.centroid(t)
		if Polygon.contains(box.polygon, center) and visible(center) then
			return area
		end
	end
	return nil
end

local function center_of(list: { Polygon }, indices: { number }): Center?
	local total, x, y = 0, 0, 0
	for _, i in ipairs(indices) do
		local area, c = Triangle.area(list[i]), Triangle.centroid(list[i])
		total += area
		x += area * c.x
		y += area * c.y
	end
	if total <= 0 then
		return nil
	end
	x, y = x / total, y / total
	local vx, vy = 0, 0
	for _, i in ipairs(indices) do
		local area, c = Triangle.area(list[i]), Triangle.centroid(list[i])
		vx += area * (c.x - x) ^ 2
		vy += area * (c.y - y) ^ 2
	end
	return { x = x, y = y, sx = math.max(math.sqrt(vx / total), 1), sy = math.max(math.sqrt(vy / total), 1) }
end

local function centered_index(list: { Polygon }, indices: { number }, center: Center, from: Point?): number?
	local sx, sy = center.sx * Clickbox.CENTER_SPREAD, center.sy * Clickbox.CENTER_SPREAD
	local reach = Clickbox.MOUSE_PULL * math.max(center.sx, center.sy)
	return Random.weighted(indices, function(i: number): number
		local c = Triangle.centroid(list[i])
		local weight = Triangle.area(list[i])
			* math.exp(-0.5 * (((c.x - center.x) / sx) ^ 2 + ((c.y - center.y) / sy) ^ 2))
		if from then
			weight *= math.exp(-Point.distance(c, from) / reach)
		end
		return weight
	end)
end

local function pick_triangle(box: Clickbox, from: Point?, visible: (Point) -> boolean): Polygon?
	local list = box.triangles or {}
	local aimable: { number } = {}
	for i, t in ipairs(list) do
		if aimable_area(box, t, visible) then
			table.insert(aimable, i)
		end
	end
	local center = center_of(list, aimable)
	if center == nil then
		return nil
	end
	local index = centered_index(list, aimable, center, from)
	return if index then list[index] else nil
end

local function stable_triangles(box: Clickbox): { Polygon }?
	if box.triangles then
		return box.triangles
	end
	local cuboid = box.box
	if cuboid == nil then
		return nil
	end
	local out: { Polygon } = {}
	for _, quad in ipairs({ cuboid.top, cuboid.bottom }) do
		local c = Quad.corners(quad)
		table.insert(out, { c[1], c[2], c[3] })
		table.insert(out, { c[1], c[3], c[4] })
	end
	return out
end

Clickbox.FOLLOW_AREA = 25

local function pick_index(box: Clickbox, list: { Polygon }, from: Point?, visible: (Point) -> boolean): number?
	local candidates: { number }, small: { number }, aimable: { number } = {}, {}, {}
	for i, t in ipairs(list) do
		local area = aimable_area(box, t, visible)
		if area then
			table.insert(aimable, i)
			table.insert(if area >= Clickbox.FOLLOW_AREA then candidates else small, i)
		end
	end
	if #candidates == 0 then
		candidates = small
	end
	table.sort(candidates, function(a: number, b: number): boolean
		return Triangle.area(list[a]) > Triangle.area(list[b])
	end)
	for i = #candidates, math.max(1, math.ceil(#candidates / 2)) + 1, -1 do
		candidates[i] = nil
	end
	local center = center_of(list, aimable)
	if #candidates == 0 or center == nil then
		return nil
	end
	return centered_index(list, candidates, center, from)
end

local function visibility(keep: ((Point) -> boolean)?, ignore_windows: boolean?): ((Point) -> boolean)?
	local view = Toplevel.viewport()
	if view == nil then
		return nil
	end
	local blockers = (if ignore_windows then Toplevel.panel_blockers() else Toplevel.blockers()) or {}
	return function(p: Point): boolean
		local at = Point.new(math.round(p.x), math.round(p.y))
		return Toplevel.is_visible(at, view, blockers) and (keep == nil or keep(at))
	end
end

function Clickbox.target(
	box: Clickbox,
	what: string,
	keep: ((Point) -> boolean)?,
	ignore_windows: boolean?
): (Point?, Polygon?)
	local visible = visibility(keep, ignore_windows)
	if visible == nil then
		return nil, nil
	end
	local from = Mouse.position()
	for _ = 1, Clickbox.TARGET_TRIES do
		local p: Point?
		local t: Polygon? = nil
		if box.triangles then
			t = pick_triangle(box, from, visible)
			if t == nil then
				break
			end
			p = Triangle.random_point_near_center(t)
		else
			p = Polygon.random_weighted_point(box.polygon, from or Quad.mean((box.box :: any).top))
		end
		if p and Polygon.contains(box.polygon, p) and visible(p) then
			return p, t
		end
	end
	return nil, nil
end

Clickbox.ANCHOR_HEIGHT = 200

local function shifted(box: Clickbox, anchor: Anchor, shift: Host.FinePoint): Clickbox?
	local ref: { Host.FinePoint } = {
		{ x = anchor.x, y = anchor.y, z = 0 },
		{ x = anchor.x, y = anchor.y, z = -(anchor.height or Clickbox.ANCHOR_HEIGHT) },
	}
	local now, later = Projection.points(ref), Projection.shifted(ref, shift)
	if now == nil or later == nil or #now < 2 or #later < 2 then
		return nil
	end
	local span = Point.distance(now[1], now[2])
	local scale = if span >= 1 then Point.distance(later[1], later[2]) / span else 1
	local function move(p: Point): Point
		return Point.new(
			math.round(later[1].x + (p.x - now[1].x) * scale),
			math.round(later[1].y + (p.y - now[1].y) * scale)
		)
	end
	local function move_all(polygon: Polygon): Polygon
		local out: Polygon = {}
		for i, p in ipairs(polygon) do
			out[i] = move(p)
		end
		return out
	end
	local cuboid = box.box
	local triangles: { Polygon }? = nil
	if box.triangles then
		triangles = {}
		for i, t in ipairs(box.triangles) do
			(triangles :: { Polygon })[i] = move_all(t)
		end
	end
	return {
		polygon = move_all(box.polygon),
		bounding_box = box.bounding_box,
		box = if cuboid
			then {
				bottom = Quad.new(
					move(cuboid.bottom.top),
					move(cuboid.bottom.right),
					move(cuboid.bottom.bottom),
					move(cuboid.bottom.left)
				),
				top = Quad.new(
					move(cuboid.top.top),
					move(cuboid.top.right),
					move(cuboid.top.bottom),
					move(cuboid.top.left)
				),
			}
			else nil,
		triangles = triangles,
	}
end

local function anchor_point(at: Anchor, shift: Host.FinePoint?): Point?
	local ref: { Host.FinePoint } = { { x = at.x, y = at.y, z = 0 } }
	local projected = if shift then Projection.shifted(ref, shift) else Projection.points(ref)
	local p = projected and projected[1]
	return if p then Point.new(math.round(p.x), math.round(p.y)) else nil
end

function Clickbox.walking(model: () -> Clickbox?, anchor: () -> Anchor?, what: string): Aim
	return function(lead_ms: number?)
		local box = model()
		local at = anchor()
		local shift = if lead_ms and lead_ms > 0 then Walker.ahead(lead_ms) else nil
		if shift == nil or (shift.x == 0 and shift.y == 0) or at == nil then
			if box and at then
				box.anchor = anchor_point(at)
			end
			return box
		end
		if box then
			local moved = shifted(box, at, shift) or box
			moved.anchor = anchor_point(at, shift)
			return moved
		end
		local cuboid = Projection.cuboid(at.x, at.y, at.size, at.height, nil, shift)
		if cuboid == nil then
			return nil
		end
		return Clickbox.from_cuboid(cuboid)
	end
end

Clickbox.WALK_REACTION_MS = 30
Clickbox.WALK_HOLD_MS = 150
Clickbox.WALK_LEAD_MS = 300
Clickbox.WALK_LEAD_TOLERANCE = 40

local function lead_ms(to: Point): number
	return Mouse.travel_ms(to) + Clickbox.WALK_REACTION_MS
end

local function target_ahead(clickbox: Aim, what: string, lead: number): Point?
	local ahead = clickbox(lead)
	if ahead == nil then
		return nil
	end
	local later = clickbox(lead + Clickbox.WALK_HOLD_MS)
	if later then
		local held = Clickbox.target(ahead, what, function(p: Point): boolean
			return Polygon.contains(later.polygon, p)
		end)
		if held then
			return held
		end
	end
	return (Clickbox.target(ahead, what))
end

local function close_window_over(box: Clickbox, what: string): boolean
	if not Toplevel.has_interface() or Clickbox.target(box, what, nil, true) == nil then
		return false
	end
	Log.log("Clickbox", `closing the {Toplevel.interface_name() or "window"} that covers {what}`)
	return Toplevel.escape_clear()
end

local function aim(clickbox: Aim, what: string): Point?
	local box = clickbox()
	local target = nil
	if box then
		target = Clickbox.target(box, what)
		if target == nil and close_window_over(box, what) then
			box = clickbox()
			target = if box then Clickbox.target(box, what) else nil
		end
	end
	if not Player.in_transit() then
		return target
	end
	local lead = if target then lead_ms(target) else Clickbox.WALK_LEAD_MS
	for _ = 1, 2 do
		local p = target_ahead(clickbox, what, lead)
		if p == nil then
			break
		end
		target = p
		local next_lead = lead_ms(p)
		if math.abs(next_lead - lead) < Clickbox.WALK_LEAD_TOLERANCE then
			break
		end
		lead = next_lead
	end
	return target
end

Clickbox.CAMERA_RANGE = 15 :: number?
Clickbox.REVEAL_YAW_STEP = 5
Clickbox.REVEAL_ZOOM_STEPS = 6
Clickbox.REVEAL_ZOOM_COST = 8
Clickbox.REVEAL_SLACK = 12
Clickbox.REVEAL_MARGIN = 0.08
Clickbox.REVEAL_SHOWN = 0.85

local function on_screen(clickbox: Aim, what: string): boolean
	local box = clickbox()
	return box ~= nil and Clickbox.target(box, what) ~= nil
end

local function overlap(a: Box, b: Box): Box?
	local x1, y1 = math.max(a.x1, b.x1), math.max(a.y1, b.y1)
	local x2, y2 = math.min(a.x2, b.x2), math.min(a.y2, b.y2)
	return if x2 > x1 and y2 > y1 then Box.new(x1, y1, x2, y2) else nil
end

local function area(b: Box?): number
	return if b then (b.x2 - b.x1) * (b.y2 - b.y1) else 0
end

local function shown_share(box: Box, view: Box, blockers: { Box }): number
	local whole = area(box)
	local inside = overlap(box, view)
	if whole <= 0 or inside == nil then
		return 0
	end
	local clear = area(inside)
	for _, blocker in ipairs(blockers) do
		clear -= area(overlap(inside, blocker))
	end
	return math.max(0, clear) / whole
end

local function predicted_box(fine_x: number, fine_y: number, view: Host.CameraView): Box?
	local cuboid = Projection.cuboid(fine_x, fine_y, 1, Clickbox.ANCHOR_HEIGHT, nil, nil, view)
	if cuboid == nil then
		return nil
	end
	local box = Projection.bounds(cuboid)
	local far = 1e6
	if math.abs(box.x1) > far or math.abs(box.y1) > far or math.abs(box.x2) > far or math.abs(box.y2) > far then
		return nil
	end
	return box
end

local function lerp_box(a: Box, b: Box, t: number): Box
	return Box.new(
		a.x1 + (b.x1 - a.x1) * t,
		a.y1 + (b.y1 - a.y1) * t,
		a.x2 + (b.x2 - a.x2) * t,
		a.y2 + (b.y2 - a.y2) * t
	)
end

type Framing = { yaw: number, zoom: number, turn: number, cost: number, share: number }

local function framings(at: Tile, yaw: number, zoom: number): { Framing }?
	local base, view, blockers = Projection.base(), Toplevel.viewport(), Toplevel.blockers()
	if base == nil or view == nil or blockers == nil then
		return nil
	end
	local margin_x = (view.x2 - view.x1) * Clickbox.REVEAL_MARGIN
	local margin_y = (view.y2 - view.y1) * Clickbox.REVEAL_MARGIN
	local inset = Box.new(view.x1 + margin_x, view.y1 + margin_y, view.x2 - margin_x, view.y2 - margin_y)
	local fine_x = (at.x - base.x) * Projection.TILE + Projection.TILE / 2
	local fine_y = (at.y - base.y) * Projection.TILE + Projection.TILE / 2
	local farthest = math.min(zoom, Camera.ZOOM_MIN)
	local steps = if farthest < zoom - Camera.zoom_tolerance then Clickbox.REVEAL_ZOOM_STEPS else 0
	local step = Clickbox.REVEAL_YAW_STEP
	local turns = { 0 }
	for degrees = step - Random.int(0, step - 1), 180, step do
		table.insert(turns, degrees)
		table.insert(turns, -degrees)
	end
	local out: { Framing } = {}
	for _, turn in ipairs(turns) do
		local target = (yaw + math.round(turn * Camera.TURN / 360)) % Camera.TURN
		local near = predicted_box(fine_x, fine_y, { yaw = target })
		local far = if steps > 0 then predicted_box(fine_x, fine_y, { yaw = target, zoom = farthest }) else near
		if near and far then
			for k = 0, steps do
				local t = if steps > 0 then k / steps else 0
				local to = zoom + (farthest - zoom) * t
				table.insert(out, {
					yaw = target,
					zoom = math.round(to),
					turn = turn,
					cost = math.abs(turn) + Clickbox.REVEAL_ZOOM_COST * (zoom - to) / Camera.ZOOM_STEP,
					share = shown_share(lerp_box(near, far, t), inset, blockers),
				})
			end
		end
	end
	return out
end

local function pick_framing(options: { Framing }): Framing?
	local cheapest: number? = nil
	for _, f in ipairs(options) do
		if f.share >= Clickbox.REVEAL_SHOWN and (cheapest == nil or f.cost < cheapest) then
			cheapest = f.cost
		end
	end
	if cheapest == nil then
		local best: Framing? = nil
		for _, f in ipairs(options) do
			if best == nil or f.share > best.share or (f.share == best.share and f.cost < best.cost) then
				best = f
			end
		end
		return best
	end
	local close: { Framing } = {}
	for _, f in ipairs(options) do
		if f.share >= Clickbox.REVEAL_SHOWN and f.cost <= cheapest + Clickbox.REVEAL_SLACK then
			table.insert(close, f)
		end
	end
	return close[Random.int(1, #close)]
end

function Clickbox.reveal(clickbox: Aim, tile: () -> Tile?, what: string): boolean
	if on_screen(clickbox, what) then
		return true
	end
	local range = Clickbox.CAMERA_RANGE
	if range == nil then
		return false
	end
	local state = game_state()
	local me, at = state.position, tile()
	local yaw, zoom = Camera.yaw(), Camera.zoom()
	if me == nil or at == nil or yaw == nil or zoom == nil then
		return false
	end
	local away = math.max(math.abs(at.x - me.x), math.abs(at.y - me.y))
	if away > range then
		return false
	end
	local options = framings(at, yaw, zoom)
	local framing = if options then pick_framing(options) else nil
	if framing == nil then
		Log.log("Clickbox", `can't frame {what}: the view isn't readable`)
		return false
	end
	if framing.turn ~= 0 then
		Camera.set_yaw(framing.yaw)
	end
	if framing.zoom < zoom - Camera.zoom_tolerance then
		Camera.set_zoom(framing.zoom)
	end
	if framing.share < Clickbox.REVEAL_SHOWN then
		Log.log("Clickbox", `no yaw or zoom shows {what}; turned to the closest view`)
	end
	return on_screen(clickbox, what)
end

Clickbox.ATTEMPTS = 5

local function clear_menu(clickbox: Aim)
	if not Minimenu.is_open() then
		return
	end
	local box = clickbox()
	if box == nil or #box.polygon == 0 then
		return
	end
	local x1, y1, x2, y2 = math.huge, math.huge, -math.huge, -math.huge
	for _, p in ipairs(box.polygon) do
		x1 = math.min(x1, p.x)
		y1 = math.min(y1, p.y)
		x2 = math.max(x2, p.x)
		y2 = math.max(y2, p.y)
	end
	if Minimenu.overlaps(Geometry.Box.new(x1, y1, x2, y2)) and not Minimenu.close() then
		Log.log("Clickbox", "couldn't close the minimenu covering the target")
	end
end

function Clickbox.hover(clickbox: Aim, hovered: () -> boolean, what: string, tile: (() -> Tile?)?): boolean
	if tile then
		local at = tile()
		if at then
			Camera.face(at)
		end
		Clickbox.reveal(clickbox, tile, what)
	end
	clear_menu(clickbox)
	for _ = 1, Clickbox.ATTEMPTS do
		local box = clickbox()
		if box and Clickbox.target(box, what) == nil and close_window_over(box, what) then
			box = clickbox()
		end
		local visible = visibility()
		if box and box.triangles and visible then
			local ok, why = Track.hover_heat(function(): Track.Shape?
				local now = clickbox()
				return if now and now.triangles then { triangles = now.triangles, anchor = now.anchor } else nil
			end, visible, hovered)
			if ok then
				return true
			end
			Log.log("Clickbox", `couldn't keep the mouse on {what} ({why})`)
			continue
		end
		local list = if box then stable_triangles(box) else nil
		local index = if box and list and visible then pick_index(box, list, Mouse.position(), visible) else nil
		if index == nil then
			return false
		end
		local ok, why = Track.hover(function(): { Polygon }?
			local now = clickbox()
			return if now then stable_triangles(now) else nil
		end, index, visible, true, hovered)
		if ok then
			return true
		end
		Log.log("Clickbox", `couldn't keep the mouse on {what} ({why})`)
	end
	return false
end

function Clickbox.hover_async(clickbox: Aim, what: string): boolean
	clear_menu(clickbox)
	local target = aim(clickbox, what)
	if target == nil then
		return false
	end
	Mouse.move_async(target)
	return true
end

function Clickbox.click(
	clickbox: Aim,
	hovered: () -> boolean,
	what: string,
	button: Mouse.Button?,
	tile: (() -> Tile?)?,
	spam: number?
): boolean
	local ok = Clickbox.hover(clickbox, hovered, what, tile)
	if not ok then
		return false
	end
	Mouse.click(nil, button)
	Interact.spam_click(spam, hovered, button)
	return true
end

Clickbox.MISCLICK_ATTEMPTS = 3

local function option_list(option: string | { string }): { string }
	return if type(option) == "string" then { option } else option :: { string }
end

local function left_click_is(options: { string }, target: Minimenu.Target?): boolean
	local entries = Minimenu.entries()
	local index = Minimenu.left_click_index(entries)
	local entry = entries and index and entries[index]
	if entry == nil then
		return false
	end
	for _, option in ipairs(options) do
		if Minimenu.matches(entry, option, target) then
			return true
		end
	end
	return false
end

local function did_any(clicked: Host.MenuClick, options: { string }, target: Minimenu.Target?): boolean
	for _, option in ipairs(options) do
		if Minimenu.matches(clicked.entry, option, target) then
			return true
		end
	end
	return false
end

local function offered(options: { string }, target: Minimenu.Target?): string?
	for _, option in ipairs(options) do
		if Minimenu.find(option, target) ~= nil then
			return option
		end
	end
	return nil
end

Clickbox.ACT_TIMEOUT_MS = 3000
Clickbox.ACT_NEAR_SHARE = 0.4
Clickbox.ACT_NEAR_MIN = 3

local function swapped(what: string): boolean
	local name = string.lower(what)
	for _, swap in ipairs(Minimenu.swaps()) do
		if string.lower(swap.target) == name then
			return true
		end
	end
	return false
end

local function act_near(box: Clickbox): number
	local x1, y1, x2, y2 = math.huge, math.huge, -math.huge, -math.huge
	for _, p in ipairs(box.polygon) do
		x1 = math.min(x1, p.x)
		y1 = math.min(y1, p.y)
		x2 = math.max(x2, p.x)
		y2 = math.max(y2, p.y)
	end
	return math.max(Clickbox.ACT_NEAR_MIN, Clickbox.ACT_NEAR_SHARE * math.min(x2 - x1, y2 - y1))
end

type Acted = "done" | "retry" | "unusable"

local function act_once(
	act: ActTarget,
	clickbox: Aim,
	what: string,
	options: { string },
	target: Minimenu.Target?,
	tile: (() -> Tile?)?,
	walk_timeout_ms: (number | false)?,
	spam: number?
): (Acted, boolean?)
	local box = clickbox()
	if tile and (box == nil or Clickbox.target(box, what) == nil) then
		Clickbox.reveal(clickbox, tile, what)
		box = clickbox()
	end
	local view = Toplevel.viewport()
	if box == nil or view == nil or Clickbox.target(box, what) == nil then
		return "unusable", nil
	end
	local before = Minimenu.clicked()
	local result = Host.act({
		model = act.model,
		entry = act.entry,
		options = options,
		right = true,
		near = act_near(box),
		spread = Clickbox.CENTER_SPREAD,
		min_area = Clickbox.MIN_TRIANGLE_AREA,
		timeout = Clickbox.ACT_TIMEOUT_MS,
		viewport = view,
		blockers = Toplevel.blockers(),
		mouse = Mouse.wind_options(),
		hold = Mouse.press_ms(),
	})
	local status = result.status
	if status == "left" then
		local clicked = if before then Minimenu.wait_click(before) else nil
		if before == nil or (clicked and did_any(clicked, options, target)) then
			Interact.spam_click(spam, function(): boolean
				return left_click_is(options, target)
			end)
			return "done",
				walk_timeout_ms == false or Player.wait_walk(if walk_timeout_ms then walk_timeout_ms else nil)
		end
		if clicked then
			Log.log("Clickbox", `misclicked {what}: the client did "{Minimenu.text(clicked.entry)}"`)
		end
		return "retry", nil
	elseif status == "right" then
		local handled, ok = Interact.open_menu(options, target, walk_timeout_ms)
		return if handled then "done" else "retry", ok == true
	elseif status == "no_option" then
		return "done", false
	elseif status == "gone" then
		if tile then
			Clickbox.reveal(clickbox, tile, what)
		end
		return "retry", nil
	elseif status == "menu_open" then
		Minimenu.close()
		return "retry", nil
	elseif status == "timeout" or status == "lost" or status == "cancelled" then
		return "retry", nil
	end
	Log.log("Clickbox", `can't act on {what}: {status}`)
	return "unusable", nil
end

local function interact(
	clickbox: Aim,
	hovered: () -> boolean,
	what: string,
	option: string | { string },
	target: Minimenu.Target?,
	tile: (() -> Tile?)?,
	walk_timeout_ms: (number | false)?,
	spam: number?,
	menu: boolean?,
	act: ActTarget?
): boolean
	local options = option_list(option)
	local handled, result = Interact.open_menu(options, target, walk_timeout_ms)
	if handled then
		return result == true
	end
	local left = false
	local function on_it(): boolean
		left = not menu and left_click_is(options, target)
		return left or hovered()
	end
	local acting = act ~= nil and not menu and not swapped(what)
	for _ = 1, Clickbox.MISCLICK_ATTEMPTS do
		if act and acting then
			local outcome, done = act_once(act, clickbox, what, options, target, tile, walk_timeout_ms, spam)
			if outcome == "done" then
				return done == true
			elseif outcome == "retry" then
				continue
			end
			acting = false
		end
		local before = Minimenu.clicked()
		local ok = Clickbox.hover(clickbox, on_it, what, tile)
		if not ok then
			return false
		end
		if left then
			Mouse.click()
			local clicked = if before then Minimenu.wait_click(before) else nil
			if before == nil or (clicked and did_any(clicked, options, target)) then
				Interact.spam_click(spam, function(): boolean
					return left_click_is(options, target)
				end)
				return walk_timeout_ms == false or Player.wait_walk(if walk_timeout_ms then walk_timeout_ms else nil)
			end
			if clicked == nil then
				if Player.destination() == nil then
					return false
				end
			else
				Log.log("Clickbox", `misclicked {what}: the client did "{Minimenu.text(clicked.entry)}"`)
			end
			continue
		end
		local at = Mouse.position()
		if at == nil then
			return false
		end
		local pick = offered(options, target) or options[1]
		local walking = Player.destination() ~= nil
		local done, misclicked = Interact.with(at, pick, target, walk_timeout_ms, spam, true, menu)
		if done or (misclicked == nil and not walking) then
			return done
		end
	end
	return false
end

function Clickbox.interact(
	clickbox: Aim,
	hovered: () -> boolean,
	what: string,
	option: string | { string },
	target: Minimenu.Target?,
	tile: (() -> Tile?)?,
	walk_timeout_ms: (number | false)?,
	spam: number?,
	menu: boolean?,
	act: ActTarget?
): boolean
	Walker.follow_destination()
	return interact(clickbox, hovered, what, option, target, tile, walk_timeout_ms, spam, menu, act)
end

Clickbox.APPROACH_NEAR = 3
Clickbox.STAND_CANDIDATES = 4
Clickbox.EARLY_RETRY_MS = 1500
Clickbox.APPROACH_TIMEOUT_MS = 30000

local function aimable(clickbox: Aim, what: string): boolean
	return aim(clickbox, what) ~= nil
end

local function stand_goal(stand: { Tile }, me: Tile): Tile?
	for _, t in ipairs(stand) do
		if t.x == me.x and t.y == me.y then
			return t
		end
	end
	local nearest = table.clone(stand)
	table.sort(nearest, function(a: Tile, b: Tile): boolean
		return Walker.distance(me, a) < Walker.distance(me, b)
	end)
	for i = 1, math.min(#nearest, Clickbox.STAND_CANDIDATES) do
		local plan = Walker.plan(nearest[i], nil, { near = 0 })
		if plan and plan.reached then
			return nearest[i]
		end
	end
	return nil
end

local function stands_on(tiles: { Tile }, at: Tile): boolean
	for _, t in ipairs(tiles) do
		if t.x == at.x and t.y == at.y then
			return true
		end
	end
	return false
end

local function lands(reach: () -> Host.ReachTarget?, extra: { Tile }?): boolean
	local me = Walker.position()
	local to = reach()
	if me == nil or to == nil then
		return false
	end
	local route = Host.reach_route({ x = me.x, y = me.y }, to)
	if route == nil then
		return false
	end
	local last = route.tiles[#route.tiles]
	return route.reached or (extra ~= nil and last ~= nil and stands_on(extra, last))
end

function Clickbox.approach(
	clickbox: Aim,
	hovered: () -> boolean,
	what: string,
	option: string | { string },
	target: Minimenu.Target?,
	tile: () -> Tile?,
	reach: () -> Host.ReachTarget?,
	opts: Approach?,
	act: ActTarget?
): boolean
	local o: Approach = opts or {}
	local options = option_list(option)
	local near = o.near or Clickbox.APPROACH_NEAR
	local walk: Walker.WalkOptions = o.walk or {}
	local at = tile()
	if at == nil then
		return false
	end
	local me = Walker.position()
	if me == nil then
		return false
	end

	Camera.face(at)
	local picked_early = false
	local to_reach = reach()
	local candidates: { Tile } = {}
	for _, list in { (to_reach and Host.reach_tiles(to_reach)) or {}, o.stand or {} } do
		for _, t in ipairs(list) do
			table.insert(candidates, t)
		end
	end
	local stand: Tile? = nil
	local function goal(): (Tile?, number)
		if stand then
			return stand, 0
		end
		local now = tile()
		if now == nil then
			return nil, near
		end
		return { x = math.floor(now.x), y = math.floor(now.y) }, near
	end
	local function ready(): boolean
		if #candidates > 0 then
			return lands(reach, o.stand)
		end
		local to, within = goal()
		return to ~= nil and Walker.plain_walk(to, within)
	end
	local close = stands_on(candidates, me) or Walker.distance(me, at) <= near
	local pick_now = if #candidates > 0
		then (close or aimable(clickbox, what)) and ready()
		else close or (aimable(clickbox, what) and ready())
	if not pick_now then
		stand = stand_goal(candidates, me)
		local last_try: number? = nil
		local to, within = goal()
		local ok = Walker.walk(to or { x = math.floor(at.x), y = math.floor(at.y) }, {
			near = within,
			timeout_ms = o.timeout_ms or Clickbox.APPROACH_TIMEOUT_MS,
			straight = walk.straight,
			prefer_house = walk.prefer_house,
			before_click = walk.before_click,
			accept = walk.accept or function(entry: Host.MenuEntry): boolean
				for _, option in ipairs(options) do
					if Minimenu.matches(entry, option, target) then
						return true
					end
				end
				return false
			end,
			on_step = function(here: Tile): boolean
				if walk.on_step and walk.on_step(here) == false then
					return false
				end
				if
					not o.settle
					and (last_try == nil or Host.time() - last_try >= Clickbox.EARLY_RETRY_MS)
					and on_screen(clickbox, what)
					and aimable(clickbox, what)
					and ready()
				then
					last_try = Host.time()
					if
						Clickbox.interact(clickbox, hovered, what, option, target, tile, false, o.spam, nil, act)
						== true
					then
						picked_early = true
						return false
					end
				end
				return true
			end,
		})
		if not picked_early and not ok then
			return false
		end
	end

	if picked_early then
		if o.arrive_ms == false then
			return true
		end
		return Player.wait_walk(if o.arrive_ms then o.arrive_ms else nil)
	end
	if o.settle then
		Player.wait_walk(o.timeout_ms or Clickbox.APPROACH_TIMEOUT_MS)
	end
	if Clickbox.interact(clickbox, hovered, what, option, target, tile, o.arrive_ms, o.spam, nil, act) then
		return true
	end
	local closer = o.closer
	local again = if closer then tile() else nil
	if closer == nil or again == nil then
		return false
	end
	Camera.face(again)
	local reached = Walker.walk({ x = math.floor(again.x), y = math.floor(again.y) }, {
		near = closer,
		timeout_ms = o.timeout_ms or Clickbox.APPROACH_TIMEOUT_MS,
		straight = walk.straight,
		prefer_house = walk.prefer_house,
	})
	if not reached then
		return false
	end
	Player.wait_still(3000)
	return Clickbox.interact(clickbox, hovered, what, option, target, tile, o.arrive_ms, o.spam, nil, act)
end

function Clickbox.draw(box: Clickbox, color: Draw.Color?)
	if box.box then
		Draw.cuboid(box.box, Draw.BLUE)
	end
	Draw.polygon(box.polygon, color or (if box.triangles then Draw.YELLOW else Draw.CYAN))
end

return Clickbox
