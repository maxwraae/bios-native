//! The window's menus: what is in them, their shortcuts, and which can run right now.
//!
//! The menus are declared once here and laid out for each system by `craft-menu`: on macOS they
//! go in the system menu bar, elsewhere in a bar at the top of the window.

use craft_keymap::{Chord, Key, Mods};
use craft_menu::{CommandRef, ItemKind, ItemRole, MenuBar, MenuRole, Row, from_items};

use crate::keys::Keys;

pub const NEW_PROJECT: &str = "project.new";
pub const OPEN_FOLDER: &str = "project.open";
/// Followed by which of the recent projects, counted from 0: `project.recent:2`.
pub const OPEN_RECENT: &str = "project.recent:";
pub const IMPORT: &str = "project.import";
pub const NEW_FOLDER: &str = "project.new_folder";
pub const EXPORT: &str = "plasmid.export";
pub const RENAME: &str = "plasmid.rename";
pub const MOVE: &str = "plasmid.move";
pub const SAVE: &str = "plasmid.save";
pub const REVERT: &str = "plasmid.revert";
pub const UNDO: &str = "edit.undo";
pub const REDO: &str = "edit.redo";
/// Find in the open plasmid (`FindBar.tsx`).
pub const FIND: &str = "edit.find";
/// The project's search box, in the sidebar.
pub const FIND_PROJECT: &str = "edit.find_project";
pub const COPY_REVERSE: &str = "selection.copy_reverse";
pub const NEW_FEATURE: &str = "selection.feature";
pub const NEW_PRIMER: &str = "selection.primer";
pub const DUPLICATE: &str = "plasmid.duplicate";
pub const DELETE: &str = "plasmid.delete";
pub const SHARE: &str = "share.publish";
pub const JOIN: &str = "share.join";
pub const SYNC: &str = "share.sync";
pub const KEEP_MINE: &str = "share.mine";
pub const TAKE_THEIRS: &str = "share.theirs";
pub const PLACE_EDIT: &str = "view.edit";
pub const PLACE_BUILD: &str = "view.build";
pub const PLACE_VERIFY: &str = "view.verify";
pub const TOGGLE_PANEL: &str = "view.panel";
pub const TOGGLE_CHANGES: &str = "view.changes";
/// ⌘B: the work column, the open plasmid's map and rows (`layout.toggleWork`).
pub const TOGGLE_WORK: &str = "view.work";
pub const CLOSE_TAB: &str = "tab.close";
pub const NEXT_TAB: &str = "tab.next";
pub const PREVIOUS_TAB: &str = "tab.previous";
/// Followed by which open tab, counted from 0: `tab.select:2`.
pub const SELECT_TAB: &str = "tab.select:";
pub const CONNECT_AGENT: &str = "help.agent";
pub const ABOUT: &str = "help.about";
/// Help > Check for Updates…: the engine's `check_for_update`, and the prompt to install.
pub const CHECK_UPDATES: &str = "help.updates";
/// The Settings window (platform/SettingsModal.tsx).
pub const SETTINGS: &str = "app.settings";
/// ⌘P: go to a plasmid by typing its name (explorer/FilePalette.tsx).
pub const QUICK_OPEN: &str = "project.quick_open";
/// ⌘/: how the place you are in works (intro/ModeIntro.tsx, `app.intro`); on the projects page,
/// the welcome sheet.
pub const INTRO: &str = "help.intro";
/// The welcome sheet: how Bios works, open by itself on the first launch.
pub const WELCOME: &str = "help.welcome";
/// The documentation (docs/DocsPage.tsx).
pub const DOCS: &str = "help.docs";

/// What the menus need to know about the window to say which items can run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct State {
    pub project: bool,
    pub plasmid: bool,
    /// The open plasmid has a change that can be undone.
    pub undo: bool,
    /// A step that was undone can be done again.
    pub redo: bool,
    /// The open plasmid has changes made by hand that are not saved yet.
    pub unsaved: bool,
    /// Bases of the open plasmid are selected.
    pub selection: bool,
    /// A Bios server is set up for this run.
    pub server: bool,
    /// The project is shared on a server.
    pub shared: bool,
    /// The open plasmid changed both here and on the server, and waits for a decision.
    pub conflict: bool,
    /// Which place the window is in: 0 Edit, 1 Build, 2 Verify.
    pub place: u8,
    /// The sidebar is put away.
    pub panel_hidden: bool,
    /// The Changes column is shown.
    pub changes: bool,
    /// The work column is put away.
    pub work_hidden: bool,
    /// How many plasmids are open as tabs.
    pub tabs: u8,
}

impl State {
    fn allows(self, command: &str) -> bool {
        match command {
            IMPORT | FIND_PROJECT | NEW_FOLDER | QUICK_OPEN => self.project,
            DUPLICATE | DELETE | RENAME | MOVE | EXPORT | FIND => self.plasmid,
            UNDO => self.undo,
            REDO => self.redo,
            SAVE | REVERT => self.unsaved,
            NEW_FEATURE | NEW_PRIMER | COPY_REVERSE => self.selection,
            SHARE | JOIN => self.project && self.server && !self.shared,
            SYNC => self.shared && self.server,
            KEEP_MINE | TAKE_THEIRS => self.conflict,
            PLACE_EDIT | PLACE_BUILD | PLACE_VERIFY | TOGGLE_PANEL | TOGGLE_CHANGES | TOGGLE_WORK => self.project,
            CLOSE_TAB => self.plasmid,
            INTRO => !self.project || self.place != 0,
            NEXT_TAB | PREVIOUS_TAB => self.tabs > 1,
            _ => true,
        }
    }

    /// A number that changes whenever the menus would.
    pub fn key(self) -> u64 {
        let flags = [self.project, self.plasmid, self.undo, self.selection, self.redo, self.unsaved, self.server, self.shared, self.conflict]
            .iter()
            .chain(&[self.panel_hidden, self.changes, self.work_hidden])
            .enumerate()
            .fold(0, |key, (i, on)| key | u64::from(*on) << i);
        flags | u64::from(self.place) << 16 | u64::from(self.tabs) << 24
    }

    /// How an item is drawn: the place you are in is the chosen one of the three.
    fn kind(self, command: &str) -> ItemKind {
        match command {
            PLACE_EDIT => ItemKind::Radio(self.place == 0),
            PLACE_BUILD => ItemKind::Radio(self.place == 1),
            PLACE_VERIFY => ItemKind::Radio(self.place == 2),
            _ => ItemKind::Plain,
        }
    }

    /// An item's label where it says what pressing it will do.
    fn label(self, command: &str, label: &'static str) -> &'static str {
        match command {
            TOGGLE_PANEL if !self.panel_hidden => "Hide Panel",
            TOGGLE_CHANGES if self.changes => "Hide Changes",
            TOGGLE_WORK if self.work_hidden => "Show Work Column",
            _ => label,
        }
    }
}

const SHIFT_CMD: Mods = Mods { cmd: true, ctrl: false, alt: false, shift: true };
const ALT_CMD: Mods = Mods { cmd: true, ctrl: false, alt: true, shift: false };

/// (menu, command, label, shortcut). A row labelled `---` is a separator.
pub(crate) const ITEMS: &[(&str, &str, &str, Option<Chord>)] = &[
    ("File", NEW_PROJECT, "New Project…", Some(Chord::new(SHIFT_CMD, Key::N))),
    ("File", OPEN_FOLDER, "Open Folder…", Some(Chord::new(Mods::CMD, Key::O))),
    ("File", "", "---", None),
    ("File", QUICK_OPEN, "Go to Plasmid…", Some(Chord::new(Mods::CMD, Key::P))),
    ("File", IMPORT, "Import Files…", Some(Chord::new(SHIFT_CMD, Key::I))),
    ("File", NEW_FOLDER, "New Folder…", None),
    ("File", EXPORT, "Export…", Some(Chord::new(SHIFT_CMD, Key::E))),
    ("File", "", "---", None),
    ("File", SAVE, "Save Now", Some(Chord::new(Mods::CMD, Key::S))),
    ("File", CLOSE_TAB, "Close Plasmid", Some(Chord::new(Mods::CMD, Key::W))),
    ("File", REVERT, "Revert to Saved", None),
    ("Edit", UNDO, "Undo", Some(Chord::new(Mods::CMD, Key::Z))),
    ("Edit", REDO, "Redo", Some(Chord::new(SHIFT_CMD, Key::Z))),
    ("Edit", "", "---", None),
    ("Edit", COPY_REVERSE, "Copy Reverse Complement", Some(Chord::new(SHIFT_CMD, Key::C))),
    ("Edit", "", "---", None),
    ("Edit", FIND, "Find…", Some(Chord::new(Mods::CMD, Key::F))),
    ("Edit", FIND_PROJECT, "Find in Project", None),
    ("Edit", "", "---", None),
    ("Edit", NEW_FEATURE, "New Feature from Selection…", Some(Chord::new(SHIFT_CMD, Key::F))),
    ("Edit", NEW_PRIMER, "New Primer from Selection…", Some(Chord::new(SHIFT_CMD, Key::P))),
    // The places and the panel, with the web's chords (keys/commands.ts: `mode.edit` Mod+Alt+1…,
    // `layout.toggleSessions` Mod+Shift+Backslash). Changes is the native column the web has not got.
    ("View", PLACE_EDIT, "Edit", Some(Chord::new(ALT_CMD, Key::Num1))),
    ("View", PLACE_BUILD, "Build", Some(Chord::new(ALT_CMD, Key::Num2))),
    ("View", PLACE_VERIFY, "Verify", Some(Chord::new(ALT_CMD, Key::Num3))),
    ("View", "", "---", None),
    ("View", TOGGLE_PANEL, "Show Panel", Some(Chord::new(SHIFT_CMD, Key::Backslash))),
    ("View", TOGGLE_CHANGES, "Show Changes", Some(Chord::new(SHIFT_CMD, Key::H))),
    ("View", TOGGLE_WORK, "Hide Work Column", Some(Chord::new(Mods::CMD, Key::B))),
    ("Plasmid", RENAME, "Rename…", None),
    ("Plasmid", MOVE, "Move to Folder…", None),
    ("Plasmid", DUPLICATE, "Duplicate", Some(Chord::new(Mods::CMD, Key::D))),
    ("Plasmid", DELETE, "Delete…", None),
    ("Share", SHARE, "Share This Project…", None),
    ("Share", JOIN, "Join a Shared Project…", None),
    ("Share", "", "---", None),
    ("Share", SYNC, "Sync Now", Some(Chord::new(Mods::CMD, Key::R))),
    ("Share", "", "---", None),
    ("Share", KEEP_MINE, "Settle Conflict: Keep Mine", None),
    ("Share", TAKE_THEIRS, "Settle Conflict: Take Theirs", None),
    // The open tabs (keys/commands.ts: `construct.stepPrev` Ctrl+Shift+Tab, `construct.stepNext`
    // Ctrl+Tab, `construct.select1`… Mod+1…). The tabs themselves follow, each under its number.
    ("Window", PREVIOUS_TAB, "Previous Plasmid", Some(Chord::new(Mods { cmd: false, ctrl: true, alt: false, shift: true }, Key::Tab))),
    ("Window", NEXT_TAB, "Next Plasmid", Some(Chord::new(Mods { cmd: false, ctrl: true, alt: false, shift: false }, Key::Tab))),
    ("Help", DOCS, "Bios Documentation", None),
    ("Help", WELCOME, "How Bios Works", None),
    ("Help", INTRO, "How This Place Works", Some(Chord::new(Mods::CMD, Key::Slash))),
    ("Help", CONNECT_AGENT, "Connect an Agent…", None),
    ("Help", CHECK_UPDATES, "Check for Updates…", None),
    // Laid out in the app menu on macOS, under Edit elsewhere, by its role.
    ("Edit", SETTINGS, "Settings…", Some(Chord::new(Mods::CMD, Key::Comma))),
    ("Help", ABOUT, "About Bios", None),
];

/// ⌘1 to ⌘9: the open tabs, by place in the strip.
const DIGITS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

/// The shortcut a command ships with, before the person changes it.
pub fn default_chord(command: &str) -> Option<Chord> {
    ITEMS.iter().find(|(_, c, ..)| *c == command).and_then(|(.., chord)| *chord)
}

/// The menus as they stand for `state`, with `recent` (the names of the projects opened lately,
/// the latest first) under File > Open Recent, `tabs` (the open plasmids) under Window, and each
/// item's shortcut as `keys` has it.
pub fn bar(state: State, recent: &[String], tabs: &[String], keys: &Keys) -> MenuBar {
    let mut rows: Vec<Row> = Vec::new();
    // Sharing needs a Bios server. Without one set up there is nothing in the menu to use, so
    // it is not shown at all.
    let sharing = state.server || state.shared;
    for (menu, command, label, _) in ITEMS.iter().filter(|(menu, ..)| sharing || *menu != "Share") {
        rows.push(Row {
            path: vec![(*menu).to_string()],
            command: CommandRef::new(*command),
            label: state.label(command, label).to_string(),
            shortcut: if command.is_empty() { None } else { keys.chord(command) },
            enabled: state.allows(command),
            kind: state.kind(command),
        });
        // The recent folders come straight after Open Folder.
        for (i, name) in recent.iter().enumerate().filter(|_| *command == OPEN_FOLDER) {
            rows.push(Row {
                path: vec!["File".to_string(), "Open Recent".to_string()],
                command: CommandRef::new(format!("{OPEN_RECENT}{i}")),
                label: name.clone(),
                shortcut: None,
                enabled: true,
                kind: ItemKind::Plain,
            });
        }
        // The open tabs come straight after Next Plasmid, the first nine with their numbers.
        if *command == NEXT_TAB {
            if !tabs.is_empty() {
                rows.push(Row {
                    path: vec!["Window".to_string()],
                    command: CommandRef::new(""),
                    label: "---".into(),
                    shortcut: None,
                    enabled: true,
                    kind: ItemKind::Plain,
                });
            }
            for (i, name) in tabs.iter().enumerate() {
                rows.push(Row {
                    path: vec!["Window".to_string()],
                    command: CommandRef::new(format!("{SELECT_TAB}{i}")),
                    label: name.clone(),
                    shortcut: DIGITS.get(i).map(|digit| Chord::new(Mods::CMD, *digit)),
                    enabled: true,
                    kind: ItemKind::Plain,
                });
            }
        }
    }
    let menus: &[&str] = if sharing {
        &["File", "Edit", "View", "Plasmid", "Share", "Window", "Help"]
    } else {
        &["File", "Edit", "View", "Plasmid", "Window", "Help"]
    };
    let mut bar = from_items(menus, &rows);
    bar.tag_menu("File", MenuRole::File);
    bar.tag_menu("Edit", MenuRole::Edit);
    bar.tag_menu("View", MenuRole::View);
    bar.tag_menu("Window", MenuRole::Window);
    bar.tag_menu("Help", MenuRole::Help);
    bar.tag_item(&CommandRef::new(ABOUT), ItemRole::About);
    bar.tag_item(&CommandRef::new(SETTINGS), ItemRole::Settings);
    bar
}

/// The commands whose shortcut was pressed this frame and that can run now. A shortcut that a
/// text field uses itself (undo while typing in the search box) is left to the field.
pub fn pressed(ctx: &egui::Context, state: State, keys: &Keys) -> Vec<String> {
    let typing = ctx.egui_wants_keyboard_input();
    let mut out = Vec::new();
    for (_, command, _, _) in ITEMS.iter().filter(|(_, c, ..)| !c.is_empty()) {
        let Some(chord) = keys.chord(command) else { continue };
        if (typing && matches!(*command, UNDO | REDO)) || !state.allows(command) {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&chord.to_egui_shortcut())) {
            out.push((*command).to_string());
        }
    }
    // After the items, so ⌥⌘1 is taken by Edit before ⌘1 could read it as the first tab. A digit
    // past the open tabs is left alone rather than eaten for nothing (useConstructKeys.ts).
    for (i, digit) in DIGITS.iter().enumerate().take(usize::from(state.tabs)) {
        if ctx.input_mut(|input| input.consume_shortcut(&Chord::new(Mods::CMD, *digit).to_egui_shortcut())) {
            out.push(format!("{SELECT_TAB}{i}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ITEMS, State, bar};

    #[test]
    fn the_share_menu_is_shown_only_with_a_server_or_a_shared_project() {
        let titles =
            |state: State| super::bar(state, &[], &[], &crate::keys::Keys::default()).menus.iter().map(|m| m.title.clone()).collect::<Vec<_>>();
        let project = State { project: true, ..State::default() };
        assert_eq!(titles(project), ["File", "Edit", "View", "Plasmid", "Window", "Help"]);
        assert!(titles(State { server: true, ..project }).contains(&"Share".to_string()));
        assert!(titles(State { shared: true, ..project }).contains(&"Share".to_string()));
    }

    #[test]
    fn sharing_waits_for_a_server_and_syncing_for_a_shared_project() {
        let project = State { project: true, ..State::default() };
        assert!(!project.allows(super::SHARE) && !project.allows(super::SYNC));
        let signed_in = State { server: true, ..project };
        assert!(signed_in.allows(super::SHARE) && signed_in.allows(super::JOIN) && !signed_in.allows(super::SYNC));
        let shared = State { shared: true, ..signed_in };
        assert!(shared.allows(super::SYNC) && !shared.allows(super::SHARE) && !shared.allows(super::KEEP_MINE));
        assert!(State { conflict: true, ..shared }.allows(super::TAKE_THEIRS));
    }

    #[test]
    fn every_item_is_in_the_menus_and_no_shortcut_is_used_twice() {
        // With a server set up, so that the Share menu is there too.
        let bar = bar(State { server: true, ..State::default() }, &[], &[], &crate::keys::Keys::default());
        assert_eq!(bar.items().len(), ITEMS.iter().filter(|(_, _, label, _)| *label != "---").count());
        let chords: Vec<_> = ITEMS.iter().filter_map(|(_, _, _, chord)| *chord).collect();
        for (i, chord) in chords.iter().enumerate() {
            assert!(!chords[..i].contains(chord), "{chord:?} is bound twice");
        }
    }

    #[test]
    fn recent_projects_are_a_submenu_of_file_each_with_its_own_command() {
        let plain = bar(State::default(), &[], &[], &crate::keys::Keys::default());
        let listed = bar(State::default(), &["Lab".to_string(), "Thesis".to_string()], &[], &crate::keys::Keys::default());
        assert_eq!(listed.items().len(), plain.items().len() + 2);
        let second = listed.find(&craft_menu::CommandRef::new("project.recent:1")).map(|item| item.label.clone());
        assert_eq!(second.as_deref(), Some("Thesis"));
    }

    #[test]
    fn what_needs_a_project_or_a_plasmid_waits_for_one() {
        let nothing = State::default();
        assert!(nothing.allows(super::OPEN_FOLDER) && nothing.allows(super::NEW_PROJECT));
        assert!(nothing.allows(super::WELCOME) && nothing.allows(super::INTRO), "the projects page has the welcome sheet");
        assert!(!State { project: true, ..nothing }.allows(super::INTRO), "Edit has no sheet");
        assert!(!nothing.allows(super::IMPORT) && !nothing.allows(super::UNDO) && !nothing.allows(super::DELETE));
        let open = State { project: true, plasmid: true, ..State::default() };
        assert!(!open.allows(super::SAVE) && State { unsaved: true, ..open }.allows(super::REVERT));
        assert_ne!(State { redo: true, ..open }.key(), State { unsaved: true, ..open }.key());
        assert!(!open.allows(super::NEW_FEATURE) && State { selection: true, ..open }.allows(super::NEW_PRIMER));
        assert!(open.allows(super::IMPORT) && open.allows(super::DUPLICATE) && !open.allows(super::UNDO));
        assert_ne!(nothing.key(), open.key());
    }

    #[test]
    fn the_open_tabs_are_listed_under_window_the_first_nine_with_their_numbers() {
        let names: Vec<String> = (1..=10).map(|n| format!("p{n}")).collect();
        let listed = bar(State::default(), &[], &names, &crate::keys::Keys::default());
        let first = listed.find(&craft_menu::CommandRef::new("tab.select:0")).map(|item| (item.label.clone(), item.shortcut));
        assert_eq!(first, Some(("p1".to_string(), Some(craft_keymap::Chord::new(craft_keymap::Mods::CMD, craft_keymap::Key::Num1)))));
        let tenth = listed.find(&craft_menu::CommandRef::new("tab.select:9")).map(|item| item.shortcut);
        assert_eq!(tenth, Some(None));
    }
}
