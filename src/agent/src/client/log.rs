use std::{
    ffi::{c_void, CString},
    sync::{
        atomic::{AtomicIsize, AtomicPtr, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

use ipc::read::Memory;
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Memory::{
                CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
                MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
            },
            Threading::{GetCurrentProcessId, OpenEventW, SetEvent, EVENT_MODIFY_STATE},
        },
    },
};

use super::own::OwnMemory;
use crate::shared::windows::wide;

pub struct SharedLog<T> {
    view: AtomicPtr<T>,
    mapping: AtomicIsize,
    start: OnceLock<Instant>,
}

impl<T> SharedLog<T> {
    pub const fn new() -> Self {
        Self {
            view: AtomicPtr::new(std::ptr::null_mut()),
            mapping: AtomicIsize::new(0),
            start: OnceLock::new(),
        }
    }

    pub fn open(
        &self,
        name: fn(u32) -> String,
        size: usize,
        what: &str,
        init: unsafe fn(*mut T),
    ) -> bool {
        if self.get().is_some() {
            return true;
        }
        let name = CString::new(name(unsafe { GetCurrentProcessId() })).unwrap();
        unsafe {
            let Ok(mapping) = CreateFileMappingA(
                HANDLE::default(),
                None,
                PAGE_READWRITE,
                0,
                size as u32,
                PCSTR(name.as_ptr().cast()),
            ) else {
                println!("[Agent]: Cannot create the {what} log shared memory.\r\n");
                return false;
            };
            let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, size);
            if view.Value.is_null() {
                let _ = CloseHandle(mapping);
                println!("[Agent]: Cannot map the {what} log.\r\n");
                return false;
            }
            init(view.Value.cast());
            self.mapping.store(mapping.0 as isize, Ordering::Release);
            self.view.store(view.Value.cast(), Ordering::Release);
        }
        self.start.get_or_init(Instant::now);
        true
    }

    pub fn get(&self) -> Option<*mut T> {
        let view = self.view.load(Ordering::Acquire);
        (!view.is_null()).then_some(view)
    }

    pub fn elapsed(&self) -> Duration {
        self.start.get().map_or(Duration::ZERO, Instant::elapsed)
    }

    pub unsafe fn close(&self) {
        let view = self.view.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !view.is_null() {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.cast() });
        }
        let mapping = self.mapping.swap(0, Ordering::AcqRel);
        if mapping != 0 {
            let _ = CloseHandle(HANDLE(mapping as *mut c_void));
        }
    }
}

pub unsafe fn wake(mut mask: u32, event_name: fn(u32, u32) -> String, mut gone: impl FnMut(u32)) {
    if mask == 0 {
        return;
    }
    let pid = GetCurrentProcessId();
    while mask != 0 {
        let slot = mask.trailing_zeros();
        mask &= mask - 1;
        let name = wide(&event_name(pid, slot));
        match OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) {
            Ok(event) => {
                let _ = SetEvent(event);
                let _ = CloseHandle(event);
            }
            Err(_) => gone(slot),
        }
    }
}

pub struct Tick(OnceLock<(u64, u32)>);

impl Tick {
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    pub fn set(&self, module_base: u64, tick: Option<(u32, u32)>) {
        if let Some((client, offset)) = tick {
            let _ = self.0.set((module_base + client as u64, offset));
        }
    }

    pub fn read(&self) -> Option<i32> {
        let &(client, offset) = self.0.get()?;
        let memory = OwnMemory;
        memory
            .ptr(client)
            .and_then(|client| memory.i32(client + offset as u64))
    }
}
