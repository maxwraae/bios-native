//! What a sync did, in sentences that can be shown to a person as they are.

use bios_project::ConstructId;

/// One thing that happened to one plasmid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub construct: ConstructId,
    pub name: String,
    /// The whole sentence, naming the plasmid.
    pub what: String,
}

/// A plasmid that differs here and on the server in a way sync will not settle by itself.
/// Nothing was changed on either side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub construct: ConstructId,
    pub name: String,
    /// When the version here was saved, in UTC as RFC 3339, when known.
    pub mine_saved: Option<String>,
    /// When the version on the server was saved, as the server wrote it, when known.
    pub theirs_saved: Option<String>,
    pub what: String,
}

/// Something sync left undone, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// Absent when the reason is about the whole sync rather than one plasmid.
    pub construct: Option<ConstructId>,
    pub name: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The name the project's server goes by.
    pub server: String,
    /// Changed on the server: brought in as a new version here.
    pub brought_in: Vec<Item>,
    /// Changed here: saved on the server.
    pub sent: Vec<Item>,
    /// New on the server: added here.
    pub added_here: Vec<Item>,
    /// New here: added on the server.
    pub added_there: Vec<Item>,
    /// The same plasmid was already on both sides; the two copies are now kept in step.
    pub matched: Vec<Item>,
    /// A rename or a move made on one side and carried to the other.
    pub carried: Vec<Item>,
    pub conflicts: Vec<Conflict>,
    /// No longer on the server. The plasmid here is kept and is no longer shared.
    pub gone: Vec<Item>,
    pub skipped: Vec<Skipped>,
}

impl Report {
    /// Nothing is left for a person to decide or to try again.
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty() && self.skipped.is_empty()
    }

    /// True when the sync found the two sides already in step and did nothing.
    pub fn is_empty(&self) -> bool {
        self.lines().is_empty()
    }

    /// Every sentence of the report: what needs a person first, then what was done.
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.conflicts.iter().map(|c| c.what.clone()).collect();
        out.extend(self.skipped.iter().map(|s| s.why.clone()));
        out.extend(self.gone.iter().map(|i| i.what.clone()));
        for items in [&self.brought_in, &self.added_here, &self.sent, &self.added_there, &self.matched, &self.carried] {
            out.extend(items.iter().map(|i| i.what.clone()));
        }
        out
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines = self.lines();
        if lines.is_empty() {
            return write!(f, "Everything is in step with {}.", self.server);
        }
        f.write_str(&lines.join("\n"))
    }
}
