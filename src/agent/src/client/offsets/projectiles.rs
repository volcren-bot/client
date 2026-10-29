use super::{
    model::{chunk_root, whole_function},
    *,
};

// Java's `Projectile.update` uses `tan(slope * 0.02454369)`; the constant appears once in the native client.
const SLOPE_FACTOR: f64 = 0.02454369;

/// The projectile list and fields, from the update function, its caller loop and the constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Projectiles {
    /// the list in the client object: a circular list whose head is the sentinel node (next, previous, size)
    pub list: u32,
    /// the projectile pointer in a list node
    pub node: u32,
    /// RVA of the client cycle the loop compares the end cycle with (50 per second)
    pub cycle: u32,
    pub source_level: u32,
    pub source_x: u32,
    pub source_y: u32,
    pub source_actor: u32,
    pub start_height: u32,
    pub target_level: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub target_actor: u32,
    pub end_height: u32,
    pub start_cycle: u32,
    pub end_cycle: u32,
    pub slope: u32,
    pub start_pos: u32,
    pub id: u32,
    /// local fine x, height and y as doubles (x, z, y in Java)
    pub x: u32,
    pub z: u32,
    pub y: u32,
    pub orientation: u32,
    /// the byte set once the projectile has started moving
    pub moving: u32,
}

fn field(i: &Instruction) -> Option<u32> {
    (i.op_kinds().any(|k| k == OpKind::Memory)
        && !matches!(i.memory_base(), Register::RSP | Register::RIP | Register::None)
        && i.memory_index() == Register::None)
        .then(|| i.memory_displacement64() as u32)
}

fn constant_rvas(pe: &Pe, value: f64) -> HashSet<u32> {
    let needle = value.to_le_bytes();
    pe.image.windows(8).enumerate().filter(|(_, w)| *w == needle).map(|(at, _)| at as u32).collect()
}

// The args of a call to the actor position helper: (actor in edx, x in r8d, y in r9d, level stored at [rsp+0x20]).
fn position_args(before: &[Instruction]) -> Option<(u32, u32, u32, u32)> {
    let mut actor = None;
    let (mut x, mut y, mut level) = (None, None, None);
    let mut level_reg = None;
    for i in before.iter().rev() {
        if i.mnemonic() == Mnemonic::Call {
            break;
        }
        if i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.memory_base() == Register::RSP && i.memory_displacement64() == 0x20 {
            level_reg.get_or_insert(i.op1_register());
        }
        if i.mnemonic() != Mnemonic::Mov || i.op0_kind() != OpKind::Register {
            continue;
        }
        let Some(disp) = field(i) else { continue };
        match i.op0_register() {
            Register::EDX => _ = actor.get_or_insert(disp),
            Register::R8D => _ = x.get_or_insert(disp),
            Register::R9D => _ = y.get_or_insert(disp),
            r if Some(r) == level_reg => _ = level.get_or_insert(disp),
            _ => {}
        }
    }
    Some((actor?, x?, y?, level?))
}

// The update function: everything but the id, the list and the cycle.
fn update_fields(instrs: &[Instruction], slope_rvas: &HashSet<u32>) -> Option<Projectiles> {
    let start_cycle = instrs.iter().take(16).find(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Register).and_then(field)?;

    // two calls to the same helper with an actor and a tile: the target's position, then the source's
    let mut helpers: HashMap<u64, Vec<usize>> = HashMap::new();
    for (at, i) in instrs.iter().enumerate() {
        if i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64 && position_args(&instrs[..at]).is_some() {
            helpers.entry(i.near_branch_target()).or_default().push(at);
        }
    }
    let [target_call, source_call] = helpers.into_values().find(|calls| calls.len() == 2)?[..] else { return None };
    let (target_actor, target_x, target_y, target_level) = position_args(&instrs[..target_call])?;
    let (source_actor, source_x, source_y, source_level) = position_args(&instrs[..source_call])?;
    // the height subtracted from each position's
    let subtracted = |from: usize| instrs[from..].iter().take(12).filter(|i| i.mnemonic() == Mnemonic::Sub && i.op0_kind() == OpKind::Register).find_map(field);
    let end_height = subtracted(target_call)?;
    let start_height = subtracted(source_call)?;
    let moving = instrs[target_call..source_call]
        .iter()
        .find(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt8)
        .and_then(field)?;

    // `movd xmm, [slope]; cvtdq2pd; mulsd xmm, [SLOPE_FACTOR]`
    let factor = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Mulsd && i.is_ip_rel_memory_operand() && slope_rvas.contains(&(i.ip_rel_memory_address() as u32)))?;
    let slope = instrs[factor.saturating_sub(3)..factor].iter().find(|i| i.mnemonic() == Mnemonic::Movd).and_then(field)?;
    // the start position along the line, `movd xmm, [start_pos]` scaled by the distance
    let start_pos = one(instrs[..factor].iter().filter(|i| i.mnemonic() == Mnemonic::Movd).filter_map(field).filter(|&d| d != slope).collect()).ok()?;
    // `mov eax, [end_cycle]`, a few instructions later `sub eax, cycle; inc eax`
    let end_cycle = instrs.windows(2).enumerate().find_map(|(at, w)| {
        let (sub, inc) = (&w[0], &w[1]);
        if !(sub.mnemonic() == Mnemonic::Sub && sub.op1_kind() == OpKind::Register && inc.mnemonic() == Mnemonic::Inc && inc.op0_register() == sub.op0_register()) {
            return None;
        }
        let load = instrs[at.saturating_sub(10)..at].iter().rev().find(|i| i.op0_kind() == OpKind::Register && i.op0_register() == sub.op0_register())?;
        field(load).filter(|_| load.mnemonic() == Mnemonic::Mov)
    })?;
    // the position written back to the caller: `cvttsd2si eax, [x]; mov [out], eax`, then +4 (z) and +8 (y)
    let mut out = [None; 3];
    for (at, convert) in instrs.iter().enumerate() {
        let Some(disp) = field(convert).filter(|_| convert.mnemonic() == Mnemonic::Cvttsd2si) else { continue };
        let store = instrs[at + 1..].iter().find(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Memory && i.op1_kind() == OpKind::Register && i.op1_register() == convert.op0_register());
        if let Some(store) = store.filter(|s| s.memory_base() != Register::RSP && matches!(s.memory_displacement64(), 0 | 4 | 8)) {
            out[store.memory_displacement64() as usize / 4] = Some(disp);
        }
    }
    let [Some(x), Some(z), Some(y)] = out else { return None };
    // `and ecx, 0x7ff; mov [orientation], ecx`: the first of the two angles (then the pitch)
    let orientation = instrs.windows(2).find_map(|w| {
        (w[0].mnemonic() == Mnemonic::And
            && w[0].op1_kind() == OpKind::Immediate32
            && w[0].immediate(1) == 0x7ff
            && w[1].mnemonic() == Mnemonic::Mov
            && w[1].op0_kind() == OpKind::Memory
            && w[1].op1_register() == w[0].op0_register())
            .then(|| field(&w[1]))
            .flatten()
    })?;

    Some(Projectiles {
        list: 0,
        node: 0,
        cycle: 0,
        source_level,
        source_x,
        source_y,
        source_actor,
        start_height,
        target_level,
        target_x,
        target_y,
        target_actor,
        end_height,
        start_cycle,
        end_cycle,
        slope,
        start_pos,
        id: 0,
        x,
        z,
        y,
        orientation,
        moving,
    })
}

// The loop over the list: `mov rcx, [node + value]` (a few instructions up), `mov eax, [rcx + end_cycle]; cmp [rip + cycle], eax`.
fn loop_fields(instrs: &[Instruction], end_cycle: u32) -> Option<(u32, u32)> {
    instrs.windows(2).enumerate().find_map(|(at, w)| {
        let (load, compare) = (&w[0], &w[1]);
        if !(load.mnemonic() == Mnemonic::Mov
            && load.op1_kind() == OpKind::Memory
            && load.memory_displacement64() == end_cycle as u64
            && compare.mnemonic() == Mnemonic::Cmp
            && compare.is_ip_rel_memory_operand()
            && compare.op1_register() == load.op0_register())
        {
            return None;
        }
        let value = instrs[at.saturating_sub(6)..at].iter().rev().find(|i| {
            i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register() == load.memory_base() && i.op1_kind() == OpKind::Memory
        })?;
        Some((field(value)?, compare.ip_rel_memory_address() as u32))
    })
}

// The client method that runs the loop: `mov rdx, rcx; add rcx, list; jmp loop`.
fn list_thunk(instrs: &[Instruction], loop_rva: u32) -> Option<u32> {
    let [mov, add, jmp, ..] = instrs else { return None };
    (mov.mnemonic() == Mnemonic::Mov
        && mov.op0_register() == Register::RDX
        && mov.op1_register() == Register::RCX
        && add.mnemonic() == Mnemonic::Add
        && add.op0_register() == Register::RCX
        && matches!(add.op1_kind(), OpKind::Immediate32to64 | OpKind::Immediate8to64)
        && matches!(jmp.mnemonic(), Mnemonic::Jmp | Mnemonic::Call)
        && jmp.op0_kind() == OpKind::NearBranch64
        && jmp.near_branch_target() == loop_rva as u64)
        .then(|| add.immediate(1) as u32)
}

// The constructor copies its parameter block over the fields, then reads the id back to look up the spot animation.
fn constructor_id(instrs: &[Instruction], fields: &Projectiles) -> Option<u32> {
    let stores = |mnemonic: Mnemonic, at: u32| instrs.iter().any(|i| i.mnemonic() == mnemonic && i.op0_kind() == OpKind::Memory && field(i) == Some(at));
    if !(stores(Mnemonic::Movups, fields.source_level) && stores(Mnemonic::Mov, fields.moving)) {
        return None;
    }
    instrs.iter().enumerate().find_map(|(at, store)| {
        let id = field(store).filter(|_| {
            store.mnemonic() == Mnemonic::Mov && store.op0_kind() == OpKind::Memory && store.op1_kind() == OpKind::Register && store.op1_register().size() == 4
        })?;
        instrs[at + 1..(at + 4).min(instrs.len())]
            .iter()
            .any(|load| {
                load.mnemonic() == Mnemonic::Mov
                    && load.op0_kind() == OpKind::Register
                    && load.op1_kind() == OpKind::Memory
                    && load.memory_base() == store.memory_base()
                    && field(load) == Some(id)
            })
            .then_some(id)
    })
}

pub fn find_projectiles(image: &[u8]) -> Result<Projectiles, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let slope_rvas = constant_rvas(&pe, SLOPE_FACTOR);
    if slope_rvas.is_empty() {
        return Err(ScanError::NotFound);
    }
    let mut updates = Vec::new();
    for &(begin, end) in &pe.functions {
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        if instrs.iter().any(|i| i.is_ip_rel_memory_operand() && slope_rvas.contains(&(i.ip_rel_memory_address() as u32))) {
            updates.extend(chunk_root(&pe, begin));
        }
    }
    let update = one(updates)?;
    let mut fields = update_fields(&whole_function(&pe, update, 0x800), &slope_rvas).ok_or(ScanError::NotFound)?;

    let callers: Vec<u32> = direct_calls(&pe).into_iter().filter(|&(_, target)| target == update).filter_map(|(site, _)| pe.function_containing(site)).filter_map(|(begin, _)| chunk_root(&pe, begin)).collect();
    let mut loops = Vec::new();
    for caller in callers {
        if let Some((node, cycle)) = loop_fields(&whole_function(&pe, caller, 0x400), fields.end_cycle) {
            loops.push((caller, node, cycle));
        }
    }
    let (loop_rva, node, cycle) = one(loops)?;
    fields.node = node;
    fields.cycle = cycle;

    let mut lists = Vec::new();
    let mut ids = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        // `mov rdx, rcx` (48 8b d1 or 48 89 ca) starts the thunk
        for (offset, w) in data.windows(3).enumerate() {
            if w == [0x48, 0x8b, 0xd1] || w == [0x48, 0x89, 0xca] {
                lists.extend(list_thunk(&decode_at(&pe, (section.rva + offset) as u32, 3), loop_rva));
            }
        }
    }
    fields.list = one(lists)?;
    for &(begin, end) in &pe.functions {
        if end - begin > 0x400 {
            continue;
        }
        let instrs: Vec<Instruction> = Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect();
        ids.extend(constructor_id(&instrs, &fields));
    }
    fields.id = one(ids)?;
    Ok(fields)
}
