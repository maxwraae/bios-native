//! Binders and Find's alignment, as an agent calls them through `Session::execute`.

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

fn with_plasmid() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures/pUC19.gb");
    s.execute("import_plasmid", &json!({ "file": file.display().to_string() })).unwrap();
    (dir, s)
}

fn bases(s: &Session, start: i64, end: i64) -> String {
    s.execute("get_sequence", &json!({ "plasmid": "pUC19", "start": start, "end": end })).unwrap()["sequence"].as_str().unwrap().to_string()
}

#[test]
fn a_binder_lands_where_it_anneals_and_names_its_tail_without_touching_the_file() {
    let (_dir, s) = with_plasmid();
    let before = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap()["version"].clone();
    // The bottom-strand oligo over 301..322, carrying a 24-base 5' tail.
    let anneal = bios_core::seq::revcomp(&bases(&s, 301, 322));
    let tailed = format!("GCTAGCGGATCCGCTAGCGGATCC{anneal}");
    let out = s.execute("binder_sites", &json!({ "plasmid": "pUC19", "oligos": [tailed], "names": ["fwd"] })).unwrap();
    let b = &out["binders"][0];
    assert_eq!(b["name"], "fwd");
    assert_eq!(b["length"], 46);
    let site = &b["sites"][0];
    assert_eq!((site["start"].as_i64(), site["end"].as_i64(), site["strand"].as_str()), (Some(301), Some(322), Some("reverse")));
    assert_eq!((site["tail_length"].as_i64(), site["anneal_length"].as_i64()), (Some(24), Some(22)));
    assert_eq!(site["label"], "fwd — 24nt 5' tail");
    assert_eq!(site["on_target"], true);
    let after = s.execute("read_plasmid", &json!({ "plasmid": "pUC19" })).unwrap()["version"].clone();
    assert_eq!(before, after, "a binder never reaches the file");
}

#[test]
fn a_binder_needs_bases() {
    let (_dir, s) = with_plasmid();
    assert!(s.execute("binder_sites", &json!({ "plasmid": "pUC19", "oligos": ["12 -"] })).is_err());
}

#[test]
fn an_alignment_reports_the_fit_with_its_caption_and_says_the_size_it_admits() {
    let (_dir, s) = with_plasmid();
    let piece = bases(&s, 1001, 1030);
    let with_insert = format!("{}GATTCA{}", &piece[..15], &piece[15..]);
    let out = s.execute("align_sequence", &json!({ "plasmid": "pUC19", "query": with_insert })).unwrap();
    let best: &Value = &out["hits"][0];
    assert_eq!((best["start"].as_i64(), best["end"].as_i64(), best["strand"].as_str()), (Some(1001), Some(1030), Some("forward")));
    let caption = best["caption"].as_str().unwrap();
    assert!(caption.starts_with("30/36 nt aligned"), "{caption}");
    // The six extra bases, named by their bases; where the gap sits among equal bases is the aligner's choice.
    assert!(caption.contains(" extra") && !caption.contains("mismatch"), "{caption}");

    assert!(s.execute("align_sequence", &json!({ "plasmid": "pUC19", "query": "ACGTA" })).is_err());
    let long = "ACGT".repeat(3_000);
    let refused = s.execute("align_sequence", &json!({ "plasmid": "pUC19", "query": long })).unwrap_err();
    assert!(refused.0.contains("up to"), "{}", refused.0);
}
