//! Save games: a versioned JSON document holding the campaign and, optionally, a battle
//! in progress. Storage (files natively, localStorage on the web) is the frontend's job.

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
