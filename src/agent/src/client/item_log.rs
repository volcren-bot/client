use std::{
    ffi::{c_void, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicIsize, AtomicPtr, Ordering},
        OnceLock,
    },
    time::Instant,
};

use decode::world::{item_slot, Item};
use ipc::{
    item_log::{item_log_memory_name, item_wake_event_name, Change, Log, LOG_SIZE, NO_TICK},
    read::Memory,
    state::WorldLayout,
};
use retour::GenericDetour;
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Memory::{CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE},
            Threading::{GetCurrentProcessId, OpenEventW, SetEvent, EVENT_MODIFY_STATE},
        },
    },
};

use super::{offsets::ItemContainers, own::OwnMemory, unload::HookCall};

type SetSlot = unsafe extern "system" fn(i32, i32, i32, i32);

static SET_SLOT: OnceLock<GenericDetour<SetSlot>> = OnceLock::new();

// Where the container table and the game tick are (module base added).
static CONTAINERS: OnceLock<WorldLayout> = OnceLock::new();
static TICK: OnceLock<(u64, u32)> = OnceLock::new();

static LOG: AtomicPtr<Log> = AtomicPtr::new(std::ptr::null_mut());
static MAPPING: AtomicIsize = AtomicIsize::new(0);
static START: OnceLock<Instant> = OnceLock::new();

fn open_log() -> bool {
    if !LOG.load(Ordering::Acquire).is_null() {
        return true;
    }
    let name = CString::new(item_log_memory_name(unsafe { GetCurrentProcessId() })).unwrap();
    unsafe {
        let Ok(mapping) = CreateFileMappingA(HANDLE::default(), None, PAGE_READWRITE, 0, LOG_SIZE as u32, PCSTR(name.as_ptr() as *const u8)) else {
            println!("[Agent]: Cannot create the item log shared memory.\r\n");
            return false;
        };
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, LOG_SIZE);
        if view.Value.is_null() {
            let _ = CloseHandle(mapping);
            println!("[Agent]: Cannot map the item log.\r\n");
            return false;
        }
        ipc::item_log::init(view.Value as *mut Log);
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

fn log(change: &Change) {
    let log = LOG.load(Ordering::Acquire);
    if log.is_null() {
        return;
    }
    let time_ms = START.get().map_or(0, |start| start.elapsed().as_millis() as u64);
    unsafe {
        ipc::item_log::push(log, time_ms, change);
        wake_readers(log);
    }
}

// Wakes every waiting reader; one whose event is gone loses its slot.
unsafe fn wake_readers(log: *const Log) {
    let mut mask = ipc::item_log::subscribers(log);
    if mask == 0 {
        return;
    }
    let pid = GetCurrentProcessId();
    while mask != 0 {
        let slot = mask.trailing_zeros();
        mask &= mask - 1;
        let name: Vec<u16> = item_wake_event_name(pid, slot).encode_utf16().chain(std::iter::once(0)).collect();
        match OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) {
            Ok(event) => {
                let _ = SetEvent(event);
                let _ = CloseHandle(event);
            }
            Err(_) => ipc::item_log::unsubscribe(log, slot),
        }
    }
}

// The game tick now: the client object's tick counter, which the game thread (where the client changes items) writes.
fn tick() -> i32 {
    let Some(&(client, offset)) = TICK.get() else { return NO_TICK };
    let memory = OwnMemory;
    memory.u64(client).filter(|&client| client != 0).and_then(|client| memory.i32(client + offset as u64)).unwrap_or(NO_TICK)
}

unsafe extern "system" fn set_slot(container: i32, slot: i32, item: i32, quantity: i32) {
    let _call = HookCall::enter();
    let detour = SET_SLOT.get().unwrap();
    // what the slot held: None when the client has no such container yet (this write makes it)
    let old = catch_unwind(AssertUnwindSafe(|| CONTAINERS.get().map(|layout| item_slot(&OwnMemory, layout, container, slot)))).ok().flatten();
    detour.call(container, slot, item, quantity);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(old) = old else { return };
        let empty = Item { slot, id: -1, quantity: 0 };
        // a write of what the slot already held is not a change; one that makes the container always is
        if old.is_some_and(|old| old.id == item && old.quantity == quantity) {
            return;
        }
        let known = old.is_some();
        let old = old.unwrap_or(empty);
        log(&Change { tick: tick(), container, slot, old_known: known, old_item: old.id, old_quantity: old.quantity, new_item: item, new_quantity: quantity });
    }));
}

/// `containers` locates the container table; `tick` is the RVA of the client object global and the tick offset in it.
pub unsafe fn hook(module_base: u64, rule: u32, containers: ItemContainers, tick: Option<(u32, u32)>) {
    if SET_SLOT.get().is_some() || !open_log() {
        return;
    }
    let _ = CONTAINERS.set(WorldLayout {
        module_base,
        containers: containers.table,
        container_ids: containers.ids,
        container_quantities: containers.quantities,
        container_next: containers.next,
        ..Default::default()
    });
    if let Some((client, offset)) = tick {
        let _ = TICK.set((module_base + client as u64, offset));
    }
    let function: SetSlot = std::mem::transmute((module_base + rule as u64) as *const c_void);
    match GenericDetour::new(function, set_slot) {
        Ok(detour) => {
            let _ = SET_SLOT.set(detour);
        }
        Err(error) => {
            println!("[Agent]: Failed to hook the item container writes: {error}.\r\n");
            return;
        }
    }
    match SET_SLOT.get().unwrap().enable() {
        Ok(()) => println!("[Agent]: Item log hooked.\r\n"),
        Err(error) => println!("[Agent]: Failed to enable the item hook: {error}.\r\n"),
    }
}

pub unsafe fn rehook() {
    if let Some(detour) = SET_SLOT.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook() {
    if let Some(detour) = SET_SLOT.get() {
        if detour.is_enabled() {
            match detour.disable() {
                Ok(()) => println!("[Agent]: Item log unhooked.\r\n"),
                Err(error) => println!("[Agent]: Failed to disable the item hook: {error}.\r\n"),
            }
        }
    }
}
