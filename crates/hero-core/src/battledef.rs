//! Battle scenario definitions (`battles/<id>.toml`).

use crate::data::{Equipment, Id};
use crate::geom::Pos;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn yes() -> bool {
    true
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Units the player controls.
    #[default]
    Player,
    /// Friendly units controlled by the AI (move in their own phase after the player).
    Ally,
    Enemy,
}

impl Side {
    /// Player and ally are friends; enemy is hostile to both.
    pub fn is_hostile(self, other: Side) -> bool {
        (self == Side::Enemy) != (other == Side::Enemy)
    }
}

/// How an AI-controlled unit behaves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiMode {
    /// Seek out and attack the most attractive hostile unit anywhere on the map.
    #[default]
    Aggressive,
    /// Stay put until a hostile unit comes within (move + attack) reach, then fight.
    Defensive,
    /// Never move; attack or use strategies only from the current tile.
    Hold,
    /// Stay within 3 tiles of `ai_pos` (defaults to the spawn tile); attack anything that comes close.
    Guard,
    /// Head for the unit named by `ai_target` (tag or officer id) and attack it.
    Target,
    /// Move towards `ai_pos`, attacking targets of opportunity on the way.
    Advance,
    /// Move away from hostile units (fleeing civilians, escaping commanders).
    Flee,
}

/// The map of a battle: written in the battle file, or taken from the pack's map files with
/// `use`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapDef {
    /// Id of a map of the pack's map files ([`MapEntry`], `maps` in `pack.toml`). Such a
    /// battle writes nothing else in `[map]`: `Pack::load` copies the map's `rows`, `legend`,
    /// `theme` and `image` here and keeps the id, so a loaded battle always has its rows.
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_map: Option<Id>,
    /// One text line per map row; characters are terrain glyphs.
    #[serde(default)]
    pub rows: String,
    /// Extra glyph -> terrain id mappings for this map (single-character keys).
    #[serde(default)]
    pub legend: BTreeMap<String, Id>,
    /// Visual theme hint for the renderer (e.g. `field`, `castle`, `snow`, `desert`).
    #[serde(default)]
    pub theme: Option<String>,
    /// Picture layer: media key of `gfx/maps/<image>.png`, a picture of the whole map drawn
    /// instead of the terrain tileset. `rows` stay the rules (movement, defence, healing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl MapDef {
    /// Whether the definition writes any part of a map itself (which a `use` map must not).
    pub fn has_own_content(&self) -> bool {
        !self.rows.trim().is_empty()
            || !self.legend.is_empty()
            || self.theme.is_some()
            || self.image.is_some()
    }
}

/// A map of a map file (`[[map]]`), shared by the battles that `use` its id. Map files let a
/// pack ship maps apart from battles, e.g. the converted original maps before the battles
/// that play on them exist, or one map for several battles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapEntry {
    pub id: Id,
    /// Display name for tools and authors (battles show their own name).
    #[serde(default)]
    pub name: String,
    /// Same as [`MapDef::rows`].
    pub rows: String,
    #[serde(default)]
    pub legend: BTreeMap<String, Id>,
    #[serde(default)]
    pub theme: Option<String>,
    /// Same as [`MapDef::image`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl MapEntry {
    /// The battle map definition of a battle that uses this map.
    pub fn to_def(&self) -> MapDef {
        MapDef {
            use_map: Some(self.id.clone()),
            rows: self.rows.clone(),
            legend: self.legend.clone(),
            theme: self.theme.clone(),
            image: self.image.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeployDef {
    /// Maximum number of player officers that may be deployed.
    pub max: u32,
    /// Officers that must be deployed (always includes the lord implicitly).
    #[serde(default)]
    pub required: Vec<Id>,
    /// Officers that may NOT be deployed in this battle.
    #[serde(default)]
    pub forbidden: Vec<Id>,
    /// Deployment tiles, filled in order (required officers first).
    pub slots: Vec<Pos>,
}

/// A unit placed on the map at battle start (or later, when its `group` is spawned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitSpawn {
    pub side: Side,
    /// Named officer (portrait, stats and class come from `officers.toml`).
    #[serde(default)]
    pub officer: Option<Id>,
    /// Display name for generic units, e.g. `황건적`. Ignored when `officer` is set.
    #[serde(default)]
    pub name: Option<String>,
    /// Class; required for generic units, overrides the officer's class otherwise.
    #[serde(default)]
    pub class: Option<Id>,
    /// Level; defaults to the officer's level (required for generic units).
    #[serde(default)]
    pub level: Option<u32>,
    /// `[str, int, lead]` for generic units (default: the class's `generic` stats).
    #[serde(default)]
    pub stats: Option<[i32; 3]>,
    pub pos: Pos,
    #[serde(default)]
    pub ai: AiMode,
    /// Tag or officer id for `ai = "target"`.
    #[serde(default)]
    pub ai_target: Option<String>,
    /// Destination for `advance`, centre for `guard`.
    #[serde(default)]
    pub ai_pos: Option<Pos>,
    /// Enemy commander (the usual "defeat the commander" victory target).
    #[serde(default)]
    pub commander: bool,
    /// Name used by conditions and events to refer to this unit.
    #[serde(default)]
    pub tag: Option<String>,
    /// Reinforcement group: units with a group stay off-map until an event spawns the group.
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub equip: Option<Equipment>,
    /// Item given to the player when this unit is defeated.
    #[serde(default)]
    pub drop: Option<Id>,
}

/// Something that can become true during a battle. Unit references (`target`, `who`)
/// accept a spawn `tag` or an officer id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// Every enemy unit on the map has retreated (hidden reinforcements do not count).
    DefeatAll,
    /// The given unit has retreated.
    DefeatUnit { target: String },
    /// Any enemy commander has retreated.
    DefeatCommander,
    /// A unit (or any player unit when `who` is absent) stands within `radius` of `pos`.
    Reach {
        #[serde(default)]
        who: Option<String>,
        pos: Pos,
        #[serde(default)]
        radius: i32,
    },
    /// The given turn has been completed (all phases of that turn ended).
    SurviveTurns { turns: u32 },
    /// The given (usually allied) unit has retreated — used as a defeat condition.
    UnitRetreated { target: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Start of `side`'s phase on `turn`.
    TurnStart {
        turn: u32,
        #[serde(default)]
        side: Side,
    },
    /// A unit retreated.
    UnitDefeated { target: String },
    /// A unit (any player unit when `who` is absent) moved within `radius` of `pos`.
    Reach {
        #[serde(default)]
        who: Option<String>,
        pos: Pos,
        #[serde(default)]
        radius: i32,
    },
    /// Two units stand orthogonally adjacent (typical duel trigger).
    Adjacent { a: String, b: String },
    /// A unit's HP fell below `pct` percent of its max.
    HpBelow { target: String, pct: i32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventAction {
    /// Play a drama scene (global scene id).
    Drama {
        scene: String,
    },
    /// Bring a reinforcement group onto the map (occupied tiles shift to the nearest free tile).
    Spawn {
        group: String,
    },
    /// Replace the AI of every unit `target` names. All AI fields are replaced: an omitted
    /// `ai_target` or `ai_pos` is cleared (`guard` without `ai_pos` guards the current tile;
    /// `advance` without `ai_pos` behaves as `aggressive`).
    SetAi {
        target: String,
        ai: AiMode,
        #[serde(default)]
        ai_target: Option<String>,
        #[serde(default)]
        ai_pos: Option<Pos>,
    },
    /// Remove a unit from the map without defeating it in combat (duel loser, escape).
    Retreat {
        target: String,
    },
    /// Grant levels (duel reward).
    LevelUp {
        target: String,
        amount: u32,
    },
    GiveItem {
        item: Id,
    },
    GiveGold {
        amount: i64,
    },
    SetFlag {
        flag: String,
        value: i64,
    },
    Victory,
    Defeat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDef {
    pub trigger: Trigger,
    /// Fire only the first time the trigger becomes true.
    #[serde(default = "yes")]
    pub once: bool,
    pub actions: Vec<EventAction>,
}

/// Optional secondary objective; completing it gives every surviving deployed unit bonus EXP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BonusDef {
    pub condition: Condition,
    pub exp: u32,
    #[serde(default)]
    pub desc: String,
}

/// Treasury / granary / village tile: the first player unit to stop there takes the reward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreasureDef {
    pub pos: Pos,
    #[serde(default)]
    pub item: Option<Id>,
    #[serde(default)]
    pub gold: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleDef {
    pub id: Id,
    /// Display name, e.g. `탁현 전투`.
    pub name: String,
    /// Place/year caption, e.g. `184년 유주 탁현`.
    #[serde(default)]
    pub location: String,
    /// One-line objective shown to the player, e.g. `적장 정원지를 물리쳐라`.
    pub objective: String,
    /// Music for the player phase / enemy phase.
    #[serde(default)]
    pub bgm: Option<String>,
    #[serde(default)]
    pub bgm_enemy: Option<String>,
    /// The battle is lost when this turn ends without victory.
    pub turn_limit: u32,
    pub map: MapDef,
    pub deploy: DeployDef,
    pub units: Vec<UnitSpawn>,
    /// Any satisfied condition wins the battle.
    pub victory: Vec<Condition>,
    /// Any satisfied condition loses the battle. The lord retreating and running out of
    /// turns always lose and need not be listed.
    #[serde(default)]
    pub defeat: Vec<Condition>,
    #[serde(default)]
    pub bonus: Option<BonusDef>,
    #[serde(default)]
    pub events: Vec<EventDef>,
    #[serde(default)]
    pub treasures: Vec<TreasureDef>,
    /// Gold awarded on victory.
    #[serde(default)]
    pub reward_gold: i64,
    /// Drama scene played before the first turn / after victory.
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub outro: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_battle() {
        let src = r#"
id = "b01"
name = "탁현 전투"
objective = "적장을 물리쳐라"
turn_limit = 20
reward_gold = 300
victory = [{ type = "defeat_commander" }]

[map]
rows = """
..T
~~.
"""

[deploy]
max = 3
slots = [[0, 0], [1, 0]]

[[units]]
side = "enemy"
name = "황건적"
class = "bandit"
level = 2
pos = [2, 1]
ai = "aggressive"
commander = true

[[events]]
trigger = { type = "turn_start", turn = 3, side = "enemy" }
actions = [{ type = "spawn", group = "rein" }, { type = "drama", scene = "b01_rein" }]
"#;
        let b: BattleDef = toml::from_str(src).unwrap();
        assert_eq!(b.units[0].side, Side::Enemy);
        assert!(b.units[0].commander);
        assert_eq!(b.victory, vec![Condition::DefeatCommander]);
        assert_eq!(b.events[0].actions.len(), 2);
        assert!(b.events[0].once);
        assert!(Side::Player.is_hostile(Side::Enemy));
        assert!(!Side::Player.is_hostile(Side::Ally));
    }
}
