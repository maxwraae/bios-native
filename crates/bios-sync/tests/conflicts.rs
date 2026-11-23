//! A plasmid changed in two places: nothing is merged, neither side is overwritten, and a person
//! settles it. Also the saves that go wrong on the way.

mod support;

use bios_project::Actor;
use bios_sync::{Gateway, GatewayError, Keep, Role, SyncError};
use support::{Meddler, Person, SERVER, calls_since, plasmid, remote_id, save_there, shared};

const BASES: &str = "ACGTACGTAGACGTACGTAG";

/// Ana saved `AAAA` on the end and it is on the server; Ben saved `CCCC` on the end and has not
/// synced since.
fn changed_on_both() -> (Person, Person, String) {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    ana.edit(&ana.named("pGFP").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    assert!(ana.sync().is_clean());
    ben.edit(&ben.named("pGFP").id, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    let id = remote_id(&ana, "pGFP");
    (ana, ben, id)
}

#[test]
fn two_people_take_turns_and_both_keep_the_whole_history() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let (a, b) = (ana.named("pGFP").id, ben.named("pGFP").id);
    for _ in 0..3 {
        ana.edit(&a, "Ana's edit", |c| c.sequence.push('A'));
        assert!(ana.sync().is_clean());
        assert!(ben.sync().is_clean());
        ben.edit(&b, "Ben's edit", |c| c.sequence.push('C'));
        assert!(ben.sync().is_clean());
        assert!(ana.sync().is_clean());
    }

    let expected = format!("{BASES}ACACAC");
    assert_eq!(ana.named("pGFP").construct.sequence, expected);
    assert_eq!(ben.named("pGFP").construct.sequence, expected);
    assert_eq!(ana.gateway.read_document(&remote_id(&ana, "pGFP")).unwrap().version, 7);
    // Every version is in both histories, each with where it came from.
    let sources = |person: &Person, id| person.project.history(id).unwrap().into_iter().map(|e| e.source).collect::<Vec<_>>();
    assert_eq!(sources(&ana, &a), vec!["test", "test", SERVER, "test", SERVER, "test", SERVER]);
    assert_eq!(sources(&ben, &b), vec![SERVER, SERVER, "test", SERVER, "test", SERVER, "test"]);
    assert_eq!(ana.project.at_version(&a, 3).unwrap().sequence, format!("{BASES}AC"));
    assert_eq!(ben.project.at_version(&b, 4).unwrap().sequence, format!("{BASES}ACA"));
    assert!(ana.sync().is_empty() && ben.sync().is_empty());
}

#[test]
fn a_plasmid_changed_on_both_sides_is_left_alone_on_both_and_reported() {
    let (_ana, ben, id) = changed_on_both();
    let there = ben.gateway.read_document(&id).unwrap();
    let here = ben.named("pGFP");
    let before = ben.gateway.calls().len();

    let report = ben.sync();

    assert!(!report.is_clean());
    assert_eq!(report.conflicts.len(), 1);
    let conflict = &report.conflicts[0];
    assert_eq!((conflict.construct.clone(), conflict.name.as_str()), (here.id.clone(), "pGFP"));
    assert_eq!(conflict.theirs_saved, there.updated_at);
    assert_eq!(conflict.mine_saved.as_deref(), Some(ben.project.history(&here.id).unwrap()[1].at.as_str()));
    assert!(conflict.what.starts_with("pGFP was changed both here (saved 20"), "{}", conflict.what);
    assert!(conflict.what.contains(" and on lab server (saved 20"), "{}", conflict.what);
    assert!(conflict.what.ends_with("Neither was touched: choose which one to keep."), "{}", conflict.what);
    // Neither side moved.
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there);
    assert_eq!(ben.named("pGFP"), here);
    assert!(calls_since(&ben.gateway, before, "save_document").is_empty());

    // It stays a conflict, and stays untouched, until someone decides.
    assert_eq!(ben.sync().conflicts.len(), 1);
    assert_eq!(ben.named("pGFP"), here);
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there);
}

#[test]
fn keeping_mine_saves_it_on_top_of_theirs() {
    let (ana, ben, id) = changed_on_both();
    let b = ben.named("pGFP").id;

    let report = bios_sync::resolve(&ben.project, &ben.gateway, &b, Keep::Mine, &ben.who).unwrap();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.sent.len(), 1);
    let there = ben.gateway.read_document(&id).unwrap();
    assert_eq!(there.version, 3, "on top of Ana's version 2, not instead of it");
    assert_eq!(there.sequence, format!("{BASES}CCCC"));
    assert!(ben.sync().is_empty());
    assert_eq!(ben.project.history(&b).unwrap().len(), 2, "nothing was written here");

    // Ana gets Ben's version; the one she saved is still in her history.
    let a = ana.named("pGFP").id;
    let report = ana.sync();
    assert!(report.is_clean(), "{report}");
    assert_eq!(report.brought_in.len(), 1);
    assert_eq!(ana.named("pGFP").construct.sequence, format!("{BASES}CCCC"));
    assert_eq!(ana.project.at_version(&a, 2).unwrap().sequence, format!("{BASES}AAAA"));
}

#[test]
fn keeping_theirs_brings_it_in_and_mine_stays_in_the_history() {
    let (ana, ben, id) = changed_on_both();
    let b = ben.named("pGFP").id;
    let there = ben.gateway.read_document(&id).unwrap();

    let report = bios_sync::resolve(&ben.project, &ben.gateway, &b, Keep::Theirs, &ben.who).unwrap();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.brought_in.len(), 1);
    assert_eq!(ben.named("pGFP").construct.sequence, format!("{BASES}AAAA"));
    let history = ben.project.history(&b).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!((history[2].actor, history[2].source.as_str()), (Actor::Import, SERVER));
    assert_eq!(ben.project.at_version(&b, 2).unwrap().sequence, format!("{BASES}CCCC"), "Ben's own version can be gone back to");
    assert_eq!(ben.gateway.read_document(&id).unwrap(), there, "the server was not written");
    assert!(ben.sync().is_empty() && ana.sync().is_empty());
}

#[test]
fn settling_needs_a_shared_plasmid() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let local_only = ben.add(plasmid("pMine", "ACGTTTTT"), "");
    assert_eq!(bios_sync::resolve(&ben.project, &ben.gateway, &local_only.id, Keep::Mine, &ben.who), Err(SyncError::NotShared("pMine".into())));
    // With nothing to settle, either answer changes nothing.
    let b = ben.named("pGFP").id;
    let before = ben.gateway.calls().len();
    for keep in [Keep::Mine, Keep::Theirs] {
        assert!(bios_sync::resolve(&ben.project, &ben.gateway, &b, keep, &ben.who).unwrap().sent.is_empty());
    }
    assert!(calls_since(&ben.gateway, before, "save_document").is_empty());
    assert_eq!(ben.project.history(&b).unwrap().len(), 1);
    assert!(ana.sync().is_empty());
}

#[test]
fn a_save_the_server_refuses_as_stale_is_a_conflict_not_an_overwrite() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let id = remote_id(&ana, "pGFP");
    let b = ben.named("pGFP").id;
    ben.edit(&b, "Ben's edit", |c| c.sequence.push_str("CCCC"));
    let here = ben.named("pGFP");
    // Ana saves in the moment between Ben's sync reading the server's copy and sending his own.
    let meddler = Meddler::new(&ben.gateway);
    *meddler.before_save.borrow_mut() = Some(Box::new(|| {
        save_there(&ana.gateway, &id, |c| c.sequence.push_str("GGGG"));
    }));

    let report = bios_sync::sync(&ben.project, &meddler, &ben.who).unwrap();

    assert_eq!(report.conflicts.len(), 1, "{report}");
    assert!(report.sent.is_empty());
    assert!(report.conflicts[0].theirs_saved.is_some());
    let there = ben.gateway.read_document(&id).unwrap();
    assert_eq!((there.version, there.sequence.as_str()), (2, "ACGTACGTAGACGTACGTAGGGGG"));
    assert_eq!(ben.named("pGFP"), here);
    // The next sync sees the same thing without trying to save again.
    let before = ben.gateway.calls().len();
    assert_eq!(ben.sync().conflicts.len(), 1);
    assert!(calls_since(&ben.gateway, before, "save_document").is_empty());
}

#[test]
fn a_save_whose_answer_was_lost_is_repeated_as_the_same_save() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let id = remote_id(&ana, "pGFP");
    ana.edit(&ana.named("pGFP").id, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    let start = ana.gateway.calls().len();
    ana.gateway.lose_next_answer();

    let report = ana.sync();

    assert!(!report.is_clean());
    assert!(report.sent.is_empty());
    assert!(report.skipped[0].why.starts_with("pGFP was not sent to lab server: the server could not be reached"), "{report}");
    assert_eq!(ana.gateway.read_document(&id).unwrap().version, 2, "the save did happen");

    // Here it looks changed on both sides. It is not a conflict: it is the same save, sent again.
    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!((report.sent.len(), report.conflicts.len()), (1, 0));
    assert_eq!(ana.gateway.read_document(&id).unwrap().version, 2, "and it was not applied twice");
    let saves = calls_since(&ana.gateway, start, "save_document");
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[0], saves[1], "the same version named, the same request id");
    assert!(ana.sync().is_empty());
    assert_eq!(ben.sync().brought_in.len(), 1);
    assert_eq!(ben.named("pGFP").construct.sequence, format!("{BASES}AAAA"));
}

#[test]
fn a_repeated_save_that_someone_has_saved_over_since_ends_with_theirs_brought_in() {
    let (ana, ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let id = remote_id(&ana, "pGFP");
    let a = ana.named("pGFP").id;
    ana.edit(&a, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    ana.gateway.lose_next_answer();
    assert!(!ana.sync().is_clean());
    // Ben's copy of Bios, or the web app, saves on top of the version Ana's lost save made.
    save_there(&ben.gateway, &id, |c| c.sequence.push_str("CC"));

    let report = ana.sync();
    assert!(report.conflicts.is_empty(), "{report}");
    let report = ana.sync();

    assert!(report.is_clean(), "{report}");
    assert_eq!(report.brought_in.len(), 1);
    assert_eq!(ana.named("pGFP").construct.sequence, format!("{BASES}AAAACC"));
    assert_eq!(ana.gateway.read_document(&id).unwrap().version, 3);
    assert_eq!(ana.project.at_version(&a, 2).unwrap().sequence, format!("{BASES}AAAA"));
    assert!(ana.sync().is_empty());
}

#[test]
fn an_edit_made_after_a_lost_answer_is_a_new_save_and_a_real_conflict() {
    let (ana, _ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let id = remote_id(&ana, "pGFP");
    let a = ana.named("pGFP").id;
    ana.edit(&a, "First", |c| c.sequence.push_str("AAAA"));
    ana.gateway.lose_next_answer();
    assert!(!ana.sync().is_clean());
    ana.edit(&a, "Second", |c| c.sequence.push_str("TT"));
    let before = ana.gateway.calls().len();

    // What is here now was never sent, and the server's copy moved: that is changed on both.
    let report = ana.sync();

    assert_eq!(report.conflicts.len(), 1, "{report}");
    assert!(calls_since(&ana.gateway, before, "save_document").is_empty());
    assert_eq!(ana.gateway.read_document(&id).unwrap().sequence, format!("{BASES}AAAA"));
}

#[test]
fn with_no_server_nothing_is_changed_and_the_error_says_what_to_do() {
    let (ana, _ben) = shared(vec![(plasmid("pGFP", BASES), "")], Role::Editor);
    let a = ana.named("pGFP").id;
    ana.edit(&a, "Ana's edit", |c| c.sequence.push_str("AAAA"));
    let here = ana.named("pGFP");
    ana.gateway.set_offline(true);

    let failed = bios_sync::sync(&ana.project, &ana.gateway, &ana.who).unwrap_err();

    assert!(matches!(failed, SyncError::Gateway(GatewayError::Unreachable(_))));
    assert_eq!(failed.to_string(), "the server could not be reached (the fake server is offline); check the connection and try again");
    assert_eq!(ana.named("pGFP"), here);
    ana.gateway.set_offline(false);
    assert_eq!(ana.sync().sent.len(), 1);
}
