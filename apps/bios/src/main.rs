//! The Bios window: the places strip and the project's plasmids on the left, the open one in the
//! middle, and, when it is asked for, everything that has happened to it on the right.
//!
//! The window changes nothing by itself. Every action goes through the engine's commands, the
//! same ones an agent uses over MCP, and the window notices changes made by anyone else (an
//! agent, the command line, another window) by asking the project twice a second whether
//! anything was written.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod build;
mod drop;
mod editing;
mod floating;
mod intro_sheet;
mod keys;
mod menus;
mod oligos;
mod palette;
mod prefs;
mod projects_page;
mod script;
mod session_restore;
mod settings_window;
mod side;
mod updates;
mod verify;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bios_core::enzymes::CutSite;
use bios_core::{Construct, formats, genbank_write};
use bios_engine::Session;
use bios_engine::editor::edit::{Direction, MoveKey};
use bios_layout::overview::flywheel;
use bios_layout::overview::hit;
use bios_layout::overview::minimap::NavFeature;
use bios_layout::overview::navigate;
use bios_layout::overview::selection::{DragSelection, FinishedAt, drag_selection};
use bios_layout::overview::viewport::{set_center, set_width};
use bios_layout::pane::Focus;
use bios_project::recents::{self, Recents};
use bios_project::{Actor, ConstructId, Entry, Listed, Project, Who};
use bios_ui::ask::{self, Ask};
use bios_ui::changes::{self, Action, Lineage};
use bios_ui::feature_card::FeatureCard;
use bios_ui::find_bar::{FindBar, FindMatch};
use bios_ui::row_menu::RowMenu;
use bios_ui::selection_readout::PrimerAsk;
use bios_ui::session_nav::{self, Nav};
use bios_ui::sidebar::{self, Place};
use bios_ui::tab_strip::{self, TabStrip};
use bios_ui::{app_layout, canvas_tools, enzymes_menu, map, navigator, pane, plasmid_title, rows, theme, view_tab};
use craft_dialogs::{Answer, Dialogs, FileDialog, Message, MessageButtons, MessageLevel, MessageResult};
use craft_menu::{Gate, LayoutOptions, MenuBarUi, NativeMenu, Target};
use craft_settings::{Location, SettingsStore};
use editing::Editing;
use egui::{Align2, Rect, Stroke, UiBuilder, pos2, vec2};
use prefs::Prefs;
use serde_json::{Value, json};

const APP_ID: &str = "com.wraae.bios.native";
/// How often the project is asked whether anyone else changed it.
const POLL: Duration = Duration::from_millis(500);
const APP_NAME: &str = "Bios";
/// Height of the menu bar when it is drawn inside the window.
const MENU_BAR: f32 = 26.0;
/// How long after the last edit made by hand it is saved, as in the web app.
const AUTOSAVE: Duration = Duration::from_millis(2000);

/// What a dialog's answer is for.
enum Then {
    NewProject,
    OpenFolder,
    Import,
    Export(ConstructId),
    Delete(ConstructId),
    /// Quit although a change could not be saved.
    Quit,
    /// Files for Verify, or where to save one of an alignment's.
    Verify(verify::Pick),
    /// Files chosen for Build's list of inserts.
    BuildFiles,
    Nothing,
}

/// What a line of text that was asked for is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Rename(ConstructId),
    MoveTo(ConstructId),
    NewFolder,
    /// What the project is to be called on the server.
    ShareAs,
    /// Which project on the server to join.
    Join,
    /// The organism to codon-optimise these bases for, first and last, 0-based.
    Optimise(usize, usize),
}

/// Where the menus are.
enum Menus {
    /// In the system menu bar (macOS).
    System(NativeMenu),
    /// In a bar at the top of the window.
    Window,
}

/// The construct on screen, from the open project.
struct Opened {
    id: ConstructId,
    /// The version on screen, which an edit that depends on positions has to name.
    token: String,
    /// What is on screen: the working copy, which runs ahead of the project between saves.
    construct: Construct,
    history: Vec<Entry>,
    lineage: Lineage,
    editing: Editing,
}

struct App {
    session: Option<Session>,
    listing: Vec<Listed>,
    folders: Vec<PathBuf>,
    /// What was deleted from the project and can be brought back.
    deleted: Vec<bios_project::Deleted>,
    /// The server the project is shared on, when it is.
    shared_on: Option<String>,
    /// Plasmids that changed both here and on the server and wait for a decision.
    conflicts: Vec<ConstructId>,
    /// What is remembered between launches. `None` remembers nothing, as in a test.
    prefs: Option<SettingsStore<Prefs>>,
    /// The project's stamp when it was last read; a different one means someone wrote to it.
    stamp: u64,
    polled: Instant,
    opened: Option<Opened>,
    /// A file opened on its own, outside any project. It can be looked at, not changed.
    loose: Option<Construct>,
    query: String,
    /// The query the results on screen belong to.
    searched: String,
    found: Option<Vec<session_nav::Found>>,
    /// Bumped whenever what is drawn changes, so cached layout knows to rebuild.
    revision: u64,
    problem: Option<String>,
    plan: Option<rows::Plan>,
    /// The strip's one row laid out, while the map's pane is the strip.
    strip_plan: Option<rows::Plan>,
    styled: bool,
    /// Where to save a picture of the first settled frame, then quit.
    shot: Option<PathBuf>,
    /// How long to wait before taking that picture, so that it can show something that happens
    /// after the window opens.
    shot_after: Duration,
    started: Instant,
    frames: u32,
    /// The picture has been asked for and not yet delivered.
    shooting: bool,
    /// Where the person is in the open sequence.
    cursor: rows::Cursor,
    /// The gap a press started at, which a drag selects from.
    anchor: Option<usize>,
    /// The same for a press on the map, and how many turns of the ring a shift-press stood away
    /// from where the pointer reads: a drag round the ring counts on from there.
    map_anchor: i64,
    map_turn: i64,
    /// When the last edit was made by hand. It is saved once this is `AUTOSAVE` old.
    edited: Option<Instant>,
    /// A save was refused. It is not tried again until the person edits or asks.
    save_refused: bool,
    /// Text a command put aside for the clipboard, handed over at the end of the frame.
    to_clipboard: Option<String>,
    /// The window's `movable` flag as last set, so AppKit is only told when it changes.
    window_movable: Option<bool>,
    /// `--input` and `--record`: scripted input and pictures over time.
    script: Option<script::Script>,
    /// What was copied here last, with its features. A paste of exactly these bases brings them.
    copied: Option<bios_engine::editor::fragment::DnaFragment>,
    /// Something the person should know that is not a failure, shown where problems are.
    notice: Option<String>,
    /// The sheet naming a new feature or primer on the selection, while it is open.
    ask: Option<Ask>,
    /// + Primer, being answered on the selection's pill.
    primer_ask: Option<PrimerAsk>,
    /// ⌘F's bar while it is open, what it found, and in which revision of what is drawn.
    find: Option<(FindBar, Vec<FindMatch>, u64)>,
    /// The menu on a selection or on what is drawn over the bases, while it is open.
    row_menu: Option<RowMenu>,
    /// Open that menu on the selection as soon as it is on screen (`--menu`, for a picture).
    menu_on_open: bool,
    /// The card of the feature that was clicked, while it is open.
    card: Option<FeatureCard>,
    /// A line of text being asked for (a new name, a folder), and what it is for.
    prompt: Option<(Asked, ask::Prompt)>,
    menus: Menus,
    /// Made on the first frame, when there is a window to wake.
    dialogs: Option<Dialogs<Then>>,
    focus_search: bool,
    /// Which place the window is in. Only Edit is built here; Build and Verify say so.
    place: Place,
    /// The sidebar is put away.
    panel_hidden: bool,
    /// The panel's width, dragged between 168 and 420.
    panel_width: f32,
    /// The Changes column is shown. Hidden unless asked for: the web has no such column.
    changes_shown: bool,
    /// ⌘B put the work column away (`workPanel.tsx`): the map and rows are not drawn, and stay as
    /// they were for when it comes back.
    work_hidden: bool,
    /// Draw the drop overlay as if files were held over the window (`--dropping`, for a picture).
    dropping: bool,
    /// The panel's own state: open folders, a name being typed, a menu.
    nav: Nav,
    /// The plasmids open as tabs, in the strip's order. The one on screen is `opened`.
    tabs: Vec<ConstructId>,
    /// The tabs last written down, with the project they were in, so they are written down only
    /// when they change.
    tabs_seen: Option<(PathBuf, prefs::OpenTabs)>,
    tab_strip: TabStrip,
    /// The editor pane: which tracks show, the split, and how each construct's map is viewed.
    pane: pane::Pane,
    /// The Build place: what is being built, and the folder's page once something has been.
    build: build::Build,
    /// The Verify place: its panel, the front door, a board, an alignment.
    verify: verify::Verify,
    /// What kind each typed folder is (`build`, `verification`), for the panel's rows.
    folder_kinds: Vec<(String, String)>,
    /// Oligos held against the open plasmid without landing in it (oligos.rs).
    oligos: oligos::Oligos,
    /// Checking for and installing a newer Bios.
    updates: updates::Updates,
    /// The projects page, shown when no project is open.
    page: projects_page::Page,
    /// The folders opened lately: the projects page and File > Open Recent, shared with
    /// `bios-cli` and every agent. In memory in a test and for a picture.
    recents: Recents,
    /// Where New Project and a drop on the projects page make a project, when not the usual place
    /// (a test's own folder).
    new_place: Option<PathBuf>,
    /// The Settings window, while it is open.
    settings: Option<bios_ui::settings::Settings>,
    /// ⌘P's palette, while it is open.
    quick_open: Option<bios_ui::quick_open::QuickOpen>,
    /// The place whose "how it works" sheet is open.
    intro: Option<Place>,
    /// The welcome sheet, while it is open.
    welcome: Option<intro_sheet::Welcome>,
    /// The documentation, while it is open over the window.
    docs: Option<bios_ui::docs::Docs>,
    /// The person said to quit with a change unsaved; the next close goes through.
    quitting: bool,
}

fn label(path: &Path) -> String {
    path.with_extension("").components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

impl App {
    fn new(shot: Option<PathBuf>, shot_after: Duration) -> Self {
        App {
            session: None,
            listing: Vec::new(),
            folders: Vec::new(),
            deleted: Vec::new(),
            shared_on: None,
            conflicts: Vec::new(),
            prefs: None,
            stamp: 0,
            polled: Instant::now(),
            opened: None,
            loose: None,
            query: String::new(),
            searched: String::new(),
            found: None,
            revision: 0,
            problem: None,
            plan: None,
            strip_plan: None,
            styled: false,
            shot,
            shot_after,
            started: Instant::now(),
            frames: 0,
            shooting: false,
            cursor: rows::Cursor::default(),
            anchor: None,
            map_anchor: 0,
            map_turn: 0,
            edited: None,
            save_refused: false,
            to_clipboard: None,
            window_movable: None,
            script: None,
            copied: None,
            notice: None,
            ask: None,
            primer_ask: None,
            find: None,
            row_menu: None,
            menu_on_open: false,
            card: None,
            prompt: None,
            menus: Menus::Window,
            dialogs: None,
            focus_search: false,
            place: Place::Edit,
            panel_hidden: false,
            panel_width: theme::NAV_FULL_W,
            changes_shown: false,
            work_hidden: false,
            dropping: false,
            nav: Nav::default(),
            tabs: Vec::new(),
            tabs_seen: None,
            tab_strip: TabStrip::default(),
            pane: pane::Pane::default(),
            build: build::Build::default(),
            verify: verify::Verify::default(),
            folder_kinds: Vec::new(),
            oligos: oligos::Oligos::default(),
            updates: updates::Updates::default(),
            page: projects_page::Page::default(),
            recents: Recents::in_memory(),
            new_place: None,
            settings: None,
            quick_open: None,
            intro: None,
            welcome: None,
            docs: None,
            quitting: false,
        }
    }

    /// The open tabs' names, in the strip's order.
    fn tab_names(&self) -> Vec<String> {
        self.tabs.iter().map(|id| self.listing.iter().find(|l| l.id == *id).map(|l| l.name.clone()).unwrap_or_default()).collect()
    }

    /// Brings a tab to the front.
    fn select_tab(&mut self, which: usize) {
        if let Some(id) = self.tabs.get(which).cloned() {
            self.open_construct(&id);
        }
    }

    /// Steps through the tabs, round from the last to the first (`construct.stepNext`).
    fn step_tab(&mut self, by: isize) {
        let count = self.tabs.len() as isize;
        if count == 0 {
            return;
        }
        let at = self.opened.as_ref().and_then(|o| self.tabs.iter().position(|t| *t == o.id)).map_or(0, |i| i as isize);
        self.select_tab((at + by).rem_euclid(count) as usize);
    }

    /// Closes a tab. When it was the one in front, the tab after it takes its place, or the one
    /// before (workspace/tabs.ts, `closeTabAt`); with none left, nothing is open.
    fn close_tab(&mut self, which: usize) {
        let Some(id) = self.tabs.get(which).cloned() else {
            return;
        };
        let front = self.opened.as_ref().is_some_and(|o| o.id == id);
        // What was typed in it is saved before it goes.
        if front && !self.save() {
            return;
        }
        self.tabs.remove(which);
        if front {
            match self.tabs.get(which).or_else(|| which.checked_sub(1).and_then(|i| self.tabs.get(i))).cloned() {
                Some(next) => self.open_construct(&next),
                None => {
                    self.opened = None;
                    self.ask = None;
                    self.card = None;
                    self.revision += 1;
                }
            }
        }
    }

    /// Takes the window's arrangement from what was remembered.
    fn arrange_from_prefs(&mut self) {
        if let Some(layout) = self.prefs.as_ref().map(|store| store.get().layout.clone()) {
            self.panel_hidden = layout.panel_hidden;
            self.panel_width = app_layout::clamp_nav(layout.panel_width);
            self.changes_shown = layout.changes_shown;
        }
    }

    /// Remembers the window's arrangement for the next launch.
    fn remember_arrangement(&mut self) {
        let layout = prefs::Layout { panel_width: self.panel_width, panel_hidden: self.panel_hidden, changes_shown: self.changes_shown };
        if let Some(store) = &mut self.prefs
            && store.get().layout != layout
        {
            store.update(|prefs| prefs.layout = layout);
        }
    }

    /// Goes to a place. Going somewhere while the panel is away brings it back, because you went
    /// there to see it (layout/PlaceStrip.tsx, `onPick`).
    fn go_to_place(&mut self, place: Place) {
        if place == self.place {
            return;
        }
        self.place = place;
        self.ask = None;
        self.card = None;
        self.intro_arrive(place);
        if self.panel_hidden {
            self.panel_hidden = false;
            self.remember_arrangement();
        }
    }

    /// Leaves the project for the first screen, as the web's mark and "All projects" leave it for
    /// the projects grid.
    fn leave_project(&mut self) {
        if !self.save() {
            return;
        }
        self.session = None;
        self.listing.clear();
        self.folders.clear();
        self.deleted.clear();
        self.shared_on = None;
        self.conflicts.clear();
        self.opened = None;
        self.loose = None;
        self.query.clear();
        self.searched.clear();
        self.found = None;
        self.problem = None;
        self.notice = None;
        self.ask = None;
        self.card = None;
        self.prompt = None;
        self.place = Place::Edit;
        self.tabs.clear();
        self.revision += 1;
        self.read_cards(true);
    }

    /// The folders opened lately, the latest first, as the menu names them.
    fn recent(&self) -> Vec<String> {
        let recent = self.recents.folders();
        recent
            .iter()
            .take(prefs::RECENT)
            .map(|root| root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned()))
            .collect()
    }

    /// Where a new project is made unless the person chooses another place.
    fn new_projects_place(&self) -> Option<PathBuf> {
        // A test never makes anything in the person's own folders.
        if cfg!(test) {
            return self.new_place.clone();
        }
        self.new_place.clone().or_else(recents::default_location)
    }

    /// A file dialog that starts in the place new projects are made.
    fn in_new_projects_place(&self, dialog: FileDialog) -> FileDialog {
        match self.new_projects_place() {
            Some(folder) => dialog.directory(folder),
            None => dialog,
        }
    }

    fn menu_state(&self) -> menus::State {
        menus::State {
            project: self.session.is_some(),
            plasmid: self.opened.is_some(),
            undo: self.opened.as_ref().is_some_and(|o| o.editing.can_undo() || o.history.len() > 1),
            redo: self.opened.as_ref().is_some_and(|o| o.editing.can_redo()),
            unsaved: self.opened.as_ref().is_some_and(|o| o.editing.dirty_since().is_some()),
            selection: self.opened.is_some() && self.cursor.selection.is_some(),
            server: self.session.as_ref().is_some_and(|s| s.server().is_some()),
            shared: self.shared_on.is_some(),
            conflict: self.opened.as_ref().is_some_and(|o| self.conflicts.contains(&o.id)),
            place: match self.place {
                Place::Edit => 0,
                Place::Build => 1,
                Place::Verify => 2,
            },
            panel_hidden: self.panel_hidden,
            changes: self.changes_shown,
            work_hidden: self.work_hidden,
            tabs: u8::try_from(self.tabs.len()).unwrap_or(u8::MAX),
        }
    }

    /// The menus for this frame, and the commands chosen from them or by their shortcuts. `full`
    /// loses its top strip when the menus are drawn in the window.
    fn menu_commands(&mut self, ui: &mut egui::Ui, full: &mut Rect) -> Vec<String> {
        let state = self.menu_state();
        let gate = Gate {
            modal: self.dialogs.as_ref().is_some_and(Dialogs::is_open) || self.settings.is_some() || self.quick_open.is_some() || self.docs.is_some(),
            recording_shortcut: self.settings.as_ref().is_some_and(bios_ui::settings::Settings::recording),
            text_editing: ui.ctx().egui_wants_keyboard_input(),
        };
        let options = LayoutOptions::new(APP_NAME);
        let recent = self.recent();
        let tabs = self.tab_names();
        let keys = self.shortcuts();
        // The menus are laid out again when anything they show changes, the recent projects and
        // the open tabs included.
        let shown = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (state.key(), &recent, &tabs, keys.to_saved()).hash(&mut hasher);
            hasher.finish()
        };
        let mut chosen: Vec<String> = Vec::new();
        match &mut self.menus {
            Menus::System(menu) => {
                menu.set_gate(gate);
                chosen.extend(menu.take_clicks(ui.ctx()).into_iter().map(|c| c.id));
                menu.sync_with(shown, || craft_menu::mac_layout(&menus::bar(state, &recent, &tabs, &keys), &options));
            }
            Menus::Window => {
                // The system layout would move About and Quit to an app menu the window has not got.
                let target = if cfg!(target_os = "macos") { Target::Linux } else { Target::current() };
                let laid_out = craft_menu::layout(&menus::bar(state, &recent, &tabs, &keys), target, &options).bar;
                let strip = Rect::from_min_size(full.min, vec2(full.width(), MENU_BAR));
                ui.painter().rect_filled(strip, 0.0, theme::CANVAS);
                ui.painter().line_segment([strip.left_bottom(), strip.right_bottom()], Stroke::new(1.0, theme::HAIRLINE));
                ui.scope_builder(UiBuilder::new().max_rect(strip.shrink2(vec2(8.0, 2.0))), |ui| {
                    chosen.extend(MenuBarUi::new(&laid_out).gate(gate).show(ui).into_iter().map(|c| c.id));
                });
                full.min.y += MENU_BAR;
            }
        }
        if !gate.modal {
            chosen.extend(menus::pressed(ui.ctx(), state, &keys));
        }
        chosen
    }

    fn ask(&mut self, dialog: FileDialog, then: Then) {
        if let Some(dialogs) = &mut self.dialogs {
            dialogs.ask(dialog, then);
        }
    }

    /// Runs one menu command.
    fn command(&mut self, command: &str) {
        if let Some(which) = command.strip_prefix(menus::SELECT_TAB).and_then(|n| n.parse::<usize>().ok()) {
            self.select_tab(which);
            return;
        }
        if let Some(which) = command.strip_prefix(menus::OPEN_RECENT).and_then(|n| n.parse::<usize>().ok()) {
            let root = self.recents.folders().get(which).cloned();
            if let Some(root) = root {
                self.open_project(&root);
            }
            return;
        }
        let open = self.opened.as_ref().map(|o| (o.id.clone(), o.construct.name.clone()));
        match (command, open) {
            // A project is a folder anywhere: a save panel asks where to make it and its name.
            (menus::NEW_PROJECT, _) => {
                let dialog = self.in_new_projects_place(FileDialog::save().title("New Project").file_name("My Project"));
                self.ask(dialog, Then::NewProject)
            }
            (menus::OPEN_FOLDER, _) => {
                let dialog = self.in_new_projects_place(FileDialog::folder().title("Open Folder"));
                self.ask(dialog, Then::OpenFolder)
            }
            (menus::IMPORT, _) => self.ask(FileDialog::open_many().title("Import Files"), Then::Import),
            (menus::FIND_PROJECT, _) => self.focus_search = true,
            (menus::INTRO, _) => self.intro_toggle(),
            (menus::WELCOME, _) => self.welcome_toggle(),
            (menus::DOCS, _) => self.docs = Some(bios_ui::docs::Docs::open(None)),
            (menus::QUICK_OPEN, _) => self.quick_open = Some(bios_ui::quick_open::QuickOpen::default()),
            (menus::FIND, Some(_)) => self.open_find(None),
            // A file for someone else. The name the person gives decides the format: .fasta or
            // .fa is FASTA, anything else GenBank.
            (menus::EXPORT, Some((id, name))) => self.ask(FileDialog::save().title("Export").file_name(format!("{name}.gb")), Then::Export(id)),
            (menus::SHARE, _) => {
                let name = self.session.as_ref().map(|s| s.project().name().to_string()).unwrap_or_default();
                self.prompt = Some((Asked::ShareAs, ask::Prompt::new("Share this project on the server as", name, "Share")));
            }
            (menus::JOIN, _) => self.prompt = Some((Asked::Join, ask::Prompt::new("Join the shared project called", "", "Join"))),
            (menus::SYNC, _) => self.sync(),
            (menus::KEEP_MINE | menus::TAKE_THEIRS, Some((id, _))) => {
                let keep = if command == menus::KEEP_MINE { "mine" } else { "theirs" };
                if let Some(result) = self.run("settle_conflict", json!({ "plasmid": id.as_str(), "keep": keep })) {
                    self.told(&result["report"]);
                }
            }
            (menus::NEW_FOLDER, _) => self.prompt = Some((Asked::NewFolder, ask::Prompt::new("New folder", "", "Make folder"))),
            (menus::RENAME, Some((id, name))) => {
                self.prompt = Some((Asked::Rename(id), ask::Prompt::new(format!("Rename {name}"), name, "Rename")));
            }
            (menus::MOVE, Some((id, name))) => {
                let folder = self.listing.iter().find(|l| l.id == id).and_then(|l| l.path.parent()).map(label).unwrap_or_default();
                self.prompt = Some((Asked::MoveTo(id), ask::Prompt::new(format!("Move {name} to folder"), folder, "Move")));
            }
            // The other strand of what is selected, read 5' to 3': what you order to prime against it.
            (menus::COPY_REVERSE, Some(_)) => self.copy(true),
            // The verbs on the selection's pill: a primer is answered on the pill itself, a feature
            // in the sheet the pill turns into. Either reads the way the selection was made:
            // dragged leftwards, it reads in reverse.
            (menus::NEW_PRIMER, Some(_)) => {
                if let Some((start, end)) = self.cursor.selection {
                    self.ask = None;
                    self.primer_ask = Some(PrimerAsk::new((start, end), self.anchor.is_none_or(|anchor| anchor <= start)));
                }
            }
            (menus::NEW_FEATURE, Some(_)) => {
                if let Some((start, _)) = self.cursor.selection {
                    self.primer_ask = None;
                    let forward = self.anchor.is_none_or(|anchor| anchor <= start);
                    let span = self.cursor.selection.unwrap_or((start, start));
                    self.ask = Some(Ask::new(ask::What::Feature, forward).serving(self.ask_targets(), span));
                }
            }
            // The last thing done: a step of this sitting when there is one, else the last saved change.
            (menus::UNDO, Some((id, _))) => {
                if self.opened.as_ref().is_some_and(|o| o.editing.can_undo()) {
                    self.edit(|editing| Ok(editing.undo()));
                } else {
                    self.run("undo_last_edit", json!({ "plasmid": id.as_str() }));
                }
            }
            (menus::REDO, Some(_)) => self.edit(|editing| Ok(editing.redo())),
            (menus::SAVE, Some(_)) => {
                self.save_refused = false;
                self.save();
            }
            (menus::REVERT, Some((id, _))) => self.revert(&id),
            (menus::DUPLICATE, Some((id, _))) => self.duplicate(&id),
            (menus::DELETE, Some((id, name))) => {
                let message = Message {
                    level: MessageLevel::Warning,
                    title: format!("Delete {name}?"),
                    description: "It goes to the project's trash with its history, and can be brought back.".into(),
                    buttons: MessageButtons::OkCancel,
                };
                if let Some(dialogs) = &mut self.dialogs {
                    dialogs.ask_message(message, Then::Delete(id));
                }
            }
            (menus::PLACE_EDIT, _) => self.go_to_place(Place::Edit),
            (menus::PLACE_BUILD, _) => self.go_to_place(Place::Build),
            (menus::PLACE_VERIFY, _) => self.go_to_place(Place::Verify),
            (menus::TOGGLE_PANEL, _) => {
                self.panel_hidden = !self.panel_hidden;
                self.remember_arrangement();
            }
            (menus::TOGGLE_WORK, _) => self.work_hidden = !self.work_hidden,
            (menus::TOGGLE_CHANGES, _) => {
                self.changes_shown = !self.changes_shown;
                self.remember_arrangement();
            }
            (menus::CLOSE_TAB, Some((id, _))) => {
                if let Some(which) = self.tabs.iter().position(|t| *t == id) {
                    self.close_tab(which);
                }
            }
            (menus::NEXT_TAB, _) => self.step_tab(1),
            (menus::PREVIOUS_TAB, _) => self.step_tab(-1),
            (menus::CONNECT_AGENT, _) => {
                let (line, description) = connect_agent();
                self.to_clipboard = Some(line);
                let message = Message { level: MessageLevel::Info, title: "Connect an Agent".into(), description, buttons: MessageButtons::Ok };
                if let Some(dialogs) = &mut self.dialogs {
                    dialogs.ask_message(message, Then::Nothing);
                }
            }
            // About is Settings' last section, as the web's account menu opens it.
            (menus::ABOUT, _) => self.open_settings(bios_ui::settings::Section::About),
            (menus::CHECK_UPDATES, _) => self.updates.check_now(),
            (menus::SETTINGS, _) => self.open_settings(bios_ui::settings::Section::Appearance),
            _ => {}
        }
    }

    /// Acts on what the person chose in a dialog.
    fn answered(&mut self, then: Then, answer: Answer) {
        match (then, answer) {
            (_, Answer::Unavailable(why)) => self.problem = Some(format!("That dialog could not be shown: {why}")),
            (Then::NewProject, Answer::Paths(paths)) => {
                if let Some(root) = paths.first() {
                    let name = root.file_name().map_or_else(|| "Project".to_string(), |n| n.to_string_lossy().into_owned());
                    match Project::create(root, &name) {
                        Ok(_) => self.open_project(root),
                        Err(e) => self.problem = Some(e.to_string()),
                    }
                }
            }
            (Then::OpenFolder, Answer::Paths(paths)) => {
                if let Some(root) = paths.first() {
                    self.open_project(root);
                }
            }
            // Files chosen with no project open start one from them.
            (Then::Import, Answer::Paths(paths)) if self.session.is_none() => self.start_from_files(&paths),
            (Then::Verify(pick), Answer::Paths(paths)) => self.verify_picked(pick, paths),
            (Then::BuildFiles, Answer::Paths(paths)) => self.build.take_files(&paths),
            (Then::Import, Answer::Paths(paths)) => {
                for path in paths {
                    if let Some(result) = self.run("import_plasmid", json!({ "file": path.display().to_string() }))
                        && let Some(id) = result["id"].as_str().and_then(ConstructId::parse)
                    {
                        self.open_construct(&id);
                    }
                }
            }
            (Then::Export(id), Answer::Paths(paths)) => {
                if let Some(path) = paths.first() {
                    let fasta =
                        path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["fasta", "fa", "fna"].contains(&e.to_lowercase().as_str()));
                    // The save panel has already asked before replacing a file that is there.
                    let params = json!({ "plasmid": id.as_str(), "path": path.display().to_string(), "format": if fasta { "fasta" } else { "genbank" }, "overwrite": true });
                    self.run("export_plasmid", params);
                }
            }
            (Then::Quit, Answer::Message(MessageResult::Ok)) => {
                self.quitting = true;
            }
            (Then::Delete(id), Answer::Message(MessageResult::Ok)) => {
                self.run("delete_plasmid", json!({ "plasmid": id.as_str(), "confirm": true }));
            }
            _ => {}
        }
    }

    /// Opens whatever `path` is: a project folder, any other folder (as a project read from its
    /// files), a plasmid inside a project, or a loose file.
    fn open_path(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if path.is_dir() && !path.ancestors().any(Project::is_project) {
            self.open_project(&path);
            return;
        }
        if let Some(root) = path.ancestors().find(|a| Project::is_project(a)) {
            self.open_project(root);
            let inside = path.strip_prefix(root).unwrap_or(Path::new(""));
            if let Some(id) = self.listing.iter().find(|l| l.path == inside).map(|l| l.id.clone()) {
                self.open_construct(&id);
            }
            return;
        }
        if self.session.is_some() && path.is_file() {
            // A file dropped on an open project joins it, as a copy.
            if let Some(result) = self.run("import_plasmid", json!({ "file": path.display().to_string() }))
                && let Some(id) = result["id"].as_str().and_then(ConstructId::parse)
            {
                self.open_construct(&id);
            }
            return;
        }
        let read = std::fs::read(&path)
            .map_err(|e| format!("{} could not be opened: {e}", path.display()))
            .and_then(|bytes| formats::read(&bytes, &path.to_string_lossy()));
        match read {
            Ok(opened) => {
                self.loose = Some(opened.construct);
                self.revision += 1;
                self.problem = None;
            }
            Err(why) => self.problem = Some(why),
        }
    }

    fn open_project(&mut self, root: &Path) {
        if !self.save() {
            return;
        }
        match Project::open(root) {
            Ok(project) => {
                self.session = Some(Session::open(project, Who { actor: Actor::Human, source: "window".into(), session: None }));
                let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                let shut_folders = self.prefs.as_ref().map(|store| store.get().projects.shut_folders_of(&root)).unwrap_or_default();
                self.nav.arrive(shut_folders);
                self.tabs.clear();
                if let Err(e) = self.recents.opened(&root) {
                    self.notice = Some(format!("Bios could not add this folder to its list of projects: {e}"));
                }
                self.loose = None;
                self.opened = None;
                self.problem = None;
                self.refresh();
                self.restore_tabs(&root);
            }
            Err(e) => self.problem = Some(e.to_string()),
        }
    }

    /// Reads the project again: what is in it, and the open construct if it has moved on.
    fn refresh(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let project = session.project();
        self.stamp = project.stamp();
        match (project.list(), project.folders()) {
            (Ok(listing), Ok(folders)) => {
                self.listing = listing;
                self.folders = folders;
                let listing = &self.listing;
                self.tabs.retain(|id| listing.iter().any(|l| l.id == *id));
                self.deleted = project.deleted().unwrap_or_default();
                self.folder_kinds =
                    project.folder_metas().unwrap_or_default().into_iter().map(|(path, meta)| (label(&path.with_extension("")), meta.kind)).collect();
                self.shared_on = bios_sync::connection(project).ok().flatten().map(|c| c.server);
            }
            (Err(e), _) | (_, Err(e)) => self.problem = Some(e.to_string()),
        }
        // Reading can itself record something (a change made by another program), so the stamp
        // is taken again after it.
        if let Some(id) = self.opened.as_ref().map(|o| o.id.clone()) {
            if self.listing.iter().any(|l| l.id == id) {
                self.open_construct(&id);
            } else {
                self.opened = None;
                self.revision += 1;
            }
        }
        self.verify_refresh();
        if let Some(session) = &self.session {
            self.stamp = session.project().stamp();
        }
        self.searched.clear();
    }

    fn open_construct(&mut self, id: &ConstructId) {
        let same = self.opened.as_ref().is_some_and(|o| o.id == *id);
        // What was typed into the plasmid being left is saved before another takes its place.
        if !same && !self.save() {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let project = session.project();
        match (project.read(id), project.history(id)) {
            (Ok(loaded), Ok(history)) => {
                let parent = self.listing.iter().find(|l| &l.id == id).and_then(|l| l.derived_from.as_ref());
                let lineage = Lineage {
                    made_from: parent
                        .map(|p| self.listing.iter().find(|l| &l.id == p).map_or_else(|| "a plasmid no longer here".to_string(), |l| label(&l.path))),
                    made_into: self.listing.iter().filter(|l| l.derived_from.as_ref() == Some(id)).map(|l| label(&l.path)).collect(),
                };
                let token = format!("{}:{}", loaded.version, loaded.sha);
                match self.opened.as_mut().filter(|_| same) {
                    Some(opened) => {
                        opened.history = history;
                        opened.lineage = lineage;
                        // The project moved on without this window. With nothing unsaved here its
                        // version is taken. With something unsaved both are kept apart: the save
                        // names the version it was made on, is refused, and says so.
                        if opened.token != token && opened.editing.dirty_since().is_none() {
                            opened.editing.replaced(loaded.construct);
                            opened.token = token;
                        }
                    }
                    None => {
                        self.cursor = rows::Cursor::default();
                        self.anchor = None;
                        self.ask = None;
                        self.card = None;
                        self.edited = None;
                        self.save_refused = false;
                        self.revision += 1;
                        if !self.tabs.contains(&loaded.id) {
                            self.tabs.push(loaded.id.clone());
                        }
                        let editing = Editing::open(loaded.construct.clone());
                        self.opened = Some(Opened { id: loaded.id, token, construct: loaded.construct, history, lineage, editing });
                    }
                }
                self.show_editing(None, false);
            }
            (Err(e), _) | (_, Err(e)) => self.problem = Some(e.to_string()),
        }
    }

    /// Copies a plasmid under a free name ("… copy", "… copy 2"), and opens the copy.
    fn duplicate(&mut self, id: &ConstructId) {
        let Some(name) = self.listing.iter().find(|l| l.id == *id).map(|l| l.name.clone()) else {
            return;
        };
        let taken = |candidate: &str| self.listing.iter().any(|l| l.name.eq_ignore_ascii_case(candidate));
        let copy = std::iter::once(format!("{name} copy")).chain((2..100).map(|n| format!("{name} copy {n}"))).find(|c| !taken(c));
        if let Some(result) = self.run("duplicate_plasmid", json!({ "plasmid": id.as_str(), "name": copy }))
            && let Some(made) = result["id"].as_str().and_then(ConstructId::parse)
        {
            self.open_construct(&made);
        }
    }

    /// A new plasmid, as the web's New plasmid makes one: "Untitled construct" (or the next free
    /// "Untitled construct 2"), from the web's own starting bases with no features, opened.
    fn new_plasmid(&mut self, folder: &str) {
        const NAME: &str = "Untitled construct";
        let taken = |candidate: &str| self.listing.iter().any(|l| l.name.eq_ignore_ascii_case(candidate));
        let Some(name) = std::iter::once(NAME.to_string()).chain((2..1000).map(|n| format!("{NAME} {n}"))).find(|c| !taken(c)) else {
            return;
        };
        let params = json!({ "name": name, "sequence": include_str!("untitled.seq").trim(), "circular": true, "folder": folder });
        if let Some(result) = self.run("create_plasmid", params)
            && let Some(made) = result["id"].as_str().and_then(ConstructId::parse)
        {
            self.open_construct(&made);
        }
    }

    /// Does what was asked for in the sidebar's list, through the commands.
    fn nav_chose(&mut self, chosen: session_nav::Chosen) {
        use session_nav::Chosen;
        match chosen {
            // A plasmid row always lands in Edit, wherever it lives (SessionNav.tsx, `land`).
            Chosen::Open { id, span } => {
                self.go_to_place(Place::Edit);
                self.go_to(&id, span);
            }
            Chosen::ShowFolder(folder) => {
                self.go_to_place(Place::Build);
                self.build.show_folder(&folder);
            }
            Chosen::NewBuild => {
                self.go_to_place(Place::Build);
                self.build.backbone_changed();
            }
            Chosen::Restore(id) => self.bring_back(&id),
            Chosen::Import => self.command(menus::IMPORT),
            Chosen::NewPlasmid => self.new_plasmid(""),
            Chosen::NewPlasmidIn(folder) => self.new_plasmid(&folder),
            Chosen::NewFolder(folder) => {
                self.run("create_folder", json!({ "folder": folder }));
            }
            Chosen::RenameFolder { from, to } => {
                self.run("rename_folder", json!({ "from": from, "to": to }));
            }
            Chosen::Rename { id, name } => {
                self.run("rename_plasmid", json!({ "plasmid": id.as_str(), "new_name": name }));
            }
            Chosen::Duplicate(id) => self.duplicate(&id),
            Chosen::Download(id) => {
                let name = self.listing.iter().find(|l| l.id == id).map(|l| l.name.clone()).unwrap_or_default();
                self.ask(FileDialog::save().title("Export").file_name(format!("{name}.gb")), Then::Export(id));
            }
            Chosen::MoveTo { id, folder } => {
                let here = self.listing.iter().find(|l| l.id == id).and_then(|l| l.path.parent()).map(label).unwrap_or_default();
                if here != folder {
                    self.run("move_plasmid", json!({ "plasmid": id.as_str(), "folder": folder }));
                }
            }
            // The row asked first, in place: this is the person's yes.
            Chosen::Delete(id) => {
                self.run("delete_plasmid", json!({ "plasmid": id.as_str(), "confirm": true }));
            }
            Chosen::Width { width, done } => {
                self.panel_width = app_layout::clamp_nav(width);
                if done {
                    self.remember_arrangement();
                }
            }
            Chosen::ShutFolders(shut) => {
                let root = self.session.as_ref().map(|s| s.project().root().to_path_buf());
                if let (Some(store), Some(root)) = (&mut self.prefs, root) {
                    store.update(|prefs| prefs.projects.remember_shut_folders(&root, shut));
                }
            }
        }
    }

    /// The sidebar: its head (the toggle, the project, the places), then the place's own list, then
    /// its divider and drag edge.
    fn sidebar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        sidebar::ground(ui, rect);
        let project = self.session.as_ref().map(|s| s.project().name().to_string()).unwrap_or_default();
        let mut acts: Vec<(&[bios_ui::icon::Node], &str)> = match self.place {
            Place::Verify => vec![(bios_ui::lucide::PLUS, "Verify a new run")],
            _ => vec![
                (bios_ui::lucide::UPLOAD, "Import files…"),
                (bios_ui::lucide::FILE_PLUS, "New plasmid"),
                (bios_ui::lucide::FOLDER_PLUS, "New folder"),
            ],
        };
        // In Build, the way back to the console from a folder's page: a new build into whatever
        // plasmid is in front.
        if self.place == Place::Build {
            acts.push((bios_ui::lucide::BLOCKS, "New build"));
        }
        let (body_top, pressed) = sidebar::head(ui, rect, &sidebar::Head { project: &project, place: self.place, acts: &acts });
        let body = Rect::from_min_max(egui::pos2(rect.left(), body_top), rect.max);
        let mut chosen = Vec::new();
        if self.place == Place::Verify {
            self.verify_panel(ui, body);
        } else if self.session.is_some() {
            let model = session_nav::Model {
                folders: &self.folders,
                listing: &self.listing,
                open: self.opened.as_ref().map(|o| &o.id),
                found: self.found.as_deref(),
                focus_search: std::mem::take(&mut self.focus_search),
                deleted: &self.deleted,
                folder_kinds: &self.folder_kinds,
                building: self.place == Place::Build,
            };
            chosen = self.nav.show(ui, body, &model, &mut self.query);
        }
        if let Some((width, done)) = sidebar::edge(ui, rect) {
            chosen.push(session_nav::Chosen::Width { width, done });
        }
        for chosen in chosen {
            self.nav_chose(chosen);
        }
        match pressed {
            Some(sidebar::Pressed::TogglePanel) => self.command(menus::TOGGLE_PANEL),
            Some(sidebar::Pressed::AllProjects) => self.leave_project(),
            Some(sidebar::Pressed::Go(place)) => self.go_to_place(place),
            Some(sidebar::Pressed::Act(i)) => match (self.place, i) {
                (Place::Verify, _) => self.verify.screen = verify::Screen::Front,
                (_, 0) => self.nav_chose(session_nav::Chosen::Import),
                (_, 1) => self.nav_chose(session_nav::Chosen::NewPlasmid),
                (_, 2) => self.nav.start_new_folder(),
                _ => self.nav_chose(session_nav::Chosen::NewBuild),
            },
            None => {}
        }
    }

    /// Brings a deleted plasmid back to where it was, and opens it.
    fn bring_back(&mut self, id: &ConstructId) {
        if self.run("restore_plasmid", json!({ "plasmid": id.as_str() })).is_some() {
            self.open_construct(id);
        }
    }

    /// Opens a plasmid and, when a place in it is named, selects those bases and brings them
    /// into view: what choosing a search hit does.
    fn go_to(&mut self, id: &ConstructId, span: Option<(usize, usize)>) {
        self.open_construct(id);
        if let (Some((start, end)), Some(opened)) = (span, self.opened.as_mut().filter(|o| o.id == *id)) {
            opened.editing.select(start, end);
            self.anchor = None;
            self.show_editing(None, true);
        }
    }

    /// Makes what is drawn agree with the editing state: the working copy, and the place in it.
    /// `now` restarts the caret's blink; `reveal` brings the caret's row into view.
    fn show_editing(&mut self, now: Option<f64>, reveal: bool) {
        let Some(opened) = self.opened.as_mut() else {
            return;
        };
        if let Some(working) = opened.editing.construct().filter(|working| **working != opened.construct) {
            opened.construct = working.clone();
            self.revision += 1;
            // A card is about one feature. When that feature is gone or is another one now, the
            // card has nothing true to say.
            let still = |card: &FeatureCard| opened.construct.features.get(card.feature).is_some_and(|f| f.name == card.was.name);
            if !self.card.as_ref().is_some_and(still) {
                self.card = None;
            }
        }
        let (caret, selection) = (opened.editing.caret(), opened.editing.selection());
        if caret != self.cursor.caret || selection != self.cursor.selection {
            let last = opened.construct.len().saturating_sub(1);
            let reveal = reveal.then(|| opened.editing.cursor().min(last));
            self.cursor = rows::Cursor { caret, selection, moved: now, reveal };
            // A note about the last copy has been read once the person has moved on.
            self.notice = None;
        }
    }

    /// Does something to the plasmid being edited, then shows it. An edit that changed the bases
    /// starts the wait for the save; one that was refused says why.
    fn edit(&mut self, change: impl FnOnce(&mut Editing) -> Result<bool, String>) {
        let Some(opened) = self.opened.as_mut() else {
            return;
        };
        match change(&mut opened.editing) {
            Ok(true) => {
                self.edited = Some(Instant::now());
                self.save_refused = false;
                self.problem = None;
                self.card = None;
            }
            Ok(false) => {}
            Err(why) => self.problem = Some(why),
        }
        self.show_editing(Some(self.started.elapsed().as_secs_f64()), true);
    }

    /// Copies the selection, as it is or reverse-complemented: its bases go to the clipboard, and
    /// the features and primers on them are kept here for a paste of those same bases.
    fn copy(&mut self, reverse: bool) {
        let Some((fragment, notice)) = self.opened.as_ref().and_then(|o| o.editing.copy_fragment(reverse)) else {
            return;
        };
        self.to_clipboard = Some(fragment.sequence.clone());
        self.copied = Some(fragment);
        self.notice = notice;
    }

    /// Saves what was changed by hand, as one entry in the history. True when nothing is left
    /// unsaved, which is what anything that is about to read or change the project needs first.
    fn save(&mut self) -> bool {
        let (Some(opened), Some(session)) = (self.opened.as_mut(), &self.session) else {
            return true;
        };
        let (true, Some(working)) = (opened.editing.dirty_since().is_some(), opened.editing.construct()) else {
            return true;
        };
        let text = match genbank_write::write(working) {
            Ok(text) => text,
            Err(e) => {
                self.problem = Some(format!("Not saved: {e}"));
                self.save_refused = true;
                return false;
            }
        };
        let params = json!({ "plasmid": opened.id.as_str(), "version": opened.token, "genbank": text, "summary": opened.editing.summary() });
        let saved = session.execute("write_plasmid", &params).and_then(|_| Ok(session.project().read(&opened.id)?));
        match saved {
            Ok(loaded) => {
                opened.token = format!("{}:{}", loaded.version, loaded.sha);
                opened.editing.saved(loaded.construct);
                self.edited = None;
                self.problem = None;
                self.refresh();
                true
            }
            Err(failure) => {
                self.problem = Some(failure.0);
                self.save_refused = true;
                false
            }
        }
    }

    /// Lets go of what was changed by hand and not saved, and shows the plasmid as the project has it.
    fn revert(&mut self, id: &ConstructId) {
        let Some(loaded) = self.session.as_ref().and_then(|s| s.project().read(id).ok()) else {
            return;
        };
        if let Some(opened) = self.opened.as_mut().filter(|o| o.id == *id) {
            opened.token = format!("{}:{}", loaded.version, loaded.sha);
            opened.editing.replaced(loaded.construct);
        }
        self.edited = None;
        self.save_refused = false;
        self.problem = None;
        self.show_editing(None, false);
    }

    /// Saves once the person has stopped editing for a moment.
    fn autosave(&mut self, ctx: &egui::Context) {
        let Some(edited) = self.edited.filter(|_| !self.save_refused) else {
            return;
        };
        match AUTOSAVE.checked_sub(edited.elapsed()) {
            Some(wait) if !wait.is_zero() => ctx.request_repaint_after(wait),
            _ => {
                self.save();
            }
        }
    }

    /// What the keyboard did to the open plasmid this frame. A text field that has the keys
    /// keeps them.
    fn keys(&mut self, ctx: &egui::Context) {
        if self.opened.is_none()
            || self.place != Place::Edit
            || self.ask.is_some()
            || self.prompt.is_some()
            || self.settings.is_some()
            || self.quick_open.is_some()
            || self.docs.is_some()
            || ctx.egui_wants_keyboard_input()
            || self.dialogs.as_ref().is_some_and(Dialogs::is_open)
        {
            return;
        }
        let per_row = self.plan.as_ref().map_or(0, rows::Plan::bps_per_row);
        for event in ctx.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(text) => self.edit(|editing| editing.type_text(&text)),
                egui::Event::Paste(text) => {
                    let copied = self.copied.take();
                    self.edit(|editing| editing.paste_with(&text, copied.as_ref()));
                    self.copied = copied;
                }
                egui::Event::Copy => self.copy(false),
                egui::Event::Cut => {
                    self.copy(false);
                    self.edit(|editing| Ok(editing.cut()?.is_some()));
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let moved = match key {
                        egui::Key::ArrowLeft => Some(MoveKey::ArrowLeft),
                        egui::Key::ArrowRight => Some(MoveKey::ArrowRight),
                        egui::Key::ArrowUp => Some(MoveKey::ArrowUp),
                        egui::Key::ArrowDown => Some(MoveKey::ArrowDown),
                        egui::Key::Home => Some(MoveKey::Home),
                        egui::Key::End => Some(MoveKey::End),
                        _ => None,
                    };
                    match (key, moved) {
                        (_, Some(step)) if !modifiers.command => self.edit(|editing| {
                            editing.step(step, modifiers.shift, per_row);
                            Ok(false)
                        }),
                        (egui::Key::Backspace, _) => self.edit(|editing| editing.delete(Direction::Back)),
                        (egui::Key::Delete, _) => self.edit(|editing| editing.delete(Direction::Forward)),
                        (egui::Key::A, _) if modifiers.command => self.edit(|editing| {
                            editing.select_all();
                            Ok(false)
                        }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// Makes the feature or primer the sheet describes, on the selection it was opened for.
    fn land(&mut self, ask: &Ask) {
        // The command names the version it is made on, so what was typed is saved first.
        if !self.save() {
            return;
        }
        let (Some(opened), Some((start, end))) = (&self.opened, self.cursor.selection) else {
            return;
        };
        let (id, token) = (opened.id.clone(), opened.token.clone());
        let strand = if ask.forward { "forward" } else { "reverse" };
        let mut params =
            json!({ "plasmid": id.as_str(), "version": token, "name": ask.name(), "start": start + 1, "end": end + 1, "strand": strand });
        let command = match ask.what {
            ask::What::Primer => "add_primer",
            ask::What::Feature => {
                params["type"] = json!(ask.genbank_type());
                side::said_about(ask, &mut params);
                "add_feature"
            }
        };
        if self.run(command, params).is_some() {
            // The selection gives way to a caret at the footprint's end, so the new annotation is
            // what is left on screen (`SelectionReadout.tsx` `finish`: `collapseSelectionTo(sel.end)`).
            if let Some(opened) = self.opened.as_mut() {
                opened.editing.put_caret(end);
            }
            self.anchor = Some(end);
            self.show_editing(None, false);
        }
    }

    /// Puts the caret in a gap, or selects the bases between two gaps.
    fn point(&mut self, anchor: usize, gap: usize, now: f64) {
        if let Some(opened) = self.opened.as_mut() {
            opened.editing.point(anchor, gap);
        }
        self.anchor = Some(anchor);
        self.show_editing(Some(now), false);
    }

    /// Brings in what others saved on the server and sends what was saved here.
    fn sync(&mut self) {
        if let Some(result) = self.run("sync_project", json!({})) {
            self.told(&result);
        }
    }

    /// Shows what a sync said, and remembers which plasmids wait for a decision.
    fn told(&mut self, report: &Value) {
        let waiting: Vec<&str> = report["conflicts"].as_array().into_iter().flatten().filter_map(|c| c["plasmid"].as_str()).collect();
        self.conflicts = self.listing.iter().filter(|l| waiting.contains(&l.name.as_str())).map(|l| l.id.clone()).collect();
        let said: Vec<&str> = report["said"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let shown = match said.as_slice() {
            [] => None,
            [one] => Some((*one).to_string()),
            [first, rest @ ..] => Some(format!("{first} (and {} more)", rest.len())),
        };
        // A conflict is something to act on, so it is said the way a problem is.
        if self.conflicts.is_empty() {
            self.notice = shown;
        } else {
            self.problem = shown;
        }
    }

    /// Does what a line of text was asked for, through the commands.
    fn asked(&mut self, what: &Asked, text: &str) {
        match what {
            Asked::NewFolder => {
                self.run("create_folder", json!({ "folder": text }));
            }
            Asked::Rename(id) => {
                self.run("rename_plasmid", json!({ "plasmid": id.as_str(), "new_name": text }));
            }
            Asked::MoveTo(id) => {
                self.run("move_plasmid", json!({ "plasmid": id.as_str(), "folder": text }));
            }
            Asked::ShareAs => {
                if let Some(result) = self.run("share_project", json!({ "title": text })) {
                    self.told(&result["report"]);
                }
            }
            Asked::Join => {
                if let Some(result) = self.run("join_shared_project", json!({ "project": text })) {
                    self.told(&result["report"]);
                }
            }
            Asked::Optimise(start, end) => self.optimise(*start, *end, text),
        }
    }

    /// A press or a drag on the map. A drag round the ring keeps counting past the origin, so it
    /// selects through it; a shift-press extends the short way round.
    fn map_gesture(&mut self, gesture: map::MapGesture, now: f64) {
        // The feature panel stays open while the person works elsewhere, as in the web app; only
        // a press on another feature re-points it.
        self.ask = None;
        let Some(opened) = self.opened.as_mut() else {
            return;
        };
        let length = i64::try_from(opened.construct.len()).unwrap_or(i64::MAX);
        let base = |position: i64| usize::try_from(position).unwrap_or(0);
        let apply = |editing: &mut Editing, picked: DragSelection| match picked {
            DragSelection::Caret(gap) => editing.put_caret(base(gap)),
            DragSelection::Range { start, end, finished_at } => editing.select_from(base(start), base(end), finished_at == FinishedAt::Start),
        };
        let mut clicked = None;
        match gesture {
            map::MapGesture::Press { gap, shift: false } => {
                self.map_anchor = i64::try_from(gap).unwrap_or(0);
                self.map_turn = 0;
                opened.editing.put_caret(gap);
            }
            map::MapGesture::Press { gap, shift: true } => {
                let gap = i64::try_from(gap).unwrap_or(0);
                let anchor = opened.editing.anchor().and_then(|a| i64::try_from(a).ok()).unwrap_or(gap);
                let to = if opened.construct.circular { hit::ring_extend_to(anchor, gap, length) } else { gap };
                self.map_anchor = anchor;
                self.map_turn = to - gap;
                apply(&mut opened.editing, drag_selection(anchor, to, length));
            }
            map::MapGesture::Drag { gap } => apply(&mut opened.editing, drag_selection(self.map_anchor, gap.saturating_add(self.map_turn), length)),
            map::MapGesture::Feature { start, end } => {
                opened.editing.select(start, end);
                clicked = opened.construct.features.iter().position(|f| f.start == start && f.end == end);
            }
        }
        self.anchor = None;
        self.show_editing(Some(now), true);
        if let Some(feature) = clicked {
            self.open_card(feature);
        }
    }

    fn gesture(&mut self, gesture: rows::Gesture, now: f64) {
        self.ask = None;
        match gesture {
            // A shift-click extends from the end the last gesture did not finish on, as a text
            // editor does: the anchor stays put however many times you shift-click.
            rows::Gesture::Press { gap, shift: true } => {
                let anchor = self.opened.as_ref().and_then(|o| o.editing.anchor()).unwrap_or(gap);
                self.point(anchor, gap, now);
            }
            rows::Gesture::Press { gap, shift: false } => self.point(gap, gap, now),
            // A drag belongs to the press that began it; one that began on a bar selects nothing more.
            rows::Gesture::Drag { gap } => {
                if self.drag_primer(gap) {
                } else if let Some(anchor) = self.anchor {
                    self.point(anchor, gap, now);
                }
            }
            rows::Gesture::PrimerEnd { feature, start } => self.begin_primer_drag(feature, start),
            rows::Gesture::Span { feature, start, end } => {
                if let Some(opened) = self.opened.as_mut() {
                    opened.editing.select(start, end);
                }
                self.anchor = None;
                self.show_editing(Some(now), false);
                self.open_card(feature);
            }
            // The pane's: it hands the rows back to the map (`work`).
            rows::Gesture::ZoomedOut { .. } => {}
        }
    }

    /// Runs an engine command as the person at the window, then shows what it did.
    fn run(&mut self, command: &str, params: Value) -> Option<Value> {
        // A command works on what the project holds, so what was typed is saved before it runs.
        if !self.save() {
            return None;
        }
        let result = self.session.as_ref()?.execute(command, &params);
        match result {
            Ok(value) => {
                self.problem = None;
                self.refresh();
                Some(value)
            }
            Err(failure) => {
                self.problem = Some(failure.0);
                None
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if self.polled.elapsed() >= POLL {
            self.polled = Instant::now();
            if self.session.as_ref().is_some_and(|s| s.project().stamp() != self.stamp) {
                self.refresh();
            }
        }
        if self.session.is_some() {
            ctx.request_repaint_after(POLL);
        }
    }

    fn search(&mut self) {
        if self.query == self.searched {
            return;
        }
        self.searched = self.query.clone();
        let query = self.query.trim().to_string();
        if query.chars().count() < 2 {
            self.found = None;
            return;
        }
        let Some(Ok(result)) = self.session.as_ref().map(|s| s.execute("search_project", &json!({ "query": query }))) else {
            self.found = None;
            return;
        };
        let id_of = |path: &Value| self.listing.iter().find(|l| Some(label(&l.path).as_str()) == path.as_str()).map(|l| l.id.clone());
        let rows = |kind: &str| result[kind]["rows"].as_array().cloned().unwrap_or_default();
        // A hit's place, from the command's 1-based positions.
        let span = |row: &Value| {
            let at = |key: &str| row[key].as_u64().and_then(|n| usize::try_from(n).ok()).and_then(|n| n.checked_sub(1));
            Some((at("start")?, at("end")?))
        };
        let mut found = Vec::new();
        for row in rows("plasmids") {
            if let Some(id) = id_of(&row["path"]) {
                found.push(session_nav::Found {
                    id,
                    span: None,
                    title: row["name"].as_str().unwrap_or_default().to_string(),
                    detail: format!("plasmid · {}", row["path"].as_str().unwrap_or_default()),
                });
            }
        }
        for row in rows("features") {
            if let Some(id) = id_of(&row["plasmid"]) {
                let detail = format!(
                    "{} in {}, {}..{}",
                    row["type"].as_str().unwrap_or("feature"),
                    row["plasmid"].as_str().unwrap_or_default(),
                    row["start"],
                    row["end"]
                );
                found.push(session_nav::Found { id, span: span(&row), title: row["feature"].as_str().unwrap_or_default().to_string(), detail });
            }
        }
        for row in rows("sequence") {
            if let Some(id) = id_of(&row["plasmid"]) {
                let detail = format!("these bases, {}..{}, {} strand", row["start"], row["end"], row["strand"].as_str().unwrap_or_default());
                found.push(session_nav::Found { id, span: span(&row), title: row["plasmid"].as_str().unwrap_or_default().to_string(), detail });
            }
        }
        self.found = Some(found);
    }

    fn work(&mut self, ui: &mut egui::Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, theme::CELL);
        let Some(c) = self.opened.as_ref().map(|o| &o.construct).or(self.loose.as_ref()) else {
            if self.session.is_some() {
                let hint = "Choose a plasmid on the left, or drop a sequence file here to add it";
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, hint, theme::sans(13.0), theme::MUTED);
                return;
            }
            self.projects_page(ui, rect);
            return;
        };
        // What the layers let the map and the rows draw of it.
        let drawn = pane::drawn(c, self.pane.layers);
        let c = drawn.as_ref();
        // The editor pane's column: the map, the split handle, the rows, the navigator seam.
        let key = self.opened.as_ref().map_or_else(|| "loose".to_string(), |o| o.id.as_str().to_string());
        let length = i64::try_from(c.len()).unwrap_or(i64::MAX);
        self.pane.keep_strip();
        let layout = bios_layout::pane::pane_layout(f64::from(rect.height()), self.pane.focus, self.pane.map_frac, length > 0);
        let sites = self.pane.sites(self.revision, c).to_vec();
        let mut view = *self.pane.view(&key, c);
        let map_band = layout.map.map(|b| pane::band_rect(rect, b));
        // The corner label is placed from the map's top left, which is the pane's in every view.
        let map_rect = map_band.unwrap_or(Rect::from_min_size(rect.min, vec2(rect.width(), 0.0)));
        let rows_rect = layout.rows.map(|b| pane::band_rect(rect, b)).unwrap_or(map_rect);

        let on_map = map::MapCursor { caret: self.cursor.caret, selection: self.cursor.selection };
        // A hand on the map: a wheel over it or a press on it (`onWheelCapture`, `onPointerDownCapture`).
        if let Some(band) = map_band
            && ui.input(|i| {
                i.pointer.hover_pos().is_some_and(|at| band.contains(at))
                    && (i.smooth_scroll_delta != egui::Vec2::ZERO || i.zoom_delta() != 1.0 || i.pointer.any_pressed())
            })
        {
            self.pane.map_hand = ui.input(|i| i.time);
        }
        let revision = self.revision;
        // A picture is taken with the caret held steady, so that it is in it.
        let cursor = rows::Cursor { moved: self.cursor.moved.filter(|_| self.shot.is_none() && !self.shooting), ..self.cursor };
        // Brought into view once; after that the person scrolls where they like.
        self.cursor.reveal = None;
        self.oligos.refresh(self.opened.as_ref().map(|o| &o.id), c, revision);
        let lit = rows::lit_feature(ui.ctx());
        let seamed = layout.seam.is_some();
        // The map's pane: the map, or past the letters in the split, the strip.
        let mut strip = self.pane.strip;
        let mut strip_out = rows::StripOut::default();
        let map_gesture = match (map_band, strip.as_mut()) {
            (Some(band), Some(strip)) => {
                let options = row_options(&self.pane, &self.oligos, &key, &sites, seamed, revision, lit, None);
                strip_out = rows::show_strip(ui, band, c, revision, strip, &mut self.strip_plan, &cursor, options);
                None
            }
            (band, _) => band.and_then(|band| map::show_moving(ui, band, c, &on_map, &mut view.map, &mut self.pane.motion, &sites, self.pane.layers)),
        };
        if let Some(handle) = layout.handle.map(|b| pane::band_rect(rect, b))
            && let Some(frac) = pane::split_handle(ui, handle, rect, layout.seam.map_or(0.0, |s| s.height() as f32))
        {
            self.pane.map_frac = frac;
        }
        if let Some(seam) = layout.seam.map(|b| pane::band_rect(rect, b)) {
            let scene = map::scene_with(c, self.pane.layers.splice_scan);
            let features: Vec<navigator::Feature> = scene
                .features
                .iter()
                .enumerate()
                .map(|(i, f)| navigator::Feature {
                    nav: NavFeature { start: f.start, end: f.end, rank: scene.priorities.get(i).copied().unwrap_or(0.0) },
                    color: theme::role_color(f.role),
                })
                .collect();
            let cuts: Vec<i64> = sites.iter().map(|s| i64::try_from(s.top_cut).unwrap_or(0)).collect();
            // The seam draws the strip's window while the strip is up, and moves it (`seamWindow`).
            match strip.as_mut() {
                Some(strip) => {
                    navigator::show(ui, seam, length, &features, &cuts, &mut strip.vp, false);
                    strip.vp = bios_layout::strip::apply(strip.vp, bios_layout::strip::min_width(f64::from(rect.width()), length));
                }
                None => navigator::show(ui, seam, length, &features, &cuts, &mut view.map.vp, view.map.ring),
            }
        }
        *self.pane.view(&key, c) = view;
        self.pane.strip = strip;
        let now = ui.input(|i| i.time);
        // The strip reports its window as the map reports its own, so the rows below follow it.
        let pane_window = strip.map_or(view.map.vp, |s| s.vp);
        let center_on = map_band.and_then(|band| self.pane.follow_map(pane_window, f64::from(band.width()), length, now, &cursor));
        if let Some(vp) = strip_out.handed_back {
            self.pane.strip_handed_back(&key, vp, f64::from(rect.width()), length, now);
        }
        // The name, length and description in the corner. Something typed and not saved yet is
        // the dot on its tab.
        plasmid_title::show(ui, map_rect, c);

        let plan = &mut self.plan;
        let options = row_options(&self.pane, &self.oligos, &key, &sites, seamed, revision, lit, center_on);
        let gesture = layout.rows.and_then(|_| {
            ui.scope_builder(UiBuilder::new().max_rect(rows_rect), |ui| {
                ui.set_clip_rect(rows_rect);
                rows::show_with(ui, c, revision, plan, &cursor, options)
            })
            .inner
        });
        // The floating tool row, standing on the seam.
        let selection = self.cursor.selection.and_then(|(a, b)| Some((i64::try_from(a).ok()?, i64::try_from(b).ok()?)));
        let fold = bios_layout::fold_target::fold_target(length, c.circular, selection);
        let state = self.menu_state();
        let tools = canvas_tools::Tools {
            can_undo: state.undo,
            can_redo: state.redo,
            focus: self.pane.focus,
            style: view.map_style,
            // Folding is not in Bios native, so Fold stays grey even where the web could fold.
            fold_reason: Some(fold.reason.unwrap_or_else(|| "Fold — secondary structure, not in this version of Bios yet.".to_string())),
            open: self.pane.panel,
            side: self.pane.side,
        };
        let tools_bottom = rect.top() + bios_layout::pane::tools_bottom(f64::from(rect.height()), layout.seam.is_some()) as f32;
        let (tool, placed) = canvas_tools::show(ui, rect, tools_bottom, &tools);
        if self.pane.panel == Some(canvas_tools::Panel::Layers) {
            let splice_hits = c.features.iter().filter(|f| bios_layout::overview::display::is_splice_scan(&f.name)).count();
            match view_tab::show_with(ui, placed.layers, self.pane.layers, splice_hits) {
                Some(view_tab::Outcome::Changed(layers)) => self.set_layers(layers),
                Some(view_tab::Outcome::Close) => self.pane.panel = None,
                None => {}
            }
        }
        if self.pane.panel == Some(canvas_tools::Panel::Enzymes) {
            match enzymes_menu::show(ui, placed.row.center().x, placed.enzymes, self.pane.cut_view, &sites, self.pane.layers.cut_names) {
                Some(enzymes_menu::Outcome::Changed(view)) => self.pane.cut_view = view,
                Some(enzymes_menu::Outcome::Close) => self.pane.panel = None,
                Some(enzymes_menu::Outcome::OpenList) => {
                    self.pane.panel = None;
                    self.pane.side = Some(bios_ui::side_panel::Side::Enzymes);
                }
                Some(enzymes_menu::Outcome::NamesOn) => self.set_layers(view_tab::Layers { cut_names: true, ..self.pane.layers }),
                None => {}
            }
        }
        // Zoomed out past the letters, the rows hand back to the map.
        if let Some(rows::Gesture::ZoomedOut { base }) = gesture {
            self.pane.rows_handed_back(&key, base, f64::from(rect.width()), length, now);
        }
        // The rows and the strip are pointed at alike: a press places the caret, a bar opens its card.
        let opened = self.opened.is_some();
        for mut gesture in [gesture, strip_out.gesture].into_iter().flatten().filter(|_| opened) {
            if let rows::Gesture::Span { feature, .. } = &mut gesture
                && let Some(file) = self.opened.as_ref().and_then(|o| pane::feature_in_file(&o.construct, self.pane.layers, *feature))
            {
                *feature = file;
            }
            self.gesture(gesture, now);
        }
        if let Some(gesture) = map_gesture.filter(|_| self.opened.is_some()) {
            let now = ui.input(|i| i.time);
            self.map_gesture(gesture, now);
        }
        match tool {
            Some(canvas_tools::Action::Back) => self.command(menus::UNDO),
            Some(canvas_tools::Action::Forward) => self.command(menus::REDO),
            Some(canvas_tools::Action::Style(style)) => self.pane.set_style(&key, style),
            Some(canvas_tools::Action::Focus(focus)) => self.pane.focus = focus,
            Some(canvas_tools::Action::Panel(canvas_tools::Panel::Contents)) => self.pane.toggle_side(bios_ui::side_panel::Side::Contents),
            Some(canvas_tools::Action::Panel(which)) => self.pane.panel = if self.pane.panel == Some(which) { None } else { Some(which) },
            None => {}
        }
        self.side_panels(ui, rect, tools_bottom);
        // The selection's read-out, the feature panel, the menu and Find (floating.rs).
        self.floating(ui, rect, rows_rect);
    }

    fn take_shot(&mut self, ctx: &egui::Context) {
        if self.shooting {
            // The picture is asked for; frames have to keep coming until it has been delivered,
            // and a window with nothing to animate would otherwise sit still.
            ctx.request_repaint();
            return;
        }
        let Some(path) = self.shot.clone() else {
            return;
        };
        self.frames = self.frames.saturating_add(1);
        // A few frames let the layout settle; the wait lets the picture show what happens after
        // the window opens.
        // A picture of the projects page waits for its cards.
        let page_loading = self.session.is_none() && self.loose.is_none() && self.page.loading();
        if self.frames >= 4 && self.started.elapsed() >= self.shot_after && !page_loading {
            self.shot = None;
            self.shooting = true;
            let closer = ctx.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
                if let Err(why) = save_png(&path, &image) {
                    eprintln!("could not save {}: {why}", path.display());
                }
                closer.send_viewport_cmd(egui::ViewportCommand::Close);
            })));
        }
        ctx.request_repaint();
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.width() as u32, image.height() as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.as_raw())?;
    Ok(())
}

impl App {
    /// What was dropped on the window this frame. Verify takes every file of a drop, a run being
    /// many files; on the projects page sequence files start a project; anywhere else each is opened.
    fn take_drop(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if self.place == Place::Verify && self.session.is_some() {
            // Verify takes every file of a drop: a run is many files.
            self.verify_dropped(dropped);
        } else if self.place == Place::Build && self.session.is_some() && self.build.folder_open().is_none() && !dropped.is_empty() {
            // IN BUILD, A DROPPED FILE IS AN INSERT. Everywhere else it is a plasmid joining the
            // project: the same gesture, read by where it lands, which is what the web does too.
            self.build.take_files(&dropped);
        } else {
            // Sequence files dropped on the projects page start a project from them; anything else
            // dropped (a project folder, a file onto an open project) is opened.
            let on_page = self.session.is_none() && self.loose.is_none();
            if on_page && !dropped.is_empty() && dropped.iter().all(|p| p.is_file()) {
                self.start_from_files(&dropped);
            } else if !dropped.is_empty() {
                self.dropped(dropped);
            }
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Menus::System(menu) = &mut self.menus {
            menu.raw_input(raw_input);
        }
        if let Some(script) = self.script.as_mut() {
            script.feed(ctx, raw_input);
        }
        // `--hover x,y`: the pointer stands there, for a picture of what hovering shows. Moved
        // there only when it is elsewhere, so it stands still and a tooltip can come up.
        if let Some(at) = self.oligos.hover_at.filter(|at| ctx.input(|i| i.pointer.latest_pos()) != Some(*at)) {
            raw_input.events.push(egui::Event::PointerMoved(at));
        }
    }

    fn on_exit(&mut self) {
        self.save();
        if let Some(store) = &mut self.prefs {
            // Nothing can be shown any more; what could not be written is only not remembered.
            let _ = store.flush();
        }
        // An update installed while the window was open: the new version opens once this one ends.
        bios_update::relaunch_after_exit();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.frame_ui(ui, frame);
        // `BIOS_FRAME_LOG=1`: every frame that kept the window busy longer than one refresh is
        // named, with what it was showing, to find what makes the window slow.
        if *FRAME_LOG && started.elapsed() > SLOW_FRAME {
            let shown = self
                .opened
                .as_ref()
                .map_or_else(|| if self.session.is_some() { "project" } else { "projects page" }.to_string(), |o| o.construct.name.clone());
            eprintln!("slow frame: {:.1} ms ({shown})", started.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

/// How long a frame may take before `BIOS_FRAME_LOG` names it: one refresh at 60 Hz.
const SLOW_FRAME: Duration = Duration::from_millis(16);
static FRAME_LOG: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("BIOS_FRAME_LOG").is_some_and(|v| v == "1"));

impl App {
    fn frame_ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.styled {
            theme::install_fonts(ui.ctx());
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = theme::CANVAS;
            ui.ctx().set_visuals(visuals);
            self.styled = true;
            ui.ctx().request_discard("fonts installed");
            return;
        }
        self.take_drop(ui.ctx());
        if self.dialogs.is_none() {
            let wake = ui.ctx().clone();
            self.dialogs = Some(Dialogs::new(Arc::new(move || wake.request_repaint())));
        }
        let answers = self.dialogs.as_mut().map(Dialogs::take_answers).unwrap_or_default();
        for (then, answer) in answers {
            self.answered(then, answer);
        }
        let mut full = ui.ctx().content_rect();
        for command in self.menu_commands(ui, &mut full) {
            self.command(&command);
        }
        self.keys(ui.ctx());
        // After the menus and the keys, either of which may have copied something this frame.
        if let Some(text) = self.to_clipboard.take() {
            ui.ctx().copy_text(text);
        }
        self.autosave(ui.ctx());
        self.guard_quit(ui.ctx());
        self.remember_tabs();
        if let Some(store) = &mut self.prefs {
            // Settings are written a moment after they change; an idle window has to be woken for it.
            if let Some(deadline) = store.tick(Instant::now()).next_deadline {
                ui.ctx().request_repaint_after(deadline.saturating_duration_since(Instant::now()));
            }
        }
        self.poll(ui.ctx());
        self.search();
        self.updates.frame(ui.ctx());

        let has_project = self.session.is_some();
        let on_page = !has_project && self.loose.is_none();
        let nav = ((has_project || on_page) && !self.panel_hidden).then_some(self.panel_width);
        let changes = (self.opened.is_some() && self.changes_shown).then_some(changes::WIDTH);
        let columns = app_layout::columns(full, nav, has_project && !self.tabs.is_empty() && self.place != Place::Verify, changes);
        let work_rect = columns.cell;
        // The head band is the title bar (styles.css `.is-desktop .canvas-head`): its empty part
        // moves the window and double-clicking it zooms, as the system setting says. Laid down
        // before anything is drawn in the band, so the tabs and buttons there still take clicks.
        craft_window::title_bar::empty_area(ui, Rect::from_min_size(full.min, vec2(full.width(), theme::HEAD_H)));
        // `trafficLightPosition: { x: 19, y: 12 }` (main.cjs): 14pt buttons, centred in the head.
        frame.set_traffic_lights_position(ui.ctx(), egui::Rangef::new(full.top(), full.top() + theme::HEAD_H), 19.0);

        match (has_project, self.place) {
            (true, _) if self.work_hidden => {
                ui.painter().rect_filled(work_rect, 0.0, theme::CANVAS);
            }
            (true, Place::Build) => self.build_place(ui, work_rect),
            (true, Place::Verify) => {
                let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                self.verify_work(ui, work_rect, hovering);
            }
            (true, place) if place != Place::Edit => empty_place(ui, work_rect, place),
            _ => self.work(ui, work_rect),
        }

        if let Some(tabs_rect) = columns.tabs.filter(|_| !self.work_hidden) {
            let dirty = self.opened.as_ref().filter(|o| o.editing.dirty_since().is_some()).map(|o| o.id.clone());
            let tabs: Vec<tab_strip::Tab> = self
                .tabs
                .iter()
                .map(|id| {
                    let listed = self.listing.iter().find(|l| l.id == *id);
                    tab_strip::Tab {
                        name: listed.map_or("", |l| l.name.as_str()),
                        circular: listed.is_some_and(|l| l.circular),
                        dirty: dirty.as_ref() == Some(id),
                    }
                })
                .collect();
            let active = self.opened.as_ref().and_then(|o| self.tabs.iter().position(|t| *t == o.id));
            // With the sidebar away the tabs start right of the traffic lights and its toggle.
            let lead = if self.panel_hidden { sidebar::TOGGLE_END + 8.0 - (tabs_rect.left() - full.left()) } else { 0.0 };
            let pressed = self.tab_strip.show(ui, tabs_rect, lead.max(0.0), &tabs, active, theme::PLACE_EDIT);
            match pressed {
                // A tab's click lands in Edit (TabStrip.tsx: `selectTab`, then `setMode("edit")`).
                Some(tab_strip::Pressed::Select(which)) => {
                    self.go_to_place(Place::Edit);
                    self.select_tab(which);
                }
                Some(tab_strip::Pressed::Close(which)) => self.close_tab(which),
                Some(tab_strip::Pressed::Move(from, to)) => tab_strip::move_tab(&mut self.tabs, from, to),
                Some(tab_strip::Pressed::CloseOthers(which)) => {
                    if let Some(keep) = self.tabs.get(which).cloned() {
                        self.open_construct(&keep);
                        self.tabs.retain(|t| *t == keep);
                    }
                }
                Some(tab_strip::Pressed::CloseRight(which)) => {
                    let front_goes = self.opened.as_ref().and_then(|o| self.tabs.iter().position(|t| *t == o.id)).is_some_and(|at| at > which);
                    if front_goes {
                        self.select_tab(which);
                    }
                    self.tabs.truncate(which + 1);
                }
                None => {}
            }
        }

        if let (true, Some(nav_rect)) = (has_project, columns.nav) {
            self.sidebar(ui, nav_rect);
        } else if has_project && self.panel_hidden && sidebar::toggle(ui, full.left(), full.top(), true) {
            // Put away, the sidebar's toggle stays where it was, at the tab strip's left end.
            self.command(menus::TOGGLE_PANEL);
        }
        // The sidebar runs from the window's left edge now, so the projects rail starts under the
        // head band and its traffic lights.
        if let (true, Some(rail_rect)) = (on_page, columns.nav)
            && let Some(filter) = {
                sidebar::ground(ui, rail_rect);
                let below = Rect::from_min_max(egui::pos2(rail_rect.left(), rail_rect.top() + theme::HEAD_H), rail_rect.max);
                bios_ui::projects_home::rail(ui, below, self.page.filter, self.page_counts())
            }
        {
            self.page.filter = filter;
        }

        let action = self
            .opened
            .as_ref()
            .zip(columns.changes)
            .and_then(|(o, changes_rect)| changes::show(ui, changes_rect, &o.history, &o.lineage).map(|a| (o.id.clone(), a)));
        match action {
            Some((id, Action::Restore(version))) => {
                self.run("restore_version", json!({ "plasmid": id.as_str(), "to_version": version }));
            }
            Some((id, Action::Undo)) => {
                self.run("undo_last_edit", json!({ "plasmid": id.as_str() }));
            }
            Some((_, Action::Open(path))) => {
                if let Some(id) = self.listing.iter().find(|l| label(&l.path) == path).map(|l| l.id.clone()) {
                    self.open_construct(&id);
                }
            }
            None => {}
        }

        if let Some((what, mut prompt)) = self.prompt.take() {
            match ask::prompt(ui, work_rect.left_top() + vec2(16.0, 58.0), &mut prompt) {
                Some(ask::Outcome::Done) => self.asked(&what, &prompt.value()),
                Some(ask::Outcome::Cancelled) => {}
                None => self.prompt = Some((what, prompt)),
            }
        }

        if let Some(problem) = self.problem.as_ref().or(self.notice.as_ref()) {
            let bar = Rect::from_min_max(pos2(work_rect.left(), work_rect.bottom() - 30.0), work_rect.max);
            ui.painter().rect_filled(bar, 0.0, theme::CANVAS);
            ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, theme::HAIRLINE));
            ui.painter().with_clip_rect(bar).text(
                bar.left_center() + vec2(16.0, 0.0),
                Align2::LEFT_CENTER,
                problem,
                theme::sans(12.0),
                if self.problem.is_some() { theme::DANGER_TEXT } else { theme::MUTED },
            );
        }
        self.intro_layer(ui, work_rect);
        self.quick_open_palette(ui, work_rect);
        if let Some(docs) = self.docs.as_mut()
            && docs.show(ui, ui.ctx().content_rect()).is_some()
        {
            self.docs = None;
        }
        self.settings_window(ui);
        if let Some(dialogs) = &mut self.dialogs {
            dialogs.show_pending(Some(&*frame));
        }
        self.drop_overlay(ui, full);
        #[cfg(target_os = "macos")]
        craft_window::title_bar::sync_movable(ui.ctx(), &mut self.window_movable);
        self.take_shot(ui.ctx());
        if let Some(script) = self.script.as_mut() {
            script.record(ui.ctx());
        }
    }
}

impl App {
    /// The Build place: the map, the console and the folder's page, and the few things Build
    /// cannot do for itself — opening a plasmid, the clipboard, saying what went wrong.
    fn build_place(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(session) = &self.session else {
            return;
        };
        // On a folder's page the backbone in front is nobody's business: the page is about the
        // folder's own frozen copy.
        let construct = self.build.folder_open().is_none().then(|| self.opened.as_ref().map(|o| o.construct.clone())).flatten();
        let plasmid = self.opened.as_ref().and_then(|o| self.listing.iter().find(|l| l.id == o.id)).map(|l| label(&l.path));
        let root = session.project().root().to_path_buf();
        let ctx = build::Ctx { backbone: construct.as_ref(), plasmid, listing: &self.listing, folders: &self.folders, root: &root };
        let requests = self.build.show(ui, rect, session, &ctx);
        for request in requests {
            match request {
                build::Request::Refresh => self.refresh(),
                build::Request::Open(path) => {
                    if let Some(id) = self.listing.iter().find(|l| label(&l.path) == path).map(|l| l.id.clone()) {
                        self.open_construct(&id);
                        if !self.tabs.contains(&id) {
                            self.tabs.push(id);
                        }
                    }
                }
                build::Request::Copy(text) => self.to_clipboard = Some(text),
                build::Request::Say(said) => self.problem = Some(said),
                build::Request::AskForFiles => self.ask(FileDialog::open_many().title("Sequences to put in"), Then::BuildFiles),
            }
        }
    }
}

/// What the work column says in a place that is not built here yet: one sentence, rather than a
/// place that silently does nothing.
fn empty_place(ui: &egui::Ui, rect: Rect, place: Place) {
    ui.painter().rect_filled(rect, 0.0, theme::CELL);
    let said = match place {
        Place::Edit => return,
        Place::Build => "Build is not in this version of Bios yet.",
        Place::Verify => "Verify is not in this version of Bios yet.",
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, said, theme::sans(13.0), theme::MUTED);
}

/// Where `bios-cli` is: beside the app's own executable, where it ships; else whatever the PATH has.
fn cli_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("bios-cli")))
        .filter(|cli| cli.exists())
        .map_or_else(|| "bios-cli".to_string(), |cli| cli.display().to_string())
}

/// The line that connects Claude Code to this copy of Bios: Help > Connect an Agent and the
/// welcome sheet both give this one.
fn connect_line() -> String {
    format!("claude mcp add --scope user bios -- \"{}\" mcp", cli_path())
}

/// The line that connects Claude Code to Bios, and what to tell the person about it and about
/// connecting any other agent.
fn connect_agent() -> (String, String) {
    let cli = cli_path();
    let line = connect_line();
    let config = format!("{{ \"mcpServers\": {{ \"bios\": {{ \"command\": \"{cli}\", \"args\": [\"mcp\"] }} }} }}");
    let description = format!(
        "Any agent that speaks MCP can work on the folders you open in Bios, and any other folder it is given: read, edit, design primers, simulate PCR and more. Everything it changes shows here as it happens and can be undone.\n\n\
         For Claude Code, this line is now on your clipboard. Paste it into a terminal:\n{line}\n\n\
         For Claude Desktop, Cursor or another app, add Bios to its MCP settings:\n{config}"
    );
    (line, description)
}

/// Puts the menus in the system menu bar, where the system has one and lets us.
#[cfg(target_os = "macos")]
fn system_menus(ctx: &egui::Context, state: menus::State, recent: &[String], keys: &keys::Keys) -> Option<NativeMenu> {
    craft_menu::mac::install(ctx, &craft_menu::mac_layout(&menus::bar(state, recent, &[], keys), &LayoutOptions::new(APP_NAME)).bar)
}

#[cfg(not(target_os = "macos"))]
fn system_menus(_: &egui::Context, _: menus::State, _: &[String], _: &keys::Keys) -> Option<NativeMenu> {
    None
}

const USAGE: &str = "\
bios [<project folder or sequence file>] [--open <plasmid>] [--select <from>..<to> [--ask feature|primer] [--menu] | --caret <after base>] [--feature <name> [--acts]] [--layer <label>] [--find <text>] [--search <text>]
     [--panel contents|enzymes|layers|dial [--enzyme-search <text>]] [--window-menu] [--hide-panel] [--zoom <width>[@<centre>]] [--flywheel <stage>[:<n>]] [--focus map|split|sequence]
     [--shot <file.png> [--after <seconds>]]

  --open     open this plasmid of the project (its path, name or id)
  --select   start with these bases of the open plasmid selected, counted from 1
  --caret    start with the caret after this base (0 is before the first)
  --ask      with --select, start with the sheet for a new feature or primer open
  --feature  start with this feature of the open plasmid selected and its card open
  --acts     with --feature, start with a new line of its Acts on open
  --layer    switch this row of the layers popover the other way (its label, as Interactions)
  --find     start with Find open in the open plasmid, this typed in it
  --binder   hold the oligo that anneals to <from>..<to> of the open plasmid as a binder
  --hover    keep the pointer at <x>,<y> in the window's points, for a picture
  --menu     with --select, start with the selection's menu open
  --search   start with this in the search box
  --place    start in this place: edit, build or verify
  --build-inserts  with --place build: take the sequences in this file as the inserts
  --build-file     with --place build: take this file as an insert, as dropping it does
  --build-compute  with --build-inserts: design them before the picture is taken
  --build-fill     with --place build: answer the cut and give the list the screen
  --build-after    with --build-compute: draw what the design makes, not the backbone
  --build-folder   with --place build: open this build folder's page
  --settings start with Settings open at appearance, keyboard or about
  --go-to    start with the ⌘P palette open, this typed in it
  --docs     start with the documentation open at this page (what-bios-is, feature, …) or its contents
  --intro    start with a sheet on how things work open: build, verify or welcome
  --verify-open    with --place verify: open this verification's board, or folder::sample for an alignment (@N on its Nth change)
  --translate  with --select, float the selection's amino acids in this frame: +1, +2, +3, -1, -2 or -3
  --drop     drop these files on the window, comma-separated, as a drag from the Finder does
  --dropping draw the drop overlay, as while files are held over the window
  --panel    start with a panel open: contents, enzymes, layers or dial (with --enzyme-search, a query in it)
  --hide-panel   start with the sidebar put away
  --window-menu  draw the menus in the window, also where the system has a menu bar
  --zoom     start the map this wide, a fraction of the plasmid (0.45), or wide@centre (0.45@0.2), the rows following it
  --flywheel start with the ring caught as a flywheel: spin:<rpm>, leaving:<0..1>, gone or returning:<0..1>
  --focus    start with the map only, map and sequence, or the sequence only
  --shot     save a picture of the window to this file and quit
  --after    wait this many seconds before taking the picture
  --input    feed this script of pointer, button, wheel and pinch steps into the window (see script.rs)
  --record   save a picture every so often while the script runs, <dir>@<seconds>, then close";

/// What the Build flags asked for, so `main` stays one sentence per thing it sets up.
#[derive(Default)]
struct BuildStart {
    inserts: Option<String>,
    file: Option<String>,
    compute: bool,
    fill: bool,
    after: bool,
    folder: Option<String>,
}

/// The Build place as a picture can reach it: what to put in, whether to design it, which half of
/// the screen the map has, and a folder's page.
fn start_build(app: &mut App, start: BuildStart) {
    if let Some(file) = start.inserts {
        match std::fs::read_to_string(&file) {
            Ok(text) => app.build.seed(&text, start.compute),
            Err(e) => app.problem = Some(format!("{file}: {e}")),
        }
    }
    if let Some(file) = start.file {
        // The same door a dropped file goes through.
        app.build.take_files(&[PathBuf::from(file)]);
    }
    if start.fill {
        app.build.fill();
    }
    if start.after {
        app.build.show_product();
    }
    if let Some(folder) = start.folder {
        app.build.show_folder(&folder);
    }
}

/// What the editor pane asks of the rows and of the strip beyond the construct itself.
#[allow(clippy::too_many_arguments)]
fn row_options<'a>(
    pane: &'a pane::Pane,
    oligos: &'a oligos::Oligos,
    key: &str,
    sites: &'a [CutSite],
    seamed: bool,
    revision: u64,
    lit: Option<usize>,
    center_on: Option<usize>,
) -> rows::Options<'a> {
    rows::Options {
        run_off: bios_layout::pane::rows_run_off(seamed) as f32,
        sites: Some(sites),
        layers: pane.layers,
        lit,
        binders: oligos.sites(),
        hits: oligos.aligner.hits(revision),
        oligo_key: oligos.key(),
        translations: pane.translations_of(key),
        center_on,
    }
}

/// The map's motion as a picture can catch it: the window at a width (mid-unroll, say), and the ring
/// held at one moment of the flywheel.
fn start_motion(app: &mut App, zoom: Option<&str>, spin: Option<&str>, focus: Option<&str>) {
    let Some(opened) = app.opened.as_ref() else { return };
    let key = opened.id.as_str().to_string();
    let c = opened.construct.clone();
    if let Some(focus) = focus {
        // The construct is brought to the front first, which would otherwise put it back to split.
        app.pane.view(&key, &c);
        match focus {
            "map" => app.pane.focus = Focus::Map,
            "split" => app.pane.focus = Focus::Split,
            "sequence" => app.pane.focus = Focus::Sequence,
            other => app.problem = Some(format!("--focus takes map, split or sequence, not {other}")),
        }
    }
    if let Some(zoom) = zoom {
        let (width, center) = zoom.split_once('@').map_or((zoom, None), |(w, c)| (w, Some(c)));
        let width = width.trim().parse::<f64>().ok().filter(|w| *w > 0.0 && *w <= 1.0);
        let center = center.map(|c| c.trim().parse::<f64>().ok().filter(|c| c.is_finite()));
        match (width, center) {
            (Some(width), None | Some(Some(_))) => {
                let view = app.pane.view(&key, &c);
                let vp = set_width(view.map.vp, width);
                let vp = center.flatten().map_or(vp, |c| set_center(vp, c));
                view.map.vp = navigate::normalize(vp, view.map.ring);
                // As if a hand had zoomed it there: the rows follow, as they do after a zoom.
                app.pane.map_hand = f64::MAX;
            }
            _ => app.problem = Some(format!("--zoom takes a width from 0 to 1, as 0.45 or 0.45@0.2, not {zoom}")),
        }
    }
    if let Some(spin) = spin {
        let (stage, value) = spin.split_once(':').map_or((spin, "0"), |(s, v)| (s, v));
        let now = 10.0;
        match value.trim().parse::<f64>().ok().and_then(|v| flywheel::pose(stage.trim(), v, now)) {
            Some(fly) => {
                app.pane.view(&key, &c);
                app.pane.motion.fly = fly;
                app.pane.motion.frozen = true;
                app.pane.motion.now = now;
            }
            None => app.problem = Some(format!("--flywheel takes spin:<rpm under 1300>, leaving:<0..1>, gone or returning:<0..1>, not {spin}")),
        }
    }
}

fn main() -> eframe::Result {
    bios_update::set_up(bios_update::After::ThisWindow);
    let mut args = std::env::args().skip(1);
    let (mut path, mut shot, mut open, mut search, mut after, mut window_menu) = (None, None, None, None, 0.0, false);
    let mut hide_panel = false;
    let (mut select, mut caret, mut asking, mut feature, mut find) = (None, None, None, None, None);
    let (mut app_menu, mut acts, mut layers) = (false, false, Vec::new());
    let (mut place, mut build_inserts, mut build_folder, mut build_compute) = (None, None, None, false);
    let (mut build_fill, mut build_after) = (false, false);
    let mut build_file = None;
    let (mut binder, mut hover) = (None, None);
    let (mut zoom, mut spin, mut focus) = (None, None, None);
    let mut settings = None;
    let mut go_to = None;
    let mut intro = None;
    let mut docs = None;
    let mut verify_open: Option<String> = None;
    let (mut panel, mut enzyme_search, mut translate) = (None, None, None);
    let (mut drop_files, mut dropping) = (None, false);
    let (mut input, mut record): (Option<String>, Option<String>) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--shot" => shot = args.next().map(PathBuf::from),
            "--open" => open = args.next(),
            "--search" => search = args.next(),
            "--window-menu" => window_menu = true,
            "--hide-panel" => hide_panel = true,
            "--input" => input = args.next(),
            "--record" => record = args.next(),
            "--select" => select = args.next(),
            "--ask" => asking = args.next(),
            "--feature" => feature = args.next(),
            "--acts" => acts = true,
            "--layer" => layers.extend(args.next()),
            "--find" => find = args.next(),
            "--binder" => binder = args.next(),
            "--hover" => hover = args.next().and_then(|xy| oligos::point(&xy)),
            "--menu" => app_menu = true,
            "--settings" => settings = args.next(),
            "--go-to" => go_to = args.next(),
            "--intro" => intro = args.next(),
            "--docs" => docs = args.next(),
            "--panel" => panel = args.next(),
            "--drop" => drop_files = args.next(),
            "--dropping" => dropping = true,
            "--translate" => translate = args.next(),
            "--enzyme-search" => enzyme_search = args.next(),
            "--caret" => caret = args.next().and_then(|n| n.parse::<usize>().ok()),
            "--place" => place = args.next(),
            "--verify-open" => verify_open = args.next(),
            "--zoom" => zoom = args.next(),
            "--flywheel" => spin = args.next(),
            "--focus" => focus = args.next(),
            "--build-inserts" => build_inserts = args.next(),
            "--build-folder" => build_folder = args.next(),
            "--build-compute" => build_compute = true,
            "--build-file" => build_file = args.next(),
            "--build-fill" => build_fill = true,
            "--build-after" => build_after = true,
            "--after" => after = args.next().and_then(|s| s.parse::<f64>().ok()).filter(|s| s.is_finite() && *s >= 0.0).unwrap_or(0.0),
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    // A picture is taken without touching what the person's own Bios remembers.
    let picture = shot.is_some();
    let remembered = if picture { Location::Memory } else { Location::Auto };
    let mut app = App::new(shot, Duration::from_secs_f64(after));
    match script::Script::new(input.as_deref(), record.as_deref()) {
        Ok(script) => app.script = script,
        Err(why) => {
            eprintln!("bios: {why}");
            return Ok(());
        }
    }
    app.oligos.hover_at = hover;
    // A picture or a scripted run never asks the network.
    if !picture && app.script.is_none() {
        app.updates.enable();
    }
    let (store, report) = SettingsStore::<Prefs>::open(remembered);
    if report.has_problems() {
        eprintln!("Bios could not read all of its settings and is using defaults for the rest.");
    }
    // The folders opened lately, beside the settings. A picture works on a copy of the list and
    // opens only what it names, so the page it shows is the person's and nothing is changed.
    app.recents = if picture { Recents::standard_copy() } else { Recents::standard(&store.get().projects.recent) };
    let last = if picture { None } else { app.recents.folders().first().cloned() };
    app.prefs = Some(store);
    app.arrange_from_prefs();
    app.welcome_arrive();
    // With nothing named, Bios opens where it was left.
    if let Some(path) = path.or(last) {
        app.open_path(&path);
    }
    if let Some(wanted) = open {
        let wanted = wanted.trim_end_matches(".gb").to_lowercase();
        let hit = app.listing.iter().find(|l| l.id.as_str() == wanted || label(&l.path).to_lowercase() == wanted || l.name.to_lowercase() == wanted);
        match hit.map(|l| l.id.clone()) {
            Some(id) => app.open_construct(&id),
            None => app.problem = Some(format!("no plasmid called \"{wanted}\" in this project")),
        }
    }
    if let Some(text) = search {
        app.query = text;
    }
    let length = app.opened.as_ref().map_or(0, |o| o.construct.len());
    if let Some(range) = select {
        let ends = range.split_once("..").and_then(|(from, to)| Some((from.trim().parse::<usize>().ok()?, to.trim().parse::<usize>().ok()?)));
        match ends {
            Some((from, to)) if (1..=length).contains(&from) && (1..=length).contains(&to) => {
                if let Some(opened) = app.opened.as_mut() {
                    opened.editing.select(from - 1, to - 1);
                }
                app.show_editing(None, true);
            }
            _ => app.problem = Some(format!("--select takes two bases of the open plasmid, as 120..180; it has {length}")),
        }
    } else if let Some(gap) = caret.filter(|gap| *gap <= length && app.opened.is_some()) {
        if let Some(opened) = app.opened.as_mut() {
            opened.editing.put_caret(gap);
        }
        app.anchor = Some(gap);
        app.show_editing(None, true);
    }
    if let Some(name) = feature {
        let hit = app.opened.as_ref().and_then(|o| {
            o.construct.features.iter().enumerate().find(|(_, f)| f.name.eq_ignore_ascii_case(&name)).map(|(i, f)| (i, f.start, f.end))
        });
        match hit {
            Some((index, start, end)) => {
                app.gesture(rows::Gesture::Span { feature: index, start, end }, 0.0);
                app.cursor.moved = None;
                app.cursor.reveal = Some(start);
                if let Some(card) = app.card.as_mut().filter(|_| acts) {
                    card.acts.open(None);
                }
            }
            None => app.problem = Some(format!("no feature called \"{name}\" in the open plasmid")),
        }
    }
    for label in layers {
        app.pane.layers = app.pane.layers.flipped(&label);
    }
    // The Build place, as a picture can reach it: the place itself, what to put in, whether to
    // design it, and a folder's page.
    match place.as_deref() {
        Some("build") => app.go_to_place(Place::Build),
        Some("verify") => app.go_to_place(Place::Verify),
        Some("edit") | None => {}
        Some(other) => app.problem = Some(format!("--place is edit, build or verify, not \"{other}\"")),
    }
    app.panel_hidden |= hide_panel;
    // Verify's board or an alignment, for a picture.
    verify_open.iter().for_each(|what| app.verify_open(what));
    start_build(
        &mut app,
        BuildStart { inserts: build_inserts, file: build_file, compute: build_compute, fill: build_fill, after: build_after, folder: build_folder },
    );
    if let Some(range) = binder {
        app.flag_binder(&range);
    }
    start_motion(&mut app, zoom.as_deref(), spin.as_deref(), focus.as_deref());
    app.menu_on_open = app_menu;
    match intro.as_deref() {
        Some("build") => {
            app.go_to_place(Place::Build);
            app.intro_toggle();
        }
        Some("verify") => {
            app.go_to_place(Place::Verify);
            app.intro_toggle();
        }
        Some("welcome") => app.welcome_open(),
        Some(other) => app.problem = Some(format!("--intro takes build, verify or welcome, not {other}")),
        None => {}
    }
    if let Some(page) = docs {
        app.docs = Some(bios_ui::docs::Docs::open(Some(page.as_str())));
    }
    if let Some(query) = go_to {
        app.command(menus::QUICK_OPEN);
        if let Some(palette) = app.quick_open.as_mut() {
            palette.query = query;
        }
    }
    match settings.as_deref() {
        Some("appearance") => app.open_settings(bios_ui::settings::Section::Appearance),
        Some("keyboard") => app.open_settings(bios_ui::settings::Section::Keyboard),
        Some("about") => app.open_settings(bios_ui::settings::Section::About),
        Some(other) => app.problem = Some(format!("--settings takes appearance, keyboard or about, not {other}")),
        None => {}
    }
    app.flags(side::Flags { dropping, drop_files, panel, translate, enzyme_search });
    if let Some(query) = find.filter(|_| app.opened.is_some()) {
        app.open_find(Some(query));
    }
    match asking.as_deref() {
        Some("feature") => app.command(menus::NEW_FEATURE),
        Some("primer") => app.command(menus::NEW_PRIMER),
        Some(other) => app.problem = Some(format!("--ask takes feature or primer, not {other}")),
        None => {}
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bios")
            // No native title bar (the web desktop build's `titleBarStyle: "hidden"`, main.cjs): the
            // window's content runs to its top edge, the traffic lights sit inside the 38pt head and
            // the head's empty part is the window's drag handle.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_app_id(APP_ID),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            if !window_menu {
                app.menus = system_menus(&cc.egui_ctx, app.menu_state(), &app.recent(), &app.shortcuts()).map_or(Menus::Window, Menus::System);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests;
