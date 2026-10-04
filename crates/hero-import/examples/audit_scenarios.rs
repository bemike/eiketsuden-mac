//! Read-only scenario inspection. The JSON contains game data: keep output private.
use hero_import::{ls11, scenario};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: audit_scenarios ORIGINAL_DIR OUTPUT_JSON".into());
    }
    let install = PathBuf::from(&args[0]);
    let mut scenes = Vec::new();
    for chapter in 0..=4 {
        let bytes = std::fs::read(install.join(format!("SNR{chapter}D.R3")))?;
        let archive = ls11::Archive::parse(&bytes)?;
        for index in 0..archive.len() {
            let decoded = archive.decode(index)?;
            let scene = scenario::parse_scene(&decoded)?;
            let parts: Vec<_> = hero_import::chapters::parts(&scene).iter().filter_map(|part| {
                if let hero_import::chapters::Part::Battle { block, leg, .. } = *part {
                    Some(serde_json::json!({"block":block,"leg":leg,"records":hero_import::battles::battle_leg(&scene, block, leg).records}))
                } else { None }
            }).collect();
            scenes.push(serde_json::json!({"chapter":chapter,"index":index,"scene":scene,"battle_parts":parts}));
        }
    }
    std::fs::write(&args[1], serde_json::to_vec_pretty(&scenes)?)?;
    Ok(())
}
