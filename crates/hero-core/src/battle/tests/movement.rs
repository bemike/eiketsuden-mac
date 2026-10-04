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
fn newly_collected_bean_can_heal_self_or_adjacent_friend_immediately() {
    for heal_friend in [false, true] {
        let mut def = battle(OPEN_MAP);
        def.treasures = vec![TreasureDef {
            pos: p(2, 0),
            item: Some("bean".into()),
            gold: 0,
        }];
        let pack = pack_with(def);
        let mut st = state(&pack);
        let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 0));
        let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 1));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        let target = if heal_friend { friend } else { me };
        st.units[target].hp = 1;
        st.apply(
            &pack,
            Action::Move {
                unit: me,
                to: p(2, 0),
            },
        )
        .unwrap();
        assert_eq!(st.inventory.get("bean"), Some(&1));
        assert!(st.item_targets(&pack, me, "bean").contains(&target));
        st.apply(
            &pack,
            Action::UseItem {
                unit: me,
                item: "bean".into(),
                target,
            },
        )
        .unwrap();
        assert!(st.units[target].hp > 1);
        assert!(!st.inventory.contains_key("bean"));
        assert!(
            st.items_found.is_empty(),
            "used loot must not be awarded again"
        );
        assert!(
            st.items_used.is_empty(),
            "using loot must not spend pre-battle stock"
        );
    }
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

#[test]
fn the_threat_range_counts_a_confused_units_full_move() {
    use crate::battle::ActiveStatus;
    use crate::data::StatusKind;
    let pack = pack(
        "
        .....
        .....
        .....",
    );
    let mut st = state(&pack);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 1));
    let full = st.movement_range(&pack, foe);
    assert_eq!(st.threat_range(&pack, foe), full);
    st.units[foe].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 2,
    });
    // Confused it cannot move now, but it may recover when its phase starts.
    assert!(st.movement_range(&pack, foe).tiles.len() <= 1);
    assert_eq!(st.threat_range(&pack, foe), full);
    // A unit that is gone threatens nothing.
    st.units[foe].state = crate::battle::UnitState::Retreated;
    assert!(st.threat_range(&pack, foe).tiles.is_empty());
}

#[test]
fn personal_loot_is_only_usable_by_its_finder_and_consumes_one_slot() {
    use crate::inventory::Pocket;
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 0));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    for id in [me, friend] {
        st.units[id].equip.carried = Some(Pocket::default());
    }
    assert!(st.gain_item_for(me, "bean"));
    st.units[friend].hp = 1;
    assert!(st.item_targets(&pack, friend, "bean").is_empty());
    st.apply(
        &pack,
        Action::UseItem {
            unit: me,
            item: "bean".into(),
            target: friend,
        },
    )
    .unwrap();
    assert!(st.units[friend].hp > 1);
    assert_eq!(st.item_count_for(me, "bean"), 0);
    assert!(st.items_used.is_empty());
    assert!(st.items_found.is_empty());
    assert!(st.inventory.is_empty());
}

#[test]
fn full_personal_pocket_cannot_destroy_a_new_item() {
    use crate::inventory::Pocket;
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    st.units[me].equip.carried = Some(Pocket::from_items(vec!["bean".into(); 8]).unwrap());
    let before = st.clone();
    assert!(!st.gain_item_for(me, "wine"));
    assert_eq!(st, before);
}
