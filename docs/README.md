# VOLCREN Client: start here

Docs for agents working on this repo. Read this file, then the doc for the part you are changing.

VOLCREN Client automates the native game client (a C++ x64 PE). It reads game state straight from the client's memory (not from screenshots), sends synthetic mouse and keyboard input, draws overlays, and runs Luau scripts against a typed API over all of that. It runs on Windows, and on Linux with the client under Wine (usually Bolt launcher's prefix, `~/.local/share/bolt-launcher/`).

## Docs

| Doc                                                                | Covers                                                                                                                                                                                                             |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [agent-dll.md](agent-dll.md)                                       | `src/agent`: the DLL inside the client. Hooks, per-frame state publishing, command messages, unload.                                                                                                               |
| [reverse-engineering.md](reverse-engineering.md)                   | Offset rules, native client internals, `standalone/` (decompiled Java client), `archive/` (old client builds), `tools/memscan`. Read before adding game state or fixing offsets after an update.                   |
| [ipc-protocol.md](ipc-protocol.md)                                 | `src/ipc`: the shared-memory contract and `PROTOCOL_VERSION`.                                                                                                                                                      |
| [decode](../src/decode/src/lib.rs)                                 | `src/decode`: the decoders over client memory (`Memory`) and their data types, shared by the agent (in-process, the worker) and the injector (`ReadProcessMemory`). Details in [ipc-protocol.md](ipc-protocol.md). |
| [injector-and-sandbox.md](injector-and-sandbox.md)                 | `src/injector`: CLI, state readers, input, pathfinding, the Luau sandbox and `require`.                                                                                                                            |
| [loader-gui.md](loader-gui.md)                                     | `src/loader`: the egui app, the Wine backend, script processes, dev tools.                                                                                                                                         |

## Architecture

```
 loader (egui, native)                       game client process (game client exe)
   |  spawns / talks to                        +----------------------------------+
   v                                           |  agent.dll (hooks, offset rules) |
 injector (lib + CLI) --- injects --------->   |    reads client memory           |
   |  |                                        |    hooks OpenGL swap + WndProc   |
   |  +---- reads state / writes input ---<--> |    writes/reads shared memory    |
   |        via shared memory (ipc layout)     +----------------------------------+
   v
 Luau sandbox (mlua): scripts written in Luau against the typed `host` wrapper
```

The loader also talks to the Volcren API (sign-in, store, sessions, heartbeats, stats); see [loader-gui.md](loader-gui.md). It never talks to the database directly.

Per frame:

1. The agent's render hook reads game structures, using offsets it found at startup, and writes them to named shared memory: `AGENT_DATA_<pid>` (header, frame image, overlay) and `AGENT_STATE_<pid>` (game state).
2. The injector, from any process (including a Windows process in the same Wine prefix), opens those mappings and exposes them as a Rust API and as Luau globals.
3. Input goes back as window messages posted to the client, flagged `ipc::input::SYNTHETIC`; overlay data goes into shared memory for the agent to draw.
4. The loader runs each script as its own `injector run` process.

## Invariants

- **No hardcoded addresses.** Offsets come from rules that match code shape at runtime, and a rule that doesn't match exactly one candidate fails rather than guesses. See [reverse-engineering.md](reverse-engineering.md).
- **`src/ipc` owns the wire format.** Any change to it bumps `ipc::PROTOCOL_VERSION`, with the agent writer, injector reader and the Luau `host` wrapper updated in the same change. See [ipc-protocol.md](ipc-protocol.md).
- **Agent version = hash of the DLL** (`ipc::build_id`). The DLL hot reloads without restarting the client.
- **Windows code is cross-compiled** for `x86_64-pc-windows-gnu`. Only the loader and the injector's non-Windows stubs are native.
- **Scripts see only the sandbox globals** from `src/injector/src/sandbox.rs`; the Luau lib's `host.luau` is their typed wrapper. The lib and the scripts live in their own projects (`lib/`, `scripts/` next to this repo).
- **`standalone/` is decompiled game client code**: read only, keep private.
- **The agent runs inside a real game client.** Reloading the DLL is safe; avoid anything that could crash the client.

## Build, lint, test

```sh
cargo build --release --target x86_64-pc-windows-gnu -p agent -p injector   # agent.dll, injector.exe
cargo build --release                                                        # native loader + injector
cargo fast-win; cargo fast-loader     # `fast` profile (release without full LTO) for iteration
cargo lint                            # clippy, whole workspace, Windows target
cargo test -p ipc -p decode -p injector -p loader
```

Outputs: `target/x86_64-pc-windows-gnu/<profile>/{agent.dll,injector.exe}`, `target/<profile>/loader`. Aliases are in `.cargo/config.toml`. The agent's tests only build for the Windows target.

## Where to change what

| Task                              | Path through the code                                                                                                                                                                                                                                                                                                                                |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| New game state                    | offset rule in `src/agent/src/client/offsets/` → reader in `src/agent/src/client/` → layout in `src/ipc/src/state.rs` (bump protocol) → decoder in `src/decode/src/` (plus a worker kind if scripts poll it, see [ipc-protocol.md](ipc-protocol.md)) → global in `sandbox.rs` → type and wrapper in the lib's `host.luau`, and its docs |
| Offsets broke after a game update | [reverse-engineering.md](reverse-engineering.md)                                                                                                                                                                                                                                                                                                     |
| New sandbox global                | checklist in [injector-and-sandbox.md](injector-and-sandbox.md)                                                                                                                                                                                                                                                                                      |
| Loader UI                         | `src/loader/src/app.rs`, platform calls in `src/loader/src/backend/`                                                                                                                                                                                                                                                                                 |

Editions: `agent` and `ipc` are 2021, `injector` and `loader` 2024.
