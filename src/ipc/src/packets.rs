//! The packet log: the last packets received and sent, in a lock-free ring in shared memory (`AGENT_PACKETS_<pid>`).

use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicU64, Ordering},
};

pub fn packets_memory_name(pid: u32) -> String {
    format!("AGENT_PACKETS_{pid}")
}

/// Slots in the ring (a power of two): about 7 minutes of an idle client's traffic, a few seconds of a busy one.
pub const SLOT_COUNT: usize = 4096;

/// Bytes of a packet's payload a slot keeps; `Entry::length` is the whole length.
pub const PAYLOAD_SIZE: usize = 104;

pub mod direction {
    /// Server to client, as the game thread takes it off the queue: `opcode` is the server packet id, the payload has no framing.
    pub const RECEIVED: u8 = 0;
    /// Client to server: `opcode` is the client packet id, the payload is what follows the opcode byte.
    pub const SENT: u8 = 1;
}

#[repr(C)]
pub struct Slot {
    commit: AtomicU64,
    time_ms: u64,
    length: u32,
    opcode: u16,
    direction: u8,
    stored: u8,
    payload: [u8; PAYLOAD_SIZE],
}

#[repr(C)]
pub struct Log {
    slots: u32,
    slot_size: u32,
    /// Sequence numbers handed out so far: the next packet is number `written`.
    written: AtomicU64,
    reserved: [u8; 48],
    entries: [Slot; SLOT_COUNT],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    /// Milliseconds since the agent created the log: for ordering and spacing entries, not a wall clock.
    pub time_ms: u64,
    pub direction: u8,
    pub opcode: u16,
    pub length: u32,
    /// The first `PAYLOAD_SIZE` bytes of the payload.
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Pass this as `since` to get what comes after.
    pub next: u64,
    /// Entries between `since` and `next` that were overwritten (or torn by a writer) before they could be read.
    pub dropped: u64,
}

/// Sets up a freshly created (zeroed) mapping of `LOG_SIZE` writable bytes.
pub unsafe fn init(log: *mut Log) {
    write_volatile(addr_of_mut!((*log).slots), SLOT_COUNT as u32);
    write_volatile(addr_of_mut!((*log).slot_size), core::mem::size_of::<Slot>() as u32);
}

/// Whether the mapping was set up by an agent with this layout; `log` must point to `LOG_SIZE` readable bytes.
pub unsafe fn is_compatible(log: *const Log) -> bool {
    read_volatile(addr_of!((*log).slots)) == SLOT_COUNT as u32 && read_volatile(addr_of!((*log).slot_size)) == core::mem::size_of::<Slot>() as u32
}

/// Appends a packet, keeping the first `PAYLOAD_SIZE` bytes of `data`; `log` must point to an initialised `Log`.
pub unsafe fn push(log: *const Log, time_ms: u64, direction: u8, opcode: u16, length: u32, data: &[u8]) {
    let sequence = (*log).written.fetch_add(1, Ordering::AcqRel);
    let slot = addr_of!((*log).entries[sequence as usize % SLOT_COUNT]) as *mut Slot;
    let stored = data.len().min(PAYLOAD_SIZE);

    (*slot).commit.store(0, Ordering::Release);
    fence(Ordering::Release);
    write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
    write_volatile(addr_of_mut!((*slot).length), length);
    write_volatile(addr_of_mut!((*slot).opcode), opcode);
    write_volatile(addr_of_mut!((*slot).direction), direction);
    write_volatile(addr_of_mut!((*slot).stored), stored as u8);
    for (i, &byte) in data[..stored].iter().enumerate() {
        write_volatile(addr_of_mut!((*slot).payload[i]), byte);
    }
    (*slot).commit.store(sequence + 1, Ordering::Release);
}

/// Copies out up to `limit` entries from `since` on (restarting at the oldest if `since` is ahead); `log` must point to an initialised `Log`.
pub unsafe fn read_since(log: *const Log, since: u64, limit: usize) -> Page {
    let written = (*log).written.load(Ordering::Acquire);
    let mut at = if since > written { 0 } else { since };
    let mut page = Page::default();

    // anything older than the ring holds is gone
    let oldest = written.saturating_sub(SLOT_COUNT as u64);
    if at < oldest {
        page.dropped += oldest - at;
        at = oldest;
    }

    while at < written && page.entries.len() < limit {
        let slot = addr_of!((*log).entries[at as usize % SLOT_COUNT]);
        let before = (*slot).commit.load(Ordering::Acquire);
        if before < at + 1 {
            // claimed but not published yet: stop here, the next read picks it up
            break;
        }
        let entry = (before == at + 1).then(|| {
            let stored = (read_volatile(addr_of!((*slot).stored)) as usize).min(PAYLOAD_SIZE);
            let mut payload = Vec::with_capacity(stored);
            for i in 0..stored {
                payload.push(read_volatile(addr_of!((*slot).payload[i])));
            }
            Entry {
                sequence: at,
                time_ms: read_volatile(addr_of!((*slot).time_ms)),
                direction: read_volatile(addr_of!((*slot).direction)),
                opcode: read_volatile(addr_of!((*slot).opcode)),
                length: read_volatile(addr_of!((*slot).length)),
                payload,
            }
        });
        fence(Ordering::Acquire);
        match entry {
            Some(entry) if (*slot).commit.load(Ordering::Acquire) == before => page.entries.push(entry),
            _ => page.dropped += 1,
        }
        at += 1;
    }
    page.next = at;
    page
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

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 128);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 128);
        assert_eq!(packets_memory_name(32), "AGENT_PACKETS_32");
    }

    #[test]
    fn reads_what_was_pushed_in_order() {
        let memory = Memory::new();
        unsafe {
            assert!(is_compatible(memory.0));
            push(memory.0, 10, direction::RECEIVED, 40, 6, &[1, 2, 3, 4, 5, 6]);
            push(memory.0, 11, direction::SENT, 75, 0, &[]);
            let page = read_since(memory.0, 0, 10);
            assert_eq!(page.next, 2);
            assert_eq!(page.dropped, 0);
            assert_eq!(page.entries[0], Entry { sequence: 0, time_ms: 10, direction: direction::RECEIVED, opcode: 40, length: 6, payload: alloc::vec![1, 2, 3, 4, 5, 6] });
            assert_eq!(page.entries[1].opcode, 75);
            assert!(page.entries[1].payload.is_empty());

            // reading on from `next` returns only what is new
            push(memory.0, 12, direction::RECEIVED, 2, 0, &[]);
            let page = read_since(memory.0, page.next, 10);
            assert_eq!((page.entries.len(), page.entries[0].sequence, page.next), (1, 2, 3));
        }
    }

    #[test]
    fn keeps_only_the_start_of_a_long_payload_but_the_whole_length() {
        let memory = Memory::new();
        let data: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
        unsafe {
            push(memory.0, 0, direction::RECEIVED, 7, 200, &data);
            let entry = &read_since(memory.0, 0, 1).entries[0];
            assert_eq!(entry.length, 200);
            assert_eq!(entry.payload, data[..PAYLOAD_SIZE]);
        }
    }

    #[test]
    fn limit_pages_and_overwritten_entries_are_counted() {
        let memory = Memory::new();
        unsafe {
            for i in 0..(SLOT_COUNT + 10) {
                push(memory.0, i as u64, direction::RECEIVED, 1, 0, &[]);
            }
            let page = read_since(memory.0, 0, 5);
            assert_eq!(page.dropped, 10);
            assert_eq!(page.entries[0].sequence, 10);
            assert_eq!(page.entries.len(), 5);
            assert_eq!(page.next, 15);
        }
    }

    #[test]
    fn a_since_from_a_previous_agent_starts_over() {
        let memory = Memory::new();
        unsafe {
            push(memory.0, 0, direction::SENT, 75, 0, &[]);
            let page = read_since(memory.0, 5000, 10);
            assert_eq!((page.entries.len(), page.next, page.dropped), (1, 1, 0));
        }
    }

    #[test]
    fn an_unpublished_slot_ends_the_page() {
        let memory = Memory::new();
        unsafe {
            push(memory.0, 0, direction::SENT, 75, 0, &[]);
            // a writer that claimed number 1 but has not published it
            (*memory.0).written.store(2, Ordering::Release);
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.entries.len(), page.next), (1, 1));
        }
    }
}
