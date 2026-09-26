//! Detection of PC-98 disk images by their headers (probe only: files inside are not read yet).
//!
//! * **D88** (floppy): a 0x2B0-byte header — 17-byte disk name, 9 reserved bytes, write-protect
//!   flag at 0x1A (0x00 or 0x10), media type at 0x1B (0x00 2D, 0x10 2DD, 0x20 2HD, 0x30 1D,
//!   0x40 1DD), little-endian total size at 0x1C that equals the file size, then a table of
//!   little-endian track offsets from 0x20.
//! * **Anex86 FDI / HDI** (floppy / hard disk): eight little-endian `u32` — 0, disk type, header
//!   size, data size, sector size, sectors per track, heads, cylinders — where header size +
//!   data size equals the file size and sector size × sectors × heads × cylinders equals the
//!   data size. Both use the same header; the file extension (or, without one, the geometry)
//!   tells them apart.
//!
//! Every check is a size/consistency check, so random files are practically never mistaken for
//! an image. Only [`HEAD_LEN`] bytes and the file length are needed.

use serde::Serialize;

/// Bytes of the file start needed for detection.
pub const HEAD_LEN: usize = 0x2b0;

/// Kind of disk image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskImageKind {
    D88,
    Fdi,
    Hdi,
}

/// A recognised disk image header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiskImage {
    pub kind: DiskImageKind,
    /// Media type (D88: "2D", "2DD", "2HD", "1D", "1DD") or geometry (FDI/HDI).
    pub detail: String,
}

fn le32(head: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]])
}

/// Recognise a disk image from the first bytes of a file (at least [`HEAD_LEN`] when the file
/// is that long), its total length and its extension (lower-case, without the dot).
pub fn detect(head: &[u8], file_len: u64, extension: &str) -> Option<DiskImage> {
    detect_d88(head, file_len).or_else(|| detect_anex86(head, file_len, extension))
}

fn detect_d88(head: &[u8], file_len: u64) -> Option<DiskImage> {
    if head.len() < 0x24 || file_len < 0x2b0 {
        return None;
    }
    let media = match head[0x1b] {
        0x00 => "2D",
        0x10 => "2DD",
        0x20 => "2HD",
        0x30 => "1D",
        0x40 => "1DD",
        _ => return None,
    };
    if !matches!(head[0x1a], 0x00 | 0x10) || u64::from(le32(head, 0x1c)) != file_len {
        return None;
    }
    // The first track normally starts right after the header (0x2B0, or 0x2A0 for images with
    // a 160-entry track table); any non-zero first offset must lie inside the file.
    let first_track = u64::from(le32(head, 0x20));
    if first_track != 0 && !(0x20..file_len).contains(&first_track) {
        return None;
    }
    Some(DiskImage {
        kind: DiskImageKind::D88,
        detail: media.into(),
    })
}

fn detect_anex86(head: &[u8], file_len: u64, extension: &str) -> Option<DiskImage> {
    if head.len() < 32 {
        return None;
    }
    let f: [u64; 8] = std::array::from_fn(|i| u64::from(le32(head, i * 4)));
    let [zero, _kind, header, data, sector, sectors, heads, cylinders] = f;
    let geometry = sector
        .checked_mul(sectors)
        .and_then(|v| v.checked_mul(heads))
        .and_then(|v| v.checked_mul(cylinders));
    if zero != 0
        || !matches!(sector, 128 | 256 | 512 | 1024 | 2048)
        || sectors == 0
        || heads == 0
        || cylinders == 0
        || header < 32
        || header.checked_add(data) != Some(file_len)
        || geometry != Some(data)
    {
        return None;
    }
    let kind = match extension {
        "fdi" => DiskImageKind::Fdi,
        "hdi" => DiskImageKind::Hdi,
        _ if cylinders <= 85 && heads <= 2 => DiskImageKind::Fdi,
        _ => DiskImageKind::Hdi,
    };
    Some(DiskImage {
        kind,
        detail: format!(
            "{cylinders} cylinders × {heads} heads × {sectors} sectors × {sector} bytes"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d88(media: u8, len: u32) -> Vec<u8> {
        let mut h = vec![0u8; HEAD_LEN];
        h[..6].copy_from_slice(b"SAMPLE");
        h[0x1b] = media;
        h[0x1c..0x20].copy_from_slice(&len.to_le_bytes());
        h[0x20..0x24].copy_from_slice(&0x2b0u32.to_le_bytes());
        h
    }

    fn anex86(fields: [u32; 8]) -> Vec<u8> {
        let mut h: Vec<u8> = fields.iter().flat_map(|v| v.to_le_bytes()).collect();
        h.resize(HEAD_LEN, 0);
        h
    }

    #[test]
    fn detects_d88() {
        let len = 1_261_568u32 + 0x2b0;
        let img = detect(&d88(0x20, len), u64::from(len), "d88").unwrap();
        assert_eq!(img.kind, DiskImageKind::D88);
        assert_eq!(img.detail, "2HD");
        // Size field disagreeing with the file.
        assert_eq!(detect(&d88(0x20, len), u64::from(len) + 1, "d88"), None);
        // Unknown media byte.
        assert_eq!(detect(&d88(0x50, len), u64::from(len), ""), None);
        // Bad write-protect flag.
        let mut h = d88(0x20, len);
        h[0x1a] = 7;
        assert_eq!(detect(&h, u64::from(len), ""), None);
    }

    #[test]
    fn detects_fdi_and_hdi() {
        // 2HD floppy: 77 cylinders × 2 heads × 8 sectors × 1024 bytes.
        let data = 77 * 2 * 8 * 1024u32;
        let h = anex86([0, 0x90, 0x1000, data, 1024, 8, 2, 77]);
        let len = u64::from(0x1000 + data);
        assert_eq!(detect(&h, len, "fdi").unwrap().kind, DiskImageKind::Fdi);
        assert_eq!(detect(&h, len, "").unwrap().kind, DiskImageKind::Fdi);
        // 40 MB hard disk.
        let data = 615 * 8 * 17 * 512u32;
        let h = anex86([0, 0, 0x1000, data, 512, 17, 8, 615]);
        let len = u64::from(0x1000 + data);
        let img = detect(&h, len, "").unwrap();
        assert_eq!(img.kind, DiskImageKind::Hdi);
        assert!(img.detail.contains("615 cylinders"));
        // Inconsistent geometry or size.
        let h = anex86([0, 0, 0x1000, data, 512, 17, 8, 614]);
        assert_eq!(detect(&h, len, "hdi"), None);
        let h = anex86([0, 0, 0x1000, data, 512, 17, 8, 615]);
        assert_eq!(detect(&h, len - 1, "hdi"), None);
    }

    #[test]
    fn ordinary_files_are_not_images() {
        assert_eq!(detect(b"MZ\x90\x00", 4, "exe"), None);
        assert_eq!(detect(&[0u8; HEAD_LEN], 0x10_000, ""), None);
        let mut ls11 = b"LS11".to_vec();
        ls11.resize(HEAD_LEN, 0);
        assert_eq!(detect(&ls11, 5000, "r3"), None);
        // Huge geometry values must not overflow.
        let h = anex86([0, 0, 32, u32::MAX, 2048, u32::MAX, u32::MAX, u32::MAX]);
        assert_eq!(detect(&h, 32 + u64::from(u32::MAX), "hdi"), None);
    }
}
