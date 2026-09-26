//! Gauges: HP (troops), MP, EXP, morale.

use super::format;
use super::theme;
use crate::gfx::{fill_gradient_v, fill_rect, stroke_rect, Align, FontId, Gfx, TextStyle};
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GaugeKind {
    /// Green, turning yellow below 50 % and red below 25 %.
    Hp,
    Mp,
    Exp,
    Morale,
    Custom(Color),
}

impl GaugeKind {
    /// Fill colour for a fill ratio 0..=1.
    pub fn color(self, ratio: f32) -> Color {
        match self {
            GaugeKind::Hp => {
                if ratio > 0.5 {
                    theme::HP_HIGH
                } else if ratio > 0.25 {
                    theme::HP_MID
                } else {
                    theme::HP_LOW
                }
            }
            GaugeKind::Mp => theme::MP,
            GaugeKind::Exp => theme::EXP,
            GaugeKind::Morale => theme::MORALE,
            GaugeKind::Custom(c) => c,
        }
    }
}

/// Fill ratio clamped to 0..=1 (0 when `max` is not positive).
pub fn ratio(value: f32, max: f32) -> f32 {
    if max > 0.0 && value.is_finite() {
        (value / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A bar: dark frame, background, filled part with a highlight line.
pub fn draw_gauge(r: Rect, value: f32, max: f32, kind: GaugeKind) {
    fill_rect(r, theme::GAUGE_BG);
    stroke_rect(r, theme::GAUGE_FRAME);
    let inner = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
    let k = ratio(value, max);
    let fill_w = (inner.w * k).round();
    if fill_w > 0.0 && inner.h > 0.0 {
        let c = kind.color(k);
        let dark = Color::new(c.r * 0.6, c.g * 0.6, c.b * 0.6, c.a);
        let fill = Rect::new(inner.x, inner.y, fill_w, inner.h);
        fill_gradient_v(fill, c, dark);
        fill_rect(
            Rect::new(inner.x, inner.y, fill_w, 1.0),
            Color::new(1.0, 1.0, 1.0, 0.35),
        );
    }
}

/// A labelled gauge with its top-left corner at `pos`: `label` on the left and `value/max` on
/// the right above a thin bar. Occupies `w` × 17 virtual pixels.
pub fn draw_gauge_labeled(
    gfx: &Gfx,
    pos: Vec2,
    w: f32,
    label: &str,
    value: i64,
    max: i64,
    kind: GaugeKind,
) {
    let (x, y) = (pos.x, pos.y);
    let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
    gfx.text(label, x, y, small);
    gfx.text_aligned(
        &format::ratio(value, max),
        x,
        y,
        w,
        Align::Right,
        TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW),
    );
    let bar_y = y + gfx.line_height(FontId::Small, 1);
    draw_gauge(Rect::new(x, bar_y, w, 5.0), value as f32, max as f32, kind);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_and_hp_colours() {
        assert_eq!(ratio(5.0, 10.0), 0.5);
        assert_eq!(ratio(15.0, 10.0), 1.0);
        assert_eq!(ratio(-1.0, 10.0), 0.0);
        assert_eq!(ratio(1.0, 0.0), 0.0);
        assert_eq!(ratio(f32::NAN, 10.0), 0.0);
        assert_eq!(GaugeKind::Hp.color(0.9), theme::HP_HIGH);
        assert_eq!(GaugeKind::Hp.color(0.4), theme::HP_MID);
        assert_eq!(GaugeKind::Hp.color(0.1), theme::HP_LOW);
        assert_eq!(GaugeKind::Mp.color(0.1), theme::MP);
    }
}
