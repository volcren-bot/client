//! The client's own definition lookups and model builder, called to build an NPC type's model posed at an animation frame.
use super::items::cache_reader;
use super::model::{chunk_root, field_load, whole_function};
use super::*;

const NPC_GROUP: u64 = 9;
const SEQ_GROUP: u64 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeModels {
    /// `(out: *mut [u64; 2], id)`: the NPC and animation definitions as {control, object} pairs (a bad id gives a default one)
    pub npc_lookup: u32,
    pub seq_lookup: u32,
    /// `NpcType::getModel(npc, out, seq, frame, seq2, frame2, overrides)`: the model posed at `frame` of `seq` (null for none)
    pub npc_get_model: u32,
    /// `(seq) -> bool`: whether an animation is skeletal; a frame-based one has `frame_count` frames in the array at `frames`
    pub seq_skeletal: u32,
    pub seq_frame_count: u32,
    pub seq_frames: u32,
}

fn decode_function(pe: &Pe, begin: u32, end: u32) -> Vec<Instruction> {
    Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE)
        .into_iter()
        .collect()
}

// The definition lookup of a config group: the function calling the cache reader right after `mov r8d, group`.
fn group_lookup(pe: &Pe, reader: u32, group: u64) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(site, target) in direct_calls(pe) {
        if target != reader {
            continue;
        }
        let Some((begin, end)) = pe.function_containing(site) else {
            continue;
        };
        let instrs = decode_function(pe, begin, end);
        let Some(at) = instrs.iter().position(|i| i.ip32() == site) else {
            continue;
        };
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
    whole_function(pe, function, 0x4000).iter().any(|i| {
        i.mnemonic() == Mnemonic::Call
            && i.op0_kind() == OpKind::NearBranch64
            && i.near_branch_target() as u32 == function
    })
}

// In the NPC actor's get-model: the call after it loads the actor's definition into rcx that calls itself (for the
// NPC's other forms), which is `NpcType::getModel`.
fn npc_type_get_model(pe: &Pe, get_model: u32, npc_definition: u32) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if chunk_root(pe, begin) != Some(get_model) {
            continue;
        }
        let instrs = decode_function(pe, begin, end);
        for (at, i) in instrs.iter().enumerate() {
            if field_load(i, npc_definition) != Some(Register::RCX) {
                continue;
            }
            let Some(call) = instrs[at + 1..]
                .iter()
                .take(8)
                .find(|i| i.mnemonic() == Mnemonic::Call)
            else {
                continue;
            };
            if call.op0_kind() != OpKind::NearBranch64 {
                continue;
            }
            let target = call.near_branch_target() as u32;
            if calls_itself(pe, target) {
                found.push(target);
            }
        }
    }
    one(found)
}

// The skeletal check (`mov eax, [rcx+X]; shr eax, 31; xor al, 1; ret`: the skeletal animation id isn't -1), then, after
// it says no in its callers, the fields they read: a dword (the frame count) or a qword (the frame array).
fn seq_frames(pe: &Pe) -> Result<(u32, u32, u32), ScanError> {
    let mut checks = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, w) in data.windows(9).enumerate() {
            if w[0] == 0x8b && w[1] == 0x41 && w[3..] == [0xc1, 0xe8, 0x1f, 0x34, 0x01, 0xc3] {
                checks.push((section.rva + offset) as u32);
            }
        }
    }
    let skeletal = one(checks)?;
    let (mut counts, mut arrays) = (Vec::new(), Vec::new());
    for &(site, target) in direct_calls(pe) {
        if target != skeletal {
            continue;
        }
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
