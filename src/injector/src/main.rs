use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex, PoisonError, mpsc},
};

use injector::{antiban, form, http, prices, transports};
#[cfg(windows)]
use {
    serde::Serialize,
    std::{path::Path, str::FromStr, time::Duration},
};

#[cfg(windows)]
const STACK_BYTES: usize = 256 << 20;

#[cfg(windows)]
const PACKETS_PER_READ: usize = 1000;
#[cfg(windows)]
const SCRIPT_RUNS_PER_READ: usize = 2000;
#[cfg(windows)]
const VARC_CHANGES_PER_READ: usize = 1000;
#[cfg(windows)]
const ITEM_CHANGES_PER_READ: usize = 2000;
#[cfg(windows)]
const ITEM_CHANGES_MAX_WAIT_MS: u64 = 5000;

#[cfg(windows)]
fn main() -> ExitCode {
    match std::thread::Builder::new()
        .name("main".into())
        .stack_size(STACK_BYTES)
        .spawn(run)
    {
        Ok(thread) => thread.join().unwrap_or(ExitCode::FAILURE),
        Err(error) => {
            eprintln!("Couldn't start the main thread: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn json<T: Serialize>(result: Result<T, String>) -> String {
    serde_json::to_string(&result).unwrap()
}

#[cfg(windows)]
fn reply<T: Serialize>(
    result: Option<Result<T, String>>,
    invalid: impl FnOnce() -> String,
) -> String {
    result.map_or_else(|| json::<()>(Err(invalid())), json)
}

#[cfg(windows)]
fn checked<T: Serialize>(
    args: &[&str],
    what: &str,
    run: impl FnOnce() -> Option<Result<T, String>>,
) -> String {
    reply(run(), || format!("Invalid {what}: {args:?}"))
}

#[cfg(windows)]
fn with_pid<T: Serialize>(pid: &str, run: impl FnOnce(u32) -> Result<T, String>) -> String {
    reply(num(pid).map(run), || format!("Invalid pid: {pid}"))
}

#[cfg(windows)]
fn page<T: Serialize>(
    pid: &str,
    since: &str,
    read: impl FnOnce(u32, u64) -> Result<T, String>,
) -> String {
    reply(
        num(pid)
            .zip(num(since))
            .map(|(pid, since)| read(pid, since)),
        || format!("Invalid pid or sequence number: {pid} {since}"),
    )
}

#[cfg(windows)]
fn num<T: FromStr>(text: &str) -> Option<T> {
    text.parse().ok()
}

#[cfg(windows)]
fn nums<T: FromStr>(texts: &[&str]) -> Option<Vec<T>> {
    texts.iter().map(|text| num(text)).collect()
}

#[cfg(windows)]
fn dispatch(args: &[&str]) -> String {
    if let Some(input) = injector::Input::from_args(args) {
        return json(input.and_then(|(pid, input)| injector::send_input(pid, &input)));
    }
    if let Some(overlay) = injector::Overlay::from_args(args) {
        return json(overlay.and_then(|(pid, overlay)| injector::draw(pid, &overlay)));
    }
    if let Some(cursor) = injector::Cursor::from_args(args) {
        return json(cursor.and_then(|(pid, cursor)| injector::set_cursor(pid, &cursor)));
    }
    match args {
        ["list"] => json(Ok(injector::list_clients())),
        ["load", pid, dll, cursor @ ..] => with_pid(pid, |pid| {
            let cursor = match cursor {
                [] => Ok(None),
                rest => {
                    let mut args = vec!["cursor", "0"];
                    args.extend(rest);
                    injector::Cursor::from_args(&args)
                        .transpose()
                        .map(|c| c.map(|(_, cursor)| cursor))
                }
            };
            cursor.and_then(|cursor| injector::load(pid, Path::new(dll), cursor.as_ref()))
        }),
        ["unload", pid] => with_pid(pid, injector::unload),
        ["status", pid] => with_pid(pid, |pid| Ok(injector::connection(pid))),
        ["console", pid] => with_pid(pid, injector::open_console),
        ["state", pid] => with_pid(pid, injector::game_state),
        ["name", pid, name] => with_pid(pid, |pid| injector::set_display_name(pid, name)),
        ["total-xp", pid, offset] => with_pid(pid, |pid| match *offset {
            "off" => injector::set_total_xp(pid, None),
            "random" => injector::game_state(pid).and_then(|state| {
                let real: i64 = state
                    .skills
                    .iter()
                    .flatten()
                    .map(|s| s.xp.max(0) as i64)
                    .sum();
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64);
                injector::set_total_xp(pid, Some(injector::random_total_xp_offset(real, seed)))
            }),
            offset => match offset.parse() {
                Ok(offset) => injector::set_total_xp(pid, Some(offset)),
                Err(_) => Err(format!(
                    "Invalid offset: {offset} (a number, random or off)"
                )),
            },
        }),
        ["varps", pid] => with_pid(pid, injector::varps),
        ["varp", pid, id] => checked(args, "varp command", || {
            Some(injector::varp_value(num(pid)?, num(id)?))
        }),
        ["varbit", pid, id] => checked(args, "varbit command", || {
            Some(injector::varbit(num(pid)?, num(id)?))
        }),
        ["varbits-of", pid, varp] => checked(args, "varbits-of command (varp)", || {
            Some(injector::varbits_of(num(pid)?, num(varp)?))
        }),
        ["enum", pid, id] => checked(args, "enum command", || {
            Some(injector::enum_def(num(pid)?, num(id)?))
        }),
        ["item-params", pid, ids @ ..] => checked(args, "item-params command", || {
            Some(injector::item_params(num(pid)?, &nums::<i32>(ids)?))
        }),
        ["containers", pid] => with_pid(pid, injector::item_containers),
        ["widget-tree", pid] => with_pid(pid, injector::widget_tree),
        ["container", pid, id] => checked(args, "container command", || {
            Some(injector::item_container(num(pid)?, num(id)?))
        }),
        ["project", pid, coords @ ..] => checked(args, "project command (x y height ...)", || {
            let pid = num(pid)?;
            let coords = nums::<i32>(coords)?;
            let (points, []) = coords.as_chunks::<3>() else {
                return None;
            };
            let points: Vec<_> = points
                .iter()
                .map(|&[x, y, height]| injector::projection::Point::new(x, y, height))
                .collect();
            Some(injector::project(
                pid,
                &points,
                injector::projection::Via::Auto,
            ))
        }),
        ["mem", pid, address, count] => {
            checked(args, "mem command (pid hex-address count)", || {
                Some(injector::memory_words(
                    num(pid)?,
                    u64::from_str_radix(address.trim_start_matches("0x"), 16).ok()?,
                    num(count)?,
                ))
            })
        }
        ["npc-words", pid, index, count, rest @ ..] if rest.len() <= 1 => {
            checked(args, "npc-words command (pid index count [def])", || {
                Some(injector::npc_words(
                    num(pid)?,
                    num(index)?,
                    num(count)?,
                    rest == ["def"],
                ))
            })
        }
        ["widget-words", pid, group, child, index, count] => checked(
            args,
            "widget-words command (pid group child index|-1 count)",
            || {
                Some(injector::widget_words(
                    num(pid)?,
                    num(group)?,
                    num(child)?,
                    num(index)?,
                    num(count)?,
                ))
            },
        ),
        ["widget-dump", pid, count] => checked(args, "widget-dump command (pid count)", || {
            Some(injector::widget_dump(num(pid)?, num(count)?))
        }),
        ["watch", pid, seconds, bytes] => {
            checked(args, "watch command (pid seconds bytes)", || {
                Some(injector::watch_client(
                    num(pid)?,
                    num(seconds)?,
                    num(bytes)?,
                ))
            })
        }
        ["world-entities", pid] => checked(args, "world-entities command", || {
            Some(injector::world_entities(num(pid)?))
        }),
        ["world-debug", pid] => checked(args, "world-debug command", || {
            Some(injector::world_debug(num(pid)?))
        }),
        ["player-words", pid, index, count] => {
            checked(args, "player-words command (pid index count)", || {
                Some(injector::player_words(num(pid)?, num(index)?, num(count)?))
            })
        }
        ["npc-model", pid, index] => checked(args, "npc-model command", || {
            Some(injector::npc_model(num(pid)?, num(index)?))
        }),
        ["player-model", pid, index] => checked(args, "player-model command", || {
            Some(injector::player_model(num(pid)?, num(index)?))
        }),
        ["cache-list", pid, index] => checked(args, "cache-list command (index [archive])", || {
            Some(injector::cache_archives(num(pid)?, num(index)?))
        }),
        ["cache-list", pid, index, archive] => {
            checked(args, "cache-list command (index [archive])", || {
                Some(injector::cache_archive_files(
                    num(pid)?,
                    num(index)?,
                    num(archive)?,
                ))
            })
        }
        ["cache-file", pid, index, archive, file, out] => {
            checked(args, "cache-file command (index archive file out)", || {
                let (pid, index, archive, file) =
                    (num(pid)?, num(index)?, num(archive)?, num(file)?);
                Some(
                    injector::cache_files(pid, index, &[[archive, file]]).and_then(|mut files| {
                        let data = files
                            .pop()
                            .flatten()
                            .ok_or_else(|| format!("no file {index}/{archive}/{file}"))?;
                        std::fs::write(out, &data)
                            .map_err(|e| format!("Cannot write {out}: {e}"))?;
                        Ok(data.len())
                    }),
                )
            })
        }
        ["collision", pid, plane, x1, y1, x2, y2] => {
            checked(args, "collision command (plane x1 y1 x2 y2)", || {
                Some(injector::collision_area(
                    num(pid)?,
                    num(plane)?,
                    num(x1)?,
                    num(y1)?,
                    num(x2)?,
                    num(y2)?,
                ))
            })
        }
        ["path", pid, plane, x1, y1, x2, y2] => {
            checked(args, "path command (plane x1 y1 x2 y2)", || {
                Some(injector::find_path(
                    num(pid)?,
                    num(plane)?,
                    [num(x1)?, num(y1)?],
                    [num(x2)?, num(y2)?],
                    None,
                ))
            })
        }
        ["route", pid, x1, y1, x2, y2, rest @ ..] => {
            checked(args, "route command (x1 y1 x2 y2 [seed])", || {
                let options = match rest {
                    [] => None,
                    [seed] => Some(injector::scene_graph::Options {
                        seed: num(seed)?,
                        ..Default::default()
                    }),
                    _ => return None,
                };
                Some(injector::scene_route(
                    num(pid)?,
                    [num(x1)?, num(y1)?],
                    [num(x2)?, num(y2)?],
                    options,
                ))
            })
        }
        ["scene-collision", pid] => with_pid(pid, injector::scene_collision),
        ["locs", pid] => with_pid(pid, |pid| injector::locs(pid, None)),
        ["loc-model", pid, kind, x, y, id] => {
            checked(args, "loc-model command (kind scene_x scene_y id)", || {
                Some(injector::loc_model(
                    num(pid)?,
                    num(kind)?,
                    num(x)?,
                    num(y)?,
                    num(id)?,
                ))
            })
        }
        ["menu", pid] => with_pid(pid, injector::menu),
        ["selected", pid] => with_pid(pid, injector::selected_widget),
        ["interfaces", pid] => with_pid(pid, injector::sub_interfaces),
        ["game-vals", pid, kind, ids @ ..] => checked(args, "game-vals command", || {
            Some(injector::game_val_names(
                num(pid)?,
                num(kind)?,
                &nums::<i32>(ids)?,
            ))
        }),
        ["item-names", pid, ids @ ..] => checked(args, "item-names command", || {
            Some(injector::item_names(num(pid)?, &nums::<i32>(ids)?))
        }),
        ["item-icons", pid, icons @ ..] => checked(
            args,
            "item-icons command (id[:quantity[:1 to draw the count]]...)",
            || {
                let pid = num(pid)?;
                let icons = icons
                    .iter()
                    .map(|icon| {
                        let mut parts = icon.split(':');
                        let id = num(parts.next()?)?;
                        let quantity = parts.next().map_or(Some(1), num)?;
                        let show_count = parts.next().is_some_and(|c| c == "1");
                        Some(injector::ItemIconRequest {
                            id,
                            quantity,
                            show_count,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(injector::item_icons(pid, &icons))
            },
        ),
        ["sprites", pid, ids @ ..] => checked(args, "sprites command", || {
            Some(injector::sprites(num(pid)?, &nums::<i32>(ids)?))
        }),
        ["item-families", pid, ids @ ..] => checked(args, "item-families command", || {
            Some(injector::item_families(num(pid)?, &nums::<i32>(ids)?))
        }),
        ["item-stackable", pid, ids @ ..] => checked(args, "item-stackable command", || {
            Some(injector::item_stackable(num(pid)?, &nums::<i32>(ids)?))
        }),
        ["varc", pid, id] => checked(args, "varc command", || {
            Some(injector::varc(num(pid)?, num(id)?))
        }),
        ["tiles-next", pid, after, layers @ ..] => checked(args, "tiles-next command", || {
            Some(injector::scene_tiles_next_frame(
                num(pid)?,
                layers,
                num(after)?,
            ))
        }),
        ["tiles", pid, layers @ ..] => with_pid(pid, |pid| injector::scene_tiles(pid, layers)),
        ["npcs", pid] => with_pid(pid, injector::npcs),
        ["projectiles", pid] => with_pid(pid, injector::projectiles),
        ["spot-anims", pid] => with_pid(pid, injector::spot_anims),
        ["packets", pid, since] => page(pid, since, |pid, since| {
            injector::packets(pid, since, PACKETS_PER_READ)
        }),
        ["packet-layouts", pid] => with_pid(pid, injector::packet_layouts),
        ["scripts", pid, since] => page(pid, since, |pid, since| {
            injector::script_runs(pid, since, SCRIPT_RUNS_PER_READ)
        }),
        ["varc-changes", pid, since] => page(pid, since, |pid, since| {
            injector::varc_changes(pid, since, VARC_CHANGES_PER_READ)
        }),
        ["item-changes", pid, since, rest @ ..] => {
            checked(args, "pid, sequence number or wait", || {
                let wait = match rest.first() {
                    Some(wait) => num::<u64>(wait)?,
                    None => 0,
                };
                Some(injector::item_changes_wait(
                    num(pid)?,
                    num(since)?,
                    ITEM_CHANGES_PER_READ,
                    Duration::from_millis(wait.min(ITEM_CHANGES_MAX_WAIT_MS)),
                ))
            })
        }
        ["chat", pid] => with_pid(pid, injector::chat_messages),
        ["in-instance", pid] => with_pid(pid, injector::in_instance),
        ["map-regions", pid] => with_pid(pid, injector::map_regions),
        ["ground-items", pid] => with_pid(pid, injector::ground_items),
        ["children", pid, group, child] => checked(args, "children command", || {
            Some(injector::widget_children(
                num(pid)?,
                num(group)?,
                num(child)?,
            ))
        }),
        ["world-select", pid] => with_pid(pid, injector::world_select),
        ["players", pid] => with_pid(pid, injector::players),
        ["screenshot", pid, path] => {
            with_pid(pid, |pid| injector::screenshot(pid, Path::new(path)))
        }
        ["widget", pid, group, child] => checked(args, "widget command", || {
            Some(injector::widget(num(pid)?, num(group)?, num(child)?))
        }),
        _ => json::<()>(Err(format!("Unknown command: {args:?}"))),
    }
}

#[cfg(windows)]
fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    if args == ["serve"] {
        let mut stdout = std::io::stdout();
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Vec<String>>(&line) {
                Ok(args) => dispatch(&args.iter().map(String::as_str).collect::<Vec<_>>()),
                Err(error) => json::<()>(Err(format!("Invalid command line: {error}"))),
            };
            if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
        return ExitCode::SUCCESS;
    }

    if let ["run", rest @ ..] = args.as_slice() {
        return run_script(rest);
    }

    if args.is_empty() {
        eprintln!(
            "Usage: injector list | load <pid> <dll> [<style> <r> <g> <b> <size>] | unload <pid> | status <pid> | state <pid> | console <pid>"
        );
        eprintln!(
            "       injector widget <pid> <group> <child> | children <pid> <group> <child> | interfaces <pid> | screenshot <pid> <file.bmp>"
        );
        eprintln!(
            "       injector varp <pid> <id> | varps <pid> | varbit <pid> <id> | enum <pid> <id> | item-params <pid> <ids...> | name <pid> <display name>"
        );
        eprintln!(
            "       injector container <pid> <id> | containers <pid> | widget-tree <pid> | widget-words <pid> <group> <child> <index|-1> <count> | widget-dump <pid> <count> | item-names <pid> <id>... | item-icons <pid> <id[:quantity[:1]]>... | sprites <pid> <id>... | item-stackable <pid> <id>... | item-families <pid> <id>... | varc <pid> <id> | npcs <pid> | tiles <pid> [layer]... | npc-model <pid> <index> | spot-anims <pid> | packets <pid> <since> | scripts <pid> <since> | varc-changes <pid> <since> | item-changes <pid> <since> [wait ms] (u64 max: only the position) | locs <pid> | loc-model <pid> <kind> <scene x> <scene y> <id> | ground-items <pid> | chat <pid> | players <pid> | world-select <pid> | selected <pid>"
        );
        eprintln!(
            "       injector move <pid> <x> <y> | click <pid> left|right|middle <x> <y> | type <pid> <text> [hold ms]"
        );
        eprintln!(
            "       injector button <pid> left|right|middle down|up | scroll <pid> <notches> | key <pid> <vk> down|up"
        );
        eprintln!(
            "       injector wind_mouse <pid> <x> <y> <speed> <wind> <gravity> <timeout ms> <pace>"
        );
        eprintln!(
            "       injector overlay <pid> rect <x1> <y1> <x2> <y2> <r> <g> <b> <a> | overlay <pid> clear"
        );
        eprintln!("       injector serve | run [--script <path> [--lib <dir>]] [pid]");
        return ExitCode::from(2);
    }
    println!("{}", dispatch(&args));
    ExitCode::SUCCESS
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["run", rest @ ..] = args.as_slice()
        && matches!(rest, [] | ["--script", _] | ["--script", _, "--lib", _])
    {
        return run_script(rest);
    }
    eprintln!("injector only runs on Windows or under Wine (except `run` with no pid).");
    ExitCode::FAILURE
}

fn unreadable(what: &str) -> ExitCode {
    eprintln!("Couldn't read the {what} from stdin.");
    ExitCode::from(2)
}

fn read_block(input: &mut impl Read, len: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    input.take(len as u64).read_to_end(&mut bytes).ok()?;
    (bytes.len() == len).then_some(bytes)
}

fn apply_setting(line: &str) -> bool {
    if let Some(payload) = line.strip_prefix(antiban::PREFIX) {
        if let Some(config) = antiban::parse(payload) {
            antiban::set(config);
        }
    } else if let Some(payload) = line.strip_prefix(prices::PREFIX) {
        if let Some(url) = prices::parse_url(payload) {
            prices::set_url(url);
        }
    } else if let Some(payload) = line.strip_prefix(transports::SETTINGS_PREFIX) {
        if let Some(settings) = transports::parse_settings(payload) {
            transports::set_settings(settings);
        }
    } else {
        return false;
    }
    true
}

fn ask(replies: &Mutex<mpsc::Receiver<String>>, request: String) -> Result<String, String> {
    {
        let mut out = std::io::stdout().lock();
        write!(out, "{request}{}", form::ENTRY_END)
            .and_then(|()| out.flush())
            .map_err(|e| e.to_string())?;
    }
    replies
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recv()
        .map_err(|_| "the loader closed the script's input".to_owned())
}

fn run_script(args: &[&str]) -> ExitCode {
    let (script_path, args) = match args {
        ["--script", path, rest @ ..] => (Some(PathBuf::from(path)), rest),
        _ => (None, args),
    };
    let (lib_dir, args) = match args {
        ["--lib", dir, rest @ ..] => (Some(PathBuf::from(dir)), rest),
        _ => (None, args),
    };
    let pid = match args {
        [] => None,
        [pid] => match pid.parse() {
            Ok(pid) => Some(pid),
            Err(_) => {
                eprintln!("Invalid pid: {pid}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("Usage: injector run [--script <path> [--lib <dir>]] [pid]");
            return ExitCode::from(2);
        }
    };
    #[cfg(windows)]
    if let Some(pid) = pid {
        match injector::connection(pid) {
            None => {
                eprintln!(
                    "The agent isn't loaded in client {pid}. Load it first (`injector load <pid> <dll>`)."
                );
                return ExitCode::FAILURE;
            }
            Some(conn) => {
                if let Some(problem) = conn.problem() {
                    eprintln!("Client {pid}: {problem}.");
                    return ExitCode::FAILURE;
                }
            }
        }
    }

    let mut input = BufReader::new(std::io::stdin());
    let mut first = String::new();
    if input.read_line(&mut first).is_err() {
        return unreadable("script");
    }
    let (mut loader_has_pins, mut loader_has_http) = (false, false);
    loop {
        match first.trim_end() {
            form::PINS_PREFIX => loader_has_pins = true,
            http::CAPABILITY_PREFIX => loader_has_http = true,
            line if apply_setting(line) => {}
            line if line.starts_with(form::SETTINGS_PREFIX) => {
                match form::parse_settings(&line[form::SETTINGS_PREFIX.len()..]) {
                    Some(values) => form::set_settings(values),
                    None => return unreadable("script's settings"),
                }
            }
            line if line.starts_with(form::RESUME_PREFIX) => {
                if let Some(plan) = form::parse_breaks(&line[form::RESUME_PREFIX.len()..]) {
                    form::set_resume(plan);
                }
            }
            line if line.starts_with(transports::PREFIX) => {
                if let Some(data) = transports::parse(&line[transports::PREFIX.len()..]) {
                    transports::set(data);
                }
            }
            _ => break,
        }
        first.clear();
        if input.read_line(&mut first).is_err() {
            return unreadable("script");
        }
    }
    let mut modules = HashMap::new();
    if let Some(len) = first
        .strip_prefix(form::MODULES_PREFIX)
        .map(|len| len.trim().parse::<usize>())
    {
        let files = len.ok().and_then(|len| {
            serde_json::from_slice::<HashMap<String, String>>(&read_block(&mut input, len)?).ok()
        });
        first.clear();
        match files {
            Some(files) if input.read_line(&mut first).is_ok() => modules = files,
            _ => return unreadable("script's files"),
        }
    }
    if !form::settings_given()
        && let Some(path) = script_path.as_deref()
    {
        match injector::manifest::Manifest::beside(path) {
            Ok(manifest) => form::set_settings(manifest.unwrap_or_default().defaults()),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let source = match first
        .strip_prefix(form::SCRIPT_PREFIX)
        .map(|len| len.trim().parse::<usize>())
    {
        Some(Ok(len)) => {
            let Some(bytes) = read_block(&mut input, len) else {
                return unreadable("script");
            };
            let (replies_in, replies) = mpsc::channel::<String>();
            std::thread::spawn(move || {
                let mut line = String::new();
                loop {
                    line.clear();
                    if !matches!(input.read_line(&mut line), Ok(read) if read > 0) {
                        break;
                    }
                    if apply_setting(&line) {
                        continue;
                    }
                    if line.trim_end() == form::FINISH_PREFIX {
                        form::request_finish();
                        continue;
                    }
                    if let Some(payload) = line.strip_prefix(antiban::SKIP_PREFIX) {
                        if let Some(skip) = antiban::parse_skip(payload) {
                            antiban::push_skip(skip);
                        }
                    } else if let Some(payload) = line.strip_prefix(form::UPDATE_PREFIX) {
                        if let Some(values) = form::parse_update(payload) {
                            form::push_update(values);
                        }
                    } else if replies_in.send(std::mem::take(&mut line)).is_err() {
                        break;
                    }
                }
            });
            let replies = Arc::new(Mutex::new(replies));
            if loader_has_pins {
                let pin_replies = replies.clone();
                injector::bank_pin::set_lookup(Box::new(move |hash| {
                    form::parse_pin_reply(&ask(&pin_replies, form::pin_request_line(hash))?)
                }));
                let lamp_replies = replies.clone();
                injector::lamp::set_lookup(Box::new(move |hash| {
                    form::parse_lamp_reply(&ask(&lamp_replies, form::lamp_request_line(hash))?)
                }));
            }
            if loader_has_http {
                http::set_handler(Box::new(move |request| {
                    http::parse_reply(&ask(&replies, http::request_line(request))?)
                }));
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
        _ => {
            let mut rest = String::new();
            if input.read_to_string(&mut rest).is_err() {
                return unreadable("script");
            }
            first + &rest
        }
    };

    let stdout = std::io::stdout();
    let name = script_path
        .as_deref()
        .and_then(std::path::Path::file_name)
        .map_or_else(
            || "script.luau".into(),
            |n| n.to_string_lossy().into_owned(),
        );
    let result = injector::sandbox::run_with_modules(
        &name,
        &source,
        pid,
        move |line| {
            let mut out = stdout.lock();
            let _ = write!(out, "{line}{}", form::ENTRY_END);
            let _ = out.flush();
        },
        script_path.as_deref(),
        lib_dir.as_deref(),
        modules,
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
