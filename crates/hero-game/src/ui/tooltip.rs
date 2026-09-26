//! Tooltips: a small window of small text next to the pointer, shown after a short hover.

use super::theme;
use super::window::{draw_window_ex, WindowStyle};
use crate::gfx::{FontId, Gfx, TextStyle};
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
    /// Feed the id of the hovered target (or `None`) once per frame; returns whether its tooltip
    /// is visible. Hover time counts from the first frame the target is seen (that frame's `dt`
    /// is not counted, the pointer arrived somewhere during it).
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
    let r = tooltip_rect(gfx.size(), anchor, vec2(w.round(), h));
    draw_window_ex(r, WindowStyle::Panel, 0.95);
    gfx.text_lines(&lines, r.x + 6.0, r.y + 4.0, TextStyle::small(theme::TEXT));
}

/// Position a `size` box below-right of `anchor`, flipped to stay on a `canvas` sized canvas.
pub fn tooltip_rect(canvas: Vec2, anchor: Vec2, size: Vec2) -> Rect {
    let mut x = anchor.x + 8.0;
    let mut y = anchor.y + 12.0;
    if x + size.x > canvas.x - 2.0 {
        x = anchor.x - size.x - 4.0;
    }
    if y + size.y > canvas.y - 2.0 {
        y = anchor.y - size.y - 4.0;
    }
    Rect::new(
        x.clamp(2.0, (canvas.x - size.x - 2.0).max(2.0)),
        y.clamp(2.0, (canvas.y - size.y - 2.0).max(2.0)),
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
        // First sighting: the clock starts, the frame time is not counted.
        assert!(!h.update(Some(1), 0.1));
        assert!(!h.update(Some(1), 0.2));
        assert!(!h.update(Some(1), 0.2)); // 0.4 s < TOOLTIP_DELAY
        assert!(h.update(Some(1), 0.1)); // 0.5 s
        assert_eq!(h.target(), Some(1));
        // Another target restarts the delay; leaving hides it.
        assert!(!h.update(Some(2), 0.5));
        assert!(!h.update(None, 1.0));
        assert!(!h.visible());
    }

    #[test]
    fn tooltip_stays_on_screen() {
        for canvas in [crate::gfx::DEFAULT_CANVAS, vec2(640.0, 480.0)] {
            let corner = canvas - vec2(10.0, 10.0);
            let r = tooltip_rect(canvas, corner, vec2(100.0, 30.0));
            assert!(r.right() <= canvas.x && r.bottom() <= canvas.y && r.x >= 0.0 && r.y >= 0.0);
            let r = tooltip_rect(canvas, vec2(10.0, 10.0), vec2(100.0, 30.0));
            assert_eq!((r.x, r.y), (18.0, 22.0));
        }
    }
}
