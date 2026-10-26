# AGENTS.md

VOLCREN Client automates the native game client: an injected DLL reads game state from memory, and Luau scripts drive the game through it.

Read [docs/README.md](docs/README.md) before any change: architecture, invariants, build/lint/test commands, and the per-component doc for the code you're touching.

- New game state, or offsets broken after a game update: [docs/reverse-engineering.md](docs/reverse-engineering.md).
