//! AI refinements for the player's side (see `battle/ai.rs`): lord safety, protecting the
//! lord, careful player units and scripted battle endings — and that enemies stay aggressive.

use crate::battle::testkit::*;
use crate::battle::{Action, BattleState, Outcome, UnitId};
use crate::battledef::{AiMode, Condition, EventAction, EventDef, Side, Trigger};
use crate::geom::Pos;
use crate::pack::Pack;

/// A unit without MP (the testkit classes know strategies at level 1).
fn unit(
    st: &mut BattleState,
    pack: &Pack,
    side: Side,
    class: &str,
    level: u32,
    pos: Pos,
) -> UnitId {
    let id = add(st, pack, side, class, level, pos);
    st.units[id].mp = 0;
    id
}

/// An enemy that never moves, so the tiles it threatens are exactly its neighbours.
fn post(st: &mut BattleState, pack: &Pack, level: u32, pos: Pos) -> UnitId {
    let id = unit(st, pack, Side::Enemy, "infantry", level, pos);
    st.units[id].ai = AiMode::Hold;
    id
}

fn destination(st: &BattleState, id: UnitId, plan: &[Action]) -> Pos {
    plan.iter()
        .find_map(|a| match a {
            Action::Move { to, .. } => Some(*to),
            _ => None,
        })
        .unwrap_or(st.units[id].pos)
}

fn attacks(plan: &[Action]) -> Option<UnitId> {
    plan.iter().find_map(|a| match a {
        Action::Attack { target, .. } => Some(*target),
        _ => None,
    })
}

#[test]
fn a_lord_keeps_off_tiles_where_it_could_be_defeated() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let lord = unit(&mut st, &pack, Side::Player, "infantry", 5, p(3, 0));
    st.units[lord].lord = true;
    let foe = post(&mut st, &pack, 10, p(3, 3));
    let hit = st.forecast_attack(&pack, foe, lord).damage;
    assert_eq!(hit, 305);
    assert_eq!(st.forecast_attack(&pack, lord, foe).damage, 122);

    // One hit would defeat it: no attack, and it does not stop next to the foe.
    st.units[lord].hp = 300;
    let plan = st.ai_actions(&pack, lord);
    assert_eq!(attacks(&plan), None, "{plan:?}");
    assert!(
        destination(&st, lord, &plan).manhattan(p(3, 3)) > 1,
        "{plan:?}"
    );

    // Against a weak foe it fights, as long as two of its hits would not defeat it.
    st.units[foe].level = 1;
    let hit = st.forecast_attack(&pack, foe, lord).damage;
    assert_eq!(hit, 85);
    st.units[lord].hp = 2 * hit;
    let plan = st.ai_actions(&pack, lord);
    assert_eq!(attacks(&plan), None, "two hits would defeat it: {plan:?}");
    st.units[lord].hp = 2 * hit + 1;
    let plan = st.ai_actions(&pack, lord);
    assert_eq!(attacks(&plan), Some(foe), "{plan:?}");
}

#[test]
fn a_lord_with_nothing_to_do_stays_with_its_army() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let lord = unit(&mut st, &pack, Side::Player, "infantry", 1, p(4, 4));
    let friend = unit(&mut st, &pack, Side::Player, "infantry", 1, p(0, 4));
    unit(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 0));

    // An ordinary unit heads for the enemy...
    let to = destination(&st, lord, &st.ai_actions(&pack, lord));
    assert!(to.manhattan(p(7, 0)) < p(4, 4).manhattan(p(7, 0)), "{to:?}");
    // ...the lord joins its army.
    st.units[lord].lord = true;
    let plan = st.ai_actions(&pack, lord);
    assert_eq!(
        plan,
        vec![
            Action::Move {
                unit: lord,
                to: p(1, 4)
            },
            Action::Wait { unit: lord }
        ]
    );
    assert_eq!(p(1, 4).manhattan(st.units[friend].pos), 1);
}

#[test]
fn units_go_for_the_foe_that_threatens_their_lord() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let lord = unit(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    // Two identical foes; the one created first wins ties.
    let far = post(&mut st, &pack, 1, p(6, 5));
    let near = post(&mut st, &pack, 1, p(1, 0));
    let rider = unit(&mut st, &pack, Side::Player, "cavalry", 1, p(4, 3));
    assert_eq!(
        st.forecast_attack(&pack, rider, far),
        st.forecast_attack(&pack, rider, near)
    );
    assert_eq!(
        attacks(&st.ai_actions(&pack, rider)),
        Some(far),
        "no lord to protect"
    );
    st.units[lord].lord = true;
    let plan = st.ai_actions(&pack, rider);
    assert_eq!(attacks(&plan), Some(near), "{plan:?}");
}

#[test]
fn enemies_prefer_the_player_lord() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let other = unit(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let lord = unit(&mut st, &pack, Side::Player, "infantry", 1, p(4, 3));
    let foe = unit(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 3));
    st.phase = Side::Enemy;
    assert_eq!(attacks(&st.ai_actions(&pack, foe)), Some(other));
    st.units[lord].lord = true;
    assert_eq!(
        st.ai_actions(&pack, foe),
        vec![Action::Attack {
            unit: foe,
            target: lord
        }]
    );
}

#[test]
fn player_units_avoid_being_defeated_but_enemies_stay_aggressive() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let mine = unit(&mut st, &pack, Side::Player, "infantry", 1, p(3, 1));
    let foe = post(&mut st, &pack, 1, p(3, 3));
    assert_eq!(st.forecast_attack(&pack, foe, mine).damage, 134);

    // Healthy, it attacks; a hit from the foe next phase would defeat it at 100 HP.
    assert_eq!(attacks(&st.ai_actions(&pack, mine)), Some(foe));
    st.units[mine].hp = 100;
    let plan = st.ai_actions(&pack, mine);
    assert_eq!(attacks(&plan), None, "{plan:?}");
    assert!(
        destination(&st, mine, &plan).manhattan(p(3, 3)) > 1,
        "{plan:?}"
    );

    // The same situation from the enemy's side: it attacks anyway.
    let mut st = state(&pack);
    let foe = unit(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 1));
    let target = unit(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    st.units[foe].hp = 100;
    st.phase = Side::Enemy;
    assert_eq!(attacks(&st.ai_actions(&pack, foe)), Some(target));
}

/// The test battle with a duel-style event: `hero` next to `boss` wins the battle.
fn duel_pack(extra: Vec<EventDef>) -> Pack {
    let mut def = battle(OPEN_MAP);
    def.events = vec![EventDef {
        trigger: Trigger::Adjacent {
            a: "hero".into(),
            b: "boss".into(),
        },
        once: true,
        stage: None,
        when: Vec::new(),
        actions: vec![
            EventAction::Drama {
                scene: "duel".into(),
            },
            EventAction::Victory,
        ],
    }];
    def.events.extend(extra);
    pack_with(def)
}

fn tagged(st: &mut BattleState, id: UnitId, tag: &str) -> UnitId {
    st.units[id].tag = Some(tag.into());
    id
}

#[test]
fn the_player_side_takes_a_scripted_victory() {
    let pack = duel_pack(Vec::new());
    let mut st = state(&pack);
    let hero = unit(&mut st, &pack, Side::Player, "cavalry", 1, p(0, 0));
    tagged(&mut st, hero, "hero");
    let boss = post(&mut st, &pack, 10, p(5, 0));
    tagged(&mut st, boss, "boss");
    unit(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 2));

    let plan = st.ai_actions(&pack, hero);
    assert_eq!(
        plan,
        vec![
            Action::Move {
                unit: hero,
                to: p(4, 0)
            },
            Action::Wait { unit: hero }
        ]
    );
    st.run_ai_phase(&pack);
    assert_eq!(st.outcome, Some(Outcome::Victory));

    // Enemy units do not play the script: the boss's side never seeks the hero out for it.
    let mut st = state(&pack);
    let hero = unit(&mut st, &pack, Side::Player, "cavalry", 1, p(0, 0));
    tagged(&mut st, hero, "hero");
    let boss = unit(&mut st, &pack, Side::Enemy, "infantry", 10, p(0, 6));
    tagged(&mut st, boss, "boss");
    let bait = unit(&mut st, &pack, Side::Player, "infantry", 1, p(4, 6));
    st.units[bait].hp = 50;
    st.phase = Side::Enemy;
    assert_eq!(attacks(&st.ai_actions(&pack, boss)), Some(bait));
}

#[test]
fn the_player_side_avoids_a_scripted_defeat() {
    let trap = EventDef {
        trigger: Trigger::Reach {
            who: Some("hero".into()),
            pos: p(4, 0),
            radius: 0,
            to: None,
        },
        once: true,
        stage: None,
        when: Vec::new(),
        actions: vec![EventAction::Defeat],
    };
    let pack = duel_pack(vec![trap]);
    let mut st = state(&pack);
    let hero = unit(&mut st, &pack, Side::Player, "cavalry", 1, p(0, 0));
    tagged(&mut st, hero, "hero");
    let boss = post(&mut st, &pack, 10, p(5, 0));
    tagged(&mut st, boss, "boss");
    // (4, 0) would also win, but the defeat takes precedence: (5, 1) wins safely.
    let plan = st.ai_actions(&pack, hero);
    assert_eq!(
        plan,
        vec![
            Action::Move {
                unit: hero,
                to: p(5, 1)
            },
            Action::Wait { unit: hero }
        ]
    );
    st.run_ai_phase(&pack);
    assert_eq!(st.outcome, Some(Outcome::Victory));
}

#[test]
fn an_idle_unit_heads_for_its_scripted_objective() {
    let pack = duel_pack(Vec::new());
    let mut st = state(&pack);
    let hero = unit(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let decoy = post(&mut st, &pack, 1, p(0, 0));
    let boss = post(&mut st, &pack, 1, p(7, 7));
    tagged(&mut st, boss, "boss");
    // Nothing is in reach; the decoy is the nearest hostile unit.
    let to = destination(&st, hero, &st.ai_actions(&pack, hero));
    assert!(to.manhattan(st.units[decoy].pos) < 6, "{to:?}");
    tagged(&mut st, hero, "hero");
    let to = destination(&st, hero, &st.ai_actions(&pack, hero));
    assert_eq!(
        to.manhattan(p(7, 7)),
        8 - 4,
        "a full move towards the boss: {to:?}"
    );
}

#[test]
fn reach_objectives_lead_onto_tiles_the_unit_can_stand_on() {
    // The objective column x = 7 is river for y = 0..=4; only (7, 5)–(7, 7) can be entered.
    // Standing next to the river part must not count as having arrived.
    let rows = "
.......~
.......~
.......~
.......~
.......~
........
........
........";
    for (pos, radius, to) in [
        (p(7, 0), 0, Some(p(7, 7))),
        // The same trap with a radius: the centre is river, the tiles around it are not.
        (p(7, 2), 3, None),
    ] {
        let mut def = battle(rows);
        def.victory = vec![Condition::Reach {
            who: Some("hero".into()),
            pos,
            radius,
            to,
        }];
        let pack = pack_with(def);
        let mut st = state(&pack);
        let hero = unit(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
        tagged(&mut st, hero, "hero");
        post(&mut st, &pack, 1, p(0, 7));
        for _ in 0..12 {
            if st.outcome.is_some() {
                break;
            }
            st.run_ai_phase(&pack);
        }
        assert_eq!(
            st.outcome,
            Some(Outcome::Victory),
            "{pos:?} {radius} {to:?}: hero at {:?}",
            st.units[hero].pos
        );
    }
}
