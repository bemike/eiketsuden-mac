//! Helpers shared by the integration tests: the `mini` fixture pack as an in-memory file
//! map that tests can edit to build deliberately broken copies.

// Every test binary compiles this module but uses a different subset of it.
#![allow(dead_code)]

use hero_core::pack::{Issue, Pack, PackError, Severity};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub type Files = BTreeMap<String, String>;

pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini")
}

/// Every text file of the fixture pack, keyed by its `/`-separated path inside the pack.
pub fn fixture_files() -> Files {
    let mut files = Files::new();
    collect(&fixture_dir(), "", &mut files);
    files
}

fn collect(dir: &Path, prefix: &str, files: &mut Files) {
    for entry in std::fs::read_dir(dir).expect("fixture directory is readable") {
        let entry = entry.expect("fixture entry is readable");
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let path = entry.path();
        if path.is_dir() {
            collect(&path, &rel, files);
        } else {
            let text = std::fs::read_to_string(&path).expect("fixture file is UTF-8 text");
            files.insert(rel, text);
        }
    }
}

pub fn load(files: &Files) -> Pack {
    match Pack::load(files) {
        Ok(pack) => pack,
        Err(e) => panic!("pack failed to load: {e}"),
    }
}

pub fn load_fixture() -> Pack {
    load(&fixture_files())
}

pub fn load_err(files: &Files) -> PackError {
    match Pack::load(files) {
        Ok(_) => panic!("pack loaded although it should not"),
        Err(e) => e,
    }
}

/// Replace the first occurrence of `from` in `file`; panics when `from` is absent so that a
/// changed fixture cannot silently turn a test into a no-op.
pub fn edit(files: &mut Files, file: &str, from: &str, to: &str) {
    let text = files
        .get_mut(file)
        .unwrap_or_else(|| panic!("no fixture file {file}"));
    assert!(text.contains(from), "{file} does not contain {from:?}");
    *text = text.replacen(from, to, 1);
}

pub fn append(files: &mut Files, file: &str, extra: &str) {
    let text = files
        .get_mut(file)
        .unwrap_or_else(|| panic!("no fixture file {file}"));
    text.push_str(extra);
}

pub fn format_issues(issues: &[Issue]) -> String {
    issues
        .iter()
        .map(|i| format!("  {:?} [{}] {}", i.severity, i.context, i.msg))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert that `issues` contains an issue of `severity` whose context and message contain the
/// given fragments.
pub fn assert_issue(issues: &[Issue], severity: Severity, context: &str, msg: &str) {
    let found = issues
        .iter()
        .any(|i| i.severity == severity && i.context.contains(context) && i.msg.contains(msg));
    assert!(
        found,
        "expected {severity:?} [{context}] ...{msg}... in:\n{}",
        format_issues(issues)
    );
}
