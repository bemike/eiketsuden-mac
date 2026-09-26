//! Colours and metrics of the procedural Eiketsuden-style UI.

use macroquad::color::Color;

const fn rgb(hex: u32) -> Color {
    Color::from_hex(hex)
}

const fn rgba(hex: u32, a: f32) -> Color {
    Color::from_hex(hex).with_alpha(a)
}

/// Letterbox around the canvas.
pub const LETTERBOX: Color = rgb(0x05060c);
/// Plain screen background.
pub const BACKGROUND: Color = rgb(0x0a0d1c);

// Window body: deep blue vertical gradient.
pub const WIN_TOP: Color = rgb(0x24357f);
pub const WIN_BOTTOM: Color = rgb(0x0d1650);
// Darker variant for inner panels and inactive windows.
pub const PANEL_TOP: Color = rgb(0x131c4a);
pub const PANEL_BOTTOM: Color = rgb(0x0a1033);
// Bevelled frame.
pub const BORDER_OUTER: Color = rgb(0x03061a);
pub const BORDER_LIGHT: Color = rgb(0xe6ecff);
pub const BORDER_MID: Color = rgb(0x8d9bd8);
pub const BORDER_INNER: Color = rgb(0x0a1140);
pub const WINDOW_SHADOW: Color = rgba(0x000000, 0.45);

// Selection highlight.
pub const CURSOR_TOP: Color = rgba(0x7d9cff, 0.75);
pub const CURSOR_BOTTOM: Color = rgba(0x4262d6, 0.75);
pub const CURSOR_IDLE: Color = rgba(0x5a6fb8, 0.35);
pub const CURSOR_ARROW: Color = rgb(0xffe27a);

// Text.
pub const TEXT: Color = rgb(0xf4f1e4);
pub const TEXT_DIM: Color = rgb(0xa9b0d4);
pub const TEXT_DISABLED: Color = rgb(0x646b8e);
pub const TEXT_ACCENT: Color = rgb(0xf5d06a);
pub const TEXT_NAME: Color = rgb(0xffe39a);
pub const TEXT_GOOD: Color = rgb(0x8ef0a0);
pub const TEXT_BAD: Color = rgb(0xff8a7a);
pub const TEXT_SHADOW: Color = rgba(0x000000, 0.7);

// Gauges.
pub const GAUGE_BG: Color = rgb(0x0b0f24);
pub const GAUGE_FRAME: Color = rgb(0x02040f);
pub const HP_HIGH: Color = rgb(0x52d86a);
pub const HP_MID: Color = rgb(0xf0c940);
pub const HP_LOW: Color = rgb(0xf0563e);
pub const MP: Color = rgb(0x4ea6ff);
pub const EXP: Color = rgb(0xf5b83d);
pub const MORALE: Color = rgb(0xd77cff);

/// Thickness of the bevelled window frame.
pub const BORDER: f32 = 3.0;
/// Distance from the window edge to its content.
pub const PADDING: f32 = 6.0;
/// Height of a menu row.
pub const ROW_HEIGHT: f32 = 16.0;
