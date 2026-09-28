//! Unknown-key detection behind [`Pack::unknown_fields`].
//!
//! serde ignores keys it does not know, so a misspelt optional field (`can_couter = true`)
//! would silently fall back to its default. The check parses each TOML file twice — as a
//! plain table and as the schema type — re-serializes the typed value and reports every key
//! of the original that did not survive the round trip.

use super::{
    parse_error, parse_toml, read, ClassesFile, FileSource, ItemsFile, MapsFile, OfficersFile,
    PackChain, PackError, PackFile, PackManifest, StrategiesFile, TerrainFile,
};
use super::{Issue, Pack, Severity};
use crate::battledef::BattleDef;
use crate::campaign::CampaignDef;
use crate::data::GameRules;
use crate::media_index::{FxFile, TilesetFile, UnitsFile, FX_FILE, TILESET_FILE, UNITS_FILE};
use serde::de::DeserializeOwned;
use serde::Serialize;
use toml::Value;

impl Pack {
    /// Keys in the pack's TOML files that the schema does not know (typos such as
    /// `hp_grwth`), as warnings with the file as context. For a layered pack every `pack.toml`
    /// of the chain, the rules, officer and campaign files in use and the battle and map files
    /// of every layer are checked, and the battle media indexes (`units.toml`, `terrain.toml`,
    /// `fx.toml`) the game reads, each from the first layer that has it. Files that fail to parse
    /// return the same error as [`Pack::load`]; a media index that does not parse is skipped here
    /// and reported by [`Pack::missing_media`] when the pack uses it (classes for `units.toml`,
    /// terrain for `terrain.toml`, a strategy with an effect for `fx.toml`).
    pub fn unknown_fields(src: &dyn FileSource) -> Result<Vec<Issue>, PackError> {
        let chain = PackChain::read(src)?;
        let files = chain.resolve()?;
        let mut issues = Vec::new();
        for layer in chain.layers() {
            let path = layer.manifest_path();
            check::<PackManifest>(&path, &read(src, &path)?, &mut issues)?;
        }
        let mut run = |file: &PackFile, check: Check| -> Result<(), PackError> {
            let path = file.source_path();
            check(&path, &read(src, &path)?, &mut issues)
        };
        run(&files.game, check::<GameRules>)?;
        run(&files.terrain, check::<TerrainFile>)?;
        run(&files.classes, check::<ClassesFile>)?;
        run(&files.strategies, check::<StrategiesFile>)?;
        run(&files.items, check::<ItemsFile>)?;
        run(&files.officers, check::<OfficersFile>)?;
        run(&files.campaign, check::<CampaignDef>)?;
        for file in &files.battles {
            run(file, check::<BattleDef>)?;
        }
        for file in &files.maps {
            run(file, check::<MapsFile>)?;
        }
        let media: [(&str, Check); 3] = [
            (UNITS_FILE, check::<UnitsFile>),
            (TILESET_FILE, check::<TilesetFile>),
            (FX_FILE, check::<FxFile>),
        ];
        for (rel, check) in media {
            // The first layer that has the file, as the game reads it; a file that exists but
            // cannot be read stops the search (a parent's copy is not the one in use).
            let mut found = None;
            for path in chain.layers().iter().map(|layer| layer.file(rel)) {
                // `read` drops a byte-order mark like for every other pack file.
                match read(src, &path) {
                    Ok(text) => {
                        found = Some((path, text));
                        break;
                    }
                    Err(PackError::Missing { .. }) => continue,
                    Err(_) => break,
                }
            }
            if let Some((path, text)) = found {
                let mut found_issues = Vec::new();
                if check(&path, &text, &mut found_issues).is_ok() {
                    issues.extend(found_issues);
                }
            }
        }
        Ok(issues)
    }
}

/// One file's check: `(file name, text, issues)`.
type Check = fn(&str, &str, &mut Vec<Issue>) -> Result<(), PackError>;

fn check<T: DeserializeOwned + Serialize>(
    file: &str,
    text: &str,
    issues: &mut Vec<Issue>,
) -> Result<(), PackError> {
    let original: toml::Table = parse_toml(file, text)?;
    let typed: T = parse_toml(file, text)?;
    let known = Value::try_from(&typed).map_err(|e| {
        parse_error(
            file,
            format!("cannot re-serialize for the unknown-field check: {e}"),
        )
    })?;
    compare(&Value::Table(original), &known, "", file, issues);
    Ok(())
}

fn compare(original: &Value, known: &Value, path: &str, file: &str, issues: &mut Vec<Issue>) {
    match (original, known) {
        (Value::Table(orig), Value::Table(known)) => {
            for (key, value) in orig {
                let sub = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match known.get(key) {
                    Some(k) => compare(value, k, &sub, file, issues),
                    None => issues.push(Issue {
                        severity: Severity::Warning,
                        context: file.to_string(),
                        msg: format!("unknown field `{sub}` is ignored"),
                    }),
                }
            }
        }
        (Value::Array(orig), Value::Array(known)) => {
            for (i, (o, k)) in orig.iter().zip(known).enumerate() {
                // Name entries of `[[class]]`-style arrays by their id when they have one.
                let sub = match o.get("id").and_then(Value::as_str) {
                    Some(id) => format!("{path}[{id}]"),
                    None => format!("{path}[{i}]"),
                };
                compare(o, k, &sub, file, issues);
            }
        }
        _ => {}
    }
}
