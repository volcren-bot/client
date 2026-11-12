# ipc (`src/ipc`)

Shared-memory contract between the agent (writer, in the client) and the injector (reader; the loader reaches it through the injector). `no_std` + `alloc`, no dependencies. Layouts, names and constants are in the crate.

## Protocol version

- Any change to a layout, a message id, a `worker_kind`, or a field's meaning bumps `PROTOCOL_VERSION` (`lib.rs`).
- Update together in one change: the agent writer, the readers in `src/decode` and `src/injector/src/win.rs`, the layout tests (`cargo test -p ipc`), and `lib/host.luau` if Luau-visible types change.
- `Header` and the state block both carry the version; readers reject a mismatch as `Incompatible`, which means a stale agent or injector (reload the agent).

## Concurrency invariants

- State (`AGENT_STATE_<pid>`) is sequence-locked (`AtomicU32` + fences). One writer per field group; readers detect torn reads and retry. Always read through the `Shared` accessors, never raw.
- Logs (packets, scripts, items, varcs) share `ring.rs`: writers on any thread claim a sequence with `fetch_add`, blank the slot's `commit`, fill, then publish the sequence in `commit`. Readers in other processes copy lock-free; an entry counts only if `commit` matched before and after the copy, otherwise it is counted as dropped. `ring::Header::is_compatible` checks slot count and size.
- Each log exists only when its offset rule matched; readers must handle its absence.
- Log subscribers: a reader creates an auto-reset wake event, claims a bit in `subscribers`, and blocks. The agent signals matching subscribers per entry and clears bits whose event it can no longer open. Packet subscribers also set a per-opcode `Filter`.
- Packet header `Movement` is seqlocked with one writer (the receive hook). `layouts` are written once at hook time, then `layouts_ready` is set; read nothing before it.
- Overlay is double-buffered: the injector paints the slot the agent isn't showing, then bumps `overlay_serial`; the agent shows slot `overlay_serial & 1`, so it never shows a half-painted overlay.
- `frame_serial` is unaligned in the packed `Header`: read it unaligned. Waiters on `AGENT_FRAME_<pid>` re-read `frame_serial` in slices because another waiter may consume the auto-reset wake.

## Requests

- Worker path (preferred): the injector holds mutex `AGENT_WORKER_<pid>` from writing `worker_request` to reading `worker_reply`, so any threads/processes can share the single slot. It sets `AGENT_WORK_<pid>`; the agent's listener thread serves it in-process (no wait for the game thread's next frame, which costs ~17 ms at 60 fps) and sets `AGENT_WORK_DONE_<pid>`. A stale wake shows the wrong `sequence`; the injector waits again.
- Fallback: the injector reads memory itself if the worker doesn't answer in 50 ms, replies `TOO_BIG`, or an event is missing. `VOLCREN_NO_WORKER=1` forces it for comparison. Every worker read must therefore also work through `reader(..)`.
- Window-message requests (`input::*_MESSAGE`, and cache requests) run on the game thread. The agent answers only a slot's newest request, so injectors hold `AGENT_REQUEST_<pid>_<message>` around each ask; otherwise a second asker leaves the first waiting out its 3 s timeout. Injector-only; not a layout change.
- Async input posts window messages and never touches the worker slot.

## Adding a worker read

1. Write it as a plain function over a `Memory` and the `Layout` in `src/decode`.
2. Add a `worker_kind::*` id, its `decode::worker` function, its `part` flags in `decode::worker::parts`, and an arm in `decode::worker::serve`.
3. In `src/injector/src/win.rs`, call `ask_worker(pid, kind, args)` and fall back to the same function through `reader(..)`.
4. Bump `PROTOCOL_VERSION`.
