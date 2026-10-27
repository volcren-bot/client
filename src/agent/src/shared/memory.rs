use std::{
    ffi::CString,
    mem::size_of,
    ptr::{addr_of, addr_of_mut, null_mut, read_volatile, write_bytes},
    sync::{Mutex, OnceLock},
    thread::sleep,
    time::{Duration, Instant},
};
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::LibraryLoader::GetModuleFileNameW,
        System::Threading::{CreateEventW, SetEvent},
        System::Memory::{
            CreateFileMappingA, MapViewOfFile, OpenFileMappingA, UnmapViewOfFile,
            FILE_MAP_ALL_ACCESS, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
        },
    },
};
use ipc::{
    build_id, cursor_style, frame_event_name, initial_cursor_name, offsets_state, overlay_mode, shared_memory_name, Header, IMAGE_DATA_SIZE, OVERLAY_SLOTS, PROTOCOL_VERSION,
    INITIAL_CURSOR_SIZE, VERSION_LEN,
};

use super::main::MODULE;

pub fn version() -> [u8; VERSION_LEN] {
    static VERSION: OnceLock<[u8; VERSION_LEN]> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let mut buffer = [0u16; 1024];
        let len = unsafe { GetModuleFileNameW(Some(MODULE), &mut buffer) } as usize;
        let path = String::from_utf16_lossy(&buffer[..len]);
        match std::fs::read(&path) {
            Ok(dll) => build_id(&dll),
            Err(error) => {
                println!("[Agent]: Cannot read {path} to compute the version: {error}.\r\n");
                [b'0'; VERSION_LEN]
            }
        }
    })
}

#[repr(C, packed)]
pub struct SharedMemory {
    pub header: Header,
    pub img: [u8; IMAGE_DATA_SIZE],
    pub overlay: [[u8; IMAGE_DATA_SIZE]; OVERLAY_SLOTS],
}

const BUFFER_SIZE: usize = std::mem::size_of::<SharedMemory>();

pub struct MemoryManager {
    ptr: *mut SharedMemory,
    hmap: HANDLE,
    // Set after each presented frame (`ipc::frame_event_name`); only the client's own manager has one, not Simba's.
    frame_event: HANDLE,
}

unsafe impl Send for MemoryManager {}
unsafe impl Sync for MemoryManager {}

fn mapping_name(pid: u32) -> CString {
    CString::new(shared_memory_name(pid)).unwrap()
}

// The cursor the injector left for this client, if it did.
unsafe fn initial_cursor(pid: u32) -> (u8, [u8; 3], u8) {
    let default = (cursor_style::CROSS, [255, 0, 0], 20);
    let name = CString::new(initial_cursor_name(pid)).unwrap();
    let Ok(hmap) = OpenFileMappingA(FILE_MAP_READ.0, false, PCSTR(name.as_ptr() as *const u8)) else {
        return default;
    };
    let view = MapViewOfFile(hmap, FILE_MAP_READ, 0, 0, INITIAL_CURSOR_SIZE);
    let cursor = if view.Value.is_null() {
        default
    } else {
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, INITIAL_CURSOR_SIZE);
        let cursor = (bytes[0], [bytes[1], bytes[2], bytes[3]], bytes[4]);
        let _ = UnmapViewOfFile(view);
        cursor
    };
    let _ = CloseHandle(hmap);
    cursor
}

impl MemoryManager {
    pub unsafe fn create_map(pid: u32) -> Self {
        let version = version();
        let name = mapping_name(pid);
        let hmap = CreateFileMappingA(
            HANDLE::default(),
            None,
            PAGE_READWRITE,
            0,
            BUFFER_SIZE as u32,
            PCSTR(name.as_ptr() as *const u8),
        )
        .expect("[Agent]: Cannot initialize mappings.\r\n");

        let view = MapViewOfFile(hmap, FILE_MAP_ALL_ACCESS, 0, 0, BUFFER_SIZE);
        assert!(!view.Value.is_null(), "[Agent]: Cannot map memory.\r\n");

        let ptr = view.Value as *mut SharedMemory;

        let (cursor_style, cursor_color, cursor_size) = initial_cursor(pid);

        // Initialize default values
        (*ptr).header = Header {
            flag: 1,
            version,
            protocol: PROTOCOL_VERSION,
            mouse_x: -1,
            mouse_y: -1,
            width: -1,
            height: -1,
            offset_x: 0,
            offset_y: 0,
            window_height: -1,
            overlay_mode: overlay_mode::OFF,
            offsets_state: offsets_state::PENDING,
            capture_enabled: 0,
            overlay_serial: 0,
            overlay_hash: 0,
            cursor_style,
            cursor_color,
            cursor_size,
            frame_serial: 0,
        };

        let event_name: Vec<u16> = frame_event_name(pid).encode_utf16().chain(std::iter::once(0)).collect();
        let frame_event = CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())).unwrap_or_else(|error| {
            println!("[Agent]: Cannot create the frame event ({error}); overlays fall back to polling.\r\n");
            HANDLE::default()
        });

        Self { ptr, hmap, frame_event }
    }

    pub unsafe fn open_map(pid: u32, time: u64) -> Self {
        let start = Instant::now();
        let timeout = Duration::from_millis(time);
        let name = mapping_name(pid);

        let hmap = loop {
            let handle = OpenFileMappingA(
                FILE_MAP_ALL_ACCESS.0,
                false,
                PCSTR(name.as_ptr() as *const u8),
            );

            if let Ok(h) = handle {
                if !h.0.is_null() {
                    break h;
                }
            }

            if start.elapsed() >= timeout {
                panic!("[Agent]: Cannot open shared memory.\r\n");
            }

            sleep(Duration::from_millis(100));
        };

        let view = MapViewOfFile(hmap, FILE_MAP_ALL_ACCESS, 0, 0, BUFFER_SIZE);
        assert!(!view.Value.is_null(), "[Agent]: Cannot map memory.\r\n");

        let ptr = view.Value as *mut SharedMemory;

        let header = &(*ptr).header;
        assert!(
            header.flag == 1 && header.protocol == PROTOCOL_VERSION && header.version == version(),
            "[Agent]: Simba and the client are using different builds of the agent. Load this build into the client with loader, or restart the client.\r\n"
        );

        Self { ptr, hmap, frame_event: HANDLE::default() }
    }

    pub unsafe fn mark_closed(&self) {
        if !self.ptr.is_null() {
            (*self.ptr).header.flag = 0;
        }
    }

    pub unsafe fn close_map(&mut self) {
        if !self.ptr.is_null() {
            if UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as _,
            })
            .is_err()
            {
                println!("[Agent]: Failed to unmap memory map.\r\n");
            }
            self.ptr = null_mut();
        }

        if !self.hmap.is_invalid() {
            if CloseHandle(self.hmap).is_err() {
                println!("[Agent]: Failed to close memory map handle.\r\n");
            }
            self.hmap = HANDLE::default();
        }

        if !self.frame_event.is_invalid() {
            let _ = CloseHandle(self.frame_event);
            self.frame_event = HANDLE::default();
        }
    }

    /// Counts a frame the client presented and wakes whoever waits for the next one.
    pub unsafe fn frame_presented(&self) {
        if self.ptr.is_null() {
            return;
        }
        // packed and unaligned; only the game thread writes it, and a reader only looks for a change
        let at = addr_of_mut!((*self.ptr).header.frame_serial);
        at.write_unaligned(at.read_unaligned().wrapping_add(1));
        if !self.frame_event.is_invalid() {
            let _ = SetEvent(self.frame_event);
        }
    }

    pub unsafe fn image_ptr(&self) -> *mut u8 {
        (*self.ptr).img.as_ptr() as *mut u8
    }

    pub unsafe fn overlay_ptr(&self, slot: usize) -> *mut u8 {
        (*self.ptr).overlay[slot % OVERLAY_SLOTS].as_ptr() as *mut u8
    }

    pub unsafe fn clear_overlay(&self) {
        if !self.ptr.is_null() {
            for slot in 0..OVERLAY_SLOTS {
                write_bytes(self.overlay_ptr(slot), 0, IMAGE_DATA_SIZE);
            }
        }
    }

    pub unsafe fn overlay_mode(&self) -> u8 {
        (*self.ptr).header.overlay_mode
    }

    pub unsafe fn set_overlay_mode(&self, mode: u8) {
        (*self.ptr).header.overlay_mode = mode;
    }

    pub unsafe fn overlay_serial(&self) -> u32 {
        read_volatile(addr_of!((*self.ptr).header.overlay_serial))
    }

    pub unsafe fn cursor(&self) -> (u8, [u8; 3], u8) {
        let header = addr_of!((*self.ptr).header);
        (
            read_volatile(addr_of!((*header).cursor_style)),
            read_volatile(addr_of!((*header).cursor_color)),
            read_volatile(addr_of!((*header).cursor_size)),
        )
    }

    pub unsafe fn capture_enabled(&self) -> bool {
        (*self.ptr).header.capture_enabled != 0
    }

    pub unsafe fn set_capture_enabled(&self, enabled: bool) {
        (*self.ptr).header.capture_enabled = enabled as u8;
    }

    pub unsafe fn set_offsets_found(&self, found: bool) {
        (*self.ptr).header.offsets_state = if found {
            offsets_state::FOUND
        } else {
            offsets_state::NOT_FOUND
        };
    }

    pub unsafe fn get_mouse_position(&self) -> (i32, i32) {
        ((*self.ptr).header.mouse_x, (*self.ptr).header.mouse_y)
    }

    pub unsafe fn set_mouse_position(&self, x: i32, y: i32) {
        (*self.ptr).header.mouse_x = x;
        (*self.ptr).header.mouse_y = y;
    }

    pub unsafe fn get_dimensions(&self) -> (i32, i32) {
        ((*self.ptr).header.width, (*self.ptr).header.height)
    }

    pub unsafe fn set_viewport(&self, offset_x: i32, offset_y: i32, width: i32, height: i32, window_height: i32) {
        (*self.ptr).header.width = width;
        (*self.ptr).header.height = height;
        (*self.ptr).header.offset_x = offset_x;
        (*self.ptr).header.offset_y = offset_y;
        (*self.ptr).header.window_height = window_height;
    }
}

pub static MEMORY_MANAGER: OnceLock<Mutex<MemoryManager>> = OnceLock::new();

pub fn client_header(pid: u32) -> Option<Header> {
    let name = mapping_name(pid);
    unsafe {
        let hmap = OpenFileMappingA(FILE_MAP_READ.0, false, PCSTR(name.as_ptr() as *const u8)).ok()?;
        let view = MapViewOfFile(hmap, FILE_MAP_READ, 0, 0, size_of::<Header>());
        let header = (!view.Value.is_null()).then(|| (view.Value as *const Header).read_unaligned());
        if !view.Value.is_null() {
            let _ = UnmapViewOfFile(view);
        }
        let _ = CloseHandle(hmap);
        header.filter(|h| h.flag == 1)
    }
}
