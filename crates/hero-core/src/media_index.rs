//! Schema of the battle media index files of a pack (`docs/ASSETS.md`): unit sprites
//! (`gfx/units/units.toml`), the terrain tileset (`gfx/tiles/terrain.toml`) and strategy effects
//! (`gfx/fx/fx.toml`).
//!
//! The game draws with these types, `Pack::missing_media` checks packs with them and
//! `Pack::unknown_fields` lints them, so a file the validator accepts is one the game can read.
//! Unknown keys are ignored when reading (the lint reports them), like the rules files.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Pack-relative path of the unit sprite index.
pub const UNITS_FILE: &str = "gfx/units/units.toml";
/// Pack-relative path of the terrain tileset.
pub const TILESET_FILE: &str = "gfx/tiles/terrain.toml";
/// Pack-relative path of the effect index.
pub const FX_FILE: &str = "gfx/fx/fx.toml";
/// Tile size in virtual pixels when `terrain.toml` does not set `tile_size`, and of the flat
/// colour map drawn without a tileset.
pub const DEFAULT_TILE: u32 = 16;

/// Parse a media index. A leading byte-order mark is dropped as for the rules files (the TOML
/// parser accepts it too).
pub fn parse<T: DeserializeOwned>(src: &str) -> Result<T, String> {
    toml::from_str(src.strip_prefix('\u{feff}').unwrap_or(src))
        .map_err(|e| e.to_string().trim_end().to_string())
}

// ----- units.toml --------------------------------------------------------------------------------

/// Frame size and anchor of one sprite key.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpriteDef {
    pub frame: [u32; 2],
    pub anchor: [i32; 2],
}

impl Default for SpriteDef {
    /// The documented default: a 16×16 frame whose bottom-centre pixel stands on the tile's
    /// bottom-centre pixel.
    fn default() -> SpriteDef {
        SpriteDef {
            frame: [16, 16],
            anchor: [8, 15],
        }
    }
}

/// Key of an [`UnitsFile::officers`] entry that applies whatever the officer's class.
pub const ANY_CLASS: &str = "*";

/// `units.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UnitsFile {
    #[serde(default)]
    pub sprites: BTreeMap<String, SpriteDef>,
    /// Sprites of particular officers in battle: officer id -> the sprite key of the class the
    /// officer is (or [`ANY_CLASS`]) -> the sprite key to draw instead (the original draws Liu
    /// Bei, Lü Bu and Cao Cao with their own icons).
    #[serde(default)]
    pub officers: BTreeMap<String, BTreeMap<String, String>>,
}

impl UnitsFile {
    /// The sprite key a unit of `officer` (if any) whose class draws with `class_sprite` is
    /// drawn with: the officer's own for that class, else for any class, else the class's.
    pub fn sprite_for<'a>(&'a self, officer: Option<&str>, class_sprite: &'a str) -> &'a str {
        officer
            .and_then(|o| self.officers.get(o))
            .and_then(|m| m.get(class_sprite).or_else(|| m.get(ANY_CLASS)))
            .map_or(class_sprite, String::as_str)
    }
}

/// Parse `units.toml`.
pub fn parse_units(src: &str) -> Result<UnitsFile, String> {
    parse::<UnitsFile>(src)
}

// ----- fx.toml -----------------------------------------------------------------------------------

/// One effect strip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

/// `fx.toml`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FxFile {
    #[serde(default)]
    pub fx: BTreeMap<String, FxDef>,
}

/// Parse `fx.toml`.
pub fn parse_fx(src: &str) -> Result<BTreeMap<String, FxDef>, String> {
    parse::<FxFile>(src).map(|f| f.fx)
}

// ----- terrain.toml ------------------------------------------------------------------------------

fn default_tile_size() -> u32 {
    DEFAULT_TILE
}

fn default_image() -> String {
    "terrain.png".into()
}

/// `terrain.toml` as written by the asset pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TilesetFile {
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default)]
    pub tiles: BTreeMap<String, TileFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TileFile {
    #[serde(default)]
    pub layers: Vec<LayerFile>,
}

/// One layer as written; [`LayerFile::validate`] checks it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerFile {
    #[serde(default)]
    pub cells: Vec<[u32; 2]>,
    #[serde(default)]
    pub auto: Vec<[u32; 2]>,
    #[serde(default)]
    pub connect: Vec<String>,
    #[serde(default)]
    pub offset: [i32; 2],
    #[serde(default)]
    pub fps: f32,
    #[serde(default)]
    pub frames: Vec<Vec<[u32; 2]>>,
}

/// One validated layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Cell lists per animation frame (one entry for static layers). For autotile layers every
    /// list has exactly 16 cells.
    pub frames: Vec<Vec<[u32; 2]>>,
    /// Autotile layer: the cell is picked by the neighbour mask instead of the position hash.
    pub auto: bool,
    pub connect: Vec<String>,
    /// Shift of the drawn cell in pixels.
    pub offset: [i32; 2],
    /// Animation speed (frames per second); only used when there is more than one frame.
    pub fps: f32,
}

impl LayerFile {
    /// The layer, or why it cannot be drawn.
    pub fn validate(self) -> Result<Layer, String> {
        let auto = !self.auto.is_empty() || (!self.connect.is_empty() && !self.frames.is_empty());
        let frames = if !self.frames.is_empty() {
            self.frames
        } else if !self.auto.is_empty() {
            vec![self.auto]
        } else {
            vec![self.cells]
        };
        if frames.iter().any(|f| f.is_empty()) {
            return Err("a layer needs `cells`, `auto` or `frames`".into());
        }
        if auto && frames.iter().any(|f| f.len() != 16) {
            return Err("autotile layers need exactly 16 cells per frame".into());
        }
        Ok(Layer {
            frames,
            auto,
            connect: self.connect,
            offset: self.offset,
            fps: self.fps.max(0.0),
        })
    }
}

impl TilesetFile {
    /// Parse `terrain.toml`.
    pub fn parse(src: &str) -> Result<TilesetFile, String> {
        parse(src)
    }

    /// Media key of the atlas (`tiles/terrain` for `gfx/tiles/terrain.png`).
    pub fn texture_key(&self) -> String {
        format!("tiles/{}", self.image.trim_end_matches(".png"))
    }

    /// The validated layers of every tile key, and one message per layer that cannot be drawn
    /// (`tile `key` layer i: why`); those layers are left out.
    pub fn layers(&self) -> (BTreeMap<String, Vec<Layer>>, Vec<String>) {
        let mut problems = Vec::new();
        let mut tiles = BTreeMap::new();
        for (key, tile) in &self.tiles {
            let mut layers = Vec::new();
            for (i, l) in tile.layers.iter().cloned().enumerate() {
                match l.validate() {
                    Ok(layer) => layers.push(layer),
                    Err(e) => problems.push(format!("tile `{key}` layer {i}: {e}")),
                }
            }
            tiles.insert(key.clone(), layers);
        }
        (tiles, problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_need_frame_and_anchor() {
        let ok = parse_units("[sprites.infantry]\nframe = [16, 16]\nanchor = [8, 15]").unwrap();
        assert_eq!(ok.sprites["infantry"], SpriteDef::default());
        let e = parse_units("[sprites.infantry]\nframe = [16, 16]").unwrap_err();
        assert!(e.contains("anchor"), "{e}");
        assert!(
            parse_units("\u{feff}").unwrap().sprites.is_empty(),
            "BOM, no sprites"
        );
    }

    #[test]
    fn officers_can_have_their_own_sprites() {
        let units = parse_units(
            "[officers.liu_bei]\ninfantry = \"liu_bei_flag\"\n\n[officers.lu_bu]\n\"*\" = \"red_hare\"\n",
        )
        .unwrap();
        assert_eq!(
            units.sprite_for(Some("liu_bei"), "infantry"),
            "liu_bei_flag"
        );
        // Another class of his has no own sprite.
        assert_eq!(units.sprite_for(Some("liu_bei"), "cavalry"), "cavalry");
        assert_eq!(units.sprite_for(Some("lu_bu"), "cavalry"), "red_hare");
        assert_eq!(units.sprite_for(Some("cao_cao"), "cavalry"), "cavalry");
        assert_eq!(units.sprite_for(None, "cavalry"), "cavalry");
    }

    #[test]
    fn effects_play_their_frames() {
        let fx = parse_fx("[fx.fire]\nframe = [32, 32]\nframes = 4\nfps = 8").unwrap();
        let fire = fx["fire"];
        assert_eq!(fire.duration(), 0.5);
        assert_eq!(fire.frame_at(0.3), Some(2));
        assert_eq!(fire.frame_at(0.5), None);
        assert!(parse_fx("[fx.fire]\nframe = [32, 32]\nfps = 8").is_err());
    }

    #[test]
    fn tileset_defaults_and_layers() {
        let t = TilesetFile::parse(
            "[tiles.plain]\nlayers = [{ cells = [[0, 0]] }, { auto = [[0, 0]] }, {}]\n",
        )
        .unwrap();
        assert_eq!(t.tile_size, DEFAULT_TILE);
        assert_eq!(t.texture_key(), "tiles/terrain");
        let (tiles, problems) = t.layers();
        assert_eq!(tiles["plain"].len(), 1);
        assert_eq!(
            problems,
            [
                "tile `plain` layer 1: autotile layers need exactly 16 cells per frame",
                "tile `plain` layer 2: a layer needs `cells`, `auto` or `frames`"
            ]
        );
        assert!(TilesetFile::parse("tile_size = -1").is_err());
    }
}
