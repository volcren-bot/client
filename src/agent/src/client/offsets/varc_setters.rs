use super::{model::whole_function, *};

/// The functions that write a client variable (varc), which the interpreter's `put_varc` opcodes call and nothing else does.
///
/// The client keeps its varcs in a map inside one object (`Varcs`). The Lua getter `getIntegerVarc` jumps to its int lookup, which
/// finds the entry with a shared map search; the setters are the other functions that use the same search and the same map
/// fields, and end by marking the varcs as changed (a byte store of 1). They differ in what they store:
///
/// - `set_int(varcs, index, value)` sets the entry's kind to 1 (`mov r11b, 1`);
/// - `set_long` does the same with kind 5 (not logged);
/// - `set_object(varcs, index, variant)` takes a pointer to a variant (what the interpreter's object stack holds): the kind is
///   whatever the variant says. The varc string opcodes use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VarcSetters {
    /// RVA of the int lookup the Lua getter jumps to: `get_int(varcs, index)`, -1 when the varc has no int
    pub get_int: u32,
    pub set_int: u32,
    pub set_object: u32,
}

/// A variant is `VARIANT_SIZE` bytes with its kind at `VARIANT_KIND` (2 for a string); a string is inline in the first 0x17 bytes
/// (the byte at 0x17 is the room left) or, when that byte's top bit is set, a pointer at 0 and a length at 8. No code shape names
/// them (the interpreter indexes the object stack with them); they were checked on the builds the rule was written for.
pub const VARIANT_KIND: u32 = 0x20;
pub const VARIANT_STRING: u8 = 2;

fn imm1(i: &Instruction) -> Option<u64> {
    matches!(i.op1_kind(), OpKind::Immediate8 | OpKind::Immediate8to32 | OpKind::Immediate8to64 | OpKind::Immediate32 | OpKind::Immediate32to64).then(|| i.immediate(1))
}

pub fn find_varc_setters(image: &[u8]) -> Result<VarcSetters, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;

    // the getter: `mov rax, [client]; mov rcx, [rax + varcs]; jmp lookup`
    let binding = decode_at(&pe, lua_binding(&pe, "getIntegerVarc")?, 3);
    let [_, _, jmp, ..] = binding.as_slice() else { return Err(ScanError::NotFound) };
    if jmp.mnemonic() != Mnemonic::Jmp || jmp.op0_kind() != OpKind::NearBranch64 {
        return Err(ScanError::NotFound);
    }
    let get_int = jmp.near_branch_target() as u32;
    let (begin, end) = pe.function_containing(get_int).ok_or(ScanError::NotFound)?;
    let lookup = whole_function(&pe, begin, end - begin);

    // the map search every varc function calls, and the two map fields it is given (loaded from `rcx`)
    let search = lookup.iter().find(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64).ok_or(ScanError::NotFound)?.near_branch_target() as u32;
    let fields: Vec<u32> = lookup.iter().take(12).filter_map(|i| load_from(i, Register::RCX)).collect();
    if fields.len() < 2 {
        return Err(ScanError::NotFound);
    }

    let (mut ints, mut longs, mut objects) = (Vec::new(), Vec::new(), Vec::new());
    for &(begin, end) in &pe.functions {
        if !(0x80..=0x130).contains(&(end - begin)) {
            continue;
        }
        let instrs = whole_function(&pe, begin, end - begin);
        let calls_search = instrs.iter().any(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64 && i.near_branch_target() as u32 == search);
        let uses_fields = fields.iter().all(|&field| instrs.iter().any(|i| i.memory_displacement64() as u32 == field && i.memory_base() != Register::None));
        // ends by marking the varcs as changed: `mov byte [varcs + n], 1`
        let marks_changed = instrs.iter().any(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt8 && imm1(i) == Some(1));
        if !calls_search || !uses_fields || !marks_changed {
            continue;
        }
        let kind = instrs.iter().find_map(|i| (i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::R11L).then(|| imm1(i)).flatten());
        match kind {
            Some(1) => ints.push(begin),
            Some(5) => longs.push(begin),
            _ => objects.push(begin),
        }
    }
    let (set_int, _long, set_object) = (one(ints)?, one(longs)?, one(objects)?);
    Ok(VarcSetters { get_int, set_int, set_object })
}
