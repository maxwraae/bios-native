use std::path::Path;
use std::time::Duration;

use bios_ui::row_menu::Action;
use craft_dialogs::Answer;
use egui::pos2;

use crate::{App, Then};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    (dir, app)
}

#[test]
fn copy_as_fasta_names_the_plasmid_and_the_bases() {
    let (_dir, mut app) = app();
    app.point(71, 82, 0.0);
    app.menu_action(Action::CopyFasta, pos2(0.0, 0.0));
    assert_eq!(app.to_clipboard.as_deref(), Some(">wrap_demo 72-82\nCGTACGTAGAC\n"));
    app.menu_action(Action::CopyProtein, pos2(0.0, 0.0));
    assert_eq!(app.to_clipboard.as_deref(), Some("RT*"));
}

#[test]
fn a_new_construct_from_the_selection_is_made_through_create_plasmid_and_opened() {
    let (_dir, mut app) = app();
    app.point(199, 500, 0.0);
    app.menu_action(Action::NewConstruct, pos2(0.0, 0.0));
    assert_eq!(app.problem, None);
    let opened = app.opened.as_ref().unwrap();
    assert_eq!((opened.construct.name.as_str(), opened.construct.len(), opened.construct.circular), ("wrap_demo 200–500", 301, false));
    assert!(opened.construct.features.iter().any(|f| f.name == "GFP"), "the feature inside the range comes along");
}

#[test]
fn delete_takes_the_bases_and_find_elsewhere_opens_find_with_them() {
    let (_dir, mut app) = app();
    app.point(71, 82, 0.0);
    app.menu_action(Action::Find, pos2(0.0, 0.0));
    assert_eq!(app.ed.find.as_ref().map(|(bar, ..)| bar.query.as_str()), Some("CGTACGTAGAC"));
    app.point(10, 20, 0.0);
    app.menu_action(Action::Delete, pos2(0.0, 0.0));
    assert_eq!(app.opened.as_ref().unwrap().construct.len(), 790);
}
