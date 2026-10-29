use std::{
    ffi::{c_void, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicIsize, AtomicPtr, Ordering},
        OnceLock,
    },
    time::Instant,
};

use ipc::{
    packets::{direction, packets_memory_name, Log, LOG_SIZE, PAYLOAD_SIZE},
    read::Memory,
};
use retour::GenericDetour;
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Memory::{CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE},
            Threading::GetCurrentProcessId,
        },
    },
};

use super::{
    offsets::{Packets, BUFFER_SIZE},
    own::OwnMemory,
    unload::HookCall,
};

// A received payload is read from a buffer the client owns; this is far more than any packet (the biggest are map data, a few KB).
const MAX_PAYLOAD: u64 = 1 << 20;

type Receive = unsafe extern "system" fn(*mut c_void, *mut u8) -> u8;
type Send = unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8) -> *mut c_void;

static RECEIVE: OnceLock<(GenericDetour<Receive>, Packets)> = OnceLock::new();
static SEND: OnceLock<(GenericDetour<Send>, Packets)> = OnceLock::new();

static LOG: AtomicPtr<Log> = AtomicPtr::new(std::ptr::null_mut());
static MAPPING: AtomicIsize = AtomicIsize::new(0);
static START: OnceLock<Instant> = OnceLock::new();

fn open_log() -> bool {
    if !LOG.load(Ordering::Acquire).is_null() {
        return true;
    }
    let name = CString::new(packets_memory_name(unsafe { GetCurrentProcessId() })).unwrap();
    unsafe {
        let Ok(mapping) = CreateFileMappingA(HANDLE::default(), None, PAGE_READWRITE, 0, LOG_SIZE as u32, PCSTR(name.as_ptr() as *const u8)) else {
            println!("[Agent]: Cannot create the packet log shared memory.\r\n");
            return false;
        };
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, LOG_SIZE);
        if view.Value.is_null() {
            let _ = CloseHandle(mapping);
            println!("[Agent]: Cannot map the packet log.\r\n");
            return false;
        }
        ipc::packets::init(view.Value as *mut Log);
        MAPPING.store(mapping.0 as isize, Ordering::Release);
        LOG.store(view.Value as *mut Log, Ordering::Release);
    }
    START.get_or_init(Instant::now);
    true
}

/// Closes the log. The hooks must be off and no hook call running.
pub unsafe fn close() {
    let log = LOG.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !log.is_null() {
        let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: log.cast() });
    }
    let mapping = MAPPING.swap(0, Ordering::AcqRel);
    if mapping != 0 {
        let _ = CloseHandle(HANDLE(mapping as *mut c_void));
    }
}

fn log(direction: u8, opcode: u16, length: u32, payload: &[u8]) {
    let log = LOG.load(Ordering::Acquire);
    if log.is_null() {
        return;
    }
    let time_ms = START.get().map_or(0, |start| start.elapsed().as_millis() as u64);
    unsafe { ipc::packets::push(log, time_ms, direction, opcode, length, payload) };
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

// `out` was just filled by the receive function: the packet id, and a buffer object with its size and data pointer.
fn record_received(out: u64, rule: &Packets) -> Option<()> {
    let memory = OwnMemory;
    let opcode = memory.i32(out + rule.receive_opcode as u64)? as u32;
    let buffer = out + rule.receive_buffer as u64;
    let size = memory.u64(buffer + BUFFER_SIZE as u64)?;
    let data = memory.u64(buffer + rule.buffer_data as u64)?;
    if size > MAX_PAYLOAD {
        return None;
    }
    log(direction::RECEIVED, opcode as u16, size as u32, &read_payload(&memory, data, size, 0));
    Some(())
}

// `packet` is about to be written to the socket; its opcode byte is cipher masked, the plain id is its `type`.
fn record_sent(packet: u64, rule: &Packets) -> Option<()> {
    let memory = OwnMemory;
    let kind = memory.i32(packet + rule.send_type as u64)? as u32;
    let buffer = packet + rule.send_buffer as u64;
    let data = memory.u64(buffer + rule.buffer_data as u64)?;
    let written = memory.u64(buffer + rule.buffer_cursor as u64)?;
    if written == 0 || written > MAX_PAYLOAD {
        return None;
    }
    log(direction::SENT, kind as u16, (written - 1) as u32, &read_payload(&memory, data, written, 1));
    Some(())
}

unsafe extern "system" fn received(connection: *mut c_void, out: *mut u8) -> u8 {
    let _call = HookCall::enter();
    let (detour, rule) = RECEIVE.get().unwrap();
    let got = detour.call(connection, out);
    if got != 0 && !out.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| record_received(out as u64, rule)));
    }
    got
}

unsafe extern "system" fn sent(connection: *mut c_void, result: *mut c_void, packet: *const u8) -> *mut c_void {
    let _call = HookCall::enter();
    let (detour, rule) = SEND.get().unwrap();
    if !packet.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| record_sent(packet as u64, rule)));
    }
    detour.call(connection, result, packet)
}

pub unsafe fn hook(module_base: u64, rule: Packets) {
    if RECEIVE.get().is_some() || !open_log() {
        return;
    }
    let receive: Receive = std::mem::transmute((module_base + rule.receive as u64) as *const c_void);
    let send: Send = std::mem::transmute((module_base + rule.send as u64) as *const c_void);
    match (GenericDetour::new(receive, received), GenericDetour::new(send, sent)) {
        (Ok(receive), Ok(send)) => {
            let _ = (RECEIVE.set((receive, rule)), SEND.set((send, rule)));
        }
        (receive, send) => {
            for error in [receive.err(), send.err()].into_iter().flatten() {
                println!("[Agent]: Failed to hook the packet functions: {error}.\r\n");
            }
            return;
        }
    }
    match (RECEIVE.get().unwrap().0.enable(), SEND.get().unwrap().0.enable()) {
        (Ok(()), Ok(())) => println!("[Agent]: Packet log hooked.\r\n"),
        (receive, send) => println!("[Agent]: Failed to enable the packet hooks: {:?} {:?}.\r\n", receive.err(), send.err()),
    }
}

pub unsafe fn rehook() {
    if let Some((detour, _)) = RECEIVE.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
    if let Some((detour, _)) = SEND.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook() {
    if let Some((detour, _)) = RECEIVE.get() {
        if detour.is_enabled() {
            if let Err(error) = detour.disable() {
                println!("[Agent]: Failed to disable the packet receive hook: {error}.\r\n");
            }
        }
    }
    if let Some((detour, _)) = SEND.get() {
        if detour.is_enabled() {
            match detour.disable() {
                Ok(()) => println!("[Agent]: Packet log unhooked.\r\n"),
                Err(error) => println!("[Agent]: Failed to disable the packet send hook: {error}.\r\n"),
            }
        }
    }
}
