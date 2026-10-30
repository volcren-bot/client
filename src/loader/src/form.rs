use std::collections::HashMap;

use eframe::egui;
use serde_json::{Map, Value, json};
use injector::form::{self as spec_form};

pub enum FieldValue {
    Check(bool),
    Number(f64),
    Text(String),
}

pub struct Form {
    pub title: String,
    spec: Value,
    values: HashMap<String, FieldValue>,
}

pub enum Outcome {
    Open,
    Submitted(Value),
    Cancelled,
}

fn collect(values: &HashMap<String, FieldValue>) -> Map<String, Value> {
    values
        .iter()
        .map(|(id, value)| {
            let v = match value {
                FieldValue::Check(b) => json!(b),
                FieldValue::Number(n) if n.fract() == 0.0 && n.abs() < 9e15 => json!(*n as i64),
                FieldValue::Number(n) => json!(n),
                FieldValue::Text(s) => json!(s),
            };
            (id.clone(), v)
        })
        .collect()
}

fn value_of(field: &Value, saved: Option<&Value>) -> Option<FieldValue> {
    let default = spec_form::default_value(field)?;
    let pick = |v: &Value| match spec_form::kind(field) {
        "check" => v.as_bool().map(FieldValue::Check),
        "number" => v.as_f64().map(FieldValue::Number),
        "choice" => v.as_str().filter(|s| spec_form::options(field).iter().any(|o| o == s)).map(|s| FieldValue::Text(s.to_owned())),
        _ => v.as_str().map(|s| FieldValue::Text(s.to_owned())),
    };
    saved.and_then(pick).or_else(|| pick(&default))
}

impl Form {
    pub fn new(spec: Value, remembered: Option<&Map<String, Value>>) -> Self {
        let title = spec.get("title").and_then(Value::as_str).unwrap_or("Script settings").to_owned();
        let mut values = HashMap::new();
        for field in spec_form::fields(&spec) {
            if let Some(id) = spec_form::id(field)
                && let Some(value) = value_of(field, remembered.and_then(|r| r.get(id))) {
                    values.insert(id.to_owned(), value);
                }
        }
        Self { title, spec, values }
    }

    fn field_ui(ui: &mut egui::Ui, field: &Value, value: Option<&mut FieldValue>) {
        let label = field.get("label").or_else(|| field.get("text")).and_then(Value::as_str).or_else(|| spec_form::id(field)).unwrap_or("");
        let number = |key: &str| field.get(key).and_then(Value::as_f64);
        match (spec_form::kind(field), value) {
            ("label", _) => {
                ui.label(label);
            }
            ("check", Some(FieldValue::Check(b))) => {
                ui.checkbox(b, label);
            }
            ("number", Some(FieldValue::Number(n))) => {
                ui.horizontal(|ui| {
                    ui.label(label);
                    let mut drag = egui::DragValue::new(n).range(number("min").unwrap_or(f64::MIN)..=number("max").unwrap_or(f64::MAX));
                    if field.get("integer").and_then(Value::as_bool).unwrap_or(false) {
                        drag = drag.fixed_decimals(0).speed(1.0);
                    } else if let Some(step) = number("step") {
                        drag = drag.speed(step);
                    }
                    ui.add(drag);
                });
            }
            ("choice", Some(FieldValue::Text(selected))) => {
                egui::ComboBox::from_label(label).selected_text(selected.clone()).show_ui(ui, |ui| {
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
            _ => {}
        }
    }

    /// The form's fields and buttons in `ui`, for a page that hosts the form itself.
    pub fn show_inline(&mut self, ui: &mut egui::Ui) -> Outcome {
        let mut outcome = Outcome::Open;
        let submit = self.spec.get("submit").and_then(Value::as_str).unwrap_or("Start");
        let fields = spec_form::fields(&self.spec);
        for field in fields {
            let value = spec_form::id(field).and_then(|id| self.values.get_mut(id));
            Self::field_ui(ui, field, value);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(submit).clicked() {
                outcome = Outcome::Submitted(Value::Object(collect(&self.values)));
            }
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancelled;
            }
        });
        outcome
    }
}
