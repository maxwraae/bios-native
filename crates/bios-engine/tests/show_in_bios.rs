//! `show_in_bios`: the request the window takes, what it names, and what is said when no window
//! takes it.

use bios_engine::Session;
use bios_project::window::{self, Front, Selection};
use bios_project::{Actor, Project, Who, recents};
use serde_json::json;

fn who() -> Who {
    Who { actor: Actor::Agent, source: "test".into(), session: None }
}

#[test]
fn a_window_takes_the_request_and_the_answer_says_it_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let lab = dir.path().join("Lab");
    let file = window::file_beside(&dir.path().join(recents::FILE));
    let session = Session::open(Project::create(&lab, "Lab").unwrap(), who()).with_window_file(file.clone());
    assert!(bios_engine::command("show_in_bios").unwrap().needs_project());
    let made = session.execute("create_plasmid", &json!({ "name": "pTiny", "folder": "backbones", "sequence": "ACGTACGTACGTACGTACGT" })).unwrap();
    let id = made["id"].as_str().unwrap().to_string();

    // A window running in this process, which takes what it is asked for a moment later.
    window::publish(&file, &Front::default()).unwrap();
    let taker = {
        let file = file.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Some(show) = window::take_show(&file) {
                    return Some(show);
                }
            }
            None
        })
    };
    let shown = session.execute("show_in_bios", &json!({ "plasmid": "backbones/pTiny", "start": 18, "end": 3 })).unwrap();
    let taken = taker.join().unwrap().unwrap();
    assert_eq!(shown["shown"], true, "{shown}");
    assert_eq!(shown["plasmid"], "backbones/pTiny");
    assert!(shown["said"].as_str().unwrap().starts_with("Bios now shows backbones/pTiny in project Lab"), "{}", shown["said"]);
    assert_eq!(taken.project, lab);
    assert_eq!(taken.plasmid.as_deref(), Some(id.as_str()));
    assert_eq!(taken.selection, Some(Selection { start: 18, end: 3 }));
}

#[test]
fn bad_bases_are_refused_and_with_no_window_the_request_waits() {
    let dir = tempfile::tempdir().unwrap();
    let lab = dir.path().join("Lab");
    let file = window::file_beside(&dir.path().join(recents::FILE));
    let session = Session::open(Project::create(&lab, "Lab").unwrap(), who()).with_window_file(file.clone());
    session.execute("create_plasmid", &json!({ "name": "pTiny", "sequence": "ACGTACGTAC" })).unwrap();

    let past = session.execute("show_in_bios", &json!({ "plasmid": "pTiny", "start": 1, "end": 11 })).unwrap_err();
    assert!(past.0.contains("1..10"), "{}", past.0);
    let half = session.execute("show_in_bios", &json!({ "plasmid": "pTiny", "start": 1 })).unwrap_err();
    assert!(half.0.contains("both"), "{}", half.0);
    let loose = session.execute("show_in_bios", &json!({ "start": 1, "end": 2 })).unwrap_err();
    assert!(loose.0.contains("plasmid"), "{}", loose.0);
    assert!(!window::show_file(&file).exists());

    // No window, and none to start from a test: the request is left for the next window.
    let project = session.execute("show_in_bios", &json!({})).unwrap();
    assert_eq!(project["shown"], false);
    assert_eq!(project["plasmid"], json!(null));
    assert!(window::take_show(&file).is_some_and(|s| s.project == lab && s.plasmid.is_none()));
}
