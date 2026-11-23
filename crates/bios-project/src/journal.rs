//! The change journal: one line of JSON per write to a construct, in order, never rewritten.
//!
//! The fields follow the journal the Bios server keeps (`construct_op`), so that a project in a
//! folder and a project on a server record the same things.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who made a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    Human,
    Agent,
    /// Not an edit: a file brought in, or found changed by another program.
    Import,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// 1 for the entry that created the construct, then one more for each write.
    pub version: u64,
    /// When, in UTC, as RFC 3339.
    pub at: String,
    pub actor: Actor,
    /// Where the change came from, such as `window`, `cli` or `mcp`.
    pub source: String,
    /// Groups the changes of one sitting, or of one agent run, so they can be shown together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The engine command that made the change, and its parameters.
    pub command: String,
    #[serde(default)]
    pub params: Value,
    /// One sentence a person can read.
    pub summary: String,
    /// The sha256 of the file before the write. Absent on the entry that created the construct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    /// The sha256 of the file after the write. Its text is kept under `objects/`.
    pub result_sha: String,
    /// On the first entry only: the construct this one was made from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    /// On the first entry only: where an imported file came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// Every entry in the journal at `path`, oldest first. A journal that does not exist is empty.
pub fn read(path: &Path) -> Result<Vec<Entry>, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut entries = Vec::new();
    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {e}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        entries.push(serde_json::from_str(&line).map_err(|e| format!("{} line {}: {e}", path.display(), n + 1))?);
    }
    Ok(entries)
}

/// Adds `entry` to the end of the journal at `path` and flushes it to disk.
pub fn append(path: &Path, entry: &Entry) -> Result<(), String> {
    let mut line = serde_json::to_string(entry).map_err(|e| format!("{}: {e}", path.display()))?;
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(line.as_bytes()).and_then(|()| file.sync_all()).map_err(|e| format!("{}: {e}", path.display()))
}
