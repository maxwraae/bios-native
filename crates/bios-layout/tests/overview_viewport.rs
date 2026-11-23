//! Parity with Bios `editor/overview/viewport.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.
//!
//! Where the original leaves the anchor out (`zoomBy(vp, factor)`), this passes `None`.

use bios_layout::overview::viewport::{
    LINE_TO_RING, MAX_WIDTH, MIN_WIDTH, Projection, RING_TO_LINE, Viewport, char_width_for_width, clamp_center, nice_ticks, pan_by,
    projection_for_width, ring_delta, set_projection, set_width, split_span, visible_range, width_for_char_width, window_pieces, wrap1, zoom_by,
};

fn ring(center: f64, width: f64) -> Viewport {
    Viewport { center, width, projection: Projection::Ring }
}

fn line(center: f64, width: f64) -> Viewport {
    Viewport { center, width, projection: Projection::Line }
}

/// `approx(a, b)`: the original's default tolerance.
fn approx(a: f64, b: f64) -> bool {
    approx_eps(a, b, 1e-9)
}

fn approx_eps(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

/// projectionForWidth: a linear molecule is always a line, at every zoom
#[test]
fn projection_for_width_a_linear_molecule_is_always_a_line_at_every_zoom() {
    for w in [1.0, 0.8, 0.5, 0.2, MIN_WIDTH] {
        assert_eq!(projection_for_width(w, false, Projection::Ring), Projection::Line);
        assert_eq!(projection_for_width(w, false, Projection::Line), Projection::Line);
    }
}

/// projectionForWidth: a circular map flips ring→line as you zoom in, with hysteresis
#[test]
fn projection_for_width_a_circular_map_flips_ring_to_line_as_you_zoom_in_with_hysteresis() {
    // Coming from a ring: stays a ring until you're zoomed in past RING_TO_LINE.
    assert_eq!(projection_for_width(1.0, true, Projection::Ring), Projection::Ring);
    assert_eq!(projection_for_width(RING_TO_LINE + 0.01, true, Projection::Ring), Projection::Ring);
    assert_eq!(projection_for_width(RING_TO_LINE - 0.01, true, Projection::Ring), Projection::Line);
    // Coming from a line: stays a line until you zoom back out past LINE_TO_RING (> RING_TO_LINE).
    const { assert!(LINE_TO_RING > RING_TO_LINE) }; // the hysteresis gap
    assert_eq!(projection_for_width(LINE_TO_RING - 0.01, true, Projection::Line), Projection::Line);
    assert_eq!(projection_for_width(LINE_TO_RING + 0.01, true, Projection::Line), Projection::Ring);
    // In the gap the current projection is held (no flicker at the boundary).
    let mid = (RING_TO_LINE + LINE_TO_RING) / 2.0;
    assert_eq!(projection_for_width(mid, true, Projection::Ring), Projection::Ring);
    assert_eq!(projection_for_width(mid, true, Projection::Line), Projection::Line);
}

/// wrap1 folds into [0,1)
#[test]
fn wrap1_folds_into_0_1() {
    assert_eq!(wrap1(1.25), 0.25);
    assert_eq!(wrap1(-0.25), 0.75);
    assert_eq!(wrap1(0.0), 0.0);
}

/// ringDelta takes the short way round the origin
#[test]
fn ring_delta_takes_the_short_way_round_the_origin() {
    assert!(approx(ring_delta(0.95, 0.05), 0.1)); // forward across the seam, not -0.9
    assert!(approx(ring_delta(0.05, 0.95), -0.1));
}

/// clampCenter: ring wraps, line clamps to keep the window on-screen
#[test]
fn clamp_center_ring_wraps_line_clamps_to_keep_the_window_on_screen() {
    assert!(approx(clamp_center(1.1, 0.2, Projection::Ring), 0.1));
    assert_eq!(clamp_center(0.0, 0.4, Projection::Line), 0.2); // can't center at 0 with 40% visible
    assert_eq!(clamp_center(1.0, 0.4, Projection::Line), 0.8);
    assert_eq!(clamp_center(0.5, 0.4, Projection::Line), 0.5);
}

/// panBy wraps on a ring and clamps on a line
#[test]
fn pan_by_wraps_on_a_ring_and_clamps_on_a_line() {
    assert!(approx(pan_by(ring(0.95, 0.2), 0.1).center, 0.05));
    assert_eq!(pan_by(line(0.8, 0.4), 0.5).center, 0.8); // clamped at 1 - width/2
}

/// setWidth respects MIN/MAX and re-clamps a line center
#[test]
fn set_width_respects_min_max_and_re_clamps_a_line_center() {
    assert_eq!(set_width(line(0.5, 0.2), 5.0).width, MAX_WIDTH);
    assert_eq!(set_width(line(0.5, 0.2), 0.0).width, MIN_WIDTH);
    // Widening near an edge pushes the center back inside.
    let w = set_width(line(0.9, 0.1), 0.6);
    assert_eq!(w.center, 0.7);
}

/// zoomBy on a line keeps the anchored bp under the cursor
#[test]
fn zoom_by_on_a_line_keeps_the_anchored_bp_under_the_cursor() {
    let vp = line(0.5, 0.4); // window [0.30, 0.70]
    let anchor = 0.6; // right-of-center bp
    let z = zoom_by(vp, 0.5, Some(anchor)); // width 0.4 -> 0.2
    // anchor's on-screen fraction s = (0.6-0.30)/0.4 = 0.75; must be preserved
    let left = z.center - z.width / 2.0;
    let s_after = (anchor - left) / z.width;
    assert!(approx_eps(s_after, 0.75, 1e-9));
    assert!(approx(z.width, 0.2));
}

/// zoomBy on a ring zooms about center (no anchor drift)
#[test]
fn zoom_by_on_a_ring_zooms_about_center_no_anchor_drift() {
    let z = zoom_by(ring(0.42, 0.8), 0.5, Some(0.9));
    assert!(approx(z.center, 0.42)); // center unchanged despite anchor arg
    assert!(approx(z.width, 0.4));
}

/// zoomBy clamps at the extremes
#[test]
fn zoom_by_clamps_at_the_extremes() {
    assert_eq!(zoom_by(ring(0.5, 0.8), 10.0, None).width, MAX_WIDTH);
    assert_eq!(zoom_by(line(0.5, 0.02), 0.01, None).width, MIN_WIDTH);
}

/// setProjection preserves the look but re-clamps for the line
#[test]
fn set_projection_preserves_the_look_but_re_clamps_for_the_line() {
    let p = set_projection(ring(0.02, 0.3), Projection::Line); // center 0.02 illegal for a line at 30%
    assert_eq!(p.projection, Projection::Line);
    assert_eq!(p.center, 0.15);
}

/// visibleRange: simple line window, 0-based inclusive
#[test]
fn visible_range_simple_line_window_0_based_inclusive() {
    let r = visible_range(line(0.5, 0.25), 800); // 200 bp centered at 400
    assert_eq!(r.span, 200);
    assert_eq!(r.start_bp, 300);
    assert_eq!(r.end_bp, 499);
    assert!(!r.wraps);
}

/// visibleRange: ring window straddling the origin wraps
#[test]
fn visible_range_ring_window_straddling_the_origin_wraps() {
    let r = visible_range(ring(0.0, 0.1), 800); // 80 bp centered on bp 0
    assert_eq!(r.span, 80);
    assert!(r.wraps);
    assert!(r.start_bp > r.end_bp); // e.g. 760..39
}

/// splitSpan: normal span is one piece
#[test]
fn split_span_normal_span_is_one_piece() {
    let s = split_span(100, 199, 800);
    assert_eq!(s.len(), 1);
    assert!(approx(s[0].start_frac, 100.0 / 800.0));
    assert!(approx(s[0].width_frac, 100.0 / 800.0)); // (199-100+1)=100 bases
}

/// splitSpan: origin-spanning span is two pieces that meet at the seam
#[test]
fn split_span_origin_spanning_span_is_two_pieces_that_meet_at_the_seam() {
    let s = split_span(790, 9, 800); // wraps: 790..799 then 0..9 (20 bases)
    assert_eq!(s.len(), 2);
    assert!(approx(s[0].start_frac, 790.0 / 800.0));
    assert!(approx(s[0].width_frac, 10.0 / 800.0));
    assert!(s[0].cut_right);
    assert!(approx(s[1].start_frac, 0.0));
    assert!(approx(s[1].width_frac, 10.0 / 800.0));
    assert!(s[1].cut_left);
    // total visible width = 20 bases
    assert!(approx((s[0].width_frac + s[1].width_frac) * 800.0, 20.0));
}

/// windowPieces: full width is a single seamless piece
#[test]
fn window_pieces_full_width_is_a_single_seamless_piece() {
    let p = window_pieces(0.5, 1.0, true);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].width, 1.0);
}

/// windowPieces: circular window across origin splits into two seam-cut pieces
#[test]
fn window_pieces_circular_window_across_origin_splits_into_two_seam_cut_pieces() {
    let p = window_pieces(0.0, 0.2, true); // [-0.1,0.1] -> [0.9,1] and [0,0.1]
    assert_eq!(p.len(), 2);
    assert!(approx(p[0].left, 0.9));
    assert!(p[0].cut_right && p[0].left_edge);
    assert!(approx(p[1].left, 0.0));
    assert!(p[1].cut_left && p[1].right_edge);
}

/// windowPieces: linear window never wraps, clamps to the ends
#[test]
fn window_pieces_linear_window_never_wraps_clamps_to_the_ends() {
    let p = window_pieces(0.0, 0.2, false);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].left, 0.0); // clamped
    assert!(p[0].left_edge && p[0].right_edge);
}

/// niceTicks lands on round numbers spanning the molecule
#[test]
fn nice_ticks_lands_on_round_numbers_spanning_the_molecule() {
    let t = nice_ticks(800); // target 200 -> step 200
    assert_eq!(t, [200.0, 400.0, 600.0]);
    let t2 = nice_ticks(171); // target ~43 -> step 50
    assert_eq!(t2, [50.0, 100.0, 150.0]);
}

/// charWidth inversion round-trips (proves the OVE hand-off is lossless)
#[test]
fn char_width_inversion_round_trips_proves_the_ove_hand_off_is_lossless() {
    let client_px = 900.0;
    let l = 5000;
    for width in [1.0, 0.5, 0.2, 0.05, MIN_WIDTH] {
        let cw = char_width_for_width(width, client_px, l);
        let back = width_for_char_width(cw, client_px, l);
        assert!(approx_eps(back, width, 1e-9), "width {width} -> cw {cw} -> {back}");
    }
}

/// frac↔bp↔frac round-trips exactly at integer bp (selection-loop guard)
#[test]
fn frac_bp_frac_round_trips_exactly_at_integer_bp_selection_loop_guard() {
    let l: i64 = 800;
    let mut bp = 0;
    while bp < l {
        let frac = bp as f64 / l as f64;
        let back = (frac * l as f64).round() as i64;
        assert_eq!(back, bp);
        bp += 37;
    }
}
