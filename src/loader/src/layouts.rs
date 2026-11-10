use std::collections::HashMap;

use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use loader::backend::{self, Settings, Target};

use crate::icons::{Icons, Key, SLOT_CELL, SLOT_SIZE};
use crate::theme;

pub const EQUIPMENT_SLOTS: [(usize, &str); 12] = [
    (0, "Head"),
    (1, "Cape"),
    (2, "Amulet"),
    (3, "Weapon"),
    (4, "Body"),
    (5, "Shield"),
    (7, "Legs"),
    (9, "Hands"),
    (10, "Feet"),
    (12, "Ring"),
    (13, "Ammo"),
    (SECONDARY_AMMO_SLOT, "Secondary ammo"),
];
const EQUIPMENT_CELLS: [(usize, usize, usize); 12] = [
    (0, 1, 0),
    (SECONDARY_AMMO_SLOT, 2, 0),
    (1, 0, 1),
    (2, 1, 1),
    (13, 2, 1),
    (3, 0, 2),
    (4, 1, 2),
    (5, 2, 2),
    (7, 1, 3),
    (9, 0, 4),
    (10, 1, 4),
    (12, 2, 4),
];
pub const EQUIPMENT_SIZE: usize = 15;
pub const SECONDARY_AMMO_SLOT: usize = 14;
const SECONDARY_AMMO_HINT: &str =
    "Ignored unless the account has a bolt pouch in its inventory or Dizana's quiver.";
pub const INVENTORY_CONTAINER: i32 = 93;
pub const EQUIPMENT_CONTAINER: i32 = 94;
const CAPE_SLOT: i32 = 1;
const BOLT_POUCH: i32 = 9433;
const POUCH_EXTRA_BOLT_VARBIT: i32 = 2476;
const POUCH_EXTRA_AMOUNT_VARBIT: i32 = 2472;
const POUCH_BOLTS_ENUM: i32 = 336;
const QUIVER_PARAM: u32 = 1910;
const QUIVER_AMMO_VARP: usize = 4142;
const QUIVER_AMOUNT_VARP: usize = 4141;
pub const INVENTORY_SIZE: usize = 28;
pub const INVENTORY_COLUMNS: usize = 4;
pub const POUCH_SIZE: usize = 4;
pub const POUCH_MAX_AMOUNT: i32 = 16000;
pub const POUCH_RUNE_VARBITS: [i32; POUCH_SIZE] = [29, 1622, 1623, 14285];
pub const POUCH_AMOUNT_VARBITS: [i32; POUCH_SIZE] = [1624, 1625, 1626, 14286];
pub const POUCH_RUNES_ENUM: i32 = 982;

pub mod boost {
    pub const ATTACK: u8 = 1;
    pub const STRENGTH: u8 = 2;
    pub const DEFENCE: u8 = 4;
    pub const RANGED: u8 = 8;
    pub const MAGIC: u8 = 16;
    pub const OVERHEAD: u8 = 32;
    pub const ALL_STATS: u8 = ATTACK | STRENGTH | DEFENCE | RANGED | MAGIC;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrayerInfo {
    pub key: &'static str,
    pub name: &'static str,
    pub level: u32,
    pub varbit: i32,
    pub quick_bit: u32,
    pub sprite: i32,
    pub boosts: u8,
    pub replaces: Option<&'static str>,
    pub unlock: Option<i32>,
}

const fn prayer(
    key: &'static str,
    name: &'static str,
    level: u32,
    varbit: i32,
    quick_bit: u32,
    sprite: i32,
    boosts: u8,
) -> PrayerInfo {
    PrayerInfo {
        key,
        name,
        level,
        varbit,
        quick_bit,
        sprite,
        boosts,
        replaces: None,
        unlock: None,
    }
}

const fn upgrade(
    base: PrayerInfo,
    key: &'static str,
    name: &'static str,
    level: u32,
    varbit: i32,
    unlock: i32,
    sprite: i32,
) -> PrayerInfo {
    PrayerInfo {
        key,
        name,
        level,
        varbit,
        quick_bit: base.quick_bit,
        sprite,
        boosts: base.boosts,
        replaces: Some(base.key),
        unlock: Some(unlock),
    }
}

const EAGLE_EYE: PrayerInfo = prayer("eagle_eye", "Eagle Eye", 44, 4126, 22, 504, boost::RANGED);
const MYSTIC_MIGHT: PrayerInfo = prayer(
    "mystic_might",
    "Mystic Might",
    45,
    4127,
    23,
    505,
    boost::MAGIC,
);
const PIETY: PrayerInfo = prayer("piety", "Piety", 70, 4129, 26, 946, boost::ALL_STATS);

pub const PRAYERS: [PrayerInfo; 32] = [
    prayer("thick_skin", "Thick Skin", 1, 4104, 0, 115, boost::DEFENCE),
    prayer(
        "burst_of_strength",
        "Burst of Strength",
        4,
        4105,
        1,
        116,
        boost::STRENGTH,
    ),
    prayer(
        "clarity_of_thought",
        "Clarity of Thought",
        7,
        4106,
        2,
        117,
        boost::ATTACK,
    ),
    prayer("sharp_eye", "Sharp Eye", 8, 4122, 18, 133, boost::RANGED),
    prayer("mystic_will", "Mystic Will", 9, 4123, 19, 134, boost::MAGIC),
    prayer("rock_skin", "Rock Skin", 10, 4107, 3, 118, boost::DEFENCE),
    prayer(
        "superhuman_strength",
        "Superhuman Strength",
        13,
        4108,
        4,
        119,
        boost::STRENGTH,
    ),
    prayer(
        "improved_reflexes",
        "Improved Reflexes",
        16,
        4109,
        5,
        120,
        boost::ATTACK,
    ),
    prayer("rapid_restore", "Rapid Restore", 19, 4110, 6, 121, 0),
    prayer("rapid_heal", "Rapid Heal", 22, 4111, 7, 122, 0),
    prayer("protect_item", "Protect Item", 25, 4112, 8, 123, 0),
    prayer("hawk_eye", "Hawk Eye", 26, 4124, 20, 502, boost::RANGED),
    prayer(
        "mystic_lore",
        "Mystic Lore",
        27,
        4125,
        21,
        503,
        boost::MAGIC,
    ),
    prayer("steel_skin", "Steel Skin", 28, 4113, 9, 124, boost::DEFENCE),
    prayer(
        "ultimate_strength",
        "Ultimate Strength",
        31,
        4114,
        10,
        125,
        boost::STRENGTH,
    ),
    prayer(
        "incredible_reflexes",
        "Incredible Reflexes",
        34,
        4115,
        11,
        126,
        boost::ATTACK,
    ),
    prayer(
        "protect_from_magic",
        "Protect from Magic",
        37,
        4116,
        12,
        127,
        boost::OVERHEAD,
    ),
    prayer(
        "protect_from_missiles",
        "Protect from Missiles",
        40,
        4117,
        13,
        128,
        boost::OVERHEAD,
    ),
    prayer(
        "protect_from_melee",
        "Protect from Melee",
        43,
        4118,
        14,
        129,
        boost::OVERHEAD,
    ),
    EAGLE_EYE,
    MYSTIC_MIGHT,
    prayer(
        "retribution",
        "Retribution",
        46,
        4119,
        15,
        131,
        boost::OVERHEAD,
    ),
    prayer(
        "redemption",
        "Redemption",
        49,
        4120,
        16,
        130,
        boost::OVERHEAD,
    ),
    prayer("smite", "Smite", 52, 4121, 17, 132, boost::OVERHEAD),
    prayer("preserve", "Preserve", 55, 5466, 28, 947, 0),
    prayer("chivalry", "Chivalry", 60, 4128, 25, 945, boost::ALL_STATS),
    upgrade(EAGLE_EYE, "deadeye", "Deadeye", 62, 16090, 16097, 1422),
    upgrade(
        MYSTIC_MIGHT,
        "mystic_vigour",
        "Mystic Vigour",
        63,
        16091,
        16098,
        1423,
    ),
    PIETY,
    prayer("rigour", "Rigour", 74, 5464, 24, 1420, boost::ALL_STATS),
    prayer("augury", "Augury", 77, 5465, 27, 1421, boost::ALL_STATS),
    upgrade(PIETY, "zeal", "Zeal", 84, 16577, 16578, 8595),
];
pub const PRAYER_COLUMNS: usize = 5;
pub const QUICK_SELECTED_VARBIT: i32 = 4102;

pub fn prayer_info(key: &str) -> Option<&'static PrayerInfo> {
    PRAYERS.iter().find(|p| p.key == key)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Book {
    Standard,
    Ancient,
    Arceuus,
}

impl Book {
    pub fn label(self) -> &'static str {
        match self {
            Book::Standard => "Standard",
            Book::Ancient => "Ancient",
            Book::Arceuus => "Arceuus",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Book::Standard => "standard",
            Book::Ancient => "ancient",
            Book::Arceuus => "arceuus",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellInfo {
    pub key: &'static str,
    pub name: &'static str,
    pub level: u32,
    pub sprite: i32,
    pub book: Book,
}

const fn spell(
    key: &'static str,
    name: &'static str,
    level: u32,
    sprite: i32,
    book: Book,
) -> SpellInfo {
    SpellInfo {
        key,
        name,
        level,
        sprite,
        book,
    }
}

pub const AUTOCAST_SPELLS: [SpellInfo; 48] = [
    spell("wind_strike", "Wind Strike", 1, 15, Book::Standard),
    spell("water_strike", "Water Strike", 5, 17, Book::Standard),
    spell("earth_strike", "Earth Strike", 9, 19, Book::Standard),
    spell("fire_strike", "Fire Strike", 13, 21, Book::Standard),
    spell("wind_bolt", "Wind Bolt", 17, 23, Book::Standard),
    spell("water_bolt", "Water Bolt", 23, 26, Book::Standard),
    spell("earth_bolt", "Earth Bolt", 29, 29, Book::Standard),
    spell("fire_bolt", "Fire Bolt", 35, 32, Book::Standard),
    spell("wind_blast", "Wind Blast", 41, 35, Book::Standard),
    spell("water_blast", "Water Blast", 47, 38, Book::Standard),
    spell("earth_blast", "Earth Blast", 53, 40, Book::Standard),
    spell("fire_blast", "Fire Blast", 59, 44, Book::Standard),
    spell("wind_wave", "Wind Wave", 62, 46, Book::Standard),
    spell("water_wave", "Water Wave", 65, 48, Book::Standard),
    spell("earth_wave", "Earth Wave", 70, 51, Book::Standard),
    spell("fire_wave", "Fire Wave", 75, 52, Book::Standard),
    spell("crumble_undead", "Crumble Undead", 39, 34, Book::Standard),
    spell("magic_dart", "Magic Dart", 50, 324, Book::Standard),
    spell("claws_of_guthix", "Claws of Guthix", 60, 60, Book::Standard),
    spell(
        "flames_of_zamorak",
        "Flames of Zamorak",
        60,
        59,
        Book::Standard,
    ),
    spell("smoke_rush", "Smoke Rush", 50, 329, Book::Ancient),
    spell("shadow_rush", "Shadow Rush", 52, 337, Book::Ancient),
    spell("blood_rush", "Blood Rush", 56, 333, Book::Ancient),
    spell("ice_rush", "Ice Rush", 58, 325, Book::Ancient),
    spell("smoke_burst", "Smoke Burst", 62, 330, Book::Ancient),
    spell("shadow_burst", "Shadow Burst", 64, 338, Book::Ancient),
    spell("blood_burst", "Blood Burst", 68, 334, Book::Ancient),
    spell("ice_burst", "Ice Burst", 70, 326, Book::Ancient),
    spell("smoke_blitz", "Smoke Blitz", 74, 331, Book::Ancient),
    spell("shadow_blitz", "Shadow Blitz", 76, 339, Book::Ancient),
    spell("blood_blitz", "Blood Blitz", 80, 335, Book::Ancient),
    spell("ice_blitz", "Ice Blitz", 82, 327, Book::Ancient),
    spell("smoke_barrage", "Smoke Barrage", 86, 332, Book::Ancient),
    spell("shadow_barrage", "Shadow Barrage", 88, 340, Book::Ancient),
    spell("blood_barrage", "Blood Barrage", 92, 336, Book::Ancient),
    spell("ice_barrage", "Ice Barrage", 94, 328, Book::Ancient),
    spell("iban_blast", "Iban Blast", 50, 53, Book::Standard),
    spell("wind_surge", "Wind Surge", 81, 362, Book::Standard),
    spell("water_surge", "Water Surge", 85, 363, Book::Standard),
    spell("earth_surge", "Earth Surge", 90, 364, Book::Standard),
    spell("fire_surge", "Fire Surge", 95, 365, Book::Standard),
    spell(
        "saradomin_strike",
        "Saradomin Strike",
        60,
        61,
        Book::Standard,
    ),
    spell(
        "inferior_demonbane",
        "Inferior Demonbane",
        44,
        1302,
        Book::Arceuus,
    ),
    spell(
        "superior_demonbane",
        "Superior Demonbane",
        62,
        1303,
        Book::Arceuus,
    ),
    spell("dark_demonbane", "Dark Demonbane", 82, 1304, Book::Arceuus),
    spell("ghostly_grasp", "Ghostly Grasp", 35, 1267, Book::Arceuus),
    spell("skeletal_grasp", "Skeletal Grasp", 56, 1268, Book::Arceuus),
    spell("undead_grasp", "Undead Grasp", 79, 1269, Book::Arceuus),
];
pub const AUTOCAST_WEAPON_CATEGORIES: [i32; 3] = [18, 21, 22];
pub const WEAPON_CATEGORY_VARBIT: i32 = 357;

fn spell_column(info: &SpellInfo) -> &'static str {
    let first = info.name.split(' ').next().unwrap_or(info.name);
    match info.book {
        Book::Arceuus => info.name.rsplit(' ').next().unwrap_or(info.name),
        Book::Standard if !["Wind", "Water", "Earth", "Fire"].contains(&first) => "Other",
        _ => first,
    }
}

pub fn spell_info(key: &str) -> Option<&'static SpellInfo> {
    AUTOCAST_SPELLS.iter().find(|s| s.key == key)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Doses {
    pub all: Vec<u32>,
    pub accepted: Vec<u32>,
    pub highest_first: bool,
}

impl Doses {
    fn sort(&mut self) {
        self.accepted.sort_unstable();
        if self.highest_first {
            self.accepted.reverse();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: i32,
    pub name: String,
    pub stackable: bool,
    pub min: i32,
    pub max: i32,
    pub doses: Option<Doses>,
    pub locked: bool,
    pub ids: Vec<i32>,
}

impl Item {
    pub fn read(id: i32, quantity: i32, name: String, stackable: bool) -> Self {
        let amount = if stackable { quantity } else { 1 };
        Item {
            id,
            name,
            stackable,
            min: amount,
            max: amount,
            doses: None,
            locked: false,
            ids: Vec::new(),
        }
    }

    fn icon_key(&self) -> Key {
        Key::item(self.id, self.max.max(1), self.stackable && self.max > 0)
    }

    fn to_value(&self) -> Value {
        let mut ids = vec![self.id];
        ids.extend(self.ids.iter().copied().filter(|&id| id != self.id));
        json!({
            "id": self.id,
            "ids": ids,
            "name": self.name,
            "stackable": self.stackable,
            "min": self.min,
            "max": self.max,
            "doses": self.doses.as_ref().map(|d| &d.accepted),
            "locked": self.locked,
        })
    }

    pub fn is_rune_pouch(&self) -> bool {
        self.name.to_ascii_lowercase().contains("rune pouch")
    }

    fn dose_base(&self) -> Option<&str> {
        self.doses.as_ref()?;
        injector::config::dose_of(&self.name).map(|(base, _)| base)
    }

    fn counts_as(&self, id: i32) -> bool {
        self.id == id || self.ids.contains(&id)
    }
}

pub fn add_doses<'a>(
    items: impl IntoIterator<Item = &'a mut Item>,
    families: &HashMap<i32, Vec<u32>>,
) {
    for item in items {
        let Some(all) = families.get(&item.id).filter(|all| all.len() > 1) else {
            continue;
        };
        if item.stackable {
            continue;
        }
        if let Some((_, dose)) = injector::config::dose_of(&item.name) {
            item.doses = Some(Doses {
                all: all.clone(),
                accepted: vec![dose],
                highest_first: true,
            });
        }
    }
}

pub type Known = HashMap<i32, (String, bool)>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    pub required: Vec<injector::form::Required>,
    pub locked: Vec<String>,
}

fn required_item(rule: &injector::form::Required, known: &Known) -> Item {
    let id = rule.ids[0];
    let (name, stackable) = known
        .get(&id)
        .cloned()
        .unwrap_or_else(|| (String::new(), rule.max > 0));
    let (min, max) = if stackable {
        (rule.min, rule.max)
    } else {
        (1, 1)
    };
    let doses = (!rule.doses.is_empty()).then(|| {
        let mut all = rule.doses.clone();
        all.sort_unstable();
        Doses {
            highest_first: rule.doses.first() >= rule.doses.last(),
            all,
            accepted: rule.doses.clone(),
        }
    });
    Item {
        id,
        name,
        stackable,
        min,
        max,
        doses,
        locked: true,
        ids: rule.ids[1..].to_vec(),
    }
}

impl Rules {
    pub fn unknown(&self, known: &Known) -> Vec<i32> {
        self.required
            .iter()
            .map(|r| r.ids[0])
            .filter(|id| !known.contains_key(id))
            .collect()
    }

    pub fn fit_inventory(&self, layout: &mut Inventory, known: &Known) -> usize {
        layout
            .slots
            .resize(INVENTORY_SIZE.max(layout.slots.len()), None);
        for item in layout.slots.iter_mut().flatten() {
            if item.locked && !self.required.iter().any(|r| r.ids.contains(&item.id)) {
                item.locked = false;
                item.ids.clear();
            }
        }
        let mut placed: Vec<usize> = Vec::new();
        let mut homeless = Vec::new();
        for rule in &self.required {
            let at = |locked: bool| {
                layout.slots.iter().position(|slot| {
                    slot.as_ref()
                        .is_some_and(|item| item.locked == locked && rule.ids.contains(&item.id))
                })
            };
            match at(true)
                .or_else(|| at(false))
                .filter(|i| !placed.contains(i))
            {
                Some(i) => {
                    layout.slots[i] = Some(required_item(rule, known));
                    placed.push(i);
                }
                None => homeless.push(rule),
            }
        }
        let mut missing = 0;
        for rule in homeless {
            let free = rule
                .slot
                .filter(|&s| layout.slots.get(s).is_some_and(Option::is_none))
                .or_else(|| layout.slots.iter().position(Option::is_none));
            match free {
                Some(i) => layout.slots[i] = Some(required_item(rule, known)),
                None => missing += 1,
            }
        }
        for i in 0..layout.slots.len() {
            let Some(item) = layout.slots[i].as_ref().filter(|item| !item.locked) else {
                continue;
            };
            if let Some(doses) = layout.script_doses(item)
                && let Some(item) = layout.slots[i].as_mut()
            {
                item.doses = Some(doses);
            }
        }
        missing
    }

    pub fn fit_equipment(&self, layout: &mut Equipment, known: &Known) {
        for &(slot, _) in &EQUIPMENT_SLOTS {
            if let EquipmentSlot::Item(item) = layout.slot_mut(slot)
                && item.locked
                && !self
                    .required
                    .iter()
                    .any(|r| r.slot == Some(slot) && r.ids.contains(&item.id))
            {
                item.locked = false;
                item.ids.clear();
            }
        }
        for rule in &self.required {
            if let Some(slot) = rule.slot.filter(|&s| s < EQUIPMENT_SIZE) {
                *layout.slot_mut(slot) = EquipmentSlot::Item(required_item(rule, known));
            }
        }
    }

    pub fn blocks(&self, key: &str) -> bool {
        let Some(info) = prayer_info(key) else {
            return false;
        };
        self.locked.iter().any(|locked| {
            locked == key || prayer_info(locked).is_some_and(|l| l.boosts & info.boosts != 0)
        })
    }

    pub fn fit_prayer(&self, layout: &mut Prayer) {
        layout.prayers.retain(|key| !self.blocks(key));
    }

    fn blocked_prayers(&self) -> Vec<&'static str> {
        PRAYERS
            .iter()
            .filter(|p| self.blocks(p.key))
            .map(|p| p.key)
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentSlot {
    #[default]
    Empty,
    Any,
    Item(Item),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipment {
    pub name: String,
    pub slots: Vec<EquipmentSlot>,
    #[serde(default)]
    pub spell: Option<String>,
    #[serde(default)]
    pub autocast: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub name: String,
    pub slots: Vec<Option<Item>>,
    pub pouch: Option<Vec<Item>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prayer {
    pub name: String,
    pub prayers: Vec<String>,
}

impl Prayer {
    fn has(&self, key: &str) -> bool {
        self.prayers.iter().any(|k| k == key)
    }

    pub fn toggle(&mut self, key: &str) {
        if self.has(key) {
            self.prayers.retain(|k| k != key);
            return;
        }
        let Some(info) = prayer_info(key) else {
            return;
        };
        self.prayers
            .retain(|k| prayer_info(k).is_none_or(|other| other.boosts & info.boosts == 0));
        self.prayers.push(key.to_owned());
        self.sort();
    }

    fn sort(&mut self) {
        self.prayers.sort_by_key(|k| {
            PRAYERS
                .iter()
                .position(|p| p.key == k)
                .unwrap_or(usize::MAX)
        });
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLayouts {
    pub equipment: Vec<Equipment>,
    pub inventory: Vec<Inventory>,
    #[serde(default)]
    pub prayer: Vec<Prayer>,
}

pub type AllLayouts = HashMap<String, ScriptLayouts>;

pub fn load(
    storage: Option<&dyn eframe::Storage>,
    key: &str,
    backup_dir: Option<&std::path::Path>,
) -> (AllLayouts, Option<String>) {
    let Some(text) = storage.and_then(|s| s.get_string(key)) else {
        return (AllLayouts::default(), None);
    };
    if let Some(layouts) = storage.and_then(|s| eframe::get_value(s, key)) {
        return (layouts, None);
    }
    let kept = backup_dir
        .map(|dir| {
            let secs = crate::paths::unix_secs();
            dir.join(format!("layouts-unreadable-{secs}.ron"))
        })
        .ok_or_else(|| "no data folder".to_owned())
        .and_then(|path| {
            std::fs::create_dir_all(path.parent().unwrap_or(&path))
                .and_then(|()| std::fs::write(&path, &text))
                .map(|()| path)
                .map_err(|e| e.to_string())
        });
    let copy = match kept {
        Ok(path) => format!("a copy is in {}", path.display()),
        Err(e) => format!("they couldn't be copied: {e}"),
    };
    (
        AllLayouts::default(),
        Some(format!(
            "The saved layouts can't be read by this loader (saved by a newer one?), so they are left as they were and layout \
             changes won't be saved until it is updated; {copy}"
        )),
    )
}

pub trait Layout: Clone + Default + PartialEq {
    const KIND: Kind;
    fn name(&self) -> &str;
    fn set_name(&mut self, name: String);
    fn to_value(&self) -> Value;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Equipment,
    Inventory,
    Prayer,
}

impl Kind {
    pub fn field_kind(self) -> &'static str {
        match self {
            Kind::Equipment => "equipment",
            Kind::Inventory => "inventory",
            Kind::Prayer => "prayer",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Equipment => "Equipment",
            Kind::Inventory => "Inventory",
            Kind::Prayer => "Prayer",
        }
    }

    pub fn names(self, layouts: &ScriptLayouts) -> Vec<&str> {
        match self {
            Kind::Equipment => layouts.equipment.iter().map(|l| l.name.as_str()).collect(),
            Kind::Inventory => layouts.inventory.iter().map(|l| l.name.as_str()).collect(),
            Kind::Prayer => layouts.prayer.iter().map(|l| l.name.as_str()).collect(),
        }
    }

    pub fn value_of(
        self,
        layouts: &ScriptLayouts,
        name: &str,
        rules: &Rules,
        known: &Known,
    ) -> Value {
        match self {
            Kind::Equipment => layouts.equipment.iter().find(|l| l.name == name).map(|l| {
                let mut l = l.clone();
                rules.fit_equipment(&mut l, known);
                l.to_value()
            }),
            Kind::Inventory => layouts
                .inventory
                .iter()
                .find(|l| l.name == name)
                .and_then(|l| {
                    let mut l = l.clone();
                    (rules.fit_inventory(&mut l, known) == 0).then(|| l.to_value())
                }),
            Kind::Prayer => layouts.prayer.iter().find(|l| l.name == name).map(|l| {
                let mut l = l.clone();
                rules.fit_prayer(&mut l);
                let mut value = l.to_value();
                value["locked"] = json!(rules.blocked_prayers());
                value
            }),
        }
        .unwrap_or(Value::Null)
    }

    pub fn unusable(self, layouts: &ScriptLayouts, name: &str, rules: &Rules) -> Option<String> {
        if self != Kind::Inventory || rules.required.is_empty() {
            return None;
        }
        let mut layout = layouts.inventory.iter().find(|l| l.name == name)?.clone();
        let missing = rules.fit_inventory(&mut layout, &Known::new());
        (missing > 0).then(|| no_room(missing))
    }
}

impl Layout for Equipment {
    const KIND: Kind = Kind::Equipment;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn to_value(&self) -> Value {
        let slots: Vec<Value> = EQUIPMENT_SLOTS
            .iter()
            .map(|&(slot, _)| match self.slot(slot) {
                EquipmentSlot::Item(item) => {
                    let mut value = item.to_value();
                    value["slot"] = json!(slot);
                    value["mode"] = json!("item");
                    value
                }
                EquipmentSlot::Any => json!({ "slot": slot, "mode": "any" }),
                EquipmentSlot::Empty => json!({ "slot": slot, "mode": "empty" }),
            })
            .collect();
        let spell = self.spell.as_deref().and_then(spell_info).map(|s| {
            json!({ "key": s.key, "name": s.name, "book": s.book.key(), "level": s.level, "autocast": self.autocast })
        });
        json!({ "name": self.name, "slots": slots, "spell": spell })
    }
}

impl Layout for Inventory {
    const KIND: Kind = Kind::Inventory;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn to_value(&self) -> Value {
        let slots: Vec<Value> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| {
                let mut value = item.as_ref()?.to_value();
                value["slot"] = json!(slot);
                Some(value)
            })
            .collect();
        let pouch = self
            .pouch
            .as_ref()
            .map(|runes| Value::Array(runes.iter().map(Item::to_value).collect()));
        json!({ "name": self.name, "slots": slots, "pouch": pouch })
    }
}

impl Layout for Prayer {
    const KIND: Kind = Kind::Prayer;
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, name: String) {
        self.name = name;
    }
    fn to_value(&self) -> Value {
        json!({ "name": self.name, "prayers": self.prayers })
    }
}

impl Equipment {
    fn slot(&self, slot: usize) -> &EquipmentSlot {
        self.slots
            .get(slot)
            .unwrap_or(if slot == SECONDARY_AMMO_SLOT {
                &EquipmentSlot::Any
            } else {
                &EquipmentSlot::Empty
            })
    }

    fn slot_mut(&mut self, slot: usize) -> &mut EquipmentSlot {
        while self.slots.len() < EQUIPMENT_SIZE {
            self.slots.push(self.slot(self.slots.len()).clone());
        }
        &mut self.slots[slot]
    }
}

impl Inventory {
    fn slot_mut(&mut self, slot: usize) -> &mut Option<Item> {
        if self.slots.len() < INVENTORY_SIZE {
            self.slots.resize(INVENTORY_SIZE, None);
        }
        &mut self.slots[slot]
    }

    fn script_doses(&self, item: &Item) -> Option<Doses> {
        let (base, _) = injector::config::dose_of(&item.name)?;
        self.slots
            .iter()
            .flatten()
            .find(|other| {
                other.locked
                    && injector::config::dose_of(&other.name).is_some_and(|(b, _)| b == base)
            })
            .and_then(|other| other.doses.clone())
    }

    fn holds_pouch(&self) -> bool {
        self.slots.iter().flatten().any(Item::is_rune_pouch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Equipment {
        slots: Vec<EquipmentSlot>,
        autocast: bool,
    },
    Inventory {
        slots: Vec<Option<Item>>,
        pouch: Option<Vec<Item>>,
    },
    Prayers(Vec<String>),
}

#[derive(Debug, Clone, Default)]
pub struct Editor<L> {
    pub index: Option<usize>,
    opened: Option<String>,
    pub draft: L,
    pub selected: Option<usize>,
    pub selected_in_pouch: bool,
    pub reading: bool,
    pub error: Option<String>,
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Editors {
    pub equipment: Editor<Equipment>,
    pub inventory: Editor<Inventory>,
    pub prayer: Editor<Prayer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    ReadQuick,
}

const NEW_LAYOUT: &str = "New layout";

fn copy_name<L: Layout>(saved: &[L], name: &str) -> String {
    let free = |candidate: &str| saved.iter().all(|l| l.name() != candidate);
    let first = format!("{name} copy");
    if free(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{first} {n}"))
        .find(|candidate| free(candidate))
        .unwrap_or(first)
}

impl<L: Layout> Editor<L> {
    fn open(&mut self, saved: &[L], index: Option<usize>) {
        self.index = index.filter(|&i| i < saved.len());
        self.opened = self.index.map(|i| saved[i].name().to_owned());
        self.draft = self.index.map(|i| saved[i].clone()).unwrap_or_default();
        self.selected = None;
        self.selected_in_pouch = false;
        self.error = None;
    }

    pub fn finish_read(&mut self, result: Result<Read, String>, apply: impl FnOnce(&mut L, Read)) {
        self.reading = false;
        match result {
            Ok(read) => {
                self.error = None;
                apply(&mut self.draft, read);
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn dirty(&self, saved: &[L]) -> bool {
        self.index.and_then(|i| saved.get(i)) != Some(&self.draft)
    }

    fn follow(&mut self, saved: &[L]) {
        self.index = self
            .opened
            .as_deref()
            .and_then(|name| saved.iter().position(|l| l.name() == name));
        if self.index.is_none() {
            self.opened = None;
        }
    }

    fn header(&mut self, ui: &mut egui::Ui, saved: &mut Vec<L>) {
        self.follow(saved);
        let mut open: Option<Option<usize>> = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label("Layout:");
                let current = self
                    .index
                    .and_then(|i| saved.get(i))
                    .map_or(NEW_LAYOUT, |l| l.name());
                egui::ComboBox::from_id_salt((L::KIND.field_kind(), "layout_picker"))
                    .width(PICKER_WIDTH)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (i, layout) in saved.iter().enumerate() {
                            if ui
                                .selectable_label(self.index == Some(i), layout.name())
                                .clicked()
                            {
                                open = Some(Some(i));
                            }
                        }
                        if ui
                            .selectable_label(self.index.is_none(), NEW_LAYOUT)
                            .clicked()
                        {
                            open = Some(None);
                        }
                    });
            });
            ui.vertical(|ui| {
                ui.label("Name:");
                let mut name = self.draft.name().to_owned();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(NAME_WIDTH))
                    .changed()
                {
                    self.draft.set_name(name);
                }
            });
        });
        ui.add_space(4.0);
        let button = (COLUMN_WIDTH - 2.0 * ui.spacing().item_spacing.x) / 3.0;
        ui.horizontal(|ui| {
            let name = self.draft.name().trim().to_owned();
            let taken = saved
                .iter()
                .enumerate()
                .any(|(i, l)| Some(i) != self.index && l.name() == name);
            let can_save =
                !name.is_empty() && !taken && self.invalid.is_none() && self.dirty(saved);
            if ui
                .add_enabled(
                    can_save,
                    egui::Button::new("Save").min_size(egui::vec2(button, 0.0)),
                )
                .on_disabled_hover_text(if let Some(invalid) = &self.invalid {
                    invalid.as_str()
                } else if taken {
                    "Another layout has this name."
                } else if name.is_empty() {
                    "Give the layout a name."
                } else {
                    "Nothing changed."
                })
                .clicked()
            {
                self.draft.set_name(name.clone());
                match self.index {
                    Some(i) => saved[i] = self.draft.clone(),
                    None => {
                        saved.push(self.draft.clone());
                        self.index = Some(saved.len() - 1);
                    }
                }
                self.opened = Some(name);
            }
            if ui
                .add_enabled(
                    self.index.is_some(),
                    egui::Button::new("Duplicate").min_size(egui::vec2(button, 0.0)),
                )
                .clicked()
                && let Some(i) = self.index
            {
                let mut copy = saved[i].clone();
                copy.set_name(copy_name(saved, copy.name()));
                saved.push(copy);
                open = Some(Some(saved.len() - 1));
            }
            if ui
                .add_enabled(
                    self.index.is_some(),
                    egui::Button::new("Remove").min_size(egui::vec2(button, 0.0)),
                )
                .clicked()
                && let Some(i) = self.index
            {
                saved.remove(i);
                open = Some(
                    i.checked_sub(1)
                        .or_else(|| (!saved.is_empty()).then_some(0)),
                );
            }
        });
        if let Some(index) = open {
            self.open(saved, index);
        }
    }

    fn begin_read(&mut self) {
        self.reading = true;
        self.error = None;
    }

    fn read_ui(
        &mut self,
        ui: &mut egui::Ui,
        hint: Option<&str>,
        can_read: bool,
        quick_read: bool,
    ) -> Option<Action> {
        let mut action = None;
        ui.vertical(|ui| {
            ui.set_width(COLUMN_WIDTH);
            if let Some(hint) = hint {
                ui.vertical_centered(|ui| ui.weak(hint));
            }
            ui.vertical_centered_justified(|ui| {
                let read = ui
                    .add_enabled(
                        can_read && !self.reading,
                        egui::Button::new("Read from client"),
                    )
                    .on_hover_text("Fills the layout with what the selected client holds now.")
                    .on_disabled_hover_text(
                        "Select a client with the agent loaded in the Clients tab.",
                    );
                if self.reading {
                    let height = read.rect.height();
                    let center = read.rect.right_center() - egui::vec2(height / 2.0, 0.0);
                    egui::Spinner::new().paint_at(
                        ui,
                        egui::Rect::from_center_size(center, egui::Vec2::splat(height * 0.7)),
                    );
                }
                if read.clicked() {
                    self.begin_read();
                    action = Some(Action::Read);
                }
                if quick_read
                    && ui
                        .add_enabled(
                            can_read && !self.reading,
                            egui::Button::new("Read quick prayers"),
                        )
                        .on_hover_text("Fills the layout with the client's quick prayer selection.")
                        .clicked()
                {
                    self.begin_read();
                    action = Some(Action::ReadQuick);
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
        action
    }
}

const fn grid_width(columns: usize, cell: f32, gap: f32) -> f32 {
    columns as f32 * cell + (columns - 1) as f32 * gap
}

const PRAYER_CELL: f32 = 42.0;
const PRAYER_GAP: f32 = 4.0;
const PICKER_WIDTH: f32 = 100.0;
const NAME_WIDTH: f32 = 160.0;

const COLUMN_WIDTH: f32 = PICKER_WIDTH + 8.0 + NAME_WIDTH;

fn centered_column<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    centered_width(ui, COLUMN_WIDTH, add)
}

fn centered_width<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let pad = ((ui.available_width() - width) / 2.0 - ui.spacing().item_spacing.x).max(0.0);
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui)
        })
        .inner
    })
    .inner
}

fn slot_ui(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    item: Option<&Item>,
    empty_text: &str,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(SLOT_CELL, sense);
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let stroke = if selected {
            egui::Stroke::new(1.5, crate::theme::BLUE)
        } else {
            visuals.bg_stroke
        };
        ui.painter().rect(
            rect,
            3.0,
            visuals.weak_bg_fill,
            stroke,
            egui::StrokeKind::Inside,
        );
        let inner = egui::Rect::from_center_size(rect.center(), SLOT_SIZE);
        match item {
            Some(item) => {
                match icons.texture(ui.ctx(), item.icon_key()) {
                    Some(texture) => {
                        egui::Image::from_texture(texture).paint_at(ui, inner);
                    }
                    None => {
                        let short = if item.name.is_empty() {
                            item.id.to_string()
                        } else {
                            item.name.chars().take(4).collect()
                        };
                        ui.painter().text(
                            inner.center(),
                            egui::Align2::CENTER_CENTER,
                            short,
                            egui::TextStyle::Small.resolve(ui.style()),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }
                if item.locked {
                    paint_padlock(ui.painter(), rect);
                }
            }
            None => {
                ui.painter().text(
                    inner.center(),
                    egui::Align2::CENTER_CENTER,
                    empty_text,
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
            }
        }
    }
    response
}

fn range_text(item: &Item) -> String {
    match (item.min, item.max) {
        (0, 0) => "any amount".to_owned(),
        (min, max) if min == max => format!("x{min}"),
        (0, max) => format!("up to {max}"),
        (min, max) => format!("{min} to {max}"),
    }
}

fn doses_text(doses: &Doses) -> String {
    let list: Vec<String> = doses.accepted.iter().map(|d| format!("({d})")).collect();
    format!("doses {}", list.join(" "))
}

fn item_hover(item: &Item) -> String {
    let mut text = item_title(item);
    if item.stackable {
        text = format!("{text}, {}", range_text(item));
    }
    if let Some(doses) = &item.doses {
        text = format!("{text}, {}", doses_text(doses));
    }
    if item.locked {
        text.push_str("\nRequired by the script.");
    }
    text
}

fn item_label(item: &Item) -> String {
    if item.name.is_empty() {
        format!("Item {}", item.id)
    } else {
        item.name.clone()
    }
}

fn item_title(item: &Item) -> String {
    format!("{}  (id {})", item_label(item), item.id)
}

fn slot_name(slot: usize) -> &'static str {
    EQUIPMENT_SLOTS
        .iter()
        .find(|(s, _)| *s == slot)
        .map_or("", |(_, name)| name)
}

fn paint_sprite(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    sprite: i32,
    rect: egui::Rect,
    padding: f32,
    tint: egui::Color32,
    name: &str,
) {
    match icons.texture(ui.ctx(), Key::Sprite(sprite)) {
        Some(texture) => {
            let size = texture
                .size_vec2()
                .min(rect.size() - egui::vec2(padding, padding));
            egui::Image::from_texture(texture)
                .tint(tint)
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), size));
        }
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                name.split(' ')
                    .map(|w| w.chars().next().unwrap_or(' '))
                    .collect::<String>(),
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    }
}

fn paint_padlock(painter: &egui::Painter, rect: egui::Rect) {
    let size = (rect.width() * 0.32).clamp(9.0, 14.0);
    let body = egui::Rect::from_min_size(
        egui::pos2(rect.right() - size - 2.0, rect.top() + size * 0.55 + 2.0),
        egui::vec2(size, size * 0.75),
    );
    let backing = body
        .expand2(egui::vec2(2.0, size * 0.55 + 1.5))
        .translate(egui::vec2(0.0, -size * 0.27));
    painter.rect_filled(backing, 3.0, egui::Color32::from_black_alpha(170));
    let gold = egui::Color32::from_rgb(232, 190, 80);
    let radius = size * 0.3;
    let center = egui::pos2(body.center().x, body.top());
    let mut shackle: Vec<egui::Pos2> = vec![egui::pos2(center.x - radius, body.top() + 1.0)];
    shackle.extend((0..=12).map(|i| {
        let angle = std::f32::consts::PI * i as f32 / 12.0;
        egui::pos2(
            center.x - radius * angle.cos(),
            center.y - radius * angle.sin(),
        )
    }));
    shackle.push(egui::pos2(center.x + radius, body.top() + 1.0));
    painter.add(egui::Shape::line(
        shackle,
        egui::Stroke::new(size * 0.16, gold),
    ));
    painter.rect_filled(body, 1.5, gold);
    painter.circle_filled(
        body.center(),
        size * 0.1,
        egui::Color32::from_black_alpha(200),
    );
}

fn item_ui(ui: &mut egui::Ui, item: &mut Item, doses_fixed: bool, cap: i32) -> Option<Doses> {
    ui.label(item_title(item));
    if item.locked {
        ui.colored_label(
            theme::BLUE,
            "The script requires this item: it can be moved, not removed or changed.",
        );
        let mut parts = Vec::new();
        if item.stackable {
            parts.push(range_text(item));
        }
        if let Some(doses) = &item.doses {
            parts.push(doses_text(doses));
        }
        if !parts.is_empty() {
            ui.weak(parts.join(", "));
        }
        return None;
    }
    if item.stackable {
        ui.horizontal(|ui| {
            let spacing = ui.spacing().item_spacing.x;
            let width = 2.0 * 30.0 + 2.0 * 70.0 + 3.0 * spacing;
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            ui.label("Min");
            let min = ui.add(
                egui::DragValue::new(&mut item.min)
                    .range(0..=cap)
                    .speed(1.0)
                    .update_while_editing(false),
            );
            ui.label("Max");
            let max = ui.add(
                egui::DragValue::new(&mut item.max)
                    .range(0..=cap)
                    .speed(1.0)
                    .update_while_editing(false),
            );
            if min.changed() && item.min > item.max {
                item.max = item.min;
            }
            if max.changed() && (item.max == 0 || item.max < item.min) {
                item.min = item.min.min(item.max);
            }
        });
        ui.weak("Equal: exactly that many. Both 0: any amount.");
    }
    if doses_fixed {
        if let Some(doses) = &item.doses {
            ui.weak(format!(
                "{}: the same as the item the script requires.",
                doses_text(doses)
            ));
        }
        return None;
    }
    let doses = item.doses.as_mut()?;
    let before = doses.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label("Doses:");
        for &dose in &doses.all {
            let mut on = doses.accepted.contains(&dose);
            if ui.checkbox(&mut on, format!("({dose})")).changed() {
                if on {
                    doses.accepted.push(dose);
                } else if doses.accepted.len() > 1 {
                    doses.accepted.retain(|&d| d != dose);
                }
            }
        }
    });
    egui::ComboBox::from_id_salt("dose_order")
        .selected_text(if doses.highest_first {
            "Highest first"
        } else {
            "Lowest first"
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut doses.highest_first, true, "Highest first");
            ui.selectable_value(&mut doses.highest_first, false, "Lowest first");
        });
    doses.sort();
    ui.weak("A carried dose that isn't ticked is swapped at the bank.");
    (*doses != before).then(|| doses.clone())
}

fn share_doses<'a>(items: impl IntoIterator<Item = &'a mut Item>, base: &str, doses: &Doses) {
    for item in items {
        if !item.locked && item.dose_base() == Some(base) {
            item.doses = Some(doses.clone());
        }
    }
}

pub fn equipment_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Equipment>,
    saved: &mut Vec<Equipment>,
    icons: &mut Icons,
    can_read: bool,
    rules: &Rules,
    known: &Known,
) -> Option<Action> {
    for layout in saved.iter_mut() {
        rules.fit_equipment(layout, known);
    }
    rules.fit_equipment(&mut editor.draft, known);
    let cell = SLOT_CELL;
    let gap = 6.0;
    let width = grid_width(3, cell.x, gap);
    let margin = (COLUMN_WIDTH - width) / 2.0;
    let mut action = None;
    let mut part = |ui: &mut egui::Ui, spells: bool| {
        if spells {
            spell_ui(ui, &mut editor.draft, icons);
            return;
        }
        ui.set_width(COLUMN_WIDTH);
        editor.header(ui, saved);
        ui.add_space(6.0);
        let hint = if rules.required.is_empty() {
            "Right-click to empty."
        } else {
            "Right-click to empty.\nPadlocked items are the script's."
        };
        action = editor.read_ui(ui, Some(hint), can_read, false);
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            let (grid, _) = ui.allocate_exact_size(
                egui::vec2(width, 5.0 * cell.y + 4.0 * gap),
                egui::Sense::hover(),
            );
            for &(slot, column, row) in &EQUIPMENT_CELLS {
                let min = grid.min
                    + egui::vec2(column as f32 * (cell.x + gap), row as f32 * (cell.y + gap));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_size(min, cell))
                        .layout(egui::Layout::left_to_right(egui::Align::Min)),
                );
                let name = slot_name(slot);
                let (item, text, hover) = match editor.draft.slot(slot) {
                    EquipmentSlot::Item(item) => {
                        (Some(item), "", format!("{name}: {}", item_hover(item)))
                    }
                    EquipmentSlot::Empty => (None, "", format!("{name}: must be empty.")),
                    EquipmentSlot::Any => (
                        None,
                        "any",
                        format!("{name}: anything or nothing may be worn."),
                    ),
                };
                let hover = if slot == SECONDARY_AMMO_SLOT {
                    format!("{hover}\n{SECONDARY_AMMO_HINT}")
                } else {
                    hover
                };
                let response = slot_ui(
                    &mut child,
                    icons,
                    item,
                    text,
                    editor.selected == Some(slot),
                    egui::Sense::click(),
                )
                .on_hover_text(hover);
                if response.clicked() {
                    editor.selected = Some(slot);
                }
                if response.secondary_clicked() {
                    editor.selected = Some(slot);
                    let slot = editor.draft.slot_mut(slot);
                    if !matches!(slot, EquipmentSlot::Item(item) if item.locked) {
                        *slot = EquipmentSlot::Empty;
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            if let Some(slot) = editor.selected {
                ui.strong(slot_name(slot));
                if slot == SECONDARY_AMMO_SLOT {
                    ui.weak(SECONDARY_AMMO_HINT);
                }
                match editor.draft.slot_mut(slot) {
                    EquipmentSlot::Item(item) => {
                        item_ui(ui, item, false, i32::MAX);
                    }
                    state => {
                        const EMPTY: &str = "Must be empty";
                        const ANY: &str = "Any";
                        egui::ComboBox::from_id_salt("equipment_slot_mode")
                            .selected_text(if *state == EquipmentSlot::Any {
                                ANY
                            } else {
                                EMPTY
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(state, EquipmentSlot::Empty, EMPTY);
                                ui.selectable_value(state, EquipmentSlot::Any, ANY);
                            });
                    }
                }
            }
        });
    };
    let gap = 24.0;
    if ui.available_width() >= COLUMN_WIDTH + gap + COLUMN_WIDTH {
        centered_width(ui, COLUMN_WIDTH + gap + COLUMN_WIDTH, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| part(ui, false));
                ui.add_space(gap);
                ui.vertical(|ui| part(ui, true));
            });
        });
    } else {
        centered_width(ui, COLUMN_WIDTH, |ui| part(ui, false));
        ui.add_space(gap);
        centered_width(ui, COLUMN_WIDTH, |ui| part(ui, true));
    }
    action
}

fn spell_ui(ui: &mut egui::Ui, draft: &mut Equipment, icons: &mut Icons) {
    let width = COLUMN_WIDTH;
    ui.set_width(width);
    ui.strong(if draft.autocast {
        "Autocast"
    } else {
        "Spell to cast by hand"
    });
    ui.weak(if draft.autocast {
        "The weapon can autocast: the script sets this spell on the combat tab."
    } else {
        "The weapon can't autocast (or none was read): the script gets the spell to cast itself."
    });
    let cell = egui::vec2(30.0, 30.0);
    let mut clicked: Option<&'static str> = None;
    let book_id = egui::Id::new("spell_book");
    let mut book = ui
        .data(|d| d.get_temp::<Book>(book_id))
        .or_else(|| draft.spell.as_deref().and_then(spell_info).map(|s| s.book))
        .unwrap_or(Book::Standard);
    ui.add_space(4.0);
    egui::ComboBox::from_id_salt("spell_book_mode")
        .width(width)
        .selected_text(book.label())
        .show_ui(ui, |ui| {
            for option in [Book::Standard, Book::Ancient, Book::Arceuus] {
                ui.selectable_value(&mut book, option, option.label());
            }
        });
    ui.data_mut(|d| d.insert_temp(book_id, book));
    ui.add_space(4.0);
    let mut columns: Vec<(&str, Vec<&SpellInfo>)> = Vec::new();
    for info in AUTOCAST_SPELLS.iter().filter(|s| s.book == book) {
        let column = spell_column(info);
        match columns.iter_mut().find(|(name, _)| *name == column) {
            Some((_, spells)) => spells.push(info),
            None => columns.push((column, vec![info])),
        }
    }
    for (_, spells) in &mut columns {
        spells.sort_by_key(|s| s.level);
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        ui.add_space(((width - grid_width(columns.len(), cell.x, 3.0)) / 2.0 - 3.0).max(0.0));
        for (_, spells) in &columns {
            ui.vertical(|ui| {
                for info in spells {
                    let on = draft.spell.as_deref() == Some(info.key);
                    let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
                    if ui.is_rect_visible(rect) {
                        let visuals = ui.style().interact(&response);
                        let (fill, stroke) = if on {
                            (
                                theme::BLUE.linear_multiply(0.35),
                                egui::Stroke::new(1.5, theme::BLUE),
                            )
                        } else {
                            (visuals.weak_bg_fill, visuals.bg_stroke)
                        };
                        ui.painter()
                            .rect(rect, 3.0, fill, stroke, egui::StrokeKind::Inside);
                        paint_sprite(
                            ui,
                            icons,
                            info.sprite,
                            rect,
                            4.0,
                            egui::Color32::WHITE,
                            info.name,
                        );
                    }
                    if response
                        .on_hover_text(format!("{} (level {})", info.name, info.level))
                        .clicked()
                    {
                        clicked = Some(info.key);
                    }
                }
            });
        }
    });
    if let Some(key) = clicked {
        draft.spell = if draft.spell.as_deref() == Some(key) {
            None
        } else {
            Some(key.to_owned())
        };
    }
    match draft.spell.as_deref().and_then(spell_info) {
        Some(info) => ui.label(format!(
            "{} ({}, level {})",
            info.name,
            info.book.label(),
            info.level
        )),
        None => ui.weak("No spell."),
    };
}

fn no_room(missing: usize) -> String {
    if missing == 1 {
        "No room for an item the script requires: free a slot.".to_owned()
    } else {
        format!("No room for {missing} items the script requires: free {missing} slots.")
    }
}

pub fn inventory_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Inventory>,
    saved: &mut Vec<Inventory>,
    icons: &mut Icons,
    can_read: bool,
    rules: &Rules,
    known: &Known,
) -> Option<Action> {
    for layout in saved.iter_mut() {
        rules.fit_inventory(layout, known);
    }
    let missing = rules.fit_inventory(&mut editor.draft, known);
    editor.invalid = (missing > 0).then(|| no_room(missing));
    let gap = 6.0;
    let margin = (COLUMN_WIDTH - grid_width(INVENTORY_COLUMNS, SLOT_CELL.x, gap)) / 2.0;
    centered_column(ui, |ui| {
        editor.header(ui, saved);
        ui.add_space(6.0);
        let hint = if rules.required.is_empty() {
            "Drag to move.\nRight-click to empty."
        } else {
            "Drag to move. Right-click to empty.\nPadlocked items are the script's."
        };
        let action = editor.read_ui(ui, Some(hint), can_read, false);
        ui.add_space(6.0);
        let mut moved: Option<(usize, usize)> = None;
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            egui::Grid::new("inventory_layout_grid")
                .spacing(egui::vec2(gap, gap))
                .show(ui, |ui| {
                    for slot in 0..INVENTORY_SIZE {
                        let item = editor.draft.slots.get(slot).and_then(Option::as_ref);
                        let selected = editor.selected == Some(slot) && !editor.selected_in_pouch;
                        let response = match item {
                            Some(item) => {
                                let response = slot_ui(
                                    ui,
                                    icons,
                                    Some(item),
                                    "",
                                    selected,
                                    egui::Sense::click_and_drag(),
                                )
                                .on_hover_text(item_hover(item));
                                if response.drag_started() {
                                    egui::DragAndDrop::set_payload(ui.ctx(), slot);
                                }
                                if response.dragged() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                    if let Some(pos) = ui.ctx().pointer_interact_pos() {
                                        egui::Area::new(egui::Id::new("inventory_layout_ghost"))
                                            .order(egui::Order::Tooltip)
                                            .interactable(false)
                                            .fixed_pos(pos - SLOT_CELL / 2.0)
                                            .show(ui.ctx(), |ui| {
                                                slot_ui(
                                                    ui,
                                                    icons,
                                                    Some(item),
                                                    "",
                                                    true,
                                                    egui::Sense::hover(),
                                                );
                                            });
                                    }
                                }
                                response
                            }
                            None => slot_ui(ui, icons, None, "", selected, egui::Sense::click())
                                .on_hover_text(format!("Slot {slot}: empty")),
                        };
                        if let Some(from) = response.dnd_release_payload::<usize>()
                            && *from != slot
                        {
                            moved = Some((*from, slot));
                        }
                        if response.clicked() {
                            editor.selected = Some(slot);
                            editor.selected_in_pouch = false;
                        }
                        if response.secondary_clicked() {
                            let slot_item = editor.draft.slot_mut(slot);
                            if !slot_item.as_ref().is_some_and(|item| item.locked) {
                                *slot_item = None;
                            }
                            editor.selected = Some(slot);
                            editor.selected_in_pouch = false;
                        }
                        if (slot + 1) % INVENTORY_COLUMNS == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            if editor.draft.pouch.is_some() || editor.draft.holds_pouch() {
                ui.strong("Rune pouch");
                let runes = editor.draft.pouch.get_or_insert_with(Vec::new);
                let spacing = ui.spacing().item_spacing.x;
                let row = grid_width(POUCH_SIZE, SLOT_CELL.x, spacing);
                ui.horizontal(|ui| {
                    ui.add_space(((COLUMN_WIDTH - row) / 2.0 - spacing).max(0.0));
                    for i in 0..POUCH_SIZE {
                        let selected = editor.selected == Some(i) && editor.selected_in_pouch;
                        let response =
                            slot_ui(ui, icons, runes.get(i), "", selected, egui::Sense::click())
                                .on_hover_text(
                                    runes.get(i).map_or_else(|| "Empty".to_owned(), item_hover),
                                );
                        if response.clicked() {
                            editor.selected = Some(i);
                            editor.selected_in_pouch = true;
                        }
                        if response.secondary_clicked() && i < runes.len() {
                            runes.remove(i);
                            editor.selected = None;
                        }
                    }
                });
                if let Some(rune) = editor
                    .selected
                    .filter(|_| editor.selected_in_pouch)
                    .and_then(|i| runes.get_mut(i))
                {
                    item_ui(ui, rune, false, POUCH_MAX_AMOUNT);
                }
                ui.weak("Read from client fills it from the pouch's contents.");
                ui.add_space(10.0);
            }
            if let Some(slot) = editor.selected.filter(|_| !editor.selected_in_pouch) {
                ui.strong(format!("Slot {slot}"));
                let mut shared = None;
                let fixed = editor
                    .draft
                    .slots
                    .get(slot)
                    .and_then(Option::as_ref)
                    .is_some_and(|item| !item.locked && editor.draft.script_doses(item).is_some());
                match editor.draft.slot_mut(slot) {
                    Some(item) => {
                        if let Some(doses) = item_ui(ui, item, fixed, i32::MAX) {
                            shared = item.dose_base().map(|base| (base.to_owned(), doses));
                        }
                    }
                    None => {
                        ui.label("Empty.");
                    }
                }
                if let Some((base, doses)) = shared {
                    share_doses(editor.draft.slots.iter_mut().flatten(), &base, &doses);
                }
            }
            if let Some(invalid) = &editor.invalid {
                ui.colored_label(ui.visuals().error_fg_color, invalid);
            }
        });
        if let Some((from, to)) = moved {
            editor
                .draft
                .slots
                .resize(editor.draft.slots.len().max(INVENTORY_SIZE), None);
            editor.draft.slots.swap(from, to);
            editor.selected = Some(to);
            editor.selected_in_pouch = false;
        }
        action
    })
}

pub fn prayer_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Prayer>,
    saved: &mut Vec<Prayer>,
    icons: &mut Icons,
    can_read: bool,
    prayer_level: Option<u32>,
    rules: &Rules,
) -> Option<Action> {
    for layout in saved.iter_mut() {
        rules.fit_prayer(layout);
    }
    rules.fit_prayer(&mut editor.draft);
    let cell = egui::Vec2::splat(PRAYER_CELL);
    centered_column(ui, |ui| {
        editor.header(ui, saved);
        ui.add_space(6.0);
        let action = editor.read_ui(ui, None, can_read, true);
        ui.add_space(6.0);
        let mut toggled: Option<&'static str> = None;
        let mut picked_blocked: Option<usize> = None;
        let margin = (COLUMN_WIDTH - grid_width(PRAYER_COLUMNS, PRAYER_CELL, PRAYER_GAP)) / 2.0;
        ui.horizontal_top(|ui| {
            ui.add_space(margin);
            egui::Grid::new("prayer_layout_grid")
                .spacing(egui::Vec2::splat(PRAYER_GAP))
                .show(ui, |ui| {
                    for (i, info) in PRAYERS.iter().enumerate() {
                        let on = editor.draft.has(info.key);
                        let blocked = rules.blocks(info.key);
                        let too_high = prayer_level.is_some_and(|level| level < info.level);
                        let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
                        if ui.is_rect_visible(rect) {
                            let visuals = ui.style().interact(&response);
                            let fill = if on {
                                theme::BLUE.linear_multiply(0.35)
                            } else {
                                visuals.weak_bg_fill
                            };
                            let stroke = if on || editor.selected == Some(i) {
                                egui::Stroke::new(1.5, theme::BLUE)
                            } else {
                                visuals.bg_stroke
                            };
                            ui.painter()
                                .rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
                            let tint = if too_high || blocked {
                                egui::Color32::from_white_alpha(90)
                            } else {
                                egui::Color32::WHITE
                            };
                            paint_sprite(ui, icons, info.sprite, rect, 6.0, tint, info.name);
                            if blocked {
                                paint_padlock(ui.painter(), rect);
                            }
                        }
                        let hover = if blocked {
                            format!("{}: {CONTROLLED}", info.name)
                        } else if too_high {
                            format!(
                                "{} (level {}, above the client's {})",
                                info.name,
                                info.level,
                                prayer_level.unwrap_or(0)
                            )
                        } else {
                            format!("{} (level {})", info.name, info.level)
                        };
                        if response.on_hover_text(hover).clicked() {
                            if blocked {
                                picked_blocked = Some(i);
                            } else {
                                toggled = Some(info.key);
                            }
                        }
                        if (i + 1) % PRAYER_COLUMNS == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
        if let Some(key) = toggled {
            editor.draft.toggle(key);
            editor.selected = None;
        }
        if picked_blocked.is_some() {
            editor.selected = picked_blocked;
        }
        ui.add_space(6.0);
        if let Some(info) = editor
            .selected
            .and_then(|i| PRAYERS.get(i))
            .filter(|p| rules.blocks(p.key))
        {
            ui.vertical_centered(|ui| {
                ui.strong(info.name);
                ui.colored_label(theme::BLUE, CONTROLLED);
            });
            ui.add_space(4.0);
        }
        let names: Vec<&str> = editor
            .draft
            .prayers
            .iter()
            .filter_map(|k| prayer_info(k).map(|p| p.name))
            .collect();
        if names.is_empty() {
            ui.weak("No prayers: click them in the book, or read the ones on in the client.");
        } else {
            ui.label(names.join(", "));
        }
        action
    })
}

const CONTROLLED: &str = "The script controls this prayer: it can't be customised.";

pub fn apply_prayers(draft: &mut Prayer, read: Read) {
    if let Read::Prayers(keys) = read {
        draft.prayers = keys;
        draft.sort();
    }
}

pub fn apply_equipment(draft: &mut Equipment, read: Read) {
    if let Read::Equipment { slots, autocast } = read {
        draft.slots = slots;
        draft.slots.resize(EQUIPMENT_SIZE, EquipmentSlot::Any);
        draft.autocast = autocast;
    }
}

fn enum_item(def: &injector::config::EnumDef, key: i32) -> Option<i32> {
    def.entries
        .iter()
        .find(|e| e.key == key)
        .and_then(|e| match &e.value {
            injector::config::Value::Int(id) => i32::try_from(*id).ok(),
            injector::config::Value::Str(_) => None,
        })
}

pub fn read_secondary_ammo(
    target: &Target,
    settings: &Settings,
    worn: &[injector::Item],
) -> Result<Option<Option<(i32, i32)>>, String> {
    let carried =
        backend::item_container(target, settings, INVENTORY_CONTAINER)?.unwrap_or_default();
    let mut ids: Vec<i32> = carried.iter().map(|item| item.id).collect();
    ids.extend(worn.iter().filter(|i| i.slot == CAPE_SLOT).map(|i| i.id));
    ids.retain(|&id| id >= 0);
    ids.sort_unstable();
    ids.dedup();
    let params = if ids.is_empty() {
        Vec::new()
    } else {
        backend::item_params(target, settings, &ids)?
    };
    let quiver = params.iter().flatten().any(|params| {
        matches!(params.get(&QUIVER_PARAM), Some(injector::config::Value::Int(value)) if *value > 0)
    });
    if quiver {
        let id = backend::varp(target, settings, QUIVER_AMMO_VARP)?.unwrap_or(-1);
        let amount = backend::varp(target, settings, QUIVER_AMOUNT_VARP)?.unwrap_or(0);
        return Ok(Some((id >= 0 && amount > 0).then_some((id, amount))));
    }
    if !carried.iter().any(|item| item.id == BOLT_POUCH) {
        return Ok(None);
    }
    let bolts = backend::enum_def(target, settings, POUCH_BOLTS_ENUM)?
        .ok_or("the cache has no bolt pouch enum")?;
    let index = backend::varbit(target, settings, POUCH_EXTRA_BOLT_VARBIT)?.unwrap_or(0);
    let amount = backend::varbit(target, settings, POUCH_EXTRA_AMOUNT_VARBIT)?.unwrap_or(0);
    Ok(Some(
        enum_item(&bolts, index)
            .filter(|&id| id > 0 && amount > 0)
            .map(|id| (id, amount)),
    ))
}

pub fn apply_inventory(draft: &mut Inventory, read: Read) {
    if let Read::Inventory { slots, pouch } = read {
        let locked: Vec<(usize, Item)> = draft
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| Some((i, slot.as_ref().filter(|item| item.locked)?.clone())))
            .collect();
        draft.slots = slots;
        draft.slots.resize(INVENTORY_SIZE, None);
        draft.pouch = pouch;
        for (at, item) in locked {
            let held = draft.slots.iter().flatten().any(|i| item.counts_as(i.id));
            if !held && let Some(slot @ None) = draft.slots.get_mut(at) {
                *slot = Some(item);
            }
        }
    }
}

pub type Described = (HashMap<i32, String>, HashMap<i32, bool>);

pub fn items_of(
    container: &[injector::Item],
    names: &HashMap<i32, String>,
    stackable: &HashMap<i32, bool>,
) -> Vec<Option<Item>> {
    container
        .iter()
        .map(|slot| {
            (slot.id >= 0).then(|| {
                Item::read(
                    slot.id,
                    slot.quantity,
                    names.get(&slot.id).cloned().unwrap_or_default(),
                    stackable.get(&slot.id).copied().unwrap_or(false),
                )
            })
        })
        .collect()
}

pub fn picker_ui(
    ui: &mut egui::Ui,
    kind: Kind,
    layouts: &ScriptLayouts,
    chosen: &mut Option<String>,
    id: &str,
    rules: &Rules,
) {
    let names = kind.names(layouts);
    if chosen
        .as_deref()
        .is_some_and(|name| !names.contains(&name) || kind.unusable(layouts, name, rules).is_some())
    {
        *chosen = None;
    }
    egui::ComboBox::from_id_salt(("layout_field", id))
        .selected_text(chosen.as_deref().unwrap_or("None"))
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, "None");
            for name in names {
                match kind.unusable(layouts, name, rules) {
                    Some(why) => {
                        ui.add_enabled(false, egui::Button::selectable(false, name))
                            .on_disabled_hover_text(why);
                    }
                    None => {
                        ui.selectable_value(chosen, Some(name.to_owned()), name);
                    }
                }
            }
        });
    ui.weak(format!("Edit them in the {} tab.", kind.label()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Storage(HashMap<String, String>);

    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn unreadable_layouts_are_kept_and_copied() {
        let dir = std::env::temp_dir().join(format!("volcren-layouts-{}", std::process::id()));
        let mut storage = Storage::default();
        let (none, message) = load(Some(&storage), "layouts", Some(&dir));
        assert!(none.is_empty() && message.is_none());

        let mut saved = AllLayouts::default();
        let zulrah = saved.entry("Zulrah".into()).or_default();
        zulrah.prayer.push(Prayer {
            name: "Boost".into(),
            prayers: vec!["rigour".into()],
        });
        let mut potion = item(2434, 1, false);
        potion.doses = Some(Doses {
            all: vec![1, 2, 3, 4],
            accepted: vec![4],
            highest_first: true,
        });
        zulrah.inventory.push(Inventory {
            name: "Trip".into(),
            slots: vec![Some(item(995, 1000, true)), None, Some(potion)],
            pouch: Some(vec![item(565, 100, true)]),
        });
        zulrah.equipment.push(Equipment {
            name: "Ranged".into(),
            slots: vec![
                EquipmentSlot::Item(item(23971, 1, false)),
                EquipmentSlot::Any,
            ],
            spell: None,
            autocast: false,
        });
        eframe::set_value(&mut storage, "layouts", &saved);
        let (read, message) = load(Some(&storage), "layouts", Some(&dir));
        assert_eq!(read, saved);
        assert!(message.is_none());

        let text = storage.0["layouts"].replace("prayers:[", "prayers:5,old:[");
        assert_ne!(text, storage.0["layouts"]);
        storage.0.insert("layouts".into(), text.clone());
        let (read, message) = load(Some(&storage), "layouts", Some(&dir));
        assert!(read.is_empty());
        assert!(message.unwrap().contains("left as they were"));
        let copies: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(std::fs::read_to_string(copies[0].path()).unwrap(), text);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn item(id: i32, quantity: i32, stackable: bool) -> Item {
        Item::read(id, quantity, format!("item {id}"), stackable)
    }

    fn rule(ids: &[i32], slot: Option<usize>) -> injector::form::Required {
        injector::form::Required {
            ids: ids.to_vec(),
            slot,
            min: 0,
            max: 0,
            doses: vec![],
        }
    }

    #[test]
    fn a_read_stack_wants_exactly_its_amount() {
        let old = item(995, 1000, true);
        assert_eq!((old.min, old.max), (1000, 1000));
        assert!(!old.locked && old.doses.is_none());
        let value = old.to_value();
        assert_eq!(value["min"], 1000);
        assert_eq!(value["ids"], json!([995]));
        assert_eq!(value["doses"], Value::Null);
        assert_eq!(
            range_text(&Item {
                min: 0,
                max: 0,
                ..old.clone()
            }),
            "any amount"
        );
        assert_eq!(
            range_text(&Item {
                min: 0,
                max: 50,
                ..old.clone()
            }),
            "up to 50"
        );
        assert_eq!(
            range_text(&Item {
                min: 5,
                max: 50,
                ..old
            }),
            "5 to 50"
        );
    }

    #[test]
    fn required_items_go_in_every_inventory_layout_and_stay_where_moved() {
        let rules = Rules {
            required: vec![rule(&[2434, 139], Some(5)), rule(&[563], None)],
            ..Default::default()
        };
        let known = Known::from([(2434, ("Prayer potion(4)".to_owned(), false))]);
        let mut layout = Inventory::default();
        *layout.slot_mut(5) = Some(item(1, 1, false));
        *layout.slot_mut(9) = Some(item(139, 1, false));
        assert_eq!(rules.fit_inventory(&mut layout, &known), 0);
        let potion = layout.slots[9].as_ref().unwrap();
        assert!(potion.locked);
        assert_eq!((potion.id, potion.ids.as_slice()), (2434, &[139][..]));
        assert_eq!(potion.name, "Prayer potion(4)");
        assert_eq!(layout.slots[0].as_ref().unwrap().id, 563);
        assert_eq!(layout.slots[0].as_ref().unwrap().name, "");
        layout.slots.swap(9, 20);
        assert_eq!(rules.fit_inventory(&mut layout, &known), 0);
        assert!(layout.slots[20].as_ref().unwrap().locked);
        assert!(layout.slots[9].is_none());
        let value = layout.to_value();
        assert_eq!(value["slots"][2]["ids"], json!([2434, 139]));
        assert_eq!(value["slots"][2]["locked"], true);
        assert_eq!(Rules::default().fit_inventory(&mut layout, &known), 0);
        assert!(!layout.slots[20].as_ref().unwrap().locked);
        let mut full = Inventory {
            slots: (0..28).map(|i| Some(item(1000 + i, 1, false))).collect(),
            ..Default::default()
        };
        assert_eq!(rules.fit_inventory(&mut full, &known), 2);
        let saved = ScriptLayouts {
            inventory: vec![Inventory {
                name: "Full".into(),
                ..full
            }],
            ..Default::default()
        };
        assert!(Kind::Inventory.unusable(&saved, "Full", &rules).is_some());
        assert_eq!(
            Kind::Inventory.value_of(&saved, "Full", &rules, &known),
            Value::Null
        );
        assert!(
            Kind::Inventory
                .unusable(&saved, "Full", &Rules::default())
                .is_none()
        );
    }

    #[test]
    fn the_users_items_of_a_required_family_take_its_doses() {
        let rules = Rules {
            required: vec![injector::form::Required {
                doses: vec![4, 3],
                ..rule(&[2434], None)
            }],
            ..Default::default()
        };
        let known = Known::from([(2434, ("Prayer potion(4)".to_owned(), false))]);
        let mut layout = Inventory::default();
        let mut own = Item::read(139, 1, "Prayer potion(3)".into(), false);
        add_doses([&mut own], &HashMap::from([(139, vec![1, 2, 3, 4])]));
        *layout.slot_mut(4) = Some(own);
        assert_eq!(rules.fit_inventory(&mut layout, &known), 0);
        assert_eq!(
            layout.slots[4]
                .as_ref()
                .unwrap()
                .doses
                .as_ref()
                .unwrap()
                .accepted,
            [4, 3]
        );
        let value = layout.to_value();
        assert_eq!(value["slots"][0]["doses"], value["slots"][1]["doses"]);
    }

    #[test]
    fn a_read_keeps_the_required_items_where_the_user_put_them() {
        let mut draft = Inventory::default();
        *draft.slot_mut(12) = Some(Item {
            locked: true,
            ..item(563, 100, true)
        });
        apply_inventory(
            &mut draft,
            Read::Inventory {
                slots: vec![Some(item(1, 1, false))],
                pouch: None,
            },
        );
        assert!(draft.slots[12].as_ref().unwrap().locked);
        assert_eq!(draft.slots[0].as_ref().unwrap().id, 1);
    }

    #[test]
    fn required_equipment_replaces_the_slot() {
        let rules = Rules {
            required: vec![rule(&[1712], Some(2))],
            ..Default::default()
        };
        let mut layout = Equipment::default();
        *layout.slot_mut(2) = EquipmentSlot::Item(item(1725, 1, false));
        rules.fit_equipment(&mut layout, &Known::new());
        assert!(matches!(layout.slot(2), EquipmentSlot::Item(i) if i.id == 1712 && i.locked));
        Rules::default().fit_equipment(&mut layout, &Known::new());
        assert!(matches!(layout.slot(2), EquipmentSlot::Item(i) if i.id == 1712 && !i.locked));
    }

    #[test]
    fn dose_items_take_their_own_dose_and_share_a_setting() {
        let mut items = [
            Item::read(2434, 1, "Prayer potion(4)".into(), false),
            Item::read(139, 1, "Prayer potion(3)".into(), false),
            Item::read(995, 10, "Coins".into(), true),
            Item::read(1712, 1, "Amulet of glory(4)".into(), false),
        ];
        let families = HashMap::from([
            (2434, vec![1, 2, 3, 4]),
            (139, vec![1, 2, 3, 4]),
            (1712, vec![1, 2, 3, 4, 5, 6]),
        ]);
        add_doses(items.iter_mut(), &families);
        assert_eq!(items[0].doses.as_ref().unwrap().accepted, [4]);
        assert_eq!(items[1].doses.as_ref().unwrap().accepted, [3]);
        assert!(items[2].doses.is_none());
        let mut doses = Doses {
            all: vec![1, 2, 3, 4],
            accepted: vec![3, 4],
            highest_first: true,
        };
        doses.sort();
        assert_eq!(doses.accepted, [4, 3]);
        share_doses(items.iter_mut(), "Prayer potion", &doses);
        assert_eq!(items[1].doses.as_ref().unwrap().accepted, [4, 3]);
        assert_eq!(items[3].doses.as_ref().unwrap().accepted, [4]);
        assert_eq!(items[0].to_value()["doses"], json!([4, 3]));
        let required = required_item(
            &injector::form::Required {
                doses: vec![3, 4],
                ..rule(&[139], None)
            },
            &Known::new(),
        );
        assert_eq!(required.to_value()["doses"], json!([3, 4]));
        assert!(!required.doses.unwrap().highest_first);
    }

    #[test]
    fn locked_prayers_and_their_conflicts_are_the_scripts() {
        let rules = Rules {
            locked: vec!["protect_from_melee".into(), "piety".into()],
            ..Default::default()
        };
        for key in [
            "protect_from_melee",
            "smite",
            "redemption",
            "piety",
            "zeal",
            "rigour",
            "steel_skin",
        ] {
            assert!(rules.blocks(key), "{key}");
        }
        assert!(!rules.blocks("rapid_heal"));
        assert!(!rules.blocks("protect_item"));
        let mut layout = Prayer {
            name: "P".into(),
            prayers: vec!["rapid_heal".into(), "rigour".into()],
        };
        rules.fit_prayer(&mut layout);
        assert_eq!(layout.prayers, ["rapid_heal"]);
        let saved = ScriptLayouts {
            prayer: vec![layout],
            ..Default::default()
        };
        let value = Kind::Prayer.value_of(&saved, "P", &rules, &Known::new());
        let locked = value["locked"].as_array().unwrap();
        assert!(locked.contains(&json!("smite")) && locked.contains(&json!("augury")));
        assert!(!locked.contains(&json!("rapid_heal")));
    }

    #[test]
    fn equipment_values_list_every_used_slot_with_its_mode() {
        let mut layout = Equipment {
            name: "Melee".into(),
            slots: vec![],
            ..Default::default()
        };
        *layout.slot_mut(3) = EquipmentSlot::Item(item(4151, 1, false));
        *layout.slot_mut(13) = EquipmentSlot::Any;
        let value = layout.to_value();
        let slots = value["slots"].as_array().unwrap();
        assert_eq!(slots.len(), EQUIPMENT_SLOTS.len());
        assert_eq!(slots[0], json!({ "slot": 0, "mode": "empty" }));
        assert_eq!(slots[3]["mode"], "item");
        assert_eq!(slots[3]["id"], 4151);
        assert_eq!(slots[10], json!({ "slot": 13, "mode": "any" }));
        assert_eq!(slots[11], json!({ "slot": 14, "mode": "any" }));
        assert_eq!(value["name"], "Melee");
        assert_eq!(value["spell"], Value::Null);
        layout.spell = Some("ice_barrage".into());
        layout.autocast = true;
        let value = layout.to_value();
        assert_eq!(value["spell"]["name"], "Ice Barrage");
        assert_eq!(value["spell"]["book"], "ancient");
        assert_eq!(value["spell"]["autocast"], true);
        assert_eq!(AUTOCAST_SPELLS.len(), 48);
        assert_eq!(spell_info("fire_surge").map(|s| s.level), Some(95));
    }

    #[test]
    fn item_params_parse_as_the_injector_prints_them() {
        let params: Vec<Option<std::collections::BTreeMap<u32, injector::config::Value>>> =
            serde_json::from_str(r#"[{"295":1},{"295":1,"595":"Empty","1910":1},null]"#).unwrap();
        assert_eq!(params.len(), 3);
        let quiver = params[1].as_ref().unwrap().get(&QUIVER_PARAM);
        assert!(matches!(quiver, Some(injector::config::Value::Int(1))));
    }

    #[test]
    fn a_layout_without_the_secondary_ammo_slot_doesnt_care_about_it() {
        let mut saved = Equipment {
            slots: vec![EquipmentSlot::Empty; SECONDARY_AMMO_SLOT],
            ..Default::default()
        };
        assert_eq!(saved.to_value()["slots"][11]["mode"], "any");
        *saved.slot_mut(0) = EquipmentSlot::Any;
        assert_eq!(saved.slots.len(), EQUIPMENT_SIZE);
        assert_eq!(*saved.slot(SECONDARY_AMMO_SLOT), EquipmentSlot::Any);
        assert_eq!(saved.to_value()["slots"][1]["mode"], "empty");
    }

    #[test]
    fn inventory_values_keep_slot_numbers_and_the_pouch() {
        let mut layout = Inventory::default();
        *layout.slot_mut(2) = Some(item(995, 1000, true));
        *layout.slot_mut(27) = Some(Item {
            name: "Rune pouch".into(),
            ..item(12791, 1, false)
        });
        layout.pouch = Some(vec![item(563, 500, true)]);
        let value = layout.to_value();
        let slots = value["slots"].as_array().unwrap();
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0]["slot"], 2);
        assert_eq!(slots[0]["max"], 1000);
        assert_eq!(slots[1]["slot"], 27);
        assert_eq!(value["pouch"][0]["id"], 563);
        assert!(layout.holds_pouch());
        let plain = Inventory::default();
        assert_eq!(plain.to_value()["pouch"], Value::Null);
        assert!(!plain.holds_pouch());
    }

    #[test]
    fn the_editor_opens_saves_and_tracks_changes() {
        let mut saved = vec![Equipment {
            name: "A".into(),
            slots: vec![],
            ..Default::default()
        }];
        let mut editor: Editor<Equipment> = Editor::default();
        assert!(editor.dirty(&saved));
        editor.open(&saved, Some(0));
        assert!(!editor.dirty(&saved));
        *editor.draft.slot_mut(0) = EquipmentSlot::Any;
        assert!(editor.dirty(&saved));
        editor.open(&saved, Some(5));
        assert_eq!(editor.index, None);
        editor.finish_read(
            Ok(Read::Equipment {
                autocast: false,
                slots: vec![EquipmentSlot::Item(item(1, 1, false))],
            }),
            apply_equipment,
        );
        assert_eq!(editor.draft.slots.len(), EQUIPMENT_SIZE);
        assert!(matches!(editor.draft.slots[0], EquipmentSlot::Item(_)));
        editor.finish_read(Err("gone".into()), apply_equipment);
        assert_eq!(editor.error.as_deref(), Some("gone"));
        saved.push(editor.draft.clone());
        assert_eq!(
            Kind::Equipment.names(&ScriptLayouts {
                equipment: saved.clone(),
                inventory: vec![],
                prayer: vec![]
            }),
            ["A", ""]
        );
    }

    #[test]
    fn the_editor_follows_its_layout_when_the_list_changes_under_it() {
        let named = |name: &str| Equipment {
            name: name.into(),
            ..Default::default()
        };
        let mut saved = vec![named("A"), named("B"), named("C")];
        let mut editor: Editor<Equipment> = Editor::default();
        editor.open(&saved, Some(2));

        saved.remove(0);
        editor.follow(&saved);
        assert_eq!(editor.index, Some(1));

        saved.remove(1);
        editor.follow(&saved);
        assert_eq!(editor.index, None);
        assert_eq!(editor.draft.name, "C");
        assert!(editor.dirty(&saved));
    }

    #[test]
    fn a_copy_gets_a_name_no_layout_has() {
        let named = |name: &str| Equipment {
            name: name.into(),
            ..Default::default()
        };
        let mut saved = vec![named("X")];
        assert_eq!(copy_name(&saved, "X"), "X copy");
        saved.push(named("X copy"));
        assert_eq!(copy_name(&saved, "X"), "X copy 2");
        saved.push(named("X copy 2"));
        assert_eq!(copy_name(&saved, "X"), "X copy 3");
    }

    #[test]
    fn a_prayer_takes_out_the_ones_it_conflicts_with() {
        let mut layout = Prayer::default();
        layout.toggle("protect_from_melee");
        layout.toggle("steel_skin");
        layout.toggle("ultimate_strength");
        layout.toggle("rapid_heal");
        assert_eq!(
            layout.prayers,
            [
                "rapid_heal",
                "steel_skin",
                "ultimate_strength",
                "protect_from_melee"
            ]
        );
        layout.toggle("smite");
        layout.toggle("piety");
        assert_eq!(layout.prayers, ["rapid_heal", "smite", "piety"]);
        layout.toggle("piety");
        assert_eq!(layout.prayers, ["rapid_heal", "smite"]);
        assert_eq!(layout.to_value()["prayers"][1], "smite");
        assert_eq!(PRAYERS.len(), 32);
        let mut bits: Vec<u32> = PRAYERS
            .iter()
            .filter(|p| p.replaces.is_none())
            .map(|p| p.quick_bit)
            .collect();
        bits.sort_unstable();
        assert_eq!(bits, (0..29).collect::<Vec<u32>>());
        assert_eq!(
            prayer_info("zeal").unwrap().quick_bit,
            prayer_info("piety").unwrap().quick_bit
        );
        layout.toggle("piety");
        layout.toggle("zeal");
        assert_eq!(layout.prayers, ["rapid_heal", "smite", "zeal"]);
    }

    #[test]
    fn container_items_carry_names_and_stackability() {
        let container = vec![
            injector::Item {
                slot: 0,
                id: 995,
                quantity: 5,
            },
            injector::Item {
                slot: 1,
                id: -1,
                quantity: 0,
            },
        ];
        let names = HashMap::from([(995, "Coins".to_owned())]);
        let stackable = HashMap::from([(995, true)]);
        let items = items_of(&container, &names, &stackable);
        assert_eq!(items[0].as_ref().unwrap().name, "Coins");
        assert!(items[0].as_ref().unwrap().stackable);
        assert!(items[1].is_none());
        assert!(item(12791, 1, false).to_value()["stackable"] == false);
        assert!(
            Item {
                name: "Divine rune pouch".into(),
                ..item(1, 1, false)
            }
            .is_rune_pouch()
        );
    }
}
