//! The original's hidden command ("금단의 비법"): tapping the lord's portrait many times opens
//! a prompt; after a yes, a small orb appears whose tap raises the lord to the level cap
//! ([`hero_core::campaign::CampaignState::forbidden_secret`]).
//!
//! The counting follows the PC game (`MAIN.EXE`, docs/RULES.md §13): the 44th tap plays a
//! chime and arms the prompt, the 9th tap after that asks. A "no" disarms it but keeps the
//! count, so the chime comes again only after the count climbs back to 44. The state lives in
//! the [`crate::flow::Session`]: it is not saved, and a new game or a loaded save starts over.

/// Taps on the lord's portrait that arm the prompt (with a chime).
pub const ARM_TAPS: u32 = 44;
/// Taps after arming that open the prompt.
pub const ASK_TAPS: u32 = 9;

/// What a tap on the lord's portrait does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStep {
    /// Nothing visible.
    None,
    /// The prompt is armed: play the chime.
    Chime,
    /// Open the prompt.
    Ask,
}

/// Counter of the hidden command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForbiddenSecret {
    taps: u32,
    armed: bool,
    enabled: bool,
}

impl ForbiddenSecret {
    /// A tap on the lord's portrait.
    pub fn tap(&mut self) -> SecretStep {
        if self.enabled {
            return SecretStep::None;
        }
        self.taps = self.taps.saturating_add(1);
        if !self.armed {
            if self.taps == ARM_TAPS {
                self.armed = true;
                self.taps = 0;
                return SecretStep::Chime;
            }
        } else if self.taps == ASK_TAPS {
            return SecretStep::Ask;
        }
        SecretStep::None
    }

    /// The answer to the prompt: yes enables the orb, no disarms (the count stays).
    pub fn answer(&mut self, yes: bool) {
        if yes {
            self.enabled = true;
        } else {
            self.armed = false;
        }
    }

    /// The orb is shown.
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taps(s: &mut ForbiddenSecret, n: u32) -> Vec<SecretStep> {
        (0..n)
            .map(|_| s.tap())
            .filter(|&t| t != SecretStep::None)
            .collect()
    }

    #[test]
    fn follows_the_original_count() {
        let mut s = ForbiddenSecret::default();
        assert_eq!(taps(&mut s, ARM_TAPS - 1), []);
        assert_eq!(s.tap(), SecretStep::Chime);
        assert_eq!(taps(&mut s, ASK_TAPS - 1), []);
        assert_eq!(s.tap(), SecretStep::Ask);
        s.answer(true);
        assert!(s.enabled());
        assert_eq!(taps(&mut s, 100), [], "done once enabled");
    }

    #[test]
    fn a_no_keeps_the_count_and_needs_the_chime_again() {
        let mut s = ForbiddenSecret::default();
        assert_eq!(
            taps(&mut s, ARM_TAPS + ASK_TAPS),
            [SecretStep::Chime, SecretStep::Ask]
        );
        s.answer(false);
        assert!(!s.enabled());
        // The count went on from 9: 35 more taps reach 44 again.
        assert_eq!(taps(&mut s, ARM_TAPS - ASK_TAPS - 1), []);
        assert_eq!(s.tap(), SecretStep::Chime);
        assert_eq!(taps(&mut s, ASK_TAPS), [SecretStep::Ask]);
    }
}
