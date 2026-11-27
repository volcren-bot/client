use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcModel {
    pub definition: u32,
    pub get_model: u32,
    pub bounding_box: u32,
}

fn is_root(pe: &Pe, begin: u32) -> bool {
    chunk_root(pe, begin) == Some(begin)
}

pub(super) fn field_load(i: &Instruction, field: u32) -> Option<Register> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Register
        && i.op0_register().size() == 8
        && i.op1_kind() == OpKind::Memory
        && i.memory_index() == Register::None
        && i.memory_displacement64() == field as u64)
        .then(|| i.op0_register())
}

fn compares_one(i: &Instruction, base: Register) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Cmp
        && i.op0_kind() == OpKind::Memory
        && i.memory_base() == base
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt32
        && matches!(i.op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
        && i.immediate(1) == 1)
        .then(|| i.memory_displacement64() as u32)
}

fn sets_flag(i: &Instruction) -> Option<u32> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Memory
        && i.memory_index() == Register::None
        && i.memory_size() == iced_x86::MemorySize::UInt8
        && i.op1_kind() == OpKind::Immediate8
        && i.immediate(1) == 1)
        .then(|| i.memory_displacement64() as u32)
}

const FLAG_WINDOW: usize = 2;
const MODEL_RELOAD_WINDOW: usize = 4;

fn object_load(i: &Instruction) -> Option<(Register, u32)> {
    (i.mnemonic() == Mnemonic::Mov
        && i.op0_kind() == OpKind::Register
        && i.op0_register().size() == 8
        && i.op1_kind() == OpKind::Memory
        && i.memory_index() == Register::None
        && !matches!(
            i.memory_base(),
            Register::None | Register::RIP | Register::RSP
        ))
    .then(|| (i.op0_register(), i.memory_displacement64() as u32))
}

pub fn find_npc_model(pe: &Pe) -> Result<NpcModel, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs = function_instrs(pe, begin, end);
        for (n, load) in instrs.iter().enumerate() {
            let Some((definition, field)) = object_load(load) else {
                continue;
            };
            let [compare, skip, rest @ ..] = &instrs[n + 1..] else {
                continue;
            };
            let Some(size) = compares_one(compare, definition) else {
                continue;
            };
            if skip.mnemonic() != Mnemonic::Jne {
                continue;
            }
            let Some(flag) = rest
                .iter()
                .take(FLAG_WINDOW)
                .find(|i| i.next_ip() == skip.near_branch_target())
                .and_then(sets_flag)
            else {
                continue;
            };
            if let Some(root) = chunk_root(pe, begin) {
                found.push((field, root, size, flag));
            }
        }
    }
    let (definition, get_model, _, bounding_box) = one(found)?;
    Ok(NpcModel {
        definition,
        get_model,
        bounding_box,
    })
}

const MAX_BOUNDS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelVertices {
    pub bounds: [u32; MAX_BOUNDS],
    pub height: u32,
    pub count: u32,
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

fn model_height(pe: &Pe, get_model: u32, bounding_box: u32) -> Option<u32> {
    let instrs = whole_function(pe, get_model, 0x1000);
    let flag = instrs
        .iter()
        .position(|i| sets_flag(i) == Some(bounding_box))?;
    let model = instrs[flag].memory_base();
    let stack_slot = |i: &Instruction, register: Register| {
        (i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Register
            && i.op0_register() == register
            && i.op1_kind() == OpKind::Memory
            && i.memory_base() == Register::RSP)
            .then(|| i.memory_displacement64())
    };
    let spilled = flag
        .checked_sub(1)
        .and_then(|before| stack_slot(&instrs[before], model));
    let holds_model = |at: usize, register: Register| {
        register == model
            || spilled.is_some_and(|slot| {
                instrs[at.saturating_sub(MODEL_RELOAD_WINDOW)..at]
                    .iter()
                    .any(|i| stack_slot(i, register) == Some(slot))
            })
    };
    let at = instrs[..flag].iter().enumerate().position(|(at, i)| {
        i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Register
            && i.op0_register().size() == 4
            && i.op1_kind() == OpKind::Memory
            && i.memory_index() == Register::None
            && holds_model(at, i.memory_base())
    })?;
    let height = instrs[at].memory_displacement64() as u32;
    let follows_virtual_call = instrs[at.saturating_sub(3)..at].iter().any(|i| {
        i.mnemonic() == Mnemonic::Call
            && i.op0_kind() == OpKind::Memory
            && i.memory_base() == Register::RAX
    });
    follows_virtual_call.then_some(height)
}

fn bounds_cylinder(instrs: &[Instruction], height: u32) -> Option<ModelVertices> {
    let mem_dword = |i: &Instruction| {
        i.op0_kind() == OpKind::Memory
            && i.memory_index() == Register::None
            && i.memory_size() == iced_x86::MemorySize::UInt32
    };
    let stores_height = instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Mov && mem_dword(i) && i.memory_displacement64() == height as u64
    });
    if !stores_height || !instrs.iter().any(|i| i.mnemonic() == Mnemonic::Neg) {
        return None;
    }
    let pointer_loads: Vec<(Register, u32)> = instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_kind() == OpKind::Memory
                && i.memory_base() != Register::RIP
                && i.memory_base() != Register::RSP
                && i.memory_index() == Register::None
        })
        .map(|i| (i.memory_base(), i.memory_displacement64() as u32))
        .collect();
    let (base, x) = pointer_loads.windows(3).find_map(|w| {
        (w[0].0 == w[1].0
            && w[1].0 == w[2].0
            && w[0].1.checked_add(0x18) == Some(w[1].1)
            && w[0].1.checked_add(0x30) == Some(w[2].1))
        .then_some(w[0])
    })?;
    let count = instrs.iter().find_map(|i| {
        (i.mnemonic() == Mnemonic::Mov
            && i.op0_kind() == OpKind::Register
            && i.op0_register().size() == 4
            && i.op1_kind() == OpKind::Memory
            && i.memory_base() == Register::RCX
            && i.memory_index() == Register::None)
            .then(|| i.memory_displacement64() as u32)
    })?;
    let loads_count_from_base = base == Register::RCX
        || instrs.iter().any(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_register() == base
                && i.op1_register() == Register::RCX
        });
    let mut bounds = [0; MAX_BOUNDS];
    bounds[0] = instrs.first()?.ip() as u32;
    loads_count_from_base.then_some(ModelVertices {
        bounds,
        height,
        count,
        x,
        y: x + 0x18,
        z: x + 0x30,
    })
}

pub fn find_model_vertices(pe: &Pe, rule: &NpcModel) -> Result<ModelVertices, ScanError> {
    let height = model_height(pe, rule.get_model, rule.bounding_box).ok_or(ScanError::NotFound)?;
    let mut found = Vec::new();
    for &(begin, _) in &pe.functions {
        if !is_root(pe, begin) {
            continue;
        }
        let instrs = whole_function(pe, begin, 0x400);
        found.extend(bounds_cylinder(&instrs, height));
    }
    let mut merged = *found.first().ok_or(ScanError::NotFound)?;
    let same = |v: &ModelVertices| {
        ModelVertices {
            bounds: merged.bounds,
            ..*v
        } == merged
    };
    if !found.iter().all(same) || found.len() > MAX_BOUNDS {
        return Err(ScanError::Ambiguous(found.len()));
    }
    for (slot, v) in merged.bounds.iter_mut().zip(&found) {
        *slot = v.bounds[0];
    }
    Ok(merged)
}

pub fn find_animation_height(pe: &Pe, npc_definition: u32) -> Result<u32, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if end - begin > 0x60 {
            continue;
        }
        let instrs = function_instrs(pe, begin, end);
        if !instrs
            .iter()
            .any(|i| field_load(i, npc_definition).is_some() && i.memory_base() == Register::RCX)
        {
            continue;
        }
        let Some(default) = instrs.iter().find_map(|i| {
            (i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && i.op1_kind() == OpKind::Immediate8to32
                && i.immediate(1) as i32 == -1)
                .then(|| i.op0_register())
        }) else {
            continue;
        };
        for w in instrs.windows(2) {
            let adds = w[1].mnemonic() == Mnemonic::Add
                && w[1].op0_kind() == OpKind::Register
                && w[1].op0_register() == Register::EAX
                && w[1].op1_kind() == OpKind::Register
                && w[1].op1_register() == default;
            if adds {
                found.extend(call_target(&w[0]));
            }
        }
    }
    one(found)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelFaces {
    pub count: u32,
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub color: u32,
}

fn model_faces(instrs: &[Instruction], vertices: &ModelVertices) -> Option<ModelFaces> {
    let mut by_base: HashMap<Register, (HashSet<u32>, HashSet<u32>)> = Default::default();
    for i in instrs {
        if i.mnemonic() != Mnemonic::Mov
            || i.op0_kind() != OpKind::Register
            || i.op1_kind() != OpKind::Memory
            || i.memory_index() != Register::None
        {
            continue;
        }
        let entry = by_base.entry(i.memory_base()).or_default();
        let displacement = i.memory_displacement64() as u32;
        match i.op0_register().size() {
            8 => entry.0.insert(displacement),
            4 => entry.1.insert(displacement),
            _ => false,
        };
    }
    let triple = |set: &HashSet<u32>, d: u32| {
        set.contains(&d) && set.contains(&(d + 0x18)) && set.contains(&(d + 0x30))
    };
    by_base.values().find_map(|(pointers, dwords)| {
        if !triple(pointers, vertices.x) || !dwords.contains(&vertices.count) {
            return None;
        }
        let a = (vertices.z + 8..vertices.z + 0x100)
            .step_by(8)
            .find(|&d| triple(pointers, d))?;
        let colors = a + 0x48;
        (triple(pointers, colors) && dwords.contains(&(a - 0x18))).then_some(ModelFaces {
            count: a - 0x18,
            a,
            b: a + 0x18,
            c: a + 0x30,
            color: colors + 0x30,
        })
    })
}

pub fn find_model_faces(pe: &Pe, vertices: &ModelVertices) -> Result<ModelFaces, ScanError> {
    let mut found = Vec::new();
    for &(begin, _) in &pe.functions {
        if !is_root(pe, begin) {
            continue;
        }
        let instrs = whole_function(pe, begin, 0x300);
        if instrs
            .iter()
            .any(|i| i.is_jcc_short_or_near() && i.near_branch_target() < i.ip())
        {
            continue;
        }
        found.extend(model_faces(&instrs, vertices));
    }
    one(found)
}

pub fn render_slots(image: &[u8], function: u32) -> Vec<u32> {
    let instrs = image_function(image, function, 0x200);

    let mut offsets: HashMap<Register, i64> = HashMap::from([(Register::RCX, 0)]);
    let mut found = Vec::new();
    for i in &instrs {
        let reg = (i.op0_kind() == OpKind::Register).then(|| i.op0_register().full_register());
        match (i.mnemonic(), reg) {
            (Mnemonic::Cmp, _)
                if i.op0_kind() == OpKind::Memory
                    && i.memory_size() == iced_x86::MemorySize::UInt64
                    && i.memory_index() == Register::None
                    && matches!(
                        i.op1_kind(),
                        OpKind::Immediate8to64 | OpKind::Immediate32to64
                    )
                    && i.immediate(1) == 0 =>
            {
                if let Some(&base) = offsets.get(&i.memory_base()) {
                    let offset = (base + i.memory_displacement64() as i64) as u32;
                    if !found.contains(&offset) {
                        found.push(offset);
                    }
                }
            }
            (Mnemonic::Mov, Some(reg)) if i.op1_kind() == OpKind::Register => {
                match offsets.get(&i.op1_register().full_register()).copied() {
                    Some(offset) => offsets.insert(reg, offset),
                    None => offsets.remove(&reg),
                };
            }
            (Mnemonic::Add, Some(reg))
                if matches!(
                    i.op1_kind(),
                    OpKind::Immediate8to64 | OpKind::Immediate32to64
                ) =>
            {
                if let Some(offset) = offsets.get_mut(&reg) {
                    *offset += i.immediate(1) as i64;
                }
            }
            (Mnemonic::Lea, Some(reg)) if i.memory_index() == Register::None => {
                match offsets.get(&i.memory_base()).copied() {
                    Some(base) => offsets.insert(reg, base + i.memory_displacement64() as i64),
                    None => offsets.remove(&reg),
                };
            }
            (_, Some(reg)) => {
                offsets.remove(&reg);
            }
            _ => {}
        }
    }
    found
}
