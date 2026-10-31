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
    Client, Connection, Cursor, DisplayName, GameState, Input, Item, LoadReport, Npc, Overlay,
    PacketPage, Player, Widget,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{Scan, Settings, Target};

const CLIENT_EXES: &[&str] = &["osclient.exe", "osrswindows.exe"];
const HELPER_TIMEOUT: Duration = Duration::from_secs(45);

/// Which of a prefix's helpers a request goes to; each kind of wait gets its own lane so others don't queue behind it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Lane {
    Main,
    Frames,
    /// Requests that wait for the client to log something (item changes).
    Events,
}

static SERVERS: OnceLock<Mutex<HashMap<(PathBuf, Lane), Arc<Mutex<Server>>>>> = OnceLock::new();

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
    let registry = SERVERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry
        .lock()
        .map_err(|_| "injector connection registry lock poisoned.".to_string())?;
    let key = (location.prefix.clone(), lane);
    let alive = registry.get(&key).is_some_and(|server| {
        server
            .lock()
            .map(|mut server| server.alive())
            .unwrap_or(false)
    });
    if !alive {
        registry.insert(
            key.clone(),
            Arc::new(Mutex::new(Server::spawn(location, helper)?)),
        );
    }
    Ok(registry.get(&key).expect("just ensured").clone())
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
        match run::<Vec<Client>>(&location, &helper, &["list"]) {
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
    // `cursor <pid> <style> <r> <g> <b> <size>`: the same arguments follow the dll here.
    let cursor = cursor.args(target.client.pid);
    let mut args = vec!["load", pid.as_str(), dll.as_str()];
    args.extend(cursor[2..].iter().map(String::as_str));
    run(&target.location, &helper, &args)
}

pub fn unload(target: &Target, settings: &Settings) -> Result<(), String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["unload", &target.client.pid.to_string()],
    )
}

pub fn console(target: &Target, settings: &Settings) -> Result<(), String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["console", &target.client.pid.to_string()],
    )
}

pub fn input(target: &Target, settings: &Settings, input: &Input) -> Result<(), String> {
    let helper = helper(settings)?;
    let args = input.args(target.client.pid);
    run(
        &target.location,
        &helper,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

pub fn draw(target: &Target, settings: &Settings, overlay: &Overlay) -> Result<(), String> {
    let helper = helper(settings)?;
    let args = overlay.args(target.client.pid);
    run(
        &target.location,
        &helper,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

pub fn set_cursor(target: &Target, settings: &Settings, cursor: &Cursor) -> Result<(), String> {
    let helper = helper(settings)?;
    let args = cursor.args(target.client.pid);
    run(
        &target.location,
        &helper,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

pub fn connection(target: &Target, settings: &Settings) -> Result<Option<Connection>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["status", &target.client.pid.to_string()],
    )
}

pub fn state(target: &Target, settings: &Settings) -> Result<GameState, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["state", &target.client.pid.to_string()],
    )
}

pub fn set_display_name(
    target: &Target,
    settings: &Settings,
    name: &str,
) -> Result<DisplayName, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["name", &target.client.pid.to_string(), name],
    )
}

pub fn varps(target: &Target, settings: &Settings) -> Result<Option<Vec<i32>>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["varps", &target.client.pid.to_string()],
    )
}

pub fn varp(target: &Target, settings: &Settings, id: usize) -> Result<Option<i32>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["varp", &target.client.pid.to_string(), &id.to_string()],
    )
}

pub fn varbits_of(
    target: &Target,
    settings: &Settings,
    varp: u16,
) -> Result<Vec<(i32, injector::config::VarbitDef, Option<String>)>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &[
            "varbits-of",
            &target.client.pid.to_string(),
            &varp.to_string(),
        ],
    )
}

pub fn widget(
    target: &Target,
    settings: &Settings,
    group: u16,
    child: u16,
) -> Result<Option<Widget>, String> {
    let helper = helper(settings)?;
    let (pid, group, child) = (
        target.client.pid.to_string(),
        group.to_string(),
        child.to_string(),
    );
    run(&target.location, &helper, &["widget", &pid, &group, &child])
}

pub fn item_container(
    target: &Target,
    settings: &Settings,
    id: i32,
) -> Result<Option<Vec<Item>>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["container", &target.client.pid.to_string(), &id.to_string()],
    )
}

pub fn item_containers(
    target: &Target,
    settings: &Settings,
) -> Result<Vec<(i32, Vec<Item>)>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["containers", &target.client.pid.to_string()],
    )
}

pub fn widget_tree(target: &Target, settings: &Settings) -> Result<injector::WidgetTree, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["widget-tree", &target.client.pid.to_string()],
    )
}

pub fn game_val_names(
    target: &Target,
    settings: &Settings,
    kind: u32,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    let helper = helper(settings)?;
    let (pid, kind) = (target.client.pid.to_string(), kind.to_string());
    let mut args = vec!["game-vals", pid.as_str(), kind.as_str()];
    let ids: Vec<String> = ids.iter().map(i32::to_string).collect();
    args.extend(ids.iter().map(String::as_str));
    run(&target.location, &helper, &args)
}

pub fn item_stackable(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<bool>>, String> {
    let helper = helper(settings)?;
    let pid = target.client.pid.to_string();
    let mut args = vec!["item-stackable", pid.as_str()];
    let ids: Vec<String> = ids.iter().map(i32::to_string).collect();
    args.extend(ids.iter().map(String::as_str));
    run(&target.location, &helper, &args)
}

/// With nothing new it blocks up to `wait`, so it goes to its own helper.
pub fn item_changes(
    target: &Target,
    settings: &Settings,
    since: u64,
    wait: std::time::Duration,
) -> Result<injector::ItemPage, String> {
    let helper = helper(settings)?;
    let (pid, since, wait) = (
        target.client.pid.to_string(),
        since.to_string(),
        wait.as_millis().to_string(),
    );
    run_on(
        Lane::Events,
        &target.location,
        &helper,
        &["item-changes", &pid, &since, &wait],
    )
}

pub fn item_names(
    target: &Target,
    settings: &Settings,
    ids: &[i32],
) -> Result<Vec<Option<String>>, String> {
    let helper = helper(settings)?;
    let pid = target.client.pid.to_string();
    let mut args = vec!["item-names", pid.as_str()];
    let ids: Vec<String> = ids.iter().map(i32::to_string).collect();
    args.extend(ids.iter().map(String::as_str));
    run(&target.location, &helper, &args)
}

pub fn varc(target: &Target, settings: &Settings, id: i32) -> Result<Option<i32>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["varc", &target.client.pid.to_string(), &id.to_string()],
    )
}

pub fn scene_tiles_next_frame(
    target: &Target,
    settings: &Settings,
    layers: &[&str],
    after: u32,
) -> Result<Option<u32>, String> {
    let helper = helper(settings)?;
    let (pid, after) = (target.client.pid.to_string(), after.to_string());
    let mut args = vec!["tiles-next", &pid, &after];
    args.extend_from_slice(layers);
    run_on(Lane::Frames, &target.location, &helper, &args)
}

pub fn npcs(target: &Target, settings: &Settings) -> Result<Vec<Npc>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["npcs", &target.client.pid.to_string()],
    )
}

pub fn players(target: &Target, settings: &Settings) -> Result<Vec<Player>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["players", &target.client.pid.to_string()],
    )
}

pub fn packets(target: &Target, settings: &Settings, since: u64) -> Result<PacketPage, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &[
            "packets",
            &target.client.pid.to_string(),
            &since.to_string(),
        ],
    )
}

pub fn packet_layouts(
    target: &Target,
    settings: &Settings,
) -> Result<Option<Vec<injector::PacketLayout>>, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &["packet-layouts", &target.client.pid.to_string()],
    )
}

pub fn script_runs(
    target: &Target,
    settings: &Settings,
    since: u64,
) -> Result<injector::ScriptPage, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &[
            "scripts",
            &target.client.pid.to_string(),
            &since.to_string(),
        ],
    )
}

pub fn varc_changes(
    target: &Target,
    settings: &Settings,
    since: u64,
) -> Result<injector::VarcPage, String> {
    let helper = helper(settings)?;
    run(
        &target.location,
        &helper,
        &[
            "varc-changes",
            &target.client.pid.to_string(),
            &since.to_string(),
        ],
    )
}

/// `lib` is only for scripts run from a local file: store scripts get the downloaded lib beside them.
pub fn script_command(
    target: Option<&Target>,
    settings: &Settings,
    script: Option<&Path>,
    lib: Option<&Path>,
) -> Result<Command, String> {
    let Some(target) = target else {
        let mut command = Command::new(native_helper()?);
        command.arg("run");
        if let Some(script) = script {
            command.arg("--script").arg(script);
            if let Some(lib) = lib {
                command.arg("--lib").arg(lib);
            }
        }
        return Ok(command);
    };
    let mut command = wine_command(&target.location, &helper(settings)?);
    command.arg("run");
    // Under Wine the injector needs the script's Windows path; a script outside every drive just can't require from disk.
    if let Some(path) = script.and_then(|s| windows_path(&target.location.prefix, s).ok()) {
        command.args(["--script", &path]);
        if let Some(lib) = lib.and_then(|l| windows_path(&target.location.prefix, l).ok()) {
            command.args(["--lib", &lib]);
        }
    }
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
    if settings.helper.is_empty() || !helper.is_file() {
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

        // Bundled Wines (AppImages) keep the launcher in `bin/` while the process runs from `lib/wine/<arch>-unix/`.
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
                    // Per-process state of the running client (an inherited socket fd, the preloader's address range).
                    && !["WINELOADERNOEXEC", "WINEDEBUG", "WINESERVERSOCKET", "WINEPRELOADRESERVE"].contains(&k.as_str())
            })
            .collect();

        locations.push(Location { prefix, wine, env });
    }
    locations
}

fn windows_path(prefix: &Path, path: &Path) -> Result<String, String> {
    // A premium script's entry file doesn't exist (it only lives in memory), so its folder stands in for it.
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

fn run<T: DeserializeOwned>(
    location: &Location,
    helper: &Path,
    args: &[&str],
) -> Result<T, String> {
    run_on(Lane::Main, location, helper, args)
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
