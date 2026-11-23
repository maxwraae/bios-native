//! ⌘P: the palette lists the project's plasmids and opens the one picked, in Edit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bios_ui::quick_open::rank;
use bios_ui::sidebar::Place;
use craft_dialogs::Answer;

use crate::{App, Then, menus};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures").join(name)
}

#[test]
fn go_to_plasmid_opens_the_one_typed_in_edit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    assert!(!app.menu_state().project || app.session.is_none());
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb"), fixture("pUC19.gb")]));
    app.command(menus::PLACE_BUILD);
    app.command(menus::QUICK_OPEN);
    assert!(app.quick_open.is_some());
    let entries = app.palette_entries();
    let hit = rank(&entries, "puc")[0];
    app.quick_open = None;
    app.palette_pick(hit);
    assert_eq!(app.place, Place::Edit);
    assert!(entries[hit].name.starts_with("pUC"));
    assert_eq!(app.opened.as_ref().map(|o| o.construct.name.clone()), Some(entries[hit].name.clone()));
}
