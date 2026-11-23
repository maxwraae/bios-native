//! The seven result files of `scripts/map_reads.py` and `scripts/summarize_mapping.py`, written
//! byte for byte as the original writes them.

use std::fmt::Write as _;

use crate::final_consensus::FinalConsensus;
use crate::paired::PairedAlignment;
use crate::py::{Json, float_g};
use crate::ranking::RankedAlignment;
use crate::sam::SamRead;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `alignment.sam`.
pub fn alignment_sam(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// One line of `sam-rows.jsonl` (`json.dumps(asdict(read))`).
pub fn sam_row_json(read: &SamRead) -> String {
    let internal = text(&read.internal);
    let (a, b, c) = read.contig_sort_key;
    let value = Json::Object(vec![
        ("index", Json::Int(read.index)),
        ("name", Json::Str(&read.name)),
        ("leading", Json::Int(read.leading)),
        ("internal", Json::Str(&internal)),
        ("quality", Json::Str(&read.quality)),
        ("reverse", Json::Bool(read.reverse)),
        ("mate_index", Json::Int(read.mate_index)),
        ("expected_distance", Json::Int(read.expected_distance)),
        ("trim_left", Json::Int(read.trim_left)),
        ("trim_right", Json::Int(read.trim_right)),
        ("mapping_quality", Json::Int(read.mapping_quality)),
        ("mapped_locations", Json::Int(read.mapped_locations)),
        ("structural_split", Json::Bool(read.structural_split)),
        ("contig_sort_key", Json::List(vec![Json::Int(a), Json::Int(b), Json::Int(c)])),
    ]);
    let mut out = String::new();
    value.write(&mut out);
    out
}

/// `sam-rows.jsonl`.
pub fn sam_rows_jsonl(rows: &[SamRead]) -> String {
    let mut out = String::new();
    for row in rows {
        out.push_str(&sam_row_json(row));
        out.push('\n');
    }
    out
}

/// `reference-aligned.seq`.
pub fn reference_aligned(padded_reference: &[u8]) -> String {
    format!("{}\n", text(padded_reference))
}

/// `final-consensus.json` (`json.dumps(asdict(result))`).
pub fn final_consensus_json(result: &FinalConsensus) -> String {
    let consensus = text(&result.consensus_aligned);
    let m = &result.metadata;
    let base_names: Vec<Vec<String>> = result.column_counts.iter().map(|c| c.iter().map(|(b, _)| b.to_string()).collect()).collect();
    let columns = result
        .column_counts
        .iter()
        .zip(&base_names)
        .map(|(column, names)| {
            Json::Object(
                column
                    .iter()
                    .zip(names)
                    .map(|((_, v), name)| (name.as_str(), Json::List(vec![Json::Int(v.count), Json::Int(v.quality_sum), Json::Int(v.quality_max)])))
                    .collect(),
            )
        })
        .collect();
    let value = Json::Object(vec![
        ("consensus_aligned", Json::Str(&consensus)),
        ("coverage", Json::List(result.coverage.iter().map(|&c| Json::Float(c)).collect())),
        ("quality", Json::List(result.quality.iter().map(|&q| Json::Int(q)).collect())),
        (
            "metadata",
            Json::Object(vec![
                ("profile", Json::Str("default Illumina weighted_60")),
                ("threshold", Json::Float(m.threshold)),
                ("exclude_reference", Json::Bool(true)),
                ("ignore_trimmed_regions", Json::Bool(true)),
                ("gaps", Json::Str("canonical state")),
                ("uncovered_character", Json::Str("?")),
                (
                    "quality",
                    Json::Str("mapping/base probability combination; symmetric homopolymers; reduced gap quality; native 100-column lookaround"),
                ),
                ("counts_cache_window", Json::Int(m.counts_cache_window)),
                ("raw_gap_quality", Json::Str(m.raw_gap_quality)),
                (
                    "storage_profile_note",
                    Json::Str(
                        "Automatic defaults reproduce the tested CLI profiles; explicit window/gap policy overrides support a different native document representation",
                    ),
                ),
                ("consensus_quality", Json::Str("sum agreeing minus disagreeing qualities")),
                ("low_coverage_replacement", Json::Bool(false)),
                ("circular_length", Json::Int(m.circular_length)),
                ("padded_length", Json::Int(m.padded_length)),
                ("read_count", Json::Int(m.read_count)),
                ("scope", Json::Str("Illumina DNA reads with FASTQ qualities; no Sanger chromatogram peak adjustment")),
            ]),
        ),
        ("column_counts", Json::List(columns)),
    ]);
    let mut out = String::new();
    value.write(&mut out);
    out.push('\n');
    out
}

/// `consensus.fasta`: the ungapped consensus in lines of 80.
pub fn consensus_fasta(reference_name: &str, result: &FinalConsensus) -> String {
    let sequence = result.consensus_ungapped();
    let lines: Vec<String> = sequence.chunks(80).map(text).collect();
    format!(">{reference_name} weighted_60 consensus\n{}\n", lines.join("\n"))
}

/// `coverage.tsv` and `column-differences.tsv`.
pub fn coverage_tables(padded_reference: &[u8], result: &FinalConsensus) -> (String, String) {
    let header = "alignment_column\treference_position\treference\tconsensus\tcoverage\tconsensus_quality\n";
    let (mut coverage, mut differences) = (header.to_string(), header.to_string());
    let mut reference_position = 0;
    for (i, (((&base, &consensus), &cov), &quality)) in
        padded_reference.iter().zip(&result.consensus_aligned).zip(&result.coverage).zip(&result.quality).enumerate()
    {
        if base != b'-' {
            reference_position += 1;
        }
        let mut line = String::new();
        let _ = writeln!(line, "{}\t{}\t{}\t{}\t{}\t{}", i + 1, reference_position, base as char, consensus as char, float_g(cov), quality);
        coverage.push_str(&line);
        if consensus != b'?' && consensus != base {
            differences.push_str(&line);
        }
    }
    (coverage, differences)
}

fn hit_json(hit: &RankedAlignment) -> Json<'static> {
    let ints = |v: &[i64]| Json::List(v.iter().map(|&x| Json::Int(x)).collect());
    Json::Object(vec![
        ("reference_id", Json::Int(hit.reference_id)),
        ("query_start", Json::Int(hit.query_start)),
        ("reference_start", Json::Int(hit.reference_start)),
        ("alignment_length", Json::Int(hit.alignment_length)),
        ("reverse", Json::Bool(hit.reverse)),
        ("score", Json::Float(hit.score)),
        ("mismatch_count", Json::Int(hit.mismatch_count)),
        ("query_gap_positions", ints(&hit.query_gap_positions)),
        ("ref_gap_positions", ints(&hit.ref_gap_positions)),
        ("query_gap_sizes", ints(&hit.query_gap_sizes)),
        ("ref_gap_sizes", ints(&hit.ref_gap_sizes)),
        ("second_best_errors", Json::Int(hit.second_best_errors)),
        ("other_best_count", Json::Int(hit.other_best_count)),
    ])
}

/// One line of an iteration's `pairs.jsonl` checkpoint: the raw result of one pair.
pub fn pair_json(pair: &PairedAlignment) -> String {
    let value = Json::Object(vec![
        ("read", pair.read.as_ref().map_or(Json::Null, hit_json)),
        ("mate", pair.mate.as_ref().map_or(Json::Null, hit_json)),
        ("score", Json::Float(pair.score)),
        ("mapping_quality", Json::Int(pair.mapping_quality)),
    ]);
    let mut out = String::new();
    value.write(&mut out);
    out
}
