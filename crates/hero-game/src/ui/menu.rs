//! Vertical menu: keyboard, mouse wheel, hover, tap and drag scrolling, disabled items,
//! adjustable values (◀ value ▶) and cancel.
//!
//! ```ignore
//! let mut menu = Menu::new(vec![MenuItem::new("새 게임"), MenuItem::new("종료")])
//!     .at(180.0, 120.0, 120.0);
//! match menu.update(ctx) {
//!     MenuEvent::Selected(0) => { /* ... */ }
//!     MenuEvent::Cancelled => { /* ... */ }
//!     _ => {}
//! }
//! menu.draw(ctx);
//! ```
//!
//! The menu plays the UI sounds itself (`cursor`, `confirm`, `cancel`, `error` for disabled
//! items).

use super::theme;
use super::window::{
    draw_arrow_cursor, draw_highlight, draw_side_arrow, draw_small_arrow, draw_window_ex,
    WindowStyle,
};
use crate::app::Ctx;
use crate::audio::sfx;
use crate::gfx::{FontId, Gfx, TextStyle};
use crate::input::Dir;
use macroquad::prelude::*;

/// Width reserved at the left of each row for the arrow cursor.
const CURSOR_GUTTER: f32 = 12.0;
/// Width of the ◀ / ▶ hit areas of adjustable items.
const ARROW_W: f32 = 12.0;

#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    /// Short text in a fixed-width column before the label (slot names, counts, ...); the
    /// column width is [`Menu::tag_width`].
    pub tag: Option<String>,
    pub label: String,
    /// Right-aligned value text (e.g. `80%`, a price, a date).
    pub detail: Option<String>,
    pub enabled: bool,
    /// Left/right (or tapping the arrows) changes the value: emits [`MenuEvent::Adjust`].
    pub adjustable: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> MenuItem {
        MenuItem {
            tag: None,
            label: label.into(),
            detail: None,
            enabled: true,
            adjustable: false,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> MenuItem {
        self.enabled = enabled;
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> MenuItem {
        self.detail = Some(detail.into());
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> MenuItem {
        self.tag = Some(tag.into());
        self
    }

    pub fn adjustable(mut self) -> MenuItem {
        self.adjustable = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEvent {
    None,
    /// The cursor moved (keyboard, wheel or hover).
    Moved(usize),
    /// An enabled item was chosen (confirm key or tap).
    Selected(usize),
    /// An adjustable item's value should change by `-1` or `+1`.
    Adjust(usize, i32),
    /// Cancel was pressed (only when [`Menu::cancellable`]).
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub items: Vec<MenuItem>,
    cursor: usize,
    scroll: usize,
    /// Rows shown at once; more items scroll.
    pub visible_rows: usize,
    /// Cancel emits [`MenuEvent::Cancelled`].
    pub cancellable: bool,
    /// Moving past the last item wraps to the first.
    pub wrap: bool,
    /// Draw the window frame behind the rows.
    pub framed: bool,
    /// Whether the menu has focus (inactive menus draw a dim cursor and ignore input).
    pub active: bool,
    pub font: FontId,
    pub row_height: f32,
    /// Width of the tag column (see [`MenuItem::tag`]).
    pub tag_width: f32,
    pos: Vec2,
    width: f32,
    drag_accum: f32,
}

impl Menu {
    pub fn new(items: Vec<MenuItem>) -> Menu {
        let rows = items.len().max(1);
        let mut menu = Menu {
            items,
            cursor: 0,
            scroll: 0,
            visible_rows: rows,
            cancellable: true,
            wrap: true,
            framed: true,
            active: true,
            font: FontId::Main,
            row_height: theme::ROW_HEIGHT,
            tag_width: 0.0,
            pos: Vec2::ZERO,
            width: 120.0,
            drag_accum: 0.0,
        };
        menu.cursor = menu.first_enabled().unwrap_or(0);
        menu
    }

    /// Place the window's top-left corner and set its width.
    pub fn at(mut self, x: f32, y: f32, width: f32) -> Menu {
        self.pos = vec2(x, y);
        self.width = width;
        self
    }

    /// Limit the number of visible rows (the rest scrolls).
    pub fn rows(mut self, visible: usize) -> Menu {
        self.visible_rows = visible.max(1);
        self.ensure_visible();
        self
    }

    pub fn cancellable(mut self, cancellable: bool) -> Menu {
        self.cancellable = cancellable;
        self
    }

    /// Move the window.
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.pos = vec2(x, y);
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width;
    }

    /// Width that fits every label and detail.
    pub fn fit_width(&self, gfx: &Gfx) -> f32 {
        let widest = self
            .items
            .iter()
            .map(|it| {
                let label = gfx.text_width(&it.label, self.font, 1);
                let detail = it
                    .detail
                    .as_deref()
                    .map(|d| gfx.text_width(d, self.font, 1) + 12.0)
                    .unwrap_or(0.0);
                let arrows = if it.adjustable { 2.0 * ARROW_W } else { 0.0 };
                label + detail + arrows + self.tag_width
            })
            .fold(0.0, f32::max);
        (widest + CURSOR_GUTTER + 2.0 * theme::PADDING + 6.0).ceil()
    }

    /// Outer rectangle of the menu window.
    pub fn rect(&self) -> Rect {
        let rows = self.visible_rows.min(self.items.len().max(1));
        let pad = if self.framed { theme::PADDING } else { 0.0 };
        Rect::new(
            self.pos.x,
            self.pos.y,
            self.width,
            rows as f32 * self.row_height + 2.0 * pad,
        )
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.cursor = index.min(self.items.len() - 1);
            self.ensure_visible();
        }
    }

    /// Replace the items, keeping the cursor position where possible.
    pub fn set_items(&mut self, items: Vec<MenuItem>) {
        self.items = items;
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        self.ensure_visible();
    }

    fn first_enabled(&self) -> Option<usize> {
        self.items.iter().position(|i| i.enabled)
    }

    fn ensure_visible(&mut self) {
        let rows = self.visible_rows.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
        let max_scroll = self.items.len().saturating_sub(rows);
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Index of the next enabled item in direction `step` (±1), honouring `wrap`.
    fn step_cursor(&self, step: i32) -> Option<usize> {
        next_enabled(
            &self.items.iter().map(|i| i.enabled).collect::<Vec<_>>(),
            self.cursor,
            step,
            self.wrap,
        )
    }

    /// Rectangle of the row showing item `index` (it may be scrolled out of view).
    pub fn row_rect(&self, index: usize) -> Rect {
        let r = self.rect();
        let pad = if self.framed { theme::PADDING } else { 0.0 };
        let row = index as f32 - self.scroll as f32;
        Rect::new(
            r.x + pad - 2.0,
            r.y + pad + row * self.row_height,
            r.w - 2.0 * pad + 4.0,
            self.row_height,
        )
    }

    fn visible_range(&self) -> std::ops::Range<usize> {
        let end = (self.scroll + self.visible_rows).min(self.items.len());
        self.scroll..end
    }

    fn row_at(&self, p: Vec2) -> Option<usize> {
        self.visible_range().find(|&i| self.row_rect(i).contains(p))
    }

    /// Hit areas of the ◀ and ▶ arrows of an adjustable row.
    fn arrow_rects(&self, index: usize, gfx: &Gfx) -> (Rect, Rect) {
        let row = self.row_rect(index);
        let detail_w = self.items[index]
            .detail
            .as_deref()
            .map(|d| gfx.text_width(d, self.font, 1))
            .unwrap_or(0.0);
        let right = Rect::new(row.right() - ARROW_W - 2.0, row.y, ARROW_W, row.h);
        let left = Rect::new(right.x - detail_w - ARROW_W - 4.0, row.y, ARROW_W, row.h);
        (left, right)
    }

    /// Handle this frame's input.
    pub fn update(&mut self, ctx: &mut Ctx) -> MenuEvent {
        if !self.active || self.items.is_empty() {
            return MenuEvent::None;
        }
        let input = &ctx.input;

        // Tap: select the row (or adjust via its arrows).
        if let Some(p) = input.tap() {
            if let Some(i) = self.row_at(p) {
                self.cursor = i;
                let item = &self.items[i];
                if !item.enabled {
                    ctx.sfx(sfx::ERROR);
                    return MenuEvent::None;
                }
                if item.adjustable {
                    let (l, r) = self.arrow_rects(i, &ctx.gfx);
                    if l.contains(p) || r.contains(p) {
                        ctx.sfx(sfx::CURSOR);
                        return MenuEvent::Adjust(i, if l.contains(p) { -1 } else { 1 });
                    }
                }
                ctx.sfx(sfx::CONFIRM);
                return MenuEvent::Selected(i);
            }
        }

        // Drag inside the menu scrolls it (touch screens).
        if let Some(drag) = input.drag() {
            if self.rect().contains(drag.origin) && self.items.len() > self.visible_rows {
                self.drag_accum -= drag.delta.y;
                let max_scroll = self.items.len() - self.visible_rows;
                while self.drag_accum >= self.row_height && self.scroll < max_scroll {
                    self.drag_accum -= self.row_height;
                    self.scroll += 1;
                }
                while self.drag_accum <= -self.row_height && self.scroll > 0 {
                    self.drag_accum += self.row_height;
                    self.scroll -= 1;
                }
                let vis = self.visible_range();
                if !vis.contains(&self.cursor) {
                    self.cursor = self.cursor.clamp(vis.start, vis.end.saturating_sub(1));
                }
                return MenuEvent::None;
            }
        } else {
            self.drag_accum = 0.0;
        }

        // Hover follows real pointer movement only.
        if input.pointer_moved() {
            if let Some(i) = input.pointer().and_then(|p| self.row_at(p)) {
                if i != self.cursor {
                    self.cursor = i;
                    return MenuEvent::Moved(i);
                }
            }
        }

        let wheel = input.wheel();
        let hovering = input.hovering(self.rect());
        let nav = input.nav();
        let step = match nav {
            Some(Dir::Up) => -1,
            Some(Dir::Down) => 1,
            _ if wheel != 0 && hovering => wheel.signum(),
            _ => 0,
        };
        if step != 0 {
            if let Some(next) = self.step_cursor(step) {
                if next != self.cursor {
                    self.cursor = next;
                    self.ensure_visible();
                    ctx.sfx(sfx::CURSOR);
                    return MenuEvent::Moved(next);
                }
            }
            return MenuEvent::None;
        }
        if let Some(d @ (Dir::Left | Dir::Right)) = nav {
            if self.items[self.cursor].adjustable && self.items[self.cursor].enabled {
                ctx.sfx(sfx::CURSOR);
                return MenuEvent::Adjust(self.cursor, if d == Dir::Left { -1 } else { 1 });
            }
        }

        if input.confirm_key() {
            if self.items[self.cursor].enabled {
                ctx.sfx(sfx::CONFIRM);
                return MenuEvent::Selected(self.cursor);
            }
            ctx.sfx(sfx::ERROR);
            return MenuEvent::None;
        }
        if self.cancellable && input.cancel() {
            ctx.sfx(sfx::CANCEL);
            return MenuEvent::Cancelled;
        }
        MenuEvent::None
    }

    pub fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let r = self.rect();
        if self.framed {
            let style = if self.active {
                WindowStyle::Normal
            } else {
                WindowStyle::Panel
            };
            draw_window_ex(r, style, 1.0);
        }
        let lh = gfx.line_height(self.font, 1);
        for i in self.visible_range() {
            let item = &self.items[i];
            let row = self.row_rect(i);
            if i == self.cursor {
                draw_highlight(row, self.active, ctx.time);
                if self.active {
                    draw_arrow_cursor(row.x + 8.0, row.y + row.h / 2.0, ctx.time);
                }
            }
            let color = if item.enabled {
                theme::TEXT
            } else {
                theme::TEXT_DISABLED
            };
            let ty = row.y + (row.h - lh) / 2.0;
            let style = TextStyle::main(color).shadow(theme::TEXT_SHADOW);
            let style = TextStyle {
                font: self.font,
                ..style
            };
            if let Some(tag) = &item.tag {
                let tcolor = if item.enabled {
                    theme::TEXT_DIM
                } else {
                    theme::TEXT_DISABLED
                };
                gfx.text(tag, row.x + CURSOR_GUTTER, ty, TextStyle { color: tcolor, ..style });
            }
            gfx.text(&item.label, row.x + CURSOR_GUTTER + self.tag_width, ty, style);
            if let Some(detail) = &item.detail {
                let dcolor = if item.enabled {
                    theme::TEXT_ACCENT
                } else {
                    theme::TEXT_DISABLED
                };
                let dstyle = TextStyle { color: dcolor, ..style };
                let dw = gfx.text_width(detail, self.font, 1);
                if item.adjustable {
                    let (la, ra) = self.arrow_rects(i, gfx);
                    let acolor = if item.enabled && i == self.cursor {
                        theme::CURSOR_ARROW
                    } else {
                        theme::TEXT_DIM
                    };
                    draw_side_arrow(la.x + la.w / 2.0, row.y + row.h / 2.0, false, acolor);
                    draw_side_arrow(ra.x + ra.w / 2.0, row.y + row.h / 2.0, true, acolor);
                    gfx.text(detail, ra.x - 2.0 - dw, ty, dstyle);
                } else {
                    gfx.text(detail, row.right() - 4.0 - dw, ty, dstyle);
                }
            }
        }
        // Scroll indicators.
        if self.items.len() > self.visible_rows {
            let cx = r.x + r.w / 2.0;
            let blink = (ctx.time * 2.0).fract() < 0.7;
            if self.scroll > 0 && blink {
                draw_small_arrow(cx, r.y + 3.0, false, theme::TEXT_ACCENT);
            }
            if self.scroll + self.visible_rows < self.items.len() && blink {
                draw_small_arrow(cx, r.bottom() - 3.0, true, theme::TEXT_ACCENT);
            }
        }
    }
}

/// Next enabled index from `from` in direction `step` (±1). With `wrap` the search continues
/// at the other end. Returns `None` when no other enabled item exists in that direction.
pub fn next_enabled(enabled: &[bool], from: usize, step: i32, wrap: bool) -> Option<usize> {
    let n = enabled.len() as i32;
    if n == 0 {
        return None;
    }
    let mut i = from as i32;
    for _ in 0..n {
        i += step;
        if i < 0 || i >= n {
            if !wrap {
                return None;
            }
            i = i.rem_euclid(n);
        }
        if enabled[i as usize] {
            return Some(i as usize);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_skips_disabled_items() {
        let e = [true, false, true, true];
        assert_eq!(next_enabled(&e, 0, 1, true), Some(2));
        assert_eq!(next_enabled(&e, 2, -1, true), Some(0));
        assert_eq!(next_enabled(&e, 3, 1, true), Some(0));
        assert_eq!(next_enabled(&e, 0, -1, true), Some(3));
        assert_eq!(next_enabled(&e, 3, 1, false), None);
        assert_eq!(next_enabled(&e, 0, -1, false), None);
        assert_eq!(next_enabled(&[false, false], 0, 1, true), None);
        assert_eq!(next_enabled(&[], 0, 1, true), None);
        // The only enabled item wraps onto itself.
        assert_eq!(next_enabled(&[true], 0, 1, true), Some(0));
    }

    #[test]
    fn scrolling_keeps_cursor_visible() {
        let items = (0..10).map(|i| MenuItem::new(format!("{i}"))).collect();
        let mut m = Menu::new(items).rows(4);
        m.set_cursor(6);
        assert_eq!(m.scroll, 3);
        assert!(m.visible_range().contains(&6));
        m.set_cursor(1);
        assert_eq!(m.scroll, 1);
        m.set_cursor(99);
        assert_eq!(m.cursor(), 9);
        assert_eq!(m.scroll, 6);
        m.set_items((0..3).map(|i| MenuItem::new(format!("{i}"))).collect());
        assert_eq!(m.cursor(), 2);
        assert_eq!(m.scroll, 0);
    }

    #[test]
    fn initial_cursor_is_first_enabled_item() {
        let m = Menu::new(vec![
            MenuItem::new("a").enabled(false),
            MenuItem::new("b"),
        ]);
        assert_eq!(m.cursor(), 1);
    }

    #[test]
    fn layout() {
        let m = Menu::new(vec![MenuItem::new("a"), MenuItem::new("b")]).at(10.0, 20.0, 100.0);
        let r = m.rect();
        assert_eq!((r.x, r.y, r.w), (10.0, 20.0, 100.0));
        assert_eq!(r.h, 2.0 * theme::ROW_HEIGHT + 2.0 * theme::PADDING);
        assert!(m.row_rect(1).y > m.row_rect(0).y);
        assert_eq!(m.row_at(m.row_rect(1).center()), Some(1));
    }
}
