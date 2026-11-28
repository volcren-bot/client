# ipc (`src/ipc`)

Shared-memory contract between the agent (writer, in the client) and the injector (reader; the loader reaches it through the injector). `no_std` + `alloc`, no dependencies. Layouts, names and constants are in the crate.

## Protocol version

- Any change to a layout, a message id, a `worker_kind`, or a field's meaning bumps `PROTOCOL_VERSION` (`lib.rs`).
- Update together in one change: the agent writer, the readers in `src/decode` and `src/injector/src/win.rs`, the layout tests (`cargo test -p ipc`), and `sdk/luau/modules/host.luau` if Luau-visible types change.
- `Header` and the state block both carry the version; readers reject a mismatch as `Incompatible`, which means a stale agent or injector (reload the agent).

## Concurrency invariants

- State (`AGENT_STATE_<pid>`) is sequence-locked (`AtomicU32` + fences). One writer per field group; readers detect torn reads and retry. Always read through the `Shared` accessors, never raw.
- Logs (packets, scripts, items, varcs) share `ring.rs`: writers on any thread claim a sequence with `fetch_add`, blank the slot's `commit`, fill, then publish the sequence in `commit`. Readers in other processes copy lock-free; an entry counts only if `commit` matched before and after the copy, otherwise it is counted as dropped. `ring::Header::is_compatible` checks slot count and size.
- Each log exists only when its offset rule matched; readers must handle its absence.
- Log subscribers: a reader creates an auto-reset wake event, claims a bit in `subscribers`, and blocks. The agent signals matching subscribers per entry and clears bits whose event it can no longer open. Packet subscribers also set a per-opcode `Filter`.
- `InterfacesLayout::ui_scale` 0 means the client has no UI scale (builds before 2026-09) and readers use 1.
- Packet header `Movement` is seqlocked with one writer (the receive hook). `layouts` are written once at hook time, then `layouts_ready` is set; read nothing before it.
- A layout slot may carry a `packets::Name` and an `order`: entry `i` is the field index of the name's role `i` (`Name::roles`). The agent publishes a name only when its role count matches, and `decode_named` turns a payload into role values; `MESSAGE_GAME` has no fixed layout and is decoded from its known shape.
- Hider: the injector writes `hider_request` and the agent adopts the newest one at its next frame, then answers `hider_status` with that request's sequence. The status mask is what is applied: the agent clears a category when a script reaches for something it hides, so readers show the status, not what they asked for.
- Overlay is double-buffered: the injector paints the slot the agent isn't showing, then bumps `overlay_serial`; the agent shows slot `overlay_serial & 1`, so it never shows a half-painted overlay. Each slot's row stride is the viewport size it was painted at, stored in `overlay_sizes[slot]` before the serial bump; the viewport can change between paints, so readers and the injector's copy of the shown slot use that size, never the current one.
- Overlay owner: each paint writes the painter's pid to `overlay_writer`, and the agent shows the overlay only while that process is alive, as it does for world shapes, so a killed run leaves nothing on screen. A painter keeps the shown pixels only when it is the current writer; a new one starts from blank. The one-shot `injector overlay` command therefore draws nothing that lasts; draw through `serve` or a run.
- Overlay layers: every non-transparent overlay pixel carries its layer in alpha's lowest bit (`ipc::tag_alpha`): 1 is UI, 0 is world. Every painter must tag every pixel it writes (a ±1 alpha change is invisible); an untagged painter's pixels would land on a random layer.
- `frame_serial` is unaligned in the packed `Header`: read it unaligned. Waiters on `AGENT_FRAME_<pid>` re-read `frame_serial` in slices because another waiter may consume the auto-reset wake.

## World shapes (`world_shapes.rs`)

- World drawings are retained shapes, not pixels: writers publish `decode::shapes::Shape`s (postcard) and the agent projects them every frame with that frame's camera, so they follow the camera with no repaint and no writer awake. Points are in world units (128 per tile from tile 0), not scene-local, so a scene reload doesn't move them.
- `AGENT_WORLD_<pid>_<protocol>` is a mapping of its own, created by the agent, so a hot reload never has to grow `AGENT_DATA_<pid>`; the protocol in the name keeps an agent of another protocol from mapping a different layout. A writer that can't open it is talking to an agent without world drawings.
- Two seqlocked channels, each with one writer at a time: `VIEW` for the loader's development layers (`scene_tiles`) and `SCRIPT` for the running script (`debug_shapes`). Each publish replaces the channel and records the writer's pid; the agent drops a channel's shapes once that process has exited, so a killed run or helper leaves nothing on screen. A channel's writer must therefore stay alive while its shapes should show.
- The label fonts (`decode::font::Font::encode_all`: `p12_full`, then the bold `b12_full`) are published once into the font block by the first writer that sends a label, since only the injector reads the disk cache. Labels draw nothing until they are there.
- A `Loc` shape names a loc by kind, id and world tile; the agent draws the model the client drew for it that frame (`model::served`, the worker's own model path), so it moves with the loc's animation and shows nothing while the loc isn't drawn.

## Proxy setting (`proxy.rs`)

- `AGENT_PROXY_<pid>` is one 4096-byte page in every version, so any release's agent maps whatever another release created. It has its own `proxy::VERSION`, not `PROTOCOL_VERSION`: the loader writes it for agents of older pinned releases too, and asks a pinned injector's `proxy-version` before trusting it. Bump `proxy::VERSION` for any change to it.
- The loader's side (one writer) stamps the version and writes the request seqlocked; never written (version and sequence 0) means off. The agent never writes the request; it answers in `Status` with atomics (several hook threads update the counters), and `adopted` equal to the written sequence means the sweep for that setting is done.
- The mapping lives while anyone holds it: the agent from start to unload, the loader process (Windows) or its `serve` helper (Wine) from the first write, and `injector load` across a reload, so the new agent reads the setting before its first connection. The loader writes it again before each load it starts.

## Requests

- Worker path (preferred): the injector holds mutex `AGENT_WORKER_<pid>` from writing `worker_request` to reading `worker_reply`, so any threads/processes can share the single slot. It sets `AGENT_WORK_<pid>`; the agent's listener thread serves it in-process (no wait for the game thread's next frame, which costs ~17 ms at 60 fps) and sets `AGENT_WORK_DONE_<pid>`. A stale wake shows the wrong `sequence`; the injector waits again.
- Fallback: the injector reads memory itself if the worker doesn't answer in 50 ms, replies `TOO_BIG`, or an event is missing. `VOLCREN_NO_WORKER=1` forces it for comparison. Every worker read must therefore also work through `reader(..)`.
- Window-message requests (`input::*_MESSAGE`, and cache requests) run on the game thread. The agent answers only a slot's newest request, so injectors hold `AGENT_REQUEST_<pid>_<message>` around each ask; otherwise a second asker leaves the first waiting out its 3 s timeout. Injector-only; not a layout change.
- Async input posts window messages and never touches the worker slot.
- Menu (`worker_kind::MENU`): the client rebuilds the menu in stages during each frame, so a read from another thread catches a half-built list (measured 3.3% of reads while hovering an inventory item). The agent instead copies the finished menu at the end of every frame on the game thread (`menu::capture` right after the client's draw step, `client/frame.rs`; not the swap, which some renderer modes run on a render thread), and the worker answers from that copy: the entries, open flag, last click and selection, plus an open menu's rows, geometry and submenus, which need the client's layout routine and so can only be read there. The copy is refused once it is over a second old (the client stopped drawing). There is no fallback: a raw read is what tears, and there is no menu window message, so an unanswered menu read is an error.
- Locs (`worker_kind::LOCS`, arg `decode::locs::request`): the worker scans the scene itself and names each loc from the definitions the agent decoded from the client's disk cache at start (`client/definitions.rs`), resolving multilocs with live varp values. The client's own lookup and transform change its in-memory definition cache, so they may only run on the game thread; the disk copy needs neither. Until the decode finishes, a read answers `DefinitionsLoading`. A failed decode (no cache handle yet, or a group read while the client was writing it) answers `DefinitionsFailed`, and the first read 30 s after a failure starts the decode again; it never runs after unload has joined it. There is no fallback: an unanswered read is an error.
- Models (`worker_kind::MODEL`, args from `decode::model::args`: NPCs, players and locs on scene tiles): the agent keeps every model the client drew in the last frame (`client/drawn.rs`, see [agent-dll.md](agent-dll.md)), and the worker projects the asked one from that frame's placement and camera. A model the client didn't draw (off screen, culled) is `None`. The frame is refused once it is over a second old (the client stopped drawing), and there is no fallback: an unanswered read is an error.
- Act: `ACT_MESSAGE` delivers `act_request` like any window-message request and is answered in `act_state`, which the agent then rewrites every frame until the job ends. The injector polls `act_state` directly (no message) while it drives the mouse.

## Adding a worker read

1. Write it as a plain function over a `Memory` and the `Layout` in `src/decode`.
2. Add a `worker_kind::*` id, its `decode::worker` function, its `part` flags in `decode::worker::parts`, and an arm in `decode::worker::serve`.
3. In `src/injector/src/win.rs`, call `ask_worker(pid, kind, args)` and fall back to the same function through `reader(..)`.
4. Bump `PROTOCOL_VERSION`.
