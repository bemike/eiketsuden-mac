//! `[presentation]` of `pack.toml`: the default canvas, the allowed range (checked when the
//! manifest loads) and the warning for canvases the frontend's screens are not laid out for.

mod common;

use common::*;
use hero_core::pack::{Pack, PackError, Presentation, DEFAULT_CANVAS, MAX_CANVAS, MIN_CANVAS};

/// The fixture with a `[presentation]` table holding `body`.
fn with_presentation(body: &str) -> Files {
    let mut files = fixture_files();
    append(
        &mut files,
        "pack.toml",
        &format!("\n[presentation]\n{body}\n"),
    );
    files
}

#[test]
fn packs_without_presentation_get_the_default_canvas() {
    assert_eq!(DEFAULT_CANVAS, [480, 270]);
    let pack = load_fixture();
    assert_eq!(pack.manifest.presentation, Presentation::default());
    assert_eq!(pack.manifest.presentation.canvas, DEFAULT_CANVAS);
}

#[test]
fn a_declared_canvas_is_used() {
    let pack = load(&with_presentation("canvas = [640, 480]"));
    assert_eq!(pack.manifest.presentation.canvas, [640, 480]);
    let issues = pack.validate();
    assert!(issues.is_empty(), "{}", format_issues(&issues));
    // Both limits are allowed.
    for limit in [MIN_CANVAS, MAX_CANVAS] {
        let body = format!("canvas = [{}, {}]", limit[0], limit[1]);
        assert_eq!(
            load(&with_presentation(&body)).manifest.presentation.canvas,
            limit
        );
    }
}

#[test]
fn canvases_outside_the_range_do_not_load() {
    for bad in [
        "canvas = [319, 240]",
        "canvas = [640, 199]",
        "canvas = [1281, 480]",
        "canvas = [640, 801]",
    ] {
        match load_err(&with_presentation(bad)) {
            PackError::Parse { file, msg } => {
                assert_eq!(file, "pack.toml");
                assert!(msg.contains("presentation.canvas"), "{bad}: {msg}");
            }
            other => panic!("{bad}: unexpected error {other:?}"),
        }
    }
}

#[test]
fn small_canvases_are_allowed_with_a_warning() {
    for small in ["canvas = [320, 200]", "canvas = [640, 240]"] {
        let issues = load(&with_presentation(small)).validate();
        assert!(
            issues
                .iter()
                .all(|i| i.severity == hero_core::pack::Severity::Warning),
            "{small}:\n{}",
            format_issues(&issues)
        );
        assert_issue(
            &issues,
            hero_core::pack::Severity::Warning,
            "pack.toml",
            "smaller than 480x270",
        );
    }
    // The default and bigger canvases do not warn.
    let issues = load(&with_presentation("canvas = [480, 270]")).validate();
    assert!(issues.is_empty(), "{}", format_issues(&issues));
}

#[test]
fn misspelt_keys_fall_back_to_the_default_and_are_reported() {
    let files = with_presentation("canvs = [640, 480]");
    assert_eq!(load(&files).manifest.presentation, Presentation::default());
    let issues = Pack::unknown_fields(&files).expect("pack parses");
    assert_issue(
        &issues,
        hero_core::pack::Severity::Warning,
        "pack.toml",
        "unknown field `presentation.canvs`",
    );
}
