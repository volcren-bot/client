//! Config definitions from the cache's config index: enums and the params of items and varbits, decoded here so it tests without a client.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const ENUM_ARCHIVE: i32 = 8;
pub const OBJ_ARCHIVE: i32 = 10;
pub const VARBIT_ARCHIVE: i32 = 14;

/// An enum value or a param: ints (and the longs of `'l'` enums) or strings.
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
    /// The script var type chars (`'i'` int, `'o'` obj, `'g'` enum, `'s'` string, ...).
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
        let bits = (self.msb - self.lsb + 1) as u32;
        let mask = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
        (((varp as u32) >> self.lsb) & mask) as i32
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
        Some(i32::from_be_bytes([self.u8()?, self.u8()?, self.u8()?, self.u8()?]))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(((self.i32()? as i64) << 32) | self.i32()? as u32 as i64)
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        (self.at + n <= self.data.len()).then(|| self.at += n)
    }

    fn string(&mut self) -> Option<String> {
        let end = self.data.get(self.at..)?.iter().position(|&b| b == 0)?;
        let text = self.data[self.at..self.at + end].iter().map(|&b| cp1252(b)).collect();
        self.at += end + 1;
        Some(text)
    }
}

/// The client's strings are CP1252; it only differs from Latin-1 in 0x80..0xA0.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

pub fn decode_enum(data: &[u8]) -> Option<EnumDef> {
    let mut r = Reader::new(data);
    let mut def = EnumDef { key_type: String::new(), value_type: String::new(), default: None, entries: Vec::new() };
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

/// What an item definition says that the loader and scripts use: its params (opcode 249) and whether it stacks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjDef {
    pub params: BTreeMap<u32, Value>,
    /// Opcode 11, or a noted item (opcode 98 names its note template), which stacks whatever the plain item does.
    pub stackable: bool,
}

/// The params (opcode 249) of an item definition; an unknown opcode fails the decode instead of misreading the rest.
pub fn decode_obj_params(data: &[u8]) -> Option<BTreeMap<u32, Value>> {
    decode_obj(data).map(|def| def.params)
}

pub fn decode_obj(data: &[u8]) -> Option<ObjDef> {
    let mut r = Reader::new(data);
    let mut params = BTreeMap::new();
    let mut stackable = false;
    loop {
        match r.u8()? {
            0 => return Some(ObjDef { params, stackable }),
            11 => stackable = true,
            98 => stackable |= r.u16()? != 0,
            1 | 4..=8 | 24 | 26 | 75 | 78 | 79 | 90..=95 | 97 | 99 | 110..=112 | 139 | 140 | 148 | 149 => r.skip(2)?,
            2 | 3 | 9 | 30..=39 => {
                r.string()?;
            }
            15 | 16 | 65 | 160 | 251 => {}
            12 | 44 | 46 | 47 | 49..=54 => r.skip(4)?,
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
            100..=109 => r.skip(4)?,
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
                    let value = if is_string { Value::Str(r.string()?) } else { Value::Int(r.i32()? as i64) };
                    params.insert(key, value);
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
            1 => def = Some(VarbitDef { varp: r.u16()?, lsb: r.u8()?, msb: r.u8()? }),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn decodes_the_spellbook_varbit() {
        // varbit 4070: varp 439, bits 0..1
        let def = decode_varbit(&hex("0101b7000100")).unwrap();
        assert_eq!(def, VarbitDef { varp: 439, lsb: 0, msb: 1 });
        assert_eq!(def.value(0b1110), 2);
    }

    #[test]
    fn varbit_masks_high_bits() {
        let def = VarbitDef { varp: 0, lsb: 4, msb: 31 };
        assert_eq!(def.value(-16), 0x0fff_ffff);
    }

    #[test]
    fn decodes_an_int_enum() {
        // enum 1981: the spellbooks' enums
        let data = hex("0169026706000400000000000007be00000001000007bf00000002000007c000000003000007c100");
        let def = decode_enum(&data).unwrap();
        assert_eq!(def.key_type, "i");
        assert_eq!(def.value_type, "g");
        let values: Vec<_> = def.entries.iter().map(|e| (e.key, e.value.clone())).collect();
        assert_eq!(values, [(0, Value::Int(1982)), (1, Value::Int(1983)), (2, Value::Int(1984)), (3, Value::Int(1985))]);
    }

    #[test]
    fn decodes_a_string_enum() {
        let data = hex(concat!("016902730300", "050002", "000000014100", "000000074200", "00"));
        let def = decode_enum(&data).unwrap();
        assert_eq!(def.default, Some(Value::Str(String::new())));
        assert_eq!(def.entries[0], EnumEntry { key: 1, value: Value::Str("A".into()) });
        assert_eq!(def.entries[1], EnumEntry { key: 7, value: Value::Str("B".into()) });
    }

    #[test]
    fn decodes_item_params_after_other_opcodes() {
        // name "X", stackable, cost 5, params { 601 = "Wind Bolt", 596 = 14286869 }
        let data = hex(concat!("025800", "0b", "0c00000005", "f902", "01000259", "57696e6420426f6c7400", "00000254", "00da0015", "00"));
        let params = decode_obj_params(&data).unwrap();
        assert_eq!(params.get(&601), Some(&Value::Str("Wind Bolt".into())));
        assert_eq!(params.get(&596), Some(&Value::Int(14286869)));
    }

    #[test]
    fn reads_whether_an_item_stacks() {
        assert!(decode_obj(&hex("0b00")).unwrap().stackable);
        assert!(!decode_obj(&hex("02580000")).unwrap().stackable);
        // a note: its template is item 799
        assert!(decode_obj(&hex("62031f00")).unwrap().stackable);
        assert!(!decode_obj(&hex("62000000")).unwrap().stackable);
    }

    #[test]
    fn decodes_a_cache_spell_item() {
        // obj 3281 (Wind Bolt) as the cache had it on 2026-10-16
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
    fn unknown_obj_opcode_fails() {
        assert_eq!(decode_obj_params(&hex("fe00")), None);
    }

    #[test]
    fn cp1252_strings() {
        assert_eq!(Reader::new(&[0x80, b'a', 0xe9, 0]).string().unwrap(), "€aé");
    }
}
