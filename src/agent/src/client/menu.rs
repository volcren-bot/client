use std::sync::{
    atomic::{AtomicU32, Ordering},
    OnceLock,
};

use ipc::{
    read::{self, Memory},
    state::{
        menu_kind, menu_open, menu_status, MenuEntryInfo, MenuGeometry, MENU_ENTRIES,
        MENU_SUBMENUS, MENU_SUBMENU_ENTRIES,
    },
};

use super::{
    detour::function,
    offsets::{Menu, MenuClick, MenuLayout, Selection},
    own::{on_other_thread, OwnMemory, LAYOUT},
    state::mapping,
};

const SEARCH: u64 = 0x2000;
const MAX_ENTRIES: u64 = 500;
const EXAMINE_PRIORITY: i32 = 0x7fff_fffe;
const ROW_SIZE: u64 = 24;
const ROW_INDEX: u64 = 8;

static MENU: OnceLock<(u64, Menu)> = OnceLock::new();
static VECTOR: AtomicU32 = AtomicU32::new(0);
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

pub fn set_menu_open(offset: u32) {
    let _ = OPEN.set(offset);
}

pub fn set_menu_layout(rule: MenuLayout) {
    let _ = LAYOUT_RULE.set(rule);
}

#[derive(Clone, Copy)]
struct MenuObject {
    module_base: u64,
    rule: &'static Menu,
    object: u64,
}

impl MenuObject {
    fn current(memory: &impl Memory) -> Option<Self> {
        let (module_base, rule) = MENU.get()?;
        let client = read::client(memory, LAYOUT.get()?)?;
        Some(Self {
            module_base: *module_base,
            rule,
            object: memory.ptr(client + rule.object as u64)?,
        })
    }

    fn entry_vtable(&self) -> u64 {
        self.module_base + self.rule.entry_vtable as u64
    }
}

fn entries_vector(memory: &impl Memory, object: u64, vtable: u64, size: u64) -> Option<u64> {
    let known = VECTOR.load(Ordering::Relaxed);
    if known != 0 {
        return Some(known as u64);
    }
    for offset in (0..SEARCH).step_by(8) {
        let (Some(begin), Some(end), Some(cap)) = (
            memory.u64(object + offset),
            memory.u64(object + offset + 8),
            memory.u64(object + offset + 16),
        ) else {
            continue;
        };
        if begin == 0
            || end <= begin
            || cap < end
            || (end - begin) % size != 0
            || (cap - begin) % size != 0
            || (end - begin) / size > MAX_ENTRIES
        {
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
    let (Some(ptr), Some(len)) = (memory.u64(address), memory.u64(address + 8)) else {
        return;
    };
    let len = (len as usize).min(out.len() - 1);
    if ptr != 0 && len > 0 {
        memory.read(ptr, &mut out[..len]);
    }
}

fn byte(memory: &impl Memory, address: u64) -> u8 {
    let mut byte = [0u8];
    memory.read(address, &mut byte);
    byte[0]
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
    info.cancel = byte(memory, e + rule.cancel as u64);
    Some(info)
}

fn minimenu(memory: &impl Memory, object: u64, vtable: u64) -> Option<u64> {
    let known = MINIMENU.load(Ordering::Relaxed);
    if known != u32::MAX {
        return Some(object + known as u64);
    }
    let offset = (0..SEARCH)
        .step_by(8)
        .find(|&offset| memory.u64(object + offset) == Some(vtable))?;
    MINIMENU.store(offset as u32, Ordering::Relaxed);
    Some(object + offset)
}

fn read_click(memory: &impl Memory, menu: MenuObject) -> Option<(MenuEntryInfo, i32, i32, i32)> {
    let click = CLICK.get()?;
    let minimenu = open_minimenu(memory, menu)?;
    let entry = minimenu + click.entry as u64;
    if memory.u64(entry)? != menu.entry_vtable() {
        return None;
    }
    let x = minimenu + click.click_x as u64;
    Some((
        read_entry(memory, entry, menu.rule)?,
        memory.i32(x)?,
        memory.i32(x + 4)?,
        memory.i32(minimenu + click.counter as u64)?,
    ))
}

fn read_open(memory: &impl Memory, menu: MenuObject) -> Option<bool> {
    let offset = *OPEN.get()?;
    Some(memory.u64(menu.object + offset as u64)? != 0)
}

#[repr(C)]
#[derive(Default)]
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

fn read_geometry(menu: MenuObject, minimenu: u64, path: &[u64]) -> Option<MenuGeometry> {
    let layout = LAYOUT_RULE.get()?;
    let function: unsafe extern "C" fn(u64, *mut LayoutOut, *const [u64; 3]) -> *mut LayoutOut =
        unsafe { function(menu.module_base + layout.function as u64) };
    let empty = 0u64;
    let begin = if path.is_empty() {
        &empty as *const u64 as u64
    } else {
        path.as_ptr() as u64
    };
    let end = begin + 8 * path.len() as u64;
    let vector = [begin, end, end];
    let mut out = LayoutOut::default();
    unsafe { function(minimenu + layout.view as u64, &mut out, &vector) };
    let [x, y, width, height] = out.menu;
    let [rows_x, rows_y, rows_width, rows_height] = out.rows;
    Some(MenuGeometry {
        x,
        y,
        width,
        height,
        rows_x,
        rows_y,
        rows_width,
        rows_height,
        header_height: out.header_height,
        row_height: out.row_height,
        scroll: out.scroll,
        margin: layout.margin,
    })
}

fn read_selection(memory: &impl Memory, menu: MenuObject) -> Option<(MenuEntryInfo, bool)> {
    let selection = SELECTION.get()?;
    let s = menu.object + selection.offset as u64;
    let mut info = MenuEntryInfo::default();
    string(memory, s + selection.verb as u64, &mut info.option);
    string(memory, s + selection.name as u64, &mut info.target);
    info.id = memory.i32(s + selection.id as u64)?;
    info.index = memory.i32(s + selection.child as u64)?;
    info.action = memory.i32(s + selection.item as u64)?;
    info.kind = byte(memory, s + selection.flags as u64);
    Some((info, byte(memory, s + selection.selected as u64) != 0))
}

fn read_raw_entries(memory: &impl Memory, menu: MenuObject) -> Option<Vec<MenuEntryInfo>> {
    let size = menu.rule.entry_size as u64;
    let vector = entries_vector(memory, menu.object, menu.entry_vtable(), size)?;
    let (begin, end) = (
        memory.u64(menu.object + vector)?,
        memory.u64(menu.object + vector + 8)?,
    );
    if end < begin || (end - begin) / size > MAX_ENTRIES {
        return None;
    }
    (0..(end - begin) / size)
        .map(|k| read_entry(memory, begin + k * size, menu.rule))
        .collect()
}

fn sorted(entries: Vec<MenuEntryInfo>) -> Vec<MenuEntryInfo> {
    let mut entries: Vec<_> = entries.into_iter().enumerate().collect();
    entries.sort_by(|(ia, a), (ib, b)| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                if rank(a) == 0 {
                    a.priority.cmp(&b.priority)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| ib.cmp(ia))
    });
    entries.into_iter().map(|(_, e)| e).collect()
}

fn open_minimenu(memory: &impl Memory, menu: MenuObject) -> Option<u64> {
    let click = CLICK.get()?;
    minimenu(
        memory,
        menu.object,
        menu.module_base + click.minimenu_vtable as u64,
    )
}

fn vector(memory: &impl Memory, at: u64, size: u64, max: u64) -> Option<(u64, u64)> {
    let (begin, end) = (memory.u64(at)?, memory.u64(at + 8)?);
    (end >= begin && (end - begin) % size == 0 && (end - begin) / size <= max)
        .then_some((begin, (end - begin) / size))
}

fn read_rows(
    memory: &impl Memory,
    menu: MenuObject,
    rows: u64,
    max: u64,
) -> Option<Vec<MenuEntryInfo>> {
    let layout = LAYOUT_RULE.get()?;
    let size = menu.rule.entry_size as u64;
    let (begin, count) = vector(memory, rows, ROW_SIZE, max)?;
    (0..count)
        .map(|k| {
            let row = begin + k * ROW_SIZE;
            let list = memory.u64(row)?;
            let known = LIST_ENTRIES.load(Ordering::Relaxed);
            let entries = if known != u32::MAX {
                known as u64
            } else {
                let vector = VECTOR.load(Ordering::Relaxed) as u64;
                let offset = list
                    .checked_sub(menu.object)
                    .filter(|&o| vector != 0 && o <= vector)?;
                LIST_ENTRIES.store((vector - offset) as u32, Ordering::Relaxed);
                vector - offset
            };
            let (first, n) = vector(memory, list + entries, size, MAX_ENTRIES)?;
            let index = memory.u64(row + ROW_INDEX)?;
            let entry = first + index * size;
            if index >= n || memory.u64(entry)? != menu.entry_vtable() {
                return None;
            }
            let mut info = read_entry(memory, entry, menu.rule)?;
            info.submenu = (memory.u64(row + layout.submenu as u64)? != 0) as u8;
            Some(info)
        })
        .collect()
}

fn open_path(memory: &impl Memory, minimenu: u64) -> Option<Vec<u64>> {
    let layout = LAYOUT_RULE.get()?;
    let (begin, count) = vector(
        memory,
        minimenu + layout.path as u64,
        8,
        MENU_SUBMENUS as u64,
    )?;
    let mut path = (0..count)
        .map(|k| memory.u64(begin + 8 * k))
        .collect::<Option<Vec<_>>>()?;
    let open = memory.u64(minimenu + layout.open_row as u64)?;
    if open != u64::MAX {
        path.push(open);
    }
    path.truncate(MENU_SUBMENUS);
    Some(path)
}

fn read_submenus(
    memory: &OwnMemory,
    menu: MenuObject,
    minimenu: u64,
) -> Vec<(u32, Vec<MenuEntryInfo>, MenuGeometry)> {
    let Some(layout) = LAYOUT_RULE.get() else {
        return Vec::new();
    };
    let path = open_path(memory, minimenu).unwrap_or_default();
    let mut rows = minimenu + layout.rows as u64;
    let mut submenus = Vec::new();
    for (depth, &row) in path.iter().enumerate() {
        let Some((begin, count)) = vector(memory, rows, ROW_SIZE, MAX_ENTRIES) else {
            break;
        };
        let Some(sub) = (row < count)
            .then(|| memory.u64(begin + row * ROW_SIZE + layout.submenu as u64))
            .flatten()
            .filter(|&s| s != 0)
        else {
            break;
        };
        let (Some(entries), Some(geometry)) = (
            read_rows(memory, menu, sub, MENU_SUBMENU_ENTRIES as u64),
            read_geometry(menu, minimenu, &path[..=depth]),
        ) else {
            break;
        };
        submenus.push((row as u32, entries, geometry));
        rows = sub;
    }
    submenus
}

pub enum Snapshot {
    Closed(Vec<MenuEntryInfo>),
    Open,
}

pub fn snapshot() -> Option<Snapshot> {
    if on_other_thread() {
        return None;
    }
    let memory = OwnMemory;
    let menu = MenuObject::current(&memory)?;
    if read_open(&memory, menu) == Some(true) {
        return Some(Snapshot::Open);
    }
    read_raw_entries(&memory, menu).map(|raw| Snapshot::Closed(sorted(raw)))
}

pub fn on_request() {
    let Some(mapping) = mapping() else { return };
    let shared = mapping.shared();
    let Some(sequence) = shared.read_menu_request() else {
        return;
    };
    let other_thread = on_other_thread();
    let memory = OwnMemory;
    let menu = (!other_thread)
        .then(|| MenuObject::current(&memory))
        .flatten();
    let raw = menu.and_then(|menu| read_raw_entries(&memory, menu));
    let (click, selection, open) = match menu.filter(|_| raw.is_some()) {
        Some(menu) => (
            read_click(&memory, menu),
            read_selection(&memory, menu),
            read_open(&memory, menu),
        ),
        None => (None, None, None),
    };
    let minimenu = menu
        .filter(|_| open == Some(true))
        .and_then(|menu| open_minimenu(&memory, menu).map(|minimenu| (menu, minimenu)));
    let entries = raw.map(|raw| match minimenu {
        Some((menu, minimenu)) => LAYOUT_RULE
            .get()
            .and_then(|layout| read_rows(&memory, menu, minimenu + layout.rows as u64, MAX_ENTRIES))
            .unwrap_or_else(|| sorted(raw)),
        None => sorted(raw),
    });
    let geometry = minimenu.and_then(|(menu, minimenu)| read_geometry(menu, minimenu, &[]));
    let submenus = match (minimenu, geometry) {
        (Some((menu, minimenu)), Some(_)) => read_submenus(&memory, menu, minimenu),
        _ => Vec::new(),
    };
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
        (
            reply.clicked,
            reply.click_x,
            reply.click_y,
            reply.click_age,
            reply.has_click,
        ) = (clicked, x, y, age, click.is_some() as u32);
        let (selected_entry, selected) = selection.unwrap_or_default();
        (reply.selection, reply.has_selection, reply.selected) =
            (selected_entry, selection.is_some() as u32, selected as u32);
        (reply.geometry, reply.has_geometry) =
            (geometry.unwrap_or_default(), geometry.is_some() as u32);
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
