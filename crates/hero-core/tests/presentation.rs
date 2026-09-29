//! `[presentation]` of `pack.toml`: the default canvas and the allowed range (checked when the
//! manifest loads).

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
        "canvas = [479, 270]",
        "canvas = [640, 269]",
        "canvas = [320, 200]",
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

/// A battle frame the size of a 640x400 canvas.
const FRAME: &str = "canvas = [640, 400]\n\
    [presentation.battle_frame]\n\
    image = \"ui/battle_frame\"\n\
    map = [16, 32, 416, 352]\n\
    info = [448, 74, 176, 196]\n\
    title = [224, 8, 174, 16]\n\
    status = [448, 34, 78, 28]";

#[test]
fn a_battle_frame_is_read_and_checked_against_the_canvas() {
    let pack = load(&with_presentation(FRAME));
    let frame = pack.manifest.presentation.battle_frame.expect("frame");
    assert_eq!(frame.image, "ui/battle_frame");
    assert_eq!(frame.map, [16, 32, 416, 352]);
    // The buttons and the weather box are optional.
    assert_eq!((frame.menu, frame.weather), (None, None));
    let with_buttons = FRAME.replace(
        "status = [448, 34, 78, 28]",
        "status = [448, 34, 78, 28]\nmenu = [15, 7, 66, 18]\nweather = [594, 33, 28, 30]",
    );
    let buttons = load(&with_presentation(&with_buttons))
        .manifest
        .presentation
        .battle_frame
        .expect("frame");
    assert_eq!(buttons.menu, Some([15, 7, 66, 18]));
    // An area outside the canvas, an empty one and a path for a key do not load.
    for (from, to) in [
        ("map = [16, 32, 416, 352]", "map = [16, 32, 416, 400]"),
        ("info = [448, 74, 176, 196]", "info = [448, 74, 0, 196]"),
        ("image = \"ui/battle_frame\"", "image = \"../ui.png\""),
        // The optional buttons and weather box are checked too.
        (
            "status = [448, 34, 78, 28]",
            "status = [448, 34, 78, 28]\nenemies = [630, 31, 33, 34]",
        ),
        // A button on the map.
        (
            "status = [448, 34, 78, 28]",
            "status = [448, 34, 78, 28]\nmenu = [100, 100, 32, 16]",
        ),
    ] {
        match load_err(&with_presentation(&FRAME.replace(from, to))) {
            PackError::Parse { file, msg } => {
                assert_eq!(file, "pack.toml");
                assert!(msg.contains("battle_frame"), "{to}: {msg}");
            }
            other => panic!("{to}: unexpected error {other:?}"),
        }
    }
    // Without a canvas big enough (the default 480x270), the frame does not fit.
    let small = FRAME.replace("canvas = [640, 400]\n", "");
    assert!(matches!(
        load_err(&with_presentation(&small)),
        PackError::Parse { .. }
    ));
}

/// A camp frame the size of a 640x400 canvas.
const CAMP: &str = "canvas = [640, 400]\n\
    [presentation.camp_frame]\n\
    image = \"ui/camp_frame\"\n\
    view = [17, 15, 511, 322]\n\
    portrait = [552, 24, 64, 80]\n\
    gold = [568, 136, 52, 15]\n\
    level = [584, 160, 36, 15]\n\
    place = [568, 200, 52, 15]\n\
    caption = [12, 352, 244, 32]\n\
    clock = [270, 352, 100, 32]";

#[test]
fn a_camp_frame_needs_a_view_the_camp_screens_fit_in() {
    let pack = load(&with_presentation(CAMP));
    let frame = pack.manifest.presentation.camp_frame.expect("frame");
    assert_eq!(frame.view, [17, 15, 511, 322]);
    // The camp screens are laid out in the view: smaller than the smallest canvas is an error,
    // like an area outside the canvas.
    for (from, to) in [
        ("view = [17, 15, 511, 322]", "view = [17, 15, 400, 322]"),
        ("clock = [270, 352, 100, 32]", "clock = [600, 352, 100, 32]"),
    ] {
        match load_err(&with_presentation(&CAMP.replace(from, to))) {
            PackError::Parse { file, msg } => {
                assert_eq!(file, "pack.toml");
                assert!(msg.contains("camp_frame"), "{to}: {msg}");
            }
            other => panic!("{to}: unexpected error {other:?}"),
        }
    }
}

/// A status window of the original's layout: two of its six slots.
const STATUS: &str = "canvas = [640, 400]\n\
    [presentation.status_frame]\n\
    image = \"ui/status\"\n\
    size = [512, 320]\n\
    title = [8, 8, 288, 32]\n\
    portrait = [320, 16, 64, 80]\n\
    name = [400, 8, 80, 16]\n\
    level = [464, 32, 16, 16]\n\
    lead = [456, 64, 24, 16]\n\
    strength = [456, 96, 24, 16]\n\
    intellect = [456, 128, 24, 16]\n\
    class = [320, 112, 64, 32]\n\
    info = [314, 170, 188, 140]\n\
    page = [48, 272, 48, 32]\n\
    pager = [96, 272, 32, 32]\n\
    rest = [176, 272, 48, 32]\n\
    close = [240, 272, 48, 32]\n\
    [[presentation.status_frame.slots]]\n\
    icon = [16, 64, 32, 32]\n\
    level = [16, 96, 32, 16]\n\
    troops = [104, 72, 40, 16]\n\
    [[presentation.status_frame.slots]]\n\
    icon = [160, 64, 32, 32]\n\
    level = [160, 96, 32, 16]\n\
    troops = [248, 72, 40, 16]";

#[test]
fn a_status_frame_lays_its_parts_out_in_its_picture() {
    let pack = load(&with_presentation(STATUS));
    let frame = pack.manifest.presentation.status_frame.expect("frame");
    assert_eq!(frame.slots.len(), 2);
    assert_eq!(frame.slots[1].troops, [248, 72, 40, 16]);
    for (from, to) in [
        // An area outside the picture.
        ("close = [240, 272, 48, 32]", "close = [500, 272, 48, 32]"),
        ("troops = [248, 72, 40, 16]", "troops = [248, 72, 40, 0]"),
        // A picture larger than the canvas.
        ("size = [512, 320]", "size = [700, 320]"),
        ("image = \"ui/status\"", "image = \"../status.png\""),
    ] {
        match load_err(&with_presentation(&STATUS.replace(from, to))) {
            PackError::Parse { file, msg } => {
                assert_eq!(file, "pack.toml");
                assert!(msg.contains("status_frame"), "{to}: {msg}");
            }
            other => panic!("{to}: unexpected error {other:?}"),
        }
    }
    // Without slots.
    let bare = STATUS
        .split("[[presentation.status_frame.slots]]")
        .next()
        .unwrap()
        .to_string();
    match load_err(&with_presentation(&bare)) {
        PackError::Parse { msg, .. } => assert!(msg.contains("slots"), "{msg}"),
        other => panic!("unexpected error {other:?}"),
    }
}
