//! Settings: a shortcut changed in Keyboard is remembered and is what the menus fire.

use std::time::Duration;

use bios_ui::settings::{Outcome, Section};
use craft_keymap::{Chord, Platform};
use craft_settings::{Location, SettingsStore};

use crate::prefs::Prefs;
use crate::{App, menus};

fn app() -> App {
    let mut app = App::new(None, Duration::ZERO);
    app.prefs = Some(SettingsStore::<Prefs>::open(Location::Memory).0);
    app
}

#[test]
fn settings_opens_from_the_menu_and_about_is_its_last_section() {
    let mut app = app();
    app.command(menus::SETTINGS);
    assert_eq!(app.settings.as_ref().map(|s| s.section), Some(Section::Appearance));
    app.settings_did(Outcome::Close, Platform::Mac);
    assert!(app.settings.is_none());
    app.command(menus::ABOUT);
    assert_eq!(app.settings.as_ref().map(|s| s.section), Some(Section::About));
}

#[test]
fn a_rebound_shortcut_is_remembered_shown_and_put_in_the_menus() {
    let mut app = app();
    let cmd_alt_f = Chord::parse("Cmd+Alt+F").unwrap();
    let modifiers = egui::Modifiers { alt: true, command: true, mac_cmd: true, ..Default::default() };
    app.settings_did(Outcome::Bind(menus::FIND, egui::Key::F, modifiers), Platform::Mac);
    assert_eq!(app.shortcuts().chord(menus::FIND), Some(cmd_alt_f));
    assert_eq!(app.prefs.as_ref().unwrap().get().keys.get(menus::FIND).map(String::as_str), Some("Cmd+Alt+F"));
    let row = app.key_rows(Platform::Mac).into_iter().find(|r| r.command == menus::FIND).unwrap();
    assert!(!row.is_default);
    let bar = menus::bar(app.menu_state(), &[], &[], &app.shortcuts());
    let find = bar.find(&craft_menu::CommandRef::new(menus::FIND)).map(|item| item.shortcut);
    assert_eq!(find, Some(Some(cmd_alt_f)));
    // Unbound, then back to the default, then everything back.
    app.settings_did(Outcome::Unbind(menus::SAVE), Platform::Mac);
    assert_eq!(app.shortcuts().chord(menus::SAVE), None);
    app.settings_did(Outcome::Reset(menus::SAVE), Platform::Mac);
    assert!(app.shortcuts().chord(menus::SAVE).is_some());
    app.settings_did(Outcome::ResetAll, Platform::Mac);
    assert_eq!(app.shortcuts().changed(), 0);
}

#[test]
fn two_commands_on_one_chord_are_both_kept_and_the_row_says_so() {
    let mut app = app();
    let modifiers = egui::Modifiers { command: true, mac_cmd: true, ..Default::default() };
    app.settings_did(Outcome::Bind(menus::FIND_PROJECT, egui::Key::F, modifiers), Platform::Mac);
    let rows = app.key_rows(Platform::Mac);
    let row = rows.iter().find(|r| r.command == menus::FIND_PROJECT).unwrap();
    assert_eq!(row.clash.as_deref(), Some("Also find in this construct. The one that can act right now wins."));
}

#[test]
fn the_documentation_opens_from_help_at_its_contents() {
    let mut app = app();
    app.command(menus::DOCS);
    assert_eq!(app.docs.as_ref().map(|d| d.slug.clone()), Some(None));
    assert!(bios_ui::docs::Docs::open(Some("no-such-page")).slug.is_none());
    assert_eq!(bios_ui::docs::Docs::open(Some("feature")).slug.as_deref(), Some("feature"));
}
