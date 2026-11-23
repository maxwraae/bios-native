//! What is on one side only, what moved or was renamed, who may not send, and a server that
//! answers wrongly.

mod support;

use std::path::Path;

use bios_project::Actor;
use bios_sync::{FakeGateway, Gateway, Keep, Role, SyncError};
use support::{Meddler, Person, SERVER, calls_since, plasmid, remote_id, save_there, shared};

const BASES: &str = "ACGTACGTAGACGTACGTAG";

fn one() -> Vec<(bios_core::Construct, &'static str)> {
    vec![(plasmid("pGFP", BASES), "")]
}

#[test]
fn a_plasmid_added_on_either_side_appears_on_the_other_in_its_folder() {
    let (ana, ben) = shared(one(), Role::Editor);
    ben.add(plasmid("pNew", "TTTTGGGGCCCC"), "inserts/round 2");

    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.added_there.len(), 1);
    assert_eq!(report.added_there[0].what, "pNew is new here and was added to lab server.");
    let listing = ana.gateway.list_constructs(&ana.remote_project()).unwrap();
    let there = listing.constructs.iter().find(|c| c.name == "pNew").unwrap();
    assert_eq!(there.folder, "inserts/round 2");

    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.added_here.len(), 1);
    assert_eq!(report.added_here[0].what, "pNew is new on lab server and was added here, in inserts/round 2.");
    let new = ana.named("pNew");
    assert_eq!(new.path, Path::new("inserts/round 2/pNew.gb"));
    assert_eq!(new.construct.sequence, "TTTTGGGGCCCC");
    assert!(ana.sync().is_empty() && ben.sync().is_empty());
    assert_eq!(ana.gateway.list_constructs(&ana.remote_project()).unwrap().constructs.len(), 2);
}

#[test]
fn a_viewer_brings_changes_in_and_cannot_send() {
    let (ana, ben) = shared(one(), Role::Viewer);
    let id = remote_id(&ana, "pGFP");
    ana.edit(&ana.named("pGFP").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    assert!(ana.sync().is_clean());
    assert_eq!(ben.sync().brought_in.len(), 1);

    let b = ben.named("pGFP").id;
    ben.edit(&b, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    ben.add(plasmid("pMine", "ACGTTTTT"), "");
    let there = ben.gateway.read_document(&id).unwrap();
    let before = ben.gateway.calls().len();

    let report = ben.sync();

    assert!(!report.is_clean());
    let whys: Vec<&str> = report.skipped.iter().map(|s| s.why.as_str()).collect();
    assert_eq!(
        whys,
        vec![
            "pGFP was not sent: you can view this project on lab server but not change it.",
            "pMine is only here: you can view this project on lab server but not add to it.",
        ]
    );
    for write in ["save_document", "create_document", "sync_project"] {
        assert!(calls_since(&ben.gateway, before, write).is_empty(), "{write}");
    }
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there);
    assert_eq!(ben.gateway.list_constructs(&ben.remote_project()).unwrap().constructs.len(), 1);
    // What he made is still his, here.
    assert_eq!(ben.named("pGFP").construct.sequence, format!("{BASES}AAAACCCC"));
    assert_eq!(ben.names(), vec!["pGFP", "pMine"]);
    // Nor can he settle it by sending.
    let report = bios_sync::resolve(&ben.project, &ben.gateway, &b, Keep::Mine, &ben.who).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there);
}

#[test]
fn a_save_the_server_refuses_with_nothing_newer_there_is_said_to_be_about_permission() {
    let (ana, ben) = shared(one(), Role::Viewer);
    let id = remote_id(&ana, "pGFP");
    let b = ben.named("pGFP").id;
    ben.edit(&b, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    let there = ben.gateway.read_document(&id).unwrap();
    // The list of projects says he may edit; the save says otherwise, the way the real server
    // says it: as a refusal carrying the copy that is there.
    let mut meddler = Meddler::new(&ben.gateway);
    meddler.role = Some(Role::Editor);

    let report = bios_sync::sync(&ben.project, &meddler, &ben.who).unwrap();

    assert!(report.conflicts.is_empty(), "a refusal that is not about a newer version is not a conflict");
    assert_eq!(report.skipped.len(), 1, "{report}");
    assert!(report.skipped[0].why.contains("refused the change although nothing changed there"), "{report}");
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there);
    assert_eq!(ben.named("pGFP").construct.sequence, format!("{BASES}CCCC"));
}

#[test]
fn a_plasmid_gone_from_the_server_is_kept_here_and_no_longer_shared() {
    let (ana, ben) = shared(one(), Role::Editor);
    ana.gateway.destroy(&remote_id(&ana, "pGFP"));
    let kept = ben.named("pGFP");

    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.gone.len(), 1);
    assert_eq!(report.gone[0].what, "pGFP is no longer on lab server. It is kept here and is no longer shared.");
    assert_eq!(ben.named("pGFP"), kept);
    assert_eq!(bios_sync::connection(&ben.project).unwrap().unwrap().constructs, 0);

    // It is not sent back by itself, edited or not: that would undo someone's deletion.
    ben.edit(&kept.id, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    let before = ben.gateway.calls().len();
    assert!(ben.sync().is_empty());
    assert!(calls_since(&ben.gateway, before, "create_document").is_empty());

    // Until he says so. Then it is a new plasmid there.
    bios_sync::send_again(&ben.project, &kept.id).unwrap();
    let report = ben.sync();
    assert_eq!(report.added_there.len(), 1, "{report}");
    assert_eq!(ben.gateway.read_document(&remote_id(&ben, "pGFP")).unwrap().sequence, format!("{BASES}CCCC"));
}

#[test]
fn a_plasmid_removed_from_the_project_there_is_kept_here_too() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), ""), (plasmid("pOther", "GGGGCCCCAAAA"), "")], Role::Editor);
    // Deleted in the web app: the project no longer lists it, though its document still answers.
    ana.gateway.remove_from_project(&ana.remote_project(), &remote_id(&ana, "pOther"));

    for person in [&ana, &ben] {
        let report = person.sync();
        assert!(report.is_clean(), "{report}");
        assert_eq!(report.gone.len(), 1);
        assert_eq!(report.gone[0].what, "pOther was removed from the project on lab server. It is kept here and is no longer shared.");
        assert_eq!(person.names(), vec!["pGFP", "pOther"]);
        assert!(person.sync().is_empty());
    }
}

#[test]
fn a_plasmid_deleted_here_is_reported_and_not_deleted_there() {
    let (ana, ben) = shared(one(), Role::Editor);
    let b = ben.named("pGFP").id;
    ben.project.delete(&b, &ben.who).unwrap();

    let report = ben.sync();

    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].why.starts_with("pGFP was deleted here and is still on lab server."), "{report}");
    assert!(ben.names().is_empty(), "and it is not brought back in behind his back");
    assert_eq!(ana.gateway.list_constructs(&ana.remote_project()).unwrap().constructs.len(), 1);
    // Brought back, it is in step again as if nothing had happened.
    ben.project.undelete(&b, &ben.who).unwrap();
    assert!(ben.sync().is_empty());
}

#[test]
fn a_rename_on_one_side_is_carried_to_the_other() {
    let (ana, ben) = shared(one(), Role::Editor);
    // Renamed here: in Bios that is a save under a new name.
    ana.edit(&ana.named("pGFP").id, "Renamed", |c| c.name = "pGFP v2".into());
    assert!(ana.sync().is_clean());
    // A server keeps a listed name as one word.
    let id = remote_id(&ana, "pGFP_v2");

    let report = ben.sync();
    assert!(report.is_clean(), "{report}");
    assert_eq!(ben.names(), vec!["pGFP_v2"]);
    assert!(ben.project.root().join("pGFP_v2.gb").is_file());

    // Renamed there, as the web app's project list does it.
    ben.gateway.rename_construct(&ben.remote_project(), &id, "pGFP final").unwrap();
    for person in [&ana, &ben] {
        let report = person.sync();
        assert!(report.is_clean(), "{report}");
        assert_eq!(person.names(), vec!["pGFP_final"]);
        assert_eq!(person.named("pGFP_final").construct.sequence, BASES);
    }
    let before = ana.gateway.calls().len();
    assert!(ana.sync().is_empty() && ben.sync().is_empty());
    assert!(calls_since(&ana.gateway, before, "save_document").is_empty());
}

#[test]
fn a_rename_there_alone_renames_the_file_here_without_touching_the_plasmid() {
    let (ana, ben) = shared(one(), Role::Editor);
    let b = ben.named("pGFP").id;
    // The project's own copy on the server is renamed and its document is not: the listing shows
    // a new name while the version stays what it was.
    let project = ana.remote_project();
    let path = ana.gateway.list_constructs(&project).unwrap().constructs[0].path.clone();
    let mut renamed = bios_sync::Snapshot::of(&ana.named("pGFP").construct).unwrap();
    renamed.name = "pGFP-2".into();
    ana.gateway.sync_project(&project, &[(path, renamed)]).unwrap();

    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.carried.len(), 1);
    assert_eq!(report.carried[0].what, "pGFP is now called pGFP-2, as it is on lab server.");
    let now = ben.named("pGFP-2");
    assert_eq!(now.id, b);
    assert_eq!(now.construct.sequence, BASES);
    let last = ben.project.history(&b).unwrap().pop().unwrap();
    assert_eq!((last.actor, last.source.as_str(), last.summary.as_str()), (Actor::Import, SERVER, "Renamed to pGFP-2 on lab server"));
    assert!(ben.sync().is_empty(), "taking a name is not a change to send back");
}

#[test]
fn a_rename_there_is_taken_before_an_edit_here_is_sent_so_it_is_not_undone() {
    let (ana, ben) = shared(one(), Role::Editor);
    let b = ben.named("pGFP").id;
    let project = ana.remote_project();
    let path = ana.gateway.list_constructs(&project).unwrap().constructs[0].path.clone();
    let mut renamed = bios_sync::Snapshot::of(&ana.named("pGFP").construct).unwrap();
    renamed.name = "pGFP-2".into();
    ana.gateway.sync_project(&project, &[(path, renamed)]).unwrap();
    ben.edit(&b, "Ben's edit", |c| c.sequence.push_str("CCCC"));

    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!((report.carried.len(), report.sent.len()), (1, 1), "{report}");
    assert_eq!(ben.names(), vec!["pGFP-2"]);
    // Sending carries the name. Had his old one gone with it, the rename made there would be lost.
    let id = remote_id(&ben, "pGFP-2");
    let there = ben.gateway.read_document(&id).unwrap();
    assert_eq!((there.name.as_deref(), there.sequence.as_str()), (Some("pGFP-2"), "ACGTACGTAGACGTACGTAGCCCC"));
    assert!(ben.sync().is_empty());
    assert!(ana.sync().is_clean());
    assert_eq!(ana.names(), vec!["pGFP-2"]);
}

#[test]
fn a_file_renamed_by_another_program_is_renamed_on_the_server_too() {
    let (ana, ben) = shared(one(), Role::Editor);
    std::fs::rename(ana.project.root().join("pGFP.gb"), ana.project.root().join("pGFP old stock.gb")).unwrap();
    let before = ana.gateway.calls().len();

    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.carried[0].what, "pGFP old stock was renamed on lab server, as it is here.");
    assert_eq!(calls_since(&ana.gateway, before, "rename_construct").len(), 1);
    assert!(calls_since(&ana.gateway, before, "save_document").is_empty(), "a name has no version: no bases were sent");
    remote_id(&ana, "pGFP_old_stock");
    assert_eq!(ana.names(), vec!["pGFP old stock"]);
    assert!(ana.sync().is_empty());
    assert!(ben.sync().is_clean());
    assert_eq!(ben.names(), vec!["pGFP_old_stock"]);
}

#[test]
fn two_different_renames_are_reported_and_settled_by_choosing() {
    let (ana, ben) = shared(one(), Role::Editor);
    ana.edit(&ana.named("pGFP").id, "Renamed", |c| c.name = "pAna".into());
    assert!(ana.sync().is_clean());
    let b = ben.named("pGFP").id;
    ben.edit(&b, "Renamed", |c| c.name = "pBen".into());
    let before = ben.gateway.calls().len();

    let report = ben.sync();

    assert_eq!(report.conflicts.len(), 1, "{report}");
    assert_eq!(
        report.conflicts[0].what,
        "pBen was renamed here, and on lab server it is now called pAna. Neither was touched: choose which one to keep."
    );
    assert_eq!(ben.names(), vec!["pBen"]);
    assert!(calls_since(&ben.gateway, before, "save_document").is_empty());
    remote_id(&ben, "pAna");

    let report = bios_sync::resolve(&ben.project, &ben.gateway, &b, Keep::Mine, &ben.who).unwrap();
    assert!(report.is_clean(), "{report}");
    remote_id(&ben, "pBen");
    assert!(ben.sync().is_empty());
    assert!(ana.sync().is_clean());
    assert_eq!(ana.names(), vec!["pBen"]);
}

#[test]
fn a_move_on_one_side_is_carried_to_the_other_and_two_different_moves_are_reported() {
    let (ana, ben) = shared(one(), Role::Editor);
    let (a, b) = (ana.named("pGFP").id, ben.named("pGFP").id);
    ana.project.move_to(&a, "backbones", &ana.who).unwrap();
    let before = ana.gateway.calls().len();

    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.carried[0].what, "pGFP was moved to backbones on lab server, as it is here.");
    assert!(calls_since(&ana.gateway, before, "save_document").is_empty(), "a move is not a change to the plasmid");
    assert_eq!(ana.gateway.list_constructs(&ana.remote_project()).unwrap().constructs[0].folder, "backbones");

    let report = ben.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.carried[0].what, "pGFP was moved to backbones, as it is on lab server.");
    assert_eq!(ben.named("pGFP").path, Path::new("backbones/pGFP.gb"));
    let last = ben.project.history(&b).unwrap().pop().unwrap();
    assert_eq!((last.actor, last.source.as_str(), last.summary.as_str()), (Actor::Import, SERVER, "Moved to backbones"));
    assert!(ana.sync().is_empty() && ben.sync().is_empty());

    // Back to the top there; somewhere else here.
    ana.project.move_to(&a, "", &ana.who).unwrap();
    assert!(ana.sync().is_clean());
    ben.project.move_to(&b, "mine", &ben.who).unwrap();
    let report = ben.sync();
    assert_eq!(report.skipped.len(), 1, "{report}");
    assert_eq!(
        report.skipped[0].why,
        "pGFP was moved to mine here and to the top of the project on lab server. Neither was touched: move one of them so they agree."
    );
    assert_eq!(ben.named("pGFP").path, Path::new("mine/pGFP.gb"));
    assert_eq!(ana.gateway.list_constructs(&ana.remote_project()).unwrap().constructs[0].folder, "");
    // He moves his to where theirs is, and it is settled.
    ben.project.move_to(&b, "", &ben.who).unwrap();
    assert!(ben.sync().is_empty());
}

#[test]
fn connecting_a_folder_that_already_holds_a_plasmid_links_it_instead_of_doubling_it() {
    let server = FakeGateway::new("ana");
    let ana = Person::new("ana", server.clone());
    ana.add(plasmid("pGFP", BASES), "");
    ana.add(plasmid("pOnlyAna", "GGGGCCCCAAAA"), "");
    ana.publish("Shared work");
    let project = ana.remote_project();
    server.share(&project, "ben", Role::Editor);
    let ben = Person::new("ben", server.as_account("ben"));
    ben.add(plasmid("my copy of GFP", BASES), "mine");
    ben.add(plasmid("pOnlyBen", "TTTTTTTTGGGG"), "");

    let report = ben.connect(&project);

    assert!(report.is_clean(), "{report}");
    assert_eq!((report.matched.len(), report.added_here.len(), report.added_there.len()), (1, 1, 1), "{report}");
    assert_eq!(report.matched[0].what, "my copy of GFP was already here and on lab server; the two are now kept in step.");
    assert_eq!(ben.names(), vec!["my copy of GFP", "pOnlyAna", "pOnlyBen"]);
    assert_eq!(server.list_constructs(&project).unwrap().constructs.len(), 3);
    assert!(ben.sync().is_empty());

    // The linked pair is one plasmid from now on, each side under its own name and place.
    ana.edit(&ana.named("pGFP").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    assert!(ana.sync().is_clean());
    assert!(ben.sync().is_clean());
    let linked = ben.named("my copy of GFP");
    assert_eq!(linked.construct.sequence, format!("{BASES}AAAA"));
    assert_eq!(linked.path, Path::new("mine/my copy of GFP.gb"));
    assert_eq!(ana.names(), vec!["pGFP", "pOnlyAna", "pOnlyBen"]);

    // Connecting again to the same project is just a sync; to another, refused.
    assert!(ben.connect(&project).is_empty());
    assert!(matches!(bios_sync::connect(&ben.project, &ben.gateway, SERVER, "elsewhere", &ben.who), Err(SyncError::AlreadyConnected { .. })));
}

#[test]
fn a_project_that_is_not_listed_any_more_is_not_synced_at_all() {
    let (_ana, ben) = shared(one(), Role::Editor);
    let b = ben.named("pGFP").id;
    ben.edit(&b, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    let stranger = ben.gateway.as_account("someone else");

    let failed = bios_sync::sync(&ben.project, &stranger, &ben.who).unwrap_err();

    assert_eq!(failed, SyncError::ProjectGone { server: SERVER.into() });
    assert_eq!(failed.to_string(), "this project is no longer on lab server, or this account may no longer open it; nothing was changed here");
    // Nothing was let go of on the strength of a project that could not be seen.
    assert_eq!(bios_sync::connection(&ben.project).unwrap().unwrap().constructs, 1);
    assert_eq!(ben.sync().sent.len(), 1);
    assert!(matches!(
        bios_sync::connect(&Person::new("cy", stranger.clone()).project, &stranger, SERVER, "no-such-project", &ben.who),
        Err(SyncError::ProjectGone { .. })
    ));
}

#[test]
fn an_empty_folder_made_there_is_made_here() {
    let (ana, ben) = shared(one(), Role::Editor);
    ana.gateway.make_folder(&ana.remote_project(), "primers/ordered");
    assert!(ben.sync().is_empty());
    assert_eq!(ben.project.folders().unwrap(), vec![Path::new("primers").to_path_buf(), Path::new("primers/ordered").to_path_buf()]);
}

#[test]
fn text_that_is_behind_its_sequence_or_is_not_a_plasmid_is_never_brought_in() {
    let (ana, ben) = shared(one(), Role::Editor);
    let id = remote_id(&ana, "pGFP");
    let here = ben.named("pGFP");
    // The server holds newer bases than its text describes.
    ana.gateway.save_bases_without_text(&id, "ACGTACGTAGACGTACGTAGTTTT");

    let report = ben.sync();

    assert_eq!(report.skipped.len(), 1, "{report}");
    assert_eq!(
        report.skipped[0].why,
        "pGFP was not brought in from lab server: its file there is older than its sequence; open it in the Bios web app and save it, then sync again."
    );
    assert_eq!(ben.named("pGFP"), here);

    // An answer that is not a plasmid at all, for one that changed and for one that is new.
    save_there(&ana.gateway, &id, |c| c.sequence.push_str("GG"));
    ana.add(plasmid("pNew", "TTTTGGGGCCCC"), "");
    assert_eq!(ana.sync().added_there.len(), 1);
    let mut meddler = Meddler::new(&ben.gateway);
    meddler.garble = true;

    let report = bios_sync::sync(&ben.project, &meddler, &ben.who).unwrap();

    assert_eq!(report.skipped.len(), 2, "{report}");
    assert!(report.skipped.iter().all(|s| s.why.contains("its file there could not be read")), "{report}");
    assert_eq!(ben.named("pGFP"), here);
    assert_eq!(ben.names(), vec!["pGFP"]);
    // When the server answers properly again, it all arrives.
    let report = ben.sync();
    assert!(report.is_clean(), "{report}");
    assert_eq!((report.brought_in.len(), report.added_here.len()), (1, 1));
}

#[test]
fn a_report_reads_as_sentences_with_what_needs_a_person_first() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), ""), (plasmid("pOther", "GGGGCCCCAAAA"), "")], Role::Editor);
    ana.edit(&ana.named("pGFP").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    ana.edit(&ana.named("pOther").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    assert!(ana.sync().is_clean());
    ben.edit(&ben.named("pGFP").id, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    ben.add(plasmid("pNew", "TTTTGGGGCCCC"), "");

    let report = ben.sync();

    let lines = report.lines();
    assert_eq!(lines.len(), 3, "{report}");
    assert!(lines[0].starts_with("pGFP was changed both here"));
    assert!(lines[1].starts_with("pOther was brought in from lab server, saved there 20"));
    assert_eq!(lines[2], "pNew is new here and was added to lab server.");
    assert_eq!(report.to_string(), lines.join("\n"));
    assert!(!report.is_clean() && !report.is_empty());
}
