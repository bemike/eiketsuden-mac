//! Unit sprite sheets (`gfx/units/units.toml`) and battle effects (`gfx/fx/fx.toml`), format in
//! `docs/ASSETS.md`.
//!
//! Unit sheets have 4 columns (facing down, up, left, right) and 6 rows: 0–3 walk cycle (idle uses
//! the cycle slowly), 4 attack pose, 5 hurt pose. A frame's `anchor` pixel is placed on the tile's
//! bottom-centre pixel `(8, 15)`, so larger frames overhang the tile upwards and sideways.

use hero_core::battledef::Side;
use hero_core::geom::Dir;
use macroquad::prelude::*;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const UNITS_FILE: &str = "gfx/units/units.toml";
pub const FX_FILE: &str = "gfx/fx/fx.toml";

/// Frame size and anchor of one sprite key.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SpriteDef {
    pub frame: [u32; 2],
    pub anchor: [i32; 2],
}

impl Default for SpriteDef {
    /// The documented default: a 16×16 frame standing on the tile's bottom-centre pixel.
    fn default() -> SpriteDef {
        SpriteDef {
            frame: [16, 16],
            anchor: [8, 15],
        }
    }
}

#[derive(Debug, Deserialize)]
struct UnitsFile {
    #[serde(default)]
    sprites: BTreeMap<String, SpriteDef>,
}

/// Parse `units.toml`.
pub fn parse_units(src: &str) -> Result<BTreeMap<String, SpriteDef>, String> {
    let file: UnitsFile = toml::from_str(src).map_err(|e| e.to_string())?;
    Ok(file.sprites)
}

/// One effect strip.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct FxDef {
    pub frame: [u32; 2],
    pub frames: u32,
    pub fps: f32,
}

impl FxDef {
    /// Seconds the strip takes to play once.
    pub fn duration(&self) -> f32 {
        if self.fps > 0.0 {
            self.frames.max(1) as f32 / self.fps
        } else {
            0.0
        }
    }

    /// Frame shown `t` seconds after the start, `None` once the strip has finished.
    pub fn frame_at(&self, t: f32) -> Option<u32> {
        if t < 0.0 || self.fps <= 0.0 {
            return None;
        }
        let f = (t * self.fps) as u32;
        (f < self.frames).then_some(f)
    }
}

#[derive(Debug, Deserialize)]
struct FxFile {
    #[serde(default)]
    fx: BTreeMap<String, FxDef>,
}

/// Parse `fx.toml`.
pub fn parse_fx(src: &str) -> Result<BTreeMap<String, FxDef>, String> {
    let file: FxFile = toml::from_str(src).map_err(|e| e.to_string())?;
    Ok(file.fx)
}

/// Sheet colour suffix of a side (`<sprite>_<side>.png`).
pub fn side_suffix(side: Side) -> &'static str {
    match side {
        Side::Player => "player",
        Side::Ally => "ally",
        Side::Enemy => "enemy",
    }
}

/// Media key of a unit sheet.
pub fn sheet_key(sprite: &str, side: Side) -> String {
    format!("units/{sprite}_{}", side_suffix(side))
}

/// Sheet column of a facing.
pub fn facing_column(dir: Dir) -> u32 {
    match dir {
        Dir::Down => 0,
        Dir::Up => 1,
        Dir::Left => 2,
        Dir::Right => 3,
    }
}

/// What a unit is doing, which selects the sheet row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    /// Standing: the walk cycle played slowly (static for units that have acted).
    #[default]
    Idle,
    Walk,
    Attack,
    Hurt,
}

/// Sheet row for a pose at `time` (seconds). `animate` is false for units that already acted.
pub fn pose_row(pose: Pose, time: f64, animate: bool) -> u32 {
    match pose {
        Pose::Idle if animate => ((time * 3.0).floor() as i64).rem_euclid(4) as u32,
        Pose::Idle => 0,
        Pose::Walk => ((time * 8.0).floor() as i64).rem_euclid(4) as u32,
        Pose::Attack => 4,
        Pose::Hurt => 5,
    }
}

/// Top-left of a frame so its anchor lands on the tile's bottom-centre pixel.
pub fn frame_origin(tile_top_left: Vec2, def: &SpriteDef) -> Vec2 {
    tile_top_left + vec2(8.0 - def.anchor[0] as f32, 15.0 - def.anchor[1] as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units_and_fx() {
        let units = parse_units(
            "[sprites.archer]\nframe = [24, 24]\nanchor = [12, 23]\n[sprites.chariot]\nframe = [32, 24]\nanchor = [16, 23]\n",
        )
        .unwrap();
        assert_eq!(units["chariot"].frame, [32, 24]);
        let o = frame_origin(vec2(32.0, 48.0), &units["archer"]);
        // The anchor pixel (12, 23) of the frame sits on the tile pixel (8, 15).
        assert_eq!(o + vec2(12.0, 23.0), vec2(32.0 + 8.0, 48.0 + 15.0));
        assert_eq!(frame_origin(Vec2::ZERO, &SpriteDef::default()), Vec2::ZERO);

        let fx = parse_fx("[fx.fire]\nframe = [32, 32]\nframes = 8\nfps = 12\n").unwrap();
        let fire = fx["fire"];
        assert!((fire.duration() - 8.0 / 12.0).abs() < 1e-6);
        assert_eq!(fire.frame_at(0.0), Some(0));
        assert_eq!(fire.frame_at(0.1), Some(1));
        assert_eq!(fire.frame_at(0.7), None);
        assert_eq!(fire.frame_at(-0.1), None);
        assert!(parse_fx("[fx.bad]\nframe = 3\n").is_err());
    }

    #[test]
    fn sheet_layout() {
        assert_eq!(sheet_key("archer", Side::Enemy), "units/archer_enemy");
        assert_eq!(facing_column(Dir::Down), 0);
        assert_eq!(facing_column(Dir::Right), 3);
        assert_eq!(pose_row(Pose::Attack, 1.0, true), 4);
        assert_eq!(pose_row(Pose::Hurt, 1.0, false), 5);
        assert_eq!(pose_row(Pose::Idle, 5.0, false), 0);
        assert!(pose_row(Pose::Idle, 5.4, true) < 4);
    }

    #[test]
    fn base_pack_metadata_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
        let units = parse_units(&std::fs::read_to_string(dir.join(UNITS_FILE)).unwrap()).unwrap();
        assert_eq!(units.len(), 19);
        let fx = parse_fx(&std::fs::read_to_string(dir.join(FX_FILE)).unwrap()).unwrap();
        for key in ["slash", "arrow", "fire", "water", "rock", "heal", "levelup"] {
            assert!(fx.contains_key(key), "fx `{key}` missing");
        }
    }
}
