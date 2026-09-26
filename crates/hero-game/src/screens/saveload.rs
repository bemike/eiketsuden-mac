//! Save / load slot screen, reusable from the title screen (load) and the camp (save).
//!
//! * [`SaveLoadScreen::load`] lists every slot; choosing a filled slot offers
//!   불러오기 / 삭제, and loading continues the game ([`Flow::Continue`]).
//! * [`SaveLoadScreen::save`] writes the given [`SaveGame`] into a manual slot (the autosave slot
//!   is read-only here), asking before overwriting, then returns to the previous screen.
//!
//! Deleting always asks for confirmation. The Delete key deletes the selected slot directly
//! (with confirmation). The screen pops itself on cancel.

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{fill_rect, Align, TextStyle, SCREEN, VIRTUAL_W};
use crate::platform::unix_now;
use crate::saves::{self, SaveSlot, SlotInfo, SlotStatus};
use crate::ui::dialog::{ChoiceBox, ChoiceEvent, ConfirmDialog, ConfirmEvent};
use crate::ui::format;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::{draw_title_bar, draw_window_ex, WindowStyle};
use hero_core::save::SaveGame;
use macroquad::prelude::*;

enum Mode {
    Load,
    Save(Box<SaveGame>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotAction {
    Load,
    Save,
    Delete,
    Cancel,
}

impl SlotAction {
    fn label(self) -> &'static str {
        match self {
            SlotAction::Load => "불러오기",
            SlotAction::Save => "여기에 기록",
            SlotAction::Delete => "삭제",
            SlotAction::Cancel => "취소",
        }
    }
}

enum Popup {
    None,
    Actions {
        slot: usize,
        actions: Vec<SlotAction>,
        choice: ChoiceBox,
    },
    Confirm {
        slot: usize,
        action: SlotAction,
        dialog: ConfirmDialog,
    },
}

const LIST_RECT: Rect = Rect {
    x: 12.0,
    y: 28.0,
    w: 456.0,
    h: 0.0,
};

pub struct SaveLoadScreen {
    mode: Mode,
    pack_id: String,
    slots: Vec<SlotInfo>,
    menu: Menu,
    popup: Popup,
    now: u64,
}

impl SaveLoadScreen {
    /// Pick a save to load (for pack `pack_id`).
    pub fn load(pack_id: &str) -> SaveLoadScreen {
        SaveLoadScreen::new(Mode::Load, pack_id)
    }

    /// Pick a slot to write `save` into.
    pub fn save(save: SaveGame) -> SaveLoadScreen {
        let pack_id = save.pack_id.clone();
        SaveLoadScreen::new(Mode::Save(Box::new(save)), &pack_id)
    }

    fn new(mode: Mode, pack_id: &str) -> SaveLoadScreen {
        SaveLoadScreen {
            mode,
            pack_id: pack_id.to_string(),
            slots: Vec::new(),
            menu: Menu::new(Vec::new()),
            popup: Popup::None,
            now: 0,
        }
    }

    fn saving(&self) -> bool {
        matches!(self.mode, Mode::Save(_))
    }

    fn refresh(&mut self, ctx: &Ctx) {
        self.now = unix_now();
        self.slots = saves::list(ctx.storage.as_ref(), &self.pack_id);
        let saving = self.saving();
        let items = self
            .slots
            .iter()
            .map(|info| {
                let (text, detail) = match &info.status {
                    SlotStatus::Empty => ("— 비어 있음 —".to_string(), String::new()),
                    SlotStatus::Ready(s) => {
                        (s.label.clone(), format::relative_time(s.saved_at, self.now))
                    }
                    SlotStatus::Unreadable(_) => ("(읽을 수 없는 기록)".to_string(), String::new()),
                };
                let enabled = match (&info.status, info.slot) {
                    (_, SaveSlot::Auto) if saving => false,
                    (SlotStatus::Empty, _) => saving,
                    _ => true,
                };
                let item = MenuItem::new(text).tag(info.slot.name()).enabled(enabled);
                if detail.is_empty() {
                    item
                } else {
                    item.detail(detail)
                }
            })
            .collect();
        let first_refresh = self.menu.items.is_empty();
        let cursor = self.menu.cursor();
        // A new menu starts on the first enabled slot: the first loadable save when loading,
        // the first manual slot when saving.
        let mut menu = Menu::new(items).at(LIST_RECT.x, LIST_RECT.y, LIST_RECT.w);
        menu.wrap = false;
        menu.tag_width = 64.0;
        // After an action keep the cursor on the same slot while it is still selectable.
        if !first_refresh && menu.items.get(cursor).is_some_and(|it| it.enabled) {
            menu.set_cursor(cursor);
        }
        self.menu = menu;
    }

    fn open_actions(&mut self, ctx: &mut Ctx, index: usize) {
        let info = &self.slots[index];
        let mut actions = Vec::new();
        match (&self.mode, &info.status) {
            (Mode::Load, SlotStatus::Ready(_)) => actions.push(SlotAction::Load),
            (Mode::Save(_), _) if info.slot != SaveSlot::Auto => actions.push(SlotAction::Save),
            _ => {}
        }
        if info.status != SlotStatus::Empty {
            actions.push(SlotAction::Delete);
        }
        actions.push(SlotAction::Cancel);
        let labels: Vec<&str> = actions.iter().map(|a| a.label()).collect();
        let prompt = match &info.status {
            SlotStatus::Unreadable(why) => format!("{} — {why}", info.slot.name()),
            _ => info.slot.name(),
        };
        let cancel = actions.len() - 1;
        let choice = ChoiceBox::new(&ctx.gfx, Some(&prompt), &labels, Some(cancel));
        self.popup = Popup::Actions {
            slot: index,
            actions,
            choice,
        };
    }

    fn confirm(&mut self, ctx: &Ctx, slot: usize, action: SlotAction) {
        let name = self.slots[slot].slot.name();
        let dialog = match action {
            SlotAction::Delete => {
                ConfirmDialog::new(&ctx.gfx, &format!("{name}을(를) 삭제할까요?")).default_no()
            }
            _ => ConfirmDialog::new(&ctx.gfx, &format!("{name}에 덮어쓸까요?")).default_no(),
        };
        self.popup = Popup::Confirm {
            slot,
            action,
            dialog,
        };
    }

    /// Carry out an action on slot `index`.
    fn perform(&mut self, ctx: &mut Ctx, index: usize, action: SlotAction) -> Transition {
        let slot = self.slots[index].slot;
        match action {
            SlotAction::Load => match saves::read(ctx.storage.as_ref(), slot, &self.pack_id) {
                Ok(save) => return Transition::Flow(Flow::Continue(Box::new(save))),
                Err(e) => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(e.to_string());
                }
            },
            SlotAction::Save => {
                if let Mode::Save(save) = &self.mode {
                    let mut save = (**save).clone();
                    save.saved_at = unix_now();
                    match saves::write(ctx.storage.as_mut(), slot, &save) {
                        Ok(()) => {
                            ctx.toast(format!("{}에 기록했습니다.", slot.name()));
                            return Transition::Pop;
                        }
                        Err(e) => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(format!("기록하지 못했습니다: {e}"));
                        }
                    }
                }
            }
            SlotAction::Delete => match saves::delete(ctx.storage.as_mut(), slot) {
                Ok(()) => ctx.toast(format!("{}을(를) 삭제했습니다.", slot.name())),
                Err(e) => {
                    ctx.sfx(sfx::ERROR);
                    ctx.toast(format!("삭제하지 못했습니다: {e}"));
                }
            },
            SlotAction::Cancel => {}
        }
        self.refresh(ctx);
        Transition::None
    }

    fn update_popup(&mut self, ctx: &mut Ctx) -> Option<Transition> {
        match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::None => None,
            Popup::Actions {
                slot,
                actions,
                mut choice,
            } => {
                match choice.update(ctx) {
                    ChoiceEvent::Chosen(i) => match actions[i] {
                        SlotAction::Cancel => {}
                        SlotAction::Delete => self.confirm(ctx, slot, SlotAction::Delete),
                        SlotAction::Save if self.slots[slot].status != SlotStatus::Empty => {
                            self.confirm(ctx, slot, SlotAction::Save)
                        }
                        action => return Some(self.perform(ctx, slot, action)),
                    },
                    ChoiceEvent::Cancelled => {}
                    ChoiceEvent::None => {
                        self.popup = Popup::Actions {
                            slot,
                            actions,
                            choice,
                        }
                    }
                }
                Some(Transition::None)
            }
            Popup::Confirm {
                slot,
                action,
                mut dialog,
            } => {
                match dialog.update(ctx) {
                    ConfirmEvent::Yes => return Some(self.perform(ctx, slot, action)),
                    ConfirmEvent::No => {}
                    ConfirmEvent::None => {
                        self.popup = Popup::Confirm {
                            slot,
                            action,
                            dialog,
                        }
                    }
                }
                Some(Transition::None)
            }
        }
    }
}

impl Screen for SaveLoadScreen {
    fn name(&self) -> &'static str {
        "saveload"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, _how: Enter) {
        self.refresh(ctx);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        if let Some(t) = self.update_popup(ctx) {
            return t;
        }
        if ctx.input.key_pressed(KeyCode::Delete) {
            let i = self.menu.cursor();
            if self
                .slots
                .get(i)
                .is_some_and(|s| s.status != SlotStatus::Empty)
            {
                self.confirm(ctx, i, SlotAction::Delete);
                return Transition::None;
            }
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => {
                let info = &self.slots[i];
                if self.saving() && info.status == SlotStatus::Empty {
                    return self.perform(ctx, i, SlotAction::Save);
                }
                self.open_actions(ctx, i);
                Transition::None
            }
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn draw(&self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        fill_rect(SCREEN, theme::BACKGROUND);
        draw_title_bar(
            ctx,
            if self.saving() {
                "기록하기"
            } else {
                "불러오기"
            },
        );
        self.menu.draw(ctx);

        // Details of the selected slot.
        let list = self.menu.rect();
        let info_rect = Rect::new(
            list.x,
            list.bottom() + 6.0,
            list.w,
            252.0 - list.bottom() - 6.0,
        );
        if info_rect.h >= 20.0 {
            draw_window_ex(info_rect, WindowStyle::Panel, 1.0);
            let x = info_rect.x + 10.0;
            let y = info_rect.y + 6.0;
            let small = TextStyle::small(theme::TEXT_DIM);
            let text = TextStyle::main(theme::TEXT).shadow(theme::TEXT_SHADOW);
            if let Some(info) = self.slots.get(self.menu.cursor()) {
                match &info.status {
                    SlotStatus::Ready(s) => {
                        gfx.text(&s.label, x, y, text);
                        let when = if s.saved_at > 0 {
                            format!(
                                "{} ({})",
                                format::relative_time(s.saved_at, self.now),
                                format::date_utc(s.saved_at)
                            )
                        } else {
                            "시각 모름".into()
                        };
                        let mut detail = format!(
                            "기록 시각 {when}   플레이 {}",
                            format::play_time(s.play_seconds)
                        );
                        if s.mid_battle {
                            detail.push_str("   전투 중");
                        }
                        gfx.text(&detail, x, y + 17.0, small);
                    }
                    SlotStatus::Empty => {
                        gfx.text(
                            "비어 있는 칸입니다.",
                            x,
                            y,
                            TextStyle::main(theme::TEXT_DIM),
                        );
                    }
                    SlotStatus::Unreadable(why) => {
                        gfx.text(
                            "읽을 수 없는 기록입니다.",
                            x,
                            y,
                            TextStyle::main(theme::TEXT_BAD),
                        );
                        let lines = gfx.wrap(why, crate::gfx::FontId::Small, 1, info_rect.w - 20.0);
                        gfx.text_lines(&lines[..lines.len().min(2)], x, y + 17.0, small);
                    }
                }
            }
        }
        gfx.text_aligned(
            "Z/Enter 선택 · X/Esc 돌아가기 · Delete 삭제",
            0.0,
            255.0,
            VIRTUAL_W - 8.0,
            Align::Right,
            TextStyle::small(theme::TEXT_DISABLED),
        );

        match &self.popup {
            Popup::None => {}
            Popup::Actions { choice, .. } => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.35));
                choice.draw(ctx);
            }
            Popup::Confirm { dialog, .. } => {
                fill_rect(SCREEN, Color::new(0.0, 0.0, 0.0, 0.35));
                dialog.draw(ctx);
            }
        }
    }
}
