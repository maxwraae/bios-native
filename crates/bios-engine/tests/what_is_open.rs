//! `what_is_open`: what the window wrote down, read through the command, and no window once it quit.

use bios_engine::Session;
use bios_project::recents::{self, Recents};
use bios_project::window::{self, Front, Selection, ShownPlasmid, ShownProject, Tab};
use bios_project::{Actor, Who};
use serde_json::json;

fn who() -> Who {
    Who { actor: Actor::Agent, source: "test".into(), session: None }
}

#[test]
fn what_the_window_shows_reads_back_and_a_closed_window_is_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let lab = dir.path().join("Lab");
    std::fs::create_dir(&lab).unwrap();
    let file = window::file_beside(&dir.path().join(recents::FILE));
    let session = Session::for_app(who()).unwrap().with_window_file(file.clone());
    assert!(!bios_engine::command("what_is_open").unwrap().needs_project());

    let none = session.execute("what_is_open", &json!({})).unwrap();
    assert_eq!(none["window_open"], false);
    assert!(none["said"].as_str().unwrap().starts_with("No Bios window is open"));
    assert_eq!(none["latest_project"], json!(null));

    let front = Front {
        project: Some(ShownProject { path: lab.clone(), name: "Lab".into() }),
        place: "edit".into(),
        plasmid: Some(ShownPlasmid { id: "c1".into(), path: "backbones/pUC19".into(), name: "pUC19".into(), length: 2686, circular: true }),
        tabs: vec![Tab { path: "backbones/pUC19".into(), name: "pUC19".into() }],
        selection: Some(Selection { start: 10, end: 20 }),
    };
    window::publish(&file, &front).unwrap();
    let open = session.execute("what_is_open", &json!({})).unwrap();
    assert_eq!(open["window_open"], true);
    assert_eq!(open["project"], json!({ "name": "Lab", "path": lab.display().to_string() }));
    assert_eq!(open["place"], "edit");
    assert_eq!(open["plasmid"]["path"], "backbones/pUC19");
    assert_eq!(open["plasmid"]["length"], 2686);
    assert_eq!(open["tabs"][0]["name"], "pUC19");
    assert_eq!(open["selection"], json!({ "start": 10, "end": 20 }));
    assert!(open["said"].as_str().unwrap().contains("pUC19 (backbones/pUC19) open in project Lab"), "{}", open["said"]);

    // The window quit; the folder opened last is named as a hint only.
    Recents::at(&dir.path().join(recents::FILE)).opened(&lab).unwrap();
    window::closed(&file).unwrap();
    let after = session.execute("what_is_open", &json!({})).unwrap();
    assert_eq!(after["window_open"], false);
    assert_eq!(after["latest_project"]["path"], lab.canonicalize().unwrap().display().to_string());
}
