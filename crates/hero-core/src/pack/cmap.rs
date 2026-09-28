//! The characters a TrueType/OpenType font maps (its `cmap` table), for the Hanja coverage check
//! of [`super::Pack::missing_media`]: the base pack's fonts hold Galmuri's own Hanja plus the ones
//! the base pack's text needs, not every Hanja, and a character a font lacks is drawn as a blank.
//!
//! Only what the check needs is read: the table directory, the `cmap` subtables for Unicode
//! (platform 0, or platform 3 encodings 1 and 10) in formats 4 (BMP ranges) and 12 (full ranges).

/// Inclusive code point ranges a font maps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    ranges: Vec<(u32, u32)>,
}

impl Coverage {
    pub fn contains(&self, c: char) -> bool {
        let c = c as u32;
        // Ranges are sorted and merged: the last one starting at or before `c`.
        let i = self.ranges.partition_point(|&(a, _)| a <= c);
        i > 0 && self.ranges[i - 1].1 >= c
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The code points `font` (a whole TTF/OTF file) maps to a glyph other than `.notdef`.
pub fn coverage(font: &[u8]) -> Result<Coverage, String> {
    let bad = || "not a TrueType/OpenType font with a Unicode cmap".to_string();
    let tables = usize::from(u16_at(font, 4).ok_or_else(bad)?);
    let cmap = (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&rec| font.get(rec..rec + 4) == Some(b"cmap"))
        .and_then(|rec| u32_at(font, rec + 8))
        .ok_or_else(bad)? as usize;
    let subtables = usize::from(u16_at(font, cmap + 2).ok_or_else(bad)?);
    let mut ranges = Vec::new();
    for i in 0..subtables {
        let rec = cmap + 4 + 8 * i;
        let (Some(platform), Some(encoding), Some(offset)) = (
            u16_at(font, rec),
            u16_at(font, rec + 2),
            u32_at(font, rec + 4),
        ) else {
            return Err(bad());
        };
        let unicode = platform == 0 || (platform == 3 && matches!(encoding, 1 | 10));
        if !unicode {
            continue;
        }
        let at = cmap + offset as usize;
        match u16_at(font, at) {
            Some(4) => format4(font, at, &mut ranges).ok_or_else(bad)?,
            Some(12) => format12(font, at, &mut ranges).ok_or_else(bad)?,
            _ => {}
        }
    }
    if ranges.is_empty() {
        return Err(bad());
    }
    ranges.sort_unstable();
    // Merge overlapping and touching ranges (a font maps the same code points in several
    // subtables).
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (a, b) in ranges {
        match merged.last_mut() {
            Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    Ok(Coverage { ranges: merged })
}

/// Format 4: segments of BMP code points; a segment maps a code point unless its glyph works out
/// to 0 (`.notdef`).
fn format4(b: &[u8], at: usize, out: &mut Vec<(u32, u32)>) -> Option<()> {
    let segs = usize::from(u16_at(b, at + 6)? / 2);
    let ends = at + 14;
    let starts = ends + 2 * segs + 2;
    let deltas = starts + 2 * segs;
    let offsets = deltas + 2 * segs;
    // Segments are sorted and do not overlap (the spec requires it); a font that breaks this is
    // refused, which also bounds the work below by the 65,536 BMP code points.
    let mut next_free = 0u32;
    for s in 0..segs {
        let end = u32::from(u16_at(b, ends + 2 * s)?);
        let start = u32::from(u16_at(b, starts + 2 * s)?);
        let delta = u16_at(b, deltas + 2 * s)?;
        let range_offset = usize::from(u16_at(b, offsets + 2 * s)?);
        if start == 0xFFFF {
            continue;
        }
        if start > end || start < next_free {
            return None;
        }
        next_free = end + 1;
        if range_offset == 0 {
            // Glyph = code + delta (mod 65536); only one code point can land on `.notdef`.
            let hole = u32::from(0u16.wrapping_sub(delta));
            push_without(out, start, end, hole);
            continue;
        }
        // Glyph ids read from the glyph array, relative to this segment's idRangeOffset word.
        let mut run: Option<u32> = None;
        for c in start..=end {
            let at_glyph = offsets + 2 * s + range_offset + 2 * (c - start) as usize;
            let glyph = u16_at(b, at_glyph)?;
            let mapped = glyph != 0 && glyph.wrapping_add(delta) != 0;
            match (mapped, run) {
                (true, None) => run = Some(c),
                (false, Some(a)) => {
                    out.push((a, c - 1));
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(a) = run {
            out.push((a, end));
        }
    }
    Some(())
}

/// `start..=end` without `hole`.
fn push_without(out: &mut Vec<(u32, u32)>, start: u32, end: u32, hole: u32) {
    if hole < start || hole > end {
        out.push((start, end));
        return;
    }
    if hole > start {
        out.push((start, hole - 1));
    }
    if hole < end {
        out.push((hole + 1, end));
    }
}

/// Format 12: groups of consecutive code points mapped to consecutive glyphs.
fn format12(b: &[u8], at: usize, out: &mut Vec<(u32, u32)>) -> Option<()> {
    let groups = u32_at(b, at + 12)? as usize;
    for g in 0..groups {
        let rec = at + 16 + 12 * g;
        let (start, end, glyph) = (u32_at(b, rec)?, u32_at(b, rec + 4)?, u32_at(b, rec + 8)?);
        if start > end {
            continue;
        }
        if glyph == 0 {
            // The first code point is `.notdef`, the rest map.
            if start < end {
                out.push((start + 1, end));
            }
        } else {
            out.push((start, end));
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font file with just a table directory and a cmap holding `subtable` (platform 3,
    /// encoding `encoding`).
    fn font(encoding: u16, subtable: &[u8]) -> Vec<u8> {
        let mut f = vec![0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        f.extend_from_slice(b"cmap");
        f.extend_from_slice(&[0; 4]); // checksum
        f.extend_from_slice(&28u32.to_be_bytes()); // offset of the cmap
        f.extend_from_slice(&0u32.to_be_bytes()); // length (unused)
                                                  // cmap header: version 0, one subtable at offset 12.
        f.extend_from_slice(&[0, 0, 0, 1, 0, 3]);
        f.extend_from_slice(&encoding.to_be_bytes());
        f.extend_from_slice(&12u32.to_be_bytes());
        f.extend_from_slice(subtable);
        f
    }

    fn be16(v: &[u16]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_be_bytes()).collect()
    }

    #[test]
    fn format_4_segments() {
        // Segments: 'A'..='C' by delta, 劉 (U+5289) by the glyph array, and the 0xFFFF end.
        let segs = 3u16;
        let mut t = be16(&[4, 0, 0, segs * 2, 0, 0, 0]);
        t.extend(be16(&[0x43, 0x5289, 0xFFFF])); // ends
        t.extend(be16(&[0])); // reserved pad
        t.extend(be16(&[0x41, 0x5289, 0xFFFF])); // starts
        t.extend(be16(&[0u16.wrapping_sub(0x40), 0, 1])); // deltas
        t.extend(be16(&[0, 4, 0])); // range offsets: the second points at the glyph array
        t.extend(be16(&[7])); // glyph array: 劉 → glyph 7
        let c = coverage(&font(1, &t)).unwrap();
        assert!(c.contains('A') && c.contains('C') && c.contains('劉'));
        assert!(!c.contains('D') && !c.contains('備'));
    }

    #[test]
    fn format_12_groups() {
        let mut t = be16(&[12, 0]);
        t.extend(0u32.to_be_bytes()); // length
        t.extend(0u32.to_be_bytes()); // language
        t.extend(2u32.to_be_bytes()); // groups
        for (a, b, g) in [(0x4E00u32, 0x4E05u32, 10u32), (0x20000, 0x20001, 0)] {
            t.extend(a.to_be_bytes());
            t.extend(b.to_be_bytes());
            t.extend(g.to_be_bytes());
        }
        let c = coverage(&font(10, &t)).unwrap();
        assert!(c.contains('\u{4E00}') && c.contains('\u{4E05}'));
        assert!(!c.contains('\u{4E06}'));
        // A group starting at glyph 0: its first code point is `.notdef`.
        assert!(!c.contains('\u{20000}') && c.contains('\u{20001}'));
    }

    #[test]
    fn format_4_holes() {
        // 'A'..='C' where the delta sends 'B' to glyph 0, and '1'..='3' through the glyph array
        // with '2' at glyph 0: neither 'B' nor '2' is mapped.
        let segs = 3u16;
        let mut t = be16(&[4, 0, 0, segs * 2, 0, 0, 0]);
        t.extend(be16(&[0x33, 0x43, 0xFFFF])); // ends
        t.extend(be16(&[0])); // reserved pad
        t.extend(be16(&[0x31, 0x41, 0xFFFF])); // starts
        t.extend(be16(&[0, 0u16.wrapping_sub(0x42), 1])); // deltas
        t.extend(be16(&[6, 0, 0])); // range offsets: the first points at the glyph array
        t.extend(be16(&[5, 0, 6])); // glyph array for '1', '2', '3'
        let c = coverage(&font(1, &t)).unwrap();
        assert!(c.contains('1') && !c.contains('2') && c.contains('3'));
        assert!(c.contains('A') && !c.contains('B') && c.contains('C'));
        // Overlapping segments are refused.
        let mut bad = be16(&[4, 0, 0, 6, 0, 0, 0]);
        bad.extend(be16(&[0x50, 0x30, 0xFFFF])); // ends
        bad.extend(be16(&[0])); // reserved pad
        bad.extend(be16(&[0x10, 0x20, 0xFFFF])); // starts: 0x20 lies inside 0x10..=0x50
        bad.extend(be16(&[1, 1, 1])); // deltas
        bad.extend(be16(&[0, 0, 0])); // range offsets
        assert!(coverage(&font(1, &bad)).is_err());
    }

    #[test]
    fn not_a_font() {
        assert!(coverage(b"").is_err());
        assert!(coverage(b"GIF89a").is_err());
    }
}
