//! A data pack held in memory: the original mode, converted from the player's install at every
//! launch (`crate::original`) and never written to disk.
//!
//! The converted pack is **mounted** at a directory that stands next to the base pack
//! (`<data>/original`, see [`crate::platform::DataRoot::memory_pack`]), so the layered-pack
//! chain reads it like a pack on disk: its `pack.toml` says `extends = "../base"`, and every
//! path the game builds for it is `<data>/original/<rel>` or, for a parent's file,
//! `<data>/original/../base/<rel>`. Every file read ([`crate::assets`]) and existence check
//! ([`crate::platform::DataRoot::path`]) goes through [`lookup`]:
//!
//! * a path inside the mount directory is served from memory, and a file the converted pack
//!   lacks is *missing* — a folder of that name on disk (an earlier `hero-tools original pack`
//!   output) is never read while the mount is active;
//! * any other path is read from disk, with `..` resolved lexically first (like URLs and the
//!   pack chain, `docs/DECISIONS.md` D8), because the mount directory need not exist on disk.
//!
//! Without a mount every path is passed through unchanged. The mount lives for the process
//! (macroquad runs the game on one thread); [`unmount`] drops it when the game reloads the data
//! pack.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

struct Mount {
    /// Normalized mount directory with a trailing `/`.
    prefix: String,
    /// File contents by `/`-separated path relative to the mount directory.
    files: HashMap<String, Vec<u8>>,
}

thread_local! {
    static MOUNT: RefCell<Option<Mount>> = const { RefCell::new(None) };
}

/// Where a path is read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Inside the mounted pack: its bytes, or `None` when the pack has no such file.
    Memory(Option<Vec<u8>>),
    /// On disk (or, on the web, by URL) at this path.
    Disk(String),
}

/// Resolve `.` and `..` lexically and use `/` separators. A leading separator (or two, for a
/// UNC path) is kept; `..` at the start of a relative path is kept; a drive (`C:`) is never
/// removed by `..`.
pub fn normalize(path: &str) -> String {
    let is_sep = |c: char| c == '/' || c == '\\';
    let root = match path.chars().take_while(|c| is_sep(*c)).count() {
        0 => "",
        1 => "/",
        _ => "//",
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split(is_sep) {
        match part {
            "" | "." => {}
            ".." => {
                if parts
                    .last()
                    .is_some_and(|l| *l != ".." && !l.ends_with(':'))
                {
                    parts.pop();
                } else if root.is_empty() && parts.last().is_none_or(|l| *l == "..") {
                    parts.push("..");
                }
                // Otherwise at a root or a drive: `..` stays there.
            }
            normal => parts.push(normal),
        }
    }
    format!("{root}{}", parts.join("/"))
}

/// Mount `files` (paths relative to `dir`, `/`-separated) at the directory `dir`, replacing any
/// earlier mount.
pub fn mount(dir: &str, files: BTreeMap<String, Vec<u8>>) {
    let mut prefix = normalize(dir);
    prefix.push('/');
    let files = files
        .into_iter()
        .map(|(rel, bytes)| (normalize(&rel), bytes))
        .collect();
    MOUNT.with(|m| *m.borrow_mut() = Some(Mount { prefix, files }));
}

/// Drop the mounted pack (the data pack is about to be loaded again).
pub fn unmount() {
    MOUNT.with(|m| *m.borrow_mut() = None);
}

/// The mount directory (normalized, without a trailing `/`), if a pack is mounted.
pub fn mounted() -> Option<String> {
    MOUNT.with(|m| {
        m.borrow()
            .as_ref()
            .map(|m| m.prefix.trim_end_matches('/').to_string())
    })
}

/// Where the file at `path` (as built by [`crate::platform::DataRoot`]) is read from.
pub fn lookup(path: &str) -> Lookup {
    MOUNT.with(|m| match m.borrow().as_ref() {
        None => Lookup::Disk(path.to_string()),
        Some(mount) => {
            let norm = normalize(path);
            match norm.strip_prefix(&mount.prefix) {
                Some(rel) => Lookup::Memory(mount.files.get(rel).cloned()),
                None => Lookup::Disk(norm),
            }
        }
    })
}

/// Whether a file exists at `path`: in the mounted pack, or on disk (native builds; the web
/// build cannot check without fetching and answers `false` for files outside the mount).
pub fn is_file(path: &str) -> bool {
    MOUNT.with(|m| match m.borrow().as_ref() {
        None => disk_is_file(path),
        Some(mount) => {
            let norm = normalize(path);
            match norm.strip_prefix(&mount.prefix) {
                Some(rel) => mount.files.contains_key(rel),
                None => disk_is_file(&norm),
            }
        }
    })
}

fn disk_is_file(path: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::path::Path::new(path).is_file()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_lexically() {
        assert_eq!(
            normalize("data/original/../base/x.toml"),
            "data/base/x.toml"
        );
        assert_eq!(normalize("/g/data/original/../base/"), "/g/data/base");
        assert_eq!(
            normalize(r"D:\Project\hero\data\original/../base/pack.toml"),
            "D:/Project/hero/data/base/pack.toml"
        );
        assert_eq!(normalize("./a/./b"), "a/b");
        assert_eq!(normalize("../a/../../b"), "../../b");
        assert_eq!(normalize("/.."), "/");
        assert_eq!(normalize("C:/.."), "C:");
        assert_eq!(normalize(r"\\server\share\a\..\b"), "//server/share/b");
    }

    #[test]
    fn serves_the_mount_from_memory_and_the_rest_from_disk() {
        assert_eq!(lookup("x/../y"), Lookup::Disk("x/../y".into()), "no mount");
        let mut files = BTreeMap::new();
        files.insert("pack.toml".to_string(), b"id = \"original\"".to_vec());
        files.insert("gfx/maps/hexz_00.png".to_string(), vec![1, 2, 3]);
        mount(r"D:\games\data\original", files);
        assert_eq!(mounted().as_deref(), Some("D:/games/data/original"));

        assert_eq!(
            lookup(r"D:\games\data\original/pack.toml"),
            Lookup::Memory(Some(b"id = \"original\"".to_vec()))
        );
        assert_eq!(
            lookup(r"D:\games\data\original/gfx/maps/hexz_00.png"),
            Lookup::Memory(Some(vec![1, 2, 3]))
        );
        // A file the converted pack lacks is missing, whatever is on disk.
        assert_eq!(
            lookup(r"D:\games\data\original/gfx/ui/title.png"),
            Lookup::Memory(None)
        );
        assert!(is_file(r"D:\games\data\original/pack.toml"));
        assert!(!is_file(r"D:\games\data\original/credits.txt"));
        // The parent pack is on disk, reached without going through the mount directory.
        assert_eq!(
            lookup(r"D:\games\data\original/../base/rules/game.toml"),
            Lookup::Disk("D:/games/data/base/rules/game.toml".into())
        );
        // A sibling whose name starts with the mount's is not inside it.
        assert_eq!(
            lookup("D:/games/data/original2/pack.toml"),
            Lookup::Disk("D:/games/data/original2/pack.toml".into())
        );

        unmount();
        assert_eq!(mounted(), None);
        assert_eq!(
            lookup(r"D:\games\data\original/pack.toml"),
            Lookup::Disk(r"D:\games\data\original/pack.toml".into())
        );
    }
}
