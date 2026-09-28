//! Layered packs (`extends`): the `mini_ext` fixture on top of `mini`, the two-phase chain API,
//! override and union rules, `[presentation]`, and broken chains (cycles, missing parents,
//! chains that are too deep).

mod common;

use common::*;
use hero_core::pack::{
    DirSource, Pack, PackChain, PackError, PackFile, Presentation, Severity, MAX_CHAIN_DEPTH,
};
use hero_core::script::Cmd;

fn parse_error(e: PackError) -> (String, String) {
    match e {
        PackError::Parse { file, msg } => (file, msg),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

fn file(dir: &str, path: &str) -> PackFile {
    PackFile {
        dir: dir.into(),
        path: path.into(),
    }
}

/// A minimal `pack.toml` for chain tests: `id`, then `extra` lines (`extends = ...` etc.).
fn manifest(id: &str, extra: &str) -> String {
    format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\n{extra}\n")
}

#[test]
fn loads_the_layered_fixture() {
    let pack = Pack::load(&DirSource {
        root: fixture_path("mini_ext"),
    })
    .expect("mini_ext loads from disk");

    // Identity comes from the top pack; the chain is recorded top first.
    assert_eq!(pack.manifest.id, "mini_ext");
    let dirs: Vec<&str> = pack.layers.iter().map(|l| l.dir.as_str()).collect();
    assert_eq!(dirs, ["", "../mini"]);
    assert_eq!(pack.layers[1].manifest.id, "mini");
    assert_eq!(pack.manifest.presentation.canvas, [640, 480]);

    // rules/game.toml is the child's; every other rules file, the officers come from `mini`.
    assert_eq!((pack.rules.gold_cap, pack.rules.exp_per_level), (5000, 120));
    assert_eq!(pack.files.game, file("", "rules/game.toml"));
    assert_eq!(pack.files.terrain, file("../mini", "rules/terrain.toml"));
    assert_eq!(
        pack.files.terrain.source_path(),
        "../mini/rules/terrain.toml"
    );
    assert_eq!(pack.files.officers, file("../mini", "officers.toml"));
    assert_eq!(pack.files.campaign, file("", "campaign.toml"));
    assert_eq!(pack.terrain.len(), 8);
    assert_eq!(pack.classes.len(), 6);
    assert_eq!(pack.items.len(), 9);
    assert_eq!(pack.officers.len(), 6);

    // The campaign is the child's; battles and scenes are the union of both packs.
    assert_eq!(pack.campaign.title, "Mini campaign, extended");
    assert_eq!(pack.campaign.nodes.len(), 12);
    let battles: Vec<&str> = pack.battles.keys().map(String::as_str).collect();
    assert_eq!(battles, ["b01", "b02", "b03"]);
    assert_eq!(pack.scenes.len(), 10, "mini's 9 scenes and b03_intro");
    assert!(pack.scene("oath").is_some(), "parent scenes are kept");
    assert!(pack.scene("b03_intro").is_some());
    // b01_outro is replaced by the child's scene of the same id.
    assert_eq!(
        pack.scene("b01_outro").unwrap().cmds,
        [Cmd::Narr("적이 산으로 달아났다.".into()), Cmd::End]
    );
    // Battle files are listed farthest parent first.
    assert_eq!(
        pack.files.battles,
        [
            file("../mini", "battles/b01.toml"),
            file("../mini", "battles/b02.toml"),
            file("", "battles/b03.toml"),
        ]
    );
}

#[test]
fn the_layered_fixture_is_clean() {
    let pack = Pack::load(&layered_files()).expect("mini_ext loads from memory");
    let issues = pack.validate();
    assert!(issues.is_empty(), "{}", format_issues(&issues));
    let unknown = Pack::unknown_fields(&layered_files()).expect("chain parses");
    assert!(unknown.is_empty(), "{}", format_issues(&unknown));
}

#[test]
fn memory_and_directory_sources_agree() {
    let from_dir = Pack::load(&DirSource {
        root: fixture_path("mini_ext"),
    })
    .unwrap();
    let from_map = Pack::load(&layered_files()).unwrap();
    assert_eq!(from_dir.manifest, from_map.manifest);
    assert_eq!(from_dir.layers, from_map.layers);
    assert_eq!(from_dir.files, from_map.files);
    assert_eq!(from_dir.battles, from_map.battles);
    assert_eq!(from_dir.scenes, from_map.scenes);
    assert_eq!(from_dir.campaign, from_map.campaign);
}

#[test]
fn two_phase_loading_lists_every_file_of_the_chain() {
    let files = layered_files();
    let mut chain = PackChain::new(&files["pack.toml"]).unwrap();
    assert!(!chain.is_complete());
    assert!(
        chain.text_files().is_err(),
        "files are known only once the chain is complete"
    );
    assert_eq!(chain.next_parent().as_deref(), Some("../mini/pack.toml"));
    chain.push_parent(&files["../mini/pack.toml"]).unwrap();
    assert!(chain.is_complete());
    assert_eq!(chain.next_parent(), None);
    assert!(
        chain.push_parent(&files["../mini/pack.toml"]).is_err(),
        "no parent is pending"
    );
    // A rejected parent leaves the chain waiting for it.
    let mut pending = PackChain::new(&files["pack.toml"]).unwrap();
    assert!(pending.push_parent("id = ").is_err());
    assert!(pending
        .push_parent(&manifest("mini_ext", "extends = \"../x\""))
        .is_err());
    assert_eq!(pending.layers().len(), 1);
    assert_eq!(pending.next_parent().as_deref(), Some("../mini/pack.toml"));
    pending.push_parent(&files["../mini/pack.toml"]).unwrap();
    assert!(pending.is_complete());

    let listed = chain.text_files().unwrap();
    // The child's own files and the parent's files it does not replace.
    for expected in [
        file("", "rules/game.toml"),
        file("../mini", "rules/terrain.toml"),
        file("../mini", "rules/classes.toml"),
        file("../mini", "rules/strategies.toml"),
        file("../mini", "rules/items.toml"),
        file("../mini", "officers.toml"),
        file("", "campaign.toml"),
        file("../mini", "battles/b01.toml"),
        file("", "battles/b03.toml"),
        file("../mini", "dramas/story.drama"),
        file("", "dramas/ext.drama"),
    ] {
        assert!(listed.contains(&expected), "{expected:?} in {listed:?}");
    }
    // Replaced files are not fetched at all.
    assert!(!listed.contains(&file("../mini", "rules/game.toml")));
    assert!(!listed.contains(&file("../mini", "campaign.toml")));
    assert_eq!(listed.len(), 13, "7 single files, 3 battles, 3 dramas");

    // Exactly these files (plus the manifests) are enough for `Pack::load`.
    let mut fetched = Files::new();
    for layer in chain.layers() {
        let path = layer.manifest_path();
        fetched.insert(path.clone(), files[&path].clone());
    }
    for f in &listed {
        let path = f.source_path();
        fetched.insert(path.clone(), files[&path].clone());
    }
    let pack = Pack::load(&fetched).expect("the listed files are enough");
    assert_eq!(pack.battles.len(), 3);
}

#[test]
fn a_child_battle_overrides_the_parent_battle_with_the_same_id() {
    let mut files = layered_files();
    let b01 =
        files["../mini/battles/b01.toml"].replace("name = \"들판 전투\"", "name = \"새 전투\"");
    files.insert("battles/b01.toml".into(), b01);
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b03.toml\"]",
        "battles = [\"battles/b03.toml\", \"battles/b01.toml\"]",
    );
    let pack = load(&files);
    assert_eq!(pack.battles.len(), 3);
    assert_eq!(pack.battles["b01"].name, "새 전투");

    // Within one pack an id must still be unique.
    let mut files = layered_files();
    let b03 = files["battles/b03.toml"].clone();
    files.insert("battles/b03_copy.toml".into(), b03);
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b03.toml\"]",
        "battles = [\"battles/b03.toml\", \"battles/b03_copy.toml\"]",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "battles/b03_copy.toml");
    assert!(
        msg.contains("duplicate battle id `b03` (first defined in battles/b03.toml)"),
        "{msg}"
    );

    // So must a scene id.
    let mut files = layered_files();
    append(
        &mut files,
        "dramas/ext.drama",
        "\n== b03_intro\n@narr 중복.\n",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(msg.contains("duplicate scene id `b03_intro`"), "{msg}");
}

/// Scenes the top pack no longer plays are reported only when they are its own: a parent's
/// scene left over by a battle the child replaced is the parent's business.
#[test]
fn only_the_top_packs_unused_scenes_are_reported() {
    let pack = load(&layered_files());
    assert!(pack.parent_scenes.contains("oath"));
    assert!(!pack.parent_scenes.contains("b03_intro"), "the child's own");
    assert!(
        !pack.parent_scenes.contains("b01_outro"),
        "the child replaced it"
    );

    let mut files = layered_files();
    let b01 =
        files["../mini/battles/b01.toml"].replace("scene = \"b01_duel\"", "scene = \"b01_rein\"");
    files.insert("battles/b01.toml".into(), b01);
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b03.toml\"]",
        "battles = [\"battles/b03.toml\", \"battles/b01.toml\"]",
    );
    append(
        &mut files,
        "dramas/ext.drama",
        "\n== spare\n@narr 쓰이지 않는 장면.\n",
    );
    let issues = load(&files).validate();
    assert!(
        !issues.iter().any(|i| i.context == "scene b01_duel"),
        "{}",
        format_issues(&issues)
    );
    assert_issue(&issues, Severity::Warning, "scene spare", "is never played");
}

#[test]
fn a_replaced_parent_battle_never_meets_the_child_terrain() {
    // The child's terrain drops the village glyph `v`, which mini's b01 uses; replacing b01 as
    // well keeps the chain loadable, keeping mini's b01 does not.
    let mut files = layered_files();
    let terrain = files["../mini/rules/terrain.toml"].replace(
        "[[terrain]]\nid = \"village\"\nname = \"마을\"\nglyph = \"v\"",
        "[[terrain]]\nid = \"village\"\nname = \"마을\"\nglyph = \"V\"",
    );
    assert_ne!(terrain, files["../mini/rules/terrain.toml"]);
    files.insert("rules/terrain.toml".into(), terrain);
    edit(
        &mut files,
        "pack.toml",
        "game = \"rules/game.toml\"",
        "game = \"rules/game.toml\"\nterrain = \"rules/terrain.toml\"",
    );
    let (err_file, msg) = parse_error(load_err(&files));
    assert_eq!(
        err_file, "../mini/battles/b01.toml",
        "errors name the parent's file"
    );
    assert!(msg.contains("unknown map glyph 'v'"), "{msg}");

    edit(&mut files, "battles/b03.toml", "..v.....", "..V.....");
    let b01 = files["../mini/battles/b01.toml"].replace("^^..TT..v.", "^^..TT..V.");
    files.insert("battles/b01.toml".into(), b01);
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b03.toml\"]",
        "battles = [\"battles/b03.toml\", \"battles/b01.toml\"]",
    );
    let pack = load(&files);
    assert_eq!(pack.files.terrain, file("", "rules/terrain.toml"));
}

#[test]
fn presentation_is_inherited_from_the_nearest_declaring_pack() {
    // A grandchild of `mini` through `mini_ext`, which declares 640x480.
    let mut files = Files::new();
    for (key, text) in fixture_files_at("mini_ext", "../mini_ext") {
        files.insert(key, text);
    }
    files.extend(fixture_files_at("mini", "../mini"));
    files.insert(
        "pack.toml".into(),
        manifest("balance", "extends = \"../mini_ext\""),
    );
    let pack = load(&files);
    let dirs: Vec<&str> = pack.layers.iter().map(|l| l.dir.as_str()).collect();
    assert_eq!(dirs, ["", "../mini_ext", "../mini"]);
    assert_eq!(pack.manifest.presentation.canvas, [640, 480], "inherited");
    assert_eq!(
        pack.layers[0].manifest.presentation,
        Presentation::default(),
        "the layer keeps what it declares"
    );
    assert_eq!(pack.rules.gold_cap, 5000, "the middle pack's rules");

    files.insert(
        "pack.toml".into(),
        manifest(
            "balance",
            "extends = \"../mini_ext\"\n[presentation]\ncanvas = [800, 600]",
        ),
    );
    assert_eq!(load(&files).manifest.presentation.canvas, [800, 600]);

    // Fields are inherited one by one: an empty `[presentation]` in the child keeps the
    // parent's 640x480.
    files.insert(
        "pack.toml".into(),
        manifest("balance", "extends = \"../mini_ext\"\n[presentation]"),
    );
    assert_eq!(
        load(&files).manifest.presentation.canvas,
        [640, 480],
        "field-level inheritance"
    );

    // Without any declaration: the default.
    assert_eq!(load_fixture().manifest.presentation.canvas, [480, 270]);
    assert_eq!(Presentation::default().canvas, [480, 270]);
}

#[test]
fn the_canvas_must_be_within_limits() {
    for (canvas, ok) in [
        ("[320, 200]", true),
        ("[1280, 800]", true),
        ("[640, 480]", true),
        ("[319, 240]", false),
        ("[640, 199]", false),
        ("[1281, 720]", false),
        ("[1280, 801]", false),
    ] {
        let mut files = fixture_files();
        append(
            &mut files,
            "pack.toml",
            &format!("\n[presentation]\ncanvas = {canvas}\n"),
        );
        if ok {
            load(&files);
        } else {
            let (file, msg) = parse_error(load_err(&files));
            assert_eq!(file, "pack.toml");
            assert!(
                msg.contains("must be between [320, 200] and [1280, 800]"),
                "{canvas}: {msg}"
            );
        }
    }

    // A bad canvas in a parent names the parent's manifest.
    let mut files = layered_files();
    append(
        &mut files,
        "../mini/pack.toml",
        "\n[presentation]\ncanvas = [100, 100]\n",
    );
    let (file, _) = parse_error(load_err(&files));
    assert_eq!(file, "../mini/pack.toml");
}

#[test]
fn a_missing_parent_is_an_error() {
    let mut files = layered_files();
    files.retain(|k, _| !k.starts_with("../mini/"));
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "pack.toml");
    assert_eq!(
        msg,
        "extends `../mini`, but ../mini/pack.toml cannot be read: file not found"
    );

    // The same through a directory: the parent directory is named relative to the child.
    let mut files = layered_files();
    edit(
        &mut files,
        "pack.toml",
        "extends = \"../mini\"",
        "extends = \"../nowhere\"",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(msg.contains("../nowhere/pack.toml cannot be read"), "{msg}");
}

#[test]
fn a_missing_parent_file_is_named_with_its_chain_path() {
    let mut files = layered_files();
    files.remove("../mini/rules/classes.toml");
    assert_eq!(
        load_err(&files),
        PackError::Missing {
            file: "../mini/rules/classes.toml".into()
        }
    );
    let mut files = layered_files();
    edit(
        &mut files,
        "../mini/rules/items.toml",
        "price = 20",
        "price = \"x\"",
    );
    let (file, _) = parse_error(load_err(&files));
    assert_eq!(file, "../mini/rules/items.toml");
}

#[test]
fn cycles_are_errors() {
    // A pack that extends its own directory.
    let mut files = fixture_files();
    edit(
        &mut files,
        "pack.toml",
        "id = \"mini\"",
        "id = \"mini\"\nextends = \".\"",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "pack.toml");
    assert!(msg.contains("already part of this chain"), "{msg}");

    // a -> b -> a, seen through the directory paths.
    let mut files = Files::new();
    files.insert("pack.toml".into(), manifest("top", "extends = \"../a\""));
    files.insert("../a/pack.toml".into(), manifest("a", "extends = \"../b\""));
    files.insert("../b/pack.toml".into(), manifest("b", "extends = \"../a\""));
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "../b/pack.toml");
    assert!(
        msg.contains("extends `../a`, which is already part of this chain"),
        "{msg}"
    );

    // top -> a -> top again under another path: caught by the repeated pack id.
    let top = manifest("top", "extends = \"../a\"");
    let mut files = Files::new();
    files.insert("pack.toml".into(), top.clone());
    files.insert(
        "../a/pack.toml".into(),
        manifest("a", "extends = \"../top\""),
    );
    files.insert("../top/pack.toml".into(), top);
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "../top/pack.toml");
    assert!(
        msg.contains("pack id `top` is also the id of pack.toml"),
        "{msg}"
    );

    // Two different packs with one id cannot share a chain either.
    let mut files = layered_files();
    edit(
        &mut files,
        "pack.toml",
        "id = \"mini_ext\"",
        "id = \"mini\"",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "../mini/pack.toml");
    assert!(msg.contains("need distinct ids"), "{msg}");
}

#[test]
fn chains_deeper_than_the_limit_are_errors() {
    assert_eq!(MAX_CHAIN_DEPTH, 4);
    // p0 -> p1 -> p2 -> mini: four packs load.
    let mut files = fixture_files_at("mini", "../mini");
    files.insert("pack.toml".into(), manifest("p0", "extends = \"../p1\""));
    files.insert(
        "../p1/pack.toml".into(),
        manifest("p1", "extends = \"../p2\""),
    );
    files.insert(
        "../p2/pack.toml".into(),
        manifest("p2", "extends = \"../mini\""),
    );
    let pack = load(&files);
    assert_eq!(pack.layers.len(), 4);
    assert_eq!(pack.layers[3].dir, "../mini");

    // p0 -> p1 -> p2 -> p3 -> mini: five packs do not.
    files.insert(
        "../p2/pack.toml".into(),
        manifest("p2", "extends = \"../p3\""),
    );
    files.insert(
        "../p3/pack.toml".into(),
        manifest("p3", "extends = \"../mini\""),
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "../p3/pack.toml");
    assert!(msg.contains("a chain holds at most 4 packs"), "{msg}");
}

#[test]
fn parents_resolve_relative_to_the_pack_that_names_them() {
    // p0 lives in `mods/p0`; its parent `../../base` is two levels up, and that parent's own
    // parent `../core` is next to it.
    let mut files = fixture_files_at("mini", "../../core");
    files.insert(
        "pack.toml".into(),
        manifest("p0", "extends = \"../../base\""),
    );
    files.insert(
        "../../base/pack.toml".into(),
        manifest("base", "extends = \"./../core/\""),
    );
    let pack = load(&files);
    let dirs: Vec<&str> = pack.layers.iter().map(|l| l.dir.as_str()).collect();
    assert_eq!(dirs, ["", "../../base", "../../core"]);
    assert_eq!(pack.files.game.source_path(), "../../core/rules/game.toml");
}

#[test]
fn extends_must_be_a_relative_directory() {
    for bad in ["", "/opt/base", "C:/base", "..\\\\base"] {
        let mut files = layered_files();
        edit(
            &mut files,
            "pack.toml",
            "extends = \"../mini\"",
            &format!("extends = \"{bad}\""),
        );
        let (file, msg) = parse_error(load_err(&files));
        assert_eq!(file, "pack.toml");
        assert!(
            msg.contains("must be a directory relative to this pack"),
            "{bad}: {msg}"
        );
    }
}

#[test]
fn every_role_needs_a_file_somewhere_in_the_chain() {
    // A pack without `extends` must list everything.
    let mut files = fixture_files();
    edit(
        &mut files,
        "pack.toml",
        "officers = \"officers.toml\"\n",
        "",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "pack.toml");
    assert!(
        msg.contains("`officers` is missing: a pack without `extends` must list"),
        "{msg}"
    );

    // In a chain, some pack must list it.
    let mut files = layered_files();
    edit(
        &mut files,
        "../mini/pack.toml",
        "items = \"rules/items.toml\"\n",
        "",
    );
    let (_, msg) = parse_error(load_err(&files));
    assert!(
        msg.contains("`rules.items` is listed neither here nor in any pack this one extends"),
        "{msg}"
    );

    // Battles and dramas may be left out everywhere.
    let mut files = fixture_files();
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b01.toml\", \"battles/b02.toml\"]\n",
        "",
    );
    edit(
        &mut files,
        "pack.toml",
        "dramas = [\"dramas/story.drama\", \"dramas/battles.drama\"]\n",
        "",
    );
    let pack = load(&files);
    assert!(pack.battles.is_empty() && pack.scenes.is_empty());
}

#[test]
fn child_paths_follow_the_usual_rules() {
    let mut files = layered_files();
    edit(
        &mut files,
        "pack.toml",
        "battles = [\"battles/b03.toml\"]",
        "battles = [\"../mini/battles/b01.toml\"]",
    );
    let (file, msg) = parse_error(load_err(&files));
    assert_eq!(file, "pack.toml");
    assert!(msg.contains("must be relative"), "{msg}");
}

#[test]
fn unknown_fields_are_reported_across_the_chain() {
    let mut files = layered_files();
    edit(
        &mut files,
        "../mini/pack.toml",
        "id = \"mini\"",
        "id = \"mini\"\nhomepage = \"x\"",
    );
    edit(
        &mut files,
        "rules/game.toml",
        "gold_cap = 5000",
        "gold_cap = 5000\ngold_kap = 1",
    );
    edit(
        &mut files,
        "../mini/rules/classes.toml",
        "hp_growth = 50",
        "hp_growth = 50\nhp_grwth = 5",
    );
    // The parent's game.toml is replaced, so a typo there does not matter.
    edit(
        &mut files,
        "../mini/rules/game.toml",
        "mp_cap = 200",
        "mp_cap = 200\nmp_kap = 1",
    );
    let issues = Pack::unknown_fields(&files).unwrap();
    assert_issue(
        &issues,
        Severity::Warning,
        "../mini/pack.toml",
        "`homepage`",
    );
    assert_issue(&issues, Severity::Warning, "rules/game.toml", "`gold_kap`");
    assert_issue(
        &issues,
        Severity::Warning,
        "../mini/rules/classes.toml",
        "hp_grwth",
    );
    assert_eq!(issues.len(), 3, "{}", format_issues(&issues));
}

#[test]
fn validation_contexts_name_the_file_in_use() {
    let mut files = layered_files();
    edit(
        &mut files,
        "rules/game.toml",
        "level_cap = 50",
        "level_cap = 0",
    );
    let issues = load(&files).validate();
    assert_issue(
        &issues,
        Severity::Error,
        "rules/game.toml",
        "level_cap must be at least 1",
    );
    assert!(
        issues
            .iter()
            .all(|i| !i.context.starts_with("../mini/rules/game")),
        "{}",
        format_issues(&issues)
    );
}
