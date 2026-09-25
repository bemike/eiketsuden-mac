//! Platform layer: everything that differs between the native build and the browser build.
//!
//! * [`LaunchOptions`] — command line (`--data <dir>`, `--gallery`) natively, the URL hash
//!   (`#gallery`) on the web.
//! * [`DataRoot`] — where the data pack lives. Natively it is resolved from `--data`, the
//!   `EIKETSUDEN_DATA` environment variable, `<exe dir>/data/base` and `./data/base` (first match
//!   wins); on the web it is the relative URL `data/base/` next to `index.html`. All pack files are
//!   read through [`DataRoot::path`] + `macroquad::file::load_file`, which is a file read natively
//!   and an HTTP fetch on the web.
//! * [`unix_now`] — wall clock time for save timestamps.
//! * [`storage`] — key/value persistence for saves and settings (files natively,
//!   `localStorage` on the web).

pub mod storage;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

use std::path::{Path, PathBuf};

/// Environment variable that points at the data pack directory (native builds).
pub const DATA_ENV: &str = "EIKETSUDEN_DATA";

/// Options chosen when the game was launched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// Explicit data pack directory (`--data <dir>`), native only.
    pub data_dir: Option<PathBuf>,
    /// Start the UI gallery dev screen instead of the game (`--gallery` / `#gallery`).
    pub gallery: bool,
    /// Problems found while parsing the options (unknown flags, ...). Logged at startup.
    pub warnings: Vec<String>,
}

impl LaunchOptions {
    /// Options of this process: `std::env::args` natively, `window.location.hash` on the web.
    pub fn from_environment() -> LaunchOptions {
        #[cfg(not(target_arch = "wasm32"))]
        {
            LaunchOptions::parse_args(std::env::args().skip(1))
        }
        #[cfg(target_arch = "wasm32")]
        {
            LaunchOptions::parse_hash(&web::location_hash())
        }
    }

    /// Parse native command line arguments (without the program name).
    pub fn parse_args(args: impl IntoIterator<Item = String>) -> LaunchOptions {
        let mut opts = LaunchOptions::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--gallery" => opts.gallery = true,
                "--data" => match args.next() {
                    Some(dir) => opts.data_dir = Some(PathBuf::from(dir)),
                    None => opts.warnings.push("--data needs a directory argument".into()),
                },
                other => {
                    if let Some(dir) = other.strip_prefix("--data=") {
                        opts.data_dir = Some(PathBuf::from(dir));
                    } else {
                        opts.warnings.push(format!("unknown argument `{other}` ignored"));
                    }
                }
            }
        }
        opts
    }

    /// Parse the URL fragment of the web build, e.g. `#gallery`.
    pub fn parse_hash(hash: &str) -> LaunchOptions {
        let mut opts = LaunchOptions::default();
        for word in hash.trim_start_matches('#').split(['&', ',']) {
            match word {
                "" => {}
                "gallery" => opts.gallery = true,
                other => opts.warnings.push(format!("unknown URL option `{other}` ignored")),
            }
        }
        opts
    }
}

/// Location of the data pack. Every pack-relative path goes through [`DataRoot::path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    /// Prefix joined with pack-relative paths; empty or ending with `/`.
    prefix: String,
    /// Human readable location for error messages.
    display: String,
    /// Every location that was considered, in order (for the "pack not found" error screen).
    candidates: Vec<String>,
}

impl DataRoot {
    /// Resolve the data pack location for this platform.
    pub fn resolve(opts: &LaunchOptions) -> DataRoot {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = opts;
            DataRoot::from_prefix("data/base/")
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let env = std::env::var_os(DATA_ENV).map(PathBuf::from);
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf));
            let (dir, candidates) = resolve_data_dir(
                opts.data_dir.as_deref(),
                env.as_deref(),
                exe_dir.as_deref(),
                |p| p.join("pack.toml").is_file(),
            );
            DataRoot::from_dir(&dir, &candidates)
        }
    }

    /// A root given as a URL/path prefix (used by the web build and tests).
    pub fn from_prefix(prefix: &str) -> DataRoot {
        let mut prefix = prefix.to_string();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        DataRoot {
            display: prefix.clone(),
            candidates: vec![prefix.clone()],
            prefix,
        }
    }

    /// A root on the local file system. Pack-relative paths are appended with `/`, which every
    /// supported OS (Windows included) accepts as a separator.
    pub fn from_dir(dir: &Path, candidates: &[PathBuf]) -> DataRoot {
        let mut prefix = dir.to_string_lossy().into_owned();
        if !prefix.is_empty() && !prefix.ends_with('/') && !prefix.ends_with('\\') {
            prefix.push('/');
        }
        DataRoot {
            prefix,
            display: dir.display().to_string(),
            candidates: candidates.iter().map(|p| p.display().to_string()).collect(),
        }
    }

    /// Path or URL of a pack-relative file (`fonts/Galmuri11.ttf`), for `load_file`.
    pub fn path(&self, rel: &str) -> String {
        format!("{}{}", self.prefix, rel.trim_start_matches('/'))
    }

    /// Where the pack is, for messages.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// Every location that was considered, first match first.
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }
}

/// Pick the data pack directory. Explicit choices (`--data`, then the environment variable) are
/// used even when they do not contain a pack so the error screen can name them; otherwise the
/// first implicit location containing `pack.toml` wins, falling back to `./data/base`.
/// Returns the chosen directory and every candidate that was considered.
pub fn resolve_data_dir(
    arg: Option<&Path>,
    env: Option<&Path>,
    exe_dir: Option<&Path>,
    has_pack: impl Fn(&Path) -> bool,
) -> (PathBuf, Vec<PathBuf>) {
    if let Some(dir) = arg {
        return (dir.to_path_buf(), vec![dir.to_path_buf()]);
    }
    if let Some(dir) = env.filter(|d| !d.as_os_str().is_empty()) {
        return (dir.to_path_buf(), vec![dir.to_path_buf()]);
    }
    let mut candidates = Vec::new();
    if let Some(exe) = exe_dir {
        candidates.push(exe.join("data").join("base"));
    }
    candidates.push(PathBuf::from("data").join("base"));
    let chosen = candidates
        .iter()
        .find(|c| has_pack(c))
        .cloned()
        .unwrap_or_else(|| PathBuf::from("data").join("base"));
    (chosen, candidates)
}

/// Seconds since the Unix epoch (0 if the clock is unavailable or before 1970).
pub fn unix_now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let secs = web::now_seconds();
        if secs.is_finite() && secs > 0.0 {
            secs as u64
        } else {
            0
        }
    }
}

/// `true` in the browser build.
pub const fn is_web() -> bool {
    cfg!(target_arch = "wasm32")
}

/// Whether the game may offer a "quit" command (browsers cannot close their tab).
pub const fn can_quit() -> bool {
    !is_web()
}

/// Whether a fullscreen toggle is offered. Browsers only allow fullscreen from inside an input
/// event handler, which the game loop is not, so the web build leaves it to the browser.
pub const fn can_toggle_fullscreen() -> bool {
    !is_web()
}

/// Tell the host page that the first frame is on screen (removes the HTML loading overlay).
/// No-op natively.
pub fn notify_ready() {
    #[cfg(target_arch = "wasm32")]
    web::ready();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_native_arguments() {
        let o = LaunchOptions::parse_args(args(&["--gallery", "--data", "packs/base"]));
        assert!(o.gallery);
        assert_eq!(o.data_dir, Some(PathBuf::from("packs/base")));
        assert!(o.warnings.is_empty());

        let o = LaunchOptions::parse_args(args(&["--data=/x/y", "--bogus"]));
        assert_eq!(o.data_dir, Some(PathBuf::from("/x/y")));
        assert!(!o.gallery);
        assert_eq!(o.warnings.len(), 1);

        let o = LaunchOptions::parse_args(args(&["--data"]));
        assert_eq!(o.data_dir, None);
        assert_eq!(o.warnings.len(), 1);
    }

    #[test]
    fn parses_url_hash() {
        assert!(LaunchOptions::parse_hash("#gallery").gallery);
        assert!(LaunchOptions::parse_hash("gallery").gallery);
        assert!(!LaunchOptions::parse_hash("").gallery);
        let o = LaunchOptions::parse_hash("#foo&gallery");
        assert!(o.gallery);
        assert_eq!(o.warnings.len(), 1);
    }

    #[test]
    fn explicit_data_dir_wins_even_without_pack() {
        let (dir, cands) = resolve_data_dir(
            Some(Path::new("/mods/x")),
            Some(Path::new("/env/y")),
            Some(Path::new("/opt/game")),
            |_| false,
        );
        assert_eq!(dir, PathBuf::from("/mods/x"));
        assert_eq!(cands, vec![PathBuf::from("/mods/x")]);

        let (dir, _) = resolve_data_dir(None, Some(Path::new("/env/y")), None, |_| false);
        assert_eq!(dir, PathBuf::from("/env/y"));
    }

    #[test]
    fn implicit_data_dir_prefers_exe_dir_then_cwd() {
        let exe = Path::new("/opt/game");
        let (dir, cands) = resolve_data_dir(None, None, Some(exe), |p| p.starts_with("/opt"));
        assert_eq!(dir, exe.join("data").join("base"));
        assert_eq!(cands.len(), 2);

        let (dir, _) = resolve_data_dir(None, Some(Path::new("")), Some(exe), |p| {
            p == Path::new("data/base")
        });
        assert_eq!(dir, PathBuf::from("data").join("base"));

        // Nothing found: fall back to ./data/base so the error screen has a concrete path.
        let (dir, cands) = resolve_data_dir(None, None, Some(exe), |_| false);
        assert_eq!(dir, PathBuf::from("data").join("base"));
        assert_eq!(cands.len(), 2);
    }

    #[test]
    fn data_root_paths() {
        let web = DataRoot::from_prefix("data/base");
        assert_eq!(web.path("pack.toml"), "data/base/pack.toml");
        assert_eq!(web.path("/fonts/a.ttf"), "data/base/fonts/a.ttf");

        let native = DataRoot::from_dir(Path::new("/games/hero/data/base"), &[]);
        assert_eq!(native.path("rules/game.toml"), "/games/hero/data/base/rules/game.toml");
        let trailing = DataRoot::from_dir(Path::new("base/"), &[]);
        assert_eq!(trailing.path("pack.toml"), "base/pack.toml");
    }
}
