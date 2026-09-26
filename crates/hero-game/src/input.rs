//! Per-frame input snapshot: the one place screens query keyboard, mouse and touch.
//!
//! [`Input::update`] runs once per frame (by the app) before the active screen updates.
//!
//! | action | keys | pointer |
//! |---|---|---|
//! | confirm ([`Input::confirm`]) | Z, Enter, Space, keypad Enter | left click / tap ([`Input::tap`]) |
//! | cancel ([`Input::cancel`]) | X, Esc, Backspace | right click |
//! | navigation ([`Input::nav`]) | arrow keys, WASD — with key repeat | mouse wheel ([`Input::wheel`]) |
//!
//! Touch input arrives as mouse events (macroquad simulates the left button). A *tap* is a
//! press and release without moving more than [`DRAG_THRESHOLD`] virtual pixels; moving further
//! turns the gesture into a [`Drag`] (used for scrolling lists and panning the battle map).
//! All pointer positions are in virtual canvas coordinates.
//!
//! Screens that react to a key and then change state should call [`Input::consume`] so a later
//! widget in the same frame does not see the same press again.

use crate::gfx::Canvas;
use macroquad::prelude::*;

/// Delay before a held direction key starts repeating, in seconds.
pub const REPEAT_DELAY: f32 = 0.32;
/// Interval between repeats of a held direction key, in seconds.
pub const REPEAT_RATE: f32 = 0.075;
/// Pointer movement (virtual pixels) that turns a press into a drag.
pub const DRAG_THRESHOLD: f32 = 4.0;

const CONFIRM_KEYS: [KeyCode; 4] = [KeyCode::Z, KeyCode::Enter, KeyCode::Space, KeyCode::KpEnter];
const CANCEL_KEYS: [KeyCode; 3] = [KeyCode::X, KeyCode::Escape, KeyCode::Backspace];

/// A navigation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];

    fn keys(self) -> [KeyCode; 2] {
        match self {
            Dir::Up => [KeyCode::Up, KeyCode::W],
            Dir::Down => [KeyCode::Down, KeyCode::S],
            Dir::Left => [KeyCode::Left, KeyCode::A],
            Dir::Right => [KeyCode::Right, KeyCode::D],
        }
    }

    /// Unit step `(dx, dy)` in screen orientation (y grows downwards).
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }
}

/// Key repeat state machine for the direction keys (pure, unit tested).
#[derive(Debug, Clone, Default)]
pub struct KeyRepeat {
    held: Option<Dir>,
    timer: f32,
}

impl KeyRepeat {
    /// Advance by `dt`. `pressed` is a direction pressed this frame, `down` reports whether a
    /// direction is currently held. Returns the direction to act on this frame, if any.
    pub fn step(
        &mut self,
        dt: f32,
        pressed: Option<Dir>,
        down: impl Fn(Dir) -> bool,
    ) -> Option<Dir> {
        if let Some(d) = pressed {
            self.held = Some(d);
            self.timer = REPEAT_DELAY;
            return Some(d);
        }
        let held = self.held?;
        if !down(held) {
            self.held = None;
            return None;
        }
        self.timer -= dt;
        if self.timer <= 0.0 {
            // Fire at most once per frame; after a long frame restart the interval instead of
            // firing a burst on the following frames.
            self.timer += REPEAT_RATE;
            if self.timer <= 0.0 {
                self.timer = REPEAT_RATE;
            }
            Some(held)
        } else {
            None
        }
    }
}

/// An ongoing pointer drag, in virtual pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// Where the press started.
    pub origin: Vec2,
    /// Current pointer position.
    pub pos: Vec2,
    /// Movement since the previous frame.
    pub delta: Vec2,
}

#[derive(Debug, Clone, Copy, Default)]
struct Press {
    origin: Vec2,
    dragging: bool,
}

/// This frame's input. Owned by [`crate::app::Ctx`]; see the module docs.
#[derive(Debug, Default)]
pub struct Input {
    confirm_key: bool,
    cancel_key: bool,
    nav: Option<Dir>,
    repeat: KeyRepeat,
    pointer: Vec2,
    prev_pointer: Vec2,
    pointer_moved: bool,
    left_pressed: bool,
    left_down: bool,
    right_pressed: bool,
    tap: Option<Vec2>,
    press: Option<Press>,
    drag: Option<Drag>,
    wheel: i32,
    any: bool,
    consumed: bool,
    /// Size of the canvas the pointer positions refer to (zero before the first update, when
    /// no pointer position is known yet).
    canvas: Vec2,
}

impl Input {
    pub fn new() -> Input {
        Input::default()
    }

    /// Read macroquad's input state for this frame. Called by the app once per frame.
    pub fn update(&mut self, dt: f32, canvas: &Canvas) {
        self.consumed = false;
        self.canvas = canvas.size();
        let any_key = |keys: &[KeyCode]| keys.iter().any(|k| is_key_pressed(*k));
        self.confirm_key = any_key(&CONFIRM_KEYS);
        self.cancel_key = any_key(&CANCEL_KEYS);

        let pressed_dir = Dir::ALL.into_iter().find(|d| any_key(&d.keys()));
        let down = |d: Dir| d.keys().iter().any(|k| is_key_down(*k));
        self.nav = self.repeat.step(dt, pressed_dir, down);

        let (mx, my) = mouse_position();
        let pointer = canvas.screen_to_virtual(vec2(mx, my));
        self.prev_pointer = self.pointer;
        self.pointer_moved = pointer != self.pointer;
        self.pointer = pointer;

        self.left_pressed = is_mouse_button_pressed(MouseButton::Left);
        self.left_down = is_mouse_button_down(MouseButton::Left);
        self.right_pressed = is_mouse_button_pressed(MouseButton::Right);
        let released = is_mouse_button_released(MouseButton::Left);

        self.tap = None;
        self.drag = None;
        if self.left_pressed {
            self.press = Some(Press {
                origin: pointer,
                dragging: false,
            });
        }
        if let Some(press) = self.press.as_mut() {
            if !press.dragging && press.origin.distance(pointer) > DRAG_THRESHOLD {
                press.dragging = true;
            }
            if press.dragging {
                self.drag = Some(Drag {
                    origin: press.origin,
                    pos: pointer,
                    delta: pointer - self.prev_pointer,
                });
            }
            if released || !self.left_down {
                if !press.dragging && in_canvas(pointer, self.canvas) {
                    self.tap = Some(pointer);
                }
                self.press = None;
            }
        }

        let (_, wy) = mouse_wheel();
        self.wheel = if wy > 0.0 {
            -1
        } else if wy < 0.0 {
            1
        } else {
            0
        };

        self.any = self.confirm_key
            || self.cancel_key
            || pressed_dir.is_some()
            || self.left_pressed
            || self.right_pressed
            || get_last_key_pressed().is_some();
    }

    /// Forget this frame's events (presses, taps, navigation, wheel). Held state and the pointer
    /// position stay available.
    pub fn consume(&mut self) {
        self.consumed = true;
    }

    fn live(&self) -> bool {
        !self.consumed
    }

    /// Confirm: a confirm key or a tap anywhere.
    pub fn confirm(&self) -> bool {
        self.live() && (self.confirm_key || self.tap.is_some())
    }

    /// Confirm pressed on the keyboard (widgets use this plus their own hit-tested taps).
    pub fn confirm_key(&self) -> bool {
        self.live() && self.confirm_key
    }

    /// Cancel: a cancel key or a right click.
    pub fn cancel(&self) -> bool {
        self.live() && (self.cancel_key || self.right_pressed)
    }

    /// Direction pressed or repeated this frame.
    pub fn nav(&self) -> Option<Dir> {
        if self.live() {
            self.nav
        } else {
            None
        }
    }

    /// Whether a direction key is held down.
    pub fn held(&self, dir: Dir) -> bool {
        dir.keys().iter().any(|k| is_key_down(*k))
    }

    /// A specific key pressed this frame (for screen-specific shortcuts).
    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.live() && is_key_pressed(key)
    }

    /// A specific key held down.
    pub fn key_down(&self, key: KeyCode) -> bool {
        is_key_down(key)
    }

    /// Pointer position in virtual pixels, `None` when outside the canvas.
    pub fn pointer(&self) -> Option<Vec2> {
        in_canvas(self.pointer, self.canvas).then_some(self.pointer)
    }

    /// The pointer moved this frame (hover highlighting should follow only real movement, so a
    /// resting mouse does not fight keyboard navigation).
    pub fn pointer_moved(&self) -> bool {
        self.pointer_moved
    }

    /// Left button / touch went down this frame (at [`Input::pointer`]).
    pub fn pressed(&self) -> bool {
        self.live() && self.left_pressed
    }

    /// Left button / touch is held.
    pub fn down(&self) -> bool {
        self.left_down
    }

    /// A click or tap completed this frame, at this virtual position.
    pub fn tap(&self) -> Option<Vec2> {
        if self.live() {
            self.tap
        } else {
            None
        }
    }

    /// A tap inside `r`.
    pub fn tapped(&self, r: Rect) -> bool {
        self.tap().is_some_and(|p| r.contains(p))
    }

    /// The pointer is over `r`.
    pub fn hovering(&self, r: Rect) -> bool {
        self.pointer().is_some_and(|p| r.contains(p))
    }

    pub fn right_click(&self) -> bool {
        self.live() && self.right_pressed
    }

    /// Wheel steps this frame: positive = scroll down / next, negative = up / previous.
    pub fn wheel(&self) -> i32 {
        if self.live() {
            self.wheel
        } else {
            0
        }
    }

    /// The current drag gesture, if the pointer is held and has moved past the threshold.
    pub fn drag(&self) -> Option<Drag> {
        if self.live() {
            self.drag
        } else {
            None
        }
    }

    /// Any key, button or touch was pressed this frame (used to unlock web audio).
    pub fn any_activity(&self) -> bool {
        self.any
    }
}

/// Whether `p` lies on a `canvas` sized canvas.
fn in_canvas(p: Vec2, canvas: Vec2) -> bool {
    p.x >= 0.0 && p.y >= 0.0 && p.x < canvas.x && p.y < canvas.y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_fires_after_delay_then_at_rate() {
        let mut r = KeyRepeat::default();
        let held = |d: Dir| d == Dir::Down;
        assert_eq!(r.step(0.016, Some(Dir::Down), held), Some(Dir::Down));
        // Nothing during the initial delay.
        let mut t = 0.0;
        while t + 0.016 < REPEAT_DELAY {
            assert_eq!(r.step(0.016, None, held), None);
            t += 0.016;
        }
        // Fires once the delay is over, then every REPEAT_RATE seconds.
        let mut fired = 0;
        for _ in 0..60 {
            if r.step(0.016, None, held).is_some() {
                fired += 1;
            }
        }
        let expected = (60.0 * 0.016 / REPEAT_RATE) as i32;
        assert!(
            (fired - expected).abs() <= 2,
            "fired {fired}, expected ~{expected}"
        );
    }

    #[test]
    fn repeat_stops_on_release_and_fires_once_per_frame() {
        let mut r = KeyRepeat::default();
        assert_eq!(r.step(0.0, Some(Dir::Left), |_| true), Some(Dir::Left));
        // A huge frame fires one repeat and does not cause a burst afterwards.
        assert_eq!(r.step(5.0, None, |_| true), Some(Dir::Left));
        assert_eq!(r.step(0.0, None, |_| true), None);
        assert_eq!(r.step(REPEAT_RATE, None, |_| true), Some(Dir::Left));
        assert_eq!(r.step(0.01, None, |_| false), None);
        assert_eq!(r.step(1.0, None, |_| true), None);
    }

    #[test]
    fn canvas_bounds() {
        let c = crate::gfx::DEFAULT_CANVAS;
        assert!(in_canvas(vec2(0.0, 0.0), c));
        assert!(in_canvas(vec2(479.9, 269.9), c));
        assert!(!in_canvas(vec2(480.0, 10.0), c));
        assert!(!in_canvas(vec2(-0.1, 10.0), c));
        let vga = vec2(640.0, 480.0);
        assert!(in_canvas(vec2(600.0, 400.0), vga));
        assert!(!in_canvas(vec2(600.0, 480.0), vga));
    }
}
