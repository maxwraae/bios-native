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

#[test]
fn what_is_in_front_is_written_down_for_an_agent_as_it_changes_and_closed_on_quit() {
    use bios_project::recents::{self, Recents};
    use bios_project::window;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lab");
    let file = window::file_beside(&dir.path().join(recents::FILE));
    let mut app = App::new(None, Duration::ZERO);
    app.recents = Recents::at(&dir.path().join(recents::FILE));
    app.publish_front();
    let front = window::live(&file).unwrap().front;
    assert_eq!((front.place.as_str(), front.project), ("projects", None));

    app.answered(Then::NewProject, Answer::Paths(vec![root.clone()]));
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb"), fixture("pUC19.gb")]));
    let puc = app.listing.iter().find(|l| l.name == "pUC19").unwrap().id.clone();
    app.open_construct(&puc);
    app.publish_front();
    let front = window::live(&file).unwrap().front;
    assert_eq!(front.project.as_ref().map(|p| p.name.as_str()), Some("Lab"));
    assert_eq!(front.place, "edit");
    let plasmid = front.plasmid.unwrap();
    assert_eq!((plasmid.path.as_str(), plasmid.name.as_str(), plasmid.length, plasmid.circular), ("pUC19", "pUC19", 2686, true));
    assert_eq!(front.tabs.len(), 2);
    assert_eq!(front.selection, None);

    // Selecting bases 10..20 as a person counts them; an unchanged frame writes nothing.
    app.opened.as_mut().unwrap().editing.select(9, 19);
    app.publish_front();
    let written = window::read(&file).unwrap();
    assert_eq!(written.front.selection, Some(window::Selection { start: 10, end: 20 }));
    std::fs::write(&file, "untouched").unwrap();
    app.publish_front();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "untouched", "nothing changed, so nothing was written");
    window::publish(&file, &app.front()).unwrap();

    app.unpublish_front();
    assert!(window::read(&file).is_some_and(|r| !r.open));
    assert_eq!(window::live(&file), None);
}
