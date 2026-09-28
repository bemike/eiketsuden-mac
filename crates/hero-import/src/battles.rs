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
//! * a `reach` victory or bonus condition of Liu Bei: the tile (`unit_at_cell`) or rectangle
//!   (`unit_in_area`) of the record for Liu Bei whose script runs the battle routine (`data`
//!   kind 4) in the battle's first phase — the original's "Liu Bei reaches the gate" objectives;
//! * the AI the opening script gives (`set_ai` in group 2);
//! * the mid-battle events: every trigger record of the battle's phases ([`FIRST_PHASE_GROUP`]
//!   on) becomes an event ([`convert`]): its trigger ([`trigger_of`]), its dialogue as a drama
//!   scene written from the player's copy (duels included, as dialogue with sound), and its
//!   actions — units joining (`spawn`), AI changes, levels, retreats, gold and items, map cells
//!   that change (a gate opens, a drawbridge comes down: `set_terrain` with the new chips'
//!   picture) and the end of the battle. The original's trigger groups are phases (FORMATS
//!   §13.2): a parallel group watches all its records until one leaves parallel control, any
//!   other group runs the first record whose trigger holds; a battle with more than one phase
//!   gets `stage`s. Where the base battle keeps an event with the same trigger (the duels the
//!   base pack tells in its own words), the base event stays and the record is left out.
//!
//! What cannot follow is left out and listed as a note: base units whose officer the original
//! roster does not have, events and conditions that name them or a tile of the base map,
//! reinforcement groups, and the parts of the original's scripts the engine has no counterpart
//! for (music, mid-battle objective texts, campaign flags, changes of allegiance).
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
//! * **AI** ([`ai_mode`]): `MAIN.EXE` maps each mode to an AI routine and names them (FORMATS
//!   §13.4): 0 대기 waits until an enemy can be reached (`defensive`), 1 최단 적공격 attacks the
//!   nearest enemy (`aggressive`), 2 부동 never moves (`hold`), 3 and 4 이동 head for an officer
//!   or a tile and fight on the way (`target`, `advance`), 5 and 6 무공격이동 head there without
//!   attacking (`march`).
//! * **Cast.** Where the base pack gives an original person's part to another officer (the bandit
//!   chiefs Liu Bei wins over), [`Pairing::roles`] names the officer who plays it.
//! * **Classes** follow the game's class order ([`crate::pack::CLASS_SPRITES`]); **items** match
//!   the base pack's items by name.

use crate::bakdata::{Item, Officer};
use crate::scenario::{BattleHeader, Instr, Operands, Record, RosterUnit, Scene};
use hero_core::battledef::{
    AiMode, BattleDef, Condition, EventAction, EventDef, FlagCond, MapDef, Side, TreasureDef,
    Trigger, UnitSpawn,
};
use hero_core::geom::Pos;
use hero_core::script::Compare;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

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
    /// Trigger group: below [`FIRST_PHASE_GROUP`], the opening of the battle.
    pub group: u8,
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
    /// The trigger records of the battle's block.
    pub records: Vec<Record>,
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
                group: r.trigger.group,
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
        records: block.records.clone(),
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
        2 => (AiMode::Hold, None, None),
        3 => (AiMode::Target, Some(param), None),
        4 => (AiMode::Advance, None, tile()),
        5 => (AiMode::March, Some(param), None),
        6 => (AiMode::March, None, tile()),
        _ => (AiMode::Defensive, None, None),
    }
}

/// The AI of a unit whose target officer could not be resolved: `target` attacks the nearest
/// enemy instead, `march` (which never attacks) stays where it is.
fn without_target(ai: AiMode) -> AiMode {
    match ai {
        AiMode::March => AiMode::Hold,
        _ => AiMode::Aggressive,
    }
}

fn fallback_note(ai: AiMode) -> &'static str {
    match ai {
        AiMode::Hold => "holds its ground instead",
        _ => "attacks instead",
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
    /// Officers of the player's army in the pack chain (the campaign's starting officers and
    /// those that join in its scenes): events may name them in any battle.
    pub player_officers: BTreeSet<String>,
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
            player_officers: BTreeSet::new(),
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
    /// Drama scenes of the original's mid-battle events, as `.drama` text (empty without any).
    pub drama: String,
    pub notes: Vec<String>,
}

/// The text section of a scene (`SNRnM`), decoded.
pub trait TextSource {
    /// Lines `(speaker person, text)` of the dialogue at `offset`.
    fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String>;
    /// The string at `offset`.
    fn string(&self, offset: u16) -> Result<String, String>;
}

/// A cell after a map-cell operation (`set_map_chip`): its new terrain id and the key of its tile
/// picture, or `None` when the operation leaves the cell alone.
pub type CellChange = Option<(String, Option<String>)>;

/// What the event conversion needs beyond the scenario block.
pub struct EventSources<'a> {
    /// The text of the battle's scene.
    pub text: &'a dyn TextSource,
    /// The cell at a tile after a `set_map_chip` operation.
    pub cell_change: &'a mut dyn FnMut(Pos, u8) -> Result<CellChange, String>,
}

/// First trigger group of a battle block that plays during the battle: group 0 loads the map,
/// 1 starts the battle, 2 is the opening.
pub const FIRST_PHASE_GROUP: u8 = 3;
/// Trigger kinds: runs when its group's turn comes / the battle is won / lost / unit in an area.
const RUN: u8 = 0;
const BATTLE_WON: u8 = 7;
const BATTLE_LOST: u8 = 8;
const UNIT_IN_AREA: u8 = 11;
/// `duel_action` moves of a fighter who falls or flees.
const DUEL_FALLS: u16 = 3;
const DUEL_FLEES: u16 = 4;

/// A trigger group of a battle block from [`FIRST_PHASE_GROUP`] on (FORMATS §13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Phase {
    /// The group flag of its first record: all records are watched until one leaves parallel
    /// control; otherwise the first record whose trigger holds runs, then the next phase follows.
    parallel: bool,
    records: Vec<usize>,
    /// Only `run` records: the script after the battle (or between two phases).
    runs_only: bool,
    /// One of its scripts ends the battle (`battle_end`).
    ends_battle: bool,
}

/// The phases of a battle block, in order.
fn phases(records: &[Record]) -> Vec<Phase> {
    let mut out: Vec<(u8, Phase)> = Vec::new();
    for (i, r) in records.iter().enumerate() {
        let group = r.trigger.group;
        if group < FIRST_PHASE_GROUP {
            continue;
        }
        if out.last().is_none_or(|(g, _)| *g != group) {
            out.push((
                group,
                Phase {
                    parallel: r.trigger.group_flag,
                    records: Vec::new(),
                    runs_only: true,
                    ends_battle: false,
                },
            ));
        }
        let phase = &mut out.last_mut().expect("pushed above").1;
        phase.records.push(i);
        phase.runs_only &= r.trigger.kind == RUN;
        phase.ends_battle |= r.code.iter().any(|c| c.mnemonic == "battle_end");
    }
    out.into_iter().map(|(_, p)| p).collect()
}

/// What follows the end of phase `i`: victory, or the stage of the next phase to watch with the
/// `run` records to play on the way.
enum Next {
    Victory,
    Stage(u32, Vec<usize>),
}

fn next_after(phases: &[Phase], stages: &[Option<u32>], i: usize) -> Next {
    let mut on_the_way = Vec::new();
    for (j, p) in phases.iter().enumerate().skip(i + 1) {
        if !p.runs_only {
            return Next::Stage(stages[j].expect("watched phases have a stage"), on_the_way);
        }
        if p.ends_battle {
            return Next::Victory;
        }
        on_the_way.extend(&p.records);
    }
    Next::Victory
}

/// Whether two triggers fire on the same occasion: the same turn (either side's phase), the same
/// pair of adjacent units, the same unit defeated or the same area. Unit references must be
/// canonical ([`EventWriter::canonical`]).
fn same_occasion(a: &Trigger, b: &Trigger) -> bool {
    match (a, b) {
        (Trigger::TurnStart { turn: x, .. }, Trigger::TurnStart { turn: y, .. }) => x == y,
        (Trigger::Adjacent { a: a1, b: b1 }, Trigger::Adjacent { a: a2, b: b2 }) => {
            (a1 == a2 && b1 == b2) || (a1 == b2 && b1 == a2)
        }
        _ => a == b,
    }
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
            EventAction::SetTerrain { .. } => {
                return Some("it changes a tile of the base map".into())
            }
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

/// A drama speaker without an officer id: the name as the game shows it, without spaces
/// (drama speakers are one word of at most 24 characters, and one that looks like an id must be
/// an officer, which a free name is not).
fn free_speaker(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':')
        .take(24)
        .collect();
    let id_like = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if name.is_empty() || id_like {
        "???".to_string()
    } else {
        name
    }
}

/// Append `text` to a drama as `head` (`speaker:` or `@narr`) and indented continuation lines;
/// a line the drama parser would read as something else starts a new `head` line instead.
fn push_text(out: &mut String, head: &str, text: &str) {
    let mut lines = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty());
    let Some(first) = lines.next() else {
        return;
    };
    let _ = writeln!(out, "{head} {first}");
    for line in lines {
        if line.starts_with(['@', '-', '#']) || line.starts_with("==") {
            let _ = writeln!(out, "{head} {line}");
        } else {
            let _ = writeln!(out, "    {line}");
        }
    }
}

/// How a record's script ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptEnd {
    /// Ran to its end.
    Done,
    /// `leave_parallel`: the phase is over.
    LeavesPhase,
    /// `battle_end` (the battle is won) or `goto_block` (the scenario moves on).
    EndsBattle,
    /// A part guarded by flags other records set ended the script ([`Branch`]); what follows
    /// it runs only while the flags do not hold.
    Branched,
}

/// A part of a record's script that runs only while flags other records set hold (an `if_flags`
/// on shared flags): it becomes an event of its own with the record's trigger and these
/// conditions.
#[derive(Debug, Clone, PartialEq)]
struct Branch {
    when: Vec<FlagCond>,
    actions: Vec<EventAction>,
    end: ScriptEnd,
}

/// The converter of one battle's records into events and drama scenes.
struct EventWriter<'a, 'b> {
    battle_id: &'a str,
    names: &'a Names,
    roles: &'a [(u16, &'a str)],
    /// Scenario flags set for the route (`if_flags` holds for them).
    route: &'a [u8],
    /// Flags the block's scripts set: battle-local, clear when the battle starts.
    local_flags: BTreeSet<u8>,
    /// Battle-local flags one record sets and another tests: they become battle flags
    /// ([`EventWriter::flag_name`]) and event conditions.
    shared_flags: BTreeSet<u8>,
    /// Branches of the scripts converted since they were last taken.
    branches: Vec<Branch>,
    /// Drama scenes written per record, so every scene id stays unique.
    scenes: BTreeMap<usize, usize>,
    /// Scripts of `run` records played on the way between phases, converted once.
    on_the_way: BTreeMap<usize, (Vec<EventAction>, ScriptEnd)>,
    units: &'a mut Vec<UnitSpawn>,
    persons: &'a [u16],
    /// Person → reinforcement group of its unit (`None`: on the map from the start).
    arrival: &'a BTreeMap<u16, Option<String>>,
    groups: &'a BTreeSet<String>,
    sources: &'a mut EventSources<'b>,
    drama: String,
    notes: Vec<String>,
    /// Kinds of things left out, reported once per battle.
    skipped: BTreeSet<&'static str>,
}

impl EventWriter<'_, '_> {
    fn officer_ref(&self, person: u16) -> Option<String> {
        self.roles
            .iter()
            .find(|(p, _)| *p == person)
            .map(|(_, id)| id.to_string())
            .or_else(|| self.names.officers.get(&person).cloned())
    }

    /// A person as a unit reference: an officer on the map or in the player's army, else a tag
    /// given to the person's (first) unit on the map. `Ok(None)` is [`ANY_UNIT`].
    fn unit_ref(&mut self, person: u16) -> Result<Option<String>, String> {
        if person == ANY_UNIT {
            return Ok(None);
        }
        if let Some(id) = self.officer_ref(person) {
            if self.names.player_officers.contains(&id)
                || self.units.iter().any(|u| u.officer.as_deref() == Some(&id))
            {
                return Ok(Some(id));
            }
        }
        let i = self
            .persons
            .iter()
            .position(|&q| q == person)
            .ok_or_else(|| format!("{} is not on the map", self.names.person_label(person)))?;
        let unit = &mut self.units[i];
        let reference = unit
            .officer
            .clone()
            .or_else(|| unit.tag.clone())
            .unwrap_or_else(|| {
                let tag = format!("person_{person}");
                unit.tag = Some(tag.clone());
                tag
            });
        Ok(Some(reference))
    }

    /// `trigger` with each unit reference replaced by one that names its unit alone (`#<index>`
    /// for a unit of the battle), so references by tag and by officer id compare equal.
    fn canonical(&self, trigger: &Trigger) -> Trigger {
        let canon = |r: &String| {
            self.units
                .iter()
                .position(|u| u.officer.as_ref() == Some(r) || u.tag.as_ref() == Some(r))
                .map_or_else(|| r.clone(), |i| format!("#{i}"))
        };
        match trigger {
            Trigger::Adjacent { a, b } => Trigger::Adjacent {
                a: canon(a),
                b: canon(b),
            },
            Trigger::UnitDefeated { target } => Trigger::UnitDefeated {
                target: canon(target),
            },
            Trigger::HpBelow { target, pct } => Trigger::HpBelow {
                target: canon(target),
                pct: *pct,
            },
            Trigger::Reach {
                who,
                pos,
                radius,
                to,
            } => Trigger::Reach {
                who: who.as_ref().map(canon),
                pos: *pos,
                radius: *radius,
                to: *to,
            },
            other => other.clone(),
        }
    }

    /// A named unit reference (not [`ANY_UNIT`]).
    fn named(&mut self, person: u16) -> Result<String, String> {
        self.unit_ref(person)?
            .ok_or_else(|| "needs a unit, not any unit".to_string())
    }

    fn speaker(&self, person: u16) -> String {
        match self.officer_ref(person) {
            Some(id) => id,
            None => free_speaker(
                self.names
                    .person_names
                    .get(&person)
                    .map_or("???", String::as_str),
            ),
        }
    }

    /// The battle flag of a shared scenario flag.
    fn flag_name(&self, flag: u8) -> String {
        format!("orig_{}_{flag}", self.battle_id)
    }

    /// Whether an `if_flags` condition holds when the battle's events run: the route's flags are
    /// set (and the other route flags of [`ORIGINAL_BATTLES`] clear), the block's own flags
    /// clear; any other flag is taken as clear and noted.
    fn holds(&mut self, record: usize, all_set: &[u8], all_clear: &[u8]) -> bool {
        let value = |f: u8, this: &mut Self| {
            let known = this.local_flags.contains(&f)
                || ORIGINAL_BATTLES.iter().any(|p| p.flags.contains(&f));
            if !known {
                let note = format!(
                    "record {record}: scenario flag {f} is taken as clear (set outside the battle)"
                );
                if !this.notes.contains(&note) {
                    this.notes.push(note);
                }
            }
            this.route.contains(&f)
        };
        let mut ok = true;
        for &f in all_set {
            ok &= value(f, self);
        }
        for &f in all_clear {
            ok &= !value(f, self);
        }
        ok
    }

    /// Convert the script of record `record` into actions (with its text as drama scenes named
    /// after the record, unless `with_text` is off); returns how it ended. Parts guarded by
    /// shared flags go to [`EventWriter::branches`].
    fn script(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
        with_text: bool,
    ) -> ScriptEnd {
        self.script_part(record, code, actions, with_text, &[])
    }

    /// The script of a `run` record played on the way from one phase to the next, converted
    /// once and reused by every record that leaves the phase.
    fn on_the_way(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
    ) -> ScriptEnd {
        if let Some((cached, end)) = self.on_the_way.get(&record) {
            actions.extend(cached.iter().cloned());
            return *end;
        }
        let taken = std::mem::take(&mut self.branches);
        let mut converted = Vec::new();
        let end = self.script(record, code, &mut converted, true);
        if !std::mem::replace(&mut self.branches, taken).is_empty() {
            self.notes.push(format!(
                "record {record}: its flag-guarded parts are left out (it runs between phases)"
            ));
        }
        actions.extend(converted.iter().cloned());
        self.on_the_way.insert(record, (converted, end));
        end
    }

    /// [`EventWriter::script`] for a part of a script that runs while `when` holds.
    fn script_part(
        &mut self,
        record: usize,
        code: &[Instr],
        actions: &mut Vec<EventAction>,
        with_text: bool,
        when: &[FlagCond],
    ) -> ScriptEnd {
        let base_id = format!("orig_{}_{record}", self.battle_id);
        let mut scene = String::new();
        let flush = |scene: &mut String, actions: &mut Vec<EventAction>, this: &mut Self| {
            if scene.is_empty() {
                return;
            }
            let n = this.scenes.entry(record).or_insert(0);
            *n += 1;
            let id = if *n == 1 {
                base_id.clone()
            } else {
                format!("{base_id}_{n}")
            };
            let _ = write!(this.drama, "\n== {id}\n{scene}@hide all\n");
            scene.clear();
            actions.push(EventAction::Drama { scene: id });
        };
        let mut skip = 0u8;
        let mut after_levels = false;
        let mut end = ScriptEnd::Done;
        for (index, instr) in code.iter().enumerate() {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let get = |name: &str| instr.operands.get(name).unwrap_or(0);
            let was_levels = std::mem::take(&mut after_levels);
            let text = matches!(
                instr.mnemonic,
                "dialogue" | "narration" | "caption" | "duel" | "duel_action" | "duel_end"
            );
            if text && !with_text {
                continue;
            }
            match instr.mnemonic {
                "if_flags" => {
                    if let Operands::Condition {
                        skip: n,
                        all_set,
                        all_clear,
                    } = &instr.operands
                    {
                        // Shared flags become conditions of the event; the others are decided
                        // now.
                        let shared = |f: &&u8| self.shared_flags.contains(*f);
                        let (set_shared, set_now): (Vec<u8>, Vec<u8>) =
                            all_set.iter().partition(shared);
                        let (clear_shared, clear_now): (Vec<u8>, Vec<u8>) =
                            all_clear.iter().partition(shared);
                        if !self.holds(record, &set_now, &clear_now) {
                            skip = *n;
                        } else if !set_shared.is_empty() || !clear_shared.is_empty() {
                            // The guarded part becomes a branch with these conditions.
                            let mut cond = when.to_vec();
                            for (flags, cmp) in
                                [(set_shared, Compare::Ne), (clear_shared, Compare::Eq)]
                            {
                                for f in flags {
                                    let c = FlagCond {
                                        flag: self.flag_name(f),
                                        cmp,
                                        value: 0,
                                    };
                                    if !cond.contains(&c) {
                                        cond.push(c);
                                    }
                                }
                            }
                            flush(&mut scene, actions, self);
                            let guarded_to = (index + 1 + usize::from(*n)).min(code.len());
                            let mut guarded = Vec::new();
                            let branch_end = self.script_part(
                                record,
                                &code[index + 1..guarded_to],
                                &mut guarded,
                                with_text,
                                &cond,
                            );
                            self.branches.push(Branch {
                                when: cond,
                                actions: guarded,
                                end: branch_end,
                            });
                            skip = *n;
                            if branch_end != ScriptEnd::Done {
                                // What follows runs only while the flags do not hold.
                                if code[guarded_to..].iter().any(|c| c.mnemonic != "end") {
                                    self.notes.push(format!(
                                        "record {record}: what its script does while its flags \
                                         do not hold is left out"
                                    ));
                                }
                                end = ScriptEnd::Branched;
                                break;
                            }
                        }
                    }
                }
                "dialogue" => match self.sources.text.dialogue(get("text")) {
                    Ok(lines) => {
                        for (speaker, text) in lines {
                            let head = format!("{}:", self.speaker(speaker));
                            push_text(&mut scene, &head, &text);
                        }
                    }
                    Err(e) => self.notes.push(format!("record {record}: {e}")),
                },
                "narration" | "caption" => {
                    // The game shows level-ups itself.
                    if instr.mnemonic == "caption" && was_levels {
                        continue;
                    }
                    match self.sources.text.string(get("text")) {
                        Ok(text) => push_text(&mut scene, "@narr", &text),
                        Err(e) => self.notes.push(format!("record {record}: {e}")),
                    }
                }
                "duel" => {
                    for (person, slot) in [(get("first"), "left"), (get("second"), "right")] {
                        if let Some(id) = self.officer_ref(person) {
                            let _ = writeln!(scene, "@show {id} {slot}");
                        }
                    }
                }
                "duel_action" => {
                    let sound = match get("action") {
                        DUEL_FALLS | DUEL_FLEES => "retreat",
                        _ => "hit_heavy",
                    };
                    let _ = writeln!(scene, "@sfx {sound}\n@wait 300");
                }
                "duel_end" => scene.push_str("@hide all\n"),
                "add_levels" => {
                    flush(&mut scene, actions, self);
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::LevelUp {
                            target,
                            amount: u32::from(get("levels")),
                        }),
                        Err(e) => self.notes.push(format!("record {record}: level-up: {e}")),
                    }
                    after_levels = true;
                }
                "remove_person" => {
                    flush(&mut scene, actions, self);
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::Retreat { target }),
                        Err(e) => self.notes.push(format!("record {record}: retreat: {e}")),
                    }
                }
                "set_ai" => {
                    flush(&mut scene, actions, self);
                    let mode = get("mode") as u8;
                    let param = match mode {
                        4 | 6 => get("p1") | (get("p2") << 8),
                        _ => get("target"),
                    };
                    let (mut ai, target, ai_pos) = ai_mode(mode, param);
                    let mut ai_target = None;
                    if let Some(t) = target {
                        match self.unit_ref(t) {
                            Ok(Some(r)) => ai_target = Some(r),
                            _ => {
                                ai = without_target(ai);
                                self.notes.push(format!(
                                    "record {record}: AI target {} is not on the map; {}",
                                    self.names.person_label(t),
                                    fallback_note(ai)
                                ));
                            }
                        }
                    }
                    match self.named(get("person")) {
                        Ok(target) => actions.push(EventAction::SetAi {
                            target,
                            ai,
                            ai_target,
                            ai_pos,
                        }),
                        Err(e) => self.notes.push(format!("record {record}: AI change: {e}")),
                    }
                }
                "join_battle" => {
                    flush(&mut scene, actions, self);
                    let person = get("person");
                    match self.arrival.get(&person) {
                        Some(Some(group)) if self.groups.contains(group) => {
                            let spawn = EventAction::Spawn {
                                group: group.clone(),
                            };
                            if !actions.contains(&spawn) {
                                actions.push(spawn);
                            }
                        }
                        Some(_) => {}
                        None => self.notes.push(format!(
                            "record {record}: {} joins but has no unit in the battle",
                            self.names.person_label(person)
                        )),
                    }
                }
                "add_item" => {
                    flush(&mut scene, actions, self);
                    let item = get("item") as u8;
                    match self.names.items.get(&item) {
                        Some(id) => actions.push(EventAction::GiveItem { item: id.clone() }),
                        None => self.notes.push(format!(
                            "record {record}: item {item} has no base-pack item"
                        )),
                    }
                }
                "data" => {
                    if get("kind") == DATA_GOLD {
                        flush(&mut scene, actions, self);
                        actions.push(EventAction::GiveGold {
                            amount: i64::from(get("value")),
                        });
                    }
                }
                "set_map_chip" => {
                    flush(&mut scene, actions, self);
                    let pos = Pos::new(i32::from(get("x")), i32::from(get("y")));
                    match (self.sources.cell_change)(pos, get("chip") as u8) {
                        Ok(Some((terrain, image))) => actions.push(EventAction::SetTerrain {
                            pos,
                            terrain,
                            image,
                        }),
                        Ok(None) => self.notes.push(format!(
                            "record {record}: map cell ({}, {}): the operation does not apply \
                             to its terrain",
                            pos.x, pos.y
                        )),
                        Err(e) => self.notes.push(format!(
                            "record {record}: map cell ({}, {}): {e}",
                            pos.x, pos.y
                        )),
                    }
                }
                "set_flag" => {
                    let flag = get("flag") as u8;
                    if self.shared_flags.contains(&flag) {
                        flush(&mut scene, actions, self);
                        actions.push(EventAction::SetFlag {
                            flag: self.flag_name(flag),
                            value: i64::from(get("clear") == 0),
                        });
                    } else if !self.local_flags.contains(&flag) {
                        self.skipped.insert("campaign flags set during battles");
                    }
                }
                "leave_parallel" => {
                    end = ScriptEnd::LeavesPhase;
                    break;
                }
                "battle_end" | "goto_block" => {
                    end = ScriptEnd::EndsBattle;
                    break;
                }
                "set_objective" => {
                    self.skipped
                        .insert("objective texts changed during battles");
                }
                "set_country" | "set_allegiance" | "set_class" | "set_officer_bit"
                | "withdraw_unit" => {
                    self.notes.push(format!(
                        "record {record}: `{}` is not converted",
                        instr.mnemonic
                    ));
                }
                // Presentation the drama or the battle screen does on its own.
                _ => {}
            }
        }
        flush(&mut scene, actions, self);
        if end == ScriptEnd::EndsBattle {
            actions.push(EventAction::Victory);
        }
        end
    }
}

/// Re-stage `base` as the original battle `orig` of `pairing` on the map `map_id`, with the
/// original's mid-battle events read through `sources`.
pub fn convert(
    base: &BattleDef,
    orig: &OriginalBattle,
    names: &Names,
    pairing: &Pairing,
    map_id: &str,
    sources: &mut EventSources<'_>,
) -> Result<Converted, String> {
    let roles = pairing.roles;
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
    // the start (the opening brings it in right after the battle begins).
    let mut arrival: BTreeMap<u16, Option<String>> = BTreeMap::new();
    for j in &orig.joins {
        let group = (j.group >= FIRST_PHASE_GROUP).then(|| arrival_group(j.record));
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
                        spawn.ai = without_target(spawn.ai);
                        notes.push(format!(
                            "{}: AI target {} has no base-pack officer; {}",
                            names.person_label(u.person),
                            names.person_label(t),
                            fallback_note(spawn.ai)
                        ));
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

    // The opening's AI (group 2 and before).
    for rec in orig
        .records
        .iter()
        .filter(|r| r.trigger.group < FIRST_PHASE_GROUP)
    {
        for instr in rec.code.iter().filter(|i| i.mnemonic == "set_ai") {
            let get = |name: &str| instr.operands.get(name).unwrap_or(0);
            let mode = get("mode") as u8;
            let param = match mode {
                4 | 6 => get("p1") | (get("p2") << 8),
                _ => get("target"),
            };
            let (ai, target, ai_pos) = ai_mode(mode, param);
            let ai_target = target.and_then(officer_ref);
            for (u, _) in units
                .iter_mut()
                .zip(&persons)
                .filter(|(_, &p)| p == get("person"))
            {
                (u.ai, u.ai_target, u.ai_pos) = match (target, &ai_target) {
                    (Some(_), None) => (without_target(ai), None, None),
                    _ => (ai, ai_target.clone(), ai_pos),
                };
            }
        }
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
    let phases = phases(&orig.records);
    let mut stages: Vec<Option<u32>> = Vec::with_capacity(phases.len());
    let mut watched = 0u32;
    for p in &phases {
        stages.push((!p.runs_only).then(|| {
            watched += 1;
            watched - 1
        }));
    }
    let staged = watched > 1;
    let first_watched = stages.iter().position(Option::is_some);

    // Conditions. A `reach` of Liu Bei (or of any player unit) takes the original objective
    // area of the first phase; objectives of later phases become events of their stage.
    let routine = |r: &Record| {
        !r.trigger.inverted
            && matches!(r.trigger.kind, UNIT_AT_CELL | UNIT_IN_AREA)
            && r.trigger.word(0) == LIU_BEI
            && r.code
                .iter()
                .any(|c| c.mnemonic == "data" && c.operands.get("kind") == Some(DATA_ROUTINE))
    };
    let area = |r: &Record| {
        let a = r.trigger.args;
        let tile = |row: usize| Pos::new(i32::from(a[row + 1]), i32::from(a[row]));
        (tile(2), (r.trigger.kind == UNIT_IN_AREA).then(|| tile(4)))
    };
    let objective = {
        let areas: BTreeSet<(Pos, Option<Pos>)> = first_watched
            .map(|i| {
                phases[i]
                    .records
                    .iter()
                    .map(|&r| &orig.records[r])
                    .filter(|r| routine(r))
                    .map(area)
                    .collect()
            })
            .unwrap_or_default();
        (areas.len() == 1).then(|| areas.into_iter().next().expect("one area"))
    };
    // The objective area of a later phase, for a bonus (reaching it there wins the battle).
    let later_objective = {
        let areas: BTreeSet<(Pos, Option<Pos>)> = phases
            .iter()
            .enumerate()
            .filter(|(i, p)| !p.runs_only && Some(*i) != first_watched)
            .flat_map(|(_, p)| p.records.iter().map(|&r| &orig.records[r]))
            .filter(|r| routine(r))
            .map(area)
            .collect();
        (areas.len() == 1).then(|| areas.into_iter().next().expect("one area"))
    };
    let lord = officer_ref(LIU_BEI);
    let fix = |c: &Condition,
               notes: &mut Vec<String>,
               what: &str,
               objective: Option<(Pos, Option<Pos>)>|
     -> Option<Condition> {
        if let Some(r) = condition_refs(c).into_iter().find(|r| gone.contains(*r)) {
            notes.push(format!(
                "{what} naming `{r}` dropped (not in the original battle)"
            ));
            return None;
        }
        match c {
            Condition::Reach { who, .. } if who.is_none() || who.as_deref() == lord.as_deref() => {
                match objective {
                    Some((pos, to)) => Some(Condition::Reach {
                        who: who.clone(),
                        pos,
                        radius: 0,
                        to,
                    }),
                    None => {
                        notes.push(format!(
                            "{what} `reach` dropped: the original has no single objective area \
                             for it"
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
        .filter_map(|c| fix(c, &mut notes, "victory condition", objective))
        .collect();
    battle.defeat = base
        .defeat
        .iter()
        .filter_map(|c| fix(c, &mut notes, "defeat condition", objective))
        .collect();
    battle.bonus = base.bonus.as_ref().and_then(|b| {
        let condition = fix(
            &b.condition,
            &mut notes,
            "bonus condition",
            objective.or(later_objective),
        )?;
        Some(hero_core::battledef::BonusDef {
            condition,
            ..b.clone()
        })
    });

    // Events: the base battle's that still fit, then the original's.
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
    let base_events = battle.events.len();
    let flags_of = |r: &Record, tested: bool| -> BTreeSet<u8> {
        r.code
            .iter()
            .flat_map(|c| match (&c.operands, tested) {
                (
                    Operands::Condition {
                        all_set, all_clear, ..
                    },
                    true,
                ) => all_set.iter().chain(all_clear).copied().collect(),
                (_, false) if c.mnemonic == "set_flag" => c
                    .operands
                    .get("flag")
                    .map(|f| f as u8)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    };
    let local_flags: BTreeSet<u8> = orig
        .records
        .iter()
        .flat_map(|r| flags_of(r, false))
        .collect();
    let mut shared_flags = BTreeSet::new();
    for (a, ra) in orig.records.iter().enumerate() {
        for (b, rb) in orig.records.iter().enumerate() {
            if a != b {
                shared_flags.extend(flags_of(ra, true).intersection(&flags_of(rb, false)));
            }
        }
    }
    let mut writer = EventWriter {
        battle_id: &base.id,
        names,
        roles,
        route: pairing.flags,
        local_flags,
        shared_flags,
        branches: Vec::new(),
        scenes: BTreeMap::new(),
        on_the_way: BTreeMap::new(),
        units: &mut units,
        persons: &persons,
        arrival: &arrival,
        groups: &groups,
        sources,
        drama: String::new(),
        notes: Vec::new(),
        skipped: BTreeSet::new(),
    };
    // Where a script that ended with `end` in phase `i` moves the battle on: the actions to
    // add (victory, or the next stage and the scripts on the way), if it leaves the phase.
    let moves_on = |writer: &mut EventWriter, i: usize, parallel: bool, end: ScriptEnd| {
        let leaves = match end {
            ScriptEnd::LeavesPhase => true,
            ScriptEnd::Done => !parallel,
            ScriptEnd::EndsBattle | ScriptEnd::Branched => false,
        };
        if !leaves {
            return Vec::new();
        }
        match next_after(&phases, &stages, i) {
            Next::Victory => vec![EventAction::Victory],
            Next::Stage(next, on_the_way) => {
                let mut actions = vec![EventAction::SetStage { stage: next }];
                for w in on_the_way {
                    if writer.on_the_way(w, &orig.records[w].code, &mut actions)
                        == ScriptEnd::EndsBattle
                    {
                        break;
                    }
                }
                actions
            }
        }
    };
    let mut events = Vec::new();
    for (i, phase) in phases.iter().enumerate() {
        let Some(stage) = stages[i] else {
            continue; // run records: played on the way from one phase to the next
        };
        for &r in &phase.records {
            let rec = &orig.records[r];
            let t = &rec.trigger;
            match t.kind {
                BATTLE_WON | BATTLE_LOST => {
                    // The base battle's conditions and outro.
                    writer
                        .skipped
                        .insert("the original's victory and defeat scripts");
                    continue;
                }
                UNIT_AT_CELL if t.word(0) == ANY_UNIT && !routine(rec) => continue, // treasures
                _ => {}
            }
            if Some(i) == first_watched && routine(rec) {
                continue; // the objective condition
            }
            let trigger = if t.kind == RUN {
                if stage != 0 {
                    notes.push(format!(
                        "record {r}: a script at the start of a later phase is not converted"
                    ));
                    continue;
                }
                Trigger::TurnStart {
                    turn: 1,
                    side: Side::Player,
                }
            } else {
                match trigger_of(t.kind, t.inverted, t.args, &mut |p| writer.unit_ref(p)) {
                    Ok(trigger) => trigger,
                    Err(e) => {
                        notes.push(format!("record {r} is not converted: {e}"));
                        continue;
                    }
                }
            };
            let occasion = writer.canonical(&trigger);
            let kept = battle.events[..base_events].iter().position(|e| {
                same_occasion(&writer.canonical(&e.trigger), &occasion)
                    // A base event the record of another phase took over stays with that one.
                    && !(staged && e.stage.is_some_and(|s| s != stage))
            });
            if let Some(k) = kept {
                notes.push(format!(
                    "record {r}: the base battle's event on {:?} is kept instead",
                    battle.events[k].trigger
                ));
                // The base event tells it in its own words; the record's other actions (and
                // where it moves the battle on) are added to it.
                let mut extra = Vec::new();
                let end = writer.script(r, &rec.code, &mut extra, false);
                if !std::mem::take(&mut writer.branches).is_empty() {
                    notes.push(format!(
                        "record {r}: its flag-guarded parts are not added to the base event"
                    ));
                }
                let moved = if battle.events[k].actions.contains(&EventAction::Victory) {
                    Vec::new()
                } else {
                    moves_on(&mut writer, i, phase.parallel, end)
                };
                let kept = &mut battle.events[k];
                let before = kept.actions.clone();
                let mut at = kept
                    .actions
                    .iter()
                    .position(|a| *a == EventAction::Victory)
                    .unwrap_or(kept.actions.len());
                for a in extra.into_iter().chain(moved) {
                    if a == EventAction::Victory {
                        if !kept.actions.contains(&a) {
                            kept.actions.push(a);
                        }
                    } else if !kept.actions.contains(&a) {
                        kept.actions.insert(at, a);
                        at += 1;
                    }
                }
                if end == ScriptEnd::EndsBattle && !kept.actions.contains(&EventAction::Victory) {
                    kept.actions.push(EventAction::Victory);
                }
                if staged && kept.actions != before {
                    // What the original adds happens in the record's phase only.
                    kept.stage = Some(stage);
                }
                continue;
            }
            let mut actions = Vec::new();
            let end = writer.script(r, &rec.code, &mut actions, true);
            let branches = std::mem::take(&mut writer.branches);
            let parts = std::iter::once(Branch {
                when: Vec::new(),
                actions,
                end,
            })
            .chain(branches);
            for part in parts {
                let mut actions = part.actions;
                actions.extend(moves_on(&mut writer, i, phase.parallel, part.end));
                if actions.is_empty() {
                    continue;
                }
                events.push(EventDef {
                    trigger: trigger.clone(),
                    once: true,
                    stage: staged.then_some(stage),
                    when: part.when,
                    actions,
                });
            }
        }
    }
    let drama = std::mem::take(&mut writer.drama);
    notes.append(&mut writer.notes);
    for what in &writer.skipped {
        notes.push(format!("not converted: {what}"));
    }
    battle.events.extend(events);
    if battle.victory.is_empty()
        && !battle
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::Victory) && e.stage.is_none_or(|s| s == 0))
    {
        battle.victory = match orig.header.defeat_to_win {
            None => vec![Condition::DefeatAll],
            Some(p) => match officer_ref(p)
                .filter(|id| units.iter().any(|u| u.officer.as_deref() == Some(id)))
            {
                Some(target) => vec![Condition::DefeatUnit { target }],
                None => vec![Condition::DefeatCommander],
            },
        };
        notes.push("victory taken from the original's battle header".into());
    }

    // Units that wait for a group no event brings in never appear.
    let spawned: BTreeSet<&str> = battle
        .events
        .iter()
        .flat_map(|e| &e.actions)
        .filter_map(|a| match a {
            EventAction::Spawn { group } => Some(group.as_str()),
            _ => None,
        })
        .collect();
    let before = units.len();
    units.retain(|u| u.group.as_deref().is_none_or(|g| spawned.contains(g)));
    if units.len() < before {
        notes.push(format!(
            "{} unit(s) brought in only by records that are not converted are left out",
            before - units.len()
        ));
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
    Ok(Converted {
        battle,
        drama,
        notes,
    })
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

    fn op(mnemonic: &'static str) -> Instr {
        fields(mnemonic, &[])
    }

    /// A trigger record of `group`; `flag` is the group flag (parallel control).
    fn record(kind: u8, group: u8, flag: bool, args: [u8; 6], code: Vec<Instr>) -> Record {
        Record {
            offset: 0,
            trigger: RecTrigger {
                kind,
                kind_name: "",
                inverted: false,
                group,
                group_flag: flag,
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

    /// Two battles offered by a choice (setups for both first), then the battle on map 2 in two
    /// phases: group 3 (parallel) until person 300 falls, group 4 (parallel), group 5 after it.
    fn scene() -> Scene {
        let choice = Block {
            offset: 0,
            records: vec![
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![setup(
                        Some(54),
                        25,
                        vec![unit(ANY_OFFICER, 1, 1), unit(0, 2, 2), unit(1, 3, 3)],
                    )],
                ),
                record(
                    0,
                    1,
                    false,
                    [0; 6],
                    vec![setup(Some(105), 30, vec![unit(0, 2, 2), unit(1, 3, 2)])],
                ),
            ],
        };
        let battle = Block {
            offset: 0,
            records: vec![
                // 0: the map and the enemies.
                record(
                    0,
                    0,
                    false,
                    [0; 6],
                    vec![
                        roster(vec![
                            unit(54, 9, 4),
                            unit(300, 8, 4),
                            hidden(302, 2, 2),
                            hidden(303, 5, 5),
                            unit(304, 7, 7),
                        ]),
                        fields("load_map", &[("map", 0x3002)]),
                    ],
                ),
                // 1: the battle starts.
                record(0, 1, false, [0; 6], vec![op("begin_battle")]),
                // 2: the opening makes person 304 hold its ground.
                record(
                    0,
                    2,
                    false,
                    [0; 6],
                    vec![fields(
                        "set_ai",
                        &[("person", 304), ("mode", 2), ("unused", 0)],
                    )],
                ),
                // 3–5: treasures and Liu Bei's objective tile.
                record(
                    UNIT_AT_CELL,
                    3,
                    true,
                    [0, 4, 5, 6, 0, 0],
                    vec![fields("data", &[("kind", 2), ("value", 100)])],
                ),
                record(
                    UNIT_AT_CELL,
                    3,
                    false,
                    [0, 4, 7, 1, 0, 0],
                    vec![fields("add_item", &[("item", 30)])],
                ),
                record(
                    UNIT_AT_CELL,
                    3,
                    false,
                    [0, 0, 3, 1, 0, 0],
                    vec![fields("data", &[("kind", 4), ("value", 50)])],
                ),
                // 6: a later roster (not converted).
                record(
                    9,
                    3,
                    false,
                    [5, 0, 0, 0, 0, 0],
                    vec![roster(vec![unit(301, 1, 1)])],
                ),
                // 7: Liu Bei in rows 2..=4, columns 3..=5 brings in person 302.
                record(
                    UNIT_IN_AREA,
                    3,
                    false,
                    [0, 0, 2, 3, 4, 5],
                    vec![fields("join_battle", &[("person", 302)])],
                ),
                // 8: Guan Yu's duel with 54, which the base battle tells itself.
                record(
                    4,
                    3,
                    false,
                    [1, 0, 54, 0, 0, 0],
                    vec![
                        fields("dialogue", &[("text", 0x10)]),
                        fields("duel", &[("first", 1), ("second", 54)]),
                        op("leave_parallel"),
                    ],
                ),
                // 9: 300 falls: the drawbridge comes down and the next phase begins.
                record(
                    12,
                    3,
                    false,
                    [44, 1, 0, 0, 0, 0],
                    vec![
                        fields("set_map_chip", &[("x", 3), ("y", 1), ("chip", 2)]),
                        fields("narration", &[("text", 0x30)]),
                        fields("set_flag", &[("flag", 90), ("clear", 0)]),
                        op("leave_parallel"),
                    ],
                ),
                // 10: turn 8 of the second phase: a duel, 54 heads for (2, 1), 303 arrives.
                record(
                    9,
                    4,
                    true,
                    [8, 0, 0, 0, 0, 0],
                    vec![
                        fields("dialogue", &[("text", 0x40)]),
                        fields("duel", &[("first", 1), ("second", 54)]),
                        fields("duel_action", &[("person", 1), ("action", 0)]),
                        fields("duel_action", &[("person", 54), ("action", 4)]),
                        op("duel_end"),
                        fields("add_levels", &[("person", 1), ("levels", 1)]),
                        fields("caption", &[("text", 0x50)]),
                        fields(
                            "set_ai",
                            &[("person", 54), ("mode", 4), ("p1", 2), ("p2", 1)],
                        ),
                        fields("join_battle", &[("person", 303)]),
                        fields("play_music", &[("song", 3)]),
                    ],
                ),
                // 11: the battle is won: the base battle's outro.
                record(7, 4, false, [0; 6], vec![op("leave_parallel")]),
                // 12: Liu Bei at (2, 1) in the second phase wins.
                record(
                    UNIT_AT_CELL,
                    4,
                    false,
                    [0, 0, 1, 2, 0, 0],
                    vec![
                        fields("data", &[("kind", 4), ("value", 50)]),
                        fields("battle_end", &[("next_map", 0x1000)]),
                    ],
                ),
                // 13: after the battle.
                record(
                    0,
                    5,
                    false,
                    [0; 6],
                    vec![
                        fields("data", &[("kind", 2), ("value", 500)]),
                        fields("battle_end", &[("next_map", 0x1000)]),
                    ],
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
        assert_eq!(b.records.len(), 14);
        assert_eq!(
            b.cells[..3],
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
        let joins: Vec<_> = b
            .joins
            .iter()
            .map(|j| (j.record, j.group, j.kind, &j.persons[..]))
            .collect();
        assert_eq!(
            joins,
            [(7, 3, UNIT_IN_AREA, &[302][..]), (10, 4, 9, &[303][..])]
        );
        assert!(find_battle(&scene(), 3, &[]).is_err());
    }

    #[test]
    fn phases_follow_the_trigger_groups() {
        let b = find_battle(&scene(), 2, &[]).unwrap();
        let p = phases(&b.records);
        assert_eq!(p.len(), 3);
        assert_eq!(
            (p[0].parallel, p[0].records.len(), p[0].runs_only),
            (true, 7, false)
        );
        assert_eq!(
            (p[1].parallel, &p[1].records[..], p[1].runs_only),
            (true, &[10, 11, 12][..], false)
        );
        assert!(p[2].runs_only && p[2].ends_battle);
        let stages = [Some(0), Some(1), None];
        assert!(matches!(next_after(&p, &stages, 0), Next::Stage(1, ref w) if w.is_empty()));
        assert!(matches!(next_after(&p, &stages, 1), Next::Victory));
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
        assert_eq!(ai_mode(2, 0), (AiMode::Hold, None, None));
        assert_eq!(ai_mode(3, 7), (AiMode::Target, Some(7), None));
        assert_eq!(
            ai_mode(4, 0x0D16),
            (AiMode::Advance, None, Some(Pos::new(22, 13)))
        );
        assert_eq!(ai_mode(5, 7), (AiMode::March, Some(7), None));
        assert_eq!(
            ai_mode(6, 0x0D16),
            (AiMode::March, None, Some(Pos::new(22, 13)))
        );
        assert_eq!(ai_mode(0, 0), (AiMode::Defensive, None, None));
    }

    #[test]
    fn drama_text_is_safe_for_the_parser() {
        let mut out = String::new();
        push_text(
            &mut out,
            "guan_yu:",
            "첫 줄\r\n  둘째 줄\n@명령 같은 줄\n\n- 목록 같은 줄",
        );
        push_text(&mut out, "@narr", "   ");
        assert_eq!(
            out,
            "guan_yu: 첫 줄\n    둘째 줄\nguan_yu: @명령 같은 줄\nguan_yu: - 목록 같은 줄\n"
        );
        let scenes = hero_core::script::parse_drama("t", &format!("== s\n{out}")).unwrap();
        assert_eq!(scenes[0].cmds.len(), 4, "three lines and the end");
        assert_eq!(free_speaker("공 손찬:"), "공손찬");
        assert_eq!(free_speaker("guard"), "???", "an id-like name is not free");
        assert_eq!(
            free_speaker("긴이름".repeat(10).as_str()).chars().count(),
            24
        );
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
        n.person_names.insert(9, "전령 갑".into());
        n.classes.insert(0, "short_infantry".into());
        n.items.insert(30, "bean".into());
        n.player_officers = ["liu_bei", "guan_yu"].map(String::from).into();
        n
    }

    /// The scene's text: dialogues and strings by offset.
    #[derive(Default)]
    struct Text {
        dialogues: BTreeMap<u16, Vec<(u16, String)>>,
        strings: BTreeMap<u16, String>,
    }

    impl TextSource for Text {
        fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String> {
            self.dialogues
                .get(&offset)
                .cloned()
                .ok_or_else(|| format!("no dialogue at {offset:#x}"))
        }

        fn string(&self, offset: u16) -> Result<String, String> {
            self.strings
                .get(&offset)
                .cloned()
                .ok_or_else(|| format!("no string at {offset:#x}"))
        }
    }

    fn text() -> Text {
        let mut t = Text::default();
        t.dialogues.insert(0x10, vec![(1, "결투다!".into())]);
        t.dialogues.insert(
            0x40,
            vec![
                (1, "첫 줄\n둘째 줄".into()),
                (54, "덤벼라".into()),
                (9, "큰일입니다".into()),
            ],
        );
        t.strings.insert(0x30, "다리가 내려왔다.".into());
        t.strings.insert(0x50, "관우는 레벨이 올라갔다!".into());
        t
    }

    fn converted() -> Converted {
        let orig = find_battle(&scene(), 2, &[]).unwrap();
        let text = text();
        let mut cells = |pos: Pos, op: u8| -> Result<CellChange, String> {
            assert_eq!(op, 2);
            Ok(Some((
                "bridge".to_string(),
                Some(format!("cell_{}_{}", pos.x, pos.y)),
            )))
        };
        convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut cells,
            },
        )
        .unwrap()
    }

    #[test]
    fn converts_onto_the_original_map() {
        let c = converted();
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

        assert_eq!(b.units.len(), 5);
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
            generic.tag.as_deref(),
            Some("person_300"),
            "named by an event"
        );
        // The hidden units wait for the records that bring them in.
        assert_eq!(b.units[2].group.as_deref(), Some("original_7"));
        assert_eq!(b.units[3].group.as_deref(), Some("original_10"));
        assert_eq!(
            b.units[4].ai,
            AiMode::Hold,
            "the opening's AI (mode 2, 부동)"
        );

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

    #[test]
    fn mid_battle_events_follow_the_phases() {
        let c = converted();
        let e = &c.battle.events;
        // The base duel stays (Guan Yu next to `chief`, the tag of officer `boss`), the base
        // events naming the missing officer or the base map's group go; record 8, the same duel,
        // is left out, and the base event takes over its end of the first phase.
        assert!(matches!(e[0].trigger, Trigger::Adjacent { .. }));
        assert_eq!(e[0].stage, Some(0));
        assert_eq!(
            e[0].actions,
            [
                EventAction::Drama {
                    scene: "duel".into()
                },
                EventAction::SetStage { stage: 1 }
            ]
        );
        let notes = c.notes.join("\n");
        assert!(
            notes.contains("record 8: the base battle's event"),
            "{notes}"
        );
        assert!(
            notes.contains("not converted: the original's victory and defeat scripts"),
            "{notes}"
        );
        let original: Vec<&EventDef> = e[1..].iter().collect();
        assert_eq!(original.len(), 4, "{original:#?}");
        // Record 7: Liu Bei in the rectangle brings in 302.
        assert_eq!(
            *original[0],
            EventDef {
                trigger: Trigger::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(3, 2),
                    radius: 0,
                    to: Some(Pos::new(5, 4)),
                },
                once: true,
                stage: Some(0),
                when: Vec::new(),
                actions: vec![EventAction::Spawn {
                    group: "original_7".into()
                }],
            }
        );
        // Record 9: the drawbridge, the narration, then the next phase.
        assert_eq!(
            *original[1],
            EventDef {
                trigger: Trigger::UnitDefeated {
                    target: "person_300".into()
                },
                once: true,
                stage: Some(0),
                when: Vec::new(),
                actions: vec![
                    EventAction::SetTerrain {
                        pos: Pos::new(3, 1),
                        terrain: "bridge".into(),
                        image: Some("cell_3_1".into()),
                    },
                    EventAction::Drama {
                        scene: "orig_b_9".into()
                    },
                    EventAction::SetStage { stage: 1 },
                ],
            }
        );
        // Record 10: dialogue and duel as a drama, then the level-up (its caption is the game's
        // own), the AI change and the arrival.
        assert_eq!(
            *original[2],
            EventDef {
                trigger: Trigger::TurnStart {
                    turn: 8,
                    side: Side::Player
                },
                once: true,
                stage: Some(1),
                when: Vec::new(),
                actions: vec![
                    EventAction::Drama {
                        scene: "orig_b_10".into()
                    },
                    EventAction::LevelUp {
                        target: "guan_yu".into(),
                        amount: 1
                    },
                    EventAction::SetAi {
                        target: "boss".into(),
                        ai: AiMode::Advance,
                        ai_target: None,
                        ai_pos: Some(Pos::new(2, 1)),
                    },
                    EventAction::Spawn {
                        group: "original_10".into()
                    },
                ],
            }
        );
        // Record 12: the second phase's objective wins the battle.
        assert_eq!(
            *original[3],
            EventDef {
                trigger: Trigger::Reach {
                    who: Some("liu_bei".into()),
                    pos: Pos::new(2, 1),
                    radius: 0,
                    to: None,
                },
                once: true,
                stage: Some(1),
                when: Vec::new(),
                actions: vec![EventAction::Victory],
            }
        );
        assert_eq!(
            c.drama,
            "\n== orig_b_9\n@narr 다리가 내려왔다.\n@hide all\n\
             \n== orig_b_10\nguan_yu: 첫 줄\n    둘째 줄\nboss: 덤벼라\n전령갑: 큰일입니다\n\
             @show guan_yu left\n@show boss right\n@sfx hit_heavy\n@wait 300\n@sfx retreat\n\
             @wait 300\n@hide all\n@hide all\n"
        );
        let scenes = hero_core::script::parse_drama("t", &c.drama).unwrap();
        assert_eq!(scenes.len(), 2);
    }

    /// Records that leave a phase share the script on the way (one scene, written once), and a
    /// base event that a later phase's record joins fires in that phase only.
    #[test]
    fn phase_changes_share_the_way_and_keep_their_stage() {
        let mut scene = scene();
        scene.blocks[1].records = vec![
            record(
                0,
                0,
                false,
                [0; 6],
                vec![
                    roster(vec![unit(54, 9, 4), unit(300, 8, 4)]),
                    fields("load_map", &[("map", 0x3002)]),
                ],
            ),
            record(0, 1, false, [0; 6], vec![op("begin_battle")]),
            // Phase 0 (not parallel): turn 2 or 300 falls, whichever comes first.
            record(
                9,
                3,
                false,
                [2, 0, 0, 0, 0, 0],
                vec![fields("dialogue", &[("text", 0x10)])],
            ),
            record(
                12,
                3,
                false,
                [44, 1, 0, 0, 0, 0],
                vec![fields("narration", &[("text", 0x30)])],
            ),
            // On the way to phase 1.
            record(
                0,
                4,
                false,
                [0; 6],
                vec![fields("dialogue", &[("text", 0x40)])],
            ),
            // Phase 1: Guan Yu next to 54 (the base battle's duel) also makes 54 retreat.
            record(
                4,
                5,
                true,
                [1, 0, 54, 0, 0, 0],
                vec![fields("remove_person", &[("person", 54)])],
            ),
            record(
                9,
                5,
                false,
                [9, 0, 0, 0, 0, 0],
                vec![fields("narration", &[("text", 0x50)])],
            ),
        ];
        let orig = find_battle(&scene, 2, &[]).unwrap();
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let c = convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &text,
                cell_change: &mut none,
            },
        )
        .unwrap();
        let way = [
            EventAction::SetStage { stage: 1 },
            EventAction::Drama {
                scene: "orig_b_4".into(),
            },
        ];
        let staged: Vec<&EventDef> = c
            .battle
            .events
            .iter()
            .filter(|e| e.stage == Some(0))
            .collect();
        assert_eq!(staged.len(), 2, "{:#?}", c.battle.events);
        for e in staged {
            assert!(e.actions.ends_with(&way), "{e:#?}");
        }
        assert_eq!(c.drama.matches("== orig_b_4\n").count(), 1, "{}", c.drama);
        hero_core::script::parse_drama("t", &c.drama).expect("scene ids are unique");
        let duel = &c.battle.events[0];
        assert!(matches!(duel.trigger, Trigger::Adjacent { .. }));
        assert_eq!(duel.stage, Some(1));
        assert!(duel.actions.contains(&EventAction::Retreat {
            target: "boss".into()
        }));
    }

    /// Without the scene's text the battle is still converted, with its dialogue left out.
    #[test]
    fn missing_text_leaves_only_the_dialogue_out() {
        struct Missing;
        impl TextSource for Missing {
            fn dialogue(&self, _: u16) -> Result<Vec<(u16, String)>, String> {
                Err("dialogue left out: SNR1M.R3 missing".into())
            }
            fn string(&self, _: u16) -> Result<String, String> {
                Err("text left out: SNR1M.R3 missing".into())
            }
        }
        let orig = find_battle(&scene(), 2, &[]).unwrap();
        let mut cells = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let c = convert(
            &base_battle(),
            &orig,
            &names(),
            &pair("b", 1, 0, 2),
            "hexz_02",
            &mut EventSources {
                text: &Missing,
                cell_change: &mut cells,
            },
        )
        .unwrap();
        // The duel's portraits and sounds stay; no line of dialogue does.
        assert!(!c.drama.contains(": "), "{}", c.drama);
        assert!(c
            .battle
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetStage { stage: 1 })));
        assert!(
            c.notes.iter().any(|n| n.contains("SNR1M.R3 missing")),
            "{:?}",
            c.notes
        );
    }

    #[test]
    fn route_flags_decide_the_conditions() {
        let mut scene = scene();
        // Record 7 speaks only on the route of flag 133, and not once flag 90 is set.
        scene.blocks[1].records[7].code.insert(
            0,
            instr(
                "if_flags",
                Operands::Condition {
                    skip: 1,
                    all_set: vec![133],
                    all_clear: vec![90],
                },
            ),
        );
        scene.blocks[1].records[7]
            .code
            .insert(1, fields("narration", &[("text", 0x30)]));
        let text = text();
        let mut none = |_: Pos, _: u8| -> Result<CellChange, String> { Ok(None) };
        let orig = find_battle(&scene, 2, &[]).unwrap();
        let run = |pairing: &Pairing,
                   none: &mut dyn FnMut(Pos, u8) -> Result<CellChange, String>| {
            convert(
                &base_battle(),
                &orig,
                &names(),
                pairing,
                "hexz_02",
                &mut EventSources {
                    text: &text,
                    cell_change: none,
                },
            )
            .unwrap()
        };
        let off_route = run(&pair("b", 1, 0, 2), &mut none);
        assert!(!off_route.drama.contains("orig_b_7"), "{}", off_route.drama);
        let on_route = run(
            &Pairing {
                flags: &[133],
                ..pair("b", 1, 0, 2)
            },
            &mut none,
        );
        assert!(
            on_route.drama.contains("== orig_b_7\n@narr"),
            "{}",
            on_route.drama
        );
        // Flag 90 is set by record 9 and tested by record 7: a battle flag and a condition. Only
        // the narration it guards waits for it; the arrival does not.
        let events = &on_route.battle.events;
        let rec7: Vec<&EventDef> = events
            .iter()
            .filter(|e| matches!(e.trigger, Trigger::Reach { to: Some(_), .. }))
            .collect();
        assert_eq!(rec7.len(), 2, "{rec7:#?}");
        assert_eq!(
            (&rec7[0].when[..], &rec7[0].actions[..]),
            (
                &[][..],
                &[EventAction::Spawn {
                    group: "original_7".into()
                }][..]
            )
        );
        assert_eq!(
            rec7[1].when,
            [FlagCond {
                flag: "orig_b_90".into(),
                cmp: Compare::Eq,
                value: 0
            }]
        );
        assert_eq!(
            rec7[1].actions,
            [EventAction::Drama {
                scene: "orig_b_7".into()
            }]
        );
        assert_eq!(rec7[1].stage, Some(0));
        assert!(events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetFlag {
                flag: "orig_b_90".into(),
                value: 1
            })));
        assert!(
            off_route.battle.events.iter().all(|e| e.when.is_empty()),
            "the route decides first"
        );
        // A cell the operation leaves alone changes nothing.
        assert!(!on_route
            .battle
            .events
            .iter()
            .flat_map(|e| &e.actions)
            .any(|a| matches!(a, EventAction::SetTerrain { .. })));
    }
}
