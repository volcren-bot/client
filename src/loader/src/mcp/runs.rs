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
    changed: Condvar,
}

pub struct Page {
    run: u64,
    pid: u32,
    since: usize,
    start: usize,
    lines: Vec<String>,
    status: Status,
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
        let next = self.start + self.lines.len();
        text.push_str(&format!("next line: {next}"));
        if self.start > self.since {
            text.push_str(&format!(
                " ({} earlier lines were dropped)",
                self.start - self.since
            ));
        }
        text.push('\n');
        if !self.lines.is_empty() {
            text.push_str("---\n");
            text.push_str(&self.lines.join("\n"));
        }
        text
    }
}

impl Run {
    fn push(&self, line: String) {
        let mut log = lock(&self.log);
        if log.lines.len() == KEEP_LINES {
            log.lines.pop_front();
            log.first += 1;
        }
        log.lines.push_back(line);
        self.changed.notify_all();
    }

    fn finish(&self, status: Status) {
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

    pub fn read(&self, since: usize, wait: Duration, until_finished: bool, most: usize) -> Page {
        let deadline = Instant::now() + wait;
        let mut log = lock(&self.log);
        loop {
            let fresh = log.first + log.lines.len() > since;
            if log.status != Status::Running || (fresh && !until_finished) {
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
        let start = since.max(log.first);
        Page {
            run: self.id,
            pid: self.pid,
            since,
            start,
            lines: log
                .lines
                .iter()
                .skip(start - log.first)
                .take(most)
                .cloned()
                .collect(),
            status: log.status.clone(),
        }
    }
}

#[derive(Default)]
pub struct Runs {
    list: Mutex<Vec<Arc<Run>>>,
    next: AtomicU64,
}

pub struct Start<'a> {
    pub api: &'a Api,
    pub host: &'a Host,
    pub source: &'a str,
    pub path: &'a Path,
    pub lib: &'a Path,
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
            Some(start.lib),
            Some(start.target),
            start.settings,
            start.launch,
        ) {
            Ok(script) => script,
            Err(error) => {
                end_session(start.api, &session);
                return Err(error);
            }
        };
        let run = Arc::new(Run {
            id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
            pid: start.target.client.pid,
            process: script.process(),
            log: Mutex::new(Log {
                first: 0,
                lines: VecDeque::new(),
                status: Status::Running,
            }),
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
            run.read(usize::MAX, STOP_GRACE, true, 0);
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
