use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Mutex, OnceLock},
};

use ipc::{
    packets::{
        direction, packet_wake_event_name, packets_memory_name, Log, Movement, LOG_SIZE, NO_TICK,
        PAYLOAD_SIZE,
    },
    read::{client, Memory},
    state::part,
};
use retour::GenericDetour;

use super::{
    detour,
    log::{wake, SharedLog},
    offsets::{PacketLayouts, Packets, BUFFER_SIZE},
    own::{OwnMemory, LAYOUT},
    unload::HookCall,
};

const MAX_PAYLOAD: u64 = 1 << 20;

type Receive = unsafe extern "system" fn(*mut c_void, *mut u8) -> u8;
type Send = unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8) -> *mut c_void;

static RECEIVE: OnceLock<(GenericDetour<Receive>, Packets)> = OnceLock::new();
static SEND: OnceLock<(GenericDetour<Send>, Packets)> = OnceLock::new();

static LOG: SharedLog<Log> = SharedLog::new();

static MOVEMENT: Mutex<Option<Movement>> = Mutex::new(None);

pub unsafe fn close() {
    LOG.close();
}

fn log(direction: u8, opcode: u16, length: u32, payload: &[u8]) {
    let Some(log) = LOG.get() else { return };
    unsafe {
        ipc::packets::push(
            log,
            LOG.elapsed().as_millis() as u64,
            direction,
            opcode,
            length,
            payload,
        );
        wake(
            ipc::packets::subscribers_for(log, direction, opcode),
            packet_wake_event_name,
            |slot| ipc::packets::unsubscribe(log, slot),
        );
    }
}

fn read_payload(memory: &OwnMemory, data: u64, length: u64, skip: u64) -> Vec<u8> {
    let wanted = length.min(skip + PAYLOAD_SIZE as u64);
    let mut bytes = vec![0u8; wanted as usize];
    if data == 0 || wanted == 0 || !memory.read(data, &mut bytes) {
        return Vec::new();
    }
    bytes.drain(..(skip as usize).min(bytes.len()));
    bytes
}

fn observe_player() -> Option<()> {
    let log = LOG.get()?;
    let layout = LAYOUT
        .get()
        .filter(|layout| layout.has(part::TICK_COUNT | part::PATH))?;
    let memory = OwnMemory;
    let tile = decode::state::server_tile(&memory, layout)?;
    let tick = memory.i32(client(&memory, layout)? + layout.tick_count as u64)?;
    let plane = tile.plane.unwrap_or(-1);
    let mut last = MOVEMENT.lock().ok()?;
    let (moved_tick, (from_x, from_y, from_plane)) = match *last {
        Some(seen) if (seen.x, seen.y, seen.plane) == (tile.x, tile.y, plane) => return Some(()),
        Some(seen) => (tick, (seen.x, seen.y, seen.plane)),
        None => (NO_TICK, (tile.x, tile.y, plane)),
    };
    let next = Movement {
        x: tile.x,
        y: tile.y,
        plane,
        moved_tick,
        from_x,
        from_y,
        from_plane,
    };
    *last = Some(next);
    unsafe { ipc::packets::set_movement(log, &next) };
    Some(())
}

fn record_received(out: u64, rule: &Packets) -> Option<()> {
    let memory = OwnMemory;
    let opcode = memory.i32(out + rule.receive_opcode as u64)? as u16;
    let buffer = out + rule.receive_buffer as u64;
    let size = memory.u64(buffer + BUFFER_SIZE as u64)?;
    let data = memory.u64(buffer + rule.buffer_data as u64)?;
    if size > MAX_PAYLOAD {
        return None;
    }
    log(
        direction::RECEIVED,
        opcode,
        size as u32,
        &read_payload(&memory, data, size, 0),
    );
    Some(())
}

fn record_sent(packet: u64, rule: &Packets) -> Option<()> {
    let memory = OwnMemory;
    let kind = memory.i32(packet + rule.send_type as u64)? as u16;
    let buffer = packet + rule.send_buffer as u64;
    let data = memory.u64(buffer + rule.buffer_data as u64)?;
    let written = memory.u64(buffer + rule.buffer_cursor as u64)?;
    if written == 0 || written > MAX_PAYLOAD {
        return None;
    }
    log(
        direction::SENT,
        kind,
        (written - 1) as u32,
        &read_payload(&memory, data, written, 1),
    );
    Some(())
}

unsafe extern "system" fn received(connection: *mut c_void, out: *mut u8) -> u8 {
    let _call = HookCall::enter();
    let (detour, rule) = RECEIVE.get().unwrap();
    let _ = catch_unwind(observe_player);
    let got = detour.call(connection, out);
    if got != 0 && !out.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| record_received(out as u64, rule)));
    }
    got
}

unsafe extern "system" fn sent(
    connection: *mut c_void,
    result: *mut c_void,
    packet: *const u8,
) -> *mut c_void {
    let _call = HookCall::enter();
    let (detour, rule) = SEND.get().unwrap();
    if !packet.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| record_sent(packet as u64, rule)));
    }
    detour.call(connection, result, packet)
}

pub unsafe fn hook(module_base: u64, rule: Packets, layouts: Option<&PacketLayouts>) {
    if RECEIVE.get().is_some()
        || !LOG.open(packets_memory_name, LOG_SIZE, "packet", ipc::packets::init)
    {
        return;
    }
    if let (Some(layouts), Some(log)) = (layouts, LOG.get()) {
        let published: Vec<_> = layouts
            .sizes
            .iter()
            .copied()
            .zip(layouts.layouts.iter().cloned())
            .collect();
        ipc::packets::publish_layouts(log, &published);
    }
    let what = "packet functions";
    let (Some(receive), Some(send)) = (
        detour::create::<Receive>(module_base + rule.receive as u64, received, what),
        detour::create::<Send>(module_base + rule.send as u64, sent, what),
    ) else {
        return;
    };
    let (receive, _) = RECEIVE.get_or_init(|| (receive, rule));
    let (send, _) = SEND.get_or_init(|| (send, rule));
    match (receive.enable(), send.enable()) {
        (Ok(()), Ok(())) => println!("[Agent]: Packet log hooked.\r\n"),
        (receive, send) => println!(
            "[Agent]: Failed to enable the packet hooks: {:?} {:?}.\r\n",
            receive.err(),
            send.err()
        ),
    }
}

pub unsafe fn unhook() {
    detour::disable(
        RECEIVE.get().map(|(detour, _)| detour),
        "packet receive hook",
        None,
    );
    detour::disable(
        SEND.get().map(|(detour, _)| detour),
        "packet send hook",
        Some("Packet log unhooked"),
    );
}
