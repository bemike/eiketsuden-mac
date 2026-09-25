//! UI gallery — the frontend's visual test bench.
//!
//! Started with `eiketsuden --gallery` natively or `index.html#gallery` on the web. Only the
//! fonts and UI sounds are loaded (no data pack is needed); portraits, sprites, icons and music
//! are requested from the pack directory as usual, so with a pack present they show the real
//! media and without one they show their fallbacks — both are worth looking at.
//!
//! Pages (Tab / Shift+Tab, PageUp / PageDown, keys 1–5 or tapping a tab):
//!
//! 1. **글꼴** — Galmuri11 12 px and Galmuri9 10 px with Hangul, Hanja and Latin, integer sizes,
//!    the Korean word wrap (←/→ changes the wrap width) and the canvas scale.
//! 2. **창·메뉴** — window styles, highlights, cursors, a scrolling menu with disabled and
//!    adjustable items and tooltips, icons, animated sprites, images and portrait boxes.
//! 3. **대화** — message box pages with typewriter text, name tab and portrait; choice box
//!    (`C`), yes/no confirmation (`Y`), next sample (`N`).
//! 4. **게이지·숫자** — HP/MP/EXP/morale gauges, number and time formatting, toast (`T`) and
//!    banner (`B`).
//! 5. **화면** — opens the real screens (title, settings, credits, error, placeholder, game over,
//!    the game itself) and exercises the audio manager.

use super::backdrop::draw_backdrop;
use super::credits::CreditsScreen;
use super::error::ErrorScreen;
use super::gameover::GameOverScreen;
use super::loading::{LoadingScreen, Target};
use super::placeholder::PlaceholderScreen;
use super::settings::SettingsScreen;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::AssetState;
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, Fit, FontId, TextStyle, SCREEN, VIRTUAL_H, VIRTUAL_W};
use crate::platform::unix_now;
use crate::settings::{cycle, TextSpeed};
use crate::ui::bars::{draw_gauge, draw_gauge_labeled, GaugeKind};
use crate::ui::dialog::{ChoiceBox, ChoiceEvent, ConfirmDialog, ConfirmEvent};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::message::{MessageBox, MessageEvent};
use crate::ui::theme;
use crate::ui::toast::Banner;
use crate::ui::tooltip::{draw_tooltip, HoverTimer};
use crate::ui::window::{
    content_rect, draw_arrow_cursor, draw_divider, draw_highlight, draw_icon, draw_image,
    draw_portrait, draw_side_arrow, draw_small_arrow, draw_sprite, draw_window, draw_window_ex,
    inset, WindowStyle,
};
use hero_core::campaign::Node;
use macroquad::prelude::*;

const TABS: [&str; 5] = ["글꼴", "창·메뉴", "대화", "게이지·숫자", "화면"];
const TAB_H: f32 = 18.0;
/// Top of the page area below the tab bar.
const TOP: f32 = TAB_H + 5.0;

const PAGE_FONTS: usize = 0;
const PAGE_WIDGETS: usize = 1;
const PAGE_DIALOGUE: usize = 2;
const PAGE_GAUGES: usize = 3;
const PAGE_SCREENS: usize = 4;

const HANGUL: &str = "가나다라마바사 아자차카타파하 · 유비 관우 장비 조조";
const HANJA: &str = "劉備 關羽 張飛 諸葛亮 曹操 · 英傑傳 · 三國志";
const LATIN: &str = "ABC xyz 0123456789 ,.!?%()[]:; 1,200/1,500";

const WRAP_SAMPLE: &str = "황건적이 들고일어나 온 고을이 불타던 해, 탁현의 한 젊은이가 \
    돗자리를 짜던 손을 멈추고 칼을 들었다. 공백이 있으면 공백에서 줄을 바꾸고, \
    공백없이아주길게이어지는낱말은음절과음절사이에서끊습니다.\n\
    줄바꿈 문자는 그대로 지킵니다 — 英傑傳 (Eiketsuden).";
const WRAP_MIN: f32 = 96.0;
const WRAP_MAX: f32 = 440.0;

/// Dialogue samples: speaker, portrait key, text. All text is original to this project.
const DIALOGUE: [(Option<&str>, Option<&str>, &str); 3] = [
    (
        Some("유비"),
        Some("liu_bei"),
        "도적 떼가 백성을 괴롭히는데 관군은 손을 놓고 있구나. 뜻을 같이할 사람이 있다면 \
         함께 의병을 일으키고 싶소. 이 대사는 여러 페이지에 걸쳐 한 글자씩 나타나며, \
         확인 키나 클릭으로 페이지를 넘깁니다. 누르고 있으면 빨라집니다.",
    ),
    (
        None,
        None,
        "— 화자도 초상화도 없는 해설 상자입니다. 긴 문장은 자동으로 줄이 바뀌고, \
         세 줄을 넘으면 다음 페이지로 이어집니다.",
    ),
    (
        Some("관우"),
        None,
        "형님의 뜻이 곧 저의 뜻입니다. (초상화 없이 이름표만 있는 대사입니다.)",
    ),
];

/// The demo menu of the widgets page: label, detail, enabled, adjustable, tooltip.
const DEMO_ITEMS: [(&str, &str, bool, bool, &str); 12] = [
    ("공격", "", true, false, "인접한 적 부대를 공격합니다."),
    (
        "책략",
        "MP 6",
        true,
        false,
        "책략을 사용합니다. 오른쪽 값은 소모 MP입니다.",
    ),
    ("도구", "3개", true, false, "소지한 도구를 사용합니다."),
    ("대기", "", true, false, "이번 턴의 행동을 마칩니다."),
    (
        "퇴각",
        "",
        false,
        false,
        "비활성 항목: 선택하면 오류음이 납니다.",
    ),
    (
        "음량",
        "",
        true,
        true,
        "←/→ 또는 ◀ ▶ 를 눌러 값을 바꿉니다.",
    ),
    (
        "글자 속도",
        "",
        true,
        true,
        "조절 항목: 설정 화면과 같은 방식입니다.",
    ),
    (
        "장비",
        "",
        true,
        false,
        "목록이 길면 휠·드래그·방향키로 스크롤됩니다.",
    ),
    (
        "능력치",
        "",
        true,
        false,
        "커서가 보이는 범위를 벗어나면 자동으로 스크롤됩니다.",
    ),
    (
        "전황",
        "",
        true,
        false,
        "아래쪽의 깜빡이는 화살표는 남은 항목을 뜻합니다.",
    ),
    (
        "부대 목록",
        "",
        true,
        false,
        "마우스를 올려 두면 이 설명(툴팁)이 나타납니다.",
    ),
    ("턴 종료", "", true, false, "마지막 항목입니다."),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenItem {
    Title,
    Settings,
    Credits,
    Error,
    Placeholder,
    GameOver,
    StartGame,
    MusicTitle,
    MusicBattle,
    MusicStop,
    Jingle,
    Effect,
    Quit,
}

impl ScreenItem {
    fn label(self) -> &'static str {
        match self {
            ScreenItem::Title => "타이틀 화면",
            ScreenItem::Settings => "설정 (오버레이)",
            ScreenItem::Credits => "제작진",
            ScreenItem::Error => "오류 화면",
            ScreenItem::Placeholder => "개발용 대체 화면",
            ScreenItem::GameOver => "게임 오버",
            ScreenItem::StartGame => "데이터 팩을 불러와 게임 시작",
            ScreenItem::MusicTitle => "배경음: title",
            ScreenItem::MusicBattle => "배경음: battle (교차 페이드)",
            ScreenItem::MusicStop => "배경음 정지 (페이드 아웃)",
            ScreenItem::Jingle => "징글: victory (1회)",
            ScreenItem::Effect => "효과음: levelup",
            ScreenItem::Quit => "종료",
        }
    }
}

/// Small clickable button with a keyboard shortcut (gallery only).
struct Button {
    rect: Rect,
    label: &'static str,
    key: KeyCode,
}

impl Button {
    const fn new(x: f32, y: f32, w: f32, label: &'static str, key: KeyCode) -> Button {
        Button {
            rect: Rect { x, y, w, h: 18.0 },
            label,
            key,
        }
    }

    /// Pressed this frame (tap or shortcut key). Consumes the input when it fires so widgets
    /// updated later in the frame do not react to the same click.
    fn pressed(&self, ctx: &mut Ctx) -> bool {
        let hit = ctx.input.tapped(self.rect) || ctx.input.key_pressed(self.key);
        if hit {
            ctx.input.consume();
            ctx.sfx(sfx::CONFIRM);
        }
        hit
    }

    fn draw(&self, ctx: &Ctx) {
        draw_window_ex(self.rect, WindowStyle::Panel, 1.0);
        if ctx.input.hovering(self.rect) {
            draw_highlight(inset(self.rect, 3.0), true, ctx.time);
        }
        ctx.gfx.text_aligned(
            self.label,
            self.rect.x,
            self.rect.y + 3.0,
            self.rect.w,
            Align::Center,
            TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW),
        );
    }
}

const DIALOGUE_BUTTONS: [Button; 3] = [
    Button::new(8.0, TOP + 2.0, 92.0, "선택지 [C]", KeyCode::C),
    Button::new(104.0, TOP + 2.0, 92.0, "예/아니오 [Y]", KeyCode::Y),
    Button::new(200.0, TOP + 2.0, 92.0, "다음 예시 [N]", KeyCode::N),
];
const GAUGE_BUTTONS: [Button; 2] = [
    Button::new(250.0, 196.0, 106.0, "토스트 [T]", KeyCode::T),
    Button::new(362.0, 196.0, 106.0, "배너 [B]", KeyCode::B),
];

enum Modal {
    None,
    Choice(ChoiceBox),
    Confirm(ConfirmDialog),
}

pub struct GalleryScreen {
    page: usize,
    // Fonts page.
    wrap_width: f32,
    wrapped: Vec<String>,
    // Widgets page.
    demo_menu: Menu,
    volume: u8,
    text_speed: TextSpeed,
    hover: HoverTimer,
    // Dialogue page.
    sample: usize,
    message: Option<MessageBox>,
    modal: Modal,
    result: String,
    // Gauges page.
    banner: Option<Banner>,
    toasts_shown: u32,
    // Screens page.
    screen_items: Vec<ScreenItem>,
    screens_menu: Menu,
}

impl GalleryScreen {
    pub fn new() -> GalleryScreen {
        let mut screen_items = vec![
            ScreenItem::Title,
            ScreenItem::Settings,
            ScreenItem::Credits,
            ScreenItem::Error,
            ScreenItem::Placeholder,
            ScreenItem::GameOver,
            ScreenItem::StartGame,
            ScreenItem::MusicTitle,
            ScreenItem::MusicBattle,
            ScreenItem::MusicStop,
            ScreenItem::Jingle,
            ScreenItem::Effect,
        ];
        if crate::platform::can_quit() {
            screen_items.push(ScreenItem::Quit);
        }
        let screens_menu = Menu::new(
            screen_items
                .iter()
                .map(|s| MenuItem::new(s.label()))
                .collect(),
        )
        .at(8.0, TOP + 2.0, 212.0)
        .rows(14)
        .cancellable(false);
        GalleryScreen {
            page: PAGE_FONTS,
            wrap_width: 300.0,
            wrapped: Vec::new(),
            demo_menu: Menu::new(Vec::new()),
            volume: 70,
            text_speed: TextSpeed::Normal,
            hover: HoverTimer::default(),
            sample: 0,
            message: None,
            modal: Modal::None,
            result: "—".into(),
            banner: None,
            toasts_shown: 0,
            screen_items,
            screens_menu,
        }
    }

    fn tab_rect(i: usize) -> Rect {
        let w = VIRTUAL_W / TABS.len() as f32;
        Rect::new((i as f32 * w).round(), 0.0, w.round(), TAB_H)
    }

    fn rewrap(&mut self, ctx: &Ctx) {
        self.wrapped = ctx.gfx.wrap(WRAP_SAMPLE, FontId::Main, 1, self.wrap_width);
    }

    fn demo_items(&self) -> Vec<MenuItem> {
        DEMO_ITEMS
            .iter()
            .enumerate()
            .map(|(i, (label, detail, enabled, adjustable, _))| {
                let mut item = MenuItem::new(*label).enabled(*enabled);
                let detail = match i {
                    5 => format::percent(self.volume),
                    6 => self.text_speed.label().to_string(),
                    _ => detail.to_string(),
                };
                if !detail.is_empty() {
                    item = item.detail(detail);
                }
                if *adjustable {
                    item = item.adjustable();
                }
                item
            })
            .collect()
    }

    fn start_sample(&mut self, ctx: &Ctx) {
        let (speaker, portrait, text) = DIALOGUE[self.sample % DIALOGUE.len()];
        self.message = Some(MessageBox::new(&ctx.gfx, speaker, portrait, text));
    }

    /// Tab switching: returns `true` when the page changed.
    fn switch_page(&mut self, ctx: &mut Ctx) -> bool {
        let input = &ctx.input;
        let n = TABS.len();
        let mut target = None;
        let shift = input.key_down(KeyCode::LeftShift) || input.key_down(KeyCode::RightShift);
        if input.key_pressed(KeyCode::Tab) {
            target = Some(if shift {
                (self.page + n - 1) % n
            } else {
                (self.page + 1) % n
            });
        } else if input.key_pressed(KeyCode::PageDown) {
            target = Some((self.page + 1) % n);
        } else if input.key_pressed(KeyCode::PageUp) {
            target = Some((self.page + n - 1) % n);
        }
        let digits = [
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
        ];
        for (i, key) in digits.iter().enumerate().take(n) {
            if input.key_pressed(*key) {
                target = Some(i);
            }
        }
        if let Some(p) = input.tap() {
            if let Some(i) = (0..n).find(|&i| Self::tab_rect(i).contains(p)) {
                target = Some(i);
            }
        }
        match target {
            Some(t) if t != self.page => {
                self.page = t;
                self.modal = Modal::None;
                ctx.input.consume();
                ctx.sfx(sfx::CURSOR);
                true
            }
            Some(_) => {
                ctx.input.consume();
                false
            }
            None => false,
        }
    }

    fn update_fonts(&mut self, ctx: &mut Ctx) {
        let delta = match ctx.input.nav() {
            Some(crate::input::Dir::Left) => -8.0,
            Some(crate::input::Dir::Right) => 8.0,
            _ => 0.0,
        };
        if delta != 0.0 {
            self.wrap_width = (self.wrap_width + delta).clamp(WRAP_MIN, WRAP_MAX);
            self.rewrap(ctx);
            ctx.sfx(sfx::CURSOR);
        }
    }

    fn update_widgets(&mut self, ctx: &mut Ctx) {
        match self.demo_menu.update(ctx) {
            MenuEvent::Selected(i) => ctx.toast(format!("‘{}’ 선택", DEMO_ITEMS[i].0)),
            MenuEvent::Adjust(5, d) => {
                self.volume = (i32::from(self.volume) + d * 10).clamp(0, 100) as u8;
                let items = self.demo_items();
                self.demo_menu.set_items(items);
            }
            MenuEvent::Adjust(_, d) => {
                self.text_speed = cycle(&TextSpeed::ALL, self.text_speed, d);
                let items = self.demo_items();
                self.demo_menu.set_items(items);
            }
            MenuEvent::Cancelled => ctx.toast("취소 (X / Esc / 오른쪽 클릭)"),
            MenuEvent::Moved(_) | MenuEvent::None => {}
        }
        let hovered = ctx
            .input
            .pointer()
            .and_then(|p| self.demo_menu.row_at(p))
            .map(|i| i as u64);
        self.hover.update(hovered, ctx.dt);
    }

    fn update_dialogue(&mut self, ctx: &mut Ctx) {
        match &mut self.modal {
            Modal::Choice(choice) => {
                match choice.update(ctx) {
                    ChoiceEvent::Chosen(i) => {
                        self.result = format!("선택지 {}번", i + 1);
                        self.modal = Modal::None;
                    }
                    ChoiceEvent::Cancelled => {
                        self.result = "선택 취소".into();
                        self.modal = Modal::None;
                    }
                    ChoiceEvent::None => {}
                }
                return;
            }
            Modal::Confirm(dialog) => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => {
                        self.result = "예".into();
                        self.modal = Modal::None;
                    }
                    ConfirmEvent::No => {
                        self.result = "아니오".into();
                        self.modal = Modal::None;
                    }
                    ConfirmEvent::None => {}
                }
                return;
            }
            Modal::None => {}
        }
        if DIALOGUE_BUTTONS[0].pressed(ctx) {
            let bottom = MessageBox::box_rect(true).y - 22.0;
            let choice = ChoiceBox::new(
                &ctx.gfx,
                Some("어디로 향하시겠습니까?"),
                &[
                    "북쪽 관문으로 진군한다",
                    "마을에서 병사를 모은다",
                    "잠시 쉬어 간다",
                ],
                Some(2),
            )
            .with_bottom(bottom);
            self.modal = Modal::Choice(choice);
            return;
        }
        if DIALOGUE_BUTTONS[1].pressed(ctx) {
            self.modal = Modal::Confirm(ConfirmDialog::new(&ctx.gfx, "정말로 출진하시겠습니까?"));
            return;
        }
        if DIALOGUE_BUTTONS[2].pressed(ctx) {
            self.sample += 1;
            self.start_sample(ctx);
            return;
        }
        if let Some(message) = self.message.as_mut() {
            if message.update(ctx) == MessageEvent::Finished {
                self.sample += 1;
                self.start_sample(ctx);
            }
        }
    }

    fn update_gauges(&mut self, ctx: &mut Ctx) {
        if let Some(banner) = self.banner.as_mut() {
            if banner.update(ctx.dt) {
                self.banner = None;
            } else if ctx.input.confirm() {
                banner.dismiss();
                ctx.input.consume();
            }
        }
        if GAUGE_BUTTONS[0].pressed(ctx) {
            self.toasts_shown += 1;
            ctx.toast(format!(
                "알림 {}: 토스트는 모든 화면 위에 잠시 표시됩니다.",
                self.toasts_shown
            ));
        }
        if GAUGE_BUTTONS[1].pressed(ctx) {
            self.banner = Some(Banner::new("제1장", Some("탁현의 의병"), 2.6));
        }
    }

    fn update_screens(&mut self, ctx: &mut Ctx) -> Transition {
        let MenuEvent::Selected(i) = self.screens_menu.update(ctx) else {
            return Transition::None;
        };
        match self.screen_items[i] {
            ScreenItem::Title => Transition::Flow(Flow::Title),
            ScreenItem::Settings => Transition::push(SettingsScreen::new()),
            ScreenItem::Credits => Transition::push(CreditsScreen::new()),
            ScreenItem::Error => Transition::push(ErrorScreen::recoverable(
                "오류 화면 예시",
                vec![
                    "오류 화면은 제목, 자세한 내용(줄바꿈·스크롤), 선택 메뉴로 이루어집니다."
                        .into(),
                    "technical detail: example error text for bug reports".into(),
                ],
            )),
            ScreenItem::Placeholder => {
                Transition::push(PlaceholderScreen::for_node(&Node::Drama {
                    id: "gallery_drama".into(),
                    scene: "prologue".into(),
                    next: "gallery_camp".into(),
                }))
            }
            ScreenItem::GameOver => Transition::push(GameOverScreen::new()),
            ScreenItem::StartGame => Transition::replace(LoadingScreen::new(Target::Game)),
            ScreenItem::MusicTitle => {
                ctx.audio.play_bgm(bgm::TITLE);
                Transition::None
            }
            ScreenItem::MusicBattle => {
                ctx.audio.play_bgm(bgm::BATTLE);
                Transition::None
            }
            ScreenItem::MusicStop => {
                ctx.audio.stop_bgm();
                Transition::None
            }
            ScreenItem::Jingle => {
                ctx.audio.play_jingle(bgm::VICTORY);
                Transition::None
            }
            ScreenItem::Effect => {
                ctx.sfx(sfx::LEVELUP);
                Transition::None
            }
            ScreenItem::Quit => Transition::Quit,
        }
    }

    // ----- drawing ---------------------------------------------------------------------------

    fn draw_tabs(&self, ctx: &Ctx) {
        fill_rect(
            Rect::new(0.0, 0.0, VIRTUAL_W, TAB_H + 1.0),
            theme::BORDER_OUTER,
        );
        for (i, label) in TABS.iter().enumerate() {
            let r = Self::tab_rect(i);
            let active = i == self.page;
            let style = if active {
                WindowStyle::Normal
            } else {
                WindowStyle::Panel
            };
            draw_window_ex(r, style, 1.0);
            let color = if active {
                theme::TEXT_ACCENT
            } else if ctx.input.hovering(r) {
                theme::TEXT
            } else {
                theme::TEXT_DIM
            };
            ctx.gfx.text_aligned(
                &format!("{} {label}", i + 1),
                r.x,
                r.y + 1.0,
                r.w,
                Align::Center,
                TextStyle::main(color).shadow(theme::TEXT_SHADOW),
            );
        }
    }

    fn draw_fonts(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let label = TextStyle::small(theme::TEXT_DIM);
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);

        let r = Rect::new(8.0, TOP, VIRTUAL_W - 16.0, 146.0);
        draw_window(r);
        let x = r.x + 10.0;
        let mut y = r.y + 6.0;
        gfx.text("Galmuri11 · 12px (본문·대사)", x, y, label);
        y += 12.0;
        for line in [HANGUL, HANJA, LATIN] {
            gfx.text(line, x, y, main);
            y += 16.0;
        }
        y += 2.0;
        gfx.text("Galmuri9 · 10px (작은 숫자·설명)", x, y, label);
        y += 12.0;
        for line in [HANGUL, HANJA, LATIN] {
            gfx.text(line, x, y, small);
            y += 12.0;
        }
        let big = TextStyle::main(theme::TEXT_ACCENT)
            .size(2)
            .shadow(theme::TEXT_SHADOW);
        gfx.text_aligned(
            "영걸전 英傑傳",
            r.x,
            r.y + 6.0,
            r.w - 12.0,
            Align::Right,
            big,
        );
        gfx.text_aligned(
            "×2",
            r.x,
            r.y + 38.0,
            r.w - 12.0,
            Align::Right,
            TextStyle::small(theme::TEXT_DIM),
        );

        // Word wrap demo with a guide at the wrap width.
        let w = Rect::new(
            8.0,
            r.bottom() + 5.0,
            VIRTUAL_W - 16.0,
            VIRTUAL_H - r.bottom() - 26.0,
        );
        draw_window_ex(w, WindowStyle::Panel, 1.0);
        let c = content_rect(w);
        gfx.text(
            &format!("자동 줄바꿈 · 폭 {}px (←/→로 조절)", self.wrap_width),
            c.x + 2.0,
            c.y - 2.0,
            label,
        );
        let tx = c.x + 2.0;
        let guide_x = tx + self.wrap_width;
        fill_rect(
            Rect::new(guide_x, c.y + 10.0, 1.0, c.h - 10.0),
            theme::TEXT_BAD.with_alpha(0.6),
        );
        let max_lines = ((c.h - 10.0) / 16.0) as usize;
        let shown = &self.wrapped[..self.wrapped.len().min(max_lines)];
        gfx.text_lines(shown, tx, c.y + 10.0, main);
        if self.wrapped.len() > max_lines {
            draw_small_arrow(w.right() - 10.0, w.bottom() - 7.0, true, theme::TEXT_ACCENT);
        }

        // Status line: scale and font state.
        let status = if gfx.fonts.missing().is_empty() {
            format!(
                "S={}  창 {}×{}  캔버스 {}×{}  글꼴 정상",
                gfx.scale(),
                screen_width(),
                screen_height(),
                480 * gfx.scale(),
                270 * gfx.scale()
            )
        } else {
            format!(
                "글꼴 없음(대체 글꼴 사용): {}",
                gfx.fonts.missing().join(", ")
            )
        };
        let color = if gfx.fonts.missing().is_empty() {
            theme::TEXT_DIM
        } else {
            theme::TEXT_BAD
        };
        gfx.text(&status, 10.0, VIRTUAL_H - 15.0, TextStyle::small(color));
    }

    fn draw_widgets(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let label = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);

        // Left column: window styles, highlight, cursors, divider.
        let a = Rect::new(8.0, TOP, 144.0, 44.0);
        draw_window(a);
        gfx.text("기본 창", a.x + 10.0, a.y + 6.0, main);
        gfx.text("WindowStyle::Normal", a.x + 10.0, a.y + 24.0, label);
        let b = Rect::new(8.0, a.bottom() + 6.0, 144.0, 44.0);
        draw_window_ex(b, WindowStyle::Panel, 1.0);
        gfx.text("패널 창", b.x + 10.0, b.y + 6.0, main);
        gfx.text("WindowStyle::Panel", b.x + 10.0, b.y + 24.0, label);
        let c = Rect::new(8.0, b.bottom() + 6.0, 144.0, 60.0);
        draw_window(c);
        let row1 = Rect::new(c.x + 5.0, c.y + 6.0, c.w - 10.0, 16.0);
        draw_highlight(row1, true, ctx.time);
        draw_arrow_cursor(row1.x + 8.0, row1.y + 8.0, ctx.time);
        gfx.text("선택 (활성)", row1.x + 12.0, row1.y, main);
        let row2 = Rect::new(c.x + 5.0, row1.bottom() + 2.0, c.w - 10.0, 16.0);
        draw_highlight(row2, false, ctx.time);
        gfx.text("선택 (비활성)", row2.x + 12.0, row2.y, main);
        draw_divider(c.x + 6.0, row2.bottom() + 4.0, c.w - 12.0);
        let ay = row2.bottom() + 12.0;
        draw_side_arrow(c.x + 14.0, ay, false, theme::CURSOR_ARROW);
        draw_side_arrow(c.x + 24.0, ay, true, theme::CURSOR_ARROW);
        draw_small_arrow(c.x + 38.0, ay, false, theme::TEXT_ACCENT);
        draw_small_arrow(c.x + 48.0, ay, true, theme::TEXT_ACCENT);
        let d = Rect::new(8.0, c.bottom() + 6.0, 144.0, VIRTUAL_H - c.bottom() - 14.0);
        draw_window_ex(d, WindowStyle::Panel, 1.0);
        gfx.text("16×16 아이콘", d.x + 8.0, d.y + 5.0, label);
        for (i, key) in ["gold", "hp", "mp", "atk", "def", "move", "exp"]
            .iter()
            .enumerate()
        {
            draw_icon(ctx, key, vec2(d.x + 8.0 + i as f32 * 18.0, d.y + 20.0));
        }

        // Middle: the demo menu.
        self.demo_menu.draw(ctx);
        let m = self.demo_menu.rect();
        gfx.text(
            "휠·드래그·방향키, 클릭 선택",
            m.x + 2.0,
            m.bottom() + 4.0,
            label,
        );

        // Right column: sprites, image, portraits.
        let s = Rect::new(318.0, TOP, 154.0, 50.0);
        draw_window_ex(s, WindowStyle::Panel, 1.0);
        gfx.text("유닛 스프라이트 (걷기)", s.x + 8.0, s.y + 5.0, label);
        let walk = ((ctx.time * 4.0) as u32) % 4;
        for (i, key) in [
            "units/short_infantry_player",
            "units/archer_ally",
            "units/light_cavalry_enemy",
            "units/sorcerer_player",
        ]
        .iter()
        .enumerate()
        {
            let facing = (i as u32) % 4;
            draw_sprite(
                ctx,
                key,
                vec2(16.0, 16.0),
                (facing, walk),
                vec2(s.x + 12.0 + i as f32 * 34.0, s.y + 24.0),
                false,
            );
        }
        let img = Rect::new(318.0, s.bottom() + 6.0, 154.0, 64.0);
        draw_window_ex(img, WindowStyle::Panel, 1.0);
        draw_image(ctx, "bg/palace", inset(img, 4.0), Fit::Cover);
        gfx.text("bg/palace (Cover)", img.x + 6.0, img.bottom() - 16.0, label);
        let p1 = Rect::new(318.0, img.bottom() + 6.0, 64.0, 80.0);
        draw_portrait(ctx, Some("liu_bei"), p1);
        let p2 = Rect::new(p1.right() + 8.0, p1.y, 64.0, 80.0);
        draw_portrait(ctx, None, p2);
        gfx.text(
            "초상화 64×80 · 없으면 실루엣",
            p1.x,
            p1.bottom() + 1.0,
            label,
        );

        // Tooltip for the hovered menu row.
        if self.hover.visible() {
            if let (Some(id), Some(p)) = (self.hover.target(), ctx.input.pointer()) {
                if let Some(item) = DEMO_ITEMS.get(id as usize) {
                    draw_tooltip(gfx, item.4, p);
                }
            }
        }
    }

    fn draw_dialogue(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        match ctx.media.texture_state("bg/palace") {
            AssetState::Ready => draw_image(ctx, "bg/palace", SCREEN, Fit::Cover),
            _ => draw_backdrop(ctx.time),
        }
        for b in &DIALOGUE_BUTTONS {
            b.draw(ctx);
        }
        gfx.text(
            &format!("결과: {}", self.result),
            300.0,
            TOP + 4.0,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        if let Some(message) = &self.message {
            message.draw(ctx);
        }
        match &self.modal {
            Modal::None => {}
            Modal::Choice(choice) => choice.draw(ctx),
            Modal::Confirm(dialog) => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.35));
                dialog.draw(ctx);
            }
        }
    }

    fn draw_gauges(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let label = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);

        let g = Rect::new(8.0, TOP, 234.0, VIRTUAL_H - TOP - 8.0);
        draw_window(g);
        let x = g.x + 10.0;
        let w = g.w - 20.0;
        let mut y = g.y + 6.0;
        let wave = ((ctx.time * 0.8).sin() as f32 + 1.0) / 2.0;
        let rows: [(&str, i64, i64, GaugeKind); 8] = [
            ("병력", 1500, 1500, GaugeKind::Hp),
            ("병력 (60%)", 900, 1500, GaugeKind::Hp),
            ("병력 (40%)", 600, 1500, GaugeKind::Hp),
            ("병력 (10%)", 150, 1500, GaugeKind::Hp),
            (
                "병력 (변화)",
                (wave * 1500.0).round() as i64,
                1500,
                GaugeKind::Hp,
            ),
            ("책략 MP", 18, 30, GaugeKind::Mp),
            ("경험치", 72, 100, GaugeKind::Exp),
            ("사기", 80, 100, GaugeKind::Morale),
        ];
        for (name, value, max, kind) in rows {
            draw_gauge_labeled(gfx, vec2(x, y), w, name, value, max, kind);
            y += 23.0;
        }
        gfx.text("draw_gauge (막대만)", x, y, label);
        y += 13.0;
        for (i, kind) in [
            GaugeKind::Hp,
            GaugeKind::Mp,
            GaugeKind::Exp,
            GaugeKind::Custom(theme::TEXT_ACCENT),
        ]
        .into_iter()
        .enumerate()
        {
            let bw = (w - 18.0) / 4.0;
            draw_gauge(
                Rect::new(x + i as f32 * (bw + 6.0), y, bw, 8.0),
                wave * 100.0,
                100.0,
                kind,
            );
        }

        // Number and time formatting.
        let n = Rect::new(250.0, TOP, 222.0, 166.0);
        draw_window(n);
        let now = unix_now();
        let rows: [(&str, String); 9] = [
            ("thousands", format::thousands(1_234_567)),
            ("음수", format::thousands(-45_000)),
            ("ratio", format::ratio(1200, 1500)),
            ("percent", format::percent(80)),
            ("play_time", format::play_time(3 * 3600 + 25 * 60 + 7)),
            (
                "5분 전",
                format::relative_time(now.saturating_sub(300), now),
            ),
            (
                "3시간 전",
                format::relative_time(now.saturating_sub(3 * 3600), now),
            ),
            (
                "40일 전",
                format::relative_time(now.saturating_sub(40 * 86_400), now),
            ),
            ("오늘 (UTC)", format::date_utc(now)),
        ];
        let mut y = n.y + 6.0;
        for (name, value) in rows {
            gfx.text(name, n.x + 10.0, y + 2.0, label);
            gfx.text_aligned(&value, n.x, y, n.w - 10.0, Align::Right, main);
            y += 17.0;
        }

        for b in &GAUGE_BUTTONS {
            b.draw(ctx);
        }
        gfx.text(
            "토스트는 앱이, 배너는 화면이 그립니다.",
            250.0,
            218.0,
            label,
        );
        if let Some(banner) = &self.banner {
            banner.draw(gfx);
        }
    }

    fn draw_screens(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        draw_backdrop(ctx.time);
        self.screens_menu.draw(ctx);
        let info = Rect::new(230.0, TOP, 242.0, 150.0);
        draw_window(info);
        let label = TextStyle::small(theme::TEXT_DIM);
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let lines = [
            "실제 화면을 그대로 엽니다.",
            "설정·제작진·게임 오버는 X/Esc로",
            "돌아옵니다. 타이틀로 가면 갤러리로",
            "돌아오지 않습니다.",
        ];
        gfx.text_lines(&lines, info.x + 10.0, info.y + 6.0, main);
        let mut y = info.y + 6.0 + 16.0 * lines.len() as f32 + 6.0;
        draw_divider(info.x + 8.0, y, info.w - 16.0);
        y += 6.0;
        let music = match ctx.audio.bgm() {
            Some(key) => {
                let state = match ctx.media.sound_state(&format!("bgm/{key}")) {
                    AssetState::Ready => "재생",
                    AssetState::Loading => "불러오는 중",
                    AssetState::Missing => "파일 없음",
                };
                format!("배경음: {key} ({state})")
            }
            None => "배경음: 없음".into(),
        };
        gfx.text(&music, info.x + 10.0, y, main);
        y += 16.0;
        let unlock = if ctx.audio.unlocked() {
            "오디오 사용 가능"
        } else {
            "오디오 잠김 — 첫 입력 후 재생 (웹)"
        };
        gfx.text(unlock, info.x + 10.0, y, label);
        y += 13.0;
        gfx.text(
            &format!("데이터: {}", ctx.data_root.display()),
            info.x + 10.0,
            y,
            label,
        );
    }
}

impl Default for GalleryScreen {
    fn default() -> Self {
        GalleryScreen::new()
    }
}

impl Screen for GalleryScreen {
    fn name(&self) -> &'static str {
        "gallery"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            self.rewrap(ctx);
            let items = self.demo_items();
            self.demo_menu = Menu::new(items).at(160.0, TOP, 150.0).rows(9);
            self.demo_menu.tag_width = 0.0;
            self.start_sample(ctx);
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if self.switch_page(ctx) {
            return Transition::None;
        }
        match self.page {
            PAGE_FONTS => self.update_fonts(ctx),
            PAGE_WIDGETS => self.update_widgets(ctx),
            PAGE_DIALOGUE => self.update_dialogue(ctx),
            PAGE_GAUGES => self.update_gauges(ctx),
            PAGE_SCREENS => return self.update_screens(ctx),
            _ => {}
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        fill_rect(SCREEN, theme::BACKGROUND);
        match self.page {
            PAGE_FONTS => self.draw_fonts(ctx),
            PAGE_WIDGETS => self.draw_widgets(ctx),
            PAGE_DIALOGUE => self.draw_dialogue(ctx),
            PAGE_GAUGES => self.draw_gauges(ctx),
            PAGE_SCREENS => self.draw_screens(ctx),
            _ => {}
        }
        self.draw_tabs(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_tile_the_top_edge() {
        let last = GalleryScreen::tab_rect(TABS.len() - 1);
        assert_eq!(GalleryScreen::tab_rect(0).x, 0.0);
        assert_eq!(last.right(), VIRTUAL_W);
        for i in 1..TABS.len() {
            assert_eq!(
                GalleryScreen::tab_rect(i - 1).right(),
                GalleryScreen::tab_rect(i).x
            );
        }
    }

    #[test]
    fn demo_menu_items_reflect_values() {
        let mut g = GalleryScreen::new();
        g.volume = 30;
        g.text_speed = TextSpeed::Fast;
        let items = g.demo_items();
        assert_eq!(items.len(), DEMO_ITEMS.len());
        assert_eq!(items[5].detail.as_deref(), Some("30%"));
        assert_eq!(items[6].detail.as_deref(), Some("빠름"));
        assert!(!items[4].enabled);
        assert!(items[5].adjustable && items[6].adjustable);
    }
}
