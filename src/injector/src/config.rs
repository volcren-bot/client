use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ENUM_ARCHIVE: i32 = 8;
pub const OBJ_ARCHIVE: i32 = 10;
pub const VARBIT_ARCHIVE: i32 = 14;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumEntry {
    pub key: i32,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumDef {
    pub key_type: String,
    pub value_type: String,
    pub default: Option<Value>,
    pub entries: Vec<EnumEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarbitDef {
    pub varp: u16,
    pub lsb: u8,
    pub msb: u8,
}

impl VarbitDef {
    pub fn value(&self, varp: i32) -> i32 {
        let bits = (u32::from(self.msb) + 1).saturating_sub(u32::from(self.lsb));
        let mask = if bits >= 32 {
            u32::MAX
        } else {
            (1u32 << bits) - 1
        };
        ((varp as u32).checked_shr(u32::from(self.lsb)).unwrap_or(0) & mask) as i32
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let v = *self.data.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }

    fn u24(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes([0, self.u8()?, self.u8()?, self.u8()?]))
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes([
            self.u8()?,
            self.u8()?,
            self.u8()?,
            self.u8()?,
        ]))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(((self.i32()? as i64) << 32) | self.i32()? as u32 as i64)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.data.len()).then(|| self.at += n)
    }

    fn string(&mut self) -> Option<String> {
        let end = self.data.get(self.at..)?.iter().position(|&b| b == 0)?;
        let text = self.data[self.at..self.at + end]
            .iter()
            .map(|&b| cp1252(b))
            .collect();
        self.at += end + 1;
        Some(text)
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

pub fn decode_enum(data: &[u8]) -> Option<EnumDef> {
    let mut r = Reader::new(data);
    let mut def = EnumDef {
        key_type: String::new(),
        value_type: String::new(),
        default: None,
        entries: Vec::new(),
    };
    loop {
        match r.u8()? {
            0 => return Some(def),
            1 => def.key_type = (r.u8()? as char).to_string(),
            2 => def.value_type = (r.u8()? as char).to_string(),
            3 => def.default = Some(Value::Str(r.string()?)),
            4 => def.default = Some(Value::Int(r.i32()? as i64)),
            8 => def.default = Some(Value::Int(r.i64()?)),
            op @ 5..=7 => {
                let size = r.u16()?;
                def.entries = (0..size)
                    .map(|_| {
                        let key = r.i32()?;
                        let value = match op {
                            5 => Value::Str(r.string()?),
                            6 => Value::Int(r.i32()? as i64),
                            _ => Value::Int(r.i64()?),
                        };
                        Some(EnumEntry { key, value })
                    })
                    .collect::<Option<_>>()?;
            }
            _ => return None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjDef {
    pub name: Option<String>,
    pub params: BTreeMap<u32, Value>,
    pub stackable: bool,
    pub copy: bool,
}

pub fn dose_of(name: &str) -> Option<(&str, u32)> {
    let inner = name.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let digits = &inner[open + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || open == 0 {
        return None;
    }
    Some((&name[..open], digits.parse().ok()?))
}

pub type DoseFamily = Vec<(u32, i32)>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemFamily {
    pub doses: Option<DoseFamily>,
    pub variants: Vec<i32>,
}

pub fn condition_of(name: &str) -> Option<(&str, u32)> {
    let (base, digits) = name.rsplit_once(' ')?;
    if base.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((base, digits.parse().ok()?))
}

pub const BONUS_PARAMS: [u32; 14] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 299];

pub type Bonuses = [i64; BONUS_PARAMS.len()];

pub fn bonuses_of(params: &BTreeMap<u32, Value>) -> Bonuses {
    BONUS_PARAMS.map(|key| match params.get(&key) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    })
}

fn condition_key<'a>(name: &'a str, bonuses: &Bonuses) -> Option<(&'a str, Bonuses)> {
    if bonuses.iter().all(|&b| b == 0) {
        return None;
    }
    let base = condition_of(name).map_or(name, |(base, _)| base);
    Some((base, *bonuses))
}

pub fn item_families<'a>(
    wanted: &[(i32, Option<&str>, Bonuses)],
    items: impl IntoIterator<Item = (i32, &'a str, Bonuses)>,
) -> Vec<ItemFamily> {
    let mut doses: BTreeMap<&str, BTreeMap<u32, i32>> = wanted
        .iter()
        .filter_map(|(_, name, _)| dose_of((*name)?))
        .map(|(base, _)| (base, BTreeMap::new()))
        .collect();
    let mut named: BTreeMap<&str, Vec<i32>> = wanted
        .iter()
        .filter_map(|(_, name, _)| name.filter(|name| !name.is_empty()))
        .map(|name| (name, Vec::new()))
        .collect();
    let mut conditions: BTreeMap<(&str, Bonuses), Vec<i32>> = wanted
        .iter()
        .filter_map(|(_, name, bonuses)| condition_key((*name)?, bonuses))
        .map(|key| (key, Vec::new()))
        .collect();
    for (id, name, bonuses) in items {
        if let Some(ids) = named.get_mut(name) {
            ids.push(id);
        }
        if let Some((base, dose)) = dose_of(name)
            && let Some(family) = doses.get_mut(base)
        {
            let entry = family.entry(dose).or_insert(id);
            *entry = (*entry).min(id);
        }
        if let Some(key) = condition_key(name, &bonuses)
            && let Some(ids) = conditions.get_mut(&key)
        {
            ids.push(id);
        }
    }
    wanted
        .iter()
        .map(|&(id, name, bonuses)| ItemFamily {
            doses: name
                .and_then(dose_of)
                .and_then(|(base, _)| doses.get(base))
                .map(|family| family.iter().map(|(&dose, &id)| (dose, id)).collect()),
            variants: {
                let same_name = name.and_then(|name| named.get(name)).into_iter().flatten();
                let condition = name
                    .and_then(|name| condition_key(name, &bonuses))
                    .and_then(|key| conditions.get(&key))
                    .into_iter()
                    .flatten();
                let mut others: Vec<i32> = same_name
                    .chain(condition)
                    .copied()
                    .filter(|&o| o != id)
                    .collect();
                others.sort_unstable();
                others.dedup();
                others
            },
        })
        .collect()
}

pub fn decode_obj_params(data: &[u8]) -> Option<BTreeMap<u32, Value>> {
    decode_obj(data).map(|def| def.params)
}

pub fn decode_obj(data: &[u8]) -> Option<ObjDef> {
    let mut r = Reader::new(data);
    let mut def = ObjDef::default();
    loop {
        match r.u8()? {
            0 => return Some(def),
            2 => def.name = Some(r.string()?),
            11 => def.stackable = true,
            98 => {
                let noted = r.u16()? != 0;
                def.stackable |= noted;
                def.copy |= noted;
            }
            140 | 149 => def.copy |= r.u16()? != 0,
            1 | 4..=8 | 24 | 26 | 75 | 78 | 79 | 90..=95 | 97 | 99 | 110..=112 | 139 | 148 => {
                r.skip(2)?
            }
            3 | 9 | 30..=39 => {
                r.string()?;
            }
            15 | 16 | 65 | 160 | 251 => {}
            12 | 44 | 46 | 47 | 49..=54 | 100..=109 => r.skip(4)?,
            13 | 14 | 27 | 42 | 113..=115 => r.skip(1)?,
            23 | 25 => r.skip(3)?,
            45 | 48 => r.skip(5)?,
            40 | 41 => {
                let n = r.u8()? as usize;
                r.skip(4 * n)?;
            }
            43 => {
                r.u8()?;
                while r.u8()? != 0 {
                    r.string()?;
                }
            }
            161 => {
                let n = r.u16()? as usize;
                r.skip(2 * n)?;
            }
            200 => {
                r.skip(2)?;
                r.string()?;
            }
            201 => {
                r.skip(1 + 2 + 2 + 4 + 4)?;
                r.string()?;
            }
            202 => {
                r.skip(1 + 2 + 2 + 2 + 4 + 4)?;
                r.string()?;
            }
            249 => {
                for _ in 0..r.u8()? {
                    let is_string = r.u8()? == 1;
                    let key = r.u24()?;
                    let value = if is_string {
                        Value::Str(r.string()?)
                    } else {
                        Value::Int(r.i32()? as i64)
                    };
                    def.params.insert(key, value);
                }
            }
            _ => return None,
        }
    }
}

pub fn decode_varbit(data: &[u8]) -> Option<VarbitDef> {
    let mut r = Reader::new(data);
    let mut def = None;
    loop {
        match r.u8()? {
            0 => return def,
            1 => {
                def = Some(VarbitDef {
                    varp: r.u16()?,
                    lsb: r.u8()?,
                    msb: r.u8()?,
                })
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Bonuses = [0; BONUS_PARAMS.len()];

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn decodes_the_spellbook_varbit() {
        let def = decode_varbit(&hex("0101b7000100")).unwrap();
        assert_eq!(
            def,
            VarbitDef {
                varp: 439,
                lsb: 0,
                msb: 1
            }
        );
        assert_eq!(def.value(0b1110), 2);
    }

    #[test]
    fn varbit_masks_high_bits() {
        let def = VarbitDef {
            varp: 0,
            lsb: 4,
            msb: 31,
        };
        assert_eq!(def.value(-16), 0x0fff_ffff);
    }

    #[test]
    fn malformed_varbits_read_without_overflow() {
        let reversed = VarbitDef {
            varp: 0,
            lsb: 9,
            msb: 2,
        };
        assert_eq!(reversed.value(-1), 0);
        let past_the_word = VarbitDef {
            varp: 0,
            lsb: 40,
            msb: 255,
        };
        assert_eq!(past_the_word.value(-1), 0);
    }

    #[test]
    fn decodes_an_int_enum() {
        let data =
            hex("0169026706000400000000000007be00000001000007bf00000002000007c000000003000007c100");
        let def = decode_enum(&data).unwrap();
        assert_eq!(def.key_type, "i");
        assert_eq!(def.value_type, "g");
        let values: Vec<_> = def
            .entries
            .iter()
            .map(|e| (e.key, e.value.clone()))
            .collect();
        assert_eq!(
            values,
            [
                (0, Value::Int(1982)),
                (1, Value::Int(1983)),
                (2, Value::Int(1984)),
                (3, Value::Int(1985))
            ]
        );
    }

    #[test]
    fn decodes_a_string_enum() {
        let data = hex(concat!(
            "016902730300",
            "050002",
            "000000014100",
            "000000074200",
            "00"
        ));
        let def = decode_enum(&data).unwrap();
        assert_eq!(def.default, Some(Value::Str(String::new())));
        assert_eq!(
            def.entries[0],
            EnumEntry {
                key: 1,
                value: Value::Str("A".into())
            }
        );
        assert_eq!(
            def.entries[1],
            EnumEntry {
                key: 7,
                value: Value::Str("B".into())
            }
        );
    }

    #[test]
    fn decodes_item_params_after_other_opcodes() {
        let data = hex(concat!(
            "025800",
            "0b",
            "0c00000005",
            "f902",
            "01000259",
            "57696e6420426f6c7400",
            "00000254",
            "00da0015",
            "00"
        ));
        let params = decode_obj_params(&data).unwrap();
        assert_eq!(params.get(&601), Some(&Value::Str("Wind Bolt".into())));
        assert_eq!(params.get(&596), Some(&Value::Int(14286869)));
    }

    #[test]
    fn reads_whether_an_item_stacks() {
        assert!(decode_obj(&hex("0b00")).unwrap().stackable);
        assert!(!decode_obj(&hex("02580000")).unwrap().stackable);
        assert!(decode_obj(&hex("62031f00")).unwrap().stackable);
        assert!(!decode_obj(&hex("62000000")).unwrap().stackable);
    }

    #[test]
    fn decodes_a_cache_spell_item() {
        let data = hex(concat!(
            "2c00003f6e0ff9120100025957696e6420426f6c74000100025a41206c6f77206c6576656c20416972206d697373696c6500",
            "00000150000000000000025d000000000000025b000000000000025c000000110000025400da001500000255000000170000",
            "025600000049000002570000000500000258000007180000016d000002320000016e000000010000016f0000022c00000170",
            "000000020000078f00000009000008d300000001000008dc0000000a00",
        ));
        let params = decode_obj_params(&data).unwrap();
        assert_eq!(params.len(), 18);
        assert_eq!(params.get(&601), Some(&Value::Str("Wind Bolt".into())));
        assert_eq!(params.get(&596), Some(&Value::Int(218 << 16 | 21)));
        assert_eq!(params.get(&604), Some(&Value::Int(17)));
        assert_eq!(params.get(&365), Some(&Value::Int(562)));
    }

    #[test]
    fn reads_the_name_and_whether_an_item_is_a_copy() {
        let def = decode_obj(&hex("02580000")).unwrap();
        assert_eq!(def.name.as_deref(), Some("X"));
        assert!(!def.copy);
        assert!(decode_obj(&hex("95384100")).unwrap().copy);
        assert!(decode_obj(&hex("8c000100")).unwrap().copy);
        assert!(!decode_obj(&hex("95000000")).unwrap().copy);
        assert!(decode_obj(&hex("62031f00")).unwrap().copy);
    }

    #[test]
    fn splits_dose_and_charge_names() {
        assert_eq!(dose_of("Prayer potion(4)"), Some(("Prayer potion", 4)));
        assert_eq!(dose_of("Black mask (10)"), Some(("Black mask ", 10)));
        assert_eq!(dose_of("Amulet of glory"), None);
        assert_eq!(dose_of("Rune scimitar (g)"), None);
        assert_eq!(dose_of("Graceful hood(i)"), None);
        assert_eq!(dose_of("(4)"), None);
        assert_eq!(dose_of("Potion()"), None);
    }

    #[test]
    fn groups_dose_families_by_their_base() {
        let items = [
            (2434, "Prayer potion(4)"),
            (139, "Prayer potion(3)"),
            (141, "Prayer potion(2)"),
            (143, "Prayer potion(1)"),
            (20393, "Prayer potion(4)"),
            (1712, "Amulet of glory(4)"),
            (1704, "Amulet of glory"),
            (2440, "Super strength(4)"),
        ];
        let families = item_families(
            &[
                (139, Some("Prayer potion(3)"), NONE),
                (995, Some("Coins"), NONE),
                (-1, None, NONE),
                (1712, Some("Amulet of glory(4)"), NONE),
            ],
            items.map(|(id, name)| (id, name, NONE)),
        );
        assert_eq!(
            families[0].doses.as_deref(),
            Some(&[(1, 143), (2, 141), (3, 139), (4, 2434)][..])
        );
        assert_eq!(families[1].doses, None);
        assert_eq!(families[2], ItemFamily::default());
        assert_eq!(families[3].doses.as_deref(), Some(&[(4, 1712)][..]));
    }

    #[test]
    fn lists_the_other_items_of_the_same_name() {
        let items = [
            (24125, "Crystal halberd"),
            (23987, "Crystal halberd"),
            (23989, "Crystal halberd (inactive)"),
            (2434, "Prayer potion(4)"),
            (20393, "Prayer potion(4)"),
        ];
        let families = item_families(
            &[
                (24125, Some("Crystal halberd"), NONE),
                (2434, Some("Prayer potion(4)"), NONE),
                (4151, Some("Abyssal whip"), NONE),
                (0, Some(""), NONE),
            ],
            items.map(|(id, name)| (id, name, NONE)),
        );
        assert_eq!(families[0].variants, [23987]);
        assert_eq!(families[1].variants, [20393]);
        assert!(families[2].variants.is_empty());
        assert!(families[3].variants.is_empty());
    }

    #[test]
    fn condition_numbers_end_the_name_after_a_space() {
        assert_eq!(
            condition_of("Ahrim's robetop 75"),
            Some(("Ahrim's robetop", 75))
        );
        assert_eq!(condition_of("Ahrim's robetop"), None);
        assert_eq!(condition_of("Prayer potion(4)"), None);
        assert_eq!(condition_of(" 75"), None);
        assert_eq!(condition_of("Robe 7a"), None);
    }

    #[test]
    fn groups_conditions_with_the_same_bonuses() {
        let mut worn = NONE;
        worn[3] = 30;
        worn[8] = 30;
        let mut other = worn;
        other[3] = 31;
        let items = [
            (4712, "Ahrim's robetop", worn),
            (4868, "Ahrim's robetop 100", worn),
            (4869, "Ahrim's robetop 75", worn),
            (4870, "Ahrim's robetop 50", worn),
            (4871, "Ahrim's robetop 25", worn),
            (4872, "Ahrim's robetop 0", NONE),
            (9999, "Ahrim's robetop 10", other),
            (4151, "Abyssal whip", NONE),
        ];
        let families = item_families(
            &[
                (4868, Some("Ahrim's robetop 100"), worn),
                (4712, Some("Ahrim's robetop"), worn),
                (4872, Some("Ahrim's robetop 0"), NONE),
                (4151, Some("Abyssal whip"), NONE),
            ],
            items,
        );
        assert_eq!(families[0].variants, [4712, 4869, 4870, 4871]);
        assert_eq!(families[1].variants, [4868, 4869, 4870, 4871]);
        assert!(families[2].variants.is_empty());
        assert!(families[3].variants.is_empty());
    }

    #[test]
    fn unknown_obj_opcode_fails() {
        assert_eq!(decode_obj_params(&hex("fe00")), None);
    }

    #[test]
    fn cp1252_strings() {
        assert_eq!(Reader::new(&[0x80, b'a', 0xe9, 0]).string().unwrap(), "€aé");
    }
}
