//! AI planning (RULES.md §12).
//!
//! A unit scores every (reachable tile × action) candidate: the value of the action (damage,
//! kills, healing, morale and confusion effects) plus a position term (terrain defence,
//! regeneration, minus half the damage hostile units could deal on that tile next phase).
//! When no action is possible it walks towards its goal along the cheapest path. All
//! iteration is in unit-id / position order and ties are broken by the affected unit's id,
//! then staying on the current tile, then the tile's position order, so a plan is
//! deterministic for a given state.
//!
//! Scores are expressed in "HP-equivalents": one point is one HP of damage dealt or healed.

use super::board::Board;
use super::combat::{counter_chance, hit_damage, morale_loss};
use super::{Action, BattleEvent, BattleState, Unit, UnitId};
use crate::battledef::{AiMode, Side};
use crate::data::{Area, Effect, ItemDef, StatusKind, StrategyDef, TargetSide, TerrainDef};
use crate::geom::Pos;
use crate::pack::Pack;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// `guard` units stay within this manhattan distance of their `ai_pos`.
const GUARD_RADIUS: i32 = 3;

impl BattleState {
    /// First unit of `side` (in unit order) that can still act and is not confused.
    pub(super) fn next_actor(&self, side: Side) -> Option<UnitId> {
        if self.outcome.is_some() || self.phase != side {
            return None;
        }
        self.units
            .iter()
            .find(|u| u.is_active() && u.side == side && !u.acted && !u.has_status(StatusKind::Confused))
            .map(|u| u.id)
    }

    pub(super) fn plan_ai(&self, pack: &Pack, id: UnitId) -> Vec<Action> {
        if id >= self.units.len() || !self.can_act(id) || self.units[id].has_status(StatusKind::Confused) {
            return Vec::new();
        }
        Planner::new(self, pack, id).plan()
    }

    pub(super) fn run_ai(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        let mut ev = Vec::new();
        let (side, turn) = (self.phase, self.turn);
        let same_phase = |s: &BattleState| s.outcome.is_none() && s.phase == side && s.turn == turn;
        while same_phase(self) {
            let Some(id) = self.next_actor(side) else { break };
            self.run_ai_unit(pack, id, &mut ev);
        }
        if same_phase(self) {
            self.end_phase(pack, &mut ev);
        }
        ev
    }

    /// Play one unit: apply the planned move, re-plan from the new tile (events fired by the
    /// move may have changed the situation), apply the action.
    fn run_ai_unit(&mut self, pack: &Pack, id: UnitId, ev: &mut Vec<BattleEvent>) {
        for _ in 0..2 {
            let Some(first) = self.plan_ai(pack, id).into_iter().next() else { break };
            let is_move = matches!(first, Action::Move { .. });
            match self.apply(pack, first) {
                Ok(events) => ev.extend(events),
                Err(err) => {
                    debug_assert!(false, "AI planned an invalid action for unit {id}: {err}");
                    break;
                }
            }
            if !is_move || !self.can_act(id) {
                break;
            }
        }
        if self.can_act(id) {
            // Planning failed or produced nothing: the unit waits so the phase always advances.
            match self.apply(pack, Action::Wait { unit: id }) {
                Ok(events) => ev.extend(events),
                Err(err) => {
                    debug_assert!(false, "AI unit {id} cannot wait: {err}");
                    self.units[id].acted = true;
                }
            }
        }
    }
}

/// Counter-attack data of a potential target (the attacker's tile decides whether it applies).
#[derive(Clone)]
struct CounterInfo {
    /// The target's attack offsets (the attacker must stand on one of them).
    offsets: Vec<Pos>,
    chance: i32,
    /// The target's ATK after losing morale to our hit.
    atk: i32,
    /// Our DEF multiplier against the target.
    affinity: i32,
}

/// Facts about physically attacking one unit that do not depend on the attacker's tile.
#[derive(Clone)]
struct AttackInfo {
    value: i64,
    counter: Option<CounterInfo>,
}

/// Value of aiming a strategy at a tile.
#[derive(Clone, Copy)]
struct AimValue {
    value: i64,
    /// Lowest affected unit id (tie-break key).
    key: UnitId,
    /// A hostile unit is affected.
    hostile: bool,
    /// The unit this AI is focused on (`ai = "target"`) is affected.
    hits_focus: bool,
}

#[derive(Clone)]
struct Choice {
    score: i64,
    /// Unit the action is aimed at (tie-break: lower id first).
    key: UnitId,
    tile: Pos,
    action: Action,
}

/// Value of dealing `damage` to `target` (§12): the damage, ×3 when it defeats the target,
/// +50% against the lord or a commander.
fn damage_value(damage: i32, target: &Unit) -> i64 {
    let mut value = damage as i64;
    if damage >= target.hp {
        value *= 3;
    }
    if target.lord || target.commander {
        value = value * 3 / 2;
    }
    value
}

/// Keep `c` when it beats `best`: higher score, then lower affected unit id, then acting from
/// `origin` (no needless move), then the lower tile in position order.
fn offer(best: &mut Option<Choice>, c: Choice, origin: Pos) {
    let rank = |x: &Choice| (x.score, Reverse(x.key), x.tile == origin, Reverse(x.tile));
    let better = match best {
        None => true,
        Some(b) => rank(&c) > rank(b),
    };
    if better {
        *best = Some(c);
    }
}

struct Planner<'a> {
    st: &'a BattleState,
    pack: &'a Pack,
    id: UnitId,
    me: &'a Unit,
    board: Board<'a>,
    /// Tiles the unit may end its move on, in position order.
    reach: Vec<Pos>,
    /// Raw damage (before the tile's terrain defence) hostile units could deal to this unit
    /// on each tile during their next phase.
    threat: Vec<i64>,
    atk: i32,
    def: i32,
    attack_offsets: Vec<Pos>,
    strategies: Vec<&'a StrategyDef>,
    /// Deduplicated reach offsets per strategy.
    strategy_offsets: Vec<Vec<Pos>>,
    /// `ai = "target"`: the unit to go for.
    focus: Option<UnitId>,
    attack_cache: HashMap<UnitId, AttackInfo>,
    /// Enemy-targeted single/cross strategies do not depend on the caster's tile.
    aim_cache: HashMap<(usize, Pos), Option<AimValue>>,
}

impl<'a> Planner<'a> {
    fn new(st: &'a BattleState, pack: &'a Pack, id: UnitId) -> Planner<'a> {
        let me = &st.units[id];
        let board = Board::new(st, pack);
        let reach: Vec<Pos> = if me.moved {
            vec![me.pos]
        } else {
            st.movement_range(pack, id).tiles.keys().copied().collect()
        };
        let mut attack_offsets: Vec<Pos> = Vec::new();
        for o in st.class_of(pack, id).range.offsets().unwrap_or_default() {
            if o != Pos::new(0, 0) && !attack_offsets.contains(&o) {
                attack_offsets.push(o);
            }
        }
        let strategies: Vec<&StrategyDef> = st
            .usable_strategies(pack, id)
            .iter()
            .filter_map(|s| pack.strategy(s))
            .collect();
        let strategy_offsets = strategies
            .iter()
            .map(|s| {
                let mut v: Vec<Pos> = Vec::new();
                for o in s.range.offsets().unwrap_or_default() {
                    if !v.contains(&o) {
                        v.push(o);
                    }
                }
                v
            })
            .collect();
        let focus = match me.ai {
            AiMode::Target => me
                .ai_target
                .as_deref()
                .and_then(|r| st.units.iter().find(|u| u.is_active() && u.matches(r)))
                .map(|u| u.id),
            _ => None,
        };
        let mut planner = Planner {
            st,
            pack,
            id,
            me,
            board,
            reach,
            threat: Vec::new(),
            atk: st.attack_power(pack, id),
            def: st.defense_power(pack, id),
            attack_offsets,
            strategies,
            strategy_offsets,
            focus,
            attack_cache: HashMap::new(),
            aim_cache: HashMap::new(),
        };
        planner.threat = planner.compute_threat();
        planner
    }

    fn plan(mut self) -> Vec<Action> {
        let origin = self.me.pos;
        let reach = std::mem::take(&mut self.reach);
        let (tile, action) = match self.me.ai {
            AiMode::Hold => self.act_at(origin),
            AiMode::Aggressive => self.aggressive(&reach),
            AiMode::Defensive => self.defensive(&reach),
            AiMode::Guard => {
                let center = self.me.ai_pos.unwrap_or(origin);
                if origin.manhattan(center) <= GUARD_RADIUS {
                    let zone: Vec<Pos> = reach
                        .iter()
                        .copied()
                        .filter(|p| p.manhattan(center) <= GUARD_RADIUS)
                        .collect();
                    match self.best_from(&zone, None).0 {
                        Some(c) => (c.tile, Some(c.action)),
                        None => (origin, None),
                    }
                } else {
                    self.approach_and_act(&[center], &reach)
                }
            }
            AiMode::Target => match self.focus {
                Some(target) => match self.best_from(&reach, Some(target)).0 {
                    Some(c) => (c.tile, Some(c.action)),
                    None => self.approach_and_act(&[self.st.units[target].pos], &reach),
                },
                None => self.aggressive(&reach),
            },
            AiMode::Advance => match self.me.ai_pos {
                None => self.aggressive(&reach),
                Some(p) if p == origin => self.defensive(&reach),
                Some(p) => self.approach_and_act(&[p], &reach),
            },
            AiMode::Flee => {
                let tile = self.flee_tile(&reach);
                self.act_at(tile)
            }
        };
        let mut out = Vec::with_capacity(2);
        if tile != origin {
            out.push(Action::Move { unit: self.id, to: tile });
        }
        out.push(action.unwrap_or(Action::Wait { unit: self.id }));
        out
    }

    // ----- modes ---------------------------------------------------------------------------

    fn aggressive(&mut self, reach: &[Pos]) -> (Pos, Option<Action>) {
        if let Some(c) = self.best_from(reach, None).0 {
            return (c.tile, Some(c.action));
        }
        let goals = self.hostile_positions();
        if goals.is_empty() {
            return (self.me.pos, None);
        }
        (self.approach(&goals, reach), None)
    }

    /// Fight only when a hostile unit can be reached this phase; otherwise stay (supporting
    /// friends from the current tile).
    fn defensive(&mut self, reach: &[Pos]) -> (Pos, Option<Action>) {
        let (best, hostile_in_reach) = self.best_from(reach, None);
        match best {
            Some(c) if hostile_in_reach => (c.tile, Some(c.action)),
            _ => self.act_at(self.me.pos),
        }
    }

    fn act_at(&mut self, tile: Pos) -> (Pos, Option<Action>) {
        (tile, self.best_from(&[tile], None).0.map(|c| c.action))
    }

    fn approach_and_act(&mut self, goals: &[Pos], reach: &[Pos]) -> (Pos, Option<Action>) {
        let tile = self.approach(goals, reach);
        self.act_at(tile)
    }

    // ----- candidates ----------------------------------------------------------------------

    /// Best action from any of `tiles` (only actions involving `focus` when given), and
    /// whether any hostile unit can be attacked or hit by a strategy from them.
    fn best_from(&mut self, tiles: &[Pos], focus: Option<UnitId>) -> (Option<Choice>, bool) {
        let mut best: Option<Choice> = None;
        let mut hostile_in_reach = false;
        for &tile in tiles {
            let position = self.position_value(tile);
            for oi in 0..self.attack_offsets.len() {
                let o = self.attack_offsets[oi];
                let Some(t) = self.occupant(tile.offset(o.x, o.y), tile) else { continue };
                if !self.is_hostile(t) {
                    continue;
                }
                hostile_in_reach = true;
                if focus.is_some_and(|f| f != t) {
                    continue;
                }
                let score = self.attack_value(tile, t) + position;
                offer(
                    &mut best,
                    Choice {
                        score,
                        key: t,
                        tile,
                        action: Action::Attack { unit: self.id, target: t },
                    },
                    self.me.pos,
                );
            }
            for si in 0..self.strategies.len() {
                let s = self.strategies[si];
                let aims: Vec<Pos> = match s.area {
                    Area::AllInRange => vec![tile],
                    Area::Single | Area::Cross => self.strategy_offsets[si]
                        .iter()
                        .map(|o| tile.offset(o.x, o.y))
                        .collect(),
                };
                for aim in aims {
                    let Some(av) = self.aim_value(si, tile, aim) else { continue };
                    hostile_in_reach |= av.hostile;
                    if av.value <= 0 || (focus.is_some() && !av.hits_focus) {
                        continue;
                    }
                    offer(
                        &mut best,
                        Choice {
                            score: av.value - s.mp.max(0) as i64 + position,
                            key: av.key,
                            tile,
                            action: Action::Strategy {
                                unit: self.id,
                                strategy: s.id.clone(),
                                target: aim,
                            },
                        },
                        self.me.pos,
                    );
                }
            }
            if focus.is_none() {
                self.item_choices(tile, position, &mut best);
            }
        }
        (best, hostile_in_reach)
    }

    fn attack_value(&mut self, tile: Pos, target: UnitId) -> i64 {
        let info = self.attack_info(target);
        let mut value = info.value;
        if let Some(c) = &info.counter {
            let t_pos = self.st.units[target].pos;
            let delta = Pos::new(tile.x - t_pos.x, tile.y - t_pos.y);
            if tile.chebyshev(t_pos) == 1 && c.offsets.contains(&delta) {
                let terrain = self.board.terrain(tile).map_or(0, |t| t.defense);
                let dmg = hit_damage(c.atk, self.def, c.affinity, terrain) as i64;
                let dmg = (dmg * self.pack.rules.counter_damage_pct as i64 / 100).max(1);
                value -= dmg.min(self.me.hp as i64) * c.chance as i64 / 100;
            }
        }
        value
    }

    fn attack_info(&mut self, target: UnitId) -> AttackInfo {
        if let Some(info) = self.attack_cache.get(&target) {
            return info.clone();
        }
        let (st, pack) = (self.st, self.pack);
        let t = &st.units[target];
        let terrain = self.board.terrain(t.pos).map_or(0, |tt| tt.defense);
        let dmg = hit_damage(self.atk, st.defense_power(pack, target), st.affinity(pack, self.id, target), terrain);
        let kill = dmg >= t.hp;
        let value = damage_value(dmg, t);
        let mut counter = None;
        let t_class = st.class_of(pack, target);
        if !kill && t_class.can_counter && st.class_of(pack, self.id).provokes_counter {
            let morale = t.morale - morale_loss(&pack.rules, dmg, t.max_hp).min(t.morale);
            if !(morale == 0 && t.has_status(StatusKind::Confused)) {
                counter = Some(CounterInfo {
                    offsets: t_class.range.offsets().unwrap_or_default(),
                    chance: counter_chance(&pack.rules, t.strength),
                    atk: st.attack_with_morale(pack, target, morale),
                    affinity: st.affinity(pack, target, self.id),
                });
            }
        }
        let info = AttackInfo { value, counter };
        self.attack_cache.insert(target, info.clone());
        info
    }

    fn aim_value(&mut self, si: usize, tile: Pos, aim: Pos) -> Option<AimValue> {
        let s = self.strategies[si];
        let cacheable = s.target == TargetSide::Enemy && s.area != Area::AllInRange;
        if cacheable {
            if let Some(v) = self.aim_cache.get(&(si, aim)) {
                return *v;
            }
        }
        let v = self.compute_aim(si, tile, aim);
        if cacheable {
            self.aim_cache.insert((si, aim), v);
        }
        v
    }

    fn compute_aim(&self, si: usize, tile: Pos, aim: Pos) -> Option<AimValue> {
        let s = self.strategies[si];
        if !self.board.in_bounds(aim) {
            return None;
        }
        let area: Vec<Pos> = match s.area {
            Area::Single => vec![aim],
            Area::Cross => std::iter::once(aim).chain(aim.neighbors4()).collect(),
            Area::AllInRange => self.strategy_offsets[si]
                .iter()
                .map(|o| tile.offset(o.x, o.y))
                .collect(),
        };
        if s.area != Area::AllInRange && !self.st.element_allows(s, self.board.terrain(aim)) {
            return None;
        }
        let mut out: Option<AimValue> = None;
        for p in area {
            let Some(u) = self.occupant(p, tile) else { continue };
            let valid = match s.target {
                TargetSide::Enemy => self.is_hostile(u),
                TargetSide::Ally => !self.is_hostile(u),
            };
            let terrain = self.board.terrain(p);
            if !valid || !self.st.element_allows(s, terrain) {
                continue;
            }
            let value = self.unit_value(s, u, terrain);
            let hostile = self.is_hostile(u);
            let hits_focus = self.focus == Some(u);
            out = Some(match out {
                None => AimValue {
                    value,
                    key: u,
                    hostile,
                    hits_focus,
                },
                Some(a) => AimValue {
                    value: a.value + value,
                    key: a.key.min(u),
                    hostile: a.hostile || hostile,
                    hits_focus: a.hits_focus || hits_focus,
                },
            });
        }
        out
    }

    /// Expected value of `s`'s effects on unit `u` standing on `terrain`.
    fn unit_value(&self, s: &StrategyDef, u: UnitId, terrain: Option<&TerrainDef>) -> i64 {
        let (st, pack) = (self.st, self.pack);
        let t = &st.units[u];
        let sign: i64 = if self.is_hostile(u) { 1 } else { -1 };
        let chance = match s.target {
            TargetSide::Enemy => st.hit_chance(pack, self.id, u),
            TargetSide::Ally => 100,
        } as i64;
        let level_factor = t.level as i64 + 10;
        let (mut hp, mut morale, mut confused) = (t.hp, t.morale, t.has_status(StatusKind::Confused));
        let mut v: i64 = 0;
        for e in &s.effects {
            if hp <= 0 {
                break;
            }
            match e {
                Effect::Damage { power } => {
                    let dmg = st.strategy_damage_base(pack, self.id, s, *power, u, terrain);
                    let mut value = dmg as i64;
                    if dmg >= hp {
                        value *= 3;
                    }
                    if t.lord || t.commander {
                        value = value * 3 / 2;
                    }
                    v += sign * value;
                    hp -= dmg.min(hp);
                    morale -= morale_loss(&pack.rules, dmg, t.max_hp).min(morale);
                }
                Effect::Heal { power } => {
                    let heal = st.strategy_heal(self.id, *power, u).min(t.max_hp - hp).max(0);
                    v -= sign * heal as i64;
                    hp += heal;
                }
                Effect::Morale { amount } => {
                    let new = (morale + st.morale_shift(self.id, u, *amount)).clamp(0, 100);
                    // Morale enters ATK/DEF as `(level + 10) * morale / 10`.
                    let change = (new - morale) as i64 * level_factor / 10;
                    v += if sign > 0 { -change } else { change / 2 };
                    morale = new;
                }
                Effect::Status { .. } => {
                    if !confused {
                        confused = true;
                        // A confused unit skips its phase: roughly the damage it would deal.
                        v += sign * st.attack_power(pack, u) as i64 / 2;
                    }
                }
                Effect::Promote | Effect::ChangeClass { .. } => {}
            }
        }
        if sign > 0 && confused && morale == 0 && hp > 0 {
            v += hp as i64; // routed (§6)
        }
        v * chance / 100
    }

    /// Healing / morale consumables (player side only: the inventory is the player's).
    fn item_choices(&self, tile: Pos, position: i64, best: &mut Option<Choice>) {
        if self.me.side != Side::Player {
            return;
        }
        for (item, &count) in &self.st.inventory {
            let Some(def) = self.pack.item(item) else { continue };
            let usable = count > 0
                && def.battle_use
                && def.strategy.is_none()
                && !def.effects.is_empty()
                && def
                    .effects
                    .iter()
                    .all(|e| matches!(e, Effect::Heal { .. } | Effect::Morale { .. }));
            if !usable {
                continue;
            }
            let targets = std::iter::once(tile)
                .chain(tile.neighbors4())
                .filter_map(|p| self.occupant(p, tile))
                .filter(|&u| !self.is_hostile(u));
            for u in targets {
                let value = self.item_value(def, u);
                if value > 0 {
                    offer(
                        best,
                        Choice {
                            score: value + position,
                            key: u,
                            tile,
                            action: Action::UseItem {
                                unit: self.id,
                                item: item.clone(),
                                target: u,
                            },
                        },
                        self.me.pos,
                    );
                }
            }
        }
    }

    /// Items are finite, so they are only worth half their effect and only on units in need
    /// (HP below half, morale in the confusion zone).
    fn item_value(&self, def: &ItemDef, u: UnitId) -> i64 {
        let t = &self.st.units[u];
        let mut v: i64 = 0;
        for e in &def.effects {
            match e {
                Effect::Heal { power } if t.hp * 2 < t.max_hp => {
                    v += (*power).min(t.max_hp - t.hp).max(0) as i64;
                }
                Effect::Morale { amount } if t.morale <= self.pack.rules.confuse_morale => {
                    let gain = (*amount).min(100 - t.morale).max(0) as i64;
                    v += gain * (t.level as i64 + 10) / 10;
                }
                _ => {}
            }
        }
        v / 2
    }

    // ----- positions -----------------------------------------------------------------------

    /// Unit on `p` if this unit stood on `tile` (its real tile is then empty).
    fn occupant(&self, p: Pos, tile: Pos) -> Option<UnitId> {
        if p == tile {
            Some(self.id)
        } else {
            self.board.unit_at(p).filter(|&u| u != self.id)
        }
    }

    fn is_hostile(&self, u: UnitId) -> bool {
        self.st.units[u].side.is_hostile(self.me.side)
    }

    fn hostile_positions(&self) -> Vec<Pos> {
        self.st
            .units
            .iter()
            .filter(|u| u.is_active() && u.side.is_hostile(self.me.side))
            .map(|u| u.pos)
            .collect()
    }

    fn compute_threat(&self) -> Vec<i64> {
        let (st, pack) = (self.st, self.pack);
        let n = self.board.len();
        let mut threat = vec![0i64; n];
        let mut stamp = vec![usize::MAX; n];
        for h in st.units.iter().filter(|h| h.is_active() && self.is_hostile(h.id)) {
            // Still confused during its next phase: no threat.
            if h.statuses.iter().any(|s| s.status == StatusKind::Confused && s.turns >= 2) {
                continue;
            }
            let Some(offsets) = st.class_of(pack, h.id).range.offsets() else { continue };
            let range = st.reach(pack, &self.board, h.id, h.pos, st.base_move_points(pack, h.id));
            let def = self.def as i64 * st.affinity(pack, h.id, self.id) as i64 / 100;
            let raw = (st.attack_power(pack, h.id) as i64 - def / 2).max(1);
            for e in range.tiles.keys() {
                for o in &offsets {
                    if let Some(i) = self.board.index(e.offset(o.x, o.y)) {
                        if stamp[i] != h.id {
                            stamp[i] = h.id;
                            threat[i] += raw;
                        }
                    }
                }
            }
        }
        threat
    }

    /// Expected damage taken on `tile` next phase, at most the unit's HP.
    fn threat_at(&self, tile: Pos) -> i64 {
        let Some(i) = self.board.index(tile) else { return 0 };
        let defense = self.board.terrain(tile).map_or(0, |t| t.defense) as i64;
        (self.threat[i] * (100 - defense) / 100).clamp(0, self.me.hp as i64)
    }

    /// Terrain defence, expected regeneration if hurt, minus half the expected damage taken.
    fn position_value(&self, tile: Pos) -> i64 {
        let terrain = self.board.terrain(tile);
        let mut v = terrain.map_or(0, |t| t.defense) as i64;
        let missing = (self.me.max_hp - self.me.hp) as i64;
        if let (true, Some(t)) = (missing > 0, terrain) {
            v += (self.me.max_hp as i64 * t.heal_hp.max(0) as i64 / 100).min(missing) / 2;
        }
        v - self.threat_at(tile) / 2
    }

    /// Tile of `allowed` closest to any goal along the cheapest path for this unit's move
    /// type (ignoring units); manhattan distance when no goal is reachable at all. Ties prefer
    /// less threat, staying put, then position order.
    fn approach(&self, goals: &[Pos], allowed: &[Pos]) -> Pos {
        let origin = self.me.pos;
        let dist = self.goal_distance(goals);
        let path_dist = |p: Pos| self.board.index(p).map_or(i32::MAX, |i| dist[i]);
        let reachable = allowed.iter().any(|&p| path_dist(p) < i32::MAX);
        allowed
            .iter()
            .copied()
            .min_by_key(|&p| {
                let d = if reachable {
                    path_dist(p) as i64
                } else {
                    goals.iter().map(|g| g.manhattan(p)).min().unwrap_or(0) as i64
                };
                (d, self.threat_at(p), p != origin, p)
            })
            .unwrap_or(origin)
    }

    /// Reverse Dijkstra from the goals: cost of walking from each tile to the nearest goal.
    /// Entering a goal tile is free (the unit only needs to get next to it).
    fn goal_distance(&self, goals: &[Pos]) -> Vec<i32> {
        let n = self.board.len();
        let move_type = &self.st.class_of(self.pack, self.id).move_type;
        let cost: Vec<Option<i32>> = (0..n)
            .map(|i| {
                self.board
                    .terrain_at_index(i)
                    .and_then(|t| t.move_cost(move_type))
                    .map(i32::from)
            })
            .collect();
        let mut dist = vec![i32::MAX; n];
        let mut is_goal = vec![false; n];
        let mut heap = BinaryHeap::new();
        for g in goals {
            if let Some(i) = self.board.index(*g) {
                is_goal[i] = true;
                dist[i] = 0;
                heap.push(Reverse((0i32, i)));
            }
        }
        while let Some(Reverse((d, i))) = heap.pop() {
            if d > dist[i] {
                continue;
            }
            let enter = if is_goal[i] { 0 } else { cost[i].unwrap_or(0) };
            for nb in self.board.pos_of(i).neighbors4() {
                let Some(j) = self.board.index(nb) else { continue };
                if cost[j].is_none() {
                    continue;
                }
                let nd = d.saturating_add(enter);
                if nd < dist[j] {
                    dist[j] = nd;
                    heap.push(Reverse((nd, j)));
                }
            }
        }
        dist
    }

    /// Tile maximising the distance to the nearest hostile unit.
    fn flee_tile(&self, reach: &[Pos]) -> Pos {
        let origin = self.me.pos;
        let hostiles = self.hostile_positions();
        if hostiles.is_empty() {
            return origin;
        }
        reach
            .iter()
            .copied()
            .max_by_key(|&p| {
                let nearest = hostiles.iter().map(|h| h.manhattan(p)).min().unwrap_or(0);
                (nearest, Reverse(self.threat_at(p)), p == origin, Reverse(p))
            })
            .unwrap_or(origin)
    }
}
