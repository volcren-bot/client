use std::{
    collections::{BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    ops::Range,
};

use crate::highlight::{Kind, tokenize};

/// Every region that can be folded, as `(first line, last line)` with 0-based lines: `function`, `if`, `for`, `while`, `do`,
/// `repeat` blocks up to their `end`/`until`, and tables up to their `}`. Sorted by first line, one region per line (the longest).
pub fn ranges(source: &str) -> Vec<(usize, usize)> {
    #[derive(PartialEq)]
    enum Open {
        Block,
        Brace,
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut stack: Vec<(Open, usize)> = Vec::new();
    let mut line = 0;
    // the `do` of a `for` or `while` belongs to that loop, not to a block of its own
    let mut loop_do = false;
    for (range, kind) in tokenize(source) {
        let Some(text) = source.get(range) else { return Vec::new() };
        match kind {
            Kind::Keyword => {
                match text {
                    "function" | "if" | "repeat" => stack.push((Open::Block, line)),
                    "for" | "while" => {
                        stack.push((Open::Block, line));
                        loop_do = true;
                    }
                    "do" if loop_do => loop_do = false,
                    "do" => stack.push((Open::Block, line)),
                    "end" | "until" => {
                        if stack.last().is_some_and(|(open, _)| *open == Open::Block) {
                            let (_, start) = stack.pop().unwrap();
                            found.push((start, line));
                        }
                    }
                    _ => {}
                }
                line += text.matches('\n').count();
            }
            Kind::Plain => {
                for c in text.chars() {
                    match c {
                        '\n' => line += 1,
                        '{' => stack.push((Open::Brace, line)),
                        '}' if stack.last().is_some_and(|(open, _)| *open == Open::Brace) => {
                            let (_, start) = stack.pop().unwrap();
                            found.push((start, line));
                        }
                        _ => {}
                    }
                }
            }
            _ => line += text.matches('\n').count(),
        }
    }
    found.retain(|&(start, end)| end > start);
    found.sort_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
    found.dedup_by_key(|&mut (start, _)| start);
    found
}

/// What the editor shows of the text with the current folds.
pub struct View {
    /// The lines that are on screen, in order.
    pub visible: Vec<usize>,
    /// The bytes that stay in the text but take no room: from the end of each folded header to the end of the fold's last line.
    pub hidden: Vec<Range<usize>>,
    /// Each folded header on screen, with the byte where its text ends.
    pub headers: Vec<(usize, usize)>,
}

/// The editor's folds. The text never changes: folded lines are laid out at no height, so cursors and offsets stay valid.
#[derive(Default)]
pub struct Folds {
    path: String,
    seen: Option<(u64, usize)>,
    ranges: Vec<(usize, usize)>,
    folded: BTreeSet<usize>,
    /// Where the cursor was (a char index) after the last frame, and the line it is on.
    cursor: (usize, usize),
}

impl Folds {
    /// Brings the folds up to date with the text: they move with lines added or removed at the cursor, and go when their line stops being a fold.
    pub fn sync(&mut self, path: &str, text: &str) {
        if self.path != path {
            self.path = path.into();
            self.folded.clear();
            self.seen = None;
        }
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if self.seen.is_some_and(|(seen, _)| seen == hash) {
            return;
        }
        let lines = text.matches('\n').count() + 1;
        if let Some((_, before)) = self.seen.filter(|&(_, before)| before != lines) {
            let cursor = self.cursor.1;
            let moved: BTreeSet<usize> = if lines > before {
                let first = cursor.saturating_sub(lines - before);
                self.folded.iter().map(|&at| if at > first { at + lines - before } else { at }).collect()
            } else {
                let removed = before - lines;
                self.folded
                    .iter()
                    .filter(|&&at| at <= cursor || at > cursor + removed)
                    .map(|&at| if at > cursor { at - removed } else { at })
                    .collect()
            };
            self.folded = moved;
        }
        self.seen = Some((hash, lines));
        self.ranges = ranges(text);
        let starts = &self.ranges;
        self.folded.retain(|at| starts.binary_search_by_key(at, |&(start, _)| start).is_ok());
    }

    fn end_of(&self, line: usize) -> Option<usize> {
        self.ranges.binary_search_by_key(&line, |&(start, _)| start).ok().map(|i| self.ranges[i].1)
    }

    pub fn is_foldable(&self, line: usize) -> bool {
        self.end_of(line).is_some()
    }

    pub fn is_folded(&self, line: usize) -> bool {
        self.folded.contains(&line)
    }

    pub fn toggle(&mut self, line: usize) {
        if self.is_foldable(line) && !self.folded.remove(&line) {
            self.folded.insert(line);
        }
    }

    /// Opens every fold that hides `line`.
    pub fn reveal(&mut self, line: usize) {
        let ranges = &self.ranges;
        self.folded.retain(|&start| {
            let end = ranges.binary_search_by_key(&start, |&(s, _)| s).map_or(start, |i| ranges[i].1);
            !(start < line && line <= end)
        });
    }

    /// Notes where the cursor (a char index) is after a frame, and opens the fold it moved into.
    pub fn follow_cursor(&mut self, text: &str, index: usize) {
        if index == self.cursor.0 {
            return;
        }
        let line = text.chars().take(index).filter(|&c| c == '\n').count();
        self.cursor = (index, line);
        self.reveal(line);
    }

    pub fn view(&self, text: &str) -> View {
        let lines = text.matches('\n').count() + 1;
        let newlines: Vec<usize> = text.match_indices('\n').map(|(at, _)| at).collect();
        let mut view = View { visible: Vec::new(), hidden: Vec::new(), headers: Vec::new() };
        let mut line = 0;
        while line < lines {
            view.visible.push(line);
            let end = self.folded.contains(&line).then(|| self.end_of(line)).flatten();
            let Some(end) = end else {
                line += 1;
                continue;
            };
            let header_end = newlines.get(line).copied().unwrap_or(text.len());
            let hidden_end = newlines.get(end).copied().unwrap_or(text.len());
            view.headers.push((line, header_end));
            view.hidden.push(header_end..hidden_end);
            line = end + 1;
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_blocks_and_tables() {
        let source = "local function f(a)\n  if a then\n    return {\n      1,\n    }\n  end\nend\nfor i = 1, 3 do\n  print(i)\nend\n";
        assert_eq!(ranges(source), [(0, 6), (1, 5), (2, 4), (7, 9)]);
    }

    #[test]
    fn single_line_blocks_and_loop_do() {
        assert_eq!(ranges("if x then y() end\nwhile x do\nx()\nend"), [(1, 3)]);
        assert_eq!(ranges("do\nx()\nend\nrepeat\nx()\nuntil y"), [(0, 2), (3, 5)]);
    }

    #[test]
    fn braces_in_strings_and_comments_are_ignored() {
        assert!(ranges("local s = \"{\"\n-- {\nlocal t = `{`\n").is_empty());
    }

    #[test]
    fn hides_from_the_header_end_to_the_last_line() {
        let text = "function f()\n  x()\nend\ny()";
        let mut folds = Folds::default();
        folds.sync("a", text);
        folds.toggle(0);
        let view = folds.view(text);
        assert_eq!(view.visible, [0, 3]);
        assert_eq!(view.hidden, [12..text.find("\ny()").unwrap()]);
        assert_eq!(view.headers, [(0, 12)]);
    }

    #[test]
    fn folds_follow_edits_and_open_for_the_cursor() {
        let mut folds = Folds::default();
        folds.sync("a", "x = 1\nfunction f()\n  x()\nend");
        folds.toggle(1);
        // a line added above moves the fold down; the cursor sits on the new line
        folds.follow_cursor("x = 1\nfunction f()\n  x()\nend", 0);
        folds.cursor = (6, 1);
        folds.sync("a", "x = 1\ny = 2\nfunction f()\n  x()\nend");
        assert!(folds.is_folded(2));
        folds.follow_cursor("x = 1\ny = 2\nfunction f()\n  x()\nend", 25);
        assert!(!folds.is_folded(2));
    }
}
