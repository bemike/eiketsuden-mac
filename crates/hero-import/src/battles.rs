//! The **original battles** of the original mode: the base pack's battles re-staged the way the
//! original stages them, on the original battle maps.
//!
//! The base pack's prologue and chapter 1 retell the original's battles ([`ORIGINAL_BATTLES`] pairs
//! each base battle with the original battle it follows). For each pair the converter keeps the
//! base battle's story (name, objective text, intro and outro scenes, music, rewards, the events
//! that still make sense) and takes from the original:
//!
//! * the map: `[map] use = "hexz_NN"`, the converted `HEXZMAP` entry the scenario loads
//!   (`load_map` `0x3NNN`, FORMATS §13.3);
//! * the turn limit (header of `battle_setup`, `0x03`);
//! * the deployment tiles: the player slots of `battle_setup`, in order (slots that need a flag are
//!   left out). A named officer of the setup whom the base battle has as an allied or guest unit
//!   keeps that side at the original tile; one the setup marks as AI-controlled (third unknown
//!   byte 1: Zhao Yun at Jieqiao, Cao Cao's officers at Xiapi) becomes an allied unit;
//! * the units: the enemy and allied rosters (`battle_roster`, `0x22`) loaded with the map —
//!   officer, tile, class, level and AI; where the scenario picks a roster with `if_flags`, the
//!   one for the route of the base battle ([`Pairing::flags`]). A `BAKDATA` person with a base-pack
//!   officer of the same name plays as that officer; any other person as a generic unit with the
//!   `BAKDATA` name. A base unit of the same officer lends its `tag`, `drop`, `equip` and
//!   `commander`. Units the roster keeps off the map (second unknown byte 1; the original's events
//!   bring them in with `join_battle`) are left out until those events are converted;
//! * the treasures: trigger records `unit_at_cell` (kind 6) for any unit whose script gives gold
//!   (`data` kind 2) or an item;
//! * a `reach` victory or bonus condition of Liu Bei: the tile of the `unit_at_cell` record for
//!   Liu Bei whose script runs the battle routine (`data` kind 4) — the original's "Liu Bei reaches
//!   the gate" objectives.
//!
//! What cannot follow is left out and listed as a note: base units whose officer the original
//! roster does not have, events and conditions that name them or a tile of the base map, and
//! reinforcement groups. The original's own mid-battle events (the scenario's trigger records) are
//! not converted yet.
//!
//! # Mapping rules
//!
//! * **Which setup.** A battle's `battle_setup` precedes its rosters in the scene; where a scene
//!   offers two battles (a choice), the right one is the latest setup whose victory officer is in
//!   the battle's enemy roster, else the latest without a victory officer ([`find_battle`]).
//! * **Coordinates.** In rosters and setups the first coordinate byte is the column: every tile
//!   lies inside the map only this way round (verified on all 19 battles of the prologue and
//!   chapter 1). Two-byte AI tile parameters hold the column in the low byte. Trigger records
//!   store **row, column** (record bytes 4 and 5): read that way, every tile whose script gives
//!   gold or an item is a granary or treasury on the map, and the "reach" objectives are the
//!   granary (Jieqiao) and forts (Julu, Huainan) their objective texts name.
//! * **AI** ([`ai_mode`]): 1 attacks (Lü Bu at Hulao switches from 2 to 1 on turn 18, when he
//!   leaves the gate), 2 holds its ground (`guard`), 3 and 5 go for an officer (`target`), 4 and 6
//!   head for a tile (`advance`), 0 — the most common — waits until approached (`defensive`). The
//!   meaning of 0, 5 and 6 is inferred **[추론]**.
//! * **Cast.** Where the base pack gives an original person's part to another officer (the bandit
//!   chiefs Liu Bei wins over), [`Pairing::roles`] names the officer who plays it.
//! * **Classes** follow the game's class order ([`crate::pack::CLASS_SPRITES`]); **items** match
//!   the base pack's items by name.

use crate::bakdata::{Item, Officer};
use crate::scenario::{BattleHeader, Operands, RosterUnit, Scene};
use hero_core::battledef::{
    AiMode, BattleDef, Condition, EventAction, EventDef, MapDef, Side, TreasureDef, Trigger,
    UnitSpawn,
};
use hero_core::geom::Pos;
use std::collections::{BTreeMap, BTreeSet};

/// A base-pack battle and the original battle it follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    /// Base-pack battle id.
    pub battle: &'static str,
    /// `SNRnD.R3` file number.
    pub file: usize,
    pub scene: usize,
    /// Battle map number (`HEXZMAP.R3` entry).
    pub map: u8,
    /// Scenario flags set on the way to the base battle (the others count as clear), for rosters
    /// the scenario picks with `if_flags`.
    pub flags: &'static [u8],
    /// `(BAKDATA person, base officer id)`: the base battle gives this person's part to another
    /// officer (its story, events and campaign flags name that officer).
    pub roles: &'static [(u16, &'static str)],
}

const fn pair(battle: &'static str, file: usize, scene: usize, map: u8) -> Pairing {
    Pairing {
        battle,
        file,
        scene,
        map,
        flags: &[],
        roles: &[],
    }
}

/// Flag 133: set when Liu Bei chooses the road through Julu (`SNR1D` scene 0 block 10); Jieqiao
/// then gets the enemy roster for that route.
const JULU_ROUTE: u8 = 133;

/// Base-pack battles and the original battle each one follows.
pub const ORIGINAL_BATTLES: &[Pairing] = &[
    pair("p1_sishui", 0, 0, 0),
    pair("p2_hulao", 0, 0, 1),
    pair("c1_guangchuan", 1, 0, 2),
    pair("c1_xindu", 1, 0, 3),
    pair("c1_qinghe", 1, 0, 5),
    pair("c1_julu", 1, 0, 4),
    // Jieqiao after Julu (a) and after Qinghe (b): one battle, two enemy rosters.
    Pairing {
        flags: &[JULU_ROUTE],
        ..pair("c1_jieqiao_a", 1, 0, 6)
    },
    pair("c1_jieqiao_b", 1, 0, 6),
    pair("c1_beihai", 1, 1, 7),
    pair("c1_xuzhou1", 1, 1, 8),
    pair("c1_xiaopei", 1, 1, 9),
    // The bandit chiefs Liu Bei wins over: the base pack casts Chang Xi, Xia Kun and Shi Meng in
    // the parts of the original's 이명 (375), 조하 (223) and 동량 (228).
    Pairing {
        roles: &[(375, "chang_xi")],
        ..pair("c1_taishan", 1, 2, 10)
    },
    Pairing {
        roles: &[(223, "xia_kun")],
        ..pair("c1_pengcheng1", 1, 2, 12)
    },
    Pairing {
        roles: &[(228, "shi_meng")],
        ..pair("c1_xiaqiu1", 1, 2, 11)
    },
    pair("c1_huainan", 1, 2, 13),
    pair("c1_xiaqiu2", 1, 3, 11),
    pair("c1_pengcheng2", 1, 3, 12),
    // With and without Gao Shun: the same battle.
    pair("c1_xiapi", 1, 3, 14),
    pair("c1_xiapi_b", 1, 3, 14),
    pair("c1_guangling", 1, 4, 15),
    pair("c1_xuzhou2", 1, 4, 8),
];

/// Upper nibble of a `load_map` value that loads a battle map.
const BATTLE_MAP: u16 = 0x3000;
/// Trigger kind `unit_at_cell`.
const UNIT_AT_CELL: u8 = 6;
/// Person value of trigger records meaning "any unit".
const ANY_UNIT: u16 = 0x400;
/// `BAKDATA` person of Liu Bei.
const LIU_BEI: u16 = 0;
/// Person value of setup slots that any deployed officer may take.
const ANY_OFFICER: u16 = 0x400;
/// `data` kinds: add gold / run the battle routine.
const DATA_GOLD: u16 = 2;
const DATA_ROUTINE: u16 = 4;

/// A trigger record for a unit on a tile (`unit_at_cell`) and what its script gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRecord {
    /// `BAKDATA` person, or [`ANY_UNIT`].
    pub person: u16,
    pub x: u8,
    pub y: u8,
    pub gold: u16,
    pub item: Option<u8>,
    /// The script runs the battle routine (`data` kind 4): an objective tile.
    pub routine: bool,
}

/// A trigger record whose script brings units onto the map (`join_battle`, `0x1A`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRecord {
    /// Record number in the battle's block.
    pub record: usize,
    /// Trigger kind (0 = runs when its group's turn comes, right after the battle starts).
    pub kind: u8,
    pub inverted: bool,
    pub args: [u8; 6],
    /// `BAKDATA` persons it brings in.
    pub persons: Vec<u16>,
}

/// A battle of the original scenario.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalBattle {
    /// Block of the scene that loads the battle map.
    pub block: usize,
    pub map: u8,
    pub header: BattleHeader,
    /// Player slots of `battle_setup`.
    pub player: Vec<RosterUnit>,
    /// Rosters loaded with the map for the route: `(friendly, units)`.
    pub rosters: Vec<(bool, Vec<RosterUnit>)>,
    /// Rosters loaded with the map for another route (skipped by `if_flags`).
    pub other_route_rosters: usize,
    /// Rosters loaded later in the battle (reinforcements; not converted).
    pub later_rosters: usize,
    pub cells: Vec<CellRecord>,
    pub joins: Vec<JoinRecord>,
}

/// Find the battle of `scene` fought on battle map `map` with the scenario flags `flags` set (see
/// the module docs for which `battle_setup` and rosters belong to it).
pub fn find_battle(scene: &Scene, map: u8, flags: &[u8]) -> Result<OriginalBattle, String> {
    let wanted = BATTLE_MAP | u16::from(map);
    let is_load = |op: &Operands| op.get("map") == Some(wanted);
    let (block_index, record_index) = scene
        .blocks
        .iter()
        .enumerate()
        .find_map(|(b, block)| {
            block.records.iter().enumerate().find_map(|(r, rec)| {
                rec.code
                    .iter()
                    .any(|i| i.mnemonic == "load_map" && is_load(&i.operands))
                    .then_some((b, r))
            })
        })
        .ok_or_else(|| format!("no block loads battle map {map}"))?;
    let block = &scene.blocks[block_index];

    let (mut rosters, mut later_rosters, mut other_route_rosters) = (Vec::new(), 0, 0);
    for (r, rec) in block.records.iter().enumerate() {
        // `if_flags` guards: (instructions still guarded, the condition holds).
        let mut guards: Vec<(u8, bool)> = Vec::new();
        for instr in &rec.code {
            let runs = guards.iter().all(|&(_, holds)| holds);
            for g in guards.iter_mut() {
                g.0 -= 1;
            }
            guards.retain(|&(left, _)| left > 0);
            match &instr.operands {
                Operands::Condition {
                    skip,
                    all_set,
                    all_clear,
                } if *skip > 0 => {
                    let holds = all_set.iter().all(|f| flags.contains(f))
                        && all_clear.iter().all(|f| !flags.contains(f));
                    guards.push((*skip, holds));
                }
                Operands::Roster { friendly, units } if r == record_index => {
                    if runs {
                        rosters.push((*friendly, units.clone()));
                    } else {
                        other_route_rosters += 1;
                    }
                }
                Operands::Roster { .. } => later_rosters += 1,
                _ => {}
            }
        }
    }
    if rosters.is_empty() {
        return Err(format!(
            "block {block_index} loads battle map {map} without a roster"
        ));
    }
    let enemies: BTreeSet<u16> = rosters
        .iter()
        .filter(|(friendly, _)| !friendly)
        .flat_map(|(_, units)| units.iter().map(|u| u.person))
        .collect();

    let setups: Vec<(&BattleHeader, &Vec<RosterUnit>)> = scene.blocks[..=block_index]
        .iter()
        .flat_map(|b| b.records.iter().flat_map(|r| r.code.iter()))
        .filter_map(|i| match &i.operands {
            Operands::BattleSetup { header, units } => Some((header, units)),
            _ => None,
        })
        .collect();
    let (header, player) = setups
        .iter()
        .rev()
        .find(|(h, _)| h.defeat_to_win.is_some_and(|p| enemies.contains(&p)))
        .or_else(|| setups.iter().rev().find(|(h, _)| h.defeat_to_win.is_none()))
        .ok_or_else(|| {
            format!("no battle_setup before block {block_index} fits battle map {map}")
        })?;

    let cells = block
        .records
        .iter()
        .filter(|r| r.trigger.kind == UNIT_AT_CELL && !r.trigger.inverted)
        .map(|r| {
            let mut cell = CellRecord {
                person: r.trigger.word(0),
                // Row, then column.
                x: r.trigger.args[3],
                y: r.trigger.args[2],
                gold: 0,
                item: None,
                routine: false,
            };
            for instr in &r.code {
                match instr.mnemonic {
                    "data" => match (instr.operands.get("kind"), instr.operands.get("value")) {
                        (Some(DATA_GOLD), Some(v)) => cell.gold = cell.gold.saturating_add(v),
                        (Some(DATA_ROUTINE), _) => cell.routine = true,
                        _ => {}
                    },
                    "add_item" => cell.item = instr.operands.get("item").map(|v| v as u8),
                    _ => {}
                }
            }
            cell
        })
        .collect();

    let joins = block
        .records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let persons: Vec<u16> = r
                .code
                .iter()
                .filter(|c| c.mnemonic == "join_battle")
                .filter_map(|c| c.operands.get("person"))
                .collect();
            (!persons.is_empty()).then_some(JoinRecord {
                record: i,
                kind: r.trigger.kind,
                inverted: r.trigger.inverted,
                args: r.trigger.args,
                persons,
            })
        })
        .collect();

    Ok(OriginalBattle {
        block: block_index,
        map,
        header: (*header).clone(),
        player: (*player).clone(),
        rosters,
        other_route_rosters,
        later_rosters,
        cells,
        joins,
    })
}

/// The engine AI of an original AI mode and parameter: `(mode, target person, tile)`.
pub fn ai_mode(mode: u8, param: u16) -> (AiMode, Option<u16>, Option<Pos>) {
    let tile = || {
        Some(Pos::new(
            i32::from(param as u8),
            i32::from((param >> 8) as u8),
        ))
    };
    match mode {
        1 => (AiMode::Aggressive, None, None),
        2 => (AiMode::Guard, None, None),
        3 | 5 => (AiMode::Target, Some(param), None),
        4 | 6 => (AiMode::Advance, None, tile()),
        _ => (AiMode::Defensive, None, None),
    }
}

/// The engine trigger of an original trigger record, with `unit` naming a person (`None` for
/// [`ANY_UNIT`], any player unit). Kinds: 9 turn, 6 unit on a tile, 11 unit in a rectangle, 12 unit
/// defeated, 4 two units next to each other; tiles of trigger records are row first.
pub fn trigger_of(
    kind: u8,
    inverted: bool,
    args: [u8; 6],
    unit: &mut dyn FnMut(u16) -> Result<Option<String>, String>,
) -> Result<Trigger, String> {
    if inverted {
        return Err(format!("inverted trigger kind {kind} is not converted"));
    }
    let word = |i: usize| u16::from_le_bytes([args[2 * i], args[2 * i + 1]]);
    let tile = |row: usize| Pos::new(i32::from(args[row + 1]), i32::from(args[row]));
    let named = |p: u16, unit: &mut dyn FnMut(u16) -> Result<Option<String>, String>| {
        unit(p)?.ok_or_else(|| format!("trigger kind {kind} needs a unit, not any unit"))
    };
    Ok(match kind {
        9 => Trigger::TurnStart {
            turn: u32::from(word(0)),
            side: Side::Player,
        },
        6 => Trigger::Reach {
            who: unit(word(0))?,
            pos: tile(2),
            radius: 0,
            to: None,
        },
        11 => Trigger::Reach {
            who: unit(word(0))?,
            pos: tile(2),
            radius: 0,
            to: Some(tile(4)),
        },
        12 => Trigger::UnitDefeated {
            target: named(word(0), unit)?,
        },
        4 => Trigger::Adjacent {
            a: named(word(0), unit)?,
            b: named(word(1), unit)?,
        },
        other => return Err(format!("trigger kind {other} is not converted")),
    })
}

/// What the converter knows about the pack chain and the release.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// `BAKDATA` person → base-pack officer id.
    pub officers: BTreeMap<u16, String>,
    /// `BAKDATA` person → name (for generic units).
    pub person_names: BTreeMap<u16, String>,
    /// Original class number → base-pack class id.
    pub classes: BTreeMap<u8, String>,
    /// Original item number → base-pack item id.
    pub items: BTreeMap<u8, String>,
}

impl Names {
    /// Build the lookups. `officer_of` gives the `BAKDATA` records a base officer matches.
    pub fn new(
        people: &[Officer],
        items: &[Item],
        officer_of: impl Fn(&Officer) -> Option<String>,
        class_sprites: &[&str],
        base_classes: &[(String, String)],
        base_items: &[(String, String)],
    ) -> Names {
        let officers = people
            .iter()
            .filter_map(|o| Some((o.index as u16, officer_of(o)?)))
            .collect();
        let person_names = people
            .iter()
            .map(|o| (o.index as u16, o.name.clone()))
            .collect();
        let classes = class_sprites
            .iter()
            .enumerate()
            .filter_map(|(i, sprite)| {
                let (id, _) = base_classes.iter().find(|(_, s)| s == sprite)?;
                Some((i as u8, id.clone()))
            })
            .collect();
        let items = items
            .iter()
            .filter_map(|it| {
                let mut ids = base_items.iter().filter(|(_, name)| *name == it.name);
                match (ids.next(), ids.next()) {
                    (Some((id, _)), None) => Some((it.index as u8, id.clone())),
                    _ => None,
                }
            })
            .collect();
        Names {
            officers,
            person_names,
            classes,
            items,
        }
    }

    fn person_label(&self, person: u16) -> String {
        match self.person_names.get(&person) {
            Some(name) => format!("{name} ({person})"),
            None => format!("person {person}"),
        }
    }
}

/// A converted battle and what did not carry over.
#[derive(Debug, Clone)]
pub struct Converted {
    pub battle: BattleDef,
    pub notes: Vec<String>,
}

/// Every unit reference of a condition.
fn condition_refs(c: &Condition) -> Vec<&str> {
    match c {
        Condition::DefeatUnit { target } | Condition::UnitRetreated { target } => vec![target],
        Condition::Reach { who, .. } => who.iter().map(String::as_str).collect(),
        Condition::DefeatAll | Condition::DefeatCommander | Condition::SurviveTurns { .. } => {
            Vec::new()
        }
    }
}

/// Why an event cannot follow onto the original map, if it cannot.
fn event_problem(e: &EventDef, gone: &BTreeSet<String>) -> Option<String> {
    let mut refs: Vec<&str> = Vec::new();
    match &e.trigger {
        Trigger::Reach { .. } => return Some("it fires on a tile of the base map".into()),
        Trigger::UnitDefeated { target } | Trigger::HpBelow { target, .. } => refs.push(target),
        Trigger::Adjacent { a, b } => refs.extend([a.as_str(), b.as_str()]),
        Trigger::TurnStart { .. } => {}
    }
    for action in &e.actions {
        match action {
            EventAction::Spawn { group } => {
                return Some(format!(
                    "it brings in reinforcement group `{group}` of the base map"
                ))
            }
            EventAction::SetAi {
                ai_pos: Some(_), ..
            } => return Some("it sends a unit to a tile of the base map".into()),
            EventAction::SetAi {
                target, ai_target, ..
            } => {
                refs.push(target);
                refs.extend(ai_target.iter().map(String::as_str));
            }
            EventAction::Retreat { target } | EventAction::LevelUp { target, .. } => {
                refs.push(target)
            }
            _ => {}
        }
    }
    refs.into_iter()
        .find(|r| gone.contains(*r))
        .map(|r| format!("it names `{r}`, who is not in the original roster"))
}

/// Re-stage `base` as the original battle `orig` on the map `map_id`; `roles` as in
/// [`Pairing::roles`].
pub fn convert(
    base: &BattleDef,
    orig: &OriginalBattle,
    names: &Names,
    roles: &[(u16, &str)],
    map_id: &str,
) -> Result<Converted, String> {
    let mut notes = Vec::new();
    let mut battle = base.clone();
    battle.map = MapDef {
        use_map: Some(map_id.to_string()),
        ..MapDef::default()
    };
    battle.turn_limit = u32::from(orig.header.turn_limit);
    let officer_ref = |person: u16| {
        roles
            .iter()
            .find(|(p, _)| *p == person)
            .map(|(_, id)| id.to_string())
            .or_else(|| names.officers.get(&person).cloned())
    };

    // When each person comes onto the map: with the first record that brings it in; `None` = at
    // the start (a `run` record right after the battle begins).
    let mut arrival: BTreeMap<u16, Option<String>> = BTreeMap::new();
    for j in &orig.joins {
        let group = (j.kind != 0).then(|| arrival_group(j.record));
        for &p in &j.persons {
            arrival.entry(p).or_insert_with(|| group.clone());
        }
    }
    let mut never_arrive = 0;

    // Units, and the person of each (for references by trigger records).
    let mut units: Vec<UnitSpawn> = Vec::new();
    let mut persons: Vec<u16> = Vec::new();
    let mut placed: BTreeSet<String> = BTreeSet::new();

    // Deployment. Named officers of the setup whom the base battle has on its side, and those
    // the setup keeps back until an event brings them in, become units; every other slot is a
    // deploy tile.
    let mut slots = Vec::new();
    let mut conditional = 0;
    for u in &orig.player {
        if u.requires_flag.is_some() {
            conditional += 1;
            continue;
        }
        let pos = Pos::new(i32::from(u.x), i32::from(u.y));
        let officer = (u.person != LIU_BEI && u.person != ANY_OFFICER)
            .then(|| officer_ref(u.person))
            .flatten();
        let joins_later = u.other.get(2) == Some(&1);
        let group = if joins_later {
            match arrival.get(&u.person) {
                Some(g) => g.clone(),
                None => {
                    never_arrive += 1;
                    continue;
                }
            }
        } else {
            None
        };
        let guest = officer.as_ref().and_then(|id| {
            base.units
                .iter()
                .find(|b| b.officer.as_deref() == Some(id) && b.side != Side::Enemy)
        });
        match (officer, guest) {
            (Some(id), Some(b)) => {
                units.push(UnitSpawn {
                    pos,
                    group,
                    ..b.clone()
                });
                persons.push(u.person);
                placed.insert(id);
            }
            (Some(id), None) if joins_later => {
                units.push(UnitSpawn {
                    side: Side::Ally,
                    officer: Some(id.clone()),
                    name: None,
                    class: None,
                    level: None,
                    stats: None,
                    pos,
                    ai: AiMode::Aggressive,
                    ai_target: None,
                    ai_pos: None,
                    commander: false,
                    tag: None,
                    group,
                    equip: None,
                    drop: None,
                });
                persons.push(u.person);
                placed.insert(id);
            }
            (None, _) if joins_later => notes.push(format!(
                "{}: an officer who joins during the battle without a base-pack officer; left out",
                names.person_label(u.person)
            )),
            (officer, _) => slots.push((officer, u.person, pos)),
        }
    }
    if conditional > 0 {
        notes.push(format!(
            "{conditional} player slot(s) that need a campaign flag (officers who join for this \
             battle only) are left out"
        ));
    }
    if slots.is_empty() {
        return Err("the original battle has no player slot".into());
    }
    // The engine fills the tiles with the required officers first, then the lord: put the
    // original tiles of those officers first so everyone starts where the original has them.
    let mut ordered = Vec::with_capacity(slots.len());
    for id in &base.deploy.required {
        if let Some(i) = slots.iter().position(|(o, ..)| o.as_ref() == Some(id)) {
            ordered.push(slots.remove(i));
        }
    }
    if let Some(i) = slots.iter().position(|&(_, p, _)| p == LIU_BEI) {
        ordered.push(slots.remove(i));
    }
    ordered.append(&mut slots);
    let slots: Vec<Pos> = ordered.into_iter().map(|(_, _, pos)| pos).collect();
    if (battle.deploy.max as usize) > slots.len() {
        notes.push(format!(
            "deploy max {} lowered to the original's {} slots",
            battle.deploy.max,
            slots.len()
        ));
        battle.deploy.max = slots.len() as u32;
    }
    battle.deploy.slots = slots;

    // Enemy and allied rosters.
    for (friendly, roster) in &orig.rosters {
        let side = if *friendly { Side::Ally } else { Side::Enemy };
        for u in roster {
            if u.requires_flag.is_some() {
                notes.push(format!(
                    "{} needs a campaign flag in the original and is left out",
                    names.person_label(u.person)
                ));
                continue;
            }
            let group = if u.other.get(1) == Some(&1) {
                match arrival.get(&u.person) {
                    Some(g) => g.clone(),
                    None => {
                        never_arrive += 1;
                        continue;
                    }
                }
            } else {
                None
            };
            let Some(class) = u.class.and_then(|c| names.classes.get(&c)).cloned() else {
                notes.push(format!(
                    "{}: class {:?} has no base-pack class; left out",
                    names.person_label(u.person),
                    u.class
                ));
                continue;
            };
            let mut officer = officer_ref(u.person);
            if officer.as_ref().is_some_and(|o| placed.contains(o)) {
                // An officer appears at most once per battle; a second record plays generic.
                officer = None;
            }
            let (ai, target, ai_pos) = ai_mode(u.ai_mode.unwrap_or(0), u.ai_param.unwrap_or(0));
            let mut spawn = UnitSpawn {
                side,
                officer: officer.clone(),
                name: None,
                class: Some(class),
                level: u.level.map(u32::from),
                stats: None,
                pos: Pos::new(i32::from(u.x), i32::from(u.y)),
                ai,
                ai_target: None,
                ai_pos,
                commander: orig.header.defeat_to_win == Some(u.person),
                tag: None,
                group,
                equip: None,
                drop: None,
            };
            if spawn.officer.is_none() {
                spawn.name = Some(
                    names
                        .person_names
                        .get(&u.person)
                        .cloned()
                        .unwrap_or_else(|| "병사".to_string()),
                );
            }
            if let Some(t) = target {
                match officer_ref(t) {
                    Some(id) => spawn.ai_target = Some(id),
                    None => {
                        notes.push(format!(
                            "{}: AI target {} has no base-pack officer; attacks instead",
                            names.person_label(u.person),
                            names.person_label(t)
                        ));
                        spawn.ai = AiMode::Aggressive;
                    }
                }
            }
            if let Some(id) = &officer {
                if let Some(b) = base
                    .units
                    .iter()
                    .find(|b| b.officer.as_deref() == Some(id) && b.side == side)
                {
                    spawn.tag = b.tag.clone();
                    spawn.drop = b.drop.clone();
                    spawn.equip = b.equip.clone();
                    spawn.commander |= b.commander;
                }
                placed.insert(id.clone());
            }
            units.push(spawn);
            persons.push(u.person);
        }
    }
    if never_arrive > 0 {
        notes.push(format!(
            "{never_arrive} unit(s) the original keeps off the map and brings in only from other \
             events are left out"
        ));
    }
    if orig.other_route_rosters > 0 {
        notes.push(format!(
            "{} roster(s) for another route of the original are not used",
            orig.other_route_rosters
        ));
    }
    if orig.later_rosters > 0 {
        notes.push(format!(
            "{} roster(s) loaded later in the original battle are not converted yet",
            orig.later_rosters
        ));
    }

    // Base units that did not come along, and the references that name them.
    let kept: BTreeSet<&str> = units
        .iter()
        .flat_map(|u| u.officer.iter().chain(u.tag.iter()))
        .map(String::as_str)
        .collect();
    let mut gone = BTreeSet::new();
    for b in &base.units {
        let refs: Vec<&String> = b.officer.iter().chain(b.tag.iter()).collect();
        if refs.is_empty() || refs.iter().any(|r| kept.contains(r.as_str())) {
            // Generic base units are replaced by the original's rosters as a whole.
            continue;
        }
        notes.push(format!(
            "base unit `{}` is not in the original battle",
            refs[0]
        ));
        gone.extend(refs.into_iter().cloned());
    }

    let groups: BTreeSet<String> = units.iter().filter_map(|u| u.group.clone()).collect();

    // A person as a unit reference: its officer id, or a tag given to its (first) unit.
    let mut unit_ref = |p: u16| -> Result<Option<String>, String> {
        if p == ANY_UNIT {
            return Ok(None);
        }
        if let Some(id) = officer_ref(p) {
            return Ok(Some(id));
        }
        let i = persons
            .iter()
            .position(|&q| q == p)
            .ok_or_else(|| format!("{} is not on the map", names.person_label(p)))?;
        let tag = units[i].tag.get_or_insert_with(|| format!("person_{p}"));
        Ok(Some(tag.clone()))
    };

    // Conditions. A `reach` of Liu Bei (or of any player unit) takes the original objective tile.
    let objective = {
        let tiles: Vec<Pos> = orig
            .cells
            .iter()
            .filter(|c| c.routine && c.person == LIU_BEI)
            .map(|c| Pos::new(i32::from(c.x), i32::from(c.y)))
            .collect();
        (tiles.len() == 1).then(|| tiles[0])
    };
    let lord = officer_ref(LIU_BEI);
    let fix = |c: &Condition, notes: &mut Vec<String>, what: &str| -> Option<Condition> {
        if let Some(r) = condition_refs(c).into_iter().find(|r| gone.contains(*r)) {
            notes.push(format!(
                "{what} naming `{r}` dropped (not in the original battle)"
            ));
            return None;
        }
        match c {
            Condition::Reach { who, .. } if who.is_none() || who.as_deref() == lord.as_deref() => {
                match objective {
                    Some(pos) => Some(Condition::Reach {
                        who: who.clone(),
                        pos,
                        radius: 0,
                        to: None,
                    }),
                    None => {
                        notes.push(format!(
                            "{what} `reach` dropped: the original has no single objective tile"
                        ));
                        None
                    }
                }
            }
            Condition::Reach { .. } => {
                notes.push(format!(
                    "{what} `reach` of another unit dropped (base-map tile)"
                ));
                None
            }
            other => Some(other.clone()),
        }
    };
    battle.victory = base
        .victory
        .iter()
        .filter_map(|c| fix(c, &mut notes, "victory condition"))
        .collect();
    battle.defeat = base
        .defeat
        .iter()
        .filter_map(|c| fix(c, &mut notes, "defeat condition"))
        .collect();
    battle.bonus = base.bonus.as_ref().and_then(|b| {
        let condition = fix(&b.condition, &mut notes, "bonus condition")?;
        Some(hero_core::battledef::BonusDef {
            condition,
            ..b.clone()
        })
    });
    if battle.victory.is_empty() {
        battle.victory = match orig.header.defeat_to_win {
            None => vec![Condition::DefeatAll],
            Some(p) => match unit_ref(p) {
                Ok(Some(target)) => vec![Condition::DefeatUnit { target }],
                _ => vec![Condition::DefeatCommander],
            },
        };
        notes.push("victory taken from the original's battle header".into());
    }

    // Events: the base battle's that still fit, then the original's arrivals.
    battle.events = base
        .events
        .iter()
        .filter(|e| match event_problem(e, &gone) {
            Some(why) => {
                notes.push(format!("event on {:?} dropped: {why}", e.trigger));
                false
            }
            None => true,
        })
        .cloned()
        .collect();
    for j in orig.joins.iter().filter(|j| j.kind != 0) {
        let spawn: BTreeSet<&String> = j
            .persons
            .iter()
            .filter_map(|p| arrival.get(p).and_then(Option::as_ref))
            .filter(|g| groups.contains(*g))
            .collect();
        if spawn.is_empty() {
            continue;
        }
        match trigger_of(j.kind, j.inverted, j.args, &mut unit_ref) {
            Ok(trigger) => battle.events.push(EventDef {
                trigger,
                once: true,
                actions: spawn
                    .into_iter()
                    .map(|g| EventAction::Spawn { group: g.clone() })
                    .collect(),
            }),
            Err(e) => notes.push(format!(
                "units brought in by record {} never arrive: {e}",
                j.record
            )),
        }
    }
    battle.units = units;

    // Treasures.
    let mut treasures = Vec::new();
    for c in orig.cells.iter().filter(|c| c.person == ANY_UNIT) {
        let item = match c.item {
            Some(i) => match names.items.get(&i) {
                Some(id) => Some(id.clone()),
                None => {
                    notes.push(format!(
                        "treasure item {i} at ({}, {}) has no base-pack item",
                        c.x, c.y
                    ));
                    None
                }
            },
            None => None,
        };
        if item.is_none() && c.gold == 0 {
            continue;
        }
        treasures.push(TreasureDef {
            pos: Pos::new(i32::from(c.x), i32::from(c.y)),
            item,
            gold: i64::from(c.gold),
        });
    }
    battle.treasures = treasures;
    Ok(Converted { battle, notes })
}

/// Reinforcement group of the units a trigger record brings in.
fn arrival_group(record: usize) -> String {
    format!("original_{record}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Arg, ArgKind, Block, Instr, Record, Trigger as RecTrigger};

    fn instr(mnemonic: &'static str, operands: Operands) -> Instr {
        Instr {
            offset: 0,
            opcode: 0,
            mnemonic,
            operands,
        }
    }

    fn fields(mnemonic: &'static str, args: &[(&'static str, u16)]) -> Instr {
        instr(
            mnemonic,
            Operands::Fields {
                args: args
                    .iter()
                    .map(|&(name, value)| Arg {
                        name,
                        kind: ArgKind::Number,
                        value,
                    })
                    .collect(),
            },
        )
    }

    fn record(kind: u8, args: [u8; 6], code: Vec<Instr>) -> Record {
        Record {
            offset: 0,
            trigger: RecTrigger {
                kind,
                kind_name: "",
                inverted: false,
                group: 0,
                group_flag: false,
                args,
            },
            code_offset: 0,
            code,
        }
    }

    fn unit(person: u16, x: u8, y: u8) -> RosterUnit {
        RosterUnit {
            person,
            x,
            y,
            requires_flag: None,
            class: Some(0),
            level: Some(3),
            ai_mode: Some(0),
            ai_param: Some(0),
            other: Vec::new(),
        }
    }

    /// A roster unit kept off the map until an event brings it in.
    fn hidden(person: u16, x: u8, y: u8) -> RosterUnit {
        RosterUnit {
            other: vec![1, 1],
            ..unit(person, x, y)
        }
    }

    fn setup(win: Option<u16>, turns: u8, player: Vec<RosterUnit>) -> Instr {
        instr(
            "battle_setup",
            Operands::BattleSetup {
                header: BattleHeader {
                    turn_limit: turns,
                    defeat_to_win: win,
                    lose_if_defeated: Some(0),
                    other: [0; 4],
                },
                units: player,
            },
        )
    }

    fn roster(units: Vec<RosterUnit>) -> Instr {
        instr(
            "battle_roster",
            Operands::Roster {
                friendly: false,
                units,
            },
        )
    }

    /// Two battles offered by a choice (setups for both first), then the battle on map 2.
    fn scene() -> Scene {
        let choice = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    [0; 6],
                    vec![setup(
                        Some(54),
                        25,
                        vec![unit(ANY_OFFICER, 1, 1), unit(0, 2, 2), unit(1, 3, 3)],
                    )],
                ),
                record(
                    0,
                    [0; 6],
                    vec![setup(Some(105), 30, vec![unit(0, 2, 2), unit(1, 3, 2)])],
                ),
            ],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    [0; 6],
                    vec![
                        roster(vec![unit(54, 9, 4), unit(300, 8, 4), hidden(302, 2, 2)]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                record(
                    UNIT_AT_CELL,
                    [0, 4, 5, 6, 0, 0],
                    vec![fields("data", &[("kind", 2), ("value", 100)])],
                ),
                record(
                    UNIT_AT_CELL,
                    [0, 4, 7, 1, 0, 0],
                    vec![fields("add_item", &[("item", 30)])],
                ),
                record(
                    UNIT_AT_CELL,
                    [0, 0, 3, 1, 0, 0],
                    vec![fields("data", &[("kind", 4), ("value", 50)])],
                ),
                record(9, [5, 0, 0, 0, 0, 0], vec![roster(vec![unit(301, 1, 1)])]),
                // Liu Bei in rows 2..=4, columns 3..=5 brings in person 302.
                record(
                    11,
                    [0, 0, 2, 3, 4, 5],
                    vec![fields("join_battle", &[("person", 302)])],
                ),
            ],
        };
        Scene {
            blocks: vec![choice, battle],
        }
    }

    #[test]
    fn finds_the_setup_whose_target_is_in_the_roster() {
        let b = find_battle(&scene(), 2, &[]).unwrap();
        assert_eq!(
            (b.block, b.header.turn_limit),
            (1, 25),
            "the setup that targets 54"
        );
        assert_eq!(b.player.len(), 3);
        assert_eq!(b.rosters.len(), 1);
        assert_eq!(b.later_rosters, 1);
        assert_eq!(
            b.cells,
            [
                // Trigger records hold the row first.
                CellRecord {
                    person: ANY_UNIT,
                    x: 6,
                    y: 5,
                    gold: 100,
                    item: None,
                    routine: false
                },
                CellRecord {
                    person: ANY_UNIT,
                    x: 1,
                    y: 7,
                    gold: 0,
                    item: Some(30),
                    routine: false
                },
                CellRecord {
                    person: 0,
                    x: 1,
                    y: 3,
                    gold: 0,
                    item: None,
                    routine: true
                },
            ]
        );
        assert_eq!(b.joins.len(), 1);
        assert_eq!(
            (b.joins[0].record, b.joins[0].kind, &b.joins[0].persons[..]),
            (5, 11, &[302][..])
        );
        assert!(find_battle(&scene(), 3, &[]).is_err());
    }

    #[test]
    fn trigger_records() {
        let mut named = |p: u16| -> Result<Option<String>, String> {
            Ok((p != ANY_UNIT).then(|| format!("o{p}")))
        };
        assert_eq!(
            trigger_of(9, false, [7, 0, 0, 0, 0, 0], &mut named),
            Ok(Trigger::TurnStart {
                turn: 7,
                side: Side::Player
            })
        );
        assert_eq!(
            trigger_of(6, false, [0, 4, 12, 3, 0, 0], &mut named),
            Ok(Trigger::Reach {
                who: None,
                pos: Pos::new(3, 12),
                radius: 0,
                to: None
            })
        );
        assert_eq!(
            trigger_of(12, false, [89, 0, 0, 0, 0, 0], &mut named),
            Ok(Trigger::UnitDefeated {
                target: "o89".into()
            })
        );
        assert_eq!(
            trigger_of(4, false, [1, 0, 5, 0, 0, 0], &mut named),
            Ok(Trigger::Adjacent {
                a: "o1".into(),
                b: "o5".into()
            })
        );
        assert!(
            trigger_of(12, false, [0, 4, 0, 0, 0, 0], &mut named).is_err(),
            "needs a unit"
        );
        assert!(
            trigger_of(9, true, [7, 0, 0, 0, 0, 0], &mut named).is_err(),
            "inverted"
        );
        assert!(trigger_of(2, false, [0; 6], &mut named).is_err());
    }

    #[test]
    fn ai_modes() {
        assert_eq!(ai_mode(1, 0), (AiMode::Aggressive, None, None));
        assert_eq!(ai_mode(2, 0), (AiMode::Guard, None, None));
        assert_eq!(ai_mode(3, 7), (AiMode::Target, Some(7), None));
        assert_eq!(
            ai_mode(6, 0x0D16),
            (AiMode::Advance, None, Some(Pos::new(22, 13)))
        );
        assert_eq!(ai_mode(0, 0), (AiMode::Defensive, None, None));
    }

    fn base_battle() -> BattleDef {
        toml::from_str(
            r#"
id = "b"
name = "시험 전투"
objective = "적장을 물리쳐라"
turn_limit = 20
reward_gold = 100
intro = "b_intro"
victory = [{ type = "defeat_unit", target = "boss" }, { type = "reach", who = "liu_bei", pos = [1, 1] }]

[map]
rows = """
..
..
"""

[deploy]
max = 4
required = ["guan_yu"]
slots = [[0, 0], [1, 0], [0, 1], [1, 1]]

[[units]]
side = "enemy"
officer = "boss"
tag = "chief"
drop = "bean"
pos = [1, 1]

[[units]]
side = "enemy"
officer = "extra"
pos = [0, 1]

[[units]]
side = "enemy"
name = "복병"
class = "archer"
level = 2
group = "ambush"
pos = [0, 0]

[[events]]
trigger = { type = "adjacent", a = "guan_yu", b = "chief" }
actions = [{ type = "drama", scene = "duel" }]

[[events]]
trigger = { type = "unit_defeated", target = "extra" }
actions = [{ type = "drama", scene = "extra_falls" }]

[[events]]
trigger = { type = "turn_start", turn = 3 }
actions = [{ type = "spawn", group = "ambush" }]

[[treasures]]
pos = [0, 0]
item = "wine"
"#,
        )
        .unwrap()
    }

    fn names() -> Names {
        let mut n = Names::default();
        n.officers.insert(0, "liu_bei".into());
        n.officers.insert(1, "guan_yu".into());
        n.officers.insert(54, "boss".into());
        n.person_names.insert(300, "보병대".into());
        n.classes.insert(0, "short_infantry".into());
        n.items.insert(30, "bean".into());
        n
    }

    #[test]
    fn converts_onto_the_original_map() {
        let orig = find_battle(&scene(), 2, &[]).unwrap();
        let c = convert(&base_battle(), &orig, &names(), &[], "hexz_02").unwrap();
        let b = &c.battle;
        assert_eq!(b.map.use_map.as_deref(), Some("hexz_02"));
        assert!(!b.map.has_own_content());
        assert_eq!((b.turn_limit, b.intro.as_deref()), (25, Some("b_intro")));
        // Guan Yu (required) and Liu Bei first, as the engine fills the tiles.
        assert_eq!(
            b.deploy.slots,
            [Pos::new(3, 3), Pos::new(2, 2), Pos::new(1, 1)]
        );
        assert_eq!(b.deploy.max, 3, "lowered to the original slots");

        assert_eq!(b.units.len(), 3);
        let boss = &b.units[0];
        assert_eq!(boss.officer.as_deref(), Some("boss"));
        assert_eq!(
            (boss.tag.as_deref(), boss.drop.as_deref()),
            (Some("chief"), Some("bean"))
        );
        assert!(boss.commander, "the header's victory officer");
        assert_eq!(boss.pos, Pos::new(9, 4));
        assert_eq!(boss.class.as_deref(), Some("short_infantry"));
        let generic = &b.units[1];
        assert_eq!(
            (generic.officer.as_ref(), generic.name.as_deref()),
            (None, Some("보병대"))
        );
        assert_eq!(generic.ai, AiMode::Defensive);

        assert_eq!(
            b.victory,
            [
                Condition::DefeatUnit {
                    target: "boss".into()
                },
                Condition::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(1, 3),
                    radius: 0,
                    to: None,
                },
            ]
        );
        // The hidden unit waits for the original's area trigger.
        let late = &b.units[2];
        assert_eq!(late.group.as_deref(), Some("original_5"));
        assert_eq!(late.tag, None);
        // The duel stays; the event naming the missing officer and the ambush go; the arrival
        // follows the original's rectangle (rows 2..=4, columns 3..=5).
        assert_eq!(b.events.len(), 2);
        assert!(matches!(b.events[0].trigger, Trigger::Adjacent { .. }));
        assert_eq!(
            b.events[1],
            EventDef {
                trigger: Trigger::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(3, 2),
                    radius: 0,
                    to: Some(Pos::new(5, 4)),
                },
                once: true,
                actions: vec![EventAction::Spawn {
                    group: "original_5".into()
                }],
            }
        );
        assert_eq!(
            b.treasures,
            [
                TreasureDef {
                    pos: Pos::new(6, 5),
                    item: None,
                    gold: 100
                },
                TreasureDef {
                    pos: Pos::new(1, 7),
                    item: Some("bean".into()),
                    gold: 0
                },
            ]
        );
        let notes = c.notes.join("\n");
        assert!(
            notes.contains("base unit `extra` is not in the original battle"),
            "{notes}"
        );
        assert!(
            !notes.contains("복병"),
            "generic base units are not listed: {notes}"
        );
        assert!(notes.contains("roster(s) loaded later"), "{notes}");
        assert!(notes.contains("ambush"), "{notes}");
        // Round trip through the battle file format.
        let text = toml::to_string(b).unwrap();
        let back: BattleDef = toml::from_str(&text).unwrap();
        assert_eq!(&back, b);
    }
}
