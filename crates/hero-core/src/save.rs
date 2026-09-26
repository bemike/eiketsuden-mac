//! Save games: a versioned JSON document holding the campaign and, optionally, a battle
//! in progress. Storage (files natively, localStorage on the web) is the frontend's job; it
//! stores each slot under [`slot_key`], which keeps the saves of different packs apart.

use crate::battle::BattleState;
use crate::campaign::CampaignState;
use serde::{Deserialize, Serialize};

/// Bump when the save layout changes incompatibly; add a migration in [`SaveGame::from_json`].
pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    /// Pack the save belongs to (`pack.toml` id and version).
    pub pack_id: String,
    pub pack_version: String,
    /// Human readable summary for the load screen, e.g. `제2장 · 광종 전투 준비`.
    pub label: String,
    /// Unix seconds when saved, supplied by the frontend (0 when unknown).
    #[serde(default)]
    pub saved_at: u64,
    pub campaign: CampaignState,
    /// Present for a mid-battle save.
    #[serde(default)]
    pub battle: Option<BattleState>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SaveError {
    #[error("save data is corrupt: {0}")]
    Corrupt(String),
    #[error("save version {found} is newer than this game supports ({supported})")]
    TooNew { found: u32, supported: u32 },
    #[error("save belongs to pack `{found}`, not `{expected}`")]
    WrongPack { found: String, expected: String },
}

impl SaveGame {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("save game serialization cannot fail")
    }

    /// Parse and check the version. `expected_pack` guards against loading a save of another pack.
    pub fn from_json(src: &str, expected_pack: &str) -> Result<SaveGame, SaveError> {
        let v: serde_json::Value =
            serde_json::from_str(src).map_err(|e| SaveError::Corrupt(e.to_string()))?;
        let found = v.get("version").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        if found > SAVE_VERSION {
            return Err(SaveError::TooNew {
                found,
                supported: SAVE_VERSION,
            });
        }
        let save: SaveGame =
            serde_json::from_value(v).map_err(|e| SaveError::Corrupt(e.to_string()))?;
        if save.pack_id != expected_pack {
            return Err(SaveError::WrongPack {
                found: save.pack_id,
                expected: expected_pack.to_string(),
            });
        }
        Ok(save)
    }
}

/// Longest pack id used verbatim in a storage key; see [`slot_key`].
const MAX_PLAIN_PACK_KEY: usize = 32;
/// Longest slot name accepted by [`slot_key`].
const MAX_SLOT_NAME: usize = 8;

/// Storage key of a save slot, scoped to the pack the save belongs to: `save_<pack>_<slot>`.
///
/// Every pack gets its own keys, so playing one pack (a mod, the original mode) never
/// overwrites the saves of another; [`SaveGame::from_json`] still checks the pack id when a
/// save is read. `slot` names the slot within the pack (`auto`, `1`, `2`, ...) and must be
/// 1 to 8 lowercase ASCII letters or digits.
///
/// The key follows the frontend's storage key rules (lowercase ASCII letters, digits, `_` and
/// `-`, at most 64 bytes). A pack id of 1 to 32 lowercase ASCII letters, digits and `-` is
/// used as is (`save_base_auto`). Any other id is reduced to those characters (at most 32),
/// followed by `_` and a stable hash of the full id (`save_my-mod_1a2b3c4d_3`), so two distinct
/// ids never share keys: the plain form has no `_`, the reduced form exactly one, and the slot
/// name never contains `_`. Keys without a pack part (`save_auto`, `save_3`, written before
/// saves were pack-scoped) can therefore never collide with these.
///
/// # Panics
///
/// When `slot` breaks the rule above (a programming error in the caller).
pub fn slot_key(pack_id: &str, slot: &str) -> String {
    assert!(
        (1..=MAX_SLOT_NAME).contains(&slot.len())
            && slot
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
        "invalid save slot name `{slot}`"
    );
    format!("save_{}_{slot}", pack_key(pack_id))
}

/// The pack part of [`slot_key`].
fn pack_key(pack_id: &str) -> String {
    let plain = (1..=MAX_PLAIN_PACK_KEY).contains(&pack_id.len())
        && pack_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        return pack_id.to_string();
    }
    let reduced: String = pack_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(MAX_PLAIN_PACK_KEY)
        .collect();
    format!("{reduced}_{:08x}", fnv1a32(pack_id.as_bytes()))
}

/// 32-bit FNV-1a: a hash that stays the same across Rust versions and platforms, unlike
/// `std`'s `DefaultHasher`, because stored keys depend on it.
fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h: u32, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's storage key rule (`hero-game` `platform::storage::validate_key`).
    fn is_storage_key(key: &str) -> bool {
        !key.is_empty()
            && key.len() <= 64
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    }

    #[test]
    fn simple_pack_ids_are_used_verbatim() {
        assert_eq!(slot_key("base", "auto"), "save_base_auto");
        assert_eq!(slot_key("base", "3"), "save_base_3");
        assert_eq!(slot_key("original-kr", "8"), "save_original-kr_8");
    }

    #[test]
    fn other_pack_ids_are_reduced_and_hashed() {
        let long = "a".repeat(200);
        let ids = [
            "Base",
            "base_",
            "my_mod",
            "my mod",
            "영걸전",
            "",
            "x".repeat(33).as_str(),
            long.as_str(),
        ]
        .map(String::from);
        let mut keys = std::collections::BTreeSet::new();
        for id in &ids {
            for slot in ["auto", "1", "12345678"] {
                let key = slot_key(id, slot);
                assert!(is_storage_key(&key), "{id:?} -> {key}");
                assert!(keys.insert(key.clone()), "{id:?} -> {key} collides");
                assert_ne!(key, slot_key("base", slot));
            }
        }
        // Stable across builds: stored keys depend on it.
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(
            slot_key("Base", "auto"),
            format!("save_base_{:08x}_auto", fnv1a32(b"Base"))
        );
    }

    #[test]
    fn keys_never_collide_with_the_unscoped_legacy_keys() {
        for id in ["auto", "1", "save", "Auto"] {
            for slot in ["auto", "1"] {
                let key = slot_key(id, slot);
                assert!(key != "save_auto" && key != "save_1", "{key}");
            }
        }
    }

    #[test]
    #[should_panic(expected = "invalid save slot name")]
    fn slot_names_are_checked() {
        slot_key("base", "my_slot");
    }
}
