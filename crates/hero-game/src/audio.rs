//! Music and sound effects by key, with fades and volume settings.
//!
//! * [`Audio::play_bgm`] switches the music: the current track fades out, then the new one fades
//!   in (looping). Requesting the track that is already playing does nothing, so screens can
//!   call it every time they are entered. [`Audio::play_jingle`] plays a track once (victory,
//!   defeat). [`Audio::stop_bgm`] fades to silence.
//! * [`Audio::sfx`] plays an effect once (keys from `docs/ASSETS.md`, see [`sfx`]). Effects
//!   that are not loaded yet are skipped (and requested, so the next use plays); the engine
//!   effects are preloaded after the pack loads.
//! * Volumes come from [`Settings`] (master × music / master × effects).
//! * Browsers refuse to start audio before the first user gesture, so on the web the manager
//!   starts *locked*: music requests are remembered and start on the first key press, click or
//!   touch ([`Audio::update`] receives that signal from the input snapshot); effects before that
//!   are dropped.
//!
//! Missing audio files are logged by the media store and otherwise ignored — the game plays on
//! silently.

use crate::assets::{AssetState, Media};
use crate::settings::Settings;
use macroquad::audio::{play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};

/// Seconds for the music to fade out when it changes or stops.
pub const FADE_OUT: f32 = 0.6;
/// Seconds for new music to fade in.
pub const FADE_IN: f32 = 0.4;

/// Engine sound effect keys (`docs/ASSETS.md`).
pub mod sfx {
    pub const CURSOR: &str = "cursor";
    pub const CONFIRM: &str = "confirm";
    pub const CANCEL: &str = "cancel";
    pub const ERROR: &str = "error";
    pub const STEP: &str = "step";
    pub const HIT: &str = "hit";
    pub const HIT_HEAVY: &str = "hit_heavy";
    pub const ARROW: &str = "arrow";
    pub const FIRE: &str = "fire";
    pub const WATER: &str = "water";
    pub const ROCK: &str = "rock";
    pub const HEAL: &str = "heal";
    pub const MORALE_UP: &str = "morale_up";
    pub const MORALE_DOWN: &str = "morale_down";
    pub const CONFUSE: &str = "confuse";
    pub const LEVELUP: &str = "levelup";
    pub const RETREAT: &str = "retreat";
    pub const TREASURE: &str = "treasure";
    pub const PHASE: &str = "phase";
    pub const VICTORY: &str = "victory";
    pub const DEFEAT: &str = "defeat";

    /// Every engine effect, for preloading.
    pub const ALL: [&str; 21] = [
        CURSOR,
        CONFIRM,
        CANCEL,
        ERROR,
        STEP,
        HIT,
        HIT_HEAVY,
        ARROW,
        FIRE,
        WATER,
        ROCK,
        HEAL,
        MORALE_UP,
        MORALE_DOWN,
        CONFUSE,
        LEVELUP,
        RETREAT,
        TREASURE,
        PHASE,
        VICTORY,
        DEFEAT,
    ];
}

/// Engine music keys (`docs/ASSETS.md`).
pub mod bgm {
    pub const TITLE: &str = "title";
    pub const PEACE: &str = "peace";
    pub const TENSION: &str = "tension";
    pub const SAD: &str = "sad";
    pub const CAMP: &str = "camp";
    pub const BATTLE: &str = "battle";
    pub const ENEMY: &str = "enemy";
    pub const BOSS: &str = "boss";
    pub const VICTORY: &str = "victory";
    pub const DEFEAT: &str = "defeat";
    pub const ENDING: &str = "ending";
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    key: String,
    looped: bool,
}

struct Track {
    request: Request,
    sound: Sound,
    /// Fade level 0..=1.
    level: f32,
    applied_volume: f32,
    /// Its file was replaced ([`Audio::reload_bgm`]): fade it out and start the new one.
    stale: bool,
}

/// The music/effects manager. Owned by [`crate::app::Ctx`].
pub struct Audio {
    bgm_gain: f32,
    sfx_gain: f32,
    unlocked: bool,
    /// Music that should be playing (`None` = silence).
    wanted: Option<Request>,
    current: Option<Track>,
    /// Effects already started this frame (the same effect twice in a frame is one sound).
    played_this_frame: Vec<String>,
}

impl Audio {
    /// `unlocked` is `false` on the web until the first user gesture.
    pub fn new(settings: &Settings) -> Audio {
        Audio {
            bgm_gain: settings.bgm_gain(),
            sfx_gain: settings.sfx_gain(),
            unlocked: !crate::platform::is_web(),
            wanted: None,
            current: None,
            played_this_frame: Vec::new(),
        }
    }

    /// Apply changed volume settings (takes effect immediately).
    pub fn apply_settings(&mut self, settings: &Settings) {
        self.bgm_gain = settings.bgm_gain();
        self.sfx_gain = settings.sfx_gain();
    }

    /// Whether audio may play yet (always `true` natively).
    pub fn unlocked(&self) -> bool {
        self.unlocked
    }

    /// Play `bgm/<key>.ogg` looping, cross-fading from the current music.
    pub fn play_bgm(&mut self, key: &str) {
        self.wanted = Some(Request {
            key: key.to_string(),
            looped: true,
        });
    }

    /// Play `bgm/<key>.ogg` once (victory / defeat jingles).
    pub fn play_jingle(&mut self, key: &str) {
        self.wanted = Some(Request {
            key: key.to_string(),
            looped: false,
        });
        // A jingle restarts even if the same jingle played before.
        if let Some(t) = &self.current {
            if t.request.key == key {
                stop_sound(&t.sound);
                self.current = None;
            }
        }
    }

    /// The file of music `key` was replaced (the original mode adds its songs while the game
    /// runs): if it is playing as music, fade it out and start it again from the new file. A
    /// jingle plays on to its end (it is not started again).
    pub fn reload_bgm(&mut self, key: &str) {
        if let Some(track) = self.current.as_mut() {
            if track.request.key == key && track.request.looped {
                track.stale = true;
            }
        }
    }

    /// Fade the music out.
    pub fn stop_bgm(&mut self) {
        self.wanted = None;
    }

    /// Key of the music that is playing or about to play.
    pub fn bgm(&self) -> Option<&str> {
        self.wanted.as_ref().map(|r| r.key.as_str())
    }

    /// Play `sfx/<key>` once. Skipped while audio is locked or the effect is not loaded yet.
    pub fn sfx(&mut self, media: &Media, key: &str) {
        let full = format!("sfx/{key}");
        if !self.unlocked {
            // Still request it so it is ready once audio unlocks.
            media.sound_state(&full);
            return;
        }
        if self.sfx_gain <= 0.0 || self.played_this_frame.contains(&full) {
            return;
        }
        if let Some(sound) = media.sound(&full) {
            play_sound(
                &sound,
                PlaySoundParams {
                    looped: false,
                    volume: self.sfx_gain,
                },
            );
            self.played_this_frame.push(full);
        }
    }

    /// Advance fades and start pending music. `user_gesture` is the input snapshot's
    /// "any activity" flag (unlocks audio on the web). Called by the app once per frame.
    pub fn update(&mut self, dt: f32, media: &Media, user_gesture: bool) {
        self.played_this_frame.clear();
        if !self.unlocked {
            if !user_gesture {
                if let Some(w) = &self.wanted {
                    // Load the music meanwhile so it starts right after the gesture.
                    media.sound_state(&format!("bgm/{}", w.key));
                }
                return;
            }
            self.unlocked = true;
        }

        // Fade out music that is no longer wanted.
        let keep = matches!((&self.current, &self.wanted), (Some(t), Some(w)) if t.request == *w && !t.stale);
        if let Some(track) = self.current.as_mut() {
            if !keep {
                track.level -= dt / FADE_OUT;
                if track.level <= 0.0 {
                    stop_sound(&track.sound);
                    let key = format!("bgm/{}", track.request.key);
                    let stale = track.stale;
                    self.current = None;
                    // Decoded music is large; drop it unless it is wanted again right away (and
                    // is still the same file).
                    if stale || self.bgm().map(|k| format!("bgm/{k}")) != Some(key.clone()) {
                        media.release_sound(&key);
                    }
                }
            } else if track.level < 1.0 {
                track.level = (track.level + dt / FADE_IN).min(1.0);
            }
        }

        // Start the wanted music once the old track is gone and the file is loaded.
        if self.current.is_none() {
            if let Some(w) = self.wanted.clone() {
                let key = format!("bgm/{}", w.key);
                match media.sound_state(&key) {
                    AssetState::Ready => {
                        if let Some(sound) = media.sound(&key) {
                            play_sound(
                                &sound,
                                PlaySoundParams {
                                    looped: w.looped,
                                    volume: 0.0,
                                },
                            );
                            self.current = Some(Track {
                                request: w,
                                sound,
                                level: 0.0,
                                applied_volume: 0.0,
                                stale: false,
                            });
                        }
                    }
                    AssetState::Loading => {}
                    AssetState::Missing => {
                        // Logged by the media store; play nothing for this request.
                        if !w.looped {
                            self.wanted = None;
                        }
                    }
                }
            }
        }

        if let Some(track) = self.current.as_mut() {
            let volume = track.level.clamp(0.0, 1.0) * self.bgm_gain;
            if (volume - track.applied_volume).abs() > 0.001 {
                set_sound_volume(&track.sound, volume);
                track.applied_volume = volume;
            }
        }
    }
}
