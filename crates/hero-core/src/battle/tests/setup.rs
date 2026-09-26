//! `BattleState::new`, `begin` and serialization.

use crate::battle::testkit::*;
use crate::battle::{BattleError, BattleEvent, BattleState, UnitState, Weather};
use crate::battledef::{AiMode, EventAction, EventDef, Side, TreasureDef, Trigger, UnitSpawn};
use crate::data::Equipment;
use crate::geom::Pos;

fn spawn(side: Side, pos: Pos) -> UnitSpawn {
    UnitSpawn {
        side,
        officer: None,
        name: None,
        class: Some("infantry".into()),
        level: Some(3),
        stats: None,
        pos,
        ai: AiMode::Aggressive,
        ai_target: None,
        ai_pos: None,
        commander: false,
        tag: None,
        group: None,
        equip: None,
        drop: None,
    }
}

#[test]
fn deployed_officers_take_slots_in_order_with_campaign_progress() {
    let pack = pack(OPEN_MAP);
    let mut guan = officer_state(&pack, "guan_yu");
    guan.level = 12;
    guan.exp = 40;
    guan.equip = Equipment {
        weapon: Some("sword".into()),
        armor: None,
        accessory: Some("horse".into()),
    };
    let liu = officer_state(&pack, "liu_bei");
    let camp = campaign(vec![liu, guan], &["guan_yu", "liu_bei"]);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();

    assert_eq!(st.units.len(), 2);
    let g = &st.units[0];
    assert_eq!(g.officer.as_deref(), Some("guan_yu"));
    assert_eq!(g.pos, p(0, 0));
    assert_eq!((g.level, g.exp), (12, 40));
    assert_eq!(g.equip.weapon.as_deref(), Some("sword"));
    // cavalry: hp 600 + 20 * 11; MP (12 + 10) * 70 / 40 = 38
    assert_eq!((g.hp, g.max_hp), (820, 820));
    assert_eq!((g.mp, g.max_mp), (38, 38));
    assert_eq!(g.morale, 100);
    assert!(!g.lord);
    assert_eq!(g.portrait.as_deref(), Some("guan_yu"));
    let l = &st.units[1];
    assert_eq!(l.pos, p(1, 0));
    assert!(l.lord);
    assert_eq!(st.move_points(&pack, 0), 8, "cavalry 6 + horse 2");
    assert_eq!(
        (st.turn, st.phase, st.weather),
        (1, Side::Player, Weather::Clear)
    );
}

#[test]
fn fallback_deploys_required_then_lord_then_roster_up_to_max() {
    let mut def = battle(OPEN_MAP);
    def.deploy.max = 3;
    def.deploy.required = vec!["jian_yong".into()];
    def.deploy.forbidden = vec!["zhang_fei".into()];
    let pack = pack_with(def);
    let roster = ["guan_yu", "zhang_fei", "zhang_bao", "liu_bei", "jian_yong"]
        .iter()
        .map(|id| officer_state(&pack, id))
        .collect();
    let st = BattleState::new(&pack, BATTLE, &campaign(roster, &[]), 1).unwrap();
    let ids: Vec<_> = st
        .units
        .iter()
        .map(|u| u.officer.clone().unwrap())
        .collect();
    assert_eq!(ids, ["jian_yong", "liu_bei", "guan_yu"]);
}

#[test]
fn spawns_use_class_or_officer_stats_and_groups_start_hidden() {
    let mut def = battle(OPEN_MAP);
    let generic = spawn(Side::Enemy, p(5, 5));
    let mut custom = spawn(Side::Enemy, p(6, 5));
    custom.stats = Some([80, 10, 20]);
    custom.name = Some("rebel".into());
    custom.commander = true;
    custom.tag = Some("boss".into());
    custom.drop = Some("bean".into());
    let mut named = spawn(Side::Ally, p(4, 4));
    named.officer = Some("zhang_bao".into());
    named.class = None;
    named.level = None;
    named.ai = AiMode::Guard;
    let mut rein = spawn(Side::Enemy, p(7, 7));
    rein.group = Some("wave".into());
    // A reinforcement listed first still comes after the units that start on the map.
    def.units = vec![rein, generic, custom, named];
    let pack = pack_with(def);
    let st = state(&pack);

    let u = &st.units[0];
    assert_eq!(
        (u.side, u.name.as_str(), u.level),
        (Side::Enemy, "infantry", 3)
    );
    assert_eq!(
        [u.strength, u.int, u.lead],
        [50, 30, 50],
        "class generic stats"
    );
    assert_eq!(u.hp, 540);
    let c = &st.units[1];
    assert_eq!([c.strength, c.int, c.lead], [80, 10, 20]);
    assert_eq!(c.name, "rebel");
    assert!(c.commander);
    assert_eq!(c.drop.as_deref(), Some("bean"));
    assert_eq!(st.find_unit("boss"), Some(1));
    let z = &st.units[2];
    assert_eq!((z.class.as_str(), z.level, z.strength), ("bandit", 8, 70));
    assert_eq!(z.ai_pos, Some(p(4, 4)), "guard defaults to its spawn tile");
    assert_eq!(st.find_unit("zhang_bao"), Some(2));
    let r = &st.units[3];
    assert_eq!(r.state, UnitState::Hidden);
    assert_eq!(st.unit_at(p(7, 7)), None);
}

#[test]
fn setup_errors() {
    let pack = pack(OPEN_MAP);
    assert_eq!(
        BattleState::new(&pack, "nope", &campaign(Vec::new(), &[]), 1),
        Err(BattleError::UnknownBattle("nope".into()))
    );
    let camp = campaign(vec![officer_state(&pack, "liu_bei")], &["guan_yu"]);
    assert!(matches!(
        BattleState::new(&pack, BATTLE, &camp, 1),
        Err(BattleError::Setup(_))
    ));

    let mut def = battle(OPEN_MAP);
    def.units = vec![spawn(Side::Enemy, p(0, 0))];
    let pack = pack_with(def);
    let camp = campaign(vec![officer_state(&pack, "liu_bei")], &["liu_bei"]);
    let err = BattleState::new(&pack, BATTLE, &camp, 1).unwrap_err();
    assert!(
        matches!(err, BattleError::Setup(ref m) if m.contains("both start")),
        "{err}"
    );

    let mut def = battle(OPEN_MAP);
    def.deploy.slots.truncate(1);
    let pack = pack_with(def);
    let roster = vec![
        officer_state(&pack, "liu_bei"),
        officer_state(&pack, "guan_yu"),
    ];
    let camp = campaign(roster, &["liu_bei", "guan_yu"]);
    assert!(matches!(
        BattleState::new(&pack, BATTLE, &camp, 1),
        Err(BattleError::Setup(_))
    ));
}

#[test]
fn state_copies_inventory_and_sizes_tracking_arrays() {
    let mut def = battle(OPEN_MAP);
    def.treasures = vec![
        TreasureDef {
            pos: p(3, 3),
            item: Some("bean".into()),
            gold: 0,
        };
        2
    ];
    def.events = vec![
        EventDef {
            trigger: Trigger::TurnStart {
                turn: 5,
                side: Side::Enemy,
            },
            once: true,
            actions: vec![EventAction::GiveGold { amount: 1 }],
        };
        3
    ];
    let pack = pack_with(def);
    let mut camp = campaign(Vec::new(), &[]);
    camp.inventory.insert("bean".into(), 2);
    let st = BattleState::new(&pack, BATTLE, &camp, 1).unwrap();
    assert_eq!(st.treasures_taken, vec![false; 2]);
    assert_eq!(st.fired, vec![false; 3]);
    assert_eq!(st.inventory.get("bean"), Some(&2));
}

#[test]
fn begin_plays_intro_then_starts_turn_one() {
    let mut def = battle(OPEN_MAP);
    def.intro = Some("intro".into());
    def.events = vec![EventDef {
        trigger: Trigger::TurnStart {
            turn: 1,
            side: Side::Player,
        },
        once: true,
        actions: vec![EventAction::Drama { scene: "t1".into() }],
    }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    let ev = st.begin(&pack);
    assert_eq!(
        ev,
        vec![
            BattleEvent::Drama {
                scene: "intro".into()
            },
            BattleEvent::PhaseStart {
                side: Side::Player,
                turn: 1
            },
            BattleEvent::Drama { scene: "t1".into() },
        ]
    );
    assert_eq!((st.turn, st.phase), (1, Side::Player));
}

#[test]
fn battle_state_round_trips_through_json() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 5, p(3, 3));
    let e = add(&mut st, &pack, Side::Enemy, "bandit", 5, p(3, 4));
    st.units[e].tag = Some("boss".into());
    st.inventory.insert("bean".into(), 1);
    st.begin(&pack);
    st.apply(&pack, crate::battle::Action::Attack { unit: a, target: e })
        .unwrap();
    st.flags.insert("flag".into(), 3);
    let json = serde_json::to_string(&st).unwrap();
    let back: BattleState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, st);
    // The RNG state survives: both continue identically.
    let (mut x, mut y) = (st.clone(), back);
    assert_eq!(x.rng.next_u64(), y.rng.next_u64());
}
