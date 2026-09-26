//! Battle screen: the tactical map of a campaign battle on top of [`hero_core::battle`].
//!
//! * **Entry points** — [`BattleScreen::start`] builds the battle of a campaign `Battle` node
//!   (title card → objective window → `begin`, whose intro scene plays as a drama overlay);
//!   [`BattleScreen::resume`] continues the battle of a mid-battle save. Both are wired in
//!   `crate::flow`. When the battle is over the screen returns
//!   `Transition::Flow(Flow::BattleEnded(state))`.
//! * **State** — the [`BattleState`] is the single source of truth; after every applied action
//!   it is copied into `ctx.session.battle`, so 중단 기록 saves exactly what is on screen.
//! * **Animation** — actions return events; [`anim`] turns them into beats that move
//!   [`anim::UnitView`]s (what is drawn) until they catch up with the state.
//! * **Player phase** — [`player::PlayerUi`] is the command state machine (select, move,
//!   공격/책략/도구/대기, undo of an unconfirmed move); this module maps keyboard, mouse and
//!   touch onto it and draws the menus and forecasts.
//! * **AI phases** — one unit at a time from `next_ai_unit` / `ai_actions`, animated like the
//!   player's actions; holding confirm fast-forwards.
//!
//! Controls: arrows/WASD move the cursor, Z/Enter/Space confirm, X/Esc/right click cancel,
//! Tab/E and Q cycle through units that can still act, mouse at the screen edge / right-drag /
//! touch drag / wheel scroll the map, and holding confirm speeds animations up.

mod anim;
mod camera;
mod hud;
mod player;
mod sprites;
#[cfg(test)]
mod testutil;
mod text;
mod tileset;

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::assets::{AssetState, FileRequest};
use crate::audio::{bgm, sfx};
use crate::flow::Flow;
use crate::gfx::{
    draw_placeholder, fill_rect, Align, FontId, TextStyle, SCREEN, VIRTUAL_H, VIRTUAL_W,
};
use crate::screens::drama::DramaScreen;
use crate::screens::error::ErrorScreen;
use crate::screens::saveload::SaveLoadScreen;
use crate::screens::settings::SettingsScreen;
use crate::ui::dialog::{ConfirmDialog, ConfirmEvent};
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_icon, draw_window, draw_window_ex, WindowStyle};
use anim::{Cue, EventPlayer, Scene};
use camera::{edge_direction, Camera, EDGE_PAN_SPEED};
use hero_core::battle::{Action, BattleEvent, BattleState, Outcome, UnitId};
use hero_core::battledef::Side;
use hero_core::geom::Pos;
use hero_core::pack::Pack;
use macroquad::prelude::*;
use player::{Command, Mode, PlayerUi, Request};
use sprites::{FxDef, SpriteDef};
use std::collections::BTreeMap;
use std::rc::Rc;
use tileset::{MapRenderer, Tileset, TILE};

/// Screen area of the map (below the top bar).
const VIEWPORT: Rect = Rect {
    x: 0.0,
    y: hud::TOP_BAR_H,
    w: VIRTUAL_W,
    h: VIRTUAL_H - hud::TOP_BAR_H,
};
/// Seconds the title card stays up.
const TITLE_SECONDS: f32 = 2.6;
/// Pause between AI units (seconds at normal speed).
const AI_PAUSE: f32 = 0.3;
/// Speed factor while confirm is held.
const FAST_FORWARD: f32 = 3.0;
/// Seconds after the last player unit acted before the phase ends by itself.
const AUTO_END_DELAY: f32 = 0.4;

/// Metadata files of the battle art, loaded when the screen opens.
#[derive(Default)]
struct Meta {
    tileset_req: Option<FileRequest>,
    units_req: Option<FileRequest>,
    fx_req: Option<FileRequest>,
    tileset: Option<Tileset>,
    sprites: BTreeMap<String, SpriteDef>,
    fx: BTreeMap<String, FxDef>,
    /// Media key per effect (`fx/<key>`), so drawing does not format strings.
    fx_textures: BTreeMap<String, String>,
}

impl Meta {
    fn units_loaded(&self) -> bool {
        self.units_req.is_none()
    }
}

/// Read a finished request as UTF-8 text.
fn request_text(req: &mut Option<FileRequest>) -> Option<Result<String, String>> {
    let result = req.as_mut()?.poll()?.clone();
    *req = None;
    Some(result.and_then(|b| String::from_utf8(b).map_err(|e| e.to_string())))
}

enum Stage {
    Title {
        age: f32,
    },
    Objective,
    Battle,
    Result {
        lines: Vec<(String, Vec<(String, Color)>)>,
        title: String,
    },
}

/// Which overlay screen the battle is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Drama,
    Screen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Command,
    Strategies,
    Items,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BattleMenuItem {
    EndTurn,
    Units,
    Objective,
    Save,
    Settings,
    Title,
}

impl BattleMenuItem {
    const ALL: [BattleMenuItem; 6] = [
        BattleMenuItem::EndTurn,
        BattleMenuItem::Units,
        BattleMenuItem::Objective,
        BattleMenuItem::Save,
        BattleMenuItem::Settings,
        BattleMenuItem::Title,
    ];

    fn label(self) -> &'static str {
        match self {
            BattleMenuItem::EndTurn => "턴 종료",
            BattleMenuItem::Units => "부대 일람",
            BattleMenuItem::Objective => "승리 조건",
            BattleMenuItem::Save => "중단 기록",
            BattleMenuItem::Settings => "설정",
            BattleMenuItem::Title => "타이틀로",
        }
    }
}

/// Windows opened from the battle menu (they are modal over the map).
enum Panel {
    None,
    Menu(Menu),
    Units {
        side: Side,
        ids: Vec<UnitId>,
        menu: Menu,
    },
    Objective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    EndTurn,
    Title,
}

/// The AI unit being played.
#[derive(Debug, Clone, Copy)]
struct AiStep {
    unit: UnitId,
    /// Plans applied so far (a move, then the action).
    tries: u8,
}

/// Right mouse button gesture: a click cancels, a drag pans the map.
#[derive(Debug, Clone, Copy)]
struct RightDrag {
    origin: Vec2,
    last: Vec2,
    moved: bool,
}

pub struct BattleScreen {
    pack: Rc<Pack>,
    state: BattleState,
    /// A new battle (title card, objective, `begin`) rather than a resumed save.
    fresh: bool,
    stage: Stage,
    meta: Meta,
    map: MapRenderer,
    camera: Camera,
    scene: Scene,
    events: EventPlayer,
    ui: PlayerUi,
    mode_menu: Option<(MenuKind, Menu)>,
    panel: Panel,
    dialog: Option<(ConfirmDialog, Confirm)>,
    waiting: Option<Waiting>,
    cursor: Pos,
    /// Cursor tile when the current press began (tap-to-preview, tap-again-to-confirm).
    cursor_at_press: Option<Pos>,
    ai: Option<AiStep>,
    ai_pause: f32,
    /// The pending move can be undone (its events were a plain `Moved`).
    move_undoable: bool,
    /// Jump to the player's units when the queue runs dry after a player phase started.
    focus_player: bool,
    idle_time: f32,
    touch_seen: bool,
    rdrag: Option<RightDrag>,
    /// Class id -> sprite key; (sprite key, side) -> sheet texture key.
    sprite_of: BTreeMap<String, String>,
    sheets: BTreeMap<(String, Side), String>,
    cues: Vec<Cue>,
}

impl BattleScreen {
    /// New battle `battle_id` for the session's campaign. Returns an error screen when the battle
    /// cannot be built (unknown id, broken data).
    pub fn start(ctx: &mut Ctx, battle_id: &str) -> Box<dyn Screen> {
        let (Some(pack), Some(session)) = (ctx.pack.clone(), ctx.session.as_ref()) else {
            return Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec!["진행 중인 캠페인이 없습니다.".into()],
            ));
        };
        let seed = crate::platform::unix_now() ^ ctx.frame.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        match BattleState::new(&pack, battle_id, &session.campaign, seed) {
            Ok(state) => {
                if let Some(s) = ctx.session.as_mut() {
                    s.battle = Some(state.clone());
                }
                Box::new(BattleScreen::new(pack, state, true))
            }
            Err(e) => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec![format!("전투 `{battle_id}`를 시작할 수 없습니다: {e}")],
            )),
        }
    }

    /// Continue the battle stored in the session (mid-battle save).
    pub fn resume(ctx: &mut Ctx) -> Box<dyn Screen> {
        let pack = ctx.pack.clone();
        let battle = ctx.session.as_ref().and_then(|s| s.battle.clone());
        match (pack, battle) {
            (Some(pack), Some(state)) if pack.battles.contains_key(&state.battle_id) => {
                Box::new(BattleScreen::new(pack, state, false))
            }
            (Some(_), Some(state)) => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec![format!(
                    "기록된 전투 `{}`가 데이터 팩에 없습니다.",
                    state.battle_id
                )],
            )),
            _ => Box::new(ErrorScreen::recoverable(
                "전투 오류",
                vec!["이어서 할 전투 기록이 없습니다.".into()],
            )),
        }
    }

    fn new(pack: Rc<Pack>, state: BattleState, fresh: bool) -> BattleScreen {
        let map = MapRenderer::new(&state.map);
        let mut camera = Camera::new(VIEWPORT, map.size);
        let scene = Scene::new(&state);
        let cursor = state
            .units
            .iter()
            .find(|u| u.side == Side::Player && u.is_active() && u.lord)
            .or_else(|| {
                state
                    .units
                    .iter()
                    .find(|u| u.side == Side::Player && u.is_active())
            })
            .map_or(Pos::new(0, 0), |u| u.pos);
        camera.snap_to(cursor);
        let mut sprite_of = BTreeMap::new();
        let mut sheets = BTreeMap::new();
        for c in pack.classes.values() {
            sprite_of.insert(c.id.clone(), c.sprite.clone());
            for side in [Side::Player, Side::Ally, Side::Enemy] {
                sheets.insert(
                    (c.sprite.clone(), side),
                    sprites::sheet_key(&c.sprite, side),
                );
            }
        }
        BattleScreen {
            pack,
            state,
            fresh,
            stage: Stage::Title { age: 0.0 },
            meta: Meta::default(),
            map,
            camera,
            scene,
            events: EventPlayer::default(),
            ui: PlayerUi::default(),
            mode_menu: None,
            panel: Panel::None,
            dialog: None,
            waiting: None,
            cursor,
            cursor_at_press: None,
            ai: None,
            ai_pause: 0.0,
            move_undoable: false,
            focus_player: false,
            idle_time: 0.0,
            touch_seen: false,
            rdrag: None,
            sprite_of,
            sheets,
            cues: Vec::new(),
        }
    }

    // ----- resources ---------------------------------------------------------------------

    fn start_loading(&mut self, ctx: &Ctx) {
        let root = ctx.media.root();
        self.meta.tileset_req = Some(FileRequest::new(root.path(tileset::TILESET_FILE)));
        self.meta.units_req = Some(FileRequest::new(root.path(sprites::UNITS_FILE)));
        self.meta.fx_req = Some(FileRequest::new(root.path(sprites::FX_FILE)));
        let mut textures: Vec<String> = vec!["ui/flags".into(), "ui/icons".into()];
        for u in &self.state.units {
            if let Some(sprite) = self.sprite_of.get(&u.class) {
                if let Some(key) = self.sheets.get(&(sprite.clone(), u.side)) {
                    if !textures.contains(key) {
                        textures.push(key.clone());
                    }
                }
            }
        }
        ctx.media.preload_textures(&textures);
        let mut sounds: Vec<String> = sfx::ALL.iter().map(|k| format!("sfx/{k}")).collect();
        for key in [self.bgm_for(Side::Player), self.bgm_for(Side::Enemy)] {
            sounds.push(format!("bgm/{key}"));
        }
        sounds.push(format!("bgm/{}", bgm::VICTORY));
        sounds.push(format!("bgm/{}", bgm::DEFEAT));
        ctx.media.preload_sounds(&sounds);
    }

    fn poll_meta(&mut self, ctx: &Ctx) {
        if let Some(r) = request_text(&mut self.meta.tileset_req) {
            match r.and_then(|s| Tileset::parse(&s)) {
                Ok((ts, warnings)) => {
                    for w in warnings {
                        macroquad::logging::warn!("{}: {}", tileset::TILESET_FILE, w);
                    }
                    ctx.media.preload_textures(&[ts.texture.as_str()]);
                    self.meta.tileset = Some(ts);
                }
                Err(e) => macroquad::logging::warn!(
                    "{} unavailable, drawing flat terrain: {}",
                    tileset::TILESET_FILE,
                    e
                ),
            }
        }
        if let Some(r) = request_text(&mut self.meta.units_req) {
            match r.and_then(|s| sprites::parse_units(&s)) {
                Ok(s) => self.meta.sprites = s,
                Err(e) => macroquad::logging::warn!(
                    "{} unavailable, using 16x16 frames: {}",
                    sprites::UNITS_FILE,
                    e
                ),
            }
        }
        if let Some(r) = request_text(&mut self.meta.fx_req) {
            match r.and_then(|s| sprites::parse_fx(&s)) {
                Ok(fx) => {
                    let keys: Vec<String> = fx.keys().map(|k| format!("fx/{k}")).collect();
                    ctx.media.preload_textures(&keys);
                    self.meta.fx_textures =
                        fx.keys().map(|k| (k.clone(), format!("fx/{k}"))).collect();
                    self.meta.fx = fx;
                }
                Err(e) => {
                    macroquad::logging::warn!("{} unavailable, no effects: {}", sprites::FX_FILE, e)
                }
            }
        }
        if !self.map.is_built() && self.meta.tileset_req.is_none() {
            match &self.meta.tileset {
                Some(ts) => match ctx.media.texture_state(&ts.texture) {
                    AssetState::Loading => {}
                    AssetState::Ready => {
                        let atlas = ctx.media.texture(&ts.texture);
                        self.map
                            .build(&self.state.map, &self.pack, Some(ts), atlas.as_ref());
                    }
                    AssetState::Missing => {
                        self.map.build(&self.state.map, &self.pack, None, None);
                    }
                },
                None => self.map.build(&self.state.map, &self.pack, None, None),
            }
        }
    }

    // ----- helpers -----------------------------------------------------------------------

    fn def(&self) -> &hero_core::battledef::BattleDef {
        self.state.def(&self.pack)
    }

    fn bgm_for(&self, side: Side) -> String {
        let def = self.def();
        match side {
            Side::Enemy => def
                .bgm_enemy
                .clone()
                .unwrap_or_else(|| bgm::ENEMY.to_string()),
            Side::Player | Side::Ally => def.bgm.clone().unwrap_or_else(|| bgm::BATTLE.to_string()),
        }
    }

    fn speed(&self, ctx: &Ctx) -> f32 {
        let held = ctx.input.key_down(KeyCode::Z)
            || ctx.input.key_down(KeyCode::Enter)
            || ctx.input.key_down(KeyCode::Space)
            || (ctx.input.down()
                && self.ui.mode == Mode::Browse
                && self.state.phase != Side::Player);
        ctx.settings.battle_speed.multiplier() * if held { FAST_FORWARD } else { 1.0 }
    }

    fn store_session(&self, ctx: &mut Ctx) {
        if let Some(s) = ctx.session.as_mut() {
            s.battle = Some(self.state.clone());
        }
    }

    /// Display name of a unit reference (tag or officer id) for objective texts.
    fn ref_name(&self, r: &str) -> String {
        if let Some(u) = self.state.find_unit(r) {
            return self.state.units[u].name.clone();
        }
        self.pack
            .officer(r)
            .map_or_else(|| r.to_string(), |o| o.name.clone())
    }

    /// Apply an action, keep the session in sync and queue its animation.
    fn apply(&mut self, ctx: &mut Ctx, action: Action) -> Option<Vec<BattleEvent>> {
        match self.state.apply(&self.pack, action.clone()) {
            Ok(events) => {
                self.store_session(ctx);
                self.events
                    .push(anim::plan(&events, &self.state, &self.pack, &self.meta.fx));
                Some(events)
            }
            Err(e) => {
                macroquad::logging::error!("battle action {:?} rejected: {}", action, e);
                ctx.sfx(sfx::ERROR);
                None
            }
        }
    }

    fn play_phase_music(&self, ctx: &mut Ctx, side: Side) {
        let key = self.bgm_for(side);
        ctx.audio.play_bgm(&key);
    }

    /// Carry out the cues of the animation; returns a transition for drama overlays.
    fn handle_cues(&mut self, ctx: &mut Ctx) -> Transition {
        let mut out = Transition::None;
        for cue in std::mem::take(&mut self.cues) {
            match cue {
                Cue::Sfx(k) => ctx.sfx(k),
                Cue::Follow(p) => self.camera.keep_visible(p, 2.5),
                Cue::Center(p) => self.camera.center_on(p),
                Cue::PhaseMusic(side) => {
                    self.play_phase_music(ctx, side);
                    if side == Side::Player {
                        self.focus_player = true;
                    }
                }
                Cue::Jingle(victory) => {
                    let key = if victory { bgm::VICTORY } else { bgm::DEFEAT };
                    // Without the jingle file, the short effect of the same name stands in.
                    if ctx.media.sound_state(&format!("bgm/{key}")) == AssetState::Missing {
                        ctx.audio.stop_bgm();
                        ctx.sfx(if victory { sfx::VICTORY } else { sfx::DEFEAT });
                    } else {
                        ctx.audio.play_jingle(key);
                    }
                }
                Cue::Drama(scene) => {
                    if self.pack.scene(&scene).is_some() {
                        self.waiting = Some(Waiting::Drama);
                        out = Transition::push(DramaScreen::overlay(ctx, &scene));
                    } else {
                        macroquad::logging::warn!("battle drama scene `{}` not found", scene);
                        self.events.resume();
                    }
                }
            }
        }
        out
    }

    /// The animation queue ran dry: snap the views to the state and continue the flow.
    fn events_done(&mut self) {
        self.scene.sync(&self.state);
        if matches!(self.ui.mode, Mode::Walking { .. }) {
            self.ui.walked(&self.state, self.move_undoable);
            self.move_undoable = false;
        }
        if self.state.phase == Side::Player && self.focus_player && self.state.outcome.is_none() {
            self.focus_player = false;
            self.ui.reset();
            let focus = player::cycle_actor(&self.state, None, 1).or_else(|| {
                self.state
                    .units
                    .iter()
                    .position(|u| u.lord && u.is_active())
            });
            if let Some(u) = focus {
                self.cursor = self.state.units[u].pos;
                self.camera.center_on(self.cursor);
            }
        }
        self.idle_time = 0.0;
    }

    fn begin_battle(&mut self, ctx: &mut Ctx) {
        let events = self.state.begin(&self.pack);
        self.store_session(ctx);
        self.events
            .push(anim::plan(&events, &self.state, &self.pack, &self.meta.fx));
        self.stage = Stage::Battle;
    }

    fn open_result(&mut self) {
        let def = self.def();
        let item_name = |id: &str| {
            self.pack
                .item(id)
                .map_or(id.to_string(), |i| i.name.clone())
        };
        let (title, sections) = match self.state.outcome {
            Some(Outcome::Victory) => {
                let mut sections = Vec::new();
                sections.push((
                    "전리품".to_string(),
                    vec![(
                        format!("금 {}", crate::ui::format::thousands(self.state.gold_found)),
                        theme::TEXT_ACCENT,
                    )],
                ));
                let mut counts: BTreeMap<String, u32> = BTreeMap::new();
                for i in &self.state.items_found {
                    *counts.entry(item_name(i)).or_default() += 1;
                }
                if !counts.is_empty() {
                    let items = counts
                        .into_iter()
                        .map(|(n, c)| if c > 1 { format!("{n} ×{c}") } else { n })
                        .collect::<Vec<_>>()
                        .join(", ");
                    sections[0].1.push((items, theme::TEXT));
                }
                if let Some(b) = &def.bonus {
                    let line = if self.state.bonus_done {
                        (
                            format!("달성 — 출진 부대 경험치 +{}", b.exp),
                            theme::TEXT_GOOD,
                        )
                    } else {
                        (format!("미달성 — {}", b.desc), theme::TEXT_DIM)
                    };
                    sections.push(("보너스 목표".to_string(), vec![line]));
                }
                (format!("{} 승리", def.name), sections)
            }
            Some(Outcome::Defeat(reason)) => {
                let lord = self
                    .state
                    .units
                    .iter()
                    .find(|u| u.lord)
                    .map(|u| u.name.clone());
                (
                    format!("{} 패배", def.name),
                    vec![(
                        "패인".to_string(),
                        vec![(text::defeat_text(reason, lord.as_deref()), theme::TEXT_BAD)],
                    )],
                )
            }
            None => return,
        };
        self.stage = Stage::Result {
            lines: sections,
            title,
        };
    }

    // ----- AI ----------------------------------------------------------------------------

    fn ai_update(&mut self, ctx: &mut Ctx, dt: f32) {
        if self.ai_pause > 0.0 {
            self.ai_pause -= dt;
            return;
        }
        let side = self.state.phase;
        let Some(step) = self.ai else {
            match self.state.next_ai_unit() {
                Some(id) => {
                    self.ai = Some(AiStep { unit: id, tries: 0 });
                    let p = self.state.units[id].pos;
                    self.cursor = p;
                    if !self.camera.is_visible(p) {
                        self.camera.center_on(p);
                    } else {
                        self.camera.keep_visible(p, 3.0);
                    }
                    self.ai_pause = AI_PAUSE;
                }
                None => {
                    self.ai = None;
                    if self.apply(ctx, Action::EndPhase).is_none() {
                        macroquad::logging::error!("the AI phase could not be ended");
                    }
                }
            }
            return;
        };
        let id = step.unit;
        if !self.state.can_act(id) || self.state.phase != side {
            self.ai = None;
            return;
        }
        let plan = if step.tries < 2 {
            self.state.ai_actions(&self.pack, id).into_iter().next()
        } else {
            None
        };
        let Some(action) = plan else {
            self.ai_wait(ctx, id);
            return;
        };
        let is_move = matches!(action, Action::Move { .. });
        if self.apply(ctx, action).is_none() {
            self.ai_wait(ctx, id);
            return;
        }
        if is_move && self.state.can_act(id) {
            self.ai = Some(AiStep {
                unit: id,
                tries: step.tries + 1,
            });
        } else {
            if self.state.can_act(id) {
                self.ai_wait(ctx, id);
            }
            self.ai = None;
        }
    }

    /// Let an AI unit wait (planning failed or produced nothing); if even that is refused, end
    /// the phase so the battle cannot get stuck.
    fn ai_wait(&mut self, ctx: &mut Ctx, id: UnitId) {
        self.ai = None;
        if self.apply(ctx, Action::Wait { unit: id }).is_none() {
            self.apply(ctx, Action::EndPhase);
        }
    }

    // ----- player phase ------------------------------------------------------------------

    fn open_battle_menu(&mut self, ctx: &mut Ctx) {
        let items: Vec<MenuItem> = BattleMenuItem::ALL
            .iter()
            .map(|i| MenuItem::new(i.label()))
            .collect();
        let mut menu = Menu::new(items);
        let w = menu.fit_width(&ctx.gfx).max(110.0);
        let h = menu.rect().h;
        menu.set_width(w);
        menu.set_position(
            ((VIRTUAL_W - w) / 2.0).round(),
            (VIEWPORT.y + (VIEWPORT.h - h) / 2.0).round(),
        );
        ctx.sfx(sfx::CONFIRM);
        self.panel = Panel::Menu(menu);
    }

    fn open_unit_list(&mut self, ctx: &Ctx, side: Side) {
        let ids: Vec<UnitId> = self
            .state
            .units
            .iter()
            .filter(|u| u.side == side && u.is_active())
            .map(|u| u.id)
            .collect();
        let items: Vec<MenuItem> = ids
            .iter()
            .map(|&id| {
                let u = &self.state.units[id];
                let class = self
                    .pack
                    .class(&u.class)
                    .map_or(u.class.as_str(), |c| c.name.as_str());
                let mut label = format!("{class} Lv{}", u.level);
                if side == Side::Player && u.acted {
                    label.push_str(" (행동 끝)");
                }
                MenuItem::new(label)
                    .tag(u.name.clone())
                    .detail(format!("{}/{}", u.hp, u.max_hp))
            })
            .collect();
        let empty = items.is_empty();
        let mut menu = Menu::new(if empty {
            vec![MenuItem::new("— 없음 —").enabled(false)]
        } else {
            items
        })
        .rows(9);
        menu.tag_width = 64.0;
        menu.wrap = false;
        let w = 300.0;
        menu.set_width(w);
        menu.set_position(((VIRTUAL_W - w) / 2.0).round(), VIEWPORT.y + 30.0);
        let _ = ctx;
        self.panel = Panel::Units { side, ids, menu };
    }

    /// Rebuild the command / strategy / item menu when the mode changed.
    fn refresh_mode_menu(&mut self, ctx: &Ctx) {
        let wanted = match &self.ui.mode {
            Mode::Command { .. } => Some(MenuKind::Command),
            Mode::Strategies { .. } => Some(MenuKind::Strategies),
            Mode::Items { .. } => Some(MenuKind::Items),
            _ => None,
        };
        if self.mode_menu.as_ref().map(|(k, _)| *k) == wanted {
            return;
        }
        let Some(kind) = wanted else {
            self.mode_menu = None;
            return;
        };
        let unit = self.ui.unit().expect("menu modes have a unit");
        let (items, width) = match &self.ui.mode {
            Mode::Command { .. } => (
                Command::ALL
                    .iter()
                    .map(|c| {
                        MenuItem::new(c.label()).enabled(player::command_enabled(
                            &self.state,
                            &self.pack,
                            unit,
                            *c,
                        ))
                    })
                    .collect::<Vec<_>>(),
                76.0,
            ),
            Mode::Strategies { list, .. } => (
                list.iter()
                    .map(|e| {
                        MenuItem::new(e.name.clone())
                            .tag(String::new())
                            .detail(format!("MP {}", e.mp))
                            .enabled(e.aims.is_ok())
                    })
                    .collect(),
                176.0,
            ),
            Mode::Items { list, .. } => (
                list.iter()
                    .map(|e| {
                        MenuItem::new(e.name.clone())
                            .tag(String::new())
                            .detail(format!("×{}", e.count))
                            .enabled(e.targets.is_ok())
                    })
                    .collect(),
                176.0,
            ),
            _ => return,
        };
        let mut menu = Menu::new(items).rows(7);
        if kind != MenuKind::Command {
            menu.tag_width = 18.0;
            menu.wrap = false;
        }
        let w = if kind == MenuKind::Command {
            menu.fit_width(&ctx.gfx).max(width)
        } else {
            width
        };
        menu.set_width(w);
        let h = menu.rect().h;
        let tile = self.camera.tile_screen(self.state.units[unit].pos);
        let (x, y) = if kind == MenuKind::Command {
            let x = if tile.x + TILE + 6.0 + w <= VIRTUAL_W - 4.0 {
                tile.x + TILE + 6.0
            } else {
                tile.x - w - 6.0
            };
            (x, tile.y + TILE / 2.0 - h / 2.0)
        } else {
            let x = if tile.x < VIRTUAL_W / 2.0 {
                VIRTUAL_W - w - 8.0
            } else {
                8.0
            };
            (x, VIEWPORT.y + 6.0)
        };
        let x = x.clamp(4.0, VIRTUAL_W - w - 4.0).round();
        let y = y.clamp(VIEWPORT.y + 4.0, VIRTUAL_H - h - 4.0).round();
        menu.set_position(x, y);
        self.mode_menu = Some((kind, menu));
    }

    /// Carry out a decision of the command state machine.
    fn handle_request(&mut self, ctx: &mut Ctx, req: Request) {
        match req {
            Request::None => {}
            Request::Invalid => ctx.sfx(sfx::ERROR),
            Request::BattleMenu => self.open_battle_menu(ctx),
            Request::Restore(snapshot) => {
                self.state = *snapshot;
                self.store_session(ctx);
                self.scene.sync(&self.state);
                if let Some(u) = self.ui.unit() {
                    self.cursor = self.state.units[u].pos;
                }
                ctx.sfx(sfx::CANCEL);
            }
            Request::Apply(action) => {
                let moving = matches!(action, Action::Move { .. });
                match self.apply(ctx, action) {
                    Some(events) => {
                        if moving {
                            self.move_undoable = player::move_is_undoable(&events);
                        } else {
                            ctx.sfx(sfx::CONFIRM);
                        }
                    }
                    None => self.ui.reset(),
                }
            }
        }
        self.refresh_mode_menu(ctx);
    }

    /// Target list of the attack / item target modes (for keyboard cycling).
    fn cycle_targets(&self) -> Vec<Pos> {
        match &self.ui.mode {
            Mode::Attack { targets, .. } => {
                targets.iter().map(|&t| self.state.units[t].pos).collect()
            }
            Mode::ItemTarget { list, index, .. } => match &list[*index].targets {
                Ok(t) => t.iter().map(|&u| self.state.units[u].pos).collect(),
                Err(_) => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn is_target_mode(&self) -> bool {
        matches!(
            self.ui.mode,
            Mode::Attack { .. } | Mode::Aim { .. } | Mode::ItemTarget { .. }
        )
    }

    /// Put the cursor on the first target when a target mode opens.
    fn snap_cursor_to_target(&mut self) {
        let first = match &self.ui.mode {
            Mode::Aim { list, index, .. } => {
                let aims = list[*index].aims.as_ref().ok();
                aims.and_then(|a| {
                    a.iter()
                        .copied()
                        .find(|p| self.state.unit_at(*p).is_some())
                        .or_else(|| a.first().copied())
                })
            }
            _ => self.cycle_targets().first().copied(),
        };
        if let Some(p) = first {
            self.cursor = p;
            self.camera.keep_visible(p, 2.0);
        }
    }

    fn description_rect(&self, list: Rect) -> Rect {
        Rect::new(list.x, list.bottom() + 4.0, list.w, 40.0)
    }

    fn player_update(&mut self, ctx: &mut Ctx, dt: f32) -> Transition {
        // Modal dialog.
        if let Some((dialog, kind)) = self.dialog.as_mut() {
            let kind = *kind;
            match dialog.update(ctx) {
                ConfirmEvent::Yes => {
                    self.dialog = None;
                    match kind {
                        Confirm::EndTurn => {
                            self.ui.reset();
                            self.refresh_mode_menu(ctx);
                            self.apply(ctx, Action::EndPhase);
                        }
                        Confirm::Title => return Transition::Flow(Flow::Title),
                    }
                }
                ConfirmEvent::No => self.dialog = None,
                ConfirmEvent::None => {}
            }
            return Transition::None;
        }

        // Battle menu and its windows.
        match std::mem::replace(&mut self.panel, Panel::None) {
            Panel::None => {}
            Panel::Menu(mut menu) => {
                match menu.update(ctx) {
                    MenuEvent::Selected(i) => return self.battle_menu(ctx, BattleMenuItem::ALL[i]),
                    MenuEvent::Cancelled => {}
                    _ => self.panel = Panel::Menu(menu),
                }
                return Transition::None;
            }
            Panel::Units {
                side,
                ids,
                mut menu,
            } => {
                let switch = if ctx.input.nav() == Some(crate::input::Dir::Left) {
                    Some(-1)
                } else if ctx.input.nav() == Some(crate::input::Dir::Right) {
                    Some(1)
                } else {
                    ctx.input.tap().and_then(unit_tab_at)
                };
                if let Some(d) = switch {
                    let sides = [Side::Player, Side::Ally, Side::Enemy];
                    let next = match d {
                        -1 | 1 => {
                            let i = sides.iter().position(|s| *s == side).unwrap_or(0) as i32;
                            sides[(i + d).rem_euclid(3) as usize]
                        }
                        i => sides[(i - 10) as usize],
                    };
                    ctx.sfx(sfx::CURSOR);
                    ctx.input.consume();
                    self.open_unit_list(ctx, next);
                    return Transition::None;
                }
                match menu.update(ctx) {
                    MenuEvent::Selected(i) => {
                        if let Some(&u) = ids.get(i) {
                            self.cursor = self.state.units[u].pos;
                            self.camera.center_on(self.cursor);
                        }
                    }
                    MenuEvent::Cancelled => {}
                    _ => self.panel = Panel::Units { side, ids, menu },
                }
                return Transition::None;
            }
            Panel::Objective => {
                if ctx.input.confirm() || ctx.input.cancel() {
                    ctx.sfx(sfx::CANCEL);
                    ctx.input.consume();
                } else {
                    self.panel = Panel::Objective;
                }
                return Transition::None;
            }
        }

        // Command / strategy / item menus.
        if let Some((kind, menu)) = self.mode_menu.as_mut() {
            let kind = *kind;
            let ev = menu.update(ctx);
            let over = ctx.input.pointer().is_some_and(|p| menu.rect().contains(p));
            match ev {
                MenuEvent::Selected(i) => {
                    let req = self.ui.choose(&self.state, &self.pack, i);
                    self.handle_request(ctx, req);
                    if self.is_target_mode() {
                        self.snap_cursor_to_target();
                    }
                }
                MenuEvent::Cancelled => {
                    let req = self.ui.cancel(&self.state, &self.pack);
                    self.handle_request(ctx, req);
                }
                _ => {
                    // A tap outside the menu steps back (touch friendly).
                    if ctx.input.tap().is_some() && !over {
                        ctx.sfx(sfx::CANCEL);
                        let req = self.ui.cancel(&self.state, &self.pack);
                        self.handle_request(ctx, req);
                    }
                }
            }
            let _ = kind;
            return Transition::None;
        }

        self.map_input(ctx, dt);

        // Everyone has acted: end the phase by itself.
        if self.ui.mode == Mode::Browse
            && self.state.phase == Side::Player
            && player::cycle_actor(&self.state, None, 1).is_none()
        {
            self.idle_time += dt;
            if self.idle_time >= AUTO_END_DELAY {
                self.idle_time = 0.0;
                self.apply(ctx, Action::EndPhase);
            }
        } else {
            self.idle_time = 0.0;
        }
        Transition::None
    }

    fn battle_menu(&mut self, ctx: &mut Ctx, item: BattleMenuItem) -> Transition {
        match item {
            BattleMenuItem::EndTurn => {
                if player::cycle_actor(&self.state, None, 1).is_some() {
                    self.dialog = Some((
                        ConfirmDialog::new(
                            &ctx.gfx,
                            "아직 행동하지 않은 부대가 있습니다. 턴을 종료할까요?",
                        ),
                        Confirm::EndTurn,
                    ));
                } else {
                    self.ui.reset();
                    self.apply(ctx, Action::EndPhase);
                }
                Transition::None
            }
            BattleMenuItem::Units => {
                self.open_unit_list(ctx, Side::Player);
                Transition::None
            }
            BattleMenuItem::Objective => {
                self.panel = Panel::Objective;
                Transition::None
            }
            BattleMenuItem::Save => match ctx.session.as_ref() {
                Some(session) => {
                    let save = session.to_save(&self.pack);
                    self.waiting = Some(Waiting::Screen);
                    Transition::push(SaveLoadScreen::save(save))
                }
                None => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast("기록할 게임이 없습니다.");
                    Transition::None
                }
            },
            BattleMenuItem::Settings => {
                self.waiting = Some(Waiting::Screen);
                Transition::push(SettingsScreen::new())
            }
            BattleMenuItem::Title => {
                self.dialog = Some((
                    ConfirmDialog::new(
                        &ctx.gfx,
                        "전투를 중단하고 타이틀로 돌아갈까요? 기록하지 않은 진행은 사라집니다.",
                    )
                    .default_no(),
                    Confirm::Title,
                ));
                Transition::None
            }
        }
    }

    /// Cursor, camera and map clicks while no menu is open.
    fn map_input(&mut self, ctx: &mut Ctx, dt: f32) {
        let input = &ctx.input;
        if input.pressed() {
            self.cursor_at_press = Some(self.cursor);
        }
        let pointer = input.pointer();
        let hover = pointer.and_then(|p| self.camera.tile_at(p));

        // Camera: left drag (touch / mouse), right drag, wheel, mouse at the edges.
        if let Some(drag) = input.drag() {
            if VIEWPORT.contains(drag.origin) {
                self.camera.pan(-drag.delta);
            }
        }
        let right_down = is_mouse_button_down(MouseButton::Right);
        let mut right_click = false;
        match (self.rdrag, right_down, pointer) {
            (None, true, Some(p)) if input.right_click() => {
                self.rdrag = Some(RightDrag {
                    origin: p,
                    last: p,
                    moved: false,
                });
            }
            (Some(mut r), true, Some(p)) => {
                if !r.moved && r.origin.distance(p) > crate::input::DRAG_THRESHOLD {
                    r.moved = true;
                }
                if r.moved {
                    self.camera.pan(r.last - p);
                }
                r.last = p;
                self.rdrag = Some(r);
            }
            (Some(r), false, _) | (Some(r), true, None) => {
                self.rdrag = None;
                right_click = !r.moved;
            }
            _ => {}
        }
        let wheel = input.wheel();
        if wheel != 0 && pointer.is_some_and(|p| VIEWPORT.contains(p)) {
            if input.key_down(KeyCode::LeftShift) || input.key_down(KeyCode::RightShift) {
                self.camera.pan(vec2(wheel as f32 * 32.0, 0.0));
            } else {
                self.camera.pan(vec2(0.0, wheel as f32 * 32.0));
            }
        }
        if !touches().is_empty() {
            self.touch_seen = true;
        }
        if !self.touch_seen && self.rdrag.is_none() && !input.down() {
            if let Some(p) = pointer {
                let d = edge_direction(VIEWPORT, p);
                if d != Vec2::ZERO {
                    self.camera.pan(d * EDGE_PAN_SPEED * dt);
                }
            }
        }

        // Hover moves the cursor (real pointer movement only).
        if input.pointer_moved() && input.drag().is_none() && self.rdrag.is_none_or(|r| !r.moved) {
            if let Some(t) = hover {
                self.cursor = t;
            }
        }

        // Keyboard.
        let key_cancel = input.cancel() && !input.right_click();
        if let Some(dir) = input.nav() {
            let targets = self.cycle_targets();
            if !targets.is_empty() {
                let i = targets.iter().position(|p| *p == self.cursor);
                let step = match dir {
                    crate::input::Dir::Left | crate::input::Dir::Up => -1,
                    _ => 1,
                };
                let n = targets.len() as i32;
                let next = match i {
                    Some(i) => (i as i32 + step).rem_euclid(n) as usize,
                    None => 0,
                };
                self.cursor = targets[next];
            } else {
                let (dx, dy) = dir.delta();
                let m = &self.state.map;
                self.cursor = Pos::new(
                    (self.cursor.x + dx).clamp(0, m.width - 1),
                    (self.cursor.y + dy).clamp(0, m.height - 1),
                );
            }
            ctx.sfx(sfx::CURSOR);
            self.camera.keep_visible(self.cursor, 2.0);
        }
        let cycle = if ctx.input.key_pressed(KeyCode::Tab) || ctx.input.key_pressed(KeyCode::E) {
            Some(1)
        } else if ctx.input.key_pressed(KeyCode::Q) {
            Some(-1)
        } else {
            None
        };
        if let Some(step) = cycle {
            if matches!(
                self.ui.mode,
                Mode::Browse | Mode::Move { .. } | Mode::Inspect { .. }
            ) {
                let from = self.ui.unit().or_else(|| self.state.unit_at(self.cursor));
                match player::cycle_actor(&self.state, from, step) {
                    Some(u) => {
                        self.cursor = self.state.units[u].pos;
                        self.camera.center_on(self.cursor);
                        self.ui.select(&self.state, &self.pack, u);
                        ctx.sfx(sfx::CURSOR);
                    }
                    None => ctx.sfx(sfx::ERROR),
                }
            }
        }

        // Confirm: key on the cursor, tap on a tile.
        let tap_tile = ctx.input.tap().and_then(|p| self.camera.tile_at(p));
        let mut confirm_at = None;
        if ctx.input.confirm_key() {
            confirm_at = Some(self.cursor);
        } else if let Some(t) = tap_tile {
            let previewed = self.cursor_at_press == Some(t);
            self.cursor = t;
            // In target modes the first tap only previews (touch has no hover).
            if !self.is_target_mode() || previewed {
                confirm_at = Some(t);
            } else {
                ctx.sfx(sfx::CURSOR);
            }
        }
        if let Some(at) = confirm_at {
            let before = std::mem::discriminant(&self.ui.mode);
            let req = self.ui.confirm(&self.state, &self.pack, at);
            let changed = std::mem::discriminant(&self.ui.mode) != before;
            if changed || matches!(req, Request::Apply(_)) {
                ctx.sfx(sfx::CONFIRM);
            }
            self.handle_request(ctx, req);
            return;
        }
        if key_cancel || right_click {
            let had_selection = self.ui.mode != Mode::Browse;
            let req = self.ui.cancel(&self.state, &self.pack);
            if had_selection {
                ctx.sfx(sfx::CANCEL);
                if let Some(u) = self.ui.unit() {
                    self.cursor = self.state.units[u].pos;
                }
            }
            if matches!(self.ui.mode, Mode::Command { .. }) {
                // Stepped back from a target mode: the cursor goes back to the unit.
                if let Some(u) = self.ui.unit() {
                    self.cursor = self.state.units[u].pos;
                }
            }
            self.handle_request(ctx, req);
        }
    }

    // ----- drawing -----------------------------------------------------------------------

    fn draw_map(&self, ctx: &Ctx) {
        let tileset = self.meta.tileset.as_ref();
        let atlas = tileset.and_then(|ts| ctx.media.texture(&ts.texture));
        self.map.draw(
            self.camera.to_screen(Vec2::ZERO),
            &self.state.map,
            tileset,
            atlas.as_ref(),
            ctx.time,
        );
        // Untaken treasures twinkle.
        for (i, t) in self.def().treasures.iter().enumerate() {
            if !self.state.treasures_taken.get(i).copied().unwrap_or(false) {
                hud::draw_twinkle(self.camera.tile_screen(t.pos), ctx.time);
            }
        }
    }

    fn draw_highlights(&self, ctx: &Ctx) {
        let t = ctx.time;
        let hl = |p: Pos, c: Color| hud::draw_tile_highlight(self.camera.tile_screen(p), c, t);
        match &self.ui.mode {
            Mode::Move { range, reach, unit } | Mode::Inspect { range, reach, unit } => {
                let own = matches!(self.ui.mode, Mode::Move { .. });
                let move_color = if own {
                    hud::MOVE_COLOR
                } else {
                    hud::MOVE_COLOR.with_alpha(0.26)
                };
                for p in range.tiles.keys() {
                    hl(*p, move_color);
                }
                for p in reach {
                    hl(*p, hud::REACH_COLOR);
                }
                // Path preview to the cursor.
                if own && self.cursor != self.state.units[*unit].pos {
                    if let Some(path) = range.path_to(self.cursor) {
                        for w in path.windows(2) {
                            let a = self.camera.tile_screen(w[0]) + vec2(TILE / 2.0, TILE / 2.0);
                            let b = self.camera.tile_screen(w[1]) + vec2(TILE / 2.0, TILE / 2.0);
                            draw_line(a.x, a.y, b.x, b.y, 3.0, Color::new(1.0, 0.9, 0.4, 0.85));
                        }
                        let end =
                            self.camera.tile_screen(self.cursor) + vec2(TILE / 2.0, TILE / 2.0);
                        draw_circle(end.x, end.y, 3.0, Color::new(1.0, 0.9, 0.4, 0.95));
                    }
                }
            }
            Mode::Attack { unit, targets, .. } => {
                let pos = self.state.units[*unit].pos;
                for p in self.state.attack_tiles(&self.pack, *unit, pos) {
                    hl(p, hud::REACH_COLOR);
                }
                for &u in targets {
                    hl(self.state.units[u].pos, hud::TARGET_COLOR);
                }
            }
            Mode::Aim {
                unit, list, index, ..
            } => {
                let e = &list[*index];
                if let Ok(aims) = &e.aims {
                    for p in aims {
                        hl(*p, hud::AIM_COLOR);
                    }
                    if aims.contains(&self.cursor) {
                        let caster = self.state.units[*unit].pos;
                        for p in player::area_tiles(&self.pack, &e.id, caster, self.cursor) {
                            if self.state.map.in_bounds(p) {
                                hl(p, hud::AREA_COLOR);
                            }
                        }
                    }
                }
            }
            Mode::ItemTarget { list, index, .. } => {
                if let Ok(targets) = &list[*index].targets {
                    for &u in targets {
                        hl(self.state.units[u].pos, hud::ITEM_COLOR);
                    }
                }
            }
            _ => {}
        }
    }

    fn draw_units(&self, ctx: &Ctx) {
        if !self.meta.units_loaded() {
            return;
        }
        let mut order: Vec<usize> = (0..self.scene.views.len())
            .filter(|&i| self.scene.views[i].visible)
            .collect();
        order.sort_by(|&a, &b| {
            let (va, vb) = (&self.scene.views[a], &self.scene.views[b]);
            (va.pos.y + va.offset.y)
                .total_cmp(&(vb.pos.y + vb.offset.y))
                .then(a.cmp(&b))
        });
        let default_sprite = SpriteDef::default();
        for i in order {
            let v = &self.scene.views[i];
            let u = &self.state.units[i];
            let screen = self.camera.to_screen(v.pos + v.offset);
            if screen.x < -40.0
                || screen.x > VIRTUAL_W + 24.0
                || screen.y < -40.0
                || screen.y > VIRTUAL_H + 24.0
            {
                continue;
            }
            let sprite = self
                .sprite_of
                .get(&v.class)
                .map_or(v.class.as_str(), |s| s.as_str());
            let def = self.meta.sprites.get(sprite).unwrap_or(&default_sprite);
            let frame = vec2(def.frame[0] as f32, def.frame[1] as f32);
            let origin = sprites::frame_origin(screen, def);
            let flicker = v.flash > 0.0 && ((ctx.time * 30.0) as i64) % 2 == 0;
            let grey = if v.acted { 0.5 } else { 1.0 };
            let tint = Color::new(grey, grey, grey + if v.acted { 0.05 } else { 0.0 }, v.alpha);
            // Shadow under the unit.
            draw_ellipse(
                screen.x + TILE / 2.0,
                screen.y + TILE - 2.0,
                6.0,
                2.0,
                0.0,
                Color::new(0.0, 0.0, 0.0, 0.28 * v.alpha),
            );
            if !flicker {
                let key = self.sheets.get(&(sprite.to_string(), v.side));
                match key.map(|k| (k, ctx.media.texture_state(k))) {
                    Some((k, AssetState::Ready)) => {
                        if let Some(tex) = ctx.media.texture(k) {
                            let row =
                                sprites::pose_row(v.pose, ctx.time + i as f64 * 0.37, !v.acted);
                            hud::draw_frame(
                                &tex,
                                frame,
                                (sprites::facing_column(v.facing), row),
                                origin,
                                tint,
                            );
                        }
                    }
                    Some((_, AssetState::Loading)) => {}
                    _ => draw_placeholder(
                        Rect::new(screen.x + 2.0, screen.y + 1.0, 12.0, 14.0),
                        sprite,
                    ),
                }
            }
            if v.alpha > 0.05 {
                if u.commander {
                    hud::draw_flag(
                        ctx,
                        screen + vec2(7.0, -14.0),
                        v.side,
                        ctx.time + i as f64 * 0.2,
                        v.alpha,
                    );
                }
                if u.lord {
                    hud::draw_crown(screen + vec2(0.0, -8.0), v.alpha);
                }
                if v.confused {
                    hud::draw_confusion(screen + vec2(TILE / 2.0, -9.0), ctx.time, v.alpha);
                }
                if v.knocked.is_none() {
                    hud::draw_mini_hp(screen, v.hp, v.max_hp, v.alpha);
                }
            }
        }
    }

    fn draw_fx(&self, ctx: &Ctx) {
        for f in &self.scene.fx {
            let (Some(def), Some(key)) =
                (self.meta.fx.get(&f.key), self.meta.fx_textures.get(&f.key))
            else {
                continue;
            };
            let Some(frame) = def.frame_at(f.age) else {
                continue;
            };
            let Some(tex) = ctx.media.texture(key) else {
                continue;
            };
            let size = vec2(def.frame[0] as f32, def.frame[1] as f32);
            let c = self.camera.to_screen(f.center);
            hud::draw_frame(&tex, size, (frame, 0), c - size / 2.0, WHITE);
        }
    }

    /// Unit shown in the info panel: the one under the cursor, else the selected / acting one.
    fn panel_unit(&self) -> Option<UnitId> {
        if !self.events.is_idle() {
            return self.events.focus_unit().or(self.ai.map(|a| a.unit));
        }
        self.state.unit_at(self.cursor).or_else(|| self.ui.unit())
    }

    /// Whether the info panels go to the top (the cursor is in the lower half).
    fn panels_on_top(&self) -> bool {
        let s = self.camera.tile_screen(self.cursor);
        s.y > VIEWPORT.y + VIEWPORT.h * 0.55
    }

    fn draw_panels(&self, ctx: &Ctx) {
        let top = self.panels_on_top();
        let y_unit = if top {
            VIEWPORT.y + 4.0
        } else {
            VIRTUAL_H - hud::UNIT_PANEL.y - 4.0
        };
        if let Some(u) = self.panel_unit() {
            if self.state.units[u].is_active() || self.scene.views[u].visible {
                hud::draw_unit_panel(
                    ctx,
                    vec2(4.0, y_unit),
                    &self.pack,
                    &self.state,
                    u,
                    &self.scene.views[u],
                );
            }
        }
        if self.events.is_idle() && self.state.phase == Side::Player {
            if let Some(t) = self.state.terrain_at(&self.pack, self.cursor) {
                let treasure = self.def().treasures.iter().enumerate().any(|(i, tr)| {
                    tr.pos == self.cursor
                        && !self.state.treasures_taken.get(i).copied().unwrap_or(false)
                });
                let y = if top {
                    VIEWPORT.y + 4.0
                } else {
                    VIRTUAL_H - hud::TERRAIN_PANEL.y - 4.0
                };
                hud::draw_terrain_panel(
                    ctx,
                    vec2(VIRTUAL_W - hud::TERRAIN_PANEL.x - 4.0, y),
                    t,
                    treasure,
                    !t.cost.is_empty(),
                );
            }
        }
    }

    /// Forecast window for the target under the cursor (attack, strategy, item).
    fn draw_forecast(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let Some(unit) = self.ui.unit() else {
            return;
        };
        let top = !self.panels_on_top();
        let place = |h: f32, w: f32| {
            let x = ((VIRTUAL_W - w) / 2.0).round();
            let y = if top {
                VIEWPORT.y + 4.0
            } else {
                VIRTUAL_H - h - 4.0
            };
            Rect::new(x, y, w, h)
        };
        let main = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let small = TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW);
        let me = &self.state.units[unit];
        match &self.ui.mode {
            Mode::Attack { targets, .. } => {
                let Some(t) = self
                    .state
                    .unit_at(self.cursor)
                    .filter(|t| targets.contains(t))
                else {
                    return;
                };
                let f = self.state.forecast_attack(&self.pack, unit, t);
                let target = &self.state.units[t];
                let lines = text::attack_lines(&f, target.hp);
                let r = place(66.0, 188.0);
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} → {}", me.name, target.name),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                let dw = gfx.text(
                    &lines.damage,
                    r.x + 10.0,
                    r.y + 20.0,
                    main.color(theme::TEXT_ACCENT),
                );
                if let Some(up) = text::affinity_mark(f.affinity) {
                    hud::draw_affinity_arrow(vec2(r.x + 14.0 + dw, r.y + 25.0), up);
                    gfx.text(
                        if up { "상성 유리" } else { "상성 불리" },
                        r.x + 26.0 + dw,
                        r.y + 22.0,
                        small.color(if up {
                            theme::TEXT_GOOD
                        } else {
                            theme::TEXT_BAD
                        }),
                    );
                }
                gfx.text(
                    &lines.result,
                    r.x + 10.0,
                    r.y + 36.0,
                    small.color(if lines.defeats {
                        theme::TEXT_GOOD
                    } else {
                        theme::TEXT
                    }),
                );
                gfx.text(
                    &lines.counter,
                    r.x + 10.0,
                    r.y + 49.0,
                    small.color(if f.counter.is_some() {
                        theme::TEXT_BAD
                    } else {
                        theme::TEXT_DIM
                    }),
                );
            }
            Mode::Aim { list, index, .. } => {
                let e = &list[*index];
                if !e.aims.as_ref().is_ok_and(|a| a.contains(&self.cursor)) {
                    return;
                }
                let fs = self
                    .state
                    .forecast_strategy(&self.pack, unit, &e.id, self.cursor);
                let shown = fs.len().min(5);
                let r = place(
                    24.0 + shown as f32 * 13.0 + if fs.len() > 5 { 12.0 } else { 0.0 },
                    210.0,
                );
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} · {} (MP {})", me.name, e.name, e.mp),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                for (i, f) in fs.iter().take(5).enumerate() {
                    let y = r.y + 20.0 + i as f32 * 13.0;
                    let name = &self.state.units[f.unit].name;
                    gfx.text(
                        name,
                        r.x + 10.0,
                        y,
                        small.color(hud::side_color(self.state.units[f.unit].side)),
                    );
                    gfx.text(
                        &text::strategy_line(f),
                        r.x + 78.0,
                        y,
                        small.color(theme::TEXT),
                    );
                }
                if fs.len() > 5 {
                    gfx.text(
                        &format!("외 {}부대", fs.len() - 5),
                        r.x + 10.0,
                        r.y + 20.0 + 5.0 * 13.0,
                        small,
                    );
                }
            }
            Mode::ItemTarget { list, index, .. } => {
                let e = &list[*index];
                let Some(t) = self.state.unit_at(self.cursor) else {
                    return;
                };
                if !e.targets.as_ref().is_ok_and(|ts| ts.contains(&t)) {
                    return;
                }
                let target = &self.state.units[t];
                let r = place(38.0, 188.0);
                draw_window_ex(r, WindowStyle::Normal, 0.96);
                gfx.text(
                    &format!("{} → {}", e.name, target.name),
                    r.x + 8.0,
                    r.y + 4.0,
                    main.color(theme::TEXT_NAME),
                );
                let line = match &e.strategy {
                    Some(s) => {
                        let fs = self
                            .state
                            .forecast_strategy(&self.pack, unit, s, target.pos);
                        fs.iter()
                            .find(|f| f.unit == t)
                            .map_or_else(|| "효과 없음".to_string(), text::strategy_line)
                    }
                    None => item_effect_line(&self.pack, &e.id, target),
                };
                gfx.text(&line, r.x + 10.0, r.y + 20.0, small.color(theme::TEXT));
            }
            _ => {}
        }
    }

    fn draw_mode_menu(&self, ctx: &Ctx) {
        let Some((kind, menu)) = &self.mode_menu else {
            return;
        };
        menu.draw(ctx);
        if *kind == MenuKind::Command {
            return;
        }
        // Icons in the tag column and the description of the highlighted row.
        let cursor = menu.cursor();
        let (desc, blocked, icons): (String, Option<&'static str>, Vec<Option<String>>) =
            match &self.ui.mode {
                Mode::Strategies { list, .. } => (
                    list.get(cursor).map_or(String::new(), |e| {
                        let el = text::element_name(e.element.as_deref());
                        if el.is_empty() {
                            e.desc.clone()
                        } else {
                            format!("[{el}] {}", e.desc)
                        }
                    }),
                    list.get(cursor)
                        .and_then(|e| e.aims.as_ref().err().map(|b| b.text())),
                    list.iter()
                        .map(|e| {
                            self.pack
                                .strategy(&e.id)
                                .map(|s| strategy_icon(s).to_string())
                        })
                        .collect(),
                ),
                Mode::Items { list, .. } => (
                    list.get(cursor).map_or(String::new(), |e| e.desc.clone()),
                    list.get(cursor)
                        .and_then(|e| e.targets.as_ref().err().map(|b| b.text())),
                    list.iter()
                        .map(|e| Some(e.icon.clone()).filter(|i| !i.is_empty()))
                        .collect(),
                ),
                _ => return,
            };
        for (i, icon) in icons.iter().enumerate() {
            if let Some(icon) = icon {
                let row = menu.row_rect(i);
                if row.y >= menu.rect().y && row.bottom() <= menu.rect().bottom() {
                    draw_icon(ctx, icon, vec2(row.x + 11.0, row.y));
                }
            }
        }
        let r = self.description_rect(menu.rect());
        draw_window_ex(r, WindowStyle::Panel, 0.96);
        let lines = ctx.gfx.wrap(&desc, FontId::Small, 1, r.w - 14.0);
        let small = TextStyle::small(theme::TEXT).shadow(theme::TEXT_SHADOW);
        let mut y = r.y + 4.0;
        if let Some(b) = blocked {
            ctx.gfx.text(b, r.x + 7.0, y, small.color(theme::TEXT_BAD));
            y += 12.0;
        }
        let room = ((r.bottom() - 4.0 - y) / 12.0).floor().max(0.0) as usize;
        ctx.gfx
            .text_lines(&lines[..lines.len().min(room)], r.x + 7.0, y, small);
    }

    fn draw_panel(&self, ctx: &Ctx) {
        match &self.panel {
            Panel::None => {}
            Panel::Menu(menu) => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.25));
                menu.draw(ctx);
            }
            Panel::Units { side, menu, .. } => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.3));
                let m = menu.rect();
                let frame = Rect::new(m.x - 4.0, m.y - 26.0, m.w + 8.0, m.h + 30.0);
                draw_window(frame);
                for (i, s) in [Side::Player, Side::Ally, Side::Enemy].iter().enumerate() {
                    let tab = unit_tab_rect(i);
                    let active = s == side;
                    draw_window_ex(
                        tab,
                        if active {
                            WindowStyle::Normal
                        } else {
                            WindowStyle::Panel
                        },
                        1.0,
                    );
                    ctx.gfx.text_aligned(
                        text::side_name(*s),
                        tab.x,
                        tab.y + 1.0,
                        tab.w,
                        Align::Center,
                        TextStyle::main(if active {
                            hud::side_color(*s)
                        } else {
                            theme::TEXT_DIM
                        })
                        .shadow(theme::TEXT_SHADOW),
                    );
                }
                menu.draw(ctx);
                ctx.gfx.text_aligned(
                    "←/→ 진영 · Z 이동 · X 닫기",
                    frame.x,
                    frame.bottom() + 2.0,
                    frame.w,
                    Align::Right,
                    TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
                );
            }
            Panel::Objective => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.3));
                self.draw_objective(ctx, "Z / X 닫기");
            }
        }
    }

    fn objective_sections(&self) -> Vec<(String, Vec<(String, Color)>)> {
        let def = self.def();
        let name = |r: &str| self.ref_name(r);
        let mut victory: Vec<(String, Color)> = def
            .victory
            .iter()
            .map(|c| (text::condition_text(c, name), theme::TEXT))
            .collect();
        if victory.is_empty() {
            victory.push(("—".into(), theme::TEXT_DIM));
        }
        let mut defeat: Vec<(String, Color)> = Vec::new();
        if let Some(l) = self.state.units.iter().find(|u| u.lord) {
            defeat.push((format!("{} 퇴각", l.name), theme::TEXT));
        }
        defeat.extend(
            def.defeat
                .iter()
                .map(|c| (text::condition_text(c, name), theme::TEXT)),
        );
        defeat.push((format!("{}턴 경과", self.state.turn_limit), theme::TEXT));
        let mut sections = vec![
            ("승리 조건".to_string(), victory),
            ("패배 조건".to_string(), defeat),
        ];
        if let Some(b) = &def.bonus {
            let (line, color) = if self.state.bonus_done {
                (format!("{} (달성)", b.desc), theme::TEXT_GOOD)
            } else {
                (format!("{} (경험치 +{})", b.desc, b.exp), theme::TEXT)
            };
            sections.push(("보너스 목표".to_string(), vec![(line, color)]));
        }
        sections.push((
            "턴 제한".to_string(),
            vec![(
                format!(
                    "{} / {}턴",
                    self.state.turn.min(self.state.turn_limit),
                    self.state.turn_limit
                ),
                theme::TEXT,
            )],
        ));
        sections
    }

    fn draw_objective(&self, ctx: &Ctx, footer: &str) {
        let r = hud::draw_text_window(
            ctx,
            &self.def().objective,
            &self.objective_sections(),
            footer,
        );
        let _ = r;
    }
}

/// Rectangle of a side tab of the unit list (0 = 아군, 1 = 우군, 2 = 적군).
fn unit_tab_rect(i: usize) -> Rect {
    let x0 = (VIRTUAL_W - 300.0) / 2.0;
    Rect::new(x0 + 4.0 + i as f32 * 62.0, VIEWPORT.y + 6.0, 58.0, 19.0)
}

/// Tab under a tap in the unit list: `Some(10 + index)`.
fn unit_tab_at(p: Vec2) -> Option<i32> {
    (0..3)
        .find(|&i| unit_tab_rect(i).contains(p))
        .map(|i| 10 + i as i32)
}

/// Icon of a strategy: its element, or its effect for element-less strategies.
fn strategy_icon(s: &hero_core::data::StrategyDef) -> &'static str {
    use hero_core::data::{Effect, StrategyKind};
    match s.element.as_deref() {
        Some("fire") => "fire",
        Some("water") => "water",
        Some("earth") => "earth",
        _ => match s.kind {
            StrategyKind::Heal => "heal",
            _ => {
                if s.effects.iter().any(|e| matches!(e, Effect::Status { .. })) {
                    "confuse"
                } else if s
                    .effects
                    .iter()
                    .any(|e| matches!(e, Effect::Morale { amount } if *amount < 0))
                {
                    "morale_down"
                } else {
                    "morale_up"
                }
            }
        },
    }
}

/// Forecast of a healing / morale item on `target`.
fn item_effect_line(pack: &Pack, item: &str, target: &hero_core::battle::Unit) -> String {
    use hero_core::data::Effect;
    let Some(d) = pack.item(item) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for e in &d.effects {
        match e {
            Effect::Heal { power } => {
                let h = (*power).min(target.max_hp - target.hp).max(0);
                parts.push(format!("회복 {h}"));
            }
            Effect::Morale { amount } => {
                let m = (target.morale + amount).clamp(0, 100) - target.morale;
                parts.push(text::morale_text(m));
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        "효과 없음".into()
    } else {
        parts.join(" · ")
    }
}

impl Screen for BattleScreen {
    fn name(&self) -> &'static str {
        "battle"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        match how {
            Enter::Fresh => {
                self.start_loading(ctx);
                self.play_phase_music(ctx, self.state.phase);
            }
            Enter::Resumed => {
                match self.waiting.take() {
                    Some(Waiting::Drama) => self.events.resume(),
                    Some(Waiting::Screen) | None => {}
                }
                // A drama or the settings may have changed the music.
                if self.state.outcome.is_none() {
                    self.play_phase_music(ctx, self.scene.hud.phase);
                }
            }
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        self.poll_meta(ctx);
        let dt = ctx.dt;
        let speed = self.speed(ctx);
        self.camera.update(dt);
        self.scene.tick(dt * speed);

        match &mut self.stage {
            Stage::Title { age } => {
                *age += dt;
                let skip = *age > 0.4 && ctx.input.confirm();
                if *age >= TITLE_SECONDS || skip {
                    ctx.input.consume();
                    if self.fresh {
                        self.stage = Stage::Objective;
                        ctx.sfx(sfx::CONFIRM);
                    } else {
                        self.stage = Stage::Battle;
                        // Resumed mid-battle: announce whose phase it is.
                        let ev = [BattleEvent::PhaseStart {
                            side: self.state.phase,
                            turn: self.state.turn,
                        }];
                        self.events
                            .push(anim::plan(&ev, &self.state, &self.pack, &self.meta.fx));
                    }
                }
                return Transition::None;
            }
            Stage::Objective => {
                if ctx.input.confirm() || ctx.input.cancel() {
                    ctx.input.consume();
                    ctx.sfx(sfx::CONFIRM);
                    self.begin_battle(ctx);
                }
                return Transition::None;
            }
            Stage::Result { .. } => {
                if ctx.input.confirm() {
                    ctx.input.consume();
                    ctx.sfx(sfx::CONFIRM);
                    return Transition::Flow(Flow::BattleEnded(Box::new(self.state.clone())));
                }
                return Transition::None;
            }
            Stage::Battle => {}
        }

        // Animations first; input waits until they are done.
        if !self.events.is_idle() {
            let skip = ctx.input.confirm();
            let mut cues = std::mem::take(&mut self.cues);
            self.events
                .update(dt * speed, skip, &mut self.scene, &self.meta.fx, &mut cues);
            self.cues = cues;
            let t = self.handle_cues(ctx);
            if skip {
                ctx.input.consume();
            }
            if self.events.is_idle() {
                self.events_done();
                self.refresh_mode_menu(ctx);
            }
            return t;
        }
        if self.state.outcome.is_some() {
            self.open_result();
            return Transition::None;
        }
        if self.state.phase != Side::Player {
            self.ai_update(ctx, dt * speed);
            return Transition::None;
        }
        self.player_update(ctx, dt)
    }

    fn draw(&self, ctx: &Ctx) {
        fill_rect(SCREEN, Color::from_hex(0x0b0f1c));
        self.draw_map(ctx);
        let player_turn = matches!(self.stage, Stage::Battle)
            && self.events.is_idle()
            && self.state.phase == Side::Player
            && self.state.outcome.is_none();
        if player_turn {
            self.draw_highlights(ctx);
        }
        self.draw_units(ctx);
        self.draw_fx(ctx);
        let show_cursor = matches!(self.stage, Stage::Battle)
            && self.state.outcome.is_none()
            && (player_turn || self.ai.is_some());
        if show_cursor {
            let color = if self.is_target_mode() {
                Color::from_hex(0xff6a5a)
            } else {
                theme::CURSOR_ARROW
            };
            hud::draw_cursor(self.camera.tile_screen(self.cursor), ctx.time, color);
        }
        for f in &self.scene.floats {
            hud::draw_float(&ctx.gfx, self.camera.to_screen(f.at), f);
        }
        hud::draw_top_bar(
            ctx,
            &self.def().name,
            &self.scene.hud,
            self.state.turn_limit,
            ctx.session.as_ref().map_or(0, |s| s.campaign.gold),
        );

        match &self.stage {
            Stage::Title { age } => {
                let sub = if self.fresh {
                    self.def().location.clone()
                } else {
                    format!("{} — 이어서", self.def().location)
                };
                hud::draw_title_card(ctx, &self.def().name, &sub, *age);
                return;
            }
            Stage::Objective => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.02, 0.45));
                self.draw_objective(ctx, "Z / 클릭 — 출진");
                return;
            }
            Stage::Result { lines, title } => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.02, 0.5));
                hud::draw_text_window(ctx, title, lines, "Z / 클릭 — 계속");
                return;
            }
            Stage::Battle => {}
        }

        if player_turn && self.mode_menu.is_none() && matches!(self.panel, Panel::None) {
            self.draw_panels(ctx);
            self.draw_forecast(ctx);
        } else if !self.events.is_idle() || self.ai.is_some() {
            if self.scene.banner.is_none() && self.scene.popup.is_none() {
                self.draw_panels(ctx);
            }
        } else if player_turn {
            // A command menu is open: keep the acting unit's panel visible.
            self.draw_panels(ctx);
        }
        if player_turn {
            self.draw_mode_menu(ctx);
            self.draw_panel(ctx);
        }
        if let Some(c) = &self.scene.caption {
            hud::draw_caption(ctx, &c.text, c.age, VIEWPORT);
        }
        if let Some(b) = &self.scene.banner {
            hud::draw_banner(ctx, b, VIEWPORT);
        }
        if let Some(p) = &self.scene.popup {
            hud::draw_popup(ctx, p, &self.state, VIEWPORT);
        }
        if let Some(v) = self.scene.outcome {
            hud::draw_outcome(ctx, v, self.scene.outcome_age);
        }
        if let Some((dialog, _)) = &self.dialog {
            fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.35));
            dialog.draw(ctx);
        }
        if !self.events.is_idle()
            && self.state.phase != Side::Player
            && self.scene.outcome.is_none()
        {
            ctx.gfx.text_aligned(
                "Z 길게 누르기: 빨리 감기",
                0.0,
                VIRTUAL_H - 13.0,
                VIRTUAL_W - 6.0,
                Align::Right,
                TextStyle::small(theme::TEXT_DIM).shadow(theme::TEXT_SHADOW),
            );
        }
    }
}
