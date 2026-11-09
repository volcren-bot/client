use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub position: u32,
    pub pitch: u32,
    pub yaw: u32,
}

const CAM_FORCEANGLE: u32 = 5504;

fn position(pe: &Pe, client: u32) -> Result<u32, ScanError> {
    let instrs = decode_at(pe, lua_binding(pe, "worldToScreenCoord")?, 24);
    let clients = client_registers(&instrs, client);
    let fields: Vec<u32> = instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_register().size() == 4
                && clients.contains(&i.memory_base())
        })
        .map(|i| i.memory_displacement64() as u32)
        .collect();
    one(fields
        .windows(3)
        .filter(|w| w[1] == w[0] + 4 && w[2] == w[0] + 8)
        .map(|w| w[0])
        .collect())
}

fn angles(pe: &Pe, client: u32) -> Result<(u32, u32), ScanError> {
    let subtracts = |i: &Instruction, value: u32| {
        i.mnemonic() == Mnemonic::Sub && i.op0_kind() == OpKind::Register && imm32(i) == Some(value)
    };
    let is_je =
        |i: &Instruction| i.mnemonic() == Mnemonic::Je && i.op0_kind() == OpKind::NearBranch64;
    let mut found = Vec::new();
    for (begin, end) in functions_with_dword(pe, CAM_FORCEANGLE) {
        let instrs = function_instrs(pe, begin, end);
        let clients = client_registers(&instrs, client);
        for w in instrs.windows(6) {
            if !(subtracts(&w[0], CAM_FORCEANGLE)
                && is_je(&w[1])
                && subtracts(&w[2], 1)
                && is_je(&w[3])
                && subtracts(&w[4], 1)
                && is_je(&w[5]))
            {
                continue;
            }
            let read = |target: u64| {
                decode_at(pe, target as u32, 2)
                    .first()
                    .filter(|i| {
                        i.op1_kind() == OpKind::Memory && clients.contains(&i.memory_base())
                    })
                    .map(|i| i.memory_displacement64() as u32)
            };
            if let (Some(pitch), Some(yaw)) = (
                read(w[3].near_branch_target()),
                read(w[5].near_branch_target()),
            ) {
                found.push((pitch, yaw));
            }
        }
    }
    one(found)
}

pub fn find_camera(pe: &Pe, client: u32) -> Result<Camera, ScanError> {
    let (pitch, yaw) = angles(pe, client)?;
    Ok(Camera {
        position: position(pe, client)?,
        pitch,
        yaw,
    })
}
