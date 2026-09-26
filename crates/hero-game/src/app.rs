//! Application core: the shared [`Ctx`], the [`Screen`] trait and the screen stack.
//!
//! # Writing a screen
//!
//! A screen is a struct implementing [`Screen`]. Every frame the app calls
//! [`Screen::update`] on the **top** screen only, then [`Screen::draw`] on the visible screens
//! (the top screen, plus the screens below it while the top ones are overlays). `update` returns
//! a [`Transition`]:
//!
//! * [`Transition::Push`] a screen on top (e.g. settings from the title screen),
//!   [`Transition::Pop`] back, [`Transition::Replace`] the top screen;
//! * [`Transition::Flow`] to move through the game flow ([`crate::flow::Flow`]: title, new game,
//!   next campaign node, ...), which replaces the whole stack;
//! * [`Transition::Quit`] (native only).
//!
//! Stack changes fade to black and back ([`FADE_SECONDS`] each way) unless the screen being
//! pushed or popped is an overlay ([`Screen::is_overlay`]), which appears instantly on top of
//! the screen below it. [`Screen::on_enter`] runs when a screen becomes the top screen: after it
//! was pushed ([`Enter::Fresh`]) or when the screen above it was popped ([`Enter::Resumed`]) —
//! the place to (re)start music or refresh data.
//!
//! Screens draw in virtual 480×270 coordinates (see [`crate::gfx`]) and read input from
//! [`Ctx::input`]. Widgets from [`crate::ui`] take `&mut Ctx` in their `update` (they play UI
//! sounds) and `&Ctx` in `draw`.

use crate::assets::Media;
use crate::audio::Audio;
use crate::flow::{Flow, Session};
use crate::gfx::{fill_rect, Gfx, TextStyle, SCREEN};
use crate::input::Input;
use crate::platform::storage::KeyValueStore;
use crate::platform::{DataRoot, LaunchOptions};
use crate::settings::Settings;
use crate::ui::theme;
use crate::ui::toast::Toasts;
use hero_core::pack::Pack;
use macroquad::prelude::*;
use std::rc::Rc;

/// Duration of each half of a screen transition fade.
pub const FADE_SECONDS: f32 = 0.18;
/// Longest frame time fed to the game (avoids huge jumps after a stall).
pub const MAX_FRAME_TIME: f32 = 0.1;

/// Everything screens share. Created once at startup.
pub struct Ctx {
    pub gfx: Gfx,
    pub input: Input,
    pub audio: Audio,
    pub media: Media,
    pub settings: Settings,
    pub storage: Box<dyn KeyValueStore>,
    pub toasts: Toasts,
    /// The loaded data pack (`None` before loading finishes and in the UI gallery).
    pub pack: Option<Rc<Pack>>,
    /// The campaign being played (`None` on the title screen).
    pub session: Option<Session>,
    pub options: LaunchOptions,
    pub data_root: DataRoot,
    /// Seconds since the previous frame (clamped to [`MAX_FRAME_TIME`]).
    pub dt: f32,
    /// Seconds since startup.
    pub time: f64,
    pub frame: u64,
}

impl Ctx {
    /// Build the context: platform storage, settings, canvas, media and audio.
    pub fn new(options: LaunchOptions) -> Ctx {
        let data_root = DataRoot::resolve(&options);
        let storage = crate::platform::storage::open_default();
        let (settings, warning) = Settings::load(storage.as_ref());
        if let Some(w) = warning {
            macroquad::logging::warn!("{}", w);
        }
        Ctx {
            gfx: Gfx::new(),
            input: Input::new(),
            audio: Audio::new(&settings),
            media: Media::new(data_root.clone()),
            settings,
            storage,
            toasts: Toasts::default(),
            pack: None,
            session: None,
            options,
            data_root,
            dt: 0.0,
            time: 0.0,
            frame: 0,
        }
    }

    /// Play a UI sound effect by key (see [`crate::audio::sfx`]).
    pub fn sfx(&mut self, key: &str) {
        self.audio.sfx(&self.media, key);
    }

    /// Show a short message at the top of the screen.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(text);
    }

    /// Id of the loaded pack.
    pub fn pack_id(&self) -> Option<&str> {
        self.pack.as_deref().map(|p| p.manifest.id.as_str())
    }

    /// Apply the current settings (volumes, fullscreen) and persist them. Storage failures are
    /// reported with a toast.
    pub fn commit_settings(&mut self) {
        self.audio.apply_settings(&self.settings);
        if crate::platform::can_toggle_fullscreen() {
            set_fullscreen(self.settings.fullscreen);
        }
        if let Err(e) = self.settings.save(self.storage.as_mut()) {
            macroquad::logging::error!("cannot save settings: {}", e);
            self.toast(format!("설정을 저장하지 못했습니다: {e}"));
        }
    }
}

/// How a screen became the top screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enter {
    /// Just pushed (or installed by a flow change).
    Fresh,
    /// The screen above it was popped.
    Resumed,
}

/// What the app should do after a screen's update.
pub enum Transition {
    None,
    Push(Box<dyn Screen>),
    Pop,
    Replace(Box<dyn Screen>),
    Flow(Flow),
    /// Close the game (ignored on the web).
    Quit,
}

impl Transition {
    pub fn push(screen: impl Screen + 'static) -> Transition {
        Transition::Push(Box::new(screen))
    }

    pub fn replace(screen: impl Screen + 'static) -> Transition {
        Transition::Replace(Box::new(screen))
    }
}

/// A full screen or overlay. See the module docs.
pub trait Screen {
    /// Short name for logs.
    fn name(&self) -> &'static str;

    /// The screen became the top screen.
    fn on_enter(&mut self, _ctx: &mut Ctx, _how: Enter) {}

    /// Handle input and advance animations. Called once per frame while on top.
    fn update(&mut self, ctx: &mut Ctx) -> Transition;

    /// Draw in virtual coordinates.
    fn draw(&self, ctx: &Ctx);

    /// Overlays are pushed/popped without a fade and the screen below keeps being drawn.
    fn is_overlay(&self) -> bool {
        false
    }
}

enum Fade {
    Idle,
    Out { t: f32, pending: Transition },
    In { t: f32 },
}

/// The screen stack and main loop body.
pub struct App {
    ctx: Ctx,
    stack: Vec<Box<dyn Screen>>,
    fade: Fade,
    quit: bool,
    presented: bool,
    show_fps: bool,
}

impl App {
    pub fn new(mut ctx: Ctx, mut first: Box<dyn Screen>) -> App {
        if ctx.settings.fullscreen && crate::platform::can_toggle_fullscreen() {
            set_fullscreen(true);
        }
        first.on_enter(&mut ctx, Enter::Fresh);
        App {
            ctx,
            stack: vec![first],
            fade: Fade::In { t: 0.0 },
            quit: false,
            presented: false,
            show_fps: false,
        }
    }

    /// Run one frame: input, update, draw. Returns `false` when the game should exit.
    pub fn frame(&mut self) -> bool {
        let dt = get_frame_time().clamp(0.0, MAX_FRAME_TIME);
        let ctx = &mut self.ctx;
        ctx.dt = dt;
        ctx.time += f64::from(dt);
        ctx.frame += 1;
        ctx.gfx.canvas.update();
        ctx.input.update(dt, &ctx.gfx.canvas);
        ctx.media.pump();
        ctx.audio.update(dt, &ctx.media, ctx.input.any_activity());
        ctx.toasts.update(dt);
        if let Some(session) = ctx.session.as_mut() {
            session.tick(dt);
        }
        self.global_keys();

        match std::mem::replace(&mut self.fade, Fade::Idle) {
            Fade::Out { t, pending } => {
                self.ctx.input.consume();
                let t = t + dt / FADE_SECONDS;
                if t >= 1.0 {
                    self.apply(pending);
                    self.fade = Fade::In { t: 0.0 };
                } else {
                    self.fade = Fade::Out { t, pending };
                }
            }
            Fade::In { t } => {
                self.ctx.input.consume();
                let t = t + dt / FADE_SECONDS;
                if t < 1.0 {
                    self.fade = Fade::In { t };
                }
            }
            Fade::Idle => {
                if let Some(top) = self.stack.last_mut() {
                    let transition = top.update(&mut self.ctx);
                    self.handle(transition);
                }
            }
        }

        self.draw();
        if !self.presented {
            self.presented = true;
            crate::platform::notify_ready();
        }
        !self.quit
    }

    fn global_keys(&mut self) {
        if is_key_pressed(KeyCode::F3) {
            self.show_fps = !self.show_fps;
        }
        let alt_enter = is_key_pressed(KeyCode::Enter)
            && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt));
        if (is_key_pressed(KeyCode::F11) || alt_enter) && crate::platform::can_toggle_fullscreen() {
            self.ctx.settings.fullscreen = !self.ctx.settings.fullscreen;
            self.ctx.commit_settings();
            self.ctx.input.consume();
        }
    }

    fn handle(&mut self, transition: Transition) {
        match transition {
            Transition::None => {}
            Transition::Quit => {
                if crate::platform::can_quit() {
                    self.quit = true;
                }
            }
            Transition::Push(screen) if screen.is_overlay() => self.apply(Transition::Push(screen)),
            Transition::Pop if self.stack.last().is_some_and(|s| s.is_overlay()) => {
                self.apply(Transition::Pop)
            }
            other => {
                self.fade = Fade::Out {
                    t: 0.0,
                    pending: other,
                };
            }
        }
    }

    fn apply(&mut self, transition: Transition) {
        let ctx = &mut self.ctx;
        match transition {
            Transition::None | Transition::Quit => {}
            Transition::Push(mut screen) => {
                screen.on_enter(ctx, Enter::Fresh);
                self.stack.push(screen);
            }
            Transition::Pop => {
                if self.stack.len() <= 1 {
                    macroquad::logging::error!("screen stack: cannot pop the last screen");
                    return;
                }
                self.stack.pop();
                if let Some(top) = self.stack.last_mut() {
                    top.on_enter(ctx, Enter::Resumed);
                }
            }
            Transition::Replace(mut screen) => {
                self.stack.pop();
                screen.on_enter(ctx, Enter::Fresh);
                self.stack.push(screen);
            }
            Transition::Flow(flow) => {
                let mut screen = crate::flow::enter(flow, ctx);
                self.stack.clear();
                screen.on_enter(ctx, Enter::Fresh);
                self.stack.push(screen);
            }
        }
        // The new top screen starts with a clean input frame.
        ctx.input.consume();
    }

    fn draw(&self) {
        let ctx = &self.ctx;
        ctx.gfx.canvas.begin();
        clear_background(BLACK);
        let base = self
            .stack
            .iter()
            .rposition(|s| !s.is_overlay())
            .unwrap_or(0);
        for screen in &self.stack[base..] {
            screen.draw(ctx);
        }
        ctx.toasts.draw(&ctx.gfx);

        let fade = match &self.fade {
            Fade::Idle => 0.0,
            Fade::Out { t, .. } => t.clamp(0.0, 1.0),
            Fade::In { t } => 1.0 - t.clamp(0.0, 1.0),
        };
        if fade > 0.0 {
            fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, fade));
        }
        if self.show_fps {
            let top = self.stack.last().map(|s| s.name()).unwrap_or("-");
            let text = format!(
                "{} fps  S={}  {}  media:{}",
                get_fps(),
                ctx.gfx.scale(),
                top,
                ctx.media.pending()
            );
            fill_rect(
                Rect::new(0.0, 0.0, 480.0, 13.0),
                Color::new(0.0, 0.0, 0.0, 0.6),
            );
            ctx.gfx.text(&text, 3.0, 0.0, TextStyle::small(theme::TEXT));
        }
        ctx.gfx.canvas.present(theme::LETTERBOX);
    }
}

/// Run the game until it quits: the entry point used by `main.rs`.
pub async fn run(options: LaunchOptions) {
    for w in &options.warnings {
        macroquad::logging::warn!("{}", w);
    }
    let ctx = Ctx::new(options);
    let target = if ctx.options.gallery {
        crate::screens::loading::Target::Gallery
    } else {
        crate::screens::loading::Target::Game
    };
    let first = Box::new(crate::screens::loading::LoadingScreen::new(target));
    let mut app = App::new(ctx, first);
    while app.frame() {
        next_frame().await;
    }
}
