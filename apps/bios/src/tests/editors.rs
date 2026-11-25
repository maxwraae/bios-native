//! Two editors in one window, or in two windows: one working copy per plasmid however many editors
//! show it, a side that loses its last tab closes, and what is asked goes to the editor and the
//! window being worked on.

use std::path::Path;
use std::time::Duration;

use bios_project::ConstructId;
use craft_dialogs::Answer;

use crate::{App, Then, menus};

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures");
    app.answered(Then::Import, Answer::Paths(vec![fixtures.join("gfp.fasta"), fixtures.join("wrap_demo.gb")]));
    (dir, app)
}

/// wrap_demo, or the other plasmid (the GFP FASTA).
fn id_of(app: &App, name: &str) -> ConstructId {
    app.listing.iter().find(|l| (l.name == "wrap_demo") == (name == "wrap_demo")).unwrap().id.clone()
}

fn front(app: &App) -> Option<ConstructId> {
    app.opened.as_ref().map(|o| o.id.clone())
}

fn on_disk(app: &App, id: &ConstructId) -> String {
    app.session.as_ref().unwrap().project().read(id).unwrap().construct.sequence
}

#[test]
fn two_editors_on_one_plasmid_edit_one_working_copy_and_save_it_once() {
    let (_dir, mut app) = app();
    let wrap = id_of(&app, "wrap_demo");
    app.open_construct(&wrap);
    let before = app.opened.as_ref().unwrap().construct.sequence.clone();
    let writes = app.opened.as_ref().unwrap().history.len();
    app.point(3, 3, 0.0);
    // ⌘\: the plasmid in front, to the side.
    app.command(menus::TO_SIDE);
    assert!(app.split.as_ref().is_some_and(|s| s.ed_right), "the new side is the one worked on");
    assert_eq!(app.ed.tabs, vec![wrap.clone()]);
    app.point(20, 20, 0.0);
    app.edit(|e| e.type_text("ggg"));
    let typed = format!("{}GGG{}", &before[..20], &before[20..]);
    // The left side shows the same working copy, unsaved, at its own caret.
    assert!(app.focus_side(false));
    assert_eq!(app.opened.as_ref().unwrap().construct.sequence, typed);
    assert_eq!(app.ed.cursor.caret, Some(3));
    assert!(app.menu_state().unsaved, "going to the other side of the same plasmid saves nothing yet");
    assert_eq!(on_disk(&app, &wrap), before);
    // Typed on this side too, then saved: one copy, one write.
    app.edit(|e| e.type_text("c"));
    assert!(app.save());
    let both = format!("{}C{}GGG{}", &before[..3], &before[3..20], &before[20..]);
    assert_eq!(on_disk(&app, &wrap), both);
    assert_eq!(app.opened.as_ref().unwrap().history.len(), writes + 1);
    assert!(app.focus_side(true));
    assert_eq!(app.opened.as_ref().unwrap().construct.sequence, both);
    assert!(!app.menu_state().unsaved);
}

#[test]
fn closing_the_last_tab_of_a_side_gives_the_window_back_to_the_other() {
    let (_dir, mut app) = app();
    let (gfp, wrap) = (id_of(&app, "gfp"), id_of(&app, "wrap_demo"));
    app.open_construct(&gfp);
    app.open_to_side(&wrap);
    assert_eq!(front(&app), Some(wrap.clone()));
    assert_eq!(app.split.as_ref().map(|s| s.other.tabs.len()), Some(2), "the left keeps both of its tabs");
    app.command(menus::CLOSE_TAB);
    assert!(app.split.is_none());
    assert_eq!(app.ed.tabs, vec![gfp.clone(), wrap.clone()]);
    assert_eq!(front(&app), Some(gfp.clone()), "the left side's plasmid in front stays in front");
    assert!(app.copies.is_empty(), "no copy is kept for an editor that is gone");
}

#[test]
fn what_is_asked_goes_to_the_side_being_worked_on_and_leaving_a_plasmid_saves_it() {
    let (_dir, mut app) = app();
    let (gfp, wrap) = (id_of(&app, "gfp"), id_of(&app, "wrap_demo"));
    app.open_construct(&gfp);
    app.open_to_side(&wrap);
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("aaaa"));
    // Going to the other side, which shows another plasmid, saves this one first.
    assert!(app.focus_side(false));
    assert!(on_disk(&app, &wrap).starts_with("AAAA"));
    assert_eq!(front(&app), Some(gfp.clone()));
    app.command(menus::FIND);
    assert!(app.ed.find.is_some());
    assert!(app.split.as_ref().is_some_and(|s| s.other.find.is_none()), "Find opened on the left only");
    app.command(menus::NEXT_TAB);
    assert_eq!(front(&app), Some(wrap.clone()), "the left side's tabs step, not the right's");
    assert_eq!(app.split.as_ref().map(|s| s.other.tabs.clone()), Some(vec![wrap.clone()]));
    app.leave_project();
    assert!(app.split.is_none() && app.copies.is_empty(), "leaving the project closes the split");
}

#[test]
fn a_second_window_edits_the_same_working_copy_and_closing_it_leaves_the_main_one() {
    let (_dir, mut app) = app();
    let wrap = id_of(&app, "wrap_demo");
    app.open_construct(&wrap);
    let before = app.opened.as_ref().unwrap().construct.sequence.clone();
    app.point(5, 5, 0.0);
    app.command(menus::NEW_WINDOW);
    let second = app.active;
    assert_ne!(second, egui::ViewportId::ROOT, "the new window is the one worked on");
    assert_eq!(app.window_ids().len(), 2);
    assert_eq!(app.ed.tabs, vec![wrap.clone()], "with its own tabs");
    assert_eq!((app.place, app.split.is_none()), (bios_ui::sidebar::Place::Edit, true));
    app.point(10, 10, 0.0);
    app.edit(|e| e.type_text("ggg"));
    // Back in the main window: the same copy, unsaved, at the main window's own caret.
    assert!(app.activate(egui::ViewportId::ROOT));
    assert_eq!(app.opened.as_ref().unwrap().construct.sequence, format!("{}GGG{}", &before[..10], &before[10..]));
    assert_eq!(app.ed.cursor.caret, Some(5));
    assert!(app.menu_state().unsaved);
    app.close_window(second);
    assert_eq!(app.window_ids(), vec![egui::ViewportId::ROOT]);
    assert_eq!(front(&app), Some(wrap.clone()));
    assert!(app.save());
    assert!(on_disk(&app, &wrap).contains("GGG"));
}

#[test]
fn commands_act_in_the_window_worked_on_and_leaving_one_saves_what_the_other_does_not_show() {
    let (_dir, mut app) = app();
    let (gfp, wrap) = (id_of(&app, "gfp"), id_of(&app, "wrap_demo"));
    app.open_construct(&gfp);
    let main_tabs = app.ed.tabs.clone();
    app.command(menus::NEW_WINDOW);
    let second = app.active;
    app.open_construct(&wrap);
    assert_eq!(app.ed.tabs, vec![gfp.clone(), wrap.clone()]);
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("tttt"));
    assert!(app.activate(egui::ViewportId::ROOT));
    assert!(on_disk(&app, &wrap).starts_with("TTTT"), "the second window's plasmid was saved on leaving it");
    assert_eq!(front(&app), Some(gfp.clone()));
    assert_eq!(app.ed.tabs, main_tabs, "the main window kept its own tabs");
    app.command(menus::FIND);
    assert!(app.ed.find.is_some());
    assert!(app.windows.iter().all(|w| w.shell.ed.find.is_none()), "Find opened in the main window only");
    // Closing the window worked on hands the main one back.
    assert!(app.activate(second));
    app.close_window(second);
    assert_eq!((app.active, app.here), (egui::ViewportId::ROOT, egui::ViewportId::ROOT));
    assert_eq!(front(&app), Some(gfp.clone()));
    // Typed into the main window while the new one is in front, as when the main one is drawn
    // and takes a key before the system brings it forward: saved with the next save all the same.
    app.command(menus::NEW_WINDOW);
    let third = app.active;
    app.open_construct(&wrap);
    app.enter(egui::ViewportId::ROOT);
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("cc"));
    app.enter(third);
    assert_eq!(front(&app), Some(wrap.clone()));
    assert!(app.save());
    let gfp_bases = on_disk(&app, &gfp);
    assert!(gfp_bases.starts_with("CC"), "{}", &gfp_bases[..10]);
    app.leave_project();
    assert!(app.windows.is_empty() && app.active == egui::ViewportId::ROOT, "leaving the project closes the other windows");
}
