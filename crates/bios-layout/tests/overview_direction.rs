//! Parity with Bios `editor/overview/direction.test.ts`: one test here per test there, in the same
//! order, with the same assertions and the same numbers.

use bios_layout::overview::direction::{HEAD_PX, arrow_frac, fits_head, head_path, head_span};

// The direction mark is the one thing on the map that can be silently, invisibly BACKWARDS: flip it
// and every shape still draws, every test still passes, and the map quietly lies about which way a
// gene reads. These pin the rule so that flip has to be deliberate.

/// the mark sits at the 5' end of the piece, not the 3'
#[test]
fn the_mark_sits_at_the_5_end_of_the_piece_not_the_3() {
    // A feature from 0.2 to 0.5 of the way round. Forward reads 0.2 -> 0.5, so the mark is at 0.2.
    let fwd = head_span(0.2, 0.3, 0.01, true);
    assert_eq!(fwd.base_frac, 0.2);
    // Reverse reads 0.5 -> 0.2, so it is at 0.5 — the same physical end a 3' rule would put it on for
    // the forward case, which is exactly why "left vs right" is the wrong way to state this.
    let rev = head_span(0.2, 0.3, 0.01, false);
    assert_eq!(rev.base_frac, 0.5);
}

/// the tip is always downstream of the base — that is what makes it read > or <
#[test]
fn the_tip_is_always_downstream_of_the_base_that_is_what_makes_it_read_gt_or_lt() {
    let fwd = head_span(0.2, 0.3, 0.01, true);
    assert!(fwd.tip_frac > fwd.base_frac, "forward points toward higher coordinates");
    let rev = head_span(0.2, 0.3, 0.01, false);
    assert!(rev.tip_frac < rev.base_frac, "reverse points toward lower coordinates");
}

/// the mark stays inside the piece, so the drawn extent never overstates the feature
#[test]
fn the_mark_stays_inside_the_piece_so_the_drawn_extent_never_overstates_the_feature() {
    for forward in [true, false] {
        let span = head_span(0.2, 0.3, 0.05, forward);
        for v in [span.base_frac, span.tip_frac] {
            assert!((0.2..=0.5).contains(&v), "{v} escaped the piece [0.2, 0.5]");
        }
    }
}

/// headPath is headSpan's straight-line twin — same end, same direction
#[test]
fn head_path_is_head_spans_straight_line_twin_same_end_same_direction() {
    // Base is the flat side across the bar; tip is the apex on the centre line.
    assert_eq!(head_path(100.0, 300.0, 50.0, 6.0, true), "M100 44L107 50L100 56Z");
    assert_eq!(head_path(100.0, 300.0, 50.0, 6.0, false), "M300 44L293 50L300 56Z");
}

/// headPath's apex is HEAD_PX from the edge it hangs off
#[test]
fn head_paths_apex_is_head_px_from_the_edge_it_hangs_off() {
    let apex_x = |d: String| -> f64 { d.split('L').nth(1).unwrap().split(' ').next().unwrap().parse().unwrap() };
    assert_eq!(apex_x(head_path(100.0, 300.0, 50.0, 6.0, true)) - 100.0, HEAD_PX);
    assert_eq!(300.0 - apex_x(head_path(100.0, 300.0, 50.0, 6.0, false)), HEAD_PX);
}

/// a piece too narrow to hold a head is left unmarked rather than swallowed by one
#[test]
fn a_piece_too_narrow_to_hold_a_head_is_left_unmarked_rather_than_swallowed_by_one() {
    assert!(!fits_head(HEAD_PX, HEAD_PX));
    assert!(!fits_head(HEAD_PX * 1.6, HEAD_PX), "the threshold itself does not qualify");
    assert!(fits_head(HEAD_PX * 1.61, HEAD_PX));
}

/// arrowFrac converts px to a circle fraction, and cannot divide by a zero radius
#[test]
fn arrow_frac_converts_px_to_a_circle_fraction_and_cannot_divide_by_a_zero_radius() {
    // A 7px head on a radius-100 ring is 7 / (2*pi*100) of the way round.
    assert!((arrow_frac(HEAD_PX, 100.0) - HEAD_PX / (2.0 * std::f64::consts::PI * 100.0)).abs() < 1e-12);
    // Inner lanes can collapse to r=0; the guard keeps this finite rather than Infinity, which would
    // otherwise make fitsHead reject every feature on that lane instead of just the narrow ones.
    assert!(arrow_frac(HEAD_PX, 0.0).is_finite());
}
