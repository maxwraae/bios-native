//! Find's Alignment mode, as the web app's `editor/alignment.test.ts` checks it, in its order and
//! with its numbers: whether the caption tells the truth about a fit, and whether the row
//! segmentation puts bases in the columns they belong to.

use bios_core::duplex::AlignRunKind;
use bios_core::find_align::{AlignHit, TM_MAX_NT, alignment_hits, describe_hit, hit_runs, row_inserts, row_segments, template_aligned_bases};
use bios_core::seq::revcomp;

const T: &str = "GGGGGGGGGGATGCGTACGTTGACCGATCATTTTTTTTTTAAACCCTTTGGGACGTACGT"; // 60nt
const TAIL: &str = "CCTGCAGGTCGACTCTAG";

fn core() -> &'static str {
    &T[10..40] // 30nt at cols 10..39
}

fn flip(b: char) -> char {
    match b {
        'A' => 'C',
        'C' => 'A',
        'G' => 'T',
        _ => 'G',
    }
}

fn complement(s: &str) -> String {
    s.chars()
        .map(|b| match b {
            'A' => 'T',
            'T' => 'A',
            'G' => 'C',
            _ => 'G',
        })
        .collect()
}

fn first(sequence: &str, circular: bool, q: &str) -> AlignHit {
    alignment_hits(sequence, circular, q).into_iter().next().expect("a hit")
}

fn describe(h: &AlignHit) -> String {
    describe_hit(h, None)
}

fn with_flip_at(s: &str, i: usize) -> String {
    s.chars().enumerate().map(|(k, b)| if k == i { flip(b) } else { b }).collect()
}

// It always answers: the whole point of the mode.

#[test]
fn a_query_that_only_half_fits_still_reports_the_half_that_fits() {
    let hits = alignment_hits(T, false, &format!("{}CACACACACACACAC", &core()[..15]));
    assert!(!hits.is_empty(), "must not return nothing");
    assert!(describe(&hits[0]).contains("nt overhang"));
}

#[test]
fn every_3rd_base_mismatched_still_aligns_and_the_caption_says_how_badly() {
    let q: String = core().chars().enumerate().map(|(i, b)| if i % 3 == 2 { flip(b) } else { b }).collect();
    let caption = describe(&first(T, false, &q));
    assert!(caption.contains("9 mismatches"), "{caption}");
    let identity = caption.split(" · ").find(|p| p.ends_with("% identity")).unwrap();
    assert!(identity.starts_with('6') && identity.len() == "6x% identity".len(), "{caption}");
}

#[test]
fn an_insert_is_named_as_an_insert_not_as_a_pile_of_mismatches() {
    let q = format!("{}{}{}", &core()[..15], "C".repeat(20), &core()[15..]);
    let caption = describe(&first(T, false, &q));
    assert!(caption.contains("20 bp extra"), "{caption}");
    assert!(!caption.contains("mismatch"), "{caption}");
    assert!(caption.contains(&format!("{}/{} nt aligned", q.len() - 20, q.len())), "{caption}");
}

#[test]
fn a_short_insert_is_named_by_its_bases() {
    let q = format!("{}GATTCA{}", &core()[..15], &core()[15..]);
    assert!(describe(&first(T, false, &q)).contains("GATTCA extra"));
}

#[test]
fn a_deletion_says_missing_and_the_two_changes_share_no_word() {
    let q = format!("{}{}", &core()[..12], &core()[18..]);
    let caption = describe(&first(T, false, &q));
    assert!(caption.contains("6 bp missing"), "{caption}");
    assert!(!caption.contains("extra"), "{caption}");
}

// Priming is a label, never a filter.

#[test]
fn a_clean_full_length_fit_would_prime() {
    assert!(describe(&first(T, false, core())).contains("would prime"));
}

#[test]
fn a_3_prime_unpaired_fit_is_reported_and_told_it_wont_prime() {
    let h = first(T, false, &format!("{}CACACACACACACAC", &core()[..15]));
    assert!(describe(&h).contains("won't prime — 3' end unpaired"));
}

#[test]
fn a_fit_that_melts_below_the_annealing_step_says_so_with_its_number() {
    let cold = &T[28..40]; // the AT-rich stretch
    if let Some(h) = alignment_hits(T, false, cold).first() {
        let caption = describe(h);
        let verdict = caption.split("won't prime — ").nth(1).unwrap_or_default();
        assert!(verdict.contains("Tm ") && verdict.contains("°C"), "{caption}");
    }
}

// A long fit is told the truth, and not asked to prime.

fn rnd(n: usize, seed: u32) -> String {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ['A', 'C', 'G', 'T'][(x >> 30) as usize]
        })
        .collect()
}

fn plasmid() -> String {
    rnd(1200, 3)
}

#[test]
fn identity_is_floored() {
    let p = plasmid();
    let q = format!("{}{}{}", &p[100..350], flip(p.as_bytes()[350] as char), &p[351..400]);
    let h = first(&p, false, &q);
    assert!(h.alignment.identity > 0.996 && h.alignment.identity < 1.0, "identity {}", h.alignment.identity);
    assert!(describe(&h).contains("· 99% identity · 1 mismatch"));
}

#[test]
fn past_tm_max_annealed_bases_there_is_no_tm_and_no_verdict() {
    let p = plasmid();
    let h = first(&p, false, &p[100..400]);
    const { assert!(TM_MAX_NT < 300) };
    assert!(!h.primable);
    assert_eq!(h.anneal_tm, None);
    assert!(h.priming_faults.is_empty());
    assert_eq!(describe(&h), "300/300 nt aligned · 100% identity");
}

#[test]
fn a_long_tailed_primer_is_still_an_oligo() {
    let p = plasmid();
    let h = first(&p, false, &format!("CCTGCAGGTCGACTCTAG{}", &p[100..124]));
    assert!(h.primable);
    assert!(h.anneal_tm.is_some());
    let caption = describe(&h);
    assert!(caption.ends_with("°C · would prime") || caption.contains("°C · won't prime"), "{caption}");
}

fn long_edit(p: &str) -> String {
    format!("{}GGGGGGGGGGGGGGGGGGGG{}{}", &p[100..300], &p[300..500], &p[560..800])
}

#[test]
fn a_long_fit_names_each_length_change_with_its_size_and_its_place() {
    let p = plasmid();
    let caption = describe_hit(&first(&p, false, &long_edit(&p)), Some(p.len()));
    assert!(caption.contains("20 bp extra after 300"), "{caption}");
    assert!(caption.contains("60 bp missing at 501..560"), "{caption}");
    assert!(!caption.contains("GGGG"), "a long fit names the size, not the bases");
}

#[test]
fn on_the_minus_strand_the_places_are_still_plus_strand_columns() {
    let p = plasmid();
    let h = first(&p, false, &revcomp(&long_edit(&p)));
    assert!(!h.alignment.forward);
    let caption = describe_hit(&h, Some(p.len()));
    assert!(caption.contains("20 bp extra after 300"), "{caption}");
    assert!(caption.contains("60 bp missing at 501..560"), "{caption}");
}

#[test]
fn more_than_three_length_changes_are_counted_not_listed() {
    let p = plasmid();
    let q = [&p[100..200], "GGGGGGGG", &p[200..300], "GGGGGGGG", &p[300..400], "GGGGGGGG", &p[400..500], "GGGGGGGG", &p[500..600]].concat();
    let caption = describe_hit(&first(&p, false, &q), Some(p.len()));
    assert!(caption.contains("4 insertions"), "{caption}");
    assert!(!caption.contains("extra after"), "{caption}");
}

#[test]
fn a_fit_that_laps_the_construct_says_so() {
    let p = plasmid();
    let circ = format!("{}{}", &p[600..], &p[..600]);
    let h = first(&p, true, &circ);
    assert!(describe_hit(&h, Some(p.len())).contains("covers the whole construct"));
    assert!(!describe(&h).contains("covers"));
}

// The flat lane.

#[test]
fn aligned_bases_are_laid_against_template_columns() {
    let q = with_flip_at(core(), 8);
    let h = first(T, false, &q);
    let bases = template_aligned_bases(&hit_runs(&h, T.len(), false));
    assert_eq!(bases.len(), h.alignment.end - h.alignment.start + 1, "one base per template column, exactly");
}

#[test]
fn a_reverse_hit_hands_over_the_primers_bases_not_the_column_order_text() {
    let q = revcomp(core());
    let h = first(T, false, &q);
    assert!(!h.alignment.forward);
    assert_eq!(template_aligned_bases(&hit_runs(&h, T.len(), false)), q);
    assert_ne!(template_aligned_bases(&hit_runs(&h, T.len(), false)), complement(core()));
}

#[test]
fn a_deletion_is_filled_with_ampersands_the_gap_in_place() {
    let q = format!("{}{}", &core()[..15], &core()[21..]);
    let h = first(T, false, &q);
    let bases = template_aligned_bases(&hit_runs(&h, T.len(), false));
    assert!(bases.contains('&'), "expected an invisible filler, got {bases}");
    assert_eq!(bases.len(), h.alignment.end - h.alignment.start + 1);
}

// Rows.

const BPR: usize = 10;

#[test]
fn a_run_straddling_a_row_boundary_splits_and_the_pieces_sum_to_the_run() {
    let h = first(T, false, core());
    let segs = row_segments(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    assert_eq!(segs.iter().map(|s| s.length).sum::<usize>(), 30);
    assert_eq!(segs.iter().map(|s| s.row_index).collect::<Vec<_>>(), [1, 2, 3]);
}

#[test]
fn mismatches_come_back_paired() {
    let h = first(T, false, &with_flip_at(core(), 8));
    let segs = row_segments(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    let mm: Vec<_> = segs.iter().filter(|s| s.kind == AlignRunKind::Mismatch).collect();
    assert_eq!(mm.len(), 1);
    assert!(mm[0].paired);
    assert_eq!(mm[0].col_in_row + mm[0].row_index * BPR, 18);
}

#[test]
fn only_bases_with_no_column_lift_and_an_overhang_is_the_one_that_still_has_columns() {
    let h = first(T, false, &format!("{TAIL}{}", core()));
    let segs = row_segments(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    assert!(segs.iter().filter(|s| !s.paired).all(|s| s.kind == AlignRunKind::Overhang));
}

#[test]
fn an_insertion_claims_no_column_at_all() {
    let q = format!("{}GGGGG{}", &core()[..15], &core()[15..]);
    let h = first(T, false, &q);
    let segs = row_segments(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    assert!(segs.iter().all(|s| s.kind != AlignRunKind::Insertion));
    let resume = h.alignment.start + 15;
    let over = segs
        .iter()
        .filter(|s| {
            let c0 = s.row_index * BPR + s.col_in_row;
            resume >= c0 && resume < c0 + s.length
        })
        .count();
    assert_eq!(over, 1, "columns covered by {over} runs");
}

// An insertion sits between two columns.

#[test]
fn forward_the_boundary_is_the_column_the_alignment_resumes_at() {
    let q = format!("{}GGGGG{}", &core()[..15], &core()[15..]);
    let h = first(T, false, &q);
    assert!(h.alignment.forward);
    let ins = row_inserts(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    assert_eq!(ins.len(), 1);
    assert_eq!(ins[0].bases, "GGGGG");
    assert_eq!(ins[0].row_index * BPR + ins[0].boundary, h.alignment.start + 15);
}

#[test]
fn reverse_the_boundary_is_on_the_other_side_and_the_bases_stay_5_to_3() {
    let q = revcomp(&format!("{}GGGGG{}", &core()[..15], &core()[15..]));
    let h = first(T, false, &q);
    assert!(!h.alignment.forward);
    let ins = row_inserts(&hit_runs(&h, T.len(), false), BPR, T.len(), false);
    assert_eq!(ins.len(), 1);
    assert_eq!(ins[0].bases, "CCCCC");
    assert_eq!(ins[0].row_index * BPR + ins[0].boundary, h.alignment.end - 15 + 1);
}

#[test]
fn a_nonsense_row_width_places_nothing_rather_than_dividing_by_it() {
    let q = format!("{}GGGGG{}", &core()[..15], &core()[15..]);
    let h = first(T, false, &q);
    assert_eq!(row_inserts(&hit_runs(&h, T.len(), false), 0, T.len(), false), []);
}

#[test]
fn a_nonsense_row_width_draws_nothing_rather_than_dividing_by_it() {
    let h = first(T, false, core());
    assert_eq!(row_segments(&hit_runs(&h, T.len(), false), 0, T.len(), false), []);
}

// Bios's own: the size Find admits, stated rather than tried.

#[test]
fn the_longest_query_is_the_one_the_cell_budget_admits() {
    use bios_core::find_align::{CELL_BUDGET, FIND_MAX_NT, cells_for, query_limit};
    for (length, circular) in [(6_000, true), (6_000, false), (48_000, true), (200, true), (1, false)] {
        let m = query_limit(length, circular);
        assert!(m <= FIND_MAX_NT);
        assert!(cells_for(length, m, circular) <= CELL_BUDGET, "{length} {circular} {m}");
        if m < FIND_MAX_NT {
            assert!(cells_for(length, m + 1, circular) > CELL_BUDGET, "{length} {circular} {m}");
        }
    }
    assert_eq!(query_limit(0, true), 0);
    let p = plasmid();
    let too_long = rnd(query_limit(p.len(), true) + 1, 9);
    assert!(too_long.len() <= FIND_MAX_NT);
    assert_eq!(alignment_hits(&p, true, &too_long), []);
}

#[test]
#[ignore = "a timing, run by hand: cargo test --release -p bios-core --test find_align -- --ignored --nocapture"]
fn how_long_the_largest_query_takes() {
    let p = rnd(6_000, 5);
    let m = bios_core::find_align::query_limit(p.len(), true);
    let q = rnd(m, 11);
    let started = std::time::Instant::now();
    let hits = alignment_hits(&p, true, &q);
    println!("{m} nt against 6 kb circular: {} hits in {:?}", hits.len(), started.elapsed());
}
