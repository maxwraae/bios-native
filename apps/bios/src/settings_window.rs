//! The window's part of Settings: the keyboard rows from the keymap, and what a change in the
//! window does to what is remembered and to the menus.

use craft_keymap::{Chord, Key, Mods, Platform};

use bios_ui::settings::{KeyRow, Model, Outcome, Section, Settings};

use crate::App;
use crate::keys::{self, Keys};

impl App {
    /// The person's shortcuts, from what is remembered.
    pub(crate) fn shortcuts(&self) -> Keys {
        self.prefs.as_ref().map(|store| Keys::from_saved(&store.get().keys)).unwrap_or_default()
    }

    fn remember_keys(&mut self, keys: &Keys) {
        let saved = keys.to_saved();
        if let Some(store) = &mut self.prefs {
            store.update(|prefs| prefs.keys = saved);
        }
    }

    pub(crate) fn open_settings(&mut self, section: Section) {
        self.settings = Some(Settings::new(section));
    }

    /// The Keyboard section's rows, every menu command with its shortcut.
    pub(crate) fn key_rows(&self, platform: Platform) -> Vec<KeyRow> {
        let keys = self.shortcuts();
        keys::ROWS
            .iter()
            .map(|(command, label, group, needs_selection)| {
                let fixed = *command == keys::DIGITS;
                let chord = keys.chord(command);
                let clash = chord.map(|c| keys.conflicts(c, command, platform)).filter(|others| !others.is_empty() && !fixed).map(|others| {
                    let names: Vec<&str> = others.iter().map(|o| keys::label(o)).collect();
                    format!("Also {}. The one that can act right now wins.", names.join(", ").to_lowercase())
                });
                KeyRow {
                    command,
                    label,
                    group,
                    chord: if fixed {
                        Some(if platform.is_mac() { "\u{2318}1\u{2013}\u{2318}9".into() } else { "Ctrl+1\u{2013}9".into() })
                    } else {
                        chord.map(|c| c.display(platform))
                    },
                    fixed,
                    is_default: keys.is_default(command),
                    needs_selection: *needs_selection,
                    clash,
                }
            })
            .collect()
    }

    /// Does what was done in the Settings window.
    pub(crate) fn settings_did(&mut self, outcome: Outcome, platform: Platform) {
        let mut keys = self.shortcuts();
        match outcome {
            Outcome::Close => {
                self.settings = None;
                return;
            }
            Outcome::Bind(command, key, modifiers) => {
                let Some(key) = Key::from_egui(key) else {
                    return;
                };
                keys.set(command, Some(Chord { mods: Mods::from_egui(&modifiers, platform), key }));
            }
            Outcome::Unbind(command) => keys.set(command, None),
            Outcome::Reset(command) => keys.reset(command),
            Outcome::ResetAll => keys.reset_all(),
        }
        self.remember_keys(&keys);
    }

    /// Draws the Settings window when it is open.
    pub(crate) fn settings_window(&mut self, ui: &mut egui::Ui) {
        if self.settings.is_none() {
            return;
        }
        let platform = craft_keymap::egui::platform(ui.ctx());
        let rows = self.key_rows(platform);
        let changed = self.shortcuts().changed();
        let model = Model { rows: &rows, groups: &keys::GROUPS, changed, version: env!("CARGO_PKG_VERSION") };
        let screen = ui.ctx().content_rect();
        let outcomes = self.settings.as_mut().map(|s| s.show(ui, screen, &model)).unwrap_or_default();
        for outcome in outcomes {
            self.settings_did(outcome, platform);
        }
    }
}
