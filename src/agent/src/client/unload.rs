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
    hooks::{unhook_wgl_swap_buffers, unhook_wndproc, wndproc_can_be_restored},
    state::LAYOUT_MAP,
};
use crate::shared::{main::MODULE, memory::MEMORY_MANAGER};

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
    unhook_wndproc();
    unhook_wgl_swap_buffers();
    super::name::unhook_decoder();
    super::total_xp::unhook();
    super::packets::unhook();

    if !wait_for_hook_calls() {
        println!("[Agent]: Cannot unload: a hook call is still running. agent stays loaded with its hooks off.\r\n");
        return;
    }

    super::packets::close();
    if let Some(layout) = LAYOUT_MAP.get() {
        layout.close();
    }
    if let Some(memory) = MEMORY_MANAGER.get() {
        let mut memory = memory.lock().unwrap_or_else(|e| e.into_inner());
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
