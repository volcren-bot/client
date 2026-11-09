use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PREFIX: &str = "\u{1e}ANTIBAN ";

pub const SKIP_PREFIX: &str = "\u{1e}SKIP ";

const MINUTE: f64 = 60_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub interval: f64,
    pub length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logout_chance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sleep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logout_chance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub breaks_enabled: bool,
    pub sleeps_enabled: bool,
    pub breaks: Vec<Break>,
    pub sleeps: Vec<Sleep>,
}

fn timed(minutes: f64, length: f64, randomness: f64, logout_chance: Option<f64>) -> Break {
    Break {
        interval: minutes * MINUTE,
        length: length * MINUTE,
        randomness: Some(randomness),
        logout_chance,
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            breaks_enabled: true,
            sleeps_enabled: true,
            breaks: vec![
                timed(30.0, 2.0, 0.33, Some(0.01)),
                timed(45.0, 5.0, 0.33, Some(0.02)),
                timed(60.0, 10.0, 0.33, None),
                timed(250.0, 20.0, 0.33, None),
            ],
            sleeps: vec![Sleep {
                randomness: Some(0.1),
                ..Sleep::default()
            }],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        let fraction = |what: &str, value: Option<f64>| match value {
            Some(v) if !(0.0..=1.0).contains(&v) => Err(format!("{what} must be from 0 to 1")),
            _ => Ok(()),
        };
        let positive = |what: &str, value: f64| {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(format!("{what} must be more than 0"))
            }
        };
        for (i, b) in self.breaks.iter().enumerate() {
            let at = |what: &str| format!("break {}: {what}", i + 1);
            positive(&at("interval"), b.interval)?;
            positive(&at("length"), b.length)?;
            fraction(&at("randomness"), b.randomness)?;
            fraction(&at("logout chance"), b.logout_chance)?;
        }
        for (i, s) in self.sleeps.iter().enumerate() {
            let at = |what: &str| format!("sleep {}: {what}", i + 1);
            if let Some(length) = s.length {
                positive(&at("length"), length)?;
            }
            if let Some(time) = &s.time
                && parse_time(time).is_none()
            {
                return Err(at("time must be HH:MM or HH:MM:SS"));
            }
            fraction(&at("randomness"), s.randomness)?;
            fraction(&at("logout chance"), s.logout_chance)?;
        }
        Ok(())
    }
}

pub fn parse_time(text: &str) -> Option<u32> {
    let mut parts = text.split(':');
    let mut next = |max: u32| {
        parts
            .next()
            .and_then(|p| p.parse::<u32>().ok())
            .filter(|v| *v < max)
    };
    let (h, m) = (next(24)?, next(60)?);
    let s = match parts.next() {
        Some(p) => p.parse::<u32>().ok().filter(|v| *v < 60)?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(h * 3600 + m * 60 + s)
}

pub fn line(config: &Config) -> String {
    format!(
        "{PREFIX}{}",
        serde_json::to_string(config).unwrap_or_default()
    )
}

pub fn parse(payload: &str) -> Option<Config> {
    let config: Config = serde_json::from_str(payload.trim()).ok()?;
    config.validate().ok()?;
    Some(config)
}

static CURRENT: std::sync::Mutex<(u64, Option<Config>)> = std::sync::Mutex::new((0, None));

fn lock() -> std::sync::MutexGuard<'static, (u64, Option<Config>)> {
    CURRENT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn set(config: Config) {
    let mut current = lock();
    current.0 += 1;
    current.1 = Some(config);
}

pub fn current() -> (u64, Config) {
    let current = lock();
    (current.0, current.1.clone().unwrap_or_default())
}

pub fn current_value() -> Value {
    let (version, config) = current();
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| json!({}));
    value["version"] = json!(version);
    value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skip {
    pub kind: String,
    pub index: u32,
}

static SKIPS: std::sync::Mutex<Vec<Skip>> = std::sync::Mutex::new(Vec::new());

pub fn skip_line(kind: &str, index: u32) -> String {
    format!("{SKIP_PREFIX}{kind} {index}")
}

pub fn parse_skip(payload: &str) -> Option<Skip> {
    let (kind, index) = payload.trim().split_once(' ')?;
    matches!(kind, "break" | "sleep").then_some(())?;
    Some(Skip {
        kind: kind.to_owned(),
        index: index.trim().parse().ok()?,
    })
}

pub fn push_skip(skip: Skip) {
    SKIPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(skip);
}

pub fn take_skips() -> Vec<Skip> {
    std::mem::take(
        &mut *SKIPS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_valid_and_survives_a_line() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert_eq!(
            parse(line(&config).strip_prefix(PREFIX).unwrap()),
            Some(config)
        );
    }

    #[test]
    fn auto_fields_are_left_out_of_the_json() {
        let json = serde_json::to_value(Config::default()).unwrap();
        assert!(json["sleeps"][0].get("time").is_none());
        assert!(json["breaks"][2].get("logout_chance").is_none());
        assert_eq!(json["breaks"][0]["logout_chance"], 0.01);
    }

    #[test]
    fn bad_configs_are_refused() {
        assert!(parse("{\"breaks\":[{\"interval\":0,\"length\":1}]}").is_none());
        assert!(
            parse("{\"breaks\":[{\"interval\":1,\"length\":1,\"logout_chance\":2}]}").is_none()
        );
        assert!(parse("{\"sleeps\":[{\"time\":\"25:00\"}]}").is_none());
        assert!(parse("not json").is_none());
        assert!(parse("{\"breaks_enabled\":false}").is_some());
    }

    #[test]
    fn skips_round_trip_and_queue_in_order() {
        let line = skip_line("sleep", 1);
        assert_eq!(
            parse_skip(line.strip_prefix(SKIP_PREFIX).unwrap()),
            Some(Skip {
                kind: "sleep".into(),
                index: 1
            })
        );
        assert!(parse_skip("nap 1").is_none());
        assert!(parse_skip("break x").is_none());
        assert!(parse_skip("break").is_none());
        push_skip(parse_skip("break 0").unwrap());
        push_skip(parse_skip("break 2").unwrap());
        assert_eq!(
            take_skips().iter().map(|s| s.index).collect::<Vec<_>>(),
            [0, 2]
        );
        assert!(take_skips().is_empty());
    }

    #[test]
    fn times_of_day_parse() {
        assert_eq!(parse_time("01:30:45"), Some(5445));
        assert_eq!(parse_time("23:05"), Some(83100));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("1:2:3:4"), None);
        assert_eq!(parse_time("ab"), None);
    }
}
