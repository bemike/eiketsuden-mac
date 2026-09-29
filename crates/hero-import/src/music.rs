//! The original's music: `MUSIC.R3`, `OPMUSIC.R3` and `EDMUSIC.R3` hold songs for KOEI's
//! Sound Blaster driver `SBOPL2.COM` (docs/reverse-engineering/FORMATS.md §16). [`render`] plays a
//! song the way the driver does — the same event parsing, instrument loading, volume, note,
//! tie and loop handling, instrument LFOs and pitch slides, on the driver's two clocks — into
//! the OPL2 emulator ([`crate::opl`]) and returns the sound as 16-bit mono samples.
//!
//! The driver runs off the PC timer at 1193182 / 4096 Hz. Each timer tick adds the tempo
//! increment (`tempo × 180`) to a 16-bit accumulator and advances the song by one step on its
//! overflow (tempo 120 ≈ 96 steps a second), and adds 0x36 to an 8-bit accumulator that
//! advances the effects (LFOs, slides) on its overflow (≈ 61 Hz).

use crate::opl::{Opl2, RATE};
use std::sync::atomic::{AtomicBool, Ordering};

/// Seconds between two driver timer ticks.
const TIMER_TICK: f64 = 4096.0 / 1_193_182.0;
/// Channels a song has (the driver's music channels; 7 and 8 are for sound effects).
pub const TRACKS: usize = 7;
/// Operator register offset of each channel's modulator.
const OPERATOR: [u8; TRACKS] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0a, 0x10];
/// Bytes of an instrument.
const PATCH_LEN: usize = 22;
/// F-number of each semitone of an octave.
const FNUM: [u16; 12] = [
    0x157, 0x16b, 0x181, 0x198, 0x1b0, 0x1ca, 0x1e5, 0x202, 0x220, 0x241, 0x263, 0x287,
];
/// Pitch slide range by semitones (0–12), as a fraction of 0x7fff.
const SLIDE_RANGE: [u16; 13] = [
    0, 1948, 4013, 6200, 8517, 10972, 13573, 16328, 19247, 22340, 25617, 29089, 32767,
];
/// Instrument LFO pitch depth scales.
const LFO_SCALE: [u16; 16] = [
    76, 114, 190, 266, 381, 766, 1948, 4013, 8517, 10972, 16328, 19247, 22340, 32767, 40792, 59912,
];

/// Songs of a music file: `[u16 count][count × (u32 offset, u16 length)]`, each song at
/// `2 + count × 6 + offset`.
pub fn songs(file: &[u8]) -> Result<Vec<&[u8]>, String> {
    let count = usize::from(u16::from_le_bytes(
        file.get(..2)
            .ok_or("shorter than its header")?
            .try_into()
            .unwrap(),
    ));
    let base = 2 + count * 6;
    (0..count)
        .map(|i| {
            let at = 2 + i * 6;
            let entry = file
                .get(at..at + 6)
                .ok_or_else(|| format!("song {i}: the table is cut short"))?;
            let offset = u32::from_le_bytes(entry[..4].try_into().unwrap()) as usize;
            let len = usize::from(u16::from_le_bytes(entry[4..].try_into().unwrap()));
            base.checked_add(offset)
                .and_then(|start| file.get(start..start.checked_add(len)?))
                .ok_or_else(|| {
                    format!("song {i}: {len} bytes at {base} + {offset} run past the file")
                })
        })
        .collect()
}

/// A song rendered to 16-bit mono samples at `rate` Hz.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub rate: u32,
    pub samples: Vec<i16>,
    /// Whether the samples are one pass of the song's loop, to be repeated without a seam: every
    /// track loops from the same step. Otherwise they are the song played once from its start.
    pub seamless: bool,
    /// Seconds of the song before its loop point, left out of `samples`: 0 without a loop point
    /// and when the samples are not [`seamless`](Self::seamless) (they start at the song's start).
    pub intro_seconds: f64,
}

impl Rendered {
    /// A RIFF WAVE file of the samples.
    pub fn wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend(b"RIFF");
        out.extend((36 + data_len).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes()); // PCM
        out.extend(1u16.to_le_bytes()); // mono
        out.extend(self.rate.to_le_bytes());
        out.extend((self.rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend(data_len.to_le_bytes());
        for s in &self.samples {
            out.extend(s.to_le_bytes());
        }
        out
    }
}

/// Error of a render whose `cancel` was set ([`render_cancellable`]).
pub(crate) const CANCELLED: &str = "cancelled";

/// Song steps a second at most (tempo 255 overflows the step accumulator on every timer tick).
const MAX_STEPS_PER_SECOND: f64 = 1.0 / TIMER_TICK;

/// Render `song` at `rate` Hz for a player that repeats the samples, as the driver loops a song
/// (`FF` goes back to the track's `FE`, or to its start). When every track loops from the same
/// step the samples are one pass of the loop, taken from the second time round so the releases
/// of its end ring into its start as they do in the game; the intro before the loop point is left
/// out ([`Rendered::intro_seconds`]). Otherwise the song is played once from its start. Longer
/// than `max_seconds` is an error.
pub fn render(song: &[u8], rate: u32, max_seconds: f64) -> Result<Rendered, String> {
    render_cancellable(song, rate, max_seconds, &AtomicBool::new(false))
}

/// [`render`], giving up with an error as soon as `cancel` is set (checked at every timer
/// tick, so a song stops within a fraction of a second: the game drops a render it no longer
/// needs).
pub fn render_cancellable(
    song: &[u8],
    rate: u32,
    max_seconds: f64,
    cancel: &AtomicBool,
) -> Result<Rendered, String> {
    render_checking(song, rate, max_seconds, &mut || {
        cancel.load(Ordering::Relaxed)
    })
}

/// [`render_cancellable`] asking `cancelled` at every timer tick.
fn render_checking(
    song: &[u8],
    rate: u32,
    max_seconds: f64,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Rendered, String> {
    let mut cancelled = || {
        if cancelled() {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    };
    // The driver alone first: where the tracks end and where they loop to.
    let mut probe = Driver::new(song)?;
    if !probe.playing() {
        return Err("no track has any events".into());
    }
    let max_steps = (max_seconds * MAX_STEPS_PER_SECOND) as u64;
    let unfinished = |d: &Driver| {
        d.channels
            .iter()
            .any(|c| c.pos.is_some() && c.first_end.is_none())
    };
    while unfinished(&probe) {
        cancelled()?;
        probe.timer_tick();
        probe.writes.clear();
        if let Some(e) = probe.broken.take() {
            return Err(e);
        }
        if probe.steps > max_steps {
            return Err(format!("does not reach its end within {max_seconds} s"));
        }
    }
    let tracks: Vec<&Channel> = probe.channels.iter().filter(|c| c.pos.is_some()).collect();
    let end = tracks.iter().filter_map(|c| c.first_end).max().unwrap_or(0);
    let loop_step = tracks[0].loop_step;
    let seamless = tracks
        .iter()
        .all(|c| c.first_end == Some(end) && c.loop_step == loop_step);
    // Recorded from the step `start` is reached until `stop` is.
    let (start, stop) = if seamless {
        (end, end + (end - loop_step))
    } else {
        (0, end)
    };

    let mut driver = Driver::new(song)?;
    let mut chip = Opl2::new();
    for (reg, value) in driver.writes.drain(..) {
        chip.write(reg, value);
    }
    let ticks_per_sample = TIMER_TICK * RATE; // chip samples per timer tick
    let step = RATE / f64::from(rate); // chip samples per output sample
    let mut samples = Vec::new();
    let (mut acc, mut n) = (0i64, 0u32);
    let mut chip_time = 0.0f64; // chip samples until the next timer tick
    let mut out_time = 0.0f64; // chip samples until the next output sample
    let limit = (max_seconds * f64::from(rate)) as usize;
    let mut recording = start == 0;
    // Timer ticks, the one of the first song step, and the time from it to the loop's first.
    let mut ticks = 0u64;
    let mut first_step = None;
    let mut intro_seconds = None;
    loop {
        if chip_time <= 0.0 {
            cancelled()?;
            driver.timer_tick();
            ticks += 1;
            for (reg, value) in driver.writes.drain(..) {
                chip.write(reg, value);
            }
            if let Some(e) = driver.broken.take() {
                return Err(e);
            }
            if first_step.is_none() && driver.steps >= 1 {
                first_step = Some(ticks);
            }
            if let (None, Some(first)) = (intro_seconds, first_step) {
                if driver.steps >= loop_step {
                    intro_seconds = Some((ticks - first) as f64 * TIMER_TICK);
                }
            }
            if driver.steps >= stop {
                break;
            }
            if !recording && driver.steps >= start {
                recording = true;
                (acc, n, out_time) = (0, 0, 0.0);
            }
            chip_time += ticks_per_sample;
        }
        acc += i64::from(chip.sample());
        n += 1;
        chip_time -= 1.0;
        out_time -= 1.0;
        if out_time <= 0.0 {
            if recording {
                // A box filter over the chip samples of one output sample, doubled (one
                // channel at full level is 13 bits).
                let v = (acc * 2 / i64::from(n.max(1))).clamp(-32768, 32767);
                samples.push(v as i16);
                if samples.len() > limit {
                    return Err(format!("longer than {max_seconds} s"));
                }
            }
            acc = 0;
            n = 0;
            out_time += step;
        }
    }
    Ok(Rendered {
        rate,
        samples,
        seamless,
        intro_seconds: if seamless {
            intro_seconds.unwrap_or(0.0)
        } else {
            0.0
        },
    })
}

/// One music channel of the driver (its 0x30-byte record).
#[derive(Debug, Clone, Default)]
struct Channel {
    /// Position of the next event, `None` once the track has ended.
    pos: Option<usize>,
    /// `FE`: where the track loops to (its start without one).
    loop_point: usize,
    /// The song step that first reads the event at `loop_point`.
    loop_step: u64,
    /// The song step that first reaches the track's `FF`.
    first_end: Option<u64>,
    /// Bit 0x80 keyed on, 0x40 sounding, 0x20 tie, 0x08 LFO requested (`F8`), 0x04 the
    /// instrument has an LFO, 0x02 slide set, 0x01 not started.
    flags: u8,
    volume: u8,
    last_note: u8,
    duration: u8,
    gate: u8,
    duration_left: u8,
    gate_left: u8,
    repeat_left: u8,
    repeat_start: usize,
    repeat_exit: usize,
    /// The frequency word written to A0/B0 (F-number, block << 10).
    freq: u16,
    /// The total levels last computed (before the LFO's amplitude change).
    level: [u8; 2],
    /// The LFO's amplitude change of each operator (0–127).
    amp: [u8; 2],
    patch: Option<usize>,
    transpose: i8,
    /// Bit 0x80 LFO on, 0x40 slide running, 0x04 slide down, 0x03 LFO state.
    fx: u8,
    lfo_rate: u16,
    lfo_scale: u16,
    lfo_count: u16,
    lfo_dir: i16,
    lfo_value: i16,
    /// Register caches: 0x40 (both operators), A0, B0.
    tl: [u8; 2],
    a0: u8,
    b0: u8,
    /// Frames until a released channel is silenced.
    release_left: u8,
}

/// The driver playing one song.
struct Driver<'a> {
    song: &'a [u8],
    channels: [Channel; TRACKS],
    tempo: u8,
    tempo_inc: u16,
    song_acc: u16,
    fx_acc: u8,
    frame: u16,
    random: u16,
    bd: u8,
    /// Song steps played.
    steps: u64,
    /// Why the song cannot be played on.
    broken: Option<String>,
    writes: Vec<(u8, u8)>,
}

impl<'a> Driver<'a> {
    fn new(song: &'a [u8]) -> Result<Driver<'a>, String> {
        if song.len() < TRACKS * 2 {
            return Err(format!(
                "{} bytes: shorter than the track table",
                song.len()
            ));
        }
        let mut channels: [Channel; TRACKS] = Default::default();
        for (i, ch) in channels.iter_mut().enumerate() {
            let offset = usize::from(u16::from_le_bytes([song[i * 2], song[i * 2 + 1]]));
            if offset >= song.len() {
                return Err(format!("track {i} starts at {offset}, past the song"));
            }
            *ch = Channel {
                pos: (offset != 0).then_some(offset),
                loop_point: offset,
                // The first step reads the first events.
                loop_step: 1,
                flags: 0x01,
                last_note: 0xff,
                duration: 0x30,
                gate: 0x2d,
                duration_left: 1,
                lfo_dir: 1,
                tl: [0x3f, 0x3f],
                ..Channel::default()
            };
        }
        let mut driver = Driver {
            song,
            channels,
            tempo: 0,
            tempo_inc: 0,
            song_acc: 0,
            fx_acc: 0,
            frame: 0,
            random: 0x037d,
            bd: 0,
            steps: 0,
            broken: None,
            writes: vec![(0x01, 0x20), (0x08, 0x40), (0xbd, 0x00)],
        };
        for c in 0..TRACKS {
            driver.silence(c);
        }
        driver.set_tempo(0x78);
        Ok(driver)
    }

    fn playing(&self) -> bool {
        self.channels.iter().any(|c| c.pos.is_some())
    }

    fn byte(&self, at: usize) -> u8 {
        self.song.get(at).copied().unwrap_or(0xff)
    }

    fn write(&mut self, reg: u8, value: u8) {
        self.writes.push((reg, value));
    }

    fn set_tempo(&mut self, tempo: u8) {
        if tempo == 0 {
            return;
        }
        self.tempo = tempo;
        self.tempo_inc = (u32::from(tempo) * 0x464e / 100) as u16;
        self.song_acc = 0;
    }

    /// One timer tick: song steps and effect frames on their accumulators' overflows.
    fn timer_tick(&mut self) {
        let (acc, step) = self.song_acc.overflowing_add(self.tempo_inc);
        self.song_acc = acc;
        if step {
            self.steps += 1;
            for c in 0..TRACKS {
                self.step(c);
            }
        }
        let (acc, frame) = self.fx_acc.overflowing_add(0x36);
        self.fx_acc = acc;
        if frame {
            self.effects();
        }
    }

    /// Mute a channel: frequency off, fast release, total level off (`0x800`).
    fn silence(&mut self, c: usize) {
        let (ch, op) = (c as u8, OPERATOR[c]);
        self.channels[c].a0 = 0;
        self.channels[c].b0 = 0;
        self.write(0xa0 + ch, 0);
        self.write(0xb0 + ch, 0);
        self.write(0x80 + op, 0x0f);
        self.write(0x83 + op, 0x0f);
        self.write(0x40 + op, 0x3f);
        self.write(0x43 + op, 0x3f);
        let chan = &mut self.channels[c];
        chan.tl = [0x3f, 0x3f];
        chan.flags &= 0x1f;
        chan.fx &= !0x40;
        chan.release_left = 0;
    }

    fn key_off(&mut self, c: usize) {
        let chan = &mut self.channels[c];
        chan.b0 &= !0x20;
        chan.flags &= 0x5f;
        chan.fx &= !0x40;
        chan.release_left = 0xff;
        let b0 = chan.b0;
        self.write(0xb0 + c as u8, b0);
    }

    fn key_on(&mut self, c: usize) {
        let chan = &mut self.channels[c];
        chan.b0 |= 0x20;
        chan.flags |= 0xc0;
        chan.fx |= 0x03;
        chan.release_left = 0;
        let b0 = chan.b0;
        self.write(0xb0 + c as u8, b0);
    }

    /// Write the frequency word `w` (F-number, block << 10), keeping the key bit (`0x8d2`).
    fn write_freq(&mut self, c: usize, w: u16) {
        let chan = &self.channels[c];
        let current = (u16::from(chan.b0 & !0x20) << 8) | u16::from(chan.a0);
        let [lo, hi] = w.to_le_bytes();
        let hi = hi & !0x20;
        if current == ((u16::from(hi) << 8) | u16::from(lo)) {
            return;
        }
        let chan = &mut self.channels[c];
        chan.a0 = lo;
        chan.b0 = (chan.b0 & 0x20) | hi;
        let b0 = chan.b0;
        self.write(0xa0 + c as u8, lo);
        self.write(0xb0 + c as u8, b0);
    }

    /// Write operator `i`'s total level `level`, lowered by the LFO's amplitude change and
    /// keeping the key scale bits (`0x8a2`).
    fn write_level(&mut self, c: usize, i: usize, level: u8) {
        let chan = &self.channels[c];
        let loud = !level & 0x3f;
        let cut = (u16::from(loud) * u16::from(chan.amp[i]) / 127) as u8;
        let loud = loud.saturating_sub(cut);
        let value = (!loud & 0x3f) | (chan.tl[i] & 0xc0);
        if value == chan.tl[i] {
            return;
        }
        self.channels[c].tl[i] = value;
        self.write(0x40 + OPERATOR[c] + 3 * i as u8, value);
    }

    /// `F0`: the volume (attenuation 0–127) applied to the operators the connection makes
    /// audible (`0xa76`).
    fn apply_volume(&mut self, c: usize) {
        let chan = &self.channels[c];
        let Some(p) = chan.patch else { return };
        let attenuation = chan.volume.min(0x7f) / 2;
        let mut audible = self.byte(p + 2) | 0x02;
        for i in 0..2 {
            let mut level = self.byte(p + 5 + i) & 0x3f;
            if audible & 1 != 0 {
                level = (level + attenuation).min(0x3f);
            }
            audible >>= 1;
            self.channels[c].level[i] = level;
            self.write_level(c, i, level);
        }
    }

    /// `F4`: load the instrument at `p` (`0xb11`).
    fn load_patch(&mut self, c: usize, p: usize) {
        self.channels[c].patch = Some(p);
        self.silence(c);
        let (ch, op) = (c as u8, OPERATOR[c]);
        let regs = [
            0x20 + op,
            0x23 + op,
            0xc0 + ch,
            0xe0 + op,
            0xe3 + op,
            0x40 + op,
            0x43 + op,
            0x60 + op,
            0x63 + op,
            0x80 + op,
            0x83 + op,
        ];
        for (k, reg) in regs.into_iter().enumerate() {
            let value = self.byte(p + k);
            self.write(reg, value);
        }
        let (tl0, tl1, transpose, lfo) = (
            self.byte(p + 5),
            self.byte(p + 6),
            self.byte(p + 11) as i8,
            self.byte(p + 0x15),
        );
        let chan = &mut self.channels[c];
        chan.tl = [tl0, tl1];
        chan.transpose = transpose;
        chan.fx &= 0x7f;
        chan.flags &= !0x04;
        if lfo != 0 {
            chan.flags |= 0x04;
            if chan.flags & 0x08 != 0 {
                chan.fx |= 0x80;
                chan.lfo_count = 0;
                chan.lfo_value = 0;
                chan.lfo_dir = 1;
            }
        }
        chan.amp = [0, 0];
        self.apply_volume(c);
    }

    /// One song step of channel `c` (`0xccd`).
    fn step(&mut self, c: usize) {
        if self.channels[c].pos.is_none() {
            return;
        }
        let chan = &mut self.channels[c];
        chan.duration_left = chan.duration_left.wrapping_sub(1);
        if chan.duration_left == 0 {
            self.events(c);
            self.channels[c].flags &= !0x01;
            return;
        }
        if chan.gate_left != 0 {
            chan.gate_left -= 1;
            if chan.gate_left == 0 {
                self.key_off(c);
            }
        }
    }

    /// Read events up to the next note or rest (`0xcf1`).
    fn events(&mut self, c: usize) {
        // A malformed track cannot loop without a note forever.
        for _ in 0..100_000 {
            let Some(pos) = self.channels[c].pos else {
                return;
            };
            let b = self.byte(pos);
            let mut next = pos + 1;
            match b {
                0x00..=0x5f => {
                    let (duration, gate) = (self.byte(next), self.byte(next + 1));
                    next += 2;
                    self.channels[c].duration = duration;
                    self.channels[c].gate = gate;
                    self.channels[c].pos = Some(next);
                    self.note(c, b);
                    return;
                }
                0x60..=0xbf => {
                    self.channels[c].pos = Some(next);
                    self.note(c, b - 0x60);
                    return;
                }
                0xc0..=0xcf | 0xfa..=0xfc => {}
                0xd0..=0xdf => next = self.repeat(c, b & 0x0f, next),
                0xe0..=0xed | 0xf3 => next += 1,
                0xf5 => next += 2,
                0xee | 0xef => {
                    let bit = if b == 0xee { 0x80 } else { 0x40 };
                    self.bd = if self.byte(next) & 1 != 0 {
                        self.bd | bit
                    } else {
                        self.bd & !bit
                    };
                    let bd = self.bd;
                    self.write(0xbd, bd);
                    next += 1;
                }
                0xf0 => {
                    self.channels[c].volume = self.byte(next) & 0x7f;
                    self.apply_volume(c);
                    next += 1;
                }
                0xf1 => {
                    let d = 256 - u32::from(self.byte(next));
                    self.set_tempo((78_125 / (d * 18)) as u8);
                    next += 1;
                }
                0xf2 => {
                    self.set_tempo(self.byte(next));
                    next += 1;
                }
                0xf4 => {
                    let rel = i16::from_le_bytes([self.byte(next), self.byte(next + 1)]);
                    next += 2;
                    let p = pos as i64 + i64::from(rel);
                    match usize::try_from(p) {
                        Ok(p) if p + PATCH_LEN <= self.song.len() => self.load_patch(c, p),
                        _ => {
                            self.broken = Some(format!(
                                "track {c}: the instrument at {p} is outside the song"
                            ));
                            self.channels[c].pos = None;
                            return;
                        }
                    }
                }
                0xf6 | 0xf7 => {
                    let (depth, time) = (self.byte(next) as i8, self.byte(next + 1));
                    next += 2;
                    self.slide(c, depth, time, b == 0xf6);
                }
                0xf8 => {
                    let chan = &mut self.channels[c];
                    if chan.fx & 0x80 == 0 {
                        chan.flags |= 0x08;
                        if chan.flags & 0x04 != 0 {
                            chan.fx |= 0x83;
                        }
                    }
                }
                0xf9 => {
                    let chan = &mut self.channels[c];
                    if chan.fx & 0x80 != 0 {
                        chan.flags &= !0x08;
                        chan.fx &= 0x7f;
                        if chan.flags & 0xc0 != 0 {
                            let freq = chan.freq;
                            self.write_freq(c, freq);
                            self.channels[c].amp = [0, 0];
                            self.apply_volume(c);
                        }
                    }
                }
                0xfd => {
                    let chan = &mut self.channels[c];
                    if chan.repeat_left == 1 {
                        chan.repeat_left = 0;
                        chan.repeat_start = 0;
                        next = chan.repeat_exit;
                    }
                }
                0xfe => {
                    let steps = self.steps;
                    let chan = &mut self.channels[c];
                    chan.loop_point = next;
                    if chan.first_end.is_none() {
                        chan.loop_step = steps;
                    }
                }
                0xff => {
                    let steps = self.steps;
                    let chan = &mut self.channels[c];
                    chan.first_end.get_or_insert(steps);
                    next = chan.loop_point;
                }
            }
            self.channels[c].pos = Some(next);
        }
        self.broken = Some(format!(
            "track {c}: 100000 events without a note (a loop without notes?)"
        ));
        self.channels[c].pos = None;
    }

    /// `D0`–`DF`: repeat marks (`0xa3e`). Returns where to go on.
    fn repeat(&mut self, c: usize, n: u8, next: usize) -> usize {
        let chan = &mut self.channels[c];
        if n == 0 {
            if chan.repeat_start == 0 {
                chan.repeat_start = next;
            }
            return next;
        }
        if chan.repeat_start == 0 {
            return next;
        }
        if chan.repeat_left == 0 {
            chan.repeat_left = n + 1;
            chan.repeat_exit = next;
        }
        chan.repeat_left -= 1;
        if chan.repeat_left == 0 {
            chan.repeat_start = 0;
            next
        } else {
            chan.repeat_start
        }
    }

    /// `F6`/`F7`: a pitch slide of `depth` semitones (negative: down) over `time` (`0xb73`).
    fn slide(&mut self, c: usize, depth: i8, time: u8, tempo_relative: bool) {
        let frames = if tempo_relative {
            let t = if time == 0 { 256 } else { u32::from(time) };
            let tempo = u32::from(self.tempo.max(1));
            ((t * 0x4e2) / (tempo * 16)).max(1)
        } else {
            u32::from(time) + 1
        };
        let chan = &mut self.channels[c];
        chan.lfo_rate = (0x8000 / frames) as u16;
        chan.fx &= !0x04;
        let mut depth = i32::from(depth);
        if depth < 0 {
            depth = -depth;
            chan.fx |= 0x04;
        }
        let depth = if depth > 12 { 0 } else { depth as usize };
        chan.lfo_scale = SLIDE_RANGE[depth];
        chan.lfo_count = 0;
        chan.lfo_value = 0;
        chan.lfo_dir = 1;
        chan.flags |= 0x02;
        let freq = chan.freq;
        self.write_freq(c, freq);
        self.channels[c].amp = [0, 0];
        self.apply_volume(c);
    }

    /// A note or rest (`0xd31`).
    fn note(&mut self, c: usize, n: u8) {
        let gate = self.channels[c].gate;
        if gate == 0 {
            self.channels[c].last_note = 0xff;
            if self.channels[c].flags & 0x80 != 0 {
                self.key_off(c);
            }
        } else {
            let total = i32::from(n) + i32::from(self.channels[c].transpose);
            let (block, key, id) = if !(0..256).contains(&total) {
                (
                    0u16,
                    total.rem_euclid(12) as usize,
                    total.rem_euclid(12) as u8,
                )
            } else if total / 12 > 7 {
                (7, (total % 12) as usize, (total % 12) as u8 + 0x54)
            } else {
                ((total / 12) as u16, (total % 12) as usize, total as u8)
            };
            let chan = &self.channels[c];
            let retrigger = !(chan.flags & 0x20 != 0 && id == chan.last_note);
            if retrigger {
                if chan.flags & 0x20 == 0 && chan.flags & 0x80 != 0 {
                    self.key_off(c);
                }
                self.channels[c].last_note = id;
                let freq = FNUM[key] | (block << 10);
                self.write_freq(c, freq);
                self.channels[c].freq = freq;
                if self.channels[c].flags & 0x20 == 0 {
                    self.key_on(c);
                }
                self.channels[c].fx &= !0x40;
            }
            if self.channels[c].flags & 0x02 != 0 {
                self.channels[c].fx |= 0x40;
            }
        }
        let chan = &mut self.channels[c];
        chan.duration_left = chan.duration;
        chan.gate_left = chan.gate;
        chan.flags &= !0x22;
        if chan.gate > chan.duration {
            chan.flags |= 0x20;
        }
    }

    /// One effect frame: slides and instrument LFOs of every channel (`0x10a0`).
    fn effects(&mut self) {
        for c in 0..TRACKS {
            if self.channels[c].fx & 0x40 != 0 {
                let mut value = self.ramp(c);
                if self.channels[c].fx & 0x04 != 0 {
                    value = -value;
                }
                let freq = self.bend(c, value);
                if self.channels[c].lfo_value == 0x7fff {
                    self.channels[c].freq = freq;
                    self.channels[c].fx &= !0x40;
                }
            } else if self.channels[c].patch.is_some() {
                self.lfo(c);
            }
        }
        self.frame = self.frame.wrapping_add(1);
    }

    /// Step the LFO value by `rate × dir`; on a sign change undo it and report it (`0xe52`).
    fn lfo_add(&mut self, c: usize, amount: i32) -> bool {
        let chan = &mut self.channels[c];
        let old = chan.lfo_value;
        let new = (i32::from(old) + amount) as i16;
        if (old ^ new) < 0 {
            return true;
        }
        chan.lfo_value = new;
        false
    }

    /// A one-way ramp up to 0x7fff (`0xe98`).
    fn ramp(&mut self, c: usize) -> i32 {
        let chan = &self.channels[c];
        if chan.lfo_value != 0x7fff {
            let amount = i32::from(chan.lfo_rate as i16) * i32::from(chan.lfo_dir);
            if self.lfo_add(c, amount) {
                self.channels[c].lfo_value = 0x7fff;
            }
        }
        i32::from(self.channels[c].lfo_value)
    }

    /// The instrument LFO's waveform value (`0xf02`).
    fn wave(&mut self, c: usize, shape: u8) -> i32 {
        let value = match shape {
            1 => {
                let chan = &self.channels[c];
                let half = (0x7fff / u32::from(chan.lfo_rate.max(1))).max(1);
                let v = if (u32::from(chan.lfo_count) / half) & 1 == 1 {
                    -0x8000
                } else {
                    0x7fff
                };
                self.channels[c].lfo_value = v as i16;
                v
            }
            2 => {
                let chan = &self.channels[c];
                let amount = i32::from(chan.lfo_rate as i16) * 2 * i32::from(chan.lfo_dir);
                let old = chan.lfo_value;
                let new = (i32::from(old) + amount) as i16;
                if (old ^ new) < 0 && (i32::from(chan.lfo_dir) as i16 ^ new) < 0 {
                    self.channels[c].lfo_dir = -self.channels[c].lfo_dir;
                } else {
                    self.channels[c].lfo_value = new;
                }
                i32::from(self.channels[c].lfo_value)
            }
            3 => {
                let chan = &self.channels[c];
                let period = (0x7fff / u32::from(chan.lfo_rate.max(1))) | 1;
                // Signed arithmetic, as the driver's `imul` / `idiv`.
                if i32::from(chan.lfo_count as i16) % period as i32 == 0 {
                    self.random = ((i32::from(self.random as i16) * 0x383) % 0x7fff) as i16 as u16;
                }
                i32::from(self.random as i16)
            }
            4 => self.ramp(c),
            _ => {
                let chan = &self.channels[c];
                let amount = i32::from(chan.lfo_rate as i16) * i32::from(chan.lfo_dir);
                if self.lfo_add(c, amount) {
                    let chan = &mut self.channels[c];
                    chan.lfo_value = chan.lfo_value.wrapping_neg();
                }
                i32::from(self.channels[c].lfo_value)
            }
        };
        if value == -0x8000 {
            -0x7fff
        } else {
            value
        }
    }

    /// Bend the channel's frequency by `amount` (±0x7fff of the scale's range) and write it;
    /// returns the frequency word written (`0xf29`).
    fn bend(&mut self, c: usize, amount: i32) -> u16 {
        let chan = &self.channels[c];
        let freq = u32::from(chan.freq);
        let fnum = freq & 0x3ff;
        let mut block = freq & 0x1c00;
        let scale = u32::from(chan.lfo_scale);
        let fnum = if amount >= 0 {
            let delta = fnum * amount as u32 / 0x7fff * scale / 0x7fff;
            let mut f = fnum + delta;
            loop {
                if f <= 0x3ff {
                    break f;
                }
                if block >= 0x1c00 {
                    block = 0x1c00;
                    break 0x3ff;
                }
                block += 0x400;
                f >>= 1;
            }
        } else {
            let scaled = (-amount) as u32 * scale / 0x7fff;
            fnum * 0x7fff / (0x7fff + scaled)
        };
        let word = (fnum | block) as u16;
        self.write_freq(c, word);
        word
    }

    /// The instrument LFO of channel `c` (`0xffc`).
    fn lfo(&mut self, c: usize) {
        let Some(p) = self.channels[c].patch else {
            return;
        };
        if self.channels[c].fx & 0x80 == 0 {
            self.channels[c].lfo_count = self.channels[c].lfo_count.wrapping_add(1);
            return;
        }
        if self.channels[c].flags & 0xc0 == 0 {
            self.channels[c].fx |= 0x03;
            self.channels[c].lfo_count = self.channels[c].lfo_count.wrapping_add(1);
            return;
        }
        if self.channels[c].release_left != 0 {
            self.channels[c].release_left -= 1;
            if self.channels[c].release_left == 0 {
                self.silence(c);
            }
        }
        let shape = self.byte(p + 0x0e);
        let shape = if shape > 4 { 0 } else { shape };
        let chan = &mut self.channels[c];
        chan.lfo_scale = LFO_SCALE[usize::from(self.song.get(p + 0x10).copied().unwrap_or(0) & 15)];
        chan.lfo_rate = u16::from_le_bytes([
            self.song.get(p + 0x0c).copied().unwrap_or(0),
            self.song.get(p + 0x0d).copied().unwrap_or(0),
        ]);
        if chan.lfo_rate == 0 {
            chan.fx |= 0x03;
            chan.lfo_count = chan.lfo_count.wrapping_add(1);
            return;
        }
        let run = match chan.fx & 0x03 {
            0 => false,
            1 => {
                chan.lfo_count = chan.lfo_count.wrapping_sub(1);
                if chan.lfo_count != 0 {
                    return;
                }
                chan.lfo_value = 0;
                chan.lfo_dir = 1;
                chan.fx = (chan.fx & !0x03) | 0x02;
                true
            }
            2 => true,
            _ => {
                chan.fx = (chan.fx & !0x03) | 0x01;
                let delay = self.song.get(p + 0x14).copied().unwrap_or(0);
                match delay {
                    0 if shape != 4 => {
                        chan.lfo_count = self.frame;
                        chan.fx = (chan.fx & !0x03) | 0x02;
                        true
                    }
                    0 | 1 => {
                        chan.lfo_count = 0;
                        chan.lfo_value = 0;
                        chan.lfo_dir = 1;
                        chan.fx = (chan.fx & !0x03) | 0x02;
                        true
                    }
                    d => {
                        // The driver counts this frame too (it goes on to the count's `inc`).
                        chan.lfo_count = (u16::from(d) - 1) * 4 + 1;
                        return;
                    }
                }
            }
        };
        if run {
            let value = self.wave(c, shape);
            // Pitch.
            let depth = i32::from(self.byte(p + 0x0f) as i8);
            self.bend(c, depth * value / 127);
            // Amplitude.
            let shaped = match shape {
                4 => value * 2,
                1 => {
                    if value < 0 {
                        -1
                    } else {
                        0
                    }
                }
                // The triangle's magnitude doubled (`shl ax, 1` after it in the driver).
                2 => value.abs() * 2,
                _ => value,
            };
            let amp_depth = self.byte(p + 0x11) as i8;
            let magnitude = u32::from(amp_depth.unsigned_abs() & 0x7f);
            let mut w = (shaped as u16) as u32;
            if amp_depth < 0 {
                w ^= 0xffff;
            }
            let amount = w * magnitude / 0xfffe;
            for i in 0..2 {
                let per_op = u32::from(self.byte(p + 0x12 + i) & 0x0f);
                self.channels[c].amp[i] = (amount * per_op / 15).min(127) as u8;
                let level = self.channels[c].level[i];
                self.write_level(c, i, level);
            }
        }
        self.channels[c].lfo_count = self.channels[c].lfo_count.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A song of `tracks`, each given its events after an instrument and a volume.
    fn song_of(tracks: &[&[u8]]) -> Vec<u8> {
        let mut s = vec![0u8; 14];
        let patch_at = s.len();
        // AM/VIB/EG/KSR/MULT ×2, FB/CON, waves ×2, TL ×2, AR/DR ×2, SL/RR ×2, transpose, LFO 0.
        s.extend([
            0x21, 0x21, 0x01, 0, 0, 0x3f, 0x00, 0xf0, 0xf0, 0x0f, 0x0f, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0,
        ]);
        for (i, events) in tracks.iter().enumerate() {
            let track = s.len();
            s[i * 2..i * 2 + 2].copy_from_slice(&(track as u16).to_le_bytes());
            let rel = (patch_at as i64 - track as i64) as i16;
            s.push(0xf4);
            s.extend(rel.to_le_bytes());
            s.extend([0xf0, 0x00]);
            s.extend(*events);
        }
        s
    }

    /// A song: one track with an instrument, a volume, two notes and the end.
    fn song() -> Vec<u8> {
        // A4 (note 57) for 48 steps, then a rest, then the end.
        song_of(&[&[57, 48, 40, 0x00, 24, 0, 0xff]])
    }

    /// Crossings of zero upwards a second in the first `seconds` of `samples`.
    fn pitch(samples: &[i16], seconds: f64) -> f64 {
        let tone = &samples[..(seconds * 22050.0) as usize];
        tone.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count() as f64 / seconds
    }

    #[test]
    fn the_container_lists_its_songs() {
        let mut file = vec![2, 0];
        file.extend(0u32.to_le_bytes());
        file.extend(3u16.to_le_bytes());
        file.extend(3u32.to_le_bytes());
        file.extend(2u16.to_le_bytes());
        file.extend([1, 2, 3, 4, 5]);
        assert_eq!(songs(&file).unwrap(), [&[1, 2, 3][..], &[4, 5][..]]);
        file.pop();
        assert!(songs(&file).unwrap_err().contains("song 1"));
    }

    #[test]
    fn a_song_plays_its_notes_for_their_steps() {
        let rendered = render(&song(), 22050, 10.0).unwrap();
        // The whole song loops: 72 steps at tempo 120 (≈ 96 a second), no intro.
        assert!(rendered.seamless);
        assert_eq!(rendered.intro_seconds, 0.0);
        let seconds = rendered.samples.len() as f64 / 22050.0;
        assert!((0.72..0.78).contains(&seconds), "{seconds}");
        // A4 sounds for 40 of the note's 48 steps: about 0.42 s of a 440 Hz tone.
        let hz = pitch(&rendered.samples, 0.3);
        assert!((420.0..460.0).contains(&hz), "{hz}");
        // Silent at the end.
        let end = &rendered.samples[rendered.samples.len() - 200..];
        assert!(end.iter().all(|&s| s.abs() < 50));
        // A valid WAVE file.
        let wav = rendered.wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + rendered.samples.len() * 2);
    }

    #[test]
    fn a_cancelled_render_stops() {
        let cancel = AtomicBool::new(true);
        assert_eq!(
            render_cancellable(&song(), 22050, 10.0, &cancel).unwrap_err(),
            CANCELLED
        );
        cancel.store(false, Ordering::Relaxed);
        assert!(render_cancellable(&song(), 22050, 10.0, &cancel).is_ok());
        // Cancelled while the samples are rendered (the last check of a whole render is in
        // that loop, after the driver's first pass).
        let mut checks = 0;
        render_checking(&song(), 22050, 10.0, &mut || {
            checks += 1;
            false
        })
        .unwrap();
        let mut asked = 0;
        let cancelled = render_checking(&song(), 22050, 10.0, &mut || {
            asked += 1;
            asked == checks
        });
        assert_eq!(cancelled.unwrap_err(), CANCELLED);
        assert_eq!(asked, checks);
    }

    #[test]
    fn repeats_play_their_body_again() {
        let mut s = song();
        // D0, the note, D1: the note twice.
        let end = s.len() - 4;
        s.splice(end..end, [0xd1]);
        let note = end - 3;
        s.splice(note..note, [0xd0]);
        let once = render(&song(), 22050, 10.0).unwrap().samples.len();
        let twice = render(&s, 22050, 10.0).unwrap().samples.len();
        let step = 22050.0 / 96.0;
        let extra = (twice - once) as f64 / step;
        assert!((46.0..50.0).contains(&extra), "{extra}");
    }

    #[test]
    fn the_loop_is_rendered_without_its_intro() {
        // A4 for 24 steps, the loop point, C5 for 48 steps, the end.
        let rendered = render(
            &song_of(&[&[57, 24, 20, 0xfe, 60, 48, 40, 0xff]]),
            22050,
            10.0,
        )
        .unwrap();
        assert!(rendered.seamless);
        assert!(
            (0.23..0.27).contains(&rendered.intro_seconds),
            "{}",
            rendered.intro_seconds
        );
        let seconds = rendered.samples.len() as f64 / 22050.0;
        assert!((0.48..0.52).contains(&seconds), "{seconds}");
        // The samples start with the loop's C5 (523 Hz), not the intro's A4.
        let hz = pitch(&rendered.samples, 0.3);
        assert!((500.0..545.0).contains(&hz), "{hz}");
    }

    #[test]
    fn tracks_looping_from_different_steps_play_once() {
        let rendered = render(
            &song_of(&[&[57, 24, 20, 0xfe, 60, 48, 40, 0xff], &[45, 72, 60, 0xff]]),
            22050,
            10.0,
        )
        .unwrap();
        assert!(!rendered.seamless);
        assert_eq!(rendered.intro_seconds, 0.0);
        let seconds = rendered.samples.len() as f64 / 22050.0;
        assert!((0.72..0.78).contains(&seconds), "{seconds}");
    }

    #[test]
    fn broken_songs_are_errors() {
        assert!(render(&[0; 4], 22050, 1.0).is_err());
        let mut s = vec![0u8; 14];
        s[0..2].copy_from_slice(&100u16.to_le_bytes());
        assert!(render(&s, 22050, 1.0).unwrap_err().contains("track 0"));
        // An instrument before the song's start.
        let mut s = song();
        s[37] = 0x00;
        s[38] = 0x80;
        assert!(render(&s, 22050, 1.0).unwrap_err().contains("instrument"));
        // A loop without notes.
        let e = render(&song_of(&[&[0xfe, 0xff]]), 22050, 1.0).unwrap_err();
        assert!(e.contains("without a note"), "{e}");
        // Too long for the limit: the song does not end within it, or its loop is longer.
        let e = render(&song(), 22050, 0.2).unwrap_err();
        assert!(e.contains("does not reach its end"), "{e}");
        let e = render(&song(), 22050, 0.5).unwrap_err();
        assert!(e.contains("longer than"), "{e}");
    }
}
