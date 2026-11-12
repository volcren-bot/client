use std::collections::HashMap;

use eframe::egui;
use injector::form::{self as spec_form};
use serde_json::{Map, Value, json};

use crate::layouts::{self, Kind, Known, Rules, ScriptLayouts};

const STACK_BELOW: f32 = 420.0;

const FIELD_GAP: f32 = 10.0;

const ROW_GAP: f32 = 24.0;

fn with_colon(label: &str) -> String {
    if label.ends_with(':') {
        label.to_owned()
    } else {
        format!("{label}:")
    }
}

pub enum FieldValue {
    Check(bool),
    Number(f64),
    Text(String),
    List(Vec<(String, String, bool)>),
    Layout(Option<String>),
}

fn layout_kind(field: &Value) -> Option<Kind> {
    match spec_form::kind(field) {
        "equipment" => Some(Kind::Equipment),
        "inventory" => Some(Kind::Inventory),
        "prayer" => Some(Kind::Prayer),
        _ => None,
    }
}

struct KindRules([Rules; 3]);

impl KindRules {
    fn of(&self, kind: Kind) -> &Rules {
        &self.0[match kind {
            Kind::Equipment => 0,
            Kind::Inventory => 1,
            Kind::Prayer => 2,
        }]
    }
}

pub struct Form {
    pub title: String,
    spec: Value,
    values: HashMap<String, FieldValue>,
    rules: KindRules,
}

pub enum Outcome {
    Open,
    Submitted(Value),
    Cancelled,
}

fn collect_one(value: &FieldValue) -> Value {
    match value {
        FieldValue::Check(b) => json!(b),
        FieldValue::Number(n) if n.fract() == 0.0 && n.abs() < 9e15 => json!(*n as i64),
        FieldValue::Number(n) => json!(n),
        FieldValue::Text(s) => json!(s),
        FieldValue::List(rows) => Value::Object(
            rows.iter()
                .map(|(id, _, on)| (id.clone(), json!(on)))
                .collect(),
        ),
        FieldValue::Layout(name) => json!(name),
    }
}

fn collect(values: &HashMap<String, FieldValue>) -> Map<String, Value> {
    values
        .iter()
        .map(|(id, value)| (id.clone(), collect_one(value)))
        .collect()
}

fn value_of(field: &Value, saved: Option<&Value>) -> Option<FieldValue> {
    if layout_kind(field).is_some() {
        return Some(FieldValue::Layout(
            saved.and_then(Value::as_str).map(str::to_owned),
        ));
    }
    let default = spec_form::default_value(field)?;
    if spec_form::kind(field) == "list" {
        let rows = spec_form::items(field)
            .into_iter()
            .map(|(id, label, on)| {
                let ticked = saved
                    .and_then(|s| s.get(&id))
                    .and_then(Value::as_bool)
                    .unwrap_or(on);
                (id, label, ticked)
            })
            .collect();
        return Some(FieldValue::List(rows));
    }
    let pick = |v: &Value| match spec_form::kind(field) {
        "check" => v.as_bool().map(FieldValue::Check),
        "number" => v.as_f64().map(FieldValue::Number),
        "choice" => v
            .as_str()
            .filter(|s| spec_form::options(field).iter().any(|o| o == s))
            .map(|s| FieldValue::Text(s.to_owned())),
        _ => v.as_str().map(|s| FieldValue::Text(s.to_owned())),
    };
    saved.and_then(pick).or_else(|| pick(&default))
}

impl Form {
    pub fn new(spec: Value, remembered: Option<&Map<String, Value>>) -> Self {
        let title = spec
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Script settings")
            .to_owned();
        let mut values = HashMap::new();
        for field in spec_form::fields(&spec) {
            if let Some(id) = spec_form::id(field)
                && let Some(value) = value_of(field, remembered.and_then(|r| r.get(id)))
            {
                values.insert(id.to_owned(), value);
            }
        }
        let rules = KindRules(
            [Kind::Equipment, Kind::Inventory, Kind::Prayer].map(|kind| {
                let mut rules = Rules::default();
                for field in spec_form::fields(&spec) {
                    if layout_kind(field) == Some(kind) {
                        rules.required.extend(spec_form::required(field));
                        rules.locked.extend(spec_form::locked(field));
                    }
                }
                rules
            }),
        );
        Self {
            title,
            spec,
            values,
            rules,
        }
    }

    fn list_ui(ui: &mut egui::Ui, id: &str, rows: &mut [(String, String, bool)]) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, true])
                .max_height(240.0)
                .show(ui, |ui| {
                    egui::Grid::new(id)
                        .num_columns(1)
                        .striped(true)
                        .min_col_width(ui.available_width())
                        .spacing(egui::vec2(16.0, 6.0))
                        .show(ui, |ui| {
                            for (_, label, on) in rows.iter_mut() {
                                ui.checkbox(on, label.as_str());
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn field_ui(
        ui: &mut egui::Ui,
        field: &Value,
        value: Option<&mut FieldValue>,
        layouts: &ScriptLayouts,
        rules: &KindRules,
    ) {
        let label = field
            .get("label")
            .or_else(|| field.get("text"))
            .and_then(Value::as_str)
            .or_else(|| spec_form::id(field))
            .unwrap_or("");
        let number = |key: &str| field.get(key).and_then(Value::as_f64);
        match (spec_form::kind(field), value) {
            ("label", _) => match spec_form::heading(field) {
                Some(1) => {
                    ui.heading(label);
                }
                Some(_) => {
                    let style = ui.style();
                    let size = (egui::TextStyle::Body.resolve(style).size
                        + egui::TextStyle::Heading.resolve(style).size)
                        / 2.0;
                    ui.label(egui::RichText::new(label).strong().size(size));
                }
                None => {
                    ui.label(label);
                }
            },
            ("list", Some(FieldValue::List(rows))) => {
                if !label.is_empty() {
                    ui.label(label);
                }
                Self::list_ui(ui, spec_form::id(field).unwrap_or("list"), rows);
            }
            ("check", Some(FieldValue::Check(b))) => {
                ui.checkbox(b, label);
            }
            ("number", Some(FieldValue::Number(n))) => {
                ui.horizontal(|ui| {
                    let box_first = spec_form::box_first(field);
                    if !box_first {
                        ui.label(label);
                    }
                    let mut drag = egui::DragValue::new(n).range(
                        number("min").unwrap_or(f64::MIN)..=number("max").unwrap_or(f64::MAX),
                    );
                    if field
                        .get("integer")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        drag = drag.fixed_decimals(0).speed(1.0);
                    } else if let Some(step) = number("step") {
                        drag = drag.speed(step);
                    }
                    ui.add(drag);
                    if box_first {
                        ui.label(label);
                    }
                });
            }
            ("choice", Some(FieldValue::Text(selected))) => {
                if !label.is_empty() {
                    ui.label(with_colon(label));
                }
                egui::ComboBox::from_id_salt(spec_form::id(field).unwrap_or(label))
                    .selected_text(selected.clone())
                    .show_ui(ui, |ui| {
                        for option in spec_form::options(field) {
                            ui.selectable_value(selected, option.clone(), option);
                        }
                    });
            }
            (_, Some(FieldValue::Text(text))) => {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.text_edit_singleline(text);
                });
            }
            (_, Some(FieldValue::Layout(chosen))) => {
                if let Some(kind) = layout_kind(field) {
                    ui.label(with_colon(if label.is_empty() {
                        kind.label()
                    } else {
                        label
                    }));
                    layouts::picker_ui(
                        ui,
                        kind,
                        layouts,
                        chosen,
                        spec_form::id(field).unwrap_or(""),
                        rules.of(kind),
                    );
                }
            }
            _ => {}
        }
    }

    pub fn rules(&self, kind: Kind) -> &Rules {
        self.rules.of(kind)
    }

    pub fn has_layout(&self, kind: Kind) -> bool {
        spec_form::fields(&self.spec)
            .iter()
            .any(|field| layout_kind(field) == Some(kind))
    }

    pub fn remembered(&self) -> Map<String, Value> {
        collect(&self.values)
    }

    pub fn submit_label(&self) -> &str {
        self.spec
            .get("submit")
            .and_then(Value::as_str)
            .unwrap_or("Start")
    }

    pub fn submitted(&self, layouts: &ScriptLayouts, known: &Known) -> Outcome {
        let mut values = collect(&self.values);
        let rules = &self.rules;
        for field in spec_form::fields(&self.spec) {
            if let (Some(kind), Some(id)) = (layout_kind(field), spec_form::id(field)) {
                let chosen = values.get(id).and_then(Value::as_str).unwrap_or("");
                values.insert(
                    id.to_owned(),
                    kind.value_of(layouts, chosen, rules.of(kind), known),
                );
            }
        }
        Outcome::Submitted(Value::Object(values))
    }

    pub fn show_inline(&mut self, ui: &mut egui::Ui, layouts: &ScriptLayouts) {
        ui.spacing_mut().item_spacing.y = FIELD_GAP;
        let rules = &self.rules;
        let fields = spec_form::fields(&self.spec);
        let columns = self
            .spec
            .get("columns")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let gap = spec_form::row_gap(&self.spec).map_or(ROW_GAP, |g| g as f32);
        let values = &mut self.values;
        if !columns {
            Self::show_fields(ui, fields, gap, values, layouts, rules);
            return;
        }
        let (checks, others): (Vec<&Value>, Vec<&Value>) = fields
            .iter()
            .partition(|field| spec_form::kind(field) == "check");
        if ui.available_width() < STACK_BELOW {
            Self::show_fields(
                ui,
                others.into_iter().chain(checks),
                gap,
                values,
                layouts,
                rules,
            );
            return;
        }
        ui.columns(2, |cols| {
            Self::show_fields(&mut cols[0], others, gap, values, layouts, rules);
            Self::show_fields(&mut cols[1], checks, gap, values, layouts, rules);
        });
    }

    fn show_fields<'a>(
        ui: &mut egui::Ui,
        fields: impl IntoIterator<Item = &'a Value>,
        gap: f32,
        values: &mut HashMap<String, FieldValue>,
        layouts: &ScriptLayouts,
        rules: &KindRules,
    ) {
        let shown: Vec<&Value> = fields
            .into_iter()
            .filter(|field| Self::visible(field, values))
            .collect();
        let mut rest = shown.as_slice();
        while let Some(first) = rest.first() {
            let len = spec_form::row(first).map_or(1, |row| {
                rest.iter()
                    .take_while(|field| spec_form::row(field) == Some(row))
                    .count()
            });
            let (line, after) = rest.split_at(len);
            rest = after;
            if len == 1 {
                Self::show_field(ui, first, values, layouts, rules);
            } else {
                Self::show_row(ui, line, gap, values, layouts, rules);
            }
        }
    }

    fn show_row(
        ui: &mut egui::Ui,
        line: &[&Value],
        gap: f32,
        values: &mut HashMap<String, FieldValue>,
        layouts: &ScriptLayouts,
        rules: &KindRules,
    ) {
        let id = ui
            .id()
            .with(("form_row", spec_form::row(line[0]), spec_form::id(line[0])));
        let width: Option<f32> = ui.data(|d| d.get_temp(id));
        if width.is_some_and(|w| w > ui.available_width()) {
            for field in line {
                Self::show_field(ui, field, values, layouts, rules);
            }
            return;
        }
        let row = ui.horizontal_top(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.spacing_mut().item_spacing.x = gap;
            for field in line {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.x = FIELD_GAP;
                    Self::show_field(ui, field, values, layouts, rules);
                });
            }
        });
        ui.data_mut(|d| d.insert_temp(id, row.response.rect.width()));
    }

    fn visible(field: &Value, values: &HashMap<String, FieldValue>) -> bool {
        spec_form::visible_rules(field)
            .into_iter()
            .all(|(id, equals)| {
                values
                    .get(id)
                    .is_some_and(|v| spec_form::matches(&collect_one(v), equals))
            })
    }

    fn show_field(
        ui: &mut egui::Ui,
        field: &Value,
        values: &mut HashMap<String, FieldValue>,
        layouts: &ScriptLayouts,
        rules: &KindRules,
    ) {
        if !Self::visible(field, values) {
            return;
        }
        let checked = |id: &str| matches!(values.get(id), Some(FieldValue::Check(true)));
        let usable = spec_form::enabled_by(field).is_none_or(checked)
            && !spec_form::disabled_by(field).is_some_and(checked);
        let value = spec_form::id(field).and_then(|id| values.get_mut(id));
        ui.add_enabled_ui(usable, |ui| {
            Self::field_ui(ui, field, value, layouts, rules)
        });
    }
}
