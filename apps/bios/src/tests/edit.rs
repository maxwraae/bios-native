use std::path::Path;
use std::time::Duration;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use craft_dialogs::Answer;
use serde_json::json;

use crate::{App, Then, menus};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures");
    app.answered(Then::Import, Answer::Paths(vec![fixtures.join("gfp.fasta"), fixtures.join("wrap_demo.gb")]));
    (dir, app)
}

fn bases(app: &App) -> String {
    app.opened.as_ref().unwrap().construct.sequence.clone()
}

/// What the project holds for the open plasmid, read by someone else.
fn on_disk(app: &App) -> String {
    let opened = app.opened.as_ref().unwrap();
    app.session.as_ref().unwrap().project().read(&opened.id).unwrap().construct.sequence
}

fn last_change(app: &App) -> (String, String) {
    let entry = app.opened.as_ref().unwrap().history.last().unwrap();
    (entry.command.clone(), entry.summary.clone())
}

/// Another program working on the same project folder: an agent over MCP.
fn agent(dir: &tempfile::TempDir) -> Session {
    let project = Project::open(&dir.path().join("Lab")).unwrap();
    Session::new(project, Who { actor: Actor::Agent, source: "mcp".into(), session: None })
}

#[test]
fn what_is_typed_shows_at_once_and_is_saved_as_one_change_that_says_what_it_was() {
    let (_dir, mut app) = app();
    let before = bases(&app);
    app.point(10, 10, 0.0);
    app.edit(|e| e.type_text("ggat"));
    assert_eq!(bases(&app), format!("{}GGAT{}", &before[..10], &before[10..]));
    assert_eq!((app.cursor.caret, app.cursor.selection), (Some(14), None));
    // On screen and unsaved: the project still holds what it had.
    assert_eq!(on_disk(&app), before);
    assert!(app.menu_state().unsaved && app.edited.is_some());

    assert!(app.save());
    assert_eq!(on_disk(&app), bases(&app));
    assert_eq!(last_change(&app), ("write_plasmid".to_string(), "Typed 4 bases after base 10".to_string()));
    assert!(!app.menu_state().unsaved && app.edited.is_none());
    // Features after the insertion moved with their bases.
    let gfp = app.opened.as_ref().unwrap().construct.features.iter().find(|f| f.name == "GFP").unwrap().clone();
    assert_eq!((gfp.start, gfp.end), (203, 503));
}

#[test]
fn undo_takes_back_a_step_of_this_sitting_first_and_a_saved_change_after_that() {
    let (_dir, mut app) = app();
    let before = bases(&app);
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("c"));
    assert!(app.save());
    app.command(menus::UNDO);
    assert_eq!(bases(&app), before);
    assert!(app.menu_state().redo && app.menu_state().unsaved);
    app.command(menus::REDO);
    assert_eq!(bases(&app), format!("C{before}"));
    // Nothing differs from what is saved any more, so saving records nothing new.
    let entries = app.opened.as_ref().unwrap().history.len();
    assert!(app.save());
    assert_eq!(app.opened.as_ref().unwrap().history.len(), entries);

    // A plasmid opened afresh has no steps of its own: undo is the last saved change.
    let id = app.opened.as_ref().unwrap().id.clone();
    app.opened = None;
    app.open_construct(&id);
    assert!(!app.opened.as_ref().unwrap().editing.can_undo());
    app.command(menus::UNDO);
    assert_eq!(bases(&app), before);
    assert_eq!(last_change(&app).0, "restore_version");
}

#[test]
fn an_agents_change_appears_and_does_not_get_undone_by_the_persons_undo() {
    let (dir, mut app) = app();
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("c"));
    assert!(app.save());
    let version = app.opened.as_ref().unwrap().token.clone();
    agent(&dir).execute("insert_sequence", &json!({ "plasmid": "wrap_demo", "position": 0, "sequence": "TTTT", "version": version })).unwrap();
    app.refresh();
    assert!(bases(&app).starts_with("TTTTC"));
    // The step from before the agent's change is gone: it would have taken the agent's bases too.
    assert!(!app.opened.as_ref().unwrap().editing.can_undo());
}

#[test]
fn unsaved_typing_and_an_agents_change_are_kept_apart_until_the_person_decides() {
    let (dir, mut app) = app();
    let before = bases(&app);
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("c"));
    let version = app.opened.as_ref().unwrap().token.clone();
    agent(&dir).execute("insert_sequence", &json!({ "plasmid": "wrap_demo", "position": 0, "sequence": "TTTT", "version": version })).unwrap();
    app.refresh();
    // The window still shows what the person typed, and the project holds what the agent did.
    assert_eq!(bases(&app), format!("C{before}"));
    assert_eq!(on_disk(&app), format!("TTTT{before}"));
    assert!(!app.save());
    assert!(app.problem.as_deref().is_some_and(|p| p.starts_with("CONFLICT: nothing was saved.")), "{:?}", app.problem);
    assert!(app.save_refused && app.menu_state().unsaved);
    assert_eq!(on_disk(&app), format!("TTTT{before}"));

    app.command(menus::REVERT);
    assert_eq!(bases(&app), format!("TTTT{before}"));
    assert_eq!(app.problem, None);
    assert!(!app.menu_state().unsaved);
}

#[test]
fn leaving_a_plasmid_or_running_a_command_saves_what_was_typed_first() {
    let (_dir, mut app) = app();
    let (wrap, gfp) = (
        app.listing.iter().find(|l| l.name == "wrap_demo").unwrap().id.clone(),
        app.listing.iter().find(|l| l.name == "pGFP_demo").unwrap().id.clone(),
    );
    app.point(4, 8, 0.0);
    app.edit(|e| e.delete(bios_engine::editor::edit::Direction::Back));
    app.open_construct(&gfp);
    assert_eq!(app.opened.as_ref().unwrap().construct.name, "pGFP_demo");
    app.open_construct(&wrap);
    assert_eq!(last_change(&app), ("write_plasmid".to_string(), "Deleted 5..8".to_string()));
    assert_eq!(bases(&app).len(), 796);

    app.point(0, 0, 0.0);
    app.edit(|e| e.paste("ATGATGATG"));
    app.command(menus::DUPLICATE);
    // The copy was made from the plasmid with the paste in it.
    assert_eq!(app.opened.as_ref().unwrap().construct.name, "wrap_demo copy");
    assert!(bases(&app).starts_with("ATGATGATG"));
}
