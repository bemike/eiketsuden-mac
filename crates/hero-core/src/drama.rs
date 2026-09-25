//! Executes drama scenes: turns script commands into presentation steps for the frontend
//! and applies their side effects (flags, gold, items, officers joining) to the campaign.

use crate::campaign::CampaignState;
use crate::pack::Pack;
use crate::script::Slot;
use serde::{Deserialize, Serialize};

/// What the frontend should present next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Background(Option<String>),
    Music(Option<String>),
    Sound(String),
    /// Show a portrait (portrait key already resolved from officer ids) in a slot.
    Show { portrait: String, slot: Slot },
    Hide(Option<Slot>),
    Wait { ms: u32 },
    FadeOut,
    FadeIn,
    Title(String),
    /// Narration box; wait for the player to continue.
    Narration(String),
    /// Dialogue box; wait for the player to continue. `portrait` is set when the speaker is
    /// an officer with a portrait key.
    Line {
        speaker: String,
        portrait: Option<String>,
        text: String,
    },
    /// Offer choices; the frontend must call [`DramaRunner::choose`] before `next` again.
    Choice(Vec<String>),
    /// Notification that an officer joined (frontend may show a banner).
    Joined { officer: String, name: String },
    /// Notification of gold/items received.
    Received { gold: i64, item: Option<String> },
    /// Scene finished.
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DramaError {
    #[error("unknown scene `{0}`")]
    UnknownScene(String),
    #[error("a choice is pending; call choose() first")]
    ChoicePending,
    #[error("no choice is pending")]
    NoChoicePending,
    #[error("choice index {0} out of range")]
    BadChoice(usize),
}

/// Cursor into a scene. Serializable so a save made during a drama could resume it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DramaRunner {
    pub scene: String,
    pub pc: usize,
    /// Labels of the currently offered choice, if any.
    pub pending_choice: Option<Vec<String>>,
    pub finished: bool,
}

impl DramaRunner {
    pub fn new(pack: &Pack, scene: &str) -> Result<DramaRunner, DramaError> {
        let _ = pack;
        todo!("W1b: DramaRunner::new {scene}")
    }

    /// Advance to the next presentation step, executing control-flow and side-effect commands
    /// (`@set`, `@if`, `@goto`, `@label`, `@join`, `@leave`, `@gold`, `@item`) on the way.
    /// Guards against infinite `@goto` loops (returns `Step::End` after 10 000 commands without output).
    pub fn next(&mut self, pack: &Pack, campaign: &mut CampaignState) -> Result<Step, DramaError> {
        let _ = (pack, campaign);
        todo!("W1b: DramaRunner::next")
    }

    /// Resolve a pending choice by index; execution continues at the chosen label.
    pub fn choose(&mut self, pack: &Pack, index: usize) -> Result<(), DramaError> {
        let _ = pack;
        todo!("W1b: DramaRunner::choose {index}")
    }
}
