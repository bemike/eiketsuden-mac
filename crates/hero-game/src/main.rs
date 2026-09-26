//! `eiketsuden` — the game executable (native) and `eiketsuden.wasm` (browser).
//!
//! Native command line: `eiketsuden [--data <pack dir>] [--gallery]`; web: `index.html#gallery`.
//! Everything else lives in the `hero_game` library (see its crate docs).

// Release builds on Windows are GUI applications without a console window; panics are then
// reported in `crash.log` in the user data directory (see `platform::report_panic`).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use hero_game::platform::{self, LaunchOptions};
use macroquad::prelude::*;

/// Window title (also the browser tab title is set by `web/index.html`).
const TITLE: &str = "영걸전 Reloaded";

fn window_conf() -> Conf {
    Conf {
        window_title: TITLE.to_owned(),
        // 3× the 480×270 virtual canvas.
        window_width: 1440,
        window_height: 810,
        window_resizable: true,
        // The renderer does its own integer scaling of the virtual canvas; physical pixels on
        // high-DPI screens would only change the window size, not the look.
        high_dpi: false,
        ..Default::default()
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
    hero_game::app::run(LaunchOptions::from_environment()).await;
}
