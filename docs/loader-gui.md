# loader (`src/loader`)

The desktop app (GPUI with gpui-component, through `gpui-kit`). It never touches the client itself: a new capability goes into the injector (and usually the agent) first, then gets surfaced here. Releases, signing and the installer are in [releases.md](releases.md).

## Map

| Path | Holds |
| --- | --- |
| `app.rs` | `LoaderApp`: app state, events, jobs, run/session logic, `tick` (the per-frame logic), `save`/`on_exit` |
| `app/chrome.rs` | Window, `Render`, the wake loop, header, sidebar tree and drawer, sign-in gate, update banner/screen, page dispatch, overlay dialogs |
| `app/nav.rs` | Which page belongs to which client, client state (colour), page repair, the per-client editor and inspector swaps |
| `app/<page>.rs` | One module per page (`clients` the client page, `store_page`, `sessions` the overview and a client's history, `run_page`, `editor` + `editor/code.rs`, `development`, `terminal_page`, `accounts_page`, `feedback_page`, `settings_page`, `queue_page`, `output`, `dialogs`), each with its widget state in one `Ui` field on `LoaderApp`; `shared.rs` holds what several pages use |
| `app/snapshots.rs` | Offscreen page snapshots (test only), see below |
| `ui/` | `theme.rs` palette, sizes, fonts, gpui-component theme; `widgets.rs` and `fields.rs` shared widgets |
| `storage.rs`, `wake.rs` | `app.ron` key/value store; `Wake`, the handle background threads use to wake the window |
| `script.rs` | Script process: spawn, stdio pumping, pause/stop, `ScriptEvent`; `Process` (session thread's handle); `Host` (answers PIN, lamp, HTTP off the UI) |
| `session.rs` | Store run start (cache, in-memory download, decrypt), `Beat` heartbeat state machine, `keep`, quotes/restart |
| `store.rs` | Scripts page, prices, community tags, `Confirmed` |
| `api/` | `config.rs` endpoints, `auth.rs` PKCE + keyring, `client.rs` `Api` (blocking, 401 refresh-and-retry), `models.rs` (credits formatting, `API_VERSION`) |
| `account.rs` | `Auth` state, sign-in round trip |
| `accounts.rs` | Accounts page and sealed `accounts.bin` |
| `feedback.rs` | Feedback page/tab, diagnostics, name scrubbing, sealed run records |
| `layouts.rs` | Equipment/inventory/prayer layouts, rules, picker |
| `breaks.rs`, `xp.rs`, `counters.rs` | Break/sleep editors, run trackers |
| `hider.rs` | Entity hider categories and the client page's poll state |
| `proxies.rs` | Saved proxies (settings `network/`, sealed `proxies.bin`) and each client's chosen proxy |
| `form.rs` | Form spec rendering |
| `queue.rs`, `app/queue_page.rs` | Queue model and pure rules (when an entry ends, what follows an exit, break carry-over); Queue page, start checks and the per-client runner |
| `http.rs`, `whitelist.rs` | Script HTTP requests, whitelist |
| `transports.rs` | Transport data versions |
| `sdk.rs` | The SDK built into the loader, unpacked for docs, checks and the workspace `.luaurc`, see [sdk.md](sdk.md) |
| `injectors.rs` | Pinned injectors: downloads and verifies the release a store revision runs on |
| `update.rs`, `update/` | Signed manifest, `archive.rs` (Linux), `msi.rs` + `msi_command.rs` (Windows) |
| `vars.rs`, `script_log.rs`, `script_names.rs`, `inventory.rs`, `widgets.rs`, `packets.rs`, `packet_defs.rs` | Development inspectors |
| `icons.rs` | Icon store |
| `terminal.rs` | Terminal page (`portable-pty` + `alacritty_terminal`), started in the agent workspace |
| `mcp.rs`, `mcp/`, `workspace.rs` | `loader mcp` server and the agent workspace, see [mcp.md](mcp.md) |
| `agent_runs.rs` | Run records `loader mcp` writes and the window reads, see Agent runs |
| `log_view.rs`, `pipe.rs` | Output tab buffer; append-to-file pipe |
| `highlight.rs`, `fold.rs` | Luau highlighting, folding |
| `backend/` | `mod.rs` `Settings` + platform-neutral facade; `native.rs` (Windows, calls `injector` lib); `wine.rs` (Linux helper) |
| `paths.rs` | `ProjectDirs` `com.volcren.Volcren`, user agent |
| `fake_api.rs`, `flow_tests.rs` | Test API and flow tests |

## Threads and frame independence

- The window draws no frames while minimised or on a hidden workspace, so nothing billing-critical runs in `tick`.
- `LoaderApp::tick` (events, script events, queues, xp, inspector polls, the 30 s autosave) runs at the start of every render, as egui's frame did, so a click that changes what a poll watches acts on the next frame. Threads call `Wake::wake` (render now) or `wake_after` (render by then); the wake loop in `chrome.rs` keeps only the earliest deadline, and every tick asks again for what it still needs.
- Each store run has its own thread (`session::keep`): heartbeat every `heartbeat_seconds` (clamped 10 to 120), stats from `report_stats`, stop on server-ended session, stop after `GIVE_UP_AFTER` (4 min) without a successful beat, before the server's 5 min timeout. Ends the session when the script exits.
- PIN, lamp and `http_request` are answered on the script's output thread (`script::Host`), since a script blocks until answered. Nothing a script asks for waits for the window: settings are fixed before it starts.
- All `Api` calls block and run on `api_job` threads.
- Pausing a store script does not pause billing.
- Sign-out (button or server-revoked) runs `App::stop_all` first: stops scripts and ends sessions, with a 3 s limit; the server's sweep covers the rest. Same on exit.

## Sign-in and storage

- Discord via Supabase, PKCE; listener on `127.0.0.1:5217`, redirect URL `http://localhost:5217/`. Login times out after 5 min.
- Keyring service `volcren-loader`: `refresh-token` (sign-in, shared with `loader mcp`, see [mcp.md](mcp.md)), `accounts-key` (32-byte AES-256-GCM key for `accounts.bin`, `proxies.bin` and run records). Never in `app.ron`.
- `app.ron` (`storage.rs`) is the file eframe wrote, kept so installs keep their data: in the data dir of app id `Loader` (`~/.local/share/loader`, `%APPDATA%\Loader\data`), a RON map of RON-encoded values. Keys: `app` (`Settings`), `script`, `last_script`, `forms`, `queues`, `layouts`, `cursor`, `ui_font`, `community_confirmed`, `http_whitelist`. Keys it doesn't know (eframe's `egui`, `window`) are kept. Written every 30 s and on exit, through a temporary file and a rename.
- `accounts.bin` (data dir) is `nonce || ciphertext || tag`. If it can't be opened (no keyring, wrong key) the page refuses to save so it isn't overwritten. Only `hash_name` of the account name is kept, normalised like the client (lowercase, spaces/hyphens to `_`).
- `layouts`: if unreadable (written by a newer loader) it is copied to `layouts-unreadable-<unix>.ron` and never saved over. Two open loaders still overwrite each other's storage.
- Endpoints compiled from `src/loader/config.json`; `API_URL`, `SUPABASE_URL`, `SUPABASE_ANON_KEY`, `SITE_URL`, `PRICES_URL` env vars override at run time. No `.env`.

## Store runs and billing

- `API_VERSION` = 3; the server tells an older loader to update.
- Credits: API counts microcredits (1,000,000 per CREDIT); `price` is microcredits per hour.
- Run asks `GET /launcher/scripts/:id/quote` first: only the server knows author/grantee/tester pricing (0) and the account's open sessions. The quote carries the current balance, so website top-ups count. A quoted fee means another session is open; the loader confirms before starting.
- The listing's `can_access` decides whether Run is enabled. `StoreScript::blocked`/`short_of_funds` only word the refusal, using the server's rounding `ceil(min_balance_minutes * price / 60)` without the fee. The server's `min_balance_minutes` decides.
- Restart (`App::restart_run`) ends the old session again (idempotent), re-reads the listing (community list only for tester+), the quote and, unless its live revision is cached, the manifest; `session::asks_first` prompts when the quoted price changed or a fee applies. The old run closes only once the new one starts.
- A run keeps its settings form after it ends, and Restart (store or editor) starts with what that form shows. If the manifest's settings spec differs from the form's, Restart opens a configuring run instead, so a script never gets values made for another spec.
- Store files are AES-256-GCM encrypted (`session::decrypt_file`; `key: null` means plain, pre-encryption revisions), downloaded on every start, never written to disk, sent to the injector over stdin (`MODULES`). The revision cache folder is empty and only anchors relative `require`.
- Community scripts: the warning's acceptance is stored per script and revision (`Confirmed`, max 500). `session::prepare` refuses and ends a session whose started revision isn't the confirmed one, so a revision published after the warning never runs unseen.
- One script per client, enforced across processes by a lock file (see [mcp.md](mcp.md)). A store start in progress (quoting, dialog, downloading) holds its client.
- Store scripts run on the SDK built into the injector; the loader passes no `--sdk` for them, so the settings' SDK folder never reaches a paid run.
- **Pinned injectors.** Each live revision names the client release it runs on (`injector` in the quote and the started session, see `database/CLAUDE.md`). The quote resolves it before anything is billed (`injectors::pinned`): this loader's own release is the bundled `injector.exe`; any other is downloaded once to `<cache>/injectors/<version>/injector.exe` and used only if its signature is by an update key and its trusted comment is exactly that version, so one release's file can't stand in for another's. A quote without an injector (`start_block`, `session::UNSUPPORTED`) is refused. The queue's check resolves every entry's injector the same way.
- **Agent swaps.** An injector only speaks to its own agent, so `session::prepare` loads the pinned injector's agent before the session starts when the client has another (`swap_agent`), and again if the started session names a different release than the quote. The run then uses that `injector.exe` (`Settings::with_helper`). While an older agent is loaded, the loader's own reads of the client (XP tracker, bug-report snapshot, layouts, development tools) fail with the protocol mismatch; that is accepted. When a pinned run ends outside a queue, or a queue ends, the loader loads the current agent back (`load_current_agent_back`). A queue's logout runs on the last entry's injector, since its agent is still loaded.

## Update flow

- Fetches `latest.signed.json` from the public `client` bucket and verifies the signature before parsing. Prompts when its `build` (commit Unix time, `VOLCREN_BUILD` from `build.rs`) is newer; build 0 never prompts.
- Installs by itself (`update::installs_by_itself`) unless a run is active/starting/in a dialog, or the loader was restarted with `--update-failed`, so a failure can't loop. Checks once per start.
- Never installs while another loader runs: each holds a share of `loader.lock` in the data dir (`update::Instance`).
- Under a cargo `target/` dir it never installs (`Kind::Dev`); the header's version hover names the newer version.
- Linux: unpack into `<install>/.update/`, rename each file in `FILES` (`update/archive.rs`) in with hard links for rollback, start the new loader after this one closes (so `save()`/`on_exit` run). Unwritable install dir falls back to browser download.
- Windows: MSI to `<data>/update/`, sha256 checked; loader copies itself there and runs the copy with `--finish-update`, which waits, runs `msiexec /passive` with the install's own mode and folder (read from Windows Installer; upgrades don't cross modes), and restarts the installed loader unelevated, with `--update-failed <msg>` on failure (declined UAC counts). Non-MSI installs get browser download.
- `Settings::fill_missing` makes every loader use the `injector.exe` it detects over the saved path, so a saved path can't pair versions; a saved path is used only when nothing is detected. An install finds it next to the executable. A build under `target/` takes the newest of `target/x86_64-pc-windows-gnu/{its profile,fast,release,debug}`, so a `cargo fast-win` build is picked up on the next start.
- Versions: `YYYY.MM.DD-<7 char commit>`, or `dev` outside git.

## Navigation

- The sidebar is a tree rebuilt every frame from state: `sessions/` lists every detected client, plus any closed client that still has runs. Clients are scanned at start, after a load or unload, and every 5 s (`RESCAN_EVERY`). A scan never sets `busy`, which only disables the client and state-reading buttons while such a job runs; a scan error is logged only when it is new.
- Only `history/`, the runs under it and `settings/` fold: they show their children only when the current page is in them, so a long history doesn't fill the tree.
- `Page` names where the user is. Client pages carry the pid; a run page uses `active_run`. Nothing holds a global selected client: every action takes its client from its page, so the store, queue and editor start only on the client whose page started them.
- A client's children follow its state: idle and loaded shows `scripts/`, `queue/`, `editor/`; a live run (running or configuring) or a queue hides them, since the client lock allows one script per client; `development/` shows while the agent is loaded; ended runs sit under `history/`. A client's page has a button for each of its children, built from the same node. `repair_page` moves the user to the client page (or `sessions/`) when the page they are on stops being offered.
- Client colour: dimmed without a loaded agent, green while a script or queue runs, red when its latest ended run failed.
- The output panel shows the run of the page's client (the open run, else its live run, else its latest) next to the Loader log; outside a client it shows the Loader log.
- Each client has its own editor and inspectors. The one on screen lives in `LoaderApp::editor`/`dev` and the others are parked by owner (`show_editor`, `show_dev`), so page code works on one set. A client's editor starts as a copy of the global editor's file, and saving updates other editors of the same file that hold no unsaved edits; the rest get the reload prompt when they run. Only the global editor's file is saved to `app.ron`. Inspectors poll only for the client whose development page was opened last, so a capture keeps going while other pages are open; parked ones pause and their tiles worker stops.
- The global `editor/` only edits files. The accounts page reads the account name from the client opened last.
- `accounts/` lists each saved account as a child page with House and Transportation tabs, which save as soon as they change. Transport types are per account only: an account that isn't saved gets every type.

## Agent runs

- `loader mcp` appends each run to `<data>/agent-runs/<unix>-<process>-<run>.jsonl` (`agent_runs::Entry`: start, output lines, end). The window polls the folder every second and shows each record as a run under its client with `Run::agent` set: its log, state and trackers work as for its own runs, but the window can't pause, stop or restart it, since only the agent's process owns the script.
- A record without an end is live while its client's lock is held (`script::client_held`, a shared-lock probe); when the lock is free, the run is marked ended. A record whose client isn't detected yet is read again on the next poll.
- On start the window skips records that ended before it opened. Records are deleted after 24 h without writes.

## UI mechanisms

- GPUI keeps no hidden widget state for us: text inputs are entities, scroll positions handles, open sections flags. Each page keeps them in its `Ui` struct, created on first render; the model the logic reads (`Settings`, forms, the inspectors) stays the source of truth, and inputs are pushed from it when they aren't focused.
- Overlay dialogs (start prompt, community warning, reload prompt, queue prompt, accounts and whitelist dialogs) are absolutely positioned elements over a backdrop, not windows.
- Key events only travel the path to the focused element and the window root usually has focus, so page shortcuts (accounts Escape) use `cx.observe_keystrokes` gated on the tab, no modifiers and `window.has_focused_input`, which replaces egui's `wants_keyboard_input`. The Editor takes focus when nothing inside a page has it, so Ctrl+F/Ctrl+R/Escape reach it.
- `widgets::text_width` measures with the current UI font (`theme::current_ui_family`), not `window.text_style()`, which is GPUI's default outside layout. `widgets::icon_button_width` sizes a group from all its possible labels so buttons never resize.
- Output panel (`app/output.rs`): the log view is a `uniform_list` of wrapped rows, so only visible rows are drawn. Each entry is wrapped once; an append wraps only the new entries (`Log::appended` tells an append from a rebuild because a rebuild or clear raises `revision` by more than the entries added). A tab switch, filter change, trim or clear re-wraps the whole tab and drops the selection; a new width only re-wraps it.
- Output tabs are capped (`backend::LOG_LINES`, 2000 to 10 000), trimmed a quarter at a time. A trim or filter change rebuilds the tab's text and re-wraps it, and Copy all and Save copy the whole text.
- GPUI has no selectable text, so the log view selects by itself: a selection is two (entry, byte) spots, rows hit-test by shaping the one row under the pointer, Ctrl+C copies it. Dragging past the edge keeps scrolling (`LogView::step_past_edge`); speed is relative to room to the window edge since the output sits at the bottom.
- Dropdowns (`widgets::combo`, `fields::select`) are scrollable only past `MENU_SCROLLS_PAST` items. gpui-component scrolls a scrollable menu to its checked item when it opens, and that leaves a menu which already fits scrolled a few pixels, with a scrollbar.
- The Editor's text element (`app/editor/code.rs`) is the loader's own, not gpui-component's `Editor`: that one keeps its folds private (no way to fold from the app or a snapshot) and its gutter can't select a line. Folds live in `LoaderApp::fold` (`fold.rs`): the element draws the rows it is given and reports cursor moves and fold clicks back, so a fold opens when the cursor enters it. Tabs draw four columns wide, the width `Folds::longest_line` counts.
- The terminal paints its grid in one canvas, one shaped line per row with every glyph forced to the cell width; text arrives through an input handler so IME and dead keys work.
- Offscreen snapshots: `SNAPSHOT_DIR=<dir> [SNAPSHOT_ONLY=<prefix,…>] [SNAPSHOT_SIZE=900x640] cargo test -p loader app::snapshots::pages -- --ignored` renders each page state listed in the pages' `snapshots()` to a PNG (2x scale) with GPUI's headless WGPU renderer, without a window, a client or the API. Use it to check a page's look; Windows has no headless renderer.

## Scripts, settings and stdin contract

- Each script runs as `injector run --script <path> <pid>`, so a crash can't take the app down. On Windows it starts with `backend::no_window` (`CREATE_NO_WINDOW`): the loader is a GUI program outside debug builds, so a console child would otherwise open a terminal per run. Use it for any console program the loader starts.
- Lines written to the script's stdin before start: antiban config (`injector::antiban::line`), `ACCOUNTS` (every saved account's transport toggles and house pieces, keyed by name hash; `Accounts::settings_line`), Script debugging (`DEBUG`), `prices_url` (`injector::prices::line`), `SETTINGS` (the form's values, `script::Launch`) and, for a queue entry, `RESUME` (the previous entry's break plan). Live: `UPDATE` (settings changed while running, read by `Host.form_update()`), antiban push, `ACCOUNTS` push (whenever the saved accounts change: `Accounts::version`), Script debugging push, skip-next, `FINISH` (queue only).
- Settings come from the script's `manifest.json` (`injector::manifest`): the store's live revision through `GET /launcher/scripts/:id/manifest`, cached in memory by script and revision, or the file beside an editor script. A script with settings opens a **configuring** run (`Run::is_configuring`: no process, no session) whose Start quotes, starts the session and launches it, so nothing is billed while the form is open. Without settings it starts at once. Values are remembered per script title; layouts resolve to their contents when Start is pressed.
- Script requests on stdout: `LAMPREQ`, PIN, `HTTPREQ`. The layout JSON a script gets is documented in `sdk/luau/modules/host.luau`.
- Settings persisted: `helper`, `lib_dir`, `antiban`, `include_system_info`, `log_lines`. `helper` is replaced by the detected file on start (see Update flow).
- Cursor preference is passed to `injector load`; the injector leaves it in an `AGENT_CURSOR_<pid>` mapping the agent reads.
- The agent exists only inside `injector.exe`, so every load runs `injector.exe load`, on Windows too, and the outdated check compares clients against `injector.exe agent-version` (cached per file size and mtime on Windows). Under Wine the long-lived `serve` processes are restarted when `injector.exe` changes on disk; otherwise a rebuilt injector would keep loading the old agent from the process still running.

## HTTP whitelist

- At most 32 requests in flight across all scripts (`MAX_HTTP_IN_FLIGHT`); beyond that the answer is an error.
- Built-in hosts: those of `api_url`, `supabase_url`, `site_url`, `prices_url`. User entries live in `app.ron`, out of scripts' reach. `*.example.com` matches subdomains only.
- Redirects followed by hand (max 5), each host checked; `Authorization`/`Cookie`/`Proxy-Authorization` dropped on a host change or https-to-http.
- The whitelist is copied per request, so changes apply from the next one.

## Feedback

- `GET /launcher/sessions` (last 24 h), `POST .../:id/score` (one score per script per account), `POST .../:id/report`.
- Account names are scrubbed (`feedback::scrub`) from logs: every name seen during the run (`App::run_names`) in any case and separator becomes `[account]`.
- On store run end (`App::keep_run_record`, before ending the session) a record is sealed to `<data>/runs/<session id>.bin` under the accounts key, so a report on an ended session can include its log. Pruned after `KEEP` (24 h, the server's window).

## Account house

- The House tab keeps only what can't be read from outside the house (`injector::house::PIECES`, the same kinds and name parts as the SDK's `House.SAVED` and `House.FURNITURE`). Scripts read the Nexus, a fancy or ornate box and the town from varbits themselves.
- "Read from client" runs `injector house <pid>`, and only after the client's logged-in account matches the entry's hash. It replaces the saved pieces only when the player is inside a house (in an instance with the exit portal); otherwise it warns. It can't tell whose house it is.
- A run's own survey wins for that run and is never written back.

## Transports

- Upstream commit checked daily; last 3 versions under `<data>/transports/` with `versions.json`. A version is used only when fully downloaded and parsed; a running script keeps its data. Roll back ends at the bundled copy; the daily check won't re-take a rolled-back commit.
- Upstream rows we know are wrong are fixed by `CORRECTIONS` in `transports.rs`, applied to every copy, bundled or downloaded, so the bundled files stay as upstream had them. A file upstream can't be trusted with at all goes in `BUNDLED_ONLY`.

## Layouts

- Only items read from a client go into a layout (no item list). Item families from `injector item-families` give dose settings and `variants` so degradable/Barrows items still match after their id changes.
- A form's `required` items and `locked` prayers (`layouts::Rules`) are fitted into every layout and the value sent, each frame a tab shows. An inventory layout with no room is unusable and sends null.
- The secondary ammo slot is always shown since a layout can't know about a bolt pouch or quiver; the SDK ignores it without one.
- Prayer data and upgrades come from the client's prayer objs and RuneLite's cs2 scripts; a layout is only a set, the SDK decides how to use it.

## Entity hider

- The client page's Hide boxes show the agent's applied mask, not what was asked: the loader reads it every 2 s while that page is open (`hider::POLL_EVERY`), so a box the agent turned off because a script reached for a hidden thing turns off here too, with a log line. A read asked before the latest change is dropped, so a poll can't undo a click.
- Nothing is saved: a new agent starts with nothing hidden. The boxes are greyed out unless this loader's own agent is loaded, since an older one speaks another protocol.

## Proxies

- Saved in `proxies.bin` (data dir), sealed like `accounts.bin` with its own AAD; it holds proxy passwords, so never in `app.ron`. If it can't be opened the section refuses to save. A proxy a client uses can't be deleted; editing one sets it again on every client that uses it.
- A client's choice is kept in memory by pid only, since pids change each launch, and dropped when a clean scan no longer lists the pid. The agent keeps applying it after the loader exits (see [agent-dll.md](agent-dll.md)).
- The setting is written before every load the loader starts. A pinned release's agent replaces this one for a run, so the quote job asks that release's `injector.exe proxy-version`; a release without proxy support (no answer or another version) gets the start prompt's warning, and the queue prompt lists such entries. Allowed after the warning: the loader loads its own agent back when the run ends, which shuts down the connections made meanwhile.
- Status is polled every 2 s on the client page; a failed poll keeps the last answer, a failed change is logged.

## Development inspectors

- Inspectors poll on `POLL_EVERY` except the inventory inspector, which blocks on an agent event (`WAIT_FOR_CHANGE`) and on Linux uses its own helper lane (`Lane::Events`); after a baseline it applies slot changes to its own copy.
- Script names are downloaded on demand from RuneLite's `cs2-scripts` zip and kept in memory only (the cache has only hashes).
- Packet layouts come from the agent (`injector packet-layouts`) and fall back to `packet_defs.tsv`. Names the agent found (with their field names) win; shipped names apply by opcode only while all packet sizes match. Opcodes change with game updates.
- Icons are drawn by the client opened last (else any loaded client) and cached in `icons.bin` (append-only; older format restarted), so layouts show icons with no client loaded.

## Queue

- A queue is a list of store scripts whose live revision is queueable, each entry with its own remembered settings, an optional goal (a counter id the manifest names, compared with the script's live `COUNTERS`) and minimum/maximum minutes. Saved by name under `queues`.
- **One queue per client** (`client_in_use` counts runners), several clients at once. The current entry is an ordinary store run; its session is its own, and the next entry starts only after the previous session's keep thread has ended it (`StoreRun::closed`), so an entry never pays the extra-session fee for its predecessor.
- **Start** checks every entry once (listing, manifest, quote) and shows one confirmation with each price per hour and the most it can cost (`Entry::most_cost`, from the maximum time). Each entry is checked again just before it starts (`Check::ready`): not queueable any more, settings spec changed since the entry was set up (`spec_hash`), unreviewed community revision, or a price other than the approved one is a **change**, handled by the queue's skip/stop option. Too few credits or a failed start always stops it.
- **Ending an entry** (`Entry::should_finish`): the maximum time, or the goal (and the minimum time) reached, or the minimum alone when there is no goal. The loader then sends `FINISH`; after the manifest's `finish_grace_minutes` it kills the script, which counts as failed.
- **Exits** (`queue::outcome`): done and blocked (`report_exit`) move on; failed (error or killed) ends the queue with its when-finished step; stopped by the user ends it at once.
- **Breaks**: the last `BREAKS` plan of an entry is passed to the next one as `RESUME`, minus the time since it was reported (`queue::resumed`), so a long queue breaks like one long run.
- **When finished**: log out runs a short SDK script as its own run, or the account is left as it is.

## Platform gotchas

- The loader depends on the `injector` crate, so its non-Windows stubs must keep compiling.
- Linux: anything Win32 goes through `injector.exe serve` per Wine prefix (JSON lines, `HELPER_TIMEOUT` 45 s). A helper answers one request at a time, so frame-waiting requests use `Lane::Frames` and blocking event waits `Lane::Events`.
