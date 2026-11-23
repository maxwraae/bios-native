use std::path::Path;
use std::time::Duration;

use bios_ui::rows::Gesture;
use craft_dialogs::Answer;

use crate::{App, Then};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    (dir, app)
}

/// The place of a feature in the open plasmid's list, and a click on its bar.
fn click(app: &mut App, name: &str) -> usize {
    let (index, start, end) = {
        let features = &app.opened.as_ref().unwrap().construct.features;
        let index = features.iter().position(|f| f.name == name).unwrap();
        (index, features[index].start, features[index].end)
    };
    app.gesture(Gesture::Span { feature: index, start, end }, 0.0);
    index
}

#[test]
fn a_click_on_a_feature_selects_it_and_opens_its_panel() {
    let (_dir, mut app) = app();
    let index = click(&mut app, "GFP");
    assert_eq!(app.cursor.selection, Some((199, 499)));
    let card = app.card.clone().unwrap();
    assert_eq!((card.feature, card.was.start, card.was.end), (index, 200, 500));
    assert_eq!((card.was.name.as_str(), card.was.forward, card.type_word.as_str()), ("GFP", true, "CDS"));
    // As in the web app, the panel stays open while the person works in the bases.
    app.gesture(Gesture::Press { gap: 10, shift: false }, 0.0);
    assert!(app.card.is_some());
    // The feature through the origin is one feature with one panel.
    click(&mut app, "AmpR");
    assert_eq!(app.card.as_ref().map(|c| (c.was.start, c.was.end)), Some((750, 100)));
}

#[test]
fn leaving_a_field_writes_only_what_changed_and_the_panel_stays_on_the_new_record() {
    let (_dir, mut app) = app();
    let index = click(&mut app, "MCS");
    let mut card = app.card.clone().unwrap();
    card.edit.name = "polylinker ".into();
    card.edit.purpose = "where inserts go".into();
    card.edit.category = "linker".into();
    app.commit_card(&card);
    assert_eq!(app.problem, None);
    let opened = app.opened.as_ref().unwrap();
    let feature = &opened.construct.features[index];
    // The kind is Bios's own fact; the GenBank type is left as the file has it (editFeature.ts).
    assert_eq!((feature.name.as_str(), feature.kind.as_str(), feature.strand), ("polylinker", "misc_feature", bios_core::Strand::Forward));
    assert_eq!(bios_engine::bios_note(feature, "category"), Some("linker"));
    assert_eq!(bios_engine::bios_note(feature, "use"), Some("where inserts go"));
    assert_eq!(opened.history.last().map(|e| e.command.as_str()), Some("update_feature"));
    let after = app.card.clone().unwrap();
    assert_eq!((after.was.name.as_str(), after.was.category.as_str(), after.was.purpose.as_str()), ("polylinker", "linker", "where inserts go"));
    let entries = opened.history.len();
    // A field left with nothing changed is not an edit.
    let mut same = app.card.clone().unwrap();
    same.edit = same.was.clone();
    app.commit_card(&same);
    assert_eq!(app.opened.as_ref().unwrap().history.len(), entries);
}

#[test]
fn a_new_start_and_end_move_the_feature() {
    let (_dir, mut app) = app();
    let index = click(&mut app, "MCS");
    let mut card = app.card.clone().unwrap();
    card.edit.start = 590;
    card.edit.end = 9000;
    app.commit_card(&card);
    assert_eq!(app.problem, None);
    let feature = &app.opened.as_ref().unwrap().construct.features[index];
    assert_eq!((feature.start, feature.end), (589, 799), "an end past the last base is kept to the last base");
    assert_eq!(app.card.as_ref().map(|c| (c.was.start, c.was.end)), Some((590, 800)));
}

#[test]
fn a_primer_opens_the_same_panel_and_turns_round() {
    let (_dir, mut app) = app();
    app.point(200, 222, 0.0);
    let mut ask = bios_ui::ask::Ask::new(bios_ui::ask::What::Primer, true);
    ask.name = "gfp-F".into();
    app.land(&ask);
    let index = click(&mut app, "gfp-F");
    let mut card = app.card.clone().unwrap();
    assert_eq!(card.type_word, "primer bind");
    card.edit.name = "gfp-R".into();
    card.edit.forward = false;
    app.commit_card(&card);
    assert_eq!(app.problem, None);
    let primer = app.opened.as_ref().unwrap().construct.features[index].clone();
    assert_eq!((primer.name.as_str(), primer.kind.as_str(), primer.strand), ("gfp-R", "primer_bind", bios_core::Strand::Reverse));
}

#[test]
fn opening_another_panel_lands_what_was_typed_in_the_first() {
    let (_dir, mut app) = app();
    let index = click(&mut app, "MCS");
    if let Some(card) = app.card.as_mut() {
        card.edit.purpose = "where inserts go".into();
    }
    click(&mut app, "GFP");
    let feature = &app.opened.as_ref().unwrap().construct.features[index];
    assert_eq!(bios_engine::bios_note(feature, "use"), Some("where inserts go"));
    assert_eq!(app.card.as_ref().map(|c| c.was.name.as_str()), Some("GFP"));
}

#[test]
fn typing_puts_the_panel_away_because_the_feature_under_it_may_no_longer_be_the_same() {
    let (_dir, mut app) = app();
    click(&mut app, "MCS");
    assert!(app.card.is_some());
    app.edit(|e| e.type_text("a"));
    assert_eq!(app.card, None);
}
