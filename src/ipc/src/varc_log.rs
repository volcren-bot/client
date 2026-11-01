//! The varc log: the last varcs the client's scripts changed, in a ring in shared memory (`AGENT_VARCS_<pid>`), using the packet log's scheme.

use crate::ring;
use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::AtomicU64,
};

pub fn varc_log_memory_name(pid: u32) -> String {
    format!("AGENT_VARCS_{pid}")
}

/// Slots in the ring (a power of two): scripts change varcs a few times a second when idle, many more while the interface moves.
pub const SLOT_COUNT: usize = 4096;

/// Bytes of a string value a slot keeps (of the new and of the old one); `Entry::*_length` is the whole length.
pub const TEXT_SIZE: usize = 60;

pub mod kind {
    pub const INT: u8 = 1;
    pub const STRING: u8 = 2;
}

#[repr(C)]
pub struct Slot {
    commit: AtomicU64,
    time_ms: u64,
    index: i32,
    old_int: i32,
    new_int: i32,
    kind: u8,
    /// whether the old value is known (a string's isn't until the hook has seen it set once)
    old_known: u8,
    new_stored: u8,
    old_stored: u8,
    new_length: u32,
    old_length: u32,
    new_text: [u8; TEXT_SIZE],
    old_text: [u8; TEXT_SIZE],
}

impl ring::Slot for Slot {
    fn commit(&self) -> &AtomicU64 {
        &self.commit
    }
}

#[repr(C)]
pub struct Log {
    slots: u32,
    slot_size: u32,
    /// Sequence numbers handed out so far: the next entry is number `written`.
    written: AtomicU64,
    reserved: [u8; 48],
    entries: [Slot; SLOT_COUNT],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

/// One varc change: ints in `old_int`/`new_int`, strings in `old_text`/`new_text` (first `TEXT_SIZE` bytes plus the length).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    /// Milliseconds since the agent created the log: for ordering and spacing entries, not a wall clock.
    pub time_ms: u64,
    pub index: i32,
    pub kind: u8,
    pub old_known: bool,
    pub old_int: i32,
    pub new_int: i32,
    pub old_length: u32,
    pub new_length: u32,
    pub old_text: Vec<u8>,
    pub new_text: Vec<u8>,
}

/// A change to push: the same fields as an `Entry` without the log's own bookkeeping.
pub struct Change<'a> {
    pub index: i32,
    pub kind: u8,
    pub old_known: bool,
    pub old_int: i32,
    pub new_int: i32,
    pub old_text: &'a [u8],
    pub old_length: u32,
    pub new_text: &'a [u8],
    pub new_length: u32,
}

pub type Page = ring::Page<Entry>;

/// Sets up a freshly created (zeroed) mapping of `LOG_SIZE` writable bytes.
pub unsafe fn init(log: *mut Log) {
    write_volatile(addr_of_mut!((*log).slots), SLOT_COUNT as u32);
    write_volatile(
        addr_of_mut!((*log).slot_size),
        core::mem::size_of::<Slot>() as u32,
    );
}

/// Whether the mapping was set up by an agent with this layout; `log` must point to `LOG_SIZE` readable bytes.
pub unsafe fn is_compatible(log: *const Log) -> bool {
    read_volatile(addr_of!((*log).slots)) == SLOT_COUNT as u32
        && read_volatile(addr_of!((*log).slot_size)) == core::mem::size_of::<Slot>() as u32
}

unsafe fn write_text(target: *mut [u8; TEXT_SIZE], text: &[u8]) -> u8 {
    let stored = text.len().min(TEXT_SIZE);
    for (i, &byte) in text[..stored].iter().enumerate() {
        write_volatile(addr_of_mut!((*target)[i]), byte);
    }
    stored as u8
}

unsafe fn read_text(source: *const [u8; TEXT_SIZE], stored: u8) -> Vec<u8> {
    (0..(stored as usize).min(TEXT_SIZE))
        .map(|i| read_volatile(addr_of!((*source)[i])))
        .collect()
}

/// Appends a change; `log` must point to an initialised `Log`.
pub unsafe fn push(log: *const Log, time_ms: u64, change: &Change) {
    ring::push(
        &(*log).written,
        addr_of!((*log).entries) as *const Slot,
        SLOT_COUNT,
        |slot| {
            write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
            write_volatile(addr_of_mut!((*slot).index), change.index);
            write_volatile(addr_of_mut!((*slot).kind), change.kind);
            write_volatile(addr_of_mut!((*slot).old_known), change.old_known as u8);
            write_volatile(addr_of_mut!((*slot).old_int), change.old_int);
            write_volatile(addr_of_mut!((*slot).new_int), change.new_int);
            write_volatile(addr_of_mut!((*slot).old_length), change.old_length);
            write_volatile(addr_of_mut!((*slot).new_length), change.new_length);
            let old_stored = write_text(addr_of_mut!((*slot).old_text), change.old_text);
            let new_stored = write_text(addr_of_mut!((*slot).new_text), change.new_text);
            write_volatile(addr_of_mut!((*slot).old_stored), old_stored);
            write_volatile(addr_of_mut!((*slot).new_stored), new_stored);
        },
    );
}

/// Copies out up to `limit` entries from `since` on (restarting at the oldest if `since` is ahead); `log` must point to an initialised `Log`.
pub unsafe fn read_since(log: *const Log, since: u64, limit: usize) -> Page {
    ring::read_since(
        &(*log).written,
        addr_of!((*log).entries) as *const Slot,
        SLOT_COUNT,
        since,
        limit,
        |at, slot| Entry {
            sequence: at,
            time_ms: read_volatile(addr_of!((*slot).time_ms)),
            index: read_volatile(addr_of!((*slot).index)),
            kind: read_volatile(addr_of!((*slot).kind)),
            old_known: read_volatile(addr_of!((*slot).old_known)) != 0,
            old_int: read_volatile(addr_of!((*slot).old_int)),
            new_int: read_volatile(addr_of!((*slot).new_int)),
            old_length: read_volatile(addr_of!((*slot).old_length)),
            new_length: read_volatile(addr_of!((*slot).new_length)),
            old_text: read_text(
                addr_of!((*slot).old_text),
                read_volatile(addr_of!((*slot).old_stored)),
            ),
            new_text: read_text(
                addr_of!((*slot).new_text),
                read_volatile(addr_of!((*slot).new_stored)),
            ),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::alloc::{alloc_zeroed, dealloc, Layout};

    struct Memory(*mut Log);

    impl Memory {
        fn new() -> Self {
            let layout = Layout::new::<Log>();
            let log = unsafe { alloc_zeroed(layout) } as *mut Log;
            unsafe { init(log) };
            Self(log)
        }
    }

    impl Drop for Memory {
        fn drop(&mut self) {
            unsafe { dealloc(self.0.cast(), Layout::new::<Log>()) };
        }
    }

    fn int(index: i32, old: i32, new: i32) -> Change<'static> {
        Change {
            index,
            kind: kind::INT,
            old_known: true,
            old_int: old,
            new_int: new,
            old_text: &[],
            old_length: 0,
            new_text: &[],
            new_length: 0,
        }
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 160);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 160);
    }

    #[test]
    fn reads_what_was_pushed_in_order() {
        let memory = Memory::new();
        unsafe {
            assert!(is_compatible(memory.0));
            push(memory.0, 10, &int(5, 0, 7));
            push(
                memory.0,
                11,
                &Change {
                    index: 6,
                    kind: kind::STRING,
                    old_known: false,
                    old_int: 0,
                    new_int: 0,
                    old_text: &[],
                    old_length: 0,
                    new_text: b"hello",
                    new_length: 5,
                },
            );
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.next, page.dropped), (2, 0));
            let first = &page.entries[0];
            assert_eq!(
                (
                    first.sequence,
                    first.time_ms,
                    first.index,
                    first.kind,
                    first.old_known,
                    first.old_int,
                    first.new_int
                ),
                (0, 10, 5, kind::INT, true, 0, 7)
            );
            let second = &page.entries[1];
            assert_eq!(
                (
                    second.kind,
                    second.old_known,
                    second.new_text.as_slice(),
                    second.new_length
                ),
                (kind::STRING, false, b"hello".as_slice(), 5)
            );

            // reading on from `next` returns only what is new
            push(memory.0, 12, &int(5, 7, 8));
            let page = read_since(memory.0, page.next, 10);
            assert_eq!(
                (page.entries.len(), page.entries[0].sequence, page.next),
                (1, 2, 3)
            );
        }
    }

    #[test]
    fn keeps_the_start_of_a_long_string_but_its_whole_length() {
        let memory = Memory::new();
        let text = [b'x'; 200];
        unsafe {
            push(
                memory.0,
                0,
                &Change {
                    index: 1,
                    kind: kind::STRING,
                    old_known: true,
                    old_int: 0,
                    new_int: 0,
                    old_text: b"old",
                    old_length: 3,
                    new_text: &text,
                    new_length: 200,
                },
            );
            let entry = &read_since(memory.0, 0, 1).entries[0];
            assert_eq!(
                (
                    entry.new_length,
                    entry.new_text.len(),
                    entry.old_text.as_slice()
                ),
                (200, TEXT_SIZE, b"old".as_slice())
            );
        }
    }
}
