//! The item log: the container slots the client changed, in a ring in shared memory (`AGENT_ITEMS_<pid>`), using the packet log's scheme.

use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicU32, AtomicU64, Ordering},
};

pub fn item_log_memory_name(pid: u32) -> String {
    format!("AGENT_ITEMS_{pid}")
}

/// The auto-reset event the agent sets on each logged change, for the reader that claimed `slot`.
pub fn item_wake_event_name(pid: u32, slot: u32) -> String {
    format!("AGENT_ITEMS_WAKE_{pid}_{slot}")
}

/// How many readers can wait for changes at once.
pub const SUBSCRIBER_SLOTS: u32 = 32;

/// Slots in the ring (a power of two): a bank opening sets every one of its slots, a few hundred to a thousand writes at once.
pub const SLOT_COUNT: usize = 8192;

/// A tick the hook could not read.
pub const NO_TICK: i32 = -1;

#[repr(C)]
pub struct Slot {
    commit: AtomicU64,
    time_ms: u64,
    tick: i32,
    container: i32,
    slot: i32,
    old_item: i32,
    old_quantity: i32,
    new_item: i32,
    new_quantity: i32,
    /// whether the container held the slot before: 0 when the write created the container
    old_known: u8,
}

#[repr(C)]
pub struct Log {
    slots: u32,
    slot_size: u32,
    /// Sequence numbers handed out so far: the next entry is number `written`.
    written: AtomicU64,
    /// Bit `n` is set while a reader waits on `item_wake_event_name(pid, n)`; the agent sets that event for every change it logs.
    subscribers: AtomicU32,
    reserved: [u8; 44],
    entries: [Slot; SLOT_COUNT],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

/// A slot write that changed something (an empty slot is item -1, quantity 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub tick: i32,
    pub container: i32,
    pub slot: i32,
    /// false when the container didn't exist: the write made it, and the old pair is the empty slot
    pub old_known: bool,
    pub old_item: i32,
    pub old_quantity: i32,
    pub new_item: i32,
    pub new_quantity: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    /// Milliseconds since the agent created the log: for ordering and spacing entries, not a wall clock.
    pub time_ms: u64,
    pub change: Change,
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

/// Appends a change; `log` must point to an initialised `Log`.
pub unsafe fn push(log: *const Log, time_ms: u64, change: &Change) {
    let sequence = (*log).written.fetch_add(1, Ordering::AcqRel);
    let slot = addr_of!((*log).entries[sequence as usize % SLOT_COUNT]) as *mut Slot;

    (*slot).commit.store(0, Ordering::Release);
    fence(Ordering::Release);
    write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
    write_volatile(addr_of_mut!((*slot).tick), change.tick);
    write_volatile(addr_of_mut!((*slot).container), change.container);
    write_volatile(addr_of_mut!((*slot).slot), change.slot);
    write_volatile(addr_of_mut!((*slot).old_item), change.old_item);
    write_volatile(addr_of_mut!((*slot).old_quantity), change.old_quantity);
    write_volatile(addr_of_mut!((*slot).new_item), change.new_item);
    write_volatile(addr_of_mut!((*slot).new_quantity), change.new_quantity);
    write_volatile(addr_of_mut!((*slot).old_known), change.old_known as u8);
    (*slot).commit.store(sequence + 1, Ordering::Release);
}

/// The readers waiting for changes, as a bit mask of their slots; `log` must point to an initialised `Log`.
pub unsafe fn subscribers(log: *const Log) -> u32 {
    (*log).subscribers.load(Ordering::Acquire)
}

/// Claims a free subscriber slot (None when full), skipping slots whose event already exists; `log` must be writable.
pub unsafe fn subscribe(log: *const Log, mut create_event: impl FnMut(u32) -> bool) -> Option<u32> {
    for slot in 0..SUBSCRIBER_SLOTS {
        let bit = 1u32 << slot;
        if (*log).subscribers.load(Ordering::Acquire) & bit != 0 || !create_event(slot) {
            continue;
        }
        if (*log).subscribers.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            return Some(slot);
        }
    }
    None
}

/// Gives a slot back; `log` must be writable.
pub unsafe fn unsubscribe(log: *const Log, slot: u32) {
    (*log).subscribers.fetch_and(!(1u32 << slot), Ordering::AcqRel);
}

/// The sequence number the next entry will get; `log` must point to an initialised `Log`.
pub unsafe fn written(log: *const Log) -> u64 {
    (*log).written.load(Ordering::Acquire)
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
        let entry = (before == at + 1).then(|| Entry {
            sequence: at,
            time_ms: read_volatile(addr_of!((*slot).time_ms)),
            change: Change {
                tick: read_volatile(addr_of!((*slot).tick)),
                container: read_volatile(addr_of!((*slot).container)),
                slot: read_volatile(addr_of!((*slot).slot)),
                old_known: read_volatile(addr_of!((*slot).old_known)) != 0,
                old_item: read_volatile(addr_of!((*slot).old_item)),
                old_quantity: read_volatile(addr_of!((*slot).old_quantity)),
                new_item: read_volatile(addr_of!((*slot).new_item)),
                new_quantity: read_volatile(addr_of!((*slot).new_quantity)),
            },
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

    fn change(slot: i32, new_item: i32) -> Change {
        Change { tick: 100, container: 93, slot, old_known: true, old_item: -1, old_quantity: 0, new_item, new_quantity: 1 }
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 48);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 48);
        assert_eq!(item_log_memory_name(32), "AGENT_ITEMS_32");
    }

    #[test]
    fn reads_what_was_pushed_in_order() {
        let memory = Memory::new();
        unsafe {
            assert!(is_compatible(memory.0));
            push(memory.0, 10, &change(0, 995));
            push(memory.0, 11, &Change { old_known: false, ..change(1, 1511) });
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.next, page.dropped), (2, 0));
            assert_eq!(page.entries[0], Entry { sequence: 0, time_ms: 10, change: change(0, 995) });
            assert!(!page.entries[1].change.old_known);
            assert_eq!(written(memory.0), 2);

            // reading on from `next` returns only what is new
            push(memory.0, 12, &change(2, 590));
            let page = read_since(memory.0, page.next, 10);
            assert_eq!((page.entries.len(), page.entries[0].sequence, page.next), (1, 2, 3));
        }
    }

    #[test]
    fn overwritten_entries_are_counted_and_an_old_since_starts_over() {
        let memory = Memory::new();
        unsafe {
            for i in 0..(SLOT_COUNT + 10) {
                push(memory.0, i as u64, &change(i as i32, 1));
            }
            let page = read_since(memory.0, 0, 5);
            assert_eq!((page.dropped, page.entries[0].sequence, page.entries.len(), page.next), (10, 10, 5, 15));
            let page = read_since(memory.0, 90_000, 1);
            assert_eq!((page.dropped, page.entries[0].sequence), (10, 10));
        }
    }

    #[test]
    fn subscriber_slots_are_claimed_and_given_back() {
        let memory = Memory::new();
        unsafe {
            assert_eq!(subscribers(memory.0), 0);
            assert_eq!(subscribe(memory.0, |_| true), Some(0));
            // slot 1 has a live event that isn't in the mask (another process's), so it is skipped
            assert_eq!(subscribe(memory.0, |slot| slot != 1), Some(2));
            assert_eq!(subscribers(memory.0), 0b101);
            unsubscribe(memory.0, 0);
            assert_eq!(subscribers(memory.0), 0b100);
            for _ in 0..SUBSCRIBER_SLOTS {
                subscribe(memory.0, |_| true);
            }
            assert_eq!(subscribe(memory.0, |_| true), None);
        }
    }

    #[test]
    fn an_unpublished_slot_ends_the_page() {
        let memory = Memory::new();
        unsafe {
            push(memory.0, 0, &change(0, 1));
            (*memory.0).written.store(2, Ordering::Release);
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.entries.len(), page.next), (1, 1));
        }
    }
}
