use std::{
    collections::HashMap,
    ffi::{c_void, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicIsize, AtomicPtr, Ordering},
        Mutex, OnceLock,
    },
    time::Instant,
};

use ipc::{
    varc_log::{kind, varc_log_memory_name, Change, Log, LOG_SIZE, TEXT_SIZE},
    read::Memory,
};
use retour::GenericDetour;
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Memory::{CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE},
            Threading::GetCurrentProcessId,
        },
    },
};

use super::{
    offsets::{VarcSetters, VARIANT_KIND, VARIANT_STRING},
    own::OwnMemory,
    unload::HookCall,
};

// A string is read from a buffer the client owns; this is far more than any varc string.
const MAX_TEXT: u64 = 1 << 20;

type SetInt = unsafe extern "system" fn(*mut c_void, i32, i32);
type GetInt = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type SetObject = unsafe extern "system" fn(*mut c_void, i32, *const u8);

static SET_INT: OnceLock<(GenericDetour<SetInt>, GetInt)> = OnceLock::new();
static SET_OBJECT: OnceLock<GenericDetour<SetObject>> = OnceLock::new();

// What each string varc was last seen set to, as the value before the next change.
type Strings = Option<HashMap<i32, (Vec<u8>, u32)>>;
static STRINGS: Mutex<Strings> = Mutex::new(None);

static LOG: AtomicPtr<Log> = AtomicPtr::new(std::ptr::null_mut());
static MAPPING: AtomicIsize = AtomicIsize::new(0);
static START: OnceLock<Instant> = OnceLock::new();

fn open_log() -> bool {
    if !LOG.load(Ordering::Acquire).is_null() {
        return true;
    }
    let name = CString::new(varc_log_memory_name(unsafe { GetCurrentProcessId() })).unwrap();
    unsafe {
        let Ok(mapping) = CreateFileMappingA(HANDLE::default(), None, PAGE_READWRITE, 0, LOG_SIZE as u32, PCSTR(name.as_ptr() as *const u8)) else {
            println!("[Agent]: Cannot create the varc log shared memory.\r\n");
            return false;
        };
        let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, LOG_SIZE);
        if view.Value.is_null() {
            let _ = CloseHandle(mapping);
            println!("[Agent]: Cannot map the varc log.\r\n");
            return false;
        }
        ipc::varc_log::init(view.Value as *mut Log);
        MAPPING.store(mapping.0 as isize, Ordering::Release);
        LOG.store(view.Value as *mut Log, Ordering::Release);
    }
    START.get_or_init(Instant::now);
    true
}

/// Closes the log. The hooks must be off and no hook call running.
pub unsafe fn close() {
    let log = LOG.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !log.is_null() {
        let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: log.cast() });
    }
    let mapping = MAPPING.swap(0, Ordering::AcqRel);
    if mapping != 0 {
        let _ = CloseHandle(HANDLE(mapping as *mut c_void));
    }
}

fn log(change: &Change) {
    let log = LOG.load(Ordering::Acquire);
    if log.is_null() {
        return;
    }
    let time_ms = START.get().map_or(0, |start| start.elapsed().as_millis() as u64);
    unsafe { ipc::varc_log::push(log, time_ms, change) };
}

// A string variant: the kind byte says so; the text is inline or behind a pointer at 0 with the length at 8.
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
    let (data, length) = if room[0] & 0x80 != 0 { (memory.u64(variant)?, memory.u64(variant + 8)?) } else { (variant, 0x17 - room[0] as u64) };
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
            log(&Change { index, kind: kind::INT, old_known: true, old_int: old, new_int: value, old_text: &[], old_length: 0, new_text: &[], new_length: 0 })
        }));
    }
}

unsafe extern "system" fn set_object(varcs: *mut c_void, index: i32, variant: *const u8) {
    let _call = HookCall::enter();
    let detour = SET_OBJECT.get().unwrap();
    if !variant.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let Some((text, length)) = read_text(variant as u64) else { return };
            let Ok(mut strings) = STRINGS.lock() else { return };
            let old = strings.get_or_insert_with(HashMap::new).insert(index, (text.clone(), length));
            if old.as_ref().is_some_and(|(old_text, old_length)| *old_text == text && *old_length == length) {
                return;
            }
            let (old_text, old_length) = old.clone().unzip();
            log(&Change {
                index,
                kind: kind::STRING,
                old_known: old.is_some(),
                old_int: 0,
                new_int: 0,
                old_text: old_text.as_deref().unwrap_or(&[]),
                old_length: old_length.unwrap_or(0),
                new_text: &text,
                new_length: length,
            });
        }));
    }
    detour.call(varcs, index, variant);
}

pub unsafe fn hook(module_base: u64, rule: VarcSetters) {
    if SET_INT.get().is_some() || !open_log() {
        return;
    }
    let at = |rva: u32| (module_base + rva as u64) as *const c_void;
    let get: GetInt = std::mem::transmute(at(rule.get_int));
    let int: SetInt = std::mem::transmute(at(rule.set_int));
    let object: SetObject = std::mem::transmute(at(rule.set_object));
    match (GenericDetour::new(int, set_int), GenericDetour::new(object, set_object)) {
        (Ok(int), Ok(object)) => {
            let _ = (SET_INT.set((int, get)), SET_OBJECT.set(object));
        }
        (int, object) => {
            for error in [int.err(), object.err()].into_iter().flatten() {
                println!("[Agent]: Failed to hook the varc setters: {error}.\r\n");
            }
            return;
        }
    }
    match (SET_INT.get().unwrap().0.enable(), SET_OBJECT.get().unwrap().enable()) {
        (Ok(()), Ok(())) => println!("[Agent]: Varc log hooked.\r\n"),
        (int, object) => println!("[Agent]: Failed to enable the varc hooks: {:?} {:?}.\r\n", int.err(), object.err()),
    }
}

pub unsafe fn rehook() {
    if let Some((detour, _)) = SET_INT.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
    if let Some(detour) = SET_OBJECT.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook() {
    if let Some((detour, _)) = SET_INT.get() {
        if detour.is_enabled() {
            if let Err(error) = detour.disable() {
                println!("[Agent]: Failed to disable the varc int hook: {error}.\r\n");
            }
        }
    }
    if let Some(detour) = SET_OBJECT.get() {
        if detour.is_enabled() {
            match detour.disable() {
                Ok(()) => println!("[Agent]: Varc log unhooked.\r\n"),
                Err(error) => println!("[Agent]: Failed to disable the varc object hook: {error}.\r\n"),
            }
        }
    }
}
