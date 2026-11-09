use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PREFIX: &str = "\u{1e}TRANSPORTS ";

pub const SETTINGS_PREFIX: &str = "\u{1e}TRANSPORT_SETTINGS ";

pub const TYPES: &[(&str, &str)] = &[
    ("spells", "Teleport spells"),
    ("home_teleports", "Home teleports"),
    ("minigames", "Minigame teleports"),
    ("items", "Teleport items"),
    ("quetzal_whistle", "Quetzal whistle"),
    ("scroll_book", "Master scroll book"),
    ("quetzals", "Quetzals"),
    ("spirit_trees", "Spirit trees"),
    ("gnome_gliders", "Gnome gliders"),
    ("hot_air_balloons", "Hot air balloons"),
    ("fairy_rings", "Fairy rings"),
    ("magic_mushtrees", "Magic mushtrees"),
    ("magic_carpets", "Magic carpets"),
    ("canoes", "Canoes"),
    ("portals", "Portals"),
    ("wilderness_obelisks", "Wilderness obelisks"),
    ("house", "Player's house"),
    ("charter", "Charter ships"),
    ("ride", "Boats, ships and carts"),
    ("minecarts", "Lovakengj minecarts"),
    ("gates", "Toll gates"),
    ("levers", "Levers"),
    ("shortcuts", "Agility shortcuts"),
];

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub off: Vec<String>,
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings { off: Vec::new() });

pub fn settings_line(settings: &Settings) -> String {
    format!(
        "{SETTINGS_PREFIX}{}",
        serde_json::to_string(settings).expect("settings serialize")
    )
}

pub fn parse_settings(payload: &str) -> Option<Settings> {
    serde_json::from_str(payload.trim()).ok()
}

pub fn set_settings(settings: Settings) {
    *lock(&SETTINGS) = settings;
}

pub fn current_settings() -> Settings {
    lock(&SETTINGS).clone()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub level: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clause {
    pub id: u32,
    pub op: char,
    pub value: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub x: i32,
    pub y: i32,
    pub plane: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transport {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<Tile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<Tile>,
    pub name: String,
    pub duration: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wilderness: Option<u32>,
    pub skills: Vec<Skill>,
    pub quests: Vec<String>,
    pub varbits: Vec<Clause>,
    pub varps: Vec<Clause>,
    pub items: String,
    pub consumable: bool,
    #[serde(default)]
    pub menu: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemName {
    pub name: String,
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Data {
    pub source: String,
    pub transports: Vec<Transport>,
    #[serde(default)]
    pub items: Vec<ItemName>,
    #[serde(default)]
    pub climbs: Vec<Transport>,
}

pub const GATED_KIND: &str = "climb_gated";
pub const LINK_GATED_KIND: &str = "link_gated";
pub const SHORTCUT_GATED_KIND: &str = "shortcut_gated";
pub const SHORTCUT_KIND: &str = "agility_shortcuts";

pub const MAX_SHORTCUT_HOP: i32 = 70;
pub const MAX_LINK_HOP: i32 = 15;
pub const MAX_CLIMB_HOP: i32 = 350;

pub fn is_shortcut(t: &Transport) -> bool {
    clicks_object(t) && hop(t).is_some_and(|hop| hop <= MAX_SHORTCUT_HOP)
}

pub fn menu_option(menu: &str) -> Option<&str> {
    const SECOND_WORDS: &[&str] = &[
        "over", "on", "through", "up", "down", "to", "off", "in", "across",
    ];
    let first = menu.split_whitespace().next()?;
    let start = menu.find(first)?;
    let rest = &menu[start + first.len()..];
    match rest.split_whitespace().next() {
        Some(second) if SECOND_WORDS.contains(&second) => {
            let end = rest.find(second)? + second.len();
            Some(&menu[start..start + first.len() + end])
        }
        _ => Some(first),
    }
}

fn underground(tile: Tile) -> bool {
    tile.y >= crate::nav::UNDERGROUND
}

fn hop(t: &Transport) -> Option<i32> {
    let (o, d) = (t.origin?, t.destination?);
    let folded = |tile: Tile| {
        if underground(tile) {
            tile.y - crate::nav::UNDERGROUND
        } else {
            tile.y
        }
    };
    Some((o.x - d.x).abs().max((folded(o) - folded(d)).abs()))
}

fn clicks_object(t: &Transport) -> bool {
    !t.menu.trim().is_empty() && !t.menu.contains(" -> ")
}

const LEFT_OUT: &[&str] = &[
    "Enter Door of Dinh 29322",
    "Channel Teleport Platform 36198",
    "Use Spade -> Mole hill 12202",
    "Enter Cave 678",
    "Enter Centre of crop circle 24991",
    "Teleport Mage of Zamorak 2581",
    "Jump-into Whirlpool 25274",
    "Slide Slope 5015",
];

fn left_out(t: &Transport) -> bool {
    LEFT_OUT.contains(&t.menu.trim())
}

pub fn is_item_on(t: &Transport) -> bool {
    t.origin.is_some()
        && t.destination.is_some()
        && t.menu.starts_with("Use ")
        && t.menu.contains(" -> ")
        && !left_out(t)
}

pub fn is_far_hop(t: &Transport) -> bool {
    let Some(hop) = hop(t) else {
        return false;
    };
    clicks_object(t)
        && !t.menu.starts_with("Pay-toll")
        && !t.menu.starts_with("Join ")
        && !left_out(t)
        && hop
            > if changes_plane(t) {
                MAX_CLIMB_HOP
            } else {
                MAX_LINK_HOP
            }
}

pub fn is_gated_door(t: &Transport) -> bool {
    gated(t) && !changes_plane(t) && hop(t) == Some(1) && t.menu.starts_with("Open ")
}

pub fn is_link(t: &Transport) -> bool {
    !changes_plane(t)
        && hop(t).is_some_and(|hop| {
            (1..=MAX_LINK_HOP).contains(&hop) && !(hop == 1 && t.menu.starts_with("Open "))
        })
        && clicks_object(t)
        && !t.menu.starts_with("Pay-toll")
}

pub fn is_climb(t: &Transport) -> bool {
    (changes_plane(t) && !is_far_hop(t) && !t.menu.contains(" -> "))
        || is_link(t)
        || is_gated_door(t)
}

pub fn gated_kind(t: &Transport) -> &'static str {
    if t.kind == SHORTCUT_KIND {
        SHORTCUT_GATED_KIND
    } else if is_link(t) || is_gated_door(t) {
        LINK_GATED_KIND
    } else {
        GATED_KIND
    }
}

pub fn in_wilderness(x: i32, y: i32) -> bool {
    (2944..3392).contains(&x) && ((3523..3968).contains(&y) || (9918..10368).contains(&y))
}

pub fn touches_wilderness(t: &Transport) -> bool {
    [t.origin, t.destination]
        .into_iter()
        .flatten()
        .any(|tile| in_wilderness(tile.x, tile.y))
}

pub fn gated(t: &Transport) -> bool {
    !(t.skills.is_empty()
        && t.quests.is_empty()
        && t.varbits.is_empty()
        && t.varps.is_empty()
        && t.items.is_empty()
        && t.note.is_empty())
}

pub fn changes_plane(t: &Transport) -> bool {
    let (Some(o), Some(d)) = (t.origin, t.destination) else {
        return false;
    };
    o.plane != d.plane || underground(o) != underground(d)
}

pub fn climb_key(t: &Transport) -> String {
    let (o, d) = (
        t.origin.expect("a climb has an origin"),
        t.destination.expect("a climb has a destination"),
    );
    format!(
        "{},{},{}>{},{},{}|{}",
        o.x, o.y, o.plane, d.x, d.y, d.plane, t.menu
    )
}

fn tile(text: &str) -> Result<Option<Tile>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let bad = || format!("\"{text}\" isn't a tile (x y plane)");
    let mut parts = text.split_whitespace().map(|p| p.parse::<i32>());
    let (Some(Ok(x)), Some(Ok(y)), Some(Ok(plane)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    Ok(Some(Tile { x, y, plane }))
}

fn skills(text: &str) -> Result<Vec<Skill>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("Max "))
        .map(|s| {
            let (level, name) = s
                .split_once(' ')
                .ok_or_else(|| format!("\"{s}\" isn't a skill (level name)"))?;
            Ok(Skill {
                name: name.trim().to_owned(),
                level: level
                    .parse()
                    .map_err(|_| format!("\"{s}\" has no skill level"))?,
            })
        })
        .collect()
}

fn clauses(text: &str) -> Result<Vec<Clause>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let bad = || format!("\"{s}\" isn't a varbit or varp test");
            let at = s.find(['=', '>', '<', '&', '@']).ok_or_else(bad)?;
            Ok(Clause {
                id: s[..at].trim().parse().map_err(|_| bad())?,
                op: s[at..].chars().next().ok_or_else(bad)?,
                value: s[at + 1..].trim().parse().map_err(|_| bad())?,
            })
        })
        .collect()
}

pub fn parse_file(kind: &str, text: &str) -> Result<(Vec<Transport>, Vec<String>), String> {
    let mut lines = text.lines().enumerate();
    let header = loop {
        match lines.next() {
            Some((_, line)) if line.trim_start_matches(['#', ' ']).trim().is_empty() => {}
            Some((_, line)) => break line.trim_start_matches('#'),
            None => return Err(format!("{kind}: the file is empty")),
        }
    };
    let columns: Vec<&str> = header.split('\t').map(str::trim).collect();
    let column = |name: &str| columns.iter().position(|c| c.eq_ignore_ascii_case(name));
    let destination =
        column("Destination").ok_or_else(|| format!("{kind}: no Destination column"))?;
    let (origin, items, skill, quest, duration, info, wilderness, varbit, varp, consumable, menu) = (
        column("Origin"),
        column("Items"),
        column("Skills"),
        column("Quests"),
        column("Duration"),
        column("Display info"),
        column("Wilderness level"),
        column("Varbits"),
        column("VarPlayers"),
        column("Consumable"),
        column("menuOption menuTarget objectID"),
    );
    let (mut found, mut skipped) = (Vec::new(), Vec::new());
    let mut note = String::new();
    for (n, line) in lines {
        if let Some(comment) = line.strip_prefix('#') {
            note = if comment.contains('?') {
                comment.trim().to_owned()
            } else {
                String::new()
            };
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        let cell = |i: Option<usize>| i.and_then(|i| cells.get(i)).map_or("", |c| c.trim());
        let row = (|| -> Result<Transport, String> {
            Ok(Transport {
                kind: kind.to_owned(),
                origin: tile(cell(origin))?,
                destination: tile(cell(Some(destination)))?,
                name: cell(info).to_owned(),
                duration: match cell(duration) {
                    "" => 0,
                    d => d.parse().map_err(|_| format!("bad Duration \"{d}\""))?,
                },
                wilderness: match cell(wilderness) {
                    "" => None,
                    w => Some(
                        w.parse()
                            .map_err(|_| format!("bad Wilderness level \"{w}\""))?,
                    ),
                },
                skills: skills(cell(skill))?,
                quests: cell(quest)
                    .split(';')
                    .map(str::trim)
                    .filter(|q| !q.is_empty())
                    .map(str::to_owned)
                    .collect(),
                varbits: clauses(cell(varbit))?,
                varps: clauses(cell(varp))?,
                items: cell(items).to_owned(),
                consumable: cell(consumable).eq_ignore_ascii_case("t"),
                menu: cell(menu).to_owned(),
                note: note.clone(),
            })
        })();
        match row {
            Ok(t) => found.push(t),
            Err(why) => skipped.push(format!("{kind} line {}: {why}", n + 1)),
        }
    }
    Ok((found, skipped))
}

pub fn parse_item_variations(text: &str) -> Result<Vec<ItemName>, String> {
    let mut lines = text.lines();
    let header = lines
        .by_ref()
        .find(|l| !l.trim().is_empty())
        .ok_or("item-variations: the file is empty")?;
    let columns: Vec<&str> = header
        .trim_start_matches('#')
        .split('\t')
        .map(str::trim)
        .collect();
    let (Some(name), Some(ids)) = (
        columns.iter().position(|c| c.eq_ignore_ascii_case("name")),
        columns.iter().position(|c| c.eq_ignore_ascii_case("ids")),
    ) else {
        return Err("item-variations: no name and ids columns".into());
    };
    let mut found = Vec::new();
    for line in lines {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').collect();
        let id_list: Vec<u32> = cells
            .get(ids)
            .map_or("", |c| c.trim())
            .split(',')
            .filter_map(|id| id.trim().parse().ok())
            .collect();
        let name = cells.get(name).map_or("", |c| c.trim());
        if !name.is_empty() && !id_list.is_empty() {
            found.push(ItemName {
                name: name.to_owned(),
                ids: id_list,
            });
        }
    }
    Ok(found)
}

pub fn line(data: &Data) -> String {
    format!(
        "{PREFIX}{}",
        serde_json::to_string(data).expect("transports serialize")
    )
}

pub fn parse(payload: &str) -> Option<Data> {
    serde_json::from_str(payload.trim()).ok()
}

static CURRENT: Mutex<(u64, Option<Data>)> = Mutex::new((0, None));

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn set(data: Data) {
    let mut current = lock(&CURRENT);
    current.0 += 1;
    current.1 = Some(data);
}

pub fn version() -> u64 {
    lock(&CURRENT).0
}

pub fn with_climbs<R>(f: impl FnOnce(&[Transport]) -> R) -> R {
    let current = lock(&CURRENT);
    f(current.1.as_ref().map_or(&[], |d| d.climbs.as_slice()))
}

pub fn current_value() -> Value {
    let current = lock(&CURRENT);
    let data = current.1.as_ref();
    json!({
        "source": data.map_or("none", |d| d.source.as_str()),
        "transports": data.map_or(&[][..], |d| d.transports.as_slice()),
        "items": data.map_or(&[][..], |d| d.items.as_slice()),
        "version": current.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPELLS: &str = "# Destination\tItems\tSkills\tQuests\tDuration\tDisplay info\tWilderness level\tVarbits\tVarPlayers\n\
# Standard Spellbook - Varbit 4070=0\t\t\t\t\t\t\t\t\n\
\n\
3213 3424 0\tAIR_RUNE=3&&FIRE_RUNE=1&&LAW_RUNE=1\t25 Magic\t\t4\tVarrock Teleport\t20\t4070=0\t\n\
3165 3479 0\tAIR_RUNE=3\t25 Magic\t\t4\tVarrock Teleport: Grand Exchange\t20\t4070=0;4480=1\t\n";

    #[test]
    fn spells_parse() {
        let (rows, skipped) = parse_file("teleportation_spells", SPELLS).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(rows.len(), 2);
        let varrock = &rows[0];
        assert_eq!(varrock.origin, None);
        assert_eq!(
            varrock.destination,
            Some(Tile {
                x: 3213,
                y: 3424,
                plane: 0
            })
        );
        assert_eq!(varrock.name, "Varrock Teleport");
        assert_eq!(varrock.duration, 4);
        assert_eq!(varrock.wilderness, Some(20));
        assert_eq!(
            varrock.skills,
            [Skill {
                name: "Magic".into(),
                level: 25
            }]
        );
        assert_eq!(rows[1].varbits.len(), 2);
        assert_eq!(
            rows[1].varbits[1],
            Clause {
                id: 4480,
                op: '=',
                value: 1
            }
        );
    }

    #[test]
    fn the_bundled_item_teleports_parse() {
        let text = include_str!("../../loader/data/transports/teleportation_items.tsv");
        let (rows, skipped) = parse_file("teleportation_items", text).unwrap();
        assert!(skipped.is_empty(), "{skipped:?}");
        assert!(rows.len() > 400);
        assert!(
            rows.iter()
                .all(|t| t.origin.is_none() && t.destination.is_some())
        );
        let games = rows
            .iter()
            .find(|t| t.name == "Games necklace: Burthorpe")
            .unwrap();
        assert!(games.consumable);
        assert!(games.items.contains("3853=1"));
        assert_eq!(games.wilderness, Some(20));
    }

    #[test]
    fn home_teleport_has_a_cooldown_clause() {
        let text = "# Destination\tItems\tSkills\tQuests\tDuration\tDisplay info\tWilderness level\tVarbits\tVarPlayers\n\
3221 3218 0\t\t\t\t19\tLumbridge Home Teleport\t20\t4070=0\t4560&4;892@30\n";
        let (rows, skipped) = parse_file("teleportation_spells_home", text).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(
            rows[0].varps,
            [
                Clause {
                    id: 4560,
                    op: '&',
                    value: 4
                },
                Clause {
                    id: 892,
                    op: '@',
                    value: 30
                }
            ]
        );
    }

    #[test]
    fn a_bad_row_is_skipped_not_the_file() {
        let text = "Destination\tDisplay info\n1 2\tbroken\n3 4 0\tfine\n";
        let (rows, skipped) = parse_file("x", text).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "fine");
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].contains("line 2"));
    }

    #[test]
    fn a_file_without_the_columns_is_refused() {
        assert!(parse_file("x", "<html>rate limited</html>\n").is_err());
        assert!(parse_file("x", "").is_err());
        assert!(parse_file("x", "# only a comment\n").is_err());
    }

    #[test]
    fn item_groups_and_the_click_column_parse() {
        let items = parse_item_variations("#name\tids\tstaves\toffhands\nCOINS\t995\t\t\nAXE\t1351,1349\t\t\n\t5\t\t\nBAD\t\t\t\n").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].ids, [1351, 1349]);
        assert!(parse_item_variations("nonsense").is_err());
        let text = "# Origin\tDestination\tmenuOption menuTarget objectID\tItems\tDisplay info\n1455 2968 0\t2760 3238 0\tCharter Trader Crewmember 1330\tCOINS=2500\t1: Brimhaven\n";
        let (rows, skipped) = parse_file("charter_ships", text).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(rows[0].menu, "Charter Trader Crewmember 1330");
        assert_eq!(rows[0].items, "COINS=2500");
        assert_eq!(rows[0].note, "");
        let barrier = "Origin\tDestination\tmenuOption menuTarget objectID\n# Port Phasmatys (varbit:217=7?ECTO_TOKEN=2)\n3659 3509 0\t3659 3507 0\tPay-toll(2-Ecto) Energy Barrier 16105\n";
        let (rows, _) = parse_file("transports", barrier).unwrap();
        assert_eq!(rows[0].note, "Port Phasmatys (varbit:217=7?ECTO_TOKEN=2)");
        assert_eq!(rows[0].menu, "Pay-toll(2-Ecto) Energy Barrier 16105");
        assert!(rows[0].origin.is_some() && rows[0].destination.is_some());
    }

    #[test]
    fn a_menu_option_can_be_two_words() {
        assert_eq!(menu_option("Climb-up Rope 51647"), Some("Climb-up"));
        assert_eq!(
            menu_option("Climb over Broken cart 28931"),
            Some("Climb over")
        );
        assert_eq!(
            menu_option("Jump on Rubber cap mushroom 1"),
            Some("Jump on")
        );
        assert_eq!(
            menu_option("Swing across Monkeybars 23566"),
            Some("Swing across")
        );
        assert_eq!(menu_option("Open trapdoor 5"), Some("Open"));
        assert_eq!(menu_option("Open Tree Door 1967"), Some("Open"));
        assert_eq!(menu_option(""), None);
    }

    #[test]
    fn only_a_comment_with_a_requirement_is_kept_as_a_note() {
        let text = "Origin\tDestination\tmenuOption menuTarget objectID\n# Stairs\n1 2 0\t1 2 1\tClimb-up Stairs 1\n# Toll (varbit:217=7?ECTO_TOKEN=2)\n3 4 0\t3 6 0\tPay-toll Barrier 2\n# Doors\n5 6 0\t5 8 0\tOpen Door 3\n";
        let (rows, _) = parse_file("transports", text).unwrap();
        assert_eq!(rows[0].note, "");
        assert_eq!(rows[1].note, "Toll (varbit:217=7?ECTO_TOKEN=2)");
        assert_eq!(rows[2].note, "", "a later comment ends it");
    }

    #[test]
    fn a_gated_row_goes_to_the_script_under_its_own_kind() {
        let text = "Origin\tDestination\tmenuOption menuTarget objectID\n1 2 0\t1 2 1\tClimb-up Stairs 1\n3 4 0\t3 6 0\tOpen Door 2\n";
        let (rows, _) = parse_file("transports", text).unwrap();
        assert_eq!(gated_kind(&rows[0]), GATED_KIND);
        assert_eq!(gated_kind(&rows[1]), LINK_GATED_KIND);
        let shortcut = Transport {
            kind: SHORTCUT_KIND.into(),
            ..rows[1].clone()
        };
        assert_eq!(gated_kind(&shortcut), SHORTCUT_GATED_KIND);
    }

    #[test]
    fn the_ditch_touches_the_wilderness() {
        let text = "Origin\tDestination\tmenuOption menuTarget objectID\n3100 3520 0\t3100 3523 0\tCross Wilderness Ditch 23271\n3100 3300 0\t3100 3303 0\tCross Stile 1\n";
        let (rows, _) = parse_file("transports", text).unwrap();
        assert!(touches_wilderness(&rows[0]) && !touches_wilderness(&rows[1]));
        assert!(in_wilderness(3100, 10000) && !in_wilderness(3100, 3522));
    }

    #[test]
    fn a_line_round_trips() {
        let (transports, _) = parse_file("teleportation_spells", SPELLS).unwrap();
        let data = Data {
            source: "bundled".into(),
            transports,
            items: vec![ItemName {
                name: "COINS".into(),
                ids: vec![995],
            }],
            climbs: Vec::new(),
        };
        assert_eq!(parse(line(&data).strip_prefix(PREFIX).unwrap()), Some(data));
        assert!(parse("not json").is_none());
    }
}
