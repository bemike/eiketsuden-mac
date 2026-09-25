//! Procedural night landscape used behind the title, credits and game over screens when the
//! pack has no title artwork: gradient sky, stars, moon, layered mountain ridges and mist.
//! Everything is computed from fixed seeds, so it looks the same every frame.

use crate::gfx::{fill_gradient_v, fill_rect, VIRTUAL_H, VIRTUAL_W};
use macroquad::prelude::*;

/// Deterministic pseudo random value in 0..1 for an integer seed.
fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// Height of a mountain ridge at `x` (sum of a few sines with seeded phases).
fn ridge(x: f32, seed: u32, base: f32, amp: f32) -> f32 {
    let p1 = hash01(seed) * 6.28;
    let p2 = hash01(seed + 1) * 6.28;
    let p3 = hash01(seed + 2) * 6.28;
    base - amp
        * (0.55 * (x / 57.0 + p1).sin().abs()
            + 0.3 * (x / 23.0 + p2).sin()
            + 0.15 * (x / 9.0 + p3).sin())
}

/// Draw the backdrop over the whole canvas. `time` animates the stars and mist slightly.
pub fn draw_backdrop(time: f64) {
    let t = time as f32;
    fill_gradient_v(
        Rect::new(0.0, 0.0, VIRTUAL_W, VIRTUAL_H * 0.75),
        Color::from_hex(0x05071a),
        Color::from_hex(0x2a2350),
    );
    fill_gradient_v(
        Rect::new(0.0, VIRTUAL_H * 0.75, VIRTUAL_W, VIRTUAL_H * 0.25),
        Color::from_hex(0x2a2350),
        Color::from_hex(0x120e24),
    );

    // Stars.
    for i in 0..70u32 {
        let x = (hash01(i * 3) * VIRTUAL_W).floor();
        let y = (hash01(i * 3 + 1) * VIRTUAL_H * 0.6).floor();
        let tw = 0.55 + 0.45 * (t * (0.8 + hash01(i * 3 + 2) * 2.0) + i as f32).sin();
        let size = if i % 11 == 0 { 2.0 } else { 1.0 };
        fill_rect(
            Rect::new(x, y, size, size),
            Color::new(1.0, 0.95, 0.85, 0.35 + 0.5 * tw * hash01(i * 7)),
        );
    }

    // Moon with a soft halo.
    let (mx, my) = (372.0, 58.0);
    for (r, a) in [(34.0, 0.04), (26.0, 0.07), (20.0, 0.1)] {
        draw_circle(mx, my, r, Color::new(1.0, 0.92, 0.7, a));
    }
    draw_circle(mx, my, 15.0, Color::from_hex(0xf6e8c0));
    draw_circle(mx + 5.0, my - 3.0, 13.0, Color::from_hex(0xfdf3d6).with_alpha(0.5));

    // Mountain ridges, far to near.
    let layers: [(u32, f32, f32, u32); 3] = [
        (11, 170.0, 60.0, 0x2c2a58),
        (23, 200.0, 50.0, 0x1b1a3c),
        (37, 232.0, 36.0, 0x0e0d22),
    ];
    for (seed, base, amp, color) in layers {
        let c = Color::from_hex(color);
        let step = 4.0;
        let mut x = 0.0;
        while x < VIRTUAL_W {
            let y0 = ridge(x, seed, base, amp);
            let y1 = ridge(x + step, seed, base, amp);
            draw_triangle(
                vec2(x, y0),
                vec2(x + step, y1),
                vec2(x, VIRTUAL_H),
                c,
            );
            draw_triangle(
                vec2(x + step, y1),
                vec2(x + step, VIRTUAL_H),
                vec2(x, VIRTUAL_H),
                c,
            );
            x += step;
        }
    }

    // Drifting mist band.
    let drift = (t * 6.0) % VIRTUAL_W;
    for k in 0..2 {
        let x = drift - VIRTUAL_W * k as f32;
        fill_gradient_v(
            Rect::new(x, 206.0, VIRTUAL_W, 18.0),
            Color::new(0.7, 0.72, 0.9, 0.0),
            Color::new(0.7, 0.72, 0.9, 0.08),
        );
        fill_gradient_v(
            Rect::new(x, 224.0, VIRTUAL_W, 14.0),
            Color::new(0.7, 0.72, 0.9, 0.08),
            Color::new(0.7, 0.72, 0.9, 0.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic_and_in_range() {
        for s in 0..1000 {
            let v = hash01(s);
            assert!((0.0..=1.0).contains(&v));
            assert_eq!(v, hash01(s));
        }
        assert_ne!(hash01(1), hash01(2));
    }
}
