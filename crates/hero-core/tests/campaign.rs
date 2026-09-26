//! `CampaignState`: new game, campaign flow, shop, equipment, class items and battle results.

mod common;

use common::*;
use hero_core::battle::BattleState;
use hero_core::battledef::Side;
use hero_core::campaign::{CampaignDef, CampaignError, CampaignState, Node};
use hero_core::data::{Equipment, ItemKind};
use hero_core::pack::Pack;
use hero_core::script::Compare;
use serde_json::{json, Value};

fn new_game() -> (Pack, CampaignState) {
    let pack = load_fixture();
    let state = CampaignState::new_game(&pack);
    (pack, state)
}

#[test]
fn new_game_uses_the_campaign_definition() {
    let (_, state) = new_game();
    assert_eq!(state.node, "prologue");
    let ids: Vec<&str> = state.roster.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["liu_bei", "guan_yu", "zhang_fei"]);
    let liu_bei = state.officer("liu_bei").unwrap();
    assert_eq!((liu_bei.level, liu_bei.exp), (5, 0));
    assert_eq!(liu_bei.class, "short_infantry");
    assert_eq!((liu_bei.strength, liu_bei.int, liu_bei.lead), (72, 75, 78));
    assert_eq!(liu_bei.equip.weapon.as_deref(), Some("bronze_sword"));
    assert_eq!(state.item_count("bean"), 3);
    assert_eq!(state.gold, 500);
    assert!(state.flags.is_empty() && state.deployed.is_empty() && state.battles_won.is_empty());
}

#[test]
fn campaign_state_survives_json() {
    let (_, state) = new_game();
    let json = serde_json::to_string(&state).unwrap();
    let back: CampaignState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, state);
    assert!(json.contains("\"str\":72"), "{json}");
}

#[test]
fn advance_walks_the_campaign() {
    let (pack, mut state) = new_game();
    assert_eq!(state.advance(&pack).unwrap(), "camp1");
    assert_eq!(state.advance(&pack).unwrap(), "battle1");
    // Victory, `pursue` unset: route -> camp2.
    assert_eq!(state.advance(&pack).unwrap(), "camp2");
    assert_eq!(state.advance(&pack).unwrap(), "battle2");
    assert_eq!(state.advance(&pack).unwrap(), "finale");
    // An ending has nothing after it.
    assert_eq!(state.advance(&pack).unwrap(), "finale");
    assert_eq!(state.node, "finale");
}

#[test]
fn branch_chains_follow_the_flags() {
    let (pack, mut state) = new_game();
    let after_battle1 = |state: &mut CampaignState| {
        state.node = "battle1".into();
        state.advance(&pack).unwrap()
    };
    state.flags.insert("pursue".into(), 1);
    assert_eq!(after_battle1(&mut state), "camp2", "captives < 2");
    state.flags.insert("captives".into(), 2);
    assert_eq!(
        after_battle1(&mut state),
        "mercy",
        "route -> mercy_check -> mercy"
    );
    state.flags.insert("pursue".into(), 0);
    assert_eq!(after_battle1(&mut state), "camp2");

    // A current branch node is resolved by `advance`.
    state.node = "mercy_check".into();
    assert_eq!(state.advance(&pack).unwrap(), "mercy");
}

#[test]
fn jump_resolves_branches_for_on_defeat() {
    let (pack, mut state) = new_game();
    state.node = "battle1".into();
    let on_defeat = match pack.campaign.node(&state.node).unwrap() {
        Node::Battle { on_defeat, .. } => on_defeat.clone().unwrap(),
        other => panic!("not a battle node: {other:?}"),
    };
    assert_eq!(state.jump(&pack, &on_defeat).unwrap(), "retreat");
    assert_eq!(state.advance(&pack).unwrap(), "camp2");

    state.flags.insert("pursue".into(), 1);
    state.flags.insert("captives".into(), 3);
    assert_eq!(state.jump(&pack, "route").unwrap(), "mercy");

    assert_eq!(
        state.jump(&pack, "nowhere"),
        Err(CampaignError::UnknownNode("nowhere".into()))
    );
    assert_eq!(state.node, "mercy", "state is unchanged on error");
}

#[test]
fn unknown_current_node_is_an_error() {
    let (pack, mut state) = new_game();
    state.node = "lost".into();
    assert_eq!(
        state.advance(&pack),
        Err(CampaignError::UnknownNode("lost".into()))
    );
}

#[test]
fn looping_branches_are_an_error() {
    let mut files = fixture_files();
    edit(
        &mut files,
        "campaign.toml",
        "value = 2\nthen = \"mercy\"\nelse = \"camp2\"",
        "value = 2\nthen = \"mercy\"\nelse = \"route\"",
    );
    let pack = load(&files);
    let mut state = CampaignState::new_game(&pack);
    state.node = "battle1".into();
    state.flags.insert("pursue".into(), 1);
    assert_eq!(
        state.advance(&pack),
        Err(CampaignError::BranchLoop("route".into()))
    );
    assert_eq!(state.node, "battle1");
}

#[test]
fn branch_comparison_accepts_operators_and_names() {
    for (cmp, expected) in [
        ("==", Compare::Eq),
        ("!=", Compare::Ne),
        ("<", Compare::Lt),
        ("le", Compare::Le),
        ("Gt", Compare::Gt),
        (">=", Compare::Ge),
    ] {
        let src = format!(
            "title = \"t\"\nstart = \"a\"\nstarting_officers = []\n[[node]]\ntype = \"branch\"\nid = \"a\"\nflag = \"f\"\ncmp = \"{cmp}\"\nthen = \"a\"\nelse = \"a\"\n"
        );
        let def: CampaignDef = toml::from_str(&src).unwrap();
        assert!(
            matches!(&def.nodes[0], Node::Branch { cmp, .. } if *cmp == expected),
            "{cmp}"
        );
        // The serialized form (variant name) parses back.
        let json = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<CampaignDef>(&json).unwrap(), def);
    }
    let bad = "title = \"t\"\nstart = \"a\"\nstarting_officers = []\n[[node]]\ntype = \"branch\"\nid = \"a\"\nflag = \"f\"\ncmp = \"~=\"\nthen = \"a\"\nelse = \"a\"\n";
    let err = toml::from_str::<CampaignDef>(bad).unwrap_err().to_string();
    assert!(err.contains("unknown comparison `~=`"), "{err}");
}

#[test]
fn join_and_leave() {
    let (pack, mut state) = new_game();
    state.join(&pack, "jian_yong").unwrap();
    let jian_yong = state.officer("jian_yong").unwrap();
    assert_eq!((jian_yong.class.as_str(), jian_yong.level), ("sorcerer", 4));
    state.join(&pack, "jian_yong").unwrap();
    assert_eq!(state.roster.len(), 4, "joining twice is a no-op");
    assert_eq!(
        state.join(&pack, "cao_cao"),
        Err(CampaignError::UnknownOfficer("cao_cao".into()))
    );

    state.deployed = vec!["liu_bei".into(), "guan_yu".into()];
    state.leave("liu_bei").unwrap();
    assert!(state.officer("liu_bei").is_none());
    assert_eq!(
        state.item_count("bronze_sword"),
        1,
        "equipment returns to the inventory"
    );
    assert_eq!(state.deployed, ["guan_yu"]);
    assert_eq!(
        state.leave("liu_bei"),
        Err(CampaignError::NotInArmy("liu_bei".into()))
    );
}

#[test]
fn inventory_and_gold() {
    let (pack, mut state) = new_game();
    state.remove_item("bean").unwrap();
    assert_eq!(state.item_count("bean"), 2);
    state.remove_item("bean").unwrap();
    state.remove_item("bean").unwrap();
    assert!(
        !state.inventory.contains_key("bean"),
        "empty entries are removed"
    );
    assert_eq!(
        state.remove_item("bean"),
        Err(CampaignError::NotOwned("bean".into()))
    );
    state.add_item("wine", 0);
    assert!(!state.inventory.contains_key("wine"));

    state.add_gold(&pack, 100_000);
    assert_eq!(state.gold, 9999, "clamped to gold_cap");
    state.add_gold(&pack, -20_000);
    assert_eq!(state.gold, 0, "never negative");
}

#[test]
fn shop_buy_and_sell() {
    let (pack, mut state) = new_game();
    state.buy(&pack, "bronze_sword").unwrap();
    assert_eq!((state.gold, state.item_count("bronze_sword")), (300, 1));
    // Exactly enough gold is enough.
    state.buy(&pack, "long_spear").unwrap();
    assert_eq!(state.gold, 0);
    assert_eq!(
        state.buy(&pack, "bean"),
        Err(CampaignError::NotEnoughGold { need: 20, have: 0 })
    );
    assert_eq!(
        state.buy(&pack, "fire_scroll"),
        Err(CampaignError::CannotBuy("fire_scroll".into()))
    );
    assert_eq!(
        state.buy(&pack, "peach"),
        Err(CampaignError::UnknownItem("peach".into()))
    );

    state.sell(&pack, "bronze_sword").unwrap();
    assert_eq!(
        (state.gold, state.item_count("bronze_sword")),
        (100, 0),
        "sold for half"
    );
    assert_eq!(
        state.sell(&pack, "bronze_sword"),
        Err(CampaignError::NotOwned("bronze_sword".into()))
    );
    state.add_item("red_horse", 1);
    assert_eq!(
        state.sell(&pack, "red_horse"),
        Err(CampaignError::CannotSell("red_horse".into()))
    );
    assert_eq!(state.item_count("red_horse"), 1);
}

#[test]
fn equip_and_unequip() {
    let (pack, mut state) = new_game();
    state.add_item("bronze_sword", 2);
    state.add_item("short_bow", 1);
    state.add_item("war_manual", 1);

    state.equip(&pack, "guan_yu", "bronze_sword").unwrap();
    assert_eq!(
        state.officer("guan_yu").unwrap().equip.weapon.as_deref(),
        Some("bronze_sword")
    );
    assert_eq!(state.item_count("bronze_sword"), 1);

    // Swapping puts the old item back.
    state.add_item("red_horse", 1);
    state.equip(&pack, "guan_yu", "red_horse").unwrap();
    state.equip(&pack, "guan_yu", "war_manual").unwrap();
    assert_eq!(
        state.officer("guan_yu").unwrap().equip,
        Equipment {
            weapon: Some("bronze_sword".into()),
            armor: Some("war_manual".into()),
            accessory: Some("red_horse".into()),
        }
    );
    state.equip(&pack, "guan_yu", "bronze_sword").unwrap();
    assert_eq!(
        state.item_count("bronze_sword"),
        1,
        "the swapped-out sword came back"
    );

    assert_eq!(
        state.equip(&pack, "guan_yu", "short_bow"),
        Err(CampaignError::CannotEquip {
            item: "short_bow".into(),
            family: "infantry".into()
        })
    );
    assert_eq!(
        state.equip(&pack, "guan_yu", "bean"),
        Err(CampaignError::NotEquipment("bean".into()))
    );
    assert_eq!(
        state.equip(&pack, "guan_yu", "war_manual"),
        Err(CampaignError::NotOwned("war_manual".into()))
    );
    assert_eq!(
        state.equip(&pack, "cao_cao", "bronze_sword"),
        Err(CampaignError::NotInArmy("cao_cao".into()))
    );
    assert_eq!(
        state.equip(&pack, "guan_yu", "halberd"),
        Err(CampaignError::UnknownItem("halberd".into()))
    );

    state.unequip("guan_yu", ItemKind::Armor).unwrap();
    assert_eq!(state.item_count("war_manual"), 1);
    assert!(state.officer("guan_yu").unwrap().equip.armor.is_none());
    let before = state.clone();
    state.unequip("guan_yu", ItemKind::Armor).unwrap();
    state.unequip("guan_yu", ItemKind::Consumable).unwrap();
    assert_eq!(state, before, "unequipping an empty slot changes nothing");
    assert_eq!(
        state.unequip("cao_cao", ItemKind::Weapon),
        Err(CampaignError::NotInArmy("cao_cao".into()))
    );
}

#[test]
fn promotion_item() {
    let (pack, mut state) = new_game();
    state.add_item("long_spear", 3);

    // liu_bei is short_infantry but only level 5 (promotion needs 15).
    let err = state.use_item(&pack, "liu_bei", "long_spear").unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("needs level 15")),
        "{err}"
    );
    // zhang_fei's class has no promotion.
    let err = state
        .use_item(&pack, "zhang_fei", "long_spear")
        .unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("has no promotion")),
        "{err}"
    );
    assert_eq!(
        state.item_count("long_spear"),
        3,
        "failed uses consume nothing"
    );

    state.use_item(&pack, "guan_yu", "long_spear").unwrap();
    let guan_yu = state.officer("guan_yu").unwrap();
    assert_eq!(
        (guan_yu.class.as_str(), guan_yu.level),
        ("long_infantry", 15)
    );
    assert_eq!(state.item_count("long_spear"), 2);

    // long_infantry has no further promotion.
    assert!(matches!(
        state.use_item(&pack, "guan_yu", "long_spear"),
        Err(CampaignError::CannotUse { .. })
    ));
    assert_eq!(
        state.use_item(&pack, "guan_yu", "archery_manual"),
        Err(CampaignError::NotOwned("archery_manual".into()))
    );
}

#[test]
fn promotion_needs_the_matching_item() {
    let mut files = fixture_files();
    edit(
        &mut files,
        "rules/items.toml",
        "[[item]]\nid = \"archery_manual\"",
        "[[item]]\nid = \"horse_armor\"\nname = \"마개\"\nkind = \"consumable\"\nprice = 400\neffects = [{ type = \"promote\" }]\n\n[[item]]\nid = \"archery_manual\"",
    );
    let pack = load(&files);
    let mut state = CampaignState::new_game(&pack);
    state.add_item("horse_armor", 1);
    let err = state.use_item(&pack, "guan_yu", "horse_armor").unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("promoted with `long_spear`")),
        "{err}"
    );
}

#[test]
fn class_change_item() {
    let (pack, mut state) = new_game();
    state.add_item("archery_manual", 2);
    state.add_item("red_horse", 1);
    state.equip(&pack, "guan_yu", "red_horse").unwrap();
    state.add_item("bronze_sword", 1);
    state.equip(&pack, "guan_yu", "bronze_sword").unwrap();

    let err = state
        .use_item(&pack, "liu_bei", "archery_manual")
        .unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("cannot change class")),
        "{err}"
    );

    state.use_item(&pack, "guan_yu", "archery_manual").unwrap();
    let guan_yu = state.officer("guan_yu").unwrap();
    assert_eq!((guan_yu.class.as_str(), guan_yu.level), ("archer", 15));
    // The sword is not for archers and goes back; the horse (any family) stays.
    assert_eq!(guan_yu.equip.weapon, None);
    assert_eq!(guan_yu.equip.accessory.as_deref(), Some("red_horse"));
    assert_eq!(state.item_count("bronze_sword"), 1);
    assert_eq!(state.item_count("archery_manual"), 1);

    let err = state
        .use_item(&pack, "guan_yu", "archery_manual")
        .unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("already")),
        "{err}"
    );
    let err = state.use_item(&pack, "zhang_fei", "bean").unwrap_err();
    assert!(
        matches!(&err, CampaignError::CannotUse { reason, .. } if reason.contains("no effect outside battle")),
        "{err}"
    );
}

// ----- apply_battle_result ------------------------------------------------------------------

/// One serialized battle unit; `progress` is `(level, exp)`.
fn unit(
    id: usize,
    side: &str,
    officer: Option<&str>,
    class: &str,
    progress: (u32, u32),
    stats: [i32; 3],
    equip: Value,
) -> Value {
    let (level, exp) = progress;
    let lord = officer == Some("liu_bei");
    json!({
        "id": id, "side": side, "officer": officer, "name": officer.unwrap_or("황건적"),
        "class": class, "level": level, "exp": exp,
        "str": stats[0], "int": stats[1], "lead": stats[2],
        "hp": 0, "max_hp": 700, "mp": 0, "max_mp": 20, "morale": 60,
        "pos": [0, id], "facing": "down", "moved": false, "acted": false,
        "equip": equip, "statuses": [],
        "ai": "aggressive", "ai_target": null, "ai_pos": null,
        "commander": false, "lord": lord,
        "tag": null, "group": null, "state": "active", "portrait": null, "drop": null
    })
}

/// A finished b01 built from JSON (without `BattleState::new`, which belongs to the battle
/// engine): guan_yu gained levels and a promotion, a bean and a wine were used.
fn finished_battle(outcome: Value) -> BattleState {
    let units = vec![
        unit(
            0,
            "player",
            Some("liu_bei"),
            "short_infantry",
            (6, 20),
            [72, 75, 78],
            json!({ "weapon": "bronze_sword" }),
        ),
        unit(
            1,
            "player",
            Some("guan_yu"),
            "long_infantry",
            (17, 45),
            [98, 75, 95],
            json!({ "weapon": "bronze_sword", "armor": "war_manual" }),
        ),
        // Allied copy of an army officer: must not overwrite the army's zhang_fei.
        unit(
            2,
            "ally",
            Some("zhang_fei"),
            "light_cavalry",
            (30, 99),
            [1, 1, 1],
            json!({}),
        ),
        // Player guest who is not in the army: ignored.
        unit(
            3,
            "player",
            Some("jian_yong"),
            "sorcerer",
            (9, 10),
            [30, 70, 40],
            json!({}),
        ),
        unit(
            4,
            "enemy",
            Some("zhang_bao"),
            "bandit",
            (7, 0),
            [60, 50, 60],
            json!({}),
        ),
    ];
    serde_json::from_value(json!({
        "battle_id": "b01",
        "map": { "width": 1, "height": 5, "terrain_ids": ["plain"], "tiles": [0, 0, 0, 0, 0] },
        "units": units,
        "turn": 7, "turn_limit": 20, "phase": "player", "weather": "clear",
        "rng": { "state": 1 },
        "fired": [true, false, true, false, false],
        "treasures_taken": [true],
        "outcome": outcome,
        "bonus_done": false,
        "inventory": { "bean": 2, "wine": 1, "war_manual": 5 },
        "gold_found": 250,
        "items_found": ["red_horse", "fire_scroll"],
        "flags": { "captives": 2, "pursue": 1 }
    }))
    .expect("battle state JSON matches BattleState")
}

#[test]
fn victory_copies_progress_and_rewards() {
    let (pack, mut state) = new_game();
    state.add_item("wine", 3);
    state.add_item("bronze_sword", 1); // not a battle item: kept as it is
    state.flags.insert("pursue".into(), 0);
    state.flags.insert("oath".into(), 1);
    state.apply_battle_result(&pack, &finished_battle(json!("victory")));

    let guan_yu = state.officer("guan_yu").unwrap();
    assert_eq!(
        (guan_yu.level, guan_yu.exp, guan_yu.class.as_str()),
        (17, 45, "long_infantry")
    );
    assert_eq!((guan_yu.strength, guan_yu.int, guan_yu.lead), (98, 75, 95));
    assert_eq!(guan_yu.equip.armor.as_deref(), Some("war_manual"));
    assert_eq!(state.officer("liu_bei").unwrap().level, 6);
    let zhang_fei = state.officer("zhang_fei").unwrap();
    assert_eq!(
        (zhang_fei.level, zhang_fei.strength),
        (6, 98),
        "allied units are not copied"
    );
    assert!(state.officer("jian_yong").is_none(), "guests do not join");

    // Battle items come from the battle; others (bronze_sword) stay; the battle's non-battle
    // entries (war_manual) are ignored.
    assert_eq!(state.item_count("bean"), 2);
    assert_eq!(state.item_count("wine"), 1);
    assert_eq!(state.item_count("bronze_sword"), 1);
    assert_eq!(state.item_count("war_manual"), 0);
    // Found items and gold.
    assert_eq!(state.item_count("red_horse"), 1);
    assert_eq!(state.item_count("fire_scroll"), 1);
    assert_eq!(state.gold, 750);
    assert_eq!(state.battles_won, ["b01"]);
    assert_eq!(
        (
            state.flag("captives"),
            state.flag("pursue"),
            state.flag("oath")
        ),
        (2, 1, 1)
    );

    // Applying the same victory twice records the battle once.
    state.apply_battle_result(&pack, &finished_battle(json!("victory")));
    assert_eq!(state.battles_won, ["b01"]);
}

/// Player officers placed by a battle, in unit order.
fn placed(battle: &BattleState) -> Vec<&str> {
    battle
        .units
        .iter()
        .filter(|u| u.side == Side::Player)
        .filter_map(|u| u.officer.as_deref())
        .collect()
}

#[test]
fn the_next_battle_fits_the_last_deployment_to_its_rules() {
    let (pack, mut state) = new_game();
    state.join(&pack, "jian_yong").unwrap();
    state.deployed = vec!["liu_bei".into(), "guan_yu".into(), "zhang_fei".into()];
    let b01 = BattleState::new(&pack, "b01", &state, 1).unwrap();
    assert_eq!(placed(&b01), ["liu_bei", "guan_yu", "zhang_fei"]);
    state.apply_battle_result(&pack, &b01);

    // b02 requires jian_yong and forbids zhang_fei. Without a deploy screen in between the
    // b01 choice is still stored; the battle fits it to its own rules instead of failing.
    let b02 = BattleState::new(&pack, "b02", &state, 1).unwrap();
    assert_eq!(placed(&b02), ["jian_yong", "liu_bei", "guan_yu"]);
    assert_eq!(
        state.deployed,
        ["liu_bei", "guan_yu", "zhang_fei"],
        "the player's choice is kept for later battles"
    );
}

#[test]
fn defeat_keeps_progress_but_not_rewards() {
    let (pack, mut state) = new_game();
    state.apply_battle_result(&pack, &finished_battle(json!({ "defeat": "turn_limit" })));
    assert_eq!(state.officer("guan_yu").unwrap().level, 17);
    assert_eq!(state.item_count("bean"), 2, "used items stay used");
    assert_eq!(state.flag("captives"), 2);
    assert_eq!(state.gold, 500);
    assert_eq!(state.item_count("red_horse"), 0);
    assert!(state.battles_won.is_empty());
}
