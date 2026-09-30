//! Backlog (최근 대사): the recent dialogue of a drama scene and a scrollable window showing it.
//!
//! [`Backlog`] records what was said; [`BacklogView`] is the modal window (arrows / PageUp /
//! PageDown / wheel / drag scroll, confirm, cancel or a tap closes it). It opens scrolled to
//! the newest line, at the bottom.

use super::theme;
use super::window::{draw_small_arrow, draw_window, inset};
use crate::app::Ctx;
use crate::audio::sfx;
use crate::gfx::{fill_rect, Align, FontId, Gfx, TextStyle};
use crate::input::Dir;
use macroquad::prelude::*;
use std::collections::VecDeque;

/// Entries kept per scene; older ones are dropped.
pub const BACKLOG_CAPACITY: usize = 120;

/// One remembered line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogEntry {
    /// Speaker name; `None` for narration and notices.
    pub speaker: Option<String>,
    pub text: String,
}

/// Recent lines, oldest first, at most `capacity` of them.
#[derive(Debug, Clone)]
pub struct Backlog {
    entries: VecDeque<BacklogEntry>,
    capacity: usize,
}

impl Default for Backlog {
    fn default() -> Self {
        Backlog::new(BACKLOG_CAPACITY)
    }
}

impl Backlog {
    pub fn new(capacity: usize) -> Backlog {
        Backlog {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Remember a line (empty text is ignored).
    pub fn push(&mut self, speaker: Option<&str>, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(BacklogEntry {
            speaker: speaker.filter(|s| !s.is_empty()).map(str::to_string),
            text: text.to_string(),
        });
    }

    pub fn entries(&self) -> impl Iterator<Item = &BacklogEntry> {
        self.entries.iter()
    }

    /// A backlog holding `lines` (speaker, text), oldest first: the lines a quick save kept.
    /// Only the newest [`BACKLOG_CAPACITY`] of them are taken, as if they had been pushed.
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) -> Backlog {
        let mut log = Backlog::default();
        for (speaker, text) in lines {
            log.push(speaker, text);
        }
        log
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A laid-out row of the backlog window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklogLine {
    Speaker(String),
    Text(String),
    /// Space between two entries.
    Gap,
}

impl BacklogLine {
    pub fn height(&self) -> f32 {
        match self {
            BacklogLine::Speaker(_) | BacklogLine::Text(_) => 16.0,
            BacklogLine::Gap => 6.0,
        }
    }
}

/// Lay the entries out as rows; `wrap` breaks a text into lines of the window width.
pub fn layout<'a>(
    entries: impl IntoIterator<Item = &'a BacklogEntry>,
    mut wrap: impl FnMut(&str) -> Vec<String>,
) -> Vec<BacklogLine> {
    let mut out = Vec::new();
    for (i, e) in entries.into_iter().enumerate() {
        if i > 0 {
            out.push(BacklogLine::Gap);
        }
        if let Some(s) = &e.speaker {
            out.push(BacklogLine::Speaker(s.clone()));
        }
        out.extend(wrap(&e.text).into_iter().map(BacklogLine::Text));
    }
    out
}

/// Largest scroll offset for `content` pixels in a `view` pixel tall viewport.
pub fn max_scroll(content: f32, view: f32) -> f32 {
    (content - view).max(0.0)
}

/// The backlog window on a `canvas` sized canvas: centred, 28 pixels in from the sides and 14
/// from the top and bottom.
fn window_rect(canvas: Vec2) -> Rect {
    Rect::new(28.0, 14.0, canvas.x - 56.0, canvas.y - 28.0)
}
const HEADER_H: f32 = 22.0;
const TEXT_INDENT: f32 = 10.0;

/// The modal backlog window.
#[derive(Debug, Clone)]
pub struct BacklogView {
    /// The window, laid out for the canvas it was opened on.
    window: Rect,
    lines: Vec<BacklogLine>,
    content_h: f32,
    scroll: f32,
}

impl BacklogView {
    /// Lay out `backlog`, scrolled to the newest line.
    pub fn open(gfx: &Gfx, backlog: &Backlog) -> BacklogView {
        let window = window_rect(gfx.size());
        let width = Self::viewport_of(window).w - TEXT_INDENT - 4.0;
        let lines = layout(backlog.entries(), |t| {
            super::dialogue::layout_text(t, width, |c| gfx.char_width(c, FontId::Main, 1))
        });
        let content_h = lines.iter().map(BacklogLine::height).sum();
        let mut view = BacklogView {
            window,
            lines,
            content_h,
            scroll: 0.0,
        };
        view.scroll = view.max_scroll();
        view
    }

    /// Text area of a backlog `window` (below its header).
    fn viewport_of(window: Rect) -> Rect {
        let inner = inset(window, theme::PADDING + 2.0);
        Rect::new(inner.x, inner.y + HEADER_H, inner.w, inner.h - HEADER_H)
    }

    fn viewport(&self) -> Rect {
        Self::viewport_of(self.window)
    }

    fn max_scroll(&self) -> f32 {
        max_scroll(self.content_h, self.viewport().h)
    }

    fn scroll_by(&mut self, dy: f32) {
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
    }

    /// Handle input; returns `true` when the window should close.
    pub fn update(&mut self, ctx: &mut Ctx) -> bool {
        let input = &ctx.input;
        if input.cancel() || input.confirm_key() {
            ctx.sfx(sfx::CANCEL);
            return true;
        }
        // A tap closes it too (dragging scrolls, so taps are free for this on touch screens).
        if input.tap().is_some() {
            ctx.sfx(sfx::CANCEL);
            return true;
        }
        let page = self.viewport().h - 16.0;
        let mut dy = 0.0;
        match input.nav() {
            Some(Dir::Up) => dy -= 16.0,
            Some(Dir::Down) => dy += 16.0,
            _ => {}
        }
        if input.key_pressed(KeyCode::PageUp) {
            dy -= page;
        }
        if input.key_pressed(KeyCode::PageDown) {
            dy += page;
        }
        dy += input.wheel() as f32 * 16.0;
        if let Some(d) = input.drag() {
            dy -= d.delta.y;
        }
        self.scroll_by(dy);
        false
    }

    pub fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        fill_rect(gfx.screen(), Color::new(0.0, 0.0, 0.02, 0.55));
        draw_window(self.window);
        let inner = inset(self.window, theme::PADDING + 2.0);
        gfx.text(
            "최근 대사",
            inner.x + 2.0,
            inner.y,
            TextStyle::main(theme::TEXT_ACCENT).shadow(theme::TEXT_SHADOW),
        );
        gfx.text_aligned(
            "방향키·휠·끌기 스크롤 · X·터치 닫기",
            inner.x,
            inner.y + 2.0,
            inner.w - 2.0,
            Align::Right,
            TextStyle::small(theme::TEXT_DIM),
        );
        super::window::draw_divider(inner.x, inner.y + HEADER_H - 5.0, inner.w);

        let view = self.viewport();
        if self.lines.is_empty() {
            gfx.text_aligned(
                "아직 대사가 없습니다.",
                view.x,
                view.y + view.h / 2.0 - 8.0,
                view.w,
                Align::Center,
                TextStyle::main(theme::TEXT_DIM),
            );
            return;
        }
        let name = TextStyle::main(theme::TEXT_NAME).shadow(theme::TEXT_SHADOW);
        let text = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let mut y = view.y - self.scroll;
        for line in &self.lines {
            let h = line.height();
            // Only rows fully inside the viewport are drawn (no clipping support needed).
            if y >= view.y - 0.5 && y + h <= view.bottom() + 0.5 {
                match line {
                    BacklogLine::Speaker(s) => {
                        gfx.text(s, view.x, y, name);
                    }
                    BacklogLine::Text(t) => {
                        gfx.text(t, view.x + TEXT_INDENT, y, text);
                    }
                    BacklogLine::Gap => {}
                }
            }
            y += h;
        }
        let cx = view.x + view.w / 2.0;
        if self.scroll > 0.5 {
            draw_small_arrow(cx, view.y - 3.0, false, theme::TEXT_ACCENT);
        }
        if self.scroll < self.max_scroll() - 0.5 {
            draw_small_arrow(cx, view.bottom() + 3.0, true, theme::TEXT_ACCENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_entries() {
        let mut b = Backlog::new(2);
        b.push(Some("유비"), "첫째");
        b.push(None, "둘째");
        b.push(Some(""), "셋째");
        b.push(Some("관우"), "   ");
        assert_eq!(b.len(), 2);
        let v: Vec<_> = b.entries().cloned().collect();
        assert_eq!(v[0].text, "둘째");
        assert_eq!(v[1].speaker, None);
        assert_eq!(v[1].text, "셋째");
    }

    /// A quick save keeps the backlog as lines; loading rebuilds it the way it was built.
    #[test]
    fn a_backlog_is_rebuilt_from_saved_lines() {
        let mut original = Backlog::default();
        original.push(Some("유비"), "가자.");
        original.push(None, "밤이 깊었다");
        let saved: Vec<(Option<String>, String)> = original
            .entries()
            .map(|e| (e.speaker.clone(), e.text.clone()))
            .collect();
        let rebuilt = Backlog::from_lines(saved.iter().map(|(s, t)| (s.as_deref(), t.as_str())));
        assert_eq!(
            rebuilt.entries().collect::<Vec<_>>(),
            original.entries().collect::<Vec<_>>()
        );

        // More lines than the capacity keeps the newest, as pushing them would.
        let many: Vec<String> = (0..BACKLOG_CAPACITY + 5)
            .map(|i| format!("줄 {i}"))
            .collect();
        let long = Backlog::from_lines(many.iter().map(|t| (None, t.as_str())));
        assert_eq!(long.len(), BACKLOG_CAPACITY);
        assert_eq!(long.entries().next().unwrap().text, "줄 5");
    }

    #[test]
    fn layout_rows_and_scrolling() {
        let mut b = Backlog::default();
        b.push(Some("장비"), "형님 a b");
        b.push(None, "밤이 깊었다");
        let rows = layout(b.entries(), |t| t.split(' ').map(str::to_string).collect());
        assert_eq!(
            rows,
            vec![
                BacklogLine::Speaker("장비".into()),
                BacklogLine::Text("형님".into()),
                BacklogLine::Text("a".into()),
                BacklogLine::Text("b".into()),
                BacklogLine::Gap,
                BacklogLine::Text("밤이".into()),
                BacklogLine::Text("깊었다".into()),
            ]
        );
        let h: f32 = rows.iter().map(BacklogLine::height).sum();
        assert_eq!(h, 6.0 * 16.0 + 6.0);
        assert_eq!(max_scroll(h, 200.0), 0.0);
        assert_eq!(max_scroll(h, 50.0), h - 50.0);
    }
}
