use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

pub const API: &str = "https://prices.runescape.wiki/api/v2/osrs";
const LATEST_EVERY: Duration = Duration::from_secs(10 * 60);
const MAPPING_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(60);
const TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Id(i32),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Price {
    pub id: i32,
    pub name: String,
    pub members: bool,
    pub value: Option<i64>,
    pub high_alch: Option<i64>,
    pub low_alch: Option<i64>,
    pub limit: Option<i64>,
    pub high: Option<i64>,
    pub high_time: Option<i64>,
    pub low: Option<i64>,
    pub low_time: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Mapped {
    id: i32,
    name: String,
    #[serde(default)]
    members: bool,
    value: Option<i64>,
    highalch: Option<i64>,
    lowalch: Option<i64>,
    limit: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Latest {
    high: Option<i64>,
    high_time: Option<i64>,
    low: Option<i64>,
    low_time: Option<i64>,
}

#[derive(Deserialize)]
struct LatestBody {
    data: HashMap<i32, Latest>,
}

struct Mapping {
    items: HashMap<i32, Mapped>,
    by_name: HashMap<String, i32>,
}

fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn parse_mapping(body: &str) -> Result<Mapping, String> {
    let list: Vec<Mapped> = serde_json::from_str(body)
        .map_err(|e| format!("bad item mapping from the price API: {e}"))?;
    let mut by_name: HashMap<String, i32> = HashMap::new();
    for item in &list {
        by_name
            .entry(name_key(&item.name))
            .and_modify(|id| *id = (*id).min(item.id))
            .or_insert(item.id);
    }
    Ok(Mapping {
        items: list.into_iter().map(|item| (item.id, item)).collect(),
        by_name,
    })
}

fn parse_latest(body: &str) -> Result<HashMap<i32, Latest>, String> {
    serde_json::from_str::<LatestBody>(body)
        .map(|body| body.data)
        .map_err(|e| format!("bad latest prices from the price API: {e}"))
}

struct Feed<T> {
    data: Option<T>,
    next: Option<Instant>,
    error: String,
}

impl<T> Default for Feed<T> {
    fn default() -> Self {
        Self {
            data: None,
            next: None,
            error: String::new(),
        }
    }
}

impl<T> Feed<T> {
    fn get(
        &mut self,
        now: Instant,
        every: Duration,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<&T, String> {
        if self.next.is_none_or(|next| now >= next) {
            match load() {
                Ok(data) => {
                    self.data = Some(data);
                    self.next = Some(now + every);
                }
                Err(error) => {
                    self.error = error;
                    self.next = Some(now + RETRY_AFTER);
                }
            }
        }
        self.data.as_ref().ok_or_else(|| self.error.clone())
    }
}

#[derive(Default)]
struct Prices {
    mapping: Feed<Mapping>,
    latest: Feed<HashMap<i32, Latest>>,
}

impl Prices {
    fn lookup(
        &mut self,
        now: Instant,
        items: &[Item],
        fetch: &mut dyn FnMut(&str) -> Result<String, String>,
        names_of: impl FnOnce(&[i32]) -> Result<Vec<Option<String>>, String>,
    ) -> Result<Vec<Option<Price>>, String> {
        let mapping = self
            .mapping
            .get(now, MAPPING_EVERY, || parse_mapping(&fetch("mapping")?))?;
        let latest = self
            .latest
            .get(now, LATEST_EVERY, || parse_latest(&fetch("latest")?))?;

        let mut ids: Vec<Option<i32>> = items
            .iter()
            .map(|item| match item {
                Item::Id(id) => mapping.items.contains_key(id).then_some(*id),
                Item::Name(name) => mapping.by_name.get(&name_key(name)).copied(),
            })
            .collect();
        let unmapped: Vec<(usize, i32)> = items
            .iter()
            .zip(&ids)
            .enumerate()
            .filter_map(|(at, pair)| match pair {
                (Item::Id(id), None) => Some((at, *id)),
                _ => None,
            })
            .collect();
        if !unmapped.is_empty() {
            let wanted: Vec<i32> = unmapped.iter().map(|&(_, id)| id).collect();
            let names = names_of(&wanted)?;
            for (&(at, _), name) in unmapped.iter().zip(names) {
                ids[at] = name.and_then(|name| mapping.by_name.get(&name_key(&name)).copied());
            }
        }

        Ok(ids
            .into_iter()
            .map(|id| {
                let item = mapping.items.get(&id?)?;
                let trade = latest.get(&item.id).copied().unwrap_or_default();
                Some(Price {
                    id: item.id,
                    name: item.name.clone(),
                    members: item.members,
                    value: item.value,
                    high_alch: item.highalch,
                    low_alch: item.lowalch,
                    limit: item.limit,
                    high: trade.high,
                    high_time: trade.high_time,
                    low: trade.low,
                    low_time: trade.low_time,
                })
            })
            .collect())
    }
}

fn fetch(path: &str) -> Result<String, String> {
    let response = crate::http::request(&crate::http::Request {
        url: format!("{API}/{path}"),
        method: "GET".into(),
        headers: BTreeMap::new(),
        body: None,
        timeout_ms: TIMEOUT_MS,
    })
    .map_err(|e| format!("couldn't reach the price API: {e}"))?;
    if response.status != 200 {
        return Err(format!(
            "the price API answered {} for /{path}",
            response.status
        ));
    }
    Ok(response.body)
}

static PRICES: Mutex<Option<Prices>> = Mutex::new(None);

pub fn lookup(
    items: &[Item],
    names_of: impl FnOnce(&[i32]) -> Result<Vec<Option<String>>, String>,
) -> Result<Vec<Option<Price>>, String> {
    PRICES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(Prices::default)
        .lookup(Instant::now(), items, &mut fetch, names_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPPING: &str = r#"[
        {"examine":"x","id":4151,"members":true,"lowalch":48000,"limit":70,"value":120001,"highalch":72000,"icon":"x.png","name":"Abyssal whip"},
        {"examine":"x","id":1333,"members":false,"lowalch":12800,"limit":70,"value":32000,"highalch":19200,"icon":"x.png","name":"Rune scimitar"},
        {"examine":"x","id":34762,"members":true,"icon":"x.png","name":"Amulet of sight"},
        {"examine":"x","id":20000,"members":false,"icon":"x.png","name":"Rune scimitar"}
    ]"#;
    const LATEST: &str = r#"{"data":{
        "4151":{"high":685000,"highTime":1794395480,"low":666749,"lowTime":1794395601},
        "1333":{"high":15100,"highTime":1794395000,"low":null,"lowTime":null}
    }}"#;

    struct Api {
        calls: Vec<String>,
        fail: bool,
    }

    impl Api {
        fn new() -> Self {
            Self {
                calls: Vec::new(),
                fail: false,
            }
        }

        fn fetch(&mut self, path: &str) -> Result<String, String> {
            self.calls.push(path.to_owned());
            if self.fail {
                return Err("offline".into());
            }
            Ok(match path {
                "mapping" => MAPPING,
                "latest" => LATEST,
                _ => unreachable!(),
            }
            .to_owned())
        }
    }

    fn no_names(ids: &[i32]) -> Result<Vec<Option<String>>, String> {
        Ok(vec![None; ids.len()])
    }

    fn ask(
        prices: &mut Prices,
        api: &mut Api,
        now: Instant,
        items: &[Item],
    ) -> Result<Vec<Option<Price>>, String> {
        prices.lookup(now, items, &mut |path| api.fetch(path), no_names)
    }

    #[test]
    fn finds_items_by_id_and_by_name_in_any_case() {
        let (mut prices, mut api, now) = (Prices::default(), Api::new(), Instant::now());
        let found = ask(
            &mut prices,
            &mut api,
            now,
            &[
                Item::Id(4151),
                Item::Name("  RUNE scimitar ".into()),
                Item::Name("Nothing".into()),
                Item::Id(1),
                Item::Name("Amulet of sight".into()),
            ],
        )
        .unwrap();
        let whip = found[0].as_ref().unwrap();
        assert_eq!(
            (
                whip.high,
                whip.low,
                whip.high_alch,
                whip.limit,
                whip.members
            ),
            (Some(685000), Some(666749), Some(72000), Some(70), true)
        );
        let scimitar = found[1].as_ref().unwrap();
        assert_eq!(
            (scimitar.id, scimitar.high, scimitar.low, scimitar.low_time),
            (1333, Some(15100), None, None),
            "a shared name resolves to the lowest id"
        );
        assert_eq!((&found[2], &found[3]), (&None, &None));
        let amulet = found[4].as_ref().unwrap();
        assert_eq!(
            (amulet.high, amulet.high_alch, amulet.limit),
            (None, None, None)
        );
    }

    #[test]
    fn an_unmapped_id_is_found_through_its_name() {
        let (mut prices, mut api) = (Prices::default(), Api::new());
        let mut asked = Vec::new();
        let found = prices
            .lookup(
                Instant::now(),
                &[Item::Id(4152), Item::Id(4151), Item::Id(99)],
                &mut |path| api.fetch(path),
                |ids| {
                    asked = ids.to_vec();
                    Ok(vec![Some("Abyssal whip".into()), None])
                },
            )
            .unwrap();
        assert_eq!(asked, [4152, 99], "only ids the API doesn't know are named");
        assert_eq!(found[0].as_ref().map(|p| p.id), Some(4151));
        assert_eq!(found[1].as_ref().map(|p| p.id), Some(4151));
        assert_eq!(found[2], None);
    }

    #[test]
    fn caches_each_feed_for_its_own_time() {
        let (mut prices, mut api, start) = (Prices::default(), Api::new(), Instant::now());
        let items = [Item::Id(4151)];
        for later in [Duration::ZERO, LATEST_EVERY - Duration::from_secs(1)] {
            ask(&mut prices, &mut api, start + later, &items).unwrap();
        }
        assert_eq!(api.calls, ["mapping", "latest"]);
        ask(&mut prices, &mut api, start + LATEST_EVERY, &items).unwrap();
        assert_eq!(api.calls, ["mapping", "latest", "latest"]);
        ask(&mut prices, &mut api, start + MAPPING_EVERY, &items).unwrap();
        assert_eq!(
            api.calls,
            ["mapping", "latest", "latest", "mapping", "latest"]
        );
    }

    #[test]
    fn a_failed_refresh_keeps_the_old_prices_and_waits_before_trying_again() {
        let (mut prices, mut api, start) = (Prices::default(), Api::new(), Instant::now());
        let items = [Item::Id(4151)];
        ask(&mut prices, &mut api, start, &items).unwrap();
        api.fail = true;
        let stale = start + LATEST_EVERY;
        let kept = ask(&mut prices, &mut api, stale, &items).unwrap();
        assert_eq!(kept[0].as_ref().and_then(|p| p.high), Some(685000));
        ask(&mut prices, &mut api, stale + RETRY_AFTER / 2, &items).unwrap();
        assert_eq!(api.calls.len(), 3, "no second try within RETRY_AFTER");
        ask(&mut prices, &mut api, stale + RETRY_AFTER, &items).unwrap();
        assert_eq!(api.calls.len(), 4);
    }

    #[test]
    fn with_nothing_fetched_yet_a_failure_is_an_error_and_is_not_retried_at_once() {
        let (mut prices, mut api, start) = (Prices::default(), Api::new(), Instant::now());
        api.fail = true;
        let items = [Item::Id(4151)];
        assert_eq!(
            ask(&mut prices, &mut api, start, &items),
            Err("offline".into())
        );
        assert_eq!(
            ask(
                &mut prices,
                &mut api,
                start + Duration::from_secs(1),
                &items
            ),
            Err("offline".into())
        );
        assert_eq!(api.calls, ["mapping"]);
        api.fail = false;
        assert!(ask(&mut prices, &mut api, start + RETRY_AFTER, &items).is_ok());
    }

    #[test]
    fn bad_bodies_are_errors() {
        assert!(parse_mapping("{}").is_err());
        assert!(parse_latest("[]").is_err());
        assert!(parse_latest(r#"{"data":{"x":{}}}"#).is_err());
    }
}
