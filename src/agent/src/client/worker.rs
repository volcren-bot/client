use ipc::state::worker_status;
use windows::Win32::{Foundation::HANDLE, System::Threading::SetEvent};

use super::{
    own::{OwnMemory, LAYOUT},
    state::mapping,
};

pub fn on_work(done: HANDLE) {
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared
        .read_worker_request()
        .filter(|(s, _)| shared.worker_reply_answers() != Some(*s))
    else {
        return;
    };
    shared.write_worker_reply_with(|reply| {
        reply.sequence = sequence;
        reply.length = 0;
        reply.status = match LAYOUT.get() {
            None => worker_status::NOT_FOUND,
            Some(layout) => match decode::worker::serve(
                &OwnMemory,
                layout,
                request.kind,
                request.args,
                &mut reply.data,
            ) {
                Ok(length) => {
                    reply.length = length as u32;
                    worker_status::OK
                }
                Err(status) => status,
            },
        };
    });
    let _ = unsafe { SetEvent(done) };
}
