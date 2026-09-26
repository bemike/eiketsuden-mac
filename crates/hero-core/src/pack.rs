//! Data packs: a directory with `pack.toml` plus rules, officers, battles, dramas and media.
//!
//! Loading is split in two so the web build can fetch files asynchronously:
//! 1. [`PackManifest::parse`] reads `pack.toml`; [`PackManifest::text_files`] lists every text
//!    file the pack needs.
//! 2. The frontend reads those files (fetch on web, `std::fs` natively) into a [`FileSource`]
//!    and calls [`Pack::load`], which parses everything synchronously.
//!
//! [`Pack::validate`] then cross-checks every reference; tools additionally run
//! [`Pack::unknown_fields`] (typos in TOML keys) and, natively, [`Pack::missing_media`].
//! The file format is documented for modders in `docs/MODDING.md`.

mod lint;
#[cfg(not(target_arch = "wasm32"))]
mod media;
mod validate;

use crate::battledef::BattleDef;
use crate::campaign::CampaignDef;
use crate::data::{ClassDef, GameRules, Id, ItemDef, OfficerDef, StrategyDef, TerrainDef};
use crate::map::BattleMap;
use crate::script::Scene;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RulesFiles {
    pub game: String,
    pub terrain: String,
    pub classes: String,
    pub strategies: String,
    pub items: String,
}

/// Default virtual canvas size in pixels (the base pack's 16 px tiles: 30 × 17 visible tiles).
pub const DEFAULT_CANVAS: [u32; 2] = [480, 270];
/// Smallest virtual canvas a pack may ask for, `[width, height]`.
pub const MIN_CANVAS: [u32; 2] = [320, 200];
/// Largest virtual canvas a pack may ask for, `[width, height]`.
pub const MAX_CANVAS: [u32; 2] = [1280, 800];

fn default_canvas() -> [u32; 2] {
    DEFAULT_CANVAS
}

/// `[presentation]` of `pack.toml`: how the frontend lays the pack's media out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Presentation {
    /// Size of the virtual canvas the game renders to, `[width, height]` in pixels, within
    /// [`MIN_CANVAS`]..=[`MAX_CANVAS`]. Default [`DEFAULT_CANVAS`].
    #[serde(default = "default_canvas")]
    pub canvas: [u32; 2],
}

impl Default for Presentation {
    fn default() -> Self {
        Presentation {
            canvas: DEFAULT_CANVAS,
        }
    }
}

/// `pack.toml`. All paths are relative to the pack directory and use `/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackManifest {
    /// Machine id, e.g. `base`.
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
    /// Canvas size and other presentation settings (`[presentation]`).
    #[serde(default)]
    pub presentation: Presentation,
    pub rules: RulesFiles,
    pub officers: String,
    pub campaign: String,
    /// Battle definition files.
    pub battles: Vec<String>,
    /// Drama script files.
    pub dramas: Vec<String>,
}

impl PackManifest {
    pub fn parse(src: &str) -> Result<PackManifest, PackError> {
        toml::from_str(src).map_err(|e| PackError::Parse {
            file: "pack.toml".into(),
            msg: e.to_string(),
        })
    }

    /// Every text file (relative path) that [`Pack::load`] will read, excluding `pack.toml`.
    pub fn text_files(&self) -> Vec<String> {
        let mut v = vec![
            self.rules.game.clone(),
            self.rules.terrain.clone(),
            self.rules.classes.clone(),
            self.rules.strategies.clone(),
            self.rules.items.clone(),
            self.officers.clone(),
            self.campaign.clone(),
        ];
        v.extend(self.battles.iter().cloned());
        v.extend(self.dramas.iter().cloned());
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
    fn read_text(&self, path: &str) -> Result<String, PackError>;
}

impl FileSource for BTreeMap<String, String> {
    fn read_text(&self, path: &str) -> Result<String, PackError> {
        self.get(path)
            .cloned()
            .ok_or_else(|| PackError::Missing { file: path.into() })
    }
}

/// Reads pack files from a directory on disk (native builds and tools).
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

/// A fully parsed data pack.
#[derive(Debug, Clone)]
pub struct Pack {
    pub manifest: PackManifest,
    pub rules: GameRules,
    /// Terrain in file order (order matters only for display).
    pub terrain: Vec<TerrainDef>,
    pub classes: BTreeMap<Id, ClassDef>,
    pub strategies: BTreeMap<Id, StrategyDef>,
    pub items: BTreeMap<Id, ItemDef>,
    pub officers: BTreeMap<Id, OfficerDef>,
    pub battles: BTreeMap<Id, BattleDef>,
    /// Drama scenes by globally unique scene id.
    pub scenes: BTreeMap<String, Scene>,
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

impl PackManifest {
    /// Every listed path must be relative, use `/` and stay inside the pack, no file may be
    /// listed twice (each file has exactly one role), and the canvas is within
    /// [`MIN_CANVAS`]..=[`MAX_CANVAS`].
    fn check_paths(&self) -> Result<(), PackError> {
        let mut seen = BTreeSet::new();
        for path in self.text_files() {
            let bad = path.trim().is_empty()
                || path.starts_with('/')
                || path.contains('\\')
                || path.contains(':')
                || path.split('/').any(|part| part == "..");
            if bad {
                return Err(parse_error(
                    MANIFEST_FILE,
                    format!("path `{path}` must be relative to the pack directory, use `/` and not contain `..`"),
                ));
            }
            if !seen.insert(path.clone()) {
                return Err(parse_error(
                    MANIFEST_FILE,
                    format!("file `{path}` is listed more than once"),
                ));
            }
        }
        let [w, h] = self.presentation.canvas;
        let ([min_w, min_h], [max_w, max_h]) = (MIN_CANVAS, MAX_CANVAS);
        if !(min_w..=max_w).contains(&w) || !(min_h..=max_h).contains(&h) {
            return Err(parse_error(
                MANIFEST_FILE,
                format!("presentation.canvas [{w}, {h}] must be between [{min_w}, {min_h}] and [{max_w}, {max_h}]"),
            ));
        }
        Ok(())
    }
}

impl Pack {
    /// Parse `pack.toml` and every file it lists. Structural errors (bad TOML, duplicate ids,
    /// unparsable maps or dramas) fail here; cross-reference problems are reported by
    /// [`Pack::validate`].
    pub fn load(src: &dyn FileSource) -> Result<Pack, PackError> {
        let manifest = PackManifest::parse(&read(src, MANIFEST_FILE)?)?;
        manifest.check_paths()?;
        let files = &manifest.rules;

        let rules: GameRules = parse_toml(&files.game, &read(src, &files.game)?)?;

        let terrain =
            parse_toml::<TerrainFile>(&files.terrain, &read(src, &files.terrain)?)?.terrain;
        let mut terrain_ids = BTreeSet::new();
        let mut glyphs = BTreeMap::new();
        for t in &terrain {
            if t.id.trim().is_empty() {
                return Err(parse_error(&files.terrain, "terrain with an empty id"));
            }
            if !terrain_ids.insert(t.id.as_str()) {
                return Err(parse_error(
                    &files.terrain,
                    format!("duplicate terrain id `{}`", t.id),
                ));
            }
            if let Some(other) = glyphs.insert(t.glyph, t.id.as_str()) {
                return Err(parse_error(
                    &files.terrain,
                    format!(
                        "terrain `{}` reuses glyph {:?} of terrain `{other}`",
                        t.id, t.glyph
                    ),
                ));
            }
        }

        let classes = parse_toml::<ClassesFile>(&files.classes, &read(src, &files.classes)?)?.class;
        let classes = index_by_id(&files.classes, "class", classes, |c| &c.id)?;
        let strategies =
            parse_toml::<StrategiesFile>(&files.strategies, &read(src, &files.strategies)?)?
                .strategy;
        let strategies = index_by_id(&files.strategies, "strategy", strategies, |s| &s.id)?;
        let items = parse_toml::<ItemsFile>(&files.items, &read(src, &files.items)?)?.item;
        let items = index_by_id(&files.items, "item", items, |i| &i.id)?;
        let officers =
            parse_toml::<OfficersFile>(&manifest.officers, &read(src, &manifest.officers)?)?
                .officer;
        let officers = index_by_id(&manifest.officers, "officer", officers, |o| &o.id)?;

        let campaign: CampaignDef =
            parse_toml(&manifest.campaign, &read(src, &manifest.campaign)?)?;
        let mut node_ids = BTreeSet::new();
        for node in &campaign.nodes {
            if node.id().trim().is_empty() {
                return Err(parse_error(
                    &manifest.campaign,
                    "campaign node with an empty id",
                ));
            }
            if !node_ids.insert(node.id()) {
                return Err(parse_error(
                    &manifest.campaign,
                    format!("duplicate campaign node id `{}`", node.id()),
                ));
            }
        }

        let mut battles: BTreeMap<Id, BattleDef> = BTreeMap::new();
        let mut battle_files: BTreeMap<Id, &str> = BTreeMap::new();
        for file in &manifest.battles {
            let battle: BattleDef = parse_toml(file, &read(src, file)?)?;
            if battle.id.trim().is_empty() {
                return Err(parse_error(file, "battle with an empty id"));
            }
            if let Some(first) = battle_files.get(&battle.id) {
                return Err(parse_error(
                    file,
                    format!(
                        "duplicate battle id `{}` (first defined in {first})",
                        battle.id
                    ),
                ));
            }
            if let Some(key) = battle.map.legend.keys().find(|k| k.chars().count() != 1) {
                return Err(parse_error(
                    file,
                    format!("map legend key `{key}` must be exactly one character"),
                ));
            }
            BattleMap::parse(&battle.map.rows, &battle.map.legend, &terrain)
                .map_err(|e| parse_error(file, format!("battle `{}` map: {e}", battle.id)))?;
            battle_files.insert(battle.id.clone(), file);
            battles.insert(battle.id.clone(), battle);
        }

        let mut scenes: BTreeMap<String, Scene> = BTreeMap::new();
        let mut scene_files: BTreeMap<String, &str> = BTreeMap::new();
        for file in &manifest.dramas {
            let parsed = crate::script::parse_drama(file, &read(src, file)?)
                .map_err(|e| parse_error(&e.file, format!("line {}: {}", e.line, e.msg)))?;
            for scene in parsed {
                if let Some(first) = scene_files.get(&scene.id) {
                    return Err(parse_error(
                        file,
                        format!(
                            "duplicate scene id `{}` (first defined in {first})",
                            scene.id
                        ),
                    ));
                }
                scene_files.insert(scene.id.clone(), file);
                scenes.insert(scene.id.clone(), scene);
            }
        }

        Ok(Pack {
            manifest,
            rules,
            terrain,
            classes,
            strategies,
            items,
            officers,
            battles,
            scenes,
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
