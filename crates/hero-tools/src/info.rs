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

pub fn render(pack: &Pack) -> String {
    let m = &pack.manifest;
    let mut out = format!("{} ({} {})\n", m.name, m.id, m.version);
    if !m.authors.is_empty() {
        let _ = writeln!(out, "Authors:     {}", m.authors.join(", "));
    }
    if !m.license.is_empty() {
        let _ = writeln!(out, "License:     {}", m.license);
    }
    if !m.description.is_empty() {
        let _ = writeln!(out, "About:       {}", m.description);
    }

    let _ = writeln!(out, "\nTerrain:     {}", pack.terrain.len());
    let _ = writeln!(out, "Classes:     {}", pack.classes.len());
    let _ = writeln!(out, "Strategies:  {}", pack.strategies.len());
    let _ = writeln!(out, "Items:       {}", pack.items.len());
    let _ = writeln!(out, "Officers:    {}", pack.officers.len());
    let _ = writeln!(out, "Battles:     {}", pack.battles.len());

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
        m.dramas.len()
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
        ] {
            assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
        }
    }
}
