//! What is under a point on the map, on frames small enough to check by hand.
//!
//! The ring is laid out in a 600 x 600 frame: its margin is 15% of that, 90, so the axis is a
//! circle of radius 210 about (300, 300). Lane 0 sits half a lane (7) and 12 inside it, at 191,
//! and lane 1 a band (24) further in, at 167. A lane is 14 thick, so 7 either side of its centre.
//!
//! The line is laid out in a 1000 x 400 frame: its axis runs from x 44 to x 956, 912 for the whole
//! molecule. With one lane the drawing is 26 above the axis and 92 below, centred, so the axis is
//! at y 167 and lane 0 at y 148.

use bios_layout::overview::display::{FeatureRole, MapFeature};
use bios_layout::overview::hit::{MapHit, line_feature_at, line_gap_at, ring_extend_to, ring_feature_at, ring_gap_at};
use bios_layout::overview::line_map::{LineMap, layout_line};
use bios_layout::overview::projection::Point;
use bios_layout::overview::ring_map::{RingMap, layout_ring};
use bios_layout::overview::scene::{Frame, MapScene};
use bios_layout::overview::selection::{DragSelection, FinishedAt, drag_selection};

fn feature(name: &str, start: i64, end: i64) -> MapFeature {
    MapFeature {
        start,
        end,
        name: name.to_owned(),
        kind: "misc_feature".to_owned(),
        forward: true,
        locations: None,
        location_unsupported: false,
        role: FeatureRole::Structural,
        directional: false,
        source: 0,
    }
}

/// Six tenths of the type size a letter: near enough for a name to fit or not as it would.
fn measure(text: &str, font_px: f64) -> f64 {
    text.chars().count() as f64 * font_px * 0.6
}

/// 1,200 bases, so that a degree is three and a third of them and thirty degrees is a hundred.
const RING: i64 = 1200;

/// `long` over bases 201 to 500 (60 to 150 degrees), `inner` nested in it over 301 to 400 (90 to
/// 120 degrees), `wrap` from base 1,101 through the origin to base 100 (30 degrees either side of
/// twelve o'clock), and `a name too long`, 20 bases at 240 degrees, too short to carry it.
fn ring_scene() -> MapScene {
    MapScene::new(
        &[feature("long", 200, 499), feature("inner", 300, 399), feature("wrap", 1100, 99), feature("a name too long", 800, 819)],
        RING,
        true,
    )
}

fn ring() -> RingMap {
    layout_ring(&ring_scene(), Frame::ring(600.0, 600.0), measure)
}

/// The point `radius` from the middle of the ring, `degrees` clockwise from twelve o'clock.
fn spoke(degrees: f64, radius: f64) -> Point {
    let theta = degrees.to_radians();
    Point { x: 300.0 + radius * theta.sin(), y: 300.0 - radius * theta.cos() }
}

#[test]
fn the_ring_is_where_the_arithmetic_above_says() {
    let map = ring();
    assert_eq!((map.geom.cx, map.geom.cyc, map.axis_r, map.thickness), (300.0, 300.0, 210.0, 14.0));
    let scene = ring_scene();
    assert_eq!(scene.lanes, vec![0, 1, 0, 0]);
    let radii: Vec<f64> = map.arcs.iter().map(|arc| arc.radius).collect();
    // `long`, `inner`, the two halves of `wrap`, `a name too long`.
    assert_eq!(radii, vec![191.0, 167.0, 191.0, 191.0, 191.0]);
}

#[test]
fn a_point_on_the_ring_reads_the_gap_at_its_angle() {
    let map = ring();
    let gap = |degrees: f64, radius: f64| ring_gap_at(&map, RING, spoke(degrees, radius));
    assert_eq!(gap(90.0, 210.0), 300, "three o'clock is a quarter of the way round");
    assert_eq!(gap(180.0, 210.0), 600);
    assert_eq!(gap(270.0, 210.0), 900);
    assert_eq!(gap(45.0, 210.0), 150);
    // One base is 0.3 of a degree: the gap is the nearest one, a half going up.
    assert_eq!(gap(0.3 * 10.4, 210.0), 10);
    assert_eq!(gap(0.3 * 10.6, 210.0), 11);
    // Anywhere along the spoke is the same gap: on a lane, outside the scale, near the middle.
    for radius in [191.0, 167.0, 260.0, 20.0] {
        assert_eq!(gap(90.0, radius), 300, "radius {radius}");
    }
}

#[test]
fn the_origin_reads_zero_from_one_side_and_the_length_from_the_other() {
    let map = ring();
    assert_eq!(ring_gap_at(&map, RING, Point { x: 300.0, y: 90.0 }), 0, "twelve o'clock exactly");
    assert_eq!(ring_gap_at(&map, RING, spoke(0.1, 210.0)), 0, "a third of a base clockwise");
    assert_eq!(ring_gap_at(&map, RING, spoke(-0.1, 210.0)), RING, "a third of a base anticlockwise");
    assert_eq!(ring_gap_at(&map, RING, spoke(-0.3, 210.0)), RING - 1);
    // The very middle has no angle; it reads the origin and never fails.
    assert_eq!(ring_gap_at(&map, RING, Point { x: 300.0, y: 300.0 }), 0);
    // A molecule with no bases has one gap.
    assert_eq!(ring_gap_at(&map, 0, spoke(90.0, 210.0)), 0);
}

#[test]
fn a_ring_turned_reads_from_what_is_at_twelve_o_clock() {
    let quarter = layout_ring(&ring_scene(), Frame { center: 0.25, ..Frame::ring(600.0, 600.0) }, measure);
    assert_eq!(ring_gap_at(&quarter, RING, Point { x: 300.0, y: 90.0 }), 300);
    assert_eq!(ring_gap_at(&quarter, RING, Point { x: 90.0, y: 300.0 }), 0, "the origin is at nine o'clock");
    // `long` runs from 60 to 150 degrees of the molecule, so from 30 before twelve to 60 after.
    assert_eq!(ring_feature_at(&quarter, spoke(0.0, 191.0)), Some(MapHit::Shape(0)));
    assert_eq!(ring_feature_at(&quarter, spoke(-45.0, 191.0)), None);
}

#[test]
fn a_point_on_an_arc_is_on_its_feature() {
    let map = ring();
    let at = |degrees: f64, radius: f64| ring_feature_at(&map, spoke(degrees, radius));
    assert_eq!(at(75.0, 191.0), Some(MapHit::Shape(0)), "the middle of `long`'s lane");
    // 7 either side of the lane's centre line, and not a pixel more.
    assert_eq!(at(75.0, 197.5), Some(MapHit::Shape(0)));
    assert_eq!(at(75.0, 184.5), Some(MapHit::Shape(0)));
    assert_eq!(at(75.0, 198.5), None);
    assert_eq!(at(75.0, 183.5), None);
    // Past either end of it, on the same lane.
    assert_eq!(at(55.0, 191.0), None);
    assert_eq!(at(155.0, 191.0), None);
    // On the axis itself and out on the scale there is no feature.
    assert_eq!(at(75.0, 210.0), None);
    assert_eq!(at(75.0, 222.0), None);
}

#[test]
fn a_nested_feature_is_caught_on_its_own_lane() {
    let map = ring();
    let at = |degrees: f64, radius: f64| ring_feature_at(&map, spoke(degrees, radius));
    assert_eq!(at(105.0, 167.0), Some(MapHit::Shape(1)), "`inner`, a lane in");
    assert_eq!(at(105.0, 191.0), Some(MapHit::Shape(0)), "`long`, over the same bases");
    assert_eq!(at(105.0, 179.0), None, "the ground between the two lanes");
    assert_eq!(at(75.0, 167.0), None, "the inner lane where `inner` is not");
}

#[test]
fn a_feature_through_the_origin_is_caught_on_both_sides_of_it() {
    let map = ring();
    let at = |degrees: f64| ring_feature_at(&map, spoke(degrees, 191.0));
    for degrees in [-29.0, -15.0, -2.0, 2.0, 15.0, 29.0] {
        assert_eq!(at(degrees), Some(MapHit::Shape(2)), "{degrees} degrees");
    }
    assert_eq!(at(-31.0), None);
    assert_eq!(at(31.0), None);
    // The same spokes read gaps that run through the origin without a break.
    assert_eq!(ring_gap_at(&map, RING, spoke(-15.0, 191.0)), 1150);
    assert_eq!(ring_gap_at(&map, RING, spoke(15.0, 191.0)), 50);
}

#[test]
fn the_empty_middle_of_the_ring_is_no_feature() {
    let map = ring();
    assert_eq!(ring_feature_at(&map, Point { x: 300.0, y: 300.0 }), None);
    for degrees in [0.0, 75.0, 105.0, 240.0] {
        assert_eq!(ring_feature_at(&map, spoke(degrees, 100.0)), None, "{degrees} degrees, well inside every lane");
    }
}

#[test]
fn a_flag_is_its_feature_and_its_leader_is_not() {
    let map = ring();
    // `wrap` is in two pieces and `a name too long` is too short for its name: both take a flag.
    assert_eq!(map.flags.iter().map(|flag| flag.feature).collect::<Vec<_>>(), vec![2, 3]);
    let flag = &map.flags[1];
    let middle = Point { x: flag.x + flag.w / 2.0, y: flag.y };
    assert_eq!(ring_feature_at(&map, middle), Some(MapHit::Flag(3)));
    assert_eq!(ring_feature_at(&map, Point { x: flag.x + 1.0, y: flag.y - flag.h / 2.0 + 1.0 }), Some(MapHit::Flag(3)), "inside a corner");
    assert_eq!(ring_feature_at(&map, Point { x: flag.x - 1.0, y: flag.y }), None, "a pixel off its left edge");
    assert_eq!(ring_feature_at(&map, Point { x: middle.x, y: flag.y + flag.h / 2.0 + 1.0 }), None, "a pixel under it");
    // The leader runs from the arc to the chip; half way along it is neither.
    let (from, to) = flag.leader;
    assert_eq!(ring_feature_at(&map, Point { x: (from.x + to.x) / 2.0, y: (from.y + to.y) / 2.0 }), None);
    // The arc the flag names is still its own shape.
    assert_eq!(ring_feature_at(&map, spoke(243.0, 191.0)), Some(MapHit::Shape(3)));
    assert_eq!(MapHit::Flag(3).feature(), MapHit::Shape(3).feature());
}

const LINE: i64 = 1000;

/// `bar` over bases 101 to 300, and `a name too long`, 30 bases from 601, which takes a flag.
fn line_scene() -> MapScene {
    MapScene::new(&[feature("bar", 100, 299), feature("a name too long", 600, 629)], LINE, false)
}

fn line_frame() -> Frame {
    Frame::line(1000.0, 400.0)
}

fn line() -> LineMap {
    layout_line(&line_scene(), line_frame(), measure)
}

#[test]
fn the_line_is_where_the_arithmetic_above_says() {
    let map = line();
    assert_eq!((map.left, map.right, map.axis_y, map.thickness), (44.0, 956.0, 167.0, 14.0));
    let bar = &map.bars[0];
    // 100 and 300 thousandths of 912 from the left end, less a pixel of seam at each end.
    assert!((bar.x0 - 136.2).abs() < 1e-9 && (bar.x1 - 316.6).abs() < 1e-9, "{bar:?}");
    assert_eq!(bar.y, 148.0);
}

#[test]
fn a_point_on_the_line_reads_the_gap_at_its_x() {
    let map = line();
    let gap = |x: f64| line_gap_at(&map, line_frame(), LINE, x);
    assert_eq!(gap(44.0), 0, "the left end of the axis");
    assert_eq!(gap(956.0), 1000, "the right end: after the last base");
    assert_eq!(gap(500.0), 500);
    assert_eq!(gap(44.0 + 0.912 * 10.0), 10);
    assert_eq!(gap(44.0 + 0.912 * 10.4), 10);
    assert_eq!(gap(44.0 + 0.912 * 10.6), 11);
    // Off either end is held at that end.
    assert_eq!(gap(0.0), 0);
    assert_eq!(gap(999.0), 1000);
    assert_eq!(line_gap_at(&map, line_frame(), 0, 500.0), 0, "no bases, one gap");
}

#[test]
fn a_line_showing_part_of_the_molecule_reads_from_its_window() {
    // The middle half: bases 251 to 750 across the same 912 pixels.
    let frame = Frame { width: 0.5, ..line_frame() };
    let map = layout_line(&line_scene(), frame, measure);
    assert_eq!(line_gap_at(&map, frame, LINE, 44.0), 250);
    assert_eq!(line_gap_at(&map, frame, LINE, 500.0), 500);
    assert_eq!(line_gap_at(&map, frame, LINE, 956.0), 750);
}

#[test]
fn a_point_on_a_bar_is_on_its_feature() {
    let map = line();
    let at = |x: f64, y: f64| line_feature_at(&map, Point { x, y });
    assert_eq!(at(200.0, 148.0), Some(MapHit::Shape(0)));
    assert_eq!(at(200.0, 141.5), Some(MapHit::Shape(0)), "just inside its top edge");
    assert_eq!(at(200.0, 154.5), Some(MapHit::Shape(0)), "just inside its bottom edge");
    assert_eq!(at(200.0, 140.5), None);
    assert_eq!(at(200.0, 155.5), None, "between the bar and the axis");
    assert_eq!(at(200.0, 167.0), None, "the axis");
    assert_eq!(at(136.0, 148.0), None, "the seam at its left end");
    assert_eq!(at(137.0, 148.0), Some(MapHit::Shape(0)));
    assert_eq!(at(316.0, 148.0), Some(MapHit::Shape(0)));
    assert_eq!(at(317.0, 148.0), None);
    assert_eq!(at(450.0, 148.0), None, "the lane between two features");
}

#[test]
fn a_flag_over_the_line_is_its_feature() {
    let map = line();
    let [flag] = map.flags.as_slice() else { panic!("one name is lifted onto a flag, got {:?}", map.flags) };
    assert_eq!(flag.feature, 1);
    assert_eq!(line_feature_at(&map, Point { x: flag.x + flag.w / 2.0, y: flag.y }), Some(MapHit::Flag(1)));
    assert_eq!(line_feature_at(&map, Point { x: flag.x + flag.w + 1.0, y: flag.y }), None);
    let (from, to) = flag.leader;
    assert_eq!(line_feature_at(&map, Point { x: (from.x + to.x) / 2.0, y: (from.y + to.y) / 2.0 }), None, "its leader");
    assert_eq!(line_feature_at(&map, Point { x: 44.0 + 0.912 * 615.0, y: 148.0 }), Some(MapHit::Shape(1)), "its bar");
}

#[test]
fn a_shift_press_on_the_ring_extends_the_short_way_round() {
    // 100 gaps round. The pointer is taken to be within half a turn of the anchor.
    assert_eq!(ring_extend_to(10, 30, 100), 30);
    assert_eq!(ring_extend_to(10, 90, 100), -10, "20 back through the origin, not 80 forward");
    assert_eq!(ring_extend_to(90, 10, 100), 110, "20 on through the origin, not 80 back");
    assert_eq!(ring_extend_to(10, 10, 100), 10);
    // Exactly half way round goes anticlockwise, whichever side the press is on.
    assert_eq!(ring_extend_to(10, 60, 100), -40);
    assert_eq!(ring_extend_to(60, 10, 100), 10);
    // The origin is one place with two names; a press on the other one moves nothing.
    assert_eq!(ring_extend_to(0, 100, 100), 0);
    assert_eq!(ring_extend_to(100, 0, 100), 100);
}

#[test]
fn what_a_shift_press_selects_runs_through_the_origin_when_that_is_shorter() {
    let select = |anchor: i64, gap: i64| drag_selection(anchor, ring_extend_to(anchor, gap, 100), 100);
    assert_eq!(select(10, 30), DragSelection::Range { start: 10, end: 29, finished_at: FinishedAt::End });
    assert_eq!(select(10, 90), DragSelection::Range { start: 90, end: 9, finished_at: FinishedAt::Start });
    assert_eq!(select(90, 10), DragSelection::Range { start: 90, end: 9, finished_at: FinishedAt::End });
    assert_eq!(select(10, 10), DragSelection::Caret(10));
    assert_eq!(select(0, 100), DragSelection::Caret(0));
}
