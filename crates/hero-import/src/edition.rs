//! Which release of the original game a folder holds.
//!
//! Identification follows the research notes and never guesses silently: every verdict lists
//! its evidence, and a folder that matches no rule is reported as [`EditionId::Unknown`].
//!
//! | edition | rule | confidence |
//! |---|---|---|
//! | Steam 2017 (シブサワ・コウ アーカイブス) | `Eiketsuden1_Launcher.exe` present | high |
//! | Korean DOS/V (Visco) | `DISK1.R3I` carries the Japanese DOS/V disk header (Shift-JIS `DOS/V 三國志英傑伝`), the DOS/V file family is present and the scenario text is Korean (EUC-KR, mostly Hangul) | high |
//! | Korean DOS/V (Visco), alternative | `DISK1.R3I` contains `DOS/V 삼국지영걸전` in EUC-KR (claimed by the research notes, not seen on a real copy) | high |
//! | Traditional-Chinese DOS (第三波) | DOS/V file family and clearly Big5 text in `SNRnM` / `IPPAN0M` / `BAKDATA` | medium |
//! | PC-98 disk images | a D88 / FDI / HDI header on a file in the folder | high (content not read) |
//!
//! Only the files directly inside the folder are considered.
//!
//! The Korean release is a localisation of the Japanese DOS/V release: on the verified copy the
//! disk identifier still holds the Japanese header (`DOS/V 三國志英傑伝 ﾃﾞｨｽｸ1 Ver 1.00 Rel 1.00`,
//! Shift-JIS, `(C)(P) 1995 KOEI`), while every message file is EUC-KR Hangul. The header alone
//! therefore does not decide the language; the text does. A folder with the Japanese header and
//! non-Korean text (a Japanese DOS/V copy, which we have never seen) stays `unknown`.

use crate::diskimage;
use crate::install::InstallDir;
use crate::ls11;
use crate::text::{EncodingEvidence, TextEncoding};
use serde::Serialize;
use std::io::Read;

/// Launcher of the Steam release (app 628150).
pub const STEAM_LAUNCHER: &str = "Eiketsuden1_Launcher.exe";
/// Disk identifier file of the DOS/V builds.
pub const DISK_ID_FILE: &str = "DISK1.R3I";
/// Header text of the Korean build's disk identifier as the research notes describe it
/// (EUC-KR). Kept as an alternative rule; the verified copy carries [`DOS_V_DISK_MARKER`].
pub const KOREAN_DISK_MARKER: &str = "DOS/V 삼국지영걸전";
/// Header text of the DOS/V disk identifier as found on the verified Korean copy (Shift-JIS,
/// inherited from the Japanese DOS/V release).
pub const DOS_V_DISK_MARKER: &str = "DOS/V 三國志英傑伝";
/// Hangul syllables needed before the text counts as Korean.
const MIN_HANGUL: usize = 500;
/// Files whose text decides between EUC-KR and Big5.
const TEXT_FILES: [&str; 7] = [
    "SNR0M.R3",
    "SNR1M.R3",
    "SNR2M.R3",
    "SNR3M.R3",
    "SNR4M.R3",
    "IPPAN0M.R3",
    "BAKDATA.R3",
];

/// A known release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EditionId {
    /// Steam 2017: KOEI's PC-98 emulator; container format not known yet.
    #[serde(rename = "steam-2017")]
    Steam2017,
    /// Korean DOS/V (삼국지 영걸전, Visco).
    #[serde(rename = "korean-dos")]
    KoreanDos,
    /// Traditional-Chinese DOS (三國志英傑傳, 第三波).
    #[serde(rename = "chinese-dos")]
    ChineseDos,
    /// PC-98 floppy or hard-disk images.
    #[serde(rename = "pc98-disk-images")]
    Pc98Images,
    #[serde(rename = "unknown")]
    Unknown,
}

impl EditionId {
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            EditionId::Steam2017 => "Steam 2017 (シブサワ・コウ アーカイブス, PC-98 emulation)",
            EditionId::KoreanDos => "Korean DOS/V (삼국지 영걸전)",
            EditionId::ChineseDos => "Traditional-Chinese DOS (三國志英傑傳)",
            EditionId::Pc98Images => "PC-98 disk images",
            EditionId::Unknown => "unknown edition",
        }
    }

    /// The id as written in manifests and accepted by `--edition`.
    pub fn as_str(self) -> &'static str {
        match self {
            EditionId::Steam2017 => "steam-2017",
            EditionId::KoreanDos => "korean-dos",
            EditionId::ChineseDos => "chinese-dos",
            EditionId::Pc98Images => "pc98-disk-images",
            EditionId::Unknown => "unknown",
        }
    }

    /// Parse an id written by [`EditionId::as_str`].
    pub fn parse(s: &str) -> Option<EditionId> {
        [
            EditionId::Steam2017,
            EditionId::KoreanDos,
            EditionId::ChineseDos,
            EditionId::Pc98Images,
            EditionId::Unknown,
        ]
        .into_iter()
        .find(|e| e.as_str() == s)
    }

    /// Text encoding of the edition's data files, for the editions whose data can be extracted.
    pub fn text_encoding(self) -> Option<TextEncoding> {
        match self {
            EditionId::KoreanDos => Some(TextEncoding::EucKr),
            EditionId::ChineseDos => Some(TextEncoding::Big5),
            _ => None,
        }
    }

    /// Whether [`crate::extract`] can convert this edition's files (the DOS/V family).
    pub fn is_extractable(self) -> bool {
        self.text_encoding().is_some()
    }
}

/// How sure an identification is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    None,
}

impl Confidence {
    /// The word written in manifests.
    pub fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::None => "none",
        }
    }
}

/// An identification with its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edition {
    pub id: EditionId,
    pub name: String,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
    /// Chosen by the user (`--edition`) instead of identified.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub forced: bool,
}

impl Edition {
    fn new(id: EditionId, confidence: Confidence, evidence: Vec<String>) -> Edition {
        Edition {
            id,
            name: id.name().to_string(),
            confidence,
            evidence,
            forced: false,
        }
    }

    /// An edition chosen by the user, keeping what identification found as evidence.
    pub fn forced(id: EditionId, identified: &Edition) -> Edition {
        let mut evidence = vec![format!(
            "chosen with --edition; identification said `{}` ({} confidence)",
            identified.id.as_str(),
            identified.confidence.as_str()
        )];
        evidence.extend(identified.evidence.iter().cloned());
        Edition {
            forced: true,
            ..Edition::new(id, Confidence::None, evidence)
        }
    }
}

/// Bytes of `data` to scan for text evidence: every decodable entry of an LS11 archive, or the
/// file itself.
fn text_bytes(data: &[u8]) -> Vec<u8> {
    match ls11::Archive::parse(data) {
        Ok(archive) => (0..archive.len())
            .filter_map(|i| archive.decode(i).ok())
            .flatten()
            .collect(),
        Err(_) => data.to_vec(),
    }
}

/// Double-byte pairs of `bytes` read as strict EUC-KR (KS X 1001): pairs in the Hangul block
/// (lead 0xB0–0xC8, trail 0xA1–0xFE) and all pairs with a lead byte of 0x81 or above.
fn hangul_counts(bytes: &[u8]) -> (usize, usize) {
    let (mut hangul, mut pairs) = (0, 0);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let (lead, trail) = (bytes[i], bytes[i + 1]);
        if lead >= 0x81 && lead != 0xff {
            pairs += 1;
            if (0xb0..=0xc8).contains(&lead) && (0xa1..=0xfe).contains(&trail) {
                hangul += 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    (hangul, pairs)
}

/// Whether the pair counts show Korean text: enough Hangul syllables, and at least four in five
/// double-byte pairs in the Hangul block (the verified Korean text has about 96 %; Big5 or
/// Shift-JIS text spreads its lead bytes over the whole high range).
fn is_korean_text(hangul: usize, pairs: usize) -> bool {
    hangul >= MIN_HANGUL && hangul * 5 >= pairs * 4
}

/// Top-level files that carry a PC-98 disk image header.
fn disk_images(install: &InstallDir) -> Vec<String> {
    let mut found = Vec::new();
    for name in install.names() {
        let Some(path) = install.path(name) else {
            continue;
        };
        let Ok(mut file) = std::fs::File::open(path) else {
            continue;
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            continue;
        };
        let mut head = Vec::with_capacity(diskimage::HEAD_LEN);
        if file
            .by_ref()
            .take(diskimage::HEAD_LEN as u64)
            .read_to_end(&mut head)
            .is_err()
        {
            continue;
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if let Some(img) = diskimage::detect(&head, len, &ext.to_lowercase()) {
            found.push(format!("{name}: {:?} image ({})", img.kind, img.detail));
        }
    }
    found
}

/// Identify the edition in `install`. Unreadable files count as missing evidence.
pub fn identify(install: &InstallDir) -> Edition {
    if install.has(STEAM_LAUNCHER) {
        return Edition::new(
            EditionId::Steam2017,
            Confidence::High,
            vec![format!(
                "{STEAM_LAUNCHER} present (launcher of Steam app 628150)"
            )],
        );
    }

    let mut evidence = Vec::new();
    let korean_marker = TextEncoding::EucKr
        .encode(KOREAN_DISK_MARKER)
        .expect("the marker is EUC-KR text");
    let (dos_v_marker, _, unmappable) = encoding_rs::SHIFT_JIS.encode(DOS_V_DISK_MARKER);
    debug_assert!(!unmappable, "the marker is Shift-JIS text");
    let contains = |data: &[u8], m: &[u8]| data.windows(m.len()).any(|w| w == m);
    let mut dos_v_header = false;
    if let Ok(Some(disk)) = install.read(DISK_ID_FILE) {
        if contains(&disk, &korean_marker) {
            return Edition::new(
                EditionId::KoreanDos,
                Confidence::High,
                vec![format!(
                    "{DISK_ID_FILE} contains the header text \"{KOREAN_DISK_MARKER}\" (EUC-KR)"
                )],
            );
        }
        if contains(&disk, &dos_v_marker) {
            dos_v_header = true;
            evidence.push(format!(
                "{DISK_ID_FILE} carries the Japanese DOS/V disk header \"{DOS_V_DISK_MARKER}\" \
                 (Shift-JIS)"
            ));
        } else {
            evidence.push(format!(
                "{DISK_ID_FILE} present but without a known DOS/V header text"
            ));
        }
    }

    let dos_v = install.has("MAIN.EXE") && install.has("SNR0M.R3");
    if dos_v {
        evidence.push("DOS/V file family present (MAIN.EXE, SNR0M.R3)".into());
        let mut ev = EncodingEvidence::default();
        let (mut hangul, mut pairs) = (0, 0);
        let mut scanned = Vec::new();
        for name in TEXT_FILES {
            if let Ok(Some(data)) = install.read(name) {
                let bytes = text_bytes(&data);
                ev.add(EncodingEvidence::scan(&bytes));
                let (h, n) = hangul_counts(&bytes);
                hangul += h;
                pairs += n;
                scanned.push(name);
            }
        }
        evidence.push(format!(
            "text in {}: {} EUC-KR/Big5 pairs, {} Big5-only pairs; {hangul} of {pairs} \
             double-byte pairs in the EUC-KR Hangul block",
            scanned.join(", "),
            ev.high_pairs,
            ev.big5_pairs
        ));
        let verdict = ev.verdict();
        let korean = verdict == Some(TextEncoding::EucKr) && is_korean_text(hangul, pairs);
        match verdict {
            Some(TextEncoding::Big5) if !dos_v_header => {
                return Edition::new(EditionId::ChineseDos, Confidence::Medium, evidence);
            }
            _ if korean && dos_v_header => {
                evidence.push(
                    "Korean text on the Japanese DOS/V disk set: the Korean localisation".into(),
                );
                return Edition::new(EditionId::KoreanDos, Confidence::High, evidence);
            }
            _ if dos_v_header => evidence.push(
                "Japanese DOS/V disk header without enough Korean text: a build we have not \
                 verified (not identified)"
                    .into(),
            ),
            Some(TextEncoding::EucKr) => evidence.push(
                "text looks like EUC-KR, but no known disk header was found: \
                 not identified (use --edition korean-dos to try anyway)"
                    .into(),
            ),
            _ => evidence.push("text encoding inconclusive".into()),
        }
    }

    let images = disk_images(install);
    if !images.is_empty() {
        return Edition::new(EditionId::Pc98Images, Confidence::High, images);
    }
    if evidence.is_empty() {
        evidence.push("no known file of any edition in this folder".into());
        if std::fs::metadata(install.root().join("GAME")).is_ok_and(|m| m.is_dir()) {
            evidence.push("there is a GAME sub-folder: the DOS/V data usually lives there".into());
        }
    }
    Edition::new(EditionId::Unknown, Confidence::None, evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{self, TempDir};

    fn identify_dir(dir: &TempDir) -> Edition {
        identify(&InstallDir::open(dir.path()).unwrap())
    }

    #[test]
    fn korean_by_disk_header() {
        let dir = TempDir::new("ed-ko");
        testutil::write_korean_install(dir.path());
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::KoreanDos);
        assert_eq!(e.confidence, Confidence::High);
        assert_eq!(e.id.text_encoding(), Some(TextEncoding::EucKr));
    }

    #[test]
    fn chinese_by_big5_text() {
        let dir = TempDir::new("ed-zh");
        testutil::write_chinese_install(dir.path());
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::ChineseDos, "{e:?}");
        assert_eq!(e.confidence, Confidence::Medium);
    }

    /// The verified layout: Japanese Shift-JIS disk header, Korean EUC-KR text.
    fn japanese_header() -> Vec<u8> {
        let mut h = encoding_rs::SHIFT_JIS
            .encode("DOS/V 三國志英傑伝 ﾃﾞｨｽｸ1 Ver 1.00 Rel 1.00\r\n")
            .0
            .into_owned();
        h.extend_from_slice(b"(C) 1995 KOEI CO.,LTD\r\n\x1a");
        h
    }

    /// Enough Korean text to pass the Hangul threshold.
    fn long_korean_text(dir: &TempDir) {
        let line = TextEncoding::EucKr
            .encode("유비는 관우와 장비를 만나 도원에서 형제의 의를 맺었다.")
            .unwrap();
        let blocks: Vec<Vec<u8>> = vec![line; 40];
        std::fs::write(
            dir.path().join("IPPAN0M.R3"),
            crate::text::build_messages(&[blocks]),
        )
        .unwrap();
    }

    #[test]
    fn korean_by_japanese_header_and_korean_text() {
        let dir = TempDir::new("ed-ko-sjis");
        testutil::write_korean_install(dir.path());
        std::fs::write(dir.path().join("DISK1.R3I"), japanese_header()).unwrap();
        long_korean_text(&dir);
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::KoreanDos, "{e:?}");
        assert_eq!(e.confidence, Confidence::High);
        assert!(e.evidence.iter().any(|l| l.contains("Shift-JIS")), "{e:?}");
        assert!(e.evidence.iter().any(|l| l.contains("Hangul")), "{e:?}");
    }

    #[test]
    fn japanese_header_with_other_text_is_unknown() {
        // Big5 text under the Japanese header: neither Korean nor a known Chinese copy.
        let dir = TempDir::new("ed-sjis-big5");
        testutil::write_chinese_install(dir.path());
        std::fs::write(dir.path().join("DISK1.R3I"), japanese_header()).unwrap();
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::Unknown, "{e:?}");
        assert!(
            e.evidence.iter().any(|l| l.contains("not verified")),
            "{e:?}"
        );
        // Too little Korean text under the Japanese header: also unknown.
        let dir = TempDir::new("ed-sjis-short");
        testutil::write_korean_install(dir.path());
        std::fs::write(dir.path().join("DISK1.R3I"), japanese_header()).unwrap();
        assert_eq!(identify_dir(&dir).id, EditionId::Unknown);
    }

    #[test]
    fn hangul_share() {
        assert!(is_korean_text(500, 625));
        assert!(!is_korean_text(499, 499));
        assert!(!is_korean_text(1000, 1300));
        let big5 = TextEncoding::Big5
            .encode("劉備與關羽張飛於桃園結義共圖大事天下大亂百姓困苦")
            .unwrap();
        let (h, n) = hangul_counts(&big5.repeat(20));
        assert!(!is_korean_text(h, n), "{h} of {n}");
    }

    #[test]
    fn korean_text_without_marker_is_unknown() {
        let dir = TempDir::new("ed-ko-nomarker");
        testutil::write_korean_install(dir.path());
        std::fs::write(dir.path().join("DISK1.R3I"), b"something else").unwrap();
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::Unknown);
        assert!(
            e.evidence
                .iter()
                .any(|l| l.contains("--edition korean-dos")),
            "{e:?}"
        );
    }

    #[test]
    fn steam_by_launcher() {
        let dir = TempDir::new("ed-steam");
        std::fs::write(dir.path().join("eiketsuden1_launcher.EXE"), b"MZ").unwrap();
        assert_eq!(identify_dir(&dir).id, EditionId::Steam2017);
    }

    #[test]
    fn pc98_by_image_header() {
        let dir = TempDir::new("ed-pc98");
        let len = 0x2b0u32 + 1024;
        let mut img = vec![0u8; len as usize];
        img[0x1b] = 0x20;
        img[0x1c..0x20].copy_from_slice(&len.to_le_bytes());
        std::fs::write(dir.path().join("disk1.d88"), img).unwrap();
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::Pc98Images);
        assert!(e.evidence[0].contains("DISK1.D88"), "{e:?}");
    }

    #[test]
    fn empty_folder_is_unknown_with_a_hint() {
        let dir = TempDir::new("ed-empty");
        std::fs::create_dir(dir.path().join("GAME")).unwrap();
        let e = identify_dir(&dir);
        assert_eq!(e.id, EditionId::Unknown);
        assert_eq!(e.confidence, Confidence::None);
        assert!(e.evidence.iter().any(|l| l.contains("GAME")), "{e:?}");
    }

    #[test]
    fn ids_round_trip_and_forcing_keeps_evidence() {
        for id in [
            EditionId::Steam2017,
            EditionId::KoreanDos,
            EditionId::ChineseDos,
            EditionId::Pc98Images,
            EditionId::Unknown,
        ] {
            assert_eq!(EditionId::parse(id.as_str()), Some(id));
            assert_eq!(
                serde_json::to_value(id).unwrap(),
                serde_json::Value::String(id.as_str().into())
            );
        }
        assert_eq!(EditionId::parse("dos"), None);
        let found = Edition::new(EditionId::Unknown, Confidence::None, vec!["x".into()]);
        let forced = Edition::forced(EditionId::KoreanDos, &found);
        assert!(forced.forced);
        assert_eq!(forced.id, EditionId::KoreanDos);
        assert!(forced.evidence.iter().any(|l| l == "x"));
        assert!(EditionId::KoreanDos.is_extractable());
        assert!(!EditionId::Steam2017.is_extractable());
    }
}
