//! Test helpers: temporary folders and synthetic installs built with this crate's own encoders.
//! No byte of the original game is involved; the text is made up.

use crate::image::IndexedImage;
use crate::text::{build_sections, dialogue_bytes, TextEncoding};
use crate::{bakdata, ippan, ls11, maps, palette, planar, scenario, table6, tfdce};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A folder under the system temp directory, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "hero-import-test-{}-{label}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 16×16 cell whose pixels cycle through the colours starting at `seed`.
pub fn cell(seed: u8) -> Vec<u8> {
    planar::encode(&IndexedImage {
        width: 16,
        height: 16,
        pixels: (0..256).map(|i| ((i as u8 / 16) + seed) % 16).collect(),
    })
    .expect("valid geometry")
}

pub fn cells(count: usize) -> Vec<u8> {
    (0..count).flat_map(|i| cell(i as u8)).collect()
}

/// Palette slots with distinct, recognisable colours (slot `s`, colour `c` = R c, G 15−c, B s).
pub fn palette_slots() -> [[[u8; 3]; 16]; palette::SLOTS] {
    std::array::from_fn(|s| std::array::from_fn(|c| [c as u8, (15 - c) as u8, s as u8]))
}

/// A chapter's scene: `[string][dialogue]` in the message section and a script that shows
/// both (music, narration, dialogue, end), plus a talk trigger for officer 1 in a second block.
fn fixture_scene(
    enc: &dyn Fn(&str) -> Vec<u8>,
    narration: &str,
    lines: [&str; 2],
) -> (Vec<u8>, Vec<u8>) {
    let mut section = enc(narration);
    section.push(0);
    let dialogue_at = section.len() as u16;
    section.extend(dialogue_bytes(&[(0, enc(lines[0])), (1, enc(lines[1]))]));
    let [lo, hi] = dialogue_at.to_le_bytes();
    let script = vec![0x38, 0x02, 0x08, 0x00, 0x00, 0x00, lo, hi, 0x12, 0xff];
    let talk = vec![0x00, lo, hi, 0xff];
    let scene = scenario::build_scene(&[
        vec![([0; 8], script)],
        vec![
            ([0; 8], vec![0x0f, 0x00, 0xff]),
            ([0x03, 0x01, 0x01, 0, 0, 0, 0, 0], talk),
        ],
    ]);
    (section, scene)
}

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(dir.join(name), bytes).expect("write fixture file");
}

/// Write a DOS/V-style install whose text uses `encoding`, with `lines` as dialogue.
fn write_dos_v_install(
    dir: &Path,
    encoding: TextEncoding,
    lines: &[&str],
    names: [&str; 3],
    disk_header: &[u8],
) {
    let enc = |s: &str| encoding.encode(s).expect("fixture text is encodable");
    write(dir, "DISK1.R3I", disk_header);

    let mut exe = b"MZ".to_vec();
    exe.extend(vec![0x90; 3000]);
    exe.extend(palette::build_bank(&palette_slots()));
    exe.extend(vec![0xcc; 1000]);
    write(dir, "MAIN.EXE", &exe);

    // SNR0: one scene, messages stored raw.
    let (m0, d0) = fixture_scene(&enc, lines[0], [lines[1], lines[2]]);
    write(dir, "SNR0M.R3", &build_sections(&[m0]));
    write(dir, "SNR0D.R3", &ls11::build(&[&d0]));
    // SNR1: two scenes, messages wrapped in a single-entry LS11 archive.
    let (m1, d1) = fixture_scene(&enc, lines[3], [lines[0], lines[1]]);
    let (m2, d2) = fixture_scene(&enc, lines[2], [lines[3], lines[0]]);
    write(dir, "SNR1M.R3", &ls11::build(&[&build_sections(&[m1, m2])]));
    write(dir, "SNR1D.R3", &ls11::build(&[&d1, &d2]));
    // IPPAN0 / IPPAN0M: one chapter, one town whose default group says both lines.
    let (index, pool) = ippan::build(&[(
        vec![vec![(125, vec![0, 1])]],
        vec![enc(lines[2]), enc(lines[3])],
    )]);
    write(dir, "IPPAN0.R3", &index);
    write(dir, "IPPAN0M.R3", &pool);
    write(
        dir,
        "BAKDATA.R3",
        &bakdata::build(
            encoding,
            &[
                (names[0], [91, 75, 64], 0, 1),
                (names[1], [100, 98, 80], 6, 1),
            ],
            &[(names[2], 255, 12, 0)],
        ),
    );

    // Unit sprites: a 4×4-cell sprite, a 3×3-cell sprite and a non-cell entry.
    let (s16, s9, other) = (cells(16), cells(9), vec![1u8; 100]);
    write(dir, "HEXBCHR.R3", &ls11::build(&[&s16, &s9, &other]));
    // Map chips: a sheet of 5 cells.
    write(dir, "HEXZCHP.R3", &ls11::build(&[&cells(5)]));
    // Portraits: three 64×80 TF-DCE images in the 6-byte table.
    let faces = [
        tfdce::fixture(8, 80, [0xFF, 0x00, 0xFF]),
        tfdce::fixture(8, 80, [0x0F, 0xF0, 0x00]),
        tfdce::fixture(8, 80, [0x00, 0x00, 0x00]),
    ];
    let faces: Vec<&[u8]> = faces.iter().map(Vec::as_slice).collect();
    write(
        dir,
        "FACEDAT.R3",
        &table6::build(&faces).expect("small entries"),
    );
}

const KOREAN_LINES: [&str; 4] = [
    "유비는 관우와 장비를 만나 도원에서 형제의 의를 맺었다.",
    "천하가 어지러우니 백성을 구할 영웅이 필요하다.",
    "적군이 다리를 건너오고 있습니다. 서둘러 진을 치십시오.",
    "승리하였다! 모두 수고하였다.",
];

const CHINESE_LINES: [&str; 4] = [
    "劉備與關羽張飛於桃園結義共圖大事。",
    "天下大亂百姓困苦須有英雄出而救之。",
    "敵軍正在渡橋請速布陣迎擊。",
    "我軍大勝眾將辛苦了。",
];

/// A synthetic Korean DOS/V install (identified by its disk header).
pub fn write_korean_install(dir: &Path) {
    let mut header = TextEncoding::EucKr
        .encode("DOS/V 삼국지영걸전 1 Ver 1.00 Rel 1.00")
        .expect("encodable");
    header.extend_from_slice(&[0x1a, 0, 0, 0]);
    write_dos_v_install(
        dir,
        TextEncoding::EucKr,
        &KOREAN_LINES,
        ["유비", "관우", "청룡언월도"],
        &header,
    );
}

/// A synthetic Traditional-Chinese DOS install (identified by its Big5 text).
pub fn write_chinese_install(dir: &Path) {
    write_dos_v_install(
        dir,
        TextEncoding::Big5,
        &CHINESE_LINES,
        ["劉備", "關羽", "青龍偃月刀"],
        b"DOS/V disk 1\x1a\0\0\0",
    );
}

/// A Korean install with every map archive: two battle maps (the second on chip set 2)
/// and their names, scene strips, one campaign map and one town and palace screen each.
pub fn write_map_install(dir: &Path) {
    write_korean_install(dir);
    let names: Vec<Vec<u8>> = maps::TERRAIN_IDS
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
    let refs: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
    let mut exe = maps::build_exe_fixture(
        &maps::ExeFixture {
            second_set_maps: &[1],
            backdrop: [0; maps::TERRAIN_COUNT],
            ground: [5; maps::TERRAIN_COUNT],
            terrain_names: &refs,
            campaign_sizes: [(16, 4); maps::CHAPTERS],
        },
        0,
    );
    exe.extend(palette::build_bank(&palette_slots()));
    std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
    let ls11 = |name: &str, entries: &[Vec<u8>]| {
        let refs: Vec<&[u8]> = entries.iter().map(Vec::as_slice).collect();
        std::fs::write(dir.join(name), ls11::build(&refs)).unwrap();
    };
    ls11("HEXZCHP.R3", &[cells(80), cells(2), cells(3)]);
    let map = |chip: u8, terrain: u8| {
        maps::BattleMap {
            width: 4,
            height: 2,
            chips: vec![0, 1, chip, chip, 2, 3, chip, chip],
            terrain: vec![1, terrain],
        }
        .encode()
    };
    ls11(
        "HEXZMAP.R3",
        &[
            map(81, 8),
            map(82, 3),
            b"\xb0\xa1\n\xb0\xa2 1\r\n\r\n\x1a".to_vec(),
        ],
    );
    ls11("HEXBCHP.R3", &[cells(4)]);
    ls11("HEXBMAP.R3", &[vec![3; 230], vec![1; 528]]);
    ls11("MMAPBGPL.R3", &[cells(2)]);
    let mut campaign = vec![1u8; 64];
    campaign.extend([0x7f, 0xff]);
    ls11("MMAP.R3", &[campaign]);
    ls11("SMAPBGPL.R3", &[cells(2), cells(3)]);
    let mut town = vec![1u8; 640];
    town.extend(vec![maps::WALK_BLOCKED; 620]);
    town[640 + 32] = maps::WALK_OPEN;
    town[640 + 33] = 9;
    town.extend([1, 7, 2, 3]);
    ls11("SMAP.R3", std::slice::from_ref(&town));
    ls11("PMAP.R3", &[town]);
}
