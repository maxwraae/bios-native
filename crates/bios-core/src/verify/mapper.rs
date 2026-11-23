//! The read mapper's result, as Bios keeps and reads it (`reads/mapperResult.ts`, the server's
//! result JSON), and what Bios says about it: the consensus differences, the three verdicts, the
//! numbers line, the words line and a change's chip.
//!
//! Bios does no alignment or consensus calculation here. The padded reference, the read rows, the
//! consensus, the coverage and the quality are the mapper's, kept exactly; adjacent gap columns are
//! grouped for navigation only.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// Any JSON value, kept as it came: the mapper's provenance and consensus settings, which Bios
/// carries along and never reads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

use super::folder::RunSummary;
use super::grouped;

/// The map the reads were aligned against, as it was handed to the mapper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapperReference {
    pub name: String,
    pub bases: String,
    pub circular: bool,
    /// The map's features, `{name, type, start, end, strand}` with 0-based inclusive ends.
    #[serde(default)]
    pub features: Vec<ReferenceFeature>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReferenceFeature {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    pub start: usize,
    pub end: usize,
    #[serde(default = "forward")]
    pub strand: i8,
}

fn forward() -> i8 {
    1
}

/// One read as the mapper laid it out against the padded reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapperRow {
    pub index: i64,
    pub name: String,
    /// The padded column of the row's first character (may be negative on a circular map).
    pub leading: i64,
    /// The row's characters, one per padded column, gaps as `-`.
    pub internal: String,
    pub quality: String,
    pub reverse: bool,
    #[serde(default)]
    pub mate_index: Option<i64>,
    pub trim_left: usize,
    pub trim_right: usize,
    pub mapping_quality: i64,
    pub mapped_locations: i64,
}

/// The final consensus over the padded reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinalConsensus {
    pub consensus_aligned: String,
    pub coverage: Vec<f64>,
    pub quality: Vec<f64>,
    #[serde(default)]
    pub column_counts: Vec<BTreeMap<String, Vec<f64>>>,
    #[serde(default)]
    pub metadata: Json,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapperManifest {
    pub pair_count: u64,
    pub mapped_records: u64,
    #[serde(default)]
    pub completed_iterations: u64,
    #[serde(default)]
    pub elapsed_seconds: f64,
    #[serde(default)]
    pub alignment_sha256: String,
}

/// Everything one mapping returns. The text files are the mapper's own exports, kept byte for byte.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapperOutput {
    pub schema: u32,
    pub reference: MapperReference,
    pub padded_reference: String,
    pub rows: Vec<MapperRow>,
    #[serde(rename = "final")]
    pub final_consensus: FinalConsensus,
    pub manifest: MapperManifest,
    #[serde(default)]
    pub source: Json,
    #[serde(default)]
    pub sam: String,
    #[serde(default)]
    pub consensus_fasta: String,
    #[serde(default)]
    pub coverage_tsv: String,
    #[serde(default)]
    pub differences_tsv: String,
}

/// Original base -> padded column.
pub fn reference_columns(padded: &str) -> Vec<usize> {
    padded.chars().enumerate().filter(|(_, b)| *b != '-').map(|(i, _)| i).collect()
}

/// How many alignment columns are drawn: the consensus's. On a circular map the padded reference
/// can run past them with gap columns, where reads hang over the origin before they wrap; those
/// are the same columns as the start of the map and are not drawn twice.
pub fn display_columns(raw: &MapperOutput) -> usize {
    raw.final_consensus.consensus_aligned.chars().count()
}

/// Refuses a result whose dimensions disagree, rather than drawing plausible data from it.
pub fn validate(raw: &MapperOutput) -> Result<(), String> {
    let n = display_columns(raw);
    let padded: Vec<char> = raw.padded_reference.chars().collect();
    let overhang_ok = padded.len() >= n && padded[n.min(padded.len())..].iter().all(|c| *c == '-');
    let unpadded: String = padded.iter().filter(|c| **c != '-').collect();
    if raw.schema != 1 || n == 0 || !overhang_ok || unpadded.to_uppercase() != raw.reference.bases.to_uppercase() {
        return Err("Mapper reference differs from the selected reference".into());
    }
    let f = &raw.final_consensus;
    let counts_ok = f.column_counts.is_empty() || f.column_counts.len() == n;
    if f.consensus_aligned.chars().count() != n || f.coverage.len() != n || f.quality.len() != n || !counts_ok {
        return Err("Mapper display dimensions differ".into());
    }
    if raw.rows.len() as u64 != raw.manifest.mapped_records || raw.rows.is_empty() {
        return Err("Mapper read count differs".into());
    }
    if f.coverage.iter().any(|x| !x.is_finite() || *x < 0.0) || f.quality.iter().any(|x| !x.is_finite()) {
        return Err("Invalid mapper values".into());
    }
    Ok(())
}

/// Whether a difference adds or removes bases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IndelKind {
    Insertion,
    Deletion,
}

/// One consensus difference, adjacent gap columns of one kind grouped.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    /// The first padded column, 0-based.
    pub column: usize,
    /// The original reference base it sits at (the base before an insertion), 0-based.
    pub reference_pos: usize,
    pub expected: String,
    pub observed: String,
    pub indel_kind: Option<IndelKind>,
    /// How many columns it spans.
    pub size: usize,
}

/// Every column where the consensus says something other than the reference (`mapperDifferences`).
pub fn differences(raw: &MapperOutput) -> Vec<Difference> {
    let mut out: Vec<Difference> = Vec::new();
    let mut reference: i64 = -1;
    for (col, (expected, observed)) in raw.padded_reference.chars().zip(raw.final_consensus.consensus_aligned.chars()).enumerate() {
        if expected != '-' {
            reference += 1;
        }
        if observed == '?' || observed == expected {
            continue;
        }
        let kind = if expected == '-' {
            Some(IndelKind::Insertion)
        } else if observed == '-' {
            Some(IndelKind::Deletion)
        } else {
            None
        };
        if let Some(prev) = out.last_mut()
            && kind.is_some()
            && prev.indel_kind == kind
            && prev.column + prev.size == col
        {
            if expected != '-' {
                prev.expected.push(expected);
            }
            if observed != '-' {
                prev.observed.push(observed);
            }
            prev.size += 1;
            continue;
        }
        out.push(Difference {
            column: col,
            reference_pos: reference.max(0) as usize,
            expected: if expected == '-' { String::new() } else { expected.to_string() },
            observed: if observed == '-' { String::new() } else { observed.to_string() },
            indel_kind: kind,
            size: 1,
        });
    }
    out
}

/// The numbers Bios reads off a result, at the original reference's bases.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The original reference's length.
    pub ref_length: usize,
    pub total_reads: u64,
    pub mapped_reads: u64,
    pub breadth: f64,
    pub mean_depth: f64,
    pub differences: Vec<Difference>,
}

/// What a result says, in numbers (`mapperResult`).
pub fn read(raw: &MapperOutput) -> Reading {
    let n = display_columns(raw);
    let columns: Vec<usize> = reference_columns(&raw.padded_reference).into_iter().filter(|&c| c < n).collect();
    let n = columns.len();
    let cov = &raw.final_consensus.coverage;
    let at = |c: usize| cov.get(c).copied().unwrap_or(0.0);
    let covered = columns.iter().filter(|&&c| at(c) > 0.0).count();
    let total: f64 = columns.iter().map(|&c| at(c)).sum();
    let mapped = raw.rows.iter().map(|r| r.index).collect::<HashSet<_>>().len() as u64;
    let share = |x: f64| if n == 0 { 0.0 } else { x / n as f64 };
    Reading {
        ref_length: n,
        total_reads: raw.manifest.pair_count * 2,
        mapped_reads: mapped,
        breadth: share(covered as f64),
        mean_depth: share(total),
        differences: differences(raw),
    }
}

/// The colour a verdict carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerdictTone {
    /// Agrees with the map.
    Pick,
    /// Differs from it.
    Fail,
    /// Agrees where it was read, but not all of it was.
    Caveat,
}

/// The three verdicts (`verdictOf` for the mapper): `N consensus differences`, `partial coverage`
/// or `consensus matches`.
pub fn verdict(r: &Reading) -> (String, VerdictTone) {
    let n = r.differences.len();
    if n > 0 {
        (format!("{n} consensus differences"), VerdictTone::Fail)
    } else if r.breadth < 1.0 {
        ("partial coverage".into(), VerdictTone::Caveat)
    } else {
        ("consensus matches".into(), VerdictTone::Pick)
    }
}

/// The row the panel and the board draw, from the result it draws it for (`summaryOf`).
pub fn summary_of(name: &str, raw: &MapperOutput, added_at: &str, ref_hash: &str, map_id: &str, read_files: &[String]) -> RunSummary {
    let r = read(raw);
    RunSummary {
        name: name.into(),
        map_id: (!map_id.is_empty()).then(|| map_id.to_string()),
        read_files: (!read_files.is_empty()).then(|| read_files.to_vec()),
        added_at: added_at.into(),
        total_reads: r.total_reads,
        mapped_reads: r.mapped_reads,
        mean_depth: r.mean_depth,
        breadth: r.breadth,
        changes: r.differences.len() as u64,
        verdict: verdict(&r).0,
        median_read_length: None,
        ref_name: raw.reference.name.clone(),
        ref_length: r.ref_length as u64,
        ref_hash: (!ref_hash.is_empty()).then(|| ref_hash.to_string()),
        call: None,
        called_at: None,
        called_by: None,
    }
}

/// A term on the header's line, and the colour the picture gives the same thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    /// `differ` red, `thin` amber, `quiet` muted, `None` plain ink.
    pub tone: Option<&'static str>,
}

fn depth_label(depth: f64) -> String {
    if depth > 0.0 && depth < 0.01 {
        return "<0.01×".into();
    }
    if depth > 0.0 && depth < 1.0 { format!("{depth:.2}×") } else { format!("{depth:.0}×") }
}

fn mapped_share_label(r: &Reading) -> String {
    let share = if r.total_reads == 0 { 0.0 } else { 100.0 * r.mapped_reads as f64 / r.total_reads as f64 };
    if r.mapped_reads > 0 && share < 1.0 { "<1% mapped".into() } else { format!("{}% mapped", share.floor()) }
}

/// The three numbers that stay on the row: the share of the library that is this map, how much
/// of the map was read, and how deep. Floored.
pub fn header_numbers(r: &Reading) -> Vec<String> {
    if r.total_reads == 0 {
        return Vec::new();
    }
    let first = if r.mapped_reads > 0 && (r.mapped_reads as f64 / r.total_reads as f64) < 0.01 {
        format!("{} of {} reads mapped", grouped(r.mapped_reads as i64), grouped(r.total_reads as i64))
    } else {
        mapped_share_label(r)
    };
    vec![first, format!("{:.1}% covered", (1000.0 * r.breadth).floor() / 10.0), depth_label(r.mean_depth)]
}

/// The words that can change a call: how many consensus differences, and how much was covered.
pub fn header_words(r: &Reading) -> Vec<Term> {
    let n = r.differences.len();
    vec![
        Term { text: format!("{n} consensus differences"), tone: Some(if n > 0 { "differ" } else { "quiet" }) },
        Term {
            text: format!("{} / {} reference bases covered", (r.breadth * r.ref_length as f64).round() as u64, r.ref_length),
            tone: Some(if r.breadth < 1.0 { "thin" } else { "quiet" }),
        },
    ]
}

/// `G→T`, `GGCTGG missing`, `GATTCA extra`, and the size instead of the bases past 12 (`changeText`).
pub fn change_text(d: &Difference) -> String {
    match d.indel_kind {
        None => format!("{}→{}", d.expected, d.observed),
        Some(kind) => {
            let what = if kind == IndelKind::Deletion { "missing" } else { "extra" };
            let seq = if d.observed.is_empty() { &d.expected } else { &d.observed };
            let size = if !seq.is_empty() && seq.chars().count() <= 12 { seq.clone() } else { format!("{} bp", d.size) };
            format!("{size} {what}")
        }
    }
}

/// A named feature on the map, 0-based inclusive; `end < start` wraps the origin.
#[derive(Clone, Debug, PartialEq)]
pub struct ChipFeature {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// The NARROWEST named feature covering a 0-based position, or nothing (`featureAt`).
pub fn feature_at(features: &[ChipFeature], pos: usize, length: usize) -> Option<String> {
    let mut best: Option<&ChipFeature> = None;
    let mut best_span = usize::MAX;
    for f in features.iter().filter(|f| !f.name.is_empty()) {
        let wraps = f.end < f.start;
        let covers = if wraps { pos >= f.start || pos <= f.end } else { pos >= f.start && pos <= f.end };
        if !covers {
            continue;
        }
        let span = if wraps { length.saturating_sub(f.start) + f.end + 1 } else { f.end - f.start + 1 };
        if span < best_span {
            best = Some(f);
            best_span = span;
        }
    }
    best.map(|f| f.name.clone())
}

/// `2,401 G→T · SV40 promoter`: the 1-based base, the change, and the feature it is in (`chipLabel`).
pub fn chip_label(pos: usize, d: &Difference, feature: Option<&str>) -> String {
    let base = format!("{} {}", grouped(pos as i64 + 1), change_text(d));
    match feature {
        Some(f) => format!("{base} · {f}"),
        None => base,
    }
}

/// Each padded column's label: the original base number, or `N+` for an insertion column after
/// base N.
pub fn column_labels(padded: &str) -> Vec<String> {
    let mut base = 0usize;
    padded
        .chars()
        .map(|b| {
            if b == '-' {
                format!("{base}+")
            } else {
                base += 1;
                base.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> MapperOutput {
        let text = r#"{"schema":1,"reference":{"name":"test","bases":"ACG","circular":true},"paddedReference":"A-CG",
            "rows":[{"index":0,"name":"read","leading":3,"internal":"GR-A","quality":"III","reverse":true,"mate_index":null,"trim_left":1,"trim_right":0,"mapping_quality":0,"mapped_locations":2}],
            "final":{"consensus_aligned":"AT?G","coverage":[0.5,0.5,0,1.5],"quality":[7,9,0,31],"column_counts":[{"A":[1,7,7]},{"T":[1,9,9]},{},{"G":[2,31,20]}],"metadata":{"profile":"weighted_60"}},
            "manifest":{"pair_count":1,"mapped_records":1,"completed_iterations":3,"elapsed_seconds":1,"alignment_sha256":"samhash"},
            "source":{"sha256":{"iterative_mapper.py":"sourcehash"}},"sam":"exact SAM\n","consensusFasta":">test\nAT?G\n","coverageTsv":"exact coverage","differencesTsv":"exact differences"}"#;
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn transport_retains_exact_padded_data_ambiguities_trims_and_fractional_coverage() {
        let data = sample();
        let back: MapperOutput = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(back, data);
        validate(&data).unwrap();
        let r = read(&data);
        assert_eq!(r.ref_length, 3);
        assert_eq!(r.differences.len(), 1);
        assert_eq!(r.differences[0].observed, "T");
        assert_eq!(reference_columns(&data.padded_reference), vec![0, 2, 3]);
        // Coverage at the original bases: 0.5, 0, 1.5.
        assert!((r.mean_depth - 2.0 / 3.0).abs() < 1e-12);
        assert!((r.breadth - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_read_whose_mate_did_not_map_reads_back() {
        // The mapper writes `mate_index: -1` for a read placed without its mate.
        let line = r#"{"index": 7, "name": "r", "leading": 0, "internal": "AC", "quality": "II", "reverse": false, "mate_index": -1, "expected_distance": -200, "trim_left": 0, "trim_right": 0, "mapping_quality": 254, "mapped_locations": 1, "structural_split": false, "contig_sort_key": [1, 1, 1]}"#;
        let row: MapperRow = serde_json::from_str(line).unwrap();
        assert_eq!(row.mate_index, Some(-1));
    }

    #[test]
    fn corrupt_reference_dimensions_and_rows_fail_rather_than_drawing_plausible_data() {
        let mut bad = sample();
        bad.padded_reference = "A-TG".into();
        assert!(validate(&bad).is_err());
        let mut bad = sample();
        bad.rows.clear();
        assert!(validate(&bad).is_err());
        let mut bad = sample();
        bad.final_consensus.coverage = vec![1.0];
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn a_circular_overhang_past_the_consensus_is_not_drawn_twice() {
        let mut data = sample();
        data.padded_reference = "A-CG--".into();
        validate(&data).unwrap();
        assert_eq!(display_columns(&data), 4);
        assert_eq!(read(&data).ref_length, 3);
        data.padded_reference = "A-CGT-".into();
        assert!(validate(&data).is_err());
    }

    #[test]
    fn adjacent_gap_columns_group_into_one_difference() {
        let mut data = sample();
        data.padded_reference = "ACGTA".into();
        data.reference.bases = "ACGTA".into();
        data.final_consensus.consensus_aligned = "A--TC".into();
        let d = differences(&data);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].column, d[0].size, d[0].expected.as_str(), d[0].indel_kind), (1, 2, "CG", Some(IndelKind::Deletion)));
        assert_eq!(change_text(&d[0]), "CG missing");
        assert_eq!(change_text(&d[1]), "A→C");
    }

    #[test]
    fn the_three_verdicts() {
        let mut r = read(&sample());
        assert_eq!(verdict(&r).0, "1 consensus differences");
        r.differences.clear();
        assert_eq!(verdict(&r).0, "partial coverage");
        r.breadth = 1.0;
        assert_eq!(verdict(&r), ("consensus matches".into(), VerdictTone::Pick));
    }

    #[test]
    fn the_header_lines() {
        let r = read(&sample());
        assert_eq!(header_numbers(&r), vec!["50% mapped", "66.6% covered", "0.67×"]);
        let words = header_words(&r);
        assert_eq!(words[0].text, "1 consensus differences");
        assert_eq!(words[1].text, "2 / 3 reference bases covered");
    }

    const L: usize = 6000;

    fn feats() -> Vec<ChipFeature> {
        let f = |name: &str, start, end| ChipFeature { name: name.into(), start, end };
        vec![f("cassette", 1000, 4000), f("EGFP", 2000, 2700), f("", 0, 5999), f("ori", 5800, 150)]
    }

    #[test]
    fn the_narrowest_named_feature_wins_and_an_unnamed_one_never_does() {
        assert_eq!(feature_at(&feats(), 2400, L).as_deref(), Some("EGFP"));
        assert_eq!(feature_at(&feats(), 1500, L).as_deref(), Some("cassette"));
        assert_eq!(feature_at(&feats(), 4500, L), None);
    }

    #[test]
    fn a_feature_across_the_origin_covers_both_ends_and_edges_are_inclusive() {
        assert_eq!(feature_at(&feats(), 5900, L).as_deref(), Some("ori"));
        assert_eq!(feature_at(&feats(), 100, L).as_deref(), Some("ori"));
        assert_eq!(feature_at(&feats(), 200, L), None);
        assert_eq!(feature_at(&feats(), 2000, L).as_deref(), Some("EGFP"));
        assert_eq!(feature_at(&feats(), 2700, L).as_deref(), Some("EGFP"));
        assert_eq!(feature_at(&feats(), 2701, L).as_deref(), Some("cassette"));
    }

    fn diff(expected: &str, observed: &str, kind: Option<IndelKind>, size: usize) -> Difference {
        Difference { column: 0, reference_pos: 0, expected: expected.into(), observed: observed.into(), indel_kind: kind, size }
    }

    #[test]
    fn chip_label_is_1_based_grouped_then_the_change_then_the_feature() {
        let row = diff("G", "T", None, 1);
        assert_eq!(chip_label(2400, &row, None), "2,401 G→T");
        assert_eq!(chip_label(2400, &row, Some("SV40 promoter")), "2,401 G→T · SV40 promoter");
    }

    #[test]
    fn a_length_change_says_missing_or_extra_never_a_sign() {
        let del = diff("GGCTGGCCACGA", "", Some(IndelKind::Deletion), 12);
        assert_eq!(chip_label(3002, &del, Some("NeoR/KanR")), "3,003 GGCTGGCCACGA missing · NeoR/KanR");
        let ins = diff("", "GATTCA", Some(IndelKind::Insertion), 6);
        assert_eq!(chip_label(4199, &ins, None), "4,200 GATTCA extra");
        let big = diff(&"A".repeat(60), "", Some(IndelKind::Deletion), 60);
        assert_eq!(chip_label(100, &big, None), "101 60 bp missing");
        for d in [ins, del] {
            assert!(!chip_label(1, &d, None).contains(['−', '+']));
        }
    }

    #[test]
    fn insertion_columns_are_labelled_after_the_base_they_follow() {
        assert_eq!(column_labels("A-CG"), vec!["1", "1+", "2", "3"]);
    }
}
