use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use crate::api::{
    Api, ApiError, Heartbeat, StartedSession, Stats, StoreScript,
    models::{Ended, Me, Quote, credits_label, end_reason_message},
};
use crate::libs;
use crate::script::Process;

pub const GIVE_UP_AFTER: Duration = Duration::from_secs(4 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const WATCH_EVERY: Duration = Duration::from_millis(250);
const MAX_FILE_BYTES: u64 = 6 * 1024 * 1024;

pub fn cache_root() -> Option<PathBuf> {
    crate::paths::project_dirs().map(|dirs| dirs.cache_dir().to_path_buf())
}

pub fn safe_file_name(name: &str) -> Option<&str> {
    let bad = name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.contains(['/', '\\', ':', '\0'])
        || name.chars().any(char::is_control);
    (!bad).then_some(name)
}

pub fn safe_id(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then_some(id)
}

pub fn revision_dir(root: &Path, script_id: &str, revision: i64) -> PathBuf {
    root.join(format!("{script_id}_{revision:09}"))
}

pub fn prune_old(root: &Path, script_id: &str, keep: i64) {
    let keep = format!("{script_id}_{keep:09}");
    let prefix = format!("{script_id}_");
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub fn decrypt_file(
    key_hex: &str,
    script_id: &str,
    revision: i64,
    name: &str,
    sealed: &[u8],
) -> Result<Vec<u8>, String> {
    let key = hex_decode(key_hex)
        .filter(|key| key.len() == 32)
        .ok_or("The server sent a key that can't be used.")?;
    let aad = format!("volcren:{script_id}/{revision:09}/{name}");
    crate::accounts::open_with(&key, aad.as_bytes(), sealed)
        .ok_or_else(|| format!("Couldn't decrypt {name}."))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    text
}

pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

pub struct Prepared {
    pub session: StartedSession,
    pub entry: PathBuf,
    pub source: String,
    pub modules: Vec<(String, String)>,
    pub warning: Option<String>,
    pub lib_version: String,
}

pub fn prepare(api: &Api, script: &StoreScript, warned: Option<i64>) -> Result<Prepared, String> {
    let root = cache_root().ok_or("Can't find a cache folder on this system.")?;
    prepare_warned(api, script, &root, warned)
}

pub fn quote(api: &Api, script_id: &str) -> Result<(Me, Quote), ApiError> {
    let me = api.me()?;
    Ok((me, api.quote(script_id)?))
}

pub fn restart_quote(
    api: &Api,
    old_session: &str,
    script_id: &str,
    community: bool,
) -> Result<Option<(Me, StoreScript, Quote)>, ApiError> {
    let _ = api.end_session(old_session);
    let me = api.me()?;
    let Some(script) = api
        .scripts("", "", community && me.can_browse_community())?
        .into_iter()
        .find(|s| s.id == script_id)
    else {
        return Ok(None);
    };
    let quote = api.quote(script_id)?;
    Ok(Some((me, script, quote)))
}

pub fn start_block(script: &StoreScript, quote: &Quote, microcredits: i64) -> Option<String> {
    if microcredits < quote.required {
        return Some(format!(
            "Needs a balance of at least {} to start.",
            credits_label(quote.required as f64)
        ));
    }
    if !script.can_access {
        return Some("You don't have access to this script.".into());
    }
    script
        .revision
        .is_none()
        .then(|| "This script has no live revision.".into())
}

pub fn asks_first(old_price: Option<i64>, quote: &Quote) -> bool {
    old_price.is_some_and(|old| old != quote.price) || quote.fee > 0
}

pub fn prepare_warned(
    api: &Api,
    script: &StoreScript,
    root: &Path,
    warned: Option<i64>,
) -> Result<Prepared, String> {
    let id = safe_id(&script.id).ok_or("The server sent a script id that can't be used.")?;
    let lib = libs::ensure(api, root)?;
    let session = api.start_session(id).map_err(|e| e.to_string())?;
    if script.community && warned != Some(session.revision) {
        let _ = api.end_session(&session.session_id);
        return Err(format!(
            "A new revision of {} went live since you looked at it, so it wasn't run. Review the warning and run it again.",
            script.title
        ));
    }
    match download_to_memory(api, root, id, &session) {
        Ok((entry, source, modules)) => {
            prune_old(root, id, session.revision);
            Ok(Prepared {
                session,
                entry,
                source,
                modules,
                warning: lib.warning,
                lib_version: lib.version,
            })
        }
        Err(error) => {
            let _ = api.end_session(&session.session_id);
            Err(error)
        }
    }
}

pub fn get_ok(
    agent: &ureq::Agent,
    url: &str,
    what: &str,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("Couldn't download {what}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Couldn't download {what}: the server answered {}.",
            response.status()
        ));
    }
    Ok(response)
}

pub fn get_bytes(
    agent: &ureq::Agent,
    url: &str,
    what: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    get_ok(agent, url, what)?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| format!("Couldn't download {what}: {e}"))
}

fn fetch(
    api: &Api,
    id: &str,
    session: &StartedSession,
    url: &str,
    name: &str,
    sha256: Option<&str>,
) -> Result<Vec<u8>, String> {
    let bytes = get_bytes(api.agent(), url, name, MAX_FILE_BYTES)?;
    if let Some(expected) = sha256
        && !expected.eq_ignore_ascii_case(&sha256_hex(&bytes))
    {
        return Err(format!(
            "{name} didn't match its checksum, so it was discarded."
        ));
    }
    match &session.key {
        Some(key) => decrypt_file(key, id, session.revision, name, &bytes),
        None => Ok(bytes),
    }
}

type Downloaded = (PathBuf, String, Vec<(String, String)>);

fn download_to_memory(
    api: &Api,
    root: &Path,
    id: &str,
    session: &StartedSession,
) -> Result<Downloaded, String> {
    let names = session
        .files
        .iter()
        .map(|file| {
            safe_file_name(&file.name).ok_or_else(|| {
                format!(
                    "The server sent a file name that can't be used: {:?}",
                    file.name
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let entry_name = safe_file_name(&session.entry)
        .filter(|entry| names.contains(entry))
        .ok_or_else(|| {
            format!(
                "This revision's entry file {:?} isn't one of its files.",
                session.entry
            )
        })?;

    let mut source = None;
    let mut modules = Vec::new();
    for (file, name) in session.files.iter().zip(&names) {
        let is_entry = *name == entry_name;
        if !is_entry && !name.ends_with(".luau") && !name.ends_with(".lua") {
            continue;
        }
        let bytes = fetch(api, id, session, &file.url, name, file.sha256.as_deref())?;
        let text = String::from_utf8(bytes).map_err(|_| format!("{name} isn't valid text."))?;
        if is_entry {
            source = Some(text);
        } else {
            modules.push(((*name).to_owned(), text));
        }
    }
    let dir = revision_dir(root, id, session.revision);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the cache folder: {e}"))?;
    Ok((
        dir.join(entry_name),
        source.ok_or("The entry file wasn't downloaded.")?,
        modules,
    ))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    Stop { reason: String, sign_out: bool },
}

#[derive(Debug)]
pub enum Kept {
    Beat(Heartbeat),
    Stop { reason: String, sign_out: bool },
    Ended(Result<Ended, ApiError>),
}

pub fn keep(
    api: &Api,
    session_id: &str,
    process: &Process,
    interval: Duration,
    report: impl Fn(Kept),
) {
    let mut beat = Beat::new(Instant::now(), interval);
    let mut stopping = false;
    while !process.exited() {
        if stopping || !beat.due(Instant::now()) {
            thread::sleep(WATCH_EVERY);
            continue;
        }
        let stats = process.take_stats();
        beat.report(stats.experience, stats.gold, &stats.custom);
        let sent = beat.take(Instant::now());
        let result = api.heartbeat(session_id, sent.as_ref());
        if let Ok(answer) = &result {
            report(Kept::Beat(answer.clone()));
        }
        if let Verdict::Stop { reason, sign_out } = beat.on_result(Instant::now(), sent, result) {
            process.stop();
            stopping = true;
            report(Kept::Stop { reason, sign_out });
        }
    }
    report(Kept::Ended(api.end_session(session_id)));
}

pub struct Beat {
    interval: Duration,
    next_due: Instant,
    last_ok: Instant,
    last_taken: Instant,
    failures: u32,
    pending: Stats,
}

impl Beat {
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            interval,
            next_due: now + interval,
            last_ok: now,
            last_taken: now,
            failures: 0,
            pending: Stats::default(),
        }
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_due
    }

    pub fn report(&mut self, experience: i64, gold: i64, custom: &[i64]) {
        self.pending.add(experience, gold, custom);
    }

    pub fn take(&mut self, now: Instant) -> Option<Stats> {
        let mut stats = std::mem::take(&mut self.pending);
        stats.minutes += now.saturating_duration_since(self.last_taken).as_secs_f64() / 60.0;
        self.last_taken = now;
        (!stats.is_empty()).then_some(stats)
    }

    pub fn on_result(
        &mut self,
        now: Instant,
        sent: Option<Stats>,
        result: Result<Heartbeat, ApiError>,
    ) -> Verdict {
        match result {
            Ok(beat) if beat.alive => {
                self.last_ok = now;
                self.failures = 0;
                self.next_due = now + self.interval;
                Verdict::Continue
            }
            Ok(beat) => Verdict::Stop {
                reason: end_reason_message(beat.reason.as_deref()).into(),
                sign_out: false,
            },
            Err(error @ (ApiError::SignedOut(_) | ApiError::Forbidden(_))) => Verdict::Stop {
                reason: error.to_string(),
                sign_out: true,
            },
            Err(ApiError::NotFound(_)) => Verdict::Stop {
                reason: "The session no longer exists.".into(),
                sign_out: false,
            },
            Err(error) => {
                if let Some(sent) = sent {
                    self.pending.add(sent.experience, sent.gold, &sent.custom);
                    self.pending.minutes += sent.minutes;
                }
                self.failures += 1;
                let backoff = Duration::from_secs(5u64 << self.failures.min(3)).min(MAX_BACKOFF);
                self.next_due = now + backoff;
                if now.saturating_duration_since(self.last_ok) > GIVE_UP_AFTER {
                    Verdict::Stop {
                        reason: format!(
                            "Connection lost, so the script was stopped before the server ends the session. ({error})"
                        ),
                        sign_out: false,
                    }
                } else {
                    Verdict::Continue
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive() -> Result<Heartbeat, ApiError> {
        Ok(Heartbeat {
            alive: true,
            reason: None,
            microcredits: Some(1_000),
            rank: None,
            fee: None,
            fee_charged: None,
        })
    }

    fn network() -> Result<Heartbeat, ApiError> {
        Err(ApiError::Network("down".into()))
    }

    #[test]
    fn file_names_cannot_escape_the_revision_folder() {
        for bad in [
            "",
            "../x",
            "a/b",
            "a\\b",
            ".hidden",
            "C:x",
            "a\0b",
            "line\nbreak",
        ] {
            assert_eq!(safe_file_name(bad), None, "{bad:?}");
        }
        assert_eq!(safe_file_name("main.luau"), Some("main.luau"));
        assert_eq!(safe_file_name("util_2.luau"), Some("util_2.luau"));
    }

    #[test]
    fn ids_must_look_like_uuids() {
        assert!(safe_id("7d081fdd-59de-4a4e-9c29-b2f92d9bc697").is_some());
        assert!(safe_id("../etc").is_none());
        assert!(safe_id("").is_none());
    }

    #[test]
    fn old_revisions_are_pruned_and_the_current_one_stays() {
        let root = std::env::temp_dir().join(format!("volcren-prune-{}", std::process::id()));
        for revision in [1, 2, 3] {
            fs::create_dir_all(revision_dir(&root, "s", revision)).unwrap();
        }
        prune_old(&root, "s", 3);
        assert!(revision_dir(&root, "s", 3).is_dir());
        assert!(!revision_dir(&root, "s", 1).exists());
        assert!(!revision_dir(&root, "s", 2).exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(
            hex_decode(&hex_encode(&[0, 15, 255])),
            Some(vec![0, 15, 255])
        );
        assert_eq!(hex_decode("abc"), None);
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode("+a"), None);
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn heartbeats_are_due_an_interval_after_the_last_answer() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        assert!(!beat.due(t0 + Duration::from_secs(59)));
        assert!(beat.due(t0 + Duration::from_secs(60)));
        beat.take(t0 + Duration::from_secs(60));
        assert_eq!(
            beat.on_result(t0 + Duration::from_secs(61), None, alive()),
            Verdict::Continue
        );
        assert!(beat.due(t0 + Duration::from_secs(121)));
    }

    #[test]
    fn minutes_come_from_the_session_clock() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        beat.report(100, 5, &[]);
        let stats = beat.take(t0 + Duration::from_secs(90)).unwrap();
        assert_eq!((stats.experience, stats.gold), (100, 5));
        assert_eq!(stats.minutes, 1.5);
        assert!(
            beat.take(t0 + Duration::from_secs(150)).is_none(),
            "nothing reported means nothing sent"
        );
    }

    #[test]
    fn an_ended_session_stops_the_script_with_the_reason() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        let dead = Ok(Heartbeat {
            alive: false,
            reason: Some("insufficient_credits".into()),
            microcredits: None,
            rank: None,
            fee: None,
            fee_charged: None,
        });
        match beat.on_result(t0, None, dead) {
            Verdict::Stop { reason, sign_out } => {
                assert!(reason.contains("add funds"));
                assert!(!sign_out);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_ban_stops_the_script_and_signs_out() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        let verdict = beat.on_result(
            t0,
            None,
            Err(ApiError::Forbidden("This account is banned.".into())),
        );
        assert_eq!(
            verdict,
            Verdict::Stop {
                reason: "This account is banned.".into(),
                sign_out: true
            }
        );
    }

    #[test]
    fn network_errors_retry_with_backoff_keep_the_stats_and_finally_give_up() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        beat.report(50, 0, &[]);
        let mut now = t0 + Duration::from_secs(60);
        let sent = beat.take(now);
        assert_eq!(beat.on_result(now, sent, network()), Verdict::Continue);
        assert_eq!(beat.next_due - now, Duration::from_secs(10));

        now += Duration::from_secs(10);
        let resent = beat.take(now).unwrap();
        assert_eq!(resent.experience, 50);
        assert!(
            (resent.minutes - 70.0 / 60.0).abs() < 1e-9,
            "the failed window's minutes come with its stats: {}",
            resent.minutes
        );
        assert_eq!(
            beat.on_result(now, Some(resent), network()),
            Verdict::Continue
        );
        assert_eq!(beat.next_due - now, Duration::from_secs(20));

        let mut verdict = Verdict::Continue;
        while verdict == Verdict::Continue {
            now += Duration::from_secs(30);
            assert!(now - t0 < Duration::from_secs(600), "never gave up");
            beat.take(now);
            verdict = beat.on_result(now, None, network());
        }
        assert!(now - t0 > GIVE_UP_AFTER);
        assert!(matches!(
            verdict,
            Verdict::Stop {
                sign_out: false,
                ..
            }
        ));
    }

    #[test]
    fn a_success_resets_the_failure_count() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        beat.take(t0);
        beat.on_result(t0, None, network());
        beat.take(t0 + Duration::from_secs(10));
        beat.on_result(t0 + Duration::from_secs(10), None, alive());
        beat.take(t0 + Duration::from_secs(70));
        beat.on_result(t0 + Duration::from_secs(70), None, network());
        assert_eq!(
            beat.next_due - (t0 + Duration::from_secs(70)),
            Duration::from_secs(10)
        );
    }
}

#[cfg(test)]
mod decrypt_tests {
    use super::decrypt_file;

    const SCRIPT: &str = "10000000-0000-0000-0000-000000000001";
    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const SEALED: &str =
        "e215109cbac11277ceb0ba7977032371b4effbe804668d0342d0f5393ed8dda6488e183c1d10c4";

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn opens_what_the_webapp_sealed() {
        assert_eq!(
            decrypt_file(KEY, SCRIPT, 7, "main.luau", &bytes(SEALED)).unwrap(),
            b"print('hi')"
        );
    }

    #[test]
    fn a_file_only_opens_in_its_own_place() {
        let sealed = bytes(SEALED);
        for (script, revision, name) in [
            (SCRIPT, 8, "main.luau"),
            (SCRIPT, 7, "other.luau"),
            ("20000000-0000-0000-0000-000000000001", 7, "main.luau"),
        ] {
            assert!(
                decrypt_file(KEY, script, revision, name, &sealed).is_err(),
                "{script} {revision} {name}"
            );
        }
    }

    #[test]
    fn tampering_a_wrong_key_and_short_input_are_refused() {
        let mut sealed = bytes(SEALED);
        *sealed.last_mut().unwrap() ^= 1;
        assert!(decrypt_file(KEY, SCRIPT, 7, "main.luau", &sealed).is_err());
        assert!(decrypt_file(&"ab".repeat(32), SCRIPT, 7, "main.luau", &bytes(SEALED)).is_err());
        assert!(decrypt_file(KEY, SCRIPT, 7, "main.luau", &[0; 10]).is_err());
        assert!(decrypt_file("not hex", SCRIPT, 7, "main.luau", &bytes(SEALED)).is_err());
    }
}
