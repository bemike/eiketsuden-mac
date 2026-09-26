//! `Pack::load`: parsing the fixture pack and rejecting structurally broken packs.

mod common;

use common::*;
use hero_core::campaign::Node;
use hero_core::data::RangeSpec;
use hero_core::geom::Pos;
use hero_core::pack::{DirSource, Pack, PackError};
use hero_core::script::Compare;

fn parse_error(e: PackError) -> (String, String) {
    match e {
        PackError::Parse { file, msg } => (file, msg),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn loads_the_fixture_pack() {
    let pack = load_fixture();
    assert_eq!(pack.manifest.id, "mini");
    assert_eq!(pack.terrain.len(), 8);
    assert_eq!(pack.terrain[0].id, "plain", "terrain keeps file order");
    assert_eq!(pack.classes.len(), 6);
    assert_eq!(pack.strategies.len(), 4);
    assert_eq!(pack.items.len(), 9);
    assert_eq!(pack.officers.len(), 6);
    assert_eq!(pack.battles.len(), 2);
    assert_eq!(pack.scenes.len(), 9);
    assert_eq!(pack.campaign.nodes.len(), 10);

    let sorcerer = pack.class("sorcerer").unwrap();
    assert!(matches!(&sorcerer.range, RangeSpec::Offsets(o) if o.len() == 4));
    assert_eq!(
        pack.class("short_infantry")
            .unwrap()
            .promote
            .as_ref()
            .unwrap()
            .to,
        "long_infantry"
    );
    assert_eq!(
        pack.officer("jian_yong").unwrap().portrait_key(),
        "jianyong"
    );
    assert_eq!(pack.rules.affinity_pct("infantry", "archer"), 75);

    let b02 = &pack.battles["b02"];
    assert_eq!(b02.map.legend["G"], "castle");
    assert_eq!(b02.deploy.slots[3], Pos::new(4, 5));

    // `cmp` defaults to `!=` and accepts operators.
    match pack.campaign.node("route").unwrap() {
        Node::Branch { cmp, value, .. } => assert_eq!((*cmp, *value), (Compare::Ne, 0)),
        other => panic!("route is not a branch: {other:?}"),
    }
    match pack.campaign.node("mercy_check").unwrap() {
        Node::Branch { cmp, value, .. } => assert_eq!((*cmp, *value), (Compare::Ge, 2)),
        other => panic!("mercy_check is not a branch: {other:?}"),
    }

    // Promotion chains and inherited strategies work on the loaded data.
    let chain: Vec<&str> = pack
        .class_chain("long_infantry")
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(chain, ["short_infantry", "long_infantry"]);
    assert_eq!(pack.known_strategies("long_infantry", 20), ["fire", "heal"]);
}

#[test]
fn loads_from_a_directory() {
    let from_dir = Pack::load(&DirSource {
        root: fixture_dir(),
    })
    .expect("fixture loads from disk");
    let from_map = load_fixture();
    assert_eq!(from_dir.manifest, from_map.manifest);
    assert_eq!(from_dir.classes, from_map.classes);
    assert_eq!(from_dir.battles, from_map.battles);
    assert_eq!(from_dir.scenes, from_map.scenes);
    assert_eq!(from_dir.campaign, from_map.campaign);
}

#[test]
fn byte_order_mark_is_ignored() {
    let mut files = fixture_files();
    let text = files.get_mut("rules/classes.toml").unwrap();
    text.insert(0, '\u{feff}');
    let pack = load(&files);
    assert_eq!(pack.classes.len(), 6);
}

#[test]
fn missing_file_is_named() {
    let mut files = fixture_files();
    files.remove("battles/b02.toml");
    assert_eq!(
        load_err(&files),
        PackError::Missing {
            file: "battles/b02.toml".into()
        }
    );
}

#[test]
fn toml_syntax_error_names_the_file() {
    let mut files = fixture_files();
    edit(&mut files, "rules/classes.toml", "tier = 1", "tier = = 1");
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "rules/classes.toml");
    assert!(msg.contains("line"), "{msg}");
}

#[test]
fn schema_error_names_the_file() {
    let mut files = fixture_files();
    edit(&mut files, "officers.toml", "level = 5", "level = \"five\"");
    let (file, _) = parse_error(load_err(&files));
    assert_eq!(file, "officers.toml");
}

#[test]
fn misspelt_table_name_is_rejected() {
    let mut files = fixture_files();
    let text = files.get_mut("rules/items.toml").unwrap();
    *text = text.replace("[[item]]", "[[items]]");
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "rules/items.toml");
    assert!(msg.contains("items"), "{msg}");
}

#[test]
fn duplicate_ids_are_rejected() {
    let cases = [
        (
            "rules/classes.toml",
            "id = \"long_infantry\"",
            "id = \"short_infantry\"",
            "duplicate class id `short_infantry`",
        ),
        (
            "rules/strategies.toml",
            "id = \"rockfall\"",
            "id = \"fire\"",
            "duplicate strategy id `fire`",
        ),
        (
            "rules/items.toml",
            "id = \"wine\"",
            "id = \"bean\"",
            "duplicate item id `bean`",
        ),
        (
            "officers.toml",
            "id = \"deng_mao\"",
            "id = \"zhang_bao\"",
            "duplicate officer id `zhang_bao`",
        ),
        (
            "rules/terrain.toml",
            "id = \"wall\"",
            "id = \"river\"",
            "duplicate terrain id `river`",
        ),
        (
            "rules/terrain.toml",
            "glyph = \"#\"",
            "glyph = \"~\"",
            "reuses glyph '~' of terrain `river`",
        ),
        (
            "campaign.toml",
            "id = \"retreat\"",
            "id = \"mercy\"",
            "duplicate campaign node id `mercy`",
        ),
        (
            "battles/b02.toml",
            "id = \"b02\"",
            "id = \"b01\"",
            "duplicate battle id `b01` (first defined in battles/b01.toml)",
        ),
        (
            "rules/classes.toml",
            "id = \"archer\"",
            "id = \"\"",
            "class with an empty id",
        ),
    ];
    for (file, from, to, expected) in cases {
        let mut files = fixture_files();
        edit(&mut files, file, from, to);
        let (err_file, msg) = parse_error(load_err(&files));
        assert_eq!(err_file, file, "{expected}");
        assert!(msg.contains(expected), "expected {expected:?}, got {msg:?}");
    }
}

#[test]
fn duplicate_scene_across_files_is_rejected() {
    let mut files = fixture_files();
    append(
        &mut files,
        "dramas/battles.drama",
        "\n== mercy\n@narr 중복.\n",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "dramas/battles.drama");
    assert!(
        msg.contains("duplicate scene id `mercy` (first defined in dramas/story.drama)"),
        "{msg}"
    );
}

#[test]
fn drama_syntax_error_has_file_and_line() {
    let mut files = fixture_files();
    edit(
        &mut files,
        "dramas/story.drama",
        "@fade out",
        "@fade sideways",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "dramas/story.drama");
    assert!(msg.starts_with("line 29:"), "{msg}");
}

#[test]
fn unparsable_maps_are_rejected() {
    let cases = [
        (
            "^^^.......\n^^..TT..v.",
            "^^^....Q..\n^^..TT..v.",
            "unknown map glyph 'Q' at (7, 0)",
        ),
        (
            "^^^.......\n^^..TT..v.",
            "^^^......\n^^..TT..v.",
            "row 1 has width 10, expected 9",
        ),
    ];
    for (from, to, expected) in cases {
        let mut files = fixture_files();
        edit(&mut files, "battles/b01.toml", from, to);
        let (file, msg) = parse_error(load_err(&files));
        assert_eq!(file, "battles/b01.toml");
        assert!(
            msg.contains("battle `b01` map:") && msg.contains(expected),
            "{msg}"
        );
    }

    let mut files = fixture_files();
    edit(
        &mut files,
        "battles/b02.toml",
        "G = \"castle\"",
        "G = \"moat\"",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(msg.contains("unknown terrain `moat`"), "{msg}");

    let mut files = fixture_files();
    edit(
        &mut files,
        "battles/b02.toml",
        "G = \"castle\"",
        "GG = \"castle\"",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(
        msg.contains("legend key `GG` must be exactly one character"),
        "{msg}"
    );
}

#[test]
fn manifest_paths_must_stay_inside_the_pack() {
    for bad in [
        "../outside.toml",
        "/abs/b01.toml",
        "battles\\\\b01.toml",
        "C:/b01.toml",
    ] {
        let mut files = fixture_files();
        edit(
            &mut files,
            "pack.toml",
            "\"battles/b02.toml\"]",
            &format!("\"battles/b02.toml\", \"{bad}\"]"),
        );
        let (file, msg) = parse_error(load_err(&files));
        assert_eq!(file, "pack.toml");
        assert!(msg.contains("must be relative"), "{bad}: {msg}");
    }

    let mut files = fixture_files();
    edit(
        &mut files,
        "pack.toml",
        "\"battles/b02.toml\"]",
        "\"battles/b02.toml\", \"battles/b01.toml\"]",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(msg.contains("listed more than once"), "{msg}");
}
