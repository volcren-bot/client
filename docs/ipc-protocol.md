# ipc (`src/ipc`)

The shared-memory contract between the agent and its readers. `#![no_std]` with `alloc`, no dependencies. `agent` and `injector` depend on it; the loader gets it through the injector.

## Contents

| Item | Purpose |
| --- | --- |
| `PROTOCOL_VERSION` (`u8`, `lib.rs`) | Checked on both sides; a mismatch means a stale agent or injector. |
| `shared_memory_name(pid)` | `AGENT_DATA_<pid>`: `Header`, frame image (`IMAGE_DATA_SIZE`), then two overlay slots (`overlay_offset(slot)`). Slot 0 is Simba's; in `ON_SERIAL` mode the injector paints the slot the agent isn't showing and the agent shows slot `overlay_serial & 1`, so it never reads a half painted overlay. |
| `state::state_memory_name(pid)` | `AGENT_STATE_<pid>`: the game state block. |
| `initial_cursor_name(pid)` | `AGENT_CURSOR_<pid>`: optional 5-byte mapping (`INITIAL_CURSOR_SIZE`: style, r, g, b, size) the injector creates before loading; the agent reads it once when building its header. |
| `unload_event_name(pid)` | `AGENT_UNLOAD_<pid>`: named event that asks the agent to unload. |
| `Header` (`repr(C, packed)`) | `flag` (0 = closed), `version`, `protocol`, mouse position, frame size and offset, window height, `overlay_mode` (`OFF`; `EVERY_FRAME` for Simba, which draws without saying when; `ON_SERIAL`: upload when `overlay_serial` changes), `offsets_state` (`PENDING`, `FOUND`, `NOT_FOUND`), `capture_enabled` (frame image wanted: Simba sets it for its session, `injector screenshot` briefly), `overlay_serial` (bumped by the injector after each paint; its low bit picks the slot to show), `overlay_hash` (what `scene_tiles` last painted, so an identical frame is skipped), `cursor_style` (`OFF`, `CROSS`, `DOT`, `X`: what the agent draws at the mouse) `cursor_color` (RGB) and `cursor_size` (px; the loader sets all three through `injector::set_cursor`), `frame_serial` (bumped by the agent after each presented frame, wrapping; unaligned, so read it unaligned). `Header::from_bytes` rejects other protocols with `Incompatible`. |
| `work_event_name(pid)`, `worker_mutex_name(pid)` | The agent's worker. `AGENT_WORK_<pid>` is an auto-reset event the injector sets after writing a request into `LayoutMemory`'s single `worker_request` slot (`WorkerRequest`: a `worker_kind::*` and four `i32` arguments); the agent's listener thread waits on it with the restart and unload events, runs `decode::worker::serve` in-process and writes the answer to `worker_reply` (`WorkerReply`: `sequence` echoing the request's, `status`, `length`, then up to `WORKER_DATA_SIZE` bytes of postcard). Nothing waits for the game thread's next frame, which is what a window-message request costs (17 ms at 60 fps). `AGENT_WORKER_<pid>` is a named mutex the injector holds from writing the request to reading the answer, so any number of threads and processes can share the slot (async input never touches it: it posts window messages). The injector falls back to reading the memory itself when the worker doesn't answer within 50 ms, the answer is `TOO_BIG`, or the event is missing. `VOLCREN_NO_WORKER=1` forces that fallback, to compare the two paths. |
| `frame_event_name(pid)` | `AGENT_FRAME_<pid>`: auto-reset event the agent sets after each `frame_serial` bump. `injector::scene_tiles_next_frame` waits on it (in slices, re-reading `frame_serial`, so a wake taken by another waiter is still noticed) to paint the dev overlays once per frame. |
| `build_id(dll)` | FNV-1a hash of the DLL as 7 hex chars: the agent version. |
| `input` | Window message ids (`0x0400 + n`), the `SYNTHETIC` flag bit, mouse message range, modifier bits, `REAL_MOUSE_BLOCK_MS`. |
| `state.rs` | The state layout: `part::*` validity bitflags, skill names, game state ids, well-known varps, per-feature layouts (`PlayerLayout`, `CameraLayout`, `ChatLayout`, ...), sequence-locked read/write helpers. |
| `read.rs` | Reader side: the `Memory` trait, `read_string`. |

## Adding a worker read

1. The read is a plain function over a `Memory` and the `Layout`, in `src/decode` (the decoders and their serde data types live there, not in `ipc`).
2. Add a `worker_kind::*` id here, its `decode::worker` function, its required `part` flags in `decode::worker::parts` and an arm in `decode::worker::serve`.
3. In `src/injector/src/win.rs` call `ask_worker(pid, kind, args)`, then fall back to the same `decode::worker` function through `reader(..)`.
4. Bump `PROTOCOL_VERSION` (new kinds are a protocol change).

## Changing the protocol

1. Any change to a layout, a message id or a field's meaning bumps `PROTOCOL_VERSION`.
2. In the same change, update the agent writer, the injector reader (`src/decode/src/` and `src/injector/src/win.rs`) and, if Luau-visible types change, the Luau lib's `host.luau`.
3. Update the layout tests (`cargo test -p ipc`).

State writes are sequence-locked (`AtomicU32` with fences): readers detect a torn read and retry, so always read through these helpers.
