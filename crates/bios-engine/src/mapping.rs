//! Read mapping with Bios's own read mapper (`bios-mapper`), in a build with the `mapper` feature:
//! the mapper Verify aligns with ([`BiosMapper`]) and the `map_reads` command.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bios_core::verify::mapper::{FinalConsensus, MapperManifest, MapperOutput, MapperRow};
use bios_mapper::{Outcome, Reads, Reference, Settings};
use bios_project::write_outside;
use serde_json::{Value, json};

use crate::verify::{Control, MapJob, MapReads, ReadMapper};
use crate::{Args, Failure, Session, fail, refs};

/// The mapper a session of this build starts with.
pub fn own_mapper() -> Arc<dyn ReadMapper> {
    Arc::new(BiosMapper)
}

/// Bios's own read mapper (`bios-mapper`), with the web's settings: insert size 200, up to five
/// passes, paired-overhang trimming, weighted-60 consensus. The result is read back through the
/// mapper's own seven files, so what Bios keeps is exactly what those files say.
pub struct BiosMapper;

impl ReadMapper for BiosMapper {
    fn map(&self, job: &MapJob, control: &Control) -> Result<MapperOutput, String> {
        let reads = match &job.reads {
            MapReads::Pair { r1, r2 } => bios_mapper::Reads::from_paired_bytes(r1, r2),
            MapReads::Interlaced(all) => bios_mapper::Reads::from_interleaved_bytes(all),
        }
        .map_err(|e| e.to_string())?;
        // The web writes the reference's name with runs of white space joined by `_`.
        let name = job.reference.name.split(bios_mapper::py::is_py_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
        let reference = bios_mapper::Reference { sequence: job.reference.bases.to_uppercase(), circular: job.reference.circular, name };
        let report = |p: &bios_mapper::Progress| match p {
            bios_mapper::Progress::Mapping { iteration, pairs, total_pairs } => {
                control.say(&format!("pass {iteration} · {pairs} of {total_pairs} pairs"))
            }
            bios_mapper::Progress::IterationComplete { iteration, accepted_records, .. } => {
                control.say(&format!("pass {iteration} done · {accepted_records} reads placed"))
            }
        };
        let started = std::time::Instant::now();
        let outcome = bios_mapper::map_reads(&reference, reads, &bios_mapper::Settings::default(), Some(&report)).map_err(|e| e.to_string())?;
        let result = match outcome {
            bios_mapper::Outcome::NoResults { pair_count, .. } => {
                return Err(format!("none of the {pair_count} read pairs mapped to this map; no alignment or consensus was produced"));
            }
            bios_mapper::Outcome::Mapped(result) => result,
        };
        let files: std::collections::HashMap<&str, String> = bios_mapper::result_files(&result).into_iter().collect();
        let file = |name: &str| files.get(name).cloned().unwrap_or_default();
        let rows = file("sam-rows.jsonl")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<MapperRow>(l).map_err(|e| format!("sam-rows.jsonl: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let final_consensus: FinalConsensus =
            serde_json::from_str(&file("final-consensus.json")).map_err(|e| format!("final-consensus.json: {e}"))?;
        let manifest = MapperManifest {
            pair_count: result.pair_count as u64,
            mapped_records: rows.len() as u64,
            completed_iterations: result.completed_iterations as u64,
            elapsed_seconds: started.elapsed().as_secs_f64(),
            alignment_sha256: String::new(),
        };
        let mut source = std::collections::BTreeMap::new();
        source.insert("engine".to_string(), bios_core::verify::mapper::Json::Text("bios-mapper".into()));
        Ok(MapperOutput {
            schema: 1,
            reference: job.reference.clone(),
            padded_reference: result.padded_reference.clone(),
            rows,
            final_consensus,
            manifest,
            source: bios_core::verify::mapper::Json::Object(source),
            sam: file("alignment.sam"),
            consensus_fasta: file("consensus.fasta"),
            coverage_tsv: file("coverage.tsv"),
            differences_tsv: file("column-differences.tsv"),
        })
    }
}

// ── map_reads: one mapping outside any verification ─────────────────────────────────────────
// Verify: mapping sequencing reads to a plasmid with Bios's read mapper (`bios-mapper`), the
// same mapper and settings the Bios web app runs on its server.
//
// `map_reads` reads the FASTQ files, maps them to the plasmid as it is now and writes the
// mapper's seven result files to a folder outside the project:
//
// | File | Holds |
// |---|---|
// | `alignment.sam` | Padded SAM of every mapped read. |
// | `sam-rows.jsonl` | One JSON row per read, in display order: `leading` (padded column where `internal` starts, 0-based), `internal` (bases and gaps), `trim_left`/`trim_right` (padded columns trimmed at each end), `reverse`, `mate_index`, `mapping_quality`, `quality`, `name`. |
// | `reference-aligned.seq` | The plasmid with every alignment column (`-` at insertions). |
// | `final-consensus.json` | The weighted-60 consensus over the padded columns with per-column `coverage`, `quality` and `column_counts`. |
// | `consensus.fasta` | The consensus without gaps. |
// | `coverage.tsv` | Per padded column: reference position, reference base, consensus base, coverage, consensus quality. |
// | `column-differences.tsv` | The `coverage.tsv` lines where the consensus differs from the reference. |
//
// A window that wants the data rather than the files calls `bios_mapper::map_reads` itself; its
// `MappingResult` holds the same values as Rust types.

/// The most differences listed in the summary; the full list is in `column-differences.tsv`.
const LISTED_DIFFERENCES: usize = 200;

fn outside_path(session: &Session, given: &str) -> Result<PathBuf, Failure> {
    let path = match given.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)).unwrap_or_else(|| PathBuf::from(given)),
        None => PathBuf::from(given),
    };
    if !path.is_absolute() {
        return fail(format!("{given} is not a full path; give the file's absolute path"));
    }
    if path.starts_with(session.project().root()) {
        return fail(format!("{given} is inside the project; keep reads and results outside it"));
    }
    Ok(path)
}

fn read_file(session: &Session, given: &str) -> Result<Vec<u8>, Failure> {
    let path = outside_path(session, given)?;
    std::fs::read(&path).map_err(|e| Failure(format!("could not read {}: {e}", path.display())))
}

pub(crate) fn map_reads(session: &Session, args: &Args) -> Result<Value, Failure> {
    let loaded = session.load(args)?;
    let construct = &loaded.construct;
    let reads = match (args.opt_str("r1")?, args.opt_str("r2")?, args.opt_str("reads")?) {
        (Some(r1), Some(r2), None) => Reads::from_paired_bytes(&read_file(session, &r1)?, &read_file(session, &r2)?),
        (None, None, Some(reads)) => Reads::from_interleaved_bytes(&read_file(session, &reads)?),
        _ => return fail("give r1 and r2 together, or reads alone"),
    }
    .map_err(|e| Failure(e.to_string()))?;
    let output = match args.opt_str("output")? {
        Some(given) => outside_path(session, &given)?,
        None => std::env::temp_dir().join("bios-map-reads").join(format!("{}-{}", construct.name.replace(['/', '\\'], "_"), uuid::Uuid::new_v4())),
    };
    // The web writes the reference's GenBank file with the plasmid's name, runs of white space
    // joined by `_`, and the mapper reads its bases back uppercase.
    let reference = Reference {
        sequence: construct.sequence.to_uppercase(),
        circular: construct.circular,
        name: construct.name.split(bios_mapper::py::is_py_space).filter(|part| !part.is_empty()).collect::<Vec<_>>().join("_"),
    };
    let outcome = bios_mapper::map_reads(&reference, reads, &Settings::default(), None).map_err(|e| Failure(e.to_string()))?;
    let result = match outcome {
        Outcome::NoResults { pair_count, completed_iterations } => {
            return Ok(json!({
                "plasmid": refs::label(&loaded.path),
                "status": "no_results",
                "message": "No reads mapped to the plasmid; no alignment or consensus was produced.",
                "pairs": pair_count,
                "iterations": completed_iterations,
            }));
        }
        Outcome::Mapped(result) => result,
    };
    let mut files = Vec::new();
    for (name, contents) in bios_mapper::result_files(&result) {
        let path = output.join(name);
        write_outside(Path::new(&path), &contents, false)?;
        files.push(path.display().to_string());
    }
    let consensus = &result.consensus;
    let padded = result.padded_reference.as_bytes();
    let mut reference_position = 0;
    let mut differences = Vec::new();
    let mut difference_count = 0;
    for (i, (&base, &called)) in padded.iter().zip(&consensus.consensus_aligned).enumerate() {
        if base != b'-' {
            reference_position += 1;
        }
        if called != b'?' && called != base {
            difference_count += 1;
            if differences.len() < LISTED_DIFFERENCES {
                differences.push(json!({
                    "column": i + 1,
                    "position": reference_position,
                    "kind": if base == b'-' { "insertion" } else if called == b'-' { "deletion" } else { "substitution" },
                    "reference": (base as char).to_string(),
                    "consensus": (called as char).to_string(),
                    "coverage": consensus.coverage.get(i).copied().unwrap_or(0.0),
                    "quality": consensus.quality.get(i).copied().unwrap_or(0),
                }));
            }
        }
    }
    let reference_columns: Vec<f64> = padded.iter().zip(&consensus.coverage).filter(|(b, _)| **b != b'-').map(|(_, c)| *c).collect();
    let covered = reference_columns.iter().filter(|c| **c > 0.0).count();
    let uncovered = consensus.consensus_aligned.iter().zip(padded).filter(|(c, b)| **c == b'?' && **b != b'-').count();
    Ok(json!({
        "plasmid": refs::label(&loaded.path),
        "status": "mapped",
        "pairs": result.pair_count,
        "mapped_reads": result.rows.len(),
        "iterations": result.completed_iterations,
        "plasmid_length": construct.len(),
        "alignment_columns": padded.len(),
        "covered_bases": covered,
        "uncovered_bases": uncovered,
        "maximum_coverage": consensus.coverage.iter().copied().fold(0.0, f64::max),
        "mean_coverage": if reference_columns.is_empty() { 0.0 } else { reference_columns.iter().sum::<f64>() / reference_columns.len() as f64 },
        "difference_count": difference_count,
        "differences": differences,
        "differences_note": "Positions are 1-based on the plasmid; at an insertion, position is the base before it. column is the 1-based padded alignment column.",
        "output": output.display().to_string(),
        "files": files,
    }))
}
