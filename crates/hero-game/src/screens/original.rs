//! 원작 데이터 (native only): choose the folder of the player's own copy of the original game and
//! switch between the original mode and the base pack, without a command line (`crate::original`).
//!
//! The screen has two views:
//!
//! * **Overview** — what the original mode is, the chosen folder and its edition (the same
//!   verdict and evidence as `hero-tools original probe`), what is being played now, and the
//!   commands: play the original mode / the base pack, pick a folder, back.
//! * **Folder browser** — the drives (Windows) and folders, one level at a time. Folders that
//!   directly hold original files are marked ★; the current folder's edition is shown above the
//!   list, and "이 폴더 사용" is enabled only for an edition the original mode can play.
//!
//! Choosing a folder or switching the mode saves the settings and reloads the data pack
//! ([`Flow::Reload`]): the loading screen converts the install in memory.

use crate::app::{Ctx, Enter, Screen, Transition};
use crate::audio::sfx;
use crate::flow::Flow;
use crate::gfx::{FontId, Gfx, TextStyle};
use crate::original::{self, Folder, FolderCheck, Place};
use crate::platform::memfs;
use crate::ui::menu::{Menu, MenuEvent, MenuItem};
use crate::ui::theme;
use crate::ui::window::draw_window;
use macroquad::prelude::*;

/// Left and right margin of the text and the menu.
const MARGIN: f32 = 24.0;
/// Top of the text area, below the heading.
const TEXT_TOP: f32 = 48.0;
/// Line height of the small font.
const LINE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    PlayOriginal,
    PlayBase,
    Browse,
    Back,
}

/// A row of the folder browser.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    UseThis,
    Up,
    Open(Folder),
}

enum View {
    Overview { commands: Vec<Command> },
    Browse { place: Place, rows: Vec<Row> },
}

pub struct OriginalScreen {
    view: View,
    menu: Menu,
    /// Wrapped lines above the menu.
    lines: Vec<(String, Color)>,
    /// The browser was opened directly (from the error screen): cancel leaves the screen.
    browse_only: bool,
    /// The saved folder's edition, for the overview.
    saved: Option<FolderCheck>,
}

/// `text` shortened with `…` to fit `width` pixels of the main font.
fn fit(gfx: &Gfx, text: &str, width: f32) -> String {
    if gfx.text_width(text, FontId::Main, 1) <= width {
        return text.to_string();
    }
    let mut out: String = text.to_string();
    while !out.is_empty() && gfx.text_width(&format!("{out}…"), FontId::Main, 1) > width {
        out.pop();
    }
    format!("{out}…")
}

/// The first folder row after `cursor` (wrapping around) whose name starts with `c`, ignoring
/// case. Letters bound to confirm and cancel do not jump.
fn next_starting_with(rows: &[Row], cursor: usize, c: char) -> Option<usize> {
    // Z / X / Space are the game's confirm and cancel keys.
    if c.is_control() || c.is_whitespace() || matches!(c, 'z' | 'Z' | 'x' | 'X') {
        return None;
    }
    let lower: String = c.to_lowercase().collect();
    let n = rows.len();
    (1..=n).map(|k| (cursor + k) % n).find(|&i| match &rows[i] {
        Row::Open(f) => f.name.to_lowercase().starts_with(&lower),
        _ => false,
    })
}

impl OriginalScreen {
    /// The overview.
    pub fn new() -> OriginalScreen {
        OriginalScreen {
            view: View::Overview {
                commands: Vec::new(),
            },
            menu: Menu::new(Vec::new()),
            lines: Vec::new(),
            browse_only: false,
            saved: None,
        }
    }

    /// Straight to the folder browser; leaving it leaves the screen.
    pub fn browse() -> OriginalScreen {
        OriginalScreen {
            browse_only: true,
            ..OriginalScreen::new()
        }
    }

    fn wrap_into(&mut self, gfx: &Gfx, text: &str, color: Color) {
        let width = gfx.size().x - 2.0 * MARGIN - 8.0;
        for line in gfx.wrap(text, FontId::Small, 1, width) {
            self.lines.push((line, color));
        }
    }

    /// Place the menu below the text, as wide as the canvas allows, `rows` rows at most.
    fn place_menu(&mut self, gfx: &Gfx, mut menu: Menu) {
        let canvas = gfx.size();
        let top = TEXT_TOP + self.lines.len() as f32 * LINE + 12.0;
        let room = (canvas.y - 8.0 - top - 2.0 * theme::PADDING).max(theme::ROW_HEIGHT);
        let rows = ((room / theme::ROW_HEIGHT) as usize).max(1);
        menu.visible_rows = rows;
        menu.set_position(MARGIN, top);
        menu.set_width(canvas.x - 2.0 * MARGIN);
        let cursor = menu.cursor();
        menu.set_cursor(cursor);
        self.menu = menu;
    }

    fn show_overview(&mut self, ctx: &Ctx) {
        let gfx = &ctx.gfx;
        let saved_dir = ctx.settings.original_dir.clone();
        self.saved = saved_dir
            .as_deref()
            .map(|d| original::check_folder(std::path::Path::new(d)));
        let playing = memfs::mounted().is_some();
        self.lines.clear();
        self.wrap_into(
            gfx,
            "가지고 있는 원작(삼국지 영걸전 한국어 DOS/V판·중국어 DOS판)의 설치 폴더를 고르면, 원작의 \
             얼굴·유닛·지형·전투 맵 그림으로 플레이합니다. 원작 파일은 읽기만 하고, 변환한 그림은 \
             저장하지 않고 실행할 때마다 다시 만듭니다. 아직 변환하지 못한 것은 기본 팩의 것을 씁니다.",
            theme::TEXT_DIM,
        );
        self.lines.push((String::new(), theme::TEXT));
        match (&saved_dir, self.saved.clone()) {
            (Some(dir), Some(check)) => {
                self.wrap_into(gfx, &format!("원작 폴더: {dir}"), theme::TEXT);
                let color = if check.is_supported() {
                    theme::TEXT_GOOD
                } else {
                    theme::TEXT_BAD
                };
                self.wrap_into(gfx, &check.summary(), color);
            }
            _ => self.wrap_into(gfx, "원작 폴더: 아직 고르지 않았습니다", theme::TEXT),
        }
        let now = if playing {
            "지금: 원작 모드로 플레이하고 있습니다"
        } else if crate::platform::explicit_data(&ctx.options) {
            "지금: --data로 지정한 팩으로 플레이하고 있습니다(원작 모드 설정은 적용되지 않습니다)"
        } else {
            "지금: 기본 팩으로 플레이하고 있습니다"
        };
        self.wrap_into(gfx, now, theme::TEXT_ACCENT);

        let supported = self.saved.as_ref().is_some_and(FolderCheck::is_supported);
        let mut commands = Vec::new();
        commands.push(if playing {
            Command::PlayBase
        } else {
            Command::PlayOriginal
        });
        commands.push(Command::Browse);
        commands.push(Command::Back);
        let items = commands
            .iter()
            .map(|c| match c {
                Command::PlayOriginal => MenuItem::new("원작 모드로 플레이").enabled(supported),
                Command::PlayBase => MenuItem::new("기본 팩으로 플레이"),
                Command::Browse => MenuItem::new("원작 폴더 고르기…"),
                Command::Back => MenuItem::new("돌아가기"),
            })
            .collect();
        self.view = View::Overview { commands };
        self.place_menu(gfx, Menu::new(items));
    }

    /// Show the folders at `place`, the cursor on the folder `focus` when it is listed (the
    /// folder just left when going up).
    fn show_place(&mut self, ctx: &Ctx, place: Place, focus: Option<&std::path::Path>) {
        let gfx = &ctx.gfx;
        let (folders, error) = match original::list(&place) {
            Ok(f) => (f, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let check = match &place {
            Place::Roots => None,
            Place::Dir(dir) if original::looks_like_install(dir) => {
                Some(original::check_folder(dir))
            }
            Place::Dir(_) => None,
        };
        self.lines.clear();
        let here = match &place {
            Place::Roots => "드라이브".to_string(),
            Place::Dir(dir) => dir.display().to_string(),
        };
        self.wrap_into(gfx, &format!("위치: {here}"), theme::TEXT);
        match (&check, &error) {
            (_, Some(e)) => self.wrap_into(gfx, &format!("이 폴더를 읽을 수 없습니다: {e}"), theme::TEXT_BAD),
            (Some(c), None) => {
                let color = if c.is_supported() {
                    theme::TEXT_GOOD
                } else {
                    theme::TEXT_BAD
                };
                self.wrap_into(gfx, &c.summary(), color);
                // The evidence, as `hero-tools original probe` prints it.
                for e in c.evidence() {
                    self.wrap_into(gfx, &format!("· {e}"), theme::TEXT_DIM);
                }
            }
            (None, None) => self.wrap_into(
                gfx,
                "원작 파일(DISK1.R3I, HEXZMAP.R3 등)이 있는 폴더로 들어가세요. ★는 원작 파일이 있는 폴더입니다.",
                theme::TEXT_DIM,
            ),
        }

        let mut rows = Vec::new();
        if matches!(place, Place::Dir(_)) {
            rows.push(Row::UseThis);
        }
        if original::parent(&place).is_some() {
            rows.push(Row::Up);
        }
        rows.extend(folders.into_iter().map(Row::Open));
        let supported = check.as_ref().is_some_and(FolderCheck::is_supported);
        let width = gfx.size().x - 2.0 * MARGIN - 2.0 * theme::PADDING - 40.0;
        let items = rows
            .iter()
            .map(|r| match r {
                Row::UseThis => MenuItem::new("이 폴더 사용").enabled(supported),
                Row::Up => MenuItem::new("..").detail("상위 폴더"),
                Row::Open(f) => {
                    let item = MenuItem::new(fit(gfx, &f.name, width));
                    if f.install {
                        item.tag("★")
                    } else {
                        item
                    }
                }
            })
            .collect();
        let mut menu = Menu::new(items);
        menu.tag_width = 16.0;
        menu.wrap = false;
        // Land on the folder just left, on "use this folder" when it can be used, else on the
        // first subfolder.
        let focused = focus.and_then(|focus| {
            rows.iter()
                .position(|r| matches!(r, Row::Open(f) if f.path == focus))
        });
        let first = if let Some(i) = focused {
            i
        } else if supported {
            0
        } else {
            rows.iter()
                .position(|r| matches!(r, Row::Open(_)))
                .or_else(|| rows.iter().position(|r| *r == Row::Up))
                .unwrap_or(0)
        };
        menu.set_cursor(first);
        self.view = View::Browse { place, rows };
        self.place_menu(gfx, menu);
    }

    /// Save the settings and load everything again.
    fn apply(ctx: &mut Ctx, dir: Option<String>, on: bool) -> Transition {
        if let Some(dir) = dir {
            ctx.settings.original_dir = Some(dir);
        }
        ctx.settings.original_mode = on;
        ctx.commit_settings();
        Transition::Flow(Flow::Reload)
    }

    fn leave_browser(&mut self, ctx: &mut Ctx) -> Transition {
        if self.browse_only {
            Transition::Pop
        } else {
            self.show_overview(ctx);
            Transition::None
        }
    }

    fn update_overview(&mut self, ctx: &mut Ctx, commands: &[Command]) -> Transition {
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => match commands[i] {
                Command::PlayOriginal => OriginalScreen::apply(ctx, None, true),
                Command::PlayBase => OriginalScreen::apply(ctx, None, false),
                Command::Browse => {
                    let place = original::start_place(ctx.settings.original_dir.as_deref());
                    self.show_place(ctx, place, None);
                    Transition::None
                }
                Command::Back => Transition::Pop,
            },
            MenuEvent::Cancelled => Transition::Pop,
            _ => Transition::None,
        }
    }

    fn update_browser(&mut self, ctx: &mut Ctx, place: &Place, rows: &[Row]) -> Transition {
        let up = original::parent(place);
        let here = match place {
            Place::Dir(dir) => Some(dir.as_path()),
            Place::Roots => None,
        };
        // Like a file manager: Backspace goes up one level (handled before the menu, where it
        // is a cancel key), a letter jumps to the next folder starting with it, Page Up / Page
        // Down move a page.
        if ctx.input.key_pressed(KeyCode::Backspace) {
            if let Some(p) = up.clone() {
                ctx.sfx(sfx::CANCEL);
                self.show_place(ctx, p, here);
                ctx.input.consume();
                return Transition::None;
            }
        }
        while let Some(c) = get_char_pressed() {
            if let Some(i) = next_starting_with(rows, self.menu.cursor(), c) {
                self.menu.set_cursor(i);
                ctx.sfx(sfx::CURSOR);
            }
        }
        let page = self.menu.visible_rows;
        if ctx.input.key_pressed(KeyCode::PageDown) {
            self.menu.set_cursor(self.menu.cursor() + page);
        } else if ctx.input.key_pressed(KeyCode::PageUp) {
            self.menu
                .set_cursor(self.menu.cursor().saturating_sub(page));
        }
        match self.menu.update(ctx) {
            MenuEvent::Selected(i) => match &rows[i] {
                Row::UseThis => {
                    let Place::Dir(dir) = place else {
                        return Transition::None;
                    };
                    // Identify again: the folder may have changed since it was listed.
                    match original::check_folder(dir) {
                        FolderCheck::Supported(_) => match original::storable_path(dir) {
                            Some(path) => OriginalScreen::apply(ctx, Some(path), true),
                            None => {
                                ctx.sfx(sfx::ERROR);
                                ctx.toast(
                                    "폴더 경로에 저장할 수 없는 문자가 있습니다(유니코드가 아닌 이름). \
                                     폴더 이름을 바꾸거나 다른 곳으로 옮겨 주세요.",
                                );
                                Transition::None
                            }
                        },
                        other => {
                            ctx.sfx(sfx::ERROR);
                            ctx.toast(other.summary());
                            self.show_place(ctx, place.clone(), None);
                            Transition::None
                        }
                    }
                }
                Row::Up => {
                    if let Some(p) = up {
                        self.show_place(ctx, p, here);
                    }
                    Transition::None
                }
                Row::Open(folder) => {
                    self.show_place(ctx, Place::Dir(folder.path.clone()), None);
                    Transition::None
                }
            },
            MenuEvent::Cancelled => self.leave_browser(ctx),
            _ => Transition::None,
        }
    }
}

impl Default for OriginalScreen {
    fn default() -> Self {
        OriginalScreen::new()
    }
}

impl Screen for OriginalScreen {
    fn name(&self) -> &'static str {
        "original"
    }

    fn on_enter(&mut self, ctx: &mut Ctx, how: Enter) {
        if how != Enter::Fresh {
            return;
        }
        if self.browse_only {
            let place = original::start_place(ctx.settings.original_dir.as_deref());
            self.show_place(ctx, place, None);
        } else {
            self.show_overview(ctx);
        }
    }

    fn update(&mut self, ctx: &mut Ctx) -> Transition {
        // The views are rebuilt by the handlers, so work on a copy of the rows.
        match &self.view {
            View::Overview { commands } => {
                let commands = commands.clone();
                self.update_overview(ctx, &commands)
            }
            View::Browse { place, rows } => {
                let (place, rows) = (place.clone(), rows.clone());
                self.update_browser(ctx, &place, &rows)
            }
        }
    }

    fn draw(&self, ctx: &Ctx) {
        clear_background(theme::BACKGROUND);
        let gfx = &ctx.gfx;
        let heading = match self.view {
            View::Overview { .. } => "원작 데이터",
            View::Browse { .. } => "원작 폴더 고르기",
        };
        gfx.text(
            heading,
            MARGIN,
            14.0,
            TextStyle::main(theme::TEXT_ACCENT)
                .size(2)
                .shadow(theme::TEXT_SHADOW),
        );
        if !self.lines.is_empty() {
            let frame = Rect::new(
                MARGIN - 6.0,
                TEXT_TOP - 6.0,
                gfx.size().x - 2.0 * MARGIN + 12.0,
                self.lines.len() as f32 * LINE + 10.0,
            );
            draw_window(frame);
        }
        for (i, (line, color)) in self.lines.iter().enumerate() {
            gfx.text(
                line,
                MARGIN + 4.0,
                TEXT_TOP + i as f32 * LINE,
                TextStyle::small(*color),
            );
        }
        self.menu.draw(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder(name: &str) -> Row {
        Row::Open(Folder {
            name: name.into(),
            path: PathBuf::from(name),
            install: false,
        })
    }

    #[test]
    fn letters_jump_to_folders() {
        let rows = vec![
            Row::UseThis,
            Row::Up,
            folder("Game"),
            folder("hero"),
            folder("Hermes"),
            folder("tools"),
        ];
        assert_eq!(next_starting_with(&rows, 0, 'h'), Some(3));
        assert_eq!(next_starting_with(&rows, 3, 'H'), Some(4));
        // Wraps around to the first match.
        assert_eq!(next_starting_with(&rows, 4, 'h'), Some(3));
        assert_eq!(next_starting_with(&rows, 5, 'g'), Some(2));
        assert_eq!(next_starting_with(&rows, 0, 'x'), None);
        assert_eq!(next_starting_with(&rows, 0, ' '), None);
        let rows = vec![folder("xargs"), folder("zelda")];
        assert_eq!(next_starting_with(&rows, 0, 'x'), None, "cancel key");
        assert_eq!(next_starting_with(&rows, 0, 'Z'), None, "confirm key");
    }
}
