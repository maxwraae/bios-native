//! A build of Bios without a read mapper: the read-mapping commands are still there and say so.
#![cfg(not(feature = "mapper"))]

use std::path::Path;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

const NO_MAPPER: &str = "Read mapping is not in this build of Bios yet.";

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

fn ok(s: &Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None });
    ok(&s, "import_plasmid", json!({ "file": fixture("pUC19.gb") }));
    (dir, s)
}

#[test]
fn map_reads_is_a_command_and_says_read_mapping_is_not_in_this_build() {
    let (dir, s) = session();
    assert!(bios_engine::registry().any(|c| c.name == "map_reads"));
    let reads = dir.path().join("reads.fastq");
    std::fs::write(&reads, "@r/1\nACGT\n+\nIIII\n@r/2\nACGT\n+\nIIII\n").unwrap();
    let err = s.execute("map_reads", &json!({ "plasmid": "pUC19", "reads": reads.display().to_string() })).unwrap_err();
    assert_eq!(err.0, NO_MAPPER);
}

#[test]
fn a_verification_run_says_read_mapping_is_not_in_this_build_and_the_sample_waits() {
    let (dir, s) = session();
    let folder = ok(&s, "create_verification", json!({ "name": "R" }))["created"].as_str().unwrap().to_string();
    ok(&s, "add_verification_map", json!({ "folder": folder, "plasmid": "pUC19" }));
    let reads = dir.path().join("pUC19_A1.fastq");
    std::fs::write(&reads, "@r/1\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n@r/2\nACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIII\n").unwrap();
    ok(&s, "add_reads", json!({ "folder": folder, "files": [reads.display().to_string()], "map": "pUC19" }));
    let ran = ok(&s, "run_verification", json!({ "folder": folder }));
    assert_eq!(ran["failed"][0]["error"], NO_MAPPER, "{ran}");
    let read = ok(&s, "read_verification", json!({ "folder": folder }));
    assert_eq!(read["maps"][0]["waiting"].as_array().unwrap().len(), 1);
}
