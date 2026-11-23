//! A Bios server in memory, with the rules sync depends on and no network.
//!
//! It keeps what the real one keeps: a document has integer versions from 1 and an etag of
//! `version:sha256(text)`; a save against any other etag is refused with the current copy; a
//! save whose `request_id` was already applied answers as a success; a construct with no bases
//! is refused; an unknown id is not found; a viewer's save is refused exactly as a stale one is.
//! The name a project lists is kept as one word, as the real listing's is.
//!
//! One `FakeGateway` is one account on the server. [`FakeGateway::as_account`] gives another
//! person on the same server, which is how a test has someone else save a document.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bios_core::genbank;
use sha2::{Digest, Sha256};

use crate::gateway::{Created, Gateway, GatewayError, ProjectListing, RemoteConstruct, RemoteDocument, RemoteProject, Role, SaveOutcome, folder_of};
use crate::snapshot::Snapshot;

#[derive(Default)]
struct State {
    projects: Vec<FakeProject>,
    documents: BTreeMap<String, FakeDocument>,
    offline: bool,
    lose_next_answer: bool,
    calls: Vec<String>,
}

struct FakeProject {
    id: String,
    title: String,
    owner: String,
    members: BTreeMap<String, Role>,
    held: Vec<Held>,
    folders: BTreeSet<String>,
}

struct Held {
    id: String,
    path: String,
    /// The name in the project's own copy, which is what the listing shows.
    name: String,
    /// Deleted from the project: no longer listed, though its document still answers.
    binned: bool,
}

struct FakeDocument {
    owner: String,
    version: u64,
    text: String,
    sequence: String,
    circular: bool,
    name: String,
    updated_at: String,
    requests: BTreeSet<String>,
}

#[derive(Clone)]
pub struct FakeGateway {
    state: Arc<Mutex<State>>,
    account: String,
}

impl FakeGateway {
    /// A new, empty server, seen as `account`.
    pub fn new(account: &str) -> FakeGateway {
        FakeGateway { state: Arc::new(Mutex::new(State::default())), account: account.to_string() }
    }

    /// The same server, seen as another person.
    pub fn as_account(&self, account: &str) -> FakeGateway {
        FakeGateway { state: Arc::clone(&self.state), account: account.to_string() }
    }

    /// Lets `account` into a project with `role`, as claiming a share link does.
    pub fn share(&self, project: &str, account: &str, role: Role) {
        if let Some(p) = self.lock().projects.iter_mut().find(|p| p.id == project) {
            p.members.insert(account.to_string(), role);
        }
    }

    /// This account saves `text` as the next version of `construct`, on top of whatever is there
    /// now: what a person editing in the web app does.
    pub fn save_text(&self, construct: &str, text: &str) -> Result<SaveOutcome, GatewayError> {
        let parsed = genbank::read(text).map_err(|e| GatewayError::Refused { status: 422, reason: e.to_string() })?;
        let etag = self.read_document(construct)?.etag;
        let snapshot =
            Snapshot { name: String::new(), text: text.to_string(), sequence: parsed.sequence, circular: parsed.circular, features: Vec::new() };
        self.save_document(construct, &etag, &snapshot, &uuid::Uuid::new_v4().to_string())
    }

    /// Moves a document's bases on without its text, as a save of edit steps alone does on the
    /// real server. The text then describes an older version than the sequence.
    pub fn save_bases_without_text(&self, construct: &str, sequence: &str) {
        if let Some(d) = self.lock().documents.get_mut(construct) {
            d.version += 1;
            d.sequence = sequence.to_string();
            d.updated_at = now();
        }
    }

    /// Deletes a construct from a project the way the web app does: it is no longer listed.
    pub fn remove_from_project(&self, project: &str, construct: &str) {
        if let Some(p) = self.lock().projects.iter_mut().find(|p| p.id == project) {
            for held in p.held.iter_mut().filter(|h| h.id == construct) {
                held.binned = true;
            }
        }
    }

    /// Removes a construct altogether: every later request for it is not found.
    pub fn destroy(&self, construct: &str) {
        let mut state = self.lock();
        state.documents.remove(construct);
        for p in &mut state.projects {
            p.held.retain(|h| h.id != construct);
        }
    }

    /// Makes an empty folder in a project.
    pub fn make_folder(&self, project: &str, folder: &str) {
        if let Some(p) = self.lock().projects.iter_mut().find(|p| p.id == project) {
            remember_folder(&mut p.folders, folder);
        }
    }

    /// While offline, every call fails as a server that cannot be reached does.
    pub fn set_offline(&self, offline: bool) {
        self.lock().offline = offline;
    }

    /// The next save is carried out, and its answer never arrives.
    pub fn lose_next_answer(&self) {
        self.lock().lose_next_answer = true;
    }

    /// Every call made to this server by anyone, oldest first, such as `save_document <id>`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A test that panicked while holding the lock has already failed; the state is still usable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn enter(&self, call: String) -> Result<MutexGuard<'_, State>, GatewayError> {
        let mut state = self.lock();
        state.calls.push(call);
        if state.offline { Err(GatewayError::Unreachable("the fake server is offline".into())) } else { Ok(state) }
    }
}

fn role_in(project: &FakeProject, account: &str) -> Option<Role> {
    if project.owner == account { Some(Role::Owner) } else { project.members.get(account).copied() }
}

fn rank(role: Role) -> u8 {
    match role {
        Role::Viewer => 0,
        Role::Editor => 1,
        Role::Owner => 2,
    }
}

impl State {
    /// The most this account may do with a construct, through owning it or through any project
    /// that holds it.
    fn access(&self, construct: &str, account: &str) -> Option<Role> {
        let document = self.documents.get(construct)?;
        if document.owner == account {
            return Some(Role::Owner);
        }
        self.projects.iter().filter(|p| p.held.iter().any(|h| h.id == construct)).filter_map(|p| role_in(p, account)).max_by_key(|r| rank(*r))
    }

    /// The project, if this account has at least `least` there. The real server answers "not
    /// found" both for a project that does not exist and for one this account may not do this in.
    fn project(&mut self, id: &str, account: &str, least: Role) -> Result<&mut FakeProject, GatewayError> {
        self.projects
            .iter_mut()
            .find(|p| p.id == id && role_in(p, account).is_some_and(|r| rank(r) >= rank(least)))
            .ok_or_else(|| GatewayError::NotFound("this project".into()))
    }

    fn document(&self, id: &str) -> Option<RemoteDocument> {
        self.documents.get(id).map(|d| RemoteDocument {
            construct_id: id.to_string(),
            version: d.version,
            etag: etag(d.version, &d.text),
            text: d.text.clone(),
            sequence: d.sequence.clone(),
            circular: d.circular,
            name: Some(d.name.clone()),
            updated_at: Some(d.updated_at.clone()),
        })
    }
}

fn etag(version: u64, text: &str) -> String {
    let sha: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    format!("{version}:{sha}")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A listing's name is one word: whitespace becomes underscores.
fn one_word(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join("_")
}

fn refused(status: u16, reason: &str) -> GatewayError {
    GatewayError::Refused { status, reason: reason.to_string() }
}

/// The refusals of the real server's document writes: emptiness is evidence of a failed read,
/// never an edit worth saving.
fn check(snapshot: &Snapshot) -> Result<(), GatewayError> {
    if snapshot.sequence.is_empty() {
        return Err(refused(422, "a document with no bases is a failed read, not an edit"));
    }
    if !snapshot.sequence.chars().all(|c| "ACGTRYKMSWBDHVN".contains(c.to_ascii_uppercase())) {
        return Err(refused(422, "sequence is not DNA"));
    }
    if snapshot.text.is_empty() {
        return Err(refused(422, "text is required"));
    }
    Ok(())
}

fn check_folder(folder: &str) -> Result<(), GatewayError> {
    let bad = |part: &str| {
        part.is_empty() || part.starts_with('.') || part.ends_with('.') || part.chars().any(|c| c.is_control() || "<>:\"|?*\\".contains(c))
    };
    if folder.split('/').any(bad) { Err(refused(400, &format!("{folder} is not a folder name the server accepts"))) } else { Ok(()) }
}

/// A folder exists together with every folder above it.
fn remember_folder(folders: &mut BTreeSet<String>, folder: &str) {
    let mut path = String::new();
    for part in folder.split('/').filter(|p| !p.is_empty()) {
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
        folders.insert(path.clone());
    }
}

/// The text with the name on its `LOCUS` line replaced, or `None` when it has no such line.
fn rename_text(text: &str, name: &str) -> Option<String> {
    let (first, rest) = text.split_once('\n')?;
    let after = first.strip_prefix("LOCUS")?;
    let fields = after.trim_start();
    if fields.len() == after.len() {
        return None;
    }
    let tail = fields.split_once(char::is_whitespace).map(|(_, tail)| tail.trim_start()).unwrap_or_default();
    Some(format!("LOCUS       {name} {tail}\n{rest}"))
}

impl Gateway for FakeGateway {
    fn list_projects(&self) -> Result<Vec<RemoteProject>, GatewayError> {
        let state = self.enter("list_projects".into())?;
        Ok(state
            .projects
            .iter()
            .filter_map(|p| role_in(p, &self.account).map(|role| RemoteProject { id: p.id.clone(), title: p.title.clone(), role }))
            .collect())
    }

    fn create_project(&self, title: &str) -> Result<RemoteProject, GatewayError> {
        let mut state = self.enter(format!("create_project {title}"))?;
        let id = uuid::Uuid::new_v4().to_string();
        state.projects.push(FakeProject {
            id: id.clone(),
            title: title.to_string(),
            owner: self.account.clone(),
            members: BTreeMap::new(),
            held: Vec::new(),
            folders: BTreeSet::new(),
        });
        Ok(RemoteProject { id, title: title.to_string(), role: Role::Owner })
    }

    fn list_constructs(&self, project: &str) -> Result<ProjectListing, GatewayError> {
        let mut state = self.enter(format!("list_constructs {project}"))?;
        let p = state.project(project, &self.account, Role::Viewer)?;
        let constructs = p
            .held
            .iter()
            .filter(|h| !h.binned)
            .map(|h| RemoteConstruct { id: h.id.clone(), name: h.name.clone(), path: h.path.clone(), folder: folder_of(&h.path) })
            .collect();
        Ok(ProjectListing { constructs, folders: p.folders.iter().cloned().collect() })
    }

    fn read_document(&self, construct: &str) -> Result<RemoteDocument, GatewayError> {
        let state = self.enter(format!("read_document {construct}"))?;
        // Not yours and not there are one answer, as on the real server.
        state.access(construct, &self.account).and_then(|_| state.document(construct)).ok_or_else(|| GatewayError::NotFound("this plasmid".into()))
    }

    fn create_document(&self, project: &str, folder: &str, snapshot: &Snapshot, request_id: &str) -> Result<Created, GatewayError> {
        let mut state = self.enter(format!("create_document {project} {folder:?} {}", snapshot.name))?;
        state.project(project, &self.account, Role::Editor)?;
        check(snapshot)?;
        if !folder.is_empty() {
            check_folder(folder)?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let name = if snapshot.name.trim().is_empty() { "Untitled".to_string() } else { snapshot.name.trim().to_string() };
        state.documents.insert(
            id.clone(),
            FakeDocument {
                owner: self.account.clone(),
                version: 1,
                text: snapshot.text.clone(),
                sequence: snapshot.sequence.clone(),
                circular: snapshot.circular,
                name: name.clone(),
                updated_at: now(),
                requests: BTreeSet::from([request_id.to_string()]),
            },
        );
        let path = if folder.is_empty() { format!("{id}.gb") } else { format!("{folder}/{id}.gb") };
        let p = state.project(project, &self.account, Role::Editor)?;
        p.held.push(Held { id: id.clone(), path: path.clone(), name: one_word(&name), binned: false });
        remember_folder(&mut p.folders, folder);
        let document = state.document(&id).ok_or_else(|| GatewayError::Unexpected("the document was not kept".into()))?;
        Ok(Created { document, path })
    }

    fn save_document(&self, construct: &str, presented: &str, snapshot: &Snapshot, request_id: &str) -> Result<SaveOutcome, GatewayError> {
        let mut state = self.enter(format!("save_document {construct} if-match {presented} request {request_id}"))?;
        let role = state.access(construct, &self.account).ok_or_else(|| GatewayError::NotFound("this plasmid".into()))?;
        check(snapshot)?;
        let current = state.document(construct).ok_or_else(|| GatewayError::NotFound("this plasmid".into()))?;
        // A viewer can read it and must not write it; the real server answers as it does to a
        // stale save, with the current copy.
        if !role.can_write() {
            return Ok(SaveOutcome::Conflict(Some(current)));
        }
        let lost = std::mem::take(&mut state.lose_next_answer);
        let answer =
            |outcome: SaveOutcome| if lost { Err(GatewayError::Unreachable("the answer was lost on the way back".into())) } else { Ok(outcome) };
        let Some(document) = state.documents.get_mut(construct) else {
            return Err(GatewayError::NotFound("this plasmid".into()));
        };
        // A replay is not a conflict: the first sending already made its version.
        if document.requests.contains(request_id) {
            return answer(SaveOutcome::Saved(current));
        }
        if presented != current.etag {
            return answer(SaveOutcome::Conflict(Some(current)));
        }
        document.version += 1;
        document.text = snapshot.text.clone();
        document.sequence = snapshot.sequence.clone();
        document.circular = snapshot.circular;
        document.updated_at = now();
        document.requests.insert(request_id.to_string());
        if !snapshot.name.trim().is_empty() {
            document.name = snapshot.name.trim().to_string();
        }
        let mut saved = state.document(construct).ok_or_else(|| GatewayError::Unexpected("the document was not kept".into()))?;
        // The real answer to a save does not carry the name.
        saved.name = None;
        answer(SaveOutcome::Saved(saved))
    }

    fn move_construct(&self, project: &str, construct: &str, folder: &str) -> Result<String, GatewayError> {
        let mut state = self.enter(format!("move_construct {construct} {folder:?}"))?;
        let p = state.project(project, &self.account, Role::Editor)?;
        if !folder.is_empty() {
            check_folder(folder)?;
        }
        let held = p.held.iter_mut().find(|h| h.id == construct && !h.binned).ok_or_else(|| GatewayError::NotFound("this plasmid".into()))?;
        let file = held.path.rsplit('/').next().unwrap_or_default().to_string();
        held.path = if folder.is_empty() { file } else { format!("{folder}/{file}") };
        let path = held.path.clone();
        remember_folder(&mut p.folders, folder);
        Ok(path)
    }

    fn rename_construct(&self, project: &str, construct: &str, name: &str) -> Result<String, GatewayError> {
        let mut state = self.enter(format!("rename_construct {construct} {name}"))?;
        let name = one_word(name);
        let p = state.project(project, &self.account, Role::Editor)?;
        let held = p.held.iter_mut().find(|h| h.id == construct && !h.binned).ok_or_else(|| GatewayError::NotFound("this plasmid".into()))?;
        if name.is_empty() {
            return Err(refused(400, "name required"));
        }
        held.name = name.clone();
        // The real server also carries the name into the document's text, which makes a version.
        if let Some(document) = state.documents.get_mut(construct) {
            document.name = name.clone();
            if let Some(text) = rename_text(&document.text, &name) {
                document.text = text;
                document.version += 1;
                document.updated_at = now();
            }
        }
        Ok(name)
    }

    fn sync_project(&self, project: &str, constructs: &[(String, Snapshot)]) -> Result<(), GatewayError> {
        let paths: Vec<&str> = constructs.iter().map(|(path, _)| path.as_str()).collect();
        let mut state = self.enter(format!("sync_project {project} {}", paths.join(" ")))?;
        let p = state.project(project, &self.account, Role::Editor)?;
        for (path, snapshot) in constructs {
            if let Some(held) = p.held.iter_mut().find(|h| &h.path == path) {
                held.name = one_word(&snapshot.name);
            }
        }
        Ok(())
    }
}
