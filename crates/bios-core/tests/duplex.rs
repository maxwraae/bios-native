//! Duplex runs: which bases pair, and which column each one faces, ported from the web app's
//! `duplex.test.ts` in its order and with its numbers.
//!
//! Same discipline as the primer search's tests: every test is named for a specific way this can
//! be CONFIDENTLY WRONG rather than fail. This function feeds a drawing, and a drawing that is
//! mirrored, shifted by one, or missing its tail looks entirely plausible on screen. The reverse
//! strand is the whole risk: get the step backwards and every reverse primer's tail is drawn on
//! the wrong side, silently, forever. So the fixtures are small enough to check by eye and the
//! columns are written out rather than computed by the same expression under test.

use bios_core::duplex::{DuplexRun, RunKind, duplex_runs};
use bios_core::primer::{PrimerSearchOptions, PrimerSite, find_primer_sites};
use bios_core::seq::revcomp;

// The primer search's fixtures, on purpose: the two modules describe the same sites, so a
// disagreement between them shows up as a test failure here rather than as a wrong picture.

/// 40nt.
const TEMPLATE: &str = "GGGGGGGGGGATGCGTACGTTGACCGATCATTTTTTTTTT";

/// ATGCGTACGTTGACCGATCA, 20nt, at cols 10..29.
fn anneal() -> &'static str {
    &TEMPLATE[10..30]
}

/// 18nt of deliberate non-homology.
const TAIL: &str = "CCCCAAAACCCCAAAACC";

fn lin() -> PrimerSearchOptions {
    PrimerSearchOptions { circular: false, ..Default::default() }
}

fn circ() -> PrimerSearchOptions {
    PrimerSearchOptions { circular: true, ..Default::default() }
}

/// The column the oligo's base at `i` faces, derived from a run the way a renderer would.
fn col_of(run: &DuplexRun, i: usize) -> i64 {
    run.start_col + (i as i64 - run.oligo_start as i64) * run.step
}

fn only(target: &str, oligo: &str, options: PrimerSearchOptions) -> PrimerSite {
    let mut sites = find_primer_sites(target, oligo, options).expect("the settings are valid");
    assert_eq!(sites.len(), 1, "fixture should bind exactly once");
    sites.remove(0)
}

fn kinds(runs: &[DuplexRun]) -> Vec<RunKind> {
    runs.iter().map(|r| r.kind).collect()
}

// the forward case: columns ascend, the tail hangs off to the LEFT

fn forward_runs() -> Vec<DuplexRun> {
    let oligo = [TAIL, anneal()].concat();
    let site = only(TEMPLATE, &oligo, lin());
    duplex_runs(&site, &oligo, TEMPLATE.len(), false)
}

#[test]
fn a_tailed_primer_is_two_runs_the_tail_lifted_the_anneal_flat() {
    let runs = forward_runs();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].kind, RunKind::Tail);
    assert!(!runs[0].paired);
    assert_eq!(runs[1].kind, RunKind::Anneal);
    assert!(runs[1].paired);
}

#[test]
fn the_anneal_sits_exactly_on_the_footprint_find_primer_sites_reported() {
    let runs = forward_runs();
    let a = &runs[1];
    assert_eq!(a.start_col, 10); // == site.start
    assert_eq!(a.step, 1);
    assert_eq!(a.length, 20);
    assert_eq!(col_of(a, a.oligo_start + a.length - 1), 29); // == site.end
}

#[test]
fn the_tail_is_contiguous_with_the_anneal_one_molecule_no_gap_between_them() {
    // The claim the whole drawing makes. If these ever differ by even one column the tail reads
    // as a separate floating oligo, which is the thing we are here to not do.
    let runs = forward_runs();
    let (tail, anneal) = (&runs[0], &runs[1]);
    let tail_last_col = col_of(tail, tail.oligo_start + tail.length - 1);
    assert_eq!(tail_last_col, anneal.start_col - 1);
}

#[test]
fn the_tail_lifts_to_the_left_its_columns_are_below_the_anneals() {
    let runs = forward_runs();
    assert_eq!(runs[0].start_col, -8); // 10 - 18
    assert!(runs[0].start_col < runs[1].start_col);
}

// the reverse case: columns descend, the tail hangs off to the RIGHT
//
// The mirror of the block above, and the one that silently breaks. A reverse primer's 3' base
// sits at `start` and its 5' end runs rightward to `end`, so reading the oligo 5'→3' walks
// columns DOWNWARD and the tail continues past `end`, not before `start`.

fn reverse_site_and_runs() -> (PrimerSite, Vec<DuplexRun>) {
    let oligo = [TAIL, &revcomp(anneal())].concat();
    let site = only(TEMPLATE, &oligo, lin());
    let runs = duplex_runs(&site, &oligo, TEMPLATE.len(), false);
    (site, runs)
}

#[test]
fn the_fixture_really_is_a_reverse_site_over_the_same_footprint() {
    let (site, _) = reverse_site_and_runs();
    assert!(!site.forward);
    assert_eq!(site.start, 10);
    assert_eq!(site.end, 29);
    assert_eq!(site.tail_length, 18);
}

#[test]
fn reading_the_oligo_5_prime_to_3_prime_walks_columns_downward() {
    let (_, runs) = reverse_site_and_runs();
    assert_eq!(runs[1].step, -1);
    assert_eq!(runs[1].start_col, 29); // the 5'-most ANNEALED base sits at `end`, not `start`
    let a = &runs[1];
    assert_eq!(col_of(a, a.oligo_start + a.length - 1), 10); // the 3' base lands on `start`
}

#[test]
fn the_tail_lifts_to_the_right_its_columns_are_above_the_anneals_not_below() {
    // The sign error this file exists for. Reversed, this would read 11.
    let (_, runs) = reverse_site_and_runs();
    assert_eq!(runs[0].start_col, 47); // 29 + 18
    assert!(runs[0].start_col > runs[1].start_col);
}

#[test]
fn the_tail_is_contiguous_with_the_anneal_on_this_strand_too() {
    let (_, runs) = reverse_site_and_runs();
    let (tail, anneal) = (&runs[0], &runs[1]);
    assert_eq!(col_of(tail, tail.oligo_start + tail.length - 1), anneal.start_col + 1);
}

// mismatches: a gap in the flat line, the bases lifted in their own columns
//
// ANNEAL with positions 8 and 9 replaced. The 3'-most 10 bases still pair, so the anchor holds
// and the primer still anneals full-length, which is the case that matters: a mutagenesis primer,
// not a chance landing.

fn mismatched_oligo() -> String {
    [&anneal()[..8], "AA", &anneal()[10..]].concat()
}

fn mismatched_site_and_runs() -> (PrimerSite, Vec<DuplexRun>) {
    let oligo = mismatched_oligo();
    let site = only(TEMPLATE, &oligo, lin());
    let runs = duplex_runs(&site, &oligo, TEMPLATE.len(), false);
    (site, runs)
}

#[test]
fn the_fixture_anneals_full_length_with_exactly_the_two_mismatches_we_installed() {
    let (site, _) = mismatched_site_and_runs();
    assert_eq!(site.tail_length, 0);
    assert_eq!(site.anneal_length, 20);
    assert_eq!(site.mismatch_positions, [8, 9]);
}

#[test]
fn a_mismatch_splits_the_paired_run_in_two_and_lifts_the_bases_between_them() {
    let (_, runs) = mismatched_site_and_runs();
    assert_eq!(runs.len(), 3);
    assert_eq!(kinds(&runs), [RunKind::Anneal, RunKind::Mismatch, RunKind::Anneal]);
    assert_eq!(runs.iter().map(|r| r.paired).collect::<Vec<_>>(), [true, false, true]);
}

#[test]
fn consecutive_mismatches_merge_into_one_lifted_run_not_one_run_per_base() {
    // "Ten nucleotides here don't match" is a single lifted stretch. Emitting them individually
    // would leave the renderer stitching them back together to draw one label.
    let (_, runs) = mismatched_site_and_runs();
    let m = &runs[1];
    assert_eq!(m.length, 2);
    assert_eq!(m.bases, "AA");
}

#[test]
fn the_lifted_bases_keep_the_columns_they_would_have_occupied_reading_order_holds() {
    // Not floated aside, not collapsed out: the gap in the flat line is exactly as wide as the
    // lifted run above it.
    let (_, runs) = mismatched_site_and_runs();
    let (left, mid, right) = (&runs[0], &runs[1], &runs[2]);
    assert_eq!(left.start_col, 10);
    assert_eq!(mid.start_col, 18); // 10 + 8
    assert_eq!(right.start_col, 20); // 10 + 10
    assert_eq!(col_of(left, 7), mid.start_col - 1); // flat run ends where the gap begins
    assert_eq!(col_of(mid, 9), right.start_col - 1); // gap ends where the flat run resumes
}

// the origin: a circular target has no edge to fall off

#[test]
fn a_site_spanning_the_origin_reports_columns_that_come_back_round() {
    // ANNEAL's last 10 bases at cols 0..9 and its first 10 at cols 30..39, so the primer binds at
    // start=30, end=9.
    let wrapped = [&anneal()[10..], &"G".repeat(20), &anneal()[..10]].concat();
    assert_eq!(wrapped.len(), 40);
    let site = only(&wrapped, anneal(), circ());
    assert_eq!(site.start, 30);
    assert_eq!(site.end, 9);
    assert!(site.wraps);

    let runs = duplex_runs(&site, anneal(), wrapped.len(), true);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].start_col, 30);
    assert!(!runs[0].off_template); // circular: nothing is ever off the template
}

#[test]
fn a_tail_reaching_back_past_the_origin_lands_at_the_far_end_not_at_a_negative_column() {
    let oligo = [TAIL, anneal()].concat();
    let site = only(TEMPLATE, &oligo, circ());
    let runs = duplex_runs(&site, &oligo, TEMPLATE.len(), true);
    // Linear, this tail starts at -8 (tested above). Circular, -8 is column 32.
    assert_eq!(runs[0].start_col, 32);
    assert!(!runs[0].off_template);
}

// a linear end: reported, never clamped

#[test]
fn a_tail_running_off_the_5_prime_end_is_flagged_out_of_range_rather_than_stacked_on_base_0() {
    // Clamping would pile 18 bases onto column 0 and draw a confident lie. The renderer needs to
    // know so it can hang the tail in the margin or cut it with a count.
    let oligo = [TAIL, anneal()].concat();
    let runs = duplex_runs(&only(TEMPLATE, &oligo, lin()), &oligo, TEMPLATE.len(), false);
    assert!(runs[0].off_template);
    assert_eq!(runs[0].start_col, -8); // the real column, negative and honest
    assert!(!runs[1].off_template); // the anneal is on the template, as always
}

// invariants that catch every off-by-one at once

fn invariant_case(name: &str) -> (String, bool) {
    let oligo = match name {
        "forward, tailed" => [TAIL, anneal()].concat(),
        "reverse, tailed" => [TAIL, &revcomp(anneal())].concat(),
        "forward, mismatched" => mismatched_oligo(),
        "forward, untailed" => anneal().to_string(),
        "reverse, untailed" => revcomp(anneal()),
        other => panic!("no such case: {other}"),
    };
    (oligo, false)
}

/// The body of the web app's first test per case.
fn the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost(name: &str) {
    let (oligo, circular) = invariant_case(name);
    let site = only(TEMPLATE, &oligo, PrimerSearchOptions { circular, ..Default::default() });
    let runs = duplex_runs(&site, &oligo, TEMPLATE.len(), circular);
    assert_eq!(runs.iter().map(|r| r.bases.as_str()).collect::<String>(), oligo);
    // And the indices are a partition: each run starts where the last one ended.
    let mut next = 0;
    for r in &runs {
        assert_eq!(r.oligo_start, next);
        next += r.length;
    }
    assert_eq!(next, oligo.len());
}

/// The body of the web app's second test per case.
fn every_annealed_column_is_inside_the_reported_footprint(name: &str) {
    let (oligo, circular) = invariant_case(name);
    let site = only(TEMPLATE, &oligo, PrimerSearchOptions { circular, ..Default::default() });
    let runs = duplex_runs(&site, &oligo, TEMPLATE.len(), circular);
    let lo = site.start.min(site.end) as i64;
    let hi = site.start.max(site.end) as i64;
    for r in &runs {
        // The tail is the one thing NOT on the footprint.
        if r.kind == RunKind::Tail {
            continue;
        }
        for i in r.oligo_start..r.oligo_start + r.length {
            let c = col_of(r, i);
            assert!(c >= lo && c <= hi, "{name}: column {c} outside {lo}..{hi}");
        }
    }
}

#[test]
fn forward_tailed_the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost() {
    the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost("forward, tailed");
}

#[test]
fn forward_tailed_every_annealed_column_is_inside_the_reported_footprint() {
    every_annealed_column_is_inside_the_reported_footprint("forward, tailed");
}

#[test]
fn reverse_tailed_the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost() {
    the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost("reverse, tailed");
}

#[test]
fn reverse_tailed_every_annealed_column_is_inside_the_reported_footprint() {
    every_annealed_column_is_inside_the_reported_footprint("reverse, tailed");
}

#[test]
fn forward_mismatched_the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost() {
    the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost("forward, mismatched");
}

#[test]
fn forward_mismatched_every_annealed_column_is_inside_the_reported_footprint() {
    every_annealed_column_is_inside_the_reported_footprint("forward, mismatched");
}

#[test]
fn forward_untailed_the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost() {
    the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost("forward, untailed");
}

#[test]
fn forward_untailed_every_annealed_column_is_inside_the_reported_footprint() {
    every_annealed_column_is_inside_the_reported_footprint("forward, untailed");
}

#[test]
fn reverse_untailed_the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost() {
    the_runs_concatenate_back_to_the_oligo_in_order_with_nothing_lost("reverse, untailed");
}

#[test]
fn reverse_untailed_every_annealed_column_is_inside_the_reported_footprint() {
    every_annealed_column_is_inside_the_reported_footprint("reverse, untailed");
}

// refusals

#[test]
fn a_site_paired_with_a_different_oligo_draws_nothing_rather_than_something_plausible() {
    let oligo = [TAIL, anneal()].concat();
    let site = only(TEMPLATE, &oligo, lin());
    assert_eq!(duplex_runs(&site, anneal(), TEMPLATE.len(), false), []);
}

#[test]
fn an_empty_oligo_or_a_zero_length_target_draws_nothing() {
    let site = only(TEMPLATE, anneal(), lin());
    assert_eq!(duplex_runs(&site, "", TEMPLATE.len(), false), []);
    assert_eq!(duplex_runs(&site, anneal(), 0, false), []);
}

// the contract, independent of what find_primer_sites happens to emit

#[test]
fn a_mismatch_immediately_after_the_tail_stays_a_separate_run_they_are_not_the_same_thing() {
    // find_primer_sites can never end an anneal on a mismatch (the extension only closes on a
    // paired base), so these shapes don't arise from it today. They are still part of this
    // function's contract, and a future caller, the gapped aligner, will produce them.
    let hand = PrimerSite {
        start: 10,
        end: 29,
        forward: true,
        tail_length: 4,
        anneal_length: 20,
        mismatches: 1,
        mismatch_positions: vec![4],
        tm: Some(60.0),
        wraps: false,
    };
    // Both are unpaired and adjacent, so a naive merge would fuse them. It must not: one is a
    // payload you designed, the other is a base that failed to pair, and the drawing says so.
    let oligo = ["CCCC", "A", &anneal()[1..]].concat();
    let runs = duplex_runs(&hand, &oligo, TEMPLATE.len(), false);
    assert_eq!(kinds(&runs), [RunKind::Tail, RunKind::Mismatch, RunKind::Anneal]);
    assert_eq!(runs[0].length, 4);
    assert_eq!(runs[1].length, 1);
    assert_eq!(runs[1].start_col, 10); // the anneal's first column, mismatched or not
}
