//! Finding where a sequence occurs or binds. Ported from the Bios web app.
//!
//! A binding site is derived, never authored. Given a target and an oligo, this answers "where
//! does it land, on which strand, and how badly", so nothing upstream ever has to type a
//! coordinate. Everything a binder shows (its position, its mismatches, the off-target warning)
//! comes from here.
//!
//! Strand semantics, the trap in this file, stated once:
//!   An oligo whose own sequence appears in the plus strand is identical to the plus strand, so
//!   it anneals to the minus strand and points rightward: `forward: true`.
//!   An oligo whose reverse complement appears in the plus strand is complementary to the plus
//!   strand, so it anneals to the plus strand: `forward: false`.
//!   An ASO against a sense transcript is the second case. Getting this backwards is a silent
//!   wrong answer: it draws on the wrong strand and never fails.
//!
//! Indels are deliberately not supported: a bulged duplex needs alignment, not a Hamming window,
//! and every consumer here (primer binding, ASO placement) is gapless by design.

use crate::seq::{clean_seq, revcomp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingSite {
    /// 0-based inclusive start on the target's plus strand, whichever strand the oligo binds.
    pub start: usize,
    /// 0-based inclusive end on the plus strand. For a site crossing the origin of a circular
    /// target, `end < start`: the origin-spanning convention features use, not a collapsed range.
    pub end: usize,
    /// See the strand note above. An ASO against a sense transcript is `false`.
    pub forward: bool,
    /// Mismatched base count. 0 is a perfect duplex.
    pub mismatches: usize,
    /// 0-based offsets within the oligo, read 5'→3': indices into the cleaned sequence the caller
    /// passed, whichever strand it turned out to bind. Always ascending. Reporting these against
    /// the matched strand instead would make a reverse hit's numbers point at the wrong end of
    /// the user's own oligo.
    pub mismatch_positions: Vec<usize>,
    /// True if the site crosses the origin. Only possible when the target is circular.
    pub wraps: bool,
}

/// The web app has no default for `circular`, because there is no safe one: a circular target
/// searched as linear silently loses every site crossing the origin. `Default` is here only to
/// fill in `max_mismatches`; always name `circular` yourself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// Maximum mismatched bases to tolerate. Default 0 (perfect duplex only).
    pub max_mismatches: usize,
    /// Whether the target is circular. The derived default is `false`, a linear search.
    pub circular: bool,
}

/// One window that passed the scan: where it starts in the haystack, and the offsets within the
/// needle that did not pair.
struct Hit {
    index: usize,
    mism: Vec<usize>,
}

/// Hamming scan with early exit. Returns window starts and their mismatch offsets.
fn scan(hay: &[u8], needle: &[u8], max_mismatches: usize) -> Vec<Hit> {
    let mut hits = Vec::new();
    // `windows` panics on a width of zero. The caller never passes one; this keeps that true here.
    if needle.is_empty() {
        return hits;
    }
    // One buffer for every window: most windows fail, and a hostile `max_mismatches` must not
    // size an allocation.
    let mut mism = Vec::new();
    for (index, window) in hay.windows(needle.len()).enumerate() {
        mism.clear();
        let mut bad = 0usize;
        for (k, (&h, &o)) in window.iter().zip(needle).enumerate() {
            // Only a real base pairs. An ambiguity code (N across a genomic gap, R/Y in a
            // degenerate oligo) is not expanded, so it counts as a mismatch on both sides. That
            // means N against N is a mismatch too, since an oligo must never be reported as
            // binding a run of Ns. Conservative on purpose: this errs toward "no site", and a
            // missed wobble is a question, while a phantom site is a wrong answer.
            if h != o || !matches!(h, b'A' | b'C' | b'G' | b'T') {
                bad += 1;
                if bad > max_mismatches {
                    // Early exit: this is what bounds the work on a long target.
                    break;
                }
                mism.push(k);
            }
        }
        if bad <= max_mismatches {
            hits.push(Hit { index, mism: mism.clone() });
        }
    }
    hits
}

/// Every place `oligo` binds `target`, on both strands.
///
/// Both arguments are 5'→3'. Results are 0-based inclusive on the target's plus strand, ascending
/// by start. A palindromic oligo legitimately reports both strands at one site, forward first.
pub fn find_binding_sites(target: &str, oligo: &str, options: SearchOptions) -> Vec<BindingSite> {
    let t = clean_seq(target);
    let o = clean_seq(oligo);
    let n = t.len();
    let m = o.len();
    if m == 0 || m > n {
        return Vec::new();
    }

    // Circular: extend by m-1 so a window can run off the end and wrap. That is exactly enough
    // for the last window to start at n-1, so no site is found a second time in the extension.
    let mut hay = t.into_bytes();
    if options.circular {
        let head: Vec<u8> = hay.iter().take(m - 1).copied().collect();
        hay.extend(head);
    }

    let mut sites = Vec::new();
    let mut collect = |needle: &str, forward: bool| {
        for Hit { index, mism } in scan(&hay, needle.as_bytes(), options.max_mismatches) {
            let end_abs = index + m - 1;
            let wraps = end_abs >= n;
            sites.push(BindingSite {
                start: index,
                end: if wraps { end_abs - n } else { end_abs },
                forward,
                mismatches: mism.len(),
                // A reverse hit matched revcomp(oligo): window offset k is oligo offset m-1-k.
                mismatch_positions: if forward { mism } else { mism.iter().rev().map(|k| m - 1 - k).collect() },
                wraps,
            });
        }
    };

    collect(&o, true);
    collect(&revcomp(&o), false);

    sites.sort_by(|a, b| a.start.cmp(&b.start).then(b.forward.cmp(&a.forward)));
    sites
}
