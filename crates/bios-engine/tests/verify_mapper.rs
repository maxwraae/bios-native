//! Verify with Bios's own read mapper: what it aligns, and that the files it keeps are the
//! mapper's own. Only in a build with the `mapper` feature.
#![cfg(feature = "mapper")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bios_engine::Session;
use bios_project::{Actor, Project, Who};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures").join(name).display().to_string()
}

fn ok(s: &Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command}: {e}"))
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

/// R1 and R2 FASTQ text read off pUC19 as a circle: a 300-base fragment starting every 40 bases,
/// R1 its first 100 bases, R2 the reverse complement of its last 100.
fn puc19_pairs() -> (String, String, usize) {
    let text = std::fs::read_to_string(fixture("pUC19.gb")).unwrap();
    let bases = bios_core::genbank::read(&text).unwrap().sequence.to_uppercase().into_bytes();
    let circle = [bases.as_slice(), bases.as_slice()].concat();
    let quality = "I".repeat(100);
    let (mut r1, mut r2) = (String::new(), String::new());
    let mut count = 0;
    for start in (0..bases.len()).step_by(40) {
        let fragment = &circle[start..start + 300];
        let first = String::from_utf8(fragment[..100].to_vec()).unwrap();
        let second = String::from_utf8(reverse_complement(&fragment[200..])).unwrap();
        r1 += &format!("@frag{count}/1\n{first}\n+\n{quality}\n");
        r2 += &format!("@frag{count}/2\n{second}\n+\n{quality}\n");
        count += 1;
    }
    (r1, r2, count)
}

fn gzipped(dir: &Path, name: &str, text: &str) -> PathBuf {
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(text.as_bytes()).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, gz.finish().unwrap()).unwrap();
    path
}

fn real_mapper_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let project = Project::create(&dir.path().join("Lab"), "Lab").unwrap();
    let s = Session::new(project, Who { actor: Actor::Human, source: "test".into(), session: None })
        .with_mapper(Arc::new(bios_engine::mapping::BiosMapper));
    (dir, s)
}

#[test]
fn the_real_mapper_aligns_a_dropped_pair_and_its_files_are_the_mappers_own() {
    let (dir, s) = real_mapper_session();
    ok(&s, "import_plasmid", json!({ "file": fixture("pUC19.gb") }));
    let folder = ok(&s, "create_verification", json!({ "name": "Exact" }))["created"].as_str().unwrap().to_string();
    ok(&s, "add_verification_map", json!({ "folder": folder, "plasmid": "pUC19" }));
    // r1.fastq.gz and r2.fastq.gz say nothing about pairing in their names: the reads pair them.
    let (r1, r2, pairs) = puc19_pairs();
    let files = [gzipped(dir.path(), "r1.fastq.gz", &r1).display().to_string(), gzipped(dir.path(), "r2.fastq.gz", &r2).display().to_string()];
    let added = ok(&s, "add_reads", json!({ "folder": folder, "files": files, "map": "pUC19" }));
    assert_eq!(added["added"].as_array().unwrap().len(), 1, "{added}");
    let ran = ok(&s, "run_verification", json!({ "folder": folder }));
    assert_eq!(ran["aligned"].as_array().unwrap().len(), 1, "{ran}");
    let sample = ran["aligned"][0]["sample"].as_str().unwrap().to_string();
    let read = ok(&s, "read_alignment", json!({ "folder": folder, "sample": sample }));
    assert_eq!(read["reads"]["total"], 2 * pairs);
    assert!(read["differences"].as_array().unwrap().is_empty(), "every read is an exact copy of pUC19: {}", read["differences"]);

    // The exported files are the ones the mapper writes for the same reads.
    let text = std::fs::read_to_string(fixture("pUC19.gb")).unwrap();
    let construct = bios_core::genbank::read(&text).unwrap();
    let reference = bios_mapper::Reference { sequence: construct.sequence.to_uppercase(), circular: construct.circular, name: "pUC19".into() };
    let reads = bios_mapper::Reads::from_paired_bytes(r1.as_bytes(), r2.as_bytes()).unwrap();
    let bios_mapper::Outcome::Mapped(result) = bios_mapper::map_reads(&reference, reads, &bios_mapper::Settings::default(), None).unwrap() else {
        panic!("no reads mapped")
    };
    let expected = bios_mapper::result_files(&result);
    for (what, name) in
        [("consensus", "consensus.fasta"), ("coverage", "coverage.tsv"), ("differences", "column-differences.tsv"), ("sam", "alignment.sam")]
    {
        let out = dir.path().join("out").join(name);
        ok(&s, "export_alignment", json!({ "folder": folder, "sample": sample, "what": what, "path": out.display().to_string() }));
        let want = &expected.iter().find(|(file, _)| *file == name).unwrap().1;
        assert_eq!(&std::fs::read_to_string(&out).unwrap(), want, "{name}");
    }
}

mod private {
    use std::path::PathBuf;

    use serde_json::json;

    use super::{ok, real_mapper_session};

    /// A file in the private fixtures folder, which keeps this repository's layout.
    fn private_fixture(path: &str) -> PathBuf {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        PathBuf::from(root).join(path)
    }

    fn iupac(name: &str) -> PathBuf {
        private_fixture("crates/bios-mapper/tests/fixtures/iupac").join(name)
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_real_mapper_matches_the_original_mappers_files() {
        let (dir, s) = real_mapper_session();
        let copy = dir.path().join("lab_construct.gb");
        std::fs::copy(iupac("reference.gb"), &copy).unwrap();
        let plasmid = ok(&s, "import_plasmid", json!({ "file": copy.display().to_string() }))["plasmid"].as_str().unwrap().to_string();
        let folder = ok(&s, "create_verification", json!({ "name": "IUPAC" }))["created"].as_str().unwrap().to_string();
        ok(&s, "add_verification_map", json!({ "folder": folder, "plasmid": plasmid }));
        let files = [iupac("r1.fastq.gz").display().to_string(), iupac("r2.fastq.gz").display().to_string()];
        let added = ok(&s, "add_reads", json!({ "folder": folder, "files": files, "map": plasmid }));
        assert_eq!(added["added"].as_array().unwrap().len(), 1, "{added}");
        let ran = ok(&s, "run_verification", json!({ "folder": folder }));
        assert_eq!(ran["aligned"].as_array().unwrap().len(), 1, "{ran}");
        let sample = ran["aligned"][0]["sample"].as_str().unwrap().to_string();
        let read = ok(&s, "read_alignment", json!({ "folder": folder, "sample": sample }));
        assert_eq!(read["reads"]["total"], 140);
        for (what, name) in
            [("consensus", "consensus.fasta"), ("coverage", "coverage.tsv"), ("differences", "column-differences.tsv"), ("sam", "alignment.sam")]
        {
            let out = dir.path().join("out").join(name);
            ok(&s, "export_alignment", json!({ "folder": folder, "sample": sample, "what": what, "path": out.display().to_string() }));
            assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(iupac("expected").join(name)).unwrap(), "{name}");
        }
    }
}
