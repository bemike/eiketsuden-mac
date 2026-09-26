//! Conversion of an install into a **media overlay** folder.
//!
//! ```text
//! <out>/index.json                        what was extracted, from which edition and files
//! <out>/gfx/original/palettes.json        the palette bank of MAIN.EXE (9 slots × 16 colours)
//! <out>/gfx/original/sprites.json         per archive: palette slot, geometry of every entry,
//!                                         entry groups (unit classes, effects)
//! <out>/gfx/original/<archive>/<nnn>.png  one image per sprite / chip entry (media key
//!                                         `original/<archive>/<nnn>`), indexed, colour 0
//!                                         transparent
//! <out>/gfx/original/sheets/<archive>.png every entry of an archive on one contact sheet
//! <out>/gfx/original/facedat/<nnn>.png    TF-DCE portraits of FACEDAT.R3
//! <out>/text/<file>.json                  message files as UTF-8 JSON
//! <out>/maps/battle.json                  battle maps: names, chip sets, terrain table
//!                                         (names and battle-scene strips per terrain from
//!                                         MAIN.EXE), chip → terrain statistics
//! <out>/maps/battle/<nnn>.json            chip and terrain grids of one battle map
//! <out>/maps/{scene,campaign,town}.json   battle-scene strips, campaign maps (tiles + routes),
//!                                         town / palace screens (tiles, walk grid, objects)
//! <out>/gfx/original/maps/...             the maps drawn with their chips (see [`maps`])
//! ```
//!
//! The game reads the folder with `--original <out>`: media keys are looked up there first,
//! then in the data pack. Only the DOS/V editions can be extracted; every asset kind reports its
//! own status (`extracted`, `partial`, `failed`, `unsupported`, `missing-source`) so an
//! unsupported format (the `BAKDATA.R3` record layout) is an explicit result
//! rather than a guess.
//!
//! The install is only read. The output folder must not lie inside it, and must be empty, new,
//! or a previous extraction (whose listed files are replaced).

use crate::edition::{identify, Edition, EditionId};
use crate::image::IndexedImage;
use crate::image::{encode_png, grey_ramp, Palette16};
use crate::install::{lies_inside, InstallDir, InstallError};
use crate::sprites::{self, Group, SpriteArchive};
use crate::text::{parse_messages, TextEncoding};
use crate::{ls11, maps, palette, table6};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Name of the index file in the output folder.
pub const INDEX_FILE: &str = "index.json";
/// Index format identifier.
pub const FORMAT: &str = "eiketsuden-original-overlay";
/// Index format version.
pub const FORMAT_VERSION: u32 = 1;
/// Folder of the extracted graphics inside the overlay (media keys `original/...`).
pub const GFX_DIR: &str = "gfx/original";
/// Folder of the extracted text.
pub const TEXT_DIR: &str = "text";
/// Portrait container.
pub const PORTRAIT_SOURCE: &str = "FACEDAT.R3";

/// Entries per row on the contact sheets.
pub const SHEET_COLUMNS: usize = 16;

/// Message files and the scenario archive whose scene count each must match.
pub const TEXT_SOURCES: [(&str, Option<&str>); 6] = [
    ("SNR0M.R3", Some("SNR0D.R3")),
    ("SNR1M.R3", Some("SNR1D.R3")),
    ("SNR2M.R3", Some("SNR2D.R3")),
    ("SNR3M.R3", Some("SNR3D.R3")),
    ("SNR4M.R3", Some("SNR4D.R3")),
    ("IPPAN0M.R3", None),
];

/// Asset kinds chosen on the command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Selection {
    pub maps: bool,
    pub portraits: bool,
    pub sprites: bool,
    pub text: bool,
}

impl Selection {
    /// Every kind.
    pub fn all() -> Selection {
        Selection {
            maps: true,
            portraits: true,
            sprites: true,
            text: true,
        }
    }
}

/// What to extract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Explicitly chosen kinds; `None` extracts every kind and treats unsupported ones as
    /// informational. An explicitly chosen kind that cannot be extracted is a failure.
    pub selection: Option<Selection>,
    /// Use this edition instead of identifying it (must be a DOS/V edition).
    pub edition: Option<EditionId>,
}

/// Outcome of one asset kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Everything found was converted.
    Extracted,
    /// Some files converted, some failed (see `errors`).
    Partial,
    /// Nothing could be converted (see `errors`).
    Failed,
    /// The format is not implemented (see `summary` and `notes`).
    Unsupported,
    /// The install has none of the source files.
    MissingSource,
}

/// A source file and its hash, for provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    pub file: String,
    pub sha256: String,
}

/// Report of one asset kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindReport {
    pub status: Status,
    /// Chosen explicitly (then `unsupported` / `missing-source` count as failures).
    pub requested: bool,
    pub summary: String,
    /// Files written.
    pub outputs: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

impl KindReport {
    fn new(status: Status, requested: bool, summary: impl Into<String>) -> KindReport {
        KindReport {
            status,
            requested,
            summary: summary.into(),
            outputs: 0,
            sources: Vec::new(),
            notes: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Whether this kind counts as a success for the exit code.
    pub fn ok(&self) -> bool {
        match self.status {
            Status::Extracted => true,
            Status::Partial | Status::Failed => false,
            Status::Unsupported | Status::MissingSource => !self.requested,
        }
    }

    /// Status from how many files were found, converted and failed.
    fn settle(&mut self, found: usize, failed: usize) {
        self.status = if found == 0 {
            Status::MissingSource
        } else if failed == 0 {
            Status::Extracted
        } else if failed < found {
            Status::Partial
        } else {
            Status::Failed
        };
    }
}

/// Contents of `index.json`.
#[derive(Debug, Clone, Serialize)]
pub struct Index {
    pub format: String,
    pub format_version: u32,
    pub tool: String,
    pub edition: Edition,
    /// By asset kind: `names`, `portraits`, `sprites`, `text`.
    pub assets: BTreeMap<String, KindReport>,
    /// Every file written, relative to the output folder.
    pub files: Vec<String>,
}

impl Index {
    /// `true` when no asset kind failed (see [`KindReport::ok`]).
    pub fn success(&self) -> bool {
        self.assets.values().all(KindReport::ok)
    }
}

/// The part of a previous `index.json` needed to replace its files.
#[derive(Deserialize)]
struct PreviousIndex {
    format: String,
    files: Vec<String>,
}

/// Why extraction could not run (per-kind problems are reported in the [`Index`] instead).
#[derive(Debug)]
pub enum ExtractError {
    Source(InstallError),
    /// The edition is not one whose files can be converted.
    NotExtractable(Box<Edition>),
    /// `--edition` named an edition that cannot be converted.
    BadForcedEdition(EditionId),
    /// The output folder and the install are the same, or one lies inside the other.
    OutputOverlapsSource {
        output: PathBuf,
        source: PathBuf,
    },
    /// The output folder holds files that are not a previous extraction.
    OutputNotEmpty(PathBuf),
    Output {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractError::Source(e) => write!(f, "{e}"),
            ExtractError::NotExtractable(edition) => write!(
                f,
                "{} (`{}`) cannot be extracted yet; only the DOS/V editions (korean-dos, \
                 chinese-dos) are supported. Run `probe` and share the manifest to help add it.",
                edition.name,
                edition.id.as_str()
            ),
            ExtractError::BadForcedEdition(id) => write!(
                f,
                "--edition {} is not extractable (use korean-dos or chinese-dos)",
                id.as_str()
            ),
            ExtractError::OutputOverlapsSource { output, source } => write!(
                f,
                "the output folder {} and the install {} overlap (one lies inside the other); \
                 choose a separate folder (the install is never written to)",
                output.display(),
                source.display()
            ),
            ExtractError::OutputNotEmpty(path) => write!(
                f,
                "the output folder {} is not empty and holds no previous extraction \
                 ({INDEX_FILE}); choose an empty or new folder (if an earlier extraction \
                 into it was interrupted, delete the folder and run again)",
                path.display()
            ),
            ExtractError::Output { path, message } => {
                write!(f, "{}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ExtractError {}

impl From<InstallError> for ExtractError {
    fn from(e: InstallError) -> ExtractError {
        ExtractError::Source(e)
    }
}

fn output_error(path: &Path, e: impl fmt::Display) -> ExtractError {
    ExtractError::Output {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

/// Files written into the output folder.
struct Output {
    root: PathBuf,
    files: Vec<String>,
}

impl Output {
    fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<(), ExtractError> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| output_error(parent, e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| output_error(&path, e))?;
        self.files.push(rel.to_string());
        Ok(())
    }

    fn write_json(&mut self, rel: &str, value: &impl Serialize) -> Result<(), ExtractError> {
        let mut bytes =
            serde_json::to_vec_pretty(value).map_err(|e| output_error(&self.root.join(rel), e))?;
        bytes.push(b'\n');
        self.write(rel, &bytes)
    }
}

/// A relative path from a previous index that stays inside the output folder.
fn safe_relative(rel: &str) -> Option<PathBuf> {
    let path = Path::new(rel);
    let ok = !rel.is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)));
    ok.then(|| path.to_path_buf())
}

/// Check the output folder and remove the files of a previous extraction.
fn prepare_output(source: &Path, out: &Path) -> Result<(), ExtractError> {
    let overlap = lies_inside(out, source)
        .and_then(|inside| {
            if inside {
                Ok(true)
            } else {
                lies_inside(source, out)
            }
        })
        .map_err(|e| output_error(out, e))?;
    if overlap {
        return Err(ExtractError::OutputOverlapsSource {
            output: out.to_path_buf(),
            source: source.to_path_buf(),
        });
    }
    if !out.exists() {
        return std::fs::create_dir_all(out).map_err(|e| output_error(out, e));
    }
    if !out.is_dir() {
        return Err(output_error(out, "exists and is not a folder"));
    }
    let mut entries = std::fs::read_dir(out).map_err(|e| output_error(out, e))?;
    if entries.next().is_none() {
        return Ok(());
    }
    let index_path = out.join(INDEX_FILE);
    let previous: PreviousIndex = match std::fs::read(&index_path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| ExtractError::OutputNotEmpty(out.to_path_buf()))?,
        Err(_) => return Err(ExtractError::OutputNotEmpty(out.to_path_buf())),
    };
    if previous.format != FORMAT {
        return Err(ExtractError::OutputNotEmpty(out.to_path_buf()));
    }
    for rel in &previous.files {
        let Some(path) = safe_relative(rel) else {
            return Err(output_error(
                &index_path,
                format!("lists an unsafe path `{rel}`; remove the folder by hand"),
            ));
        };
        let path = out.join(path);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(output_error(&path, e)),
        }
    }
    std::fs::remove_file(&index_path).map_err(|e| output_error(&index_path, e))
}

/// Extract the selected assets of the install in `source` into `out`.
pub fn extract(source: &Path, out: &Path, options: &Options) -> Result<Index, ExtractError> {
    let install = InstallDir::open(source)?;
    let identified = identify(&install);
    let edition = match options.edition {
        Some(id) if id.is_extractable() => Edition::forced(id, &identified),
        Some(id) => return Err(ExtractError::BadForcedEdition(id)),
        None => identified,
    };
    let Some(encoding) = edition.id.text_encoding() else {
        return Err(ExtractError::NotExtractable(Box::new(edition)));
    };
    prepare_output(source, out)?;

    let (selection, requested) = match options.selection {
        Some(s) => (s, true),
        None => (Selection::all(), false),
    };
    let mut output = Output {
        root: out.to_path_buf(),
        files: Vec::new(),
    };
    let mut assets = BTreeMap::new();
    if selection.text {
        assets.insert(
            "text".to_string(),
            extract_text(&install, encoding, &mut output, requested)?,
        );
        assets.insert("names".to_string(), names_report());
    }
    if selection.sprites {
        assets.insert(
            "sprites".to_string(),
            extract_sprites(&install, &mut output, requested)?,
        );
    }
    if selection.portraits {
        assets.insert(
            "portraits".to_string(),
            extract_portraits(&install, &mut output, requested)?,
        );
    }
    if selection.maps {
        assets.insert(
            "maps".to_string(),
            extract_maps(&install, encoding, &mut output, requested)?,
        );
    }
    output.files.sort();
    let index = Index {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        tool: crate::tool_version(),
        edition,
        assets,
        files: output.files.clone(),
    };
    output.write_json(INDEX_FILE, &index)?;
    Ok(index)
}

fn read_source(
    install: &InstallDir,
    name: &str,
    report: &mut KindReport,
) -> Result<Option<Vec<u8>>, ExtractError> {
    let data = install.read(name)?;
    if let Some(bytes) = &data {
        report.sources.push(Source {
            file: name.to_string(),
            sha256: crate::sha256_hex(bytes),
        });
    }
    Ok(data)
}

// ----- text ----------------------------------------------------------------------------------

#[derive(Serialize)]
struct TextFile {
    source: String,
    encoding: &'static str,
    blocks: usize,
    malformed_blocks: usize,
    note: &'static str,
    sections: Vec<TextSection>,
}

#[derive(Serialize)]
struct TextSection {
    index: usize,
    base: usize,
    blocks: Vec<TextBlock>,
}

#[derive(Serialize)]
struct TextBlock {
    offset: usize,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    malformed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_hex: Option<String>,
}

const TEXT_NOTE: &str = "Blocks are split at NUL bytes; offsets are relative to the section base \
    (as the scenario bytecode addresses them). Dialogue records start with a u16le speaker id \
    that is not separated from the text.";

/// The message bytes of a file: the file itself, or the single entry of an LS11 archive.
fn message_payload(data: &[u8]) -> Result<Cow<'_, [u8]>, String> {
    if !(ls11::has_magic(data) || ls11::has_variant_magic(data)) {
        return Ok(Cow::Borrowed(data));
    }
    let archive = ls11::Archive::parse(data).map_err(|e| e.to_string())?;
    if archive.len() != 1 {
        return Err(format!(
            "LS11 archive with {} entries (a message file is a single entry)",
            archive.len()
        ));
    }
    archive.decode(0).map(Cow::Owned).map_err(|e| e.to_string())
}

fn scene_count(install: &InstallDir, name: &str) -> Result<Option<usize>, String> {
    let data = match install.read(name) {
        Ok(Some(d)) => d,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    ls11::Archive::parse(&data)
        .map(|a| Some(a.len()))
        .map_err(|e| format!("{name}: {e}"))
}

fn convert_messages(
    install: &InstallDir,
    name: &str,
    scenes_from: Option<&str>,
    data: &[u8],
    encoding: TextEncoding,
    notes: &mut Vec<String>,
) -> Result<(TextFile, usize), String> {
    let payload = message_payload(data)?;
    let messages = parse_messages(&payload).map_err(|e| e.to_string())?;
    if let Some(d) = scenes_from {
        match scene_count(install, d)? {
            Some(expected) if expected != messages.sections.len() => {
                return Err(crate::text::TextError::SectionCount {
                    found: messages.sections.len(),
                    expected,
                }
                .to_string())
            }
            Some(_) => {}
            None => notes.push(format!(
                "{d} missing: {name}'s scene count not cross-checked"
            )),
        }
    }
    let mut malformed_blocks = 0;
    let sections: Vec<TextSection> = messages
        .sections
        .iter()
        .enumerate()
        .map(|(index, s)| TextSection {
            index,
            base: s.base,
            blocks: s
                .blocks
                .iter()
                .map(|b| {
                    let d = encoding.decode(b.bytes);
                    malformed_blocks += usize::from(d.malformed);
                    TextBlock {
                        offset: b.offset,
                        text: d.text,
                        malformed: d.malformed,
                        raw_hex: d.malformed.then(|| crate::hex(b.bytes)),
                    }
                })
                .collect(),
        })
        .collect();
    let blocks = messages.block_count();
    Ok((
        TextFile {
            source: name.to_string(),
            encoding: encoding.name(),
            blocks,
            malformed_blocks,
            note: TEXT_NOTE,
            sections,
        },
        blocks,
    ))
}

fn extract_text(
    install: &InstallDir,
    encoding: TextEncoding,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let (mut found, mut failed, mut blocks, mut malformed) = (0, 0, 0, 0);
    for (name, scenes_from) in TEXT_SOURCES {
        let Some(data) = read_source(install, name, &mut report)? else {
            continue;
        };
        found += 1;
        match convert_messages(
            install,
            name,
            scenes_from,
            &data,
            encoding,
            &mut report.notes,
        ) {
            Ok((file, n)) => {
                blocks += n;
                malformed += file.malformed_blocks;
                let stem = name.trim_end_matches(".R3").to_lowercase();
                out.write_json(&format!("{TEXT_DIR}/{stem}.json"), &file)?;
                report.outputs += 1;
            }
            Err(e) => {
                failed += 1;
                report.errors.push(format!("{name}: {e}"));
            }
        }
    }
    report.settle(found, failed);
    report.summary = if found == 0 {
        "no message files (SNR0M.R3–SNR4M.R3, IPPAN0M.R3) in the install".into()
    } else {
        format!(
            "{} of {found} message files converted ({blocks} blocks, {malformed} not cleanly decodable) as {}",
            report.outputs,
            encoding.name()
        )
    };
    Ok(report)
}

fn names_report() -> KindReport {
    let mut r = KindReport::new(
        Status::Unsupported,
        false,
        "officer and item names (BAKDATA.R3) are not extracted: the record layout is not published",
    );
    r.notes.push(
        "known from the Chinese editor: 384 officers (name ≤ 6 bytes) and 63 items (name ≤ 10 \
         bytes); byte offsets unknown, so reading them would be guesswork"
            .into(),
    );
    r
}

// ----- sprites -------------------------------------------------------------------------------

#[derive(Serialize)]
struct PaletteFile {
    source: &'static str,
    offset: String,
    order: &'static str,
    slots: Vec<Vec<String>>,
    slot_notes: Vec<&'static str>,
    note: &'static str,
}

const PALETTE_NOTE: &str = "The game picks the slot at run time from scenario / map data; the \
    sprite PNGs use the slot listed per archive in sprites.json (checked visually). The PNGs are \
    indexed, so another slot can be applied without re-extracting.";

/// The palette bank of `MAIN.EXE`, written to `palettes.json`; `None` (with an error) when it
/// cannot be located.
fn palette_bank(
    install: &InstallDir,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<Option<[Palette16; palette::SLOTS]>, ExtractError> {
    let exe = read_source(install, "MAIN.EXE", report)?;
    let bank = match exe.as_deref().map(palette::find_bank) {
        Some(Ok(bank)) => bank,
        Some(Err(e)) => {
            report
                .errors
                .push(format!("MAIN.EXE: {e}; sprites use a grey ramp"));
            return Ok(None);
        }
        None => {
            report
                .errors
                .push("MAIN.EXE missing: no palette, sprites use a grey ramp".into());
            return Ok(None);
        }
    };
    let file = PaletteFile {
        source: "MAIN.EXE",
        offset: format!("{:#x}", bank.offset),
        order: "9 slots × 16 colours, #rrggbb (stored as 4-bit B,R,G; expanded like the VGA DAC)",
        slots: bank
            .slots
            .iter()
            .map(|slot| {
                slot.iter()
                    .map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
                    .collect()
            })
            .collect(),
        slot_notes: palette::SLOT_NOTES.to_vec(),
        note: PALETTE_NOTE,
    };
    out.write_json(&format!("{GFX_DIR}/palettes.json"), &file)?;
    report.outputs += 1;
    report.notes.push(format!(
        "palette: MAIN.EXE bank at {:#x}; slot per archive in {GFX_DIR}/sprites.json, all slots \
         in {GFX_DIR}/palettes.json",
        bank.offset
    ));
    Ok(Some(bank.slots))
}

#[derive(Serialize)]
struct SpritesFile {
    note: &'static str,
    archives: Vec<ArchiveInfo>,
}

#[derive(Serialize)]
struct ArchiveInfo {
    file: &'static str,
    what: &'static str,
    /// Default slot; entries that need another one list their own.
    palette_slot: usize,
    sheets: Vec<String>,
    entries: Vec<EntryInfo>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    groups: &'static [Group],
}

#[derive(Serialize)]
struct EntryInfo {
    index: usize,
    bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrangement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<[usize; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    palette_slot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped: Option<String>,
}

const SPRITES_NOTE: &str = "Entries of 16×16 planar cells (plane p = colour bit p) composed in \
    row-major cell order, or packed planar images; only the stored facing exists (the game \
    mirrors the other one). Colour 0 is transparent. Groups were identified by looking at the \
    images; class groups follow the game's class order.";

/// Convert one archive with the palette bank (`None`: grey ramp). `Err` = the container
/// itself is invalid.
fn convert_archive(
    spec: &SpriteArchive,
    data: &[u8],
    bank: Option<&[Palette16; palette::SLOTS]>,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<Result<ArchiveInfo, String>, ExtractError> {
    let palette_of = |slot: usize| bank.map_or_else(grey_ramp, |b| b[slot]);
    let entries = match ls11::Archive::parse(data).and_then(|a| a.decode_all()) {
        Ok(entries) => entries,
        Err(e) => return Ok(Err(e.to_string())),
    };
    let stem = spec.file.trim_end_matches(".R3").to_lowercase();
    // Images per palette slot, in entry order: one contact sheet per slot.
    let mut images: BTreeMap<usize, Vec<IndexedImage>> = BTreeMap::new();
    let mut infos = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let mut info = EntryInfo {
            index: i,
            bytes: entry.len(),
            key: None,
            arrangement: None,
            size: None,
            palette_slot: None,
            skipped: None,
        };
        let Some(arrangement) = (spec.arrangement)(i, entry.len()) else {
            info.skipped = Some("not an image of a known geometry".into());
            infos.push(info);
            continue;
        };
        let slot = spec.slot_for(i);
        let image = sprites::decode_entry(entry, arrangement).map_err(|e| e.to_string());
        let png = image.and_then(|image| {
            encode_png(&image, &palette_of(slot), true)
                .map(|png| (image, png))
                .map_err(|e| e.to_string())
        });
        match png {
            Ok((image, png)) => {
                out.write(&format!("{GFX_DIR}/{stem}/{i:03}.png"), &png)?;
                report.outputs += 1;
                info.key = Some(format!("original/{stem}/{i:03}"));
                info.arrangement = Some(arrangement.describe());
                info.size = Some([image.width, image.height]);
                info.palette_slot = Some(slot);
                images.entry(slot).or_default().push(image);
            }
            Err(e) => {
                report.errors.push(format!("{} entry {i}: {e}", spec.file));
                info.skipped = Some(e);
            }
        }
        infos.push(info);
    }
    let mut sheets = Vec::new();
    for (&slot, images) in &images {
        let sheet = if slot == spec.slot {
            format!("{GFX_DIR}/sheets/{stem}.png")
        } else {
            format!("{GFX_DIR}/sheets/{stem}-slot{slot}.png")
        };
        let png = encode_png(
            &sprites::contact_sheet(images, SHEET_COLUMNS.min(images.len()), 2),
            &palette_of(slot),
            true,
        )
        .map_err(|e| output_error(&out.root.join(&sheet), e))?;
        out.write(&sheet, &png)?;
        report.outputs += 1;
        sheets.push(sheet);
    }
    Ok(Ok(ArchiveInfo {
        file: spec.file,
        what: spec.what,
        palette_slot: spec.slot,
        sheets,
        entries: infos,
        groups: spec.groups,
    }))
}

fn extract_sprites(
    install: &InstallDir,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let (mut found, mut failed) = (0, 0);
    let mut bank: Option<Option<[Palette16; palette::SLOTS]>> = None;
    let mut archives = Vec::new();
    let mut skipped = Vec::new();
    for spec in &sprites::ARCHIVES {
        let Some(data) = read_source(install, spec.file, &mut report)? else {
            continue;
        };
        found += 1;
        let slots = match bank {
            Some(b) => b,
            None => *bank.insert(palette_bank(install, out, &mut report)?),
        };
        let written = report.outputs;
        match convert_archive(spec, &data, slots.as_ref(), out, &mut report)? {
            Ok(info) => {
                skipped.extend(
                    info.entries
                        .iter()
                        .filter(|e| e.skipped.is_some())
                        .map(|e| format!("{} entry {} ({} bytes)", spec.file, e.index, e.bytes)),
                );
                archives.push(info);
            }
            Err(e) => {
                failed += 1;
                debug_assert_eq!(written, report.outputs);
                report
                    .errors
                    .push(format!("{} ({}): {e}", spec.file, spec.what));
            }
        }
    }
    let images = archives
        .iter()
        .flat_map(|a| &a.entries)
        .filter(|e| e.key.is_some())
        .count();
    if !archives.is_empty() {
        out.write_json(
            &format!("{GFX_DIR}/sprites.json"),
            &SpritesFile {
                note: SPRITES_NOTE,
                archives,
            },
        )?;
        report.outputs += 1;
        report.notes.push(format!(
            "geometry, palette slot and entry groups of every archive: {GFX_DIR}/sprites.json; \
             contact sheets in {GFX_DIR}/sheets/"
        ));
    }
    if !skipped.is_empty() {
        let shown: Vec<&str> = skipped.iter().take(10).map(String::as_str).collect();
        report.notes.push(format!(
            "{} entries are not images of a known geometry and were skipped: {}{}",
            skipped.len(),
            shown.join(", "),
            if skipped.len() > shown.len() {
                ", …"
            } else {
                ""
            }
        ));
    }
    report.settle(found, failed);
    if report.status == Status::Extracted && !report.errors.is_empty() {
        report.status = Status::Partial; // converted, but without the original palette
    }
    report.summary = if found == 0 {
        "no sprite or chip archives (HEX?CHR.R3, HEX?CHP.R3, HEXGRP.R3, *BGPL.R3) in the install"
            .into()
    } else {
        format!(
            "{images} images from {} of {found} archives",
            found - failed
        )
    };
    Ok(report)
}

// ----- portraits -----------------------------------------------------------------------------

/// Folder of the portrait PNGs inside [`GFX_DIR`] (media keys `original/facedat/<nnn>`).
pub const PORTRAIT_DIR: &str = "facedat";
/// Palette slot used for the portrait PNGs. The portraits only use colours 0–7, which are the
/// same in every slot except 4 (the digital 8-colour slot), so any other slot gives the same
/// image.
pub const PORTRAIT_PALETTE_SLOT: usize = 0;

/// The portrait palette: slot [`PORTRAIT_PALETTE_SLOT`] of `MAIN.EXE`'s bank; `None` (with an
/// error in the report) when the bank is missing.
fn portrait_palette(
    install: &InstallDir,
    report: &mut KindReport,
) -> Result<Option<Palette16>, ExtractError> {
    let exe = read_source(install, "MAIN.EXE", report)?;
    Ok(match exe.as_deref().map(palette::find_bank) {
        Some(Ok(bank)) => {
            report.notes.push(format!(
                "palette: MAIN.EXE bank at {:#x}, slot {PORTRAIT_PALETTE_SLOT}",
                bank.offset
            ));
            Some(bank.slots[PORTRAIT_PALETTE_SLOT])
        }
        Some(Err(e)) => {
            report
                .errors
                .push(format!("MAIN.EXE: {e}; portraits use a grey ramp"));
            None
        }
        None => {
            report
                .errors
                .push("MAIN.EXE missing: no palette, portraits use a grey ramp".into());
            None
        }
    })
}

/// One portrait as a PNG.
fn portrait_png(payload: &[u8], pal: &Palette16) -> Result<Vec<u8>, String> {
    let image = crate::tfdce::decode(payload).map_err(|e| e.to_string())?;
    let indexed = crate::planar::decode(&image.planar, image.width, image.height)
        .map_err(|e| e.to_string())?;
    encode_png(&indexed, pal, false).map_err(|e| e.to_string())
}

/// Decode every TF-DCE portrait of `FACEDAT.R3` into `gfx/original/facedat/<nnn>.png`.
fn extract_portraits(
    install: &InstallDir,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let Some(data) = read_source(install, PORTRAIT_SOURCE, &mut report)? else {
        report.status = Status::MissingSource;
        report.summary = format!("{PORTRAIT_SOURCE} not in the install");
        return Ok(report);
    };
    let table = match table6::Table6::parse(&data) {
        Ok(table) => table,
        Err(e) => {
            report.status = Status::Failed;
            report.summary = format!("{PORTRAIT_SOURCE}: invalid container");
            report.errors.push(format!("{PORTRAIT_SOURCE}: {e}"));
            return Ok(report);
        }
    };
    let found_palette = portrait_palette(install, &mut report)?;
    let pal = found_palette.unwrap_or_else(grey_ramp);
    let mut written = 0;
    for i in 0..table.len() {
        let payload = table.get(i).unwrap_or_default();
        match portrait_png(payload, &pal) {
            Ok(png) => {
                out.write(&format!("{GFX_DIR}/{PORTRAIT_DIR}/{i:03}.png"), &png)?;
                written += 1;
                report.outputs += 1;
            }
            Err(e) => report
                .errors
                .push(format!("{PORTRAIT_SOURCE} entry {i}: {e}")),
        }
    }
    report.status = if written == 0 {
        Status::Failed
    } else if written < table.len() || found_palette.is_none() {
        Status::Partial
    } else {
        Status::Extracted
    };
    report.summary = format!(
        "{written} of {} portraits from {PORTRAIT_SOURCE} (TF-DCE)",
        table.len()
    );
    report.notes.push(
        "each image keeps the size its header gives (64×80 in the Korean build); colour index 0 \
         is opaque"
            .into(),
    );
    Ok(report)
}

// ----- maps ----------------------------------------------------------------------------------

/// Folder of the map data (JSON) in the output.
pub const MAPS_DIR: &str = "maps";
/// Folder of the map images inside [`GFX_DIR`].
pub const MAP_GFX_DIR: &str = "maps";
/// Palette slot of the battle maps, battle-scene strips and campaign maps (the green field slot;
/// the game picks the slot per scenario at run time).
pub const MAP_PALETTE_SLOT: usize = 1;
/// Palette slots of the town (`SMAP`, slot 0) and palace (`PMAP`, slot 2) screens.
pub const TOWN_PALETTE_SLOTS: [usize; 2] = [0, 2];

const BATTLE_NOTE: &str = "Battle maps of HEXZMAP.R3. `chips` are rows of 16-px chip indices \
    into a bank of HEXZCHP entry 0 (80 cells, indices 0–79) followed by entry `chip_set` \
    (indices 80–255); the chip sheets gfx/original/maps/battle/chips-<set>.png show a bank in \
    index order, 16 per row. `terrain` are rows of terrain codes, one per 2×2-chip cell (the \
    32-px grid units move on); `terrain_table` names the codes. `chip_terrain` counts, for every \
    chip, the terrain codes of the cells it is drawn in over all maps (a guide for mapping chips \
    to terrain; the map's own terrain grid is authoritative).";

const SCENE_NOTE: &str = "HEXBMAP.R3: battle-scene strips drawn with the cells of HEXBCHP.R3 \
    entry 0; backdrops (sky and horizon, 46×5 cells) and grounds (66×8 cells). \
    battle.json's terrain_table gives the strips the game picks for the terrain of the \
    fighting unit's cell.";

const CAMPAIGN_NOTE: &str = "MMAP.R3: campaign maps drawn with MMAPBGPL.R3 entry 0; the size \
    of each entry is the one of MAIN.EXE's per-chapter table that matches its length. `routes` \
    marks the 32-px cells of the road network the army marches along.";

const TOWN_NOTE: &str = "SMAP.R3 (towns, SMAPBGPL entry 0) and PMAP.R3 (palaces, SMAPBGPL \
    entry 1): 32×20 tiles; `walk` is the 31×20 walk grid whose point (x, y) sits at pixel \
    (16x + 16, 16y + 8); `markers` lists (x, y, value) of the marked points; `objects` are the \
    entry's (id, x, y) triples, whose meaning is not established.";

#[derive(Serialize)]
struct BattleIndex {
    note: &'static str,
    palette_slot: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    second_set_maps: Option<Vec<u16>>,
    terrain_table: Vec<TerrainInfo>,
    maps: Vec<BattleSummary>,
    chip_terrain: Vec<ChipInfo>,
}

#[derive(Serialize)]
struct TerrainInfo {
    code: usize,
    id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene_backdrop: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene_ground: Option<u8>,
}

#[derive(Serialize)]
struct BattleSummary {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    size_chips: [usize; 2],
    size_cells: [usize; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    chip_set: Option<usize>,
    data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
}

#[derive(Serialize)]
struct BattleFile {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// The whole line of the name list (the game shows only the leading Hangul / Hanzi).
    #[serde(skip_serializing_if = "Option::is_none")]
    name_line: Option<String>,
    size_chips: [usize; 2],
    size_cells: [usize; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    chip_set: Option<usize>,
    chips: Vec<Vec<u8>>,
    terrain: Vec<Vec<u8>>,
}

#[derive(Serialize)]
struct ChipInfo {
    set: usize,
    index: usize,
    /// Index in the bank (what the maps store).
    chip: usize,
    uses: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrain: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    share: Option<f64>,
}

#[derive(Serialize)]
struct SceneIndex {
    note: &'static str,
    palette_slot: usize,
    strips: Vec<SceneInfo>,
}

#[derive(Serialize)]
struct SceneInfo {
    index: usize,
    kind: &'static str,
    size_cells: [usize; 2],
    image: String,
}

#[derive(Serialize)]
struct CampaignIndex {
    note: &'static str,
    palette_slot: usize,
    sizes_by_chapter: Vec<[usize; 2]>,
    maps: Vec<CampaignFile>,
}

#[derive(Serialize)]
struct CampaignFile {
    index: usize,
    size_tiles: [usize; 2],
    image: String,
    tiles: Vec<Vec<u8>>,
    /// Rows of `W/2` characters, `#` = route cell.
    routes: Vec<String>,
}

#[derive(Serialize)]
struct TownIndex {
    note: &'static str,
    maps: Vec<TownFile>,
}

#[derive(Serialize)]
struct TownFile {
    source: &'static str,
    index: usize,
    chips: &'static str,
    palette_slot: usize,
    image: String,
    tiles: Vec<Vec<u8>>,
    /// Rows of 31 characters: `.` blocked, `#` walkable, `*` walkable marked point (the value
    /// is in `markers`).
    walk: Vec<String>,
    markers: Vec<[u8; 3]>,
    objects: Vec<[u8; 3]>,
}

fn rows<T: Copy>(data: &[T], width: usize) -> Vec<Vec<T>> {
    data.chunks(width.max(1)).map(<[T]>::to_vec).collect()
}

/// The result of one map step: `None` when its source is not in the install, otherwise
/// whether it could be converted (per-entry problems go to the report's errors).
type MapStep = Option<Result<(), String>>;

/// The decoded entries of an archive, or why they could not be decoded.
type ArchiveEntries = Result<Vec<Vec<u8>>, String>;

/// Decode an LS11 archive of the install; `Ok(None)` when the file is missing.
fn map_archive(
    install: &InstallDir,
    name: &str,
    report: &mut KindReport,
) -> Result<Option<ArchiveEntries>, ExtractError> {
    let Some(data) = read_source(install, name, report)? else {
        return Ok(None);
    };
    Ok(Some(
        ls11::Archive::parse(&data)
            .and_then(|a| a.decode_all())
            .map_err(|e| format!("{name}: {e}")),
    ))
}

/// Entry 0 of a single-bank chip archive, or why it is not available.
fn first_bank(
    install: &InstallDir,
    name: &str,
    report: &mut KindReport,
) -> Result<Result<Vec<u8>, String>, ExtractError> {
    Ok(match map_archive(install, name, report)? {
        Some(Ok(mut e)) if !e.is_empty() => Ok(e.swap_remove(0)),
        Some(Ok(_)) => Err(format!("{name} has no entry")),
        Some(Err(e)) => Err(e),
        None => Err(format!("{name} missing")),
    })
}

fn write_map_png(
    out: &mut Output,
    report: &mut KindReport,
    rel: &str,
    image: &IndexedImage,
    pal: &Palette16,
) -> Result<(), ExtractError> {
    let png = encode_png(image, pal, false).map_err(|e| output_error(&out.root.join(rel), e))?;
    out.write(rel, &png)?;
    report.outputs += 1;
    Ok(())
}

struct MapContext<'a> {
    install: &'a InstallDir,
    encoding: TextEncoding,
    bank: Option<[Palette16; palette::SLOTS]>,
    tables: Option<maps::ExeTables>,
}

impl MapContext<'_> {
    fn palette(&self, slot: usize) -> Palette16 {
        self.bank.map_or_else(grey_ramp, |b| b[slot])
    }

    fn decode(&self, raw: &[u8]) -> String {
        self.encoding.decode(raw).text.trim().to_string()
    }
}

/// Chip sheets of the two battle banks; returns the banks that could be built.
fn battle_banks(
    ctx: &MapContext,
    chipsets: &[Vec<u8>],
    out: &mut Output,
    report: &mut KindReport,
) -> Result<BTreeMap<usize, Vec<u8>>, ExtractError> {
    let pal = ctx.palette(MAP_PALETTE_SLOT);
    let mut banks = BTreeMap::new();
    for set in [1, 2] {
        let bank = match maps::battle_bank(chipsets, set) {
            Ok(bank) => bank,
            Err(e) => {
                report.errors.push(format!("HEXZCHP.R3 set {set}: {e}"));
                continue;
            }
        };
        let layout = crate::planar::CellLayout::Sheet {
            columns: SHEET_COLUMNS,
        };
        match crate::planar::cells_to_image(&bank, layout) {
            Ok(image) => write_map_png(
                out,
                report,
                &format!("{GFX_DIR}/{MAP_GFX_DIR}/battle/chips-{set}.png"),
                &image,
                &pal,
            )?,
            Err(e) => report.errors.push(format!("HEXZCHP.R3 set {set}: {e}")),
        }
        banks.insert(set, bank);
    }
    Ok(banks)
}

/// Every chip of the three `HEXZCHP` entries with the terrain it is mostly drawn on.
fn chip_terrain(
    tables: &maps::ExeTables,
    chipsets: &[Vec<u8>],
    battle: &[(usize, maps::BattleMap)],
) -> Vec<ChipInfo> {
    if chipsets.len() < 3 {
        return Vec::new();
    }
    let sizes = [0, 1, 2].map(|s| chipsets[s].len() / crate::planar::CELL_BYTES);
    let pairs: Vec<(&maps::BattleMap, usize)> = battle
        .iter()
        .map(|(i, m)| (m, tables.chip_set_for(*i)))
        .collect();
    maps::chip_uses(&pairs, sizes)
        .iter()
        .map(|u| {
            let dominant = u.dominant();
            ChipInfo {
                set: u.set,
                index: u.index,
                chip: if u.set == 0 {
                    u.index
                } else {
                    maps::COMMON_CHIPS + u.index
                },
                uses: u.uses(),
                terrain: dominant.map(|(c, _)| maps::TERRAIN_IDS[c]),
                code: dominant.map(|(c, _)| c),
                share: dominant.map(|(_, s)| (s * 1000.0).round() / 1000.0),
            }
        })
        .collect()
}

/// Battle maps (HEXZMAP), their chip banks and the chip → terrain statistics.
fn extract_battle_maps(
    ctx: &MapContext,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<MapStep, ExtractError> {
    let entries = match map_archive(ctx.install, "HEXZMAP.R3", report)? {
        None => return Ok(None),
        Some(Err(e)) => return Ok(Some(Err(e))),
        Some(Ok(e)) => e,
    };
    let mut battle = Vec::new();
    let mut names = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match maps::BattleMap::parse(entry) {
            Ok(map) => battle.push((i, map)),
            // The last entry is the name list.
            Err(_) if i + 1 == entries.len() && !battle.is_empty() => {
                names = maps::parse_map_names(entry);
            }
            Err(e) => report.errors.push(format!("HEXZMAP.R3 entry {i}: {e}")),
        }
    }
    if battle.is_empty() {
        return Ok(Some(Err("HEXZMAP.R3 holds no battle map".into())));
    }
    if names.len() != battle.len() {
        report.notes.push(format!(
            "HEXZMAP.R3: {} names for {} maps; names are matched by position",
            names.len(),
            battle.len()
        ));
    }

    let chipsets = match map_archive(ctx.install, "HEXZCHP.R3", report)? {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            report.errors.push(e);
            Vec::new()
        }
        None => {
            report
                .errors
                .push("HEXZCHP.R3 missing: battle maps are written without images".into());
            Vec::new()
        }
    };
    let banks = if chipsets.is_empty() {
        BTreeMap::new()
    } else {
        battle_banks(ctx, &chipsets, out, report)?
    };
    let pal = ctx.palette(MAP_PALETTE_SLOT);
    let tables = ctx.tables.as_ref();
    let mut summaries = Vec::new();
    for (k, (i, map)) in battle.iter().enumerate() {
        let (cw, ch) = map.cells();
        let chip_set = tables.map(|t| t.chip_set_for(*i));
        let mut image = None;
        if let Some(bank) = chip_set.and_then(|s| banks.get(&s)) {
            match maps::render_tiles(&map.chips, map.width, map.height, bank) {
                Ok(picture) => {
                    let rel = format!("{GFX_DIR}/{MAP_GFX_DIR}/battle/{i:03}.png");
                    write_map_png(out, report, &rel, &picture, &pal)?;
                    image = Some(rel);
                }
                Err(e) => report.errors.push(format!("HEXZMAP.R3 map {i}: {e}")),
            }
        }
        let raw = names.get(k);
        let name = raw.map(|r| ctx.decode(maps::display_name(r)));
        let data = format!("{MAPS_DIR}/battle/{i:03}.json");
        out.write_json(
            &data,
            &BattleFile {
                index: *i,
                name: name.clone(),
                name_line: raw.map(|r| ctx.decode(r)),
                size_chips: [map.width, map.height],
                size_cells: [cw, ch],
                chip_set,
                chips: rows(&map.chips, map.width),
                terrain: rows(&map.terrain, cw),
            },
        )?;
        report.outputs += 1;
        summaries.push(BattleSummary {
            index: *i,
            name,
            size_chips: [map.width, map.height],
            size_cells: [cw, ch],
            chip_set,
            data,
            image,
        });
    }

    let unknown: usize = battle
        .iter()
        .flat_map(|(_, m)| &m.terrain)
        .filter(|&&c| usize::from(c) >= maps::TERRAIN_COUNT)
        .count();
    if unknown > 0 {
        report.notes.push(format!(
            "{unknown} battle-map cells carry a terrain code ≥ {} (kept as stored)",
            maps::TERRAIN_COUNT
        ));
    }
    let terrain_table = (0..maps::TERRAIN_COUNT)
        .map(|code| TerrainInfo {
            code,
            id: maps::TERRAIN_IDS[code],
            name: tables
                .and_then(|t| t.terrain_names.as_ref())
                .and_then(|n| n.get(code))
                .map(|raw| ctx.decode(raw)),
            scene_backdrop: tables.and_then(|t| t.backdrop.get(code).copied()),
            scene_ground: tables.and_then(|t| t.ground.get(code).copied()),
        })
        .collect();
    out.write_json(
        &format!("{MAPS_DIR}/battle.json"),
        &BattleIndex {
            note: BATTLE_NOTE,
            palette_slot: MAP_PALETTE_SLOT,
            second_set_maps: tables.map(|t| t.second_set_maps.clone()),
            terrain_table,
            maps: summaries,
            chip_terrain: tables.map_or_else(Vec::new, |t| chip_terrain(t, &chipsets, &battle)),
        },
    )?;
    report.outputs += 1;
    report.notes.push(format!(
        "{} battle maps: data in {MAPS_DIR}/battle/, images in {GFX_DIR}/{MAP_GFX_DIR}/battle/, \
         index and terrain table in {MAPS_DIR}/battle.json",
        battle.len()
    ));
    Ok(Some(Ok(())))
}

/// Battle-scene strips (HEXBMAP with HEXBCHP).
fn extract_scene_strips(
    ctx: &MapContext,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<MapStep, ExtractError> {
    let entries = match map_archive(ctx.install, "HEXBMAP.R3", report)? {
        None => return Ok(None),
        Some(Err(e)) => return Ok(Some(Err(e))),
        Some(Ok(e)) => e,
    };
    let bank = match first_bank(ctx.install, "HEXBCHP.R3", report)? {
        Ok(bank) => bank,
        Err(e) => return Ok(Some(Err(e))),
    };
    let pal = ctx.palette(MAP_PALETTE_SLOT);
    let mut strips = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let Some(kind) = maps::SceneStrip::of(entry) else {
            report.errors.push(format!(
                "HEXBMAP.R3 entry {i}: {} bytes is neither a backdrop nor a ground strip",
                entry.len()
            ));
            continue;
        };
        let (w, h) = kind.cells();
        match maps::render_tiles(entry, w, h, &bank) {
            Ok(image) => {
                let rel = format!("{GFX_DIR}/{MAP_GFX_DIR}/scene/{i:03}.png");
                write_map_png(out, report, &rel, &image, &pal)?;
                strips.push(SceneInfo {
                    index: i,
                    kind: kind.name(),
                    size_cells: [w, h],
                    image: rel,
                });
            }
            Err(e) => report.errors.push(format!("HEXBMAP.R3 entry {i}: {e}")),
        }
    }
    out.write_json(
        &format!("{MAPS_DIR}/scene.json"),
        &SceneIndex {
            note: SCENE_NOTE,
            palette_slot: MAP_PALETTE_SLOT,
            strips,
        },
    )?;
    report.outputs += 1;
    Ok(Some(Ok(())))
}

/// Campaign maps (MMAP with MMAPBGPL, sizes from MAIN.EXE).
fn extract_campaign_maps(
    ctx: &MapContext,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<MapStep, ExtractError> {
    let entries = match map_archive(ctx.install, "MMAP.R3", report)? {
        None => return Ok(None),
        Some(Err(e)) => return Ok(Some(Err(e))),
        Some(Ok(e)) => e,
    };
    let Some(tables) = &ctx.tables else {
        return Ok(Some(Err(
            "MMAP.R3: the map sizes come from MAIN.EXE's table, which was not found".into(),
        )));
    };
    let bank = match first_bank(ctx.install, "MMAPBGPL.R3", report)? {
        Ok(bank) => bank,
        Err(e) => return Ok(Some(Err(e))),
    };
    let pal = ctx.palette(MAP_PALETTE_SLOT);
    let mut files = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let drawn = maps::CampaignMap::parse(entry, &tables.campaign_sizes).and_then(|map| {
            maps::render_tiles(&map.tiles, map.width, map.height, &bank).map(|image| (map, image))
        });
        let (map, image) = match drawn {
            Ok(v) => v,
            Err(e) => {
                report.errors.push(format!("MMAP.R3 entry {i}: {e}"));
                continue;
            }
        };
        let rel = format!("{GFX_DIR}/{MAP_GFX_DIR}/campaign/{i:03}.png");
        write_map_png(out, report, &rel, &image, &pal)?;
        files.push(CampaignFile {
            index: i,
            size_tiles: [map.width, map.height],
            image: rel,
            tiles: rows(&map.tiles, map.width),
            routes: map
                .routes
                .chunks(map.width / 2)
                .map(|r| r.iter().map(|&on| if on { '#' } else { '.' }).collect())
                .collect(),
        });
    }
    out.write_json(
        &format!("{MAPS_DIR}/campaign.json"),
        &CampaignIndex {
            note: CAMPAIGN_NOTE,
            palette_slot: MAP_PALETTE_SLOT,
            sizes_by_chapter: tables.campaign_sizes.iter().map(|&(w, h)| [w, h]).collect(),
            maps: files,
        },
    )?;
    report.outputs += 1;
    Ok(Some(Ok(())))
}

fn town_file(
    source: &'static str,
    index: usize,
    set: usize,
    image: String,
    town: maps::TownMap,
) -> TownFile {
    let ww = maps::TOWN_WALK.0;
    TownFile {
        source,
        index,
        chips: if set == 0 {
            "SMAPBGPL.R3 entry 0"
        } else {
            "SMAPBGPL.R3 entry 1"
        },
        palette_slot: TOWN_PALETTE_SLOTS[set],
        image,
        tiles: rows(&town.tiles, maps::TOWN_TILES.0),
        walk: town
            .walk
            .chunks(ww)
            .map(|r| {
                r.iter()
                    .map(|&v| match v {
                        maps::WALK_BLOCKED => '.',
                        maps::WALK_OPEN => '#',
                        _ => '*',
                    })
                    .collect()
            })
            .collect(),
        markers: town
            .walk
            .iter()
            .enumerate()
            .filter(|&(_, &v)| v != maps::WALK_BLOCKED && v != maps::WALK_OPEN)
            .map(|(p, &v)| [(p % ww) as u8, (p / ww) as u8, v])
            .collect(),
        objects: town.objects,
    }
}

/// Town and palace screens (SMAP / PMAP with SMAPBGPL entries 0 / 1).
fn extract_town_maps(
    ctx: &MapContext,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<MapStep, ExtractError> {
    let mut archives = Vec::new();
    for (set, name, stem) in [(0, "SMAP.R3", "smap"), (1, "PMAP.R3", "pmap")] {
        if let Some(entries) = map_archive(ctx.install, name, report)? {
            archives.push((set, name, stem, entries));
        }
    }
    if archives.is_empty() {
        return Ok(None);
    }
    let banks = match map_archive(ctx.install, "SMAPBGPL.R3", report)? {
        Some(Ok(e)) if e.len() >= 2 => e,
        Some(Ok(e)) => {
            return Ok(Some(Err(format!(
                "SMAPBGPL.R3 has {} entries, expected 2",
                e.len()
            ))))
        }
        Some(Err(e)) => return Ok(Some(Err(e))),
        None => return Ok(Some(Err("SMAPBGPL.R3 missing".into()))),
    };
    let mut files = Vec::new();
    for (set, name, stem, entries) in archives {
        let entries = match entries {
            Ok(e) => e,
            Err(e) => {
                report.errors.push(e);
                continue;
            }
        };
        let pal = ctx.palette(TOWN_PALETTE_SLOTS[set]);
        let (w, h) = maps::TOWN_TILES;
        for (i, entry) in entries.iter().enumerate() {
            let drawn = maps::TownMap::parse(entry).and_then(|town| {
                maps::render_tiles(&town.tiles, w, h, &banks[set]).map(|image| (town, image))
            });
            let (town, image) = match drawn {
                Ok(v) => v,
                Err(e) => {
                    report.errors.push(format!("{name} entry {i}: {e}"));
                    continue;
                }
            };
            let rel = format!("{GFX_DIR}/{MAP_GFX_DIR}/town/{stem}-{i:03}.png");
            write_map_png(out, report, &rel, &image, &pal)?;
            files.push(town_file(name, i, set, rel, town));
        }
    }
    out.write_json(
        &format!("{MAPS_DIR}/town.json"),
        &TownIndex {
            note: TOWN_NOTE,
            maps: files,
        },
    )?;
    report.outputs += 1;
    Ok(Some(Ok(())))
}

/// The palette bank and map tables of `MAIN.EXE` (problems go to the report).
fn map_context<'a>(
    install: &'a InstallDir,
    encoding: TextEncoding,
    report: &mut KindReport,
) -> Result<MapContext<'a>, ExtractError> {
    let exe = read_source(install, "MAIN.EXE", report)?;
    let Some(exe) = exe else {
        report.errors.push(
            "MAIN.EXE missing: no palette and no map tables; battle maps are written without \
             chip set and image"
                .into(),
        );
        return Ok(MapContext {
            install,
            encoding,
            bank: None,
            tables: None,
        });
    };
    let bank = match palette::find_bank(&exe) {
        Ok(b) => Some(b.slots),
        Err(e) => {
            report
                .errors
                .push(format!("MAIN.EXE: {e}; maps use a grey ramp"));
            None
        }
    };
    let tables = match maps::find_exe_tables(&exe) {
        Ok(t) => {
            report.notes.push(format!(
                "MAIN.EXE map tables located through the code that reads them (data segment at \
                 {:#x})",
                t.data_base
            ));
            Some(t)
        }
        Err(e) => {
            report.errors.push(format!(
                "MAIN.EXE map tables: {e}; battle maps are written without chip set and image, \
                 campaign maps not at all"
            ));
            None
        }
    };
    Ok(MapContext {
        install,
        encoding,
        bank,
        tables,
    })
}

/// Battle maps, battle-scene strips, campaign maps and town screens.
fn extract_maps(
    install: &InstallDir,
    encoding: TextEncoding,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    type Step = fn(&MapContext, &mut Output, &mut KindReport) -> Result<MapStep, ExtractError>;
    let steps: [(&str, Step); 4] = [
        ("battle maps", extract_battle_maps),
        ("battle-scene strips", extract_scene_strips),
        ("campaign maps", extract_campaign_maps),
        ("town screens", extract_town_maps),
    ];
    let sources = ["HEXZMAP.R3", "HEXBMAP.R3", "MMAP.R3", "SMAP.R3", "PMAP.R3"];
    if sources.iter().all(|s| install.path(s).is_none()) {
        report.status = Status::MissingSource;
        report.summary = format!("no map archives ({}) in the install", sources.join(", "));
        return Ok(report);
    }
    let ctx = map_context(install, encoding, &mut report)?;
    let (mut found, mut failed) = (0, 0);
    let mut done = Vec::new();
    for (what, step) in steps {
        match step(&ctx, out, &mut report)? {
            None => {}
            Some(Ok(())) => {
                found += 1;
                done.push(what);
            }
            Some(Err(e)) => {
                found += 1;
                failed += 1;
                report.errors.push(e);
            }
        }
    }
    report.settle(found, failed);
    if report.status == Status::Extracted && !report.errors.is_empty() {
        report.status = Status::Partial;
    }
    report.summary = format!("{} ({} files)", done.join(", "), report.outputs);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{self, TempDir};

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn korean() -> TempDir {
        let dir = TempDir::new("ex-src");
        testutil::write_korean_install(dir.path());
        dir
    }

    #[test]
    fn extracts_a_synthetic_korean_install() {
        let src = korean();
        let out = TempDir::new("ex-out");
        let target = out.path().join("overlay");
        let index = extract(src.path(), &target, &Options::default()).unwrap();
        assert!(index.success(), "{index:#?}");
        assert_eq!(index.edition.id, EditionId::KoreanDos);

        // Text: SNR0M (raw) and SNR1M (LS11-wrapped) and IPPAN0M.
        let text = &index.assets["text"];
        assert_eq!(text.status, Status::Extracted, "{text:#?}");
        assert_eq!(text.outputs, 3);
        let snr0m = read_json(&target.join("text/snr0m.json"));
        assert_eq!(snr0m["encoding"], "EUC-KR (cp949)");
        let blocks = &snr0m["sections"][0]["blocks"];
        assert!(blocks[0]["text"].as_str().unwrap().starts_with("유비는"));
        assert_eq!(blocks[1]["text"], "\u{5}");
        assert_eq!(snr0m["sections"][0]["base"], 2);
        let snr1m = read_json(&target.join("text/snr1m.json"));
        assert_eq!(snr1m["sections"].as_array().unwrap().len(), 2);
        assert_eq!(index.assets["names"].status, Status::Unsupported);

        // Sprites: 2 composed sprites (+1 skipped entry) and a 5-cell chip sheet.
        let sprites = &index.assets["sprites"];
        assert_eq!(sprites.status, Status::Extracted, "{sprites:#?}");
        // 3 PNGs + 2 contact sheets + palettes.json + sprites.json
        assert_eq!(sprites.outputs, 7);
        let decoder = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(target.join("gfx/original/hexbchr/000.png")).unwrap(),
        ));
        let info = decoder.read_info().unwrap().info().clone();
        assert_eq!((info.width, info.height), (64, 64));
        // HEXBCHR uses slot 1: colour 1 of the fixture palette is R 1, G 14, B 1.
        let pal = info.palette.unwrap();
        assert_eq!(
            &pal[3..6],
            &[
                palette::expand_channel(1),
                palette::expand_channel(14),
                palette::expand_channel(1)
            ]
        );
        let chips = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(target.join("gfx/original/hexzchp/000.png")).unwrap(),
        ))
        .read_info()
        .unwrap()
        .info()
        .clone();
        assert_eq!((chips.width, chips.height), (80, 16));
        assert!(sprites
            .notes
            .iter()
            .any(|n| n.contains("HEXBCHR.R3 entry 2")));
        let palettes = read_json(&target.join("gfx/original/palettes.json"));
        assert_eq!(palettes["offset"], "0xbba");
        assert_eq!(palettes["slots"].as_array().unwrap().len(), 9);
        assert_eq!(palettes["slot_notes"].as_array().unwrap().len(), 9);
        let info = read_json(&target.join("gfx/original/sprites.json"));
        let hexbchr = &info["archives"][0];
        assert_eq!(hexbchr["file"], "HEXBCHR.R3");
        assert_eq!(hexbchr["palette_slot"], 1);
        assert_eq!(hexbchr["entries"][0]["size"], serde_json::json!([64, 64]));
        assert_eq!(hexbchr["entries"][1]["arrangement"], "3×3 cells");
        assert!(hexbchr["entries"][2]["skipped"].is_string());
        assert_eq!(
            hexbchr["groups"][0]["what"],
            "short-weapon infantry (sword)"
        );
        let sheet = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(target.join("gfx/original/sheets/hexbchr.png")).unwrap(),
        ))
        .read_info()
        .unwrap()
        .info()
        .clone();
        // Two entries of at most 64×64 side by side, 2-pixel gaps.
        assert_eq!((sheet.width, sheet.height), (2 * 66, 66));

        // Portraits: three synthetic TF-DCE images decoded to PNG.
        let portraits = &index.assets["portraits"];
        assert_eq!(portraits.status, Status::Extracted, "{portraits:#?}");
        assert_eq!(portraits.outputs, 3);
        assert!(portraits.summary.starts_with("3 of 3 portraits"));
        let face = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(target.join("gfx/original/facedat/000.png")).unwrap(),
        ));
        let mut face = face.read_info().unwrap();
        assert_eq!((face.info().width, face.info().height), (64, 80));
        let mut pixels = vec![0; face.output_buffer_size()];
        face.next_frame(&mut pixels).unwrap();
        // Fixture face 0: planes 0 and 2 set, plane 1 clear → colour 5 everywhere.
        assert!(pixels.iter().all(|&p| p == 5));
        assert!(target.join("gfx/original/facedat/002.png").is_file());

        // The index lists every file and records provenance.
        let on_disk = read_json(&target.join(INDEX_FILE));
        assert_eq!(on_disk["format"], FORMAT);
        assert_eq!(
            on_disk["files"].as_array().unwrap().len(),
            index.files.len()
        );
        for f in &index.files {
            assert!(target.join(f).is_file(), "{f}");
        }
        assert!(text.sources.iter().any(|s| s.file == "SNR0M.R3"));
    }

    #[test]
    fn explicit_kind_that_fails_fails_the_run() {
        let src = korean();
        // A portrait container whose single entry is not a TF-DCE image.
        std::fs::write(
            src.path().join("FACEDAT.R3"),
            table6::build(&[b"not an image"]).unwrap(),
        )
        .unwrap();
        let out = TempDir::new("ex-out-portraits");
        let options = Options {
            selection: Some(Selection {
                portraits: true,
                ..Selection::default()
            }),
            edition: None,
        };
        let index = extract(src.path(), out.path(), &options).unwrap();
        assert!(!index.success());
        assert_eq!(index.assets.len(), 1);
        let portraits = &index.assets["portraits"];
        assert_eq!(portraits.status, Status::Failed);
        assert!(portraits.errors[0].starts_with("FACEDAT.R3 entry 0: TF-DCE"));
        assert!(!portraits.ok());
    }

    #[test]
    fn corrupt_files_fail_their_kind_only() {
        let src = korean();
        // Break SNR1M (not a valid message table) and HEXZCHP (trailing byte).
        std::fs::write(src.path().join("SNR1M.R3"), [3u8, 0, 0]).unwrap();
        let mut chp = std::fs::read(src.path().join("HEXZCHP.R3")).unwrap();
        chp.push(0);
        std::fs::write(src.path().join("HEXZCHP.R3"), chp).unwrap();
        let out = TempDir::new("ex-out-corrupt");
        let index = extract(src.path(), out.path(), &Options::default()).unwrap();
        assert!(!index.success());
        let text = &index.assets["text"];
        assert_eq!(text.status, Status::Partial);
        assert!(text.errors[0].starts_with("SNR1M.R3:"), "{text:#?}");
        assert!(!out.path().join("text/snr1m.json").exists());
        let sprites = &index.assets["sprites"];
        assert_eq!(sprites.status, Status::Partial);
        assert!(sprites.errors[0].contains("HEXZCHP.R3"));
        assert!(!out.path().join("gfx/original/hexzchp").exists());
    }

    #[test]
    fn scene_count_mismatch_is_an_error() {
        let src = korean();
        // SNR1D with three scenes while SNR1M has two sections.
        std::fs::write(
            src.path().join("SNR1D.R3"),
            ls11::build(&[b"a1", b"b2", b"c3"]),
        )
        .unwrap();
        let out = TempDir::new("ex-out-scenes");
        let index = extract(src.path(), out.path(), &Options::default()).unwrap();
        let text = &index.assets["text"];
        assert!(
            text.errors
                .iter()
                .any(|e| e.contains("2 sections") && e.contains("3 scenes")),
            "{text:#?}"
        );
    }

    #[test]
    fn missing_palette_degrades_to_partial() {
        let src = korean();
        std::fs::remove_file(src.path().join("MAIN.EXE")).unwrap();
        std::fs::write(src.path().join("MAIN.EXE"), b"MZ no palette here").unwrap();
        let out = TempDir::new("ex-out-nopal");
        let index = extract(src.path(), out.path(), &Options::default()).unwrap();
        let sprites = &index.assets["sprites"];
        assert_eq!(sprites.status, Status::Partial);
        assert!(sprites.errors[0].contains("grey ramp"));
        assert!(out.path().join("gfx/original/hexbchr/000.png").is_file());
    }

    #[test]
    fn chinese_text_is_decoded_as_big5() {
        let src = TempDir::new("ex-zh");
        testutil::write_chinese_install(src.path());
        let out = TempDir::new("ex-out-zh");
        let index = extract(
            src.path(),
            out.path(),
            &Options {
                selection: Some(Selection {
                    text: true,
                    ..Selection::default()
                }),
                edition: None,
            },
        )
        .unwrap();
        assert!(index.success(), "{index:#?}");
        assert_eq!(index.edition.id, EditionId::ChineseDos);
        let snr0m = read_json(&out.path().join("text/snr0m.json"));
        assert_eq!(snr0m["encoding"], "Big5");
        assert!(snr0m["sections"][0]["blocks"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("劉備"));
    }

    #[test]
    fn refuses_unknown_editions_unless_forced() {
        let src = korean();
        std::fs::write(src.path().join("DISK1.R3I"), b"?").unwrap();
        let out = TempDir::new("ex-out-unknown");
        let err = extract(src.path(), out.path(), &Options::default()).unwrap_err();
        assert!(matches!(err, ExtractError::NotExtractable(_)));
        assert!(err.to_string().contains("probe"), "{err}");

        let forced = Options {
            edition: Some(EditionId::KoreanDos),
            ..Options::default()
        };
        let index = extract(src.path(), out.path(), &forced).unwrap();
        assert!(index.edition.forced);
        assert_eq!(index.edition.id, EditionId::KoreanDos);

        let bad = Options {
            edition: Some(EditionId::Steam2017),
            ..Options::default()
        };
        assert!(matches!(
            extract(src.path(), out.path(), &bad).unwrap_err(),
            ExtractError::BadForcedEdition(EditionId::Steam2017)
        ));
    }

    #[test]
    fn output_folder_rules() {
        let src = korean();
        // Inside the install: refused, nothing written.
        let inside = src.path().join("out");
        let err = extract(src.path(), &inside, &Options::default()).unwrap_err();
        assert!(
            matches!(err, ExtractError::OutputOverlapsSource { .. }),
            "{err}"
        );
        assert!(!inside.exists());
        let err = extract(src.path(), &src.path().join("a/../b"), &Options::default()).unwrap_err();
        assert!(
            matches!(err, ExtractError::OutputOverlapsSource { .. }),
            "{err}"
        );
        // The install inside the output folder (e.g. an install folder named `text`): refused
        // even when the output folder holds a previous extraction.
        let parent = TempDir::new("ex-out-parent");
        let nested = parent.path().join("text");
        std::fs::create_dir(&nested).unwrap();
        testutil::write_korean_install(&nested);
        std::fs::write(
            parent.path().join(INDEX_FILE),
            format!(r#"{{"format":"{FORMAT}","files":[]}}"#),
        )
        .unwrap();
        let err = extract(&nested, parent.path(), &Options::default()).unwrap_err();
        assert!(
            matches!(err, ExtractError::OutputOverlapsSource { .. }),
            "{err}"
        );
        assert!(!nested.join("snr0m.json").exists());

        // A folder with unrelated files: refused.
        let out = TempDir::new("ex-out-busy");
        std::fs::write(out.path().join("notes.txt"), b"mine").unwrap();
        let err = extract(src.path(), out.path(), &Options::default()).unwrap_err();
        assert!(matches!(err, ExtractError::OutputNotEmpty(_)), "{err}");

        // A previous extraction is replaced: its files are removed, other files kept.
        let out = TempDir::new("ex-out-rerun");
        extract(src.path(), out.path(), &Options::default()).unwrap();
        std::fs::write(out.path().join("mine.txt"), b"keep").unwrap();
        let text_only = Options {
            selection: Some(Selection {
                text: true,
                ..Selection::default()
            }),
            edition: None,
        };
        let index = extract(src.path(), out.path(), &text_only).unwrap();
        assert!(index.files.iter().all(|f| f.starts_with("text/")));
        assert!(!out.path().join("gfx/original/hexbchr/000.png").exists());
        assert!(out.path().join("mine.txt").is_file());

        // A tampered index with an escaping path is refused.
        let index_path = out.path().join(INDEX_FILE);
        std::fs::write(
            &index_path,
            format!(r#"{{"format":"{FORMAT}","files":["../escape.txt"]}}"#),
        )
        .unwrap();
        let err = extract(src.path(), out.path(), &Options::default()).unwrap_err();
        assert!(err.to_string().contains("unsafe path"), "{err}");
    }

    /// A Korean install with every map archive: two battle maps (the second on chip set 2)
    /// and their names, scene strips, one campaign map and one town and palace screen each.
    fn write_map_install(dir: &Path) {
        testutil::write_korean_install(dir);
        let names: Vec<Vec<u8>> = maps::TERRAIN_IDS
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        let refs: Vec<&[u8]> = names.iter().map(Vec::as_slice).collect();
        let mut exe = maps::build_exe_fixture(
            &maps::ExeFixture {
                second_set_maps: &[1],
                backdrop: [0; maps::TERRAIN_COUNT],
                ground: [5; maps::TERRAIN_COUNT],
                terrain_names: &refs,
                campaign_sizes: [(16, 4); maps::CHAPTERS],
            },
            0,
        );
        exe.extend(palette::build_bank(&testutil::palette_slots()));
        std::fs::write(dir.join("MAIN.EXE"), exe).unwrap();
        let ls11 = |name: &str, entries: &[Vec<u8>]| {
            let refs: Vec<&[u8]> = entries.iter().map(Vec::as_slice).collect();
            std::fs::write(dir.join(name), ls11::build(&refs)).unwrap();
        };
        ls11(
            "HEXZCHP.R3",
            &[testutil::cells(80), testutil::cells(2), testutil::cells(3)],
        );
        let map = |chip: u8, terrain: u8| {
            maps::BattleMap {
                width: 4,
                height: 2,
                chips: vec![0, 1, chip, chip, 2, 3, chip, chip],
                terrain: vec![1, terrain],
            }
            .encode()
        };
        ls11(
            "HEXZMAP.R3",
            &[
                map(81, 8),
                map(82, 3),
                b"\xb0\xa1\n\xb0\xa2 1\r\n\r\n\x1a".to_vec(),
            ],
        );
        ls11("HEXBCHP.R3", &[testutil::cells(4)]);
        ls11("HEXBMAP.R3", &[vec![3; 230], vec![1; 528]]);
        ls11("MMAPBGPL.R3", &[testutil::cells(2)]);
        let mut campaign = vec![1u8; 64];
        campaign.extend([0x7f, 0xff]);
        ls11("MMAP.R3", &[campaign]);
        ls11("SMAPBGPL.R3", &[testutil::cells(2), testutil::cells(3)]);
        let mut town = vec![1u8; 640];
        town.extend(vec![maps::WALK_BLOCKED; 620]);
        town[640 + 32] = maps::WALK_OPEN;
        town[640 + 33] = 9;
        town.extend([1, 7, 2, 3]);
        ls11("SMAP.R3", std::slice::from_ref(&town));
        ls11("PMAP.R3", &[town]);
    }

    #[test]
    fn extracts_maps() {
        let src = TempDir::new("maps-src");
        write_map_install(src.path());
        let out = TempDir::new("maps-out");
        let options = Options {
            selection: Some(Selection {
                maps: true,
                ..Selection::default()
            }),
            edition: None,
        };
        let index = extract(src.path(), out.path(), &options).unwrap();
        let report = &index.assets["maps"];
        assert_eq!(report.status, Status::Extracted, "{report:#?}");

        let battle = read_json(&out.path().join("maps/battle.json"));
        assert_eq!(battle["second_set_maps"], serde_json::json!([1]));
        assert_eq!(battle["maps"][0]["chip_set"], 1);
        assert_eq!(battle["maps"][1]["chip_set"], 2);
        assert_eq!(battle["maps"][0]["name"], "가");
        assert_eq!(battle["maps"][1]["name"], "각");
        assert_eq!(battle["terrain_table"][8]["id"], "village");
        assert_eq!(battle["terrain_table"][8]["name"], "village");
        assert_eq!(battle["terrain_table"][8]["scene_ground"], 5);
        // Chip 81 = set 1 index 1, drawn only in the village cell of map 0.
        let chips = battle["chip_terrain"].as_array().unwrap();
        let chip = |set: u64, index: u64| {
            chips
                .iter()
                .find(|c| c["set"] == set && c["index"] == index)
                .unwrap()
        };
        assert_eq!(chip(1, 1)["terrain"], "village");
        assert_eq!(chip(1, 1)["uses"], 4);
        assert_eq!(chip(2, 2)["terrain"], "stream");
        assert_eq!(chip(0, 0)["terrain"], "forest");
        let map1 = read_json(&out.path().join("maps/battle/001.json"));
        assert_eq!(map1["name_line"], "각 1");
        assert_eq!(map1["chips"][0], serde_json::json!([0, 1, 82, 82]));
        assert_eq!(map1["terrain"], serde_json::json!([[1, 3]]));
        let png = |rel: &str| {
            let decoder = png::Decoder::new(std::io::Cursor::new(
                std::fs::read(out.path().join(rel)).unwrap(),
            ));
            let info = decoder.read_info().unwrap().info().clone();
            (info.width, info.height)
        };
        assert_eq!(png("gfx/original/maps/battle/001.png"), (64, 32));
        assert_eq!(png("gfx/original/maps/battle/chips-2.png"), (256, 96));
        assert_eq!(png("gfx/original/maps/scene/000.png"), (46 * 16, 5 * 16));
        assert_eq!(png("gfx/original/maps/scene/001.png"), (66 * 16, 8 * 16));

        let campaign = read_json(&out.path().join("maps/campaign.json"));
        assert_eq!(
            campaign["maps"][0]["size_tiles"],
            serde_json::json!([16, 4])
        );
        assert_eq!(campaign["maps"][0]["routes"][0], "#.......");
        assert_eq!(png("gfx/original/maps/campaign/000.png"), (256, 64));

        let town = read_json(&out.path().join("maps/town.json"));
        let smap = &town["maps"][0];
        assert_eq!(smap["palette_slot"], 0);
        assert_eq!(town["maps"][1]["palette_slot"], 2);
        assert_eq!(&smap["walk"][1].as_str().unwrap()[..4], ".#*.");
        assert_eq!(smap["markers"], serde_json::json!([[2, 1, 9]]));
        assert_eq!(smap["objects"], serde_json::json!([[7, 2, 3]]));
        assert_eq!(png("gfx/original/maps/town/pmap-000.png"), (512, 320));

        // Without the tables of MAIN.EXE the grids are still written, without chip set.
        std::fs::write(src.path().join("MAIN.EXE"), b"MZ").unwrap();
        let index = extract(src.path(), out.path(), &options).unwrap();
        let report = &index.assets["maps"];
        assert_eq!(report.status, Status::Partial, "{report:#?}");
        assert!(!report.ok());
        let map1 = read_json(&out.path().join("maps/battle/001.json"));
        assert!(map1.get("chip_set").is_none());
        assert!(!out.path().join("gfx/original/maps/battle/001.png").exists());
    }

    #[test]
    fn safe_relative_paths() {
        assert!(safe_relative("text/a.json").is_some());
        assert!(safe_relative("").is_none());
        assert!(safe_relative("../x").is_none());
        assert!(safe_relative("a/../../x").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
    }
}
