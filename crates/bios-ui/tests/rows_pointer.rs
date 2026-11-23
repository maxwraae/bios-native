//! A press and a drag in the sequence rows, run without a window: the gap a pointer lands in is
//! the one the letter under it was drawn at.

use bios_core::{Construct, Feature, Segment, Strand};
use bios_layout::rows as layout;
use bios_ui::rows::{self, Cursor, Gesture};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

const SIZE: egui::Vec2 = vec2(1000.0, 700.0);
/// How far down the first row's letters are, found by pressing: see `letters_y`.
const FIRST_ROW_Y: f32 = 60.0;

/// 600 bases with one feature on bases 11 to 30 of the first row.
fn construct() -> Construct {
    let gene = Feature {
        name: "gene".into(),
        kind: "CDS".into(),
        start: 10,
        end: 29,
        strand: Strand::Forward,
        segments: vec![Segment::new(10, 29)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    Construct { name: "t".into(), sequence: "ACGT".repeat(150), features: vec![gene], ..Construct::default() }
}

struct Window {
    ctx: egui::Context,
    plan: Option<rows::Plan>,
    c: Construct,
}

impl Window {
    fn new() -> Window {
        let mut window = Window { ctx: egui::Context::default(), plan: None, c: construct() };
        // One frame with nothing happening, so the sheet exists to be pressed on.
        assert_eq!(window.frame(Vec::new(), Modifiers::NONE), None);
        window
    }

    fn frame(&mut self, mut events: Vec<Event>, modifiers: Modifiers) -> Option<Gesture> {
        events.insert(0, Event::ModifiersChanged(modifiers));
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)), events, ..RawInput::default() };
        let mut gesture = None;
        let (c, plan) = (&self.c, &mut self.plan);
        // Nothing is painted here, so what the frame would have drawn is let go of.
        self.ctx
            .run_ui(input, |ui| {
                gesture = rows::show(ui, c, 1, plan, &Cursor::default());
            })
            .drop_without_applying_deltas();
        gesture
    }

    /// Where the gap before base `gap` of the first row is on screen.
    fn at(&self, gap: usize) -> Pos2 {
        let metrics = layout::metrics_for(f64::from(self.ctx.content_rect().width()), None, None);
        let left = self.ctx.content_rect().left() + (metrics.margin_width / 2.0) as f32;
        pos2(left + gap as f32 * metrics.char_width as f32 + 1.0, FIRST_ROW_Y)
    }

    fn press(&mut self, gap: usize, modifiers: Modifiers) -> Option<Gesture> {
        self.press_at(self.at(gap), modifiers)
    }

    fn press_at(&mut self, pos: Pos2, modifiers: Modifiers) -> Option<Gesture> {
        self.frame(vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers }], modifiers)
    }

    fn move_to(&mut self, gap: usize) -> Option<Gesture> {
        let pos = self.at(gap);
        self.frame(vec![Event::PointerMoved(pos)], Modifiers::NONE)
    }

    fn release(&mut self, gap: usize) -> Option<Gesture> {
        let pos = self.at(gap);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }], Modifiers::NONE)
    }
}

#[test]
fn a_press_reports_the_gap_under_the_pointer() {
    let mut window = Window::new();
    assert_eq!(window.press(7, Modifiers::NONE), Some(Gesture::Press { gap: 7, shift: false }));
    assert_eq!(window.release(7), None);
    assert_eq!(window.press(0, Modifiers::SHIFT), Some(Gesture::Press { gap: 0, shift: true }));
}

#[test]
fn a_drag_follows_the_pointer_until_the_button_comes_up() {
    let mut window = Window::new();
    assert_eq!(window.press(3, Modifiers::NONE), Some(Gesture::Press { gap: 3, shift: false }));
    assert_eq!(window.move_to(12), Some(Gesture::Drag { gap: 12 }));
    assert_eq!(window.move_to(5), Some(Gesture::Drag { gap: 5 }));
    assert_eq!(window.release(5), None);
    assert_eq!(window.move_to(20), None);
}

#[test]
fn a_pointer_past_the_last_base_of_a_row_lands_after_it() {
    let mut window = Window::new();
    let per_row = layout::metrics_for(f64::from(window.ctx.content_rect().width()), None, None).bps_per_row as usize;
    // In the margin to the right of the row's last letter, clear of the scroll bar at the edge.
    let margin = pos2(window.ctx.content_rect().right() - 24.0, FIRST_ROW_Y);
    assert!(margin.x > window.at(per_row).x);
    assert_eq!(window.press_at(margin, Modifiers::NONE), Some(Gesture::Press { gap: per_row, shift: false }));
}

#[test]
fn a_press_on_a_feature_bar_selects_the_feature_and_a_press_on_the_letters_under_it_does_not() {
    let mut window = Window::new();
    // The bar sits above the letters of its bases. Walk up the first row from the letters until
    // a press is on it; every press on the way is still a press in the gap under the pointer.
    let x = window.at(20).x;
    let mut found = None;
    for step in 0..30 {
        let y = FIRST_ROW_Y - step as f32 * 2.0;
        match window.press_at(pos2(x, y), Modifiers::NONE) {
            Some(Gesture::Span { feature: 0, start, end }) => {
                found = Some((start, end, step));
                break;
            }
            Some(Gesture::Press { gap: 20, shift: false }) => {
                let _ = window.release(20);
            }
            other => panic!("a press at y {y} gave {other:?}"),
        }
    }
    let (start, end, step) = found.expect("no press landed on the feature's bar");
    assert_eq!((start, end), (10, 29));
    assert!(step > 0, "the letters themselves selected the feature");
    // Beside the bar, at the same height, it is a press in the sequence again.
    let _ = window.release(20);
    let beside = pos2(window.at(45).x, FIRST_ROW_Y - step as f32 * 2.0);
    assert_eq!(window.press_at(beside, Modifiers::NONE), Some(Gesture::Press { gap: 45, shift: false }));
}
