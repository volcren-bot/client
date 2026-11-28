# MCP server (`loader mcp`)

The loader binary doubles as a stdio MCP server, so any agent CLI (Claude Code, Codex, ...) can drive the user's game clients and write scripts. Code: `src/loader/src/mcp.rs`, `mcp/runs.rs` (runs and metering), `mcp/look.rs` (`game_state` sections), `mcp/library.rs` (docs search, `luau-analyze`), `workspace.rs`.

## Why it works this way

- **Headless.** `loader mcp` never waits on the loader window. It reads the window's saved settings (`app.ron` under app id `Loader`, `storage.rs`: `app`, `cursor`, `http_whitelist`), its keyring sign-in and the SDK it unpacks into the cache, and runs scripts itself. Runs stop when the agent closes stdin. The only channel to the window is one-way: each run's record in `<data>/agent-runs/`, which the window shows under the run's client but can't control (see Agent runs in [loader-gui.md](loader-gui.md)).
- **The agent acts through Luau, not clicks.** A model turn is far slower than a game tick and per-click control would bypass the SDK's human-like input, so the only way to act is `run_luau`: a short script against the SDK, run in the same sandbox as editor scripts. Agents get no power editor scripts lack.
- **Store scripts can't be run** from the MCP server. Their decrypted source must never reach an agent.
- **Actions are built-in scripts, not new powers.** Writing and checking a script for one click takes longer than many situations last (a random event NPC leaves within about a minute), so `talk`, `interact`, `use_item_on`, `open_bank`, `travel` and `travel_plan` each run a fixed Luau script (`mcp/*.luau`) through the same path as `run_luau`, with their arguments passed as its settings. They act through the SDK like any script, hold the client's lock and are metered as runs.
- **`game_state` reads memory, not a script.** It goes through the injector like the loader window, so it works while a run holds the client and isn't metered. Sections are opt-in (`include`) and scene sections are cut by `name`, `radius` and `limit`, because a full read of NPCs, objects and chat fills an agent's context.
- **Bank PINs never reach the agent.** Every run gets the loader's `PINS` header and accounts, so the injector enters the PIN when a script asks (`BankpinKeypad.enter`); the instructions tell agents to rely on that instead of clicking the keypad.

## Invariants

- **One script per client, across processes.** `Script::start` takes an exclusive lock on `<data>/clients/<sha256(target key)[..16]>.lock` for the run's client, and releases it when the process exits. The window and every `loader mcp` share it. Lock files are never deleted: deleting one would let two holders race.
- **Shared refresh token.** Supabase rotates refresh tokens, so `Api::refresh` uses the keyring's token when there is one (another process may have rotated it) and clears the keyring on rejection only if it still holds the rejected token.
- **Sign-in gates the client tools.** `load_agent`, `game_state`, `run_luau` and the actions need the loader's saved sign-in; `clients`, `sdk_docs` and `check_luau` don't.
- **Agent sessions meter every run** (free, `/launcher/agent-sessions` in the API README). A run starts its agent session before spawning and is refused if that fails, heartbeats every `heartbeat_seconds` (clamped 10 to 120) and ends it on exit; `ended: true` on a heartbeat starts a new one.
- **Run output.** Each run keeps its last 5000 lines in memory for `run_output`, whose `match` filters them and waits for a matching line; every line also goes to the run's record in `<data>/agent-runs/` (kept a day), whose path `run_output` returns, for logs longer than that. `run_luau`'s `debug` overrides the loader's Script debugging setting for that run only.
- **Settings come from the manifest.** A run of `path` takes the defaults of a `manifest.json` beside it, overridden by the tool's `settings`; a snippet gets `{}`. An `exit` line reports the script's `Exit` (done or blocked).
- `loader mcp` holds a share of `loader.lock`, so the window never self-updates under a running server.

## SDK, docs and analyzer

- `sdk_docs` searches and reads the SDK folder from `sdk::dir`: the settings' SDK folder when set, else the copy built into the loader ([sdk.md](sdk.md)). `run_luau` passes `--sdk` only for a settings folder; otherwise the injector's own SDK runs.
- `sdk_docs` maps `docs/<name>.md` to `require("@volcren/<name>")`, which holds because docs and modules are both flat and named alike.
- `check_luau` runs `luau-analyze` from beside the loader executable (shipped in releases, see [releases.md](releases.md)), else from `PATH`, and returns every diagnostic for the checked file only. Published scripts check clean. Snippets are checked from the workspace, so `@volcren` resolves through its `.luaurc`.

## Workspace

`<data>/workspace` holds `.mcp.json` (registers this loader's `mcp` command), `.luaurc` (aliases `@volcren` to the SDK's `modules`), `AGENTS.md` and `CLAUDE.md`. `workspace::prepare` rewrites them when they differ, each time the Terminal page opens and before each run or check. The Terminal page starts its shell there, so an agent CLI launched from it finds the server with no setup. `runs/<pid>.luau` keeps the last snippet run on each client and anchors its relative `require`.
