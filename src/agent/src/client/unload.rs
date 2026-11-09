use std::{
    sync::atomic::{AtomicIsize, Ordering},
    thread::sleep,
    time::{Duration, Instant},
};

use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::LibraryLoader::FreeLibraryAndExitThread,
};

use super::{
    hooks::{unhook_all, wndproc_can_be_restored},
    state::LAYOUT_MAP,
};
use crate::shared::{main::MODULE, memory::try_memory_manager};

static ACTIVE_CALLS: AtomicIsize = AtomicIsize::new(0);

const TIMEOUT: Duration = Duration::from_secs(10);
const GRACE: Duration = Duration::from_millis(200);

pub struct HookCall;

impl HookCall {
    pub fn enter() -> Self {
        ACTIVE_CALLS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for HookCall {
    fn drop(&mut self) {
        ACTIVE_CALLS.fetch_sub(1, Ordering::SeqCst);
    }
}

pub unsafe fn unload(events: &[HANDLE]) {
    if !wndproc_can_be_restored() {
        println!("[Agent]: Cannot unload: the window procedure was hooked again after agent.\r\n");
        return;
    }

    println!("[Agent]: Unloading.\r\n");
    unhook_all();

    if !wait_for_hook_calls() {
        println!("[Agent]: Cannot unload: a hook call is still running. agent stays loaded with its hooks off.\r\n");
        return;
    }

    super::packets::close();
    super::script_runs::close();
    super::varc_log::close();
    super::item_log::close();
    if let Some(layout) = LAYOUT_MAP.get() {
        layout.close();
    }
    if let Some(mut memory) = try_memory_manager() {
        memory.mark_closed();
        memory.close_map();
    }
    for &event in events {
        let _ = CloseHandle(event);
    }

    println!("[Agent]: Unloaded.\r\n");
    FreeLibraryAndExitThread(MODULE, 0);
}

fn wait_for_hook_calls() -> bool {
    let start = Instant::now();
    loop {
        if ACTIVE_CALLS.load(Ordering::SeqCst) == 0 {
            sleep(GRACE);
            if ACTIVE_CALLS.load(Ordering::SeqCst) == 0 {
                return true;
            }
        }
        if start.elapsed() > TIMEOUT {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}
