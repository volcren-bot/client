use std::sync::Arc;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    OnceLock,
};

use retour::GenericDetour;
use ipc::{
    read::{self, short_string, Memory},
    state::{part, total_xp_status, Layout, TotalXpStatus},
};

use super::{
    offsets::ComponentInvalidate,
    own::{note_game_thread, on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
    unload::HookCall,
};

const XP_COUNTER: i32 = 122 << 16 | 10;
const MAX_TEXT: u64 = 4096;

static ENABLED: AtomicBool = AtomicBool::new(false);
static OFFSET: AtomicI64 = AtomicI64::new(0);

type Invalidate = unsafe extern "system" fn(u64, u64) -> usize;

static HOOK: OnceLock<(GenericDetour<Invalidate>, ComponentInvalidate)> = OnceLock::new();

fn commas(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() * 4 / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn find_number(text: &[u8], number: &[u8]) -> Option<usize> {
    let part_of_number = |b: u8| b.is_ascii_digit() || b == b',';
    (0..=text.len().checked_sub(number.len())?).find(|&at| {
        text[at..at + number.len()] == *number
            && (at == 0 || !part_of_number(text[at - 1]))
            && text.get(at + number.len()).is_none_or(|&b| !part_of_number(b))
    })
}

fn replace_total(text: &[u8], from: i64, to: i64) -> Option<Vec<u8>> {
    let (from, to) = (commas(from), commas(to));
    let at = find_number(text, from.as_bytes())?;
    let mut out = Vec::with_capacity(text.len() + to.len());
    out.extend_from_slice(&text[..at]);
    out.extend_from_slice(to.as_bytes());
    out.extend_from_slice(&text[at + from.len()..]);
    Some(out)
}

fn real_total(memory: &impl Memory, layout: &Layout) -> Option<i64> {
    if !layout.has(part::SKILLS) {
        return None;
    }
    let client = read::client(memory, layout)?;
    let xp = memory.i32s(client + layout.skills.xp as u64, layout.skills.count as usize)?;
    Some(xp.iter().map(|&x| x.max(0) as i64).sum())
}

fn swap(memory: &impl Memory, text: u64, from: i64, to: i64) -> bool {
    let mut string = [0u8; 24];
    if from == to || !memory.read(text, &mut string) {
        return false;
    }
    if string[23] & 0x80 == 0 {
        let Some(len) = 23usize.checked_sub(string[23] as usize) else { return false };
        let new = replace_total(&string[..len], from, to).and_then(|new| short_string(&new));
        return new.is_some_and(|new| memory.write(text, &new));
    }
    let data = u64::from_le_bytes(string[0..8].try_into().unwrap());
    let len = u64::from_le_bytes(string[8..16].try_into().unwrap());
    let (from_text, to_text) = (commas(from), commas(to));
    if len > MAX_TEXT || from_text.len() != to_text.len() {
        return false;
    }
    let mut bytes = vec![0u8; len as usize];
    if !memory.read(data, &mut bytes) {
        return false;
    }
    find_number(&bytes, from_text.as_bytes()).is_some_and(|at| memory.write(data + at as u64, to_text.as_bytes()))
}

pub unsafe fn hook(module_base: u64, rule: ComponentInvalidate) {
    if HOOK.get().is_some() {
        return;
    }
    let target: Invalidate = std::mem::transmute((module_base + rule.function as u64) as *const ());
    let detour = match GenericDetour::new(target, invalidated) {
        Ok(detour) => detour,
        Err(error) => {
            println!("[Agent]: Failed to hook the component invalidation: {error}.\r\n");
            return;
        }
    };
    let _ = HOOK.set((detour, rule));
    match HOOK.get().unwrap().0.enable() {
        Ok(()) => println!("[Agent]: Component invalidation hooked.\r\n"),
        Err(error) => println!("[Agent]: Failed to enable the component invalidation hook: {error}.\r\n"),
    }
}

pub unsafe fn rehook() {
    if let Some((detour, _)) = HOOK.get() {
        if !detour.is_enabled() {
            let _ = detour.enable();
        }
    }
}

pub unsafe fn unhook() {
    let Some((detour, _)) = HOOK.get() else { return };
    if detour.is_enabled() {
        match detour.disable() {
            Ok(()) => println!("[Agent]: Component invalidation unhooked.\r\n"),
            Err(error) => println!("[Agent]: Failed to disable the component invalidation hook: {error}.\r\n"),
        }
    }
}

unsafe extern "system" fn invalidated(client: u64, holder: u64) -> usize {
    let _call = HookCall::enter();
    let (detour, rule) = HOOK.get().unwrap();
    let result = detour.call(client, holder);
    note_game_thread();
    if ENABLED.load(Ordering::Acquire) {
        let memory = OwnMemory;
        let offset = OFFSET.load(Ordering::Relaxed);
        if let (Some(layout), Some(component)) = (LAYOUT.get(), memory.ptr(holder + rule.holder_component as u64)) {
            if let Some(total) = real_total(&memory, layout) {
                swap(&memory, component + rule.text as u64, total, total + offset);
            }
        }
    }
    result
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

fn report(shared: ipc::state::Shared<'_>, sequence: u32, status: i32) {
    shared.write_total_xp_status(&TotalXpStatus { sequence, status });
}

pub fn on_request() {
    let (Some(layout), Some(mapping)) = (LAYOUT.get(), shared()) else { return };
    let shared = mapping.shared();
    let Some((sequence, request)) = shared.read_total_xp_request() else { return };
    if on_other_thread() {
        report(shared, sequence, total_xp_status::OTHER_THREAD);
        return;
    }
    let Some((detour, rule)) = HOOK.get().filter(|(detour, _)| detour.is_enabled()) else {
        let status = if request.enabled == 0 { total_xp_status::OFF } else { total_xp_status::NOT_FOUND };
        report(shared, sequence, status);
        return;
    };

    let shown = if ENABLED.load(Ordering::Relaxed) { OFFSET.load(Ordering::Relaxed) } else { 0 };
    let wanted = if request.enabled != 0 { request.offset } else { 0 };
    OFFSET.store(wanted, Ordering::Relaxed);
    ENABLED.store(wanted != 0, Ordering::Release);

    let memory = OwnMemory;
    let counter = read::client(&memory, layout).filter(|_| layout.has(part::INTERFACES)).and_then(|client| {
        Some((client, read::component(&memory, &layout.interfaces, client, XP_COUNTER)?))
    });
    if let (Some(total), Some((client, component))) = (real_total(&memory, layout), counter) {
        let slot = rule.holder_component as usize / 8;
        if swap(&memory, component + rule.text as u64, total + shown, total + wanted) && rule.holder_component % 8 == 0 && slot < 4 {
            let mut holder = [0u64; 4];
            holder[slot] = component;
            unsafe { detour.call(client, holder.as_ptr() as u64) };
        }
    }
    let status = if wanted != 0 { total_xp_status::ON } else { total_xp_status::OFF };
    report(shared, sequence, status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_scripts() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(6_019_725), "6,019,725");
        assert_eq!(commas(509_552_478), "509,552,478");
        assert_eq!(commas(1_000_000_000), "1,000,000,000");
        assert_eq!(commas(-1_234), "-1,234");
    }

    #[test]
    fn replaces_the_whole_number_only() {
        assert_eq!(replace_total(b"6,019,725", 6_019_725, 7_123_456).unwrap(), b"7,123,456");
        assert_eq!(
            replace_total(b"Total XP: <col=ffffff>6,019,725</col>", 6_019_725, 7_123_456).unwrap(),
            b"Total XP: <col=ffffff>7,123,456</col>"
        );
        assert_eq!(replace_total(b"16,019,725", 6_019_725, 7_123_456), None);
        assert_eq!(replace_total(b"6,019,7250", 6_019_725, 7_123_456), None);
        assert_eq!(replace_total(b"Attack", 6_019_725, 7_123_456), None);
        assert_eq!(replace_total(b"999,999", 999_999, 1_000_001).unwrap(), b"1,000,001");
    }
}
