use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const SCRIPT_PREFIX: &str = "\u{1e}SCRIPT ";
/// Optional, before the script: a JSON object of the script's other files by name, which the injector keeps in memory.
pub const MODULES_PREFIX: &str = "\u{1e}MODULES ";
/// Optional, the first line: the loader answers `PIN_REQUEST_PREFIX` requests, so `bank_pin_enter()` may ask it.
pub const PINS_PREFIX: &str = "\u{1e}PINS";
/// A line the injector writes for `bank_pin_enter()`: the hash of the logged in account's name. The loader answers on
/// the script's stdin with `pin_reply_line`. The PIN goes no further than the injector's own code.
pub const PIN_REQUEST_PREFIX: &str = "\u{1e}PINREQ ";
pub const FORM_PREFIX: &str = "\u{1e}FORM ";
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
    format!("{BREAKS_PREFIX}{}", serde_json::to_string(plan).unwrap_or_default())
}

/// The plan of a `BREAKS_PREFIX` line, without the prefix.
pub fn parse_breaks(payload: &str) -> Option<BreakPlan> {
    serde_json::from_str(payload).ok()
}

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
    format!("{COUNTERS_PREFIX}{}", serde_json::to_string(counters).unwrap_or_default())
}

/// The counters of a `COUNTERS_PREFIX` line, without the prefix.
pub fn parse_counters(payload: &str) -> Option<Vec<Counter>> {
    serde_json::from_str(payload).ok()
}

pub type FormHandler = Box<dyn FnMut(&Value) -> Result<Option<Value>, String>>;

pub const KINDS: [&str; 6] = ["label", "check", "number", "text", "choice", "list"];

pub fn fields(spec: &Value) -> &[Value] {
    spec.get("fields").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
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
        .map(|o| o.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect())
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
                    Some((id.to_owned(), label.to_owned(), row.get("default").and_then(Value::as_bool).unwrap_or(false)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The id of the check that has to be ticked for the field to be usable, when it names one.
pub fn enabled_by(field: &Value) -> Option<&str> {
    field.get("enabled_by").and_then(Value::as_str)
}

pub fn validate(spec: &Value) -> Result<(), String> {
    if !spec.is_object() {
        return Err("form() takes a table: { title = ..., fields = { ... } }".into());
    }
    let mut seen = Vec::new();
    for (i, field) in fields(spec).iter().enumerate() {
        let kind = kind(field);
        if !KINDS.contains(&kind) {
            return Err(format!("field {}: unknown kind \"{kind}\" (one of {})", i + 1, KINDS.join(", ")));
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
            return Err(format!("field \"{id}\": a list needs items, each with an id"));
        }
    }
    Ok(())
}

pub fn default_value(field: &Value) -> Option<Value> {
    let given = field.get("default").cloned();
    match kind(field) {
        "label" => None,
        "check" => Some(json!(given.and_then(|v| v.as_bool()).unwrap_or(false))),
        "number" => Some(given.filter(Value::is_number).or_else(|| field.get("min").cloned().filter(Value::is_number)).unwrap_or(json!(0))),
        "choice" => {
            let options = options(field);
            let given = given.and_then(|v| v.as_str().map(str::to_owned)).filter(|g| options.contains(g));
            Some(json!(given.or_else(|| options.first().cloned()).unwrap_or_default()))
        }
        "list" => Some(Value::Object(items(field).into_iter().map(|(id, _, on)| (id, json!(on))).collect())),
        _ => Some(json!(given.and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default())),
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
    format!("{STATS_PREFIX}{}", json!({ "experience": experience, "gold": gold, "custom": custom }))
}

/// The values of a `STATS_PREFIX` line, without the prefix.
pub fn parse_stats(payload: &str) -> Option<(i64, i64, Vec<i64>)> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let custom = value.get("custom").and_then(Value::as_array).map_or_else(Vec::new, |values| values.iter().filter_map(Value::as_i64).collect());
    Some((value.get("experience")?.as_i64()?, value.get("gold")?.as_i64()?, custom))
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

pub fn parse_pin_reply(line: &str) -> Result<Option<String>, String> {
    let reply: Value = serde_json::from_str(line.trim()).map_err(|e| format!("bad PIN reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(reply.get("pin").and_then(Value::as_str).map(str::to_owned))
}

pub fn parse_reply(line: &str) -> Result<Option<Value>, String> {
    let reply: Value = serde_json::from_str(line.trim()).map_err(|e| format!("bad form reply from the loader: {e}"))?;
    if reply.get("cancelled").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    reply.get("values").cloned().map(Some).ok_or_else(|| "the loader's form reply has no values".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_replies_round_trip() {
        for answer in [Ok(Some("0123".to_owned())), Ok(None), Err("locked".to_owned())] {
            assert_eq!(parse_pin_reply(&pin_reply_line(&answer)), answer);
        }
        assert!(parse_pin_reply("nonsense").is_err());
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
        assert_eq!(defaults(&spec), json!({ "spot": "Harpoon", "bank": true, "hours": 1, "note": "" }));
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "slider" }] })).is_err());
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "choice" }] })).is_err());
        assert!(validate(&json!({ "fields": [{ "id": "a", "kind": "list" }] })).is_err());
        let list = json!({ "id": "l", "kind": "list", "items": [{ "id": "x", "label": "X", "default": true }, { "id": "y" }] });
        assert_eq!(default_value(&list), Some(json!({ "x": true, "y": false })));
        assert!(validate(&json!({ "fields": [{ "id": "a" }, { "id": "a" }] })).is_err());
        let line = stats_line(10, -5, &[1, 2]);
        assert_eq!(parse_stats(line.strip_prefix(STATS_PREFIX).unwrap()), Some((10, -5, vec![1, 2])));
        assert_eq!(parse_stats("nope"), None);
        let plan = BreakPlan {
            breaks: vec![PlannedBreak { remaining: 1000.0, length: 500.0, at: None }],
            sleeps: vec![PlannedBreak { remaining: 5000.0, length: 9000.0, at: Some(3600.0) }],
            active: Some(ActiveBreak { kind: "break".into(), remaining: 20.0 }),
        };
        assert_eq!(parse_breaks(breaks_line(&plan).strip_prefix(BREAKS_PREFIX).unwrap()), Some(plan));
        let counters = vec![
            Counter { id: "wins".into(), label: "Wins".into(), value: 3.0, hourly: true, per: None, percent_of: Some("games".into()) },
            Counter { id: "points".into(), label: "Points".into(), value: 9.5, hourly: false, per: Some("games".into()), percent_of: None },
        ];
        assert_eq!(parse_counters(counters_line(&counters).strip_prefix(COUNTERS_PREFIX).unwrap()), Some(counters));
        assert!(parse_counters(r#"[{"id":"a","label":"A","value":1}]"#).unwrap()[0].hourly);
        assert_eq!(parse_counters("{}"), None);
        assert_eq!(parse_breaks("{}"), Some(BreakPlan::default()));
        assert_eq!(parse_reply("{\"cancelled\":true}"), Ok(None));
        assert_eq!(parse_reply("{\"values\":{\"a\":1}}"), Ok(Some(json!({ "a": 1 }))));
    }
}
