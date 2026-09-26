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
//! <out>/text/snr<n>.json                  chapter n: decoded event scripts of every scene with
//!                                         the dialogues and strings they show (UTF-8)
//! <out>/text/snr<n>.txt                   the same as a plain-text listing
//! <out>/text/ippan0m.json                 the townspeople string pool
//! <out>/text/townsfolk_talk.json          townspeople of every town with their lines
//! <out>/text/officers.json, items.json,   BAKDATA.R3 master tables
//!            townsfolk.json
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
//! unsupported format is an explicit result rather than a guess.
//!
//! The install is only read. The output folder must not lie inside it, and must be empty, new,
//! or a previous extraction (whose listed files are replaced).

use crate::bakdata::{self, Bakdata};
use crate::edition::{identify, Edition, EditionId};
use crate::image::IndexedImage;
use crate::image::{encode_png, grey_ramp, Palette16};
use crate::install::{lies_inside, InstallDir, InstallError};
use crate::scenario::{self, ArgKind, Operands};
use crate::sprites::{self, Group, SpriteArchive};
use crate::text::{parse_messages, Section, TextEncoding};
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
    pub(crate) fn new(status: Status, requested: bool, summary: impl Into<String>) -> KindReport {
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
    pub(crate) fn settle(&mut self, found: usize, failed: usize) {
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
                "the output folder {} is not empty and holds no previous run of the importer \
                 ({INDEX_FILE} of an extraction, {} of an original pack); choose an empty or \
                 new folder (if an earlier run into it was interrupted, delete the folder and \
                 run again)",
                path.display(),
                crate::pack::PACK_INDEX
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

pub(crate) fn output_error(path: &Path, e: impl fmt::Display) -> ExtractError {
    ExtractError::Output {
        path: path.to_path_buf(),
        message: e.to_string(),
    }
}

/// Files written into the output folder.
pub(crate) struct Output {
    pub(crate) root: PathBuf,
    pub(crate) files: Vec<String>,
}

impl Output {
    pub(crate) fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<(), ExtractError> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| output_error(parent, e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| output_error(&path, e))?;
        self.files.push(rel.to_string());
        Ok(())
    }

    pub(crate) fn write_json(
        &mut self,
        rel: &str,
        value: &impl Serialize,
    ) -> Result<(), ExtractError> {
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

/// Check the output folder and remove the files of a previous run: the folder must be new,
/// empty, or hold an `index_file` whose `format` is `format` (the files it lists are removed).
pub(crate) fn prepare_output(
    source: &Path,
    out: &Path,
    index_file: &str,
    format: &str,
) -> Result<(), ExtractError> {
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
    let index_path = out.join(index_file);
    let previous: PreviousIndex = match std::fs::read(&index_path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| ExtractError::OutputNotEmpty(out.to_path_buf()))?,
        Err(_) => return Err(ExtractError::OutputNotEmpty(out.to_path_buf())),
    };
    if previous.format != format {
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
    prepare_output(source, out, INDEX_FILE, FORMAT)?;

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
        let (names, bakdata) = extract_names(&install, encoding, &mut output, requested)?;
        assets.insert("names".to_string(), names);
        assets.insert(
            "text".to_string(),
            extract_text(&install, encoding, bakdata.as_ref(), &mut output, requested)?,
        );
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

pub(crate) fn read_source(
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

/// Master tables (officer, item and townsperson names).
pub const NAMES_SOURCE: &str = "BAKDATA.R3";
/// The generic string pool.
pub const POOL_SOURCE: &str = "IPPAN0M.R3";

const SCENARIO_NOTE: &str = "Scripts decoded from the scenario bytecode; message offsets are \
    relative to the scene's section of the message file, persons are officer indices of \
    BAKDATA.R3. `resolved` repeats the referenced names and text for reading. Names marked \
    op_xx and summaries marked unverified are not pinned down yet (docs/ORIGINAL_DATA.md §10).";

const POOL_NOTE: &str = "NUL-terminated strings (the townspeople's lines, indexed by \
    IPPAN0.R3; see townsfolk_talk.json).";

#[derive(Serialize)]
struct TextBlock {
    offset: usize,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    malformed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_hex: Option<String>,
}

impl TextBlock {
    fn new(offset: usize, bytes: &[u8], encoding: TextEncoding) -> TextBlock {
        let d = encoding.decode(bytes);
        TextBlock {
            offset,
            raw_hex: d.malformed.then(|| crate::hex(bytes)),
            malformed: d.malformed,
            text: d.text,
        }
    }
}

#[derive(Serialize)]
struct PoolFile {
    source: String,
    encoding: &'static str,
    note: &'static str,
    strings: Vec<TextBlock>,
}

#[derive(Serialize)]
struct ScenarioFile {
    scenario: String,
    messages: String,
    encoding: &'static str,
    note: &'static str,
    scenes: Vec<SceneOut>,
}

#[derive(Serialize)]
struct SceneOut {
    index: usize,
    /// Absolute offset of the scene's section in the message file.
    message_base: usize,
    dialogues: Vec<DialogueOut>,
    strings: Vec<StringOut>,
    /// Section bytes no instruction refers to (none in the verified copy).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unreferenced: Vec<TextBlock>,
    blocks: Vec<BlockOut>,
}

#[derive(Serialize)]
struct DialogueOut {
    offset: usize,
    lines: Vec<LineOut>,
}

#[derive(Serialize)]
struct LineOut {
    speaker: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker_name: Option<String>,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    malformed: bool,
}

#[derive(Serialize)]
struct StringOut {
    offset: usize,
    used_by: Vec<&'static str>,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    malformed: bool,
}

#[derive(Serialize)]
struct BlockOut {
    index: usize,
    offset: usize,
    records: Vec<RecordOut>,
}

#[derive(Serialize)]
struct RecordOut {
    index: usize,
    offset: usize,
    trigger: scenario::Trigger,
    #[serde(skip_serializing_if = "Option::is_none")]
    trigger_person: Option<String>,
    code_offset: usize,
    code: Vec<InstrOut>,
}

#[derive(Serialize)]
struct InstrOut {
    #[serde(flatten)]
    instr: scenario::Instr,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    resolved: BTreeMap<&'static str, String>,
}

/// Counters for the report.
#[derive(Default)]
struct TextCounts {
    scenes: usize,
    instructions: usize,
    dialogues: usize,
    lines: usize,
    strings: usize,
    malformed: usize,
    unreferenced_bytes: usize,
}

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

struct Lookup<'a> {
    names: Option<&'a Bakdata>,
    encoding: TextEncoding,
}

impl Lookup<'_> {
    fn person(&self, id: u16) -> Option<String> {
        match id {
            0x400 => Some("(any unit)".into()),
            _ => self
                .names
                .and_then(|n| n.officer_name(id))
                .map(str::to_string),
        }
    }

    fn item(&self, id: u16) -> Option<String> {
        self.names.and_then(|n| n.item_name(id)).map(str::to_string)
    }
}

/// Readable forms of an instruction's references.
fn resolve(
    instr: &scenario::Instr,
    section: &Section<'_>,
    look: &Lookup<'_>,
) -> Result<BTreeMap<&'static str, String>, String> {
    let mut out = BTreeMap::new();
    let at = |e: crate::text::TextError| format!("instruction at {:#x}: {e}", instr.offset);
    for arg in instr.operands.args() {
        let v = arg.value;
        let text = match arg.kind {
            ArgKind::Person => look.person(v),
            ArgKind::Item => look.item(v),
            ArgKind::Message => Some(
                look.encoding
                    .decode(section.string_at(v as usize).map_err(at)?)
                    .text,
            ),
            ArgKind::Dialogue => {
                let (lines, _) = section.dialogue_at(v as usize).map_err(at)?;
                let rendered: Vec<String> = lines
                    .iter()
                    .map(|l| {
                        let who = look
                            .person(l.speaker)
                            .unwrap_or_else(|| l.speaker.to_string());
                        format!("{who}: {}", look.encoding.decode(l.text).text)
                    })
                    .collect();
                Some(rendered.join("\n"))
            }
            _ => None,
        };
        if let Some(t) = text {
            out.insert(arg.name, t);
        }
    }
    if instr.opcode == 0x0e {
        // The key's first character selects one of the titles built into MAIN.EXE.
        if let Some(k) = out.get("key").and_then(|k| k.bytes().next()) {
            out.insert(
                "title_index",
                u32::from(k).wrapping_sub(u32::from(b'0')).to_string(),
            );
        }
    }
    let roster = match &instr.operands {
        Operands::BattleSetup { units, .. } | Operands::Roster { units, .. } => Some(units),
        _ => None,
    };
    if let Some(units) = roster {
        let names: Vec<String> = units
            .iter()
            .map(|u| {
                let who = look
                    .person(u.person)
                    .unwrap_or_else(|| u.person.to_string());
                format!("{who} ({}, {})", u.x, u.y)
            })
            .collect();
        out.insert("units", names.join(", "));
    }
    Ok(out)
}

/// Decode one chapter: its scenes' scripts and the text they reference.
fn convert_scenario(
    scenario_name: &str,
    scenario_data: &[u8],
    message_name: &str,
    message_data: &[u8],
    look: &Lookup<'_>,
    counts: &mut TextCounts,
) -> Result<ScenarioFile, String> {
    let archive =
        ls11::Archive::parse(scenario_data).map_err(|e| format!("{scenario_name}: {e}"))?;
    let payload = message_payload(message_data).map_err(|e| format!("{message_name}: {e}"))?;
    let messages = parse_messages(&payload).map_err(|e| format!("{message_name}: {e}"))?;
    if messages.sections.len() != archive.len() {
        return Err(format!(
            "{message_name}: {}",
            crate::text::TextError::SectionCount {
                found: messages.sections.len(),
                expected: archive.len(),
            }
        ));
    }
    let mut scenes = Vec::with_capacity(archive.len());
    for (index, section) in messages.sections.iter().enumerate() {
        let where_ = |e: String| format!("{scenario_name} scene {index}: {e}");
        let data = archive.decode(index).map_err(|e| where_(e.to_string()))?;
        let scene = scenario::parse_scene(&data).map_err(|e| where_(e.to_string()))?;
        // Every item of the section, by offset: dialogues and strings (with their users).
        let mut dialogues: BTreeMap<usize, DialogueOut> = BTreeMap::new();
        let mut strings: BTreeMap<usize, StringOut> = BTreeMap::new();
        let mut covered = vec![false; section.bytes.len()];
        for instr in scene.instructions() {
            counts.instructions += 1;
            for arg in instr.operands.args() {
                let off = arg.value as usize;
                let err = |e: crate::text::TextError| {
                    where_(format!("{} at {:#x}: {e}", instr.mnemonic, instr.offset))
                };
                match arg.kind {
                    ArgKind::Dialogue if !dialogues.contains_key(&off) => {
                        let (lines, end) = section.dialogue_at(off).map_err(err)?;
                        covered[off..end].fill(true);
                        let lines = lines
                            .iter()
                            .map(|l| {
                                let d = look.encoding.decode(l.text);
                                LineOut {
                                    speaker: l.speaker,
                                    speaker_name: look.person(l.speaker),
                                    text: d.text,
                                    malformed: d.malformed,
                                }
                            })
                            .collect();
                        dialogues.insert(off, DialogueOut { offset: off, lines });
                    }
                    ArgKind::Message => {
                        let bytes = section.string_at(off).map_err(err)?;
                        covered[off..off + bytes.len() + 1].fill(true);
                        let entry = strings.entry(off).or_insert_with(|| {
                            let d = look.encoding.decode(bytes);
                            StringOut {
                                offset: off,
                                used_by: Vec::new(),
                                text: d.text,
                                malformed: d.malformed,
                            }
                        });
                        if !entry.used_by.contains(&instr.mnemonic) {
                            entry.used_by.push(instr.mnemonic);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut unreferenced = Vec::new();
        let mut at = 0;
        while at < covered.len() {
            if covered[at] {
                at += 1;
                continue;
            }
            let end = (at..covered.len())
                .find(|&i| covered[i])
                .unwrap_or(covered.len());
            counts.unreferenced_bytes += end - at;
            unreferenced.push(TextBlock::new(at, &section.bytes[at..end], look.encoding));
            at = end;
        }
        let mut blocks = Vec::with_capacity(scene.blocks.len());
        for (bi, block) in scene.blocks.iter().enumerate() {
            let mut records = Vec::with_capacity(block.records.len());
            for (ri, record) in block.records.iter().enumerate() {
                let code = record
                    .code
                    .iter()
                    .map(|i| {
                        Ok(InstrOut {
                            instr: i.clone(),
                            resolved: resolve(i, section, look).map_err(&where_)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let t = &record.trigger;
                let has_person = matches!(t.kind, 3 | 4 | 6 | 0x0b | 0x0c);
                records.push(RecordOut {
                    index: ri,
                    offset: record.offset,
                    trigger: t.clone(),
                    trigger_person: has_person.then(|| look.person(t.word(0))).flatten(),
                    code_offset: record.code_offset,
                    code,
                });
            }
            blocks.push(BlockOut {
                index: bi,
                offset: block.offset,
                records,
            });
        }
        counts.scenes += 1;
        counts.dialogues += dialogues.len();
        counts.lines += dialogues.values().map(|d| d.lines.len()).sum::<usize>();
        counts.strings += strings.len();
        counts.malformed += dialogues
            .values()
            .flat_map(|d| &d.lines)
            .filter(|l| l.malformed)
            .count()
            + strings.values().filter(|s| s.malformed).count();
        scenes.push(SceneOut {
            index,
            message_base: section.base,
            dialogues: dialogues.into_values().collect(),
            strings: strings.into_values().collect(),
            unreferenced,
            blocks,
        });
    }
    Ok(ScenarioFile {
        scenario: scenario_name.to_string(),
        messages: message_name.to_string(),
        encoding: look.encoding.name(),
        note: SCENARIO_NOTE,
        scenes,
    })
}

/// A plain-text listing of a decoded chapter (for reading and diffing).
fn scenario_listing(file: &ScenarioFile) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# {} + {} ({})",
        file.scenario, file.messages, file.encoding
    );
    for scene in &file.scenes {
        let _ = writeln!(
            s,
            "\n## scene {} (message section at {:#x})",
            scene.index, scene.message_base
        );
        for block in &scene.blocks {
            let _ = writeln!(s, "\n### block {} @{:#06x}", block.index, block.offset);
            for r in &block.records {
                let t = &r.trigger;
                let who = r
                    .trigger_person
                    .as_deref()
                    .map(|p| format!(" [{p}]"))
                    .unwrap_or_default();
                let _ = writeln!(
                    s,
                    "\nrecord {} {}{}({}) group {}{} args {}{} -> code {:#06x}",
                    r.index,
                    if t.inverted { "!" } else { "" },
                    t.kind_name,
                    t.kind,
                    t.group,
                    if t.group_flag { "*" } else { "" },
                    crate::hex(&t.args),
                    who,
                    r.code_offset
                );
                for i in &r.code {
                    let _ = writeln!(s, "  {:04x}  {}", i.instr.offset, instr_text(&i.instr));
                    for (k, v) in &i.resolved {
                        for (n, line) in v.lines().enumerate() {
                            let label = if n == 0 { *k } else { "" };
                            let _ = writeln!(s, "        {label:>8} │ {line}");
                        }
                    }
                }
            }
        }
    }
    s
}

fn instr_text(i: &scenario::Instr) -> String {
    let mut s = format!("{:02x} {}", i.opcode, i.mnemonic);
    match &i.operands {
        Operands::Fields { args } => {
            for a in args {
                let v = match a.kind {
                    ArgKind::Message | ArgKind::Dialogue | ArgKind::Map => {
                        format!("{:#06x}", a.value)
                    }
                    _ => a.value.to_string(),
                };
                s.push_str(&format!(" {}={v}", a.name));
            }
        }
        Operands::BattleSetup { header, units } => {
            s.push_str(&format!(
                " turns={} win={:?} lose={:?} other={} units={}",
                header.turn_limit,
                header.defeat_to_win,
                header.lose_if_defeated,
                crate::hex(&header.other),
                units.len()
            ));
        }
        Operands::Roster { friendly, units } => {
            s.push_str(&format!(" friendly={friendly} units={}", units.len()));
            for u in units {
                s.push_str(&format!(
                    "\n          unit {} at ({}, {}) class {:?} lv {:?} ai {:?}/{:?}{}",
                    u.person,
                    u.x,
                    u.y,
                    u.class.unwrap_or_default(),
                    u.level.unwrap_or_default(),
                    u.ai_mode.unwrap_or_default(),
                    u.ai_param.unwrap_or_default(),
                    u.requires_flag
                        .map(|f| format!(" if flag {f}"))
                        .unwrap_or_default()
                ));
            }
        }
        Operands::Condition {
            skip,
            all_set,
            all_clear,
        } => s.push_str(&format!(
            " unless set{all_set:?} and clear{all_clear:?} skip {skip}"
        )),
        Operands::List { reset_all, values } => {
            s.push_str(&format!(" reset_all={reset_all} {values:?}"))
        }
        Operands::Bytes { bytes } => s.push_str(&format!(" {}", crate::hex(bytes))),
    }
    s
}

fn extract_text(
    install: &InstallDir,
    encoding: TextEncoding,
    names: Option<&Bakdata>,
    out: &mut Output,
    requested: bool,
) -> Result<KindReport, ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let look = Lookup { names, encoding };
    let mut counts = TextCounts::default();
    let (mut found, mut failed) = (0, 0);
    let mut pool_strings = 0;
    if names.is_none() {
        report
            .notes
            .push("BAKDATA.R3 not decoded: speakers and persons are shown as numbers".into());
    }
    for (message_name, scenario_name) in TEXT_SOURCES {
        let Some(message_data) = read_source(install, message_name, &mut report)? else {
            continue;
        };
        found += 1;
        let stem = message_name.trim_end_matches(".R3").to_lowercase();
        let Some(scenario_name) = scenario_name else {
            // The string pool.
            match crate::text::split_strings(&message_data) {
                Ok(blocks) => {
                    pool_strings += blocks.len();
                    let strings: Vec<TextBlock> = blocks
                        .iter()
                        .map(|b| TextBlock::new(b.offset, b.bytes, encoding))
                        .collect();
                    counts.malformed += strings.iter().filter(|b| b.malformed).count();
                    out.write_json(
                        &format!("{TEXT_DIR}/{stem}.json"),
                        &PoolFile {
                            source: message_name.to_string(),
                            encoding: encoding.name(),
                            note: POOL_NOTE,
                            strings,
                        },
                    )?;
                    report.outputs += 1;
                    if let Some(index) = read_source(install, IPPAN_INDEX, &mut report)? {
                        found += 1;
                        match townsfolk_talk(&index, &message_data, &look) {
                            Ok(file) => {
                                out.write_json(&format!("{TEXT_DIR}/townsfolk_talk.json"), &file)?;
                                report.outputs += 1;
                            }
                            Err(e) => {
                                failed += 1;
                                report.errors.push(e.to_string());
                            }
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    report.errors.push(format!("{message_name}: {e}"));
                }
            }
            continue;
        };
        let Some(scenario_data) = read_source(install, scenario_name, &mut report)? else {
            failed += 1;
            report.errors.push(format!(
                "{message_name}: {scenario_name} is missing; message items are delimited by its scripts"
            ));
            continue;
        };
        match convert_scenario(
            scenario_name,
            &scenario_data,
            message_name,
            &message_data,
            &look,
            &mut counts,
        ) {
            Ok(file) => {
                let chapter = stem.trim_end_matches('m');
                out.write_json(&format!("{TEXT_DIR}/{chapter}.json"), &file)?;
                out.write(
                    &format!("{TEXT_DIR}/{chapter}.txt"),
                    scenario_listing(&file).as_bytes(),
                )?;
                report.outputs += 2;
            }
            Err(e) => {
                failed += 1;
                report.errors.push(e);
            }
        }
    }
    report.settle(found, failed);
    if counts.unreferenced_bytes > 0 {
        report.notes.push(format!(
            "{} message bytes are not referenced by any script (listed as `unreferenced`)",
            counts.unreferenced_bytes
        ));
    }
    report.summary = if found == 0 {
        "no message files (SNR0M.R3–SNR4M.R3, IPPAN0M.R3) in the install".into()
    } else {
        format!(
            "{} of {found} text files converted: {} scenes, {} instructions, {} dialogues \
             ({} lines), {} strings, {pool_strings} pool strings; {} not cleanly decodable as {}",
            found - failed,
            counts.scenes,
            counts.instructions,
            counts.dialogues,
            counts.lines,
            counts.strings,
            counts.malformed,
            encoding.name()
        )
    };
    Ok(report)
}

/// Index of the townspeople chatter.
pub const IPPAN_INDEX: &str = "IPPAN0.R3";

const TALK_NOTE: &str = "Townspeople of each chapter's towns (IPPAN0.R3) and their lines \
    (IPPAN0M.R3). An entry is both the townsperson (BAKDATA.R3 townspeople) and the line; the \
    game places the group whose key matches a game-state value (125 = default group).";

#[derive(Serialize)]
struct TalkFile {
    note: &'static str,
    chapters: Vec<TalkChapter>,
}

#[derive(Serialize)]
struct TalkChapter {
    /// Scenario chapter (1-4).
    chapter: usize,
    pool_base: usize,
    towns: Vec<TalkTown>,
}

#[derive(Serialize)]
struct TalkTown {
    town: usize,
    groups: Vec<TalkGroup>,
}

#[derive(Serialize)]
struct TalkGroup {
    key: u8,
    people: Vec<TalkLine>,
}

#[derive(Serialize)]
struct TalkLine {
    entry: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    text: String,
}

/// The townspeople chatter of every chapter.
fn townsfolk_talk(
    index: &[u8],
    pool: &[u8],
    look: &Lookup<'_>,
) -> Result<TalkFile, crate::ippan::IppanError> {
    let chapters = crate::ippan::parse(index, pool)?;
    let chapters = chapters
        .iter()
        .enumerate()
        .map(|(c, ch)| TalkChapter {
            chapter: c + 1,
            pool_base: ch.pool_base,
            towns: ch
                .towns
                .iter()
                .map(|t| TalkTown {
                    town: t.index,
                    groups: t
                        .groups
                        .iter()
                        .map(|g| TalkGroup {
                            key: g.key,
                            people: g
                                .entries
                                .iter()
                                .map(|&e| TalkLine {
                                    entry: e,
                                    name: look.names.and_then(|n| {
                                        n.townsfolk.get(e as usize).map(|p| p.name.clone())
                                    }),
                                    text: ch.lines[e as usize]
                                        .map(|b| look.encoding.decode(b).text)
                                        .unwrap_or_default(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    Ok(TalkFile {
        note: TALK_NOTE,
        chapters,
    })
}

/// Decode `BAKDATA.R3` and write the officer, item and townsperson tables.
fn extract_names(
    install: &InstallDir,
    encoding: TextEncoding,
    out: &mut Output,
    requested: bool,
) -> Result<(KindReport, Option<Bakdata>), ExtractError> {
    let mut report = KindReport::new(Status::Extracted, requested, "");
    let Some(data) = read_source(install, NAMES_SOURCE, &mut report)? else {
        report.settle(0, 0);
        report.summary = "no BAKDATA.R3 in the install".into();
        return Ok((report, None));
    };
    match bakdata::parse(&data, encoding) {
        Ok(b) => {
            out.write_json(&format!("{TEXT_DIR}/officers.json"), &b.officers)?;
            out.write_json(&format!("{TEXT_DIR}/items.json"), &b.items)?;
            out.write_json(&format!("{TEXT_DIR}/townsfolk.json"), &b.townsfolk)?;
            report.outputs = 3;
            report.settle(1, 0);
            report.summary = format!(
                "{} officers, {} items, {} townspeople from BAKDATA.R3",
                b.officers.len(),
                b.items.len(),
                b.townsfolk.len()
            );
            report.notes.push(
                "the last item and the last townsperson are unused placeholder records; \
                 initial class / level / army are overridden by scenario scripts"
                    .into(),
            );
            Ok((report, Some(b)))
        }
        Err(e) => {
            report.settle(1, 1);
            report.summary = "BAKDATA.R3 could not be decoded".into();
            report.errors.push(e.to_string());
            Ok((report, None))
        }
    }
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

        // Text: SNR0 (messages raw), SNR1 (messages LS11-wrapped), IPPAN0M and BAKDATA.
        let text = &index.assets["text"];
        assert_eq!(text.status, Status::Extracted, "{text:#?}");
        assert_eq!(text.outputs, 6);
        assert!(text.notes.is_empty(), "{text:#?}");
        assert!(text.summary.contains("3 scenes"), "{}", text.summary);
        let snr0 = read_json(&target.join("text/snr0.json"));
        assert_eq!(snr0["encoding"], "EUC-KR (cp949)");
        let scene = &snr0["scenes"][0];
        assert_eq!(scene["message_base"], 2);
        assert!(scene["strings"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("유비는"));
        assert_eq!(scene["strings"][0]["used_by"][0], "narration");
        let line = &scene["dialogues"][0]["lines"][1];
        assert_eq!(line["speaker"], 1);
        assert_eq!(line["speaker_name"], "관우");
        let record = &scene["blocks"][1]["records"][1];
        assert_eq!(record["trigger"]["kind_name"], "talk");
        assert_eq!(record["trigger_person"], "관우");
        let code = &scene["blocks"][0]["records"][0]["code"];
        assert_eq!(code[0]["mnemonic"], "play_music");
        assert!(code[2]["resolved"]["text"]
            .as_str()
            .unwrap()
            .starts_with("유비: 천하가"));
        let listing = std::fs::read_to_string(target.join("text/snr0.txt")).unwrap();
        assert!(listing.contains("38 play_music song=2"), "{listing}");
        let snr1 = read_json(&target.join("text/snr1.json"));
        assert_eq!(snr1["scenes"].as_array().unwrap().len(), 2);
        let pool = read_json(&target.join("text/ippan0m.json"));
        assert_eq!(pool["strings"].as_array().unwrap().len(), 2);
        let talk = read_json(&target.join("text/townsfolk_talk.json"));
        let people = &talk["chapters"][0]["towns"][0]["groups"][0]["people"];
        assert_eq!(people[1]["entry"], 1);
        assert!(people[1]["text"]
            .as_str()
            .unwrap()
            .starts_with("승리하였다"));
        let names = &index.assets["names"];
        assert_eq!(names.status, Status::Extracted, "{names:#?}");
        let officers = read_json(&target.join("text/officers.json"));
        assert_eq!(officers.as_array().unwrap().len(), 384);
        assert_eq!(officers[1]["name"], "관우");
        assert_eq!(officers[1]["war"], 98);
        let items = read_json(&target.join("text/items.json"));
        assert_eq!(items[0]["name"], "청룡언월도");

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
        assert!(!out.path().join("text/snr1.json").exists());
        assert!(out.path().join("text/snr0.json").exists());
        let sprites = &index.assets["sprites"];
        assert_eq!(sprites.status, Status::Partial);
        assert!(sprites.errors[0].contains("HEXZCHP.R3"));
        assert!(!out.path().join("gfx/original/hexzchp").exists());
    }

    #[test]
    fn broken_townspeople_index_is_reported() {
        let src = korean();
        // A town record that runs past the end of the index.
        std::fs::write(src.path().join("IPPAN0.R3"), [1u8, 0x10, 0, 0, 0]).unwrap();
        let out = TempDir::new("ex-out-ippan");
        let index = extract(src.path(), out.path(), &Options::default()).unwrap();
        let text = &index.assets["text"];
        assert_eq!(text.status, Status::Partial, "{text:#?}");
        assert!(text.errors[0].starts_with("IPPAN0.R3 chunk 0"), "{text:#?}");
        assert!(out.path().join("text/ippan0m.json").exists());
        assert!(!out.path().join("text/townsfolk_talk.json").exists());
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
        let snr0 = read_json(&out.path().join("text/snr0.json"));
        assert_eq!(snr0["encoding"], "Big5");
        let scene = &snr0["scenes"][0];
        assert!(scene["strings"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("劉備"));
        assert_eq!(scene["dialogues"][0]["lines"][0]["speaker_name"], "劉備");
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
    fn extracts_maps() {
        let src = TempDir::new("maps-src");
        testutil::write_map_install(src.path());
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
