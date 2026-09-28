//! `Pack::missing_media` against a temporary media tree for the fixture pack.

mod common;

use common::*;
use hero_core::pack::{Issue, Severity};
use std::path::{Path, PathBuf};

/// A scratch directory removed on drop (also when the test fails).
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("hero-core-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(root: &Path, rel: &str, content: impl AsRef<[u8]>) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// The first 24 bytes of a PNG of `w`×`h` pixels (all the size checks read).
fn png_head(w: u32, h: u32) -> Vec<u8> {
    let mut v = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
    ];
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v
}

/// Every media file the fixture pack refers to (the check looks at file names and the size of
/// unit sheets, so empty files stand in for other images and audio).
fn complete_media(root: &Path) {
    // The base pack's real fonts (Galmuri's own Hanja plus the ones the base pack needs): the
    // Hanja coverage check reads them.
    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base/fonts");
    for f in ["Galmuri11.ttf", "Galmuri9.ttf"] {
        write(
            root,
            &format!("fonts/{f}"),
            std::fs::read(fonts.join(f)).unwrap(),
        );
    }
    let sprites = [
        "short_infantry",
        "long_infantry",
        "light_cavalry",
        "archer",
        "bandit",
        "sorcerer",
    ];
    let mut units_toml = String::new();
    for key in sprites {
        for side in ["player", "ally", "enemy"] {
            // 4 × 6 frames of 16×16.
            write(
                root,
                &format!("gfx/units/{key}_{side}.png"),
                png_head(64, 96),
            );
        }
        units_toml.push_str(&format!(
            "[sprites.{key}]\nframe = [16, 16]\nanchor = [8, 15]\n"
        ));
    }
    write(root, "gfx/units/units.toml", &units_toml);
    for key in [
        "_unknown",
        "liu_bei",
        "guan_yu",
        "zhang_fei",
        "jianyong",
        "zhang_bao",
        "deng_mao",
    ] {
        write(root, &format!("gfx/portraits/{key}.png"), "");
    }
    for key in ["battle", "enemy", "peace", "ending", "tension"] {
        write(root, &format!("bgm/{key}.ogg"), "");
    }
    for key in ["village", "camp", "field", "palace", "castle"] {
        write(root, &format!("gfx/bg/{key}.png"), "");
    }
    write(root, "sfx/confirm.wav", "");
    let mut tiles = String::from("tile_size = 16\nimage = \"terrain.png\"\n");
    for key in [
        "plain", "forest", "mountain", "river", "bridge", "village", "castle", "wall",
    ] {
        tiles.push_str(&format!(
            "[tiles.{key}]\nlayers = [{{ cells = [[0, 0]] }}]\n"
        ));
    }
    write(root, "gfx/tiles/terrain.toml", &tiles);
    write(root, "gfx/tiles/terrain.png", "");
    let mut fx = String::new();
    for key in ["fire", "rock", "heal", "confuse"] {
        fx.push_str(&format!(
            "[fx.{key}]\nframe = [32, 32]\nframes = 8\nfps = 12\n"
        ));
        write(root, &format!("gfx/fx/{key}.png"), "");
    }
    write(root, "gfx/fx/fx.toml", &fx);
    let icons = [
        "bean",
        "wine",
        "scroll",
        "class_up",
        "class_change",
        "weapon",
        "armor",
        "accessory",
    ]
    .iter()
    .enumerate()
    .map(|(i, key)| format!("{key} = [{i}, 0]\n"))
    .collect::<String>();
    write(root, "gfx/ui/icons.toml", format!("[icons]\n{icons}"));
}

#[test]
fn complete_media_has_no_issues() {
    let dir = TempDir::new("media-complete");
    complete_media(&dir.0);
    let issues = load_fixture().missing_media(&dir.0);
    assert!(issues.is_empty(), "{}", format_issues(&issues));
}

#[test]
fn missing_media_is_reported() {
    let dir = TempDir::new("media-missing");
    let root = &dir.0;
    complete_media(root);
    let remove = |rel: &str| std::fs::remove_file(root.join(rel)).unwrap();
    remove("gfx/units/bandit_ally.png");
    remove("gfx/portraits/jianyong.png");
    remove("gfx/portraits/_unknown.png");
    remove("bgm/tension.ogg");
    remove("gfx/bg/palace.png");
    remove("sfx/confirm.wav");
    remove("gfx/fx/rock.png");
    write(
        root,
        "gfx/units/units.toml",
        "[sprites.archer]\nframe = [16, 16]\nanchor = [8, 15]\n",
    );
    write(
        root,
        "gfx/tiles/terrain.toml",
        "image = \"terrain.png\"\n[tiles.plain]\nlayers = []\n",
    );
    write(
        root,
        "gfx/fx/fx.toml",
        "[fx.fire]\nframe = [32, 32]\nframes = 8\nfps = 12\n",
    );
    write(root, "gfx/ui/icons.toml", "[icons]\nbean = [0, 0]\n");

    let issues = load_fixture().missing_media(root);
    let expect = |severity, context: &str, msg: &str| assert_issue(&issues, severity, context, msg);
    expect(
        Severity::Error,
        "class bandit",
        "missing gfx/units/bandit_ally.png",
    );
    expect(
        Severity::Error,
        "class bandit",
        "gfx/units/units.toml has no [sprites.bandit] entry",
    );
    expect(
        Severity::Warning,
        "officer jian_yong",
        "missing gfx/portraits/jianyong.png",
    );
    expect(
        Severity::Error,
        "portraits",
        "missing gfx/portraits/_unknown.png",
    );
    expect(
        Severity::Error,
        "scene b01_intro",
        "missing bgm/tension.ogg",
    );
    expect(
        Severity::Error,
        "scene epilogue",
        "missing gfx/bg/palace.png",
    );
    expect(
        Severity::Error,
        "scene b01_rein",
        "missing sfx/confirm.ogg or sfx/confirm.wav",
    );
    expect(
        Severity::Error,
        "terrain forest",
        "has no [tiles.forest] entry",
    );
    expect(
        Severity::Error,
        "strategy rockfall",
        "gfx/fx/fx.toml has no [fx.rock] entry",
    );
    expect(
        Severity::Error,
        "strategy rockfall",
        "missing gfx/fx/rock.png",
    );
    expect(
        Severity::Warning,
        "item wine",
        "gfx/ui/icons.toml has no icon `wine`",
    );
    // Each missing file is reported once, even when several things use it.
    let weapon_icon = issues
        .iter()
        .filter(|i| i.msg.contains("icon `weapon`"))
        .count();
    assert_eq!(
        weapon_icon,
        2,
        "one per item using it:\n{}",
        format_issues(&issues)
    );
    let bgm = issues
        .iter()
        .filter(|i| i.msg.contains("bgm/tension.ogg"))
        .count();
    assert_eq!(bgm, 1);
}

#[test]
fn layered_packs_find_media_in_any_layer_top_first() {
    // `top` extends `../mini` (the `mini_ext` fixture's text files), with the media split
    // between the two directories.
    let dir = TempDir::new("media-layers");
    let (top, parent) = (dir.0.join("top"), dir.0.join("mini"));
    for (key, text) in fixture_files_at("mini_ext", "") {
        write(&top, &key, &text);
    }
    for (key, text) in fixture_files() {
        write(&parent, &key, &text);
    }
    let load = || {
        hero_core::pack::Pack::load(&hero_core::pack::DirSource { root: top.clone() })
            .expect("layered pack loads")
    };
    complete_media(&parent);
    // A file only the child has counts too.
    std::fs::remove_file(parent.join("gfx/bg/field.png")).unwrap();
    write(&top, "gfx/bg/field.png", "");
    let issues = load().missing_media(&top);
    // The fixture's text is scanned here (it lies in the pack directories), and its 鄧 (鄧茂)
    // is not in the base pack's fonts: the Hanja check is right, and tested on its own.
    let hanja = |i: &Issue| i.severity == Severity::Warning && i.msg.contains("鄧");
    let issues: Vec<Issue> = issues.into_iter().filter(|i| !hanja(i)).collect();
    assert!(issues.is_empty(), "{}", format_issues(&issues));

    // A media index file of the child replaces the parent's as a whole.
    write(
        &top,
        "gfx/units/units.toml",
        "[sprites.archer]\nframe = [16, 16]\nanchor = [8, 15]\n",
    );
    std::fs::remove_file(parent.join("bgm/battle.ogg")).unwrap();
    let issues = load().missing_media(&top);
    assert_issue(
        &issues,
        Severity::Error,
        "class bandit",
        "gfx/units/units.toml has no [sprites.bandit] entry",
    );
    assert_issue(
        &issues,
        Severity::Error,
        "battle b01",
        "missing bgm/battle.ogg",
    );
}

#[test]
fn missing_index_files_are_errors() {
    let dir = TempDir::new("media-empty");
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(
        &issues,
        Severity::Error,
        "unit sprites",
        "missing gfx/units/units.toml",
    );
    assert_issue(
        &issues,
        Severity::Error,
        "terrain tiles",
        "missing gfx/tiles/terrain.toml",
    );
    assert_issue(
        &issues,
        Severity::Error,
        "strategy effects",
        "missing gfx/fx/fx.toml",
    );
    assert_issue(
        &issues,
        Severity::Warning,
        "item icons",
        "missing gfx/ui/icons.toml",
    );

    write(&dir.0, "gfx/tiles/terrain.toml", "image = ");
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(&issues, Severity::Error, "gfx/tiles/terrain.toml", "");
}

#[test]
fn unit_sheets_match_their_frame_size() {
    let dir = TempDir::new("media-sheets");
    complete_media(&dir.0);
    // A 24×24 sheet layout where units.toml says 16×16: the battle and the camp would cut it
    // differently.
    write(&dir.0, "gfx/units/archer_enemy.png", png_head(96, 144));
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(
        &issues,
        Severity::Error,
        "class archer",
        "gfx/units/archer_enemy.png is 96×144 pixels; with frame = [16, 16] in gfx/units/units.toml a sheet of 4 × 6 frames is 64×96",
    );
    assert_eq!(issues.len(), 1, "{}", format_issues(&issues));

    // Not a PNG: an error, not a panic; a frame so big 4 × 6 of it overflows 32 bits: a plain
    // mismatch, not a panic or a wrapped-around match.
    write(&dir.0, "gfx/units/archer_enemy.png", "");
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(
        &issues,
        Severity::Error,
        "class archer",
        "not a readable PNG",
    );
    write(&dir.0, "gfx/units/archer_enemy.png", png_head(4, 96));
    let units = std::fs::read_to_string(dir.0.join("gfx/units/units.toml")).unwrap();
    write(
        &dir.0,
        "gfx/units/units.toml",
        units.replacen(
            "[sprites.archer]\nframe = [16, 16]",
            "[sprites.archer]\nframe = [1073741825, 16]",
            1,
        ),
    );
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(
        &issues,
        Severity::Error,
        "class archer",
        "a sheet of 4 × 6 frames is 4294967300×96",
    );
}

#[test]
fn hanja_the_fonts_lack_are_reported() {
    let dir = TempDir::new("media-hanja");
    complete_media(&dir.0);
    let issues = load_fixture().missing_media(&dir.0);
    assert!(issues.is_empty(), "{}", format_issues(&issues));

    // A drama line with Hanja the fonts do not have (鼂 and 龘), and one they have (劉).
    write(&dir.0, "dramas/extra.drama", "== x\n@say 유비: 劉 鼂 龘\n");
    let issues = load_fixture().missing_media(&dir.0);
    for font in ["fonts/Galmuri11.ttf", "fonts/Galmuri9.ttf"] {
        assert_issue(
            &issues,
            Severity::Warning,
            font,
            "lacks 2 Hanja the pack's text uses, drawn as blanks: 鼂龘",
        );
    }
    assert_eq!(issues.len(), 2, "{}", format_issues(&issues));

    // A missing font: every Korean text would be blank.
    std::fs::remove_file(dir.0.join("fonts/Galmuri9.ttf")).unwrap();
    let issues = load_fixture().missing_media(&dir.0);
    assert_issue(
        &issues,
        Severity::Error,
        "fonts",
        "missing fonts/Galmuri9.ttf (font",
    );
}

#[test]
fn index_files_are_read_with_the_games_schema() {
    let dir = TempDir::new("media-schema");
    // Entries the game cannot read: a sprite without its anchor, an effect without its frame
    // size, a tile size that is not a number.
    write(
        &dir.0,
        "gfx/units/units.toml",
        "[sprites.archer]\nframe = [16, 16]\n",
    );
    write(
        &dir.0,
        "gfx/fx/fx.toml",
        "[fx.fire]\nframes = 8\nfps = 12\n",
    );
    write(&dir.0, "gfx/tiles/terrain.toml", "tile_size = \"big\"\n");
    let issues = load_fixture().missing_media(&dir.0);
    let expect = |context: &str, msg: &str| assert_issue(&issues, Severity::Error, context, msg);
    expect("gfx/units/units.toml", "missing field `anchor`");
    expect("gfx/fx/fx.toml", "missing field `frame`");
    expect("gfx/tiles/terrain.toml", "tile_size");

    // A zero tile size and a layer the game leaves out; without `image` the game uses
    // `terrain.png`.
    write(
        &dir.0,
        "gfx/tiles/terrain.toml",
        "tile_size = 0\n[tiles.plain]\nlayers = [{ auto = [[0, 0]] }]\n",
    );
    let issues = load_fixture().missing_media(&dir.0);
    let expect = |msg: &str| assert_issue(&issues, Severity::Error, "gfx/tiles/terrain.toml", msg);
    expect("tile_size 0 must be a positive whole number of pixels");
    expect("tile `plain` layer 0: autotile layers need exactly 16 cells per frame");
    expect("missing gfx/tiles/terrain.png (terrain atlas)");
}
