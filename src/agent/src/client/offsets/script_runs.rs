use super::{model::whole_function, *};

/// The function that runs a script for an event, and where an event keeps what the script log records.
///
/// Java's `StructComposition.az(event, ...)` runs a `ScriptEvent`; the native client does the same in one function that picks
/// the script from the event's type and arguments, puts the arguments on the interpreter's stacks and calls the interpreter. The
/// nearly all scripts the client runs (widget events, timers, the per frame ones) come through it.
///
/// `run(client, event, max_operations, flags)`: the event's type is at `+event_type`, its argument count at `+arg_count` and a
/// pointer to its arguments (variants of `ARG_SIZE` bytes whose kind byte is at `ARG_KIND`, with the int, if it is one, at the
/// start) at `+args`. For ordinary events (not the component and menu ones, which find the script by trigger) argument 0 is the
/// script id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptRuns {
    /// RVA of the function
    pub function: u32,
    pub event_type: u32,
    pub args: u32,
    pub arg_count: u32,
}

/// The size of one argument variant, the offset of its kind byte (1 int, 2 string, 3 and 5 other) and the kind of an int. No code
/// shape names them (the function indexes the vector with them): they were checked against the argument reader on the builds the
/// rule was written for.
pub const ARG_SIZE: u32 = 0x28;
pub const ARG_KIND: u32 = 0x20;
pub const KIND_INT: u8 = 1;

fn imm(i: &Instruction) -> Option<u64> {
    matches!(i.op1_kind(), OpKind::Immediate8 | OpKind::Immediate8to32 | OpKind::Immediate8to64 | OpKind::Immediate32 | OpKind::Immediate32to64).then(|| i.immediate(1))
}

// The function branches on the event type: `lea eax, [type - 0xa]; cmp eax, 7` (component events 10..=17), then compares it with
// 0x12, 0x55 and 0x56.
fn is_event_runner(instrs: &[Instruction]) -> bool {
    let compares = |value: u64| instrs.iter().any(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Register && i.op0_register().size() == 4 && imm(i) == Some(value));
    let range = instrs.windows(2).any(|w| {
        w[0].mnemonic() == Mnemonic::Lea && w[0].memory_displacement64() as i64 == -0xa && w[1].mnemonic() == Mnemonic::Cmp && w[1].op0_register() == w[0].op0_register() && imm(&w[1]) == Some(7)
    });
    range && compares(0x12) && compares(0x55) && compares(0x56)
}

fn fields(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
    // `event` arrives in rdx and is copied to a callee saved register that the argument loop uses
    let copies = instrs.iter().take(24).filter(|i| i.mnemonic() == Mnemonic::Mov && i.op0_kind() == OpKind::Register && i.op0_register().size() == 8 && i.op1_register() == Register::RDX).map(|i| i.op0_register());
    let event: Vec<Register> = core::iter::once(Register::RDX).chain(copies).collect();
    let loads = |size: usize| instrs.iter().take(24).filter(move |i| i.op0_kind() == OpKind::Register && i.op0_register().size() == size).filter_map(|i| event.iter().find_map(|&base| load_from(i, base)));
    let event_type = loads(4).next()?;
    let args = loads(8).next()?;
    let count = one(
        instrs
            .iter()
            .filter(|i| i.mnemonic() == Mnemonic::Cmp && i.op0_kind() == OpKind::Memory && i.memory_size() == iced_x86::MemorySize::UInt32 && event.contains(&i.memory_base()) && i.op1_kind() == OpKind::Register)
            .map(|i| i.memory_displacement64() as u32)
            .collect(),
    )
    .ok()?;
    Some((event_type, args, count))
}

pub fn find_script_runs(image: &[u8]) -> Result<ScriptRuns, ScanError> {
    let pe = Pe::parse(image).ok_or(ScanError::InvalidImage)?;
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if !(0x200..=0x800).contains(&(end - begin)) {
            continue;
        }
        let instrs = whole_function(&pe, begin, end - begin);
        if !is_event_runner(&instrs) {
            continue;
        }
        if let Some((event_type, args, arg_count)) = fields(&instrs) {
            found.push(ScriptRuns { function: begin, event_type, args, arg_count });
        }
    }
    one(found)
}
