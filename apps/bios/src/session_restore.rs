//! Coming back to where you were, and not leaving work behind: a project's open tabs, and the one
//! in front, are put back when it opens again (web: workspace/tabs.ts with `restoreTabs`, the
//! `openTabs` and `activeTab` prefs), and quitting with a change that could not be saved asks
//! first (platform/unsaved.ts, `hasUnsavedWork`).

use std::path::Path;

use bios_project::ConstructId;
use craft_dialogs::{Message, MessageButtons, MessageLevel};

use crate::prefs::OpenTabs;
use crate::{App, Then};

impl App {
    fn tabs_key(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.project().root().canonicalize().unwrap_or_else(|_| s.project().root().to_path_buf()).display().to_string())
    }

    /// Remembers the open tabs and the one in front, when they changed.
    pub(crate) fn remember_tabs(&mut self) {
        let Some(root) = self.session.as_ref().map(|s| s.project().root().to_path_buf()) else {
            return;
        };
        let now = OpenTabs {
            tabs: self.tabs.iter().map(|t| t.as_str().to_string()).collect(),
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
        self.tabs = here;
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
        let unsaved = !saved || self.opened.as_ref().is_some_and(|o| o.editing.dirty_since().is_some());
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
