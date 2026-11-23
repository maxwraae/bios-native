use std::path::Path;
use std::time::Duration;

use craft_dialogs::Answer;
use serde_json::json;

use crate::{App, Then};

fn with_puc19() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/pUC19.gb");
    app.answered(Then::Import, Answer::Paths(vec![file]));
    (dir, app)
}

#[test]
fn a_binder_is_held_against_the_plasmid_on_screen_and_never_reaches_the_file() {
    let (_dir, mut app) = with_puc19();
    let id = app.opened.as_ref().unwrap().id.clone();
    let token = app.opened.as_ref().unwrap().token.clone();
    app.add_binder_from_selection(159, 180);
    let c = app.opened.as_ref().unwrap().construct.clone();
    app.oligos.refresh(Some(&id), &c, app.revision);
    let sites = app.oligos.sites().to_vec();
    assert_eq!(sites.len(), 1);
    assert_eq!((sites[0].site.start, sites[0].site.end, sites[0].site.forward, sites[0].on_target), (159, 180, false, true));
    assert_eq!(app.opened.as_ref().unwrap().token, token, "holding a binder writes nothing");
    let (index, all) = app.oligos.binder_at(&id, 0).unwrap();
    assert_eq!((index, all.len()), (0, 1));
    app.remove_binder(0);
    app.oligos.refresh(Some(&id), &c, app.revision);
    assert!(app.oligos.sites().is_empty());
    // A selection through the origin makes none, as on the web.
    app.add_binder_from_selection(180, 159);
    assert_eq!(app.oligos.binders_of(&id).count(), 0);
}

#[test]
fn dragging_a_primers_end_resizes_it_live_and_writes_it_once_when_let_go() {
    let (_dir, mut app) = with_puc19();
    let (id, token) = {
        let o = app.opened.as_ref().unwrap();
        (o.id.clone(), o.token.clone())
    };
    app.run("add_primer", json!({ "plasmid": id.as_str(), "version": token, "name": "fwd", "start": 301, "end": 320, "strand": "forward" }));
    let index = app.opened.as_ref().unwrap().construct.features.iter().position(|f| f.name == "fwd").unwrap();
    let history = app.opened.as_ref().unwrap().history.len();

    app.begin_primer_drag(index, false);
    assert!(app.drag_primer(326));
    assert!(app.drag_primer(330));
    let live = &app.opened.as_ref().unwrap().construct.features[index];
    assert_eq!((live.start, live.end), (300, 329), "drawn at its new length while held");
    assert_eq!(app.opened.as_ref().unwrap().history.len(), history, "nothing written while held");

    // The button comes up: a context with no button down.
    app.primer_drag_frame(&egui::Context::default());
    let o = app.opened.as_ref().unwrap();
    let f = o.construct.features.iter().find(|f| f.name == "fwd").unwrap();
    assert_eq!((f.start, f.end), (300, 329));
    assert_eq!(o.history.len(), history + 1, "one step in the history");
    assert!(!app.drag_primer(340), "let go, nothing is held");
}
