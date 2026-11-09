use std::{
    ffi::{c_void, CString},
    ptr::NonNull,
    sync::{Arc, Mutex, OnceLock},
};

use ipc::state::{state_memory_name, Layout, LayoutMemory, Shared, LAYOUT_SIZE};
use windows::core::PCSTR;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Memory::{
        CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
        MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
    },
};

pub struct Mapping {
    view: NonNull<LayoutMemory>,
    mapping: HANDLE,
}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    fn create(pid: u32) -> Option<Self> {
        let name = CString::new(state_memory_name(pid)).ok()?;
        let mapping = unsafe {
            CreateFileMappingA(
                HANDLE::default(),
                None,
                PAGE_READWRITE,
                0,
                LAYOUT_SIZE as u32,
                PCSTR(name.as_ptr() as *const u8),
            )
        }
        .ok()?;
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, LAYOUT_SIZE) };
        let Some(block) = NonNull::new(view.Value.cast::<LayoutMemory>()) else {
            let _ = unsafe { CloseHandle(mapping) };
            return None;
        };
        unsafe { Shared::create(block) };
        Some(Self {
            view: block,
            mapping,
        })
    }

    pub fn shared(&self) -> Shared<'_> {
        unsafe { Shared::attach(self.view) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        let _ = unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.as_ptr().cast::<c_void>(),
            })
        };
        let _ = unsafe { CloseHandle(self.mapping) };
    }
}

pub struct LayoutMap {
    mapping: Mutex<Option<Arc<Mapping>>>,
}

impl LayoutMap {
    pub fn create(pid: u32) -> Option<Self> {
        Some(Self {
            mapping: Mutex::new(Some(Arc::new(Mapping::create(pid)?))),
        })
    }

    pub fn publish(&self, layout: &Layout) {
        if let Some(mapping) = self.memory() {
            mapping.shared().write(layout);
        }
    }

    pub fn memory(&self) -> Option<Arc<Mapping>> {
        self.mapping
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn close(&self) {
        self.mapping
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
    }
}

pub static LAYOUT_MAP: OnceLock<LayoutMap> = OnceLock::new();

pub fn mapping() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}
