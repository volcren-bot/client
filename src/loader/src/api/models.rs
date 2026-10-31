use serde::{Deserialize, Serialize};

/// Credits per euro cent factor: 10_000 credits are 1 EUR.
pub const CREDITS_PER_EUR: f64 = 10_000.0;

pub fn credits_to_eur(credits: i64) -> f64 {
    credits as f64 / CREDITS_PER_EUR
}

/// Euros per hour for a price in credits per minute.
pub fn eur_per_hour(credits_per_minute: i64) -> f64 {
    credits_per_minute as f64 * 60.0 / CREDITS_PER_EUR
}

/// The launcher API contract version this build speaks; `GET /me` reports the server's.
pub const API_VERSION: u32 = 2;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Me {
    pub username: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub role: String,
    pub credits: i64,
    pub min_balance_minutes: i64,
    pub api_version: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StoreScript {
    pub id: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub stage: String,
    /// Credits per minute; 0 is free.
    #[serde(default)]
    pub price: i64,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub can_access: bool,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub online: i64,
    /// Written by someone below scripter: not reviewed by staff, and the user is warned before it runs.
    #[serde(default)]
    pub community: bool,
    /// Came from a share code, so the account can remove it again.
    #[serde(default)]
    pub granted: bool,
}

/// The answer to a redeemed share code.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Redeemed {
    pub script_id: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Removed {
    #[serde(default)]
    pub removed: bool,
}

/// A share code is 64 hex digits; whitespace and case are forgiven, anything else is refused before asking the server.
pub fn normalize_share_code(input: &str) -> Option<String> {
    let code = input.trim().to_ascii_lowercase();
    (code.len() == 64
        && code
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(code)
}

impl Me {
    /// Testers and above can list every community script (`GET /launcher/scripts?community=true`).
    pub fn can_browse_community(&self) -> bool {
        matches!(
            self.role.as_str(),
            "tester" | "scripter" | "moderator" | "administrator"
        )
    }
}

impl StoreScript {
    pub fn is_free(&self) -> bool {
        self.price == 0
    }

    /// Why the Run button is disabled, or `None` when the script can start with `credits` in the account.
    pub fn blocked(&self, credits: i64, min_balance_minutes: i64) -> Option<String> {
        if !self.can_access {
            return Some("You don't have access to this script.".into());
        }
        if self.revision.is_none() {
            return Some("This script has no live revision yet.".into());
        }
        let required = self.price * min_balance_minutes;
        (credits < required)
            .then(|| format!("Needs at least {required} credits to start ({credits} available)."))
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScriptList {
    pub scripts: Vec<StoreScript>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SessionFile {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StartedSession {
    pub session_id: String,
    pub revision: i64,
    pub price: i64,
    /// The file to run; one of `files`.
    pub entry: String,
    pub files: Vec<SessionFile>,
    /// The key the files were encrypted with, as 64 hex digits. `None` for a revision published before files were encrypted.
    #[serde(default)]
    pub key: Option<String>,
    pub heartbeat_seconds: u64,
}

/// Stats gained since the previous heartbeat. `minutes` is measured by the loader's session clock, never reported by the script.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Stats {
    pub experience: i64,
    pub gold: i64,
    pub minutes: f64,
    pub custom: Vec<i64>,
}

impl Stats {
    pub fn is_empty(&self) -> bool {
        self.experience == 0 && self.gold == 0 && self.custom.iter().all(|&value| value == 0)
    }

    /// Adds another report to this one, summing custom trackers position by position.
    pub fn add(&mut self, experience: i64, gold: i64, custom: &[i64]) {
        self.experience = self.experience.saturating_add(experience);
        self.gold = self.gold.saturating_add(gold);
        if self.custom.len() < custom.len() {
            self.custom.resize(custom.len(), 0);
        }
        for (total, value) in self.custom.iter_mut().zip(custom) {
            *total = total.saturating_add(*value);
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Heartbeat {
    pub alive: bool,
    /// Why the session ended when `alive` is false: `insufficient_credits`, `timeout` or `stopped`.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub credits: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Ended {
    #[serde(default)]
    pub charged: i64,
}

/// Why a session ended, as the UI says it.
pub fn end_reason_message(reason: Option<&str>) -> &'static str {
    match reason {
        Some("insufficient_credits") => "Out of credits: top up to keep running.",
        Some("timeout") => "The session timed out.",
        Some("stopped") => "The session was stopped.",
        _ => "The session ended.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_me() {
        let me: Me = serde_json::from_str(
            r#"{"username":"dinis","avatar_url":null,"role":"user","credits":125000,"min_balance_minutes":5,"api_version":1}"#,
        )
        .unwrap();
        assert_eq!(me.credits, 125_000);
        assert_eq!(credits_to_eur(me.credits), 12.5);
        assert!(me.avatar_url.is_none());
    }

    #[test]
    fn parses_scripts_and_gates_running() {
        let list: ScriptList = serde_json::from_str(
            r#"{"scripts":[{"id":"a1","url":"tempoross","title":"Tempoross","price":30,"revision":4,"can_access":true,"author":"x","online":12,"future_field":1}]}"#,
        )
        .unwrap();
        let script = &list.scripts[0];
        assert_eq!(script.title, "Tempoross");
        assert!(!script.is_free());
        assert_eq!(eur_per_hour(script.price), 0.18);
        assert_eq!(script.blocked(1000, 5), None);
        assert!(script.blocked(100, 5).unwrap().contains("150"));
        let mut locked = script.clone();
        locked.can_access = false;
        assert!(locked.blocked(1_000_000, 5).unwrap().contains("access"));
        locked.can_access = true;
        locked.revision = None;
        assert!(
            locked
                .blocked(1_000_000, 5)
                .unwrap()
                .contains("live revision")
        );
    }

    #[test]
    fn community_flags_default_to_off_for_older_servers() {
        let list: ScriptList = serde_json::from_str(r#"{"scripts":[{"id":"a","url":"a","title":"A"},{"id":"b","url":"b","title":"B","community":true,"granted":true}]}"#).unwrap();
        assert!(!list.scripts[0].community && !list.scripts[0].granted);
        assert!(list.scripts[1].community && list.scripts[1].granted);
    }

    #[test]
    fn only_testers_and_above_browse_community_scripts() {
        let me = |role: &str| Me {
            username: "x".into(),
            avatar_url: None,
            role: role.into(),
            credits: 0,
            min_balance_minutes: 1,
            api_version: API_VERSION,
        };
        assert!(!me("user").can_browse_community());
        assert!(!me("contributor").can_browse_community());
        assert!(me("tester").can_browse_community());
        assert!(me("administrator").can_browse_community());
        assert!(!me("").can_browse_community());
    }

    #[test]
    fn share_codes_are_checked_before_the_server_is_asked() {
        let code = "0123456789abcdef".repeat(4);
        assert_eq!(normalize_share_code(&code), Some(code.clone()));
        assert_eq!(
            normalize_share_code(&format!("  {}\n", code.to_uppercase())),
            Some(code.clone())
        );
        assert_eq!(normalize_share_code(&code[1..]), None);
        assert_eq!(normalize_share_code(&format!("{code}0")), None);
        assert_eq!(normalize_share_code(&format!("g{}", &code[1..])), None);
        assert_eq!(normalize_share_code(""), None);
    }

    #[test]
    fn parses_a_started_session() {
        let session: StartedSession = serde_json::from_str(
            r#"{"session_id":"s","revision":3,"price":0,"entry":"main.luau","files":[{"name":"main.luau","url":"https://x/y"},{"name":"util.luau","url":"https://x/z","sha256":"ab"}],"heartbeat_seconds":60}"#,
        )
        .unwrap();
        assert_eq!(session.files.len(), 2);
        assert_eq!(session.files[1].sha256.as_deref(), Some("ab"));
    }

    #[test]
    fn heartbeat_reasons_parse_and_read_well() {
        let beat: Heartbeat =
            serde_json::from_str(r#"{"alive":false,"reason":"insufficient_credits"}"#).unwrap();
        assert!(!beat.alive);
        assert!(end_reason_message(beat.reason.as_deref()).contains("top up"));
    }

    #[test]
    fn stats_accumulate_and_serialize() {
        let mut stats = Stats::default();
        assert!(stats.is_empty());
        stats.add(10, 5, &[1]);
        stats.add(20, 0, &[1, 7]);
        assert_eq!(
            stats,
            Stats {
                experience: 30,
                gold: 5,
                minutes: 0.0,
                custom: vec![2, 7]
            }
        );
        assert_eq!(
            serde_json::to_value(&stats).unwrap()["custom"],
            serde_json::json!([2, 7])
        );
    }
}
