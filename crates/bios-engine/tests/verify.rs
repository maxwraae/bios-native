//! Verify through `Session::execute`: a run dropped in, placed, aligned, called and exported.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use bios_core::verify::mapper::{FinalConsensus, MapperManifest, MapperOutput, MapperRow};
use bios_engine::Session;
use bios_engine::verify::{Control, MapJob, ReadMapper};
use bios_project::{Actor, Project, Who};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

/// A mapper that "finds" one substitution at base 11 and covers every base five deep.
struct OneChange;

impl ReadMapper for OneChange {
    fn map(&self, job: &MapJob, control: &Control) -> Result<MapperOutput, String> {
        if control.cancelled() {
            return Err("cancelled".into());
        }
        let pairs = (job.reads.count() / 2) as u64;
        let bases = job.reference.bases.clone();
        let mut consensus: Vec<char> = bases.chars().collect();
        consensus[10] = if consensus[10] == 'A' { 'C' } else { 'A' };
        let n = bases.len();
        Ok(MapperOutput {
            schema: 1,
            reference: job.reference.clone(),
            padded_reference: bases.clone(),
            rows: vec![MapperRow {
                index: 0,
                name: "read 1".into(),
                leading: 0,
                internal: bases[..50].to_string(),
                quality: "I".repeat(50),
                reverse: false,
                mate_index: None,
                trim_left: 0,
                trim_right: 0,
                mapping_quality: 60,
                mapped_locations: 1,
            }],
            final_consensus: FinalConsensus {
                consensus_aligned: consensus.into_iter().collect(),
                coverage: vec![5.0; n],
                quality: vec![30.0; n],
                column_counts: Vec::new(),
                metadata: Default::default(),
            },
            manifest: MapperManifest {
                pair_count: pairs,
                mapped_records: 1,
                completed_iterations: 1,
                elapsed_seconds: 0.1,
                alignment_sha256: "x".into(),
            },
            source: Default::default(),
            sam: "@HD\tVN:1.6\n".into(),
            consensus_fasta: ">consensus\nACGT\n".into(),
            coverage_tsv: "alignment_column\tcoverage\n".into(),
            differences_tsv: "alignment_column\treference\n11\tX\n".into(),
        })
    }
}

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let who = Who { actor: Actor::Human, source: "test".into(), session: None };
    (dir, Session::new(project, who).with_mapper(Arc::new(OneChange)))
}

/// A gzipped FASTQ of `n` Illumina records for `read` (1 or 2), written in two gzip members.
fn reads(dir: &Path, name: &str, read: u8, n: usize) -> PathBuf {
    let mut text = String::new();
    for i in 0..n {
        text += &format!("@M1:1:FC:1:1101:{}:100 {read}:N:0:ACGT\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n", 1000 + i);
    }
    let (a, b) = text.split_at(text.len() / 2);
    let mut bytes = Vec::new();
    for part in [a, b] {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(part.as_bytes()).unwrap();
        bytes.extend(gz.finish().unwrap());
    }
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn ok(s: &Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

/// A verification holding pUC19 as its map and two pairs of reads, one named for the map.
fn plate() -> (tempfile::TempDir, Session, String) {
    let (dir, s) = session();
    ok(&s, "import_plasmid", json!({ "file": fixture("pUC19.gb"), "folder": "backbones" }));
    let folder = ok(&s, "create_verification", json!({ "name": "Plate 3" }))["created"].as_str().unwrap().to_string();
    assert_eq!(folder, "Plate 3");
    ok(&s, "add_verification_map", json!({ "folder": folder, "plasmid": "pUC19" }));
    let files: Vec<String> = [
        reads(dir.path(), "pUC19_P001_A1_S1_R1_001.fastq.gz", 1, 40),
        reads(dir.path(), "pUC19_P001_A1_S1_R2_001.fastq.gz", 2, 40),
        reads(dir.path(), "other_P001_B1_S2_R1_001.fastq.gz", 1, 10),
        reads(dir.path(), "other_P001_B1_S2_R2_001.fastq.gz", 2, 10),
    ]
    .iter()
    .map(|p| p.display().to_string())
    .collect();
    let added = ok(&s, "add_reads", json!({ "folder": folder, "files": files }));
    assert_eq!(added["added"].as_array().unwrap().len(), 2, "{added}");
    (dir, s, folder)
}

#[test]
fn a_run_is_made_named_and_listed() {
    let (_dir, s) = session();
    let made = ok(&s, "create_verification", json!({}));
    let name = made["created"].as_str().unwrap();
    assert!(name.split(' ').count() == 2, "today's date: {name}");
    let again = ok(&s, "create_verification", json!({}));
    assert_eq!(again["created"].as_str().unwrap(), format!("{name} (2)"));
    let listed = ok(&s, "list_verifications", json!({}));
    assert_eq!(listed["verifications"].as_array().unwrap().len(), 2);
}

#[test]
fn reads_are_paired_kept_and_placed_by_name_and_the_rest_wait_in_the_tray() {
    let (_dir, s, folder) = plate();
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    let row = &read["maps"][0];
    assert_eq!(row["map"], "pUC19");
    assert_eq!(row["waiting"][0]["sample"], "pUC19_P001_A1_S1");
    assert_eq!(row["waiting"][0]["files"].as_array().unwrap().len(), 2);
    assert_eq!(read["not_placed"][0]["sample"], "other_P001_B1_S2");
    assert_eq!(read["reads"].as_array().unwrap().len(), 4);
}

#[test]
fn a_person_moves_a_sample_and_auto_match_leaves_it_where_they_put_it() {
    let (_dir, s, folder) = plate();
    ok(&s, "place_samples", json!({ "folder": folder, "samples": ["pUC19_P001_A1_S1"] }));
    let moved = ok(&s, "match_samples", json!({ "folder": folder }));
    // Auto-match is asking on purpose: it takes the loose ones, including those taken off by hand.
    assert_eq!(moved["placed"], 1);
    ok(&s, "place_samples", json!({ "folder": folder, "samples": ["other_P001_B1_S2"], "map": "pUC19" }));
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert_eq!(read["maps"][0]["waiting"].as_array().unwrap().len(), 2);
    ok(&s, "remove_sample", json!({ "folder": folder, "sample": "other_P001_B1_S2" }));
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert_eq!(read["reads"].as_array().unwrap().len(), 2, "leaving a sample out deletes its reads");
}

#[test]
fn running_aligns_what_is_placed_keeps_it_and_says_what_it_found() {
    let (_dir, s, folder) = plate();
    let ran = ok(&s, "run_verification", json!({ "folder": folder }));
    assert_eq!(ran["aligned"].as_array().unwrap().len(), 1, "{ran}");
    assert_eq!(ran["aligned"][0]["found"], "1 consensus differences");
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert!(read["maps"][0]["waiting"].as_array().unwrap().is_empty());
    assert_eq!(read["maps"][0]["aligned"][0]["stale"], false);
    assert_eq!(read["not_placed"].as_array().unwrap().len(), 1, "the tray stays");

    let alignment = ok(&s, "read_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" }));
    assert_eq!(alignment["differences"][0]["position"], 11);
    assert_eq!(alignment["reads"]["total"], 80);
    assert_eq!(alignment["call"], Value::Null, "the app never fills in a call");
}

#[test]
fn a_call_is_kept_apart_from_the_finding_and_the_picks_are_the_good_ones() {
    let (dir, s, folder) = plate();
    ok(&s, "run_verification", json!({ "folder": folder }));
    let called = ok(&s, "call_sample", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1", "call": "good" }));
    assert_eq!(called["call"], "good");
    assert_eq!(called["next_to_call"], Value::Null);
    let read = ok(&s, "read_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" }));
    assert_eq!(read["found"], "1 consensus differences");
    assert_eq!(read["overruled"], true);
    let picks = ok(&s, "copy_picks", json!({ "folder": folder }));
    assert_eq!(picks["text"], "pUC19_P001_A1_S1_R1_001.fastq.gz\tpUC19");

    let zip = dir.path().join("out/alignment.zip");
    ok(&s, "export_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1", "what": "zip", "path": zip.display().to_string() }));
    assert!(std::fs::read(&zip).unwrap().starts_with(b"PK\x03\x04"));
    let fasta = dir.path().join("out/consensus.fasta");
    ok(&s, "export_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1", "what": "consensus", "path": fasta.display().to_string() }));
    assert_eq!(std::fs::read_to_string(&fasta).unwrap(), ">consensus\nACGT\n");

    // Run again replaces the alignment and clears the call: it is a new measurement.
    ok(&s, "rerun_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" }));
    let read = ok(&s, "read_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" }));
    assert_eq!(read["call"], Value::Null);
    ok(&s, "delete_alignment", json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" }));
    assert!(s.execute("read_alignment", &json!({ "folder": folder, "sample": "pUC19_P001_A1_S1" })).is_err());
}

#[test]
fn changing_the_map_marks_the_old_alignments_stale() {
    let (_dir, s, folder) = plate();
    ok(&s, "run_verification", json!({ "folder": folder }));
    ok(&s, "import_plasmid", json!({ "file": fixture("wrap_demo.gb") }));
    ok(&s, "change_verification_map", json!({ "folder": folder, "map": "pUC19", "plasmid": "wrap_demo" }));
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert_eq!(read["maps"][0]["map"], "wrap_demo");
    assert_eq!(read["maps"][0]["aligned"][0]["stale"], true);
}

#[test]
fn a_cancelled_run_stops_and_says_so() {
    let (_dir, s, folder) = plate();
    let cancel = Arc::new(AtomicBool::new(true));
    let ran = s.execute_with("run_verification", &json!({ "folder": folder }), Control::new(cancel, |_| {})).unwrap();
    assert_eq!(ran["cancelled"], true);
    assert!(ran["aligned"].as_array().unwrap().is_empty());
}

#[test]
fn without_a_mapper_nothing_is_aligned_and_the_sample_waits() {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Human, source: "test".into(), session: None })
        .with_mapper(Arc::new(bios_engine::verify::Unavailable));
    ok(&s, "import_plasmid", json!({ "file": fixture("pUC19.gb") }));
    let folder = ok(&s, "create_verification", json!({ "name": "R" }))["created"].as_str().unwrap().to_string();
    ok(&s, "add_verification_map", json!({ "folder": folder, "plasmid": "pUC19" }));
    let r1 = reads(dir.path(), "pUC19_A1_R1.fastq.gz", 1, 4);
    let r2 = reads(dir.path(), "pUC19_A1_R2.fastq.gz", 2, 4);
    ok(&s, "add_reads", json!({ "folder": folder, "files": [r1.display().to_string(), r2.display().to_string()], "map": "pUC19" }));
    let ran = ok(&s, "run_verification", json!({ "folder": folder }));
    assert!(ran["failed"][0]["error"].as_str().unwrap().contains("Read mapping is not in this build of Bios yet."));
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert_eq!(read["maps"][0]["waiting"].as_array().unwrap().len(), 1);
}

#[test]
fn a_verification_is_renamed_and_deleted_with_its_maps_but_not_the_original() {
    let (_dir, s, folder) = plate();
    let renamed = ok(&s, "rename_verification", json!({ "folder": folder, "name": "Plate 3 redo" }));
    assert_eq!(renamed["folder"], "Plate 3 redo");
    assert!(s.execute("delete_verification", &json!({ "folder": "Plate 3 redo", "confirm": false })).is_err());
    ok(&s, "delete_verification", json!({ "folder": "Plate 3 redo", "confirm": true }));
    let listed = ok(&s, "list_plasmids", json!({}));
    let text = listed.to_string();
    assert!(text.contains("backbones/pUC19"), "{text}");
    assert_eq!(ok(&s, "list_verifications", json!({}))["verifications"].as_array().unwrap().len(), 0);
}
