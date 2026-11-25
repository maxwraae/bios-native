//! The line, against what the Bios web app drew.
//!
//! `renderLine` and `layoutLineAnnotations` have no tests of their own in the web app: they are
//! drawing code. So the recorded answer here is the drawing itself: for three constructs, the
//! axis, every tick, bar, direction mark, name inside a bar and flag the web app's map put in a
//! 1440 x 378 frame, read out of its SVG (see `support`). The layout is handed the web app's own
//! text widths and has to arrive at the same picture.

mod support;

use bios_layout::overview::line_map::{LineMap, NUMBER_DROP, layout_line};
use bios_layout::overview::scene::{Frame, MapScene};
use support::{Case, all_close, number, path_numbers};

fn line_of(case: &Case) -> LineMap {
    let frame = case.drawn("line", "frame");
    let mut map = layout_line(&case.scene, Frame::line(number(&frame[0][0]), number(&frame[0][1])), case.measure());
    // Native centres the whole drawing where the web placed it by its lanes (PARITY.md, Max
    // 2026-11-25). That moves the line up or down as one piece, so it is put back at the web's
    // height and everything else is held to the recording unchanged.
    let web_axis = number(&case.drawn("line", "axis")[0][2]);
    map.shift(web_axis - map.axis_y);
    map
}

fn numbers(row: &[String]) -> Vec<f64> {
    row.iter().map(|v| number(v)).collect()
}

/// Everything about the line that does not depend on a name: the axis, the scale, the bars.
fn assert_line_shapes(case: &Case, map: &LineMap) {
    let axis = &case.drawn("line", "axis")[0];
    assert!(all_close(&[map.left, map.axis_y, map.right, map.axis_y], &numbers(&axis[1..])), "axis {axis:?}, got {map:?}");

    // tick: x1 y1 x2 y2 major|minor
    let ticks = case.drawn("line", "tick");
    assert_eq!(map.ticks.len(), ticks.len(), "ticks");
    for (tick, want) in map.ticks.iter().zip(&ticks) {
        let reach = if tick.major { 6.0 } else { 3.0 };
        assert!(all_close(&[tick.x, map.axis_y, tick.x, map.axis_y + reach], &numbers(&want[..4])), "tick {want:?}, got {tick:?}");
        assert_eq!(tick.major, want[4] == "major");
    }

    // number: x y middle text, y being the baseline
    let drawn = case.drawn("line", "number");
    let got: Vec<_> = map.ticks.iter().filter_map(|t| t.number.as_ref().map(|n| (t.x, n))).collect();
    assert_eq!(got.len(), drawn.len(), "numbers");
    for ((x, text), want) in got.iter().zip(&drawn) {
        assert!(all_close(&[*x, map.axis_y + NUMBER_DROP], &numbers(&want[..2])), "number {want:?}");
        assert_eq!(text.as_str(), want[3]);
    }

    // bar: feature x y w h wide|mark
    let bars = case.drawn("line", "bar");
    let got: Vec<_> = map.bars.iter().filter(|b| !b.connector).collect();
    assert_eq!(got.len(), bars.len(), "bars");
    for (bar, want) in got.iter().zip(&bars) {
        let rect = [bar.x0, bar.y - map.thickness / 2.0, bar.x1 - bar.x0, map.thickness];
        assert!(all_close(&rect, &numbers(&want[1..5])), "bar {want:?}, got {bar:?}");
        assert_eq!((bar.feature.to_string(), bar.mark), (want[0].clone(), want[5] == "mark"));
    }

    // head: feature d, where d is one corner of the base, the tip, the other corner
    let heads = case.drawn("line", "head");
    let got: Vec<_> = map.bars.iter().filter_map(|b| b.head.map(|h| (b.feature, h))).collect();
    assert_eq!(got.len(), heads.len(), "heads");
    for ((feature, head), want) in got.iter().zip(&heads) {
        let points: Vec<f64> = head.iter().flat_map(|p| [p.x, p.y]).collect();
        assert_eq!(feature.to_string(), want[0]);
        assert!(all_close(&points, &path_numbers(&want[1])), "head {want:?}, got {head:?}");
    }
}

/// The names: which are written inside their own bar, and where the flags of the rest sit.
fn assert_line_names(case: &Case, map: &LineMap) {
    // name: feature x y text, centred on the bar
    let names = case.drawn("line", "name");
    let got: Vec<_> = map.bars.iter().filter_map(|b| b.name.as_ref().map(|n| (b, n))).collect();
    assert_eq!(got.len(), names.len(), "names in bars");
    for ((bar, text), want) in got.iter().zip(&names) {
        assert!(all_close(&[(bar.x0 + bar.x1) / 2.0, bar.y], &numbers(&want[1..3])), "name {want:?}, got {bar:?}");
        assert_eq!((bar.feature.to_string(), text.as_str(), map.lane_font), (want[0].clone(), want[3].as_str(), 11.0));
    }

    // flag: x y w h x1 y1 x2 y2 text, the box and then the leader
    let flags = case.drawn("line", "flag");
    assert_eq!(map.flags.len(), flags.len(), "flags: {:?}", map.flags);
    for (flag, want) in map.flags.iter().zip(&flags) {
        let (from, to) = flag.leader;
        let got = [flag.x, flag.y - flag.h / 2.0, flag.w, flag.h, from.x, from.y, to.x, to.y];
        assert!(all_close(&got, &numbers(&want[..8])), "flag {want:?}, got {flag:?}");
        assert_eq!((flag.text.as_str(), flag.font), (want[8].as_str(), 11.0));
    }
}

/// A feature through the origin is two bars on a line, named once, on the wider of the two.
#[test]
fn a_small_plasmid_draws_as_the_web_app_draws_it() {
    let case = Case::load("wrap_demo");
    let map = line_of(&case);
    assert_line_shapes(&case, &map);
    assert_line_names(&case, &map);
    let ampr: Vec<_> = map.bars.iter().filter(|b| b.feature == 0).map(|b| b.name.as_deref()).collect();
    assert_eq!(ampr, [None, Some("AmpR")]);
}

/// The cases recorded on private data (see `support`).
mod private {
    use super::*;

    /// The hard case: thirty shown features on six kilobases, most of them inside one 500-base
    /// stretch. Small features are marks, nested ones take lanes, names that do not fit their bar
    /// are shortened or go up to rows of flags that never overlap.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn thirty_features_on_a_line_draw_as_the_web_app_draws_them() {
        let case = Case::load_private("lab_construct");
        assert_eq!(case.scene.features.len(), 30, "the source and the scan hits are not drawn");
        let map = line_of(&case);
        assert_line_shapes(&case, &map);
        assert_line_names(&case, &map);
        let shortened = map.bars.iter().filter_map(|b| b.name.as_deref()).filter(|n| n.ends_with("..")).count();
        assert_eq!(shortened, 3, "three names in bars are shortened");
    }

    /// A real vector with long backbone notes, as a line.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn a_crowded_plasmid_draws_as_the_web_app_draws_it() {
        let case = Case::load_private("crowded_ring");
        let map = line_of(&case);
        assert_line_shapes(&case, &map);
        assert_line_names(&case, &map);
    }
}

/// A construct with no features is its axis and its scale.
#[test]
fn a_bare_sequence_is_an_axis_and_a_scale() {
    let scene = MapScene::new(&[], 700, false);
    let map = layout_line(&scene, Frame::line(600.0, 300.0), |_, _| 0.0);
    assert!(map.bars.is_empty() && map.flags.is_empty());
    let labelled: Vec<_> = map.ticks.iter().filter_map(|t| t.number.as_deref()).collect();
    assert_eq!(labelled, ["1", "100", "200", "300", "400", "500", "600", "1"]);
    assert_eq!(map.ticks.len(), 36);
}

/// An empty construct has an axis and nothing on it.
#[test]
fn an_empty_construct_is_an_empty_line() {
    let case = Case::load("wrap_demo");
    let mut scene = case.scene.clone();
    scene.length = 0;
    let map = layout_line(&scene, Frame::line(600.0, 300.0), |_, _| 0.0);
    assert!(map.ticks.is_empty() && map.bars.is_empty() && map.flags.is_empty());
    assert_eq!((map.left, map.right), (44.0, 556.0));
}
