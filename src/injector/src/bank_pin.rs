//! Types the bank PIN on the keypad (interface 213) without the PIN ever reaching a script.
//!
//! The sandbox's `bank_pin_enter()` takes no arguments and returns only how it went. The injector reads the logged
//! in account's name, hashes it (`hash_name`) and asks the loader for the PIN of that hash (`set_lookup`), keeping it
//! only for the call. Everything that could tell a script which digit is being clicked, the
//! button lookup, the mouse path and the click, happens here. This is a port of the lib's `bankpin_keypad.luau`
//! with the helpers it used from the lib (toplevel, chatbox, mouse, keyboard, biometrics) folded in.

// The keypad logic is only reached through `enter`, which needs a Windows client; the tests run it everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use std::{
    sync::{Mutex, PoisonError},
    time::Duration,
};

#[cfg(windows)]
use crate::async_input::Pace;
use crate::{Input, MouseButton, SubInterface, Widget, wind_mouse::{self, Rng}};

const GROUP: i32 = 213;
const PROMPT: u16 = 10;
const BUTTONS: [u16; 10] = [16, 18, 20, 22, 24, 26, 28, 30, 32, 34];
const ORDINALS: [&str; 4] = ["FIRST", "SECOND", "THIRD", "FOURTH"];

// Where each root interface keeps its modal, crm and floater sub-interfaces (the lib's `Toplevel.SLOTS`).
const SLOTS: [(i32, [i32; 3]); 4] = [(161, [16, 17, 18]), (164, [16, 17, 18]), (165, [14, 15, 41]), (548, [41, 42, 43])];

// The chatbox's own interfaces (the lib's `Chatbox`): the modal slot of group 162, and the level up display wherever it is.
const CHATBOX_GROUP: i32 = 162;
const CHATBOX_MODAL: i32 = 568;
const LEVELUP_DISPLAY: i32 = 233;

const POLL_MS: u64 = 50;
const HOVER_TIMEOUT_MS: u64 = 600;
const LOADING_TIMEOUT_MS: u64 = 3000;
const DIGIT_TIMEOUT_MS: u64 = 5000;
const UNLOCK_TIMEOUT_MS: u64 = 3000;
const REOPEN_TIMEOUT_MS: u64 = 3000;
const FORGOTTEN_TIMEOUT_MS: u64 = 20000;
const ATTEMPTS: usize = 3;
const PIN_LEN: usize = 4;
const VK_SPACE: u8 = 0x20;

// The lib's mouse defaults.
const MOUSE: wind_mouse::Options = wind_mouse::Options { speed: 14.0, wind: 5.0, gravity: 10.0, timeout_ms: 15000 };
const CLICK_HOLD_MS: (f64, f64) = (40.0, 220.0);
const KEY_HOLD_MS: (f64, f64) = (30.0, 60.0);
// Between digits: usually around a second, never under 0.3 s or over 1.8 s.
const DIGIT_PAUSE_MS: (f64, f64) = (300.0, 1800.0);
const DIGIT_PAUSE_MEAN_MS: f64 = 1000.0;
const DIGIT_PAUSE_SPREAD_MS: f64 = 300.0;

/// Asks the loader for the PIN saved for an account hash: the PIN, `None` if there is none, or why it can't say.
pub type Lookup = Box<dyn FnMut(&str) -> Result<Option<String>, String> + Send>;

static LOOKUP: Mutex<Option<Lookup>> = Mutex::new(None);

/// Where `bank_pin_enter` gets PINs from. Without one every call is `no_pin`.
pub fn set_lookup(lookup: Lookup) {
    *LOOKUP.lock().unwrap_or_else(PoisonError::into_inner) = Some(lookup);
}

/// SHA-256 of the account name as the client normalizes it (trimmed, lowercase, underscores, hyphens and nbsp as spaces), as
/// lowercase hex. A name typed as "My Name" and the client's own `my_name` hash the same. The loader keys its saved
/// accounts by this.
pub fn hash_name(account: &str) -> String {
    use sha2::{Digest, Sha256};
    let clean: String = account.trim().to_lowercase().chars().map(|c| if matches!(c, '_' | '-' | '\u{a0}') { ' ' } else { c }).collect();
    Sha256::digest(clean.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_pin(pin: &str) -> Option<[u8; 4]> {
    let digits: Vec<u8> = pin.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    <[u8; 4]>::try_from(digits).ok().filter(|_| pin.len() == 4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The keypad is gone and the chat shows no complaint.
    Entered,
    /// The keypad isn't open.
    Closed,
    /// No PIN is saved for the logged in account, or this run has no loader to ask.
    NoPin,
    /// The name of the logged in account isn't known, or the loader couldn't say (its accounts are locked, ...).
    Unavailable,
    /// The client said the PIN was entered incorrectly several times.
    Locked,
    /// Every attempt ended in "try again".
    Wrong,
    /// The keypad didn't behave: buttons never loaded, a digit wasn't clicked.
    Failed,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Self::Entered => "entered",
            Self::Closed => "closed",
            Self::NoPin => "no_pin",
            Self::Unavailable => "unavailable",
            Self::Locked => "locked",
            Self::Wrong => "wrong",
            Self::Failed => "failed",
        }
    }
}

/// What the loader had for the logged in account.
enum Pin {
    Found([u8; 4]),
    None,
    Unavailable,
}

/// Enters the logged in account's PIN on the open keypad of client `pid`.
#[cfg(windows)]
pub fn enter(pid: u32, pace: &(dyn Pace + Sync)) -> Result<Outcome, String> {
    // On a thread of its own: one menu read puts hundreds of KB on the stack (a 500 entry reply, copied by value a few
    // times), and the hover checks start deeper than a script's own `menu()` call, on the main thread's smaller stack.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("bank-pin".into())
            .stack_size(16 << 20)
            .spawn_scoped(scope, || enter_here(pid, pace))
            .map_err(|error| format!("couldn't start the bank PIN thread: {error}"))?
            .join()
            .unwrap_or_else(|_| Err("the bank PIN thread panicked".into()))
    })
}

#[cfg(windows)]
fn enter_here(pid: u32, pace: &dyn Pace) -> Result<Outcome, String> {
    let client = Live { pid, pace };
    run(&client, &mut || {
        let mut lookup = LOOKUP.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(lookup) = lookup.as_mut() else { return Ok(Pin::None) };
        let Some(name) = client.player_name()? else { return Ok(Pin::Unavailable) };
        Ok(match lookup(&hash_name(&name)) {
            Ok(Some(pin)) => parse_pin(&pin).map_or(Pin::Unavailable, Pin::Found),
            Ok(None) => Pin::None,
            Err(_) => Pin::Unavailable,
        })
    })
}

fn run(client: &dyn Client, pin: &mut dyn FnMut() -> Result<Pin, String>) -> Result<Outcome, String> {
    let mut keypad = Keypad { client, rng: Rng::seeded() };
    match keypad.enter(pin) {
        Ok(outcome) => Ok(outcome),
        // A cancelled run posts nothing more; to the script it is a keypad that wasn't entered.
        Err(Stop::Cancelled) => Ok(Outcome::Failed),
        Err(Stop::Error(error)) => Err(error),
    }
}

/// What the keypad reads from and posts to the client.
trait Client {
    fn widget(&self, group: u16, child: u16) -> Result<Option<Widget>, String>;
    fn children(&self, group: u16, child: u16) -> Result<Option<Vec<Widget>>, String>;
    fn sub_interfaces(&self) -> Result<Option<Vec<SubInterface>>, String>;
    fn root_interface(&self) -> Result<Option<i32>, String>;
    /// The logged in player's name as the game shows it.
    fn player_name(&self) -> Result<Option<String>, String>;
    fn mouse(&self) -> Option<(i32, i32)>;
    /// The id of the menu's first entry, which is what a click would pick.
    fn hovered(&self) -> Result<Option<i32>, String>;
    fn send(&self, input: &Input) -> Result<(), String>;
    /// False when cancelled.
    fn sleep(&self, duration: Duration) -> bool;
    /// Darkens the keypad and says who is handling it (`true`), or takes that away again (`false`). Best effort.
    fn cover(&self, _on: bool) {}
}

#[cfg(windows)]
struct Live<'a> {
    pid: u32,
    pace: &'a dyn Pace,
}

#[cfg(windows)]
impl Client for Live<'_> {
    fn widget(&self, group: u16, child: u16) -> Result<Option<Widget>, String> {
        crate::widget(self.pid, group, child)
    }
    fn children(&self, group: u16, child: u16) -> Result<Option<Vec<Widget>>, String> {
        crate::widget_children(self.pid, group, child)
    }
    fn sub_interfaces(&self) -> Result<Option<Vec<SubInterface>>, String> {
        crate::sub_interfaces(self.pid)
    }
    fn root_interface(&self) -> Result<Option<i32>, String> {
        Ok(crate::game_state(self.pid)?.root_interface)
    }
    fn player_name(&self) -> Result<Option<String>, String> {
        Ok(crate::game_state(self.pid)?.player_name)
    }
    fn mouse(&self) -> Option<(i32, i32)> {
        crate::connection(self.pid).filter(|conn| conn.mouse_x >= 0).map(|conn| (conn.mouse_x, conn.mouse_y))
    }
    fn hovered(&self) -> Result<Option<i32>, String> {
        Ok(crate::menu(self.pid)?.first().map(|entry| entry.id))
    }
    fn send(&self, input: &Input) -> Result<(), String> {
        crate::send_input_with(self.pid, input, self.pace)
    }
    fn sleep(&self, duration: Duration) -> bool {
        self.pace.sleep(duration)
    }
    fn cover(&self, on: bool) {
        if !on {
            let _ = crate::draw(self.pid, &crate::Overlay::Clear);
            return;
        }
        // the box around every visible part of the keypad
        let mut area: Option<[i32; 4]> = None;
        let mut child = 0;
        while let Ok(Some(part)) = crate::widget(self.pid, GROUP as u16, child) {
            if !part.hidden && part.width > 0 && part.height > 0 {
                let [x1, y1, x2, y2] = [part.x, part.y, part.x + part.width - 1, part.y + part.height - 1];
                area = Some(area.map_or([x1, y1, x2, y2], |a| [a[0].min(x1), a[1].min(y1), a[2].max(x2), a[3].max(y2)]));
            }
            child += 1;
        }
        if let Some(area) = area {
            let _ = crate::paint_overlay(self.pid, |put| crate::bank_pin_overlay::paint(area, put));
        }
    }
}

enum Stop {
    Cancelled,
    Error(String),
}

impl From<String> for Stop {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

type Step<T> = Result<T, Stop>;

struct Keypad<'a> {
    client: &'a dyn Client,
    rng: Rng,
}

impl Keypad<'_> {
    fn sleep(&self, ms: u64) -> Step<()> {
        if self.client.sleep(Duration::from_millis(ms)) { Ok(()) } else { Err(Stop::Cancelled) }
    }

    // A pause that is usually near `lo` and rarely near `hi` (the lib's `Random.left`).
    fn left(&mut self, (lo, hi): (f64, f64)) -> u64 {
        const CUTOFF: f64 = 5.0;
        let mut r = CUTOFF + 1.0;
        while r >= CUTOFF {
            let u = self.rng.random().max(1.0e-300);
            r = ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * self.rng.random()).cos()).abs();
        }
        (r / CUTOFF * (hi - lo) + lo) as u64
    }

    // A standard normal number.
    fn normal(&mut self) -> f64 {
        let u = self.rng.random().max(1.0e-300);
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * self.rng.random()).cos()
    }

    // A time bell-shaped around `mean` with standard deviation `spread`, redrawn until it is within `lo..=hi`.
    fn around(&mut self, mean: f64, spread: f64, (lo, hi): (f64, f64)) -> u64 {
        loop {
            let value = mean + spread * self.normal();
            if (lo..=hi).contains(&value) {
                return value as u64;
            }
        }
    }

    // A coordinate in `lo..lo + size`, bell-shaped around the middle (the lib's `Box.random_point_center`).
    fn around_center(&mut self, lo: i32, size: i32) -> i32 {
        let offset = (self.normal() * f64::from(size) / 6.0).round() as i32;
        (lo + size / 2 + offset).clamp(lo, lo + (size - 1).max(0))
    }

    fn wait_until(&mut self, timeout_ms: u64, mut check: impl FnMut(&mut Self) -> Step<bool>) -> Step<bool> {
        let mut waited = 0;
        loop {
            if check(self)? {
                return Ok(true);
            }
            if waited >= timeout_ms {
                return Ok(false);
            }
            self.sleep(POLL_MS)?;
            waited += POLL_MS;
        }
    }

    fn is_open(&self) -> Step<bool> {
        let Some(root) = self.client.root_interface()? else { return Ok(false) };
        let Some((_, slots)) = SLOTS.iter().find(|(known, _)| *known == root) else { return Ok(false) };
        let Some(open) = self.client.sub_interfaces()? else { return Ok(false) };
        Ok(open.iter().any(|s| s.group == GROUP && s.parent_group == root && slots.contains(&s.parent_child)))
    }

    // Every visible line of the chatbox's interfaces, lowercased.
    fn chat(&self) -> Step<String> {
        let Some(open) = self.client.sub_interfaces()? else { return Ok(String::new()) };
        let mut lines: Vec<String> = Vec::new();
        for s in open.iter().filter(|s| (s.parent_group == CHATBOX_GROUP && s.parent_child == CHATBOX_MODAL) || s.group == LEVELUP_DISPLAY) {
            let mut child = 0;
            while let Some(w) = self.client.widget(s.group as u16, child)? {
                if !w.hidden {
                    lines.extend(w.text.filter(|t| !t.is_empty()));
                    for created in self.client.children(s.group as u16, child)?.unwrap_or_default() {
                        if !created.hidden {
                            lines.extend(created.text.filter(|t| !t.is_empty()));
                        }
                    }
                }
                child += 1;
            }
        }
        Ok(lines.join("\n").to_lowercase())
    }

    // The button for each digit. A created child of the button holds the digit; it is hidden while the client shuffles,
    // and the client also empties the child of the button under the mouse. With the other nine known, that button's
    // digit is the one left. Fewer than nine known means the keypad is still loading.
    fn buttons(&self) -> Step<[Option<Widget>; 10]> {
        let mouse = self.client.mouse();
        let mut found: [Option<Widget>; 10] = Default::default();
        let mut hovered = None;
        for child in BUTTONS {
            let Some(button) = self.client.widget(GROUP as u16, child)?.filter(|b| !b.hidden) else { continue };
            let under_mouse = mouse.is_some_and(|(x, y)| x >= button.x && y >= button.y && x < button.x + button.width && y < button.y + button.height);
            let mut digit = None;
            for created in self.client.children(GROUP as u16, button.child as u16)?.unwrap_or_default() {
                if let Some([d @ b'0'..=b'9']) = created.text.as_deref().map(str::as_bytes) {
                    digit = Some((usize::from(d - b'0'), !created.hidden));
                    break;
                }
            }
            match digit {
                Some((digit, shown)) if shown || under_mouse => found[digit] = Some(button),
                None if under_mouse => hovered = Some(button),
                _ => {}
            }
        }
        if let Some(button) = hovered {
            let mut missing = (0..10).filter(|digit| found[*digit].is_none());
            if let (Some(digit), None) = (missing.next(), missing.next()) {
                found[digit] = Some(button);
            }
        }
        Ok(found)
    }

    fn is_loading(&self) -> Step<bool> {
        Ok(self.is_open()? && self.buttons()?.iter().any(Option::is_none))
    }

    // Which digit of the PIN the prompt asks for, 1 to 4, or 0 when it asks for none.
    fn digit_index(&self) -> Step<usize> {
        let prompt = self.client.widget(GROUP as u16, PROMPT)?;
        let Some(text) = prompt.filter(|p| !p.hidden).and_then(|p| p.text) else { return Ok(0) };
        Ok(ORDINALS.iter().position(|ordinal| text.contains(ordinal)).map_or(0, |i| i + 1))
    }

    fn wait_loaded(&mut self, timeout_ms: u64) -> Step<bool> {
        self.wait_until(timeout_ms, |this| Ok(!this.is_loading()?))
    }

    fn hovering(&mut self, button: &Widget) -> Step<bool> {
        let mut over = false;
        self.wait_until(HOVER_TIMEOUT_MS, |this| match this.client.hovered() {
            Ok(first) => {
                over = first == Some(button.id);
                Ok(over)
            }
            Err(_) => Ok(true),
        })?;
        Ok(over)
    }

    fn click_digit(&mut self, digit: u8) -> Step<bool> {
        if !self.is_open()? {
            return Ok(false);
        }
        let Some(button) = self.buttons()?[usize::from(digit)].take() else { return Ok(false) };
        let (x, y) = (self.around_center(button.x, button.width), self.around_center(button.y, button.height));
        self.client.send(&Input::WindMouse { x, y, options: MOUSE })?;
        if !self.hovering(&button)? {
            return Ok(false);
        }
        let hold_ms = self.left(CLICK_HOLD_MS);
        self.client.send(&Input::Button { button: MouseButton::Left, down: true })?;
        self.sleep(hold_ms)?;
        self.client.send(&Input::Button { button: MouseButton::Left, down: false })?;
        Ok(true)
    }

    fn type_digits(&mut self, pin: [u8; 4]) -> Step<bool> {
        for _ in 0..4 {
            let index = self.digit_index()?;
            if index == 0 {
                break;
            }
            if !self.wait_loaded(LOADING_TIMEOUT_MS)? {
                return Ok(false);
            }
            if !self.click_digit(pin[index - 1])? {
                return Ok(false);
            }
            // a pause between clicks, usually short
            if index < PIN_LEN {
                let pause = self.around(DIGIT_PAUSE_MEAN_MS, DIGIT_PAUSE_SPREAD_MS, DIGIT_PAUSE_MS);
                self.sleep(pause)?;
            }
            self.wait_until(DIGIT_TIMEOUT_MS, |this| Ok(this.digit_index()? != index && !this.is_loading()?))?;
        }
        Ok(true)
    }

    fn press_space(&mut self) -> Step<()> {
        let hold_ms = self.left(KEY_HOLD_MS);
        self.client.send(&Input::Key { key: VK_SPACE, down: true })?;
        self.sleep(hold_ms)?;
        self.client.send(&Input::Key { key: VK_SPACE, down: false })?;
        Ok(())
    }

    fn failed(&self) -> Step<bool> {
        let text = self.chat()?;
        Ok(["try again", "forgotten your pin", "pin incorrectly several"].iter().any(|needle| text.contains(needle)))
    }

    fn enter(&mut self, pin: &mut dyn FnMut() -> Result<Pin, String>) -> Step<Outcome> {
        if !self.is_open()? {
            return Ok(Outcome::Closed);
        }
        let pin = match pin()? {
            Pin::Found(pin) => pin,
            Pin::None => return Ok(Outcome::NoPin),
            Pin::Unavailable => return Ok(Outcome::Unavailable),
        };
        self.client.cover(true);
        let outcome = self.attempts(pin);
        self.client.cover(false);
        outcome
    }

    fn attempts(&mut self, pin: [u8; 4]) -> Step<Outcome> {
        let mut wrong = false;
        for _ in 0..ATTEMPTS {
            if !self.type_digits(pin)? {
                return Ok(Outcome::Failed);
            }

            let text = self.chat()?;
            wrong = false;
            if text.contains("pin incorrectly several") {
                return Ok(Outcome::Locked);
            } else if text.contains("try again") {
                wrong = true;
                self.press_space()?;
                if !self.wait_until(REOPEN_TIMEOUT_MS, |this| this.is_open())? {
                    return Ok(Outcome::Failed);
                }
            } else if text.contains("forgotten your pin") {
                self.wait_until(FORGOTTEN_TIMEOUT_MS, |this| Ok(!this.chat()?.contains("forgotten your pin")))?;
            } else if self.wait_until(UNLOCK_TIMEOUT_MS, |this| Ok(!this.is_open()? && !this.failed()?))? {
                return Ok(Outcome::Entered);
            }

            if !self.is_open()? && !self.failed()? {
                return Ok(Outcome::Entered);
            }
        }
        Ok(if wrong { Outcome::Wrong } else { Outcome::Failed })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    // A keypad whose buttons sit at fixed places; clicking the button for the asked-for digit moves to the next
    // position, and a wrong digit says "try again" once the four are in.
    struct Fake {
        pin: [u8; 4],
        position: Cell<usize>,
        mouse: Cell<(i32, i32)>,
        clicks: RefCell<Vec<u8>>,
        message: RefCell<&'static str>,
        open: Cell<bool>,
        wrong: Cell<usize>,
        covers: RefCell<Vec<bool>>,
    }

    impl Fake {
        fn new(pin: [u8; 4]) -> Self {
            Self { pin, position: Cell::new(0), mouse: Cell::new((0, 0)), clicks: RefCell::default(), covers: RefCell::default(), message: RefCell::new(""), open: Cell::new(true), wrong: Cell::new(0) }
        }

        // Button `slot` shows the digit `(slot * 3 + 1) % 10`, laid out in a row of 40 wide boxes.
        fn digit_at(slot: usize) -> u8 {
            ((slot * 3 + 1) % 10) as u8
        }

        fn slot_under_mouse(&self) -> Option<usize> {
            let (x, y) = self.mouse.get();
            (0..10).find(|slot| x >= *slot as i32 * 50 && x < *slot as i32 * 50 + 40 && (0..40).contains(&y))
        }

        fn text(text: &str) -> Widget {
            Widget { id: 0, group: 0, child: 0, kind: 4, x: 0, y: 0, width: 0, height: 0, parent: 0, sprite: 0, index: -1, hidden: false, text: Some(text.into()), text_color: -1, scroll_x: 0, scroll_y: 0, scroll_width: 0, scroll_height: 0 }
        }
    }

    impl Client for Fake {
        fn widget(&self, group: u16, child: u16) -> Result<Option<Widget>, String> {
            if i32::from(group) == CHATBOX_GROUP + 1 {
                return Ok((child == 0).then(|| Self::text(&self.message.borrow())));
            }
            if i32::from(group) != GROUP || !self.open.get() {
                return Ok(None);
            }
            if child == PROMPT {
                let ordinal = ORDINALS.get(self.position.get()).copied().unwrap_or("");
                let mut prompt = Self::text(ordinal);
                prompt.hidden = self.position.get() >= 4;
                return Ok(Some(prompt));
            }
            Ok(BUTTONS.iter().position(|b| *b == child).map(|slot| Widget { id: 1000 + slot as i32, child: i32::from(child), x: slot as i32 * 50, y: 0, width: 40, height: 40, ..Self::text("") }))
        }
        fn children(&self, group: u16, child: u16) -> Result<Option<Vec<Widget>>, String> {
            if i32::from(group) != GROUP {
                return Ok(None);
            }
            // like the client: the button under the mouse has its digit hidden and emptied
            Ok(BUTTONS.iter().position(|b| *b == child).map(|slot| {
                if self.slot_under_mouse() == Some(slot) {
                    let mut digit = Self::text("");
                    digit.hidden = true;
                    vec![digit]
                } else {
                    vec![Self::text(&Self::digit_at(slot).to_string())]
                }
            }))
        }
        fn sub_interfaces(&self) -> Result<Option<Vec<SubInterface>>, String> {
            let mut open = vec![SubInterface { group: CHATBOX_GROUP + 1, parent: 0, parent_group: CHATBOX_GROUP, parent_child: CHATBOX_MODAL }];
            if self.open.get() {
                open.push(SubInterface { group: GROUP, parent: 0, parent_group: 161, parent_child: 16 });
            }
            Ok(Some(open))
        }
        fn root_interface(&self) -> Result<Option<i32>, String> {
            Ok(Some(161))
        }
        fn player_name(&self) -> Result<Option<String>, String> {
            Ok(Some("Some Name".into()))
        }
        fn mouse(&self) -> Option<(i32, i32)> {
            Some(self.mouse.get())
        }
        fn hovered(&self) -> Result<Option<i32>, String> {
            Ok(self.slot_under_mouse().map(|slot| 1000 + slot as i32))
        }
        fn send(&self, input: &Input) -> Result<(), String> {
            match *input {
                Input::WindMouse { x, y, .. } => self.mouse.set((x, y)),
                Input::Button { down: true, .. } => {
                    let digit = self.slot_under_mouse().map(Self::digit_at).ok_or("clicked nothing")?;
                    self.clicks.borrow_mut().push(digit);
                    let position = self.position.get();
                    if digit == self.pin[position] {
                        self.position.set(position + 1);
                    } else {
                        self.wrong.set(self.wrong.get() + 1);
                        self.position.set(position + 1);
                        *self.message.borrow_mut() = "That's not the correct PIN, try again";
                    }
                    if self.position.get() == 4 {
                        self.open.set(!self.message.borrow().is_empty());
                    }
                }
                Input::Key { key: VK_SPACE, down: true } => {
                    *self.message.borrow_mut() = "";
                    self.position.set(0);
                    self.open.set(true);
                }
                _ => {}
            }
            Ok(())
        }
        fn sleep(&self, _: Duration) -> bool {
            true
        }
        fn cover(&self, on: bool) {
            self.covers.borrow_mut().push(on);
        }
    }

    #[test]
    fn clicks_the_digits_of_the_pin_in_order() {
        let fake = Fake::new([4, 7, 0, 3]);
        assert_eq!(run(&fake, &mut || Ok(Pin::Found([4, 7, 0, 3]))), Ok(Outcome::Entered));
        assert_eq!(*fake.clicks.borrow(), [4, 7, 0, 3]);
    }

    #[test]
    fn covers_the_keypad_only_while_it_handles_the_pin() {
        let fake = Fake::new([4, 7, 0, 3]);
        assert_eq!(run(&fake, &mut || Ok(Pin::None)), Ok(Outcome::NoPin));
        assert!(fake.covers.borrow().is_empty());
        assert_eq!(run(&fake, &mut || Ok(Pin::Found([4, 7, 0, 3]))), Ok(Outcome::Entered));
        assert_eq!(*fake.covers.borrow(), [true, false]);

        // also when it gives up
        let fake = Fake::new([4, 7, 0, 3]);
        assert_eq!(run(&fake, &mut || Ok(Pin::Found([1, 1, 1, 1]))), Ok(Outcome::Wrong));
        assert_eq!(*fake.covers.borrow(), [true, false]);
    }

    #[test]
    fn needs_a_pin_and_an_open_keypad() {
        let fake = Fake::new([4, 7, 0, 3]);
        assert_eq!(run(&fake, &mut || Ok(Pin::None)), Ok(Outcome::NoPin));
        assert_eq!(run(&fake, &mut || Ok(Pin::Unavailable)), Ok(Outcome::Unavailable));
        // a closed keypad isn't worth asking the loader about
        fake.open.set(false);
        assert_eq!(run(&fake, &mut || panic!("asked for a PIN")), Ok(Outcome::Closed));
        assert!(fake.clicks.borrow().is_empty());
    }

    #[test]
    fn a_wrong_pin_is_retried_then_reported() {
        let fake = Fake::new([4, 7, 0, 3]);
        assert_eq!(run(&fake, &mut || Ok(Pin::Found([1, 1, 1, 1]))), Ok(Outcome::Wrong));
        assert_eq!(fake.clicks.borrow().len(), 4 * ATTEMPTS);
    }

    #[test]
    fn pauses_between_digits_are_around_a_second_within_bounds() {
        let fake = Fake::new([0; 4]);
        let mut keypad = Keypad { client: &fake, rng: Rng::seeded() };
        let pauses: Vec<u64> = (0..4000).map(|_| keypad.around(DIGIT_PAUSE_MEAN_MS, DIGIT_PAUSE_SPREAD_MS, DIGIT_PAUSE_MS)).collect();
        assert!(pauses.iter().all(|&ms| (300..=1800).contains(&ms)));
        let mean = pauses.iter().sum::<u64>() as f64 / pauses.len() as f64;
        assert!((950.0..1050.0).contains(&mean), "mean {mean}");
        // most of them within a spread of the mean
        let near = pauses.iter().filter(|&&ms| (700..=1300).contains(&ms)).count();
        assert!(near > pauses.len() * 6 / 10, "{near} near");
    }

    #[test]
    fn a_pin_is_four_digits() {
        assert_eq!(parse_pin("0123"), Some([0, 1, 2, 3]));
        for bad in ["", "123", "12345", "12a4", "١٢٣٤"] {
            assert_eq!(parse_pin(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn names_hash_as_the_client_writes_them() {
        assert_eq!(hash_name("  Some Name "), hash_name("some_name"));
        assert_eq!(hash_name("P 4I N"), hash_name("p_4i_n"));
        assert_eq!(hash_name("p_4i_n"), hash_name("p 4i n"));
        assert_eq!(hash_name("a-b\u{a0}c"), hash_name("a_b_c"));
        assert_ne!(hash_name("some name"), hash_name("some  name"));
        assert_eq!(hash_name("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
