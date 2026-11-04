use ipc::{
    read::{self, Memory},
    state::{
        cache_kind, cache_status, CacheReply, CacheRequest, CACHE_BATCH, CACHE_DATA_SIZE,
        CACHE_FILE_MISSING, CACHE_FILE_NO_ROOM, ITEM_ICON_HEIGHT, ITEM_ICON_ID_MASK,
        ITEM_ICON_SHOW_COUNT, ITEM_ICON_WIDTH,
    },
};
use std::sync::{Arc, OnceLock};

use super::{
    items::{game_vals, release, Reader, BUFFER_DATA, BUFFER_LENGTH},
    offsets::{CacheTable, GameVals},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
};

const INDEX_SEARCH: i64 = 0x200;
const INDEX_ID: u64 = 0x10;
const ENTRY_HASHED: usize = 0x18;
const MAX_ENTRIES: u64 = 1 << 20;

/// The client's item icon builder: `(out, id, quantity, border, shadow, text mode, noted, width, height, text)`, see `offsets::find_item_icon`.
type IconBuilder = unsafe extern "system" fn(
    *mut [u64; 2],
    i32,
    i32,
    i32,
    i32,
    i32,
    u8,
    i32,
    i32,
    *const u8,
) -> *mut [u64; 2];

static ICON_BUILDER: OnceLock<IconBuilder> = OnceLock::new();

/// What the inventory draws its items with: a one pixel border, the shadow, the number drawn or not.
const ICON_BORDER: i32 = 1;
const ICON_SHADOW: i32 = 0x30_2020;
const ICON_TEXT_OFF: i32 = 0;
const ICON_TEXT_COUNT: i32 = 1;
/// The client's string: 0x17 inline bytes and a last byte holding the room left, so this is the empty string.
const EMPTY_TEXT: [u8; 0x18] = {
    let mut text = [0u8; 0x18];
    text[0x17] = 0x17;
    text
};
/// The sprite the builder returns: its pixel count and the pixels, `0xAARRGGBB` words.
const SPRITE_COUNT: u64 = 0x0;
const SPRITE_PIXELS: u64 = 0x10;
const ICON_BYTES: usize = (ITEM_ICON_WIDTH * ITEM_ICON_HEIGHT * 4) as usize;
const MAX_ITEM_ID: i32 = 1 << 16;

pub unsafe fn set_item_icon(module_base: u64, rva: u32) {
    let builder: IconBuilder = std::mem::transmute((module_base + rva as u64) as *const ());
    let _ = ICON_BUILDER.set(builder);
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

pub fn on_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_cache_request() else {
        return;
    };
    shared.write_cache_reply_with(|reply| {
        reply.sequence = sequence;
        reply.count = 0;
        reply.used = 0;
        reply.fingerprint = 0;
        reply.status = fill(reply, &request);
    });
}

fn fill(reply: &mut CacheReply, request: &CacheRequest) -> i32 {
    if matches!(request.kind, cache_kind::FILES | cache_kind::ITEM_ICONS)
        && request.count as usize > CACHE_BATCH
    {
        return cache_status::TOO_MANY;
    }
    if on_other_thread() {
        return cache_status::OTHER_THREAD;
    }
    let memory = OwnMemory;
    if request.kind == cache_kind::ITEM_ICONS {
        return match ICON_BUILDER.get() {
            Some(&builder) => unsafe { draw_icons(&memory, builder, request, reply) },
            None => cache_status::NOT_FOUND,
        };
    }
    let Some(&(reader, rule)) = game_vals() else {
        return cache_status::NOT_FOUND;
    };
    let Some(client) = LAYOUT
        .get()
        .and_then(|layout| read::client(&memory, layout))
    else {
        return cache_status::NOT_FOUND;
    };
    let Some(index) = index_handle(&memory, client, &rule, request.index) else {
        return cache_status::NO_INDEX;
    };
    match request.kind {
        cache_kind::FILES => unsafe { read_files(&memory, reader, index, request, reply) },
        cache_kind::ARCHIVES => list_archives(&memory, &rule.table, index, reply),
        cache_kind::ARCHIVE_FILES => {
            list_files(&memory, &rule.table, index, request.archive, reply)
        }
        _ => cache_status::NOT_FOUND,
    }
}

fn index_handle(memory: &OwnMemory, client: u64, rule: &GameVals, id: u32) -> Option<u64> {
    let known = memory.ptr(client + rule.index as u64).filter(|&p| p != 0)?;
    let vtable = memory.ptr(known)?;
    (-INDEX_SEARCH..INDEX_SEARCH).step_by(8).find_map(|delta| {
        let field = (client + rule.index as u64).checked_add_signed(delta)?;
        let handle = memory.ptr(field).filter(|&p| p != 0)?;
        (memory.ptr(handle)? == vtable && memory.i32(handle + INDEX_ID)? as u32 == id)
            .then_some(handle)
    })
}

fn fnv(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

fn put_id(reply: &mut CacheReply, id: u32) -> bool {
    let at = reply.used as usize;
    if at + 4 > CACHE_DATA_SIZE {
        return false;
    }
    reply.data[at..at + 4].copy_from_slice(&id.to_le_bytes());
    reply.used += 4;
    reply.count += 1;
    true
}

fn archive_table(memory: &OwnMemory, table: &CacheTable, index: u64) -> Option<Vec<u8>> {
    let count = memory
        .u64(index + table.archive_count as u64)
        .filter(|&c| c <= MAX_ENTRIES)?;
    let base = memory.u64(index + table.archives as u64)?;
    let mut entries = vec![0u8; count as usize * table.archive_stride as usize];
    (count == 0 || memory.read(base, &mut entries)).then_some(entries)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn list_archives(
    memory: &OwnMemory,
    table: &CacheTable,
    index: u64,
    reply: &mut CacheReply,
) -> i32 {
    let Some(entries) = archive_table(memory, table, index) else {
        return cache_status::NOT_FOUND;
    };
    let mut hash = FNV_START;
    for (id, entry) in entries
        .chunks_exact(table.archive_stride as usize)
        .enumerate()
    {
        if u32_at(entry, table.archive_missing as usize) == Some(u32::MAX) {
            continue;
        }
        fnv(&mut hash, &(id as u32).to_le_bytes());
        fnv(&mut hash, &entry[..ENTRY_HASHED.min(entry.len())]);
        if !put_id(reply, id as u32) {
            return cache_status::TOO_MANY;
        }
    }
    reply.fingerprint = hash;
    cache_status::OK
}

fn list_files(
    memory: &OwnMemory,
    table: &CacheTable,
    index: u64,
    archive: i32,
    reply: &mut CacheReply,
) -> i32 {
    let Some(count) = memory.u64(index + table.archive_count as u64) else {
        return cache_status::NOT_FOUND;
    };
    if archive < 0 || archive as u64 >= count {
        return cache_status::NO_ARCHIVE;
    }
    let Some(base) = memory.ptr(index + table.archives as u64) else {
        return cache_status::NOT_FOUND;
    };
    let mut entry = vec![0u8; table.archive_stride as usize];
    if !memory.read(
        base + archive as u64 * table.archive_stride as u64,
        &mut entry,
    ) {
        return cache_status::NOT_FOUND;
    }
    if u32_at(&entry, table.archive_missing as usize) == Some(u32::MAX) {
        return cache_status::NO_ARCHIVE;
    }
    let mut hash = FNV_START;
    fnv(&mut hash, &entry[..ENTRY_HASHED.min(entry.len())]);
    reply.fingerprint = hash;
    let entry_u64 = |at: u32| {
        Some(u64::from_le_bytes(
            entry.get(at as usize..at as usize + 8)?.try_into().ok()?,
        ))
    };
    let (Some(file_count), Some(files)) = (entry_u64(table.file_count), entry_u64(table.files))
    else {
        return cache_status::NOT_FOUND;
    };
    if file_count > MAX_ENTRIES {
        return cache_status::NOT_FOUND;
    }
    let mut slots = vec![0u8; file_count as usize * table.file_stride as usize];
    if file_count > 0 && !memory.read(files, &mut slots) {
        return cache_status::NOT_FOUND;
    }
    let at = table.file_missing as usize;
    for (id, slot) in slots.chunks_exact(table.file_stride as usize).enumerate() {
        if slot
            .get(at..at + 8)
            .is_some_and(|b| b.iter().all(|&b| b == 0xff))
        {
            continue;
        }
        if !put_id(reply, id as u32) {
            return cache_status::TOO_MANY;
        }
    }
    cache_status::OK
}

unsafe fn read_files(
    memory: &OwnMemory,
    reader: Reader,
    index: u64,
    request: &CacheRequest,
    reply: &mut CacheReply,
) -> i32 {
    let count = request.count as usize;
    reply.count = count as u32;
    let mut full = false;
    for (slot, &[archive, file]) in reply.files[..count].iter_mut().zip(&request.files[..count]) {
        slot.offset = reply.used;
        slot.length = CACHE_FILE_NO_ROOM;
        if full {
            continue;
        }
        let mut out = [0u64; 2];
        reader(index, &mut out, archive, file, 0);
        let [control, buffer] = out;
        let length = (buffer != 0)
            .then(|| memory.u64(buffer + BUFFER_LENGTH))
            .flatten();
        let data = (buffer != 0)
            .then(|| memory.ptr(buffer + BUFFER_DATA))
            .flatten();
        match (length, data) {
            (Some(length), Some(data))
                if length as usize <= CACHE_DATA_SIZE - reply.used as usize =>
            {
                let at = reply.used as usize;
                if length == 0 || memory.read(data, &mut reply.data[at..at + length as usize]) {
                    slot.length = length as i32;
                    reply.used += length as u32;
                } else {
                    slot.length = CACHE_FILE_MISSING;
                }
            }
            (Some(_), Some(_)) => full = true,
            _ => slot.length = CACHE_FILE_MISSING,
        }
        release(control);
    }
    cache_status::OK
}

/// Draws each requested item through the client's icon builder, which caches them as the inventory's own draws do, and copies the pixels out.
unsafe fn draw_icons(
    memory: &OwnMemory,
    builder: IconBuilder,
    request: &CacheRequest,
    reply: &mut CacheReply,
) -> i32 {
    let count = request.count as usize;
    reply.count = count as u32;
    let mut full = false;
    for (slot, &[id, quantity]) in reply.files[..count].iter_mut().zip(&request.files[..count]) {
        slot.offset = reply.used;
        slot.length = CACHE_FILE_NO_ROOM;
        if full {
            continue;
        }
        if ICON_BYTES > CACHE_DATA_SIZE - reply.used as usize {
            full = true;
            continue;
        }
        let text = if id & ITEM_ICON_SHOW_COUNT != 0 {
            ICON_TEXT_COUNT
        } else {
            ICON_TEXT_OFF
        };
        let id = id & ITEM_ICON_ID_MASK;
        // the lookup gives a default definition for an id past the last item, which would draw as a stray icon
        if !(0..MAX_ITEM_ID).contains(&id) {
            slot.length = CACHE_FILE_MISSING;
            continue;
        }
        let mut out = [0u64; 2];
        builder(
            &mut out,
            id,
            quantity,
            ICON_BORDER,
            ICON_SHADOW,
            text,
            0,
            ITEM_ICON_WIDTH as i32,
            ITEM_ICON_HEIGHT as i32,
            EMPTY_TEXT.as_ptr(),
        );
        let [control, sprite] = out;
        let pixels = (sprite != 0)
            .then(|| memory.u64(sprite + SPRITE_COUNT))
            .flatten()
            .filter(|&count| count == (ITEM_ICON_WIDTH * ITEM_ICON_HEIGHT) as u64)
            .and_then(|_| memory.ptr(sprite + SPRITE_PIXELS));
        let at = reply.used as usize;
        let copied =
            pixels.is_some_and(|pixels| memory.read(pixels, &mut reply.data[at..at + ICON_BYTES]));
        if copied {
            slot.length = ICON_BYTES as i32;
            reply.used += ICON_BYTES as u32;
        } else {
            slot.length = CACHE_FILE_MISSING;
        }
        release(control);
    }
    cache_status::OK
}
