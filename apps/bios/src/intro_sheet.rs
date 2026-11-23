//! How a place works: its sheet opens by itself the first time Build or Verify is visited, and on
//! ⌘/ or the ? after that (web: intro/ModeIntro.tsx, intro/seen.ts). Which sheets have been seen
//! is remembered with the window's settings.
//!
//! The welcome sheet opens by itself on the first launch that has settings to remember it in, and
//! from Help > How Bios Works, or ⌘/ on the projects page, after that. Closing it is what marks it
//! seen, so a launch that ended before it was read shows it again.

use bios_ui::intro::{self, Sheet, Welcomed};
use bios_ui::sidebar::Place;
use egui::Rect;

use crate::{App, menus};

fn sheet_of(place: Place) -> Option<&'static Sheet> {
    match place {
        Place::Build => Some(&intro::BUILD),
        Place::Verify => Some(&intro::VERIFY),
        Place::Edit => None,
    }
}

/// The welcome sheet's name among the sheets seen.
const WELCOME_KEY: &str = "welcome";

/// The welcome sheet, while it is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Welcome {
    /// Copy was pressed, so its button says Copied.
    pub copied: bool,
}

fn seen_key(place: Place) -> &'static str {
    match place {
        Place::Edit => "edit",
        Place::Build => "build",
        Place::Verify => "verify",
    }
}

impl App {
    /// A place was entered: its sheet opens if it has never been seen (`auto && !hasSeenIntro`).
    /// A window without remembered settings (a test, a picture) counts every sheet as seen, as
    /// the web does without storage.
    pub(crate) fn intro_arrive(&mut self, place: Place) {
        self.intro = None;
        if sheet_of(place).is_none() || self.shot.is_some() {
            return;
        }
        let Some(store) = &mut self.prefs else {
            return;
        };
        let key = seen_key(place).to_string();
        if !store.get().intro_seen.contains(&key) {
            store.update(|prefs| prefs.intro_seen.push(key));
            self.intro = Some(place);
        }
    }

    /// ⌘/ and the ?: the sheet of the place you are in, open or shut. On the projects page, the
    /// welcome sheet.
    pub(crate) fn intro_toggle(&mut self) {
        if self.welcome.is_some() {
            self.welcome_close();
        } else if self.intro.is_some() {
            self.intro = None;
        } else if self.session.is_none() {
            self.welcome_open();
        } else if sheet_of(self.place).is_some() {
            self.intro = Some(self.place);
        }
    }

    /// The window has started: the welcome sheet opens if it was never closed. A window without
    /// remembered settings, or one taking a picture, does not show it by itself.
    pub(crate) fn welcome_arrive(&mut self) {
        if self.shot.is_some() || self.prefs.as_ref().is_none_or(|store| store.get().intro_seen.iter().any(|k| k == WELCOME_KEY)) {
            return;
        }
        self.welcome_open();
    }

    /// Help > How Bios Works: the welcome sheet, open or shut.
    pub(crate) fn welcome_toggle(&mut self) {
        if self.welcome.is_some() {
            self.welcome_close();
        } else {
            self.welcome_open();
        }
    }

    pub(crate) fn welcome_open(&mut self) {
        self.intro = None;
        self.welcome = Some(Welcome::default());
    }

    /// Shuts the welcome sheet and remembers it was seen.
    fn welcome_close(&mut self) {
        self.welcome = None;
        if let Some(store) = &mut self.prefs
            && !store.get().intro_seen.iter().any(|k| k == WELCOME_KEY)
        {
            store.update(|prefs| prefs.intro_seen.push(WELCOME_KEY.to_string()));
        }
    }

    /// Does what was pressed on the welcome sheet. New project and Open folder shut it and do what
    /// the page's tiles do.
    pub(crate) fn welcomed(&mut self, what: Welcomed) {
        match what {
            Welcomed::Close => self.welcome_close(),
            Welcomed::Copy => {
                self.to_clipboard = Some(crate::connect_line());
                self.welcome = Some(Welcome { copied: true });
            }
            Welcomed::NewProject => {
                self.welcome_close();
                self.command(menus::NEW_PROJECT);
            }
            Welcomed::OpenFolder => {
                self.welcome_close();
                self.command(menus::OPEN_FOLDER);
            }
        }
    }

    /// The welcome sheet over the work column, when it is open.
    pub(crate) fn welcome_layer(&mut self, ui: &mut egui::Ui, work: Rect) {
        let Some(open) = self.welcome else {
            return;
        };
        if let Some(what) = intro::welcome(ui, work, &crate::connect_line(), open.copied) {
            self.welcomed(what);
        }
    }

    /// The ? in the corner of a place that has a sheet, and the sheet when it is open.
    pub(crate) fn intro_layer(&mut self, ui: &mut egui::Ui, work: Rect) {
        self.welcome_layer(ui, work);
        if self.session.is_none() {
            return;
        }
        let Some(sheet) = sheet_of(self.place) else {
            return;
        };
        let keys = self.shortcuts();
        let platform = craft_keymap::egui::platform(ui.ctx());
        let tip = match keys.chord(menus::INTRO) {
            Some(chord) => format!("How {} works ({})", sheet.title, chord.display(platform)),
            None => format!("How {} works", sheet.title),
        };
        // Build keeps Compute and Build at its foot, so its ? stands in the top corner.
        if intro::button(ui, work, self.place == Place::Build, self.intro.is_some(), &tip) {
            self.intro_toggle();
        }
        if self.intro.is_some() {
            let commands = [menus::PLACE_EDIT, menus::PLACE_BUILD, menus::PLACE_VERIFY, menus::NEXT_TAB, menus::INTRO];
            let rows: Vec<(String, &str)> = commands
                .iter()
                .zip(intro::KEY_ROWS)
                .filter_map(|(command, label)| keys.chord(command).map(|chord| (chord.display(platform), label)))
                .collect();
            if intro::show(ui, work, sheet, &rows) {
                self.intro = None;
            }
        }
    }
}
