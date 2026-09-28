//! `hero-tools simulate`: plays battles AI against AI across seeds to catch panics, battles
//! that never end, and battles that are always or never won.
//!
//! The army for a battle is a new game's starting army plus every officer the battle needs
//! on the player side (`deploy.required` and officers its conditions and events refer to
//! that are not units of the battle), at their `officers.toml` levels. Later battles are
//! therefore played with an under-levelled army, so their win rates are pessimistic.

use crate::Failure;
use hero_core::battle::{BattleState, DefeatReason, Outcome};
use hero_core::battledef::BattleDef;
use hero_core::campaign::{CampaignState, Node};
use hero_core::pack::{Pack, Severity};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

/// Phases (player, ally and enemy phases each count) after which a run that has not ended
/// counts as stuck.
pub const MAX_PHASES: u32 = 200;

/// `Ok(true)` when no battle failed. An unknown `only` battle is a [`Failure::Usage`].
pub fn run(dir: &Path, seeds: u32, only: Option<&str>) -> Result<bool, Failure> {
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
    let battles = battle_order(&pack, only).map_err(Failure::Usage)?;
    println!(
        "Simulating {} battle(s) x {seeds} seed(s), at most {MAX_PHASES} phases per run\n",
        battles.len()
    );
    let _quiet = QuietPanics::install();
    let (mut failed, mut warned) = (0, 0);
    for id in &battles {
        let mut stats = BattleStats::new(id);
        match army_for(&pack, id) {
            Ok(army) => {
                for seed in 1..=seeds {
                    stats.record(seed, run_seed(&pack, id, &army, seed));
                }
            }
            Err(e) => stats.setup_errors.push((0, e)),
        }
        print!("{}", stats.render(&pack.battles[id.as_str()].name));
        failed += usize::from(!stats.errors().is_empty());
        warned += usize::from(!stats.warnings().is_empty());
    }
    println!(
        "\n{} battle(s): {failed} failed, {warned} with warnings: {}",
        battles.len(),
        if failed == 0 { "OK" } else { "FAILED" }
    );
    Ok(failed == 0)
}

/// Battles in campaign order, then battles the campaign does not use (by id); only `only`
/// when it is given. An unknown `only` is an error that lists the pack's battles.
pub fn battle_order(pack: &Pack, only: Option<&str>) -> Result<Vec<String>, String> {
    if let Some(id) = only {
        return if pack.battles.contains_key(id) {
            Ok(vec![id.to_string()])
        } else {
            let known: Vec<&str> = pack.battles.keys().map(|k| k.as_str()).collect();
            Err(format!(
                "unknown battle `{id}` (the pack's battles: {})",
                known.join(", ")
            ))
        };
    }
    let mut order: Vec<String> = Vec::new();
    let campaign_battles = pack.campaign.nodes.iter().filter_map(|n| match n {
        Node::Battle { battle, .. } => Some(battle),
        _ => None,
    });
    for id in campaign_battles.chain(pack.battles.keys()) {
        if pack.battles.contains_key(id) && !order.contains(id) {
            order.push(id.clone());
        }
    }
    Ok(order)
}

/// Officers the battle refers to that must come from the player's army: references in
/// conditions, events and AI targets that name an officer who is not a unit of the battle.
pub fn player_needs(pack: &Pack, battle: &BattleDef) -> Vec<String> {
    let mut needs: Vec<String> = Vec::new();
    for (_, r) in battle.unit_refs() {
        let is_unit = battle
            .units
            .iter()
            .any(|u| u.tag.as_deref() == Some(r) || u.officer.as_deref() == Some(r));
        if !is_unit && pack.officer(r).is_some() && !needs.iter().any(|n| n == r) {
            needs.push(r.to_string());
        }
    }
    needs
}

/// Starting army plus the battle's required and referenced officers, with `deployed` set:
/// required officers, then lords, then the roster in order, skipping forbidden officers, up
/// to `deploy.max`.
pub fn army_for(pack: &Pack, id: &str) -> Result<CampaignState, String> {
    let battle = pack
        .battles
        .get(id)
        .ok_or_else(|| format!("unknown battle `{id}`"))?;
    let mut army = CampaignState::new_game(pack);
    for officer in battle
        .deploy
        .required
        .iter()
        .cloned()
        .chain(player_needs(pack, battle))
    {
        army.join(pack, &officer).map_err(|e| e.to_string())?;
    }
    let deploy = &battle.deploy;
    let lords = army
        .roster
        .iter()
        .filter(|o| pack.officer(&o.id).is_some_and(|def| def.lord))
        .map(|o| &o.id);
    let mut deployed: Vec<String> = Vec::new();
    // Required officers and lords always go; the rest of the roster fills up to `max`.
    for officer in deploy.required.iter().chain(lords) {
        push_deployable(&mut deployed, &deploy.forbidden, officer);
    }
    for o in &army.roster {
        if deployed.len() >= deploy.max as usize {
            break;
        }
        push_deployable(&mut deployed, &deploy.forbidden, &o.id);
    }
    army.deployed = deployed;
    Ok(army)
}

/// Append `officer` unless it is forbidden or already deployed.
fn push_deployable(deployed: &mut Vec<String>, forbidden: &[String], officer: &str) {
    if !forbidden.iter().any(|f| f == officer) && !deployed.iter().any(|d| d == officer) {
        deployed.push(officer.to_string());
    }
}

/// How one seeded run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    Finished {
        outcome: Outcome,
        turns: u32,
    },
    /// Still undecided after [`MAX_PHASES`] phases.
    Stuck {
        turns: u32,
    },
    SetupFailed(String),
    Panicked(String),
}

fn run_seed(pack: &Pack, id: &str, army: &CampaignState, seed: u32) -> RunResult {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut state = match BattleState::new(pack, id, army, u64::from(seed)) {
            Ok(s) => s,
            Err(e) => return RunResult::SetupFailed(e.to_string()),
        };
        state.begin(pack);
        let mut phases = 0;
        while state.outcome.is_none() && phases < MAX_PHASES {
            state.run_ai_phase(pack);
            phases += 1;
        }
        match state.outcome {
            Some(outcome) => RunResult::Finished {
                outcome,
                turns: state.turn,
            },
            None => RunResult::Stuck { turns: state.turn },
        }
    }));
    result.unwrap_or_else(|_| RunResult::Panicked(take_panic_message()))
}

thread_local! {
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn take_panic_message() -> String {
    LAST_PANIC
        .with(|p| p.borrow_mut().take())
        .unwrap_or_else(|| "panic without a message".into())
}

/// While alive, panics are recorded for the report instead of being printed by the default
/// hook; dropping it restores the default hook.
pub(crate) struct QuietPanics;

impl QuietPanics {
    pub(crate) fn install() -> QuietPanics {
        panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".into());
            let at = info
                .location()
                .map(|l| format!(" ({}:{})", l.file(), l.line()))
                .unwrap_or_default();
            LAST_PANIC.with(|p| *p.borrow_mut() = Some(format!("{msg}{at}")));
        }));
        QuietPanics
    }
}

impl Drop for QuietPanics {
    fn drop(&mut self) {
        // `take_hook` unregisters our hook and puts the default one back.
        drop(panic::take_hook());
    }
}

fn defeat_name(reason: DefeatReason) -> &'static str {
    match reason {
        DefeatReason::LordRetreated => "lord retreated",
        DefeatReason::TurnLimit => "turn limit",
        DefeatReason::Condition => "defeat condition",
        DefeatReason::Event => "event",
    }
}

fn seed_list(seeds: &[u32]) -> String {
    seeds
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Results of all seeds of one battle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BattleStats {
    pub id: String,
    pub runs: u32,
    pub wins: u32,
    /// Defeats by reason.
    pub defeats: BTreeMap<&'static str, u32>,
    /// Sum of the final turn of every finished run.
    pub turns: u64,
    pub stuck: Vec<u32>,
    /// `(seed, message)`.
    pub panics: Vec<(u32, String)>,
    /// `(seed, message)`; seed 0 when the army could not be built.
    pub setup_errors: Vec<(u32, String)>,
}

impl BattleStats {
    pub fn new(id: &str) -> BattleStats {
        BattleStats {
            id: id.to_string(),
            ..BattleStats::default()
        }
    }

    pub fn record(&mut self, seed: u32, result: RunResult) {
        self.runs += 1;
        match result {
            RunResult::Finished { outcome, turns } => {
                self.turns += u64::from(turns);
                match outcome {
                    Outcome::Victory => self.wins += 1,
                    Outcome::Defeat(reason) => {
                        *self.defeats.entry(defeat_name(reason)).or_insert(0) += 1
                    }
                }
            }
            RunResult::Stuck { .. } => self.stuck.push(seed),
            RunResult::SetupFailed(msg) => self.setup_errors.push((seed, msg)),
            RunResult::Panicked(msg) => self.panics.push((seed, msg)),
        }
    }

    fn finished(&self) -> u32 {
        self.wins + self.defeats.values().sum::<u32>()
    }

    /// Problems that make `simulate` fail.
    pub fn errors(&self) -> Vec<String> {
        let mut out = Vec::new();
        let grouped = |list: &[(u32, String)], what: &str| -> Vec<String> {
            // One line per distinct message, with the seeds that hit it.
            let mut by_msg: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
            for (seed, msg) in list {
                by_msg.entry(msg).or_default().push(*seed);
            }
            by_msg
                .into_iter()
                .map(|(msg, seeds)| format!("{what} (seeds {}): {msg}", seed_list(&seeds)))
                .collect()
        };
        out.extend(grouped(&self.setup_errors, "setup failed"));
        out.extend(grouped(&self.panics, "panicked"));
        if !self.stuck.is_empty() {
            out.push(format!(
                "no outcome after {MAX_PHASES} phases (seeds {})",
                seed_list(&self.stuck)
            ));
        }
        out
    }

    /// Balance hints that do not fail the run.
    pub fn warnings(&self) -> Vec<String> {
        let finished = self.finished();
        if finished == 0 {
            return Vec::new();
        }
        if self.wins == 0 {
            let reasons: Vec<String> = self
                .defeats
                .iter()
                .map(|(reason, n)| format!("{reason} {n}"))
                .collect();
            vec![format!("never won (defeats: {})", reasons.join(", "))]
        } else if self.wins == self.runs {
            vec!["always won".to_string()]
        } else {
            Vec::new()
        }
    }

    pub fn render(&self, name: &str) -> String {
        let mut out = format!("{} {name}: {} run(s)", self.id, self.runs);
        let finished = self.finished();
        if finished > 0 {
            let _ = write!(
                out,
                ", won {:.0}% ({}/{}), {:.1} turns on average",
                f64::from(self.wins) * 100.0 / f64::from(self.runs),
                self.wins,
                self.runs,
                self.turns as f64 / f64::from(finished)
            );
        }
        out.push('\n');
        for e in self.errors() {
            let _ = writeln!(out, "    ERROR: {e}");
        }
        for w in self.warnings() {
            let _ = writeln!(out, "    WARNING: {w}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fixture_pack;
    use hero_core::battledef::Condition;
    use hero_core::geom::Pos;

    #[test]
    fn battles_follow_the_campaign() {
        let pack = fixture_pack();
        assert_eq!(battle_order(&pack, None).unwrap(), ["b01", "b02"]);
        assert_eq!(battle_order(&pack, Some("b02")).unwrap(), ["b02"]);
        assert_eq!(
            battle_order(&pack, Some("b99")).unwrap_err(),
            "unknown battle `b99` (the pack's battles: b01, b02)"
        );

        // A battle the campaign does not use still runs, after the campaign's.
        let mut pack = pack;
        let mut extra = pack.battles["b01"].clone();
        extra.id = "a00".into();
        pack.battles.insert("a00".into(), extra);
        assert_eq!(battle_order(&pack, None).unwrap(), ["b01", "b02", "a00"]);
    }

    #[test]
    fn unknown_battles_are_usage_errors() {
        // A typo in `--battle` is a bad command line (exit 2), not a broken pack (exit 1).
        let dir = crate::tests::fixture_dir();
        match run(&dir, 1, Some("b99")) {
            Err(Failure::Usage(msg)) => assert!(msg.contains("unknown battle `b99`"), "{msg}"),
            other => panic!("expected a usage error, got {other:?}"),
        }
        let missing = dir.join("does-not-exist");
        assert!(matches!(
            run(&missing, 1, Some("b01")),
            Err(Failure::Failed(_))
        ));
    }

    #[test]
    fn layered_packs_simulate_every_battle_of_the_chain() {
        let pack = crate::tests::layered_fixture_pack();
        // b01 and b02 come from the parent pack, b03 from the child's campaign.
        assert_eq!(battle_order(&pack, None).unwrap(), ["b01", "b02", "b03"]);
        let dir = crate::tests::layered_fixture_dir();
        assert!(matches!(run(&dir, 1, Some("b03")), Ok(true)));
    }

    #[test]
    fn army_deploys_required_lord_then_roster() {
        let pack = fixture_pack();
        let b01 = army_for(&pack, "b01").unwrap();
        assert_eq!(b01.deployed, ["liu_bei", "guan_yu", "zhang_fei"]);
        // b02: jian_yong is required (joins), zhang_fei is forbidden, max 3.
        let b02 = army_for(&pack, "b02").unwrap();
        assert!(b02.officer("jian_yong").is_some());
        assert_eq!(b02.deployed, ["jian_yong", "liu_bei", "guan_yu"]);
    }

    #[test]
    fn officers_referenced_by_the_battle_join() {
        let mut pack = fixture_pack();
        let b01 = pack.battles.get_mut("b01").unwrap();
        // References to battle units (`boss`, `zhang_bao`) and starting officers need nothing;
        // `jian_yong` is neither and must come from the army.
        b01.victory.push(Condition::Reach {
            who: Some("jian_yong".into()),
            pos: Pos::new(8, 1),
            radius: 0,
            to: None,
        });
        b01.victory.push(Condition::DefeatUnit {
            target: "zhang_bao".into(),
        });
        let needs = player_needs(&pack, &pack.battles["b01"]);
        assert_eq!(needs, ["jian_yong", "guan_yu", "liu_bei"]);
        let army = army_for(&pack, "b01").unwrap();
        assert!(army.officer("jian_yong").is_some());
        assert_eq!(army.deployed, ["liu_bei", "guan_yu", "zhang_fei"], "max 3");
    }

    #[test]
    fn stats_verdicts() {
        let finished = |outcome, turns| RunResult::Finished { outcome, turns };
        let mut mixed = BattleStats::new("b01");
        mixed.record(1, finished(Outcome::Victory, 10));
        mixed.record(2, finished(Outcome::Defeat(DefeatReason::TurnLimit), 20));
        assert!(mixed.errors().is_empty() && mixed.warnings().is_empty());
        assert_eq!(
            mixed.render("들판 전투"),
            "b01 들판 전투: 2 run(s), won 50% (1/2), 15.0 turns on average\n"
        );

        let mut lost = BattleStats::new("b02");
        lost.record(1, finished(Outcome::Defeat(DefeatReason::TurnLimit), 15));
        lost.record(2, finished(Outcome::Defeat(DefeatReason::LordRetreated), 4));
        assert_eq!(
            lost.warnings(),
            ["never won (defeats: lord retreated 1, turn limit 1)"]
        );
        assert!(lost.errors().is_empty());

        let mut won = BattleStats::new("b03");
        won.record(1, finished(Outcome::Victory, 3));
        assert_eq!(won.warnings(), ["always won"]);

        let mut broken = BattleStats::new("b04");
        broken.record(1, RunResult::Panicked("boom (src/battle/mod.rs:1)".into()));
        broken.record(2, RunResult::Panicked("boom (src/battle/mod.rs:1)".into()));
        broken.record(3, RunResult::Stuck { turns: 67 });
        broken.record(
            4,
            RunResult::SetupFailed("battle setup failed: no slots".into()),
        );
        broken.record(5, finished(Outcome::Victory, 9));
        assert_eq!(
            broken.errors(),
            [
                "setup failed (seeds 4): battle setup failed: no slots",
                "panicked (seeds 1, 2): boom (src/battle/mod.rs:1)",
                "no outcome after 200 phases (seeds 3)",
            ]
        );
        // One win in five runs is neither "never" nor "always".
        assert!(broken.warnings().is_empty());
        let out = broken.render("x");
        assert!(
            out.starts_with("b04 x: 5 run(s), won 20% (1/5), 9.0 turns on average\n"),
            "{out}"
        );
        assert!(
            out.contains("    ERROR: panicked (seeds 1, 2): boom"),
            "{out}"
        );
    }
}
