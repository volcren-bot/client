use std::{io, ptr::null_mut, sync::Mutex, thread::sleep, time::Duration};

use ipc::input;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{
        Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC, MapVirtualKeyW, VkKeyScanW},
        WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetWindowThreadProcessId, IsWindow, PostMessageW, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
            WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
        },
    },
};

use crate::{
    Input, MouseButton,
    async_input::{Blocking, Pace},
    win::{canvas_offset, class_is, connection},
    wind_mouse,
};

const MK_LBUTTON: WPARAM = 0x1;
const MK_RBUTTON: WPARAM = 0x2;
const MK_MBUTTON: WPARAM = 0x10;

const WHEEL_DELTA: i32 = 120;

const BETWEEN_KEYS: Duration = Duration::from_millis(40);

pub fn open_console(pid: u32) -> Result<(), String> {
    let window = render_view(pid).ok_or("The client's JagRenderView window wasn't found.")?;
    post(window, input::OPEN_CONSOLE_MESSAGE, 0, 0)
}

pub fn post_request(pid: u32, message: u32) -> Result<(), String> {
    let window = render_view(pid).ok_or("The client's JagRenderView window wasn't found.")?;
    post(window, message, 0, 0)
}

pub fn send(pid: u32, input: &Input) -> Result<(), String> {
    send_with(pid, input, &Blocking)
}

// Sleeps go through `pace`, so a cancelled input stops early. A pressed button or key is still released.
pub fn send_with(pid: u32, input: &Input, pace: &dyn Pace) -> Result<(), String> {
    if pace.cancelled() {
        return Ok(());
    }
    let window = render_view(pid).ok_or("The client's JagRenderView window wasn't found.")?;
    match input {
        Input::Move { x, y } => {
            let (ox, oy) = canvas_offset(pid);
            post(window, WM_MOUSEMOVE, 0, point(x + ox, y + oy))
        }
        Input::WindMouse { x, y, options } => {
            let (ox, oy) = canvas_offset(pid);
            let start = connection(pid)
                .filter(|c| c.is_compatible() && c.mouse_x > -1)
                .map(|c| (c.mouse_x as f64, c.mouse_y as f64));
            let post_move = |px: i32, py: i32| post(window, WM_MOUSEMOVE, 0, point(px + ox, py + oy));
            wind_mouse::run_with(start, (*x, *y), options, post_move, |duration| pace.sleep(duration))
        }
        Input::Click { button, x, y, hold_ms } => {
            let (down, up, held) = button_messages(*button);
            let (ox, oy) = canvas_offset(pid);
            let at = point(x + ox, y + oy);
            post(window, WM_MOUSEMOVE, 0, at)?;
            post(window, down, held, at)?;
            pace.sleep(Duration::from_millis(*hold_ms as u64));
            post(window, up, 0, at)
        }
        Input::Button { button, down } => {
            let (x, y) = mouse_at(pid)?;
            let (down_message, up_message, held) = button_messages(*button);
            match down {
                true => post(window, down_message, held, point(x, y)),
                false => post(window, up_message, 0, point(x, y)),
            }
        }
        Input::Scroll { notches } => {
            let (x, y) = mouse_at(pid)?;
            let mut at = POINT { x, y };
            if unsafe { ClientToScreen(window, &mut at) } == 0 {
                return Err(format!("ClientToScreen failed: {}", io::Error::last_os_error()));
            }
            let delta = (-notches.saturating_mul(WHEEL_DELTA)).clamp(i16::MIN as i32, i16::MAX as i32);
            post(window, WM_MOUSEWHEEL, ((delta as u16 as u32) << 16) as WPARAM, point(at.x, at.y))
        }
        Input::Key { key, down } => match modifier(*key) {
            Some(bit) => post(window, input::MODIFIERS_MESSAGE, bit, 0),
            None => {
                let message = if *down { WM_KEYDOWN } else { WM_KEYUP };
                post(window, message, *key as WPARAM, key_lparam(*key as u32, *down))
            }
        },
        Input::Type { text, hold_ms } => type_text(window, text, Duration::from_millis(*hold_ms as u64), pace),
    }
}

fn button_messages(button: MouseButton) -> (u32, u32, WPARAM) {
    match button {
        MouseButton::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP, MK_LBUTTON),
        MouseButton::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP, MK_RBUTTON),
        MouseButton::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP, MK_MBUTTON),
    }
}

fn mouse_at(pid: u32) -> Result<(i32, i32), String> {
    let (ox, oy) = canvas_offset(pid);
    for _ in 0..25 {
        if let Some(c) = connection(pid).filter(|c| c.is_compatible() && c.mouse_x > -1) {
            return Ok((c.mouse_x + ox, c.mouse_y + oy));
        }
        sleep(Duration::from_millis(20));
    }
    Err("agent hasn't seen the mouse yet, move it first.".to_string())
}

fn modifier(key: u8) -> Option<WPARAM> {
    match key {
        0x10 | 0xa0 | 0xa1 => Some(input::SHIFT),
        0x11 | 0xa2 | 0xa3 => Some(input::CTRL),
        0x12 | 0xa4 | 0xa5 => Some(input::ALT),
        _ => None,
    }
}

fn key_lparam(vk: u32, down: bool) -> LPARAM {
    let scancode = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) } as isize;
    let extended = matches!(vk, 0x21..=0x28 | 0x2c..=0x2e | 0x5b..=0x5d | 0x6f | 0x90) as isize;
    let lparam = 1 | scancode << 16 | extended << 24;
    if down { lparam } else { lparam | 1 << 30 | 1 << 31 }
}

fn post(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<(), String> {
    let wparam = if (input::MOUSE_FIRST..=input::MOUSE_LAST).contains(&message) { wparam | input::SYNTHETIC as WPARAM } else { wparam };
    if unsafe { PostMessageW(window, message, wparam, lparam) } == 0 {
        return Err(format!("PostMessageW failed: {}", io::Error::last_os_error()));
    }
    Ok(())
}

fn point(x: i32, y: i32) -> LPARAM {
    ((y as u16 as u32) << 16 | x as u16 as u32) as LPARAM
}

fn type_text(window: HWND, text: &str, hold: Duration, pace: &dyn Pace) -> Result<(), String> {
    let keys = text
        .encode_utf16()
        .map(|unit| match unsafe { VkKeyScanW(unit) } {
            -1 => Err(format!("{:?} can't be typed with this keyboard layout.", char::from_u32(unit as u32).unwrap_or('?'))),
            key => Ok(key as u16),
        })
        .collect::<Result<Vec<u16>, String>>()?;

    let mut held = 0;
    for (i, key) in keys.into_iter().enumerate() {
        if i > 0 && !pace.sleep(BETWEEN_KEYS) {
            break;
        }
        let vk = (key & 0xff) as u32;
        let shift_state = (key >> 8) as usize;
        let wanted = (if shift_state & 1 != 0 { input::SHIFT } else { 0 })
            | (if shift_state & 2 != 0 { input::CTRL } else { 0 })
            | (if shift_state & 4 != 0 { input::ALT } else { 0 });
        if wanted != held {
            post(window, input::MODIFIERS_MESSAGE, wanted ^ held, 0)?;
            held = wanted;
        }

        post(window, WM_KEYDOWN, vk as WPARAM, key_lparam(vk, true))?;
        pace.sleep(hold);
        post(window, WM_KEYUP, vk as WPARAM, key_lparam(vk, false))?;
    }
    if held != 0 {
        post(window, input::MODIFIERS_MESSAGE, held, 0)?;
    }
    Ok(())
}

// Finding the window walks every top-level window, so the answer is kept until the window is gone.
static RENDER_VIEWS: Mutex<Vec<(u32, usize)>> = Mutex::new(Vec::new());

fn render_view(pid: u32) -> Option<HWND> {
    let mut known = RENDER_VIEWS.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|&(p, window)| p != pid || unsafe { IsWindow(window as HWND) } != 0);
    if let Some(&(_, window)) = known.iter().find(|&&(p, _)| p == pid) {
        return Some(window as HWND);
    }
    let window = find_render_view(pid)?;
    known.push((pid, window as usize));
    Some(window)
}

fn find_render_view(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn child(hwnd: HWND, lparam: LPARAM) -> i32 {
        if class_is(hwnd, "JagRenderView") {
            unsafe { *(lparam as *mut HWND) = hwnd };
            return 0;
        }
        1
    }

    unsafe extern "system" fn top(hwnd: HWND, lparam: LPARAM) -> i32 {
        let (pid, found) = unsafe { &mut *(lparam as *mut (u32, HWND)) };
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        if owner != *pid || !class_is(hwnd, "JagWindow") {
            return 1;
        }
        unsafe { EnumChildWindows(hwnd, Some(child), found as *mut HWND as LPARAM) };
        if found.is_null() { 1 } else { 0 }
    }

    let mut search: (u32, HWND) = (pid, null_mut());
    unsafe { EnumWindows(Some(top), &mut search as *mut _ as LPARAM) };
    (!search.1.is_null()).then_some(search.1)
}
