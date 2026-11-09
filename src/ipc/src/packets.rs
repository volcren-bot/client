use crate::ring;
use alloc::{format, string::String, vec::Vec};
use core::{
    ptr::{addr_of, addr_of_mut, read_volatile, write_volatile},
    sync::atomic::{fence, AtomicI32, AtomicU32, AtomicU64, Ordering},
};

pub fn packets_memory_name(pid: u32) -> String {
    format!("AGENT_PACKETS_{pid}")
}

pub fn packet_wake_event_name(pid: u32, slot: u32) -> String {
    format!("AGENT_PACKETS_WAKE_{pid}_{slot}")
}

pub const SUBSCRIBER_SLOTS: u32 = 32;

pub const FILTER_WORDS: usize = 16;

pub type Filter = [u32; FILTER_WORDS];

pub fn filter_bit(direction: u8, opcode: u16) -> Option<usize> {
    (direction <= direction::SENT && opcode < 256)
        .then(|| direction as usize * 256 + opcode as usize)
}

pub fn filter_has(filter: &Filter, direction: u8, opcode: u16) -> bool {
    filter_bit(direction, opcode).is_some_and(|bit| filter[bit / 32] & (1 << (bit % 32)) != 0)
}

pub fn filter_add(filter: &mut Filter, direction: u8, opcode: u16) -> bool {
    let Some(bit) = filter_bit(direction, opcode) else {
        return false;
    };
    filter[bit / 32] |= 1 << (bit % 32);
    true
}

pub const SLOT_COUNT: usize = 4096;

pub const PAYLOAD_SIZE: usize = 104;

pub mod direction {
    pub const RECEIVED: u8 = 0;
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

impl ring::Slot for Slot {
    fn commit(&self) -> &AtomicU64 {
        &self.commit
    }
}

#[repr(C)]
pub struct Log {
    header: ring::Header,
    subscribers: AtomicU32,
    movement_sequence: AtomicU32,
    movement: [AtomicI32; MOVEMENT_FIELDS],
    layouts_ready: AtomicU32,
    reserved: [u8; 8],
    entries: [Slot; SLOT_COUNT],
    filters: [[AtomicU32; FILTER_WORDS]; SUBSCRIBER_SLOTS as usize],
    layouts: [LayoutSlot; LAYOUT_IDS],
}

pub const LAYOUT_IDS: usize = 256;

pub const LAYOUT_FIELDS: usize = 16;

const NO_LAYOUT: u8 = 0xff;

#[repr(C)]
pub struct LayoutSlot {
    size: i32,
    fields_count: u8,
    fields: [[u8; 2]; LAYOUT_FIELDS],
    reserved: [u8; 3],
}

pub const LOG_SIZE: usize = core::mem::size_of::<Log>();

const MOVEMENT_FIELDS: usize = 7;

pub const NO_TICK: i32 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Movement {
    pub x: i32,
    pub y: i32,
    pub plane: i32,
    pub moved_tick: i32,
    pub from_x: i32,
    pub from_y: i32,
    pub from_plane: i32,
}

impl Movement {
    fn fields(&self) -> [i32; MOVEMENT_FIELDS] {
        [
            self.x,
            self.y,
            self.plane,
            self.moved_tick,
            self.from_x,
            self.from_y,
            self.from_plane,
        ]
    }

    pub fn moving(&self, tick: i32) -> bool {
        self.moved_tick != NO_TICK && (0..=1).contains(&tick.wrapping_sub(self.moved_tick))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u64,
    pub time_ms: u64,
    pub direction: u8,
    pub opcode: u16,
    pub length: u32,
    pub payload: Vec<u8>,
}

pub type Page = ring::Page<Entry>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub signed: bool,
    pub shifts: Vec<u8>,
    pub transform: Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    None = 0,
    Add = 1,
    Neg = 2,
    Sub = 3,
}

pub fn encode_field(field: &Field) -> Option<[u8; 2]> {
    let width = field.shifts.len();
    if !(1..=4).contains(&width) || field.shifts.iter().any(|&s| s > 3) {
        return None;
    }
    let order = field
        .shifts
        .iter()
        .enumerate()
        .fold(0u8, |acc, (i, &s)| acc | s << (2 * i));
    Some([
        width as u8 | (field.signed as u8) << 3 | (field.transform as u8) << 4,
        order,
    ])
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
    Some(Field {
        signed: code[0] >> 3 & 1 == 1,
        shifts: (0..width).map(|i| code[1] >> (2 * i) & 3).collect(),
        transform,
    })
}

impl Field {
    pub fn spelling(&self) -> String {
        let width = self.shifts.len();
        let order = if width == 1 {
            String::new()
        } else if self.shifts.windows(2).all(|w| w[0] > w[1]) {
            "be".into()
        } else if self.shifts.windows(2).all(|w| w[0] < w[1]) {
            "le".into()
        } else {
            format!(
                ":{}",
                self.shifts
                    .iter()
                    .map(|s| format!("{s}"))
                    .collect::<Vec<_>>()
                    .join(".")
            )
        };
        let transform = match self.transform {
            Transform::None => "",
            Transform::Add => "add",
            Transform::Neg => "neg",
            Transform::Sub => "sub",
        };
        format!(
            "{}{}{order}{transform}",
            if self.signed || width == 4 { "i" } else { "u" },
            width * 8
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub opcode: u16,
    pub size: i32,
    pub fields: Option<Vec<Field>>,
}

pub unsafe fn publish_layouts(log: *const Log, layouts: &[(i32, Option<Vec<Field>>)]) {
    for (id, (size, fields)) in layouts.iter().enumerate().take(LAYOUT_IDS) {
        let slot = addr_of!((*log).layouts[id]) as *mut LayoutSlot;
        write_volatile(addr_of_mut!((*slot).size), *size);
        let codes = fields
            .as_ref()
            .and_then(|fields| fields.iter().map(encode_field).collect::<Option<Vec<_>>>())
            .filter(|codes| codes.len() <= LAYOUT_FIELDS);
        let count = match codes {
            Some(codes) => {
                for (i, &code) in codes.iter().enumerate() {
                    write_volatile(addr_of_mut!((*slot).fields[i]), code);
                }
                codes.len() as u8
            }
            None => NO_LAYOUT,
        };
        write_volatile(addr_of_mut!((*slot).fields_count), count);
    }
    (*log)
        .layouts_ready
        .store(layouts.len().min(LAYOUT_IDS) as u32 + 1, Ordering::Release);
}

pub unsafe fn read_layouts(log: *const Log) -> Option<Vec<Layout>> {
    let ready = (*log).layouts_ready.load(Ordering::Acquire) as usize;
    let count = ready.checked_sub(1)?.min(LAYOUT_IDS);
    Some(
        (0..count)
            .map(|id| {
                let slot = addr_of!((*log).layouts[id]);
                let n = read_volatile(addr_of!((*slot).fields_count));
                let fields = (n != NO_LAYOUT && (n as usize) <= LAYOUT_FIELDS)
                    .then(|| {
                        (0..n as usize)
                            .map(|i| decode_field(read_volatile(addr_of!((*slot).fields[i]))))
                            .collect::<Option<Vec<_>>>()
                    })
                    .flatten();
                Layout {
                    opcode: id as u16,
                    size: read_volatile(addr_of!((*slot).size)),
                    fields,
                }
            })
            .collect(),
    )
}

pub unsafe fn init(log: *mut Log) {
    ring::Header::init::<Slot, SLOT_COUNT>(addr_of_mut!((*log).header));
}

pub unsafe fn is_compatible(log: *const Log) -> bool {
    ring::Header::is_compatible::<Slot, SLOT_COUNT>(addr_of!((*log).header))
}

pub unsafe fn push(
    log: *const Log,
    time_ms: u64,
    direction: u8,
    opcode: u16,
    length: u32,
    data: &[u8],
) {
    let (payload, stored) = ring::prefix::<PAYLOAD_SIZE>(data);
    ring::push(&(*log).header, addr_of!((*log).entries), |slot| {
        write_volatile(addr_of_mut!((*slot).time_ms), time_ms);
        write_volatile(addr_of_mut!((*slot).length), length);
        write_volatile(addr_of_mut!((*slot).opcode), opcode);
        write_volatile(addr_of_mut!((*slot).direction), direction);
        write_volatile(addr_of_mut!((*slot).stored), stored);
        write_volatile(addr_of_mut!((*slot).payload), payload);
    });
}

pub unsafe fn set_movement(log: *const Log, movement: &Movement) {
    let sequence = (*log).movement_sequence.load(Ordering::Relaxed);
    (*log)
        .movement_sequence
        .store(sequence.wrapping_add(1) | 1, Ordering::Relaxed);
    fence(Ordering::Release);
    for (field, value) in (*log).movement.iter().zip(movement.fields()) {
        field.store(value, Ordering::Relaxed);
    }
    (*log)
        .movement_sequence
        .store((sequence | 1).wrapping_add(1).max(2), Ordering::Release);
}

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
        let [x, y, plane, moved_tick, from_x, from_y, from_plane] =
            core::array::from_fn(|i| (*log).movement[i].load(Ordering::Relaxed));
        fence(Ordering::Acquire);
        if (*log).movement_sequence.load(Ordering::Relaxed) == before {
            return Some(Movement {
                x,
                y,
                plane,
                moved_tick,
                from_x,
                from_y,
                from_plane,
            });
        }
    }
    None
}

pub unsafe fn subscribers_for(log: *const Log, direction: u8, opcode: u16) -> u32 {
    let mask = (*log).subscribers.load(Ordering::Acquire);
    let Some(bit) = filter_bit(direction, opcode).filter(|_| mask != 0) else {
        return 0;
    };
    let mut wanted = 0;
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros();
        rest &= rest - 1;
        if (*log).filters[slot as usize][bit / 32].load(Ordering::Acquire) & (1 << (bit % 32)) != 0
        {
            wanted |= 1 << slot;
        }
    }
    wanted
}

pub unsafe fn subscribe(log: *const Log, mut create_event: impl FnMut(u32) -> bool) -> Option<u32> {
    ring::subscribe(&(*log).subscribers, SUBSCRIBER_SLOTS, |slot| {
        let created = create_event(slot);
        if created {
            set_filter(log, slot, &[0; FILTER_WORDS]);
        }
        created
    })
}

pub unsafe fn set_filter(log: *const Log, slot: u32, filter: &Filter) {
    for (word, &bits) in (*log).filters[slot as usize].iter().zip(filter) {
        word.store(bits, Ordering::Release);
    }
}

pub unsafe fn unsubscribe(log: *const Log, slot: u32) {
    ring::unsubscribe(&(*log).subscribers, slot);
}

pub unsafe fn written(log: *const Log) -> u64 {
    (*log).header.written()
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
            direction: read_volatile(addr_of!((*slot).direction)),
            opcode: read_volatile(addr_of!((*slot).opcode)),
            length: read_volatile(addr_of!((*slot).length)),
            payload: ring::read_prefix(
                addr_of!((*slot).payload),
                read_volatile(addr_of!((*slot).stored)),
            ),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring::Zeroed;
    use alloc::vec;

    fn memory() -> Zeroed<Log> {
        let memory = Zeroed::new();
        unsafe { init(memory.0) };
        memory
    }

    #[test]
    fn layout() {
        assert_eq!(core::mem::size_of::<Slot>(), 128);
        assert_eq!(core::mem::offset_of!(Log, movement), 24);
        assert_eq!(core::mem::offset_of!(Log, entries), 64);
        assert_eq!(core::mem::offset_of!(Log, filters), 64 + SLOT_COUNT * 128);
        assert_eq!(core::mem::size_of::<LayoutSlot>(), 40);
        assert_eq!(
            core::mem::offset_of!(Log, layouts),
            64 + SLOT_COUNT * 128 + SUBSCRIBER_SLOTS as usize * FILTER_WORDS * 4
        );
        assert_eq!(
            LOG_SIZE,
            64 + SLOT_COUNT * 128 + SUBSCRIBER_SLOTS as usize * FILTER_WORDS * 4 + LAYOUT_IDS * 40
        );
        assert_eq!(packet_wake_event_name(32, 5), "AGENT_PACKETS_WAKE_32_5");
    }

    #[test]
    fn fields_survive_the_two_byte_form_and_spell_like_the_loader_table() {
        for (field, spelling) in [
            (
                Field {
                    signed: false,
                    shifts: vec![0],
                    transform: Transform::Add,
                },
                "u8add",
            ),
            (
                Field {
                    signed: true,
                    shifts: vec![1, 0],
                    transform: Transform::None,
                },
                "i16be",
            ),
            (
                Field {
                    signed: false,
                    shifts: vec![0, 1],
                    transform: Transform::Add,
                },
                "u16leadd",
            ),
            (
                Field {
                    signed: true,
                    shifts: vec![2, 3, 0, 1],
                    transform: Transform::None,
                },
                "i32:2.3.0.1",
            ),
        ] {
            assert_eq!(field.spelling(), spelling);
            assert_eq!(decode_field(encode_field(&field).unwrap()), Some(field));
        }
        assert_eq!(
            encode_field(&Field {
                signed: false,
                shifts: vec![],
                transform: Transform::None
            }),
            None
        );
    }

    #[test]
    fn layouts_are_unpublished_until_the_agent_publishes_them() {
        let memory = memory();
        let u16le = Field {
            signed: false,
            shifts: vec![0, 1],
            transform: Transform::None,
        };
        unsafe {
            assert_eq!(read_layouts(memory.0), None);
            publish_layouts(
                memory.0,
                &[
                    (0, Some(vec![])),
                    (-1, None),
                    (2, Some(vec![u16le.clone()])),
                    (32, Some(vec![u16le.clone(); LAYOUT_FIELDS])),
                    (34, Some(vec![u16le.clone(); LAYOUT_FIELDS + 1])),
                ],
            );
            let layouts = read_layouts(memory.0).unwrap();
            assert_eq!(layouts.len(), 5);
            assert_eq!(
                layouts[3].fields.as_ref().map(Vec::len),
                Some(LAYOUT_FIELDS)
            );
            assert_eq!(layouts[4].fields, None);
            assert_eq!(
                layouts[0],
                Layout {
                    opcode: 0,
                    size: 0,
                    fields: Some(vec![])
                }
            );
            assert_eq!(
                layouts[1],
                Layout {
                    opcode: 1,
                    size: -1,
                    fields: None
                }
            );
            assert_eq!(
                layouts[2],
                Layout {
                    opcode: 2,
                    size: 2,
                    fields: Some(vec![u16le])
                }
            );
        }
    }

    #[test]
    fn movement_is_published_whole_and_says_when_it_changed() {
        let memory = memory();
        unsafe {
            assert_eq!(movement(memory.0), None);
            let standing = Movement {
                x: 3200,
                y: 3200,
                plane: 0,
                moved_tick: NO_TICK,
                from_x: 3200,
                from_y: 3200,
                from_plane: 0,
            };
            set_movement(memory.0, &standing);
            assert_eq!(movement(memory.0), Some(standing));
            assert!(!standing.moving(500));

            let walked = Movement {
                x: 3201,
                moved_tick: 100,
                ..standing
            };
            set_movement(memory.0, &walked);
            assert_eq!(movement(memory.0), Some(walked));
            assert!(walked.moving(100) && walked.moving(101));
            assert!(!walked.moving(102) && !walked.moving(99) && !walked.moving(0));
            assert!(Movement {
                moved_tick: i32::MAX,
                ..walked
            }
            .moving(i32::MIN));
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
        let memory = memory();
        unsafe {
            assert_eq!(subscribe(memory.0, |_| true), Some(0));
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
        let memory = memory();
        unsafe {
            assert!(is_compatible(memory.0));
            push(
                memory.0,
                10,
                direction::RECEIVED,
                40,
                6,
                &[1, 2, 3, 4, 5, 6],
            );
            push(memory.0, 11, direction::SENT, 75, 0, &[]);
            let page = read_since(memory.0, 0, 10);
            assert_eq!(page.next, 2);
            assert_eq!(page.dropped, 0);
            assert_eq!(
                page.entries[0],
                Entry {
                    sequence: 0,
                    time_ms: 10,
                    direction: direction::RECEIVED,
                    opcode: 40,
                    length: 6,
                    payload: alloc::vec![1, 2, 3, 4, 5, 6]
                }
            );
            assert_eq!(page.entries[1].opcode, 75);
            assert!(page.entries[1].payload.is_empty());

            push(memory.0, 12, direction::RECEIVED, 2, 0, &[]);
            let page = read_since(memory.0, page.next, 10);
            assert_eq!(
                (page.entries.len(), page.entries[0].sequence, page.next),
                (1, 2, 3)
            );
        }
    }

    #[test]
    fn keeps_only_the_start_of_a_long_payload_but_the_whole_length() {
        let memory = memory();
        let data: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
        unsafe {
            push(memory.0, 0, direction::RECEIVED, 7, 200, &data);
            let entry = &read_since(memory.0, 0, 1).entries[0];
            assert_eq!(entry.length, 200);
            assert_eq!(entry.payload, data[..PAYLOAD_SIZE]);
        }
    }
}
