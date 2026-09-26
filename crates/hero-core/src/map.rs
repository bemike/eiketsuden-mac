//! Battle map grid parsed from ASCII rows.

use crate::data::{Id, TerrainDef};
use crate::geom::Pos;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapError {
    #[error("map has no rows")]
    Empty,
    #[error("row {row} has width {got}, expected {expected}")]
    Ragged {
        row: usize,
        got: usize,
        expected: usize,
    },
    #[error("unknown map glyph {glyph:?} at ({x}, {y})")]
    UnknownGlyph { glyph: char, x: usize, y: usize },
    #[error("legend maps {glyph:?} to unknown terrain `{terrain}`")]
    UnknownLegendTerrain { glyph: char, terrain: String },
}

/// A rectangular grid of terrain. Tiles store indices into `terrain_ids`, which is
/// kept inside the map so that a saved battle stays self-describing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleMap {
    pub width: i32,
    pub height: i32,
    /// Terrain id per palette index.
    pub terrain_ids: Vec<Id>,
    /// Row-major palette indices, `width * height` entries.
    pub tiles: Vec<u16>,
}

impl BattleMap {
    /// Parse `rows` (one line per map row, blank lines and surrounding whitespace ignored).
    /// Each character is resolved through `legend` first, then through the terrain glyphs.
    pub fn parse(
        rows: &str,
        legend: &BTreeMap<String, Id>,
        terrain: &[TerrainDef],
    ) -> Result<BattleMap, MapError> {
        let lines: Vec<&str> = rows
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            return Err(MapError::Empty);
        }
        let width = lines[0].chars().count();
        let mut terrain_ids: Vec<Id> = Vec::new();
        let mut tiles = Vec::with_capacity(width * lines.len());
        for (y, line) in lines.iter().enumerate() {
            let got = line.chars().count();
            if got != width {
                return Err(MapError::Ragged {
                    row: y,
                    got,
                    expected: width,
                });
            }
            for (x, glyph) in line.chars().enumerate() {
                let id: &str = match legend.get(&glyph.to_string()) {
                    Some(t) => {
                        if !terrain.iter().any(|d| &d.id == t) {
                            return Err(MapError::UnknownLegendTerrain {
                                glyph,
                                terrain: t.clone(),
                            });
                        }
                        t
                    }
                    None => match terrain.iter().find(|d| d.glyph == glyph) {
                        Some(d) => &d.id,
                        None => return Err(MapError::UnknownGlyph { glyph, x, y }),
                    },
                };
                let idx = match terrain_ids.iter().position(|t| t == id) {
                    Some(i) => i,
                    None => {
                        terrain_ids.push(id.to_string());
                        terrain_ids.len() - 1
                    }
                };
                tiles.push(idx as u16);
            }
        }
        Ok(BattleMap {
            width: width as i32,
            height: lines.len() as i32,
            terrain_ids,
            tiles,
        })
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height
    }

    /// Terrain id at `p`, or `None` when out of bounds.
    pub fn terrain_at(&self, p: Pos) -> Option<&str> {
        if !self.in_bounds(p) {
            return None;
        }
        let idx = self.tiles[(p.y * self.width + p.x) as usize] as usize;
        self.terrain_ids.get(idx).map(|s| s.as_str())
    }

    /// All positions, row-major.
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| Pos::new(x, y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, glyph: char) -> TerrainDef {
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

    #[test]
    fn parse_and_lookup() {
        let terrain = vec![t("plain", '.'), t("forest", 'T'), t("river", '~')];
        let mut legend = BTreeMap::new();
        legend.insert("F".to_string(), "forest".to_string());
        let m = BattleMap::parse("\n  ..T\n  ~F.\n", &legend, &terrain).unwrap();
        assert_eq!((m.width, m.height), (3, 2));
        assert_eq!(m.terrain_at(Pos::new(2, 0)), Some("forest"));
        assert_eq!(m.terrain_at(Pos::new(1, 1)), Some("forest"));
        assert_eq!(m.terrain_at(Pos::new(0, 1)), Some("river"));
        assert_eq!(m.terrain_at(Pos::new(3, 0)), None);
        assert_eq!(m.positions().count(), 6);
    }

    #[test]
    fn errors() {
        let terrain = vec![t("plain", '.')];
        let legend = BTreeMap::new();
        assert_eq!(
            BattleMap::parse("", &legend, &terrain),
            Err(MapError::Empty)
        );
        assert!(matches!(
            BattleMap::parse("..\n.", &legend, &terrain),
            Err(MapError::Ragged { .. })
        ));
        assert!(matches!(
            BattleMap::parse(".x", &legend, &terrain),
            Err(MapError::UnknownGlyph { glyph: 'x', .. })
        ));
    }
}
