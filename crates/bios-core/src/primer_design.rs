//! Designing primers: pairs that amplify a stretch of a construct, and pairs that install a
//! change in it. Bios's own design, from published thermodynamics and the criteria a cloner
//! applies by hand; nothing here is ported from another designer.
//!
//! What it does is pick lengths. A pair that amplifies `start..end` has the forward primer's 5'
//! end pinned at `start` and the reverse primer's 5' end pinned at `end`, so the product is
//! exactly the stretch asked for whatever lengths are chosen; the two lengths are then varied
//! inside the length window and the combinations ranked by how close each primer's melting
//! temperature lands to the target, how far its GC content strays from the wanted band, whether
//! it ends in G or C, and the liabilities [`crate::primer_quality::primer_flags`] reports. Melting
//! temperature is the nearest-neighbour estimate of [`crate::seq::tm`] (SantaLucia 1998).
//!
//! [`PrimerCriteria::boundary_slack`] loosens the two pins. Where a boundary is GC-rich or
//! AT-rich, no primer of any allowed length starting exactly there reaches the target temperature,
//! and the slack lets each 5' end step OUTWARD — never inward — so the stretch asked for stays
//! whole inside a longer product. Each pair says how far each end moved.
//!
//! These are candidates ranked by well-known criteria. Nothing here predicts that a reaction will
//! amplify at the bench: no mispriming search beyond the flags, no dimer or folding
//! thermodynamics, no polymerase, no magnesium. What it does report, per pair, is the cross-dimer
//! and hairpin base-pair counts of [`crate::primer_quality`] and a recommended annealing
//! temperature from [`crate::annealing`].
//!
//! Coordinates are 0-based inclusive on the plus strand, as everywhere in `bios-core`; a stretch
//! through the origin of a circular construct has `end < start`.

use crate::annealing::{AnnealingTemperature, annealing_temperature};
use crate::pcr::{PcrInput, PcrPrediction, PcrPrimer, predict_pcr_product};
use crate::primer_quality::{DimerCheck, HairpinCheck, PrimerFlags, cross_dimer, hairpins, primer_flags};
use crate::seq::{DNA_TM_DEFAULTS, clean_seq, gc_content, revcomp, tm};

/// What a designed pair aims for. [`DEFAULT_PRIMER_CRITERIA`] is the usual starting point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrimerCriteria {
    /// Melting temperature each primer aims for, °C, of the part that anneals.
    pub target_tm: f64,
    /// Most the two primers' melting temperatures may differ, °C. It is also the distance from
    /// the target past which a primer's own temperature is called out in the reason.
    pub max_tm_difference: f64,
    pub min_length: usize,
    pub max_length: usize,
    pub min_gc: f64,
    pub max_gc: f64,
    /// Prefer a primer whose 3' base is G or C; it is a preference, never a refusal.
    pub prefer_gc_clamp: bool,
    /// Most pairs returned.
    pub max_pairs: usize,
    /// How far each primer's 5' end may move OUTWARD from the edge of the stretch asked for, in
    /// bases. Zero pins both ends exactly, which is the only behaviour there was. Outward only, so
    /// the stretch asked for is always wholly inside the product; the product is then longer than
    /// the stretch by however far the two ends moved, and each pair says how far that was.
    pub boundary_slack: usize,
}

pub const DEFAULT_PRIMER_CRITERIA: PrimerCriteria = PrimerCriteria {
    target_tm: 60.0,
    max_tm_difference: 3.0,
    min_length: 18,
    max_length: 25,
    min_gc: 40.0,
    max_gc: 60.0,
    prefer_gc_clamp: true,
    max_pairs: 5,
    boundary_slack: 0,
};

/// Shortest stretch of correct sequence a mutagenic primer keeps on each side of the change.
pub const MIN_MUTAGENESIS_FLANK: usize = 10;
/// Longest mutagenic oligo the flanks are grown to. A change needs correct sequence on both
/// sides, so these run longer than an amplifying primer.
pub const MAX_MUTAGENESIS_LENGTH: usize = 45;

/// One primer as designed: the oligo to order, where the part of it that anneals sits, and what
/// the usual checks say about it.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignedPrimer {
    /// The oligo to order, 5'→3', the 5' tail included.
    pub sequence: String,
    /// The 5' tail that is not on the template; empty when there is none.
    pub tail: String,
    /// The part of the oligo that anneals, 5'→3'.
    pub anneal: String,
    /// 0-based inclusive start of the annealing footprint on the plus strand.
    pub start: usize,
    /// 0-based inclusive end of it. `end < start` = through the origin.
    pub end: usize,
    /// The primer reads along the plus strand.
    pub forward: bool,
    /// Melting temperature of the annealing part, °C. `None` when it holds anything but A, C, G, T.
    pub tm: Option<f64>,
    /// GC content of the annealing part, 0 to 100.
    pub gc: f64,
    /// Length of the whole oligo, tail included.
    pub length: usize,
    pub flags: PrimerFlags,
    /// What the oligo folds on itself, over the whole oligo including its tail. Reported, and left
    /// out of the ranking: the warnings `flags` carries are what the score prices in.
    pub hairpin: HairpinCheck,
}

impl DesignedPrimer {
    /// The primer as [`crate::pcr`] takes it, so a design can be run through
    /// [`predict_pcr_product`] without the caller rebuilding it.
    pub fn as_pcr_primer(&self, name: &str) -> PcrPrimer {
        PcrPrimer {
            name: Some(name.to_string()),
            bases: self.sequence.clone(),
            start: self.start as i64,
            end: self.end as i64,
            forward: self.forward,
            tail_length: self.tail.len(),
            ..PcrPrimer::default()
        }
    }
}

/// A forward and a reverse primer that amplify one stretch, with why they are ranked where they are.
#[derive(Clone, Debug, PartialEq)]
pub struct PrimerPair {
    pub forward: DesignedPrimer,
    pub reverse: DesignedPrimer,
    /// How far apart the two annealing temperatures are, °C.
    pub tm_difference: f64,
    /// Lower is better. Distance from the target temperature in °C, plus what the other criteria
    /// cost; it orders the candidates and means nothing on its own.
    pub score: f64,
    /// One line: the numbers, then anything that falls short of the criteria.
    pub reason: String,
    /// Bases the forward primer's 5' end moved outward from the start of the stretch asked for,
    /// and the reverse primer's from its end. Both zero unless
    /// [`PrimerCriteria::boundary_slack`] allowed otherwise.
    pub forward_moved: usize,
    pub reverse_moved: usize,
    /// The duplex the two oligos form with each other, over their whole length, tails included.
    pub dimer: DimerCheck,
    /// Where to start a gradient, by the one rule in [`crate::annealing`]. `None` for a product
    /// too short for that rule, or a primer with no melting temperature.
    pub annealing: Option<AnnealingTemperature>,
    /// What the cross-dimer check and the two hairpin checks found, worded for a person. Empty for
    /// a pair with nothing against it. These are reported, not priced into `score`.
    pub warnings: Vec<String>,
}

/// Why no primers were designed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesignError {
    /// A criterion that cannot be met by any pair, such as a maximum length below the minimum.
    Criteria(String),
    /// The stretch cannot hold two primers of the shortest allowed length.
    RegionTooShort { needed: usize, have: usize },
    /// Candidates were built and every one was refused; the message says what the best of them did.
    NoPair(String),
}

impl std::fmt::Display for DesignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesignError::Criteria(why) => f.write_str(why),
            DesignError::RegionTooShort { needed, have } => write!(
                f,
                "this stretch is {have} bases long and two primers of the shortest allowed length need {needed}; widen the stretch or lower the shortest length"
            ),
            DesignError::NoPair(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for DesignError {}

pub(crate) fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

pub(crate) fn anneal_tm(bases: &str) -> Option<f64> {
    tm(bases, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm)
}

/// `length` bases of a circular-or-linear template from `from`, reading along the plus strand.
/// `None` when a linear template has no such stretch.
fn window(template: &[u8], from: usize, length: usize, circular: bool) -> Option<String> {
    let n = template.len();
    if length == 0 || n == 0 || length > n {
        return None;
    }
    if !circular && from + length > n {
        return None;
    }
    (0..length).map(|k| template.get((from + k) % n).map(|b| char::from(*b))).collect()
}

/// How many bases `start..=end` covers, through the origin when `end < start`.
fn span(start: usize, end: usize, n: usize) -> usize {
    if end >= start { end - start + 1 } else { n.saturating_sub(start) + end + 1 }
}

/// One primer from a footprint already chosen, with its tail.
pub(crate) fn designed(anneal: String, tail: &str, start: usize, end: usize, forward: bool) -> DesignedPrimer {
    let sequence = format!("{tail}{anneal}");
    DesignedPrimer {
        gc: round1(gc_content(&anneal)),
        tm: anneal_tm(&anneal).map(round1),
        flags: primer_flags(&sequence),
        hairpin: hairpins(&sequence),
        // `clean_seq` leaves ASCII letters only, so bytes are bases.
        length: sequence.len(),
        tail: tail.to_string(),
        start,
        end,
        forward,
        anneal,
        sequence,
    }
}

/// How far outside the wanted band a GC figure falls, in percentage points.
fn gc_miss(gc: f64, criteria: &PrimerCriteria) -> f64 {
    (criteria.min_gc - gc).max(gc - criteria.max_gc).max(0.0)
}

/// What [`primer_flags`] says for a primer with no 3' G or C. It is scored and worded here from
/// `PrimerFlags::gc_clamp`, so its own warning is left out rather than counted twice.
const CLAMP_WARNING: &str = "no 3' G/C clamp";

/// The warnings beyond the 3' clamp.
fn other_warnings(flags: &PrimerFlags) -> impl Iterator<Item = &String> {
    flags.warnings.iter().filter(|w| w.as_str() != CLAMP_WARNING)
}

/// What one primer costs: °C off target, plus the other criteria priced in °C-sized penalties.
/// A touch is added for length so that two equally good candidates rank by the shorter oligo.
pub(crate) fn cost(primer: &DesignedPrimer, criteria: &PrimerCriteria) -> Option<f64> {
    let tm = primer.tm?;
    let mut cost = (tm - criteria.target_tm).abs();
    cost += 0.2 * gc_miss(primer.gc, criteria);
    if criteria.prefer_gc_clamp && !primer.flags.gc_clamp {
        cost += 2.0;
    }
    cost += other_warnings(&primer.flags).count() as f64;
    Some(cost + 0.02 * primer.length as f64)
}

/// What falls short of the criteria for one primer, in words.
fn shortfalls(primer: &DesignedPrimer, label: &str, criteria: &PrimerCriteria) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(tm) = primer.tm {
        let off = tm - criteria.target_tm;
        if off.abs() > criteria.max_tm_difference {
            let way = if off > 0.0 { "above" } else { "below" };
            out.push(format!("{label} Tm {tm} °C is {} °C {way} the {} °C target", round1(off.abs()), round1(criteria.target_tm)));
        }
    }
    if gc_miss(primer.gc, criteria) > 0.0 {
        out.push(format!("{label} GC {}% is outside {}–{}%", primer.gc, round1(criteria.min_gc), round1(criteria.max_gc)));
    }
    if criteria.prefer_gc_clamp && !primer.flags.gc_clamp {
        out.push(format!("{label} does not end in G or C"));
    }
    out.extend(other_warnings(&primer.flags).map(|w| format!("{label}: {w}")));
    out
}

/// What one base of outward movement costs, in the same °C-sized units as everything else in
/// [`cost`]. Small, so slack is taken when it buys a better temperature and left alone when it
/// buys nothing: of two pairs that are otherwise equal, the one that amplifies the least extra
/// sequence wins.
const MOVE_COST: f64 = 0.2;

/// The product a pair makes: the forward tail, the template from the forward primer's 5' end
/// through the reverse primer's 5' end, and the reverse tail read back along the plus strand.
fn product_bases(template: &[u8], circular: bool, f_start: usize, r_end: usize, forward_tail: &str, reverse_tail: &str) -> Option<String> {
    let across = span(f_start, r_end, template.len());
    let middle = window(template, f_start, across, circular)?;
    Some(format!("{forward_tail}{middle}{}", revcomp(reverse_tail)))
}

fn pair_reason(forward: &DesignedPrimer, reverse: &DesignedPrimer, difference: f64, criteria: &PrimerCriteria) -> String {
    let shown = |p: &DesignedPrimer| p.tm.map_or_else(|| "?".to_string(), |tm| tm.to_string());
    let head = format!(
        "Tm {}/{} °C (Δ{}), GC {}/{}%, {}/{} bases",
        shown(forward),
        shown(reverse),
        round1(difference),
        forward.gc,
        reverse.gc,
        forward.length,
        reverse.length
    );
    let mut missed = shortfalls(forward, "forward", criteria);
    missed.extend(shortfalls(reverse, "reverse", criteria));
    if missed.is_empty() { format!("{head}; meets every criterion") } else { format!("{head}; {}", missed.join(", ")) }
}

fn check_criteria(criteria: &PrimerCriteria) -> Result<(), DesignError> {
    let bad = |why: &str| Err(DesignError::Criteria(why.to_string()));
    if criteria.min_length < 2 {
        return bad("the shortest primer length must be at least 2 bases");
    }
    if criteria.max_length < criteria.min_length {
        return bad("the longest primer length is below the shortest");
    }
    if criteria.max_gc < criteria.min_gc {
        return bad("the highest GC percentage is below the lowest");
    }
    if !criteria.target_tm.is_finite() || !criteria.max_tm_difference.is_finite() || criteria.max_tm_difference < 0.0 {
        return bad("the target Tm must be a number and the allowed Tm difference a number that is not negative");
    }
    if criteria.max_pairs == 0 {
        return bad("at least one pair has to be returned");
    }
    Ok(())
}

/// Candidate pairs that amplify `start..=end`, the best first.
///
/// The forward primer's 5' end is pinned at `start` and the reverse primer's 5' end at `end`, so
/// every pair's product is the stretch asked for — unless [`PrimerCriteria::boundary_slack`] is
/// set, which lets each 5' end step that many bases OUTWARD, never inward, so the stretch asked
/// for stays whole inside a product that is longer by however far the ends moved.
/// `forward_tail` and `reverse_tail` are 5' bases that are not on the template (a cloning site, an
/// overhang): they are part of the oligo and end up in the product, and are left out of the
/// annealing temperature.
///
/// `Err` when the stretch cannot hold two primers, or when no combination of lengths and allowed
/// movement brings the two temperatures within [`PrimerCriteria::max_tm_difference`]; the message
/// says what the closest candidate did.
pub fn design_primer_pairs(
    template: &str,
    circular: bool,
    start: usize,
    end: usize,
    criteria: &PrimerCriteria,
    forward_tail: &str,
    reverse_tail: &str,
) -> Result<Vec<PrimerPair>, DesignError> {
    check_criteria(criteria)?;
    let bases = clean_seq(template);
    let n = bases.len();
    if n == 0 || start >= n || end >= n {
        return Err(DesignError::Criteria(format!("the stretch must lie inside the construct's {n} bases")));
    }
    if end < start && !circular {
        return Err(DesignError::Criteria("that stretch runs through the origin, but this construct is linear".to_string()));
    }
    let region = span(start, end, n);
    // How far each end may actually move: what was asked for, less what the construct allows. A
    // linear construct has nothing beyond its own ends, and no product may lap a circle.
    let slack = criteria.boundary_slack;
    let (forward_slack, reverse_slack) = if circular {
        // Whatever the two ends move, the product may not lap the circle. The forward end takes
        // what it asked for and the reverse end what is left, which on anything but a stretch
        // covering nearly the whole construct is all of it for both.
        let room = n.saturating_sub(region);
        let forward = slack.min(room);
        (forward, slack.min(room - forward))
    } else {
        (slack.min(start), slack.min(n - 1 - end))
    };
    let have = region + forward_slack + reverse_slack;
    let needed = 2 * criteria.min_length;
    if have < needed {
        return Err(DesignError::RegionTooShort { needed, have });
    }
    let template = bases.as_bytes();
    let (forward_tail, reverse_tail) = (clean_seq(forward_tail), clean_seq(reverse_tail));

    // One primer per length and per position of its 5' end, built once: every combination reuses
    // them. Each is kept with how far outward that 5' end sits.
    let lengths = criteria.min_length..=criteria.max_length.min(have - criteria.min_length);
    let mut forwards: Vec<(DesignedPrimer, usize)> = Vec::new();
    let mut reverses: Vec<(DesignedPrimer, usize)> = Vec::new();
    for length in lengths {
        for moved in 0..=forward_slack {
            let from = (start + n - moved) % n;
            if let Some(anneal) = window(template, from, length, circular) {
                forwards.push((designed(anneal, &forward_tail, from, (from + length - 1) % n, true), moved));
            }
        }
        for moved in 0..=reverse_slack {
            let to = (end + moved) % n;
            let from = (to + n + 1).saturating_sub(length) % n;
            if let Some(top) = window(template, from, length, circular) {
                reverses.push((designed(revcomp(&top), &reverse_tail, from, to, false), moved));
            }
        }
    }

    let mut pairs: Vec<PrimerPair> = Vec::new();
    let mut closest: Option<f64> = None;
    for (f, f_moved) in &forwards {
        for (r, r_moved) in &reverses {
            // A product may not lap the circle, and the two footprints must not overlap: a pair
            // that reads into its partner primes itself as readily as the template.
            let across = region + f_moved + r_moved;
            if across > n || f.anneal.len() + r.anneal.len() > across {
                continue;
            }
            let (Some(f_tm), Some(r_tm)) = (f.tm, r.tm) else {
                continue;
            };
            let difference = (f_tm - r_tm).abs();
            closest = Some(closest.map_or(difference, |best: f64| best.min(difference)));
            if difference > criteria.max_tm_difference {
                continue;
            }
            let (Some(f_cost), Some(r_cost)) = (cost(f, criteria), cost(r, criteria)) else {
                continue;
            };
            pairs.push(PrimerPair {
                reason: pair_reason(f, r, difference, criteria),
                tm_difference: round1(difference),
                score: round1(f_cost + r_cost + 0.5 * difference + MOVE_COST * (f_moved + r_moved) as f64),
                forward: f.clone(),
                reverse: r.clone(),
                forward_moved: *f_moved,
                reverse_moved: *r_moved,
                dimer: DimerCheck::default(),
                annealing: None,
                warnings: Vec::new(),
            });
        }
    }
    if pairs.is_empty() {
        let widen = if slack == 0 {
            ". Both 5' ends are pinned to the ends of the stretch, so the only freedom is length; boundary_slack would let each end move outward to find better bases"
        } else {
            ""
        };
        return Err(DesignError::NoPair(match closest {
            Some(difference) => format!(
                "no pair of {}–{} base primers has melting temperatures within {} °C of each other over this stretch; the closest is {} °C apart{widen}",
                criteria.min_length,
                criteria.max_length,
                round1(criteria.max_tm_difference),
                round1(difference)
            ),
            None => "no melting temperature can be estimated here: the stretch holds bases other than A, C, G and T at one or both ends".to_string(),
        }));
    }
    // Deterministic: by cost, then the pair that moved its ends least, then the shorter forward
    // primer, then the shorter reverse one.
    pairs.sort_by(|a, b| {
        a.score
            .total_cmp(&b.score)
            .then((a.forward_moved + a.reverse_moved).cmp(&(b.forward_moved + b.reverse_moved)))
            .then(a.forward.anneal.len().cmp(&b.forward.anneal.len()))
            .then(a.reverse.anneal.len().cmp(&b.reverse.anneal.len()))
    });
    pairs.truncate(criteria.max_pairs);
    // The checks that only a chosen pair needs: a cross-dimer is a comparison of two whole oligos
    // and an annealing temperature needs the product, so neither is worth doing for thousands of
    // candidates that will not be returned.
    for pair in &mut pairs {
        pair.dimer = cross_dimer(&pair.forward.sequence, &pair.reverse.sequence);
        if let (Some(f_tm), Some(r_tm), Some(product)) =
            (pair.forward.tm, pair.reverse.tm, product_bases(template, circular, pair.forward.start, pair.reverse.end, &forward_tail, &reverse_tail))
        {
            pair.annealing = annealing_temperature(f_tm, r_tm, &product, DNA_TM_DEFAULTS.sodium_mm);
        }
        pair.warnings = pair.dimer.warnings.clone();
        for (primer, label) in [(&pair.forward, "forward"), (&pair.reverse, "reverse")] {
            pair.warnings.extend(primer.hairpin.warnings.iter().map(|w| format!("{label}: {w}")));
        }
    }
    Ok(pairs)
}

/// The product a designed pair makes, through [`predict_pcr_product`], so a design can be checked
/// against the same prediction `simulate_pcr` reports.
pub fn pair_product(template: &str, circular: bool, pair: &PrimerPair) -> PcrPrediction {
    predict_pcr_product(&PcrInput {
        sequence: template.to_string(),
        circular,
        forward: pair.forward.as_pcr_primer("forward primer"),
        reverse: pair.reverse.as_pcr_primer("reverse primer"),
    })
}

#[cfg(test)]
mod clamp_wording {
    use super::CLAMP_WARNING;
    use crate::primer_quality::primer_flags;

    /// The one place the two modules have to agree. If `primer_quality` rewords the clamp warning
    /// and this constant stays behind, every reason counts the missing clamp twice.
    #[test]
    fn the_clamp_warning_is_worded_as_primer_quality_words_it() {
        let no_clamp = primer_flags("GCTTTCCAGTCGGGAAACCTGT");
        assert!(!no_clamp.gc_clamp);
        assert_eq!(no_clamp.warnings, vec![CLAMP_WARNING.to_string()]);
    }
}

/// The change a mutagenic pair installs: `covers` template bases from the position give way to
/// `to`. A substitution covers as many bases as it installs, an insertion covers none, and a
/// deletion installs none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mutagenesis {
    pub covers: usize,
    pub to: String,
}

impl Mutagenesis {
    /// These bases take the place of the same number of bases from the position.
    pub fn substitute(to: &str) -> Mutagenesis {
        let to = clean_seq(to);
        Mutagenesis { covers: to.len(), to }
    }

    /// These bases go in immediately before the position, removing nothing.
    pub fn insert(bases: &str) -> Mutagenesis {
        Mutagenesis { covers: 0, to: clean_seq(bases) }
    }

    /// This many bases from the position onwards go.
    pub fn delete(length: usize) -> Mutagenesis {
        Mutagenesis { covers: length, to: String::new() }
    }

    /// What this is called: `substitution`, `insertion`, `deletion` or `replacement`.
    pub fn kind(&self) -> &'static str {
        match (self.covers, self.to.len()) {
            (0, _) => "insertion",
            (_, 0) => "deletion",
            (covers, installs) if covers == installs => "substitution",
            _ => "replacement",
        }
    }
}

/// A pair that installs one change, and what that change is.
#[derive(Clone, Debug, PartialEq)]
pub struct MutagenesisDesign {
    /// Both primers carry the change; the reverse one is the forward one's reverse complement.
    pub forward: DesignedPrimer,
    pub reverse: DesignedPrimer,
    /// Correct bases kept on the 5' side of the change, and on the 3' side.
    pub flank_5: usize,
    pub flank_3: usize,
    /// Melting temperature of the 3' flank alone, °C: the part that has to pair for the polymerase
    /// to extend in the first round.
    pub flank_3_tm: Option<f64>,
    /// `A1234G`, `insert 6 bases before 1234`, `delete 1234..1239`: 1-based, as a person writes it.
    pub describes: String,
    /// The template bases the change replaces, 1-based inclusive; equal start and end for one
    /// base, and for an insertion the base it goes before with no bases removed.
    pub changed_from: String,
    pub changed_to: String,
    /// The whole construct with the change made: what the pair amplifies.
    pub product: String,
    /// The product's bases around the change, and the template's, to compare by eye.
    pub before_window: String,
    pub after_window: String,
    /// One line: the numbers, then anything that falls short of the criteria.
    pub reason: String,
}

/// Bases either side of the change in the windows that let a person check it by eye.
const WINDOW_FLANK: usize = 15;

/// A pair of mutagenic primers centred on a change at 0-based `at`.
///
/// Both primers span the change with correct template sequence on each side, the forward one
/// 5'→3' along the plus strand and the reverse one its reverse complement, which is how
/// whole-plasmid mutagenesis is primed. The flanks grow a base at a time, the 3' side first, until
/// the oligo's own melting temperature reaches `criteria.target_tm`, keeping at least
/// [`MIN_MUTAGENESIS_FLANK`] correct bases on each side and stopping at
/// [`MAX_MUTAGENESIS_LENGTH`]. A target the longest oligo cannot reach is reported in the reason,
/// not refused: over a GC-poor stretch that is the honest answer.
pub fn design_mutagenesis_primers(
    template: &str,
    circular: bool,
    at: usize,
    change: &Mutagenesis,
    criteria: &PrimerCriteria,
) -> Result<MutagenesisDesign, DesignError> {
    check_criteria(criteria)?;
    let bases = clean_seq(template);
    let n = bases.len();
    let covers = change.covers;
    let installs = clean_seq(&change.to);
    if n == 0 || at >= n {
        return Err(DesignError::Criteria(format!("the position must be inside the construct's {n} bases")));
    }
    if at + covers > n {
        return Err(DesignError::Criteria(
            "the change would run past the last base; it may not run through the origin, so move the origin first".to_string(),
        ));
    }
    if installs.len() != change.to.len() {
        return Err(DesignError::Criteria("the bases to install may only be A, C, G and T: an oligo to order has to be exact".to_string()));
    }
    if covers == 0 && installs.is_empty() {
        return Err(DesignError::Criteria("say what the change is: bases to put in, bases to replace, or how many to remove".to_string()));
    }
    let replaced: String = bases.get(at..at + covers).unwrap_or_default().to_string();
    if replaced == installs {
        return Err(DesignError::Criteria(format!("the construct already reads {installs} there, so there is nothing to change")));
    }
    // Room for the shortest flanks on both sides, plus the bases the change covers.
    let needed = 2 * MIN_MUTAGENESIS_FLANK + covers;
    if n < needed {
        return Err(DesignError::RegionTooShort { needed, have: n });
    }
    if !circular && (at < MIN_MUTAGENESIS_FLANK || at + covers + MIN_MUTAGENESIS_FLANK > n) {
        return Err(DesignError::Criteria(format!(
            "a mutagenic primer needs at least {MIN_MUTAGENESIS_FLANK} correct bases on each side of the change, and this linear construct has fewer on one side"
        )));
    }

    let after = at + covers;
    let left = |length: usize| window(bases.as_bytes(), (at + n - length) % n, length, circular).unwrap_or_default();
    let right = |length: usize| window(bases.as_bytes(), after % n, length, circular).unwrap_or_default();
    // The 3' side grows first: it is the side the polymerase extends from.
    let (mut flank_5, mut flank_3) = (MIN_MUTAGENESIS_FLANK, MIN_MUTAGENESIS_FLANK);
    let oligo = |five: usize, three: usize| format!("{}{installs}{}", left(five), right(three));
    while flank_5 + installs.len() + flank_3 < MAX_MUTAGENESIS_LENGTH {
        if anneal_tm(&oligo(flank_5, flank_3)).is_some_and(|tm| tm >= criteria.target_tm) {
            break;
        }
        // Keep the longest correct run on the 3' side; add there first, then even up.
        if flank_3 <= flank_5 {
            flank_3 += 1;
        } else {
            flank_5 += 1;
        }
        if flank_5 + covers + flank_3 > n {
            break;
        }
    }

    let footprint_start = (at + n - flank_5) % n;
    let footprint_end = (after + flank_3 + n - 1) % n;
    let top = oligo(flank_5, flank_3);
    let forward = designed(top.clone(), "", footprint_start, footprint_end, true);
    let reverse = designed(revcomp(&top), "", footprint_start, footprint_end, false);
    let flank_3_tm = anneal_tm(&right(flank_3)).map(round1);

    let describes = match change.kind() {
        "substitution" if covers == 1 => format!("{replaced}{}{installs}", at + 1),
        "insertion" => format!("insert {} bases ({installs}) before base {}", installs.len(), at + 1),
        "deletion" => format!("delete {}..{} ({replaced})", at + 1, after),
        _ => format!("{}..{} {replaced} → {installs}", at + 1, after),
    };
    let product = format!("{}{installs}{}", bases.get(..at).unwrap_or_default(), bases.get(after..).unwrap_or_default());
    let before_window = format!("{}[{replaced}]{}", left(WINDOW_FLANK), right(WINDOW_FLANK));
    let after_window = format!("{}[{installs}]{}", left(WINDOW_FLANK), right(WINDOW_FLANK));
    // Only a temperature BELOW the target is a fault here. A mutagenic primer is as long as the
    // change and the flanks make it, and one that runs hot anneals all the better; it is the
    // amplifying primers of a pair that have to match each other.
    let mut missed: Vec<String> = Vec::new();
    if let Some(tm) = forward.tm.filter(|tm| criteria.target_tm - tm > criteria.max_tm_difference) {
        missed.push(format!("Tm {tm} °C is {} °C below the {} °C target", round1(criteria.target_tm - tm), round1(criteria.target_tm)));
    }
    if gc_miss(forward.gc, criteria) > 0.0 {
        missed.push(format!("GC {}% is outside {}–{}%", forward.gc, round1(criteria.min_gc), round1(criteria.max_gc)));
    }
    for (primer, label) in [(&forward, "forward"), (&reverse, "reverse")] {
        if criteria.prefer_gc_clamp && !primer.flags.gc_clamp {
            missed.push(format!("the {label} primer does not end in G or C"));
        }
        missed.extend(other_warnings(&primer.flags).map(|w| format!("the {label} primer: {w}")));
    }
    if flank_3 < MIN_MUTAGENESIS_FLANK || flank_5 < MIN_MUTAGENESIS_FLANK {
        missed.push(format!("fewer than {MIN_MUTAGENESIS_FLANK} correct bases on one side"));
    }
    let shown = forward.tm.map_or_else(|| "?".to_string(), |tm| tm.to_string());
    let head = format!(
        "{} bases, Tm {shown} °C, GC {}%, {flank_5} correct bases before the change and {flank_3} after (3' flank Tm {} °C)",
        forward.length,
        forward.gc,
        flank_3_tm.map_or_else(|| "?".to_string(), |tm| tm.to_string())
    );
    let reason = if missed.is_empty() { format!("{head}; meets every criterion") } else { format!("{head}; {}", missed.join(", ")) };

    Ok(MutagenesisDesign {
        forward,
        reverse,
        flank_5,
        flank_3,
        flank_3_tm,
        describes,
        changed_from: replaced,
        changed_to: installs,
        product,
        before_window,
        after_window,
        reason,
    })
}
