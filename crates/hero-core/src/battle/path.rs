//! Movement ranges (RULES.md §3).

use super::board::Board;
use super::{BattleState, MoveRange, MoveStep, UnitId};
use crate::battledef::Side;
use crate::geom::Pos;
use crate::pack::Pack;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

impl BattleState {
    /// Dijkstra search for unit `id` standing on `from` with `points` movement points.
    ///
    /// * Tile cost is `terrain.cost[class.move_type]`; a missing entry is impassable.
    /// * Hostile units block; friendly units can be crossed but not stopped on (`through`).
    /// * Entering a zone-of-control tile (orthogonal neighbour of an active hostile unit) or,
    ///   for player units, an untaken treasure tile ends the move there. The start tile never
    ///   restricts leaving it.
    /// * Equal-cost ties keep the path found first; neighbours are expanded up, down, left,
    ///   right and the queue is ordered by (cost, insertion order), so the result is
    ///   deterministic.
    pub(super) fn reach(&self, pack: &Pack, board: &Board, id: UnitId, from: Pos, points: i32) -> MoveRange {
        let mut range = MoveRange {
            origin: from,
            tiles: BTreeMap::new(),
            through: BTreeMap::new(),
        };
        let Some(start) = board.index(from) else {
            return range;
        };
        let unit = &self.units[id];
        let move_type = &self.class_of(pack, id).move_type;
        let n = board.len();

        let mut stop = vec![false; n];
        for other in self.units.iter().filter(|o| o.is_active() && o.side.is_hostile(unit.side)) {
            for p in other.pos.neighbors4() {
                if let Some(i) = board.index(p) {
                    stop[i] = true;
                }
            }
        }
        if unit.side == Side::Player {
            for (i, t) in self.def(pack).treasures.iter().enumerate() {
                let taken = self.treasures_taken.get(i).copied().unwrap_or(false);
                if let (false, Some(ti)) = (taken, board.index(t.pos)) {
                    stop[ti] = true;
                }
            }
        }

        let mut cost = vec![i32::MAX; n];
        let mut prev: Vec<Option<usize>> = vec![None; n];
        let mut visited: Vec<usize> = Vec::new();
        let mut heap = BinaryHeap::new();
        let mut seq: u32 = 0;
        cost[start] = 0;
        heap.push(Reverse((0i32, seq, start)));
        while let Some(Reverse((c, _, i))) = heap.pop() {
            if c > cost[i] {
                continue;
            }
            visited.push(i);
            if i != start && stop[i] {
                continue;
            }
            for nb in board.pos_of(i).neighbors4() {
                let Some(j) = board.index(nb) else { continue };
                let blocked = board
                    .unit_at(nb)
                    .is_some_and(|o| o != id && self.units[o].side.is_hostile(unit.side));
                if blocked {
                    continue;
                }
                let Some(step) = board.terrain_at_index(j).and_then(|t| t.move_cost(move_type)) else {
                    continue;
                };
                let nc = c + step as i32;
                if nc > points || nc >= cost[j] {
                    continue;
                }
                cost[j] = nc;
                prev[j] = Some(i);
                seq += 1;
                heap.push(Reverse((nc, seq, j)));
            }
        }

        for i in visited {
            let pos = board.pos_of(i);
            let step = MoveStep {
                cost: cost[i],
                prev: prev[i].map(|j| board.pos_of(j)),
            };
            if board.unit_at(pos).is_some_and(|o| o != id) {
                range.through.insert(pos, step);
            } else {
                range.tiles.insert(pos, step);
            }
        }
        range
    }
}
