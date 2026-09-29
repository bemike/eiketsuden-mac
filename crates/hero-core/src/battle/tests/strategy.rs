//! Strategies (§5), battle items (§10) and the EXP they give (§7).

use crate::battle::testkit::*;
use crate::battle::{
    Action, ActionError, ActiveStatus, BattleEvent, BattleState, StrategyHit, UnitId, UnitState,
    Weather,
};
use crate::battledef::Side;
use crate::data::{Effect, StatusKind, StrategyFormulas};
use crate::geom::Pos;

fn cast(unit: UnitId, strategy: &str, target: Pos) -> Action {
    Action::Strategy {
        unit,
        strategy: strategy.into(),
        target,
    }
}

fn use_item(unit: UnitId, item: &str, target: UnitId) -> Action {
    Action::UseItem {
        unit,
        item: item.into(),
        target,
    }
}

/// Hits of the `StrategyUsed` event in `ev`.
fn hits(ev: &[BattleEvent]) -> Vec<StrategyHit> {
    ev.iter()
        .find_map(|e| match e {
            BattleEvent::StrategyUsed { hits, .. } => Some(hits.clone()),
            _ => None,
        })
        .expect("a StrategyUsed event")
}

fn hit(unit: UnitId) -> StrategyHit {
    StrategyHit {
        unit,
        success: true,
        damage: 0,
        healed: 0,
        morale: 0,
        status: None,
    }
}

fn exp_events(ev: &[BattleEvent]) -> Vec<BattleEvent> {
    ev.iter()
        .filter(|e| matches!(e, BattleEvent::ExpGained { .. }))
        .cloned()
        .collect()
}

#[test]
fn hit_chance_follows_int_and_level_with_strategy_guard() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 10, p(3, 3));
    set_stats(&mut st, &pack, c, [50, 50, 50]);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(3, 4));
    set_stats(&mut st, &pack, foe, [50, 50, 50]);
    let band = add(&mut st, &pack, Side::Enemy, "band", 10, p(4, 4));
    set_stats(&mut st, &pack, band, [10, 50, 10]);
    let dull = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(2, 4));
    set_stats(&mut st, &pack, dull, [50, 0, 50]);
    let sage = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(2, 2));
    set_stats(&mut st, &pack, sage, [50, 200, 50]);

    // power(caster) = 50 * 10 / 100 + 50 = 55
    let forecast = |t: UnitId| {
        let f = st.forecast_strategy(&pack, c, "fire", st.units[t].pos);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].unit, t);
        (f[0].chance, f[0].amount)
    };
    // damage = 200 + 2 * (50 * 10 / 50 + 50) - (int * 10 / 50 + int)
    assert_eq!(forecast(foe), (75, 260), "100 - 100 * 55 / (4 * 55)");
    assert_eq!(
        forecast(band),
        (50, 130),
        "strategy_guard: INT doubled for evasion, damage halved"
    );
    assert_eq!(forecast(dull), (100, 320));
    assert_eq!(forecast(sage).0, 0, "power 220 >= 4 * 55");
}

#[test]
fn strategy_damage_terrain_boost_and_weather() {
    let pack = pack(
        "
        ....
        .T..
        ....",
    );
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 10, p(2, 1));
    set_stats(&mut st, &pack, c, [50, 50, 50]);
    let wood = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(1, 1));
    let open = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(2, 2));
    for t in [wood, open] {
        set_stats(&mut st, &pack, t, [50, 0, 50]);
    }
    let amount = |st: &BattleState, s: &str, t: UnitId| {
        st.forecast_strategy(&pack, c, s, st.units[t].pos)[0].amount
    };
    assert_eq!(amount(&st, "fire", open), 320);
    assert_eq!(amount(&st, "fire", wood), 400, "fire in forest +25%");
    assert_eq!(amount(&st, "flood", open), 420);
    assert!(
        st.forecast_strategy(&pack, c, "flood", p(1, 1)).is_empty(),
        "no water in forests"
    );
    assert_eq!(
        st.strategy_targets(&pack, c, "fire", p(2, 1)),
        vec![p(1, 1), p(2, 2)]
    );
    assert_eq!(
        st.strategy_targets(&pack, c, "flood", p(2, 1)),
        vec![p(2, 2)]
    );

    // Rain: fire is impossible, water deals +25%.
    let mut rain = st.clone();
    rain.weather = Weather::Rain;
    assert!(rain.strategy_targets(&pack, c, "fire", p(2, 1)).is_empty());
    assert_eq!(
        rain.apply(&pack, cast(c, "fire", p(2, 2))),
        Err(ActionError::WrongTerrain)
    );
    assert_eq!(amount(&rain, "flood", open), 525);

    // The cast deals the forecast plus at most 1/50 random bonus and costs morale.
    let ev = st.apply(&pack, cast(c, "fire", p(1, 1))).unwrap();
    let h = &hits(&ev)[0];
    assert!(h.success && (400..=408).contains(&h.damage), "{h:?}");
    let max_hp = st.units[wood].max_hp;
    assert_eq!(max_hp, 680);
    assert_eq!(st.units[wood].hp, max_hp - h.damage);
    let loss = (h.damage * 100 + max_hp - 1) / max_hp;
    assert_eq!((h.morale, st.units[wood].morale), (-loss, 100 - loss));
    assert_eq!(st.units[c].mp, 25 - 4);
    assert!(st.units[c].acted);
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained { unit: c, amount: 6 }]
    );
}

#[test]
fn element_gate_needs_a_matching_tile() {
    let pack = pack(
        "
        .v.
        .^.
        ...",
    );
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 10, p(0, 1));
    let bandit = add(&mut st, &pack, Side::Player, "bandit", 10, p(2, 2));
    let in_village = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(1, 0));
    add(&mut st, &pack, Side::Enemy, "bandit", 10, p(1, 1));
    let on_plain = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(1, 2));
    assert_eq!(
        st.apply(&pack, cast(c, "fire", st.units[in_village].pos)),
        Err(ActionError::WrongTerrain),
        "villages allow no elements"
    );
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(1, 1))),
        Err(ActionError::WrongTerrain),
        "mountains allow only earth"
    );
    assert_eq!(
        st.strategy_targets(&pack, c, "fire", p(0, 1)),
        vec![p(1, 2)]
    );
    assert_eq!(
        st.apply(&pack, cast(bandit, "rock", st.units[on_plain].pos)),
        Err(ActionError::WrongTerrain)
    );
    assert_eq!(
        st.strategy_targets(&pack, bandit, "rock", p(2, 2)),
        vec![p(1, 1)]
    );
    let ev = st.apply(&pack, cast(bandit, "rock", p(1, 1))).unwrap();
    assert_eq!(hits(&ev).len(), 1);
}

#[test]
fn cross_area_hits_the_aim_and_its_neighbours_on_matching_tiles() {
    let pack = pack(
        "
        .....
        .T.v.
        .....",
    );
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "heavy_infantry", 20, p(2, 2));
    let centre = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(2, 1));
    let north = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(2, 0));
    let wood = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(1, 1));
    let village = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(3, 1));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 20, p(1, 2));
    for t in [centre, north, wood, village] {
        set_stats(&mut st, &pack, t, [50, 0, 50]);
    }
    assert_eq!(
        st.apply(&pack, cast(c, "big_fire", p(3, 1))),
        Err(ActionError::WrongTerrain),
        "the aimed tile must allow the element"
    );
    let mp = st.units[c].mp;
    let ev = st.apply(&pack, cast(c, "big_fire", p(2, 1))).unwrap();
    let hits = hits(&ev);
    let hit_units: Vec<UnitId> = hits.iter().map(|h| h.unit).collect();
    assert_eq!(
        hit_units,
        vec![centre, north, wood],
        "village arm skipped, friends and caster unaffected"
    );
    assert!(hits.iter().all(|h| h.success));
    // caster 2 * (30 * 20 / 50 + 30) = 84: 284 on plain, 355 in the forest
    assert!((284..=289).contains(&hits[0].damage) && (284..=289).contains(&hits[1].damage));
    assert!((355..=362).contains(&hits[2].damage));
    assert_eq!(st.units[village].hp, st.units[village].max_hp);
    assert_eq!(st.units[friend].hp, st.units[friend].max_hp);
    assert_eq!(st.units[c].mp, mp - 16);
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained {
            unit: c,
            amount: 18
        }],
        "attack EXP per damaged unit"
    );
}

#[test]
fn all_in_range_heal_covers_friends_in_reach() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let band = add(&mut st, &pack, Side::Player, "band", 10, p(3, 3));
    let full = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 2));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(2, 3));
    let hurt = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let ally = add(&mut st, &pack, Side::Ally, "infantry", 1, p(4, 4));
    let far = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 6));
    st.units[foe].hp = 10;
    st.units[hurt].hp = 400;
    st.units[ally].hp = 10;
    st.units[ally].morale = 20;
    st.units[far].hp = 10;

    assert_eq!(
        st.strategy_targets(&pack, band, "heal_all", p(3, 3)),
        vec![p(3, 3)]
    );
    assert_eq!(
        st.apply(&pack, cast(band, "heal_all", p(3, 4))),
        Err(ActionError::OutOfRange),
        "all_in_range is aimed at the caster"
    );
    let forecast = st.forecast_strategy(&pack, band, "heal_all", p(3, 3));
    assert_eq!(
        forecast
            .iter()
            .map(|f| (f.unit, f.chance, f.amount))
            .collect::<Vec<_>>(),
        [
            (full, 100, 0),
            (band, 100, 0),
            (hurt, 100, -100),
            (ally, 100, -184)
        ]
    );

    let ev = st.apply(&pack, cast(band, "heal_all", p(3, 3))).unwrap();
    // 200 + 2 * (70 * 10 / 50 + 70) = 368, capped at the missing HP, halved below 30 morale.
    let heal = |unit, healed| StrategyHit {
        healed,
        ..hit(unit)
    };
    assert_eq!(
        ev,
        vec![
            BattleEvent::StrategyUsed {
                caster: band,
                strategy: "heal_all".into(),
                target: p(3, 3),
                hits: vec![
                    heal(full, 0),
                    heal(band, 0),
                    heal(hurt, 100),
                    heal(ally, 184)
                ],
            },
            BattleEvent::ExpGained {
                unit: band,
                amount: 8
            },
        ],
        "support EXP once per cast; class support_exp is for single targets"
    );
    assert_eq!((st.units[hurt].hp, st.units[ally].hp), (500, 194));
    assert_eq!((st.units[far].hp, st.units[foe].hp), (10, 10));
    assert_eq!(st.units[band].mp, 35 - 24);
}

#[test]
fn single_target_support_strategies() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let band = add(&mut st, &pack, Side::Player, "band", 10, p(3, 3));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let other = add(&mut st, &pack, Side::Player, "band", 10, p(2, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    st.units[friend].hp = 100;
    st.units[friend].morale = 90;
    assert_eq!(
        st.apply(&pack, cast(band, "heal", p(7, 7))),
        Err(ActionError::OutOfRange)
    );
    let ev = st.apply(&pack, cast(band, "heal", p(3, 4))).unwrap();
    assert_eq!(
        hits(&ev),
        vec![StrategyHit {
            healed: 368,
            ..hit(friend)
        }]
    );
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained {
            unit: band,
            amount: 12
        }],
        "class support_exp"
    );
    let ev = st.apply(&pack, cast(other, "cheer", p(3, 4))).unwrap();
    assert_eq!(
        hits(&ev),
        vec![StrategyHit {
            morale: 10,
            ..hit(friend)
        }],
        "clamped at 100"
    );
    assert_eq!(st.units[friend].morale, 100);
}

#[test]
fn morale_down_is_shifted_by_the_level_difference() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let veteran = add(&mut st, &pack, Side::Player, "cavalry", 50, p(3, 3));
    let rookie = add(&mut st, &pack, Side::Player, "cavalry", 25, p(5, 5));
    let a = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(3, 4));
    let b = add(&mut st, &pack, Side::Enemy, "infantry", 20, p(5, 6));
    for t in [a, b] {
        set_stats(&mut st, &pack, t, [50, 0, 50]);
    }
    let ev = st.apply(&pack, cast(veteran, "provoke", p(3, 4))).unwrap();
    assert_eq!(
        hits(&ev),
        vec![StrategyHit {
            morale: -23,
            ..hit(a)
        }],
        "Lv5x vs Lv2x: -20 -> -23"
    );
    assert_eq!(st.units[a].morale, 77);
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained {
            unit: veteran,
            amount: 8
        }]
    );
    let ev = st.apply(&pack, cast(rookie, "provoke", p(5, 6))).unwrap();
    assert_eq!(
        hits(&ev),
        vec![StrategyHit {
            morale: -20,
            ..hit(b)
        }],
        "same decade: no shift"
    );
}

#[test]
fn confusion_disables_the_target_and_routs_it_at_zero_morale() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 5, p(3, 3));
    let c2 = add(&mut st, &pack, Side::Player, "infantry", 5, p(5, 3));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 5, p(3, 4));
    let beaten = add(&mut st, &pack, Side::Enemy, "infantry", 5, p(5, 4));
    for u in [c, c2] {
        set_stats(&mut st, &pack, u, [50, 50, 50]);
    }
    for t in [foe, beaten] {
        set_stats(&mut st, &pack, t, [50, 0, 50]);
    }
    st.units[beaten].morale = 0;
    st.units[beaten].drop = Some("bean".into());
    st.begin(&pack);

    let ev = st.apply(&pack, cast(c, "confuse", p(3, 4))).unwrap();
    assert_eq!(
        ev,
        vec![
            BattleEvent::StrategyUsed {
                caster: c,
                strategy: "confuse".into(),
                target: p(3, 4),
                hits: vec![StrategyHit {
                    status: Some(StatusKind::Confused),
                    ..hit(foe)
                }],
            },
            BattleEvent::Confused { unit: foe },
            BattleEvent::ExpGained { unit: c, amount: 8 },
        ]
    );
    // One extra count because the countdown runs at the start of the enemy's own phase.
    assert_eq!(
        st.units[foe].statuses,
        vec![ActiveStatus {
            status: StatusKind::Confused,
            turns: 2
        }]
    );

    // Confused at 0 morale: the unit retreats at once, with no defeat EXP.
    let ev = st.apply(&pack, cast(c2, "confuse", p(5, 4))).unwrap();
    assert!(
        ev.contains(&BattleEvent::Retreated { unit: beaten }),
        "{ev:?}"
    );
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained {
            unit: c2,
            amount: 8
        }]
    );
    assert_eq!(st.units[beaten].state, UnitState::Retreated);
    assert!(st.units[beaten].hp > 0);
    assert_eq!(
        st.items_found,
        vec!["bean".to_string()],
        "a routed unit still drops its item"
    );

    // The confused unit skips the next enemy phase ...
    st.apply(&pack, Action::EndPhase).unwrap();
    assert_eq!(st.phase, Side::Enemy);
    assert!(st.units[foe].has_status(StatusKind::Confused));
    assert_eq!(st.next_ai_unit(), None);
    assert!(st.ai_actions(&pack, foe).is_empty());
    assert_eq!(st.move_points(&pack, foe), 0);
    assert_eq!(
        st.apply(
            &pack,
            Action::Move {
                unit: foe,
                to: p(3, 5)
            }
        ),
        Err(ActionError::Confused(foe))
    );
    // ... and recovers at the start of the one after.
    st.apply(&pack, Action::EndPhase).unwrap();
    let ev = st.apply(&pack, Action::EndPhase).unwrap();
    assert!(ev.contains(&BattleEvent::StatusExpired {
        unit: foe,
        status: StatusKind::Confused
    }));
    assert_eq!(st.next_ai_unit(), Some(foe));
}

#[test]
fn caster_pays_mp_even_when_every_target_evades() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    set_stats(&mut st, &pack, foe, [50, 200, 50]);
    let ev = st.apply(&pack, cast(c, "fire", p(3, 4))).unwrap();
    assert_eq!(
        hits(&ev),
        vec![StrategyHit {
            success: false,
            ..hit(foe)
        }]
    );
    assert!(exp_events(&ev).is_empty());
    assert_eq!(st.units[c].mp, 8 - 4);
    assert_eq!(st.units[foe].hp, st.units[foe].max_hp);
    assert!(st.units[c].acted);
}

#[test]
fn strategy_validation_leaves_the_state_untouched() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(2, 3));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(3, 6));
    let before = st.clone();
    assert_eq!(st.usable_strategies(&pack, c), vec!["fire".to_string()]);
    assert_eq!(
        st.apply(&pack, cast(c, "nope", p(3, 4))),
        Err(ActionError::UnknownStrategy("nope".into()))
    );
    assert_eq!(
        st.apply(&pack, cast(c, "flood", p(3, 4))),
        Err(ActionError::UnknownStrategy("flood".into())),
        "learned at level 3"
    );
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(3, 6))),
        Err(ActionError::OutOfRange)
    );
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(3, 5))),
        Err(ActionError::OutOfRange)
    );
    assert_eq!(
        st.apply(&pack, cast(c, "fire", st.units[friend].pos)),
        Err(ActionError::InvalidTarget)
    );
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(4, 3))),
        Err(ActionError::InvalidTarget)
    );
    assert_eq!(st, before);

    st.units[c].mp = 3;
    assert!(st.usable_strategies(&pack, c).is_empty());
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(3, 4))),
        Err(ActionError::NotEnoughMp)
    );
    st.units[c].mp = 8;
    st.units[c].statuses.push(ActiveStatus {
        status: StatusKind::Confused,
        turns: 1,
    });
    assert!(st.usable_strategies(&pack, c).is_empty());
    assert_eq!(
        st.apply(&pack, cast(c, "fire", p(3, 4))),
        Err(ActionError::Confused(c))
    );
}

#[test]
fn healing_items_and_their_targets() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    for (item, n) in [("bean", 2), ("wine", 1), ("spear", 1)] {
        st.inventory.insert(item.into(), n);
    }
    let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let distant = add(&mut st, &pack, Side::Player, "infantry", 1, p(5, 4));
    let ally = add(&mut st, &pack, Side::Ally, "infantry", 1, p(2, 3));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 3));
    st.units[friend].hp = 100;
    st.units[friend].morale = 20;

    assert_eq!(st.item_targets(&pack, u, "bean"), vec![u, friend, ally]);
    assert!(
        st.item_targets(&pack, u, "spear").is_empty(),
        "camp-only item"
    );
    assert_eq!(
        st.apply(&pack, use_item(u, "bean", distant)),
        Err(ActionError::OutOfRange)
    );
    assert_eq!(
        st.apply(&pack, use_item(u, "bean", foe)),
        Err(ActionError::InvalidTarget)
    );
    assert_eq!(
        st.apply(&pack, use_item(u, "spear", u)),
        Err(ActionError::BadItem("spear".into()))
    );
    assert_eq!(
        st.apply(&pack, use_item(u, "sword", u)),
        Err(ActionError::BadItem("sword".into()))
    );

    let ev = st.apply(&pack, use_item(u, "bean", friend)).unwrap();
    assert_eq!(
        ev,
        vec![BattleEvent::ItemUsed {
            user: u,
            target: friend,
            item: "bean".into(),
            healed: 300,
            morale: 0
        }],
        "items heal exactly their power and give no EXP"
    );
    assert_eq!(st.units[friend].hp, 400);
    assert_eq!(st.inventory.get("bean"), Some(&1));
    assert!(st.units[u].acted);

    let ev = st.apply(&pack, use_item(friend, "wine", friend)).unwrap();
    assert_eq!(
        ev,
        vec![BattleEvent::ItemUsed {
            user: friend,
            target: friend,
            item: "wine".into(),
            healed: 0,
            morale: 30
        }]
    );
    assert_eq!(st.units[friend].morale, 50);
    assert_eq!(st.inventory.get("wine"), None, "used up");
    assert_eq!(
        st.apply(&pack, use_item(distant, "wine", distant)),
        Err(ActionError::BadItem("wine".into()))
    );

    // The inventory belongs to the player: other sides cannot use it.
    st.phase = Side::Enemy;
    assert_eq!(
        st.apply(&pack, use_item(foe, "bean", foe)),
        Err(ActionError::BadItem("bean".into()))
    );
}

/// `battle_use` on equipment is ignored (the validator warns about it): the campaign only
/// takes battle consumables back from the battle, so using equipment would never use it up.
#[test]
fn equipment_is_never_a_battle_item() {
    let mut pack = pack(OPEN_MAP);
    let jade = pack.items.get_mut("jade").unwrap();
    jade.battle_use = true;
    jade.effects = vec![Effect::Heal { power: 300 }];
    let mut st = state(&pack);
    st.inventory.insert("jade".into(), 1);
    let me = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
    let hurt = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(0, 7));
    st.units[me].mp = 0;
    st.units[hurt].hp = 100;

    assert!(st.item_targets(&pack, me, "jade").is_empty());
    assert_eq!(
        st.apply(&pack, use_item(me, "jade", hurt)),
        Err(ActionError::BadItem("jade".into()))
    );
    // The AI (the player side in simulations) does not plan to use it either.
    let plan = st.ai_actions(&pack, me);
    assert!(
        !plan.iter().any(|a| matches!(a, Action::UseItem { .. })),
        "{plan:?}"
    );
    assert_eq!(st.inventory.get("jade"), Some(&1));
}

/// Morale amounts are data; extreme ones clamp to 0..=100 instead of overflowing.
#[test]
fn extreme_morale_amounts_saturate() {
    let mut pack = pack(OPEN_MAP);
    let morale = |amount: i32| vec![Effect::Morale { amount }];
    pack.strategies.get_mut("cheer").unwrap().effects = morale(i32::MAX);
    pack.strategies.get_mut("provoke").unwrap().effects = morale(i32::MIN);
    pack.items.get_mut("wine").unwrap().effects = morale(i32::MAX);
    let mut sour = pack.items["wine"].clone();
    sour.id = "sour_wine".into();
    sour.effects = morale(i32::MIN);
    pack.items.insert(sour.id.clone(), sour);
    let mut st = state(&pack);
    st.inventory.insert("wine".into(), 1);
    st.inventory.insert("sour_wine".into(), 1);
    let band = add(&mut st, &pack, Side::Player, "band", 10, p(3, 3));
    let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
    let rider = add(&mut st, &pack, Side::Player, "cavalry", 20, p(5, 5));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(5, 6));
    set_stats(&mut st, &pack, foe, [50, 0, 50]);
    st.units[friend].morale = 20;

    // The AI weighs the effects too (the player side in simulations).
    assert!(!st.ai_actions(&pack, band).is_empty());
    assert!(!st.ai_actions(&pack, rider).is_empty());

    let ev = st.apply(&pack, cast(band, "cheer", p(3, 4))).unwrap();
    assert_eq!(hits(&ev)[0].morale, 80);
    assert_eq!(st.units[friend].morale, 100);
    // The level difference shifts morale-down further (§5).
    let ev = st.apply(&pack, cast(rider, "provoke", p(5, 6))).unwrap();
    assert_eq!(hits(&ev)[0].morale, -100);
    assert_eq!(st.units[foe].morale, 0);

    st.units[friend].morale = 50;
    st.apply(&pack, use_item(friend, "wine", friend)).unwrap();
    assert_eq!(st.units[friend].morale, 100);
    st.units[friend].acted = false;
    let ev = st
        .apply(&pack, use_item(friend, "sour_wine", friend))
        .unwrap();
    assert!(
        matches!(ev[0], BattleEvent::ItemUsed { morale: -100, .. }),
        "{ev:?}"
    );
    assert_eq!(st.units[friend].morale, 0);
}

#[test]
fn strategy_scrolls_cast_without_mp_and_earn_exp() {
    let pack = pack(OPEN_MAP);
    let mut st = state(&pack);
    st.inventory.insert("fire_scroll".into(), 1);
    let u = add(&mut st, &pack, Side::Player, "cavalry", 1, p(3, 3));
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 1, p(4, 4));
    add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
    set_stats(&mut st, &pack, foe, [50, 0, 50]);
    st.units[u].mp = 0;
    assert!(st.usable_strategies(&pack, u).is_empty(), "no MP left");
    assert_eq!(st.item_targets(&pack, u, "fire_scroll"), vec![foe]);
    let ev = st.apply(&pack, use_item(u, "fire_scroll", foe)).unwrap();
    assert_eq!(
        ev[0],
        BattleEvent::ItemUsed {
            user: u,
            target: foe,
            item: "fire_scroll".into(),
            healed: 0,
            morale: 0
        }
    );
    let h = &hits(&ev)[0];
    // 200 + 2 * (30 * 1 / 50 + 30) = 260
    assert!(h.success && (260..=265).contains(&h.damage), "{h:?}");
    assert_eq!(
        exp_events(&ev),
        vec![BattleEvent::ExpGained { unit: u, amount: 6 }]
    );
    assert_eq!(st.units[u].mp, 0);
    assert!(st.inventory.is_empty());
}

/// The test pack playing the original strategy formulas.
fn original_pack() -> crate::pack::Pack {
    let mut pack = pack(OPEN_MAP);
    pack.rules.strategy_formulas = StrategyFormulas::Original;
    pack
}

#[test]
fn original_formulas_confuse_harder_heal_less_and_have_no_least_damage() {
    let (engine, original) = (pack(OPEN_MAP), original_pack());
    let setup = |pack: &crate::pack::Pack| {
        let mut st = state(pack);
        let c = add(&mut st, pack, Side::Player, "infantry", 10, p(3, 3));
        set_stats(&mut st, pack, c, [50, 50, 50]);
        let foe = add(&mut st, pack, Side::Enemy, "infantry", 10, p(3, 4));
        set_stats(&mut st, pack, foe, [50, 50, 50]);
        let dull = add(&mut st, pack, Side::Player, "infantry", 10, p(2, 3));
        set_stats(&mut st, pack, dull, [50, 0, 50]);
        let sage = add(&mut st, pack, Side::Enemy, "infantry", 10, p(2, 4));
        set_stats(&mut st, pack, sage, [50, 200, 50]);
        let band = add(&mut st, pack, Side::Player, "band", 10, p(4, 3));
        set_stats(&mut st, pack, band, [10, 50, 10]);
        let hurt = add(&mut st, pack, Side::Player, "infantry", 1, p(4, 4));
        st.units[hurt].hp = 1;
        st.units[hurt].max_hp = 1000;
        st.units[hurt].morale = 10;
        (st, c, foe, dull, sage, band, hurt)
    };
    let forecast = |st: &BattleState, pack, caster, s: &str, t: UnitId| {
        let f = st.forecast_strategy(pack, caster, s, st.units[t].pos);
        (f[0].chance, f[0].amount)
    };
    let (st, c, foe, dull, sage, band, hurt) = setup(&engine);
    assert_eq!(forecast(&st, &engine, c, "confuse", foe).0, 75);
    assert_eq!(forecast(&st, &engine, c, "fire", foe).0, 75);
    // 200 + 0 - (200 * 10 / 50 + 200) < 0: at least 1.
    assert_eq!(forecast(&st, &engine, dull, "fire", sage).1, 1);
    // 200 + 2 * (50 * 10 / 50 + 50), halved on morale below 30.
    assert_eq!(forecast(&st, &engine, band, "heal", hurt).1, -160);

    let (st, c, foe, dull, sage, band, hurt) = setup(&original);
    // Confusion: 100 - 100 * 55 / (2 * 55); other strategies as before.
    assert_eq!(forecast(&st, &original, c, "confuse", foe).0, 50);
    assert_eq!(forecast(&st, &original, c, "fire", foe).0, 75);
    assert_eq!(forecast(&st, &original, dull, "fire", sage).1, 0);
    // 200 + 10 * 50 / 20, whatever the morale (the random 10 % comes on casting).
    assert_eq!(forecast(&st, &original, band, "heal", hurt).1, -225);
}

#[test]
fn original_formulas_add_a_random_tenth_to_support() {
    let pack = original_pack();
    let (mut healed, mut cheered) = (Vec::new(), Vec::new());
    for seed in 0..60 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let band = add(&mut st, &pack, Side::Player, "band", 10, p(3, 3));
        set_stats(&mut st, &pack, band, [10, 50, 10]);
        let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
        let other = add(&mut st, &pack, Side::Player, "band", 10, p(2, 3));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        st.units[friend].hp = 1;
        st.units[friend].max_hp = 1000;
        st.units[friend].morale = 50;
        let ev = st.apply(&pack, cast(band, "heal", p(3, 4))).unwrap();
        healed.push(hits(&ev)[0].healed);
        let ev = st.apply(&pack, cast(other, "cheer", p(3, 4))).unwrap();
        cheered.push(hits(&ev)[0].morale);
    }
    // 225 + rand(0..=22); 20 + 10 / 10 + rand(0..=2).
    assert!(healed.iter().all(|h| (225..=247).contains(h)), "{healed:?}");
    assert!(healed.iter().any(|&h| h != healed[0]), "{healed:?}");
    assert!(cheered.iter().all(|m| (21..=23).contains(m)), "{cheered:?}");
}

#[test]
fn original_formulas_confuse_until_recovered_and_on_low_morale_downs() {
    let pack = original_pack();
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "infantry", 10, p(3, 3));
    set_stats(&mut st, &pack, c, [50, 50, 50]);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(3, 4));
    set_stats(&mut st, &pack, foe, [50, 0, 50]);
    st.apply(&pack, cast(c, "confuse", p(3, 4))).unwrap();
    assert_eq!(
        st.units[foe].statuses,
        vec![ActiveStatus {
            status: StatusKind::Confused,
            turns: crate::battle::UNTIL_RECOVERED
        }],
        "no length: it lasts until a recovery roll"
    );

    // Morale 40 - 20 (+ 0 levels) = 20 < 30: confused with 60 %.
    let mut confused = 0;
    for seed in 0..400 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        let c = add(&mut st, &pack, Side::Player, "cavalry", 10, p(3, 3));
        set_stats(&mut st, &pack, c, [50, 50, 50]);
        let foe = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(3, 4));
        set_stats(&mut st, &pack, foe, [50, 0, 50]);
        st.units[foe].morale = 40;
        let ev = st.apply(&pack, cast(c, "provoke", p(3, 4))).unwrap();
        assert_eq!(st.units[foe].morale, 20);
        if st.units[foe].has_status(StatusKind::Confused) {
            assert_eq!(hits(&ev)[0].status, Some(StatusKind::Confused));
            assert!(ev.contains(&BattleEvent::Confused { unit: foe }));
            confused += 1;
        }
    }
    assert!((200..280).contains(&confused), "{confused} of 400");
    // Left with 30 or more: never.
    let mut st = state(&pack);
    let c = add(&mut st, &pack, Side::Player, "cavalry", 10, p(3, 3));
    set_stats(&mut st, &pack, c, [50, 50, 50]);
    let foe = add(&mut st, &pack, Side::Enemy, "infantry", 10, p(3, 4));
    set_stats(&mut st, &pack, foe, [50, 0, 50]);
    st.units[foe].morale = 50;
    st.apply(&pack, cast(c, "provoke", p(3, 4))).unwrap();
    assert!(st.units[foe].statuses.is_empty());
}

#[test]
fn original_formulas_add_a_random_tenth_to_healing_items() {
    let pack = original_pack();
    let (mut healed, mut raised) = (Vec::new(), Vec::new());
    for seed in 0..60 {
        let mut st = BattleState::new(&pack, BATTLE, &campaign(Vec::new(), &[]), seed).unwrap();
        for item in ["bean", "wine"] {
            st.inventory.insert(item.into(), 1);
        }
        let u = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 3));
        let friend = add(&mut st, &pack, Side::Player, "infantry", 1, p(3, 4));
        add(&mut st, &pack, Side::Enemy, "infantry", 1, p(7, 7));
        st.units[friend].hp = 1;
        st.units[friend].max_hp = 1000;
        st.units[friend].morale = 10;
        let used = |ev: Vec<BattleEvent>| match &ev[0] {
            BattleEvent::ItemUsed { healed, morale, .. } => (*healed, *morale),
            other => panic!("{other:?}"),
        };
        healed.push(used(st.apply(&pack, use_item(u, "bean", friend)).unwrap()).0);
        raised.push(used(st.apply(&pack, use_item(friend, "wine", friend)).unwrap()).1);
    }
    // 300 + rand(0..=30); 30 + rand(0..=3).
    assert!(healed.iter().all(|h| (300..=330).contains(h)), "{healed:?}");
    assert!(healed.iter().any(|&h| h != healed[0]), "{healed:?}");
    assert!(raised.iter().all(|m| (30..=33).contains(m)), "{raised:?}");
}
