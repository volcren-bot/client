use std::{
    io,
    ptr::null_mut,
    sync::{Mutex, PoisonError},
    thread::sleep,
    time::Duration,
};

use ipc::input;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    UI::{
        Input::KeyboardAndMouse::{MAPVK_VK_TO_VSC, MapVirtualKeyW, VkKeyScanW},
        WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetWindowThreadProcessId, IsWindow, PostMessageW,
            WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
        },
    },
};

use crate::{
    Act, ActResult, Input, MouseButton,
    async_input::{Blocking, Pace},
    win::{act_send, act_state, canvas_offset, class_is, connection, menu_entry},
    wind_mouse::{self, Follow},
};
use ipc::state::{ActState, act_command, act_status};

const MK_LBUTTON: WPARAM = 0x1;
const MK_RBUTTON: WPARAM = 0x2;
const MK_MBUTTON: WPARAM = 0x10;

const WHEEL_DELTA: i32 = 120;

const BETWEEN_KEYS: Duration = Duration::from_millis(40);

pub fn open_console(pid: u32) -> Result<(), String> {
    post_request(pid, input::OPEN_CONSOLE_MESSAGE)
}

pub fn post_request(pid: u32, message: u32) -> Result<(), String> {
    post(window(pid)?, message, 0, 0)
}

fn window(pid: u32) -> Result<HWND, String> {
    render_view(pid).ok_or_else(|| "The client's JagRenderView window wasn't found.".into())
}

pub fn send(pid: u32, input: &Input) -> Result<(), String> {
    send_with(pid, input, &Blocking)
}

pub fn send_with(pid: u32, input: &Input, pace: &dyn Pace) -> Result<(), String> {
    if pace.cancelled() {
        return Ok(());
    }
    let window = window(pid)?;
    match input {
        Input::Move { x, y } => {
            let (ox, oy) = canvas_offset(pid);
            post(window, WM_MOUSEMOVE, 0, point(x + ox, y + oy))
        }
        Input::WindMouse { x, y, options } => {
            let (ox, oy) = canvas_offset(pid);
            let start = seen_mouse(pid).map(|(x, y)| (f64::from(x), f64::from(y)));
            let post_move =
                |px: i32, py: i32| post(window, WM_MOUSEMOVE, 0, point(px + ox, py + oy));
            wind_mouse::run_with(start, (*x, *y), options, post_move, |duration| {
                pace.sleep(duration)
            })
        }
        Input::Click {
            button,
            x,
            y,
            hold_ms,
        } => {
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
            let (message, flags) = if *down {
                (down_message, held)
            } else {
                (up_message, 0)
            };
            post(window, message, flags, point(x, y))
        }
        Input::Scroll { notches } => {
            let (x, y) = mouse_at(pid)?;
            let mut at = POINT { x, y };
            if unsafe { ClientToScreen(window, &mut at) } == 0 {
                return Err(format!(
                    "ClientToScreen failed: {}",
                    io::Error::last_os_error()
                ));
            }
            let delta = notches
                .saturating_mul(-WHEEL_DELTA)
                .clamp(i16::MIN as i32, i16::MAX as i32);
            post(
                window,
                WM_MOUSEWHEEL,
                ((delta as u16 as u32) << 16) as WPARAM,
                point(at.x, at.y),
            )
        }
        Input::Key { key, down } => match modifier(*key) {
            Some(bit) => post(window, input::MODIFIERS_MESSAGE, bit, 0),
            None => {
                let message = if *down { WM_KEYDOWN } else { WM_KEYUP };
                post(
                    window,
                    message,
                    *key as WPARAM,
                    key_lparam(*key as u32, *down),
                )
            }
        },
        Input::Type { text, hold_ms } => {
            type_text(window, text, Duration::from_millis(*hold_ms as u64), pace)
        }
    }
}

const ACT_POLL: Duration = Duration::from_millis(2);
const ACT_GRACE_MS: u128 = 500;
const ACT_SETTLED_PX: f64 = 1.0;

fn act_status_name(status: i32) -> &'static str {
    match status {
        act_status::LEFT => "left",
        act_status::RIGHT => "right",
        act_status::NO_OPTION => "no_option",
        act_status::GONE => "gone",
        act_status::MENU_OPEN => "menu_open",
        act_status::TIMEOUT => "timeout",
        act_status::CANCELLED => "cancelled",
        act_status::OTHER_THREAD => "other_thread",
        act_status::NOT_IN_GAME => "not_in_game",
        act_status::NOT_FOUND => "not_found",
        _ => "lost",
    }
}

pub fn act(pid: u32, act: &Act, pace: &dyn Pace) -> Result<ActResult, String> {
    let window = window(pid)?;
    let request = ipc::state::ActRequest {
        command: act_command::ARM,
        ..act.request
    };
    let (sequence, _) = act_send(pid, &request)?;
    let outcome = follow(pid, window, act, sequence, pace);
    let finished = outcome
        .as_ref()
        .is_ok_and(|state| state.status != act_status::TRACKING);
    if !finished {
        let cancel = ipc::state::ActRequest {
            command: act_command::CANCEL,
            ..act.request
        };
        act_send(pid, &cancel)?;
    }
    let state = outcome?;
    if matches!(state.status, act_status::LEFT | act_status::RIGHT) {
        let button = if state.status == act_status::LEFT {
            MouseButton::Left
        } else {
            MouseButton::Right
        };
        Blocking.sleep(Duration::from_millis(act.hold_ms as u64));
        let (_, up, _) = button_messages(button);
        let (x, y) = mouse_at(pid)?;
        post(window, up, 0, point(x, y))?;
    }
    let status = if finished {
        act_status_name(state.status)
    } else if pace.cancelled() {
        "cancelled"
    } else {
        "timeout"
    };
    Ok(ActResult {
        status,
        x: state.mouse[0],
        y: state.mouse[1],
        entry: matches!(state.status, act_status::LEFT | act_status::RIGHT)
            .then(|| menu_entry(&state.entry)),
    })
}

fn current(pid: u32, sequence: u32) -> Result<Option<ActState>, String> {
    let state = act_state(pid)?;
    Ok((state.request == sequence).then_some(state))
}

const ACT_LEAD_MS: f64 = 30.0;
const ACT_PURSUIT_PX: f64 = 12.0;
const ACT_PURSUIT_GAP_MS: (f64, f64) = (6.0, 10.0);
const ACT_SPEED_SMOOTHING: f64 = 0.5;

struct Tracker {
    last: Option<(u32, [f32; 2], std::time::Instant)>,
    velocity: (f64, f64),
}

impl Tracker {
    fn observe(&mut self, state: &ActState) {
        if state.has_spot == 0 {
            return;
        }
        let now = std::time::Instant::now();
        if let Some((frame, spot, at)) = self.last {
            if frame == state.frame {
                return;
            }
            let ms = now.duration_since(at).as_secs_f64() * 1000.0;
            if ms > 0.0 {
                let vx = f64::from(state.spot[0] - spot[0]) / ms;
                let vy = f64::from(state.spot[1] - spot[1]) / ms;
                self.velocity.0 += (vx - self.velocity.0) * ACT_SPEED_SMOOTHING;
                self.velocity.1 += (vy - self.velocity.1) * ACT_SPEED_SMOOTHING;
            }
        }
        self.last = Some((state.frame, state.spot, now));
    }

    fn goal(&self, state: &ActState) -> (f64, f64) {
        (
            f64::from(state.spot[0]) + self.velocity.0 * ACT_LEAD_MS,
            f64::from(state.spot[1]) + self.velocity.1 * ACT_LEAD_MS,
        )
    }
}

fn follow(
    pid: u32,
    window: HWND,
    act: &Act,
    sequence: u32,
    pace: &dyn Pace,
) -> Result<ActState, String> {
    let started = std::time::Instant::now();
    let (ox, oy) = canvas_offset(pid);
    let mut tracker = Tracker {
        last: None,
        velocity: (0.0, 0.0),
    };
    let mut rng = wind_mouse::Rng::seeded();
    loop {
        let Some(state) = current(pid, sequence)? else {
            return Err("another act replaced this one".into());
        };
        if state.status != act_status::TRACKING
            || pace.cancelled()
            || started.elapsed().as_millis() > act.request.timeout_ms as u128 + ACT_GRACE_MS
        {
            return Ok(state);
        }
        tracker.observe(&state);
        let start = seen_mouse(pid).map(|(x, y)| (f64::from(x), f64::from(y)));
        let goal = tracker.goal(&state);
        let away = start.map_or(f64::INFINITY, |(x, y)| (x - goal.0).hypot(y - goal.1));
        let Some(start) = start.filter(|_| state.has_spot != 0) else {
            if !pace.sleep(ACT_POLL) {
                return Ok(state);
            }
            continue;
        };
        if away <= ACT_PURSUIT_PX {
            let (x, y) = (goal.0.round() as i32, goal.1.round() as i32);
            if away > ACT_SETTLED_PX {
                post(window, WM_MOUSEMOVE, 0, point(x + ox, y + oy))?;
            }
            let gap =
                ACT_PURSUIT_GAP_MS.0 + rng.random() * (ACT_PURSUIT_GAP_MS.1 - ACT_PURSUIT_GAP_MS.0);
            if !pace.sleep(Duration::from_secs_f64(gap / 1000.0)) {
                return Ok(state);
            }
            continue;
        }
        wind_mouse::follow_with(
            start,
            (goal.0.round() as i32, goal.1.round() as i32),
            &act.mouse,
            |px, py| post(window, WM_MOUSEMOVE, 0, point(px + ox, py + oy)),
            |duration| pace.sleep(duration),
            || match current(pid, sequence) {
                Ok(Some(s)) if s.status == act_status::TRACKING && s.has_spot != 0 => {
                    tracker.observe(&s);
                    let goal = tracker.goal(&s);
                    if start_distance(goal, pid) <= ACT_PURSUIT_PX / 2.0 {
                        Follow::Stop
                    } else {
                        Follow::To(goal.0, goal.1)
                    }
                }
                Ok(Some(s)) if s.status == act_status::TRACKING => Follow::Stay,
                _ => Follow::Stop,
            },
        )
        .or_else(|why| {
            if why.starts_with("wind_mouse timed out") {
                Ok(())
            } else {
                Err(why)
            }
        })?;
    }
}

fn start_distance(goal: (f64, f64), pid: u32) -> f64 {
    seen_mouse(pid).map_or(f64::INFINITY, |(x, y)| {
        (f64::from(x) - goal.0).hypot(f64::from(y) - goal.1)
    })
}

fn button_messages(button: MouseButton) -> (u32, u32, WPARAM) {
    match button {
        MouseButton::Left => (WM_LBUTTONDOWN, WM_LBUTTONUP, MK_LBUTTON),
        MouseButton::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP, MK_RBUTTON),
        MouseButton::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP, MK_MBUTTON),
    }
}

fn seen_mouse(pid: u32) -> Option<(i32, i32)> {
    connection(pid)
        .filter(|c| c.is_compatible() && c.mouse_x > -1)
        .map(|c| (c.mouse_x, c.mouse_y))
}

fn mouse_at(pid: u32) -> Result<(i32, i32), String> {
    let (ox, oy) = canvas_offset(pid);
    for _ in 0..25 {
        if let Some((x, y)) = seen_mouse(pid) {
            return Ok((x + ox, y + oy));
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
    if down {
        lparam
    } else {
        lparam | 1 << 30 | 1 << 31
    }
}

fn post(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Result<(), String> {
    let wparam = if (input::MOUSE_FIRST..=input::MOUSE_LAST).contains(&message) {
        wparam | input::SYNTHETIC as WPARAM
    } else {
        wparam
    };
    if unsafe { PostMessageW(window, message, wparam, lparam) } == 0 {
        return Err(format!(
            "PostMessageW failed: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

fn point(x: i32, y: i32) -> LPARAM {
    ((y as u16 as u32) << 16 | x as u16 as u32) as LPARAM
}

fn type_text(window: HWND, text: &str, hold: Duration, pace: &dyn Pace) -> Result<(), String> {
    let keys = text
        .chars()
        .map(|c| {
            if let Some((vk, shift)) = crate::us_keys::us_key(c) {
                return Ok(vk as u16 | if shift { 1 << 8 } else { 0 });
            }
            let mut units = [0u16; 2];
            match c.encode_utf16(&mut units) {
                [unit] => match unsafe { VkKeyScanW(*unit) } {
                    -1 => Err(format!("{c:?} can't be typed with this keyboard layout.")),
                    key if (0x60..=0x6f).contains(&(key as u16 & 0xff)) => {
                        Err(format!("{c:?} is on the keypad in this keyboard layout."))
                    }
                    key => Ok(key as u16),
                },
                _ => Err(format!("{c:?} can't be typed with this keyboard layout.")),
            }
        })
        .collect::<Result<Vec<u16>, String>>()?;

    let mut held = 0;
    for (i, key) in keys.into_iter().enumerate() {
        if i > 0 && !pace.sleep(BETWEEN_KEYS) {
            break;
        }
        let vk = (key & 0xff) as u32;
        let wanted = [input::SHIFT, input::CTRL, input::ALT]
            .into_iter()
            .enumerate()
            .filter(|&(bit, _)| key >> (8 + bit) & 1 != 0)
            .fold(0, |wanted, (_, modifier)| wanted | modifier);
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

static RENDER_VIEWS: Mutex<Vec<(u32, usize)>> = Mutex::new(Vec::new());

fn render_view(pid: u32) -> Option<HWND> {
    let mut known = RENDER_VIEWS.lock().unwrap_or_else(PoisonError::into_inner);
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
