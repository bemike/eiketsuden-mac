//! Conversion of an install into a **media overlay** folder.
//!
//! ```text
//! <out>/index.json                        what was extracted, from which edition and files
//! <out>/gfx/original/palettes.json        the palette bank of MAIN.EXE (9 slots × 16 colours)
//! <out>/gfx/original/<archive>/<nnn>.png  cells of the sprite / chip archives (media key
//!                                         `original/<archive>/<nnn>`)
//! <out>/gfx/original/facedat/<nnn>.png    TF-DCE portraits of FACEDAT.R3
//! <out>/text/<file>.json                  message files as UTF-8 JSON
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
use crate::image::{encode_png, grey_ramp, Palette16};
use crate::install::{lies_inside, InstallDir, InstallError};
use crate::planar::{cells_to_image, CellLayout, CELL_BYTES};
use crate::text::{parse_messages, TextEncoding};
use crate::{ls11, palette, table6};
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
/// Palette slot used for the sprite PNGs.
pub const PALETTE_SLOT: usize = 0;
/// Portrait container.
pub const PORTRAIT_SOURCE: &str = "FACEDAT.R3";

/// Archives of 16×16 planar cells, with what they hold.
pub const SPRITE_SOURCES: [(&str, &str); 7] = [
    ("HEXBCHR.R3", "battle unit sprites"),
    ("HEXICHR.R3", "battle unit sprites"),
    ("HEXZCHR.R3", "battle characters (Z series)"),
    ("HEXZCHP.R3", "battle map chips (Z series)"),
    ("HEXBCHP.R3", "battle map chips (B series)"),
    ("MMAPBGPL.R3", "campaign map background cells"),
    ("SMAPBGPL.R3", "city map background cells"),
];

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
    pub portraits: bool,
    pub sprites: bool,
    pub text: bool,
}

impl Selection {
    /// Every kind.
    pub fn all() -> Selection {
        Selection {
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
}

fn sprite_palette(
    install: &InstallDir,
    out: &mut Output,
    report: &mut KindReport,
) -> Result<Palette16, ExtractError> {
    let exe = read_source(install, "MAIN.EXE", report)?;
    let bank = match exe.as_deref().map(palette::find_bank) {
        Some(Ok(bank)) => bank,
        Some(Err(e)) => {
            report
                .errors
                .push(format!("MAIN.EXE: {e}; sprites use a grey ramp"));
            return Ok(grey_ramp());
        }
        None => {
            report
                .errors
                .push("MAIN.EXE missing: no palette, sprites use a grey ramp".into());
            return Ok(grey_ramp());
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
    };
    out.write_json(&format!("{GFX_DIR}/palettes.json"), &file)?;
    report.outputs += 1;
    report.notes.push(format!(
        "palette: MAIN.EXE bank at {:#x}, slot {PALETTE_SLOT} (which slot belongs to which screen \
         is not documented; all slots are in {GFX_DIR}/palettes.json)",
        bank.offset
    ));
    Ok(bank.slots[PALETTE_SLOT])
}

fn extract_sprites(
    install: &InstallDir,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let (mut found, mut failed, mut images) = (0, 0, 0);
    let mut not_cells = Vec::new();
    let mut palette = None;
    for (name, what) in SPRITE_SOURCES {
        let Some(data) = read_source(install, name, &mut report)? else {
            continue;
        };
        found += 1;
        let pal = match palette {
            Some(p) => p,
            None => *palette.insert(sprite_palette(install, out, &mut report)?),
        };
        let entries = match ls11::Archive::parse(&data).and_then(|a| a.decode_all()) {
            Ok(entries) => entries,
            Err(e) => {
                failed += 1;
                report.errors.push(format!("{name} ({what}): {e}"));
                continue;
            }
        };
        let stem = name.trim_end_matches(".R3").to_lowercase();
        for (i, entry) in entries.iter().enumerate() {
            if entry.is_empty() || entry.len() % CELL_BYTES != 0 {
                not_cells.push(format!("{name} entry {i} ({} bytes)", entry.len()));
                continue;
            }
            let layout = CellLayout::for_cells(entry.len() / CELL_BYTES);
            let png = cells_to_image(entry, layout)
                .map_err(|e| e.to_string())
                .and_then(|image| encode_png(&image, &pal, true).map_err(|e| e.to_string()));
            match png {
                Ok(png) => {
                    out.write(&format!("{GFX_DIR}/{stem}/{i:03}.png"), &png)?;
                    images += 1;
                    report.outputs += 1;
                }
                Err(e) => report.errors.push(format!("{name} entry {i}: {e}")),
            }
        }
    }
    if found > 0 {
        report.notes.extend([
            "bit-plane p is taken as bit p of the colour index (VGA convention; not stated in the notes)".to_string(),
            "entries of 9 / 16 / 36 cells are composed as 3×3 / 4×4 / 6×6 sprites in row-major cell \
             order (the order is not documented); other entries are 16-column cell sheets in storage order"
                .to_string(),
            "only the stored facing is exported (the engine mirrors the other one); colour index 0 is transparent".to_string(),
        ]);
    }
    if !not_cells.is_empty() {
        let shown: Vec<&str> = not_cells.iter().take(10).map(String::as_str).collect();
        report.notes.push(format!(
            "{} entries are not whole 128-byte cells and were skipped: {}{}",
            not_cells.len(),
            shown.join(", "),
            if not_cells.len() > shown.len() {
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
        "no sprite or chip archives (HEX?CHR.R3, HEX?CHP.R3, *BGPL.R3) in the install".into()
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

/// The portrait palette: slot [`PALETTE_SLOT`] of `MAIN.EXE`'s bank. The portraits only use
/// colours 0–7, which are the same in every slot but one; `None` (with an error in the
/// report) when the bank is missing.
fn portrait_palette(
    install: &InstallDir,
    report: &mut KindReport,
) -> Result<Option<Palette16>, ExtractError> {
    let exe = read_source(install, "MAIN.EXE", report)?;
    Ok(match exe.as_deref().map(palette::find_bank) {
        Some(Ok(bank)) => {
            report.notes.push(format!(
                "palette: MAIN.EXE bank at {:#x}, slot {PALETTE_SLOT}",
                bank.offset
            ));
            Some(bank.slots[PALETTE_SLOT])
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
        assert_eq!(sprites.outputs, 4); // 3 PNGs + palettes.json
        let decoder = png::Decoder::new(std::io::Cursor::new(
            std::fs::read(target.join("gfx/original/hexbchr/000.png")).unwrap(),
        ));
        let info = decoder.read_info().unwrap().info().clone();
        assert_eq!((info.width, info.height), (64, 64));
        // Slot 0 colour 1 of the fixture palette: R 1, G 14, B 0.
        let pal = info.palette.unwrap();
        assert_eq!(
            &pal[3..6],
            &[
                palette::expand_channel(1),
                palette::expand_channel(14),
                palette::expand_channel(0)
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

    #[test]
    fn safe_relative_paths() {
        assert!(safe_relative("text/a.json").is_some());
        assert!(safe_relative("").is_none());
        assert!(safe_relative("../x").is_none());
        assert!(safe_relative("a/../../x").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
    }
}
