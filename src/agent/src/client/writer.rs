use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

pub struct Writer(isize);

impl Writer {
    pub fn open(pid: u32) -> Option<Self> {
        unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
            .ok()
            .map(|handle| Self(handle.0 as isize))
    }

    pub fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(HANDLE(self.0 as *mut _), 0) == WAIT_OBJECT_0 }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(HANDLE(self.0 as *mut _)) };
    }
}

pub struct Owner {
    pid: u32,
    writer: Option<Writer>,
}

impl Owner {
    pub const fn new() -> Self {
        Self {
            pid: 0,
            writer: None,
        }
    }

    pub fn alive(&mut self, pid: u32) -> bool {
        if pid != self.pid {
            self.pid = pid;
            self.writer = Writer::open(pid);
        }
        self.writer.as_ref().is_some_and(|writer| !writer.exited())
    }
}
