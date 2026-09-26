//! Key/value persistence for save games and settings.
//!
//! Keys are short identifiers (`settings`, `save_base_auto`, `save_base_3`; lowercase ASCII letters,
//! digits, `_` and `-`). Values are UTF-8 text (JSON documents).
//!
//! * Natively each key is a file `<key>.json` in the user data directory
//!   (Windows `%APPDATA%\EiketsudenReloaded`, macOS `~/Library/Application
//!   Support/EiketsudenReloaded`, Linux `$XDG_DATA_HOME/eiketsuden-reloaded` or
//!   `~/.local/share/eiketsuden-reloaded`). The directory is created on the first write and every
//!   write is atomic (temporary file + rename), so a crash never leaves a half-written save.
//! * On the web each key is the `localStorage` item `eiketsuden.<key>`, accessed through
//!   `web/hero_web.js`.

use std::collections::BTreeMap;
use std::fmt;

/// Why a storage operation failed. The message is shown to the player (toast / dialog).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageError {
    pub key: String,
    pub msg: String,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.msg)
    }
}

impl std::error::Error for StorageError {}

/// Persistent string storage. See the module documentation for the key rules.
pub trait KeyValueStore {
    /// `Ok(None)` when nothing is stored under `key`.
    fn get(&self, key: &str) -> Result<Option<String>, StorageError>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError>;
    /// Removing a missing key succeeds.
    fn remove(&mut self, key: &str) -> Result<(), StorageError>;
    /// Where the data lives, for the settings/credits screens and error messages.
    fn location(&self) -> String;
}

/// Check the key syntax shared by every backend.
pub fn validate_key(key: &str) -> Result<(), StorageError> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(StorageError {
            key: key.to_string(),
            msg: "invalid storage key".into(),
        })
    }
}

/// The storage backend of this platform. Natively, when no user data directory can be
/// determined, the returned store reports every write as an error (nothing is silently lost).
pub fn open_default() -> Box<dyn KeyValueStore> {
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(super::web::LocalStorage)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match user_data_dir() {
            Some(dir) => Box::new(native::FileStore::new(dir)),
            None => Box::new(UnavailableStore {
                reason: "no user data directory (APPDATA / HOME is not set)".into(),
            }),
        }
    }
}

/// In-memory store for tests and tools.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    pub items: BTreeMap<String, String>,
}

impl KeyValueStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        validate_key(key)?;
        Ok(self.items.get(key).cloned())
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.items.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.items.remove(key);
        Ok(())
    }

    fn location(&self) -> String {
        "memory".into()
    }
}

/// Store used when the platform offers no persistent location: reads find nothing and writes
/// fail with `reason`.
#[derive(Debug, Clone)]
pub struct UnavailableStore {
    pub reason: String,
}

impl KeyValueStore for UnavailableStore {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        validate_key(key)?;
        Ok(None)
    }

    fn set(&mut self, key: &str, _value: &str) -> Result<(), StorageError> {
        Err(StorageError {
            key: key.to_string(),
            msg: self.reason.clone(),
        })
    }

    fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        Err(StorageError {
            key: key.to_string(),
            msg: self.reason.clone(),
        })
    }

    fn location(&self) -> String {
        format!("(unavailable: {})", self.reason)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::FileStore;

/// The per-user data directory of this OS (saves, settings, crash log), not created here.
/// `None` when the relevant environment variable (`APPDATA` / `HOME`) is missing.
#[cfg(not(target_arch = "wasm32"))]
pub fn user_data_dir() -> Option<std::path::PathBuf> {
    native::default_dir()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{validate_key, KeyValueStore, StorageError};
    use std::fs;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    /// Directory name under the OS user data directory.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    const APP_DIR: &str = "EiketsudenReloaded";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    const APP_DIR: &str = "eiketsuden-reloaded";

    /// The per-user data directory of this OS (not created here).
    pub fn default_dir() -> Option<PathBuf> {
        let var = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        #[cfg(target_os = "windows")]
        {
            var("APPDATA").map(|d| d.join(APP_DIR))
        }
        #[cfg(target_os = "macos")]
        {
            var("HOME").map(|h| h.join("Library").join("Application Support").join(APP_DIR))
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            var("XDG_DATA_HOME")
                .or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
                .map(|d| d.join(APP_DIR))
        }
    }

    /// One JSON file per key inside `dir`.
    #[derive(Debug, Clone)]
    pub struct FileStore {
        dir: PathBuf,
    }

    impl FileStore {
        pub fn new(dir: PathBuf) -> FileStore {
            FileStore { dir }
        }

        pub fn dir(&self) -> &Path {
            &self.dir
        }

        fn file(&self, key: &str) -> Result<PathBuf, StorageError> {
            validate_key(key)?;
            Ok(self.dir.join(format!("{key}.json")))
        }

        fn err(key: &str, what: &str, e: io::Error) -> StorageError {
            StorageError {
                key: key.to_string(),
                msg: format!("{what}: {e}"),
            }
        }
    }

    impl KeyValueStore for FileStore {
        fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
            let path = self.file(key)?;
            match fs::read_to_string(&path) {
                Ok(s) => Ok(Some(s)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(Self::err(
                    key,
                    &format!("cannot read {}", path.display()),
                    e,
                )),
            }
        }

        fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
            let path = self.file(key)?;
            fs::create_dir_all(&self.dir)
                .map_err(|e| Self::err(key, &format!("cannot create {}", self.dir.display()), e))?;
            let tmp = self.dir.join(format!("{key}.json.tmp"));
            let write = || -> io::Result<()> {
                let mut f = fs::File::create(&tmp)?;
                f.write_all(value.as_bytes())?;
                f.sync_all()?;
                drop(f);
                // `rename` replaces an existing file atomically on every supported OS.
                fs::rename(&tmp, &path)
            };
            write().map_err(|e| {
                // Best effort: do not leave the temporary file behind. The original error is
                // what matters to the caller.
                let _ = fs::remove_file(&tmp);
                Self::err(key, &format!("cannot write {}", path.display()), e)
            })
        }

        fn remove(&mut self, key: &str) -> Result<(), StorageError> {
            let path = self.file(key)?;
            match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(Self::err(
                    key,
                    &format!("cannot delete {}", path.display()),
                    e,
                )),
            }
        }

        fn location(&self) -> String {
            self.dir.display().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rules() {
        assert!(validate_key("save_auto").is_ok());
        assert!(validate_key("save-1").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key("Save").is_err());
        assert!(validate_key("../etc").is_err());
        assert!(validate_key(&"a".repeat(65)).is_err());
    }

    #[test]
    fn memory_store_roundtrip() {
        let mut s = MemoryStore::default();
        assert_eq!(s.get("k").unwrap(), None);
        s.set("k", "v").unwrap();
        assert_eq!(s.get("k").unwrap().as_deref(), Some("v"));
        s.remove("k").unwrap();
        s.remove("k").unwrap();
        assert_eq!(s.get("k").unwrap(), None);
        assert!(s.set("bad key", "v").is_err());
    }

    #[test]
    fn unavailable_store_reports_writes() {
        let mut s = UnavailableStore {
            reason: "none".into(),
        };
        assert_eq!(s.get("k").unwrap(), None);
        assert!(s.set("k", "v").is_err());
        assert!(s.remove("k").is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn file_store_roundtrip_and_atomic_replace() {
        let dir = std::env::temp_dir().join(format!(
            "hero-game-storage-test-{}-{}",
            std::process::id(),
            crate::platform::unix_now()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = FileStore::new(dir.join("nested"));
        assert_eq!(s.get("save_1").unwrap(), None);
        s.set("save_1", "{\"a\":1}").unwrap();
        s.set("save_1", "{\"a\":2}").unwrap();
        assert_eq!(s.get("save_1").unwrap().as_deref(), Some("{\"a\":2}"));
        assert!(!s.dir().join("save_1.json.tmp").exists());
        s.remove("save_1").unwrap();
        s.remove("save_1").unwrap();
        assert_eq!(s.get("save_1").unwrap(), None);
        assert!(s.get("../x").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
