//! `LS11` archives: the `.R3` containers of the DOS/V builds.
//!
//! # Layout (all integers big-endian)
//!
//! ```text
//! 0x000  16 B   "LS11" + 12 zero bytes
//! 0x010  256 B  byte dictionary shared by every entry of the file
//! 0x110  N×12 B directory: [compressed length][decoded length][absolute offset]
//!        then a zero terminator (4 bytes in the known files; at most 15 zero bytes accepted)
//! data   entry i starts at offset[i]; offset[i+1] = offset[i] + clen[i]; the last entry ends
//!        exactly at the end of the file
//! ```
//!
//! An entry whose compressed and decoded lengths are equal is stored raw. Otherwise it is an
//! MSB-first bit stream of variable-length codes. A code is a prefix of `k` bits — `k − 1` one
//! bits and a zero bit, read as a binary number `2^k − 2` — followed by a `k`-bit segment that is
//! added to it, so `k = 1` covers 0–1, `k = 2` covers 2–5, `k = 3` covers 6–13 and so on. A code
//! below 256 is a literal (`dictionary[code]`); a code `v ≥ 256` is a back-reference to the
//! output `v − 256` bytes back, whose length is the next code plus 3 (copies may overlap).
//!
//! Decoding checks every invariant: the directory chain, entries inside the file, the exact
//! decoded length and that the bit stream is consumed completely (only the padding bits of its
//! last byte may remain). [`build`] writes archives with this module's own encoder; tests use it
//! to create fixtures, so no original data is ever needed.

use std::fmt;

/// Length of the header (`LS11` + padding).
pub const HEADER_LEN: usize = 16;
/// Offset of the 256-byte dictionary.
pub const DICT_OFFSET: usize = 0x10;
/// Offset of the directory.
pub const DIR_OFFSET: usize = 0x110;
/// Size of one directory entry.
pub const DIR_ENTRY_LEN: usize = 12;
/// Largest decoded entry accepted (the known entries are well below 64 KiB).
pub const MAX_ENTRY_LEN: u32 = 16 << 20;
/// Zero bytes allowed between the last directory entry and the data (terminator + padding).
const MAX_DIR_PADDING: usize = 15;
/// Longest code prefix accepted; real codes stay far below (values < 2^25).
const MAX_PREFIX_BITS: u32 = 24;

/// Magics of the codec implemented here. The Eiketsuden files use `LS11`; the sequel's files
/// spell it `Ls11` with the same layout and codec.
const SUPPORTED_MAGICS: [&[u8; 4]; 2] = [b"LS11", b"Ls11"];
/// Sibling container variants whose codecs are not documented.
const VARIANT_MAGICS: [&[u8; 4]; 4] = [b"Ls10", b"Ls12", b"LS10", b"LS12"];

/// Whether `data` starts with a supported LS11 magic.
pub fn has_magic(data: &[u8]) -> bool {
    data.len() >= 4 && SUPPORTED_MAGICS.iter().any(|m| &data[..4] == *m)
}

/// Whether `data` starts with the magic of an LS container variant whose codec is not
/// implemented (`Ls10`, `Ls12`).
pub fn has_variant_magic(data: &[u8]) -> bool {
    data.len() >= 4 && VARIANT_MAGICS.iter().any(|m| &data[..4] == *m)
}

/// A structural problem in an archive or in one of its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ls11Error {
    /// Shorter than header + dictionary + a directory terminator.
    TooShort {
        len: usize,
    },
    BadMagic {
        found: [u8; 4],
    },
    /// An `Ls10` / `Ls12` container: same family, undocumented codec.
    UnsupportedVariant {
        magic: String,
    },
    /// Bytes 4..16 of the header are not zero.
    HeaderPaddingNotZero,
    /// The directory runs past the end of the file.
    TruncatedDirectory {
        at: usize,
    },
    /// The first entry's offset points into the directory itself.
    DataInsideDirectory {
        offset: u32,
        directory_end: usize,
    },
    /// `offset[i] != offset[i-1] + clen[i-1]`.
    BrokenChain {
        index: usize,
        expected: u64,
        found: u32,
    },
    /// An entry's data extends past the end of the file.
    EntryPastEnd {
        index: usize,
        end: u64,
        file_len: usize,
    },
    /// A decoded length above [`MAX_ENTRY_LEN`].
    EntryTooLarge {
        index: usize,
        dlen: u32,
    },
    /// Non-zero bytes, or more than a terminator's worth of bytes, between the directory and
    /// the data.
    BadDirectoryPadding {
        at: usize,
        len: usize,
    },
    /// The last entry does not end exactly at the end of the file.
    TrailingData {
        end: u64,
        file_len: usize,
    },
    /// No entry with this index.
    NoSuchEntry {
        index: usize,
        count: usize,
    },
    /// The bit stream of an entry is corrupt.
    Entry {
        index: usize,
        error: CodecError,
    },
}

impl fmt::Display for Ls11Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Ls11Error::*;
        match self {
            TooShort { len } => write!(f, "LS11: file too short ({len} bytes)"),
            BadMagic { found } => write!(f, "LS11: bad magic {}", crate::hex(found)),
            UnsupportedVariant { magic } => {
                write!(f, "LS container variant `{magic}` is not supported")
            }
            HeaderPaddingNotZero => write!(f, "LS11: header bytes 4..16 are not zero"),
            TruncatedDirectory { at } => {
                write!(f, "LS11: directory truncated at offset {at:#x}")
            }
            DataInsideDirectory {
                offset,
                directory_end,
            } => write!(
                f,
                "LS11: first entry offset {offset:#x} lies inside the directory (ends at {directory_end:#x})"
            ),
            BrokenChain {
                index,
                expected,
                found,
            } => write!(
                f,
                "LS11: entry {index} starts at {found:#x}, expected {expected:#x} (directory chain broken)"
            ),
            EntryPastEnd {
                index,
                end,
                file_len,
            } => write!(
                f,
                "LS11: entry {index} ends at {end:#x}, past the end of the file ({file_len:#x})"
            ),
            EntryTooLarge { index, dlen } => {
                write!(f, "LS11: entry {index} declares {dlen} decoded bytes (too large)")
            }
            BadDirectoryPadding { at, len } => write!(
                f,
                "LS11: {len} unexpected bytes between the directory and the data at {at:#x}"
            ),
            TrailingData { end, file_len } => write!(
                f,
                "LS11: entries end at {end:#x} but the file is {file_len:#x} bytes long"
            ),
            NoSuchEntry { index, count } => {
                write!(f, "LS11: no entry {index} (archive has {count})")
            }
            Entry { index, error } => write!(f, "LS11: entry {index}: {error}"),
        }
    }
}

impl std::error::Error for Ls11Error {}

/// A corrupt LS11 bit stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The stream ended before `expected` bytes were produced.
    UnexpectedEnd { produced: usize, expected: usize },
    /// A code prefix longer than any valid code.
    CodeTooLong { bit_offset: usize },
    /// A back-reference with distance 0.
    ZeroDistance { produced: usize },
    /// A back-reference further back than the output produced so far.
    DistanceOutOfRange { distance: u32, produced: usize },
    /// A back-reference that would produce more than the declared length.
    LengthOverrun {
        length: u32,
        produced: usize,
        expected: usize,
    },
    /// Whole bytes of input remain after the declared length was produced.
    UnconsumedInput { consumed: usize, len: usize },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CodecError::*;
        match self {
            UnexpectedEnd { produced, expected } => write!(
                f,
                "bit stream ended after {produced} of {expected} decoded bytes"
            ),
            CodeTooLong { bit_offset } => {
                write!(f, "code prefix too long at bit {bit_offset}")
            }
            ZeroDistance { produced } => {
                write!(f, "back-reference with distance 0 after {produced} bytes")
            }
            DistanceOutOfRange { distance, produced } => write!(
                f,
                "back-reference distance {distance} exceeds the {produced} bytes produced"
            ),
            LengthOverrun {
                length,
                produced,
                expected,
            } => write!(
                f,
                "back-reference of {length} bytes after {produced} bytes overruns the declared length {expected}"
            ),
            UnconsumedInput { consumed, len } => write!(
                f,
                "only {consumed} of {len} compressed bytes were consumed"
            ),
        }
    }
}

impl std::error::Error for CodecError {}

/// One directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Absolute offset of the entry's data.
    pub offset: u32,
    /// Stored (compressed) length.
    pub clen: u32,
    /// Decoded length.
    pub dlen: u32,
}

impl Entry {
    /// Stored without compression (`clen == dlen`).
    pub fn is_raw(&self) -> bool {
        self.clen == self.dlen
    }
}

/// A parsed archive borrowing the file's bytes. Construction validates the whole directory;
/// entries are decoded on demand.
#[derive(Debug, Clone)]
pub struct Archive<'a> {
    data: &'a [u8],
    dict: [u8; 256],
    entries: Vec<Entry>,
    data_start: usize,
}

fn be32(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

impl<'a> Archive<'a> {
    /// Parse and validate the header and the directory.
    pub fn parse(data: &'a [u8]) -> Result<Archive<'a>, Ls11Error> {
        if data.len() < 4 {
            return Err(Ls11Error::TooShort { len: data.len() });
        }
        if has_variant_magic(data) {
            return Err(Ls11Error::UnsupportedVariant {
                magic: String::from_utf8_lossy(&data[..4]).into_owned(),
            });
        }
        if !has_magic(data) {
            let mut found = [0u8; 4];
            found.copy_from_slice(&data[..4]);
            return Err(Ls11Error::BadMagic { found });
        }
        if data.len() < DIR_OFFSET + 4 {
            return Err(Ls11Error::TooShort { len: data.len() });
        }
        if data[4..HEADER_LEN].iter().any(|&b| b != 0) {
            return Err(Ls11Error::HeaderPaddingNotZero);
        }
        let mut dict = [0u8; 256];
        dict.copy_from_slice(&data[DICT_OFFSET..DICT_OFFSET + 256]);

        let mut entries: Vec<Entry> = Vec::new();
        let mut data_start: Option<usize> = None;
        let mut next: u64 = 0;
        let mut pos = DIR_OFFSET;
        loop {
            if let Some(start) = data_start {
                if pos + DIR_ENTRY_LEN > start {
                    break;
                }
            }
            if pos + 4 > data.len() {
                return Err(Ls11Error::TruncatedDirectory { at: pos });
            }
            let clen = be32(data, pos);
            if pos + DIR_ENTRY_LEN > data.len() {
                if clen == 0 {
                    break; // the zero terminator at the very end of an empty archive
                }
                return Err(Ls11Error::TruncatedDirectory { at: pos });
            }
            let dlen = be32(data, pos + 4);
            let offset = be32(data, pos + 8);
            if clen == 0 && dlen == 0 && offset == 0 {
                break;
            }
            let index = entries.len();
            if data_start.is_none() {
                if (offset as usize) < pos + DIR_ENTRY_LEN {
                    return Err(Ls11Error::DataInsideDirectory {
                        offset,
                        directory_end: pos + DIR_ENTRY_LEN,
                    });
                }
                data_start = Some(offset as usize);
                next = u64::from(offset);
            }
            if u64::from(offset) != next {
                return Err(Ls11Error::BrokenChain {
                    index,
                    expected: next,
                    found: offset,
                });
            }
            if dlen > MAX_ENTRY_LEN {
                return Err(Ls11Error::EntryTooLarge { index, dlen });
            }
            next = u64::from(offset) + u64::from(clen);
            if next > data.len() as u64 {
                return Err(Ls11Error::EntryPastEnd {
                    index,
                    end: next,
                    file_len: data.len(),
                });
            }
            entries.push(Entry { offset, clen, dlen });
            pos += DIR_ENTRY_LEN;
        }

        let data_start = match data_start {
            Some(start) => {
                let gap = &data[pos..start];
                if gap.len() > MAX_DIR_PADDING || gap.iter().any(|&b| b != 0) {
                    return Err(Ls11Error::BadDirectoryPadding {
                        at: pos,
                        len: gap.len(),
                    });
                }
                if next != data.len() as u64 {
                    return Err(Ls11Error::TrailingData {
                        end: next,
                        file_len: data.len(),
                    });
                }
                start
            }
            None => {
                let rest = &data[pos..];
                if rest.len() > MAX_DIR_PADDING || rest.iter().any(|&b| b != 0) {
                    return Err(Ls11Error::BadDirectoryPadding {
                        at: pos,
                        len: rest.len(),
                    });
                }
                data.len()
            }
        };
        Ok(Archive {
            data,
            dict,
            entries,
            data_start,
        })
    }

    /// The directory, in file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` for an archive without entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Offset of the first entry's data (the end of the directory and its terminator).
    pub fn data_start(&self) -> usize {
        self.data_start
    }

    /// The shared byte dictionary.
    pub fn dictionary(&self) -> &[u8; 256] {
        &self.dict
    }

    /// Whether the dictionary is a permutation of all 256 byte values, as in the known files.
    /// Informational: decoding does not depend on it.
    pub fn dictionary_is_permutation(&self) -> bool {
        let mut seen = [false; 256];
        for &b in &self.dict {
            if std::mem::replace(&mut seen[b as usize], true) {
                return false;
            }
        }
        true
    }

    /// The stored bytes of entry `index`.
    pub fn stored(&self, index: usize) -> Result<&'a [u8], Ls11Error> {
        let e = self.entry(index)?;
        let start = e.offset as usize;
        Ok(&self.data[start..start + e.clen as usize])
    }

    fn entry(&self, index: usize) -> Result<Entry, Ls11Error> {
        self.entries
            .get(index)
            .copied()
            .ok_or(Ls11Error::NoSuchEntry {
                index,
                count: self.entries.len(),
            })
    }

    /// Decode entry `index` to exactly its declared length.
    pub fn decode(&self, index: usize) -> Result<Vec<u8>, Ls11Error> {
        let e = self.entry(index)?;
        let stored = self.stored(index)?;
        if e.is_raw() {
            return Ok(stored.to_vec());
        }
        decode_stream(&self.dict, stored, e.dlen as usize)
            .map_err(|error| Ls11Error::Entry { index, error })
    }

    /// Decode every entry, stopping at the first error.
    pub fn decode_all(&self) -> Result<Vec<Vec<u8>>, Ls11Error> {
        (0..self.entries.len()).map(|i| self.decode(i)).collect()
    }
}

// ----- bit stream ----------------------------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    /// Next bit, counted from the start of `data`.
    pos: usize,
}

impl BitReader<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.pos / 8)?;
        let bit = (byte >> (7 - self.pos % 8)) & 1;
        self.pos += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Some(v)
    }

    /// Bytes touched so far (a partially read last byte counts).
    fn bytes_consumed(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

/// Read one variable-length code; `None` at the end of the input.
fn read_code(bits: &mut BitReader<'_>) -> Result<Option<u32>, CodecError> {
    let start = bits.pos;
    let mut prefix = 0u32;
    let mut width = 0u32;
    loop {
        let Some(bit) = bits.bit() else {
            return Ok(None);
        };
        prefix = (prefix << 1) | bit;
        width += 1;
        if bit == 0 {
            break;
        }
        if width >= MAX_PREFIX_BITS {
            return Err(CodecError::CodeTooLong { bit_offset: start });
        }
    }
    Ok(bits.bits(width).map(|segment| prefix + segment))
}

/// Decode an LS11 bit stream into exactly `expected` bytes. The whole input must be consumed
/// (up to the padding bits of its last byte).
pub fn decode_stream(
    dict: &[u8; 256],
    input: &[u8],
    expected: usize,
) -> Result<Vec<u8>, CodecError> {
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    let mut bits = BitReader {
        data: input,
        pos: 0,
    };
    let end = |out: &Vec<u8>| CodecError::UnexpectedEnd {
        produced: out.len(),
        expected,
    };
    while out.len() < expected {
        let code = read_code(&mut bits)?.ok_or_else(|| end(&out))?;
        if code < 256 {
            out.push(dict[code as usize]);
            continue;
        }
        let distance = code - 256;
        if distance == 0 {
            return Err(CodecError::ZeroDistance {
                produced: out.len(),
            });
        }
        if distance as usize > out.len() {
            return Err(CodecError::DistanceOutOfRange {
                distance,
                produced: out.len(),
            });
        }
        let length = read_code(&mut bits)?.ok_or_else(|| end(&out))? + 3;
        if out.len() + length as usize > expected {
            return Err(CodecError::LengthOverrun {
                length,
                produced: out.len(),
                expected,
            });
        }
        let from = out.len() - distance as usize;
        for i in 0..length as usize {
            let b = out[from + i];
            out.push(b);
        }
    }
    let consumed = bits.bytes_consumed();
    if consumed != input.len() {
        return Err(CodecError::UnconsumedInput {
            consumed,
            len: input.len(),
        });
    }
    Ok(out)
}

// ----- encoder -------------------------------------------------------------------------------

struct BitWriter {
    out: Vec<u8>,
    used: u32,
}

impl BitWriter {
    fn push(&mut self, bit: bool) {
        if self.used == 0 {
            self.out.push(0);
        }
        if bit {
            *self.out.last_mut().expect("a byte was pushed") |= 0x80 >> self.used;
        }
        self.used = (self.used + 1) % 8;
    }

    fn code(&mut self, value: u32) {
        let width = code_width(value);
        for _ in 1..width {
            self.push(true);
        }
        self.push(false);
        let segment = value + 2 - (1 << width);
        for i in (0..width).rev() {
            self.push((segment >> i) & 1 == 1);
        }
    }
}

/// Prefix width `k` of a code: `2^k − 2 <= value <= 2^(k+1) − 3`. The code takes `2k` bits.
fn code_width(value: u32) -> u32 {
    31 - (value + 2).leading_zeros()
}

fn code_bits(value: u32) -> usize {
    2 * code_width(value) as usize
}

/// Frequency-sorted dictionary for `entries` (most frequent byte first, ties by byte value):
/// a permutation of all 256 values, like the dictionaries of the known files.
pub fn frequency_dictionary<'a>(entries: impl IntoIterator<Item = &'a [u8]>) -> [u8; 256] {
    let mut counts = [0u64; 256];
    for data in entries {
        for &b in data {
            counts[b as usize] += 1;
        }
    }
    let mut order: Vec<u8> = (0..=255).collect();
    order.sort_by_key(|&b| std::cmp::Reverse(counts[b as usize]));
    let mut dict = [0u8; 256];
    dict.copy_from_slice(&order);
    dict
}

const HASH_BITS: u32 = 15;
const WINDOW: usize = 1 << 15;
const MAX_CHAIN: usize = 64;
const MAX_MATCH: usize = 1 << 12;
const MIN_MATCH: usize = 3;

fn hash3(data: &[u8], at: usize) -> usize {
    let v = (u32::from(data[at]) << 16) | (u32::from(data[at + 1]) << 8) | u32::from(data[at + 2]);
    (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Encode `data` as an LS11 bit stream using `dict` (which must contain every byte of `data`).
/// Greedy LZ77 with a hash chain; a back-reference is used only when it is cheaper than the
/// literals it replaces. Decodes back with [`decode_stream`].
///
/// # Panics
/// If a byte of `data` is missing from `dict`.
pub fn encode_stream(dict: &[u8; 256], data: &[u8]) -> Vec<u8> {
    let mut code_of = [u32::MAX; 256];
    for (i, &b) in dict.iter().enumerate() {
        if code_of[b as usize] == u32::MAX {
            code_of[b as usize] = i as u32;
        }
    }
    let literal = |b: u8| {
        let c = code_of[b as usize];
        assert!(c != u32::MAX, "byte {b:#04x} is not in the dictionary");
        c
    };

    let mut w = BitWriter {
        out: Vec::new(),
        used: 0,
    };
    let mut head = vec![usize::MAX; 1 << HASH_BITS];
    let mut prev = vec![usize::MAX; data.len()];
    let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, at: usize| {
        if at + MIN_MATCH <= data.len() {
            let h = hash3(data, at);
            prev[at] = head[h];
            head[h] = at;
        }
    };

    let mut i = 0;
    while i < data.len() {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + MIN_MATCH <= data.len() {
            let max_len = (data.len() - i).min(MAX_MATCH);
            let mut candidate = head[hash3(data, i)];
            let mut chain = 0;
            while candidate != usize::MAX && chain < MAX_CHAIN && i - candidate <= WINDOW {
                let len = (0..max_len)
                    .take_while(|&k| data[candidate + k] == data[i + k])
                    .count();
                if len > best_len {
                    best_len = len;
                    best_dist = i - candidate;
                }
                candidate = prev[candidate];
                chain += 1;
            }
        }
        if best_len >= MIN_MATCH {
            let match_cost = code_bits(256 + best_dist as u32) + code_bits(best_len as u32 - 3);
            let literal_cost: usize = data[i..i + best_len]
                .iter()
                .map(|&b| code_bits(literal(b)))
                .sum();
            if match_cost < literal_cost {
                w.code(256 + best_dist as u32);
                w.code(best_len as u32 - 3);
                for at in i..i + best_len {
                    insert(&mut head, &mut prev, at);
                }
                i += best_len;
                continue;
            }
        }
        w.code(literal(data[i]));
        insert(&mut head, &mut prev, i);
        i += 1;
    }
    w.out
}

/// Write an LS11 archive holding `entries`, in the layout of the known files: frequency-sorted
/// dictionary, directory with a 4-byte zero terminator, chained data. An entry is stored
/// compressed when that is smaller, raw otherwise (a compressed stream is never allowed to be
/// exactly as long as the raw data, which would read back as "raw").
pub fn build(entries: &[&[u8]]) -> Vec<u8> {
    let dict = frequency_dictionary(entries.iter().copied());
    let stored: Vec<(Vec<u8>, u32)> = entries
        .iter()
        .map(|data| {
            let packed = encode_stream(&dict, data);
            let dlen = u32::try_from(data.len()).expect("entry fits in u32");
            if packed.len() < data.len() {
                (packed, dlen)
            } else {
                (data.to_vec(), dlen)
            }
        })
        .collect();
    assemble(&dict, &stored)
}

/// Lay out an archive from already stored entries `(stored bytes, decoded length)`.
fn assemble(dict: &[u8; 256], stored: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"LS11");
    out.resize(HEADER_LEN, 0);
    out.extend_from_slice(dict);
    let mut offset = DIR_OFFSET + stored.len() * DIR_ENTRY_LEN + 4;
    for (bytes, dlen) in stored {
        let clen = u32::try_from(bytes.len()).expect("entry fits in u32");
        out.extend_from_slice(&clen.to_be_bytes());
        out.extend_from_slice(&dlen.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        offset += bytes.len();
    }
    out.extend_from_slice(&[0; 4]);
    for (bytes, _) in stored {
        out.extend_from_slice(bytes);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random bytes (xorshift), `alphabet` distinct values.
    fn noise(len: usize, seed: u32, alphabet: u32) -> Vec<u8> {
        let mut x = seed.max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x % alphabet) as u8
            })
            .collect()
    }

    fn sample_text() -> Vec<u8> {
        b"The quick brown fox jumps over the lazy dog. "
            .repeat(20)
            .into_iter()
            .chain(noise(300, 7, 256))
            .chain(vec![0u8; 500])
            .collect()
    }

    #[test]
    fn codes_have_the_documented_ranges() {
        for (value, width) in [(0, 1), (1, 1), (2, 2), (5, 2), (6, 3), (13, 3), (14, 4)] {
            assert_eq!(code_width(value), width, "value {value}");
        }
        // Bit patterns: 0 -> "00", 2 -> "1000", 6 -> "110000", 13 -> "110111".
        let mut w = BitWriter {
            out: Vec::new(),
            used: 0,
        };
        for v in [0, 2, 6, 13] {
            w.code(v);
        }
        // 00 1000 110000 110111 -> 0010 0011 0000 1101 11(00 0000)
        assert_eq!(w.out, vec![0b0010_0011, 0b0000_1101, 0b1100_0000]);
        let mut r = BitReader {
            data: &w.out,
            pos: 0,
        };
        for v in [0, 2, 6, 13] {
            assert_eq!(read_code(&mut r), Ok(Some(v)));
        }
    }

    #[test]
    fn stream_round_trips() {
        for data in [
            Vec::new(),
            vec![42u8],
            sample_text(),
            noise(5000, 3, 4),
            noise(5000, 11, 256),
            vec![7u8; 70_000],
        ] {
            let dict = frequency_dictionary([data.as_slice()]);
            let packed = encode_stream(&dict, &data);
            assert_eq!(
                decode_stream(&dict, &packed, data.len()).as_deref(),
                Ok(data.as_slice())
            );
        }
    }

    #[test]
    fn compressible_data_shrinks() {
        let data = sample_text();
        let dict = frequency_dictionary([data.as_slice()]);
        assert!(encode_stream(&dict, &data).len() < data.len() / 2);
    }

    #[test]
    fn archive_round_trips() {
        let a = sample_text();
        let b = noise(1000, 5, 256); // incompressible: stored raw
        let c: Vec<u8> = Vec::new();
        let d = vec![1u8, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3];
        let file = build(&[&a, &b, &c, &d]);
        let archive = Archive::parse(&file).unwrap();
        assert_eq!(archive.len(), 4);
        assert_eq!(archive.data_start(), DIR_OFFSET + 4 * 12 + 4);
        assert!(archive.dictionary_is_permutation());
        assert!(!archive.entries()[0].is_raw());
        assert!(archive.entries()[1].is_raw());
        assert_eq!(archive.decode_all().unwrap(), vec![a, b, c, d]);
    }

    #[test]
    fn known_data_starts() {
        // A single entry starts at 0x120 and 181 entries at 0x990, as documented for the
        // Korean files (directory + 4-byte terminator).
        let one = build(&[b"hello hello hello"]);
        assert_eq!(Archive::parse(&one).unwrap().data_start(), 0x120);
        let blobs: Vec<Vec<u8>> = (0..181).map(|i| vec![i as u8; 8]).collect();
        let refs: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
        let many = build(&refs);
        let archive = Archive::parse(&many).unwrap();
        assert_eq!((archive.len(), archive.data_start()), (181, 0x990));
    }

    #[test]
    fn empty_archive() {
        let file = build(&[]);
        assert_eq!(file.len(), DIR_OFFSET + 4);
        let archive = Archive::parse(&file).unwrap();
        assert!(archive.is_empty());
        assert_eq!(
            archive.decode(0),
            Err(Ls11Error::NoSuchEntry { index: 0, count: 0 })
        );
    }

    fn set_be32(file: &mut [u8], at: usize, v: u32) {
        file[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    #[test]
    fn header_errors() {
        assert_eq!(
            Archive::parse(b"LS1").unwrap_err(),
            Ls11Error::TooShort { len: 3 }
        );
        assert!(matches!(
            Archive::parse(b"MZ\x90\x00 not an archive").unwrap_err(),
            Ls11Error::BadMagic { .. }
        ));
        assert_eq!(
            Archive::parse(b"Ls12....").unwrap_err(),
            Ls11Error::UnsupportedVariant {
                magic: "Ls12".into()
            }
        );
        let mut file = build(&[b"abc"]);
        file[9] = 1;
        assert_eq!(
            Archive::parse(&file).unwrap_err(),
            Ls11Error::HeaderPaddingNotZero
        );
        let file = build(&[b"abc"]);
        assert!(matches!(
            Archive::parse(&file[..0x100]).unwrap_err(),
            Ls11Error::TooShort { .. }
        ));
        assert!(matches!(
            Archive::parse(&file[..0x118]).unwrap_err(),
            Ls11Error::TruncatedDirectory { at: 0x110 }
        ));
    }

    #[test]
    fn directory_errors() {
        let good = build(&[b"first entry", b"second entry"]);

        let mut f = good.clone();
        set_be32(&mut f, DIR_OFFSET + 8, 0x112);
        assert!(matches!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::DataInsideDirectory { offset: 0x112, .. }
        ));

        let mut f = good.clone();
        let second = be32(&f, DIR_OFFSET + 12 + 8);
        set_be32(&mut f, DIR_OFFSET + 12 + 8, second + 1);
        assert_eq!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::BrokenChain {
                index: 1,
                expected: u64::from(second),
                found: second + 1
            }
        );

        let mut f = good.clone();
        f.push(0);
        assert!(matches!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::TrailingData { .. }
        ));

        let f = &good[..good.len() - 1];
        assert!(matches!(
            Archive::parse(f).unwrap_err(),
            Ls11Error::EntryPastEnd { index: 1, .. }
        ));

        let mut f = good.clone();
        set_be32(&mut f, DIR_OFFSET + 4, MAX_ENTRY_LEN + 1);
        assert!(matches!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::EntryTooLarge { index: 0, .. }
        ));

        // A non-zero terminator.
        let mut f = good.clone();
        f[DIR_OFFSET + 24 + 3] = 1;
        assert!(matches!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::BadDirectoryPadding { .. }
        ));

        // Garbage after an empty directory.
        let mut f = build(&[]);
        f.extend_from_slice(&[0; 20]);
        assert!(matches!(
            Archive::parse(&f).unwrap_err(),
            Ls11Error::BadDirectoryPadding { .. }
        ));
    }

    #[test]
    fn longer_zero_padding_is_accepted() {
        // Terminator plus padding up to 15 zero bytes (the sequel's files pad this way).
        let data = b"padded padded padded".to_vec();
        let dict = frequency_dictionary([data.as_slice()]);
        let mut f = assemble(&dict, &[(data.clone(), data.len() as u32)]);
        let insert_at = DIR_OFFSET + 12 + 4;
        for _ in 0..8 {
            f.insert(insert_at, 0);
        }
        set_be32(&mut f, DIR_OFFSET + 8, (insert_at + 8) as u32);
        let archive = Archive::parse(&f).unwrap();
        assert_eq!(archive.decode(0).unwrap(), data);
    }

    /// An archive with one compressed entry built from a hand-made stream.
    fn with_stream(stream: Vec<u8>, dlen: u32) -> Vec<u8> {
        let dict: [u8; 256] = std::array::from_fn(|i| i as u8);
        assemble(&dict, &[(stream, dlen)])
    }

    fn codes(values: &[u32]) -> Vec<u8> {
        let mut w = BitWriter {
            out: Vec::new(),
            used: 0,
        };
        for &v in values {
            w.code(v);
        }
        w.out
    }

    fn entry_error(file: &[u8]) -> CodecError {
        match Archive::parse(file).unwrap().decode(0).unwrap_err() {
            Ls11Error::Entry { index: 0, error } => error,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hand_made_stream_decodes() {
        // "ab" then copy 8 bytes from distance 2 -> "ababababab" (overlapping copy). The
        // stream is 6 bytes, so it must not be mistaken for raw data (clen != dlen).
        let stream = codes(&[b'a' as u32, b'b' as u32, 258, 5]);
        assert_eq!(stream.len(), 6);
        let f = with_stream(stream, 10);
        assert_eq!(
            Archive::parse(&f).unwrap().decode(0).unwrap(),
            b"ababababab"
        );
    }

    #[test]
    fn stream_errors() {
        // Too few codes for the declared length (these three codes fill exactly one byte).
        let f = with_stream(codes(&[1, 2, 0]), 5);
        assert_eq!(
            entry_error(&f),
            CodecError::UnexpectedEnd {
                produced: 3,
                expected: 5
            }
        );
        // Distance 0.
        let f = with_stream(codes(&[1, 256, 0]), 4);
        assert_eq!(entry_error(&f), CodecError::ZeroDistance { produced: 1 });
        // Distance beyond the output.
        let f = with_stream(codes(&[1, 256 + 2, 0]), 4);
        assert_eq!(
            entry_error(&f),
            CodecError::DistanceOutOfRange {
                distance: 2,
                produced: 1
            }
        );
        // A copy longer than the declared length.
        let f = with_stream(codes(&[1, 257, 5]), 4);
        assert_eq!(
            entry_error(&f),
            CodecError::LengthOverrun {
                length: 8,
                produced: 1,
                expected: 4
            }
        );
        // Extra bytes after the last code (two, so that clen != dlen).
        let mut stream = codes(&[1, 2, 3]);
        stream.extend_from_slice(&[0, 0]);
        let f = with_stream(stream, 3);
        assert_eq!(
            entry_error(&f),
            CodecError::UnconsumedInput {
                consumed: 2,
                len: 4
            }
        );
        // All one bits: a prefix longer than any code.
        let f = with_stream(vec![0xff; 8], 3);
        assert_eq!(entry_error(&f), CodecError::CodeTooLong { bit_offset: 0 });
    }

    #[test]
    fn corrupted_streams_never_panic() {
        let data = sample_text();
        let file = build(&[&data]);
        let archive = Archive::parse(&file).unwrap();
        let e = archive.entries()[0];
        assert!(!e.is_raw());
        for i in 0..e.clen as usize {
            for flip in [0x01u8, 0x80, 0xff] {
                let mut f = file.clone();
                f[e.offset as usize + i] ^= flip;
                // Either an error or (rarely) other bytes of the right length; never a panic.
                if let Ok(out) = Archive::parse(&f).unwrap().decode(0) {
                    assert_eq!(out.len(), data.len());
                }
            }
        }
    }
}
