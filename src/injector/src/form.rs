use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const SCRIPT_PREFIX: &str = "\u{1e}SCRIPT ";
pub const MODULES_PREFIX: &str = "\u{1e}MODULES ";
pub const PINS_PREFIX: &str = "\u{1e}PINS";
pub const PIN_REQUEST_PREFIX: &str = "\u{1e}PINREQ ";
pub const LAMP_REQUEST_PREFIX: &str = "\u{1e}LAMPREQ ";
pub const SETTINGS_PREFIX: &str = "\u{1e}SETTINGS ";
pub const FINISH_PREFIX: &str = "\u{1e}FINISH";
pub const EXIT_PREFIX: &str = "\u{1e}EXIT ";
pub const RESUME_PREFIX: &str = "\u{1e}RESUME ";
pub const UPDATE_PREFIX: &str = "\u{1e}UPDATE ";
pub const STATS_PREFIX: &str = "\u{1e}STATS ";
pub const DEBUG_PREFIX: &str = "\u{1e}DEBUG ";

pub const BREAKS_PREFIX: &str = "\u{1e}BREAKS ";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PlannedBreak {
    pub remaining: f64,
    pub length: f64,
    #[serde(default)]
    pub at: Option<f64>,
    #[serde(default)]
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveBreak {
    pub kind: String,
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

pub fn parse_breaks(payload: &str) -> Option<BreakPlan> {
    serde_json::from_str(payload).ok()
}

pub const WARN_PREFIX: &str = "\u{1e}WARN ";

pub const COUNTERS_PREFIX: &str = "\u{1e}COUNTERS ";

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counter {
    pub id: String,
    pub label: String,
    pub value: f64,
    #[serde(default = "yes")]
    pub hourly: bool,
    #[serde(default)]
    pub per: Option<String>,
    #[serde(default)]
    pub percent_of: Option<String>,
}

pub fn counters_line(counters: &[Counter]) -> String {
    format!(
        "{COUNTERS_PREFIX}{}",
        serde_json::to_string(counters).unwrap_or_default()
    )
}

pub fn parse_counters(payload: &str) -> Option<Vec<Counter>> {
    serde_json::from_str(payload).ok()
}

static UPDATE: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

pub fn push_update(values: Value) {
    *UPDATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(values);
}

pub fn take_update() -> Option<Value> {
    UPDATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

pub fn update_line(values: &Value) -> String {
    format!("{UPDATE_PREFIX}{values}")
}

pub fn parse_update(payload: &str) -> Option<Value> {
    serde_json::from_str::<Value>(payload.trim())
        .ok()
        .filter(Value::is_object)
}

static SETTINGS: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);

pub fn set_settings(values: Value) {
    *SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(values);
}

pub fn settings() -> Value {
    SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| json!({}))
}

pub fn settings_given() -> bool {
    SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
}

pub fn settings_line(values: &Value) -> String {
    format!("{SETTINGS_PREFIX}{values}")
}

pub fn parse_settings(payload: &str) -> Option<Value> {
    parse_update(payload)
}

static FINISH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DEBUGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn debug_line(on: bool) -> String {
    format!("{DEBUG_PREFIX}{}", u8::from(on))
}

pub fn parse_debug(payload: &str) -> Option<bool> {
    match payload.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

pub fn set_debugging(on: bool) {
    DEBUGGING.store(on, std::sync::atomic::Ordering::SeqCst);
}

pub fn script_debugging() -> bool {
    DEBUGGING.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn request_finish() {
    FINISH.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub fn finish_requested() -> bool {
    FINISH.load(std::sync::atomic::Ordering::SeqCst)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExitStatus {
    Done,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exit {
    pub status: ExitStatus,
    #[serde(default)]
    pub reason: Option<String>,
}

pub fn exit_line(exit: &Exit) -> String {
    format!(
        "{EXIT_PREFIX}{}",
        serde_json::to_string(exit).unwrap_or_default()
    )
}

pub fn parse_exit(payload: &str) -> Option<Exit> {
    serde_json::from_str(payload).ok()
}

static RESUME: std::sync::Mutex<Option<BreakPlan>> = std::sync::Mutex::new(None);

pub fn resume_line(plan: &BreakPlan) -> String {
    format!(
        "{RESUME_PREFIX}{}",
        serde_json::to_string(plan).unwrap_or_default()
    )
}

pub fn set_resume(plan: BreakPlan) {
    *RESUME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(plan);
}

pub fn take_resume() -> Option<BreakPlan> {
    RESUME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

pub const KINDS: [&str; 11] = [
    "label",
    "check",
    "number",
    "text",
    "choice",
    "list",
    "grid",
    "tasks",
    "equipment",
    "inventory",
    "prayer",
];
pub const LAYOUT_KINDS: [&str; 3] = ["equipment", "inventory", "prayer"];

pub const EQUIPMENT_SLOTS: [usize; 12] = [0, 1, 2, 3, 4, 5, 7, 9, 10, 12, 13, 14];
const INVENTORY_SLOTS: usize = 28;

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
    if has_repeats(doses.clone()) {
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

fn has_repeats<T: Ord>(mut values: Vec<T>) -> bool {
    let count = values.len();
    values.sort_unstable();
    values.dedup();
    values.len() != count
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
    if has_repeats(items.iter().filter_map(|r| r.slot).collect()) {
        return Err("two required items name the same slot".into());
    }
    if kind == "inventory" && items.len() > INVENTORY_SLOTS {
        return Err(format!("more than {INVENTORY_SLOTS} required items"));
    }
    Ok(items)
}

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

pub const GRID_COLUMNS: std::ops::RangeInclusive<usize> = 2..=5;
pub const GRID_COLORS: [u32; 5] = [0x9ece6a, 0xe0af68, 0xf7768e, 0x7aa2f7, 0xbb9af7];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridColumn {
    pub id: String,
    pub label: String,
    pub summary: String,
    pub color: Option<u32>,
    pub max: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridItem {
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
    pub column: usize,
    allowed: Vec<bool>,
}

impl GridItem {
    pub fn allows(&self, column: usize) -> bool {
        self.allowed.get(column).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub columns: Vec<GridColumn>,
    pub items: Vec<GridItem>,
    pub search: String,
    pub note: Option<String>,
}

fn text_of<'a>(row: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(format!("`{key}` is not text")),
    }
}

fn parse_color(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

impl Grid {
    pub fn parse(field: &Value) -> Result<Self, String> {
        let colored = match field.get("colors") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(on)) => *on,
            Some(_) => return Err("`colors` is true or false".into()),
        };
        let Some(columns) = field.get("columns").and_then(Value::as_array) else {
            return Err("a grid needs `columns`".into());
        };
        if !GRID_COLUMNS.contains(&columns.len()) {
            return Err(format!(
                "a grid has {} to {} columns",
                GRID_COLUMNS.start(),
                GRID_COLUMNS.end()
            ));
        }
        let columns = columns
            .iter()
            .enumerate()
            .map(|(i, column)| {
                Self::parse_column(column, colored, GRID_COLORS[i])
                    .map_err(|e| format!("column {}: {e}", i + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if has_repeats(columns.iter().map(|c| &c.id).collect()) {
            return Err("two columns share an id".into());
        }
        let column_of = |id: &str| columns.iter().position(|c| c.id == id);
        let fallback = match text_of(field, "default")? {
            None => 0,
            Some(id) => {
                column_of(id).ok_or_else(|| format!("`default` \"{id}\" is not a column"))?
            }
        };
        let Some(items) = field.get("items").and_then(Value::as_array) else {
            return Err("a grid needs `items`, each with an id".into());
        };
        let items = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let parsed = || -> Result<GridItem, String> {
                    let id = text_of(item, "id")?.ok_or("needs an id")?;
                    let allowed = Self::parse_allowed(item, columns.len(), column_of)?;
                    let column = match text_of(item, "default")? {
                        None if allowed[fallback] => fallback,
                        None => allowed.iter().position(|&a| a).unwrap_or(fallback),
                        Some(c) => {
                            let c = column_of(c)
                                .ok_or_else(|| format!("`default` \"{c}\" is not a column"))?;
                            if !allowed[c] {
                                return Err("`default` is not one of its `columns`".into());
                            }
                            c
                        }
                    };
                    Ok(GridItem {
                        id: id.to_owned(),
                        label: text_of(item, "label")?.unwrap_or(id).to_owned(),
                        detail: text_of(item, "detail")?.map(str::to_owned),
                        column,
                        allowed,
                    })
                };
                parsed().map_err(|e| format!("item {}: {e}", i + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if items.is_empty() {
            return Err("a grid needs `items`, each with an id".into());
        }
        if has_repeats(items.iter().map(|item| &item.id).collect()) {
            return Err("two items share an id".into());
        }
        let grid = Self {
            columns,
            items,
            search: text_of(field, "search")?.unwrap_or("Search").to_owned(),
            note: text_of(field, "note")?.map(str::to_owned),
        };
        if !grid.fits(&grid.defaults()) {
            return Err("the defaults put more items in a column than its `max`".into());
        }
        Ok(grid)
    }

    fn parse_allowed(
        item: &Value,
        count: usize,
        column_of: impl Fn(&str) -> Option<usize>,
    ) -> Result<Vec<bool>, String> {
        let ids = match item.get("columns") {
            None | Some(Value::Null) => return Ok(vec![true; count]),
            Some(Value::Array(ids)) if !ids.is_empty() => ids,
            Some(_) => return Err("`columns` is a list of column ids".into()),
        };
        let mut allowed = vec![false; count];
        for id in ids {
            let id = id.as_str().ok_or("`columns` is a list of column ids")?;
            let c = column_of(id).ok_or_else(|| format!("`columns` \"{id}\" is not a column"))?;
            if std::mem::replace(&mut allowed[c], true) {
                return Err(format!("`columns` repeats \"{id}\""));
            }
        }
        Ok(allowed)
    }

    fn parse_column(column: &Value, colored: bool, color: u32) -> Result<GridColumn, String> {
        let id = text_of(column, "id")?.ok_or("needs an id")?;
        let label = text_of(column, "label")?.unwrap_or(id);
        let color = match (colored, text_of(column, "color")?) {
            (false, Some(_)) => return Err("`color` on a grid with `colors = false`".into()),
            (false, None) => None,
            (true, None) => Some(color),
            (true, Some(text)) => Some(
                parse_color(text).ok_or_else(|| format!("`color` \"{text}\" is not #rrggbb"))?,
            ),
        };
        let max = match column.get("max") {
            None | Some(Value::Null) => None,
            Some(max) => Some(
                max.as_u64()
                    .filter(|&m| m >= 1)
                    .ok_or("`max` is a whole number from 1")? as usize,
            ),
        };
        Ok(GridColumn {
            id: id.to_owned(),
            label: label.to_owned(),
            summary: text_of(column, "summary")?.unwrap_or(label).to_owned(),
            color,
            max,
        })
    }

    pub fn defaults(&self) -> Vec<usize> {
        self.items.iter().map(|item| item.column).collect()
    }

    pub fn count(&self, picked: &[usize], column: usize) -> usize {
        picked.iter().filter(|&&c| c == column).count()
    }

    pub fn fits(&self, picked: &[usize]) -> bool {
        self.columns
            .iter()
            .enumerate()
            .all(|(c, column)| column.max.is_none_or(|max| self.count(picked, c) <= max))
    }

    pub fn picked(&self, saved: Option<&Value>) -> Vec<usize> {
        let picked: Vec<usize> = self
            .items
            .iter()
            .map(|item| {
                saved
                    .and_then(|s| s.get(&item.id))
                    .and_then(Value::as_str)
                    .and_then(|id| self.columns.iter().position(|c| c.id == id))
                    .filter(|&c| item.allows(c))
                    .unwrap_or(item.column)
            })
            .collect();
        if self.fits(&picked) {
            picked
        } else {
            self.defaults()
        }
    }

    pub fn value(&self, picked: &[usize]) -> Value {
        Value::Object(
            self.items
                .iter()
                .zip(picked)
                .map(|(item, &c)| (item.id.clone(), json!(self.columns[c].id)))
                .collect(),
        )
    }
}

pub const MAX_TASKS: usize = 50;
pub const TASK_FIELD_KINDS: [&str; 4] = ["check", "number", "text", "choice"];
const TASK_FIELD_REFUSED: [&str; 6] = [
    "visible_when",
    "enabled_by",
    "disabled_by",
    "above",
    "row",
    "stacked",
];

#[derive(Debug, Clone, PartialEq)]
pub struct TaskType {
    pub id: String,
    pub label: String,
    pub fields: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub kind: usize,
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tasks {
    pub types: Vec<TaskType>,
    pub defaults: Vec<Task>,
    pub max: usize,
}

pub fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

pub fn accepts(field: &Value, value: &Value) -> bool {
    match kind(field) {
        "check" => value.is_boolean(),
        "number" => value.as_f64().is_some_and(|n| {
            let (low, high) = number_bounds(&Value::Null, field, |_| None);
            let integer = field
                .get("integer")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            low <= n && n <= high && (!integer || n.fract() == 0.0)
        }),
        "choice" => value
            .as_str()
            .is_some_and(|s| options(field).iter().any(|o| o == s)),
        _ => value.is_string(),
    }
}

fn check_task_field(field: &Value) -> Result<(), String> {
    let kind = kind(field);
    if !TASK_FIELD_KINDS.contains(&kind) {
        return Err(format!(
            "a task field is one of {}",
            TASK_FIELD_KINDS.join(", ")
        ));
    }
    match id(field) {
        None => return Err("needs an id".into()),
        Some("type") => return Err("`type` names the task, so no field can use it".into()),
        Some(_) => {}
    }
    if let Some(key) = TASK_FIELD_REFUSED
        .iter()
        .find(|key| field.get(**key).is_some())
    {
        return Err(format!("task fields don't take `{key}`"));
    }
    if kind == "choice" && options(field).is_empty() {
        return Err("a choice needs options".into());
    }
    if field.get("box_first").is_some()
        && !(kind == "number" && field.get("box_first").is_some_and(Value::is_boolean))
    {
        return Err("`box_first` is true or false, on a number".into());
    }
    if !default_value(field).is_some_and(|value| accepts(field, &value)) {
        return Err("its default is outside its range".into());
    }
    Ok(())
}

impl TaskType {
    fn parse(row: &Value) -> Result<Self, String> {
        let id = text_of(row, "id")?.ok_or("needs an id")?;
        let fields = match row.get("fields") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(fields)) => fields.clone(),
            Some(_) => return Err("`fields` is a list".into()),
        };
        for (i, field) in fields.iter().enumerate() {
            check_task_field(field).map_err(|e| format!("field {}: {e}", i + 1))?;
        }
        if has_repeats(fields.iter().filter_map(self::id).collect()) {
            return Err("two fields share an id".into());
        }
        Ok(Self {
            id: id.to_owned(),
            label: text_of(row, "label")?.unwrap_or(id).to_owned(),
            fields,
        })
    }
}

impl Tasks {
    pub fn parse(field: &Value) -> Result<Self, String> {
        let Some(types) = field.get("types").and_then(Value::as_array) else {
            return Err("a task list needs `types`, each with an id".into());
        };
        let types = types
            .iter()
            .enumerate()
            .map(|(i, row)| TaskType::parse(row).map_err(|e| format!("type {}: {e}", i + 1)))
            .collect::<Result<Vec<_>, _>>()?;
        if types.is_empty() {
            return Err("a task list needs `types`, each with an id".into());
        }
        if has_repeats(types.iter().map(|t| &t.id).collect()) {
            return Err("two types share an id".into());
        }
        let max = match field.get("max") {
            None | Some(Value::Null) => MAX_TASKS,
            Some(max) => max
                .as_u64()
                .filter(|m| (1..=MAX_TASKS as u64).contains(m))
                .ok_or(format!("`max` is a whole number from 1 to {MAX_TASKS}"))?
                as usize,
        };
        let mut tasks = Self {
            types,
            defaults: Vec::new(),
            max,
        };
        let defaults = match field.get("default") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(rows)) => rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    tasks
                        .given(row)
                        .map_err(|e| format!("default task {}: {e}", i + 1))
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("`default` is a list of tasks".into()),
        };
        if defaults.len() > max {
            return Err("`default` holds more tasks than `max`".into());
        }
        tasks.defaults = defaults;
        Ok(tasks)
    }

    fn given(&self, row: &Value) -> Result<Task, String> {
        let Some(row) = row.as_object() else {
            return Err("a task is an object with a `type`".into());
        };
        let type_id = row
            .get("type")
            .and_then(Value::as_str)
            .ok_or("needs a `type`")?;
        let kind = self
            .types
            .iter()
            .position(|t| t.id == type_id)
            .ok_or_else(|| format!("\"{type_id}\" is not a type"))?;
        for (key, value) in row.iter().filter(|(key, _)| *key != "type") {
            let field = self.types[kind]
                .fields
                .iter()
                .find(|f| id(f) == Some(key))
                .ok_or_else(|| format!("`{key}` is not a field of \"{type_id}\""))?;
            if !accepts(field, value) {
                return Err(format!("`{key}` is not a value its field takes"));
            }
        }
        Ok(self.fill(kind, row))
    }

    fn fill(&self, kind: usize, given: &Map<String, Value>) -> Task {
        let values = self.types[kind]
            .fields
            .iter()
            .filter_map(|field| {
                let id = id(field)?;
                let value = given
                    .get(id)
                    .filter(|value| accepts(field, value))
                    .cloned()
                    .or_else(|| default_value(field))?;
                Some((id.to_owned(), value))
            })
            .collect();
        Task { kind, values }
    }

    pub fn fresh(&self, kind: usize) -> Task {
        self.fill(kind, &Map::new())
    }

    pub fn picked(&self, saved: Option<&Value>) -> Vec<Task> {
        let Some(Value::Array(rows)) = saved else {
            return self.defaults.clone();
        };
        rows.iter()
            .filter_map(|row| {
                let row = row.as_object()?;
                let type_id = row.get("type")?.as_str()?;
                let kind = self.types.iter().position(|t| t.id == type_id)?;
                Some(self.fill(kind, row))
            })
            .take(self.max)
            .collect()
    }

    pub fn value(&self, tasks: &[Task]) -> Value {
        Value::Array(
            tasks
                .iter()
                .map(|task| {
                    let mut row =
                        Map::from_iter([("type".to_owned(), json!(self.types[task.kind].id))]);
                    row.extend(task.values.clone());
                    Value::Object(row)
                })
                .collect(),
        )
    }
}

pub fn enabled_by(field: &Value) -> Option<&str> {
    field.get("enabled_by").and_then(Value::as_str)
}

pub fn disabled_by(field: &Value) -> Option<&str> {
    field.get("disabled_by").and_then(Value::as_str)
}

pub fn row_gap(spec: &Value) -> Option<f64> {
    spec.get("row_gap").and_then(Value::as_f64)
}

pub fn heading_gap(spec: &Value) -> Option<f64> {
    spec.get("heading_gap").and_then(Value::as_f64)
}

pub fn box_first(field: &Value) -> bool {
    field
        .get("box_first")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn stacked(field: &Value) -> bool {
    field
        .get("stacked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn heading(field: &Value) -> Option<u64> {
    field.get("heading").and_then(Value::as_u64)
}

pub fn row(field: &Value) -> Option<&str> {
    field.get("row").and_then(Value::as_str)
}

pub enum Visibility<'a> {
    Rule(&'a str, &'a Value),
    All(Vec<Visibility<'a>>),
    Any(Vec<Visibility<'a>>),
}

impl Visibility<'_> {
    fn holds(&self, value_of: &impl Fn(&str) -> Option<Value>) -> bool {
        match self {
            Visibility::Rule(id, equals) => value_of(id).is_some_and(|v| matches(&v, equals)),
            Visibility::All(all) => all.iter().all(|v| v.holds(value_of)),
            Visibility::Any(any) => any.iter().any(|v| v.holds(value_of)),
        }
    }
}

fn visibility(when: &Value) -> Option<Visibility<'_>> {
    match when {
        Value::Array(all) => Some(Visibility::All(all.iter().filter_map(visibility).collect())),
        Value::Object(o) if o.contains_key("any") => match o.get("any") {
            Some(Value::Array(any)) => {
                Some(Visibility::Any(any.iter().filter_map(visibility).collect()))
            }
            _ => None,
        },
        one => Some(Visibility::Rule(
            one.get("id")?.as_str()?,
            one.get("equals")?,
        )),
    }
}

pub fn is_visible(field: &Value, value_of: impl Fn(&str) -> Option<Value>) -> bool {
    field
        .get("visible_when")
        .and_then(visibility)
        .is_none_or(|v| v.holds(&value_of))
}

pub fn matches(value: &Value, equals: &Value) -> bool {
    match equals {
        Value::Array(any) => any.iter().any(|e| e == value),
        other => other == value,
    }
}

pub fn above(field: &Value) -> Option<(&str, f64)> {
    let link = field.get("above")?;
    Some((link.get("id")?.as_str()?, link.get("percent")?.as_f64()?))
}

fn whole(value: f64, integer: bool, round: fn(f64) -> f64) -> f64 {
    if integer {
        round((value * 1e6).round() / 1e6)
    } else {
        value
    }
}

pub fn number_bounds(
    spec: &Value,
    field: &Value,
    value_of: impl Fn(&str) -> Option<f64>,
) -> (f64, f64) {
    let integer = field
        .get("integer")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut low = field.get("min").and_then(Value::as_f64).unwrap_or(f64::MIN);
    let mut high = field.get("max").and_then(Value::as_f64).unwrap_or(f64::MAX);
    if let Some((other, percent)) = above(field)
        && let Some(value) = value_of(other)
    {
        low = low.max(whole(value * (1.0 + percent / 100.0), integer, f64::ceil));
    }
    if let Some(own) = id(field) {
        for linked in fields(spec) {
            if let (Some((other, percent)), Some(linked_id)) = (above(linked), id(linked))
                && other == own
                && let Some(value) = value_of(linked_id)
            {
                high = high.min(whole(value / (1.0 + percent / 100.0), integer, f64::floor));
            }
        }
    }
    (low, high)
}

fn check_above(spec: &Value, field: &Value, own: &str) -> Result<(), String> {
    if field.get("above").is_none() {
        return Ok(());
    }
    if kind(field) != "number" {
        return Err("only number fields take `above`".into());
    }
    let Some((other, percent)) = above(field) else {
        return Err("`above` is { id, percent }".into());
    };
    if !(0.0..=100.0).contains(&percent) {
        return Err("`above.percent` is 0 to 100".into());
    }
    let names_number = fields(spec)
        .iter()
        .any(|f| id(f) == Some(other) && kind(f) == "number");
    if other == own || !names_number {
        return Err(format!("`above.id` \"{other}\" isn't another number field"));
    }
    Ok(())
}

fn check_number_defaults(spec: &Value) -> Result<(), String> {
    let defaults = defaults(spec);
    let value_of = |id: &str| defaults.get(id).and_then(Value::as_f64);
    for field in fields(spec).iter().filter(|f| kind(f) == "number") {
        let Some(own) = id(field) else { continue };
        let (low, high) = number_bounds(spec, field, value_of);
        if !value_of(own).is_some_and(|v| low <= v && v <= high) {
            return Err(format!(
                "field \"{own}\": its default is outside its range once `above` is applied"
            ));
        }
    }
    Ok(())
}

pub fn validate(spec: &Value) -> Result<(), String> {
    if !spec.is_object() {
        return Err("settings are an object: { fields = [...] }".into());
    }
    if spec.get("row_gap").is_some() && !row_gap(spec).is_some_and(|g| (0.0..=500.0).contains(&g)) {
        return Err("`row_gap` is a number of points, 0 to 500".into());
    }
    if spec.get("heading_gap").is_some()
        && !heading_gap(spec).is_some_and(|g| (0.0..=500.0).contains(&g))
    {
        return Err("`heading_gap` is a number of points, 0 to 500".into());
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
        if field.get("row").is_some_and(|r| !r.is_string()) {
            return Err(format!("field {}: `row` is the name of a row", i + 1));
        }
        if field.get("heading").is_some()
            && !(kind == "label" && matches!(heading(field), Some(1 | 2)))
        {
            return Err(format!("field {}: `heading` is 1 or 2, on a label", i + 1));
        }
        if field.get("box_first").is_some()
            && !(kind == "number" && field.get("box_first").is_some_and(Value::is_boolean))
        {
            return Err(format!(
                "field {}: `box_first` is true or false, on a number",
                i + 1
            ));
        }
        if field.get("stacked").is_some()
            && !(kind == "number" && field.get("stacked").is_some_and(Value::is_boolean))
        {
            return Err(format!(
                "field {}: `stacked` is true or false, on a number",
                i + 1
            ));
        }
        if stacked(field) && box_first(field) {
            return Err(format!(
                "field {}: a `stacked` number has its label above, so it can't be `box_first`",
                i + 1
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
        if kind == "grid" {
            Grid::parse(field).map_err(|e| format!("field \"{id}\": {e}"))?;
        }
        if kind == "tasks" {
            Tasks::parse(field).map_err(|e| format!("field \"{id}\": {e}"))?;
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
        check_above(spec, field, id).map_err(|e| format!("field \"{id}\": {e}"))?;
    }
    check_number_defaults(spec)
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
        "grid" => Grid::parse(field)
            .ok()
            .map(|grid| grid.value(&grid.defaults())),
        "tasks" => Tasks::parse(field)
            .ok()
            .map(|tasks| tasks.value(&tasks.defaults)),
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

pub const ENTRY_END: char = '\0';

pub fn pin_request_line(hash: &str) -> String {
    format!("{PIN_REQUEST_PREFIX}{hash}")
}

pub fn pin_reply_line(answer: &Result<Option<String>, String>) -> String {
    answer_line("pin", answer)
}

pub fn lamp_request_line(hash: &str) -> String {
    format!("{LAMP_REQUEST_PREFIX}{hash}")
}

pub fn lamp_reply_line(answer: &Result<Option<String>, String>) -> String {
    answer_line("skill", answer)
}

pub fn parse_lamp_reply(line: &str) -> Result<Option<String>, String> {
    parse_answer(line, "lamp", "skill")
}

pub fn parse_pin_reply(line: &str) -> Result<Option<String>, String> {
    parse_answer(line, "PIN", "pin")
}

fn answer_line(key: &str, answer: &Result<Option<String>, String>) -> String {
    match answer {
        Ok(Some(value)) => Value::Object(Map::from_iter([(key.to_owned(), json!(value))])),
        Ok(None) => json!({ "none": true }),
        Err(error) => json!({ "error": error }),
    }
    .to_string()
}

fn parse_answer(line: &str, what: &str, key: &str) -> Result<Option<String>, String> {
    let reply: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("bad {what} reply from the loader: {e}"))?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(reply.get(key).and_then(Value::as_str).map(str::to_owned))
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
    fn exits_and_resumes_round_trip() {
        let exit = Exit {
            status: ExitStatus::Blocked,
            reason: Some("out of food".into()),
        };
        assert_eq!(
            parse_exit(exit_line(&exit).strip_prefix(EXIT_PREFIX).unwrap()),
            Some(exit)
        );
        assert_eq!(
            parse_exit(r#"{"status":"done"}"#).map(|e| e.status),
            Some(ExitStatus::Done)
        );
        assert!(parse_exit(r#"{"status":"failed"}"#).is_none());
        let plan = BreakPlan {
            breaks: vec![PlannedBreak {
                remaining: 60_000.0,
                length: 120_000.0,
                at: None,
                index: 2,
            }],
            ..Default::default()
        };
        let line = resume_line(&plan);
        assert_eq!(
            parse_breaks(line.strip_prefix(RESUME_PREFIX).unwrap()),
            Some(plan)
        );
    }

    fn values(pairs: &[(&str, Value)]) -> impl Fn(&str) -> Option<Value> + use<> {
        let pairs: Vec<(String, Value)> = pairs
            .iter()
            .map(|(id, v)| ((*id).to_owned(), v.clone()))
            .collect();
        move |id| pairs.iter().find(|(k, _)| k == id).map(|(_, v)| v.clone())
    }

    #[test]
    fn visibility_rules_match_a_value_or_a_list() {
        let field =
            json!({ "id": "ring", "visible_when": { "id": "collect", "equals": ["A", "B"] } });
        assert!(is_visible(&field, values(&[("collect", json!("B"))])));
        assert!(!is_visible(&field, values(&[("collect", json!("C"))])));
        assert!(!is_visible(&field, values(&[])));
        assert!(matches(&json!(true), &json!(true)));
        assert!(is_visible(&json!({ "id": "x" }), values(&[])));
        let both =
            json!({ "visible_when": [{ "id": "a", "equals": 1 }, { "id": "b", "equals": 2 }] });
        assert!(is_visible(
            &both,
            values(&[("a", json!(1)), ("b", json!(2))])
        ));
        assert!(!is_visible(
            &both,
            values(&[("a", json!(1)), ("b", json!(3))])
        ));
    }

    #[test]
    fn visibility_any_matches_one_of_its_entries() {
        let field = json!({ "visible_when": { "any": [
            { "id": "activity", "equals": "Boss" },
            [{ "id": "activity", "equals": ["Slayer", "Turael"] }, { "id": "area", "equals": "Temple" }]
        ] } });
        assert!(is_visible(
            &field,
            values(&[("activity", json!("Boss")), ("area", json!("Cave"))])
        ));
        assert!(is_visible(
            &field,
            values(&[("activity", json!("Turael")), ("area", json!("Temple"))])
        ));
        assert!(!is_visible(
            &field,
            values(&[("activity", json!("Slayer")), ("area", json!("Cave"))])
        ));
        assert!(!is_visible(
            &field,
            values(&[("activity", json!("Combat")), ("area", json!("Temple"))])
        ));
        let nested = json!({ "visible_when": [
            { "id": "on", "equals": true },
            { "any": [{ "id": "a", "equals": 1 }, { "id": "b", "equals": 1 }] }
        ] });
        assert!(is_visible(
            &nested,
            values(&[("on", json!(true)), ("b", json!(1))])
        ));
        assert!(!is_visible(
            &nested,
            values(&[("on", json!(false)), ("b", json!(1))])
        ));
        assert!(!is_visible(
            &json!({ "visible_when": { "any": [] } }),
            values(&[])
        ));
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
    fn checks_grids_and_keeps_picks_within_max() {
        let grid = |extra: Value| {
            let mut field = json!({
                "id": "tasks",
                "kind": "grid",
                "default": "do",
                "columns": [
                    { "id": "do", "label": "Do" },
                    { "id": "skip", "color": "#112233" },
                    { "id": "block", "label": "Block", "summary": "blocked", "max": 1 }
                ],
                "items": [
                    { "id": "a", "label": "A", "detail": "7" },
                    { "id": "b", "default": "block" }
                ]
            });
            field
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            field
        };
        let field = grid(json!({}));
        assert_eq!(validate(&json!({ "fields": [field.clone()] })), Ok(()));
        let parsed = Grid::parse(&field).unwrap();
        assert_eq!(
            parsed.columns.iter().map(|c| c.color).collect::<Vec<_>>(),
            [Some(GRID_COLORS[0]), Some(0x112233), Some(GRID_COLORS[2])]
        );
        assert_eq!(parsed.columns[1].summary, "skip");
        assert_eq!(parsed.columns[2].summary, "blocked");
        assert_eq!(parsed.items[1].label, "b");
        assert_eq!(
            default_value(&field),
            Some(json!({ "a": "do", "b": "block" }))
        );
        assert_eq!(
            parsed.picked(Some(&json!({ "a": "skip", "b": "gone" }))),
            [1, 2]
        );
        assert_eq!(
            parsed.picked(Some(&json!({ "a": "block", "b": "block" }))),
            [0, 2]
        );
        let plain = Grid::parse(&grid(json!({
            "colors": false,
            "columns": [{ "id": "yes" }, { "id": "no" }],
            "default": "no",
            "items": [{ "id": "a" }]
        })))
        .unwrap();
        assert!(plain.columns.iter().all(|c| c.color.is_none()));
        let limited = Grid::parse(&grid(json!({
            "items": [
                { "id": "a", "columns": ["skip", "block"] },
                { "id": "b", "columns": ["do", "skip"], "default": "skip" }
            ]
        })))
        .unwrap();
        assert_eq!(limited.defaults(), [1, 1]);
        assert!(!limited.items[0].allows(0) && limited.items[0].allows(2));
        assert_eq!(
            limited.picked(Some(&json!({ "a": "do", "b": "block" }))),
            [1, 1]
        );
        for bad in [
            json!({ "items": [{ "id": "a", "columns": [] }] }),
            json!({ "items": [{ "id": "a", "columns": ["do", "do"] }] }),
            json!({ "items": [{ "id": "a", "columns": ["maybe"] }] }),
            json!({ "items": [{ "id": "a", "columns": "do" }] }),
            json!({ "items": [{ "id": "a", "columns": ["do"], "default": "skip" }] }),
            json!({ "columns": [{ "id": "only" }] }),
            json!({ "columns": [{ "id": "a" }, { "id": "a" }] }),
            json!({ "columns": [{ "id": "a", "color": "green" }, { "id": "b" }] }),
            json!({ "colors": false, "columns": [{ "id": "a", "color": "#000000" }, { "id": "b" }] }),
            json!({ "default": "maybe" }),
            json!({ "items": [] }),
            json!({ "items": [{ "id": "a" }, { "id": "a" }] }),
            json!({ "items": [{ "id": "a", "default": "block" }, { "id": "b", "default": "block" }] }),
            json!({ "columns": [{ "id": "do", "max": 0 }, { "id": "b" }] }),
        ] {
            assert!(
                validate(&json!({ "fields": [grid(bad.clone())] })).is_err(),
                "{bad}"
            );
        }
    }

    fn task_list(extra: Value) -> Value {
        let mut field = json!({
            "id": "tasks",
            "kind": "tasks",
            "types": [
                { "id": "chop", "label": "Chop", "fields": [
                    { "id": "tree", "kind": "choice", "options": ["Oak", "Yew"], "default": "Yew" },
                    { "id": "logs", "kind": "number", "default": 0, "min": 0, "max": 100, "integer": true }
                ] },
                { "id": "bank" }
            ],
            "default": [{ "type": "chop", "tree": "Oak" }, { "type": "bank" }]
        });
        field
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        field
    }

    #[test]
    fn task_lists_fill_defaults_and_drop_what_no_longer_fits() {
        let field = task_list(json!({}));
        assert_eq!(validate(&json!({ "fields": [field.clone()] })), Ok(()));
        assert_eq!(
            default_value(&field),
            Some(json!([{ "type": "chop", "tree": "Oak", "logs": 0 }, { "type": "bank" }]))
        );
        let tasks = Tasks::parse(&field).unwrap();
        let saved = json!([
            { "type": "bank" },
            { "type": "gone" },
            { "type": "chop", "tree": "Maple", "logs": 50 },
            { "type": "chop", "logs": 101 },
            "nonsense"
        ]);
        assert_eq!(
            tasks.value(&tasks.picked(Some(&saved))),
            json!([
                { "type": "bank" },
                { "type": "chop", "tree": "Yew", "logs": 50 },
                { "type": "chop", "tree": "Yew", "logs": 0 }
            ])
        );
        assert_eq!(tasks.picked(Some(&json!([]))), []);
        assert_eq!(tasks.picked(None), tasks.defaults);
        let capped = Tasks::parse(&task_list(json!({ "max": 2 }))).unwrap();
        assert_eq!(
            capped
                .picked(Some(
                    &json!([{ "type": "bank" }, { "type": "bank" }, { "type": "bank" }])
                ))
                .len(),
            2
        );
        assert_eq!(number_value(3.0), json!(3));
        assert_eq!(number_value(2.5), json!(2.5));
    }

    #[test]
    fn task_list_mistakes_are_refused() {
        let chop =
            |fields: Value| json!({ "types": [{ "id": "chop", "fields": fields }], "default": [] });
        for bad in [
            json!({ "types": [] }),
            json!({ "types": [{ "id": "a" }, { "id": "a" }] }),
            json!({ "types": [{ "label": "No id" }] }),
            json!({ "max": 0 }),
            json!({ "max": 51 }),
            json!({ "max": 1 }),
            json!({ "default": [{ "type": "gone" }] }),
            json!({ "default": [{ "type": "chop", "tree": "Maple" }] }),
            json!({ "default": [{ "type": "chop", "axe": "Rune" }] }),
            json!({ "default": [{ "tree": "Oak" }] }),
            json!({ "default": { "type": "chop" } }),
            chop(json!([{ "id": "type", "kind": "check" }])),
            chop(json!([{ "id": "a", "kind": "list", "items": [{ "id": "x" }] }])),
            chop(json!([{ "id": "a", "kind": "check" }, { "id": "a", "kind": "text" }])),
            chop(json!([{ "id": "a", "kind": "choice" }])),
            chop(json!([{ "id": "a", "kind": "number", "default": 5, "max": 4 }])),
            chop(
                json!([{ "id": "a", "kind": "check", "visible_when": { "id": "b", "equals": true } }]),
            ),
            chop(json!([{ "kind": "check" }])),
        ] {
            assert!(
                validate(&json!({ "fields": [task_list(bad.clone())] })).is_err(),
                "{bad}"
            );
        }
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
        assert!(validate(&json!({ "fields": [{ "id": "a", "row": 1 }] })).is_err());
        assert!(
            validate(&json!({ "fields": [{ "id": "a", "kind": "text", "box_first": true }] }))
                .is_err()
        );
        assert!(
            validate(&json!({ "fields": [{ "id": "a", "kind": "number", "box_first": 1 }] }))
                .is_err()
        );
        assert_eq!(
            validate(&json!({ "fields": [{ "id": "a", "kind": "number", "box_first": true }] })),
            Ok(())
        );
        assert_eq!(
            validate(&json!({ "fields": [{ "id": "a", "kind": "number", "stacked": true }] })),
            Ok(())
        );
        for bad in [
            json!({ "id": "a", "kind": "check", "stacked": true }),
            json!({ "id": "a", "kind": "number", "stacked": "yes" }),
            json!({ "id": "a", "kind": "number", "stacked": true, "box_first": true }),
        ] {
            assert!(
                validate(&json!({ "fields": [bad.clone()] })).is_err(),
                "{bad}"
            );
        }
        assert!(validate(&json!({ "fields": [{ "kind": "label", "heading": 3 }] })).is_err());
        assert!(validate(&json!({ "row_gap": -1, "fields": [] })).is_err());
        assert_eq!(validate(&json!({ "row_gap": 12, "fields": [] })), Ok(()));
        assert!(validate(&json!({ "heading_gap": 501, "fields": [] })).is_err());
        assert!(validate(&json!({ "heading_gap": "big", "fields": [] })).is_err());
        assert_eq!(validate(&json!({ "heading_gap": 0, "fields": [] })), Ok(()));
        assert!(validate(&json!({ "fields": [{ "id": "a", "heading": 1 }] })).is_err());
        assert_eq!(
            validate(&json!({ "fields": [{ "kind": "label", "text": "A", "heading": 2 }] })),
            Ok(())
        );
        assert_eq!(
            validate(&json!({ "fields": [{ "id": "a", "row": "x" }, { "id": "b", "row": "x" }] })),
            Ok(())
        );
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
        assert_eq!(
            parse_settings(
                settings_line(&json!({ "a": 1 }))
                    .strip_prefix(SETTINGS_PREFIX)
                    .unwrap()
            ),
            Some(json!({ "a": 1 }))
        );
        assert_eq!(parse_settings("[1]"), None);
    }

    fn keys(min_default: i64, max_default: i64) -> Value {
        json!({ "fields": [
            { "id": "low", "kind": "number", "default": min_default, "min": 10, "max": 80, "integer": true },
            { "id": "high", "kind": "number", "default": max_default, "min": 11, "max": 100, "integer": true,
              "above": { "id": "low", "percent": 10 } }
        ] })
    }

    #[test]
    fn above_links_both_fields_bounds() {
        let spec = keys(30, 50);
        assert_eq!(validate(&spec), Ok(()));
        let values = json!({ "low": 30, "high": 50 });
        let value_of = |id: &str| values.get(id).and_then(Value::as_f64);
        let [low, high] = [0, 1].map(|i| number_bounds(&spec, &fields(&spec)[i], value_of));
        assert_eq!(low, (10.0, 45.0));
        assert_eq!(high, (33.0, 100.0));
    }

    #[test]
    fn above_mistakes_are_refused() {
        assert!(validate(&keys(30, 32)).is_err());
        let to_itself = json!({ "fields": [
            { "id": "a", "kind": "number", "above": { "id": "a", "percent": 10 } }
        ] });
        assert!(validate(&to_itself).is_err());
        let on_a_check = json!({ "fields": [
            { "id": "a", "kind": "number" },
            { "id": "b", "kind": "check", "above": { "id": "a", "percent": 10 } }
        ] });
        assert!(validate(&on_a_check).is_err());
        let too_far = json!({ "fields": [
            { "id": "a", "kind": "number" },
            { "id": "b", "kind": "number", "default": 5, "above": { "id": "a", "percent": 150 } }
        ] });
        assert!(validate(&too_far).is_err());
    }
}
