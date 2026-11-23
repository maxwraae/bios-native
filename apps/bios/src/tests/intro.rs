//! How a place works: its sheet opens by itself once, and on ⌘/ after that.

use std::time::Duration;

use bios_ui::intro::Welcomed;
use bios_ui::sidebar::Place;
use craft_dialogs::Answer;
use craft_settings::{Location, SettingsStore};

use crate::prefs::Prefs;
use crate::{App, Then, menus};

#[test]
fn a_places_sheet_opens_by_itself_the_first_time_only_and_on_the_key_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(None, Duration::ZERO);
    app.prefs = Some(SettingsStore::<Prefs>::open(Location::Memory).0);
    app.answered(Then::NewProject, Answer::Paths(vec![dir.path().join("Lab")]));
    app.command(menus::INTRO);
    assert_eq!(app.intro, None, "Edit has no sheet");
    app.command(menus::PLACE_BUILD);
    assert_eq!(app.intro, Some(Place::Build), "first visit");
    app.command(menus::INTRO);
    assert_eq!(app.intro, None, "the key shuts it");
    app.command(menus::PLACE_EDIT);
    app.command(menus::PLACE_BUILD);
    assert_eq!(app.intro, None, "seen once, it waits to be asked");
    app.command(menus::INTRO);
    assert_eq!(app.intro, Some(Place::Build));
    app.command(menus::PLACE_VERIFY);
    assert_eq!(app.intro, Some(Place::Verify), "Verify has its own first time");
    assert_eq!(app.prefs.as_ref().unwrap().get().intro_seen, ["build", "verify"]);
}

fn first_run() -> App {
    let mut app = App::new(None, Duration::ZERO);
    app.prefs = Some(SettingsStore::<Prefs>::open(Location::Memory).0);
    app
}

#[test]
fn the_welcome_sheet_opens_on_the_first_run_and_not_again_once_closed() {
    let mut app = first_run();
    app.welcome_arrive();
    assert!(app.welcome.is_some(), "first run");
    assert!(app.prefs.as_ref().unwrap().get().intro_seen.is_empty(), "opening alone does not mark it seen");
    app.welcomed(Welcomed::Close);
    assert!(app.welcome.is_none());
    assert_eq!(app.prefs.as_ref().unwrap().get().intro_seen, ["welcome"]);
    app.welcome_arrive();
    assert!(app.welcome.is_none(), "closed once, it waits to be asked");
}

#[test]
fn a_picture_or_a_window_without_settings_does_not_open_the_welcome_sheet_by_itself() {
    let mut app = App::new(Some("out.png".into()), Duration::ZERO);
    app.prefs = Some(SettingsStore::<Prefs>::open(Location::Memory).0);
    app.welcome_arrive();
    assert!(app.welcome.is_none(), "a picture");
    let mut app = App::new(None, Duration::ZERO);
    app.welcome_arrive();
    assert!(app.welcome.is_none(), "no settings");
}

#[test]
fn the_menu_and_the_key_on_the_projects_page_reopen_the_welcome_sheet() {
    let mut app = first_run();
    app.welcome_arrive();
    app.welcomed(Welcomed::Close);
    app.command(menus::WELCOME);
    assert!(app.welcome.is_some(), "Help > How Bios Works");
    app.command(menus::WELCOME);
    assert!(app.welcome.is_none());
    app.command(menus::INTRO);
    assert!(app.welcome.is_some(), "⌘/ on the projects page");
    app.command(menus::INTRO);
    assert!(app.welcome.is_none());
    assert_eq!(app.prefs.as_ref().unwrap().get().intro_seen, ["welcome"], "seen once is remembered once");
}

#[test]
fn the_welcome_sheet_copies_the_connect_line_and_its_tiles_close_it() {
    let mut app = first_run();
    app.welcome_arrive();
    app.welcomed(Welcomed::Copy);
    let line = app.to_clipboard.take().unwrap();
    assert!(line.starts_with("claude mcp add --scope user bios -- \"") && line.ends_with("\" mcp"), "{line}");
    assert_eq!(line, crate::connect_agent().0, "one line for the sheet and Help > Connect an Agent");
    assert_eq!(app.welcome.map(|w| w.copied), Some(true));
    app.welcomed(Welcomed::OpenFolder);
    assert!(app.welcome.is_none(), "Open folder shuts it and asks for a folder");
    assert_eq!(app.prefs.as_ref().unwrap().get().intro_seen, ["welcome"]);
}
