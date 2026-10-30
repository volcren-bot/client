use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, channel},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use loader::backend::{self, Settings, Target};

use crate::whitelist::Whitelist;

pub enum ScriptEvent {
    Output(String),
    Form(serde_json::Value),
    /// Experience, gold and custom tracker gains from `report_stats`.
    Stats(i64, i64, Vec<i64>),
    /// The breaks and sleeps from `report_breaks`.
    Breaks(injector::form::BreakPlan),
    /// What the script counts, from `report_counters`.
    Counters(Vec<injector::form::Counter>),
    /// `http_request()` asks for this request (a JSON `injector::http::Request`); answer with `answer_http`.
    HttpRequest(String),
    /// `bank_pin_enter()` asks for the PIN saved for the account with this name hash; answer with `answer_pin`.
    PinRequest(String),
    Finished { result: Result<(), String>, stopped: bool },
}

pub struct Script {
    child: Child,
    /// Shared with the threads that answer HTTP requests, which write their reply when the response is in.
    stdin: Arc<Mutex<ChildStdin>>,
    output: Receiver<String>,
    stderr: Arc<Mutex<Vec<String>>>,
    finished: bool,
    paused: bool,
    stop_requested: Option<Instant>,
    stop_ignored_reported: bool,
    stopped: bool,
}

impl Script {
    /// `lib` is your own lib folder, for a script run from a local file.
    pub fn start(ctx: &egui::Context, source: &str, path: Option<&Path>, lib: Option<&Path>, target: Option<&Target>, settings: &Settings) -> Result<Self, String> {
        Self::start_with(ctx, source, &[], path, lib, target, settings)
    }

    /// `modules` are the script's other files by name, sent to the injector to keep in memory, beside `path`.
    pub fn start_with(ctx: &egui::Context, source: &str, modules: &[(String, String)], path: Option<&Path>, lib: Option<&Path>, target: Option<&Target>, settings: &Settings) -> Result<Self, String> {
        let mut command = backend::script_command(target, settings, path, lib)?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't run {}: {e}", command.get_program().to_string_lossy()))?;

        let mut stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr_pipe = child.stderr.take().expect("piped");
        let pins = format!("{}\n{}\n", injector::form::PINS_PREFIX, injector::http::CAPABILITY_PREFIX);
        let header = format!("{}{}\n", injector::form::SCRIPT_PREFIX, source.len());
        let files = if modules.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&modules.iter().cloned().collect::<std::collections::HashMap<_, _>>()).expect("strings serialize");
            format!("{}{}\n{json}", injector::form::MODULES_PREFIX, json.len())
        };
        if let Err(error) = stdin
            .write_all(pins.as_bytes())
            .and_then(|()| stdin.write_all(files.as_bytes()))
            .and_then(|()| stdin.write_all(header.as_bytes()))
            .and_then(|()| stdin.write_all(source.as_bytes()))
            .and_then(|()| stdin.flush())
        {
            // The injector quit before reading the script (bad arguments, an old build): say why instead of "broken pipe".
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

        let (tx, output) = channel();
        let repaint_ctx = ctx.clone();
        thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut entry = Vec::new();
            while stdout.read_until(injector::form::ENTRY_END as u8, &mut entry).is_ok_and(|read| read > 0) {
                if entry.last() == Some(&(injector::form::ENTRY_END as u8)) {
                    entry.pop();
                }
                let line = String::from_utf8_lossy(&entry).into_owned();
                entry.clear();
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
                if let Ok(mut lines) = stderr_writer.lock() {
                    lines.push(line);
                }
            }
        });

        Ok(Self {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            output,
            stderr,
            finished: false,
            paused: false,
            stop_requested: None,
            stop_ignored_reported: false,
            stopped: false,
        })
    }

    fn reply(&self, line: &str, what: &str) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        writeln!(stdin, "{line}").and_then(|()| stdin.flush()).map_err(|e| format!("Couldn't answer the script's {what}: {e}"))
    }

    pub fn answer_form(&mut self, values: Option<serde_json::Value>) -> Result<(), String> {
        let reply = match values {
            Some(values) => serde_json::json!({ "values": values }),
            None => serde_json::json!({ "cancelled": true }),
        };
        self.reply(&reply.to_string(), "form")
    }

    /// Tells the script's injector the PIN for the hash it asked about, `None` if no saved account has it, or why we can't say.
    pub fn answer_pin(&mut self, answer: Result<Option<String>, String>) -> Result<(), String> {
        self.reply(&injector::form::pin_reply_line(&answer), "PIN request")
    }

    /// Makes an `http_request` of the script on a thread of its own, if its host is on the whitelist, and sends the response
    /// (or why there is none) when it is in. The script waits for it; nothing else uses its input meanwhile. A script that
    /// ended before the answer is ignored.
    pub fn answer_http(&self, payload: &str, whitelist: Whitelist) {
        let stdin = self.stdin.clone();
        let request = injector::http::parse_request(payload);
        thread::spawn(move || {
            let answer = request.and_then(|request| whitelist.perform(&request));
            let mut stdin = stdin.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = writeln!(stdin, "{}", injector::http::reply_line(&answer)).and_then(|()| stdin.flush());
        });
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
        suspend_process(self.child.id(), paused);
    }

    pub fn stop(&mut self) {
        if self.finished {
            return;
        }
        self.stopped = true;
        self.stop_requested.get_or_insert_with(Instant::now);
        if self.paused {
            suspend_process(self.child.id(), false);
        }
        let _ = self.child.kill();
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
                    return match injector::form::parse_stats(payload) {
                        Some((experience, gold, custom)) => ScriptEvent::Stats(experience, gold, custom),
                        None => ScriptEvent::Output(format!("Bad stats from the script: {payload}")),
                    };
                }
                if let Some(payload) = line.strip_prefix(injector::http::REQUEST_PREFIX) {
                    return ScriptEvent::HttpRequest(payload.to_owned());
                }
                if let Some(hash) = line.strip_prefix(injector::form::PIN_REQUEST_PREFIX) {
                    return ScriptEvent::PinRequest(hash.trim().to_owned());
                }
                if let Some(payload) = line.strip_prefix(injector::form::BREAKS_PREFIX) {
                    return match injector::form::parse_breaks(payload) {
                        Some(plan) => ScriptEvent::Breaks(plan),
                        None => ScriptEvent::Output(format!("Bad breaks from the script: {payload}")),
                    };
                }
                if let Some(payload) = line.strip_prefix(injector::form::COUNTERS_PREFIX) {
                    return match injector::form::parse_counters(payload) {
                        Some(counters) => ScriptEvent::Counters(counters),
                        None => ScriptEvent::Output(format!("Bad counters from the script: {payload}")),
                    };
                }
                match line.strip_prefix(injector::form::FORM_PREFIX).map(serde_json::from_str) {
                    Some(Ok(spec)) => ScriptEvent::Form(spec),
                    Some(Err(error)) => ScriptEvent::Output(format!("Bad form from the script: {error}")),
                    None => ScriptEvent::Output(line),
                }
            })
            .collect();
        if !self.finished && let Ok(Some(status)) = self.child.try_wait() {
            self.finished = true;
            let result = if status.success() {
                Ok(())
            } else {
                let detail = self.stderr.lock().map(|lines| lines.join("\n")).unwrap_or_default();
                Err(if detail.is_empty() { format!("Script failed ({status}).") } else { detail })
            };
            events.push(ScriptEvent::Finished { result, stopped: self.stopped });
        }
        events
    }
}

#[cfg(unix)]
fn suspend_process(pid: u32, suspend: bool) {
    let signal = if suspend { libc::SIGSTOP } else { libc::SIGCONT };
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
            assert!(status.is_ok_and(|s| s.success()), "couldn't build injector for tests");
        });
    }

    type Run = (Vec<String>, Option<(Result<(), String>, bool)>);

    fn wait(script: &mut Script, timeout: Duration) -> Run {
        let (mut lines, start) = (Vec::new(), Instant::now());
        while start.elapsed() < timeout {
            for event in script.poll() {
                match event {
                    ScriptEvent::Output(line) => lines.push(line),
                    ScriptEvent::Form(spec) => lines.push(format!("form {spec}")),
                    ScriptEvent::Breaks(plan) => lines.push(format!("breaks {}", plan.breaks.len())),
                    ScriptEvent::Counters(counters) => lines.push(format!("counters {}", counters.len())),
                    ScriptEvent::PinRequest(hash) => lines.push(format!("pin {hash}")),
                    ScriptEvent::HttpRequest(payload) => lines.push(format!("http {payload}")),
                    ScriptEvent::Stats(experience, gold, _) => lines.push(format!("stats {experience} {gold}")),
                    ScriptEvent::Finished { result, stopped } => return (lines, Some((result, stopped))),
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        (lines, None)
    }

    fn start(source: &str) -> Script {
        ensure_runner_built();
        Script::start(&egui::Context::default(), source, None, None, None, &Settings::default()).unwrap()
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

    fn wait_form(script: &mut Script, lines: &mut Vec<String>) -> serde_json::Value {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            for event in script.poll() {
                match event {
                    ScriptEvent::Form(spec) => return spec,
                    ScriptEvent::Output(line) => lines.push(line),
                    ScriptEvent::Stats(..) | ScriptEvent::Breaks(..) | ScriptEvent::Counters(..) | ScriptEvent::PinRequest(..) | ScriptEvent::HttpRequest(..) => {}
                    ScriptEvent::Finished { .. } => panic!("the script ended before asking for a form"),
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the script never asked for a form");
    }

    #[test]
    fn forms_round_trip_through_the_loader() {
        let mut script = start(
            "local s = assert(form({ title = 'Fisher', fields = { { id = 'spot', kind = 'choice', options = { 'Harpoon', 'Cage' } }, { id = 'hours', kind = 'number', default = 2 } } }))\n\
             print(s.spot, s.hours)\n\
             print(form({ title = 'Again', fields = {} }))",
        );
        let mut lines = Vec::new();
        let spec = wait_form(&mut script, &mut lines);
        assert_eq!(spec["title"], "Fisher");
        script.answer_form(Some(serde_json::json!({ "spot": "Cage", "hours": 3 }))).unwrap();
        wait_form(&mut script, &mut lines);
        script.answer_form(None).unwrap();
        let (rest, end) = wait(&mut script, Duration::from_secs(5));
        lines.extend(rest);
        assert_eq!(lines, ["Cage\t3", "nil\tcancelled"]);
        assert_eq!(end, Some((Ok(()), false)));
    }

    #[test]
    fn report_stats_arrives_as_a_stats_event_not_output() {
        let mut script = start("report_stats(10, 5, { 1 }) report_stats(20, -5) print('done')");
        let (lines, end) = wait(&mut script, Duration::from_secs(5));
        assert_eq!(lines, ["stats 10 5", "stats 20 -5", "done"]);
        assert_eq!(end, Some((Ok(()), false)));
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
        assert_eq!(end.map(|(_, stopped)| stopped), Some(true), "JIT-compiled busy loop didn't stop");
    }

    #[cfg(unix)]
    #[test]
    fn pause_suspends_even_a_jit_compiled_busy_loop() {
        let mut script = start("local x = 0 while true do x = x + 1 end sleep(1)");
        thread::sleep(Duration::from_millis(200));
        script.set_paused(true);
        assert!(script.is_running());
        thread::sleep(Duration::from_millis(200));
        assert!(script.is_running(), "a paused process shouldn't exit on its own");
        script.stop();
        let (_, end) = wait(&mut script, Duration::from_secs(3));
        assert_eq!(end.map(|(_, stopped)| stopped), Some(true));
    }

    /// A server on 127.0.0.1 that answers each connection by path: `/hi` with a body, `/hop` with a redirect to `localhost`.
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
                    format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/hi\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                } else {
                    format!("HTTP/1.1 404 Not Found\r\nX-Seen: {}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello", head.contains("x-token: abc") || head.contains("X-Token: abc"))
                };
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        port
    }

    #[test]
    fn http_requests_only_reach_whitelisted_hosts() {
        let port = serve();
        let mut script = start(&format!(
            "local r, e = http_request({{ url = 'http://127.0.0.1:{port}/hi', headers = {{ ['X-Token'] = 'abc' }} }})\n\
             print(r.ok, r.status, r.body, r.headers['x-seen'])\n\
             print(http_request({{ url = 'http://localhost:{port}/hi' }}))\n\
             print(http_request({{ url = 'http://127.0.0.1:{port}/hop' }}))\n\
             print(http_request({{ url = 'ftp://127.0.0.1/' }}))\n\
             print(http_request({{ url = 'http://127.0.0.1:{port}/', headers = {{ Host = 'evil.com' }} }}))"
        ));
        let whitelist = Whitelist::new(vec!["127.0.0.1".into()], &crate::api::Config::default());
        let (mut lines, start) = (Vec::new(), Instant::now());
        let end = loop {
            assert!(start.elapsed() < Duration::from_secs(10), "{lines:?}");
            let mut finished = None;
            for event in script.poll() {
                match event {
                    ScriptEvent::HttpRequest(payload) => script.answer_http(&payload, whitelist.clone()),
                    ScriptEvent::Output(line) => lines.push(line),
                    ScriptEvent::Finished { result, .. } => finished = Some(result),
                    _ => {}
                }
            }
            if let Some(result) = finished {
                break result;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(end, Ok(()), "{lines:?}");
        assert_eq!(lines[0], "false\t404\thello\ttrue");
        assert!(lines[1].starts_with("nil\tlocalhost isn't on the HTTP whitelist"), "{}", lines[1]);
        assert!(lines[2].starts_with("nil\tlocalhost isn't on the HTTP whitelist"), "a redirect off the list is refused: {}", lines[2]);
        assert!(lines[3].starts_with("nil\t") && lines[3].contains("http or https"), "{}", lines[3]);
        assert!(lines[4].contains("header can't be set"), "{}", lines[4]);
    }

    fn live_client() -> Option<(Target, Settings)> {
        let mut settings = Settings::default();
        if settings.helper.is_empty() {
            let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/x86_64-pc-windows-gnu/release/injector.exe");
            if helper.is_file() {
                settings.helper = helper.display().to_string();
            }
        }
        let scan = backend::scan(&settings);
        let Some(target) = scan.targets.into_iter().find(|t| t.client.loaded.is_some()) else {
            eprintln!("skipped, no client with agent loaded was found: {:?}", scan.errors);
            return None;
        };
        Some((target, settings))
    }

    fn run_live(client: &(Target, Settings), source: &str) -> Run {
        let mut script = Script::start(&egui::Context::default(), source, None, None, Some(&client.0), &client.1).unwrap();
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
