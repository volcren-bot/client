use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VarcSetters {
    pub get_int: u32,
    pub set_int: u32,
    pub set_object: u32,
}

pub const VARIANT_KIND: u32 = 0x20;
pub const VARIANT_STRING: u8 = 2;

pub fn find_varc_setters(pe: &Pe) -> Result<VarcSetters, ScanError> {
    let binding = decode_at(pe, lua_binding(pe, "getIntegerVarc")?, 3);
    let [_, _, jmp, ..] = binding.as_slice() else {
        return Err(ScanError::NotFound);
    };
    if jmp.mnemonic() != Mnemonic::Jmp || jmp.op0_kind() != OpKind::NearBranch64 {
        return Err(ScanError::NotFound);
    }
    let get_int = jmp.near_branch_target() as u32;
    let (begin, end) = pe.function_containing(get_int).ok_or(ScanError::NotFound)?;
    let lookup = whole_function(pe, begin, end - begin);

    let search = lookup
        .iter()
        .find_map(call_target)
        .ok_or(ScanError::NotFound)?;
    let fields: Vec<u32> = lookup
        .iter()
        .take(12)
        .filter_map(|i| load_from(i, Register::RCX))
        .collect();
    if fields.len() < 2 {
        return Err(ScanError::NotFound);
    }

    let (mut ints, mut longs, mut objects) = (Vec::new(), Vec::new(), Vec::new());
    for &(begin, end) in &pe.functions {
        if !(0x80..=0x130).contains(&(end - begin)) {
            continue;
        }
        let instrs = whole_function(pe, begin, end - begin);
        let calls_search = instrs.iter().any(|i| call_target(i) == Some(search));
        let uses_fields = fields.iter().all(|&field| {
            instrs.iter().any(|i| {
                i.memory_displacement64() as u32 == field && i.memory_base() != Register::None
            })
        });
        let marks_changed = instrs.iter().any(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_size() == iced_x86::MemorySize::UInt8
                && immediate(i) == Some(1)
        });
        if !calls_search || !uses_fields || !marks_changed {
            continue;
        }
        let kind = instrs.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::R11L)
                .then(|| immediate(i))
                .flatten()
        });
        match kind {
            Some(1) => ints.push(begin),
            Some(5) => longs.push(begin),
            _ => objects.push(begin),
        }
    }
    let (set_int, _long, set_object) = (one(ints)?, one(longs)?, one(objects)?);
    Ok(VarcSetters {
        get_int,
        set_int,
        set_object,
    })
}
