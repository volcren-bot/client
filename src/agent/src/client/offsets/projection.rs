use super::{items::usertype_member, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Projection {
    pub world_to_screen: u32,
    pub tile_height: u32,
    pub top_world_view: u32,
    pub plane: u32,
}

fn reads_client(pe: &Pe, function: u32, client: u32) -> bool {
    decode_at(pe, function, 12)
        .iter()
        .any(|i| i.mnemonic() == Mnemonic::Mov && i.is_ip_rel_memory_operand() && i.ip_rel_memory_address() as u32 == client)
}

pub fn find_projection(image: &[u8], client: u32) -> Result<Projection, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let world_to_screen = lua_binding(&pe, "worldToScreenCoord")?;
    let fine = usertype_member(&pe, "osrs.MapCoord", "getFineCoordinate")?;
    if !reads_client(&pe, world_to_screen, client) || !reads_client(&pe, fine, client) {
        return Err(ScanError::NotFound);
    }
    let instrs = decode_at(&pe, fine, 40);
    let top_world_view = instrs
        .iter()
        .find_map(|i| load_from(i, Register::RAX).filter(|_| i.op0_register() == Register::RDX))
        .ok_or(ScanError::NotFound)?;
    let plane = instrs
        .iter()
        .find_map(|i| load_from(i, Register::RDX).filter(|_| i.op0_register() == Register::EAX))
        .ok_or(ScanError::NotFound)?;
    let tile_height = instrs
        .iter()
        .find(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)
        .map(|i| i.near_branch_target() as u32)
        .ok_or(ScanError::NotFound)?;
    Ok(Projection { world_to_screen, tile_height, top_world_view, plane })
}
