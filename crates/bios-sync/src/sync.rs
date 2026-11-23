//! Keeping a project folder in step with the same project on a server.
//!
//! The project remembers, in `.bios/remote.json`, which server project it is shared as and, for
//! each plasmid, the version on each side when the two were last in step. From that a sync can
//! tell which side changed since:
//!
//! - neither: nothing to do;
//! - only the server: its version is brought in as a new version here;
//! - only here: it is sent, and the server refuses it if its copy moved meanwhile;
//! - both: **nothing is merged and neither side is overwritten**. Both stay as they are, the
//!   report says so, and [`resolve`] settles it when a person has chosen.
//!
//! A name and a folder are followed the same way, each on its own.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bios_core::{Construct, genbank};
use bios_project::{Actor, Change, ConstructId, Error as ProjectError, Listed, Loaded, Project, Who};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::gateway::{Gateway, GatewayError, ProjectListing, RemoteConstruct, RemoteDocument, Role, SaveOutcome, folder_of};
use crate::report::{Conflict, Item, Report, Skipped};
use crate::snapshot::Snapshot;

/// The sidecar the binding is kept in: `.bios/remote.json`.
const SIDECAR: &str = "remote";
const FORMAT: u32 = 1;

/// Which side wins when a person settles a conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The version here is saved on top of the server's current one.
    Mine,
    /// The server's version becomes the newest version here. The one it replaces stays in the
    /// history and can be gone back to.
    Theirs,
}

/// Where a project is shared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    /// The name the server goes by, as given when the project was connected or published.
    pub server: String,
    /// The project's id on that server.
    pub project: String,
    /// How many plasmids are kept in step.
    pub constructs: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncError {
    Project(ProjectError),
    Gateway(GatewayError),
    /// The project has never been connected or published.
    NotConnected,
    AlreadyConnected {
        server: String,
    },
    /// The server no longer lists the project for this account.
    ProjectGone {
        server: String,
    },
    /// The plasmid is not one that is kept in step, so there is nothing to settle.
    NotShared(String),
    /// `.bios/remote.json` is not something this Bios can read.
    Binding(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Project(e) => write!(f, "{e}"),
            SyncError::Gateway(e) => write!(f, "{e}"),
            SyncError::NotConnected => write!(f, "this project is not shared on a server yet; connect it to a project there, or publish it, first"),
            SyncError::AlreadyConnected { server } => {
                write!(f, "this project is already shared on {server}; a project is kept in step with one project on one server")
            }
            SyncError::ProjectGone { server } => {
                write!(f, "this project is no longer on {server}, or this account may no longer open it; nothing was changed here")
            }
            SyncError::NotShared(name) => write!(f, "{name} is not shared on the server, so there is nothing to settle"),
            SyncError::Binding(why) => write!(f, "the record of where this project is shared could not be read ({why}); nothing was changed"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<ProjectError> for SyncError {
    fn from(e: ProjectError) -> Self {
        SyncError::Project(e)
    }
}

impl From<GatewayError> for SyncError {
    fn from(e: GatewayError) -> Self {
        SyncError::Gateway(e)
    }
}

/// What `.bios/remote.json` holds.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Binding {
    format: u32,
    /// The name the server goes by. Never an address or a key.
    server: String,
    /// The project's id on the server.
    project: String,
    /// By the construct's id here.
    #[serde(default)]
    constructs: BTreeMap<String, Bound>,
    /// Plasmids here whose copy on the server went away. They are kept and are not sent again
    /// by themselves: sending one back would undo a deletion someone made on purpose.
    #[serde(default)]
    detached: BTreeSet<String>,
}

/// One plasmid on both sides, as the two were when they were last in step.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Bound {
    /// The construct's id on the server.
    remote: String,
    /// The server's version token then. A save presents it.
    etag: String,
    /// The sha of the file here then.
    sha: String,
    /// What it was called here then.
    name: String,
    /// The folder it was in here then.
    folder: String,
    /// What the server's listing called it then. `None` until a listing has been seen since the
    /// last time this side changed it; whatever the listing says next is then taken as the start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_name: Option<String>,
    /// The folder it was in on the server then, with the same meaning of `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_folder: Option<String>,
    /// A save that was started. Kept until its answer arrives, so that a repeat after a lost
    /// answer carries the same request id and is not applied twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<Pending>,
    /// Saved on the server, but the project's listing there has not been refreshed from it yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    relist: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Pending {
    request_id: String,
    /// The sha of the file here that was being sent.
    sha: String,
}

fn load(project: &Project) -> Result<Option<Binding>, SyncError> {
    let Some(text) = project.read_sidecar(SIDECAR)? else {
        return Ok(None);
    };
    let binding: Binding = serde_json::from_str(&text).map_err(|e| SyncError::Binding(e.to_string()))?;
    if binding.format > FORMAT {
        return Err(SyncError::Binding("it was written by a newer Bios".into()));
    }
    Ok(Some(binding))
}

fn store(project: &Project, binding: &Binding) -> Result<(), SyncError> {
    let text = serde_json::to_string_pretty(binding).map_err(|e| SyncError::Binding(e.to_string()))?;
    Ok(project.write_sidecar(SIDECAR, &text)?)
}

/// Where this project is shared, or `None` when it is not.
pub fn connection(project: &Project) -> Result<Option<Connection>, SyncError> {
    Ok(load(project)?.map(|b| Connection { server: b.server, project: b.project, constructs: b.constructs.len() }))
}

/// Binds this project to the project `remote_project_id` on the server called `server_name`, and
/// brings in everything that is there. Plasmids already here are sent, except one that is the
/// same molecule as one on the server: those two are linked instead of doubled.
///
/// `server_name` is what the server is called in reports and in the history ("lab server"). It
/// is written into the project, so it must not be an address that holds a key.
pub fn connect(project: &Project, gateway: &dyn Gateway, server_name: &str, remote_project_id: &str, who: &Who) -> Result<Report, SyncError> {
    let server = server_label(server_name);
    if let Some(existing) = load(project)? {
        if existing.project != remote_project_id || existing.server != server {
            return Err(SyncError::AlreadyConnected { server: existing.server });
        }
        return sync(project, gateway, who);
    }
    let role = role_in(gateway, remote_project_id, &server)?;
    let binding = Binding { format: FORMAT, server, project: remote_project_id.to_string(), constructs: BTreeMap::new(), detached: BTreeSet::new() };
    store(project, &binding)?;
    Run::new(project, gateway, binding, role, who).everything()
}

/// Makes a new project called `title` on the server and sends every plasmid of this one to it.
pub fn publish(project: &Project, gateway: &dyn Gateway, server_name: &str, title: &str, who: &Who) -> Result<Report, SyncError> {
    if let Some(existing) = load(project)? {
        return Err(SyncError::AlreadyConnected { server: existing.server });
    }
    let remote = gateway.create_project(title)?;
    let binding =
        Binding { format: FORMAT, server: server_label(server_name), project: remote.id, constructs: BTreeMap::new(), detached: BTreeSet::new() };
    store(project, &binding)?;
    Run::new(project, gateway, binding, remote.role, who).everything()
}

/// Brings in what changed on the server and sends what changed here. See the module notes for
/// the rules. An error means nothing could be started; everything that happened to a single
/// plasmid, good or bad, is in the report.
pub fn sync(project: &Project, gateway: &dyn Gateway, who: &Who) -> Result<Report, SyncError> {
    let binding = load(project)?.ok_or(SyncError::NotConnected)?;
    let role = role_in(gateway, &binding.project, &binding.server)?;
    Run::new(project, gateway, binding, role, who).everything()
}

/// Settles one plasmid that differs on the two sides, the way the person chose.
pub fn resolve(project: &Project, gateway: &dyn Gateway, construct: &ConstructId, keep: Keep, who: &Who) -> Result<Report, SyncError> {
    let binding = load(project)?.ok_or(SyncError::NotConnected)?;
    let listed = project.list()?.into_iter().find(|l| &l.id == construct).ok_or_else(|| ProjectError::UnknownConstruct(construct.to_string()))?;
    let Some(entry) = binding.constructs.get(construct.as_str()).cloned() else {
        return Err(SyncError::NotShared(listed.name));
    };
    let role = role_in(gateway, &binding.project, &binding.server)?;
    let listing = gateway.list_constructs(&binding.project)?;
    let mut run = Run::new(project, gateway, binding, role, who);
    run.note_paths(&listing);
    match listing.constructs.iter().find(|c| c.id == entry.remote) {
        Some(remote) => run.decide(&listed, entry, remote, keep)?,
        None => run.missing_there(&Here::of(&listed), &entry)?,
    }
    run.finish()?;
    Ok(run.report)
}

/// Lets a plasmid whose copy on the server went away be sent again, as a new plasmid there, by
/// the next sync.
pub fn send_again(project: &Project, construct: &ConstructId) -> Result<(), SyncError> {
    let mut binding = load(project)?.ok_or(SyncError::NotConnected)?;
    if binding.detached.remove(construct.as_str()) {
        store(project, &binding)?;
    }
    Ok(())
}

fn server_label(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() { "the server".to_string() } else { name.to_string() }
}

/// What this account may do in the project. A project the server does not list is not synced
/// at all: nothing here is touched on the strength of a project that cannot be seen.
fn role_in(gateway: &dyn Gateway, project: &str, server: &str) -> Result<Role, SyncError> {
    gateway
        .list_projects()?
        .into_iter()
        .find(|p| p.id == project)
        .map(|p| p.role)
        .ok_or_else(|| SyncError::ProjectGone { server: server.to_string() })
}

/// One plasmid here, as the sync sees it now.
struct Here {
    id: ConstructId,
    name: String,
    sha: String,
    /// With `/` between its parts; empty at the top of the project.
    folder: String,
}

impl Here {
    fn of(listed: &Listed) -> Here {
        Here { id: listed.id.clone(), name: listed.name.clone(), sha: listed.sha.clone(), folder: folder_here(&listed.path) }
    }

    /// After a write here: the file is the truth for the name, the place and the sha.
    fn take(&mut self, loaded: &Loaded) {
        self.sha = loaded.sha.clone();
        self.folder = folder_here(&loaded.path);
        if let Some(stem) = loaded.path.file_stem() {
            self.name = stem.to_string_lossy().into_owned();
        }
    }
}

/// A path inside the project with `/` between its parts, whatever the platform.
fn slashed(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn folder_here(path: &Path) -> String {
    path.parent().map(slashed).unwrap_or_default()
}

fn sha_of(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A time the server wrote, as a person reads it. `None` when it is not a time.
fn when(at: Option<&str>) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(at?).ok()?;
    Some(at.with_timezone(&chrono::Utc).format("%Y-%m-%d %H:%M UTC").to_string())
}

/// The construct the server's document describes, or why it cannot be brought in.
fn readable(doc: &RemoteDocument) -> Result<Construct, String> {
    let construct = genbank::read(&doc.text).map_err(|e| format!("its file there could not be read ({e})"))?;
    // The server can hold bases that are newer than its text. Bringing the text in would bring
    // in an old sequence under a new version, so it is left until the text has caught up.
    if !construct.sequence.eq_ignore_ascii_case(&doc.sequence) || construct.circular != doc.circular {
        return Err("its file there is older than its sequence; open it in the Bios web app and save it, then sync again".to_string());
    }
    Ok(construct)
}

/// The same molecule with the same annotation, whatever the two are called.
fn same_molecule(a: &Construct, b: &Construct) -> bool {
    a.circular == b.circular && a.sequence == b.sequence && a.features == b.features
}

fn in_folder(folder: &str) -> String {
    if folder.is_empty() { "the top of the project".to_string() } else { folder.to_string() }
}

struct Run<'a> {
    project: &'a Project,
    gateway: &'a dyn Gateway,
    binding: Binding,
    /// Who the history names for what comes from the server.
    from_server: Who,
    can_write: bool,
    /// Where the server keeps each construct now, by its id there.
    paths: BTreeMap<String, String>,
    report: Report,
    /// The server stopped answering; nothing more is asked of it in this run.
    stopped: bool,
}

impl<'a> Run<'a> {
    fn new(project: &'a Project, gateway: &'a dyn Gateway, binding: Binding, role: Role, who: &Who) -> Run<'a> {
        let from_server = Who { actor: Actor::Import, source: binding.server.clone(), session: who.session.clone() };
        let report = Report { server: binding.server.clone(), ..Report::default() };
        Run { project, gateway, binding, from_server, can_write: role.can_write(), paths: BTreeMap::new(), report, stopped: false }
    }

    fn server(&self) -> String {
        self.binding.server.clone()
    }

    fn everything(mut self) -> Result<Report, SyncError> {
        let listing = self.gateway.list_constructs(&self.binding.project)?;
        self.note_paths(&listing);
        let local = self.project.list()?;
        self.bound(&local, &listing)?;
        self.new_there(&local, &listing)?;
        self.new_here(&local)?;
        self.finish()?;
        self.folders(&listing);
        if self.stopped {
            let why = format!("{} stopped answering, so the rest was left for the next sync.", self.server());
            self.report.skipped.push(Skipped { construct: None, name: String::new(), why });
        }
        Ok(self.report)
    }

    fn note_paths(&mut self, listing: &ProjectListing) {
        self.paths = listing.constructs.iter().map(|c| (c.id.clone(), c.path.clone())).collect();
    }

    // ── what is kept and what is said ──────────────────────────────────────────────────────

    fn keep(&mut self, id: &ConstructId, entry: Bound) -> Result<(), SyncError> {
        self.binding.constructs.insert(id.to_string(), entry);
        store(self.project, &self.binding)
    }

    fn skip(&mut self, construct: Option<&ConstructId>, name: &str, why: String) {
        self.report.skipped.push(Skipped { construct: construct.cloned(), name: name.to_string(), why });
    }

    /// A request about one plasmid failed. If the server cannot be reached at all there is no
    /// point in waiting out the same failure once per plasmid.
    fn failed(&mut self, construct: Option<&ConstructId>, name: &str, doing: &str, e: GatewayError) {
        if matches!(e, GatewayError::Unreachable(_) | GatewayError::NotSignedIn) {
            self.stopped = true;
        }
        self.skip(construct, name, format!("{name} was not {doing}: {e}."));
    }

    fn item(here: &Here, what: String) -> Item {
        Item { construct: here.id.clone(), name: here.name.clone(), what }
    }

    fn not_allowed(&self, name: &str, doing: &str) -> String {
        format!("{name} was not {doing}: you can view this project on {} but not change it.", self.server())
    }

    /// The copy on the server is gone. The plasmid here is kept, and from now on it is only here.
    fn detach(&mut self, here: &Here, what: String) -> Result<(), SyncError> {
        self.binding.constructs.remove(here.id.as_str());
        self.binding.detached.insert(here.id.to_string());
        store(self.project, &self.binding)?;
        self.report.gone.push(Self::item(here, what));
        Ok(())
    }

    fn conflict(&mut self, here: &Here, theirs_saved: Option<String>, what: String) -> Result<(), SyncError> {
        let mine_saved = self.saved_here(here)?;
        self.report.conflicts.push(Conflict { construct: here.id.clone(), name: here.name.clone(), mine_saved, theirs_saved, what });
        Ok(())
    }

    /// When the text that is here now was first saved: later entries that left it as it is, such
    /// as a move, do not count.
    fn saved_here(&self, here: &Here) -> Result<Option<String>, SyncError> {
        let history = self.project.history(&here.id)?;
        Ok(history.iter().rev().take_while(|e| e.result_sha == here.sha).last().map(|e| e.at.clone()))
    }

    fn changed_on_both(&self, here: &Here, mine: Option<&str>, theirs: Option<&str>) -> String {
        let at = |t: Option<&str>| when(t).map(|t| format!(" (saved {t})")).unwrap_or_default();
        format!(
            "{} was changed both here{} and on {}{}. Neither was touched: choose which one to keep.",
            here.name,
            at(mine),
            self.server(),
            at(theirs)
        )
    }

    fn brought_summary(&self, doc: &RemoteDocument) -> String {
        match when(doc.updated_at.as_deref()) {
            Some(at) => format!("Brought in from {}, saved there {at}", self.server()),
            None => format!("Brought in from {}", self.server()),
        }
    }

    fn origin(&self, doc: &RemoteDocument) -> serde_json::Value {
        json!({ "server": self.server(), "construct": doc.construct_id, "version": doc.version, "saved": doc.updated_at })
    }

    // ── plasmids that are on both sides ────────────────────────────────────────────────────

    fn bound(&mut self, local: &[Listed], listing: &ProjectListing) -> Result<(), SyncError> {
        for (id, entry) in self.binding.constructs.clone() {
            if self.stopped {
                break;
            }
            let remote = listing.constructs.iter().find(|c| c.id == entry.remote);
            match (local.iter().find(|l| l.id.as_str() == id), remote) {
                (Some(listed), Some(remote)) => self.reconcile(listed, entry, remote)?,
                (Some(listed), None) => self.missing_there(&Here::of(listed), &entry)?,
                (None, Some(_)) => {
                    let why = format!(
                        "{} was deleted here and is still on {}. Sync does not delete there: bring it back here, or delete it there as well.",
                        entry.name,
                        self.server()
                    );
                    self.skip(ConstructId::parse(&id).as_ref(), &entry.name, why);
                }
                // Gone from both sides: there is nothing left to keep in step.
                (None, None) => {
                    self.binding.constructs.remove(&id);
                    store(self.project, &self.binding)?;
                }
            }
        }
        Ok(())
    }

    /// The project's listing no longer has it. Only "not found" for the document itself, or its
    /// absence from a listing that did arrive, counts as gone: any other failure is "could not
    /// see", and nothing is concluded from that.
    fn missing_there(&mut self, here: &Here, entry: &Bound) -> Result<(), SyncError> {
        let server = self.server();
        match self.gateway.read_document(&entry.remote) {
            Err(GatewayError::NotFound(_)) => {
                self.detach(here, format!("{} is no longer on {server}. It is kept here and is no longer shared.", here.name))
            }
            Ok(_) => self.detach(here, format!("{} was removed from the project on {server}. It is kept here and is no longer shared.", here.name)),
            Err(e) => {
                self.failed(Some(&here.id), &here.name, "checked", e);
                Ok(())
            }
        }
    }

    fn reconcile(&mut self, listed: &Listed, mut entry: Bound, remote: &RemoteConstruct) -> Result<(), SyncError> {
        let mut here = Here::of(listed);
        let outcome = self.settle(&mut here, &mut entry, remote);
        // Whatever was learned on the way is kept, unless the plasmid stopped being shared.
        if self.binding.constructs.contains_key(here.id.as_str()) {
            self.keep(&here.id, entry)?;
        }
        outcome
    }

    fn settle(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct) -> Result<(), SyncError> {
        self.carry_move(here, entry, remote)?;
        let local_changed = here.sha != entry.sha;
        let local_renamed = here.name != entry.name;
        // A listing that gives no name is not a rename to nothing.
        let named_there = !remote.name.trim().is_empty();
        let remote_renamed = named_there && entry.remote_name.as_ref().is_some_and(|was| *was != remote.name);
        if entry.remote_name.is_none() && named_there {
            entry.remote_name = Some(remote.name.clone());
        }
        // A file renamed here by another program: the same text under a new name. A name has no
        // version, so it goes across by itself.
        if local_renamed && !local_changed && !remote_renamed {
            self.rename_there(here, entry);
        }
        let Some(doc) = self.fetch(here, entry)? else {
            return Ok(());
        };
        if local_renamed && remote_renamed {
            if remote.name != here.name {
                let what = format!(
                    "{} was renamed here, and on {} it is now called {}. Neither was touched: choose which one to keep.",
                    here.name,
                    self.server(),
                    remote.name
                );
                return self.conflict(here, doc.updated_at.clone(), what);
            }
            entry.name = here.name.clone();
            entry.remote_name = Some(remote.name.clone());
        }
        let remote_changed = doc.etag != entry.etag;
        // A save that was started and never answered is repeated with the same request id, even
        // when the server's copy has moved: the move may be that very save.
        let repeating = entry.pending.as_ref().is_some_and(|p| p.sha == here.sha);
        let carry_name = remote_renamed && !local_renamed;
        match (local_changed, remote_changed) {
            (true, true) if !repeating => {
                let mine = self.saved_here(here)?;
                let what = self.changed_on_both(here, mine.as_deref(), doc.updated_at.as_deref());
                self.conflict(here, doc.updated_at.clone(), what)
            }
            (false, true) => {
                let name = if carry_name { remote.name.clone() } else { here.name.clone() };
                self.bring_in(here, entry, remote, &doc, &name)
            }
            (false, false) if carry_name => self.rename_here(here, entry, remote).map(|_| ()),
            (false, false) => Ok(()),
            (true, _) => {
                // Sending carries the name; a rename made there is taken first so it is not undone.
                if carry_name && !self.rename_here(here, entry, remote)? {
                    return Ok(());
                }
                self.send(here, entry)
            }
        }
    }

    /// The server's copy as it is now, or `None` when there is nothing more to do for this
    /// plasmid in this run.
    fn fetch(&mut self, here: &Here, entry: &Bound) -> Result<Option<RemoteDocument>, SyncError> {
        if self.stopped {
            return Ok(None);
        }
        match self.gateway.read_document(&entry.remote) {
            Ok(doc) => Ok(Some(doc)),
            Err(GatewayError::NotFound(_)) => {
                let what = format!("{} is no longer on {}. It is kept here and is no longer shared.", here.name, self.server());
                self.detach(here, what)?;
                Ok(None)
            }
            Err(e) => {
                self.failed(Some(&here.id), &here.name, "checked", e);
                Ok(None)
            }
        }
    }

    /// A move made on one side is made on the other. Moved to different places on both: said,
    /// and left.
    fn carry_move(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct) -> Result<(), SyncError> {
        let Some(was_there) = entry.remote_folder.clone() else {
            entry.remote_folder = Some(remote.folder.clone());
            return Ok(());
        };
        let server = self.server();
        match (here.folder != entry.folder, remote.folder != was_there) {
            (false, false) => {}
            (true, true) if here.folder == remote.folder => {
                entry.folder = here.folder.clone();
                entry.remote_folder = Some(remote.folder.clone());
            }
            (true, true) => {
                let why = format!(
                    "{} was moved to {} here and to {} on {server}. Neither was touched: move one of them so they agree.",
                    here.name,
                    in_folder(&here.folder),
                    in_folder(&remote.folder)
                );
                self.skip(Some(&here.id), &here.name, why);
            }
            (true, false) if !self.can_write => {
                let why = self.not_allowed(&here.name, &format!("moved on {server}"));
                self.skip(Some(&here.id), &here.name, why);
            }
            (true, false) => match self.gateway.move_construct(&self.binding.project, &entry.remote, &here.folder) {
                Ok(path) => {
                    entry.folder = here.folder.clone();
                    entry.remote_folder = Some(folder_of(&path));
                    self.paths.insert(entry.remote.clone(), path);
                    self.report
                        .carried
                        .push(Self::item(here, format!("{} was moved to {} on {server}, as it is here.", here.name, in_folder(&here.folder))));
                }
                Err(e) => self.failed(Some(&here.id), &here.name, &format!("moved on {server}"), e),
            },
            (false, true) => self.move_here(here, entry, remote)?,
        }
        Ok(())
    }

    fn move_here(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct) -> Result<(), SyncError> {
        let server = self.server();
        match self.project.move_to(&here.id, &remote.folder, &self.from_server) {
            Ok(moved) => {
                here.take(&moved);
                entry.folder = here.folder.clone();
                entry.remote_folder = Some(remote.folder.clone());
                self.report.carried.push(Self::item(here, format!("{} was moved to {}, as it is on {server}.", here.name, in_folder(&here.folder))));
                Ok(())
            }
            Err(e @ (ProjectError::NameTaken(_) | ProjectError::BadFolder(_))) => {
                let why = format!("{} is in {} on {server} and was not moved there here: {e}.", here.name, in_folder(&remote.folder));
                self.skip(Some(&here.id), &here.name, why);
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn rename_there(&mut self, here: &Here, entry: &mut Bound) {
        let server = self.server();
        if !self.can_write {
            let why = self.not_allowed(&here.name, &format!("renamed on {server}"));
            return self.skip(Some(&here.id), &here.name, why);
        }
        match self.gateway.rename_construct(&self.binding.project, &entry.remote, &here.name) {
            Ok(kept) => {
                entry.name = here.name.clone();
                entry.remote_name = Some(kept);
                self.report.carried.push(Self::item(here, format!("{} was renamed on {server}, as it is here.", here.name)));
            }
            Err(e) => self.failed(Some(&here.id), &here.name, &format!("renamed on {server}"), e),
        }
    }

    /// Takes the name the server's listing has. Gives `false` when that could not be done and
    /// the plasmid is left for now.
    fn rename_here(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct) -> Result<bool, SyncError> {
        let server = self.server();
        let loaded = self.project.read(&here.id)?;
        let mut construct = loaded.construct.clone();
        construct.name = remote.name.clone();
        let change = Change {
            command: "sync.rename".into(),
            params: json!({ "server": server, "name": remote.name }),
            summary: format!("Renamed to {} on {server}", remote.name),
        };
        match self.project.save(&here.id, &loaded.sha, &construct, &self.from_server, change) {
            Ok(saved) => {
                let was = here.name.clone();
                let in_step = here.sha == entry.sha;
                here.take(&saved);
                if in_step {
                    entry.sha = here.sha.clone();
                }
                entry.name = here.name.clone();
                entry.remote_name = Some(remote.name.clone());
                if was != here.name {
                    self.report.carried.push(Self::item(here, format!("{was} is now called {}, as it is on {server}.", here.name)));
                }
                Ok(true)
            }
            Err(ProjectError::NameTaken(_)) => {
                let why = format!(
                    "{} is called {} on {server}, and another plasmid here already has that name. Rename one of them.",
                    here.name, remote.name
                );
                self.skip(Some(&here.id), &here.name, why);
                Ok(false)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The server's version becomes the newest version here, under `name`.
    fn bring_in(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct, doc: &RemoteDocument, name: &str) -> Result<(), SyncError> {
        let server = self.server();
        let mut construct = match readable(doc) {
            Ok(construct) => construct,
            Err(why) => {
                self.skip(Some(&here.id), &here.name, format!("{} was not brought in from {server}: {why}.", here.name));
                return Ok(());
            }
        };
        // A plasmid keeps the name it has here unless the server's listing gave it another.
        construct.name = if name.trim().is_empty() { here.name.clone() } else { name.to_string() };
        let change = Change { command: "sync.bring_in".into(), params: self.origin(doc), summary: self.brought_summary(doc) };
        match self.project.save(&here.id, &here.sha, &construct, &self.from_server, change) {
            Ok(saved) => {
                here.take(&saved);
                entry.etag = doc.etag.clone();
                entry.sha = here.sha.clone();
                entry.name = here.name.clone();
                entry.remote_name = Some(remote.name.clone());
                entry.pending = None;
                let at = when(doc.updated_at.as_deref()).map(|t| format!(", saved there {t}")).unwrap_or_default();
                self.report.brought_in.push(Self::item(here, format!("{} was brought in from {server}{at}.", here.name)));
                Ok(())
            }
            Err(e @ (ProjectError::NameTaken(_) | ProjectError::Conflict { .. })) => {
                self.skip(Some(&here.id), &here.name, format!("{} was not brought in from {server}: {e}.", here.name));
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Saves the version here on the server, on top of the version `entry.etag` names.
    fn send(&mut self, here: &Here, entry: &mut Bound) -> Result<(), SyncError> {
        let server = self.server();
        if !self.can_write {
            let why = self.not_allowed(&here.name, "sent");
            self.skip(Some(&here.id), &here.name, why);
            return Ok(());
        }
        let loaded = self.project.read(&here.id)?;
        let snapshot = match Snapshot::of(&loaded.construct) {
            Ok(snapshot) => snapshot,
            Err(e) => {
                self.skip(Some(&here.id), &here.name, format!("{} was not sent to {server}: {e}.", here.name));
                return Ok(());
            }
        };
        let request_id = match &entry.pending {
            Some(pending) if pending.sha == loaded.sha => pending.request_id.clone(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        // Written down before it is sent, so that a repeat after a crash or a lost answer is
        // recognised by the server as the same save.
        entry.pending = Some(Pending { request_id: request_id.clone(), sha: loaded.sha.clone() });
        self.keep(&here.id, entry.clone())?;
        match self.gateway.save_document(&entry.remote, &entry.etag, &snapshot, &request_id) {
            Ok(SaveOutcome::Saved(now)) => {
                entry.pending = None;
                entry.sha = loaded.sha.clone();
                entry.name = here.name.clone();
                entry.remote_name = None;
                // A repeated save is answered with the server's copy as it is now, which someone
                // may have saved over since. Only the etag of the text that was sent is in step
                // with it; otherwise the old one stays and the next sync brings theirs in.
                if now.etag == format!("{}:{}", now.version, sha_of(&snapshot.text)) {
                    entry.etag = now.etag;
                    entry.relist = true;
                }
                self.report.sent.push(Self::item(here, format!("{} was sent to {server}.", here.name)));
                Ok(())
            }
            Ok(SaveOutcome::Conflict(current)) => {
                entry.pending = None;
                self.refused(here, entry, current)
            }
            Err(GatewayError::NotFound(_)) => {
                self.detach(here, format!("{} is no longer on {server}. It is kept here and is no longer shared.", here.name))
            }
            Err(e) => {
                // A refusal is an answer: the save did not happen and a repeat would be a new one.
                if matches!(e, GatewayError::Refused { .. }) {
                    entry.pending = None;
                }
                self.failed(Some(&here.id), &here.name, &format!("sent to {server}"), e);
                Ok(())
            }
        }
    }

    /// The server did not take the save. If its copy moved, that is a conflict. If it did not,
    /// the refusal is about who is asking: a viewer's save is answered this way.
    fn refused(&mut self, here: &Here, entry: &Bound, current: Option<RemoteDocument>) -> Result<(), SyncError> {
        let server = self.server();
        match current {
            Some(current) if current.etag == entry.etag => {
                let why = format!(
                    "{} was not sent: {server} refused the change although nothing changed there. You may be allowed to view this project but not change it.",
                    here.name
                );
                self.skip(Some(&here.id), &here.name, why);
                Ok(())
            }
            current => {
                let theirs = current.and_then(|c| c.updated_at);
                let mine = self.saved_here(here)?;
                let what = self.changed_on_both(here, mine.as_deref(), theirs.as_deref());
                self.conflict(here, theirs, what)
            }
        }
    }

    /// One plasmid, settled the way a person chose.
    fn decide(&mut self, listed: &Listed, mut entry: Bound, remote: &RemoteConstruct, keep: Keep) -> Result<(), SyncError> {
        let mut here = Here::of(listed);
        let outcome = self.decide_one(&mut here, &mut entry, remote, keep);
        if self.binding.constructs.contains_key(here.id.as_str()) {
            self.keep(&here.id, entry)?;
        }
        outcome
    }

    fn decide_one(&mut self, here: &mut Here, entry: &mut Bound, remote: &RemoteConstruct, keep: Keep) -> Result<(), SyncError> {
        let Some(doc) = self.fetch(here, entry)? else {
            return Ok(());
        };
        match keep {
            Keep::Theirs => self.bring_in(here, entry, remote, &doc, &remote.name),
            Keep::Mine => {
                let named_alike = entry.remote_name.as_ref().is_none_or(|was| *was == remote.name) && here.name == entry.name;
                if doc.etag == entry.etag && here.sha == entry.sha && named_alike {
                    return Ok(());
                }
                // The person has seen both and chose this one: it goes on top of what is there now.
                entry.etag = doc.etag.clone();
                entry.pending = None;
                self.send(here, entry)
            }
        }
    }

    // ── plasmids that are on one side only ─────────────────────────────────────────────────

    fn unbound<'l>(&self, local: &'l [Listed]) -> Vec<&'l Listed> {
        local.iter().filter(|l| !self.binding.constructs.contains_key(l.id.as_str()) && !self.binding.detached.contains(l.id.as_str())).collect()
    }

    fn new_there(&mut self, local: &[Listed], listing: &ProjectListing) -> Result<(), SyncError> {
        let mut known: BTreeSet<String> = self.binding.constructs.values().map(|b| b.remote.clone()).collect();
        let news: Vec<&RemoteConstruct> = listing.constructs.iter().filter(|c| known.insert(c.id.clone())).collect();
        if news.is_empty() {
            return Ok(());
        }
        // One copy: a plasmid that is already here is linked to the one there, not added again.
        let mut candidates = Vec::new();
        for listed in self.unbound(local) {
            candidates.push(self.project.read(&listed.id)?);
        }
        let server = self.server();
        for remote in news {
            if self.stopped {
                break;
            }
            let doc = match self.gateway.read_document(&remote.id) {
                Ok(doc) => doc,
                Err(e) => {
                    self.failed(None, &remote.name, &format!("brought in from {server}"), e);
                    continue;
                }
            };
            let construct = match readable(&doc) {
                Ok(construct) => construct,
                Err(why) => {
                    self.skip(None, &remote.name, format!("{} was not brought in from {server}: {why}.", remote.name));
                    continue;
                }
            };
            match candidates.iter().position(|l| same_molecule(&l.construct, &construct)) {
                Some(i) => {
                    let same = candidates.swap_remove(i);
                    self.link(&same, remote, &doc)?;
                }
                None => self.add_here(remote, &doc, construct)?,
            }
        }
        Ok(())
    }

    fn bind(&mut self, loaded: &Loaded, remote: &RemoteConstruct, doc: &RemoteDocument) -> Result<Here, SyncError> {
        let mut here = Here { id: loaded.id.clone(), name: String::new(), sha: String::new(), folder: String::new() };
        here.take(loaded);
        let entry = Bound {
            remote: remote.id.clone(),
            etag: doc.etag.clone(),
            sha: here.sha.clone(),
            name: here.name.clone(),
            folder: here.folder.clone(),
            remote_name: Some(remote.name.clone()),
            remote_folder: Some(remote.folder.clone()),
            pending: None,
            relist: false,
        };
        self.keep(&here.id, entry)?;
        Ok(here)
    }

    fn link(&mut self, same: &Loaded, remote: &RemoteConstruct, doc: &RemoteDocument) -> Result<(), SyncError> {
        let here = self.bind(same, remote, doc)?;
        let what = format!("{} was already here and on {}; the two are now kept in step.", here.name, self.server());
        self.report.matched.push(Self::item(&here, what));
        Ok(())
    }

    fn add_here(&mut self, remote: &RemoteConstruct, doc: &RemoteDocument, mut construct: Construct) -> Result<(), SyncError> {
        let server = self.server();
        if !remote.name.trim().is_empty() {
            construct.name = remote.name.clone();
        }
        let change = Change { command: "sync.add".into(), params: self.origin(doc), summary: self.brought_summary(doc) };
        match self.project.create_construct(construct, &remote.folder, None, &self.from_server, change) {
            Ok(loaded) => {
                let here = self.bind(&loaded, remote, doc)?;
                let what = format!("{} is new on {server} and was added here, in {}.", here.name, in_folder(&here.folder));
                self.report.added_here.push(Self::item(&here, what));
                Ok(())
            }
            Err(e @ (ProjectError::NameTaken(_) | ProjectError::BadFolder(_) | ProjectError::Unwritable(_))) => {
                self.skip(None, &remote.name, format!("{} is new on {server} and was not added here: {e}.", remote.name));
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn new_here(&mut self, local: &[Listed]) -> Result<(), SyncError> {
        let server = self.server();
        for listed in self.unbound(local) {
            if self.stopped {
                break;
            }
            let here = Here::of(listed);
            if !self.can_write {
                let why = format!("{} is only here: you can view this project on {server} but not add to it.", here.name);
                self.skip(Some(&here.id), &here.name, why);
                continue;
            }
            let loaded = self.project.read(&here.id)?;
            let snapshot = match Snapshot::of(&loaded.construct) {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    self.skip(Some(&here.id), &here.name, format!("{} was not added to {server}: {e}.", here.name));
                    continue;
                }
            };
            match self.gateway.create_document(&self.binding.project, &here.folder, &snapshot, &uuid::Uuid::new_v4().to_string()) {
                Ok(created) => {
                    let entry = Bound {
                        remote: created.document.construct_id.clone(),
                        etag: created.document.etag.clone(),
                        sha: loaded.sha.clone(),
                        name: here.name.clone(),
                        folder: here.folder.clone(),
                        remote_name: None,
                        remote_folder: Some(folder_of(&created.path)),
                        pending: None,
                        relist: false,
                    };
                    self.paths.insert(created.document.construct_id, created.path);
                    self.keep(&here.id, entry)?;
                    self.report.added_there.push(Self::item(&here, format!("{} is new here and was added to {server}.", here.name)));
                }
                Err(e) => self.failed(Some(&here.id), &here.name, &format!("added to {server}"), e),
            }
        }
        Ok(())
    }

    // ── after the plasmids ─────────────────────────────────────────────────────────────────

    /// The server lists a project from its own copy of each construct, which a save does not
    /// touch. So after saving, that copy is refreshed, as the web app does, and the names the
    /// listing then shows are taken as the names the two sides agree on.
    fn finish(&mut self) -> Result<(), SyncError> {
        self.refresh_listing()?;
        if self.stopped || !self.binding.constructs.values().any(|b| b.remote_name.is_none()) {
            return Ok(());
        }
        // Not having this is harmless: the next sync takes whatever the listing says then.
        let Ok(listing) = self.gateway.list_constructs(&self.binding.project) else {
            return Ok(());
        };
        for entry in self.binding.constructs.values_mut().filter(|b| b.remote_name.is_none()) {
            if let Some(remote) = listing.constructs.iter().find(|c| c.id == entry.remote) {
                entry.remote_name = Some(remote.name.clone());
                entry.remote_folder = Some(remote.folder.clone());
            }
        }
        store(self.project, &self.binding)
    }

    fn refresh_listing(&mut self) -> Result<(), SyncError> {
        if self.stopped || !self.can_write {
            return Ok(());
        }
        let mut batch = Vec::new();
        let mut ids = Vec::new();
        for (id, entry) in self.binding.constructs.iter().filter(|(_, b)| b.relist) {
            let (Some(construct), Some(path)) = (ConstructId::parse(id), self.paths.get(&entry.remote)) else {
                continue;
            };
            // One that changed again since it was saved there is sent again by a later sync,
            // and refreshed then.
            let Ok(loaded) = self.project.read(&construct) else {
                continue;
            };
            if loaded.sha != entry.sha {
                continue;
            }
            if let Ok(snapshot) = Snapshot::of(&loaded.construct) {
                batch.push((path.clone(), snapshot));
                ids.push(id.clone());
            }
        }
        if batch.is_empty() {
            return Ok(());
        }
        match self.gateway.sync_project(&self.binding.project, &batch) {
            Ok(()) => {
                for id in ids {
                    if let Some(entry) = self.binding.constructs.get_mut(&id) {
                        entry.relist = false;
                    }
                }
                store(self.project, &self.binding)
            }
            Err(e) => {
                let server = self.server();
                if matches!(e, GatewayError::Unreachable(_) | GatewayError::NotSignedIn) {
                    self.stopped = true;
                }
                let why = format!(
                    "What was sent is saved on {server}, but its list of the project was not refreshed and may show old names until the next sync: {e}."
                );
                self.skip(None, "", why);
                Ok(())
            }
        }
    }

    /// A folder made on the server is made here too, so an empty one is not lost on the way.
    fn folders(&mut self, listing: &ProjectListing) {
        let Ok(have) = self.project.folders() else {
            return;
        };
        let have: BTreeSet<String> = have.iter().map(|p| slashed(p)).collect();
        for folder in listing.folders.iter().filter(|f| !have.contains(*f)) {
            // A name this side cannot hold is not worth failing a sync over.
            let _ = self.project.create_folder(folder);
        }
    }
}
