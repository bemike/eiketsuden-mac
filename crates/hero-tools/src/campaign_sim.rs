//! `hero-tools simulate --campaign`: plays the whole campaign from a new game, AI against AI,
//! the way the game's flow does (`hero-game` `flow.rs`), so each battle is fought with the army
//! the earlier ones left: levels, classes, recruits, items and flags carry over.
//!
//! * **Drama** nodes, the scenes battles play (intro, events, outro) and an ending's scene run
//!   headless ([`DramaRunner`]) with their side effects: flags, gold, items, officers joining
//!   or leaving. A choice takes the option `--choose SCENE=N,N,...` names for that scene's
//!   choices in order (1 = the first), else the first option not taken yet at that question
//!   in this play of the scene (so a question that loops back until answered right is left).
//! * **Camp** nodes buy and equip nothing and deploy what the camp screen selects when the
//!   player changes nothing ([`camp_deployment`]): the first camp the whole army, later camps
//!   that same selection fitted to their battle (an officer who joined since is not added).
//! * A scene or battle that cannot run fails the run, where the game would show an error and
//!   go on: the tool is a check.
//! * **Battle** nodes are fought by the AI on both sides (at most [`MAX_PHASES`] phases, a seed
//!   per battle made from the run's seed and the battle's number); the result is applied as
//!   the game applies it, then a victory goes to `next`, a defeat to
//!   `on_defeat` or ends the run (game over).
//! * The run ends at an **Ending** node.

use crate::simulate::MAX_PHASES;
use crate::Failure;
use hero_core::battle::{normalize_deployment, BattleEvent, BattleState, Outcome};
use hero_core::battledef::{BattleDef, Side};
use hero_core::campaign::{CampaignState, Node};
use hero_core::data::Id;
use hero_core::drama::{DramaRunner, Step};
use hero_core::pack::{Pack, Severity};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

/// Campaign nodes visited in one run before it counts as looping.
pub const MAX_NODES: usize = 1000;
/// Choices one play of a scene may ask before it counts as looping.
pub const MAX_CHOICES_PER_SCENE: usize = 100;

/// `--choose` options: scene id -> the option (0-based) to take at each choice the scene asks
/// in the run, in order (a scene played again asks again); past them the default rule of
/// [`Sim::play_scene`] applies.
pub type Choices = BTreeMap<String, Vec<usize>>;

/// One battle fought in a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Fought {
    pub battle: String,
    pub won: bool,
    pub turns: u32,
    /// Average level of the player's officers when the battle began.
    pub level: f64,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Reached the ending node (id).
    Ending(String),
    /// Lost a battle without `on_defeat`.
    GameOver(String),
    /// A battle did not finish within [`MAX_PHASES`] phases.
    Stuck(String),
    /// More than [`MAX_NODES`] nodes.
    Looping,
    /// The campaign could not go on (a missing node or scene, a bad choice).
    Error(String),
    Panicked(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub fought: Vec<Fought>,
    /// The choices taken: `scene: option text`.
    pub chose: Vec<String>,
    /// How many choices each scene asked.
    pub asked: BTreeMap<String, usize>,
    pub end: End,
}

/// `Ok(true)` when no run failed (stuck, looping, an error or a panic).
pub fn run(dir: &Path, seeds: u32, choices: &Choices) -> Result<bool, Failure> {
    let pack = crate::load_pack(dir).map_err(Failure::Failed)?;
    let errors = pack
        .validate()
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    if errors > 0 {
        return Err(Failure::Failed(format!(
            "the pack has {errors} validation error(s); run `hero-tools validate` first"
        )));
    }
    for scene in choices.keys() {
        if pack.scene(scene).is_none() {
            return Err(Failure::Usage(format!(
                "--choose names unknown scene `{scene}`"
            )));
        }
    }
    println!("Simulating the campaign x {seeds} seed(s), at most {MAX_PHASES} phases per battle\n");
    let _quiet = crate::simulate::QuietPanics::install();
    let runs: Vec<Run> = (1..=seeds)
        .map(|seed| run_seed(&pack, seed, choices))
        .collect();
    let (report, failed) = render(&pack, &runs, choices);
    print!("{report}");
    Ok(!failed)
}

/// One run from a new game. Battle `n` of the run is fought with a seed made of `seed` and `n`.
pub fn run_seed(pack: &Pack, seed: u32, choices: &Choices) -> Run {
    let mut sim = Sim {
        pack,
        seed,
        choices,
        fought: Vec::new(),
        chose: Vec::new(),
        asked: BTreeMap::new(),
    };
    let end = panic::catch_unwind(AssertUnwindSafe(|| sim.play()))
        .unwrap_or_else(|_| End::Panicked(crate::simulate::take_panic_message()));
    Run {
        fought: sim.fought,
        chose: sim.chose,
        asked: sim.asked,
        end,
    }
}

struct Sim<'a> {
    pack: &'a Pack,
    seed: u32,
    choices: &'a Choices,
    fought: Vec<Fought>,
    chose: Vec<String>,
    asked: BTreeMap<String, usize>,
}

impl Sim<'_> {
    fn play(&mut self) -> End {
        let pack = self.pack;
        let mut campaign = CampaignState::new_game(pack);
        for _ in 0..MAX_NODES {
            let Some(node) = pack.campaign.node(&campaign.node).cloned() else {
                return End::Error(format!("unknown node `{}`", campaign.node));
            };
            let next = match node {
                Node::Drama { scene, .. } => {
                    if let Err(e) = self.play_scene(&mut campaign, &scene) {
                        return End::Error(e);
                    }
                    campaign.advance(pack)
                }
                Node::Camp { battle, .. } => {
                    if let Some(def) = battle.as_deref().and_then(|b| pack.battles.get(b)) {
                        campaign.deployed = camp_deployment(pack, def, &campaign);
                    }
                    campaign.advance(pack)
                }
                Node::Battle {
                    battle, on_defeat, ..
                } => {
                    let (state, level) = match self.fight(&mut campaign, &battle) {
                        Ok(fought) => fought,
                        Err(end) => return end,
                    };
                    let won = state.outcome == Some(Outcome::Victory);
                    self.fought.push(Fought {
                        battle: battle.clone(),
                        won,
                        turns: state.turn,
                        level,
                    });
                    campaign.apply_battle_result(pack, &state);
                    match (won, on_defeat) {
                        (true, _) => campaign.advance(pack),
                        (false, Some(node)) => campaign.jump(pack, &node),
                        (false, None) => return End::GameOver(battle),
                    }
                }
                Node::Ending { id, scene, .. } => {
                    if let Some(scene) = scene {
                        if let Err(e) = self.play_scene(&mut campaign, &scene) {
                            return End::Error(e);
                        }
                    }
                    return End::Ending(id);
                }
                Node::Branch { .. } => campaign.advance(pack),
            };
            if let Err(e) = next {
                return End::Error(e.to_string());
            }
        }
        End::Looping
    }

    /// Fight `battle` with the campaign's army; the scenes it plays run on the campaign.
    /// Returns the finished battle and the army's average level at its start.
    fn fight(
        &mut self,
        campaign: &mut CampaignState,
        battle: &str,
    ) -> Result<(BattleState, f64), End> {
        let pack = self.pack;
        let seed = (u64::from(self.seed) << 16) | self.fought.len() as u64;
        let mut state = BattleState::new(pack, battle, campaign, seed)
            .map_err(|e| End::Error(format!("battle `{battle}`: {e}")))?;
        let level = average_level(&state);
        let mut events = state.begin(pack);
        let mut phases = 0;
        loop {
            for e in events {
                if let BattleEvent::Drama { scene } = e {
                    self.play_scene(campaign, &scene).map_err(End::Error)?;
                }
            }
            if state.outcome.is_some() {
                return Ok((state, level));
            }
            if phases == MAX_PHASES {
                return Err(End::Stuck(battle.to_string()));
            }
            events = state.run_ai_phase(pack);
            phases += 1;
        }
    }

    /// Play `scene` to its end. At each choice it takes the next `--choose` option of the
    /// scene; past those (or without any), the first option not taken yet at that question
    /// while the scene plays: a question that leads back to itself until the right answer is
    /// given is answered the way a player would, one option after another.
    fn play_scene(&mut self, campaign: &mut CampaignState, scene: &str) -> Result<(), String> {
        let pack = self.pack;
        let at = |e: hero_core::drama::DramaError| format!("scene `{scene}`: {e}");
        let mut runner = DramaRunner::new(pack, scene).map_err(at)?;
        // Options taken at each question (by its position in the scene) during this play.
        let mut taken: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        let mut asked_here = 0;
        loop {
            match runner.next(pack, campaign).map_err(at)? {
                Step::End => return Ok(()),
                Step::Choice(options) => {
                    asked_here += 1;
                    if asked_here > MAX_CHOICES_PER_SCENE {
                        return Err(format!(
                            "scene `{scene}` asked more than {MAX_CHOICES_PER_SCENE} choices: \
                             --choose it a way out"
                        ));
                    }
                    let asked = self.asked.entry(scene.to_string()).or_default();
                    let tried = taken.entry(runner.pc).or_default();
                    let pick = match self.choices.get(scene).and_then(|list| list.get(*asked)) {
                        Some(&pick) => pick,
                        None => (0..options.len()).find(|i| !tried.contains(i)).unwrap_or(0),
                    };
                    *asked += 1;
                    tried.insert(pick);
                    let Some(text) = options.get(pick) else {
                        return Err(format!(
                            "--choose {scene}: option {} at choice {} of the scene, which offers {} option(s)",
                            pick + 1,
                            *asked,
                            options.len()
                        ));
                    };
                    self.chose.push(format!("{scene}: {text}"));
                    runner.choose(pack, pick).map_err(at)?;
                }
                _ => {}
            }
        }
    }
}

/// What the camp screen deploys when the player changes nothing (`initial_selection` in
/// hero-game): the deployment chosen before, fitted to `def`, or the whole army when there is
/// none. The camp stores it, so it carries on to the next camp.
fn camp_deployment(pack: &Pack, def: &BattleDef, campaign: &CampaignState) -> Vec<Id> {
    let chosen: Vec<Id> = if campaign.deployed.is_empty() {
        campaign.roster.iter().map(|o| o.id.clone()).collect()
    } else {
        campaign.deployed.clone()
    };
    normalize_deployment(pack, def, campaign, &chosen)
}

fn average_level(state: &BattleState) -> f64 {
    let levels: Vec<u32> = state
        .units
        .iter()
        .filter(|u| u.side == Side::Player && u.officer.is_some())
        .map(|u| u.level)
        .collect();
    if levels.is_empty() {
        0.0
    } else {
        f64::from(levels.iter().sum::<u32>()) / levels.len() as f64
    }
}

/// The report, and whether any run failed.
pub fn render(pack: &Pack, runs: &[Run], choices: &Choices) -> (String, bool) {
    let mut out = String::new();
    let mut failed = false;
    for scene in choices.keys() {
        if !runs.iter().any(|r| r.asked.contains_key(scene)) {
            let _ = writeln!(
                out,
                "WARNING: --choose {scene}: no run reached a choice of that scene"
            );
        }
    }
    for (i, run) in runs.iter().enumerate() {
        let won = run.fought.iter().filter(|f| f.won).count();
        let end = match &run.end {
            End::Ending(id) => format!("reached the ending `{id}`"),
            End::GameOver(b) => format!("game over at `{b}`"),
            End::Stuck(b) => {
                failed = true;
                format!("FAILED: `{b}` did not finish within {MAX_PHASES} phases")
            }
            End::Looping => {
                failed = true;
                format!("FAILED: more than {MAX_NODES} campaign nodes")
            }
            End::Error(e) => {
                failed = true;
                format!("FAILED: {e}")
            }
            End::Panicked(e) => {
                failed = true;
                format!("FAILED: panicked: {e}")
            }
        };
        let _ = writeln!(
            out,
            "seed {}: won {won}/{} battle(s), {end}",
            i + 1,
            run.fought.len()
        );
        if !run.chose.is_empty() {
            let _ = writeln!(out, "    choices: {}", run.chose.join(" | "));
        }
    }

    // Per battle, in the order first fought.
    let mut order: Vec<&str> = Vec::new();
    let mut stats: BTreeMap<&str, (u32, u32, f64, f64)> = BTreeMap::new();
    for f in runs.iter().flat_map(|r| &r.fought) {
        if !stats.contains_key(f.battle.as_str()) {
            order.push(&f.battle);
        }
        let s = stats.entry(&f.battle).or_default();
        s.0 += 1;
        s.1 += u32::from(f.won);
        s.2 += f64::from(f.turns);
        s.3 += f.level;
    }
    if !order.is_empty() {
        let _ = writeln!(
            out,
            "\n{:<24} {:>7} {:>6} {:>9} {:>9}",
            "battle", "fought", "won", "turns", "level"
        );
    }
    for id in order {
        let (n, won, turns, level) = stats[id];
        let name = pack.battles.get(id).map_or("", |b| b.name.as_str());
        let _ = writeln!(
            out,
            "{:<24} {n:>7} {:>5.0}% {:>9.1} {:>9.1}  {name}",
            id,
            f64::from(won) * 100.0 / f64::from(n),
            turns / f64::from(n),
            level / f64::from(n),
        );
    }
    let endings = runs
        .iter()
        .filter(|r| matches!(r.end, End::Ending(_)))
        .count();
    let _ = writeln!(
        out,
        "\n{} run(s): {endings} reached an ending{}: {}",
        runs.len(),
        if endings == 0 && !runs.is_empty() {
            " (WARNING: none)"
        } else {
            ""
        },
        if failed { "FAILED" } else { "OK" }
    );
    (out, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hero_core::battledef::Condition;
    use hero_core::script::{ChoiceOption, Cmd};

    fn fixture() -> Pack {
        crate::tests::fixture_pack()
    }

    fn choose(scene: &str, options: &[usize]) -> Choices {
        Choices::from([(scene.to_string(), options.to_vec())])
    }

    #[test]
    fn a_run_walks_the_campaign_and_carries_the_army() {
        let mut pack = fixture();
        // Levels come quickly, so the first battle surely raises some.
        pack.rules.exp_per_level = 10;
        let run = run_seed(&pack, 1, &Choices::new());
        // The fixture campaign: oath, camp1, b01, camp2, b02, the ending (with its scene).
        assert_eq!(run.end, End::Ending("finale".into()), "{run:?}");
        let battles: Vec<&str> = run.fought.iter().map(|f| f.battle.as_str()).collect();
        assert_eq!(battles, ["b01", "b02"]);
        // The second battle starts with the army the first one left: the same officers in a
        // new game (deployed as the camps deploy them) would be at their starting levels.
        let mut fresh = CampaignState::new_game(&pack);
        fresh.join(&pack, "jian_yong").unwrap(); // `oath` recruits him
        fresh.deployed = camp_deployment(&pack, &pack.battles["b01"], &fresh);
        fresh.deployed = camp_deployment(&pack, &pack.battles["b02"], &fresh);
        let fresh = BattleState::new(&pack, "b02", &fresh, 0).unwrap();
        assert!(
            run.fought[1].level > average_level(&fresh),
            "{} vs {}",
            run.fought[1].level,
            average_level(&fresh)
        );
        // Runs are deterministic for a seed.
        assert_eq!(run, run_seed(&pack, 1, &Choices::new()));
    }

    #[test]
    fn a_defeat_follows_on_defeat_or_ends_the_run() {
        let mut pack = fixture();
        // b01 cannot be won: it would take surviving longer than its turn limit.
        let b01 = pack.battles.get_mut("b01").unwrap();
        b01.victory = vec![Condition::SurviveTurns { turns: 99 }];
        b01.turn_limit = 1;
        let run = run_seed(&pack, 1, &Choices::new());
        assert!(!run.fought[0].won, "{run:?}");
        // `on_defeat = "retreat"` goes on to camp2 and b02.
        assert_eq!(run.fought.len(), 2, "{run:?}");
        assert_eq!(run.end, End::Ending("finale".into()));
        // Without `on_defeat` the run ends there.
        for node in &mut pack.campaign.nodes {
            if let Node::Battle { on_defeat, .. } = node {
                *on_defeat = None;
            }
        }
        let run = run_seed(&pack, 1, &Choices::new());
        assert_eq!(run.end, End::GameOver("b01".into()));
        assert_eq!(run.fought.len(), 1);
    }

    #[test]
    fn the_camp_keeps_the_deployment_like_the_game() {
        let pack = fixture();
        let def = &pack.battles["b02"];
        let mut campaign = CampaignState::new_game(&pack);
        campaign.join(&pack, "jian_yong").unwrap();
        let all: Vec<Id> = campaign.roster.iter().map(|o| o.id.clone()).collect();
        // Nothing chosen yet: the whole army, fitted to the battle.
        assert_eq!(
            camp_deployment(&pack, def, &campaign),
            normalize_deployment(&pack, def, &campaign, &all)
        );
        // Chosen before: that choice, fitted again (an officer who joined since is not added).
        campaign.deployed = vec!["liu_bei".into()];
        let kept = camp_deployment(&pack, def, &campaign);
        assert_eq!(
            kept,
            normalize_deployment(&pack, def, &campaign, &campaign.deployed)
        );
        assert!(kept.contains(&"liu_bei".to_string()));
        assert_ne!(kept, normalize_deployment(&pack, def, &campaign, &all));
    }

    #[test]
    fn choices_pick_their_option_per_visit() {
        let pack = fixture();
        // The prologue's scene `oath` asks whether to pursue.
        let first = run_seed(&pack, 1, &Choices::new());
        assert_eq!(first.chose[0], "oath: 적을 끝까지 쫓는다");
        assert_eq!(first.asked["oath"], 1);
        let second = run_seed(&pack, 1, &choose("oath", &[1]));
        assert_eq!(second.chose[0], "oath: 마을을 지킨다");
        // An option the choice does not have ends the run with an error that says so.
        let bad = run_seed(&pack, 1, &choose("oath", &[8]));
        assert_eq!(
            bad.end,
            End::Error(
                "--choose oath: option 9 at choice 1 of the scene, which offers 2 option(s)".into()
            )
        );
        assert!(bad.fought.is_empty());

        // A scene that asks again takes the options in order, then the default: two more
        // choices right after the label every path of `oath` passes.
        let mut pack = fixture();
        let oath = pack.scenes.get_mut("oath").unwrap();
        let at = oath.labels["recruit"] + 1;
        for i in oath.labels.values_mut() {
            if *i >= at {
                *i += 2;
            }
        }
        oath.labels.insert("second".into(), at + 1);
        oath.labels.insert("after_again".into(), at + 2);
        let ask = |to: &str| {
            Cmd::Choice(vec![
                ChoiceOption {
                    text: "하나".into(),
                    label: to.into(),
                },
                ChoiceOption {
                    text: "둘".into(),
                    label: to.into(),
                },
            ])
        };
        oath.cmds.insert(at, ask("after_again"));
        oath.cmds.insert(at, ask("second"));
        let run = run_seed(&pack, 1, &choose("oath", &[0, 1]));
        assert_eq!(
            run.chose[..3],
            ["oath: 적을 끝까지 쫓는다", "oath: 둘", "oath: 하나"],
            "{run:?}"
        );
        assert_eq!(run.asked["oath"], 3);
    }

    #[test]
    fn a_question_that_leads_back_to_itself_is_left() {
        // After `@label recruit` of `oath`: a question whose first answer asks it again.
        let with_loop = |exit: bool| {
            let mut pack = fixture();
            let oath = pack.scenes.get_mut("oath").unwrap();
            let at = oath.labels["recruit"] + 1;
            for i in oath.labels.values_mut() {
                if *i >= at {
                    *i += 1;
                }
            }
            oath.labels.insert("again".into(), at);
            oath.labels.insert("out".into(), at + 1);
            let out = if exit { "out" } else { "again" };
            oath.cmds.insert(
                at,
                Cmd::Choice(vec![
                    ChoiceOption {
                        text: "다시".into(),
                        label: "again".into(),
                    },
                    ChoiceOption {
                        text: "나간다".into(),
                        label: out.into(),
                    },
                ]),
            );
            pack
        };
        let run = run_seed(&with_loop(true), 1, &Choices::new());
        assert_eq!(run.chose[1..3], ["oath: 다시", "oath: 나간다"], "{run:?}");
        assert_eq!(run.end, End::Ending("finale".into()));
        // A question with no way out is reported, not played forever.
        let run = run_seed(&with_loop(false), 1, &Choices::new());
        assert!(
            matches!(&run.end, End::Error(e) if e.contains("asked more than 100 choices")),
            "{:?}",
            run.end
        );
    }

    #[test]
    fn the_base_packs_questions_are_answered() {
        // `c1_jade_belt` asks who the heroes are until 5 or 6 is chosen, then what to do next
        // until 2 is chosen.
        let pack = crate::load_pack(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/base"))
            .unwrap();
        let choices = Choices::new();
        let mut sim = Sim {
            pack: &pack,
            seed: 1,
            choices: &choices,
            fought: Vec::new(),
            chose: Vec::new(),
            asked: BTreeMap::new(),
        };
        let mut campaign = CampaignState::new_game(&pack);
        sim.play_scene(&mut campaign, "c1_jade_belt").unwrap();
        assert!(
            sim.chose
                .iter()
                .any(|c| c.ends_with("소인의 눈으로는 알 수 없습니다")),
            "{:?}",
            sim.chose
        );
        assert!(
            sim.chose.last().unwrap().ends_with("원술을 막겠다고 한다"),
            "{:?}",
            sim.chose
        );
    }

    #[test]
    fn the_report_counts_battles_and_failures() {
        let pack = fixture();
        let battle = pack.battles.keys().next().unwrap().clone();
        let fought = |won| Fought {
            battle: battle.clone(),
            won,
            turns: 4,
            level: 5.0,
        };
        let runs = [
            Run {
                fought: vec![fought(true)],
                chose: vec![],
                asked: BTreeMap::from([("oath".to_string(), 1)]),
                end: End::Ending("finale".into()),
            },
            Run {
                fought: vec![fought(false)],
                chose: vec!["s: a".into()],
                asked: BTreeMap::new(),
                end: End::GameOver(battle.clone()),
            },
        ];
        let (report, failed) = render(&pack, &runs, &choose("oath", &[0]));
        assert!(!failed, "{report}");
        assert!(report.contains("50%"), "{report}");
        assert!(report.contains("1 reached an ending"), "{report}");
        assert!(!report.contains("WARNING: --choose"), "{report}");
        // A --choose whose scene never asked is reported.
        let (report, _) = render(&pack, &runs, &choose("mercy", &[0]));
        assert!(report.contains("WARNING: --choose mercy"), "{report}");
        let stuck = [Run {
            fought: vec![],
            chose: vec![],
            asked: BTreeMap::new(),
            end: End::Stuck(battle.clone()),
        }];
        let (report, failed) = render(&pack, &stuck, &Choices::new());
        assert!(failed && report.contains("WARNING: none"), "{report}");
    }
}
