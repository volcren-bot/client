use super::{
    minimap::{client_field, client_registers},
    model::whole_function,
    projection::Projection,
    scene::Scene,
    *,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalProjection {
    pub zoom: u32,
    pub view_holder: u32,
    pub view: u32,
    pub view_center: u32,
    pub view_offset_x: u32,
    pub view_offset_y: u32,
    pub trig: u32,
    pub canvas_size: u32,
    pub ui_size: u32,
    pub heights: u32,
    pub settings: u32,
    pub tile_x: u32,
    pub tile_model: u32,
    pub tile_paint: u32,
    pub model_vertices: u32,
    pub model_triangles: u32,
    pub decoration_height: u32,
}

fn calls(instrs: &[Instruction]) -> impl Iterator<Item = (usize, u32)> + '_ {
    instrs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)
        .map(|(n, i)| (n, i.near_branch_target() as u32))
}

fn plain_memory(i: &Instruction) -> bool {
    i.op1_kind() == OpKind::Memory && i.memory_index() == Register::None && !matches!(i.memory_base(), Register::RIP | Register::RSP)
}

fn zoom(instrs: &[Instruction], client: u32) -> Result<u32, ScanError> {
    let clients = client_registers(instrs, client);
    let found: HashSet<u32> = instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_register() == Register::EDX)
        .filter_map(|i| client_field(i, &clients))
        .collect();
    one(found.into_iter().collect())
}

fn view_holder(pe: &Pe, instrs: &[Instruction], canvas: u32) -> Result<u32, ScanError> {
    let found: HashSet<u32> = calls(instrs)
        .filter_map(|(_, target)| match decode_at(pe, target, 3).as_slice() {
            [load, add, ret]
                if load_from(load, Register::RCX) == Some(canvas)
                    && load.op0_register() == Register::RAX
                    && add.mnemonic() == Mnemonic::Add
                    && add.op0_register() == Register::RAX
                    && matches!(add.op1_kind(), OpKind::Immediate32to64 | OpKind::Immediate8to64)
                    && ret.mnemonic() == Mnemonic::Ret =>
            {
                Some(add.immediate(1) as u32)
            }
            _ => None,
        })
        .collect();
    one(found.into_iter().collect())
}

fn view_slot(pe: &Pe, holder: u32) -> Result<u32, ScanError> {
    let mut found = HashSet::new();
    for hit in dword_hits(pe, holder) {
        for back in [3, 4] {
            let instrs = decode_at(pe, hit - back, 4);
            let Some(lea) = instrs.first() else { continue };
            if lea.mnemonic() != Mnemonic::Lea || lea.op0_register() != Register::RCX || lea.memory_displacement64() != holder as u64 || lea.next_ip() != hit as u64 + 4 {
                continue;
            }
            let base = lea.memory_base();
            let passes_view = instrs[1..].iter().any(|i| i.mnemonic() == Mnemonic::Lea && i.op0_register() == Register::RDX && i.memory_base() == base);
            let Some((_, constructor)) = calls(&instrs[1..]).next().filter(|_| passes_view) else { continue };
            let body = whole_function(pe, constructor, 0x100);
            let Some(kept) = body.iter().find(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op1_register() == Register::RDX).map(|i| i.op0_register())
            else {
                continue;
            };
            let stores: HashSet<u32> = body
                .iter()
                .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.op1_kind() == OpKind::Register && i.op1_register() == kept)
                .map(|i| i.memory_displacement64() as u32)
                .collect();
            // other constructors called the same way spread their argument over several fields
            if let [slot] = stores.into_iter().collect::<Vec<_>>()[..] {
                found.insert(slot);
            }
        }
    }
    one(found.into_iter().collect())
}

struct Projector {
    trig: u32,
    view_center: u32,
    view_offset_x: u32,
    view_offset_y: u32,
}

fn projector(pe: &Pe, function: u32) -> Option<Projector> {
    let instrs = whole_function(pe, function, 0x200);
    let mut trig: Vec<u32> = instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Movss && i.op1_kind() == OpKind::Memory && i.memory_base() == Register::RCX && i.memory_index() == Register::None)
        .map(|i| i.memory_displacement64() as u32)
        .collect();
    trig.sort_unstable();
    trig.dedup();
    if trig.len() != 4 || trig.windows(2).any(|w| w[1] != w[0] + 4) {
        return None;
    }
    let adds = |register: Register| -> Vec<u32> {
        instrs
            .iter()
            .filter(|i| i.mnemonic() == Mnemonic::Add && i.op0_register() == register && plain_memory(i))
            .map(|i| i.memory_displacement64() as u32)
            .collect()
    };
    let (xs, ys) = (adds(Register::EDX), adds(Register::ECX));
    let [x1, x2] = xs[..] else { return None };
    let [y1, y2] = ys[..] else { return None };
    let (center, offset_x) = if ys.contains(&(x1 + 4)) { (x1, x2) } else if ys.contains(&(x2 + 4)) { (x2, x1) } else { return None };
    let offset_y = if y1 == center + 4 { y2 } else { y1 };
    Some(Projector { trig: trig[0], view_center: center, view_offset_x: offset_x, view_offset_y: offset_y })
}

fn canvas_sizes(pe: &Pe, instrs: &[Instruction]) -> Result<(u32, u32), ScanError> {
    let mut found = HashSet::new();
    for (n, lea) in instrs.iter().enumerate() {
        if lea.mnemonic() != Mnemonic::Lea || lea.op0_register() != Register::RCX || lea.memory_base() != Register::RAX {
            continue;
        }
        let Some(&(_, target)) = calls(&instrs[n + 1..(n + 2).min(instrs.len())]).collect::<Vec<_>>().first() else { continue };
        let body = decode_at(pe, target, 32);
        let reads: Vec<u32> = body
            .iter()
            .filter(|i| i.mnemonic() == Mnemonic::Movd && i.op1_kind() == OpKind::Memory && i.memory_base() == Register::RCX)
            .map(|i| i.memory_displacement64() as u32)
            .collect();
        if let [ui, size, ..] = reads[..] {
            if reads.contains(&(ui + 4)) && reads.contains(&(size + 4)) && body.iter().any(|i| i.mnemonic() == Mnemonic::Divss) {
                let m = lea.memory_displacement64() as u32;
                found.insert((m + size, m + ui));
            }
        }
    }
    one(found.into_iter().collect())
}

fn reachable(pe: &Pe, root: u32, depth: usize) -> Vec<u32> {
    let mut seen = vec![root];
    let mut frontier = vec![root];
    for _ in 0..depth {
        let mut next = Vec::new();
        for function in frontier {
            for (_, target) in calls(&whole_function(pe, function, 0x400)) {
                if !seen.contains(&target) {
                    seen.push(target);
                    next.push(target);
                }
            }
        }
        frontier = next;
    }
    seen
}

fn scene_arrays(pe: &Pe, functions: &[u32], scene: &Scene, size: usize) -> Vec<u32> {
    let mut found = HashSet::new();
    for &function in functions {
        let instrs = whole_function(pe, function, 0x400);
        let reads = |field: u32| instrs.iter().any(|i| i.op_count() == 2 && plain_memory(i) && i.memory_displacement64() == field as u64);
        if !reads(scene.size_x) || !reads(scene.size_y) {
            continue;
        }
        for (n, load) in instrs.iter().enumerate() {
            if load.mnemonic() != Mnemonic::Mov || load.op0_register().size() != 8 || !plain_memory(load) {
                continue;
            }
            let array = load.op0_register();
            let indexed = instrs[n + 1..(n + 7).min(instrs.len())].iter().any(|i| {
                matches!(i.mnemonic(), Mnemonic::Mov | Mnemonic::Movzx)
                    && i.op1_kind() == OpKind::Memory
                    && i.memory_index() != Register::None
                    && (i.memory_base() == array || i.memory_index() == array)
                    && i.memory_size().size() == size
            });
            if indexed {
                found.insert(load.memory_displacement64() as u32);
            }
        }
    }
    found.into_iter().collect()
}

fn tile_x(pe: &Pe, functions: &[u32]) -> Result<u32, ScanError> {
    let mut found = HashSet::new();
    for &function in functions {
        let instrs = decode_at(pe, function, 8);
        let mut fields = Vec::new();
        for [load, store] in instrs.as_chunks::<2>().0.iter().take(3) {
            let copies = load_from(load, Register::RCX).filter(|_| load.op0_register() == Register::EAX).filter(|_| {
                store.mnemonic() == Mnemonic::Mov
                    && store.op0_kind() == OpKind::Memory
                    && store.memory_base() == Register::RDX
                    && store.memory_displacement64() == 4 * fields.len() as u64
                    && store.op1_register() == Register::EAX
            });
            match copies {
                Some(field) => fields.push(field),
                None => break,
            }
        }
        if let [x, plane, y] = fields[..] {
            if y == x + 4 && plane == x + 8 {
                found.insert(x);
            }
        }
    }
    one(found.into_iter().collect())
}

fn tile_model(pe: &Pe) -> Result<(u32, u32), ScanError> {
    const DIVIDE_BY_12: [u8; 10] = [0x48, 0xb8, 0xab, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x2a];
    let mut counts = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(DIVIDE_BY_12.len()).enumerate() {
            if window != DIVIDE_BY_12 || offset < 11 {
                continue;
            }
            let begin = (section.rva + offset - 11) as u32;
            if let [copy, end, sub, ..] = decode_at(pe, begin, 3).as_slice() {
                let begin_field = (sub.mnemonic() == Mnemonic::Sub && sub.op0_register() == Register::RCX && plain_memory(sub) && sub.memory_base() == Register::RAX)
                    .then(|| sub.memory_displacement64() as u32);
                let copies = copy.mnemonic() == Mnemonic::Mov && copy.op0_register() == Register::RAX && copy.op1_register() == Register::RCX;
                let end_field = load_from(end, Register::RCX).filter(|_| end.op0_register() == Register::RCX);
                if let (true, Some(b), Some(e)) = (copies, begin_field, end_field) {
                    if e == b + 8 {
                        counts.push((begin, b));
                    }
                }
            }
        }
    }
    let element = |rva: u64| -> Option<u32> {
        let rva = u32::try_from(rva).ok()?;
        match decode_at(pe, rva, 4).as_slice() {
            [load, scale, address, ret]
                if load.op0_register() == Register::RAX
                    && scale.mnemonic() == Mnemonic::Lea
                    && scale.memory_index_scale() == 2
                    && address.mnemonic() == Mnemonic::Lea
                    && address.memory_index_scale() == 4
                    && ret.mnemonic() == Mnemonic::Ret =>
            {
                load_from(load, Register::RCX)
            }
            _ => None,
        }
    };
    let qword = |at: usize| pe.image.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let mut found = HashSet::new();
    for at in (0..pe.image.len().saturating_sub(0x28)).step_by(8) {
        if pe.code.iter().any(|s| (s.rva..s.rva + s.size).contains(&at)) {
            continue;
        }
        let Some(first) = qword(at) else { continue };
        for &(count, begin) in &counts {
            let base = first.wrapping_sub(count as u64);
            if base & 0xffff != 0 {
                continue;
            }
            let slot = |n: usize| qword(at + 8 * n).map(|q| q.wrapping_sub(base));
            if let (Some(vertices), Some(triangles)) = (slot(2).and_then(element), slot(4).and_then(element)) {
                if triangles == begin {
                    found.insert((vertices, triangles));
                }
            }
        }
    }
    one(found.into_iter().collect())
}

pub fn find_local_projection(image: &[u8], client: u32, canvas: u32, projection: &Projection, scene: &Scene) -> Result<LocalProjection, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let instrs = whole_function(&pe, projection.world_to_screen, 0x300);
    let zoom = zoom(&instrs, client)?;
    let view_holder = view_holder(&pe, &instrs, canvas)?;
    let view = view_slot(&pe, view_holder)?;
    let projectors: Vec<_> = calls(&instrs).filter_map(|(_, target)| projector(&pe, target)).collect();
    let [ref p] = projectors[..] else { return Err(if projectors.is_empty() { ScanError::NotFound } else { ScanError::Ambiguous(projectors.len()) }) };
    let (canvas_size, ui_size) = canvas_sizes(&pe, &instrs)?;

    // the tile height function, behind the jump of `getFineCoordinate`'s callee
    let tile_height = decode_at(&pe, projection.tile_height, 8)
        .iter()
        .find(|i| i.mnemonic() == Mnemonic::Jmp && i.op0_kind() == OpKind::NearBranch64)
        .map_or(projection.tile_height, |i| i.near_branch_target() as u32);
    let instrs = whole_function(&pe, tile_height, 0x400);
    // `lea rcx,[tile+model]`, `lea rcx,[tile+paint]`, then `add rcx,decoration`, before the definition's reference counting
    let mut slots: Vec<u32> = Vec::new();
    for i in &instrs {
        let slot = match i.mnemonic() {
            Mnemonic::Lea if i.op0_register() == Register::RCX && plain_memory(i) => i.memory_displacement64() as u32,
            Mnemonic::Add if i.op0_register() == Register::RCX && matches!(i.op1_kind(), OpKind::Immediate32to64 | OpKind::Immediate8to64) => i.immediate(1) as u32,
            _ => continue,
        };
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    let [tile_model_slot, tile_paint, decoration, ..] = slots[..] else { return Err(ScanError::NotFound) };
    if decoration != scene.ground_decoration {
        return Err(ScanError::NotFound);
    }
    let heights: HashSet<u32> = instrs
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Sub && i.op0_kind() == OpKind::Register && i.op0_register().size() == 4 && plain_memory(i))
        .map(|i| i.memory_displacement64() as u32)
        .collect();
    let decoration_height = one(heights.into_iter().collect())?;

    let functions = reachable(&pe, tile_height, 3);
    let heights = one(scene_arrays(&pe, &functions, scene, 4))?;
    let settings = one(scene_arrays(&pe, &functions, scene, 1))?;
    let tile_x = tile_x(&pe, &reachable(&pe, tile_height, 2))?;
    let (model_vertices, model_triangles) = tile_model(&pe)?;

    Ok(LocalProjection {
        zoom,
        view_holder,
        view,
        view_center: p.view_center,
        view_offset_x: p.view_offset_x,
        view_offset_y: p.view_offset_y,
        trig: p.trig,
        canvas_size,
        ui_size,
        heights,
        settings,
        tile_x,
        tile_model: tile_model_slot,
        tile_paint,
        model_vertices,
        model_triangles,
        decoration_height,
    })
}
