# Reverse engineering the native client

How the agent finds game data in a client that changes every week, the tools and reference material for it, and the hard-won facts about the client's internals. Read before writing or fixing an offset rule.

## Offset rules (`src/agent/src/client/offsets*`)

A rule finds a field offset or function from structure that survives game updates, e.g. "the only function containing both `cmp r32, 0x600` and `cmp r32, 0x2E00`" for the actor-position code.

1. Find the game logic in the Java reference (`standalone/`) and pick something that must also exist in native code: a constant, a compare order, a call shape.
2. Scan the PE's code for it, map hits to functions with `.pdata`, decode with `iced-x86`, and track which `[base+disp]` each register was loaded from. Confirm by matching the surrounding flow (same compare order, same calls).
3. Require **exactly one** match; anything else returns `ScanError`.
4. Store the result in a layout type from `ipc::state` (`LayoutMap`, `Layout`) for the readers.

Rules scan the file image only, so every build in `archive/` is regression data: does the rule still find exactly one match on each?

## Reference material

### `standalone/`: decompiled Java client

RuneLite's injected Java client, decompiled, with every name RuneLite's API reveals applied (version in `standalone/VERSION`). Most classes stay obfuscated (about 93 of 730 named). **Read only, never modify, keep private** (git-ignored).

- `src/`: one Java file per class. `MAPPING.md`, `mapping.tsv`: every rename and each obfuscated field's decode multiplier. `build/`, `tools/`: the pipeline that produced it.
- Obfuscated Java int fields hold `real * k` for a secret `k`; **the native client stores plain values**.
- Ignore the junk extra parameter and `try/catch` wrapper on most methods, and RuneLite's injected `getX`/`setX`/`rl$` methods. It does not compile; it is for reading.

### `archive/`: old native client builds

One client exe per game update, named `client-<file date>-<first 8 hex of sha256>.exe` (the exes are git-ignored). `archive/README.md` lists them with per-build notes; add a row when you add a build. After each update, copy the installed client exe (under Bolt: `~/.local/share/bolt-launcher/`) here and hash the exe itself (the `.sha256` beside it doesn't match it).

### `tools/memscan`: live memory scanner

Linux-only CLI (own `Cargo.toml`, not in the workspace; `cargo build --release` inside it). Scans a client running under Wine through `/proc/<pid>/mem`: exact 8-byte values or a `[lo, hi)` range (pointers into a region), aligned `i32`s, or `pair`s (two `i32`s within 16 bytes, for x/y). Arguments in `src/main.rs`.

Workflow: scan for a value you can see in game, change it in game, rescan to narrow, then turn the hit into an offset rule. The Python helpers beside it (`dump.py` and `watch.py` to dump or watch `i32` fields for differential scans, `snapmenu.py`, `probe_region.py`) document their usage in their headers; `snapmenu.py` hardcodes addresses from one build.

Under Wine, memory diffs that skip stack regions (`0x7ffffe...`) miss anything on the main thread's stack, such as the menu object below.

## Native client facts

- Hash tables are custom chained, not `std::unordered_map`.
- Short leaf getters (`mov rax, [rcx+X]; ret`) have no `.pdata` entry, so they are missing from `Pe::functions`; find them by their bytes.

### Menu (`offsets/menu.rs`)

- The menu object (`client + Menu.object`) lives on the main thread's stack, not the heap. Java's `widgetSelected` group (selected spell: verb, name, component id, child, item id, target mask, flag) is a sub-object of it, found by `find_selection` from the native `ug()`/`ut()` (deselect/select) and read with the menu reply.
- Menu rows are 22.5 px high with a 16 px header; the Java client's 19/15 is wrong for the native client.
- The open menu's geometry isn't stored: the client lays it out every frame with a generic list-layout routine (`find_menu_layout`: the one that refuses paths of 5+ submenus and calls itself for the parent) on the minimenu's list view (`minimenu+0x158`, returned by vtable slot 3 of the minimenu), and keeps the result on the stack. The agent calls that routine itself on the window-procedure thread while the menu is open: `fn(view, out: *mut [i32; 26], path: &std::vector<u64>)`, empty path = the menu, row indices = a submenu.
  - Inputs: the anchor written on the opening right click (`view+0x10`: x, y, area w, h) and the style block at `menu+0xac` (width cap, height cap, scrollbar width, paddings, header flag).
  - Row width is the font width of `option + " " + target`; the separator is a fixed string, so the space stays when the target is empty.
  - The open rows are the minimenu's rows vector (`minimenu+0x140`), which leaves out entries the client hides.
- Stay-open check: the menu stays open while the mouse, rounded to interface units, is inside any open level's rect grown by the input handler's margin (`+0x18`, the immediate 10 its constructor stores; check at the caller of the layout routine that grows the rect by a field and its double).
- Submenus (Java's `Menu.au[]`, one per entry) are rows too. A row is 24 bytes: the entry list it comes from (the menu's own list is a sub-object of the menu object; a submenu's list has the same type and the same 0x150-byte entries), the entry's index in that list, and a pointer to the `std::vector` of the rows of the submenu it opens (null for none; surfaced as `MenuLayout::submenu`). The open submenus are the minimenu's `std::vector<u64>` of parent rows followed by its open row (-1 for none). The stay-open check builds that same path, lays out every prefix, and closes only when the mouse is outside all of them; the agent lays out each open submenu the same way.

### Chat history (`offsets/scripts.rs`, `offsets/chat.rs`)

Java's `Messages` is found through the CS2 interpreter, not Java code shape. The range dispatcher (`scripts::range_handler`) sends opcodes 5000-5099 to a handler whose switch (`scripts::switch_cases`) has:

- case 4 (`chat_gethistory_byuid`): the uid lookup over a global hash table, then type, cycle, name, sender, text (`+0x88`, with "[HH:MM] " prepended when the timestamp setting is on);
- native-only case 30: pushes the text as received (`+0xa0`) and calls the stamp chooser (mode 1 `+0x28` UTC, mode 2 `+0x10` local, via `_localtime64`/strftime `"[%H:%M]"`).

No epoch is stored: the injector converts the message cycle to a time with the client cycle, which `clientclock` (opcode 3300) reads from a global; it ticks 50 times a second. Messages are `shared_ptr`s; dialogue lines are logged as type 114 `"Speaker|line"`.

### Scene loader and instances (`ipc::read::map_regions`)

The client rebuilds the scene from a map-rebuild packet, in the packet dispatcher: a `shared_ptr` member of the client object (the one whose offset the `client` pointer already gives) holds the **scene loader** L, and the dispatcher calls one of two methods on it, one for normal scenes and one for instances (each allocates a builder object, stores it at `L+0x28`, frees the previous one, and sets the byte `L+0x54dd` to 0 or 1: the client's own instance flag).

- L: `+0x10` the world view (the one `local_player` returns), `+0x28` the current builder, the **template table** inline from `+0x38` (Java's `instanceTemplateChunks`: 4 planes x 13 columns x 13 rows of 16-byte entries, `(chunkX, chunkY)` in 8-tile chunk units and the destination plane as an `i64`; entry index `(plane * 13 + column) * 13 + row`; written by the builder function that holds `[this+0x18] + index * 16`). Outdoors an entry is the chunk's own coordinates; in an instance it is the source chunk. Chunks the packet doesn't fill are not cleared, so a stale source can stay in the table: use the region vector below for the list of source squares.
- The builder: the base coordinates copied at `+0x58` (`baseX`, `baseY`), the world view at `+0x28`, and at `+0x68` a `std::vector` (begin, end, capacity) of 0x88-byte records: `i32 x, i32 y` (map square), `x << 8 | y` at `+0x10`, then per-plane sub-records. Outdoors it lists the squares around the player; in an instance only the *source* squares (Tempoross: one record, 47,44). Next come the squares the scene sits on (min/max x and y).
- The world view `local_player` returns has the live `baseX`/`baseY` but none of this; other objects carry stale copies of the base coordinates (they look live but never change), so match by structure, not by value.
- `map_regions` finds all of this by shape and takes it only when exactly one builder in the client's loader has a vector of that shape; nothing is an address or a fixed offset. If a game update makes it fail, the code anchors are the two builder methods (each stores its vtable into a fresh `0xf8`/`0xb0` byte object and sets the `+0x54dd` flag) and their caller, the packet dispatcher.
- Instances are placed at x >= 6400, so `Position.in_instance` is a coordinate test; the loader tells which squares an instance copies.

### Projectiles (`offsets/projectiles.rs`)

Java keeps projectiles in `Client.projectiles`; the native client keeps them in a circular list inside the client object (the one the `client` global points to): the list head is its own sentinel node (next, previous, size at `+0x10`), and each node holds a `shared_ptr` whose object pointer (`+0x18`) is the projectile.

- The anchor is the update function (Java's `Projectile.am`): the only code that reads the double `0.02454369` (`tan(slope * 0.02454369)`). Its first `.pdata` entry holding the constant is a chained chunk, so `chunk_root` walks back to the function. It gives every field but the id: the start cycle (first compare), the target and then the source position (two calls to one helper with actor in `edx`, x in `r8d`, y in `r9d`, level on the stack, each followed by `sub` of the end or start height), the `moving` byte (Java's `ay`), slope, start position, end cycle (`- cycle + 1`), the x, z, y doubles (written back to the caller in that order) and the orientation (the first `and 0x7ff` store).
- Its only caller is the loop (Java's `Client.pp`), which compares the end cycle with the client cycle global (the same one chat uses), and a client method `mov rdx, rcx; add rcx, list; jmp loop` gives the list's offset.
- The constructor copies a parameter block over the fields from the source level (`movups`), clears `moving`, and ends with the id, which it reads back to look up the spot animation.
- Tiles are world coordinates; actors are NPC index + 1 or -(player index + 1), 0 for none; the server often sends no source actor (your own bolts have none). `z` counts from the heightmap's zero: the height above the ground is the bilinear heightmap height minus `z`.

### Spot animations on tiles (`offsets/spot_anims.rs`)

Java's `GraphicsObject`s (RuneLite: graphics objects). They are not on actors: an actor's own spot animation is the `graphic` of the NPC or player. The native client keeps the tile ones in a circular list in the world view (not the client object, unlike projectiles): head at `world view + 0x160` (next, previous, size at `+0x10`), each node holding the object pointer at `+0x18`, like the projectile list.

- The anchor is the object's constructor, found by shape: a function of at most 0x200 bytes that looks up the spot animation for the id (`call lookup; mov rdx, [rax+8]; mov edx, [rdx+8]; call init`, as the projectile constructor does) and stores its parameters in fields of `this`. The id is the register it passes to the lookup; the level is the one stored from `r9d`; the three stack parameters are x, y and z in stack order; the start cycle is `add edx, [rsp+n]` (the delay plus the client cycle).
- The constructor is `std::make_shared`'s: a thunk that tail-jumps to it, called from the packet handler for the spot-animation packet. One site calls it directly and one goes through a small wrapper that allocates the block; both then append the new object to the list (`lea rcx, [view + list]` or `add rcx, list`, then a call), which gives the list offset. The last stack argument of the call is the address of the client cycle.
- Fields (build of 2026-10-28): start cycle `+0x260`, level `+0x264`, local fine x, y and z `+0x268`, `+0x26c`, `+0x270` (z is the ground height at the tile minus the height the server sent, so negative is up), id `+0x274`, world view `+0x278`, animation state `+0x280`.
- Camelot Teleport and other spells that flash on the caster set the actor's spot animation, not this list; Telekinetic Grab on a ground item places id 144 on the item's tile, a handy live trigger. The list is read for the main world view (id 0) and, when the local player is in another view (on a boat), for that one too, like RuneLite's overlay; `SpotAnim.view_id` says which. The overlay only draws the main world's: it projects points in the main world, and a boat's effects have the boat's own coordinates.


### Packets (`offsets/packets.rs`, `client/packets.rs`)

Java sends packets from `co.az` (a queue of `js` nodes written to the socket once per tick) and reads them in `Client.iy`. The native client splits the same job across two threads:

- **Network thread, receive** (the chunk around `0x202c1a` in the build of 2026-10-12): reads one byte through a buffered recv wrapper (shared by every socket reader; the same function reads the lobby), subtracts the next ISAAC byte (`sub eax, [state + idx*4 + 4]`, state refilled when its counter hits 0) and reads a second byte if the result is `>= 0x80`, then looks the size up in an int32 table indexed by opcode (`cmp r14w, 0x97` bounds it: 151 packets; `-1` a length byte follows, `-2` a short), allocates a buffer (`call alloc; vector, size, 5`), reads the payload into it and pushes a node (kind 3: opcode at `+0x10`, buffer at `+0x18`) on a lock-free list at `connection + 0x20`. The payload is plain: ISAAC only masks the opcode.
- **Game thread, receive**: the dispatcher (`0x14008e670(client, connection)`, a 151 case jump table on `[rsp+0x70]`) calls `receive(connection, out)` (`0x202840`) for the next packet: it returns a bool, writes the opcode at `out+0` and moves the node's buffer object into `out+8`. That is the receive hook: a plain function entry, one call per packet (about 7 a second in a busy area; the per tick packet is opcode 2 with no payload). `receive_fields` finds it by shape: `cmp dword [rcx + state], 3` first, node kind compared with 3 and 4, the opcode stored at the start of the copy of `rdx`, `lea rcx, [out + n]` before a call.
- **Buffer object**: `+8` size (what the payload was allocated with), `+0x10` data pointer, `+0x18` cursor (bytes written, for packets being built). `BUFFER_SIZE` is the only one of these no code shape names: the allocator and the move treat the first three fields as a unit. The data and cursor offsets come from the opcode writer.
- **Send**: `js.ac` is the packet constructor (`0x6793c0`, 97 call sites, one per client packet type): `(packet, type, cipher)`. It stores the type at `packet+0`, sets up the buffer at `packet+8` and writes the opcode through the **opcode writer** (`0x67d9c0`, `buffer, opcode in dl, cipher`: `byte = opcode + next cipher byte`, appended at `[data + cursor]`). The network thread's send loop (`0x2041f2`) pops the node queue at `connection + 0x30` and calls `send(connection, result, packet)` (`0x204110`), which hands `[packet + 0x18]` and `[packet + 0x20]` to the send-all wrapper. The send hook reads the plain type from `packet+0`, so it never needs the cipher; the first byte of the data is the masked opcode, and a variable length packet has its length right after.
- `find_packets` goes opcode writer (a function of at most 0x80 bytes with a byte `add`, the cipher refill test `mov r32, 0x100`, a byte store through base+index and one call) → its one caller, the constructor → the send function (at most 0x60 bytes, one call, loads at the buffer's data and cursor displacements from `r8`) and, independently, the receive function. Every step wants exactly one match, so a game update that changes the shapes fails the rule instead of hooking something else.
- Sizes of received packets are in the table at `0xbaf8f0` (build of 2026-10-12): the first entries are `1, 0, 0, 2, 4, 0, 5, 6, 0, -1, 28, ...`, and opcode 40 (6 bytes, the most frequent) and 2 (0 bytes, once per tick) are the easiest to see live.
- Probing the live client for this: `gdb -batch -x script -p <linux pid>` with `hbreak` at the addresses above, `handle SIGUSR1 nostop noprint pass` (Wine uses it), `commands; silent; printf ...; continue; end`, then `continue; delete; detach; quit` under `timeout -s INT`. Hardware breakpoints leave the code unpatched; never kill gdb while they are set. A packet with no payload has a null data pointer.

### CS2 interpreter (`offsets/script_runs.rs`, `client/script_runs.rs`; build of 2026-10-12)

The agent hooks the **event runner** (`0x1995c0`, found by `find_script_runs`: the one function of 0x200-0x800 bytes with `lea r, [type-0xa]; cmp r, 7` and compares with 0x12, 0x55 and 0x56; the type, argument vector and count are read from the loads of the event register). Nested runs show as depth. The interpreter itself and the trigger runners are not hooked. Found from the Java side (`ao.co/cm/dn` and `CollisionData.aq` are copies of the interpreter; `StructComposition.az(event, 500000, 475000)` is the event runner) by looking for the 500000 (`0x7a120`) operation limit in the native image. Offsets are image VAs; verified live with `hbreak`.

- **Interpreter** `0x140199dc0` (7.6 KB, opcode jump table, no recursion for gosub): `(ctx rcx, &{control block, Script*} rdx, max_ops r8d, r9d)`. The `shared_ptr` pair is `[rdx]` = control block, `[rdx+8]` = the Script. Every script run goes through it, from three callers: the event runner `0x1995c0` (return `0x199a64`, nearly all runs), the trigger runner `0x199b20` (return `0x199cf4`) and the script-by-trigger runner `0x1993e0` (return `0x199566`, via the `0x199000` wrapper; 22 call sites such as `0x6a2f0`).
- **Event runner** `0x1995c0(ctx, event, max_ops, ?)`: the event's type is `[event+0x8c]` (76 for ordinary scripts; 10..17, 0x12, 0x55, 0x56 are trigger types that find the script with `0x1400325b0(trigger, a, b)`, anything else uses `0x140032350(id)`), the argument count `[event+8]`, the argument vector `[event+0x10]` of 0x28-byte variants (kind byte at `+0x20`: 1 int, 2 string, 3 and 5 other; the int is at `+0`), and `argument 0` is the script id. Mouse x/y are `[event+0x48]`/`[event+0x4c]`, and the `0x80000001..a` placeholder ints in the arguments are replaced from the event before the run.
- **Script id**: for scripts run by the event runner the Script has its id at `+0x18` (matched argument 0 in 12 of 12 distinct scripts). Scripts found by trigger (`0x1993e0`, trigger 49 with a=b=-1) show a constant `0xfe310000` there instead, so their id isn't known; `0x1400325b0` tries `trigger + a<<8`, then `trigger - (b+3)<<8`, then `trigger - 0x200`.
- **Script fields** (as the interpreter reads them): `+0x30` instructions, `+0x48` int operands, `+0x78` long operands, `+0x80`/`+0x84`/`+0x88` local int/object/long counts.
- **Volume**: an idle logged-in account runs about 2000 scripts a second (700 under gdb, which slows it), nearly all two scripts per frame (ids 6697 and 4730 here); the rest run about 7 a second each. RuneLite's default blacklist (3174, 1004) is for other builds' per-tick scripts, so a blacklist box is needed.
- **Probing**: do not use a breakpoint condition (`hbreak ... if`) on `0x199dc0`: gdb evaluates it by stopping every hit and the client froze until gdb was interrupted. Test inside `commands` instead.

### Client variables, varcs (`offsets/varc_setters.rs`, `client/varc_log.rs`; build of 2026-10-12)

The interpreter's varc opcodes (42 and 43 int, 51 and 52 long, 49 and 50 object, as in Java's `kh.ex`) call methods on one `Varcs` object (`[[client] + 0x417ca8]`), and nothing else writes a varc, in the native client or in Java (server varc packets run scripts). The object holds a map from index to a 0x28 byte variant (value at `+8`, kind byte at `+0x28` of a map node).

- `get_int(varcs, index)` (`0x33220`): the function `getIntegerVarc` jumps to; -1 when the varc isn't an int. `set_int(varcs, index, value)` (`0x33130`), `set_long` (`0x33280`) and `set_object(varcs, index, variant*)` (`0x333f0`, takes a pointer to a variant such as an object stack slot) share the map search `0x47b40` with the getters and end by setting a dirty byte (`mov byte [varcs + 0x80], 1`, which makes the client save the varcs). `find_varc_setters` takes the getter, then the functions of 0x80-0x130 bytes that call the same search, load the same two map fields and store that byte; `mov r11b, 1` marks the int one, `mov r11b, 5` the long one, and the one with neither is the object one.
- A variant is 0x28 bytes with its kind at `+0x20` (2 string). A string is inline in the first 0x17 bytes with the room left in the byte at `+0x17`, or, when that byte's top bit is set, a pointer at `+0` and a length at `+8`.

## Item containers

The client keeps its item containers (inventory 93, equipment 94, bank 95, ...) in a hash table global (`find_item_containers` finds the table through the getters the interface scripts call). Each node is `{id at +0, item vector at +8, quantity vector at +0x20, next at +0x38}`.

- **Slot writer** (`0x140030f60` on build a2636a01, Java `ItemContainer.ab`/`setItemTableSlot`): `set_slot(container: ecx, slot: edx, item: r8d, quantity: r9d)`. It finds the node (making it when missing: the map insert function, then the vectors are filled with -1 and 0), grows both vectors with one resize function called twice, then stores the item and the quantity through `[base + index]`. The inventory update packets call it once per slot, so a full update of the bank is a few hundred calls in one tick.
- The unwind table splits the function in two chunks (the stores are in the second), so `find_item_setter` reads on from the start of the first chunk to the padding. It wants the one function that reads both of the table's globals (buckets and bucket count), calls one function twice and has two indexed 32 bit stores. The getters read the same globals but neither resize nor store.
- Removing a container (Java `ItemContainer.ai`) is a separate function the agent does not hook: the log never says a container went away, only that a later write made it again (`old_known` false).
- Verified live on a2636a01 with an unequip/equip: both containers' writes were logged in the same tick.

## Component fields (`WidgetDetail`)

The inspector's extra fields are read at fixed offsets of the client's component object (`decode/src/widgets.rs`, `DETAIL_*`; the offsets in `InterfaceLayout` that the agent finds by rules are id, index, kind, position, parent, hidden, scroll, sprite, text colour, text and the dynamic children). The Java client's field order isn't kept by the native struct, so they were found by correlation, which works for any field the cache's interface definitions state:

1. `injector widget-dump <pid> 1024` lists the first 1024 words of every open component; `injector cache-file <pid> 3 <group> <component> <out>` writes its definition from the cache (index 3, group = interface, file = component).
2. Decode the definitions with the reader in the Java client (`Widget.af`, the IF3 decoder: type, content type, original x y width height, width/height/x/y modes, parent, hidden, then per type scroll, sprite, model, font and text, rectangle or line fields, click mask, name, actions, listeners).
3. For each field, count per word (or byte) offset how many components hold the field's value there. A real field matches about 98-100% (the rest are values scripts changed after loading); chance matches are visible as a rate equal to the share of zeros.

Found on build a2636a01, by byte offset: content type 0x30, x/y/width/height modes 0x3c/0x40/0x44/0x48, original x/y/width/height 0x4c..0x58, relative x/y/width/height 0x5c..0x68, filled 0xa8 (byte), opacity 0xb0 (byte, 255 is invisible), sprite tiling 0xec (byte), font 0x150, line height 0x188, x and y alignment 0x18c/0x190, text shadowed 0x194 (byte), click mask 0x2b0 (three bytes of flags, not shown), item id 0xb38 and quantity 0xb3c (dynamic children only; matched against the inventory's slots, the quantity only seen as 1 and 0), no click through 0xb98 (byte, 99%). Width, height and their modes match the cache in only about 70%: scripts resize most of the resizable interface. The listeners are vectors (begin and end pointers; the load listener is at 0x358, mouse over 0x448); name, target verb, model fields (no open interface had a model), border type, drag settings and the item quantity mode were not found.

