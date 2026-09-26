//! `Pack::validate` and `Pack::unknown_fields`: the fixture is clean, and each deliberately
//! broken copy reports the expected issue.

mod common;

use common::*;
use hero_core::pack::{Pack, Severity};

#[test]
fn fixture_pack_has_no_issues() {
    let issues = load_fixture().validate();
    assert!(
        issues.is_empty(),
        "unexpected issues:\n{}",
        format_issues(&issues)
    );
}

#[test]
fn fixture_pack_has_no_unknown_fields() {
    let issues = Pack::unknown_fields(&fixture_files()).expect("fixture loads");
    assert!(
        issues.is_empty(),
        "unexpected issues:\n{}",
        format_issues(&issues)
    );
}

#[test]
fn unknown_fields_are_reported() {
    let mut files = fixture_files();
    edit(
        &mut files,
        CLASSES,
        "can_counter = true",
        "can_couter = true",
    );
    edit(
        &mut files,
        B01,
        "reward_gold = 200",
        "reward_gold = 200\nreward_items = 1",
    );
    edit(
        &mut files,
        CAMPAIGN,
        "cmp = \">=\"",
        "cmp = \">=\"\ncomment = \"x\"",
    );
    // Misspelt optional fields load silently with their default ...
    assert!(!load(&files).class("bandit").unwrap().can_counter);
    // ... which is what the lint is for.
    let issues = Pack::unknown_fields(&files).expect("pack parses");
    assert_issue(
        &issues,
        Severity::Warning,
        CLASSES,
        "unknown field `class[bandit].can_couter`",
    );
    assert_issue(
        &issues,
        Severity::Warning,
        "battles/b01.toml",
        "unknown field `reward_items`",
    );
    assert_issue(
        &issues,
        Severity::Warning,
        "campaign.toml",
        "unknown field `node[mercy_check].comment`",
    );
    assert_eq!(issues.len(), 3, "{}", format_issues(&issues));
}

/// One deliberately broken copy of the fixture: `(file, from, to)` edits, then the issue that
/// must be reported.
struct Case {
    edits: &'static [(&'static str, &'static str, &'static str)],
    severity: Severity,
    context: &'static str,
    msg: &'static str,
}

const fn error(
    edits: &'static [(&'static str, &'static str, &'static str)],
    context: &'static str,
    msg: &'static str,
) -> Case {
    Case {
        edits,
        severity: Severity::Error,
        context,
        msg,
    }
}

const fn warning(
    edits: &'static [(&'static str, &'static str, &'static str)],
    context: &'static str,
    msg: &'static str,
) -> Case {
    Case {
        edits,
        severity: Severity::Warning,
        context,
        msg,
    }
}

const GAME: &str = "rules/game.toml";
const TERRAIN: &str = "rules/terrain.toml";
const CLASSES: &str = "rules/classes.toml";
const STRATEGIES: &str = "rules/strategies.toml";
const ITEMS: &str = "rules/items.toml";
const OFFICERS: &str = "officers.toml";
const CAMPAIGN: &str = "campaign.toml";
const B01: &str = "battles/b01.toml";
const B02: &str = "battles/b02.toml";
const STORY: &str = "dramas/story.drama";
const BATTLE_DRAMAS: &str = "dramas/battles.drama";

fn run(cases: &[Case]) {
    for case in cases {
        let mut files = fixture_files();
        for (file, from, to) in case.edits {
            edit(&mut files, file, from, to);
        }
        let issues = load(&files).validate();
        assert_issue(&issues, case.severity, case.context, case.msg);
    }
}

#[test]
fn rules_checks() {
    run(&[
        error(
            &[(GAME, "rain = 15", "rain = 5")],
            GAME,
            "sum to 100 (they sum to 90)",
        ),
        error(
            &[(
                GAME,
                "[[-99, 1], [-5, 4], [0, 8], [5, 12]]",
                "[[-99, 1], [0, 8], [-5, 4]]",
            )],
            GAME,
            "exp_attack must be sorted",
        ),
        error(
            &[(GAME, "counter_divisor = 150", "counter_divisor = 0")],
            GAME,
            "counter_divisor must be positive",
        ),
        error(
            &[(GAME, "morale_start = 100", "morale_start = 120")],
            GAME,
            "morale_start",
        ),
        warning(
            &[(GAME, "[affinity.archer]", "[affinity.navy]")],
            GAME,
            "unknown class family `navy`",
        ),
    ]);
}

#[test]
fn terrain_checks() {
    run(&[
        error(
            &[(TERRAIN, "cost = { special = 2 }", "cost = { special = 0 }")],
            "terrain mountain",
            "must be at least 1",
        ),
        error(
            &[(TERRAIN, "defense = 30", "defense = 130")],
            "terrain mountain",
            "defense must be within 0..=100",
        ),
        warning(
            &[(
                TERRAIN,
                "cost = { special = 2 }",
                "cost = { special = 2, boat = 1 }",
            )],
            "terrain mountain",
            "move type `boat`, which no class uses",
        ),
        warning(
            &[(
                TERRAIN,
                "elements = [\"earth\"]",
                "elements = [\"earth\", \"wind\"]",
            )],
            "terrain mountain",
            "element `wind` is not used by any strategy",
        ),
        warning(
            &[(
                TERRAIN,
                "boost = [\"fire\"]",
                "boost = [\"fire\", \"water\"]",
            )],
            "terrain forest",
            "boosted element `water`",
        ),
    ]);
}

#[test]
fn class_checks() {
    run(&[
        error(&[(CLASSES, "to = \"long_infantry\"", "to = \"pike_infantry\"")], "class short_infantry", "promotes to unknown class `pike_infantry`"),
        error(&[(CLASSES, "item = \"long_spear\"", "item = \"long_pike\"")], "class short_infantry", "promotion item `long_pike` does not exist"),
        error(&[(CLASSES, "item = \"long_spear\"", "item = \"bean\"")], "class short_infantry", "has no `promote` effect"),
        error(
            &[(CLASSES, "strategies = [{ level = 20, id = \"heal\" }]", "strategies = [{ level = 20, id = \"heal\" }]\npromote = { to = \"short_infantry\", level = 30, item = \"long_spear\" }")],
            "class long_infantry",
            "promotion chain loops: long_infantry -> short_infantry -> long_infantry",
        ),
        error(&[(CLASSES, "{ level = 3, id = \"rockfall\" }", "{ level = 3, id = \"landslide\" }")], "class bandit", "learns unknown strategy `landslide`"),
        error(&[(CLASSES, "range = \"archer\"", "range = \"longbow\"")], "class archer", "unknown attack range `longbow`"),
        error(&[(CLASSES, "move_type = \"foot\"\nrange = [[0, -1]", "move_type = \"float\"\nrange = [[0, -1]")], "class sorcerer", "move type `float` has a cost on no terrain"),
        error(&[(CLASSES, "hp = 300", "hp = 0")], "class sorcerer", "hp must be positive"),
        warning(&[(CLASSES, "{ level = 3, id = \"rockfall\" }", "{ level = 99, id = \"rockfall\" }")], "class bandit", "at level 99, outside 1..=50"),
    ]);
}

#[test]
fn strategy_checks() {
    run(&[
        error(
            &[(STRATEGIES, "element = \"earth\"", "element = \"wind\"")],
            "strategy rockfall",
            "element `wind` is allowed by no terrain",
        ),
        error(
            &[(STRATEGIES, "range = \"range8\"", "range = \"range99\"")],
            "strategy heal",
            "unknown range `range99`",
        ),
        error(
            &[(
                STRATEGIES,
                "effects = [{ type = \"heal\", power = 100 }]",
                "effects = [{ type = \"promote\" }]",
            )],
            "strategy heal",
            "only work on items",
        ),
        error(
            &[(
                STRATEGIES,
                "effects = [{ type = \"heal\", power = 100 }]",
                "effects = []",
            )],
            "strategy heal",
            "has no effects",
        ),
        warning(
            &[(STRATEGIES, "target = \"ally\"", "target = \"enemy\"")],
            "strategy heal",
            "heal strategy is aimed at enemies",
        ),
    ]);
}

#[test]
fn item_checks() {
    run(&[
        error(
            &[(
                ITEMS,
                "families = [\"infantry\", \"bandit\"]",
                "families = [\"infantry\", \"pirate\"]",
            )],
            "item bronze_sword",
            "unknown class family `pirate`",
        ),
        error(
            &[(ITEMS, "strategy = \"fire\"", "strategy = \"blaze\"")],
            "item fire_scroll",
            "casts unknown strategy `blaze`",
        ),
        error(
            &[(ITEMS, "to = \"archer\" }]", "to = \"ranger\" }]")],
            "item archery_manual",
            "changes to unknown class `ranger`",
        ),
        warning(
            &[(ITEMS, "def_pct = 110", "def_pct = 110\natk_pct = 105")],
            "item war_manual",
            "atk_pct only counts on weapons",
        ),
        warning(
            &[(
                ITEMS,
                "effects = [{ type = \"morale\", amount = 20 }]\nbattle_use = true",
                "effects = [{ type = \"morale\", amount = 20 }]",
            )],
            "item wine",
            "without `battle_use = true` can never be used",
        ),
        warning(
            &[(
                CLASSES,
                "item = \"long_spear\"",
                "item = \"archery_manual\"",
            )],
            "item long_spear",
            "no class is promoted with this item",
        ),
    ]);
}

#[test]
fn officer_checks() {
    run(&[
        error(
            &[(
                OFFICERS,
                "class = \"light_cavalry\"",
                "class = \"horse_archer\"",
            )],
            "officer zhang_fei",
            "unknown class `horse_archer`",
        ),
        error(
            &[(OFFICERS, "level = 15", "level = 0")],
            "officer guan_yu",
            "level 0 is outside 1..=50",
        ),
        error(
            &[(
                OFFICERS,
                "equip = { weapon = \"bronze_sword\" }",
                "equip = { weapon = \"war_manual\" }",
            )],
            "officer liu_bei",
            "`war_manual` (armor) cannot go in the weapon slot",
        ),
        error(
            &[(
                OFFICERS,
                "equip = { weapon = \"bronze_sword\" }",
                "equip = { weapon = \"iron_sword\" }",
            )],
            "officer liu_bei",
            "weapon `iron_sword` does not exist",
        ),
        warning(
            &[(
                OFFICERS,
                "equip = { weapon = \"bronze_sword\" }",
                "equip = { weapon = \"short_bow\" }",
            )],
            "officer liu_bei",
            "not meant for class family `infantry`",
        ),
    ]);
}

#[test]
fn deploy_checks() {
    const SLOTS: &str = "slots = [[1, 6], [2, 6], [3, 6], [2, 7]]";
    run(&[
        error(
            &[(B01, SLOTS, "slots = [[1, 6], [2, 6], [3, 6], [20, 7]]")],
            "battle b01 deploy slot [20, 7]",
            "is outside the 10x8 map",
        ),
        error(
            &[(B01, SLOTS, "slots = [[1, 6], [2, 6], [3, 6], [0, 4]]")],
            "battle b01 deploy slot [0, 4]",
            "`river`, which foot units cannot enter",
        ),
        error(
            &[(B01, SLOTS, "slots = [[1, 6], [2, 6], [3, 6], [1, 6]]")],
            "battle b01 deploy slot [1, 6]",
            "is listed twice",
        ),
        error(
            &[(B01, "max = 3", "max = 5")],
            "battle b01",
            "deploy.max is 5 but only 4 deploy slots exist",
        ),
        error(
            &[(B01, "max = 3", "max = 0")],
            "battle b01",
            "deploy.max must be at least 1",
        ),
        error(
            &[(B02, "required = [\"jian_yong\"]", "required = [\"mi_zhu\"]")],
            "battle b02",
            "required officer `mi_zhu` does not exist",
        ),
        error(
            &[(
                B02,
                "forbidden = [\"zhang_fei\"]",
                "forbidden = [\"liu_bei\"]",
            )],
            "battle b02",
            "the lord `liu_bei` is always deployed",
        ),
        error(
            &[(
                B02,
                "forbidden = [\"zhang_fei\"]",
                "forbidden = [\"jian_yong\"]",
            )],
            "battle b02",
            "`jian_yong` is both required and forbidden",
        ),
        error(
            &[
                (
                    B02,
                    "required = [\"jian_yong\"]",
                    "required = [\"jian_yong\", \"guan_yu\", \"zhang_fei\"]",
                ),
                (B02, "forbidden = [\"zhang_fei\"]", "forbidden = []"),
            ],
            "battle b02",
            "4 officers must be deployed (required officers and the lord) but deploy.max is 3",
        ),
    ]);
}

#[test]
fn unit_checks() {
    run(&[
        error(
            &[(B01, "pos = [4, 3]", "pos = [40, 3]")],
            "battle b01 unit #2 (deng_mao)",
            "position [40, 3] is outside the 10x8 map",
        ),
        error(
            &[(B01, "pos = [7, 3]", "pos = [7, 4]")],
            "battle b01 unit #4",
            "on `river`, which move type `foot` cannot enter",
        ),
        error(
            &[(B01, "pos = [4, 3]", "pos = [5, 2]")],
            "battle b01 unit #3",
            "position [5, 2] is already taken",
        ),
        error(
            &[(B01, "pos = [8, 6]", "pos = [1, 6]")],
            "battle b01 unit #5 (militia)",
            "already taken by another unit or a deploy slot",
        ),
        error(
            &[(
                B01,
                "name = \"황건적\"\nclass = \"bandit\"\n",
                "name = \"황건적\"\n",
            )],
            "battle b01 unit #3",
            "a generic unit needs a class",
        ),
        error(
            &[(B01, "level = 3\npos = [5, 2]", "pos = [5, 2]")],
            "battle b01 unit #3",
            "a generic unit needs a level",
        ),
        error(
            &[(B01, "officer = \"deng_mao\"", "officer = \"deng_mau\"")],
            "battle b01 unit #2",
            "unknown officer `deng_mau`",
        ),
        error(
            &[(B01, "officer = \"deng_mao\"", "officer = \"zhang_bao\"")],
            "battle b01 unit #2",
            "appears more than once",
        ),
        error(
            &[(B01, "tag = \"militia\"", "tag = \"boss\"")],
            "battle b01 unit #5",
            "duplicate tag `boss`",
        ),
        error(
            &[(B01, "ai_target = \"liu_bei\"", "ai_target = \"cao_cao\"")],
            "battle b01 unit #4",
            "ai_target `cao_cao` names no unit",
        ),
        error(
            &[(B01, "ai_target = \"liu_bei\"\n", "")],
            "battle b01 unit #4",
            "needs an ai_target",
        ),
        error(
            &[(B01, "drop = \"war_manual\"", "drop = \"jade_seal\"")],
            "battle b01 unit #1 (boss)",
            "drops unknown item `jade_seal`",
        ),
        error(
            &[(B01, "ai_pos = [3, 4]", "ai_pos = [3, 44]")],
            "battle b01 unit #6",
            "ai_pos [3, 44] is outside the map",
        ),
        error(
            &[(
                B02,
                "equip = { weapon = \"bronze_sword\" }",
                "equip = { armor = \"bronze_sword\" }",
            )],
            "battle b02 unit #3 (chief)",
            "cannot go in the armor slot",
        ),
        warning(
            &[(B01, "pos = [9, 0]", "pos = [0, 4]")],
            "battle b01 unit #6",
            "the reinforcement will be shifted",
        ),
        warning(
            &[(
                B01,
                "officer = \"deng_mao\"",
                "officer = \"deng_mao\"\nstats = [1, 2, 3]",
            )],
            "battle b01 unit #2",
            "stats are ignored for named officers",
        ),
        warning(
            &[(B01, "tag = \"militia\"", "tag = \"guan_yu\"")],
            "battle b01 unit #5",
            "tag `guan_yu` is also an officer id",
        ),
        warning(
            &[(
                B01,
                "side = \"ally\"\nname = \"의용병\"",
                "side = \"player\"\nofficer = \"guan_yu\"",
            )],
            "battle b01 unit #5 (militia)",
            "officer `guan_yu` is in the starting army: the battle places the army's `guan_yu` here",
        ),
    ]);
}

#[test]
fn player_guests_are_not_army_officers() {
    // jian_yong only joins later (`@join` in a story scene): a guest, not a copy of the army's.
    let mut files = fixture_files();
    edit(
        &mut files,
        B01,
        "side = \"ally\"\nname = \"의용병\"",
        "side = \"player\"\nofficer = \"jian_yong\"",
    );
    let issues = load(&files).validate();
    assert!(
        !issues.iter().any(|i| i.msg.contains("starting army")),
        "{}",
        format_issues(&issues)
    );
}

#[test]
fn battle_logic_checks() {
    run(&[
        error(
            &[(B01, "commander = true\ntag = \"boss\"", "tag = \"boss\"")],
            "battle b01 victory",
            "no enemy unit is a commander",
        ),
        error(
            &[(
                B01,
                "defeat = [{ type = \"unit_retreated\", target = \"militia\" }]",
                "defeat = [{ type = \"unit_retreated\", target = \"villagers\" }]",
            )],
            "battle b01 defeat",
            "target `villagers` matches no unit",
        ),
        error(
            &[(B02, "victory = [{ type = \"defeat_all\" }]", "victory = []")],
            "battle b02",
            "no victory condition and no event grants victory",
        ),
        error(
            &[(
                B01,
                "a = \"guan_yu\", b = \"boss\"",
                "a = \"guan_yu\", b = \"chief\"",
            )],
            "battle b01 event #2",
            "b `chief` matches no unit",
        ),
        error(
            &[(
                B01,
                "{ type = \"spawn\", group = \"rein\" }",
                "{ type = \"spawn\", group = \"rain\" }",
            )],
            "battle b01 event #1",
            "spawns group `rain`, but no unit belongs to it",
        ),
        warning(
            &[(
                B01,
                "{ type = \"spawn\", group = \"rein\" }",
                "{ type = \"spawn\", group = \"rain\" }",
            )],
            "battle b01",
            "units of group `rein` never appear",
        ),
        error(
            &[(B01, "scene = \"b01_duel\"", "scene = \"b01_fight\"")],
            "battle b01 event #2",
            "plays unknown scene `b01_fight`",
        ),
        error(
            &[(B01, "item = \"fire_scroll\"", "item = \"ice_scroll\"")],
            "battle b01 event #5",
            "gives unknown item `ice_scroll`",
        ),
        error(
            &[(B01, "pct = 50", "pct = 0")],
            "battle b01 event #4",
            "pct must be within 1..=100",
        ),
        error(
            &[(B01, "pos = [8, 1], radius = 1", "pos = [8, 11], radius = 1")],
            "battle b01 event #5",
            "position [8, 11] is outside the map",
        ),
        error(
            &[(
                B01,
                "pos = [8, 1]\nitem = \"red_horse\"",
                "pos = [18, 1]\nitem = \"red_horse\"",
            )],
            "battle b01 treasure [18, 1]",
            "is outside the map",
        ),
        error(
            &[(B01, "item = \"red_horse\"", "item = \"jade_seal\"")],
            "battle b01 treasure [8, 1]",
            "gives unknown item `jade_seal`",
        ),
        error(
            &[(B01, "intro = \"b01_intro\"", "intro = \"b01_opening\"")],
            "battle b01",
            "intro scene `b01_opening` does not exist",
        ),
        error(
            &[(B01, "turn_limit = 20", "turn_limit = 0")],
            "battle b01",
            "turn_limit must be at least 1",
        ),
        warning(
            &[(
                B01,
                "turn = 3, side = \"enemy\"",
                "turn = 30, side = \"enemy\"",
            )],
            "battle b01 event #1",
            "turn 30 is after turn_limit 20",
        ),
        error(
            &[(
                B02,
                "victory = [{ type = \"defeat_all\" }]",
                "victory = [{ type = \"survive_turns\", turns = 0 }]",
            )],
            "battle b02 victory",
            "survive_turns needs at least 1 turn",
        ),
    ]);
}

#[test]
fn defeat_all_needs_enemies_on_the_map() {
    let mut files = fixture_files();
    let b02 = files.get_mut(B02).unwrap();
    *b02 = b02.replace("ai = \"hold\"", "ai = \"hold\"\ngroup = \"late\"");
    edit(
        &mut files,
        B02,
        "tag = \"chief\"",
        "tag = \"chief\"\ngroup = \"late\"",
    );
    edit(
        &mut files,
        B02,
        "{ type = \"give_gold\", amount = 100 }",
        "{ type = \"spawn\", group = \"late\" }",
    );
    let issues = load(&files).validate();
    assert_issue(
        &issues,
        Severity::Error,
        "battle b02 victory",
        "defeat_all, but no enemy unit starts on the map",
    );
}

#[test]
fn drama_checks() {
    run(&[
        error(
            &[(STORY, "@join jian_yong", "@join mi_zhu")],
            "scene oath",
            "@join names unknown officer `mi_zhu`",
        ),
        error(
            &[(STORY, "@item bean", "@item peach")],
            "scene oath",
            "@item names unknown item `peach`",
        ),
        error(
            &[(BATTLE_DRAMAS, "zhang_bao: 덤벼라!", "zhang_liang: 덤벼라!")],
            "scene b01_duel",
            "speaker `zhang_liang` looks like an officer id",
        ),
        warning(
            &[(
                BATTLE_DRAMAS,
                "== b02_intro",
                "== spare\n@narr 쓰이지 않는다.\n\n== b02_intro",
            )],
            "scene spare",
            "is never played",
        ),
    ]);
}

#[test]
fn free_speaker_names_are_allowed() {
    let mut files = fixture_files();
    edit(
        &mut files,
        BATTLE_DRAMAS,
        "장보: 지원군이",
        "Messenger: 지원군이",
    );
    edit(
        &mut files,
        BATTLE_DRAMAS,
        "guan_yu: 네 상대는",
        "전령: 네 상대는",
    );
    let issues = load(&files).validate();
    assert!(issues.is_empty(), "{}", format_issues(&issues));
}

#[test]
fn campaign_checks() {
    run(&[
        error(
            &[(CAMPAIGN, "start = \"prologue\"", "start = \"opening\"")],
            "campaign",
            "start node `opening` does not exist",
        ),
        error(
            &[(CAMPAIGN, "start = \"prologue\"", "start = \"route\"")],
            "campaign",
            "the start node must not be a branch",
        ),
        error(
            &[(CAMPAIGN, "next = \"finale\"", "next = \"final\"")],
            "campaign node battle2",
            "continues to unknown node `final`",
        ),
        warning(
            &[(CAMPAIGN, "next = \"finale\"", "next = \"final\"")],
            "campaign node finale",
            "is unreachable from the start node",
        ),
        warning(
            &[(CAMPAIGN, "next = \"finale\"", "next = \"final\"")],
            "campaign",
            "no ending node is reachable",
        ),
        error(
            &[(CAMPAIGN, "else = \"camp2\"", "else = \"camp3\"")],
            "campaign node route",
            "continues to unknown node `camp3`",
        ),
        warning(
            &[(CAMPAIGN, "on_defeat = \"retreat\"\n", "")],
            "campaign node retreat",
            "is unreachable",
        ),
        error(
            &[(CAMPAIGN, "scene = \"epilogue\"", "scene = \"afterword\"")],
            "campaign node finale",
            "scene `afterword` does not exist",
        ),
        error(
            &[(
                CAMPAIGN,
                "shop = [\"bean\", \"wine\", \"short_bow\", \"war_manual\"]",
                "shop = [\"bean\", \"wine\", \"long_bow\"]",
            )],
            "campaign node camp2",
            "shop item `long_bow` does not exist",
        ),
        warning(
            &[(
                CAMPAIGN,
                "shop = [\"bean\", \"wine\", \"short_bow\", \"war_manual\"]",
                "shop = [\"bean\", \"red_horse\"]",
            )],
            "campaign node camp2",
            "has price 0 and cannot be bought",
        ),
        error(
            &[(
                CAMPAIGN,
                "battle = \"b02\"\nnext = \"finale\"",
                "battle = \"b03\"\nnext = \"finale\"",
            )],
            "campaign node battle2",
            "battle `b03` does not exist",
        ),
        warning(
            &[(
                CAMPAIGN,
                "battle = \"b02\"\nnext = \"finale\"",
                "battle = \"b01\"\nnext = \"finale\"",
            )],
            "battle b02",
            "is not used by any campaign battle node",
        ),
        error(
            &[(
                CAMPAIGN,
                "battle = \"b02\"\nnext = \"battle2\"",
                "battle = \"b09\"\nnext = \"battle2\"",
            )],
            "campaign node camp2",
            "battle `b09` does not exist",
        ),
        error(
            &[(
                CAMPAIGN,
                "\"liu_bei\", \"guan_yu\", \"zhang_fei\"",
                "\"liu_bei\", \"guan_yu\", \"zhang_he\"",
            )],
            "campaign",
            "starting officer `zhang_he` does not exist",
        ),
        error(
            &[(OFFICERS, "lord = true\n", "")],
            "campaign",
            "no starting officer is a lord",
        ),
        error(
            &[(
                CAMPAIGN,
                "starting_items = { bean = 3 }",
                "starting_items = { bean = 3, peach = 1 }",
            )],
            "campaign",
            "starting item `peach` does not exist",
        ),
        warning(
            &[(CAMPAIGN, "flag = \"captives\"", "flag = \"prisoners\"")],
            "campaign node mercy_check",
            "flag `prisoners` is tested but never set",
        ),
        warning(
            &[(
                CAMPAIGN,
                "value = 2\nthen = \"mercy\"\nelse = \"camp2\"",
                "value = 2\nthen = \"mercy\"\nelse = \"route\"",
            )],
            "campaign",
            "branch nodes route, mercy_check can lead back to themselves",
        ),
    ]);
}
