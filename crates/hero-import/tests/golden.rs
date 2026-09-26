//! Golden tests against a real install of the original game.
//!
//! The `golden_*` tests run only when `EIKETSU_ORIGINAL_DIR` points at the folder that holds
//! the game's data files (for the Korean DOS/V build, its `GAME` folder) and are skipped
//! otherwise, so CI and the repository never need any original bytes:
//!
//! ```text
//! EIKETSU_ORIGINAL_DIR=/path/to/GAME cargo test -p hero-import --test golden -- --nocapture
//! ```
//!
//! The known answers come from the project's research notes (published facts about the Korean
//! DOS/V build). A failure means either the decoder or the documented fact is wrong; every
//! message names the fact that was checked. The published SHA-256 prefix of the decoded face 0
//! is not checked because TF-DCE decoding is not implemented.
//!
//! [`checks_pass_on_a_synthetic_known_answer_install`] runs the same checks on a synthetic
//! install built to the documented shapes (with this crate's own encoders), so the checks
//! themselves are exercised in CI.

use hero_import::edition::{identify, EditionId};
use hero_import::install::InstallDir;
use hero_import::ls11::Archive;
use hero_import::text::{build_messages, parse_messages, TextEncoding};
use hero_import::{extract, ls11, palette, probe, table6};
use std::path::{Path, PathBuf};

const ENV: &str = "EIKETSU_ORIGINAL_DIR";

// ----- shared helpers ------------------------------------------------------------------------

fn read(install: &InstallDir, name: &str) -> Vec<u8> {
    install
        .read(name)
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_else(|| panic!("{name} missing from the install"))
}

fn entries(install: &InstallDir, name: &str) -> Vec<Vec<u8>> {
    let data = read(install, name);
    let archive = Archive::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
    archive
        .decode_all()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A message file's bytes: the file itself or the single entry of an LS11 archive; also
/// returns the file size.
fn message_bytes(install: &InstallDir, name: &str) -> (usize, Vec<u8>) {
    let data = read(install, name);
    let len = data.len();
    if ls11::has_magic(&data) {
        let mut e = entries(install, name);
        assert_eq!(e.len(), 1, "{name}: a message file is one LS11 entry");
        (len, e.remove(0))
    } else {
        (len, data)
    }
}

// ----- checks --------------------------------------------------------------------------------

/// Every LS11 archive and 6-byte table in the folder validates and decodes (any edition).
fn check_every_container(dir: &Path) {
    let manifest = probe::probe(dir).expect("probe runs");
    eprintln!(
        "edition {} ({}), {} files, {} LS11 archives",
        manifest.edition.id.as_str(),
        manifest.edition.confidence.as_str(),
        manifest.summary.files,
        manifest.summary.ls11_archives
    );
    let mut failures = Vec::new();
    for f in &manifest.files {
        if let Some(l) = &f.ls11 {
            if let Some(e) = l.error.as_ref().or(l.decode_error.as_ref()) {
                failures.push(format!("{}: {e}", f.path));
            } else if !l.dictionary_is_permutation {
                eprintln!("note: {}: dictionary is not a permutation", f.path);
            }
        }
        if let Some(e) = &f.error {
            failures.push(format!("{}: {e}", f.path));
        }
    }
    assert!(
        failures.is_empty(),
        "invalid containers:\n{}",
        failures.join("\n")
    );
}

/// Entry counts and data starts of the Korean build's containers.
fn check_korean_containers(install: &InstallDir) {
    // 6-byte tables: 240 portraits from 0x5A0, 38 common graphics from 0xE4.
    for (name, count, start) in [("FACEDAT.R3", 240, 0x5a0), ("PACKGRP.R3", 38, 0xe4)] {
        let data = read(install, name);
        let t = table6::Table6::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(t.len(), count, "{name} entry count");
        assert_eq!(t.entries()[0].offset, start, "{name} data start");
    }
    for (name, count) in [
        ("HEXBCHR.R3", 181),
        ("HEXZMAP.R3", 59),
        ("HEXBMAP.R3", 9),
        ("MMAP.R3", 4),
        ("SMAP.R3", 12),
        ("PMAP.R3", 23),
        ("MMAPBGPL.R3", 1),
        ("SMAPBGPL.R3", 2),
    ] {
        assert_eq!(entries(install, name).len(), count, "{name} entry count");
    }
    let data = read(install, "HEXBCHR.R3");
    assert_eq!(
        Archive::parse(&data).unwrap().data_start(),
        0x990,
        "HEXBCHR.R3 data start"
    );
}

/// Map entry geometry: the documented size formulas hold for the decoded LS11 entries.
fn check_korean_map_geometry(install: &InstallDir) {
    // Battle maps: [W][H][W×H×5/4 bytes of 10-bit tiles]; entry 0 is 56×32; the formula holds
    // for 58 of the 59 maps.
    let maps = entries(install, "HEXZMAP.R3");
    assert_eq!(
        (maps[0][0], maps[0][1], maps[0].len()),
        (56, 32, 2 + 2240),
        "HEXZMAP entry 0"
    );
    let matching = maps
        .iter()
        .filter(|m| m.len() >= 2 && m.len() == 2 + m[0] as usize * m[1] as usize * 5 / 4)
        .count();
    assert!(
        matching >= 58,
        "only {matching} of 59 battle maps match W×H×5/4"
    );
    // Campaign maps: W×H tiles + W×H/32 mask bytes, dimensions from MAIN.EXE's table.
    let mmap = entries(install, "MMAP.R3");
    for (i, (w, h)) in [(96, 96), (72, 112), (120, 88), (112, 128)]
        .iter()
        .enumerate()
    {
        assert_eq!(
            mmap[i].len(),
            w * h + w * h / 32,
            "MMAP entry {i} ({w}×{h})"
        );
    }
    // City / inner maps: 32×20 tiles + 620-byte overlay + count + 3-byte objects.
    for name in ["SMAP.R3", "PMAP.R3"] {
        for (i, e) in entries(install, name).iter().enumerate() {
            assert!(e.len() > 1260, "{name} entry {i} too short");
            assert_eq!(e.len(), 1261 + 3 * e[1260] as usize, "{name} entry {i}");
        }
    }
}

/// Scene counts, message tables, the prologue's offset table, sizes and block counts.
fn check_korean_scenario_text(install: &InstallDir) {
    let mut blocks = 0;
    for (n, scenes) in [1usize, 5, 4, 5, 3].into_iter().enumerate() {
        let d = entries(install, &format!("SNR{n}D.R3"));
        assert_eq!(d.len(), scenes, "SNR{n}D.R3 scene count");
        let (_, m) = message_bytes(install, &format!("SNR{n}M.R3"));
        assert_eq!(
            u16::from_le_bytes([m[0], m[1]]) as usize,
            2 * scenes,
            "SNR{n}M.R3 first word"
        );
        let parsed = parse_messages(&m).unwrap_or_else(|e| panic!("SNR{n}M.R3: {e}"));
        assert_eq!(parsed.sections.len(), scenes);
        blocks += parsed.block_count();
    }
    // Event-offset table of the prologue's only scene.
    let scene = &entries(install, "SNR0D.R3")[0];
    let table: Vec<u16> = scene
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .take_while(|&v| v != 0xffff)
        .collect();
    assert_eq!(table, PROLOGUE_OFFSETS, "SNR0D scene 0 offset table");
    // Sizes (the notes do not say whether they are file or message sizes; either matches).
    for (name, size) in [("SNR0M.R3", 10_920), ("IPPAN0M.R3", 37_580)] {
        let (file_len, message) = message_bytes(install, name);
        let message_len = message.len();
        assert!(
            file_len == size || message_len == size,
            "{name}: {file_len} bytes on disk, {message_len} message bytes, documented {size}"
        );
    }
    // Block counts published by another project, whose counting method is not documented:
    // a mismatch here may be a counting difference rather than a decoding error.
    let (_, ippan) = message_bytes(install, "IPPAN0M.R3");
    let ippan_blocks = parse_messages(&ippan).unwrap().block_count();
    assert_eq!(
        (blocks, ippan_blocks),
        (5_677, 653),
        "NUL-terminated blocks in SNR0M–SNR4M and IPPAN0M"
    );
}

const PROLOGUE_OFFSETS: [u16; 10] = [
    0x16, 0x41, 0x103, 0x1b4, 0x272, 0x501, 0x77a, 0x7f9, 0xb79, 0xc27,
];
const PALETTE_OFFSET: usize = 0x38df0;

/// The palette bank sits at the documented offset of the Korean MAIN.EXE.
fn check_korean_palette(install: &InstallDir) {
    let bank = palette::find_bank(&read(install, "MAIN.EXE")).expect("palette bank");
    assert_eq!(
        bank.offset, PALETTE_OFFSET,
        "palette bank offset in MAIN.EXE"
    );
    assert_eq!(bank.slots[5], bank.slots[8]);
}

/// A default extraction of a DOS/V install reports no failure.
fn check_extraction(dir: &Path, label: &str) {
    let out = std::env::temp_dir().join(format!(
        "hero-import-golden-overlay-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out);
    let index = extract::extract(dir, &out, &extract::Options::default()).expect("extract");
    for (kind, report) in &index.assets {
        eprintln!("{kind}: {:?} — {}", report.status, report.summary);
        for e in &report.errors {
            eprintln!("  error: {e}");
        }
    }
    let ok = index.success();
    let _ = std::fs::remove_dir_all(&out);
    assert!(ok, "extraction reported failures (see above)");
}

fn check_all_korean(dir: &Path) {
    let install = InstallDir::open(dir).expect("install folder is readable");
    check_korean_containers(&install);
    check_korean_map_geometry(&install);
    check_korean_scenario_text(&install);
    check_korean_palette(&install);
}

// ----- gated tests on a real install ---------------------------------------------------------

/// The install folder, or `None` (with a note) when the tests are not configured.
fn install_dir(test: &str) -> Option<PathBuf> {
    match std::env::var_os(ENV).filter(|v| !v.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir)),
        None => {
            eprintln!("{test}: skipped ({ENV} is not set)");
            None
        }
    }
}

/// The install folder when it is identified as the Korean DOS/V build.
fn korean_dir(test: &str) -> Option<PathBuf> {
    let dir = install_dir(test)?;
    let id = identify(&InstallDir::open(&dir).expect("EIKETSU_ORIGINAL_DIR is readable")).id;
    if id != EditionId::KoreanDos {
        eprintln!(
            "{test}: skipped (the known answers are for korean-dos, this is `{}`)",
            id.as_str()
        );
        return None;
    }
    Some(dir)
}

#[test]
fn golden_every_container_validates() {
    if let Some(dir) = install_dir("golden_every_container_validates") {
        check_every_container(&dir);
    }
}

#[test]
fn golden_korean_known_answers() {
    if let Some(dir) = korean_dir("golden_korean_known_answers") {
        check_all_korean(&dir);
    }
}

#[test]
fn golden_extraction_succeeds() {
    let Some(dir) = install_dir("golden_extraction_succeeds") else {
        return;
    };
    if identify(&InstallDir::open(&dir).unwrap())
        .id
        .is_extractable()
    {
        check_extraction(&dir, "real");
    } else {
        eprintln!("golden_extraction_succeeds: skipped (not a DOS/V edition)");
    }
}

// ----- the same checks on a synthetic install ------------------------------------------------

/// `count` NUL-terminated ASCII blocks whose total size (NULs included) is `total` bytes.
fn blocks(count: usize, total: usize) -> Vec<Vec<u8>> {
    let content = total - count;
    (0..count)
        .map(|i| {
            let len = content / count + usize::from(i < content % count);
            vec![b'a' + (i % 26) as u8; len]
        })
        .collect()
}

/// Split `count` blocks over `sections` sections.
fn sections(sections: usize, count: usize) -> Vec<Vec<Vec<u8>>> {
    (0..sections)
        .map(|s| {
            let n = count / sections + usize::from(s < count % sections);
            blocks(n, n * 12)
        })
        .collect()
}

fn write_ls11(dir: &Path, name: &str, entries: &[Vec<u8>]) {
    let refs: Vec<&[u8]> = entries.iter().map(Vec::as_slice).collect();
    std::fs::write(dir.join(name), ls11::build(&refs)).unwrap();
}

/// A synthetic install with every documented Korean shape (no original bytes).
fn write_known_answer_install(dir: &Path) {
    let marker = TextEncoding::EucKr.encode("DOS/V 삼국지영걸전 1").unwrap();
    std::fs::write(dir.join("DISK1.R3I"), marker).unwrap();

    let mut exe = vec![0x90u8; PALETTE_OFFSET];
    exe.extend(palette::build_bank(&[[[3, 7, 11]; 16]; palette::SLOTS]));
    exe.extend(vec![0xcc; 64]);
    std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();

    let one = [0x11u8];
    for (name, count) in [("FACEDAT.R3", 240), ("PACKGRP.R3", 38)] {
        let payloads: Vec<&[u8]> = vec![&one[..]; count];
        std::fs::write(dir.join(name), table6::build(&payloads).unwrap()).unwrap();
    }
    let cell = vec![0x5au8; 128];
    write_ls11(dir, "HEXBCHR.R3", &vec![cell.clone(); 181]);
    write_ls11(dir, "MMAPBGPL.R3", &[cell.clone()]);
    write_ls11(dir, "SMAPBGPL.R3", &[cell.clone(), cell]);
    write_ls11(dir, "HEXBMAP.R3", &vec![vec![1, 2, 3]; 9]);

    let battle_map = |w: u8, h: u8| {
        let mut m = vec![w, h];
        m.resize(2 + w as usize * h as usize * 5 / 4, 7);
        m
    };
    let mut maps = vec![battle_map(56, 32)];
    maps.extend((0..57).map(|i| battle_map(32 + (i % 8) * 4, 22 + (i % 4) * 2)));
    maps.push(vec![9u8; 390]);
    write_ls11(dir, "HEXZMAP.R3", &maps);
    let mmap: Vec<Vec<u8>> = [(96, 96), (72, 112), (120, 88), (112, 128)]
        .iter()
        .map(|(w, h)| vec![4u8; w * h + w * h / 32])
        .collect();
    write_ls11(dir, "MMAP.R3", &mmap);
    let city = |objects: u8| {
        let mut e = vec![2u8; 1260];
        e.push(objects);
        e.extend(vec![1u8; 3 * objects as usize]);
        e
    };
    write_ls11(dir, "SMAP.R3", &(0..12).map(city).collect::<Vec<_>>());
    write_ls11(dir, "PMAP.R3", &(0..23).map(city).collect::<Vec<_>>());

    // Scenario: SNR0M is exactly 10,920 bytes with 1,000 blocks; 5,677 blocks in total.
    for (n, scenes) in [1usize, 5, 4, 5, 3].into_iter().enumerate() {
        let mut bytecode: Vec<Vec<u8>> = (0..scenes).map(|_| vec![0xff, 0xff, 0x2a]).collect();
        if n == 0 {
            bytecode[0] = PROLOGUE_OFFSETS
                .iter()
                .chain(&[0xffff])
                .flat_map(|v| v.to_le_bytes())
                .collect();
        }
        write_ls11(dir, &format!("SNR{n}D.R3"), &bytecode);
    }
    let snr0m = build_messages(&[blocks(1000, 10_920 - 2)]);
    assert_eq!(snr0m.len(), 10_920);
    std::fs::write(dir.join("SNR0M.R3"), snr0m).unwrap();
    let rest = [(1, 5, 1200), (2, 4, 1100), (3, 5, 1300), (4, 3, 1077)];
    assert_eq!(1000 + rest.iter().map(|r| r.2).sum::<usize>(), 5_677);
    for (n, scenes, count) in rest {
        let file = build_messages(&sections(scenes, count));
        // Chapter 2 is stored LS11-wrapped, like some message files may be.
        if n == 2 {
            write_ls11(dir, "SNR2M.R3", &[file]);
        } else {
            std::fs::write(dir.join(format!("SNR{n}M.R3")), file).unwrap();
        }
    }
    let ippan = build_messages(&[blocks(653, 37_580 - 2)]);
    assert_eq!(ippan.len(), 37_580);
    std::fs::write(dir.join("IPPAN0M.R3"), ippan).unwrap();
}

#[test]
fn checks_pass_on_a_synthetic_known_answer_install() {
    let dir = std::env::temp_dir().join(format!(
        "hero-import-golden-synthetic-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_known_answer_install(&dir);

    let result = std::panic::catch_unwind(|| {
        check_every_container(&dir);
        check_all_korean(&dir);
        check_extraction(&dir, "synthetic");
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
