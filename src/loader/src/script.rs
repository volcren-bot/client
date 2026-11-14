use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, channel},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use loader::backend::{self, Settings, Target};

use crate::accounts::Accounts;
use crate::api::Stats;
use crate::whitelist::Whitelist;

const STDERR_LINES: usize = 500;
const MAX_HTTP_IN_FLIGHT: usize = 32;

pub fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

pub fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone)]
pub struct Host {
    pub ctx: egui::Context,
    pub accounts: Arc<RwLock<Accounts>>,
    pub whitelist: Arc<RwLock<Whitelist>>,
    pub prices_url: String,
}

impl Host {
    fn lookup(&self, line: &str) -> Option<(String, &'static str)> {
        if let Some(hash) = line.strip_prefix(injector::form::PIN_REQUEST_PREFIX) {
            let answer = read(&self.accounts).pin_for(hash.trim());
            return Some((injector::form::pin_reply_line(&answer), "PIN request"));
        }
        let hash = line.strip_prefix(injector::form::LAMP_REQUEST_PREFIX)?;
        let answer = read(&self.accounts).lamp_skill_for(hash.trim());
        Some((injector::form::lamp_reply_line(&answer), "lamp request"))
    }
}

fn reply(stdin: &Mutex<ChildStdin>, line: &str, what: &str) -> Result<(), String> {
    let mut stdin = lock(stdin);
    writeln!(stdin, "{line}")
        .and_then(|()| stdin.flush())
        .map_err(|e| format!("Couldn't answer the script's {what}: {e}"))
}

fn answer_http(stdin: Arc<Mutex<ChildStdin>>, payload: &str, whitelist: Whitelist) {
    static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
    if IN_FLIGHT.fetch_add(1, Ordering::AcqRel) >= MAX_HTTP_IN_FLIGHT {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        let busy = Err("http_request: too many requests at once".to_owned());
        let _ = reply(&stdin, &injector::http::reply_line(&busy), "HTTP request");
        return;
    }
    let request = injector::http::parse_request(payload);
    thread::spawn(move || {
        let answer = request.and_then(|request| whitelist.perform(&request));
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
        let _ = reply(&stdin, &injector::http::reply_line(&answer), "HTTP request");
    });
}

#[derive(Default)]
pub struct Launch {
    pub values: serde_json::Value,
    pub resume: Option<injector::form::BreakPlan>,
}

impl Launch {
    pub fn with(values: serde_json::Value) -> Self {
        Self {
            values,
            resume: None,
        }
    }

    fn header(&self) -> String {
        let values = if self.values.is_object() {
            self.values.clone()
        } else {
            serde_json::json!({})
        };
        let mut header = format!("{}\n", injector::form::settings_line(&values));
        if let Some(plan) = &self.resume {
            header.push_str(&injector::form::resume_line(plan));
            header.push('\n');
        }
        header
    }
}

pub enum ScriptEvent {
    Output(String),
    Warning(String),
    Exit(injector::form::Exit),
    Breaks(injector::form::BreakPlan),
    Counters(Vec<injector::form::Counter>),
    Finished {
        result: Result<(), String>,
        stopped: bool,
    },
}

#[derive(Clone)]
pub struct Process {
    child: Arc<Mutex<Child>>,
    stopped: Arc<AtomicBool>,
    stats: Arc<Mutex<Stats>>,
}

impl Process {
    fn new(child: Child) -> Self {
        Self {
            child: Arc::new(Mutex::new(child)),
            stopped: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(Mutex::new(Stats::default())),
        }
    }

    fn child(&self) -> MutexGuard<'_, Child> {
        lock(&self.child)
    }

    fn id(&self) -> u32 {
        self.child().id()
    }

    fn status(&self) -> Option<ExitStatus> {
        self.child().try_wait().ok().flatten()
    }

    fn was_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.child().kill();
    }

    pub fn exited(&self) -> bool {
        self.status().is_some()
    }

    pub fn take_stats(&self) -> Stats {
        std::mem::take(&mut *lock(&self.stats))
    }
}

fn hold_client(target: &Target) -> Result<std::fs::File, String> {
    let dir = crate::paths::project_dirs()
        .ok_or("Can't find a data folder on this system.")?
        .data_dir()
        .join("clients");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let name = crate::session::sha256_hex(target.key().as_bytes());
    let path = dir.join(format!("{}.lock", &name[..16]));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(
            "Another script is running on this client, from the loader or an agent. Stop it first."
                .into(),
        ),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(format!("Couldn't lock {}: {error}", path.display()))
        }
    }
}

pub struct Script {
    process: Process,
    client: Option<std::fs::File>,
    stdin: Arc<Mutex<ChildStdin>>,
    output: Receiver<String>,
    stderr: Arc<Mutex<Vec<String>>>,
    finished: bool,
    paused: bool,
    stop_requested: Option<Instant>,
    stop_ignored_reported: bool,
}

impl Script {
    pub fn start(
        host: &Host,
        source: &str,
        path: Option<&Path>,
        lib: Option<&Path>,
        target: Option<&Target>,
        settings: &Settings,
        launch: &Launch,
    ) -> Result<Self, String> {
        Self::start_with(host, source, &[], path, lib, target, settings, launch)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_with(
        host: &Host,
        source: &str,
        modules: &[(String, String)],
        path: Option<&Path>,
        lib: Option<&Path>,
        target: Option<&Target>,
        settings: &Settings,
        launch: &Launch,
    ) -> Result<Self, String> {
        let client = target.map(hold_client).transpose()?;
        let mut command = backend::script_command(target, settings, path, lib)?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "Couldn't run {}: {e}",
                    command.get_program().to_string_lossy()
                )
            })?;

        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr_pipe = child.stderr.take().expect("piped");
        let pins = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            injector::form::PINS_PREFIX,
            injector::http::CAPABILITY_PREFIX,
            injector::prices::line(&host.prices_url),
            injector::antiban::line(&settings.antiban),
            injector::transports::settings_line(&settings.transports),
            crate::transports::line()
        ) + &launch.header();
        let header = format!("{}{}\n", injector::form::SCRIPT_PREFIX, source.len());
        let files = if modules.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(
                &modules
                    .iter()
                    .map(|(name, source)| (name.as_str(), source.as_str()))
                    .collect::<std::collections::HashMap<&str, &str>>(),
            )
            .expect("strings serialize");
            format!("{}{}\n{json}", injector::form::MODULES_PREFIX, json.len())
        };
        if let Err(error) = stdin
            .write_all(pins.as_bytes())
            .and_then(|()| stdin.write_all(files.as_bytes()))
            .and_then(|()| stdin.write_all(header.as_bytes()))
            .and_then(|()| stdin.write_all(source.as_bytes()))
            .and_then(|()| stdin.flush())
        {
            drop(stdin);
            let _ = child.wait();
            let mut said = String::new();
            let _ = std::io::Read::read_to_string(&mut stderr_pipe, &mut said);
            let said = said.trim();
            return Err(if said.is_empty() {
                format!("Couldn't send the script: {error}")
            } else {
                format!("The injector stopped before it got the script: {said}")
            });
        }

        let process = Process::new(child);
        let stdin = Arc::new(Mutex::new(stdin));
        let (tx, output) = channel();
        let repaint_ctx = host.ctx.clone();
        let stats = process.stats.clone();
        let (host, requests) = (host.clone(), stdin.clone());
        thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut entry = Vec::new();
            while stdout
                .read_until(injector::form::ENTRY_END as u8, &mut entry)
                .is_ok_and(|read| read > 0)
            {
                if entry.last() == Some(&(injector::form::ENTRY_END as u8)) {
                    entry.pop();
                }
                let line = String::from_utf8_lossy(&entry).into_owned();
                entry.clear();
                if let Some((experience, gold, custom)) = line
                    .strip_prefix(injector::form::STATS_PREFIX)
                    .and_then(injector::form::parse_stats)
                {
                    lock(&stats).add(experience, gold, &custom);
                    continue;
                }
                if let Some(payload) = line.strip_prefix(injector::http::REQUEST_PREFIX) {
                    answer_http(requests.clone(), payload, read(&host.whitelist).clone());
                    continue;
                }
                if let Some((answer, what)) = host.lookup(&line) {
                    if let Err(error) = reply(&requests, &answer, what) {
                        let _ = tx.send(format!("{}{error}", injector::form::WARN_PREFIX));
                        repaint_ctx.request_repaint();
                    }
                    continue;
                }
                if tx.send(line).is_err() {
                    break;
                }
                repaint_ctx.request_repaint();
            }
            repaint_ctx.request_repaint();
        });

        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_writer = stderr.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
                let mut lines = lock(&stderr_writer);
                if lines.len() >= 2 * STDERR_LINES {
                    lines.drain(..STDERR_LINES);
                }
                lines.push(line);
            }
        });

        Ok(Self {
            process,
            client,
            stdin,
            output,
            stderr,
            finished: false,
            paused: false,
            stop_requested: None,
            stop_ignored_reported: false,
        })
    }

    pub fn process(&self) -> Process {
        self.process.clone()
    }

    fn reply(&self, line: &str, what: &str) -> Result<(), String> {
        reply(&self.stdin, line, what)
    }

    pub fn finish(&self) -> Result<(), String> {
        self.reply(injector::form::FINISH_PREFIX, "finish request")
    }

    pub fn push_update(&self, values: &serde_json::Value) -> Result<(), String> {
        self.reply(&injector::form::update_line(values), "settings update")
    }

    pub fn push_antiban(&self, config: &injector::antiban::Config) -> Result<(), String> {
        self.reply(&injector::antiban::line(config), "breaks update")
    }

    pub fn push_transport_settings(
        &self,
        settings: &injector::transports::Settings,
    ) -> Result<(), String> {
        self.reply(
            &injector::transports::settings_line(settings),
            "transports update",
        )
    }

    pub fn skip_next(&self, kind: &str, index: u32) -> Result<(), String> {
        self.reply(&injector::antiban::skip_line(kind, index), "skip")
    }

    pub fn is_running(&self) -> bool {
        !self.finished
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.finished || self.paused == paused {
            return;
        }
        self.paused = paused;
        suspend_process(self.process.id(), paused);
    }

    pub fn stop(&mut self) {
        if self.finished {
            return;
        }
        self.stop_requested.get_or_insert_with(Instant::now);
        if self.paused {
            suspend_process(self.process.id(), false);
        }
        self.process.stop();
    }

    pub fn stop_requested(&self) -> bool {
        self.stop_requested.is_some()
    }

    pub fn stop_ignored(&mut self, after: Duration) -> bool {
        let ignored = !self.finished
            && !self.stop_ignored_reported
            && self.stop_requested.is_some_and(|at| at.elapsed() > after);
        self.stop_ignored_reported |= ignored;
        ignored
    }

    pub fn poll(&mut self) -> Vec<ScriptEvent> {
        let mut events: Vec<ScriptEvent> = self
            .output
            .try_iter()
            .map(|line| {
                if let Some(payload) = line.strip_prefix(injector::form::STATS_PREFIX) {
                    return ScriptEvent::Output(format!("Bad stats from the script: {payload}"));
                }
                if let Some(text) = line.strip_prefix(injector::form::WARN_PREFIX) {
                    return ScriptEvent::Warning(text.to_owned());
                }
                if let Some(payload) = line.strip_prefix(injector::form::BREAKS_PREFIX) {
                    return match injector::form::parse_breaks(payload) {
                        Some(plan) => ScriptEvent::Breaks(plan),
                        None => {
                            ScriptEvent::Output(format!("Bad breaks from the script: {payload}"))
                        }
                    };
                }
                if let Some(payload) = line.strip_prefix(injector::form::COUNTERS_PREFIX) {
                    return match injector::form::parse_counters(payload) {
                        Some(counters) => ScriptEvent::Counters(counters),
                        None => {
                            ScriptEvent::Output(format!("Bad counters from the script: {payload}"))
                        }
                    };
                }
                match line.strip_prefix(injector::form::EXIT_PREFIX) {
                    Some(payload) => match injector::form::parse_exit(payload) {
                        Some(exit) => ScriptEvent::Exit(exit),
                        None => ScriptEvent::Output(format!("Bad exit from the script: {payload}")),
                    },
                    None => ScriptEvent::Output(line),
                }
            })
            .collect();
        if !self.finished
            && let Some(status) = self.process.status()
        {
            self.finished = true;
            self.client = None;
            let result = if status.success() {
                Ok(())
            } else {
                let detail = {
                    let lines = lock(&self.stderr);
                    lines[lines.len().saturating_sub(STDERR_LINES)..].join("\n")
                };
                Err(if detail.is_empty() {
                    format!("Script failed ({status}).")
                } else {
                    detail
                })
            };
            events.push(ScriptEvent::Finished {
                result,
                stopped: self.process.was_stopped(),
            });
        }
        events
    }
}

#[cfg(unix)]
fn suspend_process(pid: u32, suspend: bool) {
    let signal = if suspend {
        libc::SIGSTOP
    } else {
        libc::SIGCONT
    };
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

#[cfg(not(unix))]
fn suspend_process(_pid: u32, _suspend: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_runner_built() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let status = std::process::Command::new(cargo)
                .args(["build", "-p", "injector", "--bin", "injector"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status();
            assert!(
                status.is_ok_and(|s| s.success()),
                "couldn't build injector for tests"
            );
        });
    }

    type Run = (Vec<String>, Option<(Result<(), String>, bool)>);

    fn wait(script: &mut Script, timeout: Duration) -> Run {
        let (mut lines, start) = (Vec::new(), Instant::now());
        while start.elapsed() < timeout {
            for event in script.poll() {
                match event {
                    ScriptEvent::Output(line) => lines.push(line),
                    ScriptEvent::Warning(line) => lines.push(format!("warning {line}")),
                    ScriptEvent::Exit(exit) => lines.push(format!("exit {:?}", exit.status)),
                    ScriptEvent::Breaks(plan) => {
                        lines.push(format!("breaks {}", plan.breaks.len()))
                    }
                    ScriptEvent::Counters(counters) => {
                        lines.push(format!("counters {}", counters.len()))
                    }
                    ScriptEvent::Finished { result, stopped } => {
                        return (lines, Some((result, stopped)));
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        (lines, None)
    }

    fn host(whitelist: Vec<String>, accounts: Vec<crate::accounts::Entry>) -> Host {
        Host {
            ctx: egui::Context::default(),
            accounts: Arc::new(RwLock::new(Accounts::with_entries(accounts))),
            whitelist: Arc::new(RwLock::new(Whitelist::new(
                whitelist,
                &crate::api::Config::default(),
            ))),
            prices_url: String::new(),
        }
    }

    fn start_on(host: &Host, source: &str) -> Script {
        ensure_runner_built();
        Script::start(
            host,
            source,
            None,
            None,
            None,
            &Settings::default(),
            &Launch::default(),
        )
        .unwrap()
    }

    fn start(source: &str) -> Script {
        start_on(&host(Vec::new(), Vec::new()), source)
    }

    #[test]
    fn saved_pins_and_lamp_skills_answer_the_scripts_lookups() {
        let entry = crate::accounts::Entry {
            name: "main".into(),
            hash: "abc".into(),
            pin: "1234".into(),
            lamp_skill: "Slayer".into(),
        };
        let host = host(Vec::new(), vec![entry]);
        let lookup = |line: String| host.lookup(&line).unwrap();
        let pin = lookup(injector::form::pin_request_line("abc"));
        assert_eq!(
            injector::form::parse_pin_reply(&pin.0),
            Ok(Some("1234".into()))
        );
        assert_eq!(
            injector::form::parse_pin_reply(&lookup(injector::form::pin_request_line("nobody")).0),
            Ok(None)
        );
        let lamp = lookup(injector::form::lamp_request_line("abc"));
        assert_eq!(
            injector::form::parse_lamp_reply(&lamp.0),
            Ok(Some("Slayer".into()))
        );
        assert_eq!(host.lookup("just a printed line"), None);
    }

    #[test]
    fn a_print_with_newlines_is_one_entry() {
        let mut script = start("print('a\\nb') print('c')");
        let (lines, _) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["a\nb", "c"]);
    }

    #[test]
    fn prints_and_finishes() {
        let mut script = start("print('a', 1, nil, true) print(2 + 3)");
        let (lines, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["a\t1\tnil\ttrue", "5"]);
        assert_eq!(end, Some((Ok(()), false)));
        assert!(!script.is_running());
    }

    #[test]
    fn report_warning_arrives_as_a_warning_not_output() {
        let mut script = start("report_warning('Escape is off')\nprint('after')");
        let (lines, _) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["warning Escape is off", "after"]);
    }

    #[test]
    fn settings_resume_and_finish_reach_the_script_and_exits_come_back() {
        ensure_runner_built();
        let launch = Launch {
            values: serde_json::json!({ "spot": "Cage", "hours": 3 }),
            resume: Some(injector::form::BreakPlan {
                breaks: vec![injector::form::PlannedBreak {
                    remaining: 900.0,
                    length: 60_000.0,
                    at: None,
                    index: 0,
                }],
                ..Default::default()
            }),
        };
        let mut script = Script::start(
            &host(Vec::new(), Vec::new()),
            "local s = settings() print(s.spot, s.hours)\n\
             print(antiban_resume().breaks[1].remaining)\n\
             while not finish_requested() do sleep(10) end\n\
             report_exit('blocked', 'out of bait')",
            None,
            None,
            None,
            &Settings::default(),
            &launch,
        )
        .unwrap();
        let start = Instant::now();
        let mut lines = Vec::new();
        while lines.len() < 2 && start.elapsed() < Duration::from_secs(5) {
            lines.extend(script.poll().into_iter().filter_map(|e| match e {
                ScriptEvent::Output(line) => Some(line),
                _ => None,
            }));
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(lines, ["Cage\t3", "900"]);
        script.finish().unwrap();
        let (rest, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(rest, ["exit Blocked"]);
        assert_eq!(end, Some((Ok(()), false)));
    }

    #[test]
    fn without_settings_a_script_gets_an_empty_table() {
        let mut script = start("local n = 0 for _ in settings() do n += 1 end print(n)");
        let (lines, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["0"]);
        assert_eq!(end, Some((Ok(()), false)));
    }

    #[test]
    fn report_stats_adds_up_on_the_process_and_is_not_output() {
        let mut script = start("report_stats(10, 5, { 1 }) report_stats(20, -5) print('done')");
        let (lines, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["done"]);
        assert_eq!(end, Some((Ok(()), false)));
        let stats = script.process().take_stats();
        assert_eq!(
            (stats.experience, stats.gold, stats.custom),
            (30, 0, vec![1])
        );
        assert!(script.process().take_stats().is_empty());
    }

    fn session_server(alive: bool) -> crate::fake_api::FakeServer {
        crate::fake_api::FakeServer::start(move |_, request| {
            if request.path.ends_with("/end") {
                crate::fake_api::Reply::json(200, serde_json::json!({ "charged": 6_000 }))
            } else {
                crate::fake_api::Reply::json(
                    200,
                    serde_json::json!({ "alive": alive, "reason": "timeout", "microcredits": 1_000 }),
                )
            }
        })
    }

    fn keep_on_a_thread(
        fake: &crate::fake_api::FakeServer,
        script: &Script,
    ) -> (
        thread::JoinHandle<()>,
        std::sync::mpsc::Receiver<crate::session::Kept>,
    ) {
        let api = crate::api::Api::signed_in(
            crate::api::Config {
                api_url: fake.url.clone(),
                supabase_url: fake.url.clone(),
                anon_key: "anon".into(),
                site_url: String::new(),
                prices_url: String::new(),
            },
            "token",
        );
        let process = script.process();
        let (tx, kept) = channel();
        let keeper = thread::spawn(move || {
            crate::session::keep(
                &api,
                "s1",
                &process,
                Duration::from_millis(50),
                move |event| {
                    let _ = tx.send(event);
                },
            );
        });
        (keeper, kept)
    }

    fn join_within(keeper: thread::JoinHandle<()>, limit: Duration) {
        let start = Instant::now();
        while !keeper.is_finished() {
            assert!(start.elapsed() < limit, "the keeper is still running");
            thread::sleep(Duration::from_millis(10));
        }
        keeper.join().unwrap();
    }

    #[test]
    fn a_session_gets_heartbeats_and_stats_without_a_poll() {
        let fake = session_server(true);
        let script = start("report_stats(10, 5)\nwhile true do sleep(1000) end");
        let (keeper, _kept) = keep_on_a_thread(&fake, &script);
        let start = Instant::now();
        let reported = |r: &crate::fake_api::Request| r.json()["stats"]["experience"] == 10;
        while !fake.requests().iter().any(reported) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{:?}",
                fake.requests()
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 0);
        script.process().stop();
        join_within(keeper, Duration::from_secs(5));
        assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
    }

    #[test]
    fn a_script_that_finishes_ends_its_session_without_a_poll() {
        let fake = session_server(true);
        let script = start("print('done')");
        let (keeper, kept) = keep_on_a_thread(&fake, &script);
        join_within(keeper, Duration::from_secs(10));
        assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
        assert!(matches!(
            kept.try_iter().last(),
            Some(crate::session::Kept::Ended(Ok(_)))
        ));
    }

    #[test]
    fn a_session_the_server_ended_kills_the_script_without_a_poll() {
        let fake = session_server(false);
        let mut script = start("while true do sleep(1000) end");
        let (keeper, kept) = keep_on_a_thread(&fake, &script);
        join_within(keeper, Duration::from_secs(10));
        assert!(kept.try_iter().any(|event| matches!(
            event,
            crate::session::Kept::Stop { reason, sign_out: false } if reason.contains("timed out")
        )));
        let (_, end) = wait(&mut script, Duration::from_secs(2));
        assert_eq!(end.map(|(_, stopped)| stopped), Some(true));
    }

    #[test]
    fn errors_name_the_script_and_line() {
        let mut script = start("print('ok')\nerror('boom')");
        let (_, end) = wait(&mut script, Duration::from_secs(5));
        let (result, stopped) = end.unwrap();
        let error = result.unwrap_err();
        assert!(error.contains("script.luau:2: boom"), "{error}");
        assert!(!stopped);
    }

    #[test]
    fn stop_kills_a_sleeping_script() {
        let mut script = start("while true do sleep(1000) end");
        thread::sleep(Duration::from_millis(100));
        script.stop();
        let (_, end) = wait(&mut script, Duration::from_secs(2));
        assert_eq!(end.map(|(_, stopped)| stopped), Some(true));
    }

    #[test]
    fn stop_kills_a_jit_compiled_busy_loop() {
        let mut script = start("local x = 0 while true do x = x + 1 end");
        thread::sleep(Duration::from_millis(300));
        script.stop();
        let (_, end) = wait(&mut script, Duration::from_secs(3));
        assert_eq!(
            end.map(|(_, stopped)| stopped),
            Some(true),
            "JIT-compiled busy loop didn't stop"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pause_suspends_even_a_jit_compiled_busy_loop() {
        let mut script = start("local x = 0 while true do x = x + 1 end sleep(1)");
        thread::sleep(Duration::from_millis(200));
        script.set_paused(true);
        assert!(script.is_running());
        thread::sleep(Duration::from_millis(200));
        assert!(
            script.is_running(),
            "a paused process shouldn't exit on its own"
        );
        script.stop();
        let (_, end) = wait(&mut script, Duration::from_secs(3));
        assert_eq!(end.map(|(_, stopped)| stopped), Some(true));
    }

    fn serve() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let reply = if head.starts_with("GET /hop") {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/hi\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                } else {
                    format!(
                        "HTTP/1.1 404 Not Found\r\nX-Seen: {}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                        head.contains("x-token: abc") || head.contains("X-Token: abc")
                    )
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        port
    }

    #[test]
    fn http_requests_only_reach_whitelisted_hosts() {
        let port = serve();
        let host = host(vec!["127.0.0.1".into()], Vec::new());
        let mut script = start_on(
            &host,
            &format!(
                "local r, e = http_request({{ url = 'http://127.0.0.1:{port}/hi', headers = {{ ['X-Token'] = 'abc' }} }})\n\
             print(r.ok, r.status, r.body, r.headers['x-seen'])\n\
             print(http_request({{ url = 'http://localhost:{port}/hi' }}))\n\
             print(http_request({{ url = 'http://127.0.0.1:{port}/hop' }}))\n\
             print(http_request({{ url = 'ftp://127.0.0.1/' }}))\n\
             print(http_request({{ url = 'http://127.0.0.1:{port}/', headers = {{ Host = 'evil.com' }} }}))"
            ),
        );
        let start = Instant::now();
        while !script.process().exited() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "the script is waiting for an answer"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let (lines, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(end, Some((Ok(()), false)), "{lines:?}");
        assert_eq!(lines[0], "false\t404\thello\ttrue");
        assert!(
            lines[1].starts_with("nil\tlocalhost isn't on the HTTP whitelist"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].starts_with("nil\tlocalhost isn't on the HTTP whitelist"),
            "a redirect off the list is refused: {}",
            lines[2]
        );
        assert!(
            lines[3].starts_with("nil\t") && lines[3].contains("http or https"),
            "{}",
            lines[3]
        );
        assert!(lines[4].contains("header can't be set"), "{}", lines[4]);
    }

    fn live_client() -> Option<(Target, Settings)> {
        let mut settings = Settings::default();
        if settings.helper.is_empty() {
            let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/x86_64-pc-windows-gnu/release/injector.exe");
            if helper.is_file() {
                settings.helper = helper.display().to_string();
            }
        }
        let scan = backend::scan(&settings);
        let Some(target) = scan.targets.into_iter().find(|t| t.client.loaded.is_some()) else {
            eprintln!(
                "skipped, no client with agent loaded was found: {:?}",
                scan.errors
            );
            return None;
        };
        Some((target, settings))
    }

    fn run_live(client: &(Target, Settings), source: &str) -> Run {
        let mut script = Script::start(
            &host(Vec::new(), Vec::new()),
            source,
            None,
            None,
            Some(&client.0),
            &client.1,
            &Launch::default(),
        )
        .unwrap();
        let (lines, end) = wait(&mut script, Duration::from_secs(30));
        for line in &lines {
            println!("{line}");
        }
        (lines, end)
    }

    #[test]
    fn game_state_and_connection_read_a_real_client() {
        let Some(client) = live_client() else { return };
        let (lines, end) = run_live(
            &client,
            "local state, err = game_state()\n\
             assert(state ~= nil, err)\n\
             print('game_state', state.game_state)\n\
             local conn, err2 = connection()\n\
             assert(conn ~= nil, err2)\n\
             print('width', conn.width, 'height', conn.height)\n\
             assert(conn.width > 0 and conn.height > 0)",
        );
        assert_eq!(end, Some((Ok(()), false)), "{lines:?}");
    }

    #[test]
    #[ignore]
    fn lib_against_a_real_client() {
        let Some(client) = live_client() else { return };
        let (lines, end) = run_live(
            &client,
            "local Player = require('player')\n\
             local SidePanels = require('side_panels')\n\
             local Stats = require('stats')\n\
             local Players = require('players')\n\
             local Ticks = require('ticks')\n\
             print('name', assert(Player.name()))\n\
             print('position', assert(Player.position()))\n\
             print('run', Player.run_energy(), Player.is_running(), 'weight', Player.weight())\n\
             print('hp', Player.hitpoints()) print('prayer', Player.prayer())\n\
             print('spec', Player.special_attack(), Player.special_attack_enabled(), Player.poison())\n\
             print('attack', Stats.level('attack'), Stats.xp('Attack'))\n\
             print('me', assert(Players.me()).index)\n\
             local before = assert(SidePanels.current())\n\
             print('tab', SidePanels.name(before))\n\
             assert(SidePanels.open('stats'))\n\
             print('opened', SidePanels.name(assert(SidePanels.current())))\n\
             assert(SidePanels.open(before))\n\
             print('back to', SidePanels.name(assert(SidePanels.current())))\n\
             local t = assert(Ticks.current())\n\
             print('tick', t, 'next', assert(Ticks.wait(1)))",
        );
        assert_eq!(end, Some((Ok(()), false)), "{lines:?}");
    }
}
