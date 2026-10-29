//! The Var Inspector on the Development page: watches the client's varps and logs what changes, as RuneLite's does.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use injector::{config::VarbitDef, VarcPage};

/// Entries kept for display; the oldest go first.
pub const KEEP: usize = 10_000;
/// How long after a read finished the next one starts.
pub const POLL_EVERY: Duration = Duration::from_millis(200);

/// The varbits stored in one varp: (id, definition, gameval name).
pub type Varbits = Vec<(i32, VarbitDef, Option<String>)>;

/// One read of the client: the varps, the game tick they were read at, the varbits of the varps that changed, and the client
/// variable changes the scripts made since the last read.
pub struct Poll {
    pub varps: Option<Vec<i32>>,
    pub tick: Option<i32>,
    pub varbits: Vec<(u16, Varbits)>,
    pub varcs: VarcPage,
    /// why the varc log couldn't be read, when it couldn't (the varps are still good)
    pub varc_error: Option<String>,
}

/// What a read of the client needs to know: the varps to compare with, the varps whose varbits are known and where to read the
/// varc log from.
pub struct Request {
    pub previous: Option<Vec<i32>>,
    pub known: Vec<u16>,
    pub varc_since: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Varbit,
    Varp,
    VarcInt,
    VarcStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub tick: Option<i32>,
    pub kind: Kind,
    pub name: String,
    pub old: String,
    pub new: String,
}

pub struct Inspector {
    pub enabled: bool,
    pub varbits: bool,
    pub varps: bool,
    pub varc_ints: bool,
    pub varc_strings: bool,
    pub follow: bool,
    pub error: Option<String>,
    pub varc_error: Option<String>,
    log: VecDeque<Entry>,
    /// The client the log is from; another client starts over.
    pid: Option<u32>,
    previous: Option<Vec<i32>>,
    /// Where to read the varc log from; none until the first read, which only skips what happened before the inspector was enabled.
    varc_next: Option<u64>,
    /// Varbits by varp, read from the cache once.
    known: HashMap<u16, Varbits>,
    reading: bool,
    next_poll: Option<Instant>,
}

impl Default for Inspector {
    fn default() -> Self {
        Self { enabled: false, varbits: true, varps: true, varc_ints: true, varc_strings: true, follow: true, error: None, varc_error: None, log: VecDeque::new(), pid: None, previous: None, varc_next: None, known: HashMap::new(), reading: false, next_poll: None }
    }
}

/// Replaces the bits of `def` in `varp` with `value`.
fn with_varbit(varp: i32, def: &VarbitDef, value: i32) -> i32 {
    let bits = (def.msb - def.lsb + 1) as u32;
    let mask = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
    ((varp as u32 & !(mask << def.lsb)) | ((value as u32 & mask) << def.lsb)) as i32
}

/// What changed from `old` to `new`. A changed varp logs its varbits that changed, then itself if bits are left that no
/// varbit explained, like RuneLite's inspector.
pub fn diff(old: &[i32], new: &[i32], tick: Option<i32>, varbits: impl Fn(u16) -> Varbits, varp_name: impl Fn(usize) -> Option<String>) -> Vec<Entry> {
    let mut entries = Vec::new();
    for (index, (&before, &after)) in old.iter().zip(new).enumerate() {
        if before == after {
            continue;
        }
        let mut explained = before;
        for (id, def, name) in varbits(index as u16) {
            let (was, is) = (def.value(before), def.value(after));
            if was != is {
                explained = with_varbit(explained, &def, is);
                entries.push(Entry { tick, kind: Kind::Varbit, name: name.unwrap_or_else(|| id.to_string()), old: was.to_string(), new: is.to_string() });
            }
        }
        if explained != after {
            let name = match varp_name(index) {
                Some(name) => format!("{name}({index})"),
                None => index.to_string(),
            };
            entries.push(Entry { tick, kind: Kind::Varp, name, old: explained.to_string(), new: after.to_string() });
        }
    }
    entries
}

impl Inspector {
    pub fn kept(&self) -> usize {
        self.log.len()
    }

    pub fn clear(&mut self) {
        self.log.clear();
    }

    /// Whether a read of `pid` should start now.
    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.enabled && (!self.reading || self.pid != Some(pid)) && self.next_poll.is_none_or(|at| now >= at)
    }

    /// Marks a read started and says what the job needs: the varps to compare with and the varps whose varbits are known.
    pub fn start(&mut self, pid: u32) -> Request {
        if self.pid != Some(pid) {
            self.pid = Some(pid);
            self.previous = None;
            self.varc_next = None;
            self.clear();
        }
        self.reading = true;
        Request { previous: self.previous.clone(), known: self.known.keys().copied().collect(), varc_since: self.varc_next.unwrap_or(0) }
    }

    /// The varps that differ and have no varbits read yet, for the job to look up.
    pub fn unknown(previous: &[i32], current: &[i32], known: &[u16]) -> Vec<u16> {
        previous.iter().zip(current).enumerate().filter(|&(i, (a, b))| a != b && !known.contains(&(i as u16))).map(|(i, _)| i as u16).collect()
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<Poll, String>, varp_name: impl Fn(usize) -> Option<String>) {
        self.reading = false;
        self.next_poll = Some(now + POLL_EVERY);
        if self.pid != Some(pid) {
            return;
        }
        match result {
            Ok(poll) => {
                self.error = None;
                self.known.extend(poll.varbits);
                self.varc_error = poll.varc_error;
                // the first read of the varc log is only a start: what it holds happened before the inspector was enabled
                if self.varc_next.is_some() {
                    self.log.extend(poll.varcs.changes.into_iter().map(|change| Entry {
                        tick: poll.tick,
                        kind: if change.string { Kind::VarcStr } else { Kind::VarcInt },
                        name: change.index.to_string(),
                        old: change.old.map_or("?".into(), |old| if change.string { format!("\"{old}\"") } else { old }),
                        new: if change.string { format!("\"{}\"", change.new) } else { change.new },
                    }));
                }
                self.varc_next = Some(poll.varcs.next);
                // logged out: the next login's varps are a fresh start, not a change
                if let Some(varps) = poll.varps {
                    if let Some(previous) = &self.previous {
                        let known = &self.known;
                        let found = diff(previous, &varps, poll.tick, |varp| known.get(&varp).cloned().unwrap_or_default(), varp_name);
                        self.log.extend(found);
                    }
                    self.previous = Some(varps);
                } else {
                    self.previous = None;
                }
                while self.log.len() > KEEP {
                    self.log.pop_front();
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The kept entries that pass the type boxes, oldest first.
    pub fn visible(&self) -> Vec<&Entry> {
        self.log
            .iter()
            .filter(|e| match e.kind {
                Kind::Varbit => self.varbits,
                Kind::Varp => self.varps,
                Kind::VarcInt => self.varc_ints,
                Kind::VarcStr => self.varc_strings,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(varp: u16, lsb: u8, msb: u8) -> VarbitDef {
        VarbitDef { varp, lsb, msb }
    }

    fn bits(index: u16) -> Varbits {
        if index == 1 { vec![(100, def(1, 0, 3), Some("MODE".into())), (101, def(1, 4, 4), None)] } else { vec![] }
    }

    fn name(index: usize) -> Option<String> {
        (index == 2).then(|| "Run".into())
    }

    #[test]
    fn a_varbit_change_hides_the_bits_it_explains() {
        let found = diff(&[0, 0x00, 0], &[0, 0x03, 0], Some(7), bits, name);
        assert_eq!(found, [Entry { tick: Some(7), kind: Kind::Varbit, name: "MODE".into(), old: "0".into(), new: "3".into() }]);
    }

    #[test]
    fn bits_no_varbit_explains_log_the_varp() {
        // bit 8 belongs to no varbit
        let found = diff(&[0, 0x03, 0], &[0, 0x103, 0], None, bits, name);
        assert_eq!(found, [Entry { tick: None, kind: Kind::Varp, name: "1".into(), old: "3".into(), new: "259".into() }]);
        let found = diff(&[0, 0, 1], &[0, 0, 0], None, bits, name);
        assert_eq!(found, [Entry { tick: None, kind: Kind::Varp, name: "Run(2)".into(), old: "1".into(), new: "0".into() }]);
    }

    #[test]
    fn unnamed_varbits_use_their_id_and_each_changed_one_is_logged() {
        let found = diff(&[0, 0], &[0, 0x13], None, bits, name);
        let names: Vec<_> = found.iter().map(|e| (e.name.as_str(), e.old.as_str(), e.new.as_str())).collect();
        assert_eq!(names, [("MODE", "0", "3"), ("101", "0", "1")]);
    }

    #[test]
    fn nothing_changed_nothing_logged() {
        assert!(diff(&[1, 2, 3], &[1, 2, 3], None, bits, name).is_empty());
    }

    fn poll(varps: Option<Vec<i32>>, varbits: Vec<(u16, Varbits)>) -> Result<Poll, String> {
        Ok(Poll { varps, tick: Some(5), varbits, varcs: VarcPage::default(), varc_error: None })
    }

    #[test]
    fn the_first_read_is_a_baseline_and_later_reads_log_changes() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        assert!(inspector.due(32, now));
        assert_eq!(inspector.start(32).previous, None);
        inspector.finish(32, now, poll(Some(vec![0, 0, 0]), vec![]), name);
        assert_eq!(inspector.kept(), 0);
        assert!(!inspector.due(32, now));
        assert!(inspector.due(32, now + POLL_EVERY));
        assert_eq!(inspector.start(32).previous, Some(vec![0, 0, 0]));
        inspector.finish(32, now, poll(Some(vec![0, 0, 1]), vec![]), name);
        assert_eq!(inspector.visible().len(), 1);
        // the varbits looked up are remembered
        let known = inspector.start(32).known;
        assert!(known.is_empty());
        inspector.finish(32, now, poll(Some(vec![0, 0x3, 1]), vec![(1, bits(1))]), name);
        assert_eq!(inspector.kept(), 2);
        assert_eq!(inspector.start(32).known, [1]);
    }

    #[test]
    fn logging_out_and_another_client_start_over() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![0]), vec![]), name);
        inspector.start(32);
        inspector.finish(32, now, poll(None, vec![]), name);
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![9]), vec![]), name);
        assert_eq!(inspector.kept(), 0);
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![8]), vec![]), name);
        assert_eq!(inspector.kept(), 1);
        assert_eq!(inspector.start(44).previous, None);
        assert_eq!(inspector.kept(), 0);
        // an answer for the client that was left is not mixed in
        inspector.finish(32, now, poll(Some(vec![1]), vec![]), name);
        assert_eq!(inspector.kept(), 0);
    }

    #[test]
    fn type_boxes_filter_the_log() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![0, 0, 0]), vec![(1, bits(1))]), name);
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![0, 0x103, 1]), vec![]), name);
        assert_eq!(inspector.visible().len(), 3);
        inspector.varps = false;
        assert_eq!(inspector.visible().iter().map(|e| e.kind).collect::<Vec<_>>(), [Kind::Varbit]);
    }

    #[test]
    fn finds_the_changed_varps_without_known_varbits() {
        assert_eq!(Inspector::unknown(&[1, 2, 3], &[1, 5, 6], &[2]), [1]);
    }

    fn varc(sequence: u64, index: i32, string: bool, old: Option<&str>, new: &str) -> injector::VarcChange {
        injector::VarcChange { sequence, time_ms: 0, index, string, old: old.map(Into::into), new: new.into() }
    }

    fn varc_poll(changes: Vec<injector::VarcChange>, next: u64) -> Result<Poll, String> {
        Ok(Poll { varps: None, tick: Some(9), varbits: vec![], varcs: VarcPage { changes, next, dropped: 0 }, varc_error: None })
    }

    #[test]
    fn varc_changes_are_logged_after_a_first_read_that_only_marks_the_place() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        assert_eq!(inspector.start(32).varc_since, 0);
        // what the log held before the inspector was enabled is not a change
        inspector.finish(32, now, varc_poll(vec![varc(0, 1, false, Some("0"), "5")], 40), name);
        assert_eq!(inspector.kept(), 0);
        assert_eq!(inspector.start(32).varc_since, 40);
        inspector.finish(32, now, varc_poll(vec![varc(40, 7, false, Some("-1"), "3"), varc(41, 8, true, None, "hi")], 42), name);
        let shown: Vec<_> = inspector.visible().iter().map(|e| (e.kind, e.name.clone(), e.old.clone(), e.new.clone(), e.tick)).collect();
        assert_eq!(
            shown,
            [
                (Kind::VarcInt, "7".into(), "-1".into(), "3".into(), Some(9)),
                (Kind::VarcStr, "8".into(), "?".into(), "\"hi\"".into(), Some(9)),
            ]
        );
        assert_eq!(inspector.start(32).varc_since, 42);
    }

    #[test]
    fn varcs_are_read_while_logged_out_and_the_type_boxes_filter_them() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, varc_poll(vec![], 0), name);
        inspector.start(32);
        inspector.finish(32, now, varc_poll(vec![varc(0, 1, false, Some("0"), "1"), varc(1, 2, true, Some("a"), "b")], 2), name);
        assert_eq!(inspector.visible().len(), 2);
        inspector.varc_strings = false;
        assert_eq!(inspector.visible().iter().map(|e| e.kind).collect::<Vec<_>>(), [Kind::VarcInt]);
        // another client starts the varc log over too
        assert_eq!(inspector.start(44).varc_since, 0);
        assert_eq!(inspector.kept(), 0);
    }

    #[test]
    fn an_unreadable_varc_log_is_reported_without_losing_the_varps() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, Ok(Poll { varps: Some(vec![0]), tick: None, varbits: vec![], varcs: VarcPage::default(), varc_error: Some("no varc log".into()) }), name);
        assert_eq!(inspector.varc_error.as_deref(), Some("no varc log"));
        assert_eq!(inspector.error, None);
        inspector.start(32);
        inspector.finish(32, now, poll(Some(vec![1]), vec![]), name);
        assert_eq!((inspector.varc_error.clone(), inspector.kept()), (None, 1));
    }
}
