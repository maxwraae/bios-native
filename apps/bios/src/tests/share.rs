//! Sharing from the window: two people's windows on two project folders, one pretend server.

use std::path::Path;
use std::time::Duration;

use bios_engine::{Server, Session};
use bios_project::{Actor, Project, Who};
use bios_sync::{FakeGateway, Role};

use crate::{App, Asked, menus};

/// A window on a new project in `dir/name`, signed in to the lab's server as `gateway`'s account.
fn window(dir: &tempfile::TempDir, name: &str, gateway: FakeGateway) -> App {
    let project = Project::create(&dir.path().join(name), "Lab").unwrap();
    let who = Who { actor: Actor::Human, source: "window".into(), session: None };
    let mut app = App::new(None, Duration::ZERO);
    app.session = Some(Session::new(project, who).with_server(Server { name: "lab server".into(), gateway: Box::new(gateway) }));
    app.refresh();
    app
}

fn bases(app: &App) -> String {
    app.opened.as_ref().unwrap().construct.sequence.clone()
}

/// Ana's window with wrap_demo shared as "Splicing", and Ben's window joined to it.
fn two_windows() -> (tempfile::TempDir, App, App) {
    let dir = tempfile::tempdir().unwrap();
    let server = FakeGateway::new("ana");
    let mut ana = window(&dir, "ana", server.as_account("ana"));
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures/wrap_demo.gb");
    ana.answered(crate::Then::Import, craft_dialogs::Answer::Paths(vec![file]));
    assert!(ana.menu_state().server && !ana.menu_state().shared);

    ana.command(menus::SHARE);
    let (what, prompt) = ana.prompt.clone().unwrap();
    assert_eq!((what.clone(), prompt.text.as_str()), (Asked::ShareAs, "Lab"));
    ana.asked(&what, "Splicing");
    assert_eq!(ana.problem, None);
    assert_eq!(ana.shared_on.as_deref(), Some("lab server"));

    let remote = bios_sync::connection(ana.session.as_ref().unwrap().project()).unwrap().unwrap().project;
    server.share(&remote, "ben", Role::Editor);
    let mut ben = window(&dir, "ben", server.as_account("ben"));
    ben.command(menus::JOIN);
    let (what, _) = ben.prompt.clone().unwrap();
    ben.asked(&what, "Splicing");
    assert_eq!(ben.problem, None);
    assert_eq!(ben.listing.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["wrap_demo"]);
    let id = ben.listing[0].id.clone();
    ben.open_construct(&id);
    (dir, ana, ben)
}

#[test]
fn what_one_person_types_reaches_the_other_after_both_sync() {
    let (_dir, mut ana, mut ben) = two_windows();
    ben.point(0, 0, 0.0);
    ben.edit(|e| e.type_text("gggg"));
    // Sync Now saves what was typed first, then sends it.
    ben.command(menus::SYNC);
    assert_eq!(ben.problem, None);
    assert_eq!(ben.notice.as_deref(), Some("wrap_demo was sent to lab server."));

    ana.command(menus::SYNC);
    assert!(bases(&ana).starts_with("GGGGACGT"));
    assert_eq!(ana.opened.as_ref().unwrap().history.last().map(|e| e.actor), Some(Actor::Import));
    ana.command(menus::SYNC);
    assert_eq!(ana.notice.as_deref(), Some("Everything is in step with lab server."));
}

#[test]
fn a_conflict_is_said_as_a_problem_and_settled_from_the_menu() {
    let (_dir, mut ana, mut ben) = two_windows();
    ana.point(0, 0, 0.0);
    ana.edit(|e| e.type_text("aaaa"));
    ben.point(0, 0, 0.0);
    ben.edit(|e| e.type_text("cccc"));
    ana.command(menus::SYNC);
    ben.command(menus::SYNC);
    assert!(ben.problem.is_some(), "a conflict should be said where problems are");
    assert!(ben.menu_state().conflict);
    assert!(bases(&ben).starts_with("CCCC"), "nothing is taken from him without his say");

    ben.command(menus::KEEP_MINE);
    assert!(!ben.menu_state().conflict);
    ana.command(menus::SYNC);
    assert!(bases(&ana).starts_with("CCCC"));
    // Ana's own version is still in her history.
    assert!(ana.opened.as_ref().unwrap().history.iter().any(|e| e.summary == "Typed 4 bases after base 0"));
}
