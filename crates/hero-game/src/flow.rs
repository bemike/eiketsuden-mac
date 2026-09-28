//! Game flow: title → new game / continue → campaign nodes → ending.
//!
//! Screens move through the game by returning [`crate::app::Transition::Flow`] with a [`Flow`];
//! [`enter`] turns it into the next root screen (the stack is replaced).
//!
//! The campaign being played lives in [`crate::app::Ctx::session`] as a [`Session`]
//! (`hero_core` `CampaignState` plus an optional battle in progress). Each campaign node kind has
//! a screen:
//!
//! | node | screen | returns when done |
//! |---|---|---|
//! | `Drama` | drama screen (plays `scene`) | `Flow::Advance` |
//! | `Camp` | camp screen (shop, equipment, deploy, save) | `Flow::Advance` |
//! | `Battle` | battle screen | `Flow::BattleEnded(state)` |
//! | `Ending` | drama screen for `scene` (if any), then | `Flow::Ending { title }` |
//! | `Branch` | — resolved by `CampaignState::advance` | — |
//!
//! # Campaign screens
//!
//! [`node_screen`] (and [`battle_screen`] for resuming a mid-battle save) is the **single place**
//! where the drama, camp and battle screens are wired in.
//!
//! # Autosave
//!
//! Moving to another node ([`Flow::Advance`], a victory, a defeat with `on_defeat`) writes the
//! autosave slot, **except when the new node is an `Ending`**: the autosave then keeps the last
//! point before it, so 이어하기 (continue) retries the scene or battle that led to a bad ending
//! instead of replaying the ending.

use crate::app::{Ctx, Screen};
use crate::assets::Media;
use crate::platform::{memfs, unix_now, DataRoot};
use crate::saves::{self, SaveSlot};
use crate::screens::camp::CampScreen;
use crate::screens::credits::CreditsScreen;
use crate::screens::drama::DramaScreen;
use crate::screens::error::ErrorScreen;
use crate::screens::gameover::GameOverScreen;
use crate::screens::loading::{LoadingScreen, Target};
use crate::screens::title::TitleScreen;
use crate::secret::ForbiddenSecret;
use hero_core::battle::{BattleState, Outcome};
use hero_core::campaign::{CampaignError, CampaignState, Node};
use hero_core::pack::Pack;
use hero_core::save::{SaveGame, SAVE_VERSION};
use std::rc::Rc;

/// A step in the game flow. See the module docs.
pub enum Flow {
    /// The title screen (ends the current session).
    Title,
    /// Start a new campaign at the pack's start node.
    NewGame,
    /// Resume a save: its battle if it was saved mid-battle, otherwise its campaign node.
    Continue(Box<SaveGame>),
    /// Show the screen of the session's current campaign node.
    Node,
    /// The current node is finished: advance the campaign (resolving branches), autosave (see
    /// the module docs), and show the next node.
    Advance,
    /// The battle screen finished a battle. Victory applies the result and advances; defeat
    /// applies the result too (flags, levels, used consumables — MODDING.md) and goes to the
    /// node's `on_defeat`, or ends the campaign on the game over screen when there is none.
    BattleEnded(Box<BattleState>),
    /// The campaign was lost.
    GameOver,
    /// The campaign reached an ending: ending credits, then the title screen.
    Ending { title: String },
    /// Load the data pack again from the start (ends the session): the original mode was
    /// switched on or off, or its folder changed. Drops the pack converted in memory.
    Reload,
}

/// The campaign being played.
#[derive(Debug, Clone)]
pub struct Session {
    pub campaign: CampaignState,
    /// A battle in progress (set by the battle screen; stored in mid-battle saves).
    pub battle: Option<BattleState>,
    /// Fraction of a second not yet added to `campaign.play_seconds`.
    play_fraction: f32,
    /// The original's hidden command (not saved: a new session starts over).
    pub secret: ForbiddenSecret,
}

impl Session {
    pub fn new(campaign: CampaignState) -> Session {
        Session {
            campaign,
            battle: None,
            play_fraction: 0.0,
            secret: ForbiddenSecret::default(),
        }
    }

    pub fn from_save(save: SaveGame) -> Session {
        Session {
            campaign: save.campaign,
            battle: save.battle,
            play_fraction: 0.0,
            secret: ForbiddenSecret::default(),
        }
    }

    /// Count play time (called every frame by the app while a session exists).
    pub fn tick(&mut self, dt: f32) {
        self.play_fraction += dt.max(0.0);
        if self.play_fraction >= 1.0 {
            let whole = self.play_fraction.floor();
            self.campaign.play_seconds += whole as u64;
            self.play_fraction -= whole;
        }
    }

    /// Snapshot for a save slot.
    pub fn to_save(&self, pack: &Pack) -> SaveGame {
        SaveGame {
            version: SAVE_VERSION,
            pack_id: pack.manifest.id.clone(),
            pack_version: pack.manifest.version.clone(),
            label: save_label(pack, &self.campaign, self.battle.as_ref()),
            saved_at: unix_now(),
            campaign: self.campaign.clone(),
            battle: self.battle.clone(),
        }
    }
}

/// Human readable summary of where the campaign is, for the save slot list.
pub fn save_label(pack: &Pack, campaign: &CampaignState, battle: Option<&BattleState>) -> String {
    let battle_name = |id: &str| {
        pack.battles
            .get(id)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    if let Some(b) = battle {
        return format!("{} · {}턴", battle_name(&b.battle_id), b.turn);
    }
    match pack.campaign.node(&campaign.node) {
        Some(Node::Camp { title, battle, .. }) => {
            if !title.is_empty() {
                title.clone()
            } else if let Some(b) = battle {
                format!("{} 준비", battle_name(b))
            } else {
                "출진 준비".into()
            }
        }
        Some(Node::Battle { battle, .. }) => battle_name(battle),
        Some(Node::Ending { title, .. }) if !title.is_empty() => title.clone(),
        Some(Node::Ending { .. }) => "엔딩".into(),
        Some(Node::Drama { .. }) | Some(Node::Branch { .. }) | None => pack.campaign.title.clone(),
    }
}

/// Write the session into the autosave slot. Failures are reported with a toast.
pub fn autosave(ctx: &mut Ctx) {
    let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
        return;
    };
    let save = session.to_save(&pack);
    if let Err(e) = saves::write(ctx.storage.as_mut(), SaveSlot::Auto, &save) {
        macroquad::logging::error!("autosave failed: {}", e);
        ctx.toast(format!("자동 기록 실패: {e}"));
    }
}

/// Turn a flow step into the new root screen.
pub fn enter(flow: Flow, ctx: &mut Ctx) -> Box<dyn Screen> {
    match flow {
        Flow::Title => {
            ctx.session = None;
            Box::new(TitleScreen::new())
        }
        Flow::NewGame => {
            let Some(pack) = ctx.pack.clone() else {
                return no_pack();
            };
            ctx.session = Some(Session::new(CampaignState::new_game(&pack)));
            show_current_node(ctx, &pack)
        }
        Flow::Continue(save) => {
            let Some(pack) = ctx.pack.clone() else {
                return no_pack();
            };
            let session = Session::from_save(*save);
            let mid_battle = session.battle.is_some();
            ctx.session = Some(session);
            if mid_battle {
                battle_screen(ctx, &pack)
            } else {
                show_current_node(ctx, &pack)
            }
        }
        Flow::Node => match ctx.pack.clone() {
            Some(pack) if ctx.session.is_some() => show_current_node(ctx, &pack),
            Some(_) => no_session(),
            None => no_pack(),
        },
        Flow::Advance => match ctx.pack.clone() {
            Some(pack) if ctx.session.is_some() => advance(ctx, &pack),
            Some(_) => no_session(),
            None => no_pack(),
        },
        Flow::BattleEnded(state) => {
            let Some(pack) = ctx.pack.clone() else {
                return no_pack();
            };
            battle_ended(ctx, &pack, *state)
        }
        Flow::GameOver => {
            ctx.session = None;
            Box::new(GameOverScreen::new())
        }
        Flow::Ending { title } => {
            ctx.session = None;
            Box::new(CreditsScreen::ending(title))
        }
        Flow::Reload => {
            ctx.session = None;
            ctx.pack = None;
            ctx.audio.stop_bgm();
            memfs::unmount();
            ctx.data_root = DataRoot::resolve(&ctx.options);
            ctx.media = Media::new(ctx.data_root.clone());
            Box::new(LoadingScreen::new(Target::Game))
        }
    }
}

fn show_current_node(ctx: &mut Ctx, pack: &Rc<Pack>) -> Box<dyn Screen> {
    let Some(session) = ctx.session.as_ref() else {
        return no_session();
    };
    let id = session.campaign.node.clone();
    match pack.campaign.node(&id) {
        // A branch is never shown; resolving it is the same as advancing past it.
        Some(Node::Branch { .. }) => advance(ctx, pack),
        Some(node) => node_screen(ctx, pack, &node.clone()),
        None => Box::new(ErrorScreen::recoverable(
            "캠페인 오류",
            vec![format!(
                "캠페인 노드 `{id}`를 찾을 수 없습니다. 데이터 팩을 확인하세요."
            )],
        )),
    }
}

fn advance(ctx: &mut Ctx, pack: &Rc<Pack>) -> Box<dyn Screen> {
    let Some(session) = ctx.session.as_mut() else {
        return no_session();
    };
    match session.campaign.advance(pack) {
        Ok(_) => arrive(ctx, pack),
        Err(e) => Box::new(ErrorScreen::recoverable(
            "캠페인 오류",
            vec![format!("다음 단계로 진행할 수 없습니다: {e}")],
        )),
    }
}

/// The session's campaign has just moved to another node: autosave (unless the node is an
/// ending, see the module docs) and show it.
fn arrive(ctx: &mut Ctx, pack: &Rc<Pack>) -> Box<dyn Screen> {
    let save = ctx
        .session
        .as_ref()
        .is_some_and(|s| autosaves_at(pack, &s.campaign.node));
    if save {
        autosave(ctx);
    }
    show_current_node(ctx, pack)
}

/// Whether arriving at campaign node `id` writes the autosave: every node except an `Ending`.
fn autosaves_at(pack: &Pack, id: &str) -> bool {
    !matches!(pack.campaign.node(id), Some(Node::Ending { .. }))
}

/// A lost battle whose node has an `on_defeat` link: keep what the battle changed (flags set by
/// its events, levels, EXP, classes, equipment, used consumables — `apply_battle_result` adds
/// found gold and items only for a victory), then jump to `on_defeat`, resolving branches with
/// the updated flags. Returns the node that became current.
fn apply_defeat(
    campaign: &mut CampaignState,
    pack: &Pack,
    battle: &BattleState,
    on_defeat: &str,
) -> Result<String, CampaignError> {
    campaign.apply_battle_result(pack, battle);
    campaign.jump(pack, on_defeat)
}

fn battle_ended(ctx: &mut Ctx, pack: &Rc<Pack>, state: BattleState) -> Box<dyn Screen> {
    let Some(session) = ctx.session.as_mut() else {
        return no_session();
    };
    session.battle = None;
    match state.outcome {
        Some(Outcome::Victory) => {
            session.campaign.apply_battle_result(pack, &state);
            advance(ctx, pack)
        }
        Some(Outcome::Defeat(_)) => {
            let on_defeat = match pack.campaign.node(&session.campaign.node) {
                Some(Node::Battle { on_defeat, .. }) => on_defeat.clone(),
                _ => None,
            };
            match on_defeat {
                Some(node) => match apply_defeat(&mut session.campaign, pack, &state, &node) {
                    Ok(_) => arrive(ctx, pack),
                    Err(e) => Box::new(ErrorScreen::recoverable(
                        "캠페인 오류",
                        vec![format!("패배 후 진행할 수 없습니다: {e}")],
                    )),
                },
                None => {
                    ctx.session = None;
                    Box::new(GameOverScreen::new())
                }
            }
        }
        None => Box::new(ErrorScreen::recoverable(
            "전투 오류",
            vec!["전투가 끝나지 않은 상태로 종료되었습니다.".into()],
        )),
    }
}

/// **Plug-in point**: the screen for a campaign node (see the module docs).
pub fn node_screen(ctx: &mut Ctx, pack: &Rc<Pack>, node: &Node) -> Box<dyn Screen> {
    let _ = pack;
    match node {
        Node::Drama { scene, .. } => Box::new(DramaScreen::node(ctx, scene)),
        Node::Camp {
            title,
            shop,
            battle,
            ..
        } => Box::new(CampScreen::new(title, shop, battle.as_deref())),
        Node::Battle { battle, .. } => crate::screens::battle::BattleScreen::start(ctx, battle),
        Node::Ending {
            scene: None, title, ..
        } => Box::new(CreditsScreen::ending(title.clone())),
        Node::Ending {
            scene: Some(scene),
            title,
            ..
        } => Box::new(DramaScreen::ending(ctx, scene, title.clone())),
        Node::Branch { id, .. } => Box::new(ErrorScreen::recoverable(
            "캠페인 오류",
            vec![format!("분기 노드 `{id}`는 화면을 가질 수 없습니다.")],
        )),
    }
}

/// **Plug-in point**: resume the battle stored in the session (mid-battle save).
pub fn battle_screen(ctx: &mut Ctx, pack: &Rc<Pack>) -> Box<dyn Screen> {
    let _ = pack;
    match ctx.session.as_ref().and_then(|s| s.battle.as_ref()) {
        Some(_) => crate::screens::battle::BattleScreen::resume(ctx),
        None => no_session(),
    }
}

fn no_pack() -> Box<dyn Screen> {
    Box::new(ErrorScreen::recoverable(
        "데이터 팩 없음",
        vec![
            "데이터 팩이 로드되지 않았습니다. (UI 갤러리 모드에서는 게임을 시작할 수 없습니다.)"
                .into(),
        ],
    ))
}

fn no_session() -> Box<dyn Screen> {
    Box::new(ErrorScreen::recoverable(
        "진행 중인 게임 없음",
        vec!["진행 중인 캠페인이 없습니다.".into()],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn campaign() -> CampaignState {
        CampaignState {
            node: "n".into(),
            roster: Vec::new(),
            inventory: BTreeMap::new(),
            gold: 0,
            flags: BTreeMap::new(),
            deployed: Vec::new(),
            battles_won: Vec::new(),
            play_seconds: 10,
        }
    }

    #[test]
    fn arriving_at_an_ending_keeps_the_previous_autosave() {
        let pack = crate::screens::camp::test_pack();
        assert!(autosaves_at(&pack, "c1_jade_belt"));
        assert!(autosaves_at(&pack, "c1_camp_guangling"));
        assert!(!autosaves_at(&pack, "c1_bad_end"));
        let endings: Vec<&str> = pack
            .campaign
            .nodes
            .iter()
            .filter(|n| matches!(n, Node::Ending { .. }))
            .map(|n| n.id())
            .collect();
        assert!(!endings.is_empty());
        assert!(endings.iter().all(|id| !autosaves_at(&pack, id)));

        // The jade-belt scene: a careless answer resolves the branch to the bad ending, which
        // must not replace the autosave made at the scene (the retry point).
        let mut campaign = CampaignState::new_game(&pack);
        campaign.node = "c1_jade_belt".into();
        campaign.flags.insert("c1_exposed".into(), 1);
        let next = campaign.advance(&pack).unwrap();
        assert_eq!(next, "c1_bad_end");
        assert!(!autosaves_at(&pack, &next));
        campaign.node = "c1_jade_belt".into();
        campaign.flags.insert("c1_exposed".into(), 0);
        let next = campaign.advance(&pack).unwrap();
        assert_eq!(next, "c1_camp_guangling");
        assert!(autosaves_at(&pack, &next));
    }

    #[test]
    fn a_defeat_keeps_the_battle_result_and_follows_on_defeat() {
        use hero_core::battle::DefeatReason;
        use hero_core::battledef::Side;

        let pack = crate::screens::camp::test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        campaign.add_item("bean", 3);
        let gold = campaign.gold;
        let mut battle = BattleState::new(&pack, "p1_sishui", &campaign, 7).unwrap();
        battle.outcome = Some(Outcome::Defeat(DefeatReason::LordRetreated));
        // During the lost battle: an event set a flag, one bean was used, an officer levelled
        // up, and gold and an item were found (those are kept only after a victory).
        battle.flags.insert("c1_exposed".into(), 1);
        battle.inventory.insert("bean".into(), 2);
        battle.items_used.insert("bean".into(), 1);
        let unit = battle
            .units
            .iter_mut()
            .find(|u| u.side == Side::Player && u.officer.is_some())
            .expect("a deployed officer");
        unit.level += 1;
        let (officer, level) = (unit.officer.clone().unwrap(), unit.level);
        battle.gold_found = 300;
        battle.items_found.push("wine".into());

        let before = campaign.clone();
        let mut broken = campaign.clone();
        assert!(apply_defeat(&mut broken, &pack, &battle, "no_such_node").is_err());
        assert_eq!(broken.node, before.node);

        // `on_defeat` leads to a branch on the flag the battle set.
        let node = apply_defeat(&mut campaign, &pack, &battle, "c1_br_exposed").unwrap();
        assert_eq!(node, "c1_bad_end");
        assert_eq!(campaign.node, "c1_bad_end");
        assert_eq!(campaign.flag("c1_exposed"), 1);
        assert_eq!(campaign.item_count("bean"), 2);
        let state = campaign.roster.iter().find(|o| o.id == officer).unwrap();
        assert_eq!(state.level, level);
        assert_eq!(campaign.gold, gold);
        assert_eq!(campaign.item_count("wine"), before.item_count("wine"));
        assert!(!campaign.battles_won.contains(&battle.battle_id));
    }

    #[test]
    fn play_time_accumulates_whole_seconds() {
        let mut s = Session::new(campaign());
        for _ in 0..90 {
            s.tick(1.0 / 60.0);
        }
        assert_eq!(s.campaign.play_seconds, 11);
        s.tick(2.75);
        assert_eq!(s.campaign.play_seconds, 14);
        s.tick(-5.0);
        assert_eq!(s.campaign.play_seconds, 14);
    }
}
