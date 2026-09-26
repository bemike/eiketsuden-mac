//! 6-byte-table containers (`FACEDAT.R3`, `PACKGRP.R3`).
//!
//! ```text
//! 0      N × 6 B  [u32le offset][u16le length]
//! N×6    data     offsets are relative to the start of the data area: offset[0] = 0,
//!                 offset[i+1] = offset[i] + length[i]; the last entry ends exactly at the end
//!                 of the file
//! ```
//!
//! The file does not store `N`. `MAIN.EXE` hard-codes the size of the table it skips (`0x5A0`
//! = 240 entries for `FACEDAT.R3`); from the file alone `N` is the only count for which the
//! chain of entries ends exactly at the end of the file. That count is unique: `N × 6 + end of
//! the chain` grows strictly with `N`, so at most one `N` can equal the file length. The Korean
//! build has 240 entries in `FACEDAT.R3` (data from `0x5A0`) and 38 in `PACKGRP.R3` (data from
//! `0xE4`); both hold TF-DCE compressed images (see [`crate::tfdce`]).

use std::fmt;

/// Size of one table entry.
pub const ENTRY_LEN: usize = 6;

/// A structural problem in a 6-byte-table container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Table6Error {
    /// Fewer than 6 bytes.
    TooShort { len: usize },
    /// The first offset is not 0 (offsets are relative to the data area).
    FirstOffsetNotZero { found: u32 },
    /// `offset[i] != offset[i-1] + length[i-1]` before the chain reached the end of the file.
    BrokenChain {
        index: usize,
        expected: u64,
        found: u32,
    },
    /// Table plus entries `0..=index` already exceed the file, so no entry count fits.
    PastEnd {
        index: usize,
        end: u64,
        file_len: usize,
    },
    /// The table runs into the end of the file before the chain ends there.
    Unterminated { entries: usize, file_len: usize },
    /// An entry longer than a `u16` length can describe (when building).
    EntryTooLong { index: usize, len: usize },
}

impl fmt::Display for Table6Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Table6Error::*;
        match self {
            TooShort { len } => write!(f, "6-byte table: file too short ({len} bytes)"),
            FirstOffsetNotZero { found } => write!(
                f,
                "6-byte table: first offset is {found:#x}, expected 0 (offsets are relative to the data area)"
            ),
            BrokenChain {
                index,
                expected,
                found,
            } => write!(
                f,
                "6-byte table: entry {index} starts at {found:#x}, expected {expected:#x}"
            ),
            PastEnd {
                index,
                end,
                file_len,
            } => write!(
                f,
                "6-byte table: with {} entries the file would need {end:#x} bytes but has {file_len:#x}",
                index + 1
            ),
            Unterminated { entries, file_len } => write!(
                f,
                "6-byte table: {entries} chained entries fill the file ({file_len:#x} bytes) without ending at its end"
            ),
            EntryTooLong { index, len } => {
                write!(f, "6-byte table: entry {index} has {len} bytes (max 65535)")
            }
        }
    }
}

impl std::error::Error for Table6Error {}

/// One table entry. `offset` is relative to the data area ([`Table6::data_start`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub offset: u32,
    pub len: u16,
}

/// A validated container borrowing the file's bytes.
#[derive(Debug, Clone)]
pub struct Table6<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

impl<'a> Table6<'a> {
    /// Parse the table, deriving the entry count, and validate the chain of entries.
    pub fn parse(data: &'a [u8]) -> Result<Table6<'a>, Table6Error> {
        if data.len() < ENTRY_LEN {
            return Err(Table6Error::TooShort { len: data.len() });
        }
        let file_len = data.len() as u64;
        let mut entries = Vec::new();
        let mut end = 0u64;
        loop {
            let index = entries.len();
            let at = index * ENTRY_LEN;
            if at + ENTRY_LEN > data.len() {
                return Err(Table6Error::Unterminated {
                    entries: index,
                    file_len: data.len(),
                });
            }
            let offset = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
            let len = u16::from_le_bytes([data[at + 4], data[at + 5]]);
            if index == 0 && offset != 0 {
                return Err(Table6Error::FirstOffsetNotZero { found: offset });
            }
            if u64::from(offset) != end {
                return Err(Table6Error::BrokenChain {
                    index,
                    expected: end,
                    found: offset,
                });
            }
            end += u64::from(len);
            entries.push(Entry { offset, len });
            let needed = (entries.len() * ENTRY_LEN) as u64 + end;
            if needed == file_len {
                return Ok(Table6 { data, entries });
            }
            if needed > file_len {
                return Err(Table6Error::PastEnd {
                    index,
                    end: needed,
                    file_len: data.len(),
                });
            }
        }
    }

    /// The table, in file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Always `false` for a parsed table (it has at least one entry).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// File offset of the data area (the size of the table).
    pub fn data_start(&self) -> usize {
        self.entries.len() * ENTRY_LEN
    }

    /// The payload of entry `index`, if it exists.
    pub fn get(&self, index: usize) -> Option<&'a [u8]> {
        let e = self.entries.get(index)?;
        let start = self.data_start() + e.offset as usize;
        Some(&self.data[start..start + e.len as usize])
    }
}

/// Write a container holding `entries` (at least one).
pub fn build(entries: &[&[u8]]) -> Result<Vec<u8>, Table6Error> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for (index, e) in entries.iter().enumerate() {
        let len = u16::try_from(e.len()).map_err(|_| Table6Error::EntryTooLong {
            index,
            len: e.len(),
        })?;
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        offset += e.len();
    }
    for e in entries {
        out.extend_from_slice(e);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_known_table_sizes() {
        let payloads: Vec<Vec<u8>> = (0..240u32)
            .map(|i| vec![i as u8; 1 + (i % 7) as usize])
            .collect();
        let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
        let file = build(&refs).unwrap();
        let t = Table6::parse(&file).unwrap();
        assert_eq!(t.len(), 240);
        assert_eq!(t.data_start(), 0x5A0);
        assert_eq!(t.entries()[0].offset, 0);
        assert_eq!(t.entries()[1].offset, 1);
        for (i, p) in payloads.iter().enumerate() {
            assert_eq!(t.get(i), Some(p.as_slice()));
        }
        assert_eq!(t.get(240), None);

        let small: Vec<&[u8]> = vec![b"xy"; 38];
        let file = build(&small).unwrap();
        let t = Table6::parse(&file).unwrap();
        assert_eq!((t.len(), t.data_start()), (38, 0xE4));
    }

    #[test]
    fn errors() {
        assert_eq!(
            Table6::parse(&[1, 2, 3]).unwrap_err(),
            Table6Error::TooShort { len: 3 }
        );
        let good = build(&[b"abc", b"defg"]).unwrap();
        assert_eq!(good.len(), 12 + 7);
        assert_eq!(Table6::parse(&good).unwrap().len(), 2);

        let mut f = good.clone();
        f[0] = 6;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::FirstOffsetNotZero { found: 6 }
        );
        let mut f = good.clone();
        f[6] += 1;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::BrokenChain {
                index: 1,
                expected: 3,
                found: 4
            }
        );
        // A second length that overshoots the file.
        let mut f = good.clone();
        f[10] = 9;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::PastEnd {
                index: 1,
                end: 12 + 12,
                file_len: 19
            }
        );
        // One byte too many: the chain can never end at the end of the file.
        let mut f = good.clone();
        f.push(0);
        assert!(matches!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::BrokenChain { index: 2, .. } | Table6Error::PastEnd { .. }
        ));
        // Zero lengths chained through the whole file.
        assert_eq!(
            Table6::parse(&[0u8; 8]).unwrap_err(),
            Table6Error::Unterminated {
                entries: 1,
                file_len: 8
            }
        );
        let big = vec![0u8; 70_000];
        assert_eq!(
            build(&[&big]).unwrap_err(),
            Table6Error::EntryTooLong {
                index: 0,
                len: 70_000
            }
        );
    }

    #[test]
    fn empty_entries_are_allowed() {
        let file = build(&[b"a", b"", b"b"]).unwrap();
        let t = Table6::parse(&file).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t.get(1), Some(&b""[..]));
        assert_eq!(t.get(2), Some(&b"b"[..]));
    }

    #[test]
    fn count_comes_from_the_file_length() {
        // One entry of 6 bytes whose payload itself looks like a table entry: the count is 1.
        let file = build(&[&[6, 0, 0, 0, 0, 0]]).unwrap();
        assert_eq!(Table6::parse(&file).unwrap().len(), 1);
    }
}
