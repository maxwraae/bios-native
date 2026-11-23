//! What a project has already read, kept so that the next read of an unchanged file costs one
//! `stat` instead of a parse.
//!
//! A file is known unchanged while its size, modification time and (on Unix) inode and change
//! time are what they were when it was read. Every write Bios makes replaces the file through
//! `craft_atomic` or appends to it, which changes those, so nothing has to be told to forget; a
//! change made by another program is noticed the same way.

use std::collections::HashMap;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use bios_core::Construct;

use crate::journal::Entry;

/// What a file looked like when it was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    inode: u64,
    changed: i64,
    changed_ns: i64,
}

impl Stamp {
    pub(crate) fn of(meta: &Metadata) -> Stamp {
        #[cfg(unix)]
        let (inode, changed, changed_ns) = {
            use std::os::unix::fs::MetadataExt;
            (meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        #[cfg(not(unix))]
        let (inode, changed, changed_ns) = (0, 0, 0);
        Stamp { len: meta.len(), modified: meta.modified().ok(), inode, changed, changed_ns }
    }
}

/// One sequence file as read: the text its version is the digest of, what it holds, and that
/// digest. A file that cannot be read keeps the reason, so asking again costs nothing either.
pub(crate) type Parsed = Result<Arc<(String, Construct, String)>, String>;

type Journal = Arc<Vec<Entry>>;

#[derive(Default)]
pub(crate) struct Cache {
    files: Mutex<HashMap<PathBuf, (Stamp, Parsed)>>,
    journals: Mutex<HashMap<PathBuf, (Stamp, Journal)>>,
    parses: AtomicU64,
}

/// A poisoned lock only means another thread panicked while holding it; the map is still a valid
/// map, and at worst holds an entry that will fail its stamp check.
fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Cache {
    pub(crate) fn file(&self, rel: &Path, stamp: Stamp, read: impl FnOnce() -> Parsed) -> Parsed {
        if let Some((s, parsed)) = guard(&self.files).get(rel)
            && *s == stamp
        {
            return parsed.clone();
        }
        self.parses.fetch_add(1, Ordering::Relaxed);
        let parsed = read();
        guard(&self.files).insert(rel.to_path_buf(), (stamp, parsed.clone()));
        parsed
    }

    /// Forgets files that are no longer in the folder, so the cache never outgrows it.
    pub(crate) fn keep_files(&self, present: &[PathBuf]) {
        let keep: std::collections::HashSet<&PathBuf> = present.iter().collect();
        guard(&self.files).retain(|k, _| keep.contains(k));
    }

    pub(crate) fn journal(&self, path: &Path, stamp: Stamp, read: impl FnOnce() -> Result<Vec<Entry>, String>) -> Result<Journal, String> {
        if let Some((s, entries)) = guard(&self.journals).get(path)
            && *s == stamp
        {
            return Ok(entries.clone());
        }
        let entries = Arc::new(read()?);
        guard(&self.journals).insert(path.to_path_buf(), (stamp, entries.clone()));
        Ok(entries)
    }

    pub(crate) fn parses(&self) -> u64 {
        self.parses.load(Ordering::Relaxed)
    }
}
