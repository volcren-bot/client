use windows::{
    core::BOOL,
    Win32::{
        Foundation::{FALSE, HWND, LPARAM, TRUE},
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetClassNameW, GetWindowThreadProcessId,
        },
    },
};

pub const WI_CONSOLE: u32 = ipc::input::OPEN_CONSOLE_MESSAGE;
pub const WI_MODIFIERS: u32 = ipc::input::MODIFIERS_MESSAGE;
pub const WI_NAME: u32 = ipc::input::NAME_MESSAGE;
pub const WI_TOTAL_XP: u32 = ipc::input::TOTAL_XP_MESSAGE;
pub const WI_ITEM_NAMES: u32 = ipc::input::ITEM_NAMES_MESSAGE;
pub const WI_GAME_VALS: u32 = ipc::input::GAME_VALS_MESSAGE;
pub const WI_PROJECT: u32 = ipc::input::PROJECT_MESSAGE;
pub const WI_MENU: u32 = ipc::input::MENU_MESSAGE;
pub const WI_MODEL: u32 = ipc::input::MODEL_MESSAGE;
pub const WI_LOCS: u32 = ipc::input::LOCS_MESSAGE;
pub const WI_CACHE: u32 = ipc::input::CACHE_MESSAGE;

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
