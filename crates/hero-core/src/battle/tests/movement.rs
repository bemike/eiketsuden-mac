//! Movement ranges (§3), the move action and attack shapes (§4).

use crate::battle::testkit::*;
use crate::battle::{Action, ActionError, BattleEvent};
use crate::battledef::{Side, TreasureDef};
use crate::data::RangeSpec;
use crate::geom::Dir;

#[test]
fn terrain_costs_and_impassable_tiles() {
    let pack = pack(
        "
        .....
        .TT..
        .~...
        .....",
    );
    let mut st = state(&pack);
    let inf = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let r = st.movement_range(&pack, inf);
    assert_eq!(r.origin, p(0, 0));
    assert_eq!(r.tiles[&p(0, 0)].cost, 0);
    assert_eq!(r.tiles[&p(3, 0)].cost, 3);
    assert_eq!(r.tiles[&p(4, 0)].cost, 4);
    assert_eq!(r.tiles[&p(1, 1)].cost, 3, "forest costs 2 for foot");
    assert!(!r.contains(p(1, 2)), "river is impassable");
    assert!(!r.contains(p(4, 1)), "cost 5 is out of range");
    // Equal-cost tie: the path expanded first (down before right) wins.
    assert_eq!(r.path_to(p(1, 1)).unwrap(), vec![p(0, 0), p(0, 1), p(1, 1)]);
    assert_eq!(r.path_to(p(9, 9)), None);

    let cav = add(&mut st, &pack, Side::Player, "cavalry", 1, p(0, 3));
    let r = st.movement_range(&pack, cav);
    assert!(
        !r.contains(p(1, 1)) && !r.contains(p(2, 1)),
        "horses cannot enter forest"
    );
    assert!(r.contains(p(4, 3)));
}

#[test]
fn zone_of_control_stops_movement_but_not_leaving() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 1));
    let r = st.movement_range(&pack, me);
    assert_eq!(r.tiles[&p(2, 0)].cost, 2, "a ZOC tile can be entered");
    assert!(!r.contains(p(3, 0)), "but movement ends there");
    assert!(!r.contains(p(2, 1)), "hostile units block");
    assert!(r.contains(p(1, 1)));
    assert!(!r.contains(p(1, 2)) || r.tiles[&p(1, 2)].prev != Some(p(1, 1)));

    // Starting inside a ZOC does not restrict leaving it.
    let inside = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 1));
    let r = st.movement_range(&pack, inside);
    assert_eq!(r.tiles[&p(0, 2)].cost, 2);
    assert_eq!(r.tiles[&p(1, 4)].cost, 3);
}

#[test]
fn friends_can_be_passed_but_not_stopped_on() {
    let rows = "
        ~~~~~
        .....
        ~~~~~";
    let pack = pack(rows);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 1));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 1));
    add(&mut st, &pack, Side::Ally, "infantry", 1, p(2, 1));
    let r = st.movement_range(&pack, me);
    assert!(!r.contains(p(1, 1)) && !r.contains(p(2, 1)));
    assert!(r.through.contains_key(&p(1, 1)) && r.through.contains_key(&p(2, 1)));
    assert_eq!(r.tiles[&p(4, 1)].cost, 4);
    assert_eq!(
        r.path_to(p(3, 1)).unwrap(),
        vec![p(0, 1), p(1, 1), p(2, 1), p(3, 1)]
    );

    // An enemy in the corridor blocks instead.
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 1));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 1));
    let r = st.movement_range(&pack, me);
    assert!(r.contains(p(1, 1)));
    assert!(!r.contains(p(3, 1)));
}

#[test]
fn move_action_validation_and_events() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    assert_eq!(
        st.apply(
            &pack,
            Action::Move {
                unit: foe,
                to: p(6, 7)
            }
        ),
        Err(ActionError::NotYourTurn(foe))
    );
    assert_eq!(
        st.apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(5, 0)
            }
        ),
        Err(ActionError::Unreachable)
    );
    assert_eq!(
        st.apply(
            &pack,
            Action::Move {
                unit: 9,
                to: p(1, 0)
            }
        ),
        Err(ActionError::NoSuchUnit(9))
    );
    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(2, 1),
            },
        )
        .unwrap();
    assert_eq!(
        ev,
        vec![BattleEvent::Moved {
            unit: me,
            path: vec![p(0, 0), p(0, 1), p(1, 1), p(2, 1)]
        }]
    );
    assert_eq!(st.units[me].pos, p(2, 1));
    assert_eq!(st.units[me].facing, Dir::Right);
    assert!(st.units[me].moved && !st.units[me].acted);
    assert!(
        st.movement_range(&pack, me).tiles.is_empty(),
        "empty after moving"
    );
    assert_eq!(
        st.apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(2, 2)
            }
        ),
        Err(ActionError::AlreadyMoved(me))
    );
    st.apply(&pack, Action::Wait { unit: me }).unwrap();
    assert_eq!(
        st.apply(&pack, Action::Wait { unit: me }),
        Err(ActionError::AlreadyActed(me))
    );
    // Enemy ranges can be inspected during the player phase.
    assert!(st.movement_range(&pack, foe).contains(p(7, 3)));
}

#[test]
fn treasure_ends_the_move_and_goes_to_player_units_only() {
    let mut def = battle(OPEN_MAP);
    def.treasures = vec![TreasureDef {
        pos: p(2, 0),
        item: Some("jade".into()),
        gold: 50,
    }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 0));
    let r = st.movement_range(&pack, me);
    assert!(r.contains(p(2, 0)));
    assert!(!r.contains(p(3, 0)), "the treasure tile ends the move");
    assert!(
        st.movement_range(&pack, foe).contains(p(1, 0)),
        "enemies walk over treasures"
    );

    let ev = st
        .apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(2, 0),
            },
        )
        .unwrap();
    assert!(ev.contains(&BattleEvent::TreasureFound {
        unit: me,
        item: Some("jade".into()),
        gold: 50
    }));
    assert_eq!(st.treasures_taken, vec![true]);
    assert_eq!(st.items_found, vec!["jade".to_string()]);
    assert_eq!(st.gold_found, 50);
}

#[test]
fn treasure_gold_saturates() {
    // The validator only asks for gold >= 0; a huge value must not wrap the battle's total.
    let mut def = battle(OPEN_MAP);
    def.treasures = vec![TreasureDef {
        pos: p(1, 0),
        item: None,
        gold: i64::MAX,
    }];
    let pack = pack_with(def);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.gold_found = 100; // from a `give_gold` event, say
    st.apply(
        &pack,
        Action::Move {
            unit: me,
            to: p(1, 0),
        },
    )
    .unwrap();
    assert_eq!(st.gold_found, i64::MAX);
}

#[test]
fn named_attack_shapes() {
    let mut pack = pack(OPEN_MAP);
    for (shape, n) in [
        ("adjacent4", 4),
        ("adjacent8", 8),
        ("archer", 8),
        ("crossbow", 16),
        ("catapult", 24),
    ] {
        pack.classes.get_mut("archer").unwrap().range = RangeSpec::Named(shape.into());
        let mut st = state(&pack);
        let a = add(&mut st, &pack, Side::Player, "archer", 1, p(4, 4));
        assert_eq!(st.attack_tiles(&pack, a, p(4, 4)).len(), n, "{shape}");
        assert!(
            st.attack_tiles(&pack, a, p(0, 0)).len() < n,
            "{shape} is clipped"
        );
    }
    // Archers hit at distance exactly 2, only hostile units.
    pack.classes.get_mut("archer").unwrap().range = RangeSpec::Named("archer".into());
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "archer", 1, p(4, 4));
    let near = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 5));
    let far = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 6));
    let diag = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(5, 5));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 4));
    assert_eq!(st.attack_targets(&pack, a, p(4, 4)), vec![far, diag]);
    assert!(!st.attack_targets(&pack, a, p(4, 4)).contains(&near));
    assert_eq!(
        st.apply(
            &pack,
            Action::Attack {
                unit: a,
                target: near
            }
        ),
        Err(ActionError::OutOfRange)
    );
    // From another tile the shape moves with the unit.
    assert_eq!(st.attack_targets(&pack, a, p(4, 3)), vec![near]);
}
