//! The folders a person has opened in Bios, the latest first: the list the projects page draws,
//! File > Open Recent names and an agent connected with `bios-cli mcp` works in.
//!
//! There is no folder of projects. A project is any folder, wherever it is, and this list is how
//! Bios finds them again, as an editor remembers the folders it opened. It is one small JSON file
//! in the folder the system gives Bios for its settings, shared by the window, the command line
//! and every agent, and read again before each change so that none of them undoes another's:
//!
//! ```json
//! { "v": 1,
//!   "folders": [ { "path": "/Users/me/Lab/AAV", "opened_at": "2026-11-22T10:00:00Z" } ],
//!   "pins": [ "/Users/me/Lab/AAV" ],
//!   "order": [] }
//! ```
//!
//! `pins` and `order` are the projects page's arrangement: the pinned folders in the order drawn,
//! and the order made by hand (empty is the latest opened first). Both name folders in the list.

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cache::Stamp;
use crate::project::{Error, Project, io, now};

/// The file's name inside Bios's settings folder.
pub const FILE: &str = "recents.json";
/// How many folders are remembered. A pinned folder is never forgotten to make room.
const KEEP: usize = 100;
const VERSION: u32 = 1;

/// One folder that was opened.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: PathBuf,
    /// When it was last opened, in UTC, as RFC 3339.
    #[serde(default)]
    pub opened_at: String,
}

/// What the file holds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct List {
    pub v: u32,
    /// The latest opened first.
    pub folders: Vec<Recent>,
    pub pins: Vec<PathBuf>,
    pub order: Vec<PathBuf>,
}

impl Default for List {
    fn default() -> Self {
        List { v: VERSION, folders: Vec::new(), pins: Vec::new(), order: Vec::new() }
    }
}

impl List {
    /// The folders, the latest opened first.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.folders.iter().map(|r| r.path.clone()).collect()
    }

    fn opened(&mut self, root: PathBuf) {
        self.folders.retain(|r| r.path != root);
        self.folders.insert(0, Recent { path: root, opened_at: now() });
        let pins = self.pins.clone();
        let mut kept = 0;
        self.folders.retain(|r| {
            kept += 1;
            kept <= KEEP || pins.contains(&r.path)
        });
        self.tidy();
    }

    fn forget(&mut self, root: &Path) {
        self.folders.retain(|r| r.path != root);
        self.tidy();
    }

    fn moved(&mut self, from: &Path, to: &Path) {
        let follow = |p: &mut PathBuf| {
            if p == from {
                *p = to.to_path_buf();
            }
        };
        self.folders.iter_mut().for_each(|r| follow(&mut r.path));
        self.pins.iter_mut().for_each(follow);
        self.order.iter_mut().for_each(follow);
    }

    /// Pins and the hand-made order only name folders that are in the list.
    fn tidy(&mut self) {
        let known: Vec<PathBuf> = self.paths();
        self.pins.retain(|p| known.contains(p));
        self.order.retain(|p| known.contains(p));
    }
}

/// The list, kept in a file, or only in memory (a test, or a picture of the window taken without
/// touching what the person's Bios remembers).
pub struct Recents {
    file: Option<PathBuf>,
    memory: List,
    /// The file as last read, so the window, which asks every frame, reads it only when it
    /// changed.
    seen: std::sync::Mutex<Option<Box<(Stamp, List)>>>,
}

/// The folder each system gives an app for its settings, as `craft-settings` finds it, so that
/// the list sits beside the window's `config.toml`. `BIOS_CONFIG_DIR` moves both.
pub fn settings_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("BIOS_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = || std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").filter(|d| !d.is_empty()).map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()).map(PathBuf::from).or_else(|| home().map(|h| h.join(".config")))
    };
    base.map(|b| b.join("bios"))
}

/// Where a new project is made unless the person chooses another place: `Bios projects` in
/// their Documents folder when there is one, else Documents itself, else their home folder.
pub fn default_location() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").filter(|h| !h.is_empty())?);
    let documents = home.join("Documents");
    let preferred = documents.join("Bios projects");
    Some(if preferred.is_dir() {
        preferred
    } else if documents.is_dir() {
        documents
    } else {
        home
    })
}

/// The projects in `~/Bios`, where earlier versions of Bios kept every project, and in the place
/// new projects are made: the list starts from them, so nothing a person made goes missing.
fn earlier_projects() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let places = [home.map(|h| h.join("Bios")), default_location()];
    let mut found: Vec<PathBuf> = places
        .iter()
        .flatten()
        .flat_map(|place| fs::read_dir(place).into_iter().flatten().flatten().map(|e| e.path()))
        .filter(|p| Project::is_project(p))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// A folder as the list remembers it: absolute, with links resolved when it is there.
fn key(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| {
        if root.is_absolute() { root.to_path_buf() } else { std::env::current_dir().map(|d| d.join(root)).unwrap_or_else(|_| root.to_path_buf()) }
    })
}

impl Recents {
    /// The list kept in `file`.
    pub fn at(file: &Path) -> Recents {
        Recents { file: Some(file.to_path_buf()), memory: List::default(), seen: Default::default() }
    }

    /// A list that lives only as long as this value.
    pub fn in_memory() -> Recents {
        Recents { file: None, memory: List::default(), seen: Default::default() }
    }

    /// The person's own list, in Bios's settings folder. The first time there is none, it starts
    /// from `seed` (folders the window remembered) and the projects in `~/Bios`.
    pub fn standard(seed: &[PathBuf]) -> Recents {
        let Some(dir) = settings_dir() else {
            return Recents::in_memory();
        };
        let mut recents = Recents::at(&dir.join(FILE));
        if !dir.join(FILE).exists() {
            let mut start: Vec<PathBuf> = seed.iter().filter(|p| p.is_dir()).cloned().collect();
            start.extend(earlier_projects());
            // A list that cannot be written is still read; the next change reports why.
            let _ = recents.change(|list| {
                for root in start.into_iter().rev() {
                    list.opened(key(&root));
                }
            });
        }
        recents
    }

    /// The person's own list as it is now, kept in memory from here on: what a picture of the
    /// window shows, without changing what Bios remembers.
    pub fn standard_copy() -> Recents {
        let memory = settings_dir().map(|dir| Recents::at(&dir.join(FILE)).list()).unwrap_or_default();
        Recents { file: None, memory, seen: Default::default() }
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The list as it is now. A file that is missing or damaged reads as an empty list.
    pub fn list(&self) -> List {
        let Some(file) = &self.file else {
            return self.memory.clone();
        };
        let Ok(meta) = fs::metadata(file) else {
            return List::default();
        };
        let stamp = Stamp::of(&meta);
        let mut seen = match self.seen.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((s, list)) = seen.as_deref()
            && *s == stamp
        {
            return list.clone();
        }
        let list: List = fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        *seen = Some(Box::new((stamp, list.clone())));
        list
    }

    /// The folders that are still there, the latest opened first.
    pub fn folders(&self) -> Vec<PathBuf> {
        self.list().paths().into_iter().filter(|p| p.is_dir()).collect()
    }

    /// A folder was opened or made: it goes to the front, once.
    pub fn opened(&mut self, root: &Path) -> Result<PathBuf, Error> {
        let root = key(root);
        let kept = root.clone();
        self.change(move |list| list.opened(kept))?;
        Ok(root)
    }

    /// Takes a folder off the list. The folder itself is not touched.
    pub fn forget(&mut self, root: &Path) -> Result<(), Error> {
        let (exact, resolved) = (root.to_path_buf(), key(root));
        self.change(|list| {
            list.forget(&exact);
            list.forget(&resolved);
        })
    }

    /// A folder was renamed or moved: its place in the list, its pin and its order follow it.
    pub fn moved(&mut self, from: &Path, to: &Path) -> Result<(), Error> {
        let to = key(to);
        self.change(|list| list.moved(from, &to))
    }

    pub fn set_pins(&mut self, pins: Vec<PathBuf>) -> Result<(), Error> {
        self.change(|list| {
            list.pins = pins;
            list.tidy();
        })
    }

    pub fn set_order(&mut self, order: Vec<PathBuf>) -> Result<(), Error> {
        self.change(|list| {
            list.order = order;
            list.tidy();
        })
    }

    /// Reads the list, changes it and writes it back, whole, with every other Bios held off
    /// meanwhile, so two programs changing it at once do not lose each other's change.
    fn change(&mut self, edit: impl FnOnce(&mut List)) -> Result<(), Error> {
        let Some(file) = self.file.clone() else {
            edit(&mut self.memory);
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        let lock_path = file.with_extension("lock");
        let lock = File::options().create(true).truncate(false).write(true).open(&lock_path).map_err(|e| io(&lock_path, e))?;
        lock.lock().map_err(|e| io(&lock_path, e))?;
        let mut list = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(e) if e.kind() == ErrorKind::NotFound => List::default(),
            Err(e) => return Err(io(&file, e)),
        };
        edit(&mut list);
        list.v = VERSION;
        let text = serde_json::to_string_pretty(&list).map_err(|e| Error::Io(e.to_string()))?;
        craft_atomic::atomic_write(&file, text.as_bytes()).map_err(|e| io(&file, e))
    }
}
