//! The script log: the last scripts the client ran for an event, in a ring in its own shared memory (`AGENT_SCRIPTS_<pid>`).
//!
//! Same scheme as the packet log (`packets.rs`): writers claim a sequence number, blank the slot's `commit`, fill the slot and
//! publish the sequence number in `commit`; a reader keeps an entry only if `commit` named the sequence it wanted before and after
//! the copy.

use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicU64, Ordering},
};

pub fn script_runs_memory_name(pid: u32) -> String {
    format!("AGENT_SCRIPTS_{pid}")
}

/// Slots in the ring (a power of two): about 8 seconds of an idle logged in client, which runs about 2000 scripts a second.
pub const SLOT_COUNT: usize = 16384;

/// Arguments a slot keeps; `Entry::arg_count` is how many the event had.
pub const ARG_COUNT: usize = 8;

pub mod arg_kind {
    pub const INT: u8 = 1;
    pub const STRING: u8 = 2;
    /// Any other kind of argument (its value isn't kept).
    pub const OTHER: u8 = 0;
}

#[repr(C)]
pub struct Slot {
    commit: AtomicU64,
    time_ms: u64,
    event_type: i32,
    arg_count: u16,
    depth: u8,
    stored: u8,
    kinds: [u8; ARG_COUNT],
    values: [i32; ARG_COUNT],
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    /// Milliseconds since the agent created the log: for ordering and spacing entries, not a wall clock.
    pub time_ms: u64,
    pub event_type: i32,
    /// How many arguments the event had (`args` keeps the first `ARG_COUNT`).
    pub arg_count: u16,
    /// 0 for a script started by the client, 1 and up for one started while another was running.
    pub depth: u8,
    /// (kind from `arg_kind`, int value; 0 for anything but an int)
    pub args: Vec<(u8, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Pass this as `since` to get what comes after.
    pub next: u64,
    /// Entries between `since` and `next` that were overwritten (or torn by a writer) before they could be read.
    pub dropped: u64,
}

/// Sets up a freshly created (zeroed) mapping.
///
/// # Safety
/// `log` must point to `LOG_SIZE` writable bytes nobody else is using yet.
pub unsafe fn init(log: *mut Log) {
    write_volatile(addr_of_mut!((*log).slots), SLOT_COUNT as u32);
    write_volatile(addr_of_mut!((*log).slot_size), core::mem::size_of::<Slot>() as u32);
}

/// Whether the mapping was set up by an agent that agrees with this layout.
///
/// # Safety
/// `log` must point to `LOG_SIZE` readable bytes.
pub unsafe fn is_compatible(log: *const Log) -> bool {
    read_volatile(addr_of!((*log).slots)) == SLOT_COUNT as u32 && read_volatile(addr_of!((*log).slot_size)) == core::mem::size_of::<Slot>() as u32
}

/// Appends a script run; `args` are (kind, int value) of the first arguments, of which `ARG_COUNT` are kept.
///
/// # Safety
/// `log` must point to an initialised `Log` that stays mapped for the call.
pub unsafe fn push(log: *const Log, time_ms: u64, event_type: i32, depth: u8, arg_count: u16, args: &[(u8, i32)]) {
    let sequence = (*log).written.fetch_add(1, Ordering::AcqRel);
    let slot = addr_of!((*log).entries[sequence as usize % SLOT_COUNT]) as *mut Slot;
    let stored = args.len().min(ARG_COUNT);

    (*slot).commit.store(0, Ordering::Release);
    fence(Ordering::Release);
    write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
    write_volatile(addr_of_mut!((*slot).event_type), event_type);
    write_volatile(addr_of_mut!((*slot).arg_count), arg_count);
    write_volatile(addr_of_mut!((*slot).depth), depth);
    write_volatile(addr_of_mut!((*slot).stored), stored as u8);
    for (i, &(kind, value)) in args[..stored].iter().enumerate() {
        write_volatile(addr_of_mut!((*slot).kinds[i]), kind);
        write_volatile(addr_of_mut!((*slot).values[i]), value);
    }
    (*slot).commit.store(sequence + 1, Ordering::Release);
}

/// Copies out up to `limit` entries from sequence number `since` on.
///
/// A `since` ahead of the log (the agent was reloaded and started a new one) starts again from the oldest entry.
///
/// # Safety
/// `log` must point to a `Log` the agent initialised (see `is_compatible`) that stays mapped for the call.
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
            let stored = (read_volatile(addr_of!((*slot).stored)) as usize).min(ARG_COUNT);
            let args = (0..stored).map(|i| (read_volatile(addr_of!((*slot).kinds[i])), read_volatile(addr_of!((*slot).values[i])))).collect();
            Entry {
                sequence: at,
                time_ms: read_volatile(addr_of!((*slot).time_ms)),
                event_type: read_volatile(addr_of!((*slot).event_type)),
                arg_count: read_volatile(addr_of!((*slot).arg_count)),
                depth: read_volatile(addr_of!((*slot).depth)),
                args,
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
        assert_eq!(core::mem::size_of::<Slot>(), 64);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 64);
        assert_eq!(script_runs_memory_name(32), "AGENT_SCRIPTS_32");
    }

    #[test]
    fn reads_what_was_pushed_in_order() {
        let memory = Memory::new();
        unsafe {
            assert!(is_compatible(memory.0));
            push(memory.0, 10, 76, 0, 3, &[(arg_kind::INT, 6697), (arg_kind::STRING, 0), (arg_kind::INT, -1)]);
            push(memory.0, 11, 76, 1, 0, &[]);
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.next, page.dropped), (2, 0));
            assert_eq!(page.entries[0], Entry { sequence: 0, time_ms: 10, event_type: 76, arg_count: 3, depth: 0, args: alloc::vec![(1, 6697), (2, 0), (1, -1)] });
            assert_eq!((page.entries[1].depth, page.entries[1].args.len()), (1, 0));

            // reading on from `next` returns only what is new
            push(memory.0, 12, 12, 0, 1, &[(arg_kind::INT, 5)]);
            let page = read_since(memory.0, page.next, 10);
            assert_eq!((page.entries.len(), page.entries[0].sequence, page.next), (1, 2, 3));
        }
    }

    #[test]
    fn keeps_only_the_first_arguments_but_the_whole_count() {
        let memory = Memory::new();
        let args: Vec<(u8, i32)> = (0..20).map(|i| (arg_kind::INT, i)).collect();
        unsafe {
            push(memory.0, 0, 76, 0, 20, &args);
            let entry = &read_since(memory.0, 0, 1).entries[0];
            assert_eq!(entry.arg_count, 20);
            assert_eq!(entry.args, args[..ARG_COUNT]);
        }
    }

    #[test]
    fn limit_pages_and_overwritten_entries_are_counted() {
        let memory = Memory::new();
        unsafe {
            for i in 0..(SLOT_COUNT + 10) {
                push(memory.0, i as u64, 76, 0, 0, &[]);
            }
            let page = read_since(memory.0, 0, 5);
            assert_eq!((page.dropped, page.entries[0].sequence, page.entries.len(), page.next), (10, 10, 5, 15));
        }
    }

    #[test]
    fn a_since_from_a_previous_agent_starts_over() {
        let memory = Memory::new();
        unsafe {
            push(memory.0, 0, 76, 0, 0, &[]);
            let page = read_since(memory.0, 50_000, 10);
            assert_eq!((page.entries.len(), page.next, page.dropped), (1, 1, 0));
        }
    }

    #[test]
    fn an_unpublished_slot_ends_the_page() {
        let memory = Memory::new();
        unsafe {
            push(memory.0, 0, 76, 0, 0, &[]);
            (*memory.0).written.store(2, Ordering::Release);
            let page = read_since(memory.0, 0, 10);
            assert_eq!((page.entries.len(), page.next), (1, 1));
        }
    }
}
