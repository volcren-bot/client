use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE},
    },
};

use ipc::{unload_event_name, work_event_name, worker_reply_event_name};

use super::windows::wide;
use crate::client::unload::unload;

pub fn event_listener(pid: u32) {
    let unload_name = wide(&unload_event_name(pid));
    let work_name = wide(&work_event_name(pid));
    let reply_name = wide(&worker_reply_event_name(pid));

    unsafe {
        let unload_event = CreateEventW(None, false, false, PCWSTR(unload_name.as_ptr()))
            .expect("[Agent]: Failed to create the unload event.\r\n");
        let work_event = CreateEventW(None, false, false, PCWSTR(work_name.as_ptr()))
            .expect("[Agent]: Failed to create the work event.\r\n");
        let reply_event = CreateEventW(None, false, false, PCWSTR(reply_name.as_ptr()))
            .expect("[Agent]: Failed to create the worker reply event.\r\n");
        let events = [unload_event, work_event];

        loop {
            let signaled = WaitForMultipleObjects(&events, false, INFINITE);
            if signaled == WAIT_OBJECT_0 {
                unload(&[unload_event, work_event, reply_event]);
            } else if signaled.0 == WAIT_OBJECT_0.0 + 1 {
                let _ = std::panic::catch_unwind(|| crate::client::worker::on_work(reply_event));
            } else {
                println!("[Agent]: Waiting for events failed, stopping the listener.\r\n");
                return;
            }
        }
    }
}
