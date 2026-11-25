//! The keyboard: every menu command's shortcut, the default the menus ship or the one the person
//! chose instead, remembered between launches. Ported from the web's `keys/keymap.ts` (overrides
//! over the shipped default, `null` for deliberately unbound, a stale override dropped rather than
//! kept, conflicts said without blocking) and the rows of `keys/commands.ts`.

use std::collections::BTreeMap;

use craft_keymap::{Chord, Platform};

use crate::menus;

/// The groups of the Keyboard settings, in their order (`GROUP_ORDER`).
pub const GROUPS: [&str; 5] = ["Editor", "Constructs", "Layout", "Modes", "App"];

/// One row of the Keyboard settings: the command, its label written as a verb, its group, and
/// whether it only acts with something selected (`needsSelection`). The web's commands carry the
/// web's labels; the menu commands the web has no shortcut row for follow in the same voice.
pub const ROWS: &[(&str, &str, &str, bool)] = &[
    (menus::FIND, "Find in this construct", "Editor", false),
    (menus::NEW_PRIMER, "Make a primer from the selection", "Editor", true),
    (menus::NEW_FEATURE, "Make a feature from the selection", "Editor", true),
    (menus::COPY_REVERSE, "Copy the reverse complement", "Editor", true),
    (menus::UNDO, "Back", "Editor", false),
    (menus::REDO, "Forward", "Editor", false),
    (menus::FIND_PROJECT, "Search this project", "Editor", false),
    (menus::NEXT_TAB, "Next construct", "Constructs", false),
    (menus::PREVIOUS_TAB, "Previous construct", "Constructs", false),
    (menus::CLOSE_TAB, "Close this construct", "Constructs", false),
    (menus::TO_SIDE, "Open this construct to the side", "Constructs", false),
    (menus::NEW_WINDOW, "Open this construct in a new window", "Constructs", false),
    (menus::QUICK_OPEN, "Go to a construct by name", "Constructs", false),
    (DIGITS, "Go to construct 1\u{2013}9", "Constructs", false),
    (menus::RENAME, "Rename this construct", "Constructs", false),
    (menus::MOVE, "Move this construct to a folder", "Constructs", false),
    (menus::DUPLICATE, "Duplicate this construct", "Constructs", false),
    (menus::DELETE, "Delete this construct", "Constructs", false),
    (menus::EXPORT, "Export this construct", "Constructs", false),
    (menus::REVERT, "Go back to what was saved", "Constructs", false),
    (menus::TOGGLE_PANEL, "Show or hide the sessions rail", "Layout", false),
    (menus::TOGGLE_CHANGES, "Show or hide Changes", "Layout", false),
    (menus::TOGGLE_WORK, "Show or hide the work column", "Layout", false),
    (menus::PLACE_EDIT, "Go to Edit", "Modes", false),
    (menus::PLACE_BUILD, "Go to Build", "Modes", false),
    (menus::PLACE_VERIFY, "Go to Verify", "Modes", false),
    (menus::SAVE, "Save now", "App", false),
    (menus::INTRO, "How this mode works", "App", false),
    (menus::WELCOME, "How Bios works", "App", false),
    (menus::NEW_PROJECT, "Make a new project", "App", false),
    (menus::OPEN_FOLDER, "Open a folder as a project", "App", false),
    (menus::IMPORT, "Import files", "App", false),
    (menus::NEW_FOLDER, "Make a new folder", "App", false),
    (menus::SETTINGS, "Open Settings", "App", false),
    (menus::DOCS, "Read the documentation", "App", false),
    (menus::CONNECT_AGENT, "Connect an agent", "App", false),
    (menus::CHECK_UPDATES, "Check for updates", "App", false),
];

/// The nine "go to construct N" chords are one idea and one row, fixed at ⌘1–⌘9.
pub const DIGITS: &str = "tab.select";

/// The person's shortcuts: only what differs from the default. `None` is deliberately unbound.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Keys {
    overrides: BTreeMap<String, Option<Chord>>,
}

fn known(command: &str) -> bool {
    command != DIGITS && ROWS.iter().any(|(c, ..)| *c == command)
}

impl Keys {
    /// From what was remembered: an empty text is unbound; a command that no longer exists, or a
    /// chord that does not read, is dropped (`sanitize`).
    pub fn from_saved(saved: &BTreeMap<String, String>) -> Keys {
        let mut overrides = BTreeMap::new();
        for (command, text) in saved {
            if !known(command) {
                continue;
            }
            if text.is_empty() {
                overrides.insert(command.clone(), None);
            } else if let Ok(chord) = Chord::parse(text) {
                overrides.insert(command.clone(), Some(chord));
            }
        }
        Keys { overrides }
    }

    /// What is remembered: each override as its chord's text, unbound as empty.
    pub fn to_saved(&self) -> BTreeMap<String, String> {
        self.overrides.iter().map(|(c, chord)| (c.clone(), chord.map(|ch| ch.portable()).unwrap_or_default())).collect()
    }

    /// What fires `command` now (`chordFor`).
    pub fn chord(&self, command: &str) -> Option<Chord> {
        match self.overrides.get(command) {
            Some(chosen) => *chosen,
            None => menus::default_chord(command),
        }
    }

    pub fn is_default(&self, command: &str) -> bool {
        !self.overrides.contains_key(command)
    }

    /// How many shortcuts differ from the default.
    pub fn changed(&self) -> usize {
        self.overrides.len()
    }

    /// Binds `command` to `chord`, or unbinds it. Choosing the default is the default again.
    pub fn set(&mut self, command: &str, chord: Option<Chord>) {
        if !known(command) {
            return;
        }
        if chord == menus::default_chord(command) {
            self.overrides.remove(command);
        } else {
            self.overrides.insert(command.to_string(), chord);
        }
    }

    pub fn reset(&mut self, command: &str) {
        self.overrides.remove(command);
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// The other commands on `chord`, as `platform` reads it (`conflictsFor`).
    pub fn conflicts(&self, chord: Chord, except: &str, platform: Platform) -> Vec<&'static str> {
        let digit = (1..=9).any(|n| Chord::parse(&format!("Cmd+{n}")).is_ok_and(|d| d.effective(platform) == chord.effective(platform)));
        let mut out: Vec<&'static str> = ROWS
            .iter()
            .filter(|(c, ..)| *c != except && *c != DIGITS)
            .filter(|(c, ..)| self.chord(c).is_some_and(|other| other.effective(platform) == chord.effective(platform)))
            .map(|(c, ..)| *c)
            .collect();
        if digit && except != DIGITS {
            out.push(DIGITS);
        }
        out
    }
}

/// A row's label, by command.
pub fn label(command: &str) -> &'static str {
    ROWS.iter().find(|(c, ..)| *c == command).map_or("", |(_, label, ..)| label)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use craft_keymap::{Chord, Platform};

    use super::{DIGITS, Keys, ROWS};
    use crate::menus;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).unwrap()
    }

    #[test]
    fn every_menu_command_has_a_row_and_every_row_a_command() {
        for (_, command, label, _) in menus::ITEMS.iter().filter(|(_, c, ..)| !c.is_empty()) {
            let shared = matches!(*command, menus::SHARE | menus::JOIN | menus::SYNC | menus::KEEP_MINE | menus::TAKE_THEIRS | menus::ABOUT);
            assert!(shared || ROWS.iter().any(|(c, ..)| c == command), "{label} has no row in Keyboard settings");
        }
        for (command, ..) in ROWS.iter().filter(|(c, ..)| *c != DIGITS) {
            assert!(menus::ITEMS.iter().any(|(_, c, ..)| c == command), "{command} is in no menu");
        }
    }

    #[test]
    fn an_override_wins_over_the_default_and_none_unbinds() {
        let mut keys = Keys::default();
        assert_eq!(keys.chord(menus::FIND), Some(chord("Cmd+F")));
        keys.set(menus::FIND, Some(chord("Cmd+Alt+F")));
        assert_eq!(keys.chord(menus::FIND), Some(chord("Cmd+Alt+F")));
        assert!(!keys.is_default(menus::FIND));
        keys.set(menus::SAVE, None);
        assert_eq!(keys.chord(menus::SAVE), None);
        assert_eq!(keys.changed(), 2);
        keys.reset(menus::FIND);
        assert_eq!(keys.chord(menus::FIND), Some(chord("Cmd+F")));
        keys.reset_all();
        assert_eq!(keys.chord(menus::SAVE), Some(chord("Cmd+S")));
        // Choosing the default again is not a change.
        keys.set(menus::SAVE, Some(chord("Cmd+S")));
        assert_eq!(keys.changed(), 0);
    }

    #[test]
    fn what_is_remembered_reads_back_and_a_stale_or_broken_entry_is_dropped() {
        let mut keys = Keys::default();
        keys.set(menus::FIND, Some(chord("Cmd+Alt+F")));
        keys.set(menus::SAVE, None);
        let saved = keys.to_saved();
        assert_eq!(Keys::from_saved(&saved), keys);
        let mut odd = BTreeMap::new();
        odd.insert("command.that.left".to_string(), "Cmd+K".to_string());
        odd.insert(menus::UNDO.to_string(), "Cmd+NoSuchKey".to_string());
        odd.insert(DIGITS.to_string(), "Cmd+K".to_string());
        assert_eq!(Keys::from_saved(&odd), Keys::default());
    }

    #[test]
    fn a_conflict_names_the_other_commands_without_blocking_the_change() {
        let mut keys = Keys::default();
        keys.set(menus::FIND_PROJECT, Some(chord("Cmd+F")));
        assert_eq!(keys.chord(menus::FIND_PROJECT), Some(chord("Cmd+F")));
        assert_eq!(keys.conflicts(chord("Cmd+F"), menus::FIND_PROJECT, Platform::Mac), vec![menus::FIND]);
        assert_eq!(keys.conflicts(chord("Cmd+3"), menus::SAVE, Platform::Mac), vec![DIGITS]);
        assert!(keys.conflicts(chord("Cmd+Alt+Shift+K"), menus::SAVE, Platform::Mac).is_empty());
    }
}
