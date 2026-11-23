//! Bios updating itself from its published releases.
//!
//! The commands (`check_for_update`, `update_bios`) live in the registry like every other, so the
//! window, the command line and MCP reach them the same way. Asking the network and swapping the
//! installed app are not the engine's work: the program running the engine hands it an
//! [`Updater`] once at start ([`set_updater`]), and a test hands a session its own
//! ([`crate::Session::with_updater`]).

use std::sync::{Arc, OnceLock};

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    /// The version this copy counts as.
    pub current: String,
    /// The newest published release, when there is one.
    pub latest: Option<String>,
    /// Whether `latest` is newer than `current`.
    pub newer: bool,
    /// The release's notes, as published.
    pub notes: String,
    /// The page of the release, or of every release when there is none.
    pub page: String,
    /// Why this copy cannot install the release itself, when it cannot (a build that is not an
    /// installed app, a release with no file for this machine).
    pub cannot_install: Option<String>,
}

/// What an installation did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Installed {
    pub from: String,
    pub to: String,
    /// Where the new copy is: the app the old one was, in the same place.
    pub location: String,
    /// What happens to the running window, in a sentence.
    pub relaunch: String,
}

/// Asks for and installs Bios's releases.
pub trait Updater: Send + Sync {
    /// The newest release, and whether it is newer than this copy.
    fn check(&self) -> Result<Check, String>;
    /// Downloads the newest release, checks it and puts it in place of this copy.
    fn install(&self) -> Result<Installed, String>;
}

static UPDATER: OnceLock<Arc<dyn Updater>> = OnceLock::new();

/// The updater every session of this process uses unless it was given its own. Set once, at
/// start; a second call changes nothing and returns `false`.
pub fn set_updater(updater: Arc<dyn Updater>) -> bool {
    UPDATER.set(updater).is_ok()
}

pub(crate) fn installed() -> Option<Arc<dyn Updater>> {
    UPDATER.get().cloned()
}
