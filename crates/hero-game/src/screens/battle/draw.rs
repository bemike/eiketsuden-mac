//! Drawing of the battle screen: map, range highlights, units, effects, info panels,
//! forecasts, menus and the battle menu windows. `Screen::draw` in the parent module decides
//! what is shown when; the pieces live here.

use super::*;

impl BattleScreen {
    pub(super) fn draw_map(&self, ctx: &Ctx) {
        let tileset = self.meta.tileset.as_ref();
        let atlas = tileset.and_then(|ts| ctx.media.texture(&ts.texture));
        self.map.draw(
            self.camera.to_screen(Vec2::ZERO),
            &self.state.map,
            tileset,
            atlas.as_ref(),
            ctx.time,
        );
        // Tiles whose terrain an event changed, with their own picture.
        let tile = self.tile();
        for m in &self.shown_tiles {
            if let Some(texture) = ctx.media.texture(&format!("maps/{}", m.image)) {
                let at = self.camera.tile_screen(m.pos);
                draw_texture_ex(
                    &texture,
                    at.x.round(),
                    at.y.round(),
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(tile, tile)),
                        ..Default::default()
                    },
                );
            }
        }
        // Untaken treasures twinkle.
        for (i, t) in self.def().treasures.iter().enumerate() {
            if !self.state.treasures_taken.get(i).copied().unwrap_or(false) {
                hud::draw_twinkle(self.camera.tile_screen(t.pos), self.tile(), ctx.time);
            }
        }
    }

    pub(super) fn draw_highlights(&self, ctx: &Ctx) {
        let t = ctx.time;
        let tile = self.tile();
        let hl =
            |p: Pos, c: Color| hud::draw_tile_highlight(self.camera.tile_screen(p), tile, c, t);
        let centre = vec2(tile, tile) / 2.0;
        match &self.ui.mode {
            Mode::Move { range, reach, unit } | Mode::Inspect { range, reach, unit } => {
                let own = matches!(self.ui.mode, Mode::Move { .. });
                let move_color = if own {
                    hud::MOVE_COLOR
                } else {
                    hud::MOVE_COLOR.with_alpha(0.26)
                };
                for p in range.tiles.keys() {
                    hl(*p, move_color);
                }
                for p in reach {
                    hl(*p, hud::REACH_COLOR);
                }
                // Path preview to the cursor.
                if own && self.cursor != self.state.units[*unit].pos {
                    if let Some(path) = range.path_to(self.cursor) {
                        for w in path.windows(2) {
                            let a = self.camera.tile_screen(w[0]) + centre;
                            let b = self.camera.tile_screen(w[1]) + centre;
                            draw_line(a.x, a.y, b.x, b.y, 3.0, Color::new(1.0, 0.9, 0.4, 0.85));
                        }
                        let end = self.camera.tile_screen(self.cursor) + centre;
                        draw_circle(end.x, end.y, 3.0, Color::new(1.0, 0.9, 0.4, 0.95));
                    }
                }
            }
            Mode::Attack { unit, targets, .. } => {
                let pos = self.state.units[*unit].pos;
                for p in self.state.attack_tiles(&self.pack, *unit, pos) {
                    hl(p, hud::REACH_COLOR);
                }
                for &u in targets {
                    hl(self.state.units[u].pos, hud::TARGET_COLOR);
                }
            }
            Mode::Aim {
                unit, list, index, ..
            } => {
                let e = &list[*index];
                if let Ok(aims) = &e.aims {
                    for p in aims {
                        hl(*p, hud::AIM_COLOR);
                    }
                    if aims.contains(&self.cursor) {
                        let caster = self.state.units[*unit].pos;
                        for p in player::area_tiles(&self.pack, &e.id, caster, self.cursor) {
                            if self.state.map.in_bounds(p) {
                                hl(p, hud::AREA_COLOR);
                            }
                        }
                    }
                }
            }
            Mode::ItemTarget { list, index, .. } => {
                if let Ok(targets) = &list[*index].targets {
                    for &u in targets {
                        hl(self.state.units[u].pos, hud::ITEM_COLOR);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn draw_units(&self, ctx: &Ctx) {
        if !self.meta.units_loaded() {
            return;
        }
        let mut order: Vec<usize> = (0..self.scene.views.len())
            .filter(|&i| self.scene.views[i].visible)
            .collect();
        order.sort_by(|&a, &b| {
            let (va, vb) = (&self.scene.views[a], &self.scene.views[b]);
            (va.pos.y + va.offset.y)
                .total_cmp(&(vb.pos.y + vb.offset.y))
                .then(a.cmp(&b))
        });
        let default_sprite = sprites::SpriteDef::default();
        let tile = self.tile();
        let canvas = ctx.gfx.size();
        // Units whose tile is this far outside the canvas cannot show (sprites overhang their
        // tile mostly upwards and sideways).
        let (before, after) = (2.5 * tile, 1.5 * tile);
        for i in order {
            let v = &self.scene.views[i];
            let u = &self.state.units[i];
            let screen = self.camera.map_to_screen(v.pos + v.offset);
            if screen.x < -before
                || screen.x > canvas.x + after
                || screen.y < -before
                || screen.y > canvas.y + after
            {
                continue;
            }
            let sprite = self.unit_sprite(u.officer.as_deref(), &v.class, v.confused);
            let def = self
                .meta
                .units
                .sprites
                .get(sprite)
                .unwrap_or(&default_sprite);
            let frame = vec2(def.frame[0] as f32, def.frame[1] as f32);
            let origin = sprites::frame_origin(screen, tile, def);
            // Marks above the unit sit relative to the top of its frame.
            let head = origin.y - screen.y;
            let foot = sprites::tile_foot(tile);
            let flicker = v.flash > 0.0 && ((ctx.time * 30.0) as i64) % 2 == 0;
            let grey = if v.acted { 0.5 } else { 1.0 };
            let tint = Color::new(grey, grey, grey + if v.acted { 0.05 } else { 0.0 }, v.alpha);
            // Shadow under the unit.
            draw_ellipse(
                screen.x + tile / 2.0,
                screen.y + tile - 2.0,
                tile * 0.375,
                tile * 0.125,
                0.0,
                Color::new(0.0, 0.0, 0.0, 0.28 * v.alpha),
            );
            if !flicker {
                let key = self.sheet(sprite, v.side);
                match key.map(|k| (k, ctx.media.texture_state(k))) {
                    Some((k, AssetState::Ready)) => {
                        if let Some(tex) = ctx.media.texture(k) {
                            let row =
                                sprites::pose_row(v.pose, ctx.time + i as f64 * 0.37, !v.acted);
                            hud::draw_frame(
                                &tex,
                                frame,
                                (sprites::facing_column(v.facing), row),
                                origin,
                                tint,
                            );
                        }
                    }
                    Some((_, AssetState::Loading)) => {}
                    _ => draw_placeholder(
                        Rect::new(screen.x + 2.0, screen.y + 1.0, tile - 4.0, tile - 2.0),
                        sprite,
                    ),
                }
            }
            if v.alpha > 0.05 {
                if u.commander {
                    hud::draw_flag(
                        ctx,
                        screen + vec2(foot.x - 1.0, head - 6.0),
                        v.side,
                        ctx.time + i as f64 * 0.2,
                        v.alpha,
                    );
                }
                if u.lord {
                    hud::draw_crown(screen + vec2(0.0, head), v.alpha);
                }
                if v.confused {
                    hud::draw_confusion(screen + vec2(tile / 2.0, head - 1.0), ctx.time, v.alpha);
                }
                if v.knocked.is_none() {
                    hud::draw_mini_hp(screen, tile, v.hp, v.max_hp, v.alpha);
                }
            }
        }
    }

    pub(super) fn draw_fx(&self, ctx: &Ctx) {
        for f in &self.scene.fx {
            let (Some(def), Some(key)) =
                (self.meta.fx.get(&f.key), self.meta.fx_textures.get(&f.key))
            else {
                continue;
            };
            let Some(frame) = def.frame_at(f.age) else {
                continue;
            };
            let Some(tex) = ctx.media.texture(key) else {
                continue;
            };
            let size = vec2(def.frame[0] as f32, def.frame[1] as f32);
            let c = self.camera.map_to_screen(f.center);
            hud::draw_frame(&tex, size, (frame, 0), c - size / 2.0, WHITE);
        }
    }

    /// Unit shown in the info panel: the one under the cursor, else the selected / acting one.
    pub(super) fn panel_unit(&self) -> Option<UnitId> {
        if !self.events.is_idle() {
            return self.events.focus_unit().or(self.ai.map(|a| a.unit));
        }
        self.state.unit_at(self.cursor).or_else(|| self.ui.unit())
    }

    /// Whether the info panels go to the top (the cursor is in the lower half).
    pub(super) fn panels_on_top(&self) -> bool {
        let s = self.camera.tile_screen(self.cursor);
        let vp = self.camera.viewport;
        s.y > vp.y + vp.h * 0.55
    }

    /// Whether the cursor is on something the forecast window describes.
    pub(super) fn has_forecast(&self) -> bool {
        let at = self.cursor;
        match &self.ui.mode {
            Mode::Attack { targets, .. } => {
                self.state.unit_at(at).is_some_and(|t| targets.contains(&t))
            }
            Mode::Aim { list, index, .. } => {
                list[*index].aims.as_ref().is_ok_and(|a| a.contains(&at))
            }
            Mode::ItemTarget { list, index, .. } => {
                match (&list[*index].targets, self.state.unit_at(at)) {
                    (Ok(ts), Some(t)) => ts.contains(&t),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Unit panel (left) and terrain panel (right) in the band away from the cursor. The
    /// terrain panel gives way to the forecast window when `terrain` is false.
    pub(super) fn draw_panels(&self, ctx: &Ctx, terrain: bool) {
        let top = self.panels_on_top();
        let canvas = ctx.gfx.size();
        let vp = self.camera.viewport;
        let y_unit = if top {
            vp.y + 4.0
        } else {
            canvas.y - hud::UNIT_PANEL.y - 4.0
        };
        if let Some(u) = self.panel_unit() {
            if self.state.units[u].is_active() || self.scene.views[u].visible {
                hud::draw_unit_panel(
                    ctx,
                    vec2(4.0, y_unit),
                    &self.pack,
                    &self.state,
                    u,
                    &self.scene.views[u],
                );
            }
        }
        if terrain && self.events.is_idle() && self.state.phase == Side::Player {
            if let Some(t) = self.state.terrain_at(&self.pack, self.cursor) {
                let treasure = self.def().treasures.iter().enumerate().any(|(i, tr)| {
                    tr.pos == self.cursor
                        && !self.state.treasures_taken.get(i).copied().unwrap_or(false)
                });
                let y = if top {
                    vp.y + 4.0
                } else {
                    canvas.y - hud::TERRAIN_PANEL.y - 4.0
                };
                hud::draw_terrain_panel(
                    ctx,
                    vec2(canvas.x - hud::TERRAIN_PANEL.x - 4.0, y),
                    t,
                    treasure,
                    !t.cost.is_empty(),
                );
            }
        }
    }

    /// Forecast window for the target under the cursor (attack, strategy, item).
    pub(super) fn draw_forecast(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let Some(unit) = self.ui.unit() else {
            return;
        };
        // Same band as the unit panel, on the right (where the terrain panel would be).
        let top = self.panels_on_top();
        let canvas = gfx.size();
        let vp = self.camera.viewport;
        let place = |h: f32, w: f32| {
            let x = canvas.x - w - 4.0;
            let y = if top { vp.y + 4.0 } else { canvas.y - h - 4.0 };
            Rect::new(x, y, w, h)
        };
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
        let me = &self.state.units[unit];
        match &self.ui.mode {
            Mode::Attack { targets, .. } => {
                let Some(t) = self
                    .state
                    .unit_at(self.cursor)
                    .filter(|t| targets.contains(t))
                else {
                    return;
                };
                let f = self.state.forecast_attack(&self.pack, unit, t);
                let target = &self.state.units[t];
                let lines = text::attack_lines(&f, target.hp);
                let r = place(66.0, 188.0);
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} → {}", me.name, target.name),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                let dw = gfx.text(
                    &lines.damage,
                    r.x + 10.0,
                    r.y + 20.0,
                    main.color(theme::TEXT_ACCENT),
                );
                if let Some(up) = text::affinity_mark(f.affinity) {
                    hud::draw_affinity_arrow(vec2(r.x + 14.0 + dw, r.y + 25.0), up);
                    gfx.text(
                        if up { "상성 유리" } else { "상성 불리" },
                        r.x + 26.0 + dw,
                        r.y + 22.0,
                        small.color(if up {
                            theme::TEXT_GOOD
                        } else {
                            theme::TEXT_BAD
                        }),
                    );
                }
                gfx.text(
                    &lines.result,
                    r.x + 10.0,
                    r.y + 36.0,
                    small.color(if lines.defeats {
                        theme::TEXT_GOOD
                    } else {
                        theme::TEXT
                    }),
                );
                gfx.text(
                    &lines.counter,
                    r.x + 10.0,
                    r.y + 49.0,
                    small.color(if f.counter.is_some() {
                        theme::TEXT_BAD
                    } else {
                        theme::TEXT_DIM
                    }),
                );
            }
            Mode::Aim { list, index, .. } => {
                let e = &list[*index];
                if !e.aims.as_ref().is_ok_and(|a| a.contains(&self.cursor)) {
                    return;
                }
                let fs = self
                    .state
                    .forecast_strategy(&self.pack, unit, &e.id, self.cursor);
                let shown = fs.len().min(5);
                let r = place(
                    24.0 + shown as f32 * 13.0 + if fs.len() > 5 { 12.0 } else { 0.0 },
                    210.0,
                );
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} · {} (MP {})", me.name, e.name, e.mp),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                for (i, f) in fs.iter().take(5).enumerate() {
                    let y = r.y + 20.0 + i as f32 * 13.0;
                    let name = &self.state.units[f.unit].name;
                    gfx.text(
                        name,
                        r.x + 10.0,
                        y,
                        small.color(hud::side_color(self.state.units[f.unit].side)),
                    );
                    gfx.text(
                        &text::strategy_line(f),
                        r.x + 78.0,
                        y,
                        small.color(theme::TEXT),
                    );
                }
                if fs.len() > 5 {
                    gfx.text(
                        &format!("외 {}부대", fs.len() - 5),
                        r.x + 10.0,
                        r.y + 20.0 + 5.0 * 13.0,
                        small,
                    );
                }
            }
            Mode::ItemTarget { list, index, .. } => {
                let e = &list[*index];
                let Some(t) = self.state.unit_at(self.cursor) else {
                    return;
                };
                if !e.targets.as_ref().is_ok_and(|ts| ts.contains(&t)) {
                    return;
                }
                let target = &self.state.units[t];
                let r = place(38.0, 188.0);
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} → {}", e.name, target.name),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                let line = match &e.strategy {
                    Some(s) => {
                        let fs = self
                            .state
                            .forecast_strategy(&self.pack, unit, s, target.pos);
                        fs.iter()
                            .find(|f| f.unit == t)
                            .map_or_else(|| "효과 없음".to_string(), text::strategy_line)
                    }
                    None => item_effect_line(&self.pack, &e.id, target),
                };
                gfx.text(&line, r.x + 10.0, r.y + 20.0, small.color(theme::TEXT));
            }
            _ => {}
        }
    }

    pub(super) fn draw_mode_menu(&self, ctx: &Ctx) {
        let Some((kind, menu)) = &self.mode_menu else {
            return;
        };
        menu.draw(ctx);
        if *kind == MenuKind::Command {
            return;
        }
        // Icons in the tag column and the description of the highlighted row.
        let cursor = menu.cursor();
        let (desc, blocked, icons): (String, Option<&'static str>, Vec<Option<String>>) =
            match &self.ui.mode {
                Mode::Strategies { list, .. } => (
                    list.get(cursor).map_or(String::new(), |e| {
                        let el = text::element_name(e.element.as_deref());
                        if el.is_empty() {
                            e.desc.clone()
                        } else {
                            format!("[{el}] {}", e.desc)
                        }
                    }),
                    list.get(cursor)
                        .and_then(|e| e.aims.as_ref().err().map(|b| b.text())),
                    list.iter()
                        .map(|e| {
                            self.pack
                                .strategy(&e.id)
                                .map(|s| strategy_icon(s).to_string())
                        })
                        .collect(),
                ),
                Mode::Items { list, .. } => (
                    list.get(cursor).map_or(String::new(), |e| e.desc.clone()),
                    list.get(cursor)
                        .and_then(|e| e.targets.as_ref().err().map(|b| b.text())),
                    list.iter()
                        .map(|e| Some(e.icon.clone()).filter(|i| !i.is_empty()))
                        .collect(),
                ),
                _ => return,
            };
        for (i, icon) in icons.iter().enumerate() {
            if let Some(icon) = icon {
                let row = menu.row_rect(i);
                if row.y >= menu.rect().y && row.bottom() <= menu.rect().bottom() {
                    draw_icon(ctx, icon, vec2(row.x + 11.0, row.y));
                }
            }
        }
        let list = menu.rect();
        let lines = ctx.gfx.wrap(&desc, FontId::Small, 1, list.w - 14.0);
        let rows = (lines.len() + usize::from(blocked.is_some())).clamp(1, 5);
        let r = Rect::new(
            list.x,
            list.bottom() + 4.0,
            list.w,
            8.0 + rows as f32 * 12.0,
        );
        draw_window_ex(r, WindowStyle::Panel, 0.96);
        let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let mut y = r.y + 4.0;
        if let Some(b) = blocked {
            ctx.gfx.text(b, r.x + 7.0, y, small.color(theme::TEXT_BAD));
            y += 12.0;
        }
        let room = ((r.bottom() - 4.0 - y) / 12.0).floor().max(0.0) as usize;
        ctx.gfx
            .text_lines(&lines[..lines.len().min(room)], r.x + 7.0, y, small);
    }

    pub(super) fn draw_panel(&self, ctx: &Ctx) {
        match &self.panel {
            Panel::None => {}
            Panel::Menu(menu) => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.25));
                menu.draw(ctx);
            }
            Panel::Units { side, menu, .. } => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.3));
                let frame = unit_list_frame(menu.rect());
                draw_window(frame);
                for (i, s) in UNIT_TABS.iter().enumerate() {
                    let tab = unit_tab_rect(self.camera.viewport, i);
                    let active = s == side;
                    draw_window_ex(
                        tab,
                        if active {
                            WindowStyle::Normal
                        } else {
                            WindowStyle::Panel
                        },
                        1.0,
                    );
                    ctx.gfx.text_aligned(
                        text::side_name(*s),
                        tab.x,
                        tab.y + 1.0,
                        tab.w,
                        Align::Center,
                        TextStyle::main(if active {
                            hud::side_color(*s)
                        } else {
                            theme::TEXT_DIM
                        })
                        .shadow(theme::TEXT_SHADOW),
                    );
                }
                menu.draw(ctx);
                ctx.gfx.text_aligned(
                    "←/→ 진영 · Z 이동 · X 닫기",
                    frame.x,
                    frame.bottom() + 2.0,
                    frame.w,
                    Align::Right,
                    TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
                );
            }
            Panel::Objective => {
                fill_rect(ctx.gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.3));
                self.draw_objective(ctx, "Z / X 닫기");
            }
        }
    }

    pub(super) fn objective_sections(&self) -> Vec<(String, Vec<(String, Color)>)> {
        let def = self.def();
        let name = |r: &str| self.ref_name(r);
        let mut victory: Vec<(String, Color)> = def
            .victory
            .iter()
            .map(|c| (text::condition_text(c, name), theme::TEXT))
            .collect();
        if victory.is_empty() {
            victory.push(("—".into(), theme::TEXT_DIM));
        }
        let mut defeat: Vec<(String, Color)> = Vec::new();
        if let Some(l) = self.state.units.iter().find(|u| u.lord) {
            defeat.push((format!("{} 퇴각", l.name), theme::TEXT));
        }
        defeat.extend(
            def.defeat
                .iter()
                .map(|c| (text::condition_text(c, name), theme::TEXT)),
        );
        defeat.push((format!("{}턴 경과", self.state.turn_limit), theme::TEXT));
        let mut sections = vec![
            ("승리 조건".to_string(), victory),
            ("패배 조건".to_string(), defeat),
        ];
        if let Some(b) = &def.bonus {
            let (line, color) = if self.state.bonus_done {
                (format!("{} (달성)", b.desc), theme::TEXT_GOOD)
            } else {
                (format!("{} (경험치 +{})", b.desc, b.exp), theme::TEXT)
            };
            sections.push(("보너스 목표".to_string(), vec![(line, color)]));
        }
        sections.push((
            "턴 제한".to_string(),
            vec![(
                format!(
                    "{} / {}턴",
                    self.state.turn.min(self.state.turn_limit),
                    self.state.turn_limit
                ),
                theme::TEXT,
            )],
        ));
        sections
    }

    pub(super) fn draw_objective(&self, ctx: &Ctx, footer: &str) {
        hud::draw_text_window(
            ctx,
            self.state.objective_text(&self.pack),
            &self.objective_sections(),
            footer,
        );
    }
}

/// Icon of a strategy: its element, or its effect for element-less strategies.
fn strategy_icon(s: &hero_core::data::StrategyDef) -> &'static str {
    use hero_core::data::{Effect, StrategyKind};
    match s.element.as_deref() {
        Some("fire") => "fire",
        Some("water") => "water",
        Some("earth") => "earth",
        _ => match s.kind {
            StrategyKind::Heal => "heal",
            _ => {
                if s.effects.iter().any(|e| matches!(e, Effect::Status { .. })) {
                    "confuse"
                } else if s
                    .effects
                    .iter()
                    .any(|e| matches!(e, Effect::Morale { amount } if *amount < 0))
                {
                    "morale_down"
                } else {
                    "morale_up"
                }
            }
        },
    }
}

/// Forecast of a healing / morale item on `target`.
fn item_effect_line(pack: &Pack, item: &str, target: &hero_core::battle::Unit) -> String {
    use hero_core::data::Effect;
    let Some(d) = pack.item(item) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for e in &d.effects {
        match e {
            Effect::Heal { power } => {
                let h = (*power).min(target.max_hp - target.hp).max(0);
                parts.push(format!("회복 {h}"));
            }
            Effect::Morale { amount } => {
                let m = (target.morale + amount).clamp(0, 100) - target.morale;
                parts.push(text::morale_text(m));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        "효과 없음".into()
    } else {
        parts.join(" · ")
    }
}
