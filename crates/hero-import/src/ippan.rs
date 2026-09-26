//! Townspeople chatter: `IPPAN0.R3` (index) and `IPPAN0M.R3` (strings).
//!
//! Verified on the Korean DOS/V copy, where both files are self-delimiting and the chunk
//! boundaries agree with the offset / length table the game keeps in `MAIN.EXE`:
//!
//! ```text
//! IPPAN0.R3     one chunk per chapter 1-4, back to back:
//!   [u8 n] [n × u16 town record offset (from byte 1 of the chunk; 0 = none)]
//!   [u16 m] [m × u16 string offset (into this chapter's part of IPPAN0M.R3; FFFF = none)]
//!   town record: [u8 g] g × ([u8 key] [u8 k] [k × u16 entry])
//! IPPAN0M.R3    NUL-terminated strings; chapter c's part starts where chapter c-1's last
//!               referenced string ends
//! ```
//!
//! When the town map places its ordinary people (script instruction `0x1F`), the game picks
//! the town's group whose key matches a game-state value (key 125 is the default group) and
//! puts each entry at a random free cell. An entry is both the townsperson index (`BAKDATA.R3`
//! townspeople: name and sprite) and the index of the line that person says.

use serde::Serialize;
use std::fmt;

/// A structural problem in the townspeople files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IppanError {
    /// The index runs out of bytes inside chunk `chapter` at byte `offset`.
    Truncated { chapter: usize, offset: usize },
    /// A string offset of chunk `chapter` does not start a NUL-terminated string.
    BadString { chapter: usize, entry: usize },
    /// A group of chunk `chapter` names an entry without a string.
    BadEntry { chapter: usize, entry: u16 },
    /// The strings of the chapters do not end exactly at the end of `IPPAN0M.R3`.
    PoolLength { used: usize, len: usize },
}

impl fmt::Display for IppanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IppanError::Truncated { chapter, offset } => {
                write!(
                    f,
                    "IPPAN0.R3 chunk {chapter}: truncated at byte {offset:#x}"
                )
            }
            IppanError::BadString { chapter, entry } => write!(
                f,
                "IPPAN0.R3 chunk {chapter}: string {entry} is not a string of IPPAN0M.R3"
            ),
            IppanError::BadEntry { chapter, entry } => {
                write!(
                    f,
                    "IPPAN0.R3 chunk {chapter}: group entry {entry} has no string"
                )
            }
            IppanError::PoolLength { used, len } => write!(
                f,
                "IPPAN0M.R3: the chapters' strings end at {used:#x}, the file is {len:#x} bytes"
            ),
        }
    }
}

impl std::error::Error for IppanError {}

/// A group of townspeople of one town.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Group {
    /// Game-state value the group is chosen for (125 = default).
    pub key: u8,
    /// Entries (townsperson index = line index).
    pub entries: Vec<u16>,
}

/// One town of a chapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Town {
    pub index: usize,
    pub groups: Vec<Group>,
}

/// One chapter's chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter<'a> {
    /// Offset of the chunk in `IPPAN0.R3`.
    pub offset: usize,
    pub len: usize,
    /// Offset of the chapter's strings in `IPPAN0M.R3`.
    pub pool_base: usize,
    pub towns: Vec<Town>,
    /// Lines by entry (`None` = no line).
    pub lines: Vec<Option<&'a [u8]>>,
}

struct Cursor<'a> {
    data: &'a [u8],
    chapter: usize,
}

impl Cursor<'_> {
    fn u8(&self, at: usize) -> Result<u8, IppanError> {
        self.data.get(at).copied().ok_or(IppanError::Truncated {
            chapter: self.chapter,
            offset: at,
        })
    }

    fn u16(&self, at: usize) -> Result<u16, IppanError> {
        Ok(u16::from(self.u8(at)?) | u16::from(self.u8(at + 1)?) << 8)
    }
}

/// Parse both files.
pub fn parse<'a>(index: &[u8], pool: &'a [u8]) -> Result<Vec<Chapter<'a>>, IppanError> {
    let mut chapters = Vec::new();
    let (mut at, mut base) = (0, 0);
    while at < index.len() {
        let chapter = chapters.len();
        let c = Cursor {
            data: &index[at..],
            chapter,
        };
        let n = c.u8(0)? as usize;
        let town_offsets = (0..n)
            .map(|i| c.u16(1 + 2 * i))
            .collect::<Result<Vec<_>, _>>()?;
        let table = 1 + 2 * n;
        let m = c.u16(table)? as usize;
        let string_offsets = (0..m)
            .map(|i| c.u16(table + 2 + 2 * i))
            .collect::<Result<Vec<_>, _>>()?;
        let mut end = table + 2 + 2 * m;
        let mut towns = Vec::new();
        for (index, &o) in town_offsets.iter().enumerate() {
            if o == 0 {
                continue;
            }
            let mut q = 1 + o as usize;
            let g = c.u8(q)?;
            q += 1;
            let mut groups = Vec::with_capacity(g as usize);
            for _ in 0..g {
                let key = c.u8(q)?;
                let k = c.u8(q + 1)? as usize;
                let entries = (0..k)
                    .map(|i| c.u16(q + 2 + 2 * i))
                    .collect::<Result<Vec<_>, _>>()?;
                q += 2 + 2 * k;
                groups.push(Group { key, entries });
            }
            end = end.max(q);
            towns.push(Town { index, groups });
        }
        let mut lines = Vec::with_capacity(m);
        let mut used = base;
        for (entry, &o) in string_offsets.iter().enumerate() {
            if o == 0xffff {
                lines.push(None);
                continue;
            }
            let start = base + o as usize;
            let bad = IppanError::BadString { chapter, entry };
            if start >= pool.len() || (start > 0 && pool[start - 1] != 0) {
                return Err(bad);
            }
            let len = pool[start..].iter().position(|&b| b == 0).ok_or(bad)?;
            used = used.max(start + len + 1);
            lines.push(Some(&pool[start..start + len]));
        }
        for town in &towns {
            for e in town.groups.iter().flat_map(|g| &g.entries) {
                if !matches!(lines.get(*e as usize), Some(Some(_))) {
                    return Err(IppanError::BadEntry { chapter, entry: *e });
                }
            }
        }
        chapters.push(Chapter {
            offset: at,
            len: end,
            pool_base: base,
            towns,
            lines,
        });
        at += end;
        base = used;
    }
    if base != pool.len() {
        return Err(IppanError::PoolLength {
            used: base,
            len: pool.len(),
        });
    }
    Ok(chapters)
}

/// Fixture input: the towns (each a list of `(key, entries)` groups) and the lines of a chapter.
pub type FixtureChapter = (Vec<Vec<(u8, Vec<u16>)>>, Vec<Vec<u8>>);

/// Fixture builder: `(IPPAN0.R3, IPPAN0M.R3)` for the given chapters.
pub fn build(chapters: &[FixtureChapter]) -> (Vec<u8>, Vec<u8>) {
    let (mut index, mut pool) = (Vec::new(), Vec::new());
    for (towns, lines) in chapters {
        let base = pool.len();
        let mut offsets = Vec::new();
        for line in lines {
            offsets.push((pool.len() - base) as u16);
            pool.extend_from_slice(line);
            pool.push(0);
        }
        let head = 1 + 2 * towns.len() + 2 + 2 * offsets.len();
        let mut records = Vec::new();
        let mut town_offsets = Vec::new();
        for groups in towns {
            town_offsets.push((head + records.len() - 1) as u16);
            records.push(groups.len() as u8);
            for (key, entries) in groups {
                records.push(*key);
                records.push(entries.len() as u8);
                records.extend(entries.iter().flat_map(|e| e.to_le_bytes()));
            }
        }
        index.push(towns.len() as u8);
        index.extend(town_offsets.iter().flat_map(|o| o.to_le_bytes()));
        index.extend((offsets.len() as u16).to_le_bytes());
        index.extend(offsets.iter().flat_map(|o| o.to_le_bytes()));
        index.extend(records);
    }
    (index, pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let (index, pool) = build(&[
            (
                vec![vec![(125, vec![0, 1]), (8, vec![2])]],
                vec![b"a".to_vec(), b"bc".to_vec(), b"d".to_vec()],
            ),
            (vec![vec![(125, vec![0])]], vec![b"xyz".to_vec()]),
        ]);
        let chapters = parse(&index, &pool).unwrap();
        assert_eq!(chapters.len(), 2);
        let c0 = &chapters[0];
        assert_eq!((c0.offset, c0.pool_base), (0, 0));
        assert_eq!(
            c0.towns[0].groups[1],
            Group {
                key: 8,
                entries: vec![2]
            }
        );
        assert_eq!(c0.lines[1], Some(&b"bc"[..]));
        let c1 = &chapters[1];
        assert_eq!(c1.offset, c0.len);
        assert_eq!(c1.pool_base, 7);
        assert_eq!(c1.lines, vec![Some(&b"xyz"[..])]);
    }

    #[test]
    fn errors() {
        let (index, pool) = build(&[(vec![vec![(125, vec![0])]], vec![b"a".to_vec()])]);
        assert!(matches!(
            parse(&index[..index.len() - 1], &pool).unwrap_err(),
            IppanError::Truncated { chapter: 0, .. }
        ));
        let mut longer = pool.clone();
        longer.extend(b"b\0");
        assert_eq!(
            parse(&index, &longer).unwrap_err(),
            IppanError::PoolLength { used: 2, len: 4 }
        );
        // A group entry without a line.
        let (index, pool) = build(&[(vec![vec![(125, vec![3])]], vec![b"a".to_vec()])]);
        assert_eq!(
            parse(&index, &pool).unwrap_err(),
            IppanError::BadEntry {
                chapter: 0,
                entry: 3
            }
        );
    }
}
