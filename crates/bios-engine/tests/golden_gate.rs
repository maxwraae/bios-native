//! Golden Gate assembly as an agent calls it through `Session::execute`: the design, the parts
//! taken from other plasmids in the project, and the product kept as a new plasmid.
//!
//! The destination vector is the same 152 bp construct `bios-core`'s `golden_gate` tests build, so
//! every number below is a consequence of where its two BsaI sites were put. pUC19 (GenBank
//! L09137) stands in where a real sequence has something to say.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

/// Sixty bases of backbone, AATG and a reverse BsaI site, a 20 bp stuffer, a forward BsaI site and
/// CGCT, then fifty bases of backbone. See `bios-core`'s `golden_gate` tests for the drawing.
const DEST: &str = concat!(
    "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACTTGTTGGCCCAGTGTGAATCG",
    "AATGTGAGACC",
    "CTTAAGGGTTAAGTAAGTGT",
    "GGTCTCACGCT",
    "GATGCATACGCCTTTACTTGCTGTGTCCACCCCATCGGACTGGCATTTTT",
);
const PART_A: &str = "AATGATTACACTCAGAAACAGAACTCGGAG";
const PART_B: &str = "GGAGGGGTAATTTTGACAGGTTCG";
const PART_C: &str = "TTCGTCACGCAGAGGCGCGCCCTCCTGAAGTGCGCT";

fn lab() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    s.execute("create_plasmid", &json!({ "name": "pGG-dest", "sequence": DEST, "circular": true })).unwrap();
    (dir, s)
}

fn three_parts() -> Value {
    json!([
        { "name": "Part A", "sequence": PART_A },
        { "name": "Part B", "sequence": PART_B },
        { "name": "Part C", "sequence": PART_C },
    ])
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_default()
}

fn strings(v: &Value) -> Vec<&str> {
    v.as_array().map(|a| a.iter().map(text).collect()).unwrap_or_default()
}

#[test]
fn the_overhangs_decide_the_order_and_the_design_says_what_to_order() {
    let (_dir, s) = lab();
    // Given out of order on purpose: the overhangs, not the list, decide.
    let parts = json!([
        { "name": "Part C", "sequence": PART_C },
        { "name": "Part A", "sequence": PART_A },
        { "name": "Part B", "sequence": PART_B },
    ]);
    let out = s.execute("design_golden_gate", &json!({ "plasmid": "pGG-dest", "parts": parts })).unwrap();
    let d = &out["design"];
    assert_eq!(text(&out["plasmid"]), "pGG-dest");
    assert_eq!(text(&d["enzyme"]["name"]), "BsaI", "BsaI by default");
    assert_eq!(text(&d["enzyme"]["cuts"]), "GGTCTC(1/5)");
    assert_eq!(d["enzyme"]["overhang_length"], 4);
    assert_eq!(strings(&d["assembly_order"]), ["pGG-dest backbone", "Part A", "Part B", "Part C"]);
    assert_eq!(d["assembles"], true);

    assert_eq!(text(&d["vector"]["opened"]), "61..98");
    assert_eq!(d["vector"]["removed_length"], 38);
    assert_eq!(text(&d["vector"]["overhang_the_first_part_joins"]), "AATG");
    assert_eq!(text(&d["vector"]["overhang_the_last_part_joins"]), "CGCT");

    // The retained backbone, then each part's own bases less the four it shares with the next.
    let expected = format!("{}{}{}{}CGCT{}", &DEST[..60], &PART_A[..26], &PART_B[..20], &PART_C[..32], &DEST[102..]);
    assert_eq!(text(&d["product"]["sequence"]), expected);
    assert_eq!(d["product"]["length"], 192);
    assert_eq!(text(&d["product"]["topology"]), "circular");

    let junctions: Vec<(&str, &str)> = d["junctions"].as_array().unwrap().iter().map(|j| (text(&j["overhang"]), text(&j["position"]))).collect();
    assert_eq!(junctions, [("AATG", "61..64"), ("GGAG", "87..90"), ("TTCG", "107..110"), ("CGCT", "139..142")]);
    assert!(d["junctions"].as_array().unwrap().iter().all(|j| j["unique"] == true && j["palindromic"] == false));

    // Every part quoted with the tails it needs, ready to paste into an order form.
    let to_order: Vec<(&str, &str)> = d["to_order"].as_array().unwrap().iter().map(|p| (text(&p["name"]), text(&p["sequence"]))).collect();
    assert_eq!(
        to_order,
        [
            ("Part C", &*format!("GGTCTCA{PART_C}AGAGACC")),
            ("Part A", &*format!("GGTCTCA{PART_A}AGAGACC")),
            ("Part B", &*format!("GGTCTCA{PART_B}AGAGACC")),
        ]
    );
    assert_eq!(strings(&d["warnings"]), Vec::<&str>::new());
}

/// A part can be a stretch of another plasmid in the project; its features inside the stretch come
/// onto the product at their new positions.
#[test]
fn a_part_taken_from_another_plasmid_brings_its_features() {
    let (_dir, s) = lab();
    s.execute(
        "create_plasmid",
        &json!({ "name": "pPartB", "folder": "parts", "sequence": format!("AAAAACCCCC{PART_B}GGGGGTTTTT"), "circular": false }),
    )
    .unwrap();
    let version = s.execute("read_plasmid", &json!({ "plasmid": "pPartB", "genbank": false })).unwrap()["version"].clone();
    s.execute("add_feature", &json!({ "plasmid": "pPartB", "version": version, "name": "B body", "start": 15, "end": 30 })).unwrap();

    let parts = json!([
        { "name": "Part A", "sequence": PART_A },
        { "name": "Part B", "plasmid": "parts/pPartB", "start": 11, "end": 34 },
        { "name": "Part C", "sequence": PART_C },
    ]);
    let out = s.execute("design_golden_gate", &json!({ "plasmid": "pGG-dest", "parts": parts })).unwrap();
    let d = &out["design"];
    assert_eq!(strings(&d["assembly_order"]), ["pGG-dest backbone", "Part A", "Part B", "Part C"]);
    // Part B sits at 87..110, and "B body" was 5 bases into the stretch, so it lands at 91..106.
    let placed: Vec<(&str, i64)> =
        d["placed"].as_array().unwrap().iter().map(|p| (text(&p["name"]), p["start"].as_i64().unwrap_or_default())).collect();
    assert_eq!(placed, [("Part A", 61), ("Part B", 87), ("Part C", 107)]);
    let body = d["features"].as_array().unwrap().iter().find(|f| text(&f["name"]) == "B body").expect("B body came along");
    assert_eq!((body["start"].as_i64(), body["end"].as_i64()), (Some(91), Some(106)));
}

/// Saving goes through the project the way every other new plasmid does: it is there afterwards
/// with the product's bases, it says what it was built from, and the history has one entry.
#[test]
fn the_product_is_kept_as_a_new_plasmid_with_its_lineage() {
    let (_dir, s) = lab();
    let args = json!({ "plasmid": "pGG-dest", "name": "pGG-ABC", "folder": "assemblies", "parts": three_parts() });
    let made = s.execute("save_golden_gate_product", &args).unwrap();
    assert_eq!(text(&made["created"]), "assemblies/pGG-ABC");
    assert_eq!(made["length"], 192);
    assert_eq!(text(&made["made_from"]), "pGG-dest");
    assert_eq!(text(&made["design"]["product"]["sequence"]).len(), 192);

    let read = s.execute("read_plasmid", &json!({ "plasmid": "pGG-ABC", "genbank": false })).unwrap();
    assert_eq!(read["length"], 192);
    assert_eq!(text(&read["topology"]), "circular");
    let kept = s.execute("get_sequence", &json!({ "plasmid": "pGG-ABC", "start": 1, "end": 192 })).unwrap();
    assert_eq!(text(&kept["sequence"]), text(&made["design"]["product"]["sequence"]));
    // One feature per part, where the design put it.
    let features: Vec<(&str, i64, i64)> = made["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (text(&f["name"]), f["start"].as_i64().unwrap_or_default(), f["end"].as_i64().unwrap_or_default()))
        .collect();
    assert_eq!(features, [("Part A", 61, 90), ("Part B", 87, 110), ("Part C", 107, 142)]);

    let history = s.execute("plasmid_history", &json!({ "plasmid": "pGG-ABC" })).unwrap();
    let entries = history["history"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(text(&entries[0]["command"]), "save_golden_gate_product");
    assert_eq!(text(&entries[0]["summary"]), "BsaI assembly of Part A, Part B, Part C into pGG-dest (192 bp)");
}

/// A set that does not close is designed and reported, but never saved.
#[test]
fn a_set_that_does_not_close_is_reported_and_not_saved() {
    let (_dir, s) = lab();
    let parts = json!([{ "name": "Part A", "sequence": PART_A }]);
    let out = s.execute("design_golden_gate", &json!({ "plasmid": "pGG-dest", "parts": parts.clone() })).unwrap();
    let d = &out["design"];
    assert_eq!(d["assembles"], false);
    assert!(d["product"].is_null());
    let said = strings(&d["warnings"]).join("\n");
    assert!(said.contains("the 3' overhang GGAG of \"Part A\" matches no part's 5' overhang"), "{said}");

    let err = s.execute("save_golden_gate_product", &json!({ "plasmid": "pGG-dest", "name": "nope", "parts": parts })).unwrap_err();
    assert!(err.to_string().contains("nothing was saved: the parts do not form one circular product"), "{err}");
    assert!(err.to_string().contains("overhang GGAG"), "{err}");
    let listed = s.execute("list_plasmids", &json!({})).unwrap();
    assert!(!format!("{listed}").contains("nope"), "nothing was created: {listed}");
}

/// An enzyme that cannot hold the assembly together is refused by name, before any design.
#[test]
fn an_enzyme_that_cannot_do_golden_gate_is_refused() {
    let (_dir, s) = lab();
    let ask = |enzyme: &str| {
        s.execute("design_golden_gate", &json!({ "plasmid": "pGG-dest", "enzyme": enzyme, "parts": three_parts() }))
            .map_err(|e| e.to_string())
            .expect_err("refused")
    };
    assert!(ask("EcoRI").contains("cuts inside its own recognition site"), "{}", ask("EcoRI"));
    assert!(ask("Nonesuch").contains("not an enzyme Bios knows"), "{}", ask("Nonesuch"));
    // BsmBI is a perfectly good Golden Gate enzyme; it just does not cut this vector.
    assert!(ask("BsmBI").contains("does not cut this plasmid"), "{}", ask("BsmBI"));
}

/// A real sequence: pUC19 carries exactly two BsmBI sites, pointing outward, so BsmBI opens it and
/// the arc free of those sites — the short one, 46 bp — is the backbone. Its 5' overhang, CGCG,
/// reads the same on both strands, which the design says out loud.
#[test]
fn puc19_assembles_with_bsmbi_and_its_palindromic_overhang_is_called_out() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures/pUC19.gb");
    s.execute("import_plasmid", &json!({ "file": file.display().to_string() })).unwrap();

    let parts = json!([{ "name": "linker", "sequence": "TCCCATCCTAGCCTAGCTACGCGTCGCG" }]);
    let out = s.execute("design_golden_gate", &json!({ "plasmid": "pUC19", "enzyme": "BsmBI", "parts": parts })).unwrap();
    let d = &out["design"];
    assert_eq!(text(&d["enzyme"]["cuts"]), "CGTCTC(1/5)");
    assert_eq!(text(&d["vector"]["opened"]), "46..3");
    assert_eq!(d["vector"]["through_origin"], true);
    assert_eq!(d["vector"]["removed_length"], 2644);
    assert_eq!(d["vector"]["kept_length"], 46);
    assert_eq!(text(&d["vector"]["overhang_the_first_part_joins"]), "TCCC");
    assert_eq!(text(&d["vector"]["overhang_the_last_part_joins"]), "CGCG");
    // The part's own bases less the shared four, then the 42 bp arc pUC19 keeps.
    let arc = s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": 4, "end": 45 })).unwrap();
    assert_eq!(text(&d["product"]["sequence"]), format!("TCCCATCCTAGCCTAGCTACGCGT{}", text(&arc["sequence"])));
    assert_eq!(d["product"]["length"], 66);
    let said = strings(&d["warnings"]).join("\n");
    assert!(said.contains("The CGCG junction between \"linker\" and \"pUC19 backbone\" at 25..28"), "{said}");
    assert!(said.contains("reads the same on both strands"), "{said}");
    assert!(said.contains("a part can go in backwards"), "{said}");
}
