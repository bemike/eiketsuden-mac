//! Message files (`SNR0M.R3`–`SNR4M.R3`, `IPPAN0M.R3`) and the legacy text encodings.
//!
//! **Scenario message files** (`SNRnM.R3`, verified on the Korean DOS/V copy): a table of `N`
//! little-endian `u16` section bases (the first base is `2 × N`, so the table is followed
//! directly by section 0), then the sections. Section `i` belongs to scene `i` of the matching
//! `SNRnD.R3` archive, and the scenario bytecode addresses its text by byte offset relative to
//! the section base. A base is only 16 bits wide although a file may exceed 64 KiB (`SNR3M.R3`
//! is 106,704 bytes): the game adds `0x10000` to the two bases of that file that wrapped. The
//! parser applies the equivalent general rule — a base smaller than the previous one has
//! wrapped — so the real bases are strictly the running sum.
//!
//! A section is not a list of NUL-separated strings; it holds two kinds of items, and only the
//! bytecode says which kind starts where:
//!
//! * a **plain string** (narration, caption, title, choice list, battle objective):
//!   EUC-KR text up to a NUL;
//! * a **dialogue**: records `[u16le speaker][text][00]` repeated until a `u16` `0xFFFF`. The
//!   speaker is an officer index of `BAKDATA.R3` (0–383).
//!
//! Every byte of every section of the verified copy is covered by exactly these items.
//!
//! **`IPPAN0M.R3`** has no table: it is a pool of 653 NUL-terminated strings, the townspeople's
//! lines, addressed through `IPPAN0.R3` (see [`crate::ippan`]); [`split_strings`] lists them.

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
    /// A (wrap-corrected) base lies past the end of the file.
    BadBase {
        section: usize,
        base: usize,
        len: usize,
    },
    /// The number of sections differs from the scene count of the matching bytecode archive.
    SectionCount { found: usize, expected: usize },
    /// A message offset lies outside its section.
    OffsetOutOfRange { offset: usize, len: usize },
    /// A string starting at `offset` has no terminating NUL inside its section.
    Unterminated { offset: usize },
    /// A dialogue starting at `offset` runs past its section without the `0xFFFF` terminator.
    UnterminatedDialogue { offset: usize },
    /// A string pool (`IPPAN0M.R3`) does not end with a NUL.
    UnterminatedPool { offset: usize },
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
            TextError::BadBase { section, base, len } => write!(
                f,
                "message file: section {section} base {base:#x} lies past the end ({len:#x} bytes)"
            ),
            TextError::SectionCount { found, expected } => write!(
                f,
                "message file has {found} sections but the scenario archive has {expected} scenes"
            ),
            TextError::OffsetOutOfRange { offset, len } => write!(
                f,
                "message offset {offset:#x} outside its section ({len:#x} bytes)"
            ),
            TextError::Unterminated { offset } => {
                write!(f, "message at {offset:#x} has no terminating NUL in its section")
            }
            TextError::UnterminatedDialogue { offset } => write!(
                f,
                "dialogue at {offset:#x} runs past its section without the 0xFFFF terminator"
            ),
            TextError::UnterminatedPool { offset } => {
                write!(f, "string pool: the string at {offset:#x} has no terminating NUL")
            }
        }
    }
}

impl std::error::Error for TextError {}

/// One line of a dialogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line<'a> {
    /// Offset of the speaker field relative to the section base.
    pub offset: usize,
    /// Officer index (`BAKDATA.R3`) of the speaker.
    pub speaker: u16,
    /// Text without the terminating NUL.
    pub text: &'a [u8],
}

/// One section (the text of one scene).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section<'a> {
    /// Absolute offset of the section in the file (wrap-corrected).
    pub base: usize,
    /// The section's bytes.
    pub bytes: &'a [u8],
}

impl<'a> Section<'a> {
    fn check(&self, offset: usize) -> Result<(), TextError> {
        if offset >= self.bytes.len() {
            return Err(TextError::OffsetOutOfRange {
                offset,
                len: self.bytes.len(),
            });
        }
        Ok(())
    }

    /// The plain string at `offset` (without its NUL).
    pub fn string_at(&self, offset: usize) -> Result<&'a [u8], TextError> {
        self.check(offset)?;
        let rest = &self.bytes[offset..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(TextError::Unterminated { offset })?;
        Ok(&rest[..end])
    }

    /// The dialogue at `offset`: its lines and the offset just past the `0xFFFF` terminator.
    pub fn dialogue_at(&self, offset: usize) -> Result<(Vec<Line<'a>>, usize), TextError> {
        self.check(offset)?;
        let mut lines = Vec::new();
        let mut at = offset;
        loop {
            let Some(pair) = self.bytes.get(at..at + 2) else {
                return Err(TextError::UnterminatedDialogue { offset });
            };
            let speaker = u16::from_le_bytes([pair[0], pair[1]]);
            if speaker == 0xffff {
                return Ok((lines, at + 2));
            }
            let text = self
                .string_at(at + 2)
                .map_err(|_| TextError::UnterminatedDialogue { offset })?;
            lines.push(Line {
                offset: at,
                speaker,
                text,
            });
            at += 2 + text.len() + 1;
        }
    }
}

/// A validated scenario message file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFile<'a> {
    pub sections: Vec<Section<'a>>,
}

fn le16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

/// Parse and validate the section table of a scenario message file.
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
    let mut previous = 0usize;
    for section in 0..count {
        let raw = le16(data, section * 2) as usize;
        // The high bits the 16-bit field cannot hold: the smallest base ≥ the previous one.
        let mut base = (previous & !0xffff) | raw;
        if base < previous {
            base += 0x10000;
        }
        if base > data.len() {
            return Err(TextError::BadBase {
                section,
                base,
                len: data.len(),
            });
        }
        bases.push(base);
        previous = base;
    }
    let sections = bases
        .iter()
        .enumerate()
        .map(|(i, &base)| {
            let end = bases.get(i + 1).copied().unwrap_or(data.len());
            Section {
                base,
                bytes: &data[base..end],
            }
        })
        .collect();
    Ok(MessageFile { sections })
}

/// One NUL-terminated string of a pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block<'a> {
    /// Offset of the first byte.
    pub offset: usize,
    /// The bytes without the terminating NUL.
    pub bytes: &'a [u8],
}

/// Split a string pool (`IPPAN0M.R3`) into its NUL-terminated strings.
pub fn split_strings(data: &[u8]) -> Result<Vec<Block<'_>>, TextError> {
    let mut blocks = Vec::new();
    let mut start = 0;
    for (at, &b) in data.iter().enumerate() {
        if b == 0 {
            blocks.push(Block {
                offset: start,
                bytes: &data[start..at],
            });
            start = at + 1;
        }
    }
    if start != data.len() {
        return Err(TextError::UnterminatedPool { offset: start });
    }
    Ok(blocks)
}

/// Build a message file from sections of raw items (fixtures): every item is followed by a
/// NUL. Use [`dialogue_bytes`] for a dialogue item (its last line's NUL is then followed by
/// the `0xFFFF` terminator bytes, which `build_messages` does not add).
pub fn build_messages(sections: &[Vec<Vec<u8>>]) -> Vec<u8> {
    let raw: Vec<Vec<u8>> = sections
        .iter()
        .map(|items| {
            items
                .iter()
                .flat_map(|s| s.iter().copied().chain([0]))
                .collect()
        })
        .collect();
    build_sections(&raw)
}

/// Build a message file from the raw bytes of each section (fixtures).
pub fn build_sections(sections: &[Vec<u8>]) -> Vec<u8> {
    let table = sections.len() * 2;
    let mut out = Vec::new();
    let mut body = Vec::new();
    for section in sections {
        out.extend_from_slice(&(((table + body.len()) & 0xffff) as u16).to_le_bytes());
        body.extend_from_slice(section);
    }
    out.extend(body);
    out
}

/// Bytes of a dialogue (fixtures): `[speaker][text][00]…` then `FF FF`, all concatenated.
pub fn dialogue_bytes(lines: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (speaker, text) in lines {
        out.extend_from_slice(&speaker.to_le_bytes());
        out.extend_from_slice(text);
        out.push(0);
    }
    out.extend_from_slice(&[0xff, 0xff]);
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
    fn sections_strings_and_dialogues() {
        let e = TextEncoding::EucKr;
        // Section 0: a plain string, then a two-line dialogue (FF FF terminated) followed by
        // a marker byte (the builder then appends a NUL after the item).
        let mut dlg = dialogue_bytes(&[(3, enc(e, "흥!")), (6, enc(e, "상국"))]);
        dlg.push(b'x');
        let file = build_messages(&[vec![enc(e, "서장"), dlg], vec![enc(e, "끝")]]);
        let m = parse_messages(&file).unwrap();
        assert_eq!(m.sections.len(), 2);
        let s0 = &m.sections[0];
        assert_eq!(s0.base, 4);
        assert_eq!(e.decode(s0.string_at(0).unwrap()).text, "서장");
        let (lines, end) = s0.dialogue_at(5).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].speaker, 3);
        assert_eq!(lines[0].offset, 5);
        assert_eq!(e.decode(lines[0].text).text, "흥!");
        assert_eq!(lines[1].speaker, 6);
        assert_eq!(lines[1].offset, 5 + 2 + 3 + 1);
        assert_eq!(end, lines[1].offset + 2 + 4 + 1 + 2);
        assert_eq!(s0.bytes[end], b'x');
        assert_eq!(e.decode(m.sections[1].string_at(0).unwrap()).text, "끝");
    }

    #[test]
    fn single_section_starts_at_two() {
        // The verified first word of SNR0M: one scene, table of 2 bytes.
        let file = build_messages(&[vec![b"abc".to_vec()]]);
        assert_eq!(&file[..2], &[2, 0]);
        assert_eq!(parse_messages(&file).unwrap().sections.len(), 1);
    }

    #[test]
    fn bases_past_64k_wrap() {
        // Three sections of 40,000 bytes: the stored bases of sections 2 wrap past 0xFFFF.
        let big = vec![b'a'; 39_999];
        let file = build_messages(&[vec![big.clone()], vec![big.clone()], vec![big]]);
        assert_eq!(file.len(), 6 + 120_000);
        assert_eq!(
            u16::from_le_bytes([file[4], file[5]]),
            (80_006 - 0x10000) as u16
        );
        let m = parse_messages(&file).unwrap();
        let bases: Vec<usize> = m.sections.iter().map(|s| s.base).collect();
        assert_eq!(bases, vec![6, 40_006, 80_006]);
        assert!(m.sections.iter().all(|s| s.bytes.len() == 40_000));
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
        // Second base before the first wraps to 64 KiB + 3, past the end.
        let f = [4, 0, 3, 0, b'a', 0];
        assert_eq!(
            parse_messages(&f).unwrap_err(),
            TextError::BadBase {
                section: 1,
                base: 0x10003,
                len: 6
            }
        );
        // Base past the end.
        let f = [4, 0, 9, 0, b'a', 0];
        assert!(matches!(
            parse_messages(&f).unwrap_err(),
            TextError::BadBase { section: 1, .. }
        ));
        // Strings and dialogues must end inside their section.
        let f = [4, 0, 6, 0, b'a', b'b', b'c', 0];
        let m = parse_messages(&f).unwrap();
        assert_eq!(
            m.sections[0].string_at(0).unwrap_err(),
            TextError::Unterminated { offset: 0 }
        );
        assert_eq!(
            m.sections[0].string_at(2).unwrap_err(),
            TextError::OffsetOutOfRange { offset: 2, len: 2 }
        );
        assert_eq!(m.sections[1].string_at(0).unwrap(), b"c");
        let f = [2, 0, 5, 0, b'h', b'i', 0, 7, 0];
        let m = parse_messages(&f).unwrap();
        assert_eq!(
            m.sections[0].dialogue_at(0).unwrap_err(),
            TextError::UnterminatedDialogue { offset: 0 }
        );
    }

    #[test]
    fn string_pool() {
        let pool = b"ab\0\0cde\0";
        let s = split_strings(pool).unwrap();
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].offset, s[0].bytes), (0, &b"ab"[..]));
        assert_eq!((s[1].offset, s[1].bytes), (3, &b""[..]));
        assert_eq!((s[2].offset, s[2].bytes), (4, &b"cde"[..]));
        assert_eq!(
            split_strings(b"ab\0c").unwrap_err(),
            TextError::UnterminatedPool { offset: 3 }
        );
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
