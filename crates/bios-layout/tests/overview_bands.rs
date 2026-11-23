//! Parity with Bios `editor/overview/bands.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.
//!
//! The original builds each band as `band({ ...overrides })`; here that is `OuterBand { ..band() }`.
//! `layoutBand(band, L)` leaves the minimum body out, so this passes `None`.

use bios_layout::overview::bands::{BandSeam, BandSegment, DIM_AT, OuterBand, layout_band, overlap_fraction, removed_len};
use bios_layout::overview::viewport::Piece;

const L: i64 = 1000;
const LF: f64 = L as f64;

fn band() -> OuterBand {
    OuterBand {
        up: 300,
        down: 400,
        left_arm_bp: 30,
        right_arm_bp: 30,
        insert_bp: 500,
        color: "#000".to_owned(),
        arm_color: None,
        label: None,
        segments: Vec::new(),
        seams: Vec::new(),
    }
}

/// `{ name, fromBp, toBp, color }`: a segment with no tier given.
fn segment(name: &str, from_bp: i64, to_bp: i64, color: &str) -> BandSegment {
    BandSegment { name: name.to_owned(), from_bp, to_bp, color: color.to_owned(), tier: None }
}

fn seam(at_bp: i64, tm: f64, label: &str) -> BandSeam {
    BandSeam { at_bp, tm, label: label.to_owned() }
}

fn total(ps: &[Piece]) -> f64 {
    ps.iter().fold(0.0, |n, p| n + p.width_frac)
}

/// the arms lie on backbone that SURVIVES, on both sides of the opening
#[test]
fn the_arms_lie_on_backbone_that_survives_on_both_sides_of_the_opening() {
    let l = layout_band(&band(), L, None);
    assert_eq!(l.arm_left.len(), 1);
    assert_eq!(l.arm_right.len(), 1);
    // left arm is the 30 bases ending just before the upstream cut: 270..299
    assert_eq!(l.arm_left[0].start_frac, 270.0 / LF);
    assert!((total(&l.arm_left) - 30.0 / LF).abs() < 1e-9, "30 bp of surviving backbone");
    // right arm is the 30 bases starting at the downstream cut: 400..429
    assert_eq!(l.arm_right[0].start_frac, 400.0 / LF);
    assert!((total(&l.arm_right) - 30.0 / LF).abs() < 1e-9);
    assert!(!l.clamped);
}

/// the body covers the opening, so the band reads as replacing exactly what leaves
#[test]
fn the_body_covers_the_opening_so_the_band_reads_as_replacing_exactly_what_leaves() {
    let l = layout_band(&band(), L, None);
    assert_eq!(l.body[0].start_frac, 300.0 / LF);
    assert!((total(&l.body) - 100.0 / LF).abs() < 1e-9, "the 100 bp being removed");
}

/// an opening across the origin comes back as two seam-cut pieces, like every other span
#[test]
fn an_opening_across_the_origin_comes_back_as_two_seam_cut_pieces_like_every_other_span() {
    let l = layout_band(&OuterBand { up: 950, down: 50, ..band() }, L, None);
    assert_eq!(removed_len(950, 50, L), 100, "wraps: 950..999 plus 0..49");
    assert_eq!(l.body.len(), 2, "split at the seam");
    assert!(l.body[0].cut_right && l.body[1].cut_left, "marked so it draws as one wrapped band");
    assert!((total(&l.body) - 100.0 / LF).abs() < 1e-9);
    // the left arm ends just before 950 and does not wrap
    assert_eq!(l.arm_left[0].start_frac, 920.0 / LF);
    // the right arm starts at 50
    assert_eq!(l.arm_right[0].start_frac, 50.0 / LF);
}

/// a pure insertion still draws a body — something goes in, at a point
#[test]
fn a_pure_insertion_still_draws_a_body_something_goes_in_at_a_point() {
    let l = layout_band(&OuterBand { up: 300, down: 300, ..band() }, L, None);
    assert_eq!(removed_len(300, 300, L), 0, "nothing removed");
    assert!(!l.body.is_empty(), "the band does not vanish");
    assert!(total(&l.body) > 0.0);
}

/// arms are clamped rather than wrapping the ring when the backbone barely survives
#[test]
fn arms_are_clamped_rather_than_wrapping_the_ring_when_the_backbone_barely_survives() {
    // opening removes 960 of 1000 → only 40 bp of backbone left to hold two 100 bp arms
    let l = layout_band(&OuterBand { up: 20, down: 980, left_arm_bp: 100, right_arm_bp: 100, ..band() }, L, None);
    assert!(l.clamped);
    assert!(total(&l.arm_left) + total(&l.arm_right) <= 40.0 / LF + 1e-9, "the arms fit in what is left");
}

/// an insert far longer than the plasmid does not widen the band
#[test]
fn an_insert_far_longer_than_the_plasmid_does_not_widen_the_band() {
    let small = layout_band(&OuterBand { insert_bp: 100, ..band() }, L, None);
    let huge = layout_band(&OuterBand { insert_bp: 500000, ..band() }, L, None);
    assert_eq!(huge.body, small.body, "the body follows the opening, never the insert size");
}

/// overlapFraction says how much of a feature is leaving, origin included
#[test]
fn overlap_fraction_says_how_much_of_a_feature_is_leaving_origin_included() {
    assert_eq!(overlap_fraction(310, 390, 300, 400, L), 1.0, "entirely inside");
    assert_eq!(overlap_fraction(100, 200, 300, 400, L), 0.0, "entirely outside");
    assert!((overlap_fraction(280, 319, 300, 400, L) - 0.5).abs() < 1e-9, "half in, half out");
    // both the feature and the opening cross the origin
    assert_eq!(overlap_fraction(960, 20, 950, 50, L), 1.0, "wrapped feature inside a wrapped opening");
    assert!(overlap_fraction(900, 20, 950, 50, L) < 1.0, "a wrapped feature only partly inside");
}

/// the dim threshold is a half, so a feature mostly leaving reads as leaving
#[test]
fn the_dim_threshold_is_a_half_so_a_feature_mostly_leaving_reads_as_leaving() {
    assert!(overlap_fraction(280, 319, 300, 400, L) >= DIM_AT);
    assert!(overlap_fraction(280, 350, 300, 400, L) >= DIM_AT);
    assert!(overlap_fraction(200, 310, 300, 400, L) < DIM_AT);
}

/// fragments divide the body in order, by length ratio
#[test]
fn fragments_divide_the_body_in_order_by_length_ratio() {
    let l = layout_band(
        &OuterBand { up: 300, down: 400, insert_bp: 400, segments: vec![segment("f1", 0, 100, "#a"), segment("f2", 100, 400, "#b")], ..band() },
        L,
        None,
    );
    assert_eq!(l.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["f1", "f2"], "in order, 5'→3'");
    // The body is the 100 bp opening; f1 is a quarter of the insert, so a quarter of the arc.
    assert!((total(&l.segments[0].pieces) - 25.0 / LF).abs() < 2e-3, "f1 takes a quarter");
    assert!((total(&l.segments[1].pieces) - 75.0 / LF).abs() < 2e-3, "f2 takes three quarters");
    let all = total(&l.segments[0].pieces) + total(&l.segments[1].pieces);
    assert!((all - total(&l.body)).abs() < 2e-3, "together they are the body, no more and no less");
}

/// a fragment's angular width follows the OPENING, never the insert's own length
#[test]
fn a_fragments_angular_width_follows_the_opening_never_the_inserts_own_length() {
    // The body belongs to what leaves; the fragments belong to what goes in. A 40 kb insert into a
    // 100 bp opening still draws a 100 bp band — otherwise it would run past its own cuts.
    let small = layout_band(&OuterBand { insert_bp: 400, segments: vec![segment("f", 0, 400, "#a")], ..band() }, L, None);
    let huge = layout_band(&OuterBand { insert_bp: 40000, segments: vec![segment("f", 0, 40000, "#a")], ..band() }, L, None);
    assert!((total(&small.segments[0].pieces) - total(&huge.segments[0].pieces)).abs() < 1e-9);
}

/// seams land exactly on the boundaries they name, ends included
#[test]
fn seams_land_exactly_on_the_boundaries_they_name_ends_included() {
    let l = layout_band(
        &OuterBand {
            up: 300,
            down: 400,
            insert_bp: 400,
            segments: vec![segment("f1", 0, 100, "#a"), segment("f2", 100, 400, "#b")],
            seams: vec![seam(0, 65.3, "backbone ↔ f1"), seam(100, 64.1, "f1 ↔ f2"), seam(400, 66.0, "f2 ↔ backbone")],
            ..band()
        },
        L,
        None,
    );
    assert_eq!(l.seams.iter().map(|s| s.tm).collect::<Vec<_>>(), [65.3, 64.1, 66.0]);
    assert!((l.seams[0].frac - 300.0 / LF).abs() < 1e-9, "the first sits at the upstream cut");
    assert!((l.seams[1].frac - 325.0 / LF).abs() < 1e-9, "the middle sits where f1 ends");
    assert!((l.seams[2].frac - 400.0 / LF).abs() < 1e-9, "the last sits at the downstream cut");
}

/// segments and seams survive an opening that crosses the origin
#[test]
fn segments_and_seams_survive_an_opening_that_crosses_the_origin() {
    let l = layout_band(
        &OuterBand {
            up: 950,
            down: 50,
            insert_bp: 200,
            segments: vec![segment("f1", 0, 100, "#a"), segment("f2", 100, 200, "#b")],
            seams: vec![seam(100, 64.0, "f1 ↔ f2")],
            ..band()
        },
        L,
        None,
    );
    assert_eq!(l.segments.len(), 2);
    let spanned = l.segments.iter().fold(0.0, |n, s| n + total(&s.pieces));
    assert!((spanned - 100.0 / LF).abs() < 2e-3, "together they still cover the 100 bp opening");
    assert!(l.seams[0].frac >= 0.0 && l.seams[0].frac < 1.0, "the seam is a real fraction, not negative");
}

/// eBlocks overlap on the band, and reach out over the arms
#[test]
fn eblocks_overlap_on_the_band_and_reach_out_over_the_arms() {
    // What you order is not a partition of the insert. Each piece carries the homology it shares
    // with its neighbour, and the outer two carry the backbone arms — so the first begins BEFORE
    // the insert does. Drawing them end to end would hide the only reason they join.
    let l = layout_band(
        &OuterBand {
            up: 300,
            down: 400,
            insert_bp: 400,
            left_arm_bp: 30,
            right_arm_bp: 30,
            segments: vec![
                BandSegment { tier: Some(0), ..segment("eB_1", -30, 220, "#a") },
                BandSegment { tier: Some(1), ..segment("eB_2", 180, 430, "#b") },
            ],
            ..band()
        },
        L,
        None,
    );
    assert_eq!(l.segments.iter().map(|s| s.tier).collect::<Vec<_>>(), [0, 1], "they are drawn on different rows");
    // eB_1 starts 30 bases before the upstream cut — out on backbone that stays.
    assert!(l.segments[0].pieces[0].start_frac < 300.0 / LF, "the first reaches back into the left arm");
    // eB_2 ends past the downstream cut, out on the right arm.
    let last = l.segments[1].pieces[l.segments[1].pieces.len() - 1];
    assert!(last.start_frac + last.width_frac > 400.0 / LF, "the last reaches out over the right arm");
    // And they genuinely share ground in the middle — that shared stretch IS the junction.
    let a_end = l.segments[0].pieces[0].start_frac + l.segments[0].pieces[0].width_frac;
    assert!(l.segments[1].pieces[0].start_frac < a_end, "they overlap rather than abut");
}

/// outside the insert the band is 1:1 with the parent, because out there it IS the parent
#[test]
fn outside_the_insert_the_band_is_1_1_with_the_parent_because_out_there_it_is_the_parent() {
    // Inside the opening the mapping is a ratio (the body's width belongs to the opening, the bases
    // to the insert). Outside it must not be: the arms lie over real backbone the map already draws
    // at its own scale, so a 30 bp arm has to cover 30 bp of ring.
    let l = layout_band(&OuterBand { up: 300, down: 400, insert_bp: 4000, segments: vec![segment("arm", -30, 0, "#a")], ..band() }, L, None);
    assert!((total(&l.segments[0].pieces) - 30.0 / LF).abs() < 2e-3, "30 bp of arm covers 30 bp of ring");
}

// ── THE REGRESSION THE MULTI-INSERT FIX EXISTS FOR ──────────────────────────────────────────────
// The build console used to size the band from EVERY insert while the segments came from the ONE
// design being previewed. That was right while an insert list meant "the fragments of one
// product"; the list means alternatives now — each row is its own plasmid — so the two frames
// disagreed and the eBlocks drew across a fraction of the opening with the closing seam short.
// Nothing threw. The map simply lied, exactly when you did the thing BUILD is for.
//
// The guarantee is a property of `layout_band`, so it is asserted as one: segments that span the
// insert they were told about fill the body, and segments that span less than they were told about
// visibly under-draw. The second is the bug, written down.

/// segments spanning their whole insert fill the body, at any insert length
#[test]
fn segments_spanning_their_whole_insert_fill_the_body_at_any_insert_length() {
    for insert_bp in [300, 700, 4000] {
        let l = layout_band(&OuterBand { insert_bp, segments: vec![segment("whole", 0, insert_bp, "#a")], ..band() }, L, None);
        assert!(
            (total(&l.segments[0].pieces) - total(&l.body)).abs() < 2e-3,
            "a {insert_bp} bp insert drew {} against a body of {}",
            total(&l.segments[0].pieces),
            total(&l.body)
        );
    }
}

/// a band told it carries MORE bases than its segments span under-draws the body
#[test]
fn a_band_told_it_carries_more_bases_than_its_segments_span_under_draws_the_body() {
    // 700 bp of segments in a band told it is carrying 2,100 draws across a third of the opening.
    let honest = layout_band(&OuterBand { insert_bp: 700, segments: vec![segment("one", 0, 700, "#a")], ..band() }, L, None);
    let lying = layout_band(&OuterBand { insert_bp: 2100, segments: vec![segment("one", 0, 700, "#a")], ..band() }, L, None);
    assert!(
        total(&lying.segments[0].pieces) < total(&honest.segments[0].pieces) / 2.0,
        "the mis-scaled band should visibly under-draw, which is what made the bug findable"
    );
}
