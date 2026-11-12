use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    api::Api,
    session::{get_ok, hex_encode},
};

#[cfg(any(windows, test))]
#[path = "update/msi_command.rs"]
mod msi_command;
#[cfg(not(windows))]
#[path = "update/archive.rs"]
mod platform;
#[cfg(windows)]
#[path = "update/msi.rs"]
mod platform;

#[cfg(windows)]
pub use platform::finish_if_asked;
#[cfg(not(windows))]
pub use platform::relaunch_after_exit;
pub use platform::{Prepared, apply, cleanup, prepare};

pub const VERSION: &str = env!("VOLCREN_VERSION");

const PUBLIC_KEYS: [&str; 2] = [
    "RWRprhgKfFuJPJZH0HUjxskMWPFr0zz6oCoTjMXP/8f3cGbD6ff3WKhe",
    "RWSClklMB6socMrdLgZuncW84XWwIJq1ZZS9tyXgoOhqjOde/GzqCdob",
];

const SIGNED_MANIFEST: &str = "latest.signed.json";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const REPAINT_EVERY: Duration = Duration::from_millis(200);
pub const FAILED_ARG: &str = "--update-failed";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub build: u64,
    #[serde(default)]
    pub files: Files,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Files {
    pub windows: Option<File>,
    pub linux: Option<File>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct File {
    pub path: String,
    pub sha256: String,
}

#[derive(Deserialize)]
struct Signed {
    manifest: String,
    signature: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckError {
    Unavailable(String),
    BadSignature,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Dev,
    Manual,
    Auto { admin: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Available {
    pub release: Release,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    cancelled: AtomicBool,
}

const CANCELLED: &str = "The update was skipped.";

impl Progress {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
}

pub struct Instance(Option<fs::File>);

impl Instance {
    pub fn hold() -> Self {
        let path = crate::paths::project_dirs().map(|dirs| dirs.data_dir().join("loader.lock"));
        path.map_or(Self(None), |path| Self::at(&path))
    }

    fn at(path: &Path) -> Self {
        let file = path
            .parent()
            .and_then(|dir| fs::create_dir_all(dir).ok())
            .and_then(|()| {
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                    .ok()
            });
        if let Some(file) = &file {
            let _ = file.lock_shared();
        }
        Self(file)
    }

    pub fn alone(&self) -> bool {
        let Some(file) = &self.0 else { return true };
        let _ = file.unlock();
        let alone = file.try_lock().is_ok();
        let _ = file.unlock();
        let _ = file.lock_shared();
        alone
    }
}

pub fn bucket_url(api: &Api, path: &str) -> String {
    format!(
        "{}/storage/v1/object/public/client/{path}",
        api.config().supabase_url
    )
}

fn own_exe() -> Result<PathBuf, String> {
    env::current_exe().map_err(|e| format!("Can't find the loader's own path: {e}"))
}

fn own_folder(exe: &Path) -> Result<PathBuf, String> {
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "The loader's own path has no folder.".into())
}

fn release_file(release: &Release) -> Result<&File, String> {
    release
        .file()
        .ok_or_else(|| "This release has no build for this system.".into())
}

fn current_build() -> u64 {
    env!("VOLCREN_BUILD").parse().unwrap_or(0)
}

pub fn is_newer(latest: u64, current: u64) -> bool {
    current != 0 && latest > current
}

fn under_target(exe: &Path) -> bool {
    exe.ancestors()
        .any(|dir| dir.file_name().is_some_and(|name| name == "target"))
}

pub fn is_dev_install() -> bool {
    env::current_exe().is_ok_and(|exe| under_target(&exe))
}

pub fn kind() -> Kind {
    match env::current_exe() {
        Ok(exe) if under_target(&exe) => Kind::Dev,
        Ok(exe) => platform::kind(&exe),
        Err(_) => Kind::Manual,
    }
}

pub fn installs_by_itself(kind: Kind, failed: bool, busy: bool) -> bool {
    matches!(kind, Kind::Auto { .. }) && !failed && !busy
}

pub fn failure_from_args() -> Option<String> {
    let mut args = env::args().skip(1);
    args.find(|arg| arg == FAILED_ARG)?;
    args.next()
}

impl Release {
    pub fn file(&self) -> Option<&File> {
        if cfg!(windows) {
            self.files.windows.as_ref()
        } else {
            self.files.linux.as_ref()
        }
    }

    pub fn download_url(&self, api: &Api) -> Option<String> {
        self.file().map(|file| bucket_url(api, &file.path))
    }
}

fn verify(body: &[u8], keys: &[&str]) -> Result<Release, CheckError> {
    let signed: Signed = serde_json::from_slice(body).map_err(|_| CheckError::BadSignature)?;
    let signature = minisign_verify::Signature::decode(&signed.signature)
        .map_err(|_| CheckError::BadSignature)?;
    let trusted = keys.iter().any(|key| {
        minisign_verify::PublicKey::from_base64(key)
            .and_then(|key| key.verify(signed.manifest.as_bytes(), &signature, false))
            .is_ok()
    });
    if !trusted {
        return Err(CheckError::BadSignature);
    }
    serde_json::from_str(&signed.manifest)
        .map_err(|e| CheckError::Unavailable(format!("unexpected update manifest: {e}")))
}

fn check_with(api: &Api, keys: &[&str], build: u64) -> Result<Option<Release>, CheckError> {
    if api.config().supabase_url.is_empty() {
        return Ok(None);
    }
    let mut response = api
        .agent()
        .get(&bucket_url(api, SIGNED_MANIFEST))
        .call()
        .map_err(|e| CheckError::Unavailable(e.to_string()))?;
    if !response.status().is_success() {
        return Err(CheckError::Unavailable(format!(
            "the server answered {}",
            response.status()
        )));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_MANIFEST_BYTES)
        .read_to_vec()
        .map_err(|e| CheckError::Unavailable(e.to_string()))?;
    let release = verify(&body, keys)?;
    Ok(is_newer(release.build, build).then_some(release))
}

pub fn check(api: &Api) -> Result<Option<Available>, CheckError> {
    let release = check_with(api, &PUBLIC_KEYS, current_build())?;
    Ok(release.map(|release| Available {
        release,
        kind: kind(),
    }))
}

fn package_name(path: &str) -> Result<&str, String> {
    let name = path.rsplit('/').next().unwrap_or_default();
    let plain = path
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.contains(['\\', ':', '\0']);
    if plain {
        Ok(name)
    } else {
        Err("the update manifest has a path that can't be used".into())
    }
}

static DOWNLOADS: AtomicU64 = AtomicU64::new(0);

fn download(
    api: &Api,
    file: &File,
    dir: &Path,
    progress: &Progress,
    repaint: &dyn Fn(),
) -> Result<PathBuf, String> {
    let name = package_name(&file.path)?;
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let partial = dir.join(format!(
        "{name}.{}-{}.part",
        std::process::id(),
        DOWNLOADS.fetch_add(1, Ordering::Relaxed)
    ));
    let result = fetch(api, file, &partial, progress, repaint).and_then(|()| progress.check());
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    let path = dir.join(name);
    fs::rename(&partial, &path).map_err(|e| format!("Couldn't save the update: {e}"))?;
    Ok(path)
}

fn fetch(
    api: &Api,
    file: &File,
    to: &Path,
    progress: &Progress,
    repaint: &dyn Fn(),
) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)))
        .user_agent(crate::paths::USER_AGENT)
        .build()
        .into();
    let mut response = get_ok(&agent, &bucket_url(api, &file.path), "the update")?;
    let body = response.body_mut();
    let total = body.content_length().unwrap_or(0);
    if total > MAX_PACKAGE_BYTES {
        return Err("The update is larger than an update can be, so it was discarded.".into());
    }
    progress.total.store(total, Ordering::Relaxed);
    let mut reader = body.with_config().limit(MAX_PACKAGE_BYTES).reader();
    let mut output = fs::File::create(to).map_err(|e| format!("Couldn't save the update: {e}"))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut last_repaint = Instant::now();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Couldn't download the update: {e}"))?;
        if read == 0 {
            break;
        }
        progress.check()?;
        hash.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|e| format!("Couldn't save the update: {e}"))?;
        progress.done.fetch_add(read as u64, Ordering::Relaxed);
        if last_repaint.elapsed() >= REPAINT_EVERY {
            last_repaint = Instant::now();
            repaint();
        }
    }
    let digest = hex_encode(&hash.finalize());
    if digest.eq_ignore_ascii_case(&file.sha256) {
        Ok(())
    } else {
        Err("The update didn't match its checksum, so it was discarded.".into())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{
        api::config::Config,
        fake_api::{FakeServer, Reply},
        session::sha256_hex,
    };

    pub struct Keys {
        pair: minisign::KeyPair,
    }

    impl Keys {
        pub fn new() -> Self {
            Self {
                pair: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
            }
        }

        pub fn public(&self) -> String {
            self.pair.pk.to_base64()
        }

        pub fn envelope(&self, manifest: &str) -> Vec<u8> {
            let signature = minisign::sign(
                None,
                &self.pair.sk,
                Cursor::new(manifest.as_bytes()),
                None,
                None,
            )
            .unwrap();
            serde_json::json!({ "manifest": manifest, "signature": signature.into_string() })
                .to_string()
                .into_bytes()
        }
    }

    const MANIFEST: &str = r#"{"version":"2026.10.26-abc1234","build":1793000000,"api_version":1,"files":{"windows":{"name":"a.msi","path":"2026.10.26-abc1234/a.msi","sha256":"ab"},"linux":{"name":"a.tar.gz","path":"2026.10.26-abc1234/a.tar.gz","sha256":"cd"}}}"#;

    pub fn api(url: &str) -> Api {
        Api::new(Config {
            api_url: url.into(),
            supabase_url: url.into(),
            anon_key: "anon".into(),
            site_url: String::new(),
            prices_url: String::new(),
        })
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("volcren-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_later_build_is_newer_and_an_older_one_of_the_same_day_is_not() {
        assert!(is_newer(1_793_000_001, 1_793_000_000));
        assert!(!is_newer(1_793_000_000, 1_793_000_000));
        assert!(
            !is_newer(1_792_999_999, 1_793_000_000),
            "a replayed manifest"
        );
    }

    #[test]
    fn a_build_without_a_commit_time_never_prompts() {
        assert!(!is_newer(1_793_000_000, 0));
    }

    #[test]
    fn a_good_signature_gives_the_manifest() {
        let keys = Keys::new();
        let release = verify(&keys.envelope(MANIFEST), &[&keys.public()]).unwrap();
        assert_eq!(release.version, "2026.10.26-abc1234");
        assert_eq!(release.build, 1_793_000_000);
        assert_eq!(
            release.files.linux.unwrap().path,
            "2026.10.26-abc1234/a.tar.gz"
        );
    }

    #[test]
    fn the_embedded_keys_accept_what_the_release_tool_signs() {
        let release = verify(include_bytes!("update/signed_fixture.json"), &PUBLIC_KEYS).unwrap();
        assert_eq!(release.build, 0);
    }

    #[test]
    fn the_second_key_is_accepted() {
        let (first, second) = (Keys::new(), Keys::new());
        let envelope = second.envelope(MANIFEST);
        assert!(verify(&envelope, &[&first.public(), &second.public()]).is_ok());
    }

    #[test]
    fn a_tampered_manifest_is_rejected() {
        let keys = Keys::new();
        let envelope = keys.envelope(MANIFEST);
        let text = String::from_utf8(envelope).unwrap();
        let tampered = text.replace("1793000000", "1893000000");
        assert_eq!(
            verify(tampered.as_bytes(), &[&keys.public()]),
            Err(CheckError::BadSignature)
        );
    }

    #[test]
    fn another_key_is_rejected() {
        let (signer, other) = (Keys::new(), Keys::new());
        assert_eq!(
            verify(&signer.envelope(MANIFEST), &[&other.public()]),
            Err(CheckError::BadSignature)
        );
    }

    #[test]
    fn unsigned_and_malformed_bodies_are_rejected() {
        let keys = Keys::new();
        for body in [
            MANIFEST.as_bytes(),
            br#"{"manifest":"{}","signature":"nonsense"}"#,
            b"not json",
        ] {
            assert_eq!(
                verify(body, &[&keys.public()]),
                Err(CheckError::BadSignature)
            );
        }
    }

    #[test]
    fn a_signed_manifest_without_a_build_is_not_used() {
        let keys = Keys::new();
        let result = verify(
            &keys.envelope(r#"{"version":"2026.10.26-abc1234"}"#),
            &[&keys.public()],
        );
        assert!(matches!(result, Err(CheckError::Unavailable(_))));
    }

    #[test]
    fn check_reads_the_signed_object_and_compares_builds() {
        let keys = Keys::new();
        let envelope = keys.envelope(MANIFEST);
        let server = FakeServer::start(move |_, request| {
            if request.path.ends_with("/client/latest.signed.json") {
                Reply::bytes(envelope.clone())
            } else {
                Reply::status(404)
            }
        });
        let api = api(&server.url);
        let public = keys.public();
        assert!(
            check_with(&api, &[&public], 1_792_000_000)
                .unwrap()
                .is_some()
        );
        assert!(
            check_with(&api, &[&public], 1_793_000_000)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_missing_manifest_is_not_a_bad_signature() {
        let server = FakeServer::start(|_, _| Reply::status(404));
        assert!(matches!(
            check_with(&api(&server.url), &[], 1),
            Err(CheckError::Unavailable(_))
        ));
    }

    #[test]
    fn a_checksum_mismatch_keeps_nothing() {
        let server = FakeServer::start(|_, _| Reply::bytes(b"the package".to_vec()));
        let dir = temp_dir("mismatch");
        let file = File {
            path: "1/a.tar.gz".into(),
            sha256: sha256_hex(b"something else"),
        };
        let result = download(&api(&server.url), &file, &dir, &Progress::default(), &|| {});
        assert!(result.unwrap_err().contains("checksum"));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn a_good_download_reports_its_progress() {
        let server = FakeServer::start(|_, _| Reply::bytes(b"the package".to_vec()));
        let dir = temp_dir("download");
        let file = File {
            path: "1/a.tar.gz".into(),
            sha256: sha256_hex(b"the package"),
        };
        let progress = Progress::default();
        let path = download(&api(&server.url), &file, &dir, &progress, &|| {}).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"the package");
        assert_eq!(progress.done.load(Ordering::Relaxed), 11);
    }

    #[test]
    fn a_skipped_download_keeps_nothing() {
        let server = FakeServer::start(|_, _| Reply::bytes(b"the package".to_vec()));
        let dir = temp_dir("skipped");
        let file = File {
            path: "1/a.tar.gz".into(),
            sha256: sha256_hex(b"the package"),
        };
        let progress = Progress::default();
        progress.cancel();
        let result = download(&api(&server.url), &file, &dir, &progress, &|| {});
        assert_eq!(result.unwrap_err(), CANCELLED);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn a_loader_is_alone_only_while_no_other_holds_the_lock() {
        let path = temp_dir("instance").join("loader.lock");
        let first = Instance::at(&path);
        assert!(first.alone());
        let second = Instance::at(&path);
        assert!(!first.alone());
        assert!(!second.alone());
        drop(second);
        assert!(first.alone(), "asking doesn't give up the share");
        assert!(first.alone());
    }

    #[test]
    fn manifest_paths_must_be_plain() {
        assert_eq!(package_name("1/a.msi").unwrap(), "a.msi");
        for path in [
            "../a.msi",
            "1/../a.msi",
            "1//a.msi",
            "1\\a.msi",
            "C:/a.msi",
            "",
        ] {
            assert!(package_name(path).is_err(), "{path}");
        }
    }

    #[test]
    fn an_update_installs_by_itself_only_when_nothing_stands_in_its_way() {
        let auto = Kind::Auto { admin: false };
        assert!(installs_by_itself(auto, false, false));
        assert!(installs_by_itself(Kind::Auto { admin: true }, false, false));
        assert!(!installs_by_itself(auto, true, false), "a failed try");
        assert!(!installs_by_itself(auto, false, true), "a running script");
        assert!(!installs_by_itself(Kind::Dev, false, false));
        assert!(!installs_by_itself(Kind::Manual, false, false));
    }

    #[test]
    fn only_a_target_folder_makes_a_dev_build() {
        assert!(under_target(Path::new("/src/client/target/release/loader")));
        assert!(!under_target(Path::new("/home/me/volcren/loader")));
        assert!(!under_target(Path::new("/home/me/targets/loader")));
    }
}
