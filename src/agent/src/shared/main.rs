use std::{ffi::c_void, ptr::null_mut};

use windows::{
    core::BOOL,
    Win32::{
        Foundation::{CloseHandle, HINSTANCE, HMODULE, HWND, TRUE},
        System::{
            LibraryLoader::DisableThreadLibraryCalls,
            Threading::{CreateThread, GetCurrentProcessId, THREAD_CREATION_FLAGS},
        },
    },
};

use crate::{
    client::hooks::start,
    shared::{memory::try_memory_manager, windows::get_jagrenderview},
};

pub static mut MODULE: HMODULE = HMODULE(null_mut());

const PROCESS_DETACH: u32 = 0;
const PROCESS_ATTACH: u32 = 1;

unsafe fn attach(hinst_dll: HINSTANCE, hwnd: HWND) {
    let _ = DisableThreadLibraryCalls(hinst_dll.into());
    match CreateThread(
        Some(null_mut()),
        0,
        Some(start),
        Some(hwnd.0),
        THREAD_CREATION_FLAGS(0),
        Some(null_mut()),
    ) {
        Ok(thread) => {
            let _ = CloseHandle(thread);
        }
        Err(_) => println!("[Agent]: Cannot start.\r\n"),
    }
}

fn detach() {
    if let Some(mut memory) = try_memory_manager() {
        unsafe { memory.close_map() };
    }
    println!("[Agent]: Detached.\r\n");
}

#[no_mangle]
pub extern "system" fn DllMain(
    hinst_dll: HINSTANCE,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> BOOL {
    unsafe { MODULE = HMODULE(hinst_dll.0) };
    match fdw_reason {
        PROCESS_ATTACH => {
            if let Some(hwnd) = get_jagrenderview(unsafe { GetCurrentProcessId() }) {
                unsafe { attach(hinst_dll, hwnd) };
            }
        }
        PROCESS_DETACH => detach(),
        _ => {}
    }
    TRUE
}
