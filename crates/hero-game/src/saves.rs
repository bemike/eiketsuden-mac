//! Save slots on top of the [`KeyValueStore`]: one autosave slot and [`MANUAL_SLOTS`] manual
//! slots, each holding one [`SaveGame`] JSON document (`hero_core::save`).
//!
//! The screens use [`list`] for the slot overview, [`write`] / [`read`] / [`delete`] for the
//! actions and [`latest`] for the title screen's "continue".

use crate::platform::storage::{KeyValueStore, StorageError};
use hero_core::save::{SaveError, SaveGame};
use std::fmt;

/// Number of manual save slots.
pub const MANUAL_SLOTS: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SaveSlot {
    /// Written automatically when the campaign advances.
    Auto,
    /// Player-chosen slot, `1..=MANUAL_SLOTS`.
    Manual(u8),
}

impl SaveSlot {
    /// Every slot in display order: the autosave first.
    pub fn all() -> impl Iterator<Item = SaveSlot> {
        std::iter::once(SaveSlot::Auto).chain((1..=MANUAL_SLOTS).map(SaveSlot::Manual))
    }

    /// Storage key of the slot.
    pub fn key(self) -> String {
        match self {
            SaveSlot::Auto => "save_auto".into(),
            SaveSlot::Manual(n) => format!("save_{n}"),
        }
    }

    /// Name shown in slot lists.
    pub fn name(self) -> String {
        match self {
            SaveSlot::Auto => "자동 기록".into(),
            SaveSlot::Manual(n) => format!("기록 {n}"),
        }
    }

    fn is_valid(self) -> bool {
        match self {
            SaveSlot::Auto => true,
            SaveSlot::Manual(n) => (1..=MANUAL_SLOTS).contains(&n),
        }
    }
}

/// What the slot list shows for a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSummary {
    pub label: String,
    /// Unix seconds (0 when unknown).
    pub saved_at: u64,
    pub play_seconds: u64,
    /// The save was made during a battle.
    pub mid_battle: bool,
    pub pack_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Ready(SlotSummary),
    /// Present but not loadable (corrupt, newer version, another pack, storage error).
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub slot: SaveSlot,
    pub status: SlotStatus,
}

impl SlotInfo {
    pub fn summary(&self) -> Option<&SlotSummary> {
        match &self.status {
            SlotStatus::Ready(s) => Some(s),
            _ => None,
        }
    }
}

/// Why a slot could not be loaded or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveSlotError {
    Empty(SaveSlot),
    InvalidSlot(SaveSlot),
    Storage(StorageError),
    Save(SaveError),
}

impl fmt::Display for SaveSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveSlotError::Empty(slot) => write!(f, "{}은(는) 비어 있습니다", slot.name()),
            SaveSlotError::InvalidSlot(slot) => write!(f, "잘못된 슬롯: {slot:?}"),
            SaveSlotError::Storage(e) => write!(f, "저장소 오류 ({e})"),
            SaveSlotError::Save(e) => match e {
                SaveError::Corrupt(msg) => write!(f, "기록이 손상되었습니다 ({msg})"),
                SaveError::TooNew { found, supported } => write!(
                    f,
                    "더 새로운 버전의 기록입니다 (기록 v{found}, 지원 v{supported})"
                ),
                SaveError::WrongPack { found, expected } => write!(
                    f,
                    "다른 데이터 팩의 기록입니다 (기록 `{found}`, 현재 `{expected}`)"
                ),
            },
        }
    }
}

impl std::error::Error for SaveSlotError {}

/// Summaries of every slot in display order.
pub fn list(store: &dyn KeyValueStore, pack_id: &str) -> Vec<SlotInfo> {
    SaveSlot::all()
        .map(|slot| {
            let status = match read(store, slot, pack_id) {
                Ok(save) => SlotStatus::Ready(SlotSummary {
                    label: save.label.clone(),
                    saved_at: save.saved_at,
                    play_seconds: save.campaign.play_seconds,
                    mid_battle: save.battle.is_some(),
                    pack_version: save.pack_version.clone(),
                }),
                Err(SaveSlotError::Empty(_)) => SlotStatus::Empty,
                Err(e) => SlotStatus::Unreadable(e.to_string()),
            };
            SlotInfo { slot, status }
        })
        .collect()
}

/// Load a slot, checking the save version and that it belongs to `pack_id`.
pub fn read(
    store: &dyn KeyValueStore,
    slot: SaveSlot,
    pack_id: &str,
) -> Result<SaveGame, SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    let json = store
        .get(&slot.key())
        .map_err(SaveSlotError::Storage)?
        .ok_or(SaveSlotError::Empty(slot))?;
    SaveGame::from_json(&json, pack_id).map_err(SaveSlotError::Save)
}

/// Write a save into a slot (replacing its content atomically).
pub fn write(
    store: &mut dyn KeyValueStore,
    slot: SaveSlot,
    save: &SaveGame,
) -> Result<(), SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    store
        .set(&slot.key(), &save.to_json())
        .map_err(SaveSlotError::Storage)
}

pub fn delete(store: &mut dyn KeyValueStore, slot: SaveSlot) -> Result<(), SaveSlotError> {
    if !slot.is_valid() {
        return Err(SaveSlotError::InvalidSlot(slot));
    }
    store.remove(&slot.key()).map_err(SaveSlotError::Storage)
}

/// The most recently saved loadable slot of `pack_id` (for "continue").
pub fn latest(store: &dyn KeyValueStore, pack_id: &str) -> Option<SaveSlot> {
    list(store, pack_id)
        .into_iter()
        .filter_map(|info| info.summary().map(|s| (s.saved_at, info.slot)))
        // Newest first; on equal timestamps prefer the autosave (it is listed first).
        .fold(
            None,
            |best: Option<(u64, SaveSlot)>, (at, slot)| match best {
                Some((b, _)) if b >= at => best,
                _ => Some((at, slot)),
            },
        )
        .map(|(_, slot)| slot)
}

/// Whether any slot holds a loadable save of `pack_id`.
pub fn any(store: &dyn KeyValueStore, pack_id: &str) -> bool {
    SaveSlot::all().any(|slot| read(store, slot, pack_id).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::storage::MemoryStore;
    use hero_core::campaign::CampaignState;
    use hero_core::save::SAVE_VERSION;
    use std::collections::BTreeMap;

    fn save(label: &str, at: u64, pack: &str) -> SaveGame {
        SaveGame {
            version: SAVE_VERSION,
            pack_id: pack.into(),
            pack_version: "0.1.0".into(),
            label: label.into(),
            saved_at: at,
            campaign: CampaignState {
                node: "start".into(),
                roster: Vec::new(),
                inventory: BTreeMap::new(),
                gold: 100,
                flags: BTreeMap::new(),
                deployed: Vec::new(),
                battles_won: Vec::new(),
                play_seconds: 3600,
            },
            battle: None,
        }
    }

    #[test]
    fn slots_and_keys() {
        let all: Vec<_> = SaveSlot::all().collect();
        assert_eq!(all.len(), 1 + MANUAL_SLOTS as usize);
        assert_eq!(all[0], SaveSlot::Auto);
        assert_eq!(SaveSlot::Manual(3).key(), "save_3");
        assert_eq!(SaveSlot::Auto.key(), "save_auto");
        let mut store = MemoryStore::default();
        assert_eq!(
            write(&mut store, SaveSlot::Manual(9), &save("x", 1, "base")),
            Err(SaveSlotError::InvalidSlot(SaveSlot::Manual(9)))
        );
    }

    #[test]
    fn write_read_list_delete() {
        let mut store = MemoryStore::default();
        assert!(!any(&store, "base"));
        assert_eq!(latest(&store, "base"), None);

        write(&mut store, SaveSlot::Manual(2), &save("탁현", 100, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("자동", 200, "base")).unwrap();
        write(
            &mut store,
            SaveSlot::Manual(5),
            &save("다른 팩", 300, "other"),
        )
        .unwrap();
        store.set("save_7", "garbage").unwrap();

        let loaded = read(&store, SaveSlot::Manual(2), "base").unwrap();
        assert_eq!(loaded.label, "탁현");
        assert!(matches!(
            read(&store, SaveSlot::Manual(1), "base"),
            Err(SaveSlotError::Empty(SaveSlot::Manual(1)))
        ));

        let infos = list(&store, "base");
        assert_eq!(infos.len(), 9);
        assert_eq!(infos[0].summary().unwrap().label, "자동");
        assert_eq!(infos[0].summary().unwrap().play_seconds, 3600);
        assert_eq!(infos[1].status, SlotStatus::Empty);
        assert!(matches!(infos[5].status, SlotStatus::Unreadable(_)));
        assert!(matches!(infos[7].status, SlotStatus::Unreadable(_)));

        // The other pack's newer save is ignored.
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Auto));
        assert!(any(&store, "base"));

        delete(&mut store, SaveSlot::Auto).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Manual(2)));
    }

    #[test]
    fn latest_prefers_autosave_on_ties() {
        let mut store = MemoryStore::default();
        write(&mut store, SaveSlot::Manual(1), &save("a", 50, "base")).unwrap();
        write(&mut store, SaveSlot::Auto, &save("b", 50, "base")).unwrap();
        assert_eq!(latest(&store, "base"), Some(SaveSlot::Auto));
    }

    #[test]
    fn error_messages_are_readable() {
        let e = SaveSlotError::Save(SaveError::WrongPack {
            found: "x".into(),
            expected: "base".into(),
        });
        assert!(e.to_string().contains("다른 데이터 팩"));
        assert!(SaveSlotError::Empty(SaveSlot::Auto)
            .to_string()
            .contains("자동 기록"));
    }
}
