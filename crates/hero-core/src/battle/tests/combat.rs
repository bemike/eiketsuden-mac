//! Unit values (§2), physical attacks and counters (§4), EXP and levels (§7), retreat (§11).

use crate::battle::testkit::*;
use crate::battle::{
    Action, ActionError, ActiveStatus, AttackForecast, BattleEvent, CounterForecast, UnitState,
};
use crate::battledef::Side;
use crate::data::{Equipment, RangeSpec, StatusKind};

#[test]
fn cao_cao_worked_example() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let cao = add(&mut st, &pack, Side::Player, "guard", 42, p(0, 0));
    set_stats(&mut st, &pack, cao, [75, 50, 98]);
    assert_eq!(st.attack_power(&pack, cao), 1669);
    assert_eq!(st.defense_power(&pack, cao), 1638);
    st.units[cao].equip = Equipment {
        weapon: Some("sword".into()),
        armor: Some("book".into()),
        accessory: None,
    };
    assert_eq!(st.attack_power(&pack, cao), 2002);
    assert_eq!(st.defense_power(&pack, cao), 1965);
    // Morale enters both directly.
    st.units[cao].morale = 50;
    assert_eq!(
        st.attack_power(&pack, cao),
        52 * (50 + 61 + 160) / 10 * 120 / 100
    );
}

#[test]
fn derived_values() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    assert_eq!((st.units[u].max_hp, st.units[u].max_mp), (500, 8));
    assert_eq!(st.attack_power(&pack, u), 268);
    assert_eq!(st.move_points(&pack, u), 4);
    st.units[u].equip.accessory = Some("horse".into());
    assert_eq!(st.move_points(&pack, u), 6);
    st.units[u].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 1,
    });
    assert_eq!(st.move_points(&pack, u), 0, "confused units cannot move");
}

#[test]
fn damage_terrain_and_morale_loss() {
    let pack = pack(
        "
        ....
        .T..
        ....",
    );
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 1));
    let d = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(1, 1));
    let spare = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 2));
    let plain = st.forecast_attack(&pack, spare, a);
    assert_eq!(plain.damage, 134, "268 - 268 / 2");
    let f = st.forecast_attack(&pack, a, d);
    assert_eq!(
        f,
        AttackForecast {
            damage: 107,
            affinity: 100,
            counter: None
        },
        "forest removes 20%"
    );
    let ev = st
        .apply(&pack, Action::Attack { unit: a, target: d })
        .unwrap();
    assert_eq!(
        ev[0],
        BattleEvent::Strike {
            attacker: a,
            defender: d,
            damage: 107,
            morale_loss: 22,
            counter: false
        }
    );
    assert_eq!((st.units[d].hp, st.units[d].morale), (393, 78));
    assert!(st.units[a].acted);
    // The same attack is deterministic: no miss, no critical.
    assert_eq!(ev.len(), 2, "strike + EXP only: {ev:?}");
}

#[test]
fn class_affinity_is_plus_minus_25_percent() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let inf = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let arc = add(&mut st, &pack, Side::Enemy, "archer", 1, p(1, 0));
    let band = add(&mut st, &pack, Side::Enemy, "band", 1, p(0, 1));
    let adv = st.forecast_attack(&pack, inf, arc);
    assert_eq!((adv.affinity, adv.damage), (75, 268 - 224 * 75 / 100 / 2));
    let dis = st.forecast_attack(&pack, arc, inf);
    assert_eq!((dis.affinity, dis.damage), (125, 268 - 268 * 125 / 100 / 2));
    assert_eq!(
        st.forecast_attack(&pack, inf, band).affinity,
        100,
        "no affinity for support classes"
    );
}

#[test]
fn counter_attack_conditions_and_forecast() {
    let mut pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let cav = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
    let bandit = add(&mut st, &pack, Side::Enemy, "bandit", 1, p(3, 4));
    let inf = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 4));
    let foot = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 3));
    assert_eq!(
        st.forecast_attack(&pack, cav, bandit),
        AttackForecast {
            damage: 190,
            affinity: 75,
            counter: Some(CounterForecast {
                damage: 36,
                chance: 33
            })
        }
    );
    assert_eq!(
        st.forecast_attack(&pack, inf, bandit).counter,
        None,
        "infantry never provokes"
    );
    assert_eq!(
        st.forecast_attack(&pack, cav, foot).counter,
        None,
        "infantry cannot counter"
    );
    // The defender must survive.
    st.units[bandit].hp = 150;
    assert_eq!(st.forecast_attack(&pack, cav, bandit).counter, None);
    st.units[bandit].hp = 500;

    // Diagonal attackers are outside the bandit's adjacent4 range.
    pack.classes.get_mut("cavalry").unwrap().range = RangeSpec::Named("adjacent8".into());
    st.units[cav].pos = p(2, 3);
    assert_eq!(st.forecast_attack(&pack, cav, bandit).counter, None);
    // Distance 2 never counters, even for a provoking class.
    let arch = pack.classes.get_mut("archer").unwrap();
    arch.provokes_counter = true;
    let archer = add(&mut st, &pack, Side::Player, "archer", 1, p(3, 6));
    assert_eq!(st.forecast_attack(&pack, archer, bandit).counter, None);
}

#[test]
fn counter_strikes_back_and_can_defeat_the_attacker() {
    let mut pack = pack(OPEN_MAP);
    pack.rules.counter_divisor = 50; // STR 50 -> 100%
    let mut st = state(&pack);
    let cav = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
    let bandit = add(&mut st, &pack, Side::Enemy, "bandit", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "bandit", 1, p(7, 7));
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: cav,
                target: bandit,
            },
        )
        .unwrap();
    assert_eq!(
        ev,
        vec![
            BattleEvent::Strike {
                attacker: cav,
                defender: bandit,
                damage: 190,
                morale_loss: 38,
                counter: false
            },
            BattleEvent::Strike {
                attacker: bandit,
                defender: cav,
                damage: 36,
                morale_loss: 6,
                counter: true
            },
            BattleEvent::ExpGained {
                unit: cav,
                amount: 6
            },
        ]
    );
    assert_eq!(st.units[cav].hp, 564);

    // Enemy phase: an enemy commander attacks a player bandit and dies to the counter;
    // the counter earns kill + commander EXP.
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "bandit", 1, p(3, 4));
    let boss = add(&mut st, &pack, Side::Enemy, "cavalry", 1, p(3, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[boss].hp = 10;
    st.units[boss].commander = true;
    st.phase = Side::Enemy;
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: boss,
                target: me,
            },
        )
        .unwrap();
    assert!(ev.contains(&BattleEvent::Retreated { unit: boss }));
    assert!(ev.contains(&BattleEvent::ExpGained {
        unit: me,
        amount: 38 + 20
    }));
    assert_eq!(st.units[boss].state, UnitState::Retreated);
    assert_eq!(st.units[me].hp, 500 - 190);
}

#[test]
fn counter_chance_is_rolled() {
    let pack = pack(OPEN_MAP);
    let mut counters = 0;
    for seed in 0..200 {
        let mut st =
            crate::battle::BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed)
                .unwrap();
        let cav = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
        let bandit = add(&mut st, &pack, Side::Enemy, "bandit", 1, p(3, 4));
        let ev = st
            .apply(
                &pack,
                Action::Attack {
                    unit: cav,
                    target: bandit,
                },
            )
            .unwrap();
        counters += ev
            .iter()
            .filter(|e| matches!(e, BattleEvent::Strike { counter: true, .. }))
            .count();
    }
    assert!((40..95).contains(&counters), "33% of 200: {counters}");
}

#[test]
fn attack_validation() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(0, 0));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 0));
    let gone = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 1));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    assert_eq!(
        st.apply(
            &pack,
            Action::Attack {
                unit: a,
                target: friend
            }
        ),
        Err(ActionError::InvalidTarget)
    );
    st.units[gone].state = UnitState::Retreated;
    assert_eq!(
        st.apply(
            &pack,
            Action::Attack {
                unit: a,
                target: gone
            }
        ),
        Err(ActionError::NoSuchUnit(gone))
    );
    st.units[a].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 1,
    });
    assert_eq!(
        st.apply(&pack, Action::Wait { unit: a }),
        Err(ActionError::Confused(a))
    );
}

#[test]
fn exp_for_attacks_and_kills_with_drop() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let high = add(&mut st, &pack, Side::Enemy, "infantry", 6, p(3, 4));
    let weak = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 3));
    let b = add(&mut st, &pack, Side::Player, "infantry", 1, p(5, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: a,
                target: high,
            },
        )
        .unwrap();
    assert!(
        ev.contains(&BattleEvent::ExpGained {
            unit: a,
            amount: 12
        }),
        "level +5 -> 12"
    );
    assert_eq!(st.units[a].exp, 12);

    st.units[weak].hp = 50;
    st.units[weak].commander = true;
    st.units[weak].drop = Some("bean".into());
    st.units[b].pos = p(5, 3);
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: b,
                target: weak,
            },
        )
        .unwrap();
    assert_eq!(
        &ev[1..],
        &[
            BattleEvent::Retreated { unit: weak },
            BattleEvent::ItemDropped {
                unit: weak,
                item: "bean".into()
            },
            BattleEvent::ExpGained {
                unit: b,
                amount: 38 + 20
            },
        ]
    );
    assert_eq!(st.items_found, vec!["bean".to_string()]);
    assert_eq!(st.unit_at(p(4, 3)), None);
}

#[test]
fn enemies_gain_no_exp() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let me = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    st.phase = Side::Enemy;
    let ev = st
        .apply(
            &pack,
            Action::Attack {
                unit: foe,
                target: me,
            },
        )
        .unwrap();
    assert!(!ev
        .iter()
        .any(|e| matches!(e, BattleEvent::ExpGained { .. })));
    assert_eq!(st.units[foe].exp, 0);
}

#[test]
fn level_up_keeps_remainder_and_learns() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 2, p(3, 3));
    let d = add(&mut st, &pack, Side::Enemy, "infantry", 2, p(3, 4));
    // INT 40: max MP (2 + 10) * 40 / 40 = 12 -> (3 + 10) * 40 / 40 = 13.
    set_stats(&mut st, &pack, a, [50, 40, 50]);
    st.units[a].exp = 95;
    st.units[a].hp = 100;
    let ev = st
        .apply(&pack, Action::Attack { unit: a, target: d })
        .unwrap();
    assert_eq!(
        &ev[1..],
        &[
            BattleEvent::ExpGained { unit: a, amount: 6 },
            BattleEvent::LevelUp {
                unit: a,
                level: 3,
                hp_gain: 20,
                mp_gain: 1
            },
            BattleEvent::Learned {
                unit: a,
                strategy: "flood".into()
            },
        ]
    );
    let u = &st.units[a];
    assert_eq!((u.level, u.exp, u.hp, u.max_hp), (3, 1, 120, 540));
    assert_eq!((u.mp, u.max_mp), (13, 13));
}

#[test]
fn level_cap_holds_exp_below_a_level() {
    let mut pack = pack(OPEN_MAP);
    pack.rules.level_cap = 2;
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 2, p(3, 3));
    let d = add(&mut st, &pack, Side::Enemy, "infantry", 2, p(3, 4));
    st.units[a].exp = 99;
    let ev = st
        .apply(&pack, Action::Attack { unit: a, target: d })
        .unwrap();
    assert!(!ev.iter().any(|e| matches!(e, BattleEvent::LevelUp { .. })));
    assert_eq!((st.units[a].level, st.units[a].exp), (2, 99));
}

#[test]
fn confused_unit_routed_by_morale_loss_gives_no_kill_exp() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let d = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[d].morale = 10;
    st.units[d].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 1,
    });
    let ev = st
        .apply(&pack, Action::Attack { unit: a, target: d })
        .unwrap();
    assert!(ev.contains(&BattleEvent::Retreated { unit: d }));
    assert!(
        ev.contains(&BattleEvent::ExpGained { unit: a, amount: 6 }),
        "attack EXP only: {ev:?}"
    );
    assert!(st.units[d].hp > 0);
}

/// Extended rules' joint attack (D25): +10 % per other unit of the attacker's side next to the
/// defender, players and allies together; enemies, diagonal neighbours and the attacker itself
/// do not count; at most +30 %. Forecast and blow agree; off by default.
#[test]
fn joint_attack_bonus_of_extended_rules() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let a = add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 2));
    let d = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 2));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 2));
    add(&mut st, &pack, Side::Ally, "infantry", 1, p(2, 1));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 3));
    add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    assert_eq!(
        st.forecast_attack(&pack, a, d).damage,
        134,
        "off: 268 - 268 / 2"
    );

    st.extended_rules = true;
    assert_eq!(st.joint_attack_pct(a, d), 20);
    let f = st.forecast_attack(&pack, a, d);
    assert_eq!(f.damage, 134 * 120 / 100);
    // The defender's side gets nothing from the attacker's neighbours.
    assert_eq!(st.joint_attack_pct(d, a), 0);
    // A third joining unit, then the cap.
    st.units[foe].side = Side::Player;
    assert_eq!(st.joint_attack_pct(a, d), 30);
    add(&mut st, &pack, Side::Player, "infantry", 1, p(1, 3));
    assert_eq!(
        st.joint_attack_pct(a, d),
        30,
        "only orthogonal neighbours of the defender"
    );
    st.units[foe].side = Side::Enemy;

    let ev = st
        .apply(&pack, Action::Attack { unit: a, target: d })
        .unwrap();
    match &ev[0] {
        BattleEvent::Strike { damage, .. } => assert_eq!(*damage, f.damage),
        other => panic!("expected a strike: {other:?}"),
    }
}
