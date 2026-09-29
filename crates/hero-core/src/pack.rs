//! Data packs: a directory with `pack.toml` plus rules, officers, battles, dramas and media.
//!
//! A pack may extend another pack (`extends = "../base"`) and then lists only the files it
//! provides; see [`PackChain`] for the layering rules.
//!
//! Loading is split in two so the web build can fetch files asynchronously:
//! 1. [`PackChain::new`] parses the top pack's `pack.toml`; while [`PackChain::next_parent`] names
//!    a parent `pack.toml`, the frontend reads it and hands it to [`PackChain::push_parent`].
//!    [`PackChain::text_files`] then lists every other text file of the chain as
//!    `(pack directory, path)` pairs.
//! 2. The frontend reads those files (fetch on web, `std::fs` natively) into a [`FileSource`],
//!    keyed by their path relative to the top pack ([`PackFile::source_path`]) together with the
//!    `pack.toml` files, and calls [`Pack::load`], which parses everything synchronously.
//!
//! [`Pack::validate`] then cross-checks every reference; tools additionally run
//! [`Pack::unknown_fields`] (typos in TOML keys) and, natively, [`Pack::missing_media`].
//! The file format is documented for modders in `docs/MODDING.md`.

mod chain;
#[cfg(not(target_arch = "wasm32"))]
mod cmap;
mod lint;
#[cfg(not(target_arch = "wasm32"))]
mod media;
mod validate;

pub use chain::{join_path, PackChain, PackFile, PackFiles, PackLayer, MAX_CHAIN_DEPTH};

/// The fonts the game draws text with, as pack-relative paths: the main font (UI and dialogue)
/// and the small one (numbers and captions).
pub const FONT_FILES: [&str; 2] = ["fonts/Galmuri11.ttf", "fonts/Galmuri9.ttf"];

use crate::battledef::{BattleDef, MapEntry};
use crate::campaign::CampaignDef;
use crate::data::{ClassDef, GameRules, Id, ItemDef, OfficerDef, StrategyDef, TerrainDef};
use crate::map::BattleMap;
use crate::script::Scene;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The `[rules]` table of `pack.toml`. A pack that extends another may leave out any of them
/// (the parent's file is used); a pack without `extends` must list all five.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RulesFiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategies: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<String>,
}

/// Default virtual canvas size in pixels (the base pack's 16 px tiles: 30 × 17 visible tiles).
pub const DEFAULT_CANVAS: [u32; 2] = [480, 270];
/// Smallest virtual canvas a pack may ask for, `[width, height]`: the smallest the camp and
/// battle screens are laid out for (`docs/DECISIONS.md` D13).
pub const MIN_CANVAS: [u32; 2] = [480, 270];
/// Largest virtual canvas a pack may ask for, `[width, height]`.
pub const MAX_CANVAS: [u32; 2] = [1280, 800];

fn default_canvas() -> [u32; 2] {
    DEFAULT_CANVAS
}

/// `[presentation]` of `pack.toml`: how the frontend lays the pack's media out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presentation {
    /// Size of the virtual canvas the game renders to, `[width, height]` in pixels, within
    /// [`MIN_CANVAS`]..=[`MAX_CANVAS`]. Default [`DEFAULT_CANVAS`].
    #[serde(default = "default_canvas")]
    pub canvas: [u32; 2],
    /// A picture the battle screen is drawn in; without one the battle screen lays itself out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battle_frame: Option<BattleFrame>,
    /// A picture the camp screens are drawn in; without one they fill the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camp_frame: Option<CampFrame>,
    /// A picture the camp's officer list (무장 정보) is drawn on; without one it is a table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_frame: Option<StatusFrame>,
}

impl Default for Presentation {
    fn default() -> Self {
        Presentation {
            canvas: DEFAULT_CANVAS,
            battle_frame: None,
            camp_frame: None,
            status_frame: None,
        }
    }
}

/// `[presentation.battle_frame]`: a picture of the whole canvas that the battle screen is drawn
/// in (the original's battle screen), and where its parts go, as `[x, y, width, height]` in
/// canvas pixels. The map shows through `map`; the rest of the picture is drawn over it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleFrame {
    /// Media key of the picture, `gfx/<image>.png`, the size of the canvas.
    pub image: String,
    /// The map.
    pub map: [u32; 4],
    /// Where the unit, terrain and forecast windows go, one under the other.
    pub info: [u32; 4],
    /// Where the battle's name, the turn and the phase go.
    pub title: [u32; 4],
    /// Where the weather and the gold go.
    pub status: [u32; 4],
    /// Buttons drawn on the picture that the player can tap on the player's turn: the battle
    /// menu, the player's unit list and the enemy's unit list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<[u32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allies: Option<[u32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enemies: Option<[u32; 4]>,
    /// Where a picture of the weather goes (its icon, centred).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<[u32; 4]>,
}

impl BattleFrame {
    /// Why the frame does not fit a `canvas` sized canvas, if it does not.
    pub fn check(&self, canvas: [u32; 2]) -> Result<(), String> {
        let mut areas = vec![
            ("map", self.map),
            ("info", self.info),
            ("title", self.title),
            ("status", self.status),
        ];
        for (name, area) in [
            ("menu", self.menu),
            ("allies", self.allies),
            ("enemies", self.enemies),
            ("weather", self.weather),
        ] {
            areas.extend(area.map(|a| (name, a)));
        }
        // A button on the map would take the taps of its tiles.
        let overlaps = |[ax, ay, aw, ah]: [u32; 4], [bx, by, bw, bh]: [u32; 4]| {
            ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
        };
        for (name, area) in [
            ("menu", self.menu),
            ("allies", self.allies),
            ("enemies", self.enemies),
        ] {
            if let Some(a) = area.filter(|&a| overlaps(a, self.map)) {
                return Err(format!(
                    "battle_frame.{name} {a:?} must not overlap battle_frame.map {:?}",
                    self.map
                ));
            }
        }
        check_frame("battle_frame", &self.image, &areas, canvas, "canvas")
    }
}

/// `[presentation.camp_frame]`: a picture of the whole canvas that the camp screens are drawn in
/// (the original's main screen), and where its parts go, as `[x, y, width, height]` in canvas
/// pixels. The camp screens are laid out in `view` as if it were the whole canvas (so it must be
/// at least [`MIN_CANVAS`]); the rest of the picture is drawn over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampFrame {
    /// Media key of the picture, `gfx/<image>.png`, the size of the canvas.
    pub image: String,
    /// Where the camp screens go.
    pub view: [u32; 4],
    /// The portrait of the army's leader (the first officer of the roster).
    pub portrait: [u32; 4],
    /// The army's gold.
    pub gold: [u32; 4],
    /// The leader's level.
    pub level: [u32; 4],
    /// Where the army is: the camp's heading up to ` — `.
    pub place: [u32; 4],
    /// The camp's whole heading.
    pub caption: [u32; 4],
    /// The play time.
    pub clock: [u32; 4],
}

impl CampFrame {
    /// Why the frame does not fit a `canvas` sized canvas, if it does not.
    pub fn check(&self, canvas: [u32; 2]) -> Result<(), String> {
        check_frame(
            "camp_frame",
            &self.image,
            &[
                ("view", self.view),
                ("portrait", self.portrait),
                ("gold", self.gold),
                ("level", self.level),
                ("place", self.place),
                ("caption", self.caption),
                ("clock", self.clock),
            ],
            canvas,
            "canvas",
        )?;
        let [_, _, w, h] = self.view;
        let [min_w, min_h] = MIN_CANVAS;
        if w < min_w || h < min_h {
            return Err(format!(
                "camp_frame.view is {w}×{h}; the camp screens need at least {min_w}×{min_h}"
            ));
        }
        Ok(())
    }
}

/// `[presentation.status_frame]`: a picture the camp's officer list (무장 정보) is drawn on (the
/// original's status window), centred in the camp screens' area, and where its parts go, as
/// `[x, y, width, height]` in the picture's pixels. The officers are shown a page at a time, one
/// per slot; the one chosen is shown in full on the side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusFrame {
    /// Media key of the picture, `gfx/<image>.png`.
    pub image: String,
    /// Size of the picture, `[width, height]`: at most the canvas.
    pub size: [u32; 2],
    /// The window's heading.
    pub title: [u32; 4],
    /// The officers of a page, in order (row by row).
    pub slots: Vec<StatusSlot>,
    /// The chosen officer's portrait.
    pub portrait: [u32; 4],
    /// The chosen officer's name.
    pub name: [u32; 4],
    /// The chosen officer's level.
    pub level: [u32; 4],
    /// The chosen officer's 통솔, 무력 and 지력.
    pub lead: [u32; 4],
    pub strength: [u32; 4],
    pub intellect: [u32; 4],
    /// The chosen officer's class.
    pub class: [u32; 4],
    /// The chosen officer's equipment and strategies.
    pub info: [u32; 4],
    /// The page number.
    pub page: [u32; 4],
    /// The page buttons: the previous page in the top half, the next in the bottom half.
    pub pager: [u32; 4],
    /// How many officers the pages after this one hold.
    pub rest: [u32; 4],
    /// The button that closes the window.
    pub close: [u32; 4],
}

/// One officer's place on a [`StatusFrame`] page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSlot {
    /// The officer's unit icon.
    pub icon: [u32; 4],
    /// The officer's level.
    pub level: [u32; 4],
    /// The officer's troops.
    pub troops: [u32; 4],
}

impl StatusFrame {
    /// Why the frame does not fit a `canvas` sized canvas, if it does not.
    pub fn check(&self, canvas: [u32; 2]) -> Result<(), String> {
        let [w, h] = self.size;
        let [cw, ch] = canvas;
        if w == 0 || h == 0 || w > cw || h > ch {
            return Err(format!(
                "status_frame.size [{w}, {h}] must be a picture no larger than the [{cw}, {ch}] canvas"
            ));
        }
        if self.slots.is_empty() {
            return Err("status_frame.slots must list at least one slot".into());
        }
        let mut areas = vec![
            ("title", self.title),
            ("portrait", self.portrait),
            ("name", self.name),
            ("level", self.level),
            ("lead", self.lead),
            ("strength", self.strength),
            ("intellect", self.intellect),
            ("class", self.class),
            ("info", self.info),
            ("page", self.page),
            ("pager", self.pager),
            ("rest", self.rest),
            ("close", self.close),
        ];
        for s in &self.slots {
            areas.extend([
                ("slots.icon", s.icon),
                ("slots.level", s.level),
                ("slots.troops", s.troops),
            ]);
        }
        check_frame("status_frame", &self.image, &areas, self.size, "picture")
    }
}

/// Why a frame `what` with picture `image` and `areas` does not fit a `bounds` sized `within`
/// (the canvas, or the picture itself).
fn check_frame(
    what: &str,
    image: &str,
    areas: &[(&str, [u32; 4])],
    bounds: [u32; 2],
    within: &str,
) -> Result<(), String> {
    if !validate::is_media_key(image) {
        return Err(format!("{what}.image `{image}` is not a media key"));
    }
    let [cw, ch] = bounds;
    for &(name, [x, y, w, h]) in areas {
        let inside = u64::from(x) + u64::from(w) <= u64::from(cw)
            && u64::from(y) + u64::from(h) <= u64::from(ch);
        if w == 0 || h == 0 || !inside {
            return Err(format!(
                "{what}.{name} [{x}, {y}, {w}, {h}] must be a non-empty area inside the \
                 [{cw}, {ch}] {within}"
            ));
        }
    }
    Ok(())
}

/// `pack.toml`. All paths are relative to the pack directory and use `/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackManifest {
    /// Machine id, e.g. `base`. Save games remember the id of the pack that was loaded.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    /// SPDX-style license summary of the pack content.
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub description: String,
    /// Directory of the pack this one is built on, relative to this pack's directory
    /// (`../base`). See [`PackChain`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Canvas size and other presentation settings. In a loaded [`Pack`] this is the
    /// effective value: the pack's own `[presentation]`, or the one it inherits
    /// ([`PackChain::presentation`]).
    #[serde(default)]
    pub presentation: Presentation,
    #[serde(default)]
    pub rules: RulesFiles,
    /// Officer list; may be left out by a pack that extends another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub officers: Option<String>,
    /// Campaign graph; may be left out by a pack that extends another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign: Option<String>,
    /// Battle definition files (added to the parent's battles).
    #[serde(default)]
    pub battles: Vec<String>,
    /// Drama script files (added to the parent's scenes).
    #[serde(default)]
    pub dramas: Vec<String>,
    /// Map files: `[[map]]` tables that battles `use` by id (added to the parent's maps).
    #[serde(default)]
    pub maps: Vec<String>,
}

impl PackManifest {
    /// Parse the text of a `pack.toml`. Only the TOML shape is checked here; [`PackChain`]
    /// (used by [`Pack::load`]) also checks paths, `extends` and `[presentation]`.
    pub fn parse(src: &str) -> Result<PackManifest, PackError> {
        PackManifest::parse_file(MANIFEST_FILE, src)
    }

    /// [`PackManifest::parse`] with `file` as the name in error messages.
    fn parse_file(file: &str, src: &str) -> Result<PackManifest, PackError> {
        parse_toml(file, src.strip_prefix('\u{feff}').unwrap_or(src))
    }

    /// Every text file (path inside this pack) this `pack.toml` itself lists, excluding
    /// `pack.toml`. For a pack that extends another, [`PackChain::text_files`] lists the files
    /// of the whole chain.
    pub fn text_files(&self) -> Vec<String> {
        let r = &self.rules;
        let mut v: Vec<String> = [
            &r.game,
            &r.terrain,
            &r.classes,
            &r.strategies,
            &r.items,
            &self.officers,
            &self.campaign,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        v.extend(self.battles.iter().cloned());
        v.extend(self.dramas.iter().cloned());
        v.extend(self.maps.iter().cloned());
        v
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    #[error("{file}: file not found")]
    Missing { file: String },
    #[error("{file}: {msg}")]
    Parse { file: String, msg: String },
    #[error("{0}")]
    Invalid(String),
}

/// Something that can provide the text content of pack files.
pub trait FileSource {
    /// Text of the file at `path`, relative to the top pack directory and `/`-separated. Files
    /// of the packs it extends have paths such as `../base/rules/game.toml`
    /// ([`PackFile::source_path`]). A file that does not exist is [`PackError::Missing`].
    fn read_text(&self, path: &str) -> Result<String, PackError>;
}

impl FileSource for BTreeMap<String, String> {
    fn read_text(&self, path: &str) -> Result<String, PackError> {
        self.get(path)
            .cloned()
            .ok_or_else(|| PackError::Missing { file: path.into() })
    }
}

/// Reads pack files from a directory on disk (native builds and tools). `root` is the top pack
/// directory; the files of packs it extends are read through `..` paths below it.
pub struct DirSource {
    pub root: std::path::PathBuf,
}

impl FileSource for DirSource {
    fn read_text(&self, path: &str) -> Result<String, PackError> {
        std::fs::read_to_string(self.root.join(path)).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PackError::Missing { file: path.into() },
            _ => PackError::Parse {
                file: path.into(),
                msg: e.to_string(),
            },
        })
    }
}

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    /// File or entity the issue is about, e.g. `battles/b01.toml` or `class light_infantry`.
    pub context: String,
    pub msg: String,
}

/// A fully parsed data pack (for a layered pack: the merged result of its chain).
#[derive(Debug, Clone)]
pub struct Pack {
    /// The top pack's `pack.toml`, with the effective `presentation` (declared or inherited).
    pub manifest: PackManifest,
    /// The packs this pack is built from, the top pack first; a pack without `extends` has
    /// exactly one layer, with an empty `dir`. Media files are looked up in every layer's
    /// directory, in this order.
    pub layers: Vec<PackLayer>,
    /// The text file each part was read from (paths relative to the top pack directory via
    /// [`PackFile::source_path`]).
    pub files: PackFiles,
    pub rules: GameRules,
    /// Terrain in file order (order matters only for display).
    pub terrain: Vec<TerrainDef>,
    pub classes: BTreeMap<Id, ClassDef>,
    pub strategies: BTreeMap<Id, StrategyDef>,
    pub items: BTreeMap<Id, ItemDef>,
    pub officers: BTreeMap<Id, OfficerDef>,
    /// Battles; the map of a battle that `use`s a map file entry is already filled in.
    pub battles: BTreeMap<Id, BattleDef>,
    /// Maps of the map files by id, whether a battle uses them or not.
    pub maps: BTreeMap<Id, MapEntry>,
    /// Drama scenes by globally unique scene id.
    pub scenes: BTreeMap<String, Scene>,
    /// Scenes whose kept definition comes from a pack this one extends (not from the top pack).
    pub parent_scenes: BTreeSet<String>,
    pub campaign: CampaignDef,
}

/// Name of the manifest file at the root of every pack.
const MANIFEST_FILE: &str = "pack.toml";

// Top-level layout of the list-style rules files. Each file holds an array of tables named
// after the entity (`[[terrain]]`, `[[class]]`, ...); unknown top-level keys are rejected so
// that a misspelt table name (`[[classes]]`) fails loudly instead of loading nothing.

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainFile {
    #[serde(default)]
    terrain: Vec<TerrainDef>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassesFile {
    #[serde(default)]
    class: Vec<ClassDef>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategiesFile {
    #[serde(default)]
    strategy: Vec<StrategyDef>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemsFile {
    #[serde(default)]
    item: Vec<ItemDef>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficersFile {
    #[serde(default)]
    officer: Vec<OfficerDef>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapsFile {
    #[serde(default)]
    map: Vec<MapEntry>,
}

/// Read a pack file, dropping a UTF-8 byte order mark (Windows editors like to add one).
fn read(src: &dyn FileSource, file: &str) -> Result<String, PackError> {
    let text = src.read_text(file)?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text,
    })
}

fn parse_toml<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, PackError> {
    toml::from_str(text).map_err(|e| PackError::Parse {
        file: file.to_string(),
        msg: e.to_string().trim_end().to_string(),
    })
}

fn parse_error(file: &str, msg: impl Into<String>) -> PackError {
    PackError::Parse {
        file: file.to_string(),
        msg: msg.into(),
    }
}

/// Key a list of definitions by id, rejecting empty and duplicate ids.
fn index_by_id<T>(
    file: &str,
    what: &str,
    defs: Vec<T>,
    id: impl Fn(&T) -> &Id,
) -> Result<BTreeMap<Id, T>, PackError> {
    let mut map = BTreeMap::new();
    for def in defs {
        let key = id(&def).clone();
        if key.trim().is_empty() {
            return Err(parse_error(file, format!("{what} with an empty id")));
        }
        if map.contains_key(&key) {
            return Err(parse_error(file, format!("duplicate {what} id `{key}`")));
        }
        map.insert(key, def);
    }
    Ok(map)
}

/// Whether `path` is a valid file path inside a pack: relative, `/`-separated, without `..`.
fn is_pack_path(path: &str) -> bool {
    !(path.trim().is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|part| part == ".."))
}

impl PackManifest {
    /// Checks of one manifest (`file` names it in errors): every listed path is relative, uses
    /// `/` and stays inside the pack, no file is listed twice (each file has exactly one role),
    /// `extends` is a relative `/`-separated directory, and the canvas is within
    /// [`MIN_CANVAS`]..=[`MAX_CANVAS`].
    fn check(&self, file: &str) -> Result<(), PackError> {
        let mut seen = BTreeSet::new();
        for path in self.text_files() {
            if !is_pack_path(&path) {
                return Err(parse_error(
                    file,
                    format!("path `{path}` must be relative to the pack directory, use `/` and not contain `..`"),
                ));
            }
            if !seen.insert(path.clone()) {
                return Err(parse_error(
                    file,
                    format!("file `{path}` is listed more than once"),
                ));
            }
        }
        if let Some(parent) = &self.extends {
            let bad = parent.trim().is_empty()
                || parent.starts_with('/')
                || parent.contains('\\')
                || parent.contains(':');
            if bad {
                return Err(parse_error(
                    file,
                    format!("extends `{parent}` must be a directory relative to this pack, using `/` (for example `../base`)"),
                ));
            }
        }
        let [w, h] = self.presentation.canvas;
        let ([min_w, min_h], [max_w, max_h]) = (MIN_CANVAS, MAX_CANVAS);
        if !(min_w..=max_w).contains(&w) || !(min_h..=max_h).contains(&h) {
            return Err(parse_error(
                file,
                format!("presentation.canvas [{w}, {h}] must be between [{min_w}, {min_h}] and [{max_w}, {max_h}]"),
            ));
        }
        let images = [
            (
                "battle_frame",
                self.presentation.battle_frame.as_ref().map(|f| &f.image),
            ),
            (
                "camp_frame",
                self.presentation.camp_frame.as_ref().map(|f| &f.image),
            ),
            (
                "status_frame",
                self.presentation.status_frame.as_ref().map(|f| &f.image),
            ),
        ];
        for (what, image) in images {
            if let Some(image) = image.filter(|i| !validate::is_media_key(i)) {
                return Err(parse_error(
                    file,
                    format!("presentation.{what}.image `{image}` is not a media key"),
                ));
            }
        }
        Ok(())
    }
}

/// Parse drama and battle files of every layer, farthest parent first. Within one pack an id
/// must be unique; an id that a later (nearer) pack defines again replaces the earlier one.
/// `parse` turns one file into its `(id, value)` entries. Returns the entries and, by id, the
/// directory of the pack whose definition was kept ([`PackFile::dir`]).
type Merged<T> = (BTreeMap<String, T>, BTreeMap<String, String>);

fn merge_layers<T>(
    files: &[PackFile],
    what: &str,
    mut parse: impl FnMut(&str) -> Result<Vec<(String, T)>, PackError>,
) -> Result<Merged<T>, PackError> {
    let mut merged: BTreeMap<String, T> = BTreeMap::new();
    // Id -> (pack directory, file) of its current definition.
    let mut defined: BTreeMap<String, (&str, String)> = BTreeMap::new();
    for file in files {
        let path = file.source_path();
        for (id, value) in parse(&path)? {
            if let Some((dir, first)) = defined.get(&id) {
                if *dir == file.dir {
                    return Err(parse_error(
                        &path,
                        format!("duplicate {what} id `{id}` (first defined in {first})"),
                    ));
                }
            }
            defined.insert(id.clone(), (&file.dir, path.clone()));
            merged.insert(id, value);
        }
    }
    let origins = defined
        .into_iter()
        .map(|(id, (dir, _))| (id, dir.to_string()))
        .collect();
    Ok((merged, origins))
}

impl Pack {
    /// Parse `pack.toml`, the packs it extends and every file of the chain (see [`PackChain`]),
    /// all read through `src` by their path relative to the top pack directory. Structural
    /// errors (bad TOML, duplicate ids, unparsable maps or dramas, broken chains) fail here;
    /// cross-reference problems are reported by [`Pack::validate`].
    pub fn load(src: &dyn FileSource) -> Result<Pack, PackError> {
        let chain = PackChain::read(src)?;
        let files = chain.resolve()?;
        let text = |f: &PackFile| -> Result<(String, String), PackError> {
            let path = f.source_path();
            let content = read(src, &path)?;
            Ok((path, content))
        };

        let (path, content) = text(&files.game)?;
        let rules: GameRules = parse_toml(&path, &content)?;

        let (terrain_file, content) = text(&files.terrain)?;
        let terrain = parse_toml::<TerrainFile>(&terrain_file, &content)?.terrain;
        let mut terrain_ids = BTreeSet::new();
        let mut glyphs = BTreeMap::new();
        for t in &terrain {
            if t.id.trim().is_empty() {
                return Err(parse_error(&terrain_file, "terrain with an empty id"));
            }
            if !terrain_ids.insert(t.id.as_str()) {
                return Err(parse_error(
                    &terrain_file,
                    format!("duplicate terrain id `{}`", t.id),
                ));
            }
            if let Some(other) = glyphs.insert(t.glyph, t.id.as_str()) {
                return Err(parse_error(
                    &terrain_file,
                    format!(
                        "terrain `{}` reuses glyph {:?} of terrain `{other}`",
                        t.id, t.glyph
                    ),
                ));
            }
        }

        let (path, content) = text(&files.classes)?;
        let classes = parse_toml::<ClassesFile>(&path, &content)?.class;
        let classes = index_by_id(&path, "class", classes, |c| &c.id)?;
        let (path, content) = text(&files.strategies)?;
        let strategies = parse_toml::<StrategiesFile>(&path, &content)?.strategy;
        let strategies = index_by_id(&path, "strategy", strategies, |s| &s.id)?;
        let (path, content) = text(&files.items)?;
        let items = parse_toml::<ItemsFile>(&path, &content)?.item;
        let items = index_by_id(&path, "item", items, |i| &i.id)?;
        let (path, content) = text(&files.officers)?;
        let officers = parse_toml::<OfficersFile>(&path, &content)?.officer;
        let officers = index_by_id(&path, "officer", officers, |o| &o.id)?;

        let (path, content) = text(&files.campaign)?;
        let campaign: CampaignDef = parse_toml(&path, &content)?;
        let mut node_ids = BTreeSet::new();
        for node in &campaign.nodes {
            if node.id().trim().is_empty() {
                return Err(parse_error(&path, "campaign node with an empty id"));
            }
            if !node_ids.insert(node.id()) {
                return Err(parse_error(
                    &path,
                    format!("duplicate campaign node id `{}`", node.id()),
                ));
            }
        }

        // Maps and battles are parsed as TOML first and their grids checked only once overrides
        // are resolved: a parent's map or battle that the child replaces never meets the
        // child's terrain.
        let one_char_legend = |path: &str, legend: &BTreeMap<String, Id>| match legend
            .keys()
            .find(|k| k.chars().count() != 1)
        {
            Some(key) => Err(parse_error(
                path,
                format!("map legend key `{key}` must be exactly one character"),
            )),
            None => Ok(()),
        };
        let mut map_files: BTreeMap<Id, String> = BTreeMap::new();
        let (maps, _) = merge_layers(&files.maps, "map", |path| {
            let file: MapsFile = parse_toml(path, &read(src, path)?)?;
            let mut entries = Vec::new();
            for map in file.map {
                if map.id.trim().is_empty() {
                    return Err(parse_error(path, "map with an empty id"));
                }
                one_char_legend(path, &map.legend)?;
                map_files.insert(map.id.clone(), path.to_string());
                entries.push((map.id.clone(), map));
            }
            Ok(entries)
        })?;
        for (id, map) in &maps {
            BattleMap::parse(&map.rows, &map.legend, &terrain)
                .map_err(|e| parse_error(&map_files[id], format!("map `{id}`: {e}")))?;
        }

        let mut battle_files: BTreeMap<Id, String> = BTreeMap::new();
        let (mut battles, _) = merge_layers(&files.battles, "battle", |path| {
            let battle: BattleDef = parse_toml(path, &read(src, path)?)?;
            if battle.id.trim().is_empty() {
                return Err(parse_error(path, "battle with an empty id"));
            }
            one_char_legend(path, &battle.map.legend)?;
            if battle.map.use_map.is_some() && battle.map.has_own_content() {
                return Err(parse_error(
                    path,
                    format!(
                        "battle `{}` map: `use` takes the whole map from the map file; remove `rows`, `legend`, `theme` and `image`",
                        battle.id
                    ),
                ));
            }
            battle_files.insert(battle.id.clone(), path.to_string());
            Ok(vec![(battle.id.clone(), battle)])
        })?;
        for (id, battle) in battles.iter_mut() {
            let file = &battle_files[id];
            if let Some(map_id) = battle.map.use_map.clone() {
                // Resolved after every layer is merged, so a child's map replaces the parent's
                // for the parent's battles too (a mod can redraw a map without copying battles).
                let entry = maps.get(&map_id).ok_or_else(|| {
                    parse_error(
                        file,
                        format!("battle `{id}` map: uses unknown map `{map_id}` (no map file of the pack defines it)"),
                    )
                })?;
                battle.map = entry.to_def();
            }
            BattleMap::parse(&battle.map.rows, &battle.map.legend, &terrain)
                .map_err(|e| parse_error(file, format!("battle `{id}` map: {e}")))?;
        }

        let (scenes, scene_dirs) = merge_layers(&files.dramas, "scene", |path| {
            let parsed = crate::script::parse_drama(path, &read(src, path)?)
                .map_err(|e| parse_error(&e.file, format!("line {}: {}", e.line, e.msg)))?;
            Ok(parsed.into_iter().map(|s| (s.id.clone(), s)).collect())
        })?;

        let mut manifest = chain.layers()[0].manifest.clone();
        manifest.presentation = chain.presentation();
        let presentation = &manifest.presentation;
        let checks = [
            (
                "battle frame",
                chain.battle_frame_dir(),
                presentation
                    .battle_frame
                    .as_ref()
                    .map(|f| f.check(presentation.canvas)),
            ),
            (
                "camp frame",
                chain.camp_frame_dir(),
                presentation
                    .camp_frame
                    .as_ref()
                    .map(|f| f.check(presentation.canvas)),
            ),
            (
                "status frame",
                chain.status_frame_dir(),
                presentation
                    .status_frame
                    .as_ref()
                    .map(|f| f.check(presentation.canvas)),
            ),
        ];
        for (what, from, checked) in checks {
            // The frames and the canvas may come from different layers: a pack that changes
            // the canvas of a pack with a frame declares its own frame (one cannot be removed).
            let Some(Err(e)) = checked else {
                continue;
            };
            let top = &chain.layers()[0].dir;
            let from = from.unwrap_or(top);
            let inherited = if from == top {
                String::new()
            } else {
                format!(
                    " (the {what} of `{}`; declare one that fits this pack's canvas)",
                    chain::join_path(from, MANIFEST_FILE)
                )
            };
            return Err(parse_error(
                &chain::join_path(top, MANIFEST_FILE),
                format!("presentation.{e}{inherited}"),
            ));
        }
        let top = &chain.layers()[0].dir;
        let parent_scenes = scene_dirs
            .into_iter()
            .filter(|(_, dir)| dir != top)
            .map(|(id, _)| id)
            .collect();
        Ok(Pack {
            manifest,
            layers: chain.layers().to_vec(),
            files,
            rules,
            terrain,
            classes,
            strategies,
            items,
            officers,
            battles,
            maps,
            scenes,
            parent_scenes,
            campaign,
        })
    }

    /// Cross-check every reference in the pack (class/item/strategy/officer/terrain/scene/battle
    /// ids, promotion chains, deploy slots inside the map and on passable tiles, unit positions,
    /// unique unit tags, reachable campaign nodes, ...). Returns all findings; the pack is usable
    /// when no `Severity::Error` is present.
    pub fn validate(&self) -> Vec<Issue> {
        validate::validate(self)
    }

    pub fn terrain(&self, id: &str) -> Option<&TerrainDef> {
        self.terrain.iter().find(|t| t.id == id)
    }

    pub fn class(&self, id: &str) -> Option<&ClassDef> {
        self.classes.get(id)
    }

    pub fn item(&self, id: &str) -> Option<&ItemDef> {
        self.items.get(id)
    }

    pub fn strategy(&self, id: &str) -> Option<&StrategyDef> {
        self.strategies.get(id)
    }

    pub fn officer(&self, id: &str) -> Option<&OfficerDef> {
        self.officers.get(id)
    }

    pub fn scene(&self, id: &str) -> Option<&Scene> {
        self.scenes.get(id)
    }

    /// Classes from the base class up to `class`, following `promote.to` links backwards.
    pub fn class_chain(&self, class: &str) -> Vec<&ClassDef> {
        let mut chain: Vec<&ClassDef> = Vec::new();
        let mut cur = self.class(class);
        while let Some(c) = cur {
            if chain.iter().any(|x| x.id == c.id) {
                break; // cycle guard; validate() reports cycles
            }
            chain.push(c);
            cur = self
                .classes
                .values()
                .find(|p| p.promote.as_ref().is_some_and(|pr| pr.to == c.id));
        }
        chain.reverse();
        chain
    }

    /// Strategy ids known by a unit of `class` at `level` (learn lists of the whole
    /// promotion chain, in learn order, without duplicates).
    pub fn known_strategies(&self, class: &str, level: u32) -> Vec<Id> {
        let mut learns: Vec<(u32, &Id)> = self
            .class_chain(class)
            .into_iter()
            .flat_map(|c| c.strategies.iter().map(|l| (l.level, &l.id)))
            .filter(|(lv, _)| *lv <= level)
            .collect();
        learns.sort_by_key(|(lv, _)| *lv);
        let mut out: Vec<Id> = Vec::new();
        for (_, id) in learns {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }

    /// Display name for a drama speaker: officer id -> officer name, otherwise the text itself.
    pub fn speaker_name<'a>(&'a self, speaker: &'a str) -> &'a str {
        match self.officer(speaker) {
            Some(o) => &o.name,
            None => speaker,
        }
    }

    /// Officer referenced by a drama speaker (by id or by display name).
    pub fn speaker_officer(&self, speaker: &str) -> Option<&OfficerDef> {
        self.officer(speaker)
            .or_else(|| self.officers.values().find(|o| o.name == speaker))
    }
}
