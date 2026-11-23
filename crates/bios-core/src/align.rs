//! How does this sequence FIT here, including where it doesn't. Ported from the Bios web app.
//!
//! This is a different question from [`crate::primer`], and the difference is why that module
//! could not be loosened into this one. `find_primer_sites` asks "where does this PRIME?": it pins
//! the oligo's 3' end because a polymerase extends from the 3'-OH, and it walks base against base.
//! Both choices are right for ordering an oligo and fatal for reading an alignment: a query whose
//! 3' end doesn't match is rejected outright, and a bulge cannot be expressed at all, so the
//! answer comes back empty. Empty is the worst possible answer to "how does this fit", because
//! the misalignment is the thing you were looking at.
//!
//! So: Smith-Waterman, local, with affine gaps. Local because a query that matches over half its
//! length should report that half rather than nothing. Affine because a 30 bp insert must cost
//! about the same as a 3 bp one plus a little (one event, not thirty), which is the only way
//! clean flanks either side can pay for it.
//!
//! The scoring, and why these numbers:
//!   What matters is the RATIO |mismatch| : match, not either number alone. To carry an alignment
//!   through a mismatch every N bases the N-1 matches must outpay the one mismatch, so tolerating
//!   1-in-3 needs a ratio better than 1:2. The primer search sits exactly AT 1:2 (+1/-2):
//!   break-even, the score never rises, the extension never commits. Measured, every 3rd base
//!   mismatched over 30: +1/-2 aligns 5, +1/-1 and +2/-2 (here) align 20. Going the other way
//!   (1:0.5) is WORSE in practice: against a real plasmid it stopped calling an 18 nt Gibson tail
//!   an overhang and chewed into it, reporting 5 invented mismatches. A mismatch has to cost
//!   enough that "this part simply isn't aligned" wins.
//!
//!   Gap open 12, extend 0.5, and the open cost is the one that matters. At 5 the aligner could
//!   buy homology that isn't there: the same clean tail became "4 mismatches and a 14 nt
//!   deletion", which scored HIGHER than the truthful answer. At 12 a gap has to be earned by
//!   real flanking homology either side. A 31 nt insert still bridges on 20 bp flanks (cost
//!   12 + 15 = 27 against +80 of flank), which is the case this is FOR.
//!
//! Coordinates: every column is 0-based on the target's plus strand, whichever strand the query
//! reads along. A circular target is searched as itself plus a query-length prefix, so an
//! alignment through the origin comes back in EXTENDED coordinates: its `end` (and the columns of
//! its later blocks) run past the target's length rather than wrapping to `end < start`. Take
//! them modulo the length, as [`crate::duplex::alignment_runs`] does.
//!
//! Complexity is O(n·m) time. Memory is O(n·m) BYTES: the score rows roll, and only the traceback
//! directions are kept in full, one byte a cell. A 6 kb plasmid against a 40-mer is 244 kB and
//! runs in a few ms, so nothing here needs banding at plasmid scale. Nothing here caps the table
//! either: a caller facing arbitrary input sets its own cell budget, as the web app's Find does.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::seq::{clean_seq, revcomp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignOp {
    Match,
    Mismatch,
    Insertion,
    Deletion,
}

/// One run of the alignment where the same thing is happening all the way along.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignBlock {
    pub op: AlignOp,
    /// 0-based index into the QUERY, 5'→3', of this block's first base. `None` for a deletion
    /// (the web app's -1).
    pub query_start: Option<usize>,
    /// Query bases consumed. 0 for a deletion (the query has nothing here).
    pub query_length: usize,
    /// 0-based plus-strand column of this block's lowest template base. `None` for an insertion
    /// (the web app's -1).
    pub target_start: Option<usize>,
    /// Template bases consumed. 0 for an insertion (the template has no column for these).
    pub target_length: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalAlignment {
    /// A query whose own sequence reads along the plus strand is `true`: the convention of
    /// [`crate::search`].
    pub forward: bool,
    /// 0-based inclusive plus-strand footprint of the ALIGNED region (overhangs excluded). See
    /// the note on extended coordinates above: through the origin, `end` is past the length.
    pub start: usize,
    pub end: usize,
    pub score: f64,
    /// Matching bases over aligned columns, 0..1. Gapped columns count against it.
    pub identity: f64,
    /// In QUERY order, 5'→3': columns ascend from `start` going forward and descend from `end`
    /// going back.
    pub blocks: Vec<AlignBlock>,
    /// Query bases before the alignment starts and after it ends, read 5'→3' on the query as the
    /// user typed it. These are the overhangs: a Gibson tail, a restriction site, or simply the
    /// part that stopped fitting. They have no template columns, which is why they are counted
    /// rather than placed.
    pub leading_overhang: usize,
    pub trailing_overhang: usize,
    /// Mismatched positions as 0-based indices into the query, 5'→3', ascending.
    pub mismatch_positions: Vec<usize>,
}

/// `Default` is [`ALIGN_DEFAULTS`]. The web app has no default for `circular`, because a circular
/// target searched as linear silently loses every alignment through the origin: always name it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignOptions {
    pub circular: bool,
    /// The web app's `match`.
    pub match_score: f64,
    /// The web app's `mismatch`.
    pub mismatch_score: f64,
    pub gap_open: f64,
    pub gap_extend: f64,
    /// Lowest score worth reporting. NOT a quality bar the user should have to reason about: it
    /// is the noise floor, below which hits are chance similarity in random DNA.
    pub min_score: f64,
    /// Most alignments to return. Ranked by score, so the ones dropped are the worst.
    pub max_results: usize,
}

pub const ALIGN_DEFAULTS: AlignOptions = AlignOptions {
    circular: false,
    match_score: 2.0,
    mismatch_score: -2.0,
    gap_open: 12.0,
    gap_extend: 0.5,
    // ≈10 perfectly matched bases. Below this, a 6 kb plasmid answers with noise.
    min_score: 20.0,
    max_results: 24,
};

impl Default for AlignOptions {
    fn default() -> Self {
        ALIGN_DEFAULTS
    }
}

// Traceback directions, packed one byte per cell. H's origin is the low two bits; E and F each
// carry a single bit saying whether they extended themselves or opened out of H; DIAG_MATCH says
// the diagonal step was a real match, which is what makes a cell a possible alignment END.
const H_ZERO: u8 = 0;
const H_DIAG: u8 = 1;
const H_FROM_E: u8 = 2;
const H_FROM_F: u8 = 3;
const E_EXTEND: u8 = 1 << 2;
const F_EXTEND: u8 = 1 << 3;
const DIAG_MATCH: u8 = 1 << 4;

const NEG: f64 = -1e9;

// Bases as small integers. Anything that is not A/C/G/T is 255 and never pairs, not even with
// itself: an ambiguity code scores as a mismatch rather than a free pass, so a run of Ns never
// fabricates an alignment. Same conservatism as the binding-site search.
const AMBIGUOUS: u8 = 255;

fn encode(s: &str) -> Vec<u8> {
    s.bytes()
        .map(|c| match c {
            b'A' => 0,
            b'C' => 1,
            b'G' => 2,
            b'T' => 3,
            _ => AMBIGUOUS,
        })
        .collect()
}

/// Every place `query` aligns to `target`, both strands, ranked by score (then by start).
///
/// Unlike `find_primer_sites` this reports a PARTIAL fit rather than nothing: if half the query
/// aligns cleanly and half is junk, you get the half that aligns plus a `trailing_overhang`. That
/// is the whole point: you are trying to see how it sits, and "nowhere" is never the useful
/// answer.
pub fn local_align(target: &str, query: &str, opts: AlignOptions) -> Vec<LocalAlignment> {
    let t = clean_seq(target);
    let q = clean_seq(query);
    if t.is_empty() || q.is_empty() {
        return Vec::new();
    }

    // A circular target is searched as itself plus a query-length prefix, so an alignment
    // crossing the origin is found as an ordinary one. The duplicated tail is why overlapping
    // hits are filtered per strand.
    let n0 = t.len();
    let mut hay = encode(&t);
    if opts.circular {
        let head: Vec<u8> = hay.iter().take((q.len() - 1).min(n0)).copied().collect();
        hay.extend(head);
    }
    // One direction table for both strands: the same size each time. A table too large to count
    // or to allocate is refused with no alignments, where the web app's allocation throws.
    let Some(cells) = (hay.len() + 1).checked_mul(q.len() + 1) else {
        return Vec::new();
    };
    let mut dir: Vec<u8> = Vec::new();
    if dir.try_reserve_exact(cells).is_err() {
        return Vec::new();
    }
    dir.resize(cells, 0);

    let mut out: Vec<LocalAlignment> = Vec::new();
    // The plus strand reads the query as given; the minus strand reads its reverse complement.
    // Both are aligned against the SAME target, so every coordinate that comes back is already a
    // plus-strand column and nothing downstream has to flip anything.
    for forward in [true, false] {
        if !forward {
            dir.fill(0);
        }
        let probe = if forward { encode(&q) } else { encode(&revcomp(&q)) };
        for a in align_one_strand(&hay, &probe, n0, &mut dir, &opts) {
            out.push(finish(a, q.len(), forward));
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal).then(a.start.cmp(&b.start)));

    // Each strand deduped itself; this deduplicates ACROSS them. A palindromic query (GAATTC, and
    // every restriction site) is identical to its own reverse complement, so both strands report
    // the SAME columns: one site, found twice, which would draw twice and be counted twice. Only
    // an exact footprint match is dropped: two opposite-strand alignments that merely overlap are
    // a real inverted repeat and both deserve to be seen.
    let mut seen = HashSet::new();
    out.retain(|a| seen.insert((a.start, a.end)));
    // The cap is on the answer, not per strand: a query present forty times is "24+", not 48.
    out.truncate(opts.max_results);
    out
}

struct RawAlignment {
    score: f64,
    target_start: usize,
    target_end: usize,
    probe_start: usize,
    probe_end: usize,
    blocks: Vec<AlignBlock>,
}

/// One column of the rolling score rows. `h_o` and `f_o` carry rule 4 below: the H score at the
/// point the current path's most recent gap opened (0 if it never gapped).
#[derive(Clone, Copy)]
struct Scores {
    h: f64,
    f: f64,
    h_o: f64,
    f_o: f64,
}

/// A cell worth tracing back: its index in the direction table and its score.
struct Candidate {
    cell: usize,
    score: f64,
}

// Which cells are worth tracing back.
// The fill is O(n·m) and cannot be helped. What CAN be helped is what happens after it. The first
// version pushed every cell over the floor as a candidate end and traced each one back (O(m) a
// time) before discovering it overlapped a hit already taken. For a long query that is most of
// the matrix: a 1 kb query against a 6 kb plasmid produced 1.5 million tracebacks of a thousand
// steps each, 19 seconds, for ONE hit.
//
// A cell is a candidate only if all four hold. Each rule removes cells whose traceback would have
// been thrown away by the footprint-overlap check anyway, so the answer does not change:
//   1. its score clears the floor;
//   2. it was reached by a diagonal step that MATCHED: a local alignment never ends on a mismatch
//      or a gap, because dropping that step scores higher;
//   3. it is a PEAK on its diagonal: the next pair is not a match, else that cell scores more on
//      the same path and would take the footprint first;
//   4. it has PAID FOR ITS LAST GAP: its score exceeds the score at the cell where the most
//      recent gap on its path opened. If the flank after a gap does not out-earn the gap, the
//      alignment that stops before the gap scores higher, shares the footprint, and goes first.
//      Without this rule the gap-reachable band around one long hit still yields a million
//      chance peaks, each a gap off the end of the real thing plus a couple of lucky bases.
// Rule 4 is not quite exact: when the pre-gap alignment is itself dropped for overlapping a
// better hit that only touches its far end, the shorter prefix before the gap is lost too. It
// takes two overlapping fits of different parts of the query to the same columns to see it.
//
// Then the survivors are visited best first, a candidate whose row is already claimed is skipped
// without a trace, and a trace that walks into a claimed row stops there. Tracebacks are now
// O(m) per ACCEPTED hit, plus a few cheap aborts.
fn align_one_strand(hay: &[u8], probe: &[u8], n0: usize, dir: &mut [u8], o: &AlignOptions) -> Vec<RawAlignment> {
    let w = probe.len() + 1;
    let mut results: Vec<RawAlignment> = Vec::new();
    // Rows are counted modulo the real length below; a target of no bases has no rows to count.
    if n0 == 0 {
        return results;
    }

    // Highest-scoring candidates first; on a tie the lower cell, so the original region beats its
    // doubled-prefix twin and the answer is deterministic.
    let mut cands = fill(hay, probe, dir, o);
    cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal).then(a.cell.cmp(&b.cell)));

    let mut taken_row = vec![false; n0];
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for cand in &cands {
        if results.len() >= o.max_results {
            break;
        }
        let i = cand.cell / w;
        if row_taken(&taken_row, i) {
            continue;
        }
        let Some(a) = traceback(hay, probe, dir, (i, cand.cell - i * w, cand.score), &taken_row) else {
            continue;
        };
        let s = a.target_start % n0;
        let e2 = a.target_end % n0;
        if taken.iter().any(|&(x, y)| overlaps(s, e2, x, y)) {
            continue;
        }
        taken.push((s, e2));
        for r in a.target_start..=a.target_end {
            if let Some(claimed) = taken_row.get_mut(r % n0) {
                *claimed = true;
            }
        }
        results.push(a);
    }
    results
}

/// The O(n·m) fill: writes every cell's traceback direction into `dir` and returns the cells that
/// pass the four rules above, in the order they were met.
fn fill(hay: &[u8], probe: &[u8], dir: &mut [u8], o: &AlignOptions) -> Vec<Candidate> {
    let m = probe.len();
    let w = m + 1;

    // Score rows roll; only directions are kept in full. This is what keeps a 6 kb × 40 nt search
    // at a quarter of a megabyte instead of three megabytes of floats.
    let edge = Scores { h: 0.0, f: NEG, h_o: 0.0, f_o: 0.0 };
    let mut prev = vec![edge; w];
    let mut cur = vec![edge; w];

    let mut cands: Vec<Candidate> = Vec::new();
    let mut consider = |cell: usize, h: f64, h_o: f64, flags: u8| {
        if h < o.min_score || h <= h_o || flags & DIAG_MATCH == 0 {
            return;
        }
        cands.push(Candidate { cell, score: h });
    };

    let mut rows = dir.chunks_exact_mut(w);
    let Some(mut prev_dir) = rows.next() else {
        return Vec::new();
    };
    let mut last_row = 0;
    for (i, (cur_dir, &hc)) in rows.zip(hay).enumerate() {
        let row = (i + 1) * w;
        last_row = row;
        let Some((first, rest)) = cur.split_first_mut() else {
            break;
        };
        *first = edge;
        let mut e = NEG;
        let mut e_o = 0.0;
        // The cell to the left, the one just written: its score opens an insertion, and at the end
        // of the row it is the last column.
        let mut left = edge;
        let mut left_flags = 0u8;
        let columns = probe.iter().zip(prev.windows(2)).zip(rest.iter_mut()).zip(cur_dir.iter_mut().skip(1)).zip(prev_dir.iter());
        for (j, ((((&pc, above), here), here_dir), &diag_dir)) in columns.enumerate() {
            let [diag, up] = above else {
                continue;
            };
            let same = hc == pc && hc != AMBIGUOUS;
            // Rule 3, decided one step late: the diagonal predecessor is a peak iff THIS pair is
            // not a match. Its score and gap origin are still in the previous row.
            if !same {
                consider(row - w + j, diag.h, diag.h_o, diag_dir);
            }
            let sub = if same { o.match_score } else { o.mismatch_score };

            // Gap in the target (the query has bases the template does not): an INSERTION.
            let e_open = left.h - o.gap_open;
            let e_ext = e - o.gap_extend;
            let mut flags = 0u8;
            if e_ext > e_open {
                e = e_ext;
                flags |= E_EXTEND;
            } else {
                e = e_open;
                e_o = left.h;
            }

            // Gap in the query (the template has bases the query does not): a DELETION.
            let f_open = up.h - o.gap_open;
            let f_ext = up.f - o.gap_extend;
            let (f, f_o) = if f_ext > f_open {
                flags |= F_EXTEND;
                (f_ext, up.f_o)
            } else {
                (f_open, up.h)
            };

            let diag_score = diag.h + sub;
            let mut best = 0.0;
            let mut h_from = H_ZERO;
            let mut h_o = 0.0;
            if diag_score > best {
                best = diag_score;
                h_from = H_DIAG;
                h_o = diag.h_o;
                if same {
                    flags |= DIAG_MATCH;
                }
            }
            if e > best {
                best = e;
                h_from = H_FROM_E;
                h_o = e_o;
                flags &= !DIAG_MATCH;
            }
            if f > best {
                best = f;
                h_from = H_FROM_F;
                h_o = f_o;
                flags &= !DIAG_MATCH;
            }
            left = Scores { h: best, f, h_o, f_o };
            left_flags = flags | h_from;
            *here = left;
            *here_dir = left_flags;
        }
        // The last column has no diagonal successor, so it is its own peak.
        consider(row + m, left.h, left.h_o, left_flags);
        std::mem::swap(&mut prev, &mut cur);
        prev_dir = cur_dir;
    }
    // And so is the last row (its last cell was already considered above).
    for (j, (scores, &flags)) in prev.iter().zip(prev_dir.iter()).enumerate().take(m).skip(1) {
        consider(last_row + j, scores.h, scores.h_o, flags);
    }
    cands
}

/// Whether an accepted hit already claims the template base that row `i` of the table faces.
fn row_taken(taken_row: &[bool], i: usize) -> bool {
    let claimed = || taken_row.get(i.checked_sub(1)?.checked_rem(taken_row.len())?).copied();
    claimed().unwrap_or(false)
}

/// Do two footprints share a column? Both may wrap, so this compares as sets on a ring.
fn overlaps(s1: usize, e1: usize, s2: usize, e2: usize) -> bool {
    let has = |s: usize, e: usize, x: usize| if s <= e { x >= s && x <= e } else { x >= s || x <= e };
    // Sampling the endpoints is enough: alignments here are contiguous ranges, so if they
    // intersect at all, at least one range's start or end lies inside the other.
    has(s1, e1, s2) || has(s1, e1, e2) || has(s2, e2, s1) || has(s2, e2, e1)
}

enum State {
    H,
    E,
    F,
}

/// Growing leftward, so extend the block last pushed rather than emitting one per base. A step
/// that has a query base names it, and one that has a template column names that.
fn push_block(blocks: &mut Vec<AlignBlock>, op: AlignOp, q_start: Option<usize>, t_start: Option<usize>) {
    let q_len = usize::from(q_start.is_some());
    let t_len = usize::from(t_start.is_some());
    if let Some(prev) = blocks.last_mut()
        && prev.op == op
    {
        prev.query_start = q_start.or(prev.query_start);
        prev.query_length += q_len;
        prev.target_start = t_start.or(prev.target_start);
        prev.target_length += t_len;
        return;
    }
    blocks.push(AlignBlock { op, query_start: q_start, query_length: q_len, target_start: t_start, target_length: t_len });
}

/// Walk one candidate, ending at row `i` and column `j` of the table, back to where its alignment
/// started. Returns `None` if the walk enters a row an accepted hit already claims: the result
/// would be thrown away for overlapping it, so there is no point finishing the walk.
fn traceback(hay: &[u8], probe: &[u8], dir: &[u8], end: (usize, usize, f64), taken_row: &[bool]) -> Option<RawAlignment> {
    let w = probe.len() + 1;
    let (end_i, end_j, score) = end;
    // Collected from the alignment's last block to its first, and turned round at the end.
    let mut blocks: Vec<AlignBlock> = Vec::new();
    let mut i = end_i;
    let mut j = end_j;
    let mut state = State::H;

    while i > 0 && j > 0 {
        let cell = *dir.get(i * w + j)?;
        match state {
            State::H => {
                let from = cell & 3;
                if from == H_ZERO {
                    break;
                }
                if from == H_DIAG {
                    if row_taken(taken_row, i) {
                        return None;
                    }
                    let hc = *hay.get(i - 1)?;
                    let same = probe.get(j - 1) == Some(&hc) && hc != AMBIGUOUS;
                    push_block(&mut blocks, if same { AlignOp::Match } else { AlignOp::Mismatch }, Some(j - 1), Some(i - 1));
                    i -= 1;
                    j -= 1;
                    continue;
                }
                state = if from == H_FROM_E { State::E } else { State::F };
            }
            State::E => {
                push_block(&mut blocks, AlignOp::Insertion, Some(j - 1), None);
                j -= 1;
                if cell & E_EXTEND == 0 {
                    state = State::H;
                }
            }
            State::F => {
                if row_taken(taken_row, i) {
                    return None;
                }
                push_block(&mut blocks, AlignOp::Deletion, None, Some(i - 1));
                i -= 1;
                if cell & F_EXTEND == 0 {
                    state = State::H;
                }
            }
        }
    }

    if blocks.is_empty() {
        return None;
    }
    blocks.reverse();
    Some(RawAlignment { score, target_start: i, target_end: end_i.checked_sub(1)?, probe_start: j, probe_end: end_j.checked_sub(1)?, blocks })
}

/// Turn a strand-local result back into the caller's frame.
///
/// On the minus strand everything was computed against `revcomp(query)`, so the probe indices run
/// backwards relative to the oligo the user typed. Remapping them here, once, in one place, is
/// what lets every consumer treat `query_start` as an index into their own oligo on both strands.
/// `m` is the query's length.
fn finish(a: RawAlignment, m: usize, forward: bool) -> LocalAlignment {
    let mut blocks = a.blocks;
    if !forward {
        for b in &mut blocks {
            // A block covering probe [s, s+len) covers query [m-s-len, m-s).
            b.query_start = b.query_start.filter(|_| b.query_length > 0).map(|s| m.saturating_sub(s + b.query_length));
        }
        blocks.reverse();
    }

    let mut mismatch_positions: Vec<usize> = Vec::new();
    let mut matched = 0usize;
    let mut columns = 0usize;
    for b in &blocks {
        if b.op == AlignOp::Match {
            matched += b.query_length;
        }
        if b.op == AlignOp::Mismatch
            && let Some(s) = b.query_start
        {
            mismatch_positions.extend(s..s + b.query_length);
        }
        columns += b.query_length.max(b.target_length);
    }
    mismatch_positions.sort_unstable();

    let before = a.probe_start;
    let after = m.saturating_sub(a.probe_end + 1);
    let (leading, trailing) = if forward { (before, after) } else { (after, before) };

    LocalAlignment {
        forward,
        start: a.target_start,
        end: a.target_end,
        score: a.score,
        identity: if columns > 0 { matched as f64 / columns as f64 } else { 0.0 },
        blocks,
        leading_overhang: leading,
        trailing_overhang: trailing,
        mismatch_positions,
    }
}
