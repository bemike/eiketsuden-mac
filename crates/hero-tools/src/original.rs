//! `hero-tools original probe|extract|pack`: the experimental importer for a legally owned copy of
//! the original game (see `docs/ORIGINAL_DATA.md` and the `hero-import` crate).

use hero_core::pack::Severity;
use hero_import::edition::{Edition, EditionId};
use hero_import::extract::{self, Index, KindReport, Options, Selection, Status};
use hero_import::install::lies_inside;
use hero_import::pack::{self, PackIndex, PackOptions};
use hero_import::probe::{self, Manifest};
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

/// `original probe`: print what the folder holds and optionally save the shareable manifest.
pub fn run_probe(dir: &Path, out: Option<&Path>) -> Result<bool, String> {
    if let Some(out) = out {
        let inside = lies_inside(out, dir).map_err(|e| format!("{}: {e}", out.display()))?;
        if inside {
            return Err(format!(
                "{} lies inside the probed folder; write the manifest somewhere else (the folder is only read)",
                out.display()
            ));
        }
    }
    let manifest = probe::probe(dir).map_err(|e| e.to_string())?;
    print!("{}", render_probe(dir, &manifest));
    match out {
        Some(out) => {
            let mut json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
            json.push(b'\n');
            std::fs::write(out, json).map_err(|e| format!("{}: {e}", out.display()))?;
            println!(
                "Manifest:  written to {} — file names, sizes, SHA-256 hashes, the first 16 bytes of\n           \
                 each file and container summaries, no game content. Check the file list before sharing it.",
                out.display()
            );
        }
        None => {
            println!("Manifest:  not written (add --out <file> to save the shareable manifest)")
        }
    }
    Ok(true)
}

fn render_edition(out: &mut String, edition: &Edition) {
    let _ = writeln!(
        out,
        "Edition:   {} [{}], confidence {}{}",
        edition.name,
        edition.id.as_str(),
        edition.confidence.as_str(),
        if edition.forced {
            " (chosen with --edition)"
        } else {
            ""
        }
    );
    for line in &edition.evidence {
        let _ = writeln!(out, "           - {line}");
    }
}

/// What the importer can do for an edition, in one line.
fn support_line(id: EditionId) -> &'static str {
    match id {
        EditionId::KoreanDos | EditionId::ChineseDos => {
            "text, sprites and portraits can be extracted (`hero-tools original extract`: \
             scenario scripts and dialogue, BAKDATA officers / items / townsfolk, unit frames, \
             map icons, chips, battle UI icons, palettes, contact sheets and TF-DCE portraits), \
             and so can battle, campaign and town maps; opening / ending pictures are not read yet"
        }
        EditionId::Steam2017 => {
            "not extractable yet: the Steam container format is unknown. Sharing this manifest \
             helps add support (docs/ORIGINAL_DATA.md)"
        }
        EditionId::Pc98Images => {
            "disk images recognised; reading the files inside them is not implemented yet"
        }
        EditionId::Unknown => "no supported edition recognised (see the evidence above)",
    }
}

pub fn render_probe(dir: &Path, m: &Manifest) -> String {
    let mut out = format!("Probed:    {} (read-only)\n", dir.display());
    render_edition(&mut out, &m.edition);
    let s = &m.summary;
    let _ = writeln!(
        out,
        "Files:     {} files, {} bytes; {} LS11 archives ({} failing validation), {} 6-byte tables, {} disk images",
        s.files, s.total_bytes, s.ls11_archives, s.ls11_failures, s.table6_containers, s.disk_images
    );
    for f in &m.files {
        let error = f
            .ls11
            .as_ref()
            .and_then(|l| l.error.as_ref().or(l.decode_error.as_ref()))
            .or(f.error.as_ref());
        if let Some(e) = error {
            let _ = writeln!(out, "           ! {}: {e}", f.path);
        }
    }
    if m.truncated {
        let _ = writeln!(
            out,
            "           ! stopped after {} files; point the probe at the game folder itself",
            probe::MAX_FILES
        );
    }
    if !m.skipped.is_empty() {
        let _ = writeln!(
            out,
            "           {} paths skipped (listed in the manifest)",
            m.skipped.len()
        );
    }
    let _ = writeln!(out, "Support:   {}", support_line(m.edition.id));
    out
}

/// `original extract`: convert into a media overlay folder. `Ok(false)` when a kind failed.
pub fn run_extract(
    dir: &Path,
    out: &Path,
    selection: Option<Selection>,
    edition: Option<EditionId>,
) -> Result<bool, String> {
    let options = Options { selection, edition };
    let index = extract::extract(dir, out, &options).map_err(|e| e.to_string())?;
    print!("{}", render_extract(dir, out, &index));
    Ok(index.success())
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Extracted => "extracted",
        Status::Partial => "partial",
        Status::Failed => "FAILED",
        Status::Unsupported => "unsupported",
        Status::MissingSource => "no source",
    }
}

fn render_kind(out: &mut String, kind: &str, r: &KindReport) {
    let marker = if r.ok() { " " } else { "!" };
    let _ = writeln!(
        out,
        "{marker} {kind:<10} {:<12} {}",
        status_word(r.status),
        r.summary
    );
    for e in &r.errors {
        let _ = writeln!(out, "    error: {e}");
    }
    for n in &r.notes {
        let _ = writeln!(out, "    note:  {n}");
    }
}

pub fn render_extract(dir: &Path, out_dir: &Path, index: &Index) -> String {
    let mut out = format!("Extracted: {} -> {}\n", dir.display(), out_dir.display());
    render_edition(&mut out, &index.edition);
    for (kind, report) in &index.assets {
        render_kind(&mut out, kind, report);
    }
    let _ = writeln!(
        out,
        "Wrote {} files and {} to {}.",
        index.files.len(),
        extract::INDEX_FILE,
        out_dir.display()
    );
    if index.success() {
        let _ = writeln!(
            out,
            "Use them in the game (native builds): eiketsuden --original \"{}\"",
            out_dir.display()
        );
    } else {
        let _ = writeln!(
            out,
            "Some asset kinds were not extracted (marked with !); see above."
        );
    }
    out
}

/// The directory `to` relative to the directory `from` (which need not exist yet), with `/`
/// separators, for `extends`.
pub fn relative_dir(from: &Path, to: &Path) -> Result<String, String> {
    let to = to
        .canonicalize()
        .map_err(|e| format!("{}: {e}", to.display()))?;
    // Canonicalize the part of `from` that exists and append the rest.
    let mut existing = from.to_path_buf();
    let mut rest = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| format!("{}: not a usable folder path", from.display()))?
            .to_os_string();
        rest.push(name);
        existing = match existing.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
    }
    let mut from_abs = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    for name in rest.into_iter().rev() {
        from_abs.push(name);
    }
    let a: Vec<Component> = from_abs.components().collect();
    let b: Vec<Component> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if common == 0 {
        return Err(format!(
            "{} and {} are on different drives; `extends` must be a relative path, so put the \
             pack next to the base pack (e.g. data/original next to data/base)",
            from.display(),
            to.display()
        ));
    }
    let mut parts: Vec<String> = vec!["..".to_string(); a.len() - common];
    for c in &b[common..] {
        let Component::Normal(name) = c else {
            return Err(format!("{}: unexpected path component", to.display()));
        };
        parts.push(
            name.to_str()
                .ok_or_else(|| format!("{}: the path is not UTF-8", to.display()))?
                .to_string(),
        );
    }
    if b.len() == common || a.len() == common {
        return Err(format!(
            "{} and the pack it extends, {}, must be separate folders (neither inside the other)",
            from.display(),
            to.display()
        ));
    }
    Ok(parts.join("/"))
}

/// `original pack`: write the original-mode pack on top of `base` (default: `base` next to
/// `out`) and validate it. `Ok(false)` when a kind failed or the written pack has errors.
pub fn run_pack(
    dir: &Path,
    out: &Path,
    base: Option<&Path>,
    edition: Option<EditionId>,
) -> Result<bool, String> {
    let base = match base {
        Some(b) => b.to_path_buf(),
        None => out
            .parent()
            .map(|p| p.join("base"))
            .ok_or_else(|| format!("{}: cannot find the base pack next to it", out.display()))?,
    };
    let parent = crate::load_pack(&base)?;
    let extends = relative_dir(out, &base)?;
    let options = PackOptions::for_pack(&parent, extends, edition);
    let index = pack::write_pack(dir, out, &options).map_err(|e| e.to_string())?;
    print!("{}", render_pack(dir, out, &index));
    let written = crate::load_pack(out)?;
    let issues = crate::validate::check(out, &written)?;
    println!("\nValidation of the written pack:");
    print!("{}", crate::validate::render(&written, &issues));
    let valid = !issues.iter().any(|i| i.severity == Severity::Error);
    if index.success() && valid {
        println!(
            "Play it (native builds): eiketsuden --data \"{}\"",
            out.display()
        );
    }
    Ok(index.success() && valid)
}

pub fn render_pack(dir: &Path, out_dir: &Path, index: &PackIndex) -> String {
    let mut out = format!(
        "Original pack: {} -> {} (extends {}, canvas {}×{})\n",
        dir.display(),
        out_dir.display(),
        index.extends,
        index.canvas[0],
        index.canvas[1]
    );
    render_edition(&mut out, &index.edition);
    for (kind, report) in &index.assets {
        render_kind(&mut out, kind, report);
    }
    let _ = writeln!(
        out,
        "Wrote {} files and {} to {}.",
        index.files.len(),
        pack::PACK_INDEX,
        out_dir.display()
    );
    if !index.success() {
        let _ = writeln!(
            out,
            "Some asset kinds were not converted (marked with !); see above."
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_import::text::{build_messages, TextEncoding};
    use hero_import::{ls11, palette, scenario, table6};
    use std::path::PathBuf;

    struct Temp(PathBuf);

    impl Temp {
        fn new(label: &str) -> Temp {
            let p = std::env::temp_dir().join(format!(
                "hero-tools-original-{}-{label}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Temp(p)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A tiny synthetic Korean DOS/V install made with the importer's own encoders.
    fn synthetic_install(dir: &Path) {
        let enc = |s: &str| TextEncoding::EucKr.encode(s).unwrap();
        std::fs::write(dir.join("DISK1.R3I"), enc("DOS/V 삼국지영걸전 1")).unwrap();
        let mut exe = b"MZ".to_vec();
        exe.extend(palette::build_bank(&[[[1, 2, 3]; 16]; palette::SLOTS]));
        std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
        // One scene whose script narrates the section's only string.
        let text = build_messages(&[vec![enc("가나다라마바사")]]);
        std::fs::write(dir.join("SNR0M.R3"), text).unwrap();
        let scene = scenario::build_scene(&[vec![([0; 8], vec![0x08, 0, 0, 0xff])]]);
        std::fs::write(dir.join("SNR0D.R3"), ls11::build(&[&scene])).unwrap();
        std::fs::write(dir.join("HEXBCHR.R3"), ls11::build(&[&[0x55; 128 * 9]])).unwrap();
        // One 8×1 TF-DCE image: planes 0–2 filled with 0x80, 0, 0 (methods 1, 1, 1, 0).
        let face: &[u8] = &[2, b'T', 1, 1, 0, 0x11, 0x01, 0xE4, 0, 0x80, 0, 0];
        std::fs::write(dir.join("FACEDAT.R3"), table6::build(&[face]).unwrap()).unwrap();
    }

    #[test]
    fn probe_writes_a_manifest_outside_the_install() {
        let tmp = Temp::new("probe");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);

        let manifest = tmp.0.join("manifest.json");
        assert_eq!(run_probe(&game, Some(manifest.as_path())), Ok(true));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(json["edition"]["id"], "korean-dos");
        assert_eq!(json["format"], "eiketsuden-original-probe");

        let err = run_probe(&game, Some(game.join("m.json").as_path())).unwrap_err();
        assert!(err.contains("inside the probed folder"), "{err}");
        assert!(!game.join("m.json").exists());

        let text = render_probe(&game, &probe::probe(&game).unwrap());
        assert!(text.contains("[korean-dos], confidence high"), "{text}");
        assert!(
            text.contains("text, sprites and portraits can be extracted"),
            "{text}"
        );
    }

    #[test]
    fn extract_reports_every_kind_and_exit_status() {
        let tmp = Temp::new("extract");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);

        let out = tmp.0.join("overlay");
        assert_eq!(run_extract(&game, &out, None, None), Ok(true));
        assert!(out.join("index.json").is_file());
        assert!(out.join("text/snr0.json").is_file());
        assert!(out.join("gfx/original/hexbchr/000.png").is_file());

        // Portraits alone.
        let portraits = Some(Selection {
            portraits: true,
            ..Selection::default()
        });
        assert_eq!(run_extract(&game, &out, portraits, None), Ok(true));
        assert!(out.join("gfx/original/facedat/000.png").is_file());

        let index = extract::extract(&game, &out, &Options::default()).unwrap();
        let text = render_extract(&game, &out, &index);
        assert!(text.contains("portraits  extracted"), "{text}");
        assert!(text.contains("eiketsuden --original"), "{text}");

        // Explicitly asking for a kind that fails is a failure.
        std::fs::write(game.join("FACEDAT.R3"), table6::build(&[b"x"]).unwrap()).unwrap();
        assert_eq!(run_extract(&game, &out, portraits, None), Ok(false));
        let index = extract::extract(&game, &out, &Options::default()).unwrap();
        let text = render_extract(&game, &out, &index);
        assert!(text.contains("portraits  FAILED"), "{text}");

        // Not an install at all: a clear error.
        let err = run_extract(&tmp.0.join("nope"), &out, None, None).unwrap_err();
        assert!(err.contains("nope"), "{err}");
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn relative_dirs_for_extends() {
        let tmp = Temp::new("relative");
        let base = tmp.0.join("data/base");
        std::fs::create_dir_all(&base).unwrap();
        assert_eq!(
            relative_dir(&tmp.0.join("data/original"), &base),
            Ok("../base".into())
        );
        assert_eq!(
            relative_dir(&tmp.0.join("mods/deep/original"), &base),
            Ok("../../../data/base".into())
        );
        std::fs::create_dir_all(tmp.0.join("data/original")).unwrap();
        assert_eq!(
            relative_dir(&tmp.0.join("data/original"), &base),
            Ok("../base".into())
        );
        for inside in [base.clone(), base.join("sub"), tmp.0.join("data")] {
            let err = relative_dir(&inside, &base).unwrap_err();
            assert!(err.contains("separate folders"), "{err}");
        }
        assert!(relative_dir(&tmp.0.join("x"), &tmp.0.join("missing")).is_err());
    }

    #[test]
    fn pack_is_written_next_to_its_base_and_validated() {
        let tmp = Temp::new("pack");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        synthetic_install(&game);
        copy_dir(&crate::tests::fixture_dir(), &tmp.0.join("data/base"));
        let out = tmp.0.join("data/original");
        // The tiny install has no battle maps and the fixture's classes are not the original's:
        // the pack is written and loads, but the conversion reports failures.
        assert_eq!(run_pack(&game, &out, None, None), Ok(false));
        let manifest = std::fs::read_to_string(out.join("pack.toml")).unwrap();
        assert!(manifest.contains("extends = \"../base\""), "{manifest}");
        let pack = crate::load_pack(&out).unwrap();
        assert_eq!(pack.layers.len(), 2);
        assert_eq!(pack.manifest.presentation.canvas, [640, 480]);
        let index = pack::write_pack(
            &game,
            &out,
            &PackOptions {
                extends: "../base".into(),
                ..PackOptions::default()
            },
        )
        .unwrap();
        let text = render_pack(&game, &out, &index);
        assert!(text.contains("tiles"), "{text}");
        assert!(text.contains("FAILED"), "{text}");
        // No base pack next to the output folder.
        let err = run_pack(&game, &tmp.0.join("elsewhere/original"), None, None).unwrap_err();
        assert!(err.contains("no pack.toml"), "{err}");
    }

    /// An install with two battle maps (one of them with a cell of unknown terrain code) and
    /// the `MAIN.EXE` tables and palette the map conversion needs.
    fn map_install(dir: &Path) {
        use hero_import::maps::{self, BattleMap};
        synthetic_install(dir);
        let names: Vec<&[u8]> = maps::TERRAIN_IDS.iter().map(|s| s.as_bytes()).collect();
        let mut exe = maps::build_exe_fixture(
            &maps::ExeFixture {
                second_set_maps: &[1],
                backdrop: [0; maps::TERRAIN_COUNT],
                ground: [5; maps::TERRAIN_COUNT],
                terrain_names: &names,
                campaign_sizes: [(16, 4); maps::CHAPTERS],
            },
            0,
        );
        exe.extend(palette::build_bank(&[[[1, 2, 3]; 16]; palette::SLOTS]));
        std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
        let cells = |n: usize| -> Vec<u8> { (0..n * 128).map(|i| (i % 7) as u8).collect() };
        let (common, first, second) = (cells(80), cells(4), cells(4));
        std::fs::write(
            dir.join("HEXZCHP.R3"),
            ls11::build(&[&common, &first, &second]),
        )
        .unwrap();
        // Cell (x, y) of terrain code c shows chips 4c .. 4c+3 (all shared chips).
        let map = |terrain: &[&[u8]]| {
            let (w, h) = (terrain[0].len(), terrain.len());
            let mut chips = vec![0; 4 * w * h];
            for (y, row) in terrain.iter().enumerate() {
                for (x, &code) in row.iter().enumerate() {
                    for (i, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                        chips[(2 * y + dy) * 2 * w + 2 * x + dx] = 4 * code.min(19) + i as u8;
                    }
                }
            }
            BattleMap {
                width: 2 * w,
                height: 2 * h,
                chips,
                terrain: terrain.concat(),
            }
        };
        let a = map(&[&[0, 1, 2, 0], &[3, 4, 3, 3], &[6, 5, 8, 0]]);
        let mut b = map(&[&[0, 6], &[0, 6]]);
        // A code without pack terrain (18, fire) on castle chips: the stand-in is castle.
        b.terrain[1] = 18;
        b.chips[2..4].copy_from_slice(&[24, 25]);
        b.chips[6..8].copy_from_slice(&[26, 27]);
        let enc = |s: &str| TextEncoding::EucKr.encode(s).unwrap();
        let mut names = enc("평원1\r\n성");
        names.extend_from_slice(b"\r\n\r\n\x1a");
        std::fs::write(
            dir.join("HEXZMAP.R3"),
            ls11::build(&[&a.encode(), &b.encode(), &names]),
        )
        .unwrap();
    }

    #[test]
    fn original_maps_load_and_validate_on_the_base_pack() {
        let tmp = Temp::new("maps");
        let game = tmp.0.join("GAME");
        std::fs::create_dir(&game).unwrap();
        map_install(&game);
        copy_dir(&crate::tests::fixture_dir(), &tmp.0.join("data/base"));
        let out = tmp.0.join("data/original");
        // Portraits and unit sheets fail on this install and fixture; the maps do not.
        assert_eq!(run_pack(&game, &out, None, None), Ok(false));
        let index: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join(pack::PACK_INDEX)).unwrap()).unwrap();
        assert_eq!(index["assets"]["maps"]["status"], "extracted", "{index:#}");
        assert_eq!(index["maps"][1]["stand_ins"][0]["used"], 6, "{index:#}");

        let written = crate::load_pack(&out).unwrap();
        assert_eq!(written.maps.len(), 2);
        let a = &written.maps["hexz_00"];
        assert_eq!(a.name, "평원1");
        assert_eq!(a.rows, "0120\n3433\n6580\n");
        assert_eq!(a.image.as_deref(), Some("hexz_00"));
        let b = &written.maps["hexz_01"];
        assert_eq!(b.rows, "06\n06\n");
        assert_eq!(b.legend["6"], "castle");
        // The rules grid reads as the base pack's terrain.
        let grid = hero_core::map::BattleMap::parse(&a.rows, &a.legend, &written.terrain).unwrap();
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(1, 0)),
            Some("forest")
        );
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(1, 1)),
            Some("bridge")
        );
        assert_eq!(
            grid.terrain_at(hero_core::geom::Pos::new(2, 2)),
            Some("village")
        );
        // Nothing about the maps is wrong: map files, legends, pictures and their size.
        let issues = crate::validate::check(&out, &written).unwrap();
        let about_maps: Vec<_> = issues
            .iter()
            .filter(|i| {
                i.context.starts_with("map ")
                    || i.context.contains("maps/")
                    || i.msg.contains("gfx/maps/")
            })
            .collect();
        assert!(about_maps.is_empty(), "{about_maps:#?}");
        assert!(
            crate::info::render(&written).contains("Maps:        2 in 1 files (0 used by battles)")
        );
    }

    /// The whole conversion on a real install (`EIKETSU_ORIGINAL_DIR`, see
    /// docs/ORIGINAL_DATA.md §6), on top of the repository's base pack.
    #[test]
    fn golden_original_pack() {
        let Some(dir) = std::env::var_os("EIKETSU_ORIGINAL_DIR") else {
            eprintln!("skipped: set EIKETSU_ORIGINAL_DIR to the game's data folder");
            return;
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let work = root.join("target/golden-original-pack");
        let _ = std::fs::remove_dir_all(&work);
        let out = work.join("original");
        let base = root.join("data/base");
        assert_eq!(run_pack(Path::new(&dir), &out, Some(&base), None), Ok(true));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join(pack::PACK_INDEX)).unwrap()).unwrap();
        // Every class, every terrain tile key and most officers of the base pack.
        let files: Vec<&str> = json["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert_eq!(
            files
                .iter()
                .filter(|f| f.starts_with("gfx/units/") && f.ends_with(".png"))
                .count(),
            // 19 classes, 5 officer icons (Liu Bei's three, Lü Bu's, Cao Cao's) and the
            // confusion icon, 3 sides.
            (19 + 5 + 1) * 3
        );
        let portraits = json["portraits"].as_array().unwrap().len();
        assert!(portraits >= 100, "{portraits} portraits");
        assert!(json["portraits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["officer"] == "yu_jin" && p["bakdata"] == 62));
        // The officers with their own unit icons are the original's officers 0, 4 and 8
        // (MAIN.EXE picks the icons by those numbers).
        for (officer, bakdata) in [("liu_bei", 0), ("lu_bu", 4), ("cao_cao", 8)] {
            assert!(pack::OFFICER_ICONS.iter().any(|(id, _)| *id == officer));
            assert!(
                json["portraits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["officer"] == officer && p["bakdata"] == bakdata),
                "{officer} is not BAKDATA {bakdata}"
            );
        }
        // All 58 battle maps (FORMATS.md §10.1); the only cell of a code without terrain is the
        // code 255 of map 32 (§10.4). Their pictures passed the size check of `run_pack`.
        let maps = json["maps"].as_array().unwrap();
        assert_eq!(maps.len(), 58);
        let stand_ins: Vec<(u64, u64, u64)> = maps
            .iter()
            .flat_map(|m| {
                let number = m["number"].as_u64().unwrap();
                m["stand_ins"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |s| {
                        (
                            number,
                            s["code"].as_u64().unwrap(),
                            s["used"].as_u64().unwrap(),
                        )
                    })
            })
            .collect();
        // Off the map in the original: cliff.
        assert_eq!(stand_ins, [(32, 255, 9)]);
        let pack = crate::load_pack(&out).unwrap();
        assert_eq!(pack.maps.len(), 58);
        // Map 0 is 56×32 chips, 28×16 cells.
        let first = &pack.maps["hexz_00"].rows;
        assert_eq!(first.lines().count(), 16);
        assert!(first.lines().all(|l| l.chars().count() == 28), "{first}");
        let tiles = std::fs::read_to_string(out.join("gfx/tiles/terrain.toml")).unwrap();
        for t in &pack.terrain {
            assert!(
                tiles.contains(&format!("[tiles.{}]", t.tile_key())),
                "{}",
                t.id
            );
        }

        // The terrain rules follow the player's MAIN.EXE and the maps use the closed gate.
        assert_eq!(
            json["assets"]["rules"]["status"], "extracted",
            "{:#?}",
            json["assets"]["rules"]
        );
        let manifest = std::fs::read_to_string(out.join("pack.toml")).unwrap();
        assert!(
            manifest.contains(
                "[rules]
terrain = \"rules/terrain.toml\""
            ),
            "{manifest}"
        );
        assert!(pack
            .terrain
            .iter()
            .any(|t| t.id.as_str() == hero_import::pack::CLOSED_GATE));

        // Every battle of the base pack's prologue and chapter 1 is re-staged on its original map
        // (verified values: FORMATS §13.4).
        let battles = json["battles"].as_array().unwrap();
        assert_eq!(battles.len(), 21, "{battles:#?}");
        let expect = [
            ("p1_sishui", "hexz_00", 30),
            ("p2_hulao", "hexz_01", 30),
            ("c1_jieqiao_a", "hexz_06", 40),
            ("c1_xiapi", "hexz_14", 45),
            ("c1_xuzhou2", "hexz_08", 50),
        ];
        for (id, map, turns) in expect {
            let b = &pack.battles[id];
            assert_eq!(b.map.use_map.as_deref(), Some(map), "{id}");
            assert_eq!(b.turn_limit, turns, "{id}");
        }
        // Sishui: Hua Xiong commands at the pass; the treasures lie on the granary and treasury.
        let sishui = &pack.battles["p1_sishui"];
        let hua = sishui
            .units
            .iter()
            .find(|u| u.officer.as_deref() == Some("hua_xiong"))
            .unwrap();
        assert_eq!((hua.pos.x, hua.pos.y, hua.commander), (3, 9, true));
        let map = &pack.maps["hexz_00"];
        let row = |y: i32| map.rows.lines().nth(y as usize).unwrap().to_string();
        let glyphs: Vec<char> = sishui
            .treasures
            .iter()
            .map(|t| row(t.pos.y).chars().nth(t.pos.x as usize).unwrap())
            .collect();
        let terrain_of = |g: char| map.legend[&g.to_string()].clone();
        let mut kinds: Vec<String> = glyphs.into_iter().map(terrain_of).collect();
        kinds.sort();
        assert_eq!(kinds, ["granary", "treasury"]);
        // Xuzhou II: Cao Cao's army waits off the map until Liu Bei reaches the east edge.
        let xuzhou2 = &pack.battles["c1_xuzhou2"];
        let cao = xuzhou2
            .units
            .iter()
            .find(|u| u.officer.as_deref() == Some("cao_cao"))
            .unwrap();
        let group = cao.group.clone().expect("Cao Cao arrives later");
        assert!(xuzhou2.events.iter().any(|e| {
            e.actions
                .contains(&hero_core::battledef::EventAction::Spawn {
                    group: group.clone(),
                })
                && matches!(
                    e.trigger,
                    hero_core::battledef::Trigger::Reach { to: Some(_), .. }
                )
        }));
        // The bandit chiefs keep the base pack's recruitment.
        for (id, chief) in [
            ("c1_taishan", "chang_xi"),
            ("c1_pengcheng1", "xia_kun"),
            ("c1_xiaqiu1", "shi_meng"),
        ] {
            let b = &pack.battles[id];
            assert!(
                b.units.iter().any(|u| u.officer.as_deref() == Some(chief)),
                "{id}"
            );
            assert!(!b.events.is_empty(), "{id}");
        }

        // Mid-battle events. Xuzhou II plays in three stages: Che Zhou falls (the base event,
        // with the original's retreat of his troops), Cao Cao arrives, then the south-western
        // village wins.
        use hero_core::battledef::{EventAction, Trigger};
        let stages: Vec<Option<u32>> = xuzhou2.events.iter().map(|e| e.stage).collect();
        assert_eq!(stages, [Some(0), Some(1), Some(2)]);
        assert!(xuzhou2.events[0]
            .actions
            .iter()
            .any(|a| matches!(a, EventAction::Retreat { .. })));
        assert_eq!(
            xuzhou2.events[2].trigger,
            Trigger::Reach {
                who: Some("liu_bei".into()),
                pos: hero_core::geom::Pos::new(1, 16),
                radius: 0,
                to: None
            }
        );
        // Xiapi: turn 30 or Liu Bei at (12, 12) lowers the drawbridge; the middle cell becomes a
        // bridge (the chip the game checks), the others keep their terrain with new chips.
        let xiapi = &pack.battles["c1_xiapi"];
        let bridges: Vec<_> = xiapi
            .events
            .iter()
            .flat_map(|e| &e.actions)
            .filter_map(|a| match a {
                EventAction::SetTerrain {
                    pos,
                    terrain,
                    image,
                } => Some((pos.x, pos.y, terrain.as_str(), image.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(bridges.len(), 12, "two triggers × six cells");
        assert!(bridges.contains(&(12, 11, "bridge", Some("hexz_14_12_11_2".into()))));
        assert!(bridges.contains(&(11, 11, "river", Some("hexz_14_11_11_2".into()))));
        assert!(out.join("gfx/maps/hexz_14_12_11_2.png").is_file());
        // Beihai: Taishi Ci's gate opens (a gate becomes plain).
        assert!(pack.battles["c1_beihai"]
            .events
            .iter()
            .any(|e| e.actions.contains(&EventAction::SetTerrain {
                pos: hero_core::geom::Pos::new(2, 2),
                terrain: "plain".into(),
                image: Some("hexz_07_2_2_0".into()),
            })));
        // The dialogue comes from the player's copy.
        let drama = std::fs::read_to_string(out.join("dramas/original_battles.drama")).unwrap();
        assert!(
            drama.contains("== orig_c1_julu_4_3\nguan_chun: "),
            "{drama}"
        );
        assert!(pack.scene("orig_c1_xuzhou2_4").is_some());
        let json_battles = json["battles"].as_array().unwrap();
        let events: u64 = json_battles
            .iter()
            .map(|b| b["events"].as_u64().unwrap())
            .sum();
        assert!(events >= 60, "{events} events");
    }
}
