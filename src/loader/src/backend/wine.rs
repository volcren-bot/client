use std::{
    collections::HashMap,
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, OnceLock, mpsc::Receiver},
    thread,
    time::Duration,
};

use injector::{
    Client, Connection, Cursor, DisplayName, GameState, Item, LoadReport, Overlay, PacketPage,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{Scan, Settings, Target};

const CLIENT_EXES: &[&str] = &["osclient.exe", "osrswindows.exe"];
const HELPER_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Lane {
    Main,
    Frames,
    Events,
}

type Registry = HashMap<(PathBuf, Lane), Arc<Mutex<Server>>>;

static SERVERS: OnceLock<Mutex<Registry>> = OnceLock::new();

struct Server {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Server {
    fn spawn(location: &Location, helper: &Path) -> Result<Self, String> {
        let mut child = wine_command(location, helper)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't run {}: {e}", location.wine.display()))?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");

        let (tx, lines) = std::sync::mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            lines,
        })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn send(&mut self, args: &[&str]) -> Result<String, String> {
        let mut line = serde_json::to_string(args).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("injector.exe: {e}"))?;
        self.lines
            .recv_timeout(HELPER_TIMEOUT)
            .map_err(|_| "injector.exe timed out or exited.".to_string())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wine_command(location: &Location, helper: &Path) -> Command {
    let mut command = Command::new(&location.wine);
    for (key, _) in env::vars().filter(|(k, _)| k.starts_with("WINE")) {
        command.env_remove(key);
    }
    command
        .arg(helper)
        .envs(location.env.iter().cloned())
        .env("WINEPREFIX", &location.prefix)
        .env("WINEDEBUG", "-all");
    command
}

fn ensure_connection(
    lane: Lane,
    location: &Location,
    helper: &Path,
) -> Result<Arc<Mutex<Server>>, String> {
    let registry = SERVERS.get_or_init(Default::default);
    let lock_registry = || {
        registry
            .lock()
            .map_err(|_| "injector connection registry lock poisoned.".to_string())
    };
    let key = (location.prefix.clone(), lane);
    let existing = lock_registry()?.get(&key).cloned();
    if let Some(server) = &existing
        && server.lock().is_ok_and(|mut server| server.alive())
    {
        return Ok(server.clone());
    }
    let mut registry = lock_registry()?;
    if let Some(current) = registry.get(&key)
        && existing
            .as_ref()
            .is_none_or(|dead| !Arc::ptr_eq(dead, current))
    {
        return Ok(current.clone());
    }
    let server = Arc::new(Mutex::new(Server::spawn(location, helper)?));
    registry.insert(key, server.clone());
    Ok(server)
}

fn disconnect(lane: Lane, location: &Location) {
    if let Some(registry) = SERVERS.get()
        && let Ok(mut registry) = registry.lock()
    {
        registry.remove(&(location.prefix.clone(), lane));
    }
}

pub fn shutdown() {
    if let Some(registry) = SERVERS.get()
        && let Ok(mut registry) = registry.lock()
    {
        registry.clear();
    }
}

fn send(lane: Lane, location: &Location, helper: &Path, args: &[&str]) -> Result<String, String> {
    let server = ensure_connection(lane, location, helper)?;
    let mut server = server
        .lock()
        .map_err(|_| "injector connection lock poisoned.".to_string())?;
    let result = server.send(args);
    if result.is_err() {
        drop(server);
        disconnect(lane, location);
    }
    result
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Location {
    prefix: PathBuf,
    wine: PathBuf,
    env: Vec<(String, String)>,
}

impl Location {
    pub fn label(&self) -> Option<String> {
        Some(self.prefix.display().to_string())
    }
}

pub fn scan(settings: &Settings) -> Scan {
    let mut scan = Scan::default();
    let locations = find_locations();
    if locations.is_empty() {
        return scan;
    }

    let helper = match helper(settings) {
        Ok(helper) => helper,
        Err(error) => {
            scan.errors.push(error);
            return scan;
        }
    };

    for location in locations {
        match run_on::<Vec<Client>>(Lane::Main, &location, &helper, &["list"]) {
            Ok(clients) => scan
                .targets
                .extend(clients.into_iter().map(|client| Target {
                    client,
                    location: location.clone(),
                })),
            Err(error) => scan
                .errors
                .push(format!("{}: {error}", location.prefix.display())),
        }
    }
    scan
}

pub fn load(target: &Target, settings: &Settings, cursor: &Cursor) -> Result<LoadReport, String> {
    let helper = helper(settings)?;
    let dll = windows_path(&target.location.prefix, Path::new(&settings.dll))?;
    let pid = target.client.pid.to_string();
    let cursor = cursor.args(target.client.pid);
    let mut args = vec!["load", pid.as_str(), dll.as_str()];
    args.extend(cursor[2..].iter().map(String::as_str));
    run_on(Lane::Main, &target.location, &helper, &args)
}

pub fn unload(target: &Target, settings: &Settings) -> Result<(), String> {
    call(target, settings, "unload", &[])
}

pub fn console(target: &Target, settings: &Settings) -> Result<(), String> {
    call(target, settings, "console", &[])
}

pub fn draw(target: &Target, settings: &Settings, overlay: &Overlay) -> Result<(), String> {
    run_args(target, settings, &overlay.args(target.client.pid))
}

pub fn set_cursor(target: &Target, settings: &Settings, cursor: &Cursor) -> Result<(), String> {
    run_args(target, settings, &cursor.args(target.client.pid))
}

pub fn connection(target: &Target, settings: &Settings) -> Result<Option<Connection>, String> {
    call(target, settings, "status", &[])
}

pub fn state(target: &Target, settings: &Settings) -> Result<GameState, String> {
    call(target, settings, "state", &[])
}

pub fn set_display_name(
    target: &Target,
    settings: &Settings,
    name: &str,
) -> Result<DisplayName, String> {
    call(target, settings, "name", &[name])
}

pub fn varps(target: &Target, settings: &Settings) -> Result<Option<Vec<i32>>, String> {
    call(target, settings, "varps", &[])
}

pub fn varbit(target: &Target, settings: &Settings, id: i32) -> Result<Option<i32>, String> {
    call(target, settings, "varbit", &[&id.to_string()])
}

pub fn enum_def(
    target: &Target,
    settings: &Settings,
    id: i32,
) -> Result<Option<injector::config::EnumDef>, String> {
    call(target, settings, "enum", &[&id.to_string()])
}

pub fn varp(target: &Target, settings: &Settings, id: usize) -> Result<Option<i32>, String> {
    call(target, settings, "varp", &[&id.to_string()])
}

pub fn varbits_of(
    target: &Target,
    settings: &Settings,
    varp: u16,
) -> Result<Vec<(i32, injector::config::VarbitDef, Option<String>)>, String> {
    call(target, settings, "varbits-of", &[&varp.to_string()])
}

pub fn item_container(
    target: &Target,
    settings: &Settings,
    id: i32,
) -> Result<Option<Vec<Item>>, String> {
    call(target, settings, "container", &[&id.to_string()])
}

pub fn item_containers(
    target: &Target,
    settings: &Settings,
) -> Result<Vec<(i32, Vec<Item>)>, String> {
    call(target, settings, "containers", &[])
}

pub fn widget_tree(target: &Target, settings: &Settings) -> Result<injector::WidgetTree, String> {
    call(target, settings, "widget-tree", &[])
}

pub fn game_val_names(
    target: &Target,
    settings: &Settings,
    kind: u32,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    call_ids(target, settings, "game-vals", &[&kind.to_string()], ids)
}

pub fn item_families(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<injector::config::ItemFamily>, String> {
    call_ids(target, settings, "item-families", &[], ids)
}

pub fn item_stackable(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<bool>>, String> {
    call_ids(target, settings, "item-stackable", &[], ids)
}

pub fn item_icons(
    target: &Target,
    settings: &Settings,
    icons: &[injector::ItemIconRequest],
) -> Result<Vec<Option<injector::Image>>, String> {
    let icons: Vec<String> = icons
        .iter()
        .map(|icon| {
            format!(
                "{}:{}:{}",
                icon.id,
                icon.quantity,
                u8::from(icon.show_count)
            )
        })
        .collect();
    call(target, settings, "item-icons", &strs(&icons))
}

pub fn sprites(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<injector::Image>>, String> {
    call_ids(target, settings, "sprites", &[], ids)
}

pub fn item_changes(
    target: &Target,
    settings: &Settings,
    since: u64,
    wait: std::time::Duration,
) -> Result<injector::ItemPage, String> {
    let (since, wait) = (since.to_string(), wait.as_millis().to_string());
    call_on(
        Lane::Events,
        target,
        settings,
        "item-changes",
        &[&since, &wait],
    )
}

pub fn item_params(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<std::collections::BTreeMap<u32, injector::config::Value>>>, String> {
    call_ids(target, settings, "item-params", &[], ids)
}

pub fn item_names(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    call_ids(target, settings, "item-names", &[], ids)
}

pub fn scene_tiles_next_frame(
    target: &Target,
    settings: &Settings,
    layers: &[&str],
    after: u32,
) -> Result<Option<u32>, String> {
    let after = after.to_string();
    let mut rest = vec![after.as_str()];
    rest.extend_from_slice(layers);
    call_on(Lane::Frames, target, settings, "tiles-next", &rest)
}

pub fn packets(target: &Target, settings: &Settings, since: u64) -> Result<PacketPage, String> {
    call(target, settings, "packets", &[&since.to_string()])
}

pub fn packet_layouts(
    target: &Target,
    settings: &Settings,
) -> Result<Option<Vec<injector::PacketLayout>>, String> {
    call(target, settings, "packet-layouts", &[])
}

pub fn script_runs(
    target: &Target,
    settings: &Settings,
    since: u64,
) -> Result<injector::ScriptPage, String> {
    call(target, settings, "scripts", &[&since.to_string()])
}

pub fn varc_changes(
    target: &Target,
    settings: &Settings,
    since: u64,
) -> Result<injector::VarcPage, String> {
    call(target, settings, "varc-changes", &[&since.to_string()])
}

pub fn script_command(
    target: Option<&Target>,
    settings: &Settings,
    script: Option<&Path>,
    lib: Option<&Path>,
) -> Result<Command, String> {
    let Some(target) = target else {
        let mut command = Command::new(native_helper()?);
        super::run_args(&mut command, script, lib);
        return Ok(command);
    };
    let mut command = wine_command(&target.location, &helper(settings)?);
    let prefix = &target.location.prefix;
    let script = script.and_then(|s| windows_path(prefix, s).ok());
    let lib = lib.and_then(|l| windows_path(prefix, l).ok());
    super::run_args(&mut command, script.as_deref(), lib.as_deref());
    command.arg(target.client.pid.to_string());
    Ok(command)
}

fn native_helper() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("Can't find the loader's own path: {e}"))?;
    [exe.parent(), exe.parent().and_then(Path::parent)]
        .into_iter()
        .flatten()
        .map(|dir| dir.join("injector"))
        .find(|path| path.is_file())
        .ok_or_else(|| "injector not found next to the loader, select a client or build it (cargo build -p injector).".into())
}

fn helper(settings: &Settings) -> Result<PathBuf, String> {
    let helper = PathBuf::from(&settings.helper);
    if !helper.is_file() {
        return Err(
            "Set the path to injector.exe, it's needed to reach clients running under Wine.".into(),
        );
    }
    Ok(helper)
}

fn find_locations() -> Vec<Location> {
    let mut locations: Vec<Location> = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return locations;
    };

    for entry in entries.flatten() {
        let proc = entry.path();
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }

        let Ok(cmdline) = fs::read(proc.join("cmdline")) else {
            continue;
        };
        let argv0 = String::from_utf8_lossy(cmdline.split(|&b| b == 0).next().unwrap_or_default());
        let exe = argv0
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !CLIENT_EXES.contains(&exe.as_str()) {
            continue;
        }

        let Ok(environ) = fs::read(proc.join("environ")) else {
            continue;
        };
        let vars: HashMap<String, String> = environ
            .split(|&b| b == 0)
            .filter_map(|var| {
                String::from_utf8_lossy(var)
                    .split_once('=')
                    .map(|(k, v)| (k.into(), v.into()))
            })
            .collect();

        let prefix = match vars.get("WINEPREFIX") {
            Some(prefix) => PathBuf::from(prefix),
            None => PathBuf::from(vars.get("HOME").cloned().unwrap_or_default()).join(".wine"),
        };
        if locations.iter().any(|l| l.prefix == prefix) {
            continue;
        }

        let wine = vars
            .get("WINE")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| {
                let exe = fs::read_link(proc.join("exe")).ok()?;
                exe.ancestors().skip(1).take(5).find_map(|dir| {
                    ["wine", "wine64", "bin/wine", "bin/wine64"]
                        .iter()
                        .map(|name| dir.join(name))
                        .find(|p| p.is_file())
                })
            })
            .unwrap_or_else(|| PathBuf::from("wine"));

        let env = vars
            .into_iter()
            .filter(|(k, _)| {
                k.starts_with("WINE")
                    && ![
                        "WINELOADERNOEXEC",
                        "WINEDEBUG",
                        "WINESERVERSOCKET",
                        "WINEPRELOADRESERVE",
                    ]
                    .contains(&k.as_str())
            })
            .collect();

        locations.push(Location { prefix, wine, env });
    }
    locations
}

fn windows_path(prefix: &Path, path: &Path) -> Result<String, String> {
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) => match (path.parent().map(Path::canonicalize), path.file_name()) {
            (Some(Ok(dir)), Some(file)) => dir.join(file),
            _ => return Err(format!("{}: {error}", path.display())),
        },
    };
    let dosdevices = prefix.join("dosdevices");

    let mut best: Option<(char, PathBuf)> = None;
    for entry in fs::read_dir(&dosdevices)
        .map_err(|e| format!("{}: {e}", dosdevices.display()))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let mut chars = name.chars();
        let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) else {
            continue;
        };
        let Ok(root) = entry.path().canonicalize() else {
            continue;
        };
        if path.starts_with(&root)
            && best
                .as_ref()
                .is_none_or(|(_, b)| root.components().count() > b.components().count())
        {
            best = Some((letter.to_ascii_uppercase(), root));
        }
    }

    let (letter, root) = best.ok_or_else(|| {
        format!(
            "{} isn't reachable from any drive in {}.",
            path.display(),
            prefix.display()
        )
    })?;
    let rest = path
        .strip_prefix(&root)
        .unwrap()
        .to_string_lossy()
        .replace('/', "\\");
    Ok(format!("{letter}:\\{rest}"))
}

fn call<T: DeserializeOwned>(
    target: &Target,
    settings: &Settings,
    verb: &str,
    rest: &[&str],
) -> Result<T, String> {
    call_on(Lane::Main, target, settings, verb, rest)
}

fn call_on<T: DeserializeOwned>(
    lane: Lane,
    target: &Target,
    settings: &Settings,
    verb: &str,
    rest: &[&str],
) -> Result<T, String> {
    let helper = helper(settings)?;
    let pid = target.client.pid.to_string();
    let mut args = vec![verb, pid.as_str()];
    args.extend_from_slice(rest);
    run_on(lane, &target.location, &helper, &args)
}

fn call_ids<T: DeserializeOwned>(
    target: &Target,
    settings: &Settings,
    verb: &str,
    head: &[&str],
    ids: &[i32],
) -> Result<T, String> {
    let ids: Vec<String> = ids.iter().map(i32::to_string).collect();
    let mut rest = head.to_vec();
    rest.extend(ids.iter().map(String::as_str));
    call(target, settings, verb, &rest)
}

fn run_args<T: DeserializeOwned>(
    target: &Target,
    settings: &Settings,
    args: &[String],
) -> Result<T, String> {
    run_on(
        Lane::Main,
        &target.location,
        &helper(settings)?,
        &strs(args),
    )
}

fn strs(strings: &[String]) -> Vec<&str> {
    strings.iter().map(String::as_str).collect()
}

fn run_on<T: DeserializeOwned>(
    lane: Lane,
    location: &Location,
    helper: &Path,
    args: &[&str],
) -> Result<T, String> {
    let line = send(lane, location, helper, args)?;
    serde_json::from_str::<Result<T, String>>(&line)
        .map_err(|e| format!("Unexpected output from injector.exe ({e}): {line}"))?
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn windows_path_uses_the_deepest_drive() {
        let root = env::temp_dir().join(format!("loader-test-{}", std::process::id()));
        let dosdevices = root.join("prefix/dosdevices");
        fs::create_dir_all(&dosdevices).unwrap();
        fs::create_dir_all(root.join("prefix/drive_c/Games")).unwrap();
        fs::write(root.join("prefix/drive_c/Games/a.dll"), "").unwrap();
        fs::write(root.join("b.dll"), "").unwrap();
        symlink("../drive_c", dosdevices.join("c:")).unwrap();
        symlink("/", dosdevices.join("z:")).unwrap();
        symlink("/dev/ttyS0", dosdevices.join("com1")).unwrap();

        let prefix = root.join("prefix");
        assert_eq!(
            windows_path(&prefix, &root.join("prefix/drive_c/Games/a.dll")).unwrap(),
            "C:\\Games\\a.dll"
        );
        let b = root.join("b.dll").canonicalize().unwrap();
        assert_eq!(
            windows_path(&prefix, &b).unwrap(),
            format!("Z:{}", b.display()).replace('/', "\\")
        );

        fs::remove_dir_all(root).unwrap();
    }
}
