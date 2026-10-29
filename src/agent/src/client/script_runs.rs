use std::{
    cell::Cell,
    ffi::{c_void, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicIsize, AtomicPtr, Ordering},
        OnceLock,
    },
    time::Instant,
};

use ipc::{
    script_runs::{arg_kind, phase, script_runs_memory_name, Log, Run, ARG_COUNT, LOG_SIZE},
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
    offsets::{ScriptRuns, ARG_KIND, ARG_SIZE, KIND_INT},
    own::OwnMemory,
    unload::HookCall,
};

// An event has a handful of arguments; a count beyond this is a misread, not an event.
const MAX_ARGS: i32 = 4096;

type RunScript = unsafe extern "system" fn(*mut c_void, *mut c_void, i32, i32);

static RUN: OnceLock<(GenericDetour<RunScript>, ScriptRuns)> = OnceLock::new();

// Where the game tick is: the global that holds the client object and the tick's offset in it (module base added).
static TICK: OnceLock<(u64, u32)> = OnceLock::new();

thread_local! {
    // How many scripts this thread is running through the hook: a script that starts another is its parent.
    static DEPTH: Cell<u8> = const { Cell::new(0) };
}

static LOG: AtomicPtr<Log> = AtomicPtr::new(std::ptr::null_mut());
static MAPPING: AtomicIsize = AtomicIsize::new(0);
static START: OnceLock<Instant> = OnceLock::new();

fn open_log() -> bool {
    if !LOG.load(Ordering::Acquire).is_null() {
        return true;
    }
    let name = CString::new(script_runs_memory_name(unsafe { GetCurrentProcessId() })).unwrap();
    unsafe {
        let Ok(mapping) = CreateFileMappingA(HANDLE::default(), None, PAGE_READWRITE, 0, LOG_SIZE as u32, PCSTR(name.as_ptr() as *const u8)) else {
            println!("[Agent]: Cannot create the script log shared memory.\r\n");
            return false;
        };
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, LOG_SIZE);
        if view.Value.is_null() {
            let _ = CloseHandle(mapping);
            println!("[Agent]: Cannot map the script log.\r\n");
            return false;
        }
        ipc::script_runs::init(view.Value as *mut Log);
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

fn log(run: &Run) {
    let log = LOG.load(Ordering::Acquire);
    if log.is_null() {
        return;
    }
    let time_us = START.get().map_or(0, |start| start.elapsed().as_micros() as u64);
    unsafe { ipc::script_runs::push(log, time_us, run) };
}

// The game tick now: the client object's tick counter, which the game thread (where scripts run) is the only writer of.
fn tick() -> Option<i32> {
    let &(client, offset) = TICK.get()?;
    let memory = OwnMemory;
    let client = memory.u64(client)?;
    (client != 0).then(|| memory.i32(client + offset as u64)).flatten()
}

// What the log records of an event: its type, how many arguments it has and the first few of them.
struct Event {
    event_type: i32,
    arg_count: u16,
    args: Vec<(u8, i32)>,
}

fn read_event(event: u64, rule: &ScriptRuns) -> Option<Event> {
    let memory = OwnMemory;
    let event_type = memory.i32(event + rule.event_type as u64)?;
    let count = memory.i32(event + rule.arg_count as u64)?;
    if !(0..=MAX_ARGS).contains(&count) {
        return None;
    }
    let args = memory.u64(event + rule.args as u64)?;
    let mut kept = Vec::new();
    if args != 0 {
        for i in 0..(count as usize).min(ARG_COUNT) {
            let at = args + i as u64 * ARG_SIZE as u64;
            let mut kind = [0u8];
            if !memory.read(at + ARG_KIND as u64, &mut kind) {
                break;
            }
            kept.push(match kind[0] {
                KIND_INT => (arg_kind::INT, memory.i32(at)?),
                2 => (arg_kind::STRING, 0),
                _ => (arg_kind::OTHER, 0),
            });
        }
    }
    Some(Event { event_type, arg_count: count as u16, args: kept })
}

unsafe extern "system" fn run(client: *mut c_void, event: *mut c_void, max_operations: i32, flags: i32) {
    let _call = HookCall::enter();
    let (detour, rule) = RUN.get().unwrap();
    let depth = DEPTH.with(|depth| depth.replace(depth.get().saturating_add(1)));
    // what was read before the script ran is logged again when it ends, so the two entries always match
    let read = if event.is_null() { None } else { catch_unwind(AssertUnwindSafe(|| read_event(event as u64, rule))).ok().flatten() };
    if let Some(Event { event_type, arg_count, args }) = &read {
        let _ = catch_unwind(AssertUnwindSafe(|| log(&Run { phase: phase::START, event_type: *event_type, tick: tick(), depth, arg_count: *arg_count, args })));
    }
    detour.call(client, event, max_operations, flags);
    DEPTH.with(|d| d.set(depth));
    if let Some(Event { event_type, arg_count, args }) = &read {
        let _ = catch_unwind(AssertUnwindSafe(|| log(&Run { phase: phase::END, event_type: *event_type, tick: tick(), depth, arg_count: *arg_count, args })));
    }
}

/// `tick` is the RVA of the global holding the client object and the offset of the tick counter in it, when both were found.
pub unsafe fn hook(module_base: u64, rule: ScriptRuns, tick: Option<(u32, u32)>) {
    if RUN.get().is_some() || !open_log() {
        return;
    }
    if let Some((client, offset)) = tick {
        let _ = TICK.set((module_base + client as u64, offset));
    }
    let function: RunScript = std::mem::transmute((module_base + rule.function as u64) as *const c_void);
    match GenericDetour::new(function, run) {
        Ok(detour) => {
            let _ = RUN.set((detour, rule));
        }
        Err(error) => {
            println!("[Agent]: Failed to hook the script runner: {error}.\r\n");
            return;
        }
    }
    match RUN.get().unwrap().0.enable() {
        Ok(()) => println!("[Agent]: Script log hooked.\r\n"),
        Err(error) => println!("[Agent]: Failed to enable the script hook: {error}.\r\n"),
    }
}

pub unsafe fn rehook() {
    if let Some((detour, _)) = RUN.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook() {
    if let Some((detour, _)) = RUN.get() {
        if detour.is_enabled() {
            match detour.disable() {
                Ok(()) => println!("[Agent]: Script log unhooked.\r\n"),
                Err(error) => println!("[Agent]: Failed to disable the script hook: {error}.\r\n"),
            }
        }
    }
}
