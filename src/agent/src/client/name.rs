use std::sync::Arc;
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use retour::GenericDetour;
use ipc::{
    read::{client, local_name_object, short_string_text, Memory, NAME_CLEAN},
    state::{name_status, Layout, NameStatus},
};

use super::{
    offsets::AppearanceDecoder,
    own::{note_game_thread, on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
    unload::HookCall,
};

#[derive(Clone, Copy)]
struct Kept {
    display: [u8; 24],
    account: [u8; 24],
    real: Option<[u8; 24]>,
}

#[derive(Default)]
struct Keeper {
    kept: Option<Kept>,
    sequence: u32,
    reported: Option<NameStatus>,
}

static KEEPER: Mutex<Keeper> = Mutex::new(Keeper { kept: None, sequence: 0, reported: None });
static ACTIVE: AtomicBool = AtomicBool::new(false);

type Decode = unsafe extern "system" fn(*mut c_void, *mut c_void) -> usize;

static DECODER: OnceLock<(GenericDetour<Decode>, u32)> = OnceLock::new();

fn text(string: &[u8; 24]) -> &[u8] {
    short_string_text(string).unwrap_or(&[])
}

fn names(memory: &impl Memory, object: u64) -> Option<([u8; 24], [u8; 24])> {
    let mut display = [0u8; 24];
    let mut clean = [0u8; 24];
    (memory.read(object, &mut display) && memory.read(object + NAME_CLEAN, &mut clean)).then_some((display, clean))
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

pub unsafe fn hook_decoder(module_base: u64, decoder: AppearanceDecoder) {
    if DECODER.get().is_some() {
        return;
    }
    let target: Decode = std::mem::transmute((module_base + decoder.function as u64) as *const c_void);
    let detour = match GenericDetour::new(target, decoded) {
        Ok(detour) => detour,
        Err(error) => {
            println!("[Agent]: Failed to hook the appearance decoder: {error}.\r\n");
            return;
        }
    };
    let _ = DECODER.set((detour, decoder.actor_id));
    match DECODER.get().unwrap().0.enable() {
        Ok(()) => println!("[Agent]: Appearance decoder hooked.\r\n"),
        Err(error) => println!("[Agent]: Failed to enable the appearance decoder hook: {error}.\r\n"),
    }
}

pub unsafe fn rehook_decoder() {
    if let Some((detour, _)) = DECODER.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook_decoder() {
    let Some((detour, _)) = DECODER.get() else { return };
    if detour.is_enabled() {
        match detour.disable() {
            Ok(()) => println!("[Agent]: Appearance decoder unhooked.\r\n"),
            Err(error) => println!("[Agent]: Failed to disable the appearance decoder hook: {error}.\r\n"),
        }
    }
}

fn decoder_hooked() -> bool {
    DECODER.get().is_some_and(|(detour, _)| detour.is_enabled())
}

unsafe extern "system" fn decoded(player: *mut c_void, buffer: *mut c_void) -> usize {
    let _call = HookCall::enter();
    let (detour, actor_id) = DECODER.get().unwrap();
    let result = detour.call(player, buffer);
    note_game_thread();
    if ACTIVE.load(Ordering::Acquire) {
        on_appearance(player as u64, *actor_id);
    }
    result
}

fn on_appearance(player: u64, actor_id: u32) {
    let (Some(layout), Ok(mut keeper)) = (LAYOUT.get(), KEEPER.lock()) else { return };
    let Some(kept) = keeper.kept else { return };
    let memory = OwnMemory;
    let local = client(&memory, layout).and_then(|client| memory.i32(client + layout.player.player_index as u64));
    if local.is_none() || memory.i32(player + actor_id as u64) != local {
        return;
    }
    let Some(object) = memory.ptr(player + layout.world.player_name as u64) else { return };
    let status = apply(&mut keeper, &memory, object, &kept);
    let real = keeper.kept.and_then(|k| k.real);
    let sequence = keeper.sequence;
    report(&mut keeper, sequence, status, real);
}

pub fn on_request() {
    let (Some(layout), Some(mapping)) = (LAYOUT.get(), shared()) else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_name_request() else { return };
    let Ok(mut keeper) = KEEPER.lock() else { return };
    keeper.sequence = sequence;
    let memory = OwnMemory;

    if on_other_thread() {
        report(&mut keeper, sequence, name_status::OTHER_THREAD, None);
        return;
    }
    if request.enabled == 0 {
        ACTIVE.store(false, Ordering::Release);
        restore(&mut keeper, &memory, layout);
        report(&mut keeper, sequence, name_status::OFF, None);
        return;
    }
    if !decoder_hooked() {
        report(&mut keeper, sequence, name_status::NOT_FOUND, None);
        return;
    }

    let previous = keeper.kept.filter(|k| text(&k.account) == text(&request.account));
    let kept = Kept { display: request.display, account: request.account, real: previous.and_then(|p| p.real) };
    let shown = previous.map(|p| p.display);
    keeper.kept = Some(kept);
    ACTIVE.store(true, Ordering::Release);
    let status = match local_name_object(&memory, layout) {
        Some(object) => apply_shown(&mut keeper, &memory, object, &kept, shown),
        None => name_status::WAITING,
    };
    let real = keeper.kept.and_then(|k| k.real);
    report(&mut keeper, sequence, status, real);
}

fn apply(keeper: &mut Keeper, memory: &impl Memory, object: u64, kept: &Kept) -> i32 {
    apply_shown(keeper, memory, object, kept, Some(kept.display))
}

fn apply_shown(keeper: &mut Keeper, memory: &impl Memory, object: u64, kept: &Kept, shown: Option<[u8; 24]>) -> i32 {
    let Some((display, clean)) = names(memory, object) else { return name_status::WAITING };
    if text(&clean) != text(&kept.account) {
        return name_status::WAITING;
    }
    if text(&display) == text(&kept.display) {
        return name_status::KEPT;
    }
    if short_string_text(&display).is_none() {
        return name_status::LONG_NAME;
    }
    if !shown.is_some_and(|shown| text(&display) == text(&shown)) {
        if let Some(k) = keeper.kept.as_mut() {
            k.real = Some(display);
        }
    }
    if memory.write(object, &kept.display) {
        name_status::KEPT
    } else {
        name_status::WAITING
    }
}

fn restore(keeper: &mut Keeper, memory: &impl Memory, layout: &Layout) {
    let Some(kept) = keeper.kept.take() else { return };
    let Some(real) = kept.real else { return };
    let Some(object) = local_name_object(memory, layout) else { return };
    let Some((display, clean)) = names(memory, object) else { return };
    if text(&clean) == text(&kept.account) && text(&display) == text(&kept.display) {
        memory.write(object, &real);
    }
}

fn report(keeper: &mut Keeper, sequence: u32, status: i32, real: Option<[u8; 24]>) {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let new = NameStatus { sequence, status, real: real.unwrap_or_default() };
    if keeper.reported != Some(new) {
        shared.write_name_status(&new);
        keeper.reported = Some(new);
    }
}
