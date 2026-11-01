//! The ring the shared-memory logs (packets, items, varcs, script runs) are built on: writers claim a sequence number and
//! fill the slot it lands on, readers copy slots out and drop any that were overwritten or torn while they read.

use alloc::vec::Vec;
use core::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

/// A slot of a log: `commit` is 0 while a writer fills it, then the sequence number plus one.
pub trait Slot {
    fn commit(&self) -> &AtomicU64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<E> {
    pub entries: Vec<E>,
    /// Pass this as `since` to get what comes after.
    pub next: u64,
    /// Entries between `since` and `next` that were overwritten (or torn by a writer) before they could be read.
    pub dropped: u64,
}

impl<E> Default for Page<E> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            dropped: 0,
        }
    }
}

/// Claims the next sequence number and runs `write` on its slot, one of `count` at `entries`; `written` is the log's counter.
pub unsafe fn push<S: Slot>(
    written: &AtomicU64,
    entries: *const S,
    count: usize,
    write: impl FnOnce(*mut S),
) {
    let sequence = written.fetch_add(1, Ordering::AcqRel);
    let slot = entries.add(sequence as usize % count) as *mut S;

    (*slot).commit().store(0, Ordering::Release);
    fence(Ordering::Release);
    write(slot);
    (*slot).commit().store(sequence + 1, Ordering::Release);
}

/// Copies out up to `limit` entries from `since` on (restarting at the oldest if `since` is ahead), each made by `read`
/// from its sequence number and slot.
pub unsafe fn read_since<S: Slot, E>(
    written: &AtomicU64,
    entries: *const S,
    count: usize,
    since: u64,
    limit: usize,
    read: impl Fn(u64, *const S) -> E,
) -> Page<E> {
    let written = written.load(Ordering::Acquire);
    let mut at = if since > written { 0 } else { since };
    let mut page = Page::default();

    // anything older than the ring holds is gone
    let oldest = written.saturating_sub(count as u64);
    if at < oldest {
        page.dropped += oldest - at;
        at = oldest;
    }

    while at < written && page.entries.len() < limit {
        let slot = entries.add(at as usize % count);
        let before = (*slot).commit().load(Ordering::Acquire);
        if before < at + 1 {
            // claimed but not published yet: stop here, the next read picks it up
            break;
        }
        let entry = (before == at + 1).then(|| read(at, slot));
        fence(Ordering::Acquire);
        match entry {
            Some(entry) if (*slot).commit().load(Ordering::Acquire) == before => {
                page.entries.push(entry)
            }
            _ => page.dropped += 1,
        }
        at += 1;
    }
    page.next = at;
    page
}

/// Claims a free subscriber slot of `slots` (None when full), skipping slots whose event already exists.
pub fn subscribe(
    subscribers: &AtomicU32,
    slots: u32,
    mut create_event: impl FnMut(u32) -> bool,
) -> Option<u32> {
    for slot in 0..slots {
        let bit = 1u32 << slot;
        if subscribers.load(Ordering::Acquire) & bit != 0 || !create_event(slot) {
            continue;
        }
        if subscribers.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            return Some(slot);
        }
    }
    None
}

/// Gives a subscriber slot back.
pub fn unsubscribe(subscribers: &AtomicU32, slot: u32) {
    subscribers.fetch_and(!(1u32 << slot), Ordering::AcqRel);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{
        alloc::{alloc_zeroed, dealloc, Layout},
        vec,
    };

    const COUNT: usize = 8;

    #[repr(C)]
    struct TestSlot {
        commit: AtomicU64,
        value: u64,
    }

    impl Slot for TestSlot {
        fn commit(&self) -> &AtomicU64 {
            &self.commit
        }
    }

    struct Ring {
        written: AtomicU64,
        slots: *mut TestSlot,
    }

    impl Ring {
        fn new() -> Self {
            let slots = unsafe { alloc_zeroed(Layout::array::<TestSlot>(COUNT).unwrap()) };
            Self {
                written: AtomicU64::new(0),
                slots: slots.cast(),
            }
        }

        unsafe fn push(&self, value: u64) {
            push(&self.written, self.slots, COUNT, |slot| {
                (*slot).value = value;
            });
        }

        unsafe fn read(&self, since: u64, limit: usize) -> Page<(u64, u64)> {
            read_since(
                &self.written,
                self.slots,
                COUNT,
                since,
                limit,
                |at, slot| (at, (*slot).value),
            )
        }
    }

    impl Drop for Ring {
        fn drop(&mut self) {
            unsafe { dealloc(self.slots.cast(), Layout::array::<TestSlot>(COUNT).unwrap()) };
        }
    }

    #[test]
    fn reads_what_was_pushed_in_order_and_on_from_next() {
        let ring = Ring::new();
        unsafe {
            ring.push(10);
            ring.push(11);
            let page = ring.read(0, 10);
            assert_eq!(
                (page.entries, page.next, page.dropped),
                (vec![(0, 10), (1, 11)], 2, 0)
            );

            ring.push(12);
            let page = ring.read(page.next, 10);
            assert_eq!((page.entries, page.next), (vec![(2, 12)], 3));
        }
    }

    #[test]
    fn overwritten_entries_are_counted_and_an_old_since_starts_over() {
        let ring = Ring::new();
        unsafe {
            for i in 0..(COUNT as u64 + 3) {
                ring.push(i);
            }
            let page = ring.read(0, 5);
            assert_eq!(
                (
                    page.dropped,
                    page.entries[0].0,
                    page.entries.len(),
                    page.next
                ),
                (3, 3, 5, 8)
            );
            // a `since` ahead of the log (a previous agent's) restarts at the oldest
            let page = ring.read(90_000, 1);
            assert_eq!((page.dropped, page.entries[0].0), (3, 3));
        }
    }

    #[test]
    fn a_since_from_a_previous_agent_starts_over() {
        let ring = Ring::new();
        unsafe {
            ring.push(1);
            let page = ring.read(5000, 10);
            assert_eq!((page.entries.len(), page.next, page.dropped), (1, 1, 0));
        }
    }

    #[test]
    fn an_unpublished_slot_ends_the_page() {
        let ring = Ring::new();
        unsafe {
            ring.push(1);
            ring.written.store(2, Ordering::Release);
            let page = ring.read(0, 10);
            assert_eq!((page.entries.len(), page.next), (1, 1));
        }
    }

    #[test]
    fn subscriber_slots_are_claimed_and_given_back() {
        let subscribers = AtomicU32::new(0);
        assert_eq!(subscribe(&subscribers, 32, |_| true), Some(0));
        // slot 1 has a live event that isn't in the mask (another process's), so it is skipped
        assert_eq!(subscribe(&subscribers, 32, |slot| slot != 1), Some(2));
        assert_eq!(subscribers.load(Ordering::Acquire), 0b101);
        unsubscribe(&subscribers, 0);
        assert_eq!(subscribers.load(Ordering::Acquire), 0b100);
        for _ in 0..32 {
            subscribe(&subscribers, 32, |_| true);
        }
        assert_eq!(subscribe(&subscribers, 32, |_| true), None);
    }
}
