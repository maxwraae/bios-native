//! Parity with Bios `editor/overview/projection.test.ts`: one test here per test there, in the
//! same order, with the same assertions and the same numbers.
//!
//! Where the original leaves an argument out (`ringGeom(width, W, H)`, `ringPoint(frac, center, g)`,
//! `morphPoint(..., marginX)`), this passes `None`, which is the original's parameter default.

use bios_layout::overview::projection::{
    MORPH_CIRC, MORPH_LINE, TAU, line_window, line_x, morph_point, point_to_frac_line, point_to_frac_ring, ring_geom, ring_point, straightness,
};
use bios_layout::overview::viewport::{Projection, Viewport};

/// `approx(a, b)`: the original's default tolerance.
fn approx(a: f64, b: f64) -> bool {
    approx_eps(a, b, 1e-6)
}

fn approx_eps(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

const W: f64 = 400.0;
const H: f64 = 400.0;

/// ringGeom: width=1 is the fit circle, centred
#[test]
fn ring_geom_width_1_is_the_fit_circle_centred() {
    let g = ring_geom(1.0, W, H, None);
    assert!(approx(g.r, g.r0));
    assert!(approx(g.cyc, H / 2.0)); // circle centre at frame centre
    assert!(approx(g.cx, W / 2.0));
}

/// ringGeom: zooming in grows the radius and drops the centre off-screen
#[test]
fn ring_geom_zooming_in_grows_the_radius_and_drops_the_centre_off_screen() {
    let g = ring_geom(0.25, W, H, None);
    assert!(approx(g.r, g.r0 * 4.0)); // R = R0/width
    assert!(g.cyc > H); // circle centre well below the frame
}

/// ringGeom: the viewed arc migrates from the top toward the vertical middle as you zoom in
#[test]
fn ring_geom_the_viewed_arc_migrates_from_the_top_toward_the_vertical_middle_as_you_zoom_in() {
    let peak_y = |width: f64| {
        let g = ring_geom(width, W, H, None);
        ring_point(0.3, 0.3, g, None).y // frac === center → 12 o'clock, the top of the viewed arc
    };
    let full = peak_y(1.0); // whole ring in frame: arc rides the top
    let mid = peak_y(0.5);
    let deep = peak_y(0.02); // zoomed way in
    assert!(approx(full, H / 2.0 - ring_geom(1.0, W, H, None).r0)); // top edge at full view
    assert!(mid > full); // has moved down toward centre
    assert!(deep > mid); // and further still
    assert!((deep - H / 2.0).abs() < 5.0); // essentially at the vertical middle when zoomed in
}

/// ringPoint: center bp sits at 12 o'clock; quarter turn is 3 o'clock
#[test]
fn ring_point_center_bp_sits_at_12_oclock_quarter_turn_is_3_oclock() {
    let g = ring_geom(1.0, W, H, None);
    let top = ring_point(0.3, 0.3, g, None); // frac === center
    assert!(approx(top.x, g.cx));
    assert!(approx(top.y, g.cyc - g.r)); // top of circle
    let right = ring_point(0.55, 0.3, g, None); // +0.25 turn clockwise
    assert!(approx(right.x, g.cx + g.r));
    assert!(approx_eps(right.y, g.cyc, 1e-4));
}

/// pointToFracRing inverts ringPoint (round-trip across the ring)
#[test]
fn point_to_frac_ring_inverts_ring_point_round_trip_across_the_ring() {
    let g = ring_geom(0.6, W, H, None);
    let center = 0.42;
    for frac in [0.42, 0.5, 0.7, 0.1, 0.95] {
        let p = ring_point(frac, center, g, None);
        let back = point_to_frac_ring(p.x, p.y, center, g);
        // compare on the circle (mod 1)
        let d = (back - frac).abs().min(1.0 - (back - frac).abs());
        assert!(d < 1e-6, "frac {frac} -> {back}");
    }
}

/// lineWindow clamps so the line never overhangs an end
#[test]
fn line_window_clamps_so_the_line_never_overhangs_an_end() {
    let vp = Viewport { center: 0.0, width: 0.4, projection: Projection::Line };
    let w = line_window(vp);
    assert_eq!(w.left, 0.0);
    assert!(approx(w.right, 0.4));
}

/// lineX maps the window edges to the padded frame; pointToFracLine inverts it
#[test]
fn line_x_maps_the_window_edges_to_the_padded_frame_point_to_frac_line_inverts_it() {
    let margin = 20.0;
    let left = 0.3;
    let width = 0.4;
    assert!(approx(line_x(left, left, width, W, margin), margin)); // left edge -> left pad
    assert!(approx(line_x(left + width, left, width, W, margin), W - margin)); // right edge -> right pad
    let px = 250.0;
    let frac = point_to_frac_line(px, left, width, W, margin);
    assert!(approx(line_x(frac, left, width, W, margin), px));
}

/// straightness: 0 while circular, 1 once linear, monotonic smoothstep between
#[test]
fn straightness_0_while_circular_1_once_linear_monotonic_smoothstep_between() {
    assert_eq!(straightness(1.0), 0.0);
    assert_eq!(straightness(MORPH_CIRC), 0.0);
    assert_eq!(straightness(MORPH_LINE), 1.0);
    assert_eq!(straightness(0.05), 1.0);
    let mid = straightness((MORPH_CIRC + MORPH_LINE) / 2.0);
    assert!(mid > 0.0 && mid < 1.0);
    assert!(approx_eps(mid, 0.5, 1e-9)); // smoothstep is 0.5 at the midpoint
    // monotonic: more zoomed in (smaller width) is never less straight
    let mut prev = -1.0;
    let mut w = MORPH_CIRC;
    while w >= MORPH_LINE {
        let s = straightness(w);
        assert!(s >= prev);
        prev = s;
        w -= 0.02;
    }
}

/// morphPoint: t=0 is the ring point, t=1 lands on the straightened line
#[test]
fn morph_point_t_0_is_the_ring_point_t_1_lands_on_the_straightened_line() {
    let (w, h) = (400.0, 400.0);
    let width = 0.4;
    let center = 0.3;
    let g = ring_geom(width, w, h, None);
    let frac = 0.34;
    let at0 = morph_point(frac, center, width, 0.0, w, h, g, 20.0, None, None);
    let ring = ring_point(frac, center, g, None);
    assert!(approx(at0.x, ring.x) && approx(at0.y, ring.y)); // t=0 === the ring

    let at1 = morph_point(frac, center, width, 1.0, w, h, g, 20.0, None, None);
    assert!(approx(at1.y, h / 2.0)); // t=1 lands on the horizontal baseline
    // the centered base maps to mid-frame; a base at the window edge maps to the frame edge
    let mid = morph_point(center, center, width, 1.0, w, h, g, 20.0, None, None);
    assert!(approx(mid.x, w / 2.0));
    let edge = morph_point(center + width / 2.0, center, width, 1.0, w, h, g, 20.0, None, None);
    assert!(approx(edge.x, w - 20.0)); // right content edge (marginX = 20)
}

/// TAU is a full turn
#[test]
fn tau_is_a_full_turn() {
    assert!(approx(TAU, 2.0 * std::f64::consts::PI));
}
