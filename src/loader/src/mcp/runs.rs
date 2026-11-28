use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use loader::backend::{Settings, Target};

use crate::agent_runs::{Entry, Record};
use crate::api::{AgentSession, Api};
use crate::script::{Host, Launch, Process, Script, ScriptEvent, lock};

const KEEP_LINES: usize = 5000;
const KEEP_RUNS: usize = 50;
const POLL: Duration = Duration::from_millis(50);
const STOP_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, PartialEq)]
pub enum Status {
    Running,
    Finished,
    Failed(String),
    Stopped,
}

impl Status {
    fn label(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed(_) => "failed",
            Self::Stopped => "stopped",
        }
    }
}

struct Log {
    first: usize,
    lines: VecDeque<String>,
    status: Status,
}

pub struct Run {
    pub id: u64,
    pub pid: u32,
    process: Process,
    log: Mutex<Log>,
    record: Mutex<Record>,
    changed: Condvar,
}

pub struct Page {
    run: u64,
    pid: u32,
    since: usize,
    start: usize,
    next: usize,
    lines: Vec<(usize, String)>,
    numbered: bool,
    status: Status,
    record: Option<String>,
}

impl Page {
    pub fn text(&self) -> String {
        let mut text = format!(
            "run {} on client {}: {}\n",
            self.run,
            self.pid,
            self.status.label()
        );
        if let Status::Failed(error) = &self.status {
            text.push_str(&format!("error: {error}\n"));
        }
        text.push_str(&format!("next line: {}", self.next));
        if self.start > self.since {
            text.push_str(&format!(
                " ({} earlier lines were dropped)",
                self.start - self.since
            ));
        }
        text.push('\n');
        if let Some(record) = &self.record {
            text.push_str(&format!("full log: {record}\n"));
        }
        if !self.lines.is_empty() {
            text.push_str("---\n");
            let lines: Vec<String> = self
                .lines
                .iter()
                .map(|(at, line)| {
                    if self.numbered {
                        format!("{at}: {line}")
                    } else {
                        line.clone()
                    }
                })
                .collect();
            text.push_str(&lines.join("\n"));
        }
        text
    }
}

pub struct Filter(String);

impl Filter {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim().to_lowercase();
        (!text.is_empty()).then_some(Self(text))
    }

    fn keeps(&self, line: &str) -> bool {
        line.to_lowercase().contains(&self.0)
    }
}

pub struct Read<'a> {
    pub since: usize,
    pub wait: Duration,
    pub until_finished: bool,
    pub most: usize,
    pub filter: Option<&'a Filter>,
}

impl Run {
    fn push(&self, line: String) {
        let mut log = lock(&self.log);
        if log.lines.len() == KEEP_LINES {
            log.lines.pop_front();
            log.first += 1;
        }
        lock(&self.record).write(&Entry::Line { text: line.clone() });
        log.lines.push_back(line);
        self.changed.notify_all();
    }

    fn finish(&self, status: Status) {
        lock(&self.record).write(&Entry::End {
            error: match &status {
                Status::Failed(error) => Some(error.clone()),
                _ => None,
            },
            stopped: status == Status::Stopped,
        });
        lock(&self.log).status = status;
        self.changed.notify_all();
    }

    pub fn next_line(&self) -> usize {
        let log = lock(&self.log);
        log.first + log.lines.len()
    }

    pub fn is_running(&self) -> bool {
        lock(&self.log).status == Status::Running
    }

    pub fn stop(&self) {
        self.process.stop();
    }

    pub fn read(&self, read: &Read<'_>) -> Page {
        let deadline = Instant::now() + read.wait;
        let mut log = lock(&self.log);
        loop {
            let fresh = log.fresh(read.since, read.filter);
            if log.status != Status::Running || (fresh && !read.until_finished) {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            log = self
                .changed
                .wait_timeout(log, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let (start, next, lines) = log.pick(read.since, read.most, read.filter);
        Page {
            run: self.id,
            pid: self.pid,
            since: read.since,
            start,
            next,
            lines,
            numbered: read.filter.is_some(),
            status: log.status.clone(),
            record: lock(&self.record)
                .path
                .as_ref()
                .map(|path| path.display().to_string()),
        }
    }
}

impl Log {
    fn from(&self, since: usize) -> impl Iterator<Item = (usize, &String)> {
        let start = since.max(self.first);
        self.lines
            .iter()
            .enumerate()
            .skip(start - self.first)
            .map(|(at, line)| (self.first + at, line))
    }

    fn fresh(&self, since: usize, filter: Option<&Filter>) -> bool {
        self.from(since)
            .any(|(_, line)| filter.is_none_or(|filter| filter.keeps(line)))
    }

    fn pick(
        &self,
        since: usize,
        most: usize,
        filter: Option<&Filter>,
    ) -> (usize, usize, Vec<(usize, String)>) {
        let mut lines = Vec::new();
        let mut next = self.first + self.lines.len();
        for (at, line) in self.from(since) {
            if filter.is_some_and(|filter| !filter.keeps(line)) {
                continue;
            }
            if lines.len() == most {
                next = at;
                break;
            }
            lines.push((at, line.clone()));
        }
        (since.max(self.first), next, lines)
    }
}

#[derive(Default)]
pub struct Runs {
    list: Mutex<Vec<Arc<Run>>>,
    next: AtomicU64,
}

pub struct Start<'a> {
    pub title: &'a str,
    pub api: &'a Api,
    pub host: &'a Host,
    pub source: &'a str,
    pub path: &'a Path,
    pub sdk: Option<&'a Path>,
    pub target: &'a Target,
    pub settings: &'a Settings,
    pub launch: &'a Launch,
}

impl Runs {
    pub fn start(&self, start: Start<'_>) -> Result<Arc<Run>, String> {
        let session = start
            .api
            .start_agent_session()
            .map_err(|e| format!("Couldn't start the agent session: {e}"))?;
        let script = match Script::start(
            start.host,
            start.source,
            Some(start.path),
            start.sdk,
            start.target,
            start.settings,
            start.launch,
        ) {
            Ok(script) => script,
            Err(error) => {
                end_session(start.api, &session);
                return Err(error);
            }
        };
        let (id, pid) = (
            self.next.fetch_add(1, Ordering::Relaxed) + 1,
            start.target.client.pid,
        );
        let run = Arc::new(Run {
            id,
            pid,
            process: script.process(),
            log: Mutex::new(Log {
                first: 0,
                lines: VecDeque::new(),
                status: Status::Running,
            }),
            record: Mutex::new(Record::create(id, pid, start.title)),
            changed: Condvar::new(),
        });
        {
            let mut list = lock(&self.list);
            while list.len() >= KEEP_RUNS {
                match list.iter().position(|r| !r.is_running()) {
                    Some(index) => drop(list.remove(index)),
                    None => break,
                }
            }
            list.push(run.clone());
        }
        let (api, pumped) = (start.api.clone(), run.clone());
        thread::spawn(move || pump(script, &pumped, &api, session));
        Ok(run)
    }

    pub fn get(&self, id: u64) -> Option<Arc<Run>> {
        lock(&self.list).iter().find(|r| r.id == id).cloned()
    }

    pub fn running_on(&self, pid: u32) -> Option<u64> {
        lock(&self.list)
            .iter()
            .find(|r| r.pid == pid && r.is_running())
            .map(|r| r.id)
    }

    pub fn stop_all(&self) {
        let running: Vec<Arc<Run>> = lock(&self.list)
            .iter()
            .filter(|r| r.is_running())
            .cloned()
            .collect();
        for run in &running {
            run.stop();
        }
        for run in &running {
            run.read(&Read {
                since: usize::MAX,
                wait: STOP_GRACE,
                until_finished: true,
                most: 0,
                filter: None,
            });
        }
    }
}

fn end_session(api: &Api, session: &AgentSession) {
    if let Err(error) = api.end_agent_session(&session.id) {
        eprintln!("Couldn't end agent session {}: {error}", session.id);
    }
}

fn pump(mut script: Script, run: &Run, api: &Api, mut session: AgentSession) {
    let mut every = Duration::from_secs(session.heartbeat_seconds.clamp(10, 120));
    let mut beaten = Instant::now();
    loop {
        for event in script.poll() {
            match event {
                ScriptEvent::Output(line) => run.push(line),
                ScriptEvent::Warning(text) => run.push(format!("warning: {text}")),
                ScriptEvent::Exit(exit) => run.push(match (exit.status, exit.reason) {
                    (injector::form::ExitStatus::Done, Some(reason)) => {
                        format!("exit: done, {reason}")
                    }
                    (injector::form::ExitStatus::Done, None) => "exit: done".into(),
                    (injector::form::ExitStatus::Blocked, Some(reason)) => {
                        format!("exit: blocked, {reason}")
                    }
                    (injector::form::ExitStatus::Blocked, None) => "exit: blocked".into(),
                }),
                ScriptEvent::Breaks(_) | ScriptEvent::Counters(_) => {}
                ScriptEvent::Finished { result, stopped } => {
                    run.finish(match (stopped, result) {
                        (true, _) => Status::Stopped,
                        (false, Ok(())) => Status::Finished,
                        (false, Err(error)) => Status::Failed(error),
                    });
                    end_session(api, &session);
                    return;
                }
            }
        }
        if beaten.elapsed() >= every {
            beaten = Instant::now();
            match api.agent_heartbeat(&session.id) {
                Ok(beat) if beat.ended => match api.start_agent_session() {
                    Ok(new) => {
                        every = Duration::from_secs(new.heartbeat_seconds.clamp(10, 120));
                        session = new;
                    }
                    Err(error) => eprintln!("Couldn't restart the agent session: {error}"),
                },
                Ok(_) => {}
                Err(error) => eprintln!("Agent session heartbeat failed: {error}"),
            }
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(lines: &[&str]) -> Log {
        Log {
            first: 2,
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            status: Status::Finished,
        }
    }

    fn page(log: &Log, since: usize, most: usize, filter: Option<&Filter>) -> Page {
        let (start, next, lines) = log.pick(since, most, filter);
        Page {
            run: 1,
            pid: 7,
            since,
            start,
            next,
            lines,
            numbered: filter.is_some(),
            status: Status::Finished,
            record: Some("runs/1.jsonl".into()),
        }
    }

    #[test]
    fn a_filter_keeps_matching_lines_with_their_numbers() {
        let log = log(&["walk", "Bank open", "deposit", "bank closed"]);
        let bank = Filter::new(" BANK ");
        let found = page(&log, 0, 10, bank.as_ref());
        assert_eq!(
            found.lines,
            vec![(3, "Bank open".to_owned()), (5, "bank closed".to_owned())]
        );
        assert_eq!(found.next, 6);
        let text = found.text();
        assert!(text.contains("3: Bank open\n5: bank closed"), "{text}");
        assert!(text.contains("(2 earlier lines were dropped)"), "{text}");
        assert!(text.contains("full log: runs/1.jsonl"), "{text}");
        assert!(!log.fresh(6, bank.as_ref()));
        assert!(!log.fresh(0, Filter::new("teleport").as_ref()));
        assert!(Filter::new("  ").is_none());
    }

    #[test]
    fn a_full_page_resumes_at_the_first_line_it_left_out() {
        let log = log(&["a1", "b", "a2", "a3"]);
        let first = page(&log, 0, 2, Filter::new("a").as_ref());
        assert_eq!(first.next, 5);
        let plain = page(&log, 3, 2, None);
        assert_eq!(plain.lines, vec![(3, "b".to_owned()), (4, "a2".to_owned())]);
        assert_eq!(plain.next, 5);
        assert!(!plain.text().contains("3: b"));
    }
}
