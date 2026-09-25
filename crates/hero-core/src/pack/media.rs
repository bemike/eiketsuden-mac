//! Native media check behind [`Pack::missing_media`]; the file conventions are those of
//! `docs/ASSETS.md`.

use super::{Issue, Pack, Severity};
use crate::script::Cmd;
use std::collections::BTreeSet;
use std::path::Path;

/// Unit sheet colours; one sheet per sprite key and side.
const SIDES: [&str; 3] = ["player", "ally", "enemy"];
const UNITS_TOML: &str = "gfx/units/units.toml";
const TILES_TOML: &str = "gfx/tiles/terrain.toml";
const FX_TOML: &str = "gfx/fx/fx.toml";
const ICONS_TOML: &str = "gfx/ui/icons.toml";
const UNKNOWN_PORTRAIT: &str = "gfx/portraits/_unknown.png";

impl Pack {
    /// Check that every media file the pack refers to exists below `root` (the pack
    /// directory): unit sheets `gfx/units/<sprite>_<side>.png` and their `units.toml` entries,
    /// officer and `@show` portraits `gfx/portraits/<key>.png` (warnings: `_unknown.png` is
    /// shown instead, which must exist), `bgm/<key>.ogg` of battles and dramas, `gfx/bg/<key>.png`
    /// and `sfx/<key>.(ogg|wav)` of dramas, a `gfx/tiles/terrain.toml` tile for every terrain,
    /// `gfx/fx/fx.toml` entries and strips for strategy effects, and `gfx/ui/icons.toml` keys
    /// of item icons (warnings).
    pub fn missing_media(&self, root: &Path) -> Vec<Issue> {
        let mut m = MediaCheck {
            root,
            issues: Vec::new(),
            reported: BTreeSet::new(),
        };
        m.units(self);
        m.portraits(self);
        m.audio_and_backgrounds(self);
        m.tiles(self);
        m.effects(self);
        m.icons(self);
        m.issues
    }
}

struct MediaCheck<'a> {
    root: &'a Path,
    issues: Vec<Issue>,
    /// Missing files already reported (each is reported once, at its first user).
    reported: BTreeSet<String>,
}

impl MediaCheck<'_> {
    fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).is_file()
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
        let text = match std::fs::read_to_string(self.root.join(rel)) {
            Ok(t) => t,
            Err(e) => {
                let msg = match e.kind() {
                    std::io::ErrorKind::NotFound => format!("missing {rel}"),
                    _ => format!("cannot read {rel}: {e}"),
                };
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
