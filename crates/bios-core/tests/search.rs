//! Binding-site search: strand, origin-wrap and mismatch-offset correctness, ported from the web
//! app's `search.test.ts` in its order and with its numbers.
//!
//! These are the three ways this function can be silently wrong: draw on the wrong strand, lose
//! an origin-spanning site, or point at the wrong base of the user's oligo. None of them fail;
//! each just renders a confident lie. So each gets a test with hand-verified expectations rather
//! than a round-trip.

use bios_core::search::{BindingSite, SearchOptions, find_binding_sites};
use bios_core::seq::revcomp;

/// "ACGGT" sits at 3..7. Deliberately not a palindrome (revcomp is "ACCGT"), so a forward and a
/// reverse hit can never be confused for one another.
const LINEAR: &str = "AAAACGGTTTTT";

/// t[10..11] + t[0..2] spells ACGGT: a site that exists only if we wrap.
const CIRCLE: &str = "GGTAAAAAAAAC";

fn linear() -> SearchOptions {
    SearchOptions { circular: false, ..Default::default() }
}

fn circular() -> SearchOptions {
    SearchOptions { circular: true, ..Default::default() }
}

// strand

#[test]
fn an_oligo_matching_the_plus_strand_binds_the_minus_strand_forward() {
    let sites = find_binding_sites(LINEAR, "ACGGT", linear());
    assert_eq!(sites, vec![BindingSite { start: 3, end: 7, forward: true, mismatches: 0, mismatch_positions: vec![], wraps: false }]);
}

#[test]
fn an_oligo_whose_revcomp_matches_the_plus_strand_binds_it_not_forward_the_aso_case() {
    let sites = find_binding_sites(LINEAR, "ACCGT", linear());
    assert_eq!(sites.len(), 1);
    assert!(!sites[0].forward);
    assert_eq!(sites[0].start, 3);
    assert_eq!(sites[0].end, 7);
    // Sanity: it binds there precisely because its revcomp is what's written at 3..7.
    assert_eq!(revcomp("ACCGT"), &LINEAR[3..8]);
}

#[test]
fn a_palindromic_oligo_honestly_reports_both_strands_at_one_site() {
    let sites = find_binding_sites("AAAGGATCCTTT", "GGATCC", linear());
    assert_eq!(sites.len(), 2);
    assert_eq!(sites.iter().map(|s| s.forward).collect::<Vec<_>>(), [true, false]);
    assert!(sites.iter().all(|s| s.start == 3 && s.end == 8));
}

// mismatches

#[test]
fn max_mismatches_0_rejects_a_single_base_mismatch() {
    assert_eq!(find_binding_sites("AAAACAGTTTTT", "ACGGT", linear()), []);
}

#[test]
fn a_forward_mismatch_is_reported_at_its_offset_in_the_oligo() {
    let hits: Vec<BindingSite> =
        find_binding_sites("AAAACAGTTTTT", "ACGGT", SearchOptions { circular: false, max_mismatches: 1 }).into_iter().filter(|s| s.forward).collect();
    assert_eq!(hits.len(), 1);
    // oligo   ACGGT
    // target  ACAGT  → the oligo's index-2 G has no partner.
    assert_eq!(hits[0].mismatches, 1);
    assert_eq!(hits[0].mismatch_positions, [2]);
}

#[test]
fn a_reverse_mismatch_is_reported_against_the_oligo_not_the_matched_strand() {
    // Target carries TCGGT at 3..7. revcomp("ACCGT") is ACGGT, so the mismatch falls at window
    // offset 0, but that is the oligo's LAST base (index 4), and that is what the user is looking
    // at. Reporting 0 here would be the silent lie.
    let hits: Vec<BindingSite> = find_binding_sites("AAATCGGTTTTT", "ACCGT", SearchOptions { circular: false, max_mismatches: 1 })
        .into_iter()
        .filter(|s| !s.forward)
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start, 3);
    assert_eq!(hits[0].mismatches, 1);
    assert_eq!(hits[0].mismatch_positions, [4]);
}

#[test]
fn an_ambiguity_code_in_the_target_is_a_mismatch_never_a_free_pass() {
    // A run of Ns is a genomic gap. An oligo must not be reported as binding it.
    assert_eq!(find_binding_sites("AAANNNNNTTTT", "NNNNN", linear()), []);
    assert_eq!(find_binding_sites("AAAACNGTTTTT", "ACGGT", linear()), []);
}

#[test]
fn two_reverse_mismatches_come_back_ascending_not_in_matched_strand_order() {
    // With a single mismatch the reversal in the offset remap is a no-op, so it can only be wrong
    // once there are two. Target carries TCGCT at 3..7; revcomp("ACCGT") is ACGGT, which
    // mismatches it at window offsets 0 and 3, i.e. oligo indices 4 and 1. Un-reversed this would
    // report [4, 1] and the UI would highlight them out of order.
    let hits: Vec<BindingSite> = find_binding_sites("AAATCGCTTTTT", "ACCGT", SearchOptions { circular: false, max_mismatches: 2 })
        .into_iter()
        .filter(|s| !s.forward)
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start, 3);
    assert_eq!(hits[0].mismatches, 2);
    assert_eq!(hits[0].mismatch_positions, [1, 4]);
}

// multiple hits

#[test]
fn sites_come_back_ascending_by_start() {
    // ACGGT at 0..4 and 7..11; revcomp is ACCGT, which is absent, so these are the only two.
    let sites = find_binding_sites("ACGGTAAACGGTAA", "ACGGT", linear());
    assert_eq!(sites.iter().map(|s| s.start).collect::<Vec<_>>(), [0, 7]);
    assert!(sites.iter().all(|s| s.forward));
}

// origin

#[test]
fn a_circular_target_reports_a_site_crossing_the_origin_end_lt_start() {
    let sites = find_binding_sites(CIRCLE, "ACGGT", circular());
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].start, 10);
    assert_eq!(sites[0].end, 2); // origin-spanning, as features are, not collapsed
    assert!(sites[0].wraps);
    assert!(sites[0].forward);
}

#[test]
fn the_same_target_searched_as_linear_loses_it_which_is_why_circular_has_no_default() {
    assert_eq!(find_binding_sites(CIRCLE, "ACGGT", linear()), []);
}

#[test]
fn a_wrap_hit_is_not_double_reported_from_the_extension() {
    let sites = find_binding_sites("ACGGTAAAAAAA", "ACGGT", circular());
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].start, 0);
    assert!(!sites[0].wraps);
}

// degenerate input

#[test]
fn an_oligo_longer_than_the_target_binds_nothing() {
    assert_eq!(find_binding_sites("ACGT", "ACGTACGT", circular()), []);
}

#[test]
fn an_empty_oligo_binds_nothing() {
    assert_eq!(find_binding_sites(LINEAR, "", linear()), []);
}

#[test]
fn formatting_in_either_argument_is_ignored() {
    let hits = find_binding_sites("aaa acg gtt ttt", "acg-gt", linear());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start, 3);
}
