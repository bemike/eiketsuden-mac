//! 상점: buy the items the camp offers (`Node::Camp::shop`) and sell from the inventory at half
//! price. Every purchase and sale goes through `CampaignState::buy` / `sell`, whose errors are
//! shown as toasts.

use super::widgets::LIST_TOP;
use super::widgets::{
    draw_camp_backdrop, draw_caption, draw_header, draw_help, draw_list_frame, item_effect,
    item_icon, slot_name, visible_rows, QuantityDialog, TOP,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{Align, FontId, TextStyle};
use crate::ui::dialog::ConfirmEvent;
use crate::ui::format;
use crate::ui::korean::{with_particle, Particle};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{
    draw_divider, draw_highlight, draw_icon, draw_window_ex, inset, WindowStyle,
};
use hero_core::campaign::{CampaignError, CampaignState};
use hero_core::data::{Id, ItemDef};
use hero_core::pack::Pack;
use macroquad::prelude::*;

/// Most copies bought or sold in one go.
pub const MAX_QUANTITY: u32 = 99;

/// Sale price of one copy (`None`: the item cannot be sold).
pub fn sell_price(item: &ItemDef) -> Option<i64> {
    (item.price > 0).then(|| i64::from(item.price / 2))
}

/// How many copies of an item priced `price` the army can pay for (capped at [`MAX_QUANTITY`]).
pub fn max_affordable(gold: i64, price: u32) -> u32 {
    if price == 0 || gold <= 0 {
        return 0;
    }
    (gold / i64::from(price)).clamp(0, i64::from(MAX_QUANTITY)) as u32
}

/// Gold lost to the gold cap when selling for `income`.
pub fn lost_to_cap(gold: i64, income: i64, cap: i64) -> i64 {
    (gold.saturating_add(income) - cap.max(0)).max(0)
}

/// Player-facing text for a shop error.
pub fn error_message(pack: &Pack, e: &CampaignError) -> String {
    let name =
        |id: &str, p: Particle| with_particle(pack.item(id).map_or(id, |i| i.name.as_str()), p);
    match e {
        CampaignError::NotEnoughGold { need, have } => format!(
            "군자금이 부족합니다. (필요 {}, 소지 {})",
            format::thousands(*need),
            format::thousands(*have)
        ),
        CampaignError::CannotBuy(id) => {
            format!("{} 팔지 않는 물건입니다.", name(id, Particle::EunNeun))
        }
        CampaignError::CannotSell(id) => {
            format!("{} 팔 수 없는 물건입니다.", name(id, Particle::EunNeun))
        }
        CampaignError::NotOwned(id) => {
            format!("{} 가지고 있지 않습니다.", name(id, Particle::EulReul))
        }
        CampaignError::UnknownItem(id) => format!("알 수 없는 물건입니다: {id}"),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Buy,
    Sell,
}

/// Rows of each tab.
fn buy_rows(pack: &Pack, shop: &[Id]) -> Vec<Id> {
    shop.iter()
        .filter(|id| pack.item(id).is_some())
        .cloned()
        .collect()
}

fn sell_rows(pack: &Pack, campaign: &CampaignState) -> Vec<Id> {
    campaign
        .inventory
        .iter()
        .filter(|(id, n)| **n > 0 && pack.item(id).is_some())
        .map(|(id, _)| id.clone())
        .collect()
}

/// Copies owned: in the inventory and equipped by officers.
fn owned(campaign: &CampaignState, item: &str) -> (u32, u32) {
    let equipped = campaign
        .roster
        .iter()
        .flat_map(|o| o.equip.iter())
        .filter(|i| *i == item)
        .count() as u32;
    (campaign.item_count(item), equipped)
}

const LIST: Rect = Rect {
    x: 8.0,
    y: TOP + 20.0,
    w: 250.0,
    h: 204.0,
};
const PANEL: Rect = Rect {
    x: 264.0,
    y: TOP + 2.0,
    w: 208.0,
    h: 222.0,
};
const TAB_W: f32 = 60.0;

/// The shop screen.
pub struct ShopScreen {
    shop: Vec<Id>,
    tab: Tab,
    rows: Vec<Id>,
    menu: Menu,
    dialog: Option<(Id, QuantityDialog)>,
}

impl ShopScreen {
    pub fn new(shop: &[Id]) -> ShopScreen {
        ShopScreen {
            shop: shop.to_vec(),
            tab: if shop.is_empty() { Tab::Sell } else { Tab::Buy },
            rows: Vec::new(),
            menu: Menu::new(Vec::new()),
            dialog: None,
        }
    }

    fn tab_rect(tab: Tab) -> Rect {
        let i = match tab {
            Tab::Buy => 0.0,
            Tab::Sell => 1.0,
        };
        Rect::new(LIST.x + i * (TAB_W + 4.0), TOP + 2.0, TAB_W, 18.0)
    }

    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        self.rows = match self.tab {
            Tab::Buy => buy_rows(pack, &self.shop),
            Tab::Sell => sell_rows(pack, campaign),
        };
        let items = self
            .rows
            .iter()
            .filter_map(|id| pack.item(id))
            .map(|item| match self.tab {
                Tab::Buy => MenuItem::new(&item.name)
                    .detail(format::thousands(i64::from(item.price)))
                    .enabled(item.price > 0),
                Tab::Sell => match sell_price(item) {
                    Some(p) => {
                        MenuItem::new(format!("{} ×{}", item.name, campaign.item_count(&item.id)))
                            .detail(format::thousands(p))
                    }
                    None => {
                        MenuItem::new(format!("{} ×{}", item.name, campaign.item_count(&item.id)))
                            .detail("매각 불가")
                            .enabled(false)
                    }
                },
            })
            .collect();
        let cursor = self.menu.cursor();
        let rows = ((LIST.h - LIST_TOP - 4.0) / 18.0).floor() as usize;
        let mut menu =
            Menu::new(items)
                .rows(rows)
                .at(LIST.x + 2.0, LIST.y + LIST_TOP, LIST.w - 4.0);
        menu.framed = false;
        menu.row_height = 18.0;
        menu.tag_width = 18.0;
        menu.wrap = false;
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    fn switch_tab(&mut self, ctx: &mut Ctx, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.menu = Menu::new(Vec::new());
            ctx.sfx(sfx::CURSOR);
            self.rebuild(ctx);
        }
    }

    fn open_dialog(&mut self, ctx: &mut Ctx, index: usize) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
            return;
        };
        let Some(item) = self.rows.get(index).and_then(|id| pack.item(id)) else {
            return;
        };
        let campaign = &session.campaign;
        let dialog = match self.tab {
            Tab::Buy => {
                let max = max_affordable(campaign.gold, item.price);
                if max == 0 {
                    let msg = error_message(
                        &pack,
                        &CampaignError::NotEnoughGold {
                            need: i64::from(item.price),
                            have: campaign.gold,
                        },
                    );
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(msg);
                    return;
                }
                QuantityDialog::new(
                    &format!("{} 사기", with_particle(&item.name, Particle::EulReul)),
                    &format!("한 개 {}", format::thousands(i64::from(item.price))),
                    i64::from(item.price),
                    max,
                    "합계",
                    "구입",
                )
            }
            Tab::Sell => {
                let Some(price) = sell_price(item) else {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(error_message(
                        &pack,
                        &CampaignError::CannotSell(item.id.clone()),
                    ));
                    return;
                };
                let count = campaign.item_count(&item.id).min(MAX_QUANTITY);
                let note = if lost_to_cap(campaign.gold, price, pack.rules.gold_cap) > 0 {
                    let cap = format!("상한 {}", format::thousands(pack.rules.gold_cap));
                    format!(
                        "군자금 {} 넘는 금액은 사라집니다",
                        with_particle(&cap, Particle::EulReul)
                    )
                } else {
                    format!("한 개 {} (정가의 절반)", format::thousands(price))
                };
                QuantityDialog::new(
                    &format!("{} 팔기", with_particle(&item.name, Particle::EulReul)),
                    &note,
                    price,
                    count,
                    "매각액",
                    "매각",
                )
            }
        };
        self.dialog = Some((item.id.clone(), dialog));
    }

    /// Buy or sell `qty` copies; stops at the first error (shown as a toast).
    fn trade(&mut self, ctx: &mut Ctx, item: &str, qty: u32) {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_mut()) else {
            return;
        };
        let campaign = &mut session.campaign;
        let before = campaign.gold;
        let mut done = 0;
        let mut error = None;
        for _ in 0..qty {
            let result = match self.tab {
                Tab::Buy => campaign.buy(&pack, item),
                Tab::Sell => campaign.sell(&pack, item),
            };
            match result {
                Ok(()) => done += 1,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        let name = pack.item(item).map_or(item, |i| i.name.as_str());
        let spent = (campaign.gold - before).abs();
        if done > 0 {
            ctx.sfx(sfx::TREASURE);
            ctx.toast(match self.tab {
                Tab::Buy => format!(
                    "{} {}개를 {}에 샀습니다.",
                    name,
                    done,
                    format::thousands(spent)
                ),
                Tab::Sell => format!(
                    "{} {}개를 {}에 팔았습니다.",
                    name,
                    done,
                    format::thousands(spent)
                ),
            });
        }
        if let Some(e) = error {
            ctx.sfx(sfx::ERROR);
            ctx.toast(error_message(&pack, &e));
        }
        self.rebuild(ctx);
    }
}

impl Screen for ShopScreen {
    fn name(&self) -> &'static str {
        "camp-shop"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.rebuild(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some((item, mut dialog)) = self.dialog.take() {
            match dialog.update(ctx) {
                ConfirmEvent::Yes => {
                    let qty = dialog.qty;
                    self.trade(ctx, &item, qty);
                }
                ConfirmEvent::No => {}
                ConfirmEvent::None => self.dialog = Some((item, dialog)),
            }
            return Transition::None;
        }
        if let Some(p) = ctx.input.tap() {
            for tab in [Tab::Buy, Tab::Sell] {
                if Self::tab_rect(tab).contains(p) {
                    ctx.input.consume();
                    self.switch_tab(ctx, tab);
                    return Transition::None;
                }
            }
        }
        match ctx.input.nav() {
            Some(crate::input::Dir::Left) => {
                self.switch_tab(ctx, Tab::Buy);
                return Transition::None;
            }
            Some(crate::input::Dir::Right) => {
                self.switch_tab(ctx, Tab::Sell);
                return Transition::None;
            }
            _ => {}
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                self.open_dialog(ctx, i);
                Transition::None
            }
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        let campaign = &session.campaign;
        let gfx = &ctx.gfx;
        draw_camp_backdrop(ctx, 0.8);
        draw_header(ctx, "상점", campaign.gold);

        for tab in [Tab::Buy, Tab::Sell] {
            let r = Self::tab_rect(tab);
            let on = self.tab == tab;
            draw_window_ex(
                r,
                if on {
                    WindowStyle::Normal
                } else {
                    WindowStyle::Panel
                },
                1.0,
            );
            if on {
                draw_highlight(inset(r, 3.0), false, ctx.time);
            }
            gfx.text_aligned(
                match tab {
                    Tab::Buy => "구입",
                    Tab::Sell => "매각",
                },
                r.x,
                r.y + 1.0,
                r.w,
                Align::Center,
                TextStyle::main(if on {
                    theme::TEXT_ACCENT
                } else {
                    theme::TEXT_DIM
                })
                .shadow(theme::TEXT_SHADOW),
            );
        }
        let caption = match self.tab {
            Tab::Buy => "파는 물건 · 값",
            Tab::Sell => "가진 물건 · 매각가",
        };
        draw_list_frame(ctx, LIST, caption, true);
        if self.rows.is_empty() {
            gfx.text_aligned(
                match self.tab {
                    Tab::Buy => "이곳에서는 파는 물건이 없습니다.",
                    Tab::Sell => "팔 물건이 없습니다.",
                },
                LIST.x,
                LIST.y + LIST.h / 2.0 - 8.0,
                LIST.w,
                Align::Center,
                TextStyle::main(theme::TEXT_DIM),
            );
        } else {
            self.menu.draw(ctx);
            for (i, row) in visible_rows(&self.menu) {
                if let Some(item) = self.rows.get(i).and_then(|id| pack.item(id)) {
                    draw_icon(ctx, item_icon(item), vec2(row.x + 12.0, row.y + 1.0));
                }
            }
        }

        // Details of the highlighted item.
        draw_window_ex(PANEL, WindowStyle::Panel, 1.0);
        if let Some(item) = self
            .rows
            .get(self.menu.cursor())
            .and_then(|id| pack.item(id))
        {
            let x = PANEL.x + 8.0;
            let w = PANEL.w - 16.0;
            let mut y = PANEL.y + 8.0;
            draw_icon(ctx, item_icon(item), vec2(x, y));
            gfx.text(
                &item.name,
                x + 20.0,
                y,
                TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
            );
            if !item.hanja.is_empty() {
                gfx.text_aligned(
                    &item.hanja,
                    x,
                    y + 2.0,
                    w,
                    Align::Right,
                    TextStyle::small(theme::TEXT_DIM),
                );
            }
            y += 20.0;
            draw_caption(gfx, slot_name(item.kind), x, y);
            let effect = item_effect(pack, item);
            gfx.text(&effect, x + 40.0, y, TextStyle::small(theme::TEXT_GOOD));
            y += 16.0;
            draw_divider(x, y, w);
            y += 6.0;
            let lines = gfx.wrap(&item.desc, FontId::Main, 1, w);
            let shown = &lines[..lines.len().min(6)];
            gfx.text_lines(
                shown,
                x,
                y,
                TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
            );
            let bottom = PANEL.bottom() - 52.0;
            draw_divider(x, bottom, w);
            let (inv, equipped) = owned(campaign, &item.id);
            let small = TextStyle::small(theme::TEXT_DIM);
            let value = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
            gfx.text("소지", x, bottom + 7.0, small);
            let have = if equipped > 0 {
                format!("{inv}개 (장비 중 {equipped})")
            } else {
                format!("{inv}개")
            };
            gfx.text_aligned(&have, x, bottom + 5.0, w, Align::Right, value);
            let (label, price) = match self.tab {
                Tab::Buy => ("값", (item.price > 0).then(|| i64::from(item.price))),
                Tab::Sell => ("매각가", sell_price(item)),
            };
            gfx.text(label, x, bottom + 25.0, small);
            gfx.text_aligned(
                &price.map_or("—".to_string(), format::thousands),
                x,
                bottom + 23.0,
                w,
                Align::Right,
                value,
            );
            if self.tab == Tab::Buy {
                let n = max_affordable(campaign.gold, item.price);
                gfx.text_aligned(
                    &format!("살 수 있는 수량 {n}"),
                    x,
                    bottom + 39.0,
                    w,
                    Align::Right,
                    TextStyle::small(if n > 0 {
                        theme::TEXT_DIM
                    } else {
                        theme::TEXT_BAD
                    }),
                );
            }
        }
        draw_help(ctx, "←→ 구입/매각 · Z 선택 · X 돌아가기");
        if let Some((_, dialog)) = &self.dialog {
            dialog.draw(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    #[test]
    fn prices_and_affordability() {
        let pack = test_pack();
        let bean = pack.item("bean").unwrap();
        assert_eq!(sell_price(bean), Some(50));
        let mut treasure = bean.clone();
        treasure.price = 0;
        assert_eq!(sell_price(&treasure), None);
        assert_eq!(max_affordable(500, 100), 5);
        assert_eq!(max_affordable(99, 100), 0);
        assert_eq!(max_affordable(-5, 100), 0);
        assert_eq!(max_affordable(1_000_000, 1), MAX_QUANTITY);
        assert_eq!(max_affordable(500, 0), 0);
        assert_eq!(lost_to_cap(9_900, 200, 10_000), 100);
        assert_eq!(lost_to_cap(100, 200, 10_000), 0);
    }

    #[test]
    fn buying_and_selling_through_the_campaign() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        campaign.add_gold(&pack, 500);
        let n = max_affordable(campaign.gold, pack.item("wine").unwrap().price);
        for _ in 0..n {
            campaign.buy(&pack, "wine").unwrap();
        }
        assert_eq!(campaign.item_count("wine"), n);
        let err = campaign.buy(&pack, "wine").unwrap_err();
        assert!(error_message(&pack, &err).starts_with("군자금이 부족합니다"));
        assert!(sell_rows(&pack, &campaign).contains(&"wine".to_string()));
        assert_eq!(
            buy_rows(&pack, &["bean".into(), "nonexistent".into()]),
            vec!["bean".to_string()]
        );
        // Guan Yu's blade is owned but equipped.
        assert_eq!(owned(&campaign, "green_dragon_blade"), (0, 1));
        let err = campaign.sell(&pack, "green_dragon_blade").unwrap_err();
        assert_eq!(
            error_message(&pack, &err),
            "청룡언월도를 가지고 있지 않습니다."
        );
    }
}
