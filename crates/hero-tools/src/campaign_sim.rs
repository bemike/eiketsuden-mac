//! `hero-tools simulate --campaign`: plays the whole campaign from a new game, AI against AI,
//! the way the game's flow does (`hero-game` `flow.rs`), so each battle is fought with the army
//! the earlier ones left: levels, classes, recruits, items and flags carry over.
//!
//! * **Drama** nodes and the scenes battles play (intro, events, outro) run headless
//!   ([`DramaRunner`]) with their side effects: flags, gold, items, officers joining or
//!   leaving. A choice takes the option `--choose SCENE=N` names (1 = the first), else the
//!   first.
//! * **Camp** nodes buy and equip nothing; the deployment is the game's default (the whole
//!   army, normalised to the battle: required officers, the lord, then the roster in order,
//!   up to `deploy.max`).
//! * **Battle** nodes are fought by the AI on both sides (at most [`MAX_PHASES`] phases); the
//!   result is applied as the game applies it, then a victory goes to `next`, a defeat to
//!   `on_defeat` or ends the run (game over).
//! * The run ends at an **Ending** node.

use crate::simulate::MAX_PHASES;
use crate::Failure;
use hero_core::battle::{BattleEvent, BattleState, Outcome};
use hero_core::battledef::Side;
use hero_core::campaign::{CampaignState, Node};
use hero_core::drama::{DramaRunner, Step};
use hero_core::pack::{Pack, Severity};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

/// Campaign nodes visited in one run before it counts as looping.
pub const MAX_NODES: usize = 1000;

/// `--choose` options: scene id -> option index (0-based).
pub type Choices = BTreeMap<String, usize>;

/// One battle fought in a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Fought {
    pub battle: String,
    pub won: bool,
    pub turns: u32,
    /// Average level of the player's units at the start.
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
    let (report, failed) = render(&pack, &runs);
    print!("{report}");
    Ok(!failed)
}

/// One run from a new game with `seed` for every battle.
pub fn run_seed(pack: &Pack, seed: u32, choices: &Choices) -> Run {
    let mut fought = Vec::new();
    let mut chose = Vec::new();
    let end = panic::catch_unwind(AssertUnwindSafe(|| {
        play(pack, seed, choices, &mut fought, &mut chose)
    }))
    .unwrap_or_else(|_| End::Panicked(crate::simulate::take_panic_message()));
    Run { fought, chose, end }
}

fn play(
    pack: &Pack,
    seed: u32,
    choices: &Choices,
    fought: &mut Vec<Fought>,
    chose: &mut Vec<String>,
) -> End {
    let mut campaign = CampaignState::new_game(pack);
    for _ in 0..MAX_NODES {
        let Some(node) = pack.campaign.node(&campaign.node).cloned() else {
            return End::Error(format!("unknown node `{}`", campaign.node));
        };
        let next = match node {
            Node::Drama { scene, .. } => {
                if let Err(e) = play_scene(pack, &mut campaign, &scene, choices, chose) {
                    return End::Error(e);
                }
                campaign.advance(pack)
            }
            Node::Camp { .. } => campaign.advance(pack),
            Node::Battle {
                battle, on_defeat, ..
            } => {
                let state = match fight(pack, &mut campaign, &battle, seed, choices, chose) {
                    Ok(state) => state,
                    Err(end) => return end,
                };
                let won = state.outcome == Some(Outcome::Victory);
                let level = average_level(&state);
                fought.push(Fought {
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
            Node::Ending { id, .. } => return End::Ending(id),
            Node::Branch { .. } => campaign.advance(pack),
        };
        if let Err(e) = next {
            return End::Error(e.to_string());
        }
    }
    End::Looping
}

/// Fight `battle` with the campaign's army; the scenes it plays run on the campaign.
fn fight(
    pack: &Pack,
    campaign: &mut CampaignState,
    battle: &str,
    seed: u32,
    choices: &Choices,
    chose: &mut Vec<String>,
) -> Result<BattleState, End> {
    let mut state = BattleState::new(pack, battle, campaign, u64::from(seed))
        .map_err(|e| End::Error(format!("battle `{battle}`: {e}")))?;
    let mut events = state.begin(pack);
    let mut phases = 0;
    loop {
        for e in events {
            if let BattleEvent::Drama { scene } = e {
                play_scene(pack, campaign, &scene, choices, chose).map_err(End::Error)?;
            }
        }
        if state.outcome.is_some() {
            return Ok(state);
        }
        if phases == MAX_PHASES {
            return Err(End::Stuck(battle.to_string()));
        }
        events = state.run_ai_phase(pack);
        phases += 1;
    }
}

/// Play `scene` to its end, taking the `--choose` option (else the first) at each choice.
fn play_scene(
    pack: &Pack,
    campaign: &mut CampaignState,
    scene: &str,
    choices: &Choices,
    chose: &mut Vec<String>,
) -> Result<(), String> {
    let at = |e: hero_core::drama::DramaError| format!("scene `{scene}`: {e}");
    let mut runner = DramaRunner::new(pack, scene).map_err(at)?;
    loop {
        match runner.next(pack, campaign).map_err(at)? {
            Step::End => return Ok(()),
            Step::Choice(options) => {
                let pick = choices.get(scene).copied().unwrap_or(0);
                let Some(text) = options.get(pick) else {
                    return Err(format!(
                        "--choose {scene}={}: the choice offers {} option(s)",
                        pick + 1,
                        options.len()
                    ));
                };
                chose.push(format!("{scene}: {text}"));
                runner.choose(pack, pick).map_err(at)?;
            }
            _ => {}
        }
    }
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
pub fn render(pack: &Pack, runs: &[Run]) -> (String, bool) {
    let mut out = String::new();
    let mut failed = false;
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

    fn fixture() -> Pack {
        crate::tests::fixture_pack()
    }

    #[test]
    fn a_run_walks_the_campaign_and_carries_the_army() {
        let pack = fixture();
        let run = run_seed(&pack, 1, &Choices::new());
        assert!(
            !matches!(run.end, End::Error(_) | End::Panicked(_) | End::Looping),
            "{run:?}"
        );
        assert!(!run.fought.is_empty(), "{run:?}");
        // Runs are deterministic for a seed.
        assert_eq!(run, run_seed(&pack, 1, &Choices::new()));
    }

    #[test]
    fn choices_pick_their_option() {
        let pack = fixture();
        // The prologue's scene `oath` asks whether to pursue.
        let first = run_seed(&pack, 1, &Choices::new());
        assert_eq!(first.chose[0], "oath: 적을 끝까지 쫓는다");
        let second = run_seed(&pack, 1, &Choices::from([("oath".to_string(), 1)]));
        assert_eq!(second.chose[0], "oath: 마을을 지킨다");
        // An option the choice does not have ends the run with an error that says so.
        let bad = run_seed(&pack, 1, &Choices::from([("oath".to_string(), 8)]));
        assert_eq!(
            bad.end,
            End::Error("--choose oath=9: the choice offers 2 option(s)".into())
        );
        assert!(bad.fought.is_empty());
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
                end: End::Ending("finale".into()),
            },
            Run {
                fought: vec![fought(false)],
                chose: vec!["s: a".into()],
                end: End::GameOver(battle.clone()),
            },
        ];
        let (report, failed) = render(&pack, &runs);
        assert!(!failed, "{report}");
        assert!(report.contains("50%"), "{report}");
        assert!(report.contains("1 reached an ending"), "{report}");
        let stuck = [Run {
            fought: vec![],
            chose: vec![],
            end: End::Stuck(battle.clone()),
        }];
        let (report, failed) = render(&pack, &stuck);
        assert!(failed && report.contains("WARNING: none"), "{report}");
    }
}
