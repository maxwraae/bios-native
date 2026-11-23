//! A project in a folder.

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use bios_core::{Construct, genbank, genbank_write};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::cache::{Cache, Parsed, Stamp};
use crate::journal::{self, Actor, Entry};

const META: &str = "bios.json";
pub(crate) const DOT: &str = ".bios";
const FORMAT: u32 = 1;
/// The comment line that carries a construct's id inside its own file. The Bios web app writes
/// the same line, so a construct keeps its identity when it moves between the two.
const ID_KEY: &str = "bios_construct_id:";
/// How deep folders are followed. A project is a few folders of plasmids, not a file system.
const MAX_DEPTH: usize = 16;
/// Files Bios writes in place: GenBank, with the construct's id inside.
const NATIVE: [&str; 2] = ["gb", "gbk"];
/// Every file a folder is read from. A file in one of the other formats is converted to GenBank
/// the first time it is changed, and the original is kept under `.bios/originals/`.
const READABLE: [&str; 12] = ["gb", "gbk", "gbf", "gbff", "genbank", "ape", "dna", "fa", "fasta", "fna", "fas", "embl"];

fn has_extension(path: &Path, list: &[&str]) -> bool {
    path.extension().is_some_and(|e| list.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A construct's identity: the same through every edit, rename and move.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConstructId(String);

impl ConstructId {
    fn fresh() -> Self {
        ConstructId(uuid::Uuid::new_v4().to_string())
    }

    /// Accepts only what is safe to use as a file name, because the journal is named after it.
    pub fn parse(text: &str) -> Option<Self> {
        let ok = !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        ok.then(|| ConstructId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConstructId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Who is making a change, and from where.
#[derive(Clone, Debug)]
pub struct Who {
    pub actor: Actor,
    /// Such as `window`, `cli` or `mcp`.
    pub source: String,
    pub session: Option<String>,
}

/// What a change is, for the journal.
#[derive(Clone, Debug)]
pub struct Change {
    pub command: String,
    pub params: Value,
    /// One sentence a person can read.
    pub summary: String,
}

/// One construct as a listing shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: ConstructId,
    /// Relative to the project folder.
    pub path: PathBuf,
    pub name: String,
    pub length: usize,
    pub circular: bool,
    pub features: usize,
    pub version: u64,
    pub sha: String,
    /// The construct this one was made from.
    pub derived_from: Option<ConstructId>,
}

/// What an import did.
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub construct: Loaded,
    /// The same molecule was already in the project, so nothing was added.
    pub already_here: bool,
    /// Things the person should know about the file, such as that it held more than one sequence.
    pub notices: Vec<String>,
}

/// A construct that was deleted and can still be brought back.
#[derive(Clone, Debug, PartialEq)]
pub struct Deleted {
    pub id: ConstructId,
    pub name: String,
    /// Where it was, relative to the project folder, and where it goes back to.
    pub path: PathBuf,
    /// When it was deleted, in UTC, as RFC 3339.
    pub at: String,
}

/// One construct, read. `sha` is what a save must present to show it edited this version.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub id: ConstructId,
    pub construct: Construct,
    /// Relative to the project folder.
    pub path: PathBuf,
    pub version: u64,
    pub sha: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Io(String),
    NotAProject(PathBuf),
    AlreadyAProject(PathBuf),
    /// The project was written by a newer Bios.
    NewerFormat(u32),
    UnknownConstruct(String),
    /// The file is not the version the caller edited. Nothing was saved and nothing is merged.
    Conflict {
        expected: String,
        found: String,
    },
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    Unwritable(String),
    UnknownVersion(u64),
    /// Another construct already has this name in this folder.
    NameTaken(PathBuf),
    BadFolder(String),
    /// The file did not read back as written, so the save is not reported as done.
    NotSaved(PathBuf),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(m) => f.write_str(m),
            Error::NotAProject(p) => write!(f, "{} is not a Bios project: it has no {META}", p.display()),
            Error::AlreadyAProject(p) => write!(f, "{} is already a Bios project", p.display()),
            Error::NewerFormat(v) => write!(f, "this project was written by a newer Bios (format {v}); update Bios to open it"),
            Error::UnknownConstruct(id) => write!(f, "no construct with id {id} in this project"),
            Error::Conflict { .. } => write!(f, "this construct changed since it was read, so nothing was saved; read it again and redo the change"),
            // A reason that already names the file stands on its own.
            Error::Unreadable { path, reason } if path.as_os_str().is_empty() => f.write_str(reason),
            Error::Unreadable { path, reason } => write!(f, "{}: {reason}", path.display()),
            Error::Unwritable(m) => f.write_str(m),
            Error::UnknownVersion(v) => write!(f, "this construct has no version {v}"),
            Error::NameTaken(p) => write!(f, "{} already exists; choose another name", p.display()),
            Error::BadFolder(m) => write!(f, "{m} is not a folder inside the project"),
            Error::NotSaved(p) => write!(f, "{} did not read back as written; the save is not confirmed", p.display()),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Io(format!("{}: {e}", path.display()))
}

#[derive(Serialize, Deserialize)]
struct Meta {
    format: u32,
    id: String,
    name: String,
}

pub struct Project {
    root: PathBuf,
    meta: Meta,
    /// What has been read already, so an unchanged file is not parsed twice.
    cache: Cache,
}

/// A construct as found on disk.
struct Found {
    id: ConstructId,
    rel: PathBuf,
    /// The file's text, or for a file that is not GenBank, the GenBank it reads as: what its
    /// version is the digest of.
    text: String,
    /// The sha256 of `text`.
    sha: String,
    construct: Construct,
    /// The id is written inside the file. A file that does not carry one yet goes by an id made
    /// from its path, until a change writes that id into it.
    carried: bool,
    /// The file is GenBank and can be written in place.
    native: bool,
}

impl Project {
    /// Makes `root` a project. The folder is created if it is missing; files already in it are
    /// left alone.
    pub fn create(root: &Path, name: &str) -> Result<Project, Error> {
        fs::create_dir_all(root).map_err(|e| io(root, e))?;
        if root.join(META).exists() {
            return Err(Error::AlreadyAProject(root.to_path_buf()));
        }
        let meta = Meta { format: FORMAT, id: uuid::Uuid::new_v4().to_string(), name: name.to_string() };
        let project = Project { root: root.to_path_buf(), meta, cache: Cache::default() };
        project.make_saved()?;
        Ok(project)
    }

    /// Whether `root` already is a project: something was written into it by Bios, so it holds a
    /// `bios.json`.
    pub fn is_project(root: &Path) -> bool {
        root.join(META).is_file()
    }

    /// Opens any folder as a project. A folder Bios has never written to is read from its files
    /// and named after itself; nothing is written into it until something is changed, when its
    /// `bios.json` and `.bios/` are made.
    pub fn open(root: &Path) -> Result<Project, Error> {
        if !root.join(META).exists() && root.is_dir() {
            let name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
            let meta = Meta { format: FORMAT, id: uuid::Uuid::new_v4().to_string(), name };
            return Ok(Project { root: root.to_path_buf(), meta, cache: Cache::default() });
        }
        Project::open_saved(root)
    }

    /// Opens a folder only if it already is a project, with a `bios.json`.
    pub fn open_saved(root: &Path) -> Result<Project, Error> {
        let meta_path = root.join(META);
        let text = match fs::read_to_string(&meta_path) {
            Ok(t) => t,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(Error::NotAProject(root.to_path_buf())),
            Err(e) => return Err(io(&meta_path, e)),
        };
        let meta: Meta = serde_json::from_str(&text).map_err(|e| Error::Unreadable { path: meta_path, reason: e.to_string() })?;
        if meta.format > FORMAT {
            return Err(Error::NewerFormat(meta.format));
        }
        Ok(Project { root: root.to_path_buf(), meta, cache: Cache::default() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn id(&self) -> &str {
        &self.meta.id
    }

    /// Whether anything has been written into the folder yet. Until then it is only read.
    pub fn is_saved(&self) -> bool {
        self.root.join(META).is_file()
    }

    /// Makes the folder a project, on its first write: its `bios.json` and `.bios/`. Files
    /// already in it are left alone.
    fn make_saved(&self) -> Result<(), Error> {
        let meta_path = self.root.join(META);
        for dir in ["journal", "objects"] {
            let dir = self.root.join(DOT).join(dir);
            fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        }
        if meta_path.exists() {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&self.meta).map_err(|e| Error::Io(e.to_string()))?;
        match craft_atomic::write_new(&meta_path, text.as_bytes()) {
            // Another Bios made it a project a moment ago: that one stands.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
            written => written.map_err(|e| io(&meta_path, e)),
        }
    }

    /// Gives the project a new name in its `bios.json`. The folder keeps its name; a folder of
    /// projects renames the folder itself (`home::rename`).
    pub fn rename(&mut self, name: &str) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Unwritable("a project needs a name".into()));
        }
        let _lock = self.lock()?;
        let meta = Meta { format: self.meta.format, id: self.meta.id.clone(), name: name.to_string() };
        let text = serde_json::to_string_pretty(&meta).map_err(|e| Error::Io(e.to_string()))?;
        let path = self.root.join(META);
        craft_atomic::atomic_write(&path, text.as_bytes()).map_err(|e| io(&path, e))?;
        self.meta = meta;
        Ok(())
    }

    /// When anything in the project was last written: the newest journal or sequence file, or
    /// the project's own file.
    pub fn last_changed(&self) -> Option<std::time::SystemTime> {
        let journals = fs::read_dir(self.root.join(DOT).join("journal")).into_iter().flatten().flatten();
        let times = journals.filter_map(|entry| entry.metadata().ok()?.modified().ok());
        let own = fs::metadata(self.root.join(META)).ok().and_then(|m| m.modified().ok());
        let files = self.files().unwrap_or_default().into_iter().filter_map(|rel| fs::metadata(self.root.join(rel)).ok()?.modified().ok());
        times.chain(own).chain(files).max()
    }

    /// Every construct in the project, by path.
    pub fn list(&self) -> Result<Vec<Listed>, Error> {
        Ok(self.list_with_constructs()?.into_iter().map(|(listed, _)| listed).collect())
    }

    /// Every construct in the project with what it holds, in one pass over the folder: for code
    /// that would otherwise `read` each listed construct in turn.
    pub fn list_with_constructs(&self) -> Result<Vec<(Listed, Construct)>, Error> {
        let _lock = self.read_lock()?;
        let mut out = Vec::new();
        for found in self.tracked()? {
            let (version, sha) = self.reconcile(&found)?;
            let derived_from = self.entries(&found.id)?.first().and_then(|e| e.derived_from.as_deref()).and_then(ConstructId::parse);
            let listed = Listed {
                id: found.id,
                path: found.rel,
                name: found.construct.name.clone(),
                length: found.construct.len(),
                circular: found.construct.circular,
                features: found.construct.features.len(),
                version,
                sha,
                derived_from,
            };
            out.push((listed, found.construct));
        }
        Ok(out)
    }

    /// Sequence files in the project folder that Bios cannot read, each with the reason.
    pub fn untracked(&self) -> Result<Vec<(PathBuf, String)>, Error> {
        let mut out = Vec::new();
        for rel in self.files()? {
            if let Err(reason) = self.cached_file(&rel) {
                out.push((rel, reason));
            }
        }
        Ok(out)
    }

    pub fn read(&self, id: &ConstructId) -> Result<Loaded, Error> {
        let _lock = self.read_lock()?;
        let found = self.locate(id)?;
        let (version, sha) = self.reconcile(&found)?;
        Ok(Loaded { id: found.id, construct: found.construct, path: found.rel, version, sha })
    }

    /// Saves `construct` as the next version of `id`, if `base_sha` is still the file's version.
    ///
    /// Saving what is already there is not a change: no version is added. A changed name moves
    /// the file with it.
    pub fn save(&self, id: &ConstructId, base_sha: &str, construct: &Construct, who: &Who, change: Change) -> Result<Loaded, Error> {
        let _lock = self.lock()?;
        let found = self.locate(id)?;
        let (version, sha) = self.reconcile(&found)?;
        if sha != base_sha {
            return Err(Error::Conflict { expected: base_sha.to_string(), found: sha });
        }
        let (found, version, sha) = if found.native { (found, version, sha) } else { self.adopt(found, version, &sha, who)? };
        self.write_version(&found, version, &sha, construct, who, change)
    }

    /// Brings a GenBank file from outside into the project as a copy. The file itself is only read.
    ///
    /// Importing a file that is already here, unchanged, gives back the construct that is here
    /// and adds nothing. A changed copy of a construct that is here comes in as a new construct
    /// that remembers the one it came from.
    pub fn import(&self, source: &Path, folder: &str, name: Option<&str>, who: &Who) -> Result<Imported, Error> {
        let unreadable = |reason: String| Error::Unreadable { path: source.to_path_buf(), reason };
        let bytes = fs::read(source).map_err(|e| unreadable(e.to_string()))?;
        let opened =
            bios_core::formats::read(&bytes, &source.to_string_lossy()).map_err(|reason| Error::Unreadable { path: PathBuf::new(), reason })?;
        let (mut construct, notices) = (opened.construct, opened.notices);
        if let Some(name) = name {
            construct.name = name.to_string();
        }

        let _lock = self.lock()?;
        let tracked = self.tracked()?;
        // One copy: the same molecule with the same annotation is already here, whatever the
        // incoming file is called.
        if let Some(i) = tracked.iter().position(|f| same_molecule(&f.construct, &construct)) {
            let here = tracked.into_iter().nth(i).ok_or_else(|| Error::Io("the project changed while it was being read".into()))?;
            let (version, sha) = self.reconcile(&here)?;
            return Ok(Imported {
                construct: Loaded { id: here.id, construct: here.construct, path: here.rel, version, sha },
                already_here: true,
                notices,
            });
        }
        let mut derived_from = None;
        let mut id = id_of(&construct);
        if id.as_ref().is_some_and(|carried| tracked.iter().any(|f| &f.id == carried)) {
            derived_from = id.take();
        }
        let file = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let change = Change { command: "import_plasmid".into(), params: json!({ "folder": folder }), summary: format!("Imported from {file}") };
        let construct = self.add(construct, id, folder, derived_from, Some(source.display().to_string()), who, change)?;
        Ok(Imported { construct, already_here: false, notices })
    }

    /// Adds a construct that did not exist before.
    /// `derived_from` is the construct it was made from, when it was made from one.
    pub fn create_construct(
        &self,
        construct: Construct,
        folder: &str,
        derived_from: Option<&ConstructId>,
        who: &Who,
        change: Change,
    ) -> Result<Loaded, Error> {
        let _lock = self.lock()?;
        self.add(construct, None, folder, derived_from.cloned(), None, who, change)
    }

    /// Makes a new construct from `id` under `name`, in the same folder, and records where it came from.
    pub fn duplicate(&self, id: &ConstructId, name: &str, who: &Who) -> Result<Loaded, Error> {
        let _lock = self.lock()?;
        let found = self.locate(id)?;
        self.reconcile(&found)?;
        let folder = found.rel.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let change = Change {
            command: "duplicate_plasmid".into(),
            params: json!({ "from": id.as_str(), "name": name }),
            summary: format!("Made from {}", found.construct.name),
        };
        let mut construct = found.construct;
        construct.name = name.to_string();
        self.add(construct, None, &folder, Some(found.id), None, who, change)
    }

    /// When `id` was first written down, from its journal alone. `None` for a construct in a
    /// folder Bios has not written to yet. Cheap: it reads no sequence file.
    pub fn first_written(&self, id: &ConstructId) -> Result<Option<String>, Error> {
        Ok(self.entries(id)?.into_iter().next().map(|e| e.at))
    }

    /// Every write to `id`, oldest first.
    pub fn history(&self, id: &ConstructId) -> Result<Vec<Entry>, Error> {
        let _lock = self.read_lock()?;
        let found = self.locate(id)?;
        let (_, sha) = self.reconcile(&found)?;
        let entries = self.entries(id)?;
        if entries.is_empty() {
            // A folder not written to yet: the file as it was found is its first version.
            return Ok(vec![found_entry(1, sha, None, "construct.adopt", "Found in the folder")]);
        }
        Ok(entries)
    }

    /// The construct as it was at `version`, without changing anything.
    pub fn at_version(&self, id: &ConstructId, version: u64) -> Result<Construct, Error> {
        let _lock = self.read_lock()?;
        let found = self.locate(id)?;
        self.version_text(&found, version)
    }

    /// Goes back to `version` by saving it again as the newest version. Nothing in between is lost.
    pub fn restore(&self, id: &ConstructId, version: u64, who: &Who) -> Result<Loaded, Error> {
        let _lock = self.lock()?;
        let found = self.locate(id)?;
        let (current, sha) = self.reconcile(&found)?;
        let construct = self.version_text(&found, version)?;
        let (found, current, sha) = if found.native { (found, current, sha) } else { self.adopt(found, current, &sha, who)? };
        let change =
            Change { command: "restore_version".into(), params: json!({ "version": version }), summary: format!("Went back to version {version}") };
        self.write_version(&found, current, &sha, &construct, who, change)
    }

    /// Moves a construct to another folder of the project. Its history goes with it.
    pub fn move_to(&self, id: &ConstructId, folder: &str, who: &Who) -> Result<Loaded, Error> {
        let _lock = self.lock()?;
        let found = self.locate(id)?;
        let (version, sha) = self.reconcile(&found)?;
        let (found, version, sha) = self.adopt(found, version, &sha, who)?;
        let folder = clean_folder(folder)?;
        let target = folder.join(file_name_for(&found.construct.name));
        if target == found.rel {
            return Ok(Loaded { id: found.id, construct: found.construct, path: found.rel, version, sha });
        }
        self.rename_file(&found.rel, &target)?;
        let shown = if folder.as_os_str().is_empty() { "the top of the project".to_string() } else { folder.display().to_string() };
        let change = Change {
            command: "move_plasmid".into(),
            params: json!({ "folder": folder.display().to_string() }),
            summary: format!("Moved to {shown}"),
        };
        let version = self.remark(&found.id, version, &sha, who, change)?;
        Ok(Loaded { id: found.id, construct: found.construct, path: target, version, sha })
    }

    /// Takes a construct out of the project. Its file is kept under `.bios/trash/` with its
    /// history, so [`Project::undelete`] can bring it back.
    pub fn delete(&self, id: &ConstructId, who: &Who) -> Result<(), Error> {
        let _lock = self.lock()?;
        let found = self.locate(id)?;
        let (version, sha) = self.reconcile(&found)?;
        let (found, version, sha) = self.adopt(found, version, &sha, who)?;
        let trash = Path::new(DOT).join("trash").join(format!("{id}.gb"));
        self.rename_file(&found.rel, &trash)?;
        let change = Change {
            command: "delete_plasmid".into(),
            params: json!({ "path": slashed(&found.rel) }),
            summary: format!("Deleted {}", found.construct.name),
        };
        self.remark(&found.id, version, &sha, who, change)?;
        Ok(())
    }

    /// The constructs that were deleted and can be brought back, newest first.
    pub fn deleted(&self) -> Result<Vec<Deleted>, Error> {
        let _lock = self.read_lock()?;
        let dir = self.root.join(DOT).join("trash");
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&dir, e)),
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| io(&dir, e))?;
            let Some(id) = entry.path().file_stem().and_then(|s| ConstructId::parse(&s.to_string_lossy())) else {
                continue;
            };
            let Some(last) = self.entries(&id)?.into_iter().rev().find(|e| e.command == "delete_plasmid") else {
                continue;
            };
            let Some(path) = last.params.get("path").and_then(Value::as_str).map(PathBuf::from) else {
                continue;
            };
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(Deleted { id, name, path, at: last.at });
        }
        out.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(out)
    }

    /// Brings a deleted construct back to where it was.
    pub fn undelete(&self, id: &ConstructId, who: &Who) -> Result<Loaded, Error> {
        let gone = self.deleted()?.into_iter().find(|d| &d.id == id).ok_or_else(|| Error::UnknownConstruct(id.to_string()))?;
        let _lock = self.lock()?;
        let trash = Path::new(DOT).join("trash").join(format!("{id}.gb"));
        let target = clean_folder(&gone.path.parent().map(slashed).unwrap_or_default())?.join(file_name_for(&gone.name));
        self.rename_file(&trash, &target)?;
        let found = self.locate(id)?;
        let (version, sha) = self.reconcile(&found)?;
        let change = Change { command: "restore_plasmid".into(), params: Value::Null, summary: format!("Brought back {}", gone.name) };
        let version = self.remark(&found.id, version, &sha, who, change)?;
        Ok(Loaded { id: found.id, construct: found.construct, path: found.rel, version, sha })
    }

    /// A number that changes whenever anything in the project is written, moved, added or
    /// deleted, by this process or another. Cheap enough to ask twice a second: it reads no
    /// construct, only the names, sizes and times of the journals, which every change touches.
    pub fn stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let dir = self.root.join(DOT).join("journal");
        let mut seen: Vec<(std::ffi::OsString, u64, Option<std::time::SystemTime>)> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| entry.metadata().ok().map(|m| (entry.file_name(), m.len(), m.modified().ok())))
            .collect();
        seen.sort();
        seen.hash(&mut hasher);
        // A file changed by another program in a folder not written to yet touches no journal.
        for rel in self.files().unwrap_or_default() {
            let meta = fs::metadata(self.root.join(&rel)).ok();
            (rel, meta.map(|m| (m.len(), m.modified().ok()))).hash(&mut hasher);
        }
        // A folder made, renamed or removed touches no journal; the folders themselves say so,
        // and so does what each one remembers about itself — a design filed or dropped changes a
        // folder's record and nothing else.
        for folder in self.folders().unwrap_or_default() {
            folder.hash(&mut hasher);
            let record = self.root.join(&folder).join(DOT).join("folder.json");
            fs::metadata(&record).ok().map(|m| (m.len(), m.modified().ok())).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Every folder in the project, relative, sorted. Empty folders are included.
    pub fn folders(&self) -> Result<Vec<PathBuf>, Error> {
        let mut out = Vec::new();
        self.walk_folders(Path::new(""), 0, &mut out)?;
        out.sort();
        Ok(out)
    }

    pub fn create_folder(&self, folder: &str) -> Result<PathBuf, Error> {
        let _lock = self.read_lock()?;
        let rel = clean_folder(folder)?;
        if rel.as_os_str().is_empty() {
            return Err(Error::BadFolder(folder.to_string()));
        }
        let path = self.root.join(&rel);
        fs::create_dir_all(&path).map_err(|e| io(&path, e))?;
        Ok(rel)
    }

    /// Renames or moves a folder; the constructs in it go with it.
    pub fn rename_folder(&self, from: &str, to: &str) -> Result<PathBuf, Error> {
        let _lock = self.read_lock()?;
        let (from_rel, to_rel) = (clean_folder(from)?, clean_folder(to)?);
        if from_rel.as_os_str().is_empty() || to_rel.as_os_str().is_empty() || !self.root.join(&from_rel).is_dir() {
            return Err(Error::BadFolder(from.to_string()));
        }
        self.rename_file(&from_rel, &to_rel)?;
        Ok(to_rel)
    }

    /// A small JSON file kept beside the project under `.bios/`, by name: what another layer has to
    /// remember about this project, such as which server it is shared on. `None` when it was
    /// never written.
    pub fn read_sidecar(&self, name: &str) -> Result<Option<String>, Error> {
        let path = self.sidecar_path(name)?;
        let _lock = self.read_lock()?;
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&path, e)),
        }
    }

    /// Replaces the sidecar `name` with `json`, whole or not at all. Text that is not JSON is
    /// refused, so a reader never finds half a file or something it cannot parse.
    pub fn write_sidecar(&self, name: &str, json: &str) -> Result<(), Error> {
        let path = self.sidecar_path(name)?;
        if serde_json::from_str::<Value>(json).is_err() {
            return Err(Error::Unwritable(format!("{name} was not written: it is not JSON")));
        }
        let _lock = self.lock()?;
        craft_atomic::atomic_write(&path, json.as_bytes()).map_err(|e| io(&path, e))
    }

    // ── below here the lock is already held ────────────────────────────────────────────────

    /// Where the sidecar `name` lives. The name becomes a file name, so it may hold nothing a
    /// path could be built from.
    fn sidecar_path(&self, name: &str) -> Result<PathBuf, Error> {
        let ok = !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            Ok(self.root.join(DOT).join(format!("{name}.json")))
        } else {
            Err(Error::Unwritable(format!("{name:?} is not a name a project can keep a file under")))
        }
    }

    /// One writer at a time, across every Bios process that has this project open. Taking it
    /// twice in one call would wait forever, so only the public methods take it. This is the
    /// lock a write takes: the folder becomes a project first, if it is not one yet.
    pub(crate) fn lock(&self) -> Result<File, Error> {
        self.make_saved()?;
        self.lock_file()
    }

    /// The lock a call that only reads takes; it may still record what another program changed.
    /// A folder never written to has nothing to record, and a read does not write into it.
    pub(crate) fn read_lock(&self) -> Result<Option<File>, Error> {
        if self.is_saved() { self.lock_file().map(Some) } else { Ok(None) }
    }

    fn lock_file(&self) -> Result<File, Error> {
        let dir = self.root.join(DOT);
        fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let path = dir.join("lock");
        let file = File::options().create(true).truncate(false).write(true).open(&path).map_err(|e| io(&path, e))?;
        file.lock().map_err(|e| io(&path, e))?;
        Ok(file)
    }

    fn journal_path(&self, id: &ConstructId) -> PathBuf {
        self.root.join(DOT).join("journal").join(format!("{id}.jsonl"))
    }

    fn object_path(&self, sha: &str) -> PathBuf {
        self.root.join(DOT).join("objects").join(format!("{sha}.gb"))
    }

    fn entries(&self, id: &ConstructId) -> Result<Vec<Entry>, Error> {
        let path = self.journal_path(id);
        let stamp = match fs::metadata(&path) {
            Ok(meta) => Stamp::of(&meta),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&path, e)),
        };
        let entries = self.cache.journal(&path, stamp, || journal::read(&path)).map_err(Error::Io)?;
        Ok(entries.as_ref().clone())
    }

    /// How many sequence files this project has parsed since it was opened. A file read again
    /// unchanged is not parsed again; tests use this to show it.
    #[doc(hidden)]
    pub fn parse_count(&self) -> u64 {
        self.cache.parses()
    }

    /// Sequence files under the project folder, relative, sorted. Hidden folders are not entered
    /// and links are not followed.
    fn files(&self) -> Result<Vec<PathBuf>, Error> {
        let mut out = Vec::new();
        self.walk(Path::new(""), 0, &mut out)?;
        out.sort();
        Ok(out)
    }

    fn walk(&self, rel: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), Error> {
        let dir = self.root.join(rel);
        for entry in fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
            let entry = entry.map_err(|e| io(&dir, e))?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type().map_err(|e| io(&entry.path(), e))?;
            let child = rel.join(&name);
            if kind.is_dir() && depth < MAX_DEPTH {
                self.walk(&child, depth + 1, out)?;
            } else if kind.is_file() && has_extension(&child, &READABLE) {
                out.push(child);
            }
        }
        Ok(())
    }

    fn walk_folders(&self, rel: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), Error> {
        let dir = self.root.join(rel);
        for entry in fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
            let entry = entry.map_err(|e| io(&dir, e))?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || !entry.file_type().map_err(|e| io(&entry.path(), e))?.is_dir() {
                continue;
            }
            let child = rel.join(&name);
            out.push(child.clone());
            if depth < MAX_DEPTH {
                self.walk_folders(&child, depth + 1, out)?;
            }
        }
        Ok(())
    }

    /// Moves a file or folder inside the project, never onto something that is already there.
    fn rename_file(&self, from: &Path, to: &Path) -> Result<(), Error> {
        let (from_abs, to_abs) = (self.root.join(from), self.root.join(to));
        if to_abs.exists() {
            return Err(Error::NameTaken(to.to_path_buf()));
        }
        if let Some(dir) = to_abs.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        fs::rename(&from_abs, &to_abs).map_err(|e| io(&to_abs, e))
    }

    /// Records something that happened to a construct without changing its text: a move, a
    /// deletion. It is a version of its own so that the history has no gaps, with the same text
    /// before and after.
    fn remark(&self, id: &ConstructId, version: u64, sha: &str, who: &Who, change: Change) -> Result<u64, Error> {
        let entry = Entry {
            version: version + 1,
            at: now(),
            actor: who.actor,
            source: who.source.clone(),
            session: who.session.clone(),
            command: change.command,
            params: change.params,
            summary: change.summary,
            base_sha: Some(sha.to_string()),
            result_sha: sha.to_string(),
            derived_from: None,
            origin: None,
        };
        journal::append(&self.journal_path(id), &entry).map_err(Error::Io)?;
        Ok(version + 1)
    }

    fn tracked(&self) -> Result<Vec<Found>, Error> {
        let files = self.files()?;
        self.cache.keep_files(&files);
        let mut out = Vec::new();
        for rel in files {
            let Ok(parsed) = self.cached_file(&rel) else {
                continue;
            };
            let (text, mut construct, sha) = parsed.as_ref().clone();
            // Inside a project a construct is called what its file is called. A LOCUS line cannot
            // hold a name with spaces; a file name can.
            if let Some(stem) = rel.file_stem() {
                construct.name = stem.to_string_lossy().into_owned();
            }
            let carried = id_of(&construct);
            let native = has_extension(&rel, &NATIVE);
            let id = carried.clone().unwrap_or_else(|| path_id(&rel));
            out.push(Found { id, rel, text, sha, construct, carried: carried.is_some(), native });
        }
        Ok(out)
    }

    /// One sequence file through the cache: parsed only if it changed since it was last read.
    fn cached_file(&self, rel: &Path) -> Parsed {
        let meta = fs::metadata(self.root.join(rel)).map_err(|e| e.to_string())?;
        self.cache.file(rel, Stamp::of(&meta), || {
            let (text, construct) = self.read_file(rel)?;
            let sha = sha_of(&text);
            Ok(std::sync::Arc::new((text, construct, sha)))
        })
    }

    /// One sequence file: its text (for a file that is not GenBank, the GenBank it reads as) and
    /// what it holds.
    fn read_file(&self, rel: &Path) -> Result<(String, Construct), String> {
        let path = self.root.join(rel);
        if has_extension(rel, &NATIVE) {
            let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let construct = genbank::read(&text).map_err(|e| e.to_string())?;
            return Ok((text, construct));
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let construct = bios_core::formats::read(&bytes, &rel.to_string_lossy())?.construct;
        let text = genbank_write::write(&construct).map_err(|e| e.to_string())?;
        Ok((text, construct))
    }

    /// Writes the construct's id into its own file, so that it keeps its identity and history
    /// when it is moved or renamed. A file that is not GenBank becomes `<name>.gb` beside it and
    /// the original is kept under `.bios/originals/`. Recorded as a version of its own.
    fn adopt(&self, found: Found, version: u64, sha: &str, who: &Who) -> Result<(Found, u64, String), Error> {
        if found.carried && found.native {
            return Ok((found, version, sha.to_string()));
        }
        let mut construct = found.construct.clone();
        set_id(&mut construct, &found.id);
        let text = genbank_write::write(&construct).map_err(|e| Error::Unwritable(e.to_string()))?;
        let new_sha = sha_of(&text);
        let target = if found.native { found.rel.clone() } else { found.rel.with_extension("gb") };
        let path = self.root.join(&target);
        self.store_object(&new_sha, &text)?;
        let summary = if found.native {
            craft_atomic::atomic_write(&path, text.as_bytes()).map_err(|e| io(&path, e))?;
            "Became part of the project".to_string()
        } else {
            craft_atomic::write_new(&path, text.as_bytes())
                .map_err(|e| if e.kind() == ErrorKind::AlreadyExists { Error::NameTaken(target.clone()) } else { io(&path, e) })?;
            let originals = Path::new(DOT).join("originals");
            let mut kept = originals.join(&found.rel);
            let mut n = 2;
            while self.root.join(&kept).exists() && n < 10_000 {
                kept = originals.join(format!("{} {n}", slashed(&found.rel)));
                n += 1;
            }
            self.rename_file(&found.rel, &kept)?;
            let file = found.rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            format!("Converted from {file} to GenBank; the original is kept in {}", slashed(&kept))
        };
        read_back(&path, &new_sha)?;
        let entry = Entry { base_sha: Some(sha.to_string()), ..found_entry(version + 1, new_sha.clone(), Some(who), "construct.adopt", &summary) };
        journal::append(&self.journal_path(&found.id), &entry).map_err(Error::Io)?;
        let adopted = Found { id: found.id, rel: target, text, sha: new_sha.clone(), construct, carried: true, native: true };
        Ok((adopted, version + 1, new_sha))
    }

    fn locate(&self, id: &ConstructId) -> Result<Found, Error> {
        self.tracked()?.into_iter().find(|f| &f.id == id).ok_or_else(|| Error::UnknownConstruct(id.to_string()))
    }

    /// The file's version and sha. If the file is not what the journal last recorded, another
    /// program changed it (or a write was cut short before its journal line); that is recorded as
    /// a version of its own, so history never has a gap.
    fn reconcile(&self, found: &Found) -> Result<(u64, String), Error> {
        let sha = found.sha.clone();
        let entries = self.entries(&found.id)?;
        let last = entries.last();
        if let Some(last) = last
            && last.result_sha == sha
        {
            return Ok((last.version, sha));
        }
        let version = last.map_or(1, |e| e.version + 1);
        if !self.is_saved() {
            // Only read: what was found is written down on the first change.
            return Ok((version, sha));
        }
        let (command, summary) =
            if last.is_some() { ("external.change", "Changed outside Bios") } else { ("construct.adopt", "Found in the project folder") };
        self.store_object(&sha, &found.text)?;
        let entry = Entry {
            version,
            at: now(),
            actor: Actor::Import,
            source: "disk".into(),
            session: None,
            command: command.into(),
            params: Value::Null,
            summary: summary.into(),
            base_sha: last.map(|e| e.result_sha.clone()),
            result_sha: sha.clone(),
            derived_from: None,
            origin: None,
        };
        journal::append(&self.journal_path(&found.id), &entry).map_err(Error::Io)?;
        Ok((version, sha))
    }

    fn store_object(&self, sha: &str, text: &str) -> Result<(), Error> {
        let path = self.object_path(sha);
        if path.exists() {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        craft_atomic::atomic_write(&path, text.as_bytes()).map_err(|e| io(&path, e))
    }

    /// The construct as it was at `version`, under the name it has now. A version is what the
    /// molecule was; what it is called and where it lives belong to the construct, and going
    /// back must not move the file.
    fn version_text(&self, found: &Found, version: u64) -> Result<Construct, Error> {
        let entries = self.entries(&found.id)?;
        if entries.is_empty() && version == 1 {
            return Ok(found.construct.clone());
        }
        let entry = entries.into_iter().find(|e| e.version == version).ok_or(Error::UnknownVersion(version))?;
        let path = self.object_path(&entry.result_sha);
        let text = fs::read_to_string(&path).map_err(|e| io(&path, e))?;
        let mut construct = genbank::read(&text).map_err(|e| Error::Unreadable { path, reason: e.to_string() })?;
        construct.name = found.construct.name.clone();
        Ok(construct)
    }

    /// Writes `construct` over `found` as version `version + 1`.
    fn write_version(&self, found: &Found, version: u64, sha: &str, construct: &Construct, who: &Who, change: Change) -> Result<Loaded, Error> {
        let mut construct = construct.clone();
        construct.name = name_on_disk(&construct.name);
        set_id(&mut construct, &found.id);
        let text = genbank_write::write(&construct).map_err(|e| Error::Unwritable(e.to_string()))?;
        let new_sha = sha_of(&text);
        if new_sha == sha {
            return Ok(Loaded { id: found.id.clone(), construct, path: found.rel.clone(), version, sha: new_sha });
        }

        let folder = found.rel.parent().unwrap_or(Path::new(""));
        let target = folder.join(file_name_for(&construct.name));
        if target != found.rel && self.root.join(&target).exists() {
            return Err(Error::NameTaken(target));
        }

        // The version's text is kept before the file changes, so the journal can never point at
        // text that was not stored.
        self.store_object(&new_sha, &text)?;
        let path = self.root.join(&found.rel);
        craft_atomic::atomic_write(&path, text.as_bytes()).map_err(|e| io(&path, e))?;
        read_back(&path, &new_sha)?;
        if target != found.rel {
            let to = self.root.join(&target);
            fs::rename(&path, &to).map_err(|e| io(&to, e))?;
        }

        let entry = Entry {
            version: version + 1,
            at: now(),
            actor: who.actor,
            source: who.source.clone(),
            session: who.session.clone(),
            command: change.command,
            params: change.params,
            summary: change.summary,
            base_sha: Some(sha.to_string()),
            result_sha: new_sha.clone(),
            derived_from: None,
            origin: None,
        };
        journal::append(&self.journal_path(&found.id), &entry).map_err(Error::Io)?;
        Ok(Loaded { id: found.id.clone(), construct, path: target, version: version + 1, sha: new_sha })
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &self,
        mut construct: Construct,
        id: Option<ConstructId>,
        folder: &str,
        derived_from: Option<ConstructId>,
        origin: Option<String>,
        who: &Who,
        change: Change,
    ) -> Result<Loaded, Error> {
        let folder = clean_folder(folder)?;
        let id = id.unwrap_or_else(ConstructId::fresh);
        construct.name = name_on_disk(&construct.name);
        set_id(&mut construct, &id);
        if construct.locus.molecule.is_empty() {
            construct.locus.molecule = "DNA".into();
        }
        if construct.locus.date.is_empty() {
            construct.locus.date = chrono::Utc::now().format("%d-%b-%Y").to_string().to_uppercase();
        }
        let text = genbank_write::write(&construct).map_err(|e| Error::Unwritable(e.to_string()))?;
        let sha = sha_of(&text);

        let rel = folder.join(file_name_for(&construct.name));
        let path = self.root.join(&rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
        }
        self.store_object(&sha, &text)?;
        // `write_new` refuses to replace a file, so two constructs can never end up at one path.
        craft_atomic::write_new(&path, text.as_bytes())
            .map_err(|e| if e.kind() == ErrorKind::AlreadyExists { Error::NameTaken(rel.clone()) } else { io(&path, e) })?;
        read_back(&path, &sha)?;

        let entry = Entry {
            version: 1,
            at: now(),
            actor: who.actor,
            source: who.source.clone(),
            session: who.session.clone(),
            command: change.command,
            params: change.params,
            summary: change.summary,
            base_sha: None,
            result_sha: sha.clone(),
            derived_from: derived_from.map(|d| d.to_string()),
            origin,
        };
        journal::append(&self.journal_path(&id), &entry).map_err(Error::Io)?;
        Ok(Loaded { id, construct, path: rel, version: 1, sha })
    }
}

/// The file must hold exactly what was written before a save is reported as done.
fn read_back(path: &Path, sha: &str) -> Result<(), Error> {
    let text = fs::read_to_string(path).map_err(|e| io(path, e))?;
    if sha_of(&text) == sha { Ok(()) } else { Err(Error::NotSaved(path.to_path_buf())) }
}

/// The id of a file that does not carry one yet: the same for the same place in the folder, so a
/// plasmid keeps it between reads until a change writes it into the file.
fn path_id(rel: &Path) -> ConstructId {
    let digest = sha_of(&slashed(rel));
    ConstructId(format!("f-{}", digest.get(..32).unwrap_or(&digest)))
}

/// A journal line for something that happened to a file, rather than an edit made in Bios.
fn found_entry(version: u64, sha: String, who: Option<&Who>, command: &str, summary: &str) -> Entry {
    Entry {
        version,
        at: now(),
        actor: who.map_or(Actor::Import, |w| w.actor),
        source: who.map_or_else(|| "disk".into(), |w| w.source.clone()),
        session: who.and_then(|w| w.session.clone()),
        command: command.into(),
        params: Value::Null,
        summary: summary.into(),
        base_sha: None,
        result_sha: sha,
        derived_from: None,
        origin: None,
    }
}

fn sha_of(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn id_of(construct: &Construct) -> Option<ConstructId> {
    construct.comments.iter().find_map(|c| c.trim().strip_prefix(ID_KEY)).and_then(|id| ConstructId::parse(id.trim()))
}

fn set_id(construct: &mut Construct, id: &ConstructId) {
    let line = format!("{ID_KEY} {id}");
    match construct.comments.iter_mut().find(|c| c.trim().starts_with(ID_KEY)) {
        Some(existing) => *existing = line,
        None => construct.comments.push(line),
    }
}

/// The same molecule and annotation, whatever the files' names, dates and ids say.
fn same_molecule(a: &Construct, b: &Construct) -> bool {
    a.circular == b.circular && a.sequence == b.sequence && a.features == b.features
}

/// The name as a file can carry it: no path separators or characters a file system refuses.
fn name_on_disk(name: &str) -> String {
    let safe: String = name.trim().chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let safe = safe.trim_matches('.').trim();
    if safe.is_empty() { "untitled".to_string() } else { safe.to_string() }
}

fn file_name_for(name: &str) -> String {
    format!("{}.gb", name_on_disk(name))
}

/// A folder inside the project: relative, and never climbing out through `..`.
pub(crate) fn clean_folder(folder: &str) -> Result<PathBuf, Error> {
    let path = Path::new(folder.trim());
    let inside = path.components().all(|c| matches!(c, Component::Normal(n) if !n.to_string_lossy().starts_with('.')));
    if inside { Ok(path.to_path_buf()) } else { Err(Error::BadFolder(folder.to_string())) }
}

/// A relative path with `/` between its parts, whatever the platform.
fn slashed(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// Writes `text` to a file outside any project, for an export. An existing file is replaced
/// only when `overwrite` says so.
pub fn write_outside(path: &Path, text: &str, overwrite: bool) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let written = if overwrite { craft_atomic::atomic_write(path, text.as_bytes()) } else { craft_atomic::write_new(path, text.as_bytes()) };
    written.map_err(|e| if e.kind() == ErrorKind::AlreadyExists { Error::NameTaken(path.to_path_buf()) } else { io(path, e) })
}
