# loader (`src/loader`)

The user-facing desktop app (`eframe`/egui), a native binary. It never touches the client itself: a new capability goes in the injector (and usually the agent) first, then gets surfaced here.

## What it does

- Lists game clients; loads, reloads and unloads the agent; shows agent version, protocol, offsets state and a protocol mismatch warning.
- Runs each script as its own `injector run --script <path> <pid>` process, so a crashing script can't take the app down. Streams output, pauses, stops (escalating if ignored), renders script forms (`FormSpec`) and remembers their values. Opens `default.luau` from a `scripts/` folder next to the executable (or the sibling scripts project) on start.
- Requires a Discord sign-in to start: until signed in the window shows only a sign-in screen (and the log). Sign-in is PKCE over a loopback redirect to `127.0.0.1:5217`; the refresh token lives in the OS keyring, never in eframe storage. The header shows the account, balance and "Buy credits".
- Scripts tab (`Tab::Store`): lists the scripts the account can see, with price, online count and a disabled Run (with the reason on hover) when there's no access, not enough credits, no loaded client, or another script running. Run starts a session, downloads the live revision to the cache, and runs `main.luau` through the normal script path.
- Community scripts (written by someone below scripter) are tagged `community` in the list, official ones `official`, and scripts that came from a share code say "shared with you". Testers and above get a Community checkbox that lists every community script (`GET /launcher/scripts?community=true`); they stay out of the default list. The redeem box takes a share code (checked locally as 64 hex digits), calls `POST /launcher/scripts/redeem` and refreshes the list; the server's message says if the code was invalid, expired or used. A granted script has "Remove from my account" (`DELETE /launcher/scripts/:id/grant`, two clicks). `API_VERSION` is 2: a loader that predates community scripts is told to update instead of running them without the warning.
- Before the first run of a community script, and again for every new revision of it, the loader shows a warning window. "Run anyway" is remembered per script and revision in the loader's local storage (`store::Confirmed`, key `community_confirmed`, one revision per script, at most 500 scripts). `session::prepare` refuses a community script whose started revision isn't the one that was confirmed, and ends that session, so a revision published after the warning never runs unseen. A failed start refreshes the list.
- While a store script runs the loader sends a heartbeat every `heartbeat_seconds` (billing, online count, stats from `report_stats`). If the server says the session is over, the script is stopped and the reason shown; if no heartbeat succeeds for 4 minutes the script is stopped before the server's 5 minute timeout. The session is ended when the script stops, finishes or crashes, and on exit (with a 3 second limit; the server's sweep covers the rest). Pausing a store script does not pause billing.
- On start it fetches `latest.json` from the public `client` bucket and shows a Download/Dismiss banner when a newer version exists (`src/update.rs`). Releases are built and published by `.github/workflows/release.yml` on every push to `main`. Versions are `YYYY.MM.DD-<7 character commit>` (UTC commit date), embedded by `build.rs`; local builds use their checked out commit, or `dev` outside git, which never prompts an update. Store scripts run against the shared `lib/` in the cache (`src/libs.rs`): the newest lib in `lib/index.json` whose `requires_host` is at most the loader's `injector::HOST_API`. If a newer one exists that this loader cannot run, the script log warns to update the loader. Settings detection looks next to the executable first, so an installed release needs no `target/` folder.
- Navigation is a neo-tree style sidebar like the webapp's (`sidebar_ui` in `app.rs`, row painting in `theme.rs`): Client, Scripts, Editor, Development, Settings. It is always shown when the window is at least `SIDEBAR_MIN_WINDOW` wide; below that the header logo toggles it as a drawer (closed by picking a page, a click outside, or Esc).
- Edits scripts with Luau syntax highlighting.
- On Run, compares the file on disk with what the editor last synced (`ScriptFile::disk`: set on open, save, startup) and prompts to reload if it changed; the prompt also warns about unsaved edits.
- Dev tools: RuneLite-style checkboxes (`DEV_TOGGLES` in `app.rs`) that draw scene layers through `injector::scene_tiles_next_frame`, once per frame the client presents (`TilesWorker`); a checkbox with no entry in `DEV_LAYERS` isn't ported yet.
- Persists `Settings { dll, helper }` (paths of `agent.dll` and `injector.exe`), auto-detected next to the executable or in `target/x86_64-pc-windows-gnu/{release,debug}`. Also persists the mouse cursor preference (`Cursor`, storage key `cursor`), which is passed to `injector load` so the agent starts with it (the injector leaves it in an `AGENT_CURSOR_<pid>` mapping the agent reads while building its header).

## Layout

| Path | Contents |
| --- | --- |
| `src/app.rs` | The whole UI and its state. |
| `src/script.rs` | Script processes: spawn, stdio pumping, pause, stop, `ScriptEvent` (`Output`, `Form`, `Stats`, `Finished`). |
| `src/form.rs` | egui rendering of a form spec. |
| `src/api/` | The launcher API: `config.rs` (embedded endpoints), `auth.rs` (PKCE, loopback listener, token exchange/refresh, keyring), `client.rs` (`Api`: blocking calls, 401 refresh-and-retry), `models.rs`, `error.rs` (`ApiError`). Everything blocks, so `app.rs` runs it through `api_job` threads. |
| `src/account.rs` | `Auth` state and the sign-in round trip. |
| `src/store.rs` | The Scripts tab: list, filters, redeem box, community tags, price and balance text, and `Confirmed`, the community warnings the user accepted. |
| `src/update.rs` | Update check against `latest.json` and the version comparison. |
| `src/session.rs` | Starting a store run: cache layout (`<cache>/scripts/<script-id>/<revision>/`), download with name/checksum checks, pruning; and `Beat`, the heartbeat state machine (retries with backoff, give-up, stats pending). |
| `src/highlight.rs` | Luau highlighter. |
| `src/backend/mod.rs` | `Settings`, `Target`, `Scan`; re-exports one function set (`connection`, `load`, `state`, `input`, `draw`, ...) from the compiled backend, so `app.rs` is platform independent. |
| `src/backend/native.rs` | Windows: calls the `injector` library directly. |
| `src/backend/wine.rs` | Linux: finds each client's Wine prefix (by the client executable's name, `CLIENT_EXES`), runs one `injector.exe serve` per prefix and talks JSON lines over stdio (`HELPER_TIMEOUT` 45 s). A helper answers one request at a time, so requests that wait for the client's next frame (the dev overlays) go to a second helper of their own (`Lane::Frames`). |

## Gotchas

- The loader depends on the `injector` crate, so the injector's non-Windows stubs must keep compiling.
- On Linux, anything that needs Win32 goes through the Wine helper protocol.
