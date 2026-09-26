//! Asset loading: non-blocking file reads, the data pack loader and the lazy media store.
//!
//! macroquad's `load_file` is a future: a plain file read natively, an HTTP fetch on the web.
//! Nothing here awaits it directly — [`FileRequest`] / [`FileBatch`] poll their futures once per
//! frame so screens keep drawing (progress bars, spinners) while files arrive, and many files are
//! fetched in parallel on the web.
//!
//! # Media store
//!
//! [`Media`] hands out textures and sounds **by key** and loads them on first use:
//!
//! | kind | key example | file |
//! |---|---|---|
//! | texture | `portraits/liu_bei`, `bg/palace`, `units/archer_player`, `ui/title` | `gfx/<key>.png` |
//! | sound | `bgm/title` | `bgm/title.ogg` |
//! | sound | `sfx/cursor` | `sfx/cursor.wav`, else `sfx/cursor.ogg` |
//! | icon | `gold` | cell of `gfx/ui/icons.png` listed in `gfx/ui/icons.toml` |
//!
//! A request returns `None` while the file is loading and after it failed; failures are logged
//! once and never retried or fatal (natively, sound files are checked before macroquad decodes
//! them, because its decoder panics on files it cannot read — see `prepare_sound`). Callers draw a fallback: [`Media::portrait`] falls back to
//! `portraits/_unknown`, and `crate::ui` draws procedural placeholders for anything else. Pixel
//! art uses nearest filtering; hi-res art (`portraits/`, `bg/`, `ui/title`) uses linear filtering
//! so it stays smooth when shown smaller than its native resolution.
//!
//! [`Media::pump`] (called by the app every frame) advances at most [`MAX_IN_FLIGHT`] loads and
//! decodes at most [`DECODES_PER_FRAME`] images per frame, so lazy loading never stalls a frame
//! for long. Decoded music is large (PCM); [`Media::release_sound`] drops a track that is no
//! longer needed (the audio manager does this when music changes).

use crate::platform::DataRoot;
use macroquad::audio::{load_sound_from_bytes, Sound};
use macroquad::prelude::*;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

/// Maximum number of media files loading at the same time.
pub const MAX_IN_FLIGHT: usize = 6;
/// Maximum number of images decoded per frame.
pub const DECODES_PER_FRAME: usize = 2;
/// Seconds after which a sound that is still decoding is given up (the web audio decoder never
/// reports some failures).
pub const SOUND_DECODE_TIMEOUT: f64 = 15.0;

type BoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;
type BytesResult = Result<Vec<u8>, String>;

/// Poll a future once without blocking. macroquad's futures never use the waker (they are
/// polled every frame), so a no-op waker is sufficient.
pub fn poll_once<T>(future: &mut BoxFuture<T>) -> Option<T> {
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(v) => Some(v),
        Poll::Pending => None,
    }
}

/// Readable description of a macroquad error.
pub fn describe_error(e: &macroquad::Error) -> String {
    match e {
        macroquad::Error::FileError { kind, path } => format!("{path}: {kind}"),
        macroquad::Error::ImageError(err) => format!("image: {err}"),
        other => format!("{other:?}"),
    }
}

fn fetch(path: String) -> BoxFuture<BytesResult> {
    Box::pin(async move { load_file(&path).await.map_err(|e| describe_error(&e)) })
}

/// One file being read (by full path/URL, see [`DataRoot::path`]).
pub struct FileRequest {
    future: Option<BoxFuture<BytesResult>>,
    result: Option<BytesResult>,
}

impl FileRequest {
    pub fn new(path: String) -> FileRequest {
        FileRequest {
            future: Some(fetch(path)),
            result: None,
        }
    }

    /// Advance the read; returns the result once it is available.
    pub fn poll(&mut self) -> Option<&BytesResult> {
        if let Some(fut) = self.future.as_mut() {
            if let Some(r) = poll_once(fut) {
                self.result = Some(r);
                self.future = None;
            }
        }
        self.result.as_ref()
    }
}

/// Several pack files read in parallel, keyed by their pack-relative path.
pub struct FileBatch {
    pending: Vec<(String, BoxFuture<BytesResult>)>,
    done: BTreeMap<String, BytesResult>,
    total: usize,
}

impl FileBatch {
    pub fn new(root: &DataRoot, files: impl IntoIterator<Item = String>) -> FileBatch {
        let mut pending = Vec::new();
        for rel in files {
            if pending.iter().any(|(r, _)| *r == rel) {
                continue;
            }
            let fut = fetch(root.path(&rel));
            pending.push((rel, fut));
        }
        FileBatch {
            total: pending.len(),
            pending,
            done: BTreeMap::new(),
        }
    }

    /// Advance every read; returns `true` once all files have a result.
    pub fn poll(&mut self) -> bool {
        let mut i = 0;
        while i < self.pending.len() {
            if let Some(result) = poll_once(&mut self.pending[i].1) {
                let (rel, _) = self.pending.swap_remove(i);
                self.done.insert(rel, result);
            } else {
                i += 1;
            }
        }
        self.pending.is_empty()
    }

    /// `(finished, total)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.done.len(), self.total)
    }

    /// A file still loading (for the progress display).
    pub fn current(&self) -> Option<&str> {
        self.pending.first().map(|(r, _)| r.as_str())
    }

    /// Results by pack-relative path.
    pub fn into_results(self) -> BTreeMap<String, BytesResult> {
        self.done
    }
}

// ----- media store ---------------------------------------------------------------------------

/// Load state of a media key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetState {
    Loading,
    Ready,
    Missing,
}

enum Slot<T> {
    Loading,
    Ready(T),
    Missing,
}

impl<T> Slot<T> {
    fn state(&self) -> AssetState {
        match self {
            Slot::Loading => AssetState::Loading,
            Slot::Ready(_) => AssetState::Ready,
            Slot::Missing => AssetState::Missing,
        }
    }
}

enum JobKind {
    Texture {
        key: String,
    },
    /// `paths` are the remaining candidate files, tried in order; `errors` collects why the
    /// earlier candidates failed.
    Sound {
        key: String,
        paths: Vec<String>,
        errors: Vec<String>,
    },
    IconIndex,
}

enum Stage {
    Waiting,
    Fetch(BoxFuture<BytesResult>),
    Decode(Vec<u8>),
    DecodeSound {
        future: BoxFuture<Result<Sound, macroquad::Error>>,
        started: f64,
    },
}

struct Job {
    kind: JobKind,
    stage: Stage,
}

/// Cells of `gfx/ui/icons.png`.
#[derive(Debug, Clone, Deserialize)]
struct IconIndex {
    #[serde(default = "default_icon_size")]
    tile_size: u32,
    icons: BTreeMap<String, [u32; 2]>,
}

fn default_icon_size() -> u32 {
    16
}

const ICON_TEXTURE: &str = "ui/icons";
const ICON_INDEX: &str = "gfx/ui/icons.toml";
/// Portrait used for officers without their own portrait.
pub const UNKNOWN_PORTRAIT: &str = "portraits/_unknown";

#[derive(Default)]
struct Inner {
    textures: HashMap<String, Slot<Texture2D>>,
    sounds: HashMap<String, Slot<Sound>>,
    icons: Option<Slot<IconIndex>>,
    jobs: VecDeque<Job>,
}

/// Lazily loaded, cached textures, sounds and icons. See the module docs.
pub struct Media {
    root: DataRoot,
    inner: RefCell<Inner>,
}

impl Media {
    pub fn new(root: DataRoot) -> Media {
        Media {
            root,
            inner: RefCell::new(Inner::default()),
        }
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    fn texture_filter(key: &str) -> FilterMode {
        if key.starts_with("portraits/") || key.starts_with("bg/") || key == "ui/title" {
            FilterMode::Linear
        } else {
            FilterMode::Nearest
        }
    }

    fn request_texture(&self, key: &str) -> AssetState {
        let mut inner = self.inner.borrow_mut();
        if let Some(slot) = inner.textures.get(key) {
            return slot.state();
        }
        inner.textures.insert(key.to_string(), Slot::Loading);
        inner.jobs.push_back(Job {
            kind: JobKind::Texture {
                key: key.to_string(),
            },
            stage: Stage::Waiting,
        });
        AssetState::Loading
    }

    /// Texture `gfx/<key>.png`; `None` while loading or if it does not exist.
    pub fn texture(&self, key: &str) -> Option<Texture2D> {
        if self.request_texture(key) != AssetState::Ready {
            return None;
        }
        match self.inner.borrow().textures.get(key) {
            Some(Slot::Ready(t)) => Some(t.clone()),
            _ => None,
        }
    }

    /// Load state of a texture (requests it when unknown).
    pub fn texture_state(&self, key: &str) -> AssetState {
        self.request_texture(key)
    }

    /// Portrait `portraits/<key>`, falling back to `portraits/_unknown`. `None` while loading or
    /// when neither exists (draw a procedural silhouette then).
    pub fn portrait(&self, key: &str) -> Option<Texture2D> {
        let full = format!("portraits/{key}");
        match self.texture_state(&full) {
            AssetState::Ready => self.texture(&full),
            AssetState::Loading => None,
            AssetState::Missing => self.texture(UNKNOWN_PORTRAIT),
        }
    }

    fn request_sound(&self, key: &str) -> AssetState {
        let mut inner = self.inner.borrow_mut();
        if let Some(slot) = inner.sounds.get(key) {
            return slot.state();
        }
        let paths = if let Some(name) = key.strip_prefix("sfx/") {
            // The base pack ships its effects as WAV, so that is tried first: every failed probe
            // is a 404 in the browser console.
            vec![format!("sfx/{name}.wav"), format!("sfx/{name}.ogg")]
        } else {
            vec![format!("{key}.ogg")]
        };
        inner.sounds.insert(key.to_string(), Slot::Loading);
        inner.jobs.push_back(Job {
            kind: JobKind::Sound {
                key: key.to_string(),
                paths,
                errors: Vec::new(),
            },
            stage: Stage::Waiting,
        });
        AssetState::Loading
    }

    /// Sound by key (`bgm/<name>` or `sfx/<name>`); `None` while loading or if missing.
    pub fn sound(&self, key: &str) -> Option<Sound> {
        if self.request_sound(key) != AssetState::Ready {
            return None;
        }
        match self.inner.borrow().sounds.get(key) {
            Some(Slot::Ready(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Load state of a sound (requests it when unknown).
    pub fn sound_state(&self, key: &str) -> AssetState {
        self.request_sound(key)
    }

    /// Forget a loaded sound so its memory can be freed once nothing plays it any more. A later
    /// request loads it again.
    pub fn release_sound(&self, key: &str) {
        let mut inner = self.inner.borrow_mut();
        if matches!(inner.sounds.get(key), Some(Slot::Ready(_))) {
            inner.sounds.remove(key);
        }
    }

    /// Icon cell by key: the atlas texture and the source rectangle in texels.
    pub fn icon(&self, key: &str) -> Option<(Texture2D, Rect)> {
        let atlas = self.texture(ICON_TEXTURE)?;
        let mut inner = self.inner.borrow_mut();
        match &inner.icons {
            None => {
                inner.icons = Some(Slot::Loading);
                inner.jobs.push_back(Job {
                    kind: JobKind::IconIndex,
                    stage: Stage::Waiting,
                });
                None
            }
            Some(Slot::Ready(index)) => {
                let [col, row] = *index.icons.get(key)?;
                let t = index.tile_size as f32;
                Some((atlas, Rect::new(col as f32 * t, row as f32 * t, t, t)))
            }
            Some(_) => None,
        }
    }

    /// Load state of an icon: `Missing` when the atlas, its index or the key does not exist.
    pub fn icon_state(&self, key: &str) -> AssetState {
        match self.texture_state(ICON_TEXTURE) {
            AssetState::Ready => {}
            other => return other,
        }
        if self.icon(key).is_some() {
            return AssetState::Ready;
        }
        match &self.inner.borrow().icons {
            Some(Slot::Ready(_)) | Some(Slot::Missing) => AssetState::Missing,
            _ => AssetState::Loading,
        }
    }

    /// Start loading textures ahead of use.
    pub fn preload_textures<S: AsRef<str>>(&self, keys: &[S]) {
        for k in keys {
            self.request_texture(k.as_ref());
        }
    }

    /// Start loading sounds ahead of use.
    pub fn preload_sounds<S: AsRef<str>>(&self, keys: &[S]) {
        for k in keys {
            self.request_sound(k.as_ref());
        }
    }

    /// Number of media loads not finished yet.
    pub fn pending(&self) -> usize {
        self.inner.borrow().jobs.len()
    }

    /// Advance loading; called by the app once per frame.
    pub fn pump(&self) {
        let mut inner = self.inner.borrow_mut();
        let mut decodes = 0;
        let mut i = 0;
        while i < inner.jobs.len() {
            let in_flight = i < MAX_IN_FLIGHT;
            let finished = {
                let job = &mut inner.jobs[i];
                if in_flight {
                    self.advance(job, &mut decodes)
                } else {
                    None
                }
            };
            match finished {
                Some(outcome) => {
                    let job = inner.jobs.remove(i).expect("index in range");
                    Self::finish(&mut inner, job.kind, outcome);
                }
                None => i += 1,
            }
        }
    }

    /// Advance one job; `Some` when it finished.
    fn advance(&self, job: &mut Job, decodes: &mut usize) -> Option<Outcome> {
        loop {
            match &mut job.stage {
                Stage::Waiting => {
                    let path = match &mut job.kind {
                        JobKind::Texture { key } => format!("gfx/{key}.png"),
                        JobKind::Sound { paths, errors, .. } => {
                            if paths.is_empty() {
                                return Some(Outcome::Failed(errors.join("; ")));
                            }
                            paths.remove(0)
                        }
                        JobKind::IconIndex => ICON_INDEX.to_string(),
                    };
                    job.stage = Stage::Fetch(fetch(self.root.path(&path)));
                }
                Stage::Fetch(fut) => {
                    let result = poll_once(fut)?;
                    match (result, &mut job.kind) {
                        (Ok(bytes), JobKind::Sound { .. }) => {
                            let bytes = match prepare_sound(bytes) {
                                Ok(bytes) => bytes,
                                Err(e) => return Some(Outcome::Failed(e)),
                            };
                            job.stage = Stage::DecodeSound {
                                future: Box::pin(
                                    async move { load_sound_from_bytes(&bytes).await },
                                ),
                                started: get_time(),
                            };
                        }
                        (Ok(bytes), _) => job.stage = Stage::Decode(bytes),
                        (Err(e), JobKind::Sound { errors, .. }) => {
                            // Try the next candidate (`.ogg` after `.wav`); fails once none is left.
                            errors.push(e);
                            job.stage = Stage::Waiting;
                        }
                        (Err(e), _) => return Some(Outcome::Failed(e)),
                    }
                }
                Stage::Decode(bytes) => {
                    if *decodes >= DECODES_PER_FRAME {
                        return None;
                    }
                    *decodes += 1;
                    let bytes = std::mem::take(bytes);
                    return Some(match &job.kind {
                        JobKind::Texture { key } => {
                            match Image::from_file_with_format(&bytes, Some(ImageFormat::Png)) {
                                Ok(img) => {
                                    let tex = Texture2D::from_image(&img);
                                    tex.set_filter(Self::texture_filter(key));
                                    Outcome::Texture(tex)
                                }
                                Err(e) => Outcome::Failed(describe_error(&e)),
                            }
                        }
                        JobKind::IconIndex => match std::str::from_utf8(&bytes)
                            .map_err(|e| e.to_string())
                            .and_then(|s| toml::from_str::<IconIndex>(s).map_err(|e| e.to_string()))
                        {
                            Ok(index) => Outcome::Icons(index),
                            Err(e) => Outcome::Failed(format!("{ICON_INDEX}: {e}")),
                        },
                        JobKind::Sound { .. } => Outcome::Failed("unexpected decode stage".into()),
                    });
                }
                Stage::DecodeSound { future, started } => {
                    if let Some(result) = poll_once(future) {
                        return Some(match result {
                            Ok(sound) => Outcome::Sound(sound),
                            Err(e) => Outcome::Failed(describe_error(&e)),
                        });
                    }
                    if get_time() - *started > SOUND_DECODE_TIMEOUT {
                        return Some(Outcome::Failed("audio decoding timed out".into()));
                    }
                    return None;
                }
            }
        }
    }

    fn finish(inner: &mut Inner, kind: JobKind, outcome: Outcome) {
        match (kind, outcome) {
            (JobKind::Texture { key }, Outcome::Texture(t)) => {
                inner.textures.insert(key, Slot::Ready(t));
            }
            (JobKind::Sound { key, .. }, Outcome::Sound(s)) => {
                inner.sounds.insert(key, Slot::Ready(s));
            }
            (JobKind::IconIndex, Outcome::Icons(index)) => inner.icons = Some(Slot::Ready(index)),
            (kind, Outcome::Failed(why)) => {
                let what = match &kind {
                    JobKind::Texture { key } => format!("texture `{key}`"),
                    JobKind::Sound { key, .. } => format!("sound `{key}`"),
                    JobKind::IconIndex => "icon index".to_string(),
                };
                macroquad::logging::warn!("media: {} unavailable ({})", what, why);
                match kind {
                    JobKind::Texture { key } => {
                        inner.textures.insert(key, Slot::Missing);
                    }
                    JobKind::Sound { key, .. } => {
                        inner.sounds.insert(key, Slot::Missing);
                    }
                    JobKind::IconIndex => inner.icons = Some(Slot::Missing),
                }
            }
            (kind, _) => {
                // A job always produces the outcome of its own kind; keep the slot consistent
                // if that invariant is ever broken.
                macroquad::logging::error!("media: mismatched load outcome");
                match kind {
                    JobKind::Texture { key } => {
                        inner.textures.insert(key, Slot::Missing);
                    }
                    JobKind::Sound { key, .. } => {
                        inner.sounds.insert(key, Slot::Missing);
                    }
                    JobKind::IconIndex => inner.icons = Some(Slot::Missing),
                }
            }
        }
    }
}

enum Outcome {
    Texture(Texture2D),
    Sound(Sound),
    Icons(IconIndex),
    Failed(String),
}

/// Make a sound file safe for macroquad's native audio backend; returns the bytes to decode.
///
/// quad-snd 0.2 decodes with audrey and **panics** instead of returning an error when the file
/// is not WAV / Ogg Vorbis, has more than two channels or has a sample that fails to decode (a
/// truncated download, an MP3 renamed to `.ogg`, 5.1 audio), which would close the game. This
/// runs the same decoder first, so such a sound becomes missing (silent) like any other media
/// failure.
///
/// A WAV file is returned unchanged (decoding it twice is cheap). Ogg Vorbis is slow to decode
/// (about 0.4 s for a four-minute track in a release build, on the main thread), so instead of
/// decoding it a second time the decoded samples are handed over as a 16-bit PCM WAV file, which
/// quad-snd reads quickly; loading music then takes about a quarter longer rather than twice as
/// long. That is lossless: the Vorbis decoder produces 16-bit samples, which quad-snd would
/// convert exactly the same way.
#[cfg(not(target_arch = "wasm32"))]
fn prepare_sound(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    fn decode_error(e: impl std::fmt::Display) -> String {
        format!("cannot decode audio: {e}")
    }
    let mut reader = audrey::Reader::new(std::io::Cursor::new(&bytes[..])).map_err(decode_error)?;
    let description = reader.description();
    let channels = description.channel_count();
    if !(1..=2).contains(&channels) {
        return Err(format!(
            "{channels} audio channels (only mono and stereo are supported)"
        ));
    }
    let rate = description.sample_rate();
    if rate == 0 {
        return Err("audio sample rate is 0".into());
    }
    let transcode = reader.format() == audrey::Format::OggVorbis;
    let mut wav = Vec::new();
    let mut samples: u64 = 0;
    if transcode {
        wav.resize(WAV_HEADER_LEN, 0);
        for sample in reader.samples::<i16>() {
            wav.extend_from_slice(&sample.map_err(decode_error)?.to_le_bytes());
            samples += 1;
        }
    } else {
        for sample in reader.samples::<f32>() {
            sample.map_err(decode_error)?;
            samples += 1;
        }
    }
    if channels == 2 && samples % 2 != 0 {
        return Err("stereo audio ends in the middle of a frame".into());
    }
    if !transcode {
        return Ok(bytes);
    }
    let header = wav_header(channels as u16, rate, wav.len() - WAV_HEADER_LEN)?;
    wav[..WAV_HEADER_LEN].copy_from_slice(&header);
    Ok(wav)
}

/// The browser decodes audio itself and reports failures (see [`SOUND_DECODE_TIMEOUT`]).
#[cfg(target_arch = "wasm32")]
fn prepare_sound(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
const WAV_HEADER_LEN: usize = 44;

/// Header of a 16-bit PCM WAV file (`channels` 1 or 2) with `data_len` bytes of interleaved
/// samples after it. Fails when a size does not fit the format's 32-bit fields.
#[cfg(not(target_arch = "wasm32"))]
fn wav_header(channels: u16, rate: u32, data_len: usize) -> Result<[u8; WAV_HEADER_LEN], String> {
    let block_align = channels * 2;
    let byte_rate = rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| format!("audio sample rate {rate} is too high"))?;
    let data_len = u32::try_from(data_len)
        .ok()
        .filter(|n| n.checked_add(WAV_HEADER_LEN as u32).is_some())
        .ok_or("audio file is too long")?;
    let fields: [&[u8]; 13] = [
        b"RIFF",
        &(data_len + 36).to_le_bytes(),
        b"WAVE",
        b"fmt ",
        &16u32.to_le_bytes(), // fmt chunk size
        &1u16.to_le_bytes(),  // PCM
        &channels.to_le_bytes(),
        &rate.to_le_bytes(),
        &byte_rate.to_le_bytes(),
        &block_align.to_le_bytes(),
        &16u16.to_le_bytes(), // bits per sample
        b"data",
        &data_len.to_le_bytes(),
    ];
    let mut header = [0u8; WAV_HEADER_LEN];
    let mut at = 0;
    for field in fields {
        header[at..at + field.len()].copy_from_slice(field);
        at += field.len();
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_once_on_ready_and_pending_futures() {
        let mut ready: BoxFuture<i32> = Box::pin(async { 7 });
        assert_eq!(poll_once(&mut ready), Some(7));
        let mut pending: BoxFuture<()> = Box::pin(std::future::pending());
        assert_eq!(poll_once(&mut pending), None);
    }

    #[test]
    fn icon_index_parses_with_default_tile_size() {
        let idx: IconIndex = toml::from_str("[icons]\ngold = [0, 0]\nhp = [3, 1]\n").unwrap();
        assert_eq!(idx.tile_size, 16);
        assert_eq!(idx.icons["hp"], [3, 1]);
    }

    /// 16-bit PCM WAV file with a saw wave.
    fn wav(channels: u16, rate: u32, frames: u32) -> Vec<u8> {
        let samples = frames * u32::from(channels);
        let mut b = wav_header(channels, rate, samples as usize * 2)
            .unwrap()
            .to_vec();
        for i in 0..samples {
            b.extend_from_slice(&((i % 64) as i16 * 256).to_le_bytes());
        }
        b
    }

    fn base_pack_file(rel: &str) -> Vec<u8> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
        std::fs::read(root.join(rel)).expect("base pack file")
    }

    /// Channel count, sample rate and samples as the native decoder reads them.
    fn decode(bytes: &[u8]) -> (u32, u32, Vec<f32>) {
        let mut reader = audrey::Reader::new(std::io::Cursor::new(bytes)).unwrap();
        let d = reader.description();
        let samples = reader.samples::<f32>().map(Result::unwrap).collect();
        (d.channel_count(), d.sample_rate(), samples)
    }

    /// Whether quad-snd 0.2.8's native decoder (`mixer::load_samples_from_file`, called by
    /// macroquad's `load_sound_from_bytes`) panics on `bytes`. It unwraps the audrey reader,
    /// asserts one or two channels, unwraps every sample, duplicates mono samples and resamples
    /// to 44.1 kHz by indexing pairs of samples.
    fn quad_snd_panics(bytes: &[u8]) -> bool {
        std::panic::catch_unwind(|| {
            let mut reader = audrey::Reader::new(std::io::Cursor::new(bytes)).unwrap();
            let description = reader.description();
            let channels = description.channel_count();
            assert!(channels == 1 || channels == 2);
            let mut frames: Vec<f32> = Vec::new();
            for sample in reader.samples::<f32>() {
                let sample = sample.unwrap();
                frames.push(sample);
                if channels == 1 {
                    frames.push(sample);
                }
            }
            let rate = description.sample_rate();
            if rate != 44_100 {
                let mut len = ((44_100.0 / rate as f32) * frames.len() as f32) as usize;
                len -= len % 2;
                let mut out = vec![0.0f32; len];
                for (n, pair) in out.chunks_exact_mut(2).enumerate() {
                    let ix = 2 * ((n as f32 / len as f32) * frames.len() as f32) as usize;
                    pair[0] = frames[ix];
                    pair[1] = frames[ix + 1];
                }
            }
        })
        .is_err()
    }

    #[test]
    fn wav_files_are_checked_and_kept() {
        for bytes in [
            base_pack_file("sfx/cursor.wav"),
            wav(1, 22_050, 100),
            wav(2, 44_100, 100),
        ] {
            assert_eq!(prepare_sound(bytes.clone()), Ok(bytes));
        }
    }

    #[test]
    fn ogg_vorbis_is_decoded_once_and_handed_over_losslessly() {
        let ogg = base_pack_file("bgm/victory.ogg");
        let wav = prepare_sound(ogg.clone()).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert!(!quad_snd_panics(&wav));
        let (channels, rate, samples) = decode(&ogg);
        assert_eq!(decode(&wav), (channels, rate, samples));
        // The WAV is decoded as it is.
        assert_eq!(prepare_sound(wav.clone()), Ok(wav));
    }

    #[test]
    fn wav_header_sizes_must_fit() {
        assert!(wav_header(2, 44_100, 8).is_ok());
        assert!(wav_header(2, u32::MAX / 2, 8).is_err());
        assert!(wav_header(1, 44_100, u32::MAX as usize).is_err());
        assert!(wav_header(1, 44_100, u32::MAX as usize - WAV_HEADER_LEN).is_ok());
    }

    #[test]
    fn sounds_the_native_decoder_would_panic_on_are_rejected() {
        let ogg = base_pack_file("bgm/victory.ogg");
        let wave = base_pack_file("sfx/cursor.wav");
        let mut flipped = ogg.clone();
        for b in &mut flipped[ogg.len() / 3..ogg.len() / 3 + 64] {
            *b ^= 0x5a;
        }
        let bad: Vec<(&str, Vec<u8>)> = vec![
            ("not audio", b"this is not an audio file".to_vec()),
            (
                "mp3",
                b"ID3\x04\x00\x00\x00\x00\x00\x00\xff\xfb\x90\x00".to_vec(),
            ),
            ("empty", Vec::new()),
            ("5.1 wav", wav(6, 44_100, 100)),
            ("truncated wav", wave[..wave.len() / 2].to_vec()),
            ("truncated ogg header", ogg[..200].to_vec()),
            ("truncated ogg", ogg[..ogg.len() * 9 / 10].to_vec()),
        ];
        for (what, bytes) in &bad {
            assert!(quad_snd_panics(bytes), "{what} would not crash quad-snd");
            assert!(prepare_sound(bytes.clone()).is_err(), "{what} was accepted");
        }
        // Damaged files may or may not still decode: whatever is accepted must decode.
        let damaged: Vec<(&str, Vec<u8>)> = vec![
            ("ogg cut at 30%", ogg[..ogg.len() * 3 / 10].to_vec()),
            ("ogg with flipped bytes", flipped),
            ("wav cut mid-sample", wave[..wave.len() - 1].to_vec()),
            ("odd stereo wav", {
                let mut w = wav(2, 22_050, 10);
                w.truncate(w.len() - 2);
                w
            }),
        ];
        for (what, bytes) in damaged {
            let panics = quad_snd_panics(&bytes);
            match prepare_sound(bytes) {
                Ok(prepared) => assert!(!quad_snd_panics(&prepared), "{what} was accepted"),
                Err(_) => assert!(panics, "{what} was rejected but decodes"),
            }
        }
    }

    #[test]
    fn hi_res_art_uses_linear_filtering() {
        assert_eq!(
            Media::texture_filter("portraits/liu_bei"),
            FilterMode::Linear
        );
        assert_eq!(Media::texture_filter("bg/palace"), FilterMode::Linear);
        assert_eq!(Media::texture_filter("ui/title"), FilterMode::Linear);
        assert_eq!(
            Media::texture_filter("units/archer_player"),
            FilterMode::Nearest
        );
        assert_eq!(Media::texture_filter("ui/icons"), FilterMode::Nearest);
    }
}
