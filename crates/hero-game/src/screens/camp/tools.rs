//! 도구: use class-up (`promote`) and class-change items on officers in camp, through
//! `CampaignState::use_item`. Every officer is listed with whether the chosen item works on them
//! (a dry run of the same call decides) and, if not, why.

use super::widgets::LIST_TOP;
use super::widgets::{
    class_name, draw_camp_backdrop, draw_header, draw_help, draw_list_frame, draw_officer_sprite,
    item_effect, item_icon, officer_name, visible_rows, TOP,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, FontId, TextStyle, SCREEN, VIRTUAL_H, VIRTUAL_W};
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::korean::{with_particle, Particle};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_icon, draw_small_arrow, draw_window};
use hero_core::campaign::{CampaignError, CampaignState};
use hero_core::data::{Effect, Id};
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Inventory items that can be used in camp (class-up and class-change items).
pub fn camp_items(pack: &Pack, campaign: &CampaignState) -> Vec<Id> {
    campaign
        .inventory
        .iter()
        .filter(|(_, n)| **n > 0)
        .filter_map(|(id, _)| pack.item(id))
        .filter(|item| {
            item.effects
                .iter()
                .any(|e| matches!(e, Effect::Promote | Effect::ChangeClass { .. }))
        })
        .map(|item| item.id.clone())
        .collect()
}

/// Whether `item` works on `officer` now: the class it would become, or why not (in Korean).
pub fn usability(
    pack: &Pack,
    campaign: &CampaignState,
    officer: &str,
    item: &str,
) -> Result<Id, String> {
    let mut trial = campaign.clone();
    match trial.use_item(pack, officer, item) {
        Ok(()) => trial
            .officer(officer)
            .map(|o| o.class.clone())
            .ok_or_else(|| "아군에 없는 무장입니다.".to_string()),
        Err(e) => Err(reason(pack, campaign, officer, item, &e)),
    }
}

/// Explain a failed `use_item` by re-reading the same data the campaign checked.
fn reason(
    pack: &Pack,
    campaign: &CampaignState,
    officer: &str,
    item: &str,
    err: &CampaignError,
) -> String {
    let CampaignError::CannotUse { .. } = err else {
        return super::shop::error_message(pack, err);
    };
    let (Some(state), Some(def)) = (campaign.officer(officer), pack.item(item)) else {
        return err.to_string();
    };
    let class = class_name(pack, &state.class);
    for effect in &def.effects {
        match effect {
            Effect::Promote => {
                let Some(promotion) = pack.class(&state.class).and_then(|c| c.promote.as_ref())
                else {
                    return format!(
                        "{} 더 이상 승급할 수 없습니다.",
                        with_particle(class, Particle::EunNeun)
                    );
                };
                if promotion.item != item {
                    return format!("{} 승급하는 도구가 아닙니다.", class_to(class));
                }
                if state.level < promotion.level {
                    return format!(
                        "레벨 {} 이상이어야 합니다. (현재 {})",
                        promotion.level, state.level
                    );
                }
            }
            Effect::ChangeClass { to } => {
                if pack.officer(officer).is_some_and(|o| o.fixed_class) {
                    return "병과를 바꿀 수 없는 무장입니다.".to_string();
                }
                if state.class == *to {
                    return format!("이미 {}입니다.", class_name(pack, to));
                }
            }
            _ => {}
        }
    }
    err.to_string()
}

/// `단병을` → used as "단병을 승급하는 ..." (object particle).
fn class_to(class: &str) -> String {
    with_particle(class, Particle::EulReul)
}

const ITEMS: Rect = Rect {
    x: 8.0,
    y: TOP + 2.0,
    w: 196.0,
    h: 222.0,
};
const OFFICERS: Rect = Rect {
    x: 210.0,
    y: TOP + 2.0,
    w: 262.0,
    h: 222.0,
};
const ROW_H: f32 = 24.0;

/// Result shown after using an item.
struct Outcome {
    title: String,
    lines: Vec<String>,
}

enum Popup {
    None,
    Confirm {
        officer: Id,
        item: Id,
        dialog: ConfirmDialog,
    },
    Outcome(Outcome),
}

/// The 도구 screen.
pub struct ToolsScreen {
    items: Vec<Id>,
    item_menu: Menu,
    officer_menu: Menu,
    choosing_officer: bool,
    popup: Popup,
}

impl Default for ToolsScreen {
    fn default() -> Self {
        ToolsScreen::new()
    }
}

impl ToolsScreen {
    pub fn new() -> ToolsScreen {
        ToolsScreen {
            items: Vec::new(),
            item_menu: Menu::new(Vec::new()),
            officer_menu: Menu::new(Vec::new()),
            choosing_officer: false,
            popup: Popup::None,
        }
    }

    fn selected_item(&self) -> Option<&Id> {
        self.items.get(self.item_menu.cursor())
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        self.items = camp_items(pack, campaign);
        if self.items.is_empty() {
            self.choosing_officer = false;
        }
        let items = self
            .items
            .iter()
            .filter_map(|id| pack.item(id))
            .map(|item| {
                MenuItem::new(&item.name).detail(format!("×{}", campaign.item_count(&item.id)))
            })
            .collect();
        let cursor = self.item_menu.cursor();
        let rows = ((ITEMS.h - LIST_TOP - 4.0) / 18.0).floor() as usize;
        let mut menu =
            Menu::new(items)
                .rows(rows)
                .at(ITEMS.x + 2.0, ITEMS.y + LIST_TOP, ITEMS.w - 4.0);
        menu.framed = false;
        menu.row_height = 18.0;
        menu.tag_width = 18.0;
        menu.wrap = false;
        menu.set_cursor(cursor);
        menu.active = !self.choosing_officer;
        self.item_menu = menu;
        self.rebuild_officers(ctx);
    }

    fn rebuild_officers(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let item = self.selected_item().cloned();
        let items = campaign
            .roster
            .iter()
            .map(|o| {
                let usable = item
                    .as_deref()
                    .is_some_and(|i| usability(pack, campaign, &o.id, i).is_ok());
                MenuItem::new(officer_name(pack, &o.id))
                    .detail(format!("Lv{}", o.level))
                    .enabled(usable)
            })
            .collect();
        let cursor = self.officer_menu.cursor();
        let rows = ((OFFICERS.h - LIST_TOP - 4.0) / ROW_H).floor() as usize;
        let mut menu = Menu::new(items).rows(rows).at(
            OFFICERS.x + 2.0,
            OFFICERS.y + LIST_TOP,
            OFFICERS.w - 4.0,
        );
        menu.framed = false;
        menu.row_height = ROW_H;
        menu.tag_width = 28.0;
        menu.wrap = false;
        // Keep the cursor on the same officer while it can still be chosen; otherwise the menu
        // starts on the first officer the item works on.
        if menu.items.get(cursor).is_some_and(|it| it.enabled) {
            menu.set_cursor(cursor);
        }
        menu.active = self.choosing_officer;
        self.officer_menu = menu;
    }

    fn confirm(&mut self, ctx: &mut Ctx, index: usize) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let (Some(officer), Some(item)) = (campaign.roster.get(index), self.selected_item()) else {
            return;
        };
        match usability(&pack, campaign, &officer.id, item) {
            Ok(to) => {
                let name = officer_name(&pack, &officer.id);
                let item_name = pack.item(item).map_or(item.as_str(), |i| i.name.as_str());
                let text = format!(
                    "{}에게 {} 사용할까요?\n{} → {}",
                    name,
                    with_particle(item_name, Particle::EulReul),
                    class_name(&pack, &officer.class),
                    class_name(&pack, &to)
                );
                self.popup = Popup::Confirm {
                    officer: officer.id.clone(),
                    item: item.clone(),
                    dialog: ConfirmDialog::new(&ctx.gfx, &text).labels("사용", "취소"),
                };
            }
            Err(why) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(why);
            }
        }
    }

    fn use_item(&mut self, ctx: &mut Ctx, officer: &str, item: &str) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_mut()) else {
            return;
        };
        let campaign = &mut session.campaign;
        let Some(before) = campaign.officer(officer).cloned() else {
            return;
        };
        match campaign.use_item(&pack, officer, item) {
            Ok(()) => {
                let after = campaign.officer(officer).cloned().unwrap_or(before.clone());
                let name = officer_name(&pack, officer);
                let mut lines = vec![format!(
                    "{} → {}",
                    class_name(&pack, &before.class),
                    class_name(&pack, &after.class)
                )];
                let returned: Vec<&str> = before
                    .equip
                    .iter()
                    .filter(|id| !after.equip.iter().any(|a| a == *id))
                    .map(|id| pack.item(id).map_or(id.as_str(), |i| i.name.as_str()))
                    .collect();
                if !returned.is_empty() {
                    lines.push(format!(
                        "새 병과로 쓸 수 없는 {} 보관함으로 돌아갔습니다.",
                        with_particle(&returned.join(", "), Particle::EunNeun)
                    ));
                }
                ctx.sfx(sfx::LEVELUP);
                self.popup = Popup::Outcome(Outcome {
                    title: format!(
                        "{} {} 되었습니다!",
                        with_particle(name, Particle::IGa),
                        with_particle(class_name(&pack, &after.class), Particle::IGa)
                    ),
                    lines,
                });
            }
            Err(e) => {
                let msg = reason(&pack, campaign, officer, item, &e);
                ctx.sfx(sfx::ERROR);
                ctx.toast(msg);
            }
        }
        self.rebuild(ctx);
    }
}

impl Screen for ToolsScreen {
    fn name(&self) -> &'static str {
        "camp-tools"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::None => {}
            Popup::Confirm {
                officer,
                item,
                mut dialog,
            } => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => self.use_item(ctx, &officer, &item),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => {
                        self.popup = Popup::Confirm {
                            officer,
                            item,
                            dialog,
                        }
                    }
                }
                return Transition::None;
            }
            Popup::Outcome(outcome) => {
                if ctx.input.confirm() || ctx.input.cancel() {
                    ctx.input.consume();
                    ctx.sfx(sfx::CONFIRM);
                    // The item was used up or changed places: pick an item again.
                    self.choosing_officer = false;
                    self.rebuild(ctx);
                } else {
                    self.popup = Popup::Outcome(outcome);
                }
                return Transition::None;
            }
        }
        if self.choosing_officer {
            match self.officer_menu.update(ctx) {
                MenuEvent::Selected(i) => self.confirm(ctx, i),
                MenuEvent::Cancelled => {
                    self.choosing_officer = false;
                    self.rebuild(ctx);
                }
                _ => {}
            }
            return Transition::None;
        }
        if self.items.is_empty() {
            if ctx.input.cancel() || ctx.input.confirm() {
                ctx.sfx(sfx::CANCEL);
                return Transition::Pop;
            }
            return Transition::None;
        }
        match self.item_menu.update(ctx) {
            MenuEvent::Selected(_) => {
                if self.officer_menu.items.iter().any(|it| it.enabled) {
                    self.choosing_officer = true;
                    self.rebuild(ctx);
                } else {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast("지금 이 도구를 쓸 수 있는 무장이 없습니다.");
                }
            }
            MenuEvent::Moved(_) => self.rebuild_officers(ctx),
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
        let gfx = &ctx.gfx;
        draw_camp_backdrop(ctx, 0.8);
        draw_header(ctx, "도구", campaign.gold);

        draw_list_frame(ctx, ITEMS, "병과 도구", !self.choosing_officer);
        if self.items.is_empty() {
            let lines = gfx.wrap(
                "승급이나 병과 변경에 쓰는 도구가 없습니다. 도구상에서 사거나 전투에서 얻을 수 있습니다.",
                FontId::Main,
                1,
                ITEMS.w - 20.0,
            );
            gfx.text_lines(
                &lines,
                ITEMS.x + 10.0,
                ITEMS.y + 30.0,
                TextStyle::main(theme::TEXT_DIM),
            );
        } else {
            self.item_menu.draw(ctx);
            for (i, row) in visible_rows(&self.item_menu) {
                if let Some(item) = self.items.get(i).and_then(|id| pack.item(id)) {
                    draw_icon(ctx, item_icon(item), vec2(row.x + 12.0, row.y + 1.0));
                }
            }
        }

        draw_list_frame(ctx, OFFICERS, "사용할 무장", self.choosing_officer);
        self.officer_menu.draw(ctx);
        let item = self.selected_item();
        for (i, row) in visible_rows(&self.officer_menu) {
            let Some(o) = campaign.roster.get(i) else {
                continue;
            };
            draw_officer_sprite(
                ctx,
                pack,
                o,
                vec2(row.x + 26.0, row.bottom() - 1.0),
                self.choosing_officer && i == self.officer_menu.cursor(),
            );
            let x = row.x + 12.0 + 28.0 + 46.0;
            let (text, color) = match item.map(|it| usability(pack, campaign, &o.id, it)) {
                Some(Ok(to)) => (
                    format!("{} → {}", class_name(pack, &o.class), class_name(pack, &to)),
                    theme::TEXT_GOOD,
                ),
                Some(Err(why)) => (why, theme::TEXT_DISABLED),
                None => (class_name(pack, &o.class).to_string(), theme::TEXT_DIM),
            };
            let max_w = row.right() - 40.0 - x;
            let lines = gfx.wrap(&text, FontId::Small, 1, max_w);
            let y = if lines.len() > 1 {
                row.y + 1.0
            } else {
                row.y + 6.0
            };
            gfx.text_lines(&lines[..lines.len().min(2)], x, y, TextStyle::small(color));
        }

        let help = match item.and_then(|id| pack.item(id)) {
            Some(it) if !self.choosing_officer => {
                format!("{} — {}", item_effect(pack, it), it.desc)
            }
            _ if self.choosing_officer => "Z 사용 · X 도구 목록".to_string(),
            _ => "X 돌아가기".to_string(),
        };
        draw_help(ctx, &help);

        match &self.popup {
            Popup::None => {}
            Popup::Confirm { dialog, .. } => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.4));
                dialog.draw(ctx);
            }
            Popup::Outcome(outcome) => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.4));
                let w = 320.0;
                let wrapped: Vec<String> = outcome
                    .lines
                    .iter()
                    .flat_map(|l| gfx.wrap(l, FontId::Main, 1, w - 24.0))
                    .collect();
                let h = 32.0 + wrapped.len() as f32 * 16.0 + 12.0;
                let r = Rect::new(
                    ((VIRTUAL_W - w) / 2.0).round(),
                    ((VIRTUAL_H - h) / 2.0).round(),
                    w,
                    h,
                );
                draw_window(r);
                gfx.text_aligned(
                    &outcome.title,
                    r.x,
                    r.y + 8.0,
                    r.w,
                    Align::Center,
                    TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
                );
                for (i, line) in wrapped.iter().enumerate() {
                    gfx.text_aligned(
                        line,
                        r.x,
                        r.y + 30.0 + i as f32 * 16.0,
                        r.w,
                        Align::Center,
                        TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
                    );
                }
                if (ctx.time * 2.5).fract() < 0.6 {
                    draw_small_arrow(r.right() - 11.0, r.bottom() - 9.0, true, theme::TEXT_ACCENT);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    #[test]
    fn class_items_and_reasons() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        campaign.add_item("horse_armor", 1);
        campaign.add_item("long_spear", 1);
        campaign.add_item("archery_guide", 1);
        campaign.add_item("bean", 2);
        let items = camp_items(&pack, &campaign);
        assert_eq!(items.len(), 3);
        assert!(!items.contains(&"bean".to_string()));

        // Level too low for the class-up item.
        let why = usability(&pack, &campaign, "guan_yu", "horse_armor").unwrap_err();
        assert_eq!(why, "레벨 15 이상이어야 합니다. (현재 1)");
        campaign.officer_mut("guan_yu").unwrap().level = 15;
        assert_eq!(
            usability(&pack, &campaign, "guan_yu", "horse_armor"),
            Ok("heavy_cavalry".to_string())
        );
        // Wrong item for the class.
        let why = usability(&pack, &campaign, "guan_yu", "long_spear").unwrap_err();
        assert_eq!(why, "경기병을 승급하는 도구가 아닙니다.");
        // The lord cannot change class; others can.
        let why = usability(&pack, &campaign, "liu_bei", "archery_guide").unwrap_err();
        assert_eq!(why, "병과를 바꿀 수 없는 무장입니다.");
        assert_eq!(
            usability(&pack, &campaign, "zhang_fei", "archery_guide"),
            Ok("archer".to_string())
        );
        // Not owned.
        let why = usability(&pack, &campaign, "zhang_fei", "war_cart").unwrap_err();
        assert!(why.contains("가지고 있지 않습니다"));
        // The dry runs changed nothing.
        assert_eq!(campaign.item_count("horse_armor"), 1);
        assert_eq!(campaign.officer("guan_yu").unwrap().class, "light_cavalry");
    }
}
