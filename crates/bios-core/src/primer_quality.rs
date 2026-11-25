//! Advisory primer-quality signals for an oligo someone already chose: the checks a cloner looks
//! at before ordering. Not a primer designer; it only flags well-known liabilities, so the worst
//! case is a caution that can be ignored.
//!
//! Ported from the web app's `editor/primerQuality.ts`:
//! - 3' GC clamp: a G or C at the 3' end anchors extension; without one the primer is weak.
//! - GC-rich 3' end: four or more of the last five being G/C over-stabilises the end and misprimes.
//! - homopolymer: a run of five or more identical bases slips in synthesis and annealing.
//! - self-complementarity: the longest stretch matching the oligo's own reverse complement, a
//!   proxy for hairpins and self-dimers; six or more is worth a look.
//!
//! Bios adds two checks of its own beyond that port, because the proxy above cannot tell a
//! hairpin from a self-dimer and says nothing at all about a primer's partner:
//!
//! - [`hairpins`]: a real stem-and-loop search inside one oligo — how many base pairs the stem
//!   holds, how many bases the loop leaves, where it sits, and whether it pairs the 3'-terminal
//!   base, which is the one that stops the polymerase extending.
//! - [`cross_dimer`]: the duplex two oligos form with EACH OTHER, which is the commonest reason a
//!   pair fails. Reported twice over: the longest complementary stretch anywhere, and the longest
//!   one that pairs a 3'-terminal base, because only the latter is extended into primer-dimer
//!   product.
//!
//! Both count paired bases. Neither is folding or dimer thermodynamics: there is no ΔG, no loop
//! penalty and no salt, so the worst case is again a caution that can be ignored. The limits at
//! which they speak up are Bios's own, written down beside them.

/// What the checks found. `warnings` is empty for a clean oligo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimerFlags {
    pub gc_clamp: bool,
    pub long_run: usize,
    pub self_comp: usize,
    pub warnings: Vec<String>,
}

const RUN_LIMIT: usize = 5;
const SELF_COMP_LIMIT: usize = 6;

fn complement(b: u8) -> u8 {
    match b {
        b'a' => b't',
        b't' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'u' => b'a',
        other => other,
    }
}

/// The longest run of one base, and where it ends.
fn longest_run(s: &[u8]) -> (usize, usize) {
    let (mut best, mut at, mut run) = (0, 0, 0);
    for (i, b) in s.iter().enumerate() {
        run = if i > 0 && s.get(i - 1) == Some(b) { run + 1 } else { 1 };
        if run > best {
            best = run;
            at = i;
        }
    }
    (best, at)
}

/// The longest common substring of `a` and `b`, O(n·m): fine at oligo length.
fn longest_common_substring(a: &[u8], b: &[u8]) -> usize {
    let mut best = 0;
    let mut prev = vec![0usize; b.len() + 1];
    for x in a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            if x == y {
                let run = prev.get(j).copied().unwrap_or(0) + 1;
                if let Some(slot) = cur.get_mut(j + 1) {
                    *slot = run;
                }
                best = best.max(run);
            }
        }
        prev = cur;
    }
    best
}

/// The flags for `seq`. Letters other than A, C, G, T, U and N are ignored, in either case.
pub fn primer_flags(seq: &str) -> PrimerFlags {
    let seq: Vec<u8> = seq.bytes().map(|b| b.to_ascii_lowercase()).filter(|b| b"acgtun".contains(b)).collect();
    let n = seq.len();
    let mut warnings = Vec::new();
    let gc_clamp = matches!(seq.last(), Some(b'g' | b'c'));
    if n >= 3 && !gc_clamp {
        warnings.push("no 3' G/C clamp".to_string());
    }
    if n >= 5 {
        let tail = seq.get(n - 5..).unwrap_or_default();
        if tail.iter().filter(|b| matches!(b, b'g' | b'c')).count() >= 4 {
            warnings.push("GC-rich 3' end".to_string());
        }
    }
    let (long_run, at) = longest_run(&seq);
    if long_run >= RUN_LIMIT {
        let base = seq.get(at).map_or('N', |b| char::from(b.to_ascii_uppercase()));
        warnings.push(format!("{long_run}× {base} run"));
    }
    let reverse: Vec<u8> = seq.iter().rev().map(|b| complement(*b)).collect();
    let self_comp = longest_common_substring(&seq, &reverse);
    if self_comp >= SELF_COMP_LIMIT {
        warnings.push(format!("{self_comp}-bp self-complementarity"));
    }
    PrimerFlags { gc_clamp, long_run, self_comp, warnings }
}

/// Shortest loop a hairpin is credited with: three unpaired bases is the tightest turn a DNA
/// strand makes, so a "stem" enclosing one or two bases is not a structure.
pub const MIN_HAIRPIN_LOOP: usize = 3;
/// Fewest base pairs recorded as a hairpin at all. One or two pairs holding a loop is not a
/// structure, and reporting every such pair would bury the hairpins that matter.
pub const MIN_HAIRPIN_STEM: usize = 3;
/// A stem of this many base pairs anywhere in the oligo is worth a look.
pub const HAIRPIN_STEM_LIMIT: usize = 5;
/// A stem that pairs the 3'-terminal base is held to this much shorter count. A hairpin there does
/// not merely compete with the template: it folds the end the polymerase has to extend from.
pub const HAIRPIN_3_PRIME_STEM_LIMIT: usize = 3;
/// Complementary bases between two oligos, anywhere, worth a look. The same count as
/// [`SELF_COMP_LIMIT`], so one number means "a duplex worth a look" throughout Bios.
pub const DIMER_LIMIT: usize = SELF_COMP_LIMIT;
/// Complementary bases that pair a 3'-terminal base. Stricter, for the same reason the hairpin
/// rule is: this duplex is the one a polymerase extends, and extending it makes primer-dimer
/// product instead of the product wanted.
pub const DIMER_3_PRIME_LIMIT: usize = 4;

/// One hairpin: a stem of `stem` base pairs holding a loop of `loop_length` unpaired bases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hairpin {
    /// Base pairs in the stem.
    pub stem: usize,
    /// Unpaired bases the stem encloses, at least [`MIN_HAIRPIN_LOOP`].
    pub loop_length: usize,
    /// The stem's 5' arm, 0-based inclusive into the oligo's letters.
    pub stem_5: (usize, usize),
    /// Its 3' arm, which pairs the 5' arm back to front.
    pub stem_3: (usize, usize),
    /// The stem pairs the oligo's 3'-terminal base.
    pub at_three_prime: bool,
    /// The oligo with the two arms in brackets and the loop between them, as
    /// `AAAAA[GGCGG]TTT[CCGCC]AA`.
    pub shape: String,
}

/// What [`hairpins`] found. Both are `None` for an oligo that folds on nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HairpinCheck {
    /// The sturdiest hairpin anywhere in the oligo: the longest stem, the tightest loop of those.
    pub worst: Option<Hairpin>,
    /// The sturdiest one whose stem pairs the 3'-terminal base, when one does. It may be the same
    /// hairpin as `worst`, a different and shorter one, or nothing at all.
    pub three_prime: Option<Hairpin>,
    /// Empty when neither limit is reached.
    pub warnings: Vec<String>,
}

/// The duplex two oligos form with each other.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DimerCheck {
    /// The longest run of consecutive complementary bases between the two, in any register.
    pub longest: usize,
    /// The longest such run that pairs the FIRST oligo's 3'-terminal base.
    pub three_prime_first: usize,
    /// The longest such run that pairs the SECOND oligo's 3'-terminal base.
    pub three_prime_second: usize,
    /// Three lines to look at: the first oligo 5'→3', a bar under each base that pairs, and the
    /// second oligo 3'→5' beneath, with a bar wherever the two bases pair in that register, not
    /// only inside the longest run. It shows the register the warnings are about: the longest
    /// 3'-anchored run once one is long enough to warn about, else simply the longest run. The
    /// three counts above are each the largest over every register and need not come from this
    /// one.
    pub diagram: String,
    /// Empty when neither limit is reached.
    pub warnings: Vec<String>,
}

/// Lowercase A, C, G and T of `seq` with U read as T, keeping N in place so that a position means
/// the same thing it does in [`primer_flags`]. Everything that is not one of those letters is
/// dropped.
fn oligo_bases(seq: &str) -> Vec<u8> {
    seq.bytes().map(|b| b.to_ascii_lowercase()).filter(|b| b"acgtun".contains(b)).map(|b| if b == b'u' { b't' } else { b }).collect()
}

/// Watson-Crick only. An N pairs with nothing: an unknown base must never be made into a duplex.
fn paired(x: u8, y: u8) -> bool {
    matches!((x, y), (b'a', b't') | (b't', b'a') | (b'c', b'g') | (b'g', b'c'))
}

/// Every antiparallel register of `a` against `b`, as `each(i, j, run)` at each position where
/// `a[i]` pairs with `b[j]`. `run` is how many consecutive pairs end there counting from the
/// outside in: …, (i-2, j+2), (i-1, j+1), (i, j). So the duplex `run` names covers
/// `a[i+1-run..=i]` against `b[j..=j+run-1]`.
///
/// One walk serves both searches here. A hairpin is this with `a` and `b` the same oligo and the
/// arms kept apart by a loop; a cross-dimer is this with the two oligos of a pair.
fn antiparallel_runs(a: &[u8], b: &[u8], mut each: impl FnMut(usize, usize, usize)) {
    let (m, n) = (a.len(), b.len());
    if m == 0 || n == 0 {
        return;
    }
    for total in 0..m + n - 1 {
        // `j` has to stay inside `b`, so `i` starts where `total - i` last does.
        let mut run = 0usize;
        for i in total.saturating_sub(n - 1)..m.min(total + 1) {
            let (Some(&x), Some(&y)) = (a.get(i), b.get(total - i)) else {
                break;
            };
            run = if paired(x, y) { run + 1 } else { 0 };
            if run > 0 {
                each(i, total - i, run);
            }
        }
    }
}

/// Sturdier of two hairpins: more base pairs first, then the tighter loop, then the earlier arm so
/// that one oligo always gives one answer.
fn sturdier(candidate: &Hairpin, held: &Hairpin) -> bool {
    (candidate.stem, held.loop_length, held.stem_5.0) > (held.stem, candidate.loop_length, candidate.stem_5.0)
}

/// The hairpins `seq` can fold into: both arms of the stem inside the one oligo, a loop of at least
/// [`MIN_HAIRPIN_LOOP`] bases between them, and only A-T and C-G pairing.
///
/// A count of base pairs, not a fold: no stacking energy, no loop penalty, no salt. A wide loop
/// closed by a short stem is reported with the same weight as a tight one, which overstates it, so
/// read `loop_length` before acting on `stem`.
pub fn hairpins(seq: &str) -> HairpinCheck {
    let s = oligo_bases(seq);
    let n = s.len();
    let upper: String = s.iter().map(|b| char::from(b.to_ascii_uppercase())).collect();
    let mut found = HairpinCheck::default();
    antiparallel_runs(&s, &s, |i, j, stem| {
        // The 3' arm has to start after the 5' arm with a real loop between them. The pair walked
        // here is the innermost one, so `j - i - 1` is the loop it closes.
        let Some(loop_length) = j.checked_sub(i + 1) else {
            return;
        };
        if loop_length < MIN_HAIRPIN_LOOP || stem < MIN_HAIRPIN_STEM {
            return;
        }
        let (Some(arm_5), Some(arm_3)) = (i.checked_sub(stem - 1), j.checked_add(stem - 1)) else {
            return;
        };
        let part = |from: usize, to: usize| upper.get(from..to).unwrap_or_default();
        let hairpin = Hairpin {
            stem,
            loop_length,
            stem_5: (arm_5, i),
            stem_3: (j, arm_3),
            at_three_prime: arm_3 + 1 == n,
            shape: format!("{}[{}]{}[{}]{}", part(0, arm_5), part(arm_5, i + 1), part(i + 1, j), part(j, arm_3 + 1), part(arm_3 + 1, n)),
        };
        if found.worst.as_ref().is_none_or(|held| sturdier(&hairpin, held)) {
            found.worst = Some(hairpin.clone());
        }
        if hairpin.at_three_prime && found.three_prime.as_ref().is_none_or(|held| sturdier(&hairpin, held)) {
            found.three_prime = Some(hairpin);
        }
    });
    if let Some(worst) = found.worst.as_ref().filter(|h| h.stem >= HAIRPIN_STEM_LIMIT) {
        found.warnings.push(format!("{}-bp hairpin stem round a {}-base loop", worst.stem, worst.loop_length));
    }
    if let Some(end) = found.three_prime.as_ref().filter(|h| h.stem >= HAIRPIN_3_PRIME_STEM_LIMIT) {
        found.warnings.push(format!("hairpin pairs the 3' end: {}-bp stem round a {}-base loop", end.stem, end.loop_length));
    }
    found
}

/// Three lines showing one register of two oligos: `a` 5'→3' over `b` 3'→5', with a bar under
/// every base that pairs. `total` is `i + j` of a pair in that register, which is what fixes the
/// offset between the two.
fn dimer_diagram(a: &[u8], b: &[u8], total: usize) -> String {
    let (m, n) = (a.len(), b.len());
    // `a[i]` and `b[total - i]` share a column, so the two strings sit `n - 1 - total` apart.
    let last = n.saturating_sub(1);
    let (pad_a, pad_b) = (last.saturating_sub(total), total.saturating_sub(last));
    let letters = |s: &[u8]| -> String { s.iter().map(|base| char::from(base.to_ascii_uppercase())).collect() };
    let mut bars = String::new();
    for column in 0..(pad_a + m).max(pad_b + n) {
        let pairs_here =
            column.checked_sub(pad_a).and_then(|i| Some((*a.get(i)?, *b.get(total.checked_sub(i)?)?))).is_some_and(|(x, y)| paired(x, y));
        bars.push(if pairs_here { '|' } else { ' ' });
    }
    let reversed: Vec<u8> = b.iter().rev().copied().collect();
    format!("5'-{}{}-3'\n   {}\n3'-{}{}-5'", " ".repeat(pad_a), letters(a), bars.trim_end(), " ".repeat(pad_b), letters(&reversed))
}

/// The duplex `first` and `second` form with each other: the cross-dimer check for the two primers
/// of a pair, or for any oligo against any other.
///
/// Both are read 5'→3'. The two are slid past each other in every antiparallel register and the
/// longest run of consecutive A-T and C-G pairs taken: overall, and separately for each oligo's
/// 3'-terminal base. Hand an oligo to itself and the answer is its self-dimer.
///
/// A count of base pairs, not thermodynamics: no ΔG and no salt, and a run broken by one mismatch
/// counts as two runs even where the duplex would in fact hold straight through.
pub fn cross_dimer(first: &str, second: &str) -> DimerCheck {
    let (a, b) = (oligo_bases(first), oligo_bases(second));
    let (m, n) = (a.len(), b.len());
    let mut check = DimerCheck::default();
    // The register the diagram will show, ranked the way the warnings are: a 3'-anchored duplex
    // long enough to warn about first, and the longest of those, else simply the longest. A single
    // base pair that happens to sit on a 3' end is not worth drawing over a real duplex.
    let mut shown: Option<(usize, usize, usize)> = None;
    antiparallel_runs(&a, &b, |i, j, run| {
        check.longest = check.longest.max(run);
        let at_first = i + 1 == m;
        let at_second = j + run == n;
        if at_first {
            check.three_prime_first = check.three_prime_first.max(run);
        }
        if at_second {
            check.three_prime_second = check.three_prime_second.max(run);
        }
        let rank = (usize::from((at_first || at_second) && run >= DIMER_3_PRIME_LIMIT), run);
        if shown.is_none_or(|(held_rank, held_run, _)| (held_rank, held_run) < rank) {
            shown = Some((rank.0, run, i + j));
        }
    });
    if let Some((_, _, total)) = shown {
        check.diagram = dimer_diagram(&a, &b, total);
    }
    if check.longest >= DIMER_LIMIT {
        check.warnings.push(format!("{}-bp cross-complementarity between the two", check.longest));
    }
    let three_prime = check.three_prime_first.max(check.three_prime_second);
    if three_prime >= DIMER_3_PRIME_LIMIT {
        let whose = match (check.three_prime_first >= DIMER_3_PRIME_LIMIT, check.three_prime_second >= DIMER_3_PRIME_LIMIT) {
            (true, true) => "both 3' ends",
            (true, false) => "the first oligo's 3' end",
            _ => "the second oligo's 3' end",
        };
        check.warnings.push(format!("{three_prime}-bp dimer on {whose}, which the polymerase extends"));
    }
    check
}
