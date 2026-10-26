use std::{hash::{DefaultHasher, Hash, Hasher}, ops::Range};

use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Comment,
    String,
    Literal,
    Keyword,
    Builtin,
    Host,
    Call,
}

const KEYWORDS: &[&str] = &[
    "and", "break", "continue", "do", "else", "elseif", "end", "for", "function", "if", "in", "local", "not",
    "or", "repeat", "return", "then", "until", "while",
];
const LITERALS: &[&str] = &["true", "false", "nil"];
const BUILTINS: &[&str] = &[
    "assert", "bit32", "buffer", "coroutine", "error", "getmetatable", "ipairs", "math", "next", "pairs",
    "pcall", "rawequal", "rawget", "rawlen", "rawset", "select", "self", "setmetatable", "string", "table",
    "tonumber", "tostring", "type", "typeof", "unpack", "utf8", "vector", "xpcall", "_G", "_VERSION",
];
const HOST: &[&str] = &[
    "print", "sleep", "require", "game_state", "connection", "widget", "item_container", "varc", "npcs", "npc_model", "locs", "loc_model", "players", "click",
    "draw_rect", "clear_overlay", "wind_mouse", "mouse_down", "mouse_up", "scroll", "key_down", "key_up", "widget_children",
    "world_select", "time",
];

fn is_name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn long_bracket(text: &[u8], at: usize) -> Option<usize> {
    if text.get(at) != Some(&b'[') {
        return None;
    }
    let level = text[at + 1..].iter().take_while(|&&c| c == b'=').count();
    (text.get(at + 1 + level) == Some(&b'[')).then_some(level)
}

fn long_bracket_end(text: &[u8], from: usize, level: usize) -> usize {
    let mut i = from;
    while i < text.len() {
        if text[i] == b']'
            && text[i + 1..].iter().take(level).filter(|&&c| c == b'=').count() == level
            && text.get(i + 1 + level) == Some(&b']')
        {
            return i + level + 2;
        }
        i += 1;
    }
    text.len()
}

fn quoted_end(text: &[u8], at: usize) -> usize {
    let quote = text[at];
    let mut i = at + 1;
    while i < text.len() {
        match text[i] {
            b'\\' => i += 2,
            b'\n' => return i,
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    text.len()
}

fn interpolation_end(text: &[u8], from: usize) -> usize {
    let (mut depth, mut i) = (0usize, from);
    while i < text.len() {
        match text[i] {
            b'"' | b'\'' => i = quoted_end(text, i),
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' if depth == 0 => return i,
            b'}' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    text.len()
}

pub fn tokenize(source: &str) -> Vec<(Range<usize>, Kind)> {
    let mut tokens = Vec::new();
    tokenize_into(source.as_bytes(), 0, source.len(), &mut tokens);
    tokens
}

fn push(tokens: &mut Vec<(Range<usize>, Kind)>, range: Range<usize>, kind: Kind) {
    if range.is_empty() {
        return;
    }
    match tokens.last_mut() {
        Some((last, last_kind)) if *last_kind == kind && last.end == range.start => last.end = range.end,
        _ => tokens.push((range, kind)),
    }
}

fn tokenize_into(text: &[u8], start: usize, end: usize, tokens: &mut Vec<(Range<usize>, Kind)>) {
    let mut i = start;
    while i < end {
        let c = text[i];
        let rest = &text[i..end];
        if rest.starts_with(b"--") {
            let close = match long_bracket(text, i + 2) {
                Some(level) => long_bracket_end(text, i + 4 + level, level),
                None => i + rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len()),
            };
            push(tokens, i..close.min(end), Kind::Comment);
            i = close.min(end);
        } else if c == b'"' || c == b'\'' {
            let close = quoted_end(text, i).min(end);
            push(tokens, i..close, Kind::String);
            i = close;
        } else if let Some(level) = long_bracket(text, i) {
            let close = long_bracket_end(text, i + 2 + level, level).min(end);
            push(tokens, i..close, Kind::String);
            i = close;
        } else if c == b'`' {
            let mut j = i + 1;
            let mut part = i;
            while j < end && text[j] != b'`' {
                match text[j] {
                    b'\\' => j += 2,
                    b'{' => {
                        push(tokens, part..j + 1, Kind::String);
                        let close = interpolation_end(text, j + 1).min(end);
                        tokenize_into(text, j + 1, close, tokens);
                        part = close;
                        j = close + 1;
                    }
                    _ => j += 1,
                }
            }
            let close = (j + 1).min(end);
            push(tokens, part.min(close)..close, Kind::String);
            i = close;
        } else if c.is_ascii_digit() || (c == b'.' && rest.get(1).is_some_and(u8::is_ascii_digit)) {
            let hex = rest.len() > 1 && c == b'0' && matches!(rest[1], b'x' | b'X');
            let mut j = i + 1;
            while j < end {
                let d = text[j];
                let exponent_sign = !hex && matches!(d, b'+' | b'-') && matches!(text[j - 1], b'e' | b'E');
                if !(is_name(d) || d == b'.' || exponent_sign) {
                    break;
                }
                j += 1;
            }
            push(tokens, i..j, Kind::Literal);
            i = j;
        } else if is_name_start(c) {
            let j = i + rest.iter().take_while(|&&c| is_name(c)).count();
            let word = std::str::from_utf8(&text[i..j]).unwrap_or("");
            let after = text[j..end].iter().position(|c| !c.is_ascii_whitespace()).map(|n| text[j + n]);
            let field = i > 0 && matches!(text[i - 1], b'.' | b':') && !(i > 1 && text[i - 2] == b'.');
            let kind = if field {
                if after == Some(b'(') { Kind::Call } else { Kind::Plain }
            } else if KEYWORDS.contains(&word) || (matches!(word, "type" | "export") && after.is_some_and(is_name_start)) {
                Kind::Keyword
            } else if LITERALS.contains(&word) {
                Kind::Literal
            } else if HOST.contains(&word) {
                Kind::Host
            } else if BUILTINS.contains(&word) {
                Kind::Builtin
            } else if after == Some(b'(') {
                Kind::Call
            } else {
                Kind::Plain
            };
            push(tokens, i..j, kind);
            i = j;
        } else {
            let mut j = i + 1;
            while j < end && !text[j].is_ascii() {
                j += 1;
            }
            push(tokens, i..j, Kind::Plain);
            i = j;
        }
    }
}

fn color(kind: Kind, dark: bool) -> Color32 {
    let (dark_color, light_color) = match kind {
        Kind::Plain => (Color32::from_rgb(0xd4, 0xd4, 0xd4), Color32::from_rgb(0x24, 0x29, 0x2e)),
        Kind::Comment => (Color32::from_rgb(0x6a, 0x99, 0x55), Color32::from_rgb(0x5a, 0x7a, 0x4a)),
        Kind::String => (Color32::from_rgb(0xce, 0x91, 0x78), Color32::from_rgb(0xa3, 0x15, 0x15)),
        Kind::Literal => (Color32::from_rgb(0xb5, 0xce, 0xa8), Color32::from_rgb(0x09, 0x86, 0x58)),
        Kind::Keyword => (Color32::from_rgb(0xc5, 0x86, 0xc0), Color32::from_rgb(0xaf, 0x00, 0xdb)),
        Kind::Builtin => (Color32::from_rgb(0x4e, 0xc9, 0xb0), Color32::from_rgb(0x26, 0x7f, 0x99)),
        Kind::Host => (Color32::from_rgb(0x56, 0x9c, 0xd6), Color32::from_rgb(0x00, 0x50, 0xb0)),
        Kind::Call => (Color32::from_rgb(0xdc, 0xdc, 0xaa), Color32::from_rgb(0x79, 0x5e, 0x26)),
    };
    if dark { dark_color } else { light_color }
}

pub fn layout_job(source: &str, font: FontId, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let tokens = tokenize(source);
    if tokens.iter().any(|(range, _)| source.get(range.clone()).is_none()) {
        job.append(source, 0.0, TextFormat::simple(font, color(Kind::Plain, dark)));
        return job;
    }
    for (range, kind) in tokens {
        job.append(&source[range], 0.0, TextFormat::simple(font.clone(), color(kind, dark)));
    }
    job
}

#[derive(Default)]
pub struct Highlighter {
    key: u64,
    job: Option<LayoutJob>,
}

impl Highlighter {
    pub fn layout(&mut self, ui: &egui::Ui, source: &str) -> LayoutJob {
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let dark = ui.visuals().dark_mode;
        let mut hasher = DefaultHasher::new();
        (source, dark, font.size.to_bits()).hash(&mut hasher);
        let key = hasher.finish();
        if self.key != key || self.job.is_none() {
            self.key = key;
            self.job = Some(layout_job(source, font, dark));
        }
        self.job.clone().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colored(source: &str) -> Vec<(&str, Kind)> {
        tokenize(source).into_iter().filter(|(_, k)| *k != Kind::Plain).map(|(r, k)| (&source[r], k)).collect()
    }

    #[test]
    fn tokens_cover_the_whole_text() {
        let source = "local s = `a {b} é` -- ü\nprint([==[x]] ]==], 0x1F, 1e-3)\n\"unterminated\nend";
        let mut at = 0;
        for (range, _) in tokenize(source) {
            assert_eq!(range.start, at);
            at = range.end;
        }
        assert_eq!(at, source.len());
    }

    #[test]
    fn colors_luau() {
        assert_eq!(
            colored("local x = 1 -- one\nif x ~= nil then continue end"),
            [
                ("local", Kind::Keyword),
                ("1", Kind::Literal),
                ("-- one", Kind::Comment),
                ("if", Kind::Keyword),
                ("nil", Kind::Literal),
                ("then", Kind::Keyword),
                ("continue", Kind::Keyword),
                ("end", Kind::Keyword),
            ]
        );
        assert_eq!(
            colored("click(b.center_x, 2) math.floor(1.5e+2) foo()"),
            [
                ("click", Kind::Host),
                ("2", Kind::Literal),
                ("math", Kind::Builtin),
                ("floor", Kind::Call),
                ("1.5e+2", Kind::Literal),
                ("foo", Kind::Call),
            ]
        );
    }

    #[test]
    fn strings_comments_and_interpolation() {
        assert_eq!(colored("--[[ a\n b ]] x"), [("--[[ a\n b ]]", Kind::Comment)]);
        assert_eq!(colored("s = [=[ ]] ]=]"), [("[=[ ]] ]=]", Kind::String)]);
        assert_eq!(colored(r#"s = "a \" b""#), [(r#""a \" b""#, Kind::String)]);
        assert_eq!(
            colored("s = `hp {hp + 1}!`"),
            [("`hp {", Kind::String), ("hp", Kind::Plain), ("1", Kind::Literal), ("}!`", Kind::String)]
                .into_iter()
                .filter(|(_, k)| *k != Kind::Plain)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn type_is_contextual() {
        assert_eq!(colored("type Point = {}"), [("type", Kind::Keyword)]);
        assert_eq!(colored("print(type(x))"), [("print", Kind::Host), ("type", Kind::Builtin)]);
    }
}
