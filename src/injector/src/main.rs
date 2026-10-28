#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::{
        io::{BufRead, Write},
        path::Path,
    };

    fn json<T: serde::Serialize>(result: Result<T, String>) -> String {
        serde_json::to_string(&result).unwrap()
    }

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
            ["load", pid, dll, cursor @ ..] => match pid.parse() {
                Ok(pid) => {
                    // The optional cursor is the `cursor` command's arguments after the pid.
                    let cursor = match cursor {
                        [] => Ok(None),
                        rest => {
                            let mut args = vec!["cursor", "0"];
                            args.extend(rest);
                            injector::Cursor::from_args(&args).transpose().map(|c| c.map(|(_, cursor)| cursor))
                        }
                    };
                    json(cursor.and_then(|cursor| injector::load(pid, Path::new(dll), cursor.as_ref())))
                }
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["unload", pid] => match pid.parse() {
                Ok(pid) => json(injector::unload(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["status", pid] => match pid.parse() {
                Ok(pid) => json(Ok(injector::connection(pid))),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["console", pid] => match pid.parse() {
                Ok(pid) => json(injector::open_console(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["state", pid] => match pid.parse() {
                Ok(pid) => json(injector::game_state(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["name", pid, name] => match pid.parse() {
                Ok(pid) => json(injector::set_display_name(pid, name)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["total-xp", pid, offset] => match pid.parse::<u32>() {
                Ok(pid) => match *offset {
                    "off" => json(injector::set_total_xp(pid, None)),
                    "random" => json(injector::game_state(pid).and_then(|state| {
                        let real: i64 = state.skills.iter().flatten().map(|s| s.xp.max(0) as i64).sum();
                        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
                        injector::set_total_xp(pid, Some(injector::random_total_xp_offset(real, seed)))
                    })),
                    offset => match offset.parse() {
                        Ok(offset) => json(injector::set_total_xp(pid, Some(offset))),
                        Err(_) => json::<()>(Err(format!("Invalid offset: {offset} (a number, random or off)"))),
                    },
                },
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["varps", pid] => match pid.parse() {
                Ok(pid) => json(injector::varps(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["varp", pid, id] => match (pid.parse(), id.parse()) {
                (Ok(pid), Ok(id)) => json(injector::varp_value(pid, id)),
                _ => json::<()>(Err(format!("Invalid varp command: {args:?}"))),
            },
            ["varbit", pid, id] => match (pid.parse(), id.parse()) {
                (Ok(pid), Ok(id)) => json(injector::varbit(pid, id)),
                _ => json::<()>(Err(format!("Invalid varbit command: {args:?}"))),
            },
            ["varbits-of", pid, varp] => match (pid.parse(), varp.parse()) {
                (Ok(pid), Ok(varp)) => json(injector::varbits_of(pid, varp)),
                _ => json::<()>(Err(format!("Invalid varbits-of command (varp): {args:?}"))),
            },
            ["enum", pid, id] => match (pid.parse(), id.parse()) {
                (Ok(pid), Ok(id)) => json(injector::enum_def(pid, id)),
                _ => json::<()>(Err(format!("Invalid enum command: {args:?}"))),
            },
            ["item-params", pid, ids @ ..] => match (pid.parse(), ids.iter().map(|id| id.parse()).collect::<Result<Vec<i32>, _>>()) {
                (Ok(pid), Ok(ids)) => json(injector::item_params(pid, &ids)),
                _ => json::<()>(Err(format!("Invalid item-params command: {args:?}"))),
            },
            ["container", pid, id] => match (pid.parse(), id.parse()) {
                (Ok(pid), Ok(id)) => json(injector::item_container(pid, id)),
                _ => json::<()>(Err(format!("Invalid container command: {args:?}"))),
            },
            ["project", pid, coords @ ..] => match (pid.parse(), coords.iter().map(|c| c.parse()).collect::<Result<Vec<i32>, _>>()) {
                (Ok(pid), Ok(c)) if c.len() % 3 == 0 => {
                    let points: Vec<injector::projection::Point> = c.chunks(3).map(|p| injector::projection::Point::new(p[0], p[1], p[2])).collect();
                    json(injector::project(pid, &points, injector::projection::Via::Auto))
                }
                _ => json::<()>(Err(format!("Invalid project command (x y height ...): {args:?}"))),
            },
            ["npc-words", pid, index, count, rest @ ..] if rest.len() <= 1 => match (pid.parse(), index.parse(), count.parse()) {
                (Ok(pid), Ok(index), Ok(count)) => json(injector::npc_words(pid, index, count, rest == ["def"])),
                _ => json::<()>(Err(format!("Invalid npc-words command (pid index count [def]): {args:?}"))),
            },
            ["npc-model", pid, index] => match (pid.parse(), index.parse()) {
                (Ok(pid), Ok(index)) => json(injector::npc_model(pid, index)),
                _ => json::<()>(Err(format!("Invalid npc-model command: {args:?}"))),
            },
            ["player-model", pid, index] => match (pid.parse(), index.parse()) {
                (Ok(pid), Ok(index)) => json(injector::player_model(pid, index)),
                _ => json::<()>(Err(format!("Invalid player-model command: {args:?}"))),
            },
            ["cache-list", pid, index] => match (pid.parse(), index.parse()) {
                (Ok(pid), Ok(index)) => json(injector::cache_archives(pid, index)),
                _ => json::<()>(Err(format!("Invalid cache-list command (index [archive]): {args:?}"))),
            },
            ["cache-list", pid, index, archive] => match (pid.parse(), index.parse(), archive.parse()) {
                (Ok(pid), Ok(index), Ok(archive)) => json(injector::cache_archive_files(pid, index, archive)),
                _ => json::<()>(Err(format!("Invalid cache-list command (index [archive]): {args:?}"))),
            },
            ["cache-file", pid, index, archive, file, out] => match (pid.parse(), index.parse(), archive.parse(), file.parse()) {
                (Ok(pid), Ok(index), Ok(archive), Ok(file)) => json(injector::cache_files(pid, index, &[[archive, file]]).and_then(|mut files| {
                    let data = files.pop().flatten().ok_or_else(|| format!("no file {index}/{archive}/{file}"))?;
                    std::fs::write(out, &data).map_err(|e| format!("Cannot write {out}: {e}"))?;
                    Ok(data.len())
                })),
                _ => json::<()>(Err(format!("Invalid cache-file command (index archive file out): {args:?}"))),
            },
            ["collision", pid, plane, x1, y1, x2, y2] => match (pid.parse(), plane.parse(), x1.parse(), y1.parse(), x2.parse(), y2.parse()) {
                (Ok(pid), Ok(plane), Ok(x1), Ok(y1), Ok(x2), Ok(y2)) => json(injector::collision_area(pid, plane, x1, y1, x2, y2)),
                _ => json::<()>(Err(format!("Invalid collision command (plane x1 y1 x2 y2): {args:?}"))),
            },
            ["path", pid, plane, x1, y1, x2, y2] => match (pid.parse(), plane.parse(), x1.parse(), y1.parse(), x2.parse(), y2.parse()) {
                (Ok(pid), Ok(plane), Ok(x1), Ok(y1), Ok(x2), Ok(y2)) => json(injector::find_path(pid, plane, [x1, y1], [x2, y2], None)),
                _ => json::<()>(Err(format!("Invalid path command (plane x1 y1 x2 y2): {args:?}"))),
            },
            ["route", pid, x1, y1, x2, y2, rest @ ..] => match (pid.parse(), x1.parse(), y1.parse(), x2.parse(), y2.parse(), rest) {
                // an optional seed asks for a randomized route
                (Ok(pid), Ok(x1), Ok(y1), Ok(x2), Ok(y2), []) => json(injector::scene_route(pid, [x1, y1], [x2, y2], None)),
                (Ok(pid), Ok(x1), Ok(y1), Ok(x2), Ok(y2), [seed]) if seed.parse::<u64>().is_ok() => {
                    let options = injector::scene_graph::Options { seed: seed.parse().unwrap_or_default(), ..Default::default() };
                    json(injector::scene_route(pid, [x1, y1], [x2, y2], Some(options)))
                }
                _ => json::<()>(Err(format!("Invalid route command (x1 y1 x2 y2 [seed]): {args:?}"))),
            },
            ["scene-collision", pid] => match pid.parse() {
                Ok(pid) => json(injector::scene_collision(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["locs", pid] => match pid.parse() {
                Ok(pid) => json(injector::locs(pid, None)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["loc-model", pid, kind, x, y, id] => match (pid.parse(), kind.parse(), x.parse(), y.parse(), id.parse()) {
                (Ok(pid), Ok(kind), Ok(x), Ok(y), Ok(id)) => json(injector::loc_model(pid, kind, x, y, id)),
                _ => json::<()>(Err(format!("Invalid loc-model command (kind scene_x scene_y id): {args:?}"))),
            },
            ["menu", pid] => match pid.parse() {
                Ok(pid) => json(injector::menu(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["selected", pid] => match pid.parse() {
                Ok(pid) => json(injector::selected_widget(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["interfaces", pid] => match pid.parse() {
                Ok(pid) => json(injector::sub_interfaces(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["game-vals", pid, kind, ids @ ..] => match (pid.parse(), kind.parse(), ids.iter().map(|id| id.parse()).collect::<Result<Vec<i32>, _>>()) {
                (Ok(pid), Ok(kind), Ok(ids)) => json(injector::game_val_names(pid, kind, &ids)),
                _ => json::<()>(Err(format!("Invalid game-vals command: {args:?}"))),
            },
            ["item-names", pid, ids @ ..] => match (pid.parse(), ids.iter().map(|id| id.parse()).collect::<Result<Vec<i32>, _>>()) {
                (Ok(pid), Ok(ids)) => json(injector::item_names(pid, &ids)),
                _ => json::<()>(Err(format!("Invalid item-names command: {args:?}"))),
            },
            ["varc", pid, id] => match (pid.parse(), id.parse()) {
                (Ok(pid), Ok(id)) => json(injector::varc(pid, id)),
                _ => json::<()>(Err(format!("Invalid varc command: {args:?}"))),
            },
            ["tiles-next", pid, after, layers @ ..] => match (pid.parse(), after.parse()) {
                (Ok(pid), Ok(after)) => json(injector::scene_tiles_next_frame(pid, layers, after)),
                _ => json::<()>(Err(format!("Invalid tiles-next command: {args:?}"))),
            },
            ["tiles", pid, layers @ ..] => match pid.parse() {
                Ok(pid) => json(injector::scene_tiles(pid, layers)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["npcs", pid] => match pid.parse() {
                Ok(pid) => json(injector::npcs(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["chat", pid] => match pid.parse() {
                Ok(pid) => json(injector::chat_messages(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["in-instance", pid] => match pid.parse() {
                Ok(pid) => json(injector::in_instance(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["map-regions", pid] => match pid.parse() {
                Ok(pid) => json(injector::map_regions(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["ground-items", pid] => match pid.parse() {
                Ok(pid) => json(injector::ground_items(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["children", pid, group, child] => match (pid.parse(), group.parse(), child.parse()) {
                (Ok(pid), Ok(group), Ok(child)) => json(injector::widget_children(pid, group, child)),
                _ => json::<()>(Err(format!("Invalid children command: {args:?}"))),
            },
            ["world-select", pid] => match pid.parse() {
                Ok(pid) => json(injector::world_select(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["players", pid] => match pid.parse() {
                Ok(pid) => json(injector::players(pid)),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["screenshot", pid, path] => match pid.parse() {
                Ok(pid) => json(injector::screenshot(pid, std::path::Path::new(path))),
                Err(_) => json::<()>(Err(format!("Invalid pid: {pid}"))),
            },
            ["widget", pid, group, child] => match (pid.parse(), group.parse(), child.parse()) {
                (Ok(pid), Ok(group), Ok(child)) => json(injector::widget(pid, group, child)),
                _ => json::<()>(Err(format!("Invalid widget command: {args:?}"))),
            },
            _ => json::<()>(Err(format!("Unknown command: {args:?}"))),
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    if args == ["serve"] {
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
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
        return std::process::ExitCode::SUCCESS;
    }

    if let ["run", rest @ ..] = args.as_slice() {
        return run_script(rest);
    }

    if args.is_empty() {
        eprintln!("Usage: injector list | load <pid> <dll> [<style> <r> <g> <b> <size>] | unload <pid> | status <pid> | state <pid> | console <pid>");
        eprintln!("       injector widget <pid> <group> <child> | children <pid> <group> <child> | interfaces <pid> | screenshot <pid> <file.bmp>");
        eprintln!("       injector varp <pid> <id> | varps <pid> | varbit <pid> <id> | enum <pid> <id> | item-params <pid> <ids...> | name <pid> <display name>");
        eprintln!("       injector container <pid> <id> | item-names <pid> <id>... | varc <pid> <id> | npcs <pid> | tiles <pid> [layer]... | npc-model <pid> <index> | locs <pid> | loc-model <pid> <kind> <scene x> <scene y> <id> | ground-items <pid> | chat <pid> | players <pid> | world-select <pid> | selected <pid>");
        eprintln!("       injector move <pid> <x> <y> | click <pid> left|right|middle <x> <y> | type <pid> <text> [hold ms]");
        eprintln!("       injector button <pid> left|right|middle down|up | scroll <pid> <notches> | key <pid> <vk> down|up");
        eprintln!("       injector wind_mouse <pid> <x> <y> <speed> <wind> <gravity> <timeout ms>");
        eprintln!("       injector overlay <pid> rect <x1> <y1> <x2> <y2> <r> <g> <b> <a> | overlay <pid> clear");
        eprintln!("       injector serve | run [--script <path> [--lib <dir>]] [pid]");
        return std::process::ExitCode::from(2);
    }
    println!("{}", dispatch(&args));
    std::process::ExitCode::SUCCESS
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["run", rest @ ..] = args.as_slice()
        && matches!(rest, [] | ["--script", _] | ["--script", _, "--lib", _])
    {
        return run_script(rest);
    }
    eprintln!("injector only runs on Windows or under Wine (except `run` with no pid).");
    std::process::ExitCode::FAILURE
}

fn run_script(args: &[&str]) -> std::process::ExitCode {
    use std::io::{Read, Write};

    let (script_path, args) = match args {
        ["--script", path, rest @ ..] => (Some(std::path::PathBuf::from(path)), rest),
        _ => (None, args),
    };
    let (lib_dir, args) = match args {
        ["--lib", dir, rest @ ..] => (Some(std::path::PathBuf::from(dir)), rest),
        _ => (None, args),
    };
    let pid = match args {
        [] => None,
        [pid] => match pid.parse() {
            Ok(pid) => Some(pid),
            Err(_) => {
                eprintln!("Invalid pid: {pid}");
                return std::process::ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("Usage: injector run [--script <path> [--lib <dir>]] [pid]");
            return std::process::ExitCode::from(2);
        }
    };
    // Scripts assume the agent is loaded (`connection()` always returns a connection), so refuse to start without it (or with a mismatched protocol).
    #[cfg(windows)]
    if let Some(pid) = pid {
        match injector::connection(pid) {
            None => {
                eprintln!("The agent isn't loaded in client {pid}. Load it first (`injector load <pid> <dll>`).");
                return std::process::ExitCode::FAILURE;
            }
            Some(conn) => {
                if let Some(problem) = conn.problem() {
                    eprintln!("Client {pid}: {problem}.");
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
    }
    use std::io::BufRead;
    use injector::form::{self, SCRIPT_PREFIX};

    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    let mut first = String::new();
    if input.read_line(&mut first).is_err() {
        eprintln!("Couldn't read the script from stdin.");
        return std::process::ExitCode::from(2);
    }
    // The script's other files can come first, so they never touch the disk.
    let mut modules = std::collections::HashMap::new();
    if let Some(len) = first.strip_prefix(form::MODULES_PREFIX).map(|len| len.trim().parse::<usize>()) {
        let files = len.ok().and_then(|len| {
            let mut bytes = vec![0u8; len];
            input.read_exact(&mut bytes).ok()?;
            serde_json::from_slice::<std::collections::HashMap<String, String>>(&bytes).ok()
        });
        first.clear();
        match files {
            Some(files) if input.read_line(&mut first).is_ok() => modules = files,
            _ => {
                eprintln!("Couldn't read the script's files from stdin.");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let (source, handler) = match first.strip_prefix(SCRIPT_PREFIX).map(|len| len.trim().parse::<usize>()) {
        Some(Ok(len)) => {
            let mut bytes = vec![0u8; len];
            if input.read_exact(&mut bytes).is_err() {
                eprintln!("Couldn't read the script from stdin.");
                return std::process::ExitCode::from(2);
            }
            let handler: form::FormHandler = Box::new(move |spec| {
                {
                    let mut out = std::io::stdout().lock();
                    write!(out, "{}{}", form::request_line(spec), form::ENTRY_END).and_then(|()| out.flush()).map_err(|e| e.to_string())?;
                }
                let mut reply = String::new();
                match input.read_line(&mut reply) {
                    Ok(0) | Err(_) => Err("the loader closed the script's input".into()),
                    Ok(_) => form::parse_reply(&reply),
                }
            });
            (String::from_utf8_lossy(&bytes).into_owned(), Some(handler))
        }
        _ => {
            let mut rest = String::new();
            if input.read_to_string(&mut rest).is_err() {
                eprintln!("Couldn't read the script from stdin.");
                return std::process::ExitCode::from(2);
            }
            (first + &rest, None)
        }
    };

    let stdout = std::io::stdout();
    let name = script_path.as_deref().and_then(std::path::Path::file_name).map_or_else(|| "script.luau".into(), |n| n.to_string_lossy().into_owned());
    let result = injector::sandbox::run_with_modules(&name, &source, pid, move |line| {
        let mut out = stdout.lock();
        let _ = write!(out, "{line}{}", form::ENTRY_END);
        let _ = out.flush();
    }, handler, script_path.as_deref(), lib_dir.as_deref(), modules);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
