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
