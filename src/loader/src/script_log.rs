//! The Script Inspector on the Development page: the scripts the client ran for events, kept and filtered for display.

use std::time::{Duration, Instant};

use injector::{ScriptPage, ScriptRun};

use crate::log_view::{Filtered, Log, View};
use crate::script_names::Names;

/// Runs kept for display; the oldest go first.
pub const KEEP: usize = 10_000;
/// How long after a read finished the next one starts.
pub const POLL_EVERY: Duration = Duration::from_millis(250);
/// Scripts an idle logged in client runs every frame (seen on build a2636a01); logging them would fill the list in seconds.
pub const DEFAULT_BLACKLIST: &str = "6697, 4730";

/// One read of the client: the log entries since the last one.
pub struct Poll {
    pub page: ScriptPage,
}

/// A script that ran: its start entry (with the game tick it started in) and how long it took, once its end has been read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub run: ScriptRun,
    pub duration_us: Option<u64>,
}

/// `850 µs` or `1.25 ms`.
pub fn duration_text(micros: u64) -> String {
    if micros < 1000 { format!("{micros} µs") } else { format!("{:.2} ms", micros as f64 / 1000.0) }
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

    /// `npc_name_timer (6697) (1, 2)` or, for a trigger event, `Trigger 12 (…)`; nested runs are indented.
    /// The row's text on one line.
    #[cfg(test)]
    pub fn text(&self, names: &Names) -> String {
        let (name, args, took) = (self.name(names), self.args_text(), self.took());
        let took = if took.is_empty() { took } else { format!("  {took}") };
        let indent = "  ".repeat(self.run.depth as usize);
        if args.is_empty() { format!("{indent}{name}{took}") } else { format!("{indent}{name} ({args}){took}") }
    }

    /// `npc_name_timer (6697)`, `Script 123` for one without a (loaded) name, or `Trigger 7` for an event whose script id isn't known.
    pub fn name(&self, names: &Names) -> String {
        match self.script_id() {
            Some(id) => match names.name(id) {
                Some(name) => format!("{name} ({id})"),
                None => format!("Script {id}"),
            },
            None => format!("Trigger {}", self.run.event_type),
        }
    }

    /// The arguments after the script id, with how many more were left out.
    pub fn args_text(&self) -> String {
        let rest = if self.script_id().is_some() { &self.run.args[1..] } else { &self.run.args[..] };
        let more = (self.run.arg_count as usize).saturating_sub(self.run.args.len());
        let mut args = rest.join(", ");
        if more > 0 {
            args.push_str(&format!("{}… +{more}", if args.is_empty() { "" } else { ", " }));
        }
        args
    }

    /// How long the run took, once its end was read.
    pub fn took(&self) -> String {
        self.duration_us.map_or(String::new(), duration_text)
    }
}

/// Scripts to leave out of the list, from text like `6697, npc_name_timer`: ids, or script names once they are loaded (see `script_names`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Blacklist(Vec<i32>);

impl Blacklist {
    pub fn parse(text: &str, names: &Names) -> Self {
        let words = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|word| !word.is_empty());
        Self(words.flat_map(|word| word.parse().map_or_else(|_| names.ids(word).to_vec(), |id| vec![id])).collect())
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
    /// The script names, once Load names has fetched them.
    pub names: Names,
    /// Load names is downloading.
    pub loading_names: bool,
    pub names_error: Option<String>,
    /// Runs lost to the log's ring overwriting them before the loader read them, since the last clear.
    pub dropped: u64,
    pub error: Option<String>,
    rows: Log<Row>,
    /// The rows `visible` shows, for the trigger box they were picked with.
    shown: Filtered<bool>,
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
            names: Names::default(),
            loading_names: false,
            names_error: None,
            dropped: 0,
            error: None,
            rows: Log::default(),
            shown: Filtered::default(),
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

    /// The result of the download Load names started.
    pub fn finish_names(&mut self, result: Result<Names, String>) {
        self.loading_names = false;
        match result {
            Ok(names) => {
                self.names = names;
                self.names_error = None;
            }
            Err(error) => self.names_error = Some(error),
        }
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
            Ok(Poll { page }) => {
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
                let blacklist = Blacklist::parse(&self.blacklist, &self.names);
                for run in page.runs {
                    if run.end {
                        self.finish_run(&run);
                    } else {
                        let row = Row { run, duration_us: None };
                        if !blacklist.hides(&row) {
                            self.rows.push(row);
                        }
                    }
                }
                self.rows.keep(KEEP);
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// An end entry belongs to the latest unended row of its depth, unless a lower-depth row follows: then its start was left out.
    fn finish_run(&mut self, end: &ScriptRun) {
        for row in self.rows.iter_mut().rev() {
            if row.run.depth < end.depth {
                return;
            }
            if row.run.depth == end.depth {
                if row.duration_us.is_none() && row.run.event_type == end.event_type && row.run.args == end.args {
                    row.duration_us = Some(end.time_us.saturating_sub(row.run.time_us));
                }
                return;
            }
        }
    }

    /// The kept rows that pass the trigger box, oldest first.
    pub fn visible(&self) -> View<'_, Row, bool> {
        let triggers = self.triggers;
        self.shown.view(&self.rows, triggers, |row| triggers || row.script_id().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sequence: u64, event_type: i32, depth: u8, args: &[&str], arg_count: u16) -> ScriptRun {
        ScriptRun { sequence, time_us: sequence * 10, tick: Some(7), end: false, event_type, arg_count, depth, args: args.iter().map(|a| a.to_string()).collect() }
    }

    fn script(sequence: u64, id: i32) -> ScriptRun {
        run(sequence, 76, 0, &[&id.to_string()], 1)
    }

    fn poll(runs: Vec<ScriptRun>, dropped: u64, _tick: Option<i32>) -> Result<Poll, String> {
        Ok(Poll { page: ScriptPage { next: runs.last().map_or(0, |r| r.sequence + 1), runs, dropped } })
    }

    fn names() -> Names {
        crate::script_names::names_for_test(&[(6697, "npc_name_timer")])
    }

    fn row(run: ScriptRun) -> Row {
        Row { run, duration_us: None }
    }

    #[test]
    fn rows_name_the_script_and_show_its_other_arguments() {
        let (names, none) = (names(), Names::default());
        // without loaded names every script is its id
        assert_eq!(row(script(0, 6697)).text(&none), "Script 6697");
        assert_eq!(row(script(0, 6697)).text(&names), "npc_name_timer (6697)");
        assert_eq!(row(script(0, 4730)).text(&names), "Script 4730");
        assert_eq!(row(run(0, 76, 0, &["4730", "-1", "\"…\""], 3)).text(&names), "Script 4730 (-1, \"…\")");
        // nested runs are indented, and arguments the log didn't keep are counted
        assert_eq!(row(run(0, 76, 2, &["4730", "5"], 11)).text(&names), "    Script 4730 (5, … +9)");
        assert_eq!(row(run(0, 12, 0, &["393217"], 1)).text(&names), "Trigger 12 (393217)");
        assert_eq!(row(run(0, 76, 0, &[], 0)).text(&names), "Trigger 76");
    }

    #[test]
    fn trigger_events_have_no_script_id() {
        assert_eq!(row(script(0, 5)).script_id(), Some(5));
        assert_eq!(row(run(0, 10, 0, &["5"], 1)).script_id(), None);
        assert_eq!(row(run(0, 0x55, 0, &["5"], 1)).script_id(), None);
        assert!(is_trigger(17) && is_trigger(0x12) && !is_trigger(19) && !is_trigger(76));
    }

    #[test]
    fn blacklist_text_hides_script_ids_and_names_and_ignores_the_rest() {
        let list = Blacklist::parse("6697, 4730 abc", &Names::default());
        assert!(list.hides(&row(script(0, 6697))) && list.hides(&row(script(0, 4730))));
        assert!(!list.hides(&row(script(0, 1))));
        // a trigger event's first argument is not a script id
        assert!(!list.hides(&row(run(0, 12, 0, &["6697"], 1))));
        assert!(!Blacklist::parse("", &Names::default()).hides(&row(script(0, 6697))));
        // a script's name stands for its id, in any case
        let list = Blacklist::parse("Npc_Name_Timer", &names());
        assert!(list.hides(&row(script(0, 6697))) && !list.hides(&row(script(0, 4730))));
        // until the names are loaded a name hides nothing
        assert!(!Blacklist::parse("Npc_Name_Timer", &Names::default()).hides(&row(script(0, 6697))));
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
        assert_eq!(inspector.visible()[0].run.tick, Some(7));
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

    fn ended(mut run: ScriptRun, sequence: u64, time_us: u64) -> ScriptRun {
        run.sequence = sequence;
        run.time_us = time_us;
        run.end = true;
        run
    }

    #[test]
    fn an_end_entry_gives_its_start_a_duration() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        let start = run(0, 76, 0, &["12"], 1);
        inspector.finish(32, now, poll(vec![start.clone()], 0, None));
        assert_eq!(inspector.visible()[0].duration_us, None);
        assert_eq!(inspector.visible()[0].text(&Names::default()), "Script 12");
        // the end can arrive in a later read
        inspector.start(32);
        inspector.finish(32, now, poll(vec![ended(start, 1, 1250)], 0, None));
        assert_eq!(inspector.visible()[0].duration_us, Some(1250));
        assert_eq!(inspector.visible()[0].text(&Names::default()), "Script 12  1.25 ms");
        assert_eq!(duration_text(850), "850 µs");
    }

    #[test]
    fn nested_ends_find_their_own_starts() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, blacklist: String::new(), ..Inspector::default() };
        inspector.start(32);
        let (parent, child_a, child_b) = (run(0, 76, 0, &["1"], 1), run(1, 76, 1, &["2"], 1), run(3, 76, 1, &["2"], 1));
        inspector.finish(
            32,
            now,
            poll(vec![parent.clone(), child_a.clone(), ended(child_a, 2, 100), child_b.clone(), ended(child_b, 4, 400), ended(parent, 5, 900)], 0, None),
        );
        // times are in microseconds since the start of the log: starts at 0, 10 and 30
        assert_eq!(inspector.visible().iter().map(|r| r.duration_us).collect::<Vec<_>>(), [Some(900), Some(90), Some(370)]);
    }

    #[test]
    fn a_blacklisted_scripts_end_is_not_given_to_another_row() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        let kept = run(0, 76, 0, &["12"], 1);
        let hidden = run(1, 76, 1, &["6697"], 1);
        // the hidden child ends inside the kept parent; the parent's own end still lands on the parent
        inspector.finish(32, now, poll(vec![kept.clone(), hidden.clone(), ended(hidden, 2, 50), ended(kept, 3, 70)], 0, None));
        assert_eq!(inspector.visible().len(), 1);
        assert_eq!(inspector.visible()[0].duration_us, Some(70));
    }

    #[test]
    fn an_end_with_no_start_kept_is_ignored() {
        let now = Instant::now();
        let mut inspector = Inspector { enabled: true, ..Inspector::default() };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![ended(run(0, 76, 0, &["12"], 1), 1, 50)], 0, None));
        assert_eq!(inspector.kept(), 0);
    }
}
