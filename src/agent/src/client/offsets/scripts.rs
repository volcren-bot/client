use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentInvalidate {
    pub function: u32,
    pub holder_component: u32,
    pub text: u32,
}

const SET_COLOUR: u32 = 1;
const SET_GRAPHIC: u32 = 5;
const SET_TEXT: u32 = 12;
const SET_SCROLL_SIZE: u32 = 20;
const SETTERS: u32 = 1100;

fn setter_handler(pe: &Pe) -> Result<u32, ScanError> {
    range_handler(pe, 1200)
}

pub(super) fn range_handler(pe: &Pe, upper: u32) -> Result<u32, ScanError> {
    let ranges = [1000, 1100, 1200, 1300, 1400, 1500];
    let compares = |i: &Instruction| {
        (i.mnemonic() == Mnemonic::Cmp
            && i.op0_kind() == OpKind::Register
            && i.op0_register().size() == 4
            && i.op1_kind() == OpKind::Immediate32)
            .then(|| i.immediate(1) as u32)
    };
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let code = pe.bytes(begin, end);
        if !ranges.iter().all(|&r| has_dword(code, r)) {
            continue;
        }
        let instrs = decode(code, begin);
        let values: HashSet<u32> = instrs.iter().filter_map(compares).collect();
        if !ranges.iter().all(|r| values.contains(r)) {
            continue;
        }
        let Some(at) = instrs.iter().position(|i| compares(i) == Some(upper)) else {
            continue;
        };
        let branches = |i: &&Instruction| {
            matches!(i.mnemonic(), Mnemonic::Call | Mnemonic::Jmp)
                && i.op0_kind() == OpKind::NearBranch64
        };
        if let Some(branch) = instrs[at..].iter().take(6).find(branches) {
            found.push(branch.near_branch_target() as u32);
        }
    }
    one(found)
}

fn setter_case(pe: &Pe, handler: u32, case: u32) -> Result<u32, ScanError> {
    switch_cases(pe, handler, SETTERS)?
        .get(case as usize)
        .copied()
        .ok_or(ScanError::NotFound)
}

pub(super) fn switch_cases(pe: &Pe, handler: u32, base: u32) -> Result<Vec<u32>, ScanError> {
    let instrs = decode_at(pe, handler, 120);
    let at = instrs
        .iter()
        .position(|i| {
            i.mnemonic() == Mnemonic::Add
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 4
                && i.op1_kind() == OpKind::Immediate32
                && i.immediate(1) as u32 == base.wrapping_neg()
        })
        .ok_or(ScanError::NotFound)?;
    let index = instrs[at].op0_register();
    let cases = instrs[at + 1..]
        .iter()
        .find_map(|i| {
            (i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && i.op0_register() == index
                && matches!(i.op1_kind(), OpKind::Immediate32 | OpKind::Immediate8to32))
            .then(|| i.immediate(1) as u32)
        })
        .ok_or(ScanError::NotFound)?;
    let table = instrs[at + 1..]
        .iter()
        .take(12)
        .find_map(|i| {
            (i.mnemonic() == Mnemonic::Mov
                && i.op0_register().size() == 4
                && i.op1_kind() == OpKind::Memory
                && i.memory_index() != Register::None
                && i.memory_index_scale() == 4)
                .then(|| i.memory_displacement64() as u32)
        })
        .ok_or(ScanError::NotFound)?;
    (0..=cases)
        .map(|case| u32_at(pe.image, (table + case * 4) as usize).ok_or(ScanError::NotFound))
        .collect()
}

fn invalidate_at(pe: &Pe, rva: u32) -> Option<(u32, u32)> {
    let first = decode_at(pe, rva, 1);
    let load = first.first()?;
    let holder = load_from(load, Register::RDX).filter(|_| load.op0_register() == Register::R8)?;
    Some((rva, holder))
}

pub fn find_component_invalidate(pe: &Pe) -> Result<ComponentInvalidate, ScanError> {
    let case = setter_case(pe, setter_handler(pe)?, SET_TEXT)?;
    let instrs = decode_at(pe, case, 250);

    let mut adds: HashMap<u32, usize> = HashMap::new();
    for i in &instrs {
        if i.mnemonic() == Mnemonic::Add
            && i.op0_kind() == OpKind::Register
            && i.op0_register().size() == 8
            && matches!(i.op1_kind(), OpKind::Immediate32to64)
        {
            *adds.entry(i.immediate(1) as u32).or_default() += 1;
        }
    }
    let text = one(adds
        .into_iter()
        .filter(|&(_, n)| n >= 2)
        .map(|(add, _)| add)
        .collect())?;
    let after = instrs
        .iter()
        .position(|i| {
            i.mnemonic() == Mnemonic::Add
                && i.op0_register().size() == 8
                && i.op1_kind() == OpKind::Immediate32to64
                && i.immediate(1) as u32 == text
        })
        .ok_or(ScanError::NotFound)?;
    let found = instrs[after..].windows(3).find_map(|w| {
        let [a, b, call] = w else { return None };
        let moves = |i: &Instruction| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op1_kind() == OpKind::Register
                && i.op0_register().size() == 8
        };
        let args = moves(a)
            && moves(b)
            && [a.op0_register(), b.op0_register()].contains(&Register::RCX)
            && [a.op0_register(), b.op0_register()].contains(&Register::RDX);
        call_target(call)
            .filter(|_| args)
            .and_then(|target| invalidate_at(pe, target))
    });
    let (function, holder_component) = found.ok_or(ScanError::NotFound)?;
    Ok(ComponentInvalidate {
        function,
        holder_component,
        text,
    })
}

pub fn find_component_sprite(pe: &Pe) -> Result<u32, ScanError> {
    let case = setter_case(pe, setter_handler(pe)?, SET_GRAPHIC)?;
    popped_stores(&decode_at(pe, case, 8))
        .next()
        .ok_or(ScanError::NotFound)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComponentFields {
    pub hidden: u32,
    pub text_color: u32,
    pub scroll_width: u32,
    pub index: u32,
    pub children: u32,
}

fn hidden(pe: &Pe) -> Result<u32, ScanError> {
    let instrs = decode_at(pe, range_handler(pe, 1100)?, 400);
    one(instrs
        .windows(4)
        .filter_map(|w| {
            let field = field_operand(&w[0], 0)?;
            let compares = w[0].mnemonic() == Mnemonic::Cmp
                && w[0].memory_size() == iced_x86::MemorySize::UInt8
                && w[0].op1_kind() == OpKind::Register;
            let stores = w[1..].iter().any(|i| {
                i.mnemonic() == Mnemonic::Mov
                    && field_operand(i, 0) == Some(field)
                    && i.op1_kind() == OpKind::Register
                    && i.op1_register() == w[0].op1_register()
            });
            (compares && stores).then_some(field)
        })
        .collect())
}

fn popped_stores(instrs: &[Instruction]) -> impl Iterator<Item = u32> + '_ {
    instrs
        .windows(2)
        .take_while(|w| w[0].mnemonic() != Mnemonic::Jmp)
        .filter_map(|w| {
            let popped = w[0].mnemonic() == Mnemonic::Mov
                && w[0].op0_kind() == OpKind::Register
                && w[0].op0_register().size() == 4
                && w[0].op1_kind() == OpKind::Memory
                && w[0].memory_index() != Register::None
                && w[0].memory_index_scale() == 4;
            (popped
                && w[1].mnemonic() == Mnemonic::Mov
                && w[1].op1_kind() == OpKind::Register
                && w[1].op1_register() == w[0].op0_register())
            .then(|| field_operand(&w[1], 0))
            .flatten()
        })
}

fn children(pe: &Pe) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs = decode(pe.bytes(begin, end), begin);
        for (at, cmp) in instrs.iter().enumerate() {
            let Some(count) = field_operand(cmp, 0) else {
                continue;
            };
            if cmp.mnemonic() != Mnemonic::Cmp
                || cmp.memory_size() != iced_x86::MemorySize::UInt64
                || cmp.op1_kind() != OpKind::Register
            {
                continue;
            }
            let base = cmp.memory_base();
            let rest = &instrs[at + 1..(at + 5).min(instrs.len())];
            let shifts = rest.iter().any(|i| {
                i.mnemonic() == Mnemonic::Shl
                    && i.op1_kind() == OpKind::Immediate8
                    && i.immediate(1) == 4
            });
            let adds = rest.iter().any(|i| {
                i.mnemonic() == Mnemonic::Add
                    && i.op1_kind() == OpKind::Memory
                    && i.memory_base() == base
                    && i.memory_displacement64() as u32 == count + 8
            });
            let checks_sign = instrs[at.saturating_sub(4)..at]
                .iter()
                .any(|i| i.mnemonic() == Mnemonic::Test);
            if shifts && adds && checks_sign && count > 0x100 {
                found.push(count);
            }
        }
    }
    one(found)
}

pub fn find_component_fields(pe: &Pe) -> Result<ComponentFields, ScanError> {
    let handler = setter_handler(pe)?;
    let colour = decode_at(pe, setter_case(pe, handler, SET_COLOUR)?, 8);
    let text_color = one(popped_stores(&colour).collect())?;

    let scroll = decode_at(pe, setter_case(pe, handler, SET_SCROLL_SIZE)?, 30);
    let mut stores = popped_stores(&scroll);
    let (Some(scroll_width), Some(scroll_height)) = (stores.next(), stores.next()) else {
        return Err(ScanError::NotFound);
    };
    if scroll_height != scroll_width + 4 {
        return Err(ScanError::NotFound);
    }
    let index = one(scroll
        .windows(3)
        .filter_map(|w| {
            let field = field_operand(&w[0], 0)?;
            let reads_group = w[1..].iter().any(|i| {
                i.mnemonic() == Mnemonic::Movsx
                    && i.memory_size() == iced_x86::MemorySize::Int16
                    && i.memory_base() == w[0].memory_base()
            });
            (w[0].mnemonic() == Mnemonic::Cmp && w[0].op1_kind() == OpKind::Register && reads_group)
                .then_some(field)
        })
        .collect())?;

    Ok(ComponentFields {
        hidden: hidden(pe)?,
        text_color,
        scroll_width,
        index,
        children: children(pe)?,
    })
}
