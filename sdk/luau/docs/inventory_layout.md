# InventoryLayout

Inventory layouts from a form's `inventory` field: checked against the inventory and filled from the bank, rune pouch included.

```luau
local InventoryLayout = require("@volcren/inventory_layout")
```

The user builds layouts per script on the loader's Inventory tab, read from the client with the rune pouch's runes; the form's `inventory` field picks one (see `Host.settings` in [Host](host.md)). A layout is positional, but the bank cares about amounts: a stack's range, or the number of copies of a non-stacking item. An item is its `ids`, or for dose/charge items its accepted `doses` (see [LayoutItem](layout_item.md)), so four `Prayer potion(4)` slots accepting doses 4 and 3 want four potions of either dose.

A field's `required` items appear padlocked in every layout (`locked = true`); the user can move but not change them. A layout without room for them can't be picked.

```luau
local Host = require("@volcren/host")
local Bankmain = require("@volcren/bankmain")
local InventoryLayout = require("@volcren/inventory_layout")

-- manifest.json: { "settings": { "fields": [{ "id": "inventory", "kind": "inventory", "label": "Inventory", "required": [{ "ids": [227], "slot": 0 }] }] } }
local layout = Host.settings().inventory
if layout and Bankmain.open() and InventoryLayout.withdraw(layout) then
	Bankmain.close()
	InventoryLayout.reorder(layout)
end
```

## Constants

Definition:
```luau
InventoryLayout.SIZE = 28
InventoryLayout.POUCH_TIMEOUT_MS = 2500
InventoryLayout.POUCH_LOAD_BASE = 0.7
InventoryLayout.POUCH_SLOTS = 3
InventoryLayout.DIVINE_POUCH_SLOTS = 4
InventoryLayout.DIVINE_POUCHES = { "Divine rune pouch", "Divine rune pouch (l)" }
InventoryLayout.POLL_MS = 50
```
Inventory size; how long to wait for withdrawn runes to reach the pouch ([Runepouch](runepouch.md)); the chance base for loading a saved pouch loadout over filling the pouch by hand (see `withdraw`); how many rune types a rune pouch and a divine rune pouch hold, and the divine pouches' names (`DIVINE_POUCHES` is frozen); poll interval.

## Types

Definition:
```luau
export type Layout = Host.InventoryLayout
export type Slot = Host.InventoryLayoutSlot
export type Rune = Host.LayoutItem
export type Entry = {
	items: { Item.Item },
	stackable: boolean,
	min: number,
	max: number,
	copies: number,
	label: string,
}
```
A layout is `{ name, slots, pouch? }`: filled slots (`LayoutItem`s with `slot`) and the pouch's runes when it holds a pouch. An `Entry` is one item as the bank sees it: accepted items in withdraw order, its stack range, and how many slots want it.

## InventoryLayout.from_client

Definition:
```luau
function InventoryLayout.from_client(name: string?): Layout?
```
A layout of the current inventory and pouch runes. Nil when the inventory can't be read. `stackable` is only known for stacks over one, which want exactly what they hold.

Example:
```luau
local layout = InventoryLayout.from_client("current")
```

## InventoryLayout.entries

Definition:
```luau
function InventoryLayout.entries(layout: Layout): { Entry }
```
The layout grouped by item: slots wanting the same thing are one entry, `copies` the slot count.

Example:
```luau
for _, e in ipairs(InventoryLayout.entries(layout)) do
	print(e.label, e.copies)
end
```

## InventoryLayout.matches

Definition:
```luau
function InventoryLayout.matches(layout: Layout): boolean?
```
Whether the inventory holds the layout, ignoring slots: each stack in range (both ends 0 means any nonzero amount), enough copies of everything else counting all accepted doses, and pouch runes in range (over `max` is fine, since the pouch can't return part of a stack). Nil when the inventory can't be read.

Example:
```luau
if InventoryLayout.matches(layout) then
	Bankmain.close()
end
```

## InventoryLayout.deposit_diff

Definition:
```luau
function InventoryLayout.deposit_diff(layout: Layout): boolean
```
With the bank open, deposits everything not in the layout (unaccepted doses included) and any excess: stacks over `max` and copies past the count, last-accepted doses first. True when all of it went in.

Example:
```luau
InventoryLayout.deposit_diff(layout)
```

## InventoryLayout.withdraw

Definition:
```luau
function InventoryLayout.withdraw(layout: Layout): boolean
```
With the bank open, makes the inventory match: `deposit_diff`, fill the pouch, then withdraw what is missing (a stack under `min` up to `max`, all of it when both are 0 and none is held; missing copies in accepted-dose order). True when the layout matches at the end.

Everything unlisted is deposited, so wear an equipment layout first (`EquipmentLayout.prepare`). The pouch is withdrawn if not carried and its bank panel opened (`BankPouch.open`); an already-open panel is dismissed first. When a saved loadout (`BankPouch.loadouts`) holds exactly the layout's rune types, it is loaded (`BankPouch.load`) with probability `POUCH_LOAD_BASE` plus the rest weighted by the account's `Biometrics.CHOOSEOPTION`. Otherwise, or when the load fails, the pouch is filled by hand: runes the layout doesn't list are banked (`BankPouch.deposit`, or "Deposit runes" when that's all of them), only as many as the layout's new rune types need slots for, so they stay while there's room. Then whatever is still short is withdrawn into the pouch (`BankPouch.withdraw`), which tops up a loadout that brought less than `min`, and the panel is dismissed. Amounts the 1/5/10 buttons can't give that several items share are taken first so the X button is set once; a one-off amount uses `Withdraw-X` from the menu (see `Bankmain.withdraw`). A last non-stacking item that fills the free slots is left-clicked when the selected quantity covers it (`Bankmain.withdraw_selected`).

Example:
```luau
if Bankmain.open() and not InventoryLayout.withdraw(layout) then
	print("bank is missing items")
end
```

## InventoryLayout.reorder

Definition:
```luau
function InventoryLayout.reorder(layout: Layout): boolean
```
Drags items into the layout's slots from the first slot on, with the inventory tab open; a slot already holding an accepted item is left alone. True when every item sits in its slot; false when a wanted item isn't in the inventory outside the slots already holding it.

Example:
```luau
InventoryLayout.reorder(layout)
```
