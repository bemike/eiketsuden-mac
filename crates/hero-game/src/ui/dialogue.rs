//! Drama dialogue box: speech (name tab + 64×80 portrait) or narration (no name, centred
//! lines), typed out at the text speed setting, three lines per page.
//!
//! It looks like [`super::message::MessageBox`] but is driven by the drama screen: besides
//! confirm (complete the page, then turn it) it has a *fast* mode (빨리 넘기기) in which pages
//! appear at once and turn by themselves after a short glance.
//!
//! Line breaks written in the script (indented continuation lines) are kept as long as every
//! line fits the box; when one of them is too long and would leave a short orphan on a line of
//! its own, the paragraph is reflowed as running text instead ([`layout_text`]).

use super::art::draw_portrait_card;
use super::textflow::Typewriter;
use super::theme;
use super::window::{draw_small_arrow, draw_window_ex, WindowStyle};
use crate::app::Ctx;
use crate::gfx::{wrap_text, FontId, Gfx, TextStyle};
use macroquad::prelude::*;

/// Wrap `text` to `width`: the script's line breaks are kept when each written line fits;
/// otherwise the text is reflowed with the breaks read as spaces, if that needs fewer lines.
/// `advance` is the width of one character.
pub fn layout_text(text: &str, width: f32, advance: impl Fn(char) -> f32) -> Vec<String> {
    let kept = wrap_text(text, width, &advance);
    let written = text.split('\n').count();
    if kept.len() <= written {
        return kept;
    }
    let flowed = wrap_text(&text.replace('\n', " "), width, &advance);
    if flowed.len() < kept.len() {
        flowed
    } else {
        kept
    }
}

fn layout(gfx: &Gfx, text: &str, width: f32) -> Vec<String> {
    layout_text(text, width, |c| gfx.char_width(c, FontId::Main, 1))
}

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
    /// Size of the canvas the box was laid out for (bottom edge, full width).
    canvas: Vec2,
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
        let canvas = gfx.size();
        let width = Self::text_rect(canvas, with_portrait).w;
        DialogueBox {
            canvas,
            speaker: Some(speaker.to_string()).filter(|s| !s.is_empty()),
            portrait: portrait.map(str::to_string),
            centered: false,
            writer: Typewriter::new(layout(gfx, text, width), LINES_PER_PAGE),
            idle: 0.0,
            finished: false,
        }
    }

    /// Narration: no name, no portrait, centred lines.
    pub fn narration(gfx: &Gfx, text: &str) -> DialogueBox {
        let canvas = gfx.size();
        let width = Self::text_rect(canvas, false).w;
        DialogueBox {
            canvas,
            speaker: None,
            portrait: None,
            centered: true,
            writer: Typewriter::new(layout(gfx, text, width), LINES_PER_PAGE),
            idle: 0.0,
            finished: false,
        }
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

    fn text_rect(canvas: Vec2, with_portrait: bool) -> Rect {
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

    /// Top edge of everything the box draws (portrait and name tab included), for placing a
    /// choice box above it.
    pub fn top(&self) -> f32 {
        let b = Self::box_rect(self.canvas, self.portrait.is_some());
        let mut top = b.y;
        if self.speaker.is_some() {
            top = top.min(b.y - 16.0);
        }
        if self.portrait.is_some() {
            top = top.min(Self::portrait_rect(self.canvas).y);
        }
        top
    }

    /// Reveal the rest of the current page.
    pub fn complete_page(&mut self) {
        self.writer.complete_page();
    }

    /// Jump to the last page, fully revealed (for a message kept on screen after it was read).
    pub fn show_last_page(&mut self) {
        while self.writer.next_page() {}
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
        let b = Self::box_rect(self.canvas, has_portrait);
        if has_portrait {
            draw_portrait_card(
                ctx,
                self.portrait.as_deref(),
                Self::portrait_rect(self.canvas),
                1.0,
                1.0,
            );
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

        let t = Self::text_rect(self.canvas, has_portrait);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::is_cjk;

    fn mono(c: char) -> f32 {
        if is_cjk(c) {
            2.0
        } else {
            1.0
        }
    }

    #[test]
    fn written_line_breaks_are_kept_when_they_fit() {
        let text = "가나 다라\n마바 사아";
        assert_eq!(
            layout_text(text, 10.0, mono),
            vec!["가나 다라", "마바 사아"]
        );
    }

    #[test]
    fn overlong_written_lines_are_reflowed() {
        // The first written line is one word too long: keeping the break would leave "사아"
        // alone on a line; running text needs fewer lines.
        let text = "가나 다라 마바 사아\n자차 카타";
        assert_eq!(
            layout_text(text, 14.0, mono),
            vec!["가나 다라 마바", "사아 자차 카타"]
        );
        // When reflowing does not help, the written breaks stay.
        let text = "가나다라마바사아\n카타파하가나다";
        assert_eq!(
            layout_text(text, 14.0, mono),
            vec!["가나다라마바사", "아", "카타파하가나다"]
        );
    }
}
