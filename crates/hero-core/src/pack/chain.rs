//! Layered packs: a pack whose `pack.toml` says `extends = "../base"` is built on top of the pack
//! in that directory (its **parent**), which may extend another pack in turn.
//!
//! Semantics (documented for modders in `docs/MODDING.md`, "Layered packs"):
//!
//! * The packs of a chain are its **layers**, the **top** pack (the one that is loaded) first.
//!   Every path is computed lexically relative to the top pack directory (`..` removes the
//!   previous segment, like in URLs), so a chain works the same from a directory and over HTTP
//!   (natively the OS opens the result, which differs only after a symbolic link on Unix).
//! * The five rules files, `officers` and `campaign` are each taken from the nearest layer that
//!   lists them: a child's file **replaces** its parent's.
//! * Battles, drama scenes and maps (map files) are the **union** of every layer's files; a
//!   battle, scene or map id that a nearer layer defines again **overrides** the one of the
//!   farther layer. Within one pack an id must still be unique.
//! * `[presentation]` is inherited **field by field**: each field comes from the nearest layer
//!   that sets it, otherwise from [`Presentation::default`]. A field a child leaves out (every
//!   field, in an empty `[presentation]`) keeps the parent's value, so adding a field to
//!   [`Presentation`] never changes what existing packs inherit (`docs/DECISIONS.md` D8).
//! * A chain holds at most [`MAX_CHAIN_DEPTH`] packs, every pack in it needs its own `id`, and a
//!   pack cannot extend itself, directly or through others; a parent that does not exist is an
//!   error.
//!
//! Media files are not read here: the frontend and `Pack::missing_media` look them up in every
//! layer, top first (see [`PackLayer::dir`]).

use super::{parse_error, read, FileSource, PackError, PackManifest, Presentation, MANIFEST_FILE};
use std::collections::BTreeSet;

/// Most packs in one chain: a pack and at most three packs it extends, directly or indirectly.
pub const MAX_CHAIN_DEPTH: usize = 4;

/// Join `rel` onto the directory `dir` (both relative and `/`-separated) and normalise the result
/// lexically: `.` and empty segments are dropped and `..` removes the previous segment. Leading
/// `..` segments that climb above the starting directory are kept. `join_path("", "a")` is `a`,
/// `join_path("../base", "rules/game.toml")` is `../base/rules/game.toml`.
pub fn join_path(dir: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(rel.split('/')) {
        match part {
            "" | "." => {}
            ".." => match parts.last() {
                Some(&last) if last != ".." => {
                    parts.pop();
                }
                _ => parts.push(".."),
            },
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// One pack of a chain.
#[derive(Debug, Clone, PartialEq)]
pub struct PackLayer {
    /// Directory of this pack relative to the top pack directory, `/`-separated and normalised
    /// by [`join_path`]: empty for the top pack itself, `../base` for a parent next to it.
    pub dir: String,
    /// The pack's own `pack.toml`, as written (its `presentation` is the declared value or the
    /// default; see [`PackChain::presentation`] for the inherited one).
    pub manifest: PackManifest,
}

impl PackLayer {
    /// Path of `rel` (a path inside this pack) relative to the top pack directory.
    pub fn file(&self, rel: &str) -> String {
        join_path(&self.dir, rel)
    }

    /// Path of this pack's `pack.toml` relative to the top pack directory.
    pub fn manifest_path(&self) -> String {
        self.file(MANIFEST_FILE)
    }
}

/// A text file of a (layered) pack: `path`, as listed in the `pack.toml` of the pack in `dir`.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackFile {
    /// Directory of the pack that lists the file (see [`PackLayer::dir`]).
    pub dir: String,
    /// Path inside that pack.
    pub path: String,
}

impl PackFile {
    /// Path relative to the top pack directory: the name [`super::Pack::load`] reads the file
    /// by from its [`FileSource`], and the file name in error messages.
    pub fn source_path(&self) -> String {
        join_path(&self.dir, &self.path)
    }
}

/// Where every text file of a (layered) pack comes from, as resolved by [`PackChain::resolve`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackFiles {
    pub game: PackFile,
    pub terrain: PackFile,
    pub classes: PackFile,
    pub strategies: PackFile,
    pub items: PackFile,
    pub officers: PackFile,
    pub campaign: PackFile,
    /// Battle files of every layer, the farthest parent first and the top pack last: a battle
    /// id defined again by a later layer overrides the earlier definition.
    pub battles: Vec<PackFile>,
    /// Drama files of every layer, in the same order and with the same override rule for
    /// scene ids.
    pub dramas: Vec<PackFile>,
    /// Map files of every layer, in the same order and with the same override rule for map
    /// ids.
    pub maps: Vec<PackFile>,
}

impl PackFiles {
    /// Every file, single-role files first, then battles, dramas and maps.
    pub fn all(&self) -> Vec<PackFile> {
        let mut all = vec![
            self.game.clone(),
            self.terrain.clone(),
            self.classes.clone(),
            self.strategies.clone(),
            self.items.clone(),
            self.officers.clone(),
            self.campaign.clone(),
        ];
        all.extend(self.battles.iter().cloned());
        all.extend(self.dramas.iter().cloned());
        all.extend(self.maps.iter().cloned());
        all
    }
}

/// The chain of packs a pack is built from, top pack first.
///
/// Building it is incremental so the web build can fetch one `pack.toml` at a time:
///
/// ```
/// use hero_core::pack::{PackChain, PackError, PackFile};
///
/// /// The files to fetch next, given the top `pack.toml` and a way to read one file.
/// fn plan(top: &str, fetch: impl Fn(&str) -> String) -> Result<Vec<PackFile>, PackError> {
///     let mut chain = PackChain::new(top)?;
///     while let Some(path) = chain.next_parent() {
///         // A failed read becomes `chain.parent_unreadable(why)`.
///         chain.push_parent(&fetch(&path))?;
///     }
///     // Fetch these into a `FileSource` together with the manifests, then `Pack::load` it.
///     chain.text_files()
/// }
/// ```
///
/// [`PackChain::read`] does the same synchronously through a [`FileSource`].
#[derive(Debug, Clone, PartialEq)]
pub struct PackChain {
    layers: Vec<PackLayer>,
    /// `[presentation]` fields, each from the nearest layer that sets it.
    canvas: Option<[u32; 2]>,
    /// Directory of the parent still to be read, relative to the top pack.
    next: Option<String>,
}

impl PackChain {
    /// Start a chain from the text of the top pack's `pack.toml`. Fails when the manifest is
    /// invalid or extends itself.
    pub fn new(top_manifest: &str) -> Result<PackChain, PackError> {
        let mut chain = PackChain {
            layers: Vec::new(),
            canvas: None,
            next: None,
        };
        chain.add(String::new(), top_manifest)?;
        Ok(chain)
    }

    /// Read the whole chain through `src`: the top pack's `pack.toml` and then every parent's.
    pub fn read(src: &dyn FileSource) -> Result<PackChain, PackError> {
        let mut chain = PackChain::new(&read(src, MANIFEST_FILE)?)?;
        while let Some(path) = chain.next_parent() {
            match read(src, &path) {
                Ok(text) => chain.push_parent(&text)?,
                Err(PackError::Missing { .. }) => {
                    return Err(chain.parent_unreadable("file not found"))
                }
                Err(e) => return Err(e),
            }
        }
        Ok(chain)
    }

    /// Path (relative to the top pack directory) of the next `pack.toml` to read and pass to
    /// [`PackChain::push_parent`]; `None` once the chain is complete.
    pub fn next_parent(&self) -> Option<String> {
        self.next
            .as_deref()
            .map(|dir| join_path(dir, MANIFEST_FILE))
    }

    /// Add the parent read from [`PackChain::next_parent`]. Fails when it is invalid, when there
    /// is no parent to add, or when the chain would become a cycle or too deep; the chain is then
    /// unchanged.
    pub fn push_parent(&mut self, manifest: &str) -> Result<(), PackError> {
        let Some(dir) = self.next.clone() else {
            return Err(PackError::Invalid(
                "the pack chain is complete; no parent pack.toml was requested".into(),
            ));
        };
        self.add(dir, manifest)
    }

    /// The error for the parent `pack.toml` of [`PackChain::next_parent`] that could not be read
    /// (`why`, e.g. "file not found"), naming the manifest whose `extends` points at it.
    pub fn parent_unreadable(&self, why: &str) -> PackError {
        let pending = self
            .next_parent()
            .zip(self.layers.last())
            .and_then(|(parent, last)| {
                let extends = last.manifest.extends.as_deref()?;
                Some((parent, last, extends))
            });
        match pending {
            Some((parent, last, extends)) => parse_error(
                &last.manifest_path(),
                format!("extends `{extends}`, but {parent} cannot be read: {why}"),
            ),
            None => PackError::Invalid(format!(
                "a parent pack.toml could not be read, but none was pending: {why}"
            )),
        }
    }

    /// The packs of the chain, top pack first.
    pub fn layers(&self) -> &[PackLayer] {
        &self.layers
    }

    /// Whether every parent has been read.
    pub fn is_complete(&self) -> bool {
        self.next.is_none()
    }

    /// The inherited `[presentation]`: each field from the nearest layer that sets it, otherwise
    /// the default.
    pub fn presentation(&self) -> Presentation {
        let default = Presentation::default();
        Presentation {
            canvas: self.canvas.unwrap_or(default.canvas),
        }
    }

    /// Decide which layer provides each file (see the module docs). Fails when the chain is not
    /// complete yet, when a rules file, `officers` or `campaign` is listed by no layer, or when
    /// one file would be read twice.
    pub fn resolve(&self) -> Result<PackFiles, PackError> {
        if !self.is_complete() {
            return Err(PackError::Invalid(format!(
                "the pack chain is incomplete: {} has not been read",
                self.next_parent().unwrap_or_default()
            )));
        }
        let role = |name: &str, pick: fn(&PackManifest) -> &Option<String>| {
            self.layers
                .iter()
                .find_map(|layer| {
                    pick(&layer.manifest).as_ref().map(|path| PackFile {
                        dir: layer.dir.clone(),
                        path: path.clone(),
                    })
                })
                .ok_or_else(|| {
                    let msg = if self.layers.len() == 1 {
                        format!(
                            "`{name}` is missing: a pack without `extends` must list all five rules files, `officers` and `campaign`"
                        )
                    } else {
                        format!("`{name}` is listed neither here nor in any pack this one extends")
                    };
                    parse_error(MANIFEST_FILE, msg)
                })
        };
        let listed = |pick: fn(&PackManifest) -> &Vec<String>| -> Vec<PackFile> {
            self.layers
                .iter()
                .rev()
                .flat_map(|layer| {
                    pick(&layer.manifest).iter().map(|path| PackFile {
                        dir: layer.dir.clone(),
                        path: path.clone(),
                    })
                })
                .collect()
        };
        let files = PackFiles {
            game: role("rules.game", |m| &m.rules.game)?,
            terrain: role("rules.terrain", |m| &m.rules.terrain)?,
            classes: role("rules.classes", |m| &m.rules.classes)?,
            strategies: role("rules.strategies", |m| &m.rules.strategies)?,
            items: role("rules.items", |m| &m.rules.items)?,
            officers: role("officers", |m| &m.officers)?,
            campaign: role("campaign", |m| &m.campaign)?,
            battles: listed(|m| &m.battles),
            dramas: listed(|m| &m.dramas),
            maps: listed(|m| &m.maps),
        };
        // Each pack lists a file once (checked per manifest); a parent directory inside a
        // child could still make two layers name the same file.
        let mut seen = BTreeSet::new();
        for file in files.all() {
            let path = file.source_path();
            if !seen.insert(path.clone()) {
                return Err(parse_error(
                    MANIFEST_FILE,
                    format!("file `{path}` is listed more than once in the pack chain"),
                ));
            }
        }
        Ok(files)
    }

    /// Every text file [`super::Pack::load`] reads besides the `pack.toml` files of the chain
    /// (which [`PackChain::layers`] lists): the files of [`PackChain::resolve`].
    pub fn text_files(&self) -> Result<Vec<PackFile>, PackError> {
        Ok(self.resolve()?.all())
    }

    /// Parse and check the manifest of the pack in `dir`, add it and work out its parent. A
    /// rejected manifest leaves the chain as it was.
    fn add(&mut self, dir: String, text: &str) -> Result<(), PackError> {
        let file = join_path(&dir, MANIFEST_FILE);
        let manifest = PackManifest::parse_file(&file, text)?;
        manifest.check(&file)?;
        if let Some(other) = self
            .layers
            .iter()
            .find(|layer| layer.manifest.id == manifest.id)
        {
            return Err(parse_error(
                &file,
                format!(
                    "pack id `{}` is also the id of {}: the packs of a chain need distinct ids, and a pack cannot extend itself, directly or through other packs",
                    manifest.id,
                    other.manifest_path()
                ),
            ));
        }
        let parent = manifest.extends.as_deref().map(|e| join_path(&dir, e));
        if let Some(parent) = &parent {
            let in_chain = *parent == dir || self.layers.iter().any(|l| l.dir == *parent);
            if in_chain {
                return Err(parse_error(
                    &file,
                    format!(
                        "extends {}, which is already part of this chain: a pack cannot extend itself, directly or through other packs",
                        describe_dir(parent)
                    ),
                ));
            }
            // This pack and its parent would make one more than the packs read so far.
            if self.layers.len() + 1 >= MAX_CHAIN_DEPTH {
                return Err(parse_error(
                    &file,
                    format!(
                        "extends {}, but a chain holds at most {MAX_CHAIN_DEPTH} packs (a pack and {} packs it extends)",
                        describe_dir(parent),
                        MAX_CHAIN_DEPTH - 1
                    ),
                ));
            }
        }
        let declared = presentation_fields(text);
        if self.canvas.is_none() && declared.iter().any(|f| f == "canvas") {
            self.canvas = Some(manifest.presentation.canvas);
        }
        self.layers.push(PackLayer { dir, manifest });
        self.next = parent;
        Ok(())
    }
}

/// A layer directory for messages.
fn describe_dir(dir: &str) -> String {
    if dir.is_empty() {
        "the top pack's own directory".into()
    } else {
        format!("`{dir}`")
    }
}

/// The fields a `pack.toml` sets in `[presentation]` (only those override the parent's). The
/// text has already been parsed as a [`PackManifest`], so it is valid TOML.
fn presentation_fields(text: &str) -> Vec<String> {
    toml::from_str::<toml::Table>(text.strip_prefix('\u{feff}').unwrap_or(text))
        .ok()
        .and_then(|t| t.get("presentation").and_then(|p| p.as_table()).cloned())
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_path_normalises_lexically() {
        assert_eq!(join_path("", "pack.toml"), "pack.toml");
        assert_eq!(join_path("", "rules/game.toml"), "rules/game.toml");
        assert_eq!(
            join_path("../base", "rules/game.toml"),
            "../base/rules/game.toml"
        );
        assert_eq!(join_path("../base", "../core"), "../core");
        assert_eq!(join_path("", "./base/"), "base");
        assert_eq!(join_path("a/b", "../../.."), "..");
        assert_eq!(join_path("..", "../x"), "../../x");
        assert_eq!(join_path("", "."), "");
    }
}
