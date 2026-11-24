//! What the Bios window has in front of the person, for an agent working beside it.
//!
//! The window and an agent's `bios-cli mcp` are separate programs. The window writes down what it
//! shows (the project, the place, the plasmid in front, its tabs and the selection) in one small
//! JSON file beside `recents.json`, whenever that changes, and marks it closed when it quits.
//! `what_is_open` reads it, so "this plasmid" means the same thing to the agent as to the person:
//!
//! ```json
//! { "pid": 4242, "updated_at": "2026-11-23T10:00:00Z", "open": true,
//!   "project": { "path": "/Users/me/Lab/AAV", "name": "AAV" }, "place": "edit",
//!   "plasmid": { "id": "…", "path": "backbones/pUC19", "name": "pUC19", "length": 2686, "circular": true },
//!   "tabs": [ { "path": "backbones/pUC19", "name": "pUC19" } ],
//!   "selection": { "start": 10, "end": 20 } }
//! ```
//!
//! One window is written down; several windows at once are not built yet.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::{Error, io, now};
use crate::recents;

/// The file's name inside Bios's settings folder, beside [`recents::FILE`].
pub const FILE: &str = "window.json";

/// The project the window has open.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ShownProject {
    pub path: PathBuf,
    pub name: String,
}

/// A plasmid, as `list_plasmids` names it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ShownPlasmid {
    pub id: String,
    /// Its path in the project as `list_plasmids` shows it ("backbones/pUC19").
    pub path: String,
    pub name: String,
    pub length: usize,
    pub circular: bool,
}

/// A tab in the strip.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub path: String,
    pub name: String,
}

/// The selected bases, 1-based and inclusive, as a person counts them. An end before the start
/// runs through the origin of a circular plasmid.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

/// What the window shows. The window compares it with what it last wrote and writes only a change.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Front {
    /// `None` on the projects page.
    pub project: Option<ShownProject>,
    /// "edit", "build", "verify", or "projects".
    pub place: String,
    pub plasmid: Option<ShownPlasmid>,
    pub tabs: Vec<Tab>,
    pub selection: Option<Selection>,
}

/// What the file holds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Record {
    /// The window's process.
    pub pid: u32,
    /// When it was written, in UTC, as RFC 3339.
    pub updated_at: String,
    /// False once the window has quit.
    pub open: bool,
    #[serde(flatten)]
    pub front: Front,
}

/// The file beside a `recents.json`, so that `BIOS_CONFIG_DIR` moves both.
pub fn file_beside(recents_file: &Path) -> PathBuf {
    recents_file.with_file_name(FILE)
}

/// The file in Bios's settings folder.
pub fn standard_file() -> Option<PathBuf> {
    recents::settings_dir().map(|dir| dir.join(FILE))
}

/// Writes down what this process's window shows.
pub fn publish(file: &Path, front: &Front) -> Result<(), Error> {
    write(file, &Record { pid: std::process::id(), updated_at: now(), open: true, front: front.clone() })
}

/// Marks the window closed: it has quit, so nothing is in front any more.
pub fn closed(file: &Path) -> Result<(), Error> {
    let mut record = read(file).unwrap_or_default();
    if record.pid != std::process::id() {
        // Another window wrote since; it is the one that says when it closes.
        return Ok(());
    }
    record.open = false;
    record.updated_at = now();
    write(file, &record)
}

fn write(file: &Path, record: &Record) -> Result<(), Error> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let text = serde_json::to_string_pretty(record).map_err(|e| Error::Io(e.to_string()))?;
    craft_atomic::atomic_write(file, text.as_bytes()).map_err(|e| io(file, e))
}

/// The file as written, whatever it says. Missing or damaged reads as `None`.
pub fn read(file: &Path) -> Option<Record> {
    fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok())
}

/// What a window that is still running shows: `None` when the file is missing or damaged, the
/// window said it closed, or its process is gone (it crashed or was killed).
pub fn live(file: &Path) -> Option<Record> {
    read(file).filter(|r| r.open && alive(r.pid))
}

/// Whether a process is running, asked of the system without touching it.
fn alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        // `kill -0` sends nothing; it only says whether the process exists.
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).split_whitespace().any(|w| w == pid.to_string()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn front() -> Front {
        Front {
            project: Some(ShownProject { path: PathBuf::from("/lab/AAV"), name: "AAV".into() }),
            place: "edit".into(),
            plasmid: Some(ShownPlasmid { id: "c1".into(), path: "backbones/pUC19".into(), name: "pUC19".into(), length: 2686, circular: true }),
            tabs: vec![Tab { path: "backbones/pUC19".into(), name: "pUC19".into() }],
            selection: Some(Selection { start: 10, end: 20 }),
        }
    }

    #[test]
    fn what_is_written_reads_back_while_the_window_runs() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_beside(&dir.path().join(recents::FILE));
        assert_eq!(file, dir.path().join(FILE));
        publish(&file, &front()).unwrap();
        let record = live(&file).unwrap();
        assert_eq!(record.front, front());
        assert_eq!(record.pid, std::process::id());
        assert!(chrono::DateTime::parse_from_rfc3339(&record.updated_at).is_ok());
        assert!(!dir.path().join("window.json.tmp").exists());
    }

    #[test]
    fn a_missing_or_damaged_file_is_no_window() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        assert_eq!(live(&file), None);
        fs::write(&file, "{ not json").unwrap();
        assert_eq!(read(&file), None);
        assert_eq!(live(&file), None);
    }

    #[test]
    fn a_window_that_quit_or_died_is_no_window() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        publish(&file, &front()).unwrap();
        closed(&file).unwrap();
        assert!(read(&file).is_some_and(|r| !r.open));
        assert_eq!(live(&file), None);

        // A process that has ended: the window crashed without saying so.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        write(&file, &Record { pid, updated_at: now(), open: true, front: front() }).unwrap();
        assert_eq!(live(&file), None);
        // Another window's file is not marked closed by this process.
        closed(&file).unwrap();
        assert!(read(&file).is_some_and(|r| r.open));
    }
}
