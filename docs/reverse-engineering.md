# Reverse engineering the native client

Facts about the client's internals that the code does not state, and the steps for finding offsets again after a game update. Read this before writing or fixing an offset rule. Addresses are image VAs or RVAs from the build named next to them, and they move with every update. Use them only as hints when probing.

## Offset rules (`src/agent/src/client/offsets*`)

- A rule `find_<name>(pe: &Pe, ...)` finds a field offset or a function from structure that survives updates, such as a constant, the order of compares or the shape of a call. `find_offsets` (`client/hooks.rs`) parses the image once and hands every rule the same `Pe`. Tests use `Pe::parse(&image)`.
- Procedure:
  1. Find the logic in `standalone/` and pick something that native code must also have.
  2. Scan the code, map each hit to its function through `.pdata`, decode it with `iced-x86` and track which `[base+disp]` loaded each register. Confirm the hit by matching the surrounding flow.
  3. Require exactly one match. Anything else is a `ScanError`, so a changed shape fails the rule instead of hooking the wrong code.
  4. Store the result in an `ipc::state` layout type.
- Every build in `archive/` is regression data. Each rule must still find exactly one match on all of them.
- Leaf functions such as `mov rax,[rcx+X]; ret` have no `.pdata` entry and are missing from `Pe::functions`. Find them by their bytes, or by scanning `direct_calls` targets up to the first `ret`.
- Some functions are split across chained `.pdata` chunks. `chunk_root` walks back to the entry.
- Hash tables are custom chained tables, not `std::unordered_map`.

## Reference material and tools

- **`standalone/`** is the decompiled Java client with the names from its API applied. Its version is in `standalone/VERSION`, and only about 93 of 730 classes are named. It is read only, private and git-ignored. `MAPPING.md` and `mapping.tsv` hold the renames and the field multipliers.
  - Obfuscated Java int fields store `real * k`. The native client stores plain values.
  - Ignore the junk extra parameter, the `try/catch` wrappers and the injected `getX`/`setX`/`rl$` methods. The code does not compile.
- **`archive/`** holds one exe per update, named `client-<file date>-<first 8 hex of sha256>.exe` (git-ignored). After each update:
  1. Copy the installed exe (under Bolt: `~/.local/share/bolt-launcher/`) into `archive/`.
  2. Hash the exe itself, because the `.sha256` file beside it doesn't match.
  3. Add a row to `archive/README.md`.
- **`tools/memscan`** is a Linux-only crate outside the workspace (`cargo build --release` inside it). It reads `/proc/<pid>/mem` of a client running under Wine. It scans for exact 8-byte values, `[lo,hi)` pointer ranges, aligned `i32`s, `pair`s (x/y within 16 bytes) and `fpair`s. See `src/main.rs`.
  - Workflow: scan for a value you can see in game, change it in game, rescan to narrow the hits, then turn the hit into a rule.
  - Helpers: `dump.py` dumps `i32` fields for diffing, and `watch.py` prints `i32` changes with timestamps. `snapmenu.py` and `probe_region.py` hardcode addresses from one build.
  - Under Wine, a diff that skips stack regions (`0x7ffffe...`) misses objects on the main thread's stack, such as the menu.
- **gdb probing**:
  - Run `gdb -batch -x script -p <linux pid>` under `timeout -s INT`.
  - Use `hbreak`, which leaves the code unpatched. Set `handle SIGUSR1 nostop noprint pass` because Wine uses that signal, and put `commands; silent; printf ...; continue; end` on each breakpoint.
  - Finish with `delete; detach; quit`. Never kill gdb while breakpoints are set.
  - Never use `hbreak ... if` on a hot function: gdb stops on every hit to check the condition, and the client froze. Test the condition inside `commands` instead.
- **Injector dumps**: `injector player-words <pid> <index> <count>`, `injector widget-dump <pid> <count>`, `injector cache-file <pid> <index> <group> <file> <out>`.

## Keyboard focus (`client/focus.rs`)

- The client drops posted `WM_KEYDOWN`s while `GetFocus` isn't its own window. Mouse input still works. The agent's `GetFocus` import hook answers with the render view, which fixes this. Swallowing `WM_KILLFOCUS` or posting `WM_SETFOCUS` does not help. Where the client makes the check wasn't traced.

## Menu (`offsets/menu.rs`)

- The menu object lives on the main thread's stack. The selected spell (Java's `widgetSelected` group) is a sub-object of it, found by `find_selection` through the native deselect and select functions.
- Rows are 22.5 px high with a 16 px header. The Java client's 19/15 is wrong for the native client.
- The client doesn't store the menu's geometry. It lays the menu out every frame and keeps the result on the stack. The agent calls the layout routine itself (`find_menu_layout`) on the window-procedure thread: `fn(view, out: *mut [i32; 26], path: &std::vector<u64>)`. An empty path means the menu, and row indices mean a submenu.
  - You can recognise the routine because it refuses paths of 5 or more submenus and calls itself for the parent.
  - The view is the minimenu's list view (`minimenu+0x158`, also vtable slot 3).
  - Inputs:
    - The anchor written on the right click: `view+0x10`, holding x, y, area w and h.
    - The style block at `menu+0xac`: caps, scrollbar width, paddings and the header flag.
  - Row width is the font width of `option + " " + target`. The space stays when the target is empty.
  - The open rows are `minimenu+0x140`, which leaves out hidden entries.
- **Stay open**: the menu stays open while the mouse, rounded to interface units, is inside any open level's rect grown by the input handler's margin (`+0x18`, set to 10 in the constructor). The margin is in the caller of the layout routine that grows the rect by a field and its double.
- **Submenus** (Java's `Menu.au[]`) are rows too.
  - A row is 24 bytes: the source entry list (0x150-byte entries), the index in that list, and a pointer to the submenu's rows vector, or null.
  - The open path is the minimenu's `std::vector<u64>` of parent rows, then its open row (-1 for none).
  - The stay-open check lays out every prefix of that path. The agent does the same.

## Chat (`offsets/scripts.rs`, `offsets/chat.rs`)

- `Messages` is found through the CS2 interpreter. The range dispatcher sends opcodes 5000-5099 to one switch.
  - Case 4 (`chat_gethistory_byuid`) holds the uid lookup in a global hash table, followed by type, cycle, name, sender and the text at `+0x88`. That text has `[HH:MM] ` prepended when the timestamp setting is on.
  - Case 30 exists only in the native client. It pushes the text as received (`+0xa0`) and calls the stamp chooser: mode 1 is UTC (`+0x28`) and mode 2 is local time (`+0x10`).
- No epoch is stored. The injector converts a message's cycle to a time with the client cycle (`clientclock`, opcode 3300, read from a global), which ticks 50 times a second.
- Messages are `shared_ptr`s. Dialogue lines are logged as type 114 `"Speaker|line"`.

## Scene loader and instances (`ipc::read::map_regions`, `offsets/scene_loader.rs`)

- A `shared_ptr` member of the client object holds the scene loader L. The packet dispatcher calls one of two methods on L, one for normal scenes and one for instances. Each method stores its vtable into a fresh builder (`0xf8` or `0xb0` bytes), stores the builder at `L+0x28` and sets L's instance flag byte to 0 or 1 (it was `+0x54dd`; `find_instance_flag` finds it). These two methods and the dispatcher are the anchors if the rule breaks.
- L layout:
  - `+0x10` is the world view.
  - `+0x38` is the inline template table (`instanceTemplateChunks`): 4x13x13 entries of 16 bytes, each `(chunkX, chunkY)` plus the plane as an `i64`. The index is `(plane*13+col)*13+row`.
  - Entries the packet doesn't fill are not cleared, so the table can hold stale data. Use the builder's vector for the list of source squares.
- Builder layout:
  - `+0x58` holds a copy of baseX and baseY.
  - `+0x68` is a `std::vector` of 0x88-byte records: `i32 x, y` (map square), `x<<8|y` at `+0x10`, then the per-plane sub-records.
  - Outdoors the records are the squares around the player. In an instance they are only the source squares (Tempoross: one record, 47,44).
- Other objects carry stale copies of baseX and baseY that look live but never change. Match by structure, not by value. `map_regions` requires exactly one builder vector of the right shape.
- Instances sit at x >= 6400, so `in_instance` is a coordinate test.

## Projectiles (`offsets/projectiles.rs`)

- The client object holds a circular list. Its head is a sentinel node (next, prev, size at `+0x10`), and each node holds a `shared_ptr` with the projectile at `+0x18`.
- Anchor: the update function (Java `Projectile.am`) is the only code that reads the double `0.02454369`. Its first chunk is chained, so the rule uses `chunk_root`. The function gives every field except the id:
  - start cycle: the first compare
  - target position, then source position: two calls to one helper (`edx` actor, `r8d` x, `r9d` y, level on the stack), each followed by a `sub` of the end or start height
  - the `moving` byte, slope and start position
  - end cycle: `- cycle + 1`
  - x, z and y doubles, in the order they are written back
  - orientation: the first `and 0x7ff` store
- The only caller is the loop, which compares the end cycle with the client cycle global. A client method `mov rdx,rcx; add rcx,list; jmp loop` gives the list offset.
- The constructor ends by reading the id back to look up the spot animation.
- Positions are world tiles. Actor references are NPC index + 1 or -(player index + 1), and 0 means none. The server often sends no source actor, including for your own bolts. `z` is measured from the heightmap's zero, so the height above the ground is the bilinear height minus `z`.

## Spot animations on tiles (`offsets/spot_anims.rs`)

- Java's `GraphicsObject`. An actor's own spot animation is the actor's `graphic` field, not this list.
- The list lives in the world view (`+0x160`) and has the same node shape as the projectile list.
- Anchor: the constructor is a function of at most 0x200 bytes that calls `lookup; mov rdx,[rax+8]; mov edx,[rdx+8]; call init` and stores its parameters in `this`.
  - The id is the register passed to the lookup, and the level is the value stored from `r9d`.
  - The three stack parameters are x, y and z.
  - The start cycle is `add edx,[rsp+n]` (delay plus client cycle).
- The constructor is reached through a `make_shared` thunk from the spot-animation packet handler, either directly or through an allocating wrapper. Each site then appends to the list (`lea rcx,[view+list]` or `add rcx,list`), which gives the list offset.
- Fields on 2026-10-28:

  | Field | Offset |
  | --- | --- |
  | start cycle | `+0x260` |
  | level | `+0x264` |
  | x, y, z | `+0x268`, `+0x26c`, `+0x270` |
  | id | `+0x274` |
  | view | `+0x278` |
  | animation | `+0x280` |

  z is the ground height minus the height sent, so negative is up.
- Live trigger: Telekinetic Grab on a ground item places id 144 on the item's tile. Teleports and similar spells set the actor's spot animation instead.
- The agent reads the main view, plus the player's view when the player is on a boat. The overlay draws only the main view.

## Actor path and server tile (`offsets/actor_path.rs`)

- `pathX`, `pathY` and `pathLength` hold scene tiles, newest first. Entry 0 is the tile the server last set. The fine position lags it by a tick or two. The world tile is the view base (`view+0x24`, `+0x28`) plus entry 0.
- Anchor: the path reset `(rcx actor, edx x, r8d y)` (Java `Player.ax`). It is a leaf with no `.pdata` entry, and it computes `local = (size + 2*tile) << 6`.
- Fields (2026-10-11 and 2026-10-12): size `+0x278`, length `+0x3ec`, `pathX` `int[10]` at `+0x3f0`, `pathY` at `+0x418`.
- The receive hook compares the server tile at its start (see Packets).

## Movement poses (`decode::world::PLAYER_MOVEMENT`)

- Eight `i32`s at `+0x280`, right after the size: stand, turn left, turn right, walk, walk back, walk left, walk right and run.
- The poses come from the appearance packet, so they follow the weapon:
  - unarmed: 808, 823, 823, 819, 820, 821, 822, 824
  - staff: 813, 1209, 1209, 1205, 1206, 1207, 1208, 1210
- The pose at `+0x4b8` is one of these unless something overrides it, such as a tightrope or a ledge.
- To find them again: `injector player-words <pid> <index> 400` while wielding a known weapon, then look for the run of eight values.

## Packets (`offsets/packets.rs`, `client/packets.rs`, `offsets/packet_layouts.rs`)

- **Network thread, receive**:
  1. Reads one byte through a buffered recv wrapper that every socket reader shares.
  2. Subtracts the next ISAAC byte. If the result is `>= 0x80`, a second opcode byte follows.
  3. Looks up the size in an `i32` table of 151 entries. -1 means a length byte follows, and -2 means a length short follows.
  4. Reads the payload and pushes a kind-3 node (opcode at `+0x10`, buffer at `+0x18`) on a lock-free list at `connection+0x20`.
  - ISAAC masks only the opcode. The payload is plain.
- **Game thread, receive**:
  - The dispatcher, which has a 151-case jump table, calls `receive(connection, out) -> bool`. It writes the opcode at `out+0` and moves the buffer into `out+8`.
  - That function is the receive hook: one call per packet, plus at least one call per frame when the queue is empty.
  - The dispatcher calls `receive` only after the previous handler returned, so the start of the hook sees the effects of every packet.
  - Opcode 2 has no payload, arrives once per tick and increments the tick counter. Opcode 40 (6 bytes) is the most frequent. Both are easy to see live.
- **Buffer object**: `+8` allocated size, `+0x10` data, `+0x18` cursor. No code shape names `BUFFER_SIZE`. The data and cursor offsets come from the opcode writer.
- **Send**:
  - The packet constructor (`js.ac`, 97 call sites) takes `(packet, type, cipher)`. It stores the plain type at `packet+0` and writes the opcode through the opcode writer (`byte = opcode + next cipher byte`).
  - The network thread pops the queue at `connection+0x30` and calls `send(connection, result, packet)`.
  - The hook reads the type, so it never needs the cipher. The first data byte is the masked opcode, and a variable-length packet has its length right after.
- **`find_packets` chain**:
  1. The opcode writer: at most 0x80 bytes, a byte `add`, `mov r32,0x100`, an indexed byte store and one call.
  2. Its only caller, the constructor.
  3. The send function: at most 0x60 bytes, one call, loads at the data and cursor offsets from `r8`.
  - The receive function is found independently.
- **Layouts** (`find_packet_layouts`) are published to the loader live, so this needs no step after an update.
  - Each case's reads go through small buffer read functions. A tiny interpreter classifies each one on probe bytes: byte order, the `x`/`x-128`/`-x`/`128-x` transforms and signedness from the caller's `movsx`.
  - Big-endian reads first test a global against `0x03020100`. The interpreter takes that test as equal.
  - A case's layout is its reads up to the first branch. It is kept only when the widths sum to the size in the table.
  - The size table is found as the one `lea r,[rip+table]` right after a `jae` whose 151 values are all in -2..512, with 10 or more distinct values.
  - Strings, smarts and loops have branches, so they don't classify. 86 of 151 packets get a layout, and none differs from what the Java client reads.
- **Names and the fallback table**: `tools/packetdefs/gen.py` produces `src/loader/src/packet_defs.tsv` from `standalone/` (`ServerPacket` `jc` sizes plus the reads of the `Client.iy` handlers).
  - `--exe` checks the native size table against `jc`.
  - `names.tsv` names packets and their fields in wire order. Check the order against the Java handler.
  - After an update, rebuild `standalone/` first. This step is optional because layouts don't depend on it.
  - Sent packets aren't covered.
  - Tests: `cd tools/packetdefs && python3 -m unittest test_gen`.
- A packet with no payload has a null data pointer.

## CS2 interpreter (`offsets/script_runs.rs`, `client/script_runs.rs`)

- Found from the Java side, where `StructComposition.az(event, 500000, 475000)` is the event runner: search the native image for the 500000 (`0x7a120`) operation limit.
- The agent hooks only the event runner, which handles nearly all runs. The interpreter, the trigger runner and the script-by-trigger runner are not hooked.
- The interpreter's signature is `(ctx, &{control, Script*}, max_ops, ?)`. Script fields: `+0x30` instructions, `+0x48` int operands, `+0x78` long operands, `+0x80`/`+0x84`/`+0x88` local int/object/long counts.
- Event fields:
  - `+0x8c` type: 76 for ordinary scripts. 10..17, 0x12, 0x55 and 0x56 are trigger types.
  - `+8` argument count.
  - `+0x10` argument vector of 0x28-byte variants (kind byte at `+0x20`: 1 int, 2 string). Argument 0 is the script id.
  - `+0x48`/`+0x4c` mouse x/y. The placeholder ints `0x80000001..a` are filled from the event.
- The Script's id is at `+0x18` for event runs. Scripts found by trigger show `0xfe310000` there, so their id is unknown.
- An idle account runs about 2000 scripts a second, mostly two per frame. The default blacklist from other tools is for other builds, so the loader needs its own blacklist box.

## Varcs (`offsets/varc_setters.rs`, `client/varc_log.rs`)

- Only the interpreter's varc opcodes write varcs: 42/43 int, 51/52 long, 49/50 object. Server varc packets run scripts.
- The opcodes call methods on one `Varcs` object, which holds a map from index to a variant.
- The setters share the getter's map search and end with `mov byte [varcs+0x80],1`, a dirty flag that makes the client save the varcs.
  - `mov r11b,1` marks the int setter.
  - `mov r11b,5` marks the long setter.
  - The setter with neither is the object setter, which takes a `variant*`.
- A variant is 0x28 bytes with its kind at `+0x20` (2 is a string).
- A string is 0x18 bytes: up to 0x17 inline characters, with the room left in byte `+0x17`. When that byte's top bit is set, the string is on the heap with its pointer at `+0` and its length at `+8`. The empty string is all zeros with `0x17` in the last byte.

## Item containers (`offsets/items.rs`, `offsets/item_setter.rs`)

- The containers are in a global hash table (inventory 93, equipment 94, bank 95). A node is `{id +0, items vector +8, quantities vector +0x20, next +0x38}`.
- The slot writer `set_slot(ecx container, edx slot, r8d item, r9d qty)` (Java `setItemTableSlot`) is the one function that:
  - reads both table globals (buckets and count),
  - calls one resize function twice,
  - and makes two indexed 32-bit stores.
- The writer is split in two chunks, so the rule reads from the first chunk on to the padding. The getters read the same globals but neither resize nor store.
- A full bank update is a few hundred calls in one tick.
- Removing a container is a separate function that isn't hooked, so the log only shows a container being recreated (`old_known` false).

## Item icons (`offsets/item_icon.rs`, `client/cache.rs`)

- Java `getSprite` is native `(out: &mut [control, sprite], id, qty, border, shadow, text mode, noted: u8, width, height, text: &string)`.
  - The client caches the sprite unless the item is noted, so the agent's calls cost the same as the inventory's own draws.
  - Native text mode 1 draws the number. Java's "when stackable" is decided by the caller.
  - The agent passes border 1, shadow `0x302020`, 36x32 and an empty string, which matches what the inventory shows.
- To find the function:
  1. Find the stack-number builder, the one function holding `<col=ffff00>`, `<col=ffffff>` and `<col=00ff80>` that compares with both thresholds.
  2. Its only caller is a chunk of the icon function. Use `chunk_root`, then check that the prologue homes `r8d` and `r9d`.
- The sprite is a shared pointer pair. The object has its pixel count at `+0` and its pixels at `+0x10`, as `0xAARRGGBB` words with alpha already applied. A pixel count other than width x height means there is no icon.
- An out-of-range id draws a default definition, so the agent refuses ids outside `0..65536`.

## Component fields (`decode/src/widgets.rs`)

- Model: the type is at 0xfc (1 means a model id), the id at 0x100, the rotation at 0x11c/0x120 and the zoom at 0x128. They were found on the certer's quiz, whose buttons hold models 8965-8967.
- The `DETAIL_*` offsets were found by correlation, because the native struct doesn't keep the Java field order. The same method works for any field the cache definition states:
  1. Run `injector widget-dump <pid> 1024` and `injector cache-file <pid> 3 <interface> <component> <out>`.
  2. Decode the definitions with Java `Widget.af`, the IF3 decoder.
  3. For each field, count how many components hold its value at each offset. A real field matches about 98-100%. A chance match shows up as a rate equal to the share of zeros.
- Width, height and their modes match only about 70%, because scripts resize most of the resizable interface.
- Listeners are vectors: onLoad at 0x358, mouseover at 0x448.
- Not found: name, target verb, border type, drag settings, the item quantity mode, and the model fields by correlation (no open interface had a model).

## Objects and the scene's edge

- Object definition transform opcodes 77 and 92 use 2-byte ids, and 97 and 98 use 3-byte ids. 92 and 98 end with a default id.
- An opcode the decoder doesn't know fails the whole definition silently, and the object then has no collision or options. Missing 97 and 98 once hid 842 of 63,320 definitions, doors among them.
- The client doesn't fill the scene's outer rows, which read as open ground. The planner takes those tiles from the cache. Planning on them made false routes and back-and-forth walking.

## Definitions and models (`offsets/types.rs`, `client/model.rs`)

- Every definition lookup calls one cache reader with the config group in `r8d`. A lookup takes `(out: *mut [u64;2], id: edx)` and returns a {control, object} pair that `release` frees.
- Groups: idk 3, inv 5, enum 8, NPC 9, obj 10, param 11, seq 12, spotanim 13, varbit 14, hitsplat 32, healthbar 33 (id in `rdx`), struct 34, db row 38, db table 39.
  - Locs (6), varps (16) and varcs (19) take the id in `ecx` and return a pointer into the cache.
  - A bad id gives a default definition, so the agent refuses ids outside `0..0x10000`.
- `NpcType::getModel(npc, out, seq, frame, seq2, frame2, overrides)`:
  - It is the call right after the NPC actor's get-model loads `[actor+npc_definition]` into `rcx`, and it calls itself for an NPC's other forms.
  - `seq` is the animation object or null. `seq2` is null with `frame2` -1, and `overrides` is null.
  - Posing copies the cached model, so the cached one is never changed.
- Animation frames:
  - The skeletal check is the leaf `mov eax,[rcx+4]; shr eax,31; xor al,1; ret`.
  - When it says no, the callers read the frame count (`+0x48`) and the frame array (`+0x50`).
  - The client doesn't bounds-check, so the agent checks `0 <= frame < count` with a non-null array.
  - Skeletal animations (`[seq+0x38]`/`[seq+0x3c]`) aren't supported.
- Found but unused:
  - the model loader `(out, source = client+0x2570, id)`
  - `ModelData` lighting (vtable `+0x70`)
  - generic animate `(out, 0x48-byte params)`
  - the spot animation and loc model builders
  - set-sequence `(client, actor, seq, delay)`, which works on any actor until the server overrides it

## NPC head icons (`offsets/values.rs` `find_npc_head_icons`)

- The NPC holds a pointer at `+0x760` to a 0x90-byte object of two `std::vector<int>` with inline storage for 8: archive ids at `+0` and sprite indices at `+0x48`. Unset slots are -1.
- Archive 440 is the prayer icons: sprite 0 protects from melee, 1 from missiles and 2 from magic.
- The rule matches the clear function: `mov r,[rcx+X]; test; je; mov qword [rcx+X],0; lea rcx,[r+V]; call F; mov rcx,r; call F`.
- Only icons the server sets are read. Default icons from the NPC's definition are not.
