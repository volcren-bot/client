# Grandexchange

The Grand Exchange (group 465): the overview of 8 offer slots and the offer setup screen, with the item search in the chatbox (group 162). Everything is read from the interface, so it needs the overview or the setup screen open. Only buying is supported.

An offer's `state` comes from the slot's title and the colour of its progress bar: green is complete (`bought`, `sold`), red is `aborted`, anything else is still `buying` or `selling`. `progress` is how far the bar is filled, 0 to 1. A slot whose Buy button is hidden (a members slot on a free account) is never free.

A buy places an offer and doesn't wait for it: `wait_done` waits for the slot to complete, and `collect` takes what is waiting into the inventory or bank. Prices move only by the setup screen's 5% buttons, from the guide price the setup screen starts at.

**Placing an offer spends coins.** `buy` refuses when the offer's total is more than the coins in the inventory.

```luau
local GrandExchange = require("@volcren/grandexchange")
```

## Constants

Definition:
```luau
GrandExchange.GROUP = 465
GrandExchange.SLOTS = 8
GrandExchange.CLERK = "Grand Exchange Clerk"
GrandExchange.OPTION = "Exchange"
```
The interface group, its number of offer slots, and the NPC and option that open it.

## GrandExchange.is_open

Definition:
```luau
function GrandExchange.is_open(): boolean
```
Whether the Grand Exchange is open, on either screen.

Example:
```luau
print(GrandExchange.is_open())
```

## GrandExchange.wait_open

Definition:
```luau
function GrandExchange.wait_open(timeout_ms: number?): boolean
```
Waits up to `timeout_ms` (default 4 seconds) for it to open.

Example:
```luau
print(GrandExchange.wait_open(2000))
```

## GrandExchange.close_button

Definition:
```luau
function GrandExchange.close_button(): Widget?
```
The frame's close button, nil when it isn't shown.

Example:
```luau
print(GrandExchange.close_button() ~= nil)
```

## GrandExchange.close

Definition:
```luau
function GrandExchange.close(key_probability: number?, timeout_ms: number?): boolean
```
Closes it with Escape or the close button, as [Toplevel.close](toplevel.md).

Example:
```luau
GrandExchange.close()
```

## GrandExchange.open

Definition:
```luau
function GrandExchange.open(): (boolean, string?)
```
Picks "Exchange" on the nearest clerk and waits for the interface. `true` at once when it is already open; `false` and a reason otherwise.

Example:
```luau
local ok, why = GrandExchange.open()
print(ok, why)
```

## GrandExchange.is_overview

Definition:
```luau
function GrandExchange.is_overview(): boolean
```
Whether the offer slots are shown.

Example:
```luau
print(GrandExchange.is_overview())
```

## GrandExchange.is_setup

Definition:
```luau
function GrandExchange.is_setup(): boolean
```
Whether the offer setup screen is shown.

Example:
```luau
print(GrandExchange.is_setup())
```

## GrandExchange.offer

Definition:
```luau
function GrandExchange.offer(slot: number): Offer?
```
Slot `slot` (0 to 7): `{ slot, state, id, name, quantity, price, progress }`. `id`, `name` and `price` are nil for an empty slot; `quantity` is what the offer is for. Nil off the overview.

Example:
```luau
local offer = GrandExchange.offer(0)
if offer then
	print(offer.state, offer.name, offer.progress)
end
```

## GrandExchange.offers

Definition:
```luau
function GrandExchange.offers(): { Offer }?
```
Every slot, in order. Nil off the overview.

Example:
```luau
for _, offer in GrandExchange.offers() or {} do
	print(offer.slot, offer.state)
end
```

## GrandExchange.free_slot

Definition:
```luau
function GrandExchange.free_slot(): number?
```
The first empty slot that can take a buy offer.

Example:
```luau
print(GrandExchange.free_slot())
```

## GrandExchange.can_collect

Definition:
```luau
function GrandExchange.can_collect(): boolean
```
Whether the overview shows the Collect button, so bought items or coins are waiting.

Example:
```luau
print(GrandExchange.can_collect())
```

## GrandExchange.collect

Definition:
```luau
function GrandExchange.collect(to_bank: boolean?): boolean
```
Collects everything waiting into the inventory, or the bank with `to_bank`, and waits for the button to go. `false` when there is nothing to collect.

Example:
```luau
GrandExchange.collect()
```

## GrandExchange.start_buy

Definition:
```luau
function GrandExchange.start_buy(slot: number?): boolean
```
Clicks Buy on `slot` (default `free_slot()`) and waits for the setup screen and its search prompt.

Example:
```luau
print(GrandExchange.start_buy())
```

## GrandExchange.choose

Definition:
```luau
function GrandExchange.choose(name: string): boolean
```
Types `name` into the search prompt and selects the result with exactly that name (ignoring case), then waits for the setup screen to show it with its quantity and price, which fill in a moment after the item. The results re-sort for a moment after the last key, so it clicks only once the match has stayed in place for a short while, and returns `false` when the setup screen shows any other item.

Example:
```luau
if GrandExchange.start_buy() then
	print(GrandExchange.choose("Fox fur"))
end
```

## GrandExchange.setup_item

Definition:
```luau
function GrandExchange.setup_item(): number?
```
The id of the item the setup screen is for, nil before one is chosen.

Example:
```luau
print(GrandExchange.setup_item())
```

## GrandExchange.quantity

Definition:
```luau
function GrandExchange.quantity(): number?
```
The setup screen's quantity.

Example:
```luau
print(GrandExchange.quantity())
```

## GrandExchange.price

Definition:
```luau
function GrandExchange.price(): number?
```
The setup screen's price per item.

Example:
```luau
print(GrandExchange.price())
```

## GrandExchange.total

Definition:
```luau
function GrandExchange.total(): number?
```
The setup screen's total price.

Example:
```luau
print(GrandExchange.total())
```

## GrandExchange.set_quantity

Definition:
```luau
function GrandExchange.set_quantity(amount: number, typed: boolean?): boolean
```
Sets the quantity to the whole number `amount`. Small rises are clicked on the +1/+10/+100/+1K buttons; rises needing more than a few clicks, and any fall, are typed into "Enter quantity". `typed` forces typing (`true`) or the buttons (`false`) for a rise.

Example:
```luau
print(GrandExchange.set_quantity(3))
```

## GrandExchange.adjust_price

Definition:
```luau
function GrandExchange.adjust_price(percent: number): boolean
```
Moves the price by `percent`, a multiple of 5 up to 100 either way, one 5% button click at a time.

Example:
```luau
print(GrandExchange.adjust_price(5))
```

## GrandExchange.confirm

Definition:
```luau
function GrandExchange.confirm(): boolean
```
Clicks Confirm and waits for the overview. **This places the offer.**

Example:
```luau
if GrandExchange.total() and GrandExchange.total() < 1000 then
	GrandExchange.confirm()
end
```

## GrandExchange.back

Definition:
```luau
function GrandExchange.back(): boolean
```
Leaves the setup screen without placing anything.

Example:
```luau
GrandExchange.back()
```

## GrandExchange.buy

Definition:
```luau
function GrandExchange.buy(name: string, amount: number, percent: number?): (number?, string?)
```
Places a buy offer for `amount` of `name` in the first free slot, `percent` (a multiple of 5) from the guide price, and returns the slot once it shows the offer, so the next `buy` sees it taken. Each failure before Confirm goes back to the overview and returns nil and a reason, without placing anything.

Example:
```luau
local slot, why = GrandExchange.buy("Fox fur", 4, 5)
if slot then
	local offer = GrandExchange.wait_done(slot, 30000)
	if offer and offer.state == "bought" then
		GrandExchange.collect()
	end
else
	print(why)
end
```

## GrandExchange.wait_done

Definition:
```luau
function GrandExchange.wait_done(slot: number, timeout_ms: number): Offer?
```
Waits up to `timeout_ms` for slot `slot` to be complete or aborted and returns it as it is then, so check its `state`.

Example:
```luau
local offer = GrandExchange.wait_done(0, 10000)
print(offer and offer.state)
```
