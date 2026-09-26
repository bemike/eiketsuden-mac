//! Values of an army officer as they will be at the start of the next battle, computed with the
//! battle engine's own formulas (RULES.md §2), and equipment previews.
//!
//! ATK, DEF and movement come straight from `BattleState::attack_power`, `defense_power` and
//! `move_points` on a one-unit probe battle, so the camp always shows exactly what the battle will
//! use. Max HP and MP are the two one-line formulas of RULES.md §2 (the engine keeps its versions
//! private); a test checks them against a real `BattleState::new`.

use hero_core::battle::{BattleState, Unit, UnitState, Weather};
use hero_core::battledef::{AiMode, Side};
use hero_core::campaign::{CampaignError, CampaignState, OfficerState};
use hero_core::data::{ClassDef, ItemKind};
use hero_core::geom::{Dir, Pos};
use hero_core::map::BattleMap;
use hero_core::pack::Pack;
use hero_core::rng::Rng;
use std::collections::BTreeMap;

/// An officer's battle values at full strength and starting morale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfficerStats {
    /// Max HP (troops).
    pub hp: i32,
    /// Max MP (strategy points).
    pub mp: i32,
    pub atk: i32,
    pub def: i32,
    /// Movement points.
    pub mov: i32,
}

/// Max HP: `class.hp + class.hp_growth * (level - 1)`, at least 1 (RULES.md §2).
pub fn max_hp(class: &ClassDef, level: u32) -> i32 {
    let v = i64::from(class.hp) + i64::from(class.hp_growth) * (i64::from(level.max(1)) - 1);
    v.clamp(1, i64::from(i32::MAX)) as i32
}

/// Max MP: `min(mp_cap, (level + 10) * int / 40)` (RULES.md §2).
pub fn max_mp(pack: &Pack, level: u32, int: i32) -> i32 {
    let v = (i64::from(level) + 10) * i64::from(int.max(0)) / 40;
    v.clamp(0, i64::from(pack.rules.mp_cap.max(0))) as i32
}

/// A battle holding only `officer`, on a one-tile map, at the battle start morale.
fn probe(pack: &Pack, officer: &OfficerState, hp: i32, mp: i32) -> BattleState {
    let terrain = pack
        .terrain
        .first()
        .map(|t| t.id.clone())
        .unwrap_or_default();
    let unit = Unit {
        id: 0,
        side: Side::Player,
        officer: Some(officer.id.clone()),
        name: officer.id.clone(),
        class: officer.class.clone(),
        level: officer.level.max(1),
        exp: officer.exp,
        strength: officer.strength,
        int: officer.int,
        lead: officer.lead,
        hp,
        max_hp: hp,
        mp,
        max_mp: mp,
        morale: pack.rules.morale_start.clamp(0, 100),
        pos: Pos::new(0, 0),
        facing: Dir::default(),
        moved: false,
        acted: false,
        equip: officer.equip.clone(),
        statuses: Vec::new(),
        ai: AiMode::Aggressive,
        ai_target: None,
        ai_pos: None,
        commander: false,
        lord: false,
        tag: None,
        group: None,
        state: UnitState::Active,
        portrait: None,
        drop: None,
    };
    BattleState {
        battle_id: String::new(),
        map: BattleMap {
            width: 1,
            height: 1,
            terrain_ids: vec![terrain],
            tiles: vec![0],
        },
        units: vec![unit],
        turn: 1,
        turn_limit: 1,
        phase: Side::Player,
        weather: Weather::Clear,
        rng: Rng::new(0),
        fired: Vec::new(),
        treasures_taken: Vec::new(),
        outcome: None,
        bonus_done: false,
        inventory: BTreeMap::new(),
        items_used: BTreeMap::new(),
        gold_found: 0,
        items_found: Vec::new(),
        flags: BTreeMap::new(),
    }
}

/// Battle values of `officer`; `None` when its class is not in the pack.
pub fn officer_stats(pack: &Pack, officer: &OfficerState) -> Option<OfficerStats> {
    let class = pack.class(&officer.class)?;
    let level = officer.level.max(1);
    let hp = max_hp(class, level);
    let mp = max_mp(pack, level, officer.int);
    let battle = probe(pack, officer, hp, mp);
    Some(OfficerStats {
        hp,
        mp,
        atk: battle.attack_power(pack, 0),
        def: battle.defense_power(pack, 0),
        mov: battle.move_points(pack, 0),
    })
}

/// A change to an officer's equipment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipChange {
    Equip(String),
    Unequip(ItemKind),
}

/// Stats before and after `change`, applied with the campaign's own rules on a copy (so class
/// family restrictions and slot rules are exactly those of `CampaignState::equip`).
pub fn preview_change(
    pack: &Pack,
    campaign: &CampaignState,
    officer: &str,
    change: &EquipChange,
) -> Result<(OfficerStats, OfficerStats), CampaignError> {
    let unknown = || CampaignError::NotInArmy(officer.to_string());
    let before = campaign
        .officer(officer)
        .and_then(|o| officer_stats(pack, o))
        .ok_or_else(unknown)?;
    let mut trial = campaign.clone();
    match change {
        EquipChange::Equip(item) => trial.equip(pack, officer, item)?,
        EquipChange::Unequip(slot) => trial.unequip(officer, *slot)?,
    }
    let after = trial
        .officer(officer)
        .and_then(|o| officer_stats(pack, o))
        .ok_or_else(unknown)?;
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::camp::test_pack;

    #[test]
    fn matches_the_battle_engine() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        // Give the officers some variety: levels and a different weapon.
        campaign.officer_mut("liu_bei").unwrap().level = 7;
        campaign.officer_mut("zhang_fei").unwrap().level = 12;
        let battle = BattleState::new(&pack, "p1_sishui", &campaign, 1).unwrap();
        let mut checked = 0;
        for unit in battle.units.iter().filter(|u| u.side == Side::Player) {
            let officer = campaign.officer(unit.officer.as_deref().unwrap()).unwrap();
            let s = officer_stats(&pack, officer).unwrap();
            assert_eq!(s.hp, unit.max_hp, "{}", unit.name);
            assert_eq!(s.mp, unit.max_mp, "{}", unit.name);
            assert_eq!(s.atk, battle.attack_power(&pack, unit.id), "{}", unit.name);
            assert_eq!(s.def, battle.defense_power(&pack, unit.id), "{}", unit.name);
            assert_eq!(s.mov, battle.move_points(&pack, unit.id), "{}", unit.name);
            checked += 1;
        }
        assert_eq!(checked, 3);
    }

    #[test]
    fn previews_equipment_changes() {
        let pack = test_pack();
        let mut campaign = CampaignState::new_game(&pack);
        campaign.add_item("red_hare", 1);
        campaign.add_item("sunzi", 1);
        let (before, after) = preview_change(
            &pack,
            &campaign,
            "liu_bei",
            &EquipChange::Equip("red_hare".into()),
        )
        .unwrap();
        assert_eq!(after.mov, before.mov + 3);
        assert_eq!(after.atk, before.atk);
        let (before, after) = preview_change(
            &pack,
            &campaign,
            "liu_bei",
            &EquipChange::Equip("sunzi".into()),
        )
        .unwrap();
        assert_eq!(after.def, before.def * 122 / 100);
        // Unequipping Guan Yu's blade removes its +12%.
        let (before, after) = preview_change(
            &pack,
            &campaign,
            "guan_yu",
            &EquipChange::Unequip(ItemKind::Weapon),
        )
        .unwrap();
        assert!(after.atk < before.atk);
        assert_eq!(before.atk, after.atk * 112 / 100);
        // The campaign itself is untouched, and the engine's errors come through.
        assert_eq!(campaign.item_count("red_hare"), 1);
        assert!(matches!(
            preview_change(
                &pack,
                &campaign,
                "liu_bei",
                &EquipChange::Equip("bean".into())
            ),
            Err(CampaignError::NotEquipment(_))
        ));
        assert!(preview_change(
            &pack,
            &campaign,
            "nobody",
            &EquipChange::Unequip(ItemKind::Armor)
        )
        .is_err());
    }
}
