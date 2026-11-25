//! `compare_plasmids` through `Session::execute`, against copies of pUC19 this file made, so
//! every expected position and base is known exactly.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// A project holding pUC19 and a circular copy of it called `copy`, ready to be edited.
fn with_copy(copy: &str) -> (tempfile::TempDir, Session, String) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    s.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb") })).unwrap();
    let made =
        s.execute("create_plasmid", &json!({ "name": copy, "from_plasmid": "pUC19", "from_start": 1, "from_end": 2686, "circular": true })).unwrap();
    let version = made["version"].as_str().unwrap().to_string();
    (dir, s, version)
}

fn edit(s: &Session, command: &str, mut params: Value, version: &str) -> String {
    if let Some(map) = params.as_object_mut() {
        map.insert("version".into(), json!(version));
    }
    let done = s.execute(command, &params).unwrap();
    done["version"].as_str().unwrap().to_string()
}

fn compared(s: &Session, params: Value) -> Value {
    s.execute("compare_plasmids", &params).unwrap()
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}

#[test]
fn a_plasmid_and_an_untouched_copy_of_it_are_reported_identical() {
    let (_dir, s, _) = with_copy("pUC19 copy");
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 copy" }));
    assert_eq!(out["identical"], json!(true));
    assert_eq!(out["comparable"], json!(true));
    assert_eq!(out["alignment"]["identity_percent"], json!(100.0));
    assert_eq!(out["alignment"]["aligned_length"], json!(2686));
    assert_eq!(out["alignment"]["strand"], json!("forward"));
    assert_eq!(out["alignment"]["to_base_at_plasmid_base_1"], json!(1));
    assert_eq!(out["differences"]["total"], json!(0));
    assert_eq!(text(&out["summary"]), "pUC19 and pUC19 copy are the same 2,686 bp circular plasmid, base for base. Their features agree too.");
}

#[test]
fn a_copy_whose_origin_was_moved_is_the_same_plasmid_and_the_turn_is_in_the_answer() {
    let (_dir, s, version) = with_copy("pUC19 turned");
    edit(&s, "set_origin", json!({ "plasmid": "pUC19 turned", "position": 1001 }), &version);
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 turned" }));
    assert_eq!(out["identical"], json!(true));
    assert_eq!(out["alignment"]["to_base_at_plasmid_base_1"], json!(1687));
    assert_eq!(text(&out["alignment"]["lines_up_at"]), "base 1,687 of pUC19 turned lines up with base 1 of pUC19");
    assert!(text(&out["summary"]).contains("Its origin is moved: base 1,687 of pUC19 turned is base 1 of pUC19"), "{}", out["summary"]);
}

#[test]
fn a_copy_stored_the_other_way_round_is_the_same_plasmid() {
    let (_dir, s, version) = with_copy("pUC19 flipped");
    edit(&s, "reverse_complement", json!({ "plasmid": "pUC19 flipped" }), &version);
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 flipped" }));
    assert_eq!(out["identical"], json!(true));
    assert_eq!(out["alignment"]["strand"], json!("reverse"));
    assert_eq!(out["alignment"]["to_base_at_plasmid_base_1"], json!(2686));
}

#[test]
fn a_change_in_a_copy_stored_the_other_way_round_is_placed_on_that_copys_own_bases() {
    let (_dir, s, version) = with_copy("pUC19 flipped point");
    // Base 500 changed from T to A, then the whole plasmid reverse-complemented. Base 500 of
    // pUC19 is base 2,686 - 500 + 1 = 2,187 of the copy, and the base reported there is the one
    // the comparison read: A on the copy's reverse strand, which its file stores as T.
    let v = edit(
        &s,
        "replace_range",
        json!({ "plasmid": "pUC19 flipped point", "start": 500, "end": 500, "sequence": "A", "expected_sequence": "T" }),
        &version,
    );
    edit(&s, "reverse_complement", json!({ "plasmid": "pUC19 flipped point" }), &v);

    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 flipped point" }));
    assert_eq!(out["alignment"]["strand"], json!("reverse"));
    assert_eq!(out["alignment"]["to_base_at_plasmid_base_1"], json!(2686));
    let list = out["differences"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["plasmid"], json!({ "start": 500, "end": 500, "bases": "T", "feature": "lac operator (protein_bind)" }));
    assert_eq!(list[0]["to"], json!({ "start": 2187, "end": 2187, "bases": "A", "feature": "lac operator (protein_bind)", "strand": "reverse" }));
    assert_eq!(text(&s.execute("get_sequence", &json!({ "plasmid": "pUC19 flipped point", "start": 2187, "end": 2187 })).unwrap()["sequence"]), "T");
}

#[test]
fn three_edits_are_three_differences_placed_on_both_plasmids() {
    let (_dir, s, version) = with_copy("pUC19 edited");
    // Highest position first, so each edit is made at the coordinates pUC19 itself has.
    let v = edit(&s, "delete_range", json!({ "plasmid": "pUC19 edited", "start": 2000, "end": 2019 }), &version);
    let v = edit(&s, "insert_sequence", json!({ "plasmid": "pUC19 edited", "position": 1200, "sequence": "GGACA" }), &v);
    edit(&s, "replace_range", json!({ "plasmid": "pUC19 edited", "start": 500, "end": 500, "sequence": "A", "expected_sequence": "T" }), &v);

    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 edited" }));
    assert_eq!(out["identical"], json!(false));
    assert_eq!(out["lengths"], json!({ "plasmid": 2686, "to": 2671 }));
    assert_eq!(out["alignment"]["aligned_length"], json!(2691));
    assert_eq!(out["alignment"]["matching_bases"], json!(2665));
    assert_eq!(out["differences"]["total"], json!(3));
    assert_eq!(out["differences"]["substitutions"], json!(1));
    assert_eq!(out["differences"]["insertions"], json!(1));
    assert_eq!(out["differences"]["deletions"], json!(1));
    assert_eq!(out["differences"]["not_shown"], json!(0));

    let list = out["differences"]["list"].as_array().unwrap();
    // Base 500: T in pUC19, A in the copy, inside the lac operator, which is not a CDS.
    assert_eq!(
        list[0],
        json!({
            "kind": "substitution",
            "length": 1,
            "plasmid": { "start": 500, "end": 500, "bases": "T", "feature": "lac operator (protein_bind)" },
            "to": { "start": 500, "end": 500, "bases": "A", "feature": "lac operator (protein_bind)" },
        })
    );
    // GGACA after base 1,200 of pUC19, which is bases 1,201..1,205 of the copy.
    assert_eq!(
        list[1],
        json!({
            "kind": "insertion",
            "length": 5,
            "plasmid": { "after": 1200, "feature": "ori (rep_origin)" },
            "to": { "start": 1201, "end": 1205, "bases": "GGACA", "feature": "ori (rep_origin)" },
        })
    );
    // The 20 bases gone from AmpR: the copy has nothing there, so it names the base before.
    assert_eq!(
        list[2],
        json!({
            "kind": "deletion",
            "length": 20,
            "plasmid": { "start": 2000, "end": 2019, "bases": "AACGATCAAGGCGAGTTACA", "feature": "AmpR (CDS)" },
            "to": { "after": 2004, "feature": "AmpR (CDS)" },
            "protein": "AmpR (CDS) on pUC19: 20 bp removed at codon 163: shifts the reading frame from there on",
        })
    );
    assert_eq!(
        text(&out["summary"]),
        "pUC19 and pUC19 edited are 99.0% identical over 2,691 aligned bases: 1 substitution, 1 insertion, 1 deletion."
    );
}

#[test]
fn a_substitution_in_a_cds_says_what_it_does_to_the_protein() {
    let (_dir, s, version) = with_copy("pUC19 point");
    // Base 198 is the middle base of lacZ alpha's codon 91 (the CDS reads the other way).
    edit(&s, "replace_range", json!({ "plasmid": "pUC19 point", "start": 198, "end": 198, "sequence": "C", "expected_sequence": "A" }), &version);
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 point" }));
    let list = out["differences"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["plasmid"]["feature"], json!("lacZ alpha (CDS)"));
    assert_eq!(text(&list[0]["protein"]), "lacZ alpha (CDS) on pUC19: codon 91 ATT→AGT, changes the protein: Ile91Ser");
}

#[test]
fn a_feature_one_map_has_and_the_other_has_not_is_reported() {
    let (_dir, s, version) = with_copy("pUC19 annotated");
    edit(&s, "add_feature", json!({ "plasmid": "pUC19 annotated", "name": "my insert", "type": "misc_feature", "start": 400, "end": 450 }), &version);
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 annotated" }));
    assert_eq!(out["identical"], json!(true));
    assert_eq!(out["features"]["only_in_plasmid_total"], json!(0));
    assert_eq!(out["features"]["only_in_to_total"], json!(1));
    let only = out["features"]["only_in_to"].as_array().unwrap();
    assert_eq!(only[0]["name"], json!("my insert"));
    assert_eq!((only[0]["start"].as_i64(), only[0]["end"].as_i64()), (Some(400), Some(450)));
    assert!(text(&out["summary"]).ends_with("1 feature only on pUC19 annotated."), "{}", out["summary"]);
}

#[test]
fn two_plasmids_that_are_not_versions_of_each_other_are_said_to_be_so() {
    let (_dir, s, _) = with_copy("pUC19 copy");
    let other: String = (0..2686).map(|i| ["A", "C", "G", "T"][(i * 7 + i / 4) % 4]).collect::<Vec<_>>().concat();
    s.execute("create_plasmid", &json!({ "name": "unrelated", "sequence": other, "circular": true })).unwrap();
    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "unrelated" }));
    assert_eq!(out["comparable"], json!(false));
    assert_eq!(out["identical"], json!(false));
    assert!(out["differences"].is_null());
    assert!(text(&out["summary"]).contains("do not line up"), "{}", out["summary"]);
}

#[test]
fn bases_can_be_pasted_in_instead_of_naming_a_second_plasmid() {
    let (_dir, s, _) = with_copy("pUC19 copy");
    let all = s.execute("get_sequence", &json!({ "plasmid": "pUC19" })).unwrap();
    let bases = text(&all["sequence"]).to_string();
    // The same plasmid read from base 1,501 on, with base 500 changed: base 500 of pUC19 is then
    // base 1,686 of what was pasted in.
    let edited = format!("{}A{}", &bases[..499], &bases[500..]);
    let pasted = format!("{}{}", &edited[1500..], &edited[..1500]);
    let out = compared(&s, json!({ "plasmid": "pUC19", "sequence": pasted }));
    assert_eq!(out["to"], json!("the sequence given"));
    assert_eq!(out["alignment"]["to_base_at_plasmid_base_1"], json!(1187));
    let list = out["differences"]["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["plasmid"], json!({ "start": 500, "end": 500, "bases": "T", "feature": "lac operator (protein_bind)" }));
    assert_eq!(list[0]["to"], json!({ "start": 1686, "end": 1686, "bases": "A" }));
    // Nothing is said about features: the bases given carry none.
    assert_eq!(out["features"], json!({ "not_compared": "the sequence given carries no features" }));
}

#[test]
fn bases_pasted_in_that_match_say_so_without_claiming_the_features_agree() {
    let (_dir, s, _) = with_copy("pUC19 copy");
    let all = s.execute("get_sequence", &json!({ "plasmid": "pUC19" })).unwrap();
    let out = compared(&s, json!({ "plasmid": "pUC19", "sequence": text(&all["sequence"]) }));
    assert_eq!(out["identical"], json!(true));
    assert_eq!(out["features"], json!({ "not_compared": "the sequence given carries no features" }));
    assert_eq!(text(&out["summary"]), "pUC19 and the sequence given are the same 2,686 bp circular plasmid, base for base.");
}

#[test]
fn the_list_is_capped_and_says_how_many_were_left_out() {
    let (_dir, s, version) = with_copy("pUC19 peppered");
    let all = s.execute("get_sequence", &json!({ "plasmid": "pUC19" })).unwrap();
    let mut bases = text(&all["sequence"]).to_string().into_bytes();
    for i in 0..120 {
        let at = 10 + i * 20;
        bases[at] = if bases[at] == b'A' { b'C' } else { b'A' };
    }
    let peppered = String::from_utf8(bases).unwrap();
    edit(&s, "replace_range", json!({ "plasmid": "pUC19 peppered", "start": 1, "end": 2686, "sequence": peppered }), &version);

    let out = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 peppered" }));
    assert_eq!(out["differences"]["total"], json!(120));
    assert_eq!(out["differences"]["shown"], json!(100));
    assert_eq!(out["differences"]["not_shown"], json!(20));
    assert_eq!(out["differences"]["list"].as_array().unwrap().len(), 100);

    let more = compared(&s, json!({ "plasmid": "pUC19", "to": "pUC19 peppered", "max_differences": 200 }));
    assert_eq!(more["differences"]["shown"], json!(120));
    assert_eq!(more["differences"]["not_shown"], json!(0));
}

#[test]
fn a_call_with_nothing_to_compare_against_says_what_it_needs() {
    let (_dir, s, _) = with_copy("pUC19 copy");
    let failed = s.execute("compare_plasmids", &json!({ "plasmid": "pUC19" })).unwrap_err();
    assert!(failed.0.contains("needs `to`"), "{}", failed.0);
    let both = s.execute("compare_plasmids", &json!({ "plasmid": "pUC19", "to": "pUC19 copy", "sequence": "ACGT" })).unwrap_err();
    assert!(both.0.contains("not both"), "{}", both.0);
}
