//! The transport data (teleport spells, charter ships, ...): checks upstream once a day, keeps the last versions, and builds the line that gives it to scripts.
//!
//! A version is one upstream commit, kept in its own folder. A new one is used only after every file of it has downloaded and parsed, so a bad answer (an error page, a half file) never replaces a good version.
//! With no version kept, or a kept file damaged, the copy bundled with the loader is used.
//! Scripts never touch the network for this: the loader makes the requests, to fixed addresses, and the HTTP whitelist doesn't apply. A script keeps the data it started with.

use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use injector::transports::{self, Data, ItemName, Transport};

/// The commit of Skretzo/shortest-path that `data/` was taken from. Bump it together with `data/`.
const BUNDLED_COMMIT: &str = "4fd7d4a220c06781145dd7ba21e9a1e46ed0fc27";
const REPO: &str = "Skretzo/shortest-path";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FILE: u64 = 1 << 20;
/// How often upstream is asked for a new version, and how often the loader looks whether it is time to. A failed check is tried again at the next look.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);
/// How many versions are kept, the one in use among them. Rolling back past the last one lands on the bundled copy.
const KEPT_VERSIONS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A file of transports (`Destination` column and so on).
    Transports,
    /// `item-variations.tsv`: the named item groups the `Items` column uses.
    ItemNames,
}

struct File {
    /// The cache file's name, and the transports' `kind`.
    name: &'static str,
    /// The path under `src/main/resources/` upstream.
    path: &'static str,
    kind: Kind,
    /// The copy bundled with the loader.
    bundled: &'static str,
    /// Keeps only the rows of a transports file that scripts can use.
    keep: Option<fn(&Transport) -> bool>,
    /// Also takes the planner's climbs out of the file.
    climbs: Option<fn(&Transport) -> bool>,
}

/// A toll barrier (`Pay-toll(2-Ecto) Energy Barrier 16105`): a short hop through a gate that the walker's own planner takes for a wall.
fn is_toll(t: &Transport) -> bool {
    t.origin.is_some() && t.destination.is_some() && t.menu.starts_with("Pay-toll")
}

/// The rows of `transports.tsv` the script takes as whole trips.
fn is_trip(t: &Transport) -> bool {
    is_toll(t) || transports::is_far_hop(t) || transports::is_item_on(t)
}

/// Files whose bundled copy is always used: it differs from upstream's, which would replace it.
const BUNDLED_ONLY: &[&str] = &["teleportation_levers"];

macro_rules! transports_file {
    ($name:literal) => {
        transports_file!($name, None, None)
    };
    ($name:literal, $keep:expr, $climbs:expr) => {
        File {
            name: $name,
            path: concat!("transports/", $name, ".tsv"),
            kind: Kind::Transports,
            bundled: include_str!(concat!("../data/transports/", $name, ".tsv")),
            keep: $keep,
            climbs: $climbs,
        }
    };
}

/// The files to load. An explicit list, so a stray file upstream is never read as a transport.
const FILES: &[File] = &[
    transports_file!("teleportation_spells"),
    transports_file!("teleportation_items"),
    transports_file!("teleportation_spells_home"),
    transports_file!("teleportation_minigames"),
    transports_file!("charter_ships"),
    transports_file!("magic_mushtrees"),
    transports_file!("quetzals"),
    transports_file!("quetzal_whistle"),
    transports_file!("spirit_trees"),
    transports_file!("gnome_gliders"),
    transports_file!("hot_air_balloons"),
    transports_file!("fairy_rings"),
    transports_file!("boats"),
    transports_file!("ships"),
    transports_file!("minecarts"),
    transports_file!("magic_carpets"),
    transports_file!("canoes"),
    transports_file!("teleportation_portals"),
    transports_file!("wilderness_obelisks"),
    transports_file!("teleportation_portals_poh"),
    transports_file!("teleportation_boxes"),
    transports_file!("teleportation_levers"),
    File {
        name: "item-variations",
        path: "item-variations.tsv",
        kind: Kind::ItemNames,
        bundled: include_str!("../data/item-variations.tsv"),
        keep: None,
        climbs: None,
    },
    transports_file!("transports", Some(is_trip), Some(transports::is_climb)),
    // the script gets only the copies that ask for something, and the planner has the rest
    transports_file!(
        "agility_shortcuts",
        Some(transports::is_item_on),
        Some(transports::is_shortcut)
    ),
];

/// One upstream commit that changed the data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Version {
    commit: String,
    date: String,
}

/// What is kept, saved next to the versions' folders.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Versions {
    /// The one in use first, then the older ones. Empty: the bundled copy.
    kept: Vec<Version>,
    /// Unix seconds of the last answer from upstream.
    checked_at: u64,
    /// A commit rolled back from: the daily check leaves it alone, the button doesn't.
    skip: Option<String>,
}

fn is_commit(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

fn root_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.data_dir().join("transports"))
}

fn read_versions(root: &Path) -> Versions {
    std::fs::read_to_string(root.join("versions.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_versions(root: &Path, versions: &Versions) -> Result<(), String> {
    let text = serde_json::to_string_pretty(versions).map_err(|e| e.to_string())?;
    write_file(&root.join("versions.json"), &text).map_err(|e| e.to_string())
}

/// The newest commit from the commits API's answer. The answer isn't trusted: the commit goes in a path and an address.
fn parse_latest(body: &str) -> Result<Version, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let newest = value.get(0).ok_or("no commits in the answer")?;
    let commit = newest["sha"].as_str().unwrap_or_default();
    if !is_commit(commit) {
        return Err(format!("not a commit: {commit:?}"));
    }
    Ok(Version {
        commit: commit.to_owned(),
        date: newest["commit"]["committer"]["date"]
            .as_str()
            .and_then(|d| d.get(..10))
            .unwrap_or_default()
            .to_owned(),
    })
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(MAX_FILE)
        .read_to_string()
        .map_err(|e| e.to_string())
}

fn latest(agent: &ureq::Agent) -> Result<Version, String> {
    parse_latest(&get(
        agent,
        &format!(
            "https://api.github.com/repos/{REPO}/commits?sha=master&path=src/main/resources&per_page=1"
        ),
    )?)
}

fn download(agent: &ureq::Agent, commit: &str, file: &File) -> Result<String, String> {
    get(
        agent,
        &format!(
            "https://raw.githubusercontent.com/{REPO}/{commit}/src/main/resources/{}",
            file.path
        ),
    )
}

/// What a file holds.
enum Parsed {
    /// The rows for scripts, and the planner's climbs.
    Transports(Vec<Transport>, Vec<Transport>),
    ItemNames(Vec<ItemName>),
}

/// The contents of a file, or why it can't be used. A file with nothing usable in it is no good (an empty answer would hide the bundled copy).
fn validate(file: &File, text: &str) -> Result<Parsed, String> {
    match file.kind {
        Kind::Transports => {
            let (mut rows, skipped) = transports::parse_file(file.name, text)?;
            for why in skipped {
                eprintln!("transports: skipped {why}");
            }
            // the climbs: all of them for the planner, and those that ask for something also for the script, which checks them
            let climbs: Vec<Transport> = match file.climbs {
                Some(accept) => rows.iter().filter(|t| accept(t)).cloned().collect(),
                None => Vec::new(),
            };
            if let Some(keep) = file.keep {
                rows.retain(keep);
            }
            if file.climbs.is_some() && climbs.is_empty() {
                return Err(format!("{}: no climbs", file.name));
            }
            if rows.is_empty() && climbs.is_empty() {
                return Err(format!("{}: no rows", file.name));
            }
            rows.extend(
                climbs
                    .iter()
                    .filter(|t| transports::gated(t))
                    .map(|t| Transport {
                        kind: transports::gated_kind(t).into(),
                        ..t.clone()
                    }),
            );
            Ok(Parsed::Transports(rows, climbs))
        }
        Kind::ItemNames => {
            let names = transports::parse_item_variations(text)?;
            if names.is_empty() {
                return Err(format!("{}: no items", file.name));
            }
            Ok(Parsed::ItemNames(names))
        }
    }
}

/// Writes next to the file and renames, so a crash never leaves half a file.
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(temp, path)
}

/// The data of a version's folder, with the bundled copy for any file it lacks or that doesn't parse. `None`: all of it bundled.
fn load(version: Option<(&Path, &str)>) -> Data {
    let mut data = Data::default();
    let mut whole = version.is_some();
    for file in FILES {
        let kept = version
            .filter(|_| !BUNDLED_ONLY.contains(&file.name))
            .and_then(|(dir, _)| {
                std::fs::read_to_string(dir.join(format!("{}.tsv", file.name))).ok()
            })
            .and_then(|text| validate(file, &text).ok());
        if kept.is_none() && version.is_some() && !BUNDLED_ONLY.contains(&file.name) {
            eprintln!("transports: {}: using the bundled copy", file.name);
            whole = false;
        }
        let parsed =
            kept.unwrap_or_else(|| validate(file, file.bundled).expect("the bundled file parses"));
        match parsed {
            Parsed::Transports(rows, climbs) => {
                data.transports.extend(rows);
                data.climbs.extend(climbs);
            }
            Parsed::ItemNames(names) => data.items = names,
        }
    }
    data.source = match version {
        Some((_, commit)) if whole => short(commit).to_owned(),
        Some((_, commit)) => format!("{}, part bundled", short(commit)),
        None => "bundled".to_owned(),
    };
    data
}

/// The data of the version in use.
fn load_current(root: Option<&Path>) -> Data {
    let Some(root) = root else {
        return load(None);
    };
    match read_versions(root).kept.first() {
        Some(version) => load(Some((&root.join(&version.commit), &version.commit))),
        None => load(None),
    }
}

/// Removes the folders of versions no longer kept. Only folders named like a commit: nothing else in there is ours to remove.
fn prune(root: &Path, versions: &Versions) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_commit(&name)
            && !versions.kept.iter().any(|v| v.commit == name)
            && let Err(e) = std::fs::remove_dir_all(entry.path())
        {
            eprintln!("transports: couldn't remove the version {name}: {e}");
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Checked {
    UpToDate,
    Skipped,
    New(Version),
}

/// Asks upstream for its newest version, and takes it if every file downloads and parses. `by_hand` also takes one that was rolled back from.
fn update(
    root: &Path,
    by_hand: bool,
    now: u64,
    newest: impl Fn() -> Result<Version, String>,
    fetch: impl Fn(&str, &File) -> Result<String, String>,
) -> Result<Checked, String> {
    let mut versions = read_versions(root);
    let new = newest()?;
    versions.checked_at = now;
    if versions
        .kept
        .first()
        .is_some_and(|v| v.commit == new.commit)
    {
        write_versions(root, &versions)?;
        return Ok(Checked::UpToDate);
    }
    if !by_hand && versions.skip.as_deref() == Some(&new.commit) {
        write_versions(root, &versions)?;
        return Ok(Checked::Skipped);
    }
    let mut texts = Vec::new();
    for file in FILES.iter().filter(|f| !BUNDLED_ONLY.contains(&f.name)) {
        let text = fetch(&new.commit, file).map_err(|e| format!("{}: {e}", file.name))?;
        validate(file, &text).map_err(|e| format!("{}: {e}", file.name))?;
        texts.push((file.name, text));
    }
    let dir = root.join(&new.commit);
    for (name, text) in &texts {
        write_file(&dir.join(format!("{name}.tsv")), text).map_err(|e| e.to_string())?;
    }
    versions.kept.insert(0, new.clone());
    versions.kept.truncate(KEPT_VERSIONS);
    versions.skip = None;
    write_versions(root, &versions)?;
    prune(root, &versions);
    Ok(Checked::New(new))
}

/// Drops the version in use, so the one before it (or the bundled copy) takes over, and the daily check leaves it alone. What is in use now, or why it couldn't.
fn roll_back(root: &Path) -> Result<String, String> {
    let mut versions = read_versions(root);
    if versions.kept.is_empty() {
        return Err("already on the bundled data".to_owned());
    }
    let dropped = versions.kept.remove(0);
    versions.skip = Some(dropped.commit);
    write_versions(root, &versions)?;
    prune(root, &versions);
    Ok(versions
        .kept
        .first()
        .map_or("the bundled data".to_owned(), |v| {
            short(&v.commit).to_owned()
        }))
}

/// What the Settings page shows.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// The version in use, like `1a2b3c4 (2026-11-03)`, or `bundled (4fd7d4a)`.
    pub version: String,
    /// Unix seconds of the last answer from upstream; 0 for none.
    pub checked_at: u64,
    pub can_roll_back: bool,
    pub busy: bool,
    /// What the last button or check came to.
    pub message: String,
}

/// The line built from the version in use, once it has been read.
static LOADED: Mutex<Option<String>> = Mutex::new(None);
static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

fn lock<T>(mutex: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Reads the version in use again, for the scripts started from now on.
fn reload(root: Option<&Path>, message: Option<String>) {
    let data = load_current(root);
    eprintln!(
        "transports: {} rows, {} item names ({})",
        data.transports.len(),
        data.items.len(),
        data.source
    );
    *lock(&LOADED) = Some(transports::line(&data));
    let versions = root.map(read_versions).unwrap_or_default();
    let mut status = lock(&STATUS);
    let status = status.get_or_insert_with(Status::default);
    status.version = versions.kept.first().map_or_else(
        || format!("bundled ({})", short(BUNDLED_COMMIT)),
        |v| format!("{} ({})", short(&v.commit), v.date),
    );
    status.checked_at = versions.checked_at;
    status.can_roll_back = !versions.kept.is_empty();
    if let Some(message) = message {
        status.message = message;
    }
}

fn check(root: &Path, by_hand: bool) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("volcren-loader/", env!("VOLCREN_VERSION")))
        .build()
        .into();
    let message = match update(
        root,
        by_hand,
        now(),
        || latest(&agent),
        |commit, file| download(&agent, commit, file),
    ) {
        Ok(Checked::New(version)) => format!(
            "Updated to {}. Scripts started from now on use it.",
            short(&version.commit)
        ),
        Ok(Checked::UpToDate) => "Already up to date.".to_owned(),
        Ok(Checked::Skipped) => "The newest version is the one rolled back from.".to_owned(),
        Err(why) => {
            eprintln!("transports: update failed: {why}");
            format!("Update failed, nothing changed: {why}")
        }
    };
    reload(Some(root), Some(message));
}

/// Runs `job` on its own thread unless another one is running.
fn spawn_job(job: impl FnOnce() + Send + 'static) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        job();
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Reads the version in use, then checks upstream when a check is due, for as long as the loader runs. Call it once, when the loader starts.
pub fn start() {
    std::thread::spawn(|| {
        let root = root_dir();
        reload(root.as_deref(), None);
        let Some(root) = root else { return };
        loop {
            let due =
                now().saturating_sub(read_versions(&root).checked_at) >= CHECK_EVERY.as_secs();
            if due && !BUSY.swap(true, Ordering::SeqCst) {
                check(&root, false);
                BUSY.store(false, Ordering::SeqCst);
            }
            std::thread::sleep(LOOK_EVERY);
        }
    });
}

/// The Settings page's update button: checks now, in the background.
pub fn update_now() {
    spawn_job(|| {
        if let Some(root) = root_dir() {
            check(&root, true);
        }
    });
}

/// The Settings page's rollback button, in the background.
pub fn roll_back_now() {
    spawn_job(|| {
        let Some(root) = root_dir() else { return };
        let message = match roll_back(&root) {
            Ok(now) => format!("Rolled back to {now}. Scripts started from now on use it."),
            Err(why) => format!("Couldn't roll back: {why}"),
        };
        reload(Some(&root), Some(message));
    });
}

pub fn status() -> Status {
    let mut status = lock(&STATUS).clone().unwrap_or_default();
    status.busy = BUSY.load(Ordering::SeqCst);
    status
}

/// The line to give a starting script. Never waits on the network.
pub fn line() -> String {
    if let Some(line) = lock(&LOADED).as_ref() {
        return line.clone();
    }
    transports::line(&load_current(root_dir().as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("volcren-transports-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A small file that is valid for the file's kind.
    fn good(file: &File) -> String {
        match file.kind {
            Kind::Transports if file.keep.is_some() => {
                "Origin\tDestination\tmenuOption menuTarget objectID\n1 2 0\t3 4 0\tPay-toll(2-Ecto) Energy Barrier 16105\n1 2 0\t3 4 0\tOpen Door 9398\n1 2 0\t1 2 1\tClimb-up Ladder 1\n".to_owned()
            }
            Kind::Transports => format!("Destination\tDisplay info\n1 2 0\t{} spell\n", file.name),
            Kind::ItemNames => "#name\tids\nCOINS\t995\n".to_owned(),
        }
    }

    #[test]
    fn the_bundled_files_parse() {
        for file in FILES {
            assert!(validate(file, file.bundled).is_ok(), "{}", file.name);
            if file.kind == Kind::Transports {
                let (_, skipped) = transports::parse_file(file.name, file.bundled).unwrap();
                assert!(skipped.is_empty(), "{}: {skipped:?}", file.name);
            }
        }
    }

    fn version(n: u8) -> Version {
        Version {
            commit: format!("{n:x}").repeat(40),
            date: format!("2026-11-0{n}"),
        }
    }

    fn take(root: &Path, n: u8) -> Result<Checked, String> {
        update(
            root,
            false,
            100,
            || Ok(version(n)),
            |_, file| Ok(good(file)),
        )
    }

    fn commits(root: &Path) -> Vec<String> {
        read_versions(root)
            .kept
            .into_iter()
            .map(|v| v.commit)
            .collect()
    }

    #[test]
    fn a_good_version_is_used_and_kept() {
        let root = temp("good");
        assert_eq!(take(&root, 1), Ok(Checked::New(version(1))));
        let data = load_current(Some(&root));
        assert_eq!(data.source, "1111111");
        let levers = |t: &&Transport| t.kind == "teleportation_levers";
        assert_eq!(
            data.transports.iter().filter(levers).count(),
            8,
            "always the bundled ones"
        );
        assert_eq!(data.items.len(), 1);
        assert!(
            root.join("1".repeat(40))
                .join("teleportation_spells.tsv")
                .exists()
        );
        assert_eq!(read_versions(&root).checked_at, 100);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn one_bad_file_keeps_the_version_in_use() {
        let root = temp("bad");
        take(&root, 1).unwrap();
        let result = update(
            &root,
            false,
            200,
            || Ok(version(2)),
            |_, file| {
                if file.name == "boats" {
                    Ok("<html>rate limited</html>".into())
                } else {
                    Ok(good(file))
                }
            },
        );
        assert!(result.is_err_and(|why| why.starts_with("boats")));
        assert_eq!(commits(&root), [version(1).commit]);
        assert!(
            !root.join(version(2).commit).exists(),
            "nothing of it is written"
        );
        assert_eq!(
            read_versions(&root).checked_at,
            100,
            "and it is tried again soon"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn up_to_date_downloads_nothing() {
        let root = temp("same");
        take(&root, 1).unwrap();
        let checked = update(
            &root,
            false,
            300,
            || Ok(version(1)),
            |_, _| panic!("no download"),
        );
        assert_eq!(checked, Ok(Checked::UpToDate));
        assert_eq!(read_versions(&root).checked_at, 300);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn three_versions_are_kept_and_older_folders_go() {
        let root = temp("three");
        std::fs::create_dir_all(root.join(BUNDLED_COMMIT)).unwrap();
        std::fs::create_dir_all(root.join("mine")).unwrap();
        for n in 1..=4 {
            take(&root, n).unwrap();
        }
        assert_eq!(
            commits(&root),
            [version(4).commit, version(3).commit, version(2).commit]
        );
        assert!(!root.join(version(1).commit).exists());
        assert!(
            !root.join(BUNDLED_COMMIT).exists(),
            "the old layout's folder"
        );
        assert!(
            root.join("mine").exists(),
            "not named like a commit, so not ours"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rolling_back_goes_to_the_one_before_and_the_daily_check_leaves_it_alone() {
        let root = temp("back");
        take(&root, 1).unwrap();
        take(&root, 2).unwrap();
        assert_eq!(roll_back(&root), Ok("1111111".to_owned()));
        assert_eq!(load_current(Some(&root)).source, "1111111");
        assert_eq!(take(&root, 2), Ok(Checked::Skipped));
        assert_eq!(load_current(Some(&root)).source, "1111111");
        assert_eq!(
            take(&root, 3),
            Ok(Checked::New(version(3))),
            "a newer one is taken"
        );
        assert_eq!(roll_back(&root), Ok("1111111".to_owned()));
        let by_hand = update(&root, true, 1, || Ok(version(3)), |_, file| Ok(good(file)));
        assert_eq!(
            by_hand,
            Ok(Checked::New(version(3))),
            "the button takes it again"
        );
        assert_eq!(roll_back(&root), Ok("1111111".to_owned()));
        assert_eq!(roll_back(&root), Ok("the bundled data".to_owned()));
        assert_eq!(load_current(Some(&root)).source, "bundled");
        assert!(roll_back(&root).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_damaged_file_falls_back_to_the_bundled_copy() {
        let root = temp("damaged");
        take(&root, 1).unwrap();
        std::fs::write(root.join(version(1).commit).join("boats.tsv"), "junk").unwrap();
        let data = load_current(Some(&root));
        assert_eq!(data.source, "1111111, part bundled");
        assert!(data.transports.iter().any(|t| t.kind == "boats"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_answer_from_upstream_is_not_trusted() {
        let good = r#"[{"sha":"4fd7d4a220c06781145dd7ba21e9a1e46ed0fc27","commit":{"committer":{"date":"2026-11-03T07:19:00Z"}}}]"#;
        assert_eq!(
            parse_latest(good),
            Ok(Version {
                commit: BUNDLED_COMMIT.to_owned(),
                date: "2026-11-03".to_owned()
            })
        );
        assert!(parse_latest(r#"[{"sha":"../../../etc"}]"#).is_err());
        assert!(parse_latest("[]").is_err());
        assert!(parse_latest("<html>").is_err());
    }

    #[test]
    #[ignore = "reaches upstream"]
    fn takes_the_real_newest_version() {
        let root = temp("real");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .user_agent("volcren-loader/test")
            .build()
            .into();
        let checked = update(
            &root,
            true,
            1,
            || latest(&agent),
            |c, f| download(&agent, c, f),
        );
        println!("{checked:?}");
        let data = load_current(Some(&root));
        println!(
            "{} rows, {} climbs, {} items ({})",
            data.transports.len(),
            data.climbs.len(),
            data.items.len(),
            data.source
        );
        assert!(matches!(checked, Ok(Checked::New(_))), "{checked:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn with_no_version_everything_is_bundled() {
        let data = load(None);
        assert_eq!(data.source, "bundled");
        assert!(data.transports.iter().any(|t| t.name == "Varrock Teleport"));
        assert!(data.transports.iter().any(|t| t.kind == "charter_ships"));
        // boats, ships and carts: whole trips from a tile to a tile, and the minecart file also has the Lovakengj network (separate starts and ends)
        let trips = |kind: &str| {
            data.transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_some() && t.destination.is_some())
                .count()
        };
        assert!(trips("boats") > 100 && trips("ships") > 30 && trips("minecarts") > 10);
        assert_eq!(trips("magic_carpets"), 13);
        assert_eq!(trips("canoes"), 45);
        assert_eq!(trips("teleportation_portals"), 145);
        assert_eq!(trips("teleportation_boxes"), 40);
        assert_eq!(
            data.transports
                .iter()
                .filter(|t| t.kind == "teleportation_minigames")
                .count(),
            253
        );
        let whistle: Vec<_> = data
            .transports
            .iter()
            .filter(|t| t.kind == "quetzal_whistle")
            .collect();
        assert_eq!(whistle.len(), 14);
        assert!(
            whistle
                .iter()
                .all(|t| t.origin.is_none() && t.destination.is_some() && t.consumable)
        );
        // the networks with separate starts and ends: sent as they are, the chooser pairs them
        for kind in [
            "quetzals",
            "spirit_trees",
            "gnome_gliders",
            "hot_air_balloons",
            "fairy_rings",
            "magic_mushtrees",
        ] {
            let starts = data
                .transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_some() && t.destination.is_none())
                .count();
            let ends = data
                .transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_none() && t.destination.is_some())
                .count();
            assert!(
                starts > 5 && ends > 3,
                "{kind}: {starts} starts, {ends} ends"
            );
        }
        let trips: Vec<_> = data
            .transports
            .iter()
            .filter(|t| t.kind == "transports")
            .collect();
        let barriers = trips
            .iter()
            .filter(|t| t.menu.starts_with("Pay-toll"))
            .count();
        assert!(barriers >= 12, "the barriers are kept");
        assert!(
            trips.iter().any(|t| t.menu == "Board Travel cart 2230"),
            "and the far hops"
        );
        assert!(
            trips.iter().all(|t| t.menu.starts_with("Pay-toll")
                || transports::is_far_hop(t)
                || transports::is_item_on(t)),
            "and nothing else of that file"
        );
        // the planner's climbs: stairs, ladders, ropes and cave mouths, with exact planes (the rope near 1559,3046 goes from plane 0 to plane 2)
        assert!(data.climbs.len() > 3000, "{} climbs", data.climbs.len());
        let plain = |t: &&Transport| t.kind != transports::SHORTCUT_KIND;
        assert!(data.climbs.iter().filter(plain).all(transports::is_climb));
        // the Tree Door of the Grand Tree: a solid object that carries you two tiles across, no plane change
        let door = data
            .climbs
            .iter()
            .find(|t| {
                t.menu == "Open Tree Door 1968"
                    && t.origin.is_some_and(|o| (o.x, o.y) == (2466, 3491))
            })
            .expect("the Tree Door");
        assert!(transports::is_link(door) && !transports::gated(door));
        // a toll barrier is the script's, and a wall door (a one tile hop) the planner's own
        assert!(data.climbs.iter().all(|t| !t.menu.starts_with("Pay-toll")));
        let rope = data
            .climbs
            .iter()
            .find(|t| t.menu == "Climb-up Rope 51647")
            .expect("the rope up");
        assert_eq!(
            (
                rope.origin.map(|t| t.plane),
                rope.destination.map(|t| t.plane)
            ),
            (Some(0), Some(2))
        );
        assert!(
            data.climbs
                .iter()
                .any(|t| t.origin.is_some_and(|o| o.y < 6400)
                    && t.destination.is_some_and(|d| d.y >= 6400)),
            "into the underground"
        );
        let gated = data
            .transports
            .iter()
            .filter(|t| t.kind == transports::GATED_KIND || t.kind == transports::LINK_GATED_KIND)
            .count();
        assert_eq!(
            gated,
            data.climbs
                .iter()
                .filter(plain)
                .filter(|t| transports::gated(t))
                .count()
        );
        // the agility shortcuts: hops that stay in one world and are short, and all of them ask for a level
        let shortcuts: Vec<_> = data
            .climbs
            .iter()
            .filter(|t| t.kind == transports::SHORTCUT_KIND)
            .collect();
        assert!(shortcuts.len() > 500, "{} shortcuts", shortcuts.len());
        assert!(shortcuts.iter().all(|t| transports::is_shortcut(t)));
        let pipe = shortcuts
            .iter()
            .find(|t| {
                t.menu == "Squeeze-through Obstacle pipe 16511"
                    && t.origin
                        .is_some_and(|o| (o.x, o.y, o.plane) == (3149, 9906, 0))
            })
            .expect("the Varrock sewer pipe, west to east");
        assert!(
            pipe.destination
                .is_some_and(|d| (d.x, d.y, d.plane) == (3155, 9906, 0))
        );
        assert!(
            pipe.skills
                .iter()
                .any(|s| s.name == "Agility" && s.level == 51)
        );
        let sent = data
            .transports
            .iter()
            .filter(|t| t.kind == transports::SHORTCUT_GATED_KIND)
            .count();
        assert_eq!(
            sent,
            shortcuts.iter().filter(|t| transports::gated(t)).count()
        );
        assert!(
            data.transports
                .iter()
                .all(|t| t.kind != transports::SHORTCUT_KIND || transports::is_item_on(t)),
            "scripts get only the gated copies, and the items used on an object"
        );
        assert!(gated > 0 && gated < 1000, "{gated} gated climbs");
        assert!(
            data.transports
                .iter()
                .filter(|t| t.kind == transports::LINK_GATED_KIND)
                .all(|t| transports::is_link(t) || transports::is_gated_door(t)),
            "the links and doors the script checks"
        );
        assert!(
            data.items
                .iter()
                .any(|i| i.name == "COINS" && i.ids == [995])
        );
    }

    #[test]
    fn an_empty_answer_does_not_replace_anything() {
        let root = temp("empty");
        assert!(update(&root, false, 1, || Ok(version(1)), |_, _| Ok(String::new())).is_err());
        assert_eq!(load_current(Some(&root)).source, "bundled");
        let _ = std::fs::remove_dir_all(root);
    }
}
