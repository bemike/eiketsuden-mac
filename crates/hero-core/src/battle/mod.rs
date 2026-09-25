//! Runtime battle state and rules.
//!
//! The frontend drives a battle through [`BattleState::apply`] and animates the returned
//! [`BattleEvent`]s in order. AI turns use [`BattleState::next_ai_unit`] +
//! [`BattleState::ai_actions`]; headless simulations use [`BattleState::run_ai_phase`].
//! Cancelling a move is done by the frontend restoring a clone taken before `Action::Move`.

use crate::battledef::{AiMode, BattleDef, Side};
use crate::campaign::CampaignState;
use crate::data::{Equipment, Id, StatusKind, TerrainDef};
use crate::geom::{Dir, Pos};
use crate::map::BattleMap;
use crate::pack::Pack;
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type UnitId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    /// Reinforcement not yet on the map.
    Hidden,
    Active,
    /// HP reached 0 or removed by an event.
    Retreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveStatus {
    pub status: StatusKind,
    /// Remaining turns; decremented at the start of the owner's phase, removed at 0.
    pub turns: u8,
}

/// Weather, re-rolled at the start of every turn. Rain blocks `fire` strategies and gives
/// `water` strategies +25% damage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weather {
    #[default]
    Clear,
    Cloudy,
    Rain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub side: Side,
    pub officer: Option<Id>,
    pub name: String,
    pub class: Id,
    pub level: u32,
    pub exp: u32,
    #[serde(rename = "str")]
    pub strength: i32,
    pub int: i32,
    pub lead: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    /// 0..=100; part of the attack/defense formula.
    pub morale: i32,
    pub pos: Pos,
    pub facing: Dir,
    pub moved: bool,
    pub acted: bool,
    pub equip: Equipment,
    pub statuses: Vec<ActiveStatus>,
    pub ai: AiMode,
    pub ai_target: Option<String>,
    pub ai_pos: Option<Pos>,
    pub commander: bool,
    pub lord: bool,
    pub tag: Option<String>,
    pub group: Option<String>,
    pub state: UnitState,
    pub portrait: Option<String>,
    pub drop: Option<Id>,
}

impl Unit {
    pub fn is_active(&self) -> bool {
        self.state == UnitState::Active
    }

    pub fn has_status(&self, s: StatusKind) -> bool {
        self.statuses.iter().any(|a| a.status == s)
    }

    /// Whether `reference` (a tag or an officer id) names this unit.
    pub fn matches(&self, reference: &str) -> bool {
        self.tag.as_deref() == Some(reference) || self.officer.as_deref() == Some(reference)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefeatReason {
    LordRetreated,
    TurnLimit,
    /// A `defeat` condition of the battle definition became true.
    Condition,
    /// An event action forced defeat.
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Victory,
    Defeat(DefeatReason),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Move within the movement range. A unit moves at most once per phase.
    Move { unit: UnitId, to: Pos },
    Attack { unit: UnitId, target: UnitId },
    /// Aim a strategy at a tile (area effects are centred there).
    Strategy { unit: UnitId, strategy: Id, target: Pos },
    /// Use a battle consumable from the army inventory. For strategy scrolls `target` is the
    /// unit on the aimed tile.
    UseItem { unit: UnitId, item: Id, target: UnitId },
    /// End this unit's action without doing anything.
    Wait { unit: UnitId },
    /// End the current side's phase (remaining units forfeit their actions).
    EndPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    #[error("battle is already over")]
    BattleOver,
    #[error("unit {0} does not exist or is not on the map")]
    NoSuchUnit(UnitId),
    #[error("unit {0} does not belong to the side whose phase it is")]
    NotYourTurn(UnitId),
    #[error("unit {0} has already acted")]
    AlreadyActed(UnitId),
    #[error("unit {0} has already moved")]
    AlreadyMoved(UnitId),
    #[error("unit {0} is confused and cannot act")]
    Confused(UnitId),
    #[error("destination is out of range or blocked")]
    Unreachable,
    #[error("target is not in range")]
    OutOfRange,
    #[error("invalid target")]
    InvalidTarget,
    #[error("strategy `{0}` is unknown or not learned")]
    UnknownStrategy(Id),
    #[error("not enough MP")]
    NotEnoughMp,
    #[error("terrain or weather does not allow this strategy")]
    WrongTerrain,
    #[error("item `{0}` is not usable or not in the inventory")]
    BadItem(Id),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BattleError {
    #[error("unknown battle `{0}`")]
    UnknownBattle(Id),
    #[error("battle setup failed: {0}")]
    Setup(String),
}

/// One reachable tile of a movement range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveStep {
    /// Movement points spent to get here.
    pub cost: i32,
    /// Previous tile on the cheapest path (`None` for the origin).
    pub prev: Option<Pos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MoveRange {
    pub origin: Pos,
    /// Every tile the unit may end its move on (the origin included). Tiles occupied by
    /// friendly units can be passed through but are not listed.
    pub tiles: BTreeMap<Pos, MoveStep>,
}

impl MoveRange {
    pub fn contains(&self, p: Pos) -> bool {
        self.tiles.contains_key(&p)
    }

    /// Path from origin to `p`, both included.
    pub fn path_to(&self, p: Pos) -> Option<Vec<Pos>> {
        let _ = p;
        todo!("W1a: MoveRange::path_to")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterForecast {
    pub damage: i32,
    /// Chance in percent that the counter happens.
    pub chance: i32,
}

/// Numbers shown in the attack preview window. Physical attacks are deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackForecast {
    /// Damage the attack will deal.
    pub damage: i32,
    /// Defender DEF multiplier from class affinity in percent (75 = attacker has the advantage).
    pub affinity: i32,
    /// Counter-attack preview, when the defender can counter this attack.
    pub counter: Option<CounterForecast>,
}

/// Preview of a strategy against one affected unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyForecast {
    pub unit: UnitId,
    /// Success chance in percent.
    pub chance: i32,
    /// Expected HP damage (positive) or healing (negative) without the random bonus;
    /// 0 for pure morale/status effects.
    pub amount: i32,
}

/// Result of a strategy on one unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyHit {
    pub unit: UnitId,
    pub success: bool,
    /// HP damage dealt (0 when none).
    pub damage: i32,
    /// HP healed (0 when none).
    pub healed: i32,
    /// Morale change applied (negative for morale-down).
    pub morale: i32,
    pub status: Option<StatusKind>,
}

/// Everything the frontend needs to animate, in the order it happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BattleEvent {
    PhaseStart { side: Side, turn: u32 },
    Moved { unit: UnitId, path: Vec<Pos> },
    /// One strike: the attack itself, or the defender's counter-attack.
    Strike {
        attacker: UnitId,
        defender: UnitId,
        damage: i32,
        /// Morale the defender lost.
        morale_loss: i32,
        /// This strike is a counter-attack.
        counter: bool,
    },
    StrategyUsed {
        caster: UnitId,
        strategy: Id,
        target: Pos,
        hits: Vec<StrategyHit>,
    },
    ItemUsed { user: UnitId, target: UnitId, item: Id, healed: i32, morale: i32 },
    /// Terrain / treasure / band-aura regeneration at phase start.
    Regenerated { unit: UnitId, hp: i32, mp: i32, morale: i32 },
    /// A unit became confused (strategy or low morale).
    Confused { unit: UnitId },
    StatusExpired { unit: UnitId, status: StatusKind },
    WeatherChanged { weather: Weather },
    ExpGained { unit: UnitId, amount: u32 },
    LevelUp { unit: UnitId, level: u32, hp_gain: i32, mp_gain: i32 },
    Promoted { unit: UnitId, from: Id, to: Id },
    Learned { unit: UnitId, strategy: Id },
    Retreated { unit: UnitId },
    Spawned { units: Vec<UnitId> },
    TreasureFound { unit: UnitId, item: Option<Id>, gold: i64 },
    ItemDropped { unit: UnitId, item: Id },
    /// Play a drama scene now (battle state has already been updated).
    Drama { scene: String },
    BonusAchieved { exp: u32 },
    Victory,
    Defeat(DefeatReason),
}

/// Full battle state; `Clone` for move-cancel snapshots and `Serialize` for mid-battle saves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleState {
    pub battle_id: Id,
    pub map: BattleMap,
    pub units: Vec<Unit>,
    pub turn: u32,
    pub turn_limit: u32,
    pub phase: Side,
    pub weather: Weather,
    pub rng: Rng,
    /// Per event definition: already fired (for `once` events).
    pub fired: Vec<bool>,
    /// Per treasure definition: already taken.
    pub treasures_taken: Vec<bool>,
    pub outcome: Option<Outcome>,
    pub bonus_done: bool,
    /// Consumables available to the player (copied from the campaign inventory, written back after).
    pub inventory: BTreeMap<Id, u32>,
    /// Gold and items picked up during the battle (added to the campaign after victory).
    pub gold_found: i64,
    pub items_found: Vec<Id>,
    /// Campaign flags set by event actions during the battle.
    pub flags: BTreeMap<String, i64>,
}

impl BattleState {
    /// Build the initial state of `battle`: player units from `campaign.deployed` (falling back to
    /// required officers + roster order up to `deploy.max`) placed on deploy slots, enemy/ally
    /// spawns without a `group` placed on the map, grouped spawns hidden.
    pub fn new(pack: &Pack, battle: &str, campaign: &CampaignState, seed: u64) -> Result<BattleState, BattleError> {
        let _ = (pack, campaign, seed);
        todo!("W1a: BattleState::new {battle}")
    }

    /// Start turn 1 (player phase): fires `TurnStart{1, player}` events and the intro drama.
    pub fn begin(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        let _ = pack;
        todo!("W1a: begin")
    }

    pub fn def<'a>(&self, pack: &'a Pack) -> &'a BattleDef {
        &pack.battles[&self.battle_id]
    }

    pub fn unit(&self, id: UnitId) -> &Unit {
        &self.units[id]
    }

    /// Active unit standing on `pos`.
    pub fn unit_at(&self, pos: Pos) -> Option<UnitId> {
        self.units
            .iter()
            .find(|u| u.is_active() && u.pos == pos)
            .map(|u| u.id)
    }

    /// First unit (any state) matching a tag or officer id.
    pub fn find_unit(&self, reference: &str) -> Option<UnitId> {
        self.units.iter().find(|u| u.matches(reference)).map(|u| u.id)
    }

    pub fn terrain_at<'a>(&self, pack: &'a Pack, pos: Pos) -> Option<&'a TerrainDef> {
        self.map.terrain_at(pos).and_then(|id| pack.terrain(id))
    }

    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }

    /// Unit may still act in the current phase.
    pub fn can_act(&self, id: UnitId) -> bool {
        let u = &self.units[id];
        u.is_active() && u.side == self.phase && !u.acted && self.outcome.is_none()
    }

    // ----- derived stats -------------------------------------------------------------------

    /// Attack power: `(level + 10) * (morale/10 + 400/(140 - str) + class.atk)`, times the best
    /// weapon's `atk_pct`. See `docs/RULES.md`.
    pub fn attack_power(&self, pack: &Pack, id: UnitId) -> i32 {
        let _ = (pack, id);
        todo!("W1a: attack_power")
    }

    /// Defense power: same shape as attack with `lead`, `class.def` and `def_pct`.
    pub fn defense_power(&self, pack: &Pack, id: UnitId) -> i32 {
        let _ = (pack, id);
        todo!("W1a: defense_power")
    }

    /// Movement points including the best horse (0 while confused).
    pub fn move_points(&self, pack: &Pack, id: UnitId) -> i32 {
        let _ = (pack, id);
        todo!("W1a: move_points")
    }

    // ----- queries -------------------------------------------------------------------------

    /// Tiles the unit may move to this phase. Hostile units block; zone of control applies
    /// (entering a tile adjacent to a hostile unit ends movement). Empty when already moved.
    pub fn movement_range(&self, pack: &Pack, id: UnitId) -> MoveRange {
        let _ = (pack, id);
        todo!("W1a: movement_range")
    }

    /// In-bounds tiles covered by the unit's attack range if it stood on `from`.
    pub fn attack_tiles(&self, pack: &Pack, id: UnitId, from: Pos) -> Vec<Pos> {
        let _ = (pack, id, from);
        todo!("W1a: attack_tiles")
    }

    /// Hostile active units attackable from `from`.
    pub fn attack_targets(&self, pack: &Pack, id: UnitId, from: Pos) -> Vec<UnitId> {
        let _ = (pack, id, from);
        todo!("W1a: attack_targets")
    }

    /// Strategies the unit knows and can currently afford (empty when confused).
    pub fn usable_strategies(&self, pack: &Pack, id: UnitId) -> Vec<Id> {
        let _ = (pack, id);
        todo!("W1a: usable_strategies")
    }

    /// Tiles where `strategy` may be aimed from `from` that would affect at least one valid
    /// unit and satisfy the terrain/weather requirement. For `Area::AllInRange` strategies
    /// this returns the caster's own tile when at least one target is in reach.
    pub fn strategy_targets(&self, pack: &Pack, id: UnitId, strategy: &str, from: Pos) -> Vec<Pos> {
        let _ = (pack, id, strategy, from);
        todo!("W1a: strategy_targets")
    }

    /// Units `item` can be used on: for healing items the user and orthogonally adjacent
    /// friendly units; for strategy scrolls the targets of that strategy from the user's tile.
    pub fn item_targets(&self, pack: &Pack, id: UnitId, item: &str) -> Vec<UnitId> {
        let _ = (pack, id, item);
        todo!("W1a: item_targets")
    }

    pub fn forecast_attack(&self, pack: &Pack, attacker: UnitId, defender: UnitId) -> AttackForecast {
        let _ = (pack, attacker, defender);
        todo!("W1a: forecast_attack")
    }

    pub fn forecast_strategy(&self, pack: &Pack, caster: UnitId, strategy: &str, target: Pos) -> Vec<StrategyForecast> {
        let _ = (pack, caster, strategy, target);
        todo!("W1a: forecast_strategy")
    }

    // ----- mutation ------------------------------------------------------------------------

    /// Validate and perform an action; returns the resulting events (including level ups,
    /// retreats, triggered events, phase changes and victory/defeat).
    pub fn apply(&mut self, pack: &Pack, action: Action) -> Result<Vec<BattleEvent>, ActionError> {
        let _ = (pack, action);
        todo!("W1a: apply")
    }

    // ----- AI ------------------------------------------------------------------------------

    /// Next AI-controlled unit of the current phase that has not acted (None during the
    /// player phase or when all have acted).
    pub fn next_ai_unit(&self) -> Option<UnitId> {
        todo!("W1a: next_ai_unit")
    }

    /// Plan the actions for one AI unit: optionally a `Move`, then exactly one of `Attack`,
    /// `Strategy`, `UseItem` or `Wait`. Deterministic for a given state.
    pub fn ai_actions(&self, pack: &Pack, id: UnitId) -> Vec<Action> {
        let _ = (pack, id);
        todo!("W1a: ai_actions")
    }

    /// Let the AI play every unit of the current phase (also usable for the player side in
    /// simulations) and then end the phase. Returns all events.
    pub fn run_ai_phase(&mut self, pack: &Pack) -> Vec<BattleEvent> {
        let _ = pack;
        todo!("W1a: run_ai_phase")
    }
}
