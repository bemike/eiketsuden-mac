//! Artwork with intentional fallbacks: drama backgrounds (`gfx/bg/<key>.png`), portrait cards
//! (`gfx/portraits/<key>.png`) and unit sprite icons (`gfx/units/<sprite>_<side>.png`).
//!
//! Packs may ship without some of this art (or it may still be loading), so every helper has a
//! fallback that looks like a deliberate design rather than a missing file:
//!
//! * a missing background is painted procedurally in the mood of its key (dusky sky and ridges for
//!   `field`, lantern-lit pillars for `palace`, stars for `night`, ...);
//! * a missing portrait uses `portraits/_unknown` (through [`crate::assets::Media::portrait`]),
//!   then a head-and-shoulders silhouette;
//! * a missing unit sheet shows the usual placeholder box.
//!
//! Hi-res art is drawn with a tint, so callers can fade (`alpha`) and dim (`light`) it.

use super::theme;
use crate::app::Ctx;
use crate::assets::AssetState;
use crate::gfx::{
    draw_placeholder, draw_sprite_frame, draw_texture_fit, fill_gradient_v, fill_rect, key_color,
    Fit, SCREEN, VIRTUAL_H, VIRTUAL_W,
};
use macroquad::prelude::*;

/// Colour with its alpha multiplied by `a`.
fn fade(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, c.a * a.clamp(0.0, 1.0))
}

/// `c` scaled towards black by `k` (0 = black, 1 = unchanged).
fn shade(c: Color, k: f32) -> Color {
    Color::new(c.r * k, c.g * k, c.b * k, c.a)
}

/// Deterministic pseudo random value in 0..1 for an integer seed.
fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

fn key_seed(key: &str) -> u32 {
    key.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// Texture key of a drama background.
pub fn background_texture_key(key: &str) -> String {
    format!("bg/{key}")
}

/// Load state of a drama background (requests it when unknown).
pub fn background_state(ctx: &Ctx, key: &str) -> AssetState {
    ctx.media.texture_state(&background_texture_key(key))
}

/// How the procedural stand-in for a background is composed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenery {
    /// Plain colour (`black`).
    Plain,
    /// Sky gradient with mountain ridges.
    Outdoor,
    /// Night sky: ridges and stars.
    Night,
    /// Dim hall with pillars.
    Indoor,
}

/// Sky/top colour, ground/bottom colour and composition of the stand-in for `key`. The keys of
/// `docs/ASSETS.md` have hand-picked moods; other keys get stable colours derived from the key.
pub fn fallback_palette(key: &str) -> (Color, Color, Scenery) {
    let c = Color::from_hex;
    match key {
        "black" => (BLACK, BLACK, Scenery::Plain),
        "night" => (c(0x060918), c(0x1b2044), Scenery::Night),
        "palace" => (c(0x4a1a1c), c(0x160608), Scenery::Indoor),
        "castle" => (c(0x3b3f4f), c(0x13151c), Scenery::Indoor),
        "camp" => (c(0x5a3418), c(0x140b06), Scenery::Outdoor),
        "village" => (c(0x6a6a8e), c(0x2e2616), Scenery::Outdoor),
        "field" => (c(0x6d8fb0), c(0x34401f), Scenery::Outdoor),
        "river" => (c(0x5a7fa6), c(0x172a40), Scenery::Outdoor),
        "mountain" => (c(0x5d6c84), c(0x1a1f2b), Scenery::Outdoor),
        "town" => (c(0x7a6a5a), c(0x2a2018), Scenery::Indoor),
        other => {
            let k = key_color(other);
            (
                Color::new(0.35 + k.r * 0.4, 0.35 + k.g * 0.4, 0.4 + k.b * 0.4, 1.0),
                shade(k, 0.35),
                Scenery::Outdoor,
            )
        }
    }
}

/// Height of a mountain ridge at `x`.
fn ridge(x: f32, seed: u32, base: f32, amp: f32) -> f32 {
    let p1 = hash01(seed) * std::f32::consts::TAU;
    let p2 = hash01(seed.wrapping_add(1)) * std::f32::consts::TAU;
    let p3 = hash01(seed.wrapping_add(2)) * std::f32::consts::TAU;
    base - amp
        * (0.55 * (x / 61.0 + p1).sin().abs()
            + 0.3 * (x / 27.0 + p2).sin()
            + 0.15 * (x / 11.0 + p3).sin())
}

fn draw_ridge(seed: u32, base: f32, amp: f32, color: Color) {
    let step = 4.0;
    let mut x = 0.0;
    while x < VIRTUAL_W {
        let y0 = ridge(x, seed, base, amp);
        let y1 = ridge(x + step, seed, base, amp);
        draw_triangle(vec2(x, y0), vec2(x + step, y1), vec2(x, VIRTUAL_H), color);
        draw_triangle(
            vec2(x + step, y1),
            vec2(x + step, VIRTUAL_H),
            vec2(x, VIRTUAL_H),
            color,
        );
        x += step;
    }
}

/// Procedural stand-in for background `key` (see [`fallback_palette`]).
pub fn draw_fallback_background(key: &str, alpha: f32) {
    let (top, bottom, scenery) = fallback_palette(key);
    let seed = key_seed(key);
    fill_gradient_v(SCREEN, fade(top, alpha), fade(bottom, alpha));
    match scenery {
        Scenery::Plain => {}
        Scenery::Outdoor | Scenery::Night => {
            if scenery == Scenery::Night {
                for i in 0..60u32 {
                    let x = (hash01(seed ^ (i * 3)) * VIRTUAL_W).floor();
                    let y = (hash01(seed ^ (i * 3 + 1)) * VIRTUAL_H * 0.55).floor();
                    let a = 0.25 + 0.6 * hash01(i * 7 + 3);
                    fill_rect(
                        Rect::new(x, y, 1.0, 1.0),
                        Color::new(1.0, 0.95, 0.85, a * alpha),
                    );
                }
            }
            let far = shade(
                Color::new(
                    (top.r + bottom.r) / 2.0,
                    (top.g + bottom.g) / 2.0,
                    (top.b + bottom.b) / 2.0,
                    1.0,
                ),
                0.8,
            );
            draw_ridge(seed, 165.0, 55.0, fade(far, alpha));
            draw_ridge(
                seed.wrapping_add(17),
                205.0,
                40.0,
                fade(shade(bottom, 0.9), alpha),
            );
            draw_ridge(
                seed.wrapping_add(41),
                238.0,
                22.0,
                fade(shade(bottom, 0.55), alpha),
            );
            // Haze over the far ridge.
            fill_gradient_v(
                Rect::new(0.0, 140.0, VIRTUAL_W, 60.0),
                fade(Color::new(top.r, top.g, top.b, 0.0), alpha),
                fade(Color::new(top.r, top.g, top.b, 0.25), alpha),
            );
        }
        Scenery::Indoor => {
            // Floor.
            fill_gradient_v(
                Rect::new(0.0, 190.0, VIRTUAL_W, 80.0),
                fade(shade(bottom, 1.6), alpha),
                fade(shade(bottom, 0.6), alpha),
            );
            // Pillars with a warm lantern glow between them.
            let pillar = fade(shade(bottom, 0.7), alpha);
            let edge = fade(shade(top, 1.3), alpha * 0.5);
            for (i, x) in [36.0, 132.0, 330.0, 426.0].into_iter().enumerate() {
                fill_rect(Rect::new(x, 0.0, 18.0, 200.0), pillar);
                fill_rect(Rect::new(x, 0.0, 2.0, 200.0), edge);
                if i % 2 == 0 {
                    let gx = x + 48.0;
                    for (r, a) in [(26.0, 0.05), (16.0, 0.08), (8.0, 0.14)] {
                        draw_circle(gx, 70.0, r, Color::new(1.0, 0.75, 0.4, a * alpha));
                    }
                }
            }
            // Beam across the top.
            fill_rect(Rect::new(0.0, 16.0, VIRTUAL_W, 10.0), pillar);
        }
    }
    // Vignette.
    fill_gradient_v(
        Rect::new(0.0, 0.0, VIRTUAL_W, 40.0),
        Color::new(0.0, 0.0, 0.0, 0.35 * alpha),
        Color::new(0.0, 0.0, 0.0, 0.0),
    );
    fill_gradient_v(
        Rect::new(0.0, VIRTUAL_H - 60.0, VIRTUAL_W, 60.0),
        Color::new(0.0, 0.0, 0.0, 0.0),
        Color::new(0.0, 0.0, 0.0, 0.45 * alpha),
    );
}

/// Draw background `key` over the whole canvas with opacity `alpha`: the image when it is loaded,
/// the procedural stand-in when it does not exist, nothing while it loads. Returns whether
/// something was drawn.
pub fn draw_background(ctx: &Ctx, key: &str, alpha: f32) -> bool {
    let tex_key = background_texture_key(key);
    match ctx.media.texture_state(&tex_key) {
        AssetState::Ready => match ctx.media.texture(&tex_key) {
            Some(t) => {
                draw_texture_fit(&t, SCREEN, Fit::Cover, fade(WHITE, alpha));
                true
            }
            None => false,
        },
        AssetState::Loading => false,
        AssetState::Missing => {
            draw_fallback_background(key, alpha);
            true
        }
    }
}

/// Portrait art for a key.
pub enum PortraitArt {
    /// The portrait, or `portraits/_unknown` when the officer has none.
    Ready(Texture2D),
    /// Still loading.
    Loading,
    /// Neither the portrait nor `_unknown` exists.
    Missing,
}

/// Look up portrait `key` (falling back to `portraits/_unknown`).
pub fn portrait_art(ctx: &Ctx, key: &str) -> PortraitArt {
    let own = format!("portraits/{key}");
    match ctx.media.texture_state(&own) {
        AssetState::Loading => PortraitArt::Loading,
        AssetState::Ready => ctx
            .media
            .texture(&own)
            .map_or(PortraitArt::Loading, PortraitArt::Ready),
        AssetState::Missing => match ctx.media.texture_state(crate::assets::UNKNOWN_PORTRAIT) {
            AssetState::Loading => PortraitArt::Loading,
            AssetState::Ready => ctx
                .media
                .texture(crate::assets::UNKNOWN_PORTRAIT)
                .map_or(PortraitArt::Loading, PortraitArt::Ready),
            AssetState::Missing => PortraitArt::Missing,
        },
    }
}

/// Head-and-shoulders silhouette inside `r` (the same shape as
/// [`super::window::draw_silhouette`], with an opacity).
pub fn draw_silhouette_alpha(r: Rect, alpha: f32) {
    let c = Color::from_hex(0x0a0f2c).with_alpha(0.9 * alpha.clamp(0.0, 1.0));
    let cx = r.x + r.w / 2.0;
    let head_r = r.w * 0.2;
    let head_y = r.y + r.h * 0.38;
    draw_circle(cx, head_y, head_r, c);
    fill_rect(
        Rect::new(
            cx - head_r * 0.35,
            head_y - head_r * 1.45,
            head_r * 0.7,
            head_r * 0.6,
        ),
        c,
    );
    let sy = head_y + head_r * 1.2;
    draw_triangle(
        vec2(cx, sy - head_r * 0.4),
        vec2(r.x + r.w * 0.05, r.bottom()),
        vec2(r.right() - r.w * 0.05, r.bottom()),
        c,
    );
    fill_rect(
        Rect::new(
            r.x + r.w * 0.12,
            sy + head_r * 0.6,
            r.w * 0.76,
            r.bottom() - sy - head_r * 0.6,
        ),
        c,
    );
}

/// A framed portrait card (stage portraits of drama scenes, officer pages). `alpha` fades the
/// whole card, `light` dims it (1 = full brightness).
pub fn draw_portrait_card(ctx: &Ctx, key: Option<&str>, r: Rect, alpha: f32, light: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 || r.w < 4.0 || r.h < 4.0 {
        return;
    }
    let light = light.clamp(0.0, 1.0);
    fill_rect(
        Rect::new(r.x + 3.0, r.y + 3.0, r.w, r.h),
        Color::new(0.0, 0.0, 0.0, 0.45 * alpha),
    );
    let inner = Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0);
    fill_gradient_v(
        inner,
        fade(shade(Color::from_hex(0x2a3668), light), alpha),
        fade(shade(Color::from_hex(0x10163a), light), alpha),
    );
    match key.map(|k| portrait_art(ctx, k)) {
        Some(PortraitArt::Ready(t)) => {
            draw_texture_fit(
                &t,
                inner,
                Fit::Cover,
                Color::new(light, light, light, alpha),
            );
        }
        Some(PortraitArt::Loading) => {}
        Some(PortraitArt::Missing) | None => {
            draw_silhouette_alpha(inner, alpha);
            if light < 1.0 {
                fill_rect(inner, Color::new(0.0, 0.0, 0.0, (1.0 - light) * alpha));
            }
        }
    }
    // Frame: dark outline, light bevel, dark inner line.
    let outline = fade(theme::BORDER_OUTER, alpha);
    let bevel = fade(shade(theme::BORDER_MID, 0.4 + 0.6 * light), alpha);
    crate::gfx::stroke_rect(r, outline);
    crate::gfx::stroke_rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), bevel);
    fill_rect(
        Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, 1.0),
        fade(shade(theme::BORDER_LIGHT, 0.5 + 0.5 * light), alpha),
    );
}

/// Frame size of a unit sheet: the layout of `docs/ASSETS.md` has 4 columns and 6 rows.
pub fn unit_frame_size(sheet_w: f32, sheet_h: f32) -> Vec2 {
    vec2((sheet_w / 4.0).floor(), (sheet_h / 6.0).floor())
}

/// Draw a unit sprite (`units/<sprite>_<side>`) standing with its feet at `feet` (bottom centre of
/// the frame), facing down, walk frame `step` (0..4). A placeholder box when the sheet is missing.
pub fn draw_unit(ctx: &Ctx, sprite: &str, side: &str, feet: Vec2, step: u32) {
    let key = format!("units/{sprite}_{side}");
    match ctx.media.texture_state(&key) {
        AssetState::Ready => {
            if let Some(t) = ctx.media.texture(&key) {
                let frame = unit_frame_size(t.width(), t.height());
                if frame.x >= 1.0 && frame.y >= 1.0 {
                    let pos = vec2((feet.x - frame.x / 2.0).round(), feet.y - frame.y);
                    draw_sprite_frame(&t, frame, (0, step % 4), pos, false, WHITE);
                }
            }
        }
        AssetState::Loading => {}
        AssetState::Missing => {
            draw_placeholder(Rect::new(feet.x - 7.0, feet.y - 14.0, 14.0, 14.0), &key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_backgrounds_have_moods() {
        assert_eq!(fallback_palette("black").2, Scenery::Plain);
        assert_eq!(fallback_palette("night").2, Scenery::Night);
        assert_eq!(fallback_palette("palace").2, Scenery::Indoor);
        assert_eq!(fallback_palette("field").2, Scenery::Outdoor);
        // Unknown keys get a stable palette.
        assert_eq!(fallback_palette("desert"), fallback_palette("desert"));
        assert_ne!(fallback_palette("desert").0, fallback_palette("snow").0);
    }

    #[test]
    fn unit_frames_follow_the_sheet_layout() {
        assert_eq!(unit_frame_size(96.0, 144.0), vec2(24.0, 24.0));
        assert_eq!(unit_frame_size(128.0, 144.0), vec2(32.0, 24.0));
        assert_eq!(unit_frame_size(64.0, 96.0), vec2(16.0, 16.0));
    }
}
