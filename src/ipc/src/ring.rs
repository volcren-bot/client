use alloc::vec::Vec;
use core::{
    mem::size_of,
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicU32, AtomicU64, Ordering},
};

pub trait Slot {
    fn commit(&self) -> &AtomicU64;
}

#[repr(C)]
pub(crate) struct Header {
    slots: u32,
    slot_size: u32,
    written: AtomicU64,
}

impl Header {
    pub(crate) unsafe fn init<S, const N: usize>(this: *mut Self) {
        write_volatile(addr_of_mut!((*this).slots), N as u32);
        write_volatile(addr_of_mut!((*this).slot_size), size_of::<S>() as u32);
    }

    pub(crate) unsafe fn is_compatible<S, const N: usize>(this: *const Self) -> bool {
        read_volatile(addr_of!((*this).slots)) == N as u32
            && read_volatile(addr_of!((*this).slot_size)) == size_of::<S>() as u32
    }

    pub(crate) fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<E> {
    pub entries: Vec<E>,
    pub next: u64,
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

pub(crate) fn prefix<const N: usize>(data: &[u8]) -> ([u8; N], u8) {
    const { assert!(N <= u8::MAX as usize) };
    let stored = data.len().min(N);
    let mut bytes = [0; N];
    bytes[..stored].copy_from_slice(&data[..stored]);
    (bytes, stored as u8)
}

pub(crate) unsafe fn push<S: Slot, const N: usize>(
    header: &Header,
    entries: *const [S; N],
    write: impl FnOnce(*mut S),
) {
    let sequence = header.written.fetch_add(1, Ordering::AcqRel);
    let slot = entries.cast::<S>().add(sequence as usize % N) as *mut S;

    (*slot).commit().store(0, Ordering::Release);
    fence(Ordering::Release);
    write(slot);
    (*slot).commit().store(sequence + 1, Ordering::Release);
}

pub(crate) unsafe fn read_since<S: Slot, E, const N: usize>(
    header: &Header,
    entries: *const [S; N],
    since: u64,
    limit: usize,
    read: impl Fn(u64, *const S) -> E,
) -> Page<E> {
    let written = header.written();
    let mut at = if since > written { 0 } else { since };
    let mut page = Page::default();

    let oldest = written.saturating_sub(N as u64);
    if at < oldest {
        page.dropped += oldest - at;
        at = oldest;
    }
    page.entries
        .reserve_exact(limit.min((written - at) as usize));

    while at < written && page.entries.len() < limit {
        let slot = entries.cast::<S>().add(at as usize % N);
        let before = (*slot).commit().load(Ordering::Acquire);
        if before < at + 1 {
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

pub(crate) fn subscribe(
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

pub(crate) fn unsubscribe(subscribers: &AtomicU32, slot: u32) {
    subscribers.fetch_and(!(1u32 << slot), Ordering::AcqRel);
}

#[cfg(test)]
pub(crate) struct Zeroed<T>(pub *mut T);

#[cfg(test)]
impl<T> Zeroed<T> {
    pub fn new() -> Self {
        Self(unsafe { alloc::alloc::alloc_zeroed(alloc::alloc::Layout::new::<T>()) }.cast())
    }
}

#[cfg(test)]
impl<T> Drop for Zeroed<T> {
    fn drop(&mut self) {
        unsafe { alloc::alloc::dealloc(self.0.cast(), alloc::alloc::Layout::new::<T>()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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
        header: Header,
        slots: Zeroed<[TestSlot; COUNT]>,
    }

    impl Ring {
        fn new() -> Self {
            Self {
                header: Header {
                    slots: 0,
                    slot_size: 0,
                    written: AtomicU64::new(0),
                },
                slots: Zeroed::new(),
            }
        }

        unsafe fn push(&self, value: u64) {
            push(&self.header, self.slots.0, |slot| {
                (*slot).value = value;
            });
        }

        unsafe fn read(&self, since: u64, limit: usize) -> Page<(u64, u64)> {
            read_since(&self.header, self.slots.0, since, limit, |at, slot| {
                (at, (*slot).value)
            })
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
            ring.header.written.store(2, Ordering::Release);
            let page = ring.read(0, 10);
            assert_eq!((page.entries.len(), page.next), (1, 1));
        }
    }

    #[test]
    fn layout_is_stamped_and_checked() {
        let header = Zeroed::<Header>::new();
        unsafe {
            assert!(!Header::is_compatible::<TestSlot, COUNT>(header.0));
            Header::init::<TestSlot, COUNT>(header.0);
            assert!(Header::is_compatible::<TestSlot, COUNT>(header.0));
            assert!(!Header::is_compatible::<TestSlot, 16>(header.0));
            assert!(!Header::is_compatible::<u8, COUNT>(header.0));
        }
    }

    #[test]
    fn prefix_keeps_the_first_bytes_and_says_how_many() {
        assert_eq!(prefix::<4>(b"ab"), (*b"ab\0\0", 2));
        assert_eq!(prefix::<4>(b"abcdef"), (*b"abcd", 4));
    }

    #[test]
    fn subscriber_slots_are_claimed_and_given_back() {
        let subscribers = AtomicU32::new(0);
        assert_eq!(subscribe(&subscribers, 32, |_| true), Some(0));
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
