mod library;
mod runs;

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

use injector::Cursor;
use loader::backend::{self, Settings, Target};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::api::{Api, Config};
use crate::script::{Host, lock};

const PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const RUN_WAIT: u64 = 30;
const MAX_WAIT: u64 = 120;
const PAGE_LINES: usize = 300;
const INVENTORY: i32 = 93;
const EQUIPMENT: i32 = 94;
const SIGN_IN: &str =
    "The loader isn't signed in. Open the loader and sign in once; `loader mcp` uses that sign-in.";

const INSTRUCTIONS: &str = "VOLCREN automates a popular MMORPG's game client through the user's desktop loader. You act in the game by writing short Luau scripts against the VOLCREN library and running them with run_luau, never by sending clicks yourself.

Workflow:
1. clients lists the game clients. load_agent injects the agent into one that isn't loaded or is outdated.
2. game_state shows the player's position, skills, inventory and equipment.
3. Before using a library module, find it with lib_docs (search, then read the page). Don't guess function names or signatures.
4. check_luau a script before running it.
5. run_luau runs it on one client and returns its output. A run that outlasts the wait keeps going: read it with run_output and end it with stop_run. One script runs per client at a time.

Scripts:
- Start with --!strict and require library modules by path: docs/main/walker.md documents require(\"@lib/main/walker\"), docs/host.md documents require(\"@lib/host\").
- Prefer the library's high-level functions (walking, interacting, inventory, bank, interfaces) to raw Host input. They move the mouse, time actions and handle failures like a person, which matters more than speed.
- print() output comes back to you. Keep scripts small, check the result, then continue.
- Runs act on the user's real account. Ask the user before anything that spends coins, trades, uses the Grand Exchange, drops or alchs valuable items, or is otherwise risky. Stop runs you no longer need; all runs stop when you exit.";

struct Saved {
    settings: Settings,
    cursor: Cursor,
    whitelist: Vec<String>,
}

fn saved() -> Saved {
    let map: HashMap<String, String> = eframe::storage_dir(crate::APP_ID)
        .and_then(|dir| std::fs::read_to_string(dir.join("app.ron")).ok())
        .and_then(|text| ron::from_str(&text).ok())
        .unwrap_or_default();
    fn value<T: DeserializeOwned>(map: &HashMap<String, String>, key: &str) -> Option<T> {
        map.get(key).and_then(|text| ron::from_str(text).ok())
    }
    let mut settings: Settings = value(&map, eframe::APP_KEY).unwrap_or_default();
    settings.fill_missing(crate::update::is_dev_install());
    Saved {
        settings,
        cursor: value(&map, crate::app::CURSOR_KEY).unwrap_or(crate::app::DEFAULT_CURSOR),
        whitelist: value(&map, crate::app::HTTP_WHITELIST_KEY).unwrap_or_default(),
    }
}

struct Context {
    api: Api,
    signed_in: Mutex<bool>,
    lib: Mutex<Option<PathBuf>>,
    runs: runs::Runs,
}

impl Context {
    fn signed_in(&self) -> Result<&Api, String> {
        let mut signed_in = lock(&self.signed_in);
        if !*signed_in {
            let missing = self.api.config().missing();
            if !missing.is_empty() {
                return Err(format!("This loader build lacks {}.", missing.join(", ")));
            }
            *signed_in = self.api.restore().map_err(|e| e.to_string())?;
        }
        if *signed_in {
            Ok(&self.api)
        } else {
            Err(SIGN_IN.into())
        }
    }

    fn lib(&self, settings: &Settings) -> Result<PathBuf, String> {
        let dir = crate::workspace::lib_dir(settings)?;
        if !settings.lib_dir.trim().is_empty() {
            return if dir.is_dir() {
                Ok(dir)
            } else {
                Err(format!(
                    "The lib folder {} set in the loader's settings doesn't exist.",
                    dir.display()
                ))
            };
        }
        let mut ensured = lock(&self.lib);
        if let Some(dir) = ensured.as_ref() {
            return Ok(dir.clone());
        }
        let root =
            crate::session::cache_root().ok_or("Can't find a cache folder on this system.")?;
        let lib = crate::libs::ensure(&self.api, &root)?;
        if let Some(warning) = lib.warning {
            eprintln!("{warning}");
        }
        *ensured = Some(dir.clone());
        Ok(dir)
    }
}

fn target(settings: &Settings, pid: u32) -> Result<Target, String> {
    let scan = backend::scan(settings);
    let errors = scan.errors.join("; ");
    scan.targets
        .into_iter()
        .find(|t| t.client.pid == pid)
        .ok_or_else(|| {
            let mut why = format!("No game client has pid {pid}. Call clients for the list.");
            if !errors.is_empty() {
                why.push_str(&format!(" Errors: {errors}"));
            }
            why
        })
}

fn loaded(settings: &Settings, pid: u32) -> Result<Target, String> {
    let target = target(settings, pid)?;
    match &target.client.loaded {
        Some(_) => Ok(target),
        None => Err(format!(
            "The agent isn't loaded in client {pid}. Call load_agent first."
        )),
    }
}

pub fn serve() {
    let context = Arc::new(Context {
        api: Api::new(Config::from_env()),
        signed_in: Mutex::new(false),
        lib: Mutex::new(None),
        runs: runs::Runs::default(),
    });
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let mut pending: Vec<thread::JoinHandle<()>> = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let message = match serde_json::from_str::<Value>(&line) {
            Ok(message) => message,
            Err(error) => {
                send(
                    &out,
                    &failure(Value::Null, -32700, format!("Parse error: {error}")),
                );
                continue;
            }
        };
        let (context, out) = (context.clone(), out.clone());
        pending.retain(|handler| !handler.is_finished());
        pending.push(thread::spawn(move || {
            if let Some(reply) = handle(&context, &message) {
                send(&out, &reply);
            }
        }));
    }
    context.runs.stop_all();
    for handler in pending {
        let _ = handler.join();
    }
    context.runs.stop_all();
}

fn send(out: &Mutex<std::io::Stdout>, message: &Value) {
    let mut out = lock(out);
    let _ = writeln!(out, "{message}").and_then(|()| out.flush());
}

fn failure(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn handle(context: &Context, message: &Value) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message.get("method")?.as_str()?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call(context, &params),
        _ => Err((-32601, format!("Unknown method {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => failure(id, code, message),
    })
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|v| PROTOCOLS.contains(v))
        .unwrap_or(PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "volcren", "version": env!("VOLCREN_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn tools() -> Vec<Value> {
    let pid = json!({ "type": "integer", "description": "The client's pid, from clients." });
    let wait = |default: u64| json!({ "type": "integer", "minimum": 0, "maximum": MAX_WAIT, "default": default, "description": "Seconds to wait for the run before returning." });
    let source = json!({ "type": "string", "description": "Luau source. Give source or path." });
    let path = json!({ "type": "string", "description": "Absolute path of a .luau file. Give source or path." });
    vec![
        tool(
            "clients",
            "Lists the game clients on this machine: pid, window title, whether the agent is loaded and current, and the id of a run on it.",
            json!({}),
            &[],
        ),
        tool(
            "load_agent",
            "Injects the agent into a client, or reloads it when outdated. Needed before game_state or run_luau.",
            json!({ "pid": pid }),
            &["pid"],
        ),
        tool(
            "game_state",
            "Reads a client's game state: login state, position, skills, run energy, camera, and the inventory and equipment with item names.",
            json!({ "pid": pid }),
            &["pid"],
        ),
        tool(
            "run_luau",
            "Runs a Luau script on a client with the VOLCREN library and returns its output once it ends or the wait runs out. It keeps running after that; use run_output and stop_run.",
            json!({
                "pid": pid,
                "source": source,
                "path": path,
                "settings": { "type": "object", "description": "Values for the settings in a manifest.json beside `path`. Missing ones take the manifest's defaults." },
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid"],
        ),
        tool(
            "run_output",
            "Returns a run's status and output lines from `since` (the `next line` of the last call). Waits up to wait_seconds for new lines.",
            json!({
                "run_id": { "type": "integer" },
                "since": { "type": "integer", "minimum": 0, "default": 0 },
                "wait_seconds": wait(0),
            }),
            &["run_id"],
        ),
        tool(
            "stop_run",
            "Stops a run and returns its status. Output it printed before stopping stays readable with run_output.",
            json!({ "run_id": { "type": "integer" } }),
            &["run_id"],
        ),
        tool(
            "lib_docs",
            "Searches the VOLCREN library's docs and examples, or reads one page. With neither query nor page, lists all pages.",
            json!({
                "query": { "type": "string", "description": "Words to search for, like `bank deposit`." },
                "page": { "type": "string", "description": "A page path from a search or the list, like docs/main/walker.md. Library sources such as main/walker.luau can be read too." },
            }),
            &[],
        ),
        tool(
            "check_luau",
            "Checks a script with luau-analyze, resolving @lib to the library: syntax, type and lint problems. Published scripts check clean, so fix everything it reports.",
            json!({
                "source": source,
                "path": path,
            }),
            &[],
        ),
    ]
}

type Failure = (i64, String);

fn call(context: &Context, params: &Value) -> Result<Value, Failure> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match name {
        "clients" => clients(context),
        "load_agent" => load_agent(context, &args),
        "game_state" => game_state(context, &args),
        "run_luau" => run_luau(context, &args),
        "run_output" => run_output(context, &args),
        "stop_run" => stop_run(context, &args),
        "lib_docs" => lib_docs(context, &args),
        "check_luau" => check_luau(context, &args),
        _ => return Err((-32602, format!("Unknown tool {name}"))),
    };
    Ok(match result {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
        Err(error) => json!({ "content": [{ "type": "text", "text": error }], "isError": true }),
    })
}

fn integer(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a whole number.")),
    }
}

fn text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a string.")),
    }
}

fn pid(args: &Value) -> Result<u32, String> {
    integer(args, "pid")?
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or_else(|| "pid is required.".into())
}

fn wait(args: &Value, default: u64) -> Result<Duration, String> {
    Ok(Duration::from_secs(
        integer(args, "wait_seconds")?
            .unwrap_or(default)
            .min(MAX_WAIT),
    ))
}

fn pretty(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

fn clients(context: &Context) -> Result<String, String> {
    let saved = saved();
    let scan = backend::scan(&saved.settings);
    let dll = saved.settings.dll_version().ok();
    let clients: Vec<Value> = scan
        .targets
        .iter()
        .map(|t| {
            let client = &t.client;
            let agent = match (&client.loaded, &client.version, &dll) {
                (None, _, _) => "not loaded",
                (Some(_), Some(version), Some(dll)) if version != dll => "outdated",
                (Some(_), _, _) => "loaded",
            };
            json!({
                "pid": client.pid,
                "title": client.title,
                "agent": agent,
                "supported": client.is_64bit && client.foreign_bot.is_none(),
                "run_id": context.runs.running_on(client.pid),
            })
        })
        .collect();
    pretty(&json!({ "clients": clients, "errors": scan.errors }))
}

fn load_agent(context: &Context, args: &Value) -> Result<String, String> {
    context.signed_in()?;
    let saved = saved();
    let target = target(&saved.settings, pid(args)?)?;
    if let Some(bot) = &target.client.foreign_bot {
        return Err(format!("Another bot ({bot}) is loaded in this client."));
    }
    if !target.client.is_64bit {
        return Err("This client is 32-bit, which isn't supported.".into());
    }
    let report = backend::load(&target, &saved.settings, &saved.cursor)?;
    if let Some(problem) = report.connection.as_ref().and_then(|c| c.problem()) {
        return Err(format!("The agent loaded but {problem}."));
    }
    pretty(&report)
}

fn game_state(context: &Context, args: &Value) -> Result<String, String> {
    context.signed_in()?;
    let settings = saved().settings;
    let target = loaded(&settings, pid(args)?)?;
    let state = backend::state(&target, &settings)?;
    let containers = backend::item_containers(&target, &settings)?;
    let wanted = |id: i32| {
        containers
            .iter()
            .find(|(container, _)| *container == id)
            .map(|(_, items)| items.clone())
            .unwrap_or_default()
    };
    let (inventory, equipment) = (wanted(INVENTORY), wanted(EQUIPMENT));
    let ids: Vec<i32> = inventory
        .iter()
        .chain(&equipment)
        .map(|item| item.id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let names: HashMap<i32, Option<String>> = backend::item_names(&target, &settings, &ids)?
        .into_iter()
        .zip(&ids)
        .map(|(name, id)| (*id, name))
        .collect();
    let named = |items: &[injector::Item]| -> Vec<Value> {
        items
            .iter()
            .map(|item| {
                json!({
                    "slot": item.slot,
                    "id": item.id,
                    "name": names.get(&item.id).cloned().flatten(),
                    "quantity": item.quantity,
                })
            })
            .collect()
    };
    pretty(&json!({
        "state": state,
        "inventory": named(&inventory),
        "equipment": named(&equipment),
    }))
}

fn snippet_path(workspace: &Path, pid: u32, source: &str) -> Result<PathBuf, String> {
    let path = workspace.join("runs").join(format!("{pid}.luau"));
    crate::paths::write_atomic(&path, source.as_bytes())
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(path)
}

fn run_luau(context: &Context, args: &Value) -> Result<String, String> {
    let pid = pid(args)?;
    let waiting = wait(args, RUN_WAIT)?;
    let api = context.signed_in()?;
    let saved = saved();
    let workspace = crate::workspace::prepare(&saved.settings)?;
    let (source, path) = match (text(args, "source")?, text(args, "path")?) {
        (Some(source), None) => (source.to_owned(), snippet_path(&workspace, pid, source)?),
        (None, Some(path)) => {
            let path = PathBuf::from(path);
            let source = std::fs::read_to_string(&path)
                .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
            (source, path)
        }
        _ => return Err("Give either source or path.".into()),
    };
    let launch = launch(args, text(args, "path")?.map(Path::new))?;
    if let Some(run) = context.runs.running_on(pid) {
        return Err(format!(
            "Run {run} is still running on client {pid}. Stop it first."
        ));
    }
    let target = loaded(&saved.settings, pid)?;
    let lib = context.lib(&saved.settings)?;
    let config = api.config();
    let host = Host {
        ctx: eframe::egui::Context::default(),
        accounts: Arc::new(RwLock::new(crate::accounts::Accounts::load())),
        whitelist: Arc::new(RwLock::new(crate::whitelist::Whitelist::new(
            saved.whitelist,
            config,
        ))),
        prices_url: config.prices_url.clone(),
    };
    let run = context.runs.start(runs::Start {
        api,
        host: &host,
        source: &source,
        path: &path,
        lib: &lib,
        target: &target,
        settings: &saved.settings,
        launch: &launch,
    })?;
    Ok(run.read(0, waiting, true, PAGE_LINES).text())
}

fn launch(args: &Value, path: Option<&Path>) -> Result<crate::script::Launch, String> {
    let manifest = match path {
        Some(path) => injector::manifest::Manifest::beside(path)?.unwrap_or_default(),
        None => injector::manifest::Manifest::default(),
    };
    let mut values = manifest.defaults();
    match args.get("settings") {
        None | Some(Value::Null) => {}
        Some(Value::Object(given)) => {
            if let Value::Object(values) = &mut values {
                values.extend(given.clone());
            }
        }
        Some(_) => return Err("settings must be an object.".into()),
    }
    Ok(crate::script::Launch::with(values))
}

fn run(context: &Context, args: &Value) -> Result<Arc<runs::Run>, String> {
    let id = integer(args, "run_id")?.ok_or("run_id is required.")?;
    context
        .runs
        .get(id)
        .ok_or_else(|| format!("There's no run {id}."))
}

fn run_output(context: &Context, args: &Value) -> Result<String, String> {
    let run = run(context, args)?;
    let since = integer(args, "since")?.unwrap_or(0);
    let since = usize::try_from(since).unwrap_or(usize::MAX);
    Ok(run.read(since, wait(args, 0)?, false, PAGE_LINES).text())
}

fn stop_run(context: &Context, args: &Value) -> Result<String, String> {
    let run = run(context, args)?;
    run.stop();
    Ok(run
        .read(run.next_line(), Duration::from_secs(5), true, 0)
        .text())
}

fn lib_docs(context: &Context, args: &Value) -> Result<String, String> {
    let lib = context.lib(&saved().settings)?;
    match (text(args, "page")?, text(args, "query")?) {
        (Some(page), _) => library::page(&lib, page),
        (None, Some(query)) => library::search(&lib, query),
        (None, None) => library::index(&lib),
    }
}

fn check_luau(context: &Context, args: &Value) -> Result<String, String> {
    let settings = saved().settings;
    context.lib(&settings)?;
    let workspace = crate::workspace::prepare(&settings)?;
    let draft = match (text(args, "source")?, text(args, "path")?) {
        (Some(source), None) => library::Draft::Source(source),
        (None, Some(path)) => library::Draft::File(Path::new(path)),
        _ => return Err("Give either source or path.".into()),
    };
    library::check(&workspace, draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            api: Api::new(Config::default()),
            signed_in: Mutex::new(false),
            lib: Mutex::new(None),
            runs: runs::Runs::default(),
        }
    }

    fn ask(message: Value) -> Option<Value> {
        handle(&context(), &message)
    }

    #[test]
    fn answers_requests_and_ignores_notifications() {
        let init = ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let unknown_version = ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } })).unwrap();
        assert_eq!(unknown_version["result"]["protocolVersion"], PROTOCOLS[0]);
        assert!(ask(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let missing =
            ask(json!({ "jsonrpc": "2.0", "id": "a", "method": "resources/list" })).unwrap();
        assert_eq!(missing["error"]["code"], -32601);
        assert_eq!(missing["id"], "a");
        let names: Vec<String> = ask(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert!(names.contains(&"run_luau".to_owned()));
    }

    #[test]
    fn bad_tool_arguments_come_back_as_tool_errors() {
        let call = |name: &str, arguments: Value| {
            ask(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": name, "arguments": arguments } })).unwrap()
        };
        assert_eq!(call("nope", json!({}))["error"]["code"], -32602);
        let no_run = call("run_output", json!({ "run_id": 9 }));
        assert_eq!(no_run["result"]["isError"], true);
        assert_eq!(no_run["result"]["content"][0]["text"], "There's no run 9.");
        let bad = call("stop_run", json!({ "run_id": "x" }));
        assert_eq!(
            bad["result"]["content"][0]["text"],
            "run_id must be a whole number."
        );
    }
}
