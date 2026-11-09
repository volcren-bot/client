use windows::{
    core::BOOL,
    Win32::{
        Foundation::{FALSE, HWND, LPARAM, TRUE},
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetClassNameW, GetWindowThreadProcessId,
        },
    },
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn class_is(hwnd: HWND, name: &str) -> bool {
    let mut class_name = [0u16; 256];
    let len = GetClassNameW(hwnd, &mut class_name).max(0) as usize;
    name.encode_utf16().eq(class_name[..len].iter().copied())
}

pub fn get_jagrenderview(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn enum_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if class_is(hwnd, "JagRenderView") {
            *(lparam.0 as *mut Option<HWND>) = Some(hwnd);
            return FALSE;
        }
        TRUE
    }

    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut (u32, Option<HWND>));
        let mut proc_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut proc_id));
        if proc_id == search.0 && class_is(hwnd, "JagWindow") {
            let found = LPARAM(&mut search.1 as *mut Option<HWND> as isize);
            let _ = EnumChildWindows(Some(hwnd), Some(enum_child_proc), found);
            if search.1.is_some() {
                return FALSE;
            }
        }
        TRUE
    }

    let mut search = (pid, None);
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut search as *mut _ as isize),
        );
    }
    search.1
}
