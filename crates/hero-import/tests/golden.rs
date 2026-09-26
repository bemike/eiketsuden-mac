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
//! is not checked: it hashes an undocumented output layout (see `tests/tfdce_golden.rs` for the
//! portrait checks).
//!
//! [`checks_pass_on_a_synthetic_known_answer_install`] runs the same checks on a synthetic
//! install built to the documented shapes (with this crate's own encoders), so the checks
//! themselves are exercised in CI.

use hero_import::edition::{identify, EditionId};
use hero_import::install::InstallDir;
use hero_import::ls11::Archive;
use hero_import::scenario::{self, ArgKind, Operands};
use hero_import::text::{build_messages, parse_messages, split_strings, TextEncoding};
use hero_import::{bakdata, extract, ls11, palette, probe, sprites, table6};
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

/// Files of a specific copy that are known to be damaged, identified by SHA-256 (a hash of
/// the owner's file, not game content), with the exact validation error they produce and the
/// number of leading entries that still decode. A damaged copy cannot be repaired by a
/// decoder; listing it keeps the other containers checked instead of hiding them behind one
/// expected failure.
///
/// `OPGRP.R3` of the verified Korean copy: 6 bytes longer than its directory, and from about
/// file offset 0x60400 (inside entry 27) the stored bytes stop being an LS11 stream (entry 27's
/// picture decodes cleanly for its first rows, then breaks; every later compressed entry fails
/// within its first bytes, and the "raw" entries after it are noise instead of `NPK016` files).
const KNOWN_DAMAGED: [(&str, &str, &str, usize); 1] = [(
    "OPGRP.R3",
    "eed970e71f4dd7abbcf910aacf232ea37c94af935900c870b8e991fd6f0f6f2b",
    "LS11: entries end at 0xb40aa but the file is 0xb40b0 bytes long",
    27,
)];

/// For a known damaged file: the leading entries still decode when the trailing bytes are cut
/// off, and the first damaged entry does not.
fn check_known_damage(dir: &Path, name: &str, good_entries: usize) {
    let data = std::fs::read(dir.join(name)).unwrap();
    let archive = (0..16)
        .find_map(|cut| Archive::parse(&data[..data.len() - cut]).ok())
        .unwrap_or_else(|| panic!("{name}: no valid directory even without trailing bytes"));
    for i in 0..good_entries {
        archive
            .decode(i)
            .unwrap_or_else(|e| panic!("{name}: undamaged entry {i}: {e}"));
    }
    assert!(
        archive.decode(good_entries).is_err(),
        "{name}: entry {good_entries} decodes; the damage record is out of date"
    );
}

/// Every LS11 archive and 6-byte table in the folder validates and decodes (any edition),
/// except the files of [`KNOWN_DAMAGED`], which must fail exactly as recorded.
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
            let damaged = KNOWN_DAMAGED
                .iter()
                .find(|(name, sha, _, _)| f.path == *name && f.sha256 == *sha);
            if let Some((name, _, error, good)) = damaged {
                eprintln!("note: {name} is a known damaged copy: {error}");
                assert_eq!(l.error.as_deref(), Some(*error), "{name}: damage changed");
                check_known_damage(dir, name, *good);
            } else if let Some(e) = l.error.as_ref().or(l.decode_error.as_ref()) {
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

/// Entry counts and data starts of the Korean build's 6-byte tables: 240 portraits from 0x5A0,
/// 38 common graphics from 0xE4.
fn check_korean_tables(install: &InstallDir) {
    for (name, count, start) in [("FACEDAT.R3", 240, 0x5a0), ("PACKGRP.R3", 38, 0xe4)] {
        let data = read(install, name);
        let t = table6::Table6::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(t.len(), count, "{name} entry count");
        assert_eq!(t.data_start(), start, "{name} data start");
    }
}

/// Entry counts, data starts and directory byte order of the Korean build's LS11 archives.
fn check_korean_containers(install: &InstallDir) {
    // The opening / ending archives spell the magic `Ls11` and store the directory
    // little-endian; all their entries decode (OPGRP.R3 of the verified copy is damaged and
    // covered by KNOWN_DAMAGED instead).
    for (name, count) in [("END1GRP.R3", 72), ("END2GRP.R3", 25)] {
        let data = read(install, name);
        let archive = Archive::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            archive.byte_order(),
            ls11::ByteOrder::Little,
            "{name} byte order"
        );
        assert_eq!(archive.len(), count, "{name} entry count");
        archive
            .decode_all()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
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

/// Scene counts, message tables, the prologue's block table, sizes, and the verified text
/// layout: every scene's script decodes, every text operand resolves (dialogues end with FFFF,
/// strings with NUL) and together they cover every byte of every message section.
fn check_korean_scenario_text(install: &InstallDir) {
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
        for (i, (data, section)) in d.iter().zip(&parsed.sections).enumerate() {
            let scene = scenario::parse_scene(data).unwrap_or_else(|e| panic!("SNR{n}D {i}: {e}"));
            let mut covered = vec![false; section.bytes.len()];
            for instr in scene.instructions() {
                for arg in instr.operands.args() {
                    let off = arg.value as usize;
                    let at =
                        || format!("SNR{n} scene {i} {} at {:#x}", instr.mnemonic, instr.offset);
                    match arg.kind {
                        ArgKind::Dialogue => {
                            let (_, end) = section
                                .dialogue_at(off)
                                .unwrap_or_else(|e| panic!("{}: {e}", at()));
                            covered[off..end].fill(true);
                        }
                        ArgKind::Message => {
                            let s = section
                                .string_at(off)
                                .unwrap_or_else(|e| panic!("{}: {e}", at()));
                            covered[off..=off + s.len()].fill(true);
                        }
                        _ => {}
                    }
                }
            }
            let missing = covered.iter().filter(|&&c| !c).count();
            assert_eq!(missing, 0, "SNR{n}M section {i}: bytes no script refers to");
        }
    }
    // Block table of the prologue's only scene.
    let scene = &entries(install, "SNR0D.R3")[0];
    let table: Vec<u16> = scene
        .chunks_exact(2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .take_while(|&v| v != 0xffff)
        .collect();
    assert_eq!(table, PROLOGUE_OFFSETS, "SNR0D scene 0 block table");
    // Sizes (the notes do not say whether they are file or message sizes; either matches).
    for (name, size) in [("SNR0M.R3", 10_920), ("IPPAN0M.R3", 37_580)] {
        let (file_len, message) = message_bytes(install, name);
        let message_len = message.len();
        assert!(
            file_len == size || message_len == size,
            "{name}: {file_len} bytes on disk, {message_len} message bytes, documented {size}"
        );
    }
    // IPPAN0M.R3 is a pool of 653 NUL-terminated strings (count published by another project).
    let (_, ippan) = message_bytes(install, "IPPAN0M.R3");
    let pool = split_strings(&ippan).expect("IPPAN0M.R3 ends with a NUL");
    assert_eq!(pool.len(), 653, "IPPAN0M.R3 strings");
}

/// Facts of the verified copy that the synthetic install does not reproduce: totals, the
/// wrapped section bases of SNR3M, the first battles of the prologue and BAKDATA.R3.
fn check_korean_scenario_facts(install: &InstallDir) {
    let names = bakdata::parse(&read(install, "BAKDATA.R3"), TextEncoding::EucKr)
        .expect("BAKDATA.R3 layout");
    let (mut blocks, mut records, mut instructions) = (0, 0, 0);
    let (mut dialogues, mut lines, mut strings) = (0, 0, 0);
    for n in 0..5 {
        let d = entries(install, &format!("SNR{n}D.R3"));
        let (_, m) = message_bytes(install, &format!("SNR{n}M.R3"));
        let parsed = parse_messages(&m).unwrap();
        if n == 3 {
            let bases: Vec<usize> = parsed.sections.iter().map(|s| s.base).collect();
            assert!(
                bases[3] > 0xffff && bases[4] > bases[3],
                "SNR3M bases {bases:x?}"
            );
            assert_eq!(bases[3] & 0xffff, 0x1029, "SNR3M section 3 stored base");
        }
        for (data, section) in d.iter().zip(&parsed.sections) {
            let scene = scenario::parse_scene(data).unwrap();
            blocks += scene.blocks.len();
            records += scene.blocks.iter().map(|b| b.records.len()).sum::<usize>();
            let mut seen_d = std::collections::BTreeSet::new();
            let mut seen_s = std::collections::BTreeSet::new();
            for instr in scene.instructions() {
                instructions += 1;
                for arg in instr.operands.args() {
                    match arg.kind {
                        ArgKind::Dialogue if seen_d.insert(arg.value) => {
                            let (l, _) = section.dialogue_at(arg.value as usize).unwrap();
                            lines += l.len();
                            assert!(l.iter().all(|l| (l.speaker as usize) < bakdata::OFFICERS));
                        }
                        ArgKind::Message => {
                            seen_s.insert(arg.value);
                        }
                        _ => {}
                    }
                }
            }
            dialogues += seen_d.len();
            strings += seen_s.len();
        }
    }
    assert_eq!(
        (blocks, records, instructions),
        (225, 2144, 14_404),
        "blocks, trigger records, instructions (decoded per record)"
    );
    assert_eq!(
        (dialogues, lines, strings),
        (2_619, 4_783, 901),
        "distinct dialogues, their lines, distinct strings"
    );

    // Published facts (research notes: campaign / mechanics): the prologue's first two
    // battles last 30 turns and are won by defeating Hua Xiong and Lü Bu (PC levels 5 and 6);
    // Tao Qian and Gongsun Zan join them only when talked to (flag-conditional slots).
    let prologue = scenario::parse_scene(&entries(install, "SNR0D.R3")[0]).unwrap();
    let setups: Vec<_> = prologue
        .instructions()
        .filter_map(|i| match &i.operands {
            Operands::BattleSetup { header, units } => Some((header.clone(), units.clone())),
            _ => None,
        })
        .collect();
    let rosters: Vec<_> = prologue
        .instructions()
        .filter_map(|i| match &i.operands {
            Operands::Roster { friendly, units } => Some((*friendly, units.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(setups.len(), 2, "prologue battle set-ups");
    assert_eq!(names.officer_name(0), Some("유비"));
    for (((header, units), (friendly, enemies)), (boss, level)) in
        setups.iter().zip(&rosters).zip([("화웅", 5), ("여포", 6)])
    {
        assert_eq!(header.turn_limit, 30);
        let target = header.defeat_to_win.expect("victory officer");
        assert_eq!(names.officer_name(target), Some(boss));
        assert_eq!(
            header.lose_if_defeated,
            Some(0),
            "defeat when Liu Bei retreats"
        );
        let conditional: Vec<&str> = units
            .iter()
            .filter(|u| u.requires_flag.is_some())
            .map(|u| names.officer_name(u.person).unwrap())
            .collect();
        assert_eq!(conditional, ["공손찬", "도겸"]);
        assert!(!friendly);
        let b = enemies
            .iter()
            .find(|u| u.person == target)
            .expect("boss unit");
        assert_eq!(b.level, Some(level), "{boss} level");
        assert_eq!(b.class, Some(6), "{boss} is light cavalry");
    }

    // BAKDATA.R3: Cao Cao LEAD 98 / WAR 75, Jiang Wei INT 94 (published stats).
    let find = |name: &str| names.officers.iter().find(|o| o.name == name).unwrap();
    let cao = find("조조");
    assert_eq!((cao.leadership, cao.war), (98, 75));
    assert_eq!(find("강유").intelligence, 94);
    assert_eq!(names.items.len(), 64);
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

/// Sprite archives: entry sizes match the geometries of `sprites::ARCHIVES`, the entry groups
/// cover exactly the entries, and the palette bank has the verified structure.
fn check_korean_sprites(install: &InstallDir) {
    for (name, count, sizes) in [
        ("HEXBCHR.R3", 181, &[2048usize, 1152][..]),
        ("HEXICHR.R3", 78, &[4608][..]),
        ("HEXZCHR.R3", 47, &[1024][..]),
    ] {
        let e = entries(install, name);
        assert_eq!(e.len(), count, "{name} entry count");
        assert!(
            e.iter().all(|x| sizes.contains(&x.len())),
            "{name}: entry sizes other than {sizes:?}"
        );
        let spec = sprites::archive(name).unwrap();
        assert_eq!(
            spec.groups.last().map(|g| g.last + 1),
            Some(count),
            "{name}: groups do not end at the last entry"
        );
    }
    // HEXBCHR: 169 frames of 64×64, then 12 of 48×48.
    let e = entries(install, "HEXBCHR.R3");
    assert!(e[..169].iter().all(|x| x.len() == 2048));
    assert!(e[169..].iter().all(|x| x.len() == 1152));
    // Every entry of every sprite archive converts except the two text entries of HEXGRP.
    let mut images = 0;
    for spec in &sprites::ARCHIVES {
        for (i, x) in entries(install, spec.file).iter().enumerate() {
            match (spec.arrangement)(i, x.len()) {
                Some(a) => {
                    sprites::decode_entry(x, a)
                        .unwrap_or_else(|e| panic!("{} entry {i}: {e}", spec.file));
                    images += 1;
                }
                None => assert!(
                    spec.file == "HEXGRP.R3" && (i == 1 || i == 2),
                    "{} entry {i} ({} bytes) has no geometry",
                    spec.file,
                    x.len()
                ),
            }
        }
    }
    assert_eq!(images, 314, "images from the sprite archives");
    // Palette slot 4 is the 8-colour digital palette: every channel 0 or 255, and the
    // [B][R][G] order puts blue at index 1, red at 2, green at 4.
    let bank = palette::find_bank(&read(install, "MAIN.EXE")).expect("palette bank");
    assert_eq!(bank.slots[4][1], [0, 0, 255]);
    assert_eq!(bank.slots[4][2], [255, 0, 0]);
    assert_eq!(bank.slots[4][4], [0, 255, 0]);
    // Colours 0–7 are shared by every slot except the digital one.
    for s in [0, 1, 2, 3, 5, 6, 7, 8] {
        assert_eq!(
            bank.slots[s][..8],
            bank.slots[1][..8],
            "slot {s} colours 0–7"
        );
    }
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
    check_korean_tables(&install);
    check_korean_map_geometry(&install);
    check_korean_scenario_text(&install);
    check_korean_palette(&install);
    check_korean_sprites(&install);
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

/// Run one Korean check on the real install (one test per topic, so a failure names the
/// topic).
fn korean_check(test: &str, check: fn(&InstallDir)) {
    if let Some(dir) = korean_dir(test) {
        check(&InstallDir::open(&dir).expect("install folder is readable"));
    }
}

#[test]
fn golden_korean_ls11_archives() {
    korean_check("golden_korean_ls11_archives", check_korean_containers);
}

#[test]
fn golden_korean_table_containers() {
    korean_check("golden_korean_table_containers", check_korean_tables);
}

#[test]
fn golden_korean_map_geometry() {
    korean_check("golden_korean_map_geometry", check_korean_map_geometry);
}

#[test]
fn golden_korean_scenario_text() {
    korean_check("golden_korean_scenario_text", check_korean_scenario_text);
}

#[test]
fn golden_korean_scenario_facts() {
    korean_check("golden_korean_scenario_facts", check_korean_scenario_facts);
}

#[test]
fn golden_korean_palette() {
    korean_check("golden_korean_palette", check_korean_palette);
}

#[test]
fn golden_korean_sprites() {
    korean_check("golden_korean_sprites", check_korean_sprites);
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
    // Every slot the 8-colour digital palette (index bit 0 = blue, 1 = red, 2 = green), twice.
    let digital: [[u8; 3]; 16] = std::array::from_fn(|c| {
        let on = |bit: usize| if c & (1 << bit) != 0 { 15 } else { 0 };
        [on(1), on(2), on(0)]
    });
    exe.extend(palette::build_bank(&[digital; palette::SLOTS]));
    exe.extend(vec![0xcc; 64]);
    std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();

    // A 64×80 TF-DCE image made by hand: planes 0–2 filled with 0x80, 0, 0, plane 3 cleared.
    let one = [2u8, b'T', 8, 80, 0, 0x11, 0x01, 0xE4, 0, 0x80, 0, 0];
    for (name, count) in [("FACEDAT.R3", 240), ("PACKGRP.R3", 38)] {
        let payloads: Vec<&[u8]> = vec![&one[..]; count];
        std::fs::write(dir.join(name), table6::build(&payloads).unwrap()).unwrap();
    }
    let cell = vec![0x5au8; 128];
    let cells = |n: usize| cell.repeat(n);
    let mut unit_frames = vec![cells(16); 169];
    unit_frames.extend(vec![cells(9); 12]);
    write_ls11(dir, "HEXBCHR.R3", &unit_frames);
    write_ls11(dir, "HEXICHR.R3", &vec![cells(36); 78]);
    write_ls11(dir, "HEXZCHR.R3", &vec![cells(8); 47]);
    write_ls11(dir, "HEXZCHP.R3", &[cells(80), cells(174), cells(175)]);
    write_ls11(dir, "HEXBCHP.R3", &[cells(224)]);
    write_ls11(dir, "MMAPBGPL.R3", std::slice::from_ref(&cell));
    write_ls11(dir, "SMAPBGPL.R3", &[cell.clone(), cell.clone()]);
    write_ls11(
        dir,
        "HEXGRP.R3",
        &[cells(114), vec![0xb0; 1576], vec![0xb1; 2016]],
    );
    // Opening / ending archives with little-endian directories.
    for (name, count) in [("END1GRP.R3", 72), ("END2GRP.R3", 25)] {
        let parts: Vec<Vec<u8>> = (0..count).map(|i| vec![i as u8; 40]).collect();
        let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
        std::fs::write(
            dir.join(name),
            ls11::build_with(ls11::ByteOrder::Little, &refs),
        )
        .unwrap();
    }
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

    // Scenario: SNR0M is exactly 10,920 bytes (1,000 strings), each scene's script narrates
    // every string of its section, the prologue's blocks start at the verified offsets.
    let chapters = [
        (0, 1, 1000),
        (1, 5, 1200),
        (2, 4, 1100),
        (3, 5, 1300),
        (4, 3, 1077),
    ];
    for (n, scenes, count) in chapters {
        let text: Vec<Vec<Vec<u8>>> = if n == 0 {
            vec![blocks(1000, 10_920 - 2)]
        } else {
            sections(scenes, count)
        };
        let file = build_messages(&text);
        if n == 0 {
            assert_eq!(file.len(), 10_920);
        }
        let bytecode: Vec<Vec<u8>> = text
            .iter()
            .map(|items| {
                let mut at = 0u16;
                let offsets: Vec<u16> = items
                    .iter()
                    .map(|s| {
                        let o = at;
                        at += s.len() as u16 + 1;
                        o
                    })
                    .collect();
                let blocks: &[u16] = if n == 0 { &PROLOGUE_OFFSETS } else { &[4] };
                narrating_scene(blocks, &offsets)
            })
            .collect();
        write_ls11(dir, &format!("SNR{n}D.R3"), &bytecode);
        // Chapter 2 is stored LS11-wrapped, like some message files may be.
        if n == 2 {
            write_ls11(dir, "SNR2M.R3", &[file]);
        } else {
            std::fs::write(dir.join(format!("SNR{n}M.R3")), file).unwrap();
        }
    }
    let ippan: Vec<u8> = blocks(653, 37_580)
        .into_iter()
        .flat_map(|b| b.into_iter().chain([0]))
        .collect();
    assert_eq!(ippan.len(), 37_580);
    std::fs::write(dir.join("IPPAN0M.R3"), ippan).unwrap();
}

/// A scene whose blocks start at `blocks` (the first right after the table); every block has
/// one unconditional record, the last one's script narrates each string offset in turn.
fn narrating_scene(blocks: &[u16], strings: &[u16]) -> Vec<u8> {
    let mut out: Vec<u8> = blocks.iter().flat_map(|b| b.to_le_bytes()).collect();
    out.extend([0xff, 0xff]);
    for (i, &block) in blocks.iter().enumerate() {
        assert!(out.len() <= block as usize, "fixture blocks overlap");
        out.resize(block as usize, 0);
        out.extend([0u8; 8]);
        out.extend(20u16.to_le_bytes());
        out.extend([0xff; 10]);
        if i + 1 == blocks.len() {
            for s in strings {
                out.push(0x08);
                out.extend(s.to_le_bytes());
            }
        } else {
            out.push(0x12);
        }
        out.push(0xff);
    }
    out
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
