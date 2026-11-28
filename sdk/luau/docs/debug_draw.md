# DebugDraw

Debug drawing that shows only while Script debugging is on in a client's development page in the loader (`Host.script_debugging`). Callers register named layers; every change repaints all layers, so the SDK's drawings (such as `Walker.show_route`) and a script's own stay on screen together.

Inside a layer's `paint`, `DebugDraw.outline`, `DebugDraw.line`, `DebugDraw.label` and `DebugDraw.loc` take tiles or locs, not pixels. They become world drawings that the agent projects every frame (`Host.debug_shapes`), so they stay on their tiles while the camera turns, zooms or pans, whatever the script is doing, hide under game interfaces, and vanish when Script debugging is turned off or the run ends. `DebugDraw.screen_label` is the one world drawing placed by screen pixel instead: game-font text that stays at a spot on the screen and shows over interfaces. `Draw` calls inside `paint` still draw screen pixels: use them only for marks that belong to the screen, such as inventory slots on a `"ui"` layer. Painting clears the screen overlay first, so don't mix DebugDraw with a script's own `Draw` calls.

A `Spot` is `{ x, y }` in tiles; whole numbers are a tile's centre and fractions are allowed, such as a player between tiles.

## DebugDraw.on

Definition:
```luau
function DebugDraw.on(): boolean
```
Whether Script debugging is on now.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")

print(DebugDraw.on())
```

## DebugDraw.set

Definition:
```luau
function DebugDraw.set(name: string, paint: (() -> ())?, layer: Draw.Layer?)
```
Adds or replaces the layer `name` (drawn in the order layers were first added) and repaints; `nil` removes it. `paint` runs inside a `Draw.batch` with `layer` (default `"world"`), so it can call `Draw` functions directly as well as DebugDraw's world drawings; pass `"ui"` for a layer whose `Draw` calls mark interfaces. With debugging off nothing is drawn, and what was shown is cleared.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")

DebugDraw.set("target", function()
	DebugDraw.outline({ { x = 3200, y = 3200 } }, Draw.GREEN, 2)
end)
DebugDraw.set("target", nil)
```

## DebugDraw.render

Definition:
```luau
function DebugDraw.render()
```
Runs every layer's `paint` again, for a drawing whose content changed (such as the tiles left of a walked route); the camera needs no repaint. Clears everything when debugging is off or no layer is left.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")

DebugDraw.render()
```

## DebugDraw.outline

Definition:
```luau
function DebugDraw.outline(tiles: { Tile }, color: Draw.Color, width: number?)
```
Outlines the border of an area of tiles on the ground: only the edges between a tile in `tiles` and one outside it, `width` pixels thick (1 to 16, default 1). Only inside a layer's `paint`.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")

DebugDraw.set("danger", function()
	DebugDraw.outline({ { x = 3200, y = 3200 }, { x = 3201, y = 3200 } }, Draw.RED, 3)
end)
```

## DebugDraw.line

Definition:
```luau
function DebugDraw.line(from: Spot, to: Spot, color: Draw.Color, width: number?)
```
A straight line between two spots on the ground, `width` pixels thick (1 to 16, default 1). Only inside a layer's `paint`.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")

DebugDraw.set("aim", function()
	DebugDraw.line({ x = 3200, y = 3200 }, { x = 3205.5, y = 3203 }, Draw.MAGENTA, 3)
end)
```

## DebugDraw.label

Definition:
```luau
function DebugDraw.label(at: Spot, text: string, color: Draw.Color, bold: boolean?)
```
Text in the game's font (its bold font when `bold`), centred on the spot with its bottom on the ground there, with a shadow; `\n` starts a new line, each centred, the last one on the ground. Up to 200 bytes. Only inside a layer's `paint`.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")

DebugDraw.set("names", function()
	DebugDraw.label({ x = 3200, y = 3200 }, "Bank", Draw.WHITE, true)
end)
```

## DebugDraw.screen_label

Definition:
```luau
function DebugDraw.screen_label(at: Point, text: string, color: Draw.Color, bold: boolean?)
```
Text in the game's font (its bold font when `bold`) with a shadow, at a screen pixel instead of a tile: its left edge at `at.x` and its bottom at `at.y`, with `\n` lines stacked upwards. It is drawn over game interfaces, like a `"ui"` layer's marks, so it suits status lines beside a panel. The agent draws it every frame like the world drawings, so it shows without a repaint while the script is busy and vanishes with them. Up to 200 bytes. Only inside a layer's `paint`.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")
local Host = require("@volcren/host")

local panel = Host.widget(161, 97)
if panel then
	DebugDraw.set("status", function()
		DebugDraw.screen_label({ x = panel.x + 8, y = panel.y - 8 }, "Alching", Draw.MAGENTA, true)
	end)
end
```

## DebugDraw.loc

Definition:
```luau
function DebugDraw.loc(loc: Host.Loc, faces: Draw.Color?, hull: Draw.Color?)
```
Draws the loc's model as the client drew it this frame: every triangle edge in `faces` and its outline in `hull`, 1 pixel wide; a missing color draws nothing. It follows the loc's animation as well as the camera, and shows nothing while the client doesn't draw the loc. At most 16 per run. Only inside a layer's `paint`.

Example:
```luau
local DebugDraw = require("@volcren/debug_draw")
local Draw = require("@volcren/draw")
local Objects = require("@volcren/objects")
local Walker = require("@volcren/walker")

local tree = Objects.nearest("Tree", assert(Walker.position()))
if tree then
	DebugDraw.set("tree", function()
		DebugDraw.loc(tree, Draw.color(255, 255, 0, 70), Draw.YELLOW)
	end)
end
```
