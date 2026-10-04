//! Personal-inventory shop: buy item then recipient; sell owner then physical item.
use super::widgets::{draw_camp_backdrop, draw_header, officer_name};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use hero_core::data::Id;

enum Step {
    Root,
    BuyItems,
    BuyOwner(Id),
    SellOwner,
    SellItems(Id),
}
pub struct PersonalShop {
    shop: Vec<Id>,
    step: Step,
    menu: Menu,
    ids: Vec<Id>,
    slots: Vec<usize>,
}
impl PersonalShop {
    pub fn new(shop: &[Id]) -> Self {
        Self {
            shop: shop.into(),
            step: Step::Root,
            menu: Menu::new(vec![]),
            ids: vec![],
            slots: vec![],
        }
    }
    fn rebuild(&mut self, ctx: &Ctx) {
        let (Some(pack), Some(session)) = (&ctx.pack, &ctx.session) else {
            return;
        };
        self.ids.clear();
        self.slots.clear();
        let entries = match &self.step {
            Step::Root => vec![
                MenuItem::new("购买"),
                MenuItem::new("贩卖"),
                MenuItem::new("离开"),
            ],
            Step::BuyItems => {
                self.ids = self
                    .shop
                    .iter()
                    .filter(|id| pack.item(id).is_some_and(|i| i.price > 0))
                    .cloned()
                    .collect();
                self.ids
                    .iter()
                    .map(|id| {
                        let i = pack.item(id).unwrap();
                        MenuItem::new(&i.name).detail(i.price.to_string())
                    })
                    .collect()
            }
            Step::BuyOwner(_) | Step::SellOwner => {
                self.ids = session
                    .campaign
                    .roster
                    .iter()
                    .filter(|o| !o.away && o.equip.carried.is_some())
                    .map(|o| o.id.clone())
                    .collect();
                self.ids
                    .iter()
                    .map(|id| MenuItem::new(officer_name(pack, id)))
                    .collect()
            }
            Step::SellItems(owner) => {
                if let Some(pocket) = session
                    .campaign
                    .officer(owner)
                    .and_then(|o| o.equip.carried.as_ref())
                {
                    for (slot, id) in pocket.slots().iter().enumerate() {
                        if let Some(id) = id {
                            self.ids.push(id.clone());
                            self.slots.push(slot);
                        }
                    }
                }
                self.ids
                    .iter()
                    .map(|id| {
                        let item = pack.item(id);
                        let price = item.and_then(super::shop::sell_price);
                        MenuItem::new(item.map_or(id.as_str(), |i| i.name.as_str()))
                            .detail(price.map_or("无法出售".into(), |p| p.to_string()))
                            .enabled(price.is_some())
                    })
                    .collect()
            }
        };
        self.menu = Menu::new(entries).rows(8).at(170.0, 80.0, 180.0);
    }
}
impl Screen for PersonalShop {
    fn name(&self) -> &'static str {
        "personal-shop"
    }
    fn in_camp_frame(&self) -> bool {
        true
    }
    fn on_enter(&mut self, ctx: &mut Ctx, _: Enter) {
        self.rebuild(ctx);
    }
    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let mut result = None;
                match &self.step {
                    Step::Root => match i {
                        0 => self.step = Step::BuyItems,
                        1 => self.step = Step::SellOwner,
                        _ => return Transition::Pop,
                    },
                    Step::BuyItems => self.step = Step::BuyOwner(self.ids[i].clone()),
                    Step::SellOwner => self.step = Step::SellItems(self.ids[i].clone()),
                    Step::BuyOwner(item) => {
                        if let (Some(pack), Some(session)) = (&ctx.pack, &mut ctx.session) {
                            result = Some(session.campaign.buy_for(pack, item, &self.ids[i]));
                        }
                    }
                    Step::SellItems(owner) => {
                        if let (Some(pack), Some(session)) = (&ctx.pack, &mut ctx.session) {
                            result = Some(session.campaign.sell_from(pack, owner, self.slots[i]));
                        }
                    }
                }
                if let Some(result) = result {
                    match result {
                        Ok(()) => {
                            ctx.sfx(sfx::TREASURE);
                            ctx.toast("交易完成。");
                        }
                        Err(e) => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(&e.to_string());
                        }
                    }
                }
                self.rebuild(ctx);
            }
            MenuEvent::Cancelled => {
                self.step = match self.step {
                    Step::Root => return Transition::Pop,
                    Step::BuyItems | Step::SellOwner => Step::Root,
                    Step::BuyOwner(_) => Step::BuyItems,
                    Step::SellItems(_) => Step::SellOwner,
                };
                self.rebuild(ctx);
            }
            _ => {}
        }
        Transition::None
    }
    fn draw(&self, ctx: &Ctx) {
        draw_camp_backdrop(ctx, 0.0);
        let gold = ctx.session.as_ref().map_or(0, |s| s.campaign.gold);
        let title = match self.step {
            Step::Root => "道具屋",
            Step::BuyItems => "买什么？",
            Step::BuyOwner(_) => "交给哪位武将？",
            Step::SellOwner => "哪位武将要卖？",
            Step::SellItems(_) => "卖什么？",
        };
        draw_header(ctx, title, gold);
        self.menu.draw(ctx);
    }
}
