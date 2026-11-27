# Clickbox

Aiming the mouse at characters and objects. A clickbox is the screen area where the game accepts a click for a thing; this module picks a good spot in it, waits for the client to confirm the mouse is on the thing, and clicks. Usually used through [Npcs](npcs.md), [Objects](objects.md) and [Players](players.md), which build the clickbox.

```luau
local Clickbox = require("@volcren/clickbox")
```

Functions that aim take an `Aim` (see `Clickbox.walking`): `aim()` is the box as drawn now, `aim(lead_ms)` where it will be after the player's walk moves the camera.

## Settings

Definition:
```luau
Clickbox.TARGET_TRIES = 30
Clickbox.MIN_TRIANGLE_AREA = 4
Clickbox.FOLLOW_AREA = 25
Clickbox.CENTER_SPREAD = 0.45
Clickbox.MOUSE_PULL = 2
Clickbox.CAMERA_RANGE = 15
Clickbox.REVEAL_YAW_STEP = 5
Clickbox.REVEAL_ZOOM_STEPS = 6
Clickbox.REVEAL_ZOOM_COST = 8
Clickbox.REVEAL_SLACK = 12
Clickbox.REVEAL_MARGIN = 0.08
Clickbox.REVEAL_SHOWN = 0.85
Clickbox.ATTEMPTS = 5
Clickbox.MISCLICK_ATTEMPTS = 3
Clickbox.ANCHOR_HEIGHT = 200
Clickbox.WALK_REACTION_MS = 30
Clickbox.WALK_HOLD_MS = 150
Clickbox.WALK_LEAD_MS = 300
Clickbox.WALK_LEAD_TOLERANCE = 40
Clickbox.ACT_TIMEOUT_MS = 3000
Clickbox.ACT_NEAR_SHARE = 0.4
Clickbox.ACT_NEAR_MIN = 3
Clickbox.APPROACH_NEAR = 3
Clickbox.EARLY_RETRY_MS = 1500
Clickbox.APPROACH_TIMEOUT_MS = 30000
```
- `TARGET_TRIES`: tries to find an uncovered spot.
- `MIN_TRIANGLE_AREA`: triangles smaller than this (pixels) are ignored.
- `FOLLOW_AREA`: minimum area (pixels) of a drawn box half `hover` follows.
- `CENTER_SPREAD`: width of the bell curve around the shape's middle, as a share of its spread (smaller is tighter).
- `MOUSE_PULL`: weakness of the lean towards the mouse, in spreads (bigger is weaker).
- `CAMERA_RANGE`: off-screen things within this many tiles get the camera moved to show them; `nil` disables it.
- `REVEAL_YAW_STEP`: degrees between the yaws `reveal` considers.
- `REVEAL_ZOOM_STEPS`: zoom levels `reveal` considers between the current zoom and `Camera.ZOOM_MIN`.
- `REVEAL_ZOOM_COST`: degrees of turning one `Camera.ZOOM_STEP` of zooming out costs when `reveal` compares views.
- `REVEAL_SLACK`: `reveal` picks at random among the views that show the thing and cost at most this much more than the cheapest.
- `REVEAL_MARGIN`: share of the viewport's width and height kept clear around a view's box.
- `REVEAL_SHOWN`: share of the thing's predicted box that must be inside that inset and clear of interfaces.
- `ATTEMPTS`: aims before `hover` gives up; confirmation waits are [Track](track.md)'s `CONFIRM_MS`.
- `MISCLICK_ATTEMPTS`: retries when the client did something else.
- `ANCHOR_HEIGHT`: default anchor height (fine units) for `walking`.
- `WALK_REACTION_MS`: delay from the mouse landing to the click while walking.
- `WALK_HOLD_MS`: how long the spot must stay in the box after that.
- `WALK_LEAD_MS`: guessed mouse travel time when nothing is drawn yet.
- `WALK_LEAD_TOLERANCE`: the aim is redone unless the real travel time is within this of the guess.
- `ACT_TIMEOUT_MS`: how long one [Host.act](host.md#hostact) follows the thing before giving up.
- `ACT_NEAR_SHARE`, `ACT_NEAR_MIN`: how close (share of the box's smaller side, at least this many px) the mouse must be to the spot before `act` clicks.
- `APPROACH_NEAR`: default `near` for `approach`.
- `EARLY_RETRY_MS`: interval between early picks during an `approach` walk.
- `APPROACH_TIMEOUT_MS`: default walk and settle timeout for `approach`.

## Clickbox.from_model

Definition:
```luau
function Clickbox.from_model(model: Model, what: string): Clickbox?
```
A clickbox from the outline and faces the client drew for a model. Nil when the model has no usable hull or faces (not in front of the camera).

Example:
```luau
local npc = (Host.npcs() or {})[1]
local model = npc and Host.npc_model(npc.index)
local box = model and Clickbox.from_model(model, npc.name)
```

## Clickbox.target

Definition:
```luau
function Clickbox.target(box: Clickbox, what: string, keep: ((Point) -> boolean)?, ignore_windows: boolean?): (Point?, Polygon?)
```
Picks a spot in the box not covered by an interface. On a model it picks a visible triangle (at least `MIN_TRIANGLE_AREA`) weighted by area times a bell curve around the shape's area-weighted middle (`CENTER_SPREAD`) times a weak pull to the mouse (`exp(-distance / (MOUSE_PULL * spread))`), then a spot near that triangle's middle; a box without triangles leans towards the mouse. `keep` rejects spots, `ignore_windows` counts only side panels as covering. Returns the spot and its triangle (nil without triangles), or nil when nothing is visible.

Example:
```luau
local npc = Npcs.nearest("Man", assert(Walker.position()))
local box = npc and Npcs.clickbox(npc)
if box then
	print(Clickbox.target(box, "the man"))
end
```

## Clickbox.reveal

Definition:
```luau
function Clickbox.reveal(clickbox: Aim, tile: () -> Tile?, what: string): boolean
```
Brings an off-screen thing within `CAMERA_RANGE` into view with one camera move, worked out before moving. It projects a one-tile box `ANCHOR_HEIGHT` tall on `tile()` for every yaw and zoom in its search (`Host.project` with a view), keeping the pitch, then turns and zooms once to one of the cheapest views that show it. Zoom only scales the view about its centre, so each yaw needs two projections, at the current and farthest zoom, and the zooms between are interpolated. When no view shows it, it turns to the one that shows the most and logs it. True when the thing can then be targeted; false otherwise, so the caller can walk closer.

Example:
```luau
print(Clickbox.reveal(function()
	return Npcs.clickbox(npc)
end, function()
	return { x = npc.x, y = npc.y }
end, "the man"))
```

## Clickbox.hover

Definition:
```luau
function Clickbox.hover(clickbox: Aim, hovered: () -> boolean, what: string, tile: (() -> Tile?)?): boolean
```
Moves onto the thing and returns once `hovered` confirms the client sees the mouse on it. A model is aimed with [Track.hover_heat](track.md#trackhover_heat), favouring cells the model has covered longest (kept relative to the box `anchor`), and the move stops early once `hovered` agrees. A drawn box follows its larger top or bottom half (at least `FOLLOW_AREA`) with [Track.hover](track.md#trackhover). Failures are logged and retried up to `ATTEMPTS`. With `tile` it first may turn the camera (`Camera.face`, on its chance) and calls `reveal`.

Example:
```luau
print(Npcs.hover(npc))
```

## Clickbox.hover_async

Definition:
```luau
function Clickbox.hover_async(clickbox: Aim, what: string): boolean
```
Starts the mouse towards where the thing will be on arrival and returns at once, without camera moves or hover checks. False when it isn't on screen.

Example:
```luau
Clickbox.hover_async(function()
	return Npcs.clickbox(npc)
end, npc.name)
```

## Clickbox.click

Definition:
```luau
function Clickbox.click(clickbox: Aim, hovered: () -> boolean, what: string, button: Mouse.Button?, tile: (() -> Tile?)?, spam: number?): boolean
```
`hover`, then clicks `button` (left by default). `spam` is the chance of extra clicks, see [Interact.spam_click](interact.md#interactspam_click).

Example:
```luau
print(Npcs.click(npc))
```

## Clickbox.interact

Definition:
```luau
function Clickbox.interact(clickbox: Aim, hovered: () -> boolean, what: string, option: string | { string }, target: Minimenu.Target?, tile: (() -> Tile?)?, walk_timeout_ms: (number | false)?, spam: number?, menu: boolean?, act: ActTarget?): boolean

export type ActTarget = { model: Host.ActModel, entry: { kind: number?, id: number?, index: number? } }
```
Answers a menu left open first ([Interact.open_menu](interact.md#interactopen_menu)). With `act` (and no `menu`, and no entry swap for `what`) it clicks through [Host.act](host.md#hostact), which follows the model frame by frame and clicks on the frame the client shows it under the mouse, so moving things are hit; a right click there leaves the menu for `Interact.open_menu`. Before acting, a thing with no point `Clickbox.target` can aim at (off screen, or drawn only under the chatbox or a panel) is brought into view with `reveal`; the agent finds no spot on a hidden model and would only time out. Otherwise, or when the agent can't act, it hovers until the left-click entry is `option` on `target` and clicks on that same menu read, or picks the first offered `option` via [Interact.with](interact.md#interactwith); then waits for the walk. Retries up to `MISCLICK_ATTEMPTS` on a misclick or a missed follow. `menu` forces the right-click menu. [Npcs](npcs.md), [Objects](objects.md) and [GroundItems](ground_items.md) pass `act`.

Example:
```luau
print(Npcs.interact(npc, "Talk-to"))
```

## Clickbox.draw

Definition:
```luau
function Clickbox.draw(box: Clickbox, color: Draw.Color?)
```
Draws the clickbox over the game, to see what a script aims at.

Example:
```luau
Clickbox.draw(box)
```

## Clickbox.from_cuboid

Definition:
```luau
function Clickbox.from_cuboid(cuboid: { bottom: Quad, top: Quad }): Clickbox
```
A clickbox from a projected cuboid (`Projection.cuboid`): the hull of its corners, with no triangles.

Example:
```luau
local box = Clickbox.from_cuboid(assert(Projection.cuboid(5824, 7104)))
```

## Clickbox.walking

Definition:
```luau
function Clickbox.walking(model: () -> Clickbox?, anchor: () -> Anchor?, what: string): Aim

export type Aim = (lead_ms: number?) -> Clickbox?
export type Anchor = { x: number, y: number, size: number?, height: number? }
```
Builds an `Aim`. Without `lead_ms` it returns `model()`; with it, the box moved and scaled as the camera shift from `Walker.ahead` moves `anchor` (fine units like `Npc.local_x`, `height` defaulting to `ANCHOR_HEIGHT`). With nothing drawn it uses a box of `size` tiles around the anchor. The result's `anchor` is the anchor's screen point. While the player has a destination, `hover`, `click`, `interact` and `approach` aim `Mouse.travel_ms + WALK_REACTION_MS` ahead and pick a spot still inside `WALK_HOLD_MS` later; with nothing drawn, `WALK_LEAD_MS` is the guess.

Example:
```luau
local aim = Clickbox.walking(function()
	return Npcs.clickbox(npc)
end, function()
	return { x = npc.local_x, y = npc.local_y }
end, npc.name)
local soon = aim(300)
```

## Clickbox.approach

Definition:
```luau
function Clickbox.approach(clickbox: Aim, hovered: () -> boolean, what: string, option: string | { string }, target: Minimenu.Target?, tile: () -> Tile?, reach: () -> Host.ReachTarget?, opts: Approach?): boolean

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
```
Picks `option` as `interact` does once the pick would get the thing used: the route the server walks to `reach` from here (`Host.reach_route`) reaches it, or ends on one of `stand`, the tiles it can also be used from that the reach rules don't cover (a banker's counter). Then it picks without walking when the thing is within `near` tiles (`APPROACH_NEAR`) or on screen at the lead. Otherwise it walks to the nearest tile of `Host.reach_tiles` and `stand` that the planner reaches (trying `STAND_CANDIDATES`), through doors, and picks early every `EARLY_RETRY_MS` once that holds, so a thing a tile away across a wall is walked round to rather than picked from where the server stops short. A thing with no such tiles (an item on a table) is walked to within `near`, picking early once `Walker.plain_walk` reports no door, climb or shortcut left. `settle` instead waits for the walk to stop. On failure, `closer` walks to that many tiles and picks once more. Returns after arriving (`arrive_ms`, false returns right after the pick). Of `walk`, `straight`, `prefer_house`, `before_click`, `accept` and `on_step` reach the walks. `walk.on_step` or `walk.before_click` returning false stops it with false; `timeout_ms` defaults to `APPROACH_TIMEOUT_MS`.

Example:
```luau
local npc = assert(Npcs.nearest("Ferryman Nathwood", assert(Walker.position())))
local ok = Npcs.approach(npc, { "Examine" }, { arrive_ms = false })
```
