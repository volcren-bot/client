use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const SCRIPT_PREFIX: &str = "\u{1e}SCRIPT ";
/// Optional, before the script: a JSON object of the script's other files by name, which the injector keeps in memory.
pub const MODULES_PREFIX: &str = "\u{1e}MODULES ";
/// Optional, the first line: the loader answers `PIN_REQUEST_PREFIX` requests, so `bank_pin_enter()` may ask it.
pub const PINS_PREFIX: &str = "\u{1e}PINS";
/// A line the injector writes for `bank_pin_enter()` with the account name hash; the loader answers on stdin with `pin_reply_line`.
pub const PIN_REQUEST_PREFIX: &str = "\u{1e}PINREQ ";
/// A line the injector writes for `lamp_skill()` with the account name hash; the loader answers on stdin with `lamp_reply_line`. Goes with `PINS_PREFIX`.
pub const LAMP_REQUEST_PREFIX: &str = "\u{1e}LAMPREQ ";
pub const FORM_PREFIX: &str = "\u{1e}FORM ";
/// A line the loader writes to stdin when the user presses Update on the settings, with the form's values as JSON.
pub const UPDATE_PREFIX: &str = "\u{1e}UPDATE ";
/// A line a script writes with `report_stats`: the loader sums these between heartbeats and sends them to the API.
pub const STATS_PREFIX: &str = "\u{1e}STATS ";

/// A line a script writes with `report_breaks`: the breaks and sleeps it has scheduled, for the loader's Breaks tab.
pub const BREAKS_PREFIX: &str = "\u{1e}BREAKS ";

/// A scheduled break or sleep. Times are milliseconds, counted from when the line was written.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlannedBreak {
    /// How long until it is taken.
    pub remaining: f64,
    /// About how long it lasts.
    pub length: f64,
    /// For a sleep, the time of day it is scheduled to, in seconds since local midnight.
    #[serde(default)]
    pub at: Option<f64>,
    /// Its position in the config, which "Skip next" names it by.
    #[serde(default)]
    pub index: u32,
}

/// The break or sleep being taken right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveBreak {
    /// `"break"` or `"sleep"`.
    pub kind: String,
    /// How long until it is over.
    pub remaining: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BreakPlan {
    #[serde(default)]
    pub breaks: Vec<PlannedBreak>,
    #[serde(default)]
    pub sleeps: Vec<PlannedBreak>,
    #[serde(default)]
    pub active: Option<ActiveBreak>,
}

pub fn breaks_line(plan: &BreakPlan) -> String {
    format!(
        "{BREAKS_PREFIX}{}",
        serde_json::to_string(plan).unwrap_or_default()
    )
}

/// The plan of a `BREAKS_PREFIX` line, without the prefix.
pub fn parse_breaks(payload: &str) -> Option<BreakPlan> {
    serde_json::from_str(payload).ok()
}

/// A line a script writes with `report_warning`: text the loader shows in its log in the warning color.
pub const WARN_PREFIX: &str = "\u{1e}WARN ";

/// A line a script writes with `report_counters`: what it counts, for the loader's XP Tracker tab.
pub const COUNTERS_PREFIX: &str = "\u{1e}COUNTERS ";

fn yes() -> bool {
    true
}

/// Something a script counts, with its total since the script started. The loader works out the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counter {
    /// What other counters refer to it by.
    pub id: String,
    pub label: String,
    pub value: f64,
    /// Whether to show it per hour of the run (on unless set to false).
    #[serde(default = "yes")]
    pub hourly: bool,
    /// The `id` of a counter to divide by, shown as "per" it, such as points per game.
    #[serde(default)]
    pub per: Option<String>,
    /// The `id` of a counter this is a share of, shown as a percent, such as wins of games.
    #[serde(default)]
    pub percent_of: Option<String>,
}

pub fn counters_line(counters: &[Counter]) -> String {
    format!(
        "{COUNTERS_PREFIX}{}",
        serde_json::to_string(counters).unwrap_or_default()
    )
}

/// The counters of a `COUNTERS_PREFIX` line, without the prefix.
pub fn parse_counters(payload: &str) -> Option<Vec<Counter>> {
    serde_json::from_str(payload).ok()
}

static UPDATE: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

/// Keeps `values` for the script to pick up, replacing an update it hasn't picked up yet.
pub fn push_update(values: Value) {
    *UPDATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(values);
}

/// The latest values the user updated the settings to since the last call, if any.
pub fn take_update() -> Option<Value> {
    UPDATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// The line that tells a running script its settings changed.
pub fn update_line(values: &Value) -> String {
    format!("{UPDATE_PREFIX}{values}")
}

/// The values of an `UPDATE_PREFIX` line, without the prefix.
pub fn parse_update(payload: &str) -> Option<Value> {
    serde_json::from_str::<Value>(payload.trim())
        .ok()
        .filter(Value::is_object)
}

pub type FormHandler = Box<dyn FnMut(&Value) -> Result<Option<Value>, String>>;

pub const KINDS: [&str; 9] = [
    "label",
    "check",
    "number",
    "text",
    "choice",
    "list",
    "equipment",
    "inventory",
    "prayer",
];
/// The kinds whose value is a layout the loader keeps per script: the script gets the chosen one, or nil.
pub const LAYOUT_KINDS: [&str; 3] = ["equipment", "inventory", "prayer"];

/// The worn items slots an `equipment` field's `required` items may name: the container's, then the secondary ammo (14).
pub const EQUIPMENT_SLOTS: [usize; 12] = [0, 1, 2, 3, 4, 5, 7, 9, 10, 12, 13, 14];
pub const INVENTORY_SLOTS: usize = 28;

/// An item a layout field's `required` list puts in every layout of that kind, which the user can move but not remove or change:
/// the ids that count as it (the first is shown and taken first), its slot (an equipment field's must have one; an inventory's is
/// where it starts), the amount range of a stack (both 0: any amount) and the doses or charges that count, in the order to take them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Required {
    pub ids: Vec<i32>,
    pub slot: Option<usize>,
    pub min: i32,
    pub max: i32,
    pub doses: Vec<u32>,
}

fn parse_required(kind: &str, row: &Value) -> Result<Required, String> {
    let int = |key: &str| -> Result<Option<i64>, String> {
        match row.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_i64()
                .map(Some)
                .ok_or_else(|| format!("`{key}` is not a whole number")),
        }
    };
    let list = |key: &str| -> Result<Vec<i64>, String> {
        match row.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(values)) => values
                .iter()
                .map(|v| {
                    v.as_i64()
                        .ok_or_else(|| format!("`{key}` holds a non-number"))
                })
                .collect(),
            Some(_) => Err(format!("`{key}` is not a list")),
        }
    };
    let ids = list("ids")?;
    if ids.is_empty()
        || ids
            .iter()
            .any(|&id| !(0..=i64::from(i32::MAX)).contains(&id))
    {
        return Err("`ids` needs one or more item ids".into());
    }
    let slot = int("slot")?;
    let slot = match (kind, slot) {
        ("equipment", None) => return Err("an equipment item needs a `slot`".into()),
        ("equipment", Some(s)) if !EQUIPMENT_SLOTS.iter().any(|&e| e as i64 == s) => {
            return Err(format!("{s} is not a worn items slot"));
        }
        (_, Some(s)) if !(0..INVENTORY_SLOTS as i64).contains(&s) => {
            return Err(format!("slot {s} is not 0 to {}", INVENTORY_SLOTS - 1));
        }
        (_, s) => s.map(|s| s as usize),
    };
    let (min, max) = (int("min")?.unwrap_or(0), int("max")?.unwrap_or(0));
    if min < 0
        || max < 0
        || max > i64::from(i32::MAX)
        || (max > 0 && min > max)
        || (max == 0 && min > 0)
    {
        return Err(
            "`min` and `max` are 0 to any amount with `min` at most `max`, or both 0".into(),
        );
    }
    let doses = list("doses")?;
    if doses
        .iter()
        .any(|&d| !(1..=i64::from(u32::MAX)).contains(&d))
    {
        return Err("`doses` are numbers from 1".into());
    }
    let mut seen = doses.clone();
    seen.sort_unstable();
    seen.dedup();
    if seen.len() != doses.len() {
        return Err("`doses` lists a dose twice".into());
    }
    Ok(Required {
        ids: ids.into_iter().map(|id| id as i32).collect(),
        slot,
        min: min as i32,
        max: max as i32,
        doses: doses.into_iter().map(|d| d as u32).collect(),
    })
}

fn check_required(kind: &str, field: &Value) -> Result<Vec<Required>, String> {
    let rows = match field.get("required") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(rows)) => rows,
        Some(_) => return Err("`required` is not a list".into()),
    };
    let items = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            parse_required(kind, row).map_err(|e| format!("required item {}: {e}", i + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut slots: Vec<usize> = items.iter().filter_map(|r| r.slot).collect();
    let named = slots.len();
    slots.sort_unstable();
    slots.dedup();
    if slots.len() != named {
        return Err("two required items name the same slot".into());
    }
    if kind == "inventory" && items.len() > INVENTORY_SLOTS {
        return Err(format!("more than {INVENTORY_SLOTS} required items"));
    }
    Ok(items)
}

/// The items a layout field puts in every layout (`required`); a list that doesn't check out (see `validate`) is none.
pub fn required(field: &Value) -> Vec<Required> {
    check_required(kind(field), field).unwrap_or_default()
}

fn check_locked(field: &Value) -> Result<Vec<String>, String> {
    match field.get("locked") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(keys)) => keys
            .iter()
            .map(|k| {
                k.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "`locked` holds a non-string".to_owned())
            })
            .collect(),
        Some(_) => Err("`locked` is not a list of prayer keys".into()),
    }
}

/// The prayers a `prayer` field's script controls itself (`locked`, by key), which its layouts leave out.
pub fn locked(field: &Value) -> Vec<String> {
    check_locked(field).unwrap_or_default()
}

pub fn fields(spec: &Value) -> &[Value] {
    spec.get("fields")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub fn kind(field: &Value) -> &str {
    field.get("kind").and_then(Value::as_str).unwrap_or("text")
}

pub fn id(field: &Value) -> Option<&str> {
    field.get("id").and_then(Value::as_str)
}

pub fn options(field: &Value) -> Vec<String> {
    field
        .get("options")
        .and_then(Value::as_array)
        .map(|o| {
            o.iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A list's rows: `(id, label, ticked by default)`. A row is a table `{ id, label, default }`.
pub fn items(field: &Value) -> Vec<(String, String, bool)> {
    field
        .get("items")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let id = row.get("id").and_then(Value::as_str)?;
                    let label = row.get("label").and_then(Value::as_str).unwrap_or(id);
                    Some((
                        id.to_owned(),
                        label.to_owned(),
                        row.get("default").and_then(Value::as_bool).unwrap_or(false),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The id of the check that has to be ticked for the field to be usable, when it names one.
pub fn enabled_by(field: &Value) -> Option<&str> {
    field.get("enabled_by").and_then(Value::as_str)
}

/// The conditions that all have to hold for the field to be shown: `visible_when` is one `{ id, equals }` or a list of them, where `equals` is a value or a list of values.
pub fn visible_rules(field: &Value) -> Vec<(&str, &Value)> {
    fn rule(r: &Value) -> Option<(&str, &Value)> {
        Some((r.get("id")?.as_str()?, r.get("equals")?))
    }
    match field.get("visible_when") {
        Some(Value::Array(rules)) => rules.iter().filter_map(rule).collect(),
        Some(one) => rule(one).into_iter().collect(),
        None => Vec::new(),
    }
}

/// Whether `value` is the `equals` of a `visible_when`, or one of them when it is a list.
pub fn matches(value: &Value, equals: &Value) -> bool {
    match equals {
        Value::Array(any) => any.iter().any(|e| e == value),
        other => other == value,
    }
}

pub fn validate(spec: &Value) -> Result<(), String> {
    if !spec.is_object() {
        return Err("form() takes a table: { title = ..., fields = { ... } }".into());
    }
    let mut seen = Vec::new();
    for (i, field) in fields(spec).iter().enumerate() {
        let kind = kind(field);
        if !KINDS.contains(&kind) {
            return Err(format!(
                "field {}: unknown kind \"{kind}\" (one of {})",
                i + 1,
                KINDS.join(", ")
            ));
        }
        if kind == "label" {
            continue;
        }
        let Some(id) = id(field) else {
            return Err(format!("field {}: needs an id", i + 1));
        };
        if seen.contains(&id) {
            return Err(format!("field {}: the id \"{id}\" is used twice", i + 1));
        }
        seen.push(id);
        if kind == "choice" && options(field).is_empty() {
            return Err(format!("field \"{id}\": a choice needs options"));
        }
        if kind == "list" && items(field).is_empty() {
            return Err(format!(
                "field \"{id}\": a list needs items, each with an id"
            ));
        }
        if field.get("required").is_some() && !matches!(kind, "equipment" | "inventory") {
            return Err(format!(
                "field \"{id}\": only equipment and inventory fields take `required`"
            ));
        }
        if field.get("locked").is_some() && kind != "prayer" {
            return Err(format!("field \"{id}\": only prayer fields take `locked`"));
        }
        check_required(kind, field).map_err(|e| format!("field \"{id}\": {e}"))?;
        check_locked(field).map_err(|e| format!("field \"{id}\": {e}"))?;
    }
    Ok(())
}

pub fn default_value(field: &Value) -> Option<Value> {
    let given = field.get("default").cloned();
    match kind(field) {
        "label" => None,
        k if LAYOUT_KINDS.contains(&k) => None,
        "check" => Some(json!(given.and_then(|v| v.as_bool()).unwrap_or(false))),
        "number" => Some(
            given
                .filter(Value::is_number)
                .or_else(|| field.get("min").cloned().filter(Value::is_number))
                .unwrap_or(json!(0)),
        ),
        "choice" => {
            let options = options(field);
            let given = given
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|g| options.contains(g));
            Some(json!(
                given
                    .or_else(|| options.first().cloned())
                    .unwrap_or_default()
            ))
        }
        "list" => Some(Value::Object(
            items(field)
                .into_iter()
                .map(|(id, _, on)| (id, json!(on)))
                .collect(),
        )),
        _ => Some(json!(
            given
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        )),
    }
}

pub fn defaults(spec: &Value) -> Value {
    let mut values = Map::new();
    for field in fields(spec) {
        if let (Some(id), Some(value)) = (id(field), default_value(field)) {
            values.insert(id.to_owned(), value);
        }
    }
    Value::Object(values)
}

pub fn stats_line(experience: i64, gold: i64, custom: &[i64]) -> String {
    format!(
        "{STATS_PREFIX}{}",
        json!({ "experience": experience, "gold": gold, "custom": custom })
    )
}

/// The values of a `STATS_PREFIX` line, without the prefix.
pub fn parse_stats(payload: &str) -> Option<(i64, i64, Vec<i64>)> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let custom = value
        .get("custom")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |values| {
            values.iter().filter_map(Value::as_i64).collect()
        });
    Some((
        value.get("experience")?.as_i64()?,
        value.get("gold")?.as_i64()?,
        custom,
    ))
}

/// Ends every entry the script writes to stdout, so an entry can hold newlines.
pub const ENTRY_END: char = '\0';

pub fn request_line(spec: &Value) -> String {
    format!("{FORM_PREFIX}{spec}")
}

pub fn pin_request_line(hash: &str) -> String {
    format!("{PIN_REQUEST_PREFIX}{hash}")
}

/// The loader's answer to a PIN request: the PIN, `None` when no saved account has that hash, or why it can't tell.
pub fn pin_reply_line(answer: &Result<Option<String>, String>) -> String {
    match answer {
        Ok(Some(pin)) => json!({ "pin": pin }),
        Ok(None) => json!({ "none": true }),
        Err(error) => json!({ "error": error }),
    }
    .to_string()
}

pub fn lamp_request_line(hash: &str) -> String {
    format!("{LAMP_REQUEST_PREFIX}{hash}")
}

/// The loader's answer to a lamp request: the skill, `None` when the account has none set, or why it can't tell.
pub fn lamp_reply_line(answer: &Result<Option<String>, String>) -> String {
    match answer {
        Ok(Some(skill)) => json!({ "skill": skill }),
        Ok(None) => json!({ "none": true }),
        Err(error) => json!({ "error": error }),
    }
    .to_string()
}

pub fn parse_lamp_reply(line: &str) -> Result<Option<String>, String> {
    let reply: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("bad lamp reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(reply
        .get("skill")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

pub fn parse_pin_reply(line: &str) -> Result<Option<String>, String> {
    let reply: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("bad PIN reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(reply.get("pin").and_then(Value::as_str).map(str::to_owned))
}

pub fn parse_reply(line: &str) -> Result<Option<Value>, String> {
    let reply: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("bad form reply from the loader: {e}"))?;
    if reply.get("cancelled").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    reply
        .get("values")
        .cloned()
        .map(Some)
        .ok_or_else(|| "the loader's form reply has no values".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_are_kept_until_taken() {
        let line = update_line(&json!({ "bank": false }));
        let values = parse_update(line.strip_prefix(UPDATE_PREFIX).unwrap()).unwrap();
        assert!(parse_update("[1]").is_none());
        push_update(json!({ "bank": true }));
        push_update(values.clone());
        assert_eq!(take_update(), Some(values));
        assert_eq!(take_update(), None);
    }

    #[test]
    fn visibility_rules_match_a_value_or_a_list() {
        let field =
            json!({ "id": "ring", "visible_when": { "id": "collect", "equals": ["A", "B"] } });
        let rules = visible_rules(&field);
        let (id, equals) = rules[0];
        assert_eq!(id, "collect");
        assert!(matches(&json!("B"), equals));
        assert!(!matches(&json!("C"), equals));
        assert!(matches(&json!(true), &json!(true)));
        assert!(visible_rules(&json!({ "id": "x" })).is_empty());
        let both =
            json!({ "visible_when": [{ "id": "a", "equals": 1 }, { "id": "b", "equals": 2 }] });
        assert_eq!(visible_rules(&both).len(), 2);
    }

    #[test]
    fn lamp_replies_round_trip() {
        for answer in [
            Ok(Some("Slayer".to_owned())),
            Ok(None),
            Err("locked".to_owned()),
        ] {
            assert_eq!(parse_lamp_reply(&lamp_reply_line(&answer)), answer);
        }
        assert!(parse_lamp_reply("nonsense").is_err());
    }

    #[test]
    fn pin_replies_round_trip() {
        for answer in [
            Ok(Some("0123".to_owned())),
            Ok(None),
            Err("locked".to_owned()),
        ] {
            assert_eq!(parse_pin_reply(&pin_reply_line(&answer)), answer);
        }
        assert!(parse_pin_reply("nonsense").is_err());
    }

    #[test]
    fn checks_required_items_and_locked_prayers() {
        let field = |kind: &str, extra: Value| {
            let mut f = json!({ "id": "f", "kind": kind });
            f.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            json!({ "fields": [f] })
        };
        let ok = field(
            "inventory",
            json!({ "required": [
                { "ids": [2434], "doses": [4, 3] },
                { "ids": [563], "slot": 27, "min": 100, "max": 500 },
                { "ids": [995], "min": 0, "max": 0 }
            ] }),
        );
        assert_eq!(validate(&ok), Ok(()));
        let parsed = required(&fields(&ok)[0]);
        assert_eq!(parsed[0].doses, [4, 3]);
        assert_eq!(parsed[1].slot, Some(27));
        assert_eq!((parsed[1].min, parsed[1].max), (100, 500));
        let equipment = field(
            "equipment",
            json!({ "required": [{ "ids": [1712, 1710], "slot": 2 }] }),
        );
        assert_eq!(validate(&equipment), Ok(()));
        for bad in [
            field("equipment", json!({ "required": [{ "ids": [1712] }] })),
            field(
                "equipment",
                json!({ "required": [{ "ids": [1712], "slot": 6 }] }),
            ),
            field("inventory", json!({ "required": [{ "ids": [] }] })),
            field(
                "inventory",
                json!({ "required": [{ "ids": [1], "slot": 28 }] }),
            ),
            field(
                "inventory",
                json!({ "required": [{ "ids": [1], "min": 5, "max": 2 }] }),
            ),
            field(
                "inventory",
                json!({ "required": [{ "ids": [1], "min": 5 }] }),
            ),
            field(
                "inventory",
                json!({ "required": [{ "ids": [1], "doses": [4, 4] }] }),
            ),
            field(
                "inventory",
                json!({ "required": [{ "ids": [1], "slot": 3 }, { "ids": [2], "slot": 3 }] }),
            ),
            field("prayer", json!({ "required": [{ "ids": [1] }] })),
            field("inventory", json!({ "locked": ["piety"] })),
            field("prayer", json!({ "locked": "piety" })),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
        let prayer = field(
            "prayer",
            json!({ "locked": ["protect_from_melee", "piety"] }),
        );
        assert_eq!(validate(&prayer), Ok(()));
        assert_eq!(locked(&fields(&prayer)[0]), ["protect_from_melee", "piety"]);
    }

    #[test]
    fn checks_specs_and_fills_defaults() {
        let spec = json!({
            "title": "Fisher",
            "fields": [
                { "kind": "label", "text": "Pick a spot" },
                { "id": "spot", "kind": "choice", "options": ["Harpoon", "Cage"] },
                { "id": "bank", "kind": "check", "default": true },
                { "id": "hours", "kind": "number", "min": 1 },
                { "id": "note", "kind": "text" }
            ]
        });
        assert_eq!(validate(&spec), Ok(()));
        assert_eq!(
            defaults(&spec),
            json!({ "spot": "Harpoon", "bank": true, "hours": 1, "note": "" })
        );
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "slider" }] })).is_err());
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "choice" }] })).is_err());
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "list" }] })).is_err());
        let list = json!({ "id": "l", "kind": "list", "items": [{ "id": "x", "label": "X", "default": true }, { "id": "y" }] });
        assert_eq!(default_value(&list), Some(json!({ "x": true, "y": false })));
        assert!(validate(&json!({ "fields": [{ "id": "a" }, { "id": "a" }] })).is_err());
        let line = stats_line(10, -5, &[1, 2]);
        assert_eq!(
            parse_stats(line.strip_prefix(STATS_PREFIX).unwrap()),
            Some((10, -5, vec![1, 2]))
        );
        assert_eq!(parse_stats("nope"), None);
        let plan = BreakPlan {
            breaks: vec![PlannedBreak {
                remaining: 1000.0,
                length: 500.0,
                at: None,
                index: 0,
            }],
            sleeps: vec![PlannedBreak {
                remaining: 5000.0,
                length: 9000.0,
                at: Some(3600.0),
                index: 0,
            }],
            active: Some(ActiveBreak {
                kind: "break".into(),
                remaining: 20.0,
            }),
        };
        assert_eq!(
            parse_breaks(breaks_line(&plan).strip_prefix(BREAKS_PREFIX).unwrap()),
            Some(plan)
        );
        let counters = vec![
            Counter {
                id: "wins".into(),
                label: "Wins".into(),
                value: 3.0,
                hourly: true,
                per: None,
                percent_of: Some("games".into()),
            },
            Counter {
                id: "points".into(),
                label: "Points".into(),
                value: 9.5,
                hourly: false,
                per: Some("games".into()),
                percent_of: None,
            },
        ];
        assert_eq!(
            parse_counters(
                counters_line(&counters)
                    .strip_prefix(COUNTERS_PREFIX)
                    .unwrap()
            ),
            Some(counters)
        );
        assert!(parse_counters(r#"[{"id":"a","label":"A","value":1}]"#).unwrap()[0].hourly);
        assert_eq!(parse_counters("{}"), None);
        assert_eq!(parse_breaks("{}"), Some(BreakPlan::default()));
        assert_eq!(parse_reply("{\"cancelled\":true}"), Ok(None));
        assert_eq!(
            parse_reply("{\"values\":{\"a\":1}}"),
            Ok(Some(json!({ "a": 1 })))
        );
    }
}
