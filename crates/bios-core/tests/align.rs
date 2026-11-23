//! Local alignment: the cases the primer search could not answer, ported from the web app's
//! `align.test.ts` in its order and with its numbers.
//!
//! The first block is not a generic test suite: it is three failures written down as tests so
//! they cannot come back. Each one returned NOTHING from `find_primer_sites` at every setting;
//! "nothing" is the answer this module exists to stop giving.

use std::time::Instant;

use bios_core::align::{AlignOp, AlignOptions, LocalAlignment, local_align};
use bios_core::duplex::{AlignRun, AlignRunKind, alignment_runs};
use bios_core::primer::{PrimerSearchOptions, find_primer_sites};
use bios_core::seq::revcomp;

/// 60nt.
const T: &str = "GGGGGGGGGGATGCGTACGTTGACCGATCATTTTTTTTTTAAACCCTTTGGGACGTACGT";

/// 30nt at cols 10..39.
fn core() -> &'static str {
    &T[10..40]
}

fn lin() -> AlignOptions {
    AlignOptions { circular: false, ..Default::default() }
}

fn flip(b: char) -> char {
    match b {
        'A' => 'C',
        'C' => 'A',
        'G' => 'T',
        'T' => 'G',
        other => panic!("not a base: {other}"),
    }
}

/// `CORE` with every third base wrong.
fn every_3rd_mismatched() -> String {
    core().chars().enumerate().map(|(i, b)| if i % 3 == 2 { flip(b) } else { b }).collect()
}

/// `CORE` with `count` Cs put in after its first 15 bases.
fn with_insert(count: usize) -> String {
    [&core()[..15], &"C".repeat(count), &core()[15..]].concat()
}

/// `CORE` with base 8 wrong.
fn one_mismatch() -> String {
    core().chars().enumerate().map(|(i, b)| if i == 8 { flip(b) } else { b }).collect()
}

fn best(a: Vec<LocalAlignment>) -> LocalAlignment {
    a.into_iter().next().expect("expected at least one alignment")
}

fn op_count(a: &LocalAlignment, op: AlignOp) -> usize {
    a.blocks.iter().filter(|b| b.op == op).map(|b| b.query_length.max(b.target_length)).sum()
}

fn ops(a: &LocalAlignment) -> Vec<AlignOp> {
    a.blocks.iter().map(|b| b.op).collect()
}

fn starts(hits: &[LocalAlignment]) -> Vec<usize> {
    let mut starts: Vec<usize> = hits.iter().map(|h| h.start).collect();
    starts.sort_unstable();
    starts
}

// the three cases that used to return nothing

#[test]
fn every_3rd_base_mismatched_was_0_sites_at_every_setting() {
    let q = every_3rd_mismatched();
    // Proof the old engine still can't, so this test keeps meaning something.
    let primer_search = PrimerSearchOptions { circular: false, max_mismatches: 99, min_anchor3: 1, ..Default::default() };
    assert_eq!(find_primer_sites(T, &q, primer_search), Ok(vec![]));

    let a = best(local_align(T, &q, lin()));
    assert_eq!(a.start, 10);
    assert!(a.forward);
    // The query's LAST base is one of the mismatches (29 % 3 === 2), and a local alignment never
    // ends on a mismatch: ending there costs score, so the optimum stops at the last match and
    // the odd base becomes overhang. Hence 38, not 39. This is Smith-Waterman being right, and
    // asserting 39 would be pinning a bug in place.
    assert_eq!(a.end, 38);
    assert_eq!(a.trailing_overhang, 1);
    assert_eq!(op_count(&a, AlignOp::Mismatch), 9); // the tenth was trimmed with the end
    assert_eq!(op_count(&a, AlignOp::Match), 20);
    assert!(a.identity > 0.6 && a.identity < 0.75, "identity {}", a.identity);
}

#[test]
fn the_scoring_ratio_is_what_carries_a_periodic_mismatch_1_2_is_the_wall() {
    // The constant this module turns on, pinned so nobody tunes it back. The primer search scores
    // +1/-2, which is exactly break-even at one mismatch in three: the score never rises, so the
    // extension never commits. Anything at or beyond 1:2 collapses the case again.
    let q = every_3rd_mismatched();
    let matched =
        |a: Option<&LocalAlignment>| a.map_or(0, |a| a.blocks.iter().filter(|b| b.op == AlignOp::Match).map(|b| b.query_length).sum::<usize>());

    let at_the_wall = local_align(T, &q, AlignOptions { match_score: 1.0, mismatch_score: -2.0, min_score: 5.0, ..lin() });
    assert!(matched(at_the_wall.first()) < 10, "1:2 should collapse, aligned {}", matched(at_the_wall.first()));

    let ours = local_align(T, &q, AlignOptions { min_score: 5.0, ..lin() });
    assert_eq!(matched(ours.first()), 20);
}

#[test]
fn an_insert_with_clean_flanks_one_alignment_spanning_both_not_two_fragments() {
    let q = with_insert(20);
    let a = best(local_align(T, &q, lin()));
    assert_eq!(a.start, 10);
    assert_eq!(a.end, 39, "the alignment must span BOTH flanks, not stop at the insert");
    assert_eq!(op_count(&a, AlignOp::Insertion), 20);
    assert_eq!(op_count(&a, AlignOp::Match), 30);
    // One event in the middle, flanks either side of it.
    assert_eq!(ops(&a), [AlignOp::Match, AlignOp::Insertion, AlignOp::Match]);
}

#[test]
fn a_gap_has_to_be_earned_by_flanking_homology_and_the_threshold_is_real() {
    // Not a bug, a deliberate trade, measured and written down so nobody tunes it away by
    // accident. Cheap gaps let the aligner buy homology that isn't there: at gapOpen 5 a clean
    // 18 nt tail on a real plasmid came back as "4 mismatches and a 14 nt deletion", because the
    // gap cost less than the extra matches it unlocked. At 12, bridging needs flanks worth more
    // than the gap. With 15 bp flanks that holds to ~20 nt; on a real plasmid 20 bp flanks
    // bridged 31 nt. Beyond it, the honest answer is one flank plus a large overhang.
    let bridged = |ins: usize| {
        let hits = local_align(T, &with_insert(ins), lin());
        hits.first().is_some_and(|a| a.blocks.iter().any(|b| b.op == AlignOp::Insertion))
    };
    assert!(bridged(6));
    assert!(bridged(20));
    assert!(!bridged(30), "15 bp of homology does not justify a 30 nt gap");
}

#[test]
fn one_end_perfect_the_other_junk_reports_the_half_that_fits() {
    let q = [&core()[..15], "CACACACACACACAC"].concat();
    let a = best(local_align(T, &q, lin()));
    assert_eq!(a.start, 10);
    assert_eq!(a.leading_overhang, 0);
    assert!(a.trailing_overhang >= 12, "the junk half is overhang, got {}", a.trailing_overhang);
    // >= rather than ==: a junk base adjacent to the clean half can chance-match the template and
    // legitimately extend the alignment by one. The claim under test is that the clean half is
    // found whole, not that the boundary falls on a particular base.
    assert!(op_count(&a, AlignOp::Match) >= 15, "matched {}", op_count(&a, AlignOp::Match));
}

// it still gets the ordinary cases right

#[test]
fn a_perfect_match_is_one_block_at_the_right_columns() {
    let a = best(local_align(T, core(), lin()));
    assert_eq!(ops(&a), [AlignOp::Match]);
    assert_eq!(a.start, 10);
    assert_eq!(a.end, 39);
    assert_eq!(a.identity, 1.0);
    assert_eq!(a.leading_overhang, 0);
    assert_eq!(a.trailing_overhang, 0);
}

#[test]
fn a_reverse_strand_match_lands_on_the_same_footprint_flagged_reverse() {
    let a = best(local_align(T, &revcomp(core()), lin()));
    assert!(!a.forward);
    assert_eq!(a.start, 10);
    assert_eq!(a.end, 39);
    assert_eq!(a.identity, 1.0);
}

#[test]
fn a_5_prime_tail_is_overhang_not_mismatch_and_on_the_reverse_strand_it_is_still_5_prime() {
    const TAIL: &str = "CCTGCAGGTCGACTCTAG";
    // >= 17, not == 18, on BOTH strands and for the same reason: this tail ends in G and the
    // template's flank is poly-G, so the alignment extends one base into the tail by a genuine
    // chance match. That is the aligner being greedy and correct. The claim worth pinning is that
    // the tail is OVERHANG (unaligned bases at the 5' end) and not a run of mismatches.
    let fwd = best(local_align(T, &[TAIL, core()].concat(), lin()));
    assert!(fwd.leading_overhang >= 17, "5' overhang landed at {}", fwd.leading_overhang);
    assert_eq!(fwd.trailing_overhang, 0);
    assert_eq!(op_count(&fwd, AlignOp::Mismatch), 0, "a tail is not a mismatch");

    // THE MIRROR. On the minus strand the query's 5' end is at the HIGH column, so the overhang
    // that is "leading" in query order must still be the 5' one: the whole tail, at the START of
    // the oligo. Getting this backwards puts every reverse tail at the wrong end, silently.
    // Allowed to be 17: the tail base abutting the anneal can chance-match and extend it.
    let rev = best(local_align(T, &[TAIL, &revcomp(core())].concat(), lin()));
    assert!(!rev.forward);
    assert!(rev.leading_overhang >= 17, "5' overhang landed at {}", rev.leading_overhang);
    assert_eq!(rev.trailing_overhang, 0, "nothing hangs off the 3' end of this oligo");
}

#[test]
fn mismatch_positions_index_the_users_query_on_both_strands() {
    let q = one_mismatch();
    let fwd = best(local_align(T, &q, lin()));
    assert_eq!(fwd.mismatch_positions, [8]);

    let rq = revcomp(&q); // the same single mismatch, now at query index 30-1-8 = 21
    let rev = best(local_align(T, &rq, lin()));
    assert!(!rev.forward);
    assert_eq!(rev.mismatch_positions, [21]);
}

// blocks are a partition: the invariant that catches every off-by-one

fn partition_case(name: &str) -> String {
    match name {
        "perfect" => core().to_string(),
        "reverse" => revcomp(core()),
        "tailed" => ["CCTGCAGGTCGACTCTAG", core()].concat(),
        "every 3rd mismatched" => every_3rd_mismatched(),
        "30bp insert" => with_insert(30),
        "reverse + insert" => revcomp(&with_insert(30)),
        other => panic!("no such case: {other}"),
    }
}

/// The body of the web app's first test per case.
fn query_bases_are_covered_once_in_order_with_no_holes(name: &str) {
    let q = partition_case(name);
    let a = best(local_align(T, &q, lin()));
    let mut next = a.leading_overhang;
    for b in a.blocks.iter().filter(|b| b.query_length > 0) {
        assert_eq!(b.query_start, Some(next), "{name}: block starts at {:?}, expected {next}", b.query_start);
        next += b.query_length;
    }
    assert_eq!(next + a.trailing_overhang, q.len(), "{name}: blocks + overhangs must be the whole query");
}

/// The body of the web app's second test per case.
fn template_columns_step_contiguously_in_the_strands_own_direction(name: &str) {
    // Blocks are in QUERY order (5'→3'). On the plus strand that means columns ascend from
    // `start`; on the MINUS strand they descend from `end`, because reading the oligo 5'→3' walks
    // the template leftward. Same rule the duplex runs encode as a step of 1 or -1, and the
    // reverse half of it is invisible in any single-block case: it took a reverse alignment WITH
    // an insert to expose that this was never actually pinned.
    let q = partition_case(name);
    let a = best(local_align(T, &q, lin()));
    let on_template = a.blocks.iter().filter(|b| b.target_length > 0);
    if a.forward {
        let mut cursor = a.start as i64;
        for b in on_template {
            assert_eq!(b.target_start.map(|s| s as i64), Some(cursor), "{name}: column jump at {:?}", b.op);
            cursor += b.target_length as i64;
        }
        assert_eq!(cursor - 1, a.end as i64, "{name}: footprint end");
    } else {
        let mut cursor = a.end as i64;
        for b in on_template {
            assert_eq!(b.target_start.map(|s| (s + b.target_length) as i64 - 1), Some(cursor), "{name}: column jump at {:?}", b.op);
            cursor -= b.target_length as i64;
        }
        assert_eq!(cursor + 1, a.start as i64, "{name}: footprint start");
    }
}

#[test]
fn perfect_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("perfect");
}

#[test]
fn perfect_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("perfect");
}

#[test]
fn reverse_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("reverse");
}

#[test]
fn reverse_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("reverse");
}

#[test]
fn tailed_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("tailed");
}

#[test]
fn tailed_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("tailed");
}

#[test]
fn every_3rd_mismatched_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("every 3rd mismatched");
}

#[test]
fn every_3rd_mismatched_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("every 3rd mismatched");
}

#[test]
fn a_30bp_insert_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("30bp insert");
}

#[test]
fn a_30bp_insert_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("30bp insert");
}

#[test]
fn reverse_insert_query_bases_are_covered_once_in_order_with_no_holes() {
    query_bases_are_covered_once_in_order_with_no_holes("reverse + insert");
}

#[test]
fn reverse_insert_template_columns_step_contiguously_in_the_strands_own_direction() {
    template_columns_step_contiguously_in_the_strands_own_direction("reverse + insert");
}

// one site is one alignment
//
// Without the overlap filter every strong alignment reports itself once per cell along its own
// diagonal: thirty near-identical hits for one site. The count is the only thing that catches it:
// each of those duplicates is individually correct, so no assertion about a single alignment's
// contents would ever fail.

#[test]
fn a_unique_site_is_reported_once_not_once_per_cell_of_its_diagonal() {
    assert_eq!(local_align(T, core(), lin()).len(), 1);
}

#[test]
fn two_real_occurrences_are_two_alignments_at_the_two_footprints() {
    let twice = [T, "ACGTACGTACGT", T].concat();
    let hits = local_align(&twice, core(), lin());
    assert_eq!(hits.len(), 2);
    assert_eq!(starts(&hits), [10, 82]);
    assert!(hits.iter().all(|h| h.identity == 1.0));
}

#[test]
fn a_palindrome_is_not_doubled_by_being_found_on_both_strands() {
    // Both strands are searched, so a self-reverse-complementary query matches the same columns
    // twice. That is one site, and the overlap filter is what says so.
    let pal = "GAATTC";
    let with_pal = ["TTTTTTTTTT", pal, "TTTTTTTTTT"].concat();
    let hits = local_align(&with_pal, pal, AlignOptions { min_score: 10.0, ..lin() });
    assert_eq!(hits.len(), 1);
}

// a long query is not a slow query
//
// The bug this pins: every cell over the floor used to be traced back before being dropped for
// overlapping the one real hit, so a 1 kb query against a 6 kb plasmid took 19 s and 1.5 million
// tracebacks. Deterministic random DNA: the web app's generator, a 32-bit LCG.

fn random_dna(len: usize, seed: u32) -> String {
    let mut x = seed;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        out.push(char::from(b"ACGT"[(x >> 30) as usize]));
    }
    out
}

fn plasmid() -> String {
    random_dna(6000, 7)
}

#[test]
fn one_kb_cut_from_a_6_kb_circular_plasmid_comes_back_in_well_under_a_second() {
    let plasmid = plasmid();
    let q = &plasmid[2500..3500];
    let t0 = Instant::now();
    // The floor the app uses (0.6 per base): at the raw default of 20, random DNA this long has a
    // dozen chance 10-mer matches per strand, and every one of them is a correct answer.
    let hits = local_align(&plasmid, q, AlignOptions { circular: true, min_score: 600.0, ..Default::default() });
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("1 kb vs 6 kb circular: {ms:.0} ms, {} hit(s)", hits.len());
    assert!(ms < 750.0, "took {ms:.0} ms");
    assert_eq!(hits.len(), 1);
    let a = best(hits);
    assert_eq!(a.start, 2500);
    assert_eq!(a.end, 3499);
    assert_eq!(a.identity, 1.0);
    assert_eq!(ops(&a), [AlignOp::Match]);
}

#[test]
fn a_500_mer_present_three_times_is_three_hits_on_either_strand() {
    let base = plasmid();
    let frag = random_dna(500, 99);
    let plasmid = [&base[..1000], &frag, &base[1000..3000], &frag, &base[3000..5000], &frag, &base[5000..]].concat();
    let options = AlignOptions { circular: true, min_score: 300.0, ..Default::default() };
    let fwd = local_align(&plasmid, &frag, options);
    assert_eq!(starts(&fwd), [1000, 3500, 6000]);
    assert!(fwd.iter().all(|h| h.forward && h.identity == 1.0));
    let rev = local_align(&plasmid, &revcomp(&frag), options);
    assert_eq!(starts(&rev), [1000, 3500, 6000]);
    assert!(rev.iter().all(|h| !h.forward && h.identity == 1.0));
}

#[test]
fn the_hit_cap_is_on_the_answer_not_per_strand() {
    // A palindrome-free 30-mer planted 30 times: 24 come back, not 24 per strand.
    let many: String = (0..30).map(|k| random_dna(120, 500 + k) + core()).collect();
    assert_eq!(local_align(&many, core(), AlignOptions { min_score: 40.0, ..lin() }).len(), 24);
}

// refusals and edges

#[test]
fn random_junk_aligns_nowhere_worth_reporting() {
    assert_eq!(local_align(T, "CACACACACACACACACACA", lin()), []);
}

#[test]
fn an_empty_query_or_target_is_not_an_alignment() {
    assert_eq!(local_align(T, "", lin()), []);
    assert_eq!(local_align("", core(), lin()), []);
}

#[test]
fn a_run_of_ns_never_fabricates_a_match() {
    let with_ns = "N".repeat(30);
    assert_eq!(local_align(&"N".repeat(60), &with_ns, lin()), []);
}

#[test]
fn a_site_spanning_the_origin_is_found_on_a_circular_target_and_lost_on_a_linear_one() {
    let wrapped = [&core()[15..], &"G".repeat(20), &core()[..15]].concat(); // binds 35..14
    let circ = best(local_align(&wrapped, core(), AlignOptions { circular: true, ..Default::default() }));
    assert_eq!(circ.start, 35);
    assert_eq!(circ.identity, 1.0);
    assert_eq!(op_count(&circ, AlignOp::Match), 30, "circular: the whole query aligns, across the origin");
    // NOT `identity < 1`: identity is measured over ALIGNED columns, so a partial-but-perfect
    // alignment scores 1.0 too. The thing a linear target must not do is align the WHOLE query.
    let linear = local_align(&wrapped, core(), lin());
    assert!(linear.iter().all(|a| op_count(a, AlignOp::Match) < 30), "a linear target must not join its two ends");
}

// alignment_runs: the drawing's view

const TAIL: &str = "CCTGCAGGTCGACTCTAG";

fn find_run(runs: &[AlignRun], wanted: impl Fn(&AlignRun) -> bool) -> &AlignRun {
    runs.iter().find(|r| wanted(r)).expect("expected such a run")
}

#[test]
fn mismatches_stay_on_the_line_in_their_own_columns() {
    // The grammar change this pins. A mismatch is a wrong base in a real column, so it keeps its
    // place and turns red; only bases with NO column lift. If this ever flips back, the two
    // signals collapse into one again and the drawing stops distinguishing them.
    let q = one_mismatch();
    let a = best(local_align(T, &q, lin()));
    let runs = alignment_runs(&a, &q, T.len(), false);
    let mm = runs.iter().find(|r| r.kind == AlignRunKind::Mismatch).expect("expected a mismatch run");
    assert!(mm.paired, "a mismatch sits in its column: it does not lift");
    assert_eq!(mm.template_length, 1);
    assert_eq!(mm.start_col, 18); // 10 + 8
}

#[test]
fn an_overhang_lifts_has_no_template_columns_and_stays_contiguous_with_the_alignment() {
    let q = [TAIL, core()].concat();
    let a = best(local_align(T, &q, lin()));
    let runs = alignment_runs(&a, &q, T.len(), false);
    let oh = find_run(&runs, |r| r.kind == AlignRunKind::Overhang);
    let first = find_run(&runs, |r| r.paired);
    assert!(!oh.paired, "an overhang lifts");
    assert_eq!(oh.template_length, 0, "an overhang occupies no template column");
    assert_eq!(oh.start_col + oh.length as i64 * oh.step, first.start_col, "contiguous with the alignment");
}

#[test]
fn an_insertion_lifts_and_does_not_advance_the_template_cursor() {
    // The flanks must stay tight: the template has no columns to give, so the bases either side of
    // an insert are adjacent. Advancing the cursor would tear a 20-column hole in the drawing.
    let q = with_insert(20);
    let a = best(local_align(T, &q, lin()));
    let runs = alignment_runs(&a, &q, T.len(), false);
    let (left, ins, right) = (&runs[0], &runs[1], &runs[2]);
    assert_eq!(ins.kind, AlignRunKind::Insertion);
    assert!(!ins.paired);
    assert_eq!(ins.template_length, 0);
    assert_eq!(left.start_col + left.length as i64, right.start_col, "flanks stay tight across the insert");
}

#[test]
fn on_the_reverse_strand_the_overhang_lifts_to_the_right() {
    let q = [TAIL, &revcomp(core())].concat();
    let a = best(local_align(T, &q, lin()));
    let runs = alignment_runs(&a, &q, T.len(), false);
    let oh = find_run(&runs, |r| r.kind == AlignRunKind::Overhang);
    let first = find_run(&runs, |r| r.paired);
    assert_eq!(oh.step, -1);
    assert!(oh.start_col > first.start_col, "reverse 5' overhang sits at HIGHER columns");
}

#[test]
fn runs_reconstruct_the_query_in_order_with_nothing_lost() {
    let q = ["CCTGCAGG", &core()[..15], &"C".repeat(12), &core()[15..]].concat();
    let a = best(local_align(T, &q, lin()));
    let runs = alignment_runs(&a, &q, T.len(), false);
    assert_eq!(runs.iter().filter(|r| r.length > 0).map(|r| r.bases.as_str()).collect::<String>(), q);
}
