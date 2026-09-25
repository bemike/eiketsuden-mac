//! Settings overlay: volumes, text speed, battle animation speed, fullscreen (native).
//!
//! Values change with left/right, the ◀ ▶ arrows or confirm (steps forward) and apply
//! immediately (the effect volume plays a sample); leaving the screen persists them through
//! the storage layer.

use crate::app::{Ctx, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, TextStyle, SCREEN, VIRTUAL_H, VIRTUAL_W};
use crate::settings::{cycle, BattleSpeed, Settings, TextSpeed};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::draw_window;
use macroquad::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Master,
    Bgm,
    Sfx,
    TextSpeed,
    BattleSpeed,
    Fullscreen,
    Defaults,
    Back,
}

const WIDTH: f32 = 260.0;

pub struct SettingsScreen {
    rows: Vec<Row>,
    menu: Menu,
    changed: bool,
}

impl SettingsScreen {
    pub fn new() -> SettingsScreen {
        let mut rows = vec![Row::Master, Row::Bgm, Row::Sfx, Row::TextSpeed, Row::BattleSpeed];
        if crate::platform::can_toggle_fullscreen() {
            rows.push(Row::Fullscreen);
        }
        rows.push(Row::Defaults);
        rows.push(Row::Back);
        SettingsScreen {
            rows,
            menu: Menu::new(Vec::new()),
            changed: false,
        }
    }

    fn items(&self, s: &Settings) -> Vec<MenuItem> {
        self.rows
            .iter()
            .map(|row| match row {
                Row::Master => MenuItem::new("전체 음량")
                    .detail(format::percent(s.master_volume))
                    .adjustable(),
                Row::Bgm => MenuItem::new("배경음")
                    .detail(format::percent(s.bgm_volume))
                    .adjustable(),
                Row::Sfx => MenuItem::new("효과음")
                    .detail(format::percent(s.sfx_volume))
                    .adjustable(),
                Row::TextSpeed => MenuItem::new("글자 속도")
                    .detail(s.text_speed.label())
                    .adjustable(),
                Row::BattleSpeed => MenuItem::new("전투 속도")
                    .detail(s.battle_speed.label())
                    .adjustable(),
                Row::Fullscreen => MenuItem::new("전체 화면")
                    .detail(if s.fullscreen { "켬" } else { "끔" })
                    .adjustable(),
                Row::Defaults => MenuItem::new("기본값으로"),
                Row::Back => MenuItem::new("돌아가기"),
            })
            .collect()
    }

    fn refresh(&mut self, ctx: &Ctx) {
        let items = self.items(&ctx.settings);
        self.menu.set_items(items);
    }

    fn adjust(&mut self, ctx: &mut Ctx, row: Row, delta: i32) {
        let s = &mut ctx.settings;
        let step = |v: u8| (i32::from(v) + delta * 10).clamp(0, 100) as u8;
        match row {
            Row::Master => s.master_volume = step(s.master_volume),
            Row::Bgm => s.bgm_volume = step(s.bgm_volume),
            Row::Sfx => s.sfx_volume = step(s.sfx_volume),
            Row::TextSpeed => s.text_speed = cycle(&TextSpeed::ALL, s.text_speed, delta),
            Row::BattleSpeed => s.battle_speed = cycle(&BattleSpeed::ALL, s.battle_speed, delta),
            Row::Fullscreen => {
                s.fullscreen = !s.fullscreen;
                // Fullscreen switches right away; the rest is persisted on leaving.
                ctx.commit_settings();
            }
            Row::Defaults | Row::Back => return,
        }
        ctx.audio.apply_settings(&ctx.settings);
        if matches!(row, Row::Master | Row::Sfx) {
            ctx.sfx(sfx::CONFIRM);
        }
        self.changed = true;
        self.refresh(ctx);
    }

    fn leave(&mut self, ctx: &mut Ctx) -> Transition {
        if self.changed {
            ctx.commit_settings();
            self.changed = false;
        }
        Transition::Pop
    }
}

impl Default for SettingsScreen {
    fn default() -> Self {
        SettingsScreen::new()
    }
}

impl Screen for SettingsScreen {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn is_overlay(&self) -> bool {
        true
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: crate::app::Enter) {
        let items = self.items(&ctx.settings);
        let mut menu = Menu::new(items);
        menu.framed = false;
        let h = menu.rect().h;
        menu.set_position(
            ((VIRTUAL_W - WIDTH) / 2.0).round() + 6.0,
            ((VIRTUAL_H - h) / 2.0).round() + 10.0,
        );
        menu.set_width(WIDTH - 12.0);
        self.menu = menu;
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Adjust(i, d) => {
                let row = self.rows[i];
                self.adjust(ctx, row, d);
            }
            MenuEvent::Selected(i) => match self.rows[i] {
                Row::Defaults => {
                    ctx.settings = Settings {
                        // Keep the window mode; resetting it unexpectedly is jarring.
                        fullscreen: ctx.settings.fullscreen,
                        ..Settings::default()
                    };
                    ctx.audio.apply_settings(&ctx.settings);
                    self.changed = true;
                    self.refresh(ctx);
                    ctx.toast("기본 설정으로 되돌렸습니다.");
                }
                Row::Back => return self.leave(ctx),
                row => self.adjust(ctx, row, 1),
            },
            MenuEvent::Cancelled => return self.leave(ctx),
            _ => {}
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        fill_rect(SCREEN, Color::new(0.0, 0.0, 0.02, 0.55));
        let m = self.menu.rect();
        let frame = Rect::new(m.x - 6.0, m.y - 26.0, WIDTH, m.h + 32.0);
        draw_window(frame);
        ctx.gfx.text_aligned(
            "설정",
            frame.x,
            frame.y + 6.0,
            frame.w,
            Align::Center,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        self.menu.draw(ctx);
        let where_ = format!("저장 위치: {}", ctx.storage.location());
        let lines = ctx
            .gfx
            .wrap(&where_, crate::gfx::FontId::Small, 1, VIRTUAL_W - 20.0);
        ctx.gfx.text_lines(
            &lines,
            10.0,
            VIRTUAL_H - 4.0 - 12.0 * lines.len() as f32,
            TextStyle::small(theme::TEXT_DISABLED),
        );
    }
}
