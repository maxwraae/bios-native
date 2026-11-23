//! The rest of the command set, each through `Session::execute`.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None }))
}

/// A session holding `file`, and the version it was imported at.
fn with(file: &str) -> (tempfile::TempDir, Session, String) {
    let (dir, s) = session();
    let imported = s.execute("import_plasmid", &json!({ "file": fixture(file) })).unwrap();
    let version = text(&imported["version"]).to_string();
    (dir, s, version)
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn read(s: &Session, plasmid: &str) -> Value {
    s.execute("read_plasmid", &json!({ "plasmid": plasmid, "genbank": false })).unwrap()
}

fn bases(s: &Session, plasmid: &str, start: i64, end: i64) -> String {
    text(&s.execute("get_sequence", &json!({ "plasmid": plasmid, "start": start, "end": end })).unwrap()["sequence"]).to_string()
}

fn feature<'a>(read: &'a Value, name: &str) -> &'a Value {
    read["features"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no feature {name}"))
}

/// What `read_plasmid` says about the features, without the version.
fn features(s: &Session, plasmid: &str) -> Value {
    read(s, plasmid)["features"].clone()
}

#[test]
fn reverse_complementing_twice_gives_the_plasmid_back() {
    let (_dir, s, version) = with("pUC19.gb");
    let (before, all) = (features(&s, "pUC19"), bases(&s, "pUC19", 1, 2686));

    let once = s.execute("reverse_complement", &json!({ "plasmid": "pUC19", "version": version })).unwrap();
    assert_eq!(bases(&s, "pUC19", 1, 2686), bios_core::seq::revcomp(&all));
    // complement(146..469) is 2218..2541 on the other strand: 2686 - 469 + 1 and 2686 - 146 + 1.
    let flipped = read(&s, "pUC19");
    let lacz = feature(&flipped, "lacZ alpha");
    assert_eq!((lacz["start"].as_i64(), lacz["end"].as_i64(), text(&lacz["strand"])), (Some(2218), Some(2541), "forward"));

    s.execute("reverse_complement", &json!({ "plasmid": "pUC19", "version": once["version"] })).unwrap();
    assert_eq!(bases(&s, "pUC19", 1, 2686), all);
    assert_eq!(features(&s, "pUC19"), before);
}

#[test]
fn reverse_complementing_a_region_flips_what_is_inside_and_reports_what_crosses() {
    let (_dir, s, version) = with("pUC19.gb");
    let region = bases(&s, "pUC19", 146, 469);
    let done = s.execute("reverse_complement", &json!({ "plasmid": "pUC19", "version": version, "start": 146, "end": 469 })).unwrap();

    assert_eq!(bases(&s, "pUC19", 146, 469), bios_core::seq::revcomp(&region));
    let after = read(&s, "pUC19");
    let lacz = feature(&after, "lacZ alpha");
    assert_eq!((lacz["start"].as_i64(), lacz["end"].as_i64(), text(&lacz["strand"])), (Some(146), Some(469), "forward"));
    assert_eq!(feature(&after, "ori")["strand"], "reverse");
    let notes: Vec<&str> = done["changes"].as_array().unwrap().iter().map(text).collect();
    assert!(notes.iter().any(|n| n.starts_with("flipped with the region:") && n.contains("lacZ alpha")), "{notes:?}");
    // The whole-plasmid `source` feature (named for its organism) cannot flip with a part of itself,
    // nor can M13 rev, complement(465..481), which hangs over the region's end.
    assert!(notes.iter().any(|n| n.starts_with("cross the edge") && n.contains("synthetic DNA construct") && n.contains("M13 rev")), "{notes:?}");
}

#[test]
fn moving_the_origin_and_back_changes_nothing() {
    let (_dir, s, version) = with("wrap_demo.gb");
    let (before, all) = (features(&s, "wrap_demo"), bases(&s, "wrap_demo", 1, 800));
    assert_eq!(feature(&read(&s, "wrap_demo"), "AmpR")["location"], "join(750..800,1..100)");

    let moved = s.execute("set_origin", &json!({ "plasmid": "wrap_demo", "version": version, "position": 750 })).unwrap();
    assert_eq!(bases(&s, "wrap_demo", 1, 800), format!("{}{}", &all[749..], &all[..749]));
    // AmpR started at 750, so it now starts the plasmid and no longer runs through the origin.
    assert_eq!(feature(&read(&s, "wrap_demo"), "AmpR")["location"], "1..151");

    // The old base 1 is 51 bases after the old base 750, so it is base 52 now.
    s.execute("set_origin", &json!({ "plasmid": "wrap_demo", "version": moved["version"], "position": 52 })).unwrap();
    assert_eq!(bases(&s, "wrap_demo", 1, 800), all);
    assert_eq!(features(&s, "wrap_demo"), before);
}

#[test]
fn a_linear_plasmid_has_no_origin_to_move() {
    let (_dir, s) = session();
    let made = s.execute("create_plasmid", &json!({ "name": "line", "sequence": "ACGT".repeat(50), "circular": false })).unwrap();
    let refused = s.execute("set_origin", &json!({ "plasmid": "line", "version": made["version"], "position": 100 }));
    assert!(refused.unwrap_err().0.contains("only a circular plasmid"));
}

#[test]
fn a_batch_is_one_step_and_all_or_nothing() {
    let (_dir, s, version) = with("pUC19.gb");
    let bad = s.execute(
        "batch_edit",
        &json!({ "plasmid": "pUC19", "version": version, "operations": [
            { "op": "delete_range", "start": 900, "end": 950 },
            { "op": "insert_sequence", "position": 99999, "sequence": "GAATTC" },
        ] }),
    );
    let message = bad.unwrap_err().0;
    assert!(message.starts_with("operation 2 (insert_sequence) failed:"), "{message}");
    assert!(message.ends_with("Nothing was saved: a batch is all-or-nothing."), "{message}");
    assert_eq!(read(&s, "pUC19")["length"], 2686);

    let done = s
        .execute(
            "batch_edit",
            &json!({ "plasmid": "pUC19", "version": version, "operations": [
                { "op": "delete_range", "start": 900, "end": 950 },
                { "op": "insert_sequence", "position": 120, "sequence": "GAATTC" },
                { "op": "add_feature", "name": "EcoRI site", "start": 121, "end": 126 },
            ] }),
        )
        .unwrap();
    assert_eq!(done["operations"], 3);
    let after = read(&s, "pUC19");
    assert_eq!(after["length"], 2686 - 51 + 6);
    assert_eq!(feature(&after, "EcoRI site")["start"], 121);
    assert_eq!(bases(&s, "pUC19", 121, 126), "GAATTC");
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(history["history"].as_array().unwrap().len(), 2);

    let nested = s.execute("batch_edit", &json!({ "plasmid": "pUC19", "version": done["version"], "operations": [{ "op": "read_plasmid" }] }));
    assert!(nested.unwrap_err().0.contains("is not an editing command"));
}

#[test]
fn undo_takes_back_the_last_change_to_the_text() {
    let (_dir, s, version) = with("pUC19.gb");
    let nothing = s.execute("undo_last_edit", &json!({ "plasmid": "pUC19" }));
    assert!(nothing.unwrap_err().0.contains("no edit of this plasmid to undo"));

    s.execute("insert_sequence", &json!({ "plasmid": "pUC19", "version": version, "position": 100, "sequence": "GAATTC" })).unwrap();
    // A move is a step in the history but not a change to undo.
    s.execute("move_plasmid", &json!({ "plasmid": "pUC19", "folder": "backbones" })).unwrap();

    let undone = s.execute("undo_last_edit", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(undone["undone"], "insert_sequence");
    assert_eq!(undone["made_by"], "agent");
    assert_eq!(undone["plasmid"], "backbones/pUC19");
    assert_eq!(read(&s, "pUC19")["length"], 2686);
}

#[test]
fn a_feature_is_changed_without_a_version_but_moved_only_with_one() {
    let (_dir, s, version) = with("pUC19.gb");
    let changed = s
        .execute("update_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ alpha", "name": "lacZ", "category": "promoter", "why": "drives  the reporter", "color": "#b7e6d7", "qualifiers": { "gene": "lacZ" } }))
        .unwrap();
    assert_eq!(changed["before"]["name"], "lacZ alpha");
    assert_eq!(changed["after"]["name"], "lacZ");
    let genbank = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    for line in ["/note=\"bios-category: promoter\"", "/note=\"bios-use: drives the reporter\"", "/color=\"#b7e6d7\"", "/gene=\"lacZ\""] {
        assert!(text(&genbank["genbank"]).contains(line), "{line}");
    }

    let moved = s.execute("update_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ", "start": 150, "end": 460 }));
    assert!(moved.unwrap_err().0.contains("version is required"));
    let stale = s.execute("update_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ", "start": 150, "end": 460, "version": version }));
    assert!(stale.unwrap_err().0.starts_with("CONFLICT"));
    let moved = s
        .execute("update_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ", "start": 150, "end": 460, "version": changed["version"] }))
        .unwrap();
    assert_eq!(moved["after"]["location"], "complement(150..460)");

    // An empty value takes a fact or a qualifier away again.
    s.execute("update_feature", &json!({ "plasmid": "pUC19", "feature": "lacZ", "why": "", "qualifiers": { "gene": "" } })).unwrap();
    let genbank = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    assert!(!text(&genbank["genbank"]).contains("bios-use"));
    assert!(!text(&genbank["genbank"]).contains("/gene=\"lacZ\""));
    assert!(text(&genbank["genbank"]).contains("bios-category: promoter"));
}

#[test]
fn what_a_feature_acts_on_is_written_on_it_and_read_back_from_both_ends() {
    let (_dir, s, _) = with("pUC19.gb");
    let said = s
        .execute(
            "set_feature_actions",
            &json!({ "plasmid": "pUC19", "feature": "lac promoter", "actions": [
                { "verb": " boosts ", "targets": ["lac operator", "#2"] },
                { "verb": "serves", "targets": ["construct"] },
                { "verb": "", "targets": ["#4"] },
            ] }),
        )
        .unwrap();
    assert_eq!(
        said["actions"],
        json!([{ "verb": "boosts", "targets": ["lac operator", "lacZ alpha"] }, { "verb": "serves", "targets": ["this construct"] }])
    );

    let now = read(&s, "pUC19");
    let operator = feature(&now, "lac operator");
    let lacz = feature(&now, "lacZ alpha");
    // The targets were given ids, and the statement names them by id on the actor.
    let (p, t) = (text(&operator["bios_id"]), text(&lacz["bios_id"]));
    let genbank = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    // Long qualifiers wrap in the file; the words are what matter.
    let flat: String = text(&genbank["genbank"]).split_whitespace().collect();
    for line in [
        format!("/note=\"bios-act: boosts > {p}, {t}\""),
        "/note=\"bios-act: serves > construct\"".into(),
        format!("/note=\"bios-for: {p}\""),
        "/note=\"bios-use-source: human\"".into(),
    ] {
        assert!(flat.contains(&line.split_whitespace().collect::<String>()), "{line}");
    }
    assert_eq!(feature(&now, "lac promoter")["acts_on"][0], json!({ "verb": "boosts", "targets": ["lac operator", "lacZ alpha"] }));
    // The qualifiers Bios keeps for itself are not shown as the feature's own.
    assert!(feature(&now, "lac promoter")["qualifiers"].as_array().is_none_or(|q| q.iter().all(|q| !text(q).contains("bios-"))));

    // A feature nobody has heard of is refused; an empty list takes every statement away.
    assert!(
        s.execute(
            "set_feature_actions",
            &json!({ "plasmid": "pUC19", "feature": "lac promoter", "actions": [{ "verb": "x", "targets": ["nothing"] }] })
        )
        .is_err()
    );
    s.execute("set_feature_actions", &json!({ "plasmid": "pUC19", "feature": "lac promoter", "actions": [] })).unwrap();
    let genbank = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    assert!(!text(&genbank["genbank"]).contains("bios-act:") && !text(&genbank["genbank"]).contains("bios-for:"));
    assert!(feature(&read(&s, "pUC19"), "lac promoter").get("acts_on").is_none());
}

#[test]
fn a_primer_is_placed_where_its_sequence_binds() {
    let (_dir, s, version) = with("pUC19.gb");
    let forward = bases(&s, "pUC19", 300, 319);
    let reverse = bios_core::seq::revcomp(&bases(&s, "pUC19", 400, 419));

    let a = s.execute("add_primer", &json!({ "plasmid": "pUC19", "version": version, "name": "fwd", "sequence": forward })).unwrap();
    assert_eq!(a["primer"]["location"], "300..319");
    assert_eq!(a["primer"]["bases"], forward);
    assert_eq!(a["primer"]["type"], "primer_bind");
    assert_eq!(a["primer"]["binding_sites"], 1);

    let b = s.execute("add_primer", &json!({ "plasmid": "pUC19", "version": a["version"], "name": "rev", "sequence": reverse })).unwrap();
    assert_eq!(b["primer"]["location"], "complement(400..419)");
    assert_eq!(b["primer"]["bases"], reverse);

    let nowhere = s.execute(
        "add_primer",
        &json!({ "plasmid": "pUC19", "version": b["version"], "name": "x", "sequence": "ACGTACGTACGTACGTACGTACGTACGTAAAAAAAAAAAACCCCCCCCGGGGTTTT" }),
    );
    assert!(nowhere.unwrap_err().0.contains("does not bind anywhere"));

    let moved = s.execute("move_primer", &json!({ "plasmid": "pUC19", "version": b["version"], "primer": "fwd", "shift": 5 })).unwrap();
    assert_eq!(moved["after"]["location"], "305..324");
    assert_eq!(moved["after"]["bases"], bases(&s, "pUC19", 305, 324));

    let renamed = s.execute("update_primer", &json!({ "plasmid": "pUC19", "primer": "rev", "name": "rev2" })).unwrap();
    assert_eq!(renamed["after"]["name"], "rev2");
    let not_a_primer = s.execute("remove_primer", &json!({ "plasmid": "pUC19", "primer": "lacZ alpha" }));
    assert!(not_a_primer.unwrap_err().0.contains("not a primer"));
    let removed = s.execute("remove_primer", &json!({ "plasmid": "pUC19", "primer": "rev2" })).unwrap();
    assert_eq!(removed["removed"]["bases"], reverse);
    assert!(read(&s, "pUC19")["features"].as_array().unwrap().iter().all(|f| f["name"] != "rev2"));
}

#[test]
fn a_plasmid_made_from_a_range_brings_its_features_and_remembers_its_parent() {
    let (_dir, s, _) = with("pUC19.gb");
    // The lac operator is 489..505, the lac promoter 513..543 and the CAP site 554..579, all on the bottom strand.
    let made = s
        .execute("create_plasmid", &json!({ "name": "lac cassette", "folder": "parts", "from_plasmid": "pUC19", "from_start": 489, "from_end": 579 }))
        .unwrap();
    assert_eq!(made["created"], "parts/lac cassette");
    assert_eq!(made["length"], 91);
    assert_eq!(made["made_from"], "pUC19");

    let part = read(&s, "lac cassette");
    assert_eq!(part["topology"], "linear");
    assert_eq!(feature(&part, "lac operator")["location"], "complement(1..17)");
    assert_eq!(feature(&part, "CAP binding site")["location"], "complement(66..91)");
    assert_eq!(bases(&s, "lac cassette", 1, 91), bases(&s, "pUC19", 489, 579));
    let list = s.execute("list_plasmids", &json!({})).unwrap();
    let row = list["plasmids"].as_array().unwrap().iter().find(|r| r["name"] == "lac cassette").unwrap();
    assert_eq!(row["made_from"], "pUC19");
    assert_eq!(list["folders"], json!(["parts"]));
}

#[test]
fn a_plasmid_is_made_from_bases_or_from_genbank_text() {
    let (_dir, s) = session();
    let made = s.execute("create_plasmid", &json!({ "name": "mini", "sequence": ">mini\nacgt acgt\nACGT" })).unwrap();
    assert_eq!(made["length"], 12);
    let mini = s.execute("read_plasmid", &json!({ "plasmid": "mini" })).unwrap();
    assert_eq!(mini["topology"], "circular");

    let copy = s.execute("create_plasmid", &json!({ "name": "mini two", "genbank": mini["genbank"] }));
    // The same molecule under another name is still a second construct here: it was asked for by name.
    assert_eq!(copy.unwrap()["created"], "mini two");
    let nothing = s.execute("create_plasmid", &json!({ "name": "empty" }));
    assert!(nothing.unwrap_err().0.contains("give genbank, sequence, or from_plasmid"));
    let taken = s.execute("create_plasmid", &json!({ "name": "mini", "sequence": "TTTT" }));
    assert!(taken.unwrap_err().0.contains("already exists"));
}

#[test]
fn a_deleted_plasmid_needs_a_yes_and_can_come_back() {
    let (_dir, s, _) = with("pUC19.gb");
    let unasked = s.execute("delete_plasmid", &json!({ "plasmid": "pUC19", "confirm": false }));
    assert!(unasked.unwrap_err().0.contains("ask the person first"));

    s.execute("delete_plasmid", &json!({ "plasmid": "pUC19", "confirm": true })).unwrap();
    assert!(s.execute("list_plasmids", &json!({})).unwrap()["plasmids"].as_array().unwrap().is_empty());
    let gone = s.execute("deleted_plasmids", &json!({})).unwrap();
    assert_eq!(gone[0]["name"], "pUC19");

    let back = s.execute("restore_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    assert_eq!(back["restored"], "pUC19");
    assert_eq!(read(&s, "pUC19")["length"], 2686);
    assert!(s.execute("deleted_plasmids", &json!({})).unwrap().as_array().unwrap().is_empty());
}

#[test]
fn an_export_writes_outside_the_project_and_never_over_a_file() {
    let (dir, s, _) = with("pUC19.gb");
    let out = dir.path().join("out/pUC19.gb").display().to_string();
    let done = s.execute("export_plasmid", &json!({ "plasmid": "pUC19", "path": out })).unwrap();
    assert_eq!(done["to"], out);
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("LOCUS       pUC19"));

    let again = s.execute("export_plasmid", &json!({ "plasmid": "pUC19", "path": out }));
    assert!(again.unwrap_err().0.contains("already exists"));
    s.execute("export_plasmid", &json!({ "plasmid": "pUC19", "path": out, "format": "fasta", "overwrite": true })).unwrap();
    let fasta = std::fs::read_to_string(&out).unwrap();
    assert!(fasta.starts_with(">pUC19\n"));
    assert_eq!(fasta.lines().nth(1).unwrap().len(), 60);
    assert_eq!(fasta.lines().skip(1).map(str::len).sum::<usize>(), 2686);

    let inside = s.project().root().join("copy.gb").display().to_string();
    let refused = s.execute("export_plasmid", &json!({ "plasmid": "pUC19", "path": inside }));
    assert!(refused.unwrap_err().0.contains("inside the project"));
}

#[test]
fn the_text_can_be_edited_but_not_the_bases_by_accident() {
    let (_dir, s, version) = with("pUC19.gb");
    let done = s
        .execute(
            "edit_plasmid",
            &json!({ "plasmid": "pUC19", "version": version, "old_string": "/label=\"lacZ alpha\"", "new_string": "/label=\"lacZ\"" }),
        )
        .unwrap();
    assert_eq!(done["replaced"], 1);
    assert_eq!(feature(&read(&s, "pUC19"), "lacZ")["start"], 146);

    let missing =
        s.execute("edit_plasmid", &json!({ "plasmid": "pUC19", "version": done["version"], "old_string": "no such text", "new_string": "x" }));
    assert!(missing.unwrap_err().0.contains("was not found"));
    let whole = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap();
    let first_row = text(&whole["genbank"]).lines().find(|l| l.starts_with("        1 ")).unwrap().to_string();
    let changed = first_row.replacen("        1 ", "        1 a", 1);
    let bases_changed =
        s.execute("edit_plasmid", &json!({ "plasmid": "pUC19", "version": done["version"], "old_string": first_row, "new_string": changed }));
    assert!(bases_changed.unwrap_err().0.contains("changes the sequence"));
    assert_eq!(read(&s, "pUC19")["length"], 2686);

    let rewritten = text(&whole["genbank"]).replace("/label=\"lac promoter\"", "/label=\"Plac\"");
    s.execute("write_plasmid", &json!({ "plasmid": "pUC19", "version": done["version"], "genbank": rewritten })).unwrap();
    assert_eq!(feature(&read(&s, "pUC19"), "Plac")["start"], 513);
    let junk = s.execute("write_plasmid", &json!({ "plasmid": "pUC19", "version": "x", "genbank": "not genbank" }));
    assert!(junk.is_err());
}

#[test]
fn a_sequence_is_found_on_either_strand() {
    let (_dir, s, _) = with("pUC19.gb");
    let piece = bases(&s, "pUC19", 300, 329);
    let hit = s.execute("find_sequence", &json!({ "plasmid": "pUC19", "query": piece })).unwrap();
    assert_eq!(hit["total"], 1);
    assert_eq!(hit["hits"][0], json!({ "start": 300, "end": 329, "strand": "forward", "mismatches": 0, "through_origin": false }));

    let other = s.execute("find_sequence", &json!({ "plasmid": "pUC19", "query": bios_core::seq::revcomp(&piece) })).unwrap();
    assert_eq!(other["hits"][0]["strand"], "reverse");
    assert_eq!(other["hits"][0]["start"], 300);

    // One wrong base in the middle is found only when a mismatch is allowed.
    let mut wrong: Vec<char> = piece.chars().collect();
    wrong[15] = if wrong[15] == 'A' { 'C' } else { 'A' };
    let wrong: String = wrong.into_iter().collect();
    assert_eq!(s.execute("find_sequence", &json!({ "plasmid": "pUC19", "query": wrong })).unwrap()["total"], 0);
    let near = s.execute("find_sequence", &json!({ "plasmid": "pUC19", "query": wrong, "max_mismatches": 1 })).unwrap();
    assert_eq!(near["hits"][0]["mismatches"], 1);
}

#[test]
fn restriction_sites_say_where_each_strand_is_cut() {
    let (_dir, s) = session();
    // EcoRI (G^AATTC) at 5..10 and BamHI (G^GATCC) at 27..32.
    s.execute("create_plasmid", &json!({ "name": "sites", "sequence": "AAAAGAATTCAAAAAAAAAAAAAAAAGGATCCAAAA", "circular": false })).unwrap();
    let found = s.execute("find_restriction_sites", &json!({ "plasmid": "sites", "enzymes": ["EcoRI", "BamHI", "HindIII"] })).unwrap();

    let by = |name: &str| found["enzymes"].as_array().unwrap().iter().find(|e| e["enzyme"] == name).cloned().unwrap();
    assert_eq!(by("EcoRI")["site"], "GAATTC");
    assert_eq!(by("EcoRI")["total_cuts_in_plasmid"], 1);
    assert_eq!(
        by("EcoRI")["cuts"][0],
        json!({ "after_base": 5, "bottom_after_base": 9, "site_starts": 5, "strand": "forward", "overhang": "5' AATT" })
    );
    assert_eq!(by("BamHI")["cuts"][0]["after_base"], 27);
    assert_eq!(by("BamHI")["cuts"][0]["overhang"], "5' GATC");
    assert_eq!(found["non_cutters"], json!(["HindIII"]));

    let window = s.execute("find_restriction_sites", &json!({ "plasmid": "sites", "enzymes": ["EcoRI", "BamHI"], "start": 20, "end": 36 })).unwrap();
    assert_eq!(window["enzymes"].as_array().unwrap().len(), 1);
    let unknown = s.execute("find_restriction_sites", &json!({ "plasmid": "sites", "enzymes": ["NotAnEnzyme"] }));
    assert!(unknown.unwrap_err().0.contains("not in the enzyme table: NotAnEnzyme"));
    // The default set runs and reports the two sites that are there.
    let common = s.execute("find_restriction_sites", &json!({ "plasmid": "sites", "max_cuts": 1 })).unwrap();
    let names: Vec<&str> = common["enzymes"].as_array().unwrap().iter().map(|e| text(&e["enzyme"])).collect();
    assert!(names.contains(&"EcoRI") && names.contains(&"BamHI"), "{names:?}");
}

#[test]
fn every_enzyme_is_listed_against_a_plasmid_by_how_often_it_cuts() {
    let (_dir, s) = session();
    // EcoRI (G^AATTC) at 5, SmaI (CCC^GGG, blunt) at 15, BsaI (Type IIS) at 25.
    s.execute("create_plasmid", &json!({ "name": "list", "sequence": "AAAAGAATTCAAAACCCGGGAAAAGGTCTCAAAAAAAAAA", "circular": false })).unwrap();
    let listed = s.execute("list_enzymes", &json!({ "plasmid": "list" })).unwrap();
    let group = |listed: &Value, label: &str| listed["groups"].as_array().unwrap().iter().find(|g| g["group"] == label).cloned();
    let once = group(&listed, "Cuts once").unwrap();
    let eco = once["enzymes"].as_array().unwrap().iter().find(|e| e["enzyme"] == "EcoRI").cloned().unwrap();
    assert_eq!(eco, json!({ "enzyme": "EcoRI", "site": "G^AATTC", "leaves": "5′ AATT", "type2s": false, "matched_alias": null, "site_starts": [5] }));
    assert!(group(&listed, "Doesn't cut").unwrap()["enzymes"].as_array().unwrap().iter().any(|e| e["enzyme"] == "NotI"));
    // A search by an isoschizomer finds the canonical enzyme and says why.
    let found = s.execute("list_enzymes", &json!({ "plasmid": "list", "query": "Eco31I" })).unwrap();
    let bsa = &group(&found, "Cuts once").unwrap()["enzymes"][0];
    assert_eq!(bsa["enzyme"], "BsaI");
    assert_eq!(bsa["matched_alias"], "Eco31I");
    assert_eq!(bsa["leaves"], "cuts outside its site");
    let blunt = s.execute("list_enzymes", &json!({ "plasmid": "list", "filter": "blunt" })).unwrap();
    assert!(!blunt.to_string().contains("\"EcoRI\""));
    assert!(s.execute("list_enzymes", &json!({ "plasmid": "list", "filter": "sticky" })).unwrap_err().0.contains("filter is"));
}

#[test]
fn a_new_feature_can_say_what_it_is_why_it_is_here_and_what_it_serves() {
    let (_dir, s) = session();
    let made = s.execute("create_plasmid", &json!({ "name": "said", "sequence": "ACGT".repeat(20), "circular": false })).unwrap();
    let a = s
        .execute("add_feature", &json!({ "plasmid": "said", "version": made["version"], "name": "GFP", "start": 1, "end": 30, "type": "CDS" }))
        .unwrap();
    let gfp_id = a["feature"]["bios_id"].as_str().unwrap().to_string();
    let b = s
        .execute(
            "add_feature",
            &json!({ "plasmid": "said", "version": a["version"], "name": "tag", "start": 31, "end": 40, "category": "Tag", "why": "so  GFP can be seen on a western", "serves": "GFP" }),
        )
        .unwrap();
    // A why with nothing named serves the construct.
    let c = s
        .execute("add_feature", &json!({ "plasmid": "said", "version": b["version"], "name": "spacer", "start": 41, "end": 50, "why": "room" }))
        .unwrap();
    s.execute("add_feature", &json!({ "plasmid": "said", "version": c["version"], "name": "bare", "start": 51, "end": 60 })).unwrap();
    let id = s.project().list().unwrap().into_iter().find(|l| l.name == "said").unwrap().id;
    let construct = s.project().read(&id).unwrap().construct;
    let notes = |name: &str| -> Vec<String> {
        let f = construct.features.iter().find(|f| f.name == name).unwrap();
        f.note("note").unwrap_or_default().iter().filter(|n| !n.starts_with("bios-id:")).cloned().collect()
    };
    assert_eq!(
        notes("tag"),
        ["bios-category: tag", "bios-use: so GFP can be seen on a western", &format!("bios-for: {gfp_id}"), "bios-use-source: agent"]
    );
    assert_eq!(notes("spacer"), ["bios-use: room", "bios-for: construct", "bios-use-source: agent"]);
    assert!(notes("bare").is_empty(), "only the name has to be filled in");
    let missing = s.execute("add_feature", &json!({ "plasmid": "said", "version": "any", "name": "x", "start": 1, "end": 2, "serves": "nothing" }));
    assert!(missing.is_err());
}

#[test]
fn pasted_bases_bring_the_features_that_lie_inside_them() {
    let (_dir, s, _) = with("pUC19.gb");
    let made = s.execute("create_plasmid", &json!({ "name": "target", "sequence": "ACGTACGTACGTACGTACGT", "circular": false })).unwrap();
    // The lac operator is complement(489..505) (17 bases) and the lac promoter complement(513..543),
    // so 489..530 holds the operator whole and the promoter only in part.
    let pasted = s
        .execute(
            "paste_region",
            &json!({ "plasmid": "target", "version": made["version"], "position": 10, "from_plasmid": "pUC19", "from_start": 489, "from_end": 530 }),
        )
        .unwrap();
    assert_eq!(pasted["pasted_bases"], 42);
    assert!(pasted["pasted_features"].as_array().unwrap().contains(&json!("lac operator")));
    assert!(!pasted["pasted_features"].as_array().unwrap().contains(&json!("lac promoter")));
    let notes: Vec<&str> = pasted["changes"].as_array().unwrap().iter().map(text).collect();
    assert!(notes.iter().any(|n| n.starts_with("\"lac promoter\" is only partly inside")), "{notes:?}");

    let target = read(&s, "target");
    assert_eq!(target["length"], 62);
    assert_eq!(feature(&target, "lac operator")["location"], "complement(11..27)");
    assert_eq!(bases(&s, "target", 11, 52), bases(&s, "pUC19", 489, 530));
    assert_eq!(bases(&s, "target", 1, 10), "ACGTACGTAC");
    // The copy is its own feature, with its own id.
    let source = read(&s, "pUC19");
    assert_ne!(feature(&target, "lac operator").get("bios_id"), None);
    assert_ne!(feature(&target, "lac operator").get("bios_id"), feature(&source, "lac operator").get("bios_id"));

    // Pasted the other way round, the operator lies on the top strand at the mirrored place.
    let flipped = s
        .execute(
            "paste_region",
            &json!({ "plasmid": "target", "version": pasted["version"], "position": 0, "from_plasmid": "pUC19", "from_start": 489, "from_end": 530, "reverse_complement": true }),
        )
        .unwrap();
    assert_eq!(flipped["pasted_bases"], 42);
    assert_eq!(bases(&s, "target", 1, 42), bios_core::seq::revcomp(&bases(&s, "pUC19", 489, 530)));
    let both = read(&s, "target");
    let locations: Vec<&str> =
        both["features"].as_array().unwrap().iter().filter(|f| f["name"] == "lac operator").map(|f| text(&f["location"])).collect();
    // 42 - 17 = 25 bases after the operator come first when flipped, so the operator is 26..42.
    assert_eq!(locations, vec!["26..42", "complement(53..69)"]);
}

#[test]
fn features_are_copied_onto_wherever_their_bases_occur() {
    let (_dir, s, _) = with("pUC19.gb");
    // lacZ alpha is complement(146..469): its top-strand bases read it backwards.
    let lacz = bases(&s, "pUC19", 146, 469);
    let made = s
        .execute(
            "create_plasmid",
            &json!({ "name": "bare", "sequence": format!("TTTTT{}GGGGG{}AAAAA", lacz, bios_core::seq::revcomp(&lacz)), "circular": false }),
        )
        .unwrap();

    let copied = s
        .execute(
            "copy_features",
            &json!({ "plasmid": "bare", "version": made["version"], "from": "pUC19", "features": ["lacZ alpha", "no such feature"] }),
        )
        .unwrap();
    let added = copied["added"].as_array().unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(added[0], json!({ "name": "lacZ alpha", "start": 6, "end": 329, "strand": "reverse", "mismatches": 0 }));
    assert_eq!(added[1], json!({ "name": "lacZ alpha", "start": 335, "end": 658, "strand": "forward", "mismatches": 0 }));
    let notes: Vec<&str> = copied["changes"].as_array().unwrap().iter().map(text).collect();
    assert!(notes.iter().any(|n| n.contains("no such feature")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("matches 2 places")), "{notes:?}");

    // A second time there is nothing new to add.
    let again =
        s.execute("copy_features", &json!({ "plasmid": "bare", "version": copied["version"], "from": "pUC19", "features": ["lacZ alpha"] })).unwrap();
    assert_eq!(again["unchanged"], true);
}

#[test]
fn known_features_are_proposed_first_and_added_only_when_asked() {
    let (_dir, s, _) = with("pUC19.gb");
    let made = s
        .execute("create_plasmid", &json!({ "name": "new build", "folder": "builds", "from_plasmid": "pUC19", "from_start": 100, "from_end": 600 }))
        .unwrap();
    // Start from bare bases: take every feature off the new plasmid.
    let stripped = text(&s.execute("read_plasmid", &json!({ "plasmid": "new build" })).unwrap()["genbank"]).to_string();
    let features_at = stripped.find("FEATURES").unwrap();
    let origin_at = stripped.find("ORIGIN").unwrap();
    let bare = format!("{}FEATURES             Location/Qualifiers\n{}", &stripped[..features_at], &stripped[origin_at..]);
    let written = s.execute("write_plasmid", &json!({ "plasmid": "new build", "version": made["version"], "genbank": bare })).unwrap();
    assert!(read(&s, "new build")["features"].as_array().unwrap().is_empty());

    let dry = s.execute("auto_annotate", &json!({ "plasmid": "new build", "sources": ["all"] })).unwrap();
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["sources_searched"], 1);
    let proposed = dry["proposed"].as_array().unwrap();
    // 100..600 of pUC19 holds lacZ alpha (complement(146..469)) and the lac promoter
    // (complement(513..543)): 47..370 and 414..444 here.
    let lacz = proposed.iter().find(|p| p["name"] == "lacZ alpha").unwrap();
    assert_eq!((lacz["start"].as_i64(), lacz["end"].as_i64(), text(&lacz["strand"])), (Some(47), Some(370), "reverse"));
    assert_eq!(lacz["from"], json!(["pUC19"]));
    let promoter = proposed.iter().find(|p| p["name"] == "lac promoter").unwrap();
    assert_eq!((promoter["start"].as_i64(), promoter["end"].as_i64()), (Some(414), Some(444)));
    assert!(read(&s, "new build")["features"].as_array().unwrap().is_empty(), "a dry run adds nothing");

    let unversioned = s.execute("auto_annotate", &json!({ "plasmid": "new build", "sources": ["all"], "dry_run": false }));
    assert!(unversioned.unwrap_err().0.contains("version is required"));
    let added = s
        .execute("auto_annotate", &json!({ "plasmid": "new build", "sources": ["pUC19"], "dry_run": false, "version": written["version"] }))
        .unwrap();
    assert_eq!(added["added"].as_array().unwrap().len(), proposed.len());
    let after = read(&s, "new build");
    assert_eq!(feature(&after, "lacZ alpha")["location"], "complement(47..370)");

    // Asked again, everything found is already there.
    let again = s.execute("auto_annotate", &json!({ "plasmid": "new build", "sources": ["all"] })).unwrap();
    assert!(again["proposed"].as_array().unwrap().is_empty(), "{}", again["proposed"]);
    assert_eq!(again["already_annotated"].as_array().unwrap().len(), proposed.len());
}

#[test]
fn stats_and_translation_of_bases_given_directly() {
    let (_dir, s) = session();
    let stats = s.execute("sequence_stats", &json!({ "sequence": "GCGC ATAT" })).unwrap();
    assert_eq!(stats["length"], 8);
    assert_eq!(stats["gc_percent"], 50.0);
    assert!(stats["tm_c"].is_number());
    assert!(s.execute("sequence_stats", &json!({ "sequence": "ACGTNACGT" })).unwrap()["tm_c"].is_null());

    // ATG GCC TAA reads Met-Ala-stop; its reverse complement read on the bottom strand is the same.
    let forward = s.execute("translate", &json!({ "sequence": "ATGGCCTAA" })).unwrap();
    assert_eq!(forward["protein"], "MA*");
    assert_eq!(forward["stops"], 1);
    let bottom = s.execute("translate", &json!({ "sequence": "TTAGGCCAT", "frame": -1 })).unwrap();
    assert_eq!(bottom["protein"], "MA*");
    assert_eq!(s.execute("translate", &json!({ "sequence": "CATGGCCTAA", "frame": 2 })).unwrap()["protein"], "MA*");
    assert!(s.execute("translate", &json!({ "sequence": "ATG", "frame": 4 })).unwrap_err().0.contains("frame must be"));
    assert!(s.execute("sequence_stats", &json!({})).unwrap_err().0.contains("give a plasmid"));
}

#[test]
fn the_gc_of_a_whole_plasmid_is_a_percentage() {
    let (_dir, s, _) = with("pUC19.gb");
    let all = bases(&s, "pUC19", 1, 2686);
    let expected = (all.bytes().filter(|b| matches!(b, b'G' | b'C')).count() as f64 / 2686.0 * 1000.0).round() / 10.0;
    assert_eq!(read(&s, "pUC19")["gc_percent"], expected);
    assert!((30.0..70.0).contains(&expected), "{expected}");
    assert_eq!(s.execute("sequence_stats", &json!({ "plasmid": "pUC19" })).unwrap()["gc_percent"], expected);
    assert_eq!(s.execute("sequence_stats", &json!({ "plasmid": "pUC19", "start": 235, "end": 254 })).unwrap()["length"], 20);
}

#[test]
fn an_open_reading_frame_is_found_on_either_strand() {
    let (_dir, s) = session();
    // ATG + 4 codons + TAA at 4..21, then the same frame reverse-complemented at 25..42.
    let orf = "ATGGCCGCCGCCGCCTAA";
    s.execute("create_plasmid", &json!({ "name": "orfs", "sequence": format!("CCC{orf}CCC{}CCC", bios_core::seq::revcomp(orf)), "circular": false }))
        .unwrap();
    let found = s.execute("find_orfs", &json!({ "plasmid": "orfs", "min_aa": 5 })).unwrap();
    assert_eq!(
        found["orfs"],
        json!([
            { "start": 4, "end": 21, "strand": "forward", "aa": 5, "protein": "MAAAA*" },
            { "start": 25, "end": 42, "strand": "reverse", "aa": 5, "protein": "MAAAA*" },
        ])
    );
    assert!(s.execute("find_orfs", &json!({ "plasmid": "orfs", "min_aa": 6 })).unwrap()["orfs"].as_array().unwrap().is_empty());
}

#[test]
fn codon_optimising_keeps_the_protein_and_moves_nothing() {
    let (_dir, s) = session();
    // Met-Ala-Arg-Leu-stop in rare codons, with a feature over it and one after it.
    let made = s.execute("create_plasmid", &json!({ "name": "cds", "sequence": "CCCATGGCACGTTTATAGCCCGGGG", "circular": false })).unwrap();
    let a = s
        .execute("add_feature", &json!({ "plasmid": "cds", "version": made["version"], "name": "orf", "start": 4, "end": 18, "type": "CDS" }))
        .unwrap();
    let b = s.execute("add_feature", &json!({ "plasmid": "cds", "version": a["version"], "name": "tail", "start": 19, "end": 25 })).unwrap();

    let done = s.execute("codon_optimise", &json!({ "plasmid": "cds", "version": b["version"], "feature": "orf", "organism": "human" })).unwrap();
    // Human: Met ATG, Ala GCC, Arg AGA, Leu CTG, stop TGA.
    assert_eq!(bases(&s, "cds", 1, 25), "CCCATGGCCAGACTGTGACCCGGGG");
    assert!(text(&done["changes"][0]).ends_with("protein identical (5 aa)"), "{}", done["changes"][0]);
    let after = read(&s, "cds");
    assert_eq!(feature(&after, "orf")["location"], "4..18");
    assert_eq!(feature(&after, "tail")["location"], "19..25");

    let ragged = s.execute("codon_optimise", &json!({ "plasmid": "cds", "version": done["version"], "start": 4, "end": 17, "organism": "human" }));
    assert!(ragged.unwrap_err().0.contains("not a whole number of codons"));
    let alien = s.execute("codon_optimise", &json!({ "plasmid": "cds", "version": done["version"], "feature": "orf", "organism": "martian" }));
    assert!(alien.unwrap_err().0.contains("organism must be one of"));
}

#[test]
fn a_reverse_gene_is_optimised_on_its_own_strand() {
    let (_dir, s) = session();
    let coding = "ATGGCACGTTTATAG";
    let made = s
        .execute("create_plasmid", &json!({ "name": "rev", "sequence": format!("CCC{}CCC", bios_core::seq::revcomp(coding)), "circular": false }))
        .unwrap();
    let a = s
        .execute(
            "add_feature",
            &json!({ "plasmid": "rev", "version": made["version"], "name": "orf", "start": 4, "end": 18, "strand": "reverse", "type": "CDS" }),
        )
        .unwrap();
    s.execute("codon_optimise", &json!({ "plasmid": "rev", "version": a["version"], "feature": "orf", "organism": "ecoli" })).unwrap();
    // E. coli: Met ATG, Ala GCG, Arg CGC, Leu CTG, stop TAA; read on the bottom strand.
    let bottom = s.execute("get_sequence", &json!({ "plasmid": "rev", "start": 4, "end": 18, "strand": "reverse" })).unwrap();
    assert_eq!(bottom["sequence"], "ATGGCGCGCCTGTAA");
}

#[test]
fn one_search_looks_at_names_features_and_bases() {
    let (_dir, s, _) = with("pUC19.gb");
    s.execute("create_plasmid", &json!({ "name": "lac cassette", "folder": "parts", "from_plasmid": "pUC19", "from_start": 489, "from_end": 579 }))
        .unwrap();

    let by_name = s.execute("search_project", &json!({ "query": "cassette" })).unwrap();
    assert_eq!(by_name["plasmids"]["total"], 1);
    assert_eq!(by_name["plasmids"]["rows"][0]["path"], "parts/lac cassette");
    assert!(by_name["sequence"].is_null());

    let by_feature = s.execute("search_project", &json!({ "query": "lac operator" })).unwrap();
    let rows = by_feature["features"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&json!({ "plasmid": "pUC19", "feature": "lac operator", "type": "protein_bind", "start": 489, "end": 505 })));
    assert!(rows.contains(&json!({ "plasmid": "parts/lac cassette", "feature": "lac operator", "type": "protein_bind", "start": 1, "end": 17 })));

    // The same thirty bases are in the parent and in the part made from it.
    let piece = bases(&s, "pUC19", 520, 549);
    let by_bases = s.execute("search_project", &json!({ "query": piece })).unwrap();
    let hits = by_bases["sequence"]["rows"].as_array().unwrap();
    assert!(hits.contains(&json!({ "plasmid": "pUC19", "start": 520, "end": 549, "strand": "forward" })));
    assert!(hits.contains(&json!({ "plasmid": "parts/lac cassette", "start": 32, "end": 61, "strand": "forward" })));
}

#[test]
fn a_fasta_file_is_imported_and_a_file_that_is_not_a_sequence_is_refused_in_a_sentence() {
    let (dir, s) = session();
    let imported = s.execute("import_plasmid", &json!({ "file": fixture("gfp.fasta"), "folder": "parts" })).unwrap();
    assert_eq!(imported["already_here"], false);
    let plasmid = text(&imported["plasmid"]).to_string();
    assert!(plasmid.starts_with("parts/"), "{plasmid}");
    let length = imported["length"].as_u64().unwrap();
    assert!(length > 100, "{length}");
    // It is a GenBank file in the project now, with a history like any other.
    let read = s.execute("read_plasmid", &json!({ "plasmid": plasmid })).unwrap();
    assert_eq!(read["topology"], "linear");
    assert!(text(&read["genbank"]).starts_with("LOCUS"));

    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, "Meeting notes, Tuesday. Order more tips. Ask about the freezer.\n").unwrap();
    let refused = s.execute("import_plasmid", &json!({ "file": notes.display().to_string() })).unwrap_err();
    assert_eq!(
        refused.0,
        "notes.txt doesn't look like any sequence format Bios knows. It starts \"Meeting notes, Tuesday. Order more tips.\". GenBank starts with LOCUS, FASTA with \">\"."
    );
    assert_eq!(s.execute("list_plasmids", &json!({})).unwrap()["plasmids"].as_array().unwrap().len(), 1);
}

/// The top strand of `plasmid` from `start` to `end`, reverse-complemented.
fn bottom(s: &Session, plasmid: &str, start: i64, end: i64) -> String {
    bases(s, plasmid, start, end)
        .chars()
        .rev()
        .map(|b| match b {
            'A' => 'T',
            'T' => 'A',
            'C' => 'G',
            _ => 'C',
        })
        .collect()
}

#[test]
fn pcr_predicts_the_product_and_a_tail_ends_up_in_it() {
    let (_dir, s, _) = with("pUC19.gb");
    let forward = bases(&s, "pUC19", 1601, 1622);
    let reverse = bottom(&s, "pUC19", 2379, 2400);
    let plain = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": forward, "reverse": reverse })).unwrap();
    let product = &plain["product"];
    assert_eq!(product["length"], 800);
    assert_eq!(product["copies"], "1601..2400");
    assert_eq!(text(&product["sequence"]), bases(&s, "pUC19", 1601, 2400));
    assert_eq!(product["forward"]["binds"], "1601..1622");
    assert_eq!(product["reverse"]["strand"], "reverse");
    assert_eq!(product["forward"]["mismatches"], 0);

    // An EcoRI tail on the forward primer is the first six bases of the product.
    let tailed = format!("GAATTC{forward}");
    // A tail is never assumed: the caller is told what was found and says so.
    let unsaid = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": tailed, "reverse": reverse })).unwrap_err();
    assert_eq!(
        unsaid.0,
        "the forward oligo binds at 1601..1622 forward with its first 6 bases (GAATTC) left over as a 5' tail. If that tail is meant, call again with forward_tail: 6"
    );
    let with_tail = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": tailed, "reverse": reverse, "forward_tail": 6 })).unwrap();
    assert_eq!(with_tail["product"]["length"], 806);
    assert_eq!(with_tail["product"]["forward"]["tail"], "GAATTC");
    assert!(text(&with_tail["product"]["sequence"]).starts_with(&tailed));

    // Given the wrong way round they are still the same two primers.
    let swapped = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": reverse, "reverse": forward })).unwrap();
    assert_eq!(swapped["product"]["sequence"], plain["product"]["sequence"]);
    let notes = |v: &Value| v["product"]["notes"].as_array().unwrap().clone();
    assert_eq!(notes(&swapped).len(), notes(&plain).len() + 1, "{:?}", notes(&swapped));
    assert!(text(&notes(&swapped)[0]).contains("the other way round"));
}

#[test]
fn pcr_refuses_in_a_sentence_when_there_is_no_product() {
    let (_dir, s, _) = with("pUC19.gb");
    let forward = bases(&s, "pUC19", 1601, 1622);
    let also_forward = bases(&s, "pUC19", 1801, 1822);
    let same = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": forward, "reverse": also_forward })).unwrap_err();
    assert!(same.0.starts_with("both primers read along the same strand"), "{}", same.0);
    let nowhere = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": forward, "reverse": "ACACACACACACACACACACAC" })).unwrap_err();
    assert!(nowhere.0.starts_with("the reverse primer does not bind this plasmid"), "{}", nowhere.0);
    let not_dna = s.execute("simulate_pcr", &json!({ "plasmid": "pUC19", "forward": forward, "reverse": "my reverse primer" })).unwrap_err();
    assert!(not_dna.0.contains("is not a primer on this plasmid"), "{}", not_dna.0);
}

#[test]
fn a_saved_pcr_product_knows_its_template_and_keeps_the_features_inside_it() {
    let (_dir, s, version) = with("pUC19.gb");
    let amp = feature(&read(&s, "pUC19"), "AmpR").clone();
    let (start, end) = (amp["start"].as_i64().unwrap(), amp["end"].as_i64().unwrap());
    // Primers stored on the plasmid, 30 bases outside the gene on each side, used by name.
    let added = s
        .execute(
            "add_primer",
            &json!({ "plasmid": "pUC19", "version": version, "name": "amp-F", "start": start - 30, "end": start - 9, "strand": "forward" }),
        )
        .unwrap();
    s.execute(
        "add_primer",
        &json!({ "plasmid": "pUC19", "version": text(&added["version"]), "name": "amp-R", "start": end + 9, "end": end + 30, "strand": "reverse" }),
    )
    .unwrap();
    let made = s
        .execute("save_pcr_product", &json!({ "plasmid": "pUC19", "forward": "amp-F", "reverse": "amp-R", "name": "amp amplicon", "folder": "pcr" }))
        .unwrap();
    assert_eq!(made["created"], "pcr/amp amplicon");
    assert_eq!(made["made_from"], "pUC19");
    assert_eq!(made["length"].as_i64().unwrap(), end - start + 61);

    let product = read(&s, "pcr/amp amplicon");
    assert_eq!(product["topology"], "linear");
    let carried = feature(&product, "AmpR");
    assert_eq!((carried["start"].as_i64().unwrap(), carried["end"].as_i64().unwrap()), (31, end - start + 31));
    assert_eq!(carried["strand"], amp["strand"]);
    assert_eq!(feature(&product, "amp-F")["start"], 1);
    assert_eq!(feature(&product, "amp-R")["end"], made["length"]);
    assert_eq!(bases(&s, "pcr/amp amplicon", 31, end - start + 31), bases(&s, "pUC19", start, end));

    // The project lists it as made from the template, and its first history entry says so.
    let listed = s.execute("list_plasmids", &json!({})).unwrap();
    let row = listed["plasmids"].as_array().unwrap().iter().find(|p| p["path"] == "pcr/amp amplicon").unwrap().clone();
    assert_eq!(row["made_from"], "pUC19");
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pcr/amp amplicon" })).unwrap();
    assert_eq!(history["history"][0]["command"], "save_pcr_product");
}

/// Bases that repeat nowhere, from the 32-bit generator the web app's tests use.
fn random_dna(length: usize, seed: u32) -> String {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            char::from(b"ACGT"[((state >> 28) & 3) as usize])
        })
        .collect()
}

#[test]
fn gibson_between_two_enzymes_designs_one_piece_that_carries_both_arms_and_keeps_both_sites() {
    let (_dir, s, _) = with("pUC19.gb");
    let insert = random_dna(300, 7);
    let out = s
        .execute(
            "design_gibson",
            &json!({ "plasmid": "pUC19", "enzymes": ["ecori", "HindIII"], "inserts": [{ "name": "payload", "sequence": insert }] }),
        )
        .unwrap();
    let design = &out["design"];
    assert_eq!(design["opening"]["enzymes"], json!(["EcoRI", "HindIII"]));
    assert_eq!(design["opening"]["sites_kept"], json!(["EcoRI", "HindIII"]));
    let removed = design["opening"]["removed_length"].as_u64().unwrap();
    assert_eq!(design["product"]["length"].as_u64().unwrap(), 2686 - removed + 300);
    // One piece to order: the upstream arm, the insert, the downstream arm.
    let order = design["order"].as_array().unwrap();
    assert_eq!(order.len(), 1);
    let (left, right) = (text(&design["junctions"][0]["overlap"]), text(&design["junctions"][1]["overlap"]));
    assert_eq!(text(&order[0]["sequence"]), format!("{left}{insert}{right}"));
    assert_eq!(design["junctions"][0]["unique"], true);
    for site in design["enzyme_sites"].as_array().unwrap() {
        assert_eq!(site["cuts_product"], 1, "{site}");
    }
    // The arms are the plasmid's own bases on each side of what leaves.
    let (first, last) = {
        let span: Vec<i64> = text(&design["opening"]["removed"]).split("..").map(|n| n.parse().unwrap()).collect();
        (span[0], span[1])
    };
    assert_eq!(bases(&s, "pUC19", first - left.len() as i64, first - 1), left);
    assert_eq!(bases(&s, "pUC19", last + 1, last + right.len() as i64), right);
}

#[test]
fn gibson_inserts_without_removing_and_joins_several_fragments_in_order() {
    let (_dir, s, _) = with("pUC19.gb");
    let (a, b) = (random_dna(200, 11), random_dna(250, 23));
    let out = s
        .execute(
            "design_gibson",
            &json!({ "plasmid": "pUC19", "insert_after": 1000, "inserts": [{ "name": "A", "sequence": a }, { "name": "B", "sequence": b }] }),
        )
        .unwrap();
    let design = &out["design"];
    assert_eq!(design["opening"]["removed_length"], 0);
    assert_eq!(design["opening"]["removed"], "nothing; the new DNA goes after base 1000");
    assert_eq!(design["product"]["length"], 2686 + 450);
    assert_eq!(design["placed"], json!([{ "name": "A", "start": 1001, "end": 1200 }, { "name": "B", "start": 1201, "end": 1450 }]));
    let kinds: Vec<&str> = design["junctions"].as_array().unwrap().iter().map(|j| text(&j["kind"])).collect();
    assert_eq!(kinds, ["plasmid to insert", "insert to insert", "insert to plasmid"]);
    let fragments: Vec<&str> = design["order"].as_array().unwrap().iter().map(|o| text(&o["fragment"])).collect();
    assert_eq!(fragments, ["A", "B"]);
}

#[test]
fn gibson_refuses_in_a_sentence() {
    let (_dir, s, _) = with("pUC19.gb");
    // A linear copy of the whole plasmid, for the stretch that would have to run through an origin.
    s.execute("create_plasmid", &json!({ "name": "line", "from_plasmid": "pUC19", "from_start": 1, "from_end": 2686 })).unwrap();
    let insert = json!([{ "sequence": random_dna(120, 5) }]);
    let refused = |params: Value| s.execute("design_gibson", &params).unwrap_err().0;
    assert_eq!(
        refused(json!({ "plasmid": "pUC19", "inserts": insert })),
        "say where the plasmid opens in one way: enzymes, or replace_start with replace_end, or insert_after"
    );
    assert_eq!(
        refused(json!({ "plasmid": "line", "inserts": insert, "replace_start": 2600, "replace_end": 20 })),
        "that stretch runs through the origin, but this plasmid is linear"
    );
    assert_eq!(
        refused(json!({ "plasmid": "pUC19", "inserts": insert, "enzymes": ["EcoRI", "NotAnEnzyme"] })),
        "NotAnEnzyme is not an enzyme Bios knows"
    );
    assert_eq!(refused(json!({ "plasmid": "pUC19", "inserts": insert, "enzymes": ["EcoRI", "ecori"] })), "give two different enzymes");
    assert!(
        refused(json!({ "plasmid": "pUC19", "inserts": [{ "sequence": "ACGTNNNNACGT" }], "insert_after": 10 }))
            .contains("may only hold A, C, G and T")
    );
    assert_eq!(
        refused(json!({ "plasmid": "pUC19", "inserts": [], "insert_after": 10 })),
        "give at least one insert, as [{\"name\": ..., \"sequence\": ...}]"
    );
}

#[test]
fn a_saved_gibson_design_is_a_new_plasmid_made_from_its_backbone() {
    let (_dir, s, _) = with("pUC19.gb");
    let insert = random_dna(300, 7);
    let made = s
        .execute(
            "save_gibson_design",
            &json!({ "plasmid": "pUC19", "replace_start": 1001, "replace_end": 1100, "inserts": [{ "name": "payload", "sequence": insert }], "name": "pUC19 payload", "folder": "built" }),
        )
        .unwrap();
    assert_eq!(made["created"], "built/pUC19 payload");
    assert_eq!(made["made_from"], "pUC19");
    assert_eq!(made["length"], 2686 - 100 + 300);
    // The backbone is untouched and the product holds the insert where the design says.
    assert_eq!(read(&s, "pUC19")["length"], 2686);
    assert_eq!(bases(&s, "built/pUC19 payload", 1001, 1300), insert);
    assert_eq!(bases(&s, "built/pUC19 payload", 1, 1000), bases(&s, "pUC19", 1, 1000));
    assert_eq!(bases(&s, "built/pUC19 payload", 1301, 2886), bases(&s, "pUC19", 1101, 2686));
    let product = read(&s, "built/pUC19 payload");
    let placed = feature(&product, "payload");
    assert_eq!((placed["start"].as_i64().unwrap(), placed["end"].as_i64().unwrap()), (1001, 1300));
    // A feature well clear of the opening moved with its bases.
    let (before, after) = (feature(&read(&s, "pUC19"), "AmpR").clone(), feature(&product, "AmpR").clone());
    assert_eq!(after["start"].as_i64().unwrap(), before["start"].as_i64().unwrap() + 200);
    let history = s.execute("plasmid_history", &json!({ "plasmid": "built/pUC19 payload" })).unwrap();
    assert_eq!(history["history"][0]["command"], "save_gibson_design");
}

#[test]
fn a_whole_text_write_says_what_changed_and_the_history_does_not_keep_a_second_copy_of_the_text() {
    let (dir, s, version) = with("wrap_demo.gb");
    let before = text(&s.execute("read_plasmid", &json!({ "plasmid": "wrap_demo" })).unwrap()["genbank"]).to_string();
    let edited = before.replacen("misc_feature", "protein_bind", 1);
    assert_ne!(edited, before);
    s.execute(
        "write_plasmid",
        &json!({ "plasmid": "wrap_demo", "version": version, "genbank": edited, "summary": "  Made the MCS a protein_bind " }),
    )
    .unwrap();
    let history = s.execute("plasmid_history", &json!({ "plasmid": "wrap_demo" })).unwrap();
    let last = history["history"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["summary"], "Made the MCS a protein_bind");
    // The journal names the command and its other parameters, and leaves the text to the version.
    let journal: String = std::fs::read_dir(dir.path().join("Lab/.bios/journal"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert!(journal.contains("\"write_plasmid\""));
    assert!(!journal.contains("LOCUS"), "the journal holds a copy of the GenBank text");
    assert_eq!(feature(&read(&s, "wrap_demo"), "MCS")["type"], "protein_bind");
}
