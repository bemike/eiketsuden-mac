//! `BAKDATA.R3`: the master tables of townspeople, items and officers.
//!
//! Verified on the Korean DOS/V copy (19,328 bytes, not compressed) and against the loader in
//! `MAIN.EXE`, which reads exactly these four regions:
//!
//! ```text
//! 0x0000  256 × 13 B  townspeople  [name 8][sprite][3 × 00][0x80]
//! 0x0D00   64 × 16 B  items        [name 13][price][power][type]
//! 0x1100  384 × 21 B  officers     [name 6][reading 8][u16 portrait][sprite][lead][war][int][flags]
//! 0x3080  384 × 18 B  initial state[army][role][2 × 00][morale][u16 troops][class][level][exp][8 items]
//! ```
//!
//! The last townsperson and the last item are unused placeholder records. Names are in the
//! edition's text encoding, NUL-padded; the officer "reading" field still holds the Japanese
//! release's half-width katakana reading (JIS X 0201) and is not shown by the Korean game.
//! Officer stats are clamped to 0–100 when loaded. Scenario instructions override several of
//! the initial values (class, level, army) when officers join.

use crate::text::TextEncoding;
use serde::Serialize;
use std::fmt;

/// Size of the file.
pub const FILE_LEN: usize = 0x4b80;
/// Number of townsperson records (the last is a placeholder).
pub const TOWNSFOLK: usize = 256;
/// Number of item records (the last is a placeholder).
pub const ITEMS: usize = 64;
/// Number of officers.
pub const OFFICERS: usize = 384;

const TOWN_AT: usize = 0;
const TOWN_LEN: usize = 13;
const ITEM_AT: usize = 0xd00;
const ITEM_LEN: usize = 16;
const OFFICER_AT: usize = 0x1100;
const OFFICER_LEN: usize = 21;
const STATE_AT: usize = 0x3080;
const STATE_LEN: usize = 18;

/// `BAKDATA.R3` does not have the verified size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakdataError {
    pub len: usize,
}

impl fmt::Display for BakdataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BAKDATA.R3 is {} bytes; the verified layout is {FILE_LEN} bytes",
            self.len
        )
    }
}

impl std::error::Error for BakdataError {}

/// A townsperson (generic town map character).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Townsperson {
    pub index: usize,
    pub name: String,
    /// Town-map sprite.
    pub sprite: u8,
    /// Bytes 9–12 (always `00 00 00 80` except the placeholder).
    pub other: [u8; 4],
}

/// Item types (byte 15 of an item record), named from the items that carry them.
pub fn item_type_name(t: u8) -> &'static str {
    match t {
        0 => "weapon",
        1 => "class-change",
        2 => "attack-scroll",
        3 => "consumable-or-special",
        4 => "horse",
        5 => "book",
        _ => "unknown",
    }
}

/// An item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub index: usize,
    pub name: String,
    /// Shop price in tens of gold; 255 = not sold.
    pub price: u8,
    /// Bonus: percent for weapons and books, movement for horses, 0 otherwise.
    pub power: u8,
    pub item_type: u8,
    pub type_name: &'static str,
}

/// An officer: the fixed record and the initial state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Officer {
    pub index: usize,
    pub name: String,
    /// Japanese reading (half-width katakana decoded to Unicode, up to the first NUL).
    pub reading: String,
    /// `FACEDAT.R3` entry.
    pub portrait: u16,
    /// Town-map sprite.
    pub sprite: u8,
    /// 통솔 / 統率 (leadership).
    pub leadership: u8,
    /// 무력 / 武力 (war).
    pub war: u8,
    /// 지력 / 知力 (intelligence).
    pub intelligence: u8,
    /// Byte 20 of the fixed record (meaning unknown).
    pub flags: u8,
    /// Army: 0 = Liu Bei, 1 = Cao Cao, … 14 = none (stored as `0x80 + army`).
    pub army: u8,
    /// Byte 1 of the state record (0 only for Liu Bei; the game keeps it as value + 2).
    pub role: u8,
    pub morale: u8,
    pub troops: u16,
    pub class: u8,
    pub level: u8,
    pub exp: u8,
    /// Held items (`FF` = empty slot removed).
    pub items: Vec<u8>,
    /// Bytes 2–3 of the state record (always 0 in the verified copy).
    pub other: [u8; 2],
}

/// The decoded file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bakdata {
    pub townsfolk: Vec<Townsperson>,
    pub items: Vec<Item>,
    pub officers: Vec<Officer>,
}

impl Bakdata {
    /// An officer's name, if the index exists.
    pub fn officer_name(&self, index: u16) -> Option<&str> {
        self.officers.get(index as usize).map(|o| o.name.as_str())
    }

    /// An item's name, if the index exists.
    pub fn item_name(&self, index: u16) -> Option<&str> {
        self.items.get(index as usize).map(|i| i.name.as_str())
    }
}

fn name(encoding: TextEncoding, bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    // The Chinese release pads several item names with ASCII spaces inside the fixed field.
    // Padding is storage, not part of the name used to match an item to the engine's rule id.
    encoding.decode(&bytes[..end]).text.trim_end().to_string()
}

/// Half-width katakana (JIS X 0201 0xA1–0xDF) up to the first NUL; other bytes as U+FFFD.
fn reading(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| match b {
            0xa1..=0xdf => char::from_u32(0xff61 + u32::from(b - 0xa1)).unwrap_or('\u{fffd}'),
            0x20..=0x7e => char::from(b),
            _ => '\u{fffd}',
        })
        .collect()
}

/// Decode `BAKDATA.R3`.
pub fn parse(data: &[u8], encoding: TextEncoding) -> Result<Bakdata, BakdataError> {
    if data.len() != FILE_LEN {
        return Err(BakdataError { len: data.len() });
    }
    let townsfolk = (0..TOWNSFOLK)
        .map(|i| {
            let r = &data[TOWN_AT + i * TOWN_LEN..][..TOWN_LEN];
            Townsperson {
                index: i,
                name: name(encoding, &r[..8]),
                sprite: r[8],
                other: [r[9], r[10], r[11], r[12]],
            }
        })
        .collect();
    let items = (0..ITEMS)
        .map(|i| {
            let r = &data[ITEM_AT + i * ITEM_LEN..][..ITEM_LEN];
            Item {
                index: i,
                name: name(encoding, &r[..13]),
                price: r[13],
                power: r[14],
                item_type: r[15],
                type_name: item_type_name(r[15]),
            }
        })
        .collect();
    let officers = (0..OFFICERS)
        .map(|i| {
            let r = &data[OFFICER_AT + i * OFFICER_LEN..][..OFFICER_LEN];
            let s = &data[STATE_AT + i * STATE_LEN..][..STATE_LEN];
            Officer {
                index: i,
                name: name(encoding, &r[..6]),
                reading: reading(&r[6..14]),
                portrait: u16::from_le_bytes([r[14], r[15]]),
                sprite: r[16],
                leadership: r[17],
                war: r[18],
                intelligence: r[19],
                flags: r[20],
                army: s[0].wrapping_sub(0x80),
                role: s[1],
                morale: s[4],
                troops: u16::from_le_bytes([s[5], s[6]]),
                class: s[7],
                level: s[8],
                exp: s[9],
                items: s[10..18].iter().copied().filter(|&b| b != 0xff).collect(),
                other: [s[2], s[3]],
            }
        })
        .collect();
    Ok(Bakdata {
        townsfolk,
        items,
        officers,
    })
}

/// Fixture builder: a file of the verified layout with the given officer names, stats and
/// item names (everything else zero / empty).
pub fn build(
    encoding: TextEncoding,
    officers: &[(&str, [u8; 3], u8, u8)],
    items: &[(&str, u8, u8, u8)],
) -> Vec<u8> {
    let mut out = vec![0u8; FILE_LEN];
    let put = |out: &mut Vec<u8>, at: usize, len: usize, s: &str| {
        let b = encoding.encode(s).expect("fixture text is encodable");
        assert!(b.len() <= len, "fixture name too long: {s}");
        out[at..at + b.len()].copy_from_slice(&b);
    };
    for i in 0..OFFICERS {
        // Army 0 (0x80): the fixture keeps high bytes out of the text statistics.
        let s = STATE_AT + i * STATE_LEN;
        out[s] = 0x80;
        out[s + 10..s + 18].fill(0xff);
    }
    for (i, (n, [lead, war, int], class, level)) in officers.iter().enumerate() {
        let r = OFFICER_AT + i * OFFICER_LEN;
        put(&mut out, r, 6, n);
        out[r + 14] = i as u8;
        out[r + 17] = *lead;
        out[r + 18] = *war;
        out[r + 19] = *int;
        let s = STATE_AT + i * STATE_LEN;
        out[s + 4] = 100;
        out[s + 7] = *class;
        out[s + 8] = *level;
    }
    for (i, (n, price, power, t)) in items.iter().enumerate() {
        let r = ITEM_AT + i * ITEM_LEN;
        put(&mut out, r, 13, n);
        out[r + 13] = *price;
        out[r + 14] = *power;
        out[r + 15] = *t;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_fixed_width_item_names_ignore_space_padding() {
        let bytes = build(TextEncoding::Big5, &[], &[("孫子兵法    ", 255, 22, 5)]);
        let parsed = parse(&bytes, TextEncoding::Big5).unwrap();
        assert_eq!(parsed.item_name(0), Some("孫子兵法"));
        assert_eq!(parsed.items[0].power, 22);
    }

    #[test]
    fn layout_adds_up_to_the_file() {
        assert_eq!(TOWNSFOLK * TOWN_LEN, ITEM_AT);
        assert_eq!(ITEM_AT + ITEMS * ITEM_LEN, OFFICER_AT);
        assert_eq!(OFFICER_AT + OFFICERS * OFFICER_LEN, STATE_AT);
        assert_eq!(STATE_AT + OFFICERS * STATE_LEN, FILE_LEN);
    }

    #[test]
    fn round_trip() {
        let e = TextEncoding::EucKr;
        let data = build(
            e,
            &[("유비", [91, 75, 64], 0, 1), ("관우", [100, 98, 80], 6, 5)],
            &[("청룡언월도", 255, 12, 0), ("콩", 10, 0, 3)],
        );
        let b = parse(&data, e).unwrap();
        assert_eq!(b.officers.len(), OFFICERS);
        assert_eq!(b.items.len(), ITEMS);
        assert_eq!(b.townsfolk.len(), TOWNSFOLK);
        let guan = &b.officers[1];
        assert_eq!(guan.name, "관우");
        assert_eq!(
            (guan.leadership, guan.war, guan.intelligence),
            (100, 98, 80)
        );
        assert_eq!(
            (guan.class, guan.level, guan.army, guan.morale),
            (6, 5, 0, 100)
        );
        assert_eq!(guan.portrait, 1);
        assert!(guan.items.is_empty());
        assert_eq!(b.officers[2].army, 0);
        assert_eq!(b.officer_name(0), Some("유비"));
        assert_eq!(b.officer_name(400), None);
        assert_eq!(b.items[0].name, "청룡언월도");
        assert_eq!(b.items[0].type_name, "weapon");
        assert_eq!(b.item_name(1), Some("콩"));
        assert_eq!(
            parse(&data[1..], e).unwrap_err(),
            BakdataError { len: FILE_LEN - 1 }
        );
    }

    #[test]
    fn readings_are_half_width_katakana() {
        assert_eq!(reading(&[0xd8, 0xad, 0xb3, 0xcb, 0xde, 0, 0xbc, 0]), "ﾘｭｳﾋﾞ");
        assert_eq!(reading(&[0x41, 0x81, 0]), "A\u{fffd}");
    }
}
