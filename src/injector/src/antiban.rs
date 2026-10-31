//! The global break and sleep config. The loader owns and edits it and sends it to every script it starts, and again
//! whenever it changes; the lib's `Antiban` plans its breaks and sleeps from it. A field left out is "Auto": the lib
//! picks it from the account's biometrics.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A line the loader writes to the script's stdin: the config as JSON. Sent before the script starts, and later at any time.
pub const PREFIX: &str = "\u{1e}ANTIBAN ";

/// A line the loader writes to the script's stdin when the user presses "Skip next" on a queued break or sleep:
/// `break 2` or `sleep 1`, the kind and the position in the config (counting from 0).
pub const SKIP_PREFIX: &str = "\u{1e}SKIP ";

const MINUTE: f64 = 60_000.0;

/// A break taken about every `interval` ms, for about `length` ms. Times are in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    pub interval: f64,
    pub length: f64,
    /// Spread of the interval and length, as a fraction. Auto when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomness: Option<f64>,
    /// Chance of logging out for it, 0 to 1. Auto when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logout_chance: Option<f64>,
}

/// A long break taken at a time of day.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sleep {
    /// "HH:MM" or "HH:MM:SS" on this computer's clock. Auto (the account's usual bedtime) when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Auto (the account's usual sleep) when left out.
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
    /// Off means no breaks, though sleeps may still be taken.
    pub breaks_enabled: bool,
    /// Off means no sleeps, though breaks may still be taken.
    pub sleeps_enabled: bool,
    pub breaks: Vec<Break>,
    pub sleeps: Vec<Sleep>,
}

fn timed(minutes: f64, length: f64, randomness: f64, logout_chance: Option<f64>) -> Break {
    Break { interval: minutes * MINUTE, length: length * MINUTE, randomness: Some(randomness), logout_chance }
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
            sleeps: vec![Sleep { randomness: Some(0.1), ..Sleep::default() }],
        }
    }
}

impl Config {
    /// Whether every number is usable: finite, intervals and lengths above 0, chances and randomness from 0 to 1, and
    /// sleep times that read as a time of day.
    pub fn validate(&self) -> Result<(), String> {
        let fraction = |what: &str, value: Option<f64>| match value {
            Some(v) if !(0.0..=1.0).contains(&v) => Err(format!("{what} must be from 0 to 1")),
            _ => Ok(()),
        };
        let positive = |what: &str, value: f64| {
            if value.is_finite() && value > 0.0 { Ok(()) } else { Err(format!("{what} must be more than 0")) }
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

/// Seconds since midnight of "HH:MM" or "HH:MM:SS".
pub fn parse_time(text: &str) -> Option<u32> {
    let mut parts = text.split(':');
    let mut next = |max: u32| parts.next().and_then(|p| p.parse::<u32>().ok()).filter(|v| *v < max);
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
    format!("{PREFIX}{}", serde_json::to_string(config).unwrap_or_default())
}

/// The config of a `PREFIX` line, without the prefix; none if it isn't a valid config.
pub fn parse(payload: &str) -> Option<Config> {
    let config: Config = serde_json::from_str(payload.trim()).ok()?;
    config.validate().ok()?;
    Some(config)
}

static CURRENT: std::sync::Mutex<(u64, Option<Config>)> = std::sync::Mutex::new((0, None));

fn lock() -> std::sync::MutexGuard<'static, (u64, Option<Config>)> {
    CURRENT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Makes `config` the one the script plans from; the version goes up so the lib notices.
pub fn set(config: Config) {
    let mut current = lock();
    current.0 += 1;
    current.1 = Some(config);
}

/// The config the loader last sent (or the default when it sent none) with a version that changes with every `set`.
pub fn current() -> (u64, Config) {
    let current = lock();
    (current.0, current.1.clone().unwrap_or_default())
}

/// `current()` as the table the script reads: the config's fields and `version`.
pub fn current_value() -> Value {
    let (version, config) = current();
    let mut value = serde_json::to_value(config).unwrap_or_else(|_| json!({}));
    value["version"] = json!(version);
    value
}

/// A queued break or sleep to skip once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skip {
    /// `"break"` or `"sleep"`.
    pub kind: String,
    pub index: u32,
}

static SKIPS: std::sync::Mutex<Vec<Skip>> = std::sync::Mutex::new(Vec::new());

pub fn skip_line(kind: &str, index: u32) -> String {
    format!("{SKIP_PREFIX}{kind} {index}")
}

/// The skip of a `SKIP_PREFIX` line, without the prefix.
pub fn parse_skip(payload: &str) -> Option<Skip> {
    let (kind, index) = payload.trim().split_once(' ')?;
    matches!(kind, "break" | "sleep").then_some(())?;
    Some(Skip { kind: kind.to_owned(), index: index.trim().parse().ok()? })
}

pub fn push_skip(skip: Skip) {
    SKIPS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(skip);
}

/// The skips asked for since the last call, oldest first.
pub fn take_skips() -> Vec<Skip> {
    std::mem::take(&mut *SKIPS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_valid_and_survives_a_line() {
        let config = Config::default();
        assert!(config.validate().is_ok());
        assert_eq!(parse(line(&config).strip_prefix(PREFIX).unwrap()), Some(config));
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
        assert!(parse("{\"breaks\":[{\"interval\":1,\"length\":1,\"logout_chance\":2}]}").is_none());
        assert!(parse("{\"sleeps\":[{\"time\":\"25:00\"}]}").is_none());
        assert!(parse("not json").is_none());
        assert!(parse("{\"breaks_enabled\":false}").is_some());
    }

    #[test]
    fn skips_round_trip_and_queue_in_order() {
        let line = skip_line("sleep", 1);
        assert_eq!(parse_skip(line.strip_prefix(SKIP_PREFIX).unwrap()), Some(Skip { kind: "sleep".into(), index: 1 }));
        assert!(parse_skip("nap 1").is_none());
        assert!(parse_skip("break x").is_none());
        assert!(parse_skip("break").is_none());
        push_skip(parse_skip("break 0").unwrap());
        push_skip(parse_skip("break 2").unwrap());
        assert_eq!(take_skips().iter().map(|s| s.index).collect::<Vec<_>>(), [0, 2]);
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
