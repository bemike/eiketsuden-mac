//! **Development-only** stand-in for campaign screens that are not wired into this build yet.
//!
//! [`crate::flow::node_screen`] and [`crate::flow::battle_screen`] return a
//! [`PlaceholderScreen`] for every campaign node kind whose real screen (drama, camp, battle)
//! has not been integrated. It states clearly that the screen is not implemented in this build,
//! shows what the node contains and offers developer shortcuts so the campaign flow can be walked
//! end to end:
//!
//! * **다음 단계로** — `Flow::Advance` (a skipped battle is *not* counted as won: nothing is
//!   applied to the campaign);
//! * **엔딩으로** — `Flow::Ending` for ending nodes;
//! * **전투 기록 버리기** — drop a mid-battle save's battle and show its campaign node again;
//! * **기록하기** — the save slot screen with the current session;
//! * **타이틀로** — back to the title screen.
//!
//! The integrator replaces the placeholder arms in `flow.rs` with the real screens; once every
//! node kind is wired this screen is unused and can be deleted together with those arms.

use super::backdrop::draw_backdrop;
use super::saveload::SaveLoadScreen;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, FontId, TextStyle, SCREEN, VIRTUAL_W};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_divider, draw_title_bar, draw_window};
use hero_core::campaign::Node;
use macroquad::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Advance,
    Ending(String),
    AbandonBattle,
    Save,
    Title,
}

impl Action {
    fn label(&self) -> &'static str {
        match self {
            Action::Advance => "다음 단계로 (개발용)",
            Action::Ending(_) => "엔딩으로 (개발용)",
            Action::AbandonBattle => "전투 기록 버리기 (개발용)",
            Action::Save => "기록하기",
            Action::Title => "타이틀로",
        }
    }
}

const PANEL: Rect = Rect {
    x: 40.0,
    y: 34.0,
    w: 400.0,
    h: 118.0,
};

/// See the module docs.
pub struct PlaceholderScreen {
    kind: String,
    details: Vec<String>,
    wrapped: Vec<String>,
    actions: Vec<Action>,
    menu: Menu,
}

impl PlaceholderScreen {
    /// Stand-in for the screen of campaign node `node`.
    pub fn for_node(node: &Node) -> PlaceholderScreen {
        let (kind, details, actions) = match node {
            Node::Drama { id, scene, next } => (
                "대화 장면 (Drama)",
                vec![
                    format!("노드 `{id}` · 장면 `{scene}`"),
                    format!("다음 노드 `{next}`"),
                ],
                vec![Action::Advance, Action::Save, Action::Title],
            ),
            Node::Camp {
                id,
                title,
                shop,
                battle,
                next,
            } => (
                "출진 준비 (Camp)",
                vec![
                    format!("노드 `{id}` · {}", if title.is_empty() { "(제목 없음)" } else { title }),
                    format!(
                        "상점 물품 {}종 · 배치할 전투 {}",
                        shop.len(),
                        battle.as_deref().map_or("없음".to_string(), |b| format!("`{b}`"))
                    ),
                    format!("다음 노드 `{next}`"),
                ],
                vec![Action::Advance, Action::Save, Action::Title],
            ),
            Node::Battle {
                id,
                battle,
                next,
                on_defeat,
            } => (
                "전투 (Battle)",
                vec![
                    format!("노드 `{id}` · 전투 `{battle}`"),
                    format!(
                        "승리 시 `{next}` · 패배 시 {}",
                        on_defeat
                            .as_deref()
                            .map_or("게임 오버".to_string(), |n| format!("`{n}`"))
                    ),
                    "‘다음 단계로’는 전투를 건너뜁니다 (승리로 기록되지 않음).".into(),
                ],
                vec![Action::Advance, Action::Save, Action::Title],
            ),
            Node::Ending { id, scene, title } => (
                "엔딩 (Ending)",
                vec![
                    format!(
                        "노드 `{id}` · 장면 {}",
                        scene.as_deref().map_or("없음".to_string(), |s| format!("`{s}`"))
                    ),
                    format!("엔딩 제목: {}", if title.is_empty() { "(없음)" } else { title }),
                ],
                vec![Action::Ending(title.clone()), Action::Title],
            ),
            Node::Branch { id, .. } => (
                "분기 (Branch)",
                vec![format!("노드 `{id}` — 분기는 진행 시 자동으로 해석됩니다.")],
                vec![Action::Advance, Action::Title],
            ),
        };
        PlaceholderScreen::build(kind, details, actions)
    }

    /// Stand-in for resuming the battle of a mid-battle save.
    pub fn for_battle_resume(battle_id: &str) -> PlaceholderScreen {
        PlaceholderScreen::build(
            "전투 이어하기 (Battle)",
            vec![
                format!("진행 중이던 전투 `{battle_id}`"),
                "‘전투 기록 버리기’는 전투 상태를 지우고 현재 캠페인 노드를 다시 보여 줍니다.".into(),
            ],
            vec![Action::AbandonBattle, Action::Title],
        )
    }

    fn build(kind: &str, details: Vec<String>, actions: Vec<Action>) -> PlaceholderScreen {
        let items = actions.iter().map(|a| MenuItem::new(a.label())).collect();
        PlaceholderScreen {
            kind: kind.to_string(),
            details,
            wrapped: Vec::new(),
            actions,
            menu: Menu::new(items).cancellable(false),
        }
    }

    fn perform(&mut self, ctx: &mut Ctx, action: Action) -> Transition {
        match action {
            Action::Advance => Transition::Flow(Flow::Advance),
            Action::Ending(title) => Transition::Flow(Flow::Ending { title }),
            Action::AbandonBattle => {
                if let Some(session) = ctx.session.as_mut() {
                    session.battle = None;
                }
                Transition::Flow(Flow::Node)
            }
            Action::Save => match (ctx.pack.clone(), ctx.session.as_ref()) {
                (Some(pack), Some(session)) => {
                    Transition::push(SaveLoadScreen::save(session.to_save(&pack)))
                }
                _ => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast("기록할 게임이 없습니다.");
                    Transition::None
                }
            },
            Action::Title => Transition::Flow(Flow::Title),
        }
    }
}

impl Screen for PlaceholderScreen {
    fn name(&self) -> &'static str {
        "placeholder"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            ctx.audio.stop_bgm();
            macroquad::logging::warn!("placeholder screen shown for {}", self.kind);
        }
        let width = PANEL.w - 24.0;
        self.wrapped = self
            .details
            .iter()
            .flat_map(|d| ctx.gfx.wrap(d, FontId::Main, 1, width))
            .collect();
        let w = self.menu.fit_width(&ctx.gfx).max(180.0);
        self.menu
            .set_position(((VIRTUAL_W - w) / 2.0).round(), PANEL.bottom() + 12.0);
        self.menu.set_width(w);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let action = self.actions[i].clone();
                self.perform(ctx, action)
            }
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        draw_backdrop(ctx.time);
        fill_rect(SCREEN, Color::new(0.0, 0.0, 0.03, 0.6));
        draw_title_bar(ctx, "개발 중인 화면");
        draw_window(PANEL);
        gfx.text(
            &self.kind,
            PANEL.x + 12.0,
            PANEL.y + 8.0,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        gfx.text_aligned(
            "이 빌드에서는 아직 구현되지 않은 화면입니다",
            PANEL.x,
            PANEL.y + 8.0,
            PANEL.w - 12.0,
            Align::Right,
            TextStyle::small(theme::TEXT_BAD).shadow(theme::TEXT_SHADOW),
        );
        draw_divider(PANEL.x + 8.0, PANEL.y + 27.0, PANEL.w - 16.0);
        let style = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let max_lines = ((PANEL.h - 36.0) / 16.0) as usize;
        let shown = &self.wrapped[..self.wrapped.len().min(max_lines)];
        gfx.text_lines(shown, PANEL.x + 12.0, PANEL.y + 33.0, style);
        self.menu.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kinds_offer_matching_shortcuts() {
        let drama = Node::Drama {
            id: "d1".into(),
            scene: "prologue".into(),
            next: "c1".into(),
        };
        let p = PlaceholderScreen::for_node(&drama);
        assert_eq!(p.actions, vec![Action::Advance, Action::Save, Action::Title]);
        assert!(p.details.iter().any(|d| d.contains("prologue")));

        let ending = Node::Ending {
            id: "end".into(),
            scene: None,
            title: "도원의 맹세".into(),
        };
        let p = PlaceholderScreen::for_node(&ending);
        assert_eq!(p.actions[0], Action::Ending("도원의 맹세".into()));

        let p = PlaceholderScreen::for_battle_resume("b01");
        assert_eq!(p.actions, vec![Action::AbandonBattle, Action::Title]);
        assert_eq!(p.menu.items.len(), 2);
    }
}
