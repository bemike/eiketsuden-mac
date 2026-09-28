//! Player settings, persisted as JSON under the storage key [`SETTINGS_KEY`].
//!
//! Unknown or missing fields fall back to their defaults, so settings files written by older or
//! newer builds keep working.

use crate::platform::storage::{KeyValueStore, StorageError};
use serde::{Deserialize, Serialize};

pub const SETTINGS_KEY: &str = "settings";

/// How fast dialogue text is typed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextSpeed {
    Slow,
    #[default]
    Normal,
    Fast,
    /// Whole pages appear at once.
    Instant,
}

impl TextSpeed {
    pub const ALL: [TextSpeed; 4] = [
        TextSpeed::Slow,
        TextSpeed::Normal,
        TextSpeed::Fast,
        TextSpeed::Instant,
    ];

    /// Characters revealed per second; `None` means instant.
    pub fn chars_per_second(self) -> Option<f32> {
        match self {
            TextSpeed::Slow => Some(18.0),
            TextSpeed::Normal => Some(36.0),
            TextSpeed::Fast => Some(80.0),
            TextSpeed::Instant => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TextSpeed::Slow => "느림",
            TextSpeed::Normal => "보통",
            TextSpeed::Fast => "빠름",
            TextSpeed::Instant => "즉시",
        }
    }
}

/// Speed multiplier for battle animations (unit movement, effects, damage numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BattleSpeed {
    #[default]
    Normal,
    Fast,
    VeryFast,
}

impl BattleSpeed {
    pub const ALL: [BattleSpeed; 3] = [
        BattleSpeed::Normal,
        BattleSpeed::Fast,
        BattleSpeed::VeryFast,
    ];

    /// Factor applied to animation playback speed (durations are divided by it).
    pub fn multiplier(self) -> f32 {
        match self {
            BattleSpeed::Normal => 1.0,
            BattleSpeed::Fast => 2.0,
            BattleSpeed::VeryFast => 4.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BattleSpeed::Normal => "보통",
            BattleSpeed::Fast => "빠름",
            BattleSpeed::VeryFast => "매우 빠름",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Volumes in percent, 0..=100.
    pub master_volume: u8,
    pub bgm_volume: u8,
    pub sfx_volume: u8,
    pub text_speed: TextSpeed,
    pub battle_speed: BattleSpeed,
    /// Native builds only; ignored on the web.
    pub fullscreen: bool,
    /// Folder of the player's own copy of the original game, chosen in the game (native only).
    pub original_dir: Option<String>,
    /// Play the original mode: convert [`Settings::original_dir`] at launch and play it on top
    /// of the base pack (native only; see `crate::original`).
    pub original_mode: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            master_volume: 80,
            bgm_volume: 70,
            sfx_volume: 80,
            text_speed: TextSpeed::Normal,
            battle_speed: BattleSpeed::Normal,
            fullscreen: false,
            original_dir: None,
            original_mode: false,
        }
    }
}

impl Settings {
    /// Read the settings. A missing entry yields the defaults; an unreadable or corrupt entry
    /// yields the defaults plus a warning for the log.
    pub fn load(store: &dyn KeyValueStore) -> (Settings, Option<String>) {
        match store.get(SETTINGS_KEY) {
            Ok(None) => (Settings::default(), None),
            Ok(Some(json)) => match serde_json::from_str::<Settings>(&json) {
                Ok(s) => (s.sanitized(), None),
                Err(e) => (
                    Settings::default(),
                    Some(format!("settings are corrupt, using defaults: {e}")),
                ),
            },
            Err(e) => (
                Settings::default(),
                Some(format!("cannot read settings, using defaults: {e}")),
            ),
        }
    }

    pub fn save(&self, store: &mut dyn KeyValueStore) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(self).expect("settings serialization cannot fail");
        store.set(SETTINGS_KEY, &json)
    }

    /// Clamp values written by hand or by other builds into their valid ranges.
    pub fn sanitized(mut self) -> Settings {
        self.master_volume = self.master_volume.min(100);
        self.bgm_volume = self.bgm_volume.min(100);
        self.sfx_volume = self.sfx_volume.min(100);
        if self
            .original_dir
            .as_deref()
            .is_some_and(|d| d.trim().is_empty())
        {
            self.original_dir = None;
        }
        self.original_mode &= self.original_dir.is_some();
        self
    }

    /// The original-mode folder to convert at launch, when the original mode is on.
    pub fn original_to_play(&self) -> Option<&str> {
        self.original_dir.as_deref().filter(|_| self.original_mode)
    }

    /// Effective music gain 0.0..=1.0.
    pub fn bgm_gain(&self) -> f32 {
        f32::from(self.master_volume) / 100.0 * f32::from(self.bgm_volume) / 100.0
    }

    /// Effective sound effect gain 0.0..=1.0.
    pub fn sfx_gain(&self) -> f32 {
        f32::from(self.master_volume) / 100.0 * f32::from(self.sfx_volume) / 100.0
    }
}

/// Step a value through a list of options (wrapping), used by the settings screen.
pub fn cycle<T: Copy + PartialEq>(options: &[T], current: T, delta: i32) -> T {
    let len = options.len() as i32;
    if len == 0 {
        return current;
    }
    let idx = options.iter().position(|o| *o == current).unwrap_or(0) as i32;
    options[(idx + delta).rem_euclid(len) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::storage::MemoryStore;

    #[test]
    fn defaults_when_missing_or_corrupt() {
        let mut store = MemoryStore::default();
        let (s, warn) = Settings::load(&store);
        assert_eq!(s, Settings::default());
        assert!(warn.is_none());

        store.set(SETTINGS_KEY, "{not json").unwrap();
        let (s, warn) = Settings::load(&store);
        assert_eq!(s, Settings::default());
        assert!(warn.is_some());
    }

    #[test]
    fn roundtrip_and_partial_documents() {
        let mut store = MemoryStore::default();
        let s = Settings {
            bgm_volume: 10,
            text_speed: TextSpeed::Instant,
            battle_speed: BattleSpeed::VeryFast,
            fullscreen: true,
            ..Settings::default()
        };
        s.save(&mut store).unwrap();
        assert_eq!(Settings::load(&store).0, s);

        // Older files with fewer fields and out-of-range values still load.
        store
            .set(SETTINGS_KEY, r#"{"sfx_volume": 250, "text_speed": "fast"}"#)
            .unwrap();
        let (s, warn) = Settings::load(&store);
        assert!(warn.is_none());
        assert_eq!(s.sfx_volume, 100);
        assert_eq!(s.text_speed, TextSpeed::Fast);
        assert_eq!(s.master_volume, Settings::default().master_volume);
        assert_eq!(s.original_to_play(), None);
    }

    #[test]
    fn original_mode_needs_a_folder() {
        let mut store = MemoryStore::default();
        let s = Settings {
            original_dir: Some(r"D:\games\hero\GAME".into()),
            original_mode: true,
            ..Settings::default()
        };
        s.save(&mut store).unwrap();
        assert_eq!(
            Settings::load(&store).0.original_to_play(),
            Some(r"D:\games\hero\GAME")
        );

        store
            .set(
                SETTINGS_KEY,
                r#"{"original_mode": true, "original_dir": " "}"#,
            )
            .unwrap();
        let (s, _) = Settings::load(&store);
        assert_eq!((s.original_dir, s.original_mode), (None, false));
    }

    #[test]
    fn gains_and_cycle() {
        let s = Settings {
            master_volume: 50,
            bgm_volume: 50,
            sfx_volume: 100,
            ..Settings::default()
        };
        assert!((s.bgm_gain() - 0.25).abs() < 1e-6);
        assert!((s.sfx_gain() - 0.5).abs() < 1e-6);
        assert_eq!(
            cycle(&TextSpeed::ALL, TextSpeed::Instant, 1),
            TextSpeed::Slow
        );
        assert_eq!(
            cycle(&TextSpeed::ALL, TextSpeed::Slow, -1),
            TextSpeed::Instant
        );
        assert_eq!(
            cycle(&BattleSpeed::ALL, BattleSpeed::Normal, 1),
            BattleSpeed::Fast
        );
    }
}
