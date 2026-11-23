//! The mapper's results compared byte for byte with the original mapper's result files, and
//! checked on reads built from pUC19, whose answers follow from how they were made.
//!
//! The 70-pair IUPAC fixture the original was validated on, with the seven files the original
//! wrote for it, is private data: its tests are in `private` and read it from
//! `BIOS_PRIVATE_FIXTURES`. The full A1 and A7 runs are too big to keep anywhere here; their tests
//! are ignored and read the original's validation output from paths given in the environment:
//!
//! ```text
//! BIOS_MAPPER_A1=/path/to/a1-full BIOS_MAPPER_A7=/path/to/a7-full \
//!   cargo test --release -p bios-mapper --test parity -- --ignored --nocapture
//! ```
//!
//! Each folder holds `reference.gb`, `reads.fastq` and `original/` with the original's files, as
//! the web repository's `scripts/validate-mapper.py` lays them out.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bios_mapper::{MappingResult, Outcome, Reads, Reference, Settings, map_reads, result_files};

const FILES: [&str; 7] =
    ["alignment.sam", "sam-rows.jsonl", "reference-aligned.seq", "final-consensus.json", "consensus.fasta", "coverage.tsv", "column-differences.tsv"];

fn reference(path: &Path) -> Reference {
    let text = std::fs::read_to_string(path).unwrap();
    let construct = bios_core::genbank::read(&text).unwrap();
    let locus = text.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_string();
    Reference { sequence: construct.sequence.to_uppercase(), circular: construct.circular, name: locus }
}

fn run(reference_path: &Path, reads: Reads, expected: &Path) {
    let reference = reference(reference_path);
    let started = Instant::now();
    let workers = std::env::var("BIOS_MAPPER_WORKERS").ok().and_then(|w| w.parse().ok()).unwrap_or(0);
    let outcome = map_reads(&reference, reads, &Settings { workers, ..Settings::default() }, None).unwrap();
    let elapsed = started.elapsed();
    let Outcome::Mapped(result) = outcome else { panic!("no reads mapped") };
    println!(
        "{}: {} pairs, {} rows, {} passes in {:.1}s",
        expected.display(),
        result.pair_count,
        result.rows.len(),
        result.completed_iterations,
        elapsed.as_secs_f64()
    );
    let mut different = Vec::new();
    for (name, contents) in result_files(&result) {
        let original = std::fs::read(expected.join(name)).unwrap();
        if original != contents.as_bytes() {
            let line = original.split(|&b| b == b'\n').zip(contents.as_bytes().split(|&b| b == b'\n')).position(|(a, b)| a != b);
            different.push(format!("{name} (first differing line {line:?})"));
            if let Ok(dir) = std::env::var("BIOS_MAPPER_DUMP") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(Path::new(&dir).join(name), &contents).unwrap();
            }
        }
    }
    assert!(different.is_empty(), "files differ from the original: {different:?}");
    assert_eq!(FILES.len(), result_files(&result).len());
}

/// pUC19 (circular, 2686 bp) as the mapper's reference.
fn puc19() -> Reference {
    reference(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../bios-core/tests/fixtures/pUC19.gb"))
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

/// The qualities every built read carries: a fixed cycle of Illumina scores.
fn quality(length: usize) -> String {
    "IIIIHHGGFF?".chars().cycle().take(length).collect()
}

/// R1 and R2 FASTQ text for pairs read off `template` as a circle: a 300-base fragment starting
/// every `step` bases, R1 its first 100 bases, R2 the reverse complement of its last 100.
fn build_pairs(template: &[u8], step: usize) -> (String, String, usize) {
    let circle = [template, template].concat();
    let (mut r1, mut r2) = (String::new(), String::new());
    let mut count = 0;
    for start in (0..template.len()).step_by(step) {
        let fragment = &circle[start..start + 300];
        let first = String::from_utf8(fragment[..100].to_vec()).unwrap();
        let second = String::from_utf8(reverse_complement(&fragment[200..])).unwrap();
        r1 += &format!("@frag{count}/1\n{first}\n+\n{}\n", quality(100));
        r2 += &format!("@frag{count}/2\n{second}\n+\n{}\n", quality(100));
        count += 1;
    }
    (r1, r2, count)
}

/// R1 and R2 interleaved into one FASTQ text.
fn interleave(r1: &str, r2: &str) -> String {
    let records = |text: &str| text.lines().collect::<Vec<_>>().chunks(4).map(|c| c.join("\n") + "\n").collect::<Vec<_>>();
    records(r1).into_iter().zip(records(r2)).map(|(a, b)| a + &b).collect()
}

fn gzip(parts: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for part in parts {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(part.as_bytes()).unwrap();
        bytes.extend(encoder.finish().unwrap());
    }
    bytes
}

fn mapped(reference: &Reference, reads: Reads) -> Box<MappingResult> {
    match map_reads(reference, reads, &Settings::default(), None).unwrap() {
        Outcome::Mapped(result) => result,
        Outcome::NoResults { .. } => panic!("no reads mapped"),
    }
}

/// `(1-based reference position, reference base, consensus base)` wherever they differ.
fn differences(result: &MappingResult) -> Vec<(usize, u8, u8)> {
    let mut position = 0;
    let mut out = Vec::new();
    for (&base, &called) in result.padded_reference.as_bytes().iter().zip(&result.consensus.consensus_aligned) {
        if base != b'-' {
            position += 1;
        }
        if called != b'?' && called != base {
            out.push((position, base, called));
        }
    }
    out
}

#[test]
fn exact_reads_from_puc19_all_map_and_the_consensus_is_the_reference() {
    let reference = puc19();
    let (r1, r2, pairs) = build_pairs(reference.sequence.as_bytes(), 40);
    assert_eq!(pairs, 68, "2686 bases, a fragment every 40");
    let result = mapped(&reference, Reads::from_interleaved_bytes(interleave(&r1, &r2).as_bytes()).unwrap());
    assert_eq!(result.pair_count, pairs);
    assert_eq!(result.rows.len(), 2 * pairs, "every read maps");
    // Exact reads open no gap inside the reference; reads running over the origin only add
    // columns after its end.
    let (inside, past_end) = result.padded_reference.split_at(reference.sequence.len());
    assert_eq!(inside, reference.sequence);
    assert!(past_end.bytes().all(|b| b == b'-'), "{past_end}");
    // Fragments start every 40 bases round the circle, so every base is under some read.
    assert_eq!(String::from_utf8(result.consensus.consensus_ungapped()).unwrap(), reference.sequence);
    assert!(result.consensus.coverage.iter().all(|&c| c > 0.0));
    assert!(differences(&result).is_empty());
    for row in &result.rows {
        let given = quality(100);
        let expected = if row.reverse { given.chars().rev().collect() } else { given };
        assert_eq!(row.quality, expected, "{}", row.name);
    }
    let files = result_files(&result);
    assert_eq!(files.iter().map(|(name, _)| *name).collect::<Vec<_>>(), FILES);
    assert!(files[4].1.starts_with(">pUC19"), "{}", files[4].1);
    assert_eq!(files[6].1.lines().count(), 1, "column-differences.tsv holds only its header");
}

#[test]
fn a_substitution_in_every_read_is_found_at_its_position() {
    let reference = puc19();
    // pUC19 base 1000 (1-based) is planted as another base in the template the reads come from.
    let mut template = reference.sequence.clone().into_bytes();
    let was = template[999];
    let planted = if was == b'A' { b'C' } else { b'A' };
    template[999] = planted;
    let (r1, r2, pairs) = build_pairs(&template, 40);
    let result = mapped(&reference, Reads::from_interleaved_bytes(interleave(&r1, &r2).as_bytes()).unwrap());
    assert_eq!(result.rows.len(), 2 * pairs);
    assert_eq!(differences(&result), vec![(1000, was, planted)]);
    let tsv = &result_files(&result)[6].1;
    let row: Vec<&str> = tsv.lines().nth(1).unwrap().split('\t').collect();
    assert_eq!(row[1..4], ["1000".to_string(), (was as char).to_string(), (planted as char).to_string()]);
}

#[test]
fn gzip_r1_r2_gives_the_same_files_as_interleaved() {
    let reference = puc19();
    let (r1, r2, _) = build_pairs(reference.sequence.as_bytes(), 40);
    let interleaved = mapped(&reference, Reads::from_interleaved_bytes(interleave(&r1, &r2).as_bytes()).unwrap());
    let paired = mapped(&reference, Reads::from_paired_bytes(&gzip(&[&r1]), &gzip(&[&r2])).unwrap());
    assert_eq!(result_files(&paired), result_files(&interleaved));
}

#[test]
fn multi_member_gzip_reads_every_member() {
    let (r1, _, pairs) = build_pairs(puc19().sequence.as_bytes(), 40);
    let lines: Vec<&str> = r1.lines().collect();
    let (head, tail) = lines.split_at(40);
    let joined = gzip(&[&(head.join("\n") + "\n"), &(tail.join("\n") + "\n")]);
    let Reads::Interleaved(records) = Reads::from_interleaved_bytes(&joined).unwrap() else { unreachable!() };
    assert_eq!(records.len(), pairs);
    assert_eq!(records, bios_mapper::fastq::parse(&r1, "R1").unwrap());
}

#[test]
fn malformed_and_unpaired_input_is_an_error_not_a_crash() {
    assert!(Reads::from_interleaved_bytes(b"@a/1\nACGT\n+\nIII\n").is_err());
    let reference = Reference { sequence: "ACGT".repeat(50), circular: true, name: "r".into() };
    let one = Reads::from_interleaved_bytes(b"@a/1\nACGT\n+\nIIII\n").unwrap();
    assert!(map_reads(&reference, one, &Settings::default(), None).is_err());
    let names = Reads::from_interleaved_bytes(b"@a/1\nACGT\n+\nIIII\n@b/2\nACGT\n+\nIIII\n").unwrap();
    assert!(map_reads(&reference, names, &Settings::default(), None).is_err());
}

#[test]
fn reads_that_map_nowhere_are_no_results() {
    let reference = Reference { sequence: "ACGTTGCA".repeat(40), circular: true, name: "r".into() };
    let n = "N".repeat(60);
    let q = "I".repeat(60);
    let text = format!("@a/1\n{n}\n+\n{q}\n@a/2\n{n}\n+\n{q}\n");
    let reads = Reads::from_interleaved_bytes(text.as_bytes()).unwrap();
    let outcome = map_reads(&reference, reads, &Settings::default(), None).unwrap();
    assert!(matches!(outcome, Outcome::NoResults { pair_count: 1, .. }));
}

fn validation_run(variable: &str) {
    let Ok(dir) = std::env::var(variable) else {
        panic!("set {variable} to the original's validation folder");
    };
    let dir = PathBuf::from(dir);
    let reads = Reads::from_interleaved_bytes(&std::fs::read(dir.join("reads.fastq")).unwrap()).unwrap();
    run(&dir.join("reference.gb"), reads, &dir.join("original"));
}

#[test]
#[ignore = "needs the full A1 validation run; see the module notes"]
fn full_a1_matches_the_original_byte_for_byte() {
    validation_run("BIOS_MAPPER_A1");
}

#[test]
#[ignore = "needs the full A7 validation run; see the module notes"]
fn full_a7_matches_the_original_byte_for_byte() {
    validation_run("BIOS_MAPPER_A7");
}

mod private {
    use std::path::PathBuf;

    use bios_mapper::Reads;

    use super::run;

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
    fn iupac_matches_the_original_byte_for_byte() {
        let reads = Reads::from_interleaved_bytes(&std::fs::read(iupac("reads.fastq")).unwrap()).unwrap();
        run(&iupac("reference.gb"), reads, &iupac("expected"));
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn iupac_from_gzip_r1_r2_matches_too() {
        let reads = Reads::from_paired_bytes(&std::fs::read(iupac("r1.fastq.gz")).unwrap(), &std::fs::read(iupac("r2.fastq.gz")).unwrap()).unwrap();
        run(&iupac("reference.gb"), reads, &iupac("expected"));
    }
}
