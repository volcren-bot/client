use super::{model::whole_function, values::ItemContainers, *};

/// The function that writes one item container slot: `set_slot(container: ecx, slot: edx, item: r8d, quantity: r9d)`, found by the container table globals it reads plus its resize calls and two stores.
const MAX_SIZE: u32 = 0x200;

pub fn find_item_setter(pe: &Pe, containers: &ItemContainers) -> Result<u32, ScanError> {
    let (buckets, count) = (containers.table + hash_table::BUCKETS as u32, containers.table + hash_table::BUCKET_COUNT as u32);
    let mut found = Vec::new();
    for &(begin, end) in &pe.functions {
// The unwind table splits the function in two chunks, so read on from the start up to the padding.
        let reads = |instrs: &[Instruction], global: u32| instrs.iter().any(|i| i.is_ip_rel_memory_operand() && i.ip_rel_memory_address() as u32 == global);
        let head = whole_function(pe, begin, end - begin);
        if !reads(&head, buckets) || !reads(&head, count) {
            continue;
        }
        let instrs = whole_function(pe, begin, MAX_SIZE);
        let mut calls: HashMap<u32, usize> = HashMap::new();
        for i in instrs.iter().filter(|i| i.mnemonic() == Mnemonic::Call && i.op0_kind() == OpKind::NearBranch64) {
            *calls.entry(i.near_branch_target() as u32).or_default() += 1;
        }
        // the resize, called for the item vector and again for the quantity vector (the destructor of a temporary is called twice too)
        let resizes = calls.iter().filter(|&(_, &n)| n == 2).count() >= 1;
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
