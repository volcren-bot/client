# loader (`src/loader`)

The user-facing desktop app (`eframe`/egui), a native binary. It never touches the client itself: a new capability goes in the injector (and usually the agent) first, then gets surfaced here.

## What it does

- Lists game clients; loads, reloads and unloads the agent; shows agent version, protocol, offsets state and a protocol mismatch warning.
- Runs each script as its own `injector run --script <path> <pid>` process, so a crashing script can't take the app down. Streams output, pauses, stops (escalating if ignored), renders script forms (`FormSpec`) and remembers their values. Opens `default.luau` from a `scripts/` folder next to the executable (or the sibling scripts project) on start.
- Edits scripts with Luau syntax highlighting.
- On Run, compares the file on disk with what the editor last synced (`ScriptFile::disk`: set on open, save, startup) and prompts to reload if it changed; the prompt also warns about unsaved edits.
- Dev tools: RuneLite-style checkboxes (`DEV_TOGGLES` in `app.rs`) that draw scene layers through `injector::scene_tiles`; a checkbox with no entry in `DEV_LAYERS` isn't ported yet.
- Persists `Settings { dll, helper }` (paths of `agent.dll` and `injector.exe`), auto-detected next to the executable or in `target/x86_64-pc-windows-gnu/{release,debug}`. Also persists the mouse cursor preference (`Cursor`, storage key `cursor`), which is passed to `injector load` so the agent starts with it (the injector leaves it in an `AGENT_CURSOR_<pid>` mapping the agent reads while building its header).

## Layout

| Path | Contents |
| --- | --- |
| `src/app.rs` | The whole UI and its state. |
| `src/script.rs` | Script processes: spawn, stdio pumping, pause, stop, `ScriptEvent` (`Output`, `Form`, `Finished`). |
| `src/form.rs` | egui rendering of a form spec. |
| `src/highlight.rs` | Luau highlighter. |
| `src/backend/mod.rs` | `Settings`, `Target`, `Scan`; re-exports one function set (`connection`, `load`, `state`, `input`, `draw`, ...) from the compiled backend, so `app.rs` is platform independent. |
| `src/backend/native.rs` | Windows: calls the `injector` library directly. |
| `src/backend/wine.rs` | Linux: finds each client's Wine prefix (by the client executable's name, `CLIENT_EXES`), runs one `injector.exe serve` per prefix and talks JSON lines over stdio (`HELPER_TIMEOUT` 45 s). |

## Gotchas

- The loader depends on the `injector` crate, so the injector's non-Windows stubs must keep compiling.
- On Linux, anything that needs Win32 goes through the Wine helper protocol.
