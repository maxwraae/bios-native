use std::path::Path;
use std::time::Duration;

use bios_ui::map::MapGesture;
use craft_dialogs::Answer;

use crate::{App, Then};

/// wrap_demo open: 800 bases, circular, AmpR through the origin (750..100), GFP at 200..500.
fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    (dir, app)
}

#[test]
fn a_press_on_the_map_is_a_caret_and_a_drag_selects_from_it() {
    let (_dir, mut app) = app();
    app.map_gesture(MapGesture::Press { gap: 100, shift: false }, 0.0);
    assert_eq!((app.cursor.caret, app.cursor.selection), (Some(100), None));
    app.map_gesture(MapGesture::Drag { gap: 160 }, 0.0);
    assert_eq!(app.cursor.selection, Some((100, 159)));
    // The rows are asked to bring the moving end into view.
    assert_eq!(app.cursor.reveal, Some(160));
    app.map_gesture(MapGesture::Drag { gap: 40 }, 0.0);
    assert_eq!(app.cursor.selection, Some((40, 99)));
    app.map_gesture(MapGesture::Drag { gap: 100 }, 0.0);
    assert_eq!((app.cursor.caret, app.cursor.selection), (Some(100), None));
}

#[test]
fn a_drag_round_the_ring_goes_through_the_origin_and_a_whole_turn_is_everything() {
    let (_dir, mut app) = app();
    app.map_gesture(MapGesture::Press { gap: 780, shift: false }, 0.0);
    // Past the origin the map keeps counting: 820 is twenty bases into the next turn.
    app.map_gesture(MapGesture::Drag { gap: 820 }, 0.0);
    assert_eq!(app.cursor.selection, Some((780, 19)));
    app.map_gesture(MapGesture::Drag { gap: 1580 }, 0.0);
    assert_eq!(app.cursor.selection, Some((0, 799)));
    // Backwards through the origin from near the start.
    app.map_gesture(MapGesture::Press { gap: 10, shift: false }, 0.0);
    app.map_gesture(MapGesture::Drag { gap: -30 }, 0.0);
    assert_eq!(app.cursor.selection, Some((770, 9)));
}

#[test]
fn a_shift_press_extends_the_short_way_round() {
    let (_dir, mut app) = app();
    app.map_gesture(MapGesture::Press { gap: 780, shift: false }, 0.0);
    app.map_gesture(MapGesture::Press { gap: 20, shift: true }, 0.0);
    assert_eq!(app.cursor.selection, Some((780, 19)), "40 bases through the origin, not 760 the long way");
    // A drag that follows carries on from where the shift-press reached.
    app.map_gesture(MapGesture::Drag { gap: 50 }, 0.0);
    assert_eq!(app.cursor.selection, Some((780, 49)));
}

#[test]
fn a_press_on_a_feature_selects_it_and_opens_its_card() {
    let (_dir, mut app) = app();
    app.map_gesture(MapGesture::Feature { start: 749, end: 99 }, 0.0);
    assert_eq!(app.cursor.selection, Some((749, 99)));
    assert_eq!(app.card.as_ref().map(|c| c.was.name.as_str()), Some("AmpR"));
    // A press elsewhere leaves the panel open, as in the web app.
    app.map_gesture(MapGesture::Press { gap: 300, shift: false }, 0.0);
    assert!(app.card.is_some());
}
