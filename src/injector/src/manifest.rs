use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const FILE_NAME: &str = "manifest.json";
pub const DEFAULT_FINISH_GRACE_MINUTES: u32 = 5;
pub const MAX_FINISH_GRACE_MINUTES: u32 = 120;
const MAX_GOALS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub label: String,
}

fn default_grace() -> u32 {
    DEFAULT_FINISH_GRACE_MINUTES
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub queueable: bool,
    #[serde(default)]
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default = "default_grace")]
    pub finish_grace_minutes: u32,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            queueable: false,
            goals: Vec::new(),
            settings: None,
            finish_grace_minutes: DEFAULT_FINISH_GRACE_MINUTES,
        }
    }
}

fn valid_goal_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl Manifest {
    pub fn parse(value: Value) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let manifest: Self =
            serde_json::from_value(value).map_err(|e| format!("{FILE_NAME}: {e}"))?;
        manifest.validate().map_err(|e| format!("{FILE_NAME}: {e}"))?;
        Ok(manifest)
    }

    pub fn read(path: &Path) -> Result<Option<Self>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
        };
        let value = serde_json::from_str(&text)
            .map_err(|e| format!("{FILE_NAME} isn't valid JSON: {e}"))?;
        Self::parse(value).map(Some)
    }

    pub fn beside(script: &Path) -> Result<Option<Self>, String> {
        script
            .parent()
            .map_or(Ok(None), |dir| Self::read(&dir.join(FILE_NAME)))
    }

    fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_FINISH_GRACE_MINUTES).contains(&self.finish_grace_minutes) {
            return Err(format!(
                "finish_grace_minutes is 1 to {MAX_FINISH_GRACE_MINUTES}"
            ));
        }
        if self.goals.len() > MAX_GOALS {
            return Err(format!("at most {MAX_GOALS} goals"));
        }
        for (i, goal) in self.goals.iter().enumerate() {
            if !valid_goal_id(&goal.id) {
                return Err(format!(
                    "goal \"{}\": an id is 1 to 32 lowercase letters, digits or _, starting with a letter",
                    goal.id
                ));
            }
            if goal.label.is_empty() || goal.label.chars().count() > 64 {
                return Err(format!("goal \"{}\": a label is 1 to 64 characters", goal.id));
            }
            if self.goals[..i].iter().any(|g| g.id == goal.id) {
                return Err(format!("goal \"{}\" is listed twice", goal.id));
            }
        }
        if let Some(settings) = &self.settings {
            crate::form::validate(settings).map_err(|e| format!("settings: {e}"))?;
        }
        Ok(())
    }

    pub fn defaults(&self) -> Value {
        self.settings
            .as_ref()
            .map_or_else(|| json!({}), crate::form::defaults)
    }

    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_manifest_is_an_empty_one() {
        let manifest = Manifest::parse(Value::Null).unwrap();
        assert_eq!(manifest, Manifest::default());
        assert_eq!(manifest.defaults(), json!({}));
    }

    #[test]
    fn a_full_manifest_parses_and_gives_its_defaults() {
        let manifest = Manifest::parse(json!({
            "queueable": true,
            "goals": [{ "id": "kills", "label": "Kills" }],
            "finish_grace_minutes": 10,
            "settings": { "fields": [{ "id": "eat_at", "kind": "number", "default": 50 }] }
        }))
        .unwrap();
        assert!(manifest.queueable);
        assert_eq!(manifest.finish_grace_minutes, 10);
        assert_eq!(manifest.goal("kills").unwrap().label, "Kills");
        assert_eq!(manifest.defaults(), json!({ "eat_at": 50 }));
    }

    #[test]
    fn mistakes_are_refused() {
        for bad in [
            json!({ "queuable": true }),
            json!({ "queueable": "yes" }),
            json!({ "finish_grace_minutes": 0 }),
            json!({ "finish_grace_minutes": 121 }),
            json!({ "goals": [{ "id": "Kills", "label": "Kills" }] }),
            json!({ "goals": [{ "id": "kills", "label": "" }] }),
            json!({ "goals": [{ "id": "a", "label": "A" }, { "id": "a", "label": "B" }] }),
            json!({ "settings": { "fields": [{ "id": "x", "kind": "slider" }] } }),
        ] {
            assert!(Manifest::parse(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_manifest_is_read_beside_the_script() {
        let dir = std::env::temp_dir().join(format!("manifest-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("main.luau");
        assert_eq!(Manifest::beside(&script).unwrap(), None);
        std::fs::write(dir.join(FILE_NAME), r#"{ "queueable": true }"#).unwrap();
        assert!(Manifest::beside(&script).unwrap().unwrap().queueable);
        std::fs::write(dir.join(FILE_NAME), "{").unwrap();
        assert!(Manifest::beside(&script).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
