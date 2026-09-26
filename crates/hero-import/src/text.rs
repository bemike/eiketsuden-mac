//! Message files (`SNR0M.R3`–`SNR4M.R3`, `IPPAN0M.R3`) and the legacy text encodings.
//!
//! A message file starts with a table of `N` little-endian `u16` section bases (the first base
//! is therefore `2 × N`: the table is followed directly by section 0), then NUL-terminated
//! strings. The scenario bytecode addresses text by byte offset relative to its section's base,
//! so every block is reported with that relative offset. For the `SNRnM` files `N` is the number
//! of scenes of the matching `SNRnD.R3` archive (1/5/4/5/3 in the Korean build).
//!
//! Dialogue records carry a `u16le` speaker id in front of their text. Where a record starts is
//! defined by the bytecode (not decoded yet), so blocks are split at NUL bytes only and speaker
//! prefixes stay inside the block (as control characters, or as a separate short block when the
//! id's high byte is zero). Blocks that do not decode cleanly keep their raw bytes as hex.

use serde::Serialize;
use std::fmt;

/// Legacy text encodings of the supported editions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TextEncoding {
    /// Korean DOS/V: EUC-KR (decoded as its Windows superset, code page 949).
    #[serde(rename = "EUC-KR")]
    EucKr,
    /// Traditional-Chinese DOS: Big5.
    #[serde(rename = "Big5")]
    Big5,
}

impl TextEncoding {
    fn codec(self) -> &'static encoding_rs::Encoding {
        match self {
            TextEncoding::EucKr => encoding_rs::EUC_KR,
            TextEncoding::Big5 => encoding_rs::BIG5,
        }
    }

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            TextEncoding::EucKr => "EUC-KR (cp949)",
            TextEncoding::Big5 => "Big5",
        }
    }

    /// Decode bytes; `malformed` is set when a byte sequence is invalid in this encoding (it
    /// then appears as U+FFFD in the text).
    pub fn decode(self, bytes: &[u8]) -> Decoded {
        let (text, malformed) = self.codec().decode_without_bom_handling(bytes);
        Decoded {
            text: text.into_owned(),
            malformed,
        }
    }

    /// Encode text (for fixtures); `None` if a character is not representable.
    pub fn encode(self, text: &str) -> Option<Vec<u8>> {
        let (bytes, _, unmappable) = self.codec().encode(text);
        (!unmappable).then(|| bytes.into_owned())
    }
}

/// Decoded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    pub malformed: bool,
}

/// A structural problem in a message file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    /// Shorter than one table entry.
    TooShort { len: usize },
    /// The first base is not `2 × N` for some `N ≥ 1` inside the file.
    BadTable { first: u16, len: usize },
    /// A base lies before the end of the table, before the previous base, or past the end.
    BadBase {
        section: usize,
        base: u16,
        min: usize,
        max: usize,
    },
    /// A section does not end with a NUL byte.
    Unterminated { section: usize, offset: usize },
    /// The number of sections differs from the scene count of the matching bytecode archive.
    SectionCount { found: usize, expected: usize },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::TooShort { len } => {
                write!(f, "message file too short ({len} bytes)")
            }
            TextError::BadTable { first, len } => write!(
                f,
                "message file: first section base {first:#x} is not a table size (even, non-zero, within {len} bytes)"
            ),
            TextError::BadBase {
                section,
                base,
                min,
                max,
            } => write!(
                f,
                "message file: section {section} base {base:#x} outside {min:#x}..={max:#x}"
            ),
            TextError::Unterminated { section, offset } => write!(
                f,
                "message file: section {section} ends with an unterminated string at relative offset {offset:#x}"
            ),
            TextError::SectionCount { found, expected } => write!(
                f,
                "message file has {found} sections but the scenario archive has {expected} scenes"
            ),
        }
    }
}

impl std::error::Error for TextError {}

/// One NUL-terminated block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<'a> {
    /// Offset of the first byte relative to the section base.
    pub offset: usize,
    /// The bytes without the terminating NUL.
    pub bytes: &'a [u8],
}

/// One section of a message file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Absolute offset of the section in the file.
    pub base: usize,
    pub blocks: Vec<Block<'a>>,
}

/// A validated message file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFile<'a> {
    pub sections: Vec<Section<'a>>,
}

impl MessageFile<'_> {
    /// Total number of NUL-terminated blocks (empty ones included).
    pub fn block_count(&self) -> usize {
        self.sections.iter().map(|s| s.blocks.len()).sum()
    }
}

fn le16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

/// Parse and validate a message file.
pub fn parse_messages(data: &[u8]) -> Result<MessageFile<'_>, TextError> {
    if data.len() < 2 {
        return Err(TextError::TooShort { len: data.len() });
    }
    let first = le16(data, 0);
    if first == 0 || first % 2 != 0 || first as usize > data.len() {
        return Err(TextError::BadTable {
            first,
            len: data.len(),
        });
    }
    let count = first as usize / 2;
    let mut bases = Vec::with_capacity(count);
    let mut min = first as usize;
    for section in 0..count {
        let base = le16(data, section * 2);
        if (base as usize) < min || base as usize > data.len() {
            return Err(TextError::BadBase {
                section,
                base,
                min,
                max: data.len(),
            });
        }
        min = base as usize;
        bases.push(base as usize);
    }
    let mut sections = Vec::with_capacity(count);
    for (i, &base) in bases.iter().enumerate() {
        let end = bases.get(i + 1).copied().unwrap_or(data.len());
        let region = &data[base..end];
        let mut blocks = Vec::new();
        let mut start = 0;
        for (at, &b) in region.iter().enumerate() {
            if b == 0 {
                blocks.push(Block {
                    offset: start,
                    bytes: &region[start..at],
                });
                start = at + 1;
            }
        }
        if start != region.len() {
            return Err(TextError::Unterminated {
                section: i,
                offset: start,
            });
        }
        sections.push(Section { base, blocks });
    }
    Ok(MessageFile { sections })
}

/// Build a message file from sections of strings (fixtures). Every string is NUL-terminated.
pub fn build_messages(sections: &[Vec<Vec<u8>>]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut bases = Vec::new();
    let table = sections.len() * 2;
    for strings in sections {
        bases.push(u16::try_from(table + body.len()).expect("fixture fits in 64 KiB"));
        for s in strings {
            body.extend_from_slice(s);
            body.push(0);
        }
    }
    let mut out: Vec<u8> = bases.iter().flat_map(|b| b.to_le_bytes()).collect();
    out.extend(body);
    out
}

/// Byte-pair statistics used to tell Big5 text from EUC-KR text.
///
/// EUC-KR (KS X 1001) uses lead and trail bytes 0xA1–0xFE only; Big5 trail bytes are 0x40–0x7E
/// for roughly two fifths of all characters. A high byte followed by a byte in 0x40–0x7E is
/// therefore strong evidence of Big5.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EncodingEvidence {
    /// Pairs `[0xA1–0xFE][0xA1–0xFE]` (valid in both encodings).
    pub high_pairs: usize,
    /// Pairs `[0x81–0xFE][0x40–0x7E]` (Big5 only).
    pub big5_pairs: usize,
}

/// Minimum number of double-byte pairs before a verdict is given.
const MIN_PAIRS: usize = 32;

impl EncodingEvidence {
    /// Scan text-like bytes.
    pub fn scan(bytes: &[u8]) -> EncodingEvidence {
        let mut ev = EncodingEvidence::default();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let (lead, trail) = (bytes[i], bytes[i + 1]);
            if lead >= 0x81 && lead != 0xff {
                if (0x40..=0x7e).contains(&trail) {
                    ev.big5_pairs += 1;
                    i += 2;
                    continue;
                }
                if lead >= 0xa1 && (0xa1..=0xfe).contains(&trail) {
                    ev.high_pairs += 1;
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }
        ev
    }

    /// Add another scan.
    pub fn add(&mut self, other: EncodingEvidence) {
        self.high_pairs += other.high_pairs;
        self.big5_pairs += other.big5_pairs;
    }

    /// The encoding the evidence clearly points to, if any: Big5 when at least a sixth of the
    /// pairs have a Big5-only trail byte, EUC-KR when at most one in fifty does.
    pub fn verdict(&self) -> Option<TextEncoding> {
        let total = self.high_pairs + self.big5_pairs;
        if total < MIN_PAIRS {
            return None;
        }
        if self.big5_pairs * 6 >= total {
            Some(TextEncoding::Big5)
        } else if self.big5_pairs * 50 <= total {
            Some(TextEncoding::EucKr)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(e: TextEncoding, s: &str) -> Vec<u8> {
        e.encode(s).unwrap()
    }

    #[test]
    fn decodes_both_encodings() {
        let ko = enc(TextEncoding::EucKr, "유비 현덕");
        assert_eq!(
            TextEncoding::EucKr.decode(&ko),
            Decoded {
                text: "유비 현덕".into(),
                malformed: false
            }
        );
        let zh = enc(TextEncoding::Big5, "劉備玄德");
        assert_eq!(TextEncoding::Big5.decode(&zh).text, "劉備玄德");
        // A lone lead byte is malformed.
        let d = TextEncoding::EucKr.decode(&[0xb0]);
        assert!(d.malformed);
        assert_eq!(d.text, "\u{fffd}");
        // Characters outside the encoding cannot be encoded.
        assert_eq!(TextEncoding::Big5.encode("한"), None);
    }

    #[test]
    fn parses_sections_and_blocks() {
        let e = TextEncoding::EucKr;
        let file = build_messages(&[
            vec![enc(e, "첫째"), vec![], enc(e, "둘째 줄")],
            vec![vec![0x12], enc(e, "셋째")],
        ]);
        let m = parse_messages(&file).unwrap();
        assert_eq!(m.sections.len(), 2);
        assert_eq!(m.sections[0].base, 4);
        assert_eq!(m.block_count(), 5);
        let s0 = &m.sections[0];
        assert_eq!(s0.blocks[0].offset, 0);
        assert_eq!(e.decode(s0.blocks[0].bytes).text, "첫째");
        assert_eq!(s0.blocks[1].bytes, b"");
        assert_eq!(s0.blocks[2].offset, 6);
        let s1 = &m.sections[1];
        assert_eq!(s1.base, 4 + 5 + 1 + 8);
        assert_eq!(s1.blocks[0].bytes, &[0x12]);
        assert_eq!(s1.blocks[1].offset, 2);
    }

    #[test]
    fn single_section_starts_at_two() {
        // The documented first word of SNR0M: one scene, table of 2 bytes.
        let file = build_messages(&[vec![b"abc".to_vec()]]);
        assert_eq!(&file[..2], &[2, 0]);
        assert_eq!(parse_messages(&file).unwrap().sections.len(), 1);
    }

    #[test]
    fn structural_errors() {
        assert_eq!(
            parse_messages(&[2]).unwrap_err(),
            TextError::TooShort { len: 1 }
        );
        for first in [0u16, 3, 200] {
            let mut f = first.to_le_bytes().to_vec();
            f.extend_from_slice(b"abc\0");
            assert!(matches!(
                parse_messages(&f).unwrap_err(),
                TextError::BadTable { .. }
            ));
        }
        // Second base before the first.
        let f = [4, 0, 3, 0, b'a', 0];
        assert_eq!(
            parse_messages(&f).unwrap_err(),
            TextError::BadBase {
                section: 1,
                base: 3,
                min: 4,
                max: 6
            }
        );
        // Base past the end.
        let f = [4, 0, 9, 0, b'a', 0];
        assert!(matches!(
            parse_messages(&f).unwrap_err(),
            TextError::BadBase { section: 1, .. }
        ));
        // Missing final NUL.
        let f = [2, 0, b'a', 0, b'b'];
        assert_eq!(
            parse_messages(&f).unwrap_err(),
            TextError::Unterminated {
                section: 0,
                offset: 2
            }
        );
        // A string running into the next section.
        let f = [4, 0, 6, 0, b'a', b'b', b'c', 0];
        assert_eq!(
            parse_messages(&f).unwrap_err(),
            TextError::Unterminated {
                section: 0,
                offset: 0
            }
        );
    }

    #[test]
    fn empty_sections_are_allowed() {
        let file = build_messages(&[vec![], vec![b"x".to_vec()]]);
        let m = parse_messages(&file).unwrap();
        assert!(m.sections[0].blocks.is_empty());
        assert_eq!(m.sections[1].blocks.len(), 1);
    }

    #[test]
    fn encoding_evidence() {
        let ko = enc(
            TextEncoding::EucKr,
            &"유비는 관우와 장비를 만나 도원에서 결의하였다. ".repeat(5),
        );
        assert_eq!(
            EncodingEvidence::scan(&ko).verdict(),
            Some(TextEncoding::EucKr)
        );
        let zh = enc(
            TextEncoding::Big5,
            &"劉備與關羽張飛於桃園結義共圖大事。".repeat(5),
        );
        let ev = EncodingEvidence::scan(&zh);
        assert!(ev.big5_pairs > 0, "{ev:?}");
        assert_eq!(ev.verdict(), Some(TextEncoding::Big5));
        // Too little text: no verdict.
        assert_eq!(
            EncodingEvidence::scan(&enc(TextEncoding::EucKr, "유비")).verdict(),
            None
        );
        // Plain ASCII or binary noise without pairs: no verdict.
        assert_eq!(EncodingEvidence::scan(&[0u8; 4000]).verdict(), None);
        let mut sum = EncodingEvidence::scan(&ko);
        sum.add(EncodingEvidence::scan(&ko));
        assert_eq!(sum.high_pairs, 2 * EncodingEvidence::scan(&ko).high_pairs);
    }
}
