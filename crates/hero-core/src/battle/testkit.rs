//! Test fixtures: a small pack built in code and helpers to place units.
//!
//! Terrain glyphs: `.` plain (fire, water), `T` forest (defence 20, fire +25%, no horses),
//! `^` mountain (defence 30, earth, bandits only), `~` river (impassable), `v` village
//! (heals 10% HP and 10 morale, no strategy elements).
//!
//! Classes: `infantry`, `heavy_infantry` (promotion of infantry), `cavalry` (provokes
//! counters), `archer`, `bandit` (counters, mountains), `band` (MP aura, strategy guard,
//! support EXP 12), `guard` (the RULES.md worked example: atk 16 / def 12).

use super::stats::{max_hp, max_mp};
use super::{BattleState, Unit, UnitId, UnitState};
use crate::battledef::{AiMode, BattleDef, Condition, DeployDef, EventDef, MapDef, Side};
use crate::campaign::{CampaignDef, CampaignState, OfficerState};
use crate::data::{
    Area, ClassDef, Effect, Equipment, GameRules, ItemDef, ItemKind, Learn, OfficerDef, Promotion,
    RangeSpec, StatusKind, StrategyDef, StrategyFormulas, StrategyKind, TargetSide, TerrainDef,
    WeatherChances,
};
use crate::geom::{Dir, Pos};
use crate::pack::{Pack, PackFiles, PackLayer, PackManifest, Presentation, RulesFiles};
use std::collections::BTreeMap;

pub const BATTLE: &str = "test";

fn terrain(
    id: &str,
    glyph: char,
    defense: i32,
    elements: &[&str],
    costs: &[(&str, u8)],
) -> TerrainDef {
    TerrainDef {
        id: id.into(),
        name: id.into(),
        glyph,
        defense,
        heal_hp: 0,
        heal_morale: 0,
        elements: elements.iter().map(|s| s.to_string()).collect(),
        boost: Vec::new(),
        cost: costs.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        tile: None,
    }
}

fn terrains() -> Vec<TerrainDef> {
    let mut forest = terrain("forest", 'T', 20, &["fire"], &[("foot", 2), ("bandit", 1)]);
    forest.boost = vec!["fire".into()];
    let mut village = terrain(
        "village",
        'v',
        0,
        &[],
        &[("foot", 1), ("horse", 1), ("bandit", 1)],
    );
    village.heal_hp = 10;
    village.heal_morale = 10;
    vec![
        terrain(
            "plain",
            '.',
            0,
            &["fire", "water"],
            &[("foot", 1), ("horse", 1), ("bandit", 1)],
        ),
        forest,
        terrain("mountain", '^', 30, &["earth"], &[("bandit", 2)]),
        terrain("river", '~', 0, &[], &[]),
        village,
    ]
}

/// A class with neutral defaults; tests adjust fields as needed.
pub fn class(id: &str, family: &str, move_points: u8, move_type: &str, range: &str) -> ClassDef {
    ClassDef {
        id: id.into(),
        name: id.into(),
        hanja: String::new(),
        family: family.into(),
        tier: 1,
        move_points,
        move_type: move_type.into(),
        range: RangeSpec::Named(range.into()),
        atk: 10,
        def: 10,
        hp: 500,
        hp_growth: 20,
        generic: [50, 30, 50],
        strategies: Vec::new(),
        promote: None,
        sprite: id.into(),
        can_counter: false,
        provokes_counter: false,
        strategy_guard: false,
        mp_aura: false,
        support_exp: None,
        desc: String::new(),
    }
}

fn learn(level: u32, id: &str) -> Learn {
    Learn {
        level,
        id: id.into(),
    }
}

fn classes() -> BTreeMap<String, ClassDef> {
    let mut infantry = class("infantry", "infantry", 4, "foot", "adjacent4");
    infantry.strategies = vec![learn(1, "fire"), learn(3, "flood"), learn(5, "confuse")];
    infantry.promote = Some(Promotion {
        to: "heavy_infantry".into(),
        level: 20,
        item: "spear".into(),
    });
    let mut heavy = class("heavy_infantry", "infantry", 4, "foot", "adjacent4");
    heavy.tier = 2;
    heavy.strategies = vec![learn(20, "big_fire")];
    let mut cavalry = class("cavalry", "cavalry", 6, "horse", "adjacent4");
    cavalry.atk = 12;
    cavalry.def = 8;
    cavalry.hp = 600;
    cavalry.provokes_counter = true;
    cavalry.strategies = vec![learn(1, "provoke")];
    let mut archer = class("archer", "archer", 4, "foot", "archer");
    archer.def = 6;
    archer.hp = 450;
    let mut bandit = class("bandit", "bandit", 5, "bandit", "adjacent4");
    bandit.can_counter = true;
    bandit.provokes_counter = true;
    bandit.strategies = vec![learn(1, "rock")];
    let mut band = class("band", "band", 4, "foot", "adjacent4");
    band.atk = 4;
    band.def = 4;
    band.hp = 300;
    band.mp_aura = true;
    band.strategy_guard = true;
    band.support_exp = Some(12);
    band.generic = [10, 70, 10];
    band.strategies = vec![learn(1, "heal"), learn(1, "cheer"), learn(1, "heal_all")];
    let mut guard = class("guard", "cavalry", 7, "horse", "adjacent4");
    guard.atk = 16;
    guard.def = 12;
    [infantry, heavy, cavalry, archer, bandit, band, guard]
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect()
}

fn strategy(
    id: &str,
    mp: i32,
    area: Area,
    target: TargetSide,
    element: Option<&str>,
    effects: Vec<Effect>,
) -> StrategyDef {
    StrategyDef {
        id: id.into(),
        name: id.into(),
        hanja: String::new(),
        kind: match (target, effects.first()) {
            (TargetSide::Enemy, Some(Effect::Damage { .. })) => StrategyKind::Attack,
            (TargetSide::Ally, Some(Effect::Heal { .. })) => StrategyKind::Heal,
            _ => StrategyKind::Support,
        },
        mp,
        range: RangeSpec::Named("range8".into()),
        area,
        target,
        element: element.map(str::to_string),
        effects,
        fx: String::new(),
        desc: String::new(),
    }
}

fn strategies() -> BTreeMap<String, StrategyDef> {
    use Area::*;
    use TargetSide::*;
    [
        strategy(
            "fire",
            4,
            Single,
            Enemy,
            Some("fire"),
            vec![Effect::Damage { power: 200 }],
        ),
        strategy(
            "big_fire",
            16,
            Cross,
            Enemy,
            Some("fire"),
            vec![Effect::Damage { power: 200 }],
        ),
        strategy(
            "flood",
            6,
            Single,
            Enemy,
            Some("water"),
            vec![Effect::Damage { power: 300 }],
        ),
        strategy(
            "rock",
            8,
            Single,
            Enemy,
            Some("earth"),
            vec![Effect::Damage { power: 400 }],
        ),
        strategy(
            "confuse",
            8,
            Single,
            Enemy,
            None,
            vec![Effect::Status {
                status: StatusKind::Confused,
                turns: 1,
            }],
        ),
        strategy(
            "provoke",
            4,
            Single,
            Enemy,
            None,
            vec![Effect::Morale { amount: -20 }],
        ),
        strategy(
            "heal",
            6,
            Single,
            Ally,
            None,
            vec![Effect::Heal { power: 200 }],
        ),
        strategy(
            "heal_all",
            24,
            AllInRange,
            Ally,
            None,
            vec![Effect::Heal { power: 200 }],
        ),
        strategy(
            "cheer",
            4,
            Single,
            Ally,
            None,
            vec![Effect::Morale { amount: 20 }],
        ),
    ]
    .into_iter()
    .map(|s| (s.id.clone(), s))
    .collect()
}

fn item(id: &str, kind: ItemKind) -> ItemDef {
    ItemDef {
        id: id.into(),
        name: id.into(),
        hanja: String::new(),
        kind,
        price: 0,
        resale_price: None,
        desc: String::new(),
        families: Vec::new(),
        atk_pct: 0,
        def_pct: 0,
        move_bonus: 0,
        regen_hp: 0,
        regen_morale: 0,
        effects: Vec::new(),
        strategy: None,
        battle_use: false,
        icon: String::new(),
    }
}

fn items() -> BTreeMap<String, ItemDef> {
    let mut bean = item("bean", ItemKind::Consumable);
    bean.battle_use = true;
    bean.effects = vec![Effect::Heal { power: 300 }];
    let mut wine = item("wine", ItemKind::Consumable);
    wine.battle_use = true;
    wine.effects = vec![Effect::Morale { amount: 30 }];
    let mut scroll = item("fire_scroll", ItemKind::Consumable);
    scroll.battle_use = true;
    scroll.strategy = Some("fire".into());
    let mut spear = item("spear", ItemKind::Consumable);
    spear.effects = vec![Effect::Promote];
    let mut sword = item("sword", ItemKind::Weapon);
    sword.atk_pct = 120;
    let mut book = item("book", ItemKind::Armor);
    book.def_pct = 120;
    let mut horse = item("horse", ItemKind::Accessory);
    horse.move_bonus = 2;
    let mut jade = item("jade", ItemKind::Accessory);
    jade.regen_hp = 10;
    jade.regen_morale = 5;
    [bean, wine, scroll, spear, sword, book, horse, jade]
        .into_iter()
        .map(|i| (i.id.clone(), i))
        .collect()
}

fn officer(id: &str, class: &str, level: u32, stats: [i32; 3], lord: bool) -> OfficerDef {
    OfficerDef {
        id: id.into(),
        name: id.into(),
        hanja: String::new(),
        courtesy: String::new(),
        class: class.into(),
        level,
        strength: stats[0],
        int: stats[1],
        lead: stats[2],
        portrait: None,
        equip: Equipment::default(),
        lord,
        fixed_class: false,
        bio: String::new(),
    }
}

fn officers() -> BTreeMap<String, OfficerDef> {
    [
        officer("liu_bei", "infantry", 5, [70, 70, 80], true),
        officer("guan_yu", "cavalry", 5, [95, 70, 90], false),
        officer("zhang_fei", "cavalry", 5, [98, 30, 60], false),
        officer("jian_yong", "band", 5, [20, 70, 30], false),
        officer("zhang_bao", "bandit", 8, [70, 60, 60], false),
    ]
    .into_iter()
    .map(|o| (o.id.clone(), o))
    .collect()
}

pub fn rules() -> GameRules {
    let mut affinity: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::new();
    let mut set = |a: &str, d: &str, pct: i32| {
        affinity.entry(a.into()).or_default().insert(d.into(), pct);
    };
    // infantry & bandit > archer > cavalry > infantry & bandit
    for (strong, weak) in [
        ("infantry", "archer"),
        ("bandit", "archer"),
        ("archer", "cavalry"),
        ("cavalry", "infantry"),
        ("cavalry", "bandit"),
    ] {
        set(strong, weak, 75);
        set(weak, strong, 125);
    }
    GameRules {
        level_cap: 50,
        exp_per_level: 100,
        gold_cap: 999_999,
        mp_cap: 255,
        morale_start: 100,
        morale_loss_pct: 100,
        confuse_morale: 30,
        exp_attack: vec![[-10, 1], [0, 6], [5, 12]],
        exp_kill: vec![[-10, 4], [0, 38], [1, 40], [5, 48]],
        exp_commander: 20,
        exp_support: 8,
        affinity,
        counter_divisor: 150,
        counter_damage_pct: 50,
        weather: WeatherChances {
            clear: 100,
            cloudy: 0,
            rain: 0,
        },
        strategy_formulas: StrategyFormulas::Engine,
    }
}

/// A battle on `rows` with no units, one generous turn limit and `defeat_all` as victory.
pub fn battle(rows: &str) -> BattleDef {
    BattleDef {
        id: BATTLE.into(),
        name: "test battle".into(),
        location: String::new(),
        objective: "win".into(),
        bgm: None,
        bgm_enemy: None,
        turn_limit: 30,
        map: MapDef {
            rows: rows.into(),
            ..MapDef::default()
        },
        deploy: DeployDef {
            max: 4,
            required: Vec::new(),
            forbidden: Vec::new(),
            slots: vec![
                Pos::new(0, 0),
                Pos::new(1, 0),
                Pos::new(2, 0),
                Pos::new(3, 0),
            ],
        },
        units: Vec::new(),
        victory: vec![Condition::DefeatAll],
        defeat: Vec::new(),
        bonus: None,
        events: Vec::<EventDef>::new(),
        treasures: Vec::new(),
        reward_gold: 0,
        intro: None,
        outro: None,
    }
}

pub const OPEN_MAP: &str = "
........
........
........
........
........
........
........
........";

pub fn pack_with(def: BattleDef) -> Pack {
    let mut battles = BTreeMap::new();
    battles.insert(def.id.clone(), def);
    let manifest = PackManifest {
        id: "testkit".into(),
        name: "testkit".into(),
        version: "0".into(),
        authors: Vec::new(),
        license: String::new(),
        description: String::new(),
        extends: None,
        presentation: Presentation::default(),
        rules: RulesFiles::default(),
        officers: None,
        campaign: None,
        battles: Vec::new(),
        dramas: Vec::new(),
        maps: Vec::new(),
    };
    Pack {
        layers: vec![PackLayer {
            dir: String::new(),
            manifest: manifest.clone(),
        }],
        manifest,
        files: PackFiles::default(),
        rules: rules(),
        terrain: terrains(),
        classes: classes(),
        strategies: strategies(),
        items: items(),
        officers: officers(),
        battles,
        maps: BTreeMap::new(),
        scenes: BTreeMap::new(),
        parent_scenes: Default::default(),
        campaign: CampaignDef {
            title: "test".into(),
            start: "start".into(),
            starting_officers: Vec::new(),
            starting_gold: 0,
            starting_items: BTreeMap::new(),
            nodes: Vec::new(),
        },
    }
}

/// Pack whose test battle uses `rows`.
pub fn pack(rows: &str) -> Pack {
    pack_with(battle(rows))
}

pub fn campaign(roster: Vec<OfficerState>, deployed: &[&str]) -> CampaignState {
    CampaignState {
        node: "start".into(),
        roster,
        inventory: BTreeMap::new(),
        gold: 0,
        flags: BTreeMap::new(),
        deployed: deployed.iter().map(|s| s.to_string()).collect(),
        battles_won: Vec::new(),
        play_seconds: 0,
        pending_growth: BTreeMap::new(),
        difficulty: Default::default(),
        free_edit: false,
        extended_rules: false,
    }
}

/// Roster entry for a pack officer at its default level and class.
pub fn officer_state(pack: &Pack, id: &str) -> OfficerState {
    let o = &pack.officers[id];
    OfficerState {
        id: id.into(),
        class: o.class.clone(),
        level: o.level,
        exp: 0,
        strength: o.strength,
        int: o.int,
        lead: o.lead,
        equip: o.equip.clone(),
        away: false,
    }
}

/// Empty battle state (no units) for the pack's test battle.
pub fn state(pack: &Pack) -> BattleState {
    BattleState::new(pack, BATTLE, &campaign(Vec::new(), &[]), 7).expect("test battle builds")
}

/// Place a generic unit (class `generic` stats, full HP/MP, morale 100). Returns its id.
pub fn add(
    st: &mut BattleState,
    pack: &Pack,
    side: Side,
    class_id: &str,
    level: u32,
    pos: Pos,
) -> UnitId {
    let c = &pack.classes[class_id];
    let [strength, int, lead] = c.generic;
    let id = st.units.len();
    let hp = max_hp(c, level);
    let mp = max_mp(&pack.rules, level, int);
    st.units.push(Unit {
        id,
        side,
        officer: None,
        name: format!("{class_id}#{id}"),
        class: class_id.into(),
        level,
        exp: 0,
        strength,
        int,
        lead,
        hp,
        max_hp: hp,
        mp,
        max_mp: mp,
        morale: 100,
        pos,
        facing: Dir::Down,
        moved: false,
        acted: false,
        equip: Equipment::default(),
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
    });
    id
}

/// Set STR/INT/LEAD and recompute max MP (MP refilled).
pub fn set_stats(st: &mut BattleState, pack: &Pack, id: UnitId, stats: [i32; 3]) {
    let u = &mut st.units[id];
    [u.strength, u.int, u.lead] = stats;
    u.max_mp = max_mp(&pack.rules, u.level, u.int);
    u.mp = u.max_mp;
}

pub fn p(x: i32, y: i32) -> Pos {
    Pos::new(x, y)
}
