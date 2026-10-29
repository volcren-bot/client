#![allow(clippy::missing_safety_doc)]

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr::NonNull;
use std::sync::{LazyLock, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use ipc::PROTOCOL_VERSION;
use shared::memory::{client_header, version};
use shared::sync::signal_unload;
use shared::windows::{get_jagrenderview, inject, is_input_enabled, open_console, toggle_input};
use simba::target::{SimbaTarget, TARGETS};

mod client;
mod shared;
mod simba;

// Pascal types as tuples (name, definition)
const PASCAL_TYPES: &[(&str, &str)] = &[("PHelloChar", "^Char;"), ("PTestInt", "^Int32;")];

// Pascal exports as (name, declaration); the name must match the dll function exactly
const PASCAL_EXPORTS: &[(&str, &str)] = &[
    (
        "Inject",
        "function Inject(dll: String; pid: UInt32): Boolean;",
    ),
    ("OpenConsole", "procedure OpenConsole();"),
    ("GetInputState", "function GetInputState(): Boolean;"),
    (
        "SetInputState",
        "function SetInputState(state: Boolean): Boolean;",
    ),
];

static WINDOW_HWND: LazyLock<Mutex<Option<u64>>> = LazyLock::new(|| Mutex::new(None));

// dll functions

/// `path` must be null or a valid nul-terminated string.
#[no_mangle]
pub unsafe extern "system" fn Inject(path: Option<NonNull<c_char>>, pid: u32) -> bool {
    let Some(path) = path else {
        println!("[Agent]: Invalid string\n");
        return false;
    };

    let Ok(module_path) = (unsafe { CStr::from_ptr(path.as_ptr()) }).to_str() else {
        println!("[Agent]: Invalid UTF-8\n");
        return false;
    };

    inject_into(module_path, pid)
}

fn inject_into(module_path: &str, pid: u32) -> bool {
    let hwnd = match get_jagrenderview(pid) {
        Some(h) => h.0 as u64,
        None => {
            println!("[Agent]: Couldn't find JagRenderView HWND\n");
            return false;
        }
    };

    let mut targets = TARGETS.lock().unwrap();
    targets
        .entry(pid)
        .and_modify(|target| target.hwnd = hwnd)
        .or_insert_with(|| {
            Box::new(SimbaTarget {
                pid,
                hwnd,
                keyboard: [false; 255],
                mouse: [false; 3],
            })
        });
    drop(targets);

    if let Some(header) = client_header(pid) {
        if header.protocol == PROTOCOL_VERSION && header.version == version() {
            return true;
        }
        let old = String::from_utf8_lossy(&header.version).into_owned();
        println!("[Agent]: The client runs agent {old}, replacing it.\n");
        if !unload_client(pid) {
            println!("[Agent]: agent {old} didn't unload from the client. Builds from before hot reload can't, so restart the client.\n");
            return false;
        }
    }

    inject(module_path, pid)
}

fn unload_client(pid: u32) -> bool {
    if !signal_unload(pid) {
        return false;
    }

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        sleep(Duration::from_millis(100));
        if client_header(pid).is_none() {
            sleep(Duration::from_millis(500));
            return true;
        }
    }
    false
}

#[no_mangle]
pub extern "system" fn OpenConsole() {
    let hwnd = WINDOW_HWND.lock().unwrap();

    if let Some(h) = *hwnd { open_console(h) }
}

#[no_mangle]
pub extern "system" fn GetInputState() -> bool {
    let hwnd = WINDOW_HWND.lock().unwrap();
    match *hwnd {
        Some(h) => is_input_enabled(h),
        None => false,
    }
}

#[no_mangle]
pub extern "system" fn SetInputState(state: bool) -> bool {
    let hwnd = WINDOW_HWND.lock().unwrap();
    match *hwnd {
        Some(h) => toggle_input(h, state),
        None => false,
    }
}
