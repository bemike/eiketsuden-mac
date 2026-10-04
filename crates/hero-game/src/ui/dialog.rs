//! Modal dialogs: a choice box (prompt + list of options) and a yes/no confirmation.
//!
//! Both are plain widgets owned by a screen: while one is open the screen routes its input to
//! the dialog (`update`) and draws it last. They never block the frame.

use super::menu::{Menu, MenuEvent, MenuItem};
use super::theme;
use super::window::{draw_highlight, draw_window, draw_window_ex, WindowStyle};
use crate::app::Ctx;
use crate::audio::sfx;
use crate::gfx::{Align, FontId, Gfx, TextStyle};
use crate::input::Dir;
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceEvent {
    None,
    Chosen(usize),
    Cancelled,
}

/// A prompt with a vertical list of options (drama `@choice`, "which slot?", ...).
#[derive(Debug, Clone)]
pub struct ChoiceBox {
    prompt: Vec<String>,
    menu: Menu,
    rect: Rect,
    /// Option chosen by cancel (`None`: cancel reports [`ChoiceEvent::Cancelled`] if allowed).
    cancel_index: Option<usize>,
}

impl ChoiceBox {
    /// Centred on the canvas. With `cancel = Some(i)` the cancel button picks option `i`;
    /// with `None` the choice cannot be cancelled.
    pub fn new(
        gfx: &Gfx,
        prompt: Option<&str>,
        options: &[&str],
        cancel: Option<usize>,
    ) -> ChoiceBox {
        let items: Vec<MenuItem> = options.iter().map(|o| MenuItem::new(*o)).collect();
        let mut menu = Menu::new(items).rows(options.len().min(8));
        menu.cancellable = cancel.is_some();
        menu.framed = false;
        let menu_w = menu.fit_width(gfx);
        let prompt_lines = prompt
            .map(|p| gfx.wrap(p, FontId::Main, 1, 300.0))
            .unwrap_or_default();
        let prompt_w = prompt_lines
            .iter()
            .map(|l| gfx.text_width(l, FontId::Main, 1))
            .fold(0.0, f32::max);
        let canvas = gfx.size();
        let w = (menu_w.max(prompt_w + 2.0 * theme::PADDING + 8.0)).clamp(80.0, canvas.x - 16.0);
        let prompt_h = if prompt_lines.is_empty() {
            0.0
        } else {
            prompt_lines.len() as f32 * 16.0 + 6.0
        };
        let menu_h = menu.rect().h;
        let h = prompt_h + menu_h + 2.0 * theme::PADDING;
        let rect = Rect::new(
            ((canvas.x - w) / 2.0).round(),
            ((canvas.y - h) / 2.0).round(),
            w.round(),
            h,
        );
        menu.set_position(rect.x + theme::PADDING, rect.y + theme::PADDING + prompt_h);
        menu.set_width(rect.w - 2.0 * theme::PADDING);
        ChoiceBox {
            prompt: prompt_lines,
            menu,
            rect,
            cancel_index: cancel,
        }
    }

    /// Move the box so its bottom edge is at `bottom` (e.g. above a message box).
    pub fn with_bottom(mut self, bottom: f32) -> ChoiceBox {
        let dy = bottom - self.rect.bottom();
        self.rect.y += dy;
        let p = self.menu.rect();
        self.menu.set_position(p.x, p.y + dy);
        self
    }

    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn update(&mut self, ctx: &mut Ctx) -> ChoiceEvent {
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => ChoiceEvent::Chosen(i),
            MenuEvent::Cancelled => match self.cancel_index {
                Some(i) if i < self.menu.items.len() => ChoiceEvent::Chosen(i),
                _ => ChoiceEvent::Cancelled,
            },
            _ => ChoiceEvent::None,
        }
    }

    pub fn draw(&self, ctx: &Ctx) {
        draw_window(self.rect);
        let style = TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW);
        ctx.gfx.text_lines(
            &self.prompt,
            self.rect.x + theme::PADDING + 4.0,
            self.rect.y + theme::PADDING,
            style,
        );
        self.menu.draw(ctx);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmEvent {
    None,
    Yes,
    No,
}

/// "Are you sure?" dialog with two buttons (예 / 아니오 by default). Cancel answers no.
#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    lines: Vec<String>,
    yes: String,
    no: String,
    yes_selected: bool,
    rect: Rect,
}

const BUTTON_W: f32 = 64.0;
const BUTTON_H: f32 = 18.0;

impl ConfirmDialog {
    pub fn new(gfx: &Gfx, text: &str) -> ConfirmDialog {
        let lines = gfx.wrap(text, FontId::Main, 1, 280.0);
        let text_w = lines
            .iter()
            .map(|l| gfx.text_width(l, FontId::Main, 1))
            .fold(0.0, f32::max);
        let w = (text_w + 2.0 * theme::PADDING + 16.0)
            .max(2.0 * BUTTON_W + 40.0)
            .round();
        let h = lines.len() as f32 * 16.0 + BUTTON_H + 2.0 * theme::PADDING + 14.0;
        let canvas = gfx.size();
        ConfirmDialog {
            lines,
            yes: "是".into(),
            no: "否".into(),
            yes_selected: true,
            rect: Rect::new(
                ((canvas.x - w) / 2.0).round(),
                ((canvas.y - h) / 2.0).round(),
                w,
                h,
            ),
        }
    }

    /// Custom button labels.
    pub fn labels(mut self, yes: &str, no: &str) -> ConfirmDialog {
        self.yes = yes.into();
        self.no = no.into();
        self
    }

    /// Start with "no" selected (for destructive actions).
    pub fn default_no(mut self) -> ConfirmDialog {
        self.yes_selected = false;
        self
    }

    fn buttons(&self) -> (Rect, Rect) {
        let y = self.rect.bottom() - theme::PADDING - BUTTON_H - 2.0;
        let cx = self.rect.x + self.rect.w / 2.0;
        (
            Rect::new(cx - BUTTON_W - 6.0, y, BUTTON_W, BUTTON_H),
            Rect::new(cx + 6.0, y, BUTTON_W, BUTTON_H),
        )
    }

    pub fn update(&mut self, ctx: &mut Ctx) -> ConfirmEvent {
        let (yes_r, no_r) = self.buttons();
        let input = &ctx.input;
        if let Some(p) = input.tap() {
            if yes_r.contains(p) {
                ctx.sfx(sfx::CONFIRM);
                return ConfirmEvent::Yes;
            }
            if no_r.contains(p) {
                ctx.sfx(sfx::CANCEL);
                return ConfirmEvent::No;
            }
        }
        if input.pointer_moved() {
            if let Some(p) = input.pointer() {
                if yes_r.contains(p) && !self.yes_selected {
                    self.yes_selected = true;
                } else if no_r.contains(p) && self.yes_selected {
                    self.yes_selected = false;
                }
            }
        }
        match input.nav() {
            Some(Dir::Left) | Some(Dir::Right) | Some(Dir::Up) | Some(Dir::Down) => {
                self.yes_selected = !self.yes_selected;
                ctx.sfx(sfx::CURSOR);
                return ConfirmEvent::None;
            }
            None => {}
        }
        if input.confirm_key() {
            if self.yes_selected {
                ctx.sfx(sfx::CONFIRM);
                return ConfirmEvent::Yes;
            }
            ctx.sfx(sfx::CANCEL);
            return ConfirmEvent::No;
        }
        if input.cancel() {
            ctx.sfx(sfx::CANCEL);
            return ConfirmEvent::No;
        }
        ConfirmEvent::None
    }

    pub fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        draw_window(self.rect);
        let text_style = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        for (i, line) in self.lines.iter().enumerate() {
            gfx.text_aligned(
                line,
                self.rect.x,
                self.rect.y + theme::PADDING + 2.0 + i as f32 * 16.0,
                self.rect.w,
                Align::Center,
                text_style,
            );
        }
        let (yes_r, no_r) = self.buttons();
        for (r, label, selected) in [
            (yes_r, &self.yes, self.yes_selected),
            (no_r, &self.no, !self.yes_selected),
        ] {
            draw_window_ex(r, WindowStyle::Panel, 1.0);
            if selected {
                draw_highlight(super::window::inset(r, 3.0), true, ctx.time);
            }
            let color = if selected {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            gfx.text_aligned(
                label,
                r.x,
                r.y + 1.0,
                r.w,
                Align::Center,
                TextStyle::main(color).shadow(theme::TEXT_SHADOW),
            );
        }
    }
}
