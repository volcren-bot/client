# Bankmain

The bank. It has every function of [Inv](inv.md) already set up for the bank's contents, and enters the account's bank PIN if the keypad is in the way.

```luau
local Bankmain = require("@volcren/bankmain")
```

## Constants

Definition:

```luau
Bankmain.CONTAINER = 95
Bankmain.GROUP = 12
```

The bank's container id and its interface group.

## Functions from Inv

`slots`, `items`, `indices_of`, `index_of`, `find`, `contains`, `contains_any`, `contains_all`, `count`, `count_ex` and `wait_count` work as described in [Inv](inv.md), on the bank's contents. `Bankmain.inventory` is the same for the inventory the bank shows beside it (container 93 in the bank's side interface), the one deposits click; the rune pouch's panel that takes its place is [BankPouch](bankpouch.md). A placeholder's container slot reads quantity 1 (its own id, named like the item, e.g. 31108 for Confliction gauntlets 31106), so while the bank is open its quantity is taken from its grid cell, which shows 0 (`Widget.item_quantity`): placeholders count as none and aren't withdrawn. With the bank closed only the container is read, and they count as 1.

## Bankmain.is_open

Definition:

```luau
function Bankmain.is_open(): boolean
```

Whether the bank window is open. If the keypad is showing, the injector enters the account's PIN first (see [BankpinKeypad.enter](bankpin_keypad.md)); `false` if that fails. An account with no saved PIN leaves the keypad alone.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

if Bankmain.is_open() then
	print("coins in the bank:", Bankmain.count_ex("Coins"))
end
```

## Bankmain.close_button

Definition:

```luau
function Bankmain.close_button(): Widget?
```

The bank's close button, or `nil` when the bank is closed or it isn't where the frame keeps it.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

print(Bankmain.close_button())
```

## Bankmain.wait_open

Definition:

```luau
function Bankmain.wait_open(timeout_ms: number?): boolean
```

Polls `is_open` every 50 ms for up to `timeout_ms` (3 seconds by default). A PIN keypad in the way is handled by `is_open`.

## Bankmain.nearest

Definition:

```luau
export type Source = { loc: Loc?, npc: Npc? }
Bankmain.PLAN_CANDIDATES = 6
function Bankmain.nearest(): Source?
```

The bank that is the fewest steps to walk to, as a `Source` holding either a `loc` or an `npc`. It looks at every loaded object whose menu, as shown now (`Host.loc_shown`), offers "Bank", or "Use" together with "Collect" (bank chests), and every NPC that `is_banker`, plans a walk (`Walker.plan`, through doors, stairs and ladders) to the `PLAN_CANDIDATES` closest by straight distance, and takes the shortest plan. An object is planned to within `Clickbox.APPROACH_NEAR` tiles, and a plan that stops short counts the straight distance left. A banker is planned to its nearest `counter_tiles` tile and counts only when the plan gets there, so a banker behind a wall isn't taken for near. Objects are told apart by their options rather than their names because multi-state booths have no name in the cache (Varrock West's first booth, 34810) and banks come as booths, chests, tables and counters; a bare "Use" is too common to count on its own. This costs one plan per candidate. When none can be planned it returns the closest by straight distance. `nil` when none is loaded or the position is unknown.

## Bankmain.is_banker

Definition:

```luau
function Bankmain.is_banker(npc: Npc): boolean
```

Whether the NPC's menu offers "Bank" ([Npcs.options](npcs.md#npcsoptions)), so named bankers such as Emerald Benedict count, not only those called "Banker".

Example:

```luau
local Npcs = require("@volcren/npcs")

for _, npc in ipairs(Npcs.find() or {}) do
	if Bankmain.is_banker(npc) then
		print(npc.name)
	end
end
```

## Bankmain.counter_tiles

Definition:

```luau
function Bankmain.counter_tiles(npc: Npc): { Tile }
```

The tiles the banker can be used from, read off the collision map (`Host.collision`) and the loaded banks: each tile beside it with no wall between, and each tile past a bank object beside it (the counter), when neither side of that object has a wall. The tiles aren't checked for a walk from here: tiles on the banker's own side of the counter are usually closed off. Empty when the position or the collision map is unknown.

Example:

```luau
local Npcs = require("@volcren/npcs")

for _, npc in ipairs(Npcs.find() or {}) do
	if Bankmain.is_banker(npc) then
		for _, tile in ipairs(Bankmain.counter_tiles(npc)) do
			print(npc.name, tile.x, tile.y)
		end
	end
end
```

## Bankmain.can_click_banker

Definition:

```luau
function Bankmain.can_click_banker(npc: Npc): boolean
```

Whether clicking the banker from here would get it used. Works out where the game's own pathing would take the player on the collision map: the nearest tile beside the banker with no wall between, or, when none can be reached, the reachable tile closest to it within 10 tiles (fewest steps on a tie). `true` when that tile is beside the banker or one of its `counter_tiles`. `false` when the game would stop the player short, such as outside the wall behind the banker, where clicking only gets "I can't reach that". Plans over the collision map from the player and the banker out to 16 tiles, once per call.

Example:

```luau
local Npcs = require("@volcren/npcs")

local banker = Npcs.nearest("Banker", Walker.position())
if banker and Bankmain.can_click_banker(banker) then
	print("the click lands at the counter")
end
```

## Bankmain.hover

Definition:

```luau
function Bankmain.hover(source: Source?, walk: boolean?): boolean
```

Hovers the closest bank (or `source`). Walks closer first unless `walk` is `false`.

## Bankmain.open

Definition:

```luau
function Bankmain.open(source: Source?, walk: boolean?): boolean
```

Opens the closest bank (or `source`) with "Bank" (or "Use" on objects) and waits for it. Walks closer first unless `walk` is `false`. A banker is clicked at once when `can_click_banker`; otherwise the walk heads for its nearest reachable `counter_tiles` tile, checks again on each new tile and clicks as soon as the click would land, so a banker behind a wall is walked round to rather than clicked from outside. `false` when no counter tile can be walked to. Another interface in the way is closed with Escape first. If the keypad shows up, `is_open` has the injector enter the PIN. `true` when the bank ends up open.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

if Bankmain.open() then
	print("coins in the bank:", Bankmain.count_ex("Coins"))
end
```

## Buttons

```luau
export type Button = "ARRANGEMENT" | "NOTES" | "PLACEHOLDERS" | "SEARCH" | "DEPOSIT_CONTAINERS" | "DEPOSIT_INVENTORY" | "DEPOSIT_WORN" | "POTION_STORAGE" | "TUTORIAL"
export type Quantity = "ONE" | "FIVE" | "TEN" | "CUSTOM" | "ALL"
export type Arrangement = "SWAP" | "INSERT"
```

### Bankmain.button / Bankmain.click_button

Definition:

```luau
function Bankmain.button(button: Button): Widget?
function Bankmain.click_button(button: Button): boolean
```

The widget of a button, `nil` when the bank is closed or the button isn't shown (the deposit containers and potion storage buttons depend on the account). `click_button` clicks it; `false` when the bank is closed or the button isn't shown.

### Settings: arrangement, notes, placeholders

Definition:

```luau
function Bankmain.arrangement(): Arrangement?
function Bankmain.set_arrangement(mode: Arrangement, timeout_ms: number?): boolean
function Bankmain.notes(): boolean?
function Bankmain.set_notes(on: boolean, timeout_ms: number?): boolean
function Bankmain.placeholders(): boolean?
function Bankmain.set_placeholders(on: boolean, timeout_ms: number?): boolean
```

Read the settings the buttons toggle from the client (not from how the buttons look: the arrangement button looks the same in both states). `set_*` clicks the button only when the setting differs and waits for it to change (2 seconds by default); `true` when it ends up as asked. `notes` is whether items are withdrawn as notes; `placeholders` is "Always set placeholders".

### Quantity

Definition:

```luau
function Bankmain.quantity(): Quantity?
function Bankmain.custom_quantity(): number?
function Bankmain.quantity_of(amount: number): Quantity
function Bankmain.select_quantity(quantity: Quantity, timeout_ms: number?): boolean
function Bankmain.set_custom_quantity(amount: number): boolean
function Bankmain.set_quantity(amount: number): boolean
```

`quantity` is the selected default quantity and `custom_quantity` the amount of the X button. `quantity_of` is the button that gives an amount: 1, 5 and 10 have their own, 0 or less is `"ALL"` and anything else is `"CUSTOM"`. `select_quantity` clicks a quantity button and waits for it to be the selected one. `set_custom_quantity` right clicks X, picks "Set custom quantity" and types the amount (it selects X too). `set_quantity` does what is needed for an amount: select 1, 5, 10 or all, or set X first.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

Bankmain.set_notes(false)
Bankmain.set_quantity(10)
```

### Deposit buttons

Definition:

```luau
function Bankmain.deposit_inventory(): boolean
function Bankmain.deposit_worn(): boolean
function Bankmain.deposit_containers(): boolean
```

Click "Deposit inventory", "Deposit worn items" and "Empty containers" (the loot bag, herb sack and the like; only shown when the account has something to empty). They return whether the click was made; use the inventory or equipment to see that it worked.

### Search

Definition:

```luau
function Bankmain.is_search_open(): boolean
function Bankmain.open_search(timeout_ms: number?): boolean
function Bankmain.search(text: string): boolean
```

`open_search` clicks the search button and waits for the prompt in the chatbox. `search` opens it, types `text` and presses Enter. Pressing Escape while the prompt is open closes the whole bank, there is no function that only closes the prompt. Clicking the search button while a search is applied clears it (`click_button("SEARCH")`); there is no function that tells whether one is applied, so there is no `clear_search` yet.

### Potion storage

Definition:

```luau
function Bankmain.has_potion_storage(): boolean
function Bankmain.potion_storage_is_open(): boolean
function Bankmain.open_potion_storage(timeout_ms: number?): boolean
function Bankmain.close_potion_storage(timeout_ms: number?): boolean
function Bankmain.toggle_potion_storage(timeout_ms: number?): boolean
```

The potion storage is a view in the bank window itself (its title changes to "Potion storage"), and the same button opens it and goes back. `open_` and `close_` do nothing if it is already in that state.

### Tabs: Bankmain.tab_sizes, count_tabs, tab_of_slot, find_tab

Definition:

```luau
function Bankmain.tab_sizes(): { number }?
function Bankmain.count_tabs(): number?
function Bankmain.tab_of_slot(slot: number): number?
function Bankmain.find_tab(item: Item | { Item }): (number?, number?)
```

The container keeps the items of each tab together: tab 1 first, then tabs 2 to 9, then the items that aren't in a tab (the main tab, 0). `tab_sizes` reads how many items each tab holds (the client keeps those numbers), `count_tabs` is how many tabs have items, and `tab_of_slot` adds the sizes up to say which tab a slot is in (0 when it isn't in one). `find_tab` gives the tab and the slot of an item, read from the bank's contents: no searching, no opening tabs. `nil` when the item isn't in the bank. An item in several slots gives the first slot's tab; use `indices_of` and `tab_of_slot` to see all of them. `open_tab` opens a tab.

### Slots: shown, scroll and show

Definition:

```luau
function Bankmain.is_slot_shown(slot: number): boolean
function Bankmain.scroll_distance(slot: number): number?
function Bankmain.scroll_to(slot: number, timeout_ms: number?): boolean
function Bankmain.show_slot(slot: number): boolean
```

The grid holds one widget per slot, and only the open tab's are shown; their positions are on screen, so how far a slot is from the visible part is read, not guessed. `is_slot_shown` says whether at least half of the slot is inside the grid. `scroll_distance` is how many mouse wheel notches (about 79 units each) it takes to bring the slot fully into the grid: positive is down, negative up, `0` when it is there, `nil` when the slot isn't in the open tab. `scroll_to` turns the wheel until it is: a long way is first scrolled to a random place in the grid, in bursts of 2 to 6 notches with short pauses, and what is left is fixed up. `show_slot` opens the slot's own tab (so there is the least scrolling) and then scrolls to it.

`hover_slot`, `click_slot`, `interact_slot`, `box`, `boxes` and `slot_at` work on the slots that are shown, as described in [Inv](inv.md). `interact_slot(slot, "Withdraw-5")` uses the item's menu.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

local tab, slot = Bankmain.find_tab("Willow logs")
if slot and Bankmain.show_slot(slot) then
	Bankmain.click_slot(slot)
end
```

### Bankmain.withdraw

Definition:

```luau
Bankmain.ALL_BUT_ONE = -1
Bankmain.CUSTOM_BUTTON_CHANCE = 0.005
function Bankmain.withdraw(item: Item | { Item }, amount: number?, noted: boolean?, use_quantity_buttons: boolean?): boolean
```

Takes an item out of the bank. `amount` is how many (1 by default; 0 or less is all; `Bankmain.ALL_BUT_ONE` is all but one); asking for the whole stack or more takes all of it. `noted` withdraws as notes (`true`) or items (`false`); `nil` leaves the setting as it is. Placeholders are skipped. By default it selects the quantity button that gives the amount, shows the item and clicks it. For an amount X doesn't hold yet it left-clicks the X button, then takes `Withdraw-X` from the item's menu and types the amount, which also makes it X's amount (when the X button itself asks for an amount, it answers that and clicks the item instead); only with the rare biometric chance `CUSTOM_BUTTON_CHANCE` does it set X from the button's own menu (`set_quantity`) and click the item; with `use_quantity_buttons` false it takes the option from the item's menu instead (`Withdraw-1/5/10/All/All-but-1`, the amount X holds, or `Withdraw-X` and types the amount). Returns `true` once the bank holds less of the item, `false` if the bank is closed, has none of it, or nothing was taken.

It leaves the quantity, X and notes settings as it set them. Save them first (`quantity`, `custom_quantity`, `notes`) if the script needs them back.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

Bankmain.open()
Bankmain.withdraw("Willow logs", 10, false)
Bankmain.withdraw("Charcoal", 0, true, false) -- all of it, as notes, through the menu
```

Not covered: the incinerator button and the account advert (neither is shown in the bank this was written against), and the settings button (the client's top bar has no such button).

### Bankmain.selected_amount / Bankmain.withdraw_selected

Definition:

```luau
function Bankmain.selected_amount(): number?
function Bankmain.withdraw_selected(item: Item | { Item }, wait: boolean?): boolean
```

`selected_amount` is how many a left click on an item withdraws with the quantity button selected now: 1, 5, 10, the amount X holds, or `math.huge` for All (nil when it can't be read). `withdraw_selected` shows the item and left-clicks it with whatever is selected, without touching the buttons or the menu; `true` once the bank holds less of it, or right after the click with `wait` false, for a caller that clicks several things in a row and checks the inventory once at the end. [InventoryLayout.withdraw](inventory_layout.md#inventorylayoutwithdraw) uses it for a last item that fills the inventory.

## Bankmain.close

Definition:

```luau
function Bankmain.close(key_probability: number?, timeout_ms: number?): boolean
```

Closes the bank, like `DepositBox.close`: Escape with the chance `key_probability` (by default the `Biometrics.ESCAPE` probability), then the close button, up to 3 times. See `Toplevel.close`.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

Bankmain.close()
```

## Bankmain.tab

Definition:

```luau
function Bankmain.tab(): number?
```

The open bank tab: 0 for the main tab, which shows every item, 1 to 9 for the others. `nil` when it can't be read.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

print("tab:", Bankmain.tab())
```

## Bankmain.tab_button

Definition:

```luau
function Bankmain.tab_button(tab: number): Widget?
```

The button of a bank tab in the tab bar, or `nil` when the bank is closed.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

print(Bankmain.tab_button(0))
```

## Bankmain.open_tab

Definition:

```luau
function Bankmain.open_tab(tab: number, timeout_ms: number?): boolean
```

Opens a bank tab and waits for it to be the open one (2 seconds by default). The item grid only scrolls when the open tab has more than a screen of items, and the grid shows the tab's own items, so open the main tab (0) before looking for an item by its slot in the bank. Returns `true` when the tab is open in the end.

Example:

```luau
local Bankmain = require("@volcren/bankmain")

assert(Bankmain.open_tab(0), "couldn't open the main tab")
print(Bankmain.index_of("Anglerfish"))
```

## Bankmain.button

Definition:

```luau
function Bankmain.button(button: Button): Widget?
```

TODO...

Example:

```luau

```

## Bankmain.click_button

Definition:

```luau
function Bankmain.click_button(button: Button): boolean
```

TODO...

Example:

```luau

```

## Bankmain.arrangement

Definition:

```luau
function Bankmain.arrangement(): Arrangement?
```

TODO...

Example:

```luau

```

## Bankmain.set_arrangement

Definition:

```luau
function Bankmain.set_arrangement(mode: Arrangement, timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.notes

Definition:

```luau
function Bankmain.notes(): boolean?
```

TODO...

Example:

```luau

```

## Bankmain.set_notes

Definition:

```luau
function Bankmain.set_notes(on: boolean, timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.placeholders

Definition:

```luau
function Bankmain.placeholders(): boolean?
```

TODO...

Example:

```luau

```

## Bankmain.set_placeholders

Definition:

```luau
function Bankmain.set_placeholders(on: boolean, timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.quantity

Definition:

```luau
function Bankmain.quantity(): Quantity?
```

TODO...

Example:

```luau

```

## Bankmain.custom_quantity

Definition:

```luau
function Bankmain.custom_quantity(): number?
```

TODO...

Example:

```luau

```

## Bankmain.quantity_of

Definition:

```luau
function Bankmain.quantity_of(amount: number): Quantity
```

TODO...

Example:

```luau

```

## Bankmain.select_quantity

Definition:

```luau
function Bankmain.select_quantity(quantity: Quantity, timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.set_custom_quantity

Definition:

```luau
function Bankmain.set_custom_quantity(amount: number): boolean
```

TODO...

Example:

```luau

```

## Bankmain.set_quantity

Definition:

```luau
function Bankmain.set_quantity(amount: number): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit_inventory

Definition:

```luau
function Bankmain.deposit_inventory(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit_worn

Definition:

```luau
function Bankmain.deposit_worn(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit_containers

Definition:

```luau
function Bankmain.deposit_containers(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.is_search_open

Definition:

```luau
function Bankmain.is_search_open(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.open_search

Definition:

```luau
function Bankmain.open_search(timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.search

Definition:

```luau
function Bankmain.search(text: string): boolean
```

TODO...

Example:

```luau

```

## Bankmain.has_potion_storage

Definition:

```luau
function Bankmain.has_potion_storage(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.potion_storage_is_open

Definition:

```luau
function Bankmain.potion_storage_is_open(): boolean
```

TODO...

Example:

```luau

```

## Bankmain.toggle_potion_storage

Definition:

```luau
function Bankmain.toggle_potion_storage(timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.open_potion_storage

Definition:

```luau
function Bankmain.open_potion_storage(timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.close_potion_storage

Definition:

```luau
function Bankmain.close_potion_storage(timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.tab_sizes

Definition:

```luau
function Bankmain.tab_sizes(): { number }?
```

TODO...

Example:

```luau

```

## Bankmain.count_tabs

Definition:

```luau
function Bankmain.count_tabs(): number?
```

TODO...

Example:

```luau

```

## Bankmain.tab_of_slot

Definition:

```luau
function Bankmain.tab_of_slot(slot: number): number?
```

TODO...

Example:

```luau

```

## Bankmain.find_tab

Definition:

```luau
function Bankmain.find_tab(item: Item | { Item }): (number?, number?)
```

TODO...

Example:

```luau

```

## Bankmain.is_slot_shown

Definition:

```luau
function Bankmain.is_slot_shown(slot: number): boolean
```

TODO...

Example:

```luau

```

## Bankmain.scroll_distance

Definition:

```luau
function Bankmain.scroll_distance(slot: number): number?
```

TODO...

Example:

```luau

```

## Bankmain.scroll_to

Definition:

```luau
function Bankmain.scroll_to(slot: number, timeout_ms: number?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.show_slot

Definition:

```luau
function Bankmain.show_slot(slot: number): boolean
```

TODO...

Example:

```luau

```

## Bankmain.withdraw

Definition:

```luau
function Bankmain.withdraw(
	item: Item | { Item },
	amount: number?,
	noted: boolean?,
	use_quantity_buttons: boolean?
): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit

Definition:

```luau
function Bankmain.deposit(item: Item | { Item }, amount: number?, use_quantity_buttons: boolean?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit_array

Definition:

```luau
function Bankmain.deposit_array(entries: { Entry }, use_quantity_buttons: boolean?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.deposit_different

Definition:

```luau
function Bankmain.deposit_different(keep: Item | { Item }, use_quantity_buttons: boolean?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.selected_amount

Definition:

```luau
function Bankmain.selected_amount(): number?
```

TODO...

Example:

```luau

```

## Bankmain.withdraw_selected

Definition:

```luau
function Bankmain.withdraw_selected(item: Item | { Item }, wait: boolean?): boolean
```

TODO...

Example:

```luau

```

## Bankmain.lock_slot

Definition:
```luau
function Bankmain.lock_slot(slot: number, locked: boolean): boolean
```
Locks or unlocks an inventory slot from the bank's side panel (`Lock-slot` / `Unlock-slot`) and waits for [Inventory.locked_slots](inventory.md#inventorylocked_slots) to show it; `true` at once when it already is. "Deposit inventory" then leaves the slot alone.

Example:
```luau
local Inventory = require("@volcren/inventory")

local pickaxe = Inventory.index_of("Rune pickaxe")
if pickaxe and Bankmain.open() then
	Bankmain.lock_slot(pickaxe, true)
end
```
