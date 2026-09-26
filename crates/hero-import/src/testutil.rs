//! Test helpers: temporary folders and synthetic installs built with this crate's own encoders.
//! No byte of the original game is involved; the text is made up.

use crate::image::IndexedImage;
use crate::text::{build_messages, TextEncoding};
use crate::{ls11, palette, planar, table6};
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

fn scene_bytecode(offsets: &[u16]) -> Vec<u8> {
    let mut out: Vec<u8> = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    out.extend_from_slice(&0xffffu16.to_le_bytes());
    out.extend(vec![0x2a; 64]);
    out
}

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(dir.join(name), bytes).expect("write fixture file");
}

/// Write a DOS/V-style install whose text uses `encoding`, with `lines` as dialogue.
fn write_dos_v_install(dir: &Path, encoding: TextEncoding, lines: &[&str], disk_header: &[u8]) {
    let enc = |s: &str| encoding.encode(s).expect("fixture text is encodable");
    write(dir, "DISK1.R3I", disk_header);

    let mut exe = b"MZ".to_vec();
    exe.extend(vec![0x90; 3000]);
    exe.extend(palette::build_bank(&palette_slots()));
    exe.extend(vec![0xcc; 1000]);
    write(dir, "MAIN.EXE", &exe);

    // SNR0M: one scene, raw; the second block imitates a speaker-id prefix (id 5, high byte 0).
    let scene0: Vec<Vec<u8>> = vec![enc(lines[0]), vec![5], enc(lines[1]), enc(lines[2])];
    write(dir, "SNR0M.R3", &build_messages(&[scene0]));
    write(
        dir,
        "SNR0D.R3",
        &ls11::build(&[&scene_bytecode(&[0x16, 0x41, 0x103])]),
    );
    // SNR1M: two scenes, wrapped in a single-entry LS11 archive.
    let snr1m = build_messages(&[
        vec![enc(lines[3]), enc(lines[0])],
        vec![enc(lines[1]), vec![], enc(lines[2])],
    ]);
    write(dir, "SNR1M.R3", &ls11::build(&[&snr1m]));
    let (a, b) = (scene_bytecode(&[4]), scene_bytecode(&[4, 9]));
    write(dir, "SNR1D.R3", &ls11::build(&[&a, &b]));
    write(
        dir,
        "IPPAN0M.R3",
        &build_messages(&[vec![enc(lines[2]), enc(lines[3])]]),
    );
    write(dir, "BAKDATA.R3", &ls11::build(&[&enc(lines[0])]));

    // Unit sprites: a 4×4-cell sprite, a 3×3-cell sprite and a non-cell entry.
    let (s16, s9, other) = (cells(16), cells(9), vec![1u8; 100]);
    write(dir, "HEXBCHR.R3", &ls11::build(&[&s16, &s9, &other]));
    // Map chips: a sheet of 5 cells.
    write(dir, "HEXZCHP.R3", &ls11::build(&[&cells(5)]));
    // Portraits: three opaque payloads in the 6-byte table.
    write(
        dir,
        "FACEDAT.R3",
        &table6::build(&[b"face-0", b"face-1", b""]).expect("small entries"),
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
    write_dos_v_install(dir, TextEncoding::EucKr, &KOREAN_LINES, &header);
}

/// A synthetic Traditional-Chinese DOS install (identified by its Big5 text).
pub fn write_chinese_install(dir: &Path) {
    write_dos_v_install(
        dir,
        TextEncoding::Big5,
        &CHINESE_LINES,
        b"DOS/V disk 1\x1a\0\0\0",
    );
}
