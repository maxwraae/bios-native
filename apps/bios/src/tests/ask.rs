use std::path::Path;
use std::time::Duration;

use bios_ui::ask::{Ask, What};
use craft_dialogs::Answer;

use crate::{App, Then, menus};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    (dir, app)
}

#[test]
fn the_sheet_opens_only_on_a_selection_and_reads_the_way_it_was_dragged() {
    let (_dir, mut app) = app();
    app.command(menus::NEW_FEATURE);
    assert_eq!(app.ed.ask, None);
    // Dragged from gap 340 back to gap 300: bases 301 to 340, made right to left.
    app.point(340, 300, 0.0);
    assert_eq!(app.ed.cursor.selection, Some((300, 339)));
    // A primer is answered on the pill (SelectionReadout.tsx `PrimerInline`), a feature in the sheet.
    app.command(menus::NEW_PRIMER);
    assert_eq!(app.ed.primer_ask.as_ref().map(|a| (a.span, a.forward)), Some(((300, 339), false)));
    assert_eq!(app.ed.ask, None);
    app.point(300, 340, 0.0);
    app.command(menus::NEW_FEATURE);
    assert_eq!(app.ed.ask.as_ref().map(|a| (a.what, a.forward)), Some((What::Feature, true)));
}

#[test]
fn landing_the_sheet_adds_the_feature_through_the_command_and_leaves_a_caret() {
    let (_dir, mut app) = app();
    app.point(300, 340, 0.0);
    let mut ask = Ask::new(What::Feature, false);
    ask.name = "spacer".into();
    ask.kind = Some(6);
    app.land(&ask);
    assert_eq!(app.problem, None);
    let opened = app.opened.as_ref().unwrap();
    let made = opened.construct.features.iter().find(|f| f.name == "spacer").unwrap();
    assert_eq!((made.start, made.end, made.kind.as_str()), (300, 339, "misc_feature"));
    assert_eq!(made.strand, bios_core::Strand::Reverse);
    assert_eq!(opened.history.last().map(|e| e.command.as_str()), Some("add_feature"));
    // The caret parks at the footprint's end, as the web's `finish` does (`collapseSelectionTo(sel.end)`).
    assert_eq!((app.ed.cursor.caret, app.ed.cursor.selection), (Some(339), None));
}

#[test]
fn landing_a_primer_adds_a_primer() {
    let (_dir, mut app) = app();
    app.point(200, 222, 0.0);
    let mut ask = Ask::new(What::Primer, true);
    ask.name = "gfp-F".into();
    app.land(&ask);
    assert_eq!(app.problem, None);
    let opened = app.opened.as_ref().unwrap();
    let made = opened.construct.features.iter().find(|f| f.name == "gfp-F").unwrap();
    assert_eq!((made.start, made.end, made.kind.as_str()), (200, 221, "primer_bind"));
}
