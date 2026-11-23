//! The ring, against what the Bios web app drew.
//!
//! `renderRing` and `layoutRingAnnotations` have no tests of their own in the web app: they are
//! drawing code. So the recorded answer here is the drawing itself: for three constructs, every
//! tick, arc, direction mark, name along an arc and flag the web app's map put in a 1440 x 378
//! frame, read out of its SVG (see `support`). The layout is handed the web app's own text widths
//! and has to arrive at the same picture.

mod support;

use bios_layout::overview::arc_text::arc_glyphs;
use bios_layout::overview::feature_geometry::feature_pieces;
use bios_layout::overview::projection::{Point, ring_arc_path};
use bios_layout::overview::ring_map::{Anchor, ArcLabel, RingMap, layout_ring, ring_margin};
use bios_layout::overview::scene::Frame;
use support::{Case, all_close, close, number, path_numbers};

/// The ring for the recorded frame, with `center` (a fraction of the molecule) at twelve o'clock.
fn ring_turned(case: &Case, center: f64) -> RingMap {
    let frame = case.drawn("ring", "frame");
    layout_ring(&case.scene, Frame { center, ..Frame::ring(number(&frame[0][0]), number(&frame[0][1])) }, case.measure())
}

fn ring_of(case: &Case) -> RingMap {
    ring_turned(case, 0.0)
}

fn xy(points: &[Point]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.x, p.y]).collect()
}

/// Everything about the ring that does not depend on a name: the axis, the scale, the arcs.
fn assert_ring_shapes(case: &Case, map: &RingMap) {
    let axis = &case.drawn("ring", "axis")[0];
    assert!(all_close(&[map.geom.cx, map.geom.cyc, map.axis_r], &[number(&axis[1]), number(&axis[2]), number(&axis[3])]), "axis {axis:?}");

    // tick: x1 y1 x2 y2 major|minor
    let ticks = case.drawn("ring", "tick");
    assert_eq!(map.ticks.len(), ticks.len(), "ticks");
    for (tick, want) in map.ticks.iter().zip(&ticks) {
        let coords: Vec<f64> = want[..4].iter().map(|v| number(v)).collect();
        assert!(all_close(&xy(&[tick.inner, tick.outer]), &coords), "tick {want:?}, got {tick:?}");
        assert_eq!(tick.major, want[4] == "major", "tick {want:?}");
    }

    // number: x y start|middle|end text
    let numbers = case.drawn("ring", "number");
    let got: Vec<_> = map.ticks.iter().filter_map(|t| t.number.as_ref()).collect();
    assert_eq!(got.len(), numbers.len(), "numbers");
    for (n, want) in got.iter().zip(&numbers) {
        assert!(all_close(&[n.at.x, n.at.y], &[number(&want[0]), number(&want[1])]), "number {want:?}, got {n:?}");
        let anchor = match n.anchor {
            Anchor::Start => "start",
            Anchor::Middle => "middle",
            Anchor::End => "end",
        };
        assert_eq!((anchor, n.text.as_str()), (want[2].as_str(), want[3].as_str()));
    }

    // band: feature d wide|mark
    let bands = case.drawn("ring", "band");
    let arcs: Vec<_> = map.arcs.iter().filter(|a| !a.connector).collect();
    assert_eq!(arcs.len(), bands.len(), "bands");
    for (arc, want) in arcs.iter().zip(&bands) {
        let d = ring_arc_path(arc.start_frac, arc.start_frac + arc.width_frac, map.center, map.geom, arc.radius);
        assert_eq!((arc.feature.to_string(), d, arc.mark), (want[0].clone(), want[1].clone(), want[2] == "mark"));
    }

    // head: feature d, where d is one corner of the base, the tip, the other corner
    let heads = case.drawn("ring", "head");
    let got: Vec<_> = map.arcs.iter().filter_map(|a| a.head.map(|h| (a.feature, h))).collect();
    assert_eq!(got.len(), heads.len(), "heads");
    for ((feature, head), want) in got.iter().zip(&heads) {
        assert_eq!(feature.to_string(), want[0]);
        assert!(all_close(&xy(head), &path_numbers(&want[1])), "head {want:?}, got {head:?}");
    }
}

/// The path the web app lays a name along: the feature's own sweep, at the name's radius, run
/// backwards on the bottom of the ring.
fn name_path(case: &Case, map: &RingMap, label: &ArcLabel) -> String {
    let piece = feature_pieces(&case.scene.features[label.feature], case.scene.length)[0];
    let (a, b) = (piece.start_frac, piece.start_frac + piece.width_frac);
    assert!(close(label.frac, (a + b) / 2.0), "a name sits at the middle of its arc");
    if label.upright { ring_arc_path(a, b, map.center, map.geom, label.radius) } else { ring_arc_path(b, a, map.center, map.geom, label.radius) }
}

/// The names: which are written along their own arc and how, and where the flags of the rest sit.
fn assert_ring_names(case: &Case, map: &RingMap) {
    // arcname: feature d text
    let names = case.drawn("ring", "arcname");
    assert_eq!(map.arc_labels.len(), names.len(), "names along arcs: {:?}", map.arc_labels);
    for (label, want) in map.arc_labels.iter().zip(&names) {
        assert_eq!(
            (label.feature.to_string(), name_path(case, map, label), label.text.clone(), label.font),
            (want[0].clone(), want[1].clone(), want[2].clone(), 11.0)
        );
    }

    // flag: x y w h x1 y1 x2 y2 text, the box and then the leader
    let flags = case.drawn("ring", "flag");
    assert_eq!(map.flags.len(), flags.len(), "flags: {:?}", map.flags);
    for (flag, want) in map.flags.iter().zip(&flags) {
        let recorded: Vec<f64> = want[..8].iter().map(|v| number(v)).collect();
        let (from, to) = flag.leader;
        let got = [flag.x, flag.y - flag.h / 2.0, flag.w, flag.h, from.x, from.y, to.x, to.y];
        assert!(all_close(&got, &recorded), "flag {want:?}, got {flag:?}");
        assert_eq!((flag.text.as_str(), flag.font), (want[8].as_str(), 11.0));
    }
}

/// Three features, one through the origin: its two arcs take one flag, and a 21-base feature that
/// cannot hold its name takes another.
#[test]
fn a_small_plasmid_draws_as_the_web_app_draws_it() {
    let case = Case::load("wrap_demo");
    let map = ring_of(&case);
    assert_ring_shapes(&case, &map);
    assert_ring_names(&case, &map);
    assert_eq!(map.flags.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(), ["AmpR", "MCS"]);
}

/// The cases recorded on private data (see `support`).
mod private {
    use super::*;

    /// Thirteen features, three of them long backbone notes whose names run along their arcs and
    /// wall off stretches of the columns the flags have to flow around.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn a_crowded_plasmid_draws_as_the_web_app_draws_it() {
        let case = Case::load_private("crowded_ring");
        let map = ring_of(&case);
        assert_ring_shapes(&case, &map);
        assert_ring_names(&case, &map);
    }

    /// Thirty features on a ring too small for all their lanes: the deep ones share the innermost
    /// drawable lane, and most names go out to the columns. The file is linear, so the web app
    /// opened it as a line, looking at its middle, and kept looking there when asked for a ring:
    /// the origin is at six o'clock in the recording.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn thirty_features_on_a_small_ring_draw_as_the_web_app_draws_them() {
        let case = Case::load_private("lab_construct");
        let map = ring_turned(&case, 0.5);
        assert_ring_shapes(&case, &map);
        assert_ring_names(&case, &map);
    }
}

/// The margin is a share of the frame's short side, floored at 28px and capped at 30% of the side.
#[test]
fn the_ring_margin_follows_the_short_side() {
    assert_eq!(ring_margin(676.0, 676.0), Some(676.0 * 0.15));
    assert_eq!(ring_margin(1440.0, 378.0), Some(378.0 * 0.15));
    assert_eq!(ring_margin(400.0, 120.0), Some(28.0));
    assert_eq!(ring_margin(400.0, 60.0), Some(18.0));
    assert_eq!(ring_margin(0.0, 400.0), None);
}

/// An empty construct has an axis and nothing on it.
#[test]
fn an_empty_construct_is_an_empty_ring() {
    let case = Case::load("wrap_demo");
    let mut scene = case.scene.clone();
    scene.length = 0;
    let map = layout_ring(&scene, Frame::ring(600.0, 400.0), |_, _| 0.0);
    assert!(map.ticks.is_empty() && map.arcs.is_empty() && map.arc_labels.is_empty() && map.flags.is_empty());
}

/// Letters along an arc: centred on the name's own fraction, running the way the bases do on the
/// top of the ring and the other way, turned over, on the bottom.
#[test]
fn letters_follow_the_arc_and_stay_upright() {
    let case = Case::load("wrap_demo");
    let map = ring_of(&case);
    let geom = map.geom;
    let top = ArcLabel { feature: 0, text: "ab".to_owned(), frac: 0.0, radius: 100.0, upright: true, font: 11.0 };
    // Two letters 10px wide: their middles sit 5px either side of twelve o'clock.
    let glyphs = arc_glyphs(&top, 0.0, geom, &[5.0, 15.0], 20.0);
    assert!(close(glyphs[0].angle, -0.05) && close(glyphs[1].angle, 0.05));
    assert!(glyphs[0].x < geom.cx && glyphs[1].x > geom.cx);
    assert!(close(glyphs[0].y, geom.cyc - 100.0 * 0.05f64.cos()));

    let bottom = ArcLabel { frac: 0.5, upright: false, ..top };
    let glyphs = arc_glyphs(&bottom, 0.0, geom, &[5.0, 15.0], 20.0);
    // Still left to right on screen, and turned a full half turn less the same small angle.
    assert!(glyphs[0].x < geom.cx && glyphs[1].x > geom.cx);
    assert!(close(glyphs[0].angle, std::f64::consts::PI + 0.05 + std::f64::consts::PI));
    assert!(close(glyphs[0].y, geom.cyc + 100.0 * 0.05f64.cos()));
}
