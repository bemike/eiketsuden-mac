//! The original's chapters past the base pack's campaign (docs/ORIGINAL_DATA.md, DECISIONS D18):
//! the battles and the story of a scenario file the base pack has no campaign for, as new
//! battles, drama scenes and campaign nodes that continue the base campaign where it ends.
//!
//! A scenario scene is a row of blocks. A block that loads a battle map and sets a battle up is
//! a battle ([`Part::Battle`]); the others are the story in between ([`Part::Story`]): towns and
//! the campaign map, where the original lets the player walk about and talk to people. The engine
//! has no such mode, so a story block becomes one drama scene read in order ([`story_scene`]):
//! the records that move the story on (every record of a group but the optional chatter of
//! people one may talk to), choices as `@choice` (an option whose record ends in `game_over`
//! sets [`GAME_OVER_FLAG`], which a campaign branch sends to a game-over ending), and the story's
//! side effects as drama commands (officers joining and leaving, items, music). The battles are
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
    pub notes: Vec<String>,
}

/// What a story scene needs besides the block.
pub struct StoryContext<'a> {
    pub names: &'a Names,
    pub text: &'a dyn TextSource,
    /// Music key of an original song number (`MUSIC.R3`), if the pack has one for it.
    pub song_key: &'a dyn Fn(u16) -> Option<&'static str>,
}

/// Whether a record moves the story on: every record but the chatter of a person one may talk
/// to (a `talk` record that does not leave the group's parallel control).
fn moves_on(r: &Record) -> bool {
    r.trigger.kind != TALK || r.code.iter().any(|c| c.mnemonic == "leave_parallel")
}

/// The drama scene of a story block (see the module docs).
pub fn story_scene(block: &Block, ctx: &StoryContext) -> StoryScene {
    let mut out = StoryScene::default();
    let mut skipped: BTreeSet<&'static str> = BTreeSet::new();
    let mut taken = BTreeSet::new();
    let mut labels = 0usize;
    for (i, rec) in block.records.iter().enumerate() {
        if taken.contains(&i) || !moves_on(rec) {
            continue;
        }
        // A choice ends its record's script: option `k` goes on with record `i + 1 + k`.
        if let Some(at) = rec.code.iter().position(|c| c.mnemonic == "choice") {
            story_lines(&rec.code[..at], ctx, &mut out, &mut skipped);
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
                    out.notes.push(format!("record {i}: choice left out: {e}"));
                    continue;
                }
            };
            labels += 1;
            let after = format!("after_{labels}");
            out.text.push_str("@choice\n");
            for (k, option) in options.iter().enumerate() {
                let _ = writeln!(
                    out.text,
                    "- {} -> opt_{labels}_{k}",
                    option.replace("->", "→")
                );
            }
            for k in 0..options.len() {
                let _ = writeln!(out.text, "@label opt_{labels}_{k}");
                match block.records.get(i + 1 + k) {
                    Some(target) => {
                        taken.insert(i + 1 + k);
                        story_lines(&target.code, ctx, &mut out, &mut skipped);
                        if target.code.iter().any(|c| c.mnemonic == "game_over") {
                            out.game_over = true;
                            let _ = writeln!(out.text, "@set {GAME_OVER_FLAG} = 1\n@end");
                            continue;
                        }
                    }
                    None => out.notes.push(format!(
                        "record {i}: option {k} has no record to go on with"
                    )),
                }
                let _ = writeln!(out.text, "@goto {after}");
            }
            let _ = writeln!(out.text, "@label {after}");
            continue;
        }
        if rec.code.iter().any(|c| c.mnemonic == "game_over") {
            // A game over that no choice leads to (a failed errand): not part of the story.
            continue;
        }
        story_lines(&rec.code, ctx, &mut out, &mut skipped);
    }
    if !skipped.is_empty() {
        out.notes.push(format!(
            "left out: {}",
            skipped.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    out
}

/// The drama lines of `code`.
fn story_lines(
    code: &[Instr],
    ctx: &StoryContext,
    out: &mut StoryScene,
    skipped: &mut BTreeSet<&'static str>,
) {
    let names = ctx.names;
    let officer = |person: u16| names.officers.get(&person).cloned();
    for instr in code {
        let get = |name: &str| instr.operands.get(name).unwrap_or(0);
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
                    skipped.insert("songs the pack has no music key for");
                }
            },
            "set_allegiance" => match officer(get("person")) {
                // Army 0 is Liu Bei's.
                Some(id) if get("army") == 0 => {
                    let _ = writeln!(out.text, "@join {id}");
                }
                Some(id) => {
                    let _ = writeln!(out.text, "@leave {id}");
                }
                None => {
                    skipped.insert("allegiances of persons without a pack officer");
                }
            },
            "add_item" => match names.items.get(&(get("item") as u8)) {
                Some(id) => {
                    let _ = writeln!(out.text, "@item {id}");
                }
                None => {
                    skipped.insert("items without a pack item");
                }
            },
            "set_shop_items" => {
                if let Operands::Bytes { bytes } = &instr.operands {
                    out.shop = bytes
                        .iter()
                        .filter_map(|b| names.items.get(b).cloned())
                        .collect();
                }
            }
            "show_picture" => {
                skipped.insert("pictures");
            }
            "add_levels" | "set_class" => {
                skipped.insert("level and class changes");
            }
            _ => {}
        }
    }
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

/// One step of a converted chapter, in the order the campaign plays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Drama scene `scene`; `game_over`: it may set [`GAME_OVER_FLAG`].
    Story { scene: String, game_over: bool },
    /// Battle `battle`, prepared in a camp titled `title` whose shop sells `shop`.
    Battle {
        battle: String,
        title: String,
        shop: Vec<String>,
    },
}

/// `base` with the steps played after its battle node that fights `after_battle`, instead of
/// the node it went on to, and ending with `ending` (a node id and a title); the game over of
/// the story's choices is added too. `None` when the base campaign has no such battle node.
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
    // Node ids of each step: a story is one node (or two with its game-over branch), a battle
    // a camp and a battle.
    let mut nodes: Vec<Node> = Vec::new();
    let ids: Vec<String> = steps
        .iter()
        .map(|step| match step {
            Step::Story { scene, .. } => scene.clone(),
            Step::Battle { battle, .. } => format!("{battle}_camp"),
        })
        .collect();
    let first = ids.first().cloned().unwrap_or_else(|| ending.0.to_string());
    let mut game_over = false;
    for (i, step) in steps.iter().enumerate() {
        let next = ids
            .get(i + 1)
            .cloned()
            .unwrap_or_else(|| ending.0.to_string());
        match step {
            Step::Story {
                scene,
                game_over: g,
            } if *g => {
                game_over = true;
                let branch = format!("{scene}_check");
                nodes.push(Node::Drama {
                    id: scene.clone(),
                    scene: scene.clone(),
                    next: branch.clone(),
                });
                nodes.push(Node::Branch {
                    id: branch,
                    flag: GAME_OVER_FLAG.to_string(),
                    cmp: Compare::Eq,
                    value: 1,
                    then: GAME_OVER_NODE.to_string(),
                    otherwise: next,
                });
            }
            Step::Story { scene, .. } => nodes.push(Node::Drama {
                id: scene.clone(),
                scene: scene.clone(),
                next,
            }),
            Step::Battle {
                battle,
                title,
                shop,
            } => {
                let fight = format!("{battle}_battle");
                nodes.push(Node::Camp {
                    id: format!("{battle}_camp"),
                    title: title.clone(),
                    shop: shop.clone(),
                    battle: Some(battle.clone()),
                    next: fight.clone(),
                });
                nodes.push(Node::Battle {
                    id: fight,
                    battle: battle.clone(),
                    next,
                    on_defeat: None,
                });
            }
        }
    }
    nodes.push(Node::Ending {
        id: ending.0.to_string(),
        scene: None,
        title: ending.1.to_string(),
    });
    if game_over {
        nodes.push(Node::Ending {
            id: GAME_OVER_NODE.to_string(),
            scene: None,
            title: "게임 오버".to_string(),
        });
    }
    if let Node::Battle { next, .. } = &mut campaign.nodes[at] {
        *next = first;
    }
    // The node the base campaign went on to is replaced when nothing else leads to it.
    let still_used = campaign
        .nodes
        .iter()
        .any(|n| successors(n).contains(&old_next.as_str()));
    if !still_used {
        campaign.nodes.retain(|n| n.id() != old_next);
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
            record(0, 1, vec![instr("dialogue", &[("text", 4)])]),
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
        let s = story_scene(
            &b,
            &StoryContext {
                names: &names(),
                text: &Text,
                song_key: &song_key,
            },
        );
        assert!(s.game_over);
        assert_eq!(
            s.text,
            "yuan_shao: 흥.\nliu_bei: 무슨 일입니까?\n@join yuan_shao\n@item bean\n@bgm peace\n\
             @choice\n- 예. -> opt_1_0\n- 아니오. -> opt_1_1\n\
             @label opt_1_0\nyuan_shao: 처형하라!\n@narr 유비가 죽었다.\n@set orig_game_over = 1\n@end\n\
             @label opt_1_1\nyuan_shao: 실례했소.\n@goto after_1\n@label after_1\n\
             손건: 돌아왔습니다.\n@leave yuan_shao\n"
        );
        // It is a scene the drama parser takes.
        let scenes =
            hero_core::script::parse_drama("t.drama", &format!("== s\n{}", s.text)).unwrap();
        assert_eq!(scenes.len(), 1);
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
        let steps = [
            Step::Story {
                scene: "s1".into(),
                game_over: true,
            },
            Step::Battle {
                battle: "b2".into(),
                title: "연주 — 출진 준비".into(),
                shop: vec!["bean".into()],
            },
            Step::Story {
                scene: "s2".into(),
                game_over: false,
            },
        ];
        let c = continue_campaign(&base, "b1", &steps, ("end2", "2장")).unwrap();
        let ids: Vec<&str> = c.nodes.iter().map(Node::id).collect();
        // The old ending goes: nothing leads to it any more.
        assert_eq!(
            ids,
            [
                "camp",
                "fight",
                "s1",
                "s1_check",
                "b2_camp",
                "b2_battle",
                "s2",
                "end2",
                GAME_OVER_NODE
            ]
        );
        assert!(matches!(&c.nodes[1], Node::Battle { next, .. } if next == "s1"));
        assert!(matches!(&c.nodes[3], Node::Branch { then, otherwise, .. }
            if then == GAME_OVER_NODE && otherwise == "b2_camp"));
        assert!(matches!(&c.nodes[4], Node::Camp { shop, battle, .. }
            if shop == &["bean"] && battle.as_deref() == Some("b2")));
        assert!(matches!(&c.nodes[6], Node::Drama { next, .. } if next == "end2"));
        // It still reads as a campaign.
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<CampaignDef>(&text).unwrap(), c);
        // No such battle: nothing to continue.
        assert!(continue_campaign(&base, "b9", &steps, ("end2", "2장")).is_none());
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
