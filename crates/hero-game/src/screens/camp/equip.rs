//! 장비: pick an officer, a slot (무기 / 병법서 / 보물), then an item from the inventory — or take
//! the equipped one off. Changes go through `CampaignState::equip` / `unequip`; the stats panel
//! previews ATK / DEF / movement before → after with the battle engine's formulas.

use super::stats::{officer_stats, preview_change, EquipChange};
use super::widgets::{back_tapped, draw_back_button, LIST_TOP};
use super::widgets::{
    class_name, draw_camp_backdrop, draw_caption, draw_header, draw_help, draw_help_colored,
    draw_list_frame, draw_officer_sprite, draw_stats_block, item_effect, item_icon, officer_name,
    portrait_key, slot_icon, slot_name, visible_rows, TOP,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{Align, FontId, TextStyle};
use crate::ui::art::draw_portrait_card;
use crate::ui::korean::{with_particle, Particle};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_divider, draw_icon, draw_window_ex, WindowStyle};
use hero_core::campaign::{CampaignError, CampaignState};
use hero_core::data::{Id, ItemKind};
use hero_core::pack::Pack;
use macroquad::prelude::*;

const SLOTS: [ItemKind; 3] = [ItemKind::Weapon, ItemKind::Armor, ItemKind::Accessory];

const LIST: Rect = Rect {
    x: 8.0,
    y: TOP + 2.0,
    w: 150.0,
    h: 222.0,
};
const ROW_H: f32 = 24.0;
const PANEL: Rect = Rect {
    x: 164.0,
    y: TOP + 2.0,
    w: 308.0,
    h: 222.0,
};
/// Item picker window, over the slot rows (portrait and stats stay visible for the preview).
const PICKER: Rect = Rect {
    x: PANEL.x + 4.0,
    y: PANEL.y + 104.0,
    w: PANEL.w - 8.0,
    h: PANEL.h - 106.0,
};

/// One row of the item picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Take the equipped item off.
    Unequip,
    /// Equip this inventory item; `Err` holds why the officer cannot.
    Item {
        id: Id,
        allowed: Result<(), CampaignError>,
    },
}

/// Candidates for `slot` of `officer`: "take off" (when something is equipped), then every
/// inventory item of the slot's kind, strongest first. Eligibility comes from a dry run of
/// `CampaignState::equip`.
pub fn choices(
    pack: &Pack,
    campaign: &CampaignState,
    officer: &str,
    slot: ItemKind,
) -> Vec<Choice> {
    let mut out = Vec::new();
    let equipped = campaign.officer(officer).and_then(|o| match slot {
        ItemKind::Weapon => o.equip.weapon.clone(),
        ItemKind::Armor => o.equip.armor.clone(),
        ItemKind::Accessory => o.equip.accessory.clone(),
        ItemKind::Consumable => None,
    });
    if equipped.is_some() {
        out.push(Choice::Unequip);
    }
    let mut items: Vec<_> = campaign
        .inventory
        .iter()
        .filter(|(_, n)| **n > 0)
        .filter_map(|(id, _)| pack.item(id))
        .filter(|item| item.kind == slot)
        .collect();
    let strength = |i: &hero_core::data::ItemDef| {
        (
            i.atk_pct + i.def_pct,
            i.move_bonus,
            i.regen_hp + i.regen_morale,
        )
    };
    items.sort_by(|a, b| strength(b).cmp(&strength(a)).then(a.name.cmp(&b.name)));
    for item in items {
        let allowed = campaign.clone().equip(pack, officer, &item.id);
        out.push(Choice::Item {
            id: item.id.clone(),
            allowed,
        });
    }
    out
}

/// The item equipped in `slot` of `officer`.
fn equipped<'a>(campaign: &'a CampaignState, officer: &str, slot: ItemKind) -> Option<&'a Id> {
    let o = campaign.officer(officer)?;
    match slot {
        ItemKind::Weapon => o.equip.weapon.as_ref(),
        ItemKind::Armor => o.equip.armor.as_ref(),
        ItemKind::Accessory => o.equip.accessory.as_ref(),
        ItemKind::Consumable => None,
    }
}

enum Focus {
    Officers,
    Slots,
    Items {
        slot: ItemKind,
        choices: Vec<Choice>,
        menu: Menu,
    },
}

/// The equipment screen.
pub struct EquipScreen {
    officers: Menu,
    slots: Menu,
    focus: Focus,
}

impl Default for EquipScreen {
    fn default() -> Self {
        EquipScreen::new()
    }
}

impl EquipScreen {
    pub fn new() -> EquipScreen {
        EquipScreen {
            officers: Menu::new(Vec::new()),
            slots: Menu::new(Vec::new()),
            focus: Focus::Officers,
        }
    }

    fn selected_officer(&self, campaign: &CampaignState) -> Option<Id> {
        campaign
            .roster
            .get(self.officers.cursor())
            .map(|o| o.id.clone())
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let items = campaign
            .roster
            .iter()
            .map(|o| MenuItem::new(officer_name(pack, &o.id)).detail(format!("Lv{}", o.level)))
            .collect();
        let cursor = self.officers.cursor();
        let rows = ((LIST.h - LIST_TOP - 4.0) / ROW_H).floor() as usize;
        let mut menu =
            Menu::new(items)
                .rows(rows)
                .at(LIST.x + 2.0, LIST.y + LIST_TOP, LIST.w - 4.0);
        menu.framed = false;
        menu.row_height = ROW_H;
        menu.tag_width = 28.0;
        menu.wrap = false;
        menu.set_cursor(cursor);
        menu.active = matches!(self.focus, Focus::Officers);
        self.officers = menu;
        self.rebuild_slots(ctx);
    }

    fn rebuild_slots(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let officer = self.selected_officer(campaign).unwrap_or_default();
        let items = SLOTS
            .iter()
            .map(|&slot| {
                let item = equipped(campaign, &officer, slot).and_then(|id| pack.item(id));
                MenuItem::new(item.map_or("—".to_string(), |i| i.name.clone()))
                    .tag(slot_name(slot))
                    .detail(item.map_or(String::new(), |i| item_effect(pack, i)))
            })
            .collect();
        let cursor = self.slots.cursor();
        let mut menu = Menu::new(items).at(PANEL.x + 4.0, PANEL.y + 128.0, PANEL.w - 8.0);
        menu.framed = false;
        menu.row_height = 18.0;
        menu.tag_width = 64.0;
        menu.set_cursor(cursor);
        menu.active = matches!(self.focus, Focus::Slots);
        self.slots = menu;
    }

    fn open_picker(&mut self, ctx: &mut Ctx, slot: ItemKind) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let Some(officer) = self.selected_officer(campaign) else {
            return;
        };
        let choices = choices(&pack, campaign, &officer, slot);
        if choices.is_empty() {
            ctx.sfx(sfx::ERROR);
            ctx.toast(format!(
                "장비할 수 있는 {} 없습니다.",
                with_particle(slot_name(slot), Particle::IGa)
            ));
            return;
        }
        let items = choices
            .iter()
            .map(|c| match c {
                Choice::Unequip => MenuItem::new("장비 해제"),
                Choice::Item { id, allowed } => {
                    let item = pack.item(id);
                    let name = item.map_or(id.as_str(), |i| i.name.as_str());
                    let count = campaign.item_count(id);
                    let row = MenuItem::new(if count > 1 {
                        format!("{name} ×{count}")
                    } else {
                        name.to_string()
                    });
                    match allowed {
                        Ok(()) => row.detail(item.map_or(String::new(), |i| item_effect(&pack, i))),
                        Err(_) => row.detail("장비 불가").enabled(false),
                    }
                }
            })
            .collect();
        let rows = ((PICKER.h - LIST_TOP - 4.0) / 18.0).floor() as usize;
        let mut menu =
            Menu::new(items)
                .rows(rows)
                .at(PICKER.x + 2.0, PICKER.y + LIST_TOP, PICKER.w - 4.0);
        menu.framed = false;
        menu.row_height = 18.0;
        menu.tag_width = 18.0;
        menu.wrap = false;
        self.focus = Focus::Items {
            slot,
            choices,
            menu,
        };
    }

    fn apply(&mut self, ctx: &mut Ctx, slot: ItemKind, choice: &Choice) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_mut()) else {
            return;
        };
        let campaign = &mut session.campaign;
        let Some(officer) = self.selected_officer(campaign) else {
            return;
        };
        let name = officer_name(&pack, &officer).to_string();
        let result = match choice {
            Choice::Unequip => campaign.unequip(&officer, slot).map(|()| {
                format!(
                    "{}의 {} 해제했습니다.",
                    name,
                    with_particle(slot_name(slot), Particle::EulReul)
                )
            }),
            Choice::Item { id, .. } => campaign.equip(&pack, &officer, id).map(|()| {
                let item = pack.item(id).map_or(id.as_str(), |i| i.name.as_str());
                format!(
                    "{} {} 장비했습니다.",
                    with_particle(&name, Particle::IGa),
                    with_particle(item, Particle::EulReul)
                )
            }),
        };
        match result {
            Ok(msg) => {
                ctx.sfx(sfx::CONFIRM);
                ctx.toast(msg);
            }
            Err(e) => {
                ctx.sfx(sfx::ERROR);
                ctx.toast(equip_error(&pack, &e));
            }
        }
        self.focus = Focus::Slots;
        self.rebuild(ctx);
    }
}

/// Player-facing text for an equipment error.
pub fn equip_error(pack: &Pack, e: &CampaignError) -> String {
    match e {
        CampaignError::CannotEquip { item, .. } => {
            let item = pack.item(item).map_or(item.as_str(), |i| i.name.as_str());
            format!(
                "{} 이 무장의 병과로는 장비할 수 없습니다.",
                with_particle(item, Particle::EunNeun)
            )
        }
        other => super::shop::error_message(pack, other),
    }
}

impl Screen for EquipScreen {
    fn name(&self) -> &'static str {
        "camp-equip"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // The 돌아가기 button steps back one level, like cancel.
        if back_tapped(ctx) {
            self.focus = match self.focus {
                Focus::Officers => return Transition::Pop,
                Focus::Slots => Focus::Officers,
                Focus::Items { .. } => Focus::Slots,
            };
            self.rebuild(ctx);
            return Transition::None;
        }
        match std::mem::replace(&mut self.focus, Focus::Officers) {
            Focus::Officers => match self.officers.update(ctx) {
                MenuEvent::Selected(_) => {
                    self.focus = Focus::Slots;
                    self.rebuild(ctx);
                }
                MenuEvent::Moved(_) => self.rebuild_slots(ctx),
                MenuEvent::Cancelled => return Transition::Pop,
                _ => {}
            },
            Focus::Slots => {
                self.focus = Focus::Slots;
                match self.slots.update(ctx) {
                    MenuEvent::Selected(i) => self.open_picker(ctx, SLOTS[i]),
                    MenuEvent::Cancelled => {
                        self.focus = Focus::Officers;
                        self.rebuild(ctx);
                    }
                    _ => {}
                }
            }
            Focus::Items {
                slot,
                choices,
                mut menu,
            } => match menu.update(ctx) {
                MenuEvent::Selected(i) => {
                    let choice = choices[i].clone();
                    self.apply(ctx, slot, &choice);
                }
                MenuEvent::Cancelled => {
                    self.focus = Focus::Slots;
                    self.rebuild(ctx);
                }
                _ => {
                    self.focus = Focus::Items {
                        slot,
                        choices,
                        menu,
                    }
                }
            },
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
        draw_header(ctx, "장비", campaign.gold);

        // Officer list.
        draw_list_frame(ctx, LIST, "무장", matches!(self.focus, Focus::Officers));
        self.officers.draw(ctx);
        for (i, row) in visible_rows(&self.officers) {
            if let Some(o) = campaign.roster.get(i) {
                draw_officer_sprite(
                    ctx,
                    pack,
                    o,
                    vec2(row.x + 26.0, row.bottom() - 1.0),
                    i == self.officers.cursor(),
                );
            }
        }

        // Officer panel.
        draw_window_ex(PANEL, WindowStyle::Panel, 1.0);
        let Some(officer) = campaign.roster.get(self.officers.cursor()) else {
            draw_help(ctx, "X 돌아가기");
            draw_back_button(ctx);
            return;
        };
        let x = PANEL.x + 8.0;
        draw_portrait_card(
            ctx,
            Some(portrait_key(pack, &officer.id)),
            Rect::new(x, PANEL.y + 8.0, 64.0, 80.0),
            1.0,
            1.0,
        );
        let tx = x + 74.0;
        gfx.text(
            officer_name(pack, &officer.id),
            tx,
            PANEL.y + 6.0,
            TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
        );
        gfx.text_aligned(
            &format!("{} Lv{}", class_name(pack, &officer.class), officer.level),
            tx,
            PANEL.y + 6.0,
            PANEL.right() - 10.0 - tx,
            Align::Right,
            TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
        // Stats, compared with the highlighted choice.
        let preview = match &self.focus {
            Focus::Items {
                slot,
                choices,
                menu,
            } => choices.get(menu.cursor()).and_then(|c| {
                let change = match c {
                    Choice::Unequip => EquipChange::Unequip(*slot),
                    Choice::Item {
                        id,
                        allowed: Ok(()),
                    } => EquipChange::Equip(id.clone()),
                    Choice::Item { .. } => return None,
                };
                preview_change(pack, campaign, &officer.id, &change).ok()
            }),
            _ => None,
        };
        if let Some(before) = preview
            .map(|(b, _)| b)
            .or_else(|| officer_stats(pack, officer))
        {
            draw_stats_block(
                gfx,
                &before,
                preview.as_ref().map(|(_, a)| a),
                tx,
                PANEL.y + 26.0,
                PANEL.right() - 10.0 - tx,
            );
        }
        draw_divider(x, PANEL.y + 110.0, PANEL.w - 16.0);
        draw_caption(gfx, "장비", x, PANEL.y + 114.0);
        self.slots.draw(ctx);
        for (i, row) in visible_rows(&self.slots) {
            let slot = SLOTS[i];
            let icon = equipped(campaign, &officer.id, slot)
                .and_then(|id| pack.item(id))
                .map_or(slot_icon(slot), item_icon);
            draw_icon(ctx, icon, vec2(row.x + 12.0 + 42.0, row.y + 1.0));
        }

        // Description of the highlighted equipment or candidate.
        let described = match &self.focus {
            Focus::Items { choices, menu, .. } => match choices.get(menu.cursor()) {
                Some(Choice::Item { id, allowed }) => Some((id.clone(), allowed.clone())),
                _ => None,
            },
            _ => equipped(campaign, &officer.id, SLOTS[self.slots.cursor().min(2)])
                .map(|id| (id.clone(), Ok(()))),
        };
        let desc_y = PANEL.y + 190.0;
        draw_divider(x, desc_y - 4.0, PANEL.w - 16.0);
        if let Some((id, allowed)) = described {
            if let Some(item) = pack.item(&id) {
                let (text, color) = match &allowed {
                    Ok(()) => (item.desc.clone(), theme::TEXT),
                    Err(e) => (equip_error(pack, e), theme::TEXT_BAD),
                };
                let lines = gfx.wrap(&text, FontId::Small, 1, PANEL.w - 16.0);
                gfx.text_lines(
                    &lines[..lines.len().min(2)],
                    x,
                    desc_y,
                    TextStyle::small(color),
                );
            }
        }

        // Item picker.
        if let Focus::Items {
            slot,
            choices,
            menu,
        } = &self.focus
        {
            draw_list_frame(ctx, PICKER, &format!("{} 고르기", slot_name(*slot)), true);
            menu.draw(ctx);
            for (i, row) in visible_rows(menu) {
                let icon = match &choices[i] {
                    Choice::Unequip => slot_icon(*slot),
                    Choice::Item { id, .. } => pack.item(id).map_or(slot_icon(*slot), item_icon),
                };
                draw_icon(ctx, icon, vec2(row.x + 12.0, row.y + 1.0));
            }
            // The picker covers the description: the help line explains the candidate instead.
            let (text, color) = match choices.get(menu.cursor()) {
                Some(Choice::Item { id, allowed }) => match (allowed, pack.item(id)) {
                    (Err(e), _) => (equip_error(pack, e), theme::TEXT_BAD),
                    (Ok(()), Some(item)) => (item.desc.clone(), theme::TEXT_DIM),
                    (Ok(()), None) => (String::new(), theme::TEXT_DIM),
                },
                _ => (
                    "장비를 벗어 보관합니다. · Z 결정 · X 취소".to_string(),
                    theme::TEXT_DIM,
                ),
            };
            draw_help_colored(ctx, &text, color);
            draw_back_button(ctx);
            return;
        }
        draw_help(
            ctx,
            match self.focus {
                Focus::Officers => "Z 무장 선택 · X 돌아가기",
                _ => "Z 장비 바꾸기 · X 무장 목록",
            },
        );
        draw_back_button(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    #[test]
    fn candidates_are_sorted_and_checked_by_the_campaign() {
        let mut pack = test_pack();
        // Make one weapon infantry-only so the cavalry officers cannot use it.
        pack.items.get_mut("seven_star_sword").unwrap().families = vec!["infantry".into()];
        let mut campaign = CampaignState::new_game(&pack);
        campaign.add_item("seven_star_sword", 2);
        campaign.add_item("hero_sword", 1);
        campaign.add_item("bean", 3);

        let c = choices(&pack, &campaign, "guan_yu", ItemKind::Weapon);
        assert_eq!(c[0], Choice::Unequip);
        assert!(matches!(&c[1], Choice::Item { id, allowed: Ok(()) } if id == "hero_sword"));
        assert!(matches!(
            &c[2],
            Choice::Item { id, allowed: Err(CampaignError::CannotEquip { .. }) } if id == "seven_star_sword"
        ));
        assert_eq!(c.len(), 3);

        // Liu Bei (infantry) may use the sword; nothing to take off.
        let c = choices(&pack, &campaign, "liu_bei", ItemKind::Weapon);
        assert!(c.iter().all(|c| !matches!(c, Choice::Unequip)));
        assert!(c.iter().all(|c| matches!(
            c,
            Choice::Item {
                allowed: Ok(()),
                ..
            }
        )));
        assert!(choices(&pack, &campaign, "liu_bei", ItemKind::Armor).is_empty());

        let e = campaign
            .clone()
            .equip(&pack, "guan_yu", "seven_star_sword")
            .unwrap_err();
        assert!(equip_error(&pack, &e).starts_with("칠성검은"));
    }
}
