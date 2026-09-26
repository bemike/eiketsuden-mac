//! Title screen: artwork (`gfx/ui/title.png`, else the procedural backdrop), the logo and the
//! main menu — 새 게임 / 이어하기 / 불러오기 / 설정 / 제작진 / 종료 (native only).

use super::backdrop::draw_backdrop;
use super::credits::CreditsScreen;
use super::saveload::SaveLoadScreen;
use super::settings::SettingsScreen;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::AssetState;
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{draw_texture_fit, fill_gradient_v, Align, Fit, TextStyle};
use crate::saves;
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use macroquad::prelude::*;

const TITLE_ART: &str = "ui/title";
/// Top of the logo on canvases with room for it.
const LOGO_TOP: f32 = 34.0;
/// Height of the logo block: the big name (3 × 16 px lines) and the hanja line below it.
const LOGO_H: f32 = 86.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    NewGame,
    Continue,
    Load,
    Settings,
    Credits,
    Quit,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::NewGame => "새 게임",
            Item::Continue => "이어하기",
            Item::Load => "불러오기",
            Item::Settings => "설정",
            Item::Credits => "제작진",
            Item::Quit => "종료",
        }
    }
}

pub struct TitleScreen {
    items: Vec<Item>,
    menu: Menu,
    confirm_quit: Option<ConfirmDialog>,
    has_saves: bool,
    age: f32,
}

impl TitleScreen {
    pub fn new() -> TitleScreen {
        let mut items = vec![
            Item::NewGame,
            Item::Continue,
            Item::Load,
            Item::Settings,
            Item::Credits,
        ];
        if crate::platform::can_quit() {
            items.push(Item::Quit);
        }
        TitleScreen {
            menu: Menu::new(Vec::new()),
            items,
            confirm_quit: None,
            has_saves: false,
            age: 0.0,
        }
    }

    fn rebuild_menu(&mut self, ctx: &Ctx) {
        let has_pack = ctx.pack.is_some();
        let items = self
            .items
            .iter()
            .map(|it| {
                let enabled = match it {
                    Item::NewGame => has_pack,
                    Item::Continue | Item::Load => has_pack && self.has_saves,
                    _ => true,
                };
                MenuItem::new(it.label()).enabled(enabled)
            })
            .collect();
        let cursor = self.menu.cursor();
        let mut menu = Menu::new(items).cancellable(crate::platform::can_quit());
        menu.set_width(112.0);
        let h = menu.rect().h;
        let canvas = ctx.gfx.size();
        // Centred near the bottom edge.
        menu.set_position(
            ((canvas.x - 112.0) / 2.0).round(),
            (canvas.y - 18.0 - h).round(),
        );
        // Keep the cursor where it was, but land on "continue" when saves exist on first show.
        if self.age == 0.0 && self.has_saves {
            menu.set_cursor(1);
        } else if cursor < self.items.len() && menu.items[cursor].enabled {
            menu.set_cursor(cursor);
        }
        self.menu = menu;
    }

    fn activate(&mut self, ctx: &mut Ctx, item: Item) -> Transition {
        match item {
            Item::NewGame => Transition::Flow(Flow::NewGame),
            Item::Continue => {
                let Some(pack_id) = ctx.pack_id().map(str::to_string) else {
                    return Transition::None;
                };
                match saves::latest(ctx.storage.as_ref(), &pack_id) {
                    Some(slot) => match saves::read(ctx.storage.as_ref(), slot, &pack_id) {
                        Ok(save) => Transition::Flow(Flow::Continue(Box::new(save))),
                        Err(e) => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(e.to_string());
                            Transition::None
                        }
                    },
                    None => {
                        ctx.sfx(sfx::ERROR);
                        ctx.toast("이어할 기록이 없습니다.");
                        Transition::None
                    }
                }
            }
            Item::Load => match ctx.pack_id() {
                Some(id) => Transition::push(SaveLoadScreen::load(id)),
                None => Transition::None,
            },
            Item::Settings => Transition::push(SettingsScreen::new()),
            Item::Credits => Transition::push(CreditsScreen::new()),
            Item::Quit => {
                self.confirm_quit = Some(ConfirmDialog::new(&ctx.gfx, "게임을 종료할까요?"));
                Transition::None
            }
        }
    }
}

impl Default for TitleScreen {
    fn default() -> Self {
        TitleScreen::new()
    }
}

impl Screen for TitleScreen {
    fn name(&self) -> &'static str {
        "title"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        self.has_saves = ctx
            .pack_id()
            .is_some_and(|id| saves::any(ctx.storage.as_ref(), id));
        self.rebuild_menu(ctx);
        if how == Enter::Fresh || ctx.audio.bgm() != Some(bgm::TITLE) {
            ctx.audio.play_bgm(bgm::TITLE);
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.age += ctx.dt;
        if let Some(dialog) = self.confirm_quit.as_mut() {
            match dialog.update(ctx) {
                ConfirmEvent::Yes => {
                    self.confirm_quit = None;
                    return Transition::Quit;
                }
                ConfirmEvent::No => self.confirm_quit = None,
                ConfirmEvent::None => {}
            }
            return Transition::None;
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let item = self.items[i];
                self.activate(ctx, item)
            }
            MenuEvent::Cancelled => {
                // Cancel on the title screen jumps to "quit" (native).
                if let Some(q) = self.items.iter().position(|i| *i == Item::Quit) {
                    self.menu.set_cursor(q);
                }
                Transition::None
            }
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let (w, h) = (gfx.size().x, gfx.size().y);
        match ctx.media.texture_state(TITLE_ART) {
            AssetState::Ready => {
                if let Some(t) = ctx.media.texture(TITLE_ART) {
                    draw_texture_fit(&t, gfx.screen(), Fit::Cover, WHITE);
                }
            }
            _ => draw_backdrop(gfx.size(), ctx.time),
        }
        // Darken the top for the logo and the bottom for the menu.
        fill_gradient_v(
            Rect::new(0.0, 0.0, w, 120.0),
            Color::new(0.0, 0.0, 0.05, 0.55),
            Color::new(0.0, 0.0, 0.05, 0.0),
        );

        // Logo: "영걸전" large with "Reloaded", hanja subtitle below.
        let intro = (self.age / 0.8).min(1.0);
        let alpha = intro;
        let big = TextStyle::main(theme::TEXT_ACCENT.with_alpha(alpha))
            .size(3)
            .shadow(Color::new(0.1, 0.02, 0.0, 0.85 * alpha));
        let tag = TextStyle::main(theme::TEXT.with_alpha(alpha))
            .size(2)
            .shadow(Color::new(0.0, 0.0, 0.0, 0.8 * alpha));
        let w_big = gfx.text_width("영걸전", big.font, big.size);
        let w_tag = gfx.text_width("Reloaded", tag.font, tag.size);
        let gap = 10.0;
        let x0 = ((w - (w_big + gap + w_tag)) / 2.0).round();
        // 34 pixels from the top, moved up on canvases too low to fit it above the menu.
        let top = LOGO_TOP.min(self.menu.rect().y - LOGO_H - 4.0).max(2.0);
        let y0 = top + (1.0 - intro) * 6.0;
        gfx.text("영걸전", x0, y0, big);
        // Align the baseline of "Reloaded" with the big text's baseline.
        let dy = gfx.line_height(big.font, big.size) - gfx.line_height(tag.font, tag.size) - 3.0;
        gfx.text("Reloaded", x0 + w_big + gap, y0 + dy, tag);
        gfx.text_aligned(
            "英 傑 傳",
            0.0,
            y0 + 54.0,
            w,
            Align::Center,
            TextStyle::main(theme::TEXT_NAME.with_alpha(0.85 * alpha))
                .size(2)
                .shadow(Color::new(0.0, 0.0, 0.0, 0.7 * alpha)),
        );

        self.menu.draw(ctx);

        // Footer: pack and engine versions.
        let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
        if let Some(pack) = &ctx.pack {
            gfx.text(
                &format!("{} {}", pack.manifest.name, pack.manifest.version),
                4.0,
                h - 13.0,
                small,
            );
        }
        gfx.text_aligned(
            concat!("v", env!("CARGO_PKG_VERSION")),
            0.0,
            h - 13.0,
            w - 4.0,
            Align::Right,
            small,
        );

        if let Some(dialog) = &self.confirm_quit {
            crate::gfx::fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.0, 0.35));
            dialog.draw(ctx);
        }
    }
}
