use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    pub object: u32,
    pub entry_vtable: u32,
    pub entry_size: u32,
    pub target: u32,
    pub option: u32,
    pub ids: u32,
    pub priority: u32,
    pub kind: u32,
    pub cancel: u32,
}

const EXAMINE_PRIORITY: u64 = 0x7fff_fffe;
const DEFAULT_PRIORITY: u64 = 0x7fff_ffff;
const WALK_KIND_COMPARE: u64 = 7;
const INLINE_STRING: u32 = 0x28;

fn function_instrs(pe: &Pe, begin: u32, end: u32) -> Vec<Instruction> {
    let end = end.max(begin);
    Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect()
}

fn ranker(pe: &Pe) -> Result<(u32, u32, u32), ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs = function_instrs(pe, begin, end);
        let head = &instrs[..instrs.len().min(24)];
        let cancel = head.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RDX
                && i.memory_size() == iced_x86::MemorySize::UInt8
                && i.op1_kind() == OpKind::Immediate8
                && i.immediate(1) == 0)
                .then(|| i.memory_displacement64() as u32)
        });
        let priority = head.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RDX
                && matches!(i.op1_kind(), OpKind::Immediate32)
                && i.immediate(1) == EXAMINE_PRIORITY)
                .then(|| i.memory_displacement64() as u32)
        });
        let (Some(cancel), Some(priority)) = (cancel, priority) else { continue };
        let mut linear = decode_at(pe, begin, 200);
        if let Some(padding) = linear.windows(2).position(|w| w.iter().all(|i| i.mnemonic() == Mnemonic::Int3)) {
            linear.truncate(padding);
        }
        let kind = linear.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Memory
                && i.memory_size() == iced_x86::MemorySize::UInt8
                && i.op1_kind() == OpKind::Immediate8
                && i.immediate(1) == WALK_KIND_COMPARE)
                .then(|| i.memory_displacement64() as u32)
        });
        if let Some(kind) = kind {
            found.push((cancel, priority, kind));
        }
    }
    one(found)
}

struct EntryLayout {
    function: u32,
    vtable: u32,
    strings: Vec<u32>,
    ids: u32,
}

fn leaf_start(pe: &Pe, site: u32) -> Option<u32> {
    let mut at = site as usize;
    while at > 0 && site as usize - at < 0x400 {
        if pe.image[at - 1] == 0xcc {
            return Some(at as u32);
        }
        at -= 1;
    }
    None
}

fn entry_constructor(pe: &Pe, priority: u32) -> Result<EntryLayout, ScanError> {
    let mut needle = vec![0xc7, 0x81];
    needle.extend_from_slice(&priority.to_le_bytes());
    needle.extend_from_slice(&(DEFAULT_PRIORITY as u32).to_le_bytes());
    let mut starts = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, w) in data.windows(needle.len()).enumerate() {
            if w == needle.as_slice() {
                starts.extend(leaf_start(pe, (section.rva + offset) as u32));
            }
        }
    }
    let mut found = Vec::new();
    for begin in starts {
        let mut instrs = decode_at(pe, begin, 80);
        if let Some(ret) = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Ret) {
            instrs.truncate(ret + 1);
        }
        let vtable = instrs.iter().find_map(|lea| {
            (lea.mnemonic() == Mnemonic::Lea && lea.is_ip_rel_memory_operand()).then(|| lea.ip_rel_memory_address() as u32)
        });
        let mut inline: HashMap<Register, u32> = HashMap::new();
        let mut strings = Vec::new();
        let mut ids = None;
        for i in &instrs {
            if i.mnemonic() == Mnemonic::Lea && i.memory_base() == Register::RCX && !i.is_ip_rel_memory_operand() {
                inline.insert(i.op0_register(), i.memory_displacement64() as u32);
            }
            if i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RCX && i.op1_kind() == OpKind::Register {
                let at = i.memory_displacement64() as u32;
                if inline.get(&i.op1_register()) == Some(&(at + INLINE_STRING)) && !strings.contains(&at) {
                    strings.push(at);
                }
            }
            if ids.is_none()
                && i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RCX
                && i.memory_size() == iced_x86::MemorySize::UInt64
                && i.op1_kind() == OpKind::Immediate32to64
                && i.immediate(1) == u64::MAX
            {
                ids = Some(i.memory_displacement64() as u32);
            }
        }
        strings.sort_unstable();
        if let (Some(vtable), Some(ids), 2) = (vtable, ids, strings.len()) {
            found.push((begin, vtable, strings[0], strings[1], ids));
        }
    }
    let (function, vtable, target, option, ids) = one(found)?;
    Ok(EntryLayout { function, vtable, strings: vec![target, option], ids })
}

fn calls_to(pe: &Pe, target: u32) -> Vec<u32> {
    direct_calls(pe).iter().copied().filter(|&(_, t)| t == target).map(|(site, _)| site).collect()
}

fn entry_size(pe: &Pe, constructor: u32) -> Result<(u32, u32), ScanError> {
    let mut found = Vec::new();
    for site in calls_to(pe, constructor) {
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else { continue };
        let window = &instrs[at.saturating_sub(12)..at];
        let size = window.iter().position(|i| i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::R8D && i.op1_kind() == OpKind::Immediate32).and_then(|m| {
            window[m..].iter().any(|i| i.mnemonic() == Mnemonic::Call).then(|| window[m].immediate(1) as u32)
        });
        if let Some(size) = size {
            found.push((size, begin));
        }
    }
    one(found)
}

fn stored_after_call(pe: &Pe, function: u32) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    for site in calls_to(pe, function) {
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else { continue };
        let store = instrs[at + 1..].iter().take(3).find_map(|i| {
            if i.mnemonic() == Mnemonic::Jmp && i.op0_kind() == OpKind::NearBranch64 {
                let target = i.near_branch_target() as u32;
                return instrs.iter().find(|j| j.ip32() == target).filter(|j| j.mnemonic() == Mnemonic::Mov).copied();
            }
            Some(*i).filter(|i| i.mnemonic() == Mnemonic::Mov)
        });
        if let Some(store) = store {
            if store.op0_kind() == OpKind::Memory && store.op1_register() == Register::RAX && store.memory_index() == Register::None {
                found.push((site, store.memory_displacement64() as u32));
            }
        }
    }
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuClick {
    pub minimenu_vtable: u32,
    pub entry: u32,
    pub click_x: u32,
    pub counter: u32,
}

fn lea_rdx_disp32(pe: &Pe, hit: u32) -> bool {
    let at = hit as usize;
    at >= 3 && matches!(pe.image[at - 3], 0x48 | 0x49) && pe.image[at - 2] == 0x8d && pe.image[at - 1] & 0xf8 == 0x90
}

fn click_setter(instrs: &[Instruction], menu: &Menu) -> Option<(u32, u32, u32)> {
    let head = &instrs[..instrs.len().min(16)];
    let copied_from = |source: Register| {
        let mut registers = HashSet::from([source]);
        registers.extend(
            head.iter()
                .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op1_register() == source)
                .map(|i| i.op0_register()),
        );
        registers
    };
    let (this, source, xs, ys) = (copied_from(Register::RCX), copied_from(Register::RDX), copied_from(Register::R8D), copied_from(Register::R9D));
    let lea = |i: &Instruction, bases: &HashSet<Register>, target: Register| {
        (i.mnemonic() == Mnemonic::Lea && i.op0_register() == target && bases.contains(&i.memory_base()) && i.memory_index() == Register::None)
            .then(|| i.memory_displacement64() as u32)
    };
    let string_slot = |field: u32| {
        instrs.windows(2).find_map(|w| {
            let from = lea(&w[0], &source, Register::RDX).filter(|&d| d == field)?;
            lea(&w[1], &this, Register::RCX).and_then(|to| to.checked_sub(from))
        })
    };
    let slot = string_slot(menu.target)?;
    if string_slot(menu.option)? != slot {
        return None;
    }
    let store = |i: &Instruction| {
        (i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && this.contains(&i.memory_base()) && i.memory_index() == Register::None)
            .then(|| i.memory_displacement64() as u32)
    };
    let counter = instrs.iter().find_map(|i| {
        store(i).filter(|_| i.memory_size() == iced_x86::MemorySize::UInt32 && i.op1_kind() == OpKind::Immediate32 && i.immediate(1) == 0)
    })?;
    let x = instrs.iter().find_map(|i| store(i).filter(|_| i.op1_kind() == OpKind::Register && xs.contains(&i.op1_register())))?;
    instrs
        .iter()
        .any(|i| store(i) == Some(x + 4) && i.op1_kind() == OpKind::Register && ys.contains(&i.op1_register()))
        .then_some((slot, x, counter))
}

fn slot_owner_vtable(pe: &Pe, entry_constructor: u32, slot: u32) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for site in calls_to(pe, entry_constructor) {
        let Some((begin, end)) = pe.function_containing(site) else { continue };
        let instrs = function_instrs(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else { continue };
        let builds_slot = instrs[..at].iter().rev().take(2).any(|i| {
            i.mnemonic() == Mnemonic::Lea && i.op0_register() == Register::RCX && i.memory_index() == Register::None && i.memory_displacement64() == slot as u64
        });
        if !builds_slot {
            continue;
        }
        found.extend(instrs.windows(2).filter_map(|w| {
            let vtable = (w[0].mnemonic() == Mnemonic::Lea && w[0].is_ip_rel_memory_operand()).then(|| w[0].ip_rel_memory_address() as u32)?;
            (w[1].mnemonic() == Mnemonic::Mov
                && w[1].op0_kind() == OpKind::Memory
                && w[1].memory_base() == Register::RCX
                && w[1].memory_displacement64() == 0
                && w[1].op1_kind() == OpKind::Register
                && w[1].op1_register() == w[0].op0_register())
            .then_some(vtable)
        }));
    }
    one(found)
}

pub fn find_menu_click(pe: &Pe, menu: &Menu) -> Result<MenuClick, ScanError> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for hit in dword_hits(pe, menu.option).into_iter().filter(|&hit| lea_rdx_disp32(pe, hit)) {
        let Some((begin, end)) = pe.function_containing(hit) else { continue };
        if seen.insert(begin) {
            found.extend(click_setter(&function_instrs(pe, begin, end.min(begin + 0x800)), menu));
        }
    }
    let (entry, click_x, counter) = one(found)?;
    let constructor = entry_constructor(pe, menu.priority)?.function;
    let minimenu_vtable = slot_owner_vtable(pe, constructor, entry)?;
    Ok(MenuClick { minimenu_vtable, entry, click_x, counter })
}

/// The component waiting for a target (Java's `widgetSelected`), in a sub-object of the menu object at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub offset: u32,
    pub verb: u32,
    pub name: u32,
    pub id: u32,
    pub child: u32,
    pub item: u32,
    pub flags: u32,
    pub selected: u32,
}

fn is_ret(i: &Instruction) -> bool {
    i.mnemonic() == Mnemonic::Ret
}

/// `mov rax, [rcx + menu.object]; ret`: the client's menu getter.
fn menu_getter(instrs: &[Instruction], object: u32) -> bool {
    matches!(instrs, [load, ret, ..] if load_from(load, Register::RCX) == Some(object) && load.op0_register() == Register::RAX && is_ret(ret))
}

/// `call <menu getter>; add rax, imm; ret`: a getter for a sub-object of the menu.
fn sub_object_getter(instrs: &[Instruction], getters: &HashSet<u32>) -> Option<u32> {
    let call = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Call)?;
    if call > 1 || !getters.contains(&(instrs[call].near_branch_target() as u32)) {
        return None;
    }
    let rest = &instrs[call + 1..];
    let add = rest.first().filter(|i| i.mnemonic() == Mnemonic::Add && i.op0_register() == Register::RAX && i.op1_kind() != OpKind::Register)?;
    rest[1..].iter().take(2).any(is_ret).then(|| add.immediate(1) as u32)
}

/// `cmp byte [rcx+F], 0; ...; mov byte [rcx+F], 0` at a function's start: Java's `ug()`, which clears the selection.
fn deselect_flag(instrs: &[Instruction]) -> Option<u32> {
    let head = &instrs[..instrs.len().min(8)];
    let flag = head.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Cmp
            && i.memory_base() == Register::RCX
            && i.memory_index() == Register::None
            && i.memory_size() == iced_x86::MemorySize::UInt8
            && i.op1_kind() == OpKind::Immediate8
            && i.immediate(1) == 0)
            .then(|| i.memory_displacement64() as u32)
    })?;
    head.iter()
        .any(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RCX
                && i.memory_displacement64() == flag as u64
                && i.op1_kind() == OpKind::Immediate8
                && i.immediate(1) == 0
        })
        .then_some(flag)
}

/// Java's `ut()`, the setter: stores `selected` = 1, the component id, child, item id and mask, then the verb and name strings.
fn selection_setter(instrs: &[Instruction], selected: u32) -> Option<(u32, u32, u32, u32, u32, u32)> {
    let head = &instrs[..instrs.len().min(24)];
    let mut this = HashSet::from([Register::RCX]);
    let mut at: HashMap<Register, u32> = HashMap::new();
    let (mut id, mut child, mut item, mut flags, mut sets) = (None, None, None, None, false);
    let mut strings = Vec::new();
    let mut rcx = Some(0u32);
    for i in head {
        let store_base = (i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_index() == Register::None).then(|| i.memory_base());
        if let Some(base) = store_base {
            let field = if this.contains(&base) { Some(i.memory_displacement64() as u32) } else { at.get(&base).map(|&d| d.wrapping_add(i.memory_displacement64() as u32)) };
            match (field, i.op1_kind()) {
                (Some(f), OpKind::Immediate8) if f == selected && i.immediate(1) == 1 => sets = true,
                (Some(f), OpKind::Register) if i.op1_register() == Register::EDX => id = Some(f),
                (Some(f), OpKind::Register) if i.op1_register() == Register::R8D => child = Some(f),
                (Some(f), OpKind::Register) if i.op1_register() == Register::R9D => item = Some(f),
                (Some(f), OpKind::Register) if i.op1_register() == Register::AL => flags = Some(f),
                _ => {}
            }
        }
        match i.mnemonic() {
            Mnemonic::Mov if i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op1_register() == Register::RCX => {
                this.insert(i.op0_register());
            }
            Mnemonic::Mov if i.op0_register() == Register::RCX && i.op1_kind() == OpKind::Register && this.contains(&i.op1_register()) => rcx = Some(0),
            Mnemonic::Lea if i.op0_kind() == OpKind::Register && this.contains(&i.memory_base()) && i.memory_index() == Register::None => {
                let d = i.memory_displacement64() as u32;
                if i.op0_register() == Register::RCX {
                    rcx = Some(d);
                } else {
                    at.insert(i.op0_register(), d);
                }
            }
            Mnemonic::Call if sets => {
                strings.push(rcx?);
                rcx = None;
                if strings.len() == 2 {
                    break;
                }
            }
            Mnemonic::Call => rcx = None,
            _ if i.op0_kind() == OpKind::Register && i.op0_register().full_register() == Register::RCX => rcx = None,
            _ => {}
        }
    }
    match (sets, id, child, item, flags, strings.as_slice()) {
        (true, Some(id), Some(child), Some(item), Some(flags), &[verb, name]) => Some((verb, name, id, child, item, flags)),
        _ => None,
    }
}

pub fn find_selection(pe: &Pe, menu: &Menu) -> Result<Selection, ScanError> {
    let mut getters = HashSet::new();
    let mut sub_objects: HashMap<u32, u32> = HashMap::new();
    // the menu getter is a leaf without unwind info, so it's found by its bytes (`mov rax, [rcx+disp32]`)
    for hit in dword_hits(pe, menu.object) {
        let Some(begin) = hit.checked_sub(3) else { continue };
        if pe.image[begin as usize..hit as usize] == [0x48, 0x8b, 0x81] && menu_getter(&decode_at(pe, begin, 2), menu.object) {
            getters.insert(begin);
        }
    }
    let heads: Vec<(u32, Vec<Instruction>)> = pe.functions.iter().map(|&(begin, end)| (begin, decode_at(pe, begin, 8).into_iter().filter(|i| i.ip32() < end).collect())).collect();
    for (begin, instrs) in &heads {
        if let Some(offset) = sub_object_getter(instrs, &getters) {
            sub_objects.insert(*begin, offset);
        }
    }
    let calls = direct_calls(pe);
    let mut found = Vec::new();
    for &(site, target) in calls {
        let Some(&offset) = sub_objects.get(&target) else { continue };
        let after = decode_at(pe, site, 5);
        let passes_rax = after.iter().skip(1).take(3).take_while(|i| i.mnemonic() != Mnemonic::Call).any(|i| {
            i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::RCX && i.op1_kind() == OpKind::Register && i.op1_register() == Register::RAX
        });
        let Some(next) = after.iter().skip(1).take(4).find(|i| i.mnemonic() == Mnemonic::Call) else { continue };
        if passes_rax && next.op0_kind() == OpKind::NearBranch64 {
            if let Some(flag) = deselect_flag(&decode_at(pe, next.near_branch_target() as u32, 8)) {
                found.push((offset, flag));
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    let (offset, selected) = one(found)?;
    let setters = pe.functions.iter().filter_map(|&(begin, end)| selection_setter(&function_instrs(pe, begin, end.min(begin + 0x200)), selected)).collect();
    let (verb, name, id, child, item, flags) = one(setters)?;
    Ok(Selection { offset, verb, name, id, child, item, flags, selected })
}

/// The menu's open flag: the pointer field set to the minimenu while open, found as the 64-bit store of a `lea` in the open-at-point routine.
pub fn find_menu_open(pe: &Pe, click: &MenuClick) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs = function_instrs(pe, begin, end);
        let loads_entry = instrs.iter().any(|i| i.mnemonic() == Mnemonic::Lea && i.memory_base() != Register::RIP && i.memory_displacement64() == click.entry as u64);
        if !loads_entry {
            continue;
        }
        for (n, i) in instrs.iter().enumerate() {
            if i.mnemonic() != Mnemonic::Mov || i.op0_kind() != OpKind::Memory || i.memory_base() == Register::RIP || i.memory_size() != iced_x86::MemorySize::UInt64 {
                continue;
            }
            if i.op1_kind() != OpKind::Register || !i.op1_register().is_gpr64() {
                continue;
            }
            let reg = i.op1_register();
            let loads_minimenu = instrs[n.saturating_sub(12)..n].iter().any(|j| j.mnemonic() == Mnemonic::Lea && j.op0_register() == reg && j.memory_base() != Register::RIP);
            // the store comes just after the call that fills the entry
            let after_entry_call = instrs[n.saturating_sub(8)..n].iter().any(|j| j.mnemonic() == Mnemonic::Call);
            let entry_before = instrs[..n].iter().rev().take(16).any(|j| j.mnemonic() == Mnemonic::Lea && j.memory_base() != Register::RIP && j.memory_displacement64() == click.entry as u64);
            // an object field, not a stack slot or a small header
            let field = !matches!(i.memory_base(), Register::RSP | Register::RBP) && (0x100..0x2000).contains(&i.memory_displacement64());
            if loads_minimenu && after_entry_call && entry_before && field {
                found.push(i.memory_displacement64() as u32);
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    one(found)
}

/// `call qword [rax+slot]` (a virtual call through the vtable loaded into rax).
fn virtual_call(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RAX && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

/// `mov qword [rax+F], r64`: a store into the object a virtual call just returned.
fn store_into_result(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.memory_base() == Register::RAX
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt64
        && i.op1_kind() == OpKind::Register)
        .then(|| i.memory_displacement64() as u32)
}

/// The routine placing the minimenu: two `call [rax+slot]` storing the usable area, then the opening point eight bytes before it. Returns the slot.
fn placement_setter(instrs: &[Instruction]) -> Option<u32> {
    let head = &instrs[..instrs.len().min(24)];
    let stores: Vec<(u32, u32)> = head
        .iter()
        .enumerate()
        .filter_map(|(n, call)| {
            let slot = virtual_call(call)?;
            let field = head[n + 1..].iter().take(3).take_while(|i| i.mnemonic() != Mnemonic::Call).find_map(store_into_result)?;
            Some((slot, field))
        })
        .collect();
    match stores.as_slice() {
        [(slot, area), (again, point), ..] if slot == again && *area == point + 8 => Some(*slot),
        _ => None,
    }
}

/// `lea rax, [rcx+D]; ret`: a getter for a sub-object at D.
fn sub_object_at(instrs: &[Instruction]) -> Option<u32> {
    match instrs {
        [lea, ret, ..] if lea.mnemonic() == Mnemonic::Lea && lea.op0_register() == Register::RAX && lea.memory_base() == Register::RCX && lea.memory_index() == Register::None && is_ret(ret) => {
            Some(lea.memory_displacement64() as u32)
        }
        _ => None,
    }
}

/// The open menu as the client lays it out: `view` offset, the layout `function`, the `rows` vector, the `margin`, the submenu `path` and `open_row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuLayout {
    pub view: u32,
    pub function: u32,
    pub rows: u32,
    pub margin: i32,
    pub path: u32,
    pub open_row: u32,
    pub submenu: u32,
}

/// The stay-open check: lays each open menu out and grows it by a handler field (`mov ecx, [r13+M]`). Returns M.
fn stay_open_check(instrs: &[Instruction], layout: u32) -> Option<u32> {
    let call = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64 && i.near_branch_target() == layout as u64)?;
    let after = &instrs[call + 1..instrs.len().min(call + 16)];
    after.iter().enumerate().find_map(|(n, load)| {
        let base = load.memory_base();
        let field = (load.mnemonic() == Mnemonic::Mov
            && load.op0_kind() == OpKind::Register
            && load.op1_kind() == OpKind::Memory
            && load.memory_size() == iced_x86::MemorySize::UInt32
            && load.memory_index() == Register::None
            && !matches!(base, Register::RSP | Register::RBP | Register::RIP | Register::RAX))
        .then(|| load.memory_displacement64() as u32)?;
        let reg = load.op0_register();
        after[n + 1..].iter().any(|i| i.mnemonic() == Mnemonic::Add && i.op0_register() == reg && i.op1_kind() == OpKind::Register && i.op1_register() == reg).then_some(field)
    })
}

/// The handler's constructor stores its vtable and the margin as an immediate (`mov dword [rcx+M], imm`).
fn margin_store(instrs: &[Instruction], field: u32) -> Option<(u32, i32)> {
    let head = &instrs[..instrs.len().min(20)];
    let vtable = head.windows(4).find_map(|w| {
        let lea = &w[0];
        (lea.mnemonic() == Mnemonic::Lea && lea.is_ip_rel_memory_operand()).then_some(())?;
        w[1..].iter().any(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RCX && i.memory_displacement64() == 0 && i.op1_kind() == OpKind::Register && i.op1_register() == lea.op0_register()).then(|| lea.ip_rel_memory_address() as u32)
    })?;
    let margin = head.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Memory
            && i.memory_base() == Register::RCX
            && i.memory_displacement64() == field as u64
            && i.memory_size() == iced_x86::MemorySize::UInt32
            && i.op1_kind() == OpKind::Immediate32)
            .then(|| i.immediate(1) as i32)
    })?;
    Some((vtable, margin))
}

fn calls(pe: &Pe, function: u32, target: u32) -> bool {
    decode_at(pe, function, 200).iter().take_while(|i| !is_ret(i)).any(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64 && i.near_branch_target() == target as u64)
}

/// The stay-open check copies the minimenu's parent rows and appends the open row unless -1. Returns (P, O).
fn open_path(instrs: &[Instruction]) -> Option<(u32, u32)> {
    instrs.iter().enumerate().find_map(|(n, load)| {
        let cmp = instrs.get(n + 1)?;
        let base = load.memory_base();
        let open_row = load_from(load, base).filter(|_| load.memory_size() == iced_x86::MemorySize::UInt64 && !matches!(base, Register::RSP | Register::RBP | Register::RIP))?;
        (cmp.mnemonic() == Mnemonic::Cmp && cmp.op0_kind() == OpKind::Register && cmp.op0_register() == load.op0_register() && cmp.op1_kind() == OpKind::Immediate8to64 && cmp.immediate(1) == u64::MAX)
            .then_some(())?;
        let call = instrs[..n].iter().rposition(|i| i.mnemonic() == Mnemonic::Call).filter(|&c| n - c <= 2)?;
        let before = &instrs[call.saturating_sub(10)..call];
        let begin = before.iter().find_map(|i| load_from(i, base).filter(|_| i.op0_register() == Register::RDX))?;
        let end = before.iter().find_map(|i| load_from(i, base).filter(|_| i.op0_register() == Register::R8))?;
        (end == begin + 8).then_some((begin, open_row))
    })
}

/// The minimenu method telling whether a row path opens a submenu: tests the found row's pointer `[rax+S]`. Returns S.
fn submenu_check(instrs: &[Instruction], rows: u32) -> Option<u32> {
    let head = &instrs[..instrs.len().min(16)];
    let walks = head.iter().any(|i| i.mnemonic() == Mnemonic::Lea && i.memory_base() == Register::RCX && i.memory_index() == Register::None && i.memory_displacement64() == rows as u64);
    let tested = head.windows(2).find_map(|w| {
        let field = load_from(&w[0], Register::RAX).filter(|_| w[0].memory_size() == iced_x86::MemorySize::UInt64)?;
        let reg = w[0].op0_register();
        (w[1].mnemonic() == Mnemonic::Test && w[1].op0_register() == reg && w[1].op1_kind() == OpKind::Register && w[1].op1_register() == reg).then_some(field)
    })?;
    walks.then_some(tested)
}

/// The stay-open check: the one function that grows each laid-out menu by the handler's margin. Returns its start and the margin field.
fn find_stay_open_check(pe: &Pe, layout: u32) -> Result<(u32, u32), ScanError> {
    one(pe.functions.iter().filter_map(|&(begin, end)| stay_open_check(&function_instrs(pe, begin, end), layout).map(|field| (begin, field))).collect())
}

/// The margin around the menu that keeps the mouse inside it: the immediate the handler's constructor stores.
fn find_margin(pe: &Pe, image: &[u8], base: u64, check: u32, field: u32) -> Result<i32, ScanError> {
    let slot = |vtable: u32, k: usize| image.get(vtable as usize + k * 8..vtable as usize + k * 8 + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).and_then(|va| va.checked_sub(base));
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let Some((vtable, margin)) = margin_store(&function_instrs(pe, begin, end.min(begin + 0x200)), field) else { continue };
        if (0..16).filter_map(|k| slot(vtable, k)).any(|method| calls(pe, method as u32, check)) {
            found.push(margin);
        }
    }
    found.sort_unstable();
    found.dedup();
    one(found)
}

/// The minimenu's row count, `(end - begin) / 24` of its rows vector.
fn row_counter(instrs: &[Instruction]) -> Option<u32> {
    let [copy, end, begin, magic, ..] = instrs else { return None };
    let copies = copy.mnemonic() == Mnemonic::Mov && copy.op0_register() == Register::RAX && copy.op1_kind() == OpKind::Register && copy.op1_register() == Register::RCX;
    let end_at = load_from(end, Register::RCX).filter(|_| end.op0_register() == Register::RCX)?;
    let begin_at = (begin.mnemonic() == Mnemonic::Sub && begin.op0_register() == Register::RCX && begin.memory_base() == Register::RAX).then(|| begin.memory_displacement64() as u32)?;
    let divides = magic.mnemonic() == Mnemonic::Mov && magic.op1_kind() == OpKind::Immediate64 && magic.immediate(1) == 0x2aaa_aaaa_aaaa_aaab;
    (copies && divides && end_at == begin_at + 8).then_some(begin_at)
}

/// The layout routine refuses 5 or more submenus and lays out the parent by calling itself; decoded past the first `.pdata` chunk.
fn is_layout_routine(pe: &Pe, begin: u32) -> bool {
    let instrs = decode_at(pe, begin, 300);
    let head = &instrs[..instrs.len().min(16)];
    let checks_depth = head.iter().enumerate().any(|(n, sar)| {
        sar.mnemonic() == Mnemonic::Sar
            && sar.op0_register() == Register::RAX
            && sar.immediate(1) == 3
            && head[n + 1..].iter().take(3).any(|cmp| {
                cmp.mnemonic() == Mnemonic::Cmp && cmp.op0_register() == Register::RAX && cmp.op1_kind() != OpKind::Register && cmp.immediate(1) == 5
            })
    });
    checks_depth && instrs.iter().any(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64 && i.near_branch_target() == begin as u64)
}

pub fn find_menu_layout(pe: &Pe, click: &MenuClick) -> Result<MenuLayout, ScanError> {
    let image = pe.image;
    let nt = u32_at(image, 0x3c).ok_or(ScanError::InvalidImage)? as usize;
    let base = image.get(nt + 48..nt + 56).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).ok_or(ScanError::InvalidImage)?;
    let mut views = Vec::new();
    for &(begin, end) in &pe.functions {
        let Some(slot) = placement_setter(&function_instrs(pe, begin, end.min(begin + 0x100))) else { continue };
        let at = click.minimenu_vtable as usize + slot as usize;
        let Some(getter) = image.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).and_then(|va| va.checked_sub(base)) else { continue };
        views.extend(sub_object_at(&decode_at(pe, getter as u32, 2)));
    }
    views.sort_unstable();
    views.dedup();
    let view = one(views)?;
    let function = one(pe.functions.iter().map(|&(begin, _)| begin).filter(|&begin| is_layout_routine(pe, begin)).collect())?;
    let methods: Vec<u32> = (0..16)
        .filter_map(|slot| {
            let at = click.minimenu_vtable as usize + slot * 8;
            Some(image.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes)?.checked_sub(base)? as u32)
        })
        .collect();
    let rows = one(methods.iter().filter_map(|&method| row_counter(&decode_at(pe, method, 4))).collect())?;
    let submenu = one(methods.iter().filter_map(|&method| submenu_check(&decode_at(pe, method, 16), rows)).collect())?;
    let (check, field) = find_stay_open_check(pe, function)?;
    let margin = find_margin(pe, image, base, check, field)?;
    let end = pe.functions.iter().find(|&&(begin, _)| begin == check).map_or(check, |&(_, end)| end);
    let (path, open_row) = open_path(&function_instrs(pe, check, end)).ok_or(ScanError::NotFound)?;
    Ok(MenuLayout { view, function, rows, margin, path, open_row, submenu })
}

pub fn find_menu(pe: &Pe) -> Result<Menu, ScanError> {
    let (cancel, priority, kind) = ranker(pe)?;
    let entry = entry_constructor(pe, priority)?;
    let (entry_size, inner) = entry_size(pe, entry.function)?;
    let outer = one(calls_to(pe, inner).into_iter().filter_map(|site| pe.function_containing(site).map(|(b, _)| b)).collect())?;
    let object = one(stored_after_call(pe, outer).into_iter().map(|(_, d)| d).collect())?;
    Ok(Menu {
        object,
        entry_vtable: entry.vtable,
        entry_size,
        target: entry.strings[0],
        option: entry.strings[1],
        ids: entry.ids,
        priority,
        kind,
        cancel,
    })
}
