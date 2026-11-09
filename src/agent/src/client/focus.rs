use std::{
    ffi::{c_void, CStr},
    sync::atomic::{AtomicUsize, Ordering},
};

use windows::Win32::{
    Foundation::HWND,
    System::{
        LibraryLoader::GetModuleHandleW,
        Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE},
    },
};

type GetFocus = unsafe extern "system" fn() -> HWND;

static SLOT: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn focused() -> HWND {
    let window = super::hooks::hooked_window();
    if window != 0 {
        return HWND(window as *mut c_void);
    }
    match ORIGINAL.load(Ordering::SeqCst) {
        0 => HWND(std::ptr::null_mut()),
        original => unsafe { std::mem::transmute::<usize, GetFocus>(original)() },
    }
}

unsafe fn import_slot(dll: &str, function: &str) -> Option<usize> {
    unsafe {
        let base = GetModuleHandleW(None).ok()?.0 as usize;
        let read32 = |at: usize| (at as *const u32).read_unaligned() as usize;
        let nt = base + read32(base + 0x3c);
        let imports = read32(nt + 0x18 + 112 + 8);
        if imports == 0 {
            return None;
        }
        let mut descriptor = base + imports;
        loop {
            let names = read32(descriptor);
            let name = read32(descriptor + 12);
            let slots = read32(descriptor + 16);
            if name == 0 {
                return None;
            }
            let module = CStr::from_ptr((base + name) as *const _).to_string_lossy();
            if names != 0 && module.eq_ignore_ascii_case(dll) {
                let mut i = 0;
                loop {
                    let thunk = ((base + names + i * 8) as *const u64).read_unaligned();
                    if thunk == 0 {
                        break;
                    }
                    if thunk >> 63 == 0 {
                        let named = CStr::from_ptr((base + thunk as usize + 2) as *const _);
                        if named.to_bytes() == function.as_bytes() {
                            return Some(base + slots + i * 8);
                        }
                    }
                    i += 1;
                }
            }
            descriptor += 20;
        }
    }
}

unsafe fn write_slot(slot: usize, value: usize) -> bool {
    unsafe {
        let mut old = PAGE_PROTECTION_FLAGS(0);
        if VirtualProtect(slot as *const c_void, 8, PAGE_READWRITE, &mut old).is_err() {
            return false;
        }
        (slot as *mut usize).write_volatile(value);
        let mut ignored = PAGE_PROTECTION_FLAGS(0);
        let _ = VirtualProtect(slot as *const c_void, 8, old, &mut ignored);
        true
    }
}

pub unsafe fn hook() {
    if SLOT.load(Ordering::SeqCst) != 0 {
        return;
    }
    unsafe {
        let Some(slot) = import_slot("USER32.dll", "GetFocus") else {
            println!("[Agent]: GetFocus import not found, keys need the client focused.\r\n");
            return;
        };
        let original = (slot as *const usize).read_volatile();
        ORIGINAL.store(original, Ordering::SeqCst);
        if write_slot(slot, focused as *const () as usize) {
            SLOT.store(slot, Ordering::SeqCst);
            println!("[Agent]: GetFocus import hooked.\r\n");
        }
    }
}

pub unsafe fn unhook() {
    let slot = SLOT.swap(0, Ordering::SeqCst);
    if slot != 0 {
        unsafe {
            write_slot(slot, ORIGINAL.load(Ordering::SeqCst));
        }
    }
}
