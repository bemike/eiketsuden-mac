//! Game over: the campaign was lost (a battle was lost without an `on_defeat` node). Plays the
//! defeat jingle and offers 불러오기 (load a save) or 타이틀로 (title screen).

use super::backdrop::draw_backdrop;
use super::saveload::SaveLoadScreen;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::bgm;
use crate::flow::Flow;
use crate::gfx::{fill_gradient_v, fill_rect, Align, TextStyle};
use crate::saves;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use macroquad::prelude::*;

const MENU_W: f32 = 120.0;
/// The heading, the message and the menu are grouped around the canvas centre: their tops are
/// this far from the vertical centre (the heading and message above it, the menu below).
const HEADING_ABOVE: f32 = 65.0;
const MESSAGE_ABOVE: f32 = 5.0;
const MENU_BELOW: f32 = 41.0;

pub struct GameOverScreen {
    menu: Menu,
    age: f32,
}

impl GameOverScreen {
    pub fn new() -> GameOverScreen {
        GameOverScreen {
            menu: Menu::new(Vec::new()),
            age: 0.0,
        }
    }
}

impl Default for GameOverScreen {
    fn default() -> Self {
        GameOverScreen::new()
    }
}

impl Screen for GameOverScreen {
    fn name(&self) -> &'static str {
        "gameover"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            ctx.audio.play_jingle(bgm::DEFEAT);
        }
        // Re-checked when the load screen is closed again (a save may have been deleted).
        let can_load = ctx
            .pack_id()
            .is_some_and(|id| saves::any(ctx.storage.as_ref(), id));
        let cursor = if can_load { 0 } else { 1 };
        let mut menu = Menu::new(vec![
            MenuItem::new("불러오기").enabled(can_load),
            MenuItem::new("타이틀로"),
        ])
        .cancellable(false)
        .at(
            ((ctx.gfx.size().x - MENU_W) / 2.0).round(),
            (ctx.gfx.size().y / 2.0).round() + MENU_BELOW,
            MENU_W,
        );
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.age += ctx.dt;
        match self.menu.update(ctx) {
            MenuEvent::Selected(0) => match ctx.pack_id() {
                Some(id) => Transition::push(SaveLoadScreen::load(id)),
                None => Transition::None,
            },
            MenuEvent::Selected(_) => Transition::Flow(Flow::Title),
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let (w, h) = (gfx.size().x, gfx.size().y);
        let mid = (h / 2.0).round();
        draw_backdrop(gfx.size(), ctx.time);
        // Blood-red dusk over the landscape.
        fill_gradient_v(
            gfx.screen(),
            Color::new(0.25, 0.0, 0.02, 0.55),
            Color::new(0.02, 0.0, 0.0, 0.85),
        );
        let alpha = (self.age / 1.2).min(1.0);
        gfx.text_aligned(
            "패 배",
            0.0,
            mid - HEADING_ABOVE,
            w,
            Align::Center,
            TextStyle::main(theme::TEXT_BAD.with_alpha(alpha))
                .size(3)
                .shadow(Color::new(0.0, 0.0, 0.0, 0.8 * alpha)),
        );
        gfx.text_aligned(
            "군이 무너졌습니다. 기록을 불러와 다시 도전하십시오.",
            0.0,
            mid - MESSAGE_ABOVE,
            w,
            Align::Center,
            TextStyle::main(theme::TEXT.with_alpha(alpha)).shadow(theme::TEXT_SHADOW),
        );
        fill_rect(
            Rect::new(0.0, h - 1.0, w, 1.0),
            theme::TEXT_BAD.with_alpha(0.4),
        );
        self.menu.draw(ctx);
    }
}
