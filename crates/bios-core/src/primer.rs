//! Candidate primer sites on both strands, anchored at the 3′ end. Ported from the Bios web app.
//!
//! This is an ungapped sequence search, not evidence of experimental amplification. The
//! highest-scoring extension proposes a local 5′ boundary; it never changes a saved oligo or its
//! deliberate tail. SnapGene also exposes anchor/Tm controls, but its mismatch/loop
//! thermodynamics are not implemented here. Mismatched candidates retain an unknown Tm and do not
//! claim to pass the temperature threshold.
//!
//! Coordinates are 0-based inclusive on the target's plus strand, whichever strand the primer
//! binds, and they are those of the ANNEALING region, not of the primer: the 5' tail is not on the
//! template. A site through the origin of a circular target has `end < start`.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::seq::{DNA_TM_DEFAULTS, clean_seq, revcomp, tm};

/// Reproducible ungapped primer-site search criteria, separate from saved oligos and tails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimerSearchSettings {
    pub min_anchor3: usize,
    pub min_tm: f64,
    pub max_mismatches: usize,
    pub allow_isolated_mismatch: bool,
}

pub const DEFAULT_PRIMER_SEARCH_SETTINGS: PrimerSearchSettings =
    PrimerSearchSettings { min_anchor3: 10, min_tm: 40.0, max_mismatches: 3, allow_isolated_mismatch: true };

/// Bios search defaults; shared by UI and agent inspection.
pub const PRIMER_DEFAULTS: PrimerSearchSettings = DEFAULT_PRIMER_SEARCH_SETTINGS;

/// The web app's site limit when none is given to [`search_primer_sites`].
pub const DEFAULT_SITE_LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub struct PrimerSite {
    /// 0-based inclusive start of the ANNEALING region on the target's plus strand, not of the
    /// primer. The 5' tail has no coordinates because it is not on the template.
    pub start: usize,
    /// 0-based inclusive end of the annealing region. `end < start` = origin-spanning.
    pub end: usize,
    /// See [`crate::search`]. A primer whose own sequence reads along the plus strand is `true`.
    pub forward: bool,
    /// Length of the unbound 5' overhang, in bases. 0 for a fully-annealing primer. The tail is
    /// the first `tail_length` bases of the primer: always 5'-side, always contiguous, by
    /// construction.
    pub tail_length: usize,
    /// Length of the annealing region: the primer's length less `tail_length`.
    pub anneal_length: usize,
    /// Non-matching or ambiguous bases WITHIN the candidate anneal. Excludes its proposed tail.
    pub mismatches: usize,
    /// 0-based offsets into the user's primer, 5'→3', ascending. Only ever inside the annealing
    /// region, so every entry is >= `tail_length`.
    pub mismatch_positions: Vec<usize>,
    /// Perfect-duplex estimate for an exact, concrete anneal; `None` for mismatched or ambiguous
    /// sites.
    pub tm: Option<f64>,
    /// True if the annealing footprint crosses the origin. Only possible when circular.
    pub wraps: bool,
}

/// `Default` is [`PRIMER_DEFAULTS`] on a linear target. The web app has no default for
/// `circular`, because there is no safe one: a circular target searched as linear silently loses
/// every site crossing the origin. Always name it yourself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimerSearchOptions {
    pub circular: bool,
    /// Minimum 3′ anchor length. Its terminal base must match under this search model.
    pub min_anchor3: usize,
    /// SnapGene's "allowing a single isolated mismatch": permits ONE mismatch inside the anchor
    /// region, provided it is isolated (both neighbours pair). The 3'-terminal base is still
    /// required to pair: SnapGene carries a separate, off-by-default option for sites that "do
    /// not match at the 3' end", so the main rule requiring it is the reading this follows. (That
    /// the terminal base is excluded is our inference from the existence of that second option,
    /// not quoted doc text.)
    pub allow_isolated_mismatch: bool,
    /// Minimum estimated Tm for exact duplexes. Mismatched candidates have an unverified Tm.
    pub min_tm: f64,
    /// Most mismatches tolerated inside the annealing region. NOT a SnapGene setting: it stands
    /// in for the mismatch thermodynamics we don't model. See the header note.
    pub max_mismatches: usize,
}

impl Default for PrimerSearchOptions {
    fn default() -> Self {
        PrimerSearchOptions {
            circular: false,
            min_anchor3: PRIMER_DEFAULTS.min_anchor3,
            allow_isolated_mismatch: PRIMER_DEFAULTS.allow_isolated_mismatch,
            min_tm: PRIMER_DEFAULTS.min_tm,
            max_mismatches: PRIMER_DEFAULTS.max_mismatches,
        }
    }
}

/// What the web app throws. The settings that a Rust type cannot hold wrongly (a fractional
/// anchor, a negative mismatch limit, an option that is not a boolean) have no error here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimerSearchError {
    /// A 3′ anchor of zero, an anchor or mismatch limit past the web app's largest whole number,
    /// or a Tm floor that is not finite.
    Settings,
    /// A site limit outside 1 to 1000.
    SiteLimit,
}

impl std::fmt::Display for PrimerSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrimerSearchError::Settings => write!(
                f,
                "Primer search needs a positive whole 3′ anchor, a nonnegative whole mismatch limit, a finite Tm and boolean topology/mismatch options."
            ),
            PrimerSearchError::SiteLimit => write!(f, "Site limit must be a whole number from 1 to 1000."),
        }
    }
}

impl std::error::Error for PrimerSearchError {}

const MATCH_SCORE: i64 = 1;
// Costs more than a match pays, so extending into a random tail (~25% identity) loses ground and
// the anneal boundary lands where the homology actually stops. A single internal mismatch flanked
// by real homology still pays for itself within two bases, which is what lets a mutagenesis
// primer keep its full-length anneal.
const MISMATCH_SCORE: i64 = -2;

/// The largest whole number the web app counts exactly, and so the largest it accepts.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

fn is_safe_integer(n: usize) -> bool {
    u64::try_from(n).is_ok_and(|n| n <= MAX_SAFE_INTEGER)
}

fn unambiguous(base: u8) -> bool {
    matches!(base, b'A' | b'C' | b'G' | b'T')
}

struct Anneal {
    anneal_length: usize,
    /// Walk steps that did not pair, ascending.
    mismatches: Vec<usize>,
}

/// One extension from a pinned 3' anchor, walking toward the primer's 5' end.
///
/// `view[j]` is the primer base expected at walk-step j, with j=0 AT THE 3' END; `pos_at(j)` is
/// the plus-strand position it faces. Both strands collapse into this shape, which is why there
/// is one extender and not two subtly-different ones. `paired` is the caller's buffer, reused for
/// every anchor.
///
/// Returns the highest-scoring prefix, the anneal, or `None` if it never clears the bar.
fn extend_anneal(
    target: &[u8],
    view: &[u8],
    pos_at: impl Fn(usize) -> Option<usize>,
    opts: &PrimerSearchOptions,
    paired: &mut Vec<bool>,
) -> Option<Anneal> {
    let PrimerSearchOptions { min_anchor3, allow_isolated_mismatch, max_mismatches, .. } = *opts;

    // The 3'-OH first, before anything else. Three quarters of anchor positions die on this one
    // base, and this is the innermost loop of the whole search: on a plasmid it's ~12k calls per
    // primer per strand.
    let terminal = *view.first()?;
    if target.get(pos_at(0)?) != Some(&terminal) || !unambiguous(terminal) {
        return None;
    }

    // Pair the walk first, then judge it. Doing both at once tangles the anchor rule (which needs
    // to look at a mismatch's NEIGHBOURS, i.e. one base into the future) with the scoring, and
    // the isolation test is the whole reason this is two passes.
    paired.clear();
    for (j, &expected) in view.iter().enumerate() {
        // Ran off a linear end: extend no further.
        let Some(&t) = pos_at(j).and_then(|p| target.get(p)) else {
            break;
        };
        // Same conservatism as the binding-site search: only a real base pairs. An ambiguity code
        // on either side is a mismatch, never a free pass, so N-vs-N never fabricates a duplex.
        paired.push(t == expected && unambiguous(t));
    }
    // The anchor must physically fit: a primer running off the end of a linear template has no
    // anchor to test.
    if paired.len() < min_anchor3 {
        return None;
    }

    // Test 1: the 3' anchor. This search requires an exact 3′ terminal pair.
    if paired.first() != Some(&true) {
        return None;
    }
    let mut anchor_mism = paired.iter().take(min_anchor3).enumerate().filter(|(_, ok)| !**ok).map(|(j, _)| j);
    let lone = anchor_mism.next();
    if anchor_mism.next().is_some() {
        return None;
    }
    if let Some(j) = lone {
        if !allow_isolated_mismatch {
            return None;
        }
        // "Isolated" = both neighbours pair. j-1 exists (j>0, since the terminal base paired).
        // j+1 may sit just outside the anchor or past the walk; an unpaired or absent neighbour
        // means this mismatch is the leading edge of a tail, not an isolated blip.
        let pairs = |k: Option<usize>| k.and_then(|k| paired.get(k)).copied().unwrap_or(false);
        if !pairs(j.checked_sub(1)) || !pairs(j.checked_add(1)) {
            return None;
        }
    }

    // Find the anneal/tail boundary: the highest-scoring extension.
    let mut score = 0i64;
    let (mut best_len, mut best_score, mut best_mism) = (0usize, 0i64, 0usize);
    let mut mism: Vec<usize> = Vec::new();
    for (j, &ok) in paired.iter().enumerate() {
        if !ok {
            // Budget spent; the rest is tail.
            if mism.len() >= max_mismatches {
                break;
            }
            mism.push(j);
            score += MISMATCH_SCORE;
            continue;
        }
        score += MATCH_SCORE;
        // Only a PAIRED base can close the anneal: ending on a mismatch would draw a site whose
        // 5'-terminal base doesn't pair. That base is tail, and calling it anneal would both
        // overstate the footprint and poison the Tm. And the anneal can never be shorter than the
        // anchor we just validated.
        if j + 1 >= min_anchor3 && score > best_score {
            best_len = j + 1;
            best_score = score;
            best_mism = mism.len();
        }
    }
    if best_len < min_anchor3 {
        return None;
    }
    // The mismatches met by the time the best extension closed.
    mism.truncate(best_mism);
    Some(Anneal { anneal_length: best_len, mismatches: mism })
}

/// Enumerate ungapped candidates on both strands, with proposed 5' overhangs: every forward
/// candidate by ascending anchor, then every reverse one.
///
/// Both arguments are 5'→3'. Unlike `find_binding_sites` this will report a site for a primer
/// that is mostly tail, which is the point.
fn enumerate_primer_sites(
    target: &str,
    primer: &str,
    options: &PrimerSearchOptions,
    emit: &mut impl FnMut(PrimerSite),
) -> Result<(), PrimerSearchError> {
    let PrimerSearchOptions { circular, min_anchor3, min_tm, max_mismatches, .. } = *options;
    if min_anchor3 < 1 || !is_safe_integer(min_anchor3) || !is_safe_integer(max_mismatches) || !min_tm.is_finite() {
        return Err(PrimerSearchError::Settings);
    }
    let t = clean_seq(target);
    let p = clean_seq(primer);
    let n = t.len();
    let m = p.len();
    if n == 0 || m < min_anchor3 || n < min_anchor3 {
        return Ok(());
    }

    // Walk views, j=0 at the primer's 3' end in BOTH cases: the unification the extender depends
    // on. Forward: the primer reads along the plus strand, so stepping toward its 5' end steps
    // LEFT. Reverse: revcomp(primer) reads along the plus strand and its index 0 IS the primer's
    // 3' base, so stepping toward the 5' end steps RIGHT.
    let fwd_view: Vec<u8> = p.bytes().rev().collect();
    let rev_view = revcomp(&p).into_bytes();

    let mut temperatures: HashMap<usize, Option<f64>> = HashMap::new();
    let mut paired: Vec<bool> = Vec::new();
    for (view, forward) in [(&fwd_view, true), (&rev_view, false)] {
        for anchor in 0..n {
            // Modular only when circular; `None` off the ends when linear. This is where a linear
            // target refuses to wrap and a circular one refuses to lose an origin-spanning site.
            // A walk never takes a second lap: step n would face the anchor's own base again.
            let pos_at = |j: usize| -> Option<usize> {
                if j >= n {
                    None
                } else if forward {
                    if circular { Some((anchor + n - j) % n) } else { anchor.checked_sub(j) }
                } else if circular {
                    Some((anchor + j) % n)
                } else {
                    Some(anchor + j).filter(|&x| x < n)
                }
            };
            let Some(hit) = extend_anneal(t.as_bytes(), view, pos_at, options, &mut paired) else {
                continue;
            };

            let l = hit.anneal_length;
            let mism = hit.mismatches;
            // A perfect-match calculation cannot verify a mismatched or ambiguous duplex. Retain
            // those candidates for inspection, explicitly without passing the Tm floor.
            let anneal_tm = *temperatures.entry(l).or_insert_with(|| {
                p.get(m.saturating_sub(l)..).and_then(|anneal| tm(anneal, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm))
            });
            let site_tm = if mism.is_empty() { anneal_tm } else { None };
            if mism.is_empty() && site_tm.is_none_or(|site_tm| site_tm < min_tm) {
                continue;
            }

            // The anchor is the primer's 3' base. Forward, the anneal runs leftward from it, so
            // the anchor is the site's END; reverse, it runs rightward, so the anchor is its
            // START. Getting this backwards draws every reverse site L bases off. The walk fits
            // in one lap (l <= n), so the far end is at most one length off the template.
            let (start, end, crosses) = if forward {
                ((anchor + 1 + n).saturating_sub(l) % n, anchor, l > anchor + 1)
            } else {
                let hi = anchor + l - 1;
                (anchor, hi % n, hi >= n)
            };

            emit(PrimerSite {
                start,
                end,
                forward,
                tail_length: m.saturating_sub(l),
                anneal_length: l,
                mismatches: mism.len(),
                // Walk-step j is primer index m-1-j on BOTH strands (the reverse view's base j is
                // the complement of primer base m-1-j), so one remap serves both. Ascending, per
                // the binding-site search's contract.
                mismatch_positions: mism.iter().rev().map(|&j| m.saturating_sub(j + 1)).collect(),
                tm: site_tm,
                wraps: circular && crosses,
            });
        }
    }
    Ok(())
}

/// Genomic order: by start, the forward strand first where two share one, then by end.
fn site_order(a: &PrimerSite, b: &PrimerSite) -> Ordering {
    a.start.cmp(&b.start).then(b.forward.cmp(&a.forward)).then(a.end.cmp(&b.end))
}

/// All ungapped candidates, in genomic order; use [`search_primer_sites`] for a bounded
/// inspection inventory.
pub fn find_primer_sites(target: &str, primer: &str, options: PrimerSearchOptions) -> Result<Vec<PrimerSite>, PrimerSearchError> {
    let mut sites: Vec<PrimerSite> = Vec::new();
    enumerate_primer_sites(target, primer, &options, &mut |site| sites.push(site))?;
    sites.sort_by(site_order);
    Ok(sites)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimerSiteInventory {
    pub sites: Vec<PrimerSite>,
    pub total: usize,
    pub truncated: bool,
}

/// Count every candidate while retaining only the first `limit` (1 to 1000, [`DEFAULT_SITE_LIMIT`]
/// in the web app) in genomic order.
pub fn search_primer_sites(target: &str, primer: &str, options: PrimerSearchOptions, limit: usize) -> Result<PrimerSiteInventory, PrimerSearchError> {
    if !(1..=1000).contains(&limit) {
        return Err(PrimerSearchError::SiteLimit);
    }
    let mut sites: Vec<PrimerSite> = Vec::new();
    let mut total = 0usize;
    enumerate_primer_sites(target, primer, &options, &mut |site| {
        total += 1;
        if sites.len() == limit && sites.last().is_some_and(|last| site_order(&site, last) != Ordering::Less) {
            return;
        }
        // After every kept site that does not come later, so equal sites stay in the order found.
        let at = sites.partition_point(|kept| site_order(kept, &site) != Ordering::Greater);
        sites.insert(at, site);
        if sites.len() > limit {
            sites.pop();
        }
    })?;
    let truncated = total > sites.len();
    Ok(PrimerSiteInventory { sites, total, truncated })
}
