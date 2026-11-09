use super::*;

const SHARED_CALLS: usize = 3;
const NEIGHBOURS: u32 = 0x400;
const MIN_OFFSET: i32 = 0x1000;
const MAX_OFFSET: i32 = 0x10000;

pub fn find_instance_flag(pe: &Pe) -> Result<u32, ScanError> {
    let mut stores: HashMap<i32, (HashSet<u32>, HashSet<u32>)> = HashMap::new();
    for (rva, data) in pe.code_sections() {
        for (at, _) in data.iter().enumerate().filter(|(_, &byte)| byte == 0xc6) {
            let Some(&[_, modrm, d0, d1, d2, d3, imm]) = data[at..].first_chunk::<7>() else {
                continue;
            };
            if modrm & 0xf8 != 0x80 || modrm & 7 == 4 || imm > 1 {
                continue;
            }
            let disp = i32::from_le_bytes([d0, d1, d2, d3]);
            if !(MIN_OFFSET..=MAX_OFFSET).contains(&disp) {
                continue;
            }
            let Some((begin, _)) = pe.function_containing((rva + at) as u32) else {
                continue;
            };
            let entry = stores.entry(disp).or_default();
            if imm == 0 {
                entry.0.insert(begin)
            } else {
                entry.1.insert(begin)
            };
        }
    }

    let mut found = Vec::new();
    for (&disp, (zeros, ones)) in &stores {
        for &zero in zeros {
            for &one in ones {
                if zero != one
                    && zero.abs_diff(one) <= NEIGHBOURS
                    && shared_calls(pe, zero, one) >= SHARED_CALLS
                {
                    found.push(disp as u32);
                }
            }
        }
    }
    one(found)
}

fn callees(pe: &Pe, begin: u32) -> HashSet<u64> {
    let Some((_, end)) = pe.function_containing(begin) else {
        return HashSet::new();
    };
    instructions(pe.bytes(begin, end), begin)
        .filter(|i| call_target(i).is_some())
        .map(|i| i.near_branch_target())
        .collect()
}

fn shared_calls(pe: &Pe, a: u32, b: u32) -> usize {
    callees(pe, a).intersection(&callees(pe, b)).count()
}
