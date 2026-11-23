//! What Bios remembers between launches about the window: how it was arranged, and what was open
//! in each project. The folders opened lately are not here but in `bios_project::recents`, which
//! the command line and every agent share. Kept in `config.toml` by `craft-settings`, in the folder each system gives
//! an app for its settings.

use std::path::{Path, PathBuf};

use craft_settings::{DocumentMut, MigrateError, Settings};
use serde::{Deserialize, Serialize};

/// How many folders File > Open Recent lists.
pub const RECENT: usize = 8;

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Prefs {
    pub projects: Projects,
    pub layout: Layout,
    /// The shortcuts changed in Settings > Keyboard, by menu command: the chord's text, or empty
    /// for none (the web's keymap overrides, keys/keymap.ts).
    pub keys: std::collections::BTreeMap<String, String>,
    /// The places whose sheet has opened by itself once (intro/seen.ts).
    pub intro_seen: Vec<String>,
}

/// How the window was arranged, as the web keeps it in local storage: the panel's width
/// (`bios.railWidth`, layout/railWidth.ts), whether it is put away (`bios.panelHidden`,
/// layout/sessionsPanel.tsx), and whether the native Changes column is shown.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Layout {
    pub panel_width: f32,
    pub panel_hidden: bool,
    pub changes_shown: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { panel_width: bios_ui::theme::NAV_FULL_W, panel_hidden: false, changes_shown: false }
    }
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Projects {
    /// Project folders, the one opened last first, as Bios kept them before the list moved to
    /// `recents.json`: read once, to start that list.
    pub recent: Vec<PathBuf>,
    /// The folder sections shut in each project's sidebar, by the project's folder. Sections are
    /// open until shut (the web kept the opened folders instead, per project in local storage).
    pub shut_folders: std::collections::BTreeMap<String, Vec<String>>,
    /// The plasmids open as tabs in each project, by the project's folder, and the one in front
    /// (the web's `openTabs` and `activeTab`).
    pub open_tabs: std::collections::BTreeMap<String, OpenTabs>,
}

/// One project's open tabs, by construct id, in the strip's order.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct OpenTabs {
    pub tabs: Vec<String>,
    pub front: Option<String>,
}

impl Settings for Prefs {
    const APP_ID: &'static str = "bios";
    const SCHEMA_VERSION: u32 = 1;

    fn migrate(_from: u32, _doc: &mut DocumentMut) -> Result<(), MigrateError> {
        Ok(())
    }
}

impl Projects {
    /// The sections that were left shut in a project's sidebar.
    pub fn shut_folders_of(&self, root: &Path) -> Vec<String> {
        self.shut_folders.get(&root.display().to_string()).cloned().unwrap_or_default()
    }

    /// Remembers which sections are shut in a project's sidebar.
    pub fn remember_shut_folders(&mut self, root: &Path, shut: Vec<String>) {
        let key = root.display().to_string();
        if shut.is_empty() {
            self.shut_folders.remove(&key);
        } else {
            self.shut_folders.insert(key, shut);
        }
    }
}
