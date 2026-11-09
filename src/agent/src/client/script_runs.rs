use std::{
    cell::Cell,
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::OnceLock,
};

use ipc::{
    read::Memory,
    script_runs::{arg_kind, phase, script_runs_memory_name, Log, Run, ARG_COUNT, LOG_SIZE},
};
use retour::GenericDetour;

use super::{
    detour,
    log::{SharedLog, Tick},
    offsets::{ScriptRuns, ARG_KIND, ARG_SIZE, KIND_INT},
    own::OwnMemory,
    unload::HookCall,
};

const MAX_ARGS: i32 = 4096;

type RunScript = unsafe extern "system" fn(*mut c_void, *mut c_void, i32, i32);

static RUN: OnceLock<(GenericDetour<RunScript>, ScriptRuns)> = OnceLock::new();

static TICK: Tick = Tick::new();

thread_local! {
    static DEPTH: Cell<u8> = const { Cell::new(0) };
}

static LOG: SharedLog<Log> = SharedLog::new();

pub unsafe fn close() {
    LOG.close();
}

fn log(run: &Run) {
    if let Some(log) = LOG.get() {
        unsafe { ipc::script_runs::push(log, LOG.elapsed().as_micros() as u64, run) };
    }
}

struct Event {
    event_type: i32,
    arg_count: u16,
    args: [(u8, i32); ARG_COUNT],
    kept: usize,
}

impl Event {
    fn args(&self) -> &[(u8, i32)] {
        &self.args[..self.kept]
    }
}

fn read_event(event: u64, rule: &ScriptRuns) -> Option<Event> {
    let memory = OwnMemory;
    let event_type = memory.i32(event + rule.event_type as u64)?;
    let count = memory.i32(event + rule.arg_count as u64)?;
    if !(0..=MAX_ARGS).contains(&count) {
        return None;
    }
    let args = memory.u64(event + rule.args as u64)?;
    let mut read = [(arg_kind::OTHER, 0); ARG_COUNT];
    let mut kept = 0;
    if args != 0 {
        for (i, slot) in read.iter_mut().enumerate().take(count as usize) {
            let at = args + i as u64 * ARG_SIZE as u64;
            let mut kind = [0u8];
            if !memory.read(at + ARG_KIND as u64, &mut kind) {
                break;
            }
            *slot = match kind[0] {
                KIND_INT => (arg_kind::INT, memory.i32(at)?),
                2 => (arg_kind::STRING, 0),
                _ => (arg_kind::OTHER, 0),
            };
            kept = i + 1;
        }
    }
    Some(Event {
        event_type,
        arg_count: count as u16,
        args: read,
        kept,
    })
}

unsafe extern "system" fn run(
    client: *mut c_void,
    event: *mut c_void,
    max_operations: i32,
    flags: i32,
) {
    let _call = HookCall::enter();
    let (detour, rule) = RUN.get().unwrap();
    let depth = DEPTH.with(|depth| depth.replace(depth.get().saturating_add(1)));
    let read = if event.is_null() {
        None
    } else {
        catch_unwind(AssertUnwindSafe(|| read_event(event as u64, rule)))
            .ok()
            .flatten()
    };
    let log_phase = |phase| {
        if let Some(event) = &read {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                log(&Run {
                    phase,
                    event_type: event.event_type,
                    tick: TICK.read(),
                    depth,
                    arg_count: event.arg_count,
                    args: event.args(),
                })
            }));
        }
    };
    log_phase(phase::START);
    detour.call(client, event, max_operations, flags);
    DEPTH.with(|d| d.set(depth));
    log_phase(phase::END);
}

pub unsafe fn hook(module_base: u64, rule: ScriptRuns, tick: Option<(u32, u32)>) {
    if RUN.get().is_some()
        || !LOG.open(
            script_runs_memory_name,
            LOG_SIZE,
            "script",
            ipc::script_runs::init,
        )
    {
        return;
    }
    TICK.set(module_base, tick);
    let Some(created) =
        detour::create::<RunScript>(module_base + rule.function as u64, run, "script runner")
    else {
        return;
    };
    let (hooked, _) = RUN.get_or_init(|| (created, rule));
    detour::enable(hooked, "script hook", "Script log hooked");
}

pub unsafe fn unhook() {
    detour::disable(
        RUN.get().map(|(detour, _)| detour),
        "script hook",
        Some("Script log unhooked"),
    );
}
