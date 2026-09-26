//! Building the initial battle state ([`BattleState::new`]).
//!
//! Unit order is: deployed player officers (slot order), spawns that start on the map, then
//! reinforcements (definition order). AI units act in unit order, so reinforcements act after
//! the units that were on the map first (RULES.md §1.5).

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
    let deployed = deployment(pack, def, campaign)?;
    if deployed.len() > def.deploy.slots.len() {
        return Err(setup_err(format!(
            "{} officers deployed but battle `{battle}` has only {} deploy slots",
            deployed.len(),
            def.deploy.slots.len()
        )));
    }
    for (officer, &slot) in deployed.iter().zip(&def.deploy.slots) {
        let state = campaign
            .roster
            .iter()
            .find(|o| &o.id == officer)
            .ok_or_else(|| setup_err(format!("deployed officer `{officer}` is not in the army")))?;
        units.push(officer_unit(pack, units.len(), state, slot)?);
    }
    let on_map = def.units.iter().filter(|s| s.group.is_none());
    let reinforcements = def.units.iter().filter(|s| s.group.is_some());
    for spawn in on_map.chain(reinforcements) {
        units.push(spawn_unit(pack, units.len(), spawn)?);
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
        gold_found: 0,
        items_found: Vec::new(),
        flags: BTreeMap::new(),
    })
}

/// Officers taking part: the deploy screen's choice, or required officers, the lord and then
/// roster order up to `deploy.max` (skipping forbidden officers and officers not in the army).
fn deployment(
    pack: &Pack,
    def: &BattleDef,
    campaign: &CampaignState,
) -> Result<Vec<Id>, BattleError> {
    let in_army = |id: &str| campaign.roster.iter().any(|o| o.id == id);
    let forbidden = |id: &str| def.deploy.forbidden.iter().any(|f| f == id);
    let mut out: Vec<Id> = Vec::new();
    if !campaign.deployed.is_empty() {
        for id in &campaign.deployed {
            if !in_army(id) {
                return Err(setup_err(format!(
                    "deployed officer `{id}` is not in the army"
                )));
            }
            if forbidden(id) {
                return Err(setup_err(format!(
                    "officer `{id}` may not be deployed in battle `{}`",
                    def.id
                )));
            }
            if out.contains(id) {
                return Err(setup_err(format!("officer `{id}` is deployed twice")));
            }
            out.push(id.clone());
        }
        return Ok(out);
    }
    let allowed =
        |id: &str, out: &[Id]| in_army(id) && !forbidden(id) && !out.iter().any(|o| o == id);
    for id in &def.deploy.required {
        if allowed(id, &out) {
            out.push(id.clone());
        }
    }
    for o in &campaign.roster {
        if pack.officer(&o.id).is_some_and(|d| d.lord) && allowed(&o.id, &out) {
            out.push(o.id.clone());
        }
    }
    let max = (def.deploy.max as usize).min(def.deploy.slots.len());
    for o in &campaign.roster {
        if out.len() >= max {
            break;
        }
        if allowed(&o.id, &out) {
            out.push(o.id.clone());
        }
    }
    Ok(out)
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

fn spawn_unit(pack: &Pack, id: UnitId, sp: &UnitSpawn) -> Result<Unit, BattleError> {
    let who = sp
        .tag
        .clone()
        .or_else(|| sp.officer.clone())
        .or_else(|| sp.name.clone())
        .unwrap_or_else(|| format!("unit at {:?}", sp.pos));
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
    check_equipment(pack, &who, &equip)?;
    if let Some(item) = sp.drop.as_deref().filter(|i| pack.item(i).is_none()) {
        return Err(setup_err(format!(
            "unit `{who}` drops unknown item `{item}`"
        )));
    }
    let level = level.max(1);
    let [strength, int, lead] = stats;
    let hp = max_hp(class, level);
    let mp = max_mp(&pack.rules, level, int);
    let ai_pos = match (sp.ai_pos, sp.ai) {
        (Some(p), _) => Some(p),
        (None, AiMode::Guard) => Some(sp.pos),
        (None, _) => None,
    };
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
        ai: sp.ai,
        ai_target: sp.ai_target.clone(),
        ai_pos,
        commander: sp.commander,
        lord,
        tag: sp.tag.clone(),
        group: sp.group.clone(),
        state: if sp.group.is_some() {
            UnitState::Hidden
        } else {
            UnitState::Active
        },
        portrait,
        drop: sp.drop.clone(),
    })
}
