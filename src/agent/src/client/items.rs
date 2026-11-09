use std::sync::{
    atomic::{AtomicI32, Ordering},
    OnceLock,
};

use ipc::{
    read::{self, read_string, Memory},
    state::{game_val, item_name_status, NamesReply, ITEM_NAME_BATCH, ITEM_NAME_SIZE},
};

use super::{
    detour::function,
    offsets::{GameVals, ObjTypeLookup},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::mapping,
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
    let _ = LOOKUP.set((function(module_base + rule.function as u64), rule));
}

pub(super) unsafe fn release(control: u64) {
    if control == 0 {
        return;
    }
    let memory = OwnMemory;
    let Some(vtable) = memory.ptr(control) else {
        return;
    };
    let drop_one = |count: u64, call: u64| {
        let count = &*((control + count) as *const AtomicI32);
        if count.fetch_sub(1, Ordering::AcqRel) != 1 {
            return false;
        }
        if let Some(call) = memory.ptr(vtable + call) {
            function::<Release>(call)(control);
        }
        true
    };
    if drop_one(USES, DISPOSE) {
        drop_one(WEAKS, DELETE);
    }
}

unsafe fn name(lookup: Lookup, rule: &ObjTypeLookup, id: i32) -> Option<String> {
    if !(0..MAX_ID).contains(&id) {
        return None;
    }
    let mut out = [0u64; 2];
    lookup(&mut out, id);
    let [control, obj] = out;
    let name = (obj != 0)
        .then(|| read_string(&OwnMemory, obj + rule.name as u64))
        .flatten();
    release(control);
    name
}

fn names_reply(
    sequence: u32,
    ids: &[i32],
    count: usize,
    name: Option<impl Fn(i32) -> Option<Vec<u8>>>,
) -> NamesReply {
    let mut reply = NamesReply {
        sequence,
        ..NamesReply::default()
    };
    reply.status = if count > ITEM_NAME_BATCH {
        item_name_status::TOO_MANY
    } else if on_other_thread() {
        item_name_status::OTHER_THREAD
    } else if let Some(name) = name {
        reply.count = count as u32;
        for (index, &id) in ids[..count].iter().enumerate() {
            if let Some(name) = name(id) {
                reply.set_name(index, &name);
            }
        }
        item_name_status::OK
    } else {
        item_name_status::NOT_FOUND
    };
    reply
}

pub fn on_request() {
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_item_name_request() else {
        return;
    };
    let lookup = LOOKUP
        .get()
        .map(|(lookup, rule)| move |id| unsafe { name(*lookup, rule, id) }.map(String::into_bytes));
    shared.write_item_name_status(&names_reply(
        sequence,
        &request.ids,
        request.count as usize,
        lookup,
    ));
}

pub(super) type Reader = unsafe extern "system" fn(u64, *mut [u64; 2], i32, i32, u8) -> u64;

static GAME_VALS: OnceLock<(Reader, GameVals)> = OnceLock::new();

pub(super) fn game_vals() -> Option<&'static (Reader, GameVals)> {
    GAME_VALS.get()
}

pub unsafe fn set_game_vals(module_base: u64, rule: GameVals) {
    let _ = GAME_VALS.set((function(module_base + rule.reader as u64), rule));
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
            let length = memory
                .u64(buffer + BUFFER_LENGTH)
                .filter(|&l| l <= MAX_FILE)?;
            let read = length.min(ITEM_NAME_SIZE as u64 * 4) as usize;
            let mut data = vec![0u8; read];
            (read == 0 || memory.read(memory.ptr(buffer + BUFFER_DATA)?, &mut data))
                .then(|| game_val_name(kind, &data).to_vec())
        })
        .flatten();
    release(control);
    name
}

pub fn on_game_val_request() {
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_game_val_request() else {
        return;
    };
    let memory = OwnMemory;
    let index = LAYOUT
        .get()
        .and_then(|layout| read::client(&memory, layout))
        .zip(GAME_VALS.get())
        .and_then(|(client, (reader, rule))| {
            Some((*reader, memory.ptr(client + rule.index as u64)?))
        });
    let read =
        index.map(|(reader, index)| move |id| unsafe { game_val(reader, index, request.kind, id) });
    shared.write_game_val_status(&names_reply(
        sequence,
        &request.ids,
        request.count as usize,
        read,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_each_game_val_format() {
        assert_eq!(
            game_val_name(game_val::OBJ, b"bucket_empty"),
            b"bucket_empty"
        );
        assert_eq!(
            game_val_name(game_val::INTERFACE, b"inventory\0\0\0items\0\xff\xff"),
            b"inventory"
        );
        assert_eq!(
            game_val_name(game_val::TABLE, b"\x01quests\0\x01name\0\0"),
            b"quests"
        );
        assert_eq!(game_val_name(game_val::INTERFACE, b""), b"");
    }
}
