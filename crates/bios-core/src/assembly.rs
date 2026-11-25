//! Building a whole sequence out of short synthetic oligos: the layout that turns a target into a
//! numbered list of oligos to order, which anneal to one another and are filled in and amplified
//! into the full double-stranded target by PCR (polymerase cycling assembly).
//!
//! The oligos alternate strands and are staggered, so each one overlaps both its neighbours. Those
//! overlaps are the duplexes that anneal, so they are sized by melting temperature and not by
//! length: each grows out from its junction until it reaches the target Tm. Oligo lengths come out
//! uneven as a result, bounded by the longest oligo that can be ordered.
//!
//! ```text
//! target     5'--------------------------------------------------3'
//! oligo 1    ================>
//! oligo 2             <================
//! oligo 3                      ================>
//! oligo 4                               <=================
//!                     |--------|  the overlap: grown until it melts at the target Tm
//!            |-- no longer than max_length --|
//! ```
//!
//! Two promises hold for every plan this returns, and the tests check both: the oligos put back
//! together are the target, base for base, and no oligo is longer than `max_length`. Whether every
//! overlap reached the target temperature is reported per overlap rather than promised, because a
//! stretch of sequence can be too AT-rich to melt that high inside [`MAX_OVERLAP`] bases.
//!
//! The melting temperature is [`gibson_tm`], the nearest-neighbour one Bios already designs
//! overlaps with, so a temperature here means what it means in a Gibson design.
//!
//! Native-only: the Bios web app has no oligo assembly, so nothing here is a port. Coordinates are
//! 0-based, `start` inclusive and `end` exclusive, on the target's top strand.

use crate::gibson::{check_uniqueness, div_ceil, gc_of, gibson_tm, js_slice, len64, round1, tm_range, tm_range_reverse};
use crate::seq::{clean_seq, revcomp};

/// Shortest overlap worth annealing, however hot a shorter one measures. Two oligos that share
/// less than this find each other too rarely for the assembly to converge.
pub const MIN_OVERLAP: i64 = 18;
/// Longest overlap to grow before settling for the temperature reached. An overlap this long in a
/// 60 nt oligo leaves almost nothing of the oligo unique to it.
pub const MAX_OVERLAP: i64 = 60;
/// Longest oligo most suppliers make cheaply on a plate.
pub const DEFAULT_MAX_LENGTH: i64 = 60;
/// The temperature overlaps aim for, in °C.
pub const DEFAULT_TARGET_TM: f64 = 60.0;
/// Most oligos one assembly may ask for. Past this the reaction is the problem, not the design:
/// order the target as synthetic DNA instead.
pub const MAX_OLIGOS: i64 = 400;
/// How far below the temperature asked for the design may settle, in °C, when no tiling reaches
/// it. Every junction still shares whatever floor is found; what it is, is reported.
pub const MAX_SHORTFALL: i32 = 10;
/// Shortest and longest amplification primer at the ends of the target.
const PRIMER_MIN: i64 = 18;
const PRIMER_MAX: i64 = 35;

/// A design that cannot be satisfied. Every message is read by somebody about to order DNA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssemblyError {
    NoBases,
    /// An oligo cap too small to hold the overlap at each of its two ends.
    MaxLengthBelowOverlaps {
        max_length: i64,
    },
    TooManyOligos {
        needed: i64,
    },
    /// No tiling fits the cap, at any piece count tried.
    NoLayout {
        max_length: i64,
    },
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssemblyError::NoBases => write!(f, "The target holds no bases, so there is nothing to build."),
            AssemblyError::MaxLengthBelowOverlaps { max_length } => write!(
                f,
                "An oligo cap of {max_length} nt cannot hold an overlap at each end: every oligo in the middle shares {MIN_OVERLAP} nt with the one before it and {MIN_OVERLAP} nt with the one after, so the cap has to be at least {} nt.",
                MIN_OVERLAP * 2
            ),
            AssemblyError::TooManyOligos { needed } => write!(
                f,
                "This target needs about {needed} oligos, past the {MAX_OLIGOS} one assembly can hold. Build it in sections and join them, or order it as synthetic DNA."
            ),
            AssemblyError::NoLayout { max_length } => write!(
                f,
                "No set of oligos {max_length} nt or shorter tiles this target with overlaps of at least {MIN_OVERLAP} nt. Raise the oligo cap."
            ),
        }
    }
}

impl std::error::Error for AssemblyError {}

/// One oligo to order, written 5' to 3' as it is ordered.
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyOligo {
    /// Its place in the order, from 1.
    pub number: i64,
    pub name: String,
    /// The bases to order, 5' to 3'. For a reverse oligo this is the reverse complement of the
    /// target's `start..end`, which is what the supplier is sent.
    pub sequence: String,
    /// True when the oligo reads along the target's top strand.
    pub forward: bool,
    /// Where it sits on the target, 0-based, `end` exclusive.
    pub start: i64,
    pub end: i64,
    pub length: i64,
    pub gc: f64,
}

/// The duplex two consecutive oligos share, written on the top strand.
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyOverlap {
    /// It joins this oligo to the next one, numbered from 1.
    pub after: i64,
    pub sequence: String,
    pub start: i64,
    pub end: i64,
    pub length: i64,
    pub tm: f64,
    /// False when the stretch could not be grown to the target temperature inside [`MAX_OVERLAP`]
    /// bases: the sequence there is too AT-rich, and `tm` is the best that was reached.
    pub reached_target: bool,
    /// Other places in the target the overlap could anneal, either strand. Anything above 0 is a
    /// chance for the assembly to join in the wrong order.
    pub elsewhere: i64,
}

/// One of the two primers that amplify the assembled target once the oligos have built it.
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyPrimer {
    pub name: String,
    pub sequence: String,
    pub forward: bool,
    pub length: i64,
    pub tm: f64,
    pub gc: f64,
}

/// A whole assembly: what to order, and what each junction will do.
#[derive(Clone, Debug, PartialEq)]
pub struct AssemblyPlan {
    pub target_length: i64,
    pub max_length: i64,
    pub target_tm: f64,
    pub oligos: Vec<AssemblyOligo>,
    pub overlaps: Vec<AssemblyOverlap>,
    pub forward_primer: Option<AssemblyPrimer>,
    pub reverse_primer: Option<AssemblyPrimer>,
}

impl AssemblyPlan {
    /// The coolest junction in the plan: the one that decides the annealing temperature.
    pub fn lowest_overlap_tm(&self) -> Option<f64> {
        self.overlaps.iter().map(|o| o.tm).fold(None, |lowest, tm| Some(lowest.map_or(tm, |l: f64| l.min(tm))))
    }

    /// Every junction grew to the target temperature.
    pub fn all_reached_target(&self) -> bool {
        self.overlaps.iter().all(|o| o.reached_target)
    }

    /// Every junction anneals in one place only.
    pub fn all_unique(&self) -> bool {
        self.overlaps.iter().all(|o| o.elsewhere == 0)
    }

    /// Bases of oligo to pay for, the amplification primers included.
    pub fn total_bases(&self) -> i64 {
        self.oligos.iter().map(|o| o.length).sum::<i64>()
            + self.forward_primer.as_ref().map_or(0, |p| p.length)
            + self.reverse_primer.as_ref().map_or(0, |p| p.length)
    }

    /// Put the oligos back together the way the reaction does — each one found by the stretch it
    /// shares with the one before it — and return what they build. `None` when a junction has no
    /// shared stretch, which would mean the plan does not assemble at all.
    ///
    /// This reads nothing but the ordered sequences, so it is a real check on the plan rather than
    /// a restatement of the coordinates it was built from.
    pub fn rebuild(&self) -> Option<String> {
        let mut built = String::new();
        for oligo in &self.oligos {
            let top = if oligo.forward { oligo.sequence.clone() } else { revcomp(&oligo.sequence) };
            if built.is_empty() {
                built = top;
                continue;
            }
            let longest = len64(&built).min(len64(&top));
            let mut joined = false;
            for size in (MIN_OVERLAP..=longest).rev() {
                let head = js_slice(&top, 0, size);
                if built.ends_with(head) {
                    built.push_str(js_slice(&top, size, len64(&top)));
                    joined = true;
                    break;
                }
            }
            if !joined {
                return None;
            }
        }
        Some(built)
    }
}

/// Design the oligos that build `target`.
///
/// `max_length` is the longest oligo that can be ordered and `target_tm` the temperature every
/// overlap aims for. `base_name` names the oligos, which come back as `<base_name>_01` upward.
///
/// A target that already fits in one oligo comes back as that one oligo, with no overlaps.
pub fn design_assembly_oligos(target: &str, max_length: i64, target_tm: f64, base_name: &str) -> Result<AssemblyPlan, AssemblyError> {
    let seq = clean_seq(target);
    let n = len64(&seq);
    if n == 0 {
        return Err(AssemblyError::NoBases);
    }
    let name = if base_name.trim().is_empty() { "oligo" } else { base_name.trim() };

    if n <= max_length {
        let only = AssemblyOligo {
            number: 1,
            name: oligo_name(name, 1, 1),
            sequence: seq.clone(),
            forward: true,
            start: 0,
            end: n,
            length: n,
            gc: round1(gc_of(&seq)),
        };
        return Ok(AssemblyPlan {
            target_length: n,
            max_length,
            target_tm,
            oligos: vec![only],
            overlaps: Vec::new(),
            forward_primer: None,
            reverse_primer: None,
        });
    }

    // AN OLIGO IN THE MIDDLE CARRIES TWO OVERLAPS, one at each end, and they may not share bases:
    // below twice the minimum there is no tiling at all, at any piece count, so the search below
    // would spin through every count and fail. Say why rather than hand back "no layout".
    if max_length < MIN_OVERLAP * 2 {
        return Err(AssemblyError::MaxLengthBelowOverlaps { max_length });
    }

    // With k oligos the target can be at most k·max_length long, less the bases each junction
    // spends twice, so this is the fewest that could possibly reach the end.
    let fewest = div_ceil(n - MIN_OVERLAP, max_length - MIN_OVERLAP).max(2);
    if fewest > MAX_OLIGOS {
        return Err(AssemblyError::TooManyOligos { needed: fewest });
    }

    // ASK FOR ONE TEMPERATURE EVERYWHERE, AND COME DOWN TO FIND IT. The reaction anneals at a
    // single temperature, so a design whose junctions melt at 50, 60 and 68 is worse than one
    // where they all melt at 58, even though the second never reaches the number asked for.
    //
    // A target cannot always give the temperature asked for. Two overlaps sit on one oligo and may
    // not share bases, so they have to fit in it together; in a stretch where each would need 34
    // of a 60 nt oligo to reach 60 °C, no pair fits, and no tiling at that temperature exists. So
    // try the temperature asked for, then a degree lower, and so on, and take the first that every
    // junction in some tiling can meet. What that floor turned out to be is reported, per overlap
    // and as `lowest_overlap_tm`, and it is the temperature to run the reaction at.
    let reach = MAX_OVERLAP.min(max_length - MIN_OVERLAP);
    let mut junctions = None;
    for shortfall in 0..=MAX_SHORTFALL {
        let want = target_tm - f64::from(shortfall);
        if let Some(found) = tile(n, max_length, &overlap_lengths(&seq, want, reach, true)) {
            junctions = Some(found);
            break;
        }
    }
    // Last resort, for a target too cold even for that: overlaps of at most half an oligo, so any
    // two fit together and a tiling always exists, each as warm as it can be in that space.
    let settled = || overlap_lengths(&seq, target_tm, (max_length / 2).max(MIN_OVERLAP), false);
    let Some(junctions) = junctions.or_else(|| tile(n, max_length, &settled())) else {
        return Err(AssemblyError::NoLayout { max_length });
    };

    let count = len64_of(junctions.len()) + 1;
    if count > MAX_OLIGOS {
        return Err(AssemblyError::TooManyOligos { needed: count });
    }
    let mut oligos = Vec::new();
    for index in 0..count {
        let start = if index == 0 { 0 } else { junctions.get(usize_of(index - 1)).map_or(0, |j: &(i64, i64)| j.0) };
        let end = junctions.get(usize_of(index)).map_or(n, |j: &(i64, i64)| j.1);
        let top = js_slice(&seq, start, end);
        let forward = index % 2 == 0;
        let ordered = if forward { top.to_string() } else { revcomp(top) };
        oligos.push(AssemblyOligo {
            number: index + 1,
            name: oligo_name(name, index + 1, count),
            sequence: ordered,
            forward,
            start,
            end,
            length: end - start,
            gc: round1(gc_of(top)),
        });
    }

    let overlaps = junctions
        .iter()
        .enumerate()
        .map(|(index, &(start, end))| {
            let text = js_slice(&seq, start, end);
            let tm = gibson_tm(text).unwrap_or(0.0);
            let hits = check_uniqueness(&seq, text, false);
            AssemblyOverlap {
                after: len64_of(index) + 1,
                sequence: text.to_string(),
                start,
                end,
                length: end - start,
                tm: round1(tm),
                reached_target: tm >= target_tm,
                elsewhere: len64_of(hits.fwd.saturating_sub(1) + hits.rc),
            }
        })
        .collect();

    Ok(AssemblyPlan {
        target_length: n,
        max_length,
        target_tm,
        oligos,
        overlaps,
        forward_primer: Some(end_primer(&seq, target_tm, name, true)),
        reverse_primer: Some(end_primer(&seq, target_tm, name, false)),
    })
}

/// The shortest overlap that reaches `target_tm` starting at each base of the target, or `None`
/// where there is no usable one. `cap` is the longest overlap allowed, and with `require_hot` a
/// position whose overlap never reaches the temperature is refused outright rather than reported
/// with the best it managed.
///
/// Sizing every position once, up front, is what lets [`tile`] treat a junction as a yes-or-no
/// choice: whether a junction is hot depends only on where it starts, never on the oligo that
/// happens to carry it.
fn overlap_lengths(seq: &str, target_tm: f64, cap: i64, require_hot: bool) -> Vec<Option<i64>> {
    let n = len64(seq);
    (0..n)
        .map(|start| {
            let room = cap.min(n - start);
            if room < MIN_OVERLAP {
                return None;
            }
            let grown = tm_range(seq, start, target_tm, MIN_OVERLAP, room);
            if require_hot && grown.tm < target_tm {
                return None;
            }
            Some(grown.pos - start)
        })
        .collect()
}

/// Lay the junctions out along the target, using as few oligos as possible, given where an overlap
/// may start. Returns each junction's overlap as `(start, end)` on the top strand, or `None` when
/// the overlaps on offer cannot tile the target within the cap.
///
/// THIS IS A SEARCH AND NOT A WALK, and it has to be. Placing each junction in turn and keeping it
/// is what the first version did, and it wedges: one overlap that has to grow long to reach the
/// temperature eats the rest of its oligo, leaving the next junction a window of a few bases with
/// nothing warm in it, and the walk cannot go back and move the overlap that caused it. Breadth
/// first over junction positions has no such blind spot — it finds a tiling whenever one exists,
/// and the first one it reaches uses the fewest oligos, which is also the cheapest to order.
fn tile(n: i64, max_length: i64, overlap: &[Option<i64>]) -> Option<Vec<(i64, i64)>> {
    /// A position no junction has been reached at yet.
    const UNVISITED: i64 = -2;
    /// The predecessor of a first junction: the start of the target, which is no junction at all.
    const FROM_START: i64 = -1;

    let width = usize_of(n);
    let mut came_from: Vec<i64> = vec![UNVISITED; width];
    let mut queue: std::collections::VecDeque<i64> = std::collections::VecDeque::new();
    let length_at = |at: i64| overlap.get(usize_of(at)).copied().flatten();

    // The first oligo starts at the target's first base, so its overlap may begin anywhere that
    // still leaves the oligo inside the cap.
    for start in MIN_OVERLAP..n {
        if start + MIN_OVERLAP > max_length {
            break;
        }
        let Some(length) = length_at(start) else { continue };
        if start + length > max_length {
            continue;
        }
        if let Some(slot) = came_from.get_mut(usize_of(start)) {
            *slot = FROM_START;
        }
        queue.push_back(start);
    }

    let mut last = None;
    while let Some(at) = queue.pop_front() {
        // The oligo that begins here is the last one when it reaches the end inside the cap.
        if n - at <= max_length {
            last = Some(at);
            break;
        }
        let Some(length) = length_at(at) else { continue };
        // The next overlap starts after this one ends — two overlaps on one oligo may not share
        // bases — and has to finish inside the oligo they both sit on.
        for next in (at + length)..n {
            if next + MIN_OVERLAP > at + max_length {
                break;
            }
            let Some(next_length) = length_at(next) else { continue };
            if next + next_length > at + max_length {
                continue;
            }
            let Some(slot) = came_from.get_mut(usize_of(next)) else { continue };
            if *slot != UNVISITED {
                continue;
            }
            *slot = at;
            queue.push_back(next);
        }
    }

    let mut path = Vec::new();
    let mut at = last?;
    loop {
        path.push((at, at + length_at(at)?));
        match came_from.get(usize_of(at)).copied() {
            Some(FROM_START) => break,
            Some(previous) if previous >= 0 => at = previous,
            _ => return None,
        }
    }
    path.reverse();
    Some(path)
}

/// A primer on one end of the target, grown inward until it melts at the target temperature.
fn end_primer(seq: &str, target_tm: f64, base_name: &str, forward: bool) -> AssemblyPrimer {
    let n = len64(seq);
    let text = if forward {
        js_slice(seq, 0, tm_range(seq, 0, target_tm, PRIMER_MIN, PRIMER_MAX).pos).to_string()
    } else {
        revcomp(js_slice(seq, tm_range_reverse(seq, n, target_tm, PRIMER_MIN, PRIMER_MAX).pos, n))
    };
    AssemblyPrimer {
        name: format!("{base_name}_{}", if forward { "F" } else { "R" }),
        length: len64(&text),
        tm: round1(gibson_tm(&text).unwrap_or(0.0)),
        gc: round1(gc_of(&text)),
        forward,
        sequence: text,
    }
}

/// `<base>_01`, with as many digits as the longest number needs, so an ordered list sorts right.
fn oligo_name(base: &str, number: i64, count: i64) -> String {
    let width = count.to_string().len().max(2);
    format!("{base}_{number:0width$}")
}

fn len64_of(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn usize_of(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target of mixed GC that never repeats itself, which is what a real gene looks like and
    /// what [`AssemblyPlan::rebuild`] needs: on a target built from one repeated unit the junctions
    /// are genuinely ambiguous, and no reader of the oligos — the polymerase included — can tell
    /// which neighbour is which. That case has its own test.
    ///
    /// The generator is a fixed linear congruence, so the bases are the same on every run and on
    /// every machine, and a failure is reproducible.
    fn bases(length: usize, alphabet: &[char]) -> String {
        let mut state: u64 = 0x2545_F491_4F6C_DD1D;
        (0..length)
            .map(|_| {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let pick = usize::try_from(state >> 33).unwrap_or(0) % alphabet.len();
                alphabet.get(pick).copied().unwrap_or('A')
            })
            .collect()
    }

    fn target(length: usize) -> String {
        bases(length, &['A', 'C', 'G', 'T'])
    }

    fn plan(length: usize) -> AssemblyPlan {
        design_assembly_oligos(&target(length), DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "gene").expect("a plan")
    }

    #[test]
    fn the_oligos_rebuild_the_target() {
        for length in [120usize, 300, 601, 1000] {
            let plan = plan(length);
            assert_eq!(plan.rebuild().as_deref(), Some(target(length).as_str()), "length {length}");
        }
    }

    #[test]
    fn no_oligo_is_longer_than_the_cap() {
        for cap in [40i64, 60, 90, 120] {
            let plan = design_assembly_oligos(&target(800), cap, DEFAULT_TARGET_TM, "gene").expect("a plan");
            assert!(plan.oligos.iter().all(|o| o.length <= cap), "cap {cap}");
            assert_eq!(plan.rebuild().as_deref(), Some(target(800).as_str()), "cap {cap}");
        }
    }

    /// The promise is one temperature for the whole reaction: every junction at or above a single
    /// floor, and that floor as near the temperature asked for as the target allows.
    #[test]
    fn every_overlap_holds_one_temperature_near_the_one_asked_for() {
        for want in [55.0f64, 60.0, 65.0] {
            let plan = design_assembly_oligos(&target(900), DEFAULT_MAX_LENGTH, want, "gene").expect("a plan");
            let floor = plan.lowest_overlap_tm().expect("a floor");
            let shortfall = want - floor;
            assert!(shortfall <= f64::from(MAX_SHORTFALL), "at {want} °C the floor sank to {floor}");
            // A junction hotter than the floor is harmless — the reaction is held back by its
            // coolest one — but it must not be hot through waste. Every overlap is the shortest
            // stretch that does the job: one base less and it would fall below the floor, unless
            // it is already at the shortest overlap worth annealing.
            for overlap in &plan.overlaps {
                if overlap.length == MIN_OVERLAP {
                    continue;
                }
                let trimmed = js_slice(&overlap.sequence, 0, overlap.length - 1);
                let cooler = round1(gibson_tm(trimmed).unwrap_or(0.0));
                assert!(cooler < floor, "at {want} °C a {} nt overlap would still hold {cooler} at {} nt", overlap.length, overlap.length - 1);
            }
        }
    }

    /// A target of ordinary composition should give the temperature asked for outright.
    #[test]
    fn an_ordinary_target_reaches_the_temperature_asked_for() {
        let plan = design_assembly_oligos(&target(900), DEFAULT_MAX_LENGTH, 55.0, "gene").expect("a plan");
        assert!(plan.all_reached_target(), "junctions: {:?}", plan.overlaps.iter().map(|o| o.tm).collect::<Vec<_>>());
    }

    #[test]
    fn the_oligos_alternate_strands_and_run_in_order() {
        let plan = plan(700);
        for (index, oligo) in plan.oligos.iter().enumerate() {
            assert_eq!(oligo.forward, index % 2 == 0, "oligo {}", oligo.number);
            assert_eq!(oligo.number, len64_of(index) + 1);
        }
        for pair in plan.oligos.windows(2) {
            let (left, right) = (pair.first().expect("left"), pair.get(1).expect("right"));
            assert!(right.start > left.start, "oligos must advance along the target");
            assert!(right.start < left.end, "neighbours must overlap");
        }
        assert_eq!(plan.oligos.first().map(|o| o.start), Some(0));
        assert_eq!(plan.oligos.last().map(|o| o.end), Some(plan.target_length));
    }

    #[test]
    fn a_reverse_oligo_is_ordered_as_its_reverse_complement() {
        let plan = plan(400);
        let full = target(400);
        for oligo in &plan.oligos {
            let top = js_slice(&full, oligo.start, oligo.end);
            let expected = if oligo.forward { top.to_string() } else { revcomp(top) };
            assert_eq!(oligo.sequence, expected, "oligo {}", oligo.number);
        }
    }

    #[test]
    fn an_overlap_is_the_stretch_two_neighbours_share() {
        let plan = plan(600);
        for overlap in &plan.overlaps {
            let before = plan.oligos.get(usize_of(overlap.after - 1)).expect("the oligo before");
            let after = plan.oligos.get(usize_of(overlap.after)).expect("the oligo after");
            assert_eq!(overlap.start, after.start);
            assert_eq!(overlap.end, before.end);
            assert!(overlap.length >= MIN_OVERLAP);
        }
        assert_eq!(len64_of(plan.overlaps.len()), len64_of(plan.oligos.len()) - 1);
    }

    #[test]
    fn a_target_that_fits_one_oligo_is_one_oligo() {
        let plan = design_assembly_oligos("ATGGCCTAGCATCGATTACGGGCCATATTAGCCGG", DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "short").expect("a plan");
        assert_eq!(plan.oligos.len(), 1);
        assert!(plan.overlaps.is_empty());
        assert_eq!(plan.oligos.first().map(|o| o.sequence.as_str()), Some("ATGGCCTAGCATCGATTACGGGCCATATTAGCCGG"));
    }

    #[test]
    fn the_amplification_primers_sit_on_the_two_ends() {
        let plan = plan(500);
        let full = target(500);
        let forward = plan.forward_primer.as_ref().expect("a forward primer");
        let reverse = plan.reverse_primer.as_ref().expect("a reverse primer");
        assert!(full.starts_with(&forward.sequence));
        assert!(full.ends_with(&revcomp(&reverse.sequence)));
        assert!(forward.tm >= DEFAULT_TARGET_TM, "forward primer at {} °C", forward.tm);
        assert!(reverse.tm >= DEFAULT_TARGET_TM, "reverse primer at {} °C", reverse.tm);
    }

    #[test]
    fn a_cap_too_small_for_two_overlaps_says_so() {
        let err = design_assembly_oligos(&target(300), 30, DEFAULT_TARGET_TM, "gene").expect_err("refused");
        assert_eq!(err, AssemblyError::MaxLengthBelowOverlaps { max_length: 30 });
        assert!(err.to_string().contains("at least 36 nt"));
    }

    #[test]
    fn an_empty_target_says_so() {
        assert_eq!(design_assembly_oligos("   ", DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "gene"), Err(AssemblyError::NoBases));
    }

    #[test]
    fn a_target_past_the_oligo_ceiling_says_so() {
        let err = design_assembly_oligos(&target(40_000), DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "gene").expect_err("refused");
        assert!(matches!(err, AssemblyError::TooManyOligos { .. }));
        assert!(err.to_string().contains("400"));
    }

    #[test]
    fn names_are_numbered_so_they_sort() {
        let plan = plan(600);
        assert_eq!(plan.oligos.first().map(|o| o.name.as_str()), Some("gene_01"));
        assert_eq!(plan.forward_primer.as_ref().map(|p| p.name.as_str()), Some("gene_F"));
        assert_eq!(plan.reverse_primer.as_ref().map(|p| p.name.as_str()), Some("gene_R"));
    }

    #[test]
    fn the_bill_counts_every_base_ordered() {
        let plan = plan(600);
        let by_hand = plan.oligos.iter().map(|o| o.length).sum::<i64>()
            + plan.forward_primer.as_ref().map_or(0, |p| p.length)
            + plan.reverse_primer.as_ref().map_or(0, |p| p.length);
        assert_eq!(plan.total_bases(), by_hand);
        assert!(plan.total_bases() > plan.target_length, "the overlaps are paid for twice");
    }

    /// A stretch with no G or C in it tops out near 50 °C however long the overlap grows, so a
    /// junction landing there cannot reach 60. The plan still has to come back, tile the target and
    /// say which junction fell short, rather than fail or quietly report the temperature as met.
    #[test]
    fn a_junction_that_cannot_reach_the_temperature_is_reported_not_hidden() {
        let at_rich = format!("{}{}{}", target(200), bases(150, &['A', 'T']), target(200));
        let plan = design_assembly_oligos(&at_rich, DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "hard").expect("a plan");
        assert_eq!(plan.rebuild().as_deref(), Some(at_rich.as_str()));
        assert!(plan.oligos.iter().all(|o| o.length <= DEFAULT_MAX_LENGTH));
        assert!(!plan.all_reached_target(), "the AT stretch cannot melt at 60 °C");
        let lowest = plan.lowest_overlap_tm().expect("a lowest");
        assert!(lowest < DEFAULT_TARGET_TM);
    }

    /// A target made of one repeated unit gives overlaps that anneal in many places. The plan must
    /// say so: this is the design that silently assembles in the wrong order.
    #[test]
    fn an_overlap_that_anneals_elsewhere_is_counted() {
        let repeated = "ATGGCCTAGCATCGATTACGGGCCATATTAGCCGGATCAACGTTAGCCAT".repeat(8);
        let plan = design_assembly_oligos(&repeated, DEFAULT_MAX_LENGTH, DEFAULT_TARGET_TM, "repeat").expect("a plan");
        assert!(!plan.all_unique(), "a pure repeat cannot have unique junctions");
        assert!(plan.overlaps.iter().any(|o| o.elsewhere > 0));
    }
}
