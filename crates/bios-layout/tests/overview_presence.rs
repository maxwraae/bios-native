//! Parity with Bios `editor/overview/presence.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.

use bios_layout::overview::presence::{MIN_MARK_PX, SEAM_PX, arc_mark, arc_seam, is_mark, seam_span, widen_to_mark};

// Two rules that pull in opposite directions and meet here. The mark floor makes a span WIDER so a
// 3bp feature is still visible; the seam makes it NARROWER so two neighbours don't fuse. Get the
// order or the clamp wrong and the seam eats the very features the floor exists to rescue — they
// would still draw, so nothing would fail except the map quietly dropping its smallest annotations.

/// the floor widens a sub-pixel span and leaves an honest one alone
#[test]
fn the_floor_widens_a_sub_pixel_span_and_leaves_an_honest_one_alone() {
    assert!(is_mark(10.0, 11.0));
    assert!(!is_mark(10.0, 40.0));
    // Widened about its CENTRE, so the tick stays on the feature's own coordinate.
    assert_eq!(widen_to_mark(10.0, 11.0), (10.5 - MIN_MARK_PX / 2.0, 10.5 + MIN_MARK_PX / 2.0));
    assert_eq!(widen_to_mark(10.0, 40.0), (10.0, 40.0));
}

/// the seam is taken evenly off both ends, so the drawn centre stays put
#[test]
fn the_seam_is_taken_evenly_off_both_ends_so_the_drawn_centre_stays_put() {
    let (x0, x1) = seam_span(100.0, 200.0);
    assert_eq!(x1 - x0, 100.0 - SEAM_PX);
    assert_eq!((x0 + x1) / 2.0, 150.0);
}

/// two abutting features end up a full seam apart
#[test]
fn two_abutting_features_end_up_a_full_seam_apart() {
    // The case the whole thing exists for: same role, same colour, touching at 200.
    let (_, a_end) = seam_span(100.0, 200.0);
    let (b_start, _) = seam_span(200.0, 300.0);
    assert_eq!(b_start - a_end, SEAM_PX);
}

/// the seam never eats a mark
#[test]
fn the_seam_never_eats_a_mark() {
    // A span already at the floor gives up nothing — it is the smallest thing that still says
    // "something is here", and a seam taken out of it would put it back under the floor.
    let (x0, x1) = seam_span(50.0, 50.0 + MIN_MARK_PX);
    assert_eq!(x1 - x0, MIN_MARK_PX);
}

/// a span just above the floor loses only its slack, not its presence
#[test]
fn a_span_just_above_the_floor_loses_only_its_slack_not_its_presence() {
    // Clamped, not all-or-nothing: between the floor and floor+seam the feature gives up what it can
    // spare and stops there, rather than snapping back to full width at one particular size.
    let w = MIN_MARK_PX + 1.0;
    let (x0, x1) = seam_span(0.0, w);
    assert_eq!(x1 - x0, MIN_MARK_PX);
    assert_eq!((x0 + x1) / 2.0, w / 2.0);
}

/// a span already below the floor is never grown by the seam
#[test]
fn a_span_already_below_the_floor_is_never_grown_by_the_seam() {
    // arcMark should have widened it first, but the guard matters: seamSpan must not be a way to
    // silently ENLARGE a span, which is the one thing neither rule is allowed to do.
    let (x0, x1) = seam_span(0.0, 1.0);
    assert_eq!(x1 - x0, 1.0);
}

/// arcSeam is the same rule in fractions of a turn
#[test]
fn arc_seam_is_the_same_rule_in_fractions_of_a_turn() {
    let seam = 0.002;
    let floor = 0.001;
    let s = arc_seam(0.25, 0.5, seam, floor);
    assert!((s.draw_width - (0.5 - seam)).abs() < 1e-12);
    // Centre held: 0.25 + 0.5/2 === drawStart + drawWidth/2
    assert!((s.draw_start + s.draw_width / 2.0 - 0.5).abs() < 1e-12);
}

/// floor then seam: a tiny feature survives the pair intact
#[test]
fn floor_then_seam_a_tiny_feature_survives_the_pair_intact() {
    let floor = 0.01;
    let seam = 0.004;
    // A feature far under the floor — widened to it, then offered to the seam, which must decline.
    let marked = arc_mark(0.3, 0.0001, floor);
    assert!(marked.mark);
    assert!((marked.draw_width - floor).abs() < 1e-12);
    let s = arc_seam(marked.draw_start, marked.draw_width, seam, floor);
    assert!((s.draw_width - floor).abs() < 1e-12);
}
