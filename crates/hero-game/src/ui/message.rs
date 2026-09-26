//! Dialogue message box: typewriter text, pages of three lines, optional speaker name tab and
//! 64×80 portrait box — the building block of drama scenes and battle talk.
//!
//! Confirm (key or tap) first completes the current page, then turns the page, and after the
//! last page reports [`MessageEvent::Finished`]. The typing speed follows the text speed setting;
//! holding a confirm key or the pointer speeds typing up four times.

use super::theme;
use super::window::{draw_portrait, draw_small_arrow, draw_window, draw_window_ex, WindowStyle};
use crate::app::Ctx;
use crate::gfx::{FontId, Gfx, TextStyle};
use macroquad::prelude::*;

/// Lines shown at once.
pub const LINES_PER_PAGE: usize = 3;
/// Size of the portrait box in virtual pixels.
pub const PORTRAIT_SIZE: Vec2 = Vec2::new(64.0, 80.0);
const MARGIN: f32 = 6.0;
const BOX_H: f32 = LINES_PER_PAGE as f32 * 16.0 + 2.0 * theme::PADDING + 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageEvent {
    /// Text is still being typed.
    Typing,
    /// The page is complete; waiting for confirm.
    Waiting,
    /// The player confirmed the last page.
    Finished,
}

#[derive(Debug, Clone)]
pub struct MessageBox {
    /// Size of the canvas the box was laid out for (bottom edge, full width).
    canvas: Vec2,
    speaker: Option<String>,
    portrait: Option<String>,
    pages: Vec<Vec<String>>,
    page: usize,
    shown: f32,
    finished: bool,
}

impl MessageBox {
    /// A message. `speaker` is the display name for the name tab; `portrait` a portrait key
    /// (`portraits/<key>`, falling back to `_unknown`, then a silhouette).
    pub fn new(gfx: &Gfx, speaker: Option<&str>, portrait: Option<&str>, text: &str) -> MessageBox {
        let canvas = gfx.size();
        let width = Self::text_rect_for(canvas, portrait.is_some()).w;
        let lines = gfx.wrap(text, FontId::Main, 1, width);
        let pages = lines
            .chunks(LINES_PER_PAGE)
            .map(|c| c.to_vec())
            .collect::<Vec<_>>();
        MessageBox {
            canvas,
            speaker: speaker.filter(|s| !s.is_empty()).map(str::to_string),
            portrait: portrait.map(str::to_string),
            pages: if pages.is_empty() {
                vec![vec![String::new()]]
            } else {
                pages
            },
            page: 0,
            shown: 0.0,
            finished: false,
        }
    }

    /// Narration: no speaker, no portrait.
    pub fn narration(gfx: &Gfx, text: &str) -> MessageBox {
        MessageBox::new(gfx, None, None, text)
    }

    /// Window rectangle of the text box on a `canvas` sized canvas: along the bottom edge,
    /// right of the portrait when there is one.
    pub fn box_rect(canvas: Vec2, with_portrait: bool) -> Rect {
        let x = if with_portrait {
            MARGIN + PORTRAIT_SIZE.x + 4.0
        } else {
            MARGIN
        };
        Rect::new(x, canvas.y - MARGIN - BOX_H, canvas.x - MARGIN - x, BOX_H)
    }

    fn text_rect_for(canvas: Vec2, with_portrait: bool) -> Rect {
        let b = Self::box_rect(canvas, with_portrait);
        Rect::new(
            b.x + theme::PADDING + 4.0,
            b.y + theme::PADDING + 1.0,
            b.w - 2.0 * theme::PADDING - 16.0,
            b.h - 2.0 * theme::PADDING,
        )
    }

    fn portrait_rect(canvas: Vec2) -> Rect {
        Rect::new(
            MARGIN,
            canvas.y - MARGIN - PORTRAIT_SIZE.y,
            PORTRAIT_SIZE.x,
            PORTRAIT_SIZE.y,
        )
    }

    fn page_chars(&self) -> usize {
        self.pages[self.page]
            .iter()
            .map(|l| l.chars().count())
            .sum()
    }

    /// The current page is still being typed.
    pub fn is_typing(&self) -> bool {
        (self.shown as usize) < self.page_chars()
    }

    /// Complete the current page immediately.
    pub fn skip(&mut self) {
        self.shown = self.page_chars() as f32;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn update(&mut self, ctx: &mut Ctx) -> MessageEvent {
        if self.finished {
            return MessageEvent::Finished;
        }
        let total = self.page_chars() as f32;
        if self.shown < total {
            match ctx.settings.text_speed.chars_per_second() {
                None => self.shown = total,
                Some(cps) => {
                    let fast = ctx.input.key_down(KeyCode::Z)
                        || ctx.input.key_down(KeyCode::Enter)
                        || ctx.input.key_down(KeyCode::Space)
                        || ctx.input.key_down(KeyCode::LeftControl)
                        || ctx.input.down();
                    let speed = if fast { cps * 4.0 } else { cps };
                    self.shown = (self.shown + speed * ctx.dt).min(total);
                }
            }
            if ctx.input.confirm() {
                // Finish the page first; the next confirm turns it.
                ctx.input.consume();
                self.shown = total;
            }
            return if self.shown < total {
                MessageEvent::Typing
            } else {
                MessageEvent::Waiting
            };
        }
        if ctx.input.confirm() {
            ctx.input.consume();
            if self.page + 1 < self.pages.len() {
                self.page += 1;
                self.shown = 0.0;
                return MessageEvent::Typing;
            }
            self.finished = true;
            return MessageEvent::Finished;
        }
        MessageEvent::Waiting
    }

    pub fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let has_portrait = self.portrait.is_some();
        let b = Self::box_rect(self.canvas, has_portrait);
        if has_portrait {
            draw_portrait(
                ctx,
                self.portrait.as_deref(),
                Self::portrait_rect(self.canvas),
            );
        }
        draw_window(b);
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

        let t = Self::text_rect_for(self.canvas, has_portrait);
        let style = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let mut remaining = self.shown as usize;
        for (i, line) in self.pages[self.page].iter().enumerate() {
            if remaining == 0 {
                break;
            }
            let n = line.chars().count();
            let visible = if remaining >= n {
                line.as_str()
            } else {
                let end = line
                    .char_indices()
                    .nth(remaining)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line.len());
                &line[..end]
            };
            remaining = remaining.saturating_sub(n);
            gfx.text(visible, t.x, t.y + i as f32 * 16.0, style);
        }

        if !self.is_typing() && !self.finished && (ctx.time * 2.5).fract() < 0.6 {
            draw_small_arrow(b.right() - 11.0, b.bottom() - 9.0, true, theme::TEXT_ACCENT);
        }
    }
}
