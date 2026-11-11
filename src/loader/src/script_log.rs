use std::time::{Duration, Instant};

use injector::{ScriptPage, ScriptRun};

use crate::log_view::{Filtered, Log, Poller, View};
use crate::pipe::{Pipe, Rows};
use crate::script_names::Names;

pub const KEEP: usize = 10_000;
pub const POLL_EVERY: Duration = Duration::from_millis(250);
pub const DEFAULT_BLACKLIST: &str = "6697, 4730";

pub struct Poll {
    pub page: ScriptPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub run: ScriptRun,
    pub duration_us: Option<u64>,
}

pub fn duration_text(micros: u64) -> String {
    if micros < 1000 {
        format!("{micros} µs")
    } else {
        format!("{:.2} ms", micros as f64 / 1000.0)
    }
}

pub fn is_trigger(event_type: i32) -> bool {
    matches!(event_type, 10..=17 | 0x12 | 0x55 | 0x56)
}

impl Row {
    pub fn script_id(&self) -> Option<i32> {
        if is_trigger(self.run.event_type) {
            return None;
        }
        self.run.args.first()?.parse().ok()
    }

    #[cfg(test)]
    pub fn text(&self, names: &Names) -> String {
        let (name, args, took) = (self.name(names), self.args_text(), self.took());
        let took = if took.is_empty() {
            took
        } else {
            format!("  {took}")
        };
        let indent = "  ".repeat(self.run.depth as usize);
        if args.is_empty() {
            format!("{indent}{name}{took}")
        } else {
            format!("{indent}{name} ({args}){took}")
        }
    }

    pub fn name(&self, names: &Names) -> String {
        match self.script_id() {
            Some(id) => match names.name(id) {
                Some(name) => format!("{name} ({id})"),
                None => format!("Script {id}"),
            },
            None => format!("Trigger {}", self.run.event_type),
        }
    }

    pub fn args_text(&self) -> String {
        let rest = if self.script_id().is_some() {
            &self.run.args[1..]
        } else {
            &self.run.args[..]
        };
        let more = (self.run.arg_count as usize).saturating_sub(self.run.args.len());
        let mut args = rest.join(", ");
        if more > 0 {
            args.push_str(&format!(
                "{}… +{more}",
                if args.is_empty() { "" } else { ", " }
            ));
        }
        args
    }

    pub fn took(&self) -> String {
        self.duration_us.map_or(String::new(), duration_text)
    }

    pub fn tick_text(&self) -> String {
        self.run
            .tick
            .map_or_else(|| "?".to_owned(), |tick| tick.to_string())
    }

    pub fn indented_name(&self, names: &Names) -> String {
        format!(
            "{}{}",
            "  ".repeat(self.run.depth as usize),
            self.name(names)
        )
    }

    pub fn line(&self, names: &Names) -> String {
        format!(
            "{}\t{}\t{}\t{}",
            self.tick_text(),
            self.indented_name(names),
            self.args_text(),
            self.took()
        )
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Blacklist(Vec<i32>);

impl Blacklist {
    pub fn parse(text: &str, names: &Names) -> Self {
        Self(
            crate::packets::words(text)
                .flat_map(|word| {
                    word.parse()
                        .map_or_else(|_| names.ids(word).to_vec(), |id| vec![id])
                })
                .collect(),
        )
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
    pub names: Names,
    pub loading_names: bool,
    pub names_error: Option<String>,
    pub dropped: u64,
    pub error: Option<String>,
    pub pipe: Option<Pipe>,
    rows: Log<Row>,
    shown: Filtered<bool>,
    poller: Poller,
    next: u64,
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
            pipe: None,
            rows: Log::default(),
            shown: Filtered::default(),
            poller: Poller::default(),
            next: 0,
        }
    }
}

impl Inspector {
    pub fn kept(&self) -> usize {
        self.rows.len()
    }

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

    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.poller.due(self.enabled, pid, now)
    }

    pub fn start(&mut self, pid: u32) -> u64 {
        if self.poller.start(pid) {
            self.next = 0;
            self.clear();
        }
        self.next
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<Poll, String>) {
        if !self.poller.finish(pid, Some(now + POLL_EVERY)) {
            return;
        }
        match result {
            Ok(Poll { page }) => {
                self.error = None;
                self.next = page.next;
                self.dropped += page.dropped;
                if let Some(first) = page.runs.first() {
                    self.rows.restart_if_behind(first, |last, first| {
                        first.sequence <= last.run.sequence
                    });
                }
                let blacklist = Blacklist::parse(&self.blacklist, &self.names);
                for run in page.runs {
                    if run.end {
                        self.finish_run(&run);
                    } else {
                        let row = Row {
                            run,
                            duration_us: None,
                        };
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

    fn finish_run(&mut self, end: &ScriptRun) {
        for row in self.rows.iter_mut().rev() {
            if row.run.depth < end.depth {
                return;
            }
            if row.run.depth == end.depth {
                if row.duration_us.is_none()
                    && row.run.event_type == end.event_type
                    && row.run.args == end.args
                {
                    row.duration_us = Some(end.time_us.saturating_sub(row.run.time_us));
                }
                return;
            }
        }
    }

    pub fn visible(&self) -> View<'_, Row, bool> {
        let triggers = self.triggers;
        self.shown.view(&self.rows, triggers, |row| {
            triggers || row.script_id().is_some()
        })
    }
}

impl Rows for Inspector {
    fn end(&self) -> u64 {
        self.rows.end()
    }

    fn shown_text(&self) -> String {
        let visible = self.visible();
        (0..visible.len())
            .map(|index| visible[index].line(&self.names))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn write_pipe(&mut self) -> Result<(), String> {
        let Some(next) = self.pipe.as_ref().map(|pipe| pipe.next) else {
            return Ok(());
        };
        let rows: Vec<(u64, &Row)> = self.rows.since(next).collect();
        let mut lines = Vec::new();
        let mut piped = next;
        for (at, &(number, row)) in rows.iter().enumerate() {
            let over = row.duration_us.is_some()
                || rows[at + 1..]
                    .iter()
                    .any(|(_, later)| later.run.depth <= row.run.depth);
            if !over {
                break;
            }
            lines.push(row.line(&self.names));
            piped = number + 1;
        }
        if let Some(pipe) = &mut self.pipe {
            pipe.next = piped;
        }
        crate::pipe::write(&mut self.pipe, lines)
    }

    fn pipe(&mut self) -> &mut Option<Pipe> {
        &mut self.pipe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sequence: u64, event_type: i32, depth: u8, args: &[&str], arg_count: u16) -> ScriptRun {
        ScriptRun {
            sequence,
            time_us: sequence * 10,
            tick: Some(7),
            end: false,
            event_type,
            arg_count,
            depth,
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn script(sequence: u64, id: i32) -> ScriptRun {
        run(sequence, 76, 0, &[&id.to_string()], 1)
    }

    fn poll(runs: Vec<ScriptRun>, dropped: u64) -> Result<Poll, String> {
        Ok(Poll {
            page: ScriptPage {
                next: runs.last().map_or(0, |r| r.sequence + 1),
                runs,
                dropped,
            },
        })
    }

    fn names() -> Names {
        crate::script_names::names_for_test(&[(6697, "npc_name_timer")])
    }

    fn row(run: ScriptRun) -> Row {
        Row {
            run,
            duration_us: None,
        }
    }

    #[test]
    fn rows_name_the_script_and_show_its_other_arguments() {
        let (names, none) = (names(), Names::default());
        assert_eq!(row(script(0, 6697)).text(&none), "Script 6697");
        assert_eq!(row(script(0, 6697)).text(&names), "npc_name_timer (6697)");
        assert_eq!(row(script(0, 4730)).text(&names), "Script 4730");
        assert_eq!(
            row(run(0, 76, 0, &["4730", "-1", "\"…\""], 3)).text(&names),
            "Script 4730 (-1, \"…\")"
        );
        assert_eq!(
            row(run(0, 76, 2, &["4730", "5"], 11)).text(&names),
            "    Script 4730 (5, … +9)"
        );
        assert_eq!(
            row(run(0, 12, 0, &["393217"], 1)).text(&names),
            "Trigger 12 (393217)"
        );
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
        assert!(!list.hides(&row(run(0, 12, 0, &["6697"], 1))));
        assert!(!Blacklist::parse("", &Names::default()).hides(&row(script(0, 6697))));
        let list = Blacklist::parse("Npc_Name_Timer", &names());
        assert!(list.hides(&row(script(0, 6697))) && !list.hides(&row(script(0, 4730))));
        assert!(
            !Blacklist::parse("Npc_Name_Timer", &Names::default()).hides(&row(script(0, 6697)))
        );
    }

    #[test]
    fn blacklisted_scripts_are_dropped_on_arrival() {
        let now = Instant::now();
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        inspector.start(32);
        inspector.finish(
            32,
            now,
            poll(vec![script(0, 6697), script(1, 2), script(2, 4730)], 0),
        );
        assert_eq!(
            inspector
                .visible()
                .iter()
                .map(|r| r.run.sequence)
                .collect::<Vec<_>>(),
            [1]
        );
        inspector.blacklist.clear();
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(3, 6697)], 0));
        assert_eq!(inspector.kept(), 2);
    }

    #[test]
    fn the_trigger_box_hides_trigger_events() {
        let now = Instant::now();
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        inspector.start(32);
        inspector.finish(
            32,
            now,
            poll(vec![script(0, 1), run(1, 12, 0, &["5"], 1)], 0),
        );
        assert_eq!(inspector.visible().len(), 2);
        inspector.triggers = false;
        assert_eq!(inspector.visible().len(), 1);
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
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        inspector.start(32);
        let start = run(0, 76, 0, &["12"], 1);
        inspector.finish(32, now, poll(vec![start.clone()], 0));
        assert_eq!(inspector.visible()[0].duration_us, None);
        assert_eq!(inspector.visible()[0].text(&Names::default()), "Script 12");
        inspector.start(32);
        inspector.finish(32, now, poll(vec![ended(start, 1, 1250)], 0));
        assert_eq!(inspector.visible()[0].duration_us, Some(1250));
        assert_eq!(
            inspector.visible()[0].text(&Names::default()),
            "Script 12  1.25 ms"
        );
        assert_eq!(duration_text(850), "850 µs");
    }

    #[test]
    fn the_pipe_gets_every_new_row_once_its_run_is_over() {
        let now = Instant::now();
        let path =
            std::env::temp_dir().join(format!("volcren-script-pipe-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut inspector = Inspector {
            enabled: true,
            triggers: false,
            ..Inspector::default()
        };
        inspector.start(32);
        inspector.finish(32, now, poll(vec![script(0, 9)], 0));
        inspector.pipe = Some(Pipe::open(path.clone(), inspector.end()).unwrap());
        let (parent, child) = (run(1, 76, 0, &["1"], 1), run(2, 12, 1, &["5"], 1));
        inspector.start(32);
        inspector.finish(32, now, poll(vec![parent.clone(), child], 0));
        inspector.write_pipe().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        inspector.start(32);
        inspector.finish(32, now, poll(vec![ended(parent, 3, 1250), script(4, 8)], 0));
        inspector.write_pipe().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "7\tScript 1\t\t1.24 ms\n7\t  Trigger 12\t5\t\n"
        );
        drop(inspector);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn nested_ends_find_their_own_starts() {
        let now = Instant::now();
        let mut inspector = Inspector {
            enabled: true,
            blacklist: String::new(),
            ..Inspector::default()
        };
        inspector.start(32);
        let (parent, child_a, child_b) = (
            run(0, 76, 0, &["1"], 1),
            run(1, 76, 1, &["2"], 1),
            run(3, 76, 1, &["2"], 1),
        );
        inspector.finish(
            32,
            now,
            poll(
                vec![
                    parent.clone(),
                    child_a.clone(),
                    ended(child_a, 2, 100),
                    child_b.clone(),
                    ended(child_b, 4, 400),
                    ended(parent, 5, 900),
                ],
                0,
            ),
        );
        assert_eq!(
            inspector
                .visible()
                .iter()
                .map(|r| r.duration_us)
                .collect::<Vec<_>>(),
            [Some(900), Some(90), Some(370)]
        );
    }

    #[test]
    fn a_blacklisted_scripts_end_is_not_given_to_another_row() {
        let now = Instant::now();
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        inspector.start(32);
        let kept = run(0, 76, 0, &["12"], 1);
        let hidden = run(1, 76, 1, &["6697"], 1);
        inspector.finish(
            32,
            now,
            poll(
                vec![
                    kept.clone(),
                    hidden.clone(),
                    ended(hidden, 2, 50),
                    ended(kept, 3, 70),
                ],
                0,
            ),
        );
        assert_eq!(inspector.visible().len(), 1);
        assert_eq!(inspector.visible()[0].duration_us, Some(70));
    }

    #[test]
    fn an_end_with_no_start_kept_is_ignored() {
        let now = Instant::now();
        let mut inspector = Inspector {
            enabled: true,
            ..Inspector::default()
        };
        inspector.start(32);
        inspector.finish(
            32,
            now,
            poll(vec![ended(run(0, 76, 0, &["12"], 1), 1, 50)], 0),
        );
        assert_eq!(inspector.kept(), 0);
    }
}
