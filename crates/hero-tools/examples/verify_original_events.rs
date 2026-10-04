//! Verify a converted private pack without changing its files or the user's saves.
use hero_core::{
    battle::{Action, BattleEvent, BattleState, UnitState},
    battledef::Trigger,
    campaign::CampaignState,
    geom::Pos,
    pack::{DirSource, Pack},
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: verify_original_events PACK")?,
    );
    let pack = Pack::load(&DirSource { root })?;
    let mut campaign = CampaignState::new_game(&pack);
    for id in ["guan_yu", "zhang_fei", "jian_yong"] {
        if campaign.officer(id).is_none() {
            campaign.join(&pack, id)?;
        }
    }
    campaign.deployed = ["liu_bei", "guan_yu", "zhang_fei", "jian_yong"]
        .map(String::from)
        .to_vec();
    let mut state = BattleState::new(&pack, "c1_s0_b4", &campaign, 7)?;
    let opening = state.begin(&pack);
    let duel_scene = "orig_c1_s0_b4_3";
    let is_duel =
        |ev: &BattleEvent| matches!(ev, BattleEvent::Drama { scene } if scene == duel_scene);
    assert!(!opening.iter().any(is_duel), "duel must wait for contact");
    let guan = state
        .units
        .iter()
        .position(|u| u.matches("guan_yu"))
        .ok_or("Guan Yu absent")?;
    let feng = state
        .units
        .iter()
        .position(|u| u.matches("feng_ji"))
        .ok_or("Feng Ji absent")?;
    let level = state.units[guan].level;
    // Place him two cells away for an isolated contact check, then make a legal one-cell move.
    let boss = state.units[feng].pos;
    let mut chosen = None;
    for (dx, dy) in [(0, 1), (1, 0), (0, -1), (-1, 0)] {
        let near = Pos::new(boss.x + dx, boss.y + dy);
        let from = Pos::new(boss.x + 2 * dx, boss.y + 2 * dy);
        if !state.map.in_bounds(from) || !state.map.in_bounds(near) {
            continue;
        }
        if state
            .units
            .iter()
            .any(|u| u.id != guan && u.is_active() && (u.pos == from || u.pos == near))
        {
            continue;
        }
        state.units[guan].pos = from;
        if state.movement_range(&pack, guan).contains(near) {
            chosen = Some(near);
            break;
        }
    }
    let before = serde_json::to_string(&state)?;
    let mut restored: BattleState = serde_json::from_str(&before)?;
    // The 0.1.9 Guangchuan save had just its opening event. New definitions extend it safely.
    restored.fired.truncate(1);
    let target = chosen.ok_or("no legal contact tile")?;
    if let Some(directory) = std::env::args().nth(2) {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory)?;
        campaign.node = "c1_s0_b4_battle".into();
        let save = hero_core::save::SaveGame {
            version: hero_core::save::PLAIN_SAVE_VERSION,
            pack_id: pack.manifest.id.clone(),
            pack_version: pack.manifest.version.clone(),
            label: "isolated Guangchuan verification".into(),
            saved_at: 0,
            campaign: campaign.clone(),
            battle: Some(restored.clone()),
            scene: None,
            pending_scenes: vec![],
        };
        std::fs::write(
            directory.join("save_original_auto.json"),
            serde_json::to_vec_pretty(&save)?,
        )?;
        std::fs::write(
            directory.join("contact-target.json"),
            serde_json::to_vec(&target)?,
        )?;
    }

    let events = restored.apply(
        &pack,
        Action::Move {
            unit: guan,
            to: target,
        },
    )?;
    assert_eq!(events.iter().filter(|e| is_duel(e)).count(), 1);
    assert_eq!(restored.units[guan].level, level + 1);
    assert_eq!(restored.units[feng].state, UnitState::Retreated);
    let after = serde_json::to_string(&restored)?;
    let mut twice: BattleState = serde_json::from_str(&after)?;
    assert!(
        !twice.begin(&pack).iter().any(is_duel),
        "duel must not repeat after loading"
    );
    assert_eq!(twice.units[guan].level, level + 1);
    let contacts: usize = pack
        .battles
        .values()
        .filter(|b| b.id.starts_with('c'))
        .flat_map(|b| &b.events)
        .filter(|e| matches!(e.trigger, Trigger::Adjacent { .. }))
        .count();
    for id in ["c4_s2_b3", "c4_s2_b4"] {
        assert!(
            pack.battles.contains_key(id),
            "Xuchang map-switch battle missing: {id}"
        );
    }
    let mystery = pack
        .officer("orig_p372")
        .ok_or("scripted mystery participant missing")?;
    assert_eq!(mystery.name, "？？？");
    assert!(pack.battles["c4_s0_b5"]
        .events
        .iter()
        .any(|e| matches!(&e.trigger, Trigger::Adjacent { a: Some(a), .. } if a == "orig_p372")));
    println!("PASS: Guangchuan contact, +1 level, Feng Ji retreat, legacy save, no replay; Xuchang 2/3; mystery encounter; {contacts} converted contact events");
    Ok(())
}
