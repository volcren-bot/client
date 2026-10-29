# agent (`src/agent`)

`agent.dll`, a `cdylib` injected into the game client. The only component that touches the client's memory directly. Windows-only, cross-compiled for `x86_64-pc-windows-gnu`, x64 only.

How offsets are found, and what is known about the client's internals: [reverse-engineering.md](reverse-engineering.md).

## Lifecycle

1. **Load.** The injector copies the DLL to `%TEMP%\agent\<name>-<version>.dll` (so a new build can replace a loaded one) and injects it (`injector load <pid> <dll>`, or the loader's Load/Reload).
2. **Start.** `client::hooks::start` runs on a new thread: find the client window (`JagRenderView`), run every `find_*` offset rule, install the hooks (`retour` detours).
3. **Every frame** (OpenGL swap hook): read game state into `AGENT_STATE_<pid>`, capture the frame if a reader turned `capture_enabled` on (only Simba reads pixels; Luau never does), draw the overlay (re-uploaded only when its serial changes, from the slot the serial selects, except Simba's), then after the real swap bump `frame_serial` and set `AGENT_FRAME_<pid>`. Rule results are logged to the client console and summarised in the header's `offsets_state`, which the loader shows.
4. **Commands** arrive as custom window messages (`ipc::input::*_MESSAGE`) on the window-procedure hook: open console, set modifiers, set display name, offset total XP, resolve item/game-val names, project world points, read the menu, models, locs and the cache. A locs request names a plane, or all of them, optionally only the clickable locs (`ipc::state::locs_request`); each loc carries its plane in its flags, and walls and decorations their shape and rotation from their config byte (`offsets::scene::find_loc_configs`, Java's `Scene.getConfig`).
5. **Unload** on the named event `AGENT_UNLOAD_<pid>` (works with the window hook off): unhook, wait for hook calls to drain, stop the listener, close mappings, `FreeLibraryAndExitThread`.

Input: real mouse input is blocked for `REAL_MOUSE_BLOCK_MS` after synthetic input (told apart by the `SYNTHETIC` flag), and input can be toggled.

The DLL is also a [Simba](https://github.com/Villavu/Simba) plugin: `src/simba/` exports the Pascal tables (`GetFunctionInfo`, `GetTypeInfo`, ...) and `Inject`, `OpenConsole`, `GetInputState`, `SetInputState`.

## Layout

| Path | Contents |
| --- | --- |
| `src/lib.rs` | DLL exports and the Simba export tables. |
| `src/shared/` | `main.rs` DLL entry, `memory.rs` shared-memory manager, `sync.rs` unload event, `windows.rs` window and input helpers (`WI_*` message ids, `JagRenderView` lookup). |
| `src/client/hooks.rs` | Hook installation, per-frame work, message handling. |
| `src/client/offsets.rs`, `offsets/*.rs` | Offset rules, one module per feature. |
| `src/client/own.rs` | In-process `Memory` impl, the global `Layout`, game-thread tracking. |
| `src/client/packets.rs` | The packet log: creates `AGENT_PACKETS_<pid>` and detours the client's packet receive and send functions (rule in `offsets/packets.rs`). The hooks run on the game thread (receive) and the network thread (send), copy at most 104 payload bytes and return. |
| `src/client/script_runs.rs` | The script log: creates `AGENT_SCRIPTS_<pid>` and detours the client's event script runner (rule in `offsets/script_runs.rs`; see "CS2 interpreter" in `reverse-engineering.md`). The hook runs on the game thread, reads the event's type and first arguments and calls through; a thread local counts nesting. Scripts started by the client without an event (by trigger lookup) aren't logged. |
| `src/client/varc_log.rs` | The varc log: creates `AGENT_VARCS_<pid>` and detours the client's int and object (string) varc setters (rule in `offsets/varc_setters.rs`). An int's old value comes from the client's own getter before the call; a string's from what the hook last saw it set to. A set to the same value isn't logged. |
| `src/client/*.rs` | Readers that use the rules' results (`state`, `scene`, `model`, `menu`, `items`, `cache`, `projection`, ...); `graphics.rs` does frame capture and overlay drawing. |
| `src/simba/` | Simba plugin glue and per-pid input targets. |

## Gotchas

- A panic in a hook takes the client down: wrap hook bodies in `catch_unwind`.
- Save and restore OpenGL state around overlay drawing (`graphics.rs`, `restore_state`).
- Anything written to shared memory is part of the protocol: change it in `src/ipc` and bump `PROTOCOL_VERSION` ([ipc-protocol.md](ipc-protocol.md)).
