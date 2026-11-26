# Bankpouch

The rune pouch panel in the bank's side interface (group 15), shown in place of the inventory by "Configure" on a pouch: the pouch's runes, "Deposit runes", ten loadouts in a scrolling list, and "Dismiss". While it shows, runes withdrawn from the bank go straight into the pouch. Pouch contents are read from [Runepouch](runepouch.md) varbits; every function here needs the bank open and, except `open`, the panel too.

```luau
local BankPouch = require("@volcren/bankpouch")
```

## Constants

Definition:
```luau
BankPouch.GROUP = 15
BankPouch.PANEL = 19
BankPouch.DEPOSIT_RUNES = 20
BankPouch.DISMISS = 22
BankPouch.RUNES = 26
BankPouch.LIST = 32
BankPouch.LOADOUTS = { 33, 37, 40, 43, 45, 47, 49, 51, 53, 55 }
BankPouch.NAMES = { 34, 38, 41, 44, 46, 48, 50, 52, 54, 56 }
BankPouch.LAST_CHILD = 120
BankPouch.SLOTS = 4
BankPouch.EMPTY_SLOT = 11526
BankPouch.UNNAMED = "-"
BankPouch.OPTION = "Configure"
BankPouch.DISMISS_OPTION = "Dismiss"
BankPouch.DEPOSIT_RUNES_OPTION = "Deposit runes"
BankPouch.LOAD_OPTION = "Load"
BankPouch.AMOUNT_QUERY = "Enter amount"
BankPouch.MAX_SCROLLS = 12
BankPouch.TIMEOUT_MS = 2500
BankPouch.POLL_MS = 50
BankPouch.SETTLE_TICKS = 1
```
Panel components: `RUNES` holds the pouch cells as children 1 to `SLOTS`; each `LOADOUTS` row holds its rune cells the same way (`EMPTY_SLOT` where unset), with its name as child 1 of the matching `NAMES` entry (`UNNAMED` until named) and a Load button parented to the row somewhere up to `LAST_CHILD`. Then menu options, the typed-amount query, the scroll limit and timings; `SETTLE_TICKS` is how many game ticks `open` waits for after the panel shows. `LOADOUTS` and `NAMES` are frozen.

## Types

Definition:
```luau
export type Loadout = { number: number, name: string?, runes: { number } }
```
A loadout's 1-based list position, its name (nil while unnamed), and its rune item ids in slot order without empty slots.

## BankPouch.is_open

Definition:
```luau
function BankPouch.is_open(): boolean
```
Whether the bank is open with the panel showing.

## BankPouch.open

Definition:
```luau
function BankPouch.open(timeout_ms: number?): boolean
```
Uses "Configure" on the first pouch in the bank's inventory, waits up to `timeout_ms` (default `TIMEOUT_MS`) for the panel, then waits until `SETTLE_TICKS` game ticks have passed (`Ticks.wait`), since the game ignores a deposit clicked in the tick the panel appears. True at once if already open; false when the bank isn't open or no pouch is held.

Example:
```luau
if BankPouch.open() then
	BankPouch.close()
end
```

## BankPouch.close

Definition:
```luau
function BankPouch.close(timeout_ms: number?): boolean
```
Clicks Dismiss and waits for the inventory to return. True at once if the panel isn't open.

Example:
```luau
BankPouch.close()
```

## BankPouch.deposit_runes

Definition:
```luau
function BankPouch.deposit_runes(): boolean
```
Empties the pouch into the bank with the panel's button. True once the pouch is empty, at once if it already was.

Example:
```luau
BankPouch.deposit_runes()
```

## BankPouch.deposit

Definition:
```luau
function BankPouch.deposit(rune: Item, amount: number?): boolean
```
Deposits `amount` of a pouch rune from its cell: "Deposit-1/5/10" or the bank's custom amount when it matches, "Deposit-X" with a typed amount otherwise, and "Deposit-All" when `amount` is nil, 0 or at least what's held. True once the pouch holds less; false when it holds none.

Example:
```luau
BankPouch.deposit("Law rune", 100)
```

## BankPouch.withdraw

Definition:
```luau
function BankPouch.withdraw(rune: Item, amount: number?): boolean
```
Withdraws `rune` from the bank into the pouch via `Bankmain.withdraw` (nil is 1, 0 or less is all). True once the pouch holds more of it; logs and returns false when the bank or pouch refuses.

Example:
```luau
if BankPouch.open() then
	BankPouch.withdraw("Law rune", 500)
	BankPouch.close()
end
```

## BankPouch.loadouts

Definition:
```luau
function BankPouch.loadouts(): { Loadout }?
```
All ten loadouts, read from memory whether or not their row is scrolled into view. Nil while the panel isn't open.

## BankPouch.load

Definition:
```luau
function BankPouch.load(which: number | string): boolean
```
Loads a loadout by number or case-insensitive name: scrolls its row into view, clicks Load, and waits until the pouch holds exactly its runes. False for a missing or empty loadout, or when the row can't be scrolled into view.

Example:
```luau
if BankPouch.open() then
	BankPouch.load(1)
	BankPouch.close()
end
```
