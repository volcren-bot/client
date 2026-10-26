use std::sync::Arc;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    OnceLock,
};

use ipc::{
    read::{self, Memory},
    state::{menu_kind, menu_open, menu_status, MenuEntryInfo, MenuGeometry, MENU_ENTRIES, MENU_SUBMENUS, MENU_SUBMENU_ENTRIES},
};

use super::{
    offsets::{Menu, MenuClick, MenuLayout, Selection},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::{Mapping, LAYOUT_MAP},
};

const SEARCH: u64 = 0x2000;
const MAX_ENTRIES: u64 = 500;
const EXAMINE_PRIORITY: i32 = 0x7fff_fffe;
/// A row of the open menu: its entry list, the entry's index in it, and the rows of the submenu it opens.
const ROW_SIZE: u64 = 24;
const ROW_INDEX: u64 = 8;

static MENU: OnceLock<(u64, Menu)> = OnceLock::new();
static VECTOR: AtomicU32 = AtomicU32::new(0);
/// Where an entry list keeps its entries vector; the menu's own list is a sub-object of the menu object.
static LIST_ENTRIES: AtomicU32 = AtomicU32::new(u32::MAX);
static CLICK: OnceLock<MenuClick> = OnceLock::new();
static MINIMENU: AtomicU32 = AtomicU32::new(u32::MAX);
static SELECTION: OnceLock<Selection> = OnceLock::new();
static OPEN: OnceLock<u32> = OnceLock::new();
static LAYOUT_RULE: OnceLock<MenuLayout> = OnceLock::new();

pub fn set_menu(module_base: u64, rule: Menu) {
    let _ = MENU.set((module_base, rule));
}

pub fn set_menu_click(rule: MenuClick) {
    let _ = CLICK.set(rule);
}

pub fn set_selection(rule: Selection) {
    let _ = SELECTION.set(rule);
}

/// The menu object's field that is non-null while the menu is open.
pub fn set_menu_open(offset: u32) {
    let _ = OPEN.set(offset);
}

pub fn set_menu_layout(rule: MenuLayout) {
    let _ = LAYOUT_RULE.set(rule);
}

fn shared() -> Option<Arc<Mapping>> {
    LAYOUT_MAP.get().and_then(|map| map.memory())
}

fn entries_vector(memory: &impl Memory, object: u64, vtable: u64, size: u64) -> Option<u64> {
    let known = VECTOR.load(Ordering::Relaxed);
    if known != 0 {
        return Some(known as u64);
    }
    for offset in (0..SEARCH).step_by(8) {
        let (Some(begin), Some(end), Some(cap)) = (memory.u64(object + offset), memory.u64(object + offset + 8), memory.u64(object + offset + 16)) else {
            continue;
        };
        if begin == 0 || end <= begin || cap < end || (end - begin) % size != 0 || (cap - begin) % size != 0 || (end - begin) / size > MAX_ENTRIES {
            continue;
        }
        if memory.u64(begin) == Some(vtable) {
            VECTOR.store(offset as u32, Ordering::Relaxed);
            return Some(offset);
        }
    }
    None
}

fn string(memory: &impl Memory, address: u64, out: &mut [u8]) {
    let (Some(ptr), Some(len)) = (memory.u64(address), memory.u64(address + 8)) else { return };
    let len = (len as usize).min(out.len() - 1);
    if ptr != 0 && len > 0 {
        memory.read(ptr, &mut out[..len]);
    }
}

fn rank(entry: &MenuEntryInfo) -> u8 {
    if entry.cancel != 0 {
        3
    } else if entry.priority == EXAMINE_PRIORITY {
        2
    } else if entry.kind == menu_kind::TILE {
        1
    } else {
        0
    }
}

fn read_entry(memory: &impl Memory, e: u64, rule: &Menu) -> Option<MenuEntryInfo> {
    let mut info = MenuEntryInfo::default();
    string(memory, e + rule.option as u64, &mut info.option);
    string(memory, e + rule.target as u64, &mut info.target);
    let ids = e + rule.ids as u64;
    info.id = memory.i32(ids)?;
    info.index = memory.i32(ids + 4)?;
    info.action = memory.i32(ids + 8)?;
    info.priority = memory.i32(e + rule.priority as u64)?;
    let mut bytes = [0u8; 2];
    memory.read(e + rule.kind as u64, &mut bytes);
    info.kind = bytes[0];
    info.category = bytes[1];
    memory.read(e + rule.cancel as u64, &mut bytes[..1]);
    info.cancel = bytes[0];
    Some(info)
}

fn minimenu(memory: &impl Memory, object: u64, vtable: u64) -> Option<u64> {
    let known = MINIMENU.load(Ordering::Relaxed);
    if known != u32::MAX {
        return Some(object + known as u64);
    }
    let offset = (0..SEARCH).step_by(8).find(|&offset| memory.u64(object + offset) == Some(vtable))?;
    MINIMENU.store(offset as u32, Ordering::Relaxed);
    Some(object + offset)
}

fn read_click(memory: &impl Memory) -> Option<(MenuEntryInfo, i32, i32, i32)> {
    let (module_base, rule) = MENU.get()?;
    let click = CLICK.get()?;
    let client = read::client(memory, LAYOUT.get()?)?;
    let object = memory.ptr(client + rule.object as u64).filter(|&o| o != 0)?;
    let minimenu = minimenu(memory, object, module_base + click.minimenu_vtable as u64)?;
    let entry = minimenu + click.entry as u64;
    if memory.u64(entry)? != module_base + rule.entry_vtable as u64 {
        return None;
    }
    let x = minimenu + click.click_x as u64;
    Some((read_entry(memory, entry, rule)?, memory.i32(x)?, memory.i32(x + 4)?, memory.i32(minimenu + click.counter as u64)?))
}

/// Whether the choose-option menu is open, from the menu object's open field.
fn read_open(memory: &impl Memory) -> Option<bool> {
    let (_, rule) = MENU.get()?;
    let offset = *OPEN.get()?;
    let client = read::client(memory, LAYOUT.get()?)?;
    let object = memory.ptr(client + rule.object as u64).filter(|&o| o != 0)?;
    Some(memory.u64(object + offset as u64)? != 0)
}

/// What the client's layout routine writes: rectangles, heights, scroll offset and fields the agent doesn't use.
#[repr(C)]
struct LayoutOut {
    menu: [i32; 4],
    header: [i32; 4],
    rows: [i32; 4],
    header_height: i32,
    row_height: i32,
    rows_total: i32,
    scroll: i32,
    rest: [i32; 10],
}

const _: () = assert!(std::mem::size_of::<LayoutOut>() == 0x68);

/// Lays out the menu (empty path) or a submenu with the client's own routine; must run on the main thread.
fn read_geometry(minimenu: u64, path: &[u64]) -> Option<MenuGeometry> {
    let (module_base, _) = MENU.get()?;
    let layout = LAYOUT_RULE.get()?;
    let function: unsafe extern "C" fn(u64, *mut LayoutOut, *const [u64; 3]) -> *mut LayoutOut = unsafe { std::mem::transmute(module_base + layout.function as u64) };
    // a std::vector of row indices; an empty one (begin == end, like the client's own) is the menu
    let empty = 0u64;
    let begin = if path.is_empty() { &empty as *const u64 as u64 } else { path.as_ptr() as u64 };
    let end = begin + 8 * path.len() as u64;
    let vector = [begin, end, end];
    let mut out = LayoutOut { menu: [0; 4], header: [0; 4], rows: [0; 4], header_height: 0, row_height: 0, rows_total: 0, scroll: 0, rest: [0; 10] };
    unsafe { function(minimenu + layout.view as u64, &mut out, &vector) };
    let [x, y, width, height] = out.menu;
    let [rows_x, rows_y, rows_width, rows_height] = out.rows;
    Some(MenuGeometry { x, y, width, height, rows_x, rows_y, rows_width, rows_height, header_height: out.header_height, row_height: out.row_height, scroll: out.scroll, margin: layout.margin })
}

/// The component waiting for a target as an entry, plus whether one is selected at all.
fn read_selection(memory: &impl Memory) -> Option<(MenuEntryInfo, bool)> {
    let (_, rule) = MENU.get()?;
    let selection = SELECTION.get()?;
    let client = read::client(memory, LAYOUT.get()?)?;
    let object = memory.ptr(client + rule.object as u64).filter(|&o| o != 0)?;
    let s = object + selection.offset as u64;
    let mut info = MenuEntryInfo::default();
    string(memory, s + selection.verb as u64, &mut info.option);
    string(memory, s + selection.name as u64, &mut info.target);
    info.id = memory.i32(s + selection.id as u64)?;
    info.index = memory.i32(s + selection.child as u64)?;
    info.action = memory.i32(s + selection.item as u64)?;
    let mut byte = [0u8];
    memory.read(s + selection.flags as u64, &mut byte);
    info.kind = byte[0];
    memory.read(s + selection.selected as u64, &mut byte);
    Some((info, byte[0] != 0))
}

/// The menu's entries in the order the client keeps them.
fn read_raw_entries(memory: &impl Memory) -> Option<Vec<MenuEntryInfo>> {
    let (module_base, rule) = MENU.get()?;
    let client = read::client(memory, LAYOUT.get()?)?;
    let object = memory.ptr(client + rule.object as u64).filter(|&o| o != 0)?;
    let size = rule.entry_size as u64;
    let vector = entries_vector(memory, object, module_base + rule.entry_vtable as u64, size)?;
    let (begin, end) = (memory.u64(object + vector)?, memory.u64(object + vector + 8)?);
    if end < begin || (end - begin) / size > MAX_ENTRIES {
        return None;
    }
    (0..(end - begin) / size).map(|k| read_entry(memory, begin + k * size, rule)).collect()
}

/// The entries top to bottom as a hover shows them (the first is what a left click does).
fn sorted(entries: Vec<MenuEntryInfo>) -> Vec<MenuEntryInfo> {
    let mut entries: Vec<_> = entries.into_iter().enumerate().collect();
    entries.sort_by(|(ia, a), (ib, b)| {
        rank(a).cmp(&rank(b)).then_with(|| if rank(a) == 0 { a.priority.cmp(&b.priority) } else { std::cmp::Ordering::Equal }).then_with(|| ib.cmp(ia))
    });
    entries.into_iter().map(|(_, e)| e).collect()
}

/// The open minimenu, from the menu object.
fn open_minimenu(memory: &impl Memory) -> Option<(u64, u64)> {
    let (module_base, rule) = MENU.get()?;
    let click = CLICK.get()?;
    let client = read::client(memory, LAYOUT.get()?)?;
    let object = memory.ptr(client + rule.object as u64).filter(|&o| o != 0)?;
    Some((object, minimenu(memory, object, module_base + click.minimenu_vtable as u64)?))
}

/// A `std::vector`'s (begin, count) for elements of `size` bytes, if it looks sane.
fn vector(memory: &impl Memory, at: u64, size: u64, max: u64) -> Option<(u64, u64)> {
    let (begin, end) = (memory.u64(at)?, memory.u64(at + 8)?);
    (end >= begin && (end - begin) % size == 0 && (end - begin) / size <= max).then_some((begin, (end - begin) / size))
}

/// The entries of a menu level's rows, top to bottom; hidden entries have no row.
fn read_rows(memory: &impl Memory, object: u64, rows: u64, max: u64) -> Option<Vec<MenuEntryInfo>> {
    let (module_base, rule) = MENU.get()?;
    let layout = LAYOUT_RULE.get()?;
    let size = rule.entry_size as u64;
    let (begin, count) = vector(memory, rows, ROW_SIZE, max)?;
    (0..count)
        .map(|k| {
            let row = begin + k * ROW_SIZE;
            let list = memory.u64(row)?;
            let known = LIST_ENTRIES.load(Ordering::Relaxed);
            let entries = if known != u32::MAX {
                known as u64
            } else {
                // the first list seen is the menu's own, inside the menu object before its entries
                let vector = VECTOR.load(Ordering::Relaxed) as u64;
                let offset = list.checked_sub(object).filter(|&o| vector != 0 && o <= vector)?;
                LIST_ENTRIES.store((vector - offset) as u32, Ordering::Relaxed);
                vector - offset
            };
            let (first, n) = vector(memory, list + entries, size, MAX_ENTRIES)?;
            let index = memory.u64(row + ROW_INDEX)?;
            let entry = first + index * size;
            if index >= n || memory.u64(entry)? != module_base + rule.entry_vtable as u64 {
                return None;
            }
            let mut info = read_entry(memory, entry, rule)?;
            info.submenu = (memory.u64(row + layout.submenu as u64)? != 0) as u8;
            Some(info)
        })
        .collect()
}

/// The rows the open submenus are opened from, parents first, then the open row (-1 for none).
fn open_path(memory: &impl Memory, minimenu: u64) -> Option<Vec<u64>> {
    let layout = LAYOUT_RULE.get()?;
    let (begin, count) = vector(memory, minimenu + layout.path as u64, 8, MENU_SUBMENUS as u64)?;
    let mut path = (0..count).map(|k| memory.u64(begin + 8 * k)).collect::<Option<Vec<_>>>()?;
    let open = memory.u64(minimenu + layout.open_row as u64)?;
    if open != u64::MAX {
        path.push(open);
    }
    path.truncate(MENU_SUBMENUS);
    Some(path)
}

/// The open submenus, the one opened from the menu first; stops at a row that opens none, like the client.
fn read_submenus(memory: &OwnMemory, object: u64, minimenu: u64) -> Vec<(u32, Vec<MenuEntryInfo>, MenuGeometry)> {
    let Some(layout) = LAYOUT_RULE.get() else { return Vec::new() };
    let path = open_path(memory, minimenu).unwrap_or_default();
    let mut rows = minimenu + layout.rows as u64;
    let mut submenus = Vec::new();
    for (depth, &row) in path.iter().enumerate() {
        let Some((begin, count)) = vector(memory, rows, ROW_SIZE, MAX_ENTRIES) else { break };
        let Some(sub) = (row < count).then(|| memory.u64(begin + row * ROW_SIZE + layout.submenu as u64)).flatten().filter(|&s| s != 0) else { break };
        let (Some(entries), Some(geometry)) = (read_rows(memory, object, sub, MENU_SUBMENU_ENTRIES as u64), read_geometry(minimenu, &path[..=depth])) else { break };
        submenus.push((row as u32, entries, geometry));
        rows = sub;
    }
    submenus
}

pub fn on_request() {
    let Some(mapping) = shared() else { return };
    let shared = mapping.shared();
    let Some(sequence) = shared.read_menu_request() else { return };
    let other_thread = on_other_thread();
    let raw = if other_thread { None } else { read_raw_entries(&OwnMemory) };
    let (click, selection, open) = match raw {
        Some(_) => (read_click(&OwnMemory), read_selection(&OwnMemory), read_open(&OwnMemory)),
        None => (None, None, None),
    };
    let minimenu = if open == Some(true) { open_minimenu(&OwnMemory) } else { None };
    // an open menu is reported as drawn; otherwise as a hover would show it
    let entries = raw.map(|raw| match minimenu {
        Some((object, minimenu)) => LAYOUT_RULE.get().and_then(|layout| read_rows(&OwnMemory, object, minimenu + layout.rows as u64, MAX_ENTRIES)).unwrap_or_else(|| sorted(raw)),
        None => sorted(raw),
    });
    // the client only lays the menu out while it's open, so the agent doesn't either
    let geometry = minimenu.and_then(|(_, minimenu)| read_geometry(minimenu, &[]));
    let submenus = match (minimenu, geometry) {
        (Some((object, minimenu)), Some(_)) => read_submenus(&OwnMemory, object, minimenu),
        _ => Vec::new(),
    };
    // filled in place: with MENU_ENTRIES entries the reply doesn't belong on the window procedure's stack
    shared.write_menu_reply_with(|reply| {
        reply.sequence = sequence;
        reply.status = match &entries {
            _ if other_thread => menu_status::OTHER_THREAD,
            Some(_) => menu_status::OK,
            None => menu_status::NOT_FOUND,
        };
        let entries = entries.as_deref().unwrap_or_default();
        reply.count = entries.len().min(MENU_ENTRIES) as u32;
        reply.entries[..reply.count as usize].copy_from_slice(&entries[..reply.count as usize]);
        let (clicked, x, y, age) = click.unwrap_or_default();
        (reply.clicked, reply.click_x, reply.click_y, reply.click_age, reply.has_click) = (clicked, x, y, age, click.is_some() as u32);
        let (selected_entry, selected) = selection.unwrap_or_default();
        (reply.selection, reply.has_selection, reply.selected) = (selected_entry, selection.is_some() as u32, selected as u32);
        (reply.geometry, reply.has_geometry) = (geometry.unwrap_or_default(), geometry.is_some() as u32);
        reply.submenu_count = submenus.len() as u32;
        for (out, (row, entries, geometry)) in reply.submenus.iter_mut().zip(&submenus) {
            (out.row, out.geometry) = (*row, *geometry);
            out.count = entries.len().min(MENU_SUBMENU_ENTRIES) as u32;
            out.entries[..out.count as usize].copy_from_slice(&entries[..out.count as usize]);
        }
        reply.open = match open {
            Some(true) => menu_open::OPEN,
            Some(false) => menu_open::CLOSED,
            None => menu_open::UNKNOWN,
        };
    });
}
