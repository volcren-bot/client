# MCP server (`loader mcp`)

The loader binary doubles as a stdio MCP server, so any agent CLI (Claude Code, Codex, ...) can drive the user's game clients and write scripts. Code: `src/loader/src/mcp.rs`, `mcp/runs.rs` (runs and metering), `mcp/library.rs` (docs search, `luau-analyze`), `workspace.rs`.

## Why it works this way

- **Headless.** `loader mcp` never talks to the loader window. It reads the window's saved settings (eframe `app.ron` under app id `Loader`: `app`, `cursor`, `http_whitelist`), its keyring sign-in and its lib cache, and runs scripts itself. Runs it starts are invisible to the window and stop when the agent closes stdin.
- **The agent acts through Luau, not clicks.** A model turn is far slower than a game tick and per-click control would bypass the lib's human-like input, so the only way to act is `run_luau`: a short script against the lib, run in the same sandbox as editor scripts. Agents get no power editor scripts lack.
- **Store scripts can't be run** from the MCP server. Their decrypted source must never reach an agent.

## Invariants

- **One script per client, across processes.** `Script::start` takes an exclusive lock on `<data>/clients/<sha256(target key)[..16]>.lock` for any run with a client, and releases it when the process exits. The window and every `loader mcp` share it. Lock files are never deleted: deleting one would let two holders race.
- **Shared refresh token.** Supabase rotates refresh tokens, so `Api::refresh` uses the keyring's token when there is one (another process may have rotated it) and clears the keyring on rejection only if it still holds the rejected token.
- **Sign-in gates the client tools.** `load_agent`, `game_state` and `run_luau` need the loader's saved sign-in; `clients`, `lib_docs` and `check_luau` don't.
- **Agent sessions meter every run** (free, `/launcher/agent-sessions` in the API README). A run starts its agent session before spawning and is refused if that fails, heartbeats every `heartbeat_seconds` (clamped 10 to 120) and ends it on exit; `ended: true` on a heartbeat starts a new one.
- **Settings come from the manifest.** A run of `path` takes the defaults of a `manifest.json` beside it, overridden by the tool's `settings`; a snippet gets `{}`. An `exit` line reports the script's `Exit` (done or blocked).
- `loader mcp` holds a share of `loader.lock`, so the window never self-updates under a running server.

## Lib, docs and analyzer

- The lib is the store lib from `libs::ensure` (installed once per server), or the settings' lib folder when set. The lib release zip carries `docs/**/*.md` and `examples/*.luau` beside the sources, which `lib_docs` searches and reads; `libs.rs` accepts `.md` entries for that.
- `lib_docs` maps `docs/<path>.md` to `require("@lib/<path>")`; the lib's doc layout must keep mirroring its module paths.
- `check_luau` runs `luau-analyze` from beside the loader executable (shipped in releases, see [releases.md](releases.md)), else from `PATH`, and returns every diagnostic for the checked file only. Published scripts check clean. Snippets are checked from the workspace, so `@lib` resolves through its `.luaurc`.

## Workspace

`<data>/workspace` holds `.mcp.json` (registers this loader's `mcp` command), `.luaurc` (aliases `@lib` to the lib above), `AGENTS.md` and `CLAUDE.md`. `workspace::prepare` rewrites them when they differ, each time the Terminal page opens and before each run or check. The Terminal page starts its shell there, so an agent CLI launched from it finds the server with no setup. `runs/<pid>.luau` keeps the last snippet run on each client and anchors its relative `require`.
