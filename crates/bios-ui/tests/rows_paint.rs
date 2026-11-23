//! What the sequence rows paint, read back from one frame run without a window: the letters, the
//! band behind every other row and where the first row stands.

use bios_core::Construct;
use bios_layout::rows as layout;
use bios_ui::rows::{self, Cursor};
use egui::{Color32, Pos2, RawInput, Rect, Shape, vec2};

const SIZE: egui::Vec2 = vec2(1000.0, 700.0);

struct Painted {
    /// Every text drawn, with its top-left and colour.
    texts: Vec<(String, Pos2, Color32)>,
    /// Every filled rectangle drawn.
    rects: Vec<(Rect, Color32)>,
}

fn collect(shape: &Shape, out: &mut Painted) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
        Shape::Text(t) => {
            let color = t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color);
            out.texts.push((t.galley.text().to_owned(), t.pos, color));
        }
        Shape::Rect(r) => out.rects.push((r.rect, r.fill)),
        _ => {}
    }
}

fn paint(c: &Construct, options: rows::Options) -> Painted {
    let ctx = egui::Context::default();
    let mut plan = None;
    let mut out = Painted { texts: Vec::new(), rects: Vec::new() };
    // Two frames: the first lays the scroll area out, the second paints into it.
    for _ in 0..2 {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)), ..RawInput::default() };
        let full = ctx.run_ui(input, |ui| {
            rows::show_with(ui, c, 1, &mut plan, &Cursor::default(), options);
        });
        out = Painted { texts: Vec::new(), rects: Vec::new() };
        for clipped in &full.shapes {
            collect(&clipped.shape, &mut out);
        }
        full.drop_without_applying_deltas();
    }
    out
}

fn construct(sequence: &str) -> Construct {
    Construct { name: "t".into(), sequence: sequence.into(), ..Construct::default() }
}

fn letters(p: &Painted) -> Vec<&str> {
    p.texts.iter().filter(|(t, _, _)| t.chars().count() == 1).map(|(t, _, _)| t.as_str()).collect()
}

#[test]
fn the_bases_print_in_the_case_the_construct_holds_them() {
    let p = paint(&construct(&"ACGTacgt".repeat(10)), rows::Options::default());
    let shown = letters(&p);
    assert!(shown.contains(&"A") && shown.contains(&"a"), "{shown:?}");
    assert!(shown.contains(&"T") && shown.contains(&"t"), "the reverse strand keeps the case too");
}

#[test]
fn every_other_row_sits_on_a_faint_band_margin_to_margin() {
    let p = paint(&construct(&"ACGT".repeat(150)), rows::Options::default());
    let band = bios_ui::theme::alpha(bios_ui::theme::INK, 0.035);
    let bands: Vec<Rect> = p.rects.iter().filter(|(_, fill)| *fill == band).map(|(r, _)| *r).collect();
    assert!(bands.len() >= 2, "{bands:?}");
    assert!(bands.iter().all(|r| r.width() >= SIZE.x - 20.0), "margin to margin: {bands:?}");
    assert!(bands[1].top() > bands[0].bottom() + 10.0, "a band on every other row, not every row: {bands:?}");
}

#[test]
fn the_first_row_starts_at_the_top_of_the_rows_with_no_air_above_it() {
    let c = construct(&"ACGT".repeat(150));
    let p = paint(&c, rows::Options::default());
    let band = bios_ui::theme::alpha(bios_ui::theme::INK, 0.035);
    let second_row_band = p.rects.iter().find(|(_, fill)| *fill == band).map(|(r, _)| *r).expect("no band");
    // A bare row is its fixed tracks and its gap; the second row's band starts half a gap above it.
    let bare = layout::row_height(layout::Lanes::default(), layout::Tracks::default());
    assert_eq!(second_row_band.top(), (bare - layout::track::ROW_GAP / 2.0) as f32);
}

fn color_of(p: &Painted, base: &str) -> Color32 {
    p.texts.iter().find(|(t, _, _)| t == base).map(|(_, _, c)| *c).expect("base not drawn")
}

#[test]
fn the_base_colour_follows_the_style_chosen_and_calm_greys_every_letter() {
    use bios_ui::theme::{INK, MUTED, hex};
    use bios_ui::view_tab::{BaseStyle, Layers};
    let c = construct(&"ACGTN".repeat(12));
    let style = |base_style| paint(&c, rows::Options { layers: Layers { base_style, ..Layers::default() }, ..rows::Options::default() });
    let calm = paint(&c, rows::Options::default());
    assert!(["A", "C", "G", "T", "N"].iter().all(|b| color_of(&calm, b) == MUTED), "calm is the default");
    let vivid = style(BaseStyle::Vivid);
    assert_eq!(["A", "T", "C", "G", "N"].map(|b| color_of(&vivid, b)), [hex(0xe07a00), hex(0xda1f22), hex(0x045afa), hex(0x00934e), INK]);
    let gc = style(BaseStyle::GcAt);
    assert_eq!(["A", "T", "C", "G"].map(|b| color_of(&gc, b)), [hex(0xb3a795), hex(0xb3a795), hex(0x0e7c86), hex(0x0e7c86)]);
    assert_eq!(color_of(&style(BaseStyle::Black), "A"), INK);
    assert_eq!(color_of(&style(BaseStyle::Muted), "G"), hex(0x3f8158));
}

#[test]
fn switching_the_enzyme_names_off_hides_them_and_gives_their_lane_back() {
    use bios_ui::view_tab::Layers;
    let c = construct(&format!("{}GAATTC{}", "ACGT".repeat(5), "ACGT".repeat(40)));
    let band_top = |p: &Painted| {
        let band = bios_ui::theme::alpha(bios_ui::theme::INK, 0.035);
        p.rects.iter().find(|(_, fill)| *fill == band).map(|(r, _)| r.top()).expect("no band")
    };
    let on = paint(&c, rows::Options::default());
    assert!(on.texts.iter().any(|(t, _, _)| t == "EcoRI"), "the name is shown by default");
    let off = paint(&c, rows::Options { layers: Layers { cut_names: false, ..Layers::default() }, ..rows::Options::default() });
    assert!(!off.texts.iter().any(|(t, _, _)| t == "EcoRI"));
    assert_eq!(band_top(&on) - band_top(&off), layout::track::CUTSITE_LABELS as f32, "the first row is a name lane shorter");
}

#[test]
fn the_reverse_strand_and_the_ruler_switch_off_from_the_layers() {
    use bios_ui::view_tab::Layers;
    let c = construct(&"AAAA".repeat(150));
    let with = |layers| paint(&c, rows::Options { layers, ..rows::Options::default() });
    let both = with(Layers::default());
    assert!(letters(&both).contains(&"T"), "the reverse strand reads T opposite every A");
    assert!(both.texts.iter().any(|(t, _, _)| t == "100"), "a fiftieth carries its number");
    let single = with(Layers { reverse_strand: false, ..Layers::default() });
    assert!(!letters(&single).contains(&"T"));
    let band = bios_ui::theme::alpha(bios_ui::theme::INK, 0.035);
    let top = |p: &Painted| p.rects.iter().find(|(_, fill)| *fill == band).map(|(r, _)| r.top()).expect("no band");
    let weld_and_strand = (layout::track::RUNGS + layout::track::REVERSE_SEQUENCE) as f32;
    assert_eq!(top(&both) - top(&single), weld_and_strand, "the weld closes with the strand");
    let bare = with(Layers { ruler: false, ..Layers::default() });
    assert!(!bare.texts.iter().any(|(t, _, _)| t == "100"));
}

/// A linear construct of `bases` with three features: `pol` acting on `pT7`, and `stop` wired to
/// nothing.
fn wired(extra: &str) -> Construct {
    let mut file =
        String::from("LOCUS       t                        240 bp    DNA     linear   SYN 01-JAN-2026\nFEATURES             Location/Qualifiers\n");
    for (location, name, notes) in [
        ("3..12", "pol", "/note=\"bios-id: a\"\n                     /note=\"bios-act: transcribes > b\""),
        ("20..30", "pT7", "/note=\"bios-id: b\""),
        ("40..50", "stop", "/note=\"bios-id: c\""),
    ] {
        file += &format!("     misc_feature    {location}\n                     /label=\"{name}\"\n                     {notes}\n");
    }
    file += extra;
    file += "ORIGIN\n";
    for i in 0..4 {
        file += &format!("{:>9} {}\n", i * 60 + 1, ["acgtacgtac"; 6].join(" "));
    }
    file += "//\n";
    bios_core::genbank::read(&file).unwrap()
}

#[test]
fn a_part_has_its_own_lane_that_the_parts_layer_switches_off_and_the_map_leaves_it_out() {
    use bios_ui::view_tab::Layers;
    let c = wired("     misc_feature    60..90\n                     /label=\"piece\"\n                     /pragma=\"Teselagen_Part\"\n");
    let shown = paint(&c, rows::Options::default());
    assert!(shown.texts.iter().any(|(t, _, _)| t == "piece"), "parts are on by default");
    let off = paint(&c, rows::Options { layers: Layers { parts: false, ..Layers::default() }, ..rows::Options::default() });
    assert!(!off.texts.iter().any(|(t, _, _)| t == "piece"));
    assert!(off.texts.iter().any(|(t, _, _)| t == "pT7"), "the features stay");
    assert!(!bios_ui::map::scene_of(&c).features.iter().any(|f| f.name == "piece"), "a part is not a feature on the map");
}

#[test]
fn the_interactions_layer_draws_what_acts_on_what_and_greys_back_what_the_pointer_is_not_asking_about() {
    use bios_ui::view_tab::Layers;
    let c = wired("");
    let off = paint(&c, rows::Options::default());
    assert!(!off.texts.iter().any(|(t, _, _)| t == "transcribes"), "off until asked for");
    let on = Layers { interactions: true, ..Layers::default() };
    let shown = paint(&c, rows::Options { layers: on, ..rows::Options::default() });
    let name = |p: &Painted, n: &str| p.texts.iter().find(|(t, _, _)| t == n).map(|(_, _, c)| *c).unwrap_or_else(|| panic!("no {n}"));
    let verb = name(&shown, "transcribes");
    // Pointing at the actor lights its target and greys the feature wired to nothing.
    let lit = paint(&c, rows::Options { layers: on, lit: Some(0), ..rows::Options::default() });
    assert_eq!(name(&lit, "transcribes"), verb, "the edge is part of the answer");
    assert_eq!(name(&lit, "pT7"), name(&shown, "pT7"));
    assert!(name(&lit, "stop").a() < name(&shown, "stop").a(), "the unrelated feature steps back");
    let elsewhere = paint(&c, rows::Options { layers: on, lit: Some(2), ..rows::Options::default() });
    assert!(name(&elsewhere, "transcribes").a() < verb.a(), "an edge not touching the lit feature greys back");
}
