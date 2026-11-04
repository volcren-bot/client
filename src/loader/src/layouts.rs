//! Equipment and inventory layouts: what a script's `equipment` and `inventory` form fields pick from, kept per script, edited on the running script's page and read from a client.

use std::collections::HashMap;

use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::icons::{Icons, Key, SLOT_SIZE};
use crate::theme;

/// The worn items container's slots that exist, with their names, in the container's order.
pub const EQUIPMENT_SLOTS: [(usize, &str); 11] = [
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
];
/// Where each slot sits in the worn items tab: column and row of a 3 x 5 grid.
const EQUIPMENT_CELLS: [(usize, usize, usize); 11] = [
    (0, 1, 0),
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
pub const EQUIPMENT_SIZE: usize = 14;
pub const INVENTORY_SIZE: usize = 28;
pub const INVENTORY_COLUMNS: usize = 4;
/// A rune pouch holds up to 4 kinds of rune (3 unless divine).
pub const POUCH_SIZE: usize = 4;
/// The varbits of the runes in the pouch and of their amounts, in slot order.
pub const POUCH_RUNE_VARBITS: [i32; POUCH_SIZE] = [29, 1622, 1623, 14285];
pub const POUCH_AMOUNT_VARBITS: [i32; POUCH_SIZE] = [1624, 1625, 1626, 14286];
/// The enum that maps a pouch varbit's rune number to the rune's item id.
pub const POUCH_RUNES_ENUM: i32 = 982;

/// What a prayer boosts, for the ones that can't be on together: two prayers conflict when they share a flag.
pub mod boost {
    pub const ATTACK: u8 = 1;
    pub const STRENGTH: u8 = 2;
    pub const DEFENCE: u8 = 4;
    pub const RANGED: u8 = 8;
    pub const MAGIC: u8 = 16;
    pub const OVERHEAD: u8 = 32;
    /// Chivalry, Piety, Rigour and Augury can't be used with any other stat prayer.
    pub const ALL_STATS: u8 = ATTACK | STRENGTH | DEFENCE | RANGED | MAGIC;
}

/// One prayer of the book: the key the lib's `Prayer.PRAYERS` uses, its level, the varbit that is 1 while it is on, its bit in the
/// quick prayer selection (varbit `QUICK_SELECTED_VARBIT`), the sprite the prayer tab shows and what it conflicts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrayerInfo {
    pub key: &'static str,
    pub name: &'static str,
    pub level: u32,
    pub varbit: i32,
    pub quick_bit: u32,
    pub sprite: i32,
    pub boosts: u8,
    /// For an upgrade (Deadeye, Mystic Vigour, Zeal): the key of the prayer whose slot and quick bit it takes once `unlock` is 1. Every one is shown, since which an account has can't be known without reading it.
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

/// The prayer book in the order the tab shows it (by level), `PRAYER_COLUMNS` to a row; the three upgrades sit at their own levels.
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
/// The varbit whose bits (by `quick_bit`) are the quick prayer selection.
pub const QUICK_SELECTED_VARBIT: i32 = 4102;

pub fn prayer_info(key: &str) -> Option<&'static PrayerInfo> {
    PRAYERS.iter().find(|p| p.key == key)
}

/// The spellbooks with combat spells a weapon can autocast.
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

/// A spell a weapon can autocast: its slot in the game's autocast list (enum 1986, what varbit 276 holds), the key and name the
/// lib's `Magic` module knows it by, its Magic level, the 24 px sprite the spellbook and the autocast picker draw it with, and its book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellInfo {
    pub index: u32,
    pub key: &'static str,
    pub name: &'static str,
    pub level: u32,
    pub sprite: i32,
    pub book: Book,
}

const fn spell(
    index: u32,
    key: &'static str,
    name: &'static str,
    level: u32,
    sprite: i32,
    book: Book,
) -> SpellInfo {
    SpellInfo {
        index,
        key,
        name,
        level,
        sprite,
        book,
    }
}

/// Every autocastable spell, from the client's autocast enum and each spell's params (name, level, sprite), in the game's order.
pub const AUTOCAST_SPELLS: [SpellInfo; 48] = [
    spell(1, "wind_strike", "Wind Strike", 1, 15, Book::Standard),
    spell(2, "water_strike", "Water Strike", 5, 17, Book::Standard),
    spell(3, "earth_strike", "Earth Strike", 9, 19, Book::Standard),
    spell(4, "fire_strike", "Fire Strike", 13, 21, Book::Standard),
    spell(5, "wind_bolt", "Wind Bolt", 17, 23, Book::Standard),
    spell(6, "water_bolt", "Water Bolt", 23, 26, Book::Standard),
    spell(7, "earth_bolt", "Earth Bolt", 29, 29, Book::Standard),
    spell(8, "fire_bolt", "Fire Bolt", 35, 32, Book::Standard),
    spell(9, "wind_blast", "Wind Blast", 41, 35, Book::Standard),
    spell(10, "water_blast", "Water Blast", 47, 38, Book::Standard),
    spell(11, "earth_blast", "Earth Blast", 53, 40, Book::Standard),
    spell(12, "fire_blast", "Fire Blast", 59, 44, Book::Standard),
    spell(13, "wind_wave", "Wind Wave", 62, 46, Book::Standard),
    spell(14, "water_wave", "Water Wave", 65, 48, Book::Standard),
    spell(15, "earth_wave", "Earth Wave", 70, 51, Book::Standard),
    spell(16, "fire_wave", "Fire Wave", 75, 52, Book::Standard),
    spell(
        17,
        "crumble_undead",
        "Crumble Undead",
        39,
        34,
        Book::Standard,
    ),
    spell(18, "magic_dart", "Magic Dart", 50, 324, Book::Standard),
    spell(
        19,
        "claws_of_guthix",
        "Claws of Guthix",
        60,
        60,
        Book::Standard,
    ),
    spell(
        20,
        "flames_of_zamorak",
        "Flames of Zamorak",
        60,
        59,
        Book::Standard,
    ),
    spell(31, "smoke_rush", "Smoke Rush", 50, 329, Book::Ancient),
    spell(32, "shadow_rush", "Shadow Rush", 52, 337, Book::Ancient),
    spell(33, "blood_rush", "Blood Rush", 56, 333, Book::Ancient),
    spell(34, "ice_rush", "Ice Rush", 58, 325, Book::Ancient),
    spell(35, "smoke_burst", "Smoke Burst", 62, 330, Book::Ancient),
    spell(36, "shadow_burst", "Shadow Burst", 64, 338, Book::Ancient),
    spell(37, "blood_burst", "Blood Burst", 68, 334, Book::Ancient),
    spell(38, "ice_burst", "Ice Burst", 70, 326, Book::Ancient),
    spell(39, "smoke_blitz", "Smoke Blitz", 74, 331, Book::Ancient),
    spell(40, "shadow_blitz", "Shadow Blitz", 76, 339, Book::Ancient),
    spell(41, "blood_blitz", "Blood Blitz", 80, 335, Book::Ancient),
    spell(42, "ice_blitz", "Ice Blitz", 82, 327, Book::Ancient),
    spell(43, "smoke_barrage", "Smoke Barrage", 86, 332, Book::Ancient),
    spell(
        44,
        "shadow_barrage",
        "Shadow Barrage",
        88,
        340,
        Book::Ancient,
    ),
    spell(45, "blood_barrage", "Blood Barrage", 92, 336, Book::Ancient),
    spell(46, "ice_barrage", "Ice Barrage", 94, 328, Book::Ancient),
    spell(47, "iban_blast", "Iban Blast", 50, 53, Book::Standard),
    spell(48, "wind_surge", "Wind Surge", 81, 362, Book::Standard),
    spell(49, "water_surge", "Water Surge", 85, 363, Book::Standard),
    spell(50, "earth_surge", "Earth Surge", 90, 364, Book::Standard),
    spell(51, "fire_surge", "Fire Surge", 95, 365, Book::Standard),
    spell(
        52,
        "saradomin_strike",
        "Saradomin Strike",
        60,
        61,
        Book::Standard,
    ),
    spell(
        53,
        "inferior_demonbane",
        "Inferior Demonbane",
        44,
        1302,
        Book::Arceuus,
    ),
    spell(
        54,
        "superior_demonbane",
        "Superior Demonbane",
        62,
        1303,
        Book::Arceuus,
    ),
    spell(
        55,
        "dark_demonbane",
        "Dark Demonbane",
        82,
        1304,
        Book::Arceuus,
    ),
    spell(
        56,
        "ghostly_grasp",
        "Ghostly Grasp",
        35,
        1267,
        Book::Arceuus,
    ),
    spell(
        57,
        "skeletal_grasp",
        "Skeletal Grasp",
        56,
        1268,
        Book::Arceuus,
    ),
    spell(58, "undead_grasp", "Undead Grasp", 79, 1269, Book::Arceuus),
];
pub const SPELL_COLUMNS: usize = 8;
/// The weapon categories (varbit 357) whose combat tab offers autocast: staves, powered staves and the Tumeken's shadow kind.
pub const AUTOCAST_WEAPON_CATEGORIES: [i32; 3] = [18, 21, 22];
pub const WEAPON_CATEGORY_VARBIT: i32 = 357;

pub fn spell_info(key: &str) -> Option<&'static SpellInfo> {
    AUTOCAST_SPELLS.iter().find(|s| s.key == key)
}

/// An item in a layout, with what the editor needs to show it without a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: i32,
    pub quantity: i32,
    pub name: String,
    pub stackable: bool,
}

impl Item {
    fn icon_key(&self) -> Key {
        Key::item(self.id, self.quantity, self.stackable)
    }

    fn to_value(&self) -> Value {
        json!({ "id": self.id, "quantity": self.quantity, "name": self.name, "stackable": self.stackable })
    }

    /// Whether the name is a rune pouch's (plain, looted or divine).
    pub fn is_rune_pouch(&self) -> bool {
        self.name.to_ascii_lowercase().contains("rune pouch")
    }
}

/// What an equipment slot asks for: nothing worn there, anything, or one item.
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
    /// The combat spell to autocast, or to cast by hand when the weapon can't autocast; a key of `AUTOCAST_SPELLS`.
    #[serde(default)]
    pub spell: Option<String>,
    /// Whether the weapon read from the client could autocast (its combat tab offered it).
    #[serde(default)]
    pub autocast: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub name: String,
    pub slots: Vec<Option<Item>>,
    /// The runes in the rune pouch, when the inventory holds one.
    pub pouch: Option<Vec<Item>>,
}

/// A set of prayers, by key, that a script turns on or makes its quick prayers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prayer {
    pub name: String,
    pub prayers: Vec<String>,
}

impl Prayer {
    fn has(&self, key: &str) -> bool {
        self.prayers.iter().any(|k| k == key)
    }

    /// Adds the prayer, taking out the ones it can't be on with, or takes it out when it is in.
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

    /// Keeps the book's order, which is how the script gets them.
    fn sort(&mut self) {
        self.prayers.sort_by_key(|k| {
            PRAYERS
                .iter()
                .position(|p| p.key == k)
                .unwrap_or(usize::MAX)
        });
    }
}

/// What a script's form fields can pick from, by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLayouts {
    pub equipment: Vec<Equipment>,
    pub inventory: Vec<Inventory>,
    #[serde(default)]
    pub prayer: Vec<Prayer>,
}

/// Every script's layouts, by the title of its form.
pub type AllLayouts = HashMap<String, ScriptLayouts>;

pub trait Layout: Clone + Default + PartialEq {
    const KIND: Kind;
    fn name(&self) -> &str;
    fn set_name(&mut self, name: String);
    /// What the script gets as the field's value.
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

    /// The names of the saved layouts of this kind.
    pub fn names(self, layouts: &ScriptLayouts) -> Vec<&str> {
        match self {
            Kind::Equipment => layouts.equipment.iter().map(|l| l.name.as_str()).collect(),
            Kind::Inventory => layouts.inventory.iter().map(|l| l.name.as_str()).collect(),
            Kind::Prayer => layouts.prayer.iter().map(|l| l.name.as_str()).collect(),
        }
    }

    /// What the script gets for the layout called `name`, or null when there is none.
    pub fn value_of(self, layouts: &ScriptLayouts, name: &str) -> Value {
        match self {
            Kind::Equipment => layouts
                .equipment
                .iter()
                .find(|l| l.name == name)
                .map(Layout::to_value),
            Kind::Inventory => layouts
                .inventory
                .iter()
                .find(|l| l.name == name)
                .map(Layout::to_value),
            Kind::Prayer => layouts
                .prayer
                .iter()
                .find(|l| l.name == name)
                .map(Layout::to_value),
        }
        .unwrap_or(Value::Null)
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
            .map(|&(slot, _)| match self.slots.get(slot) {
                Some(EquipmentSlot::Item(item)) => {
                    let mut value = item.to_value();
                    value["slot"] = json!(slot);
                    value["mode"] = json!("item");
                    value
                }
                Some(EquipmentSlot::Any) => json!({ "slot": slot, "mode": "any" }),
                _ => json!({ "slot": slot, "mode": "empty" }),
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
        self.slots.get(slot).unwrap_or(&EquipmentSlot::Empty)
    }

    fn slot_mut(&mut self, slot: usize) -> &mut EquipmentSlot {
        if self.slots.len() < EQUIPMENT_SIZE {
            self.slots.resize(EQUIPMENT_SIZE, EquipmentSlot::Empty);
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

    fn holds_pouch(&self) -> bool {
        self.slots.iter().flatten().any(Item::is_rune_pouch)
    }
}

/// What a client read for the editor: the worn items, or the inventory with the pouch's runes.
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
    /// The prayers on, or the quick prayer selection, by key.
    Prayers(Vec<String>),
}

/// The state of one editor tab: which saved layout is open (none: a new one) and the copy being edited.
#[derive(Debug, Clone, Default)]
pub struct Editor<L> {
    pub index: Option<usize>,
    pub draft: L,
    pub selected: Option<usize>,
    /// The selected slot is in the rune pouch, not the inventory.
    pub selected_in_pouch: bool,
    pub reading: bool,
    pub error: Option<String>,
}

/// Both editor tabs of the running script's page.
#[derive(Debug, Clone, Default)]
pub struct Editors {
    pub equipment: Editor<Equipment>,
    pub inventory: Editor<Inventory>,
    pub prayer: Editor<Prayer>,
}

/// What the editor's header asked the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read the layout from the selected client: the worn items, the inventory, or the prayers on.
    Read,
    /// Read the quick prayer selection from the selected client.
    ReadQuick,
}

const NEW_LAYOUT: &str = "New layout";

impl<L: Layout> Editor<L> {
    /// Opens the saved layout at `index`, or a new one.
    fn open(&mut self, saved: &[L], index: Option<usize>) {
        self.index = index.filter(|&i| i < saved.len());
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

    /// Whether the draft differs from what is saved (a new layout always does).
    fn dirty(&self, saved: &[L]) -> bool {
        self.index.and_then(|i| saved.get(i)) != Some(&self.draft)
    }

    /// The layout dropdown, the name, Save, Duplicate and Remove.
    fn header(&mut self, ui: &mut egui::Ui, saved: &mut Vec<L>) {
        let mut open: Option<Option<usize>> = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Layout:");
            let current = self
                .index
                .and_then(|i| saved.get(i))
                .map_or(NEW_LAYOUT, |l| l.name());
            egui::ComboBox::from_id_salt((L::KIND.field_kind(), "layout_picker"))
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
            ui.label("Name:");
            let mut name = self.draft.name().to_owned();
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(160.0))
                .changed()
            {
                self.draft.set_name(name);
            }
        });
        ui.horizontal_wrapped(|ui| {
            let name = self.draft.name().trim().to_owned();
            let taken = saved
                .iter()
                .enumerate()
                .any(|(i, l)| Some(i) != self.index && l.name() == name);
            let can_save = !name.is_empty() && !taken && self.dirty(saved);
            if ui
                .add_enabled(can_save, egui::Button::new("Save"))
                .on_disabled_hover_text(if taken {
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
            }
            if ui
                .add_enabled(self.index.is_some(), egui::Button::new("Duplicate"))
                .clicked()
                && let Some(i) = self.index
            {
                let mut copy = saved[i].clone();
                copy.set_name(format!("{} copy", copy.name()));
                saved.push(copy);
                open = Some(Some(saved.len() - 1));
            }
            if ui
                .add_enabled(self.index.is_some(), egui::Button::new("Remove"))
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

    /// The column above a tab's grid: its hint, Read from client and what a failed read said.
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
                ui.weak(hint);
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
                    self.reading = true;
                    self.error = None;
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
                    self.reading = true;
                    self.error = None;
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

/// The width of a grid of `columns` cells.
const fn grid_width(columns: usize, cell: f32, gap: f32) -> f32 {
    columns as f32 * cell + (columns - 1) as f32 * gap
}

const PRAYER_CELL: f32 = 42.0;
const PRAYER_GAP: f32 = 4.0;
/// The width every tab's hint, buttons and grid share: the prayer book's, the widest grid.
const COLUMN_WIDTH: f32 = grid_width(PRAYER_COLUMNS, PRAYER_CELL, PRAYER_GAP);

/// Draws one slot: the item's icon, or what an empty slot means; selected slots get the accent border.
fn slot_ui(
    ui: &mut egui::Ui,
    icons: &mut Icons,
    item: Option<&Item>,
    empty_text: &str,
    selected: bool,
    sense: egui::Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(SLOT_SIZE + egui::vec2(8.0, 8.0), sense);
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
            Some(item) => match icons.texture(ui.ctx(), item.icon_key()) {
                Some(texture) => {
                    egui::Image::from_texture(texture).paint_at(ui, inner);
                }
                None => {
                    ui.painter().text(
                        inner.center(),
                        egui::Align2::CENTER_CENTER,
                        item.name.chars().take(4).collect::<String>(),
                        egui::TextStyle::Small.resolve(ui.style()),
                        ui.visuals().weak_text_color(),
                    );
                }
            },
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

fn item_hover(item: &Item) -> String {
    if item.stackable {
        format!("{} x{}  (id {})", item.name, item.quantity, item.id)
    } else {
        format!("{}  (id {})", item.name, item.id)
    }
}

/// The amount of a stackable item, editable; a non stackable one is always one.
fn amount_ui(ui: &mut egui::Ui, item: &mut Item) {
    ui.horizontal(|ui| {
        ui.label(format!("{}  (id {})", item.name, item.id));
        if item.stackable {
            ui.label("Amount:");
            ui.add(
                egui::DragValue::new(&mut item.quantity)
                    .range(1..=i32::MAX)
                    .speed(1.0),
            );
        }
    });
}

/// The Equipment tab: the worn items grid as the game lays it out, read from a client, with each slot's meaning editable.
pub fn equipment_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Equipment>,
    saved: &mut Vec<Equipment>,
    icons: &mut Icons,
    can_read: bool,
) -> Option<Action> {
    let cell = SLOT_SIZE + egui::vec2(8.0, 8.0);
    let gap = 6.0;
    let width = grid_width(3, cell.x, gap);
    let margin = (COLUMN_WIDTH - width) / 2.0;
    editor.header(ui, saved);
    ui.add_space(6.0);
    let hint = "Right-click an item to empty its slot.";
    let action = editor.read_ui(ui, Some(hint), can_read, false);
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        let (grid, _) = ui.allocate_exact_size(
            egui::vec2(width, 5.0 * cell.y + 4.0 * gap),
            egui::Sense::hover(),
        );
        for &(slot, column, row) in &EQUIPMENT_CELLS {
            let min =
                grid.min + egui::vec2(column as f32 * (cell.x + gap), row as f32 * (cell.y + gap));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(min, cell))
                    .layout(egui::Layout::left_to_right(egui::Align::Min)),
            );
            let name = EQUIPMENT_SLOTS
                .iter()
                .find(|(s, _)| *s == slot)
                .map_or("", |(_, name)| name);
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
                *editor.draft.slot_mut(slot) = EquipmentSlot::Empty;
            }
        }
        ui.add_space(margin + 12.0);
        ui.vertical(|ui| {
            if let Some(slot) = editor.selected {
                let name = EQUIPMENT_SLOTS
                    .iter()
                    .find(|(s, _)| *s == slot)
                    .map_or("", |(_, name)| name);
                ui.strong(name);
                match editor.draft.slot_mut(slot) {
                    EquipmentSlot::Item(item) => amount_ui(ui, item),
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
    });
    ui.add_space(10.0);
    spell_ui(ui, &mut editor.draft, icons);
    action
}

/// The spell of an equipment layout: the autocast spell when the weapon read from the client offered autocast, else a spell the
/// script casts by hand. The three books with combat spells are shown in the game's autocast order; a click picks or unpicks one.
fn spell_ui(ui: &mut egui::Ui, draft: &mut Equipment, icons: &mut Icons) {
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
    for book in [Book::Standard, Book::Ancient, Book::Arceuus] {
        ui.add_space(4.0);
        ui.label(book.label());
        egui::Grid::new(("spell_grid", book.key()))
            .spacing(egui::vec2(3.0, 3.0))
            .show(ui, |ui| {
                let mut shown = 0;
                for info in AUTOCAST_SPELLS.iter().filter(|s| s.book == book) {
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
                        match icons.texture(ui.ctx(), Key::Sprite(info.sprite)) {
                            Some(texture) => {
                                let size = texture.size_vec2().min(cell - egui::vec2(4.0, 4.0));
                                egui::Image::from_texture(texture).paint_at(
                                    ui,
                                    egui::Rect::from_center_size(rect.center(), size),
                                );
                            }
                            None => {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    info.name
                                        .split(' ')
                                        .map(|w| w.chars().next().unwrap_or(' '))
                                        .collect::<String>(),
                                    egui::TextStyle::Small.resolve(ui.style()),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                        }
                    }
                    if response
                        .on_hover_text(format!("{} (level {})", info.name, info.level))
                        .clicked()
                    {
                        clicked = Some(info.key);
                    }
                    shown += 1;
                    if shown % SPELL_COLUMNS == 0 {
                        ui.end_row();
                    }
                }
            });
    }
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

/// The Inventory tab: the 4 x 7 grid with drag to move items, amounts of stackables, and the rune pouch's runes when there is one.
pub fn inventory_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Inventory>,
    saved: &mut Vec<Inventory>,
    icons: &mut Icons,
    can_read: bool,
) -> Option<Action> {
    let gap = 6.0;
    let margin = (COLUMN_WIDTH - grid_width(INVENTORY_COLUMNS, SLOT_SIZE.x + 8.0, gap)) / 2.0;
    editor.header(ui, saved);
    ui.add_space(6.0);
    let hint = "Drag an item to move it. Right-click an item to empty its slot.";
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
                            let hover = item_hover(item);
                            ui.dnd_drag_source(
                                egui::Id::new(("inventory_layout_slot", slot)),
                                slot,
                                |ui| {
                                    slot_ui(
                                        ui,
                                        icons,
                                        Some(item),
                                        "",
                                        selected,
                                        egui::Sense::click_and_drag(),
                                    )
                                    .on_hover_text(hover)
                                },
                            )
                            .inner
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
                        *editor.draft.slot_mut(slot) = None;
                        editor.selected = Some(slot);
                        editor.selected_in_pouch = false;
                    }
                    if (slot + 1) % INVENTORY_COLUMNS == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(margin + 12.0);
        ui.vertical(|ui| {
            if let Some(slot) = editor.selected.filter(|_| !editor.selected_in_pouch) {
                ui.strong(format!("Slot {slot}"));
                match editor.draft.slot_mut(slot) {
                    Some(item) => amount_ui(ui, item),
                    None => {
                        ui.label("Empty.");
                    }
                }
            }
            if editor.draft.pouch.is_some() || editor.draft.holds_pouch() {
                ui.add_space(10.0);
                ui.strong("Rune pouch");
                let runes = editor.draft.pouch.get_or_insert_with(Vec::new);
                ui.horizontal(|ui| {
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
                    amount_ui(ui, rune);
                }
                ui.weak("Read from client fills it from the pouch's contents.");
            }
        });
    });
    if let Some((from, to)) = moved {
        editor.draft.slot_mut(from);
        editor.draft.slot_mut(to);
        editor.draft.slots.swap(from, to);
        editor.selected = Some(to);
        editor.selected_in_pouch = false;
    }
    action
}

/// The Prayer tab: the prayer book as the tab lays it out; clicking a prayer puts it in the layout and takes out the ones it can't be
/// on with. A prayer above the client's Prayer level (when known) is dimmed but can still be picked.
pub fn prayer_ui(
    ui: &mut egui::Ui,
    editor: &mut Editor<Prayer>,
    saved: &mut Vec<Prayer>,
    icons: &mut Icons,
    can_read: bool,
    prayer_level: Option<u32>,
) -> Option<Action> {
    let cell = egui::Vec2::splat(PRAYER_CELL);
    editor.header(ui, saved);
    ui.add_space(6.0);
    let action = editor.read_ui(ui, None, can_read, true);
    ui.add_space(6.0);
    let mut toggled: Option<&'static str> = None;
    egui::Grid::new("prayer_layout_grid")
        .spacing(egui::Vec2::splat(PRAYER_GAP))
        .show(ui, |ui| {
            for (i, info) in PRAYERS.iter().enumerate() {
                let on = editor.draft.has(info.key);
                let too_high = prayer_level.is_some_and(|level| level < info.level);
                let (rect, response) = ui.allocate_exact_size(cell, egui::Sense::click());
                if ui.is_rect_visible(rect) {
                    let visuals = ui.style().interact(&response);
                    let fill = if on {
                        theme::BLUE.linear_multiply(0.35)
                    } else {
                        visuals.weak_bg_fill
                    };
                    let stroke = if on {
                        egui::Stroke::new(1.5, theme::BLUE)
                    } else {
                        visuals.bg_stroke
                    };
                    ui.painter()
                        .rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
                    let tint = if too_high {
                        egui::Color32::from_white_alpha(90)
                    } else {
                        egui::Color32::WHITE
                    };
                    match icons.texture(ui.ctx(), Key::Sprite(info.sprite)) {
                        Some(texture) => {
                            let size = texture.size_vec2().min(cell - egui::vec2(6.0, 6.0));
                            egui::Image::from_texture(texture)
                                .tint(tint)
                                .paint_at(ui, egui::Rect::from_center_size(rect.center(), size));
                        }
                        None => {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                info.name
                                    .split(' ')
                                    .map(|w| w.chars().next().unwrap_or(' '))
                                    .collect::<String>(),
                                egui::TextStyle::Small.resolve(ui.style()),
                                ui.visuals().weak_text_color(),
                            );
                        }
                    }
                }
                let hover = if too_high {
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
                    toggled = Some(info.key);
                }
                if (i + 1) % PRAYER_COLUMNS == 0 {
                    ui.end_row();
                }
            }
        });
    if let Some(key) = toggled {
        editor.draft.toggle(key);
    }
    ui.add_space(6.0);
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
}

/// Puts what a client read into a prayer draft.
pub fn apply_prayers(draft: &mut Prayer, read: Read) {
    if let Read::Prayers(keys) = read {
        draft.prayers = keys;
        draft.sort();
    }
}

/// Puts what a client read into an equipment draft.
pub fn apply_equipment(draft: &mut Equipment, read: Read) {
    if let Read::Equipment { slots, autocast } = read {
        draft.slots = slots;
        draft.slots.resize(EQUIPMENT_SIZE, EquipmentSlot::Empty);
        draft.autocast = autocast;
    }
}

/// Puts what a client read into an inventory draft.
pub fn apply_inventory(draft: &mut Inventory, read: Read) {
    if let Read::Inventory { slots, pouch } = read {
        draft.slots = slots;
        draft.slots.resize(INVENTORY_SIZE, None);
        draft.pouch = pouch;
    }
}

/// Item names and stackability by id, as read from a client.
pub type Described = (HashMap<i32, String>, HashMap<i32, bool>);

/// The items of a container read from a client, with their names and stackability.
pub fn items_of(
    container: &[injector::Item],
    names: &HashMap<i32, String>,
    stackable: &HashMap<i32, bool>,
) -> Vec<Option<Item>> {
    container
        .iter()
        .map(|slot| {
            (slot.id >= 0).then(|| Item {
                id: slot.id,
                quantity: slot.quantity,
                name: names.get(&slot.id).cloned().unwrap_or_default(),
                stackable: stackable.get(&slot.id).copied().unwrap_or(false),
            })
        })
        .collect()
}

/// A dropdown of the script's layouts of `kind` for the form's field, choosing by name; "None" picks nothing.
pub fn picker_ui(
    ui: &mut egui::Ui,
    kind: Kind,
    layouts: &ScriptLayouts,
    chosen: &mut Option<String>,
    id: &str,
) {
    let names = kind.names(layouts);
    if chosen.as_deref().is_some_and(|name| !names.contains(&name)) {
        *chosen = None;
    }
    egui::ComboBox::from_id_salt(("layout_field", id))
        .selected_text(chosen.as_deref().unwrap_or("None"))
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, "None");
            for name in names {
                ui.selectable_value(chosen, Some(name.to_owned()), name);
            }
        });
    ui.weak(format!("Edit them in the {} tab.", kind.label()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i32, quantity: i32, stackable: bool) -> Item {
        Item {
            id,
            quantity,
            name: format!("item {id}"),
            stackable,
        }
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
        assert_eq!(value["name"], "Melee");
        assert_eq!(value["spell"], Value::Null);
        layout.spell = Some("ice_barrage".into());
        layout.autocast = true;
        let value = layout.to_value();
        assert_eq!(value["spell"]["name"], "Ice Barrage");
        assert_eq!(value["spell"]["book"], "ancient");
        assert_eq!(value["spell"]["autocast"], true);
        assert_eq!(AUTOCAST_SPELLS.len(), 48);
        assert_eq!(spell_info("fire_surge").map(|s| s.index), Some(51));
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
        assert_eq!(slots[0]["quantity"], 1000);
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
        // another overhead replaces the overhead, piety replaces every stat prayer
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
        // an upgrade shares its base's bit and can't be in a layout with it
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
