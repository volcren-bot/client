use super::items::cache_reader;
use super::model::field_load;
use super::*;

const NPC_GROUP: u64 = 9;
const SEQ_GROUP: u64 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeModels {
    pub npc_lookup: u32,
    pub seq_lookup: u32,
    pub npc_get_model: u32,
    pub seq_skeletal: u32,
    pub seq_frame_count: u32,
    pub seq_frames: u32,
}

fn group_lookup(pe: &Pe, reader: u32, group: u64) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for Caller {
        begin, instrs, at, ..
    } in callers(pe, reader)
    {
        let loads_group = instrs[at.saturating_sub(8)..at].iter().any(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_register() == Register::R8D
                && i.op1_kind() == OpKind::Immediate32
                && i.immediate(1) == group
        });
        if loads_group {
            found.extend(chunk_root(pe, begin));
        }
    }
    one(found)
}

fn calls_itself(pe: &Pe, function: u32) -> bool {
    whole_function(pe, function, 0x4000)
        .iter()
        .any(|i| call_target(i) == Some(function))
}

fn npc_type_get_model(pe: &Pe, get_model: u32, npc_definition: u32) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if chunk_root(pe, begin) != Some(get_model) {
            continue;
        }
        let instrs = function_instrs(pe, begin, end);
        for (at, i) in instrs.iter().enumerate() {
            if field_load(i, npc_definition) != Some(Register::RCX) {
                continue;
            }
            let Some(target) = instrs[at + 1..]
                .iter()
                .take(8)
                .find(|i| i.mnemonic() == Mnemonic::Call)
                .and_then(call_target)
            else {
                continue;
            };
            if calls_itself(pe, target) {
                found.push(target);
            }
        }
    }
    one(found)
}

fn seq_frames(pe: &Pe) -> Result<(u32, u32, u32), ScanError> {
    let mut checks = Vec::new();
    for (rva, data) in pe.code_sections() {
        for (offset, w) in data.windows(9).enumerate() {
            if w[0] == 0x8b && w[1] == 0x41 && w[3..] == [0xc1, 0xe8, 0x1f, 0x34, 0x01, 0xc3] {
                checks.push((rva + offset) as u32);
            }
        }
    }
    let skeletal = one(checks)?;
    let (mut counts, mut arrays) = (Vec::new(), Vec::new());
    for site in calls_to(pe, skeletal) {
        let instrs = decode_at(pe, site, 4);

        let [_, test, jne, read] = instrs.as_slice() else {
            continue;
        };
        if test.mnemonic() != Mnemonic::Test
            || test.op0_register() != Register::AL
            || jne.mnemonic() != Mnemonic::Jne
            || read.mnemonic() != Mnemonic::Mov
        {
            continue;
        }
        let Some(field) = load_from(read, Register::RCX) else {
            continue;
        };
        match read.op0_register().size() {
            4 => counts.push(field),
            8 => arrays.push(field),
            _ => {}
        }
    }
    Ok((skeletal, one(counts)?, one(arrays)?))
}

pub fn find_type_models(
    pe: &Pe,
    obj_lookup: u32,
    get_model: u32,
    npc_definition: u32,
) -> Result<TypeModels, ScanError> {
    let reader = cache_reader(pe, obj_lookup).ok_or(ScanError::NotFound)?;
    let (seq_skeletal, seq_frame_count, seq_frames) = seq_frames(pe)?;
    Ok(TypeModels {
        npc_lookup: group_lookup(pe, reader, NPC_GROUP)?,
        seq_lookup: group_lookup(pe, reader, SEQ_GROUP)?,
        npc_get_model: npc_type_get_model(pe, get_model, npc_definition)?,
        seq_skeletal,
        seq_frame_count,
        seq_frames,
    })
}
