//! DOS manual pp. 22–23: choose two officers, click an item to transfer it.
use super::widgets::{
    back_tapped, draw_back_button, draw_camp_backdrop, officer_name, portrait_key,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::TextStyle;
use crate::ui::art::draw_portrait_card;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::draw_window;
use hero_core::data::Id;
use macroquad::prelude::*;

/// Original warrior icon opens a command menu before either officer screen.
pub struct OfficerMenu {
    menu: Menu,
}
impl OfficerMenu {
    pub fn new() -> Self {
        Self {
            menu: Menu::new(vec![MenuItem::new("武将情报"), MenuItem::new("交换道具")])
                .at(375.0, 224.0, 112.0),
        }
    }
}
impl Screen for OfficerMenu {
    fn name(&self) -> &'static str {
        "officer-menu"
    }
    fn in_camp_frame(&self) -> bool {
        true
    }
    fn is_overlay(&self) -> bool {
        true
    }
    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Selected(0) => Transition::push(super::officers::OfficersScreen::new()),
            MenuEvent::Selected(1) => Transition::push(ExchangeScreen::new()),
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }
    fn draw(&self, ctx: &Ctx) {
        self.menu.draw(ctx);
    }
}

pub struct ExchangeScreen {
    officers: [Option<Id>; 2],
    picker: Option<(usize, Vec<Id>, Menu)>,
    stock: Option<(usize, Vec<Id>, Menu)>,
    selected: Option<(usize, usize)>,
}

impl ExchangeScreen {
    pub fn new() -> Self {
        Self {
            officers: [None, None],
            picker: None,
            stock: None,
            selected: None,
        }
    }
    fn origin(ctx: &Ctx, side: usize) -> Vec2 {
        vec2(16.0 + side as f32 * (ctx.gfx.size().x / 2.0), 114.0)
    }
    fn choose_rect(ctx: &Ctx, side: usize) -> Rect {
        let p = Self::origin(ctx, side);
        Rect::new(p.x, p.y, 84.0, 22.0)
    }
    fn item_rect(ctx: &Ctx, side: usize, slot: usize) -> Rect {
        let p = Self::origin(ctx, side);
        Rect::new(
            p.x + 90.0,
            p.y + 24.0 + slot as f32 * 17.0,
            ctx.gfx.size().x / 2.0 - 114.0,
            17.0,
        )
    }
    fn stock_rect(ctx: &Ctx, side: usize) -> Rect {
        let p = Self::origin(ctx, side);
        Rect::new(p.x + 90.0, p.y, ctx.gfx.size().x / 2.0 - 114.0, 22.0)
    }
    fn choose(&mut self, ctx: &Ctx, side: usize) {
        let (Some(pack), Some(session)) = (&ctx.pack, &ctx.session) else {
            return;
        };
        let ids: Vec<Id> = session
            .campaign
            .roster
            .iter()
            .filter(|o| !o.away && o.equip.carried.is_some())
            .filter(|o| self.officers[1 - side].as_deref() != Some(o.id.as_str()))
            .map(|o| o.id.clone())
            .collect();
        let menu = Menu::new(
            ids.iter()
                .map(|id| MenuItem::new(officer_name(pack, id)))
                .collect(),
        )
        .at(Self::origin(ctx, side).x, 140.0, 90.0)
        .rows(6);
        self.picker = Some((side, ids, menu));
    }
}

impl Screen for ExchangeScreen {
    fn name(&self) -> &'static str {
        "exchange"
    }
    fn in_camp_frame(&self) -> bool {
        true
    }
    fn is_overlay(&self) -> bool {
        true
    }
    fn on_enter(&mut self, ctx: &mut Ctx, _: Enter) {
        if let Some(session) = &ctx.session {
            for (side, o) in session
                .campaign
                .roster
                .iter()
                .filter(|o| !o.away && o.equip.carried.is_some())
                .take(2)
                .enumerate()
            {
                self.officers[side] = Some(o.id.clone());
            }
        }
    }
    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some((side, ids, menu)) = &mut self.stock {
            match menu.update(ctx) {
                MenuEvent::Selected(i) => {
                    if let (Some(id), Some(session)) = (&self.officers[*side], ctx.session.as_mut())
                    {
                        let result = session.campaign.withdraw_item(id, &ids[i]);
                        match result {
                            Ok(()) => ctx.sfx(sfx::CONFIRM),
                            Err(e) => ctx.toast(&e.to_string()),
                        }
                    }
                    self.stock = None;
                }
                MenuEvent::Cancelled => self.stock = None,
                _ => {}
            }
            return Transition::None;
        }
        if let Some((side, ids, menu)) = &mut self.picker {
            match menu.update(ctx) {
                MenuEvent::Selected(i) => {
                    self.officers[*side] = Some(ids[i].clone());
                    self.picker = None;
                }
                MenuEvent::Cancelled => self.picker = None,
                _ => {}
            }
            return Transition::None;
        }
        if ctx.input.cancel() || back_tapped(ctx) {
            if self.selected.take().is_some() {
                return Transition::None;
            }
            return Transition::Pop;
        }
        for side in 0..2 {
            if ctx.input.tapped(Self::stock_rect(ctx, side)) {
                self.selected = None;
                if let (Some(pack), Some(session)) = (&ctx.pack, &ctx.session) {
                    let ids: Vec<Id> = session
                        .campaign
                        .inventory
                        .iter()
                        .filter(|(_, n)| **n > 0)
                        .map(|(id, _)| id.clone())
                        .collect();
                    if ids.is_empty() {
                        ctx.toast("公用背包没有道具。");
                    } else {
                        let menu = Menu::new(
                            ids.iter()
                                .map(|id| {
                                    MenuItem::new(
                                        pack.item(id).map_or(id.as_str(), |i| i.name.as_str()),
                                    )
                                    .detail(format!("×{}", session.campaign.item_count(id)))
                                })
                                .collect(),
                        )
                        .at(Self::origin(ctx, side).x, 140.0, 150.0)
                        .rows(6);
                        self.stock = Some((side, ids, menu));
                    }
                }
                ctx.input.consume();
                return Transition::None;
            }
            if ctx.input.tapped(Self::choose_rect(ctx, side)) {
                self.selected = None;
                self.choose(ctx, side);
                ctx.input.consume();
                return Transition::None;
            }
            for slot in 0..8 {
                if ctx.input.tapped(Self::item_rect(ctx, side, slot)) {
                    if let Some((source_side, source_slot)) = self.selected {
                        if source_side == side {
                            self.selected = None;
                        } else if let (Some(from), Some(to), Some(session)) = (
                            &self.officers[source_side],
                            &self.officers[side],
                            ctx.session.as_mut(),
                        ) {
                            match session.campaign.exchange_items(from, source_slot, to, slot) {
                                Ok(()) => {
                                    ctx.sfx(sfx::CONFIRM);
                                    self.selected = None;
                                }
                                Err(e) => {
                                    ctx.sfx(sfx::ERROR);
                                    ctx.toast(&e.to_string());
                                }
                            }
                        }
                    } else {
                        let occupied = ctx
                            .session
                            .as_ref()
                            .and_then(|session| {
                                self.officers[side]
                                    .as_ref()
                                    .and_then(|id| session.campaign.officer(id))
                            })
                            .and_then(|o| o.equip.carried.as_ref())
                            .is_some_and(|p| p.slots()[slot].is_some());
                        if occupied {
                            self.selected = Some((side, slot));
                        }
                    }
                    ctx.input.consume();
                    return Transition::None;
                }
            }
        }
        Transition::None
    }
    fn draw(&self, ctx: &Ctx) {
        draw_camp_backdrop(ctx, 0.0);
        let (Some(pack), Some(session)) = (&ctx.pack, &ctx.session) else {
            return;
        };
        let text = TextStyle::main(theme::TEXT);
        draw_window(Rect::new(12.0, 12.0, ctx.gfx.size().x - 24.0, 78.0));
        ctx.gfx.text("交换道具", 24.0, 24.0, text);
        ctx.gfx.text(
            if self.selected.is_some() {
                "点击对方空位转交，或点击道具互换。"
            } else {
                "先点道具，再点对方位置；右键取消。"
            },
            24.0,
            48.0,
            text,
        );
        for side in 0..2 {
            let p = Self::origin(ctx, side);
            let choose = Self::choose_rect(ctx, side);
            draw_window(choose);
            let stock = Self::stock_rect(ctx, side);
            draw_window(stock);
            ctx.gfx
                .text("公用道具", stock.x + 3.0, stock.y + 3.0, text);
            ctx.gfx.text(
                &format!("武将{}", side + 1),
                choose.x + 8.0,
                choose.y + 3.0,
                text,
            );
            if let Some(id) = &self.officers[side] {
                draw_portrait_card(
                    ctx,
                    Some(portrait_key(pack, id)),
                    Rect::new(p.x, p.y + 30.0, 80.0, 100.0),
                    1.0,
                    1.0,
                );
                ctx.gfx
                    .text(officer_name(pack, id), p.x + 8.0, p.y + 138.0, text);
                if let Some(pocket) = session
                    .campaign
                    .officer(id)
                    .and_then(|o| o.equip.carried.as_ref())
                {
                    let first = Self::item_rect(ctx, side, 0);
                    draw_window(Rect::new(
                        first.x - 3.0,
                        first.y - 3.0,
                        first.w + 6.0,
                        142.0,
                    ));
                    for (slot, item) in pocket.slots().iter().enumerate() {
                        let r = Self::item_rect(ctx, side, slot);
                        let style = if self.selected == Some((side, slot)) {
                            TextStyle::main(theme::TEXT_ACCENT)
                        } else {
                            text
                        };
                        let name = item
                            .as_ref()
                            .map(|id| pack.item(id).map_or(id.as_str(), |i| i.name.as_str()))
                            .unwrap_or("— 空位 —");
                        ctx.gfx.text(name, r.x + 2.0, r.y, style);
                    }
                }
            }
        }
        draw_back_button(ctx);
        if let Some((_, _, menu)) = &self.picker {
            menu.draw(ctx);
        }
        if let Some((_, _, menu)) = &self.stock {
            menu.draw(ctx);
        }
    }
}
