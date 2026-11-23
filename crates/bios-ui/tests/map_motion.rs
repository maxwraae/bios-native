//! The map's motion, run without a window at 60 frames a second: a pinch eases the zoom in over
//! several frames and unrolls the ring on the way; a couple of seconds of scrolling hands the ring
//! to the flywheel, which then turns on its own; a press stops it dead and places no caret; and the
//! button brings a ring that rolled off back.

use bios_core::Construct;
use bios_layout::overview::flywheel::{self, SpinPhase};
use bios_layout::overview::navigate;
use bios_layout::overview::projection::straightness;
use bios_layout::overview::viewport::Projection;
use bios_ui::map::{self, MapCursor, MapGesture, MapView, Motion};
use bios_ui::theme;
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

const SCREEN: egui::Vec2 = vec2(800.0, 600.0);
const MAP: Rect = Rect { min: Pos2::ZERO, max: pos2(800.0, 500.0) };
const OVER: Pos2 = pos2(400.0, 250.0);

struct Run {
    ctx: egui::Context,
    c: Construct,
    view: MapView,
    motion: Motion,
    t: f64,
}

impl Run {
    fn new() -> Run {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let c = Construct { name: "t".into(), circular: true, sequence: "ACGT".repeat(200), ..Construct::default() };
        let view = MapView { vp: navigate::OPENING, ring: true };
        let mut run = Run { ctx, c, view, motion: Motion::spinning(), t: 0.0 };
        run.frame(vec![Event::PointerMoved(OVER)]);
        run.frame(Vec::new());
        run
    }

    /// One frame, 1/60 s after the last, with these events in it.
    fn frame(&mut self, events: Vec<Event>) -> Option<MapGesture> {
        self.t += 1.0 / 60.0;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.t), events, ..RawInput::default() };
        let mut gesture = None;
        let (c, view, motion) = (&self.c, &mut self.view, &mut self.motion);
        let out = self.ctx.run_ui(input, |ui| {
            gesture = map::show_moving(ui, MAP, c, &MapCursor::default(), view, motion, &[], Default::default());
        });
        out.drop_without_applying_deltas();
        gesture
    }
}

fn wheel(dy: f32, modifiers: Modifiers) -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, dy),
        modifiers,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    }
}

#[test]
fn a_pinch_eases_in_over_several_frames_and_unrolls_the_ring_on_the_way() {
    let mut run = Run::new();
    // A pinch spread over three frames: each asks exp(-30 * 0.02) of the width (held to the wheel's
    // clamp) of the target still being chased, so the target is that cubed.
    let target = (-0.6f64).exp().powi(3);
    let mut widths = Vec::new();
    for _ in 0..3 {
        run.frame(vec![Event::Zoom(4.0)]);
        widths.push(run.view.vp.width);
    }
    for _ in 0..40 {
        run.frame(Vec::new());
        widths.push(run.view.vp.width);
    }
    assert!(widths[0] > 0.5, "the first frame is only part of the way: {}", widths[0]);
    assert!(widths.windows(2).all(|w| w[1] <= w[0]), "it glides one way");
    assert!(widths.iter().any(|&w| straightness(w) > 0.0 && straightness(w) < 1.0), "it passes through the unroll: {widths:?}");
    assert!((run.view.vp.width - target).abs() < 1e-9, "and lands on the target: {} vs {target}", run.view.vp.width);
    assert_eq!(run.view.vp.projection, Projection::Line);
}

#[test]
fn scrolling_a_ring_for_a_couple_of_seconds_hands_it_to_the_flywheel_and_a_press_stops_it_dead() {
    let mut run = Run::new();
    // Two and a half seconds of steady scrolling, an event a frame.
    for _ in 0..150 {
        run.frame(vec![wheel(-12.0, Modifiers::NONE)]);
    }
    assert_eq!(run.motion.fly.phase, SpinPhase::Spinning, "{:?}", run.motion.fly.phase);
    // Hands off: it keeps turning by itself.
    let before = run.view.vp.center;
    for _ in 0..30 {
        run.frame(Vec::new());
    }
    assert_ne!(run.view.vp.center, before, "the ring turns on its own");
    assert!(flywheel::rpm_of(&run.motion.fly) > 0.0);
    // A hand on it: stopped, and no caret.
    let gesture = run.frame(vec![Event::PointerButton { pos: OVER, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    assert_eq!(gesture, None);
    assert_eq!(run.motion.fly.phase, SpinPhase::Idle);
    let still = run.view.vp.center;
    run.frame(vec![Event::PointerButton { pos: OVER, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    run.frame(Vec::new());
    assert_eq!(run.view.vp.center, still);
}

#[test]
fn a_zoom_takes_the_ring_back_from_the_flywheel() {
    let mut run = Run::new();
    run.motion.fly = flywheel::pose("spin", 600.0, run.t).unwrap_or_else(|| flywheel::idle(1, 0.0));
    run.frame(Vec::new());
    assert!(flywheel::active(run.motion.fly.phase));
    run.frame(vec![Event::Zoom(2.0)]);
    assert_eq!(run.motion.fly.phase, SpinPhase::Idle);
}

#[test]
fn the_button_brings_back_a_ring_that_rolled_off() {
    let mut run = Run::new();
    run.motion.fly = flywheel::pose("gone", 1.0, run.t).unwrap_or_else(|| flywheel::idle(1, 0.0));
    run.frame(Vec::new());
    // The button sits under "It rolled off." in the middle of the map.
    let button = pos2(400.0, 250.0 + 14.0);
    run.frame(vec![Event::PointerMoved(button)]);
    run.frame(vec![Event::PointerButton { pos: button, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    run.frame(vec![Event::PointerButton { pos: button, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    assert_eq!(run.motion.fly.phase, SpinPhase::Returning);
    for _ in 0..100 {
        run.frame(Vec::new());
    }
    assert_eq!(run.motion.fly.phase, SpinPhase::Idle);
}
