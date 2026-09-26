//! The 16-colour palettes of the DOS/V builds, which live inside `MAIN.EXE`.
//!
//! The notes document a bank of **9 slots × 48 bytes** (16 colours × 3 bytes, slot 5 identical
//! to slot 8) followed by the bytes `80 40 20 10`. Each colour is stored as `[B][R][G]` with 4
//! bits used per channel; the engine expands a channel to the VGA DAC's 6 bits as
//! `(n << 2) | (n >> 2)`. The bank is located by that signature, never by a hard-coded offset
//! (the Korean build has it at file offset `0x38DF0`; other builds differ).
//!
//! Which slot belongs to which screen is not documented; extraction reports the slot it used.

use crate::image::Palette16;
use std::fmt;

/// Number of palette slots in the bank.
pub const SLOTS: usize = 9;
/// Bytes per slot (16 colours × 3 channels).
pub const SLOT_BYTES: usize = 48;
/// Bytes of the whole bank.
pub const BANK_BYTES: usize = SLOTS * SLOT_BYTES;
/// The bytes that follow the bank.
pub const TERMINATOR: [u8; 4] = [0x80, 0x40, 0x20, 0x10];

/// The palette bank could not be located unambiguously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteError {
    /// No block matches the signature.
    NotFound,
    /// More than one block matches; offsets of every candidate.
    Ambiguous(Vec<usize>),
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PaletteError::NotFound => write!(
                f,
                "no palette bank (9 × 48 bytes of 4-bit values followed by 80 40 20 10) found"
            ),
            PaletteError::Ambiguous(offsets) => {
                let list: Vec<String> = offsets.iter().map(|o| format!("{o:#x}")).collect();
                write!(f, "several palette bank candidates at {}", list.join(", "))
            }
        }
    }
}

impl std::error::Error for PaletteError {}

/// A located palette bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteBank {
    /// File offset of slot 0.
    pub offset: usize,
    /// Every slot expanded to 8-bit RGB.
    pub slots: [Palette16; SLOTS],
}

/// Expand a 4-bit channel to the 6-bit VGA value the engine programs, then to 8 bits.
pub fn expand_channel(n: u8) -> u8 {
    let n = n & 0x0f;
    let vga6 = (n << 2) | (n >> 2);
    (vga6 << 2) | (vga6 >> 4)
}

fn is_bank(block: &[u8]) -> bool {
    block.len() == BANK_BYTES
        && block.iter().all(|&b| b <= 0x0f)
        && block[5 * SLOT_BYTES..6 * SLOT_BYTES] == block[8 * SLOT_BYTES..9 * SLOT_BYTES]
}

fn decode_slot(raw: &[u8]) -> Palette16 {
    std::array::from_fn(|c| {
        let (b, r, g) = (raw[c * 3], raw[c * 3 + 1], raw[c * 3 + 2]);
        [expand_channel(r), expand_channel(g), expand_channel(b)]
    })
}

/// Find the palette bank in an executable image by its signature.
pub fn find_bank(exe: &[u8]) -> Result<PaletteBank, PaletteError> {
    let mut found = Vec::new();
    if exe.len() >= BANK_BYTES + TERMINATOR.len() {
        for end in BANK_BYTES..=exe.len() - TERMINATOR.len() {
            if exe[end..end + TERMINATOR.len()] == TERMINATOR
                && is_bank(&exe[end - BANK_BYTES..end])
            {
                found.push(end - BANK_BYTES);
            }
        }
    }
    match found.as_slice() {
        [] => Err(PaletteError::NotFound),
        [offset] => {
            let raw = &exe[*offset..*offset + BANK_BYTES];
            Ok(PaletteBank {
                offset: *offset,
                slots: std::array::from_fn(|s| {
                    decode_slot(&raw[s * SLOT_BYTES..(s + 1) * SLOT_BYTES])
                }),
            })
        }
        _ => Err(PaletteError::Ambiguous(found)),
    }
}

/// Write a synthetic bank (9 slots of `[B][R][G]` nibbles + terminator) for fixtures. Slot 8 is
/// forced equal to slot 5, as in the known bank.
pub fn build_bank(slots: &[[[u8; 3]; 16]; SLOTS]) -> Vec<u8> {
    let mut out = Vec::with_capacity(BANK_BYTES + TERMINATOR.len());
    for s in 0..SLOTS {
        let slot = if s == 8 { &slots[5] } else { &slots[s] };
        for &[r, g, b] in slot {
            out.extend_from_slice(&[b & 0x0f, r & 0x0f, g & 0x0f]);
        }
    }
    out.extend_from_slice(&TERMINATOR);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_slots() -> [[[u8; 3]; 16]; SLOTS] {
        std::array::from_fn(|s| std::array::from_fn(|c| [c as u8, (15 - c) as u8, s as u8]))
    }

    #[test]
    fn channel_expansion() {
        assert_eq!(expand_channel(0), 0);
        assert_eq!(expand_channel(15), 255);
        // 8 -> VGA 34 -> 8-bit 138
        assert_eq!(expand_channel(8), 138);
        let all: Vec<u8> = (0..16).map(expand_channel).collect();
        assert!(all.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn finds_the_bank_by_signature() {
        let mut exe = vec![0x90u8; 1000];
        exe.extend(build_bank(&sample_slots()));
        exe.extend(vec![0xccu8; 500]);
        let bank = find_bank(&exe).unwrap();
        assert_eq!(bank.offset, 1000);
        // Slot 2, colour 3: R = 3, G = 12, B = 2 -> stored as [B][R][G].
        assert_eq!(
            bank.slots[2][3],
            [expand_channel(3), expand_channel(12), expand_channel(2)]
        );
        assert_eq!(bank.slots[8], bank.slots[5]);
    }

    #[test]
    fn missing_and_ambiguous_banks() {
        assert_eq!(find_bank(&[0; 10]), Err(PaletteError::NotFound));
        // The terminator alone after bytes that are not all 4-bit values.
        let mut exe = vec![0xffu8; 600];
        exe.extend_from_slice(&TERMINATOR);
        assert_eq!(find_bank(&exe), Err(PaletteError::NotFound));
        // Slot 5 != slot 8 does not match.
        let mut bank = build_bank(&sample_slots());
        bank[8 * SLOT_BYTES] ^= 1;
        assert_eq!(find_bank(&bank), Err(PaletteError::NotFound));
        // Two banks: ambiguous, both offsets reported.
        let mut exe = build_bank(&sample_slots());
        exe.extend(vec![0xeeu8; 16]);
        exe.extend(build_bank(&sample_slots()));
        assert_eq!(
            find_bank(&exe),
            Err(PaletteError::Ambiguous(vec![0, BANK_BYTES + 4 + 16]))
        );
    }
}
