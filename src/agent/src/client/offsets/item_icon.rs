use super::*;

const COLOURS: [&str; 3] = ["<col=ffff00>", "<col=ffffff>", "<col=00ff80>"];
const THOUSANDS: u64 = 100_000;
const MILLIONS: u64 = 10_000_000;

fn functions_naming(pe: &Pe, strings: &[HashSet<u32>]) -> Vec<HashSet<(u32, u32)>> {
    let mut found = vec![HashSet::new(); strings.len()];
    for (rva, data) in pe.code_sections() {
        for (offset, window) in data.windows(7).enumerate() {
            let rex = window[0] & 0xf8 == 0x48;
            let lea = window[1] == 0x8d && window[2] & 0xc7 == 0x05;
            if !rex || !lea {
                continue;
            }
            let site = (rva + offset) as u32;
            let target =
                site as i64 + 7 + i32::from_le_bytes(window[3..7].try_into().unwrap()) as i64;
            let Ok(target) = u32::try_from(target) else {
                continue;
            };
            for (holders, rvas) in found.iter_mut().zip(strings) {
                if rvas.contains(&target) {
                    holders.extend(pe.function_containing(site));
                }
            }
        }
    }
    found
}

fn compares(instrs: &[Instruction], value: u64) -> bool {
    instrs.iter().any(|i| {
        i.mnemonic() == Mnemonic::Cmp
            && i.op1_kind() == OpKind::Immediate32
            && i.immediate(1) == value
    })
}

fn stack_text(pe: &Pe) -> Result<u32, ScanError> {
    let strings: Vec<HashSet<u32>> = COLOURS
        .iter()
        .map(|colour| string_rvas(pe, colour))
        .collect();
    let mut holders = functions_naming(pe, &strings).into_iter();
    let first = holders.next().unwrap_or_default();
    let functions = holders.fold(first, |so_far, holders| &so_far & &holders);
    let candidates: Vec<u32> = functions
        .into_iter()
        .filter(|&(begin, end)| {
            let instrs = whole_function(pe, begin, end - begin);
            compares(&instrs, THOUSANDS) && compares(&instrs, MILLIONS)
        })
        .map(|(begin, _)| begin)
        .collect();
    one(candidates)
}

pub fn find_item_icon(pe: &Pe) -> Result<u32, ScanError> {
    let text = stack_text(pe)?;
    let chunk = one(caller_functions(pe, text).map(|(begin, _)| begin).collect())?;
    let builder = chunk_root(pe, chunk).ok_or(ScanError::NotFound)?;
    let homes = decode_at(pe, builder, 2)
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Memory
                && i.memory_base() == Register::RSP
                && matches!(i.op1_register(), Register::R8D | Register::R9D)
        })
        .count();
    if homes != 2 {
        return Err(ScanError::NotFound);
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{for_each_archived_build, found};
    use super::*;

    #[test]
    fn finds_item_icon_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let builder = found("item icon builder", build, find_item_icon(pe));
            if build.to_string_lossy().contains("aa10826e") {
                assert_eq!(builder, 0x6016c0, "{}", build.display());
            }
        });
    }
}
