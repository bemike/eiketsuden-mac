//! 6-byte-table containers (`FACEDAT.R3`, `PACKGRP.R3`).
//!
//! ```text
//! 0      N × 6 B  [u32le offset][u16le length]
//! N×6    data     entry 0 starts right after the table; offset[i+1] = offset[i] + length[i];
//!                 the last entry ends exactly at the end of the file
//! ```
//!
//! The table size is implied by the first offset (`N = offset[0] / 6`): 240 entries with data
//! from `0x5A0` in `FACEDAT.R3`, 38 entries with data from `0xE4` in `PACKGRP.R3`. The payloads
//! of both known files are TF-DCE compressed images, which this crate does not decode.

use std::fmt;

/// Size of one table entry.
pub const ENTRY_LEN: usize = 6;

/// A structural problem in a 6-byte-table container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Table6Error {
    /// Fewer than 6 bytes.
    TooShort { len: usize },
    /// The first offset is 0 or not a multiple of 6, so it cannot be the table size.
    BadTableSize { first_offset: u32 },
    /// The table is longer than the file.
    TablePastEnd { table_len: u64, file_len: usize },
    /// `offset[i] != offset[i-1] + length[i-1]`.
    BrokenChain {
        index: usize,
        expected: u64,
        found: u32,
    },
    /// An entry extends past the end of the file.
    EntryPastEnd {
        index: usize,
        end: u64,
        file_len: usize,
    },
    /// The last entry does not end exactly at the end of the file.
    TrailingData { end: u64, file_len: usize },
    /// An entry longer than a `u16` length can describe (when building).
    EntryTooLong { index: usize, len: usize },
}

impl fmt::Display for Table6Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Table6Error::*;
        match self {
            TooShort { len } => write!(f, "6-byte table: file too short ({len} bytes)"),
            BadTableSize { first_offset } => write!(
                f,
                "6-byte table: first offset {first_offset:#x} is not a table size (non-zero multiple of 6)"
            ),
            TablePastEnd {
                table_len,
                file_len,
            } => write!(
                f,
                "6-byte table: table of {table_len} bytes is longer than the file ({file_len} bytes)"
            ),
            BrokenChain {
                index,
                expected,
                found,
            } => write!(
                f,
                "6-byte table: entry {index} starts at {found:#x}, expected {expected:#x}"
            ),
            EntryPastEnd {
                index,
                end,
                file_len,
            } => write!(
                f,
                "6-byte table: entry {index} ends at {end:#x}, past the end of the file ({file_len:#x})"
            ),
            TrailingData { end, file_len } => write!(
                f,
                "6-byte table: entries end at {end:#x} but the file is {file_len:#x} bytes long"
            ),
            EntryTooLong { index, len } => {
                write!(f, "6-byte table: entry {index} has {len} bytes (max 65535)")
            }
        }
    }
}

impl std::error::Error for Table6Error {}

/// One table entry.
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
    /// Parse and validate the table and the chain of entries.
    pub fn parse(data: &'a [u8]) -> Result<Table6<'a>, Table6Error> {
        if data.len() < ENTRY_LEN {
            return Err(Table6Error::TooShort { len: data.len() });
        }
        let first_offset = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if first_offset == 0 || first_offset as usize % ENTRY_LEN != 0 {
            return Err(Table6Error::BadTableSize { first_offset });
        }
        if first_offset as usize > data.len() {
            return Err(Table6Error::TablePastEnd {
                table_len: u64::from(first_offset),
                file_len: data.len(),
            });
        }
        let count = first_offset as usize / ENTRY_LEN;
        let mut entries = Vec::with_capacity(count);
        let mut next = u64::from(first_offset);
        for index in 0..count {
            let at = index * ENTRY_LEN;
            let offset = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
            let len = u16::from_le_bytes([data[at + 4], data[at + 5]]);
            if u64::from(offset) != next {
                return Err(Table6Error::BrokenChain {
                    index,
                    expected: next,
                    found: offset,
                });
            }
            next += u64::from(len);
            if next > data.len() as u64 {
                return Err(Table6Error::EntryPastEnd {
                    index,
                    end: next,
                    file_len: data.len(),
                });
            }
            entries.push(Entry { offset, len });
        }
        if next != data.len() as u64 {
            return Err(Table6Error::TrailingData {
                end: next,
                file_len: data.len(),
            });
        }
        Ok(Table6 { data, entries })
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

    /// The payload of entry `index`, if it exists.
    pub fn get(&self, index: usize) -> Option<&'a [u8]> {
        let e = self.entries.get(index)?;
        let start = e.offset as usize;
        Some(&self.data[start..start + e.len as usize])
    }
}

/// Write a container holding `entries` (at least one).
pub fn build(entries: &[&[u8]]) -> Result<Vec<u8>, Table6Error> {
    let mut out = Vec::new();
    let mut offset = entries.len() * ENTRY_LEN;
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
            .map(|i| vec![i as u8; (i % 7) as usize])
            .collect();
        let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
        let file = build(&refs).unwrap();
        let t = Table6::parse(&file).unwrap();
        assert_eq!(t.len(), 240);
        assert_eq!(t.entries()[0].offset, 0x5A0);
        for (i, p) in payloads.iter().enumerate() {
            assert_eq!(t.get(i), Some(p.as_slice()));
        }
        assert_eq!(t.get(240), None);

        let small: Vec<&[u8]> = vec![b"x"; 38];
        let file = build(&small).unwrap();
        assert_eq!(Table6::parse(&file).unwrap().entries()[0].offset, 0xE4);
    }

    #[test]
    fn errors() {
        assert_eq!(
            Table6::parse(&[1, 2, 3]).unwrap_err(),
            Table6Error::TooShort { len: 3 }
        );
        let good = build(&[b"abc", b"defg"]).unwrap();

        let mut f = good.clone();
        f[0] = 7;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::BadTableSize { first_offset: 7 }
        );
        let mut f = good.clone();
        f[0] = 0;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::BadTableSize { first_offset: 0 }
        );
        let mut f = good.clone();
        f[0] = 60;
        assert!(matches!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::TablePastEnd { .. }
        ));
        let mut f = good.clone();
        f[6] += 1;
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::BrokenChain {
                index: 1,
                expected: 15,
                found: 16
            }
        );
        let mut f = good.clone();
        f[10] = 9;
        assert!(matches!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::EntryPastEnd { index: 1, .. }
        ));
        let mut f = good.clone();
        f.push(0);
        assert_eq!(
            Table6::parse(&f).unwrap_err(),
            Table6Error::TrailingData {
                end: 19,
                file_len: 20
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
        assert_eq!(t.get(1), Some(&b""[..]));
        assert_eq!(t.get(2), Some(&b"b"[..]));
    }
}
