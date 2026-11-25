//! A picture of a construct, drawn the way the window draws it.
//!
//! `draw_plasmid` is an engine command like every other, but drawing is not the engine's work:
//! placing a name needs the interface's own fonts, and only this crate may use the toolkit. So the
//! engine is handed this at start ([`set_up`], as `bios_update::set_up` hands it its updater) and
//! asks it for a picture when an agent wants one.
//!
//! Nothing is drawn twice: a frame of the real map, or of the real rows, is run without a window —
//! the toolkit lays text out with Bios's own fonts and needs no screen for it — and the shapes
//! that frame produced are written out as SVG ([`crate::svg`]). A change to the map or the rows
//! shows up in the picture by itself.

use std::sync::Arc;

use bios_core::Construct;
use bios_engine::picture::{Ask, Picture, View};
use bios_layout::overview::line_map::TITLE_CLEAR;
use bios_layout::overview::projection::{MORPH_CIRC, NAV_RESERVE, straightness};
use bios_layout::overview::viewport::{MIN_WIDTH, Projection, Viewport, clamp, visible_range, wrap1};
use egui::{Pos2, Rect, Vec2};

use crate::map::{self, MapView};
use crate::{plasmid_title, rows, svg, theme};

/// Hands the engine Bios's own drawing, so `draw_plasmid` can make a picture. Called once, at
/// start, by every program that runs the engine. A second call changes nothing and returns
/// `false`.
pub fn set_up() -> bool {
    bios_engine::picture::set_painter(Arc::new(Pictures))
}

/// Bios's drawing, as the engine's painter.
pub struct Pictures;

impl bios_engine::picture::Painter for Pictures {
    fn draw(&self, ask: &Ask<'_>) -> Result<Picture, String> {
        if ask.construct.is_empty() {
            return Err("a construct with no bases has nothing to draw".into());
        }
        Ok(draw(ask))
    }
}

/// The picture of `ask`, as a frame of the interface that no one ever sees.
///
/// The frame is run twice. The first lays every piece of text out for the first time and settles
/// what the interface keeps between frames; it also says how tall the rows of a sequence view
/// came out, so the second can be drawn at that height instead of leaving half the picture empty.
pub fn draw(ask: &Ask<'_>) -> Picture {
    let width = even_floor(ask.width);
    let height = even_floor(ask.height);
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width as f32, height as f32));
    let mut drawn = (ask.start, ask.end);
    let mut shapes = Vec::new();
    for pass in 0..2 {
        let mut used = height;
        let input = egui::RawInput { screen_rect: Some(rect), ..egui::RawInput::default() };
        let output = ctx.run_ui(input, |ui| (drawn, used) = paint(ui, rect, ask));
        shapes = output.shapes.iter().map(|clipped| clipped.shape.clone()).collect();
        // Nothing is painted on a screen here, so what the frame would have uploaded is let go of.
        output.drop_without_applying_deltas();
        if pass == 0 {
            // Up to the next even pixel, so the last row is not lost to the rounding, and never
            // past the height asked for.
            rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width as f32, even_up(used).min(height) as f32));
        }
    }
    let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
    Picture { svg: svg::document(&shapes, w, h, ask.square), drawn, size: (w, h) }
}

/// One frame of the view asked for: the bases it turned out to hold, and the height it needed.
fn paint(ui: &mut egui::Ui, rect: Rect, ask: &Ask<'_>) -> ((i64, i64), f64) {
    let c = ask.construct;
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme::CELL);
    if let View::Sequence = ask.view {
        let drawn = rows::draw_rows(&painter, rect, c, ask.start, ask.end);
        return ((drawn.from, drawn.to), drawn.height);
    }
    let ring = ask.view == View::Circular;
    let view = MapView { vp: window_on(c, ask.start, ask.end, ring), ring };
    // The cuts the editor shows at rest: the workhorse enzymes that cut this construct once.
    let drawn = map::draw_with(&painter, rect, c, view, &rows::default_cut_sites(c), None);
    plasmid_title::show(ui, rect, c);
    let l = length(c);
    let seen = visible_range(view.vp, l);
    // A whole ring's window starts half a turn before twelve o'clock and comes back round to it;
    // saying so as a range reads as nonsense when the answer is simply "all of it".
    let range = if seen.span >= l { (0, l - 1) } else { (seen.start_bp, seen.end_bp) };
    // A line map of a short stretch is a band a hundred pixels tall. Drawing it in the middle of
    // a screenful of paper is a picture of mostly nothing, so the frame comes down to what it
    // drew, with the room the title and the scale need either side of it.
    let used = match drawn.tall {
        Some(tall) => tall + TITLE_CLEAR + NAV_RESERVE,
        None => f64::from(rect.height()),
    };
    (range, used)
}

// A picture is written at a whole, EVEN number of pixels, so that padding it out to a square and
// cropping it back lands on whole pixels either side.

fn even_floor(v: f64) -> f64 {
    let whole = if v.is_finite() { v.floor().max(2.0) } else { 2.0 };
    whole - whole % 2.0
}

fn even_up(v: f64) -> f64 {
    let whole = if v.is_finite() { v.ceil().max(2.0) } else { 2.0 };
    whole + whole % 2.0
}

fn length(c: &Construct) -> i64 {
    i64::try_from(c.len()).unwrap_or(i64::MAX)
}

/// The map's window onto bases `start`..`end`, 0-based and inclusive, an `end` before the `start`
/// running through the origin.
///
/// The whole molecule is the view the editor opens on: the origin at twelve o'clock on a ring, the
/// whole line on a line. A stretch of it is the same window zoomed in, which is why a ring asked
/// for a short stretch comes back unrolled — that is what the window does when you zoom it.
///
/// The window has to say the same thing the layout will draw, or the answer would name bases the
/// picture does not hold. A line is unrolled AT the origin, so its window is held inside the two
/// ends; only a ring can have one that crosses them, and a stretch that does keeps enough width
/// to stay bent.
fn window_on(c: &Construct, start: i64, end: i64, ring: bool) -> Viewport {
    let l = length(c);
    let whole = |projection| Viewport { center: if ring { 0.0 } else { 0.5 }, width: 1.0, projection };
    if l <= 0 {
        return whole(if ring { Projection::Ring } else { Projection::Line });
    }
    let bases = if end >= start { end - start + 1 } else { l - start + end + 1 };
    if bases >= l || bases <= 0 {
        return whole(if ring { Projection::Ring } else { Projection::Line });
    }
    let asked = clamp(bases as f64 / l as f64, MIN_WIDTH, 1.0);
    let width = if end < start && ring { asked.max(MORPH_CIRC) } else { asked };
    // Which way the map will be drawn at that width: bent, so the window may cross the origin, or
    // unrolled, where it may not.
    let bent = ring && straightness(width) < 1.0;
    // The middle of the stretch asked for, which stays its middle however far the window had to
    // open to hold it.
    let middle = wrap1((start as f64 + bases as f64 / 2.0) / l as f64);
    let center = if bent { middle } else { clamp(middle, width / 2.0, 1.0 - width / 2.0) };
    Viewport { center, width, projection: if bent { Projection::Ring } else { Projection::Line } }
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;
    use bios_layout::overview::viewport::{Projection, visible_range};

    use super::window_on;

    fn plasmid(length: usize, circular: bool) -> Construct {
        Construct { name: "t".into(), circular, sequence: "ACGT".repeat(length / 4), ..Construct::default() }
    }

    #[test]
    fn the_whole_molecule_opens_where_the_editor_opens_it() {
        let c = plasmid(1200, true);
        let ring = window_on(&c, 0, 1199, true);
        assert_eq!((ring.center, ring.width, ring.projection), (0.0, 1.0, Projection::Ring));
        let line = window_on(&c, 0, 1199, false);
        assert_eq!((line.center, line.width, line.projection), (0.5, 1.0, Projection::Line));
    }

    #[test]
    fn a_stretch_is_the_window_on_that_stretch() {
        let c = plasmid(1200, true);
        let seen = visible_range(window_on(&c, 300, 599, false), 1200);
        assert_eq!((seen.start_bp, seen.end_bp), (300, 599));
    }

    #[test]
    fn a_stretch_through_the_origin_keeps_the_ring_bent_and_stays_on_screen() {
        let c = plasmid(1200, true);
        // 200 bases of 1,200 would unroll the ring, and an unrolled map cannot show a window
        // across the origin; it opens instead, round the stretch asked for.
        let window = window_on(&c, 1100, 99, true);
        assert_eq!(window.projection, Projection::Ring);
        let seen = visible_range(window, 1200);
        assert!(seen.wraps, "{seen:?}");
        assert!(seen.start_bp <= 1100 && seen.end_bp >= 99, "{seen:?}");
    }

    #[test]
    fn a_window_that_will_be_drawn_as_a_line_is_held_inside_the_two_ends() {
        let c = plasmid(1200, true);
        // The first ten bases: zoomed in this far the ring unrolls, so the window may not run off
        // the left end and back round to the right one.
        let window = window_on(&c, 0, 9, true);
        assert_eq!(window.projection, Projection::Line);
        let seen = visible_range(window, 1200);
        assert_eq!((seen.start_bp, seen.wraps), (0, false), "{seen:?}");
    }

    #[test]
    fn a_stretch_shorter_than_the_map_can_show_opens_to_the_floor_around_its_middle() {
        let c = plasmid(48000, false);
        // The overview's own zoom floor is a hundredth of the molecule; the answer says so by
        // giving back the bases actually in frame.
        let seen = visible_range(window_on(&c, 1000, 1009, false), 48000);
        assert_eq!(seen.span, 480);
        assert!(seen.start_bp < 1000 && seen.end_bp > 1009, "{seen:?}");
    }
}
