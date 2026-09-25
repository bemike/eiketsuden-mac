//! Credits: scrolls the pack's `credits.txt` (in the pack directory, e.g.
//! `data/base/credits.txt`) followed by the engine credits; when the pack has no such file only
//! the engine credits are shown.
//!
//! `credits.txt` is UTF-8 plain text with two markup rules: a line starting with `# ` is a large
//! heading and a line starting with `## ` a section heading. Long lines wrap.
//!
//! [`CreditsScreen::new`] is opened from the title screen and returns to it;
//! [`CreditsScreen::ending`] is the campaign's ending roll (ending title first, ending music) and
//! continues to the title screen. The text scrolls by itself; arrows, wheel and drag scroll
//! manually; confirm or cancel leaves (at the end, or immediately from the title menu).

use super::backdrop::draw_backdrop;
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::FileRequest;
use crate::audio::bgm;
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, FontId, TextStyle, SCREEN, VIRTUAL_H, VIRTUAL_W};
use crate::input::Dir;
use crate::ui::theme;
use macroquad::prelude::*;

/// Automatic scroll speed in virtual pixels per second.
const SCROLL_SPEED: f32 = 18.0;
const TEXT_WIDTH: f32 = 400.0;

/// The engine's own credits, shown after the pack credits.
pub fn engine_credits() -> String {
    format!(
        "# 영걸전 Reloaded\n\
         Eiketsuden Reloaded {version}\n\
         \n\
         ## 엔진\n\
         Rust + macroquad (MIT / Apache-2.0)\n\
         엔진 코드: GPL-3.0-or-later\n\
         {repo}\n\
         \n\
         ## 글꼴\n\
         Galmuri — Minseo Lee (quiple), SIL Open Font License 1.1\n\
         \n\
         ## 안내\n\
         이 게임은 KOEI의 『삼국지 영걸전』(1995)에서 영감을 받은 오픈소스 재구현입니다. \
         KOEI TECMO의 그래픽·음악·텍스트·프로그램을 포함하지 않으며, KOEI TECMO와는 관계가 없습니다.\n\
         \n\
         콘텐츠 제작진은 데이터 팩의 credits.txt와 CREDITS.md에 있습니다.",
        version = env!("CARGO_PKG_VERSION"),
        repo = env!("CARGO_PKG_REPOSITORY"),
    )
}

#[derive(Debug, Clone, PartialEq)]
enum Line {
    Heading(String),
    Section(String),
    Text(String),
    Blank,
}

impl Line {
    fn height(&self) -> f32 {
        match self {
            Line::Heading(_) => 34.0,
            Line::Section(_) => 20.0,
            Line::Text(_) => 16.0,
            Line::Blank => 10.0,
        }
    }
}

/// Parse credits markup into lines, wrapping text with `wrap`.
fn layout(text: &str, mut wrap: impl FnMut(&str) -> Vec<String>) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end();
        if let Some(h) = raw.strip_prefix("# ") {
            out.push(Line::Heading(h.trim().to_string()));
        } else if let Some(s) = raw.strip_prefix("## ") {
            out.push(Line::Section(s.trim().to_string()));
        } else if raw.trim().is_empty() {
            out.push(Line::Blank);
        } else {
            out.extend(wrap(raw).into_iter().map(Line::Text));
        }
    }
    out
}

pub struct CreditsScreen {
    ending: Option<String>,
    request: Option<FileRequest>,
    lines: Vec<Line>,
    total_height: f32,
    scroll: f32,
    hold: f32,
}

impl CreditsScreen {
    /// Credits from the title screen.
    pub fn new() -> CreditsScreen {
        CreditsScreen {
            ending: None,
            request: None,
            lines: Vec::new(),
            total_height: 0.0,
            scroll: 0.0,
            hold: 0.0,
        }
    }

    /// Ending roll after the campaign; `title` is the ending's name (may be empty).
    pub fn ending(title: String) -> CreditsScreen {
        CreditsScreen {
            ending: Some(title),
            ..CreditsScreen::new()
        }
    }

    fn set_text(&mut self, ctx: &Ctx, pack_credits: Option<String>) {
        let mut text = String::new();
        if let Some(title) = self.ending.as_deref().filter(|t| !t.is_empty()) {
            text.push_str(&format!("# {title}\n\n"));
        }
        if let Some(c) = pack_credits {
            text.push_str(&c);
            text.push_str("\n\n\n");
        }
        text.push_str(&engine_credits());
        let gfx = &ctx.gfx;
        self.lines = layout(&text, |s| gfx.wrap(s, FontId::Main, 1, TEXT_WIDTH));
        self.total_height = self.lines.iter().map(Line::height).sum();
    }

    fn max_scroll(&self) -> f32 {
        // Scroll until the last line has passed the middle of the screen.
        (self.total_height + VIRTUAL_H / 2.0).max(0.0)
    }

    fn finish(&self) -> Transition {
        if self.ending.is_some() {
            Transition::Flow(Flow::Title)
        } else {
            Transition::Pop
        }
    }
}

impl Default for CreditsScreen {
    fn default() -> Self {
        CreditsScreen::new()
    }
}

impl Screen for CreditsScreen {
    fn name(&self) -> &'static str {
        "credits"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how == Enter::Fresh {
            self.request = Some(FileRequest::new(ctx.data_root.path("credits.txt")));
            if self.ending.is_some() {
                ctx.audio.play_bgm(bgm::ENDING);
            }
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some(req) = self.request.as_mut() {
            let Some(result) = req.poll() else {
                return Transition::None;
            };
            let pack_credits = match result {
                Ok(bytes) => match String::from_utf8(bytes.clone()) {
                    Ok(s) => Some(s),
                    Err(_) => {
                        macroquad::logging::warn!("credits.txt is not valid UTF-8; skipped");
                        None
                    }
                },
                Err(e) => {
                    macroquad::logging::info!("no pack credits ({}); showing engine credits", e);
                    None
                }
            };
            self.request = None;
            self.set_text(ctx, pack_credits);
        }

        let input = &ctx.input;
        let mut manual = 0.0;
        match input.nav() {
            Some(Dir::Up) => manual -= 16.0,
            Some(Dir::Down) => manual += 16.0,
            _ => {}
        }
        manual += input.wheel() as f32 * 16.0;
        if let Some(d) = input.drag() {
            manual -= d.delta.y;
        }
        if manual != 0.0 {
            self.hold = 2.0;
        }
        if self.hold > 0.0 {
            self.hold -= ctx.dt;
        } else {
            self.scroll += SCROLL_SPEED * ctx.dt;
        }
        self.scroll = (self.scroll + manual).clamp(0.0, self.max_scroll());

        let at_end = self.scroll >= self.max_scroll();
        if input.cancel() || (input.confirm() && (at_end || self.ending.is_none())) {
            return self.finish();
        }
        if at_end && self.ending.is_some() && self.hold <= 0.0 {
            self.hold = 0.0;
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        draw_backdrop(ctx.time);
        fill_rect(SCREEN, Color::new(0.0, 0.0, 0.03, 0.55));
        let gfx = &ctx.gfx;
        // Lines start below the screen and move up.
        let mut y = VIRTUAL_H - self.scroll;
        for line in &self.lines {
            let h = line.height();
            if y + h > -40.0 && y < VIRTUAL_H + 4.0 {
                match line {
                    Line::Heading(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y,
                        VIRTUAL_W,
                        Align::Center,
                        TextStyle::main(theme::TEXT_ACCENT)
                            .size(2)
                            .shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Section(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y + 2.0,
                        VIRTUAL_W,
                        Align::Center,
                        TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Text(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y,
                        VIRTUAL_W,
                        Align::Center,
                        TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Blank => {}
                }
            }
            y += h;
        }
        if self.scroll >= self.max_scroll() && (ctx.time * 2.0).fract() < 0.7 {
            gfx.text_aligned(
                "Z / 클릭: 돌아가기",
                0.0,
                VIRTUAL_H - 18.0,
                VIRTUAL_W,
                Align::Center,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_markup() {
        let lines = layout("# Title\n## Art\nAlice, Bob\n\nMore", |s| {
            vec![s.to_string()]
        });
        assert_eq!(
            lines,
            vec![
                Line::Heading("Title".into()),
                Line::Section("Art".into()),
                Line::Text("Alice, Bob".into()),
                Line::Blank,
                Line::Text("More".into()),
            ]
        );
    }

    #[test]
    fn engine_credits_mention_license_and_fonts() {
        let c = engine_credits();
        assert!(c.contains("GPL-3.0"));
        assert!(c.contains("Galmuri"));
    }
}
