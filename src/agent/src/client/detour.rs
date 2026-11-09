use retour::{Function, GenericDetour};

pub unsafe fn function<T: Function>(address: u64) -> T {
    T::from_ptr(address as *const ())
}

pub unsafe fn create<T: Function>(address: u64, detour: T, what: &str) -> Option<GenericDetour<T>> {
    GenericDetour::new(function::<T>(address), detour)
        .map_err(|error| println!("[Agent]: Failed to hook the {what}: {error}.\r\n"))
        .ok()
}

pub unsafe fn enable<T: Function>(detour: &GenericDetour<T>, hook: &str, done: &str) {
    match detour.enable() {
        Ok(()) => println!("[Agent]: {done}.\r\n"),
        Err(error) => println!("[Agent]: Failed to enable the {hook}: {error}.\r\n"),
    }
}

pub unsafe fn disable<T: Function>(
    detour: Option<&GenericDetour<T>>,
    hook: &str,
    done: Option<&str>,
) {
    let Some(detour) = detour.filter(|detour| detour.is_enabled()) else {
        return;
    };
    match (detour.disable(), done) {
        (Ok(()), Some(done)) => println!("[Agent]: {done}.\r\n"),
        (Ok(()), None) => {}
        (Err(error), _) => println!("[Agent]: Failed to disable the {hook}: {error}.\r\n"),
    }
}
