//! `hero-tools info`: a summary of a pack's content.

use hero_core::campaign::Node;
use hero_core::pack::Pack;
use hero_core::script::Cmd;
use std::fmt::Write as _;
use std::path::Path;

pub fn run(dir: &Path) -> Result<bool, String> {
    let pack = crate::load_pack(dir)?;
    print!("{}", render(&pack));
    Ok(true)
}

/// The packs a layered pack is built on, nearest first, e.g.
/// `../mini (mini 0.1.0)`; `None` for a pack without `extends`.
pub fn parents(pack: &Pack) -> Option<String> {
    let parents: Vec<String> = pack
        .layers
        .iter()
        .skip(1)
        .map(|l| format!("{} ({} {})", l.dir, l.manifest.id, l.manifest.version))
        .collect();
    (!parents.is_empty()).then(|| parents.join(", then "))
}

pub fn render(pack: &Pack) -> String {
    let m = &pack.manifest;
    let mut out = format!("{} ({} {})\n", m.name, m.id, m.version);
    if let Some(parents) = parents(pack) {
        let _ = writeln!(out, "Extends:     {parents}");
    }
    if !m.authors.is_empty() {
        let _ = writeln!(out, "Authors:     {}", m.authors.join(", "));
    }
    if !m.license.is_empty() {
        let _ = writeln!(out, "License:     {}", m.license);
    }
    if !m.description.is_empty() {
        let _ = writeln!(out, "About:       {}", m.description);
    }
    let [w, h] = m.presentation.canvas;
    let _ = writeln!(out, "Canvas:      {w}x{h}");

    let _ = writeln!(out, "\nTerrain:     {}", pack.terrain.len());
    let _ = writeln!(out, "Classes:     {}", pack.classes.len());
    let _ = writeln!(out, "Strategies:  {}", pack.strategies.len());
    let _ = writeln!(out, "Items:       {}", pack.items.len());
    let _ = writeln!(out, "Officers:    {}", pack.officers.len());
    let _ = writeln!(out, "Battles:     {}", pack.battles.len());
    // Only packs with map files print the line, so the usual output stays as it was.
    if !pack.maps.is_empty() {
        let used = pack
            .maps
            .keys()
            .filter(|id| {
                pack.battles
                    .values()
                    .any(|b| b.map.use_map.as_ref() == Some(*id))
            })
            .count();
        let _ = writeln!(
            out,
            "Maps:        {} in {} files ({used} used by battles)",
            pack.maps.len(),
            pack.files.maps.len()
        );
    }

    let lines: usize = pack
        .scenes
        .values()
        .map(|s| {
            s.cmds
                .iter()
                .filter(|c| matches!(c, Cmd::Say { .. } | Cmd::Narr(_)))
                .count()
        })
        .sum();
    let _ = writeln!(
        out,
        "Scenes:      {} in {} files ({lines} lines of dialogue and narration)",
        pack.scenes.len(),
        pack.files.dramas.len()
    );

    let c = &pack.campaign;
    let mut kinds = [0usize; 5];
    for node in &c.nodes {
        kinds[match node {
            Node::Drama { .. } => 0,
            Node::Camp { .. } => 1,
            Node::Battle { .. } => 2,
            Node::Branch { .. } => 3,
            Node::Ending { .. } => 4,
        }] += 1;
    }
    let _ = writeln!(
        out,
        "Campaign:    \"{}\": {} nodes ({} drama, {} camp, {} battle, {} branch, {} ending), starts at `{}`",
        c.title,
        c.nodes.len(),
        kinds[0],
        kinds[1],
        kinds[2],
        kinds[3],
        kinds[4],
        c.start
    );
    let items: u32 = c.starting_items.values().sum();
    let _ = writeln!(
        out,
        "Start:       {} officers, {} gold, {} items",
        c.starting_officers.len(),
        c.starting_gold,
        items
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_the_fixture() {
        let out = render(&crate::tests::fixture_pack());
        for expected in [
            "Mini test pack (mini 0.1.0)\n",
            "License:     CC0-1.0\n",
            "Terrain:     8\n",
            "Classes:     6\n",
            "Strategies:  4\n",
            "Items:       9\n",
            "Officers:    6\n",
            "Battles:     2\n",
            "Scenes:      9 in 2 files (",
            "10 nodes (3 drama, 2 camp, 2 battle, 2 branch, 1 ending), starts at `prologue`",
            "Start:       3 officers, 500 gold, 3 items\n",
            "Canvas:      480x270\n",
        ] {
            assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
        }
        assert!(!out.contains("Extends:"), "{out}");
        assert!(!out.contains("Maps:"), "{out}");
    }

    #[test]
    fn counts_map_files_and_their_users() {
        let mut pack = crate::tests::fixture_pack();
        for id in ["field", "spare"] {
            pack.maps.insert(
                id.into(),
                hero_core::battledef::MapEntry {
                    id: id.into(),
                    name: String::new(),
                    rows: "..".into(),
                    legend: Default::default(),
                    theme: None,
                    image: None,
                },
            );
        }
        pack.files.maps.push(Default::default());
        pack.battles.get_mut("b01").unwrap().map.use_map = Some("field".into());
        let out = render(&pack);
        assert!(
            out.contains("Maps:        2 in 1 files (1 used by battles)\n"),
            "{out}"
        );
    }

    #[test]
    fn summarises_a_layered_pack() {
        let out = render(&crate::tests::layered_fixture_pack());
        for expected in [
            "Mini extension (mini_ext 0.1.0)\n",
            "Extends:     ../mini (mini 0.1.0)\n",
            "Canvas:      640x480\n",
            "Terrain:     8\n",
            "Battles:     3\n",
            "Scenes:      10 in 3 files (",
            "12 nodes (3 drama, 3 camp, 3 battle, 2 branch, 1 ending)",
        ] {
            assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
        }
    }
}
