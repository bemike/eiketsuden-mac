//! Strategies (RULES.md §5) and battle items (§10).

use super::board::Board;
use super::{
    ActionError, ActiveStatus, BattleEvent, BattleState, StrategyForecast, StrategyHit, UnitId,
    Weather,
};
use crate::battledef::Side;
use crate::data::{
    Area, Effect, ItemDef, StatusKind, StrategyDef, StrategyFormulas, TargetSide, TerrainDef,
};
use crate::geom::{Dir, Pos};
use crate::pack::Pack;

/// Element blocked by rain.
const FIRE: &str = "fire";
/// Element boosted by rain.
const WATER: &str = "water";
/// HP heals are halved on targets whose morale is below this (§5, *design*).
const LOW_MORALE_HEAL: i32 = 30;
/// Original formulas: a morale-down leaving the target's morale below this confuses it with
/// [`MORALE_DOWN_CONFUSION`] percent (§6).
const MORALE_DOWN_CONFUSES_BELOW: i32 = 30;
const MORALE_DOWN_CONFUSION: i32 = 60;

/// Whether `pack` plays the original's strategy formulas.
pub(super) fn original_formulas(pack: &Pack) -> bool {
    pack.rules.strategy_formulas == StrategyFormulas::Original
}

/// Whether `s` confuses (its hit roll then weighs the target's power twice as much under the
/// original formulas).
fn confuses(s: &StrategyDef) -> bool {
    s.effects.iter().any(|e| {
        matches!(
            e,
            Effect::Status {
                status: StatusKind::Confused,
                ..
            }
        )
    })
}

/// The INT/level term of the strategy formulas: `int * level / div + int`.
pub(super) fn int_term(int: i32, level: u32, div: i64) -> i64 {
    let int = int.max(0) as i64;
    int * level as i64 / div + int
}

/// A battle consumable as it is used in battle.
pub(super) enum BattleItem<'a> {
    /// Heal / morale effects applied to the target.
    Direct(&'a ItemDef),
    /// Casts the strategy from the user's tile without MP.
    Scroll(&'a StrategyDef),
}

/// Reach tiles of a strategy around `from`, without duplicates, in offset order.
pub(super) fn reach_tiles(s: &StrategyDef, from: Pos) -> Vec<Pos> {
    let mut out: Vec<Pos> = Vec::new();
    for o in s.range.offsets().unwrap_or_default() {
        let p = from.offset(o.x, o.y);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

impl BattleState {
    /// Element gate (§5): the tile must list the element; rain makes `fire` impossible.
    pub(super) fn element_allows(&self, s: &StrategyDef, terrain: Option<&TerrainDef>) -> bool {
        match &s.element {
            None => true,
            Some(e) if self.weather == Weather::Rain && e == FIRE => false,
            Some(e) => terrain.is_some_and(|t| t.elements.contains(e)),
        }
    }

    /// Whether `target` is a valid unit for `s` cast by `caster`.
    pub(super) fn is_strategy_target(
        &self,
        caster: UnitId,
        s: &StrategyDef,
        target: UnitId,
    ) -> bool {
        let (c, t) = (&self.units[caster], &self.units[target]);
        t.is_active()
            && match s.target {
                TargetSide::Enemy => t.side.is_hostile(c.side),
                TargetSide::Ally => !t.side.is_hostile(c.side),
            }
    }

    /// Units affected by aiming `s` at `aim` with the caster on `from` (`board` must place the
    /// caster there). Errors explain why the aim is not allowed.
    pub(super) fn strategy_area(
        &self,
        board: &Board,
        caster: UnitId,
        s: &StrategyDef,
        from: Pos,
        aim: Pos,
    ) -> Result<Vec<UnitId>, ActionError> {
        let reach = reach_tiles(s, from);
        let mut out: Vec<UnitId> = Vec::new();
        let mut gated = false;
        let mut consider = |p: Pos, out: &mut Vec<UnitId>| {
            if let Some(u) = board
                .unit_at(p)
                .filter(|&u| self.is_strategy_target(caster, s, u))
            {
                if !self.element_allows(s, board.terrain(p)) {
                    gated = true;
                } else if !out.contains(&u) {
                    out.push(u);
                }
            }
        };
        match s.area {
            Area::AllInRange => {
                if aim != from {
                    return Err(ActionError::OutOfRange);
                }
                for p in reach {
                    consider(p, &mut out);
                }
            }
            Area::Single | Area::Cross => {
                if !reach.contains(&aim) || !board.in_bounds(aim) {
                    return Err(ActionError::OutOfRange);
                }
                if !self.element_allows(s, board.terrain(aim)) {
                    return Err(ActionError::WrongTerrain);
                }
                consider(aim, &mut out);
                if s.area == Area::Cross {
                    for p in aim.neighbors4() {
                        consider(p, &mut out);
                    }
                }
            }
        }
        if out.is_empty() {
            return Err(if gated {
                ActionError::WrongTerrain
            } else {
                ActionError::InvalidTarget
            });
        }
        Ok(out)
    }

    pub(super) fn strategy_aims(
        &self,
        pack: &Pack,
        id: UnitId,
        strategy: &str,
        from: Pos,
    ) -> Vec<Pos> {
        let Some(s) = pack.strategy(strategy) else {
            return Vec::new();
        };
        let board = Board::with_moved(self, pack, Some((id, from)));
        let aims = match s.area {
            Area::AllInRange => vec![from],
            Area::Single | Area::Cross => reach_tiles(s, from),
        };
        aims.into_iter()
            .filter(|&aim| self.strategy_area(&board, id, s, from, aim).is_ok())
            .collect()
    }

    /// Success chance (§5): `clamp(100 - 100 * power(target) / (4 * power(caster)), 0, 100)`
    /// with `power(u) = int * level / 100 + int` and the target's INT doubled for
    /// `strategy_guard` classes; `2` instead of `4` for a confusion under the original formulas.
    pub(super) fn hit_chance(
        &self,
        pack: &Pack,
        caster: UnitId,
        s: &StrategyDef,
        target: UnitId,
    ) -> i32 {
        let (c, t) = (&self.units[caster], &self.units[target]);
        let t_int = if self.class_of(pack, target).strategy_guard {
            t.int.max(0).saturating_mul(2)
        } else {
            t.int
        };
        let pc = int_term(c.int, c.level, 100);
        let pt = int_term(t_int, t.level, 100);
        if pc <= 0 {
            return if pt <= 0 { 100 } else { 0 };
        }
        let div = if original_formulas(pack) && confuses(s) {
            2
        } else {
            4
        };
        (100 - 100 * pt / (div * pc)).clamp(0, 100) as i32
    }

    /// Strategy damage without the random bonus: `max(1, raw)` (§5; `max(0, raw)` under the
    /// original formulas).
    pub(super) fn strategy_damage_base(
        &self,
        pack: &Pack,
        caster: UnitId,
        s: &StrategyDef,
        power: i32,
        target: UnitId,
        terrain: Option<&TerrainDef>,
    ) -> i32 {
        let (c, t) = (&self.units[caster], &self.units[target]);
        let mut raw =
            power as i64 + 2 * int_term(c.int, c.level, 50) - int_term(t.int, t.level, 50);
        let boosted = s.element.as_deref().is_some_and(|e| {
            terrain.is_some_and(|tt| tt.boost.iter().any(|b| b == e))
                || (e == WATER && self.weather == Weather::Rain)
        });
        if boosted {
            raw = raw * 125 / 100;
        }
        if self.class_of(pack, target).strategy_guard {
            raw /= 2;
        }
        let least = if original_formulas(pack) { 0 } else { 1 };
        raw.clamp(least, i32::MAX as i64) as i32
    }

    /// Strategy heal before capping at the missing HP and without the random bonus of the
    /// original formulas (§5).
    pub(super) fn strategy_heal(
        &self,
        pack: &Pack,
        caster: UnitId,
        power: i32,
        target: UnitId,
    ) -> i32 {
        let (c, t) = (&self.units[caster], &self.units[target]);
        let amount = if original_formulas(pack) {
            power as i64 + c.level as i64 * c.int.max(0) as i64 / 20
        } else {
            let amount = power as i64 + 2 * int_term(c.int, c.level, 50);
            if t.morale < LOW_MORALE_HEAL {
                amount / 2
            } else {
                amount
            }
        };
        amount.clamp(0, i32::MAX as i64) as i32
    }

    /// Up to 10 % more of a support amount under the original formulas (their random bonus),
    /// else nothing.
    fn support_bonus(&mut self, pack: &Pack, amount: i32) -> i32 {
        if original_formulas(pack) && amount >= 10 {
            self.rng.range(0, amount / 10)
        } else {
            0
        }
    }

    /// Morale change of a `Morale { amount }` effect without the random bonus of the original
    /// formulas: morale-down is shifted by `caster.level / 10 - target.level / 10` (never
    /// turning into a gain); under the original formulas a morale gain adds `caster.level / 10`.
    pub(super) fn morale_shift(
        &self,
        pack: &Pack,
        caster: UnitId,
        target: UnitId,
        amount: i32,
    ) -> i32 {
        if amount >= 0 {
            if original_formulas(pack) {
                return amount.saturating_add((self.units[caster].level / 10) as i32);
            }
            return amount;
        }
        let shift = (self.units[caster].level / 10) as i32 - (self.units[target].level / 10) as i32;
        amount.saturating_sub(shift).min(0)
    }

    /// Stored counter for a confusion of `turns` turns. The countdown runs at the start of
    /// the owner's phase before it acts, so a unit confused outside its own phase needs one
    /// extra count to actually miss `turns` of its phases.
    fn confusion_counter(&self, pack: &Pack, target: UnitId, turns: u8) -> u8 {
        if original_formulas(pack) {
            super::UNTIL_RECOVERED
        } else if self.units[target].side == self.phase {
            turns
        } else {
            turns.saturating_add(1)
        }
    }

    /// Confuse a unit (keeping the longer duration); returns whether it was newly confused.
    pub(super) fn confuse(&mut self, id: UnitId, turns: u8) -> bool {
        let u = &mut self.units[id];
        match u
            .statuses
            .iter_mut()
            .find(|s| s.status == StatusKind::Confused)
        {
            Some(s) => {
                s.turns = s.turns.max(turns);
                false
            }
            None => {
                u.statuses.push(ActiveStatus {
                    status: StatusKind::Confused,
                    turns,
                });
                true
            }
        }
    }

    pub(super) fn strategy_forecast(
        &self,
        pack: &Pack,
        caster: UnitId,
        strategy: &str,
        aim: Pos,
    ) -> Vec<StrategyForecast> {
        let Some(s) = pack.strategy(strategy) else {
            return Vec::new();
        };
        let board = Board::new(self, pack);
        let from = self.units[caster].pos;
        let Ok(targets) = self.strategy_area(&board, caster, s, from, aim) else {
            return Vec::new();
        };
        targets
            .into_iter()
            .map(|t| {
                let tu = &self.units[t];
                let terrain = board.terrain(tu.pos);
                let mut amount = 0i64;
                for e in &s.effects {
                    match e {
                        Effect::Damage { power } => {
                            amount += self.strategy_damage_base(pack, caster, s, *power, t, terrain)
                                as i64;
                        }
                        Effect::Heal { power } => {
                            let heal = self
                                .strategy_heal(pack, caster, *power, t)
                                .min(tu.max_hp - tu.hp)
                                .max(0);
                            amount -= heal as i64;
                        }
                        _ => {}
                    }
                }
                StrategyForecast {
                    unit: t,
                    chance: match s.target {
                        TargetSide::Enemy => self.hit_chance(pack, caster, s, t),
                        TargetSide::Ally => 100,
                    },
                    amount: amount.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                }
            })
            .collect()
    }

    pub(super) fn act_strategy(
        &mut self,
        pack: &Pack,
        unit: UnitId,
        strategy: &str,
        aim: Pos,
        ev: &mut Vec<BattleEvent>,
    ) -> Result<(), ActionError> {
        self.check_actor(unit)?;
        let unknown = || ActionError::UnknownStrategy(strategy.to_string());
        let s = pack.strategy(strategy).ok_or_else(unknown)?;
        let u = &self.units[unit];
        if !pack
            .known_strategies(&u.class, u.level)
            .iter()
            .any(|k| k == strategy)
        {
            return Err(unknown());
        }
        if u.mp < s.mp {
            return Err(ActionError::NotEnoughMp);
        }
        let targets = self.strategy_area(&Board::new(self, pack), unit, s, u.pos, aim)?;
        self.units[unit].mp -= s.mp.max(0);
        self.cast(pack, unit, s, aim, &targets, ev);
        Ok(())
    }

    /// Resolve a validated strategy (MP already paid): hit rolls, effects, retreats and EXP.
    fn cast(
        &mut self,
        pack: &Pack,
        caster: UnitId,
        s: &StrategyDef,
        aim: Pos,
        targets: &[UnitId],
        ev: &mut Vec<BattleEvent>,
    ) {
        let from = self.units[caster].pos;
        if aim != from {
            self.units[caster].facing = Dir::towards(from, aim);
        }
        self.units[caster].acted = true;
        let mut hits = Vec::with_capacity(targets.len());
        let mut newly_confused = Vec::new();
        for &t in targets {
            let success = match s.target {
                TargetSide::Enemy => {
                    let chance = self.hit_chance(pack, caster, s, t);
                    self.rng.chance(chance)
                }
                TargetSide::Ally => true,
            };
            let mut hit = StrategyHit {
                unit: t,
                success,
                damage: 0,
                healed: 0,
                morale: 0,
                status: None,
            };
            if success {
                self.apply_effects(pack, caster, s, t, &mut hit, &mut newly_confused);
            }
            hits.push(hit);
        }
        ev.push(BattleEvent::StrategyUsed {
            caster,
            strategy: s.id.clone(),
            target: aim,
            hits: hits.clone(),
        });
        for &unit in &newly_confused {
            ev.push(BattleEvent::Confused { unit });
        }
        let mut defeated = Vec::new();
        for h in &hits {
            if self.units[h.unit].is_active() && self.units[h.unit].hp == 0 {
                defeated.push(h.unit);
            }
            self.retreat_if_beaten(h.unit, ev);
        }
        // §7.1: attack strategies earn per damaged unit; support strategies once per cast.
        let exp = if hits.iter().any(|h| h.damage > 0) {
            hits.iter()
                .filter(|h| h.damage > 0)
                .map(|h| self.combat_exp(pack, caster, h.unit, defeated.contains(&h.unit)))
                .fold(0u32, u32::saturating_add)
        } else if hits.iter().any(|h| h.success) {
            match (s.area, self.class_of(pack, caster).support_exp) {
                (Area::Single, Some(exp)) => exp,
                _ => pack.rules.exp_support,
            }
        } else {
            0
        };
        self.gain_exp(pack, caster, exp, ev);
    }

    fn apply_effects(
        &mut self,
        pack: &Pack,
        caster: UnitId,
        s: &StrategyDef,
        t: UnitId,
        hit: &mut StrategyHit,
        newly_confused: &mut Vec<UnitId>,
    ) {
        let terrain = self.terrain_at(pack, self.units[t].pos);
        for e in &s.effects {
            if self.units[t].hp == 0 {
                break;
            }
            match e {
                Effect::Damage { power } => {
                    let base = self.strategy_damage_base(pack, caster, s, *power, t, terrain);
                    let damage = base.saturating_add(self.rng.range(0, base / 50));
                    let loss = self.take_damage(pack, t, damage);
                    hit.damage += damage;
                    hit.morale -= loss;
                }
                Effect::Heal { power } => {
                    let amount = self.strategy_heal(pack, caster, *power, t);
                    let amount = amount.saturating_add(self.support_bonus(pack, amount));
                    let u = &mut self.units[t];
                    let healed = amount.min(u.max_hp - u.hp).max(0);
                    u.hp += healed;
                    hit.healed += healed;
                }
                Effect::Morale { amount } => {
                    let mut delta = self.morale_shift(pack, caster, t, *amount);
                    if delta > 0 {
                        delta = delta.saturating_add(self.support_bonus(pack, delta));
                    }
                    let u = &mut self.units[t];
                    let before = u.morale;
                    u.morale = u.morale.saturating_add(delta).clamp(0, 100);
                    hit.morale += u.morale - before;
                    // The original: a morale-down leaving little morale may confuse.
                    let low = u.morale < MORALE_DOWN_CONFUSES_BELOW;
                    if delta < 0
                        && low
                        && original_formulas(pack)
                        && self.rng.chance(MORALE_DOWN_CONFUSION)
                    {
                        if self.confuse(t, super::UNTIL_RECOVERED) {
                            newly_confused.push(t);
                        }
                        hit.status = Some(StatusKind::Confused);
                    }
                }
                Effect::Status {
                    status: StatusKind::Confused,
                    turns,
                } => {
                    let counter = self.confusion_counter(pack, t, *turns);
                    if self.confuse(t, counter) {
                        newly_confused.push(t);
                    }
                    hit.status = Some(StatusKind::Confused);
                }
                // Class changes are camp actions (`CampaignState::use_item`); no battle effect.
                Effect::Promote | Effect::ChangeClass { .. } => {}
            }
        }
    }

    // ----- items ---------------------------------------------------------------------------

    /// Check that `user` can use `item` now (player side, in the inventory, battle-usable,
    /// supported effects).
    pub(super) fn battle_item<'a>(
        &self,
        pack: &'a Pack,
        user: UnitId,
        item: &str,
    ) -> Result<BattleItem<'a>, ActionError> {
        let bad = || ActionError::BadItem(item.to_string());
        if self.units[user].side != Side::Player
            || self.inventory.get(item).copied().unwrap_or(0) == 0
        {
            return Err(bad());
        }
        let def = pack
            .item(item)
            .filter(|d| d.is_battle_item())
            .ok_or_else(bad)?;
        if let Some(sid) = &def.strategy {
            return pack.strategy(sid).map(BattleItem::Scroll).ok_or_else(bad);
        }
        let supported = !def.effects.is_empty()
            && def
                .effects
                .iter()
                .all(|e| matches!(e, Effect::Heal { .. } | Effect::Morale { .. }));
        if supported {
            Ok(BattleItem::Direct(def))
        } else {
            Err(bad())
        }
    }

    pub(super) fn item_target_list(&self, pack: &Pack, id: UnitId, item: &str) -> Vec<UnitId> {
        let u = &self.units[id];
        if !u.is_active() || u.has_status(StatusKind::Confused) {
            return Vec::new();
        }
        let mut out = match self.battle_item(pack, id, item) {
            Err(_) => Vec::new(),
            Ok(BattleItem::Direct(_)) => {
                let mut v = vec![id];
                for p in u.pos.neighbors4() {
                    if let Some(o) = self
                        .unit_at(p)
                        .filter(|&o| !self.units[o].side.is_hostile(u.side))
                    {
                        v.push(o);
                    }
                }
                v
            }
            Ok(BattleItem::Scroll(s)) => {
                let board = Board::new(self, pack);
                match s.area {
                    Area::AllInRange => {
                        if self.strategy_area(&board, id, s, u.pos, u.pos).is_ok() {
                            vec![id]
                        } else {
                            Vec::new()
                        }
                    }
                    Area::Single | Area::Cross => reach_tiles(s, u.pos)
                        .into_iter()
                        .filter_map(|p| board.unit_at(p))
                        .filter(|&o| self.is_strategy_target(id, s, o))
                        .filter(|&o| {
                            self.strategy_area(&board, id, s, u.pos, self.units[o].pos)
                                .is_ok()
                        })
                        .collect(),
                }
            }
        };
        out.sort_unstable();
        out.dedup();
        out
    }

    pub(super) fn act_item(
        &mut self,
        pack: &Pack,
        unit: UnitId,
        item: &str,
        target: UnitId,
        ev: &mut Vec<BattleEvent>,
    ) -> Result<(), ActionError> {
        self.check_actor(unit)?;
        let kind = self.battle_item(pack, unit, item)?;
        let t = self
            .units
            .get(target)
            .filter(|t| t.is_active())
            .ok_or(ActionError::NoSuchUnit(target))?;
        let user = &self.units[unit];
        match kind {
            BattleItem::Direct(def) => {
                if t.side.is_hostile(user.side) {
                    return Err(ActionError::InvalidTarget);
                }
                if target != unit && user.pos.manhattan(t.pos) != 1 {
                    return Err(ActionError::OutOfRange);
                }
                let (from, to) = (user.pos, t.pos);
                self.consume(item);
                let (mut healed, mut morale) = (0, 0);
                for e in &def.effects {
                    let u = &mut self.units[target];
                    match e {
                        Effect::Heal { power } => {
                            let h = (*power).min(u.max_hp - u.hp).max(0);
                            u.hp += h;
                            healed += h;
                        }
                        Effect::Morale { amount } => {
                            let before = u.morale;
                            u.morale = u.morale.saturating_add(*amount).clamp(0, 100);
                            morale += u.morale - before;
                        }
                        // Rejected by `battle_item`.
                        _ => {}
                    }
                }
                if from != to {
                    self.units[unit].facing = Dir::towards(from, to);
                }
                self.units[unit].acted = true;
                ev.push(BattleEvent::ItemUsed {
                    user: unit,
                    target,
                    item: item.to_string(),
                    healed,
                    morale,
                });
            }
            BattleItem::Scroll(s) => {
                let valid = match s.area {
                    Area::AllInRange => target == unit,
                    Area::Single | Area::Cross => self.is_strategy_target(unit, s, target),
                };
                if !valid {
                    return Err(ActionError::InvalidTarget);
                }
                let (from, aim) = (user.pos, t.pos);
                let targets = self.strategy_area(&Board::new(self, pack), unit, s, from, aim)?;
                self.consume(item);
                ev.push(BattleEvent::ItemUsed {
                    user: unit,
                    target,
                    item: item.to_string(),
                    healed: 0,
                    morale: 0,
                });
                self.cast(pack, unit, s, aim, &targets, ev);
            }
        }
        Ok(())
    }

    /// Take one `item` out of the battle's stock and record the use in `items_used`, which the
    /// campaign subtracts when the battle ends. Callers checked the stock with `battle_item`.
    fn consume(&mut self, item: &str) {
        let Some(n) = self.inventory.get_mut(item).filter(|n| **n > 0) else {
            return;
        };
        *n -= 1;
        if *n == 0 {
            self.inventory.remove(item);
        }
        let used = self.items_used.entry(item.to_string()).or_insert(0);
        *used = used.saturating_add(1);
    }
}
