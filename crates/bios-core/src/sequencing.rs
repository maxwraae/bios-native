//! Sanger sequencing primers tiled across a construct, so that the reads overlap and cover all of
//! it. Bios's own, built on the same criteria and the same nearest-neighbour melting temperature
//! as [`crate::primer_design`]; nothing here is ported from another designer.
//!
//! What a read is taken to be: a primer resolves [`SequencingSettings::read_length`] bases
//! starting at the base AFTER its 3' end, reading along its own strand. A real trace is poor for
//! its first few dozen bases and dies away at the far end, so `read_length` is the length a
//! sequencing provider quotes and [`SequencingSettings::overlap`] is what keeps the joins off the
//! bad ends. Bios models neither: it tiles the lengths it is given.
//!
//! How the tiling runs. The first primer's 3' end sits immediately before the stretch asked for,
//! so its read begins at the first base wanted. Each primer after it is aimed at
//! `read_length - overlap` bases further on, and the aim may move BACKWARDS as far as
//! [`SequencingSettings::search_window`], which is what lets a repetitive or very AT-rich patch be
//! stepped over instead of stopping the plan. Backwards only, so `overlap` is a floor and not a
//! wish: two reads always share at least that many bases.
//!
//! The one exception is a last resort. Where nothing at or behind an anchor is acceptable, Bios
//! looks forward as well rather than give up on those bases — which is how the first bases of a
//! LINEAR construct are reached at all, since no primer can sit before them. A read taken that way
//! can share fewer bases with the read before it, or leave a gap before it, and its own
//! `overlap_with_previous` and the plan's gaps say so.
//!
//! Every primer returned anneals fully and perfectly in exactly ONE place on the construct. One
//! that binds twice gives a trace of two sequences on top of each other and is unreadable, so a
//! candidate that does is refused, named in [`SequencingPlan::rejected`], and the next candidate
//! tried. Where no candidate in a window is acceptable, the stretch it would have read is reported
//! as a gap with the reason, rather than a primer no one should order.
//!
//! Coordinates are 0-based inclusive on the plus strand, as everywhere in `bios-core`; a stretch
//! through the origin of a circular construct has `end < start`.

use crate::primer::{PrimerSearchOptions, find_primer_sites};
use crate::primer_design::{DesignError, DesignedPrimer, PrimerCriteria, anneal_tm, cost, designed};
use crate::seq::{clean_seq, revcomp};

/// How the reads are laid out. [`DEFAULT_SEQUENCING_SETTINGS`] is the usual starting point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequencingSettings {
    /// Bases one read resolves, counting from the base after the primer's 3' end.
    pub read_length: usize,
    /// Bases each read shares with the one before it on the same strand.
    pub overlap: usize,
    /// Tile the bottom strand as well, so that every base is read twice, once each way.
    pub both_strands: bool,
    /// How far back from its ideal anchor a primer may be taken when nothing at the anchor is
    /// acceptable. Backwards only shortens the step, so it can never open a gap.
    pub search_window: usize,
}

pub const DEFAULT_SEQUENCING_SETTINGS: SequencingSettings =
    SequencingSettings { read_length: 800, overlap: 100, both_strands: false, search_window: 150 };

/// Most candidates whose binding is checked at one anchor. Each check searches the whole
/// construct, so the list is walked best-first and stops here; a window where twenty good
/// candidates all bind twice is a repeat, and the gap says so.
const MAX_BINDING_CHECKS: usize = 20;

/// Most refusals written down per strand. They are there to show the work, not to be exhaustive.
const MAX_REJECTED_REPORTED: usize = 20;

/// A stop on the runaway: more reads than this over one stretch means the settings make no
/// progress, not that the construct needs them.
const MAX_READS_PER_STRAND: usize = 500;

/// What one base of movement away from the ideal anchor costs, in the °C-sized units
/// [`cost`] works in. A forward step gives up overlap, so it costs twice what a backward one does.
const ANCHOR_COST: f64 = 0.05;

/// One sequencing primer and the stretch it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct SequencingRead {
    pub primer: DesignedPrimer,
    /// First base the read resolves, 0-based inclusive on the plus strand.
    pub read_start: usize,
    /// Last base it resolves. `read_end < read_start` = through the origin.
    pub read_end: usize,
    /// Bases this read shares with the read before it on the same strand; 0 for the first.
    pub overlap_with_previous: usize,
    /// Other places the oligo anneals at all, mismatches and partial landings included. It binds
    /// fully and perfectly in exactly one place or it is not in this plan, but a weaker second
    /// site is still worth knowing about.
    pub other_sites: usize,
    /// The oligo's own liabilities: the quality flags and anything its hairpin check found.
    pub warnings: Vec<String>,
}

/// A stretch of the construct no acceptable primer could be found to read, and why not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencingGap {
    /// 0-based inclusive, on the plus strand. `end < start` = through the origin.
    pub start: usize,
    pub end: usize,
    pub why: String,
}

/// A candidate refused, so that the refusal is visible rather than silent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedPrimer {
    pub sequence: String,
    /// Where it would have sat, 0-based inclusive on the plus strand.
    pub start: usize,
    pub end: usize,
    pub forward: bool,
    pub why: String,
}

/// The reads for one strand.
#[derive(Clone, Debug, PartialEq)]
pub struct StrandPlan {
    /// The primers read along the plus strand.
    pub forward: bool,
    /// In the order they read.
    pub reads: Vec<SequencingRead>,
    pub gaps: Vec<SequencingGap>,
    /// Bases of the stretch asked for that these reads cover.
    pub covered: usize,
}

/// Every read of a plan: one strand, or both.
#[derive(Clone, Debug, PartialEq)]
pub struct SequencingPlan {
    /// The stretch asked for, 0-based inclusive. `end < start` = through the origin.
    pub start: usize,
    pub end: usize,
    /// Its length in bases.
    pub requested: usize,
    /// The plus strand first, then the minus strand when both were asked for.
    pub strands: Vec<StrandPlan>,
    /// Candidates refused for binding in more than one place, over every strand.
    pub rejected: Vec<RejectedPrimer>,
}

/// `length` bases from `from`, which may be negative or past the end. `None` when a linear
/// construct has no such stretch.
fn take(bases: &[u8], from: i64, length: usize, circular: bool) -> Option<String> {
    let n = bases.len();
    if length == 0 || n == 0 || length > n {
        return None;
    }
    if !circular && (from < 0 || from as usize + length > n) {
        return None;
    }
    let start = from.rem_euclid(n as i64) as usize;
    (0..length).map(|k| bases.get((start + k) % n).map(|b| char::from(*b))).collect()
}

/// A position that may have run off either end, brought back onto a construct of `n` bases.
fn at(position: i64, n: usize) -> usize {
    position.rem_euclid(n as i64) as usize
}

/// How many bases `start..=end` covers, through the origin when `end < start`.
fn span(start: usize, end: usize, n: usize) -> usize {
    if end >= start { end - start + 1 } else { n.saturating_sub(start) + end + 1 }
}

/// One candidate primer at one anchor: the oligo, how far its 3' end sits from the ideal anchor,
/// and what it costs.
struct Candidate {
    primer: DesignedPrimer,
    offset: i64,
    cost: f64,
}

/// Tiling one strand of one frame. A frame is a construct read along its plus strand; the minus
/// strand is tiled by handing this the construct's reverse complement, which is why there is one
/// tiler and not two.
struct Frame<'a> {
    bases: &'a str,
    circular: bool,
    /// 0-based start of the stretch asked for, in this frame.
    from: usize,
    /// Its length.
    length: usize,
}

impl Frame<'_> {
    fn n(&self) -> usize {
        self.bases.len()
    }

    /// The construct position of a position relative to the start of the stretch asked for.
    fn absolute(&self, relative: i64) -> usize {
        at(self.from as i64 + relative, self.n())
    }

    /// Candidates whose 3' end sits `offsets` from `anchor`, best first.
    fn candidates(&self, anchor: i64, criteria: &PrimerCriteria, offsets: std::ops::RangeInclusive<i64>) -> Vec<Candidate> {
        let bytes = self.bases.as_bytes();
        let mut out: Vec<Candidate> = Vec::new();
        for offset in offsets {
            let three_prime = anchor + offset;
            for length in criteria.min_length..=criteria.max_length {
                let start = self.from as i64 + three_prime + 1 - length as i64;
                let Some(anneal) = take(bytes, start, length, self.circular) else {
                    continue;
                };
                // The temperature is the gate and the cheapest thing to work out, so it goes
                // first: everything else is only computed for an oligo worth keeping.
                if !anneal_tm(&anneal).is_some_and(|tm| (tm - criteria.target_tm).abs() <= criteria.max_tm_difference) {
                    continue;
                }
                let primer = designed(anneal, "", self.absolute(three_prime + 1 - length as i64), self.absolute(three_prime), true);
                let Some(base_cost) = cost(&primer, criteria) else {
                    continue;
                };
                let rate = if offset > 0 { 2.0 * ANCHOR_COST } else { ANCHOR_COST };
                out.push(Candidate { primer, offset, cost: base_cost + rate * offset.unsigned_abs() as f64 });
            }
        }
        out.sort_by(|a, b| {
            a.cost
                .total_cmp(&b.cost)
                .then(a.offset.abs().cmp(&b.offset.abs()))
                .then(a.primer.length.cmp(&b.primer.length))
                .then(a.primer.start.cmp(&b.primer.start))
        });
        out
    }

    /// How many places this oligo anneals: fully and perfectly, and in total. `oligo` holds bases
    /// only, so its length in bytes is its length in bases.
    fn binding(&self, oligo: &str) -> (usize, usize) {
        let options = PrimerSearchOptions { circular: self.circular, ..PrimerSearchOptions::default() };
        let sites = find_primer_sites(self.bases, oligo, options).unwrap_or_default();
        let full = sites.iter().filter(|s| s.mismatches == 0 && s.anneal_length == oligo.len()).count();
        (full, sites.len())
    }
}

/// The stretches of `0..length` that `covered` leaves out, merged and in order.
fn uncovered(covered: &[(i64, i64)], length: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(i64, i64)> = covered.iter().filter(|(a, b)| b >= a).copied().collect();
    ranges.sort_unstable();
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    let mut reached: i64 = 0;
    for (from, to) in ranges {
        if from > reached {
            gaps.push((reached as usize, (from - 1) as usize));
        }
        reached = reached.max(to + 1);
    }
    if reached < length as i64 {
        gaps.push((reached as usize, length - 1));
    }
    gaps
}

/// Why nothing could be taken at an anchor, in words, from the best near miss.
///
/// `back` and `ahead` are exactly the ground the tiler covered, so the reason cannot blame a
/// temperature on an oligo the tiler never looked at.
fn no_primer_here(frame: &Frame<'_>, anchor: i64, criteria: &PrimerCriteria, back: usize, ahead: usize, refused: usize) -> String {
    let bytes = frame.bases.as_bytes();
    let mut closest: Option<(f64, usize, f64)> = None;
    for offset in -(back as i64)..=ahead as i64 {
        for length in criteria.min_length..=criteria.max_length {
            let start = frame.from as i64 + anchor + offset + 1 - length as i64;
            let Some(anneal) = take(bytes, start, length, frame.circular) else {
                continue;
            };
            if let Some(tm) = anneal_tm(&anneal) {
                let off = (tm - criteria.target_tm).abs();
                if closest.is_none_or(|(best, _, _)| off < best) {
                    closest = Some((off, length, tm));
                }
            }
        }
    }
    // The temperature is only the reason when nothing in the window met it. Where candidates did
    // and were turned away for binding twice, saying the Tm was wrong would be a plain lie.
    let mut why: Vec<String> = Vec::new();
    match closest {
        None => why.push("no oligo there has a melting temperature at all, so those bases are not all A, C, G and T".to_string()),
        Some((off, length, tm)) if off > criteria.max_tm_difference => why.push(format!(
            "the closest is a {length}-base oligo at Tm {} °C, further from the {} °C target than the {} °C allowed",
            (tm * 10.0).round() / 10.0,
            criteria.target_tm,
            criteria.max_tm_difference
        )),
        Some(_) => {}
    }
    if refused > 0 {
        why.push(format!(
            "the {refused} best candidate(s) by these criteria all bind the construct in more than one place, which gives an unreadable trace"
        ));
    }
    if why.is_empty() {
        why.push("nothing there met the criteria".to_string());
    }
    format!("no acceptable primer near base {} ({back} bases back, {ahead} forward): {}", frame.absolute(anchor) + 1, why.join("; "))
}

/// What one strand's tiling produced, in the frame's own relative coordinates.
struct Tiled {
    reads: Vec<SequencingRead>,
    /// Stretches of the requested range the reads cover, relative to its start; they may overlap.
    covered: Vec<(i64, i64)>,
    /// An anchor nothing could be taken at, and why, relative to the start of the range.
    failures: Vec<(i64, String)>,
    rejected: Vec<RejectedPrimer>,
}

/// Tile one frame's plus strand, best primer first at each anchor.
fn tile(frame: &Frame<'_>, criteria: &PrimerCriteria, settings: &SequencingSettings) -> Tiled {
    let length = frame.length as i64;
    let step = (settings.read_length - settings.overlap) as i64;
    // Kept one short of the stride so that every read moves the anchor on: a window wider than the
    // stride could take a primer behind the last one and tile for ever.
    let back = settings.search_window.min(step.saturating_sub(1).max(0) as usize);
    let mut out = Tiled { reads: Vec::new(), covered: Vec::new(), failures: Vec::new(), rejected: Vec::new() };
    // The 3' end of the next primer, relative to the first base of the stretch asked for. At -1
    // the first read begins exactly at that base.
    let mut anchor: i64 = -1;
    let mut reached: i64 = -1;
    let mut rounds = 0usize;
    while anchor <= length - 2 && reached < length - 1 && rounds < MAX_READS_PER_STRAND {
        rounds += 1;
        let mut refused_here = 0usize;
        let mut taken: Option<(DesignedPrimer, i64, usize)> = None;
        // At or behind the anchor first, which keeps the overlap asked for. Only if that finds
        // nothing is the ground ahead looked at, where a read may share fewer bases with the one
        // before it or begin after it.
        for offsets in [-(back as i64)..=0, 1..=settings.search_window as i64] {
            if taken.is_some() {
                break;
            }
            for candidate in frame.candidates(anchor, criteria, offsets) {
                if refused_here >= MAX_BINDING_CHECKS {
                    break;
                }
                let (full, all) = frame.binding(&candidate.primer.sequence);
                if full == 1 {
                    taken = Some((candidate.primer, candidate.offset, all - 1));
                    break;
                }
                refused_here += 1;
                if out.rejected.len() < MAX_REJECTED_REPORTED {
                    out.rejected.push(RejectedPrimer {
                        sequence: candidate.primer.sequence.clone(),
                        start: candidate.primer.start,
                        end: candidate.primer.end,
                        forward: true,
                        why: if full == 0 {
                            format!("no full perfect landing on the construct, and {all} weaker site(s)")
                        } else {
                            format!("binds fully and perfectly in {full} places, which gives an unreadable trace")
                        },
                    });
                }
            }
        }
        match taken {
            Some((primer, offset, other_sites)) => {
                let three_prime = anchor + offset;
                let (read_from, read_to) = (three_prime + 1, three_prime + settings.read_length as i64);
                let shared = if out.reads.is_empty() || reached < read_from { 0 } else { (reached - read_from + 1) as usize };
                let mut warnings = primer.flags.warnings.clone();
                warnings.extend(primer.hairpin.warnings.iter().cloned());
                out.reads.push(SequencingRead {
                    read_start: frame.absolute(read_from),
                    read_end: frame.absolute(read_to),
                    overlap_with_previous: shared,
                    other_sites,
                    warnings,
                    primer,
                });
                out.covered.push((read_from.max(0), read_to.min(length - 1)));
                reached = reached.max(read_to);
                // Never behind where it already was: a step that went nowhere would tile for ever.
                anchor = (read_to - settings.overlap as i64).max(anchor + 1);
            }
            None => {
                out.failures.push((anchor, no_primer_here(frame, anchor, criteria, back, settings.search_window, refused_here)));
                anchor += step;
            }
        }
    }
    if reached < length - 1 && rounds >= MAX_READS_PER_STRAND {
        out.failures.push((reached, format!("the tiling stopped after {MAX_READS_PER_STRAND} reads on this strand")));
    }
    out
}

/// The gaps `tiled` leaves in a range of `length` bases, each with the reason from the anchor whose
/// read would have covered it.
fn gaps_of(frame: &Frame<'_>, tiled: &Tiled, length: usize, read_length: usize) -> Vec<SequencingGap> {
    uncovered(&tiled.covered, length)
        .into_iter()
        .map(|(from, to)| {
            let (from, to) = (from as i64, to as i64);
            let mut why: Vec<&str> = tiled
                .failures
                .iter()
                // The read that anchor would have given, had one been found, runs from the base
                // after it for `read_length`. Keep the reasons whose read would have reached here.
                .filter(|(anchor, _)| *anchor < to && anchor + read_length as i64 >= from)
                .map(|(_, why)| why.as_str())
                .collect();
            why.dedup();
            let unexplained = if from == 0 {
                // A read starts after its primer's 3' end, so the bases the first primer sits on
                // cannot be read from inside the stretch at all. On a circular construct the primer
                // goes round the back of the origin and this never arises; on a linear one it is
                // the vector's own primer that reads these bases.
                "a read begins after its primer's 3' end, so these bases need a primer upstream of the stretch asked for"
            } else {
                "no read reaches these bases"
            };
            SequencingGap {
                start: frame.absolute(from),
                end: frame.absolute(to),
                why: if why.is_empty() { unexplained.to_string() } else { why.join("; ") },
            }
        })
        .collect()
}

/// Sanger primers tiled across `start..=end` of a construct (the whole of it when that is what the
/// stretch is), so that the reads overlap by `settings.overlap` and cover every base.
///
/// `criteria` is the same set [`crate::primer_design::design_primer_pairs`] takes; a sequencing
/// primer has no partner, so `max_tm_difference` is read as how far from `target_tm` one primer may
/// be, and `max_pairs` and `boundary_slack` are not used. The melting temperature is the gate;
/// GC content, the 3' clamp and the quality flags order the candidates that pass it.
///
/// `Err` only for settings that cannot be met at all: an overlap at or past the read length, a
/// stretch outside the construct, a stretch through the origin of a linear construct. A stretch
/// that cannot be covered is not an error — it comes back as a gap that says why.
pub fn design_sequencing_primers(
    template: &str,
    circular: bool,
    start: usize,
    end: usize,
    criteria: &PrimerCriteria,
    settings: &SequencingSettings,
) -> Result<SequencingPlan, DesignError> {
    let bases = clean_seq(template);
    let n = bases.len();
    let bad = |why: &str| Err(DesignError::Criteria(why.to_string()));
    if criteria.min_length < 2 || criteria.max_length < criteria.min_length {
        return bad("the shortest primer length must be at least 2 bases and no longer than the longest");
    }
    if n == 0 || start >= n || end >= n {
        return Err(DesignError::Criteria(format!("the stretch must lie inside the construct's {n} bases")));
    }
    if end < start && !circular {
        return bad("that stretch runs through the origin, but this construct is linear");
    }
    if settings.read_length == 0 {
        return bad("a read has to be at least one base long");
    }
    if settings.overlap >= settings.read_length {
        return bad("the overlap must be shorter than the read, or the tiling never moves on");
    }
    if settings.read_length > n {
        return Err(DesignError::Criteria(format!("one read is {} bases and the whole construct is {n}; shorten read_length", settings.read_length)));
    }

    let length = span(start, end, n);
    let mut strands: Vec<StrandPlan> = Vec::new();
    let mut rejected: Vec<RejectedPrimer> = Vec::new();

    let frame = Frame { bases: &bases, circular, from: start, length };
    let tiled = tile(&frame, criteria, settings);
    let gaps = gaps_of(&frame, &tiled, length, settings.read_length);
    rejected.extend(tiled.rejected);
    strands.push(StrandPlan {
        forward: true,
        covered: length - gaps.iter().map(|g| span(g.start, g.end, n)).sum::<usize>(),
        reads: tiled.reads,
        gaps,
    });

    if settings.both_strands {
        // The bottom strand is the top strand of the reverse complement. Tiling that and mirroring
        // the answers back is the same work as a second tiler reading the other way, without a
        // second set of off-by-one mistakes to make.
        let flipped = revcomp(&bases);
        let mirror = |position: usize| n - 1 - position;
        let frame = Frame { bases: &flipped, circular, from: mirror(end), length };
        let tiled = tile(&frame, criteria, settings);
        let gaps = gaps_of(&frame, &tiled, length, settings.read_length);
        rejected.extend(tiled.rejected.into_iter().map(|r| RejectedPrimer { start: mirror(r.end), end: mirror(r.start), forward: false, ..r }));
        let covered = length - gaps.iter().map(|g| span(g.start, g.end, n)).sum::<usize>();
        strands.push(StrandPlan {
            forward: false,
            covered,
            reads: tiled
                .reads
                .into_iter()
                .map(|read| SequencingRead {
                    read_start: mirror(read.read_end),
                    read_end: mirror(read.read_start),
                    primer: DesignedPrimer { start: mirror(read.primer.end), end: mirror(read.primer.start), forward: false, ..read.primer },
                    ..read
                })
                .collect(),
            gaps: gaps.into_iter().map(|gap| SequencingGap { start: mirror(gap.end), end: mirror(gap.start), why: gap.why }).collect(),
        });
    }

    Ok(SequencingPlan { start, end, requested: length, strands, rejected })
}
