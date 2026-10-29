//! An embedded terminal: a shell on a pseudo-terminal emulated by `alacritty_terminal`, living as long as the app.

use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{Config, Term, TermMode, cell::Flags, color::Colors},
    vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb},
};
use eframe::egui::{self, Color32};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// Lines of scrollback kept.
const HISTORY: usize = 10_000;
const FONT_SIZE: f32 = 14.0;

/// Tokyonight (night) like the rest of the app: the 16 ANSI colors, then the default foreground and background.
const ANSI: [[u8; 3]; 16] = [
    [0x15, 0x16, 0x1e],
    [0xf7, 0x76, 0x8e],
    [0x9e, 0xce, 0x6a],
    [0xe0, 0xaf, 0x68],
    [0x7a, 0xa2, 0xf7],
    [0xbb, 0x9a, 0xf7],
    [0x7d, 0xcf, 0xff],
    [0xa9, 0xb1, 0xd6],
    [0x41, 0x48, 0x68],
    [0xf7, 0x76, 0x8e],
    [0x9e, 0xce, 0x6a],
    [0xe0, 0xaf, 0x68],
    [0x7a, 0xa2, 0xf7],
    [0xbb, 0x9a, 0xf7],
    [0x7d, 0xcf, 0xff],
    [0xc0, 0xca, 0xf5],
];
const FG: [u8; 3] = [0xc0, 0xca, 0xf5];
const BG: [u8; 3] = [0x16, 0x16, 0x1e];
const SELECTION: Color32 = Color32::from_rgb(0x2e, 0x3c, 0x64);

/// The color at `index` in alacritty's table: 0-15 ANSI, 16-255 the xterm cube and grays, then foreground, background, cursor, dim and bright variants.
fn default_rgb(index: usize) -> Rgb {
    let [r, g, b] = match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = (index - 16) as u8;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        232..=255 => {
            let v = 8 + (index - 232) as u8 * 10;
            [v, v, v]
        }
        257 => BG,
        259..=266 => ANSI[index - 259].map(|v| (f32::from(v) * 0.66) as u8),
        268 => FG.map(|v| (f32::from(v) * 0.66) as u8),
        _ => FG,
    };
    Rgb { r, g, b }
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// A cell color as drawn; a color the program set with OSC 4/10/11 wins over the default table.
fn resolve(color: Color, colors: &Colors) -> Color32 {
    let index = match color {
        Color::Spec(rgb) => return color32(rgb),
        Color::Indexed(i) => usize::from(i),
        Color::Named(named) => named as usize,
    };
    color32(colors[index].unwrap_or_else(|| default_rgb(index)))
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

fn send(writer: &Writer, bytes: &[u8]) {
    if let Ok(mut writer) = writer.lock() {
        // the shell may be gone already; the exit is noticed by the reader
        let _ = writer.write_all(bytes).and_then(|()| writer.flush());
    }
}

/// Answers what the emulator asks the host: replies for the program (cursor position, colors) and clipboard writes.
#[derive(Clone)]
struct Proxy {
    writer: Writer,
    ctx: egui::Context,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => send(&self.writer, text.as_bytes()),
            Event::ColorRequest(index, format) => send(&self.writer, format(default_rgb(index)).as_bytes()),
            Event::ClipboardStore(_, text) => self.ctx.copy_text(text),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// A running shell: the emulated screen, the pty it talks through, and the child process.
struct Session {
    term: Arc<Mutex<Term<Proxy>>>,
    writer: Writer,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    size: Size,
}

impl Session {
    fn spawn(ctx: &egui::Context, program: &str, size: Size) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize { rows: size.rows as u16, cols: size.cols as u16, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new(program);
        if let Ok(dir) = std::env::current_dir() {
            command.cwd(dir);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let child = pair.slave.spawn_command(command).map_err(|e| format!("{program}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));

        let proxy = Proxy { writer: writer.clone(), ctx: ctx.clone() };
        let term = Arc::new(Mutex::new(Term::new(Config { scrolling_history: HISTORY, ..Config::default() }, &size, proxy)));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let (term, exited, ctx) = (term.clone(), exited.clone(), ctx.clone());
            thread::spawn(move || {
                let mut processor: Processor = Processor::new();
                let mut buf = [0u8; 8192];
                while let Ok(n @ 1..) = reader.read(&mut buf) {
                    if let Ok(mut term) = term.lock() {
                        processor.advance(&mut *term, &buf[..n]);
                    }
                    ctx.request_repaint();
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        Ok(Self { term, writer, master: pair.master, child, exited, size })
    }

    fn write(&self, bytes: &[u8]) {
        send(&self.writer, bytes);
    }

    fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.size = size;
        if let Ok(mut term) = self.term.lock() {
            term.resize(size);
        }
        let _ = self.master.resize(PtySize { rows: size.rows as u16, cols: size.cols as u16, pixel_width: 0, pixel_height: 0 });
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub struct Terminal {
    session: Result<Session, String>,
    /// A drag that started inside the terminal and is selecting text.
    selecting: bool,
    /// Wheel movement not yet worth a whole line.
    scroll: f32,
    focus_pending: bool,
}

impl Terminal {
    pub fn new(ctx: &egui::Context) -> Self {
        Self { session: Self::start(ctx), selecting: false, scroll: 0.0, focus_pending: true }
    }

    fn start(ctx: &egui::Context) -> Result<Session, String> {
        Session::spawn(ctx, &shell(), Size { cols: 80, rows: 24 })
    }

    /// Kills the shell, e.g. when the app closes.
    pub fn close(&mut self) {
        self.session = Err("closed".to_owned());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let font = egui::FontId::new(FONT_SIZE, egui::FontFamily::Monospace);
        let (cell_w, cell_h) = ui.ctx().fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let size = Size { cols: ((rect.width() / cell_w) as usize).max(2), rows: ((rect.height() / cell_h) as usize).max(2) };

        let ctx = ui.ctx().clone();
        let Self { session, selecting, scroll, focus_pending } = self;
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                ui.painter().rect_filled(rect, 0.0, rgb(BG));
                let mut restart = false;
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
                    ui.colored_label(crate::theme::RED, format!("Couldn't start a shell: {error}"));
                    restart = ui.button("Try again").clicked();
                });
                if restart {
                    self.session = Self::start(&ctx);
                }
                return;
            }
        };
        session.resize(size);

        if std::mem::take(focus_pending) || response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
        }

        let exited = session.exited.load(Ordering::Relaxed);
        let cell_at = |pos: egui::Pos2| {
            let col = (((pos.x - rect.min.x) / cell_w).max(0.0) as usize).min(size.cols - 1);
            let row = (((pos.y - rect.min.y) / cell_h).max(0.0) as usize).min(size.rows - 1);
            let side = if (pos.x - rect.min.x) / cell_w - col as f32 > 0.5 { Side::Right } else { Side::Left };
            (row, col, side)
        };

        let mode = session.term.lock().map(|t| *t.mode()).unwrap_or(TermMode::NONE);
        let shift = ui.input(|i| i.modifiers.shift);
        let reporting = mode.intersects(TermMode::MOUSE_MODE) && !shift;

        // Mouse: a program that asked for it gets the events, otherwise a drag selects and the wheel scrolls the history.
        if response.hovered() && !reporting {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let Some(pos) = pointer.filter(|p| rect.contains(*p) || *selecting) {
            let (row, col, side) = cell_at(pos);
            let (pressed, released, modifiers) =
                ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.modifiers));
            if reporting {
                let report = |button: u8, release: bool| session.write(&mouse_report(mode, button, modifiers, row, col, release));
                if pressed && response.hovered() {
                    report(0, false);
                } else if released {
                    report(0, true);
                } else if response.dragged() && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
                    report(32, false);
                }
            } else if let Ok(mut term) = session.term.lock() {
                let offset = term.grid().display_offset() as i32;
                let point = Point::new(Line(row as i32 - offset), Column(col));
                if response.triple_clicked() {
                    term.selection = Some(Selection::new(SelectionType::Lines, point, side));
                } else if response.double_clicked() {
                    term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
                } else if pressed && response.hovered() {
                    term.selection = Some(Selection::new(SelectionType::Simple, point, side));
                    *selecting = true;
                } else if let Some(selection) = term.selection.as_mut().filter(|_| *selecting && response.dragged()) {
                    selection.update(point, side);
                }
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            *selecting = false;
        }

        if response.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            *scroll += wheel / cell_h;
            let lines = scroll.trunc() as i32;
            *scroll -= lines as f32;
            if lines != 0 {
                if reporting {
                    let (row, col, _) = pointer.map_or((0, 0, Side::Left), cell_at);
                    let button = if lines > 0 { 64 } else { 65 };
                    for _ in 0..lines.unsigned_abs() {
                        session.write(&mouse_report(mode, button, egui::Modifiers::NONE, row, col, false));
                    }
                } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
                    let up = if mode.contains(TermMode::APP_CURSOR) { b"\x1bOA" } else { b"\x1b[A" };
                    let down = if mode.contains(TermMode::APP_CURSOR) { b"\x1bOB" } else { b"\x1b[B" };
                    for _ in 0..lines.unsigned_abs() {
                        session.write(if lines > 0 { up } else { down });
                    }
                } else if let Ok(mut term) = session.term.lock() {
                    term.scroll_display(Scroll::Delta(lines));
                }
            }
        }

        if focused {
            let events = ui.input(|i| i.events.clone());
            for event in events {
                match event {
                    Event_::Key { key: egui::Key::Enter, pressed: true, .. } if exited => {
                        *session = match Session::spawn(&ctx, &shell(), session.size) {
                            Ok(new) => new,
                            Err(error) => {
                                self.session = Err(error);
                                return;
                            }
                        };
                    }
                    Event_::Key { key: egui::Key::C, pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.shift => {
                        if let Some(text) = session.term.lock().ok().and_then(|t| t.selection_to_string()) {
                            ctx.copy_text(text);
                        }
                    }
                    Event_::Text(text) => {
                        let alt = ui.input(|i| i.modifiers.alt);
                        type_into(session, &[if alt { b"\x1b".as_slice() } else { b"" }, text.as_bytes()].concat());
                    }
                    Event_::Paste(text) => {
                        let text = if mode.contains(TermMode::BRACKETED_PASTE) {
                            format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', ""))
                        } else {
                            text.replace("\r\n", "\r").replace('\n', "\r")
                        };
                        type_into(session, text.as_bytes());
                    }
                    Event_::Key { key, pressed: true, modifiers, .. } => {
                        if let Some(bytes) = key_bytes(key, modifiers, mode) {
                            type_into(session, &bytes);
                        }
                    }
                    _ => {}
                }
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, rgb(BG));
        if let Ok(term) = session.term.lock() {
            paint(&painter, &term, rect, (cell_w, cell_h), focused);
        }
        if exited {
            let text = "[the shell exited: press Enter to start a new one]";
            painter.text(rect.left_bottom() + egui::vec2(6.0, -4.0), egui::Align2::LEFT_BOTTOM, text, font, crate::theme::YELLOW);
        }
    }
}

use egui::Event as Event_;

/// What typing does to the screen: back to the live view, selection gone, the bytes to the shell.
fn type_into(session: &Session, bytes: &[u8]) {
    if let Ok(mut term) = session.term.lock() {
        term.scroll_display(Scroll::Bottom);
        term.selection = None;
    }
    session.write(bytes);
}

fn paint(painter: &egui::Painter, term: &Term<Proxy>, rect: egui::Rect, (cell_w, cell_h): (f32, f32), focused: bool) {
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let regular = egui::FontId::new(FONT_SIZE, egui::FontFamily::Monospace);
    let bold = egui::FontId::new(FONT_SIZE, crate::theme::bold_family());
    let cursor = (content.mode.contains(TermMode::SHOW_CURSOR) && content.cursor.shape != CursorShape::Hidden && offset == 0)
        .then_some(content.cursor);
    let origin = rect.min;
    let cell_rect = |row: i32, col: usize, cells: f32| {
        egui::Rect::from_min_size(origin + egui::vec2(col as f32 * cell_w, row as f32 * cell_h), egui::vec2(cell_w * cells, cell_h))
    };

    for cell in content.display_iter {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let (row, col) = (cell.point.line.0 + offset, cell.point.column.0);
        let wide = if cell.flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
        let area = cell_rect(row, col, wide);
        let inverse = cell.flags.contains(Flags::INVERSE);
        let (mut fg, mut bg) = (resolve(cell.fg, content.colors), resolve(cell.bg, content.colors));
        if inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.intersects(Flags::DIM) {
            fg = fg.gamma_multiply(0.66);
        }
        let selected = content.selection.is_some_and(|s| s.contains(cell.point));
        let on_cursor = focused && cursor.is_some_and(|c| c.shape == CursorShape::Block && c.point == cell.point);
        if on_cursor {
            // the cursor block takes the foreground color and the text the background's
            painter.rect_filled(area, 0.0, rgb(FG));
            fg = rgb(BG);
        } else if selected {
            painter.rect_filled(area, 0.0, SELECTION);
        } else if inverse || cell.bg != Color::Named(NamedColor::Background) {
            painter.rect_filled(area, 0.0, bg);
        }
        if cell.c != ' ' && !cell.flags.contains(Flags::HIDDEN) {
            let font = if cell.flags.contains(Flags::BOLD) { bold.clone() } else { regular.clone() };
            painter.text(area.min, egui::Align2::LEFT_TOP, cell.c, font, fg);
        }
        if cell.flags.intersects(Flags::ALL_UNDERLINES) {
            let y = area.bottom() - 1.5;
            painter.line_segment([egui::pos2(area.left(), y), egui::pos2(area.right(), y)], egui::Stroke::new(1.0, fg));
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            let y = area.center().y;
            painter.line_segment([egui::pos2(area.left(), y), egui::pos2(area.right(), y)], egui::Stroke::new(1.0, fg));
        }
    }

    if let Some(cursor) = cursor {
        let area = cell_rect(cursor.point.line.0, cursor.point.column.0, 1.0);
        let color = rgb(FG);
        match (cursor.shape, focused) {
            (CursorShape::Block, true) => {}
            (CursorShape::Block | CursorShape::HollowBlock, _) => {
                painter.rect_stroke(area, 0.0, egui::Stroke::new(1.0, color), egui::StrokeKind::Inside);
            }
            (CursorShape::Underline, _) => {
                painter.rect_filled(egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - 2.0), area.max), 0.0, color);
            }
            (CursorShape::Beam, _) => {
                painter.rect_filled(egui::Rect::from_min_size(area.min, egui::vec2(2.0, cell_h)), 0.0, color);
            }
            (CursorShape::Hidden, _) => {}
        }
    }
}

fn shell() -> String {
    #[cfg(windows)]
    return std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned());
    #[cfg(not(windows))]
    return std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".to_owned());
}

/// xterm's modifier parameter: 1 + shift + alt*2 + ctrl*4.
fn modifier_param(m: egui::Modifiers) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

/// A key as the bytes a terminal sends. Printable keys come through `Event::Text` instead, so they are `None` here unless Ctrl changes them.
fn key_bytes(key: egui::Key, m: egui::Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    use egui::Key as K;
    let param = modifier_param(m);
    let plain = param == 1;
    // CSI sequence with the modifier parameter when there is one: `ESC [ 1 ; m final` or `ESC [ n ; m ~`
    let csi = |number: Option<u8>, final_byte: char| -> Vec<u8> {
        match (number, plain) {
            (None, true) => format!("\x1b[{final_byte}"),
            (None, false) => format!("\x1b[1;{param}{final_byte}"),
            (Some(n), true) => format!("\x1b[{n}{final_byte}"),
            (Some(n), false) => format!("\x1b[{n};{param}{final_byte}"),
        }
        .into_bytes()
    };
    let ss3 = |final_byte: char| -> Vec<u8> {
        if plain { format!("\x1bO{final_byte}").into_bytes() } else { csi(None, final_byte) }
    };
    let arrow = |final_byte: char| if mode.contains(TermMode::APP_CURSOR) { ss3(final_byte) } else { csi(None, final_byte) };
    let alt = |bytes: &[u8]| if m.alt { [b"\x1b".as_slice(), bytes].concat() } else { bytes.to_vec() };

    Some(match key {
        K::Enter if m.shift => b"\n".to_vec(),
        K::Enter => alt(b"\r"),
        K::Tab if m.shift => b"\x1b[Z".to_vec(),
        K::Tab => b"\t".to_vec(),
        K::Backspace if m.ctrl => b"\x17".to_vec(),
        K::Backspace => alt(b"\x7f"),
        K::Escape => b"\x1b".to_vec(),
        K::ArrowUp => arrow('A'),
        K::ArrowDown => arrow('B'),
        K::ArrowRight => arrow('C'),
        K::ArrowLeft => arrow('D'),
        K::Home => arrow('H'),
        K::End => arrow('F'),
        K::Insert => csi(Some(2), '~'),
        K::Delete => csi(Some(3), '~'),
        K::PageUp => csi(Some(5), '~'),
        K::PageDown => csi(Some(6), '~'),
        K::F1 => ss3('P'),
        K::F2 => ss3('Q'),
        K::F3 => ss3('R'),
        K::F4 => ss3('S'),
        K::F5 => csi(Some(15), '~'),
        K::F6 => csi(Some(17), '~'),
        K::F7 => csi(Some(18), '~'),
        K::F8 => csi(Some(19), '~'),
        K::F9 => csi(Some(20), '~'),
        K::F10 => csi(Some(21), '~'),
        K::F11 => csi(Some(23), '~'),
        K::F12 => csi(Some(24), '~'),
        K::Space if m.ctrl => b"\0".to_vec(),
        // Ctrl+V pastes (the Paste event carries the text) and Ctrl+Shift+C copies, so neither reaches the program
        K::V if m.ctrl => return None,
        K::C if m.ctrl && m.shift => return None,
        _ if m.ctrl && !m.alt => {
            let name = key.name();
            let byte = match name {
                "OpenBracket" | "[" => 0x1b,
                "Backslash" | "\\" => 0x1c,
                "CloseBracket" | "]" => 0x1d,
                n if n.len() == 1 && n.as_bytes()[0].is_ascii_alphabetic() => n.as_bytes()[0].to_ascii_uppercase() - b'A' + 1,
                _ => return None,
            };
            vec![byte]
        }
        _ => return None,
    })
}

/// An xterm mouse report for the cell at `row`, `col`: SGR (`ESC [ < b ; x ; y M/m`) when the program asked for it, else the legacy encoding.
fn mouse_report(mode: TermMode, button: u8, m: egui::Modifiers, row: usize, col: usize, release: bool) -> Vec<u8> {
    let code = button + 4 * u8::from(m.shift) + 8 * u8::from(m.alt) + 16 * u8::from(m.ctrl);
    if mode.contains(TermMode::SGR_MOUSE) {
        format!("\x1b[<{code};{};{}{}", col + 1, row + 1, if release { 'm' } else { 'M' }).into_bytes()
    } else {
        // legacy reports have no room past column/row 223, and a release has no button
        let code = if release { 3 + (code & !3) } else { code };
        vec![0x1b, b'[', b'M', 32 + code, 32 + (col + 1).min(223) as u8, 32 + (row + 1).min(223) as u8]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(key: egui::Key, modifiers: egui::Modifiers, mode: TermMode) -> Option<String> {
        key_bytes(key, modifiers, mode).map(|b| String::from_utf8(b).unwrap())
    }

    #[test]
    fn arrows_follow_the_cursor_key_mode() {
        let none = egui::Modifiers::NONE;
        assert_eq!(bytes(egui::Key::ArrowUp, none, TermMode::NONE).unwrap(), "\x1b[A");
        assert_eq!(bytes(egui::Key::ArrowUp, none, TermMode::APP_CURSOR).unwrap(), "\x1bOA");
        let ctrl = egui::Modifiers::CTRL;
        assert_eq!(bytes(egui::Key::ArrowLeft, ctrl, TermMode::APP_CURSOR).unwrap(), "\x1b[1;5D");
    }

    #[test]
    fn control_letters_and_editing_keys() {
        assert_eq!(bytes(egui::Key::C, egui::Modifiers::CTRL, TermMode::NONE).unwrap(), "\x03");
        assert_eq!(bytes(egui::Key::D, egui::Modifiers::CTRL, TermMode::NONE).unwrap(), "\x04");
        assert_eq!(bytes(egui::Key::Delete, egui::Modifiers::NONE, TermMode::NONE).unwrap(), "\x1b[3~");
        assert_eq!(bytes(egui::Key::Tab, egui::Modifiers::SHIFT, TermMode::NONE).unwrap(), "\x1b[Z");
        assert_eq!(bytes(egui::Key::Enter, egui::Modifiers::SHIFT, TermMode::NONE).unwrap(), "\n");
        // printable keys arrive as text, and Ctrl+V is a paste
        assert!(bytes(egui::Key::A, egui::Modifiers::NONE, TermMode::NONE).is_none());
        assert!(bytes(egui::Key::V, egui::Modifiers::CTRL, TermMode::NONE).is_none());
    }

    #[test]
    fn mouse_reports() {
        let sgr = mouse_report(TermMode::SGR_MOUSE, 0, egui::Modifiers::NONE, 4, 9, false);
        assert_eq!(sgr, b"\x1b[<0;10;5M");
        let release = mouse_report(TermMode::SGR_MOUSE, 0, egui::Modifiers::NONE, 4, 9, true);
        assert_eq!(release, b"\x1b[<0;10;5m");
        let legacy = mouse_report(TermMode::NONE, 0, egui::Modifiers::NONE, 0, 0, false);
        assert_eq!(legacy, [0x1b, b'[', b'M', 32, 33, 33]);
    }

    #[test]
    fn color_table() {
        assert_eq!(default_rgb(1).r, 0xf7);
        // the xterm cube: index 16 is black, 231 is white
        assert_eq!((default_rgb(16).r, default_rgb(231).r), (0, 255));
        assert_eq!(default_rgb(232).r, 8);
    }

    #[cfg(unix)]
    #[test]
    fn shell_output_reaches_the_screen() {
        let session = Session::spawn(&egui::Context::default(), "/bin/sh", Size { cols: 40, rows: 5 }).expect("spawn");
        session.write(b"echo volcren-$((6*7))\r");
        let found = (0..100).any(|_| {
            thread::sleep(std::time::Duration::from_millis(50));
            let term = session.term.lock().unwrap();
            let grid = term.grid();
            (0..5).any(|line| {
                let text: String = (0..40).map(|col| grid[Line(line)][Column(col)].c).collect();
                text.starts_with("volcren-42")
            })
        });
        assert!(found, "the echoed line never showed up on the screen");
        session.write(b"exit\r");
        assert!((0..100).any(|_| {
            thread::sleep(std::time::Duration::from_millis(50));
            session.exited.load(Ordering::Relaxed)
        }));
    }
}
