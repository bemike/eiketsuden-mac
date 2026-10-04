//! Regression on the original Beihai map and a copied legacy save; never writes the input.
use hero_core::{
    battle::{Action, BattleEvent, UnitState},
    battledef::{AiMode, Side},
    geom::Pos,
    pack::{DirSource, Pack},
    save::SaveGame,
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err("usage: verify_beihai_ai PACK LEGACY_SAVE [FIXTURE_DIR]".into());
    }
    let pack = Pack::load(&DirSource {
        root: PathBuf::from(&args[1]),
    })?;
    assert!(pack.rules.ai_defend_on_arrival);
    let bytes = std::fs::read(&args[2])?;
    let save: SaveGame = serde_json::from_slice(&bytes)?;
    let source = save.battle.as_ref().ok_or("battle missing")?;
    assert_eq!(source.battle_id, "c1_s1_b3");
    let zhao = source
        .units
        .iter()
        .position(|u| u.matches("zhao_yun"))
        .ok_or("Zhao missing")?;
    let guan = source
        .units
        .iter()
        .position(|u| u.matches("guan_hai"))
        .ok_or("Guan missing")?;
    assert_eq!(source.units[guan].ai, AiMode::March);
    assert_eq!(source.units[guan].ai_pos, Some(source.units[guan].pos));
    let cavalry = &pack.classes[&source.units[zhao].class].move_type;
    assert!(
        source
            .terrain_at(&pack, source.units[guan].pos)
            .unwrap()
            .move_cost(cavalry)
            .is_none(),
        "the stranded boss is on cavalry-inaccessible terrain"
    );
    // Keep the real map and boss AI; isolate this encounter from unrelated combat targets.
    let mut clean = source.clone();
    clean.outcome = None;
    clean.phase = Side::Enemy;
    for u in &mut clean.units {
        u.moved = false;
        u.acted = false;
        if u.id != zhao && u.id != guan {
            u.state = UnitState::Hidden;
        }
    }
    let boss_tile = clean.units[guan].pos;
    let mut fixture = None;
    let mut tiles: Vec<_> = (0..40)
        .flat_map(|y| (0..40).map(move |x| Pos::new(x, y)))
        .collect();
    tiles.sort_by_key(|tile| (tile.manhattan(source.units[zhao].pos), *tile));
    for tile in tiles {
        if tile.manhattan(boss_tile) <= 1
            || clean
                .terrain_at(&pack, tile)
                .and_then(|t| t.move_cost(cavalry))
                .is_none()
        {
            continue;
        }
        let mut st = clean.clone();
        st.units[zhao].pos = tile;
        let before = st.units[zhao].level;
        let plan = st.ai_actions(&pack, guan);
        let Some(Action::Move { to, .. }) = plan.first() else {
            continue;
        };
        if to.manhattan(tile) != 1 || *to == boss_tile {
            continue;
        }
        let candidate = st.clone();
        let ev = st.apply(&pack, plan[0].clone())?;
        if !ev
            .iter()
            .any(|e| matches!(e, BattleEvent::Drama { scene } if scene == "orig_c1_s1_b3_3"))
        {
            continue;
        }
        assert_eq!(st.units[zhao].level, before + 1);
        assert_eq!(st.units[guan].state, UnitState::Retreated);
        assert!(st.outcome.is_some(), "duel wins Beihai");
        assert_eq!(st.units[guan].ai, AiMode::Defensive);
        println!("PASS: legacy boss {boss_tile:?} leaves mountain for {to:?}; Zhao on legal cavalry tile {tile:?}; duel, +1 level, retreat and victory");
        fixture = Some(candidate);
        break;
    }
    let mut fixture = fixture.ok_or("no successful legal foothill contact")?;
    if let Some(dir) = args.get(3) {
        let mut ui = save.clone();
        fixture.phase = Side::Player;
        for u in &mut fixture.units {
            u.moved = false;
            u.acted = false;
        }
        ui.battle = Some(fixture);
        ui.scene = None;
        ui.pending_scenes.clear();
        ui.label = "北海 AI 隔离测试".into();
        std::fs::create_dir_all(dir)?;
        std::fs::write(
            PathBuf::from(dir).join("save_original_auto.json"),
            serde_json::to_vec_pretty(&ui)?,
        )?;
    }
    assert_eq!(
        std::fs::read(&args[2])?,
        bytes,
        "input save must remain unchanged"
    );
    Ok(())
}
