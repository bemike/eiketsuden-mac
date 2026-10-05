//! Camp guide, paginated after font measurement so no reward or condition is truncated.

use super::widgets::{
    back_tapped, content_rect, draw_back_button, draw_camp_backdrop, draw_header, draw_help,
};
use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::gfx::{Align, FontId, TextStyle};
use crate::input::Dir;
use crate::ui::theme;
use crate::ui::window::{draw_window_ex, WindowStyle};
use hero_core::guide::battle_guide;
use macroquad::prelude::*;

const LINE_H: f32 = 16.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    text: String,
    heading: bool,
}

// Repeat section headings on continuation pages; never leave a heading at the page bottom.
fn paginate(sections: Vec<(String, Vec<String>)>, capacity: usize) -> Vec<Vec<Line>> {
    let capacity = capacity.max(2);
    let mut pages = vec![Vec::new()];
    for (heading, lines) in sections {
        if pages.last().unwrap().len() + 2 > capacity {
            pages.push(Vec::new());
        }
        pages.last_mut().unwrap().push(Line {
            text: heading.clone(),
            heading: true,
        });
        for text in lines {
            if pages.last().unwrap().len() == capacity {
                pages.push(vec![Line {
                    text: format!("{heading}（续）"),
                    heading: true,
                }]);
            }
            pages.last_mut().unwrap().push(Line {
                text,
                heading: false,
            });
        }
    }
    pages
}

pub struct GuideScreen {
    battle: String,
    shop: Vec<String>,
    title: String,
    pages: Vec<Vec<Line>>,
    page: usize,
}

impl GuideScreen {
    pub fn new(battle: &str, shop: &[String]) -> Self {
        Self {
            battle: battle.into(),
            shop: shop.to_vec(),
            title: String::new(),
            pages: Vec::new(),
            page: 0,
        }
    }

    fn buttons(canvas: Vec2) -> [Rect; 2] {
        let rect = content_rect(canvas);
        let y = rect.bottom() - 27.0;
        [
            Rect::new(rect.x + 9.0, y, 70.0, 22.0),
            Rect::new(rect.right() - 79.0, y, 70.0, 22.0),
        ]
    }

    fn turn(&mut self, ctx: &mut Ctx, delta: i32) {
        let next =
            (self.page as i32 + delta).clamp(0, self.pages.len().saturating_sub(1) as i32) as usize;
        if next != self.page {
            self.page = next;
            ctx.sfx(sfx::CURSOR);
        }
        ctx.input.consume();
    }
}

impl Screen for GuideScreen {
    fn name(&self) -> &'static str {
        "battle guide"
    }
    fn in_camp_frame(&self) -> bool {
        true
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
            return;
        };
        self.title = pack
            .battles
            .get(&self.battle)
            .map_or_else(|| "本关要点".into(), |b| format!("{} — 本关要点", b.name));
        let rect = content_rect(ctx.gfx.size());
        let capacity = ((rect.h - 63.0) / LINE_H).floor() as usize;
        let sections = battle_guide(pack, &self.battle, &session.campaign, &self.shop)
            .into_iter()
            .map(|section| {
                let lines = section
                    .entries
                    .iter()
                    .flat_map(|entry| {
                        ctx.gfx
                            .wrap(&format!("· {entry}"), FontId::Main, 1, rect.w - 24.0)
                    })
                    .collect();
                (section.title, lines)
            })
            .collect();
        self.pages = paginate(sections, capacity);
        self.page = self.page.min(self.pages.len().saturating_sub(1));
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if back_tapped(ctx) {
            return Transition::Pop;
        }
        if ctx.input.cancel() {
            ctx.input.consume();
            ctx.sfx(sfx::CANCEL);
            return Transition::Pop;
        }
        let [prev, next] = Self::buttons(ctx.gfx.size());
        let wheel = ctx.input.wheel();
        let nav = ctx.input.nav();
        if ctx.input.tapped(prev) || matches!(nav, Some(Dir::Left | Dir::Up)) || wheel < 0 {
            self.turn(ctx, -1);
        } else if ctx.input.tapped(next)
            || matches!(nav, Some(Dir::Right | Dir::Down))
            || wheel > 0
            || ctx.input.confirm()
        {
            self.turn(ctx, 1);
        }
        Transition::None
    }

    fn draw(&self, ctx: &Ctx) {
        draw_camp_backdrop(ctx, 0.48);
        draw_header(
            ctx,
            &self.title,
            ctx.session.as_ref().map_or(0, |s| s.campaign.gold),
        );
        let rect = content_rect(ctx.gfx.size());
        draw_window_ex(rect, WindowStyle::Panel, 1.0);
        ctx.gfx.text(
            "含剧情提示。坐标从地图左上角起，列、行均从 1 计。",
            rect.x + 10.0,
            rect.y + 8.0,
            TextStyle::small(theme::TEXT_DIM),
        );
        if let Some(page) = self.pages.get(self.page) {
            for (i, line) in page.iter().enumerate() {
                ctx.gfx.text(
                    &line.text,
                    rect.x + 11.0,
                    rect.y + 27.0 + i as f32 * LINE_H,
                    TextStyle::main(if line.heading {
                        theme::TEXT_ACCENT
                    } else {
                        theme::TEXT
                    })
                    .shadow(theme::TEXT_SHADOW),
                );
            }
        }
        let [prev, next] = Self::buttons(ctx.gfx.size());
        for (rect, text, enabled) in [
            (prev, "上一页", self.page > 0),
            (next, "下一页", self.page + 1 < self.pages.len()),
        ] {
            draw_window_ex(rect, WindowStyle::Panel, 1.0);
            ctx.gfx.text_aligned(
                text,
                rect.x,
                rect.y + 3.0,
                rect.w,
                Align::Center,
                TextStyle::main(if enabled {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                }),
            );
        }
        ctx.gfx.text_aligned(
            &format!("{} / {}", self.page + 1, self.pages.len().max(1)),
            prev.right(),
            prev.y + 3.0,
            next.x - prev.right(),
            Align::Center,
            TextStyle::main(theme::TEXT_DIM),
        );
        draw_help(
            ctx,
            "方向键、滚轮翻页；宝物格停留领取；转换道具在营地「道具」使用。",
        );
        draw_back_button(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_keeps_every_line_and_repeats_headings() {
        let lines: Vec<_> = (0..20).map(|i| format!("entry {i}")).collect();
        let pages = paginate(
            vec![
                ("treasures".into(), lines.clone()),
                ("classes".into(), vec!["last".into()]),
            ],
            7,
        );
        assert!(pages
            .iter()
            .all(|p| p.len() <= 7 && p.first().unwrap().heading && !p.last().unwrap().heading));
        let actual: Vec<_> = pages
            .iter()
            .flatten()
            .filter(|l| !l.heading)
            .map(|l| l.text.clone())
            .collect();
        assert_eq!(actual, [lines, vec!["last".into()]].concat());
    }
}
