#![no_std]

extern crate alloc;

use alloc::{format, string::String};

pub mod read;
pub mod state;

pub const PROTOCOL_VERSION: u8 = 45;

pub const SHARED_MEMORY_PREFIX: &str = "AGENT_DATA_";

pub fn shared_memory_name(pid: u32) -> String {
    format!("{SHARED_MEMORY_PREFIX}{pid}")
}

pub const IMAGE_DATA_SIZE: usize = 33177602;

pub const OVERLAY_SLOTS: usize = 2;

pub const fn overlay_offset(slot: usize) -> usize {
    HEADER_SIZE + IMAGE_DATA_SIZE * (1 + slot)
}

pub const VERSION_LEN: usize = 7;

pub fn build_id(dll: &[u8]) -> [u8; VERSION_LEN] {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in dll {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    let mut id = [0u8; VERSION_LEN];
    for (i, digit) in id.iter_mut().enumerate() {
        let nibble = (hash >> (60 - 4 * i)) & 0xf;
        *digit = b"0123456789abcdef"[nibble as usize];
    }
    id
}

// Created by the injector before the agent loads so the first frame draws the user's cursor: [style, r, g, b, size]; optional.
pub const INITIAL_CURSOR_SIZE: usize = 5;

pub fn initial_cursor_name(pid: u32) -> String {
    format!("AGENT_CURSOR_{pid}")
}

pub fn unload_event_name(pid: u32) -> String {
    format!("AGENT_UNLOAD_{pid}")
}

/// The event that wakes the agent's worker thread to answer read-only requests without waiting for a frame.
pub fn work_event_name(pid: u32) -> String {
    format!("AGENT_WORK_{pid}")
}

/// An auto-reset event the agent sets each time the client presents a frame, after bumping `Header::frame_serial`,
/// so a reader can do its work once per frame without polling.
pub fn frame_event_name(pid: u32) -> String {
    format!("AGENT_FRAME_{pid}")
}

/// Held by whoever has a request in the worker's single slot, from writing it to reading the answer.
pub fn worker_mutex_name(pid: u32) -> String {
    format!("AGENT_WORKER_{pid}")
}

pub mod input {
    pub const OPEN_CONSOLE_MESSAGE: u32 = 0x0400 + 1;
    pub const MODIFIERS_MESSAGE: u32 = 0x0400 + 3;
    pub const NAME_MESSAGE: u32 = 0x0400 + 5;
    pub const TOTAL_XP_MESSAGE: u32 = 0x0400 + 6;
    pub const ITEM_NAMES_MESSAGE: u32 = 0x0400 + 7;
    pub const GAME_VALS_MESSAGE: u32 = 0x0400 + 8;
    pub const PROJECT_MESSAGE: u32 = 0x0400 + 9;
    pub const MENU_MESSAGE: u32 = 0x0400 + 10;
    pub const MODEL_MESSAGE: u32 = 0x0400 + 11;
    pub const LOCS_MESSAGE: u32 = 0x0400 + 12;
    pub const CACHE_MESSAGE: u32 = 0x0400 + 13;
    pub const SYNTHETIC: u64 = 1 << 40;
    pub const MOUSE_FIRST: u32 = 0x0200;
    pub const MOUSE_LAST: u32 = 0x020e;
    pub const REAL_MOUSE_BLOCK_MS: u64 = 3000;
    pub const SHIFT: usize = 1 << 0;
    pub const CTRL: usize = 1 << 1;
    pub const ALT: usize = 1 << 2;
}

pub mod offsets_state {
    pub const PENDING: u8 = 0;
    pub const FOUND: u8 = 1;
    pub const NOT_FOUND: u8 = 2;
}

// How the agent treats the overlay: Simba's slot 0 is uploaded every frame, otherwise slot `overlay_serial & 1` when the serial changes.
pub mod overlay_mode {
    pub const OFF: u8 = 0;
    pub const EVERY_FRAME: u8 = 1;
    pub const ON_SERIAL: u8 = 2;
}

// What the agent draws at the mouse position.
pub mod cursor_style {
    pub const OFF: u8 = 0;
    pub const CROSS: u8 = 1;
    pub const DOT: u8 = 2;
    pub const X: u8 = 3;
}

pub const PREAMBLE_SIZE: usize = 9;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Header {
    pub flag: u8,
    pub version: [u8; VERSION_LEN],
    pub protocol: u8,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub width: i32,
    pub height: i32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub window_height: i32,
    pub overlay_mode: u8,
    pub offsets_state: u8,
    pub capture_enabled: u8,
    pub overlay_serial: u32,
    // What the injector last painted (0: unknown), read only by it to skip repainting an identical frame.
    pub overlay_hash: u64,
    pub cursor_style: u8,
    pub cursor_color: [u8; 3],
    pub cursor_size: u8,
    // Bumped by the agent each time the client presents a frame (wrapping), then `frame_event_name` is set.
    pub frame_serial: u32,
}

pub const HEADER_SIZE: usize = core::mem::size_of::<Header>();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatible {
    pub version: String,
    pub protocol: u8,
}

impl Header {
    pub fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> Result<Self, Incompatible> {
        let protocol = bytes[8];
        if protocol != PROTOCOL_VERSION {
            return Err(Incompatible {
                version: version_text(&bytes[1..8]),
                protocol,
            });
        }
        Ok(unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast()) })
    }

    pub fn version(&self) -> String {
        version_text(&self.version)
    }

    pub fn offset_y_top_down(&self) -> i32 {
        self.window_height - self.offset_y - self.height
    }

    pub fn offsets_found(&self) -> Option<bool> {
        match self.offsets_state {
            offsets_state::FOUND => Some(true),
            offsets_state::NOT_FOUND => Some(false),
            _ => None,
        }
    }
}

fn version_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        assert_eq!(core::mem::offset_of!(Header, protocol), PREAMBLE_SIZE - 1);
        assert_eq!(core::mem::offset_of!(Header, width), 17);
        assert_eq!(core::mem::offset_of!(Header, offset_x), 25);
        assert_eq!(core::mem::offset_of!(Header, window_height), 33);
        assert_eq!(core::mem::offset_of!(Header, offsets_state), 38);
        assert_eq!(core::mem::offset_of!(Header, capture_enabled), 39);
        assert_eq!(core::mem::offset_of!(Header, overlay_serial), 40);
        assert_eq!(core::mem::offset_of!(Header, overlay_hash), 44);
        assert_eq!(core::mem::offset_of!(Header, cursor_style), 52);
        assert_eq!(core::mem::offset_of!(Header, cursor_size), 56);
        assert_eq!(core::mem::offset_of!(Header, frame_serial), 57);
        assert_eq!(HEADER_SIZE, PREAMBLE_SIZE + 28 + 3 + 4 + 8 + 4 + 1 + 4);
    }

    #[test]
    fn offset_y_top_down_converts_from_opengls_bottom_up_origin() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[8] = PROTOCOL_VERSION;
        let mut header = Header::from_bytes(&bytes).unwrap();
        header.height = 880;
        header.offset_y = 940;
        header.window_height = 1820;
        assert_eq!(header.offset_y_top_down(), 0);
    }

    #[test]
    fn rejects_other_protocol_versions() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0] = 1;
        bytes[1..8].copy_from_slice(b"abcdef0");
        bytes[8] = PROTOCOL_VERSION + 1;
        bytes[core::mem::offset_of!(Header, offsets_state)] = offsets_state::FOUND;
        assert_eq!(
            Header::from_bytes(&bytes).err(),
            Some(Incompatible {
                version: "abcdef0".into(),
                protocol: PROTOCOL_VERSION + 1
            })
        );

        bytes[8] = PROTOCOL_VERSION;
        let header = Header::from_bytes(&bytes).unwrap();
        assert_eq!(header.version(), "abcdef0");
        assert_eq!(header.offsets_found(), Some(true));
    }

    #[test]
    fn build_ids_are_hex_and_change_with_the_dll() {
        let id = build_id(b"agent");
        assert!(id.iter().all(u8::is_ascii_hexdigit));
        assert_eq!(id, build_id(b"agent"));
        assert_ne!(id, build_id(b"other-dll"));
        assert_eq!(&build_id(b""), b"cbf29ce");
    }

    #[test]
    fn names_are_per_process() {
        assert_eq!(shared_memory_name(32), "AGENT_DATA_32");
        assert_eq!(frame_event_name(32), "AGENT_FRAME_32");
    }
}
