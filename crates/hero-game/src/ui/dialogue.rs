//! Drama dialogue box: speech (name tab + 64×80 portrait) or narration (no name, centred
//! lines), typed out at the text speed setting, three lines per page.
//!
//! It looks like [`super::message::MessageBox`] but is driven by the drama screen: besides
//! confirm (complete the page, then turn it) it has a *fast* mode (빨리 넘기기) in which pages
//! appear at once and turn by themselves after a short glance.

use super::textflow::Typewriter;
use super::theme;
use super::window::{draw_portrait, draw_small_arrow, draw_window_ex, WindowStyle};
use crate::app::Ctx;
use crate::gfx::{FontId, Gfx, TextStyle, VIRTUAL_H, VIRTUAL_W};
use macroquad::prelude::*;

/// Lines shown at once.
pub const LINES_PER_PAGE: usize = 3;
/// Size of the speaker portrait box.
pub const PORTRAIT_SIZE: Vec2 = Vec2::new(64.0, 80.0);
/// Seconds a complete page stays up in fast mode before it turns by itself.
pub const FAST_PAGE_SECONDS: f32 = 0.08;
const MARGIN: f32 = 6.0;
const LINE_H: f32 = 16.0;
const BOX_H: f32 = LINES_PER_PAGE as f32 * LINE_H + 2.0 * theme::PADDING + 2.0;
/// Typing speed-up while a confirm key or the pointer is held.
const HOLD_SPEEDUP: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogueEvent {
    /// The page is being typed.
    Typing,
    /// The page is complete; waiting for confirm.
    Waiting,
    /// The last page was confirmed.
    Finished,
}

/// See the module docs.
#[derive(Debug, Clone)]
pub struct DialogueBox {
    speaker: Option<String>,
    portrait: Option<String>,
    centered: bool,
    writer: Typewriter,
    /// Seconds the current page has been complete.
    idle: f32,
    finished: bool,
}

impl DialogueBox {
    /// A spoken line. `portrait` is a portrait key (`None` for speakers without one, e.g. a
    /// messenger).
    pub fn speech(gfx: &Gfx, speaker: &str, portrait: Option<&str>, text: &str) -> DialogueBox {
        let with_portrait = portrait.is_some();
        let width = Self::text_rect(with_portrait).w;
        DialogueBox {
            speaker: Some(speaker.to_string()).filter(|s| !s.is_empty()),
            portrait: portrait.map(str::to_string),
            centered: false,
            writer: Typewriter::new(gfx.wrap(text, FontId::Main, 1, width), LINES_PER_PAGE),
            idle: 0.0,
            finished: false,
        }
    }

    /// Narration: no name, no portrait, centred lines.
    pub fn narration(gfx: &Gfx, text: &str) -> DialogueBox {
        let width = Self::text_rect(false).w;
        DialogueBox {
            speaker: None,
            portrait: None,
            centered: true,
            writer: Typewriter::new(gfx.wrap(text, FontId::Main, 1, width), LINES_PER_PAGE),
            idle: 0.0,
            finished: false,
        }
    }

    /// Window rectangle of the text box.
    pub fn box_rect(with_portrait: bool) -> Rect {
        let x = if with_portrait {
            MARGIN + PORTRAIT_SIZE.x + 4.0
        } else {
            MARGIN
        };
        Rect::new(x, VIRTUAL_H - MARGIN - BOX_H, VIRTUAL_W - MARGIN - x, BOX_H)
    }

    fn text_rect(with_portrait: bool) -> Rect {
        let b = Self::box_rect(with_portrait);
        Rect::new(
            b.x + theme::PADDING + 4.0,
            b.y + theme::PADDING + 1.0,
            b.w - 2.0 * theme::PADDING - 16.0,
            b.h - 2.0 * theme::PADDING,
        )
    }

    fn portrait_rect() -> Rect {
        Rect::new(
            MARGIN,
            VIRTUAL_H - MARGIN - PORTRAIT_SIZE.y,
            PORTRAIT_SIZE.x,
            PORTRAIT_SIZE.y,
        )
    }

    /// Top edge of everything the box draws (portrait and name tab included), for placing a
    /// choice box above it.
    pub fn top(&self) -> f32 {
        let b = Self::box_rect(self.portrait.is_some());
        let mut top = b.y;
        if self.speaker.is_some() {
            top = top.min(b.y - 16.0);
        }
        if self.portrait.is_some() {
            top = top.min(Self::portrait_rect().y);
        }
        top
    }

    pub fn speaker(&self) -> Option<&str> {
        self.speaker.as_deref()
    }

    pub fn portrait(&self) -> Option<&str> {
        self.portrait.as_deref()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn is_typing(&self) -> bool {
        self.writer.is_typing()
    }

    /// Reveal the rest of the current page.
    pub fn complete_page(&mut self) {
        self.writer.complete_page();
    }

    /// Advance typing and handle confirm. In `fast` mode pages appear at once and turn after
    /// [`FAST_PAGE_SECONDS`].
    pub fn update(&mut self, ctx: &mut Ctx, fast: bool) -> DialogueEvent {
        if self.finished {
            return DialogueEvent::Finished;
        }
        if self.writer.is_typing() {
            self.idle = 0.0;
            match ctx.settings.text_speed.chars_per_second() {
                _ if fast => self.writer.complete_page(),
                None => self.writer.complete_page(),
                Some(cps) => {
                    let held = ctx.input.key_down(KeyCode::Z)
                        || ctx.input.key_down(KeyCode::Enter)
                        || ctx.input.key_down(KeyCode::Space)
                        || ctx.input.down();
                    let speed = if held { cps * HOLD_SPEEDUP } else { cps };
                    self.writer.advance(speed * ctx.dt);
                }
            }
            if ctx.input.confirm() {
                // Finish the page first; the next confirm turns it.
                ctx.input.consume();
                self.writer.complete_page();
            }
            return if self.writer.is_typing() {
                DialogueEvent::Typing
            } else {
                DialogueEvent::Waiting
            };
        }
        self.idle += ctx.dt;
        let confirmed = ctx.input.confirm();
        if confirmed {
            ctx.input.consume();
        }
        if confirmed || (fast && self.idle >= FAST_PAGE_SECONDS) {
            self.idle = 0.0;
            if self.writer.next_page() {
                return DialogueEvent::Typing;
            }
            self.finished = true;
            return DialogueEvent::Finished;
        }
        DialogueEvent::Waiting
    }

    /// Draw the box. `more_arrow` shows the blinking "continue" arrow on a complete page.
    pub fn draw(&self, ctx: &Ctx, more_arrow: bool) {
        let gfx = &ctx.gfx;
        let has_portrait = self.portrait.is_some();
        let b = Self::box_rect(has_portrait);
        if has_portrait {
            draw_portrait(ctx, self.portrait.as_deref(), Self::portrait_rect());
        }
        let style = if self.centered {
            WindowStyle::Panel
        } else {
            WindowStyle::Normal
        };
        draw_window_ex(b, style, 0.97);
        if let Some(name) = &self.speaker {
            let w = gfx.text_width(name, FontId::Main, 1) + 14.0;
            let tab = Rect::new(b.x + 6.0, b.y - 16.0, w, 19.0);
            draw_window_ex(tab, WindowStyle::Panel, 1.0);
            gfx.text(
                name,
                tab.x + 7.0,
                tab.y + 2.0,
                TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
            );
        }

        let t = Self::text_rect(has_portrait);
        let style = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let full = self.writer.page_lines();
        // Narration pages are centred vertically too when they are shorter than the box.
        let y0 = if self.centered {
            t.y + ((LINES_PER_PAGE - full.len().min(LINES_PER_PAGE)) as f32 * LINE_H / 2.0).floor()
        } else {
            t.y
        };
        for (i, visible) in self.writer.visible_lines().into_iter().enumerate() {
            let y = y0 + i as f32 * LINE_H;
            if self.centered {
                // Centre on the complete line so the text does not shift while it is typed.
                let full_w = gfx.text_width(&full[i], FontId::Main, 1);
                let x = t.x + ((t.w - full_w) / 2.0).round();
                gfx.text(visible, x, y, style);
            } else {
                gfx.text(visible, t.x, y, style);
            }
        }
        if more_arrow
            && !self.writer.is_typing()
            && !self.finished
            && (ctx.time * 2.5).fract() < 0.6
        {
            draw_small_arrow(b.right() - 11.0, b.bottom() - 9.0, true, theme::TEXT_ACCENT);
        }
    }
}
