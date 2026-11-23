//! Bios's read mapper: paired Illumina reads aligned to one reference, iteratively, with the same
//! results the Bios web app gets from its server.
//!
//! This is a port of the standalone mapper (Python with a Java kernel) that the web server runs
//! unchanged. The port's goal is fidelity, not idiom: every result file it writes is byte for byte
//! the one the original writes for the same inputs and settings. Where Python or Java arithmetic
//! shows in a result (single-precision rounding, Java `int`/`float` and `double` mixing, Python
//! float printing), the port does the same arithmetic.
//!
//! # Where each original module went
//!
//! | Original | Here |
//! |---|---|
//! | `dna_sequence.py`, `candidate_search._normalize` | [`dna`] |
//! | `mapper_primitives.py` (pair seed, Java `Random`, scores, pair penalty) | [`primitives`] |
//! | `seed_index.py`, `candidate_search.py` (seed words with ambiguity expansion, two-pass seeds) | [`search`] |
//! | `seed_extension.py` and the Java kernel (`ExtensionSupport`, `SeedExtension`, `AlignmentRefiner`; `ExtensionServer`'s text boundary) | [`kernel`] |
//! | `candidate_chaining.py` | [`chaining`] |
//! | `candidate_ranking.py` | [`ranking`] |
//! | `initial_mapper.py`, `paired_mapper.py` | [`paired`] |
//! | `iterative_mapper.py`, `iteration_selection.py` | [`iterate`] |
//! | `alignment_layout.py` | [`layout`] |
//! | `iterative_consensus.py` | [`consensus`] |
//! | `final_consensus.py` | [`final_consensus`] |
//! | `sam_export.py` | [`sam`] |
//! | `mapping_io.py` (FASTQ, pairs) | [`fastq`] |
//! | `scripts/map_reads.py`, `scripts/summarize_mapping.py` (the result files) | [`output`], [`map_reads`] |
//! | Python `repr`/`format`/`json.dumps` details | [`py`] |
//!
//! Left out because they never change a result: the raw-hit checkpoint and resume machinery
//! (`raw_checkpoint.py`), the manifest, and the viewer scripts. The candidate cache of
//! `initial_mapper.py` (which never changes a result either) becomes [`paired::CandidateCache`],
//! shared by the threads of one pass. The GenBank reader is not needed: the caller gives the reference as bases, a topology and a name.
//!
//! Pairs are independent (each has its own random generator, seeded by its index), so
//! [`map_reads`] maps them on several threads and puts the results back in input order, as the
//! web server's worker processes do.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
// A port that must match the original statement for statement keeps the original's long
// argument lists, long functions, index loops and negated float comparisons: rewriting them
// into idiomatic shapes would make each line impossible to check against the original.
#![allow(
    clippy::too_many_arguments,
    clippy::cognitive_complexity,
    clippy::needless_range_loop,
    clippy::explicit_counter_loop,
    clippy::neg_cmp_op_on_partial_ord
)]

pub mod chaining;
pub mod consensus;
pub mod dna;
pub mod fastq;
pub mod final_consensus;
pub mod iterate;
pub mod kernel;
pub mod layout;
pub mod output;
pub mod paired;
pub mod primitives;
pub mod py;
pub mod ranking;
pub mod sam;
pub mod search;

pub use fastq::FastqRead;
pub use final_consensus::{BaseCount, FinalConsensus};
pub use sam::SamRead;

/// Why mapping stopped, in words a person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapError(pub String);

impl MapError {
    pub fn new(message: impl Into<String>) -> Self {
        MapError(message.into())
    }
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MapError {}

/// The reference the reads are mapped to.
#[derive(Clone, Debug)]
pub struct Reference {
    /// The bases as the original's GenBank reader gives them: uppercase. They are used as given
    /// for the padded reference row, and normalized (U to T, unknown symbols to N) for mapping.
    pub sequence: String,
    pub circular: bool,
    /// The name written into the SAM header and the consensus FASTA, as a GenBank LOCUS name
    /// (the web writes the Bios name with runs of white space joined by `_`).
    pub name: String,
}

/// The paired reads: R1 and R2, or one interleaved file.
#[derive(Clone, Debug)]
pub enum Reads {
    Paired { r1: Vec<FastqRead>, r2: Vec<FastqRead> },
    Interleaved(Vec<FastqRead>),
}

impl Reads {
    /// Reads from FASTQ file contents (plain or gzip, multi-member included).
    pub fn from_paired_bytes(r1: &[u8], r2: &[u8]) -> Result<Self, MapError> {
        Ok(Reads::Paired { r1: fastq::parse(&fastq::decode(r1)?, "R1")?, r2: fastq::parse(&fastq::decode(r2)?, "R2")? })
    }

    pub fn from_interleaved_bytes(reads: &[u8]) -> Result<Self, MapError> {
        Ok(Reads::Interleaved(fastq::parse(&fastq::decode(reads)?, "reads")?))
    }
}

/// The mapper's settings. [`Settings::default`] is what the web uses.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Expected insert distance between mates (web: 200).
    pub insert_size: i64,
    /// Iteration preset: 1, 2, 3, 5 or 10 (web: 5).
    pub iterations: usize,
    /// Paired-overhang trimming (web: on).
    pub trim_paired_overhangs: bool,
    /// Threads to map pairs on; results do not depend on it. 0 picks min(8, available cores).
    pub workers: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { insert_size: 200, iterations: 5, trim_paired_overhangs: true, workers: 0 }
    }
}

/// How far a run has got, for a progress display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Every 1000 pairs of an iteration.
    Mapping {
        iteration: usize,
        pairs: usize,
        total_pairs: usize,
    },
    IterationComplete {
        iteration: usize,
        raw_mapped_records: usize,
        accepted_records: usize,
    },
}

/// Everything a successful run produces: the data of the seven result files.
#[derive(Clone, Debug)]
pub struct MappingResult {
    pub reference_name: String,
    pub circular: bool,
    pub pair_count: usize,
    pub completed_iterations: usize,
    /// The last iteration's mapping reference (its consensus of the previous pass).
    pub mapping_reference: String,
    /// The original reference with every alignment column (`reference-aligned.seq`, no newline).
    pub padded_reference: String,
    /// Every mapped read in final row order (`sam-rows.jsonl`). `leading` and `internal` place it
    /// in the padded columns; `trim_left`/`trim_right` are padded columns trimmed at each end.
    pub rows: Vec<SamRead>,
    /// The SAM lines, header first (`alignment.sam` without the final newline).
    pub sam_lines: Vec<String>,
    /// The weighted-60 consensus over the padded columns, with per-column coverage (fractional),
    /// consensus quality and character counts (`final-consensus.json`).
    pub consensus: FinalConsensus,
}

/// What a run ended with.
#[derive(Clone, Debug)]
pub enum Outcome {
    Mapped(Box<MappingResult>),
    /// No read mapped: the web reports this as "no results", not as a failure.
    NoResults {
        pair_count: usize,
        completed_iterations: usize,
    },
}

/// The seven files the parity validation compares, as `(file name, contents)`.
pub fn result_files(result: &MappingResult) -> Vec<(&'static str, String)> {
    let padded = result.padded_reference.as_bytes();
    let (coverage, differences) = output::coverage_tables(padded, &result.consensus);
    vec![
        ("alignment.sam", output::alignment_sam(&result.sam_lines)),
        ("sam-rows.jsonl", output::sam_rows_jsonl(&result.rows)),
        ("reference-aligned.seq", output::reference_aligned(padded)),
        ("final-consensus.json", output::final_consensus_json(&result.consensus)),
        ("consensus.fasta", output::consensus_fasta(&result.reference_name, &result.consensus)),
        ("coverage.tsv", coverage),
        ("column-differences.tsv", differences),
    ]
}

fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).min(8)
}

/// Maps paired reads to a reference (`map_reads.py --insert-size --iterations` with the web's
/// settings by default).
pub fn map_reads(
    reference: &Reference,
    reads: Reads,
    settings: &Settings,
    progress: Option<&(dyn Fn(&Progress) + Sync)>,
) -> Result<Outcome, MapError> {
    if !reference.sequence.is_ascii() {
        return Err(MapError::new("The reference holds characters that are not DNA symbols"));
    }
    let pairs = match reads {
        Reads::Paired { r1, r2 } => fastq::pairs(r1, Some(r2))?,
        Reads::Interleaved(reads) => fastq::pairs(reads, None)?,
    };
    let workers = if settings.workers == 0 { default_workers() } else { settings.workers };
    let original = reference.sequence.as_bytes();
    let last = iterate::iterate_mapping(
        &pairs,
        original,
        reference.circular,
        settings.insert_size,
        settings.iterations,
        settings.trim_paired_overhangs,
        workers,
        progress,
    )?;
    if last.reads.is_empty() {
        return Ok(Outcome::NoResults { pair_count: pairs.len(), completed_iterations: last.index + 1 });
    }
    let mut layout = last.layout;
    let reference_row = layout.reference_row()?;
    let circular_length = if layout.circular { reference_row.end() } else { 0 };
    let mut rows = Vec::with_capacity(last.reads.len());
    for item in &last.reads {
        let row = layout.project_read(&item.layout_read)?;
        let (r1, r2) = pairs.get(item.pair).ok_or_else(|| MapError::new("Read pair index out of range"))?;
        let fastq = if item.mate_number == 1 { r1 } else { r2 };
        rows.push(sam::sam_read_from_layout(&sam::RowSource {
            layout_read: &item.layout_read,
            row: &row,
            hit: &item.hit,
            mate_number: item.mate_number,
            raw_name: &fastq.name,
            raw_sequence: &fastq.sequence,
            raw_quality: &fastq.quality,
            index: item.index,
            mate_index: item.mate_index,
            expected_distance: item.expected_distance.abs(),
            mapping_quality: item.mapping_quality,
            circular_length,
        })?);
    }
    let mut rows = sam::sort_contig_reads(rows)?;
    if settings.trim_paired_overhangs {
        rows = sam::trim_all_paired_overhangs(&rows);
    }
    let padded = reference_row.render()?;
    let sam_lines = sam::export_sam(&padded, &rows, &reference.name, "reads", layout.circular)?;
    let consensus_circular = if reference.circular { (padded.len() - padded.iter().rev().take_while(|&&b| b == b'-').count()) as i64 } else { 0 };
    let window = if rows.len() >= 10000 || last.mapping_reference.len() > 100000 { 256 } else { 128 };
    let consensus = final_consensus::final_consensus(&padded, &rows, consensus_circular, window)?;
    Ok(Outcome::Mapped(Box::new(MappingResult {
        reference_name: reference.name.clone(),
        circular: reference.circular,
        pair_count: pairs.len(),
        completed_iterations: last.index + 1,
        mapping_reference: String::from_utf8_lossy(&last.mapping_reference).into_owned(),
        padded_reference: String::from_utf8_lossy(&padded).into_owned(),
        rows,
        sam_lines,
        consensus,
    })))
}
