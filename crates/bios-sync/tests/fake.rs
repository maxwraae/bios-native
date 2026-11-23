//! The fake server keeps the real one's rules. Tests here and in the apps lean on that, so the
//! rules are pinned on their own.

mod support;

use bios_sync::{FakeGateway, Gateway, GatewayError, Role, SaveOutcome, Snapshot};
use sha2::{Digest, Sha256};
use support::plasmid;

fn snapshot(name: &str, sequence: &str) -> Snapshot {
    Snapshot::of(&plasmid(name, sequence)).unwrap()
}

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn versions_count_from_one_and_the_etag_is_the_version_and_the_sha_of_the_text() {
    let server = FakeGateway::new("ana");
    let project = server.create_project("P").unwrap();
    let first = snapshot("pGFP", "ACGTACGT");

    let made = server.create_document(&project.id, "", &first, "r1").unwrap();

    assert_eq!(made.document.version, 1);
    assert_eq!(made.document.etag, format!("1:{}", sha(&first.text)));
    assert_eq!(made.path, format!("{}.gb", made.document.construct_id));
    assert_eq!(server.read_document(&made.document.construct_id).unwrap(), made.document);

    let second = snapshot("pGFP", "ACGTACGTTT");
    let saved = server.save_document(&made.document.construct_id, &made.document.etag, &second, "r2").unwrap();
    let SaveOutcome::Saved(saved) = saved else { panic!("{saved:?}") };
    assert_eq!((saved.version, saved.etag.as_str()), (2, format!("2:{}", sha(&second.text)).as_str()));
    assert_eq!(saved.text, second.text);
}

#[test]
fn a_stale_save_is_refused_with_the_current_copy_and_a_repeated_one_is_answered_as_done() {
    let server = FakeGateway::new("ana");
    let project = server.create_project("P").unwrap();
    let made = server.create_document(&project.id, "", &snapshot("pGFP", "ACGTACGT"), "r1").unwrap().document;
    let id = made.construct_id.clone();
    let second = snapshot("pGFP", "ACGTACGTTT");
    assert!(matches!(server.save_document(&id, &made.etag, &second, "r2").unwrap(), SaveOutcome::Saved(_)));
    let current = server.read_document(&id).unwrap();

    // Built on version 1, which is no longer there.
    let stale = server.save_document(&id, &made.etag, &snapshot("pGFP", "ACGTACGTCC"), "r3").unwrap();
    assert_eq!(stale, SaveOutcome::Conflict(Some(current.clone())));
    // No etag at all names no version, and is refused the same way.
    assert!(matches!(server.save_document(&id, "", &second, "r4").unwrap(), SaveOutcome::Conflict(Some(_))));
    assert_eq!(server.read_document(&id).unwrap(), current);

    // The same request again, with the etag it had then: a success, and no third version.
    let again = server.save_document(&id, &made.etag, &second, "r2").unwrap();
    let SaveOutcome::Saved(again) = again else { panic!("{again:?}") };
    assert_eq!(again.version, 2);
    assert_eq!(server.read_document(&id).unwrap().version, 2);
}

#[test]
fn an_empty_or_non_dna_construct_is_refused_and_an_unknown_id_is_not_found() {
    let server = FakeGateway::new("ana");
    let project = server.create_project("P").unwrap();
    let made = server.create_document(&project.id, "", &snapshot("pGFP", "ACGTACGT"), "r1").unwrap().document;
    let mut empty = snapshot("pGFP", "ACGTACGT");
    empty.sequence.clear();
    let mut protein = snapshot("pGFP", "ACGTACGT");
    protein.sequence = "MKLQE".into();

    for bad in [&empty, &protein] {
        assert!(matches!(server.save_document(&made.construct_id, &made.etag, bad, "r2"), Err(GatewayError::Refused { status: 422, .. })));
        assert!(matches!(server.create_document(&project.id, "", bad, "r3"), Err(GatewayError::Refused { status: 422, .. })));
    }
    assert_eq!(server.read_document(&made.construct_id).unwrap(), made);
    assert_eq!(server.list_constructs(&project.id).unwrap().constructs.len(), 1);

    assert!(matches!(server.read_document("no-such-id"), Err(GatewayError::NotFound(_))));
    assert!(matches!(server.save_document("no-such-id", "1:x", &snapshot("p", "ACGT"), "r"), Err(GatewayError::NotFound(_))));
    assert!(matches!(server.list_constructs("no-such-project"), Err(GatewayError::NotFound(_))));
}

#[test]
fn a_viewer_reads_everything_and_changes_nothing_and_a_stranger_sees_nothing() {
    let ana = FakeGateway::new("ana");
    let project = ana.create_project("P").unwrap();
    let made = ana.create_document(&project.id, "backbones", &snapshot("pGFP", "ACGTACGT"), "r1").unwrap().document;
    let id = made.construct_id.clone();
    ana.share(&project.id, "vera", Role::Viewer);
    ana.share(&project.id, "ed", Role::Editor);
    let (vera, ed, stranger) = (ana.as_account("vera"), ana.as_account("ed"), ana.as_account("stranger"));
    let change = snapshot("pGFP", "ACGTACGTTT");

    // A viewer: listed as one, reads, and a save comes back as a refusal carrying the current copy.
    assert_eq!(vera.list_projects().unwrap()[0].role, Role::Viewer);
    assert_eq!(vera.list_constructs(&project.id).unwrap().constructs[0].folder, "backbones");
    assert_eq!(vera.read_document(&id).unwrap(), made);
    assert_eq!(vera.save_document(&id, &made.etag, &change, "v1").unwrap(), SaveOutcome::Conflict(Some(made.clone())));
    // What changes a project answers a viewer "not found", as the real routes do.
    assert!(matches!(vera.create_document(&project.id, "", &change, "v2"), Err(GatewayError::NotFound(_))));
    assert!(matches!(vera.move_construct(&project.id, &id, ""), Err(GatewayError::NotFound(_))));
    assert!(matches!(vera.rename_construct(&project.id, &id, "x"), Err(GatewayError::NotFound(_))));
    assert!(matches!(vera.sync_project(&project.id, &[]), Err(GatewayError::NotFound(_))));
    assert_eq!(ana.read_document(&id).unwrap(), made);

    // An editor may do all of it; the owner is still the owner.
    assert_eq!(ed.list_projects().unwrap()[0].role, Role::Editor);
    assert!(matches!(ed.save_document(&id, &made.etag, &change, "e1").unwrap(), SaveOutcome::Saved(_)));
    assert_eq!(ed.move_construct(&project.id, &id, "").unwrap(), format!("{id}.gb"));
    assert_eq!(ana.list_projects().unwrap()[0].role, Role::Owner);

    // Someone who was never let in: no project, no document.
    assert!(stranger.list_projects().unwrap().is_empty());
    assert!(matches!(stranger.read_document(&id), Err(GatewayError::NotFound(_))));
    assert!(matches!(stranger.list_constructs(&project.id), Err(GatewayError::NotFound(_))));
}

#[test]
fn folders_names_and_what_is_removed() {
    let server = FakeGateway::new("ana");
    let project = server.create_project("P").unwrap();
    let made = server.create_document(&project.id, "parts/promoters", &snapshot("my promoter", "ACGTACGT"), "r1").unwrap();
    let id = made.document.construct_id.clone();
    server.make_folder(&project.id, "empty");

    let listing = server.list_constructs(&project.id).unwrap();
    assert_eq!(listing.folders, vec!["empty", "parts", "parts/promoters"]);
    // The listing's name is one word; the document's own record keeps the name as given.
    assert_eq!(listing.constructs[0].name, "my_promoter");
    assert_eq!(made.document.name.as_deref(), Some("my promoter"));

    assert_eq!(server.move_construct(&project.id, &id, "parts").unwrap(), format!("parts/{id}.gb"));
    assert!(matches!(server.move_construct(&project.id, &id, ".hidden"), Err(GatewayError::Refused { status: 400, .. })));
    // A rename reaches the listing and the document's text, which makes a version.
    assert_eq!(server.rename_construct(&project.id, &id, "strong promoter").unwrap(), "strong_promoter");
    let renamed = server.read_document(&id).unwrap();
    assert_eq!(renamed.version, 2);
    assert!(renamed.text.starts_with("LOCUS       strong_promoter "), "{}", renamed.text);
    assert_eq!(bios_core::genbank::read(&renamed.text).unwrap().sequence, "ACGTACGT");

    // Deleted from the project: not listed, still readable. Destroyed: not found.
    server.remove_from_project(&project.id, &id);
    assert!(server.list_constructs(&project.id).unwrap().constructs.is_empty());
    assert!(server.read_document(&id).is_ok());
    server.destroy(&id);
    assert!(matches!(server.read_document(&id), Err(GatewayError::NotFound(_))));
}

#[test]
fn while_offline_nothing_is_reached_and_every_call_is_on_record() {
    let server = FakeGateway::new("ana");
    let project = server.create_project("P").unwrap();
    server.set_offline(true);
    assert!(matches!(server.list_projects(), Err(GatewayError::Unreachable(_))));
    assert!(matches!(server.create_document(&project.id, "", &snapshot("p", "ACGT"), "r"), Err(GatewayError::Unreachable(_))));
    server.set_offline(false);
    assert!(server.list_constructs(&project.id).unwrap().constructs.is_empty(), "nothing was created while offline");
    assert_eq!(server.calls()[0], "create_project P");
    assert_eq!(server.calls()[1], "list_projects");
}
