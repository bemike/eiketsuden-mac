//! The duel scene of dramas (`@duel`, `@duel_act`, `@duel_end`, docs/MODDING.md): two mounted
//! officers on a stage 26 cells of 16 pixels wide (the original's duel screen), the left one
//! facing right and the right one mirrored, over the background `gfx/duel/<bg>.png`.
//!
//! A fighter is drawn from a sheet of fifteen 96×96 frames in a row (docs/ASSETS.md): 0–3
//! galloping, 4–11 attacking (pairs), 12 falling, 13 lying, 14 the horse alone. The sheet is
//! `gfx/duel/<officer>.png` for the officer, else `gfx/duel/left.png` / `right.png` for the side.
//! Without a sheet the fighter is not drawn (the scene still runs its timing).

use crate::app::Ctx;
use crate::assets::AssetState;
use crate::audio::sfx;
use crate::gfx::fill_rect;
use hero_core::script::{DuelAct, DuelSide};
use macroquad::prelude::*;

/// Width of the stage in cells, and a cell's size in stage pixels.
pub const STAGE_CELLS: i32 = 26;
pub const CELL: f32 = 16.0;
/// Height of the stage: the background band (5 cells of sky, 8 of ground).
pub const STAGE_H: f32 = 13.0 * CELL;
/// A fighter's frame.
pub const FRAME: f32 = 96.0;
/// Where the fighters start (cells from the stage's left) and stand (cells from its top).
pub const START: [i32; 2] = [7, 13];
pub const ROW: f32 = 3.0;
/// Seconds per animation step.
pub const STEP: f32 = 0.07;
/// How close a charge brings a fighter to the other, in cells.
pub const CLOSE: i32 = 4;

/// One step of a fighter's move: where they are and which frame shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pose {
    x: i32,
    frame: u8,
}

/// A duel fighter.
#[derive(Debug, Clone)]
struct Fighter {
    officer: String,
    x: i32,
    frame: u8,
    /// Facing away from the other (fleeing).
    turned: bool,
    gone: bool,
}

/// The duel scene.
#[derive(Debug, Clone)]
pub struct DuelView {
    fighters: [Fighter; 2],
    bg: Option<String>,
    /// Steps still to show: the fighter, their pose and whether they face away.
    queue: Vec<(usize, Pose, bool)>,
    /// Time since the last step was shown.
    timer: f32,
}

fn index(side: DuelSide) -> usize {
    match side {
        DuelSide::Left => 0,
        DuelSide::Right => 1,
    }
}

impl DuelView {
    pub fn new(left: &str, right: &str, bg: Option<String>) -> DuelView {
        let fighter = |officer: &str, x: i32| Fighter {
            officer: officer.to_string(),
            x,
            frame: 0,
            turned: false,
            gone: false,
        };
        DuelView {
            fighters: [fighter(left, START[0]), fighter(right, START[1])],
            bg,
            queue: Vec::new(),
            timer: 0.0,
        }
    }

    /// Queue a move of `side`; returns the sound it starts with.
    pub fn act(&mut self, side: DuelSide, act: DuelAct) -> Option<&'static str> {
        let i = index(side);
        let me = self.fighters[i].x;
        let other = self.fighters[1 - i].x;
        // The left fighter moves right towards the other, the right one left.
        let toward = if i == 0 { 1 } else { -1 };
        let mut steps: Vec<(Pose, bool)> = Vec::new();
        let sound = match act {
            DuelAct::Charge => {
                let target = other - toward * CLOSE;
                let mut x = me;
                let mut n = 0u8;
                while (target - x) * toward > 0 {
                    x += toward;
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        false,
                    ));
                    n += 1;
                }
                steps.push((Pose { x, frame: 0 }, false));
                Some(sfx::STEP)
            }
            DuelAct::Strike(frame) => {
                steps.push((Pose { x: me, frame }, false));
                steps.push((
                    Pose {
                        x: me,
                        frame: frame + 1,
                    },
                    false,
                ));
                steps.push((
                    Pose {
                        x: me,
                        frame: frame + 1,
                    },
                    false,
                ));
                steps.push((Pose { x: me, frame: 0 }, false));
                Some(sfx::HIT_HEAVY)
            }
            DuelAct::Fall => {
                steps.push((Pose { x: me, frame: 12 }, false));
                steps.push((Pose { x: me, frame: 12 }, false));
                steps.push((Pose { x: me, frame: 13 }, false));
                Some(sfx::RETREAT)
            }
            DuelAct::Flee => {
                let mut x = me;
                for n in 0..14u8 {
                    x -= toward;
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        true,
                    ));
                }
                Some(sfx::STEP)
            }
            DuelAct::Back => {
                let start = START[i];
                // Riding back to the start faces away from the other fighter.
                let away = (start - me).signum() == -toward;
                let mut x = me;
                let mut n = 0u8;
                while x != start {
                    x += (start - x).signum();
                    steps.push((
                        Pose {
                            x,
                            frame: 1 + n % 3,
                        },
                        away,
                    ));
                    n += 1;
                }
                steps.push((Pose { x, frame: 0 }, false));
                Some(sfx::STEP)
            }
        };
        self.queue
            .extend(steps.into_iter().map(|(pose, turned)| (i, pose, turned)));
        sound
    }

    /// Whether a move is still being shown.
    pub fn busy(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Advance by `dt` seconds (`fast`: the whole queue at once).
    pub fn update(&mut self, dt: f32, fast: bool) {
        if fast {
            for (i, pose, turned) in std::mem::take(&mut self.queue) {
                self.apply(i, pose, turned);
            }
            return;
        }
        self.timer += dt;
        while self.timer >= STEP && !self.queue.is_empty() {
            self.timer -= STEP;
            let (i, pose, turned) = self.queue.remove(0);
            self.apply(i, pose, turned);
        }
        if self.queue.is_empty() {
            self.timer = 0.0;
        }
    }

    fn apply(&mut self, i: usize, pose: Pose, turned: bool) {
        let f = &mut self.fighters[i];
        f.x = pose.x;
        f.frame = pose.frame;
        f.turned = turned;
        f.gone = !(-6..STAGE_CELLS).contains(&pose.x);
    }

    /// Textures the scene needs (for preloading).
    pub fn textures(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .fighters
            .iter()
            .map(|f| format!("duel/{}", f.officer))
            .collect();
        keys.extend(["duel/left".to_string(), "duel/right".to_string()]);
        keys.extend(self.bg.iter().map(|b| format!("duel/{b}")));
        keys
    }

    /// Draw the scene centred in `area`, as large as fits (whole-number scales from 1 up).
    pub fn draw(&self, ctx: &Ctx, area: Rect) {
        let stage = stage_rect(area);
        let scale = stage.w / STAGE_W;
        match self
            .bg
            .as_ref()
            .and_then(|b| ctx.media.texture(&format!("duel/{b}")))
        {
            Some(tex) => draw_texture_ex(
                &tex,
                stage.x,
                stage.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(stage.size()),
                    ..Default::default()
                },
            ),
            None => fill_rect(stage, Color::from_hex(0x2a3a22)),
        }
        for (i, f) in self.fighters.iter().enumerate() {
            if f.gone {
                continue;
            }
            let Some(sheet) = sheet(ctx, &f.officer, i) else {
                continue;
            };
            let size = FRAME * scale;
            let x = stage.x + f.x as f32 * CELL * scale - size / 2.0 + CELL * scale;
            let y = stage.y + ROW * CELL * scale;
            // The sheets face right: the right fighter is mirrored, and so is one turning away.
            let mirrored = (i == 1) != f.turned;
            draw_texture_ex(
                &sheet,
                x.round(),
                y.round(),
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(size, size)),
                    source: Some(Rect::new(f32::from(f.frame) * FRAME, 0.0, FRAME, FRAME)),
                    flip_x: mirrored,
                    ..Default::default()
                },
            );
        }
    }
}

/// Width of the stage in stage pixels.
const STAGE_W: f32 = STAGE_CELLS as f32 * CELL;

/// Where the stage is drawn in `area`: centred, as large as fits, at a whole-number scale when
/// it fits at 1 or more (pixel art stays sharp).
fn stage_rect(area: Rect) -> Rect {
    let fit = (area.w / STAGE_W).min(area.h / STAGE_H);
    let scale = if fit >= 1.0 { fit.floor() } else { fit };
    let (w, h) = (STAGE_W * scale, STAGE_H * scale);
    Rect::new(
        (area.x + (area.w - w) / 2.0).round(),
        (area.y + (area.h - h) / 2.0).round(),
        w,
        h,
    )
}

/// The sheet of a fighter: the officer's own, else the side's.
fn sheet(ctx: &Ctx, officer: &str, side: usize) -> Option<Texture2D> {
    let own = format!("duel/{officer}");
    if ctx.media.texture_state(&own) == AssetState::Ready {
        return ctx.media.texture(&own);
    }
    ctx.media
        .texture(if side == 0 { "duel/left" } else { "duel/right" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs(v: &DuelView) -> [i32; 2] {
        [v.fighters[0].x, v.fighters[1].x]
    }

    #[test]
    fn a_charge_stops_close_to_the_other_fighter() {
        let mut v = DuelView::new("guan_yu", "hua_xiong", None);
        assert_eq!(v.act(DuelSide::Left, DuelAct::Charge), Some(sfx::STEP));
        assert!(v.busy());
        v.update(0.0, true);
        assert_eq!(xs(&v), [START[1] - CLOSE, START[1]]);
        assert_eq!(v.fighters[0].frame, 0);
        // The right one charging closes in from the other side.
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Right, DuelAct::Charge);
        v.update(0.0, true);
        assert_eq!(xs(&v), [START[0], START[0] + CLOSE]);
    }

    #[test]
    fn moves_play_step_by_step_and_end_standing_or_lying() {
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Left, DuelAct::Strike(6));
        v.update(STEP, false);
        assert_eq!(v.fighters[0].frame, 6);
        v.update(STEP, false);
        assert_eq!(v.fighters[0].frame, 7);
        v.update(STEP * 5.0, false);
        assert!(!v.busy());
        assert_eq!(v.fighters[0].frame, 0);
        v.act(DuelSide::Right, DuelAct::Fall);
        v.update(1.0, false);
        assert_eq!(v.fighters[1].frame, 13);
    }

    #[test]
    fn the_stage_keeps_its_shape_and_sharp_scales() {
        // The original's map hole shows it at 1:1.
        assert_eq!(
            stage_rect(Rect::new(16.0, 32.0, 416.0, 352.0)),
            Rect::new(16.0, 104.0, 416.0, 208.0)
        );
        let big = stage_rect(Rect::new(0.0, 0.0, 1280.0, 600.0));
        assert_eq!((big.w, big.h), (832.0, 416.0));
        let small = stage_rect(Rect::new(0.0, 0.0, 208.0, 400.0));
        assert_eq!((small.w, small.h), (208.0, 104.0));
    }

    #[test]
    fn a_fleeing_fighter_leaves_the_stage_and_back_returns() {
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Right, DuelAct::Flee);
        v.update(0.0, true);
        assert!(v.fighters[1].gone);
        assert!(v.fighters[1].turned);
        let mut v = DuelView::new("a", "b", None);
        v.act(DuelSide::Left, DuelAct::Charge);
        v.act(DuelSide::Left, DuelAct::Back);
        v.update(0.0, true);
        assert_eq!(xs(&v), START);
    }
}
