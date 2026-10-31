//! Running a store script: start a session, download the live revision into memory, and keep the session billed with heartbeats.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use crate::api::{
    Api, ApiError, Heartbeat, StartedSession, Stats, StoreScript, models::end_reason_message,
};
use crate::libs;

/// The server ends a session after 5 minutes without a heartbeat, so the script is stopped before that.
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(4 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_FILE_BYTES: u64 = 6 * 1024 * 1024;

/// The cache folder: each revision is `<script id>_<revision>` in it, with the shared `lib` beside them.
pub fn cache_root() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.cache_dir().to_path_buf())
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
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then_some(id)
}

pub fn revision_dir(root: &Path, script_id: &str, revision: i64) -> PathBuf {
    root.join(format!("{script_id}_{revision:09}"))
}

/// Removes every cached revision of a script except `keep`.
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

/// Opens a file the webapp encrypted (`nonce || ciphertext || tag`, AES-256-GCM) authenticated with its place, `volcren:<script id>/<revision>/<name>`.
pub fn decrypt_file(
    key_hex: &str,
    script_id: &str,
    revision: i64,
    name: &str,
    sealed: &[u8],
) -> Result<Vec<u8>, String> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
    let failed = || format!("Couldn't decrypt {name}.");
    let key = (key_hex.len() == 64)
        .then(|| {
            (0..32)
                .map(|i| u8::from_str_radix(&key_hex[2 * i..2 * i + 2], 16).ok())
                .collect::<Option<Vec<u8>>>()
        })
        .flatten()
        .ok_or("The server sent a key that can't be used.")?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| "The server sent a key that can't be used.")?,
    );
    if sealed.len() < NONCE_LEN + AES_256_GCM.tag_len() {
        return Err(failed());
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| failed())?;
    let aad = format!("volcren:{script_id}/{revision:09}/{name}");
    let mut buffer = body.to_vec();
    let plain_len = key
        .open_in_place(nonce, Aad::from(aad.as_bytes()), &mut buffer)
        .map_err(|_| failed())?
        .len();
    buffer.truncate(plain_len);
    Ok(buffer)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A revision that is ready to run.
pub struct Prepared {
    pub session: StartedSession,
    pub entry: PathBuf,
    pub source: String,
    /// The script's other files by name. They are only ever in memory, and go to the injector over its stdin.
    pub modules: Vec<(String, String)>,
    /// Something the user should know before the script runs, such as an outdated loader.
    pub warning: Option<String>,
}

/// Starts a session and makes its files available, ending the session if that fails; a community script runs only if its revision is `warned`.
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
pub fn prepare_warned(
    api: &Api,
    script: &StoreScript,
    root: &Path,
    warned: Option<i64>,
) -> Result<Prepared, String> {
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
    match download_to_memory(api, root, id, &session) {
        Ok((entry, source, modules)) => {
            prune_old(root, id, session.revision);
            Ok(Prepared {
                session,
                entry,
                source,
                modules,
                warning: lib.warning,
            })
        }
        Err(error) => {
            let _ = api.end_session(&session.session_id);
            Err(error)
        }
    }
}

fn fetch(
    api: &Api,
    id: &str,
    session: &StartedSession,
    url: &str,
    name: &str,
    sha256: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut response = api
        .agent()
        .get(url)
        .call()
        .map_err(|e| format!("Couldn't download {name}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Couldn't download {name}: the server answered {}.",
            response.status()
        ));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_FILE_BYTES)
        .read_to_vec()
        .map_err(|e| format!("Couldn't download {name}: {e}"))?;
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

// The entry path, its source, and the other modules as (name, source).
type Downloaded = (PathBuf, String, Vec<(String, String)>);

/// A script's files, kept in memory (an earlier cached copy is removed); the entry path points into an empty folder.
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
        let bytes = fetch(api, id, session, &file.url, name, file.sha256.as_deref())?;
        // The script can only require Luau; anything else it ships has nothing to run it.
        if *name != entry_name && !name.ends_with(".luau") && !name.ends_with(".lua") {
            continue;
        }
        let text = String::from_utf8(bytes).map_err(|_| format!("{name} isn't valid text."))?;
        if *name == entry_name {
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

/// What the app should do after a heartbeat answer.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Continue,
    /// Stop the script and say why. `sign_out` is set when the account itself is gone.
    Stop {
        reason: String,
        sign_out: bool,
    },
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

    pub fn on_result(
        &mut self,
        now: Instant,
        sent: Option<Stats>,
        result: Result<Heartbeat, ApiError>,
    ) -> Verdict {
        self.in_flight = false;
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
            Err(ApiError::SignedOut(why)) => Verdict::Stop {
                reason: format!("Signed out: {why}"),
                sign_out: true,
            },
            Err(error @ ApiError::Forbidden(_)) => Verdict::Stop {
                reason: error.to_string(),
                sign_out: true,
            },
            Err(ApiError::NotFound(_)) => Verdict::Stop {
                reason: "The session no longer exists.".into(),
                sign_out: false,
            },
            Err(error) => {
                // Not delivered, so the stats go into the next attempt.
                if let Some(sent) = sent {
                    self.pending.add(sent.experience, sent.gold, &sent.custom);
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
            credits: Some(10),
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
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn heartbeats_are_due_after_the_interval_and_not_while_one_is_in_flight() {
        let t0 = Instant::now();
        let mut beat = Beat::new(t0, Duration::from_secs(60));
        assert!(!beat.due(t0 + Duration::from_secs(59)));
        assert!(beat.due(t0 + Duration::from_secs(60)));
        beat.take(t0 + Duration::from_secs(60));
        assert!(!beat.due(t0 + Duration::from_secs(120)));
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
            credits: None,
        });
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
        assert_eq!(beat.next_due() - now, Duration::from_secs(10));

        // The undelivered stats are sent again.
        now += Duration::from_secs(10);
        let resent = beat.take(now).unwrap();
        assert_eq!(resent.experience, 50);
        assert_eq!(
            beat.on_result(now, Some(resent), network()),
            Verdict::Continue
        );
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
            beat.next_due() - (t0 + Duration::from_secs(70)),
            Duration::from_secs(10)
        );
    }
}

#[cfg(test)]
mod decrypt_tests {
    use super::decrypt_file;

    const SCRIPT: &str = "10000000-0000-0000-0000-000000000001";
    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    /// Made by the webapp's `encryptFile` (src/lib/encryption.ts) for revision 7, `main.luau`, "print('hi')".
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
