//! Parity with Bios `editor/overview/chords.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.
//!
//! Where the original leaves `bend` or `size` out, this passes `None`. The two regular expressions
//! of the original are spelled out by hand, because this crate has no dependencies.

use bios_layout::overview::chords::{BEND, Pt, arrow_head, chord_apex, chord_path, mid_frac};

fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

/// `[-\d.]+`: a run of digits, dots and minus signs, and nothing else.
fn is_number_run(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-')
}

/// `-?[\d.]+`, whole token: an optional minus, then digits and dots.
fn is_signed_number(s: &str) -> bool {
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// `[...d.matchAll(/(-?[\d.]+) (-?[\d.]+)/g)].map((m) => Number(m[1]))`: the first number of every
/// "x y" pair in a path, left to right.
fn pair_xs(d: &str) -> Vec<f64> {
    let tokens: Vec<&str> = d.split(' ').collect();
    let mut xs = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        if is_signed_number(tokens[i]) && is_signed_number(tokens[i + 1]) {
            xs.push(tokens[i].parse().unwrap());
            i += 2;
        } else {
            i += 1;
        }
    }
    xs
}

/// the apex is pulled toward the centre, not left on the midpoint
#[test]
fn the_apex_is_pulled_toward_the_centre_not_left_on_the_midpoint() {
    // Two points on opposite sides of a circle centred at (0,0), radius 100.
    let a = pt(-100.0, 0.0);
    let b = pt(100.0, 0.0);
    let c = chord_apex(a, b, 0.0, 0.0, None);
    assert_eq!(c.x, 0.0);
    assert_eq!(c.y, 0.0, "a chord across the middle bends to the centre itself");
}

/// neighbouring features barely leave the ring; opposite ones dip deep
#[test]
fn neighbouring_features_barely_leave_the_ring_opposite_ones_dip_deep() {
    let near = chord_apex(pt(0.0, -100.0), pt(20.0, -98.0), 0.0, 0.0, None);
    let far = chord_apex(pt(0.0, -100.0), pt(0.0, 100.0), 0.0, 0.0, None);
    let depth = |p: Pt| p.x.hypot(p.y);
    assert!(depth(near) > depth(far), "a short chord should stay near the rim");
    assert!(depth(near) > 40.0, "a neighbour chord dipped to {:.1}", depth(near));
}

/// bend 0 is a straight line through the midpoint
#[test]
fn bend_0_is_a_straight_line_through_the_midpoint() {
    let c = chord_apex(pt(-100.0, 0.0), pt(100.0, 0.0), 0.0, 50.0, Some(0.0));
    assert_eq!(c, pt(0.0, 0.0));
    assert!(chord_path(pt(-1.0, 0.0), pt(1.0, 0.0), 0.0, 0.0, Some(0.0)).starts_with("M -1 0 Q 0 0 1 0"));
}

/// the path is a quadratic from one end to the other
#[test]
fn the_path_is_a_quadratic_from_one_end_to_the_other() {
    let d = chord_path(pt(1.0, 2.0), pt(3.0, 4.0), 0.0, 0.0, None);
    // /^M 1 2 Q [-\d.]+ [-\d.]+ 3 4$/
    let apex: Vec<&str> =
        d.strip_prefix("M 1 2 Q ").and_then(|rest| rest.strip_suffix(" 3 4")).map(|mid| mid.split(' ').collect()).unwrap_or_default();
    assert!(apex.len() == 2 && apex.iter().all(|s| is_number_run(s)), "{d}");
    assert!(!d.contains("NaN"));
}

/// the arrowhead points away from where the chord came from
#[test]
fn the_arrowhead_points_away_from_where_the_chord_came_from() {
    // Arriving from the left, the tip's back corners must be to its LEFT.
    let d = arrow_head(pt(100.0, 0.0), pt(0.0, 0.0), Some(6.0));
    let xs = pair_xs(&d);
    assert_eq!(xs[0], 100.0, "the tip is at the point acted on");
    assert!(xs[1] < 100.0 && xs[2] < 100.0, "the body trails behind the tip");
}

/// a degenerate arrowhead still draws, rather than emitting NaN
#[test]
fn a_degenerate_arrowhead_still_draws_rather_than_emitting_nan() {
    let d = arrow_head(pt(5.0, 5.0), pt(5.0, 5.0), None);
    assert!(!d.contains("NaN"), "{d}");
}

/// the middle of a feature that crosses the origin is on the right side of the plasmid
#[test]
fn the_middle_of_a_feature_that_crosses_the_origin_is_on_the_right_side_of_the_plasmid() {
    // 90..10 on a 100 bp circle is 20 bases long and its middle is 0, not 50.
    assert_eq!(mid_frac(90, 10, 100), 0.0);
    assert_eq!(mid_frac(10, 30, 100), 0.2);
    assert_eq!(mid_frac(0, 0, 0), 0.0, "a zero-length molecule does not divide by zero");
}

/// BEND is a fraction, not a pixel count
#[test]
fn bend_is_a_fraction_not_a_pixel_count() {
    const { assert!(BEND > 0.0 && BEND < 1.0) };
}
