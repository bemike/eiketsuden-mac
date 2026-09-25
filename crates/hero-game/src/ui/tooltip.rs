//! Tooltips: a small window of small text next to the pointer, shown after a short hover.

use super::theme;
use super::window::{draw_window_ex, WindowStyle};
use crate::gfx::{FontId, Gfx, TextStyle, VIRTUAL_H, VIRTUAL_W};
use macroquad::prelude::*;

/// Hover time before a tooltip appears.
pub const TOOLTIP_DELAY: f32 = 0.45;

/// Tracks how long the pointer has rested on the same target.
#[derive(Debug, Clone, Default)]
pub struct HoverTimer {
    target: Option<u64>,
    time: f32,
}

impl HoverTimer {
    /// Feed the id of the hovered target (or `None`); returns whether its tooltip is visible.
    pub fn update(&mut self, target: Option<u64>, dt: f32) -> bool {
        if target != self.target {
            self.target = target;
            self.time = 0.0;
        } else if target.is_some() {
            self.time += dt;
        }
        self.visible()
    }

    pub fn visible(&self) -> bool {
        self.target.is_some() && self.time >= TOOLTIP_DELAY
    }

    pub fn target(&self) -> Option<u64> {
        self.target
    }
}

/// Draw a tooltip near `anchor` (usually the pointer), kept inside the canvas.
pub fn draw_tooltip(gfx: &Gfx, text: &str, anchor: Vec2) {
    let lines = gfx.wrap(text, FontId::Small, 1, 180.0);
    let lh = gfx.line_height(FontId::Small, 1);
    let w = lines
        .iter()
        .map(|l| gfx.text_width(l, FontId::Small, 1))
        .fold(0.0, f32::max)
        + 12.0;
    let h = lines.len() as f32 * lh + 9.0;
    let r = tooltip_rect(anchor, vec2(w.round(), h));
    draw_window_ex(r, WindowStyle::Panel, 0.95);
    gfx.text_lines(&lines, r.x + 6.0, r.y + 4.0, TextStyle::small(theme::TEXT));
}

/// Position a `size` box below-right of `anchor`, flipped to stay on screen.
pub fn tooltip_rect(anchor: Vec2, size: Vec2) -> Rect {
    let mut x = anchor.x + 8.0;
    let mut y = anchor.y + 12.0;
    if x + size.x > VIRTUAL_W - 2.0 {
        x = anchor.x - size.x - 4.0;
    }
    if y + size.y > VIRTUAL_H - 2.0 {
        y = anchor.y - size.y - 4.0;
    }
    Rect::new(
        x.clamp(2.0, (VIRTUAL_W - size.x - 2.0).max(2.0)),
        y.clamp(2.0, (VIRTUAL_H - size.y - 2.0).max(2.0)),
        size.x,
        size.y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_delay() {
        let mut h = HoverTimer::default();
        assert!(!h.update(Some(1), 0.1));
        assert!(!h.update(Some(1), 0.2));
        assert!(h.update(Some(1), 0.2));
        assert!(!h.update(Some(2), 0.5));
        assert!(!h.update(None, 1.0));
    }

    #[test]
    fn tooltip_stays_on_screen() {
        let r = tooltip_rect(vec2(470.0, 260.0), vec2(100.0, 30.0));
        assert!(r.right() <= VIRTUAL_W && r.bottom() <= VIRTUAL_H && r.x >= 0.0 && r.y >= 0.0);
        let r = tooltip_rect(vec2(10.0, 10.0), vec2(100.0, 30.0));
        assert_eq!((r.x, r.y), (18.0, 22.0));
    }
}
