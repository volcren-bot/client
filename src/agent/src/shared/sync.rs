use std::ptr::null_mut;

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{
            CreateEventW, OpenEventW, ResetEvent, SetEvent, WaitForMultipleObjects,
            EVENT_MODIFY_STATE, INFINITE,
        },
    },
};

use ipc::{unload_event_name, work_event_name, worker_reply_event_name};

use crate::client::{hooks::reenable_hooks, unload::unload};

fn to_wide_null_terminated(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn event_listener(hwnd: u64, pid: u32) {
    let event_name = to_wide_null_terminated(&format!("Global\\Restart-{}", hwnd));
    let event_name_ptr = PCWSTR(event_name.as_ptr());
    let unload_name = to_wide_null_terminated(&unload_event_name(pid));
    let work_name = to_wide_null_terminated(&work_event_name(pid));
    let reply_name = to_wide_null_terminated(&worker_reply_event_name(pid));

    unsafe {
        let restart = CreateEventW(Some(null_mut()), true, false, event_name_ptr)
            .expect("[Agent]: Failed to create/open event.\r\n");
        let unload_event = CreateEventW(None, false, false, PCWSTR(unload_name.as_ptr()))
            .expect("[Agent]: Failed to create the unload event.\r\n");
        let work_event = CreateEventW(None, false, false, PCWSTR(work_name.as_ptr()))
            .expect("[Agent]: Failed to create the work event.\r\n");
        let reply_event = CreateEventW(None, false, false, PCWSTR(reply_name.as_ptr()))
            .expect("[Agent]: Failed to create the worker reply event.\r\n");
        let events = [restart, unload_event, work_event];

        loop {
            let signaled = WaitForMultipleObjects(&events, false, INFINITE);
            if signaled == WAIT_OBJECT_0 {
                reenable_hooks();
                let _ = ResetEvent(restart);
            } else if signaled.0 == WAIT_OBJECT_0.0 + 1 {
                unload(&[restart, unload_event, work_event, reply_event]);
            } else if signaled.0 == WAIT_OBJECT_0.0 + 2 {
                let _ = std::panic::catch_unwind(|| crate::client::worker::on_work(reply_event));
            } else {
                println!("[Agent]: Waiting for events failed, stopping the listener.\r\n");
                return;
            }
        }
    }
}

pub fn signal_unload(pid: u32) -> bool {
    let name = to_wide_null_terminated(&unload_event_name(pid));
    unsafe {
        let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) else {
            return false;
        };
        let signaled = SetEvent(event).is_ok();
        let _ = CloseHandle(event);
        signaled
    }
}

pub fn call_event(hwnd: u64) {
    let event_name = to_wide_null_terminated(&format!("Global\\Restart-{}", hwnd));
    let event_name_ptr = PCWSTR(event_name.as_ptr());
    unsafe {
        let event = match OpenEventW(EVENT_MODIFY_STATE, false, event_name_ptr) {
            Ok(event) => event,
            Err(_) => return,
        };
        let _ = SetEvent(event);
        let _ = CloseHandle(event);
    }
}

pub fn close_event(hwnd: u64) {
    let event_name = to_wide_null_terminated(&format!("Global\\Restart-{}", hwnd));
    let event_name_ptr = PCWSTR(event_name.as_ptr());

    unsafe {
        let event = match OpenEventW(EVENT_MODIFY_STATE, false, event_name_ptr) {
            Ok(event) => event,
            Err(_) => return,
        };
        CloseHandle(event).expect("[Agent]: Failed to create/open event.\r\n");
    }
}
