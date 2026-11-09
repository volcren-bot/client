//! The item log (`AGENT_ITEMS_<pid>`): container slot changes in a `ring`; `log` must be an initialised `Log` unless noted.

use crate::ring;
use alloc::{format, string::String};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
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

impl ring::Slot for Slot {
    fn commit(&self) -> &AtomicU64 {
        &self.commit
    }
}

#[repr(C)]
pub struct Log {
    header: ring::Header,
    /// Bit `n` is set while a reader waits on `item_wake_event_name(pid, n)` for every change.
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

pub type Page = ring::Page<Entry>;

/// Sets up a freshly created (zeroed) mapping of `LOG_SIZE` writable bytes.
pub unsafe fn init(log: *mut Log) {
    ring::Header::init::<Slot, SLOT_COUNT>(addr_of_mut!((*log).header));
}

/// Whether the mapping was set up by an agent with this layout; `log` must point to `LOG_SIZE` readable bytes.
pub unsafe fn is_compatible(log: *const Log) -> bool {
    ring::Header::is_compatible::<Slot, SLOT_COUNT>(addr_of!((*log).header))
}

/// Appends a change.
pub unsafe fn push(log: *const Log, time_ms: u64, change: &Change) {
    ring::push(&(*log).header, addr_of!((*log).entries), |slot| {
        write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
        write_volatile(addr_of_mut!((*slot).tick), change.tick);
        write_volatile(addr_of_mut!((*slot).container), change.container);
        write_volatile(addr_of_mut!((*slot).slot), change.slot);
        write_volatile(addr_of_mut!((*slot).old_item), change.old_item);
        write_volatile(addr_of_mut!((*slot).old_quantity), change.old_quantity);
        write_volatile(addr_of_mut!((*slot).new_item), change.new_item);
        write_volatile(addr_of_mut!((*slot).new_quantity), change.new_quantity);
        write_volatile(addr_of_mut!((*slot).old_known), change.old_known as u8);
    });
}

/// The readers waiting for changes, as a bit mask of their slots.
pub unsafe fn subscribers(log: *const Log) -> u32 {
    (*log).subscribers.load(Ordering::Acquire)
}

/// Claims a free subscriber slot (None when full), skipping slots whose event already exists; `log` must be writable.
pub unsafe fn subscribe(log: *const Log, create_event: impl FnMut(u32) -> bool) -> Option<u32> {
    ring::subscribe(&(*log).subscribers, SUBSCRIBER_SLOTS, create_event)
}

/// Gives a slot back; `log` must be writable.
pub unsafe fn unsubscribe(log: *const Log, slot: u32) {
    ring::unsubscribe(&(*log).subscribers, slot);
}

/// The sequence number the next entry will get.
pub unsafe fn written(log: *const Log) -> u64 {
    (*log).header.written()
}

/// Copies out up to `limit` entries from `since` on (from the oldest if `since` is ahead).
pub unsafe fn read_since(log: *const Log, since: u64, limit: usize) -> Page {
    ring::read_since(
        &(*log).header,
        addr_of!((*log).entries),
        since,
        limit,
        |at, slot| Entry {
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

    fn change(slot: i32, new_item: i32) -> Change {
        Change {
            tick: 100,
            container: 93,
            slot,
            old_known: true,
            old_item: -1,
            old_quantity: 0,
            new_item,
            new_quantity: 1,
        }
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 48);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 48);
    }

    #[test]
    fn reads_what_was_pushed_in_order() {
        let memory = memory();
        unsafe {
            assert!(is_compatible(memory.0));
            push(memory.0, 10, &change(0, 995));
            push(
                memory.0,
                11,
                &Change {
                    old_known: false,
                    ..change(1, 1511)
                },
            );
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.next, page.dropped), (2, 0));
            assert_eq!(
                page.entries[0],
                Entry {
                    sequence: 0,
                    time_ms: 10,
                    change: change(0, 995)
                }
            );
            assert!(!page.entries[1].change.old_known);
            assert_eq!(written(memory.0), 2);

            // reading on from `next` returns only what is new
            push(memory.0, 12, &change(2, 590));
            let page = read_since(memory.0, page.next, 10);
            assert_eq!(
                (page.entries.len(), page.entries[0].sequence, page.next),
                (1, 2, 3)
            );
        }
    }
}
