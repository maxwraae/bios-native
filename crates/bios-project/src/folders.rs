//! What a folder knows about itself, and the files it keeps beside its constructs.
//!
//! A folder in a project was its name and the plasmids in it and nothing else. That holds right up
//! until a folder MEANS something: a build folder is one backbone opened one way, a verification
//! folder is one submission measured against its maps. Those are not organisation, so a folder
//! gets a TYPE and a drawer of its own.
//!
//! ## Where it lives
//!
//! ```text
//! <folder>/.bios/folder.json        the folder's own record: its type, when it was made, and
//!                                   whatever that type needs to remember
//! <folder>/.bios/<dir>/<name>.json  one item of that type, too big to ride on the record
//! ```
//!
//! Inside the folder it describes, never in a table somewhere else, because renaming or moving a
//! folder is one `rename` and anything inside it moves with it for free. A directory cannot get
//! out of step with itself.
//!
//! ## Typed, but not typed here
//!
//! `bios-project` knows that a folder has a kind and some JSON; it does not know what a build or a
//! verification is. The kind is a string and the rest is [`FolderMeta::data`], so a new kind of
//! folder is a new command in `bios-engine` and nothing at all in this crate.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::project::{DOT, Error, Project, clean_folder, io};

/// The file a folder's record lives in, inside the folder's own `.bios/`.
const FOLDER_META: &str = "folder.json";

/// A folder's own record. `kind` is what sort of folder it is (`build`, `verification`); `data`
/// is whatever that sort remembers, written flat into the same file so it can be read with `head`.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderMeta {
    /// The schema's version. 1 from the first write, so the second has something to branch on.
    pub v: u32,
    pub kind: String,
    /// When the folder was given its type, in UTC, as RFC 3339.
    pub created_at: String,
    /// The typed part: the fields this kind of folder keeps.
    pub data: Map<String, Value>,
}

impl FolderMeta {
    /// A record for a folder being typed now.
    pub fn new(kind: &str, data: Map<String, Value>) -> FolderMeta {
        FolderMeta { v: 1, kind: kind.to_string(), created_at: crate::project::now(), data }
    }

    /// One field of the typed part, or `Value::Null`.
    pub fn get(&self, key: &str) -> &Value {
        self.data.get(key).unwrap_or(&Value::Null)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.data.insert(key.to_string(), value);
    }

    fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("v".into(), json!(self.v));
        out.insert("type".into(), json!(self.kind));
        out.insert("createdAt".into(), json!(self.created_at));
        for (key, value) in &self.data {
            out.insert(key.clone(), value.clone());
        }
        Value::Object(out)
    }

    /// A record read back. `None` for anything that is not a typed folder's record, including a
    /// file that will not parse: a folder whose record is broken still holds real constructs and
    /// must draw as an ordinary folder rather than take the project down with it.
    fn from_json(value: &Value) -> Option<FolderMeta> {
        let object = value.as_object()?;
        let kind = object.get("type")?.as_str()?.to_string();
        let mut data = object.clone();
        data.remove("v");
        data.remove("type");
        data.remove("createdAt");
        Some(FolderMeta {
            v: object.get("v").and_then(Value::as_u64).unwrap_or(1) as u32,
            kind,
            created_at: object.get("createdAt").and_then(Value::as_str).unwrap_or_default().to_string(),
            data,
        })
    }
}

/// A name inside a folder's drawer, optionally one directory deep (`designs/egfp-1a2b3c4d.json`).
///
/// Stricter than a folder name on purpose: nobody types these, they are minted by Bios. Two
/// segments is the cap — the drawer is a drawer, not a second file system.
fn check_drawer_name(name: &str) -> Result<(), Error> {
    let bad = |why: &str| Err(Error::Unwritable(format!("{name:?} is not a name a folder can keep a file under: {why}")));
    if name.is_empty() {
        return bad("it is empty");
    }
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() > 2 {
        return bad("it is more than one directory deep");
    }
    for part in parts {
        let ok = !part.is_empty()
            && part.len() <= 128
            && part.starts_with(|c: char| c.is_ascii_alphanumeric())
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
        if !ok {
            return bad("it holds a character a file in the drawer may not");
        }
    }
    Ok(())
}

/// The file name for one item in a folder's drawer: a slug of `name`, kept honest by a digest.
///
/// A design is called whatever the plasmid it makes is called — `pAAV (v2): round 3` is an
/// ordinary name — and none of that is a file name. So it is slugged, and the digest is of the
/// ORIGINAL name: two names that clean up to the same string would otherwise overwrite each other
/// and the second would look like it had been written when what was there was the first.
/// Deterministic, so writing the same item again replaces its own file and nothing else.
pub fn item_file_name(dir: &str, name: &str, fallback: &str) -> String {
    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' }).collect();
    let slug: String = slug.trim_start_matches(|c: char| !c.is_ascii_alphanumeric()).chars().take(80).collect();
    let slug = if slug.is_empty() { fallback.to_string() } else { slug };
    let digest: String = Sha256::digest(name.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{dir}/{slug}-{digest}.json")
}

impl Project {
    /// What the folder `folder` knows about itself, or `None` when it is an ordinary folder.
    pub fn folder_meta(&self, folder: &str) -> Result<Option<FolderMeta>, Error> {
        let path = self.drawer_path(folder, FOLDER_META)?;
        let Some(text) = read_opt(&path)? else {
            return Ok(None);
        };
        Ok(serde_json::from_str::<Value>(&text).ok().as_ref().and_then(FolderMeta::from_json))
    }

    /// Every folder in the project that has a record, with it. Ordinary folders are left out.
    pub fn folder_metas(&self) -> Result<Vec<(PathBuf, FolderMeta)>, Error> {
        let mut out = Vec::new();
        for folder in self.folders()? {
            let name = slashed(&folder);
            if let Some(meta) = self.folder_meta(&name)? {
                out.push((folder, meta));
            }
        }
        Ok(out)
    }

    /// Gives `folder` a record, or replaces the one it has. The folder is made if it is missing.
    pub fn write_folder_meta(&self, folder: &str, meta: &FolderMeta) -> Result<(), Error> {
        let path = self.drawer_path(folder, FOLDER_META)?;
        let text = serde_json::to_string_pretty(&meta.to_json()).map_err(|e| Error::Io(e.to_string()))?;
        let _lock = self.lock()?;
        write_in(&path, format!("{text}\n").as_bytes())
    }

    /// One file from a folder's drawer, as text. `None` when it was never written, and `None`
    /// when it cannot be read: a folder whose drawer is damaged is still a folder.
    pub fn folder_file(&self, folder: &str, name: &str) -> Result<Option<String>, Error> {
        check_drawer_name(name)?;
        read_opt(&self.drawer_path(folder, name)?)
    }

    /// Writes one file into a folder's drawer, whole or not at all.
    pub fn write_folder_file(&self, folder: &str, name: &str, text: &str) -> Result<(), Error> {
        check_drawer_name(name)?;
        let path = self.drawer_path(folder, name)?;
        let _lock = self.lock()?;
        write_in(&path, text.as_bytes())
    }

    /// Takes one file out of a folder's drawer. A file that is not there is not an error.
    pub fn remove_folder_file(&self, folder: &str, name: &str) -> Result<(), Error> {
        check_drawer_name(name)?;
        let path = self.drawer_path(folder, name)?;
        let _lock = self.lock()?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(&path, e)),
        }
    }

    /// The names in one directory of a folder's drawer, sorted. Missing is empty, never an error.
    pub fn folder_files(&self, folder: &str, dir: &str) -> Result<Vec<String>, Error> {
        check_drawer_name(dir)?;
        let path = self.drawer_path(folder, dir)?;
        let mut out: Vec<String> = fs::read_dir(&path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect();
        out.sort();
        Ok(out)
    }

    /// Where a file in `folder`'s drawer lives. The folder must be inside the project.
    fn drawer_path(&self, folder: &str, name: &str) -> Result<PathBuf, Error> {
        let rel = clean_folder(folder)?;
        Ok(self.root().join(rel).join(DOT).join(name))
    }
}

// ── NAMING A FOLDER A FEATURE IS ABOUT TO MAKE ──────────────────────────────────────────────
//
// Ported from the web's `layout/folderNaming.ts`. Build and verification ask the same two
// questions in the same order — "what would this folder be called?" and "is there already one of
// mine holding exactly this?" — and answer only the second differently, so the loop is here and
// the predicate is the argument. Every rule below was a bug first:
//
//   · A construct can be called anything. `pUC19 (v2): round 3` is an ordinary plasmid name and
//     half of it is not a folder name. STRIP, never refuse — the alternative is telling somebody
//     they cannot use a feature because of what they called their plasmid.
//   · NEVER STAMP A TYPE ONTO A FOLDER SOMEBODY MADE BY HAND. A plain folder whose name happens to
//     match is somebody's organisation, and adopting it would be the feature taking their drawer.
//   · The date is spelled out, not localised. A name on disk is an address, not a label.

const NAME_MAX: usize = 64;

/// A construct's name, as a folder name a project will take. `fallback` when nothing is left.
pub fn folder_name_for(construct_name: &str, fallback: &str) -> String {
    let cleaned: String = construct_name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { ' ' } else { c }).filter(|c| !c.is_control()).collect();
    let mut out = String::new();
    for word in cleaned.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    let trimmed: String = out.trim_matches(|c: char| c == '.' || c.is_whitespace()).chars().take(NAME_MAX).collect();
    let trimmed = trimmed.trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() { fallback.to_string() } else { trimmed.to_string() }
}

/// `2 Sep` — short, because it is only ever a disambiguator beside a name that carries the meaning.
pub fn folder_stamp() -> String {
    stamp_of(chrono::Local::now().date_naive())
}

pub(crate) fn stamp_of(day: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.get(day.month0() as usize).copied().unwrap_or("Jan");
    format!("{} {month}", day.day())
}

/// The candidate names, in the order they are tried: the bare name, then dated, then numbered.
///
/// Fifty is not a limit on how many folders anyone may have. It is the point past which a
/// colliding name is worse than a long one.
pub fn folder_candidates(base: &str, stamp: &str) -> Vec<String> {
    let mut out = vec![base.to_string(), format!("{base} {stamp}")];
    out.extend((2..=50).map(|n| format!("{base} {stamp} ({n})")));
    out
}

/// Which folder a feature's work goes in, and what of it to reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderPick {
    pub folder: String,
    /// Set when an existing folder of this feature's own was reused. What it means is the
    /// caller's business — for a build it is the backbone already in there.
    pub reuse: Option<String>,
}

/// Walk the candidates and take the first that is either ours-and-a-match, or free.
///
/// `reuse_of` gets each candidate's record and says what to reuse, or `None` to keep looking.
/// `None` for a folder of your OWN kind skips it rather than claiming it: the folder exists and is
/// not this piece of work, so its name is taken.
pub fn pick_folder(
    base: &str,
    stamp: &str,
    folders: &[String],
    meta_of: impl Fn(&str) -> Option<FolderMeta>,
    ours: impl Fn(&FolderMeta) -> bool,
    reuse_of: impl Fn(&FolderMeta) -> Option<String>,
) -> FolderPick {
    for candidate in folder_candidates(base, stamp) {
        let meta = meta_of(&candidate);
        if let Some(meta) = &meta
            && ours(meta)
        {
            match reuse_of(meta) {
                Some(reuse) => return FolderPick { folder: candidate, reuse: Some(reuse) },
                None => continue,
            }
        }
        // Somebody else's folder, of any kind. Leave it alone.
        if folders.iter().any(|f| f == &candidate) || meta.is_some() {
            continue;
        }
        return FolderPick { folder: cap(&candidate), reuse: None };
    }
    // Fifty folders in one day for one construct. Better a long name than a silent overwrite.
    FolderPick { folder: cap(&format!("{base} {stamp} {}", chrono::Utc::now().timestamp_millis())), reuse: None }
}

fn cap(name: &str) -> String {
    name.chars().take(NAME_MAX).collect::<String>().trim().to_string()
}

fn read_opt(path: &Path) -> Result<Option<String>, Error> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        // A drawer that cannot be read is a folder with nothing remembered, not a broken project.
        Err(_) => Ok(None),
    }
}

fn write_in(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    craft_atomic::atomic_write(path, bytes).map_err(|e| io(path, e))
}

/// A relative path with `/` between its parts, whatever the platform.
fn slashed(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}
