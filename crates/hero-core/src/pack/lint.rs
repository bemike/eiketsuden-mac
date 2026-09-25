//! Unknown-key detection behind [`Pack::unknown_fields`].
//!
//! serde ignores keys it does not know, so a misspelt optional field (`can_couter = true`)
//! would silently fall back to its default. The check parses each TOML file twice — as a
//! plain table and as the schema type — re-serializes the typed value and reports every key
//! of the original that did not survive the round trip.

use super::{
    parse_error, parse_toml, read, ClassesFile, FileSource, ItemsFile, OfficersFile, PackError, PackManifest,
    StrategiesFile, TerrainFile, MANIFEST_FILE,
};
use super::{Issue, Pack, Severity};
use crate::battledef::BattleDef;
use crate::campaign::CampaignDef;
use crate::data::GameRules;
use serde::de::DeserializeOwned;
use serde::Serialize;
use toml::Value;

impl Pack {
    /// Keys in the pack's TOML files that the schema does not know (typos such as
    /// `hp_grwth`), as warnings with the file as context. Files that fail to parse return the
    /// same error as [`Pack::load`].
    pub fn unknown_fields(src: &dyn FileSource) -> Result<Vec<Issue>, PackError> {
        let manifest_text = read(src, MANIFEST_FILE)?;
        let manifest = PackManifest::parse(&manifest_text)?;
        manifest.check_paths()?;
        let mut issues = Vec::new();
        check::<PackManifest>(MANIFEST_FILE, &manifest_text, &mut issues)?;
        let r = &manifest.rules;
        check::<GameRules>(&r.game, &read(src, &r.game)?, &mut issues)?;
        check::<TerrainFile>(&r.terrain, &read(src, &r.terrain)?, &mut issues)?;
        check::<ClassesFile>(&r.classes, &read(src, &r.classes)?, &mut issues)?;
        check::<StrategiesFile>(&r.strategies, &read(src, &r.strategies)?, &mut issues)?;
        check::<ItemsFile>(&r.items, &read(src, &r.items)?, &mut issues)?;
        check::<OfficersFile>(&manifest.officers, &read(src, &manifest.officers)?, &mut issues)?;
        check::<CampaignDef>(&manifest.campaign, &read(src, &manifest.campaign)?, &mut issues)?;
        for file in &manifest.battles {
            check::<BattleDef>(file, &read(src, file)?, &mut issues)?;
        }
        Ok(issues)
    }
}

fn check<T: DeserializeOwned + Serialize>(file: &str, text: &str, issues: &mut Vec<Issue>) -> Result<(), PackError> {
    let original: toml::Table = parse_toml(file, text)?;
    let typed: T = parse_toml(file, text)?;
    let known = Value::try_from(&typed)
        .map_err(|e| parse_error(file, format!("cannot re-serialize for the unknown-field check: {e}")))?;
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
