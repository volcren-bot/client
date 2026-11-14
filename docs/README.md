# VOLCREN Client: start here

Automates the native game client (C++ x64 PE) on Windows, or on Linux with the client under Wine. Reads game state from client memory, sends synthetic input, draws overlays, runs Luau scripts. Read this, then the doc for the part you change.

## Map

| Component        | Doc                                              |
| ---------------- | ------------------------------------------------ |
| `src/agent`      | [agent-dll.md](agent-dll.md)                     |
| `src/ipc`        | [ipc-protocol.md](ipc-protocol.md)               |
| `src/decode`     | [ipc-protocol.md](ipc-protocol.md)               |
| `src/injector`   | [injector-and-sandbox.md](injector-and-sandbox.md) |
| `src/loader`     | [loader-gui.md](loader-gui.md)                   |
| `loader mcp` (agents) | [mcp.md](mcp.md)                         |
| Offsets, client internals, `standalone/`, `archive/`, `tools/memscan` | [reverse-engineering.md](reverse-engineering.md) |
| Release workflow, signed manifest, MSI | [releases.md](releases.md)          |

## Flow

- `agent.dll` runs inside the client: finds offsets at startup, hooks the OpenGL swap and WndProc, publishes state each frame to named shared memory (`AGENT_DATA_<pid>`, `AGENT_STATE_<pid>`, ...).
- `src/decode` decodes client memory for both the agent (in-process) and the injector (`ReadProcessMemory`).
- The injector opens the mappings from any process (including a Windows process in the same Wine prefix) and exposes them as a Rust API and Luau globals. Input goes back as window messages flagged `ipc::input::SYNTHETIC`.
- The loader runs each script as its own `injector run` process and reaches the backend only through the API (`/launcher/*`); see [loader-gui.md](loader-gui.md).

## Invariants

- **No hardcoded addresses.** Offset rules match code shape at runtime; a rule that doesn't match exactly one candidate fails rather than guesses.
- **`src/ipc` owns the wire format.** Any change bumps `ipc::PROTOCOL_VERSION`, with agent, injector and the lib's `host.luau` updated together.
- **Agent version = hash of the DLL** (`ipc::build_id`); the DLL hot reloads without restarting the client.
- **Windows code is cross-compiled** for `x86_64-pc-windows-gnu`; only the loader and the injector's non-Windows stubs are native.
- **Scripts see only the globals** in `src/injector/src/sandbox.rs`; `lib/host.luau` (separate repo) is their typed wrapper.
- **`standalone/` is decompiled client code**: read only, keep private.
- **The agent runs in a real client**: avoid anything that could crash it.

## Build

- Aliases in `.cargo/config.toml`: `cargo fast-win` (agent + injector, `fast` profile), `cargo fast-loader`, `cargo b-agent`/`b-injector`/`b-loader` (release, separate target dirs so they build in parallel), `cargo lint` (clippy, Windows target).
- Outputs: `target/x86_64-pc-windows-gnu/<profile>/{agent.dll,injector.exe}`, `target/<profile>/loader`.
- The agent's tests only build for the Windows target.
- Editions: `agent`, `ipc` 2021; `decode`, `injector`, `loader` 2024.

## Where to change what

| Task                              | Path                                                                                                                                                                                                                                                                       |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| New game state                    | offset rule in `src/agent/src/client/offsets/` → reader in `src/agent/src/client/` → layout in `src/ipc/src/state.rs` (bump protocol) → decoder in `src/decode/src/` (plus a worker kind if scripts poll it, see [ipc-protocol.md](ipc-protocol.md)) → global in `sandbox.rs` → type and wrapper in the lib's `host.luau`, and its lib docs |
| Offsets broke after a game update | [reverse-engineering.md](reverse-engineering.md)                                                                                                                                                                                                                           |
| New sandbox global                | checklist in [injector-and-sandbox.md](injector-and-sandbox.md)                                                                                                                                                                                                            |
| Loader UI                         | `src/loader/src/app.rs`, platform calls in `src/loader/src/backend/`                                                                                                                                                                                                       |
