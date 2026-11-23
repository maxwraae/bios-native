//! Primer search: tails, the 3' anchor, and Tm scope, ported from the web app's `primer.test.ts`
//! in its order and with its numbers.
//!
//! Same discipline as the binding-site search's tests: every test here is named for a specific
//! way this function can be CONFIDENTLY WRONG rather than fail. A primer search that returns
//! nothing for a working Gibson primer, or reports the Tm of an oligo that isn't what anneals,
//! doesn't crash: it sends someone to the bench with a reaction that won't fire. So the
//! expectations are hand-verified against sequences small enough to check by eye.

use bios_core::primer::{PrimerSearchOptions, PrimerSite, find_primer_sites};
use bios_core::search::{SearchOptions, find_binding_sites};
use bios_core::seq::{DNA_TM_DEFAULTS, revcomp, tm};

/// 40nt. Positions 10..29 are the anneal target; flanks are homopolymer so nothing binds there
/// by accident. Not palindromic, so forward and reverse hits can't be confused.
const TEMPLATE: &str = "GGGGGGGGGGATGCGTACGTTGACCGATCATTTTTTTTTT";

/// ATGCGTACGTTGACCGATCA, 20nt.
fn anneal() -> &'static str {
    &TEMPLATE[10..30]
}

/// Deliberate non-homology: this is the Gibson overlap / restriction site / tag case. 18nt.
const TAIL: &str = "CCCCAAAACCCCAAAACC";

fn opts() -> PrimerSearchOptions {
    PrimerSearchOptions { circular: false, ..Default::default() }
}

fn sites(target: &str, primer: &str, options: PrimerSearchOptions) -> Vec<PrimerSite> {
    find_primer_sites(target, primer, options).expect("the settings are valid")
}

/// The first site, as the web app's `const [site] = ...`.
fn first(target: &str, primer: &str, options: PrimerSearchOptions) -> PrimerSite {
    sites(target, primer, options).into_iter().next().expect("a site")
}

/// `tm(seq)` of the web app: the default conditions.
fn default_tm(seq: &str) -> Option<f64> {
    tm(seq, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm)
}

/// `ANNEAL` with base 5 replaced by A: one base off, mid-anneal.
fn mutant() -> String {
    [&anneal()[..5], "A", &anneal()[6..]].concat()
}

/// Break base `i` of a sequence (0-based, 5'→3').
fn flip(s: &str, i: usize) -> String {
    [&s[..i], if &s[i..=i] == "G" { "A" } else { "G" }, &s[i + 1..]].concat()
}

fn last() -> usize {
    anneal().len() - 1
}

// the 5' tail: the case find_binding_sites cannot answer

#[test]
fn a_tailed_primer_binds_the_same_primer_asked_of_find_binding_sites_binds_nowhere() {
    // This is the whole reason this module exists. The tail is 18 real, ordered bases that don't
    // match, so a full-length Hamming window scores 18 mismatches and reports NO SITE, while at
    // the bench the primer works fine.
    let primer = [TAIL, anneal()].concat();
    assert_eq!(find_binding_sites(TEMPLATE, &primer, SearchOptions { circular: false, max_mismatches: 3 }), []);

    let found = sites(TEMPLATE, &primer, opts());
    assert_eq!(found.len(), 1);
    let site = &found[0];
    assert_eq!(site.start, 10);
    assert_eq!(site.end, 29);
    assert!(site.forward);
    assert_eq!(site.tail_length, 18);
    assert_eq!(site.anneal_length, 20);
    assert_eq!(site.mismatches, 0);
}

#[test]
fn the_tail_is_not_counted_as_mismatches_it_is_not_on_the_template_at_all() {
    let site = first(TEMPLATE, &[TAIL, anneal()].concat(), opts());
    // 18 unmatched bases, zero mismatches. A tail base has no partner to mismatch.
    assert_eq!(site.mismatches, 0);
    assert_eq!(site.mismatch_positions, []);
}

#[test]
fn the_drawn_footprint_is_the_anneal_not_the_oligo_a_tailed_site_is_shorter_than_the_primer() {
    let primer = [TAIL, anneal()].concat();
    let site = first(TEMPLATE, &primer, opts());
    let footprint = site.end - site.start + 1;
    assert_eq!(footprint, 20);
    assert_eq!(footprint, primer.len() - site.tail_length);
    // The tail is recoverable by the caller as a plain prefix: always 5', always contiguous.
    assert_eq!(&primer[..site.tail_length], TAIL);
}

#[test]
fn a_fully_annealing_primer_reports_no_tail() {
    let site = first(TEMPLATE, anneal(), opts());
    assert_eq!(site.tail_length, 0);
    assert_eq!(site.anneal_length, 20);
}

#[test]
fn a_tailed_reverse_primer_lands_on_the_same_footprint_not_shifted_by_the_tail() {
    // The reverse anchor is the site's START, not its end. Getting that backwards draws every
    // reverse site anneal_length bases away from where it actually binds.
    let found = sites(TEMPLATE, &["AAGGCCTTAAGG", &revcomp(anneal())].concat(), opts());
    assert_eq!(found.len(), 1);
    let site = &found[0];
    assert!(!site.forward);
    assert_eq!(site.start, 10);
    assert_eq!(site.end, 29);
    assert_eq!(site.tail_length, 12);
    assert_eq!(site.anneal_length, 20);
}

// Tm is of the annealing portion only

#[test]
fn a_tailed_primers_tm_is_the_anneals_not_the_whole_oligos() {
    let primer = [TAIL, anneal()].concat();
    let site = first(TEMPLATE, &primer, opts());
    // The tailed-primer error in one assertion: the oligo is 38nt and scores ~70°C, but only 20nt
    // of it anneals in cycle 1 and that duplex melts ~56°C. Anneal at the former and the reaction
    // never fires.
    assert_eq!(site.tm, default_tm(anneal()));
    assert_ne!(site.tm, default_tm(&primer));
    assert!(default_tm(&primer).expect("a Tm") - site.tm.expect("a Tm") > 10.0, "the whole-oligo Tm should be badly hotter");
}

#[test]
fn a_mismatched_duplex_does_not_receive_a_perfect_match_tm() {
    // The primer is the molecule in the tube. With a mismatch the two strands differ, and it's
    // the ordered oligo whose Tm you set the block to.
    let site = first(TEMPLATE, &mutant(), PrimerSearchOptions { max_mismatches: 1, ..opts() });
    assert_eq!(site.mismatches, 1);
    assert_eq!(site.tm, None);
}

// the 3' anchor: SnapGene's actual acceptance rule

#[test]
fn a_3_prime_terminal_mismatch_is_not_a_site_that_is_how_allele_specific_pcr_works() {
    // A polymerase extends from the 3'-OH. Unpaired there, nothing happens: designing exactly
    // this primer is how you make it NOT amplify. Reporting a site would invert the user's
    // intent. SnapGene gates 3'-mismatched sites behind a separate, off-by-default option, which
    // is why the main rule here refuses them.
    assert_eq!(sites(TEMPLATE, &flip(anneal(), last()), opts()), []);
}

#[test]
fn one_isolated_mismatch_inside_the_anchor_is_still_a_site_snapgene_allows_it() {
    // "Match at least N bases at the 3' end ... allowing a single isolated mismatch." Break a
    // base mid-anchor (the default anchor is the 3'-most 10). Its neighbours pair, so it is
    // isolated and the site survives.
    let i = last() - 4;
    let found = sites(TEMPLATE, &flip(anneal(), i), opts());
    assert_eq!(found.len(), 1);
    let site = &found[0];
    assert_eq!(site.start, 10);
    assert_eq!(site.mismatches, 1);
    assert_eq!(site.mismatch_positions, [i]);
}

#[test]
fn the_same_mismatch_is_not_a_site_once_the_isolated_mismatch_allowance_is_off() {
    // SnapGene unchecks this for large sequences; the option has to actually bite.
    let i = last() - 4;
    assert_eq!(sites(TEMPLATE, &flip(anneal(), i), PrimerSearchOptions { allow_isolated_mismatch: false, ..opts() }), []);
}

#[test]
fn two_adjacent_mismatches_in_the_anchor_are_not_a_site_isolated_has_to_mean_something() {
    // The allowance is for ONE isolated blip. Adjacent mismatches are the leading edge of a tail,
    // not a blip, and this is the test that tells the two apart.
    let dead = flip(&flip(anneal(), last() - 4), last() - 5);
    assert_eq!(sites(TEMPLATE, &dead, opts()), []);
}

#[test]
fn two_separated_mismatches_in_the_anchor_are_not_a_site_either_the_allowance_is_one() {
    let dead = flip(&flip(anneal(), last() - 3), last() - 7);
    assert_eq!(sites(TEMPLATE, &dead, opts()), []);
}

#[test]
fn a_mismatch_outside_the_anchor_is_a_site_a_mutagenesis_primer_still_primes() {
    // Beyond the anchor SnapGene imposes no positional rule, which is exactly why a
    // central-mismatch mutagenic primer keeps a full-length binding site.
    let i = 5; // well 5' of the 3'-most 10
    let site = first(TEMPLATE, &flip(anneal(), i), opts());
    assert_eq!(site.start, 10);
    assert_eq!(site.anneal_length, 20);
    assert_eq!(site.mismatch_positions, [i]);
}

// the Tm floor: why no length cutoff is needed

#[test]
fn an_anneal_that_melts_below_the_floor_is_not_a_site() {
    // The 3'-most 10 bases pair perfectly, so the anchor rule passes. The site is rejected on
    // thermodynamics alone: a 10nt duplex doesn't prime at PCR temperatures.
    let short = &anneal()[anneal().len() - 10..];
    let site = first(TEMPLATE, short, PrimerSearchOptions { min_tm: 0.0, ..opts() });
    assert_eq!(site.anneal_length, 10, "the anchor rule alone would accept it");
    assert!(site.tm.expect("a Tm") < 40.0, "expected a weak duplex, got {:?}", site.tm);
    // ...and at SnapGene's default floor it correctly disappears.
    assert_eq!(sites(TEMPLATE, short, opts()), []);
}

#[test]
fn raising_the_floor_above_a_real_sites_tm_drops_it() {
    let site = first(TEMPLATE, anneal(), opts());
    assert!(site.tm.expect("a Tm") > 50.0);
    assert_eq!(sites(TEMPLATE, anneal(), PrimerSearchOptions { min_tm: 70.0, ..opts() }), []);
}

// mismatch positions index the user's primer, on both strands

#[test]
fn a_forward_mismatch_is_reported_at_its_offset_in_the_primer_tail_included() {
    let primer = [TAIL, &mutant()].concat();
    let site = first(TEMPLATE, &primer, PrimerSearchOptions { max_mismatches: 1, ..opts() });
    // The broken base is at index 5 of the anneal, so index 18+5=23 of the oligo the user is
    // looking at. Reporting 5 here would point at a tail base.
    assert_eq!(site.mismatch_positions, [TAIL.len() + 5]);
    assert_eq!(&primer[TAIL.len() + 5..=TAIL.len() + 5], "A");
}

#[test]
fn a_reverse_mismatch_is_reported_against_the_primer_not_the_matched_strand() {
    // Same trap as the binding-site search: offsets must be into the user's oligo 5'→3'.
    let rc = revcomp(anneal());
    let site = first(TEMPLATE, &flip(&rc, 4), PrimerSearchOptions { max_mismatches: 1, ..opts() });
    assert!(!site.forward);
    assert_eq!(site.mismatches, 1);
    assert_eq!(site.mismatch_positions, [4]);
}

#[test]
fn mismatch_positions_come_back_ascending() {
    let rc = revcomp(anneal());
    let mutated = flip(&flip(&rc, 4), 8);
    let site = first(TEMPLATE, &mutated, PrimerSearchOptions { max_mismatches: 2, ..opts() });
    assert_eq!(site.mismatches, 2);
    assert_eq!(site.mismatch_positions, [4, 8]);
}

// origin

/// 40nt circle. The anneal below exists only if the search wraps.
const CIRCLE: &str = "ATGCGTACGTTGACCGATCAGGGGTTTTCCCCAAAAGGTT";

/// AAAAGGTT|ATGCGTAC, 16nt.
fn spanning() -> String {
    [&CIRCLE[32..], &CIRCLE[..8]].concat()
}

#[test]
fn a_circular_target_reports_an_origin_spanning_anneal_end_lt_start() {
    let found = sites(CIRCLE, &spanning(), PrimerSearchOptions { circular: true, ..Default::default() });
    assert_eq!(found.len(), 1);
    let site = &found[0];
    assert_eq!(site.start, 32);
    assert_eq!(site.end, 7);
    assert!(site.end < site.start, "origin-spanning, as features are, not collapsed");
    assert!(site.wraps);
    assert_eq!(site.anneal_length, 16);
}

#[test]
fn the_same_primer_searched_as_linear_loses_it_which_is_why_circular_has_no_default() {
    assert_eq!(sites(CIRCLE, &spanning(), PrimerSearchOptions { circular: false, ..Default::default() }), []);
}

#[test]
fn a_wrapping_site_is_not_double_reported_from_the_modular_walk() {
    let found = sites(CIRCLE, &CIRCLE[..16], PrimerSearchOptions { circular: true, ..Default::default() });
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].start, 0);
    assert!(!found[0].wraps);
}

// degenerate input

#[test]
fn an_ambiguity_code_in_the_template_is_a_mismatch_never_a_free_pass() {
    // Same conservatism as the binding-site search: a primer must not be reported as priming a
    // run of Ns across a genomic gap.
    let gapped = [&TEMPLATE[..10], &"N".repeat(20), &TEMPLATE[30..]].concat();
    assert_eq!(sites(&gapped, anneal(), PrimerSearchOptions { max_mismatches: 3, ..opts() }), []);
}

#[test]
fn a_primer_shorter_than_the_3_prime_anchor_binds_nothing_there_is_no_anchor_to_test() {
    assert_eq!(sites(TEMPLATE, &anneal()[anneal().len() - 6..], opts()), []);
}

#[test]
fn an_empty_primer_binds_nothing() {
    assert_eq!(sites(TEMPLATE, "", opts()), []);
}

#[test]
fn an_all_tail_primer_with_no_anneal_binds_nothing() {
    assert_eq!(sites(TEMPLATE, "CCCCAAAACCCCAAAACC", opts()), []);
}

#[test]
fn formatting_in_either_argument_is_ignored() {
    // A space after every fourth base, in lower case: "atgc gtac gttg accg atca ".
    let spaced: String = anneal().as_bytes().chunks(4).map(|four| format!("{} ", String::from_utf8_lossy(four))).collect::<String>().to_lowercase();
    let site = first(TEMPLATE, &spaced, opts());
    assert_eq!(site.start, 10);
}
