use serde::{Deserialize, Serialize};

pub const MICROCREDITS_PER_CREDIT: f64 = 1_000_000.0;

pub fn credits_text(microcredits: f64) -> String {
    let six = format!("{:.6}", microcredits / MICROCREDITS_PER_CREDIT);
    let (int, frac) = six.split_once('.').unwrap_or((&six, ""));
    match frac.trim_end_matches('0') {
        "" => int.to_owned(),
        frac => format!("{int}.{frac:0<2}"),
    }
}

pub fn credits_label(microcredits: f64) -> String {
    format!("{}\u{a0}CREDIT", credits_text(microcredits))
}

pub fn rate_label(microcredits: f64) -> String {
    let two = format!("{:.2}", microcredits / MICROCREDITS_PER_CREDIT);
    let amount = two.strip_suffix(".00").unwrap_or(&two);
    format!("{amount}\u{a0}CREDIT")
}

pub fn balance_label(microcredits: f64) -> String {
    format!("{:.6}\u{a0}CREDIT", microcredits / MICROCREDITS_PER_CREDIT)
}

pub fn fee_per_hour(fee: i64, minutes_per_month: i64) -> f64 {
    fee as f64 * 60.0 / minutes_per_month.max(1) as f64
}

pub fn per_month(price: i64, fee: i64, minutes_per_month: i64) -> f64 {
    price as f64 * minutes_per_month as f64 / 60.0 + fee as f64
}

fn default_minutes_per_month() -> i64 {
    30 * 24 * 60
}

pub const API_VERSION: u32 = 3;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Me {
    pub username: String,
    #[serde(default)]
    pub role: String,
    pub microcredits: i64,
    pub min_balance_minutes: i64,
    #[serde(default = "default_minutes_per_month")]
    pub minutes_per_month: i64,
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
    #[serde(default)]
    pub community: bool,
    #[serde(default)]
    pub granted: bool,
}

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

pub fn normalize_share_code(input: &str) -> Option<String> {
    let code = input.trim().to_ascii_lowercase();
    (code.len() == 64 && code.bytes().all(|b| b.is_ascii_hexdigit())).then_some(code)
}

impl Me {
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

    pub fn blocked(&self, microcredits: i64, min_balance_minutes: i64) -> Option<String> {
        if self.revision.is_none() {
            return Some("This script has no live revision yet.".into());
        }
        if self.can_access {
            return None;
        }
        let required = (self.price * min_balance_minutes + 59) / 60;
        Some(if microcredits < required {
            format!(
                "Needs a balance of at least {} to start ({} available).",
                credits_label(required as f64),
                balance_label(microcredits as f64)
            )
        } else {
            "You don't have access to this script.".into()
        })
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
pub struct Quote {
    pub price: i64,
    pub rank: i64,
    pub fee: i64,
    pub required: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StartedSession {
    pub session_id: String,
    pub revision: i64,
    pub price: i64,
    pub entry: String,
    pub files: Vec<SessionFile>,
    #[serde(default)]
    pub key: Option<String>,
    pub heartbeat_seconds: u64,
    #[serde(default)]
    pub rank: i64,
    #[serde(default)]
    pub fee: i64,
}

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
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub microcredits: Option<i64>,
    #[serde(default)]
    pub rank: Option<i64>,
    #[serde(default)]
    pub fee: Option<i64>,
    #[serde(default)]
    pub fee_charged: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Ended {
    #[serde(default)]
    pub charged: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RecentSession {
    pub session_id: String,
    pub script_id: String,
    pub title: String,
    pub url: String,
    pub revision: i64,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub score: Option<u8>,
    #[serde(default)]
    pub reports: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RecentSessions {
    pub sessions: Vec<RecentSession>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Report {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<serde_json::Value>,
    pub diagnostics: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Scored {
    pub score: u8,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Reported {
    pub report_id: String,
}

pub fn end_reason_message(reason: Option<&str>) -> &'static str {
    match reason {
        Some("insufficient_credits") => "Out of CREDIT: add funds to keep running.",
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
            r#"{"username":"dinis","avatar_url":null,"role":"user","microcredits":12500000,"min_balance_minutes":5,"api_version":3}"#,
        )
        .unwrap();
        assert_eq!(me.microcredits, 12_500_000);
        assert_eq!(credits_text(me.microcredits as f64), "12.50");
        assert_eq!(me.minutes_per_month, 43_200);
    }

    #[test]
    fn credits_drop_whole_decimals_and_show_two_to_six_otherwise() {
        assert_eq!(credits_text(12_500_000.0), "12.50");
        assert_eq!(credits_text(1_000.0), "0.001");
        assert_eq!(credits_text(1_234_500.0), "1.2345");
        assert_eq!(credits_text(70_000.0 / 60.0), "0.001167");
        assert_eq!(credits_text(1.0), "0.000001");
        assert_eq!(credits_text(0.0), "0");
        assert_eq!(credits_text(1_000_000.0), "1");
        assert_eq!(credits_text(-3_000_000.0), "-3");
        assert_eq!(credits_text(-50_000.0), "-0.05");
        assert_eq!(credits_text(fee_per_hour(3_000_000, 43_200)), "0.004167");
        assert_eq!(credits_label(12_500_000.0), "12.50\u{a0}CREDIT");
        assert_eq!(balance_label(12_500_000.0), "12.500000\u{a0}CREDIT");
        assert_eq!(balance_label(1_000_000.0), "1.000000\u{a0}CREDIT");
        assert_eq!(balance_label(1_234_567.0), "1.234567\u{a0}CREDIT");
        assert_eq!(balance_label(-50_000.0), "-0.050000\u{a0}CREDIT");
    }

    #[test]
    fn rates_show_at_most_two_decimals() {
        assert_eq!(rate_label(180_000.0), "0.18\u{a0}CREDIT");
        assert_eq!(rate_label(1_234_567.0), "1.23\u{a0}CREDIT");
        assert_eq!(rate_label(129_600_000.0), "129.60\u{a0}CREDIT");
        assert_eq!(rate_label(132_604_167.0), "132.60\u{a0}CREDIT");
        assert_eq!(rate_label(2_000_000.0), "2\u{a0}CREDIT");
        assert_eq!(rate_label(1_996_000.0), "2\u{a0}CREDIT");
        assert_eq!(rate_label(0.0), "0\u{a0}CREDIT");
    }

    #[test]
    fn a_month_costs_the_price_for_every_minute_plus_the_fee() {
        assert_eq!(credits_text(per_month(180_000, 0, 43_200)), "129.60");
        assert_eq!(
            credits_text(per_month(180_000, 3_000_000, 43_200)),
            "132.60"
        );
    }

    #[test]
    fn parses_scripts_and_gates_running() {
        let list: ScriptList = serde_json::from_str(
            r#"{"scripts":[{"id":"a1","url":"tempoross","title":"Tempoross","price":180000,"revision":4,"can_access":true,"author":"x","online":12,"future_field":1}]}"#,
        )
        .unwrap();
        let script = &list.scripts[0];
        assert_eq!(script.title, "Tempoross");
        assert!(!script.is_free());
        assert_eq!(credits_text(script.price as f64), "0.18");
        assert_eq!(script.blocked(1000, 5), None);
        assert_eq!(script.blocked(0, 5), None);
        let mut locked = script.clone();
        locked.can_access = false;
        assert!(locked.blocked(14_999, 5).unwrap().contains("0.015\u{a0}"));
        assert!(locked.blocked(15_000, 5).unwrap().contains("access"));
        locked.price = 70_000;
        assert!(locked.blocked(1_166, 1).unwrap().contains("0.001167\u{a0}"));
        assert!(locked.blocked(1_167, 1).unwrap().contains("access"));
        locked.can_access = true;
        locked.revision = None;
        assert!(
            locked
                .blocked(1_000_000_000, 5)
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
            role: role.into(),
            microcredits: 0,
            min_balance_minutes: 1,
            minutes_per_month: 43_200,
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
        assert!(end_reason_message(beat.reason.as_deref()).contains("add funds"));
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
