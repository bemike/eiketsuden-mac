//! Window frames, highlights, cursors and media helpers with graceful fallbacks.

use super::theme;
use crate::app::Ctx;
use crate::assets::AssetState;
use crate::gfx::{
    draw_placeholder, draw_sprite_frame, draw_texture_fit, fill_gradient_h, fill_gradient_v,
    fill_rect, stroke_rect, Fit,
};
use macroquad::prelude::*;

/// Look of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowStyle {
    /// Deep blue window with a light bevelled border.
    #[default]
    Normal,
    /// Darker body, for inner panels or inactive windows.
    Panel,
}

/// Draw a window with the normal style.
pub fn draw_window(r: Rect) {
    draw_window_ex(r, WindowStyle::Normal, 1.0);
}

/// Draw a window. `alpha` fades the whole window (0..=1).
pub fn draw_window_ex(r: Rect, style: WindowStyle, alpha: f32) {
    if r.w < 8.0 || r.h < 8.0 || alpha <= 0.0 {
        return;
    }
    let a = |c: Color| Color::new(c.r, c.g, c.b, c.a * alpha);
    // Soft shadow below-right.
    fill_rect(Rect::new(r.x + 2.0, r.y + 2.0, r.w, r.h), a(theme::WINDOW_SHADOW));
    let (top, bottom) = match style {
        WindowStyle::Normal => (theme::WIN_TOP, theme::WIN_BOTTOM),
        WindowStyle::Panel => (theme::PANEL_TOP, theme::PANEL_BOTTOM),
    };
    fill_gradient_v(r, a(top), a(bottom));
    // Frame: dark outline, light bevel (brighter top-left), dark inner line.
    stroke_rect(r, a(theme::BORDER_OUTER));
    let b = inset(r, 1.0);
    fill_rect(Rect::new(b.x, b.y, b.w, 1.0), a(theme::BORDER_LIGHT));
    fill_rect(Rect::new(b.x, b.y, 1.0, b.h), a(theme::BORDER_LIGHT));
    fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), a(theme::BORDER_MID));
    fill_rect(Rect::new(b.right() - 1.0, b.y + 1.0, 1.0, b.h - 1.0), a(theme::BORDER_MID));
    stroke_rect(inset(r, 2.0), a(theme::BORDER_INNER));
}

/// `r` shrunk by `d` on every side.
pub fn inset(r: Rect, d: f32) -> Rect {
    Rect::new(r.x + d, r.y + d, (r.w - 2.0 * d).max(0.0), (r.h - 2.0 * d).max(0.0))
}

/// Content area of a window (inside border and padding).
pub fn content_rect(r: Rect) -> Rect {
    inset(r, theme::PADDING)
}

/// Selection highlight bar. `active` is false for menus that do not have focus.
pub fn draw_highlight(r: Rect, active: bool, time: f64) {
    if active {
        let pulse = 0.85 + 0.15 * ((time * 4.0).sin() as f32);
        let t = theme::CURSOR_TOP;
        let b = theme::CURSOR_BOTTOM;
        fill_gradient_v(
            r,
            Color::new(t.r, t.g, t.b, t.a * pulse),
            Color::new(b.r, b.g, b.b, b.a * pulse),
        );
        fill_rect(Rect::new(r.x, r.y, r.w, 1.0), Color::new(1.0, 1.0, 1.0, 0.25));
    } else {
        fill_rect(r, theme::CURSOR_IDLE);
    }
}

/// Small right-pointing triangle cursor whose tip is at `(x, y)`, bobbing horizontally.
pub fn draw_arrow_cursor(x: f32, y: f32, time: f64) {
    let bob = if (time * 3.0).fract() < 0.5 { 0.0 } else { 1.0 };
    let x = x - bob;
    draw_triangle(
        vec2(x, y),
        vec2(x - 5.0, y - 4.0),
        vec2(x - 5.0, y + 4.0),
        theme::CURSOR_ARROW,
    );
    draw_triangle(
        vec2(x - 1.0, y),
        vec2(x - 5.0, y - 3.0),
        vec2(x - 5.0, y - 1.0),
        Color::new(1.0, 1.0, 1.0, 0.6),
    );
}

/// Small triangle pointing up or down, centred at `(cx, cy)` (scroll indicators, "more text").
pub fn draw_small_arrow(cx: f32, cy: f32, down: bool, color: Color) {
    if down {
        draw_triangle(
            vec2(cx - 3.0, cy - 2.0),
            vec2(cx + 3.0, cy - 2.0),
            vec2(cx, cy + 2.0),
            color,
        );
    } else {
        draw_triangle(
            vec2(cx - 3.0, cy + 2.0),
            vec2(cx + 3.0, cy + 2.0),
            vec2(cx, cy - 2.0),
            color,
        );
    }
}

/// Small triangle pointing left or right, centred at `(cx, cy)` (value selectors).
pub fn draw_side_arrow(cx: f32, cy: f32, right: bool, color: Color) {
    if right {
        draw_triangle(
            vec2(cx - 2.0, cy - 3.0),
            vec2(cx - 2.0, cy + 3.0),
            vec2(cx + 2.0, cy),
            color,
        );
    } else {
        draw_triangle(
            vec2(cx + 2.0, cy - 3.0),
            vec2(cx + 2.0, cy + 3.0),
            vec2(cx - 2.0, cy),
            color,
        );
    }
}

/// Horizontal separator line with a highlight below.
pub fn draw_divider(x: f32, y: f32, w: f32) {
    fill_rect(Rect::new(x, y, w, 1.0), theme::BORDER_INNER);
    fill_rect(Rect::new(x, y + 1.0, w, 1.0), Color::new(1.0, 1.0, 1.0, 0.18));
}

/// Title strip at the top of a screen: gradient band with a heading.
pub fn draw_title_bar(ctx: &Ctx, title: &str) {
    let r = Rect::new(0.0, 0.0, crate::gfx::VIRTUAL_W, 20.0);
    fill_gradient_h(r, theme::WIN_TOP, theme::WIN_BOTTOM.with_alpha(0.6));
    fill_rect(Rect::new(0.0, 20.0, r.w, 1.0), theme::BORDER_LIGHT.with_alpha(0.6));
    ctx.gfx.text(
        title,
        10.0,
        2.0,
        crate::gfx::TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
    );
}

/// Draw `gfx/<key>.png` into `r`. Draws nothing while it loads and a placeholder when it is
/// missing.
pub fn draw_image(ctx: &Ctx, key: &str, r: Rect, fit: Fit) {
    match ctx.media.texture_state(key) {
        AssetState::Ready => {
            if let Some(t) = ctx.media.texture(key) {
                draw_texture_fit(&t, r, fit, WHITE);
            }
        }
        AssetState::Loading => {}
        AssetState::Missing => draw_placeholder(r, key),
    }
}

/// Draw one frame of the sprite sheet `gfx/<key>.png` (frame size `frame`, cell `(column,
/// row)`) with its top-left at `pos`; a placeholder box of the frame size when the sheet is
/// missing.
pub fn draw_sprite(ctx: &Ctx, key: &str, frame: Vec2, cell: (u32, u32), pos: Vec2, flip_x: bool) {
    match ctx.media.texture_state(key) {
        AssetState::Ready => {
            if let Some(t) = ctx.media.texture(key) {
                draw_sprite_frame(&t, frame, cell, pos, flip_x, WHITE);
            }
        }
        AssetState::Loading => {}
        AssetState::Missing => draw_placeholder(Rect::new(pos.x, pos.y, frame.x, frame.y), key),
    }
}

/// Draw a 16×16 icon from the icon atlas, or a placeholder when unavailable.
pub fn draw_icon(ctx: &Ctx, key: &str, pos: Vec2) {
    match ctx.media.icon(key) {
        Some((tex, src)) => draw_texture_ex(
            &tex,
            pos.x.round(),
            pos.y.round(),
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(16.0, 16.0)),
                source: Some(src),
                ..Default::default()
            },
        ),
        None => {
            if ctx.media.icon_state(key) == AssetState::Missing {
                draw_placeholder(Rect::new(pos.x + 2.0, pos.y + 2.0, 12.0, 12.0), key);
            }
        }
    }
}

/// Portrait box (64×80 virtual in dialogue): frame, then the officer's portrait, the
/// `_unknown` portrait, or a procedural silhouette.
pub fn draw_portrait(ctx: &Ctx, key: Option<&str>, r: Rect) {
    draw_window_ex(r, WindowStyle::Panel, 1.0);
    let inner = inset(r, theme::BORDER);
    fill_gradient_v(inner, Color::from_hex(0x2a3668), Color::from_hex(0x10163a));
    let tex = key.and_then(|k| ctx.media.portrait(k));
    match tex {
        Some(t) => draw_texture_fit(&t, inner, Fit::Cover, WHITE),
        None => draw_silhouette(inner),
    }
}

/// Head-and-shoulders silhouette used when no portrait art exists.
pub fn draw_silhouette(r: Rect) {
    let c = Color::from_hex(0x0a0f2c).with_alpha(0.9);
    let cx = r.x + r.w / 2.0;
    let head_r = r.w * 0.2;
    let head_y = r.y + r.h * 0.38;
    draw_circle(cx, head_y, head_r, c);
    // Topknot / hat.
    fill_rect(
        Rect::new(cx - head_r * 0.35, head_y - head_r * 1.45, head_r * 0.7, head_r * 0.6),
        c,
    );
    // Shoulders.
    let sy = head_y + head_r * 1.2;
    draw_triangle(
        vec2(cx, sy - head_r * 0.4),
        vec2(r.x + r.w * 0.05, r.bottom()),
        vec2(r.right() - r.w * 0.05, r.bottom()),
        c,
    );
    fill_rect(
        Rect::new(r.x + r.w * 0.12, sy + head_r * 0.6, r.w * 0.76, r.bottom() - sy - head_r * 0.6),
        c,
    );
}
