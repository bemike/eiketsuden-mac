//! Rendering foundation: the 480×270 virtual canvas, fonts and text, and drawing helpers.
//!
//! # Coordinate model
//!
//! Everything is drawn in **virtual pixels** on a 480×270 canvas ([`VIRTUAL_W`]×[`VIRTUAL_H`]).
//! The canvas is a render target of `480·S × 270·S` real pixels, where `S` is the largest integer
//! scale that fits the window (at least 1), presented centred and letterboxed with nearest
//! filtering. A `Camera2D` maps the virtual rectangle onto the render target, so screens never
//! deal with window sizes:
//!
//! * pixel art drawn at virtual size ends up scaled by exactly `S` — crisp at every window size;
//! * text is rasterised at `font_size · S` and drawn with `font_scale = 1/S` — one glyph texel
//!   per real pixel, so the Galmuri pixel fonts stay sharp;
//! * high resolution art (portraits, backgrounds) drawn into a virtual box keeps up to `S` times
//!   the virtual resolution.
//!
//! Positions may be fractional; text is snapped to real pixels (`1/S` virtual pixels).
//! Mouse and touch positions are converted with [`Canvas::screen_to_virtual`] (done once per
//! frame by [`crate::input::Input`]).
//!
//! # Text
//!
//! [`Gfx::text`] draws a single line whose **top-left** corner is `(x, y)`; the line box is
//! [`Gfx::line_height`] tall with the glyphs vertically centred in it. [`FontId::Main`] is
//! Galmuri11 (nominal 12 px, 16 px lines), [`FontId::Small`] Galmuri9 (nominal 10 px, 12 px
//! lines). [`TextStyle::size`] multiplies the nominal size by an integer for titles.
//! [`wrap_text`] breaks Korean text at spaces, and between syllables only when a single word is
//! longer than the line.

use macroquad::color::hsl_to_rgb;
use macroquad::prelude::*;

/// Virtual canvas width in pixels.
pub const VIRTUAL_W: f32 = 480.0;
/// Virtual canvas height in pixels.
pub const VIRTUAL_H: f32 = 270.0;
/// The virtual canvas as a rectangle.
pub const SCREEN: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: VIRTUAL_W,
    h: VIRTUAL_H,
};
/// Largest render target scale (3840×2160); bigger windows are letterboxed.
pub const MAX_SCALE: u32 = 8;

/// Largest integer scale `S` such that `480·S × 270·S` fits `screen_w × screen_h` (min 1).
pub fn integer_scale(screen_w: f32, screen_h: f32) -> u32 {
    let s = (screen_w / VIRTUAL_W).min(screen_h / VIRTUAL_H).floor();
    if s.is_finite() && s >= 1.0 {
        (s as u32).min(MAX_SCALE)
    } else {
        1
    }
}

/// Where the `480·scale × 270·scale` canvas is shown on a `screen_w × screen_h` window: centred,
/// at 1:1 when it fits, otherwise shrunk to fit (only possible for windows below 480×270).
pub fn present_rect(screen_w: f32, screen_h: f32, scale: u32) -> Rect {
    let (w, h) = (VIRTUAL_W * scale as f32, VIRTUAL_H * scale as f32);
    let fit = (screen_w / w).min(screen_h / h).min(1.0).max(0.0);
    let (w, h) = (w * fit, h * fit);
    Rect::new(
        ((screen_w - w) / 2.0).floor(),
        ((screen_h - h) / 2.0).floor(),
        w,
        h,
    )
}

/// The virtual canvas: render target, camera and presentation rectangle.
pub struct Canvas {
    target: RenderTarget,
    camera: Camera2D,
    scale: u32,
    present: Rect,
    screen: (f32, f32),
}

impl Canvas {
    pub fn new() -> Canvas {
        let (sw, sh) = (screen_width(), screen_height());
        let scale = integer_scale(sw, sh);
        let (target, camera) = Self::make_target(scale);
        Canvas {
            target,
            camera,
            scale,
            present: present_rect(sw, sh, scale),
            screen: (sw, sh),
        }
    }

    fn make_target(scale: u32) -> (RenderTarget, Camera2D) {
        let target = render_target(
            (VIRTUAL_W as u32) * scale,
            (VIRTUAL_H as u32) * scale,
        );
        target.texture.set_filter(FilterMode::Nearest);
        let mut camera = Camera2D::from_display_rect(SCREEN);
        camera.render_target = Some(target.clone());
        (target, camera)
    }

    /// Follow window size changes; recreates the render target when the scale changes.
    /// Returns `true` when the scale changed. Call once per frame before drawing.
    pub fn update(&mut self) -> bool {
        let (sw, sh) = (screen_width(), screen_height());
        if (sw, sh) == self.screen {
            return false;
        }
        self.screen = (sw, sh);
        let scale = integer_scale(sw, sh);
        let changed = scale != self.scale;
        if changed {
            let (target, camera) = Self::make_target(scale);
            self.target = target;
            self.camera = camera;
            self.scale = scale;
        }
        self.present = present_rect(sw, sh, self.scale);
        changed
    }

    /// Integer scale `S` (real pixels per virtual pixel in the render target).
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// Where the canvas appears in the window, in window pixels.
    pub fn present_rect(&self) -> Rect {
        self.present
    }

    /// Route drawing into the canvas (virtual coordinates).
    pub fn begin(&self) {
        set_camera(&self.camera);
    }

    /// Draw the canvas to the window, filling the letterbox area with `letterbox`.
    pub fn present(&self, letterbox: Color) {
        set_default_camera();
        clear_background(letterbox);
        draw_texture_ex(
            &self.target.texture,
            self.present.x,
            self.present.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(self.present.w, self.present.h)),
                flip_y: true,
                ..Default::default()
            },
        );
    }

    /// Window pixel position -> virtual position (may lie outside the canvas).
    pub fn screen_to_virtual(&self, p: Vec2) -> Vec2 {
        screen_to_virtual(self.present, p)
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Canvas::new()
    }
}

/// Pure mapping used by [`Canvas::screen_to_virtual`].
pub fn screen_to_virtual(present: Rect, p: Vec2) -> Vec2 {
    if present.w <= 0.0 || present.h <= 0.0 {
        return Vec2::ZERO;
    }
    vec2(
        (p.x - present.x) * VIRTUAL_W / present.w,
        (p.y - present.y) * VIRTUAL_H / present.h,
    )
}

// ----- fonts ---------------------------------------------------------------------------------

/// The two UI fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Galmuri11, nominal 12 px — UI and dialogue.
    Main,
    /// Galmuri9, nominal 10 px — small numbers and captions.
    Small,
}

impl FontId {
    /// Size the font was designed for, in virtual pixels.
    pub fn nominal_px(self) -> u16 {
        match self {
            FontId::Main => 12,
            FontId::Small => 10,
        }
    }

    /// Line box height at size 1, in virtual pixels.
    pub fn line_height(self) -> f32 {
        match self {
            FontId::Main => 16.0,
            FontId::Small => 12.0,
        }
    }

    /// Pack-relative file of the font.
    pub fn file(self) -> &'static str {
        match self {
            FontId::Main => "fonts/Galmuri11.ttf",
            FontId::Small => "fonts/Galmuri9.ttf",
        }
    }
}

struct LoadedFont {
    font: Font,
    /// Distance from the top of the line box to the baseline at size 1.
    baseline: f32,
}

impl LoadedFont {
    fn new(font: Font, id: FontId) -> LoadedFont {
        let nominal = id.nominal_px();
        // Reference glyphs: a Hangul syllable, a capital and a descender.
        let dims = measure_text("가Ag", Some(&font), nominal, 1.0);
        let pad = ((id.line_height() - dims.height) / 2.0).round().max(0.0);
        LoadedFont {
            font,
            baseline: (pad + dims.offset_y).round(),
        }
    }
}

/// The loaded UI fonts. Missing font files fall back to macroquad's built-in font (Latin only),
/// recorded in [`Fonts::missing`].
pub struct Fonts {
    main: LoadedFont,
    small: LoadedFont,
    missing: Vec<String>,
}

impl Fonts {
    /// Fonts before the pack fonts are loaded: macroquad's built-in font for both.
    pub fn fallback() -> Fonts {
        Fonts {
            main: LoadedFont::new(get_default_font(), FontId::Main),
            small: LoadedFont::new(get_default_font(), FontId::Small),
            missing: Vec::new(),
        }
    }

    /// Install a font from TTF bytes. On failure the fallback font stays and the error is
    /// recorded.
    pub fn install(&mut self, id: FontId, bytes: Result<&[u8], String>) {
        let result = bytes.and_then(|b| {
            load_ttf_font_from_bytes(b).map_err(|e| format!("{}: {e}", id.file()))
        });
        match result {
            Ok(mut font) => {
                font.set_filter(FilterMode::Nearest);
                let loaded = LoadedFont::new(font, id);
                match id {
                    FontId::Main => self.main = loaded,
                    FontId::Small => self.small = loaded,
                }
            }
            Err(e) => {
                macroquad::logging::error!("font {:?} unavailable, using fallback: {}", id, e);
                self.missing.push(e);
            }
        }
    }

    /// Font files that could not be loaded (the built-in fallback is used for them).
    pub fn missing(&self) -> &[String] {
        &self.missing
    }

    fn get(&self, id: FontId) -> &LoadedFont {
        match id {
            FontId::Main => &self.main,
            FontId::Small => &self.small,
        }
    }
}

// ----- text ----------------------------------------------------------------------------------

/// Horizontal alignment inside a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// How a line of text is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    /// Integer multiple of the nominal size (1 = 12 px for `Main`).
    pub size: u8,
    pub color: Color,
    /// 1-pixel drop shadow colour, drawn below-right.
    pub shadow: Option<Color>,
}

impl TextStyle {
    pub const fn main(color: Color) -> TextStyle {
        TextStyle {
            font: FontId::Main,
            size: 1,
            color,
            shadow: None,
        }
    }

    pub const fn small(color: Color) -> TextStyle {
        TextStyle {
            font: FontId::Small,
            size: 1,
            color,
            shadow: None,
        }
    }

    pub const fn size(mut self, size: u8) -> TextStyle {
        self.size = if size == 0 { 1 } else { size };
        self
    }

    pub const fn shadow(mut self, color: Color) -> TextStyle {
        self.shadow = Some(color);
        self
    }

    pub const fn color(mut self, color: Color) -> TextStyle {
        self.color = color;
        self
    }
}

/// Canvas plus fonts: everything needed to draw. Owned by [`crate::app::Ctx`].
pub struct Gfx {
    pub canvas: Canvas,
    pub fonts: Fonts,
}

impl Gfx {
    pub fn new() -> Gfx {
        Gfx {
            canvas: Canvas::new(),
            fonts: Fonts::fallback(),
        }
    }

    /// Integer canvas scale `S`.
    pub fn scale(&self) -> u32 {
        self.canvas.scale()
    }

    fn params(&self, font: FontId, size: u8, color: Color) -> TextParams<'_> {
        let s = self.scale();
        TextParams {
            font: Some(&self.fonts.get(font).font),
            font_size: font.nominal_px() * u16::from(size.max(1)) * s as u16,
            font_scale: 1.0 / s as f32,
            color,
            ..Default::default()
        }
    }

    /// Snap a virtual coordinate to the real pixel grid.
    pub fn snap(&self, v: f32) -> f32 {
        let s = self.scale() as f32;
        (v * s).round() / s
    }

    /// Height of a line box.
    pub fn line_height(&self, font: FontId, size: u8) -> f32 {
        font.line_height() * f32::from(size.max(1))
    }

    /// Width of `text` in virtual pixels.
    pub fn text_width(&self, text: &str, font: FontId, size: u8) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let p = self.params(font, size, WHITE);
        measure_text(text, p.font, p.font_size, p.font_scale).width
    }

    /// Advance width of one character.
    pub fn char_width(&self, c: char, font: FontId, size: u8) -> f32 {
        let mut buf = [0u8; 4];
        self.text_width(c.encode_utf8(&mut buf), font, size)
    }

    /// Draw one line with its line box's top-left corner at `(x, y)`. Returns the width.
    pub fn text(&self, text: &str, x: f32, y: f32, style: TextStyle) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let size = style.size.max(1);
        let baseline = self.fonts.get(style.font).baseline * f32::from(size);
        let (x, y) = (self.snap(x), self.snap(y + baseline));
        if let Some(shadow) = style.shadow {
            draw_text_ex(
                text,
                x + f32::from(size),
                y + f32::from(size),
                self.params(style.font, size, shadow),
            );
        }
        draw_text_ex(text, x, y, self.params(style.font, size, style.color)).width
    }

    /// Draw one line aligned inside `[x, x + w]`.
    pub fn text_aligned(&self, text: &str, x: f32, y: f32, w: f32, align: Align, style: TextStyle) {
        let tx = match align {
            Align::Left => x,
            Align::Center => x + ((w - self.text_width(text, style.font, style.size)) / 2.0).round(),
            Align::Right => x + w - self.text_width(text, style.font, style.size),
        };
        self.text(text, tx, y, style);
    }

    /// Draw pre-wrapped lines; returns the total height.
    pub fn text_lines<S: AsRef<str>>(&self, lines: &[S], x: f32, y: f32, style: TextStyle) -> f32 {
        let lh = self.line_height(style.font, style.size);
        for (i, line) in lines.iter().enumerate() {
            self.text(line.as_ref(), x, y + lh * i as f32, style);
        }
        lh * lines.len() as f32
    }

    /// Wrap `text` to `max_width` virtual pixels (see [`wrap_text`]).
    pub fn wrap(&self, text: &str, font: FontId, size: u8, max_width: f32) -> Vec<String> {
        wrap_text(text, max_width, |c| self.char_width(c, font, size))
    }
}

impl Default for Gfx {
    fn default() -> Self {
        Gfx::new()
    }
}

/// Characters that may not start a line.
fn no_break_before(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | '!'
            | '?'
            | ':'
            | ';'
            | ')'
            | ']'
            | '}'
            | '…'
            | '」'
            | '』'
            | '》'
            | '〉'
            | '。'
            | '、'
            | '！'
            | '？'
            | '）'
            | '~'
            | '\''
            | '"'
            | '’'
            | '”'
    )
}

/// Characters that may not end a line.
fn no_break_after(c: char) -> bool {
    matches!(c, '(' | '[' | '{' | '「' | '『' | '《' | '〈' | '（' | '‘' | '“')
}

/// Hangul, kana and CJK ideographs: scripts where a line may break between two characters.
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF       // Hangul Jamo
        | 0x3040..=0x30FF     // Hiragana, Katakana
        | 0x3130..=0x318F     // Hangul compatibility Jamo
        | 0x3400..=0x4DBF     // CJK extension A
        | 0x4E00..=0x9FFF     // CJK unified ideographs
        | 0xAC00..=0xD7A3     // Hangul syllables
        | 0xF900..=0xFAFF     // CJK compatibility ideographs
        | 0xFF00..=0xFFEF     // full-width forms
    )
}

fn can_break_between(prev: char, next: char) -> bool {
    !no_break_before(next) && !no_break_after(prev) && (is_cjk(prev) || is_cjk(next))
}

/// Word wrap for Korean (and mixed Latin/Hanja) text.
///
/// * explicit `\n` always breaks (empty lines are kept);
/// * lines break at spaces whenever possible (the spaces are dropped);
/// * a word longer than the line breaks between syllables (Hangul/CJK) without splitting before
///   closing punctuation, and as a last resort between any two characters;
/// * `advance` returns the width of one character.
pub fn wrap_text(text: &str, max_width: f32, mut advance: impl FnMut(char) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let chars: Vec<char> = paragraph
            .chars()
            .filter(|c| *c != '\r')
            .map(|c| if c == '\t' { ' ' } else { c })
            .collect();
        let mut start = 0;
        loop {
            // Skip spaces at the start of a continuation line.
            if start > 0 {
                while start < chars.len() && chars[start] == ' ' {
                    start += 1;
                }
            }
            let mut width = 0.0;
            let mut last_space: Option<usize> = None;
            let mut last_break: Option<usize> = None;
            let mut i = start;
            let mut cut: Option<(usize, usize)> = None; // (line end, next start)
            while i < chars.len() {
                let c = chars[i];
                let w = advance(c);
                if width + w > max_width && i > start {
                    cut = Some(if c == ' ' {
                        (i, i + 1)
                    } else if let Some(sp) = last_space {
                        (sp, sp + 1)
                    } else if let Some(b) = last_break {
                        (b, b)
                    } else {
                        (i, i)
                    });
                    break;
                }
                if c == ' ' {
                    last_space = Some(i);
                } else if i > start && can_break_between(chars[i - 1], c) {
                    last_break = Some(i);
                }
                width += w;
                i += 1;
            }
            match cut {
                Some((end, next)) => {
                    let line: String = chars[start..end].iter().collect();
                    lines.push(line.trim_end().to_string());
                    start = next;
                }
                None => {
                    let line: String = chars[start..].iter().collect();
                    lines.push(line.trim_end().to_string());
                    break;
                }
            }
        }
    }
    lines
}

// ----- drawing helpers -----------------------------------------------------------------------

/// Solid rectangle.
pub fn fill_rect(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, color);
}

/// 1-pixel outline drawn inside `r` (four filled strips, so it stays on the pixel grid).
pub fn stroke_rect(r: Rect, color: Color) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    draw_rectangle(r.x, r.y, r.w, 1.0, color);
    draw_rectangle(r.x, r.y + r.h - 1.0, r.w, 1.0, color);
    draw_rectangle(r.x, r.y + 1.0, 1.0, r.h - 2.0, color);
    draw_rectangle(r.x + r.w - 1.0, r.y + 1.0, 1.0, r.h - 2.0, color);
}

/// Rectangle with a vertical colour gradient.
pub fn fill_gradient_v(r: Rect, top: Color, bottom: Color) {
    let vertices = [
        Vertex::new(r.x, r.y, 0.0, 0.0, 0.0, top),
        Vertex::new(r.x + r.w, r.y, 0.0, 1.0, 0.0, top),
        Vertex::new(r.x + r.w, r.y + r.h, 0.0, 1.0, 1.0, bottom),
        Vertex::new(r.x, r.y + r.h, 0.0, 0.0, 1.0, bottom),
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    // SAFETY: `get_internal_gl` is only unsafe because it hands out the renderer while other
    // macroquad calls could also use it; we issue one self-contained geometry batch on the main
    // thread, exactly what `draw_rectangle` does internally.
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    gl.quad_gl.geometry(&vertices, &indices);
}

/// Rectangle with a horizontal colour gradient.
pub fn fill_gradient_h(r: Rect, left: Color, right: Color) {
    let vertices = [
        Vertex::new(r.x, r.y, 0.0, 0.0, 0.0, left),
        Vertex::new(r.x + r.w, r.y, 0.0, 1.0, 0.0, right),
        Vertex::new(r.x + r.w, r.y + r.h, 0.0, 1.0, 1.0, right),
        Vertex::new(r.x, r.y + r.h, 0.0, 0.0, 1.0, left),
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    // SAFETY: see `fill_gradient_v`.
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.texture(None);
    gl.quad_gl.draw_mode(DrawMode::Triangles);
    gl.quad_gl.geometry(&vertices, &indices);
}

/// Draw one frame of a sprite sheet laid out as a grid of `frame`-sized cells, with the frame's
/// top-left at `pos` (virtual pixels), at 1 virtual pixel per texel.
pub fn draw_sprite_frame(
    texture: &Texture2D,
    frame: Vec2,
    cell: (u32, u32),
    pos: Vec2,
    flip_x: bool,
    tint: Color,
) {
    let source = Rect::new(
        cell.0 as f32 * frame.x,
        cell.1 as f32 * frame.y,
        frame.x,
        frame.y,
    );
    draw_texture_ex(
        texture,
        pos.x.round(),
        pos.y.round(),
        tint,
        DrawTextureParams {
            dest_size: Some(frame),
            source: Some(source),
            flip_x,
            ..Default::default()
        },
    );
}

/// How [`draw_texture_fit`] maps an image into a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Whole image visible, centred, aspect kept (may leave empty bands).
    Contain,
    /// Box completely covered, aspect kept, overflow cropped evenly.
    Cover,
    /// Stretched to the box.
    Stretch,
}

/// Draw a texture into `dest` (virtual pixels). Used for high resolution art such as portraits
/// and backgrounds, which keep their detail on large canvases.
pub fn draw_texture_fit(texture: &Texture2D, dest: Rect, fit: Fit, tint: Color) {
    let (tw, th) = (texture.width(), texture.height());
    if tw <= 0.0 || th <= 0.0 || dest.w <= 0.0 || dest.h <= 0.0 {
        return;
    }
    let (source, target) = fit_rects(vec2(tw, th), dest, fit);
    draw_texture_ex(
        texture,
        target.x,
        target.y,
        tint,
        DrawTextureParams {
            dest_size: Some(vec2(target.w, target.h)),
            source: Some(source),
            ..Default::default()
        },
    );
}

/// Source rectangle (texels) and destination rectangle for [`draw_texture_fit`].
pub fn fit_rects(size: Vec2, dest: Rect, fit: Fit) -> (Rect, Rect) {
    let full = Rect::new(0.0, 0.0, size.x, size.y);
    match fit {
        Fit::Stretch => (full, dest),
        Fit::Contain => {
            let k = (dest.w / size.x).min(dest.h / size.y);
            let (w, h) = (size.x * k, size.y * k);
            (
                full,
                Rect::new(dest.x + (dest.w - w) / 2.0, dest.y + (dest.h - h) / 2.0, w, h),
            )
        }
        Fit::Cover => {
            let k = (dest.w / size.x).max(dest.h / size.y);
            let (sw, sh) = (dest.w / k, dest.h / k);
            (
                Rect::new((size.x - sw) / 2.0, (size.y - sh) / 2.0, sw, sh),
                dest,
            )
        }
    }
}

/// Stable colour derived from a key (for placeholders of missing art).
pub fn key_color(key: &str) -> Color {
    // FNV-1a, then pick a hue; saturation/value fixed so placeholders look alike.
    let mut h: u32 = 0x811c_9dc5;
    for b in key.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let hue = (h % 360) as f32 / 360.0;
    hsl_to_rgb(hue, 0.55, 0.45)
}

/// Coloured box with a cross, drawn in place of a missing sprite or image.
pub fn draw_placeholder(r: Rect, key: &str) {
    let c = key_color(key);
    fill_rect(r, Color::new(c.r, c.g, c.b, 0.85));
    stroke_rect(r, Color::new(0.0, 0.0, 0.0, 0.8));
    let inner = Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0);
    let light = Color::new(1.0, 1.0, 1.0, 0.45);
    draw_line(inner.x, inner.y, inner.right(), inner.bottom(), 1.0, light);
    draw_line(inner.right(), inner.y, inner.x, inner.bottom(), 1.0, light);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_mono(text: &str, width: f32) -> Vec<String> {
        // Monospace: every character is 1 wide except Hangul/Hanja which are 2 wide.
        wrap_text(text, width, |c| if is_cjk(c) { 2.0 } else { 1.0 })
    }

    #[test]
    fn scale_and_presentation() {
        assert_eq!(integer_scale(1440.0, 810.0), 3);
        assert_eq!(integer_scale(1920.0, 1080.0), 4);
        assert_eq!(integer_scale(1366.0, 768.0), 2);
        assert_eq!(integer_scale(300.0, 200.0), 1);
        assert_eq!(integer_scale(0.0, 0.0), 1);
        assert_eq!(integer_scale(100_000.0, 100_000.0), MAX_SCALE);

        let r = present_rect(1366.0, 768.0, 2);
        assert_eq!((r.x, r.y, r.w, r.h), (203.0, 114.0, 960.0, 540.0));
        // A window smaller than the canvas shrinks it to fit, keeping the aspect ratio.
        let r = present_rect(240.0, 270.0, 1);
        assert_eq!((r.w, r.h), (240.0, 135.0));
    }

    #[test]
    fn mouse_mapping() {
        let present = Rect::new(203.0, 114.0, 960.0, 540.0);
        assert_eq!(screen_to_virtual(present, vec2(203.0, 114.0)), vec2(0.0, 0.0));
        assert_eq!(screen_to_virtual(present, vec2(1163.0, 654.0)), vec2(480.0, 270.0));
        assert_eq!(screen_to_virtual(present, vec2(683.0, 384.0)), vec2(240.0, 135.0));
        assert_eq!(screen_to_virtual(Rect::new(0.0, 0.0, 0.0, 0.0), vec2(5.0, 5.0)), Vec2::ZERO);
    }

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap_mono("aaa bbb ccc", 7.0), vec!["aaa bbb", "ccc"]);
        assert_eq!(wrap_mono("aaa bbb ccc", 3.0), vec!["aaa", "bbb", "ccc"]);
        // Korean words (2 units per syllable) break at spaces, not inside words.
        assert_eq!(
            wrap_mono("어지러운 세상이로구나 백성을", 20.0),
            vec!["어지러운", "세상이로구나 백성을"]
        );
    }

    #[test]
    fn long_words_break_between_syllables() {
        assert_eq!(wrap_mono("가나다라마바", 6.0), vec!["가나다", "라마바"]);
        // Closing punctuation stays with the previous syllable.
        assert_eq!(wrap_mono("가나다.라", 6.0), vec!["가나", "다.라"]);
        // Latin without spaces breaks anywhere as a last resort.
        assert_eq!(wrap_mono("abcdefgh", 3.0), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn explicit_newlines_and_edge_cases() {
        assert_eq!(wrap_mono("a\n\nb", 10.0), vec!["a", "", "b"]);
        assert_eq!(wrap_mono("", 10.0), vec![""]);
        assert_eq!(wrap_mono("ab  \r\ncd", 10.0), vec!["ab", "cd"]);
        // A character wider than the line still makes progress.
        assert_eq!(wrap_mono("가나", 1.0), vec!["가", "나"]);
        // Leading spaces of a paragraph are kept, those after a wrap are dropped.
        assert_eq!(wrap_mono("  ab cd", 5.0), vec!["  ab", "cd"]);
    }

    #[test]
    fn fit_rectangles() {
        let dest = Rect::new(0.0, 0.0, 64.0, 80.0);
        let (src, dst) = fit_rects(vec2(192.0, 240.0), dest, Fit::Contain);
        assert_eq!(src, Rect::new(0.0, 0.0, 192.0, 240.0));
        assert_eq!(dst, dest);
        let (src, dst) = fit_rects(vec2(200.0, 100.0), Rect::new(0.0, 0.0, 100.0, 100.0), Fit::Cover);
        assert_eq!(src, Rect::new(50.0, 0.0, 100.0, 100.0));
        assert_eq!(dst, Rect::new(0.0, 0.0, 100.0, 100.0));
        let (_, dst) = fit_rects(vec2(200.0, 100.0), Rect::new(0.0, 0.0, 100.0, 100.0), Fit::Contain);
        assert_eq!(dst, Rect::new(0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn key_colors_are_stable() {
        assert_eq!(key_color("units/archer"), key_color("units/archer"));
        assert_ne!(key_color("units/archer"), key_color("units/bandit"));
    }
}
