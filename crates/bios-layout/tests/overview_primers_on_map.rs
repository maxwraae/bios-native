//! Primers on the map, ring and line: a track of their own off the backbone, each oligo a slim
//! arrow pointing the way it reads, overlapping ones stacked into lanes, and the name beside the 5'
//! end where there is room for it.
//!
//! A DEPARTURE from the Bios web app, which draws no oligos on its map (PARITY.md, Max 2026-11-25),
//! so there is no recorded drawing to compare against: these check the rules, as the cut-site tests
//! do. The rules themselves are the sequence rows' (`rows.rs`, `row_features.rs`), restated for the
//! map.

use bios_layout::overview::display::{FileFeature, display_features, display_primers};
use bios_layout::overview::line_map::{LineMap, layout_line};
use bios_layout::overview::projection::Point;
use bios_layout::overview::ring_map::{RingMap, layout_ring};
use bios_layout::overview::scene::{Frame, MapScene};

const L: i64 = 1000;

/// Every glyph 6px wide, as the cut tests measure.
fn measure(text: &str, _font: f64) -> f64 {
    text.chars().count() as f64 * 6.0
}

fn primer(name: &str, start: i64, end: i64, forward: bool) -> FileFeature<'_> {
    FileFeature { name, kind: "primer_bind", start, end, forward, locations: None, location_unsupported: false }
}

fn feature(name: &str, start: i64, end: i64) -> FileFeature<'_> {
    FileFeature { name, kind: "CDS", start, end, forward: true, locations: None, location_unsupported: false }
}

/// The scene the map is handed, with the Primers layer ON: the file's features in the feature lanes
/// and its primers in the primer track.
fn scene(file: &[FileFeature<'_>], circular: bool) -> MapScene {
    MapScene::new(&display_features(file, false), L, circular).with_primers(&display_primers(file))
}

/// The same file with the Primers layer OFF.
fn without_primers(file: &[FileFeature<'_>], circular: bool) -> MapScene {
    MapScene::new(&display_features(file, false), L, circular)
}

fn line(scene: &MapScene) -> LineMap {
    layout_line(scene, Frame::line(1000.0, 400.0), measure)
}

fn ring(scene: &MapScene) -> RingMap {
    layout_ring(scene, Frame::ring(1000.0, 600.0), measure)
}

/// The angle of a point on the ring, from twelve o'clock, growing the way the bases run.
fn angle(p: Point, map: &RingMap) -> f64 {
    (p.x - map.geom.cx).atan2(map.geom.cyc - p.y)
}

fn radius(p: Point, map: &RingMap) -> f64 {
    (p.x - map.geom.cx).hypot(p.y - map.geom.cyc)
}

#[test]
fn a_primer_is_drawn_in_the_primer_track_and_never_in_the_feature_lanes() {
    let file = vec![feature("pol", 100, 399), primer("fwd", 500, 519, true)];
    let scene = scene(&file, true);
    assert_eq!(scene.features.len(), 1, "a primer_bind is not a feature of the map");
    assert_eq!(scene.primers.len(), 1);
    assert_eq!(scene.primers[0].name, "fwd");
    let map = ring(&scene);
    assert_eq!(map.arcs.len(), 1);
    assert_eq!(map.primers.len(), 1);
    let p = &map.primers[0];
    // Between the backbone and the features, touching neither: a thin ring hugging the axis, with
    // the feature lanes nesting below it.
    assert!(p.radius + map.primer_thickness / 2.0 < map.axis_r, "the primer track is inside the backbone");
    assert!(
        p.radius - map.primer_thickness / 2.0 > map.arcs[0].radius + map.thickness / 2.0,
        "clear of the feature lane: primer at {} ({} thick), feature at {} ({} thick)",
        p.radius,
        map.primer_thickness,
        map.arcs[0].radius,
        map.thickness
    );
    assert!(map.primer_thickness < map.thickness, "a primer's arrow is slimmer than a feature's band");

    let map = line(&scene);
    let p = &map.primers[0];
    let bar = &map.bars[0];
    assert!(p.y + map.primer_thickness / 2.0 < map.axis_y, "above the axis");
    assert!(p.y - map.primer_thickness / 2.0 > bar.y + map.thickness / 2.0, "below the feature lanes: {} against {}", p.y, bar.y);
}

#[test]
fn each_arrow_points_the_way_its_primer_reads() {
    let file = vec![primer("fwd", 100, 119, true), primer("rev", 500, 519, false)];
    let map = line(&scene(&file, false));
    assert_eq!(map.primers.len(), 2);
    let (fwd, rev) = (&map.primers[0], &map.primers[1]);
    let head = fwd.head.expect("the piece with the 3' end carries the head");
    // Base corner, tip, base corner: the tip on the lane's centre line, at the 3' end, which for a
    // forward oligo is its right-hand end, past the shaft.
    assert!((head[1].y - fwd.y).abs() < 1e-9 && (head[0].y - (fwd.y - map.primer_thickness / 2.0)).abs() < 1e-9);
    assert!((head[2].y - (fwd.y + map.primer_thickness / 2.0)).abs() < 1e-9);
    assert!(head[1].x > head[0].x && head[1].x > fwd.x1, "a forward primer's head is at its right end: {head:?} with shaft to {}", fwd.x1);
    let head = rev.head.expect("a reverse primer has a head too");
    assert!(head[1].x < head[0].x && head[1].x < rev.x0, "a reverse primer reads leftward, so its head is at its left end: {head:?}");

    // The ring says the same thing in angles: the tip is always downstream of the base.
    let map = ring(&scene(&file, true));
    let (fwd, rev) = (&map.primers[0], &map.primers[1]);
    let head = fwd.head.expect("a head on the ring too");
    assert!(angle(head[1], &map) > angle(head[0], &map), "forward: the tip is further round than the base");
    assert!((radius(head[1], &map) - fwd.radius).abs() < 1e-6, "the tip sits on the lane's centre line");
    assert!((radius(head[0], &map) - (fwd.radius - map.primer_thickness / 2.0)).abs() < 1e-6);
    let head = rev.head.expect("a head on the ring too");
    assert!(angle(head[1], &map) < angle(head[0], &map), "reverse: the tip is back the way the bases came");
}

#[test]
fn an_oligo_and_a_feature_over_the_same_bases_line_up() {
    let file = vec![feature("cds", 100, 119), primer("fwd", 100, 119, true)];
    let map = line(&scene(&file, false));
    let (bar, p) = (&map.bars[0], &map.primers[0]);
    let head = p.head.expect("a head");
    assert!((p.x0 - bar.x0).abs() < 1e-9, "same 5' edge: {} against {}", p.x0, bar.x0);
    assert!((head[1].x - bar.x1).abs() < 1e-9, "the head's tip is the 3' edge, not past it: {} against {}", head[1].x, bar.x1);
}

#[test]
fn overlapping_primers_stack_into_lanes_instead_of_drawing_over_each_other() {
    // a and b overlap, c sits clear of both and so shares a's lane.
    let file = vec![primer("a", 100, 139, true), primer("b", 120, 159, false), primer("c", 400, 439, true)];
    let scene = scene(&file, false);
    assert_eq!(scene.primer_lanes, vec![0, 1, 0]);
    let map = line(&scene);
    let (a, b, c) = (&map.primers[0], &map.primers[1], &map.primers[2]);
    assert!((a.y - c.y).abs() < 1e-9, "two primers that do not overlap share a lane");
    assert!(b.y < a.y, "the one that collided stacked away from the axis: {} against {}", b.y, a.y);
    assert!(a.y - b.y >= map.primer_thickness, "a lane apart, so the two arrows cannot touch");

    let map = ring(&scene);
    let (a, b, c) = (&map.primers[0], &map.primers[1], &map.primers[2]);
    assert!((a.radius - c.radius).abs() < 1e-9);
    assert!(b.radius < a.radius && a.radius - b.radius >= map.primer_thickness, "nested inward by a lane");
}

#[test]
fn a_primer_through_the_origin_is_two_arrows_with_one_head() {
    let file = vec![primer("wrap", 980, 19, true)];
    let map = line(&scene(&file, true));
    assert_eq!(map.primers.len(), 2, "one piece each side of the origin");
    assert!((map.primers[0].y - map.primers[1].y).abs() < 1e-9, "both in the same lane");
    assert!(map.primers[0].head.is_none(), "the piece that does not hold the 3' end is a plain bar");
    assert!(map.primers[1].head.is_some(), "the head is where the oligo ends reading");

    let file = vec![primer("wrap", 980, 19, false)];
    let map = line(&scene(&file, true));
    assert!(map.primers[0].head.is_some() && map.primers[1].head.is_none(), "a reverse oligo ends reading at the first piece");
}

#[test]
fn a_primers_name_is_set_beside_its_five_prime_end_where_there_is_room() {
    let file = vec![primer("FWD1", 400, 419, true), primer("REV1", 600, 619, false)];
    let map = line(&scene(&file, false));
    let (fwd, rev) = (&map.primers[0], &map.primers[1]);
    let name = fwd.name.as_ref().expect("a lone primer has room for its name");
    assert_eq!(name.text, "FWD1");
    assert!(name.to_left && name.x < fwd.x0, "a forward primer's name sits off its 5' (left) end: {} against {}", name.x, fwd.x0);
    assert!((name.y - fwd.y).abs() < 1e-9, "level with its own lane");
    let name = rev.name.as_ref().expect("and a reverse one");
    let head = rev.head.expect("a head");
    assert!(!name.to_left && name.x > rev.x1, "a reverse primer reads leftward, so its 5' end is its right one");
    assert!(name.x > head[1].x, "and the name is away from the head, not over it");

    // A neighbour in the same lane takes the room away: the name is dropped, never written over it.
    let file = vec![primer("FWD1", 400, 419, true), primer("blocker", 360, 379, true)];
    let map = line(&scene(&file, false));
    assert_eq!(map.primers[0].name, None, "no room upstream: the oligo beside it is there");
    assert!(map.primers[1].name.is_some(), "the one with room keeps its name");

    // On the ring the same name runs along the primer's own lane.
    let file = vec![primer("FWD1", 400, 419, true)];
    let map = ring(&scene(&file, true));
    assert_eq!(map.primer_labels.len(), 1);
    let label = &map.primer_labels[0];
    assert_eq!((label.feature, label.text.as_str()), (0, "FWD1"));
    assert!((label.radius - map.primers[0].radius).abs() < 2.0, "written on its own lane");
    assert!(label.frac < map.primers[0].start_frac, "upstream of a forward oligo's 5' end");
    assert!(label.font < map.thickness, "smaller type than a feature's band is thick");
}

#[test]
fn with_the_primers_layer_off_the_map_draws_none_and_gives_up_no_room() {
    let file = vec![feature("pol", 100, 399), primer("fwd", 500, 519, true), primer("rev", 505, 524, false)];
    let (on, off) = (scene(&file, true), without_primers(&file, true));
    assert!(off.primers.is_empty() && off.primer_lanes.is_empty());

    let (ring_on, ring_off) = (ring(&on), ring(&off));
    assert!(ring_off.primers.is_empty() && ring_off.primer_labels.is_empty());
    assert!(ring_on.arcs[0].radius < ring_off.arcs[0].radius, "with primers on, the features nest in to make room for them");
    assert_eq!(ring_off.arcs[0].radius, ring_off.axis_r - (12.0 + ring_off.thickness / 2.0), "with them off, lane 0 sits where it always did");

    let (line_on, line_off) = (line(&on), line(&off));
    assert!(line_off.primers.is_empty());
    let drop = |map: &LineMap| map.axis_y - map.bars[0].y;
    assert!(drop(&line_on) > drop(&line_off), "the lanes stand further off the axis with the track there");
    assert_eq!(drop(&line_off), 12.0 + line_off.thickness / 2.0);
}
