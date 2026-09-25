//! Derived unit values (RULES.md §2).

use super::{BattleState, UnitId};
use crate::data::{ClassDef, GameRules, Id, ItemDef};
use crate::pack::Pack;

/// Stat term `s(x) = 4000 / (140 - x)` in tenths (`x = 100 -> 100`, `x = 75 -> 61`),
/// with `x` clamped to `0..=139`.
pub(super) fn stat_term(x: i32) -> i32 {
    4000 / (140 - x.clamp(0, 139))
}

/// Max HP (troops): `class.hp + class.hp_growth * (level - 1)`, at least 1.
pub(super) fn max_hp(class: &ClassDef, level: u32) -> i32 {
    let v = class.hp as i64 + class.hp_growth as i64 * (level.max(1) as i64 - 1);
    v.clamp(1, i32::MAX as i64) as i32
}

/// Max MP: `min(mp_cap, (level + 10) * int / 40)`.
pub(super) fn max_mp(rules: &GameRules, level: u32, int: i32) -> i32 {
    let v = (level as i64 + 10) * int.max(0) as i64 / 40;
    v.clamp(0, rules.mp_cap.max(0) as i64) as i32
}

/// `(level + 10) * (morale + s(stat) + coef * 10) / 10`, then `* pct / 100` when `pct > 0`.
fn power(level: u32, morale: i32, stat: i32, coef: i32, pct: i32) -> i32 {
    let base = (level as i64 + 10) * (morale as i64 + stat_term(stat) as i64 + coef as i64 * 10) / 10;
    let v = if pct > 0 { base * pct as i64 / 100 } else { base };
    v.clamp(0, i32::MAX as i64) as i32
}

fn equipped<'a>(pack: &'a Pack, slot: &Option<Id>) -> Option<&'a ItemDef> {
    slot.as_deref().and_then(|id| pack.item(id))
}

impl BattleState {
    /// Class definition of a unit. Classes are checked when the battle is built, so a missing
    /// class means the state was loaded against a different pack.
    pub(super) fn class_of<'a>(&self, pack: &'a Pack, id: UnitId) -> &'a ClassDef {
        let class = &self.units[id].class;
        pack.class(class)
            .unwrap_or_else(|| panic!("unit {id} has class `{class}`, which is not in the pack"))
    }

    /// ATK computed with the given morale instead of the unit's current one.
    pub(super) fn attack_with_morale(&self, pack: &Pack, id: UnitId, morale: i32) -> i32 {
        let u = &self.units[id];
        let class = self.class_of(pack, id);
        let pct = equipped(pack, &u.equip.weapon).map_or(0, |i| i.atk_pct);
        power(u.level, morale, u.strength, class.atk, pct)
    }

    /// DEF computed with the given morale instead of the unit's current one.
    pub(super) fn defense_with_morale(&self, pack: &Pack, id: UnitId, morale: i32) -> i32 {
        let u = &self.units[id];
        let class = self.class_of(pack, id);
        let pct = equipped(pack, &u.equip.armor).map_or(0, |i| i.def_pct);
        power(u.level, morale, u.lead, class.def, pct)
    }

    /// Movement points ignoring confusion: `class.move + accessory move_bonus`.
    pub(super) fn base_move_points(&self, pack: &Pack, id: UnitId) -> i32 {
        let u = &self.units[id];
        let bonus = equipped(pack, &u.equip.accessory).map_or(0, |i| i.move_bonus);
        (self.class_of(pack, id).move_points as i32 + bonus).max(0)
    }
}
