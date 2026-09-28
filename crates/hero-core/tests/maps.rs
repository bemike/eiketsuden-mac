//! Map files (`maps` in `pack.toml`, `[[map]]` tables) and battles that `use` them, and the
//! picture layer (`image`) of a map.

mod common;

use common::*;
use hero_core::pack::{Pack, PackChain, Severity};
use std::path::{Path, PathBuf};

/// The map of the fixture's battle b01 as a map file entry with a picture layer.
const FIELD_MAP: &str = r#"
[[map]]
id = "field"
name = "들판"
theme = "field"
image = "field"
rows = """
^^^.......
^^..TT..v.
^...TT....
..........
~~~=~~~~~~
..........
..........
cc........
"""
"#;

/// The fixture with `maps/field.toml` listed in `pack.toml` and b01 using it.
fn with_map_file() -> Files {
    let mut files = fixture_files();
    edit(
        &mut files,
        "pack.toml",
        "dramas = [",
        "maps = [\"maps/field.toml\"]\ndramas = [",
    );
    files.insert("maps/field.toml".into(), FIELD_MAP.into());
    let b01 = files["battles/b01.toml"].clone();
    let start = b01.find("[map]").unwrap();
    let end = b01.find("[deploy]").unwrap();
    files.insert(
        "battles/b01.toml".into(),
        format!("{}[map]\nuse = \"field\"\n\n{}", &b01[..start], &b01[end..]),
    );
    files
}

#[test]
fn a_battle_takes_its_map_from_the_map_file() {
    let files = with_map_file();
    let pack = load(&files);
    let map = &pack.battles["b01"].map;
    assert_eq!(map.use_map.as_deref(), Some("field"));
    assert_eq!(map.image.as_deref(), Some("field"));
    assert_eq!(map.theme.as_deref(), Some("field"));
    assert!(map.rows.contains("~~~=~~~~~~"), "{}", map.rows);
    assert_eq!(pack.maps["field"].name, "들판");
    // The battle plays exactly as with the map written in place.
    let inline = load_fixture();
    assert_eq!(map.rows, inline.battles["b01"].map.rows);
    let issues = pack.validate();
    assert!(issues.is_empty(), "{}", format_issues(&issues));
    let unknown = Pack::unknown_fields(&files).unwrap();
    assert!(unknown.is_empty(), "{}", format_issues(&unknown));
    // The web build fetches what the chain lists.
    let chain = PackChain::read(&files).unwrap();
    let listed: Vec<String> = chain
        .text_files()
        .unwrap()
        .iter()
        .map(|f| f.source_path())
        .collect();
    assert!(
        listed.contains(&"maps/field.toml".to_string()),
        "{listed:?}"
    );
}

#[test]
fn a_map_nobody_uses_is_loaded_and_checked() {
    let mut files = with_map_file();
    append(
        &mut files,
        "maps/field.toml",
        "\n[[map]]\nid = \"spare\"\nrows = \"..\\n.T\"\n",
    );
    let pack = load(&files);
    assert_eq!(pack.maps["spare"].rows, "..\n.T");
    assert!(pack.validate().is_empty());
    // Its rows are checked at load like a battle's.
    append(
        &mut files,
        "maps/field.toml",
        "\n[[map]]\nid = \"broken\"\nrows = \"..\\n.\"\n",
    );
    let e = load_err(&files).to_string();
    assert!(
        e.contains("maps/field.toml") && e.contains("map `broken`"),
        "{e}"
    );
}

#[test]
fn use_needs_a_known_map_and_nothing_else() {
    let mut files = with_map_file();
    edit(
        &mut files,
        "battles/b01.toml",
        "use = \"field\"",
        "use = \"hill\"",
    );
    let e = load_err(&files).to_string();
    assert!(
        e.contains("battles/b01.toml") && e.contains("unknown map `hill`"),
        "{e}"
    );

    let mut files = with_map_file();
    edit(
        &mut files,
        "battles/b01.toml",
        "use = \"field\"",
        "use = \"field\"\nimage = \"other\"",
    );
    let e = load_err(&files).to_string();
    assert!(e.contains("`use` takes the whole map"), "{e}");

    // Without `use`, a battle still needs rows.
    let mut files = with_map_file();
    edit(&mut files, "battles/b01.toml", "use = \"field\"", "");
    let e = load_err(&files).to_string();
    assert!(e.contains("battle `b01` map: map has no rows"), "{e}");
}

#[test]
fn map_ids_follow_the_layer_rules() {
    // Unique within one pack.
    let mut files = with_map_file();
    append(&mut files, "maps/field.toml", FIELD_MAP);
    let e = load_err(&files).to_string();
    assert!(e.contains("duplicate map id `field`"), "{e}");

    // A child's map replaces the parent's, also for the parent's battles.
    let mut files: Files = with_map_file()
        .into_iter()
        .map(|(k, v)| (format!("../mini/{k}"), v))
        .collect();
    files.extend(fixture_files_at("mini_ext", ""));
    edit(
        &mut files,
        "pack.toml",
        "dramas = [",
        "maps = [\"maps/redrawn.toml\"]\ndramas = [",
    );
    files.insert(
        "maps/redrawn.toml".into(),
        FIELD_MAP.replace("image = \"field\"", "image = \"field_winter\""),
    );
    let pack = load(&files);
    assert_eq!(
        pack.battles["b01"].map.image.as_deref(),
        Some("field_winter")
    );
    assert!(pack.validate().is_empty());
}

#[test]
fn map_files_are_linted_and_validated() {
    let mut files = with_map_file();
    edit(
        &mut files,
        "maps/field.toml",
        "theme =",
        "themes = \"x\"\ntheme =",
    );
    let unknown = Pack::unknown_fields(&files).unwrap();
    assert_issue(
        &unknown,
        Severity::Warning,
        "maps/field.toml",
        "map[field].themes",
    );

    let mut files = with_map_file();
    edit(
        &mut files,
        "maps/field.toml",
        "image = \"field\"",
        "image = \"../field\"\nlegend = { \"L\" = \"lava\" }",
    );
    let issues = load(&files).validate();
    assert_issue(
        &issues,
        Severity::Error,
        "map field",
        "map image `../field`",
    );
    assert_issue(
        &issues,
        Severity::Error,
        "map field",
        "unknown terrain `lava`",
    );
    // Reported once for the map, not again for the battle that uses it.
    assert!(
        !issues.iter().any(|i| i.context == "battle b01"),
        "{}",
        format_issues(&issues)
    );

    // A battle's own picture key is checked too.
    let mut files = fixture_files();
    edit(
        &mut files,
        "battles/b01.toml",
        "theme = \"field\"",
        "theme = \"field\"\nimage = \"C:/x\"",
    );
    let issues = load(&files).validate();
    assert_issue(&issues, Severity::Error, "battle b01", "map image `C:/x`");
}

// ----- picture files ---------------------------------------------------------------------------

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

/// The first 24 bytes of a PNG of `w`×`h` pixels (all the check reads).
fn png_head(w: u32, h: u32) -> Vec<u8> {
    let mut v = vec![
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
    ];
    v.extend_from_slice(b"IHDR");
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v
}

fn write(root: &Path, rel: &str, content: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Issues about map pictures only (the temporary tree has no other media).
fn picture_issues(pack: &Pack, root: &Path) -> Vec<hero_core::pack::Issue> {
    pack.missing_media(root)
        .into_iter()
        .filter(|i| i.msg.contains("gfx/maps/") || i.context.starts_with("map "))
        .collect()
}

#[test]
fn the_picture_must_exist_and_fit_the_map() {
    let pack = load(&with_map_file());
    let dir = TempDir::new("map-picture");
    let root = &dir.0;
    // The map is 10×8 tiles.
    let issues = picture_issues(&pack, root);
    assert_issue(
        &issues,
        Severity::Error,
        "map field",
        "missing gfx/maps/field.png",
    );

    // Without a tileset the tile size is 16.
    write(root, "gfx/maps/field.png", &png_head(160, 128));
    assert!(picture_issues(&pack, root).is_empty());

    // With 32-px tiles the same picture is too small.
    write(
        root,
        "gfx/tiles/terrain.toml",
        b"tile_size = 32\nimage = \"terrain.png\"\n",
    );
    let issues = picture_issues(&pack, root);
    assert_issue(
        &issues,
        Severity::Error,
        "map field",
        "is 160×128 pixels; the map needs 320×256 (10×8 tiles of 32 px",
    );
    write(root, "gfx/maps/field.png", &png_head(320, 256));
    assert!(picture_issues(&pack, root).is_empty());

    // Up to 4096 pixels a side is fine (10×8 tiles of 409 px: 4090×3272); over that, a
    // warning (phones draw it black).
    write(
        root,
        "gfx/tiles/terrain.toml",
        b"tile_size = 409\nimage = \"terrain.png\"\n",
    );
    write(root, "gfx/maps/field.png", &png_head(4090, 3272));
    assert!(picture_issues(&pack, root).is_empty());
    write(
        root,
        "gfx/tiles/terrain.toml",
        b"tile_size = 512\nimage = \"terrain.png\"\n",
    );
    write(root, "gfx/maps/field.png", &png_head(5120, 4096));
    let issues = picture_issues(&pack, root);
    assert_issue(
        &issues,
        Severity::Warning,
        "map field",
        "is 5120×4096 pixels: many phones cannot load a texture over 4096",
    );

    write(root, "gfx/maps/field.png", b"GIF89a not a png at all");
    let issues = picture_issues(&pack, root);
    assert_issue(&issues, Severity::Error, "map field", "not a readable PNG");
}
