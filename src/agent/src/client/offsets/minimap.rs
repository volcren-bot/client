use super::{model::whole_function, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Minimap {
    pub object: u32,
    pub zoom: u32,
    pub angle: u32,
    pub focal_x: u32,
    pub focal_y: u32,
}

const CONTENT_TYPE: u64 = 1338;
const SCENE_TILES: u64 = 104;

fn content_type_calls(pe: &Pe) -> Vec<(u32, u32)> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for hit in dword_hits(pe, CONTENT_TYPE as u32) {
        let Some((begin, _)) = pe.function_containing(hit) else { continue };
        if !seen.insert(begin) {
            continue;
        }
        let instrs = whole_function(pe, begin, 0x40000);
        for (at, _) in instrs.iter().enumerate().filter(|(_, i)| {
            i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 4
                && i.op1_kind() == OpKind::Immediate32
                && i.immediate(1) == CONTENT_TYPE
        }) {
            let window = &instrs[at + 1..(at + 13).min(instrs.len())];
            let Some(load) = window.iter().position(|i| {
                i.mnemonic() == Mnemonic::Mov
                    && i.op0_register() == Register::RCX
                    && i.op1_kind() == OpKind::Memory
                    && i.memory_base() != Register::RIP
                    && i.memory_index() == Register::None
            }) else {
                continue;
            };
            let object = window[load].memory_displacement64() as u32;
            if let Some(call) = window[load..].iter().find(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64) {
                found.push((object, call.near_branch_target() as u32));
            }
        }
    }
    found
}

fn zoom_reads(instrs: &[Instruction]) -> Vec<u32> {
    let mut found = Vec::new();
    for (at, i) in instrs.iter().enumerate() {
        if !(i.mnemonic() == Mnemonic::Imul && i.op_count() == 3 && i.immediate(2) == SCENE_TILES) {
            continue;
        }
        if i.op1_kind() == OpKind::Memory {
            if i.memory_base() != Register::RIP && i.memory_index() == Register::None {
                found.push(i.memory_displacement64() as u32);
            }
            continue;
        }
        let register = i.op1_register();
        let written = instrs[..at].iter().rev().find(|p| p.op_count() > 0 && p.op0_kind() == OpKind::Register && p.op0_register().full_register() == register.full_register());
        if let Some(load) = written.filter(|p| {
            p.mnemonic() == Mnemonic::Mov
                && p.op0_register() == register
                && p.op1_kind() == OpKind::Memory
                && p.memory_base() != Register::RIP
                && p.memory_index() == Register::None
        }) {
            found.push(load.memory_displacement64() as u32);
        }
    }
    found
}

pub(super) fn client_registers(instrs: &[Instruction], client: u32) -> HashSet<Register> {
    instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.is_ip_rel_memory_operand() && i.ip_rel_memory_address() as u32 == client)
        .map(|i| i.op0_register())
        .collect()
}

pub(super) fn client_field(i: &Instruction, clients: &HashSet<Register>) -> Option<u32> {
    (i.op1_kind() == OpKind::Memory && clients.contains(&i.memory_base()) && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

fn angle_reads(instrs: &[Instruction], client: u32) -> Vec<u32> {
    let clients = client_registers(instrs, client);
    let mut found = Vec::new();
    for (at, i) in instrs.iter().enumerate() {
        let Some(field) = client_field(i, &clients).filter(|_| i.mnemonic() == Mnemonic::Movsxd) else { continue };
        let index = i.op0_register();
        if instrs[at + 1..].iter().take(3).any(|t| t.op1_kind() == OpKind::Memory && t.memory_index() == index && t.memory_index_scale() == 4) {
            found.push(field);
        }
    }
    found
}

fn focal_reads(instrs: &[Instruction], client: u32) -> Vec<u32> {
    let clients = client_registers(instrs, client);
    let floats: HashSet<u32> = instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Movd && i.op0_register().is_xmm())
        .filter_map(|i| client_field(i, &clients))
        .collect();
    floats.iter().copied().filter(|d| floats.contains(&(d + 8))).collect()
}

pub fn find_minimap(pe: &Pe, client: u32) -> Result<Minimap, ScanError> {
    let calls = content_type_calls(pe);
    let object = one(calls.iter().map(|&(object, _)| object).collect())?;
    let targets: HashSet<u32> = calls.iter().map(|&(_, target)| target).collect();
    let functions: Vec<Vec<Instruction>> = targets.iter().map(|&target| whole_function(pe, target, 0x4000)).collect();
    let draw = one(functions.iter().enumerate().filter(|(_, f)| !zoom_reads(f).is_empty()).map(|(at, _)| at).collect())?;
    let zoom = one(zoom_reads(&functions[draw]))?;
    let focal_x = one(focal_reads(&functions[draw], client))?;
    let angle = one(functions.iter().flat_map(|f| angle_reads(f, client)).collect())?;
    let clients = client_registers(&functions[draw], client);
    if !functions[draw].iter().any(|i| client_field(i, &clients) == Some(angle)) {
        return Err(ScanError::NotFound);
    }
    Ok(Minimap { object, zoom, angle, focal_x, focal_y: focal_x + 8 })
}

fn destination_compares(instrs: &[Instruction], object: u32) -> Vec<u32> {
    let head = &instrs[..instrs.len().min(16)];
    let Some(minimap) = head
        .iter()
        .find(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_kind() == OpKind::Memory
                && i.memory_base() == Register::RCX
                && i.memory_index() == Register::None
                && i.memory_displacement64() == object as u64
        })
        .map(|i| i.op0_register())
    else {
        return Vec::new();
    };
    let mut ys = HashSet::from([Register::R9D]);
    ys.extend(
        head.iter()
            .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op1_register() == Register::R9D)
            .map(|i| i.op0_register()),
    );
    let compare = |i: &Instruction| {
        (i.mnemonic() == Mnemonic::Cmp
            && i.op0_kind() == OpKind::Register
            && i.op1_kind() == OpKind::Memory
            && i.memory_base() == minimap
            && i.memory_index() == Register::None)
            .then(|| (i.op0_register(), i.memory_displacement64() as u32))
    };
    let compares: Vec<(Register, u32)> = head.iter().filter_map(compare).collect();
    compares
        .iter()
        .filter(|&&(register, _)| register == Register::R8D)
        .map(|&(_, x)| x)
        .filter(|&x| compares.iter().any(|&(register, y)| ys.contains(&register) && y == x + 4))
        .collect()
}

pub fn find_destination(pe: &Pe, object: u32) -> Result<u32, ScanError> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for hit in dword_hits(pe, object) {
        let Some((begin, _)) = pe.function_containing(hit) else { continue };
        if seen.insert(begin) {
            found.extend(destination_compares(&whole_function(pe, begin, 0x400), object));
        }
    }
    one(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_destination_setter() {
        // Client::setDestination on build a2636a01 (0x14006a2f0)
        let code = [
            0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x30, 0x48, 0x8b, 0x81, 0xb0, 0xea, 0x40, 0x00,
            0x41, 0x8b, 0xd9, 0x41, 0x8b, 0xf8, 0x48, 0x8b, 0xf2, 0x44, 0x3b, 0x00, 0x75, 0x05, 0x3b, 0x58, 0x04, 0x74, 0x60, 0x89, 0x38, 0x48,
            0x8b, 0x81, 0xb0, 0xea, 0x40, 0x00, 0x89, 0x58, 0x04,
        ];
        let instrs: Vec<Instruction> = Decoder::with_ip(64, &code, 0x1000, DecoderOptions::NONE).into_iter().collect();
        assert_eq!(destination_compares(&instrs, 0x40eab0), vec![0]);
        assert_eq!(destination_compares(&instrs, 0x40eab8), Vec::<u32>::new());
    }
}
