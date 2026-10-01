//! Player phase: the command state machine behind the map cursor and the battle menus.
//!
//! ```text
//! Browse ──own unit──▶ Move ──tile──▶ (walk) ──▶ Command ──공격──▶ Attack ──target──▶ apply
//!   │  ╰──other unit──▶ Inspect          ▲          │ ├─책략──▶ Strategies ──▶ Aim ──▶ apply
//!   ╰──empty tile──▶ battle menu        undo ◀─cancel │ ├─도구──▶ Items ──▶ ItemTarget ──▶ apply
//!                                                    ╰─대기──▶ apply
//! ```
//!
//! [`PlayerUi`] only decides; the screen turns input into calls ([`PlayerUi::confirm`],
//! [`PlayerUi::cancel`], [`PlayerUi::choose`]) and carries out the returned [`Request`]
//! (apply an action, restore the pre-move snapshot, open the battle menu). Cancelling the command
//! menu after a move restores a clone of the state taken before `Action::Move` — but only when
//! the move did nothing else (no treasure, no triggered event), so nothing that was shown is
//! taken back.

use hero_core::battle::{Action, BattleEvent, BattleState, MoveRange, UnitId, Weather};
use hero_core::battledef::Side;
use hero_core::data::{Area, Id, StatusKind, TargetSide};
use hero_core::geom::Pos;
use hero_core::pack::Pack;
use std::collections::BTreeSet;

/// Why a strategy or item cannot be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    NoMp,
    Rain,
    Terrain,
    NoTarget,
}

impl Blocked {
    pub fn text(self) -> &'static str {
        match self {
            Blocked::NoMp => "MP가 부족하다",
            Blocked::Rain => "비가 와서 쓸 수 없다",
            Blocked::Terrain => "지형이 맞지 않는다",
            Blocked::NoTarget => "범위 안에 대상이 없다",
        }
    }
}

/// One row of the strategy list.
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyEntry {
    pub id: Id,
    pub name: String,
    pub mp: i32,
    pub element: Option<String>,
    pub desc: String,
    /// Tiles it can be aimed at, or why it cannot be cast.
    pub aims: Result<Vec<Pos>, Blocked>,
}

/// One row of the item list.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemEntry {
    pub id: Id,
    pub name: String,
    pub icon: String,
    pub count: u32,
    pub desc: String,
    /// Strategy cast by a scroll.
    pub strategy: Option<Id>,
    pub targets: Result<Vec<UnitId>, Blocked>,
}

/// The four commands after moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Attack,
    Strategy,
    Item,
    Wait,
}

impl Command {
    pub const ALL: [Command; 4] = [
        Command::Attack,
        Command::Strategy,
        Command::Item,
        Command::Wait,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::Attack => "공격",
            Command::Strategy => "책략",
            Command::Item => "도구",
            Command::Wait => "대기",
        }
    }
}

/// Where the player is in the command flow.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// Free cursor.
    Browse,
    /// An own unit is selected: blue move range, red attack reach; pick a destination.
    Move {
        unit: UnitId,
        range: MoveRange,
        reach: Vec<Pos>,
    },
    /// Another side's (or an exhausted) unit is shown with its ranges.
    Inspect {
        unit: UnitId,
        range: MoveRange,
        reach: Vec<Pos>,
    },
    /// The move animation is playing; the command menu opens afterwards.
    Walking {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
    },
    Command {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
    },
    Attack {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
        targets: Vec<UnitId>,
    },
    Strategies {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
        list: Vec<StrategyEntry>,
    },
    Aim {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
        list: Vec<StrategyEntry>,
        index: usize,
    },
    Items {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
        list: Vec<ItemEntry>,
    },
    ItemTarget {
        unit: UnitId,
        undo: Option<Box<BattleState>>,
        list: Vec<ItemEntry>,
        index: usize,
    },
}

/// What the screen has to do after a decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    None,
    /// Apply this action to the battle.
    Apply(Action),
    /// Put this pre-move state back (undo an unconfirmed move).
    Restore(Box<BattleState>),
    /// Open the battle menu (턴 종료, 부대 일람, ...).
    BattleMenu,
    /// The input was not valid here (play the error sound).
    Invalid,
}

/// Map tiles the unit could attack after moving within `range` (excluding the move tiles).
pub fn reach_tiles(state: &BattleState, pack: &Pack, unit: UnitId, range: &MoveRange) -> Vec<Pos> {
    let mut from: Vec<Pos> = range.tiles.keys().copied().collect();
    if from.is_empty() {
        from.push(state.units[unit].pos);
    }
    let mut out = BTreeSet::new();
    for p in from {
        for t in state.attack_tiles(pack, unit, p) {
            if !range.tiles.contains_key(&t) {
                out.insert(t);
            }
        }
    }
    out.into_iter().collect()
}

/// Tiles some active enemy could attack in its next phase: its attack range from every tile of
/// its move range, as [`Mode::Inspect`] shows for one enemy (the "위험 범위" view option,
/// `docs/DECISIONS.md` D25 X4). Meant for the player's phase, when the enemies' move ranges are
/// those of their coming phase.
pub fn danger_tiles(state: &BattleState, pack: &Pack) -> Vec<Pos> {
    let mut out = BTreeSet::new();
    for (id, u) in state.units.iter().enumerate() {
        if u.side != Side::Enemy || !u.is_active() {
            continue;
        }
        let range = state.movement_range(pack, id);
        let mut from: Vec<Pos> = range.tiles.keys().copied().collect();
        if from.is_empty() {
            from.push(u.pos);
        }
        for p in from {
            out.extend(state.attack_tiles(pack, id, p));
        }
    }
    out.into_iter().collect()
}

/// What [`danger_tiles`] depends on that changes during a battle: unit positions, presence and
/// confusion (move points), the turn and terrain changes. The screen recomputes the tiles when
/// it differs.
pub fn danger_key(state: &BattleState) -> Vec<i64> {
    let mut key = vec![
        i64::from(state.turn),
        state.map_images.len() as i64,
        state.phase as i64,
    ];
    for u in &state.units {
        let flags = i64::from(u.is_active()) | i64::from(u.has_status(StatusKind::Confused)) << 1;
        key.extend([i64::from(u.pos.x), i64::from(u.pos.y), flags]);
    }
    key
}

/// Whether the player may give orders to `unit` now.
pub fn can_command(state: &BattleState, unit: UnitId) -> bool {
    let u = &state.units[unit];
    u.side == Side::Player && state.can_act(unit) && !u.has_status(StatusKind::Confused)
}

/// Next (or previous, `step = -1`) player unit that can still act, after `from`.
pub fn cycle_actor(state: &BattleState, from: Option<UnitId>, step: i32) -> Option<UnitId> {
    let ids: Vec<UnitId> = (0..state.units.len())
        .filter(|&i| can_command(state, i))
        .collect();
    if ids.is_empty() {
        return None;
    }
    let n = ids.len() as i32;
    let idx = match from.and_then(|f| ids.iter().position(|&i| i == f)) {
        Some(i) => (i as i32 + step).rem_euclid(n),
        None => {
            // Not an actor: the first actor after `from` in unit order (or the first).
            let after = from.map_or(0, |f| f + 1);
            let i = ids.iter().position(|&i| i >= after).unwrap_or(0) as i32;
            if step < 0 {
                (i - 1).rem_euclid(n)
            } else {
                i
            }
        }
    };
    Some(ids[idx as usize])
}

/// Which commands are possible for `unit` (standing where it is).
pub fn command_enabled(state: &BattleState, pack: &Pack, unit: UnitId, c: Command) -> bool {
    let u = &state.units[unit];
    match c {
        Command::Attack => !state.attack_targets(pack, unit, u.pos).is_empty(),
        Command::Strategy => {
            !u.has_status(StatusKind::Confused)
                && !pack.known_strategies(&u.class, u.level).is_empty()
        }
        Command::Item => !battle_items(state, pack).is_empty(),
        Command::Wait => true,
    }
}

/// Battle consumables of the army inventory, in inventory order: the items the engine accepts
/// in `Action::UseItem` (`ItemDef::is_battle_item`), so equipment marked `battle_use` by
/// mistake is not offered.
fn battle_items<'a>(state: &'a BattleState, pack: &'a Pack) -> Vec<(&'a Id, u32)> {
    state
        .inventory
        .iter()
        .filter(|(id, n)| **n > 0 && pack.item(id).is_some_and(|d| d.is_battle_item()))
        .map(|(id, n)| (id, *n))
        .collect()
}

/// Why a strategy has no aim (`aims` was empty): no MP is checked by the caller.
fn why_no_aim(state: &BattleState, pack: &Pack, unit: UnitId, strategy: &str) -> Blocked {
    let Some(s) = pack.strategy(strategy) else {
        return Blocked::NoTarget;
    };
    let u = &state.units[unit];
    let any_target = s
        .range
        .offsets()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|o| state.unit_at(u.pos.offset(o.x, o.y)))
        .any(|t| {
            let hostile = state.units[t].side.is_hostile(u.side);
            match s.target {
                TargetSide::Enemy => hostile,
                TargetSide::Ally => !hostile,
            }
        });
    if !any_target {
        Blocked::NoTarget
    } else if s.element.as_deref() == Some("fire") && state.weather == Weather::Rain {
        Blocked::Rain
    } else {
        Blocked::Terrain
    }
}

/// Every strategy the unit knows, with its aims or the reason it cannot be cast.
pub fn strategy_entries(state: &BattleState, pack: &Pack, unit: UnitId) -> Vec<StrategyEntry> {
    let u = &state.units[unit];
    pack.known_strategies(&u.class, u.level)
        .into_iter()
        .filter_map(|id| {
            let s = pack.strategy(&id)?;
            let aims = if u.mp < s.mp {
                Err(Blocked::NoMp)
            } else {
                let aims = state.strategy_targets(pack, unit, &id, u.pos);
                if aims.is_empty() {
                    Err(why_no_aim(state, pack, unit, &id))
                } else {
                    Ok(aims)
                }
            };
            Some(StrategyEntry {
                name: s.name.clone(),
                mp: s.mp,
                element: s.element.clone(),
                desc: s.desc.clone(),
                aims,
                id,
            })
        })
        .collect()
}

/// Battle consumables with their targets or the reason they cannot be used.
pub fn item_entries(state: &BattleState, pack: &Pack, unit: UnitId) -> Vec<ItemEntry> {
    battle_items(state, pack)
        .into_iter()
        .filter_map(|(id, count)| {
            let d = pack.item(id)?;
            let targets = state.item_targets(pack, unit, id);
            let targets = if targets.is_empty() {
                Err(match &d.strategy {
                    Some(s) => why_no_aim(state, pack, unit, s),
                    None => Blocked::NoTarget,
                })
            } else {
                Ok(targets)
            };
            Some(ItemEntry {
                id: id.clone(),
                name: d.name.clone(),
                icon: d.icon.clone(),
                count,
                desc: d.desc.clone(),
                strategy: d.strategy.clone(),
                targets,
            })
        })
        .collect()
}

/// Tiles a strategy aimed at `aim` would cover (for the area preview).
pub fn area_tiles(pack: &Pack, strategy: &str, caster: Pos, aim: Pos) -> Vec<Pos> {
    let Some(s) = pack.strategy(strategy) else {
        return Vec::new();
    };
    match s.area {
        Area::Single => vec![aim],
        Area::Cross => {
            let mut v = vec![aim];
            v.extend(aim.neighbors4());
            v
        }
        Area::AllInRange => s
            .range
            .offsets()
            .unwrap_or_default()
            .into_iter()
            .map(|o| caster.offset(o.x, o.y))
            .collect(),
    }
}

/// Whether the events of a move allow taking it back: only a plain `Moved` can be undone.
pub fn move_is_undoable(events: &[BattleEvent]) -> bool {
    matches!(events, [BattleEvent::Moved { .. }])
}

/// The player phase state machine. See the module docs.
#[derive(Debug, Clone)]
pub struct PlayerUi {
    pub mode: Mode,
}

impl Default for PlayerUi {
    fn default() -> Self {
        PlayerUi { mode: Mode::Browse }
    }
}

impl PlayerUi {
    /// Unit the current mode is about.
    pub fn unit(&self) -> Option<UnitId> {
        match &self.mode {
            Mode::Browse => None,
            Mode::Move { unit, .. }
            | Mode::Inspect { unit, .. }
            | Mode::Walking { unit, .. }
            | Mode::Command { unit, .. }
            | Mode::Attack { unit, .. }
            | Mode::Strategies { unit, .. }
            | Mode::Aim { unit, .. }
            | Mode::Items { unit, .. }
            | Mode::ItemTarget { unit, .. } => Some(*unit),
        }
    }

    fn take_undo(&mut self) -> Option<Box<BattleState>> {
        match &mut self.mode {
            Mode::Walking { undo, .. }
            | Mode::Command { undo, .. }
            | Mode::Attack { undo, .. }
            | Mode::Strategies { undo, .. }
            | Mode::Aim { undo, .. }
            | Mode::Items { undo, .. }
            | Mode::ItemTarget { undo, .. } => undo.take(),
            _ => None,
        }
    }

    /// Select the unit on a tile: own units that can act get their move range, others are
    /// inspected.
    pub fn select(&mut self, state: &BattleState, pack: &Pack, unit: UnitId) {
        let range = state.movement_range(pack, unit);
        let reach = reach_tiles(state, pack, unit, &range);
        self.mode = if can_command(state, unit) {
            Mode::Move { unit, range, reach }
        } else {
            // Ranges of a unit that already acted are what it could do next phase.
            let range = if state.units[unit].side == state.phase {
                let mut preview = state.clone();
                preview.units[unit].moved = false;
                preview.units[unit].acted = false;
                preview.movement_range(pack, unit)
            } else {
                range
            };
            let reach = reach_tiles(state, pack, unit, &range);
            Mode::Inspect { unit, range, reach }
        };
    }

    /// Confirm on the map tile `at` (keyboard confirm on the cursor, click or tap).
    pub fn confirm(&mut self, state: &BattleState, pack: &Pack, at: Pos) -> Request {
        match &self.mode {
            Mode::Browse | Mode::Inspect { .. } => match state.unit_at(at) {
                Some(u) => {
                    self.select(state, pack, u);
                    Request::None
                }
                None if matches!(self.mode, Mode::Inspect { .. }) => {
                    self.mode = Mode::Browse;
                    Request::None
                }
                None => Request::BattleMenu,
            },
            Mode::Move { unit, range, .. } => {
                let unit = *unit;
                if at == state.units[unit].pos {
                    self.mode = Mode::Command { unit, undo: None };
                    Request::None
                } else if range.contains(at) {
                    self.mode = Mode::Walking {
                        unit,
                        undo: Some(Box::new(state.clone())),
                    };
                    Request::Apply(Action::Move { unit, to: at })
                } else if let Some(other) = state.unit_at(at) {
                    // Picking another unit switches the selection.
                    self.select(state, pack, other);
                    Request::None
                } else {
                    Request::Invalid
                }
            }
            Mode::Attack { unit, targets, .. } => match state.unit_at(at) {
                Some(t) if targets.contains(&t) => Request::Apply(Action::Attack {
                    unit: *unit,
                    target: t,
                }),
                _ => Request::Invalid,
            },
            Mode::Aim {
                unit, list, index, ..
            } => {
                let e = &list[*index];
                match &e.aims {
                    Ok(aims) if aims.contains(&at) => Request::Apply(Action::Strategy {
                        unit: *unit,
                        strategy: e.id.clone(),
                        target: at,
                    }),
                    _ => Request::Invalid,
                }
            }
            Mode::ItemTarget {
                unit, list, index, ..
            } => {
                let e = &list[*index];
                match (state.unit_at(at), &e.targets) {
                    (Some(t), Ok(targets)) if targets.contains(&t) => {
                        Request::Apply(Action::UseItem {
                            unit: *unit,
                            item: e.id.clone(),
                            target: t,
                        })
                    }
                    _ => Request::Invalid,
                }
            }
            Mode::Walking { .. }
            | Mode::Command { .. }
            | Mode::Strategies { .. }
            | Mode::Items { .. } => Request::None,
        }
    }

    /// A menu row was chosen (command menu, strategy list, item list).
    pub fn choose(&mut self, state: &BattleState, pack: &Pack, index: usize) -> Request {
        let unit = match self.unit() {
            Some(u) => u,
            None => return Request::None,
        };
        match &self.mode {
            Mode::Command { .. } => {
                let Some(&c) = Command::ALL.get(index) else {
                    return Request::None;
                };
                if !command_enabled(state, pack, unit, c) {
                    return Request::Invalid;
                }
                let undo = self.take_undo();
                match c {
                    Command::Attack => {
                        let targets = state.attack_targets(pack, unit, state.units[unit].pos);
                        self.mode = Mode::Attack {
                            unit,
                            undo,
                            targets,
                        };
                        Request::None
                    }
                    Command::Strategy => {
                        self.mode = Mode::Strategies {
                            unit,
                            undo,
                            list: strategy_entries(state, pack, unit),
                        };
                        Request::None
                    }
                    Command::Item => {
                        self.mode = Mode::Items {
                            unit,
                            undo,
                            list: item_entries(state, pack, unit),
                        };
                        Request::None
                    }
                    Command::Wait => {
                        self.mode = Mode::Browse;
                        Request::Apply(Action::Wait { unit })
                    }
                }
            }
            Mode::Strategies { list, .. } => match list.get(index) {
                Some(e) if e.aims.is_ok() => {
                    let list = list.clone();
                    let undo = self.take_undo();
                    self.mode = Mode::Aim {
                        unit,
                        undo,
                        list,
                        index,
                    };
                    Request::None
                }
                _ => Request::Invalid,
            },
            Mode::Items { list, .. } => match list.get(index) {
                Some(e) if e.targets.is_ok() => {
                    let list = list.clone();
                    let undo = self.take_undo();
                    self.mode = Mode::ItemTarget {
                        unit,
                        undo,
                        list,
                        index,
                    };
                    Request::None
                }
                _ => Request::Invalid,
            },
            _ => Request::None,
        }
    }

    /// Step back one level. Leaving the command menu after a move asks the screen to restore
    /// the snapshot taken before it.
    pub fn cancel(&mut self, state: &BattleState, pack: &Pack) -> Request {
        let mode = std::mem::replace(&mut self.mode, Mode::Browse);
        match mode {
            Mode::Browse => Request::BattleMenu,
            Mode::Move { .. } | Mode::Inspect { .. } => Request::None,
            Mode::Walking { unit, undo } => {
                // The walk cannot be interrupted; stay.
                self.mode = Mode::Walking { unit, undo };
                Request::None
            }
            Mode::Command { unit, undo } => match undo {
                Some(snapshot) => {
                    self.select(&snapshot, pack, unit);
                    Request::Restore(snapshot)
                }
                // A move that cannot be taken back (treasure, triggered event): stay.
                None if state.units[unit].moved => {
                    self.mode = Mode::Command { unit, undo: None };
                    Request::Invalid
                }
                None => {
                    self.select(state, pack, unit);
                    Request::None
                }
            },
            Mode::Attack { unit, undo, .. }
            | Mode::Strategies { unit, undo, .. }
            | Mode::Items { unit, undo, .. } => {
                self.mode = Mode::Command { unit, undo };
                Request::None
            }
            Mode::Aim {
                unit, undo, list, ..
            } => {
                self.mode = Mode::Strategies { unit, undo, list };
                Request::None
            }
            Mode::ItemTarget {
                unit, undo, list, ..
            } => {
                self.mode = Mode::Items { unit, undo, list };
                Request::None
            }
        }
    }

    /// The move animation finished: open the command menu (with the undo snapshot when the move
    /// can be taken back), or return to browsing when the unit can no longer act.
    pub fn walked(&mut self, state: &BattleState, undoable: bool) {
        if let Mode::Walking { unit, undo } = std::mem::replace(&mut self.mode, Mode::Browse) {
            if can_command(state, unit) {
                self.mode = Mode::Command {
                    unit,
                    undo: if undoable { undo } else { None },
                };
            }
        }
    }

    /// Drop any selection (after an action, a phase change or a restored save).
    pub fn reset(&mut self) {
        self.mode = Mode::Browse;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::battle::testutil;

    fn begun() -> (std::rc::Rc<Pack>, BattleState) {
        let (pack, mut state) = testutil::sishui();
        state.begin(&pack);
        (pack, state)
    }

    #[test]
    fn select_move_and_undo() {
        let (pack, mut state) = begun();
        let gy = state.find_unit("guan_yu").unwrap();
        let start = state.units[gy].pos;
        let mut ui = PlayerUi::default();

        // Browse: confirm on Guan Yu selects him with a move range and attack reach.
        assert_eq!(ui.confirm(&state, &pack, start), Request::None);
        let Mode::Move { range, reach, .. } = &ui.mode else {
            panic!("expected Move, got {:?}", ui.mode);
        };
        assert!(range.contains(start));
        assert!(!reach.is_empty());
        assert!(reach.iter().all(|p| !range.contains(*p)));
        let dest = *range.tiles.keys().find(|p| p.y < start.y).unwrap();

        // An unreachable empty tile is invalid; a reachable one moves.
        assert_eq!(ui.confirm(&state, &pack, Pos::new(0, 0)), Request::Invalid);
        let req = ui.confirm(&state, &pack, dest);
        assert_eq!(req, Request::Apply(Action::Move { unit: gy, to: dest }));
        let events = state
            .apply(&pack, Action::Move { unit: gy, to: dest })
            .unwrap();
        assert!(move_is_undoable(&events));
        ui.walked(&state, move_is_undoable(&events));
        assert!(matches!(ui.mode, Mode::Command { undo: Some(_), .. }));

        // Cancel from the command menu restores the pre-move state and reselects the unit.
        let Request::Restore(snapshot) = ui.cancel(&state, &pack) else {
            panic!("expected a restore");
        };
        assert_eq!(snapshot.units[gy].pos, start);
        assert!(!snapshot.units[gy].moved);
        assert!(matches!(ui.mode, Mode::Move { unit, .. } if unit == gy));
        state = *snapshot;

        // Cancel again: back to browsing. Cancel while browsing opens the battle menu.
        assert_eq!(ui.cancel(&state, &pack), Request::None);
        assert_eq!(ui.mode, Mode::Browse);
        assert_eq!(ui.cancel(&state, &pack), Request::BattleMenu);
        // Confirm on an empty tile opens the battle menu too.
        assert_eq!(
            ui.confirm(&state, &pack, Pos::new(0, 0)),
            Request::BattleMenu
        );
    }

    #[test]
    fn staying_put_opens_commands_and_wait_applies() {
        let (pack, state) = begun();
        let gy = state.find_unit("guan_yu").unwrap();
        let at = state.units[gy].pos;
        let mut ui = PlayerUi::default();
        ui.select(&state, &pack, gy);
        assert_eq!(ui.confirm(&state, &pack, at), Request::None);
        assert!(matches!(ui.mode, Mode::Command { undo: None, .. }));
        // Nothing to attack, no strategies at level 1, no items: only 대기.
        let enabled: Vec<bool> = Command::ALL
            .iter()
            .map(|c| command_enabled(&state, &pack, gy, *c))
            .collect();
        assert_eq!(enabled, [false, false, false, true]);
        assert_eq!(ui.choose(&state, &pack, 0), Request::Invalid);
        assert_eq!(
            ui.choose(&state, &pack, 3),
            Request::Apply(Action::Wait { unit: gy })
        );
        assert_eq!(ui.mode, Mode::Browse);
        // Cancel on the command menu without a move just reselects.
        ui.select(&state, &pack, gy);
        ui.confirm(&state, &pack, at);
        assert_eq!(ui.cancel(&state, &pack), Request::None);
        assert!(matches!(ui.mode, Mode::Move { .. }));
    }

    #[test]
    fn moves_that_cannot_be_undone_keep_the_command_menu() {
        let (pack, mut state) = begun();
        let gy = state.find_unit("guan_yu").unwrap();
        let start = state.units[gy].pos;
        let mut ui = PlayerUi::default();
        ui.select(&state, &pack, gy);
        let Mode::Move { range, .. } = &ui.mode else {
            panic!("expected Move");
        };
        let dest = *range.tiles.keys().find(|p| **p != start).unwrap();
        ui.confirm(&state, &pack, dest);
        state
            .apply(&pack, Action::Move { unit: gy, to: dest })
            .unwrap();
        // As if the move had found a treasure: not undoable.
        ui.walked(&state, false);
        assert!(matches!(ui.mode, Mode::Command { undo: None, .. }));
        assert_eq!(ui.cancel(&state, &pack), Request::Invalid);
        assert!(matches!(ui.mode, Mode::Command { .. }));
    }

    #[test]
    fn inspecting_enemies_shows_their_ranges() {
        let (pack, state) = begun();
        let hx = state.find_unit("hua_xiong").unwrap();
        let mut ui = PlayerUi::default();
        ui.confirm(&state, &pack, state.units[hx].pos);
        let Mode::Inspect { unit, range, reach } = &ui.mode else {
            panic!("expected Inspect");
        };
        assert_eq!(*unit, hx);
        assert!(range.tiles.len() > 1);
        assert!(!reach.is_empty());
        // Confirm on an empty tile closes the inspection.
        ui.confirm(&state, &pack, Pos::new(0, 0));
        assert_eq!(ui.mode, Mode::Browse);
    }

    #[test]
    fn danger_tiles_cover_every_enemys_reach() {
        let (pack, state) = begun();
        let danger = danger_tiles(&state, &pack);
        assert!(!danger.is_empty());
        // Every inspected enemy's move range and reach that it could hit is inside.
        let hx = state.find_unit("hua_xiong").unwrap();
        let range = state.movement_range(&pack, hx);
        for p in range.tiles.keys() {
            for t in state.attack_tiles(&pack, hx, *p) {
                assert!(danger.binary_search(&t).is_ok(), "{t:?}");
            }
        }
        // Player units add nothing; tiles stay unique and on the map.
        let mut sorted = danger.clone();
        sorted.dedup();
        assert_eq!(sorted, danger);
        assert!(danger.iter().all(|p| state.map.in_bounds(*p)));

        // The key follows what the tiles depend on.
        let key = danger_key(&state);
        let mut moved = state.clone();
        moved.units[hx].pos = Pos::new(moved.units[hx].pos.x, moved.units[hx].pos.y + 1);
        assert_ne!(danger_key(&moved), key);
        let mut gone = state.clone();
        gone.units[hx].state = hero_core::battle::UnitState::Retreated;
        assert_ne!(danger_key(&gone), key);
        assert_eq!(danger_key(&state.clone()), key);
    }

    #[test]
    fn attack_targets_and_menus_step_back() {
        let (pack, mut state) = begun();
        let gy = state.find_unit("guan_yu").unwrap();
        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy && u.is_active())
            .unwrap();
        // Put an enemy right next to Guan Yu.
        let next = state.units[gy].pos.offset(0, -1);
        state.units[foe].pos = next;
        let mut ui = PlayerUi::default();
        ui.select(&state, &pack, gy);
        ui.confirm(&state, &pack, state.units[gy].pos);
        assert!(command_enabled(&state, &pack, gy, Command::Attack));
        assert_eq!(ui.choose(&state, &pack, 0), Request::None);
        assert!(matches!(&ui.mode, Mode::Attack { targets, .. } if targets == &vec![foe]));
        assert_eq!(ui.confirm(&state, &pack, Pos::new(0, 0)), Request::Invalid);
        assert_eq!(
            ui.confirm(&state, &pack, next),
            Request::Apply(Action::Attack {
                unit: gy,
                target: foe
            })
        );
        assert_eq!(ui.cancel(&state, &pack), Request::None);
        assert!(matches!(ui.mode, Mode::Command { .. }));
    }

    /// On touch screens (no cancel key, no right click) the screen turns a rejected tap in a
    /// target mode into `cancel`: the rejected confirm must leave the mode as it was, and the
    /// cancel must land on the menu the target mode came from.
    #[test]
    fn target_modes_reject_other_tiles_and_step_back_to_their_menu() {
        let (pack, state) = begun();
        let lb = state.find_unit("liu_bei").unwrap();
        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy && u.is_active())
            .unwrap();
        let elsewhere = Pos::new(0, 0);
        assert_eq!(state.unit_at(elsewhere), None);
        let strategy = StrategyEntry {
            id: "scorch".into(),
            name: "초열".into(),
            mp: 4,
            element: Some("fire".into()),
            desc: String::new(),
            aims: Ok(vec![state.units[foe].pos]),
        };
        let item = ItemEntry {
            id: "bean".into(),
            name: "콩".into(),
            icon: String::new(),
            count: 1,
            desc: String::new(),
            strategy: None,
            targets: Ok(vec![lb]),
        };
        let cases = [
            (
                Mode::Attack {
                    unit: lb,
                    undo: None,
                    targets: vec![foe],
                },
                "command",
            ),
            (
                Mode::Aim {
                    unit: lb,
                    undo: None,
                    list: vec![strategy],
                    index: 0,
                },
                "strategies",
            ),
            (
                Mode::ItemTarget {
                    unit: lb,
                    undo: None,
                    list: vec![item],
                    index: 0,
                },
                "items",
            ),
        ];
        for (mode, menu) in cases {
            let mut ui = PlayerUi { mode: mode.clone() };
            assert_eq!(ui.confirm(&state, &pack, elsewhere), Request::Invalid);
            assert_eq!(ui.mode, mode);
            assert_eq!(ui.cancel(&state, &pack), Request::None);
            let back = match &ui.mode {
                Mode::Command { unit, .. } if *unit == lb => "command",
                Mode::Strategies { unit, .. } if *unit == lb => "strategies",
                Mode::Items { unit, .. } if *unit == lb => "items",
                _ => "elsewhere",
            };
            assert_eq!(back, menu, "{:?}", ui.mode);
        }
    }

    #[test]
    fn strategies_and_items_explain_why_they_are_blocked() {
        let (pack, mut state) = begun();
        let lb = state.find_unit("liu_bei").unwrap();
        // Level 6 infantry knows 초열 (fire, range8, single).
        state.units[lb].level = 6;
        state.units[lb].mp = 20;
        let list = strategy_entries(&state, &pack, lb);
        let scorch = list.iter().find(|e| e.id == "scorch").unwrap();
        assert_eq!(scorch.aims, Err(Blocked::NoTarget));

        let foe = state
            .units
            .iter()
            .position(|u| u.side == Side::Enemy && u.is_active())
            .unwrap();
        let pos = state.units[lb].pos;
        state.units[foe].pos = pos.offset(1, -1);
        let fire_ok = state
            .terrain_at(&pack, pos.offset(1, -1))
            .is_some_and(|t| t.elements.iter().any(|e| e == "fire"));
        let list = strategy_entries(&state, &pack, lb);
        let scorch = list.iter().find(|e| e.id == "scorch").unwrap();
        if fire_ok {
            assert_eq!(scorch.aims, Ok(vec![pos.offset(1, -1)]));
        } else {
            assert_eq!(scorch.aims, Err(Blocked::Terrain));
        }
        state.weather = Weather::Rain;
        let scorch = strategy_entries(&state, &pack, lb)
            .into_iter()
            .find(|e| e.id == "scorch")
            .unwrap();
        assert_eq!(
            scorch.aims,
            Err(if fire_ok {
                Blocked::Rain
            } else {
                Blocked::Terrain
            })
        );
        state.units[lb].mp = 1;
        let scorch = strategy_entries(&state, &pack, lb)
            .into_iter()
            .find(|e| e.id == "scorch")
            .unwrap();
        assert_eq!(scorch.aims, Err(Blocked::NoMp));

        // Items: beans heal the user or an adjacent friend.
        assert!(item_entries(&state, &pack, lb).is_empty());
        state.inventory.insert("bean".into(), 2);
        let items = item_entries(&state, &pack, lb);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].count, 2);
        assert!(matches!(&items[0].targets, Ok(t) if t.contains(&lb)));
        assert!(command_enabled(&state, &pack, lb, Command::Item));
        let mut ui = PlayerUi::default();
        ui.select(&state, &pack, lb);
        ui.confirm(&state, &pack, pos);
        assert_eq!(ui.choose(&state, &pack, 2), Request::None);
        assert_eq!(ui.choose(&state, &pack, 0), Request::None);
        assert!(matches!(ui.mode, Mode::ItemTarget { .. }));
        assert_eq!(
            ui.confirm(&state, &pack, pos),
            Request::Apply(Action::UseItem {
                unit: lb,
                item: "bean".into(),
                target: lb
            })
        );
        assert_eq!(ui.cancel(&state, &pack), Request::None);
        assert!(matches!(ui.mode, Mode::Items { .. }));
    }

    /// Regression: equipment with `battle_use` (a pack mistake `hero-tools validate` warns about)
    /// used to be listed and then rejected by the engine with `BadItem`.
    #[test]
    fn equipment_is_never_a_battle_item() {
        let (pack, mut state) = begun();
        let mut pack = (*pack).clone();
        pack.items
            .get_mut("serpent_spear")
            .expect("the base pack has the serpent spear")
            .battle_use = true;
        let lb = state.find_unit("liu_bei").unwrap();
        state.inventory.clear();
        state.inventory.insert("serpent_spear".into(), 1);
        assert!(item_entries(&state, &pack, lb).is_empty());
        assert!(!command_enabled(&state, &pack, lb, Command::Item));
        state.inventory.insert("bean".into(), 1);
        let items = item_entries(&state, &pack, lb);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "bean");
        assert!(command_enabled(&state, &pack, lb, Command::Item));
    }

    #[test]
    fn cycling_skips_units_that_acted() {
        let (pack, mut state) = begun();
        let players: Vec<UnitId> = (0..state.units.len())
            .filter(|&i| state.units[i].side == Side::Player)
            .collect();
        assert!(players.len() >= 3);
        assert_eq!(cycle_actor(&state, None, 1), Some(players[0]));
        assert_eq!(cycle_actor(&state, Some(players[0]), 1), Some(players[1]));
        assert_eq!(
            cycle_actor(&state, Some(players[0]), -1),
            Some(*players.last().unwrap())
        );
        state
            .apply(&pack, Action::Wait { unit: players[1] })
            .unwrap();
        assert_eq!(cycle_actor(&state, Some(players[0]), 1), Some(players[2]));
        // From a unit that is not an actor, the next actor in unit order.
        assert_eq!(cycle_actor(&state, Some(players[1]), 1), Some(players[2]));
        for &p in &players {
            let _ = state.apply(&pack, Action::Wait { unit: p });
        }
        assert_eq!(cycle_actor(&state, None, 1), None);
    }

    #[test]
    fn area_preview_shapes() {
        let pack = testutil::base_pack();
        let c = Pos::new(5, 5);
        assert_eq!(
            area_tiles(&pack, "scorch", c, Pos::new(6, 5)),
            vec![Pos::new(6, 5)]
        );
        assert_eq!(
            area_tiles(&pack, "great_scorch", c, Pos::new(6, 5)).len(),
            5
        );
    }
}
