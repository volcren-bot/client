//! The packet log: the last packets received and sent, in a lock-free ring in shared memory (`AGENT_PACKETS_<pid>`).

use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicI32, AtomicU32, AtomicU64, Ordering},
};

pub fn packets_memory_name(pid: u32) -> String {
    format!("AGENT_PACKETS_{pid}")
}

/// The auto-reset event the agent sets on each logged packet that the reader holding `slot` filters for.
pub fn packet_wake_event_name(pid: u32, slot: u32) -> String {
    format!("AGENT_PACKETS_WAKE_{pid}_{slot}")
}

/// How many readers can wait for packets at once.
pub const SUBSCRIBER_SLOTS: u32 = 32;

/// Words of a subscriber's filter: one bit per direction and opcode below 256 (bit `direction * 256 + opcode`).
pub const FILTER_WORDS: usize = 16;

pub type Filter = [u32; FILTER_WORDS];

/// The filter bit of a packet; opcodes from 256 up can't be filtered for (the client has 151 server packets).
pub fn filter_bit(direction: u8, opcode: u16) -> Option<usize> {
    (direction <= direction::SENT && opcode < 256).then(|| direction as usize * 256 + opcode as usize)
}

pub fn filter_has(filter: &Filter, direction: u8, opcode: u16) -> bool {
    filter_bit(direction, opcode).is_some_and(|bit| filter[bit / 32] & (1 << (bit % 32)) != 0)
}

/// Adds a packet to `filter`; false when the opcode can't be filtered for.
pub fn filter_add(filter: &mut Filter, direction: u8, opcode: u16) -> bool {
    let Some(bit) = filter_bit(direction, opcode) else { return false };
    filter[bit / 32] |= 1 << (bit % 32);
    true
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
    /// Bit `n` is set while a reader waits on `packet_wake_event_name(pid, n)`; the agent sets that event for the packets in `filters[n]`.
    subscribers: AtomicU32,
    /// The local player's movement as the receive hook saw it (`set_movement`): sequence-locked, odd while written, 0 until first written.
    movement_sequence: AtomicU32,
    movement: [AtomicI32; MOVEMENT_FIELDS],
    /// 1 once `publish_layouts` has filled `layouts`.
    layouts_ready: AtomicU32,
    reserved: [u8; 8],
    entries: [Slot; SLOT_COUNT],
    filters: [[AtomicU32; FILTER_WORDS]; SUBSCRIBER_SLOTS as usize],
    layouts: [LayoutSlot; LAYOUT_IDS],
}

/// Server packet ids a layout is kept for (the client has 151).
pub const LAYOUT_IDS: usize = 256;

/// Fields of one packet's layout that fit.
pub const LAYOUT_FIELDS: usize = 16;

/// `LayoutSlot::fields_count` for a packet whose layout isn't known.
const NO_LAYOUT: u8 = 0xff;

/// What the agent read from the client's code about one server packet: its size, and its fields as `encode_field` makes them.
#[repr(C)]
pub struct LayoutSlot {
    size: i32,
    fields_count: u8,
    fields: [[u8; 2]; LAYOUT_FIELDS],
    reserved: [u8; 3],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

const MOVEMENT_FIELDS: usize = 7;

/// A tick the hook could not read, or the tile not having changed since the hook first saw it.
pub const NO_TICK: i32 = -1;

/// The local player's server tile (world coordinates) and when it last changed, written by the receive hook before each packet is taken, so after the previous packet was handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Movement {
    pub x: i32,
    pub y: i32,
    /// -1 when the plane offset wasn't found
    pub plane: i32,
    /// the client's tick counter when the tile last changed (`NO_TICK` if it hasn't since the hook first saw it); Player Info is handled one tick count before the tick ends
    pub moved_tick: i32,
    /// the tile before that change
    pub from_x: i32,
    pub from_y: i32,
    pub from_plane: i32,
}

impl Movement {
    fn fields(&self) -> [i32; MOVEMENT_FIELDS] {
        [self.x, self.y, self.plane, self.moved_tick, self.from_x, self.from_y, self.from_plane]
    }

    /// Whether the tile changed in the last tick that ended, or in the one being received; `tick` is the client's tick counter now (one behind `moved_tick` means the counter was reset, so not moving).
    pub fn moving(&self, tick: i32) -> bool {
        self.moved_tick != NO_TICK && (0..=1).contains(&tick.wrapping_sub(self.moved_tick))
    }
}

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

/// How a field is stored on the wire, as the agent finds it in a read function: `width` bytes, each at a `shifts` position (in bytes) of the value, with `transform` applied to the one at position 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub signed: bool,
    pub shifts: Vec<u8>,
    pub transform: Transform,
}

/// What the wire does to the low byte of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    None = 0,
    /// the wire byte is the value plus 128
    Add = 1,
    /// the wire byte is the value negated
    Neg = 2,
    /// the wire byte is 128 minus the value
    Sub = 3,
}

/// A field in two bytes: width, sign and transform, then the position of each wire byte in the value, two bits each.
pub fn encode_field(field: &Field) -> Option<[u8; 2]> {
    let width = field.shifts.len();
    if !(1..=4).contains(&width) || field.shifts.iter().any(|&s| s > 3) {
        return None;
    }
    let order = field.shifts.iter().enumerate().fold(0u8, |acc, (i, &s)| acc | s << (2 * i));
    Some([width as u8 | (field.signed as u8) << 3 | (field.transform as u8) << 4, order])
}

pub fn decode_field(code: [u8; 2]) -> Option<Field> {
    let width = (code[0] & 7) as usize;
    if !(1..=4).contains(&width) {
        return None;
    }
    let transform = match code[0] >> 4 & 3 {
        0 => Transform::None,
        1 => Transform::Add,
        2 => Transform::Neg,
        _ => Transform::Sub,
    };
    Some(Field { signed: code[0] >> 3 & 1 == 1, shifts: (0..width).map(|i| code[1] >> (2 * i) & 3).collect(), transform })
}

impl Field {
    /// The loader's packet table spelling: `u8add`, `i16be`, `u24:2.0.1`.
    pub fn spelling(&self) -> String {
        let width = self.shifts.len();
        let order = if width == 1 {
            String::new()
        } else if self.shifts.windows(2).all(|w| w[0] > w[1]) {
            "be".into()
        } else if self.shifts.windows(2).all(|w| w[0] < w[1]) {
            "le".into()
        } else {
            format!(":{}", self.shifts.iter().map(|s| format!("{s}")).collect::<Vec<_>>().join("."))
        };
        let transform = match self.transform {
            Transform::None => "",
            Transform::Add => "add",
            Transform::Neg => "neg",
            Transform::Sub => "sub",
        };
        // a four byte value is always an int: there is nothing wider to extend it into
        format!("{}{}{order}{transform}", if self.signed || width == 4 { "i" } else { "u" }, width * 8)
    }
}

/// One packet's size and layout as published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub opcode: u16,
    /// -1: a length byte follows the id; -2: a length short
    pub size: i32,
    /// `None` when the layout isn't known
    pub fields: Option<Vec<Field>>,
}

/// Publishes what the agent found in the client's code, once; `layouts[id]` is for server packet `id`. `log` must point to an initialised `Log`.
pub unsafe fn publish_layouts(log: *const Log, layouts: &[(i32, Option<Vec<Field>>)]) {
    for (id, (size, fields)) in layouts.iter().enumerate().take(LAYOUT_IDS) {
        let slot = addr_of!((*log).layouts[id]) as *mut LayoutSlot;
        write_volatile(addr_of_mut!((*slot).size), *size);
        let codes: Option<Vec<[u8; 2]>> = fields.as_ref().and_then(|fields| fields.iter().map(encode_field).collect()).filter(|codes: &Vec<[u8; 2]>| codes.len() < LAYOUT_FIELDS);
        match codes {
            Some(codes) => {
                for (i, code) in codes.iter().enumerate() {
                    write_volatile(addr_of_mut!((*slot).fields[i]), *code);
                }
                write_volatile(addr_of_mut!((*slot).fields_count), codes.len() as u8);
            }
            None => write_volatile(addr_of_mut!((*slot).fields_count), NO_LAYOUT),
        }
    }
    (*log).layouts_ready.store(layouts.len().min(LAYOUT_IDS) as u32 + 1, Ordering::Release);
}

/// What the agent published, `None` until it has. `log` must point to `LOG_SIZE` readable bytes.
pub unsafe fn read_layouts(log: *const Log) -> Option<Vec<Layout>> {
    let ready = (*log).layouts_ready.load(Ordering::Acquire) as usize;
    let count = ready.checked_sub(1)?;
    Some(
        (0..count)
            .map(|id| {
                let slot = addr_of!((*log).layouts[id]);
                let n = read_volatile(addr_of!((*slot).fields_count));
                let fields = (n != NO_LAYOUT && (n as usize) < LAYOUT_FIELDS)
                    .then(|| (0..n as usize).map(|i| decode_field(read_volatile(addr_of!((*slot).fields[i])))).collect::<Option<Vec<_>>>())
                    .flatten();
                Layout { opcode: id as u16, size: read_volatile(addr_of!((*slot).size)), fields }
            })
            .collect(),
    )
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

/// Publishes the player's movement; one writer only (the game thread); `log` must point to an initialised `Log`.
pub unsafe fn set_movement(log: *const Log, movement: &Movement) {
    let sequence = (*log).movement_sequence.load(Ordering::Relaxed);
    (*log).movement_sequence.store(sequence.wrapping_add(1) | 1, Ordering::Relaxed);
    fence(Ordering::Release);
    for (field, value) in (*log).movement.iter().zip(movement.fields()) {
        field.store(value, Ordering::Relaxed);
    }
    // even and never 0 again, so a reader can tell "written" from "never written"
    (*log).movement_sequence.store((sequence | 1).wrapping_add(1).max(2), Ordering::Release);
}

/// The player's movement as last published (None before the hook first saw the player, or when the writer kept tearing it); `log` must point to an initialised `Log`.
pub unsafe fn movement(log: *const Log) -> Option<Movement> {
    for _ in 0..100 {
        let before = (*log).movement_sequence.load(Ordering::Acquire);
        if before == 0 {
            return None;
        }
        if before & 1 == 1 {
            core::hint::spin_loop();
            continue;
        }
        let f: [i32; MOVEMENT_FIELDS] = core::array::from_fn(|i| (*log).movement[i].load(Ordering::Relaxed));
        fence(Ordering::Acquire);
        if (*log).movement_sequence.load(Ordering::Relaxed) == before {
            return Some(Movement { x: f[0], y: f[1], plane: f[2], moved_tick: f[3], from_x: f[4], from_y: f[5], from_plane: f[6] });
        }
    }
    None
}

/// The readers whose filter has this packet, as a bit mask of their slots; `log` must point to an initialised `Log`.
pub unsafe fn subscribers_for(log: *const Log, direction: u8, opcode: u16) -> u32 {
    let mask = (*log).subscribers.load(Ordering::Acquire);
    let Some(bit) = filter_bit(direction, opcode).filter(|_| mask != 0) else { return 0 };
    let mut wanted = 0;
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros();
        rest &= rest - 1;
        if (*log).filters[slot as usize][bit / 32].load(Ordering::Acquire) & (1 << (bit % 32)) != 0 {
            wanted |= 1 << slot;
        }
    }
    wanted
}

/// Claims a free subscriber slot with an empty filter (None when full), skipping slots whose event already exists; `log` must be writable.
pub unsafe fn subscribe(log: *const Log, mut create_event: impl FnMut(u32) -> bool) -> Option<u32> {
    for slot in 0..SUBSCRIBER_SLOTS {
        let bit = 1u32 << slot;
        if (*log).subscribers.load(Ordering::Acquire) & bit != 0 || !create_event(slot) {
            continue;
        }
        // the event is ours alone, so nobody else writes this filter
        set_filter(log, slot, &[0; FILTER_WORDS]);
        if (*log).subscribers.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            return Some(slot);
        }
    }
    None
}

/// Replaces the packets the reader holding `slot` is woken for; `log` must be writable.
pub unsafe fn set_filter(log: *const Log, slot: u32, filter: &Filter) {
    for (word, &bits) in (*log).filters[slot as usize].iter().zip(filter) {
        word.store(bits, Ordering::Release);
    }
}

/// Gives a slot back; `log` must be writable.
pub unsafe fn unsubscribe(log: *const Log, slot: u32) {
    (*log).subscribers.fetch_and(!(1u32 << slot), Ordering::AcqRel);
}

/// The sequence number the next packet will get; `log` must point to an initialised `Log`.
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
    use alloc::{alloc::{alloc_zeroed, dealloc, Layout as AllocLayout}, vec};

    struct Memory(*mut Log);

    impl Memory {
        fn new() -> Self {
            let layout = AllocLayout::new::<Log>();
            let log = unsafe { alloc_zeroed(layout) } as *mut Log;
            unsafe { init(log) };
            Self(log)
        }
    }

    impl Drop for Memory {
        fn drop(&mut self) {
            unsafe { dealloc(self.0.cast(), AllocLayout::new::<Log>()) };
        }
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 128);
        assert_eq!(core::mem::offset_of!(Log, movement), 24);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(core::mem::offset_of!(Log, filters), 64 + SLOT_COUNT * 128);
        assert_eq!(core::mem::size_of::<LayoutSlot>(), 40);
        assert_eq!(core::mem::offset_of!(Log, layouts), 64 + SLOT_COUNT * 128 + SUBSCRIBER_SLOTS as usize * FILTER_WORDS * 4);
        assert_eq!(LOG_SIZE, 64 + SLOT_COUNT * 128 + SUBSCRIBER_SLOTS as usize * FILTER_WORDS * 4 + LAYOUT_IDS * 40);
        assert_eq!(packets_memory_name(32), "AGENT_PACKETS_32");
        assert_eq!(packet_wake_event_name(32, 5), "AGENT_PACKETS_WAKE_32_5");
    }

    #[test]
    fn fields_survive_the_two_byte_form_and_spell_like_the_loader_table() {
        for (field, spelling) in [
            (Field { signed: false, shifts: vec![0], transform: Transform::Add }, "u8add"),
            (Field { signed: true, shifts: vec![1, 0], transform: Transform::None }, "i16be"),
            (Field { signed: false, shifts: vec![0, 1], transform: Transform::Add }, "u16leadd"),
            (Field { signed: true, shifts: vec![2, 3, 0, 1], transform: Transform::None }, "i32:2.3.0.1"),
        ] {
            assert_eq!(field.spelling(), spelling);
            assert_eq!(decode_field(encode_field(&field).unwrap()), Some(field));
        }
        assert_eq!(encode_field(&Field { signed: false, shifts: vec![], transform: Transform::None }), None);
    }

    #[test]
    fn layouts_are_unpublished_until_the_agent_publishes_them() {
        let memory = Memory::new();
        let u16le = Field { signed: false, shifts: vec![0, 1], transform: Transform::None };
        unsafe {
            assert_eq!(read_layouts(memory.0), None);
            publish_layouts(memory.0, &[(0, Some(vec![])), (-1, None), (2, Some(vec![u16le.clone()]))]);
            let layouts = read_layouts(memory.0).unwrap();
            assert_eq!(layouts.len(), 3);
            assert_eq!(layouts[0], Layout { opcode: 0, size: 0, fields: Some(vec![]) });
            assert_eq!(layouts[1], Layout { opcode: 1, size: -1, fields: None });
            assert_eq!(layouts[2], Layout { opcode: 2, size: 2, fields: Some(vec![u16le]) });
        }
    }

    #[test]
    fn movement_is_published_whole_and_says_when_it_changed() {
        let memory = Memory::new();
        unsafe {
            assert_eq!(movement(memory.0), None);
            let standing = Movement { x: 3200, y: 3200, plane: 0, moved_tick: NO_TICK, from_x: 3200, from_y: 3200, from_plane: 0 };
            set_movement(memory.0, &standing);
            assert_eq!(movement(memory.0), Some(standing));
            assert!(!standing.moving(500));

            // Player Info of the tick that ends at counter 101 moved the player: moving while it is received, and through the next tick
            let walked = Movement { x: 3201, moved_tick: 100, ..standing };
            set_movement(memory.0, &walked);
            assert_eq!(movement(memory.0), Some(walked));
            assert!(walked.moving(100) && walked.moving(101));
            assert!(!walked.moving(102) && !walked.moving(99) && !walked.moving(0));
            assert!(Movement { moved_tick: i32::MAX, ..walked }.moving(i32::MIN));
        }
    }

    #[test]
    fn filters_hold_both_directions_and_only_small_opcodes() {
        let mut filter = [0; FILTER_WORDS];
        assert!(filter_add(&mut filter, direction::RECEIVED, 2));
        assert!(filter_add(&mut filter, direction::SENT, 255));
        assert!(!filter_add(&mut filter, direction::RECEIVED, 256));
        assert!(filter_has(&filter, direction::RECEIVED, 2));
        assert!(!filter_has(&filter, direction::SENT, 2));
        assert!(filter_has(&filter, direction::SENT, 255));
        assert!(!filter_has(&filter, direction::RECEIVED, 255));
        assert!(!filter_has(&filter, 2, 2));
    }

    #[test]
    fn subscribers_are_woken_only_for_their_packets() {
        let memory = Memory::new();
        unsafe {
            assert_eq!(subscribe(memory.0, |_| true), Some(0));
            // slot 1 has a live event that isn't in the mask (another process's), so it is skipped
            assert_eq!(subscribe(memory.0, |slot| slot != 1), Some(2));
            assert_eq!(subscribers_for(memory.0, direction::RECEIVED, 2), 0);

            let mut ticks = [0; FILTER_WORDS];
            filter_add(&mut ticks, direction::RECEIVED, 2);
            set_filter(memory.0, 0, &ticks);
            filter_add(&mut ticks, direction::SENT, 75);
            set_filter(memory.0, 2, &ticks);
            assert_eq!(subscribers_for(memory.0, direction::RECEIVED, 2), 0b101);
            assert_eq!(subscribers_for(memory.0, direction::SENT, 75), 0b100);
            assert_eq!(subscribers_for(memory.0, direction::RECEIVED, 40), 0);

            // a slot given back and claimed again starts with an empty filter
            unsubscribe(memory.0, 0);
            assert_eq!(subscribers_for(memory.0, direction::RECEIVED, 2), 0b100);
            assert_eq!(subscribe(memory.0, |_| true), Some(0));
            assert_eq!(subscribers_for(memory.0, direction::RECEIVED, 2), 0b100);
            for _ in 0..SUBSCRIBER_SLOTS {
                subscribe(memory.0, |_| true);
            }
            assert_eq!(subscribe(memory.0, |_| true), None);
        }
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
