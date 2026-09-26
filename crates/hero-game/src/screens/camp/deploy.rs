//! 부대 편성: choosing the officers deployed in the next battle (`CampaignState::deployed`).
//!
//! # Rules
//!
//! They mirror how the battle engine builds a deployment (`BattleState::new`):
//!
//! * the lord and the battle's `deploy.required` officers are always deployed (locked in);
//! * `deploy.forbidden` officers cannot be deployed;
//! * at most `deploy.max` officers (and no more than the battle has deploy slots);
//! * slots are filled in a fixed order: required officers (in `deploy.required` order), the lord,
//!   then the other chosen officers in roster order — so what the camp shows is what the battle
//!   places.
//!
//! When nothing has been chosen yet the selection is the engine's default (required, lord, then
//! roster order up to the maximum). A list left over from an earlier battle is normalised to the
//! new battle's rules.

use super::stats::officer_stats;
use super::widgets::LIST_TOP;
use super::widgets::{
    class_name, draw_camp_backdrop, draw_header, draw_help, draw_list_frame, draw_officer_sprite,
    draw_stats_block, officer_name, portrait_key, visible_rows, TOP,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, stroke_rect, Align, FontId, TextStyle};
use crate::ui::art::draw_portrait_card;
use crate::ui::korean::{with_particle, Particle};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_divider, draw_window_ex, WindowStyle};
use hero_core::battledef::BattleDef;
use hero_core::campaign::CampaignState;
use hero_core::data::Id;
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// How an officer takes part in a battle's deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployStatus {
    /// The lord: always deployed.
    Lord,
    /// Listed in `deploy.required`: always deployed.
    Required,
    /// Listed in `deploy.forbidden`.
    Forbidden,
    /// Free to deploy or not.
    Free,
}

impl DeployStatus {
    pub fn locked(self) -> bool {
        matches!(self, DeployStatus::Lord | DeployStatus::Required)
    }

    /// Badge shown next to the officer.
    pub fn badge(self) -> Option<&'static str> {
        match self {
            DeployStatus::Lord => Some("군주"),
            DeployStatus::Required => Some("필수"),
            DeployStatus::Forbidden => Some("출진 불가"),
            DeployStatus::Free => None,
        }
    }
}

/// Most officers that may be deployed in `def`.
pub fn deploy_max(def: &BattleDef) -> usize {
    (def.deploy.max as usize).min(def.deploy.slots.len())
}

/// Status of `officer` in `def` (forbidden wins, as in the engine).
pub fn deploy_status(pack: &Pack, def: &BattleDef, officer: &str) -> DeployStatus {
    if def.deploy.forbidden.iter().any(|f| f == officer) {
        DeployStatus::Forbidden
    } else if pack.officer(officer).is_some_and(|o| o.lord) {
        DeployStatus::Lord
    } else if def.deploy.required.iter().any(|r| r == officer) {
        DeployStatus::Required
    } else {
        DeployStatus::Free
    }
}

/// The canonical deployment for `chosen`: locked officers first (required in `deploy.required`
/// order, then lords in roster order), then the chosen free officers in roster order, at most
/// [`deploy_max`]. Officers not in the army and forbidden officers are dropped.
pub fn normalize(pack: &Pack, def: &BattleDef, campaign: &CampaignState, chosen: &[Id]) -> Vec<Id> {
    let status = |id: &str| deploy_status(pack, def, id);
    let mut out: Vec<Id> = Vec::new();
    for id in &def.deploy.required {
        if campaign.officer(id).is_some() && status(id).locked() && !out.contains(id) {
            out.push(id.clone());
        }
    }
    for o in &campaign.roster {
        if status(&o.id) == DeployStatus::Lord && !out.contains(&o.id) {
            out.push(o.id.clone());
        }
    }
    for o in &campaign.roster {
        if status(&o.id) == DeployStatus::Free && chosen.contains(&o.id) && !out.contains(&o.id) {
            out.push(o.id.clone());
        }
    }
    out.truncate(deploy_max(def));
    out
}

/// The engine's default deployment: required officers, the lord, then roster order.
pub fn default_selection(pack: &Pack, def: &BattleDef, campaign: &CampaignState) -> Vec<Id> {
    let everyone: Vec<Id> = campaign.roster.iter().map(|o| o.id.clone()).collect();
    normalize(pack, def, campaign, &everyone)
}

/// What the camp shows first: the player's earlier choice (normalised to this battle), or the
/// default when there is none.
pub fn initial_selection(pack: &Pack, def: &BattleDef, campaign: &CampaignState) -> Vec<Id> {
    if campaign.deployed.is_empty() {
        default_selection(pack, def, campaign)
    } else {
        normalize(pack, def, campaign, &campaign.deployed)
    }
}

/// Why an officer cannot be toggled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToggleError {
    NotInArmy,
    Locked(DeployStatus),
    Forbidden,
    /// Already `max` officers deployed.
    Full(usize),
}

impl ToggleError {
    /// Player-facing explanation.
    pub fn message(&self, name: &str) -> String {
        let topic = with_particle(name, Particle::EunNeun);
        match self {
            ToggleError::NotInArmy => format!("{topic} 아군에 없습니다."),
            ToggleError::Locked(DeployStatus::Lord) => {
                format!("군주인 {topic} 반드시 출진합니다.")
            }
            ToggleError::Locked(_) => format!("{topic} 이번 전투에 반드시 출진합니다."),
            ToggleError::Forbidden => format!("{topic} 이번 전투에 출진할 수 없습니다."),
            ToggleError::Full(max) => format!("최대 {max}명까지 출진할 수 있습니다."),
        }
    }
}

/// Deploy or withdraw `officer`; returns the new (normalised) selection.
pub fn toggle(
    pack: &Pack,
    def: &BattleDef,
    campaign: &CampaignState,
    selection: &[Id],
    officer: &str,
) -> Result<Vec<Id>, ToggleError> {
    if campaign.officer(officer).is_none() {
        return Err(ToggleError::NotInArmy);
    }
    let status = deploy_status(pack, def, officer);
    if status.locked() {
        return Err(ToggleError::Locked(status));
    }
    if status == DeployStatus::Forbidden {
        return Err(ToggleError::Forbidden);
    }
    let mut chosen: Vec<Id> = selection.to_vec();
    if let Some(i) = chosen.iter().position(|id| id == officer) {
        chosen.remove(i);
    } else {
        let max = deploy_max(def);
        if selection.len() >= max {
            return Err(ToggleError::Full(max));
        }
        chosen.push(officer.to_string());
    }
    Ok(normalize(pack, def, campaign, &chosen))
}

// ----- screen --------------------------------------------------------------------------------

const LIST: Rect = Rect {
    x: 8.0,
    y: TOP + 2.0,
    w: 262.0,
    h: 222.0,
};
const ROW_H: f32 = 24.0;
const PANEL: Rect = Rect {
    x: 276.0,
    y: TOP + 2.0,
    w: 196.0,
    h: 222.0,
};
/// Space left of the name for the check box and the unit sprite.
const TAG_W: f32 = 42.0;

/// The 부대 편성 screen. Leaving it stores the selection in the campaign.
pub struct DeployScreen {
    battle: Id,
    officers: Vec<Id>,
    selection: Vec<Id>,
    menu: Menu,
}

impl DeployScreen {
    pub fn new(battle: &str) -> DeployScreen {
        DeployScreen {
            battle: battle.to_string(),
            officers: Vec::new(),
            selection: Vec::new(),
            menu: Menu::new(Vec::new()),
        }
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let Some(def) = pack.battles.get(&self.battle) else {
            return;
        };
        let campaign = &session.campaign;
        self.officers = campaign.roster.iter().map(|o| o.id.clone()).collect();
        let items = campaign
            .roster
            .iter()
            .map(|o| {
                let status = deploy_status(pack, def, &o.id);
                MenuItem::new(officer_name(pack, &o.id))
                    .detail(format!("Lv{}", o.level))
                    .enabled(status != DeployStatus::Forbidden)
            })
            .collect();
        let cursor = self.menu.cursor();
        let rows = ((LIST.h - LIST_TOP - 4.0) / ROW_H).floor() as usize;
        let mut menu =
            Menu::new(items)
                .rows(rows)
                .at(LIST.x + 2.0, LIST.y + LIST_TOP, LIST.w - 4.0);
        menu.framed = false;
        menu.row_height = ROW_H;
        menu.tag_width = TAG_W;
        menu.wrap = false;
        menu.set_cursor(cursor);
        self.menu = menu;
    }
}

impl Screen for DeployScreen {
    fn name(&self) -> &'static str {
        "camp-deploy"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            if let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) {
                if let Some(def) = pack.battles.get(&self.battle) {
                    self.selection = initial_selection(&pack, def, &session.campaign);
                }
            }
        }
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let Some(pack) = ctx.pack.clone() else {
            return Transition::Pop;
        };
        let Some(def) = pack.battles.get(&self.battle) else {
            return Transition::Pop;
        };
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let Some(id) = self.officers.get(i).cloned() else {
                    return Transition::None;
                };
                let Some(session) = ctx.session.as_ref() else {
                    return Transition::Pop;
                };
                match toggle(&pack, def, &session.campaign, &self.selection, &id) {
                    Ok(selection) => self.selection = selection,
                    Err(e) => {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast(e.message(officer_name(&pack, &id)));
                    }
                }
                Transition::None
            }
            MenuEvent::Cancelled => {
                if let Some(session) = ctx.session.as_mut() {
                    session.campaign.deployed = self.selection.clone();
                }
                Transition::Pop
            }
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let Some(def) = pack.battles.get(&self.battle) else {
            return;
        };
        let campaign = &session.campaign;
        let gfx = &ctx.gfx;
        draw_camp_backdrop(ctx, 0.8);
        draw_header(ctx, &format!("부대 편성 — {}", def.name), campaign.gold);

        // Roster list.
        draw_list_frame(ctx, LIST, "무장", true);
        let max = deploy_max(def);
        gfx.text_aligned(
            &format!("출진 {}/{}", self.selection.len(), max),
            LIST.x,
            LIST.y + 3.0,
            LIST.w - 10.0,
            Align::Right,
            TextStyle::small(if self.selection.len() == max {
                theme::TEXT_NAME
            } else {
                theme::TEXT
            }),
        );
        self.menu.draw(ctx);
        for (i, row) in visible_rows(&self.menu) {
            let Some(o) = campaign.roster.get(i) else {
                continue;
            };
            let chosen = self.selection.contains(&o.id);
            let status = deploy_status(pack, def, &o.id);
            // Check box.
            let b = Rect::new(row.x + 13.0, row.y + 7.0, 10.0, 10.0);
            fill_rect(b, theme::GAUGE_BG);
            stroke_rect(b, theme::BORDER_MID);
            if chosen {
                let c = if status.locked() {
                    theme::TEXT_NAME
                } else {
                    theme::TEXT_GOOD
                };
                fill_rect(Rect::new(b.x + 2.0, b.y + 2.0, 6.0, 6.0), c);
            }
            draw_officer_sprite(
                ctx,
                pack,
                o,
                vec2(row.x + 40.0, row.bottom() - 1.0),
                i == self.menu.cursor(),
            );
            let dim = status == DeployStatus::Forbidden;
            gfx.text(
                class_name(pack, &o.class),
                row.x + 12.0 + TAG_W + 52.0,
                row.y + 4.0,
                TextStyle::main(if dim {
                    theme::TEXT_DISABLED
                } else {
                    theme::TEXT_DIM
                })
                .shadow(theme::TEXT_SHADOW),
            );
            if let Some(badge) = status.badge() {
                gfx.text(
                    badge,
                    row.x + 12.0 + TAG_W + 108.0,
                    row.y + 6.0,
                    TextStyle::small(match status {
                        DeployStatus::Forbidden => theme::TEXT_BAD,
                        _ => theme::TEXT_NAME,
                    }),
                );
            }
        }

        // Selected officer.
        draw_window_ex(PANEL, WindowStyle::Panel, 1.0);
        if let Some(o) = campaign.roster.get(self.menu.cursor()) {
            let x = PANEL.x + 8.0;
            draw_portrait_card(
                ctx,
                Some(portrait_key(pack, &o.id)),
                Rect::new(x, PANEL.y + 8.0, 64.0, 80.0),
                1.0,
                1.0,
            );
            let tx = x + 72.0;
            gfx.text(
                officer_name(pack, &o.id),
                tx,
                PANEL.y + 8.0,
                TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
            );
            gfx.text(
                &format!("{} Lv{}", class_name(pack, &o.class), o.level),
                tx,
                PANEL.y + 25.0,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
            let status = deploy_status(pack, def, &o.id);
            let (text, color) = match (status, self.selection.contains(&o.id)) {
                (DeployStatus::Lord, _) => ("군주 — 반드시 출진", theme::TEXT_NAME),
                (DeployStatus::Required, _) => ("이번 전투 필수 출진", theme::TEXT_NAME),
                (DeployStatus::Forbidden, _) => ("이번 전투 출진 불가", theme::TEXT_BAD),
                (DeployStatus::Free, true) => ("출진", theme::TEXT_GOOD),
                (DeployStatus::Free, false) => ("대기", theme::TEXT_DIM),
            };
            gfx.text(text, tx, PANEL.y + 44.0, TextStyle::small(color));
            if let Some(stats) = officer_stats(pack, o) {
                draw_stats_block(gfx, &stats, None, x, PANEL.y + 94.0, PANEL.w - 16.0);
            }
        }
        draw_divider(PANEL.x + 6.0, PANEL.y + 172.0, PANEL.w - 12.0);
        let order: Vec<&str> = self
            .selection
            .iter()
            .map(|id| officer_name(pack, id))
            .collect();
        gfx.text(
            "출진 순서",
            PANEL.x + 8.0,
            PANEL.y + 177.0,
            TextStyle::small(theme::TEXT_ACCENT),
        );
        let lines = gfx.wrap(&order.join(" · "), FontId::Small, 1, PANEL.w - 16.0);
        gfx.text_lines(
            &lines[..lines.len().min(2)],
            PANEL.x + 8.0,
            PANEL.y + 191.0,
            TextStyle::small(theme::TEXT),
        );
        draw_help(ctx, "Z 출진/대기 전환 · X 편성 완료");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    fn ids(v: &[&str]) -> Vec<Id> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The base pack's first battle with a bigger army and one forbidden officer.
    fn setup() -> (Pack, CampaignState) {
        let mut pack = test_pack();
        let def = pack.battles.get_mut("p1_sishui").unwrap();
        def.deploy.max = 4;
        def.deploy.forbidden = ids(&["cao_cao"]);
        let mut campaign = CampaignState::new_game(&pack);
        campaign.join(&pack, "gongsun_zan").unwrap();
        campaign.join(&pack, "cao_cao").unwrap();
        campaign.join(&pack, "yuan_shao").unwrap();
        (pack, campaign)
    }

    #[test]
    fn statuses() {
        let (pack, _) = setup();
        let def = &pack.battles["p1_sishui"];
        assert_eq!(deploy_status(&pack, def, "liu_bei"), DeployStatus::Lord);
        assert_eq!(deploy_status(&pack, def, "guan_yu"), DeployStatus::Required);
        assert_eq!(
            deploy_status(&pack, def, "cao_cao"),
            DeployStatus::Forbidden
        );
        assert_eq!(deploy_status(&pack, def, "gongsun_zan"), DeployStatus::Free);
        assert_eq!(deploy_max(def), 4);
    }

    #[test]
    fn default_matches_the_engine() {
        let (pack, campaign) = setup();
        let def = &pack.battles["p1_sishui"];
        let default = default_selection(&pack, def, &campaign);
        // Required (in deploy.required order), the lord, then roster order; forbidden skipped.
        assert_eq!(
            default,
            ids(&["guan_yu", "zhang_fei", "liu_bei", "gongsun_zan"])
        );
        // The engine deploys the same officers in the same slots when nothing was chosen.
        let battle = hero_core::battle::BattleState::new(&pack, "p1_sishui", &campaign, 7).unwrap();
        let placed: Vec<Id> = battle
            .units
            .iter()
            .filter(|u| u.side == hero_core::battledef::Side::Player)
            .filter_map(|u| u.officer.clone())
            .collect();
        assert_eq!(placed, default);
        assert_eq!(initial_selection(&pack, def, &campaign), default);
    }

    #[test]
    fn toggling() {
        let (pack, campaign) = setup();
        let def = &pack.battles["p1_sishui"];
        let sel = default_selection(&pack, def, &campaign);
        assert_eq!(
            toggle(&pack, def, &campaign, &sel, "liu_bei"),
            Err(ToggleError::Locked(DeployStatus::Lord))
        );
        assert_eq!(
            toggle(&pack, def, &campaign, &sel, "guan_yu"),
            Err(ToggleError::Locked(DeployStatus::Required))
        );
        assert_eq!(
            toggle(&pack, def, &campaign, &sel, "cao_cao"),
            Err(ToggleError::Forbidden)
        );
        assert_eq!(
            toggle(&pack, def, &campaign, &sel, "yuan_shao"),
            Err(ToggleError::Full(4))
        );
        assert_eq!(
            toggle(&pack, def, &campaign, &sel, "lu_bu"),
            Err(ToggleError::NotInArmy)
        );
        let sel = toggle(&pack, def, &campaign, &sel, "gongsun_zan").unwrap();
        assert_eq!(sel, ids(&["guan_yu", "zhang_fei", "liu_bei"]));
        let sel = toggle(&pack, def, &campaign, &sel, "yuan_shao").unwrap();
        assert_eq!(sel, ids(&["guan_yu", "zhang_fei", "liu_bei", "yuan_shao"]));
        assert!(ToggleError::Full(4).message("원소").contains('4'));
    }

    #[test]
    fn stale_lists_are_normalised() {
        let (pack, mut campaign) = setup();
        let def = &pack.battles["p1_sishui"];
        // A list from another battle: missing the required officers, with a forbidden one,
        // a duplicate and an officer who left.
        campaign.deployed = ids(&["yuan_shao", "cao_cao", "yuan_shao", "lu_bu"]);
        assert_eq!(
            initial_selection(&pack, def, &campaign),
            ids(&["guan_yu", "zhang_fei", "liu_bei", "yuan_shao"])
        );
        // The normalised list is accepted by the engine and placed as shown.
        campaign.deployed = initial_selection(&pack, def, &campaign);
        let battle = hero_core::battle::BattleState::new(&pack, "p1_sishui", &campaign, 7).unwrap();
        let placed: Vec<Id> = battle
            .units
            .iter()
            .filter(|u| u.side == hero_core::battledef::Side::Player)
            .filter_map(|u| u.officer.clone())
            .collect();
        assert_eq!(placed, campaign.deployed);
    }
}
