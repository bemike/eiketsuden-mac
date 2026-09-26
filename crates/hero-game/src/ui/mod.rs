//! Procedural UI toolkit in the spirit of the PC version of Eiketsuden: deep blue gradient
//! windows with light bevelled borders, a pulsing selection bar and a gold arrow cursor. No UI
//! artwork is needed; everything is drawn with shapes and the Galmuri fonts.
//!
//! | widget | module |
//! |---|---|
//! | windows, highlight, cursors, portrait box, images/sprites/icons with fallbacks | [`window`] |
//! | vertical menu (keyboard/mouse/touch, disabled items, scrolling, ◀ value ▶) | [`menu`] |
//! | dialogue box with typewriter text, name tab and portrait | [`message`] |
//! | choice box, yes/no confirmation | [`dialog`] |
//! | toasts (app-wide) and banners | [`toast`] |
//! | HP/MP/EXP/morale gauges | [`bars`] |
//! | tooltip and hover delay | [`tooltip`] |
//! | number/time formatting | [`mod@format`] |
//! | colours and metrics | [`theme`] |
//! | drama dialogue box (speech / narration, fast mode) | [`dialogue`] |
//! | paged typewriter text (pure logic) | [`textflow`] |
//! | backlog (최근 대사) model and window | [`backlog`] |
//! | backgrounds, portrait cards, unit icons with fallbacks | [`art`] |
//! | Korean particles (을/를, 이/가, ...) | [`korean`] |
//!
//! Widgets are plain structs owned by screens: call `update(&mut ctx)` while the widget has
//! focus and `draw(&ctx)` every frame. They play the standard UI sounds themselves.

pub mod art;
pub mod backlog;
pub mod bars;
pub mod dialog;
pub mod dialogue;
pub mod format;
pub mod korean;
pub mod menu;
pub mod message;
pub mod textflow;
pub mod theme;
pub mod toast;
pub mod tooltip;
pub mod window;

pub use window::{draw_window, draw_window_ex, WindowStyle};
