//! Which release of the original game a folder holds.
//!
//! Identification follows the research notes and never guesses silently: every verdict lists
//! its evidence, and a folder that matches no rule is reported as [`EditionId::Unknown`].
//!
//! | edition | rule | confidence |
//! |---|---|---|
//! | Steam 2017 (シブサワ・コウ アーカイブス) | `Eiketsuden1_Launcher.exe` present | high |
//! | Korean DOS/V (Visco) | `DISK1.R3I` contains the header text `DOS/V 삼국지영걸전` (EUC-KR) | high |
//! | Traditional-Chinese DOS (第三波) | DOS/V file family and clearly Big5 text in `SNRnM` / `IPPAN0M` / `BAKDATA` | medium |
//! | PC-98 disk images | a D88 / FDI / HDI header on a file in the folder | high (content not read) |
//!
//! Only the files directly inside the folder are considered.

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
/// Header text of the Korean build's disk identifier.
pub const KOREAN_DISK_MARKER: &str = "DOS/V 삼국지영걸전";
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
            "chosen with --edition; identification said `{}` ({:?} confidence)",
            identified.id.as_str(),
            identified.confidence
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
    let marker = TextEncoding::EucKr
        .encode(KOREAN_DISK_MARKER)
        .expect("the marker is EUC-KR text");
    if let Ok(Some(disk)) = install.read(DISK_ID_FILE) {
        if disk.windows(marker.len()).any(|w| w == marker.as_slice()) {
            return Edition::new(
                EditionId::KoreanDos,
                Confidence::High,
                vec![format!(
                    "{DISK_ID_FILE} contains the header text \"{KOREAN_DISK_MARKER}\" (EUC-KR)"
                )],
            );
        }
        evidence.push(format!(
            "{DISK_ID_FILE} present but without the Korean header text"
        ));
    }

    let dos_v = install.has("MAIN.EXE") && install.has("SNR0M.R3");
    if dos_v {
        evidence.push("DOS/V file family present (MAIN.EXE, SNR0M.R3)".into());
        let mut ev = EncodingEvidence::default();
        let mut scanned = Vec::new();
        for name in TEXT_FILES {
            if let Ok(Some(data)) = install.read(name) {
                ev.add(EncodingEvidence::scan(&text_bytes(&data)));
                scanned.push(name);
            }
        }
        evidence.push(format!(
            "text in {}: {} EUC-KR/Big5 pairs, {} Big5-only pairs",
            scanned.join(", "),
            ev.high_pairs,
            ev.big5_pairs
        ));
        match ev.verdict() {
            Some(TextEncoding::Big5) => {
                return Edition::new(EditionId::ChineseDos, Confidence::Medium, evidence);
            }
            Some(TextEncoding::EucKr) => evidence.push(
                "text looks like EUC-KR, but the Korean disk header is missing: \
                 not identified (use --edition korean-dos to try anyway)"
                    .into(),
            ),
            None => evidence.push("text encoding inconclusive".into()),
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
