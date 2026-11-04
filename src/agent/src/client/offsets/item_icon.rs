use super::{
    items::string_rvas,
    model::{chunk_root, whole_function},
    *,
};

// The stack number is drawn yellow below 100000, white in thousands below 10 million, green in millions.
const COLOURS: [&str; 3] = ["<col=ffff00>", "<col=ffffff>", "<col=00ff80>"];
const THOUSANDS: u64 = 100_000;
const MILLIONS: u64 = 10_000_000;

/// Every `lea r64, [rip + disp32]` in the code sections that points at `target`.
fn rip_leas(pe: &Pe, target: u32) -> Vec<u32> {
    let mut found = Vec::new();
    for section in &pe.code {
        let data = &pe.image[section.rva..section.rva + section.size];
        for (offset, window) in data.windows(7).enumerate() {
            let rex = window[0] & 0xf8 == 0x48;
            let lea = window[1] == 0x8d && window[2] & 0xc7 == 0x05;
            if !rex || !lea {
                continue;
            }
            let next = (section.rva + offset) as i64 + 7;
            if next + i32::from_le_bytes(window[3..7].try_into().unwrap()) as i64 == target as i64 {
                found.push((section.rva + offset) as u32);
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

/// The function that builds the text of a stack number: the one that names all three colours and compares the quantity with both thresholds.
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

/// The client's item icon builder (Java `ItemComposition.getSprite`): draws an item's model into a sprite with its border, shadow, note and stack number.
///
/// `(out: &mut [u64; 2], id, quantity, border, shadow, text mode, noted: u8, width, height, text: &string)`, returning `out` with the control block and the sprite.
/// Found from the only caller of the stack text builder, a chunk of the builder whose unwind info chains to the whole function.
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
    // the builder's prologue homes the quantity and border, its 3rd and 4th arguments
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
    use super::super::tests::for_each_archived_build;
    use super::*;

    #[test]
    fn finds_item_icon_in_archived_builds() {
        for_each_archived_build(|build, pe| {
            let builder = find_item_icon(pe).unwrap_or_else(|error| {
                panic!("item icon not found in {}: {error:?}", build.display())
            });
            println!("{}: item icon builder {builder:#x}", build.display());
            if build.to_string_lossy().contains("aa10826e") {
                assert_eq!(builder, 0x6016c0, "{}", build.display());
            }
        });
    }
}
