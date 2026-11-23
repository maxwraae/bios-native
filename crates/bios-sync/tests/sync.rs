//! The rules of sync, with real project folders and the fake server: two people taking turns.

mod support;

use std::path::Path;

use bios_project::Actor;
use bios_sync::{FakeGateway, Gateway, Role, SyncError};
use support::{Person, SERVER, calls_since, plasmid, remote_id, shared};

const BASES: &str = "ACGTACGTAGACGTACGTAG";

#[test]
fn publishing_sends_everything_and_a_second_person_brings_it_all_in() {
    let server = FakeGateway::new("ana");
    let ana = Person::new("ana", server.clone());
    ana.add(plasmid("pGFP", BASES), "");
    ana.add(plasmid("pBackbone", "GGGGCCCCAAAATTTT"), "backbones/2026");

    let report = ana.publish("Shared work");

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.added_there.len(), 2);
    assert_eq!(report.server, SERVER);
    let projects = server.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!((projects[0].title.as_str(), projects[0].role), ("Shared work", Role::Owner));
    let listing = server.list_constructs(&projects[0].id).unwrap();
    let mut placed: Vec<(String, String)> = listing.constructs.iter().map(|c| (c.name.clone(), c.folder.clone())).collect();
    placed.sort();
    assert_eq!(placed, vec![("pBackbone".to_string(), "backbones/2026".to_string()), ("pGFP".to_string(), String::new())]);
    // The server holds the text exactly as this side writes it.
    let sent = server.read_document(&remote_id(&ana, "pGFP")).unwrap();
    assert_eq!(sent.version, 1);
    assert_eq!(bios_core::genbank::read(&sent.text).unwrap().sequence, BASES);

    server.share(&projects[0].id, "ben", Role::Editor);
    let ben = Person::new("ben", server.as_account("ben"));
    let report = ben.connect(&projects[0].id);

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.added_here.len(), 2);
    assert_eq!(ben.names(), vec!["pBackbone", "pGFP"]);
    let backbone = ben.named("pBackbone");
    assert_eq!(backbone.path, Path::new("backbones/2026/pBackbone.gb"));
    assert_eq!(backbone.construct.sequence, "GGGGCCCCAAAATTTT");
    assert_eq!(backbone.construct.features, ana.named("pBackbone").construct.features);
    // Where it came from is in its history.
    let history = ben.project.history(&backbone.id).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].actor, Actor::Import);
    assert_eq!(history[0].source, SERVER);
    assert!(history[0].summary.starts_with("Brought in from lab server, saved there 20"), "{}", history[0].summary);
    assert_eq!(history[0].session.as_deref(), Some("ben"));

    // And with nothing changed on either side, a sync asks and does nothing.
    let before = server.calls().len();
    for person in [&ana, &ben] {
        let report = person.sync();
        assert!(report.is_clean() && report.is_empty(), "{report}");
        assert_eq!(report.to_string(), "Everything is in step with lab server.");
    }
    for write in ["save_document", "create_document", "sync_project", "move_construct", "rename_construct"] {
        assert!(calls_since(&server, before, write).is_empty(), "{write}");
    }
    assert_eq!(ben.project.history(&backbone.id).unwrap().len(), 1);
}

#[test]
fn a_project_that_was_never_shared_has_nothing_to_sync() {
    let server = FakeGateway::new("ana");
    let ana = Person::new("ana", server.clone());
    assert_eq!(bios_sync::connection(&ana.project).unwrap(), None);
    assert_eq!(bios_sync::sync(&ana.project, &server, &ana.who), Err(SyncError::NotConnected));

    ana.publish("Once");
    assert!(matches!(bios_sync::publish(&ana.project, &server, SERVER, "Twice", &ana.who), Err(SyncError::AlreadyConnected { .. })));
    assert!(matches!(bios_sync::connect(&ana.project, &server, SERVER, "another-project", &ana.who), Err(SyncError::AlreadyConnected { .. })));
    assert_eq!(server.list_projects().unwrap().len(), 1);
}

#[test]
fn a_change_made_there_comes_in_as_a_new_version_and_one_made_here_is_sent() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let id = remote_id(&ana, "pGFP");
    let etag_before = ana.gateway.read_document(&id).unwrap().etag;

    // Ana edits and syncs: her version is saved there, on top of the version she had.
    let mine = ana.named("pGFP");
    ana.edit(&mine.id, "Added four bases", |c| c.sequence.push_str("TTTT"));
    let before = ana.gateway.calls().len();
    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.sent.len(), 1);
    assert_eq!(report.sent[0].what, "pGFP was sent to lab server.");
    let saves = calls_since(&ana.gateway, before, "save_document");
    assert_eq!(saves.len(), 1);
    assert!(saves[0].contains(&format!("if-match {etag_before} ")), "{saves:?}");
    // After the save, the project's own copy is refreshed, as the web app does.
    let path = ana.gateway.list_constructs(&ana.remote_project()).unwrap().constructs[0].path.clone();
    assert_eq!(calls_since(&ana.gateway, before, "sync_project"), vec![format!("sync_project {} {path}", ana.remote_project())]);
    let there = ana.gateway.read_document(&id).unwrap();
    assert_eq!(there.version, 2);
    assert!(there.sequence.ends_with("TTTT"));

    // Ben syncs: it arrives as a new version, and the one he had is still in his history.
    let his = ben.named("pGFP");
    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.brought_in.len(), 1);
    assert!(report.brought_in[0].what.starts_with("pGFP was brought in from lab server, saved there 20"), "{}", report.brought_in[0].what);
    let now = ben.named("pGFP");
    assert_eq!(now.id, his.id);
    assert_eq!(now.version, 2);
    assert_eq!(now.construct.sequence, format!("{BASES}TTTT"));
    let history = ben.project.history(&his.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].actor, Actor::Import);
    assert_eq!(history[1].source, SERVER);
    assert_eq!(history[1].command, "sync.bring_in");
    assert!(history[1].summary.starts_with("Brought in from lab server, saved there 20"), "{}", history[1].summary);
    assert_eq!(history[1].params["version"], 2);
    assert_eq!(ben.project.at_version(&his.id, 1).unwrap().sequence, BASES);

    // Nothing is sent back: bringing a version in is not a change made here.
    let before = ben.gateway.calls().len();
    assert!(ben.sync().is_empty());
    assert!(ana.sync().is_empty());
    assert!(calls_since(&ben.gateway, before, "save_document").is_empty());
    assert_eq!(ana.gateway.read_document(&id).unwrap().version, 2);
}
