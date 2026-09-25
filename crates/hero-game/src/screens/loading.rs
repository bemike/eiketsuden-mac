//! Startup loading screen: fonts, then `pack.toml`, every text file it lists (fetched in
//! parallel), `Pack::load` + `Pack::validate`, and a short media preload. Any failure leads to
//! the [`ErrorScreen`] with a retry option.
//!
//! In gallery mode only the fonts (and UI sounds) are loaded; the pack is not needed.

use super::error::ErrorScreen;
use super::gallery::GalleryScreen;
use crate::app::{Ctx, Screen, Transition};
use crate::assets::FileBatch;
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{fill_rect, stroke_rect, Align, FontId, TextStyle, VIRTUAL_W};
use crate::ui::theme;
use hero_core::pack::{Pack, PackError, PackManifest, Severity};
use macroquad::prelude::*;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Longest time the loading screen waits for optional media before moving on.
const MEDIA_WAIT_SECONDS: f64 = 4.0;
/// Validation errors listed on the error screen.
const MAX_LISTED_ISSUES: usize = 12;

/// What to start once loading is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The game: title screen.
    Game,
    /// The UI gallery dev screen.
    Gallery,
}

enum Stage {
    Start,
    Fonts(FileBatch),
    Manifest(FileBatch),
    Files {
        /// Original `pack.toml` text (`Pack::load` reads it again from the file map).
        manifest_src: String,
        batch: FileBatch,
    },
    Parse(BTreeMap<String, String>),
    Media {
        since: f64,
    },
    Done,
}

pub struct LoadingScreen {
    target: Target,
    stage: Stage,
    /// 0..=1 for the progress bar.
    progress: f32,
    status: String,
    fonts_ready: bool,
}

impl LoadingScreen {
    pub fn new(target: Target) -> LoadingScreen {
        LoadingScreen {
            target,
            stage: Stage::Start,
            progress: 0.0,
            status: String::new(),
            fonts_ready: false,
        }
    }

    fn fail(&self, title: &str, details: Vec<String>, ctx: &Ctx) -> Transition {
        for d in &details {
            macroquad::logging::error!("{}: {}", title, d);
        }
        let mut lines = details;
        lines.push(String::new());
        lines.push(format!("데이터 위치: {}", ctx.data_root.display()));
        if ctx.data_root.candidates().len() > 1 {
            lines.push(format!(
                "찾아본 위치: {}",
                ctx.data_root.candidates().join(", ")
            ));
        }
        if !crate::platform::is_web() {
            lines.push(format!(
                "--data <폴더> 옵션이나 {} 환경 변수로 데이터 팩 위치를 지정할 수 있습니다.",
                crate::platform::DATA_ENV
            ));
        }
        Transition::replace(ErrorScreen::fatal(title, lines, Some(self.target)))
    }

    fn step(&mut self, ctx: &mut Ctx) -> Transition {
        match std::mem::replace(&mut self.stage, Stage::Done) {
            Stage::Start => {
                self.status = "글꼴".into();
                self.stage = Stage::Fonts(FileBatch::new(
                    &ctx.data_root,
                    [
                        FontId::Main.file().to_string(),
                        FontId::Small.file().to_string(),
                    ],
                ));
            }
            Stage::Fonts(mut batch) => {
                if !batch.poll() {
                    self.stage = Stage::Fonts(batch);
                    return Transition::None;
                }
                let mut results = batch.into_results();
                for id in [FontId::Main, FontId::Small] {
                    let bytes = results
                        .remove(id.file())
                        .unwrap_or_else(|| Err("not requested".into()));
                    ctx.gfx
                        .fonts
                        .install(id, bytes.as_deref().map_err(Clone::clone));
                }
                self.fonts_ready = true;
                self.progress = 0.1;
                if !ctx.gfx.fonts.missing().is_empty() {
                    ctx.toast(format!(
                        "Font missing: {}",
                        ctx.gfx.fonts.missing().join(", ")
                    ));
                }
                ctx.media.preload_sounds(
                    &[sfx::CURSOR, sfx::CONFIRM, sfx::CANCEL, sfx::ERROR]
                        .map(|k| format!("sfx/{k}")),
                );
                if self.target == Target::Gallery {
                    return Transition::replace(GalleryScreen::new());
                }
                self.status = "pack.toml".into();
                self.stage =
                    Stage::Manifest(FileBatch::new(&ctx.data_root, ["pack.toml".to_string()]));
            }
            Stage::Manifest(mut batch) => {
                if !batch.poll() {
                    self.stage = Stage::Manifest(batch);
                    return Transition::None;
                }
                let result = batch
                    .into_results()
                    .remove("pack.toml")
                    .unwrap_or_else(|| Err("not requested".into()));
                let parsed = result
                    .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
                    .map_err(|e| format!("pack.toml: {e}"))
                    .and_then(|s| match PackManifest::parse(&s) {
                        Ok(m) => Ok((m, s)),
                        Err(e) => Err(e.to_string()),
                    });
                let (manifest, manifest_src) = match parsed {
                    Ok(m) => m,
                    Err(e) => {
                        return self.fail("데이터 팩을 찾을 수 없습니다", vec![e], ctx);
                    }
                };
                macroquad::logging::info!(
                    "pack {} {} ({})",
                    manifest.id,
                    manifest.version,
                    manifest.name
                );
                let batch = FileBatch::new(&ctx.data_root, manifest.text_files());
                self.status = manifest.name.clone();
                self.progress = 0.15;
                self.stage = Stage::Files {
                    manifest_src,
                    batch,
                };
            }
            Stage::Files {
                manifest_src,
                mut batch,
            } => {
                let done = batch.poll();
                let (n, total) = batch.progress();
                self.progress = 0.15 + 0.6 * (n as f32 / total.max(1) as f32);
                if let Some(cur) = batch.current() {
                    self.status = cur.to_string();
                }
                if !done {
                    self.stage = Stage::Files {
                        manifest_src,
                        batch,
                    };
                    return Transition::None;
                }
                let mut files = BTreeMap::new();
                let mut errors = Vec::new();
                for (rel, result) in batch.into_results() {
                    match result.and_then(|b| {
                        String::from_utf8(b).map_err(|_| "not valid UTF-8 text".to_string())
                    }) {
                        Ok(text) => {
                            files.insert(rel, text);
                        }
                        Err(e) => errors.push(format!("{rel}: {e}")),
                    }
                }
                if !errors.is_empty() {
                    return self.fail("데이터 팩 파일을 읽을 수 없습니다", errors, ctx);
                }
                // `Pack::load` reads `pack.toml` through the same source.
                files.insert("pack.toml".to_string(), manifest_src);
                self.status = "규칙 해석".into();
                self.progress = 0.8;
                self.stage = Stage::Parse(files);
            }
            Stage::Parse(files) => {
                let pack = match Pack::load(&files) {
                    Ok(p) => p,
                    Err(e) => {
                        let msg = match &e {
                            PackError::Missing { file } => format!("{file}: 파일이 없습니다"),
                            other => other.to_string(),
                        };
                        return self.fail("데이터 팩 오류", vec![msg], ctx);
                    }
                };
                let issues = pack.validate();
                let mut errors = Vec::new();
                for issue in &issues {
                    let line = format!("{}: {}", issue.context, issue.msg);
                    match issue.severity {
                        Severity::Warning => macroquad::logging::warn!("pack: {}", line),
                        Severity::Error => errors.push(line),
                    }
                }
                if !errors.is_empty() {
                    let total = errors.len();
                    errors.truncate(MAX_LISTED_ISSUES);
                    if total > MAX_LISTED_ISSUES {
                        errors.push(format!("… 외 {}건", total - MAX_LISTED_ISSUES));
                    }
                    return self.fail("데이터 팩 검증 실패", errors, ctx);
                }
                ctx.pack = Some(Rc::new(pack));
                let sounds: Vec<String> = sfx::ALL
                    .iter()
                    .map(|k| format!("sfx/{k}"))
                    .chain(std::iter::once(format!("bgm/{}", bgm::TITLE)))
                    .collect();
                ctx.media.preload_sounds(&sounds);
                ctx.media.preload_textures(&["ui/title"]);
                self.status = "음악·그림".into();
                self.progress = 0.9;
                self.stage = Stage::Media { since: ctx.time };
            }
            Stage::Media { since } => {
                if ctx.media.pending() > 0 && ctx.time - since < MEDIA_WAIT_SECONDS {
                    self.stage = Stage::Media { since };
                    return Transition::None;
                }
                self.progress = 1.0;
                return Transition::Flow(Flow::Title);
            }
            Stage::Done => {}
        }
        Transition::None
    }
}

impl Screen for LoadingScreen {
    fn name(&self) -> &'static str {
        "loading"
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // Parsing is synchronous; everything else advances once per frame.
        self.step(ctx)
    }

    fn draw(&self, ctx: &Ctx) {
        clear_background(theme::BACKGROUND);
        let gfx = &ctx.gfx;
        let bar = Rect::new(140.0, 170.0, 200.0, 6.0);
        fill_rect(bar, theme::GAUGE_BG);
        stroke_rect(bar, theme::BORDER_MID);
        let fill = Rect::new(
            bar.x + 1.0,
            bar.y + 1.0,
            (bar.w - 2.0) * self.progress,
            bar.h - 2.0,
        );
        fill_rect(fill, theme::TEXT_ACCENT);
        // Spinner dots (visible even before the fonts exist).
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let phase = ((ctx.time * 8.0) as i32).rem_euclid(8);
            let alpha = if i == phase { 1.0 } else { 0.25 };
            draw_circle(
                VIRTUAL_W / 2.0 + a.cos() * 8.0,
                145.0 + a.sin() * 8.0,
                1.5,
                theme::TEXT_ACCENT.with_alpha(alpha),
            );
        }
        if self.fonts_ready {
            gfx.text_aligned(
                "영걸전 Reloaded",
                0.0,
                96.0,
                VIRTUAL_W,
                Align::Center,
                TextStyle::main(theme::TEXT_ACCENT)
                    .size(2)
                    .shadow(theme::TEXT_SHADOW),
            );
            gfx.text_aligned(
                &format!("불러오는 중… {}", self.status),
                0.0,
                182.0,
                VIRTUAL_W,
                Align::Center,
                TextStyle::small(theme::TEXT_DIM),
            );
        }
    }
}
