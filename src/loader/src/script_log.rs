//! The Script Inspector on the Development page: the scripts the client ran for events, kept and filtered for display.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use injector::{ScriptPage, ScriptRun};

/// Runs kept for display; the oldest go first.
pub const KEEP: usize = 10_000;
/// How long after a read finished the next one starts.
pub const POLL_EVERY: Duration = Duration::from_millis(250);
/// Scripts an idle logged in client runs every frame (seen on build a2636a01); logging them would fill the list in seconds.
pub const DEFAULT_BLACKLIST: &str = "6697, 4730";

/// One read of the client: the runs since the last one, and the game tick at the time.
pub struct Poll {
    pub page: ScriptPage,
    pub tick: Option<i32>,
}

/// A logged run with the game tick of the read that found it, so up to a poll (a fraction of a tick) late.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub tick: Option<i32>,
    pub run: ScriptRun,
}

/// Event types whose script is found by trigger (components, menus): argument 0 is not the script id.
pub fn is_trigger(event_type: i32) -> bool {
    matches!(event_type, 10..=17 | 0x12 | 0x55 | 0x56)
}

impl Row {
    /// The script's id, for an event that carries one.
    pub fn script_id(&self) -> Option<i32> {
        if is_trigger(self.run.event_type) {
            return None;
        }
        self.run.args.first()?.parse().ok()
    }

    /// `Script 6697 (1, 2)` or, for a trigger event, `Trigger 12 (…)`; nested runs are indented.
    pub fn text(&self) -> String {
        let indent = "  ".repeat(self.run.depth as usize);
        let (name, rest) = match self.script_id() {
            Some(id) => (format!("Script {id}"), &self.run.args[1..]),
            None => (format!("Trigger {}", self.run.event_type), &self.run.args[..]),
        };
        let more = (self.run.arg_count as usize).saturating_sub(self.run.args.len());
        let mut args = rest.join(", ");
        if more > 0 {
            args.push_str(&format!("{}… +{more}", if args.is_empty() { "" } else { ", " }));
        }
        if args.is_empty() { format!("{indent}{name}") } else { format!("{indent}{name} ({args})") }
    }
}

/// Script ids to leave out of the list, from text like `6697, 4730`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Blacklist(Vec<i32>);

impl Blacklist {
    pub fn parse(text: &str) -> Self {
        Self(text.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|word| word.parse().ok()).collect())
    }

    pub fn hides(&self, row: &Row) -> bool {
        row.script_id().is_some_and(|id| self.0.contains(&id))
    }
}

pub struct Inspector {
    pub enabled: bool,
    pub follow: bool,
    pub triggers: bool,
    pub blacklist: String,
    /// Runs lost to the log's ring overwriting them before the loader read them, since the last clear.
    pub dropped: u64,
    pub error: Option<String>,
    rows: VecDeque<Row>,
    /// The client the rows are from, and the sequence number to ask it for next.
    pid: Option<u32>,
    next: u64,
    reading: bool,
    next_poll: Option<Instant>,
}

impl Default for Inspector {
    fn default() -> Self {
        Self {
            enabled: false,
            follow: true,
            triggers: true,
            blacklist: DEFAULT_BLACKLIST.into(),
            dropped: 0,
            error: None,
            rows: VecDeque::new(),
            pid: None,
            next: 0,
            reading: false,
            next_poll: None,
        }
    }
}

impl Inspector {
    pub fn kept(&self) -> usize {
        self.rows.len()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.dropped = 0;
    }

    /// Whether a read of `pid`'s log should start now.
    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.enabled && (!self.reading || self.pid != Some(pid)) && self.next_poll.is_none_or(|at| now >= at)
    }

    /// Marks a read started and says which sequence number to ask for. Another client than last time starts from its oldest run.
    pub fn start(&mut self, pid: u32) -> u64 {
        if self.pid != Some(pid) {
            self.pid = Some(pid);
            self.next = 0;
            self.clear();
        }
        self.reading = true;
        self.next
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<Poll, String>) {
        self.reading = false;
        self.next_poll = Some(now + POLL_EVERY);
        if self.pid != Some(pid) {
            return;
        }
        match result {
            Ok(Poll { page, tick }) => {
                self.error = None;
                self.next = page.next;
                self.dropped += page.dropped;
                // sequence numbers going backwards mean the client has a new agent with a new log
                if let (Some(last), Some(first)) = (self.rows.back(), page.runs.first())
                    && first.sequence <= last.run.sequence
                {
                    self.rows.clear();
                }
                // the blacklist applies as runs arrive, so the scripts it names never push the others out of the list
                let blacklist = Blacklist::parse(&self.blacklist);
                self.rows.extend(page.runs.into_iter().map(|run| Row { tick, run }).filter(|row| !blacklist.hides(row)));
                while self.rows.len() > KEEP {
                    self.rows.pop_front();
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The kept rows that pass the trigger box, oldest first.
    pub fn visible(&self) -> Vec<&Row> {
        self.rows.iter().filter(|row| self.triggers || row.script_id().is_some()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sequence: u64, event_type: i32, depth: u8, args: &[&str], arg_count: u16) -> ScriptRun {
        ScriptRun { sequence, time_ms: sequence * 10, event_type, arg_count, depth, args: args.iter().map(|a| a.to_string()).collect() }
    }

    fn script(sequence: u64, id: i32) -> ScriptRun {
        run(sequence, 76, 0, &[&id.to_string()], 1)
    }

    fn poll(runs: Vec<ScriptRun>, dropped: u64, tick: Option<i32>) -> Result<Poll, String> {
        Ok(Poll { page: ScriptPage { next: runs.last().map_or(0, |r| r.sequence + 1), runs, dropped }, tick })
    }

    fn row(run: ScriptRun) -> Row {
        Row { tick: None, run }
    }

    #[test]
    fn rows_name_the_script_and_show_its_other_arguments() {
        assert_eq!(row(script(0, 6697)).text(), "Script 6697");
        assert_eq!(row(run(0, 76, 0, &["12", "-1", "\"…\""], 3)).text(), "Script 12 (-1, \"…\")");
        // nested runs are indented, and arguments the log didn't keep are counted
        assert_eq!(row(run(0, 76, 2, &["12", "5"], 11)).text(), "    Script 12 (5, … +9)");
        assert_eq!(row(run(0, 12, 0, &["393217"], 1)).text(), "Trigger 12 (393217)");
        assert_eq!(row(run(0, 76, 0, &[], 0)).text(), "Trigger 76");
    }

    #[test]
    fn trigger_events_have_no_script_id() {
        assert_eq!(row(script(0, 5)).script_id(), Some(5));
        assert_eq!(row(run(0, 10, 0, &["5"], 1)).script_id(), None);
        assert_eq!(row(run(0, 0x55, 0, &["5"], 1)).script_id(), None);
        assert!(is_trigger(17) && is_trigger(0x12) && !is_trigger(19) && !is_trigger(76));
    }

    #[test]
    fn blacklist_text_hides_script_ids_and_ignores_the_rest() {
        let list = Blacklist::parse("6697, 4730 abc");
        assert!(list.hides(&row(script(0, 6697))) && list.hides(&row(script(0, 4730))));
        assert!(!list.hides(&row(script(0, 1))));
        // a trigger event's first argument is not a script id
        assert!(!list.hides(&row(run(0, 12, 0, &["6697"], 1))));
        assert!(!Blacklist::parse("").hides(&row(script(0, 6697))));
    }

    #[test]
    fn polls_only_when_enabled_and_idle_and_resumes_where_it_stopped() {
        let now = Instant::now();
        let mut inspector = Inspector::default();
        assert!(!inspector.due(32, now));
        inspector.enabled = true;
        assert!(inspector.due(32, now));
        assert_eq!(inspector.start(32), 0);
        assert!(!inspector.due(32, now));
        inspector.finish(32, now, poll(vec![script(0, 1), script(1, 2)], 0, Some(7)));
        assert!(!inspector.due(32, now));
        assert!(inspector.due(32, now + POLL_EVERY));
        assert_eq!(inspector.start(32), 2);
        assert_eq!(inspector.kept(), 2);
        assert_eq!(inspector.visible()[0].tick, Some(7));
    }

    #[test]
    fn blacklisted_scripts_are_dropped_on_arrival() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(0, 6697), script(1, 2), script(2, 4730)], 0, None));
        assert_eq!(inspector.visible().iter().map(|r| r.run.sequence).collect::<Vec<_>>(), [1]);
        inspector.blacklist.clear();
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(3, 6697)], 0, None));
        assert_eq!(inspector.kept(), 2);
    }

    #[test]
    fn another_client_starts_over() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(7, 1)], 3, None));
        assert_eq!((inspector.kept(), inspector.dropped), (1, 3));
        assert_eq!(inspector.start(44), 0);
        assert_eq!((inspector.kept(), inspector.dropped), (0, 0));
        // an answer for the client that was left is not mixed in
        inspector.finish(32, now, poll(vec![script(8, 1)], 0, None));
        assert_eq!(inspector.kept(), 0);
    }

    #[test]
    fn a_reloaded_agent_replaces_the_old_runs() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(100, 1), script(101, 1)], 0, None));
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(0, 2)], 0, None));
        assert_eq!(inspector.visible().iter().map(|r| r.script_id()).collect::<Vec<_>>(), [Some(2)]);
    }

    #[test]
    fn keeps_only_the_newest_runs() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, blacklist: String::new(), ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll((0..KEEP as u64 + 10).map(|i| script(i, 1)).collect(), 0, None));
        assert_eq!(inspector.kept(), KEEP);
        assert_eq!(inspector.visible()[0].run.sequence, 10);
    }

    #[test]
    fn the_trigger_box_hides_trigger_events() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(0, 1), run(1, 12, 0, &["5"], 1)], 0, None));
        assert_eq!(inspector.visible().len(), 2);
        inspector.triggers = false;
        assert_eq!(inspector.visible().len(), 1);
    }

    #[test]
    fn errors_are_kept_until_the_next_good_read() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, Err("no log".into()));
        assert_eq!(inspector.error.as_deref(), Some("no log"));
        inspector.start(32);
        inspector.finish(32, now, poll(vec![], 0, None));
        assert_eq!(inspector.error, None);
    }
}
