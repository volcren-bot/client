use super::*;

// The scene loader's two build methods (ordinary, instance) store a 0 or 1 byte in the loader; found as adjacent stores sharing at least `SHARED_CALLS` calls.
const SHARED_CALLS: usize = 3;
const NEIGHBOURS: u32 = 0x400;
const MIN_OFFSET: i32 = 0x1000;
const MAX_OFFSET: i32 = 0x10000;

pub fn find_instance_flag(image: &[u8]) -> Result<u32, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;

    // flag offset -> (functions that store 0, functions that store 1)
    let mut stores: HashMap<i32, (HashSet<u32>, HashSet<u32>)> = HashMap::new();
    for section in &pe.code {
        let data = &image[section.rva..section.rva + section.size];
        for (at, _) in data.iter().enumerate().filter(|(_, &byte)| byte == 0xc6) {
            // mov byte ptr [reg + disp32], imm8
            let Some(&[_, modrm, d0, d1, d2, d3, imm]) = data.get(at..at + 7).map(|w| <&[u8; 7]>::try_from(w).unwrap()) else { continue };
            if modrm & 0xf8 != 0x80 || modrm & 7 == 4 || imm > 1 {
                continue;
            }
            let disp = i32::from_le_bytes([d0, d1, d2, d3]);
            if !(MIN_OFFSET..=MAX_OFFSET).contains(&disp) {
                continue;
            }
            let Some((begin, _)) = pe.function_containing((section.rva + at) as u32) else { continue };
            let entry = stores.entry(disp).or_default();
            if imm == 0 { entry.0.insert(begin) } else { entry.1.insert(begin) };
        }
    }

    let mut found = Vec::new();
    for (&disp, (zeros, ones)) in &stores {
        for &zero in zeros {
            for &one in ones {
                if zero != one && zero.abs_diff(one) <= NEIGHBOURS && shared_calls(&pe, zero, one) >= SHARED_CALLS {
                    found.push(disp as u32);
                }
            }
        }
    }
    one(found)
}

fn call_targets(pe: &Pe, begin: u32) -> HashSet<u64> {
    let Some((_, end)) = pe.function_containing(begin) else { return HashSet::new() };
    Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE)
        .into_iter()
        .filter(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)
        .map(|i| i.near_branch_target())
        .collect()
}

fn shared_calls(pe: &Pe, a: u32, b: u32) -> usize {
    call_targets(pe, a).intersection(&call_targets(pe, b)).count()
}
