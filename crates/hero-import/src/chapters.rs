//! The original's chapters past the base pack's campaign (docs/ORIGINAL_DATA.md, DECISIONS D18):
//! the battles and the story of a scenario file the base pack has no campaign for, as new
//! battles, drama scenes and campaign nodes that continue the base campaign where it ends.
//!
//! A scenario scene is a row of blocks. A block that loads a battle map and sets a battle up is
//! a battle ([`Part::Battle`]); the others are the story in between ([`Part::Story`]): towns and
//! the campaign map, where the original lets the player walk about and talk to people. The engine
//! has no such mode, so a story block becomes one drama scene ([`story_scene`]): its records in
//! order but the optional chatter of people one may talk to, choices and yes/no questions as
//! `@choice` (an option that neither moves the story on nor goes elsewhere asks again; one that
//! ends in `game_over` sets [`GAME_OVER_FLAG`], which a campaign branch sends to a game-over
//! ending; one that goes to another block sets the scene's route flag, which campaign branches
//! send there, [`Next`]), and the story's side effects as drama commands (officers joining, and
//! going away for a while as the original moves them to another army, items, music). The
//! campaign joins the parts as their [`Next`] says ([`continue_campaign`]). The battles are
//! re-staged like the paired ones ([`crate::battles::convert`]) from a base made from the
//! original's header ([`chapter_base`]).

use crate::battles::BATTLE_MAP;
use crate::battles::{self, Names, TextSource};
use crate::scenario::{Block, Instr, Operands, Record, Scene};
use hero_core::battledef::{BattleDef, Condition, DeployDef, MapDef};
use hero_core::campaign::{CampaignDef, Node};
use hero_core::geom::Pos;
use hero_core::script::Compare;
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Campaign flag a story scene sets when the player takes an option that ends the game in the
/// original (a campaign branch after the scene leads to [`GAME_OVER_NODE`]).
pub const GAME_OVER_FLAG: &str = "orig_game_over";
/// The ending node of a game over in a converted chapter.
pub const GAME_OVER_NODE: &str = "orig_game_over";
/// Campaign flag a story scene sets to one more than the original's ending it ends in
/// (`ending n`); a campaign branch after the scene leads to [`ending_node`].
pub const ENDING_FLAG: &str = "orig_ending";

/// The ending node of the original's ending `n`.
pub fn ending_node(n: u8) -> String {
    format!("orig_ending_{n}")
}

/// Record kind of a person one talks to (FORMATS §13.2).
const TALK: u8 = 3;

/// A block of a scenario scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// A block that loads battle map `map` and fights on it (its roster or the battle's start).
    Battle { block: usize, map: u8 },
    /// A block of story (towns, the campaign map) with something to say.
    Story { block: usize },
}

/// The parts of `scene` in block order; blocks with neither a battle nor any text are left out.
pub fn parts(scene: &Scene) -> Vec<Part> {
    let mut out = Vec::new();
    for (i, block) in scene.blocks.iter().enumerate() {
        // The second block of a battle fought in the block after its setup.
        if i > 0 && battles::fought_in_next_block(scene, i - 1) {
            continue;
        }
        let code = || block.records.iter().flat_map(|r| &r.code);
        let battle_map = code()
            .filter(|c| c.mnemonic == "load_map")
            .filter_map(|c| c.operands.get("map"))
            .find(|m| m & 0xf000 == BATTLE_MAP);
        // The setup may come in the block before (with the camp's story).
        let sets_up = code().any(|c| {
            matches!(
                c.mnemonic,
                "battle_setup" | "battle_roster" | "begin_battle"
            )
        });
        match battle_map {
            Some(m) if sets_up => out.push(Part::Battle {
                block: i,
                map: (m & 0xff) as u8,
            }),
            _ if code().any(|c| matches!(c.mnemonic, "dialogue" | "narration" | "title")) => {
                out.push(Part::Story { block: i })
            }
            _ => {}
        }
    }
    out
}

/// A story block as a drama scene.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoryScene {
    /// The scene's lines (without its `== id` heading).
    pub text: String,
    /// An option of it ends the game in the original (the scene sets [`GAME_OVER_FLAG`]).
    pub game_over: bool,
    /// The items the block's shop sells (`set_shop_items`), as pack item ids.
    pub shop: Vec<String>,
    /// Gold the outro gives (the battle's reward; outros only).
    pub gold: i64,
    /// The original's endings the scene may end in (it sets [`ENDING_FLAG`] to one more).
    pub endings: BTreeSet<u8>,
    /// Where the story goes on after the scene.
    pub next: Next,
    pub notes: Vec<String>,
}

/// Where the campaign goes after a part of a chapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Next {
    /// The next part.
    #[default]
    Default,
    /// The part of block `n` of the same scene (`goto_block`).
    Block(usize),
    /// By the value of the route flag the part's scene sets: the block of each value; for any
    /// other, block `otherwise` or the next part (the choices, questions and flag checks that
    /// `goto_block` elsewhere).
    Routes {
        flag: String,
        targets: Vec<(i64, usize)>,
        otherwise: Option<usize>,
    },
}

/// What a story scene needs besides the block.
pub struct StoryContext<'a> {
    pub names: &'a Names,
    pub text: &'a dyn TextSource,
    /// Music key of an original song number (`MUSIC.R3`), if the pack has one for it.
    pub song_key: &'a dyn Fn(u16) -> Option<&'static str>,
    /// The block's number in its scene (a `goto_block` to it asks again).
    pub block: usize,
    /// The campaign flag the scene sets for its routes.
    pub route_flag: &'a str,
    /// The original's flags some script sets (`set_flag`): a test of any other one is decided
    /// now (it is always clear).
    pub settable: &'a BTreeSet<u8>,
    /// The event pictures the pack has (`show_picture` numbers, [`crate::pack::picture_key`]).
    pub pictures: &'a BTreeSet<u8>,
    /// Whom one meets in each block of the scene (the first person to talk to): the places one
    /// can walk to are named after them.
    pub places: &'a [Option<String>],
}

/// How a script ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// It runs to its end.
    Continue,
    /// `goto_block`: the story goes on with that block.
    Goto(usize),
    /// `game_over`.
    GameOver,
    /// `ending n`: one of the original's endings.
    Ending(u8),
}

/// The state of a scene being written.
struct Writer<'c, 'a> {
    ctx: &'c StoryContext<'a>,
    out: StoryScene,
    skipped: BTreeSet<&'static str>,
    labels: usize,
    /// Route flag values given out, with their blocks.
    routes: Vec<(i64, usize)>,
    /// Gold the script gives is the battle's reward (an outro), not a `@gold` line.
    gold_as_reward: bool,
    /// Inside a record of the story: the label ending it (`rend_<n>`) once a jump uses it (a
    /// `goto_block` to the block itself inside a question or a flag check ends the record).
    rec_end: Option<Option<usize>>,
    /// Write only the army's changes (a battle's setup, [`before_scene`]).
    army_only: bool,
    /// A picture is shown (`@picture`): the next instruction but a narration clears it.
    picture: bool,
}

impl Writer<'_, '_> {
    fn label(&mut self) -> usize {
        self.labels += 1;
        self.labels
    }

    /// End the scene on a route to `block`: the flag takes a new value that a campaign branch
    /// sends there.
    fn route(&mut self, block: usize) {
        let value = self.routes.len() as i64 + 1;
        self.routes.push((value, block));
        let _ = writeln!(
            self.out.text,
            "@set {} = {value}\n@end",
            self.ctx.route_flag
        );
    }

    fn game_over(&mut self) {
        self.out.game_over = true;
        let _ = writeln!(self.out.text, "@set {GAME_OVER_FLAG} = 1\n@end");
    }

    /// End the scene on a game over or one of the original's endings.
    fn stop(&mut self, flow: Flow) {
        match flow {
            Flow::Ending(n) => {
                self.out.endings.insert(n);
                let _ = writeln!(self.out.text, "@set {ENDING_FLAG} = {}\n@end", n + 1);
            }
            _ => self.game_over(),
        }
    }

    /// End a branch of a choice or question by `flow`: `retry` asks again at label `ask`.
    fn close(&mut self, flow: Flow, retry: bool, ask: usize, after: usize) {
        match flow {
            f @ (Flow::GameOver | Flow::Ending(_)) => self.stop(f),
            Flow::Goto(b) if b == self.ctx.block => {
                let _ = writeln!(self.out.text, "@goto ask_{ask}");
            }
            Flow::Goto(b) => self.route(b),
            Flow::Continue if retry => {
                let _ = writeln!(self.out.text, "@goto ask_{ask}");
            }
            Flow::Continue => {
                let _ = writeln!(self.out.text, "@goto after_{after}");
            }
        }
    }

    /// Jump to the end of the record being written (a `goto_block` to the block itself: the
    /// original runs the block again, whose record has had its turn); `false` outside a record.
    fn end_record(&mut self) -> bool {
        let n = match self.rec_end {
            None => return false,
            Some(Some(n)) => n,
            Some(None) => {
                let n = self.label();
                self.rec_end = Some(Some(n));
                n
            }
        };
        let _ = writeln!(self.out.text, "@goto rend_{n}");
        true
    }

    /// Write record `code` of the story: its lines, then the end label when a jump uses it.
    fn record(&mut self, code: &[Instr]) -> Flow {
        self.rec_end = Some(None);
        let flow = self.lines(code);
        if let Some(Some(n)) = self.rec_end.take() {
            let _ = writeln!(self.out.text, "@label rend_{n}");
        }
        flow
    }

    /// Write the lines of `code`; returns how it ends.
    fn lines(&mut self, code: &[Instr]) -> Flow {
        let mut i = 0;
        while i < code.len() {
            let instr = &code[i];
            let get = |name: &str| instr.operands.get(name).unwrap_or(0);
            // A picture stays for the narration after it; the next other instruction closes it
            // (FORMATS §13.3 `show_picture`).
            if self.picture && !matches!(instr.mnemonic, "narration" | "show_picture") {
                let _ = writeln!(self.out.text, "@picture none");
                self.picture = false;
            }
            match instr.mnemonic {
                "goto_block" => return Flow::Goto(usize::from(get("block"))),
                "game_over" => return Flow::GameOver,
                "ending" => return Flow::Ending(get("ending") as u8),
                "if_answer" => {
                    // The instructions it guards run when the player answered `answer` (0 =
                    // yes) to the question just asked.
                    let end = (i + 1 + usize::from(get("skip"))).min(code.len());
                    let guarded = &code[i + 1..end];
                    let sortie = guarded
                        .iter()
                        .any(|c| matches!(c.mnemonic, "op_3d" | "battle_setup" | "begin_battle"));
                    if sortie || get("answer") != 0 {
                        // "Ready to set out?": the story goes on as if the player said yes.
                        i += 1;
                        continue;
                    }
                    let (ask, after) = (self.label(), self.label());
                    let _ = writeln!(
                        self.out.text,
                        "@label ask_{ask}\n@choice\n- 예 -> yes_{ask}\n- 아니오 -> after_{after}\n@label yes_{ask}"
                    );
                    match self.lines(guarded) {
                        // Back to the block itself: the record ends (outside one, the story
                        // goes on here).
                        Flow::Goto(b) if b == self.ctx.block => {
                            if !self.end_record() {
                                self.close(Flow::Continue, false, ask, after);
                            }
                        }
                        flow => self.close(flow, false, ask, after),
                    }
                    let _ = writeln!(self.out.text, "@label after_{after}");
                    i = end;
                    continue;
                }
                "if_flags" => {
                    // The instructions it guards run when every flag of `all_set` is set and
                    // every flag of `all_clear` clear: the original's flags are campaign flags
                    // (`orig_f<n>`), and a jump there goes elsewhere only then.
                    if let Operands::Condition {
                        skip,
                        all_set,
                        all_clear,
                    } = &instr.operands
                    {
                        let end = (i + 1 + usize::from(*skip)).min(code.len());
                        // A flag no script sets is always clear.
                        if all_set.iter().any(|f| !self.ctx.settable.contains(f)) {
                            i = end;
                            continue;
                        }
                        let all_clear: Vec<u8> = all_clear
                            .iter()
                            .copied()
                            .filter(|f| self.ctx.settable.contains(f))
                            .collect();
                        let k = self.label();
                        for f in all_set {
                            let _ = writeln!(self.out.text, "@if {} == 0 -> skip_{k}", flag(*f));
                        }
                        for f in &all_clear {
                            let _ = writeln!(self.out.text, "@if {} != 0 -> skip_{k}", flag(*f));
                        }
                        match self.lines(&code[i + 1..end]) {
                            f @ (Flow::GameOver | Flow::Ending(_)) => self.stop(f),
                            Flow::Goto(b) if b != self.ctx.block => self.route(b),
                            Flow::Goto(_) => {
                                self.end_record();
                            }
                            Flow::Continue => {}
                        }
                        let _ = writeln!(self.out.text, "@label skip_{k}");
                        i = end;
                        continue;
                    }
                }
                _ => self.effect(instr),
            }
            i += 1;
        }
        Flow::Continue
    }

    /// One instruction that is not a jump.
    fn effect(&mut self, instr: &Instr) {
        if self.army_only && !matches!(instr.mnemonic, "set_allegiance" | "set_country") {
            return;
        }
        let ctx = self.ctx;
        let names = ctx.names;
        let officer = |person: u16| names.officers.get(&person).cloned();
        let get = |name: &str| instr.operands.get(name).unwrap_or(0);
        let out = &mut self.out;
        match instr.mnemonic {
            "dialogue" => match ctx.text.dialogue(get("text")) {
                Ok(lines) => {
                    for (speaker, text) in lines {
                        let head = match officer(speaker) {
                            Some(id) => format!("{id}:"),
                            None => format!(
                                "{}:",
                                battles::free_speaker(
                                    names
                                        .person_names
                                        .get(&speaker)
                                        .map_or("???", String::as_str)
                                )
                            ),
                        };
                        battles::push_text(&mut out.text, &head, &text);
                    }
                }
                Err(e) => out.notes.push(e),
            },
            "narration" | "caption" => match ctx.text.string(get("text")) {
                Ok(text) => battles::push_text(&mut out.text, "@narr", &text),
                Err(e) => out.notes.push(e),
            },
            "title" => match ctx.text.string(get("text")) {
                Ok(text) => battles::push_text(&mut out.text, "@title", &text),
                Err(e) => out.notes.push(e),
            },
            "play_music" => match (ctx.song_key)(get("song")) {
                Some(key) => {
                    let _ = writeln!(out.text, "@bgm {key}");
                }
                None => {
                    self.skipped.insert("songs the pack has no music key for");
                }
            },
            "set_country" => match officer(get("person")) {
                Some(id) if get("country") == 0 => {
                    let _ = writeln!(out.text, "@join {id}");
                }
                // A battle's setup assigns its enemies too: they have no army to leave.
                Some(id) if self.army_only && !names.player_officers.contains(&id) => {}
                Some(id) => {
                    let _ = writeln!(out.text, "@away {id}");
                }
                None => {
                    self.skipped
                        .insert("allegiances of persons without a pack officer");
                }
            },
            "set_allegiance" => match officer(get("person")) {
                // Army 0 is Liu Bei's; an officer of the army moved to another (or to none, 14)
                // is away for a while and keeps their progress (the original brings officers
                // back the same way).
                Some(id) if get("army") == 0 => {
                    let _ = writeln!(out.text, "@join {id}");
                }
                // A battle's setup assigns its enemies too: they have no army to leave.
                Some(id) if self.army_only && !names.player_officers.contains(&id) => {}
                Some(id) => {
                    let _ = writeln!(out.text, "@away {id}");
                }
                None => {
                    self.skipped
                        .insert("allegiances of persons without a pack officer");
                }
            },
            "add_item" => match names.items.get(&(get("item") as u8)) {
                Some(id) => {
                    let _ = writeln!(out.text, "@item {id}");
                }
                None => {
                    self.skipped.insert("items without a pack item");
                }
            },
            "set_flag" => {
                let value = i64::from(get("clear") == 0);
                let _ = writeln!(out.text, "@set {} = {value}", flag(get("flag") as u8));
            }
            "set_shop_items" => {
                if let Operands::Bytes { bytes } = &instr.operands {
                    out.shop = bytes
                        .iter()
                        .filter_map(|b| names.items.get(b).cloned())
                        .collect();
                }
            }
            "show_picture" => {
                let n = get("picture") as u8;
                if ctx.pictures.contains(&n) {
                    let _ = writeln!(out.text, "@picture {}", crate::pack::picture_key(n));
                    self.picture = true;
                } else {
                    self.skipped.insert("pictures the pack does not have");
                }
            }
            "add_levels" | "set_class" => {
                self.skipped.insert("level and class changes");
            }
            "data" if get("kind") == DATA_GOLD => {
                if self.gold_as_reward {
                    out.gold += i64::from(get("value"));
                } else {
                    let _ = writeln!(out.text, "@gold {}", get("value"));
                }
            }
            "data" => {
                self.skipped.insert("`data` payloads other than gold");
            }
            _ => {}
        }
    }

    fn finish(mut self) -> StoryScene {
        if !self.skipped.is_empty() {
            self.out.notes.push(format!(
                "left out: {}",
                self.skipped.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        if !self.routes.is_empty() {
            // A scene played again (a town one walks back to) starts without the last route.
            self.out.text = format!(
                "@set {} = 0
{}",
                self.ctx.route_flag, self.out.text
            );
            let otherwise = match self.out.next {
                Next::Block(b) => Some(b),
                _ => None,
            };
            self.out.next = Next::Routes {
                flag: self.ctx.route_flag.to_string(),
                targets: self.routes,
                otherwise,
            };
        }
        self.out
    }
}

/// The campaign flag of the original's flag `n` (FORMATS §13.3 `set_flag`).
pub fn flag(n: u8) -> String {
    format!("orig_f{n}")
}

/// Record kinds of a place one walks to (FORMATS §13.2): a town's location, a place on the
/// campaign map.
const PLACES: [u8; 2] = [2, 5];

/// Whether `r` is walking to a place that goes to another block: an option of where to go.
fn walks(r: &Record) -> bool {
    PLACES.contains(&r.trigger.kind) && r.code.iter().any(|c| c.mnemonic == "goto_block")
}

/// Whether `r` leaves its group's parallel control (moves the story on).
fn leaves(r: &Record) -> bool {
    r.code.iter().any(|c| c.mnemonic == "leave_parallel")
}

/// Whether `r` changes the army, the inventory or the original's flags.
fn has_effects(r: &Record) -> bool {
    r.code.iter().any(|c| {
        matches!(
            c.mnemonic,
            "set_allegiance" | "add_item" | "set_shop_items" | "data" | "set_flag"
        )
    })
}

impl<'c, 'a> Writer<'c, 'a> {
    fn new(ctx: &'c StoryContext<'a>, gold_as_reward: bool) -> Self {
        Writer {
            ctx,
            out: StoryScene::default(),
            skipped: BTreeSet::new(),
            labels: 0,
            routes: Vec::new(),
            gold_as_reward,
            rec_end: None,
            army_only: false,
            picture: false,
        }
    }

    /// Write `records` of `block` (the indexes `from..`) as the story reads them: in order but
    /// the optional chatter of people one may talk to, with their choices (see the module docs).
    fn story(&mut self, block: &Block, from: usize) {
        let ctx = self.ctx;
        // Groups with a record that moves the story on: their other talks are optional chatter.
        let progressing: BTreeSet<u8> = block.records[from..]
            .iter()
            .filter(|r| leaves(r))
            .map(|r| r.trigger.group)
            .collect();
        let chatter = |r: &Record| {
            r.trigger.kind == TALK
                && !leaves(r)
                && progressing.contains(&r.trigger.group)
                && !has_effects(r)
                // A talk that asks leads to the records of its options.
                && !r.code.iter().any(|c| c.mnemonic == "choice")
        };
        let mut taken = BTreeSet::new();
        for (i, rec) in block.records.iter().enumerate().skip(from) {
            if taken.contains(&i) || chatter(rec) {
                continue;
            }
            // Walking to places of the group that lead to other blocks: where to go is a
            // choice (a place one cannot go to yet, and the one here, ask again).
            if walks(rec) {
                let group: Vec<usize> = (i..block.records.len())
                    .take_while(|&k| block.records[k].trigger.group == rec.trigger.group)
                    .filter(|&k| walks(&block.records[k]))
                    .collect();
                if group.len() > 1 {
                    self.walk(block, &group);
                    taken.extend(group);
                    continue;
                }
            }
            // A choice ends its record's script: option `k` goes on with record `i + 1 + k`; an
            // option that neither leaves the group nor goes elsewhere asks again.
            if let Some(at) = rec.code.iter().position(|c| c.mnemonic == "choice") {
                match self.lines(&rec.code[..at]) {
                    Flow::Continue => {}
                    f @ (Flow::GameOver | Flow::Ending(_)) => {
                        self.stop(f);
                        break;
                    }
                    Flow::Goto(b) => {
                        self.out.next = Next::Block(b);
                        break;
                    }
                }
                let options = rec.code[at]
                    .operands
                    .get("options")
                    .ok_or_else(|| "choice without options".to_string())
                    .and_then(|o| ctx.text.string(o));
                let options: Vec<String> = match options {
                    Ok(t) => t
                        .split('\n')
                        .map(|l| l.trim_end_matches('\r').trim().to_string())
                        .filter(|l| !l.is_empty())
                        .collect(),
                    Err(e) => {
                        self.out
                            .notes
                            .push(format!("record {i}: choice left out: {e}"));
                        continue;
                    }
                };
                // Asking again needs an option that goes on: without one, every option does.
                let targets = || (0..options.len()).filter_map(|k| block.records.get(i + 1 + k));
                let goes_on = targets().any(|t| {
                    leaves(t)
                        || t.code.iter().any(|c| {
                            c.mnemonic == "game_over"
                                || (c.mnemonic == "goto_block"
                                    && c.operands.get("block") != Some(ctx.block as u16))
                        })
                });
                let (ask, after) = (self.label(), self.label());
                let _ = writeln!(self.out.text, "@label ask_{ask}\n@choice");
                for (k, option) in options.iter().enumerate() {
                    let _ = writeln!(
                        self.out.text,
                        "- {} -> opt_{ask}_{k}",
                        option.replace("->", "→")
                    );
                }
                for k in 0..options.len() {
                    let _ = writeln!(self.out.text, "@label opt_{ask}_{k}");
                    match block.records.get(i + 1 + k) {
                        Some(target) => {
                            taken.insert(i + 1 + k);
                            let flow = self.lines(&target.code);
                            self.close(flow, goes_on && !leaves(target), ask, after);
                        }
                        None => {
                            self.out.notes.push(format!(
                                "record {i}: option {k} has no record to go on with"
                            ));
                            let _ = writeln!(self.out.text, "@goto after_{after}");
                        }
                    }
                }
                let _ = writeln!(self.out.text, "@label after_{after}");
                continue;
            }
            if rec.code.iter().any(|c| c.mnemonic == "game_over")
                && !rec.code.iter().any(|c| c.mnemonic == "leave_parallel")
            {
                // A game over that no choice leads to (a failed errand): not part of the story.
                continue;
            }
            match self.record(&rec.code) {
                Flow::Continue => {}
                f @ (Flow::GameOver | Flow::Ending(_)) => {
                    self.stop(f);
                    break;
                }
                // The block ends there.
                Flow::Goto(b) if b != ctx.block => {
                    self.out.next = Next::Block(b);
                    break;
                }
                Flow::Goto(_) => {}
            }
        }
    }
}

impl Writer<'_, '_> {
    /// A choice of the places the records `walks` of `block` walk to.
    fn walk(&mut self, block: &Block, walks: &[usize]) {
        let ctx = self.ctx;
        let (ask, after) = (self.label(), self.label());
        let _ = writeln!(
            self.out.text,
            "@label ask_{ask}
@choice"
        );
        let mut options = Vec::new();
        for &k in walks {
            let to = block.records[k]
                .code
                .iter()
                .find(|c| c.mnemonic == "goto_block")
                .and_then(|c| c.operands.get("block"))
                .map(usize::from);
            // Staying here is not an option.
            if to.is_none_or(|b| b == ctx.block) {
                continue;
            }
            let place = to
                .and_then(|b| ctx.places.get(b).cloned().flatten())
                .map_or_else(
                    || "다른 곳으로 간다".to_string(),
                    |p| format!("{p}에게 간다"),
                );
            let _ = writeln!(
                self.out.text,
                "- {} -> opt_{ask}_{k}",
                place.replace("->", "→")
            );
            options.push(k);
        }
        for k in options {
            let _ = writeln!(self.out.text, "@label opt_{ask}_{k}");
            let flow = self.lines(&block.records[k].code);
            self.close(flow, true, ask, after);
        }
        let _ = writeln!(self.out.text, "@label after_{after}");
    }
}

/// The drama scene of a story block (see the module docs).
pub fn story_scene(block: &Block, ctx: &StoryContext) -> StoryScene {
    let mut w = Writer::new(ctx, false);
    w.story(block, 0);
    w.finish()
}

/// Record kind of the script the original runs when the battle is won.
const BATTLE_WON: u8 = 7;
/// Record kinds a battle watches (FORMATS §13.2): contact, a unit on a cell, won, lost, a turn,
/// a unit in an area, a unit defeated. The groups with them are the battle's stages.
const BATTLE_KINDS: [u8; 7] = [4, 6, BATTLE_WON, 8, 9, 11, 12];
/// `data` kind that adds gold.
const DATA_GOLD: u16 = 2;

/// What the original plays after a battle of `block` is won, as a drama scene (the battle's
/// outro): the victory script of the battle's last stage that has one, then the groups after
/// the last stage as the story reads them ([`story_scene`]: the epilogue, where officers join, go
/// away or are asked to). A `goto_block` there is where the story goes on. The gold they give is
/// the battle's reward ([`StoryScene::gold`]), shown with the battle's result.
pub fn victory_scene(block: &Block, ctx: &StoryContext) -> StoryScene {
    let mut w = Writer::new(ctx, true);
    let won = block
        .records
        .iter()
        .filter(|r| r.trigger.kind == BATTLE_WON)
        .max_by_key(|r| r.trigger.group);
    if let Some(won) = won {
        // Its first record of that group: the one the original runs.
        let first = block
            .records
            .iter()
            .find(|r| r.trigger.kind == BATTLE_WON && r.trigger.group == won.trigger.group)
            .unwrap_or(won);
        match w.lines(&first.code) {
            Flow::Goto(b) if b != ctx.block => w.out.next = Next::Block(b),
            f @ (Flow::GameOver | Flow::Ending(_)) => w.stop(f),
            _ => {}
        }
    }
    if w.out.next == Next::Default && !w.out.game_over && w.out.endings.is_empty() {
        if let Some(from) = epilogue(block) {
            w.story(block, from);
        }
    }
    w.finish()
}

/// What the original changes in the army as it sets a battle of `block` up (the setup's
/// `set_allegiance`, behind its flag checks): officers joining for it (Guan Yu's troop at
/// Maicheng) or coming back (chapter 4's detachment). Played before the battle's camp; empty
/// when the setup changes nothing.
pub fn before_scene(block: &Block, ctx: &StoryContext) -> StoryScene {
    let mut w = Writer::new(ctx, false);
    w.army_only = true;
    for rec in block
        .records
        .iter()
        .filter(|r| r.trigger.group < battles::FIRST_PHASE_GROUP)
    {
        // The setup goes on to the battle: its jumps are not the story's.
        let _ = w.lines(&rec.code);
    }
    // Only flag checks around nothing: no scene.
    if !w
        .out
        .text
        .lines()
        .any(|l| l.starts_with("@join ") || l.starts_with("@away "))
    {
        w.out.text.clear();
    }
    w.finish()
}

/// Record kind of the script the original runs when the battle is lost.
const BATTLE_LOST: u8 = 8;

/// What the original plays when a battle of `block` is lost and the story goes on (a
/// `battle_lost` script without `game_over`, of the last stage that has one): a drama scene whose
/// `next` is where the story goes on ([`Next::Default`]: the block after the battle's). `None`
/// when losing ends the game.
pub fn defeat_scene(block: &Block, ctx: &StoryContext) -> Option<StoryScene> {
    let last = block
        .records
        .iter()
        .filter(|r| r.trigger.kind == BATTLE_LOST)
        .map(|r| r.trigger.group)
        .max()?;
    let lost = block
        .records
        .iter()
        .find(|r| r.trigger.kind == BATTLE_LOST && r.trigger.group == last)?;
    if lost.code.iter().any(|c| c.mnemonic == "game_over") {
        return None;
    }
    let mut w = Writer::new(ctx, false);
    match w.lines(&lost.code) {
        Flow::Goto(b) if b != ctx.block => w.out.next = Next::Block(b),
        f @ (Flow::GameOver | Flow::Ending(_)) => w.stop(f),
        _ => {}
    }
    // The epilogue runs after a lost battle too (Maicheng's troop leaves the army).
    if w.out.next == Next::Default && !w.out.game_over && w.out.endings.is_empty() {
        if let Some(from) = epilogue(block) {
            w.story(block, from);
        }
    }
    Some(w.finish())
}

/// The first record of the groups after a battle block's last stage (its epilogue, which runs
/// when the battle is over), if it has stages.
fn epilogue(block: &Block) -> Option<usize> {
    let last = block
        .records
        .iter()
        .filter(|r| BATTLE_KINDS.contains(&r.trigger.kind))
        .map(|r| r.trigger.group)
        .max()?;
    Some(
        block
            .records
            .iter()
            .position(|r| r.trigger.group > last)
            .unwrap_or(block.records.len()),
    )
}

/// A base for re-staging a battle the base pack does not have: the original's name and
/// objective, its turn limit, victory by defeating its commander (its header's officer) or every
/// enemy, and Liu Bei reaching the objective area when the original has one (conditions that
/// [`crate::battles::convert`] completes from the original battle).
pub fn chapter_base(
    id: &str,
    name: &str,
    objective: &str,
    turn_limit: u32,
    commander: bool,
    lord: Option<&str>,
) -> BattleDef {
    let first = if commander {
        Condition::DefeatCommander
    } else {
        Condition::DefeatAll
    };
    BattleDef {
        id: id.to_string(),
        name: name.to_string(),
        location: String::new(),
        objective: objective.to_string(),
        bgm: Some("battle".into()),
        bgm_enemy: Some("enemy".into()),
        turn_limit,
        map: MapDef::default(),
        deploy: DeployDef {
            max: 12,
            required: Vec::new(),
            forbidden: Vec::new(),
            slots: Vec::new(),
        },
        units: Vec::new(),
        victory: std::iter::once(first)
            .chain(lord.map(|lord| Condition::Reach {
                who: Some(lord.to_string()),
                pos: Pos::new(0, 0),
                radius: 0,
                to: None,
            }))
            .collect(),
        defeat: Vec::new(),
        bonus: None,
        events: Vec::new(),
        treasures: Vec::new(),
        reward_gold: 0,
        intro: None,
        outro: None,
    }
}

/// The nodes `node` may go on to.
fn successors(node: &Node) -> Vec<&str> {
    match node {
        Node::Drama { next, .. } | Node::Camp { next, .. } => vec![next],
        Node::Battle {
            next, on_defeat, ..
        } => std::iter::once(next.as_str())
            .chain(on_defeat.as_deref())
            .collect(),
        Node::Branch {
            then, otherwise, ..
        } => vec![then, otherwise],
        Node::Ending { .. } => Vec::new(),
    }
}

/// One part of a converted chapter, in scenario order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Where it is in the scenario: `(file, scene, block)`.
    pub at: (usize, usize, usize),
    pub kind: StepKind,
    /// Where the campaign goes after it.
    pub next: Next,
    /// How its scene (a story's, a battle's outro) may end the campaign.
    pub ends: Ends,
}

/// How a scene may end the campaign.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ends {
    /// It may set [`GAME_OVER_FLAG`].
    pub game_over: bool,
    /// The original's endings it may set [`ENDING_FLAG`] for.
    pub endings: BTreeSet<u8>,
}

impl Ends {
    pub fn of(scene: &StoryScene) -> Ends {
        Ends {
            game_over: scene.game_over,
            endings: scene.endings.clone(),
        }
    }
}

/// What a [`Step`] plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// Drama scene `scene`.
    Story { scene: String },
    /// Battle `battle`, prepared in a camp titled `title` whose shop sells `shop`, and what the
    /// campaign plays when it is lost, when the original goes on then.
    Battle {
        battle: String,
        title: String,
        shop: Vec<String>,
        defeat: Option<Defeat>,
        /// The scene of the army's changes the battle's setup makes, played before its camp.
        before: Option<String>,
    },
}

/// What the original plays when a battle is lost and the story goes on (`battle_lost`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defeat {
    /// Its drama scene.
    pub scene: String,
    /// Where the story goes on ([`Next::Default`]: the block after the battle's).
    pub next: Next,
    pub ends: Ends,
}

impl Step {
    /// The id of the step's first node.
    fn first_node(&self) -> String {
        match &self.kind {
            StepKind::Story { scene } => scene.clone(),
            StepKind::Battle {
                before: Some(scene),
                ..
            } => scene.clone(),
            StepKind::Battle { battle, .. } => format!("{battle}_camp"),
        }
    }
}

/// The step the story goes on to from step `i` of the steps at `at` (`(file, scene, block)`, in
/// order) by `next` (a route's [`Next::Block`] or not a route): for a block, the first step of
/// the same scene at or after it (else the first of a later scene); `None` for the chapter's end.
fn step_after(at: &[(usize, usize, usize)], i: usize, next: &Next) -> Option<usize> {
    let (file, scene, _) = at[i];
    match next {
        Next::Block(b) => at
            .iter()
            .enumerate()
            .filter(|(_, a)| a.0 == file && a.1 == scene && a.2 >= *b)
            .min_by_key(|(_, a)| a.2)
            .or_else(|| {
                at.iter()
                    .enumerate()
                    .find(|(_, a)| (a.0, a.1) > (file, scene))
            })
            .map(|(k, _)| k),
        _ => (i + 1 < at.len()).then_some(i + 1),
    }
}

/// Where a lost battle at `at` goes on by `next`: [`Next::Default`] is the block after it.
pub fn after_defeat(at: (usize, usize, usize), next: &Next) -> Next {
    match next {
        Next::Default => Next::Block(at.2 + 1),
        other => other.clone(),
    }
}

/// Which of the steps at `at` (in order, each going on as its `nexts` say: a story's or battle's
/// next, a lost battle's) the story reaches from the first; the others are the alternatives of a
/// choice the conversion does not offer.
pub fn reachable(at: &[(usize, usize, usize)], nexts: &[Vec<Next>]) -> Vec<bool> {
    let mut seen = vec![false; at.len()];
    let mut todo: Vec<usize> = if at.is_empty() { Vec::new() } else { vec![0] };
    while let Some(i) = todo.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        for next in &nexts[i] {
            match next {
                Next::Routes {
                    targets, otherwise, ..
                } => {
                    todo.extend(step_after(
                        at,
                        i,
                        &otherwise.map_or(Next::Default, Next::Block),
                    ));
                    for (_, b) in targets {
                        todo.extend(step_after(at, i, &Next::Block(*b)));
                    }
                }
                n => todo.extend(step_after(at, i, n)),
            }
        }
    }
    seen
}

/// `base` with the steps played after its battle node that fights `after_battle`, instead of
/// the node it went on to, and ending with `ending` (a node id and a title) when the story runs
/// out; the steps are joined as their [`Next`] says (routes become branches on their flags), a
/// game over goes to [`GAME_OVER_NODE`] and the original's endings to their [`ending_node`]s, and
/// a battle the original goes on after losing plays its defeat scene. `None` when the base
/// campaign has no such battle node.
pub fn continue_campaign(
    base: &CampaignDef,
    after_battle: &str,
    steps: &[Step],
    ending: (&str, &str),
) -> Option<CampaignDef> {
    let at = base
        .nodes
        .iter()
        .position(|n| matches!(n, Node::Battle { battle, .. } if battle == after_battle))?;
    let Node::Battle { next: old_next, .. } = &base.nodes[at] else {
        unreachable!("found above");
    };
    let old_next = old_next.clone();
    let mut campaign = base.clone();
    let ending_id = ending.0.to_string();
    let places: Vec<_> = steps.iter().map(|s| s.at).collect();
    // The node the story goes on to from step `i` by `next` (not a route).
    let target = |i: usize, next: &Next| -> String {
        step_after(&places, i, next).map_or_else(|| ending_id.clone(), |k| steps[k].first_node())
    };
    let mut nodes: Vec<Node> = Vec::new();
    let mut game_over = false;
    let mut endings = BTreeSet::new();
    // `next` behind the checks of a scene `own` that may end the campaign as `ends` says.
    let mut checked = |nodes: &mut Vec<Node>, own: &str, ends: &Ends, next: String| {
        let mut next = next;
        for &n in ends.endings.iter().rev() {
            let id = format!("{own}_ending{n}");
            nodes.push(Node::Branch {
                id: id.clone(),
                flag: ENDING_FLAG.to_string(),
                cmp: Compare::Eq,
                value: i64::from(n) + 1,
                then: ending_node(n),
                otherwise: next,
            });
            endings.insert(n);
            next = id;
        }
        if ends.game_over {
            game_over = true;
            let id = format!("{own}_check");
            nodes.push(Node::Branch {
                id: id.clone(),
                flag: GAME_OVER_FLAG.to_string(),
                cmp: Compare::Eq,
                value: 1,
                then: GAME_OVER_NODE.to_string(),
                otherwise: next,
            });
            next = id;
        }
        next
    };
    for (i, step) in steps.iter().enumerate() {
        let first = step.first_node();
        // Where the step goes on: its routes' branches, else the target.
        let next = match &step.next {
            Next::Routes {
                flag,
                targets,
                otherwise,
            } => {
                let default = target(i, &otherwise.map_or(Next::Default, Next::Block));
                let ids: Vec<String> = (0..targets.len())
                    .map(|k| format!("{first}_route{}", k + 1))
                    .collect();
                for (k, (value, block)) in targets.iter().enumerate() {
                    nodes.push(Node::Branch {
                        id: ids[k].clone(),
                        flag: flag.clone(),
                        cmp: Compare::Eq,
                        value: *value,
                        then: target(i, &Next::Block(*block)),
                        otherwise: ids.get(k + 1).cloned().unwrap_or_else(|| default.clone()),
                    });
                }
                ids.first().cloned().unwrap_or(default)
            }
            other => target(i, other),
        };
        match &step.kind {
            StepKind::Story { scene } => {
                let next = checked(&mut nodes, scene, &step.ends, next);
                nodes.push(Node::Drama {
                    id: scene.clone(),
                    scene: scene.clone(),
                    next,
                });
            }
            StepKind::Battle {
                battle,
                title,
                shop,
                defeat,
                before,
            } => {
                let camp = format!("{battle}_camp");
                if let Some(scene) = before {
                    nodes.push(Node::Drama {
                        id: scene.clone(),
                        scene: scene.clone(),
                        next: camp.clone(),
                    });
                }
                let next = checked(&mut nodes, battle, &step.ends, next);
                let on_defeat = defeat.as_ref().map(|d| {
                    let after = target(i, &after_defeat(step.at, &d.next));
                    let after = checked(&mut nodes, &d.scene, &d.ends, after);
                    nodes.push(Node::Drama {
                        id: d.scene.clone(),
                        scene: d.scene.clone(),
                        next: after,
                    });
                    d.scene.clone()
                });
                let fight = format!("{battle}_battle");
                nodes.push(Node::Camp {
                    id: camp,
                    title: title.clone(),
                    shop: shop.clone(),
                    battle: Some(battle.clone()),
                    next: fight.clone(),
                });
                nodes.push(Node::Battle {
                    id: fight,
                    battle: battle.clone(),
                    next,
                    on_defeat,
                });
            }
        }
    }
    // The chapter's end, when the story runs out rather than ending in one of the original's.
    let first = steps
        .first()
        .map_or_else(|| ending_id.clone(), Step::first_node);
    if first == ending_id
        || nodes
            .iter()
            .any(|n| successors(n).contains(&ending_id.as_str()))
    {
        nodes.push(Node::Ending {
            id: ending_id.clone(),
            scene: None,
            title: ending.1.to_string(),
        });
    }
    for n in endings {
        nodes.push(Node::Ending {
            id: ending_node(n),
            scene: None,
            title: format!("엔딩 {}", u32::from(n) + 1),
        });
    }
    if game_over {
        nodes.push(Node::Ending {
            id: GAME_OVER_NODE.to_string(),
            scene: None,
            title: "게임 오버".to_string(),
        });
    }
    if let Node::Battle { next, .. } = &mut campaign.nodes[at] {
        *next = first.clone();
    }
    // The node the base campaign went on to is replaced when nothing else leads to it; an
    // ending's scene (the base chapter's close) still plays, before the chapters.
    let still_used = campaign
        .nodes
        .iter()
        .any(|n| successors(n).contains(&old_next.as_str()));
    if !still_used {
        let scene = campaign.nodes.iter().find_map(|n| match n {
            Node::Ending {
                id,
                scene: Some(scene),
                ..
            } if *id == old_next => Some(scene.clone()),
            _ => None,
        });
        campaign.nodes.retain(|n| n.id() != old_next);
        if let Some(scene) = scene {
            if let Node::Battle { next, .. } = &mut campaign.nodes[at] {
                *next = old_next.clone();
            }
            campaign.nodes.push(Node::Drama {
                id: old_next,
                scene,
                next: first,
            });
        }
    }
    campaign.nodes.extend(nodes);
    Some(campaign)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Arg, ArgKind, Trigger};
    use std::collections::BTreeMap;

    fn instr(mnemonic: &'static str, args: &[(&'static str, u16)]) -> Instr {
        Instr {
            offset: 0,
            opcode: 0,
            mnemonic,
            operands: Operands::Fields {
                args: args
                    .iter()
                    .map(|&(name, value)| Arg {
                        name,
                        kind: ArgKind::Number,
                        value,
                    })
                    .collect(),
            },
        }
    }

    fn record(kind: u8, group: u8, code: Vec<Instr>) -> Record {
        Record {
            offset: 0,
            trigger: Trigger {
                kind,
                kind_name: "",
                inverted: false,
                group,
                group_flag: false,
                args: [0; 6],
            },
            code_offset: 0,
            code,
        }
    }

    fn block(records: Vec<Record>) -> Block {
        Block { offset: 0, records }
    }

    struct Text;
    impl TextSource for Text {
        fn dialogue(&self, offset: u16) -> Result<Vec<(u16, String)>, String> {
            Ok(match offset {
                1 => vec![(9, "잡담".into())],
                2 => vec![(9, "흥.".into()), (0, "무슨 일입니까?".into())],
                3 => vec![(9, "처형하라!".into())],
                4 => vec![(9, "실례했소.".into())],
                5 => vec![(63, "돌아왔습니다.".into())],
                _ => return Err(format!("no dialogue at {offset}")),
            })
        }
        fn string(&self, offset: u16) -> Result<String, String> {
            match offset {
                10 => Ok("예.\r\n아니오.".into()),
                11 => Ok("유비가 죽었다.".into()),
                _ => Err(format!("no string at {offset}")),
            }
        }
    }

    const RUN: u8 = 0;

    /// Pictures the test pack has.
    static PICTURES: std::sync::LazyLock<BTreeSet<u8>> =
        std::sync::LazyLock::new(|| BTreeSet::from([12]));

    /// Flags the test scenarios set.
    static SETTABLE: std::sync::LazyLock<BTreeSet<u8>> =
        std::sync::LazyLock::new(|| BTreeSet::from([7, 150]));

    fn if_flags(skip: u8, all_set: Vec<u8>, all_clear: Vec<u8>) -> Instr {
        Instr {
            offset: 0,
            opcode: 0x21,
            mnemonic: "if_flags",
            operands: Operands::Condition {
                skip,
                all_set,
                all_clear,
            },
        }
    }

    fn names() -> Names {
        Names {
            officers: BTreeMap::from([(0, "liu_bei".into()), (9, "yuan_shao".into())]),
            person_names: BTreeMap::from([(63, "손건".into())]),
            classes: BTreeMap::new(),
            items: BTreeMap::from([(3, "bean".into())]),
            player_officers: BTreeSet::new(),
        }
    }

    #[test]
    fn a_scene_splits_into_story_and_battles() {
        let scene = Scene {
            blocks: vec![
                block(vec![record(0, 0, vec![instr("dialogue", &[("text", 2)])])]),
                block(vec![record(
                    0,
                    0,
                    vec![
                        instr("load_map", &[("map", 0x3010)]),
                        instr("battle_roster", &[]),
                    ],
                )]),
                // Nothing to say, no battle.
                block(vec![record(
                    0,
                    0,
                    vec![instr("load_map", &[("map", 0x2005)])],
                )]),
                // A battle map without a battle (a view of it) is not a battle.
                block(vec![record(
                    0,
                    0,
                    vec![
                        instr("load_map", &[("map", 0x3011)]),
                        instr("narration", &[("text", 11)]),
                    ],
                )]),
            ],
        };
        assert_eq!(
            parts(&scene),
            [
                Part::Story { block: 0 },
                Part::Battle { block: 1, map: 16 },
                Part::Story { block: 3 },
            ]
        );
    }

    fn ctx<'a>(
        names: &'a Names,
        song_key: &'a dyn Fn(u16) -> Option<&'static str>,
    ) -> StoryContext<'a> {
        StoryContext {
            names,
            text: &Text,
            song_key,
            block: 2,
            route_flag: "route",
            places: &[],
            settable: &SETTABLE,
            pictures: &PICTURES,
        }
    }

    fn parses(text: &str) {
        let scenes = hero_core::script::parse_drama("t.drama", &format!("== s\n{text}")).unwrap();
        assert_eq!(scenes.len(), 1);
    }

    #[test]
    fn a_story_block_reads_in_order_with_its_choices() {
        let b = block(vec![
            // Chatter one may skip.
            record(TALK, 0, vec![instr("dialogue", &[("text", 1)])]),
            // The talk that moves the story on, with side effects.
            record(
                TALK,
                0,
                vec![
                    instr("dialogue", &[("text", 2)]),
                    instr("set_allegiance", &[("person", 9), ("army", 0)]),
                    instr("add_item", &[("item", 3)]),
                    instr("play_music", &[("song", 5)]),
                    instr("leave_parallel", &[]),
                ],
            ),
            // The choice: option 0 goes on with the next record, option 1 with the one after.
            record(1, 1, vec![instr("choice", &[("options", 10)])]),
            record(
                0,
                1,
                vec![
                    instr("dialogue", &[("text", 3)]),
                    instr("narration", &[("text", 11)]),
                    instr("game_over", &[]),
                ],
            ),
            record(
                0,
                1,
                vec![
                    instr("dialogue", &[("text", 4)]),
                    instr("leave_parallel", &[]),
                ],
            ),
            // A failed errand's game over that no choice leads to.
            record(0, 2, vec![instr("game_over", &[])]),
            record(
                2,
                3,
                vec![
                    instr("dialogue", &[("text", 5)]),
                    instr("set_allegiance", &[("person", 9), ("army", 3)]),
                ],
            ),
        ]);
        let song_key = |song: u16| (song == 5).then_some("peace");
        let names = names();
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert!(s.game_over);
        assert_eq!(s.next, Next::Default);
        assert_eq!(
            s.text,
            "yuan_shao: 흥.\nliu_bei: 무슨 일입니까?\n@join yuan_shao\n@item bean\n@bgm peace\n\
             @label ask_1\n@choice\n- 예. -> opt_1_0\n- 아니오. -> opt_1_1\n\
             @label opt_1_0\nyuan_shao: 처형하라!\n@narr 유비가 죽었다.\n@set orig_game_over = 1\n@end\n\
             @label opt_1_1\nyuan_shao: 실례했소.\n@goto after_2\n@label after_2\n\
             손건: 돌아왔습니다.\n@away yuan_shao\n"
        );
        parses(&s.text);
    }

    #[test]
    fn an_option_that_does_not_go_on_asks_again_and_a_question_guards_its_answer() {
        let b = block(vec![
            record(1, 0, vec![instr("choice", &[("options", 10)])]),
            // Option 0 neither leaves the group nor goes elsewhere: the original asks again.
            record(0, 0, vec![instr("dialogue", &[("text", 4)])]),
            record(
                0,
                0,
                vec![
                    instr("dialogue", &[("text", 3)]),
                    instr("leave_parallel", &[]),
                ],
            ),
            // A yes/no question: the officer joins only on yes.
            record(
                0,
                1,
                vec![
                    instr("dialogue", &[("text", 2)]),
                    instr("if_answer", &[("answer", 0), ("skip", 1)]),
                    instr("set_allegiance", &[("person", 9), ("army", 0)]),
                ],
            ),
            // "Ready to set out?": as if the player said yes.
            record(
                0,
                2,
                vec![
                    instr("if_answer", &[("answer", 0), ("skip", 2)]),
                    instr("op_3d", &[]),
                    instr("dialogue", &[("text", 5)]),
                ],
            ),
        ]);
        let song_key = |_: u16| None;
        let names = names();
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert!(!s.game_over);
        assert_eq!(
            s.text,
            "@label ask_1\n@choice\n- 예. -> opt_1_0\n- 아니오. -> opt_1_1\n\
             @label opt_1_0\nyuan_shao: 실례했소.\n@goto ask_1\n\
             @label opt_1_1\nyuan_shao: 처형하라!\n@goto after_2\n@label after_2\n\
             yuan_shao: 흥.\nliu_bei: 무슨 일입니까?\n\
             @label ask_3\n@choice\n- 예 -> yes_3\n- 아니오 -> after_4\n@label yes_3\n\
             @join yuan_shao\n@goto after_4\n@label after_4\n\
             손건: 돌아왔습니다.\n"
        );
        parses(&s.text);
    }

    #[test]
    fn a_goto_to_another_block_is_a_route_or_where_the_story_goes_on() {
        // A choice between another block and asking again (a goto to its own block, 2).
        let b = block(vec![
            record(1, 0, vec![instr("choice", &[("options", 10)])]),
            record(
                0,
                0,
                vec![
                    instr("dialogue", &[("text", 3)]),
                    instr("goto_block", &[("block", 5)]),
                ],
            ),
            record(0, 0, vec![instr("goto_block", &[("block", 2)])]),
        ]);
        let song_key = |_: u16| None;
        let names = names();
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert_eq!(
            s.text,
            // The route flag starts clear (a scene played again).
            "@set route = 0\n@label ask_1\n@choice\n- 예. -> opt_1_0\n- 아니오. -> opt_1_1\n\
             @label opt_1_0\nyuan_shao: 처형하라!\n@set route = 1\n@end\n\
             @label opt_1_1\n@goto ask_1\n@label after_2\n"
        );
        assert_eq!(
            s.next,
            Next::Routes {
                flag: "route".into(),
                targets: vec![(1, 5)],
                otherwise: None,
            }
        );
        parses(&s.text);
        // A goto outside a choice: the block ends there and the story goes on with that block.
        let b = block(vec![
            record(
                RUN,
                0,
                vec![
                    instr("dialogue", &[("text", 4)]),
                    instr("goto_block", &[("block", 7)]),
                ],
            ),
            record(RUN, 1, vec![instr("dialogue", &[("text", 5)])]),
        ]);
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert_eq!(s.text, "yuan_shao: 실례했소.\n");
        assert_eq!(s.next, Next::Block(7));
    }

    #[test]
    fn a_battles_outro_is_its_victory_script_and_epilogue() {
        let b = block(vec![
            // Before the battle: not in the outro.
            record(0, 0, vec![instr("dialogue", &[("text", 1)])]),
            // The battle's watched group: a trigger, the victory script.
            record(4, 3, vec![instr("dialogue", &[("text", 2)])]),
            record(BATTLE_WON, 3, vec![instr("dialogue", &[("text", 3)])]),
            // The epilogue after it: an officer comes back, and the story goes on elsewhere.
            record(
                RUN,
                4,
                vec![
                    instr("dialogue", &[("text", 4)]),
                    instr("set_allegiance", &[("person", 9), ("army", 0)]),
                    instr("goto_block", &[("block", 6)]),
                ],
            ),
        ]);
        let song_key = |_: u16| None;
        let names = names();
        let s = victory_scene(&b, &ctx(&names, &song_key));
        assert_eq!(
            s.text,
            "yuan_shao: 처형하라!\nyuan_shao: 실례했소.\n@join yuan_shao\n"
        );
        assert_eq!(s.next, Next::Block(6));
    }

    #[test]
    fn an_outro_plays_the_last_stages_victory_and_the_whole_epilogue() {
        let b = block(vec![
            // Two stages with a victory script each: the last one's is the outro's.
            record(BATTLE_WON, 3, vec![instr("dialogue", &[("text", 1)])]),
            record(9, 4, vec![instr("leave_parallel", &[])]),
            record(
                BATTLE_WON,
                4,
                vec![
                    // Versions by a flag the battle sets: the campaign checks it (`orig_f150`).
                    if_flags(1, vec![150], vec![]),
                    instr("dialogue", &[("text", 2)]),
                    if_flags(1, vec![], vec![150]),
                    instr("dialogue", &[("text", 3)]),
                    instr("data", &[("kind", 2), ("value", 700)]),
                ],
            ),
            // The epilogue: a run group, then a talk group with a choice to have an officer
            // join (not chatter: it asks, and one option joins).
            record(RUN, 5, vec![instr("dialogue", &[("text", 4)])]),
            record(
                TALK,
                6,
                vec![
                    instr("dialogue", &[("text", 5)]),
                    instr("choice", &[("options", 10)]),
                ],
            ),
            record(
                0,
                6,
                vec![
                    instr("set_allegiance", &[("person", 9), ("army", 0)]),
                    instr("leave_parallel", &[]),
                ],
            ),
            record(0, 6, vec![instr("leave_parallel", &[])]),
        ]);
        let song_key = |_: u16| None;
        let names = names();
        let s = victory_scene(&b, &ctx(&names, &song_key));
        assert_eq!(s.gold, 700);
        assert_eq!(
            s.text,
            "@if orig_f150 == 0 -> skip_1\nyuan_shao: 흥.\nliu_bei: 무슨 일입니까?\n@label skip_1\n\
             @if orig_f150 != 0 -> skip_2\nyuan_shao: 처형하라!\n@label skip_2\n\
             yuan_shao: 실례했소.\n손건: 돌아왔습니다.\n\
             @label ask_3\n@choice\n- 예. -> opt_3_0\n- 아니오. -> opt_3_1\n\
             @label opt_3_0\n@join yuan_shao\n@goto after_4\n\
             @label opt_3_1\n@goto after_4\n@label after_4\n"
        );
        assert_eq!(s.next, Next::Default);
        parses(&s.text);
        // In a story scene gold is given there.
        let b = block(vec![record(
            RUN,
            0,
            vec![instr("data", &[("kind", 2), ("value", 100)])],
        )]);
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert_eq!((s.text.as_str(), s.gold), ("@gold 100\n", 0));
    }

    #[test]
    fn the_chapters_continue_the_campaign_after_its_last_battle() {
        let base: CampaignDef = toml::from_str(
            "title = \"t\"\nstart = \"camp\"\nstarting_officers = [\"liu_bei\"]\n\
             [[node]]\ntype = \"camp\"\nid = \"camp\"\nbattle = \"b1\"\nnext = \"fight\"\n\
             [[node]]\ntype = \"battle\"\nid = \"fight\"\nbattle = \"b1\"\nnext = \"end1\"\n\
             [[node]]\ntype = \"ending\"\nid = \"end1\"\ntitle = \"1장\"\n",
        )
        .unwrap();
        let story = |at, scene: &str, game_over, next| Step {
            at,
            kind: StepKind::Story {
                scene: scene.into(),
            },
            next,
            ends: Ends {
                game_over,
                endings: BTreeSet::new(),
            },
        };
        let steps = [
            // A game over, else a route to block 3 (s3), else on (b2).
            story(
                (2, 0, 0),
                "s1",
                true,
                Next::Routes {
                    flag: "r".into(),
                    targets: vec![(1, 3)],
                    otherwise: None,
                },
            ),
            Step {
                at: (2, 0, 1),
                kind: StepKind::Battle {
                    battle: "b2".into(),
                    title: "연주 — 출진 준비".into(),
                    shop: vec!["bean".into()],
                    defeat: None,
                    before: None,
                },
                next: Next::Default,
                ends: Ends::default(),
            },
            // On with block 4: the first part at or after it (s4 of the next scene: none in
            // this one).
            story((2, 0, 2), "s2", false, Next::Block(4)),
            story((2, 0, 3), "s3", false, Next::Default),
            story((2, 1, 0), "s4", false, Next::Default),
        ];
        let c = continue_campaign(&base, "b1", &steps, ("end2", "2장")).unwrap();
        let ids: Vec<&str> = c.nodes.iter().map(Node::id).collect();
        // The old ending goes: nothing leads to it any more.
        assert_eq!(
            ids,
            [
                "camp",
                "fight",
                "s1_route1",
                "s1_check",
                "s1",
                "b2_camp",
                "b2_battle",
                "s2",
                "s3",
                "s4",
                "end2",
                GAME_OVER_NODE
            ]
        );
        let node = |id: &str| c.nodes.iter().find(|n| n.id() == id).unwrap();
        assert!(matches!(node("fight"), Node::Battle { next, .. } if next == "s1"));
        assert!(matches!(node("s1"), Node::Drama { next, .. } if next == "s1_check"));
        assert!(
            matches!(node("s1_check"), Node::Branch { then, otherwise, .. }
            if then == GAME_OVER_NODE && otherwise == "s1_route1")
        );
        assert!(
            matches!(node("s1_route1"), Node::Branch { flag, value: 1, then, otherwise, .. }
            if flag == "r" && then == "s3" && otherwise == "b2_camp")
        );
        assert!(matches!(node("b2_camp"), Node::Camp { shop, battle, .. }
            if shop == &["bean"] && battle.as_deref() == Some("b2")));
        assert!(matches!(node("b2_battle"), Node::Battle { next, .. } if next == "s2"));
        assert!(matches!(node("s2"), Node::Drama { next, .. } if next == "s4"));
        assert!(matches!(node("s4"), Node::Drama { next, .. } if next == "end2"));
        // It still reads as a campaign.
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<CampaignDef>(&text).unwrap(), c);
        // No such battle: nothing to continue.
        assert!(continue_campaign(&base, "b9", &steps, ("end2", "2장")).is_none());
        // An ending with a scene (the base chapter's close) plays it before the chapters.
        let mut with_scene = base.clone();
        if let Some(Node::Ending { scene, .. }) = with_scene.nodes.last_mut() {
            *scene = Some("close".into());
        }
        let c = continue_campaign(&with_scene, "b1", &steps, ("end2", "2장")).unwrap();
        let node = |id: &str| c.nodes.iter().find(|n| n.id() == id).unwrap();
        assert!(matches!(node("fight"), Node::Battle { next, .. } if next == "end1"));
        assert!(matches!(node("end1"), Node::Drama { scene, next, .. }
            if scene == "close" && next == "s1"));
    }

    #[test]
    fn a_lost_battle_the_story_goes_on_after_and_the_originals_endings() {
        let base: CampaignDef = toml::from_str(
            "title = \"t\"\nstart = \"camp\"\nstarting_officers = [\"liu_bei\"]\n\
             [[node]]\ntype = \"camp\"\nid = \"camp\"\nbattle = \"b1\"\nnext = \"fight\"\n\
             [[node]]\ntype = \"battle\"\nid = \"fight\"\nbattle = \"b1\"\nnext = \"end1\"\n\
             [[node]]\ntype = \"ending\"\nid = \"end1\"\ntitle = \"1장\"\n",
        )
        .unwrap();
        let steps = [
            // Lost, the story goes on with the block after the battle (s1, an ending).
            Step {
                at: (3, 4, 6),
                kind: StepKind::Battle {
                    battle: "yiling".into(),
                    title: "이릉".into(),
                    shop: Vec::new(),
                    defeat: Some(Defeat {
                        scene: "yiling_defeat".into(),
                        next: Next::Default,
                        ends: Ends::default(),
                    }),
                    before: Some("yiling_before".into()),
                },
                next: Next::Block(8),
                ends: Ends::default(),
            },
            Step {
                at: (3, 4, 7),
                kind: StepKind::Story { scene: "s1".into() },
                next: Next::Default,
                ends: Ends {
                    game_over: false,
                    endings: BTreeSet::from([3]),
                },
            },
            Step {
                at: (3, 4, 8),
                kind: StepKind::Story { scene: "s2".into() },
                next: Next::Default,
                ends: Ends {
                    game_over: false,
                    endings: BTreeSet::from([0, 1]),
                },
            },
        ];
        let c = continue_campaign(&base, "b1", &steps, ("end2", "끝")).unwrap();
        let node = |id: &str| c.nodes.iter().find(|n| n.id() == id);
        assert!(
            matches!(node("yiling_battle"), Some(Node::Battle { next, on_defeat, .. })
            if next == "s2" && on_defeat.as_deref() == Some("yiling_defeat"))
        );
        assert!(matches!(node("yiling_defeat"), Some(Node::Drama { next, .. }) if next == "s1"));
        // The setup's changes to the army play before the camp.
        assert!(
            matches!(node("yiling_before"), Some(Node::Drama { next, .. })
            if next == "yiling_camp")
        );
        assert!(matches!(node("s1"), Some(Node::Drama { next, .. }) if next == "s1_ending3"));
        assert!(
            matches!(node("s1_ending3"), Some(Node::Branch { flag, value: 4, then, .. })
            if flag == ENDING_FLAG && *then == ending_node(3))
        );
        // s2: ending 0, else 1, else the chapter's end.
        assert!(matches!(node("s2"), Some(Node::Drama { next, .. }) if next == "s2_ending0"));
        assert!(
            matches!(node("s2_ending0"), Some(Node::Branch { otherwise, .. })
            if otherwise == "s2_ending1")
        );
        assert!(
            matches!(node("s2_ending1"), Some(Node::Branch { otherwise, .. })
            if otherwise == "end2")
        );
        for n in [0, 1, 3] {
            assert!(
                matches!(node(&ending_node(n)), Some(Node::Ending { .. })),
                "{n}"
            );
        }
        assert!(node(&ending_node(2)).is_none());
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<CampaignDef>(&text).unwrap(), c);
    }

    #[test]
    fn a_jump_back_to_the_block_ends_the_record() {
        // "Join us?" Yes: the officer joins and the block runs again (the original leaves the
        // record there); no: the refusal. A flag check that jumps back ends it too.
        let b = block(vec![record(
            TALK,
            0,
            vec![
                if_flags(2, vec![7], vec![]),
                instr("dialogue", &[("text", 5)]),
                instr("goto_block", &[("block", 2)]),
                instr("dialogue", &[("text", 2)]),
                instr("if_answer", &[("answer", 0), ("skip", 2)]),
                instr("set_allegiance", &[("person", 9), ("army", 0)]),
                instr("goto_block", &[("block", 2)]),
                instr("dialogue", &[("text", 4)]),
                instr("leave_parallel", &[]),
            ],
        )]);
        let song_key = |_: u16| None;
        let names = names();
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert_eq!(
            s.text,
            "@if orig_f7 == 0 -> skip_1\n손건: 돌아왔습니다.\n@goto rend_2\n@label skip_1\n\
             yuan_shao: 흥.\nliu_bei: 무슨 일입니까?\n\
             @label ask_3\n@choice\n- 예 -> yes_3\n- 아니오 -> after_4\n@label yes_3\n\
             @join yuan_shao\n@goto rend_2\n@label after_4\n\
             yuan_shao: 실례했소.\n@label rend_2\n"
        );
        parses(&s.text);
    }

    #[test]
    fn a_battles_setup_changes_the_army_before_its_camp() {
        let b = block(vec![
            record(
                RUN,
                0,
                vec![
                    instr("set_allegiance", &[("person", 9), ("army", 0)]),
                    instr("dialogue", &[("text", 1)]),
                ],
            ),
            record(RUN, 1, vec![instr("begin_battle", &[])]),
            // A stage's change is the battle's, not the setup's.
            record(
                9,
                3,
                vec![instr("set_allegiance", &[("person", 9), ("army", 5)])],
            ),
        ]);
        let song_key = |_: u16| None;
        let names = names();
        assert_eq!(
            before_scene(&b, &ctx(&names, &song_key)).text,
            "@join yuan_shao\n"
        );
        // A setup that changes nothing has no scene.
        let b = block(vec![record(
            RUN,
            0,
            vec![instr("dialogue", &[("text", 1)])],
        )]);
        assert!(before_scene(&b, &ctx(&names, &song_key)).text.is_empty());
    }

    #[test]
    fn an_event_picture_shows_over_the_narration_after_it() {
        let b = block(vec![record(
            RUN,
            0,
            vec![
                instr("show_picture", &[("picture", 12), ("variant", 0)]),
                instr("narration", &[("text", 11)]),
                // The next other instruction closes it (a dialogue here).
                instr("dialogue", &[("text", 4)]),
                // One the pack does not have is left out (and noted).
                instr("show_picture", &[("picture", 40), ("variant", 0)]),
                instr("show_screen", &[]),
            ],
        )]);
        let song_key = |_: u16| None;
        let names = names();
        let s = story_scene(&b, &ctx(&names, &song_key));
        assert_eq!(
            s.text,
            "@picture orig_12\n@narr 유비가 죽었다.\n@picture none\nyuan_shao: 실례했소.\n"
        );
        assert!(s
            .notes
            .iter()
            .any(|n| n.contains("pictures the pack does not have")));
        parses(&s.text);
    }

    #[test]
    fn a_chapter_battle_base_wins_by_its_commander_or_everyone() {
        let b = chapter_base(
            "c2_s0_b8",
            "연주 전투",
            "조조를 물리쳐라",
            30,
            true,
            Some("liu_bei"),
        );
        assert_eq!(b.victory.len(), 2);
        assert_eq!(b.victory[0], Condition::DefeatCommander);
        assert!(
            matches!(&b.victory[1], Condition::Reach { who, .. } if who.as_deref() == Some("liu_bei"))
        );
        let b = chapter_base("x", "x", "x", 10, false, None);
        assert_eq!(b.victory, [Condition::DefeatAll]);
    }
}
