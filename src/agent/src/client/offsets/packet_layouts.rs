use super::*;
use iced_x86::MemorySize;
use ipc::packets::{Field, Transform};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketLayouts {
    pub sizes: Vec<i32>,
    pub layouts: Vec<Option<Vec<Field>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Read {
    shifts: Vec<u32>,
    transform: Transform,
}

const OBJECT: u64 = 0x10000;
const DATA: u64 = 0x20000;
const OBJECT_SIZE: usize = 0x40;
const PROBE: usize = 8;

fn run(
    instrs: &[Instruction],
    data_at: u32,
    cursor_at: u32,
    bytes: &[u8; PROBE],
) -> Option<(u64, u64)> {
    let mut regs = [0u64; 16];
    regs[Register::RCX.number()] = OBJECT;
    let mut object = [0u8; OBJECT_SIZE];
    object[data_at as usize..data_at as usize + 8].copy_from_slice(&DATA.to_le_bytes());
    object[cursor_at as usize..cursor_at as usize + 8].copy_from_slice(&0u64.to_le_bytes());

    fn reg_get(regs: &[u64; 16], r: Register) -> Option<u64> {
        let full = regs[r.full_register().number()];
        Some(match r.size() {
            8 => full,
            4 => full & 0xffff_ffff,
            2 => full & 0xffff,
            1 if matches!(r, Register::AH | Register::CH | Register::DH | Register::BH) => {
                (full >> 8) & 0xff
            }
            1 => full & 0xff,
            _ => return None,
        })
    }
    fn reg_set(regs: &mut [u64; 16], r: Register, value: u64) -> Option<()> {
        let slot = &mut regs[r.full_register().number()];
        *slot = match r.size() {
            8 => value,
            4 => value & 0xffff_ffff,
            2 => (*slot & !0xffff) | (value & 0xffff),
            1 if matches!(r, Register::AH | Register::CH | Register::DH | Register::BH) => {
                (*slot & !0xff00) | ((value & 0xff) << 8)
            }
            1 => (*slot & !0xff) | (value & 0xff),
            _ => return None,
        };
        Some(())
    }
    let width = |size: MemorySize| match size {
        MemorySize::UInt8 | MemorySize::Int8 => Some(1usize),
        MemorySize::UInt16 | MemorySize::Int16 => Some(2),
        MemorySize::UInt32 | MemorySize::Int32 => Some(4),
        MemorySize::UInt64 | MemorySize::Int64 => Some(8),
        _ => None,
    };
    let address = |regs: &[u64; 16], i: &Instruction| -> Option<u64> {
        if i.memory_base() == Register::RIP || i.segment_prefix() != Register::None {
            return None;
        }
        let base = if i.memory_base() == Register::None {
            0
        } else {
            reg_get(regs, i.memory_base())?
        };
        let index = if i.memory_index() == Register::None {
            0
        } else {
            reg_get(regs, i.memory_index())?.wrapping_mul(i.memory_index_scale() as u64)
        };
        Some(
            base.wrapping_add(index)
                .wrapping_add(i.memory_displacement64()),
        )
    };
    let load = |object: &[u8; OBJECT_SIZE], at: u64, size: usize| -> Option<u64> {
        let mut value = 0u64;
        for k in 0..size {
            let a = at.wrapping_add(k as u64);
            let byte = if (OBJECT..OBJECT + OBJECT_SIZE as u64).contains(&a) {
                object[(a - OBJECT) as usize]
            } else if (DATA..DATA + PROBE as u64).contains(&a) {
                bytes[(a - DATA) as usize]
            } else {
                return None;
            };
            value |= (byte as u64) << (8 * k);
        }
        Some(value)
    };
    let mask = |size: usize| {
        if size >= 8 {
            u64::MAX
        } else {
            (1u64 << (size * 8)) - 1
        }
    };
    let sign = |value: u64, size: usize| -> u64 {
        let bits = size * 8;
        if bits >= 64 || value >> (bits - 1) & 1 == 0 {
            value
        } else {
            value | !mask(size)
        }
    };

    let mut order_equal = None;
    let mut at = 0;
    for _ in 0..64 {
        let i = instrs.get(at)?;
        at += 1;
        let value_of =
            |regs: &[u64; 16], object: &[u8; OBJECT_SIZE], n: u32| -> Option<(u64, usize)> {
                match i.op_kind(n) {
                    OpKind::Register => {
                        Some((reg_get(regs, i.op_register(n))?, i.op_register(n).size()))
                    }
                    OpKind::Memory => {
                        let size = width(i.memory_size())?;
                        Some((load(object, address(regs, i)?, size)?, size))
                    }
                    OpKind::Immediate8
                    | OpKind::Immediate16
                    | OpKind::Immediate32
                    | OpKind::Immediate64
                    | OpKind::Immediate8to16
                    | OpKind::Immediate8to32
                    | OpKind::Immediate8to64
                    | OpKind::Immediate32to64 => Some((i.immediate(n), 0)),
                    _ => None,
                }
            };
        let dest = i.op0_kind();
        let size = match dest {
            OpKind::Register => i.op0_register().size(),
            OpKind::Memory => width(i.memory_size()).unwrap_or(0),
            _ => 0,
        };
        let store =
            |regs: &mut [u64; 16], object: &mut [u8; OBJECT_SIZE], value: u64| -> Option<()> {
                match dest {
                    OpKind::Register => reg_set(regs, i.op0_register(), value),
                    OpKind::Memory => {
                        let at = address(regs, i)?;
                        if !(OBJECT..OBJECT + OBJECT_SIZE as u64).contains(&at)
                            || at + size as u64 > OBJECT + OBJECT_SIZE as u64
                        {
                            return None;
                        }
                        for k in 0..size {
                            object[(at - OBJECT) as usize + k] = (value >> (8 * k)) as u8;
                        }
                        Some(())
                    }
                    _ => None,
                }
            };
        match i.mnemonic() {
            Mnemonic::Ret => {
                let cursor = u64::from_le_bytes(
                    object[cursor_at as usize..cursor_at as usize + 8]
                        .try_into()
                        .ok()?,
                );
                return Some((regs[0], cursor));
            }
            Mnemonic::Mov | Mnemonic::Movzx => {
                let (v, _) = value_of(&regs, &object, 1)?;
                store(&mut regs, &mut object, v)?;
            }
            Mnemonic::Movsx | Mnemonic::Movsxd => {
                let (v, from) = value_of(&regs, &object, 1)?;
                store(&mut regs, &mut object, sign(v, from))?;
            }
            Mnemonic::Lea => {
                let a = address(&regs, i)?;
                store(&mut regs, &mut object, a)?;
            }
            Mnemonic::Add | Mnemonic::Sub | Mnemonic::And | Mnemonic::Or | Mnemonic::Xor => {
                let (a, _) = value_of(&regs, &object, 0)?;
                let (b, bsize) = value_of(&regs, &object, 1)?;
                let b = if bsize == 0 { sign(b, 8) } else { b };
                let r = match i.mnemonic() {
                    Mnemonic::Add => a.wrapping_add(b),
                    Mnemonic::Sub => a.wrapping_sub(b),
                    Mnemonic::And => a & b,
                    Mnemonic::Or => a | b,
                    _ => a ^ b,
                };
                store(&mut regs, &mut object, r & mask(size))?;
            }
            Mnemonic::Inc | Mnemonic::Dec | Mnemonic::Neg | Mnemonic::Not => {
                let (a, _) = value_of(&regs, &object, 0)?;
                let r = match i.mnemonic() {
                    Mnemonic::Inc => a.wrapping_add(1),
                    Mnemonic::Dec => a.wrapping_sub(1),
                    Mnemonic::Neg => a.wrapping_neg(),
                    _ => !a,
                };
                store(&mut regs, &mut object, r & mask(size))?;
            }
            Mnemonic::Shl | Mnemonic::Shr | Mnemonic::Sar => {
                let (a, _) = value_of(&regs, &object, 0)?;
                let count = match i.op1_kind() {
                    OpKind::Register if i.op1_register() == Register::CL => {
                        reg_get(&regs, Register::CL)?
                    }
                    _ => value_of(&regs, &object, 1)?.0,
                } & if size == 8 { 63 } else { 31 };
                let r = match i.mnemonic() {
                    Mnemonic::Shl => a << count,
                    Mnemonic::Shr => a >> count,
                    _ => (sign(a, size) as i64 >> count) as u64,
                };
                store(&mut regs, &mut object, r & mask(size))?;
            }
            Mnemonic::Imul => {
                let (a, b) = if i.op_count() == 3 {
                    (
                        value_of(&regs, &object, 1)?.0,
                        sign(value_of(&regs, &object, 2)?.0, 8),
                    )
                } else {
                    (
                        value_of(&regs, &object, 0)?.0,
                        value_of(&regs, &object, 1)?.0,
                    )
                };
                store(
                    &mut regs,
                    &mut object,
                    sign(a, size).wrapping_mul(sign(b, size)) & mask(size),
                )?;
            }
            Mnemonic::Rol | Mnemonic::Ror => {
                let (a, _) = value_of(&regs, &object, 0)?;
                let bits = size as u64 * 8;
                let count = value_of(&regs, &object, 1)?.0 % bits;
                let a = a & mask(size);
                let back = (bits - count) % bits;
                let r = if i.mnemonic() == Mnemonic::Rol {
                    (a << count | a >> back) & mask(size)
                } else {
                    (a >> count | a << back) & mask(size)
                };
                store(&mut regs, &mut object, r)?;
            }
            Mnemonic::Bswap => {
                let a = reg_get(&regs, i.op0_register())?;
                let r = if i.op0_register().size() == 4 {
                    (a as u32).swap_bytes() as u64
                } else {
                    a.swap_bytes()
                };
                reg_set(&mut regs, i.op0_register(), r)?;
            }
            Mnemonic::Cmp
                if i.memory_base() == Register::RIP && i.immediate(1) as u32 == 0x0302_0100 =>
            {
                order_equal = Some(true)
            }
            Mnemonic::Jne | Mnemonic::Je => {
                let equal = order_equal?;
                if equal == (i.mnemonic() == Mnemonic::Je) {
                    let target = i.near_branch_target();
                    at = instrs.iter().position(|n| n.ip() == target)?.max(at);
                }
            }
            Mnemonic::Nop => {}
            _ => return None,
        }
    }
    None
}

fn through_thunks(pe: &Pe, mut target: u32) -> u32 {
    for _ in 0..3 {
        let Some(bytes) = pe.image.get(target as usize..target as usize + 5) else {
            break;
        };
        if bytes[0] != 0xe9 {
            break;
        }
        let rel = i32::from_le_bytes(bytes[1..5].try_into().unwrap()) as i64;
        match u32::try_from(target as i64 + 5 + rel) {
            Ok(next) => target = next,
            Err(_) => break,
        }
    }
    target
}

fn classify_read(pe: &Pe, begin: u32, data_at: u32, cursor_at: u32) -> Option<Read> {
    let begin = through_thunks(pe, begin);
    let instrs = whole_function(pe, begin, 0x60);
    let probe = |bytes: [u8; PROBE]| run(&instrs, data_at, cursor_at, &bytes);
    let (zero, taken) = probe([0; PROBE])?;
    let width = taken as usize;
    if !(1..=4).contains(&width) {
        return None;
    }
    let mask = (1u64 << (width * 8)) - 1;
    let mut shifts = Vec::new();
    let mut low_step = None;
    for position in 0..width {
        let mut bytes = [0u8; PROBE];
        bytes[position] = 1;
        let (one, again) = probe(bytes)?;
        if again != taken {
            return None;
        }
        let delta = one.wrapping_sub(zero) & mask;
        if delta & 0xff == 1 && delta == 1 || delta == 0xff {
            low_step = Some(delta == 1);
            shifts.push(0);
        } else {
            shifts.push(
                (1..width as u32)
                    .map(|s| s * 8)
                    .find(|&s| delta == 1 << s)?,
            );
        }
    }
    if shifts.iter().filter(|&&s| s == 0).count() != 1 {
        return None;
    }
    let transform = match (low_step?, zero & 0xff) {
        (true, 0) => Transform::None,
        (true, 0x80) => Transform::Add,
        (false, 0) => Transform::Neg,
        (false, 0x80) => Transform::Sub,
        _ => return None,
    };
    let read = Read { shifts, transform };
    for seed in [0x5au8, 0xa7, 0xff, 0x13] {
        let mut bytes = [0u8; PROBE];
        for (k, b) in bytes.iter_mut().enumerate().take(width) {
            *b = seed.wrapping_mul(k as u8 + 3).wrapping_add(k as u8 * 29);
        }
        let (got, _) = probe(bytes)?;
        if got & mask != predict(&read, &bytes) & mask {
            return None;
        }
    }
    Some(read)
}

fn predict(read: &Read, bytes: &[u8; PROBE]) -> u64 {
    let mut value = 0u64;
    for (&shift, &byte) in read.shifts.iter().zip(bytes) {
        let byte = if shift == 0 {
            match read.transform {
                Transform::None => byte,
                Transform::Add => byte.wrapping_sub(128),
                Transform::Neg => byte.wrapping_neg(),
                Transform::Sub => 128u8.wrapping_sub(byte),
            }
        } else {
            byte
        };
        value |= (byte as u64) << shift;
    }
    value
}

fn jump_table(instrs: &[Instruction]) -> Option<(u32, usize)> {
    let jmp = instrs
        .iter()
        .position(|i| i.mnemonic() == Mnemonic::Jmp && i.op0_kind() == OpKind::Register)?;
    let add = jmp
        .checked_sub(1)
        .filter(|&k| instrs[k].mnemonic() == Mnemonic::Add)?;
    let load = instrs[..add].iter().rposition(|i| {
        i.mnemonic() == Mnemonic::Mov
            && i.op1_kind() == OpKind::Memory
            && i.memory_index_scale() == 4
            && i.memory_index() != Register::None
    })?;
    if add - load > 3 {
        return None;
    }
    let table = instrs[load].memory_displacement64() as u32;
    let count = instrs[..load]
        .iter()
        .rev()
        .take(8)
        .find(|i| {
            i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && matches!(
                    i.op1_kind(),
                    OpKind::Immediate8to32 | OpKind::Immediate32 | OpKind::Immediate8
                )
        })?
        .immediate(1) as usize
        + 1;
    (count > 8 && count < 1024).then_some((table, count))
}

fn size_table(pe: &Pe, count: usize) -> Option<u32> {
    let mut found = Vec::new();
    for (rva, data) in pe.code_sections() {
        for at in 6..data.len().saturating_sub(7) {
            if !matches!(data[at], 0x48 | 0x4c)
                || data[at + 1] != 0x8d
                || data[at + 2] & 0xc7 != 0x05
            {
                continue;
            }
            if data[at - 6] != 0x0f || data[at - 5] != 0x83 {
                continue;
            }
            let rel = i32::from_le_bytes(data[at + 3..at + 7].try_into().unwrap()) as i64;
            let target = (rva + at) as i64 + 7 + rel;
            let Some(table) = usize::try_from(target)
                .ok()
                .filter(|t| t + count * 4 <= pe.image.len())
            else {
                continue;
            };
            let sizes = int_table(pe, table, count);
            if sizes.iter().all(|&s| (-2..=512).contains(&s))
                && sizes.iter().collect::<HashSet<_>>().len() > 10
            {
                found.push(table as u32);
            }
        }
    }
    one(found).ok()
}

fn int_table(pe: &Pe, at: usize, count: usize) -> Vec<i32> {
    (0..count)
        .map(|k| u32_at(pe.image, at + k * 4).unwrap() as i32)
        .collect()
}

pub fn find_packet_layouts(pe: &Pe, packets: &Packets) -> Result<PacketLayouts, ScanError> {
    let mut dispatchers = Vec::new();
    for site in calls_to(pe, packets.receive) {
        let Some((begin, end)) = pe.function_containing(site) else {
            continue;
        };
        let instrs = whole_function(pe, begin, end - begin);
        if let Some((table, count)) = jump_table(&instrs) {
            dispatchers.push((table, count, begin));
        }
    }
    let (table, count, _) = one(dispatchers)?;
    let cases: Vec<u32> = (0..count)
        .map(|k| u32_at(pe.image, table as usize + k * 4).ok_or(ScanError::NotFound))
        .collect::<Result<_, _>>()?;
    let size_at = size_table(pe, count).ok_or(ScanError::NotFound)?;
    let sizes = int_table(pe, size_at as usize, count);

    let (data_at, cursor_at) = (packets.buffer_data, packets.buffer_cursor);
    let mut reads: HashMap<u32, Option<Read>> = HashMap::new();
    let mut layouts = Vec::new();
    for (opcode, &case) in cases.iter().enumerate() {
        let end = (case as usize + 0x300).min(pe.image.len());
        let mut found = Vec::new();
        let mut instrs = Decoder::with_ip(
            64,
            &pe.image[case as usize..end],
            case as u64,
            DecoderOptions::NONE,
        )
        .into_iter()
        .peekable();
        while let Some(i) = instrs.next() {
            match i.mnemonic() {
                Mnemonic::Call if i.near_branch_target() != 0 => {
                    let target = i.near_branch_target() as u32;
                    let read = reads
                        .entry(target)
                        .or_insert_with(|| classify_read(pe, target, data_at, cursor_at))
                        .clone();
                    if let Some(read) = read {
                        let signed = matches!(instrs.peek(), Some(n) if matches!(n.mnemonic(), Mnemonic::Movsx | Mnemonic::Movsxd));
                        found.push(Field {
                            signed,
                            shifts: read.shifts.iter().map(|s| (s / 8) as u8).collect(),
                            transform: read.transform,
                        });
                    }
                }
                Mnemonic::Jmp | Mnemonic::Ret | Mnemonic::Int3 => break,
                _ if i.flow_control() == iced_x86::FlowControl::ConditionalBranch => break,
                _ => {}
            }
        }
        let size = sizes[opcode];
        #[cfg(test)]
        if std::env::var("PACKET_DEBUG").is_ok() {
            eprintln!(
                "opcode {opcode} size {size} case {case:x}: {:?}",
                found.iter().map(Field::spelling).collect::<Vec<_>>()
            );
        }
        let width: usize = found.iter().map(|f| f.shifts.len()).sum();
        let layout = match size {
            0 => Some(Vec::new()),
            s if s > 0 && width == s as usize && !found.is_empty() => Some(found),
            _ => None,
        };
        layouts.push(layout);
    }
    Ok(PacketLayouts { sizes, layouts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::offsets::tests::for_each_archived_build;

    fn java_table() -> HashMap<usize, (i32, Option<Vec<String>>)> {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let manifest = if cfg!(windows) && manifest.starts_with('/') {
            format!("Z:{manifest}")
        } else {
            manifest.into()
        };
        let table = std::fs::read_to_string(
            std::path::Path::new(&manifest).join("../loader/src/packet_defs.tsv"),
        )
        .unwrap();
        table
            .lines()
            .filter(|l| l.starts_with("R\t"))
            .map(|l| {
                let c: Vec<&str> = l.split('\t').collect();
                let fields = match c[4] {
                    "" => None,
                    "-" => Some(Vec::new()),
                    layout => Some(layout.split(',').map(str::to_string).collect()),
                };
                (c[1].parse().unwrap(), (c[2].parse().unwrap(), fields))
            })
            .collect()
    }

    #[test]
    fn reads_packet_layouts_in_archived_builds() {
        let java = java_table();
        for_each_archived_build(|build, pe| {
            let packets = find_packets(pe).unwrap();
            let found = find_packet_layouts(pe, &packets).unwrap_or_else(|error| {
                panic!("packet layouts not found in {}: {error:?}", build.display())
            });
            assert_eq!(found.sizes.len(), 151);
            let (mut same, mut different, mut only_here, mut only_java) = (0, Vec::new(), 0, 0);
            for (opcode, layout) in found.layouts.iter().enumerate() {
                let (size, expected) = &java[&opcode];
                assert_eq!(found.sizes[opcode], *size, "size of {opcode}");
                match (layout, expected) {
                    (Some(a), Some(b)) if a.iter().map(Field::spelling).eq(b.iter().cloned()) => {
                        same += 1
                    }
                    (Some(a), Some(b))
                        if a.iter()
                            .map(|k| k.spelling().replacen('i', "u", 1))
                            .eq(b.iter().map(|k| k.replacen('i', "u", 1))) =>
                    {
                        same += 1
                    }
                    (Some(a), Some(b)) => different.push((
                        opcode,
                        a.iter().map(Field::spelling).collect::<Vec<_>>(),
                        b.clone(),
                    )),
                    (Some(_), None) => only_here += 1,
                    (None, Some(_)) => only_java += 1,
                    (None, None) => {}
                }
            }
            println!("{}: {same} same, {only_here} only read here, {only_java} only from Java, differing: {different:?}", build.display());
            assert!(different.is_empty(), "{different:?}");
        });
    }
}
