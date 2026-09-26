//! Sprite, chip and effect archives of the DOS/V builds: how each entry becomes an image.
//!
//! Every rule below was checked by rendering the Korean DOS/V files and looking at the result
//! (recognisable soldiers, horses, terrain and icons with plausible colours). The archives are
//! LS11 containers; their decoded entries hold raw planar pixels without a header, so the
//! geometry comes from the entry size and the archive:
//!
//! | archive | entry size | image | content |
//! |---|---|---|---|
//! | `HEXBCHR.R3` | 2048 B / 1152 B | 4×4 cells (64×64) / 3×3 cells (48×48) | battle-scene unit frames, projectiles, effects |
//! | `HEXICHR.R3` | 4608 B | 6×6 cells (96×96) | mounted officers, 5 sets × 15 frames + 3 archer frames |
//! | `HEXZCHR.R3` | 1024 B | 2×4 cells (32×64) = two 32×32 frames, one above the other | battle-map unit icons |
//! | `HEXZCHP.R3`, `HEXBCHP.R3` | n × 128 B | sheet of 16×16 cells | battle-map chips / battle-scene backdrop |
//! | `MMAPBGPL.R3`, `SMAPBGPL.R3` | n × 128 B | sheet of 16×16 cells | campaign-map / town-map cells |
//! | `HEXGRP.R3` entry 0 | 14592 B | packed planar, 32 px wide (32×456) | battle UI buttons, weather and stratagem icons |
//!
//! Cells are 16×16 pixels, 4 planes × 32 bytes (see [`crate::planar`]), composed in row-major
//! cell order. Only the stored facing exists; the engine mirrors the other one.
//!
//! Not decoded (reported, never guessed): `HEXGRP.R3` entries 1–2 are EUC-KR text (officers'
//! retreat lines), `MARK.R3` and `SSCCHR1/2.R3` use layouts that are not understood yet, and
//! the opening / ending archives mix `NPK016` compressed pictures with headerless packed images
//! whose sizes live in `OPEN.EXE` / `END.EXE`.

use crate::image::IndexedImage;
use crate::planar::{self, CellLayout, PlanarError, CELL_BYTES};
use serde::Serialize;

/// How an entry is turned into an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrangement {
    /// 16×16 planar cells, arranged by the layout.
    Cells(CellLayout),
    /// Packed planar pixels of the given width (the height follows from the size).
    Packed { width: usize },
}

impl Arrangement {
    /// A short description for reports.
    pub fn describe(self) -> String {
        match self {
            Arrangement::Cells(CellLayout::Square(n)) => format!("{n}×{n} cells"),
            Arrangement::Cells(CellLayout::Grid { columns, rows }) => {
                format!("{columns}×{rows} cells")
            }
            Arrangement::Cells(CellLayout::Sheet { columns }) => {
                format!("cell sheet, {columns} per row")
            }
            Arrangement::Packed { width } => format!("packed planar, {width} px wide"),
        }
    }
}

/// A group of consecutive entries that show one thing (identified by looking at them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Group {
    pub first: usize,
    pub last: usize,
    pub what: &'static str,
}

const fn g(first: usize, last: usize, what: &'static str) -> Group {
    Group { first, last, what }
}

/// An archive of sprites or chips.
#[derive(Debug, Clone, Copy)]
pub struct SpriteArchive {
    /// File name in the install.
    pub file: &'static str,
    /// What it holds.
    pub what: &'static str,
    /// Palette slot of the `MAIN.EXE` bank used for the PNGs (visually checked; the game picks
    /// the slot at run time, see [`crate::palette`]).
    pub slot: usize,
    /// Entries that look right only with another slot: `(entry, slot)`.
    pub slot_overrides: &'static [(usize, usize)],
    /// Arrangement for an entry of `len` bytes at `index`; `None` = not an image.
    pub arrangement: fn(index: usize, len: usize) -> Option<Arrangement>,
    /// Entry groups, as far as the images show them.
    pub groups: &'static [Group],
}

fn unit_frames(_: usize, len: usize) -> Option<Arrangement> {
    match len {
        2048 => Some(Arrangement::Cells(CellLayout::Square(4))),
        1152 => Some(Arrangement::Cells(CellLayout::Square(3))),
        _ => None,
    }
}

fn officer_frames(_: usize, len: usize) -> Option<Arrangement> {
    (len == 4608).then_some(Arrangement::Cells(CellLayout::Square(6)))
}

fn map_icons(_: usize, len: usize) -> Option<Arrangement> {
    (len == 1024).then_some(Arrangement::Cells(CellLayout::Grid {
        columns: 2,
        rows: 4,
    }))
}

fn cell_sheet(_: usize, len: usize) -> Option<Arrangement> {
    (len > 0 && len % CELL_BYTES == 0).then_some(Arrangement::Cells(CellLayout::Sheet {
        columns: (len / CELL_BYTES).min(CellLayout::SHEET_COLUMNS),
    }))
}

fn battle_ui(index: usize, len: usize) -> Option<Arrangement> {
    (index == 0 && len > 0 && len % planar::planar_len(32, 1) == 0)
        .then_some(Arrangement::Packed { width: 32 })
}

/// The battle-scene frames of `HEXBCHR.R3`: the 19 unit classes in the game's class order,
/// then effects and projectiles.
pub const HEXBCHR_GROUPS: &[Group] = &[
    g(0, 7, "short-weapon infantry (sword)"),
    g(8, 15, "long-weapon infantry (spear)"),
    g(16, 23, "chariot"),
    g(24, 29, "archers"),
    g(30, 35, "crossbowmen"),
    g(36, 45, "catapult (machine and crew frames)"),
    g(46, 55, "light cavalry"),
    g(56, 65, "heavy cavalry"),
    g(66, 75, "guard cavalry (white horses)"),
    g(76, 83, "bandits"),
    g(84, 91, "brigands"),
    g(92, 99, "outlaws"),
    g(100, 108, "military band (drums and cymbals)"),
    g(109, 114, "beast unit (tiger tamers)"),
    g(115, 122, "martial artists"),
    g(123, 133, "sorcerers"),
    g(134, 141, "barbarians"),
    g(142, 144, "civilians"),
    g(145, 153, "transport (supply carts)"),
    g(
        154,
        159,
        "fire effect: three 128×64 frames, each stored as left and right 64×64 halves",
    ),
    g(
        160,
        165,
        "flood effect: three 128×64 frames, each stored as left and right halves",
    ),
    g(166, 168, "boulders"),
    g(169, 172, "projectiles: arrows, stones (48×48)"),
    g(173, 174, "music notes (48×48)"),
    g(175, 177, "tigers (48×48)"),
    g(178, 180, "carts (48×48)"),
];

/// `HEXICHR.R3`: five sets of 15 frames of mounted officers (12 riding and attacking frames, a
/// falling rider, the fallen rider, the riderless horse), then three mounted-archer frames.
/// Which officer each set belongs to is not known yet.
pub const HEXICHR_GROUPS: &[Group] = &[
    g(0, 14, "mounted officer set 1"),
    g(15, 29, "mounted officer set 2"),
    g(30, 44, "mounted officer set 3"),
    g(45, 59, "mounted officer set 4"),
    g(60, 74, "mounted officer set 5"),
    g(75, 77, "mounted archer"),
];

/// `HEXZCHR.R3`: battle-map icons, two colour variants per class in the class order, then
/// special units and effects.
pub const HEXZCHR_GROUPS: &[Group] = &[
    g(0, 1, "short-weapon infantry"),
    g(2, 3, "long-weapon infantry"),
    g(4, 5, "chariot"),
    g(6, 7, "archers"),
    g(8, 9, "crossbowmen"),
    g(10, 11, "catapult"),
    g(12, 13, "light cavalry"),
    g(14, 15, "heavy cavalry"),
    g(16, 17, "guard cavalry"),
    g(18, 19, "bandits"),
    g(20, 21, "brigands"),
    g(22, 23, "outlaws"),
    g(24, 25, "military band"),
    g(26, 27, "beast unit"),
    g(28, 29, "martial artists"),
    g(30, 31, "sorcerers"),
    g(32, 33, "barbarians"),
    g(34, 35, "civilians"),
    g(36, 37, "transport"),
    g(38, 39, "banner infantry (special unit)"),
    g(40, 40, "chariot with white horses (special unit)"),
    g(41, 41, "fire"),
    g(42, 42, "flood"),
    g(43, 44, "stratagem effect"),
    g(
        45,
        46,
        "banner cavalry on red / yellow horses (special units)",
    ),
];

/// Every sprite and chip archive the extractor converts.
pub const ARCHIVES: [SpriteArchive; 8] = [
    SpriteArchive {
        file: "HEXBCHR.R3",
        what: "battle-scene unit frames and effects",
        slot: 1,
        slot_overrides: &[],
        arrangement: unit_frames,
        groups: HEXBCHR_GROUPS,
    },
    SpriteArchive {
        file: "HEXICHR.R3",
        what: "battle-scene mounted officers",
        slot: 1,
        slot_overrides: &[],
        arrangement: officer_frames,
        groups: HEXICHR_GROUPS,
    },
    SpriteArchive {
        file: "HEXZCHR.R3",
        what: "battle-map unit icons (two 32×32 frames each)",
        slot: 1,
        slot_overrides: &[],
        arrangement: map_icons,
        groups: HEXZCHR_GROUPS,
    },
    SpriteArchive {
        file: "HEXZCHP.R3",
        what: "battle-map chips",
        slot: 1,
        slot_overrides: &[],
        arrangement: cell_sheet,
        groups: &[],
    },
    SpriteArchive {
        file: "HEXBCHP.R3",
        what: "battle-scene backdrop cells (sky, mountains, ground)",
        slot: 1,
        slot_overrides: &[],
        arrangement: cell_sheet,
        groups: &[],
    },
    SpriteArchive {
        file: "MMAPBGPL.R3",
        what: "campaign-map cells",
        slot: 1,
        slot_overrides: &[],
        arrangement: cell_sheet,
        groups: &[],
    },
    SpriteArchive {
        file: "SMAPBGPL.R3",
        what: "town-map cells (entry 0 outdoor, entry 1 palace interiors)",
        slot: 0,
        slot_overrides: &[(1, 2)],
        arrangement: cell_sheet,
        groups: &[],
    },
    SpriteArchive {
        file: "HEXGRP.R3",
        what: "battle UI buttons, weather and stratagem icons (entry 0; entries 1–2 are text)",
        slot: 1,
        slot_overrides: &[],
        arrangement: battle_ui,
        groups: &[],
    },
];

impl SpriteArchive {
    /// The palette slot for entry `index`.
    pub fn slot_for(&self, index: usize) -> usize {
        self.slot_overrides
            .iter()
            .find(|(entry, _)| *entry == index)
            .map_or(self.slot, |&(_, slot)| slot)
    }
}

/// The spec of an archive by file name (case-insensitive).
pub fn archive(file: &str) -> Option<&'static SpriteArchive> {
    ARCHIVES.iter().find(|a| a.file.eq_ignore_ascii_case(file))
}

/// Decode one entry with its arrangement.
pub fn decode_entry(data: &[u8], arrangement: Arrangement) -> Result<IndexedImage, PlanarError> {
    match arrangement {
        Arrangement::Cells(layout) => planar::cells_to_image(data, layout),
        Arrangement::Packed { width } => {
            let row = planar::planar_len(width, 1);
            if row == 0 || data.is_empty() || data.len() % row != 0 {
                return Err(PlanarError::LengthMismatch {
                    len: data.len(),
                    expected: row.max(1) * (data.len() / row.max(1) + 1),
                });
            }
            planar::decode_packed(data, width, data.len() / row)
        }
    }
}

/// Lay out images on one sheet, `columns` per row, each in a cell as large as the largest
/// image, separated by `gap` pixels. Unused space is colour 0 (transparent).
pub fn contact_sheet(images: &[IndexedImage], columns: usize, gap: usize) -> IndexedImage {
    if images.is_empty() {
        return IndexedImage {
            width: 1,
            height: 1,
            pixels: vec![0],
        };
    }
    let columns = columns.max(1);
    let cw = images.iter().map(|i| i.width).max().unwrap_or(0) + gap;
    let ch = images.iter().map(|i| i.height).max().unwrap_or(0) + gap;
    let rows = images.len().div_ceil(columns);
    let (width, height) = (columns * cw, rows * ch);
    let mut pixels = vec![0u8; width * height];
    for (k, img) in images.iter().enumerate() {
        let (x0, y0) = ((k % columns) * cw, (k / columns) * ch);
        for y in 0..img.height {
            let dst = (y0 + y) * width + x0;
            pixels[dst..dst + img.width]
                .copy_from_slice(&img.pixels[y * img.width..(y + 1) * img.width]);
        }
    }
    IndexedImage {
        width,
        height,
        pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    #[test]
    fn arrangements_by_size() {
        let a = archive("hexbchr.r3").unwrap();
        assert_eq!(
            (a.arrangement)(0, 2048),
            Some(Arrangement::Cells(CellLayout::Square(4)))
        );
        assert_eq!(
            (a.arrangement)(170, 1152),
            Some(Arrangement::Cells(CellLayout::Square(3)))
        );
        assert_eq!((a.arrangement)(0, 1000), None);
        let z = archive("HEXZCHR.R3").unwrap();
        let img = decode_entry(&testutil::cells(8), (z.arrangement)(0, 1024).unwrap()).unwrap();
        assert_eq!((img.width, img.height), (32, 64));
        let ui = archive("HEXGRP.R3").unwrap();
        assert_eq!(
            (ui.arrangement)(0, 14592),
            Some(Arrangement::Packed { width: 32 })
        );
        assert_eq!((ui.arrangement)(1, 1576), None);
        let img = decode_entry(&[0x11; 16 * 3], Arrangement::Packed { width: 32 }).unwrap();
        assert_eq!((img.width, img.height), (32, 3));
        assert!(decode_entry(&[0; 15], Arrangement::Packed { width: 32 }).is_err());
        assert!(archive("NOPE.R3").is_none());
        let town = archive("SMAPBGPL.R3").unwrap();
        assert_eq!((town.slot_for(0), town.slot_for(1)), (0, 2));
        assert_eq!(a.slot_for(5), 1);
    }

    #[test]
    fn groups_are_ordered_and_disjoint() {
        for a in ARCHIVES {
            for w in a.groups.windows(2) {
                assert!(
                    w[0].first <= w[0].last && w[0].last < w[1].first,
                    "{}",
                    a.file
                );
            }
        }
        // The documented entry counts of the Korean build.
        assert_eq!(HEXBCHR_GROUPS.last().unwrap().last, 180);
        assert_eq!(HEXICHR_GROUPS.last().unwrap().last, 77);
        assert_eq!(HEXZCHR_GROUPS.last().unwrap().last, 46);
    }

    #[test]
    fn contact_sheet_layout() {
        let a = IndexedImage {
            width: 2,
            height: 1,
            pixels: vec![1, 2],
        };
        let b = IndexedImage {
            width: 1,
            height: 2,
            pixels: vec![3, 4],
        };
        let s = contact_sheet(&[a, b.clone(), b], 2, 1);
        // Cells of 3×3, two per row, two rows.
        assert_eq!((s.width, s.height), (6, 6));
        assert_eq!(&s.pixels[0..2], &[1, 2]);
        assert_eq!(s.pixels[3], 3);
        assert_eq!(s.pixels[6 + 3], 4);
        assert_eq!(s.pixels[3 * 6], 3);
        assert_eq!(contact_sheet(&[], 4, 1).pixels, vec![0]);
    }
}
