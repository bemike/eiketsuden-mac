//! `hero-tools original probe|extract`: the experimental importer for a legally owned copy of
//! the original game (see `docs/ORIGINAL_DATA.md` and the `hero-import` crate).

use hero_import::edition::{Edition, EditionId};
use hero_import::extract::{self, Index, KindReport, Options, Selection, Status};
use hero_import::install::lies_inside;
use hero_import::probe::{self, Manifest};
use std::fmt::Write as _;
use std::path::Path;

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
            "text, sprites and portraits can be extracted (`hero-tools original extract`: unit \
             frames, map icons, chips, battle UI icons, palettes, contact sheets and TF-DCE \
             portraits); opening / ending pictures and officer names are not read yet"
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
}
