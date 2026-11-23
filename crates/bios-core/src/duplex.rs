//! The duplex, run by run: what a drawing of a binding site needs, and nothing else. Ported from
//! the Bios web app.
//!
//! `find_primer_sites` answers WHERE an oligo primes. This answers WHAT THAT LOOKS LIKE: which
//! stretches of the oligo pair with the template, which don't, and the plus-strand column each
//! stretch sits over. It lives here, away from any pixel, because it is a statement about which
//! base faces which position: coordinates, not layout.
//!
//! The grammar it serves: a primer is ONE molecule read 5'→3', so paired runs sit flat on the
//! template and unpaired runs lift one line away from it, IN THE COLUMNS THEY WOULD OCCUPY. A 5'
//! tail and an internal mismatch are the same gesture in different places, which is exactly why
//! they are the same shape here: a run with `paired: false`. The renderer needs no special case
//! for either, and adding a third kind of not-paired later costs it nothing.
//!
//! The trap this module exists to contain: the reverse strand. For a FORWARD site the oligo's 5'
//! end sits at `start` and columns ASCEND as you read the oligo, so its tail hangs off to the
//! LEFT. For a REVERSE site the oligo's 3' base sits at `start` and its 5' end runs rightward to
//! `end`, so columns DESCEND as you read the oligo and the tail hangs off to the RIGHT. Getting
//! this backwards mirrors the entire drawing (every tail on the wrong side of every reverse
//! primer) and it never fails. It is the same class of silent wrongness the start/end note in
//! the primer search warns about, so it is pinned by one anchor-plus-step expression below rather
//! than by two parallel branches.
//!
//! Columns are 0-based on the plus strand and signed: on a linear target a tail can sit before
//! column 0 or past the last one.

use std::collections::HashSet;

use crate::align::{AlignOp, LocalAlignment};
use crate::primer::PrimerSite;
use crate::seq::clean_seq;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    Tail,
    Anneal,
    Mismatch,
}

/// One contiguous stretch of the oligo that is doing the same thing all the way along.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplexRun {
    pub kind: RunKind,
    /// True for `Anneal` only: the runs drawn flat against the template. Everything else lifts.
    /// Derived rather than inferred by the caller so "on the line / off the line" is decided in
    /// exactly one place.
    pub paired: bool,
    /// This run's own bases, taken from the OLIGO (not the template), 5'→3'.
    pub bases: String,
    /// 0-based index into the oligo, 5'→3', of this run's first base.
    pub oligo_start: usize,
    pub length: usize,
    /// Plus-strand column of the base at `oligo_start`. Subsequent bases sit at
    /// `start_col + k * step`. Normalised into `0..target_length` when circular; left raw when
    /// linear, where a tail can legitimately fall off the end (see `off_template`).
    pub start_col: i64,
    /// +1 when reading the oligo 5'→3' moves rightward along the plus strand, -1 when leftward.
    pub step: i64,
    /// True when any base of this run has no column on a linear target. Reported rather than
    /// clamped: clamping would stack the whole tail onto base 0 and draw a lie. The renderer
    /// decides whether to hang it in the margin or cut it with a count.
    pub off_template: bool,
}

/// A length or a column as a signed number. One too large to hold is pinned at the largest, so
/// that a hostile site draws off the template rather than wrapping round to a real column.
fn signed(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The column `offset` steps along from `col`.
fn col_at(col: i64, offset: i64, step: i64) -> i64 {
    col.saturating_add(offset.saturating_mul(step))
}

/// Whether any of the `count` columns stepping from `col` has no place on a linear target of `n`
/// bases. The columns run one way, so the first and the last answer for all of them; the web app
/// walks every one, which a hostile length would turn into a very long walk.
fn off_template(col: i64, count: usize, step: i64, n: i64) -> bool {
    count > 0 && !((0..n).contains(&col) && (0..n).contains(&col_at(col, signed(count - 1), step)))
}

/// `s[start..end]`, with both ends pulled inside the text as the web app's `slice` does.
fn slice(s: &str, start: usize, end: usize) -> &str {
    let start = start.min(s.len());
    s.get(start..end.clamp(start, s.len())).unwrap_or_default()
}

/// Break a binding site into drawable runs, in oligo order (5'→3').
///
/// `oligo` must be the same one the site was found for: the site's own `tail_length +
/// anneal_length` is checked against it and a disagreement returns nothing rather than a drawing
/// built from two different molecules.
pub fn duplex_runs(site: &PrimerSite, oligo: &str, target_length: usize, circular: bool) -> Vec<DuplexRun> {
    let p = clean_seq(oligo);
    let m = p.len();
    let n = signed(target_length);
    if m == 0 || n <= 0 {
        return Vec::new();
    }
    // The site describes this oligo or it describes nothing. Silence beats a confident drawing of
    // a molecule nobody asked about.
    if site.tail_length.checked_add(site.anneal_length) != Some(m) {
        return Vec::new();
    }

    let tail_length = site.tail_length;
    let step: i64 = if site.forward { 1 } else { -1 };
    // The one place the strand is decided. `anchor_col` is the column of the oligo's 5'-most
    // ANNEALED base (index `tail_length`): `start` reading rightward, `end` reading leftward. And
    // every other base, tail included, follows by the same step. Anchoring on the anneal and
    // extending through the tail is what makes the tail contiguous with it by construction, which
    // is the whole "one molecule" claim the drawing makes.
    let anchor_col = signed(if site.forward { site.start } else { site.end });
    let raw_col = |i: usize| col_at(anchor_col, signed(i) - signed(tail_length), step);
    // A wrapped site has end < start, so raw columns run past n (or below 0) and come back by the
    // modulo: the origin needs no special case.
    let norm = |c: i64| c.rem_euclid(n);

    let mism: HashSet<usize> = site.mismatch_positions.iter().copied().collect();
    let mut runs: Vec<DuplexRun> = Vec::new();

    let mut push = |kind: RunKind, i0: usize, len: usize| {
        let off = !circular && off_template(raw_col(i0), len, step, n);
        runs.push(DuplexRun {
            kind,
            paired: kind == RunKind::Anneal,
            bases: slice(&p, i0, i0 + len).to_string(),
            oligo_start: i0,
            length: len,
            start_col: if circular { norm(raw_col(i0)) } else { raw_col(i0) },
            step,
            off_template: off,
        });
    };

    // The tail is one run however long it is: none of it pairs, so there is nothing inside it to
    // split on. It is emitted first because runs are in oligo order and the tail is always 5'.
    if tail_length > 0 {
        push(RunKind::Tail, 0, tail_length);
    }

    // Then the annealed region, split wherever pairing changes. Consecutive mismatches merge into
    // ONE run: "ten nucleotides here don't match" is a single lifted stretch, not ten adjacent
    // one-base runs the renderer would have to stitch back together.
    let mut i = tail_length;
    while i < m {
        let unpaired = mism.contains(&i);
        let mut j = i + 1;
        while j < m && mism.contains(&j) == unpaired {
            j += 1;
        }
        push(if unpaired { RunKind::Mismatch } else { RunKind::Anneal }, i, j - i);
        i = j;
    }

    runs
}

// Runs from a local alignment.
//
// The sibling of `duplex_runs` for the aligner, and the shape the drawing actually wants now that
// the grammar has two signals instead of one:
//
//   ON the line, in its own column   a base the template has a position for. `Match` draws
//                                    normally, `Mismatch` draws RED. Both stay in place.
//   OFF the line, lifted             a base with NO column at all: a 5'/3' overhang, or an
//                                    insertion the template has no room for.
//
// That split is the whole reason mismatches stopped lifting. Lifting used to mean "unpaired",
// which conflated two different facts: "wrong base here" and "no here to be at". Now the lift
// means exactly the second, red means the first, and neither has to explain the other.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignRunKind {
    Match,
    Mismatch,
    Insertion,
    Overhang,
    Deletion,
}

/// A [`DuplexRun`] with the aligner's kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignRun {
    pub kind: AlignRunKind,
    /// True for `Match` and `Mismatch`: the bases that sit in a column of their own.
    pub paired: bool,
    /// This run's own bases, taken from the query, 5'→3'. Empty for a deletion.
    pub bases: String,
    /// 0-based index into the query, 5'→3', of this run's first base. `None` for a deletion,
    /// which carries no query bases of its own (the web app's -1).
    pub oligo_start: Option<usize>,
    pub length: usize,
    /// Template columns this run covers. 0 for insertions and overhangs; `length` for matches and
    /// mismatches; the columns skipped for a deletion.
    pub template_length: usize,
    /// As in [`DuplexRun`]. For an insertion it is the column of the junction it interrupts.
    pub start_col: i64,
    pub step: i64,
    pub off_template: bool,
}

/// Break a local alignment into drawable runs, in query order (5'→3').
///
/// Columns follow the strand exactly as in [`duplex_runs`]: a forward alignment ascends from
/// `start`, a reverse one descends from `end`, and the overhangs continue past either end in the
/// same direction. That is why a reverse primer's 5' overhang lands to the RIGHT.
pub fn alignment_runs(a: &LocalAlignment, query: &str, target_length: usize, circular: bool) -> Vec<AlignRun> {
    let q = clean_seq(query);
    let n = signed(target_length);
    if q.is_empty() || n <= 0 {
        return Vec::new();
    }

    let step: i64 = if a.forward { 1 } else { -1 };
    // Column of the alignment's first query base: the low end going forward, the high end going
    // back. Every other column is this plus a signed offset, so there is one expression and not
    // two.
    let anchor_col = signed(if a.forward { a.start } else { a.end });
    let norm = |c: i64| c.rem_euclid(n);
    let mut runs: Vec<AlignRun> = Vec::new();

    let mut push = |kind: AlignRunKind, oligo_start: Option<usize>, length: usize, col: i64, template_length: usize| {
        let off = !circular && off_template(col, length.max(template_length), step, n);
        runs.push(AlignRun {
            kind,
            paired: matches!(kind, AlignRunKind::Match | AlignRunKind::Mismatch),
            bases: oligo_start.map(|start| slice(&q, start, start.saturating_add(length)).to_string()).unwrap_or_default(),
            oligo_start,
            length,
            template_length,
            start_col: if circular { norm(col) } else { col },
            step,
            off_template: off,
        });
    };

    // The 5' overhang sits BEFORE the alignment in query order, so its columns run backwards from
    // the anchor: contiguous with it, which is what makes the oligo read as one molecule.
    if a.leading_overhang > 0 {
        push(AlignRunKind::Overhang, Some(0), a.leading_overhang, col_at(anchor_col, -signed(a.leading_overhang), step), 0);
    }

    let mut col = anchor_col;
    for b in &a.blocks {
        match b.op {
            // No template columns at all. It is drawn over the junction it interrupts, so it
            // takes the current cursor as its position without advancing it.
            AlignOp::Insertion => push(AlignRunKind::Insertion, b.query_start, b.query_length, col, 0),
            AlignOp::Deletion => {
                push(AlignRunKind::Deletion, None, 0, col, b.target_length);
                col = col_at(col, signed(b.target_length), step);
            }
            AlignOp::Match | AlignOp::Mismatch => {
                let kind = if b.op == AlignOp::Match { AlignRunKind::Match } else { AlignRunKind::Mismatch };
                push(kind, b.query_start, b.query_length, col, b.target_length);
                col = col_at(col, signed(b.target_length), step);
            }
        }
    }

    if a.trailing_overhang > 0 {
        push(AlignRunKind::Overhang, Some(q.len().saturating_sub(a.trailing_overhang)), a.trailing_overhang, col, 0);
    }

    runs
}
