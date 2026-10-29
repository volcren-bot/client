# Game Client Automation Toolkit

This project is a scripting and automation toolkit for a popular MMORPG game.
It attaches a small library to a running game client, reads the game's state (skills, inventory,
interfaces, the world around the player, ...) and sends mouse and keyboard input back to it.
Scripts are written in [Luau](https://luau.org) and run against a typed API, with a desktop
app to pick a game client, load the library and start or stop scripts.

It runs on Windows, and on Linux with the game client running under Wine.

## Components

1. **agent**: the library that gets loaded into the game client.
2. **injector**: loads the agent into the game client and runs Luau scripts against it.
3. **ipc**: the shared-memory layout the agent and the injector talk through.
4. **loader**: the desktop app that ties the others together.

## Building and running

### Dependencies

- [Rust](https://rustup.rs) (stable, edition 2024 support, so a recent toolchain).
- A C/C++ compiler (Luau is built from source): `gcc`/`g++` on Linux.
- For the Windows binaries, the Windows GNU target and MinGW-w64:
  ```sh
  rustup target add x86_64-pc-windows-gnu
  # Debian/Ubuntu
  sudo apt install mingw-w64
  # Arch
  sudo pacman -S mingw-w64-gcc
  ```
- On Linux, [Wine](https://www.winehq.org) to run the game client and the Windows binaries, plus the
  usual GUI libraries for the loader (X11 or Wayland, OpenGL).

The agent and the injector's client bindings are Windows code, so they are always cross-compiled
for `x86_64-pc-windows-gnu` (from Linux) or built with that target (on Windows).
The loader is a native app, and `.cargo/config.toml` already links the C++ runtime statically so
the Windows binaries run under Wine.

### Build

```sh
# Windows binaries: the agent DLL (agent.dll) and the injector helper (injector.exe)
cargo build --release --target x86_64-pc-windows-gnu -p agent -p injector

# Native loader (and the native injector it uses to run scripts when no game client is selected)
cargo build --release
```

On Windows you can build everything for the same target with
`cargo build --release --target x86_64-pc-windows-gnu --workspace`.

For development you might want to use the fast profile:

```sh
cargo build --profile fast --target x86_64-pc-windows-gnu -p agent -p injector
cargo build --profile fast
```

Outputs:

- `target/x86_64-pc-windows-gnu/release/agent.dll`
- `target/x86_64-pc-windows-gnu/release/injector.exe`
- `target/release/loader` (`loader.exe` on Windows)

### Run

1. Start the game client (under Wine on Linux).
2. Start the loader:
   ```sh
   cargo run --release -p loader
   ```
3. In the loader, select the game client, check that the DLL and helper paths point at the files built
   above (they are filled in automatically when they are in `target/`), and load the agent.
4. Press **Sign in with Discord** (in your browser); the loader needs a signed-in account. Then either pick a
   script in the **Scripts** tab and press **Run**, or open a local Luau script in the **Editor** tab and run it.

The loader's endpoints (`api_url`, `supabase_url`, the public `anon_key` and optionally `site_url`) are in
`src/loader/config.json` and compiled in; the environment variables `API_URL`, `SUPABASE_URL`, `SUPABASE_ANON_KEY`
and `SITE_URL` override them at run time, e.g. to point at a local backend.

The injector can also be used on its own, for example `injector list` to list game clients,
`injector load <pid> <dll>` to load the agent and `injector run --script script.luau <pid> < script.luau`
to run a script.

## What each component does

### agent (`src/agent`)

A DLL (`agent.dll`) loaded inside the game client process. It hooks the client's rendering and
input, finds the game's internal data (player state, skills, varps, widgets, menus, scene
models, ...) and publishes it into shared memory every frame. It also takes input requests (mouse
moves, clicks, keys) and overlay drawings from the shared memory and applies them in the game.
It exports the functions used to inject, unload and toggle input, and can be used as a
[Simba](https://github.com/Villavu/Simba) plugin as well.

### injector (`src/injector`)

Both a library and a command line tool (`injector`). It finds running game clients, injects and
unloads the agent, and exposes everything the agent publishes as a Rust API: game state, widgets,
projection from world to screen, collision and pathfinding built from the game client's cache, and
human-like mouse movement (credits to [BenLand100](https://ben.land/)).

It also embeds the Luau sandbox: `injector run <pid>` reads a script from stdin and runs it with those
functions available as globals.

### ipc (`src/ipc`)

A small `no_std` crate with the shared-memory contract between the agent and the injector: the
memory names, the header, the state layout, the input and overlay messages and a protocol version
that both sides check so an outdated agent is detected. Anything that changes the wire format
goes here, and the Luau host wrapper (`host.luau`) has to stay in sync with it.

### loader (`src/loader`)

The graphical app (built with egui). It lists the running game clients, loads, reloads and unloads the
agent, shows its connection state and version, and starts, pauses and stops scripts, each in its
own `injector run` process. On Linux it detects the Wine prefix of each game client and runs the
Windows helper inside it; on Windows it calls the injector directly.
