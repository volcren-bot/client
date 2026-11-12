# agent (`src/agent`)

`agent.dll`, a `cdylib` injected into the game client; the only component that touches client memory directly. Windows x64 only. Offsets and client internals: [reverse-engineering.md](reverse-engineering.md).

## Lifecycle

- **Load:** the injector copies the DLL to `%TEMP%\agent\` under a versioned name, so a new build can replace a loaded one, then `LoadLibraryW`s it. Only `DllMain` is exported; everything else goes through shared memory, named events and window messages.
- **Start** (`hooks::start`, own thread): find the `JagRenderView` window, run every offset rule, install `retour` detours, patch `GetFocus` (below).
- **Per frame** (swap hook): publish state; capture pixels only when a reader set `capture_enabled` (only `injector screenshot`; Luau never reads pixels); draw the overlay, re-uploaded only when its serial changes. Capture and overlay are skipped while the viewport exceeds `IMAGE_DATA_SIZE`. Rule results go to the header's `offsets_state`, shown by the loader.
- **Commands:** custom `ipc::input::*_MESSAGE` window messages handled by the WndProc hook. Worker requests (polled decoders) are signalled on `AGENT_WORK_<pid>` and answered by `client/worker.rs`.
- **Unload:** named event `AGENT_UNLOAD_<pid>` (works with the WndProc hook off): unhook, drain in-flight hook calls, restore `GetFocus`, close mappings, `FreeLibraryAndExitThread` (`client/unload.rs`).

## Input

- Real mouse input is dropped for `REAL_MOUSE_BLOCK_MS` after synthetic input (`SYNTHETIC` flag).
- The client drops keys unless `GetFocus` returns its window, so `client/focus.rs` patches the main module's `USER32!GetFocus` import slot to return the render view. Only the client's own calls are affected. The WndProc hook also swallows `WM_KILLFOCUS`.

## Event logs

`packets.rs`, `script_runs.rs`, `varc_log.rs`, `item_log.rs` detour client functions (rules in `offsets/`) and append to shared rings `AGENT_PACKETS_`/`SCRIPTS_`/`VARCS_`/`ITEMS_<pid>`; `log.rs` holds the shared parts. Format and reader wake-ups: [ipc-protocol.md](ipc-protocol.md).

- Hooks run on the game thread (packet send: network thread), copy only what they need and return fast.
- `packets.rs` checks the local player's server tile before each packet receive and publishes changes, so no tick's move is missed however slow a script is.
- Script runs log the tick read where the game thread writes it; scripts the client starts without an event aren't logged.
- Varc and item writes that leave the value unchanged aren't logged.

## Layout

- `shared/`: DLL entry, shared-memory manager, unload/worker events, window lookup.
- `client/hooks.rs`: hooks, per-frame work, messages. `client/offsets/`: one rule module per feature. `client/own.rs`: in-process `Memory`, global `Layout`, game-thread tracking. `client/graphics.rs`: capture and overlay. Other `client/*.rs`: readers over rule results.

## Gotchas

- A panic in a hook kills the client: wrap hook bodies in `catch_unwind`.
- Save and restore OpenGL state around overlay drawing (`graphics::restore_state`).
- Anything in shared memory is protocol: change it in `src/ipc` and bump `PROTOCOL_VERSION`.
