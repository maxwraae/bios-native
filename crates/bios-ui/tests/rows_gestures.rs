//! Not in the original (the web drives these with its eye script): a click inside the selection
//! waits to see whether it is a double-click, which opens the selection's menu; on its own it
//! collapses to a caret once the wait is over; and ⌘ with the wheel zooms the rows.

use bios_core::Construct;
use bios_layout::rows as layout;
use bios_ui::rows::{self, Cursor, Gesture};
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

const SIZE: egui::Vec2 = vec2(1000.0, 700.0);
/// Down among the first row's letters (`rows_pointer.rs` finds the same by pressing).
const FIRST_ROW_Y: f32 = 60.0;

struct Window {
    ctx: egui::Context,
    plan: Option<rows::Plan>,
    c: Construct,
    cursor: Cursor,
}

impl Window {
    fn new() -> Window {
        let c = Construct { name: "t".into(), sequence: "ACGT".repeat(150), ..Construct::default() };
        let mut window = Window { ctx: egui::Context::default(), plan: None, c, cursor: Cursor { selection: Some((5, 20)), ..Cursor::default() } };
        window.frame(0.0, Vec::new(), Modifiers::NONE);
        window
    }

    fn frame(&mut self, time: f64, mut events: Vec<Event>, modifiers: Modifiers) -> Option<Gesture> {
        events.insert(0, Event::ModifiersChanged(modifiers));
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)), time: Some(time), events, ..RawInput::default() };
        let mut gesture = None;
        let (c, plan, cursor) = (&self.c, &mut self.plan, &self.cursor);
        self.ctx.run_ui(input, |ui| gesture = rows::show(ui, c, 1, plan, cursor)).drop_without_applying_deltas();
        gesture
    }

    fn at(&self, gap: usize) -> Pos2 {
        let metrics = layout::metrics_for(f64::from(SIZE.x), None, None);
        pos2((metrics.margin_width / 2.0) as f32 + gap as f32 * metrics.char_width as f32 + 1.0, FIRST_ROW_Y)
    }

    fn button(&mut self, time: f64, gap: usize, pressed: bool) -> Option<Gesture> {
        let pos = self.at(gap);
        self.frame(
            time,
            vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }],
            Modifiers::NONE,
        )
    }
}

#[test]
fn a_click_inside_the_selection_collapses_to_a_caret_once_the_double_click_window_has_passed() {
    let mut w = Window::new();
    assert_eq!(w.button(0.1, 10, true), None, "it waits");
    assert_eq!(w.button(0.15, 10, false), None);
    assert_eq!(w.frame(0.3, Vec::new(), Modifiers::NONE), None, "still inside the window");
    assert_eq!(w.frame(0.5, Vec::new(), Modifiers::NONE), Some(Gesture::Press { gap: 10, shift: false }));
    assert!(rows::seen(&w.ctx).menu.is_none());
}

#[test]
fn a_double_click_on_the_selection_opens_its_menu_and_keeps_it() {
    let mut w = Window::new();
    w.button(0.1, 10, true);
    w.button(0.15, 10, false);
    w.button(0.2, 10, true);
    assert_eq!(w.button(0.25, 10, false), None);
    assert!(rows::seen(&w.ctx).menu.is_some(), "the menu a right-click opens");
    assert_eq!(w.frame(0.8, Vec::new(), Modifiers::NONE), None, "and the selection stays");
}

#[test]
fn a_press_outside_the_selection_is_a_caret_at_once() {
    let mut w = Window::new();
    assert_eq!(w.button(0.1, 40, true), Some(Gesture::Press { gap: 40, shift: false }));
}

#[test]
fn command_and_the_wheel_zoom_the_rows() {
    let mut w = Window::new();
    let before = w.plan.as_ref().map(rows::Plan::bps_per_row).unwrap_or(0);
    let pos = w.at(10);
    let wheel = Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, 20.0),
        modifiers: Modifiers::COMMAND,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    };
    for t in 0..4 {
        w.frame(0.1 + f64::from(t) * 0.02, vec![Event::PointerMoved(pos), wheel.clone()], Modifiers::COMMAND);
    }
    w.frame(0.3, Vec::new(), Modifiers::NONE);
    let after = w.plan.as_ref().map(rows::Plan::bps_per_row).unwrap_or(0);
    assert!(after < before, "wider bases, fewer to a row: {before} -> {after}");
}
