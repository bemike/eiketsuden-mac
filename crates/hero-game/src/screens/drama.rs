//! Drama screen: plays a drama scene with `hero_core::drama::DramaRunner` — backgrounds,
//! portraits, dialogue and narration boxes, choices, title cards, fades, music and sounds.
//!
//! **Contract shared by the campaign screens (camp/drama owner) and the battle screen:**
//! the three constructors below are the only public surface other screens rely on.

use crate::app::{Ctx, Screen, Transition};

/// What happens when the scene ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DramaEnd {
    /// Campaign `Drama` node: `Transition::Flow(Flow::Advance)`.
    Advance,
    /// Campaign `Ending` node with a scene: `Transition::Flow(Flow::Ending { title })`.
    Ending { title: String },
    /// Overlay (battle intro/outro, `BattleEvent::Drama`): `Transition::Pop`, so the screen below
    /// resumes (it receives `Enter::Resumed`).
    Pop,
}

/// Plays one drama scene. Side effects (`@set`, `@join`, `@gold`, `@item` ...) are applied to the
/// session's `CampaignState` as the runner reaches them.
pub struct DramaScreen {
    scene: String,
    end: DramaEnd,
}

impl DramaScreen {
    /// Campaign `Drama` node.
    pub fn node(ctx: &mut Ctx, scene: &str) -> DramaScreen {
        let _ = ctx;
        DramaScreen {
            scene: scene.to_string(),
            end: DramaEnd::Advance,
        }
    }

    /// Campaign `Ending` node that has a scene.
    pub fn ending(ctx: &mut Ctx, scene: &str, title: String) -> DramaScreen {
        let _ = ctx;
        DramaScreen {
            scene: scene.to_string(),
            end: DramaEnd::Ending { title },
        }
    }

    /// Overlay drawn above the screen that pushed it (`is_overlay() == true`); pops itself when
    /// the scene ends. Used by the battle screen for intro/outro scenes and battle events.
    pub fn overlay(ctx: &mut Ctx, scene: &str) -> DramaScreen {
        let _ = ctx;
        DramaScreen {
            scene: scene.to_string(),
            end: DramaEnd::Pop,
        }
    }
}

impl Screen for DramaScreen {
    fn name(&self) -> &'static str {
        "drama"
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let _ = ctx;
        todo!("W2B: drama screen update for scene {} ({:?})", self.scene, self.end)
    }

    fn draw(&self, ctx: &Ctx) {
        let _ = ctx;
        todo!("W2B: drama screen draw")
    }

    fn is_overlay(&self) -> bool {
        self.end == DramaEnd::Pop
    }
}
