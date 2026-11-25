//! What a project has already read, kept so that the next read of an unchanged file costs one
//! `stat` instead of a parse.
//!
//! A file is known unchanged while its size, modification time and (on Unix) inode and change
//! time are what they were when it was read; where there is no change time, its bytes stand in.
//! Every write Bios makes replaces the file through `craft_atomic` or appends to it, which
//! changes those, so nothing has to be told to forget; a change made by another program is
//! noticed the same way.

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
    content: u64,
}

impl Stamp {
    /// Takes the path as well as the metadata because on Windows the metadata is not enough.
    ///
    /// On Unix the change time settles it: anything that puts new bytes in a file moves it, even
    /// a write that keeps the length and then sets the modification time back. Windows keeps no
    /// such time — a program can rewrite a file to the same length, restore its modification
    /// time, and leave nothing in the metadata that moved. So on Windows the bytes are part of
    /// the stamp. That costs a read of a file Bios would otherwise only `stat`: the price of
    /// noticing a change rather than handing back a stale plasmid, and it still saves the parse,
    /// which is the expensive half. Sequence files are small enough for that to be the right
    /// trade. See `PARITY.md`.
    pub(crate) fn of(path: &Path, meta: &Metadata) -> Stamp {
        #[cfg(unix)]
        let (inode, changed, changed_ns) = {
            use std::os::unix::fs::MetadataExt;
            let _ = path;
            (meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        #[cfg(not(unix))]
        let (inode, changed, changed_ns) = (0, 0, 0);
        Stamp { len: meta.len(), modified: meta.modified().ok(), inode, changed, changed_ns, content: content(path) }
    }
}

/// A number that follows the file's bytes, on the platforms whose metadata cannot be trusted to.
/// Not a digest anyone relies on: it only has to differ when the bytes differ, within one run.
#[cfg(not(unix))]
fn content(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // A file that cannot be read hashes as itself being unreadable. Two reads that both fail
    // then agree, which is right: the cached answer for such a file is the failure.
    match std::fs::read(path) {
        Ok(bytes) => bytes.hash(&mut hasher),
        Err(e) => (e.kind(), path).hash(&mut hasher),
    }
    hasher.finish()
}

#[cfg(unix)]
fn content(_path: &Path) -> u64 {
    0
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
