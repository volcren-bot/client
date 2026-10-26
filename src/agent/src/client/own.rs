use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    },
};

use ipc::{read::Memory, state::Layout};
use windows::Win32::System::{
    Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
    Threading::{GetCurrentProcess, GetCurrentThreadId},
};

pub struct OwnMemory;

impl Memory for OwnMemory {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let mut read = 0;
        let ok = unsafe {
            ReadProcessMemory(GetCurrentProcess(), address as *const c_void, buffer.as_mut_ptr().cast(), buffer.len(), Some(&mut read))
        };
        ok.is_ok() && read == buffer.len()
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let mut written = 0;
        let ok = unsafe {
            WriteProcessMemory(GetCurrentProcess(), address as *const c_void, bytes.as_ptr().cast(), bytes.len(), Some(&mut written))
        };
        ok.is_ok() && written == bytes.len()
    }
}

pub static LAYOUT: OnceLock<Layout> = OnceLock::new();

static GAME_THREAD: AtomicU32 = AtomicU32::new(0);

pub fn note_game_thread() {
    GAME_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

pub fn on_other_thread() -> bool {
    let game = GAME_THREAD.load(Ordering::Relaxed);
    game != 0 && game != unsafe { GetCurrentThreadId() }
}
