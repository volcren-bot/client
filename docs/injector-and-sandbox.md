# injector (`src/injector`)

Library and CLI for everything outside the client: find clients, inject the agent, read what it publishes, send input, run Luau scripts.

Real code only on Windows. On other targets the same API compiles to stubs returning "clients can only be reached from Windows or Wine" (the `offline` module in `sandbox.rs`) so the loader still builds natively; on Linux the real work is `injector.exe` under Wine.

## Modules

| Module | Role |
| --- | --- |
| `win.rs` | Windows plumbing: `list_clients`, inject/unload, shared-memory mapping, connection status, console, display name, total-XP offset, overlays. |
| `lib.rs` | Public API and data types (`MenuEntry`, `MenuClick`, `EntityModel`, ...). |
| (re-exports in `lib.rs`) | The decoders live in `src/decode` so the agent runs the same code in-process; `lib.rs` re-exports their types. They decode game state, skills, varps, NPCs/players/ground items, widget trees and sub-interfaces, chat and the world list. |
| `projection.rs` | World to screen (the agent also answers `PROJECT_MESSAGE`; `Via` picks the method). |
| `config.rs` | Config definitions read through the client's cache: `enum_def`, `item_params`, `varbit`. |
| `cache.rs`, `collision.rs`, `pathfinding.rs` | Collision maps from the client's cache, then `find_path` (A*-style). No transports. The cache's closed doors are marked (`flag::DOOR_*`, kept in the world collision file: bump `WORLD_FORMAT` when the bits change): a wall of a loc that is interactive (Java's `int1`, op 19, else models of shape 10 or any option: what makes the minimap draw a wall red) with an "Open" option. `find_path_with(.., doors)` goes through them. |
| `nav.rs` | `nav_plan`'s planner: one Dijkstra over all four planes of the loaded scene (the cache's collision past it, doors open). Closed doors are the clickable walls (`Loc::clickable`, the tag's bit 19) with a door option whose walls are in the live collision; their bits are taken out while planning and a step through them is an action. Climbs are clickable locs with a climbing option, each paired with the nearest loc on the plane it leads to that climbs back. Loc definitions come from the cache with their transforms resolved by the live varbits and varps (`win::shown_locs`). Pure, tested natively; `win::nav_plan` gathers the data. |
| `scene_collision.rs`, `pathfinding::game_route` | The client's live collision map of the loaded scene (`read`: the player's plane, `read_all`: every plane) (works in instances) and the game's own walk-click route finder over it (Java `gp.nw`: BFS, neighbours W, E, S, N, SW, SE, NW, NE; an unreachable click ends at the nearest tile within 10 that is under 100 steps). Matches walked tiles live. Exposed as `scene_route`; the flags themselves as `collision` (`sandbox::collision_cut`, an area of the player's plane). |
| `scene_graph.rs` | Randomized routes: a lattice of jittered nodes over the scene's walkable tiles (edges are BFS walks checked with `pathfinding::step_open`), cached per scene by a hash of its flags, and a noisy Dijkstra over it with the start and target attached to their nearest nodes. Each call then moves the route's nodes by up to `offset` tiles (fresh randomness per call, re-joined with BFS walks). Falls back to `game_route` when short, unreachable or past `max_detour`. `scene_route` uses it when given options, over the scene's live collision (minus its 4 outermost tiles, which miss objects), the scenes it showed lately, and the cache's `world_collision` squares for anything else, so targets outside the scene work. |
| `input.rs`, `wind_mouse.rs` | Synthetic mouse/key input via `PostMessageW`; human-like paths (WindMouse, credit BenLand100). |
| `async_input.rs` | `Worker<J>`: one thread per pid running queued jobs, `cancel` (drop the queue, stop the running job, return once idle), `Pace` (the sleep/cancelled hook input code goes through). Platform independent, so it is tested natively. |
| `input_jobs.rs` | The jobs the sandbox's worker runs (`Job`: an input or a wait) and the held-button/key tracking released on cancel. |
| `font.rs` | The game's bitmap fonts, decoded from the cache, for overlay text. |
| `devtools.rs` | Pure logic behind the loader's dev-tool overlays (tile and movement flags, line of sight, regions). The overlays themselves are `win::scene_tiles` layers; a `widget:x,y,width,height,rrggbb` layer boxes a canvas area (the widget inspector's selection), `spot-anims` outlines each spot animation's tile in magenta with its id, and `projectiles` labels each projectile with its id and cycles left at its 3D position (height from the terrain's heightmap) and draws a line to where it lands (the target actor's tile when it has one). |
| `form.rs` | Script settings form spec and the `STATS_PREFIX` line format, shared with the loader. |
| `sandbox.rs` | The Luau runtime (`mlua` with `luau-jit`) and every script global. |
| `main.rs` | CLI. |

## Session cache and the agent's worker

- `win.rs` keeps a `Session` per pid for 500 ms (process handle, mapped state block, the agent's work event), so a read doesn't reopen them; `forget(pid)` drops it on agent load/unload, a protocol mismatch or a request timeout. `input.rs` caches the `JagRenderView` window per pid and checks it with `IsWindow`.
- Every pure memory read (`npcs`, `players`, `ground_items`, `projectiles`, `spot_anims`, `widget`, `widget_children`, `sub_interfaces`, `game_state`, `item_container`, `varc`, `chat_messages`, `world_select`) first asks the agent's worker thread (`ask_worker`, see `work_event_name` in [ipc-protocol.md](ipc-protocol.md)) and decodes its postcard answer; the same `decode::worker` function runs through `ReadProcessMemory` when it can't answer. `npcs` takes about 0.09 ms against 2.4 ms that way. The first call after an agent reload may fall back.
- `ground_items` keeps item names in a per-process cache: a name never changes while the client runs, so only unseen ids cost a game-thread request.

## CLI

Every command prints JSON (`Result<T, String>`) except `run`. The full list is the match in `main.rs`, plus input and overlay commands (`Input::from_args`, `Overlay::from_args`).

```
injector list                                      game clients
injector load <pid> <dll> | unload <pid>           inject / reload / unload the agent
injector status <pid>                              connection, version, protocol, offsets state
injector state <pid>                               full game state
injector packets <pid> <since>                     packet log from sequence number <since> (0: oldest kept): `{packets, next, dropped}`
injector scripts <pid> <since>                     script log from sequence number <since> (0: oldest kept): `{runs, next, dropped}`; each entry is a script's start or end (`end`) with its microsecond time, game tick, event type, argument count, nesting depth and the first arguments as text
injector varc-changes <pid> <since>                varc log from sequence number <since> (0: oldest kept): `{changes, next, dropped}`; each change has the varc index, whether it is a string, the old value (none when unknown) and the new one
injector item-changes <pid> <since> [wait ms]      item log from sequence number <since> (0: oldest kept; 18446744073709551615 reads nothing and returns the current `next`); with nothing new it blocks up to [wait ms] (at most 5000) for the agent to log a change: `{changes, next, dropped}`; each change is one container slot write that changed it: tick, container, slot, old and new item and quantity, and whether the container existed before
injector containers <pid>                          every item container the client holds, by id: `[[id, [{slot, id, quantity}]]]`
injector item-stackable <pid> <id>...              whether each item stacks, from its definition
injector widget-tree <pid>                        every open interface for the widget inspector: `{root, sub_interfaces, widgets}`, each widget a component of an open group or a dynamic child (with the component it hangs off); read straight from memory (about two seconds under Wine for 7000 widgets)
injector widget-words <pid> <group> <child> <index|-1> <count>   debug: the first <count> 4-byte words of a component (or of its dynamic child <index>) with its address
injector widget-dump <pid> <count>                 debug: the first <count> words of every component of the open interfaces and of their dynamic children: `[[id, index, [words]]]`
injector run [--script <path>] <pid> < x.luau      run a script; --script enables disk `require`
injector serve                                     JSON-lines server (the loader's Wine backend)
```

## Luau sandbox

`sandbox::run` / `run_with_form` create a Lua state bound to one pid, with `dofile` and `loadfile` removed and a sandboxed `require`. The globals are defined in `sandbox.rs`; scripts use them through the Luau lib's `host.luau` (a separate project), never raw.

- Queries and actions return `(result, err)`.
- `connection` and `game_state` raise instead ("no client selected", "agent isn't loaded in the client"); `run <pid>` exits at startup when the agent is missing or on another protocol, so scripts never see a nil connection.
- `time()` is ms since start, `clock()` seconds since local midnight; `form(spec)` shows a settings dialog (defaults when no handler).
- `report_stats(experience, gold, custom?)` writes a `form::STATS_PREFIX` line (`\u{1e}STATS {json}`) through the print sink; the loader sums these between heartbeats and sends them to the API. Experience must be 0 or more; values are rounded to integers. Outside a store run the loader drops them. The gains are since the last call, not totals; runtime is measured by the loader, never reported by the script.

### Async input

`click_async`, `move_async`, `wind_mouse_async`, `type_text_async`, `mouse_down_async`, `mouse_up_async`, `scroll_async`, `key_down_async` and `key_up_async` queue the same input as their blocking twins on the script's `async_input::Worker` (one per `run`, created with the Lua state) and return immediately; jobs run in order. The script takes the input back with:

- any blocking input (`click`, `move`, `mouse_down`, `key_down`, ...), which cancels first;
- `input_cancel()`;
- the live menu reads `menu`, `menu_open` and `menu_layout` (not `menu_click`, a record of the last click).

Cancelling drops the queue, interrupts the running job's sleeps and returns once the worker is idle, so nothing is posted after it. A job always releases what it pressed: a cancelled click still sends the button up, `type_text` releases the key and the modifiers. Buttons and keys pressed by `*_down_async` jobs are tracked (`input_jobs.rs`, `Held`): `cancel` and shutdown release them, last pressed first, whether the worker was idle, paused or mid-move. `sleep_async(ms)` queues a pause (`Job::Wait`) for holding something down. `input_busy()`, `input_wait()` and `input_error()` (take and clear the last failed job's error; `input_wait` waits first) cover the rest. The worker is shut down when the script ends. New input code must sleep through `Pace` (`send_with`), not `thread::sleep`, or cancel can't interrupt it.

### `require`

- **With `--script <path>`** (the loader always passes it; a Windows path under Wine): `require("./x")` / `require("../x")` reads `x.luau`, `x.lua` or `x/init.luau` from disk, relative to the requiring file. Paths must start with `./` or `../`; modules are cached by normalized path. New lib files need no rebuild.
- **Otherwise** (piped script, or a bare name like `require("game_state")`): the lib compiled into the injector (`LIB_MODULES`, `include_str!` from the sibling `lib/` project, default `embedded-lib` feature). It is stale until a rebuild, new lib files must be added to `LIB_MODULES`, and `VOLCREN_LIB_DIR` (a `lib/` directory; under Wine a Windows path like `Z:\home\me\VOLCREN\lib`) reads those modules from disk instead. `--no-default-features` drops the embedded copy, so the build no longer reads `lib/`; the release workflow builds that way, since the loader always passes `--script`.

### Item change callbacks

`on_item_change(fn) -> id`, `off_item_change(id)` and `wait_item_change(ms)` (typed in the lib's `host.luau`) register Luau callbacks for the item log. The first callback makes an `ItemWatcher` (a subscriber of the log, see [ipc-protocol.md](ipc-protocol.md)). The script is one thread running Luau, so a callback can't interrupt it: `sleep` (replaced while a callback is registered) and `wait_item_change` block on the watcher's event instead of a timer, so they wake when the agent logs a change, run the callbacks for what was logged and go on sleeping for the rest of the time. A script that never sleeps never runs its callbacks. An error in a callback is an error in `sleep`.

### Adding a global

1. Implement it in the injector and bind it in `sandbox.rs`.
2. Add the `offline` stub for non-Windows.
3. Add the type and wrapper to the Luau lib's `host.luau` and document it there (separate project).

## Gotchas

- Shared-state reads race with the agent: go through the ipc sequence-locked helpers.
- The agent tells synthetic input from real by the `SYNTHETIC` flag; post input only through `input.rs`.
