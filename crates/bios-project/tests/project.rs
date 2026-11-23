//! A project in a folder: one copy, checked saves, history that is only added to.

use std::fs;
use std::path::{Path, PathBuf};

use bios_project::{Actor, Change, Error, Project, Who};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name)
}

fn me() -> Who {
    Who { actor: Actor::Human, source: "test".into(), session: None }
}

fn agent() -> Who {
    Who { actor: Actor::Agent, source: "mcp".into(), session: Some("run-1".into()) }
}

fn change(summary: &str) -> Change {
    Change { command: "test.edit".into(), params: serde_json::Value::Null, summary: summary.into() }
}

fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, project)
}

#[test]
fn a_created_project_opens_again() {
    let (_dir, project) = project();
    let again = Project::open(project.root()).unwrap();
    assert_eq!(again.name(), "Lab");
    assert_eq!(again.id(), project.id());
    assert!(matches!(Project::create(project.root(), "Lab"), Err(Error::AlreadyAProject(_))));
}

#[test]
fn a_folder_without_bios_json_opens_but_is_not_yet_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let opened = Project::open(dir.path()).unwrap();
    assert!(!opened.is_saved());
    assert!(!Project::is_project(dir.path()));
    assert!(matches!(Project::open_saved(dir.path()), Err(Error::NotAProject(_))));
    assert!(matches!(Project::open(&dir.path().join("missing")), Err(Error::NotAProject(_))));
}

/// Every file under `dir` with its bytes, to show that nothing was written.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((path.strip_prefix(dir).unwrap().to_path_buf(), fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// The smallest SnapGene file the reader takes: the cookie block, then a sequence block (one byte
/// of flags, 0 for linear, then the bases). Each block is one byte of type, four bytes of
/// big-endian length, then that many bytes.
fn snapgene_file(bases: &str) -> Vec<u8> {
    let mut cookie = b"SnapGene".to_vec();
    cookie.extend([0, 1, 0, 15, 0, 20]);
    let mut dna = vec![0u8];
    dna.extend(bases.as_bytes());
    let mut out = Vec::new();
    for (kind, payload) in [(9u8, cookie), (0, dna)] {
        out.push(kind);
        out.extend(u32::try_from(payload.len()).unwrap().to_be_bytes());
        out.extend(payload);
    }
    out
}

#[test]
fn the_snapgene_file_built_here_opens() {
    let opened = bios_core::formats::read(&snapgene_file("ATGGCCAAGTAA"), "mini_snapgene.dna").unwrap();
    assert_eq!(opened.format, bios_core::formats::Format::SnapGene);
    assert_eq!((opened.construct.name.as_str(), opened.construct.sequence.as_str()), ("mini_snapgene", "ATGGCCAAGTAA"));
}

/// A folder of plain sequence files, as they come out of Downloads: GenBank, FASTA and SnapGene.
/// The GenBank file is not called what its LOCUS line says, as downloads often are not.
fn downloads() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Downloads");
    fs::create_dir_all(root.join("older")).unwrap();
    fs::copy(fixture("pUC19.gb"), root.join("pUC19_map.gb")).unwrap();
    fs::copy(fixture("wrap_demo.gb"), root.join("older/wrap_demo.gb")).unwrap();
    fs::copy(fixture("gfp.fasta"), root.join("gfp.fasta")).unwrap();
    fs::write(root.join("mini_snapgene.dna"), snapgene_file("ATGGCCAAGTAA")).unwrap();
    fs::write(root.join("notes.txt"), "not a plasmid").unwrap();
    (dir, root)
}

#[test]
fn a_plain_folder_is_read_from_its_files_and_never_written_by_reading() {
    let (_dir, root) = downloads();
    let before = snapshot(&root);
    let project = Project::open(&root).unwrap();
    assert_eq!(project.name(), "Downloads");

    let listing = project.list().unwrap();
    let paths: Vec<&Path> = listing.iter().map(|l| l.path.as_path()).collect();
    assert_eq!(paths, [Path::new("gfp.fasta"), Path::new("mini_snapgene.dna"), Path::new("older/wrap_demo.gb"), Path::new("pUC19_map.gb")]);
    assert_eq!(listing[3].length, 2686);
    for l in &listing {
        let read = project.read(&l.id).unwrap();
        assert_eq!(read.version, 1);
        let history = project.history(&l.id).unwrap();
        assert_eq!(history.len(), 1, "{}", l.name);
        assert_eq!(project.at_version(&l.id, 1).unwrap().sequence, read.construct.sequence);
    }
    // The same file has the same id from one read to the next.
    assert_eq!(Project::open(&root).unwrap().list().unwrap(), listing);
    assert_eq!(project.folders().unwrap(), [PathBuf::from("older")]);
    assert!(project.untracked().unwrap().is_empty());
    assert!(project.deleted().unwrap().is_empty());
    assert_eq!(project.read_sidecar("remote").unwrap(), None);
    project.stamp();
    project.last_changed();

    assert_eq!(snapshot(&root), before);
    assert!(!project.is_saved());
}

#[test]
fn the_first_edit_makes_it_a_project_and_touches_only_the_file_edited() {
    let (_dir, root) = downloads();
    let before = snapshot(&root);
    let project = Project::open(&root).unwrap();
    let target = project.list().unwrap().into_iter().find(|l| l.name == "pUC19_map").unwrap();
    let loaded = project.read(&target.id).unwrap();
    let mut edited = loaded.construct.clone();
    edited.sequence.push_str("GAATTC");

    let saved = project.save(&target.id, &loaded.sha, &edited, &me(), change("Added EcoRI")).unwrap();

    assert!(project.is_saved());
    assert!(root.join("bios.json").is_file() && root.join(".bios/journal").is_dir());
    assert_eq!(saved.id, target.id);
    assert_eq!(saved.version, 2);
    let after = snapshot(&root);
    for (path, bytes) in &before {
        let now = after.iter().find(|(p, _)| p == path).map(|(_, b)| b);
        if path == Path::new("pUC19_map.gb") {
            assert!(String::from_utf8_lossy(now.unwrap()).contains(&format!("bios_construct_id: {}", target.id)));
        } else {
            assert_eq!(now, Some(bytes), "{} was touched", path.display());
        }
    }
    let commands: Vec<String> = project.history(&target.id).unwrap().into_iter().map(|e| e.command).collect();
    assert_eq!(commands, ["construct.adopt", "test.edit"]);
    // Going back to the file as it was found works, because the first edit wrote it down.
    assert_eq!(project.at_version(&target.id, 1).unwrap().sequence, loaded.construct.sequence);
    assert_eq!(project.list().unwrap().len(), 4);
}

#[test]
fn editing_a_file_that_is_not_genbank_converts_it_and_keeps_the_original() {
    let (_dir, root) = downloads();
    let original = fs::read(root.join("mini_snapgene.dna")).unwrap();
    let project = Project::open(&root).unwrap();
    let target = project.list().unwrap().into_iter().find(|l| l.name == "mini_snapgene").unwrap();
    let loaded = project.read(&target.id).unwrap();
    let mut edited = loaded.construct.clone();
    edited.sequence.push_str("ACGT");

    let saved = project.save(&target.id, &loaded.sha, &edited, &me(), change("edit")).unwrap();

    assert_eq!(saved.path, Path::new("mini_snapgene.gb"));
    assert_eq!(saved.id, target.id);
    assert!(!root.join("mini_snapgene.dna").exists());
    assert_eq!(fs::read(root.join(".bios/originals/mini_snapgene.dna")).unwrap(), original);
    assert_eq!(project.list().unwrap().len(), 4);
    let history = project.history(&target.id).unwrap();
    assert_eq!(history.len(), 3);
    assert!(history[1].summary.starts_with("Converted from mini_snapgene.dna"), "{}", history[1].summary);
}

#[test]
fn a_file_without_an_id_keeps_its_history_when_moved() {
    let (_dir, root) = downloads();
    let project = Project::open(&root).unwrap();
    let target = project.list().unwrap().into_iter().find(|l| l.name == "pUC19_map").unwrap();
    let moved = project.move_to(&target.id, "backbones", &me()).unwrap();
    assert_eq!(moved.path, Path::new("backbones/pUC19_map.gb"));
    let again = project.list().unwrap().into_iter().find(|l| l.name == "pUC19_map").unwrap();
    assert_eq!(again.id, target.id);
    let commands: Vec<String> = project.history(&target.id).unwrap().into_iter().map(|e| e.command).collect();
    assert_eq!(commands, ["construct.adopt", "construct.adopt", "move_plasmid"]);
}

#[test]
fn import_copies_the_file_and_never_writes_the_original() {
    let (dir, project) = project();
    // Named apart from its LOCUS line, to show the copy is named after the construct.
    let source = dir.path().join("pUC19_map.gb");
    fs::copy(fixture("pUC19.gb"), &source).unwrap();
    let before = fs::read(&source).unwrap();

    let loaded = project.import(&source, "", None, &me()).unwrap().construct;

    assert_eq!(fs::read(&source).unwrap(), before);
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.path, Path::new("pUC19.gb"));
    assert_eq!(loaded.construct.len(), 2686);
    let history = project.history(&loaded.id).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].command, "import_plasmid");
    assert_eq!(history[0].origin.as_deref(), Some(source.display().to_string().as_str()));
    assert_eq!(project.list().unwrap().len(), 1);
}

#[test]
fn importing_the_same_file_twice_gives_one_construct() {
    let (_dir, project) = project();
    let first = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let second = project.import(&fixture("pUC19.gb"), "elsewhere", Some("pUC19 copy"), &me()).unwrap();
    assert!(second.already_here);
    assert_eq!(second.construct.id, first.id);
    assert_eq!(second.construct.path, first.path);
    assert_eq!(project.list().unwrap().len(), 1);
    assert_eq!(project.history(&first.id).unwrap().len(), 1);
}

#[test]
fn a_file_that_carries_a_bios_id_keeps_it() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("wrap_demo.gb"), "demos", None, &me()).unwrap().construct;
    assert_eq!(loaded.id.as_str(), "e1bce6ce-3ba7-41a1-bc1f-21012f5d610f");
    assert_eq!(loaded.path, Path::new("demos/wrap_demo.gb"));
}

#[test]
fn a_save_adds_a_version_and_says_who_made_it() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let mut edited = loaded.construct.clone();
    edited.sequence.push_str("ACGT");

    let saved = project.save(&loaded.id, &loaded.sha, &edited, &agent(), change("Added four bases")).unwrap();

    assert_eq!(saved.version, 2);
    assert_ne!(saved.sha, loaded.sha);
    let reread = project.read(&loaded.id).unwrap();
    assert_eq!(reread.construct.len(), 2690);
    assert_eq!(reread.sha, saved.sha);
    let history = project.history(&loaded.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].actor, Actor::Agent);
    assert_eq!(history[1].session.as_deref(), Some("run-1"));
    assert_eq!(history[1].base_sha.as_deref(), Some(loaded.sha.as_str()));
    assert_eq!(history[1].summary, "Added four bases");
}

#[test]
fn a_save_against_an_old_version_is_refused_and_changes_nothing() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let mut first = loaded.construct.clone();
    first.sequence.push_str("AAAA");
    let saved = project.save(&loaded.id, &loaded.sha, &first, &me(), change("first")).unwrap();

    let mut second = loaded.construct.clone();
    second.sequence.push_str("CCCC");
    let refused = project.save(&loaded.id, &loaded.sha, &second, &agent(), change("second"));

    assert!(matches!(refused, Err(Error::Conflict { .. })));
    let now = project.read(&loaded.id).unwrap();
    assert_eq!(now.sha, saved.sha);
    assert!(now.construct.sequence.ends_with("AAAA"));
    assert_eq!(project.history(&loaded.id).unwrap().len(), 2);
}

#[test]
fn saving_what_is_already_there_is_not_a_version() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let same = project.save(&loaded.id, &loaded.sha, &loaded.construct, &me(), change("nothing")).unwrap();
    assert_eq!(same.version, 1);
    assert_eq!(project.history(&loaded.id).unwrap().len(), 1);
}

#[test]
fn an_empty_construct_is_never_written() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let mut empty = loaded.construct.clone();
    empty.sequence.clear();
    assert!(matches!(project.save(&loaded.id, &loaded.sha, &empty, &me(), change("oops")), Err(Error::Unwritable(_))));
    assert_eq!(project.read(&loaded.id).unwrap().construct.len(), 2686);
}

#[test]
fn going_back_restores_an_old_version_as_a_new_one() {
    let (_dir, project) = project();
    let v1 = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let mut edited = v1.construct.clone();
    edited.sequence.push_str("ACGT");
    let v2 = project.save(&v1.id, &v1.sha, &edited, &agent(), change("agent edit")).unwrap();
    let mut again = v2.construct.clone();
    again.sequence.push_str("TTTT");
    let v3 = project.save(&v1.id, &v2.sha, &again, &me(), change("my edit")).unwrap();

    let back = project.restore(&v1.id, 1, &me()).unwrap();

    assert_eq!(back.version, 4);
    assert_eq!(back.sha, v1.sha);
    assert_eq!(back.construct.len(), 2686);
    let history = project.history(&v1.id).unwrap();
    assert_eq!(history.iter().map(|e| e.version).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    assert_eq!(history[3].command, "restore_version");
    // Nothing in between is lost: the versions that were undone can still be read.
    assert_eq!(project.at_version(&v1.id, 3).unwrap().len(), v3.construct.len());
    assert_eq!(project.at_version(&v1.id, 2).unwrap().len(), 2690);
    assert!(matches!(project.restore(&v1.id, 99, &me()), Err(Error::UnknownVersion(99))));
}

#[test]
fn a_duplicate_remembers_what_it_was_made_from() {
    let (_dir, project) = project();
    let parent = project.import(&fixture("pUC19.gb"), "backbones", None, &me()).unwrap().construct;
    let child = project.duplicate(&parent.id, "pUC19 with insert", &me()).unwrap();

    assert_ne!(child.id, parent.id);
    assert_eq!(child.path, Path::new("backbones/pUC19 with insert.gb"));
    let listed = project.list().unwrap();
    assert_eq!(listed.len(), 2);
    let listed_child = listed.iter().find(|l| l.id == child.id).unwrap();
    assert_eq!(listed_child.derived_from.as_ref(), Some(&parent.id));
    assert_eq!(listed.iter().find(|l| l.id == parent.id).unwrap().derived_from, None);
}

#[test]
fn a_change_made_by_another_program_becomes_a_version() {
    let (_dir, project) = project();
    let loaded = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let path = project.root().join(&loaded.path);
    let text = fs::read_to_string(&path).unwrap().replace("lac promoter", "lac prom");
    fs::write(&path, text).unwrap();

    let reread = project.read(&loaded.id).unwrap();

    assert_eq!(reread.version, 2);
    assert_ne!(reread.sha, loaded.sha);
    let history = project.history(&loaded.id).unwrap();
    assert_eq!(history[1].command, "external.change");
    assert_eq!(history[1].actor, Actor::Import);
    // And a save made against the version from before that change is refused.
    assert!(matches!(project.save(&loaded.id, &loaded.sha, &loaded.construct, &me(), change("stale")), Err(Error::Conflict { .. })));
}

#[test]
fn renaming_moves_the_file_and_never_onto_another_construct() {
    let (_dir, project) = project();
    let a = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let b = project.import(&fixture("wrap_demo.gb"), "", None, &me()).unwrap().construct;

    let mut renamed = a.construct.clone();
    renamed.name = "pUC19 with EGFP".into();
    let saved = project.save(&a.id, &a.sha, &renamed, &me(), change("Renamed")).unwrap();
    assert_eq!(saved.path, Path::new("pUC19 with EGFP.gb"));
    assert!(project.root().join("pUC19 with EGFP.gb").exists());
    // A name with spaces survives being written and read back.
    assert_eq!(project.read(&a.id).unwrap().construct.name, "pUC19 with EGFP");
    assert!(!project.root().join("pUC19.gb").exists());

    let mut clash = b.construct.clone();
    clash.name = "pUC19 with EGFP".into();
    assert!(matches!(project.save(&b.id, &b.sha, &clash, &me(), change("Renamed")), Err(Error::NameTaken(_))));
    assert_eq!(project.read(&b.id).unwrap().path, Path::new("wrap_demo.gb"));
}

#[test]
fn a_folder_cannot_leave_the_project() {
    let (_dir, project) = project();
    for folder in ["../outside", "/tmp", ".bios/objects"] {
        assert!(matches!(project.import(&fixture("pUC19.gb"), folder, None, &me()), Err(Error::BadFolder(_))), "{folder}");
    }
    assert!(project.list().unwrap().is_empty());
}

#[test]
fn a_file_dropped_into_the_folder_is_listed_and_one_that_cannot_be_read_is_reported() {
    let (_dir, project) = project();
    fs::copy(fixture("pUC19.gb"), project.root().join("stray.gb")).unwrap();
    fs::write(project.root().join("broken.gb"), "LOCUS nothing here").unwrap();
    let listed: Vec<String> = project.list().unwrap().into_iter().map(|l| l.name).collect();
    assert_eq!(listed, ["stray"]);
    let untracked = project.untracked().unwrap();
    assert_eq!(untracked.len(), 1);
    assert_eq!(untracked[0].0, Path::new("broken.gb"));
}

#[test]
fn going_back_keeps_the_name_and_the_place() {
    let (_dir, project) = project();
    let parent = project.import(&fixture("pUC19.gb"), "backbones", None, &me()).unwrap().construct;
    let v1 = project.duplicate(&parent.id, "pUC19 with EcoRI", &me()).unwrap();
    let mut edited = v1.construct.clone();
    edited.sequence.push_str("GAATTC");
    project.save(&v1.id, &v1.sha, &edited, &agent(), change("agent edit")).unwrap();

    let back = project.restore(&v1.id, 1, &me()).unwrap();

    assert_eq!(back.path, Path::new("backbones/pUC19 with EcoRI.gb"));
    assert_eq!(back.construct.name, "pUC19 with EcoRI");
    assert_eq!(back.sha, v1.sha);
    assert_eq!(project.at_version(&v1.id, 2).unwrap().name, "pUC19 with EcoRI");
    assert_eq!(project.list().unwrap().len(), 2);
}

#[test]
fn a_move_takes_the_history_along() {
    let (_dir, project) = project();
    let a = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let moved = project.move_to(&a.id, "backbones/2026", &me()).unwrap();
    assert_eq!(moved.path, Path::new("backbones/2026/pUC19.gb"));
    // The text did not change, so an edit made against the version from before the move still fits.
    assert_eq!(moved.sha, a.sha);
    let history = project.history(&a.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].summary, "Moved to backbones/2026");
    assert_eq!(project.folders().unwrap(), vec![Path::new("backbones").to_path_buf(), Path::new("backbones/2026").to_path_buf()]);
}

#[test]
fn a_deleted_construct_can_be_brought_back_with_its_history() {
    let (_dir, project) = project();
    let a = project.import(&fixture("pUC19.gb"), "backbones", None, &me()).unwrap().construct;
    project.delete(&a.id, &agent()).unwrap();
    assert!(project.list().unwrap().is_empty());
    assert!(!project.root().join("backbones/pUC19.gb").exists());
    let gone = project.deleted().unwrap();
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].path, Path::new("backbones/pUC19.gb"));

    let back = project.undelete(&a.id, &me()).unwrap();
    assert_eq!(back.path, Path::new("backbones/pUC19.gb"));
    assert_eq!(back.sha, a.sha);
    let commands: Vec<String> = project.history(&a.id).unwrap().into_iter().map(|e| e.command).collect();
    assert_eq!(commands, vec!["import_plasmid", "delete_plasmid", "restore_plasmid"]);
    assert!(project.deleted().unwrap().is_empty());
}

#[test]
fn a_folder_is_renamed_with_everything_in_it_and_never_onto_another() {
    let (_dir, project) = project();
    let a = project.import(&fixture("pUC19.gb"), "old", None, &me()).unwrap().construct;
    project.create_folder("taken").unwrap();
    assert!(matches!(project.rename_folder("old", "taken"), Err(Error::NameTaken(_))));
    project.rename_folder("old", "new/place").unwrap();
    assert_eq!(project.read(&a.id).unwrap().path, Path::new("new/place/pUC19.gb"));
    assert_eq!(project.history(&a.id).unwrap().len(), 1);
}

#[test]
fn the_stamp_changes_with_every_change_and_only_then() {
    let (_dir, project) = project();
    let empty = project.stamp();
    let a = project.import(&fixture("pUC19.gb"), "", None, &me()).unwrap().construct;
    let imported = project.stamp();
    assert_ne!(imported, empty);
    assert_eq!(project.stamp(), imported);
    project.read(&a.id).unwrap();
    project.list().unwrap();
    assert_eq!(project.stamp(), imported, "looking changes nothing");

    let mut edited = a.construct.clone();
    edited.sequence.push_str("ACGT");
    project.save(&a.id, &a.sha, &edited, &agent(), change("edit")).unwrap();
    let saved = project.stamp();
    assert_ne!(saved, imported);
    project.create_folder("empty").unwrap();
    assert_ne!(project.stamp(), saved);
}

#[test]
fn a_sidecar_is_kept_beside_the_project_and_is_not_a_construct() {
    let (_dir, project) = project();
    assert_eq!(project.read_sidecar("remote").unwrap(), None);

    project.write_sidecar("remote", r#"{"server":"lab"}"#).unwrap();
    assert_eq!(project.read_sidecar("remote").unwrap().as_deref(), Some(r#"{"server":"lab"}"#));
    assert!(project.root().join(".bios/remote.json").is_file());
    project.write_sidecar("remote", r#"{"server":"other"}"#).unwrap();
    let again = Project::open(project.root()).unwrap();
    assert_eq!(again.read_sidecar("remote").unwrap().as_deref(), Some(r#"{"server":"other"}"#));
    assert!(project.list().unwrap().is_empty());
    assert!(project.untracked().unwrap().is_empty());

    // Not JSON, or a name that could leave `.bios/`: refused, and what was there is untouched.
    assert!(matches!(project.write_sidecar("remote", "{half"), Err(Error::Unwritable(_))));
    for name in ["", "../bios", "journal/x", "a.b", "lock/"] {
        assert!(matches!(project.write_sidecar(name, "{}"), Err(Error::Unwritable(_))), "{name}");
        assert!(matches!(project.read_sidecar(name), Err(Error::Unwritable(_))), "{name}");
    }
    assert_eq!(project.read_sidecar("remote").unwrap().as_deref(), Some(r#"{"server":"other"}"#));
}

#[test]
fn an_unchanged_file_is_parsed_once_however_often_it_is_read() {
    let (_dir, root) = downloads();
    let project = Project::open(&root).unwrap();
    let listing = project.list().unwrap();
    assert_eq!(project.parse_count(), 4);
    for l in &listing {
        project.read(&l.id).unwrap();
        project.history(&l.id).unwrap();
    }
    assert_eq!(project.list_with_constructs().unwrap().len(), 4);
    assert!(project.untracked().unwrap().is_empty());
    assert_eq!(project.parse_count(), 4, "reading unchanged files again parsed them again");
}

#[test]
fn a_file_changed_outside_is_read_again_even_at_the_same_size_and_time() {
    let (_dir, root) = downloads();
    let project = Project::open(&root).unwrap();
    let path = root.join("pUC19_map.gb");
    let id = project.list().unwrap().into_iter().find(|l| l.path == Path::new("pUC19_map.gb")).unwrap().id;
    let before = project.read(&id).unwrap();

    // Another program changes one base, keeping the file's length and its modification time.
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let text = fs::read_to_string(&path).unwrap();
    let at = text.find("ORIGIN").unwrap() + text[text.find("ORIGIN").unwrap()..].find(|c: char| "acgt".contains(c)).unwrap();
    let swapped = if &text[at..=at] == "a" { "c" } else { "a" };
    let changed = format!("{}{swapped}{}", &text[..at], &text[at + 1..]);
    assert_eq!(changed.len(), text.len());
    fs::write(&path, &changed).unwrap();
    fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();

    let after = project.read(&id).unwrap();
    assert_ne!(after.sha, before.sha);
    assert_ne!(after.construct.sequence, before.construct.sequence);
}

#[test]
fn a_save_is_what_the_next_read_sees() {
    let (_dir, project) = project();
    let imported = project.import(&fixture("wrap_demo.gb"), "", None, &me()).unwrap().construct;
    let mut edited = imported.construct.clone();
    edited.sequence.push_str("ACGT");
    let saved = project.save(&imported.id, &imported.sha, &edited, &me(), change("four bases")).unwrap();
    let read = project.read(&imported.id).unwrap();
    assert_eq!(read.sha, saved.sha);
    assert_eq!(read.construct.sequence, edited.sequence);
    assert_eq!(project.history(&imported.id).unwrap().len(), 2);
}
