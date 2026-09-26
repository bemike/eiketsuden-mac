//! Battle-specific drawing: top bar, unit and terrain panels, forecasts, range highlights, map
//! cursor, floating numbers, banners and popups. Everything draws in virtual coordinates.

use super::anim::{BannerView, FloatKind, FloatText, HudView, Popup, Tone, UnitView};
use super::text;
use super::tileset::TILE;
use crate::app::Ctx;
use crate::gfx::{
    fill_gradient_h, fill_gradient_v, fill_rect, stroke_rect, Align, FontId, Gfx, TextStyle,
    VIRTUAL_W,
};
use crate::ui::bars::{draw_gauge, GaugeKind};
use crate::ui::theme;
use crate::ui::window::{draw_icon, draw_portrait, draw_window, draw_window_ex, WindowStyle};
use hero_core::battle::{BattleState, UnitId};
use hero_core::battledef::Side;
use hero_core::data::{StatusKind, TerrainDef};
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Height of the top bar; the map viewport starts below it.
pub const TOP_BAR_H: f32 = 16.0;

pub fn side_color(side: Side) -> Color {
    match side {
        Side::Player => Color::from_hex(0x7fb2ff),
        Side::Ally => Color::from_hex(0x86e08c),
        Side::Enemy => Color::from_hex(0xff8a7a),
    }
}

/// Text with a 1-pixel dark outline (readable on any terrain).
pub fn outlined(gfx: &Gfx, s: &str, x: f32, y: f32, style: TextStyle, outline: Color) {
    let plain = TextStyle {
        shadow: None,
        ..style
    };
    let o = TextStyle {
        color: outline,
        ..plain
    };
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (1.0, 1.0)] {
        gfx.text(s, x + dx, y + dy, o);
    }
    gfx.text(s, x, y, plain);
}

pub fn draw_top_bar(ctx: &Ctx, name: &str, hud: &HudView, turn_limit: u32, gold: i64) {
    let gfx = &ctx.gfx;
    let r = Rect::new(0.0, 0.0, VIRTUAL_W, TOP_BAR_H);
    fill_gradient_v(r, theme::WIN_TOP, theme::WIN_BOTTOM);
    fill_rect(
        Rect::new(0.0, TOP_BAR_H - 1.0, VIRTUAL_W, 1.0),
        theme::BORDER_LIGHT.with_alpha(0.55),
    );
    let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
    let y = 2.0;
    gfx.text(name, 6.0, y, small.color(theme::TEXT_ACCENT));
    let mut x = 150.0;
    x += gfx.text(&format!("제 {}턴", hud.turn), x, y, small);
    gfx.text(
        &format!(" / {turn_limit}"),
        x,
        y,
        small.color(theme::TEXT_DIM),
    );
    let phase = text::phase_title(hud.phase);
    gfx.text(&phase, 232.0, y, small.color(side_color(hud.phase)));
    draw_icon(ctx, text::weather_icon(hud.weather), vec2(318.0, 0.0));
    gfx.text(text::weather_name(hud.weather), 336.0, y, small);
    draw_icon(ctx, "gold", vec2(382.0, 0.0));
    let g = crate::ui::format::thousands(gold);
    let gw = gfx.text(&g, 400.0, y, small.color(theme::TEXT_ACCENT));
    if hud.gold_found > 0 {
        gfx.text(
            &format!("+{}", hud.gold_found),
            400.0 + gw + 3.0,
            y,
            small.color(theme::TEXT_GOOD),
        );
    }
}

/// Labelled thin gauge row: `label` (dim), bar, `value` right-aligned after the bar.
fn gauge_row(gfx: &Gfx, at: Vec2, label: &str, fill: (f32, f32), kind: GaugeKind, shown: &str) {
    let (x, y) = (at.x, at.y);
    let (value, max) = fill;
    let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
    gfx.text(label, x, y - 3.0, small);
    draw_gauge(Rect::new(x + 22.0, y + 1.0, 64.0, 5.0), value, max, kind);
    gfx.text_aligned(
        shown,
        x + 88.0,
        y - 3.0,
        40.0,
        Align::Right,
        small.color(theme::TEXT),
    );
}

pub const UNIT_PANEL: Vec2 = Vec2::new(186.0, 76.0);

/// Status panel of a unit (the hovered or selected one).
pub fn draw_unit_panel(
    ctx: &Ctx,
    at: Vec2,
    pack: &Pack,
    state: &BattleState,
    id: UnitId,
    view: &UnitView,
) {
    let gfx = &ctx.gfx;
    let u = &state.units[id];
    let r = Rect::new(at.x, at.y, UNIT_PANEL.x, UNIT_PANEL.y);
    draw_window_ex(r, WindowStyle::Normal, 0.94);
    draw_portrait(
        ctx,
        u.portrait.as_deref(),
        Rect::new(r.x + 5.0, r.y + 5.0, 38.0, 47.0),
    );
    // Side colour strip under the portrait.
    fill_rect(
        Rect::new(r.x + 8.0, r.y + 50.0, 32.0, 2.0),
        side_color(u.side),
    );
    let x = r.x + 49.0;
    let main = TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW);
    let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
    let nw = gfx.text(&u.name, x, r.y + 3.0, main);
    let mut tag_x = x + nw + 4.0;
    if u.lord {
        tag_x += gfx.text("총대장", tag_x, r.y + 6.0, small.color(theme::TEXT_ACCENT)) + 3.0;
    } else if u.commander {
        tag_x += gfx.text("대장", tag_x, r.y + 6.0, small.color(theme::TEXT_ACCENT)) + 3.0;
    }
    if view.confused {
        let turns = u
            .statuses
            .iter()
            .find(|s| s.status == StatusKind::Confused)
            .map_or(0, |s| s.turns);
        gfx.text(
            &format!("혼란 {turns}"),
            tag_x,
            r.y + 6.0,
            small.color(theme::TEXT_BAD),
        );
    }
    gfx.text_aligned(
        &format!("Lv {}", view.level),
        x,
        r.y + 5.0,
        r.right() - x - 7.0,
        Align::Right,
        small.color(theme::TEXT),
    );
    let class = pack
        .class(&view.class)
        .map_or(view.class.clone(), |c| c.name.clone());
    gfx.text(
        &format!("{} · {}", class, text::side_name(u.side)),
        x,
        r.y + 18.0,
        small.color(side_color(u.side)),
    );
    gauge_row(
        gfx,
        vec2(x, r.y + 33.0),
        "병력",
        (view.hp, view.max_hp as f32),
        GaugeKind::Hp,
        &format!("{}/{}", view.hp.round() as i32, view.max_hp),
    );
    gauge_row(
        gfx,
        vec2(x, r.y + 43.0),
        "MP",
        (view.mp as f32, view.max_mp.max(1) as f32),
        GaugeKind::Mp,
        &format!("{}/{}", view.mp, view.max_mp),
    );
    gauge_row(
        gfx,
        vec2(x, r.y + 53.0),
        "사기",
        (view.morale as f32, 100.0),
        GaugeKind::Morale,
        &view.morale.to_string(),
    );
    let stats = format!(
        "공격 {}  방어 {}  이동 {}",
        state.attack_power(pack, id),
        state.defense_power(pack, id),
        state.move_points(pack, id)
    );
    gfx.text(&stats, r.x + 7.0, r.y + 61.0, small.color(theme::TEXT));
    if u.side == Side::Player {
        gfx.text_aligned(
            &format!("EXP {}", view.exp),
            r.x,
            r.y + 61.0,
            r.w - 7.0,
            Align::Right,
            small.color(theme::EXP),
        );
    }
}

pub const TERRAIN_PANEL: Vec2 = Vec2::new(96.0, 44.0);

/// Terrain panel: name, defence, healing, treasure.
pub fn draw_terrain_panel(ctx: &Ctx, at: Vec2, t: &TerrainDef, treasure: bool, passable: bool) {
    let gfx = &ctx.gfx;
    let r = Rect::new(at.x, at.y, TERRAIN_PANEL.x, TERRAIN_PANEL.y);
    draw_window_ex(r, WindowStyle::Normal, 0.94);
    let main = TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW);
    let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
    gfx.text(&t.name, r.x + 7.0, r.y + 3.0, main);
    if treasure {
        gfx.text_aligned(
            "보물",
            r.x,
            r.y + 6.0,
            r.w - 7.0,
            Align::Right,
            small.color(theme::TEXT_GOOD),
        );
    }
    if passable {
        gfx.text(
            &format!("방어 {}%", t.defense),
            r.x + 7.0,
            r.y + 18.0,
            small,
        );
    } else {
        gfx.text(
            "진입 불가",
            r.x + 7.0,
            r.y + 18.0,
            small.color(theme::TEXT_DIM),
        );
    }
    let heal = match (t.heal_hp, t.heal_morale) {
        (0, 0) => None,
        (h, 0) => Some(format!("회복 {h}%")),
        (0, m) => Some(format!("사기 +{m}")),
        (h, m) => Some(format!("회복 {h}% · 사기 +{m}")),
    };
    if let Some(h) = heal {
        gfx.text(&h, r.x + 7.0, r.y + 29.0, small.color(theme::TEXT_GOOD));
    }
}

/// Filled tile highlight with a lighter border, pulsing slightly.
pub fn draw_tile_highlight(screen: Vec2, color: Color, time: f64) {
    let pulse = 0.82 + 0.18 * ((time * 3.2).sin() as f32 * 0.5 + 0.5);
    let r = Rect::new(screen.x, screen.y, TILE, TILE);
    fill_rect(r, color.with_alpha(color.a * pulse));
    let edge = Color::new(
        (color.r + 0.35).min(1.0),
        (color.g + 0.35).min(1.0),
        (color.b + 0.35).min(1.0),
        0.55 * pulse,
    );
    stroke_rect(
        Rect::new(r.x + 1.0, r.y + 1.0, TILE - 2.0, TILE - 2.0),
        edge,
    );
}

pub const MOVE_COLOR: Color = Color::new(0.18, 0.45, 1.0, 0.46);
pub const REACH_COLOR: Color = Color::new(0.95, 0.06, 0.08, 0.50);
pub const TARGET_COLOR: Color = Color::new(1.0, 0.22, 0.18, 0.55);
pub const AIM_COLOR: Color = Color::new(0.72, 0.35, 1.0, 0.42);
pub const AREA_COLOR: Color = Color::new(0.9, 0.55, 1.0, 0.6);
pub const ITEM_COLOR: Color = Color::new(0.3, 0.9, 0.45, 0.45);

/// Map cursor: four gold corner brackets that breathe in and out.
pub fn draw_cursor(screen: Vec2, time: f64, color: Color) {
    let inset = if (time * 2.5).fract() < 0.5 { 0.0 } else { 1.0 };
    let (x0, y0) = (screen.x - 1.0 + inset, screen.y - 1.0 + inset);
    let (x1, y1) = (screen.x + TILE + 1.0 - inset, screen.y + TILE + 1.0 - inset);
    let l = 5.0;
    let dark = Color::new(0.0, 0.0, 0.0, 0.6);
    for (c, d) in [(dark, 1.0), (color, 0.0)] {
        // top-left
        fill_rect(Rect::new(x0 + d, y0 + d, l, 2.0), c);
        fill_rect(Rect::new(x0 + d, y0 + d, 2.0, l), c);
        // top-right
        fill_rect(Rect::new(x1 - l + d, y0 + d, l, 2.0), c);
        fill_rect(Rect::new(x1 - 2.0 + d, y0 + d, 2.0, l), c);
        // bottom-left
        fill_rect(Rect::new(x0 + d, y1 - 2.0 + d, l, 2.0), c);
        fill_rect(Rect::new(x0 + d, y1 - l + d, 2.0, l), c);
        // bottom-right
        fill_rect(Rect::new(x1 - l + d, y1 - 2.0 + d, l, 2.0), c);
        fill_rect(Rect::new(x1 - 2.0 + d, y1 - l + d, 2.0, l), c);
    }
}

/// Mini HP bar under a unit.
pub fn draw_mini_hp(screen: Vec2, hp: f32, max_hp: i32, alpha: f32) {
    let r = Rect::new(screen.x + 2.0, screen.y + TILE - 2.0, TILE - 4.0, 3.0);
    fill_rect(r, Color::new(0.0, 0.0, 0.0, 0.75 * alpha));
    let k = crate::ui::bars::ratio(hp, max_hp as f32);
    let w = ((r.w - 2.0) * k).ceil();
    if w > 0.0 {
        fill_rect(
            Rect::new(r.x + 1.0, r.y + 1.0, w, 1.0),
            GaugeKind::Hp.color(k).with_alpha(alpha),
        );
    }
}

/// Tiny gold crown marking the lord (`at` = top-left of the 7×5 mark).
pub fn draw_crown(at: Vec2, alpha: f32) {
    let gold = Color::from_hex(0xffd23f).with_alpha(alpha);
    let dark = Color::new(0.2, 0.1, 0.0, 0.9 * alpha);
    fill_rect(Rect::new(at.x - 1.0, at.y - 1.0, 9.0, 7.0), dark);
    fill_rect(Rect::new(at.x, at.y + 2.0, 7.0, 3.0), gold);
    for dx in [0.0, 3.0, 6.0] {
        fill_rect(Rect::new(at.x + dx, at.y, 1.0, 2.0), gold);
    }
    fill_rect(
        Rect::new(at.x + 3.0, at.y + 3.0, 1.0, 1.0),
        Color::from_hex(0xd8342c).with_alpha(alpha),
    );
}

/// Orbiting dots above a confused unit.
pub fn draw_confusion(center: Vec2, time: f64, alpha: f32) {
    for i in 0..3 {
        let a = time as f32 * 4.0 + i as f32 * std::f32::consts::TAU / 3.0;
        let p = center + vec2(a.cos() * 5.0, a.sin() * 2.0);
        fill_rect(
            Rect::new(p.x.round(), p.y.round(), 2.0, 2.0),
            Color::from_hex(0xffe066).with_alpha(alpha),
        );
    }
}

pub fn float_color(kind: FloatKind) -> Color {
    match kind {
        FloatKind::Damage => Color::from_hex(0xffffff),
        FloatKind::Heal => Color::from_hex(0x7cf08c),
        FloatKind::Mp => Color::from_hex(0x7cc4ff),
        FloatKind::Morale => Color::from_hex(0xe7a6ff),
        FloatKind::Exp => Color::from_hex(0xffd35a),
        FloatKind::Info => Color::from_hex(0xfff2c0),
        FloatKind::Miss => Color::from_hex(0xb8bcd0),
    }
}

/// A rising number/text centred above `tile_screen` (top-left of its tile).
pub fn draw_float(gfx: &Gfx, tile_screen: Vec2, f: &FloatText) {
    let k = f.age / f.life;
    let rise = (f.age * 3.5).min(1.0) * 10.0 + f.row as f32 * 11.0;
    let alpha = if k > 0.75 {
        1.0 - (k - 0.75) / 0.25
    } else {
        1.0
    };
    let (font, size) = match f.kind {
        FloatKind::Damage => (FontId::Main, 1),
        _ => (FontId::Small, 1),
    };
    let w = gfx.text_width(&f.text, font, size);
    // Damage numbers pop in with a small bounce.
    let bounce = if f.kind == FloatKind::Damage && f.age < 0.15 {
        -3.0 * (f.age / 0.15 * std::f32::consts::PI).sin()
    } else {
        0.0
    };
    let x = (tile_screen.x + TILE / 2.0 - w / 2.0).round();
    let y = (tile_screen.y - 8.0 - rise + bounce).round();
    let color = float_color(f.kind).with_alpha(alpha);
    let outline = if f.kind == FloatKind::Damage {
        Color::from_hex(0x9a1010).with_alpha(alpha)
    } else {
        Color::new(0.0, 0.0, 0.0, 0.85 * alpha)
    };
    outlined(
        gfx,
        &f.text,
        x,
        y,
        TextStyle {
            font,
            size,
            color,
            shadow: None,
        },
        outline,
    );
}

fn tone_colors(tone: Tone) -> (Color, Color) {
    match tone {
        Tone::Player => (Color::from_hex(0x0c1a52), Color::from_hex(0x2c55c8)),
        Tone::Ally => (Color::from_hex(0x0b3318), Color::from_hex(0x2f8a45)),
        Tone::Enemy => (Color::from_hex(0x3a0a0a), Color::from_hex(0xb02a24)),
        Tone::Neutral => (Color::from_hex(0x10142c), Color::from_hex(0x3a4476)),
        Tone::Good => (Color::from_hex(0x2c2208), Color::from_hex(0x9c7a1c)),
    }
}

/// A banner across the middle of the map viewport.
pub fn draw_banner(ctx: &Ctx, b: &BannerView, viewport: Rect) {
    let gfx = &ctx.gfx;
    let a = b.alpha();
    if a <= 0.0 {
        return;
    }
    let h = if b.subtitle.is_some() { 50.0 } else { 36.0 };
    let y = (viewport.y + (viewport.h - h) / 2.0 - 20.0).round();
    let (dark, mid) = tone_colors(b.tone);
    let dark = dark.with_alpha(0.0);
    let mid = mid.with_alpha(0.88 * a);
    let half = VIRTUAL_W / 2.0;
    // Band opens from the centre.
    let open = (b.age / 0.2).min(1.0);
    let bw = half * open;
    fill_gradient_h(Rect::new(half - bw, y, bw, h), dark, mid);
    fill_gradient_h(Rect::new(half, y, bw, h), mid, dark);
    let gold = theme::TEXT_ACCENT.with_alpha(a);
    fill_gradient_h(Rect::new(half - bw, y, bw, 1.0), gold.with_alpha(0.0), gold);
    fill_gradient_h(Rect::new(half, y, bw, 1.0), gold, gold.with_alpha(0.0));
    fill_gradient_h(
        Rect::new(half - bw, y + h - 1.0, bw, 1.0),
        gold.with_alpha(0.0),
        gold,
    );
    fill_gradient_h(
        Rect::new(half, y + h - 1.0, bw, 1.0),
        gold,
        gold.with_alpha(0.0),
    );
    let title = TextStyle::main(theme::TEXT.with_alpha(a))
        .size(2)
        .shadow(Color::new(0.0, 0.0, 0.0, 0.8 * a));
    let tw = gfx.text_width(&b.title, FontId::Main, 2);
    let icon_w = if b.icon.is_some() { 20.0 } else { 0.0 };
    let x = ((VIRTUAL_W - tw - icon_w) / 2.0).round();
    if let Some(icon) = b.icon {
        draw_icon(ctx, icon, vec2(x, y + 9.0));
    }
    gfx.text(&b.title, x + icon_w, y + 2.0, title);
    if let Some(sub) = &b.subtitle {
        gfx.text_aligned(
            sub,
            0.0,
            y + 34.0,
            VIRTUAL_W,
            Align::Center,
            TextStyle::small(theme::TEXT_ACCENT.with_alpha(a)).shadow(Color::new(
                0.0,
                0.0,
                0.0,
                0.8 * a,
            )),
        );
    }
}

/// Level up / promotion / new strategy window.
/// `below` puts the window in the lower part of the viewport (the unit it is about stands in
/// the upper part).
pub fn draw_popup(ctx: &Ctx, p: &Popup, state: &BattleState, viewport: Rect, below: bool) {
    let gfx = &ctx.gfx;
    let w = 200.0;
    let h = 26.0 + p.lines.len() as f32 * 14.0 + 6.0;
    let h = h.max(58.0);
    let pop = (p.age / 0.15).min(1.0);
    let y = if below {
        viewport.bottom() - h - 24.0
    } else {
        viewport.y + 24.0
    };
    let r = Rect::new(
        ((VIRTUAL_W - w) / 2.0).round(),
        (y + (1.0 - pop) * 6.0).round(),
        w,
        h,
    );
    draw_window(r);
    let portrait = p
        .unit
        .and_then(|u| state.units.get(u))
        .and_then(|u| u.portrait.as_deref());
    let tx = if p.unit.is_some() {
        draw_portrait(ctx, portrait, Rect::new(r.x + 6.0, r.y + 6.0, 36.0, 45.0));
        r.x + 48.0
    } else {
        r.x + 10.0
    };
    gfx.text(
        &p.title,
        tx,
        r.y + 5.0,
        TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
    );
    for (i, line) in p.lines.iter().enumerate() {
        gfx.text(
            line,
            tx + 2.0,
            r.y + 24.0 + i as f32 * 14.0,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
    }
}

/// Small caption naming the action being carried out.
pub fn draw_caption(ctx: &Ctx, s: &str, age: f32, viewport: Rect) {
    let gfx = &ctx.gfx;
    let a = (age / 0.12).min(1.0);
    let w = gfx.text_width(s, FontId::Main, 1) + 24.0;
    let r = Rect::new(
        ((VIRTUAL_W - w) / 2.0).round(),
        viewport.y + 6.0,
        w.round(),
        22.0,
    );
    draw_window_ex(r, WindowStyle::Normal, 0.95 * a);
    gfx.text_aligned(
        s,
        r.x,
        r.y + 3.0,
        r.w,
        Align::Center,
        TextStyle::main(theme::TEXT.with_alpha(a)).shadow(theme::TEXT_SHADOW),
    );
}

/// Big 승리 / 패배 caption.
pub fn draw_outcome(ctx: &Ctx, victory: bool, age: f32) {
    let gfx = &ctx.gfx;
    let a = (age / 0.4).min(1.0);
    fill_rect(
        Rect::new(0.0, 0.0, VIRTUAL_W, crate::gfx::VIRTUAL_H),
        Color::new(0.0, 0.0, 0.0, 0.35 * a),
    );
    let (title, sub, tone) = if victory {
        ("승 리", "적군을 물리쳤다!", Tone::Good)
    } else {
        ("패 배", "아군이 패했다…", Tone::Enemy)
    };
    let h = 70.0;
    let y = 90.0;
    let (dark, mid) = tone_colors(tone);
    let half = VIRTUAL_W / 2.0;
    let open = (age / 0.3).min(1.0);
    let bw = half * open;
    fill_gradient_h(
        Rect::new(half - bw, y, bw, h),
        dark.with_alpha(0.0),
        mid.with_alpha(0.92),
    );
    fill_gradient_h(
        Rect::new(half, y, bw, h),
        mid.with_alpha(0.92),
        dark.with_alpha(0.0),
    );
    let gold = theme::TEXT_ACCENT;
    fill_gradient_h(Rect::new(half - bw, y, bw, 2.0), gold.with_alpha(0.0), gold);
    fill_gradient_h(Rect::new(half, y, bw, 2.0), gold, gold.with_alpha(0.0));
    fill_gradient_h(
        Rect::new(half - bw, y + h - 2.0, bw, 2.0),
        gold.with_alpha(0.0),
        gold,
    );
    fill_gradient_h(
        Rect::new(half, y + h - 2.0, bw, 2.0),
        gold,
        gold.with_alpha(0.0),
    );
    let title_color = if victory {
        Color::from_hex(0xffe27a)
    } else {
        Color::from_hex(0xffb0a0)
    };
    gfx.text_aligned(
        title,
        0.0,
        y + 4.0,
        VIRTUAL_W,
        Align::Center,
        TextStyle::main(title_color.with_alpha(a))
            .size(3)
            .shadow(Color::new(0.0, 0.0, 0.0, 0.85 * a)),
    );
    gfx.text_aligned(
        sub,
        0.0,
        y + 52.0,
        VIRTUAL_W,
        Align::Center,
        TextStyle::main(theme::TEXT.with_alpha(a)).shadow(theme::TEXT_SHADOW),
    );
}

/// Commander banner from `gfx/ui/flags.png` (4 frames per side row).
pub fn draw_flag(ctx: &Ctx, at: Vec2, side: Side, time: f64, alpha: f32) {
    let Some(tex) = ctx.media.texture("ui/flags") else {
        return;
    };
    let row = match side {
        Side::Player => 0.0,
        Side::Ally => 1.0,
        Side::Enemy => 2.0,
    };
    let frame = ((time * 6.0).floor() as i64).rem_euclid(4) as f32;
    draw_texture_ex(
        &tex,
        at.x.round(),
        at.y.round(),
        Color::new(1.0, 1.0, 1.0, alpha),
        DrawTextureParams {
            dest_size: Some(vec2(16.0, 16.0)),
            source: Some(Rect::new(frame * 16.0, row * 16.0, 16.0, 16.0)),
            ..Default::default()
        },
    );
}

/// Frame of a sprite sheet with a tint (for greyed and fading units).
pub fn draw_frame(tex: &Texture2D, frame: Vec2, cell: (u32, u32), pos: Vec2, tint: Color) {
    draw_texture_ex(
        tex,
        pos.x.round(),
        pos.y.round(),
        tint,
        DrawTextureParams {
            dest_size: Some(frame),
            source: Some(Rect::new(
                cell.0 as f32 * frame.x,
                cell.1 as f32 * frame.y,
                frame.x,
                frame.y,
            )),
            ..Default::default()
        },
    );
}

/// Title card shown when the battle opens: name, location, a thin line.
pub fn draw_title_card(ctx: &Ctx, name: &str, location: &str, age: f32) {
    let gfx = &ctx.gfx;
    let a = (age / 0.5).min(1.0) * ((2.6 - age) / 0.4).clamp(0.0, 1.0);
    fill_rect(
        Rect::new(0.0, 0.0, VIRTUAL_W, crate::gfx::VIRTUAL_H),
        Color::new(0.0, 0.0, 0.02, 0.55 + 0.25 * a),
    );
    let y = 100.0;
    let line_w = 220.0 * (age / 0.6).min(1.0);
    fill_gradient_h(
        Rect::new(VIRTUAL_W / 2.0 - line_w, y + 36.0, line_w, 1.0),
        theme::TEXT_ACCENT.with_alpha(0.0),
        theme::TEXT_ACCENT.with_alpha(a),
    );
    fill_gradient_h(
        Rect::new(VIRTUAL_W / 2.0, y + 36.0, line_w, 1.0),
        theme::TEXT_ACCENT.with_alpha(a),
        theme::TEXT_ACCENT.with_alpha(0.0),
    );
    gfx.text_aligned(
        name,
        0.0,
        y,
        VIRTUAL_W,
        Align::Center,
        TextStyle::main(theme::TEXT.with_alpha(a))
            .size(2)
            .shadow(Color::new(0.0, 0.0, 0.0, 0.8 * a)),
    );
    if !location.is_empty() {
        gfx.text_aligned(
            location,
            0.0,
            y + 42.0,
            VIRTUAL_W,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT.with_alpha(a)).shadow(theme::TEXT_SHADOW),
        );
    }
}

/// Window with a heading and sections of lines (objective, result). Returns its rectangle.
pub fn draw_text_window(
    ctx: &Ctx,
    title: &str,
    sections: &[(String, Vec<(String, Color)>)],
    footer: &str,
) -> Rect {
    let gfx = &ctx.gfx;
    let lines: usize = sections.iter().map(|(_, l)| l.len() + 1).sum();
    let w = 300.0;
    let h = 30.0 + lines as f32 * 15.0 + 22.0;
    let r = Rect::new(
        ((VIRTUAL_W - w) / 2.0).round(),
        ((crate::gfx::VIRTUAL_H - h) / 2.0).round(),
        w,
        h,
    );
    draw_window(r);
    gfx.text_aligned(
        title,
        r.x,
        r.y + 6.0,
        r.w,
        Align::Center,
        TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
    );
    crate::ui::window::draw_divider(r.x + 8.0, r.y + 24.0, r.w - 16.0);
    let mut y = r.y + 30.0;
    for (heading, lines) in sections {
        gfx.text(
            heading,
            r.x + 12.0,
            y,
            TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
        );
        y += 15.0;
        for (line, color) in lines {
            gfx.text(
                line,
                r.x + 22.0,
                y - 2.0,
                TextStyle::main(*color).shadow(theme::TEXT_SHADOW),
            );
            y += 15.0;
        }
    }
    gfx.text_aligned(
        footer,
        r.x,
        r.bottom() - 17.0,
        r.w - 10.0,
        Align::Right,
        TextStyle::small(theme::TEXT_DISABLED),
    );
    r
}

/// Small triangle: pointing up (advantage) or down (disadvantage).
pub fn draw_affinity_arrow(at: Vec2, up: bool) {
    let (c, pts) = if up {
        (
            Color::from_hex(0x7cf08c),
            [
                vec2(at.x, at.y + 7.0),
                vec2(at.x + 8.0, at.y + 7.0),
                vec2(at.x + 4.0, at.y),
            ],
        )
    } else {
        (
            Color::from_hex(0xff7a6a),
            [
                vec2(at.x, at.y),
                vec2(at.x + 8.0, at.y),
                vec2(at.x + 4.0, at.y + 7.0),
            ],
        )
    };
    draw_triangle(pts[0], pts[1], pts[2], c);
}

/// Twinkle marking an untaken treasure tile.
pub fn draw_twinkle(tile_screen: Vec2, time: f64) {
    let k = ((time * 2.0).sin() as f32 * 0.5 + 0.5) * 0.8 + 0.2;
    let c = Color::from_hex(0xfff3b0).with_alpha(k);
    let x = tile_screen.x + 12.0;
    let y = tile_screen.y + 2.0;
    fill_rect(Rect::new(x, y - 2.0, 1.0, 5.0), c);
    fill_rect(Rect::new(x - 2.0, y, 5.0, 1.0), c);
}
