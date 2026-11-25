//! A picture of a construct, drawn without a window: that each of the three views holds what the
//! window draws, that it is written at the size asked for, and that nothing a file can hold takes
//! the drawing down.

use bios_core::{Construct, Feature, Segment, Strand};
use bios_engine::picture::{Ask, Picture, View};
use bios_ui::picture;

fn feature(name: &str, kind: &str, start: usize, end: usize) -> Feature {
    Feature {
        name: name.into(),
        kind: kind.into(),
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

/// 1,200 bases with a gene, a promoter and one EcoRI site at base 601 (`GAATTC`).
fn demo(circular: bool) -> Construct {
    let mut sequence = "ACGT".repeat(300);
    sequence.replace_range(600..606, "GAATTC");
    Construct {
        name: "demo".into(),
        circular,
        sequence,
        features: vec![feature("gene", "CDS", 200, 499), feature("pT7", "promoter", 700, 760)],
        ..Construct::default()
    }
}

fn ask<'a>(c: &'a Construct, view: View, start: i64, end: i64) -> Ask<'a> {
    Ask { construct: c, view, start, end, width: 1000.0, height: 700.0, square: false }
}

fn drawn(c: &Construct, view: View, start: i64, end: i64) -> Picture {
    picture::draw(&ask(c, view, start, end))
}

/// Every piece of text in the document, one entry per `<text>` element.
fn words(svg: &str) -> Vec<String> {
    svg.split("<text ")
        .skip(1)
        .filter_map(|rest| rest.split_once('>').and_then(|(_, after)| after.split_once("</text>")).map(|(w, _)| w.to_string()))
        .collect()
}

fn says(svg: &str, word: &str) -> bool {
    words(svg).iter().any(|w| w == word)
}

/// The letters of a name written one at a time along an arc, in the order they appear.
fn letters(svg: &str) -> String {
    words(svg).concat()
}

#[test]
fn the_ring_holds_its_backbone_its_features_its_cuts_and_what_you_are_looking_at() {
    let c = demo(true);
    let picture = drawn(&c, View::Circular, 0, 1199);
    let svg = &picture.svg;
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"), "{}", &svg[..60.min(svg.len())]);
    assert!(svg.contains("width=\"1000\" height=\"700\""), "the size asked for");
    assert_eq!(picture.size, (1000.0, 700.0));
    assert_eq!(picture.drawn, (0, 1199), "the whole molecule");
    // The backbone: one circle with the hairline's colour.
    assert!(svg.contains("<circle") && svg.contains("#d2d2d7"), "the coordinate circle is drawn");
    // What you are looking at, in the corner, as the window writes it.
    assert!(says(svg, "demo"), "{:?}", words(svg));
    assert!(says(svg, "1,200 bp · circular"), "{:?}", words(svg));
    // The feature names, which on a ring are set letter by letter along their own arc.
    let letters = letters(svg);
    assert!(letters.contains("gene"), "{letters}");
    assert!(letters.contains("pT7"), "{letters}");
    // The one enzyme that cuts this construct once.
    assert!(says(svg, "EcoRI"), "{:?}", words(svg));
}

#[test]
fn the_line_holds_the_same_construct_unrolled() {
    let c = demo(false);
    let svg = drawn(&c, View::Linear, 0, 1199).svg;
    assert!(says(&svg, "demo") && says(&svg, "1,200 bp · linear"), "{:?}", words(&svg));
    assert!(says(&svg, "gene"), "a wide bar carries its own name: {:?}", words(&svg));
    assert!(says(&svg, "EcoRI"));
    // The coordinate scale, which the ring writes round its circle and the line under its axis.
    assert!(says(&svg, "1") && says(&svg, "1,000"), "{:?}", words(&svg));
    assert!(!svg.contains("<circle"), "a line has no ring");
}

#[test]
fn the_sequence_holds_both_strands_its_features_and_only_the_rows_that_fit() {
    let c = demo(true);
    let picture = drawn(&c, View::Sequence, 0, 1199);
    let svg = &picture.svg;
    // The bases, in the sequence face.
    assert!(svg.contains("'SF Mono'"), "the sequence face is asked for");
    let bases = words(svg).iter().filter(|w| ["A", "C", "G", "T"].contains(&w.as_str())).count();
    assert!(bases > 400, "both strands print their letters: {bases}");
    assert!(says(svg, "gene"), "{:?}", words(svg));
    // Whole rows, from the top, and only as many as the picture is tall enough to hold.
    assert_eq!(picture.drawn.0, 0);
    assert!(picture.drawn.1 > 0 && picture.drawn.1 < 1199, "{:?} of 1,200 bases", picture.drawn);
    assert!(picture.size.1 <= 700.0 && picture.size.1 > 100.0, "{:?}", picture.size);

    // A stretch in the middle gives the rows that hold it, not the ones at the start.
    let middle = drawn(&c, View::Sequence, 700, 760);
    assert!(middle.drawn.0 <= 700 && middle.drawn.1 >= 760, "{:?}", middle.drawn);
    assert!(says(&middle.svg, "pT7"), "{:?}", words(&middle.svg));

    // The cut the rows show at rest, where it is.
    let cut = drawn(&c, View::Sequence, 595, 610);
    assert!(says(&cut.svg, "EcoRI"), "{:?}", words(&cut.svg));
}

#[test]
fn a_square_picture_is_the_same_drawing_centred_in_a_square() {
    let c = demo(true);
    let plain = picture::draw(&ask(&c, View::Circular, 0, 1199));
    let square = picture::draw(&Ask { square: true, ..ask(&c, View::Circular, 0, 1199) });
    assert!(square.svg.contains("width=\"1000\" height=\"1000\""), "padded out to its longer side");
    assert!(square.svg.contains("translate(0 150)"), "centred: 150 above and 150 below");
    assert_eq!(square.size, plain.size, "the drawing itself is the same size either way");
    assert_eq!(square.drawn, plain.drawn);
    // Clipped to the drawing's own rectangle, so the padding holds nothing the plain one does not.
    assert!(square.svg.contains("<rect x=\"0\" y=\"0\" width=\"1000\" height=\"700\"/></clipPath>"), "{}", &square.svg[..400]);
}

#[test]
fn every_number_in_the_file_is_a_number_a_reader_can_read() {
    let c = demo(true);
    for view in [View::Circular, View::Linear, View::Sequence] {
        let svg = drawn(&c, view, 0, 1199).svg;
        assert!(!svg.contains("NaN") && !svg.contains("inf"), "{}", view.name());
        // Well-formed enough that nothing was left open and no raw angle bracket slipped into a
        // name: as many opening tags as closing ones or self-closing ones.
        assert_eq!(svg.matches("<text ").count(), svg.matches("</text>").count(), "{}", view.name());
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count(), "{}", view.name());
        assert_eq!(svg.matches("<svg").count(), svg.matches("</svg>").count(), "{}", view.name());
    }
}

#[test]
fn a_name_that_would_break_the_file_is_written_as_text() {
    let mut c = demo(true);
    c.name = "a <b> & c".into();
    c.features = vec![feature("<script>", "CDS", 100, 900)];
    for view in [View::Circular, View::Linear, View::Sequence] {
        let svg = drawn(&c, view, 0, 1199).svg;
        assert!(!svg.contains("<script>"), "{}", view.name());
        assert!(svg.contains("&lt;") || svg.contains("&amp;"), "{}: {:?}", view.name(), words(&svg));
    }
}

#[test]
fn nothing_a_file_can_hold_takes_the_drawing_down() {
    // One base, a feature off the end of the molecule, a feature that is the whole of it, a
    // feature whose end is before its start, and a hundred of them at once.
    let mut odd = Construct { name: String::new(), circular: true, sequence: "A".into(), ..Construct::default() };
    odd.features = vec![feature("off", "CDS", 50, 90), feature("all", "CDS", 0, 0)];
    let mut crowded = demo(true);
    crowded.features = (0..100).map(|i| feature(&format!("f{i}"), "CDS", i * 7, i * 7 + 400)).collect();
    crowded.features.push(feature("through the origin", "CDS", 1150, 40));
    let mut linear_wrapped = demo(false);
    linear_wrapped.features = vec![feature("backwards", "CDS", 900, 100)];

    for c in [&odd, &crowded, &linear_wrapped, &demo(true)] {
        for view in [View::Circular, View::Linear, View::Sequence] {
            // The whole of it, one base of it, a backwards stretch, and a stretch off the end.
            for (start, end) in [(0, 1199), (0, 0), (900, 100), (-5, 99_999), (1199, 1199)] {
                let picture = picture::draw(&Ask { width: 300.0, height: 220.0, ..ask(c, view, start, end) });
                assert!(picture.svg.contains("</svg>"), "{} {start}..{end}", view.name());
            }
        }
    }
}

#[test]
fn the_engine_is_handed_this_and_a_construct_with_no_bases_is_refused_rather_than_drawn_empty() {
    use bios_engine::picture::Painter as _;
    let empty = Construct { name: "nothing".into(), circular: true, ..Construct::default() };
    let Err(refused) = picture::Pictures.draw(&ask(&empty, View::Circular, 0, 0)) else { panic!("nothing to draw") };
    assert!(refused.contains("no bases"), "{refused}");
    assert!(picture::Pictures.draw(&ask(&demo(true), View::Circular, 0, 1199)).is_ok());
    assert!(picture::set_up(), "the engine takes it the first time it is offered");
    assert!(!picture::set_up(), "and keeps the first one");
}

#[test]
fn a_line_comes_down_to_what_it_drew_while_a_ring_keeps_its_frame() {
    let c = demo(true);
    // A short stretch is a band: the picture is a fraction of the height asked for, and still
    // holds the title and the feature in view.
    let band = drawn(&c, View::Linear, 100, 300);
    assert!(band.size.1 < 300.0, "a 200-base band wanted {} of the 700 asked for", band.size.1);
    assert!(says(&band.svg, "demo") && letters(&band.svg).contains("gene"), "{}", letters(&band.svg));
    // The whole molecule is trimmed to its own drawing too. How tall that is depends on what is
    // in view — a zoomed band can be the taller of the two, because it has the room to name a
    // cut site the whole view leaves off — so the rule is only that neither pads out the frame.
    let whole = drawn(&c, View::Linear, 0, 1199);
    assert!(whole.size.1 < 700.0, "the whole molecule padded out to {}", whole.size.1);
    // A ring is drawn to the frame it is given, so it is not trimmed.
    let ring = drawn(&c, View::Circular, 0, 1199);
    assert_eq!(ring.size.1, 700.0);
}
