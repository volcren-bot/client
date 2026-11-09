use crate::ring;
use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::AtomicU64,
};

pub fn varc_log_memory_name(pid: u32) -> String {
    format!("AGENT_VARCS_{pid}")
}

pub const SLOT_COUNT: usize = 4096;

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
    header: ring::Header,
    reserved: [u8; 48],
    entries: [Slot; SLOT_COUNT],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
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

pub unsafe fn init(log: *mut Log) {
    ring::Header::init::<Slot, SLOT_COUNT>(addr_of_mut!((*log).header));
}

pub unsafe fn is_compatible(log: *const Log) -> bool {
    ring::Header::is_compatible::<Slot, SLOT_COUNT>(addr_of!((*log).header))
}

unsafe fn read_text(source: *const [u8; TEXT_SIZE], stored: u8) -> Vec<u8> {
    read_volatile(source)[..(stored as usize).min(TEXT_SIZE)].to_vec()
}

pub unsafe fn push(log: *const Log, time_ms: u64, change: &Change) {
    let (old_text, old_stored) = ring::prefix::<TEXT_SIZE>(change.old_text);
    let (new_text, new_stored) = ring::prefix::<TEXT_SIZE>(change.new_text);
    ring::push(&(*log).header, addr_of!((*log).entries), |slot| {
        write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
        write_volatile(addr_of_mut!((*slot).index), change.index);
        write_volatile(addr_of_mut!((*slot).kind), change.kind);
        write_volatile(addr_of_mut!((*slot).old_known), change.old_known as u8);
        write_volatile(addr_of_mut!((*slot).old_int), change.old_int);
        write_volatile(addr_of_mut!((*slot).new_int), change.new_int);
        write_volatile(addr_of_mut!((*slot).old_length), change.old_length);
        write_volatile(addr_of_mut!((*slot).new_length), change.new_length);
        write_volatile(addr_of_mut!((*slot).old_text), old_text);
        write_volatile(addr_of_mut!((*slot).new_text), new_text);
        write_volatile(addr_of_mut!((*slot).old_stored), old_stored);
        write_volatile(addr_of_mut!((*slot).new_stored), new_stored);
    });
}

pub unsafe fn read_since(log: *const Log, since: u64, limit: usize) -> Page {
    ring::read_since(
        &(*log).header,
        addr_of!((*log).entries),
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
    use crate::ring::Zeroed;

    fn memory() -> Zeroed<Log> {
        let memory = Zeroed::new();
        unsafe { init(memory.0) };
        memory
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
        let memory = memory();
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
        let memory = memory();
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
