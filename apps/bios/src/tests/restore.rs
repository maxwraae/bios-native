//! Coming back to a project puts its tabs back; quitting with nothing unsaved just quits.

use std::path::{Path, PathBuf};
use std::time::Duration;

use craft_dialogs::Answer;
use craft_settings::{Location, SettingsStore};
use serde_json::json;

use crate::prefs::Prefs;
use crate::{App, Then};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures").join(name)
}

#[test]
fn a_projects_tabs_and_the_one_in_front_come_back_and_a_plasmid_gone_since_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lab");
    let mut app = App::new(None, Duration::ZERO);
    app.prefs = Some(SettingsStore::<Prefs>::open(Location::Memory).0);
    app.answered(Then::NewProject, Answer::Paths(vec![root.clone()]));
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb"), fixture("pUC19.gb")]));
    app.new_plasmid("");
    assert_eq!(app.tabs.len(), 3);
    let (first, second, third) = (app.tabs[0].clone(), app.tabs[1].clone(), app.tabs[2].clone());
    app.open_construct(&second);
    app.remember_tabs();
    app.leave_project();
    assert!(app.tabs.is_empty());

    app.open_project(&root);
    assert_eq!(app.tabs, [first.clone(), second.clone(), third.clone()]);
    assert_eq!(app.opened.as_ref().map(|o| o.id.clone()), Some(second.clone()), "the one in front is in front again");

    // The one in front is deleted elsewhere; the others still come back, the last of them in front.
    app.run("delete_plasmid", json!({ "plasmid": second.as_str(), "confirm": true }));
    app.remember_tabs();
    app.leave_project();
    app.open_project(&root);
    assert_eq!(app.tabs, [first, third.clone()]);
    assert_eq!(app.opened.as_ref().map(|o| o.id.clone()), Some(third));
}
