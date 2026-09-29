//! The **original mode** (native builds only): play with the art converted from the player's own
//! copy of the original game, like OpenRCT2 reading the RCT2 install.
//!
//! The player picks the install folder in the game ([`crate::screens::original`]); the choice is
//! kept in the settings ([`crate::settings::Settings::original_dir`]). At every launch the
//! loading screen first loads the base pack, then converts the install into an original-mode pack
//! **in memory** ([`Conversion`], `hero_import::pack::build_pack` on a worker thread), mounts it
//! next to the base pack ([`crate::platform::memfs`], [`crate::platform::DataRoot::memory_pack`])
//! and loads that layered pack. Nothing is written to disk and the install is only read; without
//! the install there is no original mode (the base pack stays playable). Saves of the original
//! mode use the pack id `original`, apart from the base pack's.
//!
//! `docs/DECISIONS.md` D10 records why the conversion runs at every launch.

use crate::assets::Media;
use crate::audio::Audio;
use crate::platform::memfs;
use hero_import::edition::{identify, Edition, EditionId};
use hero_import::install::InstallDir;
use hero_import::pack::{build_pack_with_progress, MemoryPack, PackOptions, BUILD_STEPS};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Directory name the converted pack is mounted under, next to the base pack.
pub const PACK_DIR: &str = hero_import::pack::PACK_ID;

/// Files whose presence alone marks a folder as a likely install in the folder browser
/// (checked case-insensitively; the full identification reads the files).
const MARKERS: [&str; 3] = [
    hero_import::edition::DISK_ID_FILE,
    "HEXZMAP.R3",
    hero_import::edition::STEAM_LAUNCHER,
];
/// The DOS/V file family (`hero_import::edition::identify`): both files together mark a likely
/// install too, because the Traditional-Chinese rule needs neither of the [`MARKERS`].
const DOS_V_FAMILY: [&str; 2] = ["MAIN.EXE", "SNR0M.R3"];

/// What a folder holds, for the folder browser and the original-data screen.
#[derive(Debug, Clone)]
pub enum FolderCheck {
    /// An edition the original mode can play.
    Supported(Edition),
    /// A recognised edition that cannot be converted yet, or no edition at all
    /// (`EditionId::Unknown`).
    Unsupported(Edition),
    /// The folder cannot be read (missing, not a folder, no permission).
    Unreadable(String),
}

impl FolderCheck {
    pub fn is_supported(&self) -> bool {
        matches!(self, FolderCheck::Supported(_))
    }

    /// One line for the screen, in Korean.
    pub fn summary(&self) -> String {
        match self {
            FolderCheck::Supported(e) => {
                format!("{} — 원작 모드로 플레이할 수 있습니다", edition_label(e.id))
            }
            FolderCheck::Unsupported(e) if e.id == EditionId::Unknown => {
                "이 폴더에서 원작 파일을 찾지 못했습니다".to_string()
            }
            FolderCheck::Unsupported(e) => {
                format!(
                    "{} — 아직 원작 모드를 지원하지 않는 판본입니다",
                    edition_label(e.id)
                )
            }
            FolderCheck::Unreadable(why) => format!("폴더를 읽을 수 없습니다: {why}"),
        }
    }

    /// The identification's evidence (as `hero-tools original probe` prints it), for the
    /// details.
    pub fn evidence(&self) -> &[String] {
        match self {
            FolderCheck::Supported(e) | FolderCheck::Unsupported(e) => &e.evidence,
            FolderCheck::Unreadable(_) => &[],
        }
    }
}

/// Korean name of an edition.
pub fn edition_label(id: EditionId) -> &'static str {
    match id {
        EditionId::KoreanDos => "한국어 DOS/V판",
        EditionId::ChineseDos => "중국어(번체) DOS판",
        EditionId::Steam2017 => "Steam 2017판",
        EditionId::Pc98Images => "PC-98 디스크 이미지",
        EditionId::Unknown => "알 수 없는 판본",
    }
}

/// Identify the edition in `dir` (reads a few files; the folder is only read).
pub fn check_folder(dir: &Path) -> FolderCheck {
    match InstallDir::open(dir) {
        Ok(install) => {
            let edition = identify(&install);
            if edition.id.is_extractable() {
                FolderCheck::Supported(edition)
            } else {
                FolderCheck::Unsupported(edition)
            }
        }
        Err(e) => FolderCheck::Unreadable(e.to_string()),
    }
}

/// Quick hint for the folder browser: `dir` directly holds a file only an install has, or the
/// DOS/V file family. Reads the folder listing only. Every folder that the identification can
/// find a playable edition in passes this test (the Korean rules need `DISK1.R3I` or the family,
/// the Chinese rule the family), so the browser runs the full identification — which also reads
/// the head of every file, too slow for every folder browsed — only on folders that pass.
pub fn looks_like_install(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut family = [false; DOS_V_FAMILY.len()];
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if MARKERS.iter().any(|m| name.eq_ignore_ascii_case(m)) {
            return true;
        }
        for (seen, m) in family.iter_mut().zip(DOS_V_FAMILY) {
            *seen |= name.eq_ignore_ascii_case(m);
        }
    }
    family.iter().all(|&seen| seen)
}

/// The folder as it is stored in the settings: `None` when its path is not valid Unicode (the
/// settings are JSON text, so such a path could not be saved without changing it).
pub fn storable_path(dir: &Path) -> Option<String> {
    dir.to_str().map(str::to_string)
}

type Outcome = Result<MemoryPack, String>;

/// A conversion running on a worker thread, so the loading screen keeps drawing.
pub struct Conversion {
    install: PathBuf,
    running: Option<JoinHandle<Outcome>>,
    /// The result when it was produced without a worker thread.
    done: Option<Outcome>,
    /// Finished steps of the worker (of `BUILD_STEPS`).
    steps: Arc<AtomicUsize>,
}

fn convert(install: &Path, options: &PackOptions, steps: &AtomicUsize) -> Outcome {
    build_pack_with_progress(install, options, &mut |n| steps.store(n, Ordering::Relaxed))
        .map_err(|e| e.to_string())
}

impl Conversion {
    /// Start converting `install` with `options` (built from the base pack, see
    /// `PackOptions::for_pack`).
    pub fn start(install: PathBuf, options: PackOptions) -> Conversion {
        let source = install.clone();
        let steps = Arc::new(AtomicUsize::new(0));
        let worker_steps = Arc::clone(&steps);
        let spawned = std::thread::Builder::new()
            .name("original-mode".into())
            .spawn(move || convert(&source, &options, &worker_steps));
        match spawned {
            Ok(handle) => Conversion {
                install,
                running: Some(handle),
                done: None,
                steps,
            },
            Err(e) => Conversion {
                done: Some(Err(format!("cannot start the conversion: {e}"))),
                install,
                running: None,
                steps,
            },
        }
    }

    /// How far the conversion is, 0..=1 (by finished steps).
    pub fn fraction(&self) -> f32 {
        self.steps.load(Ordering::Relaxed).min(BUILD_STEPS) as f32 / BUILD_STEPS as f32
    }

    /// The install being converted.
    pub fn install(&self) -> &Path {
        &self.install
    }

    /// The result once the conversion has finished (returned once).
    pub fn poll(&mut self) -> Option<Outcome> {
        if let Some(done) = self.done.take() {
            return Some(done);
        }
        let handle = self.running.take()?;
        if !handle.is_finished() {
            self.running = Some(handle);
            return None;
        }
        Some(match handle.join() {
            Ok(result) => result,
            Err(panic) => Err(format!(
                "the conversion crashed: {}",
                panic_message(&*panic)
            )),
        })
    }
}

/// The message a worker thread panicked with.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("unknown panic")
}

// ----- music -------------------------------------------------------------------------------

/// A finished song: its key and its `bgm/<key>.wav` file, or why it could not be made (an empty
/// key: the music could not be rendered at all).
type Song = (String, Result<Vec<u8>, String>);

/// The original's songs, rendered on a worker thread while the game runs: the conversion at
/// launch leaves them out because rendering takes 10–20 seconds (`PackOptions::music`,
/// `docs/DECISIONS.md` D16). Each finished song is added to the mounted pack as
/// `bgm/<key>.wav`, where it stands in for the base pack's music from then on; music that is
/// playing when its song arrives starts again with it.
pub struct MusicRender {
    songs: Receiver<Song>,
    /// Songs added whose cached sound could not be dropped yet (it was loading).
    stale: Vec<String>,
    added: usize,
    finished: bool,
}

impl MusicRender {
    /// Start rendering the songs of the install at `install`.
    pub fn start(install: PathBuf) -> MusicRender {
        let (send, songs) = mpsc::channel::<Song>();
        let failed = send.clone();
        let spawned = std::thread::Builder::new()
            .name("original-music".into())
            .spawn(move || {
                // Stops when the game no longer listens (the data pack was reloaded).
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    hero_import::pack::render_music(&install, &mut |key, wav| {
                        send.send((key.to_string(), wav)).is_ok()
                    })
                }))
                .unwrap_or_else(|panic| {
                    Err(format!("rendering crashed: {}", panic_message(&*panic)))
                });
                if let Err(e) = result {
                    let _ = send.send((String::new(), Err(e)));
                }
            });
        if let Err(e) = spawned {
            let _ = failed.send((String::new(), Err(format!("cannot start rendering: {e}"))));
        }
        MusicRender {
            songs,
            stale: Vec::new(),
            added: 0,
            finished: false,
        }
    }

    /// Add the songs finished since the last call to the mounted pack. `true` once every song
    /// has been handled.
    pub fn poll(&mut self, media: &Media, audio: &mut Audio) -> bool {
        loop {
            match self.songs.try_recv() {
                Ok((key, Ok(wav))) => {
                    if memfs::insert(&format!("bgm/{key}.wav"), wav) {
                        audio.reload_bgm(&key);
                        self.stale.push(key);
                        self.added += 1;
                    } else {
                        macroquad::logging::warn!(
                            "original mode: music {}: no pack mounted to add it to",
                            key
                        );
                    }
                }
                Ok((key, Err(e))) if key.is_empty() => {
                    macroquad::logging::warn!("original mode: music: {}", e);
                }
                Ok((key, Err(e))) => {
                    macroquad::logging::warn!("original mode: music {}: {}", key, e);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.finished {
                        macroquad::logging::info!(
                            "original mode: {} of the original's songs added",
                            self.added
                        );
                    }
                    self.finished = true;
                    break;
                }
            }
        }
        self.stale
            .retain(|key| !media.forget_sound(&format!("bgm/{key}")));
        self.finished && self.stale.is_empty()
    }
}

// ----- background reads ----------------------------------------------------------------------

/// A folder read on a worker thread. Listing, identifying or even testing a path can stall for
/// as long as the file system does (an unresponsive network share, `\\server\share`, only
/// gives up after its timeout), so the folder browser reads through this, keeps drawing and
/// lets the player cancel. A cancelled read is dropped: its thread ends on its own once the
/// file system answers, and its result is thrown away. (So each read cancelled on a stalled
/// share leaves one thread waiting out the share's timeout; the player has to cancel again
/// and again for that to add up.) The errors are the player-facing messages.
pub struct Background<T> {
    running: Option<JoinHandle<T>>,
    /// The error when the thread could not be started.
    failed: Option<String>,
}

impl<T: Send + 'static> Background<T> {
    pub fn spawn(work: impl FnOnce() -> T + Send + 'static) -> Background<T> {
        match std::thread::Builder::new()
            .name("background-read".into())
            .spawn(work)
        {
            Ok(handle) => Background {
                running: Some(handle),
                failed: None,
            },
            Err(e) => Background {
                running: None,
                failed: Some(format!("작업 스레드를 시작하지 못했습니다: {e}")),
            },
        }
    }

    /// The result once the read has finished (returned once); `Err` when the worker could not
    /// start or panicked.
    pub fn poll(&mut self) -> Option<Result<T, String>> {
        if let Some(e) = self.failed.take() {
            return Some(Err(e));
        }
        let handle = self.running.take()?;
        if !handle.is_finished() {
            self.running = Some(handle);
            return None;
        }
        Some(
            handle
                .join()
                .map_err(|panic| format!("읽다가 오류가 났습니다: {}", panic_message(&*panic))),
        )
    }
}

/// What the folder browser shows at a place, read by [`look`].
#[derive(Debug, Clone)]
pub struct Listing {
    pub place: Place,
    /// The folders, or why the place cannot be read.
    pub folders: Result<Vec<Folder>, String>,
    /// The place's own edition, when it looks like an install.
    pub check: Option<FolderCheck>,
    /// Not an install itself, but the one subfolder that is one the original mode can play
    /// (a DOSBox package's `GAME`).
    pub child: Option<Folder>,
}

/// Read everything the browser shows at `place` (blocking: run it through [`Background`]).
pub fn look(place: Place) -> Listing {
    let folders = list(&place);
    let check = match &place {
        Place::Dir(dir) if looks_like_install(dir) => Some(check_folder(dir)),
        _ => None,
    };
    let child = match (&place, &check, &folders) {
        (Place::Dir(_), None, Ok(folders)) => sole_install(folders)
            .filter(|f| check_folder(&f.path).is_supported())
            .cloned(),
        _ => None,
    };
    Listing {
        place,
        folders,
        check,
        child,
    }
}

// ----- folder browser ------------------------------------------------------------------------

/// Most subfolders listed in one folder.
pub const MAX_LISTED: usize = 1000;
/// Most subfolders checked for [`looks_like_install`] in one folder (each check lists a folder).
pub const MAX_CHECKED: usize = 300;

/// Whether every folder of a [`list`] was checked for an install (the ★ marks are complete).
pub fn all_checked(folders: &[Folder]) -> bool {
    folders.len() <= MAX_CHECKED
}

/// A place in the folder browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// The drives (Windows) or the file system root.
    Roots,
    Dir(PathBuf),
}

/// A folder listed in the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
    /// [`looks_like_install`] (not checked beyond the first `MAX_CHECKED` folders).
    pub install: bool,
}

/// The drives that exist (Windows, `C:` to `Z:`: floppy drives are skipped because probing them
/// can stall) or `/`.
pub fn roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        ('C'..='Z')
            .map(|c| PathBuf::from(format!("{c}:\\")))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        vec![PathBuf::from("/")]
    }
}

/// The place above `place`: the parent folder, the drive list above a Windows drive, nothing
/// above the roots (or `/`).
pub fn parent(place: &Place) -> Option<Place> {
    match place {
        Place::Roots => None,
        Place::Dir(dir) => match dir.parent() {
            Some(p) if !p.as_os_str().is_empty() => Some(Place::Dir(p.to_path_buf())),
            _ if cfg!(windows) => Some(Place::Roots),
            _ => None,
        },
    }
}

/// The folders shown at `place`, sorted by name (case-insensitive). Hidden folders (`.name`,
/// `$name`) are left out.
pub fn list(place: &Place) -> Result<Vec<Folder>, String> {
    let dir = match place {
        Place::Roots => {
            return Ok(roots()
                .into_iter()
                .map(|p| Folder {
                    name: p.display().to_string(),
                    install: false,
                    path: p,
                })
                .collect())
        }
        Place::Dir(dir) => dir,
    };
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut folders: Vec<Folder> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.') && !name.starts_with('$')).then(|| Folder {
                name,
                path: e.path(),
                install: false,
            })
        })
        .collect();
    folders.sort_by_cached_key(|f| (f.name.to_lowercase(), f.name.clone()));
    folders.truncate(MAX_LISTED);
    for f in folders.iter_mut().take(MAX_CHECKED) {
        f.install = looks_like_install(&f.path);
    }
    Ok(folders)
}

/// The only listed folder that looks like an install: offered when the folder shown is not one
/// itself, e.g. a DOSBox package's `res/hero` holding the game in `GAME`.
///
/// Only a listing whose folders were all checked ([`list`] checks the first `MAX_CHECKED`) can
/// tell that one is the only install, so a longer one offers none.
pub fn sole_install(folders: &[Folder]) -> Option<&Folder> {
    if !all_checked(folders) {
        return None;
    }
    let mut installs = folders.iter().filter(|f| f.install);
    let first = installs.next()?;
    installs.next().is_none().then_some(first)
}

/// Most characters of a typed or pasted path.
pub const MAX_TYPED: usize = 1024;

/// The folder a typed or pasted path names, as an absolute path (it is stored in the settings).
/// Surrounding blanks and quotes (Explorer's "Copy as path" adds them) are dropped, a relative
/// path is taken from `base` (the folder shown), a bare Windows drive (`D:`) is its root, and a
/// file's path (a pasted `MAIN.EXE`) names its folder.
pub fn typed_folder(text: &str, base: Option<&Path>) -> Result<PathBuf, String> {
    let mut text = text.trim();
    for q in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(q).and_then(|t| t.strip_suffix(q)) {
            text = inner.trim();
        }
    }
    if text.is_empty() {
        return Err("경로를 입력해 주세요".to_string());
    }
    let bare_drive = cfg!(windows)
        && text.len() == 2
        && text.as_bytes()[0].is_ascii_alphabetic()
        && text.as_bytes()[1] == b':';
    let mut path = if bare_drive {
        PathBuf::from(format!("{text}\\"))
    } else {
        PathBuf::from(text)
    };
    if path.is_relative() {
        if let Some(base) = base {
            path = base.join(path);
        }
    }
    // Still relative without a base (or drive-relative, `D:game`): resolved against the current
    // folder, so what is stored does not depend on where the game is started next time.
    let path = std::path::absolute(&path).map_err(|e| format!("{text}: {e}"))?;
    if path.is_dir() {
        return Ok(path);
    }
    match path.parent() {
        Some(p) if path.is_file() && !p.as_os_str().is_empty() => Ok(p.to_path_buf()),
        _ => Err(format!("폴더를 찾을 수 없습니다: {text}")),
    }
}

/// Where the browser opens: the saved folder, else its nearest existing parent, else the home
/// folder, else the current folder, else the roots.
pub fn start_place(saved: Option<&str>) -> Place {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    start_place_from(
        saved.map(PathBuf::from),
        home.map(PathBuf::from),
        std::env::current_dir().ok(),
    )
}

fn start_place_from(saved: Option<PathBuf>, home: Option<PathBuf>, cwd: Option<PathBuf>) -> Place {
    if let Some(mut dir) = saved {
        loop {
            if dir.is_dir() {
                return Place::Dir(dir);
            }
            match dir.parent() {
                Some(p) if !p.as_os_str().is_empty() => dir = p.to_path_buf(),
                _ => break,
            }
        }
    }
    home.into_iter()
        .chain(cwd)
        .find(|d| d.is_dir())
        .map_or(Place::Roots, Place::Dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::memfs::{self, Lookup};
    use crate::platform::DataRoot;
    use hero_core::pack::{DirSource, FileSource, Pack, PackError, Severity};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir()
                .join(format!("hero-game-original-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Poll a background read until it finishes.
    fn wait<T: Send + 'static>(mut job: Background<T>) -> Result<T, String> {
        loop {
            if let Some(done) = job.poll() {
                assert!(job.poll().is_none(), "a result is returned once");
                return done;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn background_reads_report_their_result_or_crash() {
        assert_eq!(wait(Background::spawn(|| 7)), Ok(7));
        let crashed: Result<(), String> = wait(Background::spawn(|| panic!("disk on fire")));
        assert_eq!(
            crashed,
            Err("읽다가 오류가 났습니다: disk on fire".to_string())
        );
    }

    #[test]
    fn a_look_reads_what_the_browser_shows() {
        let tmp = TempDir::new("look");
        std::fs::create_dir_all(tmp.0.join("game")).unwrap();
        std::fs::create_dir_all(tmp.0.join("other")).unwrap();
        std::fs::write(tmp.0.join("game/disk1.r3i"), b"x").unwrap();
        let listing = wait(Background::spawn({
            let dir = tmp.0.clone();
            move || look(Place::Dir(dir))
        }))
        .unwrap();
        assert_eq!(listing.place, Place::Dir(tmp.0.clone()));
        let names: Vec<_> = listing
            .folders
            .as_ref()
            .unwrap()
            .iter()
            .map(|f| (f.name.as_str(), f.install))
            .collect();
        assert_eq!(names, [("game", true), ("other", false)]);
        // Not an install itself; the one install-looking subfolder is not a playable edition,
        // so it is not offered.
        assert!(listing.check.is_none());
        assert!(listing.child.is_none());
        // Inside it: its own (unplayable) edition, nothing listed.
        let inner = look(Place::Dir(tmp.0.join("game")));
        assert!(matches!(inner.check, Some(FolderCheck::Unsupported(_))));
        assert_eq!(inner.folders.as_deref(), Ok(&[][..]));
        // A folder that is gone: the error, no check.
        let gone = look(Place::Dir(tmp.0.join("missing")));
        assert!(gone.folders.is_err() && gone.check.is_none() && gone.child.is_none());
    }

    #[test]
    fn lists_folders_and_marks_installs() {
        let tmp = TempDir::new("list");
        for d in ["b", "A", ".hidden", "$RECYCLE.BIN", "game", "zh", "zh_half"] {
            std::fs::create_dir_all(tmp.0.join(d)).unwrap();
        }
        std::fs::write(tmp.0.join("file.txt"), b"x").unwrap();
        std::fs::write(tmp.0.join("game/disk1.r3i"), b"x").unwrap();
        // A Traditional-Chinese copy is identified from MAIN.EXE + SNR0M.R3 alone.
        std::fs::write(tmp.0.join("zh/Main.exe"), b"x").unwrap();
        std::fs::write(tmp.0.join("zh/snr0m.r3"), b"x").unwrap();
        std::fs::write(tmp.0.join("zh_half/MAIN.EXE"), b"x").unwrap();
        let folders = list(&Place::Dir(tmp.0.clone())).unwrap();
        let names: Vec<_> = folders
            .iter()
            .map(|f| (f.name.as_str(), f.install))
            .collect();
        assert_eq!(
            names,
            [
                ("A", false),
                ("b", false),
                ("game", true),
                ("zh", true),
                ("zh_half", false)
            ]
        );
        assert!(looks_like_install(&tmp.0.join("game")));
        assert!(!looks_like_install(&tmp.0));
        assert!(list(&Place::Dir(tmp.0.join("missing"))).is_err());
        assert_eq!(
            storable_path(&tmp.0.join("game")).as_deref(),
            tmp.0.join("game").to_str()
        );

        // A folder that is not an install cannot be used.
        assert!(matches!(
            check_folder(&tmp.0.join("game")),
            FolderCheck::Unsupported(_)
        ));
        assert!(matches!(
            check_folder(&tmp.0.join("missing")),
            FolderCheck::Unreadable(_)
        ));

        // Two install-looking folders: none is offered; one: that one.
        assert_eq!(sole_install(&folders), None);
        let one: Vec<Folder> = folders.into_iter().filter(|f| f.name != "zh").collect();
        assert_eq!(sole_install(&one).map(|f| f.name.as_str()), Some("game"));
        assert_eq!(sole_install(&[]), None);
        // Beyond the checked folders an install could hide unmarked: no offer.
        let mut many: Vec<Folder> = (0..=MAX_CHECKED)
            .map(|i| Folder {
                name: format!("f{i:03}"),
                path: PathBuf::from(format!("f{i:03}")),
                install: false,
            })
            .collect();
        many[0].install = true;
        assert_eq!(sole_install(&many), None);
        many.pop();
        assert_eq!(sole_install(&many).map(|f| f.name.as_str()), Some("f000"));

        // Typed paths: blanks and quotes dropped, a file names its folder, relative paths are
        // taken from the folder shown; the result is absolute.
        let root = std::path::absolute(&tmp.0).unwrap();
        let game = root.join("game");
        let shown = game.display().to_string();
        assert_eq!(typed_folder(&shown, None), Ok(game.clone()));
        assert_eq!(
            typed_folder(&format!("  \"{shown}\" "), None),
            Ok(game.clone())
        );
        assert_eq!(typed_folder(&format!("'{shown}'"), None), Ok(game.clone()));
        assert_eq!(
            typed_folder(&game.join("disk1.r3i").display().to_string(), None),
            Ok(game.clone())
        );
        let relative = typed_folder("game", Some(&root)).unwrap();
        assert!(relative.is_absolute(), "{relative:?}");
        assert!(
            relative.ends_with("game") && relative.is_dir(),
            "{relative:?}"
        );
        let dotted = typed_folder("./game/", Some(&root)).unwrap();
        assert!(dotted.is_absolute() && dotted.is_dir(), "{dotted:?}");
        assert!(typed_folder("game", Some(&root.join("b"))).is_err());
        assert!(typed_folder("", None).is_err());
        assert!(typed_folder(" \"\" ", None).is_err());
        assert!(typed_folder(&root.join("missing").display().to_string(), None).is_err());
        if cfg!(windows) {
            // A bare drive is its root, not the drive's current folder.
            let drive = &shown[..2];
            assert_eq!(
                typed_folder(drive, Some(&root)),
                Ok(PathBuf::from(format!("{drive}\\")))
            );
        }
    }

    #[test]
    fn parents_and_start_places() {
        let tmp = TempDir::new("start");
        let game = tmp.0.join("game");
        std::fs::create_dir_all(&game).unwrap();
        assert_eq!(
            parent(&Place::Dir(game.clone())),
            Some(Place::Dir(tmp.0.clone()))
        );
        assert_eq!(parent(&Place::Roots), None);
        let top = roots().into_iter().next().unwrap();
        let above_top = parent(&Place::Dir(top));
        if cfg!(windows) {
            assert_eq!(above_top, Some(Place::Roots));
        } else {
            assert_eq!(above_top, None);
        }

        // The saved folder, or its nearest existing parent once it is gone.
        assert_eq!(
            start_place_from(Some(game.clone()), None, None),
            Place::Dir(game.clone())
        );
        assert_eq!(
            start_place_from(Some(game.join("moved/away")), None, None),
            Place::Dir(game.clone())
        );
        assert_eq!(
            start_place_from(None, Some(tmp.0.join("nohome")), Some(game.clone())),
            Place::Dir(game)
        );
        assert_eq!(start_place_from(None, None, None), Place::Roots);
    }

    /// Pack text files read the way the loading screen reads them: by their path relative to
    /// the top pack, through the data root and the mount.
    struct MountSource(DataRoot);

    impl FileSource for MountSource {
        fn read_text(&self, path: &str) -> Result<String, PackError> {
            let missing = || PackError::Missing { file: path.into() };
            let bytes = match memfs::lookup(&self.0.top_pack().path(path)) {
                Lookup::Memory(bytes) => bytes.ok_or_else(missing)?,
                Lookup::Disk(p) => std::fs::read(p).map_err(|_| missing())?,
            };
            String::from_utf8(bytes).map_err(|e| PackError::Parse {
                file: path.into(),
                msg: e.to_string(),
            })
        }
    }

    /// The whole original-mode path on the player's own copy (set `EIKETSU_ORIGINAL_DIR` to the
    /// install; skipped otherwise): convert on the worker thread, mount next to the base pack,
    /// load and validate the layered pack through the mount.
    #[test]
    fn golden_original_mode_plays_from_memory() {
        let Some(install) = std::env::var_os("EIKETSU_ORIGINAL_DIR") else {
            eprintln!("EIKETSU_ORIGINAL_DIR not set: skipped");
            return;
        };
        assert!(check_folder(Path::new(&install)).is_supported());
        // Its folder in the browser: the install is marked, and above it (a DOSBox package's
        // `GAME`) offered when it is the only one.
        let install_dir = PathBuf::from(&install);
        let at = look(Place::Dir(install_dir.clone()));
        assert!(matches!(at.check, Some(FolderCheck::Supported(_))));
        if let Some(parent) = install_dir.parent() {
            let above = look(Place::Dir(parent.to_path_buf()));
            let installs = above
                .folders
                .as_ref()
                .unwrap()
                .iter()
                .filter(|f| f.install)
                .count();
            if installs == 1 && above.check.is_none() {
                assert_eq!(above.child.map(|f| f.path), Some(install_dir.clone()));
            }
        }
        let base_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base");
        let base = Pack::load(&DirSource {
            root: base_dir.clone(),
        })
        .unwrap();
        let (root, extends) = DataRoot::from_dir(&base_dir, &[])
            .memory_pack(PACK_DIR, "원작")
            .unwrap();
        let started = std::time::Instant::now();
        let mut job =
            Conversion::start(install.into(), PackOptions::for_pack(&base, extends, None));
        let built = loop {
            if let Some(result) = job.poll() {
                break result.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(job.fraction(), 1.0, "every step reported");
        eprintln!("converted in {:?}", started.elapsed());
        assert!(built.index.success(), "{:#?}", built.index.assets);

        memfs::mount(root.top_dir(), built.files);
        let pack = Pack::load(&MountSource(root.clone())).unwrap();
        assert_eq!(pack.manifest.id, PACK_DIR);
        assert_eq!(pack.layers.len(), 2);
        assert_eq!(pack.maps.len(), 58);
        // The prologue's first battle is played on its original map.
        assert_eq!(
            pack.battles["p1_sishui"].map.use_map.as_deref(),
            Some("hexz_00")
        );
        let errors: Vec<_> = pack
            .validate()
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");

        // Media through the chain: converted pictures from memory, the rest from the base pack.
        let parents: Vec<String> = pack.layers[1..].iter().map(|l| l.dir.clone()).collect();
        let media = root.with_parent_packs(parents);
        assert!(memfs::is_file(
            &media.media_paths("gfx/maps/hexz_00.png")[0]
        ));
        let title = media.media_paths("gfx/ui/title.png");
        assert!(!memfs::is_file(&title[0]));
        assert!(memfs::is_file(&title[1]));
        memfs::unmount();
    }
}
