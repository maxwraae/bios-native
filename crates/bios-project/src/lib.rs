//! A Bios project: one home for a set of constructs, with their history and lineage.
//!
//! A project in a folder looks like this:
//!
//! ```text
//! My Project/
//!   bios.json                      the project's id and name
//!   <folders>/<name>.gb            plain GenBank, readable by any other tool
//!   .bios/
//!     journal/<construct-id>.jsonl one line per write, only ever added to
//!     objects/<sha256>.gb          the text of every version, so any of them can be restored
//!     lock                         held while a write is in progress
//! ```
//!
//! The rules this crate keeps:
//!
//! - **One copy.** A construct has one id, carried inside its file, and lives at one path.
//! - **Files that are not yours are not written.** A file from outside is imported as a copy.
//! - **A save is checked.** The caller says which version it edited; if the file has moved on, the
//!   save is refused and nothing is merged. The file is read back before the save is reported.
//! - **History is only added to.** Going back restores an earlier version as a new entry.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod cache;
mod folder_bytes;
pub use folder_bytes::write_outside_bytes;
mod folders;
pub mod home;
mod journal;
mod project;
pub mod recents;
pub mod window;

pub use folders::{FolderMeta, FolderPick, folder_candidates, folder_name_for, folder_stamp, item_file_name, pick_folder};
pub use journal::{Actor, Entry};
pub use project::{Change, ConstructId, Deleted, Error, Imported, Listed, Loaded, Project, Who, write_outside};
