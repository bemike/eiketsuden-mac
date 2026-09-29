//! Building the initial battle state ([`BattleState::new`]).
//!
//! Unit order is: deployed player officers (slot order), spawns that start on the map, then
//! reinforcements (definition order). AI units act in unit order, so reinforcements act after
//! the units that were on the map first (RULES.md §1.5).
//!
//! Every army officer is at most one unit: an army officer named by a `side = "player"` spawn
//! is placed there (with the army's progress) and takes no deploy slot.

use super::stats::{max_hp, max_mp};
use super::{BattleError, BattleState, Unit, UnitId, UnitState, Weather};
use crate::battledef::{AiMode, BattleDef, Side, UnitSpawn};
use crate::campaign::{CampaignState, OfficerState};
use crate::data::{Equipment, Id};
use crate::geom::{Dir, Pos};
use crate::map::BattleMap;
use crate::pack::Pack;
use crate::rng::Rng;
use std::collections::BTreeMap;

fn setup_err(msg: String) -> BattleError {
    BattleError::Setup(msg)
}

pub(super) fn build(
    pack: &Pack,
    battle: &str,
    campaign: &CampaignState,
    seed: u64,
) -> Result<BattleState, BattleError> {
    let def = pack
        .battles
        .get(battle)
        .ok_or_else(|| BattleError::UnknownBattle(battle.to_string()))?;
    let map = BattleMap::parse(&def.map.rows, &def.map.legend, &pack.terrain)
        .map_err(|e| setup_err(format!("map of battle `{battle}`: {e}")))?;

    let mut units: Vec<Unit> = Vec::new();
    // Army officers the battle places itself fight from their spawn, not from a deploy slot.
    let spawned = |officer: &str| {
        def.units
            .iter()
            .any(|s| army_officer(campaign, s).is_some_and(|o| o.id == officer))
    };
    // `normalize_deployment` keeps at most one officer per slot.
    let deployed = deployment(pack, def, campaign);
    for (officer, &slot) in deployed
        .iter()
        .filter(|o| !spawned(o))
        .zip(&def.deploy.slots)
    {
        let state = campaign
            .officer(officer)
            .ok_or_else(|| setup_err(format!("deployed officer `{officer}` is not in the army")))?;
        units.push(officer_unit(pack, units.len(), state, slot)?);
    }
    let on_map = def.units.iter().filter(|s| s.group.is_none());
    let reinforcements = def.units.iter().filter(|s| s.group.is_some());
    for spawn in on_map.chain(reinforcements) {
        units.push(spawn_unit(pack, units.len(), spawn, campaign)?);
    }

    let mut occupied: BTreeMap<Pos, UnitId> = BTreeMap::new();
    for u in &units {
        if !map.in_bounds(u.pos) {
            return Err(setup_err(format!(
                "unit `{}` is placed outside the map at {:?}",
                u.name, u.pos
            )));
        }
        if u.is_active() {
            if let Some(other) = occupied.insert(u.pos, u.id) {
                return Err(setup_err(format!(
                    "units `{}` and `{}` both start on {:?}",
                    units[other].name, u.name, u.pos
                )));
            }
        }
    }

    Ok(BattleState {
        battle_id: battle.to_string(),
        map,
        units,
        turn: 1,
        turn_limit: def.turn_limit,
        phase: Side::Player,
        weather: Weather::Clear,
        rng: Rng::new(seed),
        fired: vec![false; def.events.len()],
        treasures_taken: vec![false; def.treasures.len()],
        outcome: None,
        bonus_done: false,
        inventory: campaign.inventory.clone(),
        items_used: BTreeMap::new(),
        gold_found: 0,
        items_found: Vec::new(),
        flags: BTreeMap::new(),
        stage: 0,
        objective: None,
        start_flags: campaign.flags.clone(),
        map_images: Vec::new(),
    })
}

/// Most officers that may be deployed in `def`: `deploy.max`, but no more than it has slots.
pub fn deploy_max(def: &BattleDef) -> usize {
    (def.deploy.max as usize).min(def.deploy.slots.len())
}

/// The deployment `def` places for `chosen` (the deploy screen's choice), in slot order:
///
/// 1. the battle's required officers, in `deploy.required` order;
/// 2. the lord;
/// 3. the chosen officers, in roster order;
///
/// at most [`deploy_max`] officers. Officers who are not in the army, are away or are forbidden
/// in this battle are left out, and so are duplicates, so a list chosen for another battle can be
/// passed as it is. The deploy screen shows this list; [`BattleState::new`] places it.
pub fn normalize_deployment(
    pack: &Pack,
    def: &BattleDef,
    campaign: &CampaignState,
    chosen: &[Id],
) -> Vec<Id> {
    let forbidden = |id: &str| def.deploy.forbidden.iter().any(|f| f == id);
    let mut out: Vec<Id> = Vec::new();
    let mut add = |id: &Id| {
        if campaign.officer(id).is_some_and(|o| !o.away) && !forbidden(id) && !out.contains(id) {
            out.push(id.clone());
        }
    };
    for id in &def.deploy.required {
        add(id);
    }
    for o in &campaign.roster {
        if pack.officer(&o.id).is_some_and(|d| d.lord) {
            add(&o.id);
        }
    }
    for o in &campaign.roster {
        if chosen.contains(&o.id) {
            add(&o.id);
        }
    }
    out.truncate(deploy_max(def));
    out
}

/// Officers placed on the deploy slots: `campaign.deployed` (or, when nothing was chosen, the
/// whole roster) normalised to this battle by [`normalize_deployment`]. The list is not trusted:
/// it may have been chosen for another battle (a battle that follows a battle, or a camp
/// without a deploy screen), or the army may have changed since.
fn deployment(pack: &Pack, def: &BattleDef, campaign: &CampaignState) -> Vec<Id> {
    let chosen: Vec<Id> = if campaign.deployed.is_empty() {
        campaign.roster.iter().map(|o| o.id.clone()).collect()
    } else {
        campaign.deployed.clone()
    };
    normalize_deployment(pack, def, campaign, &chosen)
}

fn check_equipment(pack: &Pack, who: &str, equip: &Equipment) -> Result<(), BattleError> {
    match equip.iter().find(|i| pack.item(i).is_none()) {
        Some(item) => Err(setup_err(format!("`{who}` has unknown equipment `{item}`"))),
        None => Ok(()),
    }
}

fn officer_unit(
    pack: &Pack,
    id: UnitId,
    state: &OfficerState,
    slot: Pos,
) -> Result<Unit, BattleError> {
    let od = pack
        .officer(&state.id)
        .ok_or_else(|| setup_err(format!("unknown officer `{}`", state.id)))?;
    let class = pack.class(&state.class).ok_or_else(|| {
        setup_err(format!(
            "officer `{}` has unknown class `{}`",
            state.id, state.class
        ))
    })?;
    check_equipment(pack, &state.id, &state.equip)?;
    let level = state.level.max(1);
    let hp = max_hp(class, level);
    let mp = max_mp(&pack.rules, level, state.int);
    Ok(Unit {
        id,
        side: Side::Player,
        officer: Some(state.id.clone()),
        name: od.name.clone(),
        class: state.class.clone(),
        level,
        exp: state.exp,
        strength: state.strength,
        int: state.int,
        lead: state.lead,
        hp,
        max_hp: hp,
        mp,
        max_mp: mp,
        morale: pack.rules.morale_start.clamp(0, 100),
        pos: slot,
        facing: Dir::default(),
        moved: false,
        acted: false,
        equip: state.equip.clone(),
        statuses: Vec::new(),
        ai: AiMode::Aggressive,
        ai_target: None,
        ai_pos: None,
        commander: false,
        lord: od.lord,
        tag: None,
        group: None,
        state: UnitState::Active,
        portrait: Some(od.portrait_key().to_string()),
        drop: None,
    })
}

/// The army's state of the officer a `side = "player"` spawn names, when that officer is in
/// the army. Such a spawn places the army's officer (one unit, with their progress) instead of
/// a second copy built from `officers.toml`.
fn army_officer<'a>(campaign: &'a CampaignState, sp: &UnitSpawn) -> Option<&'a OfficerState> {
    if sp.side != Side::Player {
        return None;
    }
    campaign.officer(sp.officer.as_deref()?)
}

fn spawn_unit(
    pack: &Pack,
    id: UnitId,
    sp: &UnitSpawn,
    campaign: &CampaignState,
) -> Result<Unit, BattleError> {
    let who = sp
        .tag
        .clone()
        .or_else(|| sp.officer.clone())
        .or_else(|| sp.name.clone())
        .unwrap_or_else(|| format!("unit at {:?}", sp.pos));
    if let Some(item) = sp.drop.as_deref().filter(|i| pack.item(i).is_none()) {
        return Err(setup_err(format!(
            "unit `{who}` drops unknown item `{item}`"
        )));
    }
    // An army officer fights as they are in the army: the spawn's class, level, stats and
    // equipment are ignored. The spawn still decides where and how the unit fights.
    let mut unit = match army_officer(campaign, sp) {
        Some(state) => officer_unit(pack, id, state, sp.pos)?,
        None => defined_unit(pack, id, sp, &who)?,
    };
    unit.ai = sp.ai;
    unit.ai_target = sp.ai_target.clone();
    unit.ai_pos = match (sp.ai_pos, sp.ai) {
        (Some(p), _) => Some(p),
        (None, AiMode::Guard) => Some(sp.pos),
        (None, _) => None,
    };
    unit.commander = sp.commander;
    unit.tag = sp.tag.clone();
    unit.group = sp.group.clone();
    unit.state = if sp.group.is_some() {
        UnitState::Hidden
    } else {
        UnitState::Active
    };
    unit.drop = sp.drop.clone();
    Ok(unit)
}

/// A spawned unit as the battle defines it: a named officer from `officers.toml` (with the
/// spawn's overrides) or a generic unit. [`spawn_unit`] sets the behaviour fields.
fn defined_unit(pack: &Pack, id: UnitId, sp: &UnitSpawn, who: &str) -> Result<Unit, BattleError> {
    let (name, class_id, level, stats, equip, portrait, lord) = match &sp.officer {
        Some(oid) => {
            let od = pack
                .officer(oid)
                .ok_or_else(|| setup_err(format!("spawn `{who}` names unknown officer `{oid}`")))?;
            (
                od.name.clone(),
                sp.class.clone().unwrap_or_else(|| od.class.clone()),
                sp.level.unwrap_or(od.level),
                [od.strength, od.int, od.lead],
                sp.equip.clone().unwrap_or_else(|| od.equip.clone()),
                Some(od.portrait_key().to_string()),
                od.lord && sp.side == Side::Player,
            )
        }
        None => {
            let class_id = sp
                .class
                .clone()
                .ok_or_else(|| setup_err(format!("generic unit `{who}` needs a class")))?;
            let class = pack
                .class(&class_id)
                .ok_or_else(|| setup_err(format!("unit `{who}` has unknown class `{class_id}`")))?;
            let level = sp
                .level
                .ok_or_else(|| setup_err(format!("generic unit `{who}` needs a level")))?;
            (
                sp.name.clone().unwrap_or_else(|| class.name.clone()),
                class_id,
                level,
                sp.stats.unwrap_or(class.generic),
                sp.equip.clone().unwrap_or_default(),
                None,
                false,
            )
        }
    };
    let class = pack
        .class(&class_id)
        .ok_or_else(|| setup_err(format!("unit `{who}` has unknown class `{class_id}`")))?;
    check_equipment(pack, who, &equip)?;
    let level = level.max(1);
    let [strength, int, lead] = stats;
    let hp = max_hp(class, level);
    let mp = max_mp(&pack.rules, level, int);
    Ok(Unit {
        id,
        side: sp.side,
        officer: sp.officer.clone(),
        name,
        class: class_id,
        level,
        exp: 0,
        strength,
        int,
        lead,
        hp,
        max_hp: hp,
        mp,
        max_mp: mp,
        morale: pack.rules.morale_start.clamp(0, 100),
        pos: sp.pos,
        facing: Dir::default(),
        moved: false,
        acted: false,
        equip,
        statuses: Vec::new(),
        ai: AiMode::default(),
        ai_target: None,
        ai_pos: None,
        commander: false,
        lord,
        tag: None,
        group: None,
        state: UnitState::Active,
        portrait,
        drop: None,
    })
}
