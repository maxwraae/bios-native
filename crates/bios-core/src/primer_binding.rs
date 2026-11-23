//! Comparing a saved primer with the template it sits on. Ported from the Bios web app.
//!
//! Coordinates are 0-based inclusive on the template's plus strand. A stored site through the
//! origin of a circular template has `end < start`. Offsets into the oligo count from its 5' end,
//! deliberate tail included.

use crate::align::{AlignBlock, AlignOp, LocalAlignment};
use crate::duplex::{AlignRun, alignment_runs};
use crate::locations::normalized_feature_locations;
use crate::model::{Feature, Fuzzy, LocationOperator, Segment, Strand};
use crate::seq::{DNA_TM_DEFAULTS, complement, is_js_whitespace, tm};

/// A saved oligo, the template and where the oligo is stored as binding.
///
/// `Default` leaves out what the web app lets a caller leave out: no tail, nothing inferred, bound,
/// and a plain contiguous location. Always name the rest yourself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimerBindingInput<'a> {
    pub sequence: &'a str,
    /// Full oligo, always 5′→3′, including its deliberate 5′ tail.
    pub bases: &'a str,
    /// Zero-based inclusive template coordinates; ignored when unbound. Signed because a stored
    /// site can be nonsense (an unbound oligo is kept with -1), and nonsense is reported, not
    /// refused by the type.
    pub start: i64,
    pub end: i64,
    pub forward: bool,
    pub tail_length: usize,
    /// A legacy length-based guess must not be presented as confirmed tail metadata.
    pub tail_length_inferred: bool,
    pub circular: bool,
    pub unbound: bool,
    /// The pieces of the stored location, as [`Feature::segments`] holds them: none or one for a
    /// simple span.
    pub locations: &'a [Segment],
    pub location_operator: Option<LocationOperator>,
    pub start_fuzzy: Option<Fuzzy>,
    pub end_fuzzy: Option<Fuzzy>,
    pub location_unsupported: Option<&'a str>,
}

/// The value of [`PrimerBindingCheck::coordinate_system`].
pub const COORDINATE_SYSTEM: &str = "0-based-inclusive";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingStatus {
    Exact,
    Changed,
    Unbound,
    Unverified,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimerBindingCheck {
    pub coordinate_system: &'static str,
    pub status: BindingStatus,
    pub summary: String,
    pub bases: String,
    pub tail: String,
    pub annealing_bases: String,
    /// Template in the oligo's direction, 5′→3′.
    pub template_bases: String,
    /// Zero-based offsets into the FULL oligo, excluding deliberate tail bases.
    pub mismatch_positions: Vec<usize>,
    pub ambiguous_positions: Vec<usize>,
    /// Bases present only in the oligo / only in the template, respectively.
    pub insertions: usize,
    pub deletions: usize,
    pub three_prime_matched: Option<bool>,
    /// Perfect-duplex estimate only; absent for differences, ambiguities or long fragments.
    pub annealing_tm: Option<f64>,
    /// The oligo's annealing bases and the template under them, column for column, with "-" where
    /// one has no base for the other.
    pub aligned_primer: Option<String>,
    pub aligned_template: Option<String>,
    pub runs: Vec<AlignRun>,
}

const MAX_CELLS: usize = 1_000_000;
const GAP: u8 = b'-';

fn concrete(base: u8) -> bool {
    matches!(base, b'A' | b'C' | b'G' | b'T')
}

/// The same test for one unit of template text.
fn concrete_unit(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(concrete)
}

/// Uppercased, with white space dropped.
pub(crate) fn normalize(bases: &str) -> String {
    bases.chars().flat_map(char::to_uppercase).filter(|c| !is_js_whitespace(*c)).collect()
}

/// The template as the web app counts it: uppercased, white space dropped, one position per
/// UTF-16 unit. DNA is one unit a base either way; this only keeps a template holding other text
/// at the same coordinates here as there.
fn template_units(sequence: &str) -> Vec<u16> {
    let mut pair = [0u16; 2];
    let mut units: Vec<u16> = Vec::with_capacity(sequence.len());
    for c in sequence.chars().flat_map(char::to_uppercase).filter(|c| !is_js_whitespace(*c)) {
        units.extend_from_slice(c.encode_utf16(&mut pair));
    }
    units
}

/// The reverse complement of template text. A unit that is not a base passes through unchanged.
fn revcomp_units(units: &[u16]) -> Vec<u16> {
    units.iter().rev().map(|&unit| u8::try_from(unit).map_or(unit, |base| u16::from(complement(base)))).collect()
}

/// Global unit-cost edit alignment of the oligo's annealing bases against the template under
/// them, both in the oligo's direction. `None` when the comparison is too large to make.
fn edit_operations(query: &[u8], target: &[u16]) -> Option<Vec<AlignOp>> {
    let m = query.len();
    let n = target.len();
    let same = |q: u8, t: u16| u16::from(q) == t && concrete(q);
    if m == n && query.iter().zip(target).all(|(&q, &t)| same(q, t)) {
        return Some(vec![AlignOp::Match; m]);
    }
    if (m + 1).checked_mul(n + 1).is_none_or(|cells| cells > MAX_CELLS) {
        return None;
    }
    // Rolling score rows; only traceback directions require the full matrix.
    let width = n + 1;
    let mut trace = vec![0u8; (m + 1) * width];
    let mut previous: Vec<u32> = (0u32..).take(width).collect();
    let mut current = vec![0u32; width];
    let mut rows = trace.chunks_exact_mut(width);
    if let Some(top) = rows.next() {
        top.iter_mut().skip(1).for_each(|direction| *direction = 2);
    }
    for ((row, &q), i) in rows.zip(query).zip(1u32..) {
        let (Some((first, rest)), Some((edge, directions))) = (current.split_first_mut(), row.split_first_mut()) else {
            break;
        };
        *first = i;
        *edge = 1;
        let mut left = i;
        for (((above, here), direction), &t) in previous.windows(2).zip(rest).zip(directions).zip(target) {
            let [diagonal, up] = above else {
                continue;
            };
            let mut score = diagonal + u32::from(!same(q, t));
            *direction = 0;
            if up + 1 < score {
                score = up + 1;
                *direction = 1;
            }
            if left + 1 < score {
                score = left + 1;
                *direction = 2;
            }
            *here = score;
            left = score;
        }
        std::mem::swap(&mut previous, &mut current);
    }

    let mut operations: Vec<AlignOp> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        let Some(&direction) = trace.get(i * width + j) else {
            break;
        };
        // The edges of the matrix only ever point along themselves, so neither index can pass 0.
        match direction {
            1 => {
                operations.push(AlignOp::Insertion);
                i = i.saturating_sub(1);
            }
            2 => {
                operations.push(AlignOp::Deletion);
                j = j.saturating_sub(1);
            }
            _ => {
                let (Some(qi), Some(tj)) = (i.checked_sub(1), j.checked_sub(1)) else {
                    break;
                };
                let paired = query.get(qi).zip(target.get(tj)).is_some_and(|(&q, &t)| same(q, t));
                operations.push(if paired { AlignOp::Match } else { AlignOp::Mismatch });
                i = qi;
                j = tj;
            }
        }
    }
    operations.reverse();
    Some(operations)
}

/// "1 mismatch", "2 mismatches".
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A whole coordinate inside a template of `length` bases.
fn inside(coordinate: i64, length: usize) -> Option<usize> {
    usize::try_from(coordinate).ok().filter(|&n| n < length)
}

/// Whether the stored location is anything but one contiguous span.
fn segmented(input: &PrimerBindingInput<'_>, length: usize) -> bool {
    // A lone piece through the origin counts as its two arms, as the web app splits it. Only
    // several pieces are looked at below, so the arms are made here.
    let pieces: Vec<Segment> = match input.locations {
        [piece] if input.circular && length > 0 && piece.end < piece.start => vec![
            Segment { start: piece.start, end: length - 1, start_fuzzy: piece.start_fuzzy, end_fuzzy: None },
            Segment { start: 0, end: piece.end, start_fuzzy: None, end_fuzzy: piece.end_fuzzy },
        ],
        pieces => pieces.to_vec(),
    };
    let (Ok(start), Ok(end)) = (usize::try_from(input.start), usize::try_from(input.end)) else {
        // A coordinate before the template is never the plain join through the origin that reads
        // as one span, so several pieces stay several pieces.
        return pieces.len() > 1;
    };
    let feature = Feature {
        name: String::new(),
        kind: String::new(),
        start,
        end,
        strand: if input.forward { Strand::Forward } else { Strand::Reverse },
        segments: pieces,
        notes: Vec::new(),
        start_fuzzy: input.start_fuzzy,
        end_fuzzy: input.end_fuzzy,
        location_operator: input.location_operator,
        location_unsupported: None,
    };
    normalized_feature_locations(&feature, length, input.circular).is_some()
}

/// Compare a saved oligo with its stored footprint, without changing either.
///
/// This is a sequence comparison, not a prediction that PCR will succeed or a search for other
/// binding sites. Global unit-cost edit alignment keeps both ends and exposes
/// insertions/deletions. Ties prefer the direct substitution; repetitive sequence may admit other
/// equally good gap placements.
pub fn compare_primer_binding(input: &PrimerBindingInput<'_>) -> PrimerBindingCheck {
    let bases = normalize(input.bases);
    let sequence = template_units(input.sequence);
    let tail_length = input.tail_length;
    let mut out = PrimerBindingCheck {
        coordinate_system: COORDINATE_SYSTEM,
        status: BindingStatus::Unverified,
        summary: "Binding comparison unavailable".to_string(),
        bases: bases.clone(),
        tail: String::new(),
        annealing_bases: bases.clone(),
        template_bases: String::new(),
        mismatch_positions: Vec::new(),
        ambiguous_positions: Vec::new(),
        insertions: 0,
        deletions: 0,
        three_prime_matched: None,
        annealing_tm: None,
        aligned_primer: None,
        aligned_template: None,
        runs: Vec::new(),
    };
    let iupac =
        |base: u8| matches!(base, b'A' | b'C' | b'G' | b'T' | b'U' | b'R' | b'Y' | b'S' | b'W' | b'K' | b'M' | b'B' | b'D' | b'H' | b'V' | b'N');
    // Only single-byte letters pass, so the oligo can be cut at any offset from here on.
    let valid = !bases.is_empty() && bases.bytes().all(iupac) && tail_length < bases.len();
    let Some((tail, annealing)) = bases.split_at_checked(tail_length).filter(|_| valid) else {
        out.summary = "Oligo or 5′ tail length is invalid".to_string();
        return out;
    };
    out.tail = tail.to_string();
    out.annealing_bases = annealing.to_string();
    if input.unbound {
        out.status = BindingStatus::Unbound;
        out.summary = "No stored binding site — oligo retained".to_string();
        return out;
    }
    let length = sequence.len();
    if input.location_unsupported.is_some_and(|text| !text.is_empty())
        || input.location_operator == Some(LocationOperator::Order)
        || input.start_fuzzy.is_some()
        || input.end_fuzzy.is_some()
        || segmented(input, length)
    {
        out.summary = "Stored site is segmented, uncertain, or unsupported; binding is unverified".to_string();
        return out;
    }
    let forward = input.forward;
    let circular = input.circular;
    let footprint =
        inside(input.start, length).zip(inside(input.end, length)).filter(|&(start, end)| circular || end >= start).and_then(|(start, end)| {
            let units = if start <= end { sequence.get(start..=end)?.to_vec() } else { [sequence.get(start..)?, sequence.get(..=end)?].concat() };
            Some((start, end, units))
        });
    let Some((start, end, footprint)) = footprint else {
        out.summary = "Stored binding coordinates are outside the template".to_string();
        return out;
    };
    let target = if forward { footprint } else { revcomp_units(&footprint) };
    let query = annealing.as_bytes();
    out.template_bases = String::from_utf16_lossy(&target);
    let Some(operations) = edit_operations(query, &target) else {
        out.summary = "Stored footprint is too large for the primer comparison".to_string();
        return out;
    };

    let mut blocks: Vec<AlignBlock> = Vec::new();
    let mut aligned_primer = String::with_capacity(operations.len());
    let mut aligned_template: Vec<u16> = Vec::with_capacity(operations.len());
    let mut qi = tail_length;
    let mut ti = 0usize;
    // The plus-strand column under the footprint's base `i`, counted in the oligo's direction.
    let col = |i: usize| if forward { (start + i) % length } else { (end + length - i % length) % length };
    for &op in &operations {
        let qlen = usize::from(op != AlignOp::Deletion);
        let tlen = usize::from(op != AlignOp::Insertion);
        let qbase = if qlen > 0 { bases.as_bytes().get(qi).copied().unwrap_or(GAP) } else { GAP };
        let tbase = if tlen > 0 { target.get(ti).copied().unwrap_or(u16::from(GAP)) } else { u16::from(GAP) };
        aligned_primer.push(char::from(qbase));
        aligned_template.push(tbase);
        if op == AlignOp::Mismatch {
            if concrete(qbase) && concrete_unit(tbase) {
                out.mismatch_positions.push(qi);
            } else {
                out.ambiguous_positions.push(qi);
            }
        } else if qlen > 0 && !concrete(qbase) {
            out.ambiguous_positions.push(qi);
        }
        if op == AlignOp::Insertion {
            out.insertions += 1;
        }
        if op == AlignOp::Deletion {
            out.deletions += 1;
        }
        if let Some(last) = blocks.last_mut()
            && last.op == op
        {
            last.query_length += qlen;
            last.target_length += tlen;
        } else {
            blocks.push(AlignBlock {
                op,
                query_start: (qlen > 0).then_some(qi),
                query_length: qlen,
                target_start: (tlen > 0).then(|| col(ti)),
                target_length: tlen,
            });
        }
        qi += qlen;
        ti += tlen;
    }
    out.aligned_primer = Some(aligned_primer);
    out.aligned_template = Some(String::from_utf16_lossy(&aligned_template));
    // Template beyond the oligo does not unpair its last nucleotide.
    let three_prime_matched = operations.iter().rev().find(|op| **op != AlignOp::Deletion) == Some(&AlignOp::Match);
    out.three_prime_matched = Some(three_prime_matched);
    let matches = operations.iter().filter(|op| **op == AlignOp::Match).count();
    let exact = matches == operations.len();
    out.status = if exact { BindingStatus::Exact } else { BindingStatus::Changed };
    out.annealing_tm =
        if exact && (2..=150).contains(&query.len()) { tm(annealing, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm) } else { None };
    let mut details: Vec<String> = Vec::new();
    if !out.mismatch_positions.is_empty() {
        details.push(counted(out.mismatch_positions.len(), "mismatch", "mismatches"));
    }
    if !out.ambiguous_positions.is_empty() {
        details.push(counted(out.ambiguous_positions.len(), "ambiguous position", "ambiguous positions"));
    }
    if out.insertions > 0 {
        details.push(format!("{} nt extra in oligo", out.insertions));
    }
    if out.deletions > 0 {
        details.push(format!("{} nt missing from oligo", out.deletions));
    }
    if tail_length > 0 {
        details.push(format!("{tail_length} nt 5′ tail"));
    }
    let details = details.join(" · ");
    out.summary = match (exact, details.is_empty()) {
        (true, true) => "Matches stored binding region".to_string(),
        (true, false) => format!("Matches stored binding region · {details}"),
        (false, _) => details,
    };
    if !three_prime_matched {
        out.summary.push_str(" · 3′ end not paired");
    }
    let alignment = LocalAlignment {
        start,
        end,
        forward,
        score: 0.0,
        identity: matches as f64 / operations.len() as f64,
        blocks,
        leading_overhang: tail_length,
        trailing_overhang: 0,
        mismatch_positions: out.mismatch_positions.clone(),
    };
    out.runs = alignment_runs(&alignment, &bases, length, circular);
    if input.tail_length_inferred {
        out.status = BindingStatus::Unverified;
        out.annealing_tm = None;
        out.summary = format!("Assuming an inferred {tail_length} nt 5′ tail · {}. Confirm tail length to verify.", out.summary);
    }
    out
}
