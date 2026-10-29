use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjTypeLookup {
    pub function: u32,
    pub name: u32,
    pub desc: u32,
}

const STRING_SIZE: u32 = 0x18;

pub(super) fn string_rvas(pe: &Pe, name: &str) -> HashSet<u32> {
    let mut needle = vec![0u8];
    needle.extend_from_slice(name.as_bytes());
    needle.push(0);
    pe.image
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle.as_slice())
        .map(|(at, _)| at as u32 + 1)
        .collect()
}

pub(super) fn usertype_member(pe: &Pe, usertype: &str, member: &str) -> Result<u32, ScanError> {
    let usertypes = string_rvas(pe, usertype);
    let members = string_rvas(pe, member);
    let in_code = |rva: u32| pe.code.iter().any(|s| (s.rva..s.rva + s.size).contains(&(rva as usize)));
    let ip_lea = |i: &Instruction| (i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand()).then(|| i.ip_rel_memory_address() as u32);

    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        if !instrs.iter().filter_map(ip_lea).any(|rva| usertypes.contains(&rva)) {
            continue;
        }
        for (at, instr) in instrs.iter().enumerate() {
            if !ip_lea(instr).is_some_and(|rva| members.contains(&rva)) {
                continue;
            }
            let code = instrs[at.saturating_sub(8)..at].iter().rev().filter_map(ip_lea).find(|&rva| in_code(rva));
            found.extend(code);
        }
    }
    one(found)
}

fn lookup_in_wrapper(pe: &Pe, wrapper: u32) -> Option<(u32, u32)> {
    let instrs = decode_at(pe, wrapper, 30);
    let id = instrs.iter().position(|i| load_from(i, Register::RCX).is_some() && i.op0_register() == Register::EDX)?;
    let call = id + 1 + instrs[id + 1..].iter().position(|i| i.mnemonic() == Mnemonic::Call)?;
    if instrs[call].op0_kind() != OpKind::NearBranch64 {
        return None;
    }
    let function = instrs[call].near_branch_target() as u32;
    let desc = instrs[call + 1..].iter().take(4).find_map(|i| {
        (i.mnemonic() == Mnemonic::Add && i.op0_register() == Register::RDX && matches!(i.op1_kind(), OpKind::Immediate8to64 | OpKind::Immediate32to64))
            .then(|| i.immediate(1) as u32)
    })?;
    Some((function, desc))
}

fn string_fields_assigned(pe: &Pe, function: u32) -> Option<(HashSet<u32>, bool)> {
    let mut instrs = decode_at(pe, function, 3000);
    let padding = instrs.windows(2).position(|w| w.iter().all(|i| i.mnemonic() == Mnemonic::Int3))?;
    instrs.truncate(padding);
    let mut fields = HashSet::new();
    let mut recursive = false;
    for w in instrs.windows(2) {
        let [lea, call] = w else { continue };
        if call.mnemonic() == Mnemonic::Call && call.op0_kind() == OpKind::NearBranch64 && call.near_branch_target() as u32 == function {
            recursive = true;
        }
        let field = lea.mnemonic() == Mnemonic::Lea
            && lea.op0_register() == Register::RCX
            && lea.memory_base().size() == 8
            && lea.memory_base() != Register::RIP
            && lea.memory_index() == Register::None;
        if field && call.mnemonic() == Mnemonic::Call {
            fields.insert(lea.memory_displacement64() as u32);
        }
    }
    Some((fields, recursive))
}

pub fn find_obj_type_lookup(pe: &Pe) -> Result<ObjTypeLookup, ScanError> {
    let wrapper = usertype_member(pe, "osrs.ObjType", "getDesc")?;
    let (function, desc) = lookup_in_wrapper(pe, wrapper).ok_or(ScanError::NotFound)?;
    let (fields, recursive) = string_fields_assigned(pe, function).ok_or(ScanError::NotFound)?;
    let name = desc.checked_sub(STRING_SIZE).ok_or(ScanError::NotFound)?;
    if !recursive || !fields.contains(&name) {
        return Err(ScanError::NotFound);
    }
    Ok(ObjTypeLookup { function, name, desc })
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameVals {
    pub index: u32,
    pub reader: u32,
    pub table: CacheTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTable {
    pub archive_count: u32,
    pub archives: u32,
    pub archive_stride: u32,
    pub archive_missing: u32,
    pub file_count: u32,
    pub files: u32,
    pub file_stride: u32,
    pub file_missing: u32,
}

const OBJ_ARCHIVE: u64 = 10;

fn cache_reader(pe: &Pe, lookup: u32) -> Option<u32> {
    let instrs = decode_at(pe, lookup, 120);
    let at = instrs.iter().position(|i| {
        i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::R8D && i.op1_kind() == OpKind::Immediate32 && i.immediate(1) == OBJ_ARCHIVE
    })?;
    let call = instrs[at..].iter().take(8).find(|i| i.mnemonic() == Mnemonic::Call)?;
    (call.op0_kind() == OpKind::NearBranch64).then(|| call.near_branch_target() as u32)
}

fn gamevals_index(pe: &Pe, client: u32) -> Result<u32, ScanError> {
    let names = string_rvas(pe, "gamevals");
    let loads_names = |target: u32| {
        decode_at(pe, target, 80)
            .iter()
            .any(|i| i.mnemonic() == Mnemonic::Lea && i.is_ip_rel_memory_operand() && names.contains(&(i.ip_rel_memory_address() as u32)))
    };
    let mut found = Vec::new();
    for (site, target) in direct_jumps(pe) {
        let instrs = decode_at(pe, site.saturating_sub(14), 3);
        let [global, field, jump] = instrs.as_slice() else { continue };
        let loads_client = global.mnemonic() == Mnemonic::Mov
            && global.op0_register() == Register::RDX
            && global.is_ip_rel_memory_operand()
            && global.ip_rel_memory_address() as u32 == client;
        let offset = load_from(field, Register::RDX).filter(|_| field.op0_register() == Register::RDX);
        if loads_client && jump.ip32() == site && loads_names(target) {
            found.extend(offset);
        }
    }
    one(found)
}

fn direct_jumps(pe: &Pe) -> Vec<(u32, u32)> {
    let mut jumps = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(5).enumerate() {
            if window[0] != 0xe9 {
                continue;
            }
            let site = (section.rva + offset) as i64;
            let target = site + 5 + i32::from_le_bytes(window[1..5].try_into().unwrap()) as i64;
            jumps.push((site as u32, target as u32));
        }
    }
    jumps
}

fn memory_field(i: &Instruction, base: Register) -> Option<u32> {
    (i.op1_kind() == OpKind::Memory && i.memory_base() == base && i.memory_index() == Register::None).then(|| i.memory_displacement64() as u32)
}

fn cache_table(pe: &Pe, reader: u32) -> Option<CacheTable> {
    let instrs = decode_at(pe, reader, 60);
    let mut at = instrs.iter();
    let count = at.find(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Register).and_then(|i| memory_field(i, Register::RCX))?;
    let imul = at.find(|i| i.mnemonic() == Mnemonic::Imul && i.op_count() == 3)?;
    let archive = imul.op0_register();
    let add = at.find(|i| i.mnemonic() == Mnemonic::Add && i.op0_register() == archive)?;
    let archives = memory_field(add, Register::RCX)?;
    let minus_one = |i: &&Instruction, base: Register| {
        i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_base() == base && i.immediate(1) as i64 == -1
    };
    let archive_missing = at.find(|i| minus_one(i, archive))?.memory_displacement64() as u32;
    let file_count = at.find(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Register).and_then(|i| memory_field(i, archive))?;
    let shl = at.find(|i| i.mnemonic() == Mnemonic::Shl && i.op1_kind() == OpKind::Immediate8)?;
    let file = shl.op0_register();
    let add = at.find(|i| i.mnemonic() == Mnemonic::Add && i.op0_register() == file)?;
    let files = memory_field(add, archive)?;
    let file_missing = at.find(|i| minus_one(i, file))?.memory_displacement64() as u32;
    Some(CacheTable {
        archive_count: count,
        archives,
        archive_stride: imul.immediate(2) as u32,
        archive_missing,
        file_count,
        files,
        file_stride: 1 << shl.immediate(1),
        file_missing,
    })
}

pub fn find_game_vals(pe: &Pe, client: u32, lookup: &ObjTypeLookup) -> Result<GameVals, ScanError> {
    let reader = cache_reader(pe, lookup.function).ok_or(ScanError::NotFound)?;
    let index = gamevals_index(pe, client)?;
    let table = cache_table(pe, reader).ok_or(ScanError::NotFound)?;
    Ok(GameVals { index, reader, table })
}

