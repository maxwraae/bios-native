//! Build, through `Session::execute`: the folder is the backbone, one cut per folder, one plasmid
//! per insert, and a table of pieces to order across the whole folder.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// A session holding pUC19, the backbone these builds open.
fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let session = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    session.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb") })).unwrap();
    (dir, session)
}

/// Bases that are nothing in particular, the same every time.
fn random_dna(n: usize, seed: u64) -> String {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ["A", "C", "G", "T"][(x >> 33) as usize % 4]
        })
        .collect()
}

fn inserts(names: &[(&str, usize)]) -> Value {
    json!(names.iter().enumerate().map(|(i, (name, len))| json!({ "name": name, "sequence": random_dna(*len, 7 + i as u64) })).collect::<Vec<_>>())
}

fn build(s: &Session, inserts: Value) -> Value {
    s.execute("build_plasmids", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "inserts": inserts })).unwrap()
}

#[test]
fn designing_says_what_each_insert_costs_without_keeping_anything() {
    let (_dir, s) = session();
    let out = s
        .execute("design_builds", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "inserts": inserts(&[("EGFP", 700), ("BFP", 300)]) }))
        .unwrap();
    assert_eq!(out["opening"], "cut with EcoRI and HindIII");
    let designs = out["designs"].as_array().unwrap();
    assert_eq!(designs.len(), 2, "one design per insert: each is its own plasmid");
    assert_eq!(designs[0]["name"], "pUC19_EGFP");
    assert_eq!(designs[1]["name"], "pUC19_BFP");
    assert!(designs[0]["verdict"].as_str().unwrap().contains("to order"), "{}", designs[0]["verdict"]);
    assert!(!designs[0]["order"].as_array().unwrap().is_empty());
    // Nothing was kept: no folder, no plasmids.
    assert!(s.execute("list_build_folders", &json!({})).unwrap()["build_folders"].as_array().unwrap().is_empty());
}

#[test]
fn building_births_a_folder_holding_a_frozen_copy_of_the_backbone() {
    let (_dir, s) = session();
    let out = build(&s, inserts(&[("EGFP", 700)]));
    assert_eq!(out["folder"], "pUC19");
    assert_eq!(out["built"].as_array().unwrap().len(), 1);

    let folders = s.execute("list_build_folders", &json!({})).unwrap();
    let folder = &folders["build_folders"][0];
    assert_eq!(folder["folder"], "pUC19");
    assert_eq!(folder["opening"], "cut with EcoRI and HindIII");
    assert_eq!(folder["designs"], 1);
    // THE COPY IS THE POINT: the folder's backbone is its own, not the plasmid on the bench.
    assert_eq!(folder["backbone"], "pUC19/pUC19");
    assert_eq!(folder["backbone_length"], 2686);

    // And the product is an ordinary plasmid in the folder, made from that copy.
    let read = s.execute("read_plasmid", &json!({ "plasmid": "pUC19/pUC19_EGFP", "genbank": false })).unwrap();
    assert_eq!(read["length"].as_u64().unwrap(), 2686 - 43 + 700, "what the cut took out, and what went in");
}

#[test]
fn building_into_the_same_bases_again_joins_the_same_folder() {
    // Same bases → the same folder, so the two designs sit together and are comparable.
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let again = build(&s, inserts(&[("BFP", 400)]));
    assert_eq!(again["folder"], "pUC19");

    let folder = s.execute("read_build_folder", &json!({ "folder": "pUC19" })).unwrap();
    let names: Vec<&str> = folder["designs"].as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["pUC19_BFP", "pUC19_EGFP"], "newest first on the page");
    // One backbone copy, not two.
    let listed = s.execute("list_plasmids", &json!({})).unwrap();
    let copies = listed["plasmids"].as_array().unwrap().iter().filter(|p| p["path"] == "pUC19/pUC19").count();
    assert_eq!(copies, 1);
}

#[test]
fn a_product_whose_name_is_taken_is_numbered_rather_than_overwriting() {
    // Building the same insert twice is ordinary — you changed the opening and want both.
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let again = build(&s, inserts(&[("EGFP", 700)]));
    assert_eq!(again["built"][0]["name"], "pUC19_EGFP (2)");
    let folder = s.execute("list_designs", &json!({ "folder": "pUC19" })).unwrap();
    assert_eq!(folder["designs"].as_array().unwrap().len(), 2);
}

#[test]
fn an_unnamed_insert_is_numbered_so_a_hundred_pastes_become_a_hundred_plasmids() {
    let (_dir, s) = session();
    let out = build(&s, json!([{ "sequence": random_dna(300, 1) }, { "sequence": random_dna(300, 2) }]));
    let names: Vec<&str> = out["built"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["pUC19_1", "pUC19_2"]);
}

#[test]
fn the_folder_page_says_what_leaves_what_the_cut_clips_and_what_to_order() {
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700), ("BFP", 400)]));
    let page = s.execute("read_build_folder", &json!({ "folder": "pUC19" })).unwrap();

    // The two bands of the gel this digest gives.
    assert_eq!(page["digest"]["comes_out"], 43, "the MCS between EcoRI and HindIII, both sites kept with a base to spare");
    assert_eq!(page["digest"]["stays"], 2686 - 43);
    // What the cut clips is a fact about the CUT, said once rather than counted onto every row.
    assert!(page["cut_clips"].is_array());

    let pieces = page["to_order"].as_array().unwrap();
    assert_eq!(pieces.len(), 2, "two designs, a piece each");
    assert_eq!(pieces[0]["n"], 1);
    assert_eq!(pieces[1]["n"], 2);
    // And each design's recipe is the numbers of the rows it cites.
    let designs = page["designs"].as_array().unwrap();
    assert_eq!(designs[1]["recipe"], json!([1]), "the older design is piece 1");
    assert_eq!(designs[0]["recipe"], json!([2]));
}

#[test]
fn the_order_table_is_text_to_paste_and_a_workbook_to_keep() {
    let (dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let file = dir.path().join("order.xlsx");
    let out = s.execute("build_order_table", &json!({ "folder": "pUC19", "file": file.display().to_string() })).unwrap();

    let text = out["text"].as_str().unwrap();
    assert!(text.starts_with("#\tname\tlength\tsequence\n1\tEGFP\t"), "{text}");
    assert_eq!(out["recipes"], json!([{ "design": "pUC19_EGFP", "pieces": [1] }]));
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04", "a workbook is a zip");
    // An existing file is not replaced unless the caller says so.
    assert!(s.execute("build_order_table", &json!({ "folder": "pUC19", "file": file.display().to_string() })).is_err());
    s.execute("build_order_table", &json!({ "folder": "pUC19", "file": file.display().to_string(), "overwrite": true })).unwrap();
}

#[test]
fn changing_a_folders_cut_marks_what_was_built_with_the_old_one() {
    // Nothing already ordered is quietly rewritten: the design keeps the cut that made it.
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let changed = s.execute("set_build_opening", &json!({ "folder": "pUC19", "replace_start": 100, "replace_end": 400 })).unwrap();
    assert_eq!(changed["opening"], "opened at 100–400");

    let page = s.execute("read_build_folder", &json!({ "folder": "pUC19" })).unwrap();
    assert_eq!(page["opening"], "opened at 100–400");
    assert_eq!(page["designs"][0]["note"], "built with an earlier cut");
}

#[test]
fn dropping_a_design_forgets_how_it_was_made_and_keeps_the_plasmid() {
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let dropped = s.execute("drop_design", &json!({ "folder": "pUC19", "name": "pUC19_EGFP" })).unwrap();
    assert_eq!(dropped["plasmid_kept"], true);

    let page = s.execute("read_build_folder", &json!({ "folder": "pUC19" })).unwrap();
    assert!(page["designs"].as_array().unwrap().is_empty());
    assert!(page["to_order"].as_array().unwrap().is_empty(), "its pieces went with the row");
    // The product itself is an ordinary construct and stays one.
    s.execute("read_plasmid", &json!({ "plasmid": "pUC19/pUC19_EGFP", "genbank": false })).unwrap();
    assert!(s.execute("drop_design", &json!({ "folder": "pUC19", "name": "pUC19_EGFP" })).is_err());
}

#[test]
fn build_refuses_in_a_sentence() {
    let (_dir, s) = session();
    let refused = |params: Value| s.execute("build_plasmids", &params).unwrap_err().0;
    assert!(refused(json!({ "plasmid": "pUC19", "inserts": inserts(&[("EGFP", 700)]) })).contains("where the backbone opens"));
    assert!(
        refused(json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "EcoRI"], "inserts": inserts(&[("EGFP", 700)]) })).contains("two different enzymes")
    );
    assert!(
        refused(json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "PvuII"], "inserts": inserts(&[("EGFP", 700)]) })).contains("cuts"),
        "a multi-cutter is not an opening"
    );
    assert!(s.execute("read_build_folder", &json!({ "folder": "nowhere" })).unwrap_err().0.contains("not a build folder"));
}

#[test]
fn a_build_folder_is_on_disk_the_way_the_web_app_writes_one() {
    // One project, two apps: the record and the pieces are where the web app looks for them.
    let (_dir, s) = session();
    build(&s, inserts(&[("EGFP", 700)]));
    let root = s.project().root();
    let record: Value = serde_json::from_str(&std::fs::read_to_string(root.join("pUC19/.bios/folder.json")).unwrap()).unwrap();
    assert_eq!(record["type"], "build");
    assert_eq!(record["v"], 1);
    assert_eq!(record["opening"], json!({ "kind": "enzymes", "a": "EcoRI", "b": "HindIII", "arc": "between" }));
    let row = &record["designs"][0];
    assert_eq!(row["name"], "pUC19_EGFP");
    assert_eq!(row["fragments"], 1);
    assert_eq!(row["backboneName"], "pUC19");
    assert_eq!(row["backboneLength"], 2686);

    let designs = std::fs::read_dir(root.join("pUC19/.bios/designs")).unwrap().count();
    assert_eq!(designs, 1, "one payload beside the record");
}

#[test]
fn a_design_says_where_it_opens_and_what_each_piece_spans_so_it_can_be_drawn() {
    // A caller that is going to DRAW the design needs more than what it costs: where the backbone
    // opens, the arms it reaches over, and each piece's span in the laid-out frame.
    let (_dir, s) = session();
    let out =
        s.execute("design_builds", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "inserts": inserts(&[("EGFP", 700)]) })).unwrap();
    let design = &out["designs"][0];
    assert_eq!(design["opening"]["removed_length"], 43);
    let (up, down) = (design["opening"]["up"].as_i64().unwrap(), design["opening"]["down"].as_i64().unwrap());
    assert_eq!(down - up, 43, "the opening is the span that leaves, 0-based");
    let (left, right) = (design["arms"]["left"].as_i64().unwrap(), design["arms"]["right"].as_i64().unwrap());
    assert!(left > 0 && right > 0, "a Gibson piece carries an arm at each end");
    let piece = &design["order"][0];
    // The piece is laid out on `left arm + insert + right arm`, so one piece spans all of it.
    assert_eq!(piece["start"], 0);
    assert_eq!(piece["end"].as_i64().unwrap(), left + 700 + right);
    assert_eq!(design["overlaps"][0]["kind"], "plasmid to insert");
    // The product is not in the way unless it is asked for.
    assert!(design["product"].is_null());
}

#[test]
fn the_product_comes_back_only_when_it_is_asked_for() {
    let (_dir, s) = session();
    let out = s
        .execute(
            "design_builds",
            &json!({
                "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"],
                "inserts": inserts(&[("EGFP", 700)]), "with_product": true,
            }),
        )
        .unwrap();
    let product = &out["designs"][0]["product"];
    assert_eq!(product["sequence"].as_str().unwrap().len(), 2686 - 43 + 700);
    // The product has the backbone's own topology: pUC19 is circular, so this is too.
    assert_eq!(product["circular"], true);
    let features = product["features"].as_array().unwrap();
    assert!(features.iter().any(|f| f["name"] == "EGFP"), "the insert is a feature on what it makes");
    // 1-based and inclusive, as a person counts them.
    assert!(features.iter().all(|f| f["start"].as_i64().unwrap() >= 1));
}
