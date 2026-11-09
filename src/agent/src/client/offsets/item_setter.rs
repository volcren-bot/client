use super::{values::ItemContainers, *};

const MAX_SIZE: u32 = 0x200;

pub fn find_item_setter(pe: &Pe, containers: &ItemContainers) -> Result<u32, ScanError> {
    let (buckets, count) = (
        containers.table + hash_table::BUCKETS as u32,
        containers.table + hash_table::BUCKET_COUNT as u32,
    );
    let reads = |instrs: &[Instruction], global: u32| {
        instrs
            .iter()
            .any(|i| i.is_ip_rel_memory_operand() && i.ip_rel_memory_address() as u32 == global)
    };
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
        let head = whole_function(pe, begin, end - begin);
        if !reads(&head, buckets) || !reads(&head, count) {
            continue;
        }
        let instrs = whole_function(pe, begin, MAX_SIZE);
        let mut calls: HashMap<u32, usize> = HashMap::new();
        for target in instrs.iter().filter_map(call_target) {
            *calls.entry(target).or_default() += 1;
        }
        let resizes = calls.values().any(|&n| n == 2);
        let indexed_stores = instrs
            .iter()
            .filter(|i| {
                i.mnemonic() == Mnemonic::Mov
                    && i.op0_kind() == OpKind::Memory
                    && i.memory_index() != Register::None
                    && i.memory_index_scale() == 1
                    && i.op1_kind() == OpKind::Register
                    && i.op1_register().is_gpr32()
            })
            .count();
        if resizes && indexed_stores == 2 {
            found.push(begin);
        }
    }
    one(found)
}
