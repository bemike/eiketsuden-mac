//! A YM3812 (OPL2) FM synthesis emulator, enough to render the original's music
//! (docs/reverse-engineering/FORMATS.md §16): nine two-operator channels, the four OPL2
//! waveforms, ADSR envelopes with key scaling, tremolo and vibrato. Rhythm mode and the timers
//! are not emulated (the music does not use them).
//!
//! The chip runs at its own rate ([`RATE`], 3.579545 MHz / 72). The operator output follows the
//! chip's logarithmic arithmetic: a quarter-wave log-sine table, the attenuation added in
//! 1/256 steps of a doubling, and an exponent table back to a linear 13-bit value. The envelope
//! steps follow the chip's rate tables (as documented by the MAME and Nuked OPL projects).

/// Samples per second of the chip.
pub const RATE: f64 = 3_579_545.0 / 72.0;

/// Frequency multipliers ×2 (0.5, 1, 2 … 15).
const MULT2: [u32; 16] = [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// Key scale levels at block 7 in 1/8 dB, by the top four bits of the F-number (6 dB/octave).
const KSL_DB8: [u32; 16] = [
    0, 72, 96, 111, 120, 129, 135, 141, 144, 150, 153, 156, 159, 162, 165, 168,
];

/// Envelope increments: eight steps per pattern (see [`eg_step`]).
const EG_INC: [[u16; 8]; 15] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
    [2, 2, 2, 2, 2, 2, 2, 2],
    [2, 2, 2, 4, 2, 2, 2, 4],
    [2, 4, 2, 4, 2, 4, 2, 4],
    [2, 4, 4, 4, 2, 4, 4, 4],
    [4, 4, 4, 4, 4, 4, 4, 4],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// Largest attenuation (silence) of the 9-bit envelope, in 3/16 dB steps.
const MAX_ATT: u16 = 511;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Eg {
    Attack,
    Decay,
    Sustain,
    Release,
    #[default]
    Off,
}

#[derive(Debug, Clone, Default)]
struct Operator {
    am: bool,
    vib: bool,
    sustain: bool,
    ksr: bool,
    mult: u8,
    ksl: u8,
    tl: u8,
    ar: u8,
    dr: u8,
    sl: u8,
    rr: u8,
    wave: u8,
    phase: u32,
    env: u16,
    eg: Eg,
    /// This and the previous output (the modulator's feedback uses both).
    out: [i32; 2],
}

#[derive(Debug, Clone, Default)]
struct Channel {
    fnum: u16,
    block: u8,
    key: bool,
    feedback: u8,
    additive: bool,
}

/// The chip.
#[derive(Debug, Clone)]
pub struct Opl2 {
    ops: [Operator; 18],
    channels: [Channel; 9],
    logsin: [u16; 256],
    exp: [u16; 256],
    counter: u32,
    tremolo_pos: u32,
    vibrato_pos: u32,
    deep_tremolo: bool,
    deep_vibrato: bool,
    note_select: bool,
    waveforms: bool,
}

impl Default for Opl2 {
    fn default() -> Self {
        Opl2::new()
    }
}

/// Operator slot of register offset `off` (0x00–0x15, with gaps), if any.
fn slot(off: u8) -> Option<usize> {
    let group = usize::from(off / 8);
    let within = usize::from(off % 8);
    (group < 3 && within < 6).then_some(group * 6 + within)
}

/// Modulator and carrier slots of channel `ch`.
fn channel_slots(ch: usize) -> (usize, usize) {
    let m = (ch / 3) * 6 + ch % 3;
    (m, m + 3)
}

impl Opl2 {
    pub fn new() -> Opl2 {
        let mut logsin = [0u16; 256];
        let mut exp = [0u16; 256];
        for (i, (l, e)) in logsin.iter_mut().zip(exp.iter_mut()).enumerate() {
            let s = ((i as f64 + 0.5) * std::f64::consts::PI / 512.0).sin();
            *l = (-s.log2() * 256.0).round() as u16;
            *e = (2f64.powf(1.0 - i as f64 / 256.0) * 1024.0).round() as u16;
        }
        let mut ops: [Operator; 18] = Default::default();
        for op in &mut ops {
            op.env = MAX_ATT;
        }
        Opl2 {
            ops,
            channels: Default::default(),
            logsin,
            exp,
            counter: 0,
            tremolo_pos: 0,
            vibrato_pos: 0,
            deep_tremolo: false,
            deep_vibrato: false,
            note_select: false,
            waveforms: false,
        }
    }

    /// Write `value` to register `reg`.
    pub fn write(&mut self, reg: u8, value: u8) {
        let low = reg & 0x1f;
        match reg & 0xe0 {
            0x20 | 0x40 | 0x60 | 0x80 | 0xe0 => {
                let Some(s) = slot(low) else {
                    return;
                };
                let op = &mut self.ops[s];
                match reg & 0xe0 {
                    0x20 => {
                        op.am = value & 0x80 != 0;
                        op.vib = value & 0x40 != 0;
                        op.sustain = value & 0x20 != 0;
                        op.ksr = value & 0x10 != 0;
                        op.mult = value & 0x0f;
                    }
                    0x40 => {
                        op.ksl = value >> 6;
                        op.tl = value & 0x3f;
                    }
                    0x60 => {
                        op.ar = value >> 4;
                        op.dr = value & 0x0f;
                    }
                    0x80 => {
                        op.sl = value >> 4;
                        op.rr = value & 0x0f;
                    }
                    _ => op.wave = value & 0x03,
                }
            }
            0x00 => match reg {
                0x01 => self.waveforms = value & 0x20 != 0,
                0x08 => self.note_select = value & 0x40 != 0,
                _ => {}
            },
            _ => {
                let ch = usize::from(reg & 0x0f);
                match reg & 0xf0 {
                    0xa0 if ch < 9 => {
                        let c = &mut self.channels[ch];
                        c.fnum = (c.fnum & 0x300) | u16::from(value);
                    }
                    0xb0 if ch < 9 => {
                        let c = &mut self.channels[ch];
                        c.fnum = (c.fnum & 0xff) | (u16::from(value & 0x03) << 8);
                        c.block = (value >> 2) & 0x07;
                        let key = value & 0x20 != 0;
                        if key != c.key {
                            c.key = key;
                            let (m, k) = channel_slots(ch);
                            for s in [m, k] {
                                let op = &mut self.ops[s];
                                if key {
                                    op.phase = 0;
                                    op.eg = Eg::Attack;
                                } else if op.eg != Eg::Off {
                                    op.eg = Eg::Release;
                                }
                            }
                        }
                    }
                    0xb0 if reg == 0xbd => {
                        self.deep_tremolo = value & 0x80 != 0;
                        self.deep_vibrato = value & 0x40 != 0;
                    }
                    0xc0 if ch < 9 => {
                        let c = &mut self.channels[ch];
                        c.feedback = (value >> 1) & 0x07;
                        c.additive = value & 0x01 != 0;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Key scale rate code of channel `ch`.
    fn key_code(&self, ch: usize) -> u32 {
        let c = &self.channels[ch];
        let bit = if self.note_select {
            (c.fnum >> 8) & 1
        } else {
            (c.fnum >> 9) & 1
        };
        u32::from(c.block) * 2 + u32::from(bit)
    }

    /// Advance the chip by one sample and return its output (the sum of the channels).
    pub fn sample(&mut self) -> i32 {
        self.counter = self.counter.wrapping_add(1);
        if self.counter & 0x3f == 0 {
            self.tremolo_pos = (self.tremolo_pos + 1) % 210;
        }
        if self.counter & 0x3ff == 0 {
            self.vibrato_pos = (self.vibrato_pos + 1) & 7;
        }
        let tremolo_level = if self.tremolo_pos < 105 {
            self.tremolo_pos
        } else {
            210 - self.tremolo_pos
        };
        let tremolo = tremolo_level >> if self.deep_tremolo { 2 } else { 4 };
        let mut mix = 0i32;
        for ch in 0..9 {
            let (m, k) = channel_slots(ch);
            let kcode = self.key_code(ch);
            let (fnum, block, feedback, additive) = {
                let c = &self.channels[ch];
                (c.fnum, c.block, c.feedback, c.additive)
            };
            let fb_mod = if feedback == 0 {
                0
            } else {
                (self.ops[m].out[0] + self.ops[m].out[1]) >> (9 - feedback)
            };
            let mod_out = self.operator(m, fnum, block, kcode, tremolo, fb_mod);
            let car_out = self.operator(
                k,
                fnum,
                block,
                kcode,
                tremolo,
                if additive { 0 } else { mod_out },
            );
            mix += if additive { mod_out + car_out } else { car_out };
        }
        mix
    }

    /// Advance operator `s` of a channel by one sample and return its output. `modulation` is
    /// added to its phase (the modulator's output or feedback).
    fn operator(
        &mut self,
        s: usize,
        fnum: u16,
        block: u8,
        kcode: u32,
        tremolo: u32,
        modulation: i32,
    ) -> i32 {
        let counter = self.counter;
        let vib_pos = self.vibrato_pos;
        let deep_vib = self.deep_vibrato;
        let waveforms = self.waveforms;
        let op = &mut self.ops[s];
        // Phase.
        let mut f = i32::from(fnum);
        if op.vib {
            let mut range = (f >> 7) & 7;
            if vib_pos & 3 == 0 {
                range = 0;
            } else if vib_pos & 1 == 1 {
                range >>= 1;
            }
            if !deep_vib {
                range >>= 1;
            }
            if vib_pos & 4 != 0 {
                range = -range;
            }
            f += range;
        }
        let inc = ((f.max(0) as u32) << block) >> 1;
        op.phase = (op.phase + ((inc * MULT2[usize::from(op.mult)]) >> 1)) & 0x7ffff;
        // Envelope.
        let ksr = if op.ksr { kcode } else { kcode >> 2 };
        eg_step(op, ksr, counter);
        // Level: envelope, total level, key scaling, tremolo.
        let ksl = if op.ksl == 0 {
            0
        } else {
            let db8 = i32::try_from(KSL_DB8[usize::from(fnum >> 6)]).unwrap_or(0)
                - 48 * (7 - i32::from(block));
            let db8 = db8.max(0) as u32;
            // 1/8 dB → 3/16 dB steps, at 3, 1.5 or 6 dB/octave.
            let steps = db8 * 2 / 3;
            match op.ksl {
                1 => steps >> 1,
                2 => steps >> 2,
                _ => steps,
            }
        };
        let att = u32::from(op.env) + u32::from(op.tl) * 4 + ksl + if op.am { tremolo } else { 0 };
        let att = att.min(u32::from(MAX_ATT));
        if op.eg == Eg::Off {
            op.out = [0, op.out[0]];
            return 0;
        }
        let p = ((op.phase >> 9) as i32 + modulation) as u32 & 0x3ff;
        let wave = if waveforms { op.wave } else { 0 };
        let (quarter, negative, silent) = match wave {
            1 => (p, false, p & 0x200 != 0),
            2 => (p, false, false),
            3 => (p, false, p & 0x100 != 0),
            _ => (p, p & 0x200 != 0, false),
        };
        let out = if silent {
            0
        } else {
            let q = quarter & 0x1ff;
            let index = if q & 0x100 != 0 {
                255 - (q & 0xff)
            } else {
                q & 0xff
            };
            let level = u32::from(self.logsin[index as usize]) + (att << 3);
            let linear = if level >= 0x1fff {
                0
            } else {
                (i32::from(self.exp[(level & 0xff) as usize]) << 1) >> (level >> 8)
            };
            if negative {
                -linear
            } else {
                linear
            }
        };
        let op = &mut self.ops[s];
        op.out = [out, op.out[0]];
        out
    }
}

/// Index into the rate tables for rate register `r` and key scale `ksr`.
fn rate_index(r: u8, ksr: u32) -> usize {
    if r == 0 {
        0
    } else {
        16 + usize::from(r) * 4 + ksr as usize
    }
}

/// Shift and increment pattern of rate table index `i` (16 "infinite" rates first).
fn rate(i: usize) -> (u32, usize) {
    if i < 16 {
        return (0, 14);
    }
    let r = i - 16;
    let (group, low) = (r / 4, r % 4);
    match group {
        0..=12 => (12 - group as u32, low),
        13 => (0, 4 + low),
        14 => (0, 8 + low),
        _ => (0, 12),
    }
}

/// One sample of an operator's envelope.
fn eg_step(op: &mut Operator, ksr: u32, counter: u32) {
    let step = |r: u8| -> Option<u16> {
        let (shift, pattern) = rate(rate_index(r, ksr));
        if counter & ((1 << shift) - 1) != 0 {
            return None;
        }
        Some(EG_INC[pattern][((counter >> shift) & 7) as usize])
    };
    let sl = if op.sl == 15 {
        31 * 16
    } else {
        u16::from(op.sl) * 16
    };
    match op.eg {
        Eg::Attack => {
            let idx = rate_index(op.ar, ksr);
            if idx >= 16 + 62 {
                op.env = 0;
            } else if let Some(inc) = step(op.ar) {
                let env = i32::from(op.env);
                let next = env + (((!env) * i32::from(inc)) >> 3);
                op.env = next.max(0) as u16;
            }
            if op.env == 0 {
                op.eg = Eg::Decay;
            }
        }
        Eg::Decay => {
            if let Some(inc) = step(op.dr) {
                op.env = (op.env + inc).min(MAX_ATT);
            }
            if op.env >= sl {
                op.eg = Eg::Sustain;
            }
        }
        Eg::Sustain => {
            if !op.sustain {
                if let Some(inc) = step(op.rr) {
                    op.env = (op.env + inc).min(MAX_ATT);
                }
            }
        }
        Eg::Release => {
            if let Some(inc) = step(op.rr) {
                op.env = (op.env + inc).min(MAX_ATT);
            }
            if op.env >= MAX_ATT {
                op.eg = Eg::Off;
            }
        }
        Eg::Off => op.env = MAX_ATT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-operator sine: channel 0 additive, modulator silent, carrier at full level.
    fn sine(chip: &mut Opl2, fnum: u16, block: u8) {
        chip.write(0x01, 0x20);
        // Sustained (EG type), multiplier 1.
        chip.write(0x20, 0x21);
        chip.write(0x23, 0x21);
        chip.write(0x40, 0x3f);
        chip.write(0x43, 0x00);
        chip.write(0x60, 0xf0);
        chip.write(0x63, 0xf0);
        chip.write(0x80, 0x0f);
        chip.write(0x83, 0x0f);
        chip.write(0xc0, 0x01);
        chip.write(0xa0, (fnum & 0xff) as u8);
        chip.write(0xb0, 0x20 | (block << 2) | (fnum >> 8) as u8);
    }

    #[test]
    fn a_note_sounds_at_its_frequency() {
        let mut chip = Opl2::new();
        // 440 Hz: fnum = 440 × 2^(20 − block) / RATE with block 4.
        let fnum = (440.0 * 2f64.powi(16) / RATE).round() as u16;
        sine(&mut chip, fnum, 4);
        let samples: Vec<i32> = (0..RATE as usize).map(|_| chip.sample()).collect();
        let crossings = samples.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        assert!((438..=442).contains(&crossings), "{crossings}");
        // Full level is close to the chip's 13-bit maximum (plus the faint modulator, which
        // this additive channel mixes in at its lowest level).
        let peak = samples.iter().map(|s| s.abs()).max().unwrap();
        assert!((3900..=4200).contains(&peak), "{peak}");
    }

    #[test]
    fn a_released_note_fades_to_silence() {
        let mut chip = Opl2::new();
        sine(&mut chip, 0x200, 4);
        for _ in 0..1000 {
            chip.sample();
        }
        chip.write(0xb0, 0x10 | 0x02);
        let tail: Vec<i32> = (0..RATE as usize / 2).map(|_| chip.sample()).collect();
        assert!(tail[..100].iter().any(|s| s.abs() > 1000));
        assert!(tail[tail.len() - 100..].iter().all(|&s| s == 0));
    }

    #[test]
    fn total_level_attenuates() {
        let mut loud = Opl2::new();
        sine(&mut loud, 0x200, 4);
        let mut quiet = Opl2::new();
        sine(&mut quiet, 0x200, 4);
        quiet.write(0x43, 0x08); // 6 dB
        let peak = |c: &mut Opl2| (0..5000).map(|_| c.sample().abs()).max().unwrap();
        let (a, b) = (peak(&mut loud), peak(&mut quiet));
        let ratio = f64::from(a) / f64::from(b);
        assert!((1.9..2.1).contains(&ratio), "{a} / {b}");
    }
}
