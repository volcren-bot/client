use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScriptRuns {
    pub function: u32,
    pub event_type: u32,
    pub args: u32,
    pub arg_count: u32,
}

pub const ARG_SIZE: u32 = 0x28;
pub const ARG_KIND: u32 = 0x20;
pub const KIND_INT: u8 = 1;

fn is_event_runner(instrs: &[Instruction]) -> bool {
    let compares = |value: u64| {
        instrs.iter().any(|i| {
            i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 4
                && immediate(i) == Some(value)
        })
    };
    let range = instrs.windows(2).any(|w| {
        w[0].mnemonic() == Mnemonic::Lea
            && w[0].memory_displacement64() as i64 == -0xa
            && w[1].mnemonic() == Mnemonic::Cmp
            && w[1].op0_register() == w[0].op0_register()
            && immediate(&w[1]) == Some(7)
    });
    range && compares(0x12) && compares(0x55) && compares(0x56)
}

fn fields(instrs: &[Instruction]) -> Option<(u32, u32, u32)> {
    let copies = instrs
        .iter()
        .take(24)
        .filter(|i| {
            i.mnemonic() == Mnemonic::Mov
                && i.op0_kind() == OpKind::Register
                && i.op0_register().size() == 8
                && i.op1_register() == Register::RDX
        })
        .map(|i| i.op0_register());
    let event: Vec<Register> = core::iter::once(Register::RDX).chain(copies).collect();
    let loads = |size: usize| {
        instrs
            .iter()
            .take(24)
            .filter(move |i| i.op0_kind() == OpKind::Register && i.op0_register().size() == size)
            .filter_map(|i| event.iter().find_map(|&base| load_from(i, base)))
    };
    let event_type = loads(4).next()?;
    let args = loads(8).next()?;
    let count = one(instrs
        .iter()
        .filter(|i| {
            i.mnemonic() == Mnemonic::Cmp
                && i.op0_kind() == OpKind::Memory
                && i.memory_size() == iced_x86::MemorySize::UInt32
                && event.contains(&i.memory_base())
                && i.op1_kind() == OpKind::Register
        })
        .map(|i| i.memory_displacement64() as u32)
        .collect())
    .ok()?;
    Some((event_type, args, count))
}

pub fn find_script_runs(pe: &Pe) -> Result<ScriptRuns, ScanError> {
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        if !(0x200..=0x800).contains(&(end - begin)) {
            continue;
        }
        let instrs = whole_function(pe, begin, end - begin);
        if !is_event_runner(&instrs) {
            continue;
        }
        if let Some((event_type, args, arg_count)) = fields(&instrs) {
            found.push(ScriptRuns {
                function: begin,
                event_type,
                args,
                arg_count,
            });
        }
    }
    one(found)
}
