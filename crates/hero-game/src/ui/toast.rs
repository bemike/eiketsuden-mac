//! Transient messages: toasts (small notices at the top, drawn by the app above every screen)
//! and banners (large centred captions such as "제1장" or "아군 페이즈", owned by a screen).

use super::theme;
use super::window::{draw_window_ex, WindowStyle};
use crate::gfx::{fill_gradient_h, fill_rect, Align, FontId, Gfx, TextStyle, VIRTUAL_H, VIRTUAL_W};
use macroquad::prelude::*;
use std::collections::VecDeque;

/// Seconds a toast stays fully visible.
pub const TOAST_SECONDS: f32 = 2.4;
const TOAST_FADE: f32 = 0.25;
const MAX_TOASTS: usize = 3;

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    age: f32,
}

/// Queue of short notices. Use [`crate::app::Ctx::toast`] to add one.
#[derive(Debug, Default)]
pub struct Toasts {
    items: VecDeque<Toast>,
}

impl Toasts {
    pub fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        macroquad::logging::info!("toast: {}", text);
        if self.items.len() == MAX_TOASTS {
            self.items.pop_front();
        }
        self.items.push_back(Toast { text, age: 0.0 });
    }

    pub fn update(&mut self, dt: f32) {
        for t in &mut self.items {
            t.age += dt;
        }
        self.items
            .retain(|t| t.age < TOAST_SECONDS + 2.0 * TOAST_FADE);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn draw(&self, gfx: &Gfx) {
        let mut y = 6.0;
        for t in &self.items {
            let alpha = if t.age < TOAST_FADE {
                t.age / TOAST_FADE
            } else if t.age > TOAST_SECONDS + TOAST_FADE {
                1.0 - (t.age - TOAST_SECONDS - TOAST_FADE) / TOAST_FADE
            } else {
                1.0
            }
            .clamp(0.0, 1.0);
            let lines = gfx.wrap(&t.text, FontId::Main, 1, VIRTUAL_W - 60.0);
            let w = lines
                .iter()
                .map(|l| gfx.text_width(l, FontId::Main, 1))
                .fold(0.0, f32::max)
                + 2.0 * theme::PADDING
                + 8.0;
            let h = lines.len() as f32 * 16.0 + 2.0 * theme::PADDING;
            let r = Rect::new(((VIRTUAL_W - w) / 2.0).round(), y, w.round(), h);
            draw_window_ex(r, WindowStyle::Normal, alpha * 0.95);
            let style = TextStyle::main(theme::TEXT.with_alpha(alpha));
            for (i, line) in lines.iter().enumerate() {
                gfx.text_aligned(
                    line,
                    r.x,
                    r.y + theme::PADDING + i as f32 * 16.0,
                    r.w,
                    Align::Center,
                    style,
                );
            }
            y += h + 4.0;
        }
    }
}

/// A large caption across the middle of the screen that slides in, holds and fades out.
#[derive(Debug, Clone)]
pub struct Banner {
    title: String,
    subtitle: Option<String>,
    age: f32,
    duration: f32,
}

const BANNER_IN: f32 = 0.35;
const BANNER_OUT: f32 = 0.4;

impl Banner {
    /// `duration` is the total time on screen in seconds (at least the in/out animation).
    pub fn new(title: impl Into<String>, subtitle: Option<&str>, duration: f32) -> Banner {
        Banner {
            title: title.into(),
            subtitle: subtitle.map(str::to_string),
            age: 0.0,
            duration: duration.max(BANNER_IN + BANNER_OUT),
        }
    }

    /// Advance; returns `true` once the banner has disappeared.
    pub fn update(&mut self, dt: f32) -> bool {
        self.age += dt;
        self.is_done()
    }

    pub fn is_done(&self) -> bool {
        self.age >= self.duration
    }

    /// Skip to the fade-out.
    pub fn dismiss(&mut self) {
        self.age = self.age.max(self.duration - BANNER_OUT);
    }

    pub fn draw(&self, gfx: &Gfx) {
        if self.is_done() {
            return;
        }
        let t_in = (self.age / BANNER_IN).min(1.0);
        let t_out = ((self.duration - self.age) / BANNER_OUT).clamp(0.0, 1.0);
        let alpha = t_in.min(t_out);
        let ease = 1.0 - (1.0 - t_in).powi(3);
        let h = if self.subtitle.is_some() { 52.0 } else { 40.0 };
        let y = ((VIRTUAL_H - h) / 2.0).round();
        let band = Rect::new(0.0, y, VIRTUAL_W, h);
        let dark = Color::new(0.02, 0.03, 0.1, 0.85 * alpha);
        let mid = Color::new(0.08, 0.12, 0.35, 0.85 * alpha);
        fill_gradient_h(Rect::new(0.0, y, VIRTUAL_W / 2.0, h), dark, mid);
        fill_gradient_h(Rect::new(VIRTUAL_W / 2.0, y, VIRTUAL_W / 2.0, h), mid, dark);
        let gold = theme::TEXT_ACCENT.with_alpha(alpha);
        fill_rect(Rect::new(0.0, band.y, VIRTUAL_W, 1.0), gold);
        fill_rect(Rect::new(0.0, band.bottom() - 1.0, VIRTUAL_W, 1.0), gold);
        let slide = (1.0 - ease) * 40.0;
        let title_style = TextStyle::main(theme::TEXT.with_alpha(alpha))
            .size(2)
            .shadow(theme::TEXT_SHADOW.with_alpha(alpha));
        gfx.text_aligned(
            &self.title,
            slide,
            y + 4.0,
            VIRTUAL_W,
            Align::Center,
            title_style,
        );
        if let Some(sub) = &self.subtitle {
            gfx.text_aligned(
                sub,
                -slide,
                y + 36.0,
                VIRTUAL_W,
                Align::Center,
                TextStyle::main(theme::TEXT_ACCENT.with_alpha(alpha)),
            );
        }
    }
}
