use serde_json::{Map, Value, json};

pub const SCRIPT_PREFIX: &str = "\u{1e}SCRIPT ";
pub const FORM_PREFIX: &str = "\u{1e}FORM ";

pub type FormHandler = Box<dyn FnMut(&Value) -> Result<Option<Value>, String>>;

pub const KINDS: [&str; 5] = ["label", "check", "number", "text", "choice"];

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

/// Ends every entry the script writes to stdout, so an entry can hold newlines.
pub const ENTRY_END: char = '\0';

pub fn request_line(spec: &Value) -> String {
    format!("{FORM_PREFIX}{spec}")
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
        assert!(validate(&json!({ "fields": [{ "id": "a" }, { "id": "a" }] })).is_err());
        assert_eq!(parse_reply("{\"cancelled\":true}"), Ok(None));
        assert_eq!(parse_reply("{\"values\":{\"a\":1}}"), Ok(Some(json!({ "a": 1 }))));
    }
}
