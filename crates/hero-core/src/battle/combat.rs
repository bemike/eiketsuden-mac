//! Physical attacks and counters (RULES.md §4), experience and levels (§7), retreat (§11),
//! and the per-unit action validation shared by every command.

use super::stats::{max_hp, max_mp};
use super::{
    ActionError, AttackForecast, BattleEvent, BattleState, CounterForecast, UnitId, UnitState,
};
use crate::battledef::Side;
use crate::data::{GameRules, StatusKind};
use crate::geom::{Dir, Pos};
use crate::pack::Pack;

/// Physical damage: `max(1, (ATK - DEF * affinity / 100 / 2) * (100 - terrain) / 100)`.
pub(super) fn hit_damage(atk: i32, def: i32, affinity: i32, terrain_defense: i32) -> i32 {
    let def_eff = def as i64 * affinity as i64 / 100;
    let raw = (atk as i64 - def_eff / 2) * (100 - terrain_defense as i64) / 100;
    raw.clamp(1, i32::MAX as i64) as i32
}

/// Morale lost when taking `damage`: `ceil(damage * pct / max_hp)`.
pub(super) fn morale_loss(rules: &GameRules, damage: i32, max_hp: i32) -> i32 {
    if damage <= 0 || rules.morale_loss_pct <= 0 {
        return 0;
    }
    let num = damage as i64 * rules.morale_loss_pct as i64;
    let den = max_hp.max(1) as i64;
    ((num + den - 1) / den).min(i32::MAX as i64) as i32
}

/// Counter chance in percent: `str * 100 / counter_divisor`, clamped to 0..=100.
pub(super) fn counter_chance(rules: &GameRules, strength: i32) -> i32 {
    if rules.counter_divisor <= 0 {
        return 0;
    }
    (strength as i64 * 100 / rules.counter_divisor as i64).clamp(0, 100) as i32
}

impl BattleState {
    pub(super) fn terrain_defense(&self, pack: &Pack, pos: Pos) -> i32 {
        self.terrain_at(pack, pos).map_or(0, |t| t.defense)
    }

    /// Defender DEF multiplier (percent) for this attacker/defender pair.
    pub(super) fn affinity(&self, pack: &Pack, attacker: UnitId, defender: UnitId) -> i32 {
        pack.rules.affinity_pct(
            &self.class_of(pack, attacker).family,
            &self.class_of(pack, defender).family,
        )
    }

    /// Damage of one strike with explicit morale values and defender tile.
    pub(super) fn strike_damage(
        &self,
        pack: &Pack,
        att: UnitId,
        att_morale: i32,
        def: UnitId,
        def_morale: i32,
        def_pos: Pos,
    ) -> i32 {
        hit_damage(
            self.attack_with_morale(pack, att, att_morale),
            self.defense_with_morale(pack, def, def_morale),
            self.affinity(pack, att, def),
            self.terrain_defense(pack, def_pos),
        )
    }

    /// Counter damage: a normal strike by `counterer` times `counter_damage_pct`, at least 1.
    pub(super) fn counter_damage(
        &self,
        pack: &Pack,
        counterer: UnitId,
        counterer_morale: i32,
        victim: UnitId,
    ) -> i32 {
        let v = &self.units[victim];
        let dmg = self.strike_damage(pack, counterer, counterer_morale, victim, v.morale, v.pos);
        ((dmg as i64 * pack.rules.counter_damage_pct as i64 / 100).max(1)).min(i32::MAX as i64)
            as i32
    }

    /// Whether `def` would counter an attack from `att` standing on `att_pos` (all conditions
    /// of §4 except the survival of the defender and the chance roll).
    pub(super) fn counter_applies(
        &self,
        pack: &Pack,
        att: UnitId,
        att_pos: Pos,
        def: UnitId,
    ) -> bool {
        let d = &self.units[def];
        let dc = self.class_of(pack, def);
        if !(dc.can_counter && self.class_of(pack, att).provokes_counter)
            || att_pos.chebyshev(d.pos) != 1
        {
            return false;
        }
        let delta = Pos::new(att_pos.x - d.pos.x, att_pos.y - d.pos.y);
        dc.range.offsets().is_some_and(|o| o.contains(&delta))
    }

    pub(super) fn attack_forecast(&self, pack: &Pack, att: UnitId, def: UnitId) -> AttackForecast {
        let (a, d) = (&self.units[att], &self.units[def]);
        let damage = self.strike_damage(pack, att, a.morale, def, d.morale, d.pos);
        let affinity = self.affinity(pack, att, def);
        let mut counter = None;
        if damage < d.hp && self.counter_applies(pack, att, a.pos, def) {
            let d_morale = d.morale - morale_loss(&pack.rules, damage, d.max_hp).min(d.morale);
            let routed = d.has_status(StatusKind::Confused) && d_morale == 0;
            if !routed {
                counter = Some(CounterForecast {
                    damage: self.counter_damage(pack, def, d_morale, att),
                    chance: counter_chance(&pack.rules, d.strength),
                });
            }
        }
        AttackForecast {
            damage,
            affinity,
            counter,
        }
    }

    /// Validate that `unit` may act now.
    pub(super) fn check_actor(&self, unit: UnitId) -> Result<(), ActionError> {
        let u = self
            .units
            .get(unit)
            .filter(|u| u.is_active())
            .ok_or(ActionError::NoSuchUnit(unit))?;
        if u.side != self.phase {
            return Err(ActionError::NotYourTurn(unit));
        }
        if u.acted {
            return Err(ActionError::AlreadyActed(unit));
        }
        if u.has_status(StatusKind::Confused) {
            return Err(ActionError::Confused(unit));
        }
        Ok(())
    }

    pub(super) fn act_move(
        &mut self,
        pack: &Pack,
        unit: UnitId,
        to: Pos,
        ev: &mut Vec<BattleEvent>,
    ) -> Result<(), ActionError> {
        self.check_actor(unit)?;
        if self.units[unit].moved {
            return Err(ActionError::AlreadyMoved(unit));
        }
        let path = self
            .movement_range(pack, unit)
            .path_to(to)
            .ok_or(ActionError::Unreachable)?;
        let u = &mut self.units[unit];
        if let [.., before, _] = path.as_slice() {
            u.facing = Dir::towards(*before, to);
        }
        u.pos = to;
        u.moved = true;
        let side = u.side;
        ev.push(BattleEvent::Moved { unit, path });
        if side == Side::Player {
            self.take_treasure(pack, unit, ev);
        }
        Ok(())
    }

    /// Give every untaken treasure on the unit's tile to the player (§10).
    fn take_treasure(&mut self, pack: &Pack, unit: UnitId, ev: &mut Vec<BattleEvent>) {
        let pos = self.units[unit].pos;
        let def = self.def(pack);
        if self.treasures_taken.len() < def.treasures.len() {
            self.treasures_taken.resize(def.treasures.len(), false);
        }
        for (i, t) in def.treasures.iter().enumerate() {
            if t.pos != pos || self.treasures_taken[i] {
                continue;
            }
            self.treasures_taken[i] = true;
            if let Some(item) = &t.item {
                self.items_found.push(item.clone());
            }
            self.gold_found += t.gold;
            ev.push(BattleEvent::TreasureFound {
                unit,
                item: t.item.clone(),
                gold: t.gold,
            });
        }
    }

    pub(super) fn act_attack(
        &mut self,
        pack: &Pack,
        unit: UnitId,
        target: UnitId,
        ev: &mut Vec<BattleEvent>,
    ) -> Result<(), ActionError> {
        self.check_actor(unit)?;
        let t = self
            .units
            .get(target)
            .filter(|t| t.is_active())
            .ok_or(ActionError::NoSuchUnit(target))?;
        if !t.side.is_hostile(self.units[unit].side) {
            return Err(ActionError::InvalidTarget);
        }
        if !self
            .attack_tiles(pack, unit, self.units[unit].pos)
            .contains(&t.pos)
        {
            return Err(ActionError::OutOfRange);
        }
        self.do_attack(pack, unit, target, ev);
        Ok(())
    }

    /// Resolve a validated physical attack including the counter and EXP.
    fn do_attack(&mut self, pack: &Pack, att: UnitId, def: UnitId, ev: &mut Vec<BattleEvent>) {
        let (a_pos, d_pos) = (self.units[att].pos, self.units[def].pos);
        self.units[att].facing = Dir::towards(a_pos, d_pos);
        self.units[def].facing = Dir::towards(d_pos, a_pos);
        self.units[att].acted = true;

        let damage = self.strike_damage(
            pack,
            att,
            self.units[att].morale,
            def,
            self.units[def].morale,
            d_pos,
        );
        let loss = self.take_damage(pack, def, damage);
        ev.push(BattleEvent::Strike {
            attacker: att,
            defender: def,
            damage,
            morale_loss: loss,
            counter: false,
        });
        let def_killed = self.units[def].hp == 0;
        self.retreat_if_beaten(def, ev);

        let mut att_killed = false;
        let mut countered = false;
        if self.units[def].is_active() && self.counter_applies(pack, att, a_pos, def) {
            let chance = counter_chance(&pack.rules, self.units[def].strength);
            if self.rng.chance(chance) {
                countered = true;
                let damage = self.counter_damage(pack, def, self.units[def].morale, att);
                let loss = self.take_damage(pack, att, damage);
                ev.push(BattleEvent::Strike {
                    attacker: def,
                    defender: att,
                    damage,
                    morale_loss: loss,
                    counter: true,
                });
                att_killed = self.units[att].hp == 0;
                self.retreat_if_beaten(att, ev);
            }
        }

        // EXP amounts use the levels from before any level up of this exchange.
        let att_exp = self.combat_exp(pack, att, def, def_killed);
        let def_exp = if countered {
            self.combat_exp(pack, def, att, att_killed)
        } else {
            0
        };
        self.gain_exp(pack, att, att_exp, ev);
        self.gain_exp(pack, def, def_exp, ev);
    }

    /// Apply HP damage and the resulting morale loss; returns the morale actually lost.
    pub(super) fn take_damage(&mut self, pack: &Pack, id: UnitId, damage: i32) -> i32 {
        let loss = morale_loss(&pack.rules, damage, self.units[id].max_hp);
        let u = &mut self.units[id];
        u.hp = (u.hp - damage.max(0)).max(0);
        let loss = loss.min(u.morale).max(0);
        u.morale -= loss;
        loss
    }

    /// Retreat a unit at 0 HP (defeated) or confused at 0 morale (routed, §6). Both drop the
    /// unit's `drop` item. Returns whether the unit retreated.
    pub(super) fn retreat_if_beaten(&mut self, id: UnitId, ev: &mut Vec<BattleEvent>) -> bool {
        let u = &self.units[id];
        let beaten =
            u.is_active() && (u.hp == 0 || (u.morale == 0 && u.has_status(StatusKind::Confused)));
        if beaten {
            self.retreat(id, true, ev);
        }
        beaten
    }

    /// Remove a unit from the map (§11). `drop` gives its drop item to the player.
    pub(super) fn retreat(&mut self, id: UnitId, drop: bool, ev: &mut Vec<BattleEvent>) {
        let u = &mut self.units[id];
        if !u.is_active() {
            return;
        }
        u.state = UnitState::Retreated;
        ev.push(BattleEvent::Retreated { unit: id });
        if let (true, Some(item)) = (drop, u.drop.clone()) {
            self.items_found.push(item.clone());
            ev.push(BattleEvent::ItemDropped { unit: id, item });
        }
    }

    /// EXP for damaging (or defeating) `target` (§7.1).
    pub(super) fn combat_exp(
        &self,
        pack: &Pack,
        earner: UnitId,
        target: UnitId,
        defeated: bool,
    ) -> u32 {
        let rules = &pack.rules;
        let t = &self.units[target];
        let diff = t.level as i64 - self.units[earner].level as i64;
        let diff = diff.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        if defeated {
            let bonus = if t.commander { rules.exp_commander } else { 0 };
            GameRules::exp_lookup(&rules.exp_kill, diff).saturating_add(bonus)
        } else {
            GameRules::exp_lookup(&rules.exp_attack, diff)
        }
    }

    /// Add EXP and level up while enough is collected (§7.2). Only the player's army keeps
    /// progress between battles, so only player units gain EXP *(design)*.
    pub(super) fn gain_exp(
        &mut self,
        pack: &Pack,
        id: UnitId,
        amount: u32,
        ev: &mut Vec<BattleEvent>,
    ) {
        if amount == 0 || self.units[id].side != Side::Player {
            return;
        }
        ev.push(BattleEvent::ExpGained { unit: id, amount });
        let rules = &pack.rules;
        let per = rules.exp_per_level;
        let u = &mut self.units[id];
        u.exp = u.exp.saturating_add(amount);
        if per == 0 {
            return;
        }
        while self.units[id].exp >= per {
            if self.units[id].level >= rules.level_cap {
                self.units[id].exp = per - 1;
                break;
            }
            self.units[id].exp -= per;
            self.level_up(pack, id, ev);
        }
    }

    /// Grant whole levels (duel rewards); EXP is kept. Stops at the level cap.
    pub(super) fn gain_levels(
        &mut self,
        pack: &Pack,
        id: UnitId,
        levels: u32,
        ev: &mut Vec<BattleEvent>,
    ) {
        for _ in 0..levels {
            if self.units[id].level >= pack.rules.level_cap {
                break;
            }
            self.level_up(pack, id, ev);
        }
    }

    /// One level: max HP by `hp_growth` (current HP too), max MP recomputed (current MP by
    /// the difference), `LevelUp` plus `Learned` for each newly known strategy.
    fn level_up(&mut self, pack: &Pack, id: UnitId, ev: &mut Vec<BattleEvent>) {
        let class = self.class_of(pack, id);
        let u = &mut self.units[id];
        let known_before = pack.known_strategies(&u.class, u.level);
        u.level += 1;
        let new_max_hp = max_hp(class, u.level);
        let hp_gain = new_max_hp - u.max_hp;
        let new_max_mp = max_mp(&pack.rules, u.level, u.int);
        let mp_gain = new_max_mp - u.max_mp;
        u.max_hp = new_max_hp;
        u.max_mp = new_max_mp;
        if u.is_active() {
            u.hp = (u.hp + hp_gain).clamp(0, new_max_hp);
            u.mp = (u.mp + mp_gain).clamp(0, new_max_mp);
        }
        let level = u.level;
        ev.push(BattleEvent::LevelUp {
            unit: id,
            level,
            hp_gain,
            mp_gain,
        });
        for s in pack.known_strategies(&class.id, level) {
            if !known_before.contains(&s) {
                ev.push(BattleEvent::Learned {
                    unit: id,
                    strategy: s,
                });
            }
        }
    }
}
