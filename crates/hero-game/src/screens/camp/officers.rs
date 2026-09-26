//! 무장 정보: a table of the army (class, level and battle values) and a detail page per officer
//! with portrait, names, class, level and EXP, battle values, 무력/지력/통솔, known strategies,
//! equipment and biography.

use super::stats::officer_stats;
use super::widgets::{
    class_name, draw_camp_backdrop, draw_caption, draw_header, draw_help, draw_list_frame,
    draw_officer_sprite, draw_stats_block, item_icon, officer_name, portrait_key, slot_icon,
    slot_name, visible_rows, TOP,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{Align, FontId, TextStyle, VIRTUAL_W};
use crate::input::Dir;
use crate::ui::art::draw_portrait_card;
use crate::ui::bars::{draw_gauge, draw_gauge_labeled, GaugeKind};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_divider, draw_icon, draw_window_ex, WindowStyle};
use hero_core::campaign::OfficerState;
use hero_core::data::ItemKind;
use hero_core::pack::Pack;
use macroquad::prelude::*;

const TABLE: Rect = Rect {
    x: 8.0,
    y: TOP + 2.0,
    w: 464.0,
    h: 222.0,
};
const ROW_H: f32 = 22.0;
/// Column x offsets from the row's left edge, after the sprite and name.
const COLUMNS: [(&str, f32); 8] = [
    ("병과", 104.0),
    ("Lv", 160.0),
    ("병력", 200.0),
    ("책략치", 250.0),
    ("공격력", 300.0),
    ("방어력", 350.0),
    ("이동력", 400.0),
    ("", 440.0),
];

/// Names of the strategies an officer knows, with their MP cost.
pub fn strategy_list(pack: &Pack, officer: &OfficerState) -> Vec<(String, i32)> {
    pack.known_strategies(&officer.class, officer.level)
        .iter()
        .map(|id| match pack.strategy(id) {
            Some(s) => (s.name.clone(), s.mp),
            None => (id.clone(), 0),
        })
        .collect()
}

/// The officer table and detail pages.
pub struct OfficersScreen {
    menu: Menu,
    /// Index of the officer whose detail page is open.
    detail: Option<usize>,
}

impl Default for OfficersScreen {
    fn default() -> Self {
        OfficersScreen::new()
    }
}

impl OfficersScreen {
    pub fn new() -> OfficersScreen {
        OfficersScreen {
            menu: Menu::new(Vec::new()),
            detail: None,
        }
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let items = session
            .campaign
            .roster
            .iter()
            .map(|o| MenuItem::new(officer_name(pack, &o.id)))
            .collect();
        let rows = ((TABLE.h - 34.0) / ROW_H).floor() as usize;
        let cursor = self.menu.cursor();
        let mut menu = Menu::new(items)
            .rows(rows)
            .at(TABLE.x + 2.0, TABLE.y + 28.0, TABLE.w - 4.0);
        menu.framed = false;
        menu.row_height = ROW_H;
        menu.tag_width = 28.0;
        menu.wrap = false;
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    fn draw_table(&self, ctx: &Ctx, pack: &Pack, roster: &[OfficerState]) {
        let gfx = &ctx.gfx;
        draw_list_frame(ctx, TABLE, "무장 일람", true);
        let head = TextStyle::small(theme::TEXT_DIM);
        let base = self.menu.row_rect(0).x;
        for (label, dx) in COLUMNS {
            if !label.is_empty() {
                gfx.text(label, base + dx, TABLE.y + 16.0, head);
            }
        }
        gfx.text("이름", base + 40.0, TABLE.y + 16.0, head);
        self.menu.draw(ctx);
        let value = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        for (i, row) in visible_rows(&self.menu) {
            let Some(o) = roster.get(i) else {
                continue;
            };
            draw_officer_sprite(
                ctx,
                pack,
                o,
                vec2(row.x + 26.0, row.bottom() - 1.0),
                i == self.menu.cursor(),
            );
            let y = row.y + (row.h - 16.0) / 2.0;
            gfx.text(class_name(pack, &o.class), row.x + COLUMNS[0].1, y, value);
            let s = officer_stats(pack, o);
            let cols = [
                i64::from(o.level),
                s.map_or(0, |s| i64::from(s.hp)),
                s.map_or(0, |s| i64::from(s.mp)),
                s.map_or(0, |s| i64::from(s.atk)),
                s.map_or(0, |s| i64::from(s.def)),
                s.map_or(0, |s| i64::from(s.mov)),
            ];
            for (k, v) in cols.into_iter().enumerate() {
                // Right-align each number under its header.
                let x0 = row.x + COLUMNS[k + 1].1;
                let x1 = row.x + COLUMNS[k + 2].1 - 10.0;
                gfx.text_aligned(&format::thousands(v), x0, y, x1 - x0, Align::Right, value);
            }
        }
    }

    fn draw_detail(&self, ctx: &Ctx, pack: &Pack, o: &OfficerState, index: usize, count: usize) {
        let gfx = &ctx.gfx;
        let Some(def) = pack.officer(&o.id) else {
            return;
        };
        let panel = TABLE;
        draw_window_ex(panel, WindowStyle::Panel, 1.0);
        let x = panel.x + 8.0;
        let y = panel.y + 8.0;
        // Portrait and names.
        draw_portrait_card(
            ctx,
            Some(portrait_key(pack, &o.id)),
            Rect::new(x, y, 96.0, 120.0),
            1.0,
            1.0,
        );
        gfx.text(
            &def.name,
            x,
            y + 124.0,
            TextStyle::main(theme::TEXT_NAME)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        let small = TextStyle::small(theme::TEXT_DIM);
        let mut names = Vec::new();
        if !def.hanja.is_empty() {
            names.push(def.hanja.clone());
        }
        if !def.courtesy.is_empty() {
            names.push(format!("자 {}", def.courtesy));
        }
        gfx.text(
            &names.join("  "),
            x,
            y + 158.0,
            TextStyle::main(theme::TEXT),
        );
        if def.lord {
            draw_icon(ctx, "lord", vec2(x + 80.0, y + 128.0));
        }

        // Class, level, EXP and battle values.
        let cx = x + 108.0;
        let cw = 150.0;
        gfx.text(
            &format!("{}  Lv{}", class_name(pack, &o.class), o.level),
            cx,
            y,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
        if let Some(c) = pack.class(&o.class) {
            if !c.hanja.is_empty() {
                gfx.text_aligned(&c.hanja, cx, y + 2.0, cw, Align::Right, small);
            }
        }
        let per_level = pack.rules.exp_per_level.max(1);
        if o.level >= pack.rules.level_cap {
            gfx.text("경험치", cx, y + 18.0, small);
            gfx.text_aligned("최고 레벨", cx, y + 18.0, cw, Align::Right, small);
        } else {
            draw_gauge_labeled(
                gfx,
                vec2(cx, y + 18.0),
                cw,
                "경험치",
                i64::from(o.exp),
                i64::from(per_level),
                GaugeKind::Exp,
            );
        }
        if let Some(stats) = officer_stats(pack, o) {
            draw_stats_block(gfx, &stats, None, cx, y + 40.0, cw);
        }
        // 무력 / 지력 / 통솔.
        let by = y + 120.0;
        for (i, (label, v)) in [("무력", o.strength), ("지력", o.int), ("통솔", o.lead)]
            .into_iter()
            .enumerate()
        {
            let ry = by + i as f32 * 14.0;
            gfx.text(label, cx, ry, small);
            gfx.text_aligned(
                &v.to_string(),
                cx,
                ry - 1.0,
                cw,
                Align::Right,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
            draw_gauge(
                Rect::new(cx + 28.0, ry + 4.0, cw - 56.0, 5.0),
                v as f32,
                100.0,
                GaugeKind::Custom(theme::TEXT_ACCENT),
            );
        }

        // Equipment and strategies.
        let rx = cx + cw + 14.0;
        let rw = panel.right() - 8.0 - rx;
        draw_caption(gfx, "장비", rx, y);
        for (i, slot) in [ItemKind::Weapon, ItemKind::Armor, ItemKind::Accessory]
            .into_iter()
            .enumerate()
        {
            let id = match slot {
                ItemKind::Weapon => o.equip.weapon.as_ref(),
                ItemKind::Armor => o.equip.armor.as_ref(),
                _ => o.equip.accessory.as_ref(),
            };
            let item = id.and_then(|id| pack.item(id));
            let ry = y + 14.0 + i as f32 * 17.0;
            draw_icon(ctx, item.map_or(slot_icon(slot), item_icon), vec2(rx, ry));
            gfx.text(slot_name(slot), rx + 20.0, ry + 2.0, small);
            gfx.text(
                item.map_or("—", |i| i.name.as_str()),
                rx + 62.0,
                ry,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
        }
        let sy = y + 70.0;
        draw_divider(rx, sy - 4.0, rw);
        draw_caption(gfx, "책략", rx, sy);
        let strategies = strategy_list(pack, o);
        if strategies.is_empty() {
            gfx.text("없음", rx, sy + 14.0, TextStyle::main(theme::TEXT_DIM));
        } else {
            let col_w = (rw / 2.0).floor();
            let rows = 6;
            for (i, (name, mp)) in strategies.iter().take(rows * 2).enumerate() {
                let (col, row) = (i / rows, i % rows);
                let px = rx + col as f32 * col_w;
                let py = sy + 14.0 + row as f32 * 14.0;
                gfx.text(name, px, py, TextStyle::small(theme::TEXT));
                gfx.text_aligned(
                    &mp.to_string(),
                    px,
                    py,
                    col_w - 8.0,
                    Align::Right,
                    TextStyle::small(theme::MP),
                );
            }
            if strategies.len() > rows * 2 {
                gfx.text_aligned(
                    &format!("외 {}개", strategies.len() - rows * 2),
                    rx,
                    sy,
                    rw,
                    Align::Right,
                    small,
                );
            }
        }

        // Biography.
        let bio_y = panel.bottom() - 46.0;
        draw_divider(cx, bio_y - 5.0, panel.right() - 8.0 - cx);
        let lines = gfx.wrap(&def.bio, FontId::Main, 1, panel.right() - 12.0 - cx);
        gfx.text_lines(
            &lines[..lines.len().min(3)],
            cx,
            bio_y,
            TextStyle::main(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
        );
        gfx.text_aligned(
            &format!("{} / {}", index + 1, count),
            0.0,
            4.0,
            VIRTUAL_W - 90.0,
            Align::Right,
            TextStyle::small(theme::TEXT_DIM),
        );
    }
}

impl Screen for OfficersScreen {
    fn name(&self) -> &'static str {
        "camp-officers"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        let count = ctx.session.as_ref().map_or(0, |s| s.campaign.roster.len());
        if let Some(i) = self.detail {
            let step = match ctx.input.nav() {
                Some(Dir::Left | Dir::Up) => -1,
                Some(Dir::Right | Dir::Down) => 1,
                _ => 0,
            };
            if step != 0 && count > 1 {
                let next = (i as i32 + step).rem_euclid(count as i32) as usize;
                self.detail = Some(next);
                self.menu.set_cursor(next);
                ctx.sfx(sfx::CURSOR);
            } else if ctx.input.cancel() || ctx.input.confirm() {
                ctx.input.consume();
                ctx.sfx(sfx::CANCEL);
                self.detail = None;
            }
            return Transition::None;
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) if i < count => self.detail = Some(i),
            MenuEvent::Cancelled => return Transition::Pop,
            _ => {}
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        draw_camp_backdrop(ctx, 0.8);
        match self
            .detail
            .and_then(|i| campaign.roster.get(i).map(|o| (i, o)))
        {
            Some((i, o)) => {
                draw_header(
                    ctx,
                    &format!("무장 정보 — {}", officer_name(pack, &o.id)),
                    campaign.gold,
                );
                self.draw_detail(ctx, pack, o, i, campaign.roster.len());
                draw_help(ctx, "←→ 다른 무장 · X 목록으로");
            }
            None => {
                draw_header(ctx, "무장 정보", campaign.gold);
                self.draw_table(ctx, pack, &campaign.roster);
                draw_help(ctx, "Z 자세히 · X 돌아가기");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;
    use hero_core::campaign::CampaignState;

    #[test]
    fn strategies_follow_class_and_level() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        let liu = campaign.officer("liu_bei").unwrap().clone();
        let known = strategy_list(&pack, &liu);
        assert_eq!(
            known.len(),
            pack.known_strategies(&liu.class, liu.level).len()
        );
        campaign.officer_mut("liu_bei").unwrap().level = 50;
        let later = strategy_list(&pack, campaign.officer("liu_bei").unwrap());
        assert!(later.len() >= known.len());
        assert!(later.iter().all(|(name, mp)| !name.is_empty() && *mp >= 0));
    }
}
