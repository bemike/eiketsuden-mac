//! Error screen: a heading, the details (wrapped, scrollable) and a menu — retry loading,
//! back to the title screen, or quit.
//!
//! [`ErrorScreen::fatal`] is used when loading fails (retry restarts the loading screen);
//! [`ErrorScreen::recoverable`] for problems during play (back to the title screen).
//! Details are shown in both Korean and the raw technical text so bug reports are useful even
//! when the Korean font itself is what failed to load.

use super::loading::{LoadingScreen, Target};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{FontId, TextStyle, VIRTUAL_W};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_small_arrow, draw_window};
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Retry(Target),
    Title,
    Quit,
}

const TEXT_RECT: Rect = Rect {
    x: 24.0,
    y: 52.0,
    w: 432.0,
    h: 148.0,
};

pub struct ErrorScreen {
    title: String,
    details: Vec<String>,
    wrapped: Vec<String>,
    scroll: usize,
    actions: Vec<Action>,
    menu: Menu,
}

impl ErrorScreen {
    /// Loading failed: offer retry (when `retry` is given) and quit.
    pub fn fatal(title: &str, details: Vec<String>, retry: Option<Target>) -> ErrorScreen {
        let mut actions = Vec::new();
        if let Some(t) = retry {
            actions.push(Action::Retry(t));
        }
        if crate::platform::can_quit() {
            actions.push(Action::Quit);
        }
        ErrorScreen::build(title, details, actions)
    }

    /// Something went wrong during play: back to the title screen (or quit).
    pub fn recoverable(title: &str, details: Vec<String>) -> ErrorScreen {
        let mut actions = vec![Action::Title];
        if crate::platform::can_quit() {
            actions.push(Action::Quit);
        }
        ErrorScreen::build(title, details, actions)
    }

    fn build(title: &str, details: Vec<String>, actions: Vec<Action>) -> ErrorScreen {
        let items = actions
            .iter()
            .map(|a| {
                MenuItem::new(match a {
                    Action::Retry(_) => "다시 시도 (Retry)",
                    Action::Title => "타이틀로 (Title)",
                    Action::Quit => "종료 (Quit)",
                })
            })
            .collect();
        let mut menu = Menu::new(items).cancellable(false);
        menu.wrap = true;
        ErrorScreen {
            title: title.to_string(),
            details,
            wrapped: Vec::new(),
            scroll: 0,
            actions,
            menu,
        }
    }

    fn visible_lines(&self) -> usize {
        (TEXT_RECT.h / 12.0) as usize
    }
}

impl Screen for ErrorScreen {
    fn name(&self) -> &'static str {
        "error"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            ctx.audio.stop_bgm();
            ctx.sfx(sfx::ERROR);
        }
        self.wrapped = self
            .details
            .iter()
            .flat_map(|d| ctx.gfx.wrap(d, FontId::Small, 1, TEXT_RECT.w - 12.0))
            .collect();
        let w = self.menu.fit_width(&ctx.gfx).max(140.0);
        let h = self.menu.rect().h;
        self.menu
            .set_position(((VIRTUAL_W - w) / 2.0).round(), 262.0 - h);
        self.menu.set_width(w);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let max_scroll = self.wrapped.len().saturating_sub(self.visible_lines());
        if ctx.input.hovering(TEXT_RECT) {
            let w = ctx.input.wheel();
            if w != 0 {
                self.scroll = (self.scroll as i32 + w * 2).clamp(0, max_scroll as i32) as usize;
            }
        }
        if let Some(drag) = ctx.input.drag() {
            if TEXT_RECT.contains(drag.origin) {
                let lines = (-drag.delta.y / 4.0) as i32;
                self.scroll = (self.scroll as i32 + lines).clamp(0, max_scroll as i32) as usize;
            }
        }
        if ctx.input.key_pressed(KeyCode::PageDown) {
            self.scroll = (self.scroll + self.visible_lines()).min(max_scroll);
        }
        if ctx.input.key_pressed(KeyCode::PageUp) {
            self.scroll = self.scroll.saturating_sub(self.visible_lines());
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => match self.actions[i] {
                Action::Retry(target) => Transition::replace(LoadingScreen::new(target)),
                Action::Title => Transition::Flow(Flow::Title),
                Action::Quit => Transition::Quit,
            },
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        clear_background(theme::BACKGROUND);
        let gfx = &ctx.gfx;
        gfx.text(
            &self.title,
            TEXT_RECT.x,
            18.0,
            TextStyle::main(theme::TEXT_BAD)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        let frame = Rect::new(
            TEXT_RECT.x - 6.0,
            TEXT_RECT.y - 6.0,
            TEXT_RECT.w + 12.0,
            TEXT_RECT.h + 12.0,
        );
        draw_window(frame);
        let style = TextStyle::small(theme::TEXT);
        for (i, line) in self
            .wrapped
            .iter()
            .skip(self.scroll)
            .take(self.visible_lines())
            .enumerate()
        {
            gfx.text(
                line,
                TEXT_RECT.x + 4.0,
                TEXT_RECT.y + i as f32 * 12.0,
                style,
            );
        }
        if self.scroll > 0 {
            draw_small_arrow(
                frame.right() - 10.0,
                frame.y + 8.0,
                false,
                theme::TEXT_ACCENT,
            );
        }
        if self.scroll + self.visible_lines() < self.wrapped.len() {
            draw_small_arrow(
                frame.right() - 10.0,
                frame.bottom() - 8.0,
                true,
                theme::TEXT_ACCENT,
            );
        }
        self.menu.draw(ctx);
    }
}
