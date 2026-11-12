# loader (`src/loader`)

The desktop app (`eframe`/egui). It never touches the client itself: a new capability goes into the injector (and usually the agent) first, then gets surfaced here. Releases, signing and the installer are in [releases.md](releases.md).

## Map

| Path | Holds |
| --- | --- |
| `app.rs` | All page UI and app state: sidebar, header, Clients, Editor, Sessions, run pages, Development (tools, inspectors' UI, `LogTable`), Settings, output panel |
| `script.rs` | Script process: spawn, stdio pumping, pause/stop, `ScriptEvent`; `Process` (session thread's handle); `Host` (answers PIN, lamp, HTTP off the UI) |
| `session.rs` | Store run start (cache, in-memory download, decrypt), `Beat` heartbeat state machine, `keep`, quotes/restart |
| `store.rs` | Scripts page, prices, community tags, `Confirmed` |
| `api/` | `config.rs` endpoints, `auth.rs` PKCE + keyring, `client.rs` `Api` (blocking, 401 refresh-and-retry), `models.rs` (credits formatting, `API_VERSION`) |
| `account.rs` | `Auth` state, sign-in round trip |
| `accounts.rs` | Accounts page and sealed `accounts.bin` |
| `feedback.rs` | Feedback page/tab, diagnostics, name scrubbing, sealed run records |
| `layouts.rs` | Equipment/inventory/prayer layouts, rules, picker |
| `breaks.rs`, `xp.rs`, `counters.rs` | Break/sleep editors, run trackers |
| `form.rs` | Form spec rendering |
| `http.rs`, `whitelist.rs` | Script HTTP requests, whitelist |
| `transports.rs` | Transport data versions |
| `libs.rs` | Shared lib selection for store scripts |
| `update.rs`, `update/` | Signed manifest, `archive.rs` (Linux), `msi.rs` + `msi_command.rs` (Windows) |
| `vars.rs`, `script_log.rs`, `script_names.rs`, `inventory.rs`, `widgets.rs`, `packets.rs`, `packet_defs.rs` | Development inspectors |
| `icons.rs` | Icon store |
| `terminal.rs` | Terminal page (`portable-pty` + `alacritty_terminal`) |
| `log_view.rs`, `pipe.rs` | Output tab buffer; append-to-file pipe |
| `theme.rs`, `highlight.rs`, `fold.rs` | Widgets/fonts, Luau highlighting |
| `backend/` | `mod.rs` `Settings` + platform-neutral facade; `native.rs` (Windows, calls `injector` lib); `wine.rs` (Linux helper) |
| `paths.rs` | `ProjectDirs` `com.volcren.Volcren`, user agent |
| `fake_api.rs`, `flow_tests.rs` | Test API and flow tests |

## Threads and frame independence

- The window draws no frames while minimised or on a hidden workspace, so nothing billing-critical runs in `ui()`.
- Each store run has its own thread (`session::keep`): heartbeat every `heartbeat_seconds` (clamped 10 to 120), stats from `report_stats`, stop on server-ended session, stop after `GIVE_UP_AFTER` (4 min) without a successful beat, before the server's 5 min timeout. Ends the session when the script exits.
- PIN, lamp and `http_request` are answered on the script's output thread (`script::Host`), since a script blocks until answered. Only forms wait for the window.
- All `Api` calls block and run on `api_job` threads.
- Pausing a store script does not pause billing.
- Sign-out (button or server-revoked) runs `App::stop_all` first: stops scripts and ends sessions, with a 3 s limit; the server's sweep covers the rest. Same on exit.

## Sign-in and storage

- Discord via Supabase, PKCE; listener on `127.0.0.1:5217`, redirect URL `http://localhost:5217/`. Login times out after 5 min.
- Keyring service `volcren-loader`: `refresh-token` (sign-in), `accounts-key` (32-byte AES-256-GCM key for `accounts.bin` and run records). Never in eframe storage.
- eframe storage keys: `script`, `last_script`, `forms`, `layouts`, `cursor`, `ui_font`, `community_confirmed`, `http_whitelist`, plus `Settings`.
- `accounts.bin` (data dir) is `nonce || ciphertext || tag`. If it can't be opened (no keyring, wrong key) the page refuses to save so it isn't overwritten. Only `hash_name` of the account name is kept, normalised like the client (lowercase, spaces/hyphens to `_`).
- `layouts`: if unreadable (written by a newer loader) it is copied to `layouts-unreadable-<unix>.ron` and never saved over. Two open loaders still overwrite each other's storage.
- Endpoints compiled from `src/loader/config.json`; `API_URL`, `SUPABASE_URL`, `SUPABASE_ANON_KEY`, `SITE_URL`, `PRICES_URL` env vars override at run time. No `.env`.

## Store runs and billing

- `API_VERSION` = 3; the server tells an older loader to update.
- Credits: API counts microcredits (1,000,000 per CREDIT); `price` is microcredits per hour.
- Run asks `GET /launcher/scripts/:id/quote` first: only the server knows author/grantee/tester pricing (0) and the account's open sessions. The quote carries the current balance, so website top-ups count. A quoted fee means another session is open; the loader confirms before starting.
- The listing's `can_access` decides whether Run is enabled. `StoreScript::blocked`/`short_of_funds` only word the refusal, using the server's rounding `ceil(min_balance_minutes * price / 60)` without the fee. The server's `min_balance_minutes` decides.
- Restart (`App::restart_run`) ends the old session again (idempotent), re-reads the listing (community list only for tester+) and the quote; `session::asks_first` prompts when the quoted price changed or a fee applies. The old run closes only once the new one starts.
- Store files are AES-256-GCM encrypted (`session::decrypt_file`; `key: null` means plain, pre-encryption revisions), downloaded on every start, never written to disk, sent to the injector over stdin (`MODULES`). The revision cache folder is empty and only anchors relative `require`.
- Community scripts: the warning's acceptance is stored per script and revision (`Confirmed`, max 500). `session::prepare` refuses and ends a session whose started revision isn't the confirmed one, so a revision published after the warning never runs unseen.
- One script per client. A store start in progress (quoting, dialog, downloading) holds its client.
- Store scripts use the newest lib in `lib/index.json` whose `requires_host` is at most `injector::HOST_API` (`libs.rs`); if a newer lib needs a newer loader, the script log says to update.

## Update flow

- Fetches `latest.signed.json` from the public `client` bucket and verifies the signature before parsing. Prompts when its `build` (commit Unix time, `VOLCREN_BUILD` from `build.rs`) is newer; build 0 never prompts.
- Installs by itself (`update::installs_by_itself`) unless a run is active/starting/in a dialog, or the loader was restarted with `--update-failed`, so a failure can't loop. Checks once per start.
- Never installs while another loader runs: each holds a share of `loader.lock` in the data dir (`update::Instance`).
- Under a cargo `target/` dir it never installs (`Kind::Dev`); the header's version hover names the newer version.
- Linux: unpack into `<install>/.update/`, rename 4 files in with hard links for rollback, start the new loader after this one closes (so `save()`/`on_exit` run). Unwritable install dir falls back to browser download.
- Windows: MSI to `<data>/update/`, sha256 checked; loader copies itself there and runs the copy with `--finish-update`, which waits, runs `msiexec /passive` with the install's own mode and folder (read from Windows Installer; upgrades don't cross modes), and restarts the installed loader unelevated, with `--update-failed <msg>` on failure (declined UAC counts). Non-MSI installs get browser download.
- `Settings::fill_missing` forces an install to use the `agent.dll` and `injector` next to it, so a saved path can't pair versions.
- Versions: `YYYY.MM.DD-<7 char commit>`, or `dev` outside git.

## UI mechanisms

- Sidebar drawer (window narrower than `SIDEBAR_MIN_WINDOW`): it overlays the page, so row hovers use `Ui::rect_contains_pointer` (layer-aware), never the raw pointer.
- `theme::link_hovered` reads the hover from the last frame's response so text can be coloured before it's laid out.
- Output panel: the `LayoutJob` is rebuilt only when tab, revision, font or colour changes; the galley (`LogJob::laid_out`) is kept while wrap width and pixels per point are unchanged and a one-space probe galley is still the one egui's cache returns, because egui replaces that cache when it rebuilds the font atlas.
- Output tabs are capped (`backend::LOG_LINES`, 2000 to 10 000) because a tab is one `TextEdit` that egui re-lays out on every new line (~6 ms and 13 MB per 1000 lines). Trimmed a quarter at a time.
- Selection drag past the edge scrolls (`scroll_selection_past_edge`) because egui's `TextEdit` only scrolls to the cursor on key moves. Speed is relative to room to the window edge since the output sits at the bottom.
- `theme::icon_button_width` sizes a group from all its possible labels so buttons never resize.
- Inventory drag and drop uses `DragAndDrop::set_payload` on `drag_started`, not `dnd_drag_source`, whose drag-only sense swallows clicks.
- Row widths for form rows are remembered in egui temp memory to decide wrapping next frame.

## Scripts, settings and stdin contract

- Each script runs as `injector run --script <path> <pid>`, so a crash can't take the app down.
- Lines written to the script's stdin before start: antiban config (`injector::antiban::line`), transports (`TRANSPORT_SETTINGS`), `prices_url` (`injector::prices::line`). Live: `UPDATE` (form update, read by `Host.form_update()`, `HOST_API` 8), antiban push, transport push, skip-next.
- Script requests on stdout: `LAMPREQ`, PIN, `HTTPREQ`. The layout JSON a script gets is documented in the lib's `host.luau`.
- Settings persisted: `dll`, `helper`, `lib_dir`, `antiban`, `transports`, `include_system_info`, `log_lines`; detected next to the executable first, then `target/x86_64-pc-windows-gnu/{release,debug}`.
- Cursor preference is passed to `injector load`; the injector leaves it in an `AGENT_CURSOR_<pid>` mapping the agent reads.

## HTTP whitelist

- At most 32 requests in flight across all scripts (`MAX_HTTP_IN_FLIGHT`); beyond that the answer is an error.
- Built-in hosts: those of `api_url`, `supabase_url`, `site_url`, `prices_url`. User entries live in eframe storage, out of scripts' reach. `*.example.com` matches subdomains only.
- Redirects followed by hand (max 5), each host checked; `Authorization`/`Cookie`/`Proxy-Authorization` dropped on a host change or https-to-http.
- The whitelist is copied per request, so changes apply from the next one.

## Feedback

- `GET /launcher/sessions` (last 24 h), `POST .../:id/score` (one score per script per account), `POST .../:id/report`.
- Account names are scrubbed (`feedback::scrub`) from logs: every name seen during the run (`App::run_names`) in any case and separator becomes `[account]`.
- On store run end (`App::keep_run_record`, before ending the session) a record is sealed to `<data>/runs/<session id>.bin` under the accounts key, so a report on an ended session can include its log. Pruned after `KEEP` (24 h, the server's window).

## Transports

- Upstream commit checked daily; last 3 versions under `<data>/transports/` with `versions.json`. A version is used only when fully downloaded and parsed; a running script keeps its data. Roll back ends at the bundled copy; the daily check won't re-take a rolled-back commit.

## Layouts

- Only items read from a client go into a layout (no item list). Item families from `injector item-families` give dose settings and `variants` so degradable/Barrows items still match after their id changes.
- A form's `required` items and `locked` prayers (`layouts::Rules`) are fitted into every layout and the value sent, each frame a tab shows. An inventory layout with no room is unusable and sends null.
- The secondary ammo slot is always shown since a layout can't know about a bolt pouch or quiver; the lib ignores it without one.
- Prayer data and upgrades come from the client's prayer objs and RuneLite's cs2 scripts; a layout is only a set, the lib decides how to use it.

## Development inspectors

- Inspectors poll on `POLL_EVERY` except the inventory inspector, which blocks on an agent event (`WAIT_FOR_CHANGE`) and on Linux uses its own helper lane (`Lane::Events`); after a baseline it applies slot changes to its own copy.
- Script names are downloaded on demand from RuneLite's `cs2-scripts` zip and kept in memory only (the cache has only hashes).
- Packet layouts come from the agent (`injector packet-layouts`) and fall back to `packet_defs.tsv`; shipped names apply by opcode only while all packet sizes match. Opcodes change with game updates.
- Icons are drawn by the selected client and cached in `icons.bin` (append-only; older format restarted), so layouts show icons with no client loaded.

## Platform gotchas

- The loader depends on the `injector` crate, so its non-Windows stubs must keep compiling.
- Linux: anything Win32 goes through `injector.exe serve` per Wine prefix (JSON lines, `HELPER_TIMEOUT` 45 s). A helper answers one request at a time, so frame-waiting requests use `Lane::Frames` and blocking event waits `Lane::Events`.
