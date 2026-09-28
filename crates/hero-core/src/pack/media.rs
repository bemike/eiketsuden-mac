//! Native media check behind [`Pack::missing_media`]; the file conventions are those of
//! `docs/ASSETS.md`.

use super::validate::is_media_key;
use super::{Issue, Pack, Severity};
use crate::battledef::MapDef;
use crate::map::BattleMap;
use crate::script::Cmd;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Unit sheet colours; one sheet per sprite key and side.
const SIDES: [&str; 3] = ["player", "ally", "enemy"];
const UNITS_TOML: &str = "gfx/units/units.toml";
const TILES_TOML: &str = "gfx/tiles/terrain.toml";
const FX_TOML: &str = "gfx/fx/fx.toml";
const ICONS_TOML: &str = "gfx/ui/icons.toml";
const UNKNOWN_PORTRAIT: &str = "gfx/portraits/_unknown.png";
/// Tile size of a tileset without `tile_size`, and of the flat-colour map drawn without one
/// (`docs/ASSETS.md`).
const DEFAULT_TILE_SIZE: u32 = 16;

impl Pack {
    /// Check that every media file the pack refers to exists below `root` (the top pack
    /// directory) — for a layered pack in the directory of any of its [`Pack::layers`], looked
    /// up top pack first like the game does, so a media index file (`units.toml`,
    /// `terrain.toml`, `fx.toml`, `icons.toml`) is read from the first pack that has one.
    ///
    /// Checked: unit sheets `gfx/units/<sprite>_<side>.png` and their `units.toml` entries,
    /// officer and `@show` portraits `gfx/portraits/<key>.png` (warnings: `_unknown.png` is
    /// shown instead, which must exist), `bgm/<key>.ogg` of battles and dramas, `gfx/bg/<key>.png`
    /// and `sfx/<key>.(ogg|wav)` of dramas, a `gfx/tiles/terrain.toml` tile for every terrain,
    /// `gfx/fx/fx.toml` entries and strips for strategy effects, `gfx/ui/icons.toml` keys
    /// of item icons (warnings), and the picture layers `gfx/maps/<key>.png` of maps, whose
    /// size must be the map's size in tiles times the tileset's `tile_size`.
    pub fn missing_media(&self, root: &Path) -> Vec<Issue> {
        let mut dirs: Vec<PathBuf> = self.layers.iter().map(|l| root.join(&l.dir)).collect();
        if dirs.is_empty() {
            dirs.push(root.to_path_buf());
        }
        let mut m = MediaCheck {
            dirs,
            issues: Vec::new(),
            reported: BTreeSet::new(),
            tile_size: DEFAULT_TILE_SIZE,
        };
        m.units(self);
        m.portraits(self);
        m.audio_and_backgrounds(self);
        m.tiles(self);
        m.map_pictures(self);
        m.effects(self);
        m.icons(self);
        m.issues
    }
}

struct MediaCheck {
    /// Media directories in lookup order: the top pack, then the packs it extends.
    dirs: Vec<PathBuf>,
    issues: Vec<Issue>,
    /// Missing files already reported (each is reported once, at its first user).
    reported: BTreeSet<String>,
    /// `tile_size` of the tileset in use (read by [`MediaCheck::tiles`]).
    tile_size: u32,
}

impl MediaCheck {
    /// The file `rel` of the first pack directory that has it.
    fn find(&self, rel: &str) -> Option<PathBuf> {
        self.dirs
            .iter()
            .map(|dir| dir.join(rel))
            .find(|path| path.is_file())
    }

    fn exists(&self, rel: &str) -> bool {
        self.find(rel).is_some()
    }

    fn push(&mut self, severity: Severity, context: &str, msg: String) {
        self.issues.push(Issue {
            severity,
            context: context.to_string(),
            msg,
        });
    }

    /// Report `rel` once if it does not exist.
    fn require(&mut self, severity: Severity, context: &str, rel: &str, what: &str) {
        if !self.exists(rel) && self.reported.insert(rel.to_string()) {
            self.push(severity, context, format!("missing {rel} ({what})"));
        }
    }

    /// Parse a media index file (each is read once); reports and returns `None` when it is
    /// missing or broken.
    fn index(&mut self, rel: &str, context: &str) -> Option<toml::Table> {
        let Some(path) = self.find(rel) else {
            self.push(Severity::Error, context, format!("missing {rel}"));
            return None;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("cannot read {}: {e}", path.display());
                self.push(Severity::Error, context, msg);
                return None;
            }
        };
        match toml::from_str::<toml::Table>(text.strip_prefix('\u{feff}').unwrap_or(&text)) {
            Ok(t) => Some(t),
            Err(e) => {
                self.push(Severity::Error, rel, e.to_string().trim_end().to_string());
                None
            }
        }
    }

    fn units(&mut self, pack: &Pack) {
        if pack.classes.is_empty() {
            return;
        }
        let index = self.index(UNITS_TOML, "unit sprites");
        let sprites = index
            .as_ref()
            .and_then(|t| t.get("sprites"))
            .and_then(|v| v.as_table());
        let mut keys_seen = BTreeSet::new();
        for class in pack.classes.values() {
            let key = class.sprite.as_str();
            if !keys_seen.insert(key) {
                continue;
            }
            let ctx = format!("class {}", class.id);
            for side in SIDES {
                let rel = format!("gfx/units/{key}_{side}.png");
                self.require(Severity::Error, &ctx, &rel, "unit sprite sheet");
            }
            if index.is_some() && !sprites.is_some_and(|s| s.contains_key(key)) {
                self.push(
                    Severity::Error,
                    &ctx,
                    format!("{UNITS_TOML} has no [sprites.{key}] entry"),
                );
            }
        }
    }

    fn portraits(&mut self, pack: &Pack) {
        self.require(
            Severity::Error,
            "portraits",
            UNKNOWN_PORTRAIT,
            "fallback portrait",
        );
        for officer in pack.officers.values() {
            let rel = format!("gfx/portraits/{}.png", officer.portrait_key());
            self.require(
                Severity::Warning,
                &format!("officer {}", officer.id),
                &rel,
                "portrait; `_unknown` is shown instead",
            );
        }
        for scene in pack.scenes.values() {
            for cmd in &scene.cmds {
                if let Cmd::Show { who, .. } = cmd {
                    let key = pack
                        .speaker_officer(who)
                        .map_or(who.as_str(), |o| o.portrait_key());
                    let rel = format!("gfx/portraits/{key}.png");
                    self.require(
                        Severity::Warning,
                        &format!("scene {}", scene.id),
                        &rel,
                        "portrait; `_unknown` is shown instead",
                    );
                }
            }
        }
    }

    fn audio_and_backgrounds(&mut self, pack: &Pack) {
        for b in pack.battles.values() {
            for key in [&b.bgm, &b.bgm_enemy].into_iter().flatten() {
                self.require(
                    Severity::Error,
                    &format!("battle {}", b.id),
                    &format!("bgm/{key}.ogg"),
                    "music",
                );
            }
        }
        for scene in pack.scenes.values() {
            let ctx = format!("scene {}", scene.id);
            for cmd in &scene.cmds {
                match cmd {
                    Cmd::Bgm(Some(key)) => {
                        self.require(Severity::Error, &ctx, &format!("bgm/{key}.ogg"), "music")
                    }
                    Cmd::Bg(Some(key)) => self.require(
                        Severity::Error,
                        &ctx,
                        &format!("gfx/bg/{key}.png"),
                        "background",
                    ),
                    Cmd::Sfx(key) => {
                        let ogg = format!("sfx/{key}.ogg");
                        let wav = format!("sfx/{key}.wav");
                        if !self.exists(&ogg)
                            && !self.exists(&wav)
                            && self.reported.insert(ogg.clone())
                        {
                            self.push(
                                Severity::Error,
                                &ctx,
                                format!("missing {ogg} or {wav} (sound effect)"),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn tiles(&mut self, pack: &Pack) {
        if pack.terrain.is_empty() {
            return;
        }
        let Some(index) = self.index(TILES_TOML, "terrain tiles") else {
            return;
        };
        match index.get("tile_size") {
            None => {}
            Some(v) => match v.as_integer().and_then(|n| u32::try_from(n).ok()) {
                Some(n) if n > 0 => self.tile_size = n,
                _ => self.push(
                    Severity::Error,
                    TILES_TOML,
                    format!("tile_size {v} must be a positive whole number of pixels"),
                ),
            },
        }
        match index.get("image").and_then(|v| v.as_str()) {
            Some(image) => {
                let rel = format!("gfx/tiles/{image}");
                self.require(Severity::Error, TILES_TOML, &rel, "terrain atlas");
            }
            None => self.push(
                Severity::Error,
                TILES_TOML,
                "has no `image` atlas file name".into(),
            ),
        }
        let tiles = index.get("tiles").and_then(|v| v.as_table());
        for t in &pack.terrain {
            let key = t.tile_key();
            if !tiles.is_some_and(|tiles| tiles.contains_key(key)) {
                self.push(
                    Severity::Error,
                    &format!("terrain {}", t.id),
                    format!("{TILES_TOML} has no [tiles.{key}] entry"),
                );
            }
        }
    }

    /// Picture layers of the map files and of battles that write their map themselves. A
    /// picture of another size would be drawn misaligned with the rules grid, so the game
    /// falls back to the tileset for it; here that is an error.
    fn map_pictures(&mut self, pack: &Pack) {
        let mut users: Vec<(String, &MapDef)> = Vec::new();
        let library: Vec<(String, MapDef)> = pack
            .maps
            .values()
            .map(|m| (format!("map {}", m.id), m.to_def()))
            .collect();
        for (ctx, def) in &library {
            users.push((ctx.clone(), def));
        }
        for b in pack.battles.values() {
            if b.map.use_map.is_none() {
                users.push((format!("battle {}", b.id), &b.map));
            }
        }
        for (ctx, def) in users {
            // Bad keys are reported by `Pack::validate`.
            let Some(key) = def.image.as_deref().filter(|k| is_media_key(k)) else {
                continue;
            };
            let Ok(map) = BattleMap::parse(&def.rows, &def.legend, &pack.terrain) else {
                continue;
            };
            let rel = format!("gfx/maps/{key}.png");
            let Some(path) = self.find(&rel) else {
                self.require(Severity::Error, &ctx, &rel, "map picture");
                continue;
            };
            let tile = self.tile_size;
            let want = (map.width as u32 * tile, map.height as u32 * tile);
            match png_size(&path) {
                Ok(size) if size == want => {}
                Ok((w, h)) => self.push(
                    Severity::Error,
                    &ctx,
                    format!(
                        "{rel} is {w}×{h} pixels; the map needs {}×{} ({}×{} tiles of {tile} px, the tileset's tile_size)",
                        want.0, want.1, map.width, map.height
                    ),
                ),
                Err(e) => self.push(
                    Severity::Error,
                    &ctx,
                    format!("{}: not a readable PNG: {e}", path.display()),
                ),
            }
        }
    }

    fn effects(&mut self, pack: &Pack) {
        let users: Vec<_> = pack
            .strategies
            .values()
            .filter(|s| !s.fx.is_empty())
            .collect();
        if users.is_empty() {
            return;
        }
        let Some(index) = self.index(FX_TOML, "strategy effects") else {
            return;
        };
        let fx = index.get("fx").and_then(|v| v.as_table());
        for s in users {
            let ctx = format!("strategy {}", s.id);
            if !fx.is_some_and(|fx| fx.contains_key(&s.fx)) {
                self.push(
                    Severity::Error,
                    &ctx,
                    format!("{FX_TOML} has no [fx.{}] entry", s.fx),
                );
            }
            self.require(
                Severity::Error,
                &ctx,
                &format!("gfx/fx/{}.png", s.fx),
                "effect strip",
            );
        }
    }

    fn icons(&mut self, pack: &Pack) {
        let users: Vec<_> = pack.items.values().filter(|i| !i.icon.is_empty()).collect();
        if users.is_empty() {
            return;
        }
        if !self.exists(ICONS_TOML) {
            self.push(
                Severity::Warning,
                "item icons",
                format!("missing {ICONS_TOML}; items are shown without icons"),
            );
            return;
        }
        let Some(index) = self.index(ICONS_TOML, "item icons") else {
            return;
        };
        let icons = index.get("icons").and_then(|v| v.as_table());
        for item in users {
            if !icons.is_some_and(|icons| icons.contains_key(&item.icon)) {
                self.push(
                    Severity::Warning,
                    &format!("item {}", item.id),
                    format!("{ICONS_TOML} has no icon `{}`", item.icon),
                );
            }
        }
    }
}

/// Width and height of the PNG image at `path`, from its `IHDR` chunk (the first chunk of
/// every PNG), without decoding the image.
fn png_size(path: &Path) -> Result<(u32, u32), String> {
    use std::io::Read;
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    let mut head = [0u8; 24];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|e| e.to_string())?;
    if head[..8] != SIGNATURE || &head[12..16] != b"IHDR" {
        return Err("no PNG signature and IHDR chunk".into());
    }
    let be = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Ok((be(16), be(20)))
}
