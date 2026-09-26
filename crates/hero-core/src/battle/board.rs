//! Per-call lookup tables over the map: resolved terrain and active-unit occupancy per tile.
//!
//! Building a `Board` is O(tiles + units); queries are then O(1), which keeps path finding
//! and AI scoring from repeatedly searching the terrain list or the unit list.

use super::{BattleState, UnitId};
use crate::data::TerrainDef;
use crate::geom::Pos;
use crate::pack::Pack;

pub(super) struct Board<'a> {
    width: i32,
    height: i32,
    terrain: Vec<Option<&'a TerrainDef>>,
    occupant: Vec<Option<UnitId>>,
}

impl<'a> Board<'a> {
    pub fn new(state: &BattleState, pack: &'a Pack) -> Board<'a> {
        Board::with_moved(state, pack, None)
    }

    /// Like [`Board::new`] but without the units for which `absent` holds (they neither block
    /// nor exert zone of control), e.g. to ask what others could do once they have moved away.
    pub fn without(
        state: &BattleState,
        pack: &'a Pack,
        absent: impl Fn(UnitId) -> bool,
    ) -> Board<'a> {
        let mut board = Board::new(state, pack);
        for o in board.occupant.iter_mut() {
            if o.is_some_and(&absent) {
                *o = None;
            }
        }
        board
    }

    /// Like [`Board::new`] but pretending unit `moved.0` stands on `moved.1`.
    pub fn with_moved(
        state: &BattleState,
        pack: &'a Pack,
        moved: Option<(UnitId, Pos)>,
    ) -> Board<'a> {
        let map = &state.map;
        let palette: Vec<Option<&'a TerrainDef>> =
            map.terrain_ids.iter().map(|id| pack.terrain(id)).collect();
        let terrain = map
            .tiles
            .iter()
            .map(|&t| palette.get(t as usize).copied().flatten())
            .collect();
        let mut board = Board {
            width: map.width.max(0),
            height: map.height.max(0),
            terrain,
            occupant: vec![None; (map.width.max(0) * map.height.max(0)) as usize],
        };
        for u in state.units.iter().filter(|u| u.is_active()) {
            let pos = match moved {
                Some((id, p)) if id == u.id => p,
                _ => u.pos,
            };
            if let Some(i) = board.index(pos) {
                board.occupant[i] = Some(u.id);
            }
        }
        board
    }

    pub fn len(&self) -> usize {
        self.occupant.len()
    }

    pub fn index(&self, p: Pos) -> Option<usize> {
        (p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height)
            .then(|| (p.y * self.width + p.x) as usize)
    }

    pub fn pos_of(&self, i: usize) -> Pos {
        let i = i as i32;
        Pos::new(i % self.width, i / self.width)
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        self.index(p).is_some()
    }

    pub fn terrain(&self, p: Pos) -> Option<&'a TerrainDef> {
        self.index(p).and_then(|i| self.terrain[i])
    }

    pub fn terrain_at_index(&self, i: usize) -> Option<&'a TerrainDef> {
        self.terrain[i]
    }

    pub fn unit_at(&self, p: Pos) -> Option<UnitId> {
        self.index(p).and_then(|i| self.occupant[i])
    }

    /// Every unit on the board with its tile, in position order.
    pub fn occupants(&self) -> impl Iterator<Item = (Pos, UnitId)> + '_ {
        self.occupant
            .iter()
            .enumerate()
            .filter_map(|(i, o)| o.map(|u| (self.pos_of(i), u)))
    }
}
