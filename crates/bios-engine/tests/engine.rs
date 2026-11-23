//! The engine through its one door: `Session::execute`.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

fn session(actor: Actor) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, Session::new(project, Who { actor, source: "test".into(), session: None }))
}

fn with_puc19(actor: Actor) -> (tempfile::TempDir, Session, String) {
    let (dir, s) = session(actor);
    let imported = s.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb"), "folder": "backbones" })).unwrap();
    let version = imported["version"].as_str().unwrap().to_string();
    (dir, s, version)
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn a_call_is_checked_before_it_runs() {
    let (_dir, s, _) = with_puc19(Actor::Human);
    let no_such = s.execute("explode", &json!({})).unwrap_err();
    assert!(no_such.0.contains("no command called explode"), "{no_such}");
    let misspelt = s.execute("read_plasmid", &json!({ "plasmd": "pUC19" })).unwrap_err();
    assert!(misspelt.0.contains("no parameter called plasmd"), "{misspelt}");
    let missing = s.execute("read_plasmid", &json!({})).unwrap_err();
    assert!(missing.0.contains("needs plasmid"), "{missing}");
    let wrong = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": "one" })).unwrap_err();
    assert!(wrong.0.contains("start must be a whole number"), "{wrong}");
}

#[test]
fn a_plasmid_is_found_by_path_name_or_id() {
    let (_dir, s, _) = with_puc19(Actor::Human);
    let by_path = s.execute("read_plasmid", &json!({ "plasmid": "backbones/pUC19", "genbank": false })).unwrap();
    let by_name = s.execute("read_plasmid", &json!({ "plasmid": "puc19", "genbank": false })).unwrap();
    let by_id = s.execute("read_plasmid", &json!({ "plasmid": by_path["id"], "genbank": false })).unwrap();
    assert_eq!(by_path, by_name);
    assert_eq!(by_path, by_id);
    assert_eq!(by_path["length"], 2686);
    assert_eq!(by_path["topology"], "circular");
    assert_eq!(by_path["features"].as_array().unwrap().len(), 11);
    assert_eq!(by_path["features"][1]["name"], "lacZ alpha");
    assert_eq!(by_path["features"][1]["start"], 146);
    assert_eq!(by_path["features"][1]["end"], 469);
    let lost = s.execute("read_plasmid", &json!({ "plasmid": "pBR322" })).unwrap_err();
    assert!(lost.0.contains("Plasmids here: backbones/pUC19"), "{lost}");
}

#[test]
fn bases_come_back_one_based_and_inclusive() {
    let (_dir, s, _) = with_puc19(Actor::Human);
    let whole = s.execute("get_sequence", &json!({ "plasmid": "pUC19" })).unwrap();
    let all = text(&whole["sequence"]);
    let piece = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 235, "end": 244 })).unwrap();
    assert_eq!(text(&piece["sequence"]), &all[234..244]);
    assert_eq!(piece["length"], 10);
    let reverse = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 235, "end": 244, "strand": "reverse" })).unwrap();
    assert_eq!(text(&reverse["sequence"]), bios_core::seq::revcomp(&all[234..244]));
    // Through the origin of a circular plasmid: the end, then the start.
    let wrap = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 2681, "end": 4 })).unwrap();
    assert_eq!(text(&wrap["sequence"]), format!("{}{}", &all[2680..], &all[..4]));
    s.execute("create_plasmid", &json!({ "name": "line", "sequence": "ACGTACGTACGT", "circular": false })).unwrap();
    let linear = s.execute("get_sequence", &json!({ "plasmid": "line", "start": 10, "end": 2 })).unwrap_err();
    assert!(linear.0.contains("circular"), "{linear}");
}

#[test]
fn an_edit_that_depends_on_positions_needs_the_version_it_was_made_against() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    let insert = |v: Option<&str>| s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "position": 100, "sequence": "GAATTC", "version": v }));

    let without = insert(None).unwrap_err();
    assert!(without.0.contains("needs version"), "{without}");

    let done = insert(Some(&version)).unwrap();
    assert_eq!(done["saved"], true);
    assert_eq!(done["previous_version"], version);
    assert!(text(&done["version"]).starts_with("2:"));

    let stale = insert(Some(&version)).unwrap_err();
    assert!(stale.0.starts_with("CONFLICT: nothing was saved"), "{stale}");

    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19" })).unwrap();
    let last = history["history"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["command"], "insert_sequence");
    assert_eq!(last["by"], "agent");
    assert_eq!(last["summary"], "Inserted 6 bases after base 100");
}

#[test]
fn an_edit_that_does_not_depend_on_positions_is_made_on_the_newest_version() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "position": 100, "sequence": "GAATTC", "version": version })).unwrap();
    // The agent still holds the old version; a rename by name does not care.
    let renamed =
        s.execute("rename_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ alpha", "new_name": "lacZ", "version": version })).unwrap();
    assert_eq!(renamed["saved"], true);
    assert!(text(&renamed["version"]).starts_with("3:"));
    let again = s.execute("rename_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ", "new_name": "lacZ" })).unwrap();
    assert_eq!(again["unchanged"], true);
}

#[test]
fn an_agents_change_can_be_undone_and_the_undo_is_in_the_history() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    s.execute("remove_feature", &json!({ "plasmid": "pUC19", "feature": "#1", "version": version })).unwrap();
    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(after["features"].as_array().unwrap().len(), 10);

    let back = s.execute("restore_version", &json!({ "plasmid": "pUC19", "to_version": 1 })).unwrap();
    assert_eq!(back["saved"], true);
    // A new version number, holding exactly the text of version 1.
    assert_eq!(text(&back["version"]), version.replacen("1:", "3:", 1));

    let restored = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(restored["features"].as_array().unwrap().len(), 11);
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19" })).unwrap();
    let commands: Vec<&str> = history["history"].as_array().unwrap().iter().map(|e| text(&e["command"])).collect();
    assert_eq!(commands, vec!["import_plasmid", "remove_feature", "restore_version"]);
}

#[test]
fn a_variant_remembers_its_parent_and_a_rename_keeps_the_history() {
    let (_dir, s, _) = with_puc19(Actor::Human);
    let made = s.execute("duplicate_plasmid", &json!({ "plasmid": "pUC19", "name": "pUC19 v2" })).unwrap();
    assert_eq!(made["made_from"], "backbones/pUC19");

    let renamed = s.execute("rename_plasmid", &json!({ "plasmid": "pUC19 v2", "new_name": "pUC19 v2 EGFP", "version": made["version"] })).unwrap();
    assert_eq!(renamed["plasmid"], "backbones/pUC19 v2 EGFP");

    let list = s.execute("list_plasmids", &json!({})).unwrap();
    let rows = list["plasmids"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let child = rows.iter().find(|r| r["name"] == "pUC19 v2 EGFP").unwrap();
    assert_eq!(child["made_from"], "backbones/pUC19");
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19 v2 EGFP" })).unwrap();
    assert_eq!(history["history"].as_array().unwrap().len(), 2);
}

#[test]
fn importing_twice_never_makes_a_second_copy() {
    let (_dir, s, _) = with_puc19(Actor::Human);
    let again = s.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb"), "folder": "copies", "name": "pUC19 final FINAL" })).unwrap();
    assert_eq!(again["plasmid"], "backbones/pUC19");
    assert_eq!(again["already_here"], true);
    assert_eq!(s.execute("list_plasmids", &json!({})).unwrap()["plasmids"].as_array().unwrap().len(), 1);
}

fn feature<'a>(read: &'a Value, name: &str) -> &'a Value {
    read["features"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap()
}

#[test]
fn inserted_bases_push_later_features_along_and_leave_earlier_ones() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    let before = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();

    // Base 100 is before every feature but the whole-plasmid source: they all move six to the right.
    let done = s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "version": version, "position": 100, "sequence": "gaa ttc" })).unwrap();
    assert_eq!(done["saved"], true);

    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(after["length"], 2692);
    assert_eq!(feature(&after, "lacZ alpha")["start"], 152);
    assert_eq!(feature(&after, "lacZ alpha")["end"], 475);
    assert_eq!(after["features"].as_array().unwrap().len(), before["features"].as_array().unwrap().len());
    let placed = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 101, "end": 106 })).unwrap();
    assert_eq!(placed["sequence"], "GAATTC");
}

#[test]
fn a_deletion_removes_what_it_covers_and_trims_what_it_cuts() {
    let (_dir, s, version) = with_puc19(Actor::Human);
    // The lac operator is 489..505 and the lac promoter 513..543: take the operator and the promoter's first 18 bases.
    let done = s.execute("delete_range", &json!({ "plasmid": "pUC19", "version": version, "start": 489, "end": 530 })).unwrap();
    let notes: Vec<&str> = done["changes"].as_array().unwrap().iter().map(text).collect();
    assert!(notes.iter().any(|n| n.starts_with("removed \"lac operator\" (489..505)")), "{notes:?}");
    assert!(notes.iter().any(|n| n.starts_with("\"lac promoter\" lost 18 of its bases; it is now 489..501")), "{notes:?}");

    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(after["length"], 2686 - 42);
    assert!(after["features"].as_array().unwrap().iter().all(|f| f["name"] != "lac operator"));
    let whole = s.execute("delete_range", &json!({ "plasmid": "pUC19", "version": done["version"], "start": 1, "end": 2686 - 42 })).unwrap_err();
    assert!(whole.0.contains("whole plasmid"), "{whole}");
}

#[test]
fn a_replacement_checks_what_it_expects_to_find() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    let there = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 300, "end": 305 })).unwrap();
    let wrong = s.execute(
        "replace_range",
        &json!({ "plasmid": "pUC19", "version": version, "start": 300, "end": 305, "sequence": "GAATTC", "expected_sequence": "NNNNNN" }),
    );
    assert!(wrong.unwrap_err().0.contains("expected_sequence does not match"));

    let done = s
        .execute("replace_range", &json!({ "plasmid": "pUC19", "version": version, "start": 300, "end": 305, "sequence": "GAATTC", "expected_sequence": there["sequence"] }))
        .unwrap();
    assert_eq!(done["saved"], true);
    // Same length in as out: nothing moves, so nothing is reported.
    assert!(done.get("changes").is_none(), "{done}");
    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(feature(&after, "lacZ alpha")["start"], 146);
    assert_eq!(s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 300, "end": 305 })).unwrap()["sequence"], "GAATTC");
}

#[test]
fn an_added_feature_is_written_to_the_file_and_can_be_found_by_its_id() {
    let (_dir, s, version) = with_puc19(Actor::Agent);
    let done = s
        .execute("add_feature", &json!({ "plasmid": "pUC19", "version": version, "name": "EcoRI site", "start": 101, "end": 106, "strand": "reverse", "qualifiers": { "note": "added by test" } }))
        .unwrap();
    let added = &done["feature"];
    assert_eq!(added["name"], "EcoRI site");
    assert_eq!(added["location"], "complement(101..106)");
    assert_eq!(added["type"], "misc_feature");
    let id = text(&added["bios_id"]).to_string();

    let renamed =
        s.execute("rename_feature", &json!({ "plasmid": "pUC19", "version": done["version"], "feature": id, "new_name": "EcoRI" })).unwrap();
    assert_eq!(renamed["saved"], true);
    let read = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(feature(&read, "EcoRI")["bios_id"], id);
    assert!(text(&read["genbank"]).contains("/label=\"EcoRI\""));
    assert!(text(&read["genbank"]).contains("/note=\"added by test\""));

    let outside = s.execute("add_feature", &json!({ "plasmid": "pUC19", "version": renamed["version"], "name": "x", "start": 2600, "end": 3000 }));
    assert!(outside.unwrap_err().0.contains("within 1..2686"));
}

#[test]
fn a_circular_plasmid_with_a_feature_through_the_origin_stays_circular() {
    let (_dir, s) = session(Actor::Human);
    let imported = s.execute("import_plasmid", &json!({ "file": fixture("wrap_demo.gb") })).unwrap();
    let refused = s.execute("set_topology", &json!({ "plasmid": "wrap_demo", "version": imported["version"], "circular": false }));
    assert!(refused.unwrap_err().0.contains("AmpR"));
    // And importing it twice still gives one plasmid, though the file writes its wrap differently.
    let again = s.execute("import_plasmid", &json!({ "file": fixture("wrap_demo.gb") })).unwrap();
    assert_eq!(again["already_here"], true);
    let wrapped = s.execute("get_sequence", &json!({ "plasmid": "wrap_demo", "start": 799, "end": 2 })).unwrap();
    assert_eq!(wrapped["length"], 4);
}

#[test]
fn a_plain_folder_is_worked_in_through_the_commands_and_becomes_a_project_on_the_first_edit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Downloads");
    std::fs::create_dir(&root).unwrap();
    std::fs::copy(fixture("wrap_demo.gb"), root.join("wrap_demo.gb")).unwrap();
    std::fs::copy(fixture("pUC19.gb"), root.join("pUC19.gb")).unwrap();
    let s = Session::new(Project::open(&root).unwrap(), Who { actor: Actor::Agent, source: "test".into(), session: None });

    let listed = s.execute("list_plasmids", &json!({})).unwrap();
    assert_eq!(listed.to_string().matches("\"path\"").count(), 2, "{listed}");
    let read = s.execute("read_plasmid", &json!({ "plasmid": "pUC19", "genbank": false })).unwrap();
    assert_eq!(read["length"], 2686);
    assert!(!root.join("bios.json").exists() && !root.join(".bios").exists(), "reading wrote into the folder");

    let version = text(&read["version"]).to_string();
    s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "position": 0, "sequence": "GGGG", "version": version })).unwrap();
    assert!(Project::is_project(&root));
    assert_eq!(std::fs::read(root.join("wrap_demo.gb")).unwrap(), std::fs::read(fixture("wrap_demo.gb")).unwrap());
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(history.to_string().matches("\"version\"").count(), 2, "{history}");
}
