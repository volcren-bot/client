use super::{
    model::whole_function,
    scripts::{range_handler, switch_cases},
    *,
};

// The chat history, found through the script ops that read it: a global uid hash table whose values are message objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chat {
    pub messages: u32,
    pub cycle: u32,
    pub message_cycle: u32,
    pub kind: u32,
    pub name: u32,
    pub sender: u32,
    pub text: u32,
    pub time_utc: u32,
    pub time_local: u32,
}

const CHAT_OPS: u32 = 5000;
// chat_gethistory_byuid: type, cycle, name, sender, text (with the timestamp the setting adds)
const HISTORY_BY_UID: usize = 4;
// native only: like chat_gethistory_byindex, but pushes the text as received and the timestamp
const HISTORY_EX: usize = 30;
const CLOCK_OPS: u32 = 3300;
// clientclock: pushes the client cycle, 50 per second
const CLIENT_CLOCK: usize = 0;

fn case_code(pe: &Pe, cases: &[u32], case: usize) -> Option<Vec<Instruction>> {
    let begin = *cases.get(case)?;
    let end = cases.iter().copied().filter(|&c| c > begin).min().unwrap_or(begin + 0x1000);
    Some(Decoder::with_ip(64, pe.bytes(begin, end), begin as u64, DecoderOptions::NONE).into_iter().collect())
}

fn field(i: &Instruction, base: Register) -> Option<u32> {
    (i.op1_kind() == OpKind::Memory && i.memory_base() == base && i.memory_index() == Register::None).then(|| i.memory_displacement64() as u32)
}

fn uid_table(pe: &Pe, lookup: u32) -> Option<u32> {
    let instrs = whole_function(pe, lookup, 0x80);
    if !instrs.iter().any(|i| i.mnemonic() == Mnemonic::Div) {
        return None;
    }
    let globals: Vec<u32> = instrs
        .iter()
        .take_while(|i| i.mnemonic() != Mnemonic::Ret)
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_register().size() == 8 && i.is_ip_rel_memory_operand())
        .map(|i| i.ip_rel_memory_address() as u32)
        .collect();
    let buckets = hash_table::BUCKETS as u32;
    let count = hash_table::BUCKET_COUNT as u32;
    globals.iter().find(|&&b| globals.contains(&(b + count - buckets))).map(|&b| b - buckets)
}

struct History {
    messages: u32,
    kind: u32,
    cycle: u32,
    strings: [u32; 3],
}

fn history(pe: &Pe, instrs: &[Instruction]) -> Option<History> {
    let call = instrs.iter().position(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64)?;
    let messages = uid_table(pe, instrs[call].near_branch_target() as u32)?;
    let keep = instrs[call + 1..].iter().find(|i| {
        i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op1_kind() == OpKind::Register && i.op1_register() == Register::RAX
    })?;
    let message = keep.op0_register();
    let after = &instrs[call + 1..];
    let ints: Vec<u32> = after
        .iter()
        .filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 4)
        .filter_map(|i| field(i, message))
        .collect();
    let mut strings: Vec<u32> = Vec::new();
    for offset in after.iter().filter(|i| i.mnemonic() == Mnemonic::Lea).filter_map(|i| field(i, message)) {
        if !strings.contains(&offset) {
            strings.push(offset);
        }
    }
    let (&[kind, cycle, ..], &[name, sender, text, ..]) = (ints.as_slice(), strings.as_slice()) else { return None };
    Some(History { messages, kind, cycle, strings: [name, sender, text] })
}

// The timestamp chooser: a call-mode switch whose branches return `lea rax,[message + stamp]`.
fn stamps(pe: &Pe, function: u32) -> Option<(u32, u32)> {
    let instrs = whole_function(pe, function, 0x80);
    let branch = |compare: Mnemonic| {
        instrs.windows(2).find_map(|w| {
            let tests = w[0].mnemonic() == compare
                && w[0].op0_register() == Register::EAX
                && matches!(w[0].op1_kind(), OpKind::Immediate8to32 | OpKind::Immediate32)
                && w[0].immediate(1) == 1;
            (tests && w[1].mnemonic() == Mnemonic::Je).then(|| w[1].near_branch_target() as u32)
        })
    };
    let stamp = |at: u32| {
        let lea = decode_at(pe, at, 1).into_iter().next()?;
        (lea.mnemonic() == Mnemonic::Lea && lea.op0_register() == Register::RAX && lea.memory_index() == Register::None && !lea.is_ip_rel_memory_operand())
            .then(|| lea.memory_displacement64() as u32)
    };
    Some((stamp(branch(Mnemonic::Sub)?)?, stamp(branch(Mnemonic::Cmp)?)?))
}

fn client_clock(pe: &Pe) -> Result<u32, ScanError> {
    let cases = switch_cases(pe, range_handler(pe, CLOCK_OPS + 100)?, CLOCK_OPS)?;
    let code = case_code(pe, &cases, CLIENT_CLOCK).ok_or(ScanError::NotFound)?;
    let load = code.first().ok_or(ScanError::NotFound)?;
    (load.mnemonic() == Mnemonic::Mov && load.op0_register().size() == 4 && load.is_ip_rel_memory_operand())
        .then(|| load.ip_rel_memory_address() as u32)
        .ok_or(ScanError::NotFound)
}

pub fn find_chat(pe: &Pe) -> Result<Chat, ScanError> {
    let cases = switch_cases(pe, range_handler(pe, CHAT_OPS + 100)?, CHAT_OPS)?;
    let by_uid = case_code(pe, &cases, HISTORY_BY_UID).ok_or(ScanError::NotFound)?;
    let History { messages, kind, cycle, strings } = history(pe, &by_uid).ok_or(ScanError::NotFound)?;
    let [name, sender, display] = strings;

    // HISTORY_EX reads the same strings from another register, and the text as received after them
    let ex = case_code(pe, &cases, HISTORY_EX).ok_or(ScanError::NotFound)?;
    let mut leas: HashMap<Register, Vec<u32>> = HashMap::new();
    for i in ex.iter().filter(|i| i.mnemonic() == Mnemonic::Lea && i.memory_index() == Register::None && !i.is_ip_rel_memory_operand()) {
        leas.entry(i.memory_base()).or_default().push(i.memory_displacement64() as u32);
    }
    let texts = leas
        .values()
        .filter(|offsets| strings.iter().all(|s| offsets.contains(s)))
        .flat_map(|offsets| offsets.iter().copied().filter(|&o| o > display))
        .collect();
    let text = one(texts)?;
    let calls = ex.iter().filter(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64);
    let (time_utc, time_local) = one(calls.filter_map(|i| stamps(pe, i.near_branch_target() as u32)).collect())?;

    Ok(Chat { messages, cycle: client_clock(pe)?, message_cycle: cycle, kind, name, sender, text, time_utc, time_local })
}
