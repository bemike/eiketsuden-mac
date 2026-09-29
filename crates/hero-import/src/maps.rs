//! Maps of the DOS/V build and the `MAIN.EXE` tables that tie them to their chips.
//!
//! Every map is a grid of 16×16 planar cells (see [`crate::planar`]) indexing a *chip bank*:
//!
//! | archive | entry | layout | chip bank |
//! |---|---|---|---|
//! | `HEXZMAP.R3` | 0–57 | `[u8 W][u8 H][W×H chip bytes][(W/2)×(H/2) terrain bytes]` | `HEXZCHP` entry 0 (80 cells) followed by entry 1 or 2 |
//! | `HEXZMAP.R3` | 58 | LF-separated EUC-KR map names | — |
//! | `HEXBMAP.R3` | 0–4 / 5–8 | battle-scene backdrop 46×5 / ground 66×8, no header | `HEXBCHP` entry 0 (224 cells) |
//! | `MMAP.R3` | 0–3 | `[W×H tiles][(W/2)×(H/2) route bits]`, size from `MAIN.EXE` | `MMAPBGPL` entry 0 (255 cells) |
//! | `SMAP.R3` / `PMAP.R3` | 12 / 23 | `[32×20 tiles][31×20 walk grid][u8 n][n × (id, x, y)]` | `SMAPBGPL` entry 0 / 1 |
//!
//! The battle map's chip byte is a plain index into the 80 + 174/175-cell bank; values ≥ 80 are
//! ordinary chips of the second set, not overlays. Which second set a map uses, the battle-scene
//! strips per terrain, the terrain names and the campaign-map sizes are tables inside
//! `MAIN.EXE`, located here through the code that reads them ([`find_exe_tables`]), so no table
//! of the original is copied into this crate.

use crate::image::IndexedImage;
use crate::planar::{self, CELL_BYTES, CELL_PX};
use std::fmt;

/// Terrain codes of the battle maps (the game's name table has 20 entries; 18 and 19 are only
/// set at run time by fire and flood tactics).
pub const TERRAIN_COUNT: usize = 20;

/// Neutral identifiers of the terrain codes, in code order. The game's own names are read from
/// `MAIN.EXE` ([`ExeTables::terrain_names`]); these are our translations, checked against the
/// graphics of the cells that carry each code.
pub const TERRAIN_IDS: [&str; TERRAIN_COUNT] = [
    "plain",
    "forest",
    "hill",
    "stream",
    "bridge",
    "wall",
    "castle",
    "grassland",
    "village",
    "cliff",
    "gate",
    "wasteland",
    "fence",
    "fortress",
    "barracks",
    "granary",
    "treasury",
    "house",
    "fire",
    "flood",
];

/// Cells in `HEXZCHP.R3` entry 0, the part of the battle chip bank every map shares.
pub const COMMON_CHIPS: usize = 80;

/// Battle-scene backdrop strips (`HEXBMAP` entries of 230 bytes): 46 × 5 cells.
pub const BACKDROP_CELLS: (usize, usize) = (46, 5);
/// Battle-scene ground strips (`HEXBMAP` entries of 528 bytes): 66 × 8 cells.
pub const GROUND_CELLS: (usize, usize) = (66, 8);

/// Town (`SMAP`) and palace (`PMAP`) screens: 32 × 20 tiles.
pub const TOWN_TILES: (usize, usize) = (32, 20);
/// Their walk grid: 31 × 20 points; point `(x, y)` sits at pixel `(16x + 16, 16y + 8)`, on the
/// seam between tiles `x` and `x + 1`.
pub const TOWN_WALK: (usize, usize) = (31, 20);
/// Walk-grid value of a point the characters cannot enter.
pub const WALK_BLOCKED: u8 = 0xff;
/// Walk-grid value of an ordinary walkable point (other values are walkable marked points:
/// doors, exits and the like).
pub const WALK_OPEN: u8 = 0x7f;

/// Chapters with a campaign map (prologue and chapters 1–4, as `SNR0`–`SNR4`).
pub const CHAPTERS: usize = 5;

/// A map entry that does not have the documented shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// The entry length does not match the layout.
    Length {
        what: &'static str,
        len: usize,
        expected: String,
    },
    /// A dimension is zero or odd (maps are made of 2×2-chip cells).
    Dimensions { width: usize, height: usize },
    /// A tile refers to a cell the bank does not have.
    ChipOutOfRange { chip: u8, cells: usize },
    /// A bank is not a whole number of cells.
    NotCells { len: usize },
    /// A cell failed to decode.
    Planar(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Length {
                what,
                len,
                expected,
            } => write!(f, "{what}: {len} bytes, expected {expected}"),
            MapError::Dimensions { width, height } => {
                write!(
                    f,
                    "map of {width}×{height} chips (must be even and non-zero)"
                )
            }
            MapError::ChipOutOfRange { chip, cells } => {
                write!(f, "tile {chip} outside a bank of {cells} cells")
            }
            MapError::NotCells { len } => {
                write!(f, "chip bank of {len} bytes is not a whole number of cells")
            }
            MapError::Planar(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for MapError {}

// ----- battle maps ---------------------------------------------------------------------------

/// A battle map of `HEXZMAP.R3`: chips (16 px) and terrain codes per 2×2-chip cell (32 px, the
/// grid units move on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleMap {
    /// Width in chips.
    pub width: usize,
    /// Height in chips.
    pub height: usize,
    /// `width × height` chip indices into the bank, row-major.
    pub chips: Vec<u8>,
    /// `(width/2) × (height/2)` terrain codes (see [`TERRAIN_IDS`]), row-major.
    pub terrain: Vec<u8>,
}

impl BattleMap {
    /// Whether `entry` has the battle-map layout (the last entry of `HEXZMAP.R3`, the names,
    /// does not).
    pub fn matches(entry: &[u8]) -> bool {
        BattleMap::parse(entry).is_ok()
    }

    pub fn parse(entry: &[u8]) -> Result<BattleMap, MapError> {
        let [w, h, ..] = *entry else {
            return Err(MapError::Length {
                what: "battle map",
                len: entry.len(),
                expected: "a 2-byte size header".into(),
            });
        };
        let (width, height) = (usize::from(w), usize::from(h));
        let chips = width * height;
        let expected = 2 + chips + chips / 4;
        if entry.len() != expected {
            return Err(MapError::Length {
                what: "battle map",
                len: entry.len(),
                expected: format!("{expected} (2 + {width}×{height} × 5/4)"),
            });
        }
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(MapError::Dimensions { width, height });
        }
        Ok(BattleMap {
            width,
            height,
            chips: entry[2..2 + chips].to_vec(),
            terrain: entry[2 + chips..].to_vec(),
        })
    }

    /// Width and height in cells (2×2 chips).
    pub fn cells(&self) -> (usize, usize) {
        (self.width / 2, self.height / 2)
    }

    /// Serialise (for fixtures).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.width as u8, self.height as u8];
        out.extend_from_slice(&self.chips);
        out.extend_from_slice(&self.terrain);
        out
    }
}

/// The map-name entry of `HEXZMAP.R3`: one name per map, separated by LF (the lines end in
/// CR LF, except that the first two names are separated by a lone LF), ended by an empty line
/// and `0x1A`.
/// Returns the raw bytes of each line without the line end.
pub fn parse_map_names(entry: &[u8]) -> Vec<Vec<u8>> {
    let end = entry.iter().position(|&b| b == 0x1a).unwrap_or(entry.len());
    let body = &entry[..end];
    let mut lines: Vec<Vec<u8>> = body
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
        .collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// The part of a name the game shows: the leading double-byte characters (it copies byte pairs
/// while the lead byte is ≥ 0xA0, so trailing digits and spaces such as `1`/`2` of maps that
/// share a place are not shown).
pub fn display_name(raw: &[u8]) -> &[u8] {
    let mut n = 0;
    while n + 1 < raw.len() && raw[n] >= 0xa0 {
        n += 2;
    }
    &raw[..n]
}

/// The battle chip bank of a map: `HEXZCHP` entry 0 followed by entry `second` (1 or 2).
pub fn battle_bank(chipsets: &[Vec<u8>], second: usize) -> Result<Vec<u8>, MapError> {
    let (Some(common), Some(rest)) = (chipsets.first(), chipsets.get(second)) else {
        return Err(MapError::Length {
            what: "HEXZCHP.R3",
            len: chipsets.len(),
            expected: format!("entries 0 and {second}"),
        });
    };
    if common.len() != COMMON_CHIPS * CELL_BYTES {
        return Err(MapError::Length {
            what: "HEXZCHP.R3 entry 0",
            len: common.len(),
            expected: format!("{} ({COMMON_CHIPS} cells)", COMMON_CHIPS * CELL_BYTES),
        });
    }
    if rest.len() % CELL_BYTES != 0 {
        return Err(MapError::NotCells { len: rest.len() });
    }
    Ok([common.as_slice(), rest.as_slice()].concat())
}

/// Draw a grid of `width × height` tiles from a bank of cells.
pub fn render_tiles(
    tiles: &[u8],
    width: usize,
    height: usize,
    bank: &[u8],
) -> Result<IndexedImage, MapError> {
    if bank.len() % CELL_BYTES != 0 {
        return Err(MapError::NotCells { len: bank.len() });
    }
    if tiles.len() != width * height {
        return Err(MapError::Length {
            what: "tile grid",
            len: tiles.len(),
            expected: format!("{width}×{height}"),
        });
    }
    let count = bank.len() / CELL_BYTES;
    let cells = bank
        .chunks_exact(CELL_BYTES)
        .map(|c| planar::decode(c, CELL_PX, CELL_PX).map_err(|e| MapError::Planar(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let px_width = width * CELL_PX;
    let mut image = IndexedImage {
        width: px_width,
        height: height * CELL_PX,
        pixels: vec![0; px_width * height * CELL_PX],
    };
    for (i, &t) in tiles.iter().enumerate() {
        let cell = cells.get(usize::from(t)).ok_or(MapError::ChipOutOfRange {
            chip: t,
            cells: count,
        })?;
        let (x0, y0) = ((i % width) * CELL_PX, (i / width) * CELL_PX);
        for y in 0..CELL_PX {
            let dst = (y0 + y) * px_width + x0;
            image.pixels[dst..dst + CELL_PX]
                .copy_from_slice(&cell.pixels[y * CELL_PX..(y + 1) * CELL_PX]);
        }
    }
    Ok(image)
}

/// How a chip of a battle bank is used by the maps: the terrain code of the cells it appears
/// in, counted over every map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipUse {
    /// `HEXZCHP` entry the chip belongs to (0 = the shared 80 cells).
    pub set: usize,
    /// Index inside that entry.
    pub index: usize,
    /// How many cells of all maps contain the chip, per terrain code (index 20 = codes ≥ 20).
    pub counts: [u32; TERRAIN_COUNT + 1],
}

impl ChipUse {
    /// Total uses.
    pub fn uses(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// The most frequent terrain code and its share of the uses (`None` when unused or only
    /// under unknown codes).
    pub fn dominant(&self) -> Option<(usize, f64)> {
        let (code, &n) = self.counts[..TERRAIN_COUNT]
            .iter()
            .enumerate()
            .max_by_key(|&(code, &n)| (n, std::cmp::Reverse(code)))?;
        (n > 0).then(|| (code, f64::from(n) / f64::from(self.uses())))
    }
}

/// Count, for every chip of the three `HEXZCHP` entries, the terrain codes of the cells it is
/// drawn in. `maps` pairs each map with its second chip set (1 or 2).
pub fn chip_uses(maps: &[(&BattleMap, usize)], set_sizes: [usize; 3]) -> Vec<ChipUse> {
    let mut uses: Vec<ChipUse> = (0..3)
        .flat_map(|set| {
            (0..set_sizes[set]).map(move |index| ChipUse {
                set,
                index,
                counts: [0; TERRAIN_COUNT + 1],
            })
        })
        .collect();
    let offset = |set: usize| set_sizes[..set].iter().sum::<usize>();
    for &(map, second) in maps {
        for (i, &chip) in map.chips.iter().enumerate() {
            let (x, y) = (i % map.width, i / map.width);
            let code = map.terrain[(y / 2) * (map.width / 2) + x / 2];
            let chip = usize::from(chip);
            let (set, index) = if chip < COMMON_CHIPS {
                (0, chip)
            } else {
                (second, chip - COMMON_CHIPS)
            };
            if set < 3 && index < set_sizes[set] {
                let slot = usize::from(code).min(TERRAIN_COUNT);
                uses[offset(set) + index].counts[slot] += 1;
            }
        }
    }
    uses
}

// ----- battle-scene strips -------------------------------------------------------------------

/// Kind of a `HEXBMAP.R3` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneStrip {
    /// Sky and horizon behind the duelling units (46 × 5 cells).
    Backdrop,
    /// The ground they stand on (66 × 8 cells).
    Ground,
}

impl SceneStrip {
    /// The strip kind of an entry, from its length.
    pub fn of(entry: &[u8]) -> Option<SceneStrip> {
        match entry.len() {
            n if n == BACKDROP_CELLS.0 * BACKDROP_CELLS.1 => Some(SceneStrip::Backdrop),
            n if n == GROUND_CELLS.0 * GROUND_CELLS.1 => Some(SceneStrip::Ground),
            _ => None,
        }
    }

    /// Width and height in cells.
    pub fn cells(self) -> (usize, usize) {
        match self {
            SceneStrip::Backdrop => BACKDROP_CELLS,
            SceneStrip::Ground => GROUND_CELLS,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SceneStrip::Backdrop => "backdrop",
            SceneStrip::Ground => "ground",
        }
    }
}

// ----- campaign maps -------------------------------------------------------------------------

/// A campaign map of `MMAP.R3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignMap {
    /// Width in tiles.
    pub width: usize,
    /// Height in tiles.
    pub height: usize,
    /// `width × height` tile indices into `MMAPBGPL` entry 0, row-major.
    pub tiles: Vec<u8>,
    /// `(width/2) × (height/2)` cells, `true` where the route network runs (stored as a cleared
    /// bit, most significant bit first, rows packed without padding).
    pub routes: Vec<bool>,
}

impl CampaignMap {
    /// Parse an entry whose size is one of `sizes` (the table of `MAIN.EXE`); the entry length
    /// must match exactly one distinct size.
    pub fn parse(entry: &[u8], sizes: &[(usize, usize)]) -> Result<CampaignMap, MapError> {
        let mut fits: Vec<(usize, usize)> = sizes
            .iter()
            .copied()
            .filter(|&(w, h)| {
                w % 2 == 0 && h % 2 == 0 && (w * h) % 32 == 0 && w * h + w * h / 32 == entry.len()
            })
            .collect();
        fits.dedup();
        let [(width, height)] = fits[..] else {
            return Err(MapError::Length {
                what: "campaign map",
                len: entry.len(),
                expected: format!(
                    "W×H×33/32 for exactly one size of {}",
                    sizes
                        .iter()
                        .map(|(w, h)| format!("{w}×{h}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        };
        let n = width * height;
        let routes = entry[n..]
            .iter()
            .flat_map(|&byte| (0..8).rev().map(move |bit| byte & (1 << bit) == 0))
            .collect();
        Ok(CampaignMap {
            width,
            height,
            tiles: entry[..n].to_vec(),
            routes,
        })
    }
}

// ----- town and palace maps ------------------------------------------------------------------

/// A town (`SMAP.R3`) or palace (`PMAP.R3`) screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownMap {
    /// 32 × 20 tile indices into `SMAPBGPL` entry 0 (towns) or 1 (palaces).
    pub tiles: Vec<u8>,
    /// 31 × 20 walk-grid values ([`WALK_BLOCKED`], [`WALK_OPEN`] or a marker).
    pub walk: Vec<u8>,
    /// `(id, x, y)` placements on the walk grid (meaning of `id` not established).
    pub objects: Vec<[u8; 3]>,
}

impl TownMap {
    pub fn parse(entry: &[u8]) -> Result<TownMap, MapError> {
        let tiles = TOWN_TILES.0 * TOWN_TILES.1;
        let walk = TOWN_WALK.0 * TOWN_WALK.1;
        let head = tiles + walk;
        let count = entry.get(head).map(|&n| usize::from(n));
        let expected = count.map(|n| head + 1 + 3 * n);
        if expected != Some(entry.len()) {
            return Err(MapError::Length {
                what: "town map",
                len: entry.len(),
                expected: format!("{} + 1 + 3 × objects", head),
            });
        }
        Ok(TownMap {
            tiles: entry[..tiles].to_vec(),
            walk: entry[tiles..head].to_vec(),
            objects: entry[head + 1..]
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect(),
        })
    }
}

// ----- tables in MAIN.EXE --------------------------------------------------------------------

/// The map tables of `MAIN.EXE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeTables {
    /// File offset of the data segment (DGROUP).
    pub data_base: usize,
    /// Battle maps that use `HEXZCHP` entry 2; every other map uses entry 1.
    pub second_set_maps: Vec<u16>,
    /// `HEXBMAP` backdrop entry per terrain code.
    pub backdrop: Vec<u8>,
    /// `HEXBMAP` ground entry per terrain code.
    pub ground: Vec<u8>,
    /// The game's terrain names (raw bytes, EUC-KR in the Korean build), when found.
    pub terrain_names: Option<Vec<Vec<u8>>>,
    /// Campaign-map size (width, height in tiles) per chapter.
    pub campaign_sizes: Vec<(usize, usize)>,
}

impl ExeTables {
    /// `HEXZCHP` entry (1 or 2) holding the rest of battle map `map`'s chip bank.
    pub fn chip_set_for(&self, map: usize) -> usize {
        if self.second_set_maps.iter().any(|&m| usize::from(m) == map) {
            2
        } else {
            1
        }
    }
}

/// Why the tables could not be located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExeTableError {
    /// Not an MZ executable with the C runtime start-up that loads the data segment.
    NoDataSegment,
    /// A code pattern was not found exactly once.
    Code { what: &'static str, found: usize },
    /// A table address lies outside the file or has an implausible content.
    Table { what: &'static str, detail: String },
}

impl fmt::Display for ExeTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExeTableError::NoDataSegment => write!(
                f,
                "no MZ header with the C start-up code that sets the data segment"
            ),
            ExeTableError::Code { what, found } => write!(
                f,
                "the code that reads the {what} was found {found} times (expected once)"
            ),
            ExeTableError::Table { what, detail } => write!(f, "{what}: {detail}"),
        }
    }
}

impl std::error::Error for ExeTableError {}

/// A code pattern; `None` matches any byte.
type Pattern = &'static [Option<u8>];

macro_rules! pat {
    (@ _) => { None };
    (@ $b:literal) => { Some($b) };
    ($($b:tt)*) => { &[$(pat!(@ $b)),*] };
}

/// C start-up: DOS version check, then `mov di, DGROUP`.
const STARTUP: Pattern =
    pat!(0xb4 0x30 0xcd 0x21 0x3c 0x02 0x73 0x05 0x33 0xc0 0x06 0x50 0xcb 0xbf _ _);
/// `cmp [bx+list], ax / jz / inc byte [bp-1] / cmp byte [bp-1], count`: the membership test of
/// the second-chip-set map list.
const SECOND_SET_LOOP: Pattern = pat!(0x39 0x87 _ _ 0x74 0x0b 0xfe 0x46 0xff 0x80 0x7e 0xff _);
/// `mov bl, [si+0x0c] / sub bh, bh / mov al, [bx+table]`: terrain → scene strip lookups (the
/// backdrop table first, then the ground table).
const SCENE_LOOKUP: Pattern = pat!(0x8a 0x5c 0x0c 0x2a 0xff 0x8a 0x87 _ _);
/// `mov bl, [bp-4] / sub bh, bh / mov al, [bx+si+table]`: campaign-map size by chapter × 2 +
/// axis.
const CAMPAIGN_SIZE: Pattern = pat!(0x8a 0x5e 0xfc 0x2a 0xff 0x8a 0x80 _ _ 0x88 0x46 0xff);

fn find_all(hay: &[u8], pattern: Pattern) -> Vec<usize> {
    if hay.len() < pattern.len() {
        return Vec::new();
    }
    (0..=hay.len() - pattern.len())
        .filter(|&i| {
            pattern
                .iter()
                .zip(&hay[i..])
                .all(|(p, &b)| p.is_none_or(|p| p == b))
        })
        .collect()
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

/// File offset of the data segment: the MZ header size plus DGROUP, which the C start-up at
/// the entry point loads with `mov di, DGROUP`.
fn data_base(exe: &[u8]) -> Result<usize, ExeTableError> {
    if exe.get(..2) != Some(b"MZ") {
        return Err(ExeTableError::NoDataSegment);
    }
    let field = |at| u16_at(exe, at).map(usize::from);
    let (Some(header), Some(ip), Some(cs)) = (field(8), field(0x14), field(0x16)) else {
        return Err(ExeTableError::NoDataSegment);
    };
    let entry = header * 16 + cs * 16 + ip;
    let code = exe
        .get(entry..entry + STARTUP.len())
        .ok_or(ExeTableError::NoDataSegment)?;
    if !find_all(code, STARTUP).contains(&0) {
        return Err(ExeTableError::NoDataSegment);
    }
    let dgroup = usize::from(u16_at(code, STARTUP.len() - 2).unwrap_or(0));
    Ok(header * 16 + dgroup * 16)
}

fn unique(exe: &[u8], pattern: Pattern, what: &'static str) -> Result<usize, ExeTableError> {
    match find_all(exe, pattern)[..] {
        [at] => Ok(at),
        ref found => Err(ExeTableError::Code {
            what,
            found: found.len(),
        }),
    }
}

fn table<'a>(
    exe: &'a [u8],
    base: usize,
    address: u16,
    len: usize,
    what: &'static str,
) -> Result<&'a [u8], ExeTableError> {
    let at = base + usize::from(address);
    exe.get(at..at + len).ok_or_else(|| ExeTableError::Table {
        what,
        detail: format!("address {address:#06x} ({at:#x}) outside the file"),
    })
}

/// The terrain names: a table of [`TERRAIN_COUNT`] string addresses stored right after the
/// strings, which follow one another. Searched in the `window` after the map list.
fn terrain_names(exe: &[u8], base: usize, window: std::ops::Range<usize>) -> Option<Vec<Vec<u8>>> {
    let window = window.start..window.end.min(exe.len());
    for at in window.clone() {
        let pointers: Option<Vec<usize>> = (0..TERRAIN_COUNT)
            .map(|k| u16_at(exe, at + 2 * k).map(|p| base + usize::from(p)))
            .collect();
        let Some(pointers) = pointers else { continue };
        if pointers[0] < window.start || pointers[0] >= at {
            continue;
        }
        let mut names = Vec::with_capacity(TERRAIN_COUNT);
        let mut ok = true;
        for (k, &p) in pointers.iter().enumerate() {
            let Some(len) = exe[p.min(at)..at].iter().position(|&b| b == 0) else {
                ok = false;
                break;
            };
            let next = p + len + 1;
            let chained = pointers.get(k + 1).map_or(next <= at, |&q| q == next);
            if p >= at || len == 0 || !chained {
                ok = false;
                break;
            }
            names.push(exe[p..p + len].to_vec());
        }
        if ok {
            return Some(names);
        }
    }
    None
}

/// Locate the map tables of `MAIN.EXE` through the code that reads them.
pub fn find_exe_tables(exe: &[u8]) -> Result<ExeTables, ExeTableError> {
    let base = data_base(exe)?;

    let at = unique(exe, SECOND_SET_LOOP, "second chip-set map list")?;
    let list_address = u16_at(exe, at + 2).unwrap_or(0);
    let count = usize::from(exe[at + SECOND_SET_LOOP.len() - 1]);
    let list = table(
        exe,
        base,
        list_address,
        2 * count,
        "second chip-set map list",
    )?;
    let second_set_maps: Vec<u16> = list
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .collect();
    if count == 0 || second_set_maps.windows(2).any(|w| w[0] >= w[1]) {
        return Err(ExeTableError::Table {
            what: "second chip-set map list",
            detail: format!("{second_set_maps:?} is not an ascending list of map numbers"),
        });
    }

    let lookups = find_all(exe, SCENE_LOOKUP);
    let [backdrop_at, ground_at] = lookups[..] else {
        return Err(ExeTableError::Code {
            what: "terrain → battle-scene tables",
            found: lookups.len(),
        });
    };
    let address = |at: usize| u16_at(exe, at + SCENE_LOOKUP.len() - 2).unwrap_or(0);
    let (backdrop_address, ground_address) = (address(backdrop_at), address(ground_at));
    if usize::from(ground_address.wrapping_sub(backdrop_address)) != TERRAIN_COUNT {
        return Err(ExeTableError::Table {
            what: "terrain → battle-scene tables",
            detail: format!(
                "tables at {backdrop_address:#06x} and {ground_address:#06x} are not \
                 {TERRAIN_COUNT} entries apart"
            ),
        });
    }
    let backdrop = table(exe, base, backdrop_address, TERRAIN_COUNT, "backdrop table")?.to_vec();
    let ground = table(exe, base, ground_address, TERRAIN_COUNT, "ground table")?.to_vec();

    let at = unique(exe, CAMPAIGN_SIZE, "campaign-map sizes")?;
    let sizes = table(
        exe,
        base,
        u16_at(exe, at + 7).unwrap_or(0),
        2 * CHAPTERS,
        "campaign-map sizes",
    )?;
    let campaign_sizes: Vec<(usize, usize)> = sizes
        .chunks_exact(2)
        .map(|p| (usize::from(p[0]), usize::from(p[1])))
        .collect();
    if campaign_sizes.iter().any(|&(w, h)| w == 0 || h == 0) {
        return Err(ExeTableError::Table {
            what: "campaign-map sizes",
            detail: format!("{campaign_sizes:?} has an empty size"),
        });
    }

    let list_end = base + usize::from(list_address) + 2 * count;
    let terrain_names = terrain_names(exe, base, list_end..list_end + 512);
    Ok(ExeTables {
        data_base: base,
        second_set_maps,
        backdrop,
        ground,
        terrain_names,
        campaign_sizes,
    })
}

/// Movement rules of `MAIN.EXE` (FORMATS §10.4): values are read from the player's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRules {
    /// Move type per class, in class order ([`CLASSES`] entries).
    pub class_move: Vec<u8>,
    /// Movement cost per move type and terrain code; 255: cannot enter.
    pub cost: Vec<Vec<u8>>,
    /// Terrain effect per terrain code: the percent of damage it keeps off (255 where no move
    /// type can enter).
    pub effect: Vec<u8>,
}

/// Classes of the original (the class byte's range).
pub const CLASSES: usize = 19;
/// Move types of the original.
pub const MOVE_TYPES: usize = 4;

/// `mov cl, [bx+class_move] / sub ch, ch / imul di, cx, 20 / mov bx, ax /
/// cmp byte [bx+di+cost], 0xff`: whether a unit's move type can enter a terrain.
const MOVE_LOOKUP: Pattern = pat!(
    0x8a 0x8f _ _ 0x2a 0xed 0x6b 0xf9 0x14 0x8b 0xd8 0x80 0xb9 _ _ 0xff
);
/// `mov bl, [bp-1] / sub bh, bh / mov [bp-0x16], bx / cmp byte [bx+effect], 0xff / jz /
/// sub ah, ah / mov al, [bx+effect] / lea bx, [bp-0x10]`: the terrain window's effect line.
const EFFECT_LOOKUP: Pattern = pat!(
    0x8a 0x5e 0xff 0x2a 0xff 0x89 0x5e 0xea 0x80 0xbf _ _ 0xff 0x74 0x10 0x2a 0xe4 0x8a 0x87 _ _
    0x8d 0x5e 0xf0
);

/// Class rules of `MAIN.EXE` (values read from the player's file), in class order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassRules {
    /// Attack coefficient: the attack formula adds twice this value, so it is five times the
    /// class's `atk` (FORMATS §10.4).
    pub attack: Vec<u8>,
    /// Defence coefficient, the same way.
    pub defense: Vec<u8>,
    /// Movement points.
    pub move_points: Vec<u8>,
    /// Attack range: 0 the four neighbours, 1 the eight, 2 the archer's ring, 3 the crossbow's,
    /// 4 the catapult's, 255 none (the civilian).
    pub range: Vec<u8>,
}

/// `call far class_of / mov bl, al / sub bh, bh / mov al, [bx+table] / sub ah, ah /
/// sub dx, dx`: the attack formula reads the attack table here, the defence formula the
/// defence table 20 bytes on.
const CLASS_COEFFICIENT: Pattern =
    pat!(0x9a _ _ _ _ 0x8a 0xd8 0x2a 0xff 0x8a 0x87 _ _ 0x2a 0xe4 0x2b 0xd2);
/// The same lookup of the movement points, stored at `[bp-7]`.
const CLASS_MOVE: Pattern = pat!(0x9a _ _ _ _ 0x8a 0xd8 0x2a 0xff 0x8a 0x87 _ _ 0x88 0x46 0xf9);
/// The same lookup of the attack range, stored at `[bp-2]`.
const CLASS_RANGE: Pattern =
    pat!(0x9a _ _ _ _ 0x8a 0xd8 0x2a 0xff 0x8a 0x87 _ _ 0x88 0x46 0xfe 0x88);

/// Locate the class rules through the code that reads them.
pub fn find_class_rules(exe: &[u8]) -> Result<ClassRules, ExeTableError> {
    /// The defence coefficients follow the attack ones in a 20-byte slot.
    const COEFFICIENT_STRIDE: u16 = 20;
    let base = data_base(exe)?;
    let address = |at: usize| u16_at(exe, at + 11).unwrap_or(0);
    let coefficients = find_all(exe, CLASS_COEFFICIENT);
    let [attack_at, defense_at] = coefficients[..] else {
        return Err(ExeTableError::Code {
            what: "attack and defence coefficient tables",
            found: coefficients.len(),
        });
    };
    let (attack_address, defense_address) = (address(attack_at), address(defense_at));
    if defense_address.wrapping_sub(attack_address) != COEFFICIENT_STRIDE {
        return Err(ExeTableError::Table {
            what: "attack and defence coefficient tables",
            detail: format!(
                "tables at {attack_address:#06x} and {defense_address:#06x} are not 20 bytes apart"
            ),
        });
    }
    let attack = table(exe, base, attack_address, CLASSES, "attack coefficients")?.to_vec();
    let defense = table(exe, base, defense_address, CLASSES, "defence coefficients")?.to_vec();
    let at = unique(exe, CLASS_MOVE, "movement points")?;
    let move_points = table(exe, base, address(at), CLASSES, "movement points")?.to_vec();
    let at = unique(exe, CLASS_RANGE, "attack ranges")?;
    let range = table(exe, base, address(at), CLASSES, "attack ranges")?.to_vec();
    if range.iter().any(|&r| r > 4 && r != 255) {
        return Err(ExeTableError::Table {
            what: "attack ranges",
            detail: format!("{range:?} has a range other than 0-4 and 255"),
        });
    }
    Ok(ClassRules {
        attack,
        defense,
        move_points,
        range,
    })
}

/// Locate the movement rules through the code that reads them.
pub fn find_move_rules(exe: &[u8]) -> Result<MoveRules, ExeTableError> {
    let base = data_base(exe)?;
    let at = unique(exe, MOVE_LOOKUP, "move-type and movement-cost tables")?;
    let class_move = table(
        exe,
        base,
        u16_at(exe, at + 2).unwrap_or(0),
        CLASSES,
        "class move types",
    )?
    .to_vec();
    if class_move.iter().any(|&m| usize::from(m) >= MOVE_TYPES) {
        return Err(ExeTableError::Table {
            what: "class move types",
            detail: format!("{class_move:?} has a move type of {MOVE_TYPES} or more"),
        });
    }
    let costs = table(
        exe,
        base,
        u16_at(exe, at + 13).unwrap_or(0),
        MOVE_TYPES * TERRAIN_COUNT,
        "movement costs",
    )?;
    let cost: Vec<Vec<u8>> = costs
        .chunks_exact(TERRAIN_COUNT)
        .map(<[u8]>::to_vec)
        .collect();
    let at = unique(exe, EFFECT_LOOKUP, "terrain effect table")?;
    let (first, second) = (u16_at(exe, at + 10), u16_at(exe, at + 19));
    if first != second {
        return Err(ExeTableError::Table {
            what: "terrain effect table",
            detail: format!("the code reads {first:?} and {second:?}"),
        });
    }
    let effect = table(
        exe,
        base,
        first.unwrap_or(0),
        TERRAIN_COUNT,
        "terrain effects",
    )?
    .to_vec();
    Ok(MoveRules {
        class_move,
        cost,
        effect,
    })
}

/// What `MAIN.EXE` does to a battle-map cell when a scenario script changes it
/// (`set_map_chip`, opcode `0x26`, FORMATS §13.3): the value is an operation, not a chip.
/// Operations 0 and 1 open and close a gate, 2 lowers a drawbridge (3 would raise it; the data
/// never does). The tables are read from the player's `MAIN.EXE` ([`find_cell_changes`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChanges {
    /// Terrain codes by operation: operation `n` leaves the cell with `terrain[n ^ 1]`, and
    /// every operation but the drawbridge acts only on a cell whose terrain is `terrain[n]`.
    pub terrain: [u8; 4],
    /// Chip offset by chip set (entry 1, entry 2 of `HEXZCHP`) and operation.
    pub offsets: [[i16; 4]; 2],
    /// Drawbridge: each chip equal to `swap_from[i]` becomes `swap_to[i]`, in table order.
    pub swap_from: Vec<u8>,
    pub swap_to: Vec<u8>,
    /// Drawbridge: a cell that shows this chip afterwards becomes a bridge.
    pub bridge_chip: u8,
    /// Chips the offset operations replace outright: `(from, to)`.
    pub fixed: [(u8, u8); 2],
}

/// The drawbridge operation of [`CellChanges`].
pub const DRAWBRIDGE: u8 = 2;

impl CellChanges {
    /// The four chips (row-major) and terrain code of a cell after operation `op` (its high bit,
    /// a redraw flag, is ignored); `None` when the operation leaves this cell alone. `second_set`
    /// is whether the map draws with `HEXZCHP` entry 2.
    pub fn apply(
        &self,
        chips: [u8; 4],
        terrain: u8,
        second_set: bool,
        op: u8,
    ) -> Result<Option<([u8; 4], u8)>, String> {
        let op = op & 0x7f;
        let Some(&after) = self.terrain.get(usize::from(op ^ 1)) else {
            return Err(format!(
                "map-cell operation {op} is not one the game knows (0–3)"
            ));
        };
        if op == DRAWBRIDGE {
            let chips = chips.map(|mut chip| {
                for (&from, &to) in self.swap_from.iter().zip(&self.swap_to) {
                    if chip == from {
                        chip = to;
                    }
                }
                chip
            });
            let terrain = if chips.contains(&self.bridge_chip) {
                after
            } else {
                terrain
            };
            return Ok(Some((chips, terrain)));
        }
        if terrain != self.terrain[usize::from(op)] {
            return Ok(None);
        }
        let offset = self.offsets[usize::from(second_set)][usize::from(op)];
        let chips = chips.map(
            |chip| match self.fixed.iter().find(|(from, _)| *from == chip) {
                Some(&(_, to)) => to,
                None => chip.wrapping_add_signed(offset as i8),
            },
        );
        Ok(Some((chips, after)))
    }
}

/// `mov al, [si+from] / mov bx, [bp-4] / cmp es:[bx], al / jnz / mov al, [si+to] / mov es:[bx],
/// al / inc si / cmp si, n / jc / mov bx, [bp-4] / cmp byte es:[bx], bridge`: the drawbridge's
/// chip swap.
const CELL_SWAP: Pattern = pat!(
    0x8a 0x84 _ _ 0x8b 0x5e 0xfc 0x26 0x38 0x07 0x75 0x07 0x8a 0x84 _ _ 0x26 0x88 0x07 0x46 0x83
    0xfe _ 0x72 0xe7 0x8b 0x5e 0xfc 0x26 0x80 0x3f _
);
/// `cmp [bx+terrain], al / jz`: the terrain an operation needs.
const CELL_TERRAIN: Pattern = pat!(0x38 0x87 _ _ 0x74 0x03);
/// `shl bx, 2 / add bx, [bp-0x12] / add bx, bx / mov di, [bx+offsets]`: the chip offset by chip
/// set and operation.
const CELL_OFFSET: Pattern = pat!(0xc1 0xe3 0x02 0x03 0x5e 0xee 0x03 0xdb 0x8b 0xbf _ _);
/// `cmp byte es:[si], a / jnz / mov byte es:[si], b / jmp / cmp byte es:[si], c / jnz / mov
/// byte es:[si], d / jmp`: the two chips the offset operations replace outright.
const CELL_FIXED: Pattern = pat!(
    0x26 0x80 0x3c _ 0x75 0x06 0x26 0xc6 0x04 _ 0xeb 0x11 0x26 0x80 0x3c _ 0x75 0x06 0x26 0xc6 0x04
    _ 0xeb 0x05
);

/// Locate the tables of the map-cell operations of `MAIN.EXE` through the code that uses them.
pub fn find_cell_changes(exe: &[u8]) -> Result<CellChanges, ExeTableError> {
    let base = data_base(exe)?;
    let what = "map-cell change tables";
    let swap = unique(exe, CELL_SWAP, "map-cell drawbridge swap")?;
    let count = usize::from(exe[swap + 22]);
    let from = u16_at(exe, swap + 2).unwrap_or(0);
    let to = u16_at(exe, swap + 14).unwrap_or(0);
    let swap_from = table(exe, base, from, count, what)?.to_vec();
    let swap_to = table(exe, base, to, count, what)?.to_vec();
    let at = unique(exe, CELL_TERRAIN, "map-cell terrain table")?;
    let terrain: [u8; 4] = table(exe, base, u16_at(exe, at + 2).unwrap_or(0), 4, what)?
        .try_into()
        .expect("four bytes");
    let at = unique(exe, CELL_OFFSET, "map-cell chip offsets")?;
    let words = table(exe, base, u16_at(exe, at + 10).unwrap_or(0), 16, what)?;
    let word = |i: usize| i16::from_le_bytes([words[2 * i], words[2 * i + 1]]);
    let offsets = [
        std::array::from_fn(&word),
        std::array::from_fn(|op| word(4 + op)),
    ];
    let at = unique(exe, CELL_FIXED, "map-cell fixed chips")?;
    let fixed = [(exe[at + 3], exe[at + 9]), (exe[at + 15], exe[at + 21])];
    if count == 0 || terrain.iter().any(|&t| usize::from(t) >= TERRAIN_COUNT) {
        return Err(ExeTableError::Table {
            what,
            detail: format!("{count} swapped chips, terrain codes {terrain:?}"),
        });
    }
    Ok(CellChanges {
        terrain,
        offsets,
        swap_from,
        swap_to,
        bridge_chip: exe[swap + 31],
        fixed,
    })
}

/// The map-cell tables the fixtures of [`build_exe_fixture`] carry (the Korean DOS/V values).
pub fn fixture_cell_changes() -> CellChanges {
    CellChanges {
        terrain: [10, 0, 3, 4],
        offsets: [[8, -8, -9, 9], [17, -17, 0, 0]],
        swap_from: vec![0xd9, 0xd6, 0xd5, 0xd7, 0xd8, 0xda, 0xe7, 0xe8, 0xe9],
        swap_to: vec![0xd2, 0xcd, 0xcc, 0xce, 0xcf, 0xd3, 0xd0, 0xd4, 0xd1],
        bridge_chip: 0xd3,
        fixed: [(0xc5, 0xc7), (0xc6, 0xe6)],
    }
}

/// Contents of a synthetic `MAIN.EXE` for fixtures.
#[derive(Debug, Clone)]
pub struct ExeFixture<'a> {
    pub second_set_maps: &'a [u16],
    pub backdrop: [u8; TERRAIN_COUNT],
    pub ground: [u8; TERRAIN_COUNT],
    pub terrain_names: &'a [&'a [u8]],
    pub campaign_sizes: [(u8, u8); CHAPTERS],
}

/// The movement rules in [`build_exe_fixture`]'s executable: the classes on the original's
/// move types (infantry 0, cavalry 1, the band and the supply column 2, bandits and the like
/// 3), every move type entering the terrain its code allows but the river, cliff and fire and
/// flood (4 is the bridge); horses cannot enter forest.
pub fn fixture_move_rules() -> MoveRules {
    // An arbitrary arrangement (not the game's): only the synthetic pack test's classes
    // (0, 6, 9, 12) matter.
    let class_move = vec![0, 1, 2, 3, 0, 1, 1, 2, 3, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0];
    let mut cost = vec![vec![1u8; TERRAIN_COUNT]; MOVE_TYPES];
    for row in &mut cost {
        for code in [3, 9, 18, 19] {
            row[code] = 255;
        }
    }
    cost[1][1] = 255;
    let mut effect = vec![0u8; TERRAIN_COUNT];
    effect[1] = 20;
    for code in [3, 9, 18, 19] {
        effect[code] = 255;
    }
    MoveRules {
        class_move,
        cost,
        effect,
    }
}

/// The class rules [`build_exe_fixture`] embeds: the base pack's values in the original's class
/// order, but the civilian's coefficients are 15 (atk and def 3) instead of 0.
pub fn fixture_class_rules() -> ClassRules {
    let rows: [(u8, u8, u8, u8); CLASSES] = [
        (8, 8, 4, 0),
        (12, 12, 4, 1),
        (12, 16, 5, 1),
        (6, 8, 4, 2),
        (12, 8, 4, 3),
        (16, 10, 3, 4),
        (12, 6, 6, 0),
        (14, 10, 5, 0),
        (16, 12, 6, 0),
        (10, 8, 4, 0),
        (12, 10, 4, 1),
        (14, 12, 4, 1),
        (4, 4, 4, 0),
        (16, 6, 4, 1),
        (14, 12, 5, 1),
        (4, 4, 4, 0),
        (14, 16, 5, 1),
        (3, 3, 3, 255),
        (4, 4, 3, 1),
    ];
    ClassRules {
        attack: rows.iter().map(|r| r.0 * 5).collect(),
        defense: rows.iter().map(|r| r.1 * 5).collect(),
        move_points: rows.iter().map(|r| r.2).collect(),
        range: rows.iter().map(|r| r.3).collect(),
    }
}

/// Build a small MZ executable whose start-up and table-reading code have the shapes
/// [`find_exe_tables`] looks for (for this crate's tests and the golden fixtures). `len` pads
/// it with `0x90` to at least that many bytes.
pub fn build_exe_fixture(f: &ExeFixture, len: usize) -> Vec<u8> {
    const HEADER: usize = 0x20;
    const DGROUP: usize = 0x10; // data at HEADER + 0x100
    let mut exe = vec![0x90u8; len.max(0x400)];
    exe[..2].copy_from_slice(b"MZ");
    exe[2..HEADER].fill(0);
    exe[8..10].copy_from_slice(&((HEADER / 16) as u16).to_le_bytes());
    // Entry point CS:IP = 0:0 → the start-up code right after the header.
    let mut code: Vec<u8> = STARTUP
        .iter()
        .take(STARTUP.len() - 2)
        .map(|b| b.unwrap_or(0))
        .collect();
    code.extend((DGROUP as u16).to_le_bytes());
    // Data segment layout: list, names, name pointers, scene tables, campaign sizes.
    let mut data = Vec::new();
    let list_address = data.len() as u16;
    data.extend(f.second_set_maps.iter().flat_map(|m| m.to_le_bytes()));
    data.extend(b"B:hexzmap.r3\0");
    let mut pointers = Vec::new();
    for name in f.terrain_names {
        pointers.push(data.len() as u16);
        data.extend_from_slice(name);
        data.push(0);
    }
    if data.len() % 2 == 1 {
        data.push(0);
    }
    data.extend(pointers.iter().flat_map(|p| p.to_le_bytes()));
    let scene_address = data.len() as u16;
    data.extend(f.backdrop);
    data.extend(f.ground);
    let size_address = data.len() as u16;
    data.extend(f.campaign_sizes.iter().flat_map(|&(w, h)| [w, h]));
    let cells = fixture_cell_changes();
    let cell_terrain = data.len() as u16;
    data.extend(cells.terrain);
    let cell_offsets = data.len() as u16;
    data.extend(cells.offsets.iter().flatten().flat_map(|o| o.to_le_bytes()));
    let swap_from = data.len() as u16;
    data.extend(&cells.swap_from);
    let swap_to = data.len() as u16;
    data.extend(&cells.swap_to);
    let rules = fixture_move_rules();
    let class_move = data.len() as u16;
    data.extend(&rules.class_move);
    let move_cost = data.len() as u16;
    data.extend(rules.cost.iter().flatten());
    let effect = data.len() as u16;
    data.extend(&rules.effect);
    let classes = fixture_class_rules();
    let attack = data.len() as u16;
    data.extend(&classes.attack);
    data.push(0);
    data.extend(&classes.defense);
    let class_move_points = data.len() as u16;
    data.extend(&classes.move_points);
    let class_range = data.len() as u16;
    data.extend(&classes.range);

    let mut put = |pattern: Pattern, fill: &[u8]| {
        let mut fill = fill.iter();
        for p in pattern {
            code.push(p.unwrap_or_else(|| *fill.next().unwrap_or(&0)));
        }
        code.extend([0xcb]); // retf, keeps the snippets apart
    };
    let [lo, hi] = list_address.to_le_bytes();
    put(SECOND_SET_LOOP, &[lo, hi, f.second_set_maps.len() as u8]);
    let [lo, hi] = scene_address.to_le_bytes();
    put(SCENE_LOOKUP, &[lo, hi]);
    let [lo, hi] = (scene_address + TERRAIN_COUNT as u16).to_le_bytes();
    put(SCENE_LOOKUP, &[lo, hi]);
    let [lo, hi] = size_address.to_le_bytes();
    put(CAMPAIGN_SIZE, &[lo, hi]);
    let ([a, b], [c, d]) = (swap_from.to_le_bytes(), swap_to.to_le_bytes());
    put(
        CELL_SWAP,
        &[a, b, c, d, cells.swap_from.len() as u8, cells.bridge_chip],
    );
    let [lo, hi] = cell_terrain.to_le_bytes();
    put(CELL_TERRAIN, &[lo, hi]);
    let [lo, hi] = cell_offsets.to_le_bytes();
    put(CELL_OFFSET, &[lo, hi]);
    let [(a, b), (c, d)] = cells.fixed;
    put(CELL_FIXED, &[a, b, c, d]);
    let ([a, b], [c, d]) = (class_move.to_le_bytes(), move_cost.to_le_bytes());
    put(MOVE_LOOKUP, &[a, b, c, d]);
    let [lo, hi] = effect.to_le_bytes();
    put(EFFECT_LOOKUP, &[lo, hi, lo, hi]);
    for table in [attack, attack + 20] {
        let [lo, hi] = table.to_le_bytes();
        put(CLASS_COEFFICIENT, &[0, 0, 0, 0, lo, hi]);
    }
    let [lo, hi] = class_move_points.to_le_bytes();
    put(CLASS_MOVE, &[0, 0, 0, 0, lo, hi]);
    let [lo, hi] = class_range.to_le_bytes();
    put(CLASS_RANGE, &[0, 0, 0, 0, lo, hi]);

    let data_at = HEADER + DGROUP * 16;
    assert!(
        HEADER + code.len() <= data_at,
        "fixture code overlaps its data"
    );
    exe[HEADER..HEADER + code.len()].copy_from_slice(&code);
    if exe.len() < data_at + data.len() {
        exe.resize(data_at + data.len(), 0x90);
    }
    exe[data_at..data_at + data.len()].copy_from_slice(&data);
    exe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil;

    fn fixture_exe() -> Vec<u8> {
        let names: Vec<Vec<u8>> = TERRAIN_IDS.iter().map(|s| s.as_bytes().to_vec()).collect();
        let refs: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
        build_exe_fixture(
            &ExeFixture {
                second_set_maps: &[0, 3, 7],
                backdrop: std::array::from_fn(|i| (i % 5) as u8),
                ground: std::array::from_fn(|i| 5 + (i % 4) as u8),
                terrain_names: &refs,
                campaign_sizes: [(96, 96), (96, 96), (72, 112), (120, 88), (112, 128)],
            },
            0,
        )
    }

    #[test]
    fn movement_rules_are_found_through_their_code() {
        assert_eq!(find_move_rules(&fixture_exe()), Ok(fixture_move_rules()));
        // Without the code that reads them.
        let mut exe = fixture_exe();
        let at = find_all(&exe, MOVE_LOOKUP)[0];
        exe[at] = 0x90;
        assert!(matches!(
            find_move_rules(&exe),
            Err(ExeTableError::Code { found: 0, .. })
        ));
    }

    #[test]
    fn class_rules_are_found_through_their_code() {
        assert_eq!(find_class_rules(&fixture_exe()), Ok(fixture_class_rules()));
        // A third coefficient lookup leaves it unclear which table is which.
        let mut exe = fixture_exe();
        let at = find_all(&exe, CLASS_COEFFICIENT)[0];
        let snippet = exe[at..at + CLASS_COEFFICIENT.len()].to_vec();
        let pad = find_all(&exe, &[Some(0x90); 32])[0];
        exe[pad..pad + snippet.len()].copy_from_slice(&snippet);
        assert!(matches!(
            find_class_rules(&exe),
            Err(ExeTableError::Code { found: 3, .. })
        ));
        // A range code the game does not have.
        let mut exe = fixture_exe();
        let t = fixture_class_rules();
        let at = find_all(&exe, CLASS_RANGE)[0];
        let address = usize::from(u16_at(&exe, at + 11).unwrap());
        let data = data_base(&exe).unwrap();
        assert_eq!(exe[data + address], t.range[0]);
        exe[data + address] = 5;
        assert!(matches!(
            find_class_rules(&exe),
            Err(ExeTableError::Table {
                what: "attack ranges",
                ..
            })
        ));
    }

    #[test]
    fn exe_tables_are_found_through_their_code() {
        let t = find_exe_tables(&fixture_exe()).unwrap();
        assert_eq!(t.second_set_maps, vec![0, 3, 7]);
        assert_eq!((t.chip_set_for(3), t.chip_set_for(4)), (2, 1));
        assert_eq!(t.backdrop[6], 1);
        assert_eq!(t.ground[6], 7);
        assert_eq!(t.campaign_sizes[2], (72, 112));
        let names = t.terrain_names.unwrap();
        assert_eq!(names.len(), TERRAIN_COUNT);
        assert_eq!(names[19], b"flood");
    }

    #[test]
    fn cell_change_tables_are_found_through_their_code() {
        assert_eq!(
            find_cell_changes(&fixture_exe()),
            Ok(fixture_cell_changes())
        );
        let mut exe = fixture_exe();
        let at = find_all(&exe, CELL_OFFSET)[0];
        exe[at] = 0x90;
        assert_eq!(
            find_cell_changes(&exe),
            Err(ExeTableError::Code {
                what: "map-cell chip offsets",
                found: 0
            })
        );
    }

    #[test]
    fn cell_changes_follow_the_game() {
        let t = fixture_cell_changes();
        // Opening a gate (terrain 10): +8 per chip with entry 1, +17 with entry 2, the two fixed
        // chips replaced outright; the cell becomes plain.
        assert_eq!(
            t.apply([100, 101, 0xc5, 0xc6], 10, false, 0),
            Ok(Some(([108, 109, 0xc7, 0xe6], 0)))
        );
        assert_eq!(
            t.apply([100, 101, 102, 103], 10, true, 0x80),
            Ok(Some(([117, 118, 119, 120], 0))),
            "the redraw bit is ignored"
        );
        assert_eq!(
            t.apply([100, 101, 102, 103], 3, false, 0),
            Ok(None),
            "a gate operation on a river"
        );
        // Closing it again.
        assert_eq!(
            t.apply([108, 109, 110, 111], 0, false, 1),
            Ok(Some(([100, 101, 102, 103], 10)))
        );
        // The drawbridge swaps chips; a cell showing the bridge chip becomes a bridge, the others
        // keep their terrain.
        assert_eq!(
            t.apply([0xda, 0xd9, 0x10, 0xe9], 3, false, 2),
            Ok(Some(([0xd3, 0xd2, 0x10, 0xd1], 4)))
        );
        assert_eq!(
            t.apply([0xd6, 0x10, 0x11, 0x12], 0, true, 2),
            Ok(Some(([0xcd, 0x10, 0x11, 0x12], 0)))
        );
        assert!(t.apply([0; 4], 0, false, 4).is_err());
    }

    #[test]
    fn exe_tables_fail_explicitly() {
        assert_eq!(
            find_exe_tables(b"MZ not an executable"),
            Err(ExeTableError::NoDataSegment)
        );
        let mut exe = fixture_exe();
        // Break the membership loop: the list can no longer be located.
        let at = find_all(&exe, SECOND_SET_LOOP)[0];
        exe[at] = 0x90;
        assert_eq!(
            find_exe_tables(&exe),
            Err(ExeTableError::Code {
                what: "second chip-set map list",
                found: 0
            })
        );
    }

    #[test]
    fn battle_map_layout() {
        let map = BattleMap {
            width: 4,
            height: 2,
            chips: (0..8).collect(),
            terrain: vec![1, 9],
        };
        let bytes = map.encode();
        assert_eq!(bytes.len(), 2 + 8 + 2);
        assert_eq!(BattleMap::parse(&bytes), Ok(map.clone()));
        assert_eq!(map.cells(), (2, 1));
        assert!(!BattleMap::matches(&bytes[..11]));
        // Odd sizes cannot form 2×2 cells.
        let mut odd = vec![3u8, 4];
        odd.resize(2 + 12 + 3, 0);
        assert!(matches!(
            BattleMap::parse(&odd),
            Err(MapError::Dimensions { .. })
        ));
    }

    #[test]
    fn names_split_on_line_feeds() {
        let names =
            parse_map_names(b"\xbb\xe7\xbc\xf6\n\xc8\xa3\r\n\xbd\xc5\xb5\xb5 1\r\n\r\n\x1a\r\n");
        assert_eq!(
            names,
            vec![
                b"\xbb\xe7\xbc\xf6".to_vec(),
                b"\xc8\xa3".to_vec(),
                b"\xbd\xc5\xb5\xb5 1".to_vec()
            ]
        );
        assert_eq!(display_name(&names[2]), b"\xbd\xc5\xb5\xb5");
    }

    #[test]
    fn tiles_index_the_concatenated_bank() {
        let common = testutil::cells(COMMON_CHIPS);
        let second = testutil::cells(3);
        let bank = battle_bank(&[common.clone(), vec![], second.clone()], 2).unwrap();
        assert_eq!(bank.len(), (COMMON_CHIPS + 3) * CELL_BYTES);
        let image = render_tiles(&[0, 81], 2, 1, &bank).unwrap();
        let cell = |data: &[u8], i: usize| {
            planar::decode(&data[i * CELL_BYTES..(i + 1) * CELL_BYTES], 16, 16).unwrap()
        };
        let (a, b) = (cell(&common, 0), cell(&second, 1));
        for y in 0..16 {
            assert_eq!(
                &image.pixels[y * 32..y * 32 + 16],
                &a.pixels[y * 16..y * 16 + 16]
            );
            assert_eq!(
                &image.pixels[y * 32 + 16..y * 32 + 32],
                &b.pixels[y * 16..y * 16 + 16]
            );
        }
        assert_eq!(
            render_tiles(&[83], 1, 1, &bank),
            Err(MapError::ChipOutOfRange {
                chip: 83,
                cells: 83
            })
        );
        assert!(battle_bank(&[vec![0; 128]], 1).is_err());
    }

    #[test]
    fn chip_uses_follow_the_cells() {
        // 4×2 chips = 2×1 cells: left cell forest (1), right cell village (8).
        let map = BattleMap {
            width: 4,
            height: 2,
            chips: vec![0, 0, 80, 81, 0, 0, 80, 81],
            terrain: vec![1, 8],
        };
        let uses = chip_uses(&[(&map, 2)], [80, 2, 2]);
        let find = |set, index| {
            uses.iter()
                .find(|u| u.set == set && u.index == index)
                .unwrap()
        };
        assert_eq!(find(0, 0).dominant(), Some((1, 1.0)));
        assert_eq!(find(2, 0).dominant(), Some((8, 1.0)));
        assert_eq!(find(2, 1).uses(), 2);
        assert_eq!(find(1, 0).dominant(), None);
    }

    #[test]
    fn campaign_map_size_and_routes() {
        let (w, h) = (16, 4);
        let mut entry = vec![3u8; w * h];
        entry.extend([0b0111_1111, 0xff]); // first cell is a route
        let map = CampaignMap::parse(&entry, &[(96, 96), (16, 4)]).unwrap();
        assert_eq!((map.width, map.height), (16, 4));
        assert_eq!(map.routes.len(), 16);
        assert!(map.routes[0] && !map.routes[1]);
        assert!(CampaignMap::parse(&entry[1..], &[(16, 4)]).is_err());
        // Rows of the route mask are packed without padding: 24×4 tiles = 12×2 cells = 3 bytes,
        // the second row starts at bit 12.
        let mut entry = vec![0u8; 24 * 4];
        entry.extend([0xff, 0xf7, 0xff]);
        let map = CampaignMap::parse(&entry, &[(24, 4)]).unwrap();
        assert_eq!(map.routes.iter().position(|&r| r), Some(12));
    }

    #[test]
    fn town_map_layout() {
        let mut entry = vec![1u8; 640];
        entry.extend(vec![WALK_BLOCKED; 620]);
        entry.push(2);
        entry.extend([5, 30, 19, 6, 0, 0]);
        let town = TownMap::parse(&entry).unwrap();
        assert_eq!(town.objects, vec![[5, 30, 19], [6, 0, 0]]);
        assert_eq!(town.walk.len(), 620);
        assert!(TownMap::parse(&entry[..entry.len() - 1]).is_err());
    }

    #[test]
    fn scene_strip_kinds() {
        assert_eq!(SceneStrip::of(&[0; 230]), Some(SceneStrip::Backdrop));
        assert_eq!(SceneStrip::of(&[0; 528]), Some(SceneStrip::Ground));
        assert_eq!(SceneStrip::of(&[0; 3]), None);
    }
}
