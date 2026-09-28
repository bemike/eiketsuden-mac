//! Credits: scrolls the pack credits followed by the engine credits. The pack credits are every
//! `credits.txt` of the chain, nearest first (the original-data overlay, the top pack, then each
//! pack it extends, e.g. `data/base/credits.txt`), so a mod does not hide the attributions of
//! the pack it builds on; when no pack has such a file only the engine credits are shown.
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
use crate::assets::AllOf;
use crate::audio::bgm;
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, FontId, TextStyle};
use crate::input::Dir;
use crate::ui::theme;
use macroquad::prelude::*;

/// Automatic scroll speed in virtual pixels per second.
const SCROLL_SPEED: f32 = 18.0;
/// Space kept free left and right of the centred lines, together.
const TEXT_SIDE_MARGINS: f32 = 80.0;

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

/// The pack credits: every readable `credits.txt` of the chain (nearest first), each once, or
/// `None` when there is none. A file that is not UTF-8 is skipped with a warning.
fn join_credits(results: Vec<Result<Vec<u8>, String>>) -> Option<String> {
    let mut texts: Vec<String> = Vec::new();
    for bytes in results.into_iter().flatten() {
        match String::from_utf8(bytes) {
            Ok(s) if s.trim().is_empty() || texts.contains(&s) => {}
            Ok(s) => texts.push(s),
            Err(_) => macroquad::logging::warn!("a credits.txt is not valid UTF-8; skipped"),
        }
    }
    (!texts.is_empty()).then(|| {
        texts
            .iter()
            .map(|t| t.trim_end())
            .collect::<Vec<_>>()
            .join("\n\n\n")
    })
}

pub struct CreditsScreen {
    ending: Option<String>,
    request: Option<AllOf>,
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
        let width = gfx.size().x - TEXT_SIDE_MARGINS;
        self.lines = layout(&text, |s| gfx.wrap(s, FontId::Main, 1, width));
        self.total_height = self.lines.iter().map(Line::height).sum();
    }

    /// Largest scroll offset on a canvas `canvas_h` pixels high: the roll starts below the
    /// bottom edge and ends when the last line has passed the middle of the screen.
    fn max_scroll(&self, canvas_h: f32) -> f32 {
        (self.total_height + canvas_h / 2.0).max(0.0)
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
            self.request = Some(AllOf::new(ctx.data_root.media_paths("credits.txt")));
            if self.ending.is_some() {
                ctx.audio.play_bgm(bgm::ENDING);
            }
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some(req) = self.request.as_mut() {
            let Some(results) = req.poll() else {
                return Transition::None;
            };
            let pack_credits = join_credits(results);
            if pack_credits.is_none() {
                macroquad::logging::info!("no pack credits; showing engine credits");
            }
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
        let max_scroll = self.max_scroll(ctx.gfx.size().y);
        self.scroll = (self.scroll + manual).clamp(0.0, max_scroll);

        let at_end = self.scroll >= max_scroll;
        if input.cancel() || (input.confirm() && (at_end || self.ending.is_none())) {
            return self.finish();
        }
        if at_end && self.ending.is_some() && self.hold <= 0.0 {
            self.hold = 0.0;
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let (w, canvas_h) = (gfx.size().x, gfx.size().y);
        draw_backdrop(gfx.size(), ctx.time);
        fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.03, 0.55));
        // Lines start below the screen and move up.
        let mut y = canvas_h - self.scroll;
        for line in &self.lines {
            let h = line.height();
            if y + h > -40.0 && y < canvas_h + 4.0 {
                match line {
                    Line::Heading(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y,
                        w,
                        Align::Center,
                        TextStyle::main(theme::TEXT_ACCENT)
                            .size(2)
                            .shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Section(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y + 2.0,
                        w,
                        Align::Center,
                        TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Text(t) => gfx.text_aligned(
                        t,
                        0.0,
                        y,
                        w,
                        Align::Center,
                        TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW),
                    ),
                    Line::Blank => {}
                }
            }
            y += h;
        }
        if self.scroll >= self.max_scroll(canvas_h) && (ctx.time * 2.0).fract() < 0.7 {
            gfx.text_aligned(
                "Z / 클릭: 돌아가기",
                0.0,
                canvas_h - 18.0,
                w,
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
    fn every_pack_of_the_chain_is_credited() {
        let ok = |s: &str| Ok(s.as_bytes().to_vec());
        let joined = join_credits(vec![
            Err("overlay: missing".into()),
            ok("# Mod\nme\n"),
            ok("# Base\nthem\n"),
        ]);
        assert_eq!(joined.as_deref(), Some("# Mod\nme\n\n\n# Base\nthem"));
        // The same file twice (an overlay copy) once; empty and non-UTF-8 files are skipped.
        let joined = join_credits(vec![
            ok("# Base\n"),
            Ok(vec![0xff]),
            ok("  \n"),
            ok("# Base\n"),
        ]);
        assert_eq!(joined.as_deref(), Some("# Base"));
        assert_eq!(join_credits(vec![Err("missing".into())]), None);
    }

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
