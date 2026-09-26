//! TF-DCE, the image compression of the DOS/V builds' portraits (`FACEDAT.R3`) and common
//! screens (`PACKGRP.R3`).
//!
//! The game expands these images with a resident driver, `TFDED.COM` ("DOS/V Series
//! TF-graphic Data Expand Driver", TF-DCE 5.11), which `MAIN.EXE` calls through `int 62h`
//! with four planes enabled. The driver writes straight into VGA memory, one bit-plane at a
//! time; this decoder reproduces that into a plain buffer. It was written from a static reading
//! of the driver, not from any other implementation.
//!
//! # Layout
//!
//! ```text
//! u8        header length L; the L-1 bytes after it are skipped (a one-letter tag "T")
//! u8        width in bytes (8 pixels each)
//! u16le     height in rows
//! u16le     plane methods: one nibble per processed plane, low nibble first
//! u8        plane order: 2-bit plane numbers, low bits first (processing order)
//! u8        flags: 0x40 = 32 bytes follow (a palette the driver skips),
//!                  0x20 = dictionary follows: u8 count, count × u16le words
//! ...       command streams of the planes that use method 3, back to back
//! ```
//!
//! Planes are processed in the order given; each consumes the next method nibble:
//!
//! | method | effect |
//! |---|---|
//! | 0 | plane 3: cleared to 0; other planes: left untouched (an error here: undefined) |
//! | 1 | whole plane filled with the next byte |
//! | 2 | copy of the plane at the order position given by the next byte |
//! | 3 | command stream (below) |
//!
//! # Command stream
//!
//! A plane of `W` byte columns and `H` rows is written one byte at a time in **serpentine
//! column order**: column 0 top to bottom, column 1 bottom to top, and so on. The stream ends
//! when the last byte of the plane is written (a command cut short by the end is abandoned).
//! `n` is the low nibble of the command, `b` the next stream byte.
//!
//! | command | effect |
//! |---|---|
//! | `00`–`1F` | dictionary word `d = dict[cmd]`: command `d & 0xFF` with its byte argument `d >> 8` taken from the word instead of the stream (see below) |
//! | `2n b` | copy `n + 3` bytes from `b` positions back in serpentine order |
//! | `3n b` | `b + 2` bytes from the same position of the plane at order position `n >> 2`, transformed by `n & 3`: 0 copy, 1 NOT, 2 rotate right 1, 3 rotate left 1 |
//! | `4n b` | copy `b + 2` bytes from `n + 1` byte columns to the left (same row) |
//! | `5n` / `9n` | masked copy from another plane, see below; `9n` rotates the mask per byte |
//! | `6n b1 b2` | `n + 1` times the pair `b1 b2` |
//! | `7n` | `n + 1` literal bytes |
//! | `8n b` | `n + 1` times the pair `L·0x11`, `H·0x11`, where `L` / `H` are the low / high nibble of `b` |
//! | `A0`–`FF` `b` | `cmd - 0x9E` (2–97) times the byte `b` |
//!
//! Masked copies write `source & mask` from the same position of the plane at an order
//! position. Four recent masks are kept, starting as `00 FF 55 AA`. For `n >> 2` < 3 the
//! source is order position `n >> 2`, the mask is recent mask `n & 3`, which then swaps places
//! with the one before it, and `b + 2` bytes follow from the count byte `b`. For `n >> 2` = 3 the
//! source is order position `n & 3`, the next byte is a new mask (stored as recent mask 2, the
//! old one moving to 3), then the count byte. `9n` rotates the mask left after each byte: by 1
//! for `55` / `AA`, by 2 otherwise (a dither).
//!
//! Dictionary words dispatch on the high nibble of their low byte: `2` linear copy (argument =
//! distance), `3` plane transform, `4` column copy, `5` / `9` masked copy (argument = count;
//! selector 3 is plain order position 3, no new mask), `7` one literal byte (the argument),
//! `8` nibble pair, `A`–`F` fill with the argument; `0`, `1` and `6` do nothing.
//!
//! The output is 4 bpp planar, plane-major (plane `p` = bit `p` of the colour index), rows of
//! `W` bytes: the layout [`crate::planar::decode`] reads.

use std::fmt;

/// Flag bit: 32 palette bytes follow the flags (skipped by the driver).
pub const FLAG_PALETTE: u8 = 0x40;
/// Flag bit: a dictionary follows the flags.
pub const FLAG_DICTIONARY: u8 = 0x20;
/// Recent masks at the start of every image.
pub const INITIAL_MASKS: [u8; 4] = [0x00, 0xFF, 0x55, 0xAA];
/// Bytes of the skipped palette block.
pub const PALETTE_BYTES: usize = 32;

/// Why a TF-DCE stream could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TfdceError {
    /// The data ended at byte `at`.
    Truncated { at: usize },
    /// A zero width or height.
    BadGeometry { width_bytes: usize, height: usize },
    /// A plane method other than 0–3 (or 0 on a plane other than 3).
    BadPlaneMethod { plane: u8, method: u8 },
    /// A plane is never written (method 0 on planes 0–2, or missing from the plane order).
    PlaneLeftUndefined { plane: u8 },
    /// A dictionary command beyond the dictionary.
    DictionaryIndex { at: usize, index: u8, len: usize },
    /// A command reads a plane that is not decoded yet (or the plane being decoded).
    UndefinedSource { at: usize, plane: u8 },
    /// A back-reference before the start of the plane or left of the image.
    BadBackReference { at: usize, distance: usize },
    /// Bytes left after every plane was written.
    TrailingBytes { consumed: usize, len: usize },
}

impl fmt::Display for TfdceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TfdceError::*;
        match self {
            Truncated { at } => write!(f, "TF-DCE: data ends at byte {at}"),
            BadGeometry {
                width_bytes,
                height,
            } => write!(
                f,
                "TF-DCE: image of {width_bytes} bytes × {height} rows is empty"
            ),
            BadPlaneMethod { plane, method } => {
                write!(f, "TF-DCE: plane {plane} uses unknown method {method}")
            }
            PlaneLeftUndefined { plane } => {
                write!(f, "TF-DCE: plane {plane} is never written")
            }
            DictionaryIndex { at, index, len } => write!(
                f,
                "TF-DCE: byte {at}: dictionary word {index} of a {len}-word dictionary"
            ),
            UndefinedSource { at, plane } => write!(
                f,
                "TF-DCE: byte {at}: reads plane {plane} before it is decoded"
            ),
            BadBackReference { at, distance } => write!(
                f,
                "TF-DCE: byte {at}: back-reference of {distance} reaches outside the plane"
            ),
            TrailingBytes { consumed, len } => {
                write!(f, "TF-DCE: image complete after {consumed} of {len} bytes")
            }
        }
    }
}

impl std::error::Error for TfdceError {}

/// The header of a TF-DCE image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The bytes skipped after the header length byte (`b"T"` in the known files).
    pub tag: Vec<u8>,
    pub width_bytes: usize,
    pub height: usize,
    /// Plane methods, one nibble per processed plane, low nibble first.
    pub plane_methods: u16,
    /// Plane numbers in processing order.
    pub plane_order: [u8; 4],
    pub flags: u8,
    /// The palette block, when [`FLAG_PALETTE`] is set (not used by the driver).
    pub palette: Option<Vec<u8>>,
    pub dictionary: Vec<u16>,
    /// Offset of the first command stream.
    pub data_start: usize,
}

impl Header {
    /// Width in pixels.
    pub fn width(&self) -> usize {
        self.width_bytes * 8
    }
}

/// A decoded image: 4 bpp planar, plane-major, rows of `width / 8` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels (a multiple of 8).
    pub width: usize,
    pub height: usize,
    pub planar: Vec<u8>,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, TfdceError> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or(TfdceError::Truncated { at: self.pos })?;
        self.pos += 1;
        Ok(b)
    }

    fn word(&mut self) -> Result<u16, TfdceError> {
        let lo = self.byte()?;
        Ok(u16::from_le_bytes([lo, self.byte()?]))
    }

    fn take(&mut self, n: usize) -> Result<&[u8], TfdceError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(TfdceError::Truncated {
                at: self.data.len(),
            });
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
}

/// Parse the header.
pub fn parse_header(data: &[u8]) -> Result<Header, TfdceError> {
    let mut r = Reader { data, pos: 0 };
    let header_len = usize::from(r.byte()?);
    let tag = r.take(header_len.saturating_sub(1))?.to_vec();
    let width_bytes = usize::from(r.byte()?);
    let height = usize::from(r.word()?);
    if width_bytes == 0 || height == 0 {
        return Err(TfdceError::BadGeometry {
            width_bytes,
            height,
        });
    }
    let plane_methods = r.word()?;
    let order = r.byte()?;
    let plane_order = std::array::from_fn(|i| (order >> (2 * i)) & 3);
    let flags = r.byte()?;
    let palette = if flags & FLAG_PALETTE != 0 {
        Some(r.take(PALETTE_BYTES)?.to_vec())
    } else {
        None
    };
    let mut dictionary = Vec::new();
    if flags & FLAG_DICTIONARY != 0 {
        let count = r.byte()?;
        for _ in 0..count {
            dictionary.push(r.word()?);
        }
    }
    Ok(Header {
        tag,
        width_bytes,
        height,
        plane_methods,
        plane_order,
        flags,
        palette,
        dictionary,
        data_start: r.pos,
    })
}

/// Decode an image. The whole input must be consumed.
pub fn decode(data: &[u8]) -> Result<Image, TfdceError> {
    let header = parse_header(data)?;
    let mut d = Decoder {
        r: Reader {
            data,
            pos: header.data_start,
        },
        w: header.width_bytes,
        h: header.height,
        planes: vec![0; 4 * header.width_bytes * header.height],
        defined: [false; 4],
        order: header.plane_order,
        masks: INITIAL_MASKS,
        dictionary: &header.dictionary,
    };
    let mut methods = header.plane_methods;
    for plane in header.plane_order {
        let method = (methods & 0xF) as u8;
        methods >>= 4;
        d.run_plane(plane, method)?;
    }
    if let Some(plane) = (0..4u8).find(|&p| !d.defined[usize::from(p)]) {
        return Err(TfdceError::PlaneLeftUndefined { plane });
    }
    if d.r.pos != data.len() {
        return Err(TfdceError::TrailingBytes {
            consumed: d.r.pos,
            len: data.len(),
        });
    }
    Ok(Image {
        width: header.width(),
        height: header.height,
        planar: d.planes,
    })
}

/// What a command writes, byte by byte.
enum Run {
    /// One byte, `count` times.
    Fill(u8, usize),
    /// `count` bytes read from the stream as they are written.
    Literal(usize),
    /// A pair of bytes, alternating, `count` bytes in all.
    Pair([u8; 2], usize),
    /// From `columns` byte columns to the left.
    Columns { columns: usize, count: usize },
    /// From `distance` positions back in writing order.
    Back { distance: usize, count: usize },
    /// From another plane, transformed.
    Transform { source: u8, op: u8, count: usize },
    /// From another plane under a mask, rotated left by `rotate` after each byte.
    Masked {
        source: u8,
        mask: u8,
        rotate: u32,
        count: usize,
    },
}

struct Decoder<'a> {
    r: Reader<'a>,
    w: usize,
    h: usize,
    planes: Vec<u8>,
    defined: [bool; 4],
    order: [u8; 4],
    masks: [u8; 4],
    dictionary: &'a [u16],
}

impl Decoder<'_> {
    fn plane_len(&self) -> usize {
        self.w * self.h
    }

    /// Row-major index within a plane of serpentine position `p`.
    fn index(&self, p: usize) -> usize {
        let (column, k) = (p / self.h, p % self.h);
        let row = if column % 2 == 0 { k } else { self.h - 1 - k };
        row * self.w + column
    }

    fn run_plane(&mut self, plane: u8, method: u8) -> Result<(), TfdceError> {
        let len = self.plane_len();
        let base = usize::from(plane) * len;
        match method {
            0 if plane == 3 => self.planes[base..base + len].fill(0),
            1 => {
                let v = self.r.byte()?;
                self.planes[base..base + len].fill(v);
            }
            2 => {
                let at = self.r.pos;
                let source = self.order[usize::from(self.r.byte()? & 3)];
                self.require(source, plane, at)?;
                let src = usize::from(source) * len;
                self.planes.copy_within(src..src + len, base);
            }
            3 => self.command_stream(plane)?,
            _ => return Err(TfdceError::BadPlaneMethod { plane, method }),
        }
        self.defined[usize::from(plane)] = true;
        Ok(())
    }

    /// `source` must be a decoded plane other than the one being written.
    fn require(&self, source: u8, current: u8, at: usize) -> Result<(), TfdceError> {
        if source == current || !self.defined[usize::from(source)] {
            return Err(TfdceError::UndefinedSource { at, plane: source });
        }
        Ok(())
    }

    fn command_stream(&mut self, plane: u8) -> Result<(), TfdceError> {
        let total = self.plane_len();
        let mut p = 0;
        while p < total {
            let at = self.r.pos;
            let op = self.r.byte()?;
            let (cmd, arg) =
                if op < 0x20 {
                    let word = *self.dictionary.get(usize::from(op)).ok_or(
                        TfdceError::DictionaryIndex {
                            at,
                            index: op,
                            len: self.dictionary.len(),
                        },
                    )?;
                    let [lo, hi] = word.to_le_bytes();
                    (lo, Some(hi))
                } else {
                    (op, None)
                };
            let Some(run) = self.command(cmd, arg, plane, at)? else {
                continue;
            };
            p = self.execute(run, plane, p, at)?;
        }
        Ok(())
    }

    /// The run of command `cmd` (`arg` for a dictionary word); `None` for a no-op word.
    fn command(
        &mut self,
        cmd: u8,
        arg: Option<u8>,
        plane: u8,
        at: usize,
    ) -> Result<Option<Run>, TfdceError> {
        let n = cmd & 0x0F;
        let low = usize::from(n);
        let run = match (cmd >> 4, arg) {
            (0 | 1 | 6, Some(_)) => return Ok(None),
            (0xA..=0xF, _) => Run::Fill(self.arg(arg)?, usize::from(cmd) - 0x9E),
            (7, None) => Run::Literal(low + 1),
            (7, Some(a)) => Run::Fill(a, 1),
            (8, _) => {
                let b = self.arg(arg)?;
                Run::Pair([(b & 0x0F) * 0x11, (b >> 4) * 0x11], 2 * (low + 1))
            }
            (6, None) => {
                let first = self.r.byte()?;
                Run::Pair([first, self.r.byte()?], 2 * (low + 1))
            }
            (4, _) => Run::Columns {
                columns: low + 1,
                count: usize::from(self.arg(arg)?) + 2,
            },
            (2, _) => Run::Back {
                distance: usize::from(self.arg(arg)?),
                count: low + 3,
            },
            (3, _) => Run::Transform {
                source: self.order[low >> 2],
                op: n & 3,
                count: usize::from(self.arg(arg)?) + 2,
            },
            (hi @ (5 | 9), _) => {
                let (source, mask) = if arg.is_none() && low >> 2 == 3 {
                    let mask = self.r.byte()?;
                    self.masks[3] = self.masks[2];
                    self.masks[2] = mask;
                    (self.order[low & 3], mask)
                } else {
                    let i = low & 3;
                    let mask = self.masks[i];
                    if i > 0 {
                        self.masks.swap(i - 1, i);
                    }
                    (self.order[low >> 2], mask)
                };
                let rotate = match (hi, mask) {
                    (5, _) => 0,
                    (_, 0x55 | 0xAA) => 1,
                    _ => 2,
                };
                Run::Masked {
                    source,
                    mask,
                    rotate,
                    count: usize::from(self.arg(arg)?) + 2,
                }
            }
            // Commands 0x00–0x1F are always dictionary words, so this is unreachable for
            // direct commands; a dictionary word cannot name another dictionary word.
            _ => return Ok(None),
        };
        if let Run::Transform { source, .. } | Run::Masked { source, .. } = run {
            self.require(source, plane, at)?;
        }
        Ok(Some(run))
    }

    fn arg(&mut self, arg: Option<u8>) -> Result<u8, TfdceError> {
        match arg {
            Some(a) => Ok(a),
            None => self.r.byte(),
        }
    }

    /// Write `run` from serpentine position `p`; returns the position after it.
    fn execute(
        &mut self,
        run: Run,
        plane: u8,
        mut p: usize,
        at: usize,
    ) -> Result<usize, TfdceError> {
        let total = self.plane_len();
        let base = usize::from(plane) * total;
        let count = match run {
            Run::Fill(_, c) | Run::Literal(c) | Run::Pair(_, c) => c,
            Run::Columns { count, .. }
            | Run::Back { count, .. }
            | Run::Transform { count, .. }
            | Run::Masked { count, .. } => count,
        };
        let mut mask = match run {
            Run::Masked { mask, .. } => mask,
            _ => 0,
        };
        for k in 0..count {
            if p == total {
                break;
            }
            let i = self.index(p);
            let v = match run {
                Run::Fill(v, _) => v,
                Run::Literal(_) => self.r.byte()?,
                Run::Pair(pair, _) => pair[k % 2],
                Run::Columns { columns, .. } => {
                    if i % self.w < columns {
                        return Err(TfdceError::BadBackReference {
                            at,
                            distance: columns,
                        });
                    }
                    self.planes[base + i - columns]
                }
                Run::Back { distance, .. } => {
                    if distance == 0 || distance > p {
                        return Err(TfdceError::BadBackReference { at, distance });
                    }
                    self.planes[base + self.index(p - distance)]
                }
                Run::Transform { source, op, .. } => {
                    let v = self.planes[usize::from(source) * total + i];
                    match op {
                        0 => v,
                        1 => !v,
                        2 => v.rotate_right(1),
                        _ => v.rotate_left(1),
                    }
                }
                Run::Masked { source, rotate, .. } => {
                    let v = self.planes[usize::from(source) * total + i] & mask;
                    mask = mask.rotate_left(rotate);
                    v
                }
            };
            self.planes[base + i] = v;
            p += 1;
        }
        Ok(p)
    }
}

/// A small valid image for test fixtures: planes 0–2 filled with `fills`, plane 3 cleared.
#[cfg(test)]
pub(crate) fn fixture(width_bytes: u8, height: u16, fills: [u8; 3]) -> Vec<u8> {
    let mut out = vec![2, b'T', width_bytes];
    out.extend_from_slice(&height.to_le_bytes());
    // Methods 1, 1, 1, 0 for the planes in order 0, 1, 2, 3; no flags.
    out.extend_from_slice(&[0x11, 0x01, 0xE4, 0x00]);
    out.extend_from_slice(&fills);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header of a `w`-byte × `h`-row image; `order` lists plane numbers in processing order.
    fn header(w: u8, h: u16, methods: u16, order: [u8; 4], dict: &[u16]) -> Vec<u8> {
        let mut out = vec![2, b'T', w];
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&methods.to_le_bytes());
        out.push(
            order
                .iter()
                .enumerate()
                .fold(0, |acc, (i, &p)| acc | (p << (2 * i))),
        );
        if dict.is_empty() {
            out.push(0);
        } else {
            out.push(FLAG_DICTIONARY);
            out.push(dict.len() as u8);
            for w in dict {
                out.extend_from_slice(&w.to_le_bytes());
            }
        }
        out
    }

    /// Plane `p` of an image, row-major.
    fn plane(img: &Image, p: usize) -> &[u8] {
        let len = img.width / 8 * img.height;
        &img.planar[p * len..(p + 1) * len]
    }

    /// 2 byte columns × 3 rows: serpentine positions 0..6 are row-major indices
    /// 0, 2, 4, 5, 3, 1.
    const SERP: [usize; 6] = [0, 2, 4, 5, 3, 1];

    fn in_serpentine(values: [u8; 6]) -> Vec<u8> {
        let mut out = vec![0; 6];
        for (p, v) in values.into_iter().enumerate() {
            out[SERP[p]] = v;
        }
        out
    }

    /// Only plane 0 carries a stream; planes 1–2 are filled with 0, plane 3 cleared.
    fn plane0(stream: &[u8]) -> Result<Image, TfdceError> {
        let mut data = header(2, 3, 0x0113, [0, 1, 2, 3], &[]);
        data.extend_from_slice(stream);
        data.extend_from_slice(&[0, 0]); // fill bytes of planes 1 and 2
        decode(&data)
    }

    #[test]
    fn header_fields() {
        let mut data = header(8, 80, 0x0333, [2, 1, 0, 3], &[0x1234]);
        data.push(0xEE);
        let h = parse_header(&data).unwrap();
        assert_eq!(h.tag, b"T");
        assert_eq!((h.width_bytes, h.width(), h.height), (8, 64, 80));
        assert_eq!(h.plane_methods, 0x0333);
        assert_eq!(h.plane_order, [2, 1, 0, 3]);
        assert_eq!(data[7], 0xC6);
        assert_eq!(h.dictionary, vec![0x1234]);
        assert_eq!(h.palette, None);
        assert_eq!(h.data_start, data.len() - 1);

        // The palette block is skipped.
        let mut data = vec![1, 1, 1, 0, 0x01, 0x00, 0xE4, FLAG_PALETTE];
        data.extend_from_slice(&[9; 32]);
        let h = parse_header(&data).unwrap();
        assert_eq!(h.tag, b"");
        assert_eq!(h.palette, Some(vec![9; 32]));
        assert_eq!(h.data_start, data.len());
    }

    #[test]
    fn fill_literal_and_serpentine_order() {
        // 7n: 3 literals, then A1: fill 3 × 0x55.
        let img = plane0(&[0x72, 1, 2, 3, 0xA1, 0x55]).unwrap();
        assert_eq!((img.width, img.height, img.planar.len()), (16, 3, 24));
        assert_eq!(plane(&img, 0), in_serpentine([1, 2, 3, 0x55, 0x55, 0x55]));
        assert_eq!(plane(&img, 1), [0; 6]);
        assert_eq!(plane(&img, 3), [0; 6]);
    }

    #[test]
    fn runs_stop_at_the_end_of_the_plane() {
        // FF would fill 97 bytes; the plane holds 6.
        let img = plane0(&[0xFF, 0x0F]).unwrap();
        assert_eq!(plane(&img, 0), [0x0F; 6]);
        // A literal run cut short reads only the bytes it writes: 7F wants 16 bytes, reads 6.
        let img = plane0(&[0x7F, 1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(plane(&img, 0), in_serpentine([1, 2, 3, 4, 5, 6]));
    }

    #[test]
    fn pairs_and_nibble_pairs() {
        // 61: the pair (9, 8) twice; 80 3C: once the pair 0xCC, 0x33 (low nibble first).
        let img = plane0(&[0x61, 9, 8, 0x80, 0x3C]).unwrap();
        assert_eq!(plane(&img, 0), in_serpentine([9, 8, 9, 8, 0xCC, 0x33]));
    }

    #[test]
    fn back_references() {
        // 2n: 3 literals, then copy 3 from 2 back (overlapping): 1 2 3 2 3 2.
        let img = plane0(&[0x72, 1, 2, 3, 0x20, 2]).unwrap();
        assert_eq!(plane(&img, 0), in_serpentine([1, 2, 3, 2, 3, 2]));
        // 4n: 3 literals down column 0, then column 1 copies its left neighbour row by row.
        let img = plane0(&[0x72, 1, 2, 3, 0x40, 1]).unwrap();
        assert_eq!(plane(&img, 0), [1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn column_copy_follows_the_serpentine_walk() {
        // 3 columns × 2 rows. Column 0 = 1 2 (down); 40 03: 5 bytes from one column left:
        // column 1 walks up (row 1, row 0), column 2 walks down.
        let mut data = header(3, 2, 0x0113, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[0x71, 1, 2, 0x40, 3, 0, 0]);
        let img = decode(&data).unwrap();
        assert_eq!(plane(&img, 0), [1, 1, 1, 2, 2, 2]);
    }

    #[test]
    fn cross_plane_transforms() {
        // Plane 0 by stream; plane 1: 3n with n = 0b0001 (NOT of order position 0) for 6.
        let mut data = header(2, 3, 0x0333, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[0x75, 0x81, 0x01, 0x00, 0xFF, 0x80, 0x03]);
        data.extend_from_slice(&[0x31, 4]);
        // Plane 2: rotate right (n = 2) for 2, rotate left (n = 3) for 2, copy for 2.
        data.extend_from_slice(&[0x32, 0, 0x33, 0, 0x30, 0]);
        let img = decode(&data).unwrap();
        let p0 = in_serpentine([0x81, 0x01, 0x00, 0xFF, 0x80, 0x03]);
        assert_eq!(plane(&img, 0), p0);
        let p1: Vec<u8> = p0.iter().map(|v| !v).collect();
        assert_eq!(plane(&img, 1), p1);
        assert_eq!(
            plane(&img, 2),
            in_serpentine([0xC0, 0x80, 0x00, 0xFF, 0x80, 0x03])
        );
    }

    /// Plane 0 all 0xFF, plane 1 from `cmds`, plane 2 all 0, plane 3 cleared.
    fn masked(w: u8, h: u16, dict: &[u16], cmds: &[u8]) -> Vec<u8> {
        let mut data = header(w, h, 0x0131, [0, 1, 2, 3], dict);
        data.push(0xFF);
        data.extend_from_slice(cmds);
        data.push(0x00);
        plane(&decode(&data).unwrap(), 1).to_vec()
    }

    #[test]
    fn masked_copies_and_recent_masks() {
        // Recent masks start as 00 FF 55 AA.
        // 53: order position 0, recent mask 3 (0xAA) for 0 + 2 bytes; it swaps with mask 2:
        //     00 FF AA 55.
        // 52: recent mask 2 (0xAA again) for 2 bytes; swaps with mask 1: 00 AA FF 55.
        // 5C 0F: selector 3: order position 0, new mask 0x0F at 2 (0xFF moves to 3), 2 bytes.
        assert_eq!(
            masked(2, 3, &[], &[0x53, 0, 0x52, 0, 0x5C, 0x0F, 0]),
            in_serpentine([0xAA, 0xAA, 0xAA, 0xAA, 0x0F, 0x0F])
        );
    }

    #[test]
    fn recent_mask_order_is_tracked() {
        // 4 columns × 1 row, so each masked copy of 2 bytes fills two columns.
        // 53 (0xAA) → masks 00 FF AA 55; 53 again picks 0x55.
        assert_eq!(
            masked(4, 1, &[], &[0x53, 0, 0x53, 0]),
            [0xAA, 0xAA, 0x55, 0x55]
        );
        // 5C 0F puts 0x0F at position 2 (the old 0x55 moves to 3); 53 picks that 0x55.
        assert_eq!(
            masked(4, 1, &[], &[0x5C, 0x0F, 0, 0x53, 0]),
            [0x0F, 0x0F, 0x55, 0x55]
        );
        // 51 (0xFF) moves to the front (FF 00 55 AA); 52 then picks 0x55.
        assert_eq!(
            masked(4, 1, &[], &[0x51, 0, 0x52, 0]),
            [0xFF, 0xFF, 0x55, 0x55]
        );
    }

    #[test]
    fn dithered_masks_rotate() {
        let run = |cmds: &[u8]| masked(4, 1, &[], cmds);
        // 0x55 / 0xAA rotate by one bit per byte.
        assert_eq!(run(&[0x92, 2]), [0x55, 0xAA, 0x55, 0xAA]);
        // Other masks rotate by two: 0x11 → 0x44 → 0x11.
        assert_eq!(run(&[0x9C, 0x11, 2]), [0x11, 0x44, 0x11, 0x44]);
        // 5n never rotates.
        assert_eq!(run(&[0x5C, 0x11, 2]), [0x11; 4]);
    }

    #[test]
    fn dictionary_words() {
        // Words: fill 0xA2 with 0x07 (4 bytes); literal 0x70 with 0x09; a no-op 0x60;
        // column copy 0x40 with length 0 (2 bytes, 1 column left).
        let dict = [0x07A2, 0x0970, 0x3360, 0x0040];
        let run = |w: u8, h: u16, stream: &[u8]| {
            let mut data = header(w, h, 0x0113, [0, 1, 2, 3], &dict);
            data.extend_from_slice(stream);
            data.extend_from_slice(&[0, 0]);
            decode(&data)
        };
        let img = run(2, 3, &[0x00, 0x02, 0x01, 0x01]).unwrap();
        assert_eq!(plane(&img, 0), in_serpentine([7, 7, 7, 7, 9, 9]));
        let img = run(3, 1, &[0x01, 0x03]).unwrap();
        assert_eq!(plane(&img, 0), [9, 9, 9]);
        assert_eq!(
            run(2, 3, &[0x04]).unwrap_err(),
            TfdceError::DictionaryIndex {
                at: 18,
                index: 4,
                len: 4
            }
        );
    }

    #[test]
    fn dictionary_masked_and_linear_words() {
        // 4 columns × 1 row. A 9x word is dithered too; its argument is the count.
        let dict = [0x0292, 0x0120];
        assert_eq!(masked(4, 1, &dict, &[0x00]), [0x55, 0xAA, 0x55, 0xAA]);
        // 70 5: one literal; word 1 is a linear copy of 3 bytes from 1 back.
        let mut data = header(4, 1, 0x0113, [0, 1, 2, 3], &dict);
        data.extend_from_slice(&[0x70, 5, 0x01, 0, 0]);
        let img = decode(&data).unwrap();
        assert_eq!(plane(&img, 0), [5, 5, 5, 5]);
    }

    #[test]
    fn plane_methods() {
        // Order 3, 2, 1, 0: plane 3 filled 0x0F, plane 2 copy of order position 0 (plane 3),
        // plane 1 method 0 is an error, so use a stream; plane 0 fill.
        let mut data = header(1, 2, 0x1321, [3, 2, 1, 0], &[]);
        data.extend_from_slice(&[0x0F, 0x00, 0x71, 1, 2, 0xAA]);
        let img = decode(&data).unwrap();
        assert_eq!(plane(&img, 3), [0x0F, 0x0F]);
        assert_eq!(plane(&img, 2), [0x0F, 0x0F]);
        assert_eq!(plane(&img, 1), [1, 2]);
        assert_eq!(plane(&img, 0), [0xAA, 0xAA]);

        // Method 0 on plane 3 clears it; on another plane it leaves it undefined.
        let mut data = header(1, 1, 0x0111, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[1, 2, 3]);
        assert_eq!(plane(&decode(&data).unwrap(), 3), [0]);
        let mut data = header(1, 1, 0x1011, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            decode(&data).unwrap_err(),
            TfdceError::BadPlaneMethod {
                plane: 2,
                method: 0
            }
        );
        let mut data = header(1, 1, 0x0511, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[1, 2]);
        assert_eq!(
            decode(&data).unwrap_err(),
            TfdceError::BadPlaneMethod {
                plane: 2,
                method: 5
            }
        );
        // A plane missing from the order is never written.
        let mut data = header(1, 1, 0x0111, [0, 1, 1, 3], &[]);
        data.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            decode(&data).unwrap_err(),
            TfdceError::PlaneLeftUndefined { plane: 2 }
        );
    }

    #[test]
    fn stream_errors() {
        let data = header(2, 3, 0x0113, [0, 1, 2, 3], &[]);
        let at = data.len();
        // The literal run swallows the fill bytes meant for planes 1 and 2, then runs out.
        assert_eq!(
            plane0(&[0x72, 1]).unwrap_err(),
            TfdceError::Truncated { at: at + 4 }
        );
        // Distance 0 and distances before the start.
        assert_eq!(
            plane0(&[0x70, 1, 0x20, 0]).unwrap_err(),
            TfdceError::BadBackReference {
                at: at + 2,
                distance: 0
            }
        );
        assert_eq!(
            plane0(&[0x70, 1, 0x20, 2]).unwrap_err(),
            TfdceError::BadBackReference {
                at: at + 2,
                distance: 2
            }
        );
        // Column copy left of the image.
        assert_eq!(
            plane0(&[0x40, 0]).unwrap_err(),
            TfdceError::BadBackReference { at, distance: 1 }
        );
        // Plane 0 cannot read plane 1, which is decoded later, nor itself.
        assert_eq!(
            plane0(&[0x34, 4]).unwrap_err(),
            TfdceError::UndefinedSource { at, plane: 1 }
        );
        assert_eq!(
            plane0(&[0x30, 4]).unwrap_err(),
            TfdceError::UndefinedSource { at, plane: 0 }
        );
        assert_eq!(
            plane0(&[0xA4, 1, 9]).unwrap_err(),
            TfdceError::TrailingBytes {
                consumed: at + 4,
                len: at + 5
            }
        );
        assert_eq!(
            decode(&[2, b'T', 0, 1, 0]).unwrap_err(),
            TfdceError::BadGeometry {
                width_bytes: 0,
                height: 1
            }
        );
        assert_eq!(
            decode(&[2, b'T', 1]).unwrap_err(),
            TfdceError::Truncated { at: 3 }
        );
    }

    #[test]
    fn output_reads_as_planar() {
        // 1 byte × 1 row: planes 0..3 = 0x80, 0x80, 0x00, cleared → pixel 0 has index 3.
        let mut data = header(1, 1, 0x0311, [0, 1, 2, 3], &[]);
        data.extend_from_slice(&[0x80, 0x80, 0x70, 0x00]);
        let img = decode(&data).unwrap();
        let indexed = crate::planar::decode(&img.planar, img.width, img.height).unwrap();
        assert_eq!(indexed.pixels, [3, 0, 0, 0, 0, 0, 0, 0]);
    }
}
