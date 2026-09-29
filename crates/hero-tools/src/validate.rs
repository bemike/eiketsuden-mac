//! `hero-tools validate`: structural load, cross-reference checks, unknown TOML keys and the
//! media check, printed grouped by severity. A layered pack is checked as the game loads it:
//! the whole chain, with media looked up in every pack of it.

use hero_core::pack::{DirSource, Issue, Pack, Severity};
use std::fmt::Write as _;
use std::path::Path;

/// Returns `Ok(true)` when the pack has no errors (warnings are fine).
pub fn run(dir: &Path) -> Result<bool, String> {
    let pack = crate::load_pack(dir)?;
    let issues = check(dir, &pack)?;
    print!("{}", render(&pack, &issues));
    Ok(!issues.iter().any(|i| i.severity == Severity::Error))
}

/// Every issue of the pack in `dir` (already loaded as `pack`): cross-references, unknown TOML
/// keys and missing media.
pub fn check(dir: &Path, pack: &Pack) -> Result<Vec<Issue>, String> {
    let src = DirSource {
        root: dir.to_path_buf(),
    };
    let mut issues = pack.validate();
    issues.extend(Pack::unknown_fields(&src).map_err(|e| e.to_string())?);
    issues.extend(pack.missing_media(dir));
    // A written original pack (this one or one it extends) copies from the pack it extends:
    // warn when that one changed.
    for (layer, why) in hero_import::pack::stale_packs(dir, pack) {
        issues.push(Issue {
            severity: Severity::Warning,
            context: layer
                .join(hero_import::pack::PACK_INDEX)
                .display()
                .to_string(),
            msg: why,
        });
    }
    Ok(issues)
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// The report: a header, errors, warnings and a summary line.
pub fn render(pack: &Pack, issues: &[Issue]) -> String {
    let m = &pack.manifest;
    let mut out = format!("{} ({} {})\n", m.name, m.id, m.version);
    if let Some(parents) = crate::info::parents(pack) {
        let _ = writeln!(out, "extends {parents}");
    }
    let mut totals = Vec::new();
    for (severity, title) in [(Severity::Error, "Errors"), (Severity::Warning, "Warnings")] {
        let group: Vec<&Issue> = issues.iter().filter(|i| i.severity == severity).collect();
        totals.push(group.len());
        if group.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n{title} ({}):", group.len());
        for issue in group {
            // Multi-line messages (TOML parse errors) stay indented under their issue.
            let msg = issue.msg.replace('\n', "\n      ");
            let _ = writeln!(out, "  - {}: {msg}", issue.context);
        }
    }
    let verdict = if totals[0] == 0 { "OK" } else { "FAILED" };
    let _ = writeln!(
        out,
        "\n{}, {}: {verdict}",
        count(totals[0], "error"),
        count(totals[1], "warning")
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(severity: Severity, context: &str, msg: &str) -> Issue {
        Issue {
            severity,
            context: context.into(),
            msg: msg.into(),
        }
    }

    #[test]
    fn groups_by_severity() {
        let pack = crate::tests::fixture_pack();
        let issues = [
            issue(Severity::Warning, "scene spare", "is never played"),
            issue(
                Severity::Error,
                "battle b01",
                "turn_limit must be at least 1",
            ),
            issue(
                Severity::Error,
                "rules/game.toml",
                "TOML parse error\n  |\n1 | x =",
            ),
        ];
        let out = render(&pack, &issues);
        let errors = out.find("Errors (2):").expect(&out);
        let warnings = out.find("Warnings (1):").expect(&out);
        assert!(errors < warnings, "{out}");
        assert!(
            out.contains("  - battle b01: turn_limit must be at least 1\n"),
            "{out}"
        );
        assert!(
            out.contains("TOML parse error\n        |\n      1 | x ="),
            "{out}"
        );
        assert!(out.ends_with("2 errors, 1 warning: FAILED\n"), "{out}");
        assert!(out.starts_with("Mini test pack (mini 0.1.0)\n"), "{out}");
    }

    #[test]
    fn clean_report() {
        let pack = crate::tests::fixture_pack();
        let out = render(&pack, &[issue(Severity::Warning, "x", "y")]);
        assert!(!out.contains("Errors"), "{out}");
        assert!(out.ends_with("0 errors, 1 warning: OK\n"), "{out}");
        assert!(render(&pack, &[]).ends_with("0 errors, 0 warnings: OK\n"));
    }

    #[test]
    fn layered_packs_are_checked_across_the_chain() {
        let dir = crate::tests::layered_fixture_dir();
        let pack = crate::tests::layered_fixture_pack();
        let out = render(&pack, &[]);
        assert!(
            out.starts_with("Mini extension (mini_ext 0.1.0)\nextends ../mini (mini 0.1.0)\n"),
            "{out}"
        );
        // The fixtures ship no media: only missing media files are reported, and each once
        // although it is looked up in both packs.
        let issues = check(&dir, &pack).unwrap();
        assert!(!issues.is_empty());
        assert_eq!(issues, pack.missing_media(&dir), "only media is missing");
        let unknown_portrait = issues
            .iter()
            .filter(|i| i.msg.contains("gfx/portraits/_unknown.png"))
            .count();
        assert_eq!(unknown_portrait, 1);
    }
}
