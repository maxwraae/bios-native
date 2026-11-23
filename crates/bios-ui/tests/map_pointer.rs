//! A press and a drag on the map, run without a window: what the pointer is on is what was drawn
//! there, and a drag round the ring keeps counting through the origin.
//!
//! The map is given a 700 x 420 rectangle whose top left is at (40, 30). As a ring its margin is
//! 15% of the short side, 63, so the axis is a circle of radius 147 about the rectangle's middle,
//! (390, 240) on screen, and lane 0 is 19 inside it at 128. As a line its axis runs from x 44 to
//! x 656 of the rectangle, 612 for the whole molecule, at y 177, with lane 0 at y 158.

use bios_core::{Construct, Feature, Segment, Strand};
use bios_layout::overview::selection::{DragSelection, FinishedAt, drag_selection};
use bios_ui::map::{self, MapCursor, MapGesture};
use bios_ui::theme;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2};

const SCREEN: egui::Vec2 = vec2(800.0, 600.0);
const MAP: Rect = Rect::from_min_max(pos2(40.0, 30.0), pos2(740.0, 450.0));
/// 1,200 bases, so that a degree of the ring is three and a third of them.
const LENGTH: usize = 1200;
const LENGTH_BP: i64 = LENGTH as i64;

const MIDDLE: Pos2 = pos2(390.0, 240.0);
const AXIS_R: f32 = 147.0;
const LANE_R: f32 = 128.0;

const LINE_LEFT: f32 = 40.0 + 44.0;
const LINE_PER_BASE: f32 = 612.0 / 1200.0;
const AXIS_Y: f32 = 30.0 + 177.0;
const LANE_Y: f32 = 30.0 + 158.0;

fn feature(name: &str, start: usize, end: usize) -> Feature {
    Feature {
        name: name.into(),
        kind: "CDS".into(),
        start,
        end,
        strand: Strand::Forward,
        segments: vec![Segment::new(start, end)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    }
}

/// `gene` over bases 201 to 500, and `tiny`, 40 bases from 801 under a name it cannot hold. On the
/// ring there is also `wrap`, from base 1,101 through the origin to base 100.
fn construct(circular: bool) -> Construct {
    let mut features = vec![feature("gene", 200, 499), feature("a name too long for its bar", 800, 839)];
    if circular {
        features.push(feature("wrap", 1100, 99));
    }
    Construct { name: "t".into(), circular, sequence: "ACGT".repeat(LENGTH / 4), features, ..Construct::default() }
}

/// The point on the ring at `gap`, `radius` from its middle.
fn ring_at(gap: f32, radius: f32) -> Pos2 {
    let theta = gap / LENGTH as f32 * std::f32::consts::TAU;
    MIDDLE + radius * vec2(theta.sin(), -theta.cos())
}

/// The point on the line's axis at `gap`.
fn line_at(gap: f32) -> Pos2 {
    pos2(LINE_LEFT + gap * LINE_PER_BASE, AXIS_Y)
}

struct Window {
    ctx: egui::Context,
    c: Construct,
    cursor: MapCursor,
}

impl Window {
    fn new(circular: bool) -> Window {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut window = Window { ctx, c: construct(circular), cursor: MapCursor::default() };
        // One frame with nothing happening, so the map exists to be pressed on.
        assert_eq!(window.frame(Vec::new(), Modifiers::NONE), None);
        window
    }

    fn run(&mut self, mut events: Vec<Event>, modifiers: Modifiers) -> (Option<MapGesture>, egui::FullOutput) {
        events.insert(0, Event::ModifiersChanged(modifiers));
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), events, ..RawInput::default() };
        let mut gesture = None;
        let (c, cursor) = (&self.c, &self.cursor);
        let output = self.ctx.run_ui(input, |ui| {
            gesture = map::show(ui, MAP, c, cursor);
        });
        (gesture, output)
    }

    fn frame(&mut self, events: Vec<Event>, modifiers: Modifiers) -> Option<MapGesture> {
        let (gesture, output) = self.run(events, modifiers);
        // Nothing is painted here, so what the frame would have drawn is let go of.
        output.drop_without_applying_deltas();
        gesture
    }

    /// What a frame draws with the cursor at `cursor`.
    fn drawn(&mut self, cursor: MapCursor) -> Vec<Shape> {
        self.cursor = cursor;
        let (_, output) = self.run(Vec::new(), Modifiers::NONE);
        let shapes = output.shapes.iter().map(|clipped| clipped.shape.clone()).collect();
        output.drop_without_applying_deltas();
        shapes
    }

    fn press(&mut self, pos: Pos2, modifiers: Modifiers) -> Option<MapGesture> {
        self.frame(vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers }], modifiers)
    }

    fn move_to(&mut self, pos: Pos2) -> Option<MapGesture> {
        self.frame(vec![Event::PointerMoved(pos)], Modifiers::NONE)
    }

    /// A frame in which the pointer does nothing.
    fn wait(&mut self) -> Option<MapGesture> {
        self.frame(Vec::new(), Modifiers::NONE)
    }

    fn release(&mut self, pos: Pos2) -> Option<MapGesture> {
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE)
    }
}

#[test]
fn a_press_on_an_arc_selects_the_bases_its_feature_covers() {
    let mut window = Window::new(true);
    assert_eq!(window.press(ring_at(350.0, LANE_R), Modifiers::NONE), Some(MapGesture::Feature { start: 200, end: 499 }));
    assert_eq!(window.release(ring_at(350.0, LANE_R)), None);
    // A feature through the origin is one feature on either side of it, and its span wraps.
    for gap in [1150.0, 50.0] {
        assert_eq!(window.press(ring_at(gap, LANE_R), Modifiers::NONE), Some(MapGesture::Feature { start: 1100, end: 99 }), "gap {gap}");
        assert_eq!(window.release(ring_at(gap, LANE_R)), None);
    }
}

#[test]
fn a_press_on_the_ring_away_from_a_feature_is_a_press_at_its_gap() {
    let mut window = Window::new(true);
    let quarter = ring_at(300.0, AXIS_R);
    assert_eq!(window.press(quarter, Modifiers::NONE), Some(MapGesture::Press { gap: 300, shift: false }));
    assert_eq!(window.release(quarter), None);
    assert_eq!(window.press(quarter, Modifiers::SHIFT), Some(MapGesture::Press { gap: 300, shift: true }));
    assert_eq!(window.release(quarter), None);
    // The same lane where no feature is, the scale outside the axis, and the empty middle of the
    // ring: the map takes a press anywhere in its rectangle, read by its angle.
    for radius in [LANE_R, AXIS_R + 20.0, 40.0] {
        assert_eq!(window.press(ring_at(600.0, radius), Modifiers::NONE), Some(MapGesture::Press { gap: 600, shift: false }), "radius {radius}");
        assert_eq!(window.release(ring_at(600.0, radius)), None);
    }
    // Twelve o'clock is the origin.
    assert_eq!(window.press(ring_at(0.0, AXIS_R), Modifiers::NONE), Some(MapGesture::Press { gap: 0, shift: false }));
}

#[test]
fn a_press_outside_the_map_is_not_the_maps() {
    let mut window = Window::new(true);
    assert_eq!(window.press(pos2(390.0, 520.0), Modifiers::NONE), None);
    assert_eq!(window.move_to(MIDDLE), None, "nor is the drag that follows it onto the map");
    assert_eq!(window.release(MIDDLE), None);
}

#[test]
fn a_press_on_a_bar_selects_its_feature_and_a_press_on_the_axis_under_it_does_not() {
    let mut window = Window::new(false);
    let on_bar = pos2(line_at(350.0).x, LANE_Y);
    assert_eq!(window.press(on_bar, Modifiers::NONE), Some(MapGesture::Feature { start: 200, end: 499 }));
    assert_eq!(window.release(on_bar), None);
    assert_eq!(window.press(line_at(350.0), Modifiers::NONE), Some(MapGesture::Press { gap: 350, shift: false }));
    assert_eq!(window.release(line_at(350.0)), None);
    // The two ends of the molecule, and past them.
    assert_eq!(window.press(line_at(0.0), Modifiers::NONE), Some(MapGesture::Press { gap: 0, shift: false }));
    assert_eq!(window.release(line_at(0.0)), None);
    assert_eq!(window.press(line_at(1200.0), Modifiers::NONE), Some(MapGesture::Press { gap: 1200, shift: false }));
    assert_eq!(window.release(line_at(1200.0)), None);
    assert_eq!(window.press(pos2(MAP.right() - 2.0, AXIS_Y), Modifiers::NONE), Some(MapGesture::Press { gap: 1200, shift: false }));
}

#[test]
fn a_drag_across_the_line_gives_the_gap_under_the_pointer_as_it_goes() {
    let mut window = Window::new(false);
    assert_eq!(window.press(line_at(100.0), Modifiers::NONE), Some(MapGesture::Press { gap: 100, shift: false }));
    let mut last = 100;
    for gap in [150.0, 400.0, 650.0, 1100.0] {
        let Some(MapGesture::Drag { gap: at }) = window.move_to(line_at(gap)) else { panic!("no drag at {gap}") };
        assert_eq!(at, gap as i64);
        assert!(at > last);
        last = at;
    }
    // Back the way it came, and up off the axis: only x counts.
    assert_eq!(window.move_to(line_at(40.0) - vec2(0.0, 90.0)), Some(MapGesture::Drag { gap: 40 }));
    // Off the end of the map the drag is still the map's, held at the end of the molecule.
    assert_eq!(window.move_to(pos2(790.0, 500.0)), Some(MapGesture::Drag { gap: 1200 }));
    assert_eq!(window.release(pos2(790.0, 500.0)), None);
    assert_eq!(window.move_to(line_at(300.0)), None, "the button is up");
}

#[test]
fn a_drag_clockwise_through_the_origin_keeps_counting() {
    let mut window = Window::new(true);
    let at = |gap: f32| ring_at(gap, AXIS_R);
    assert_eq!(window.press(at(1080.0), Modifiers::NONE), Some(MapGesture::Press { gap: 1080, shift: false }));
    let mut gaps = Vec::new();
    for gap in [1140.0, 1188.0, 12.0, 60.0, 360.0] {
        let Some(MapGesture::Drag { gap }) = window.move_to(at(gap)) else { panic!("no drag at {gap}") };
        gaps.push(gap);
    }
    assert_eq!(gaps, vec![1140, 1188, 1212, 1260, 1560], "no jump back to 12 at the origin");
    // What the caller makes of it: the bases from the press through the origin to the pointer.
    assert_eq!(drag_selection(1080, 1260, LENGTH_BP), DragSelection::Range { start: 1080, end: 59, finished_at: FinishedAt::End });
    // On round to where it began: a full turn, which is everything.
    assert_eq!(window.move_to(at(720.0)), Some(MapGesture::Drag { gap: 1920 }));
    assert_eq!(window.move_to(at(1080.0)), Some(MapGesture::Drag { gap: 2280 }));
    assert_eq!(drag_selection(1080, 2280, LENGTH_BP), DragSelection::Range { start: 0, end: 1199, finished_at: FinishedAt::End });
    assert_eq!(window.release(at(1080.0)), None);
    // The next press starts from its own gap again.
    assert_eq!(window.press(at(1080.0), Modifiers::NONE), Some(MapGesture::Press { gap: 1080, shift: false }));
    assert_eq!(window.move_to(at(1100.0)), Some(MapGesture::Drag { gap: 1100 }));
}

#[test]
fn a_drag_anticlockwise_through_the_origin_goes_below_zero() {
    let mut window = Window::new(true);
    let at = |gap: f32| ring_at(gap, AXIS_R + 30.0);
    assert_eq!(window.press(at(60.0), Modifiers::NONE), Some(MapGesture::Press { gap: 60, shift: false }));
    assert_eq!(window.move_to(at(6.0)), Some(MapGesture::Drag { gap: 6 }));
    assert_eq!(window.move_to(at(1140.0)), Some(MapGesture::Drag { gap: -60 }));
    assert_eq!(drag_selection(60, -60, LENGTH_BP), DragSelection::Range { start: 1140, end: 59, finished_at: FinishedAt::Start });
    // And back through it to where it began: a caret again.
    assert_eq!(window.move_to(at(60.0)), Some(MapGesture::Drag { gap: 60 }));
}

#[test]
fn a_few_pixels_of_jitter_in_a_click_is_not_a_drag() {
    let mut window = Window::new(true);
    let down = ring_at(300.0, AXIS_R);
    assert_eq!(window.press(down, Modifiers::NONE), Some(MapGesture::Press { gap: 300, shift: false }));
    assert_eq!(window.wait(), None);
    assert_eq!(window.move_to(down + vec2(1.0, 2.0)), None, "three pixels in all");
    assert_eq!(window.move_to(down + vec2(0.0, 4.0)), Some(MapGesture::Drag { gap: 305 }), "four pixels round a ring of 147 is five bases");
    // Once a drag, back on the spot is the drag come home, not a click.
    assert_eq!(window.move_to(down), Some(MapGesture::Drag { gap: 300 }));
}

#[test]
fn a_drag_that_begins_on_an_arc_selects_bases_from_where_it_began() {
    let mut window = Window::new(true);
    let down = ring_at(350.0, LANE_R);
    assert_eq!(window.press(down, Modifiers::NONE), Some(MapGesture::Feature { start: 200, end: 499 }));
    assert_eq!(window.move_to(down + vec2(1.0, 1.0)), None, "still a click on the feature");
    // Leaving the spot hands the press back as one in the sequence, at the gap it was made at...
    assert_eq!(window.move_to(ring_at(420.0, LANE_R)), Some(MapGesture::Press { gap: 350, shift: false }));
    // ...and the drag follows from the next frame on, moved or not.
    assert_eq!(window.wait(), Some(MapGesture::Drag { gap: 420 }));
    assert_eq!(window.move_to(ring_at(600.0, LANE_R)), Some(MapGesture::Drag { gap: 600 }));
}

#[test]
fn shift_on_an_arc_extends_over_it_and_is_a_drag_from_the_start() {
    let mut window = Window::new(true);
    let down = ring_at(350.0, LANE_R);
    assert_eq!(window.press(down, Modifiers::SHIFT), Some(MapGesture::Press { gap: 350, shift: true }));
    assert_eq!(window.wait(), Some(MapGesture::Drag { gap: 350 }), "the range it extends to shows before the pointer moves");
    assert_eq!(window.move_to(ring_at(380.0, LANE_R)), Some(MapGesture::Drag { gap: 380 }));
}

#[test]
fn a_press_on_a_flag_selects_its_feature_and_no_drag_follows() {
    let mut window = Window::new(false);
    // The flag stands over its feature, above the lanes. Walk up from just over the bar until a
    // press is on it; every press on the way is a press in the sequence under the pointer.
    let x = line_at(820.0).x;
    let mut found = None;
    for step in 0..40 {
        let at = pos2(x, LANE_Y - 9.0 - step as f32);
        match window.press(at, Modifiers::NONE) {
            Some(MapGesture::Feature { start, end }) => {
                found = Some((start, end, at));
                break;
            }
            Some(MapGesture::Press { gap: 820, shift: false }) => assert_eq!(window.release(at), None),
            other => panic!("a press at {at:?} gave {other:?}"),
        }
    }
    let (start, end, at) = found.expect("no press landed on the flag");
    assert_eq!((start, end), (800, 839));
    // A flag takes its press whole: dragging off it selects nothing.
    assert_eq!(window.move_to(at + vec2(60.0, 0.0)), None);
    assert_eq!(window.release(at + vec2(60.0, 0.0)), None);
    // With shift held it is still the flag's press, where an arc would have let it through.
    assert_eq!(window.press(at, Modifiers::SHIFT), Some(MapGesture::Feature { start: 800, end: 839 }));
}

#[test]
fn a_construct_with_no_bases_takes_no_press() {
    let mut window = Window::new(true);
    window.c = Construct { name: "empty".into(), circular: true, ..Construct::default() };
    assert_eq!(window.wait(), None);
    assert_eq!(window.press(ring_at(300.0, AXIS_R), Modifiers::NONE), None);
    assert_eq!(window.move_to(ring_at(400.0, AXIS_R)), None);
}

/// The shapes `with` has and `without` has not.
fn added(without: &[Shape], with: &[Shape]) -> Vec<Shape> {
    with.iter().filter(|shape| !without.contains(shape)).cloned().collect()
}

#[test]
fn the_ring_draws_the_selection_as_a_band_on_the_axis_and_the_caret_as_a_mark_across_it() {
    let mut window = Window::new(true);
    let plain = window.drawn(MapCursor::default());
    let blue = theme::alpha(theme::SANA_BLUE, 0.28);

    // One arc for a selection in one piece, 26 wide, from its first base to past its last.
    let selected = window.drawn(MapCursor { caret: None, selection: Some((300, 599)) });
    let one = added(&plain, &selected);
    let [Shape::Path(band)] = one.as_slice() else { panic!("{one:?}") };
    assert_eq!((band.stroke.width, band.closed, band.fill), (26.0, false, egui::Color32::TRANSPARENT));
    assert_eq!(band.stroke.color, egui::epaint::ColorMode::Solid(blue));
    let (first, last) = (band.points[0], band.points[band.points.len() - 1]);
    assert!(first.distance(ring_at(300.0, AXIS_R)) < 0.01 && last.distance(ring_at(600.0, AXIS_R)) < 0.01, "{first:?} to {last:?}");
    // Under the axis, so the scale it picks out still reads.
    let axis = selected.iter().position(|s| matches!(s, Shape::Circle(c) if c.radius == AXIS_R && c.stroke.width == 1.5));
    assert!(selected.iter().position(|s| *s == one[0]) < axis && axis.is_some());

    // Centred on the axis: 13 either side of it.
    assert_eq!(band.stroke.kind, egui::StrokeKind::Middle);
    assert!(band.points.iter().all(|p| (p.distance(MIDDLE) - AXIS_R).abs() < 0.01));

    // Still one arc through the origin, with no seam there: a sixth of a turn, 154 of circumference.
    let wrapped = added(&plain, &window.drawn(MapCursor { caret: None, selection: Some((1100, 99)) }));
    let [Shape::Path(band)] = wrapped.as_slice() else { panic!("{wrapped:?}") };
    let (first, last) = (band.points[0], band.points[band.points.len() - 1]);
    assert!(first.distance(ring_at(1100.0, AXIS_R)) < 0.01 && last.distance(ring_at(100.0, AXIS_R)) < 0.01, "{first:?} to {last:?}");
    assert!(!band.closed && band.points.iter().any(|p| p.distance(ring_at(0.0, AXIS_R)) < 1.0), "it passes through twelve o'clock");
    let along: f32 = band.points.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    assert!((along - 153.9).abs() < 0.5, "{along}");

    // Everything selected is the whole circle, closed, the same band.
    let all = added(&plain, &window.drawn(MapCursor { caret: None, selection: Some((0, 1199)) }));
    let [Shape::Path(band)] = all.as_slice() else { panic!("{all:?}") };
    assert_eq!((band.stroke.width, band.closed, band.stroke.kind), (26.0, true, egui::StrokeKind::Middle));
    assert_eq!(band.stroke.color, egui::epaint::ColorMode::Solid(blue));
    assert!(band.points.iter().all(|p| (p.distance(MIDDLE) - AXIS_R).abs() < 0.01));
    assert!(band.points[0].distance(band.points[band.points.len() - 1]) > 1.0, "the path closes itself; its last point is not its first again");

    // The caret: a line 7 either side of the axis, and a dot on its outer end.
    let with_caret = window.drawn(MapCursor { caret: Some(300), selection: None });
    let caret = added(&plain, &with_caret);
    let [Shape::LineSegment { points, stroke }, Shape::Circle(dot)] = caret.as_slice() else { panic!("{caret:?}") };
    assert_eq!((points[0], points[1]), (ring_at(300.0, AXIS_R - 7.0), ring_at(300.0, AXIS_R + 7.0)));
    // Over the arcs and under the flags.
    let at = with_caret.iter().position(|s| *s == caret[0]);
    let last_arc = with_caret.iter().rposition(|s| matches!(s, Shape::Path(p) if p.stroke.width == 14.0));
    let first_flag = with_caret.iter().position(|s| matches!(s, Shape::Rect(_)));
    assert!(last_arc.is_some() && last_arc < at && at < first_flag, "{last_arc:?} {at:?} {first_flag:?}");
    assert_eq!((stroke.width, stroke.color), (2.0, theme::INK));
    assert_eq!((dot.center, dot.radius, dot.fill), (ring_at(300.0, AXIS_R + 7.0), 2.4, theme::INK));

    // A selection off the end of the molecule is not drawn, and nothing fails.
    assert_eq!(added(&plain, &window.drawn(MapCursor { caret: None, selection: Some((300, 5000)) })), Vec::new());
    assert_eq!(added(&plain, &window.drawn(MapCursor { caret: Some(5000), selection: None })), Vec::new());
}

#[test]
fn the_line_draws_the_selection_as_a_block_on_the_axis_and_the_caret_as_a_mark_through_it() {
    let mut window = Window::new(false);
    let plain = window.drawn(MapCursor::default());

    // 11 either side of the axis: half a lane and 4.
    let one = added(&plain, &window.drawn(MapCursor { caret: None, selection: Some((300, 599)) }));
    let [Shape::Rect(block)] = one.as_slice() else { panic!("{one:?}") };
    assert_eq!(block.fill, theme::alpha(theme::SANA_BLUE, 0.22));
    let want = Rect::from_min_max(pos2(line_at(300.0).x, AXIS_Y - 11.0), pos2(line_at(600.0).x, AXIS_Y + 11.0));
    assert!(block.rect.min.distance(want.min) < 0.01 && block.rect.max.distance(want.max) < 0.01, "{:?}", block.rect);

    // 16 either side of the axis, the dot on top; after the last base it stands at the right end.
    for gap in [300.0, 1200.0] {
        let caret = added(&plain, &window.drawn(MapCursor { caret: Some(gap as usize), selection: None }));
        let [Shape::LineSegment { points, stroke }, Shape::Circle(dot)] = caret.as_slice() else { panic!("{caret:?}") };
        let top = line_at(gap) - vec2(0.0, 16.0);
        assert!(points[0].distance(line_at(gap) + vec2(0.0, 16.0)) < 0.01 && points[1].distance(top) < 0.01, "{points:?}");
        assert_eq!((stroke.width, stroke.color), (2.0, theme::INK));
        assert!(dot.center.distance(top) < 0.01);
        assert_eq!((dot.radius, dot.fill), (2.4, theme::INK));
    }
}
