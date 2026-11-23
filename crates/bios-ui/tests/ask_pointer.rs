//! The sheet that names a new feature floats over the sequence. A press on it is the sheet's:
//! it must never also put a caret in the bases underneath.

use bios_core::Construct;
use bios_ui::ask::{self, Ask, What};
use bios_ui::rows::{self, Cursor, Gesture};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

const SIZE: egui::Vec2 = vec2(1000.0, 700.0);
/// Where the sheet's top right corner is put.
const CORNER: Pos2 = pos2(980.0, 20.0);

struct Window {
    ctx: egui::Context,
    plan: Option<rows::Plan>,
    c: Construct,
    ask: Ask,
}

impl Window {
    fn frame(&mut self, events: Vec<Event>) -> (Option<Gesture>, Option<ask::Outcome>) {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)), events, ..RawInput::default() };
        let (mut gesture, mut outcome) = (None, None);
        let (c, plan, sheet) = (&self.c, &mut self.plan, &mut self.ask);
        self.ctx
            .run_ui(input, |ui| {
                gesture = rows::show(ui, c, 1, plan, &Cursor::default());
                outcome = ask::show(ui, CORNER, sheet);
            })
            .drop_without_applying_deltas();
        (gesture, outcome)
    }

    fn press(&mut self, pos: Pos2) -> (Option<Gesture>, Option<ask::Outcome>) {
        let down = Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE };
        let pressed = self.frame(vec![Event::PointerMoved(pos), down]);
        let up = Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE };
        let released = self.frame(vec![up]);
        (pressed.0, pressed.1.or(released.1))
    }
}

#[test]
fn a_press_on_the_sheet_does_not_reach_the_sequence_under_it() {
    let c = Construct { name: "t".into(), sequence: "ACGT".repeat(150), ..Construct::default() };
    let mut window = Window { ctx: egui::Context::default(), plan: None, c, ask: Ask::new(What::Feature, true) };
    // Two quiet frames, so the sheet is on screen and known to be on top.
    assert_eq!(window.frame(Vec::new()), (None, None));
    assert_eq!(window.frame(Vec::new()), (None, None));

    // On the sheet's own paper, between its controls.
    let on_sheet = CORNER + vec2(-ask::WIDTH / 2.0, 6.0);
    assert_eq!(window.press(on_sheet), (None, None));
    // Well clear of the sheet the same press is a caret in the sequence.
    let (gesture, outcome) = window.press(pos2(120.0, 400.0));
    assert!(matches!(gesture, Some(Gesture::Press { shift: false, .. })), "{gesture:?}");
    assert_eq!(outcome, None);
}

#[test]
fn escape_lets_go_of_the_sheet() {
    let c = Construct { name: "t".into(), sequence: "ACGT".repeat(150), ..Construct::default() };
    let mut window = Window { ctx: egui::Context::default(), plan: None, c, ask: Ask::new(What::Primer, false) };
    assert_eq!(window.frame(Vec::new()), (None, None));
    let escape = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    assert_eq!(window.frame(vec![escape]).1, Some(ask::Outcome::Cancelled));
}
