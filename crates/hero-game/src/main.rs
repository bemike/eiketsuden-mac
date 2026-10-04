//! `eiketsuden` — the game executable (native) and `eiketsuden.wasm` (browser).
//!
//! Native command line: `eiketsuden [--data <pack dir>] [--original <overlay dir>] [--gallery]`;
//! web: `index.html#gallery`.
//! Everything else lives in the `hero_game` library (see its crate docs).

// Release builds on Windows are GUI applications without a console window; panics are then
// reported in `crash.log` in the user data directory (see `platform::report_panic`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use hero_game::platform::{self, LaunchOptions};
use macroquad::miniquad::conf::Icon;
use macroquad::prelude::*;

/// Window title (also the browser tab title is set by `web/index.html`).
const TITLE: &str = "三国志英杰传 · 原生 Mac 版";

fn window_conf() -> Conf {
    Conf {
        window_title: TITLE.to_owned(),
        // 3× the default 480×270 virtual canvas (packs may declare another one).
        window_width: 1280,
        window_height: 800,
        window_resizable: true,
        // The renderer does its own integer scaling of the virtual canvas; physical pixels on
        // high-DPI screens would only change the window size, not the look.
        high_dpi: false,
        // macOS must use the bundle's multi-resolution ICNS. Miniquad otherwise
        // replaces NSApplication's icon with its 64x64 runtime bitmap, making
        // the Dock and application switcher blurry even with a sharp bundle icon.
        icon: platform_window_icon(),
        platform: macroquad::miniquad::conf::Platform {
            apple_gfx_api: if std::env::var("EIKETSUDEN_RENDERER").as_deref() != Ok("metal") {
                macroquad::miniquad::conf::AppleGfxApi::OpenGl
            } else {
                macroquad::miniquad::conf::AppleGfxApi::Metal
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(target_os = "macos")]
fn platform_window_icon() -> Option<Icon> {
    None
}

#[cfg(not(target_os = "macos"))]
fn platform_window_icon() -> Option<Icon> {
    Some(window_icon())
}

/// The window and taskbar bitmap on platforms that use runtime icons.
#[cfg(not(target_os = "macos"))]
fn window_icon() -> Icon {
    Icon {
        small: *include_bytes!("../icon/icon_16.rgba"),
        medium: *include_bytes!("../icon/icon_32.rgba"),
        big: *include_bytes!("../icon/icon_64.rgba"),
    }
}

/// Route panics to [`platform::report_panic`] (console / crash log / web overlay). Natively the
/// default hook still runs afterwards so the usual message and backtrace reach stderr.
fn install_panic_hook() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            platform::report_panic(&info.to_string());
            default_hook(info);
        }));
    }
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| platform::report_panic(&info.to_string())));
}

#[macroquad::main(window_conf)]
async fn main() {
    install_panic_hook();
    let mut options = LaunchOptions::from_environment();
    // Finder does not set a useful working directory. Resolve the self-contained app's
    // converted data relative to its executable, so moving the .app keeps it playable.
    if options.data_dir.is_none() && std::env::var_os(platform::DATA_ENV).is_none() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(contents) = exe.parent().and_then(|p| p.parent()) {
                let bundled = contents.join("Resources/data/original");
                if bundled.join("pack.toml").is_file() {
                    options.data_dir = Some(bundled);
                }
            }
        }
    }
    hero_game::app::run(options).await;
}
