use std::{ffi::c_void, ptr::null_mut};

use windows::{
    core::BOOL,
    Win32::{
        Foundation::{CloseHandle, FALSE, HINSTANCE, HMODULE, HWND, TRUE},
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

fn client_main(hinst_dll: HINSTANCE, hwnd: HWND, reason: u32) -> BOOL {
    match reason {
        1 => unsafe {
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
            TRUE
        },
        0 => {
            if let Some(mut memory) = try_memory_manager() {
                unsafe { memory.close_map() };
            }
            println!("[Agent]: Detached.\r\n");
            TRUE
        }
        _ => FALSE,
    }
}

#[no_mangle]
pub extern "system" fn DllMain(
    hinst_dll: HINSTANCE,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> BOOL {
    unsafe { MODULE = HMODULE(hinst_dll.0) };

    let pid = unsafe { GetCurrentProcessId() };
    match get_jagrenderview(pid) {
        Some(hwnd) => client_main(hinst_dll, hwnd, fdw_reason),
        None => TRUE,
    }
}
