//! Two people, each with a project folder of their own, and one fake server between them.
#![allow(dead_code)]

use std::cell::RefCell;

use bios_core::model::Locus;
use bios_core::{Construct, Feature, Segment, Strand};
use bios_project::{Actor, Change, ConstructId, Loaded, Project, Who};
use bios_sync::{Created, FakeGateway, Gateway, GatewayError, ProjectListing, RemoteDocument, RemoteProject, Report, Role, SaveOutcome, Snapshot};

/// Someone saves a changed copy of a plasmid on the server, on top of what is there now.
pub fn save_there(gateway: &FakeGateway, construct: &str, change: impl FnOnce(&mut Construct)) -> RemoteDocument {
    let mut theirs = bios_core::genbank::read(&gateway.read_document(construct).unwrap().text).unwrap();
    change(&mut theirs);
    match gateway.save_text(construct, &bios_core::genbank_write::write(&theirs).unwrap()).unwrap() {
        SaveOutcome::Saved(document) => document,
        SaveOutcome::Conflict(_) => panic!("the save was refused"),
    }
}

/// The fake server with something in the way, for what only happens between two requests or
/// when a server answers wrongly.
pub struct Meddler<'a> {
    pub inner: FakeGateway,
    /// Runs once, just before the next save reaches the server.
    pub before_save: RefCell<Option<Box<dyn FnOnce() + 'a>>>,
    /// What the list of projects claims this account may do, whatever the server then allows.
    pub role: Option<Role>,
    /// Every document arrives with text that is not GenBank.
    pub garble: bool,
}

impl<'a> Meddler<'a> {
    pub fn new(inner: &FakeGateway) -> Meddler<'a> {
        Meddler { inner: inner.clone(), before_save: RefCell::new(None), role: None, garble: false }
    }
}

impl Gateway for Meddler<'_> {
    fn list_projects(&self) -> Result<Vec<RemoteProject>, GatewayError> {
        let mut projects = self.inner.list_projects()?;
        if let Some(role) = self.role {
            projects.iter_mut().for_each(|p| p.role = role);
        }
        Ok(projects)
    }

    fn create_project(&self, title: &str) -> Result<RemoteProject, GatewayError> {
        self.inner.create_project(title)
    }

    fn list_constructs(&self, project: &str) -> Result<ProjectListing, GatewayError> {
        self.inner.list_constructs(project)
    }

    fn read_document(&self, construct: &str) -> Result<RemoteDocument, GatewayError> {
        let mut document = self.inner.read_document(construct)?;
        if self.garble {
            document.text = "<html>502 Bad Gateway</html>".into();
        }
        Ok(document)
    }

    fn create_document(&self, project: &str, folder: &str, snapshot: &Snapshot, request_id: &str) -> Result<Created, GatewayError> {
        self.inner.create_document(project, folder, snapshot, request_id)
    }

    fn save_document(&self, construct: &str, etag: &str, snapshot: &Snapshot, request_id: &str) -> Result<SaveOutcome, GatewayError> {
        if let Some(meddle) = self.before_save.borrow_mut().take() {
            meddle();
        }
        self.inner.save_document(construct, etag, snapshot, request_id)
    }

    fn move_construct(&self, project: &str, construct: &str, folder: &str) -> Result<String, GatewayError> {
        self.inner.move_construct(project, construct, folder)
    }

    fn rename_construct(&self, project: &str, construct: &str, name: &str) -> Result<String, GatewayError> {
        self.inner.rename_construct(project, construct, name)
    }

    fn sync_project(&self, project: &str, constructs: &[(String, Snapshot)]) -> Result<(), GatewayError> {
        self.inner.sync_project(project, constructs)
    }
}

pub const SERVER: &str = "lab server";

pub fn who(session: &str) -> Who {
    Who { actor: Actor::Human, source: "test".into(), session: Some(session.into()) }
}

pub fn feature(name: &str, start: usize, end: usize) -> Feature {
    Feature {
        name: name.into(),
        kind: "CDS".into(),
        start,
        end,
        strand: Strand::Forward,
        segments: vec![Segment::new(start, end)],
        notes: vec![("gene".into(), vec![name.to_lowercase()])],
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

pub fn plasmid(name: &str, sequence: &str) -> Construct {
    Construct {
        name: name.into(),
        circular: true,
        sequence: sequence.into(),
        features: vec![feature("GFP", 2, 9)],
        comments: Vec::new(),
        locus: Locus { molecule: "DNA".into(), division: "SYN".into(), date: "18-NOV-2026".into() },
        header: Vec::new(),
    }
}

/// One person: a project folder, and the server as that person sees it.
pub struct Person {
    _dir: tempfile::TempDir,
    pub project: Project,
    pub gateway: FakeGateway,
    pub who: Who,
}

impl Person {
    pub fn new(name: &str, gateway: FakeGateway) -> Person {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(&dir.path().join(name), name).unwrap();
        Person { _dir: dir, project, gateway, who: who(name) }
    }

    pub fn add(&self, construct: Construct, folder: &str) -> Loaded {
        let change = Change { command: "test.new".into(), params: serde_json::Value::Null, summary: "Made for a test".into() };
        self.project.create_construct(construct, folder, None, &self.who, change).unwrap()
    }

    /// An edit made here, as the window or an agent would make it.
    pub fn edit(&self, id: &ConstructId, summary: &str, change: impl FnOnce(&mut Construct)) -> Loaded {
        let loaded = self.project.read(id).unwrap();
        let mut construct = loaded.construct.clone();
        change(&mut construct);
        let change = Change { command: "test.edit".into(), params: serde_json::Value::Null, summary: summary.into() };
        self.project.save(id, &loaded.sha, &construct, &self.who, change).unwrap()
    }

    pub fn sync(&self) -> Report {
        bios_sync::sync(&self.project, &self.gateway, &self.who).unwrap()
    }

    pub fn publish(&self, title: &str) -> Report {
        bios_sync::publish(&self.project, &self.gateway, SERVER, title, &self.who).unwrap()
    }

    pub fn connect(&self, remote_project: &str) -> Report {
        bios_sync::connect(&self.project, &self.gateway, SERVER, remote_project, &self.who).unwrap()
    }

    /// The id of the project this folder is shared as.
    pub fn remote_project(&self) -> String {
        bios_sync::connection(&self.project).unwrap().unwrap().project
    }

    /// The one plasmid here with this name.
    pub fn named(&self, name: &str) -> Loaded {
        let listed = self.project.list().unwrap();
        let found: Vec<_> = listed.iter().filter(|l| l.name == name).collect();
        assert_eq!(found.len(), 1, "{name} among {:?}", listed.iter().map(|l| &l.name).collect::<Vec<_>>());
        self.project.read(&found[0].id).unwrap()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.project.list().unwrap().into_iter().map(|l| l.name).collect();
        names.sort();
        names
    }
}

/// Ana owns a published project holding `constructs`; Ben has joined it with `role` and brought
/// everything in. Both are in step.
pub fn shared(constructs: Vec<(Construct, &str)>, role: Role) -> (Person, Person) {
    let server = FakeGateway::new("ana");
    let ana = Person::new("ana", server.clone());
    for (construct, folder) in constructs {
        ana.add(construct, folder);
    }
    assert!(ana.publish("Shared work").is_clean());
    let project = ana.remote_project();
    server.share(&project, "ben", role);
    let ben = Person::new("ben", server.as_account("ben"));
    assert!(ben.connect(&project).is_clean());
    (ana, ben)
}

/// The server's id for the plasmid the project lists under `name`.
pub fn remote_id(person: &Person, name: &str) -> String {
    let listing = person.gateway.list_constructs(&person.remote_project()).unwrap();
    let found: Vec<_> = listing.constructs.iter().filter(|c| c.name == name).collect();
    assert_eq!(found.len(), 1, "{name} among {:?}", listing.constructs);
    found[0].id.clone()
}

/// How many times the server was asked to do `what` since `from`, a length of `calls()` taken earlier.
pub fn calls_since(gateway: &FakeGateway, from: usize, what: &str) -> Vec<String> {
    gateway.calls().into_iter().skip(from).filter(|c| c.starts_with(what)).collect()
}
