//! Static rule definitions loaded from a data pack (`rules/*.toml`, `officers.toml`).
//!
//! These structs ARE the data schema: `docs/MODDING.md` documents them for content
//! authors, and every field is deserialized with serde from TOML. Anything that is
//! referenced by id (classes, terrain, items, strategies, officers, move types,
//! families, elements) is a plain `String` id that [`crate::pack::Pack::validate`] cross-checks.
//! The formulas that use these numbers are described in `docs/RULES.md`.

use crate::geom::Pos;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Id = String;

// ---------------------------------------------------------------------------
// rules/game.toml
// ---------------------------------------------------------------------------

/// Chances (percent, summing to 100) of each weather state, re-rolled every turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherChances {
    pub clear: i32,
    pub cloudy: i32,
    pub rain: i32,
}

/// Global tuning constants (`rules/game.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameRules {
    /// Highest reachable level.
    pub level_cap: u32,
    /// Experience needed per level (the counter keeps the remainder on level up).
    pub exp_per_level: u32,
    /// Maximum amount of gold the army can hold.
    pub gold_cap: i64,
    /// Upper bound for strategy points (MP).
    pub mp_cap: i32,
    /// Morale every unit starts a battle with (0..=100).
    pub morale_start: i32,
    /// Morale lost when hit = damage * morale_loss_pct / max_hp (rounded up).
    pub morale_loss_pct: i32,
    /// At or below this morale a unit may fall into confusion at the start of its phase.
    pub confuse_morale: i32,
    /// EXP for damaging an enemy, by `target level - own level`.
    /// Sorted `[diff, exp]` pairs; lookup takes the last pair whose diff <= actual diff.
    pub exp_attack: Vec<[i32; 2]>,
    /// EXP for defeating an enemy (same lookup as `exp_attack`).
    pub exp_kill: Vec<[i32; 2]>,
    /// Extra EXP for defeating an enemy commander.
    pub exp_commander: u32,
    /// EXP for a successful heal/morale/confusion strategy (classes may override for single targets).
    pub exp_support: u32,
    /// Defender DEF multiplier in percent by class family: attacker family -> defender family -> percent
    /// (PC original: 75 when the attacker has the advantage, 125 when it has the disadvantage).
    /// Missing entries mean 100.
    #[serde(default)]
    pub affinity: BTreeMap<Id, BTreeMap<Id, i32>>,
    /// Counter-attack chance in percent = attacker-of-the-counter's STR * 100 / counter_divisor.
    pub counter_divisor: i32,
    /// Counter-attack damage in percent of a normal attack.
    pub counter_damage_pct: i32,
    pub weather: WeatherChances,
    /// Which strategy formulas the battles use (RULES.md §5, §6).
    #[serde(default)]
    pub strategy_formulas: StrategyFormulas,
}

/// The strategy formulas of `GameRules::strategy_formulas`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyFormulas {
    /// The engine's (RULES.md §5): support bonus `2 × (INT + LV × INT / 50)` halved on low
    /// morale, confusion for the strategy's turns, damage at least 1.
    #[default]
    Engine,
    /// The PC original's (FORMATS §10.4): support bonus `LV × INT / 20` plus up to 10 %, morale
    /// support `+ LV / 10` plus up to 10 %, confusion hits against half the target's power and
    /// lasts until a roll `rand(100) < (LEAD + morale) / 3` at the unit's phase start, a
    /// morale-down under 30 confuses with 60 %, no minimum damage, the damage bonus below
    /// `raw / 50` (the original's `rand`), healing items plus up to 10 % too.
    Original,
}

impl GameRules {
    /// Look up an EXP table (`exp_attack` / `exp_kill`) for a level difference.
    pub fn exp_lookup(table: &[[i32; 2]], diff: i32) -> u32 {
        let mut out = table.first().map(|p| p[1]).unwrap_or(0);
        for p in table {
            if p[0] <= diff {
                out = p[1];
            } else {
                break;
            }
        }
        out.max(0) as u32
    }

    /// DEF multiplier (percent) applied to the defender for this family pairing.
    pub fn affinity_pct(&self, attacker_family: &str, defender_family: &str) -> i32 {
        self.affinity
            .get(attacker_family)
            .and_then(|m| m.get(defender_family))
            .copied()
            .unwrap_or(100)
    }
}

// ---------------------------------------------------------------------------
// rules/terrain.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainDef {
    pub id: Id,
    pub name: String,
    /// Character used for this terrain in battle map `rows`.
    pub glyph: char,
    /// Percent of incoming physical damage removed while standing here (forest 20, mountain 30 ...).
    #[serde(default)]
    pub defense: i32,
    /// Percent of max HP restored at the start of the occupant's phase (villages, barracks, forts).
    #[serde(default)]
    pub heal_hp: i32,
    /// Morale restored at the start of the occupant's phase.
    #[serde(default)]
    pub heal_morale: i32,
    /// Strategy elements that may target a unit on this tile (e.g. `fire`, `water`, `earth`).
    #[serde(default)]
    pub elements: Vec<Id>,
    /// Elements that deal +25% damage on this tile (fire in forest).
    #[serde(default)]
    pub boost: Vec<Id>,
    /// Movement cost per move type id. A move type that is absent cannot enter.
    #[serde(default)]
    pub cost: BTreeMap<Id, u8>,
    /// Renderer tile key (defaults to the terrain id).
    #[serde(default)]
    pub tile: Option<String>,
}

impl TerrainDef {
    pub fn move_cost(&self, move_type: &str) -> Option<u8> {
        self.cost.get(move_type).copied()
    }
    pub fn tile_key(&self) -> &str {
        self.tile.as_deref().unwrap_or(&self.id)
    }
}

// ---------------------------------------------------------------------------
// Ranges (attack shapes and strategy reach)
// ---------------------------------------------------------------------------

/// A set of tiles relative to a unit: a named shape or explicit `[dx, dy]` offsets.
///
/// Attack shapes: `adjacent4`, `adjacent8`, `archer` (distance exactly 2),
/// `crossbow` (distance 2-3 within a 5x5 square), `catapult` (crossbow + corners + distance-3 cross).
/// Strategy reach (includes the caster's own tile): `self`, `range8` (3x3),
/// `range12` (3x3 plus the four tiles two steps straight out), `range20` (5x5 minus corners),
/// `range28` (the whole 5x5 plus the four tiles three steps straight out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RangeSpec {
    Named(String),
    Offsets(Vec<[i32; 2]>),
}

impl RangeSpec {
    /// Offsets relative to the unit, or `None` for an unknown named shape.
    pub fn offsets(&self) -> Option<Vec<Pos>> {
        let v: Vec<Pos> = match self {
            RangeSpec::Offsets(o) => o.iter().map(|a| Pos::new(a[0], a[1])).collect(),
            RangeSpec::Named(n) => {
                let mut out = Vec::new();
                for dy in -3..=3i32 {
                    for dx in -3..=3i32 {
                        let (m, c) = (dx.abs() + dy.abs(), dx.abs().max(dy.abs()));
                        let hit = match n.as_str() {
                            "adjacent4" => m == 1,
                            "adjacent8" => c == 1,
                            "archer" => m == 2,
                            "crossbow" => (m == 2 || m == 3) && c <= 2,
                            "catapult" => (m >= 2 && c <= 2) || (m == 3 && c == 3),
                            "self" => m == 0,
                            "range8" => c <= 1,
                            "range12" => c <= 1 || (m == 2 && c == 2 && (dx == 0 || dy == 0)),
                            "range20" => c <= 2 && m <= 3,
                            "range28" => c <= 2 || (m == 3 && c == 3),
                            _ => return None,
                        };
                        if hit {
                            out.push(Pos::new(dx, dy));
                        }
                    }
                }
                out
            }
        };
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// rules/classes.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Learn {
    pub level: u32,
    pub id: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Promotion {
    /// Class id after promotion.
    pub to: Id,
    /// Minimum level.
    pub level: u32,
    /// Class-up item that must be used on the unit (PC original: 장창, 연노, 마개 ...).
    pub item: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDef {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub hanja: String,
    /// Class family id: used for the affinity table and equipment restrictions.
    pub family: Id,
    /// 1 = basic, 2 = first promotion, 3 = final (single-tier classes use 1).
    pub tier: u8,
    /// Movement points.
    #[serde(rename = "move")]
    pub move_points: u8,
    /// Move type id (keys of `TerrainDef::cost`).
    pub move_type: Id,
    /// Attack range shape.
    pub range: RangeSpec,
    /// Class attack correction (PC original: 4..16).
    pub atk: i32,
    /// Class defense correction (PC original: 4..16).
    pub def: i32,
    /// Max HP (troops) at level 1.
    pub hp: i32,
    /// Max HP gained per level: `hp + hp_growth * (level - 1)`.
    pub hp_growth: i32,
    /// `[str, int, lead]` for generic (unnamed) units of this class.
    pub generic: [i32; 3],
    /// Strategies learned at a level. A promoted class also knows its predecessors' lists.
    #[serde(default)]
    pub strategies: Vec<Learn>,
    #[serde(default)]
    pub promote: Option<Promotion>,
    /// Unit sprite sheet key.
    pub sprite: String,
    /// Can counter-attack adjacent melee attackers (PC: upper bandit tiers and martial artists).
    #[serde(default)]
    pub can_counter: bool,
    /// Attacks from this class can provoke a counter-attack (PC: cavalry, bandits, martial
    /// artists, beast tamers, tribesmen — not infantry or archers).
    #[serde(default)]
    pub provokes_counter: bool,
    /// Support class: takes half strategy damage and evades strategies as if INT were doubled.
    #[serde(default)]
    pub strategy_guard: bool,
    /// Military band aura: orthogonally adjacent units regain `level / 10 + 1` MP each phase start.
    #[serde(default)]
    pub mp_aura: bool,
    /// EXP for single-target support strategies cast by this class (overrides `exp_support`).
    #[serde(default)]
    pub support_exp: Option<u32>,
    #[serde(default)]
    pub desc: String,
}

// ---------------------------------------------------------------------------
// rules/strategies.toml and rules/items.toml share the Effect type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// Cannot move or act. Wears off after its turns, but persists while morale is low;
    /// a confused unit whose morale reaches 0 retreats.
    Confused,
}

impl StatusKind {
    pub const ALL: [StatusKind; 1] = [StatusKind::Confused];

    /// The id data files name the status by (`confused`).
    pub const fn id(self) -> &'static str {
        match self {
            StatusKind::Confused => "confused",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// Strategy damage: `power` is the base damage of the strategy (see RULES.md).
    Damage { power: i32 },
    /// Restore HP: strategies scale `power` with caster INT/level; items restore exactly `power`.
    Heal { power: i32 },
    /// Change morale by `amount` (negative = morale-down strategy), clamped to 0..=100.
    Morale { amount: i32 },
    /// Inflict a status for `turns` turns (enemy-targeted: uses the strategy hit chance).
    Status { status: StatusKind, turns: u8 },
    /// Class-up item: promote to `ClassDef::promote.to` when the level requirement is met.
    Promote,
    /// Class-change item: switch to the given (tier-1) class, keeping the level.
    ChangeClass { to: Id },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyKind {
    Attack,
    Heal,
    Support,
}

/// Who a strategy may be aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetSide {
    Enemy,
    /// Own side and allied side (including the caster).
    Ally,
}

/// Units affected by a strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// The unit on the aimed tile.
    Single,
    /// The aimed tile plus its 4 orthogonal neighbours (大- attack strategies).
    Cross,
    /// Every valid target inside the caster's reach (大- heal strategies); no aiming.
    AllInRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyDef {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub hanja: String,
    pub kind: StrategyKind,
    pub mp: i32,
    /// Reach from the caster (`range8`, `range12`, `range20`, `range28`, `self` or offsets).
    pub range: RangeSpec,
    pub area: Area,
    pub target: TargetSide,
    /// Element (`fire`, `water`, `earth`): the target tile must list it in `TerrainDef::elements`;
    /// rain blocks `fire` and boosts `water`. `None` = usable anywhere.
    #[serde(default)]
    pub element: Option<Id>,
    pub effects: Vec<Effect>,
    /// Animation / effect key for the renderer (e.g. `fire`, `water`, `rock`, `heal`, `confuse`).
    #[serde(default)]
    pub fx: String,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Raises ATK by `atk_pct` (only the officer's best weapon counts).
    Weapon,
    /// War manual: raises DEF by `def_pct`.
    Armor,
    /// Horse (move bonus) or regeneration treasure.
    Accessory,
    /// Used up on use (healing food, wine, strategy scrolls, class-up items).
    Consumable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub hanja: String,
    pub kind: ItemKind,
    /// Shop price; 0 = cannot be bought (treasure / event item). Sells for half.
    #[serde(default)]
    pub price: u32,
    #[serde(default)]
    pub desc: String,
    /// Class families that may equip it (empty = everyone). Ignored for consumables.
    #[serde(default)]
    pub families: Vec<Id>,
    /// Equipment: ATK bonus in percent (120 = +20%). 0 = none.
    #[serde(default)]
    pub atk_pct: i32,
    /// Equipment: DEF bonus in percent.
    #[serde(default)]
    pub def_pct: i32,
    /// Equipment: flat movement bonus (horses).
    #[serde(default)]
    pub move_bonus: i32,
    /// Equipment: percent of max HP regenerated at the start of each own phase.
    #[serde(default)]
    pub regen_hp: i32,
    /// Equipment: morale regenerated at the start of each own phase.
    #[serde(default)]
    pub regen_morale: i32,
    /// Consumable: effects applied to the target when used.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Consumable: casts this strategy (no MP cost, user's INT) instead of `effects`.
    #[serde(default)]
    pub strategy: Option<Id>,
    /// Consumable usable in battle (false for class-up items, which are used in camp).
    #[serde(default)]
    pub battle_use: bool,
    /// Icon key for the UI.
    #[serde(default)]
    pub icon: String,
}

// ---------------------------------------------------------------------------
// officers.toml
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipment {
    #[serde(default)]
    pub weapon: Option<Id>,
    #[serde(default)]
    pub armor: Option<Id>,
    #[serde(default)]
    pub accessory: Option<Id>,
}

impl Equipment {
    pub fn iter(&self) -> impl Iterator<Item = &Id> {
        [&self.weapon, &self.armor, &self.accessory]
            .into_iter()
            .flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficerDef {
    pub id: Id,
    /// Display name (Korean), e.g. `유비`.
    pub name: String,
    #[serde(default)]
    pub hanja: String,
    /// Courtesy name (字), e.g. `현덕`.
    #[serde(default)]
    pub courtesy: String,
    pub class: Id,
    /// Level when the officer first appears (enemy/ally default, or join level).
    pub level: u32,
    /// 무력 — drives attack and counter chance.
    #[serde(rename = "str")]
    pub strength: i32,
    /// 지력 — drives MP and strategy power/resistance.
    pub int: i32,
    /// 통솔 — drives defense.
    pub lead: i32,
    /// Portrait key (defaults to the officer id).
    #[serde(default)]
    pub portrait: Option<String>,
    #[serde(default)]
    pub equip: Equipment,
    /// The player's lord: if this unit retreats the battle is lost.
    #[serde(default)]
    pub lord: bool,
    /// Cannot use class-change items (Liu Bei in the original).
    #[serde(default)]
    pub fixed_class: bool,
    #[serde(default)]
    pub bio: String,
}

impl ItemDef {
    /// A consumable the army can use in battle (`battle_use`). Equipment never is, even with
    /// `battle_use` set: using it would not use it up.
    pub fn is_battle_item(&self) -> bool {
        self.kind == ItemKind::Consumable && self.battle_use
    }
}

impl OfficerDef {
    pub fn portrait_key(&self) -> &str {
        self.portrait.as_deref().unwrap_or(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ranges() {
        let n = |s: &str| RangeSpec::Named(s.into()).offsets().unwrap().len();
        assert_eq!(n("adjacent4"), 4);
        assert_eq!(n("adjacent8"), 8);
        assert_eq!(n("archer"), 8);
        assert_eq!(n("crossbow"), 16);
        assert_eq!(n("catapult"), 24);
        assert_eq!(n("self"), 1);
        assert_eq!(n("range8"), 9);
        assert_eq!(n("range12"), 13);
        assert_eq!(n("range20"), 21);
        assert_eq!(n("range28"), 29);
        assert!(RangeSpec::Named("nope".into()).offsets().is_none());
    }

    #[test]
    fn exp_lookup() {
        let t = vec![[-10, 1], [0, 6], [5, 12]];
        assert_eq!(GameRules::exp_lookup(&t, -20), 1);
        assert_eq!(GameRules::exp_lookup(&t, -1), 1);
        assert_eq!(GameRules::exp_lookup(&t, 0), 6);
        assert_eq!(GameRules::exp_lookup(&t, 30), 12);
    }

    #[test]
    fn effect_toml() {
        #[derive(Deserialize)]
        struct W {
            effects: Vec<Effect>,
        }
        let w: W = toml::from_str(
            r#"effects = [{ type = "damage", power = 200 }, { type = "status", status = "confused", turns = 1 }, { type = "change_class", to = "archer" }]"#,
        )
        .unwrap();
        assert_eq!(w.effects[0], Effect::Damage { power: 200 });
        assert_eq!(
            w.effects[1],
            Effect::Status {
                status: StatusKind::Confused,
                turns: 1
            }
        );
        assert_eq!(
            w.effects[2],
            Effect::ChangeClass {
                to: "archer".into()
            }
        );
    }
}
