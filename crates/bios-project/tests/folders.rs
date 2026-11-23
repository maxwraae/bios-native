//! What a folder knows about itself: its record, its drawer, and what survives a move.

use std::path::{Path, PathBuf};

use bios_project::{FolderMeta, Project, item_file_name};
use serde_json::{Map, Value, json};

fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, project)
}

fn build_meta(backbone: &str) -> FolderMeta {
    let mut data = Map::new();
    data.insert("backboneConstructId".into(), json!(backbone));
    data.insert("designs".into(), json!([]));
    FolderMeta::new("build", data)
}

#[test]
fn an_ordinary_folder_has_no_record() {
    let (_dir, project) = project();
    project.create_folder("Backbones").unwrap();
    assert_eq!(project.folder_meta("Backbones").unwrap(), None);
    assert!(project.folder_metas().unwrap().is_empty());
}

#[test]
fn a_record_is_written_flat_and_read_back() {
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    let meta = build_meta("bb-1");
    project.write_folder_meta("wrap", &meta).unwrap();

    let back = project.folder_meta("wrap").unwrap().unwrap();
    assert_eq!(back.kind, "build");
    assert_eq!(back.v, 1);
    assert_eq!(back.get("backboneConstructId"), &json!("bb-1"));

    // On disk it is the shape the Bios web app writes, so one project opens in both.
    let text = std::fs::read_to_string(project.root().join("wrap/.bios/folder.json")).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], json!("build"));
    assert_eq!(value["v"], json!(1));
    assert!(value["createdAt"].as_str().is_some_and(|at| at.contains('T')));
    assert_eq!(value["backboneConstructId"], json!("bb-1"));
}

#[test]
fn a_record_that_will_not_parse_reads_as_an_ordinary_folder() {
    // A folder whose record is damaged still holds real constructs, so it must draw as a plain
    // folder rather than take the project down with it.
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    std::fs::create_dir_all(project.root().join("wrap/.bios")).unwrap();
    std::fs::write(project.root().join("wrap/.bios/folder.json"), "{ not json").unwrap();
    assert_eq!(project.folder_meta("wrap").unwrap(), None);
}

#[test]
fn the_record_and_the_drawer_move_with_the_folder() {
    // The whole reason this lives inside the folder: a rename is one `rename` and nothing has to
    // be told about it.
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    project.write_folder_meta("wrap", &build_meta("bb-1")).unwrap();
    project.write_folder_file("wrap", "designs/one.json", "{\"v\":1}").unwrap();

    project.rename_folder("wrap", "wrap v2").unwrap();

    assert_eq!(project.folder_meta("wrap").unwrap(), None);
    let back = project.folder_meta("wrap v2").unwrap().unwrap();
    assert_eq!(back.get("backboneConstructId"), &json!("bb-1"));
    assert_eq!(project.folder_file("wrap v2", "designs/one.json").unwrap().as_deref(), Some("{\"v\":1}"));
}

#[test]
fn a_drawer_holds_files_by_name_and_gives_them_back() {
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    assert_eq!(project.folder_file("wrap", "designs/one.json").unwrap(), None);
    assert!(project.folder_files("wrap", "designs").unwrap().is_empty());

    project.write_folder_file("wrap", "designs/one.json", "1").unwrap();
    project.write_folder_file("wrap", "designs/two.json", "2").unwrap();
    assert_eq!(project.folder_files("wrap", "designs").unwrap(), vec!["one.json", "two.json"]);

    project.remove_folder_file("wrap", "designs/one.json").unwrap();
    assert_eq!(project.folder_files("wrap", "designs").unwrap(), vec!["two.json"]);
    // Removing what is not there is not a failure.
    project.remove_folder_file("wrap", "designs/one.json").unwrap();
}

#[test]
fn a_drawer_name_may_not_walk_the_tree() {
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    for bad in ["../../escape.json", "designs/../../escape.json", "a/b/c.json", ".hidden", ""] {
        assert!(project.write_folder_file("wrap", bad, "{}").is_err(), "{bad} was accepted");
    }
}

#[test]
fn the_drawer_is_not_part_of_the_project_a_person_sees() {
    // `.bios` is skipped by every walker, so a record is never mistaken for a construct and a
    // folder's drawer is never drawn as a folder of its own.
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    project.write_folder_meta("wrap", &build_meta("bb-1")).unwrap();
    assert_eq!(project.folders().unwrap(), vec![PathBuf::from("wrap")]);
    assert!(project.list().unwrap().is_empty());
    assert!(project.untracked().unwrap().is_empty());
}

#[test]
fn writing_a_record_changes_the_project_stamp() {
    // The window asks the project twice a second whether anyone wrote to it. Filing a design
    // changes a folder's record and nothing else, and it has to be noticed.
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    let before = project.stamp();
    project.write_folder_meta("wrap", &build_meta("bb-1")).unwrap();
    assert_ne!(project.stamp(), before);
}

#[test]
fn an_item_is_named_after_itself_and_kept_honest_by_a_digest() {
    // Two names that clean up to the same string must not land on one file: the second would look
    // like it had been written when what was there was the first.
    let a = item_file_name("designs", "pAAV (v2): round 3", "design");
    let b = item_file_name("designs", "pAAV _v2__ round 3", "design");
    assert!(a.starts_with("designs/pAAV__v2___round_3-"), "{a}");
    assert_ne!(a, b);
    // Deterministic, so writing the same item again replaces its own file and nothing else.
    assert_eq!(a, item_file_name("designs", "pAAV (v2): round 3", "design"));
    // A name with nothing usable in it still gives a name.
    assert!(item_file_name("designs", "…", "design").starts_with("designs/design-"));
    // And whatever the name, what comes out is a name the drawer will take.
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    project.write_folder_file("wrap", &a, "{}").unwrap();
    assert_eq!(project.folder_files("wrap", "designs").unwrap().len(), 1);
}

#[test]
fn every_typed_folder_is_listed_with_its_record() {
    let (_dir, project) = project();
    project.create_folder("one").unwrap();
    project.create_folder("two").unwrap();
    project.create_folder("plain").unwrap();
    project.write_folder_meta("one", &build_meta("bb-1")).unwrap();
    project.write_folder_meta("two", &FolderMeta::new("verification", Map::new())).unwrap();

    let metas = project.folder_metas().unwrap();
    let names: Vec<(String, String)> = metas.iter().map(|(path, meta)| (path.display().to_string(), meta.kind.clone())).collect();
    assert_eq!(names, vec![("one".into(), "build".into()), ("two".into(), "verification".into())]);
}

#[test]
fn a_record_may_be_changed_in_place() {
    let (_dir, project) = project();
    project.create_folder("wrap").unwrap();
    project.write_folder_meta("wrap", &build_meta("bb-1")).unwrap();
    let mut meta = project.folder_meta("wrap").unwrap().unwrap();
    meta.set("opening", json!({ "kind": "enzymes", "a": "EcoRI", "b": "BamHI", "arc": "between" }));
    project.write_folder_meta("wrap", &meta).unwrap();

    let back = project.folder_meta("wrap").unwrap().unwrap();
    assert_eq!(back.created_at, meta.created_at, "a folder is made once");
    assert_eq!(back.get("opening")["a"], json!("EcoRI"));
    assert_eq!(back.get("backboneConstructId"), &json!("bb-1"));
}

#[test]
fn a_record_may_be_written_for_a_folder_that_is_not_there_yet() {
    let (_dir, project) = project();
    project.write_folder_meta("fresh", &build_meta("bb-1")).unwrap();
    assert!(Path::new(&project.root().join("fresh")).is_dir());
    assert!(project.folder_meta("fresh").unwrap().is_some());
}

// ── naming a folder a feature is about to make ──────────────────────────────────────────────
// The web's `layout/folderNaming.ts` and the cases `build/buildFolder.test.ts` proves on it.

#[test]
fn a_folder_name_strips_what_a_file_system_refuses_rather_than_refusing_the_plasmid() {
    // Every one of these characters is ordinary in a plasmid name. Refusing would mean telling
    // somebody they cannot build from this file.
    assert_eq!(bios_project::folder_name_for("pUC19 (v2): round 3", "build"), "pUC19 (v2) round 3");
    assert_eq!(bios_project::folder_name_for("a/b\\c*d?e", "build"), "a b c d e");
}

#[test]
fn a_folder_name_never_starts_or_ends_with_a_dot_or_a_space() {
    // Windows eats trailing dots and spaces silently, so the name asked for is not the name got —
    // and a name on disk is an address, not a label.
    assert_eq!(bios_project::folder_name_for("  .pAAV.  ", "build"), "pAAV");
    assert_eq!(bios_project::folder_name_for("pAAV...", "build"), "pAAV");
}

#[test]
fn a_folder_name_falls_back_rather_than_being_empty_and_is_capped() {
    assert_eq!(bios_project::folder_name_for("...", "build"), "build");
    assert_eq!(bios_project::folder_name_for("///", "build"), "build");
    assert!(bios_project::folder_name_for(&"x".repeat(200), "build").chars().count() <= 64);
}

#[test]
fn the_stamp_spells_the_month_rather_than_localising_it() {
    // A localised month would land the same construct in two folders that mean the same thing.
    let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
    assert_eq!(bios_project::folder_candidates("wrap", &stamp(day)), {
        let mut want = vec!["wrap".to_string(), "wrap 3 Sep".to_string()];
        want.extend((2..=50).map(|n| format!("wrap 3 Sep ({n})")));
        want
    });
    assert_eq!(stamp(chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()), "31 Jan");
}

/// The web's `folderStamp(new Date(...))`; the crate's own takes today.
fn stamp(day: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", day.day(), MONTHS[day.month0() as usize])
}

fn pick(folders: &[&str], metas: Vec<(&str, FolderMeta)>, bases: &str) -> bios_project::FolderPick {
    let folders: Vec<String> = folders.iter().map(|f| (*f).to_string()).collect();
    let bases = bases.to_string();
    bios_project::pick_folder(
        "wrap",
        "3 Sep",
        &folders,
        |name| metas.iter().find(|(f, _)| *f == name).map(|(_, m)| m.clone()),
        |meta| meta.kind == "build",
        |meta| {
            // A build folder is its BACKBONE: the bases decide, never the name or the id.
            let holds = meta.get("backboneBases").as_str().map(str::to_uppercase);
            holds.filter(|held| *held == bases.to_uppercase()).map(|_| meta.get("backboneConstructId").as_str().unwrap_or_default().to_string())
        },
    )
}

fn build_with(backbone: &str, bases: &str) -> FolderMeta {
    let mut data = Map::new();
    data.insert("backboneConstructId".into(), json!(backbone));
    data.insert("backboneBases".into(), json!(bases));
    FolderMeta::new("build", data)
}

#[test]
fn a_fresh_name_is_taken_when_nothing_is_there() {
    assert_eq!(pick(&[], vec![], "ACGTACGTAA"), bios_project::FolderPick { folder: "wrap".into(), reuse: None });
}

#[test]
fn a_folder_of_ours_holding_exactly_this_is_reused() {
    // The whole point: design again against the same plasmid and it joins the folder, so the two
    // designs sit together and are comparable.
    let got = pick(&["wrap"], vec![("wrap", build_with("bb-1", "ACGTACGTAA"))], "acgtacgtaa");
    assert_eq!(got, bios_project::FolderPick { folder: "wrap".into(), reuse: Some("bb-1".into()) });
}

#[test]
fn a_folder_somebody_made_by_hand_is_never_stamped() {
    // A plain folder called `wrap` is somebody's organisation.
    assert_eq!(pick(&["wrap"], vec![], "ACGTACGTAA").folder, "wrap 3 Sep");
    // And another feature's folder of the same name is left alone too.
    let other = FolderMeta::new("verification", Map::new());
    assert_eq!(pick(&["wrap"], vec![("wrap", other)], "ACGTACGTAA").folder, "wrap 3 Sep");
}

#[test]
fn our_own_folder_whose_contents_do_not_match_is_skipped_rather_than_claimed() {
    let metas = vec![("wrap", build_with("bb-1", "ACGTACGTAA")), ("wrap 3 Sep", build_with("bb-1", "ACGTACGTAA"))];
    let got = pick(&["wrap", "wrap 3 Sep"], metas, "TTTTTTTTTT");
    assert_eq!(got, bios_project::FolderPick { folder: "wrap 3 Sep (2)".into(), reuse: None });
}
