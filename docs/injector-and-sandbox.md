# injector (`src/injector`)

Library and CLI for everything outside the client: find clients, inject the agent, read what it publishes, send input, run Luau scripts. Script-facing globals are documented in `lib/docs/host.md`; this doc covers the Rust side.

- Real code only on Windows. Elsewhere `sandbox.rs`'s `offline` module stubs every call with "clients can only be reached from Windows or Wine", so the loader builds natively. On Linux the real work is `injector.exe` under Wine.
- Pure logic (`async_input`, `nav`, `prices`, `bank_pin`, `devtools`, `form`, `http`, `antiban`) is kept free of Win32 so it is tested natively; Win32 data gathering lives in `win.rs`.

## Map

| File                                       | Holds                                                                                          |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| `lib.rs`                                   | Public API, data types, `HOST_API`, re-exports of `src/decode` (shared with the agent)          |
| `win.rs`                                   | All Win32: clients, inject/unload, shared memory, sessions, worker reads, overlays, cache reads |
| `sandbox.rs`                               | Luau runtime (`mlua`, `luau-jit`), every global, `require`, callbacks, `offline` stubs          |
| `main.rs`                                  | CLI and the `run` stdin/stdout protocol with the loader                                         |
| `input.rs`, `wind_mouse.rs`                | Synthetic input via `PostMessageW`; WindMouse paths                                             |
| `async_input.rs`, `input_jobs.rs`          | Per-run input `Worker`, `Pace`, held button/key tracking                                        |
| `cache.rs`, `collision.rs`, `pathfinding.rs` | Cache collision maps, `find_path`, the game's own walk-click router (`game_route`)            |
| `scene_collision.rs`, `scene_graph.rs`     | Live scene collision; randomized `scene_route` lattice                                          |
| `nav.rs`, `landmass.rs`, `transports.rs`   | `nav_plan` planner, world landmasses/`world_route`, transport rows                              |
| `projection.rs`                            | World to screen                                                                                 |
| `config.rs`, `font.rs`                     | Cache config definitions; bitmap fonts and sprites                                              |
| `form.rs`                                  | Settings form spec/validation and all `\u{1e}` line prefixes shared with the loader             |
| `bank_pin.rs`, `bank_pin_overlay.rs`, `lamp.rs` | PIN entry and lamp skill lookups by account-name hash                                      |
| `http.rs`, `prices.rs`                     | `http_request` wire format; `item_prices` cache                                                 |
| `antiban.rs`, `biometrics.rs`              | Loader-owned break/sleep config; per-account biometrics profile                                 |
| `entry_swaps.rs`                           | Parses the client's menu entry swap files                                                      |
| `devtools.rs`                              | Pure logic for the loader's dev overlays                                                        |

## Mechanisms and why

- **Session cache** (`win.rs`, `SESSION_TTL` 500 ms): per-pid process handle, mapped state, events, write view and request mutexes, so frequent reads don't reopen and remap. `forget(pid)` on load/unload, protocol mismatch or request timeout. `load` reads the header without the session so the process never holds the old agent's mappings across a reload.
- **Worker reads**: pure memory reads first go through the agent's worker thread (`ask_worker`, postcard answer; see [ipc-protocol.md](ipc-protocol.md)) and fall back to the same `decode::worker` code over `ReadProcessMemory`. In-process is ~25x faster (`npcs` ~0.09 ms vs 2.4 ms). The first call after an agent reload may fall back. Remote decoders minimise syscalls: one read per object (`decode::memory::Snapshot`), one table open for many lookups (`ipc::read::Table`).
- **Item names** (`ITEM_NAMES`): cached per process since a name never changes while the client runs.
- **World collision file**: closed-door bits (`flag::DOOR_*`) are stored in it; bump `WORLD_FORMAT` in `win.rs` whenever the bits or format change, or stale caches are reused.
- **Landmasses**: tiles above ground with no floor are excluded, or the open sky would join every landmass.
- **`nav.rs` underground**: dungeons (6400 tiles north) are folded onto four extra planes of the same box so one plan can cross surface and underground.
- **`scene_route`**: drops the scene's 4 outermost tiles (they miss objects) and fills past the scene from recent scenes and the cache's world collision.
- **Async input**: one `Worker` per `run`. Blocking input, `input_cancel` and live menu reads (`menu`, `menu_open`, `menu_layout`) cancel first. Cancel drops the queue, interrupts sleeps and returns only when idle, so nothing posts after it; held buttons/keys (`input_jobs::Held`) are released last-pressed-first on cancel and shutdown.
  - Invariant: new input code must sleep through `Pace` (`input::send_with`), never `thread::sleep`, or cancel can't interrupt it.
- **Callbacks**: the script is single-threaded Luau, so item/packet callbacks run only inside `sleep`, `wait_item_change` and `wait_packet`, which block on the watchers' events (`wait_watchers`) instead of a timer. A script that never sleeps never runs callbacks; a callback error surfaces from `sleep`. The `PacketWatcher` exists only while something wants packets, filtered to the union of requested opcodes (`Callbacks::refresh_packets`). The packet ring holds only a few seconds of busy traffic.
- **`movement()`** reads the agent's `Movement` block, filled before each packet, so it is exact across ticks a busy script missed; tick callbacks can't give that.
- **Entry swaps**: the account folder is found by duplicating client handles (up to `HANDLE_SCAN_LIMIT`) until one is a file under `users\<digits>` below a drive-root profile folder, so a numeric Windows user name doesn't match. Fails while logged out.

## Sandbox boundary

- `run_with_modules` opens only `TABLE | STRING | MATH | BIT`: no `os`, `io` or `debug`. `dofile` and `loadfile` are removed; `require` is the sandboxed one.
- Scripts reach the world only through globals. They never see a PIN: `bank_pin_enter` asks the loader by account-name hash (`PINREQ`) and clicks the keypad itself. Lamp skills use the same lookup (`LAMPREQ`).
- No network from the injector. `http_request` and `item_prices` write `HTTPREQ` lines; the loader enforces the user's host whitelist (prices URL always allowed). Limits enforced in `http::Request::validate`: body 1 MiB, response 8 MiB, timeout 1 to 60000 ms (15 s default).
- Untrusted input validated: script form specs (`form::validate`, incl. `required`/`locked` rules), HTTP requests (`http::Request::validate`), loader replies (`form::parse_reply`, `parse_pin_reply`, `parse_lamp_reply`, `http::parse_reply`), antiban configs (`antiban::Config::validate`, invalid ones dropped), skips, transports and settings lines (unparseable lines ignored). `report_stats` rejects negative experience.
- `run <pid>` exits at startup if the agent is missing or on another protocol, so scripts never see a nil connection; `connection`/`game_state` raise rather than return errors.
- Input must go through `input.rs`: it sets `ipc::input::SYNTHETIC`. The agent strips the flag and drops real mouse messages for `REAL_MOUSE_BLOCK_MS` after synthetic ones.
- Shared-state reads race with the agent: use the ipc sequence-locked helpers.

## `run` protocol (`main.rs::run_script`)

Usage: `injector run [--script <path> [--lib <dir>]] [pid]`. Lines are `\u{1e}`-prefixed (constants in `form.rs`, `http.rs`, `prices.rs`, `antiban.rs`, `transports.rs`); stdout entries end with `form::ENTRY_END` (`\0`).

- Header lines before the script, any order: `PINS` (enables PIN and lamp lookups), `HTTP` (enables the HTTP handler; otherwise "HTTP requests aren't available"), `ANTIBAN`, `PRICES <url>`, `TRANSPORT_SETTINGS`, `TRANSPORTS`.
- Then optional `MODULES <len>` + JSON `{path: source}` block (the decrypted store files, served from memory relative to the script dir), then `SCRIPT <len>` + source. Without `SCRIPT`, all of stdin is the source and there is no form handler (hand runs).
- After the script, a stdin thread applies `ANTIBAN`/`PRICES`/`TRANSPORT_SETTINGS`, `SKIP`, `UPDATE` (form updates); any other line is the reply to the pending request (`ask`).

## `require`

- `./x`, `../x`: relative to the requiring file; tries `x.luau`, `x.lua`, `x/init.luau`, from `MODULES` memory first, then disk. Cached by normalized path.
- `@lib/<path>`: lib root is `--lib`, else `../lib` beside the script. From the script itself, `../lib/...` paths resolve in `--lib` too.
- Bare names, or no `--script`: the embedded lib (`LIB_MODULES`, `include_str!` from sibling `lib/`, `embedded-lib` feature, default on). Stale until rebuild; new lib files must be added to `LIB_MODULES`; `VOLCREN_LIB_DIR` overrides from disk. Release builds use `--no-default-features` (no embedded lib), since the loader always passes `--script` (and `--lib` when it has one, `loader/src/backend/mod.rs`).

## Cross-repo contracts

- **`HOST_API`** (`lib.rs`, now 19): bump when adding or changing a sandbox global. Pair with `lib/HOST_API`, bumped in the lib commit that starts using it; `lib/tools/check-host-api.sh` fails the lib release when `host.luau` gains a global without a bump.
- **Lib selection** (`loader/src/libs.rs::choose`): the loader takes the newest lib release whose `requires_host <= injector::HOST_API`, warning when a newer one was skipped. A missed bump pairs a lib with an injector that lacks the global.
- **`PROTOCOL_VERSION`** (`ipc`): any IPC layout change bumps it and updates agent, injector and `lib/host.luau` together. `Connection::problem` reports a mismatch.
- **Adding a global**: implement and bind in `sandbox.rs`, add the `offline` stub, bump `HOST_API`, add type/wrapper to `lib/host.luau` and document it in `lib/docs/host.md`.
- Loader line formats (`form.rs`, `http.rs`, `prices.rs`, `antiban.rs`) are shared code with `src/loader`; change both sides together. See [loader-gui.md](loader-gui.md).

## CLI

The full command list is the match in `src/injector/src/main.rs` (plus `Input::from_args`, `Overlay::from_args`). Every command prints JSON `Result<T, String>` except `run`. `serve` is the JSON-lines server the loader uses as its Wine backend.
