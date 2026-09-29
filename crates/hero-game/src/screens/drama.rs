//! Drama screen: plays a drama scene with `hero_core::drama::DramaRunner` — backgrounds,
//! portraits, dialogue and narration boxes, choices, title cards, fades, music and sounds.
//!
//! **Contract shared by the campaign screens (camp/drama owner) and the battle screen:**
//! the three constructors below are the only public surface other screens rely on.
//!
//! # Presentation
//!
//! | step | shown as |
//! |---|---|
//! | `Background` | `gfx/bg/<key>.png` covering the screen, cross-faded; `@bg none` is black. Missing art is painted procedurally ([`crate::ui::art`]). |
//! | `Show` / `Hide` | framed portraits in the left / centre / right slot above the message box, faded in and out; portraits of characters who are not speaking are dimmed |
//! | `Line` | message box with the speaker's name tab and 64×80 portrait, typed at the text speed setting |
//! | `Narration` | message box without name, centred lines |
//! | `Title` | large centred caption fading in and out (chapter titles); the first line is big, the others are subtitles |
//! | `Choice` | choice box above the last message ([`DramaRunner::choose`]) |
//! | `Wait`, `FadeOut`, `FadeIn` | pause, fade the scene to black and back (text stays readable above the fade) |
//! | `Music`, `Sound` | `ctx.audio` |
//! | `Joined`, `Received` | banner (`관우 합류!`, `금 500 획득`) with a sound |
//!
//! An overlay (battle intro/outro/event scene) starts without a background, so the battle map
//! stays visible (slightly dimmed) until the scene sets one.
//!
//! # Controls
//!
//! * confirm (Z / Enter / Space / click / tap) completes the page, then continues;
//! * holding Ctrl, Tab or a cancel key (X / Esc / Backspace) fast-forwards (빨리 넘기기): pages
//!   appear at once and turn by themselves, pauses and fades are shortened; choices still wait;
//! * a short cancel press or a right click opens the scene menu: 계속 / 최근 대사 / 빨리 넘기기 /
//!   장면 건너뛰기 / 설정;
//! * L, PageUp or the mouse wheel (up) open the backlog (최근 대사);
//! * the buttons at the top right do the same for mouse and touch (in an overlay they sit below
//!   the top bar of the screen underneath, e.g. the battle HUD).
//!
//! **Scene skip** (after a confirmation) runs the rest of the scene without showing it: side
//! effects, music and the final background and portraits still apply, choices are still asked
//! (the skip resumes after them), and officers joining or items received are reported as toasts.

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::{AssetState, UNKNOWN_PORTRAIT};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{fill_gradient_h, fill_rect, Align, FontId, Gfx, TextStyle};
use crate::screens::duel::DuelView;
use crate::screens::settings::SettingsScreen;
use crate::ui::art::{background_state, draw_background, draw_portrait_card};
use crate::ui::backlog::{Backlog, BacklogView};
use crate::ui::dialog::{ChoiceBox, ChoiceEvent, ConfirmDialog, ConfirmEvent};
use crate::ui::dialogue::{DialogueBox, DialogueEvent};
use crate::ui::format;
use crate::ui::theme;
use crate::ui::window::{draw_highlight, draw_icon, draw_window_ex, inset, WindowStyle};
use hero_core::drama::{DramaRunner, Step};
use hero_core::pack::Pack;
use hero_core::script::{Cmd, Slot};
use macroquad::prelude::*;

/// Seconds for a background cross-fade.
const BG_FADE_SECONDS: f32 = 0.6;
/// Seconds for a portrait to fade in or out.
const PORTRAIT_FADE_SECONDS: f32 = 0.25;
/// Seconds for a portrait to dim or light up.
const LIGHT_SECONDS: f32 = 0.18;
/// Seconds for `@fade out` / `@fade in`.
const SCREEN_FADE_SECONDS: f32 = 0.6;
/// Brightness of portraits whose character is not speaking.
const DIMMED: f32 = 0.45;
/// Title card timing: fade in, hold, fade out.
const TITLE_IN: f32 = 0.8;
const TITLE_HOLD: f32 = 2.4;
const TITLE_OUT: f32 = 0.7;
/// Banner timing (officer joined, items received).
const NOTICE_SECONDS: f32 = 2.6;
const NOTICE_IN: f32 = 0.3;
const NOTICE_OUT: f32 = 0.35;
/// Holding a cancel key this long fast-forwards instead of opening the menu on release.
const HOLD_SECONDS: f32 = 0.3;
/// Animations and pauses run this much faster while fast-forwarding.
const FAST_FACTOR: f32 = 6.0;
/// Upper bound of steps executed in one frame (the runner itself ends endless `@goto` loops).
const MAX_STEPS_PER_FRAME: usize = 10_000;

/// What happens when the scene ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DramaEnd {
    /// Campaign `Drama` node: `Transition::Flow(Flow::Advance)`.
    Advance,
    /// Campaign `Ending` node with a scene: `Transition::Flow(Flow::Ending { title })`.
    Ending { title: String },
    /// Overlay (battle intro/outro, `BattleEvent::Drama`): `Transition::Pop`, so the screen below
    /// resumes (it receives `Enter::Resumed`).
    Pop,
}

// ----- stage: backdrop, portraits, screen fade -----------------------------------------------

/// What is behind the portraits.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Backdrop {
    /// Nothing: the screen below an overlay shows through.
    Transparent,
    Black,
    /// `gfx/bg/<key>.png`.
    Image(String),
}

impl Backdrop {
    fn from_step(key: Option<String>) -> Backdrop {
        match key {
            None => Backdrop::Black,
            Some(k) => Backdrop::Image(k),
        }
    }
}

/// Which portraits are lit.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Spotlight {
    /// Nobody is talking (title cards, pauses): every portrait is lit.
    All,
    /// Only the portrait with this key is lit.
    Speaker(String),
    /// Narration or a speaker without a portrait on stage: every portrait is dimmed.
    Nobody,
}

/// Target brightness of a stage portrait.
fn portrait_light(key: &str, spotlight: &Spotlight) -> f32 {
    match spotlight {
        Spotlight::All => 1.0,
        Spotlight::Speaker(k) if k == key => 1.0,
        Spotlight::Speaker(_) | Spotlight::Nobody => DIMMED,
    }
}

fn slot_index(slot: Slot) -> usize {
    match slot {
        Slot::Left => 0,
        Slot::Center => 1,
        Slot::Right => 2,
    }
}

/// Stage portrait size (4:5, like the portrait art) on canvases with room for it.
const STAGE_PORTRAIT: Vec2 = Vec2::new(104.0, 130.0);
/// Distance from the bottom edge of the canvas to the bottom edge of the stage portraits, just
/// above the message box portrait.
const STAGE_BOTTOM_MARGIN: f32 = 84.0;
/// Room kept free above the stage portraits (for the toolbar, also in an overlay).
const STAGE_TOP_MARGIN: f32 = 40.0;
/// Largest distance from the left and right edges of the canvas to the centres of the side
/// slots.
const SIDE_SLOT_INSET: f32 = 116.0;
/// Least horizontal gap between two slots.
const SLOT_GAP: f32 = 8.0;

/// Size of the stage portraits on a `canvas` sized canvas: [`STAGE_PORTRAIT`], shrunk (keeping
/// the 4:5 aspect) where the canvas is too low or too narrow for three of them.
fn stage_portrait_size(canvas: Vec2) -> Vec2 {
    let h = STAGE_PORTRAIT
        .y
        .min(canvas.y - STAGE_BOTTOM_MARGIN - STAGE_TOP_MARGIN);
    let w = (h * 0.8).min((canvas.x - 4.0 * SLOT_GAP) / 3.0);
    // A multiple of 4 wide, so the height is a whole number at 4:5.
    let w = ((w / 4.0).floor() * 4.0).max(4.0);
    vec2(w, w * 1.25)
}

/// Rectangle of portrait slot `index` (0 = left, 1 = centre, 2 = right) on a `canvas` sized
/// canvas.
fn slot_rect(canvas: Vec2, index: usize) -> Rect {
    let size = stage_portrait_size(canvas);
    let inset = SIDE_SLOT_INSET.min(canvas.x / 2.0 - size.x - SLOT_GAP);
    let cx = [inset, canvas.x / 2.0, canvas.x - inset][index.min(2)];
    Rect::new(
        (cx - size.x / 2.0).round(),
        canvas.y - STAGE_BOTTOM_MARGIN - size.y,
        size.x,
        size.y,
    )
}

#[derive(Debug, Clone, PartialEq)]
struct StagePortrait {
    key: String,
    alpha: f32,
    light: f32,
}

/// Move `value` towards `target` by at most `step`.
fn approach(value: f32, target: f32, step: f32) -> f32 {
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

#[derive(Debug, Clone)]
struct Stage {
    backdrop: Backdrop,
    previous: Backdrop,
    /// Opacity of `backdrop` over `previous` (1 = cross-fade done).
    mix: f32,
    slots: [Option<StagePortrait>; 3],
    /// Portraits fading out: (slot index, portrait).
    leaving: Vec<(usize, StagePortrait)>,
    /// Screen darkness (0 = clear, 1 = black) and where it is heading.
    fade: f32,
    fade_target: f32,
}

impl Stage {
    fn new(backdrop: Backdrop) -> Stage {
        Stage {
            previous: backdrop.clone(),
            backdrop,
            mix: 1.0,
            slots: [None, None, None],
            leaving: Vec::new(),
            fade: 0.0,
            fade_target: 0.0,
        }
    }

    fn set_backdrop(&mut self, backdrop: Backdrop, instant: bool) {
        if backdrop == self.backdrop {
            return;
        }
        self.previous = std::mem::replace(&mut self.backdrop, backdrop);
        self.mix = if instant { 1.0 } else { 0.0 };
    }

    fn show(&mut self, slot: Slot, key: String, instant: bool) {
        let i = slot_index(slot);
        if self.slots[i].as_ref().is_some_and(|p| p.key == key) {
            return;
        }
        self.hide_index(i, instant);
        self.slots[i] = Some(StagePortrait {
            key,
            alpha: if instant { 1.0 } else { 0.0 },
            light: 1.0,
        });
    }

    fn hide_index(&mut self, i: usize, instant: bool) {
        if let Some(p) = self.slots[i].take() {
            if !instant {
                self.leaving.push((i, p));
            }
        }
    }

    fn hide(&mut self, slot: Option<Slot>, instant: bool) {
        match slot {
            Some(s) => self.hide_index(slot_index(s), instant),
            None => (0..3).for_each(|i| self.hide_index(i, instant)),
        }
    }

    fn set_fade(&mut self, target: f32, instant: bool) {
        self.fade_target = target;
        if instant {
            self.fade = target;
        }
    }

    fn fade_done(&self) -> bool {
        (self.fade - self.fade_target).abs() < 1e-4
    }

    /// Advance the animations by `dt` scaled by `speed`. `backdrop_ready` holds the cross-fade
    /// while the new background image is still loading.
    fn update(&mut self, dt: f32, speed: f32, backdrop_ready: bool, spotlight: &Spotlight) {
        let dt = dt * speed;
        if backdrop_ready {
            self.mix = (self.mix + dt / BG_FADE_SECONDS).min(1.0);
        }
        for p in self.slots.iter_mut().flatten() {
            p.alpha = approach(p.alpha, 1.0, dt / PORTRAIT_FADE_SECONDS);
            p.light = approach(
                p.light,
                portrait_light(&p.key, spotlight),
                dt / LIGHT_SECONDS,
            );
        }
        for (_, p) in &mut self.leaving {
            p.alpha = approach(p.alpha, 0.0, dt / PORTRAIT_FADE_SECONDS);
        }
        self.leaving.retain(|(_, p)| p.alpha > 0.0);
        self.fade = approach(self.fade, self.fade_target, dt / SCREEN_FADE_SECONDS);
    }

    fn draw_backdrop(ctx: &Ctx, backdrop: &Backdrop, alpha: f32) {
        match backdrop {
            Backdrop::Transparent => {}
            Backdrop::Black => fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, alpha)),
            Backdrop::Image(key) => {
                draw_background(ctx, key, alpha);
            }
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let see_through = |b: &Backdrop| *b == Backdrop::Transparent;
        if self.mix < 1.0 {
            let prev_alpha = if see_through(&self.backdrop) {
                1.0 - self.mix
            } else {
                1.0
            };
            Self::draw_backdrop(ctx, &self.previous, prev_alpha);
        }
        Self::draw_backdrop(ctx, &self.backdrop, self.mix);
        // Over the screen below an overlay (no background of our own), dim it a little so the
        // scene reads well.
        let dim = if see_through(&self.backdrop) {
            1.0
        } else if self.mix < 1.0 && see_through(&self.previous) {
            1.0 - self.mix
        } else {
            0.0
        };
        let (screen, canvas) = (ctx.gfx.screen(), ctx.gfx.size());
        if dim > 0.0 {
            fill_rect(screen, Color::new(0.0, 0.0, 0.03, 0.22 * dim));
        }
        for (i, p) in &self.leaving {
            draw_portrait_card(ctx, Some(&p.key), slot_rect(canvas, *i), p.alpha, p.light);
        }
        for (i, p) in self.slots.iter().enumerate() {
            if let Some(p) = p {
                draw_portrait_card(ctx, Some(&p.key), slot_rect(canvas, i), p.alpha, p.light);
            }
        }
        if self.fade > 0.0 {
            fill_rect(screen, Color::new(0.0, 0.0, 0.0, self.fade.min(1.0)));
        }
    }
}

// ----- title cards and banners ----------------------------------------------------------------

/// Large centred caption (`@title`), e.g. `서장` / `세 영웅, 일어서다`.
#[derive(Debug, Clone)]
struct TitleCard {
    lines: Vec<String>,
    age: f32,
    /// When the fade-out starts (moved earlier by confirm).
    out_at: f32,
}

impl TitleCard {
    fn new(text: &str) -> TitleCard {
        TitleCard {
            lines: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect(),
            age: 0.0,
            out_at: TITLE_IN + TITLE_HOLD,
        }
    }

    fn alpha(&self) -> f32 {
        let a_in = (self.age / TITLE_IN).min(1.0);
        let a_out = 1.0 - ((self.age - self.out_at) / TITLE_OUT).clamp(0.0, 1.0);
        a_in.min(a_out).clamp(0.0, 1.0)
    }

    /// Start fading out (once fully visible).
    fn dismiss(&mut self) {
        self.out_at = self.out_at.min(self.age.max(TITLE_IN));
    }

    fn done(&self) -> bool {
        self.age >= self.out_at + TITLE_OUT
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let a = self.alpha();
        if a <= 0.0 || self.lines.is_empty() {
            return;
        }
        let (w, h) = (gfx.size().x, gfx.size().y);
        fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.02, 0.5 * a));
        let head = TextStyle::main(theme::TEXT_ACCENT.with_alpha(a))
            .size(3)
            .shadow(Color::new(0.1, 0.03, 0.0, 0.9 * a));
        let head_h = gfx.line_height(FontId::Main, 3);
        // Subtitles at double size when they fit, else at normal size.
        let sub_size = if self.lines[1..]
            .iter()
            .all(|l| gfx.text_width(l, FontId::Main, 2) <= w - 40.0)
        {
            2
        } else {
            1
        };
        let sub_h = gfx.line_height(FontId::Main, sub_size);
        let total = head_h + (self.lines.len() - 1) as f32 * (sub_h + 4.0);
        let mut y = ((h - total) / 2.0).round() - 6.0;

        let head_w = gfx.text_width(&self.lines[0], FontId::Main, 3);
        gfx.text_aligned(&self.lines[0], 0.0, y, w, Align::Center, head);
        // Gold rules on both sides of the heading, growing while it fades in.
        let reach = 90.0 * (self.age / TITLE_IN).min(1.0);
        let mid = (y + head_h / 2.0).round();
        let gap = head_w / 2.0 + 14.0;
        let gold = theme::TEXT_ACCENT;
        let cx = w / 2.0;
        fill_gradient_h(
            Rect::new(cx - gap - reach, mid, reach, 1.0),
            gold.with_alpha(0.0),
            gold.with_alpha(0.9 * a),
        );
        fill_gradient_h(
            Rect::new(cx + gap, mid, reach, 1.0),
            gold.with_alpha(0.9 * a),
            gold.with_alpha(0.0),
        );
        y += head_h + 4.0;
        let sub = TextStyle::main(theme::TEXT.with_alpha(a))
            .size(sub_size)
            .shadow(theme::TEXT_SHADOW.with_alpha(a));
        for line in &self.lines[1..] {
            gfx.text_aligned(line, 0.0, y, w, Align::Center, sub);
            y += sub_h + 4.0;
        }
    }
}

/// Picture next to a banner caption.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NoticeArt {
    Portrait(String),
    Icon(String),
}

/// Banner for `Joined` / `Received`.
#[derive(Debug, Clone)]
struct Notice {
    title: String,
    subtitle: Option<String>,
    art: NoticeArt,
    age: f32,
}

impl Notice {
    fn alpha(&self) -> f32 {
        let a_in = (self.age / NOTICE_IN).min(1.0);
        let a_out = ((NOTICE_SECONDS - self.age) / NOTICE_OUT).clamp(0.0, 1.0);
        a_in.min(a_out)
    }

    fn dismiss(&mut self) {
        self.age = self.age.max(NOTICE_SECONDS - NOTICE_OUT);
    }

    fn done(&self) -> bool {
        self.age >= NOTICE_SECONDS
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let a = self.alpha();
        if a <= 0.0 {
            return;
        }
        let canvas = gfx.size();
        let w = canvas.x;
        let h = 64.0;
        let y = ((canvas.y - h) / 2.0).round() - 20.0;
        let dark = Color::new(0.02, 0.03, 0.1, 0.88 * a);
        let mid = Color::new(0.1, 0.13, 0.36, 0.9 * a);
        fill_gradient_h(Rect::new(0.0, y, w / 2.0, h), dark, mid);
        fill_gradient_h(Rect::new(w / 2.0, y, w / 2.0, h), mid, dark);
        let gold = theme::TEXT_ACCENT.with_alpha(a);
        fill_rect(Rect::new(0.0, y, w, 1.0), gold);
        fill_rect(Rect::new(0.0, y + h - 1.0, w, 1.0), gold);

        let title = TextStyle::main(theme::TEXT.with_alpha(a))
            .size(2)
            .shadow(theme::TEXT_SHADOW.with_alpha(a));
        let title_w = gfx.text_width(&self.title, FontId::Main, 2);
        let art_w = match self.art {
            NoticeArt::Portrait(_) => 44.0,
            NoticeArt::Icon(_) => 22.0,
        };
        let sub_w = self
            .subtitle
            .as_deref()
            .map_or(0.0, |s| gfx.text_width(s, FontId::Main, 1));
        let block_w = art_w + 10.0 + title_w.max(sub_w);
        let slide = (1.0 - (self.age / NOTICE_IN).min(1.0)).powi(3) * 30.0;
        let x0 = ((w - block_w) / 2.0).round() + slide.round();
        match &self.art {
            NoticeArt::Portrait(key) => {
                draw_portrait_card(ctx, Some(key), Rect::new(x0, y + 4.0, 44.0, 55.0), a, 1.0);
            }
            NoticeArt::Icon(key) => {
                // Icons have no opacity; show them once the banner is mostly visible.
                if a > 0.5 {
                    draw_icon(ctx, key, vec2(x0 + 3.0, y + (h - 16.0) / 2.0));
                }
            }
        }
        let tx = x0 + art_w + 10.0;
        let ty = if self.subtitle.is_some() {
            y + 10.0
        } else {
            y + (h - 32.0) / 2.0
        };
        gfx.text(&self.title, tx, ty, title);
        if let Some(sub) = &self.subtitle {
            gfx.text(
                sub,
                tx,
                y + 42.0,
                TextStyle::main(theme::TEXT_ACCENT.with_alpha(a))
                    .shadow(theme::TEXT_SHADOW.with_alpha(a)),
            );
        }
    }
}

/// Banner for an officer joining: name, then hanja, courtesy name, class and level.
fn joined_notice(pack: &Pack, officer: &str, name: &str) -> Notice {
    let def = pack.officer(officer);
    let subtitle = def.map(|d| {
        let class = pack
            .class(&d.class)
            .map_or(d.class.as_str(), |c| c.name.as_str());
        let mut parts = Vec::new();
        if !d.hanja.is_empty() {
            parts.push(d.hanja.clone());
        }
        if !d.courtesy.is_empty() {
            parts.push(format!("자 {}", d.courtesy));
        }
        parts.push(format!("{class} Lv{}", d.level));
        parts.join(" · ")
    });
    Notice {
        title: format!("{name} 합류!"),
        subtitle,
        art: NoticeArt::Portrait(def.map_or(officer, |d| d.portrait_key()).to_string()),
        age: 0.0,
    }
}

/// Banner for gold or an item received; `None` when nothing changed (e.g. gold at its cap).
fn received_notice(pack: &Pack, gold: i64, item: Option<&str>) -> Option<Notice> {
    let (title, subtitle, icon) = match item {
        Some(id) => {
            let def = pack.item(id);
            let name = def.map_or(id, |d| d.name.as_str());
            let icon = def
                .map(|d| d.icon.as_str())
                .filter(|i| !i.is_empty())
                .unwrap_or("consumable");
            let sub = def.map(|d| d.hanja.clone()).filter(|h| !h.is_empty());
            (format!("{name} 획득"), sub, icon.to_string())
        }
        None if gold > 0 => (
            format!("금 {} 획득", format::thousands(gold)),
            None,
            "gold".to_string(),
        ),
        None if gold < 0 => (
            format!("금 {} 지출", format::thousands(-gold)),
            None,
            "gold".to_string(),
        ),
        None => return None,
    };
    Some(Notice {
        title,
        subtitle,
        art: NoticeArt::Icon(icon),
        age: 0.0,
    })
}

// ----- input helpers --------------------------------------------------------------------------

/// Tells a short press of a key (a tap) from holding it.
#[derive(Debug, Clone, Copy, Default)]
struct HoldTap {
    /// A press was seen on this screen (a key still held from the previous screen is ignored).
    armed: bool,
    held: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldEvent {
    None,
    /// Released before [`HOLD_SECONDS`].
    Tap,
}

impl HoldTap {
    /// `pressed`: the key went down this frame; `down`: it is held.
    fn update(&mut self, pressed: bool, down: bool, dt: f32) -> HoldEvent {
        if pressed {
            self.armed = true;
            self.held = 0.0;
        }
        if !self.armed {
            return HoldEvent::None;
        }
        if down {
            self.held += dt;
            return HoldEvent::None;
        }
        let tap = self.held < HOLD_SECONDS;
        *self = HoldTap::default();
        if tap {
            HoldEvent::Tap
        } else {
            HoldEvent::None
        }
    }

    fn holding(&self) -> bool {
        self.armed && self.held >= HOLD_SECONDS
    }

    fn reset(&mut self) {
        *self = HoldTap::default();
    }
}

fn cancel_key_down(ctx: &Ctx) -> bool {
    [KeyCode::X, KeyCode::Escape, KeyCode::Backspace]
        .into_iter()
        .any(|k| ctx.input.key_down(k))
}

fn fast_key_down(ctx: &Ctx) -> bool {
    [KeyCode::LeftControl, KeyCode::RightControl, KeyCode::Tab]
        .into_iter()
        .any(|k| ctx.input.key_down(k))
}

/// Buttons at the top right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Backlog,
    Fast,
    Skip,
    Settings,
}

impl Tool {
    const ALL: [Tool; 4] = [Tool::Backlog, Tool::Fast, Tool::Skip, Tool::Settings];

    fn label(self) -> &'static str {
        match self {
            Tool::Backlog => "최근 대사",
            Tool::Fast => "빨리 넘기기",
            Tool::Skip => "건너뛰기",
            Tool::Settings => "설정",
        }
    }
}

const TOOL_H: f32 = 14.0;
/// Top of the buttons of a full-screen scene.
const TOOL_TOP: f32 = 4.0;
/// Top of the buttons of an overlay. The screen below an overlay keeps its top bar visible (the
/// battle HUD bar with the turn, phase, weather and gold is 16 px high), so the buttons sit
/// below it instead of covering it. Screens that push an overlay keep their top bar at most
/// `OVERLAY_TOOL_TOP - 4` pixels high.
pub const OVERLAY_TOOL_TOP: f32 = 20.0;

/// Button rectangles laid out right to left from the top right corner of a `canvas_w` wide
/// canvas, with their top at `top`; `width` measures a label.
fn tool_layout(canvas_w: f32, top: f32, width: impl Fn(&str) -> f32) -> Vec<(Tool, Rect)> {
    let mut x = canvas_w - 4.0;
    let mut out = Vec::new();
    for tool in Tool::ALL.into_iter().rev() {
        let w = (width(tool.label()) + 10.0).round();
        x -= w;
        out.push((tool, Rect::new(x.round(), top, w, TOOL_H)));
        x -= 3.0;
    }
    out.reverse();
    out
}

fn tool_rects(gfx: &Gfx, top: f32) -> Vec<(Tool, Rect)> {
    tool_layout(gfx.size().x, top, |s| gfx.text_width(s, FontId::Small, 1))
}

// ----- the screen -----------------------------------------------------------------------------

/// Items of the scene menu (short cancel press / right click).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Continue,
    Backlog,
    Fast,
    Skip,
    Settings,
}

const MENU: [MenuItem; 5] = [
    MenuItem::Continue,
    MenuItem::Backlog,
    MenuItem::Fast,
    MenuItem::Skip,
    MenuItem::Settings,
];

enum Popup {
    None,
    Menu(ChoiceBox),
    ConfirmSkip(ConfirmDialog),
    Backlog(BacklogView),
}

/// What the scene is showing right now.
enum Current {
    /// Ready to run the next step.
    Next,
    Text {
        dialogue: DialogueBox,
        spotlight: Spotlight,
    },
    Title(TitleCard),
    Choice {
        choice: ChoiceBox,
        options: Vec<String>,
    },
    /// Pause; seconds left.
    Wait(f32),
    /// Waiting for `@fade out` / `@fade in` to finish.
    Fade,
    /// Waiting for a duel move (`@duel_act`) to finish.
    Duel,
    Notice(Notice),
    /// The scene ended (its transition was returned).
    Done,
}

/// Plays one drama scene. Side effects (`@set`, `@join`, `@gold`, `@item` ...) are applied to the
/// session's `CampaignState` as the runner reaches them.
pub struct DramaScreen {
    scene: String,
    end: DramaEnd,
    runner: Option<DramaRunner>,
    /// Why the scene cannot be played (reported on the first update).
    start_error: Option<String>,
    stage: Stage,
    /// The duel scene between `@duel` and `@duel_end`.
    duel: Option<DuelView>,
    current: Current,
    /// The last message, kept on screen under a following choice.
    last_text: Option<(DialogueBox, Spotlight)>,
    backlog: Backlog,
    popup: Popup,
    cancel: HoldTap,
    /// 빨리 넘기기 switched on from the menu or the button (until a choice, a tap or the end).
    fast_toggle: bool,
    /// 장면 건너뛰기 in progress.
    skipping: bool,
    /// Something has been shown; before that background changes are instant.
    shown_anything: bool,
    /// Whether the last update fast-forwarded (for drawing).
    fast_now: bool,
}

impl DramaScreen {
    /// Campaign `Drama` node.
    pub fn node(ctx: &mut Ctx, scene: &str) -> DramaScreen {
        DramaScreen::new(ctx, scene, DramaEnd::Advance)
    }

    /// Campaign `Ending` node that has a scene.
    pub fn ending(ctx: &mut Ctx, scene: &str, title: String) -> DramaScreen {
        DramaScreen::new(ctx, scene, DramaEnd::Ending { title })
    }

    /// Overlay drawn above the screen that pushed it (`is_overlay() == true`); pops itself when
    /// the scene ends. Used by the battle screen for intro/outro scenes and battle events.
    pub fn overlay(ctx: &mut Ctx, scene: &str) -> DramaScreen {
        DramaScreen::new(ctx, scene, DramaEnd::Pop)
    }

    fn new(ctx: &mut Ctx, scene: &str, end: DramaEnd) -> DramaScreen {
        let (runner, start_error) = match ctx.pack.clone() {
            Some(pack) => match DramaRunner::new(&pack, scene) {
                Ok(r) => {
                    preload(ctx, &pack, scene);
                    (Some(r), None)
                }
                Err(e) => (None, Some(e.to_string())),
            },
            None => (None, Some("데이터 팩이 로드되지 않았습니다".to_string())),
        };
        let backdrop = if end == DramaEnd::Pop {
            Backdrop::Transparent
        } else {
            Backdrop::Black
        };
        DramaScreen {
            scene: scene.to_string(),
            end,
            runner,
            start_error,
            stage: Stage::new(backdrop),
            duel: None,
            current: Current::Next,
            last_text: None,
            backlog: Backlog::default(),
            popup: Popup::None,
            cancel: HoldTap::default(),
            fast_toggle: false,
            skipping: false,
            shown_anything: false,
            fast_now: false,
        }
    }

    fn finish(&mut self) -> Transition {
        self.current = Current::Done;
        self.popup = Popup::None;
        match &self.end {
            DramaEnd::Advance => Transition::Flow(Flow::Advance),
            DramaEnd::Ending { title } => Transition::Flow(Flow::Ending {
                title: title.clone(),
            }),
            DramaEnd::Pop => Transition::Pop,
        }
    }

    /// Report a scene that cannot continue and move on, so the game never gets stuck.
    fn fail(&mut self, ctx: &mut Ctx, why: &str) -> Transition {
        macroquad::logging::error!("drama `{}`: {}", self.scene, why);
        ctx.sfx(sfx::ERROR);
        ctx.toast(format!(
            "장면 `{}`을(를) 재생할 수 없습니다: {why}",
            self.scene
        ));
        self.finish()
    }

    /// Top of the toolbar buttons (lower in an overlay, see [`OVERLAY_TOOL_TOP`]).
    fn tool_top(&self) -> f32 {
        if self.end == DramaEnd::Pop {
            OVERLAY_TOOL_TOP
        } else {
            TOOL_TOP
        }
    }

    fn spotlight(&self) -> Spotlight {
        match &self.current {
            Current::Text { spotlight, .. } => spotlight.clone(),
            Current::Choice { .. } => self
                .last_text
                .as_ref()
                .map_or(Spotlight::All, |(_, s)| s.clone()),
            _ => Spotlight::All,
        }
    }

    fn spotlight_for(&self, portrait: Option<&str>) -> Spotlight {
        match portrait {
            Some(key) if self.stage.slots.iter().flatten().any(|p| p.key == key) => {
                Spotlight::Speaker(key.to_string())
            }
            _ => Spotlight::Nobody,
        }
    }

    fn start_skip(&mut self) {
        self.skipping = true;
        self.fast_toggle = false;
        // Whatever is on screen now is done; a pending choice stays.
        if !matches!(self.current, Current::Choice { .. } | Current::Done) {
            self.current = Current::Next;
            self.last_text = None;
        }
        self.stage.fade = self.stage.fade_target;
        self.stage.mix = 1.0;
        self.stage.leaving.clear();
        for p in self.stage.slots.iter_mut().flatten() {
            p.alpha = 1.0;
        }
        if let Some(duel) = self.duel.as_mut() {
            duel.update(0.0, true);
        }
    }

    // ----- popups and controls -----

    fn open_menu(&mut self, ctx: &Ctx) {
        let labels: Vec<&str> = MENU
            .iter()
            .map(|m| match m {
                MenuItem::Continue => "계속",
                MenuItem::Backlog => "최근 대사",
                MenuItem::Fast if self.fast_toggle => "빨리 넘기기 끄기",
                MenuItem::Fast => "빨리 넘기기",
                MenuItem::Skip => "장면 건너뛰기",
                MenuItem::Settings => "설정",
            })
            .collect();
        self.popup = Popup::Menu(ChoiceBox::new(&ctx.gfx, None, &labels, Some(0)));
        self.cancel.reset();
    }

    fn open_skip_confirm(&mut self, ctx: &Ctx) {
        self.popup = Popup::ConfirmSkip(
            ConfirmDialog::new(&ctx.gfx, "이 장면을 건너뛸까요?")
                .labels("건너뛰기", "계속 보기")
                .default_no(),
        );
        self.cancel.reset();
    }

    fn open_backlog(&mut self, ctx: &Ctx) {
        self.popup = Popup::Backlog(BacklogView::open(&ctx.gfx, &self.backlog));
        self.cancel.reset();
    }

    fn use_tool(&mut self, ctx: &mut Ctx, tool: Tool) -> Transition {
        match tool {
            Tool::Backlog => {
                ctx.sfx(sfx::CONFIRM);
                self.open_backlog(ctx);
            }
            Tool::Fast => {
                self.fast_toggle = !self.fast_toggle;
                ctx.sfx(sfx::CURSOR);
            }
            Tool::Skip => {
                ctx.sfx(sfx::CONFIRM);
                self.open_skip_confirm(ctx);
            }
            Tool::Settings => {
                ctx.sfx(sfx::CONFIRM);
                return Transition::push(SettingsScreen::new());
            }
        }
        Transition::None
    }

    /// Popups own the input while open. `Some` when a popup handled this frame.
    fn update_popup(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::None => None,
            Popup::Menu(mut choice) => {
                match choice.update(ctx) {
                    ChoiceEvent::Chosen(i) => match MENU[i] {
                        MenuItem::Continue => {}
                        MenuItem::Backlog => self.open_backlog(ctx),
                        MenuItem::Fast => self.fast_toggle = !self.fast_toggle,
                        MenuItem::Skip => self.open_skip_confirm(ctx),
                        MenuItem::Settings => return Some(Transition::push(SettingsScreen::new())),
                    },
                    ChoiceEvent::Cancelled => {}
                    ChoiceEvent::None => self.popup = Popup::Menu(choice),
                }
                Some(Transition::None)
            }
            Popup::ConfirmSkip(mut dialog) => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => self.start_skip(),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => self.popup = Popup::ConfirmSkip(dialog),
                }
                Some(Transition::None)
            }
            Popup::Backlog(mut view) => {
                if !view.update(ctx) {
                    self.popup = Popup::Backlog(view);
                }
                Some(Transition::None)
            }
        }
    }

    /// Toolbar, menu and backlog keys. `Some` when the input was used.
    fn controls(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        if let Some(p) = ctx.input.tap() {
            let hit = tool_rects(&ctx.gfx, self.tool_top())
                .into_iter()
                .find(|(_, r)| r.contains(p));
            if let Some((tool, _)) = hit {
                ctx.input.consume();
                return Some(self.use_tool(ctx, tool));
            }
        }
        if ctx.input.right_click() {
            ctx.sfx(sfx::CONFIRM);
            self.open_menu(ctx);
            return Some(Transition::None);
        }
        let pressed = ctx.input.cancel();
        let down = cancel_key_down(ctx);
        if self.cancel.update(pressed, down, ctx.dt) == HoldEvent::Tap {
            ctx.sfx(sfx::CONFIRM);
            self.open_menu(ctx);
            return Some(Transition::None);
        }
        let choosing = matches!(self.current, Current::Choice { .. });
        if !choosing
            && (ctx.input.key_pressed(KeyCode::L)
                || ctx.input.key_pressed(KeyCode::PageUp)
                || ctx.input.wheel() < 0)
        {
            ctx.sfx(sfx::CONFIRM);
            self.open_backlog(ctx);
            return Some(Transition::None);
        }
        // A tap or confirm while fast-forwarding from the menu stops it.
        if self.fast_toggle && ctx.input.confirm() {
            ctx.input.consume();
            self.fast_toggle = false;
            ctx.sfx(sfx::CANCEL);
            return Some(Transition::None);
        }
        None
    }

    fn fast(&self, ctx: &Ctx) -> bool {
        self.fast_toggle || self.cancel.holding() || fast_key_down(ctx)
    }

    // ----- steps -----

    fn next_step(&mut self, ctx: &mut Ctx, pack: &Pack) -> Result<Step, String> {
        let runner = self
            .runner
            .as_mut()
            .ok_or_else(|| "장면이 준비되지 않았습니다".to_string())?;
        let campaign = &mut ctx
            .session
            .as_mut()
            .ok_or_else(|| "진행 중인 캠페인이 없습니다".to_string())?
            .campaign;
        runner.next(pack, campaign).map_err(|e| e.to_string())
    }

    /// Run steps until one needs time on screen (or the scene ends).
    fn run_steps(&mut self, ctx: &mut Ctx, pack: &Pack) -> Option<Transition> {
        for _ in 0..MAX_STEPS_PER_FRAME {
            if !matches!(self.current, Current::Next) {
                return None;
            }
            let step = match self.next_step(ctx, pack) {
                Ok(step) => step,
                Err(why) => return Some(self.fail(ctx, &why)),
            };
            if let Some(t) = self.present(ctx, pack, step) {
                return Some(t);
            }
        }
        None
    }

    /// Show a message. While skipping it is only remembered (complete, at its last page), so a
    /// choice that follows still shows the question it answers.
    fn show_text(&mut self, mut dialogue: DialogueBox, spotlight: Spotlight) {
        if self.skipping {
            dialogue.show_last_page();
            self.last_text = Some((dialogue, spotlight));
            return;
        }
        if self.fast_now {
            dialogue.complete_page();
        }
        self.current = Current::Text {
            dialogue,
            spotlight,
        };
    }

    /// Start presenting one step. Instant steps leave `current` at `Next`.
    fn present(&mut self, ctx: &mut Ctx, pack: &Pack, step: Step) -> Option<Transition> {
        let skipping = self.skipping;
        if !matches!(step, Step::Choice(_)) && step_blocks(&step) {
            self.last_text = None;
        }
        match step {
            Step::Background(key) => {
                let instant = skipping || !self.shown_anything;
                self.stage.set_backdrop(Backdrop::from_step(key), instant);
            }
            Step::Music(Some(key)) => ctx.audio.play_bgm(&key),
            Step::Music(None) => ctx.audio.stop_bgm(),
            Step::Sound(key) => {
                if !skipping {
                    ctx.sfx(&key);
                }
            }
            Step::Show { portrait, slot } => self.stage.show(slot, portrait, skipping),
            Step::Hide(slot) => self.stage.hide(slot, skipping),
            Step::Wait { ms } => {
                if !skipping {
                    self.current = Current::Wait(ms as f32 / 1000.0);
                }
            }
            Step::FadeOut | Step::FadeIn => {
                let target = if step == Step::FadeOut { 1.0 } else { 0.0 };
                self.stage.set_fade(target, skipping);
                if !skipping {
                    self.current = Current::Fade;
                }
            }
            Step::Title(text) => {
                self.backlog
                    .push(None, &format!("【{}】", text.replace('\n', " ")));
                if !skipping {
                    self.current = Current::Title(TitleCard::new(&text));
                }
            }
            Step::Narration(text) => {
                self.backlog.push(None, &text);
                let dialogue = DialogueBox::narration(&ctx.gfx, &text);
                self.show_text(dialogue, Spotlight::Nobody);
            }
            Step::Line {
                speaker,
                portrait,
                text,
            } => {
                self.backlog.push(Some(&speaker), &text);
                let spotlight = self.spotlight_for(portrait.as_deref());
                let dialogue = DialogueBox::speech(&ctx.gfx, &speaker, portrait.as_deref(), &text);
                self.show_text(dialogue, spotlight);
            }
            Step::Choice(options) => {
                self.fast_toggle = false;
                let labels: Vec<&str> = options.iter().map(String::as_str).collect();
                let bottom = self
                    .last_text
                    .as_ref()
                    .map_or(ctx.gfx.size().y - 70.0, |(d, _)| d.top() - 6.0);
                let choice = ChoiceBox::new(&ctx.gfx, None, &labels, None).with_bottom(bottom);
                self.current = Current::Choice { choice, options };
            }
            Step::Joined { officer, name } => {
                let notice = joined_notice(pack, &officer, &name);
                self.backlog.push(None, &format!("【{}】", notice.title));
                if skipping {
                    ctx.toast(notice.title);
                } else {
                    ctx.sfx(sfx::LEVELUP);
                    self.current = Current::Notice(notice);
                }
            }
            Step::Received { gold, item } => {
                if let Some(notice) = received_notice(pack, gold, item.as_deref()) {
                    self.backlog.push(None, &format!("【{}】", notice.title));
                    if skipping {
                        ctx.toast(notice.title);
                    } else {
                        ctx.sfx(sfx::TREASURE);
                        self.current = Current::Notice(notice);
                    }
                }
            }
            Step::Duel { left, right, bg } => {
                self.duel = Some(DuelView::new(&left, &right, bg));
            }
            Step::DuelAct { side, act } => {
                let Some(duel) = self.duel.as_mut() else {
                    return Some(self.fail(ctx, "@duel_act without an open @duel"));
                };
                let sound = duel.act(side, act);
                if skipping {
                    duel.update(0.0, true);
                } else {
                    if let Some(key) = sound {
                        ctx.sfx(key);
                    }
                    self.current = Current::Duel;
                }
            }
            Step::DuelEnd => self.duel = None,
            Step::End => return Some(self.finish()),
        }
        if !matches!(self.current, Current::Next) {
            self.shown_anything = true;
        }
        None
    }

    /// Advance what is on screen; returns a transition when the scene cannot go on.
    fn update_current(&mut self, ctx: &mut Ctx, pack: &Pack, fast: bool) -> Option<Transition> {
        let speed = if fast { FAST_FACTOR } else { 1.0 };
        let dt = ctx.dt;
        match std::mem::replace(&mut self.current, Current::Next) {
            Current::Next => {}
            Current::Done => self.current = Current::Done,
            Current::Text {
                mut dialogue,
                spotlight,
            } => match dialogue.update(ctx, fast) {
                DialogueEvent::Finished => self.last_text = Some((dialogue, spotlight)),
                _ => {
                    self.current = Current::Text {
                        dialogue,
                        spotlight,
                    }
                }
            },
            Current::Title(mut card) => {
                card.age += dt * speed;
                if ctx.input.confirm() {
                    ctx.input.consume();
                    card.dismiss();
                }
                if !card.done() {
                    self.current = Current::Title(card);
                }
            }
            Current::Choice {
                mut choice,
                options,
            } => match choice.update(ctx) {
                ChoiceEvent::Chosen(i) => {
                    let chosen = match self.runner.as_mut() {
                        Some(r) => r.choose(pack, i).map_err(|e| e.to_string()),
                        None => Err("장면이 준비되지 않았습니다".to_string()),
                    };
                    if let Err(why) = chosen {
                        return Some(self.fail(ctx, &why));
                    }
                    if let Some(text) = options.get(i) {
                        self.backlog.push(None, &format!("▶ {text}"));
                    }
                    self.last_text = None;
                }
                _ => self.current = Current::Choice { choice, options },
            },
            Current::Wait(left) => {
                let left = left - dt * speed;
                if left > 0.0 {
                    self.current = Current::Wait(left);
                }
            }
            Current::Fade => {
                if !self.stage.fade_done() {
                    self.current = Current::Fade;
                }
            }
            Current::Duel => {
                if let Some(duel) = self.duel.as_mut() {
                    duel.update(dt * speed, false);
                    if duel.busy() {
                        self.current = Current::Duel;
                    }
                }
            }
            Current::Notice(mut notice) => {
                notice.age += dt * speed;
                if ctx.input.confirm() {
                    ctx.input.consume();
                    notice.dismiss();
                }
                if !notice.done() {
                    self.current = Current::Notice(notice);
                }
            }
        }
        None
    }

    /// Where the duel scene is drawn: over a battle whose pack has a battle frame, the frame's
    /// map area (as in the original); otherwise between the buttons and the message box.
    fn duel_area(&self, ctx: &Ctx) -> Rect {
        let frame = ctx
            .pack
            .as_ref()
            .and_then(|p| p.manifest.presentation.battle_frame.as_ref())
            .filter(|_| self.end == DramaEnd::Pop);
        if let Some(f) = frame {
            let [x, y, w, h] = f.map;
            return Rect::new(x as f32, y as f32, w as f32, h as f32);
        }
        let canvas = ctx.gfx.size();
        let top = self.tool_top() + TOOL_H + 4.0;
        let bottom = DialogueBox::box_rect(canvas, true).y - 4.0;
        Rect::new(0.0, top, canvas.x, (bottom - top).max(0.0))
    }

    fn draw_toolbar(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let pointer = ctx.input.pointer();
        // Title cards and blacked-out moments stay uncluttered: the buttons recede.
        let quiet = matches!(self.current, Current::Title(_)) || self.stage.fade > 0.5;
        for (tool, r) in tool_rects(gfx, self.tool_top()) {
            let hover = pointer.is_some_and(|p| r.contains(p));
            let on = tool == Tool::Fast && self.fast_now;
            let alpha = match (hover || on, quiet) {
                (true, _) => 0.95,
                (false, false) => 0.6,
                (false, true) => 0.25,
            };
            draw_window_ex(r, WindowStyle::Panel, alpha);
            if on || hover {
                draw_highlight(inset(r, 2.0), on, ctx.time);
            }
            let color = if on {
                theme::TEXT_ACCENT
            } else if hover {
                theme::TEXT
            } else if quiet {
                theme::TEXT_DIM.with_alpha(0.35)
            } else {
                theme::TEXT_DIM
            };
            gfx.text_aligned(
                tool.label(),
                r.x,
                r.y + 1.0,
                r.w,
                Align::Center,
                TextStyle::small(color),
            );
        }
    }
}

/// Steps that take time on screen (and replace the message kept under a choice).
fn step_blocks(step: &Step) -> bool {
    matches!(
        step,
        Step::Wait { .. }
            | Step::FadeOut
            | Step::FadeIn
            | Step::DuelAct { .. }
            | Step::Title(_)
            | Step::Narration(_)
            | Step::Line { .. }
            | Step::Choice(_)
            | Step::Joined { .. }
            | Step::Received { .. }
    )
}

/// Start loading the backgrounds, portraits and sounds a scene uses.
fn preload(ctx: &Ctx, pack: &Pack, scene: &str) {
    let Some(scene) = pack.scene(scene) else {
        return;
    };
    let mut textures: Vec<String> = vec![UNKNOWN_PORTRAIT.to_string()];
    let mut sounds: Vec<String> = Vec::new();
    for cmd in &scene.cmds {
        let portrait = match cmd {
            Cmd::Bg(Some(key)) => {
                textures.push(format!("bg/{key}"));
                None
            }
            Cmd::Show { who, .. } => Some(
                pack.speaker_officer(who)
                    .map_or(who.as_str(), |o| o.portrait_key()),
            ),
            Cmd::Say { speaker, .. } => pack.speaker_officer(speaker).map(|o| o.portrait_key()),
            Cmd::Join(officer) => pack.officer(officer).map(|o| o.portrait_key()),
            Cmd::Sfx(key) => {
                sounds.push(format!("sfx/{key}"));
                None
            }
            Cmd::Duel { left, right, bg } => {
                textures.extend(DuelView::new(left, right, bg.clone()).textures());
                None
            }
            _ => None,
        };
        if let Some(key) = portrait {
            textures.push(format!("portraits/{key}"));
        }
    }
    textures.sort();
    textures.dedup();
    sounds.sort();
    sounds.dedup();
    ctx.media.preload_textures(&textures);
    ctx.media.preload_sounds(&sounds);
}

impl Screen for DramaScreen {
    fn name(&self) -> &'static str {
        "drama"
    }

    fn on_enter(&mut self, _ctx: &mut Ctx, how: Enter) {
        if how == Enter::Resumed {
            // Back from the settings overlay: keys held there do not count here.
            self.cancel.reset();
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if matches!(self.current, Current::Done) {
            return Transition::None;
        }
        if let Some(why) = self.start_error.take() {
            return self.fail(ctx, &why);
        }
        let Some(pack) = ctx.pack.clone() else {
            return self.fail(ctx, "데이터 팩이 로드되지 않았습니다");
        };
        if let Some(t) = self.update_popup(ctx) {
            return t;
        }
        if let Some(t) = self.controls(ctx) {
            return t;
        }

        let fast = self.fast(ctx) && !self.skipping;
        self.fast_now = fast;
        let backdrop_ready = match &self.stage.backdrop {
            Backdrop::Image(key) => background_state(ctx, key) != AssetState::Loading,
            _ => true,
        };
        let speed = if fast { FAST_FACTOR } else { 1.0 };
        let spotlight = self.spotlight();
        self.stage.update(ctx.dt, speed, backdrop_ready, &spotlight);

        if let Some(t) = self.update_current(ctx, &pack, fast) {
            return t;
        }
        self.run_steps(ctx, &pack).unwrap_or(Transition::None)
    }

    fn draw(&self, ctx: &Ctx) {
        self.stage.draw(ctx);
        if let Some(duel) = &self.duel {
            duel.draw(ctx, self.duel_area(ctx));
        }
        match &self.current {
            Current::Title(card) => card.draw(ctx),
            Current::Text { dialogue, .. } => dialogue.draw(ctx, !self.fast_now),
            Current::Choice { choice, .. } => {
                if let Some((dialogue, _)) = &self.last_text {
                    dialogue.draw(ctx, false);
                }
                choice.draw(ctx);
            }
            Current::Notice(notice) => notice.draw(ctx),
            Current::Next | Current::Wait(_) | Current::Fade | Current::Duel | Current::Done => {}
        }
        if matches!(self.current, Current::Done) {
            return;
        }
        self.draw_toolbar(ctx);
        match &self.popup {
            Popup::None => {}
            Popup::Menu(choice) => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.4));
                choice.draw(ctx);
            }
            Popup::ConfirmSkip(dialog) => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.4));
                dialog.draw(ctx);
            }
            Popup::Backlog(view) => view.draw(ctx),
        }
    }

    fn is_overlay(&self) -> bool {
        self.end == DramaEnd::Pop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speakers_are_lit_and_others_dimmed() {
        let s = Spotlight::Speaker("guan_yu".into());
        assert_eq!(portrait_light("guan_yu", &s), 1.0);
        assert_eq!(portrait_light("liu_bei", &s), DIMMED);
        assert_eq!(portrait_light("liu_bei", &Spotlight::All), 1.0);
        assert_eq!(portrait_light("liu_bei", &Spotlight::Nobody), DIMMED);
    }

    /// Canvas sizes the layout tests run on: the default (also the smallest allowed), VGA and
    /// the largest allowed.
    const CANVASES: [Vec2; 3] = [
        crate::gfx::DEFAULT_CANVAS,
        Vec2::new(640.0, 480.0),
        Vec2::new(1280.0, 800.0),
    ];

    #[test]
    fn slots_sit_side_by_side_above_the_message_box() {
        // The base pack's layout is unchanged.
        let default = crate::gfx::DEFAULT_CANVAS;
        assert_eq!(slot_rect(default, 0), Rect::new(64.0, 56.0, 104.0, 130.0));
        for canvas in CANVASES {
            let (l, c, r) = (
                slot_rect(canvas, 0),
                slot_rect(canvas, 1),
                slot_rect(canvas, 2),
            );
            assert!(l.right() < c.x && c.right() < r.x, "{canvas}");
            assert_eq!(c.center().x, canvas.x / 2.0);
            assert!(r.right() <= canvas.x && l.x >= 0.0);
            assert!(l.bottom() <= DialogueBox::box_rect(canvas, true).y);
            // Portraits keep the 4:5 aspect of the portrait art.
            assert_eq!(l.w * 5.0, l.h * 4.0);
        }
    }

    #[test]
    fn stage_replaces_and_fades_portraits() {
        let mut st = Stage::new(Backdrop::Black);
        st.show(Slot::Left, "liu_bei".into(), false);
        st.show(Slot::Left, "liu_bei".into(), false); // same portrait: nothing changes
        assert!(st.leaving.is_empty());
        st.update(1.0, 1.0, true, &Spotlight::All);
        assert_eq!(st.slots[0].as_ref().unwrap().alpha, 1.0);

        st.show(Slot::Left, "guan_yu".into(), false);
        assert_eq!(st.leaving.len(), 1);
        assert_eq!(st.slots[0].as_ref().unwrap().alpha, 0.0);
        st.update(
            PORTRAIT_FADE_SECONDS / 2.0,
            1.0,
            true,
            &Spotlight::Speaker("zhang_fei".into()),
        );
        let p = st.slots[0].as_ref().unwrap();
        assert!((p.alpha - 0.5).abs() < 1e-4);
        assert!(p.light < 1.0);
        st.update(1.0, 1.0, true, &Spotlight::All);
        assert!(st.leaving.is_empty());

        st.show(Slot::Right, "zhang_fei".into(), true);
        st.hide(None, false);
        assert!(st.slots.iter().all(Option::is_none));
        assert_eq!(st.leaving.len(), 2);
        st.show(Slot::Center, "cao_cao".into(), true);
        st.hide(Some(Slot::Center), true);
        assert_eq!(st.leaving.len(), 2);
    }

    #[test]
    fn backdrop_cross_fade_waits_for_the_image() {
        let mut st = Stage::new(Backdrop::Black);
        st.set_backdrop(Backdrop::Image("palace".into()), false);
        assert_eq!(st.previous, Backdrop::Black);
        st.update(1.0, 1.0, false, &Spotlight::All);
        assert_eq!(st.mix, 0.0);
        st.update(BG_FADE_SECONDS / 2.0, 1.0, true, &Spotlight::All);
        assert!((st.mix - 0.5).abs() < 1e-4);
        st.set_backdrop(Backdrop::Image("palace".into()), false);
        assert!((st.mix - 0.5).abs() < 1e-4);
        st.set_backdrop(Backdrop::Black, true);
        assert_eq!(st.mix, 1.0);
        assert_eq!(Backdrop::from_step(None), Backdrop::Black);
    }

    #[test]
    fn screen_fade_moves_towards_its_target() {
        let mut st = Stage::new(Backdrop::Transparent);
        st.set_fade(1.0, false);
        assert!(!st.fade_done());
        st.update(SCREEN_FADE_SECONDS / 2.0, 1.0, true, &Spotlight::All);
        assert!((st.fade - 0.5).abs() < 1e-4);
        st.update(
            SCREEN_FADE_SECONDS / 2.0 / FAST_FACTOR,
            FAST_FACTOR,
            true,
            &Spotlight::All,
        );
        assert!(st.fade_done());
        st.set_fade(0.0, true);
        assert_eq!(st.fade, 0.0);
    }

    #[test]
    fn title_card_timing() {
        let mut t = TitleCard::new("서장\n    세 영웅, 일어서다\n");
        assert_eq!(t.lines, vec!["서장", "세 영웅, 일어서다"]);
        assert_eq!(t.alpha(), 0.0);
        t.age = TITLE_IN;
        assert_eq!(t.alpha(), 1.0);
        t.dismiss();
        assert_eq!(t.out_at, TITLE_IN);
        t.age = TITLE_IN + TITLE_OUT / 2.0;
        assert!((t.alpha() - 0.5).abs() < 1e-4);
        assert!(!t.done());
        t.age = TITLE_IN + TITLE_OUT;
        assert!(t.done());
        // Dismissing during the fade-in waits for it to finish.
        let mut t = TitleCard::new("제1장");
        t.age = 0.1;
        t.dismiss();
        assert_eq!(t.out_at, TITLE_IN);
    }

    #[test]
    fn cancel_tap_and_hold() {
        let mut h = HoldTap::default();
        // A key still held from the previous screen is ignored until pressed here.
        assert_eq!(h.update(false, true, 1.0), HoldEvent::None);
        assert_eq!(h.update(false, false, 0.1), HoldEvent::None);
        // Short press: a tap on release.
        assert_eq!(h.update(true, true, 0.05), HoldEvent::None);
        assert_eq!(h.update(false, true, 0.1), HoldEvent::None);
        assert!(!h.holding());
        assert_eq!(h.update(false, false, 0.02), HoldEvent::Tap);
        // Long press: fast-forward while held, no tap afterwards.
        assert_eq!(h.update(true, true, 0.0), HoldEvent::None);
        assert_eq!(h.update(false, true, HOLD_SECONDS), HoldEvent::None);
        assert!(h.holding());
        assert_eq!(h.update(false, false, 0.02), HoldEvent::None);
        assert!(!h.holding());
    }

    #[test]
    fn toolbar_fits_in_the_top_right_corner() {
        for canvas in CANVASES {
            let rects = tool_layout(canvas.x, TOOL_TOP, |s| s.chars().count() as f32 * 10.0);
            assert_eq!(rects.len(), Tool::ALL.len());
            assert_eq!(rects[0].0, Tool::Backlog);
            assert!(rects.windows(2).all(|w| w[0].1.right() < w[1].1.x));
            let last = rects.last().unwrap().1;
            assert_eq!(last.right(), canvas.x - 4.0);
            assert!(rects.iter().all(|(_, r)| r.x > 0.0));
            assert!(rects.iter().all(|(_, r)| r.y == TOOL_TOP));
        }
        let rects = tool_layout(480.0, TOOL_TOP, |s| s.chars().count() as f32 * 10.0);
        assert!(rects.iter().all(|(_, r)| r.x > 480.0 / 3.0));
    }

    /// The overlay toolbar sits between the top bar of the screen below (the battle HUD; that
    /// pairing is checked in `screens::battle::hud`) and the stage portraits and message box.
    #[test]
    fn overlay_toolbar_stays_above_the_stage() {
        for canvas in CANVASES {
            let rects = tool_layout(canvas.x, OVERLAY_TOOL_TOP, |s| {
                s.chars().count() as f32 * 10.0
            });
            assert!(rects.iter().all(|(_, r)| r.y == OVERLAY_TOOL_TOP));
            assert!(rects
                .iter()
                .all(|(_, r)| r.bottom() < slot_rect(canvas, 2).y));
            assert!(rects
                .iter()
                .all(|(_, r)| r.bottom() < DialogueBox::box_rect(canvas, true).y));
        }
    }

    #[test]
    fn notices() {
        let pack = crate::screens::camp::test_pack();
        let n = joined_notice(&pack, "guan_yu", "관우");
        assert_eq!(n.title, "관우 합류!");
        assert!(n.subtitle.as_deref().unwrap().contains("關羽"));
        assert_eq!(n.art, NoticeArt::Portrait("guan_yu".into()));
        let n = received_notice(&pack, 500, None).unwrap();
        assert_eq!(n.title, "금 500 획득");
        let n = received_notice(&pack, 0, Some("bean")).unwrap();
        assert_eq!(n.title, "콩 획득");
        assert_eq!(n.art, NoticeArt::Icon("item_bean".into()));
        assert_eq!(
            received_notice(&pack, -1200, None).unwrap().title,
            "금 1,200 지출"
        );
        assert!(received_notice(&pack, 0, None).is_none());
    }
}
