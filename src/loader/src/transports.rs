//! Downloads the transport data (teleport spells, charter ships, ...) once per run, keeps a local copy, and builds the line that gives it to scripts.
//!
//! Each file comes from the first source that works: the download, the copy kept from an earlier run, the copy bundled with the loader.
//! A download replaces the kept copy only after it parses, so a bad answer (an error page, a half file) never damages it.
//! Scripts never touch the network for this: the loader makes the request, to one fixed address, and the HTTP whitelist doesn't apply.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use injector::transports::{self, Data, ItemName, Transport};

/// The commit of Skretzo/shortest-path the files are taken from. Bump it together with `data/`.
const COMMIT: &str = "4fd7d4a220c06781145dd7ba21e9a1e46ed0fc27";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FILE: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A file of transports (`Destination` column and so on).
    Transports,
    /// `item-variations.tsv`: the named item groups the `Items` column uses.
    ItemNames,
}

struct File {
    /// The cache file's name, and the transports' `kind`.
    name: &'static str,
    /// The path under `src/main/resources/` upstream.
    path: &'static str,
    kind: Kind,
    /// The copy bundled with the loader.
    bundled: &'static str,
    /// Keeps only the rows of a transports file that this version can use (the file also lists stairs, doors and much else, which would only make the scripts' data big).
    keep: Option<fn(&Transport) -> bool>,
    /// For a file of separate starts and ends (the mushtrees): the radius, in tiles, under which an end is too close to its start to be a trip. Upstream's `transport-types.tsv` holds it; it is
    /// written here because it is one number per type, and the type is named in this list anyway.
    permute: Option<i32>,
    /// Also takes the planner's climbs out of the file: the rows this accepts (every row that changes plane, or every shortcut a walk can use).
    climbs: Option<fn(&Transport) -> bool>,
}

/// A toll barrier (`Pay-toll(2-Ecto) Energy Barrier 16105`): a short hop through a gate that the walker's own planner takes for a wall.
fn is_toll(t: &Transport) -> bool {
    t.origin.is_some() && t.destination.is_some() && t.menu.starts_with("Pay-toll")
}

/// Files whose bundled copy is always used: it differs from upstream's, which would replace it.
const BUNDLED_ONLY: &[&str] = &["teleportation_levers"];

/// The files to load. An explicit list, so a stray file upstream is never read as a transport.
const FILES: &[File] = &[
    File {
        name: "teleportation_spells",
        path: "transports/teleportation_spells.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/teleportation_spells.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "teleportation_items",
        path: "transports/teleportation_items.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/teleportation_items.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "teleportation_spells_home",
        path: "transports/teleportation_spells_home.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/teleportation_spells_home.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "charter_ships",
        path: "transports/charter_ships.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/charter_ships.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "magic_mushtrees",
        path: "transports/magic_mushtrees.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/magic_mushtrees.tsv"),
        keep: None,
        permute: Some(5),
        climbs: None,
    },
    File {
        name: "quetzals",
        path: "transports/quetzals.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/quetzals.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "spirit_trees",
        path: "transports/spirit_trees.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/spirit_trees.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "gnome_gliders",
        path: "transports/gnome_gliders.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/gnome_gliders.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "fairy_rings",
        path: "transports/fairy_rings.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/fairy_rings.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "boats",
        path: "transports/boats.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/boats.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "ships",
        path: "transports/ships.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/ships.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "minecarts",
        path: "transports/minecarts.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/minecarts.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "teleportation_levers",
        path: "transports/teleportation_levers.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/teleportation_levers.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "item-variations",
        path: "item-variations.tsv",
        kind: Kind::ItemNames,
        bundled: include_str!("../data/item-variations.tsv"),
        keep: None,
        permute: None,
        climbs: None,
    },
    File {
        name: "transports",
        path: "transports/transports.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/transports.tsv"),
        keep: Some(is_toll),
        permute: None,
        climbs: Some(transports::is_climb),
    },
    File {
        name: "agility_shortcuts",
        path: "transports/agility_shortcuts.tsv",
        kind: Kind::Transports,
        bundled: include_str!("../data/transports/agility_shortcuts.tsv"),
        // the script gets the rows through the copies that ask for something, and the planner has the rest
        keep: Some(|_| false),
        permute: None,
        climbs: Some(transports::is_shortcut),
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Online,
    Cache,
    Bundled,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Source::Online => "online",
            Source::Cache => "cache",
            Source::Bundled => "bundled",
        }
    }
}

fn url(file: &File) -> String {
    format!(
        "https://raw.githubusercontent.com/Skretzo/shortest-path/{COMMIT}/src/main/resources/{}",
        file.path
    )
}

fn cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.data_dir().join("transports"))
}

fn download(file: &File) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("volcren-loader/", env!("VOLCREN_VERSION")))
        .build()
        .into();
    agent
        .get(url(file))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .with_config()
        .limit(MAX_FILE)
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// What a file holds.
enum Parsed {
    /// The rows for scripts, and the planner's climbs.
    Transports(Vec<Transport>, Vec<Transport>),
    ItemNames(Vec<ItemName>),
}

/// The contents of a file, or why it can't be used. A file with nothing usable in it is no good (an empty answer would hide the bundled copy).
fn validate(file: &File, text: &str) -> Result<Parsed, String> {
    match file.kind {
        Kind::Transports => {
            let (mut rows, _) = transports::parse_file(file.name, text)?;
            if let Some(radius) = file.permute {
                rows = transports::expand_permutations(rows, radius);
            }
            // the climbs: all of them for the planner, and those that ask for something also for the script, which checks them
            let climbs: Vec<Transport> = match file.climbs {
                Some(accept) => rows.iter().filter(|t| accept(t)).cloned().collect(),
                None => Vec::new(),
            };
            if let Some(keep) = file.keep {
                rows.retain(keep);
            }
            if file.climbs.is_some() && climbs.is_empty() {
                return Err(format!("{}: no climbs", file.name));
            }
            if rows.is_empty() && climbs.is_empty() {
                return Err(format!("{}: no rows", file.name));
            }
            rows.extend(climbs.iter().filter(|t| transports::gated(t)).map(|t| {
                let kind = if t.kind == transports::SHORTCUT_KIND {
                    transports::SHORTCUT_GATED_KIND
                } else {
                    transports::GATED_KIND
                };
                Transport {
                    kind: kind.into(),
                    ..t.clone()
                }
            }));
            Ok(Parsed::Transports(rows, climbs))
        }
        Kind::ItemNames => {
            let names = transports::parse_item_variations(text)?;
            if names.is_empty() {
                return Err(format!("{}: no items", file.name));
            }
            Ok(Parsed::ItemNames(names))
        }
    }
}

/// Writes next to the file and renames, so a crash never leaves half a file.
fn store(dir: &Path, name: &str, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{name}.tsv.tmp"));
    std::fs::write(&temp, text)?;
    std::fs::rename(temp, dir.join(format!("{name}.tsv")))
}

/// Every file from its best source. `fetch` gets a file; `dir` is the kept copy.
fn resolve(dir: Option<&Path>, fetch: impl Fn(&File) -> Result<String, String>) -> Data {
    let mut data = Data::default();
    let mut worst = Source::Online;
    for file in FILES {
        if BUNDLED_ONLY.contains(&file.name) {
            let parsed = validate(file, file.bundled).expect("the bundled file parses");
            if let Parsed::Transports(rows, climbs) = parsed {
                data.transports.extend(rows);
                data.climbs.extend(climbs);
            }
            continue;
        }
        let online =
            fetch(file).and_then(|text| validate(file, &text).map(|parsed| (text, parsed)));
        let (parsed, source) = match online {
            Ok((text, parsed)) => {
                if let Some(dir) = dir
                    && let Err(e) = store(dir, file.name, &text)
                {
                    eprintln!("transports: couldn't keep {}: {e}", file.name);
                }
                (parsed, Source::Online)
            }
            Err(why) => {
                let kept = dir
                    .and_then(|dir| {
                        std::fs::read_to_string(dir.join(format!("{}.tsv", file.name))).ok()
                    })
                    .and_then(|text| validate(file, &text).ok());
                match kept {
                    Some(parsed) => {
                        eprintln!("transports: {}: {why}; using the kept copy", file.name);
                        (parsed, Source::Cache)
                    }
                    None => {
                        eprintln!("transports: {}: {why}; using the bundled copy", file.name);
                        (
                            validate(file, file.bundled).expect("the bundled file parses"),
                            Source::Bundled,
                        )
                    }
                }
            }
        };
        worst = match (worst, source) {
            (Source::Bundled, _) | (_, Source::Bundled) => Source::Bundled,
            (Source::Cache, _) | (_, Source::Cache) => Source::Cache,
            _ => Source::Online,
        };
        match parsed {
            Parsed::Transports(rows, climbs) => {
                data.transports.extend(rows);
                data.climbs.extend(climbs);
            }
            Parsed::ItemNames(names) => data.items = names,
        }
    }
    data.source = worst.name().into();
    data
}

static LOADED: Mutex<Option<Data>> = Mutex::new(None);

fn loaded() -> std::sync::MutexGuard<'static, Option<Data>> {
    LOADED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts the one download of this run, in the background. Call it once, when the loader starts.
pub fn start() {
    std::thread::spawn(|| {
        let dir = cache_dir();
        let data = resolve(dir.as_deref(), download);
        eprintln!(
            "transports: {} rows, {} item names ({})",
            data.transports.len(),
            data.items.len(),
            data.source
        );
        *loaded() = Some(data);
    });
}

/// The line to give a starting script: the download if it has finished, else the kept or bundled copy. Never waits on the network.
pub fn line() -> String {
    let data = loaded()
        .clone()
        .unwrap_or_else(|| resolve(cache_dir().as_deref(), |_| Err("not downloaded yet".into())));
    transports::line(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("volcren-transports-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A small file that is valid for the file's kind.
    fn good(file: &File) -> String {
        match file.kind {
            Kind::Transports if file.keep.is_some() => {
                "Origin\tDestination\tmenuOption menuTarget objectID\n1 2 0\t3 4 0\tPay-toll(2-Ecto) Energy Barrier 16105\n1 2 0\t3 4 0\tOpen Door 9398\n1 2 0\t1 2 1\tClimb-up Ladder 1\n".to_owned()
            }
            Kind::Transports if file.permute.is_some() => {
                "Origin\tDestination\tmenuOption menuTarget objectID\tDisplay info\n1 2 0\t\tUse Tree 1\t\n\t100 200 0\t\tone\n".to_owned()
            }
            Kind::Transports => format!("Destination\tDisplay info\n1 2 0\t{} spell\n", file.name),
            Kind::ItemNames => "#name\tids\nCOINS\t995\n".to_owned(),
        }
    }

    fn spells() -> &'static File {
        &FILES[0]
    }

    #[test]
    fn the_bundled_files_parse() {
        for file in FILES {
            assert!(validate(file, file.bundled).is_ok(), "{}", file.name);
        }
    }

    #[test]
    fn a_good_download_is_used_and_kept() {
        let dir = temp("good");
        let data = resolve(Some(&dir), |file| Ok(good(file)));
        assert_eq!(data.source, "online");
        assert_eq!(
            data.transports.len(),
            // one row per file but the shortcuts, which give the script only the copies that ask for something, and the bundled-only levers
            FILES
                .iter()
                .filter(|f| {
                    f.kind == Kind::Transports
                        && f.name != "agility_shortcuts"
                        && !BUNDLED_ONLY.contains(&f.name)
                })
                .count()
                + data
                    .transports
                    .iter()
                    .filter(|t| t.kind == "teleportation_levers")
                    .count()
        );
        let levers = |t: &&Transport| t.kind == "teleportation_levers";
        assert_eq!(data.transports.iter().filter(levers).count(), 8);
        assert!(
            data.transports
                .iter()
                .any(|t| t.menu == "Edgeville Lever 1815")
        );
        assert_eq!(data.items.len(), 1);
        assert!(dir.join("teleportation_spells.tsv").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_download_keeps_the_good_copy() {
        let dir = temp("bad");
        resolve(Some(&dir), |file| Ok(good(file)));
        let data = resolve(Some(&dir), |_| Ok("<html>rate limited</html>".into()));
        assert_eq!(data.source, "cache");
        let kept = std::fs::read_to_string(dir.join(format!("{}.tsv", spells().name))).unwrap();
        assert!(kept.contains("teleportation_spells spell"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_network_uses_the_cache_then_the_bundled_copy() {
        let dir = temp("offline");
        resolve(Some(&dir), |file| Ok(good(file)));
        let offline = |_: &File| Err("no network".to_owned());
        assert_eq!(resolve(Some(&dir), offline).source, "cache");
        let _ = std::fs::remove_dir_all(&dir);
        let data = resolve(Some(&dir), offline);
        assert_eq!(data.source, "bundled");
        assert!(data.transports.iter().any(|t| t.name == "Varrock Teleport"));
        assert!(data.transports.iter().any(|t| t.kind == "charter_ships"));
        // boats, ships and carts: whole trips from a tile to a tile, and the minecart file also has the Lovakengj network (separate starts and ends)
        let trips = |kind: &str| {
            data.transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_some() && t.destination.is_some())
                .count()
        };
        assert!(trips("boats") > 100 && trips("ships") > 30 && trips("minecarts") > 10);
        // the networks with separate starts and ends: sent as they are, the chooser pairs them
        for kind in ["quetzals", "spirit_trees", "gnome_gliders", "fairy_rings"] {
            let starts = data
                .transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_some() && t.destination.is_none())
                .count();
            let ends = data
                .transports
                .iter()
                .filter(|t| t.kind == kind && t.origin.is_none() && t.destination.is_some())
                .count();
            assert!(
                starts > 5 && ends > 5,
                "{kind}: {starts} starts, {ends} ends"
            );
        }
        let rides: Vec<_> = data
            .transports
            .iter()
            .filter(|t| t.kind == "magic_mushtrees")
            .collect();
        assert_eq!(
            rides.len(),
            8 * 3,
            "two start tiles per tree, to each of the other three trees"
        );
        assert!(rides.iter().all(|t| t.origin.is_some()
            && t.destination.is_some()
            && t.menu.starts_with("Use Magic Mushtree")));
        let barriers: Vec<_> = data
            .transports
            .iter()
            .filter(|t| t.kind == "transports")
            .collect();
        assert!(barriers.len() >= 12, "the barriers are kept");
        assert!(
            barriers.iter().all(|t| t.menu.starts_with("Pay-toll")),
            "and nothing else of that file"
        );
        // the planner's climbs: stairs, ladders, ropes and cave mouths, with exact planes (the rope near 1559,3046 goes from plane 0 to plane 2)
        assert!(data.climbs.len() > 3000, "{} climbs", data.climbs.len());
        let plain = |t: &&Transport| t.kind != transports::SHORTCUT_KIND;
        assert!(data.climbs.iter().filter(plain).all(transports::is_climb));
        // the Tree Door of the Grand Tree: a solid object that carries you two tiles across, no plane change
        let door = data
            .climbs
            .iter()
            .find(|t| {
                t.menu == "Open Tree Door 1968"
                    && t.origin.is_some_and(|o| (o.x, o.y) == (2466, 3491))
            })
            .expect("the Tree Door");
        assert!(transports::is_link(door) && !transports::gated(door));
        // a toll barrier is the script's, and a wall door (a one tile hop) the planner's own
        assert!(data.climbs.iter().all(|t| !t.menu.starts_with("Pay-toll")));
        let rope = data
            .climbs
            .iter()
            .find(|t| t.menu == "Climb-up Rope 51647")
            .expect("the rope up");
        assert_eq!(
            (
                rope.origin.map(|t| t.plane),
                rope.destination.map(|t| t.plane)
            ),
            (Some(0), Some(2))
        );
        assert!(
            data.climbs
                .iter()
                .any(|t| t.origin.is_some_and(|o| o.y < 6400)
                    && t.destination.is_some_and(|d| d.y >= 6400)),
            "into the underground"
        );
        let gated = data
            .transports
            .iter()
            .filter(|t| t.kind == transports::GATED_KIND)
            .count();
        assert_eq!(
            gated,
            data.climbs
                .iter()
                .filter(plain)
                .filter(|t| transports::gated(t))
                .count()
        );
        // the agility shortcuts: hops that stay in one world and are short, and all of them ask for a level
        let shortcuts: Vec<_> = data
            .climbs
            .iter()
            .filter(|t| t.kind == transports::SHORTCUT_KIND)
            .collect();
        assert!(shortcuts.len() > 500, "{} shortcuts", shortcuts.len());
        assert!(shortcuts.iter().all(|t| transports::is_shortcut(t)));
        let pipe = shortcuts
            .iter()
            .find(|t| {
                t.menu == "Squeeze-through Obstacle pipe 16511"
                    && t.origin
                        .is_some_and(|o| (o.x, o.y, o.plane) == (3149, 9906, 0))
            })
            .expect("the Varrock sewer pipe, west to east");
        assert!(
            pipe.destination
                .is_some_and(|d| (d.x, d.y, d.plane) == (3155, 9906, 0))
        );
        assert!(
            pipe.skills
                .iter()
                .any(|s| s.name == "Agility" && s.level == 51)
        );
        let sent = data
            .transports
            .iter()
            .filter(|t| t.kind == transports::SHORTCUT_GATED_KIND)
            .count();
        assert_eq!(
            sent,
            shortcuts.iter().filter(|t| transports::gated(t)).count()
        );
        assert!(
            data.transports
                .iter()
                .all(|t| t.kind != transports::SHORTCUT_KIND),
            "scripts get only the gated copies"
        );
        assert!(gated > 0 && gated < 1000, "{gated} gated climbs");
        assert!(
            data.items
                .iter()
                .any(|i| i.name == "COINS" && i.ids == [995])
        );
        assert_eq!(resolve(None, offline).source, "bundled");
    }

    #[test]
    fn an_empty_answer_does_not_replace_anything() {
        let dir = temp("empty");
        let data = resolve(Some(&dir), |_| Ok(String::new()));
        assert_eq!(data.source, "bundled");
        assert!(!dir.join("teleportation_spells.tsv").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
