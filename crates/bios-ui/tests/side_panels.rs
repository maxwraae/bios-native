//! The side panels, run without a window: the Contents list shows what the web's does on the
//! reference plasmid, the Features switch takes the annotations off what is drawn and a press on
//! what is left still names the file's own feature, and the tool row's tag button asks for Contents.

use bios_core::genbank;
use bios_layout::pane::{Focus, MapStyle};
use bios_ui::canvas_tools::{self, Action, Panel, Tools};
use bios_ui::contents_panel::listed_features;
use bios_ui::pane::{drawn, feature_in_file};
use bios_ui::theme;
use bios_ui::view_tab::Layers;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

fn reference() -> bios_core::Construct {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bios-core/tests/fixtures/pUC19.gb");
    genbank::read(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The comparison with the web's own Contents, on the lab construct the web was read on.
mod private {
    use super::*;

    /// A file in the private fixtures folder, which keeps this repository's layout.
    fn private_fixture(path: &str) -> std::path::PathBuf {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        std::path::PathBuf::from(root).join(path)
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn contents_lists_the_features_the_web_lists_in_the_order_they_start() {
        let path = private_fixture("crates/bios-core/tests/fixtures/lab_construct.gb");
        let c = genbank::read(&std::fs::read_to_string(path).unwrap()).unwrap();
        let listed = listed_features(&c, false);
        // The web's Contents says Features 30 on this plasmid: no `source`, no splice-map hits.
        assert_eq!(listed.len(), 30);
        let names: Vec<&str> = listed.iter().take(3).map(|i| c.features[*i].name.as_str()).collect();
        assert_eq!(names, ["CMV enhancer", "CMV promoter", "T7 promoter"]);
        assert!(listed.windows(2).all(|w| c.features[w[0]].start <= c.features[w[1]].start));
        assert!(listed_features(&c, true).len() > 30, "asked for, the splice-map hits are listed too");
    }
}

#[test]
fn contents_lists_the_features_in_the_order_they_start() {
    let c = reference();
    let listed = listed_features(&c, false);
    // pUC19's eleven features, less the `source` and the two M13 primer sites, which Contents
    // leaves to the primer list.
    assert_eq!(listed.len(), 8);
    let names: Vec<&str> = listed.iter().take(3).map(|i| c.features[*i].name.as_str()).collect();
    assert_eq!(names, ["lacZ alpha", "MCS", "lac operator"]);
    assert!(listed.windows(2).all(|w| c.features[w[0]].start <= c.features[w[1]].start));
}

#[test]
fn features_off_draws_only_the_primer_sites_and_a_press_maps_back_to_the_file() {
    let c = reference();
    let primer = c.features.iter().position(|f| f.kind == "primer_bind");
    let off = Layers { features: false, ..Layers::default() };
    let bare = drawn(&c, off);
    assert!(bare.features.iter().all(|f| f.kind == "primer_bind"));
    if let Some(primer) = primer {
        assert_eq!(feature_in_file(&c, off, 0), Some(primer));
    }
    assert_eq!(drawn(&c, Layers::default()).features.len(), c.features.len());
    assert_eq!(feature_in_file(&c, Layers::default(), 7), Some(7));
}

#[test]
fn the_tag_button_asks_for_contents() {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let tools = Tools { can_undo: false, can_redo: false, focus: Focus::Split, style: MapStyle::Linear, fold_reason: None, open: None, side: None };
    // The row is 397 wide and centred at 202; the panels group is its last 98, the tag its third
    // segment: 202 + 397 - 98 + 3 + 62 + 15.
    let at = pos2(581.0, 499.0);
    let press =
        vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }];
    let release = vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }];
    let mut asked = Vec::new();
    for events in [Vec::new(), press, release] {
        let input = RawInput { screen_rect: Some(screen), events, ..RawInput::default() };
        let out = ctx.run_ui(input, |ui| asked.push(canvas_tools::show(ui, screen, 516.0, &tools).0));
        out.drop_without_applying_deltas();
    }
    assert!(asked.contains(&Some(Action::Panel(Panel::Contents))), "{asked:?}");
}

#[test]
fn a_tab_dragged_over_its_neighbour_asks_to_move_there() {
    use bios_ui::tab_strip::{Pressed, Tab, TabStrip};
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
    let strip = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 38.0));
    let tab = |name| Tab { name, circular: true, dirty: false };
    let tabs = [tab("a"), tab("b"), tab("c")];
    let mut state = TabStrip::default();
    // Each tab is at its least, 112 wide, from x 10, neighbours sharing a line.
    let down = |at| {
        vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]
    };
    let batches = vec![
        Vec::new(),
        down(pos2(60.0, 25.0)),
        vec![Event::PointerMoved(pos2(70.0, 25.0))],
        vec![Event::PointerMoved(pos2(90.0, 25.0))],
        vec![Event::PointerMoved(pos2(180.0, 25.0))],
    ];
    let mut asked = Vec::new();
    for events in batches {
        let input = RawInput { screen_rect: Some(screen), events, ..RawInput::default() };
        let out = ctx.run_ui(input, |ui| asked.push(state.show(ui, strip, 0.0, &tabs, Some(0), theme::PLACE_EDIT)));
        out.drop_without_applying_deltas();
    }
    assert!(asked.contains(&Some(Pressed::Move(0, 1))), "{asked:?}");
}
