//! The camp frame (`[presentation.camp_frame]`, docs/DECISIONS.md D15): the camp screens are laid
//! out in its `view` as if it were the whole canvas (the app sets the view, see
//! [`crate::app::Screen::in_camp_frame`]), and the frame's picture is drawn over the rest with
//! the army's leader, gold, level, place, the camp's heading and the play time in its areas.

use crate::app::Ctx;
use crate::gfx::{fill_rect, FontId, TextStyle};
use crate::ui::format;
use crate::ui::theme;
use hero_core::campaign::Node;
use hero_core::pack::CampFrame;
use macroquad::prelude::*;

/// An `[x, y, width, height]` area of a frame.
pub fn area([x, y, w, h]: [u32; 4]) -> Rect {
    Rect::new(x as f32, y as f32, w as f32, h as f32)
}

/// The frame's picture over everything outside its view, and its areas filled in. Without the
/// picture (still loading, missing, or not the canvas size) the parts are filled plainly.
pub fn draw_camp_frame(ctx: &Ctx, frame: &CampFrame) {
    let canvas = ctx.gfx.size();
    let view = area(frame.view);
    let picture = ctx
        .media
        .texture(&frame.image)
        .filter(|t| vec2(t.width(), t.height()) == canvas);
    let parts = [
        Rect::new(0.0, 0.0, canvas.x, view.y),
        Rect::new(0.0, view.bottom(), canvas.x, canvas.y - view.bottom()),
        Rect::new(0.0, view.y, view.x, view.h),
        Rect::new(view.right(), view.y, canvas.x - view.right(), view.h),
    ];
    for part in parts.into_iter().filter(|p| p.w > 0.0 && p.h > 0.0) {
        match &picture {
            Some(tex) => draw_texture_ex(
                tex,
                part.x,
                part.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(part.size()),
                    source: Some(part),
                    ..Default::default()
                },
            ),
            None => fill_rect(part, theme::WIN_BOTTOM),
        }
    }
    let (Some(pack), Some(session)) = (ctx.pack.as_deref(), ctx.session.as_ref()) else {
        return;
    };
    let campaign = &session.campaign;
    let gfx = &ctx.gfx;
    let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
    let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
    // Text shortened to `width` pixels.
    let fit = |text: &str, font: FontId, width: f32| -> String {
        let mut out = text.to_string();
        while !out.is_empty() && gfx.text_width(&out, font, 1) > width {
            out.pop();
        }
        out
    };
    // One line centred vertically in `r`, `pad` pixels in from the left.
    let line = |r: Rect, text: &str, style: TextStyle, font: FontId, pad: f32| {
        let h = gfx.line_height(font, 1);
        let shown = fit(text, font, r.w - 2.0 * pad);
        gfx.text(&shown, r.x + pad, r.y + ((r.h - h) / 2.0).max(0.0), style);
    };

    let leader = campaign.roster.first();
    if let Some(officer) = leader.and_then(|o| pack.officer(&o.id)) {
        let key = officer.portrait.as_deref().unwrap_or(officer.id.as_str());
        if let Some(tex) = ctx.media.portrait(key) {
            let r = area(frame.portrait);
            draw_texture_ex(
                &tex,
                r.x,
                r.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(r.size()),
                    ..Default::default()
                },
            );
        }
    }
    let gold = area(frame.gold);
    line(
        gold,
        &format::thousands(campaign.gold),
        small.color(theme::TEXT_ACCENT),
        FontId::Small,
        2.0,
    );
    if let Some(o) = leader {
        line(
            area(frame.level),
            &o.level.to_string(),
            small,
            FontId::Small,
            2.0,
        );
    }
    let heading = match pack.campaign.node(&campaign.node) {
        Some(Node::Camp { title, .. }) => title.clone(),
        _ => String::new(),
    };
    let place = heading.split(" — ").next().unwrap_or_default();
    line(area(frame.place), place, small, FontId::Small, 2.0);
    line(area(frame.caption), &heading, main, FontId::Main, 6.0);
    line(
        area(frame.clock),
        &format::play_time(campaign.play_seconds),
        main,
        FontId::Main,
        6.0,
    );
}
