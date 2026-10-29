use std::{ffi::c_void, ptr::null_mut, sync::OnceLock};

use windows::{
    core::BOOL,
    Win32::{
        Foundation::{CloseHandle, FALSE, HINSTANCE, HMODULE, HWND, LPARAM, TRUE, WPARAM},
        System::{
            LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW},
            Threading::{CreateThread, GetCurrentProcessId, THREAD_CREATION_FLAGS},
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
};

use crate::{
    client::hooks::start,
    shared::{memory::MEMORY_MANAGER, sync::close_event},
    simba::target::TARGETS,
};

use super::windows::{get_jagrenderview, WI_DETACH};

pub static mut MODULE: HMODULE = HMODULE(null_mut());

fn client_main(hinst_dll: HINSTANCE, hwnd: HWND, reason: u32) -> BOOL {
    match reason {
        1 => unsafe {
            let _ = DisableThreadLibraryCalls(hinst_dll.into());

            let mut buffer = [0u16; 260]; // MAX_PATH
            let len = GetModuleFileNameW(Some(MODULE), &mut buffer);

            if len > 0 {
                let path = String::from_utf16_lossy(&buffer[..len as usize]);
                let _ = DLL_NAME.set(path);
            }

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
            if let Some(memory) = MEMORY_MANAGER.get() {
                unsafe { memory.lock().unwrap_or_else(|e| e.into_inner()).close_map() };
            }

            close_event(hwnd.0 as u64);
            println!("[Agent]: Detached.\r\n");
            TRUE
        }
        _ => FALSE,
    }
}

pub static DLL_NAME: OnceLock<String> = OnceLock::new();

fn simba_main(hinst_dll: HINSTANCE, reason: u32) -> BOOL {
    match reason {
        1 => {
            let _ = unsafe { DisableThreadLibraryCalls(hinst_dll.into()) };
            TRUE
        }
        0 => {
            let targets = TARGETS.lock().unwrap();
            for target in targets.values() {
                let hwnd = HWND(target.hwnd as *mut c_void);
                let _ = unsafe { PostMessageW(Some(hwnd), WI_DETACH, WPARAM(0), LPARAM(0)) };
            }

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
        None => simba_main(hinst_dll, fdw_reason),
    }
}
