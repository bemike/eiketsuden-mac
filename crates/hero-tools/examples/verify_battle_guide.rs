//! Read-only checks on a converted pack; optional report contains game notes, never saves.
use hero_core::{
    campaign::CampaignState,
    guide::battle_guide,
    pack::{DirSource, Pack},
};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let pack = Pack::load(&DirSource {
        root: PathBuf::from(
            args.get(1)
                .ok_or("usage: verify_battle_guide PACK [REPORT]")?,
        ),
    })?;
    let campaign = CampaignState::new_game(&pack);
    let before = campaign.clone();
    let mut reports = Vec::new();
    let mut count = 0;
    for (id, battle) in &pack.battles {
        if !id.starts_with('c') {
            continue;
        }
        let guide = battle_guide(&pack, id, &campaign, &[]);
        assert_eq!(guide.len(), 4);
        let text = guide
            .iter()
            .flat_map(|s| s.entries.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        for item in battle
            .treasures
            .iter()
            .filter_map(|t| t.item.as_ref())
            .chain(battle.units.iter().filter_map(|u| u.drop.as_ref()))
        {
            assert!(
                text.contains(&pack.items[item].name),
                "missing treasure/drop {item} in {id}"
            );
        }
        if id == "c1_s1_b5" {
            assert!(
                text.contains("雌雄") && text.contains("張飛") && text.contains("于禁"),
                "XuZhou branch reward/duel missing"
            );
        }
        if id == "c1_s0_b4" {
            assert!(text.contains("關羽") && text.contains("逢紀"));
        }
        if id == "c1_s1_b3" {
            assert!(text.contains("趙雲") && text.contains("管亥"));
        }
        reports.push(serde_json::json!({"battle":id, "name":battle.name, "sections":guide.iter().map(|s|serde_json::json!({"title":s.title,"entries":s.entries})).collect::<Vec<_>>()}));
        count += 1;
    }
    assert_eq!(count, 70);
    assert_eq!(campaign, before);
    if let Some(report) = args.get(2) {
        std::fs::write(report, serde_json::to_vec_pretty(&reports)?)?;
    }
    println!("Verified {count} original battles: four sections, every treasure/drop, branch rewards, and unchanged campaign.");
    Ok(())
}
