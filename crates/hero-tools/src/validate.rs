//! `hero-tools validate`: structural load, cross-reference checks, unknown TOML keys and the
//! media check, printed grouped by severity.

use hero_core::pack::{DirSource, Issue, Pack, Severity};
use std::fmt::Write as _;
use std::path::Path;

/// Returns `Ok(true)` when the pack has no errors (warnings are fine).
pub fn run(dir: &Path) -> Result<bool, String> {
    let pack = crate::load_pack(dir)?;
    let src = DirSource {
        root: dir.to_path_buf(),
    };
    let mut issues = pack.validate();
    issues.extend(Pack::unknown_fields(&src).map_err(|e| e.to_string())?);
    issues.extend(pack.missing_media(dir));
    print!("{}", render(&pack, &issues));
    Ok(!issues.iter().any(|i| i.severity == Severity::Error))
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
}
