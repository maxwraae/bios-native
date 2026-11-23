//! Find's Alignment mode: how does this sequence FIT here, including where it doesn't. Ported from
//! the Bios web app's `editor/alignment.ts`.
//!
//! "Where does this prime?" is a yes/no gate (3'-anchored, gapless), so it answers NOTHING for a
//! query whose 3' end does not match, that carries an insert, or that only fits over half its
//! length, and every one of those is a thing a researcher is trying to look at. So Alignment asks
//! [`local_align`]: local Smith-Waterman with affine gaps, which always returns its best fit and
//! says where it breaks. Priming did not disappear; it moved from a FILTER to a LABEL. A hit that
//! would not prime still draws; it just says so.
//!
//! The web hands a query to minimap2 once the Smith-Waterman table passes 30 M cells. Bios aligns
//! every query with the Smith-Waterman, off the window's thread, inside [`CELL_BUDGET`]: past that it
//! would keep a person waiting for seconds, so Find says how long a query it can align against
//! this construct ([`query_limit`]) rather than trying.

use crate::align::{ALIGN_DEFAULTS, AlignOp, AlignOptions, LocalAlignment, local_align};
use crate::duplex::{AlignRun, AlignRunKind, alignment_runs};
use crate::seq::{DNA_TM_DEFAULTS, clean_seq, tm};

/// One place the query fits, with the judgement of what kind of fit it is.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignHit {
    pub alignment: LocalAlignment,
    /// `probe-<rank>`.
    pub id: String,
    /// The query, cleaned, carried so nothing downstream re-derives what was searched.
    pub oligo: String,
    /// The aligned portion's Tm: the duplex that actually forms, overhangs excluded.
    pub anneal_tm: Option<f64>,
    /// Why this would or would not prime. Empty when it would, or when the question does not apply.
    pub priming_faults: Vec<String>,
    /// Whether priming is a question this fit can be asked: the annealed stretch is at most
    /// [`TM_MAX_NT`]. A 3 kb fragment cannot prime, and telling it its Tm is nonsense.
    pub primable: bool,
}

/// Below this a query is too short to mean anything; nothing is aligned.
pub const FIND_MIN_NT: usize = 6;
/// Above this Find says so rather than trying.
pub const FIND_MAX_NT: usize = 50_000;
/// The longest annealed stretch that is still told its Tm and whether it would prime.
pub const TM_MAX_NT: usize = 150;
/// Smith-Waterman cells Find will fill for one query: about two seconds here, and 60 MB of traceback. The web
/// takes 30 M and hands anything larger to minimap2; Bios has no second engine and aligns up to this.
pub const CELL_BUDGET: usize = 60_000_000;
/// At or below this many bases a query is aligned on every keystroke; above it, once typing stops.
pub const FIND_LIVE_NT: usize = 200;
/// How many fits are DRAWN at once. Ranking is exhaustive, drawing is not: the bar counts them all
/// and Enter walks the whole list.
pub const DRAWN_LIMIT: usize = 6;
/// The score floor stops climbing here (30 clean bases at match +2).
const LONG_FLOOR: f64 = 60.0;

/// JavaScript's `Math.round`: halves go up.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Bases matched over the whole query, 0..1: what a researcher reads as "how well does it fit".
pub fn coverage(h: &AlignHit) -> f64 {
    let n = h.oligo.len();
    if n == 0 {
        return 0.0;
    }
    n.saturating_sub(h.alignment.leading_overhang + h.alignment.trailing_overhang) as f64 / n as f64
}

/// Smith-Waterman cells a query of this length costs against a construct of this length.
pub fn cells_for(target_length: usize, query_length: usize, circular: bool) -> usize {
    let hay = target_length + if circular { query_length.saturating_sub(1).min(target_length) } else { 0 };
    hay.saturating_mul(query_length)
}

/// The longest query [`CELL_BUDGET`] admits against a construct of this length, never more than
/// [`FIND_MAX_NT`].
pub fn query_limit(target_length: usize, circular: bool) -> usize {
    if target_length == 0 {
        return 0;
    }
    let (l, b) = (target_length as f64, CELL_BUDGET as f64);
    let m = if circular {
        // (L + m)·m ≤ B while m ≤ L, the positive root; past L the prefix stops growing: 2L·m ≤ B.
        let root = ((-l + (l * l + 4.0 * b).sqrt()) / 2.0).floor();
        if root <= l { root } else { (b / (2.0 * l)).floor() }
    } else {
        (b / l).floor()
    };
    (m.max(0.0) as usize).min(FIND_MAX_NT)
}

/// The lowest score worth reporting for a query of this length: about 0.6 a base, at least 10,
/// and never past [`LONG_FLOOR`], so a long query can still report a PIECE of itself found elsewhere.
pub fn min_score_for(query_length: usize) -> f64 {
    js_round(query_length as f64 * 0.6).clamp(10.0, LONG_FLOOR)
}

/// Every place `oligo` fits `sequence`, best first. Never filters on primability. Empty for a query
/// under [`FIND_MIN_NT`] bases or one past [`CELL_BUDGET`] against this sequence.
pub fn alignment_hits(sequence: &str, circular: bool, oligo: &str) -> Vec<AlignHit> {
    let bases = clean_seq(oligo);
    if sequence.is_empty() || bases.len() < FIND_MIN_NT || cells_for(sequence.len(), bases.len(), circular) > CELL_BUDGET {
        return Vec::new();
    }
    let options = AlignOptions { min_score: min_score_for(bases.len()), circular, ..ALIGN_DEFAULTS };
    decorate_hits(&bases, local_align(sequence, &bases, options))
}

fn tm_of(s: &str) -> Option<f64> {
    if s.len() >= 2 { tm(s, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm) } else { None }
}

/// The judgement on each fit (its Tm, whether it would prime) and the id the drawing keys on.
pub fn decorate_hits(bases: &str, alignments: Vec<LocalAlignment>) -> Vec<AlignHit> {
    alignments
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let end = bases.len().saturating_sub(a.trailing_overhang);
            let annealed = bases.get(a.leading_overhang.min(end)..end).unwrap_or_default();
            let primable = annealed.len() <= TM_MAX_NT;
            AlignHit {
                id: format!("probe-{i}"),
                oligo: bases.to_string(),
                anneal_tm: if primable { tm_of(annealed) } else { None },
                priming_faults: if primable { priming_faults(&a, annealed) } else { Vec::new() },
                primable,
                alignment: a,
            }
        })
        .collect()
}

/// What would stop this priming: a label, never a filter. A polymerase extends from the 3'-OH, so
/// an unpaired or mismatched 3' end cannot prime, and neither can a duplex melting under 40°C.
fn priming_faults(a: &LocalAlignment, annealed: &str) -> Vec<String> {
    let mut faults = Vec::new();
    if a.trailing_overhang > 0 {
        faults.push("3' end unpaired".to_string());
    } else if a.blocks.last().is_some_and(|b| b.op != AlignOp::Match) {
        faults.push("3' end mismatched".to_string());
    }
    match tm_of(annealed) {
        Some(t) if t >= 40.0 => {}
        Some(t) => faults.push(format!("Tm {}°C", js_round(t))),
        None => faults.push("Tm —°C".to_string()),
    }
    faults
}

/// The extra bases themselves, 5'→3', across every insertion in the hit.
fn inserted_bases(h: &AlignHit) -> String {
    h.alignment
        .blocks
        .iter()
        .filter(|b| b.op == AlignOp::Insertion)
        .filter_map(|b| b.query_start.and_then(|s| h.oligo.get(s..s + b.query_length)))
        .collect()
}

/// The bases when there are few enough to read, the size otherwise (the 12-base cut VERIFY uses).
fn length_change(seq: &str, size: usize) -> String {
    if !seq.is_empty() && seq.len() <= 12 { seq.to_string() } else { format!("{size} bp") }
}

/// A whole number with thousands separators, as `toLocaleString("en-US")` prints it.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A 1-based plus-strand column, wrapped onto the construct when its length is known.
fn col(x: usize, target_length: Option<usize>) -> String {
    let c = target_length.filter(|&n| n > 0).map_or(x, |n| x % n);
    thousands(c + 1)
}

/// Each length change with its size and where it sits: `35 bp extra after 1,300`, `300 bp missing
/// at 2,351..2,650`. Named while there are three or fewer; counted past that.
fn indels_of(h: &AlignHit, target_length: Option<usize>) -> Vec<String> {
    let blocks = &h.alignment.blocks;
    let mut events = Vec::new();
    let (mut insertions, mut deletions) = (0, 0);
    for (k, b) in blocks.iter().enumerate() {
        match b.op {
            AlignOp::Insertion => {
                insertions += 1;
                // The template base before the insert in plus-strand order.
                let beside = if h.alignment.forward { k.checked_sub(1).and_then(|j| blocks.get(j)) } else { blocks.get(k + 1) };
                let after = beside
                    .filter(|s| s.target_length > 0)
                    .and_then(|s| s.target_start)
                    .zip(beside)
                    .map(|(start, s)| col(start + s.target_length - 1, target_length));
                events.push(match after {
                    Some(after) => format!("{} bp extra after {after}", b.query_length),
                    None => format!("{} bp extra", b.query_length),
                });
            }
            AlignOp::Deletion => {
                deletions += 1;
                let start = b.target_start.unwrap_or(0);
                events.push(format!(
                    "{} bp missing at {}..{}",
                    b.target_length,
                    col(start, target_length),
                    col(start + b.target_length.saturating_sub(1), target_length)
                ));
            }
            AlignOp::Match | AlignOp::Mismatch => {}
        }
    }
    if events.len() <= 3 {
        return events;
    }
    let mut counted = Vec::new();
    if insertions > 0 {
        counted.push(format!("{insertions} insertion{}", if insertions > 1 { "s" } else { "" }));
    }
    if deletions > 0 {
        counted.push(format!("{deletions} deletion{}", if deletions > 1 { "s" } else { "" }));
    }
    counted
}

/// What this fit says about itself, in one line: `22/28 nt aligned · 95% identity · 1 mismatch ·
/// 6 nt overhang · Tm 59°C · would prime`.
///
/// The two length changes are named in WORDS (`GGGGG extra`, `5 bp missing`), never by a sign,
/// which is VERIFY's rule. Identity is FLOORED: 99.6% never reads as 100% beside a mismatch. A long
/// fit is not asked to prime and names each length change with a place instead. `target_length`,
/// when known, lets a fit that laps the construct say so.
pub fn describe_hit(h: &AlignHit, target_length: Option<usize>) -> String {
    let a = &h.alignment;
    let mut parts = Vec::new();
    let ins: usize = a.blocks.iter().filter(|b| b.op == AlignOp::Insertion).map(|b| b.query_length).sum();
    let del: usize = a.blocks.iter().filter(|b| b.op == AlignOp::Deletion).map(|b| b.target_length).sum();
    // Bases that landed on a column: not the overhangs, and not an insertion.
    let aligned = h.oligo.len().saturating_sub(a.leading_overhang + a.trailing_overhang + ins);
    parts.push(format!("{aligned}/{} nt aligned", h.oligo.len()));
    if target_length.is_some_and(|n| n > 0 && a.end.saturating_sub(a.start) + 1 >= n) {
        parts.push("covers the whole construct".to_string());
    }
    parts.push(format!("{}% identity", (a.identity * 100.0).floor()));
    let mismatches = a.mismatch_positions.len();
    if mismatches > 0 {
        parts.push(format!("{mismatches} mismatch{}", if mismatches > 1 { "es" } else { "" }));
    }
    if h.primable {
        if ins > 0 {
            parts.push(format!("{} extra", length_change(&inserted_bases(h), ins)));
        }
        if del > 0 {
            parts.push(format!("{del} bp missing"));
        }
    } else {
        parts.extend(indels_of(h, target_length));
    }
    let overhang = a.leading_overhang + a.trailing_overhang;
    if overhang > 0 {
        parts.push(format!("{overhang} nt overhang"));
    }
    if let Some(t) = h.anneal_tm {
        parts.push(format!("Tm {}°C", js_round(t)));
    }
    if h.primable {
        parts.push(if h.priming_faults.is_empty() { "would prime".to_string() } else { format!("won't prime — {}", h.priming_faults.join(", ")) });
    }
    parts.join(" · ")
}

/// The runs for a hit: the seam between the aligner's coordinates and the drawing.
pub fn hit_runs(h: &AlignHit, target_length: usize, circular: bool) -> Vec<AlignRun> {
    alignment_runs(&h.alignment, &h.oligo, target_length, circular)
}

/// The query's bases laid against the template's columns, in QUERY order 5'→3', a deletion filled
/// with `&` (a column the oligo has no base for). A reverse hit is complemented and placed right to
/// left by whoever draws it, the same contract a made primer's bases follow.
pub fn template_aligned_bases(runs: &[AlignRun]) -> String {
    runs.iter()
        .filter(|r| r.template_length > 0)
        .map(|r| if r.kind == AlignRunKind::Deletion { "&".repeat(r.template_length) } else { r.bases.clone() })
        .collect()
}

/// What a drawn hit is called beside its 5' end: `probe`, or `probe — <first fault>`.
pub fn probe_name(h: &AlignHit) -> String {
    h.priming_faults.first().map_or_else(|| "probe".to_string(), |f| format!("probe — {f}"))
}

/// One drawable chunk of a run: the part of it that fits on one row of the sequence view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSegment {
    pub row_index: usize,
    pub col_in_row: usize,
    pub length: usize,
    /// Bases in COLUMN order: already reversed for a reverse-strand run.
    pub text: String,
    pub kind: AlignRunKind,
    pub paired: bool,
}

/// Cut runs into per-row chunks, in column order. An insertion has no columns, so it gets none
/// here (see [`row_inserts`]).
pub fn row_segments(runs: &[AlignRun], bps_per_row: usize, target_length: usize, circular: bool) -> Vec<RowSegment> {
    let (Ok(n), Ok(per)) = (i64::try_from(target_length), i64::try_from(bps_per_row)) else {
        return Vec::new();
    };
    if per <= 0 || n <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for run in runs.iter().filter(|r| r.kind != AlignRunKind::Insertion) {
        let span = run.length.max(run.template_length);
        let bases = run.bases.as_bytes();
        let mut cells: Vec<(i64, u8)> = Vec::with_capacity(span);
        for k in 0..span {
            let mut c = run.start_col.saturating_add(i64::try_from(k).unwrap_or(i64::MAX).saturating_mul(run.step));
            if circular {
                c = c.rem_euclid(n);
            } else if c < 0 || c >= n {
                continue;
            }
            cells.push((c, bases.get(k).copied().unwrap_or(b' ')));
        }
        cells.sort_by_key(|&(c, _)| c);
        let mut i = 0;
        while i < cells.len() {
            let row = cells[i].0 / per;
            let mut j = i + 1;
            while j < cells.len() && cells[j].0 == cells[j - 1].0 + 1 && cells[j].0 / per == row {
                j += 1;
            }
            out.push(RowSegment {
                row_index: usize::try_from(row).unwrap_or(0),
                col_in_row: usize::try_from(cells[i].0 - row * per).unwrap_or(0),
                length: j - i,
                text: cells[i..j].iter().map(|&(_, b)| char::from(b)).collect(),
                kind: run.kind,
                paired: run.paired,
            });
            i = j;
        }
    }
    out.sort_by_key(|s| (s.row_index, s.col_in_row));
    out
}

/// Bases the oligo carries that the template has no column for, and the boundary they belong
/// between: `boundary` 11 means between the 11th and 12th base of the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowInsert {
    pub row_index: usize,
    pub boundary: usize,
    /// The extra bases, in query order 5'→3'.
    pub bases: String,
}

/// Every insertion in these runs, placed on the boundary it interrupts. `start_col` on an insertion
/// run is the column the alignment RESUMES at, so the extra bases fall just before it going forward
/// and just after it going back: the same edge once named by the column to its right.
pub fn row_inserts(runs: &[AlignRun], bps_per_row: usize, target_length: usize, circular: bool) -> Vec<RowInsert> {
    let (Ok(n), Ok(per)) = (i64::try_from(target_length), i64::try_from(bps_per_row)) else {
        return Vec::new();
    };
    if per <= 0 || n <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for run in runs.iter().filter(|r| r.kind == AlignRunKind::Insertion && !r.bases.is_empty()) {
        let mut edge = if run.step == 1 { run.start_col } else { run.start_col.saturating_add(1) };
        if circular {
            edge = edge.rem_euclid(n);
        } else if edge < 0 || edge > n {
            continue;
        }
        // An insert past the last base belongs on the right edge of the last row.
        let row = edge.min(n - 1) / per;
        out.push(RowInsert {
            row_index: usize::try_from(row).unwrap_or(0),
            boundary: usize::try_from(edge - row * per).unwrap_or(0),
            bases: run.bases.clone(),
        });
    }
    out
}
