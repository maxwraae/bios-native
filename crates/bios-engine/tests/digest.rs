//! `digest_plasmid`, `clone_by_restriction` and `save_restriction_clone` through
//! `Session::execute`.
//!
//! The numbers come from the published pUC19 sequence (GenBank L09137, 2686 bp circular) and the
//! enzymes' published cut positions; `bios-core/tests/digest.rs` records how each was worked out.
//! EcoRI cuts after base 396, PstI after 439, HindIII after 447, so EcoRI + PstI gives 43 bp and
//! 2643 bp, and EcoRI + HindIII takes out the 51 bp polylinker and keeps 2635 bp.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// A session holding pUC19.
fn lab() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let session = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    session.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb") })).unwrap();
    (dir, session)
}

fn sizes(digest: &Value) -> Vec<u64> {
    digest["sizes_on_a_gel"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect()
}

fn fragment(digest: &Value, length: u64) -> &Value {
    digest["fragments"].as_array().unwrap().iter().find(|f| f["length"] == length).unwrap_or_else(|| panic!("no {length} bp fragment"))
}

#[test]
fn a_double_digest_comes_back_in_gel_order_and_adds_up() {
    let (_dir, s) = lab();
    let out = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "PstI"] })).unwrap();
    assert_eq!(sizes(&out), vec![2643, 43]);
    assert_eq!(out["total_bp"], 2686);
    assert_eq!(out["fragment_count"], 2);
    assert_eq!(out["cuts"], 2);
    assert_eq!(out["topology"], "circular");

    let small = fragment(&out, 43);
    assert_eq!((small["start"].as_u64(), small["end"].as_u64(), small["through_origin"].as_bool()), (Some(397), Some(439), Some(false)));
    assert_eq!(small["left_end"], json!({ "made_by": "EcoRI", "end": "5' AATT" }));
    assert_eq!(small["right_end"], json!({ "made_by": "PstI", "end": "3' TGCA" }));
    assert_eq!(small["features_inside"], json!([]));
    assert_eq!(small["features_cut_through"], json!(["synthetic DNA construct", "lacZ alpha", "MCS"]));

    let big = fragment(&out, 2643);
    assert_eq!((big["start"].as_u64(), big["end"].as_u64(), big["through_origin"].as_bool()), (Some(440), Some(396), Some(true)));
    assert_eq!(
        big["features_inside"],
        json!(["M13 fwd", "M13 rev", "lac operator", "lac promoter", "CAP binding site", "ori", "AmpR", "AmpR promoter"])
    );

    let cuts = out["cut_sites"].as_array().unwrap();
    assert_eq!(
        cuts[0],
        json!({ "enzyme": "EcoRI", "after_base": 396, "bottom_after_base": 400, "site_starts": 396, "strand": "forward", "leaves": "5' AATT" })
    );
    assert_eq!(cuts[1]["enzyme"], "PstI");
    assert_eq!(cuts[1]["after_base"], 439);
}

#[test]
fn one_cut_in_a_circle_is_one_fragment_and_says_so() {
    let (_dir, s) = lab();
    let out = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI"] })).unwrap();
    assert_eq!(sizes(&out), vec![2686]);
    let only = fragment(&out, 2686);
    assert_eq!((only["start"].as_u64(), only["end"].as_u64(), only["through_origin"].as_bool()), (Some(397), Some(396), Some(true)));
    let notes: Vec<&str> = out["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(notes.iter().any(|n| n.contains("one linear fragment")), "{notes:?}");
}

#[test]
fn an_enzyme_that_does_not_cut_is_named() {
    let (_dir, s) = lab();
    let out = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "NotI"] })).unwrap();
    assert_eq!(out["non_cutters"], json!(["NotI"]));
    let notes: Vec<&str> = out["notes"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert!(notes.iter().any(|n| n.contains("Does not cut this DNA: NotI")), "{notes:?}");

    let uncut = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["NotI", "EcoRV"] })).unwrap();
    assert_eq!(uncut["fragment_count"], 0);
    assert_eq!(uncut["non_cutters"], json!(["NotI", "EcoRV"]));
}

#[test]
fn a_name_that_is_not_an_enzyme_is_refused() {
    let (_dir, s) = lab();
    let failed = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRl"] })).unwrap_err();
    assert!(failed.to_string().contains("not an enzyme Bios knows"), "{failed}");
}

#[test]
fn digesting_a_stretch_treats_it_as_a_linear_piece() {
    let (_dir, s) = lab();
    // The polylinker on its own, EcoRI's cut to HindIII's: 51 bases, with BamHI inside it.
    let out = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["BamHI"], "start": 397, "end": 447 })).unwrap();
    assert_eq!(out["topology"], "linear");
    assert_eq!(out["total_bp"], 51);
    assert_eq!(sizes(&out), vec![30, 21]);
    let first = fragment(&out, 21);
    assert_eq!(first["left_end"], json!({ "made_by": Value::Null, "end": "the end of the linear plasmid, as it came" }));
    assert_eq!(first["right_end"]["made_by"], "BamHI");
}

#[test]
fn a_type_iis_enzyme_cuts_outside_its_site() {
    let (_dir, s) = lab();
    let out = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19", "enzymes": ["BsaI"] })).unwrap();
    let cut = &out["cut_sites"][0];
    assert_eq!(cut["site_starts"], 1766);
    assert_eq!(cut["after_base"], 1760);
    assert_eq!(cut["strand"], "reverse");
}

/// The insert is ordered as GAATTC + 15 bases + AAGCTT; cut with both, the piece between the two
/// scissions is 21 bp, so pUC19's 2635 bp EcoRI/HindIII backbone gives a 2656 bp plasmid.
fn ordered_insert() -> &'static str {
    "GAATTCATGGGTAAACCGTAAAAGCTT"
}

#[test]
fn cloning_an_insert_into_a_double_cut_vector() {
    let (_dir, s) = lab();
    let out = s
        .execute(
            "clone_by_restriction",
            &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "sequence": ordered_insert(), "sequence_ends": ["EcoRI", "HindIII"] }),
        )
        .unwrap();

    assert_eq!(out["vector"]["backbone"], "2635 bp at 448..396");
    assert_eq!(out["vector"]["removed"], "51 bp at 397..447");
    assert_eq!(out["insert"]["length"], 21);
    assert_eq!(out["insert"]["sequence"], "AATTCATGGGTAAACCGTAAA");
    assert_eq!(out["orientations"], json!(["as given"]), "AATT one end, AGCT the other");

    let product = &out["products"][0];
    assert_eq!(product["length"], 2656);
    assert_eq!(product["topology"], "circular");
    assert_eq!(product["insert_at"], "2636..2656");
    assert_eq!(product["sequence"].as_str().unwrap().len(), 2656);

    let junctions = product["junctions"].as_array().unwrap();
    assert_eq!(junctions.len(), 2);
    assert_eq!(junctions[0]["at"], "after base 2635");
    assert_eq!(junctions[0]["joined"], "EcoRI to EcoRI");
    assert_eq!(junctions[0]["overhang"], "5' AATT");
    assert_eq!(junctions[0]["sites"], json!([{ "enzyme": "EcoRI", "site": "rebuilt: the enzyme cuts the product here again" }]));
    assert_eq!(junctions[1]["at"], "through the origin, between base 2656 and base 1");
    assert_eq!(junctions[1]["sites"][0]["enzyme"], "HindIII");
    assert!(junctions[1]["sites"][0]["site"].as_str().unwrap().starts_with("rebuilt"));

    // The vector's own features come along; the polylinker that left does not.
    let carried: Vec<&str> = out["features_on_the_product"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(carried, vec!["M13 fwd", "M13 rev", "lac operator", "lac promoter", "CAP binding site", "ori", "AmpR", "AmpR promoter", "insert"]);
    let amp = out["features_on_the_product"].as_array().unwrap().iter().find(|f| f["name"] == "AmpR").unwrap();
    // AmpR is 1626..2486 on pUC19 and the backbone starts at 448, so it moves back 447 bases.
    assert_eq!((amp["start"].as_u64(), amp["end"].as_u64()), (Some(1179), Some(2039)));
}

#[test]
fn a_different_enzyme_with_the_same_overhang_destroys_both_sites() {
    let (_dir, s) = lab();
    // BamHI (G^GATCC) opens pUC19 once; a BglII (A^GATCT) insert has the same 5' GATC ends.
    let out = s
        .execute(
            "clone_by_restriction",
            &json!({ "plasmid": "pUC19", "enzymes": ["BamHI"], "sequence": "AGATCTATGGGTAAACCGTAAAGATCT", "sequence_ends": ["BglII"] }),
        )
        .unwrap();
    assert_eq!(out["insert"]["length"], 21);
    assert_eq!(out["orientations"], json!(["as given", "turned round"]), "GATC both ends: either way round");
    let junction = &out["products"][0]["junctions"][0];
    assert_eq!(junction["joined"], "BamHI to BglII");
    let sites: Vec<&str> = junction["sites"].as_array().unwrap().iter().map(|site| site["site"].as_str().unwrap()).collect();
    assert!(sites.iter().all(|site| site.starts_with("destroyed")), "a BamHI/BglII hybrid junction is neither site: {sites:?}");

    let warnings: Vec<&str> = out["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(warnings.iter().any(|w| w.contains("can close with no insert")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("both orientations")), "{warnings:?}");
}

#[test]
fn a_blunt_insert_warns_about_orientation_and_empty_vector() {
    let (_dir, s) = lab();
    let out = s.execute("clone_by_restriction", &json!({ "plasmid": "pUC19", "enzymes": ["SmaI"], "sequence": "ATGGGTAAACCGTAAGGGG" })).unwrap();
    assert_eq!(out["products"][0]["length"], 2705);
    assert_eq!(out["orientations"], json!(["as given", "turned round"]));
    let warnings: Vec<&str> = out["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(warnings.iter().any(|w| w.contains("blunt ligation")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("can close with no insert")), "{warnings:?}");
    assert_eq!(out["products"][0]["junctions"][0]["overhang"], "blunt");
}

#[test]
fn an_insert_the_vectors_own_enzyme_would_cut_is_warned_about() {
    let (_dir, s) = lab();
    // A BglII insert with a BamHI site inside it, going into BamHI-cut pUC19.
    let donor = format!("AGATCTAAAGGATCCAAAAGATCT{}", "T".repeat(200));
    s.execute("create_plasmid", &json!({ "name": "donor", "sequence": donor, "circular": true })).unwrap();
    let out = s
        .execute("clone_by_restriction", &json!({ "plasmid": "pUC19", "enzymes": ["BamHI"], "from_plasmid": "donor", "from_enzymes": ["BglII"] }))
        .unwrap();
    assert_eq!(out["insert"]["length"], 18);
    assert_eq!(out["insert"]["sequence"], "GATCTAAAGGATCCAAAA");
    let warnings: Vec<&str> = out["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert!(
        // GGATCC sits at bases 9..14 of the insert and BamHI cuts G^GATCC, so after base 9.
        warnings.iter().any(|w| w.contains("BamHI has a site inside the insert, after base 9 of it")),
        "the digest that opened the vector would cut the insert too: {warnings:?}"
    );
}

#[test]
fn ends_that_cannot_be_ligated_are_refused_with_both_ends_named() {
    let (_dir, s) = lab();
    let failed = s
        .execute("clone_by_restriction", &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "sequence": "ATGGGTAAACCGTAA" }))
        .unwrap_err()
        .to_string();
    assert!(failed.contains("cannot be ligated"), "{failed}");
    assert!(failed.contains("blunt"), "{failed}");
    assert!(failed.contains("5' AATT") && failed.contains("5' AGCT"), "{failed}");
}

#[test]
fn an_enzyme_that_cuts_the_vector_twice_is_refused() {
    let (_dir, s) = lab();
    let failed = s
        .execute("clone_by_restriction", &json!({ "plasmid": "pUC19", "enzymes": ["DraI"], "sequence": "ATGGGTAAACCGTAA" }))
        .unwrap_err()
        .to_string();
    assert!(failed.contains("DraI cuts pUC19 3 times"), "{failed}");
}

#[test]
fn an_insert_cut_out_of_another_plasmid_carries_its_features() {
    let (_dir, s) = lab();
    let donor = format!("GAATTCATGGGTAAACCGTAAAAGCTT{}", "T".repeat(200));
    s.execute("create_plasmid", &json!({ "name": "donor", "sequence": donor, "circular": true })).unwrap();
    let marked = s.execute("read_plasmid", &json!({ "plasmid": "donor", "genbank": false })).unwrap();
    s.execute("add_feature", &json!({ "plasmid": "donor", "version": marked["version"], "name": "payload", "type": "CDS", "start": 7, "end": 21 }))
        .unwrap();

    let out = s
        .execute(
            "clone_by_restriction",
            &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "from_plasmid": "donor", "from_enzymes": ["EcoRI", "HindIII"] }),
        )
        .unwrap();
    assert_eq!(out["insert"]["length"], 21);
    assert_eq!(out["insert"]["sequence"], "AATTCATGGGTAAACCGTAAA");
    assert_eq!(out["products"][0]["length"], 2656);
    let carried: Vec<&str> = out["features_on_the_product"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(carried.contains(&"payload"), "{carried:?}");
    assert!(carried.contains(&"donor fragment"), "{carried:?}");
    let payload = out["features_on_the_product"].as_array().unwrap().iter().find(|f| f["name"] == "payload").unwrap();
    // payload is donor 7..21; the insert starts at donor base 2, so it sits 5 bases in, at
    // 2636 + 5 = 2641 on the product.
    assert_eq!((payload["start"].as_u64(), payload["end"].as_u64()), (Some(2641), Some(2655)));
}

#[test]
fn an_ambiguous_insert_is_refused_with_the_candidates_listed() {
    let (_dir, s) = lab();
    // Three cuts leave three fragments, and two of them are neither the backbone.
    let donor = format!("GAATTCATGGGTAAACCGTAAAAGCTTGGGGGAATTC{}", "T".repeat(200));
    s.execute("create_plasmid", &json!({ "name": "donor", "sequence": donor, "circular": true })).unwrap();
    let failed = s
        .execute(
            "clone_by_restriction",
            &json!({ "plasmid": "pUC19", "enzymes": ["EcoRI", "HindIII"], "from_plasmid": "donor", "from_enzymes": ["EcoRI", "HindIII"] }),
        )
        .unwrap_err()
        .to_string();
    assert!(failed.contains("more than one of them could be the insert"), "{failed}");
    assert!(failed.contains("bp at"), "{failed}");
}

#[test]
fn a_stretch_of_another_plasmid_comes_with_blunt_ends() {
    let (_dir, s) = lab();
    let out = s
        .execute(
            "clone_by_restriction",
            &json!({ "plasmid": "pUC19", "enzymes": ["SmaI"], "from_plasmid": "pUC19", "from_start": 379, "from_end": 395 }),
        )
        .unwrap();
    assert_eq!(out["insert"]["length"], 17);
    assert_eq!(out["insert"]["ends"][0], json!({ "made_by": Value::Null, "end": "blunt" }));
    assert_eq!(out["products"][0]["length"], 2703);
    let carried: Vec<&str> = out["features_on_the_product"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(carried.contains(&"M13 fwd"), "the primer_bind inside 379..395 comes along: {carried:?}");
}

#[test]
fn saving_the_clone_keeps_it_as_a_new_plasmid() {
    let (_dir, s) = lab();
    let made = s
        .execute(
            "save_restriction_clone",
            &json!({
                "plasmid": "pUC19",
                "enzymes": ["EcoRI", "HindIII"],
                "sequence": ordered_insert(),
                "sequence_ends": ["EcoRI", "HindIII"],
                "name": "pUC19-GFP",
                "folder": "clones",
            }),
        )
        .unwrap();
    assert_eq!(made["created"], "clones/pUC19-GFP");
    assert_eq!(made["length"], 2656);
    assert_eq!(made["made_from"], "pUC19");

    let read = s.execute("read_plasmid", &json!({ "plasmid": "pUC19-GFP", "genbank": false })).unwrap();
    assert_eq!(read["length"], 2656);
    assert_eq!(read["topology"], "circular");
    let names: Vec<&str> = read["features"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"AmpR") && names.contains(&"insert"), "{names:?}");
    assert!(!names.contains(&"MCS"), "the polylinker left with the 51 bp piece: {names:?}");

    // The saved plasmid really is the designed product: both sites cut it back open.
    let back = s.execute("digest_plasmid", &json!({ "plasmid": "pUC19-GFP", "enzymes": ["EcoRI", "HindIII"] })).unwrap();
    assert_eq!(sizes(&back), vec![2635, 21]);

    // And it is journaled as one change made from pUC19.
    let history = s.execute("plasmid_history", &json!({ "plasmid": "pUC19-GFP" })).unwrap();
    let entries = history["history"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["command"], "save_restriction_clone");
    assert!(entries[0]["summary"].as_str().unwrap().contains("cloned into pUC19"), "{:?}", entries[0]);
}
