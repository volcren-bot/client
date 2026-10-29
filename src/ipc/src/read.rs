use alloc::{string::String, vec, vec::Vec};

use crate::state::{game_state, interfaces, part, InterfaceLayout, Layout};

const MAX_CHAIN: usize = 64;
const TABLE_BUCKETS: u64 = 0x8;
const TABLE_COUNT: u64 = 0x10;
const NODE_VALUE: u64 = 0x10;
const NODE_NEXT: u64 = 0x18;
const NODE_SIZE: usize = NODE_NEXT as usize + 8;

pub trait Memory {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool;

    fn write(&self, _address: u64, _bytes: &[u8]) -> bool {
        false
    }

    fn u64(&self, address: u64) -> Option<u64> {
        let mut b = [0; 8];
        self.read(address, &mut b).then(|| u64::from_le_bytes(b))
    }

    fn i32(&self, address: u64) -> Option<i32> {
        let mut b = [0; 4];
        self.read(address, &mut b).then(|| i32::from_le_bytes(b))
    }

    fn f32(&self, address: u64) -> Option<f32> {
        let mut b = [0; 4];
        self.read(address, &mut b).then(|| f32::from_le_bytes(b))
    }

    fn ptr(&self, address: u64) -> Option<u64> {
        self.u64(address).filter(|&p| p != 0)
    }

    fn i32s(&self, address: u64, count: usize) -> Option<Vec<i32>> {
        let mut bytes = vec![0u8; count * 4];
        if count > 0 && !self.read(address, &mut bytes) {
            return None;
        }
        Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        )
    }
}

pub fn read_string(memory: &impl Memory, address: u64) -> Option<String> {
    let mut b = [0u8; 24];
    if !memory.read(address, &mut b) {
        return None;
    }
    if b[23] & 0x80 != 0 {
        let ptr = u64::from_le_bytes(b[0..8].try_into().ok()?);
        let len = u64::from_le_bytes(b[8..16].try_into().ok()?);
        if len > 4096 {
            return None;
        }
        let mut text = vec![0u8; len as usize];
        if len > 0 && !memory.read(ptr, &mut text) {
            return None;
        }
        // valid UTF-8 is taken as is; only invalid input pays for the lossy copy
        Some(
            String::from_utf8(text)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
        )
    } else {
        let len = 23usize.checked_sub(b[23] as usize)?;
        Some(String::from_utf8_lossy(&b[..len]).into_owned())
    }
}

pub const SHORT_STRING_MAX: usize = 22;

pub fn short_string(text: &[u8]) -> Option<[u8; 24]> {
    if text.len() > SHORT_STRING_MAX {
        return None;
    }
    let mut string = [0u8; 24];
    string[..text.len()].copy_from_slice(text);
    string[23] = (23 - text.len()) as u8;
    Some(string)
}

pub fn short_string_text(string: &[u8; 24]) -> Option<&[u8]> {
    (string[23] & 0x80 == 0 && string[23] <= 23).then(|| &string[..23 - string[23] as usize])
}

pub fn client(memory: &impl Memory, layout: &Layout) -> Option<u64> {
    memory.ptr(layout.module_base + layout.client as u64)
}

pub fn current_game_state(memory: &impl Memory, layout: &Layout) -> Option<i32> {
    if !layout.has(part::GAME_STATE) {
        return None;
    }
    memory.i32(client(memory, layout)? + layout.game_state as u64)
}

pub fn table_get(memory: &impl Memory, table: u64, key: i32) -> Option<u64> {
    let buckets = memory.ptr(table + TABLE_BUCKETS)?;
    let count = memory.i32(table + TABLE_COUNT)? as u32 as u64;
    if count == 0 || key < 0 {
        return None;
    }
    let end = memory.u64(buckets + count * 8)?;
    let mut node = memory.u64(buckets + (key as u64 % count) * 8)?;
    for _ in 0..MAX_CHAIN {
        if node == 0 || node == end {
            return None;
        }
        // one read covers the node's key, value and next pointer
        let mut fields = [0u8; NODE_SIZE];
        if !memory.read(node, &mut fields) {
            return None;
        }
        if i32::from_le_bytes(fields[..4].try_into().ok()?) == key {
            let value = u64::from_le_bytes(fields[NODE_VALUE as usize..][..8].try_into().ok()?);
            return (value != 0).then_some(value);
        }
        node = u64::from_le_bytes(fields[NODE_NEXT as usize..].try_into().ok()?);
    }
    None
}

/// Every (key, value) in a table `table_get` reads, in no order.
pub fn table_entries(memory: &impl Memory, table: u64) -> Vec<(i32, u64)> {
    let (Some(buckets), Some(count)) = (memory.ptr(table + TABLE_BUCKETS), memory.i32(table + TABLE_COUNT)) else { return Vec::new() };
    let count = count as u32 as u64;
    if count == 0 || count > 1 << 16 {
        return Vec::new();
    }
    let Some(end) = memory.u64(buckets + count * 8) else { return Vec::new() };
    let mut seen = alloc::collections::BTreeSet::new();
    let mut entries = Vec::new();
    for bucket in 0..count {
        let mut node = memory.u64(buckets + bucket * 8).unwrap_or(0);
        for _ in 0..MAX_CHAIN {
            if node == 0 || node == end || !seen.insert(node) {
                break;
            }
            let mut fields = [0u8; NODE_SIZE];
            if !memory.read(node, &mut fields) {
                break;
            }
            let key = i32::from_le_bytes(fields[..4].try_into().unwrap());
            let value = u64::from_le_bytes(fields[NODE_VALUE as usize..][..8].try_into().unwrap());
            entries.push((key, value));
            node = u64::from_le_bytes(fields[NODE_NEXT as usize..].try_into().unwrap());
        }
    }
    entries
}

pub fn local_player(memory: &impl Memory, layout: &Layout) -> Option<(u64, u64)> {
    if !layout.has(part::PLAYER) {
        return None;
    }
    let client = client(memory, layout)?;
    let p = &layout.player;
    let view_id = memory.i32(client + p.world_view_id as u64)?;
    let index = memory.i32(client + p.player_index as u64)?;
    let view = table_get(memory, client + p.world_views as u64, view_id)?;
    let player = table_get(memory, view + p.players as u64, index)?;
    Some((view, player))
}

// The map squares the scene was built from (the source squares in an instance), found through the scene loader's builder vector without offsets.
const LOADER_SCAN: [usize; 3] = [0x8000, 0x4000, 0x2000];
const LOADER_VIEW: u64 = 0x10;
const LOADER_FIELDS: usize = 0x50;
const BUILDER_WINDOW: usize = 0x100;
const REGION_MIN_STRIDE: u64 = 0x20;
const REGION_MAX_STRIDE: u64 = 0x200;
const REGION_MAX_BYTES: u64 = 64 * REGION_MAX_STRIDE;
const REGION_LIMIT: i32 = 256;

pub fn map_regions(memory: &impl Memory, layout: &Layout) -> Option<Vec<[i32; 2]>> {
    let (view, _) = local_player(memory, layout)?;
    scene_loader(memory, client(memory, layout)?, view).map(|(_, regions)| regions)
}

// The client's own flag for "this scene is an instance", read from the scene loader.
pub fn in_instance(memory: &impl Memory, layout: &Layout) -> Option<bool> {
    if !layout.has(part::SCENE_LOADER) {
        return None;
    }
    let (view, _) = local_player(memory, layout)?;
    let (loader, _) = scene_loader(memory, client(memory, layout)?, view)?;
    let mut flag = [0u8; 1];
    memory
        .read(loader + layout.scene_loader.instance_flag as u64, &mut flag)
        .then(|| flag[0] != 0)
}

fn plausible(pointer: u64) -> bool {
    pointer.is_multiple_of(8) && (0x1000..0x7fff_ffff_0000).contains(&pointer)
}

fn scene_loader(memory: &impl Memory, client: u64, view: u64) -> Option<(u64, Vec<[i32; 2]>)> {
    let (scan, window) = LOADER_SCAN.iter().find_map(|&n| {
        let mut window = vec![0u8; n];
        memory.read(client, &mut window).then_some((n, window))
    })?;
    // (loader, builder, regions)
    let mut found: Vec<(u64, u64, Vec<[i32; 2]>)> = Vec::new();
    for at in (0..scan).step_by(8) {
        let loader = u64::from_le_bytes(window[at..at + 8].try_into().unwrap());
        if !plausible(loader) || memory.u64(loader + LOADER_VIEW) != Some(view) {
            continue;
        }
        let mut fields = [0u8; LOADER_FIELDS];
        if !memory.read(loader, &mut fields) {
            continue;
        }
        for field in (8..LOADER_FIELDS).step_by(8) {
            let builder = u64::from_le_bytes(fields[field..field + 8].try_into().unwrap());
            if plausible(builder) && !found.iter().any(|(_, b, _)| *b == builder) {
                if let Some(regions) = find_map_regions(memory, builder, BUILDER_WINDOW) {
                    found.push((loader, builder, regions));
                }
            }
        }
    }
    (found.len() == 1).then(|| {
        let (loader, _, regions) = found.remove(0);
        (loader, regions)
    })
}

fn find_map_regions(memory: &impl Memory, start: u64, length: usize) -> Option<Vec<[i32; 2]>> {
    let first = start.next_multiple_of(8);
    let skipped = (first - start) as usize;
    let mut window = vec![0u8; length];
    if !memory.read(first, &mut window[..length - skipped]) {
        return None;
    }
    let word = |at: usize| u64::from_le_bytes(window[at..at + 8].try_into().unwrap());
    let mut found = Vec::new();
    for at in (0..=(length - skipped).saturating_sub(24)).step_by(8) {
        if let Some(regions) = region_vector(memory, word(at), word(at + 8), word(at + 16)) {
            found.push(regions);
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

fn region_vector(
    memory: &impl Memory,
    begin: u64,
    end: u64,
    capacity: u64,
) -> Option<Vec<[i32; 2]>> {
    let bytes = end.checked_sub(begin)?;
    if !begin.is_multiple_of(8)
        || !(REGION_MIN_STRIDE..=REGION_MAX_BYTES).contains(&bytes)
        || capacity < end
        || capacity - begin > 16 * REGION_MAX_BYTES
    {
        return None;
    }
    let mut data = vec![0u8; bytes as usize];
    if !memory.read(begin, &mut data) {
        return None;
    }
    let int = |at: usize| i32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    (REGION_MIN_STRIDE..=REGION_MAX_STRIDE.min(bytes))
        .step_by(8)
        .filter(|stride| bytes % stride == 0 && (capacity - begin).is_multiple_of(*stride))
        .find_map(|stride| {
            let stride = stride as usize;
            let count = data.len() / stride;
            let square = |i: usize| (int(i * stride), int(i * stride + 4));
            // zeroed memory would pass as square (0, 0), which is never loaded
            if (0..count).any(|i| {
                !(0..REGION_LIMIT).contains(&square(i).0)
                    || !(0..REGION_LIMIT).contains(&square(i).1)
                    || square(i) == (0, 0)
            }) {
                return None;
            }
            // the id field is wherever all of the records agree
            (8..stride - 3)
                .step_by(4)
                .any(|field| {
                    (0..count).all(|i| int(i * stride + field) == (square(i).0 << 8) + square(i).1)
                })
                .then(|| (0..count).map(|i| [square(i).0, square(i).1]).collect())
        })
}

const _: () = assert!(interfaces::ARRAY_DATA == interfaces::ARRAY_COUNT + 8);

pub fn component(
    memory: &impl Memory,
    layout: &InterfaceLayout,
    client: u64,
    id: i32,
) -> Option<u64> {
    let (group, child) = ((id as u32 >> 16) as u64, (id as u32 & 0xffff) as u64);
    let manager = memory.ptr(client + layout.manager as u64)?;
    let groups = memory.ptr(manager + layout.groups as u64)?;
    let array = groups + group * interfaces::GROUP_SIZE as u64;
    // the count and the data pointer are adjacent, so one read fetches both
    let mut header = [0u8; 16];
    if !memory.read(array + interfaces::ARRAY_COUNT as u64, &mut header) {
        return None;
    }
    let count = u64::from_le_bytes(header[..8].try_into().ok()?);
    if child >= count {
        return None;
    }
    let elements = u64::from_le_bytes(header[8..].try_into().ok()?);
    if elements == 0 {
        return None;
    }
    let component = memory.ptr(
        elements + child * interfaces::ENTRY_SIZE as u64 + interfaces::ENTRY_COMPONENT as u64,
    )?;
    (memory.i32(component + layout.id as u64)? == id).then_some(component)
}

pub const NAME_CLEAN: u64 = 0x18;

pub fn local_name_object(memory: &impl Memory, layout: &Layout) -> Option<u64> {
    if !layout.has(part::WORLD) || current_game_state(memory, layout) != Some(game_state::LOGGED_IN)
    {
        return None;
    }
    let (_, player) = local_player(memory, layout)?;
    memory.ptr(player + layout.world.player_name as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A sparse-ish fake process: one flat window starting at `base`.
    struct Fake {
        base: u64,
        bytes: Vec<u8>,
    }

    impl Fake {
        fn new(base: u64, size: usize) -> Self {
            Self {
                base,
                bytes: vec![0; size],
            }
        }

        fn put(&mut self, address: u64, data: &[u8]) {
            let start = (address - self.base) as usize;
            self.bytes[start..start + data.len()].copy_from_slice(data);
        }

        fn put_u64(&mut self, address: u64, value: u64) {
            self.put(address, &value.to_le_bytes());
        }

        fn put_i32(&mut self, address: u64, value: i32) {
            self.put(address, &value.to_le_bytes());
        }
    }

    impl Memory for Fake {
        fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
            let Some(start) = address.checked_sub(self.base).map(|s| s as usize) else {
                return false;
            };
            match self.bytes.get(start..start + buffer.len()) {
                Some(src) => {
                    buffer.copy_from_slice(src);
                    true
                }
                None => false,
            }
        }
    }

    fn short(text: &[u8]) -> [u8; 24] {
        short_string(text).unwrap()
    }

    #[test]
    fn read_string_short_and_long_forms() {
        let mut m = Fake::new(0x1000, 0x1000);
        m.put(0x1100, &short(b"Banker"));
        assert_eq!(read_string(&m, 0x1100).as_deref(), Some("Banker"));
        m.put(0x1140, &short(b""));
        assert_eq!(read_string(&m, 0x1140).as_deref(), Some(""));

        let text = b"Grand Exchange Clerk, a long one";
        m.put(0x1200, text);
        let mut header = [0u8; 24];
        header[0..8].copy_from_slice(&0x1200u64.to_le_bytes());
        header[8..16].copy_from_slice(&(text.len() as u64).to_le_bytes());
        header[23] = 0x80;
        m.put(0x1180, &header);
        assert_eq!(
            read_string(&m, 0x1180).as_deref(),
            Some("Grand Exchange Clerk, a long one")
        );

        // invalid UTF-8 is replaced, on both forms
        m.put(0x1300, &short(b"a\xffb"));
        assert_eq!(read_string(&m, 0x1300).as_deref(), Some("a\u{fffd}b"));
        m.put(0x1200, b"x\xff\xfey");
        header[8..16].copy_from_slice(&4u64.to_le_bytes());
        m.put(0x1180, &header);
        assert_eq!(
            read_string(&m, 0x1180).as_deref(),
            Some("x\u{fffd}\u{fffd}y")
        );

        // oversized long strings and bad short lengths are rejected
        header[8..16].copy_from_slice(&4097u64.to_le_bytes());
        m.put(0x1180, &header);
        assert_eq!(read_string(&m, 0x1180), None);
        let mut bad = [0u8; 24];
        bad[23] = 24;
        m.put(0x1400, &bad);
        assert_eq!(read_string(&m, 0x1400), None);
        assert_eq!(read_string(&m, 0x0), None);
    }

    // writes a vector of `stride`-byte square records and a (begin, end, capacity) header for it
    fn put_regions(
        m: &mut Fake,
        header: u64,
        records: u64,
        stride: u64,
        squares: &[(i32, i32)],
        capacity_records: u64,
    ) {
        for (i, (x, y)) in squares.iter().enumerate() {
            let at = records + i as u64 * stride;
            m.put_i32(at, *x);
            m.put_i32(at + 4, *y);
            m.put_i32(at + 16, (x << 8) + y);
        }
        m.put_u64(header, records);
        m.put_u64(header + 8, records + squares.len() as u64 * stride);
        m.put_u64(header + 16, records + capacity_records * stride);
    }

    #[test]
    fn map_regions_found_by_shape() {
        let port = [(48, 43), (48, 44), (49, 44), (50, 45)];
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x88, &port, 16);
        assert_eq!(
            find_map_regions(&m, 0x1000, 0x400),
            Some(port.iter().map(|&(x, y)| [x, y]).collect())
        );

        // a single record (an instance's source square) still has the shape
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x88, &[(47, 44)], 1);
        assert_eq!(find_map_regions(&m, 0x1000, 0x400), Some(vec![[47, 44]]));

        // another record size and id field
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x40, &port, 8);
        assert_eq!(
            find_map_regions(&m, 0x1000, 0x400).map(|r| r.len()),
            Some(4)
        );
    }

    #[test]
    fn map_regions_needs_exactly_one_vector_that_agrees() {
        let squares = [(48, 43), (49, 44)];
        // an id that doesn't match its square is not a region list
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x88, &squares, 4);
        m.put_i32(0x1800 + 16, 1);
        assert_eq!(find_map_regions(&m, 0x1000, 0x400), None);

        // squares outside the map are not either
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x88, &[(300, 44)], 1);
        assert_eq!(find_map_regions(&m, 0x1000, 0x400), None);

        // two vectors with the shape are ambiguous
        let mut m = Fake::new(0x1000, 0x2000);
        put_regions(&mut m, 0x1108, 0x1800, 0x88, &squares, 4);
        put_regions(&mut m, 0x1208, 0x1c00, 0x88, &squares, 4);
        assert_eq!(find_map_regions(&m, 0x1000, 0x400), None);

        // nothing readable, or nothing there
        assert_eq!(find_map_regions(&m, 0x9000, 0x400), None);
        assert_eq!(
            find_map_regions(&Fake::new(0x1000, 0x2000), 0x1000, 0x400),
            None
        );
    }

    #[test]
    fn map_regions_found_through_the_loader() {
        let view = 0x9000;
        let mut m = Fake::new(0x1000, 0x9000);
        // the client's slot points at the loader, which points back at the view and out to a builder
        m.put_u64(0x1100, 0x4000);
        m.put_u64(0x4000 + 0x10, view);
        m.put_u64(0x4000 + 0x28, 0x6000);
        m.put_u64(0x6000 + 0x28, view);
        put_regions(&mut m, 0x6068, 0x7000, 0x88, &[(47, 44)], 1);
        assert_eq!(
            scene_loader(&m, 0x1000, view),
            Some((0x4000, vec![[47, 44]]))
        );

        // a pointer to something that doesn't point back at the view is not the loader
        let mut other = Fake::new(0x1000, 0x9000);
        other.put_u64(0x1100, 0x4000);
        other.put_u64(0x4000 + 0x28, 0x6000);
        put_regions(&mut other, 0x6068, 0x7000, 0x88, &[(47, 44)], 1);
        assert_eq!(scene_loader(&other, 0x1000, view), None);

        // two builders with region lists are ambiguous
        m.put_u64(0x1108, 0x5000);
        m.put_u64(0x5000 + 0x10, view);
        m.put_u64(0x5000 + 0x28, 0x8000);
        put_regions(&mut m, 0x8068, 0x8400, 0x88, &[(48, 44)], 1);
        assert_eq!(scene_loader(&m, 0x1000, view), None);
    }

    #[test]
    fn table_get_walks_buckets_and_chains() {
        let mut m = Fake::new(0x1000, 0x2000);
        let table = 0x1000;
        let buckets = 0x1100;
        let count = 4u64;
        m.put_u64(table + TABLE_BUCKETS, buckets);
        m.put_i32(table + TABLE_COUNT, count as i32);
        let end = 0x1f00;
        m.put_u64(buckets + count * 8, end);

        // keys 1 and 5 share bucket 1; node 0x1200 is first in that chain
        let (first, second) = (0x1200u64, 0x1300u64);
        m.put_u64(buckets + 8, first);
        m.put_i32(first, 1);
        m.put_u64(first + NODE_VALUE, 0x1500);
        m.put_u64(first + NODE_NEXT, second);
        m.put_i32(second, 5);
        m.put_u64(second + NODE_VALUE, 0x1600);
        m.put_u64(second + NODE_NEXT, end);

        assert_eq!(table_get(&m, table, 1), Some(0x1500));
        assert_eq!(table_get(&m, table, 5), Some(0x1600));
        assert_eq!(table_get(&m, table, 9), None);
        assert_eq!(table_get(&m, table, -1), None);
        assert_eq!(table_get(&m, table, 2), None);

        // a null value pointer reads as missing
        m.put_u64(second + NODE_VALUE, 0);
        assert_eq!(table_get(&m, table, 5), None);

        // a chain that runs into the sentinel stops there
        m.put_u64(first + NODE_NEXT, end);
        assert_eq!(table_get(&m, table, 5), None);
    }

    #[test]
    fn component_finds_child_by_group_and_index() {
        let mut m = Fake::new(0x1000, 0x2000);
        let layout = InterfaceLayout {
            client: 0,
            manager: 0x10,
            groups: 0x18,
            id: 0x20,
            ..Default::default()
        };
        let client = 0x1000;
        let manager = 0x1100;
        let groups = 0x1200;
        m.put_u64(client + layout.manager as u64, manager);
        m.put_u64(manager + layout.groups as u64, groups);

        // group 2 holds two children
        let group = groups + 2 * interfaces::GROUP_SIZE as u64;
        let elements = 0x1400;
        m.put_u64(group + interfaces::ARRAY_COUNT as u64, 2);
        m.put_u64(group + interfaces::ARRAY_DATA as u64, elements);
        let node_at = |index: u64| 0x1500 + index * 0x40;
        for index in 0..2u64 {
            let entry = elements + index * interfaces::ENTRY_SIZE as u64;
            m.put_u64(entry + interfaces::ENTRY_COMPONENT as u64, node_at(index));
            m.put_i32(node_at(index) + layout.id as u64, (2 << 16) | index as i32);
        }

        assert_eq!(
            component(&m, &layout, client, 2 << 16 | 1),
            Some(node_at(1))
        );
        assert_eq!(component(&m, &layout, client, 2 << 16), Some(node_at(0)));
        // child past the count, an unset group, and a mismatched id are all missing
        assert_eq!(component(&m, &layout, client, 2 << 16 | 2), None);
        assert_eq!(component(&m, &layout, client, 3 << 16), None);
        assert_eq!(component(&m, &layout, client, 2 << 16 | 3), None);
    }
}
