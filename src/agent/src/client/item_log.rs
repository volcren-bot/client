use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::OnceLock,
};

use decode::world::{item_slot, Item};
use ipc::{
    item_log::{item_log_memory_name, item_wake_event_name, Change, Log, LOG_SIZE, NO_TICK},
    state::WorldLayout,
};
use retour::GenericDetour;

use super::{
    detour,
    log::{wake, SharedLog, Tick},
    offsets::ItemContainers,
    own::OwnMemory,
    unload::HookCall,
};

type SetSlot = unsafe extern "system" fn(i32, i32, i32, i32);

static SET_SLOT: OnceLock<GenericDetour<SetSlot>> = OnceLock::new();
static CONTAINERS: OnceLock<WorldLayout> = OnceLock::new();
static TICK: Tick = Tick::new();
static LOG: SharedLog<Log> = SharedLog::new();

pub unsafe fn close() {
    LOG.close();
}

fn log(change: &Change) {
    let Some(log) = LOG.get() else { return };
    unsafe {
        ipc::item_log::push(log, LOG.elapsed().as_millis() as u64, change);
        wake(
            ipc::item_log::subscribers(log),
            item_wake_event_name,
            |slot| ipc::item_log::unsubscribe(log, slot),
        );
    }
}

unsafe extern "system" fn set_slot(container: i32, slot: i32, item: i32, quantity: i32) {
    let _call = HookCall::enter();
    let detour = SET_SLOT.get().unwrap();
    let old = catch_unwind(AssertUnwindSafe(|| {
        CONTAINERS
            .get()
            .map(|layout| item_slot(&OwnMemory, layout, container, slot))
    }))
    .ok()
    .flatten();
    detour.call(container, slot, item, quantity);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(old) = old else { return };
        if old.is_some_and(|old| old.id == item && old.quantity == quantity) {
            return;
        }
        let old_known = old.is_some();
        let old = old.unwrap_or(Item {
            slot,
            id: -1,
            quantity: 0,
        });
        log(&Change {
            tick: TICK.read().unwrap_or(NO_TICK),
            container,
            slot,
            old_known,
            old_item: old.id,
            old_quantity: old.quantity,
            new_item: item,
            new_quantity: quantity,
        });
    }));
}

pub unsafe fn hook(
    module_base: u64,
    rule: u32,
    containers: ItemContainers,
    tick: Option<(u32, u32)>,
) {
    if SET_SLOT.get().is_some()
        || !LOG.open(item_log_memory_name, LOG_SIZE, "item", ipc::item_log::init)
    {
        return;
    }
    let _ = CONTAINERS.set(WorldLayout {
        module_base,
        containers: containers.table,
        container_ids: containers.ids,
        container_quantities: containers.quantities,
        container_next: containers.next,
        ..Default::default()
    });
    TICK.set(module_base, tick);
    let Some(created) =
        detour::create::<SetSlot>(module_base + rule as u64, set_slot, "item container writes")
    else {
        return;
    };
    detour::enable(
        SET_SLOT.get_or_init(|| created),
        "item hook",
        "Item log hooked",
    );
}

pub unsafe fn unhook() {
    detour::disable(SET_SLOT.get(), "item hook", Some("Item log unhooked"));
}
