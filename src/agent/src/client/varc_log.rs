use std::{
    collections::HashMap,
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Mutex, OnceLock},
};

use ipc::{
    read::Memory,
    varc_log::{kind, varc_log_memory_name, Change, Log, LOG_SIZE, TEXT_SIZE},
};
use retour::GenericDetour;

use super::{
    detour,
    log::SharedLog,
    offsets::{VarcSetters, VARIANT_KIND, VARIANT_STRING},
    own::OwnMemory,
    unload::HookCall,
};

const MAX_TEXT: u64 = 1 << 20;

type SetInt = unsafe extern "system" fn(*mut c_void, i32, i32);
type GetInt = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type SetObject = unsafe extern "system" fn(*mut c_void, i32, *const u8);

static SET_INT: OnceLock<(GenericDetour<SetInt>, GetInt)> = OnceLock::new();
static SET_OBJECT: OnceLock<GenericDetour<SetObject>> = OnceLock::new();

type Strings = Option<HashMap<i32, (Vec<u8>, u32)>>;
static STRINGS: Mutex<Strings> = Mutex::new(None);

static LOG: SharedLog<Log> = SharedLog::new();

pub unsafe fn close() {
    LOG.close();
}

fn log(change: &Change) {
    if let Some(log) = LOG.get() {
        unsafe { ipc::varc_log::push(log, LOG.elapsed().as_millis() as u64, change) };
    }
}

fn read_text(variant: u64) -> Option<(Vec<u8>, u32)> {
    let memory = OwnMemory;
    let mut tag = [0u8; 1];
    if !memory.read(variant + VARIANT_KIND as u64, &mut tag) || tag[0] != VARIANT_STRING {
        return None;
    }
    let mut room = [0u8; 1];
    if !memory.read(variant + 0x17, &mut room) {
        return None;
    }
    let (data, length) = if room[0] & 0x80 != 0 {
        (memory.u64(variant)?, memory.u64(variant + 8)?)
    } else {
        (variant, 0x17u64.checked_sub(room[0] as u64)?)
    };
    if length > MAX_TEXT {
        return None;
    }
    let mut bytes = vec![0u8; (length as usize).min(TEXT_SIZE)];
    if data == 0 || (!bytes.is_empty() && !memory.read(data, &mut bytes)) {
        return None;
    }
    Some((bytes, length as u32))
}

unsafe extern "system" fn set_int(varcs: *mut c_void, index: i32, value: i32) {
    let _call = HookCall::enter();
    let (detour, get) = SET_INT.get().unwrap();
    let old = catch_unwind(AssertUnwindSafe(|| get(varcs, index))).ok();
    detour.call(varcs, index, value);
    if let Some(old) = old.filter(|&old| old != value) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            log(&Change {
                index,
                kind: kind::INT,
                old_known: true,
                old_int: old,
                new_int: value,
                old_text: &[],
                old_length: 0,
                new_text: &[],
                new_length: 0,
            })
        }));
    }
}

unsafe extern "system" fn set_object(varcs: *mut c_void, index: i32, variant: *const u8) {
    let _call = HookCall::enter();
    let detour = SET_OBJECT.get().unwrap();
    if !variant.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let Some((text, length)) = read_text(variant as u64) else {
                return;
            };
            let Ok(mut strings) = STRINGS.lock() else {
                return;
            };
            let strings = strings.get_or_insert_with(HashMap::new);
            let new = (text, length);
            if strings.get(&index) == Some(&new) {
                return;
            }
            let old = strings.insert(index, new);
            let old_known = old.is_some();
            let (old_text, old_length) = old.unzip();
            log(&Change {
                index,
                kind: kind::STRING,
                old_known,
                old_int: 0,
                new_int: 0,
                old_text: old_text.as_deref().unwrap_or(&[]),
                old_length: old_length.unwrap_or(0),
                new_text: &strings[&index].0,
                new_length: length,
            });
        }));
    }
    detour.call(varcs, index, variant);
}

pub unsafe fn hook(module_base: u64, rule: VarcSetters) {
    if SET_INT.get().is_some()
        || !LOG.open(varc_log_memory_name, LOG_SIZE, "varc", ipc::varc_log::init)
    {
        return;
    }
    let at = |rva: u32| module_base + rva as u64;
    let what = "varc setters";
    let (Some(int), Some(object)) = (
        detour::create::<SetInt>(at(rule.set_int), set_int, what),
        detour::create::<SetObject>(at(rule.set_object), set_object, what),
    ) else {
        return;
    };
    let (int, _) = SET_INT.get_or_init(|| (int, detour::function(at(rule.get_int))));
    let object = SET_OBJECT.get_or_init(|| object);
    detour::enable_both(int, object, "varc", "Varc log hooked");
}

pub unsafe fn unhook() {
    detour::disable(
        SET_INT.get().map(|(detour, _)| detour),
        "varc int hook",
        None,
    );
    detour::disable(
        SET_OBJECT.get(),
        "varc object hook",
        Some("Varc log unhooked"),
    );
}
