//! Cut sites on the map, ring and line: every site in view gets its hash across the strand, a name
//! goes on a leader where a row or a column has room, and isoschizomers share one name. The rules
//! are `layoutLineAnnotations` and `layoutRingAnnotations` in Bios
//! `editor/overview/PlasmidOverview.tsx`; the web app records no drawing with cuts on it, so these
//! check the rules rather than a recorded picture.

use bios_layout::overview::line_map::layout_line;
use bios_layout::overview::ring_map::{Anchor, layout_ring, ring_margin, ring_margin_for_cuts};
use bios_layout::overview::scene::{Frame, MapScene};

/// Every glyph 6px wide.
fn measure(text: &str, _font: f64) -> f64 {
    text.chars().count() as f64 * 6.0
}

fn scene(circular: bool) -> MapScene {
    MapScene::new(&[], 1000, circular).with_cuts(vec![(100, "EcoRI".into()), (500, "XhoI".into()), (500, "PaeR7I".into()), (900, "BamHI".into())])
}

#[test]
fn the_line_hashes_every_cut_and_names_co_cutters_once() {
    let map = layout_line(&scene(false), Frame::line(1000.0, 400.0), measure);
    assert_eq!(map.cuts.len(), 3);
    let names: Vec<_> = map.cuts.iter().filter_map(|c| c.label.as_ref().map(|l| l.text.clone())).collect();
    assert_eq!(names, vec!["EcoRI", "XhoI - PaeR7I", "BamHI"]);
    for cut in &map.cuts {
        // The hash straddles the axis by half a lane and 5 more.
        assert!(cut.y0 < map.axis_y && cut.y1 > map.axis_y);
        assert!((cut.y1 - map.axis_y - (map.thickness / 2.0 + 5.0)).abs() < 1e-9);
        let label = cut.label.as_ref().map(|l| l.y).unwrap_or(f64::INFINITY);
        assert!(label < cut.y0, "a name sits above its hash");
    }
}

#[test]
fn the_line_draws_only_the_cuts_in_its_window() {
    let frame = Frame { center: 0.2, width: 0.4, ..Frame::line(1000.0, 400.0) };
    let map = layout_line(&scene(false), frame, measure);
    // 0..400 is in view: only EcoRI at 100.
    assert_eq!(map.cuts.len(), 1);
}

#[test]
fn the_ring_hashes_every_cut_and_sets_names_away_from_it() {
    let map = layout_ring(&scene(true), Frame::ring(1000.0, 600.0), measure);
    assert_eq!(map.cuts.len(), 3);
    for cut in &map.cuts {
        let r = |p: bios_layout::overview::projection::Point| ((p.x - map.geom.cx).powi(2) + (p.y - map.geom.cyc).powi(2)).sqrt();
        assert!((r(cut.tin) - (map.axis_r - 6.0)).abs() < 1e-6 && (r(cut.tout) - (map.axis_r + 6.0)).abs() < 1e-6);
        if let Some(label) = &cut.label {
            // A name on the right is set flush right toward the frame edge, on the left flush left.
            let right = label.x > map.geom.cx;
            assert_eq!(label.anchor, if right { Anchor::End } else { Anchor::Start });
        }
    }
    assert!(map.cuts.iter().all(|c| c.label.is_some()));
}

#[test]
fn with_enzyme_names_off_every_hash_stays_and_no_name_is_set() {
    let line = layout_line(&scene(false), Frame { cut_names: false, ..Frame::line(1000.0, 400.0) }, measure);
    let ring = layout_ring(&scene(true), Frame { cut_names: false, ..Frame::ring(1000.0, 600.0) }, measure);
    assert_eq!((line.cuts.len(), ring.cuts.len()), (3, 3));
    assert!(line.cuts.iter().all(|c| c.label.is_none()) && ring.cuts.iter().all(|c| c.label.is_none()));
}

#[test]
fn the_ring_comes_in_to_make_room_for_the_longest_name() {
    assert_eq!(ring_margin_for_cuts(1000.0, 600.0, 0.0), ring_margin(1000.0, 600.0));
    // 600 * 0.15 = 90; a 100px name asks for 130, under the 30% cap of 180.
    assert_eq!(ring_margin_for_cuts(1000.0, 600.0, 100.0), Some(130.0));
    assert_eq!(ring_margin_for_cuts(1000.0, 600.0, 400.0), Some(180.0));
}

#[test]
fn a_band_outside_the_ring_is_paid_for_out_of_the_margin() {
    // A band is drawn outside the coordinate scale, so the ring has to come in to make room. The
    // cap lifts from 30% to 38% of the short side with a band, and not without one: paying for it
    // always would shrink every map to serve a rare one.
    use bios_layout::overview::ring_map::ring_margin_for;
    assert_eq!(ring_margin_for(1000.0, 600.0, 0.0, 0.0), ring_margin(1000.0, 600.0));
    assert_eq!(ring_margin_for(1000.0, 600.0, 0.0, 54.0), Some(90.0 + 54.0));
    // Capped: a huge reserve cannot eat the ring, and the cap it meets is the band's own.
    assert_eq!(ring_margin_for(1000.0, 600.0, 0.0, 400.0), Some(600.0 * 0.38));
    assert_eq!(ring_margin_for(1000.0, 600.0, 400.0, 0.0), Some(600.0 * 0.3));
}

#[test]
fn the_line_centres_what_it_draws_and_keeps_its_numbers_in_a_short_frame() {
    use bios_layout::overview::line_map::TITLE_CLEAR;
    use bios_layout::overview::projection::NAV_RESERVE;
    for h in [300.0, 400.0, 800.0] {
        let map = layout_line(&scene(false), Frame::line(1000.0, h), measure);
        let (top, bottom) = map.extent();
        let floor = h - NAV_RESERVE;
        assert!((top - (floor - bottom)).abs() < 1e-6, "{h}: {top} above, {} below", floor - bottom);
    }
    // Too short to centre clear of the title: pushed down under it, numbers still in the frame.
    let map = layout_line(&scene(false), Frame::line(1000.0, 120.0), measure);
    let (top, bottom) = map.extent();
    assert!(bottom <= 120.0 - NAV_RESERVE + 1e-6 && (top >= TITLE_CLEAR - 1e-6 || bottom >= 120.0 - NAV_RESERVE - 1e-6), "{top}..{bottom}");
    // Shorter than the drawing: the axis and numbers stay; the names above give way.
    let map = layout_line(&scene(false), Frame::line(1000.0, 40.0), measure);
    assert!((map.extent().1 - (40.0 - NAV_RESERVE)).abs() < 1e-6);
}
