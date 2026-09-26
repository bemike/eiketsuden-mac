//! P0: a shareable manifest of a folder.
//!
//! The manifest lists, for every file under the folder: its path relative to the folder, size,
//! SHA-256, the first 16 bytes (magic numbers) and, where recognised, a structural summary —
//! LS11 directories (entry count, stored/decoded lengths, whether every entry decodes), 6-byte
//! tables (entry count and lengths), PC-98 disk image headers and the offset of the palette bank
//! in `MAIN.EXE`. It contains **no game content** (no decoded data, no text, no pixels), so an
//! owner can share it to help support builds we do not have (for example the Steam release).
//!
//! The probe is read-only, never follows symbolic links, and stops after [`MAX_FILES`] files or
//! [`MAX_DEPTH`] folder levels (reported in the manifest). Absolute paths are never recorded.

use crate::diskimage::{self, DiskImage};
use crate::edition::{identify, Edition};
use crate::install::{InstallDir, InstallError, MAX_READ};
use crate::{ls11, palette, table6};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Files recorded at most.
pub const MAX_FILES: usize = 5000;
/// Folder levels below the probed folder that are visited.
pub const MAX_DEPTH: usize = 8;
/// Bytes of each file shown in the manifest.
pub const HEAD_BYTES: usize = 16;
/// Manifest format identifier.
pub const FORMAT: &str = "eiketsuden-original-probe";
/// Manifest format version.
pub const FORMAT_VERSION: u32 = 1;

/// The manifest (serialised as JSON).
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub tool: String,
    pub edition: Edition,
    pub summary: Summary,
    pub files: Vec<FileRecord>,
    /// Paths that were not recorded, with the reason.
    pub skipped: Vec<Skipped>,
    /// `true` when [`MAX_FILES`] was reached.
    pub truncated: bool,
}

/// Totals over [`Manifest::files`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub files: usize,
    pub total_bytes: u64,
    pub ls11_archives: usize,
    /// LS11 archives whose directory or entries failed validation.
    pub ls11_failures: usize,
    pub table6_containers: usize,
    pub disk_images: usize,
}

/// One file.
#[derive(Debug, Clone, Serialize)]
pub struct FileRecord {
    /// Relative to the probed folder, `/`-separated.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// First [`HEAD_BYTES`] bytes as hex.
    pub head: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ls11: Option<Ls11Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table6: Option<Table6Summary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_image: Option<DiskImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette_bank: Option<PaletteProbe>,
    /// Why a file that looked like a known container failed validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Structure of an LS11 archive (or of a file that carries an LS container magic).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Ls11Summary {
    /// The first four bytes as text (`LS11`, or a variant such as `Ls12`).
    pub magic: String,
    /// Why the header or directory failed validation (the other fields are then empty).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub entries: usize,
    pub data_start: usize,
    pub dictionary_is_permutation: bool,
    /// `[stored length, decoded length]` per entry.
    pub lengths: Vec<[u32; 2]>,
    /// Entries that decoded to exactly their declared length with the input fully consumed.
    pub decoded: usize,
    /// The first decoding error, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decode_error: Option<String>,
}

/// Structure of a 6-byte-table container.
#[derive(Debug, Clone, Serialize)]
pub struct Table6Summary {
    pub entries: usize,
    pub data_start: u32,
    pub lengths: Vec<u16>,
}

/// Where the palette bank of `MAIN.EXE` was found.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteProbe {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A path left out of the manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}

/// Probe `dir`. Fails only when the folder itself cannot be read; problems with single files
/// are recorded in [`Manifest::skipped`].
pub fn probe(dir: &Path) -> Result<Manifest, InstallError> {
    let install = InstallDir::open(dir)?;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut truncated = false;
    walk(dir, "", 0, &mut files, &mut skipped, &mut truncated)?;

    let mut summary = Summary {
        files: files.len(),
        ..Summary::default()
    };
    for f in &files {
        summary.total_bytes += f.size;
        if let Some(l) = &f.ls11 {
            summary.ls11_archives += 1;
            if l.error.is_some() || l.decode_error.is_some() {
                summary.ls11_failures += 1;
            }
        }
        summary.table6_containers += usize::from(f.table6.is_some());
        summary.disk_images += usize::from(f.disk_image.is_some());
    }
    Ok(Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        tool: crate::tool_version(),
        edition: identify(&install),
        summary,
        files,
        skipped,
        truncated,
    })
}

fn walk(
    dir: &Path,
    rel: &str,
    depth: usize,
    files: &mut Vec<FileRecord>,
    skipped: &mut Vec<Skipped>,
    truncated: &mut bool,
) -> Result<(), InstallError> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let listing = std::fs::read_dir(dir).and_then(|list| {
        list.map(|entry| entry.map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())))
            .collect::<std::io::Result<Vec<_>>>()
    });
    match listing {
        Ok(list) => entries.extend(list),
        // The probed folder itself must be readable; an unreadable sub-folder is recorded.
        Err(e) if !rel.is_empty() => {
            skipped.push(Skipped {
                path: rel.to_string(),
                reason: e.to_string(),
            });
            return Ok(());
        }
        Err(e) => return Err(InstallError::io(dir, &e)),
    }
    entries.sort();
    for (name, path) in entries {
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                skipped.push(Skipped {
                    path: rel_path,
                    reason: e.to_string(),
                });
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            skipped.push(Skipped {
                path: rel_path,
                reason: "symbolic link (not followed)".into(),
            });
        } else if meta.is_dir() {
            if depth + 1 > MAX_DEPTH {
                skipped.push(Skipped {
                    path: rel_path,
                    reason: format!("deeper than {MAX_DEPTH} levels"),
                });
            } else {
                walk(&path, &rel_path, depth + 1, files, skipped, truncated)?;
            }
        } else if meta.is_file() {
            if files.len() >= MAX_FILES {
                *truncated = true;
                return Ok(());
            }
            match record(&path, &name, rel_path.clone(), meta.len()) {
                Ok(r) => files.push(r),
                Err(e) => skipped.push(Skipped {
                    path: rel_path,
                    reason: e.message,
                }),
            }
        }
        if *truncated {
            return Ok(());
        }
    }
    Ok(())
}

fn record(path: &Path, name: &str, rel: String, size: u64) -> Result<FileRecord, InstallError> {
    let upper = name.to_uppercase();
    let ext = upper
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    let mut file = std::fs::File::open(path).map_err(|e| InstallError::io(path, &e))?;
    let io = |e: std::io::Error| InstallError::io(path, &e);

    // Small files are analysed in memory; large ones (hard disk images) are only hashed.
    let (sha256, head, whole) = if size <= MAX_READ {
        let mut data = Vec::with_capacity(size as usize);
        file.read_to_end(&mut data).map_err(io)?;
        let head = data[..data.len().min(diskimage::HEAD_LEN)].to_vec();
        (crate::sha256_hex(&data), head, Some(data))
    } else {
        let mut hasher = Sha256::new();
        let mut head = Vec::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf).map_err(io)?;
            if n == 0 {
                break;
            }
            if head.len() < diskimage::HEAD_LEN {
                let take = (diskimage::HEAD_LEN - head.len()).min(n);
                head.extend_from_slice(&buf[..take]);
            }
            hasher.update(&buf[..n]);
        }
        (crate::hex(&hasher.finalize()), head, None)
    };

    let mut rec = FileRecord {
        path: rel,
        size,
        sha256,
        head: crate::hex(&head[..head.len().min(HEAD_BYTES)]),
        ls11: None,
        table6: None,
        disk_image: diskimage::detect(&head, size, &ext),
        palette_bank: None,
        error: None,
    };
    if let Some(data) = whole {
        if ls11::has_magic(&data) || ls11::has_variant_magic(&data) {
            rec.ls11 = Some(ls11_summary(&data));
        } else if ext == "r3" {
            match table6::Table6::parse(&data) {
                Ok(t) => {
                    rec.table6 = Some(Table6Summary {
                        entries: t.len(),
                        data_start: t.data_start() as u32,
                        lengths: t.entries().iter().map(|e| e.len).collect(),
                    })
                }
                // Most non-LS11 .R3 files are other formats; only report the documented
                // table containers.
                Err(e) if matches!(upper.as_str(), "FACEDAT.R3" | "PACKGRP.R3") => {
                    rec.error = Some(e.to_string())
                }
                Err(_) => {}
            }
        }
        if upper == "MAIN.EXE" {
            rec.palette_bank = Some(match palette::find_bank(&data) {
                Ok(bank) => PaletteProbe {
                    offset: Some(format!("{:#x}", bank.offset)),
                    error: None,
                },
                Err(e) => PaletteProbe {
                    offset: None,
                    error: Some(e.to_string()),
                },
            });
        }
    }
    Ok(rec)
}

fn ls11_summary(data: &[u8]) -> Ls11Summary {
    let magic = String::from_utf8_lossy(&data[..4]).into_owned();
    let archive = match ls11::Archive::parse(data) {
        Ok(a) => a,
        Err(e) => {
            return Ls11Summary {
                magic,
                error: Some(e.to_string()),
                ..Ls11Summary::default()
            }
        }
    };
    let mut decoded = 0;
    let mut decode_error = None;
    for i in 0..archive.len() {
        match archive.decode(i) {
            Ok(_) => decoded += 1,
            Err(e) => {
                decode_error.get_or_insert_with(|| e.to_string());
            }
        }
    }
    Ls11Summary {
        magic,
        error: None,
        entries: archive.len(),
        data_start: archive.data_start(),
        dictionary_is_permutation: archive.dictionary_is_permutation(),
        lengths: archive.entries().iter().map(|e| [e.clen, e.dlen]).collect(),
        decoded,
        decode_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edition::EditionId;
    use crate::testutil::{self, TempDir};

    #[test]
    fn manifest_of_a_synthetic_korean_install() {
        let dir = TempDir::new("probe-ko");
        testutil::write_korean_install(dir.path());
        std::fs::create_dir(dir.path().join("extra")).unwrap();
        std::fs::write(dir.path().join("extra").join("readme.txt"), b"hello").unwrap();

        let m = probe(dir.path()).unwrap();
        assert_eq!(m.format, FORMAT);
        assert_eq!(m.edition.id, EditionId::KoreanDos);
        assert!(!m.truncated);
        let file = |p: &str| m.files.iter().find(|f| f.path == p).unwrap();

        let hexbchr = file("HEXBCHR.R3");
        let l = hexbchr.ls11.as_ref().unwrap();
        assert_eq!((l.entries, l.decoded), (3, 3));
        assert_eq!(l.data_start, 0x110 + 3 * 12 + 4);
        assert_eq!(l.lengths[0][1], 2048);
        assert!(l.decode_error.is_none());
        assert_eq!(&hexbchr.head[..8], "4c533131"); // "LS11"

        let face = file("FACEDAT.R3").table6.as_ref().unwrap();
        assert_eq!(face.entries, 3);
        assert_eq!(face.data_start, 18);

        let exe = file("MAIN.EXE");
        assert_eq!(
            exe.palette_bank.as_ref().unwrap().offset.as_deref(),
            Some("0xbba")
        );
        assert_eq!(file("extra/readme.txt").size, 5);
        assert_eq!(file("extra/readme.txt").sha256, crate::sha256_hex(b"hello"));
        // SNR0D, SNR1M, SNR1D, HEXBCHR, HEXZCHP (BAKDATA.R3 is stored raw, as in the real copy).
        assert_eq!(m.summary.ls11_archives, 5);
        assert_eq!(m.summary.ls11_failures, 0);
        assert_eq!(m.summary.table6_containers, 1);

        // The manifest carries no content: no decoded text or pixels, only the listed fields.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("유비"));
        assert!(!json.contains(dir.path().to_str().unwrap()));
    }

    #[test]
    fn corrupt_containers_are_reported_not_fatal() {
        let dir = TempDir::new("probe-bad");
        let mut ls = ls11::build(&[b"some data some data some data"]);
        ls.push(7); // trailing byte
        std::fs::write(dir.path().join("BROKEN.R3"), &ls).unwrap();
        std::fs::write(dir.path().join("FACEDAT.R3"), [7, 0, 0, 0, 0, 0, 1]).unwrap();
        std::fs::write(dir.path().join("OTHER.R3"), [7, 0, 0, 0, 0, 0, 1]).unwrap();

        let m = probe(dir.path()).unwrap();
        let file = |p: &str| m.files.iter().find(|f| f.path == p).unwrap();
        let broken = file("BROKEN.R3").ls11.as_ref().unwrap();
        assert_eq!(broken.magic, "LS11");
        assert!(broken.error.as_ref().unwrap().contains("entries end"));
        assert!(file("FACEDAT.R3").error.is_some());
        assert!(file("OTHER.R3").error.is_none());
        assert_eq!(m.summary.ls11_failures, 1);
        assert_eq!(m.edition.id, EditionId::Unknown);
    }

    #[test]
    fn missing_folder_is_an_error() {
        let dir = TempDir::new("probe-missing");
        assert!(probe(&dir.path().join("nope")).is_err());
    }
}
