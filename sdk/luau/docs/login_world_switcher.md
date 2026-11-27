# LoginWorldSwitcher

The login screen's world list: reading worlds and switching the world you log in to. Everything returns `nil` or `false` off the login screen. Boxes are in client pixels, scaled with the login layout.

```luau
local LoginWorldSwitcher = require("@volcren/login_world_switcher")
```

## Constants

Definition:
```luau
LoginWorldSwitcher.MEMBERS = 0x1
LoginWorldSwitcher.PVP = 0x4
LoginWorldSwitcher.BOUNTY = 0x20
LoginWorldSwitcher.PVP_ARENA = 0x40
LoginWorldSwitcher.SKILL_TOTAL = 0x80
LoginWorldSwitcher.QUEST_SPEEDRUNNING = 0x100
LoginWorldSwitcher.HIGH_RISK = 0x400
LoginWorldSwitcher.HARDCORE_WORLD = 0x800
LoginWorldSwitcher.LAST_MAN_STANDING = 0x4000
LoginWorldSwitcher.BETA_WORLD = 0x10000
LoginWorldSwitcher.LEGACY_ONLY = 0x400000
LoginWorldSwitcher.EOC_ONLY = 0x800000
LoginWorldSwitcher.NOSAVE_MODE = 0x2000000
LoginWorldSwitcher.TOURNAMENT_WORLD = 0x4000000
LoginWorldSwitcher.FRESH_START_WORLD = 0x8000000
LoginWorldSwitcher.DEADMAN = 0x20000000
LoginWorldSwitcher.SEASONAL = 0x40000000
LoginWorldSwitcher.DANGEROUS: number
LoginWorldSwitcher.OFFLINE = -1
LoginWorldSwitcher.FULL = 1980
```
World type flags for `world.types`, tested with `has_type`. `DANGEROUS` combines every flag except `MEMBERS`, `SKILL_TOTAL`, `LEGACY_ONLY` and `EOC_ONLY`. `OFFLINE` is an offline world's player count and `FULL` the count above which a world is full.

## Settings

Definition:
```luau
LoginWorldSwitcher.allow_dangerous_worlds = false
```
When `false`, `switch` refuses worlds with a `DANGEROUS` flag.

Example:
```luau
LoginWorldSwitcher.allow_dangerous_worlds = true
```

## Types

Definition:
```luau
export type WorldBox = {
	index: number,
	slot: number,
	world: World,
	box: Box,
}
export type Buttons = {
	world: Box,
	previous: Box?,
	next: Box?,
}
```
`WorldBox.index` is the world's place in `worlds()` and `slot` its place on the page shown now. `Buttons` holds the login screen's "World N" button and the page arrows, set only while the list is open and that direction has a page.

The list shows `state().panel.shown`: the worlds that pass the switcher's filters (game modes, regions, members and free-to-play), in its sort, filled column by column. A world its filters hide can't be clicked.

## LoginWorldSwitcher.state

Definition:
```luau
function LoginWorldSwitcher.state(): WorldSelect?
```
The client's world select state (`Host.world_select`: open, current world, every world and, while open, the panel), or `nil` off the login screen.

Example:
```luau
local select = LoginWorldSwitcher.state()
print(select and select.open, select and select.current)
```

## LoginWorldSwitcher.is_open

Definition:
```luau
function LoginWorldSwitcher.is_open(): boolean
```
Whether the world list is open.

Example:
```luau
print("open:", LoginWorldSwitcher.is_open())
```

## LoginWorldSwitcher.wait_open

Definition:
```luau
function LoginWorldSwitcher.wait_open(timeout_ms: number?): boolean
```
Waits up to `timeout_ms` (default 5000) for the world list to open.

Example:
```luau
print(LoginWorldSwitcher.wait_open(3000))
```

## LoginWorldSwitcher.current

Definition:
```luau
function LoginWorldSwitcher.current(): number?
```
The world you'll log in to, shown on the "World N" button.

Example:
```luau
print(LoginWorldSwitcher.current())
```

## LoginWorldSwitcher.worlds

Definition:
```luau
function LoginWorldSwitcher.worlds(): { World }?
```
Every world in list order. Empty until the switcher has been opened once.

Example:
```luau
local worlds = LoginWorldSwitcher.worlds()
print(worlds and #worlds)
```

## LoginWorldSwitcher.world

Definition:
```luau
function LoginWorldSwitcher.world(id: number): World?
```
A world by number, or `nil` if it isn't listed.

Example:
```luau
local world = LoginWorldSwitcher.world(301)
if world then
	print(world.id, world.players, world.activity)
end
```

## LoginWorldSwitcher.has_type

Definition:
```luau
function LoginWorldSwitcher.has_type(world: World, types: number): boolean
```
Whether `world` has any of the flags in `types`.

Example:
```luau
local world = LoginWorldSwitcher.world(301)
if world then
	print("members:", LoginWorldSwitcher.has_type(world, LoginWorldSwitcher.MEMBERS))
end
```

## LoginWorldSwitcher.is_dangerous

Definition:
```luau
function LoginWorldSwitcher.is_dangerous(world: World): boolean
```
Whether `world` has a `DANGEROUS` flag.

Example:
```luau
local world = LoginWorldSwitcher.world(318)
if world then
	print("dangerous:", LoginWorldSwitcher.is_dangerous(world))
end
```

## LoginWorldSwitcher.is_available

Definition:
```luau
function LoginWorldSwitcher.is_available(world: World): boolean
```
Whether `world` is neither offline nor full.

Example:
```luau
local world = LoginWorldSwitcher.world(301)
if world then
	print("available:", LoginWorldSwitcher.is_available(world))
end
```

## LoginWorldSwitcher.buttons

Definition:
```luau
function LoginWorldSwitcher.buttons(): Buttons?
```
Where the switcher's buttons are.

Example:
```luau
local buttons = LoginWorldSwitcher.buttons()
print(buttons and buttons.next ~= nil)
```

## LoginWorldSwitcher.boxes

Definition:
```luau
function LoginWorldSwitcher.boxes(): { WorldBox }?
```
The rows shown on the current page; `nil` when the list is closed.

Example:
```luau
for _, b in LoginWorldSwitcher.boxes() or {} do
	print(b.slot, b.world.id)
end
```

## LoginWorldSwitcher.read

Definition:
```luau
function LoginWorldSwitcher.read(slot: number): number?
```
The world in row `slot` of the current page, 1 being the top-left, filled column by column.

Example:
```luau
print(LoginWorldSwitcher.read(1))
```

## LoginWorldSwitcher.open

Definition:
```luau
function LoginWorldSwitcher.open(): boolean
```
Clicks the "World N" button if the list is closed and waits up to 5 seconds for it to open.

Example:
```luau
print(LoginWorldSwitcher.open())
```

## LoginWorldSwitcher.close

Definition:
```luau
function LoginWorldSwitcher.close(): boolean
```
Presses Escape if the list is open (the switcher has no cancel button) and waits up to 1.5 seconds for it to close.

Example:
```luau
print(LoginWorldSwitcher.close())
```

## LoginWorldSwitcher.previous_page

Definition:
```luau
function LoginWorldSwitcher.previous_page(): boolean
```
Clicks the previous-page arrow, waits for the page to change, then pauses 0.4 to 1.2 seconds. `false` when closed or already on the first page.

Example:
```luau
print(LoginWorldSwitcher.previous_page())
```

## LoginWorldSwitcher.next_page

Definition:
```luau
function LoginWorldSwitcher.next_page(): boolean
```
Clicks the next-page arrow, waits for the page to change, then pauses 0.4 to 1.2 seconds. `false` when closed or already on the last page.

Example:
```luau
print(LoginWorldSwitcher.next_page())
```

## LoginWorldSwitcher.find

Definition:
```luau
function LoginWorldSwitcher.find(id: number): WorldBox?
```
Opens the list and turns pages until world `id` is shown, then returns its row. `nil` on failure or when the switcher's filters hide the world.

Example:
```luau
local found = LoginWorldSwitcher.find(301)
print(found and found.box.x1, found and found.box.y1)
```

## LoginWorldSwitcher.switch

Definition:
```luau
function LoginWorldSwitcher.switch(id: number): boolean
```
Switches to world `id` via `find`, clicking its box only once the client reports it hovered, and waits up to 1.5 seconds for the list to close on it. `true` at once when already on it; `false` for dangerous worlds (unless `allow_dangerous_worlds`), unavailable worlds or failures.

Example:
```luau
print(LoginWorldSwitcher.switch(301))
```

## LoginWorldSwitcher.switch_random

Definition:
```luau
function LoginWorldSwitcher.switch_random(ids: { number }): boolean
```
`true` if already on one of `ids`, otherwise switches to a random one, up to 5 attempts. `false` for an empty list.

Example:
```luau
print(LoginWorldSwitcher.switch_random({ 301, 302, 303 }))
```
