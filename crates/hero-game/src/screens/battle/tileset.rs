//! Terrain tileset (`gfx/tiles/terrain.toml` + its atlas, format in `docs/ASSETS.md`) and the
//! battle map renderer.
//!
//! Every map tile is a stack of layers. A layer picks one atlas cell per map position:
//!
//! * `cells` — variants chosen by a stable hash of the position (maps look varied, never flicker);
//! * `auto` — exactly 16 cells indexed by the 4-bit mask of orthogonal neighbours whose terrain id
//!   is in `connect` (1 = north, 2 = east, 4 = south, 8 = west; out-of-map counts as connected);
//! * `frames` + `fps` — an animated layer: `frames` is a list of cell lists, each one used like
//!   `auto` (when the layer has `connect`) or like `cells` (otherwise), cycling at `fps`;
//! * `offset = [dx, dy]` shifts the drawn cell (objects overhanging their tile).
//!
//! [`MapRenderer`] draws every static layer once into a render target per battle; only animated
//! layers are drawn per frame. Without a tileset (missing `terrain.toml` or atlas) the map is
//! drawn as flat colours per terrain so the battle stays playable. A map with a picture layer
//! (`image`, `gfx/maps/<key>.png`) is drawn from that picture instead of the tileset
//! ([`MapRenderer::use_picture`]); the tileset still sets the tile size.
//!
//! The tileset's `tile_size` is the size of a map tile on screen, in virtual pixels: one atlas
//! pixel is one virtual pixel, like every other piece of pixel art. The battle screen uses it for
//! the map, the camera, the cursor, highlights, unit placement, effects and hit-testing.

use crate::gfx::{fill_rect, key_color};
use hero_core::geom::Pos;
use hero_core::map::BattleMap;
use hero_core::pack::Pack;
use macroquad::prelude::*;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Pack-relative path of the tileset description.
pub const TILESET_FILE: &str = "gfx/tiles/terrain.toml";
/// Tile size in virtual pixels when `terrain.toml` does not set `tile_size`, and of the flat
/// colour map drawn without a tileset.
pub const DEFAULT_TILE: u32 = 16;

fn default_tile_size() -> u32 {
    DEFAULT_TILE
}

fn default_image() -> String {
    "terrain.png".into()
}

/// `terrain.toml` as written by the asset pipeline.
#[derive(Debug, Clone, Deserialize)]
pub struct TilesetFile {
    #[serde(default = "default_tile_size")]
    pub tile_size: u32,
    #[serde(default = "default_image")]
    pub image: String,
    #[serde(default)]
    pub tiles: BTreeMap<String, TileFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TileFile {
    #[serde(default)]
    pub layers: Vec<LayerFile>,
}

#[derive(Debug, Clone, Deserialize)]
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
    pub offset: Vec2,
    /// Animation speed (frames per second); only used when there is more than one frame.
    pub fps: f32,
}

impl Layer {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1 && self.fps > 0.0
    }

    /// Animation frame shown at `time` seconds.
    pub fn frame_at(&self, time: f64) -> usize {
        if !self.is_animated() {
            return 0;
        }
        ((time * f64::from(self.fps)).floor() as i64).rem_euclid(self.frames.len() as i64) as usize
    }
}

/// A parsed tileset: the atlas texture key and the layers of every tile key.
#[derive(Debug, Clone, PartialEq)]
pub struct Tileset {
    /// Media key of the atlas (`tiles/terrain` for `gfx/tiles/terrain.png`).
    pub texture: String,
    pub tile_size: f32,
    pub tiles: BTreeMap<String, Vec<Layer>>,
}

impl Tileset {
    /// Parse `terrain.toml`. Malformed layers are skipped with a warning in `warnings`.
    pub fn parse(src: &str) -> Result<(Tileset, Vec<String>), String> {
        let file: TilesetFile = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut warnings = Vec::new();
        let mut tiles = BTreeMap::new();
        for (key, tile) in file.tiles {
            let mut layers = Vec::new();
            for (i, l) in tile.layers.into_iter().enumerate() {
                match validate_layer(l) {
                    Ok(layer) => layers.push(layer),
                    Err(e) => warnings.push(format!("tile `{key}` layer {i}: {e}")),
                }
            }
            tiles.insert(key, layers);
        }
        let image = file.image.trim_end_matches(".png");
        Ok((
            Tileset {
                texture: format!("tiles/{image}"),
                tile_size: file.tile_size.max(1) as f32,
                tiles,
            },
            warnings,
        ))
    }
}

fn validate_layer(l: LayerFile) -> Result<Layer, String> {
    let auto = !l.auto.is_empty() || (!l.connect.is_empty() && !l.frames.is_empty());
    let frames = if !l.frames.is_empty() {
        l.frames
    } else if !l.auto.is_empty() {
        vec![l.auto]
    } else {
        vec![l.cells]
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
        connect: l.connect,
        offset: vec2(l.offset[0] as f32, l.offset[1] as f32),
        fps: l.fps.max(0.0),
    })
}

/// 4-bit mask of the orthogonal neighbours of `p` whose terrain id is in `connect`
/// (1 = north, 2 = east, 4 = south, 8 = west). Neighbours outside the map count as connected.
pub fn autotile_mask(map: &BattleMap, p: Pos, connect: &[String]) -> usize {
    let joins = |q: Pos| match map.terrain_at(q) {
        None => true,
        Some(id) => connect.iter().any(|c| c == id),
    };
    let mut mask = 0;
    for (bit, (dx, dy)) in [(1, (0, -1)), (2, (1, 0)), (4, (0, 1)), (8, (-1, 0))] {
        if joins(p.offset(dx, dy)) {
            mask |= bit;
        }
    }
    mask
}

/// Stable per-position hash (independent of platform and run) used to pick variants.
pub fn position_hash(x: i32, y: i32, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Atlas cell of `layer` (frame `frame`) at map position `p`.
pub fn layer_cell(
    map: &BattleMap,
    layer: &Layer,
    layer_index: usize,
    p: Pos,
    frame: usize,
) -> [u32; 2] {
    let cells = &layer.frames[frame.min(layer.frames.len() - 1)];
    if layer.auto {
        cells[autotile_mask(map, p, &layer.connect)]
    } else {
        let h = position_hash(p.x, p.y, layer_index as u32 * 0x68E3_1DA4);
        cells[h as usize % cells.len()]
    }
}

/// Flat colour of a terrain for the fallback renderer and the minimap-like highlights.
pub fn terrain_color(id: &str) -> Color {
    let hex = match id {
        "plain" | "road" => 0x9dbb4a,
        "grass" => 0x6f9a3a,
        "forest" => 0x2f6b34,
        "mountain" => 0x7a6a5a,
        "wasteland" => 0xb49a64,
        "bridge" => 0x8a6238,
        "castle" | "gate" | "granary" | "treasury" => 0xc9b48a,
        "village" | "barracks" | "fort" => 0xb0784a,
        "river" => 0x4c9bd8,
        "wall" | "cliff" => 0x5b5f66,
        "house" | "fence" => 0x80583a,
        other => return key_color(other),
    };
    Color::from_hex(hex)
}

/// An animated layer instance on one map tile, drawn every frame above the cached map.
#[derive(Debug, Clone)]
struct AnimatedTile {
    pos: Pos,
    /// Tile key and the index of its first animated layer; that layer and every layer above
    /// it are drawn per frame (keeping the stacking order of the tile).
    key: String,
    first_layer: usize,
}

/// What the static part of a map is drawn from.
enum StaticMap {
    /// The tileset's static layers (or flat colours), drawn once into a render target.
    Cache(RenderTarget),
    /// The map's picture layer, drawn as it is.
    Picture(Texture2D),
}

/// Static-map cache and animated layers of one battle map.
pub struct MapRenderer {
    base: Option<StaticMap>,
    animated: Vec<AnimatedTile>,
    /// Size of a tile in virtual pixels.
    pub tile: f32,
    /// Size of the map in virtual pixels.
    pub size: Vec2,
}

/// Padding (in tiles) around the cached map so overhanging objects on the edge are not cut.
const PAD: f32 = 1.0;

impl MapRenderer {
    /// A renderer for `map` with `tile` pixel tiles (the tileset's `tile_size`, or
    /// [`DEFAULT_TILE`] without a tileset). Nothing is drawn until [`MapRenderer::build`].
    pub fn new(map: &BattleMap, tile: f32) -> MapRenderer {
        MapRenderer {
            base: None,
            animated: Vec::new(),
            tile,
            size: vec2(map.width as f32, map.height as f32) * tile,
        }
    }

    pub fn is_built(&self) -> bool {
        self.base.is_some()
    }

    /// Whether a picture of `size` pixels covers the map exactly (one tile = `tile` pixels, as
    /// for atlas cells). Any other size would drift away from the rules grid.
    pub fn fits(&self, size: Vec2) -> bool {
        size == self.size
    }

    /// Draw the map from its picture layer instead of the tileset. The caller checks
    /// [`MapRenderer::fits`] first.
    pub fn use_picture(&mut self, picture: Texture2D) {
        self.animated.clear();
        self.base = Some(StaticMap::Picture(picture));
    }

    /// Draw the static layers into the cache render target. `atlas` is `None` when the tileset
    /// or its texture is unavailable (flat colours are used then). The tiles are drawn at the
    /// renderer's tile size, so a renderer for a tileset is made with its `tile_size`. Must be
    /// called outside the canvas camera (during `update`); it restores the default camera.
    pub fn build(
        &mut self,
        map: &BattleMap,
        pack: &Pack,
        tileset: Option<&Tileset>,
        atlas: Option<&Texture2D>,
    ) {
        let tile = self.tile;
        let (w, h) = (
            ((map.width as f32 + 2.0 * PAD) * tile) as u32,
            ((map.height as f32 + 2.0 * PAD) * tile) as u32,
        );
        let target = render_target(w.max(1), h.max(1));
        target.texture.set_filter(FilterMode::Nearest);
        let mut camera = Camera2D::from_display_rect(Rect::new(0.0, 0.0, w as f32, h as f32));
        camera.render_target = Some(target.clone());
        set_camera(&camera);
        clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
        self.animated.clear();
        let origin = vec2(PAD * tile, PAD * tile);
        for p in map.positions() {
            let terrain_id = map.terrain_at(p).unwrap_or("");
            let key = pack
                .terrain(terrain_id)
                .map(|t| t.tile_key().to_string())
                .unwrap_or_else(|| terrain_id.to_string());
            let at = origin + vec2(p.x as f32, p.y as f32) * tile;
            let layers = match (tileset, atlas) {
                (Some(ts), Some(_)) => ts.tiles.get(&key),
                _ => None,
            };
            let (Some(layers), Some(ts), Some(atlas)) = (layers, tileset, atlas) else {
                fill_rect(Rect::new(at.x, at.y, tile, tile), terrain_color(terrain_id));
                continue;
            };
            for (i, layer) in layers.iter().enumerate() {
                if layer.is_animated() {
                    self.animated.push(AnimatedTile {
                        pos: p,
                        key: key.clone(),
                        first_layer: i,
                    });
                    break;
                }
                let cell = layer_cell(map, layer, i, p, 0);
                draw_cell(atlas, ts.tile_size, tile, cell, at + layer.offset);
            }
        }
        set_default_camera();
        self.base = Some(StaticMap::Cache(target));
    }

    /// Draw the map with its top-left corner at `origin` (virtual pixels).
    pub fn draw(
        &self,
        origin: Vec2,
        map: &BattleMap,
        tileset: Option<&Tileset>,
        atlas: Option<&Texture2D>,
        time: f64,
    ) {
        match &self.base {
            None => return,
            Some(StaticMap::Picture(picture)) => {
                draw_texture_ex(
                    picture,
                    origin.x.round(),
                    origin.y.round(),
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(self.size),
                        ..Default::default()
                    },
                );
                // A picture has no animated layers.
                return;
            }
            Some(StaticMap::Cache(cache)) => {
                let tex = &cache.texture;
                let pad = vec2(PAD, PAD) * self.tile;
                draw_texture_ex(
                    tex,
                    (origin.x - pad.x).round(),
                    (origin.y - pad.y).round(),
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(tex.width(), tex.height())),
                        flip_y: true,
                        ..Default::default()
                    },
                );
            }
        }
        let (Some(ts), Some(atlas)) = (tileset, atlas) else {
            return;
        };
        for a in &self.animated {
            let Some(layers) = ts.tiles.get(&a.key) else {
                continue;
            };
            let at = origin + vec2(a.pos.x as f32, a.pos.y as f32) * self.tile;
            for (i, layer) in layers.iter().enumerate().skip(a.first_layer) {
                let cell = layer_cell(map, layer, i, a.pos, layer.frame_at(time));
                draw_cell(atlas, ts.tile_size, self.tile, cell, at + layer.offset);
            }
        }
    }
}

/// Draw atlas cell `cell` (`size` pixel cells) as a `tile` pixel tile at `at`.
fn draw_cell(atlas: &Texture2D, size: f32, tile: f32, cell: [u32; 2], at: Vec2) {
    draw_texture_ex(
        atlas,
        at.x.round(),
        at.y.round(),
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(tile, tile)),
            source: Some(Rect::new(
                cell[0] as f32 * size,
                cell[1] as f32 * size,
                size,
                size,
            )),
            ..Default::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::data::TerrainDef;

    fn terrain(id: &str, glyph: char) -> TerrainDef {
        TerrainDef {
            id: id.into(),
            name: id.into(),
            glyph,
            defense: 0,
            heal_hp: 0,
            heal_morale: 0,
            elements: vec![],
            boost: vec![],
            cost: BTreeMap::new(),
            tile: None,
        }
    }

    fn map(rows: &str) -> BattleMap {
        let t = vec![
            terrain("plain", '.'),
            terrain("river", '~'),
            terrain("bridge", '='),
        ];
        BattleMap::parse(rows, &BTreeMap::new(), &t).unwrap()
    }

    #[test]
    fn mask_counts_matching_neighbours_and_the_outside() {
        let m = map("...\n~~=\n...");
        let water = vec!["river".to_string(), "bridge".to_string()];
        // Middle river tile: west is river, east is the bridge -> 2 | 8.
        assert_eq!(autotile_mask(&m, Pos::new(1, 1), &water), 2 | 8);
        // West river tile: the west neighbour is outside the map (connected) -> 2 | 8.
        assert_eq!(autotile_mask(&m, Pos::new(0, 1), &water), 2 | 8);
        // Top-left plain tile: north and west are outside, south is river -> 1 | 4 | 8.
        assert_eq!(autotile_mask(&m, Pos::new(0, 0), &water), 1 | 4 | 8);
        // Nothing connects to an empty list except the outside.
        assert_eq!(autotile_mask(&m, Pos::new(1, 1), &[]), 0);
        assert_eq!(autotile_mask(&m, Pos::new(2, 2), &[]), 2 | 4);
    }

    #[test]
    fn variant_hash_is_stable_and_spread() {
        assert_eq!(position_hash(3, 7, 0), position_hash(3, 7, 0));
        assert_ne!(position_hash(3, 7, 0), position_hash(7, 3, 0));
        assert_ne!(position_hash(3, 7, 0), position_hash(3, 7, 1));
        // Four variants all appear on a small map.
        let mut seen = [false; 4];
        for y in 0..8 {
            for x in 0..8 {
                seen[position_hash(x, y, 0) as usize % 4] = true;
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn parses_the_documented_format() {
        let src = r#"
tile_size = 16
image = "terrain.png"

[tiles.grass]
layers = [ { cells = [[0, 0], [1, 0], [2, 0]] } ]

[tiles.forest]
layers = [ { cells = [[0, 0]] }, { cells = [[5, 3], [6, 3]], offset = [0, -4] } ]

[tiles.river]
layers = [
  { auto = [[0,8],[1,8],[2,8],[3,8],[4,8],[5,8],[6,8],[7,8],[8,8],[9,8],[10,8],[11,8],[12,8],[13,8],[14,8],[15,8]], connect = ["river", "bridge"] },
]

[tiles.sea]
layers = [
  { frames = [[[0, 9]], [[1, 9]]], fps = 4 },
  { cells = [] },
]

[tiles.broken]
layers = [ { auto = [[0, 0]], connect = ["x"] } ]
"#;
        let (ts, warnings) = Tileset::parse(src).unwrap();
        assert_eq!(ts.texture, "tiles/terrain");
        assert_eq!(ts.tile_size, 16.0);
        assert_eq!(ts.tiles["grass"][0].frames[0].len(), 3);
        assert!(!ts.tiles["grass"][0].auto);
        assert_eq!(ts.tiles["forest"][1].offset, vec2(0.0, -4.0));
        let river = &ts.tiles["river"][0];
        assert!(river.auto && river.frames[0].len() == 16);
        let sea = &ts.tiles["sea"][0];
        assert!(sea.is_animated());
        assert_eq!(sea.frame_at(0.1), 0);
        assert_eq!(sea.frame_at(0.3), 1);
        assert_eq!(sea.frame_at(0.5), 0);
        // The empty layer of `sea` and the short autotile of `broken` are rejected.
        assert_eq!(ts.tiles["sea"].len(), 1);
        assert!(ts.tiles["broken"].is_empty());
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn tile_size_comes_from_the_tileset() {
        let (ts, _) = Tileset::parse("tile_size = 32\nimage = \"big.png\"\n").unwrap();
        assert_eq!(ts.tile_size, 32.0);
        assert_eq!(ts.texture, "tiles/big");
        // Without `tile_size` the documented default applies.
        let (ts, _) = Tileset::parse("").unwrap();
        assert_eq!(ts.tile_size, DEFAULT_TILE as f32);
        let m = map("...\n...");
        assert_eq!(MapRenderer::new(&m, 32.0).size, vec2(96.0, 64.0));
        assert_eq!(MapRenderer::new(&m, 16.0).size, vec2(48.0, 32.0));
        // A picture layer must cover the map exactly at the tile size in use.
        assert!(MapRenderer::new(&m, 32.0).fits(vec2(96.0, 64.0)));
        assert!(!MapRenderer::new(&m, 16.0).fits(vec2(96.0, 64.0)));
        assert!(!MapRenderer::new(&m, 32.0).fits(vec2(96.0, 63.0)));
    }

    #[test]
    fn cells_follow_mask_and_hash() {
        let m = map("~~~\n...");
        let auto = Layer {
            frames: vec![(0..16).map(|i| [i, 0]).collect()],
            auto: true,
            connect: vec!["river".into()],
            offset: Vec2::ZERO,
            fps: 0.0,
        };
        // (1, 0): north outside, east/west river, south plain -> 1 | 2 | 8 = 11.
        assert_eq!(layer_cell(&m, &auto, 0, Pos::new(1, 0), 0), [11, 0]);
        let variants = Layer {
            frames: vec![vec![[0, 0], [1, 0]]],
            auto: false,
            connect: vec![],
            offset: Vec2::ZERO,
            fps: 0.0,
        };
        let c = layer_cell(&m, &variants, 0, Pos::new(2, 1), 0);
        assert_eq!(c, layer_cell(&m, &variants, 0, Pos::new(2, 1), 0));
        assert!(c == [0, 0] || c == [1, 0]);
    }

    #[test]
    fn the_base_pack_tileset_covers_every_terrain() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
        let src = std::fs::read_to_string(dir.join(TILESET_FILE)).unwrap();
        let (ts, warnings) = Tileset::parse(&src).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let pack = hero_core::pack::Pack::load(&hero_core::pack::DirSource { root: dir }).unwrap();
        for t in &pack.terrain {
            assert!(ts.tiles.contains_key(t.tile_key()), "no tile for {}", t.id);
        }
    }
}
