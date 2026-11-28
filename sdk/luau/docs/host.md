# Host

The sandbox globals the client gives every script: game state, input, overlay drawing and timing. The rest of the SDK is built on them. Functions that can fail return `nil` (and, where the signature says so, an error string).

```luau
local Host = require("@volcren/host")
```

## Types

All are plain tables; see `host.luau` for every field.

| Type | Holds |
| --- | --- |
| `ClientState` | Position, name, skills, run energy, weight, camera, minimap zoom, destination. Any field may be `nil` (e.g. logged out). |
| `PositionData` | World tile `x`, `y`, `plane` (0 ground, `nil` if unreadable) and scene tile `local_x`, `local_y`. |
| `Skill` | `name`, boosted `level`, `real_level`, `xp`. |
| `LoginLayout` | Where the login screen's parts are drawn, and `steam`: whether the client signs in through Steam, which changes the welcome screen (`nil` if the agent couldn't tell). |
| `CameraState` | Position, `pitch`, `yaw`, focal point, `zoom`. Positions in 128ths of a tile; angles 0 to 16383 per turn. |
| `MinimapState` | `zoom` in pixels per tile (2 to 8). |
| `Connection` | Client `version`, `protocol`, window `width`/`height`, mouse position. |
| `Widget` | One interface component: position, size, text, sprite, hidden, scroll, `item_id`/`item_quantity`. |
| `ItemSlot` | `slot`, item `id`, `quantity`. |
| `ItemChange` / `Packet` | What `Host.on_item_change` and `Host.on_packet` pass. |
| `Npc` / `PlayerInfo` | `index`, `name`, world and local tile. NPCs have `animation` (-1 none) and `head_icons`; players have `pose` and `movement` (their stand/turn/walk/run poses; a `pose` outside it is an obstacle, emote or ride). |
| `NpcModel` | On-screen outline of an actor or object this frame (box corners, hull, faces). |
| `Hitsplat` | `slot`, `kind`, `damage`, `cycle`. |
| `Collision` | Corner `x`, `y`, `width`, `height`, `plane`, per-tile `flags`. |
| `CacheLoc` / `CacheLocFilter` | An object the cache places on the map (`name`, tile, `plane`, size, `shape`, `rotation`, `actions`), and the filter for `Host.cache_locs`. |
| `Loc` / `LocView` | A scene object with tile, size, shape and `clickable`; how an object id looks now (`name`, `actions`). |
| `GroundItem` | An item on the ground. |
| `Projectile` | Graphic `id`, endpoints, start/end `cycle`, current position. See [Projectiles](projectiles.md). |
| `SpotAnim` | Graphic `id`, tile, `plane`, start. See [Spot animations](spot_anims.md). |
| `ChatMessage` | `uid` (increasing), `kind`, `name`, `sender`, `text`, `time` (ms since 1970), `age` (ms), `cycle`, `time_utc`/`time_local` ("HH:MM"). |
| `MenuEntry` / `MenuClick` | A right-click line (`submenu` true if it opens one; `x`, `y` the loc's origin world tile on loc entries); the line the client last acted on. |
| `MenuLayout` / `MenuSubmenu` / `MenuRect` | Open menu geometry (`Host.menu_layout`). |
| `EntrySwaps` / `EntrySwap` | The client's entry swaps (`Host.entry_swaps`). |
| `SelectedWidget` | A selected spell or item awaiting a target: `verb`, `name`, component, `item`, `targets` bitmask (1 ground items, 2 NPCs, 4 objects, 8 players, 16 self, 32 components). |
| `EnumDef` / `CacheValue` | A cache enum and one value (number or string). |
| `SubInterface` | An interface opened inside another. |
| `World` / `WorldSelect` / `WorldPanel` / `PanelRect` | World list and switcher state. |
| `Tile` / `NavTile` / `Area` / `Corners` / `ScreenPoint` / `FinePoint` | Coordinate shapes. |
| `CameraView` | A camera yaw and zoom to project with (`project`). |
| `SceneRoute` / `WorldRoute` / `NavPlan` / `NavOptions` / `SceneRouteOptions` | Pathing results and options. |
| `ReachTarget` | What `Host.reach_route` walks to: `kind` `"npc"` (south-west tile, `size`), `"loc"` (`size_x`, `size_y`, `shape`, `rotation` as on `Loc`) or `"tile"` (a ground item). |
| `Shape` | A `rect`, `line` or `clear` for `Host.draw_batch`. |
| `Layer` | `"world"` or `"ui"`: the overlay layer of a drawing. |
| `FormSpec` / `FormField` / `GridColumn` / `TaskType` | A settings window. |
| `LayoutItem` | An item of a layout: `id`, `ids`, `name`, `stackable`, `min`/`max` (equal: exact, both 0: any), `doses`, `locked`. See [LayoutItem](layout_item.md). |
| `EquipmentLayout` / `EquipmentLayoutSlot` / `EquipmentSpell` | An `equipment` field's value. See [EquipmentLayout](equipment_layout.md). |
| `InventoryLayout` / `InventoryLayoutSlot` | An `inventory` field's value, with rune pouch `pouch`. See [InventoryLayout](inventory_layout.md). |
| `PrayerLayout` | A `prayer` field's value. See [PrayerLayout](prayer_layout.md). |
| `LayoutRequirement` | A `required` entry: `ids`, `slot`, `min`, `max`, `doses`. |
| `WindMouseOptions` | Mouse movement tuning. |
| `HttpRequest` / `HttpResponse` | `Host.http_request` input and output. |
| `ItemPrice` | `id`, `name`, `members`, `value`, `high_alch`, `low_alch`, `limit`, `high`/`low` with `high_time`/`low_time`. |

## Host.game_state

Definition:
```luau
Host.game_state: () -> ClientState
```
Snapshot of the player and client. Always returns a table; raises if the client or agent is gone.

Example:
```luau
local state = Host.game_state()
if state.position then
	print(state.position.x, state.position.y)
end
```

## Host.connection

Definition:
```luau
Host.connection: () -> Connection
```
Client version, window size and mouse position. Never nil; raises if the client or agent goes away.

Example:
```luau
local info = Host.connection()
print(`{info.width}x{info.height}`)
```

## Host.widget

Definition:
```luau
Host.widget: (group: number, child: number) -> Widget?
```
One interface component. `item_id` (-1 for none) and `item_quantity` are the item a cell shows; a bank placeholder shows quantity 0 here but 1 in the bank container.

Example:
```luau
local w = Host.widget(149, 0)
if w then
	print(w.x, w.y, w.width, w.height)
end
```

## Host.widget_children

Definition:
```luau
Host.widget_children: (group: number, child: number) -> { Widget }?
```
The child components of a component.

Example:
```luau
print(#(Host.widget_children(149, 0) or {}))
```

## Host.widget_model

Definition:
```luau
Host.widget_model: (group: number, child: number) -> { kind: number, id: number }?
```
The model a component draws (`kind` 1 is a model id; `id` -1 for none). Nil for no such component. Slower than `Host.widget`.

Example:
```luau
print((Host.widget_model(184, 7) or { id = -1 }).id)
```

## Host.item_container

Definition:
```luau
Host.item_container: (id: number) -> { ItemSlot }?
```
Contents of an item container: inventory `93`, worn `94`, bank `95`.

Example:
```luau
for _, slot in ipairs(Host.item_container(93) or {}) do
	print(slot.slot, slot.id, slot.quantity)
end
```

## Host.on_item_change

Definition:
```luau
Host.on_item_change: (callback: (change: ItemChange) -> ()) -> number
```
Calls `callback` for every item container slot write from now on and returns an id for `Host.off_item_change`. A change has `container`, `slot`, `tick`, `old_item`, `old_quantity`, `new_item`, `new_quantity` (empty is `-1`) and `old_known` (false when the write created the container). Callbacks run only inside `Host.sleep`, `Host.wait_item_change` and `Host.wait_packet`; a script that never sleeps sees none. A callback error ends the script. One tick's changes arrive together (a bank opening is hundreds).

Example:
```luau
Host.on_item_change(function(change)
	if change.container == 93 then
		print(change.slot, change.old_item, "->", change.new_item)
	end
end)
Host.sleep(10000)
```

## Host.off_item_change

Definition:
```luau
Host.off_item_change: (id: number) -> boolean
```
Removes a callback from `Host.on_item_change`; returns whether it existed.

## Host.wait_item_change

Definition:
```luau
Host.wait_item_change: (ms: number) -> boolean
```
Waits up to `ms` for an item container change, runs the callbacks and returns true; false on timeout. Without an `on_item_change` callback it only sleeps.

Example:
```luau
Host.on_item_change(function() end)
print(Host.wait_item_change(30000))
```

## Host.on_packet

Definition:
```luau
Host.on_packet: (packets: { number | PacketName }, callback: (packet: Packet) -> (), sent: boolean?) -> number
```
Calls `callback` for each received server packet in `packets`, or each sent client packet when `sent` is true; returns an id for `Host.off_packet`. Give a packet by name or by opcode (0 to 255). Game updates change opcodes and the order of the bytes inside a packet, so use names; raw opcodes are for exploring. A name this client doesn't have is an error: check `Host.packet_opcode` first when a script can go on without it. Names are server packets only.

A packet has `opcode`, `sent`, `length`, `payload` (first 104 bytes as a string), `time_ms`, `sequence`, and for a named received packet its `name` and decoded `fields` (`nil` if the payload is cut short):

| Name | `fields` |
| --- | --- |
| `TICK` | none; one per game tick |
| `VARP_SMALL`, `VARP_LARGE` | `id`, `value` |
| `SYNTH_SOUND` | `id`, `loops`, `delay` |
| `MESSAGE_GAME` | `type`, `name` (only when the message has one), `text`; text past the first 104 payload bytes is cut |

Callbacks run only while sleeping or waiting; the client keeps the last 4096 packets, so long busy stretches drop some.

Example:
```luau
local ticks = 0
Host.on_packet({ "TICK" }, function()
	ticks += 1
end)
Host.on_packet({ "SYNTH_SOUND" }, function(packet)
	print(`sound {(packet.fields :: { [string]: number }).id}`)
end)
Host.sleep(6000)
print(ticks) -- about 10
```

## Host.off_packet

Definition:
```luau
Host.off_packet: (id: number) -> boolean
```
Removes a callback from `Host.on_packet`; returns whether it existed.

## Host.wait_packet

Definition:
```luau
Host.wait_packet: (packets: { number | PacketName }, ms: number, sent: boolean?) -> Packet?
```
Waits up to `ms` for a packet in `packets` (names or opcodes, as in `Host.on_packet`) that arrives after the call and returns it; nil on timeout. Runs pending callbacks while waiting.

Example:
```luau
if Host.wait_packet({ "TICK" }, 1200) then
	print("new tick")
end
```

## Host.packet_opcode

Definition:
```luau
Host.packet_opcode: (name: PacketName) -> number?
```
This client's opcode for a named server packet, or nil when the agent couldn't find it in this client. Errors without a client or before the agent has read the client's packets.

Example:
```luau
if Host.packet_opcode("SYNTH_SOUND") == nil then
	print("no sound packets in this client")
end
```

## Host.project

Definition:
```luau
Host.project: (
	points: { FinePoint },
	shift: { x: number, y: number, plane: number? }?,
	view: CameraView?
) -> { ScreenPoint }?
```
Projects fine world points to the screen. `shift` (fine units, optional landing `plane`) projects as the camera will be once the player has moved that far. `view` (`{ yaw: number?, zoom: number? }`, in `Camera.yaw` and `Camera.zoom` units) projects as if the camera had turned to `yaw` around its focal point and zoomed to `zoom`, keeping its pitch, so a script can work out a camera move before making it.

Example:
```luau
local cam = Host.game_state().camera
if cam then
	local p = Host.project({ { x = cam.focal_x, y = cam.focal_y } })
	print(p and p[1].x, p and p[1].y)
end
```

## Host.settings

Definition:
```luau
Host.settings: () -> { [string]: any }
```
The values the user set before the script started, by field id; an empty table when the script has no settings. Settings are declared, not asked for: the `settings` of the `manifest.json` beside the entry file, which the loader shows before anything starts (and, for a store script, before the session is billed). `injector run` without the loader uses the manifest's defaults.

`manifest.json` holds `settings` (the form below), `queueable` (the loader's queue may run the script: it starts from any safe idle state, checks `Host.finish_requested` and reports `Host.report_exit`), `goals` (`[{ "id", "label" }]`, ids of `Host.report_counters` counters a queue entry can stop at) and `finish_grace_minutes` (1 to 120, default 5: how long the queue waits after `FINISH` before stopping the script). Unknown keys are refused at upload.

Field kinds: `label`, `check`, `number`, `text`, `choice`, `list` (ticked rows, `items = [{ id, label, default }]` giving `{ id = true }`), `grid` and `tasks` (see below), `equipment`, `inventory`, `prayer`. The last three add a loader tab where the user captures and edits layouts; the value is the chosen `EquipmentLayout`, `InventoryLayout` or `PrayerLayout`, or nil. `equipment`/`inventory` take `required` (`LayoutRequirement`s padlocked into every layout); `prayer` takes `locked` (keys the script controls, excluded from layouts). Values and layouts are remembered per script title.

A `grid` is a searchable, scrollable list where each row picks one of 2 to 5 columns, like radio buttons, giving `{ [item id] = column id }`. `label` heads the name column and `search` is the search box's placeholder. `columns = [{ id, label, summary, color, max }]`: `summary` is the word after the column's count in the footer (default its label); `color` is `#rrggbb`, defaulting by position to green, yellow, red, blue and purple, and `colors = false` draws plain radios instead; `max` caps how many rows the column holds, shown as `n/max` and leaving its empty radios unclickable once full. A column without `max` gets an "all" link that moves every row the search shows into it, skipping rows that can't pick it. `items = [{ id, label, detail, columns, default }]`: `detail` is dim text after the name; `columns` lists the column ids the row may pick (default all), and the others show dimmed and unclickable; `default` is one of those, falling back to the field's `default` when the row allows it, then the row's first column. A remembered pick the row no longer allows resets to its default. `note` is dim text in the footer. The defaults must fit every `max`; remembered picks that no longer do reset to the defaults.

A `tasks` field is an ordered list the user builds from task types, like the antiban breaks: each row picks a type and sets that type's own fields, and rows can be added, moved, removed and reset to `default`. It gives `{ { type = type id, [field id] = value } }` in order, possibly empty. `types = [{ id, label, fields }]`, where `fields` are `check`, `number`, `text` or `choice` fields without `visible_when`, `enabled_by`, `disabled_by`, `above`, `row` or `stacked`, and no field id is `type`. `default` lists the starting tasks as `{ type, [field id] = value }`, leaving out fields that keep their own default; `max` (1 to 50, default 50) caps the rows. Remembered rows of a type the manifest no longer has are dropped, and values a field no longer takes revert to its default.

Field options: `enabled_by`/`disabled_by` (a check id); `row` (fields with the same name share a line, `row_gap` apart, default 24, max 500); `box_first` for `number`; `stacked` for `number`, which puts its label above the box like a `choice`'s (not with `box_first`); `above = { id, percent }` for `number`, which keeps it at least `percent` (0 to 100) above another number field: its lowest value becomes the other's value times 1 + percent/100 and the other's highest becomes its value divided by that, rounded inwards for `integer` fields, so the loader's two spinners stop each other (a pair of minimum and maximum amounts). The defaults must obey it. The loader enforces it, while MCP runs pass `settings` unchecked as they do `min` and `max`; `heading = 1 | 2` for `label` (spaced by `heading_gap`, default 12, max 500); `visible_when = { id, equals }` (`equals` may be a list), a list of entries that must all match, or `{ any = [...] }`, which matches when one of its entries does; entries nest, so `{ any = [rule, [rule, rule]] }` is "this, or both of those". Hidden fields keep and return their values. `submit` names the start button.

Example `manifest.json`:
```json
{
	"queueable": true,
	"goals": [{ "id": "loops", "label": "Loops" }],
	"settings": {
		"fields": [
			{ "id": "loops", "kind": "number", "label": "Loops", "default": 5, "min": 1, "max": 100, "integer": true },
			{ "id": "fast", "kind": "check", "label": "Go fast", "default": false }
		]
	}
}
```

```luau
local values = Host.settings()
print(values.loops, values.fast)
```

## Host.finish_requested

Definition:
```luau
Host.finish_requested: () -> boolean
```
True once the loader's queue asked the script to wrap up (its goal or time limit was reached, or the user pressed Finish), and stays true. Check it at safe points, finish the current unit of work, leave the account idle somewhere safe, then return. [Handoff](handoff.md) wraps it.

Example:
```luau
while not Host.finish_requested() do
	-- one kill, trip or round
end
```

## Host.script_debugging

Definition:
```luau
Host.script_debugging: () -> boolean
```
True while Script debugging is on in a client's development page in the loader; it can change during a run. Debug drawing in the SDK and scripts happens only then, since it clears and repaints the overlay. The MCP reads the loader's saved setting when a run starts.

Example:
```luau
if Host.script_debugging() then
	print("drawing debug shapes")
end
```

## Host.report_exit

Definition:
```luau
Host.report_exit: (status: "done" | "blocked", reason: string?) -> ()
```
Says why the script is about to return. `"blocked"` means it can't go on but left the account safe (out of supplies, a missing requirement); the queue moves to its next entry. A clean return without it counts as `"done"`; an error, or being stopped after the grace period, as failed, which ends the queue.

Example:
```luau
Host.report_exit("blocked", "out of food")
return
```

## Host.form_update

Definition:
```luau
Host.form_update: () -> { [string]: any }?
```
While the script runs its settings stay in the loader with an Update button; this returns the full new values (same shape as `Host.settings`) when the user pressed it since the last call, else nil.

Example:
```luau
local new = Host.form_update()
if new then
	values = new
end
```

## Host.menu

Definition:
```luau
Host.menu: () -> { MenuEntry }?
```
Right-click menu lines. Open: the rows shown, top to bottom, without client-hidden entries. Closed: what hovering offers, first being the left-click action, as of the last frame drawn, returned at once. Input takes the client a few frames to show (up to about 40 ms), so after a click or move, wait for the menu you expect instead of reading once. Submenu lines are in `Host.menu_layout().submenus`. Cancels async input.

Example:
```luau
for _, entry in ipairs(Host.menu() or {}) do
	print(entry.option, entry.target)
end
```

## Host.menu_click

Definition:
```luau
Host.menu_click: () -> MenuClick?
```
The menu line the client last acted on, the mouse position and `age` in frames. Nil when unavailable.

Example:
```luau
local click = Host.menu_click()
if click then
	print(click.entry.option, click.age)
end
```

## Host.entry_swaps

Definition:
```luau
Host.entry_swaps: () -> EntrySwaps?
```
The account's client entry swaps from `entryswaps.dat` and `entryswapsalt.dat`: `swaps` and `alt` (alternate key held), each `{ target, options }` with options best first. The client applies them on left click without changing the menu, so `Host.menu` shows the unswapped order. Nil when not logged in. Files are reread each call.

Example:
```luau
for _, swap in ipairs((Host.entry_swaps() or { swaps = {} }).swaps) do
	print(swap.target, table.concat(swap.options, ", "))
end
```

## Host.menu_open

Definition:
```luau
Host.menu_open: () -> boolean
```
Whether the right-click menu is open. Errors when the agent hasn't found the flag. Cancels async input.

Example:
```luau
print(Host.menu_open())
```

## Host.menu_layout

Definition:
```luau
Host.menu_layout: () -> MenuLayout?
```
The open menu as the client's own layout routine places it, in interface units (multiply by `game_state().ui_scale` for pixels): menu `x`, `y`, `width`, `height`, line area `rows`, `header_height`, `row_height`, `scroll`, and `margin` (how far outside the mouse may go before it closes). `submenus` (up to 4 deep, outermost first) have the same fields plus `row` (1-based opener line) and `entries`. Nil when closed. Cancels async input.

Example:
```luau
local menu = Host.menu_layout()
if menu then
	print(menu.x, menu.y, menu.width, menu.height, #menu.submenus)
end
```

## Host.selected_widget

Definition:
```luau
Host.selected_widget: () -> SelectedWidget?
```
What is waiting for a target, like a selected spell. Nil when nothing is. See [Magic](magic.md).

Example:
```luau
local s = Host.selected_widget()
if s then
	print(s.verb, s.name)
end
```

## Host.sub_interfaces

Definition:
```luau
Host.sub_interfaces: () -> { SubInterface }?
```
Interfaces open inside other interfaces.

Example:
```luau
for _, sub in ipairs(Host.sub_interfaces() or {}) do
	print(sub.group, "inside", sub.parent_group)
end
```

## Host.game_val_name

Definition:
```luau
Host.game_val_name: (kind: number, id: number) -> string?
```
Readable name of a game value of `kind` (see [GameVal](game_val.md)).

Example:
```luau
print(Host.game_val_name(0, 1))
```

## Host.game_val_names

Definition:
```luau
Host.game_val_names: (kind: number, ids: { number }) -> { string? }?
```
Batch `Host.game_val_name`.

Example:
```luau
print(#(Host.game_val_names(0, { 1, 2, 3 }) or {}))
```

## Host.item_name

Definition:
```luau
Host.item_name: (id: number) -> string?
```
An item's name.

Example:
```luau
print(Host.item_name(995))
```

## Host.item_names

Definition:
```luau
Host.item_names: (ids: { number }) -> { string? }?
```
Batch `Host.item_name`.

Example:
```luau
print((Host.item_names({ 995, 554 }) or {})[2])
```

## Host.varc

Definition:
```luau
Host.varc: (id: number) -> number?
```
A client variable (varc).

Example:
```luau
print(Host.varc(74))
```

## Host.varp

Definition:
```luau
Host.varp: (id: number) -> number?
```
A player variable (varp). See [Varps](varps.md).

Example:
```luau
print(Host.varp(173))
```

## Host.varps

Definition:
```luau
Host.varps: () -> { number }?
```
Every varp; id `n` is at index `n + 1`.

Example:
```luau
print(#(Host.varps() or {}))
```

## Host.varbit

Definition:
```luau
Host.varbit: (id: number) -> number?
```
A varbit; its varp and bit range are read from the cache once per script. See [Varbits](varbits.md).

Example:
```luau
print(Host.varbit(4070))
```

## Host.enum_def

Definition:
```luau
Host.enum_def: (id: number) -> EnumDef?
```
A cache enum (`key_type`, `value_type`, `default`, `entries`). Nil when it doesn't exist.

Example:
```luau
for _, e in ipairs((Host.enum_def(1981) or { entries = {} }).entries) do
	print(e.key, e.value)
end
```

## Host.world_route

Definition:
```luau
Host.world_route: (from: NavTile, to: NavTile, allow_climbs: { string }?) -> WorldRoute?
```
Whether `to` is reachable from `from` anywhere on the map, across planes and underground, via the map's climbs and links plus gated ones in `allow_climbs` (`Transports.allowed_climbs`). `known` is false when either point is on no landmass; `from`/`to` of the result are the first climb, absent when both points share a landmass. The first call builds the map (under a second once the collision file exists).

Example:
```luau
local Transports = require("@volcren/transports")

local way = Host.world_route({ x = 3222, y = 3218, plane = 0 }, { x = 2884, y = 9798, plane = 0 }, Transports.allowed_climbs())
print(way and way.reachable)
```

## Host.landmasses

Definition:
```luau
Host.landmasses: (points: { { x: number, y: number, plane: number } }) -> ({ number }?, string?)
```
Landmass number per point, from a flood fill of the cache collision on every plane (closed doors open, climbs not followed): equal numbers are walkable between. 0 means no standable tile within 3 tiles. The first call after a game update takes a few seconds.

Example:
```luau
local n = Host.landmasses({ { x = 3213, y = 3424, plane = 0 }, { x = 2797, y = 3175, plane = 0 } })
print(n and n[1] == n[2])
```

## Host.cache_file

Definition:
```luau
Host.cache_file: (index: number, archive: number, file: number) -> (string?, string?)
```
One cache file's bytes, nil if missing. Scripts are index 12 (archive = script id, file 0); configs index 2.

Example:
```luau
print(#(Host.cache_file(12, 4024, 0) or ""))
```

## Host.cache_files

Definition:
```luau
Host.cache_files: (index: number, files: { { number } }) -> ({ string | boolean }?, string?)
```
Batch `Host.cache_file` over `{ archive, file }` pairs; `false` for missing files. Much faster than separate calls.

Example:
```luau
print(#(Host.cache_files(2, { { 38, 17 }, { 38, 18 } }) or {}))
```

## Host.item_params

Definition:
```luau
Host.item_params: (ids: { number }) -> { { [number]: CacheValue }? }?
```
Each item's cache params by param id; nil for unknown items.

Example:
```luau
print(((Host.item_params({ 3281 }) or {})[1] or {})[601])
```

## Host.item_stackable

Definition:
```luau
Host.item_stackable: (ids: { number }) -> { boolean? }?
```
Whether each item stacks in the inventory, from the cache; nil for unknown ids.

Example:
```luau
print((Host.item_stackable({ 995, 1127 }) or {})[1]) -- true
```

## Host.item_placeholder

Definition:
```luau
Host.item_placeholder: (ids: { number }) -> { boolean? }?
```
Whether each id is a bank placeholder (made from the placeholder template), from the cache; nil for unknown ids. A placeholder has its own id, named like its item.

Example:
```luau
print((Host.item_placeholder({ 29471, 29470 }) or {})[1]) -- true
```

## Host.npcs

Definition:
```luau
Host.npcs: () -> { Npc }?
```
Loaded NPCs. `head_icons` are `{ archive, sprite }` pairs (archive 440 is prayers; `Npcs.overhead` names them).

Example:
```luau
for _, npc in ipairs(Host.npcs() or {}) do
	print(npc.name, npc.x, npc.y)
end
```

## Host.npc_model

Definition:
```luau
Host.npc_model: (index: number) -> NpcModel?
```
On-screen outline of an NPC by `index`.

Example:
```luau
local npc = (Host.npcs() or {})[1]
print(npc and Host.npc_model(npc.index))
```

## Host.npc_type_model

Definition:
```luau
Host.npc_type_model: (
	id: number,
	animation: number,
	frame: number,
	fine_x: number,
	fine_y: number,
	orientation: number
) -> NpcModel?
```
Builds an NPC type's model posed at `frame` of `animation` (-1 for none) and returns its outline as if standing at local fine `fine_x`, `fine_y` facing `orientation` (0 to 2047). Adds nothing to the game. `frame_count` is the animation's frame count. Nil for an id without a model; errors for a missing frame or a skeletal animation. See `examples/show_npc_animation.luau`.

Example:
```luau
local pos = assert(Host.game_state().position)
local model = Host.npc_type_model(2042, 5069, 0, pos.local_x + 256, pos.local_y, 0)
print(model and model.frame_count)
```

## Host.npc_sizes

Definition:
```luau
Host.npc_sizes: (indices: { number }) -> { [number]: number }?
```
Tile size of each NPC by index; gone NPCs are omitted. Used by [SafeSpot](safespot.md).

Example:
```luau
local npc = (Host.npcs() or {})[1]
print(npc and (Host.npc_sizes({ npc.index }) or {})[npc.index])
```

## Host.npc_actions

Definition:
```luau
Host.npc_actions: (ids: { number }) -> { [number]: { string } }?
```
The menu options of each NPC type by id, read from the cache after the type's variable picks what it shows ("Hidden" left out). Ids that show nothing are omitted. Use `Npcs.options`.

Example:
```luau
local npc = (Host.npcs() or {})[1]
print(npc and table.concat((Host.npc_actions({ npc.id }) or {})[npc.id] or {}, ", "))
```

## Host.npc_hitsplats

Definition:
```luau
Host.npc_hitsplats: (indices: { number }) -> { [number]: { Hitsplat } }?
```
Hitsplats per NPC index; gone NPCs are omitted. An actor keeps its last four (`slot` 0 to 3), a new hit replacing the oldest, so poll at least once a tick. `kind` 16 is damage and 12 a block for hits involving you; `cycle` is when it stops showing (about 50 cycles after landing; compare with `Host.client_cycle`). [NpcHealth](npc_health.md) builds on this.

Example:
```luau
local npc = (Host.npcs() or {})[1]
if npc then
	for _, s in ipairs((Host.npc_hitsplats({ npc.index }) or {})[npc.index] or {}) do
		print(s.damage, s.cycle)
	end
end
```

## Host.player_hitsplats

Definition:
```luau
Host.player_hitsplats: (indices: { number }) -> { [number]: { Hitsplat } }?
```
`Host.npc_hitsplats` for players by `PlayerInfo.index`. On yourself, kind 16 is damage taken and 12 a miss.

Example:
```luau
local Players = require("@volcren/players")

local me = assert(Players.me())
print(#((Host.player_hitsplats({ me.index }) or {})[me.index] or {}))
```

## Host.npc_overhead_text

Definition:
```luau
Host.npc_overhead_text: (indices: { number }) -> { [number]: string }?
```
The text an NPC is saying over its head, per index; NPCs saying nothing and gone NPCs are omitted. Some NPCs speak only here, never in the chatbox. The client clears it when it stops showing, so poll while you wait for it.

Example:
```luau
local npc = (Host.npcs() or {})[1]
print(npc and (Host.npc_overhead_text({ npc.index }) or {})[npc.index])
```

## Host.player_overhead_text

Definition:
```luau
Host.player_overhead_text: (indices: { number }) -> { [number]: string }?
```
`Host.npc_overhead_text` for players by `PlayerInfo.index`: their latest public chat while it shows.

Example:
```luau
local Players = require("@volcren/players")

local me = assert(Players.me())
print((Host.player_overhead_text({ me.index }) or {})[me.index])
```

## Host.client_cycle

Definition:
```luau
Host.client_cycle: () -> number?
```
The client cycle counter (50 per second) that hitsplat, projectile and spot animation cycles use.

Example:
```luau
print(Host.client_cycle())
```

## Host.locs

Definition:
```luau
Host.locs: (plane: number?) -> { Loc }?
```
Scene objects on a plane (default yours). `clickable` is the client's flag, which also makes the minimap draw a wall red: a clickable wall is a door, gate or curtain.

Example:
```luau
for _, loc in ipairs(Host.locs() or {}) do
	if loc.name == "Bank booth" then
		print(loc.x, loc.y)
	end
end
```

## Host.loc_shown

Definition:
```luau
Host.loc_shown: (ids: { number }) -> { LocView }?
```
How each object id looks now (`name`, `actions`), resolving multi-state objects by their current varbit/varp. Pass the placed ids (from `Host.cache_locs`). Unknown ids give an empty name; nil when the cache can't be read.

Example:
```luau
local v = Host.loc_shown({ 26261 })
print(v and v[1].name)
```

## Host.cache_locs

Definition:
```luau
Host.cache_locs: (area: Corners, filter: CacheLocFilter?) -> { CacheLoc }?
```
Objects the cache places inside `area` (corners included) on every plane, loaded or not. `plane` is the one the object is walked on: an object on a bridge tile is a plane lower than the cache stores it, as in the game and the collision data. The filter matches `plane`, substrings of `names` and whole `actions` (case-insensitive). At most 64 map squares. Reflects the cache only: no runtime changes, transforming objects as their first form. 80 x 80 tiles takes about 50 ms.

Example:
```luau
local me = Host.game_state().position
if me then
	local area = { x1 = me.x - 40, y1 = me.y - 40, x2 = me.x + 40, y2 = me.y + 40 }
	for _, loc in ipairs(Host.cache_locs(area, { actions = { "Climb-up" } }) or {}) do
		print(loc.name, loc.x, loc.y, loc.plane)
	end
end
```

## Host.loc_model

Definition:
```luau
Host.loc_model: (loc: Loc) -> NpcModel?
```
On-screen outline of a scene object.

Example:
```luau
local loc = (Host.locs() or {})[1]
print(loc and Host.loc_model(loc))
```

## Host.ground_items

Definition:
```luau
Host.ground_items: () -> { GroundItem }?
```
Ground items on your plane. Besides the item and its tile, each carries what the server sent when it appeared; these fields are nil when the agent couldn't find them in this client:

| Field | Meaning |
| --- | --- |
| `public_tick` | The `tick_count` (`Host.game_state`) from which everyone can see it; before that only its owner can. |
| `despawn_tick` | The `tick_count` at which the server removes it. |
| `ownership` | 0 nobody, 1 you, 2 another player, 3 your group (group ironman). |
| `never_public` | True when it stays private to its owner. |
| `op_flags` | Bit `n` set when ground option `n + 1` of the item's definition is enabled; 31 enables all five. |

None of them change while the item lies there: `ownership` still says 1 after `public_tick` has passed, so compare the ticks with `tick_count` to know whether it is public or how long it has left.

Example:
```luau
local now = Host.game_state().tick_count or 0
for _, item in ipairs(Host.ground_items() or {}) do
	print(item.name, item.quantity, item.x, item.y, item.ownership, item.despawn_tick and item.despawn_tick - now)
end
```

## Host.projectiles

Definition:
```luau
Host.projectiles: () -> { Projectile }?
```
Projectiles in flight, plus sent ones not yet launched. Helpers in [Projectiles](projectiles.md).

Example:
```luau
for _, p in ipairs(Host.projectiles() or {}) do
	print(p.id, p.end_cycle - p.cycle)
end
```

## Host.spot_anims

Definition:
```luau
Host.spot_anims: () -> { SpotAnim }?
```
Spot animations placed on tiles. Effects on an actor are its `graphic` instead. Helpers in [Spot animations](spot_anims.md).

Example:
```luau
for _, s in ipairs(Host.spot_anims() or {}) do
	print(s.id, s.x, s.y)
end
```

## Host.map_regions

Definition:
```luau
Host.map_regions: () -> { { number } }?
```
The 64x64 map squares (`{ x, y }`, tile / 64) the scene is built from; in an instance, the copied source squares. Wrapped by `Player.map_chunks`.

Example:
```luau
for _, r in ipairs(Host.map_regions() or {}) do
	print(r[1], r[2])
end
```

## Host.in_instance

Definition:
```luau
Host.in_instance: () -> boolean?
```
The client's instance flag; nil when unreadable. Wrapped by `Player.in_instance`.

Example:
```luau
print(Host.in_instance())
```

## Host.server_tile

Definition:
```luau
Host.server_tile: () -> ServerTile?
```
The tile the server last put you on, up to a tick or two ahead of `game_state().position`; `steps` is how many path steps are left to animate. Nil when logged out.

Example:
```luau
local t = Host.server_tile()
print(t and t.x, t and t.y, t and t.steps)
```

## Host.movement

Definition:
```luau
Host.movement: () -> MovementState?
```
Your server tile and whether the server moved you this tick, tracked per packet so no tick is missed. `moving` is true from the tick that moves you until the next tick ends. Teleports, logins and hops count as one move; `from_x`, `from_y`, `from_plane` give the distance (walking 1 tile per tick, running 2). Nil when logged out. Wrapped by `Player.is_moving`.

Example:
```luau
while Host.wait_packet({ "TICK" }, 1200) do
	print((Host.movement() or {}).moving)
end
```

## Host.chat_messages

Definition:
```luau
Host.chat_messages: () -> { ChatMessage }?
```
Chat history, oldest first, read from memory: up to 100 per type, including filtered messages and dialogue lines, chatbox open or not. `time` is accurate to the second. `Chat.messages` gives a cleaned version.

Example:
```luau
for _, m in ipairs(Host.chat_messages() or {}) do
	print(m.time_utc, m.name, m.text)
end
```

## Host.players

Definition:
```luau
Host.players: () -> { PlayerInfo }?
```
Other loaded players. `table.find(p.movement, p.pose) == nil` means something other than standing or moving holds the player.

Example:
```luau
for _, p in ipairs(Host.players() or {}) do
	print(p.name, p.x, p.y)
end
```

## Host.player_model

Definition:
```luau
Host.player_model: (index: number) -> NpcModel?
```
On-screen outline of a player.

Example:
```luau
local p = (Host.players() or {})[1]
print(p and Host.player_model(p.index))
```

## Host.find_path

Definition:
```luau
Host.find_path: (from: Tile, to: Tile, plane: number?, max_tiles: number?) -> { Tile }?
```
Walking route on one plane (default yours) over the whole map's collision, both ends included. Ignores climbs and treats closed doors as walls. `max_tiles` caps the search, which otherwise floods everything reachable for an unreachable target. The first call after a game update takes about ten seconds.

Example:
```luau
local me = Host.game_state().position
if me then
	print(#(Host.find_path({ x = me.x, y = me.y }, { x = me.x + 10, y = me.y }) or {}))
end
```

## Host.nav_plan

Definition:
```luau
Host.nav_plan: (from: Tile | NavTile, to: Tile | NavTile, options: NavOptions?) -> (NavPlan?, string?)
```
Plans a walk across all planes, through closed doors and climbs. A tile without `plane` is on yours.

- **Doors**: closed, clickable, blocking walls with one of `door_options`; passing costs `door_cost`.
- **Climbs**: plane changes from the Shortest Path `transports.tsv` (stairs, ladders, ropes, caves; may land on any plane or underground, 6400 tiles north), tied to the clickable object offering the row's option (row id first, else nearest within 2 tiles); cost `climb_cost`. Gated rows need their name in `allow_climbs`.
- **Links**: same-plane rows up to 15 tiles (stiles, object doors), treated as climbs. A one-tile "Open" hop is a wall door. Longer rows are `Transports.ride`. Wilderness rows are used only when the plan starts or ends there.
- **Agility shortcuts**: `agility_shortcuts.tsv` rows, costing `climb_cost` + 4 per tick of `Duration` + `shortcut_saving`.

`tiles` are every tile walked with its plane; `actions` the doors and climbs in order, `at` being the index of the tile used from. Beyond the scene the plan uses cache collision (doors open) up to about 400 tiles, then plans to `goal` on the world path with `reached` false. Unreachable targets plan to the closest tile, `reached` false. Instances use only the scene.

Options: `near` (end within this many tiles, 0), `door_options` (`{ "Open" }`; `{}` disables doors), `climbs` (`true`), `shortcut_saving` (10 tiles), `allow_climbs` (`"x,y,plane>x,y,plane|menu"`, see `Transports.allowed_climbs`), `avoid` (locs to skip), `seed` and `rnd` (with a seed each tile cost scales by 1 ± `rnd`/2, default 0.6), `door_cost` (8), `climb_cost` (50). Costs are in tenths of a tile.

Example:
```luau
local me = Host.game_state().position
if me then
	local plan = Host.nav_plan({ x = me.x, y = me.y }, { x = me.x, y = me.y, plane = 0 })
	for _, a in ipairs(plan and plan.actions or {}) do
		print(a.kind, a.loc.option, a.to_plane)
	end
end
```

## Host.scene_route

Definition:
```luau
Host.scene_route: (from: Tile, to: Tile, options: SceneRouteOptions?) -> SceneRoute?
```
The route the game walks when clicking `to` from `from`, start first, using the game's route finder on the live scene (works in instances, knows opened doors). Unreachable: `reached` false and the route ends at the nearest reachable tile within 10.

With `options` (even `{}`) the route is randomized over a jittered lattice of walkable nodes with noisy edge costs; short, unreachable or over-`max_detour` routes fall back to the game's. `to` may be outside the scene: the route then spans map squares (up to about 400 tiles) using live collision for seen tiles and cache collision elsewhere; if unreachable within that, nil (use `Host.find_path`).

Options: `seed` (random), `rnd` (0.8 = ±40%), `spacing` (8 tiles), `jitter` (2), `max_detour` (1.3x), `offset` (per-call node shift, 2; 0 makes a seed repeatable).

Example:
```luau
local me = Host.game_state().position
if me then
	local route = Host.scene_route({ x = me.x, y = me.y }, { x = me.x + 8, y = me.y - 4 }, {})
	print(route and #route.tiles, route and route.reached)
end
```

## Host.reach_route

Definition:
```luau
Host.reach_route: (from: Tile, to: ReachTarget) -> (SceneRoute?, string?)
```
The route the player walks when they pick an option on `to` from `from`, start first, on the live scene. The server walks the player for NPC, object and item options, not the client, so this follows the server's reach rules: an NPC is used from a tile beside it with no wall between, never from under it; a ground item only from its own tile; an object from its own tile or a side with no wall between, and walls and wall decorations only from the sides their shape faces. An object's blocked sides (staircases used from one side) aren't known, so every open side counts. Unreachable: `reached` false and the route ends where the server stops, the reachable tile nearest `to` within 10 (fewest steps on a tie). That can be `from` itself: picking a banker behind a wall from right outside it gets "I can't reach that".

Example:
```luau
local me = Host.game_state().position
local npc = Npcs.nearest("Banker", me)
if me and npc then
	local route = Host.reach_route({ x = me.x, y = me.y }, Npcs.reach(npc))
	print(route and route.reached)
end
```

## Host.reach_tiles

Definition:
```luau
Host.reach_tiles: (to: ReachTarget) -> ({ Tile }?, string?)
```
The walkable tiles `to` is used from under `Host.reach_route`'s rules, whether or not they can be walked to from here. Empty for an item on a blocked tile, such as a table.

Example:
```luau
local loc = Objects.nearest("Bank booth")
for _, t in ipairs(loc and Host.reach_tiles(Objects.reach(loc)) or {}) do
	print(t.x, t.y)
end
```

## Host.collision

Definition:
```luau
Host.collision: (area: Corners?) -> (Collision?, string?)
```
Live collision flags of your plane for the scene or `area`. Tile (`x + i`, `y + j`) is `flags[i * height + j + 1]`; [SafeSpot](safespot.md) names the flags. The outer ring has every flag set. Objects the client leaves out of its own map because their definition is hollow (Lumbridge's open bank booths) are blocked, as the server blocks them. About 2 ms.

Example:
```luau
local me = Host.game_state().position
if me then
	local c = Host.collision({ x1 = me.x, y1 = me.y, x2 = me.x, y2 = me.y + 1 })
	print(c and string.format("%x", c.flags[2]))
end
```

## Host.world_select

Definition:
```luau
Host.world_select: () -> WorldSelect?
```
The login world switcher: `worlds` (every world, in the client's order), `current` (the world the login screen will use) and, while the list is `open`, its `panel`: `shown` (the worlds the list shows now, after the filters and sort, in display order), `page`, `compact`, `rows` and `columns` per page, the `list` rect, `row_offset` and `row_height`, the `previous` and `next` page arrows, and `selected` / `hovered` (indices into `worlds`, -1 for none). Rects are in the switcher's own 765x503 units; `LoginWorldSwitcher` maps them to the screen.

Example:
```luau
local ws = Host.world_select()
print(ws and ws.open, ws and #ws.worlds)
```

## Host.click

Definition:
```luau
Host.click: (x: number, y: number, button: string?, hold_ms: number?) -> ()
```
Teleports the mouse and clicks, holding `hold_ms` (default 60). Prefer [Mouse](mouse.md).

Example:
```luau
Host.click(100, 100)
```

## Host.move

Definition:
```luau
Host.move: (x: number, y: number) -> ()
```
Teleports the mouse.

Example:
```luau
Host.move(200, 150)
```

## Host.wind_mouse

Definition:
```luau
Host.wind_mouse: (x: number, y: number, options: WindMouseOptions?) -> ()
```
Moves along a curved human-like path. Options: `speed`, `wind`, `gravity`, `timeout`, `pace` (default 1; 1.2 is 20% faster, hesitation pauses unchanged).

Example:
```luau
Host.wind_mouse(300, 200, { speed = 12 })
```

## Host.mouse_down

Definition:
```luau
Host.mouse_down: (button: string?) -> ()
```
Presses and holds a mouse button.

Example:
```luau
Host.mouse_down()
Host.sleep(100)
Host.mouse_up()
```

## Host.mouse_up

Definition:
```luau
Host.mouse_up: (button: string?) -> ()
```
Releases a mouse button.

Example:
```luau
Host.mouse_up()
```

## Host.scroll

Definition:
```luau
Host.scroll: (notches: number) -> ()
```
Turns the wheel by `notches`.

Example:
```luau
Host.scroll(-3)
```

## Host.key_down

Definition:
```luau
Host.key_down: (key: number) -> ()
```
Presses and holds a key (virtual key code).

Example:
```luau
Host.key_down(0x25)
Host.sleep(300)
Host.key_up(0x25)
```

## Host.key_up

Definition:
```luau
Host.key_up: (key: number) -> ()
```
Releases a key.

Example:
```luau
Host.key_up(0x25)
```

## Host.type_text

Definition:
```luau
Host.type_text: (text: string, hold_ms: number?) -> ()
```
Types text, holding each key `hold_ms`. Common characters use US layout keys (the client reads key codes as US); others ask the system layout, and an unmappable character errors.

Example:
```luau
Host.type_text("hello")
```

## Async input

The `_async` functions take the same arguments as their blocking versions but queue input on a worker thread and return immediately; queued input runs in order. The queue is cancelled and dropped by blocking input, `Host.input_cancel`, `Host.menu`/`menu_open`/`menu_layout`, or script end. Cancelled clicks and typing still release their buttons and keys; a cancelled move stops where it is; buttons and keys held by `mouse_down_async`/`key_down_async` are released last-pressed first. Prefer [Mouse](mouse.md).

## Host.click_async

Definition:
```luau
Host.click_async: (x: number, y: number, button: string?, hold_ms: number?) -> ()
```
Queues `Host.click`.

Example:
```luau
Host.click_async(100, 100)
```

## Host.move_async

Definition:
```luau
Host.move_async: (x: number, y: number) -> ()
```
Queues `Host.move`.

Example:
```luau
Host.move_async(200, 150)
```

## Host.wind_mouse_async

Definition:
```luau
Host.wind_mouse_async: (x: number, y: number, options: WindMouseOptions?) -> ()
```
Queues `Host.wind_mouse`, starting from wherever the mouse is when it runs.

Example:
```luau
Host.wind_mouse_async(300, 200, { speed = 12 })
```

## Host.type_text_async

Definition:
```luau
Host.type_text_async: (text: string, hold_ms: number?) -> ()
```
Queues `Host.type_text`.

Example:
```luau
Host.type_text_async("hello")
```

## Host.mouse_down_async

Definition:
```luau
Host.mouse_down_async: (button: string?) -> ()
```
Queues `Host.mouse_down`; released on cancel.

Example:
```luau
Host.mouse_down_async("middle")
Host.wind_mouse_async(400, 300)
Host.mouse_up_async("middle")
```

## Host.mouse_up_async

Definition:
```luau
Host.mouse_up_async: (button: string?) -> ()
```
Queues `Host.mouse_up`.

Example:
```luau
Host.mouse_up_async("middle")
```

## Host.scroll_async

Definition:
```luau
Host.scroll_async: (notches: number) -> ()
```
Queues `Host.scroll`.

Example:
```luau
Host.scroll_async(2)
```

## Host.key_down_async

Definition:
```luau
Host.key_down_async: (key: number) -> ()
```
Queues `Host.key_down`; released on cancel.

Example:
```luau
Host.key_down_async(0x25)
Host.sleep_async(300)
Host.key_up_async(0x25)
```

## Host.key_up_async

Definition:
```luau
Host.key_up_async: (key: number) -> ()
```
Queues `Host.key_up`.

Example:
```luau
Host.key_up_async(0x25)
```

## Host.sleep_async

Definition:
```luau
Host.sleep_async: (ms: number) -> ()
```
Queues a pause before the next queued input; cut short by cancel.

Example:
```luau
Host.key_down_async(0x25)
Host.sleep_async(300)
Host.key_up_async(0x25)
```

## Host.input_busy

Definition:
```luau
Host.input_busy: () -> boolean
```
Whether async input is running or queued.

Example:
```luau
print(Host.input_busy())
```

## Host.input_wait

Definition:
```luau
Host.input_wait: () -> string?
```
Blocks until async input finishes; returns the error of a failed async input since the last check, or nil.

Example:
```luau
Host.move_async(200, 150)
print(Host.input_wait())
```

## Host.input_error

Definition:
```luau
Host.input_error: () -> string?
```
Non-blocking `Host.input_wait`: the error since the last check, or nil.

Example:
```luau
print(Host.input_error())
```

## Host.input_cancel

Definition:
```luau
Host.input_cancel: () -> ()
```
Cancels async input and drops the queue; returns once nothing is being sent.

Example:
```luau
Host.input_cancel()
```

## Host.act

Definition:
```luau
Host.act: (spec: ActSpec) -> ActResult
```
Moves the mouse onto a moving model and clicks it on the frame the client shows it under the mouse. The agent re-projects `model` every frame and keeps a spot on one of its triangles (centre-weighted by `spread`, preferring triangles of `min_area` px², skipping any whose centre is outside `viewport` or inside `blockers`); the injector runs WindMouse (`mouse`) after that spot. Once a menu entry matches `entry` (`kind`, `id`, `index`, `x`, `y`, each optional) and the mouse is within `near` px of the spot (widened by the spot's speed), the agent presses: left when the left-click entry is one of `options`, else right when `right` (leaving the menu open). The injector releases after `hold` ms. `status` is `"left"`, `"right"`, `"no_option"` (the target has none of `options`), `"gone"` (no model for 30 frames), `"menu_open"`, `"timeout"` (`timeout` ms, default 3000), `"cancelled"`, or why the agent can't (`"other_thread"`, `"not_in_game"`, `"not_found"`, `"lost"`); `x`, `y` and `entry` are the mouse and the entry at the press. Blocking input; players aren't matched by name, so use it for NPCs, locs and ground items. Used by [Clickbox.interact](clickbox.md#clickboxinteract).

Example:
```luau
local item = assert(Host.ground_items())[1]
local result = Host.act({
	model = { target = "loc", kind = 4, scene_x = item.scene_x, scene_y = item.scene_y, id = item.id },
	entry = { kind = 4, id = item.id },
	options = { "Take" },
	viewport = { x1 = 0, y1 = 0, x2 = 765, y2 = 503 },
})
print(result.status)
```

## Host.bank_pin_enter

Definition:
```luau
Host.bank_pin_enter: () -> string
```
Enters the account's bank PIN on the open keypad inside the injector; the PIN comes from the loader by account name hash and never reaches the script. Returns `"entered"`, `"closed"`, `"no_pin"`, `"unavailable"`, `"locked"`, `"wrong"` or `"failed"`. Blocking input. Use [BankpinKeypad.enter](bankpin_keypad.md).

## Host.AccountSettings

Definition:
```luau
export type AccountSettings = {
	off: { string },
	house: { string },
}
```
`off`: transport types the user turned off for the account. `house`: the `House.SAVED` pieces the user says the house has.

## Host.account_settings

Definition:
```luau
Host.account_settings: () -> (AccountSettings?, string?)
```
What the loader's Accounts page holds for the logged-in account (looked up by name hash, so no other account is reachable); can change mid-run. Nil when the account isn't saved, logged out or no loader; nil and a reason when there is no client. Use `Transports.is_enabled` and `House.seen`.

Example:
```luau
local account = Host.account_settings()
print(account and #account.off)
```

## Host.lamp_skill

Definition:
```luau
Host.lamp_skill: () -> (string?, string?)
```
The lamp skill set in the loader for the logged-in account (looked up by name hash, so no other account is reachable). Nil when unset, logged out or no loader; nil and a reason when there is no client or the loader fails. Use `Randoms.lamp_skill`.

Example:
```luau
print(Host.lamp_skill())
```

## Host.http_request

Definition:
```luau
Host.http_request: (options: HttpRequest) -> (HttpResponse?, string?)
```
Blocking HTTP request made by the loader, only to hosts on its whitelist (Volcren and the wiki prices API are always allowed). Options: `url`, `method` (`"GET"`), `headers`, `body` (≤ 1 MB), `timeout` (15000 ms, max 60000, redirects included). Response: `ok` (2xx), `status`, `headers` (lowercase, repeats joined with `", "`), `body` (≤ 8 MB). Non-2xx is still a response; only a failed request returns an error. Redirects stay within the whitelist and drop `Authorization`/`Cookie` across hosts.

Example:
```luau
local res, err = Host.http_request({ url = "https://api.volcren.com/health" })
print(res and res.status, err)
```

## Host.item_prices

Definition:
```luau
Host.item_prices: (items: { number | string }) -> ({ ItemPrice? }?, string?)
```
Grand Exchange prices from the wiki prices API, in order of `items` (ids or case-insensitive names). Unlisted ids (noted, placeholders) resolve by client name. Nil for untradeable items; price fields nil when unknown. Data is cached per run: prices refresh after 10 minutes, the item list after a day; a failed refresh keeps old data and retries after a minute. The first call blocks on a ~1 MB download. Use [ItemData](item_data.md).

Example:
```luau
local prices = Host.item_prices({ 4151, "Rune scimitar" })
print(prices and prices[1] and prices[1].high)
```

## Host.draw_rect

Definition:
```luau
Host.draw_rect: (x1: number, y1: number, x2: number, y2: number, r: number, g: number, b: number, a: number?, layer: Layer?) -> ()
```
Draws a rectangle on the overlay (colors 0 to 255). `layer` is `"world"` (default), which hides under game interfaces, or `"ui"`, which never does; use `"ui"` for things that mark interfaces. Any other value is an error.

Example:
```luau
Host.draw_rect(50, 50, 150, 100, 255, 0, 0)
```

## Host.draw_line

Definition:
```luau
Host.draw_line: (x1: number, y1: number, x2: number, y2: number, r: number, g: number, b: number, a: number?, layer: Layer?) -> ()
```
Draws a line on the overlay, on `layer` as in `Host.draw_rect`.

Example:
```luau
Host.draw_line(0, 0, 200, 200, 0, 255, 0)
```

## Host.draw_lines

Definition:
```luau
Host.draw_lines: (lines: { { number } }, r: number, g: number, b: number, a: number?, layer: Layer?) -> ()
```
Draws many `{ x1, y1, x2, y2 }` lines of one color, on `layer` as in `Host.draw_rect`.

Example:
```luau
Host.draw_lines({ { 0, 0, 100, 0 }, { 100, 0, 100, 100 } }, 255, 255, 0)
```

## Host.clear_overlay

Definition:
```luau
Host.clear_overlay: () -> ()
```
Clears the overlay.

Example:
```luau
Host.clear_overlay()
```

## Host.draw_batch

Definition:
```luau
Host.draw_batch: (shapes: { Shape }) -> ()
```
Draws shapes in order as one atomic overlay update. A shape is `{ kind = "rect" | "line", x1, y1, x2, y2, r, g, b, a?, layer? }` or `{ kind = "clear" }`, with `layer` as in `Host.draw_rect`. `Draw.batch` builds the list.

Example:
```luau
Host.draw_batch({
	{ kind = "clear" },
	{ kind = "rect", x1 = 50, y1 = 50, x2 = 150, y2 = 100, r = 255, g = 0, b = 0 },
})
```

## Host.debug_shapes

Definition:
```luau
Host.debug_shapes: (shapes: { DebugShape }) -> ()
```
Replaces this run's world drawings, which the agent projects every frame so they follow the camera. A shape is `{ kind = "path", points, r, g, b, a?, width?, closed? }` (a line through `points`, `width` 1 to 16 pixels, default 2) , `{ kind = "label", at, text, r, g, b, a?, bold? }` (game-font text centred on `at`, its bottom at the point, with a shadow; `\n` splits it into centred lines), `{ kind = "screen_label", x, y, text, r, g, b, a?, bold? }` (the same text with its bottom-left at screen pixel `x, y` instead of a world point, drawn over interfaces) or `{ kind = "loc", loc_kind, x, y, id, faces?, hull? }` (the model the client drew this frame for the loc of that kind and id at world tile `x, y`: its triangle edges in the `faces` color and its outline in `hull`, each `{ r, g, b, a? }`). Points are `FinePoint`s in world units, 128 per tile from tile 0, on the ground of the player's plane plus `z` (negative is up); `tile` picks the tile whose ground they sit on. They show only while Script debugging is on and come back when it's turned on again, and the agent drops them when the run's process ends. At most 20000 shapes, 100000 points and 16 locs, labels of either kind up to 200 bytes. `DebugDraw` builds them; use it instead.

Example:
```luau
Host.debug_shapes({
	{ kind = "path", points = { { x = 3200 * 128, y = 3200 * 128 }, { x = 3201 * 128, y = 3200 * 128 } }, r = 255, g = 0, b = 0 },
	{ kind = "label", at = { x = 3200 * 128 + 64, y = 3200 * 128 + 64 }, text = "Here", r = 255, g = 255, b = 255 },
})
```

## Host.sleep

Definition:
```luau
Host.sleep: (ms: number) -> ()
```
Pauses the script; item and packet callbacks run meanwhile.

Example:
```luau
Host.sleep(1000)
```

## Host.time

Definition:
```luau
Host.time: () -> number
```
Monotonic milliseconds since the script started.

Example:
```luau
local start = Host.time()
Host.sleep(500)
print(Host.time() - start)
```

## Host.unix_time

Definition:
```luau
Host.unix_time: () -> number
```
Seconds since the Unix epoch, fractional. Useful with varps that store a moment, such as varp 892 (minute of the last home teleport).

Example:
```luau
print((Host.unix_time() / 60) - (Host.varp(892) or 0))
```

## Host.clock

Definition:
```luau
Host.clock: () -> number
```
Seconds since local midnight, fractional; for scheduling at a time of day.

Example:
```luau
print(Host.clock() // 3600)
```

## Host.antiban_config

Definition:
```luau
Host.antiban_config: () -> AntibanConfig
```
The loader's break and sleep config (default when absent): `breaks_enabled`, `sleeps_enabled`, `breaks` (`interval`, `length`, `randomness?`, `logout_chance?`), `sleeps` (`time?`, `length?`, `randomness?`, `logout_chance?`), and `version`, bumped on each new config. Times in ms; omitted fields are Auto. Read via `Antiban.sync_config`.

Example:
```luau
print(#Host.antiban_config().breaks)
```

## Host.antiban_resume

Definition:
```luau
Host.antiban_resume: () -> BreakPlan?
```
The break plan the previous queue entry last reported, already counted down to now, or nil outside a queue. Returned once. `Antiban` uses it so a queue breaks like one long run.

Example:
```luau
local plan = Host.antiban_resume()
print(plan and #plan.breaks)
```

## Host.antiban_skips

Definition:
```luau
Host.antiban_skips: () -> { { kind: string, index: number } }
```
"Skip next" presses in the loader since the last call, oldest first; `kind` is `"break"` or `"sleep"`, `index` 0-based. Handled by `Antiban.sync_config`.

## Host.transports

Definition:
```luau
Host.transports: () -> TransportsData
```
The loader's parsed Shortest Path transport data (BSD-2-Clause, github.com/Skretzo/shortest-path). Copies thousands of rows, so read once. `source` is the commit (`, part bundled` if a file was damaged), `bundled` or `none`; `version` bumps on new data. Each `Transport` has `kind`, `origin?`, `destination?`, `name`, `duration` (ticks), `wilderness?` (max level), `skills`, `quests`, `varbits`, `varps` (`{ id, op, value }`), `items`, `consumable`. Read via `Transports`.

Example:
```luau
print(#Host.transports().transports)
```

## Host.murmur2a

Definition:
```luau
Host.murmur2a: (text: string, seed: number?) -> number
```
MurmurHash2A of `text` (seed 0 by default) as 0 to 2^32 - 1. Used by `Biometrics.murmur2a`.

## Host.biometrics_profile

Definition:
```luau
Host.biometrics_profile: (name: string) -> { number }
```
Deterministic 24-byte profile (0 to 255 each) for an account name. Used by `Biometrics.setup`.

## Host.report_counters

Definition:
```luau
Host.report_counters: (counters: { Counter }) -> ()
```
Shows script counters in the loader's XP Tracker. Send running totals; each call replaces the last. A counter is `{ id, label, value, hourly?, per?, percent_of? }`: shown per hour unless `hourly = false`, `per` divides by another counter's id ("750 per Games"), `percent_of` shows a share ("75% of Games"). Values must be finite. Works in any run.

Example:
```luau
Host.report_counters({
	{ id = "games", label = "Games", value = 12 },
	{ id = "wins", label = "Wins", value = 9, percent_of = "games" },
})
```

## Host.report_warning

Definition:
```luau
Host.report_warning: (text: string) -> ()
```
Writes a yellow line to the loader log as given; `Log.warn` adds the `[tag][time]:` prefix.

Example:
```luau
Host.report_warning("[Bank][00:01:02:003]: the bank is nearly full")
```

## Host.report_breaks

Definition:
```luau
Host.report_breaks: (plan: BreakPlan) -> ()
```
Shows scheduled breaks and sleeps in the loader's Breaks tab, which counts down itself, so send only on change. `breaks` and `sleeps` hold `{ remaining, length, at?, index? }` (ms; `at` is a sleep's seconds since midnight; `index` is its place in the loader's config, 0-based); `active` is the one in progress. Sent by `Antiban`.

Example:
```luau
Host.report_breaks({ breaks = { { remaining = 600000, length = 120000 } }, sleeps = {} })
```

## Host.report_stats

Definition:
```luau
Host.report_stats: (experience: number, gold: number, custom: { number }?) -> ()
```
Reports gains since the last call (not totals) for the website stats; the loader sends them with its heartbeat. Experience ≥ 0, gold may be negative, `custom` follows the script's tracker order; rounded to integers. Reports exceeding the script's per-minute limits are refused; outside a store run they are dropped. Once a minute is enough; use `Utils.StatsReport`.

Example:
```luau
Host.report_stats(1250, 0)
```
