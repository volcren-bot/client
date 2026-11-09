use super::{items::string_rvas, *};

const COLOURS: [&str; 3] = ["<col=ffff00>", "<col=ffffff>", "<col=00ff80>"];
const THOUSANDS: u64 = 100_000;
const MILLIONS: u64 = 10_000_000;

fn rip_leas(pe: &Pe, target: u32) -> Vec<u32> {
    let mut found = Vec::new();
    for (rva, data) in pe.code_sections() {
        for (offset, window) in data.windows(7).enumerate() {
            let rex = window[0] & 0xf8 == 0x48;
            let lea = window[1] == 0x8d && window[2] & 0xc7 == 0x05;
            if !rex || !lea {
                continue;
            }
            let next = (rva + offset) as i64 + 7;
            if next + i32::from_le_bytes(window[3..7].try_into().unwrap()) as i64 == target as i64 {
                found.push((rva + offset) as u32);
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
    let mut functions: Option<HashSet<(u32, u32)>> = None;
    for colour in COLOURS {
        let holders: HashSet<(u32, u32)> = string_rvas(pe, colour)
            .into_iter()
            .flat_map(|rva| rip_leas(pe, rva))
            .filter_map(|site| pe.function_containing(site))
            .collect();
        functions = Some(match functions {
            Some(so_far) => &so_far & &holders,
            None => holders,
        });
    }
    let candidates: Vec<u32> = functions
        .unwrap_or_default()
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
    let sites: Vec<u32> = direct_calls(pe)
        .iter()
        .filter(|&&(_, target)| target == text)
        .filter_map(|&(site, _)| pe.function_containing(site))
        .map(|(begin, _)| begin)
        .collect();
    let chunk = one(sites)?;
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
