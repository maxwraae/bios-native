//! `map_reads` through `Session::execute`, in a build with the `mapper` feature.
#![cfg(feature = "mapper")]

use std::io::Write;
use std::path::{Path, PathBuf};

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use serde_json::{Value, json};

const FILES: [&str; 7] =
    ["alignment.sam", "sam-rows.jsonl", "reference-aligned.seq", "final-consensus.json", "consensus.fasta", "coverage.tsv", "column-differences.tsv"];

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

fn empty_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    (dir, Session::new(project, Who { actor: Actor::Agent, source: "test".into(), session: None }))
}

/// A session holding pUC19 (circular, 2686 bp), and its bases uppercase.
fn session() -> (tempfile::TempDir, Session, Vec<u8>) {
    let (dir, s) = empty_session();
    s.execute("import_plasmid", &json!({ "file": fixture("pUC19.gb") })).unwrap();
    let bases = bios_core::genbank::read(&std::fs::read_to_string(fixture("pUC19.gb")).unwrap()).unwrap().sequence.to_uppercase().into_bytes();
    assert_eq!(bases.len(), 2686);
    (dir, s, bases)
}

fn reverse_complement(bases: &[u8]) -> Vec<u8> {
    bases
        .iter()
        .rev()
        .map(|b| match b {
            b'A' => b'T',
            b'C' => b'G',
            b'G' => b'C',
            _ => b'A',
        })
        .collect()
}

/// R1 and R2 FASTQ text for pairs read off `template` as a circle: a 300-base fragment starting
/// every 40 bases, R1 its first 100 bases, R2 the reverse complement of its last 100.
fn build_pairs(template: &[u8]) -> (String, String, usize) {
    let circle = [template, template].concat();
    let quality = "I".repeat(100);
    let (mut r1, mut r2) = (String::new(), String::new());
    let mut count = 0;
    for start in (0..template.len()).step_by(40) {
        let fragment = &circle[start..start + 300];
        let first = String::from_utf8(fragment[..100].to_vec()).unwrap();
        let second = String::from_utf8(reverse_complement(&fragment[200..])).unwrap();
        r1 += &format!("@frag{count}/1\n{first}\n+\n{quality}\n");
        r2 += &format!("@frag{count}/2\n{second}\n+\n{quality}\n");
        count += 1;
    }
    (r1, r2, count)
}

/// The pairs interleaved into one plain FASTQ file in `dir`.
fn interleaved(dir: &Path, r1: &str, r2: &str) -> PathBuf {
    let records = |text: &str| text.lines().collect::<Vec<_>>().chunks(4).map(|c| c.join("\n") + "\n").collect::<Vec<_>>();
    let text: String = records(r1).into_iter().zip(records(r2)).map(|(a, b)| a + &b).collect();
    let path = dir.join("reads.fastq");
    std::fs::write(&path, text).unwrap();
    path
}

fn gzipped(dir: &Path, name: &str, text: &str) -> PathBuf {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, encoder.finish().unwrap()).unwrap();
    path
}

fn map(s: &Session, params: Value) -> Value {
    s.execute("map_reads", &params).unwrap_or_else(|e| panic!("map_reads: {e}"))
}

#[test]
fn map_reads_writes_the_mappers_files_and_summarizes_them() {
    let (dir, s, bases) = session();
    let (r1, r2, pairs) = build_pairs(&bases);
    let reads = interleaved(dir.path(), &r1, &r2).display().to_string();
    let output = dir.path().join("results");
    let result = map(&s, json!({ "plasmid": "pUC19", "reads": reads, "output": output.display().to_string() }));
    assert_eq!(result["status"], "mapped");
    assert_eq!(result["pairs"], pairs);
    assert_eq!(result["mapped_reads"], 2 * pairs, "every read is an exact copy of pUC19");
    assert_eq!(result["plasmid_length"], 2686);
    // A fragment starts every 40 bases round the circle, so every base is read.
    assert_eq!(result["covered_bases"], 2686);
    assert_eq!(result["uncovered_bases"], 0);
    assert_eq!(result["difference_count"], 0);
    let files: Vec<PathBuf> = result["files"].as_array().unwrap().iter().map(|f| PathBuf::from(f.as_str().unwrap())).collect();
    assert_eq!(files, FILES.map(|name| output.join(name)));
    let consensus = std::fs::read_to_string(output.join("consensus.fasta")).unwrap();
    assert!(consensus.starts_with(">pUC19 "), "{consensus}");
    assert_eq!(consensus.lines().skip(1).collect::<String>().into_bytes(), bases, "the consensus is pUC19");
    // The results are never written over.
    assert!(s.execute("map_reads", &json!({ "plasmid": "pUC19", "reads": reads, "output": output.display().to_string() })).is_err());
}

#[test]
fn map_reads_finds_a_planted_substitution_at_its_position() {
    let (dir, s, bases) = session();
    let mut template = bases.clone();
    let was = template[999];
    let planted = if was == b'A' { b'C' } else { b'A' };
    template[999] = planted;
    let (r1, r2, pairs) = build_pairs(&template);
    let reads = interleaved(dir.path(), &r1, &r2).display().to_string();
    let result = map(&s, json!({ "plasmid": "pUC19", "reads": reads, "output": dir.path().join("out").display().to_string() }));
    assert_eq!(result["mapped_reads"], 2 * pairs);
    assert_eq!(result["difference_count"], 1, "{}", result["differences"]);
    let found = &result["differences"][0];
    assert_eq!(found["position"], 1000);
    assert_eq!(found["kind"], "substitution");
    assert_eq!(found["reference"], (was as char).to_string());
    assert_eq!(found["consensus"], (planted as char).to_string());
}

#[test]
fn map_reads_takes_gzip_r1_and_r2_and_gives_what_interleaved_gives() {
    let (dir, s, bases) = session();
    let (r1, r2, pairs) = build_pairs(&bases);
    let plain = dir.path().join("plain");
    map(&s, json!({ "plasmid": "pUC19", "reads": interleaved(dir.path(), &r1, &r2).display().to_string(), "output": plain.display().to_string() }));
    let gz = dir.path().join("gz");
    let result = map(
        &s,
        json!({
            "plasmid": "pUC19",
            "r1": gzipped(dir.path(), "r1.fastq.gz", &r1).display().to_string(),
            "r2": gzipped(dir.path(), "r2.fastq.gz", &r2).display().to_string(),
            "output": gz.display().to_string(),
        }),
    );
    assert_eq!(result["mapped_reads"], 2 * pairs);
    for name in FILES {
        assert_eq!(std::fs::read(gz.join(name)).unwrap(), std::fs::read(plain.join(name)).unwrap(), "{name}");
    }
}

#[test]
fn map_reads_refuses_unclear_or_relative_inputs() {
    let (dir, s, bases) = session();
    let (r1, r2, _) = build_pairs(&bases);
    let reads = interleaved(dir.path(), &r1, &r2).display().to_string();
    assert!(s.execute("map_reads", &json!({ "plasmid": "pUC19", "r1": reads })).is_err());
    assert!(s.execute("map_reads", &json!({ "plasmid": "pUC19", "reads": "reads.fastq" })).is_err());
    assert!(s.execute("map_reads", &json!({ "plasmid": "pUC19", "reads": reads, "r1": reads, "r2": reads })).is_err());
}

mod private {
    use std::path::PathBuf;

    use bios_engine::Session;
    use serde_json::json;

    use super::{FILES, empty_session};

    /// A file in the private fixtures folder, which keeps this repository's layout.
    fn private_fixture(path: &str) -> PathBuf {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        PathBuf::from(root).join(path)
    }

    fn iupac(name: &str) -> String {
        private_fixture("crates/bios-mapper/tests/fixtures/iupac").join(name).display().to_string()
    }

    /// A session holding the IUPAC fixture's reference, and the plasmid's name as imported.
    fn session() -> (tempfile::TempDir, Session, String) {
        let (dir, s) = empty_session();
        let copy = dir.path().join("lab_construct.gb");
        std::fs::copy(iupac("reference.gb"), &copy).unwrap();
        let imported = s.execute("import_plasmid", &json!({ "file": copy.display().to_string() })).unwrap();
        let plasmid = imported["plasmid"].as_str().unwrap().to_string();
        (dir, s, plasmid)
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn map_reads_writes_the_web_mappers_files_and_summarizes_them() {
        let (dir, s, plasmid) = session();
        let output = dir.path().join("results");
        let result =
            s.execute("map_reads", &json!({ "plasmid": plasmid, "reads": iupac("reads.fastq"), "output": output.display().to_string() })).unwrap();
        assert_eq!(result["status"], "mapped");
        assert_eq!(result["pairs"], 70);
        assert_eq!(result["mapped_reads"], 140);
        assert_eq!(result["iterations"], 3);
        assert_eq!(result["difference_count"], 0);
        assert_eq!(result["files"].as_array().unwrap().len(), 7);
        for name in FILES {
            let expected = std::fs::read(PathBuf::from(iupac("expected")).join(name)).unwrap();
            assert_eq!(std::fs::read(output.join(name)).unwrap(), expected, "{name}");
        }
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn map_reads_takes_gzip_r1_and_r2() {
        let (dir, s, plasmid) = session();
        let output = dir.path().join("gz");
        let result = s
            .execute(
                "map_reads",
                &json!({ "plasmid": plasmid, "r1": iupac("r1.fastq.gz"), "r2": iupac("r2.fastq.gz"), "output": output.display().to_string() }),
            )
            .unwrap();
        assert_eq!(result["mapped_reads"], 140);
    }
}
