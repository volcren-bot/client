use std::sync::Arc;
use std::sync::OnceLock;

use ipc::{
    read::{self, read_string, Memory},
    state::{game_val, item_name_status, NamesReply, ITEM_NAME_BATCH, ITEM_NAME_SIZE},
};

use super::{
    offsets::{GameVals, ObjTypeLookup},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
};

const MAX_ID: i32 = 1 << 20;
const USES: u64 = 0x8;
const WEAKS: u64 = 0xc;
const DISPOSE: u64 = 0x8;
const DELETE: u64 = 0x10;

type Lookup = unsafe extern "system" fn(*mut [u64; 2], i32) -> *mut [u64; 2];
type Release = unsafe extern "system" fn(u64);

static LOOKUP: OnceLock<(Lookup, ObjTypeLookup)> = OnceLock::new();

pub unsafe fn set_lookup(module_base: u64, rule: ObjTypeLookup) {
    let function: Lookup = std::mem::transmute((module_base + rule.function as u64) as *const ());
    let _ = LOOKUP.set((function, rule));
}

pub(super) unsafe fn release(control: u64) {
    if control == 0 {
        return;
    }
    let memory = OwnMemory;
    let Some(vtable) = memory.ptr(control) else { return };
    let uses = &*((control + USES) as *const std::sync::atomic::AtomicI32);
    if uses.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) != 1 {
        return;
    }
    if let Some(dispose) = memory.ptr(vtable + DISPOSE) {
        std::mem::transmute::<u64, Release>(dispose)(control);
    }
    let weaks = &*((control + WEAKS) as *const std::sync::atomic::AtomicI32);
    if weaks.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) != 1 {
        return;
    }
    if let Some(delete) = memory.ptr(vtable + DELETE) {
        std::mem::transmute::<u64, Release>(delete)(control);
    }
}

unsafe fn name(lookup: Lookup, rule: &ObjTypeLookup, id: i32) -> Option<String> {
    if !(0..MAX_ID).contains(&id) {
        return None;
    }
    let mut out = [0u64; 2];
    lookup(&mut out, id);
    let [control, obj] = out;
    let name = (obj != 0).then(|| read_string(&OwnMemory, obj + rule.name as u64)).flatten();
    release(control);
    name
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

pub fn on_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_item_name_request() else { return };
    let mut status = NamesReply { sequence, ..NamesReply::default() };
    let count = request.count as usize;
    status.status = if count > ITEM_NAME_BATCH {
        item_name_status::TOO_MANY
    } else if on_other_thread() {
        item_name_status::OTHER_THREAD
    } else if let Some((lookup, rule)) = LOOKUP.get() {
        status.count = count as u32;
        for (index, &id) in request.ids[..count].iter().enumerate() {
            if let Some(name) = unsafe { name(*lookup, rule, id) } {
                status.set_name(index, name.as_bytes());
            }
        }
        item_name_status::OK
    } else {
        item_name_status::NOT_FOUND
    };
    shared.write_item_name_status(&status);
}

pub(super) type Reader = unsafe extern "system" fn(u64, *mut [u64; 2], i32, i32, u8) -> u64;

static GAME_VALS: OnceLock<(Reader, GameVals)> = OnceLock::new();

pub(super) fn game_vals() -> Option<&'static (Reader, GameVals)> {
    GAME_VALS.get()
}

pub unsafe fn set_game_vals(module_base: u64, rule: GameVals) {
    let reader: Reader = std::mem::transmute((module_base + rule.reader as u64) as *const ());
    let _ = GAME_VALS.set((reader, rule));
}

pub(super) const BUFFER_LENGTH: u64 = 0x8;
pub(super) const BUFFER_DATA: u64 = 0x10;
const MAX_FILE: u64 = 1 << 20;

fn game_val_name(kind: u32, data: &[u8]) -> &[u8] {
    let data = match kind {
        game_val::TABLE => data.get(1..).unwrap_or_default(),
        game_val::INTERFACE | 13 => data,
        _ => return data,
    };
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

unsafe fn game_val(reader: Reader, index: u64, kind: u32, id: i32) -> Option<Vec<u8>> {
    let mut out = [0u64; 2];
    reader(index, &mut out, kind as i32, id, 0);
    let [control, buffer] = out;
    let memory = OwnMemory;
    let name = (buffer != 0)
        .then(|| {
            let length = memory.u64(buffer + BUFFER_LENGTH).filter(|&l| l <= MAX_FILE)?;
            let read = length.min(ITEM_NAME_SIZE as u64 * 4) as usize;
            let mut data = vec![0u8; read];
            (read == 0 || memory.read(memory.ptr(buffer + BUFFER_DATA)?, &mut data)).then(|| game_val_name(kind, &data).to_vec())
        })
        .flatten();
    release(control);
    name
}

pub fn on_game_val_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_game_val_request() else { return };
    let mut status = NamesReply { sequence, ..NamesReply::default() };
    let count = request.count as usize;
    let memory = OwnMemory;
    let index = LAYOUT.get().and_then(|layout| read::client(&memory, layout)).zip(GAME_VALS.get()).and_then(|(client, (reader, rule))| {
        Some((*reader, memory.ptr(client + rule.index as u64).filter(|&index| index != 0)?))
    });
    status.status = if count > ITEM_NAME_BATCH {
        item_name_status::TOO_MANY
    } else if on_other_thread() {
        item_name_status::OTHER_THREAD
    } else if let Some((reader, index)) = index {
        status.count = count as u32;
        for (i, &id) in request.ids[..count].iter().enumerate() {
            if let Some(name) = unsafe { game_val(reader, index, request.kind, id) } {
                status.set_name(i, &name);
            }
        }
        item_name_status::OK
    } else {
        item_name_status::NOT_FOUND
    };
    shared.write_game_val_status(&status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_game_val_format() {
        assert_eq!(game_val_name(game_val::OBJ, b"bucket_empty"), b"bucket_empty");
        assert_eq!(game_val_name(game_val::INTERFACE, b"inventory\0\0\0items\0\xff\xff"), b"inventory");
        assert_eq!(game_val_name(game_val::TABLE, b"\x01quests\0\x01name\0\0"), b"quests");
        assert_eq!(game_val_name(game_val::INTERFACE, b""), b"");
    }
}
