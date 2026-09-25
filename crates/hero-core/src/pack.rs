//! Data packs: a directory with `pack.toml` plus rules, officers, battles, dramas and media.
//!
//! Loading is split in two so the web build can fetch files asynchronously:
//! 1. [`PackManifest::parse`] reads `pack.toml`; [`PackManifest::text_files`] lists every text
//!    file the pack needs.
//! 2. The frontend reads those files (fetch on web, `std::fs` natively) into a [`FileSource`]
//!    and calls [`Pack::load`], which parses everything synchronously.

use crate::battledef::BattleDef;
use crate::campaign::CampaignDef;
use crate::data::{ClassDef, GameRules, Id, ItemDef, OfficerDef, StrategyDef, TerrainDef};
use crate::script::Scene;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RulesFiles {
    pub game: String,
    pub terrain: String,
    pub classes: String,
    pub strategies: String,
    pub items: String,
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
        self.get(path).cloned().ok_or_else(|| PackError::Missing { file: path.into() })
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

impl Pack {
    /// Parse `pack.toml` and every file it lists. Structural errors (bad TOML, duplicate ids,
    /// unparsable maps or dramas) fail here; cross-reference problems are reported by
    /// [`Pack::validate`].
    pub fn load(src: &dyn FileSource) -> Result<Pack, PackError> {
        let _ = src;
        todo!("W1b: Pack::load")
    }

    /// Cross-check every reference in the pack (class/item/strategy/officer/terrain/scene/battle
    /// ids, promotion chains, deploy slots inside the map and on passable tiles, unit positions,
    /// unique unit tags, reachable campaign nodes, ...). Returns all findings; the pack is usable
    /// when no `Severity::Error` is present.
    pub fn validate(&self) -> Vec<Issue> {
        todo!("W1b: Pack::validate")
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
