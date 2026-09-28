//! The **original-mode pack**: a layered data pack (`extends` the base pack) converted from the
//! player's own copy, holding the original art the base pack's keys can already be mapped to.
//!
//! ```text
//! <out>/pack.toml                     id "original", `extends` the base pack, 640×480 canvas
//! <out>/original-pack.json            what was converted, from which files, and which officer
//!                                     got which portrait (also lets a later run replace the files)
//! <out>/gfx/portraits/<officer>.png   FACEDAT portraits of the base pack's officers
//! <out>/gfx/units/<sprite>_<side>.png HEXZCHR battle-map icons of the 19 classes as unit sheets
//! <out>/gfx/units/units.toml          32×32 frames standing on 32-px tiles
//! <out>/gfx/tiles/terrain.png, .toml  a 32-px tileset learned from the original battle maps
//! <out>/maps/original.toml            the original battle maps (`[[map]]`, id `hexz_NN`) ...
//! <out>/gfx/maps/hexz_NN.png          ... and their picture layers
//! <out>/battles/<battle>.toml         the base pack's prologue and chapter 1 battles re-staged
//!                                     as the original battles on those maps (`crate::battles`)
//! <out>/dramas/original_battles.drama the dialogue of their mid-battle events, from the scenario
//! <out>/gfx/maps/hexz_NN_X_Y_OP.png   cells the events change (a gate opens, a bridge comes down)
//! ```
//!
//! Everything the pack does not hold (rules, officers, battles, dramas, music, the other
//! pictures) comes from the base pack through the layered-pack chain, so the original mode grows
//! one converted asset kind at a time (`docs/ORIGINAL_DATA.md` §8, `docs/DECISIONS.md` D8). The
//! original maps are shipped as map files (D9); the base battles that follow an original battle
//! `use` them (D11).
//!
//! # Mapping rules
//!
//! * **Portraits.** A base-pack officer gets the `FACEDAT` entry of the `BAKDATA` officer with the
//!   same name (the Korean name in the Korean release, the hanja in the Chinese one). Two spellings
//!   differ between the release and the base pack ([`NAME_ALIASES`]); one name is used by two
//!   officers and their stored Japanese reading tells them apart ([`READINGS`]). An officer with no
//!   or several candidate portraits keeps the base pack's picture and is listed in the index.
//! * **Unit sheets.** `HEXZCHR` holds per class two 32×64 icons (two 32×32 frames stacked), one per
//!   army colour, in the game's class order ([`CLASS_SPRITES`]). Only the right-facing picture is
//!   stored (the game mirrors it). Engine sheets have 4 columns (down, up, left, right) × 6 rows
//!   (walk 0–3, attack, hurt), see `docs/ASSETS.md`; [`unit_sheet`] fills them with the two frames.
//!   The even entry of each pair is the player's side, allies included ([`PLAYER_ICON`]).
//! * **Terrain tiles.** One engine tile is one 2×2-chip cell of the original maps (32 px, the
//!   grid units move on). For every terrain and every mask of orthogonal neighbours (the engine's
//!   `auto` layers) [`learn_tiles`] takes the 2×2 chip block the original maps show most often; a
//!   mask no map has borrows the closest observed mask. Terrain without neighbour-dependent looks
//!   gets its most frequent block. Base terrain the original lacks reuses a stand-in
//!   ([`TILE_FALLBACK`]).
//! * **Battle maps.** Every map of `HEXZMAP.R3` becomes a map file entry: its chips drawn as they
//!   are (the picture layer, 16-px chips, so a 32-px tile is one 2×2-chip cell) and its terrain
//!   bytes as the rules grid ([`map_rows`]: the code in base 36, [`TERRAIN_MAP`] in the legend).
//!   A cell whose code has no pack terrain gets the terrain its chips are drawn with elsewhere
//!   ([`ChipTerrain::code_of`]) and is listed as a stand-in.
//!
//! The pack is written only when the palette bank of `MAIN.EXE` is found: unlike the overlay, a
//! pack is played, so no grey-ramp stand-in art is written. Unit sheets and map pictures need the
//! tileset (they are sized for 32-px tiles) and are skipped when it cannot be built.

use crate::bakdata::{self, Officer};
use crate::battles;
use crate::edition::{identify, Edition, EditionId};
use crate::extract::{
    output_error, prepare_output, read_source, ExtractError, KindReport, Output, Status,
};
use crate::image::{encode_png, IndexedImage, Palette16};
use crate::install::InstallDir;
use crate::maps::{self, BattleMap, TERRAIN_COUNT};
use crate::palette::{self};
use crate::planar::{self, CELL_BYTES, CELL_PX};
use crate::sprites;
use crate::text::TextEncoding;
use crate::{ls11, table6};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// Index file of a written pack.
pub const PACK_INDEX: &str = "original-pack.json";
/// `format` of [`PACK_INDEX`].
pub const PACK_FORMAT: &str = "eiketsuden-original-pack";
/// 2: `maps` (the converted battle maps) and the map file in `pack.toml`.
/// 3: `battles` (the base battles re-staged as the original battles) and their battle files.
/// 4: the battles' mid-battle events (`events`, [`DRAMA_FILE`], tile pictures of changed cells).
pub const PACK_FORMAT_VERSION: u32 = 4;
/// `id` of the written pack (save games remember it, so they do not mix with the base pack's).
pub const PACK_ID: &str = "original";
/// Virtual canvas of the pack: the original's 640×480 VGA screen.
pub const CANVAS: [u32; 2] = [640, 480];
/// Size of a map tile: one 2×2-chip cell of the original battle maps.
pub const TILE_PX: usize = 2 * CELL_PX;
/// Atlas cells per row of `terrain.png`.
pub const ATLAS_COLUMNS: usize = 16;
/// Palette slot of battle maps and map icons (the one the overlay uses, checked visually).
pub const MAP_PALETTE_SLOT: usize = crate::extract::MAP_PALETTE_SLOT;
/// Palette slot of the portraits.
pub const PORTRAIT_PALETTE_SLOT: usize = crate::extract::PORTRAIT_PALETTE_SLOT;

/// A base-pack officer to find a portrait for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseOfficer {
    pub id: String,
    /// Korean name.
    pub name: String,
    /// Hanja name (empty when the pack gives none).
    pub hanja: String,
    /// Portrait key: the picture is `gfx/portraits/<portrait>.png`.
    pub portrait: String,
}

/// A terrain of the base pack and the tile key it is drawn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseTerrain {
    pub id: String,
    pub tile: String,
}

/// An item of the base pack, matched to the release's items by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseItem {
    pub id: String,
    /// Korean name.
    pub name: String,
    /// Hanja name (empty when the pack gives none).
    pub hanja: String,
}

/// The base items as `(id, name)` in the language of an `edition`'s `BAKDATA` names: the hanja
/// for the Traditional-Chinese release, the Korean name otherwise (items without a name in that
/// language are left out).
pub fn item_names(items: &[BaseItem], edition: EditionId) -> Vec<(String, String)> {
    items
        .iter()
        .map(|i| {
            let name = match edition {
                EditionId::ChineseDos => &i.hanja,
                _ => &i.name,
            };
            (i.id.clone(), name.clone())
        })
        .filter(|(_, name)| !name.is_empty())
        .collect()
}

/// What the pack is built on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackOptions {
    /// Use this edition instead of identifying it (must be a DOS/V edition).
    pub edition: Option<EditionId>,
    /// `extends` of the written `pack.toml`: the base pack's directory relative to the output.
    pub extends: String,
    /// Officers of the pack chain.
    pub officers: Vec<BaseOfficer>,
    /// Terrain of the pack chain, in file order.
    pub terrain: Vec<BaseTerrain>,
    /// Sprite keys of the pack chain's classes.
    pub sprites: Vec<String>,
    /// Classes of the pack chain: `(id, sprite key)`.
    pub classes: Vec<(String, String)>,
    /// Items of the pack chain.
    pub items: Vec<BaseItem>,
    /// Battles of the pack chain; those that follow an original battle are re-staged
    /// ([`crate::battles`]).
    pub battles: Vec<hero_core::battledef::BattleDef>,
    /// Officers of the player's army in the pack chain: the campaign's starting officers and
    /// those that join in its scenes (events of any battle may name them).
    pub player_officers: Vec<String>,
}

impl PackOptions {
    /// Options for a pack on top of `parent` (the loaded pack chain it will extend), whose
    /// directory is `extends` relative to the written pack.
    pub fn for_pack(
        parent: &hero_core::pack::Pack,
        extends: String,
        edition: Option<EditionId>,
    ) -> PackOptions {
        let sprites: BTreeSet<String> = parent.classes.values().map(|c| c.sprite.clone()).collect();
        PackOptions {
            edition,
            extends,
            officers: parent
                .officers
                .values()
                .map(|o| BaseOfficer {
                    id: o.id.to_string(),
                    name: o.name.clone(),
                    hanja: o.hanja.clone(),
                    portrait: o.portrait.clone().unwrap_or_else(|| o.id.to_string()),
                })
                .collect(),
            terrain: parent
                .terrain
                .iter()
                .map(|t| BaseTerrain {
                    id: t.id.to_string(),
                    tile: t.tile_key().to_string(),
                })
                .collect(),
            sprites: sprites.into_iter().collect(),
            classes: parent
                .classes
                .values()
                .map(|c| (c.id.to_string(), c.sprite.clone()))
                .collect(),
            items: parent
                .items
                .values()
                .map(|i| BaseItem {
                    id: i.id.to_string(),
                    name: i.name.clone(),
                    hanja: i.hanja.clone(),
                })
                .collect(),
            battles: parent.battles.values().cloned().collect(),
            player_officers: player_officers(parent),
        }
    }
}

/// The campaign's starting officers and the officers its scenes let join, in id order.
fn player_officers(pack: &hero_core::pack::Pack) -> Vec<String> {
    let mut ids: BTreeSet<String> = pack
        .campaign
        .starting_officers
        .iter()
        .map(|s| s.to_string())
        .collect();
    for scene in pack.scenes.values() {
        for cmd in &scene.cmds {
            if let hero_core::script::Cmd::Join(o) = cmd {
                ids.insert(o.clone());
            }
        }
    }
    ids.into_iter().collect()
}

/// A portrait given to a base-pack officer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortraitMatch {
    pub officer: String,
    /// `BAKDATA` officer record.
    pub bakdata: usize,
    /// `FACEDAT` entry.
    pub portrait: u16,
}

/// A base-pack officer left with the base pack's portrait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unmatched {
    pub officer: String,
    pub reason: String,
}

/// Contents of [`PACK_INDEX`].
#[derive(Debug, Clone, Serialize)]
pub struct PackIndex {
    pub format: String,
    pub format_version: u32,
    pub tool: String,
    pub edition: Edition,
    pub extends: String,
    pub canvas: [u32; 2],
    /// By asset kind: `battles`, `maps`, `portraits`, `tiles`, `units`.
    pub assets: BTreeMap<String, KindReport>,
    pub portraits: Vec<PortraitMatch>,
    /// Battle maps written to [`MAPS_FILE`].
    pub maps: Vec<MapRecord>,
    /// Battles re-staged as the original battles.
    pub battles: Vec<BattleRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmatched_officers: Vec<Unmatched>,
    /// Every file written, relative to the pack folder.
    pub files: Vec<String>,
}

impl PackIndex {
    /// `true` when every asset kind was converted completely.
    pub fn success(&self) -> bool {
        self.assets.values().all(KindReport::ok)
    }
}

/// `MAIN.EXE` and what the pack needs from it.
struct Exe {
    bank: Result<[Palette16; palette::SLOTS], String>,
    tables: Result<maps::ExeTables, String>,
    cells: Result<maps::CellChanges, String>,
}

impl Exe {
    fn read(install: &InstallDir, report: &mut KindReport) -> Result<Exe, ExtractError> {
        let Some(exe) = read_source(install, "MAIN.EXE", report)? else {
            let missing = || "MAIN.EXE missing".to_string();
            return Ok(Exe {
                bank: Err(missing()),
                tables: Err(missing()),
                cells: Err(missing()),
            });
        };
        Ok(Exe {
            bank: palette::find_bank(&exe)
                .map(|b| b.slots)
                .map_err(|e| format!("MAIN.EXE palette bank: {e}")),
            tables: maps::find_exe_tables(&exe).map_err(|e| format!("MAIN.EXE map tables: {e}")),
            cells: maps::find_cell_changes(&exe)
                .map_err(|e| format!("MAIN.EXE map-cell tables: {e}")),
        })
    }
}

/// Convert the install in `source` into an original-mode pack in `out`.
pub fn write_pack(
    source: &Path,
    out: &Path,
    options: &PackOptions,
) -> Result<PackIndex, ExtractError> {
    let install = InstallDir::open(source)?;
    let (edition, encoding) = pack_edition(&install, options)?;
    // Checked before anything is written; the final manifest also lists the map file.
    pack_toml(&options.extends, edition.id, false, &[], false)
        .map_err(|e| output_error(&out.join("pack.toml"), e))?;
    prepare_output(source, out, PACK_INDEX, PACK_FORMAT)?;
    let mut output = Output::dir(out);
    convert(&install, edition, encoding, options, &mut output)
}

/// An original-mode pack converted in memory ([`build_pack`]).
#[derive(Debug, Clone)]
pub struct MemoryPack {
    pub index: PackIndex,
    /// Every file of the pack ([`PACK_INDEX`] included) by its path relative to the pack folder,
    /// `/`-separated.
    pub files: BTreeMap<String, Vec<u8>>,
}

/// Convert the install in `source` into an original-mode pack held in memory: the same files
/// [`write_pack`] writes, for the game to play without writing anything (it converts at every
/// launch, like OpenRCT2 reading the RCT2 install). The install is only read.
pub fn build_pack(source: &Path, options: &PackOptions) -> Result<MemoryPack, ExtractError> {
    let install = InstallDir::open(source)?;
    let (edition, encoding) = pack_edition(&install, options)?;
    pack_toml(&options.extends, edition.id, false, &[], false)
        .map_err(|e| output_error(Path::new("pack.toml"), e))?;
    let mut output = Output::in_memory(PACK_ID);
    let index = convert(&install, edition, encoding, options, &mut output)?;
    let files = output.memory.take().unwrap_or_default();
    Ok(MemoryPack { index, files })
}

/// The edition of the install (or the one `options` forces) and its text encoding; an error
/// when it cannot be converted.
fn pack_edition(
    install: &InstallDir,
    options: &PackOptions,
) -> Result<(Edition, TextEncoding), ExtractError> {
    let identified = identify(install);
    let edition = match options.edition {
        Some(id) if id.is_extractable() => Edition::forced(id, &identified),
        Some(id) => return Err(ExtractError::BadForcedEdition(id)),
        None => identified,
    };
    match edition.id.text_encoding() {
        Some(encoding) => Ok((edition, encoding)),
        None => Err(ExtractError::NotExtractable(Box::new(edition))),
    }
}

/// Convert every asset kind into `output` and write `pack.toml` and [`PACK_INDEX`].
fn convert(
    install: &InstallDir,
    edition: Edition,
    encoding: TextEncoding,
    options: &PackOptions,
    output: &mut Output,
) -> Result<PackIndex, ExtractError> {
    // MAIN.EXE is listed as a source of every kind that uses it.
    let mut exe_report = KindReport::new(Status::Extracted, true, "");
    let exe = Exe::read(install, &mut exe_report)?;
    let with_exe = |mut r: KindReport| {
        r.sources.extend(exe_report.sources.iter().cloned());
        r
    };

    let (portraits, matches, unmatched) = convert_portraits(
        install,
        encoding,
        edition.id,
        &exe,
        options,
        output,
        with_exe(KindReport::new(Status::Extracted, true, "")),
    )?;
    let tiles = convert_tiles(
        install,
        &exe,
        options,
        output,
        with_exe(KindReport::new(Status::Extracted, true, "")),
    )?;
    let tiles_ok = matches!(tiles.status, Status::Extracted | Status::Partial);
    let units = convert_units(
        install,
        &exe,
        options,
        tiles_ok,
        output,
        with_exe(KindReport::new(Status::Extracted, true, "")),
    )?;
    let (maps, map_records, map_store) = convert_maps(
        install,
        encoding,
        &exe,
        options,
        tiles_ok,
        output,
        with_exe(KindReport::new(Status::Extracted, true, "")),
    )?;

    let (battles, battle_records, drama) = convert_battles(
        install,
        encoding,
        edition.id,
        options,
        &exe,
        &map_records,
        map_store.as_ref(),
        output,
        with_exe(KindReport::new(Status::Extracted, false, "")),
    )?;

    let battle_files: Vec<String> = battle_records.iter().map(|b| b.file.clone()).collect();
    let manifest = pack_toml(
        &options.extends,
        edition.id,
        !map_records.is_empty(),
        &battle_files,
        drama,
    )
    .map_err(|e| output_error(&output.root.join("pack.toml"), e))?;
    output.write("pack.toml", manifest.as_bytes())?;
    output.files.sort();
    let mut assets = BTreeMap::new();
    assets.insert("portraits".to_string(), portraits);
    assets.insert("tiles".to_string(), tiles);
    assets.insert("units".to_string(), units);
    assets.insert("maps".to_string(), maps);
    assets.insert("battles".to_string(), battles);
    let index = PackIndex {
        format: PACK_FORMAT.into(),
        format_version: PACK_FORMAT_VERSION,
        tool: crate::tool_version(),
        edition,
        extends: options.extends.clone(),
        canvas: CANVAS,
        assets,
        portraits: matches,
        maps: map_records,
        battles: battle_records,
        unmatched_officers: unmatched,
        files: output.files.clone(),
    };
    output.write_json(PACK_INDEX, &index)?;
    Ok(index)
}

/// A TOML basic string.
fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `pack.toml` of the pack; `extends` must be a relative directory. `maps`: list
/// [`MAPS_FILE`]; `battles`: the battle files; `dramas`: list [`DRAMA_FILE`].
fn pack_toml(
    extends: &str,
    edition: EditionId,
    maps: bool,
    battles: &[String],
    dramas: bool,
) -> Result<String, String> {
    if extends.is_empty()
        || Path::new(extends).is_absolute()
        || extends.starts_with('/')
        || extends.contains('\\')
        || extends.contains(':')
    {
        return Err(format!(
            "`extends` must be a relative directory with `/` separators, got `{extends}`"
        ));
    }
    let maps = if maps {
        format!("maps = [{}]\n", toml_str(MAPS_FILE))
    } else {
        String::new()
    };
    let battles = if battles.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = battles
            .iter()
            .map(|b| format!("  {},\n", toml_str(b)))
            .collect();
        format!("battles = [\n{}]\n", list.concat())
    };
    let dramas = if dramas {
        format!("dramas = [{}]\n", toml_str(DRAMA_FILE))
    } else {
        String::new()
    };
    Ok(format!(
        "# Original mode, written by `hero-tools original pack` ({tool}) from the player's own copy\n\
         # of KOEI's Sangokushi Eiketsuden ({edition}). It holds converted game art: keep it on this\n\
         # computer, do not share or commit it. Run the importer again to rebuild it; hand edits are\n\
         # lost. What is converted and how: original-pack.json and docs/ORIGINAL_DATA.md.\n\
         \n\
         id = {id}\n\
         name = \"영걸전 원작 모드\"\n\
         version = {version}\n\
         license = \"LicenseRef-Private (converted from the player's own copy; not redistributable)\"\n\
         description = \"보유한 원작에서 변환한 얼굴·유닛·지형 그림과 전투 맵, 원작 맵 위로 옮긴 전투를 기본 팩 위에 얹은 팩. 변환되지 않은 것은 기본 팩에서 온다.\"\n\
         extends = {extends}\n\
         {maps}\
         {battles}\
         {dramas}\
         \n\
         [presentation]\n\
         canvas = [{w}, {h}]\n",
        tool = crate::tool_version(),
        edition = edition.as_str(),
        id = toml_str(PACK_ID),
        version = toml_str(env!("CARGO_PKG_VERSION")),
        extends = toml_str(extends),
        w = CANVAS[0],
        h = CANVAS[1],
    ))
}

// ----- battles -------------------------------------------------------------------------------

/// Folder of the re-staged battles.
pub const BATTLES_DIR: &str = "battles";

/// A base battle re-staged as an original battle ([`crate::battles`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BattleRecord {
    /// Battle id (the base pack's, which the file replaces).
    pub id: String,
    /// Battle file, relative to the pack.
    pub file: String,
    /// Where the original battle is: `SNRnD.R3`, scene and block.
    pub source: String,
    /// Map file id the battle `use`s.
    pub map: String,
    pub turn_limit: u32,
    pub units: usize,
    pub treasures: usize,
    /// Events of the battle, and how many of them are the base battle's.
    pub events: usize,
    pub base_events: usize,
    /// What did not carry over.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

type BattlesResult = (KindReport, Vec<BattleRecord>, bool);

/// Drama file of the original battles' mid-battle events.
pub const DRAMA_FILE: &str = "dramas/original_battles.drama";

/// A scene's text section (`SNRnM`) decoded for [`battles::TextSource`].
struct SceneText<'a> {
    section: crate::text::Section<'a>,
    encoding: TextEncoding,
}

impl battles::TextSource for SceneText<'_> {
    fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String> {
        let (lines, _) = self
            .section
            .dialogue_at(usize::from(offset))
            .map_err(|e| e.to_string())?;
        Ok(lines
            .iter()
            .map(|l| (l.speaker, self.encoding.decode(l.text).text))
            .collect())
    }

    fn string(&self, offset: u16) -> Result<String, String> {
        let bytes = self
            .section
            .string_at(usize::from(offset))
            .map_err(|e| e.to_string())?;
        Ok(self.encoding.decode(bytes).text)
    }
}

/// The text of a scene whose message file could not be read: every lookup reports why.
struct NoText(String);

impl battles::TextSource for NoText {
    fn dialogue(&self, _: u16) -> Result<Vec<(u16, String)>, String> {
        Err(format!("dialogue left out: {}", self.0))
    }

    fn string(&self, _: u16) -> Result<String, String> {
        Err(format!("text left out: {}", self.0))
    }
}

/// The key of the tile picture of cell `(x, y)` of map `map_id` after operation `op`.
pub fn cell_picture(map_id: &str, x: usize, y: usize, op: u8) -> String {
    format!("{map_id}_{x}_{y}_{op}")
}

/// Re-stage the base battles that follow an original battle ([`battles::ORIGINAL_BATTLES`]) on
/// the converted maps and write them to [`BATTLES_DIR`], with the dialogue of their mid-battle
/// events in [`DRAMA_FILE`] (the last value: whether it was written).
#[allow(clippy::too_many_arguments)]
fn convert_battles(
    install: &InstallDir,
    encoding: TextEncoding,
    edition: EditionId,
    options: &PackOptions,
    exe: &Exe,
    maps: &[MapRecord],
    store: Option<&MapStore>,
    out: &mut Output,
    mut report: KindReport,
) -> Result<BattlesResult, ExtractError> {
    let wanted: Vec<_> = battles::ORIGINAL_BATTLES
        .iter()
        .filter(|p| options.battles.iter().any(|b| b.id == p.battle))
        .collect();
    if wanted.is_empty() {
        report.status = Status::Unsupported;
        report.summary =
            "the pack chain has none of the base pack's battles that follow the original".into();
        return Ok((report, Vec::new(), false));
    }
    report.status = Status::Failed;
    let Some(bak) = read_source(install, "BAKDATA.R3", &mut report)? else {
        report.status = Status::MissingSource;
        report.summary = "BAKDATA.R3 missing".into();
        return Ok((report, Vec::new(), false));
    };
    let bak = match bakdata::parse(&bak, encoding) {
        Ok(b) => b,
        Err(e) => {
            report.summary = "BAKDATA.R3 invalid".into();
            report.errors.push(e.to_string());
            return Ok((report, Vec::new(), false));
        }
    };
    let mut names = battles::Names::new(
        &bak.officers,
        &bak.items,
        |person| {
            let mut ids = options
                .officers
                .iter()
                .filter(|o| is_same_officer(o, person, edition));
            match (ids.next(), ids.next()) {
                (Some(o), None) => Some(o.id.clone()),
                _ => None,
            }
        },
        &CLASS_SPRITES,
        &options.classes,
        &item_names(&options.items, edition),
    );
    names.player_officers = options.player_officers.iter().cloned().collect();

    // Scenario and message files, read once.
    let mut scenarios: BTreeMap<usize, Option<Vec<u8>>> = BTreeMap::new();
    let mut messages: BTreeMap<usize, Option<Vec<u8>>> = BTreeMap::new();
    let mut records = Vec::new();
    let mut drama = String::from(
        "# Dialogue of the original battles' mid-battle events, converted from the scenario of the\n\
         # player's own copy by `hero-tools original pack` (do not edit; run the importer again).\n\
         # Scene `orig_<battle>_<record>` belongs to trigger record <record> of the battle's block.\n",
    );
    let mut scenes = 0usize;
    let mut pictures: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for pairing in &wanted {
        let (id, file, scene_index, map) =
            (pairing.battle, pairing.file, pairing.scene, pairing.map);
        let name = format!("SNR{file}D.R3");
        let message_name = format!("SNR{file}M.R3");
        if let std::collections::btree_map::Entry::Vacant(slot) = scenarios.entry(file) {
            slot.insert(read_source(install, &name, &mut report)?);
        }
        if let std::collections::btree_map::Entry::Vacant(slot) = messages.entry(file) {
            slot.insert(read_source(install, &message_name, &mut report)?);
        }
        let result = (|| -> Result<(BattleRecord, String), String> {
            let bytes = scenarios[&file]
                .as_deref()
                .ok_or_else(|| format!("{name} missing"))?;
            let archive = ls11::Archive::parse(bytes).map_err(|e| format!("{name}: {e}"))?;
            let data = archive
                .decode(scene_index)
                .map_err(|e| format!("{name} scene {scene_index}: {e}"))?;
            let scene = crate::scenario::parse_scene(&data)
                .map_err(|e| format!("{name} scene {scene_index}: {e}"))?;
            // Without the text the battle is still converted; its dialogue is left out.
            let payload = messages[&file]
                .as_deref()
                .ok_or_else(|| format!("{message_name} missing"))
                .and_then(|bytes| {
                    crate::extract::message_payload(bytes)
                        .map_err(|e| format!("{message_name}: {e}"))
                });
            let section = payload
                .as_deref()
                .map_err(Clone::clone)
                .and_then(|payload| {
                    let sections = crate::text::parse_messages(payload)
                        .map_err(|e| format!("{message_name}: {e}"))?;
                    sections
                        .sections
                        .get(scene_index)
                        .cloned()
                        .ok_or_else(|| format!("{message_name} has no section {scene_index}"))
                });
            let text: Box<dyn battles::TextSource + '_> = match section {
                Ok(section) => Box::new(SceneText { section, encoding }),
                Err(e) => Box::new(NoText(e)),
            };
            let original = battles::find_battle(&scene, map, pairing.flags)
                .map_err(|e| format!("{name} scene {scene_index}: {e}"))?;
            let map_id = map_id(usize::from(map));
            if !maps.iter().any(|m| m.id == map_id) {
                return Err(format!("battle map {map} ({map_id}) was not converted"));
            }
            let mut cell_state: BTreeMap<(usize, usize), ([u8; 4], u8)> = BTreeMap::new();
            let mut cell_change = |pos: hero_core::geom::Pos,
                                   op: u8|
             -> Result<battles::CellChange, String> {
                let store = store.ok_or("the battle maps were not read")?;
                let (cells, tables) = match (&exe.cells, &exe.tables) {
                    (Ok(c), Ok(t)) => (c, t),
                    (Err(e), _) | (_, Err(e)) => return Err(e.clone()),
                };
                let number = usize::from(map);
                let grid = store
                    .maps
                    .get(&number)
                    .ok_or_else(|| format!("battle map {number} was not read"))?;
                let (w, h) = grid.cells();
                let (Ok(x), Ok(y)) = (usize::try_from(pos.x), usize::try_from(pos.y)) else {
                    return Err("outside the map".into());
                };
                if x >= w || y >= h {
                    return Err("outside the map".into());
                }
                // Operations apply one after another, in the order of the block's scripts.
                let chip =
                    |dx: usize, dy: usize| grid.chips[(2 * y + dy) * grid.width + 2 * x + dx];
                let (chips, before) = *cell_state.entry((x, y)).or_insert((
                    [chip(0, 0), chip(1, 0), chip(0, 1), chip(1, 1)],
                    grid.terrain[y * w + x],
                ));
                let set = tables.chip_set_for(number);
                let Some((after, code)) = cells.apply(chips, before, set == 2, op)? else {
                    return Ok(None);
                };
                cell_state.insert((x, y), (after, code));
                let terrain = TERRAIN_MAP
                    .get(usize::from(code))
                    .copied()
                    .flatten()
                    .ok_or_else(|| format!("terrain code {code} has no pack terrain"))?;
                let image = maps::render_tiles(&after, 2, 2, &store.banks[&set])
                    .map_err(|e| e.to_string())?;
                let png = encode_png(&image, &store.palette, false).map_err(|e| e.to_string())?;
                // The same operation on a cell another operation changed first looks
                // different: it gets its own picture.
                let mut key = cell_picture(&map_id, x, y, op & 0x7f);
                let mut n = 1;
                while pictures.get(&key).is_some_and(|p| *p != png) {
                    n += 1;
                    key = format!("{}_{n}", cell_picture(&map_id, x, y, op & 0x7f));
                }
                pictures.insert(key.clone(), png);
                Ok(Some((terrain.to_string(), Some(key))))
            };
            let base = options
                .battles
                .iter()
                .find(|b| b.id == id)
                .expect("filtered above");
            let converted = battles::convert(
                base,
                &original,
                &names,
                pairing,
                &map_id,
                &mut battles::EventSources {
                    text: text.as_ref(),
                    cell_change: &mut cell_change,
                },
            )?;
            let source = format!("{name} scene {scene_index} block {}", original.block);
            let file = format!("{BATTLES_DIR}/{id}.toml");
            let body = toml::to_string(&converted.battle)
                .map_err(|e| format!("{id}: cannot write the battle: {e}"))?;
            let mut text = format!(
                "# {id}: the base pack's battle re-staged as the original battle ({source}) on the\n\
                 # original map {map_id}. Written by `hero-tools original pack` from the player's own\n\
                 # copy: keep it on this computer. Rules: docs/ORIGINAL_DATA.md §4.5.\n"
            );
            for note in &converted.notes {
                let _ = writeln!(text, "# note: {note}");
            }
            text.push('\n');
            text.push_str(&body);
            out.write(&file, text.as_bytes())
                .map_err(|e| e.to_string())?;
            let base_events = base.events.len();
            Ok((
                BattleRecord {
                    id: id.to_string(),
                    file,
                    source,
                    map: map_id,
                    turn_limit: converted.battle.turn_limit,
                    units: converted.battle.units.len(),
                    treasures: converted.battle.treasures.len(),
                    events: converted.battle.events.len(),
                    base_events,
                    notes: converted.notes,
                },
                converted.drama,
            ))
        })();
        match result {
            Ok((r, text)) => {
                if !text.is_empty() {
                    let _ = write!(drama, "\n# ----- {} ({})\n{text}", r.id, r.source);
                    scenes += text.matches("\n== ").count();
                }
                records.push(r);
            }
            Err(e) => report.errors.push(format!("{id}: {e}")),
        }
    }
    for (key, png) in &pictures {
        out.write(&format!("gfx/maps/{key}.png"), png)?;
    }
    let wrote_drama = scenes > 0;
    if wrote_drama {
        out.write(DRAMA_FILE, drama.as_bytes())?;
    }
    report.outputs = records.len() + pictures.len() + usize::from(wrote_drama);
    report.status = if report.errors.is_empty() {
        Status::Extracted
    } else if records.is_empty() {
        Status::Failed
    } else {
        Status::Partial
    };
    let events: usize = records.iter().map(|r| r.events).sum();
    report.summary = format!(
        "{} of {} base battles re-staged as the original battles on the original maps, {events} \
         events ({scenes} drama scenes, {} changed-cell pictures)",
        records.len(),
        wanted.len(),
        pictures.len()
    );
    report.notes.push(
        "the original's mid-battle events come from the scenario's trigger records; where the \
         base battle keeps an event with the same trigger, the base event stays"
            .into(),
    );
    report.notes.push(
        "AI modes 0, 5 and 6 are inferred (docs/reverse-engineering/FORMATS.md §13.4)".into(),
    );
    Ok((report, records, wrote_drama))
}

// ----- portraits -----------------------------------------------------------------------------

/// Base-pack officers whose Korean name the Korean release spells differently:
/// `(officer id, name in BAKDATA)`.
pub const NAME_ALIASES: &[(&str, &str)] = &[
    // 張遼: 장료 in today's spelling, 장요 in the release.
    ("zhang_liao", "장요"),
    // 紀靈: 기령 / 기영.
    ("ji_ling", "기영"),
];

/// Base-pack officers whose name `BAKDATA` gives to two officers, with the Japanese reading
/// (kept from the Japanese release) of the right one: `(officer id, reading)`.
pub const READINGS: &[(&str, &str)] = &[
    // 于禁 (ウキン); the other 우금 is 牛金 (ギュウキン).
    ("yu_jin", "ｳｷﾝ"),
];

/// Outcome of looking up an officer's portrait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaceMatch {
    Found {
        bakdata: usize,
        portrait: u16,
    },
    /// No `BAKDATA` officer has the name.
    Missing,
    /// Several officers with different portraits have the name: `(record, portrait)`.
    Ambiguous(Vec<(usize, u16)>),
}

/// Whether the `BAKDATA` officer `person` of an `edition` is the base-pack `officer`: the same
/// name (the hanja in the Chinese release, [`NAME_ALIASES`] for spellings that differ) and, for
/// the officers in [`READINGS`], the same Japanese reading.
pub fn is_same_officer(officer: &BaseOfficer, person: &Officer, edition: EditionId) -> bool {
    let lookup = |list: &[(&str, &'static str)]| {
        list.iter()
            .find(|(id, _)| *id == officer.id)
            .map(|&(_, v)| v)
    };
    let name = match edition {
        EditionId::ChineseDos => officer.hanja.as_str(),
        _ => lookup(NAME_ALIASES).unwrap_or(&officer.name),
    };
    !name.is_empty() && person.name == name && lookup(READINGS).is_none_or(|r| person.reading == r)
}

/// Find the portrait of `officer` among the `BAKDATA` officers of an `edition`.
pub fn match_officer(officer: &BaseOfficer, table: &[Officer], edition: EditionId) -> FaceMatch {
    let candidates: Vec<&Officer> = table
        .iter()
        .filter(|o| is_same_officer(officer, o, edition))
        .collect();
    let portraits: BTreeSet<u16> = candidates.iter().map(|o| o.portrait).collect();
    match (candidates.first(), portraits.len()) {
        (None, _) => FaceMatch::Missing,
        (Some(o), 1) => FaceMatch::Found {
            bakdata: o.index,
            portrait: o.portrait,
        },
        _ => FaceMatch::Ambiguous(candidates.iter().map(|o| (o.index, o.portrait)).collect()),
    }
}

type PortraitResult = (KindReport, Vec<PortraitMatch>, Vec<Unmatched>);

fn convert_portraits(
    install: &InstallDir,
    encoding: TextEncoding,
    edition: EditionId,
    exe: &Exe,
    options: &PackOptions,
    out: &mut Output,
    mut report: KindReport,
) -> Result<PortraitResult, ExtractError> {
    let fail = |mut report: KindReport,
                summary: String,
                error: String|
     -> Result<PortraitResult, ExtractError> {
        report.status = Status::Failed;
        report.summary = summary;
        report.errors.push(error);
        Ok((report, Vec::new(), Vec::new()))
    };
    let pal = match &exe.bank {
        Ok(bank) => bank[PORTRAIT_PALETTE_SLOT],
        Err(e) => return fail(report, "no palette".into(), e.clone()),
    };
    let Some(bak) = read_source(install, "BAKDATA.R3", &mut report)? else {
        return fail(
            report,
            "BAKDATA.R3 missing".into(),
            "BAKDATA.R3 missing".into(),
        );
    };
    let table = match bakdata::parse(&bak, encoding) {
        Ok(b) => b.officers,
        Err(e) => return fail(report, "BAKDATA.R3 invalid".into(), e.to_string()),
    };
    let Some(faces) = read_source(install, crate::extract::PORTRAIT_SOURCE, &mut report)? else {
        return fail(
            report,
            "FACEDAT.R3 missing".into(),
            "FACEDAT.R3 missing".into(),
        );
    };
    let faces = match table6::Table6::parse(&faces) {
        Ok(t) => t,
        Err(e) => return fail(report, "FACEDAT.R3 invalid".into(), e.to_string()),
    };

    let (mut matches, mut unmatched) = (Vec::new(), Vec::new());
    let mut written = BTreeSet::new();
    for officer in &options.officers {
        let plain_key = !officer.portrait.is_empty()
            && officer
                .portrait
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !plain_key {
            unmatched.push(Unmatched {
                officer: officer.id.clone(),
                reason: format!(
                    "portrait key `{}` is not a plain file name",
                    officer.portrait
                ),
            });
            continue;
        }
        if written.contains(officer.portrait.as_str()) {
            // Officers sharing a portrait key share the picture written for the first one.
            continue;
        }
        let (bakdata, portrait) = match match_officer(officer, &table, edition) {
            FaceMatch::Found { bakdata, portrait } => (bakdata, portrait),
            FaceMatch::Missing => {
                unmatched.push(Unmatched {
                    officer: officer.id.clone(),
                    reason: "no BAKDATA officer of that name".into(),
                });
                continue;
            }
            FaceMatch::Ambiguous(list) => {
                let list: Vec<String> = list
                    .iter()
                    .map(|(r, p)| format!("record {r} → portrait {p}"))
                    .collect();
                unmatched.push(Unmatched {
                    officer: officer.id.clone(),
                    reason: format!("several BAKDATA officers of that name: {}", list.join(", ")),
                });
                continue;
            }
        };
        let payload = faces.get(usize::from(portrait)).unwrap_or_default();
        let png = crate::tfdce::decode(payload)
            .map_err(|e| e.to_string())
            .and_then(|img| {
                planar::decode(&img.planar, img.width, img.height).map_err(|e| e.to_string())
            })
            .and_then(|img| encode_png(&img, &pal, false).map_err(|e| e.to_string()));
        match png {
            Ok(png) => {
                out.write(&format!("gfx/portraits/{}.png", officer.portrait), &png)?;
                written.insert(officer.portrait.as_str());
                report.outputs += 1;
                matches.push(PortraitMatch {
                    officer: officer.id.clone(),
                    bakdata,
                    portrait,
                });
            }
            Err(e) => report
                .errors
                .push(format!("{}: FACEDAT.R3 entry {portrait}: {e}", officer.id)),
        }
    }
    report.status = match (matches.len(), report.errors.len()) {
        (_, 0) => Status::Extracted,
        (0, _) => Status::Failed,
        _ => Status::Partial,
    };
    report.summary = format!(
        "{} of {} officers have their original portrait",
        matches.len(),
        options.officers.len()
    );
    if !unmatched.is_empty() {
        report.notes.push(format!(
            "{} officers keep the base pack's portrait (the base pack's own characters, or names \
             the release does not have); listed in {PACK_INDEX}",
            unmatched.len()
        ));
    }
    Ok((report, matches, unmatched))
}

// ----- unit sheets ---------------------------------------------------------------------------

/// Sprite keys of the base pack's classes in the game's class order: `HEXZCHR` entries `2k` and
/// `2k + 1` show class `k` in the two army colours.
pub const CLASS_SPRITES: [&str; 19] = [
    "short_infantry",
    "long_infantry",
    "chariot",
    "archer",
    "crossbow",
    "catapult",
    "light_cavalry",
    "heavy_cavalry",
    "guard_cavalry",
    "bandit",
    "brigand",
    "outlaw",
    "band",
    "beast",
    "martial",
    "sorcerer",
    "tribe",
    "civilian",
    "supply",
];

/// `HEXZCHR` entry `2k + PLAYER_ICON` (the orange one) is drawn for the player's and allied
/// units, entry `2k + 1 - PLAYER_ICON` (the green / teal one) for enemies: `MAIN.EXE` picks
/// `class × 2 + 1` for a unit off the player's side (unit slots 15–29) and `class × 2` for the
/// player's side (slots 0–14, allies included), see docs/reverse-engineering/FORMATS.md §8.
pub const PLAYER_ICON: usize = 0;

/// Frame size of a map icon.
pub const ICON_PX: usize = 32;

/// Unit sheet (4 columns: down, up, left, right; 6 rows: walk 0–3, attack, hurt) from a 32×64
/// map icon (two 32×32 frames, top and bottom, facing right). Right and down show the stored
/// picture, left and up its mirror image; the walk rows alternate the two frames (the idle
/// animation plays the walk rows slowly, which gives the original's two-frame idle), the
/// attack pose is the first frame and the hurt pose the second.
pub fn unit_sheet(icon: &IndexedImage) -> Result<IndexedImage, String> {
    if icon.width != ICON_PX || icon.height != 2 * ICON_PX {
        return Err(format!(
            "map icon of {}×{}, expected {ICON_PX}×{}",
            icon.width,
            icon.height,
            2 * ICON_PX
        ));
    }
    const COLUMNS: usize = 4;
    const ROWS: usize = 6;
    let width = COLUMNS * ICON_PX;
    let mut sheet = IndexedImage {
        width,
        height: ROWS * ICON_PX,
        pixels: vec![0; width * ROWS * ICON_PX],
    };
    for row in 0..ROWS {
        let frame = row % 2; // walk f0 f1 f0 f1, attack f0, hurt f1
        for column in 0..COLUMNS {
            let mirrored = matches!(column, 1 | 2);
            for y in 0..ICON_PX {
                for x in 0..ICON_PX {
                    let sx = if mirrored { ICON_PX - 1 - x } else { x };
                    let pixel = icon.pixels[(frame * ICON_PX + y) * ICON_PX + sx];
                    sheet.pixels[(row * ICON_PX + y) * width + column * ICON_PX + x] = pixel;
                }
            }
        }
    }
    Ok(sheet)
}

fn units_toml() -> String {
    let mut s = String::from(
        "# Unit sprites of the original mode: the battle-map icons of HEXZCHR.R3, written by\n\
         # `hero-tools original pack` (do not edit; run the importer again). 32×32 frames stand on\n\
         # the 32-px tiles of gfx/tiles/terrain.toml. Layout and side colours: crates/hero-import/\n\
         # src/pack.rs (`unit_sheet`, `PLAYER_ICON`).\n",
    );
    for key in CLASS_SPRITES {
        let _ = write!(
            s,
            "\n[sprites.{key}]\nframe = [{ICON_PX}, {ICON_PX}]\nanchor = [{}, {}]\n",
            ICON_PX / 2,
            ICON_PX - 1
        );
    }
    s
}

fn convert_units(
    install: &InstallDir,
    exe: &Exe,
    options: &PackOptions,
    tiles_ok: bool,
    out: &mut Output,
    mut report: KindReport,
) -> Result<KindReport, ExtractError> {
    report.status = Status::Failed;
    let unknown: Vec<&str> = options
        .sprites
        .iter()
        .map(String::as_str)
        .filter(|s| !CLASS_SPRITES.contains(s))
        .collect();
    if !unknown.is_empty() {
        report.summary = "the pack has classes the original does not".into();
        report.errors.push(format!(
            "no original icons for the sprite keys {}; units.toml replaces the base pack's as a \
             whole, so no unit sheets were written",
            unknown.join(", ")
        ));
        return Ok(report);
    }
    if !tiles_ok {
        report.summary = "not written: the 32-px tileset could not be built".into();
        report.errors.push(
            "the original unit frames are sized for 32-px tiles; without the tileset they would \
             stand on the base pack's 16-px tiles"
                .into(),
        );
        return Ok(report);
    }
    let pal = match &exe.bank {
        Ok(bank) => bank[MAP_PALETTE_SLOT],
        Err(e) => {
            report.summary = "no palette".into();
            report.errors.push(e.clone());
            return Ok(report);
        }
    };
    let Some(data) = read_source(install, "HEXZCHR.R3", &mut report)? else {
        report.status = Status::MissingSource;
        report.summary = "HEXZCHR.R3 missing".into();
        return Ok(report);
    };
    let spec = sprites::archive("HEXZCHR.R3").expect("HEXZCHR.R3 is a known archive");
    let entries = match ls11::Archive::parse(&data).and_then(|a| a.decode_all()) {
        Ok(e) => e,
        Err(e) => {
            report.summary = "HEXZCHR.R3 invalid".into();
            report.errors.push(format!("HEXZCHR.R3: {e}"));
            return Ok(report);
        }
    };
    let icon = |i: usize| -> Result<IndexedImage, String> {
        let entry = entries
            .get(i)
            .ok_or_else(|| format!("HEXZCHR.R3 has no entry {i}"))?;
        let arrangement = (spec.arrangement)(i, entry.len())
            .ok_or_else(|| format!("HEXZCHR.R3 entry {i}: not an icon"))?;
        sprites::decode_entry(entry, arrangement).map_err(|e| format!("HEXZCHR.R3 entry {i}: {e}"))
    };
    // Build every sheet first: the index file is written only for a complete set.
    let mut sheets = Vec::new();
    for (k, key) in CLASS_SPRITES.iter().enumerate() {
        let player = icon(2 * k + PLAYER_ICON).and_then(|i| unit_sheet(&i));
        let enemy = icon(2 * k + 1 - PLAYER_ICON).and_then(|i| unit_sheet(&i));
        let encoded = player.and_then(|p| {
            let e = enemy?;
            let png = |img: &IndexedImage| encode_png(img, &pal, true).map_err(|e| e.to_string());
            Ok((png(&p)?, png(&e)?))
        });
        match encoded {
            Ok(pair) => sheets.push((*key, pair)),
            Err(e) => report.errors.push(format!("{key}: {e}")),
        }
    }
    if !report.errors.is_empty() {
        report.summary = "HEXZCHR.R3 does not hold every class icon".into();
        return Ok(report);
    }
    for (key, (player, enemy)) in &sheets {
        out.write(&format!("gfx/units/{key}_player.png"), player)?;
        out.write(&format!("gfx/units/{key}_ally.png"), player)?;
        out.write(&format!("gfx/units/{key}_enemy.png"), enemy)?;
        report.outputs += 3;
    }
    out.write("gfx/units/units.toml", units_toml().as_bytes())?;
    report.outputs += 1;
    report.status = Status::Extracted;
    report.summary = format!("{} classes, 32×32 frames", sheets.len());
    report.notes.push(
        "officer-specific icons (HEXZCHR 38-40, 45-46) and status icons (43-44) are not \
         used: every unit is drawn with its class icon"
            .into(),
    );
    Ok(report)
}

// ----- terrain tileset -----------------------------------------------------------------------

/// Base-pack terrain id of each original terrain code; `None` for fire and flood, which only
/// tactics set at run time.
pub const TERRAIN_MAP: [Option<&str>; TERRAIN_COUNT] = [
    Some("plain"),
    Some("forest"),
    Some("mountain"), // 산지, the green hills
    Some("river"),    // 개울
    Some("bridge"),
    Some("wall"),
    Some("castle"),
    Some("grass"), // 초원
    Some("village"),
    Some("cliff"),
    Some("gate"),
    Some("wasteland"),
    Some("fence"),
    Some("fort"), // 성채
    Some("barracks"),
    Some("granary"),
    Some("treasury"),
    Some("house"),
    None,
    None,
];

/// Base-pack terrain without an original terrain code, and the terrain whose tile it reuses.
/// (The original draws roads with plain cells.)
pub const TILE_FALLBACK: &[(&str, &str)] = &[("road", "plain")];

/// Terrain whose look depends on its neighbours (`auto` layers), with the terrain it joins.
/// Everything else is one block (`cells`).
pub const CONNECT: &[(&str, &[&str])] = &[
    ("grass", &["grass"]),
    ("forest", &["forest"]),
    ("mountain", &["mountain"]),
    ("wasteland", &["wasteland"]),
    ("river", &["river", "bridge"]),
    // The deck runs across the water, so a bridge follows the river beside it.
    ("bridge", &["river"]),
    ("wall", &["wall", "gate"]),
    ("castle", &["castle", "gate", "wall"]),
    ("cliff", &["cliff"]),
    ("fence", &["fence"]),
];

fn connect_of(id: &str) -> Option<&'static [&'static str]> {
    CONNECT.iter().find(|(t, _)| *t == id).map(|&(_, c)| c)
}

/// A 2×2 chip block of a battle map: the tile of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Block {
    /// `HEXZCHP` entry of the chips above [`maps::COMMON_CHIPS`] (0 when all four chips are
    /// shared ones, which look the same in both banks).
    pub set: u8,
    /// Top left, top right, bottom left, bottom right.
    pub chips: [u8; 4],
}

/// What [`learn_tiles`] found for one terrain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Learned {
    /// One block per neighbour mask (index = mask).
    Auto([Block; 16]),
    Cells(Block),
}

/// Tiles learned from the maps, by base-pack terrain id, with the number of cells seen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnedTiles {
    pub tiles: BTreeMap<&'static str, (Learned, u32)>,
    /// Neighbour masks borrowed from another mask, per terrain.
    pub borrowed: BTreeMap<&'static str, Vec<u8>>,
}

/// The mask of orthogonal neighbours of `(x, y)` that `joins` (1 = north, 2 = east, 4 = south,
/// 8 = west; outside the grid counts as joined, like the engine's `auto` layers).
pub fn neighbour_mask(
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    joins: impl Fn(usize, usize) -> bool,
) -> u8 {
    let mut mask = 0;
    for (bit, dx, dy) in [(1u8, 0i64, -1i64), (2, 1, 0), (4, 0, 1), (8, -1, 0)] {
        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
        let outside = nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64;
        if outside || joins(nx as usize, ny as usize) {
            mask |= bit;
        }
    }
    mask
}

/// The most frequent block (ties: the smallest).
fn mode(counts: &BTreeMap<Block, u32>) -> Option<Block> {
    counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(&b, _)| b)
}

/// Learn the tile of every terrain from battle maps given with their chip set (1 or 2).
pub fn learn_tiles(maps: &[(&BattleMap, usize)]) -> LearnedTiles {
    let id_of = |code: u8| TERRAIN_MAP.get(usize::from(code)).copied().flatten();
    let mut stats: BTreeMap<&'static str, BTreeMap<u8, BTreeMap<Block, u32>>> = BTreeMap::new();
    for &(map, set) in maps {
        let (w, h) = map.cells();
        for y in 0..h {
            for x in 0..w {
                let Some(id) = id_of(map.terrain[y * w + x]) else {
                    continue;
                };
                let mask = match connect_of(id) {
                    Some(joined) => neighbour_mask(w, h, x, y, |nx, ny| {
                        id_of(map.terrain[ny * w + nx]).is_some_and(|n| joined.contains(&n))
                    }),
                    None => 0,
                };
                let chip = |dx: usize, dy: usize| map.chips[(2 * y + dy) * map.width + 2 * x + dx];
                let chips = [chip(0, 0), chip(1, 0), chip(0, 1), chip(1, 1)];
                let own = chips.iter().any(|&c| usize::from(c) >= maps::COMMON_CHIPS);
                let block = Block {
                    set: if own { set as u8 } else { 0 },
                    chips,
                };
                *stats
                    .entry(id)
                    .or_default()
                    .entry(mask)
                    .or_default()
                    .entry(block)
                    .or_default() += 1;
            }
        }
    }
    let mut learned = LearnedTiles::default();
    for (id, by_mask) in stats {
        let seen: u32 = by_mask.values().flat_map(|c| c.values()).sum();
        let tile = if connect_of(id).is_some() {
            let totals: BTreeMap<u8, u32> = by_mask
                .iter()
                .map(|(&m, c)| (m, c.values().sum()))
                .collect();
            let mut borrowed = Vec::new();
            let blocks = std::array::from_fn(|m| {
                let m = m as u8;
                let source = if by_mask.contains_key(&m) {
                    m
                } else {
                    borrowed.push(m);
                    // The closest observed mask; ties: the more frequent, then the lower.
                    *totals
                        .iter()
                        .min_by_key(|&(&k, &n)| ((k ^ m).count_ones(), std::cmp::Reverse(n), k))
                        .expect("a terrain in the statistics has an observed mask")
                        .0
                };
                mode(&by_mask[&source]).expect("an observed mask has blocks")
            });
            if !borrowed.is_empty() {
                learned.borrowed.insert(id, borrowed);
            }
            Learned::Auto(blocks)
        } else {
            let mut all = BTreeMap::new();
            for counts in by_mask.values() {
                for (&b, &n) in counts {
                    *all.entry(b).or_default() += n;
                }
            }
            Learned::Cells(mode(&all).expect("a terrain in the statistics has blocks"))
        };
        learned.tiles.insert(id, (tile, seen));
    }
    learned
}

/// Decoded chips of the two battle banks (`HEXZCHP` entry 0 + entry 1 or 2).
struct Banks(BTreeMap<u8, Vec<IndexedImage>>);

impl Banks {
    fn new(chipsets: &[Vec<u8>]) -> Result<Banks, String> {
        let mut banks = BTreeMap::new();
        for set in [1u8, 2] {
            let bank = maps::battle_bank(chipsets, usize::from(set))
                .map_err(|e| format!("HEXZCHP.R3 set {set}: {e}"))?;
            let cells = bank
                .chunks_exact(CELL_BYTES)
                .map(|c| planar::decode(c, CELL_PX, CELL_PX).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            banks.insert(set, cells);
        }
        Ok(Banks(banks))
    }

    /// Draw `block` at `(x0, y0)` of `image`.
    fn draw(
        &self,
        block: Block,
        image: &mut IndexedImage,
        x0: usize,
        y0: usize,
    ) -> Result<(), String> {
        let bank = &self.0[&block.set.max(1)];
        for (i, &chip) in block.chips.iter().enumerate() {
            let cell = bank
                .get(usize::from(chip))
                .ok_or_else(|| format!("chip {chip} outside the bank of set {}", block.set))?;
            let (cx, cy) = (x0 + (i % 2) * CELL_PX, y0 + (i / 2) * CELL_PX);
            for y in 0..CELL_PX {
                let dst = (cy + y) * image.width + cx;
                image.pixels[dst..dst + CELL_PX]
                    .copy_from_slice(&cell.pixels[y * CELL_PX..(y + 1) * CELL_PX]);
            }
        }
        Ok(())
    }
}

/// Atlas cells of the blocks, in first-use order.
#[derive(Default)]
struct Atlas {
    blocks: Vec<Block>,
}

impl Atlas {
    fn cell(&mut self, block: Block) -> [usize; 2] {
        let i = match self.blocks.iter().position(|&b| b == block) {
            Some(i) => i,
            None => {
                self.blocks.push(block);
                self.blocks.len() - 1
            }
        };
        [i % ATLAS_COLUMNS, i / ATLAS_COLUMNS]
    }

    fn render(&self, banks: &Banks) -> Result<IndexedImage, String> {
        let rows = self.blocks.len().div_ceil(ATLAS_COLUMNS).max(1);
        let width = ATLAS_COLUMNS * TILE_PX;
        let mut image = IndexedImage {
            width,
            height: rows * TILE_PX,
            pixels: vec![0; width * rows * TILE_PX],
        };
        for (i, &block) in self.blocks.iter().enumerate() {
            banks.draw(
                block,
                &mut image,
                (i % ATLAS_COLUMNS) * TILE_PX,
                (i / ATLAS_COLUMNS) * TILE_PX,
            )?;
        }
        Ok(image)
    }
}

/// `terrain.toml` for the pack's tile keys and the atlas it refers to. Returns the file, the
/// atlas and notes on stand-ins.
fn tileset(
    learned: &LearnedTiles,
    terrain: &[BaseTerrain],
) -> Result<(String, Atlas, Vec<String>), String> {
    let known: BTreeSet<&str> = terrain.iter().map(|t| t.id.as_str()).collect();
    let mut atlas = Atlas::default();
    let mut notes = Vec::new();
    let mut toml = String::from(
        "# Battle-map terrain of the original mode, learned from the battle maps of the player's\n\
         # copy by `hero-tools original pack` (do not edit; run the importer again). One 32-px tile\n\
         # is one 2×2-chip cell of the original maps, the grid units move on. An `auto` layer picks\n\
         # the tile by the mask of orthogonal neighbours in `connect` (1 = north, 2 = east,\n\
         # 4 = south, 8 = west; outside the map counts as connected, docs/ASSETS.md); each tile is\n\
         # the block the original maps show most often for that terrain and mask.\n\
         \n",
    );
    let _ = writeln!(toml, "tile_size = {TILE_PX}\nimage = \"terrain.png\"");
    let mut done = BTreeSet::new();
    for t in terrain {
        if !done.insert(t.tile.as_str()) {
            continue;
        }
        let mapped = TERRAIN_MAP
            .iter()
            .flatten()
            .find(|&&id| id == t.id)
            .copied();
        let fallback = TILE_FALLBACK
            .iter()
            .find(|(id, _)| *id == t.id)
            .map(|&(_, f)| f);
        let source = match (mapped, fallback) {
            (Some(id), _) => id,
            (None, Some(f)) => {
                notes.push(format!(
                    "`{}`: the original has no such terrain; drawn with the `{f}` tile",
                    t.tile
                ));
                f
            }
            (None, None) => {
                notes.push(format!(
                    "`{}`: unknown to the original, drawn as plain",
                    t.tile
                ));
                "plain"
            }
        };
        let (tile, seen, source) = match learned.tiles.get(source) {
            Some((tile, seen)) => (tile, *seen, source),
            None => {
                let (tile, seen) = learned
                    .tiles
                    .get("plain")
                    .ok_or("the battle maps have no plain cell to stand in for missing terrain")?;
                notes.push(format!(
                    "`{}`: no `{source}` cell in the original maps, drawn as plain",
                    t.tile
                ));
                (tile, *seen, "plain")
            }
        };
        let _ = write!(
            toml,
            "\n# {}: {source}, {seen} cells in the original maps\n",
            t.id
        );
        let _ = writeln!(toml, "[tiles.{}]\nlayers = [", toml_key(&t.tile));
        match tile {
            Learned::Cells(block) => {
                let [c, r] = atlas.cell(*block);
                let _ = writeln!(toml, "  {{ cells = [[{c}, {r}]] }},");
            }
            Learned::Auto(blocks) => {
                let cells: Vec<String> = blocks
                    .iter()
                    .map(|&b| {
                        let [c, r] = atlas.cell(b);
                        format!("[{c}, {r}]")
                    })
                    .collect();
                let joined: Vec<String> = connect_of(source)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|id| known.contains(*id))
                    .map(|id| toml_str(id))
                    .collect();
                let _ = writeln!(
                    toml,
                    "  {{ auto = [{}], connect = [{}] }},",
                    cells.join(", "),
                    joined.join(", ")
                );
            }
        }
        toml.push_str("]\n");
    }
    Ok((toml, atlas, notes))
}

/// A TOML key: bare when it can be, quoted otherwise.
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_string()
    } else {
        toml_str(key)
    }
}

/// The readable battle maps of `HEXZMAP.R3` with their entry numbers, the map names of its
/// last entry, and the `HEXZCHP.R3` entries.
struct BattleMaps {
    maps: Vec<(usize, BattleMap)>,
    /// Raw name per map number (empty when the name entry is missing).
    names: Vec<Vec<u8>>,
    chipsets: Vec<Vec<u8>>,
}

/// Read the battle maps; an unreadable archive is the inner error, an unreadable map entry is
/// pushed to `report.errors` and left out.
fn read_battle_maps(
    install: &InstallDir,
    report: &mut KindReport,
) -> Result<Result<BattleMaps, String>, ExtractError> {
    let archive = |name: &str,
                   report: &mut KindReport|
     -> Result<Result<Vec<Vec<u8>>, String>, ExtractError> {
        Ok(match read_source(install, name, report)? {
            None => Err(format!("{name} missing")),
            Some(data) => ls11::Archive::parse(&data)
                .and_then(|a| a.decode_all())
                .map_err(|e| format!("{name}: {e}")),
        })
    };
    let entries = archive("HEXZMAP.R3", report)?;
    let chipsets = archive("HEXZCHP.R3", report)?;
    let (entries, chipsets) = match (entries, chipsets) {
        (Ok(e), Ok(c)) => (e, c),
        (Err(e), _) | (_, Err(e)) => return Ok(Err(e)),
    };
    let mut maps: Vec<(usize, BattleMap)> = Vec::new();
    let mut names = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match BattleMap::parse(entry) {
            Ok(map) => maps.push((i, map)),
            // The last entry is the name list (as in the map extraction).
            Err(_) if i + 1 == entries.len() && !maps.is_empty() => {
                names = maps::parse_map_names(entry);
            }
            Err(e) => report.errors.push(format!("HEXZMAP.R3 entry {i}: {e}")),
        }
    }
    Ok(Ok(BattleMaps {
        maps,
        names,
        chipsets,
    }))
}

fn convert_tiles(
    install: &InstallDir,
    exe: &Exe,
    options: &PackOptions,
    out: &mut Output,
    mut report: KindReport,
) -> Result<KindReport, ExtractError> {
    report.status = Status::Failed;
    let (bank, tables) = match (&exe.bank, &exe.tables) {
        (Ok(bank), Ok(tables)) => (bank, tables),
        (Err(e), _) | (_, Err(e)) => {
            report.summary = "MAIN.EXE tables not found".into();
            report.errors.push(e.clone());
            return Ok(report);
        }
    };
    let BattleMaps {
        maps: battle,
        chipsets,
        ..
    } = match read_battle_maps(install, &mut report)? {
        Ok(b) => b,
        Err(e) => {
            report.summary = "battle maps not readable".into();
            report.errors.push(e);
            return Ok(report);
        }
    };
    if battle.is_empty() {
        report.summary = "HEXZMAP.R3 holds no readable battle map".into();
        return Ok(report);
    }
    let pairs: Vec<(&BattleMap, usize)> = battle
        .iter()
        .map(|(i, m)| (m, tables.chip_set_for(*i)))
        .collect();
    let learned = learn_tiles(&pairs);
    let built = Banks::new(&chipsets).and_then(|banks| {
        let (toml, atlas, notes) = tileset(&learned, &options.terrain)?;
        let image = atlas.render(&banks)?;
        let png = encode_png(&image, &bank[MAP_PALETTE_SLOT], false).map_err(|e| e.to_string())?;
        Ok((toml, png, atlas.blocks.len(), notes))
    });
    let (toml, png, blocks, notes) = match built {
        Ok(b) => b,
        Err(e) => {
            report.summary = "tileset not built".into();
            report.errors.push(e);
            return Ok(report);
        }
    };
    out.write("gfx/tiles/terrain.png", &png)?;
    out.write("gfx/tiles/terrain.toml", toml.as_bytes())?;
    report.outputs += 2;
    // Learned from the maps that could be read; a map that could not is an error.
    report.status = if report.errors.is_empty() {
        Status::Extracted
    } else {
        Status::Partial
    };
    report.summary = format!(
        "{} terrain tiles from {} battle maps ({blocks} distinct 32-px blocks)",
        learned.tiles.len(),
        battle.len()
    );
    report.notes.extend(notes);
    for (id, masks) in &learned.borrowed {
        let masks: Vec<String> = masks.iter().map(u8::to_string).collect();
        report.notes.push(format!(
            "`{id}`: neighbour masks {} never occur in the maps; the closest observed mask's tile is used",
            masks.join(", ")
        ));
    }
    Ok(report)
}

// ----- battle maps ---------------------------------------------------------------------------

/// Id of the map file entry (and key of the picture) of `HEXZMAP.R3` entry `number`. The number
/// is kept because the scenario scripts name maps by it.
pub fn map_id(number: usize) -> String {
    format!("hexz_{number:02}")
}

/// Pack-relative path of the map file the pack writes.
pub const MAPS_FILE: &str = "maps/original.toml";

/// Rules-grid character of an original terrain code: the code in base 36 (`0`–`9`, `a`–`h`),
/// so a row reads as the map's terrain bytes (docs/reverse-engineering/FORMATS.md §10.4). The
/// map's `legend` names the pack terrain of each character, which keeps the grid independent
/// of the glyphs the base pack happens to use.
pub fn code_glyph(code: u8) -> Option<char> {
    char::from_digit(u32::from(code), 36)
}

/// A cell whose terrain code names no pack terrain (fire, flood, or a code the documentation
/// does not know), and the code whose terrain it gets instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StandIn {
    /// `[x, y]` in cells (32-px tiles), `[0, 0]` top left.
    pub cell: [usize; 2],
    pub code: u8,
    pub used: u8,
}

/// A converted battle map, as listed in [`PACK_INDEX`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapRecord {
    /// `HEXZMAP.R3` entry.
    pub number: usize,
    /// Map file id and picture key ([`map_id`]).
    pub id: String,
    /// Name from the name entry, decoded (empty when there is none).
    pub name: String,
    /// `[width, height]` in cells.
    pub cells: [usize; 2],
    /// Second `HEXZCHP` entry of the chip bank (1 or 2).
    pub chip_set: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stand_ins: Vec<StandIn>,
}

/// How often each chip is drawn in cells of each terrain code, over every map
/// ([`maps::chip_uses`]).
pub struct ChipTerrain {
    uses: Vec<maps::ChipUse>,
    sizes: [usize; 3],
}

impl ChipTerrain {
    /// Statistics of `maps` (each with its chip set 1 or 2) for banks of `sizes` chips.
    pub fn new(maps: &[(&BattleMap, usize)], sizes: [usize; 3]) -> ChipTerrain {
        ChipTerrain {
            uses: maps::chip_uses(maps, sizes),
            sizes,
        }
    }

    fn counts(&self, chip: u8, set: usize) -> Option<&[u32]> {
        let chip = usize::from(chip);
        let (set, index) = if chip < maps::COMMON_CHIPS {
            (0, chip)
        } else {
            (set, chip - maps::COMMON_CHIPS)
        };
        if set >= 3 || index >= self.sizes[set] {
            return None;
        }
        let offset: usize = self.sizes[..set].iter().sum();
        Some(&self.uses[offset + index].counts)
    }

    /// The terrain code the four chips of a cell are most often drawn in, among codes with a
    /// pack terrain (ties: the lower code). This is what the picture shows at that cell.
    pub fn code_of(&self, chips: [u8; 4], set: usize) -> Option<u8> {
        let mut totals = [0u32; TERRAIN_COUNT];
        for chip in chips {
            if let Some(counts) = self.counts(chip, set) {
                for (code, total) in totals.iter_mut().enumerate() {
                    *total += counts[code];
                }
            }
        }
        (0..TERRAIN_COUNT)
            .filter(|&code| TERRAIN_MAP[code].is_some() && totals[code] > 0)
            .max_by_key(|&code| (totals[code], std::cmp::Reverse(code)))
            .map(|code| code as u8)
    }
}

/// Rows, legend and stand-ins of a map's rules grid.
pub type MapRows = (String, BTreeMap<char, &'static str>, Vec<StandIn>);

/// The rules grid of `map` (chip set `set`): rows of [`code_glyph`] characters and the legend
/// of the characters used. A code without pack terrain ([`TERRAIN_MAP`]: fire, flood, unknown
/// codes) gets the terrain its chips show ([`ChipTerrain::code_of`]) and is listed as a stand-in.
/// Fails when a terrain the map needs is not in `known` (the pack chain's terrain ids).
pub fn map_rows(
    map: &BattleMap,
    set: usize,
    chips: &ChipTerrain,
    known: &BTreeSet<&str>,
) -> Result<MapRows, String> {
    let (w, h) = map.cells();
    let mut rows = String::with_capacity((w + 1) * h);
    let mut legend = BTreeMap::new();
    let mut stand_ins = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let code = map.terrain[y * w + x];
            let used = match TERRAIN_MAP.get(usize::from(code)).copied().flatten() {
                Some(_) => code,
                None => {
                    let chip =
                        |dx: usize, dy: usize| map.chips[(2 * y + dy) * map.width + 2 * x + dx];
                    let used = chips
                        .code_of([chip(0, 0), chip(1, 0), chip(0, 1), chip(1, 1)], set)
                        .ok_or_else(|| {
                            format!(
                                "cell ({x}, {y}) has terrain code {code} and chips that no \
                                 terrain is drawn with"
                            )
                        })?;
                    stand_ins.push(StandIn {
                        cell: [x, y],
                        code,
                        used,
                    });
                    used
                }
            };
            let id = TERRAIN_MAP[usize::from(used)].expect("stand-ins have a pack terrain");
            if !known.contains(id) {
                return Err(format!(
                    "terrain code {used} is `{id}`, which the pack's terrain does not have"
                ));
            }
            let glyph = code_glyph(used).expect("terrain codes are below 36");
            legend.insert(glyph, id);
            rows.push(glyph);
        }
        rows.push('\n');
    }
    Ok((rows, legend, stand_ins))
}

/// One `[[map]]` table of [`MAPS_FILE`]; `shown` is the part of the name the game shows.
fn map_entry_toml(
    record: &MapRecord,
    shown: &str,
    rows: &str,
    legend: &BTreeMap<char, &str>,
) -> String {
    let mut s = String::new();
    let [w, h] = record.cells;
    let name = if record.name.is_empty() {
        "no name".to_string()
    } else {
        format!("{} (the game shows \"{shown}\")", record.name)
    };
    let _ = write!(
        s,
        "\n# HEXZMAP.R3 entry {}: {name}, {w}×{h} cells, chip set {}",
        record.number, record.chip_set
    );
    for st in &record.stand_ins {
        let _ = write!(
            s,
            "\n# cell [{}, {}]: terrain code {} has no pack terrain; code {} (what its chips show) is used",
            st.cell[0], st.cell[1], st.code, st.used
        );
    }
    let legend: Vec<String> = legend
        .iter()
        .map(|(g, id)| format!("{} = {}", toml_str(&g.to_string()), toml_str(id)))
        .collect();
    let id = toml_str(&record.id);
    let _ = write!(
        s,
        "\n[[map]]\nid = {id}\nname = {}\nimage = {id}\nlegend = {{ {} }}\nrows = '''\n{rows}'''\n",
        toml_str(&record.name),
        legend.join(", "),
    );
    s
}

fn maps_file_header() -> String {
    format!(
        "# Battle maps of the original mode, converted from HEXZMAP.R3 of the player's copy by\n\
         # `hero-tools original pack` (do not edit; run the importer again). A battle plays on one\n\
         # with `[map] use = \"<id>\"`; the id keeps the map's entry number, which the scenario\n\
         # scripts use. Each map has a picture layer, gfx/maps/<id>.png (the map's 16-px chips as\n\
         # the game draws them, so one {TILE_PX}-px tile is one 2×2-chip cell), and a rules grid made\n\
         # from the map's terrain bytes: one character per cell, the original terrain code in\n\
         # base 36 (docs/reverse-engineering/FORMATS.md §10.4), with `legend` naming its terrain.\n"
    )
}

/// The decoded battle maps, their chip banks and palette, for the battles' changed cells.
pub struct MapStore {
    maps: BTreeMap<usize, BattleMap>,
    /// Chip bank by `HEXZCHP` entry (1 or 2).
    banks: BTreeMap<usize, Vec<u8>>,
    palette: Palette16,
}

type MapsResult = (KindReport, Vec<MapRecord>, Option<MapStore>);

fn convert_maps(
    install: &InstallDir,
    encoding: TextEncoding,
    exe: &Exe,
    options: &PackOptions,
    tiles_ok: bool,
    out: &mut Output,
    mut report: KindReport,
) -> Result<MapsResult, ExtractError> {
    report.status = Status::Failed;
    if !tiles_ok {
        report.summary = "not written: the 32-px tileset could not be built".into();
        report.errors.push(
            "the map pictures are drawn at 32 px per tile; without the tileset the pack would \
             use the base pack's 16-px tiles"
                .into(),
        );
        return Ok((report, Vec::new(), None));
    }
    let (bank, tables) = match (&exe.bank, &exe.tables) {
        (Ok(bank), Ok(tables)) => (bank, tables),
        (Err(e), _) | (_, Err(e)) => {
            report.summary = "MAIN.EXE tables not found".into();
            report.errors.push(e.clone());
            return Ok((report, Vec::new(), None));
        }
    };
    let BattleMaps {
        maps: battle,
        names,
        chipsets,
    } = match read_battle_maps(install, &mut report)? {
        Ok(b) => b,
        Err(e) => {
            report.summary = "battle maps not readable".into();
            report.errors.push(e);
            return Ok((report, Vec::new(), None));
        }
    };
    let banks: Result<BTreeMap<usize, Vec<u8>>, String> = [1, 2]
        .into_iter()
        .map(|set| {
            maps::battle_bank(&chipsets, set)
                .map(|b| (set, b))
                .map_err(|e| format!("HEXZCHP.R3 set {set}: {e}"))
        })
        .collect();
    let banks = match banks {
        Ok(b) => b,
        Err(e) => {
            report.summary = "chip banks not readable".into();
            report.errors.push(e);
            return Ok((report, Vec::new(), None));
        }
    };
    let bank_cells = |set: usize| chipsets.get(set).map_or(0, |c| c.len() / CELL_BYTES);
    let pairs: Vec<(&BattleMap, usize)> = battle
        .iter()
        .map(|(i, m)| (m, tables.chip_set_for(*i)))
        .collect();
    let chips = ChipTerrain::new(&pairs, [maps::COMMON_CHIPS, bank_cells(1), bank_cells(2)]);
    let known: BTreeSet<&str> = options.terrain.iter().map(|t| t.id.as_str()).collect();
    let pal = &bank[MAP_PALETTE_SLOT];
    let decode = |bytes: &[u8]| encoding.decode(bytes).text.trim().to_string();

    let mut toml = maps_file_header();
    let mut records = Vec::new();
    for (number, map) in &battle {
        let number = *number;
        let set = tables.chip_set_for(number);
        let converted = map_rows(map, set, &chips, &known).and_then(|(rows, legend, stand_ins)| {
            let image = maps::render_tiles(&map.chips, map.width, map.height, &banks[&set])
                .map_err(|e| e.to_string())?;
            let png = encode_png(&image, pal, false).map_err(|e| e.to_string())?;
            Ok((rows, legend, stand_ins, png))
        });
        let (rows, legend, stand_ins, png) = match converted {
            Ok(c) => c,
            Err(e) => {
                report
                    .errors
                    .push(format!("HEXZMAP.R3 entry {number}: {e}"));
                continue;
            }
        };
        let raw = names.get(number).map(Vec::as_slice).unwrap_or_default();
        let (w, h) = map.cells();
        let record = MapRecord {
            number,
            id: map_id(number),
            name: decode(raw),
            cells: [w, h],
            chip_set: set,
            stand_ins,
        };
        out.write(&format!("gfx/maps/{}.png", record.id), &png)?;
        report.outputs += 1;
        let shown = decode(maps::display_name(raw));
        toml.push_str(&map_entry_toml(&record, &shown, &rows, &legend));
        records.push(record);
    }
    if records.is_empty() {
        report.summary = "no battle map could be converted".into();
        return Ok((report, records, None));
    }
    out.write(MAPS_FILE, toml.as_bytes())?;
    report.outputs += 1;
    report.status = if report.errors.is_empty() {
        Status::Extracted
    } else {
        Status::Partial
    };
    let stand_ins: usize = records.iter().map(|r| r.stand_ins.len()).sum();
    report.summary = format!(
        "{} battle maps (picture layer + rules grid), {stand_ins} cells with a stand-in terrain",
        records.len()
    );
    if names.is_empty() {
        report
            .notes
            .push("HEXZMAP.R3 has no name entry; the maps have no names".into());
    }
    if stand_ins > 0 {
        report.notes.push(format!(
            "{stand_ins} cells have a terrain code without pack terrain; they get the terrain \
             their chips are drawn with elsewhere (listed per map in {PACK_INDEX} and {MAPS_FILE})"
        ));
    }
    report.notes.push(
        "the base battles re-staged as the original battles use them (`battles`); the other \
         maps wait for the chapters the base pack does not have yet"
            .into(),
    );
    let store = MapStore {
        maps: battle.into_iter().collect(),
        banks,
        palette: *pal,
    };
    Ok((report, records, Some(store)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{self, TempDir};

    fn officer(index: usize, name: &str, reading: &str, portrait: u16) -> Officer {
        Officer {
            index,
            name: name.into(),
            reading: reading.into(),
            portrait,
            sprite: 0,
            leadership: 0,
            war: 0,
            intelligence: 0,
            flags: 0,
            army: 0,
            role: 0,
            morale: 0,
            troops: 0,
            class: 0,
            level: 0,
            exp: 0,
            items: Vec::new(),
            other: [0; 2],
        }
    }

    fn base(id: &str, name: &str, hanja: &str) -> BaseOfficer {
        BaseOfficer {
            id: id.into(),
            name: name.into(),
            hanja: hanja.into(),
            portrait: id.into(),
        }
    }

    #[test]
    fn officers_are_matched_by_name_alias_and_reading() {
        let table = [
            officer(0, "유비", "ﾘｭｳﾋﾞ", 0),
            officer(62, "우금", "ｳｷﾝ", 39),
            officer(170, "우금", "ｷﾞｭｳｷﾝ", 49),
            officer(79, "장요", "ﾁｮｳﾘｮｳ", 36),
            officer(256, "보병대", "", 215),
            officer(257, "보병대", "", 215),
            officer(300, "쌍둥이", "", 1),
            officer(301, "쌍둥이", "", 2),
        ];
        let korean = EditionId::KoreanDos;
        let found = |bakdata, portrait| FaceMatch::Found { bakdata, portrait };
        assert_eq!(
            match_officer(&base("liu_bei", "유비", "劉備"), &table, korean),
            found(0, 0)
        );
        assert_eq!(
            match_officer(&base("yu_jin", "우금", "于禁"), &table, korean),
            found(62, 39)
        );
        assert_eq!(
            match_officer(&base("zhang_liao", "장료", "張遼"), &table, korean),
            found(79, 36)
        );
        // The same portrait under several records is one match.
        assert_eq!(
            match_officer(&base("inf", "보병대", ""), &table, korean),
            found(256, 215)
        );
        assert_eq!(
            match_officer(&base("twin", "쌍둥이", ""), &table, korean),
            FaceMatch::Ambiguous(vec![(300, 1), (301, 2)])
        );
        assert_eq!(
            match_officer(&base("x", "없음", ""), &table, korean),
            FaceMatch::Missing
        );
        // The Chinese release names officers in hanja.
        let chinese = [officer(0, "劉備", "ﾘｭｳﾋﾞ", 0)];
        assert_eq!(
            match_officer(
                &base("liu_bei", "유비", "劉備"),
                &chinese,
                EditionId::ChineseDos
            ),
            found(0, 0)
        );
        assert_eq!(
            match_officer(&base("x", "유비", ""), &chinese, EditionId::ChineseDos),
            FaceMatch::Missing
        );
    }

    #[test]
    fn unit_sheets_mirror_and_alternate_the_two_frames() {
        // Top frame: colour 1 in the left column, bottom frame: colour 2 in the right column.
        let mut icon = IndexedImage {
            width: 32,
            height: 64,
            pixels: vec![0; 32 * 64],
        };
        for y in 0..32 {
            icon.pixels[y * 32] = 1;
            icon.pixels[(32 + y) * 32 + 31] = 2;
        }
        let sheet = unit_sheet(&icon).unwrap();
        assert_eq!((sheet.width, sheet.height), (128, 192));
        let at =
            |col: usize, row: usize, x: usize| sheet.pixels[(row * 32 + 5) * 128 + col * 32 + x];
        for row in 0..6 {
            let (colour, stored_x) = if row % 2 == 0 { (1, 0) } else { (2, 31) };
            // down and right keep the stored facing, up and left are mirrored
            assert_eq!(at(0, row, stored_x), colour, "row {row}");
            assert_eq!(at(3, row, stored_x), colour, "row {row}");
            assert_eq!(at(1, row, 31 - stored_x), colour, "row {row}");
            assert_eq!(at(2, row, 31 - stored_x), colour, "row {row}");
            assert_eq!(at(0, row, 31 - stored_x), 0, "row {row}");
        }
        assert!(unit_sheet(&IndexedImage {
            width: 32,
            height: 32,
            pixels: vec![0; 1024]
        })
        .is_err());
    }

    /// A map of `terrain` codes whose cell `(x, y)` shows chips `4k .. 4k+3`, `k` = the code, so
    /// the learned block tells which terrain it came from; one cell uses a set-specific chip.
    fn map(terrain: &[&[u8]]) -> BattleMap {
        let (w, h) = (terrain[0].len(), terrain.len());
        let mut chips = vec![0; 4 * w * h];
        for (y, row) in terrain.iter().enumerate() {
            for (x, &code) in row.iter().enumerate() {
                for (i, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                    chips[(2 * y + dy) * 2 * w + 2 * x + dx] = 4 * code + i as u8;
                }
            }
        }
        BattleMap {
            width: 2 * w,
            height: 2 * h,
            chips,
            terrain: terrain.concat(),
        }
    }

    #[test]
    fn tiles_are_the_most_frequent_block_per_terrain_and_mask() {
        // A horizontal stream (code 3) across a plain (0), with a bridge (4) in the middle.
        let mut m = map(&[&[0, 0, 0, 0, 0], &[3, 3, 4, 3, 3], &[0, 0, 0, 0, 0]]);
        // A second look for one plain cell: the more frequent one wins.
        m.chips[0] = 90;
        let learned = learn_tiles(&[(&m, 2)]);
        let plain = Block {
            set: 0,
            chips: [0, 1, 2, 3],
        };
        assert_eq!(learned.tiles["plain"], (Learned::Cells(plain), 10));
        let Learned::Auto(river) = learned.tiles["river"].0 else {
            panic!("river is an auto tile");
        };
        let stream = Block {
            set: 0,
            chips: [12, 13, 14, 15],
        };
        // Observed: west end (out of map west + east neighbour) = 2|8, middle = 2|8 too.
        assert_eq!(river[2 | 8], stream);
        // Every other mask is borrowed from the closest observed one.
        assert_eq!(river.iter().filter(|&&b| b == stream).count(), 16);
        let borrowed = &learned.borrowed["river"];
        assert_eq!(borrowed.len(), 15);
        assert!(!borrowed.contains(&(2 | 8)));
        // The bridge joins the river on both sides.
        let Learned::Auto(bridge) = learned.tiles["bridge"].0 else {
            panic!("bridge is an auto tile");
        };
        assert_eq!(
            bridge[2 | 8],
            Block {
                set: 0,
                chips: [16, 17, 18, 19]
            }
        );
        // A set-specific chip keeps its set.
        let mut m2 = map(&[&[0]]);
        m2.chips = vec![90, 91, 92, 93];
        let learned = learn_tiles(&[(&m2, 2), (&m2, 2)]);
        assert_eq!(
            learned.tiles["plain"],
            (
                Learned::Cells(Block {
                    set: 2,
                    chips: [90, 91, 92, 93]
                }),
                2
            )
        );
        // Fire / flood and unknown codes are ignored.
        assert!(learn_tiles(&[(&map(&[&[18, 19, 30]]), 1)]).tiles.is_empty());
    }

    #[test]
    fn neighbour_masks_count_the_outside_as_joined() {
        let grid = [1, 0, 1, 1];
        let joins = |x: usize, y: usize| grid[y * 2 + x] == 1;
        assert_eq!(neighbour_mask(2, 2, 0, 0, joins), 1 | 8 | 4);
        assert_eq!(neighbour_mask(2, 2, 1, 1, joins), 2 | 4 | 8);
        assert_eq!(neighbour_mask(1, 1, 0, 0, |_, _| false), 15);
    }

    #[test]
    fn tileset_covers_every_tile_key_with_stand_ins() {
        let m = map(&[&[0, 3, 3], &[7, 7, 0]]);
        let learned = learn_tiles(&[(&m, 1)]);
        let terrain: Vec<BaseTerrain> = [
            ("plain", "plain"),
            ("road", "road"),
            ("river", "river"),
            ("grass", "grass"),
            ("lava", "lava"),
            ("plain2", "plain"),
        ]
        .iter()
        .map(|&(id, tile)| BaseTerrain {
            id: id.into(),
            tile: tile.into(),
        })
        .collect();
        let (toml, atlas, notes) = tileset(&learned, &terrain).unwrap();
        assert!(toml.contains("tile_size = 32\n"), "{toml}");
        for key in [
            "[tiles.plain]",
            "[tiles.road]",
            "[tiles.river]",
            "[tiles.grass]",
            "[tiles.lava]",
        ] {
            assert_eq!(toml.matches(key).count(), 1, "{key} in {toml}");
        }
        // River joins river and bridge, but only known terrain is listed.
        assert!(toml.contains("connect = [\"river\"] }"), "{toml}");
        assert!(!atlas.blocks.is_empty());
        assert!(
            notes
                .iter()
                .any(|n| n.contains("`road`") && n.contains("`plain`")),
            "{notes:?}"
        );
        assert!(notes.iter().any(|n| n.contains("`lava`")), "{notes:?}");
        // Without any plain cell a missing terrain cannot be drawn.
        let only_river = learn_tiles(&[(&map(&[&[3]]), 1)]);
        assert!(tileset(&only_river, &terrain).is_err());
    }

    #[test]
    fn items_match_in_the_release_language() {
        let items = [
            BaseItem {
                id: "bean".into(),
                name: "콩".into(),
                hanja: "豆".into(),
            },
            BaseItem {
                id: "new".into(),
                name: "새 아이템".into(),
                hanja: String::new(),
            },
        ];
        assert_eq!(
            item_names(&items, EditionId::KoreanDos),
            [
                ("bean".to_string(), "콩".to_string()),
                ("new".to_string(), "새 아이템".to_string())
            ]
        );
        assert_eq!(
            item_names(&items, EditionId::ChineseDos),
            [("bean".to_string(), "豆".to_string())]
        );
    }

    #[test]
    fn manifest_needs_a_relative_extends() {
        let toml = pack_toml("../base", EditionId::KoreanDos, false, &[], false).unwrap();
        assert!(toml.contains("\nid = \"original\"\n"), "{toml}");
        assert!(toml.contains("\nextends = \"../base\"\n"), "{toml}");
        assert!(toml.contains("canvas = [640, 480]"), "{toml}");
        assert!(!toml.contains("maps"), "{toml}");
        let toml = pack_toml("../base", EditionId::KoreanDos, true, &[], false).unwrap();
        assert!(
            toml.contains(
                "\nextends = \"../base\"\nmaps = [\"maps/original.toml\"]\n\n[presentation]"
            ),
            "{toml}"
        );
        let battles = [
            "battles/p1_sishui.toml".to_string(),
            "battles/p2_hulao.toml".to_string(),
        ];
        let toml = pack_toml("../base", EditionId::KoreanDos, true, &battles, true).unwrap();
        let manifest: hero_core::pack::PackManifest = toml::from_str(&toml).unwrap();
        assert_eq!(manifest.battles, battles);
        assert_eq!(manifest.dramas, [DRAMA_FILE]);
        for bad in ["", "C:/data/base", "/data/base", "..\\base"] {
            assert!(
                pack_toml(bad, EditionId::KoreanDos, false, &[], false).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn rules_grids_keep_the_terrain_codes() {
        assert_eq!(code_glyph(0), Some('0'));
        assert_eq!(code_glyph(10), Some('a'));
        assert_eq!(code_glyph(17), Some('h'));
        let known: BTreeSet<&str> = ["plain", "forest", "river", "bridge", "castle"].into();
        let m = map(&[&[0, 1, 0], &[3, 4, 3]]);
        let chips = ChipTerrain::new(&[(&m, 1)], [80, 0, 0]);
        let (rows, legend, stand_ins) = map_rows(&m, 1, &chips, &known).unwrap();
        assert_eq!(rows, "010\n343\n");
        let legend: Vec<(char, &str)> = legend.into_iter().collect();
        assert_eq!(
            legend,
            [
                ('0', "plain"),
                ('1', "forest"),
                ('3', "river"),
                ('4', "bridge")
            ]
        );
        assert!(stand_ins.is_empty());
        // A terrain the pack does not have stops the map.
        let known_less: BTreeSet<&str> = ["plain", "forest", "river"].into();
        let e = map_rows(&m, 1, &chips, &known_less).unwrap_err();
        assert!(e.contains("code 4 is `bridge`"), "{e}");
    }

    #[test]
    fn cells_without_pack_terrain_get_what_their_chips_show() {
        let known: BTreeSet<&str> = ["plain", "forest", "castle"].into();
        // Cell (1, 0) has code 255 but the chips of a castle cell (code 6), which the other
        // map draws twice as castle and once as forest.
        let mut odd = map(&[&[0, 6]]);
        odd.terrain[1] = 255;
        let other = map(&[&[6, 6], &[1, 0]]);
        let mut other_forest = map(&[&[1]]);
        other_forest.chips = vec![24, 25, 26, 27];
        let chips = ChipTerrain::new(&[(&odd, 1), (&other, 1), (&other_forest, 1)], [80, 0, 0]);
        let (rows, legend, stand_ins) = map_rows(&odd, 1, &chips, &known).unwrap();
        assert_eq!(rows, "06\n");
        assert_eq!(legend[&'6'], "castle");
        assert_eq!(
            stand_ins,
            vec![StandIn {
                cell: [1, 0],
                code: 255,
                used: 6
            }]
        );
        // Fire and flood (18, 19) are stand-ins too; chips never drawn with pack terrain fail.
        let mut fire = map(&[&[0, 18]]);
        fire.chips[2] = 70;
        fire.chips[3] = 71;
        fire.chips[6] = 72;
        fire.chips[7] = 73;
        let chips = ChipTerrain::new(&[(&fire, 1)], [80, 0, 0]);
        let e = map_rows(&fire, 1, &chips, &known).unwrap_err();
        assert!(e.contains("cell (1, 0) has terrain code 18"), "{e}");
    }

    /// The map install with maps that have plain, forest, stream and bridge cells, and the 47
    /// map icons of HEXZCHR.R3.
    fn write_pack_install(dir: &Path) {
        testutil::write_map_install(dir);
        let a = map(&[&[0, 0, 1], &[3, 4, 3]]);
        let mut b = map(&[&[1, 0], &[0, 0]]);
        b.chips[0] = 81; // a chip of set 2
        let names = b"\xb0\xa1\r\n\xb0\xa2\r\n\r\n\x1a".to_vec();
        std::fs::write(
            dir.join("HEXZMAP.R3"),
            ls11::build(&[&a.encode(), &b.encode(), &names]),
        )
        .unwrap();
        let icons: Vec<Vec<u8>> = (0..47).map(|_| testutil::cells(8)).collect();
        let refs: Vec<&[u8]> = icons.iter().map(Vec::as_slice).collect();
        std::fs::write(dir.join("HEXZCHR.R3"), ls11::build(&refs)).unwrap();
    }

    fn options() -> PackOptions {
        PackOptions {
            edition: None,
            extends: "../base".into(),
            officers: vec![
                base("liu_bei", "유비", "劉備"),
                base("ours", "없는사람", ""),
                BaseOfficer {
                    portrait: "../evil".into(),
                    ..base("guan_yu", "관우", "關羽")
                },
            ],
            terrain: ["plain", "forest", "river", "bridge", "road"]
                .iter()
                .map(|&id| BaseTerrain {
                    id: id.into(),
                    tile: id.into(),
                })
                .collect(),
            sprites: CLASS_SPRITES.iter().map(|s| s.to_string()).collect(),
            ..PackOptions::default()
        }
    }

    #[test]
    fn builds_in_memory_the_files_it_writes() {
        let src = TempDir::new("pack-mem-src");
        write_pack_install(src.path());
        let out = TempDir::new("pack-mem-out");
        let dir = out.path().join("original");
        let written = write_pack(src.path(), &dir, &options()).unwrap();
        let listing = |p: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(p)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let (install_before, out_before) = (listing(src.path()), listing(out.path()));
        let built = build_pack(src.path(), &options()).unwrap();
        // Nothing is written anywhere.
        assert_eq!(listing(src.path()), install_before);
        assert_eq!(listing(out.path()), out_before);
        assert!(built.index.success(), "{:#?}", built.index.assets);
        assert_eq!(built.index.files, written.files);
        let mut listed = written.files.clone();
        listed.push(PACK_INDEX.to_string());
        listed.sort();
        assert_eq!(built.files.keys().cloned().collect::<Vec<_>>(), listed);
        for (rel, bytes) in &built.files {
            assert_eq!(&std::fs::read(dir.join(rel)).unwrap(), bytes, "{rel}");
        }

        // An install that cannot be converted is refused before anything is converted.
        let empty = TempDir::new("pack-mem-empty");
        assert!(matches!(
            build_pack(empty.path(), &options()),
            Err(ExtractError::NotExtractable(_))
        ));
    }

    #[test]
    fn writes_a_pack_from_a_synthetic_install() {
        let src = TempDir::new("pack-src");
        write_pack_install(src.path());
        let out = TempDir::new("pack-out");
        let pack = out.path().join("original");
        let index = write_pack(src.path(), &pack, &options()).unwrap();
        assert!(index.success(), "{:#?}", index.assets);
        assert_eq!(
            index.portraits,
            vec![PortraitMatch {
                officer: "liu_bei".into(),
                bakdata: 0,
                portrait: 0
            }]
        );
        assert_eq!(index.unmatched_officers.len(), 2);
        assert!(
            index.unmatched_officers[1]
                .reason
                .contains("not a plain file name"),
            "{:?}",
            index.unmatched_officers
        );
        assert!(!out.path().join("evil.png").exists());
        for f in [
            "pack.toml",
            "gfx/portraits/liu_bei.png",
            "gfx/units/units.toml",
            "gfx/units/supply_ally.png",
            "gfx/units/short_infantry_enemy.png",
            "gfx/tiles/terrain.png",
            "gfx/tiles/terrain.toml",
            "gfx/maps/hexz_00.png",
            "gfx/maps/hexz_01.png",
            "maps/original.toml",
        ] {
            assert!(pack.join(f).is_file(), "{f}");
            assert!(
                f == "pack.toml" || index.files.contains(&f.to_string()),
                "{f}"
            );
        }
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(pack.join(PACK_INDEX)).unwrap()).unwrap();
        assert_eq!(json["format"], PACK_FORMAT);

        // Both maps, with their names, sizes and chip sets; the pictures are 16 px per chip.
        let summary: Vec<(&str, &str, [usize; 2], usize)> = index
            .maps
            .iter()
            .map(|m| (m.id.as_str(), m.name.as_str(), m.cells, m.chip_set))
            .collect();
        assert_eq!(
            summary,
            [("hexz_00", "가", [3, 2], 1), ("hexz_01", "각", [2, 2], 2)]
        );
        let png = std::fs::read(pack.join("gfx/maps/hexz_00.png")).unwrap();
        assert_eq!(&png[16..24], &[0, 0, 0, 96, 0, 0, 0, 64]);
        let manifest = std::fs::read_to_string(pack.join("pack.toml")).unwrap();
        assert!(
            manifest.contains("maps = [\"maps/original.toml\"]"),
            "{manifest}"
        );
        let maps = std::fs::read_to_string(pack.join(MAPS_FILE)).unwrap();
        assert!(
            maps.contains("[[map]]\nid = \"hexz_00\"\nname = \"가\"\nimage = \"hexz_00\"\nlegend = { \"0\" = \"plain\", \"1\" = \"forest\", \"3\" = \"river\", \"4\" = \"bridge\" }\nrows = '''\n001\n343\n'''\n"),
            "{maps}"
        );
        assert_eq!(index.assets["maps"].status, Status::Extracted);

        // A second run replaces the files; a pack folder that is not ours is refused.
        std::fs::write(pack.join("gfx/portraits/stale.png"), b"x").unwrap();
        write_pack(src.path(), &pack, &options()).unwrap();
        assert!(
            pack.join("gfx/portraits/stale.png").is_file(),
            "unlisted files are kept"
        );
        let foreign = out.path().join("base");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("pack.toml"), b"id = \"base\"").unwrap();
        let err = write_pack(src.path(), &foreign, &options()).unwrap_err();
        assert!(matches!(err, ExtractError::OutputNotEmpty(_)), "{err}");
    }

    #[test]
    fn malformed_battle_maps_are_reported() {
        let src = TempDir::new("pack-src-badmap");
        write_pack_install(src.path());
        let good = map(&[&[0, 0], &[3, 4]]).encode();
        let names = b"\xb0\xa1\r\n\xb0\xa2\r\n\r\n\x1a".to_vec();
        std::fs::write(
            src.path().join("HEXZMAP.R3"),
            ls11::build(&[&good, &[4, 4, 1, 2, 3], &names]),
        )
        .unwrap();
        let out = TempDir::new("pack-out-badmap");
        let index = write_pack(src.path(), out.path(), &options()).unwrap();
        let tiles = &index.assets["tiles"];
        assert_eq!(tiles.status, Status::Partial, "{tiles:#?}");
        assert!(tiles.errors[0].contains("HEXZMAP.R3 entry 1"), "{tiles:#?}");
        assert!(!index.success());
        // The tileset from the readable maps is still written, and the unit sheets and the
        // readable map with it.
        assert!(out.path().join("gfx/tiles/terrain.toml").is_file());
        assert_eq!(index.assets["units"].status, Status::Extracted);
        let maps = &index.assets["maps"];
        assert_eq!(maps.status, Status::Partial, "{maps:#?}");
        assert_eq!(index.maps.len(), 1);
        assert!(out.path().join("maps/original.toml").is_file());

        // No readable map at all: nothing is learned.
        std::fs::write(src.path().join("HEXZMAP.R3"), ls11::build(&[&names])).unwrap();
        let out = TempDir::new("pack-out-nomap");
        let index = write_pack(src.path(), out.path(), &options()).unwrap();
        assert_eq!(index.assets["tiles"].status, Status::Failed);
        assert!(!out.path().join("gfx/tiles/terrain.toml").exists());
        // No tileset, no maps (their pictures are sized for its 32-px tiles).
        assert_eq!(index.assets["maps"].status, Status::Failed);
        assert!(index.maps.is_empty());
        let manifest = std::fs::read_to_string(out.path().join("pack.toml")).unwrap();
        assert!(!manifest.contains("maps"), "{manifest}");
    }

    #[test]
    fn classes_the_original_lacks_stop_the_unit_sheets() {
        let src = TempDir::new("pack-src-classes");
        write_pack_install(src.path());
        let out = TempDir::new("pack-out-classes");
        let mut opts = options();
        opts.sprites.push("dragon".into());
        let index = write_pack(src.path(), out.path(), &opts).unwrap();
        let units = &index.assets["units"];
        assert_eq!(units.status, Status::Failed);
        assert!(units.errors[0].contains("dragon"), "{units:#?}");
        assert!(!out.path().join("gfx/units/units.toml").exists());
        assert!(!index.success());
        assert_eq!(index.assets["tiles"].status, Status::Extracted);
    }

    #[test]
    fn no_palette_no_art() {
        let src = TempDir::new("pack-src-nopal");
        write_pack_install(src.path());
        std::fs::write(src.path().join("MAIN.EXE"), b"MZ not a game").unwrap();
        let out = TempDir::new("pack-out-nopal");
        let index = write_pack(src.path(), out.path(), &options()).unwrap();
        for kind in ["portraits", "tiles", "units", "maps"] {
            assert_eq!(index.assets[kind].status, Status::Failed, "{kind}");
        }
        assert!(out.path().join("pack.toml").is_file());
        assert!(!out.path().join("gfx").exists());
    }
}
