//! The editor pane's chrome, run without a window: the tool row answers presses where it drew its
//! buttons, a greyed arrow does nothing, the navigator moves and zooms the map's window, and the
//! wheel pans the map.
//!
//! The pane is the whole 800 x 600 screen. The tool row stands 16 above the 68px seam, so its
//! buttons span y 482..516; it is 397 wide (history 77, three groups of 98, gaps 14, 6 and 6) and
//! centred, so it starts at x 202.

use bios_core::Construct;
use bios_layout::overview::navigate;
use bios_layout::overview::viewport::{Projection, Viewport};
use bios_layout::pane::{Focus, MapStyle};
use bios_ui::canvas_tools::{self, Action, Tools};
use bios_ui::map::{self, MapCursor, MapView};
use bios_ui::{navigator, theme};
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

const SCREEN: egui::Vec2 = vec2(800.0, 600.0);

fn input(events: Vec<Event>) -> RawInput {
    RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), events, ..RawInput::default() }
}

fn press(at: Pos2) -> Vec<Event> {
    vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]
}

fn release(at: Pos2) -> Vec<Event> {
    vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]
}

fn tools() -> Tools {
    Tools { can_undo: false, can_redo: true, focus: Focus::Split, open: None, side: None }
}

/// Runs frames of the tool row, one per batch of events, and gives back what each asked for.
fn row(batches: Vec<Vec<Event>>) -> Vec<Option<Action>> {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let tools = tools();
    let mut asked = Vec::new();
    for events in std::iter::once(Vec::new()).chain(batches) {
        let mut action = None;
        let out = ctx.run_ui(input(events), |ui| {
            action = canvas_tools::show(ui, Rect::from_min_size(Pos2::ZERO, SCREEN), 516.0, &tools).0;
        });
        out.drop_without_applying_deltas();
        asked.push(action);
    }
    asked
}

#[test]
fn a_press_on_a_segment_asks_for_it() {
    // The row is 262 wide, centred at 400, so it starts at 269. "Map only" is the first segment
    // of the view group, which now follows the history straight away: 269 + 77 + 14 + 3 + 15.
    let at = pos2(378.0, 499.0);
    let asked = row(vec![press(at), release(at)]);
    assert!(asked.contains(&Some(Action::Focus(Focus::Map))), "{asked:?}");
}

/// The viewing controls in the map's top right corner (`map_tools`), run the same way.
fn corner(batches: Vec<Vec<Event>>) -> Vec<Option<bios_ui::map_tools::Action>> {
    use bios_ui::map_tools::{MapTools, show};
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let tools = MapTools { style: MapStyle::Circular, fold_reason: None, view_open: false };
    let map = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0));
    let mut asked = Vec::new();
    for events in std::iter::once(Vec::new()).chain(batches) {
        let mut action = None;
        let out = ctx.run_ui(input(events), |ui| action = show(ui, map, &tools).0);
        out.drop_without_applying_deltas();
        asked.push(action);
    }
    asked
}

#[test]
fn the_maps_corner_sets_the_style_and_opens_what_is_shown() {
    use bios_ui::map_tools::Action as Corner;
    // 140 wide, 16 in from the map's right edge, so it starts at 644; the style group is its
    // first 98 and Linear its second segment: 644 + 3 + 31 + 15, 16 + 2 + 15 down.
    let linear = pos2(693.0, 33.0);
    let asked = corner(vec![press(linear), release(linear)]);
    assert!(asked.contains(&Some(Corner::Style(MapStyle::Linear))), "{asked:?}");
    // The View button is the group after it: 644 + 98 + 6 + 3 + 15.
    let view = pos2(766.0, 33.0);
    let asked = corner(vec![press(view), release(view)]);
    assert!(asked.contains(&Some(Corner::View)), "{asked:?}");
    // Nothing in the air the controls stand in from the corner, above them or beside them.
    for miss in [pos2(790.0, 33.0), pos2(693.0, 8.0), pos2(600.0, 33.0)] {
        assert!(corner(vec![press(miss), release(miss)]).iter().all(Option::is_none), "a press at {miss:?} asked for something");
    }
}

#[test]
fn forward_works_and_a_greyed_back_does_nothing() {
    let back = pos2(272.0 + 17.0, 499.0);
    let asked = row(vec![press(back), release(back)]);
    assert!(asked.iter().all(Option::is_none), "{asked:?}");
    let forward = pos2(272.0 + 36.0 + 17.0, 499.0);
    let asked = row(vec![press(forward), release(forward)]);
    assert!(asked.contains(&Some(Action::Forward)), "{asked:?}");
}

/// The seam over the bottom 68px; its track runs from x 44 to 756 (712 wide), 13px below its top.
fn seam(vp: &mut Viewport, batches: Vec<Vec<Event>>) {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let rect = Rect::from_min_max(pos2(0.0, 532.0), pos2(800.0, 600.0));
    for events in std::iter::once(Vec::new()).chain(batches) {
        let out = ctx.run_ui(input(events), |ui| navigator::show(ui, rect, 6000, navigator::Rail::default(), vp, false));
        out.drop_without_applying_deltas();
    }
}

#[test]
fn a_press_on_the_rail_sends_the_window_there() {
    let mut vp = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
    // Three quarters of the way along the rail, below the window box.
    let at = pos2(44.0 + 712.0 * 0.75, 532.0 + 13.0 + 30.0);
    seam(&mut vp, vec![press(at), release(at)]);
    assert!((vp.center - 0.75).abs() < 1e-6, "{vp:?}");
}

#[test]
fn dragging_the_right_grip_zooms_out_from_the_left_end() {
    let mut vp = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
    let grip = pos2(44.0 + 712.0 * 0.4, 532.0 + 13.0 + 8.0);
    let to = grip + vec2(71.2, 0.0);
    seam(&mut vp, vec![press(grip), vec![Event::PointerMoved(to)], release(to)]);
    assert!((vp.width - 0.5).abs() < 1e-6, "{vp:?}");
    assert!((vp.center - 0.25).abs() < 1e-6, "{vp:?}");
}

#[test]
fn the_wheel_slides_a_line_map() {
    let c = Construct { name: "t".into(), circular: false, sequence: "ACGT".repeat(1500), ..Construct::default() };
    let mut view = MapView { vp: navigate::style_jump(navigate::OPENING, false), ring: false };
    assert!((view.vp.center - 0.2).abs() < 1e-12);
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 400.0));
    let over = pos2(400.0, 200.0);
    let wheel = Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, -80.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
    };
    for events in [vec![Event::PointerMoved(over)], vec![wheel], Vec::new(), Vec::new(), Vec::new()] {
        let out = ctx.run_ui(input(events), |ui| {
            map::show_view(ui, rect, &c, &MapCursor::default(), &mut view, &[], Default::default());
        });
        out.drop_without_applying_deltas();
    }
    assert!(view.vp.center > 0.2, "{:?}", view.vp);
    assert_eq!(view.vp.width, 0.4);
}

#[test]
fn the_dial_narrows_what_is_drawn_and_the_foot_says_so() {
    use bios_ui::enzymes_menu::{CutsiteView, cut_sites, summary};
    // EcoRI once, BamHI twice, in a stretch of filler that neither cuts.
    let filler = "ACACACACACACACACACAC";
    let sequence = format!("{filler}GAATTC{filler}GGATCC{filler}GGATCC{filler}");
    let c = Construct { name: "t".into(), circular: false, sequence, ..Construct::default() };
    let once = cut_sites(&c, CutsiteView { limit: Some(1), all: false });
    assert!(once.iter().all(|s| s.enzyme != "BamHI") && once.iter().any(|s| s.enzyme == "EcoRI"));
    let twice = cut_sites(&c, CutsiteView { limit: Some(2), all: false });
    assert_eq!(twice.iter().filter(|s| s.enzyme == "BamHI").count(), 2);
    assert!(cut_sites(&c, CutsiteView { limit: Some(0), all: false }).is_empty());
    assert_eq!(summary(&[], Some(0)), "No cut sites drawn");
    assert_eq!(summary(&[], None), "Nothing in this set cuts here");
    assert_eq!(summary(&[], Some(2)), "Nothing cuts this few times — try All");
    let eco: Vec<_> = once.iter().filter(|s| s.enzyme == "EcoRI").cloned().collect();
    assert_eq!(summary(&eco, Some(1)), "1 enzyme cuts once");
    let bam: Vec<_> = twice.iter().filter(|s| s.enzyme == "BamHI").cloned().collect();
    assert_eq!(summary(&bam, Some(2)), "1 enzyme · 2 sites");
}

#[test]
fn the_layers_popover_switches_enzyme_names_and_closes_on_a_press_elsewhere() {
    use bios_ui::view_tab::{self, Layers, Outcome};
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    // The popover is 590 tall and opens 6 above its button, so with the button at y 700 its top is
    // at 104; the rows start under the 25px title at 135, each 32 tall. "Primers" is the second of
    // them (y 167..199) and "Enzyme names" the third (199..231).
    let trigger = Rect::from_min_size(pos2(500.0, 700.0), vec2(30.0, 30.0));
    let screen = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 900.0))), ..RawInput::default() };
    let run = |events: Vec<Event>| {
        let mut outcome = None;
        let out = ctx.run_ui(RawInput { events, ..screen.clone() }, |ui| outcome = view_tab::show(ui, trigger, Layers::default()));
        out.drop_without_applying_deltas();
        outcome
    };
    run(Vec::new()); // an Area is laid out unseen on its first frame
    run(Vec::new()); // an Area is laid out unseen on its first frame
    let row = pos2(300.0, 215.0);
    let outcomes = [run(press(row)), run(release(row))];
    assert!(outcomes.contains(&Some(Outcome::Changed(Layers { cut_names: false, ..Layers::default() }))), "{outcomes:?}");
    // Primers is native's own row, on at rest, and switches like any other.
    let row = pos2(300.0, 183.0);
    let outcomes = [run(press(row)), run(release(row))];
    assert!(outcomes.contains(&Some(Outcome::Changed(Layers { primers: false, ..Layers::default() }))), "{outcomes:?}");
    assert_eq!(run(press(pos2(100.0, 850.0))), Some(Outcome::Close));
}

#[test]
fn the_layers_popover_picks_the_base_colour() {
    use bios_ui::view_tab::{self, BaseStyle, Layers, Outcome};
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    // As above, the popover's top is at 104. The base colour pills follow the title and ten
    // 32px rows: Calm and Muted side by side at y 482..511, Muted from x 403 to 516.
    let trigger = Rect::from_min_size(pos2(500.0, 700.0), vec2(30.0, 30.0));
    let screen = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 900.0))), ..RawInput::default() };
    let run = |events: Vec<Event>| {
        let mut outcome = None;
        let out = ctx.run_ui(RawInput { events, ..screen.clone() }, |ui| outcome = view_tab::show(ui, trigger, Layers::default()));
        out.drop_without_applying_deltas();
        outcome
    };
    run(Vec::new());
    run(Vec::new());
    let muted = pos2(450.0, 495.0);
    let outcomes = [run(press(muted)), run(release(muted))];
    assert!(outcomes.contains(&Some(Outcome::Changed(Layers { base_style: BaseStyle::Muted, ..Layers::default() }))), "{outcomes:?}");
}
