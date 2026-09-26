//! Read-only access to the files of an install folder.
//!
//! DOS file names are matched case-insensitively (copies made on other systems are often
//! lower-case). Only the folder itself is indexed; the probe walks sub-folders separately.
//! Files are only ever opened for reading.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Largest file read into memory (the known data files are below 1 MiB).
pub const MAX_READ: u64 = 64 << 20;

/// An install folder or one of its files could not be read.
#[derive(Debug)]
pub struct InstallError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for InstallError {}

impl InstallError {
    pub(crate) fn io(path: &Path, e: &std::io::Error) -> InstallError {
        InstallError {
            path: path.to_path_buf(),
            message: e.to_string(),
        }
    }
}

/// The regular files directly inside an install folder, by upper-case name.
#[derive(Debug, Clone)]
pub struct InstallDir {
    root: PathBuf,
    files: BTreeMap<String, PathBuf>,
}

impl InstallDir {
    /// Index the files of `root` (not recursive; symbolic links are ignored).
    pub fn open(root: &Path) -> Result<InstallDir, InstallError> {
        let meta = std::fs::metadata(root).map_err(|e| InstallError::io(root, &e))?;
        if !meta.is_dir() {
            return Err(InstallError {
                path: root.to_path_buf(),
                message: "not a directory".into(),
            });
        }
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(root).map_err(|e| InstallError::io(root, &e))? {
            let entry = entry.map_err(|e| InstallError::io(root, &e))?;
            let file_type = entry
                .file_type()
                .map_err(|e| InstallError::io(&entry.path(), &e))?;
            if !file_type.is_file() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str() {
                files.insert(name.to_uppercase(), entry.path());
            }
        }
        Ok(InstallDir {
            root: root.to_path_buf(),
            files,
        })
    }

    /// The folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether a file exists (case-insensitive).
    pub fn has(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_uppercase())
    }

    /// Path of a file (case-insensitive).
    pub fn path(&self, name: &str) -> Option<&Path> {
        self.files.get(&name.to_uppercase()).map(PathBuf::as_path)
    }

    /// Upper-case names of every file, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Read a whole file; `Ok(None)` when it does not exist.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, InstallError> {
        match self.path(name) {
            Some(path) => read_limited(path).map(Some),
            None => Ok(None),
        }
    }
}

/// Read a file of at most [`MAX_READ`] bytes.
pub fn read_limited(path: &Path) -> Result<Vec<u8>, InstallError> {
    let len = std::fs::metadata(path)
        .map_err(|e| InstallError::io(path, &e))?
        .len();
    if len > MAX_READ {
        return Err(InstallError {
            path: path.to_path_buf(),
            message: format!("{len} bytes is larger than the {MAX_READ}-byte limit"),
        });
    }
    std::fs::read(path).map_err(|e| InstallError::io(path, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_lookup() {
        let dir = crate::testutil::TempDir::new("install");
        std::fs::write(dir.path().join("Main.exe"), b"MZ").unwrap();
        std::fs::write(dir.path().join("snr0m.r3"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("SUB")).unwrap();
        let install = InstallDir::open(dir.path()).unwrap();
        assert!(install.has("MAIN.EXE"));
        assert!(install.has("SNR0M.R3"));
        assert!(!install.has("SUB"));
        assert_eq!(install.read("main.EXE").unwrap(), Some(b"MZ".to_vec()));
        assert_eq!(install.read("NOPE.R3").unwrap(), None);
        assert_eq!(
            install.names().collect::<Vec<_>>(),
            ["MAIN.EXE", "SNR0M.R3"]
        );
    }

    #[test]
    fn errors_name_the_path() {
        let dir = crate::testutil::TempDir::new("install-missing");
        let missing = dir.path().join("nope");
        let err = InstallDir::open(&missing).unwrap_err();
        assert!(err.to_string().contains("nope"), "{err}");
        std::fs::write(dir.path().join("file"), b"").unwrap();
        let err = InstallDir::open(&dir.path().join("file")).unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err}");
    }
}
