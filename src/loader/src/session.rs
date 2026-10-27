//! Running a store script: start a session, download the live revision into the cache, and keep the session billed with heartbeats.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use crate::libs;
use crate::api::{Api, ApiError, Heartbeat, StartedSession, Stats, StoreScript, models::end_reason_message};

/// The server ends a session after 5 minutes without a heartbeat, so the script is stopped before that.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(4 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_FILE_BYTES: u64 = 6 * 1024 * 1024;
const ENTRY: &str = "main.luau";
const COMPLETE: &str = ".complete";

/// The cache folder. Each revision is a folder `<script id>_<revision>` directly in it, and the shared lib is `lib` beside them,
/// so a script's `require("../lib/...")` finds it.
pub fn cache_root() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren").map(|dirs| dirs.cache_dir().to_path_buf())
}

/// A file name from the server, accepted only if it can't leave the revision's folder.
pub fn safe_file_name(name: &str) -> Option<&str> {
    let bad = name.is_empty()
        || name.len() > 255
        || name.starts_with('.')
        || name.contains(['/', '\\', ':', '\0'])
        || name.chars().any(char::is_control);
    (!bad).then_some(name)
}

/// Script ids are UUIDs; anything else would be a path component we shouldn't trust.
pub fn safe_id(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(id)
}

pub fn revision_dir(root: &Path, script_id: &str, revision: i64) -> PathBuf {
    root.join(format!("{script_id}_{revision:09}"))
}

/// Removes every cached revision of a script except `keep`.
pub fn prune_old(root: &Path, script_id: &str, keep: i64) {
    let keep = format!("{script_id}_{keep:09}");
    let prefix = format!("{script_id}_");
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A revision that is ready to run.
pub struct Prepared {
    pub session: StartedSession,
    pub entry: PathBuf,
    pub source: String,
    /// Something the user should know before the script runs, such as an outdated loader.
    pub warning: Option<String>,
}

/// Starts a session and makes sure its files are on disk. If anything after the start fails, the session is ended so the user isn't billed.
///
/// `warned` is the revision of a community script the user was warned about and agreed to run (see `store::Confirmed`).
/// A community script only runs if the revision the server started is that one, so a revision published since the
/// warning was shown is never run unseen.
pub fn prepare(api: &Api, script: &StoreScript, warned: Option<i64>) -> Result<Prepared, String> {
    let root = cache_root().ok_or("Can't find a cache folder on this system.")?;
    prepare_warned(api, script, &root, warned)
}

/// `prepare_warned` for a script that needs no warning, with the cache folder given so tests don't touch the real one.
#[cfg(test)]
pub fn prepare_in(api: &Api, script: &StoreScript, root: &Path) -> Result<Prepared, String> {
    prepare_warned(api, script, root, None)
}

/// `prepare` with the cache folder given.
pub fn prepare_warned(api: &Api, script: &StoreScript, root: &Path, warned: Option<i64>) -> Result<Prepared, String> {
    let id = safe_id(&script.id).ok_or("The server sent a script id that can't be used.")?;
    // Before the session starts, so downloading the lib is never billed.
    let lib = libs::ensure(api, root)?;
    let session = api.start_session(id).map_err(|e| e.to_string())?;
    if script.community && warned != Some(session.revision) {
        let _ = api.end_session(&session.session_id);
        return Err(format!(
            "A new revision of {} went live since you looked at it, so it wasn't run. Review the warning and run it again.",
            script.title
        ));
    }
    match download(api, root, id, &session) {
        Ok((entry, source)) => {
            prune_old(root, id, session.revision);
            Ok(Prepared { session, entry, source, warning: lib.warning })
        }
        Err(error) => {
            let _ = api.end_session(&session.session_id);
            Err(error)
        }
    }
}

fn download(api: &Api, root: &Path, id: &str, session: &StartedSession) -> Result<(PathBuf, String), String> {
    let dir = revision_dir(root, id, session.revision);
    let names = session
        .files
        .iter()
        .map(|file| safe_file_name(&file.name).ok_or_else(|| format!("The server sent a file name that can't be used: {:?}", file.name)))
        .collect::<Result<Vec<_>, _>>()?;
    if !names.contains(&ENTRY) {
        return Err(format!("This revision has no {ENTRY}."));
    }

    // A finished download of this revision is reused. Revisions never change once published.
    if !(dir.join(COMPLETE).is_file() && names.iter().all(|name| dir.join(name).is_file())) {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create the cache folder: {e}"))?;
        for (file, name) in session.files.iter().zip(&names) {
            let mut response = api
                .agent()
                .get(&file.url)
                .call()
                .map_err(|e| format!("Couldn't download {name}: {e}"))?;
            if !response.status().is_success() {
                return Err(format!("Couldn't download {name}: the server answered {}.", response.status()));
            }
            let bytes = response
                .body_mut()
                .with_config()
                .limit(MAX_FILE_BYTES)
                .read_to_vec()
                .map_err(|e| format!("Couldn't download {name}: {e}"))?;
            if let Some(expected) = &file.sha256
                && !expected.eq_ignore_ascii_case(&sha256_hex(&bytes))
            {
                return Err(format!("{name} didn't match its checksum, so it was discarded."));
            }
            fs::write(dir.join(name), &bytes).map_err(|e| format!("Couldn't save {name}: {e}"))?;
        }
        fs::write(dir.join(COMPLETE), b"").map_err(|e| format!("Couldn't finish the download: {e}"))?;
    }

    let entry = dir.join(ENTRY);
    let mut source = String::new();
    fs::File::open(&entry)
        .and_then(|mut file| file.read_to_string(&mut source))
        .map_err(|e| format!("Couldn't read {ENTRY}: {e}"))?;
    Ok((entry, source))
}

/// What the app should do after a heartbeat answer.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// Stop the script and say why. `sign_out` is set when the account itself is gone.
    Stop { reason: String, sign_out: bool },
}

/// The heartbeat timer of one running store script. It owns no clock: callers pass `now`, so tests can drive it.
pub struct Beat {
    interval: Duration,
    next_due: Instant,
    last_ok: Instant,
    last_taken: Instant,
    in_flight: bool,
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
            in_flight: false,
            failures: 0,
            pending: Stats::default(),
        }
    }

    pub fn due(&self, now: Instant) -> bool {
        !self.in_flight && now >= self.next_due
    }

    pub fn next_due(&self) -> Instant {
        self.next_due
    }

    /// Stats the script reported since the last heartbeat.
    pub fn report(&mut self, experience: i64, gold: i64, custom: &[i64]) {
        self.pending.add(experience, gold, custom);
    }

    /// Starts a heartbeat and returns what to send: the pending stats with the minutes since the last one, measured here.
    pub fn take(&mut self, now: Instant) -> Option<Stats> {
        self.in_flight = true;
        let mut stats = std::mem::take(&mut self.pending);
        stats.minutes = now.saturating_duration_since(self.last_taken).as_secs_f64() / 60.0;
        self.last_taken = now;
        (!stats.is_empty()).then_some(stats)
    }

    pub fn on_result(&mut self, now: Instant, sent: Option<Stats>, result: Result<Heartbeat, ApiError>) -> Verdict {
        self.in_flight = false;
        match result {
            Ok(beat) if beat.alive => {
                self.last_ok = now;
                self.failures = 0;
                self.next_due = now + self.interval;
                Verdict::Continue
            }
            Ok(beat) => Verdict::Stop { reason: end_reason_message(beat.reason.as_deref()).into(), sign_out: false },
            Err(ApiError::SignedOut(why)) => Verdict::Stop { reason: format!("Signed out: {why}"), sign_out: true },
            Err(error @ ApiError::Forbidden(_)) => Verdict::Stop { reason: error.to_string(), sign_out: true },
            Err(ApiError::NotFound(_)) => Verdict::Stop { reason: "The session no longer exists.".into(), sign_out: false },
            Err(error) => {
                // Not delivered, so the stats go into the next attempt.
                if let Some(sent) = sent {
                    self.pending.add(sent.experience, sent.gold, &sent.custom);
                }
                self.failures += 1;
                let backoff = Duration::from_secs(5u64 << self.failures.min(3)).min(MAX_BACKOFF);
                self.next_due = now + backoff;
                if now.saturating_duration_since(self.last_ok) > GIVE_UP_AFTER {
                    Verdict::Stop { reason: format!("Connection lost, so the script was stopped before the server ends the session. ({error})"), sign_out: false }
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
        Ok(Heartbeat { alive: true, reason: None, credits: Some(10) })
    }

    fn network() -> Result<Heartbeat, ApiError> {
        Err(ApiError::Network("down".into()))
    }

    #[test]
    fn file_names_cannot_escape_the_revision_folder() {
        for bad in ["", "../x", "a/b", "a\\b", ".hidden", "C:x", "a\0b", "line\nbreak"] {
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
    fn sha256_is_lowercase_hex() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn heartbeats_are_due_after_the_interval_and_not_while_one_is_in_flight() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        assert!(!beat.due(t0 + Duration::from_secs(59)));
        assert!(beat.due(t0 + Duration::from_secs(60)));
        beat.take(t0 + Duration::from_secs(60));
        assert!(!beat.due(t0 + Duration::from_secs(120)));
        assert_eq!(beat.on_result(t0 + Duration::from_secs(61), None, alive()), Verdict::Continue);
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
        assert!(beat.take(t0 + Duration::from_secs(150)).is_none(), "nothing reported means nothing sent");
    }

    #[test]
    fn an_ended_session_stops_the_script_with_the_reason() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        let dead = Ok(Heartbeat { alive: false, reason: Some("insufficient_credits".into()), credits: None });
        match beat.on_result(t0, None, dead) {
            Verdict::Stop { reason, sign_out } => {
                assert!(reason.contains("top up"));
                assert!(!sign_out);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_ban_stops_the_script_and_signs_out() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        let verdict = beat.on_result(t0, None, Err(ApiError::Forbidden("This account is banned.".into())));
        assert_eq!(verdict, Verdict::Stop { reason: "This account is banned.".into(), sign_out: true });
    }

    #[test]
    fn network_errors_retry_with_backoff_keep_the_stats_and_finally_give_up() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        beat.report(50, 0, &[]);
        let mut now = t0 + Duration::from_secs(60);
        let sent = beat.take(now);
        assert_eq!(beat.on_result(now, sent, network()), Verdict::Continue);
        assert_eq!(beat.next_due() - now, Duration::from_secs(10));

        // The undelivered stats are sent again.
        now += Duration::from_secs(10);
        let resent = beat.take(now).unwrap();
        assert_eq!(resent.experience, 50);
        assert_eq!(beat.on_result(now, Some(resent), network()), Verdict::Continue);
        assert_eq!(beat.next_due() - now, Duration::from_secs(20));

        // Backoff is capped, and after 4 minutes without a success the script is stopped.
        let mut verdict = Verdict::Continue;
        while verdict == Verdict::Continue {
            now += Duration::from_secs(30);
            assert!(now - t0 < Duration::from_secs(600), "never gave up");
            beat.take(now);
            verdict = beat.on_result(now, None, network());
        }
        assert!(now - t0 > GIVE_UP_AFTER);
        assert!(matches!(verdict, Verdict::Stop { sign_out: false, .. }));
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
        assert_eq!(beat.next_due() - (t0 + Duration::from_secs(70)), Duration::from_secs(10));
    }
}
