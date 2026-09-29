//! Executes drama scenes: turns script commands into presentation steps for the frontend
//! and applies their side effects (flags, gold, items, officers joining) to the campaign.

use crate::campaign::{CampaignError, CampaignState};
use crate::pack::Pack;
use crate::script::{Cmd, DuelAct, DuelSide, Scene, SetOp, Slot};
use serde::{Deserialize, Serialize};

/// Commands executed in one [`DramaRunner::next`] call without producing a step before the
/// runner assumes an endless `@goto` loop and ends the scene.
const LOOP_GUARD: usize = 10_000;

/// What the frontend should present next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Background(Option<String>),
    /// Show a picture framed over the background (`gfx/pictures/<key>.png`), `None` clears it.
    Picture(Option<String>),
    Music(Option<String>),
    Sound(String),
    /// Show a portrait (portrait key already resolved from officer ids) in a slot.
    Show {
        portrait: String,
        slot: Slot,
    },
    Hide(Option<Slot>),
    Wait {
        ms: u32,
    },
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
    /// Notification that an officer joined (frontend may show a banner), or came back from
    /// `@away` (`returned`).
    Joined {
        officer: String,
        name: String,
        returned: bool,
    },
    /// Notification of gold/items received.
    Received {
        gold: i64,
        item: Option<String>,
    },
    /// Start a duel scene between two officers (ids as written) over background `bg`.
    Duel {
        left: String,
        right: String,
        bg: Option<String>,
    },
    /// A duel fighter's move; wait until the frontend has played it.
    DuelAct {
        side: DuelSide,
        act: DuelAct,
    },
    /// Close the duel scene.
    DuelEnd,
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
    /// A jump names a label the scene does not define (scenes from `parse_drama` never do).
    #[error("scene `{scene}` has no label `{label}`")]
    UnknownLabel { scene: String, label: String },
    /// A side effect failed, e.g. `@join` of an officer the pack does not define.
    #[error(transparent)]
    Campaign(#[from] CampaignError),
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
        if pack.scene(scene).is_none() {
            return Err(DramaError::UnknownScene(scene.to_string()));
        }
        Ok(DramaRunner {
            scene: scene.to_string(),
            pc: 0,
            pending_choice: None,
            finished: false,
        })
    }

    fn current<'a>(&self, pack: &'a Pack) -> Result<&'a Scene, DramaError> {
        pack.scene(&self.scene)
            .ok_or_else(|| DramaError::UnknownScene(self.scene.clone()))
    }

    fn jump(&mut self, scene: &Scene, label: &str) -> Result<(), DramaError> {
        self.pc = scene
            .label_index(label)
            .ok_or_else(|| DramaError::UnknownLabel {
                scene: scene.id.clone(),
                label: label.to_string(),
            })?;
        Ok(())
    }

    /// Advance to the next presentation step, executing control-flow and side-effect commands
    /// (`@set`, `@if`, `@goto`, `@label`, `@join`, `@leave`, `@gold`, `@item`) on the way.
    /// Guards against infinite `@goto` loops (returns `Step::End` after 10 000 commands without output).
    ///
    /// Speakers and `@show` names are resolved through [`Pack::speaker_officer`] (officer id or
    /// display name); other names are shown as written, and `@show` uses them as portrait keys.
    /// `@join` of an officer already in the army and `@leave` of one who is not change nothing
    /// and show nothing. `@gold` reports the change actually applied (gold is clamped to
    /// `0..=gold_cap`). Once the scene has ended, `next` keeps returning `Step::End`.
    pub fn next(&mut self, pack: &Pack, campaign: &mut CampaignState) -> Result<Step, DramaError> {
        if self.finished {
            return Ok(Step::End);
        }
        if self.pending_choice.is_some() {
            return Err(DramaError::ChoicePending);
        }
        let scene = self.current(pack)?;
        for _ in 0..LOOP_GUARD {
            let Some(cmd) = scene.cmds.get(self.pc) else {
                self.finished = true;
                return Ok(Step::End);
            };
            self.pc += 1;
            let step = match cmd {
                Cmd::Bg(key) => Step::Background(key.clone()),
                Cmd::Picture(key) => Step::Picture(key.clone()),
                Cmd::Bgm(key) => Step::Music(key.clone()),
                Cmd::Sfx(key) => Step::Sound(key.clone()),
                Cmd::Show { who, slot } => Step::Show {
                    portrait: pack
                        .speaker_officer(who)
                        .map_or_else(|| who.clone(), |o| o.portrait_key().to_string()),
                    slot: *slot,
                },
                Cmd::Hide(slot) => Step::Hide(*slot),
                Cmd::Duel { left, right, bg } => Step::Duel {
                    left: left.clone(),
                    right: right.clone(),
                    bg: bg.clone(),
                },
                Cmd::DuelAct { side, act } => Step::DuelAct {
                    side: *side,
                    act: *act,
                },
                Cmd::DuelEnd => Step::DuelEnd,
                Cmd::Wait(ms) => Step::Wait { ms: *ms },
                Cmd::FadeOut => Step::FadeOut,
                Cmd::FadeIn => Step::FadeIn,
                Cmd::Title(text) => Step::Title(text.clone()),
                Cmd::Narr(text) => Step::Narration(text.clone()),
                Cmd::Say { speaker, text } => {
                    let officer = pack.speaker_officer(speaker);
                    Step::Line {
                        speaker: officer.map_or_else(|| speaker.clone(), |o| o.name.clone()),
                        portrait: officer.map(|o| o.portrait_key().to_string()),
                        text: text.clone(),
                    }
                }
                Cmd::Choice(options) => {
                    self.pending_choice = Some(options.iter().map(|o| o.label.clone()).collect());
                    Step::Choice(options.iter().map(|o| o.text.clone()).collect())
                }
                Cmd::Label(_) => continue,
                Cmd::Goto(label) => {
                    self.jump(scene, label)?;
                    continue;
                }
                Cmd::If { cond, label } => {
                    if cond.cmp.eval(campaign.flag(&cond.flag), cond.value) {
                        self.jump(scene, label)?;
                    }
                    continue;
                }
                Cmd::Set { flag, op, value } => {
                    let old = campaign.flag(flag);
                    let new = match op {
                        SetOp::Assign => *value,
                        SetOp::Add => old.saturating_add(*value),
                        SetOp::Sub => old.saturating_sub(*value),
                    };
                    campaign.flags.insert(flag.clone(), new);
                    continue;
                }
                Cmd::Join(officer) => {
                    if campaign.officer(officer).is_some_and(|o| !o.away) {
                        continue;
                    }
                    let returned = campaign.officer(officer).is_some();
                    campaign.join(pack, officer)?;
                    let name = pack
                        .officer(officer)
                        .map_or_else(|| officer.clone(), |o| o.name.clone());
                    Step::Joined {
                        officer: officer.clone(),
                        name,
                        returned,
                    }
                }
                Cmd::Away(officer) => {
                    match campaign.set_away(officer) {
                        Ok(()) | Err(CampaignError::NotInArmy(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                // (An officer not in the army is left as they are, as `@away` does.)
                Cmd::Level { officer, levels } => {
                    match campaign.add_levels(pack, officer, *levels) {
                        Ok(()) | Err(CampaignError::NotInArmy(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                Cmd::Class { officer, class } => {
                    match campaign.set_class(pack, officer, class) {
                        Ok(()) | Err(CampaignError::NotInArmy(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                Cmd::Leave(officer) => {
                    match campaign.leave(officer) {
                        Ok(()) | Err(CampaignError::NotInArmy(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                    continue;
                }
                Cmd::Gold(amount) => {
                    let before = campaign.gold;
                    campaign.add_gold(pack, *amount);
                    Step::Received {
                        gold: campaign.gold - before,
                        item: None,
                    }
                }
                Cmd::Item(item) => {
                    if pack.item(item).is_none() {
                        return Err(CampaignError::UnknownItem(item.clone()).into());
                    }
                    campaign.add_item(item, 1);
                    Step::Received {
                        gold: 0,
                        item: Some(item.clone()),
                    }
                }
                Cmd::End => {
                    self.finished = true;
                    Step::End
                }
            };
            return Ok(step);
        }
        self.finished = true;
        Ok(Step::End)
    }

    /// Resolve a pending choice by index; execution continues at the chosen label.
    pub fn choose(&mut self, pack: &Pack, index: usize) -> Result<(), DramaError> {
        let labels = self
            .pending_choice
            .as_ref()
            .ok_or(DramaError::NoChoicePending)?;
        let label = labels
            .get(index)
            .ok_or(DramaError::BadChoice(index))?
            .clone();
        let scene = self.current(pack)?;
        self.jump(scene, &label)?;
        self.pending_choice = None;
        Ok(())
    }
}
