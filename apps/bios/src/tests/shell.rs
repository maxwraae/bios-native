//! The shell around the editor: the sidebar and the Changes column.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bios_ui::sidebar::Place;
use craft_dialogs::Answer;

use crate::{App, Then, menus};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/bios-core/tests/fixtures").join(name)
}

fn app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("My Lab")]));
    app.answered(Then::Import, Answer::Paths(vec![fixture("wrap_demo.gb")]));
    (dir, app)
}

#[test]
fn the_toggle_puts_the_panel_away_and_going_to_another_place_brings_it_back() {
    let (_dir, mut app) = app();
    assert_eq!((app.place, app.panel_hidden), (Place::Edit, false));
    app.command(menus::TOGGLE_PANEL);
    assert!(app.panel_hidden && app.menu_state().panel_hidden);
    app.command(menus::PLACE_BUILD);
    assert_eq!((app.place, app.panel_hidden), (Place::Build, false), "going somewhere shows its panel");
    assert_eq!(app.menu_state().place, 1);
    app.command(menus::PLACE_EDIT);
    assert_eq!(app.place, Place::Edit);
}

#[test]
fn the_changes_column_is_hidden_until_asked_for() {
    let (_dir, mut app) = app();
    assert!(!app.changes_shown);
    app.command(menus::TOGGLE_CHANGES);
    assert!(app.changes_shown && app.menu_state().changes);
    app.command(menus::TOGGLE_CHANGES);
    assert!(!app.changes_shown);
}

#[test]
fn the_project_name_leaves_the_project_for_the_first_screen_after_saving() {
    let (dir, mut app) = app();
    app.point(0, 0, 0.0);
    app.edit(|e| e.type_text("ggg"));
    app.leave_project();
    assert!(app.session.is_none() && app.opened.is_none() && app.listing.is_empty());
    let saved = std::fs::read_to_string(dir.path().join("My Lab/wrap_demo.gb")).unwrap();
    assert!(saved.to_lowercase().contains("        1 ggg"), "what was typed was saved before leaving");
}

#[test]
fn the_panels_actions_go_through_the_commands() {
    use bios_ui::session_nav::Chosen;
    let (_dir, mut app) = app();
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    app.nav_chose(Chosen::NewFolder("parts".into()));
    app.nav_chose(Chosen::MoveTo { id: id.clone(), folder: "parts".into() });
    app.nav_chose(Chosen::Rename { id: id.clone(), name: "wrap v2".into() });
    app.nav_chose(Chosen::RenameFolder { from: "parts".into(), to: "backbones".into() });
    assert_eq!(app.problem, None);
    assert_eq!(crate::label(&app.listing[0].path), "backbones/wrap v2");
    app.nav_chose(Chosen::Duplicate(id.clone()));
    assert_eq!(app.opened.as_ref().map(|o| o.construct.name.as_str()), Some("wrap v2 copy"));
    app.nav_chose(Chosen::NewPlasmid);
    let made = app.opened.as_ref().map(|o| (o.construct.name.clone(), o.construct.len(), o.construct.circular, o.construct.features.len()));
    assert_eq!(made, Some(("Untitled construct".to_string(), 2686, true, 0)));
    app.nav_chose(Chosen::NewPlasmid);
    assert_eq!(app.opened.as_ref().map(|o| o.construct.name.as_str()), Some("Untitled construct 2"));
    // A section header's + makes it in that folder.
    app.nav_chose(Chosen::NewPlasmidIn("backbones".into()));
    let made = app.listing.iter().find(|l| l.name == "Untitled construct 3").map(|l| crate::label(&l.path));
    assert_eq!(made.as_deref(), Some("backbones/Untitled construct 3"));
    // The row asked in place; this is the yes.
    app.nav_chose(Chosen::Delete(id.clone()));
    assert!(app.listing.iter().all(|l| l.id != id));
    assert!(app.deleted.iter().any(|d| d.id == id));
    app.nav_chose(Chosen::Restore(id.clone()));
    assert!(app.listing.iter().any(|l| l.id == id));
    app.nav_chose(Chosen::Width { width: 900.0, done: true });
    assert_eq!(app.panel_width, 420.0);
}

#[test]
fn each_plasmid_opened_gets_a_tab_and_closing_the_front_one_brings_the_next() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("gfp.fasta"), fixture("pUC19.gb")]));
    let names = |app: &App| app.tab_names();
    let front = |app: &App| app.opened.as_ref().map(|o| o.construct.name.clone()).unwrap_or_default();
    assert_eq!(names(&app), ["wrap_demo", "pGFP_demo", "pUC19"]);
    assert_eq!(front(&app), "pUC19");
    // ⌘1 is the first tab; ⌃Tab steps round from the last to the first.
    app.command("tab.select:0");
    assert_eq!(front(&app), "wrap_demo");
    app.command(menus::PREVIOUS_TAB);
    assert_eq!(front(&app), "pUC19");
    app.command(menus::NEXT_TAB);
    assert_eq!(front(&app), "wrap_demo");
    // Opening one already open brings its tab forward rather than adding another.
    let gfp = app.listing.iter().find(|l| l.name == "pGFP_demo").map(|l| l.id.clone()).unwrap();
    app.open_construct(&gfp);
    assert_eq!(names(&app).len(), 3);
    // ⌘W closes the front tab and the one after it takes its place, or the one before.
    app.command(menus::CLOSE_TAB);
    assert_eq!((names(&app), front(&app)), (vec!["wrap_demo".to_string(), "pUC19".to_string()], "pUC19".to_string()));
    app.command(menus::CLOSE_TAB);
    assert_eq!(front(&app), "wrap_demo");
    app.command(menus::CLOSE_TAB);
    assert!(app.opened.is_none() && app.tabs.is_empty());
    // A plasmid deleted elsewhere loses its tab.
    app.open_construct(&gfp);
    app.run("delete_plasmid", serde_json::json!({ "plasmid": gfp.as_str(), "confirm": true }));
    assert!(app.tabs.is_empty());
}

#[test]
fn command_and_a_digit_bring_that_tab_forward_and_a_digit_past_the_tabs_is_left_alone() {
    let (_dir, mut app) = app();
    app.answered(Then::Import, Answer::Paths(vec![fixture("gfp.fasta")]));
    let ctx = egui::Context::default();
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
    let mut pressed = Vec::new();
    ctx.run_ui(egui::RawInput { events: vec![key(egui::Key::Num1), key(egui::Key::Num3)], ..egui::RawInput::default() }, |ui| {
        pressed = menus::pressed(ui.ctx(), app.menu_state(), &app.shortcuts());
    })
    .drop_without_applying_deltas();
    assert_eq!(pressed, ["tab.select:0"]);
}

/// One frame of the sidebar's list alone, at the window's left edge and the web's width, with these
/// things happening.
fn nav_frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Vec<bios_ui::session_nav::Chosen> {
    let mut chosen = Vec::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
    let input = egui::RawInput { events, screen_rect: Some(screen), ..egui::RawInput::default() };
    ctx.run_ui(input, |ui| {
        let model = bios_ui::session_nav::Model {
            folders: &app.folders,
            listing: &app.listing,
            open: app.opened.as_ref().map(|o| &o.id),
            found: None,
            focus_search: false,
            deleted: &app.deleted,
            folder_kinds: &app.folder_kinds,
            building: app.place == bios_ui::sidebar::Place::Build,
        };
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 900.0));
        chosen.extend(app.nav.show(ui, rect, &model, &mut app.query));
    })
    .drop_without_applying_deltas();
    chosen
}

/// A press and a release of `button` at `at`, over a few frames.
fn click(app: &mut App, ctx: &egui::Context, at: egui::Pos2, button: egui::PointerButton) -> Vec<bios_ui::session_nav::Chosen> {
    let press = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
    let mut chosen = nav_frame(app, ctx, vec![egui::Event::PointerMoved(at)]);
    chosen.extend(nav_frame(app, ctx, vec![press(true)]));
    chosen.extend(nav_frame(app, ctx, vec![press(false)]));
    chosen.extend(nav_frame(app, ctx, vec![]));
    chosen
}

#[test]
fn the_panel_draws_a_project_without_asking_for_anything() {
    let (_dir, mut app) = app();
    let ctx = egui::Context::default();
    let chosen: Vec<_> = (0..2).flat_map(|_| nav_frame(&mut app, &ctx, vec![])).collect();
    assert!(chosen.is_empty(), "{chosen:?}");
}

#[test]
fn deleting_from_a_rows_menu_asks_in_the_row_and_only_its_yes_deletes() {
    use bios_ui::session_nav::Chosen;
    let (_dir, mut app) = app();
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    let ctx = egui::Context::default();
    nav_frame(&mut app, &ctx, vec![]);
    // The one row, with no folders to put it under a section: 0 to 32.
    let row = egui::pos2(100.0, 16.0);
    assert!(click(&mut app, &ctx, row, egui::PointerButton::Secondary).is_empty());
    // The menu opens at the pointer: Open, a rule, Rename, Duplicate, Download, a rule, Delete….
    let delete = egui::pos2(140.0, 16.0 + 5.0 + 28.0 + 9.0 + 3.0 * 28.0 + 9.0 + 14.0);
    assert!(click(&mut app, &ctx, delete, egui::PointerButton::Primary).is_empty(), "Delete… only asks");
    // The row now holds Delete and Keep, side by side in its right half.
    let yes = egui::pos2(127.0, 16.0);
    assert_eq!(click(&mut app, &ctx, yes, egui::PointerButton::Primary), vec![Chosen::Delete(id)]);
}

#[test]
fn a_folder_is_a_section_its_header_shuts_and_opens_it_and_the_shut_ones_are_remembered() {
    use bios_ui::session_nav::Chosen;
    let (_dir, mut app) = app();
    let id = app.opened.as_ref().map(|o| o.id.clone()).unwrap();
    app.nav_chose(Chosen::MoveTo { id: id.clone(), folder: "parts".into() });
    let ctx = egui::Context::default();
    nav_frame(&mut app, &ctx, vec![]);
    // The section's header is the first row, 0 to 28; its plasmid sits under it.
    let header = egui::pos2(60.0, 14.0);
    assert_eq!(click(&mut app, &ctx, header, egui::PointerButton::Primary), vec![Chosen::ShutFolders(vec!["parts".into()])]);
    // Shut, the plasmid's row is gone from under it: a click there opens nothing.
    assert!(click(&mut app, &ctx, egui::pos2(60.0, 44.0), egui::PointerButton::Primary).is_empty());
    assert_eq!(click(&mut app, &ctx, header, egui::PointerButton::Primary), vec![Chosen::ShutFolders(vec![])]);
    assert_eq!(click(&mut app, &ctx, egui::pos2(60.0, 44.0), egui::PointerButton::Primary), vec![Chosen::Open { id, span: None }]);
    // Hovering the header offers a new plasmid in it, left of the chevron.
    assert_eq!(
        click(&mut app, &ctx, egui::pos2(200.0 - 4.0 - 10.0 - 12.0 - 4.0 - 10.0, 14.0), egui::PointerButton::Primary),
        vec![Chosen::NewPlasmidIn("parts".into())]
    );
}
