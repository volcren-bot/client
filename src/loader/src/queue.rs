use std::time::{Duration, Instant};

use injector::form::{BreakPlan, Exit, ExitStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const STORAGE_KEY: &str = "queues";
pub const MAX_ENTRIES: usize = 100;
pub const MAX_SAVED: usize = 50;
pub const MAX_MINUTES: u32 = 24 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WhenFinished {
    #[default]
    LogOut,
    StayLoggedIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OnChange {
    #[default]
    Stop,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub counter: String,
    pub target: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub script_id: String,
    pub title: String,
    #[serde(default)]
    pub settings: Map<String, Value>,
    #[serde(default)]
    pub spec_hash: Option<String>,
    #[serde(default)]
    pub goal: Option<Goal>,
    #[serde(default)]
    pub min_minutes: Option<u32>,
    #[serde(default)]
    pub max_minutes: Option<u32>,
}

impl Entry {
    pub fn new(script_id: String, title: String) -> Self {
        Self {
            script_id,
            title,
            settings: Map::new(),
            spec_hash: None,
            goal: None,
            min_minutes: None,
            max_minutes: None,
        }
    }

    pub fn limits_problem(&self) -> Option<String> {
        let (min, max) = (self.min_minutes, self.max_minutes);
        if [min, max].into_iter().flatten().any(|m| m > MAX_MINUTES) {
            return Some(format!("{}: at most {MAX_MINUTES} minutes.", self.title));
        }
        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            return Some(format!(
                "{}: the minimum time is longer than the maximum.",
                self.title
            ));
        }
        if self.goal.as_ref().is_some_and(|g| g.target.is_nan() || g.target <= 0.0) {
            return Some(format!("{}: a goal needs a number above 0.", self.title));
        }
        None
    }

    pub fn should_finish(&self, elapsed: Duration, counter: Option<f64>) -> bool {
        let minutes = |m: u32| Duration::from_secs(u64::from(m) * 60);
        if self.max_minutes.is_some_and(|max| elapsed >= minutes(max)) {
            return true;
        }
        let goal_met = self
            .goal
            .as_ref()
            .is_none_or(|goal| counter.is_some_and(|value| value >= goal.target));
        let has_end = self.goal.is_some() || self.min_minutes.is_some();
        has_end && goal_met && elapsed >= minutes(self.min_minutes.unwrap_or(0))
    }

    pub fn most_cost(&self, price_per_hour: i64) -> Option<i64> {
        self.max_minutes
            .map(|max| (price_per_hour * i64::from(max) + 59) / 60)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    pub name: String,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub repeat: bool,
    #[serde(default)]
    pub when_finished: WhenFinished,
    #[serde(default)]
    pub on_change: OnChange,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            name: "Queue".into(),
            entries: Vec::new(),
            repeat: false,
            when_finished: WhenFinished::default(),
            on_change: OnChange::default(),
        }
    }
}

impl Queue {
    pub fn problem(&self) -> Option<String> {
        if self.entries.is_empty() {
            return Some("Add at least one script.".into());
        }
        if self.entries.len() > MAX_ENTRIES {
            return Some(format!("A queue holds at most {MAX_ENTRIES} entries."));
        }
        if self.repeat
            && self
                .entries
                .iter()
                .all(|e| e.goal.is_none() && e.min_minutes.is_none() && e.max_minutes.is_none())
        {
            return Some("A repeating queue needs a goal or a time limit on an entry.".into());
        }
        self.entries.iter().find_map(Entry::limits_problem)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub current: Queue,
    #[serde(default)]
    pub saved: Vec<Queue>,
}

impl Saved {
    pub fn save_as(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the queue a name.".into());
        }
        let mut queue = self.current.clone();
        queue.name = name.to_owned();
        self.current.name = queue.name.clone();
        match self.saved.iter().position(|q| q.name == queue.name) {
            Some(index) => self.saved[index] = queue,
            None if self.saved.len() >= MAX_SAVED => {
                return Err(format!("At most {MAX_SAVED} saved queues."));
            }
            None => self.saved.push(queue),
        }
        self.saved.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }

    pub fn load(&mut self, name: &str) -> bool {
        match self.saved.iter().find(|q| q.name == name) {
            Some(queue) => {
                self.current = queue.clone();
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self, name: &str) {
        self.saved.retain(|q| q.name != name);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Blocked,
    Failed,
    Stopped,
}

pub fn outcome(result: &Result<(), String>, exit: Option<&Exit>, killed: bool, stopped: bool) -> Outcome {
    if killed {
        return Outcome::Failed;
    }
    if stopped {
        return Outcome::Stopped;
    }
    match (result, exit.map(|e| e.status)) {
        (Err(_), _) => Outcome::Failed,
        (Ok(()), Some(ExitStatus::Blocked)) => Outcome::Blocked,
        (Ok(()), _) => Outcome::Done,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Entry { index: usize, pass: u32 },
    End,
}

pub fn next(len: usize, index: usize, pass: u32, repeat: bool) -> Next {
    if index + 1 < len {
        Next::Entry {
            index: index + 1,
            pass,
        }
    } else if repeat && len > 0 {
        Next::Entry {
            index: 0,
            pass: pass + 1,
        }
    } else {
        Next::End
    }
}

pub fn resumed(plan: &BreakPlan, reported: Instant, now: Instant) -> BreakPlan {
    let passed = now.saturating_duration_since(reported).as_secs_f64() * 1000.0;
    let shift = |b: &injector::form::PlannedBreak| injector::form::PlannedBreak {
        remaining: (b.remaining - passed).max(0.0),
        ..b.clone()
    };
    BreakPlan {
        breaks: plan.breaks.iter().map(shift).collect(),
        sleeps: plan.sleeps.iter().map(shift).collect(),
        active: plan
            .active
            .as_ref()
            .map(|a| injector::form::ActiveBreak {
                kind: a.kind.clone(),
                remaining: (a.remaining - passed).max(0.0),
            })
            .filter(|a| a.remaining > 0.0),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Starting,
    Running { run: u64, finish_sent: Option<Instant> },
    LoggingOut { run: u64 },
}

pub struct Runner {
    pub queue: Queue,
    pub pid: u32,
    pub index: usize,
    pub pass: u32,
    pub phase: Phase,
    pub approved: std::collections::HashMap<String, i64>,
    pub resume: Option<(BreakPlan, Instant)>,
    pub started: Instant,
    pub killed: bool,
    pub finishing: bool,
}

impl Runner {
    pub fn new(queue: Queue, pid: u32, approved: std::collections::HashMap<String, i64>) -> Self {
        Self {
            queue,
            pid,
            index: 0,
            pass: 1,
            phase: Phase::Starting,
            approved,
            resume: None,
            started: Instant::now(),
            killed: false,
            finishing: false,
        }
    }

    pub fn entry(&self) -> Option<&Entry> {
        self.queue.entries.get(self.index)
    }

    pub fn run_id(&self) -> Option<u64> {
        match self.phase {
            Phase::Running { run, .. } | Phase::LoggingOut { run } => Some(run),
            Phase::Starting => None,
        }
    }

    pub fn progress(&self) -> String {
        let len = self.queue.entries.len();
        let title = self.entry().map_or("", |e| e.title.as_str());
        let pass = if self.queue.repeat {
            format!(", pass {}", self.pass)
        } else {
            String::new()
        };
        match self.phase {
            Phase::LoggingOut { .. } => format!("{}: logging out", self.queue.name),
            _ => format!(
                "{}: {} of {len}, {title}{pass}",
                self.queue.name,
                self.index + 1
            ),
        }
    }

    pub fn advance(&mut self) -> bool {
        match next(
            self.queue.entries.len(),
            self.index,
            self.pass,
            self.queue.repeat,
        ) {
            Next::Entry { index, pass } => {
                self.index = index;
                self.pass = pass;
                self.phase = Phase::Starting;
                self.killed = false;
                true
            }
            Next::End => false,
        }
    }

    pub fn grace(&self, finish_grace_minutes: u32) -> Option<Duration> {
        match self.phase {
            Phase::Running {
                finish_sent: Some(at),
                ..
            } => Some(
                Duration::from_secs(u64::from(finish_grace_minutes) * 60)
                    .saturating_sub(at.elapsed()),
            ),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry::new("id".into(), "Bosses".into())
    }

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn an_entry_without_limits_runs_until_the_script_ends() {
        assert!(!entry().should_finish(MIN * 1000, Some(1e9)));
    }

    #[test]
    fn a_goal_ends_the_entry_once_reached_but_not_before_the_minimum() {
        let mut e = entry();
        e.goal = Some(Goal {
            counter: "kills".into(),
            target: 20.0,
        });
        assert!(!e.should_finish(MIN, Some(19.0)));
        assert!(!e.should_finish(MIN, None));
        assert!(e.should_finish(MIN, Some(20.0)));
        e.min_minutes = Some(30);
        assert!(!e.should_finish(MIN * 29, Some(25.0)));
        assert!(e.should_finish(MIN * 30, Some(25.0)));
    }

    #[test]
    fn the_maximum_ends_the_entry_whatever_the_goal() {
        let mut e = entry();
        e.goal = Some(Goal {
            counter: "kills".into(),
            target: 20.0,
        });
        e.max_minutes = Some(60);
        assert!(!e.should_finish(MIN * 59, Some(3.0)));
        assert!(e.should_finish(MIN * 60, Some(3.0)));
    }

    #[test]
    fn a_minimum_alone_is_how_long_the_entry_runs() {
        let mut e = entry();
        e.min_minutes = Some(45);
        assert!(!e.should_finish(MIN * 44, None));
        assert!(e.should_finish(MIN * 45, None));
    }

    #[test]
    fn limits_are_checked() {
        let mut e = entry();
        e.min_minutes = Some(50);
        e.max_minutes = Some(40);
        assert!(e.limits_problem().is_some());
        e.max_minutes = Some(MAX_MINUTES + 1);
        assert!(e.limits_problem().is_some());
        e.max_minutes = Some(60);
        assert!(e.limits_problem().is_none());
        e.goal = Some(Goal {
            counter: "kills".into(),
            target: 0.0,
        });
        assert!(e.limits_problem().is_some());
    }

    #[test]
    fn most_cost_rounds_up_and_needs_a_maximum() {
        let mut e = entry();
        assert_eq!(e.most_cost(1000), None);
        e.max_minutes = Some(90);
        assert_eq!(e.most_cost(1000), Some(1500));
        e.max_minutes = Some(1);
        assert_eq!(e.most_cost(1000), Some(17));
    }

    #[test]
    fn outcomes_follow_the_exit_and_how_the_process_ended() {
        let blocked = Exit {
            status: ExitStatus::Blocked,
            reason: None,
        };
        assert_eq!(outcome(&Ok(()), None, false, false), Outcome::Done);
        assert_eq!(outcome(&Ok(()), Some(&blocked), false, false), Outcome::Blocked);
        assert_eq!(outcome(&Err("boom".into()), None, false, false), Outcome::Failed);
        assert_eq!(outcome(&Ok(()), None, true, true), Outcome::Failed);
        assert_eq!(outcome(&Ok(()), None, false, true), Outcome::Stopped);
    }

    #[test]
    fn next_moves_on_and_repeats_when_asked() {
        assert_eq!(next(3, 0, 1, false), Next::Entry { index: 1, pass: 1 });
        assert_eq!(next(3, 2, 1, false), Next::End);
        assert_eq!(next(3, 2, 1, true), Next::Entry { index: 0, pass: 2 });
        assert_eq!(next(0, 0, 1, true), Next::End);
    }

    #[test]
    fn a_resumed_plan_counts_the_time_since_it_was_reported() {
        let reported = Instant::now();
        let plan = BreakPlan {
            breaks: vec![
                injector::form::PlannedBreak {
                    remaining: 90_000.0,
                    length: 1.0,
                    at: None,
                    index: 0,
                },
                injector::form::PlannedBreak {
                    remaining: 10_000.0,
                    length: 1.0,
                    at: None,
                    index: 1,
                },
            ],
            sleeps: Vec::new(),
            active: Some(injector::form::ActiveBreak {
                kind: "break".into(),
                remaining: 5_000.0,
            }),
        };
        let resumed = resumed(&plan, reported, reported + Duration::from_secs(30));
        assert_eq!(resumed.breaks[0].remaining, 60_000.0);
        assert_eq!(resumed.breaks[1].remaining, 0.0);
        assert!(resumed.active.is_none());
    }

    #[test]
    fn saved_queues_are_named_replaced_loaded_and_deleted() {
        let mut saved = Saved::default();
        saved.current.entries.push(entry());
        assert!(saved.save_as("  ").is_err());
        saved.save_as("Daily").unwrap();
        saved.current.repeat = true;
        saved.save_as("Daily").unwrap();
        assert_eq!(saved.saved.len(), 1);
        assert!(saved.saved[0].repeat);
        saved.current = Queue::default();
        assert!(saved.load("Daily"));
        assert_eq!(saved.current.entries.len(), 1);
        assert!(!saved.load("Nope"));
        saved.delete("Daily");
        assert!(saved.saved.is_empty());
    }

    #[test]
    fn a_repeating_queue_needs_something_that_ends_its_entries() {
        let mut queue = Queue::default();
        assert!(queue.problem().is_some());
        queue.entries.push(entry());
        assert!(queue.problem().is_none());
        queue.repeat = true;
        assert!(queue.problem().is_some());
        queue.entries[0].max_minutes = Some(30);
        assert!(queue.problem().is_none());
    }

    #[test]
    fn the_runner_walks_the_entries_and_reports_progress() {
        let queue = Queue {
            entries: vec![entry(), entry()],
            ..Default::default()
        };
        let mut runner = Runner::new(queue, 7, Default::default());
        assert_eq!(runner.progress(), "Queue: 1 of 2, Bosses");
        assert!(runner.advance());
        assert_eq!(runner.index, 1);
        assert!(!runner.advance());
        runner.queue.repeat = true;
        assert!(runner.advance());
        assert_eq!((runner.index, runner.pass), (0, 2));
        assert_eq!(runner.progress(), "Queue: 1 of 2, Bosses, pass 2");
    }
}
