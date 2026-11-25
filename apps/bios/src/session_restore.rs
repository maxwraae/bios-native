//! Coming back to where you were, and not leaving work behind: a project's open tabs, and the one
//! in front, are put back when it opens again (web: workspace/tabs.ts with `restoreTabs`, the
//! `openTabs` and `activeTab` prefs), and quitting with a change that could not be saved asks
//! first (platform/unsaved.ts, `hasUnsavedWork`). Also what is in front, written down for an
//! agent working beside the window (`what_is_open`).

use std::path::Path;

use bios_project::ConstructId;
use bios_project::window::{self, Front, Selection, ShownPlasmid, ShownProject, Tab};
use bios_ui::sidebar::Place;
use craft_dialogs::{Message, MessageButtons, MessageLevel};

use crate::prefs::OpenTabs;
use crate::{App, Then, label};

impl App {
    fn tabs_key(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.project().root().canonicalize().unwrap_or_else(|_| s.project().root().to_path_buf()).display().to_string())
    }

    /// Remembers the open tabs and the one in front, when they changed.
    pub(crate) fn remember_tabs(&mut self) {
        // The main window's tabs are the ones that come back.
        if self.here != egui::ViewportId::ROOT {
            return;
        }
        let Some(root) = self.session.as_ref().map(|s| s.project().root().to_path_buf()) else {
            return;
        };
        let now = OpenTabs {
            tabs: self.ed.tabs.iter().map(|t| t.as_str().to_string()).collect(),
            front: self.opened.as_ref().map(|o| o.id.as_str().to_string()),
        };
        // Asked every frame: the key costs a look at the disk, so only a change goes on.
        let seen = Some((root, now.clone()));
        if self.tabs_seen == seen {
            return;
        }
        self.tabs_seen = seen;
        let Some(key) = self.tabs_key() else {
            return;
        };
        if let Some(store) = &mut self.prefs
            && store.get().projects.open_tabs.get(&key) != Some(&now)
        {
            store.update(|prefs| {
                if now.tabs.is_empty() {
                    prefs.projects.open_tabs.remove(&key);
                } else {
                    prefs.projects.open_tabs.insert(key, now);
                }
            });
        }
    }

    /// What the window shows now, as `what_is_open` reports it.
    pub(crate) fn front(&self) -> Front {
        let Some(session) = &self.session else {
            return Front { place: "projects".into(), ..Front::default() };
        };
        let project = session.project();
        let path_of = |id: &ConstructId| self.listing.iter().find(|l| l.id == *id).map(|l| (label(&l.path), l.name.clone()));
        let plasmid = self.opened.as_ref().map(|o| ShownPlasmid {
            id: o.id.as_str().to_string(),
            path: path_of(&o.id).map_or_else(|| o.construct.name.clone(), |(path, _)| path),
            name: o.construct.name.clone(),
            length: o.construct.len(),
            circular: o.construct.circular,
        });
        let selection = self.opened.as_ref().and_then(|o| o.editing.selection()).map(|(start, end)| Selection { start: start + 1, end: end + 1 });
        Front {
            project: Some(ShownProject { path: project.root().to_path_buf(), name: project.name().to_string() }),
            place: self.place.label().to_lowercase(),
            plasmid,
            tabs: self.ed.tabs.iter().filter_map(path_of).map(|(path, name)| Tab { path, name }).collect(),
            selection,
        }
    }

    /// Writes down what is in front, beside the list of opened folders, when it changed. A test
    /// or a picture keeps that list in memory, and writes nothing.
    pub(crate) fn publish_front(&mut self) {
        let Some(file) = self.recents.file().map(window::file_beside) else {
            return;
        };
        let now = self.front();
        // Asked every frame: only a change goes to the disk.
        if self.front_seen.as_ref() == Some(&now) {
            return;
        }
        // What could not be written is tried again on the next change.
        if window::publish(&file, &now).is_ok() {
            self.front_seen = Some(now);
        }
    }

    /// Shows what an agent asked for with `show_in_bios`: the project, the plasmid in front and
    /// the bases selected. The window is then brought forward, since the person asked to see it.
    pub(crate) fn take_show(&mut self, ctx: &egui::Context) {
        let Some(file) = self.recents.file().map(window::file_beside) else {
            return;
        };
        let Some(show) = window::take_show(&file) else {
            return;
        };
        let wanted = show.project.canonicalize().unwrap_or_else(|_| show.project.clone());
        if self.tabs_key().as_deref() != Some(wanted.display().to_string().as_str()) {
            self.open_project(&wanted);
        }
        if self.session.is_none() {
            return;
        }
        if let Some(id) = show.plasmid.as_deref().and_then(ConstructId::parse) {
            self.go_to_place(Place::Edit);
            let span = show.selection.map(|s| (s.start.saturating_sub(1), s.end.saturating_sub(1)));
            self.go_to(&id, span);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    /// The window is closing: nothing is in front any more.
    pub(crate) fn unpublish_front(&mut self) {
        if let Some(file) = self.recents.file().map(window::file_beside) {
            // Nothing can be shown any more; a window that is gone is also told by its process.
            let _ = window::closed(&file);
        }
    }

    /// Puts back the tabs that were open in the project at `root`. A plasmid that is no longer
    /// in the project is skipped in silence, wherever it stood.
    pub(crate) fn restore_tabs(&mut self, root: &Path) {
        let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display().to_string();
        let Some(saved) = self.prefs.as_ref().and_then(|store| store.get().projects.open_tabs.get(&key).cloned()) else {
            return;
        };
        let here: Vec<ConstructId> =
            saved.tabs.iter().filter_map(|t| ConstructId::parse(t)).filter(|id| self.listing.iter().any(|l| l.id == *id)).collect();
        if here.is_empty() {
            return;
        }
        let front = saved.front.as_deref().and_then(ConstructId::parse).filter(|f| here.contains(f)).or_else(|| here.last().cloned());
        self.ed.tabs = here;
        if let Some(front) = front {
            self.open_construct(&front);
        }
    }

    /// The window was asked to close. What can be saved is saved; a change that could not be
    /// saved keeps the window open and asks first.
    pub(crate) fn guard_quit(&mut self, ctx: &egui::Context) {
        if self.quitting {
            // The person said to quit anyway: close, and keep asking until the window goes.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        self.remember_tabs();
        let saved = self.save();
        let unsaved = !saved
            || self.opened.as_ref().is_some_and(|o| o.editing.dirty_since().is_some())
            || self.copies.values().any(|o| o.editing.dirty_since().is_some());
        if !unsaved {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        let name = self.opened.as_ref().map(|o| o.construct.name.clone()).unwrap_or_default();
        let message = Message {
            level: MessageLevel::Warning,
            title: format!("Quit without saving {name}?"),
            description: format!(
                "{} What was typed since the last save is lost if you quit now.",
                self.problem.clone().unwrap_or_else(|| "A change to it could not be saved.".into())
            ),
            buttons: MessageButtons::OkCancel,
        };
        if let Some(dialogs) = &mut self.dialogs {
            dialogs.ask_message(message, Then::Quit);
        }
    }
}
