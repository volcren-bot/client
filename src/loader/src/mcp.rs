mod library;
mod look;
mod runs;

use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
    thread,
    time::Duration,
};

use injector::Cursor;
use loader::backend::{self, Settings, Target};
use serde_json::{Value, json};

use crate::api::{Api, Config};
use crate::script::{Host, lock};

const PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const RUN_WAIT: u64 = 30;
const MAX_WAIT: u64 = 120;
const PAGE_LINES: usize = 300;
const SIGN_IN: &str =
    "The loader isn't signed in. Open the loader and sign in once; `loader mcp` uses that sign-in.";

const INSTRUCTIONS: &str = "VOLCREN automates a popular MMORPG's game client through the user's desktop loader. You act in the game by writing short Luau scripts against the VOLCREN SDK and running them with run_luau, never by sending clicks yourself.

Workflow:
1. clients lists the game clients. load_agent injects the agent into one that isn't loaded or is outdated.
2. game_state reads the game. Ask only for the sections you need (include), and narrow NPCs, objects and items with name and radius.
3. For a single action, use the built-in actions: they act at once, with no script to write. interact picks an option on an NPC, object, ground item or inventory item; use_item_on uses an inventory item on one; talk talks to an NPC and returns the dialogue, so use it at once for a random event NPC interacting with the player (they leave within about a minute); open_bank opens the nearest bank; travel walks or teleports to a tile, and travel_plan shows how it would get there without moving.
4. For anything longer, write a script. Before using an SDK module, find it with sdk_docs (search, then read the page). Don't guess function names or signatures.
5. check_luau a script before running it.
6. run_luau runs it on one client and returns its output. A run that outlasts the wait keeps going: read it with run_output (match filters its lines) and end it with stop_run. One script or action runs per client at a time.

Scripts:
- Start with --!strict and require SDK modules by name: docs/walker.md documents require(\"@volcren/walker\"), docs/host.md documents require(\"@volcren/host\").
- Prefer the SDK's high-level functions (walking, interacting, inventory, bank, interfaces) to raw Host input. They move the mouse, time actions and handle failures like a person, which matters more than speed.
- print() output comes back to you. Keep scripts small, check the result, then continue.
- Bank PINs: never type a PIN or click the keypad. The loader keeps each account's PIN (its Accounts page) and enters it for any run: Bankmain.open and open_bank enter it when the keypad shows, and BankpinKeypad.enter does it for a keypad already open. If it says no PIN is saved, ask the user to save it there.
- Travel: Walker.walk (and the travel action) picks teleports, ships, doors and shortcuts the account can use and the user hasn't turned off on the account's Transportation tab in the loader. Plan first with travel_plan, or Transports.choose and Walker.plan in a script.
- Runs act on the user's real account. Ask the user before anything that spends coins, trades, uses the Grand Exchange, drops or alchs valuable items, or is otherwise risky. Stop runs you no longer need; all runs stop when you exit.";

struct Saved {
    settings: Settings,
    cursor: Cursor,
    whitelist: Vec<String>,
}

fn saved() -> Saved {
    let storage = crate::storage::Storage::open();
    let mut settings: Settings = storage
        .get(crate::storage::SETTINGS_KEY)
        .unwrap_or_default();
    settings.fill_missing();
    Saved {
        settings,
        cursor: storage
            .get(crate::app::CURSOR_KEY)
            .unwrap_or(crate::app::DEFAULT_CURSOR),
        whitelist: storage
            .get(crate::app::HTTP_WHITELIST_KEY)
            .unwrap_or_default(),
    }
}

struct Context {
    api: Api,
    signed_in: Mutex<bool>,
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
    let kind = json!({ "type": "string", "enum": ["npc", "object", "ground_item", "item"], "description": "What the target is: an NPC, an object, an item on the ground, or an item in the inventory." });
    let target = json!({ "type": "string", "description": "The target's name or id. The nearest match is used." });
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
            "Reads a client's game state, also while a run is going. Returns only the sections in `include`. NPCs, players, objects and ground items are the nearest within `radius`, at most `limit` each, with the options they offer; chat is the last `limit` messages.",
            json!({
                "pid": pid,
                "include": {
                    "type": "array",
                    "items": { "type": "string", "enum": look::SECTIONS },
                    "default": look::DEFAULT_SECTIONS,
                    "description": "player: login state, position, run energy, weight, camera, destination. skills, inventory, equipment. npcs, players, objects, ground_items: what is near. interfaces: the open interface groups. chat: recent messages."
                },
                "name": { "type": "string", "description": "Keep only NPCs, players, objects and ground items whose name contains this, or with this id; for chat, messages containing it." },
                "radius": { "type": "integer", "minimum": 0, "maximum": 104, "default": look::RADIUS, "description": "Tiles from the player." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": look::LIMIT, "description": "Most entries per section." },
            }),
            &["pid"],
        ),
        tool(
            "run_luau",
            "Runs a Luau script on a client with the VOLCREN SDK and returns its output once it ends or the wait runs out. It keeps running after that; use run_output and stop_run.",
            json!({
                "pid": pid,
                "source": source,
                "path": path,
                "settings": { "type": "object", "description": "Values for the settings in a manifest.json beside `path`. Missing ones take the manifest's defaults." },
                "debug": { "type": "boolean", "description": "Turn script debugging on or off for this run: SDK debug logging and debug drawings. Defaults to the loader's setting." },
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid"],
        ),
        tool(
            "interact",
            "Picks a menu option on the nearest matching NPC, object, ground item or inventory item at once, walking there if needed, like a person would. Returns what it found, whether the click went through, where the player ended up and any dialogue that opened.",
            json!({
                "pid": pid,
                "kind": kind,
                "target": target,
                "option": { "type": "string", "description": "The menu option, like Talk-to, Bank, Chop down, Take or Eat." },
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid", "kind", "target", "option"],
        ),
        tool(
            "use_item_on",
            "Uses an inventory item on the nearest matching NPC, object, ground item or inventory item at once, like a key on a door or a tinderbox on logs.",
            json!({
                "pid": pid,
                "item": { "type": "string", "description": "The inventory item's name or id." },
                "kind": kind,
                "target": target,
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid", "item", "kind", "target"],
        ),
        tool(
            "talk",
            "Talks to an NPC at once and returns the dialogue: every line, then the options it stops at. Without `npc` it picks the NPC interacting with you, such as a random event, which doesn't wait long. `choose` picks options in order as they appear.",
            json!({
                "pid": pid,
                "npc": { "type": "string", "description": "The NPC's name or id. Leave it out for the NPC interacting with you." },
                "choose": { "type": "array", "items": { "type": "string" }, "description": "Option texts to pick, in order." },
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid"],
        ),
        tool(
            "open_bank",
            "Opens the nearest bank, travelling to one when none is near, and enters the account's bank PIN that the loader keeps. Says when no PIN is saved.",
            json!({ "pid": pid, "wait_seconds": wait(MAX_WAIT) }),
            &["pid"],
        ),
        tool(
            "travel",
            "Goes to a tile, using the teleports, ships, doors and shortcuts the account can use, and returns where the player ended up. A long trip outlasts the wait: follow it with run_output.",
            json!({
                "pid": pid,
                "x": { "type": "integer" },
                "y": { "type": "integer" },
                "plane": { "type": "integer", "description": "Defaults to the player's." },
                "near": { "type": "integer", "minimum": 0, "default": 2, "description": "Done within this many tiles." },
                "wait_seconds": wait(MAX_WAIT),
            }),
            &["pid", "x", "y"],
        ),
        tool(
            "travel_plan",
            "Shows how travel would reach a tile without moving: each transport with where it leaves and lands and what it needs, then the walk with its doors and climbs, and the estimated ticks.",
            json!({
                "pid": pid,
                "x": { "type": "integer" },
                "y": { "type": "integer" },
                "plane": { "type": "integer", "description": "Defaults to the player's." },
                "near": { "type": "integer", "minimum": 0, "default": 2 },
                "wait_seconds": wait(RUN_WAIT),
            }),
            &["pid", "x", "y"],
        ),
        tool(
            "run_output",
            "Returns a run's status and output lines from `since` (the `next line` of the last call), and the path of its full log. Waits up to wait_seconds for a new line, or with `match` for a new matching one.",
            json!({
                "run_id": { "type": "integer" },
                "since": { "type": "integer", "minimum": 0, "default": 0 },
                "match": { "type": "string", "description": "Return only lines containing this text, ignoring case, each with its line number." },
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
            "sdk_docs",
            "Searches the VOLCREN SDK's docs and examples, or reads one page. With neither query nor page, lists all pages.",
            json!({
                "query": { "type": "string", "description": "Words to search for, like `bank deposit`." },
                "page": { "type": "string", "description": "A page path from a search or the list, like docs/walker.md. SDK sources such as modules/walker.luau can be read too." },
            }),
            &[],
        ),
        tool(
            "check_luau",
            "Checks a script with luau-analyze, resolving @volcren to the SDK: syntax, type and lint problems. Published scripts check clean, so fix everything it reports.",
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
        "interact" => interact(context, &args, false),
        "use_item_on" => interact(context, &args, true),
        "talk" => talk(context, &args),
        "open_bank" => builtin(context, &args, MAX_WAIT, OPEN_BANK, "open bank", json!({})),
        "travel" => travel(context, &args, false),
        "travel_plan" => travel(context, &args, true),
        "run_output" => run_output(context, &args),
        "stop_run" => stop_run(context, &args),
        "sdk_docs" => sdk_docs(&args),
        "check_luau" => check_luau(&args),
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
    let clients: Vec<Value> = scan
        .targets
        .iter()
        .map(|t| {
            let client = &t.client;
            let agent = match (&client.loaded, &client.version, &scan.agent_version) {
                (None, _, _) => "not loaded",
                (Some(_), Some(version), Some(embedded)) if version != embedded => "outdated",
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
    let sections = match args.get("include") {
        None | Some(Value::Null) => look::DEFAULT_SECTIONS.map(String::from).to_vec(),
        Some(Value::Array(sections)) => sections
            .iter()
            .map(|section| match section.as_str() {
                Some(name) if look::SECTIONS.contains(&name) => Ok(name.to_owned()),
                _ => Err(format!(
                    "include takes {}; got {section}.",
                    look::SECTIONS.join(", ")
                )),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("include must be a list of section names.".into()),
    };
    let look = look::Look {
        sections,
        name: text(args, "name")?
            .map(|name| name.trim().to_lowercase())
            .filter(|name| !name.is_empty()),
        radius: integer(args, "radius")?
            .map_or(look::RADIUS, |radius| i32::try_from(radius.min(104)).unwrap_or(104)),
        limit: integer(args, "limit")?
            .map_or(look::LIMIT, |limit| usize::try_from(limit.clamp(1, 200)).unwrap_or(200)),
    };
    context.signed_in()?;
    let settings = saved().settings;
    let target = loaded(&settings, pid(args)?)?;
    pretty(&look::read(&target, &settings, &look)?)
}

fn snippet_path(workspace: &Path, pid: u32, source: &str) -> Result<PathBuf, String> {
    let path = workspace.join("runs").join(format!("{pid}.luau"));
    crate::paths::write_atomic(&path, source.as_bytes())
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    Ok(path)
}

const TALK: &str = include_str!("mcp/talk.luau");
const INTERACT: &str = include_str!("mcp/interact.luau");
const OPEN_BANK: &str = include_str!("mcp/open_bank.luau");
const TRAVEL: &str = include_str!("mcp/travel.luau");

fn builtin(
    context: &Context,
    args: &Value,
    default_wait: u64,
    source: &str,
    title: &str,
    values: Value,
) -> Result<String, String> {
    let pid = pid(args)?;
    let waiting = wait(args, default_wait)?;
    let saved = saved();
    let workspace = crate::workspace::prepare(&saved.settings)?;
    let path = snippet_path(&workspace, pid, source)?;
    let launch = crate::script::Launch::with(values);
    start(context, pid, waiting, &saved, source, &path, title, &launch)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    text(args, key)?
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key} is required."))
}

fn talk(context: &Context, args: &Value) -> Result<String, String> {
    let choose = match args.get("choose") {
        None | Some(Value::Null) => json!([]),
        Some(Value::Array(options)) if options.iter().all(Value::is_string) => json!(options),
        Some(_) => return Err("choose must be a list of option texts.".into()),
    };
    let values = json!({ "npc": text(args, "npc")?, "choose": choose });
    builtin(context, args, RUN_WAIT, TALK, "talk", values)
}

fn interact(context: &Context, args: &Value, using: bool) -> Result<String, String> {
    let kind = required(args, "kind")?;
    if !["npc", "object", "ground_item", "item"].contains(&kind) {
        return Err("kind must be npc, object, ground_item or item.".into());
    }
    let mut values = json!({ "kind": kind, "target": required(args, "target")? });
    if using {
        values["item"] = json!(required(args, "item")?);
    } else {
        values["option"] = json!(required(args, "option")?);
    }
    let title = if using { "use item on" } else { "interact" };
    builtin(context, args, RUN_WAIT, INTERACT, title, values)
}

fn coordinate(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a whole number.")),
    }
}

fn travel(context: &Context, args: &Value, plan: bool) -> Result<String, String> {
    let at = |key: &str| coordinate(args, key)?.ok_or_else(|| format!("{key} is required."));
    let values = json!({
        "x": at("x")?,
        "y": at("y")?,
        "plane": coordinate(args, "plane")?,
        "near": integer(args, "near")?,
        "plan": plan,
    });
    let (title, default_wait) = if plan {
        ("travel plan", RUN_WAIT)
    } else {
        ("travel", MAX_WAIT)
    };
    builtin(context, args, default_wait, TRAVEL, title, values)
}

fn run_luau(context: &Context, args: &Value) -> Result<String, String> {
    let pid = pid(args)?;
    let waiting = wait(args, RUN_WAIT)?;
    let mut saved = saved();
    match args.get("debug") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(on)) => saved.settings.script_debugging = *on,
        Some(_) => return Err("debug must be true or false.".into()),
    }
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
    let title = match text(args, "path")? {
        Some(_) => path.file_name().map_or_else(
            || "agent file".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        ),
        None => "agent snippet".to_owned(),
    };
    start(
        context, pid, waiting, &saved, &source, &path, &title, &launch,
    )
}

#[allow(clippy::too_many_arguments)]
fn start(
    context: &Context,
    pid: u32,
    waiting: Duration,
    saved: &Saved,
    source: &str,
    path: &Path,
    title: &str,
    launch: &crate::script::Launch,
) -> Result<String, String> {
    let api = context.signed_in()?;
    if let Some(run) = context.runs.running_on(pid) {
        return Err(format!(
            "Run {run} is still running on client {pid}. Stop it first."
        ));
    }
    let target = loaded(&saved.settings, pid)?;
    let sdk = crate::sdk::custom(&saved.settings)?;
    let config = api.config();
    let host = Host {
        wake: crate::wake::Wake::default(),
        accounts: Arc::new(RwLock::new(crate::accounts::Accounts::load())),
        whitelist: Arc::new(RwLock::new(crate::whitelist::Whitelist::new(
            saved.whitelist.clone(),
            config,
        ))),
        prices_url: config.prices_url.clone(),
    };
    let run = context.runs.start(runs::Start {
        title,
        api,
        host: &host,
        source,
        path,
        sdk: sdk.as_deref(),
        target: &target,
        settings: &saved.settings,
        launch,
    })?;
    Ok(run
        .read(&runs::Read {
            since: 0,
            wait: waiting,
            until_finished: true,
            most: PAGE_LINES,
            filter: None,
        })
        .text())
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
    let filter = text(args, "match")?.and_then(runs::Filter::new);
    Ok(run
        .read(&runs::Read {
            since: usize::try_from(since).unwrap_or(usize::MAX),
            wait: wait(args, 0)?,
            until_finished: false,
            most: PAGE_LINES,
            filter: filter.as_ref(),
        })
        .text())
}

fn stop_run(context: &Context, args: &Value) -> Result<String, String> {
    let run = run(context, args)?;
    run.stop();
    Ok(run
        .read(&runs::Read {
            since: run.next_line(),
            wait: Duration::from_secs(5),
            until_finished: true,
            most: 0,
            filter: None,
        })
        .text())
}

fn sdk_docs(args: &Value) -> Result<String, String> {
    let sdk = crate::sdk::dir(&saved().settings)?;
    match (text(args, "page")?, text(args, "query")?) {
        (Some(page), _) => library::page(&sdk, page),
        (None, Some(query)) => library::search(&sdk, query),
        (None, None) => library::index(&sdk),
    }
}

fn check_luau(args: &Value) -> Result<String, String> {
    let settings = saved().settings;
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
        for name in [
            "run_luau",
            "talk",
            "interact",
            "use_item_on",
            "open_bank",
            "travel",
            "travel_plan",
        ] {
            assert!(names.contains(&name.to_owned()), "{name}");
        }
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
        let section = call("game_state", json!({ "pid": 1, "include": ["npcs", "bank"] }));
        assert_eq!(section["result"]["isError"], true);
        assert!(
            section["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("include takes player, skills")
        );
        let kind = call(
            "interact",
            json!({ "pid": 1, "kind": "tree", "target": "Oak", "option": "Chop down" }),
        );
        assert_eq!(
            kind["result"]["content"][0]["text"],
            "kind must be npc, object, ground_item or item."
        );
        let bad = call("stop_run", json!({ "run_id": "x" }));
        assert_eq!(
            bad["result"]["content"][0]["text"],
            "run_id must be a whole number."
        );
    }
}
