//! The strip: the rows' own row, drawn once and unwrapped, that the map's pane becomes in the split
//! once a base is wide enough to read. Ported from Bios `editor/rows/SequenceRows.tsx` (`zoomLine`
//! 248-260, the strip's window 409-431, its geometry 433-460, its wheel 764-780, its door back
//! 822-832 and its slide to the caret 1153-1173).
//!
//! The strip owns its window, in the map's units: a centre and a width, both fractions of the
//! construct, clamped the way a line is. Its one row starts on whatever base the window's left edge
//! falls on, and the base it starts inside is `shift` pixels to the left, so a pan slides smoothly
//! instead of stepping a base at a time.

use crate::overview::viewport::{Projection, Viewport, clamp, clamp_center, pan_by, set_center};
use crate::rows::{DEFAULT_CHAR_WIDTH, DEFAULT_MARGIN, HIDE_BASES_BELOW, MAX_CHAR_WIDTH, Metrics, Row};

/// Where the strip's row sits for one window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// `bps_per_row` is the bases the row is built with (two more than the box shows, to cover the
    /// slide), `char_width` what divides the content box into the window's span, and
    /// `content_width` the row's own drawn width.
    pub metrics: Metrics,
    pub row: Row,
    /// How far the row is slid left, in pixels: the part of its first base already past the edge.
    pub shift: f64,
    /// The last base actually in the box, 1-based: what the number at the right says.
    pub last: i64,
}

/// The box the row slides under: the strip less its two half margins.
pub fn content_width(box_w: f64) -> f64 {
    (box_w - DEFAULT_MARGIN).max(0.0)
}

/// The strip's row for the window `vp`, in a box `box_w` wide, over a construct `length` long.
/// `None` for an empty construct.
pub fn geometry(length: i64, box_w: f64, vp: Viewport) -> Option<Geometry> {
    if length <= 0 {
        return None;
    }
    let l = length as f64;
    let content = content_width(box_w);
    let visible = (vp.width * l).max(1.0);
    let cw = if content > 0.0 { content / visible } else { DEFAULT_CHAR_WIDTH };
    let start_exact = (vp.center * l - visible / 2.0).max(0.0);
    let start = (start_exact.floor() as i64).clamp(0, length - 1);
    let n = visible.ceil() as i64 + 2;
    let end = (start + n - 1).min(length - 1);
    Some(Geometry {
        metrics: Metrics { bps_per_row: n, char_width: cw, margin_width: DEFAULT_MARGIN, content_width: (end - start + 1) as f64 * cw },
        row: Row { index: 0, start, end },
        shift: (start_exact - start as f64) * cw,
        last: ((start_exact + visible).floor() as i64).clamp(1, length),
    })
}

/// The narrowest window: where a base would be wider than the rows ever draw one.
pub fn min_width(box_w: f64, length: i64) -> f64 {
    let content = content_width(box_w);
    if content > 0.0 && length > 0 { (content / MAX_CHAR_WIDTH / length as f64).min(1.0) } else { 0.01 }
}

/// Holds a window the strip was asked for to what a line allows (`applyStrip`).
pub fn apply(v: Viewport, min_width: f64) -> Viewport {
    let width = clamp(v.width, min_width, 1.0);
    Viewport { center: clamp_center(v.center, width, Projection::Line), width, projection: Projection::Line }
}

/// Zooms a line window by `factor` (<1 in, >1 out) about the base at screen fraction `s`, so the
/// base under the pointer stays under the pointer (`zoomLine`).
pub fn zoom_line(v: Viewport, factor: f64, min_width: f64, s: f64) -> Viewport {
    let width = clamp(v.width * factor, min_width, 1.0);
    let anchor = v.center - v.width / 2.0 + s * v.width;
    Viewport { width, center: clamp_center(anchor - s * width + width / 2.0, width, Projection::Line), projection: Projection::Line }
}

/// Pinch or ⌘-wheel on the strip: `dz` is the wheel's delta (positive zooms out), `x` the pointer
/// from the strip's left edge. Same torque as the map and the rows.
pub fn wheel_zoom(v: Viewport, dz: f64, x: f64, box_w: f64, length: i64) -> Viewport {
    let dz = dz.clamp(-30.0, 30.0);
    let w = content_width(box_w).max(1.0);
    let s = clamp((x - DEFAULT_MARGIN / 2.0) / w, 0.0, 1.0);
    apply(zoom_line(v, (dz * 0.02).exp(), min_width(box_w, length), s), min_width(box_w, length))
}

/// Plain scroll on the strip slides along the strand: `delta` is the wheel's dominant axis, in the
/// web's sign (positive moves on towards the end).
pub fn wheel_pan(v: Viewport, delta: f64, box_w: f64, length: i64) -> Viewport {
    let w = content_width(box_w).max(1.0);
    apply(pan_by(Viewport { projection: Projection::Line, ..v }, (delta / w) * v.width), min_width(box_w, length))
}

/// Whether the strip has been zoomed out past the letters and hands back to the map.
pub fn hands_back(char_width: f64) -> bool {
    char_width < HIDE_BASES_BELOW
}

/// Where the strip slides when something else puts the caret, or the start of the selection, at
/// `at`: nowhere while it is in view, else centred on it.
pub fn follow_caret(v: Viewport, g: &Geometry, at: usize, length: i64) -> Option<Viewport> {
    let at = i64::try_from(at).ok()?;
    if length <= 0 || (at >= g.row.start && at < g.last) {
        return None;
    }
    Some(set_center(Viewport { projection: Projection::Line, ..v }, (at as f64 + 0.5) / length as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(center: f64, width: f64) -> Viewport {
        Viewport { center, width, projection: Projection::Line }
    }

    #[test]
    fn the_row_starts_wherever_the_window_does_and_slides_by_the_part_of_a_base() {
        // 6,093 bp in a 1,180 px strip: 1,070 px of content over 114 bases.
        let length = 6093;
        let vp = line(1220.25 / 6093.0, 114.0 / 6093.0);
        let g = geometry(length, 1180.0, vp).unwrap();
        assert!((g.metrics.char_width - 1070.0 / 114.0).abs() < 1e-9);
        assert_eq!(g.row.start, 1163);
        assert_eq!(g.metrics.bps_per_row, 116, "two bases wider than the box, to cover the slide");
        assert_eq!(g.row.end, 1163 + 115);
        assert!((g.shift - 0.25 * g.metrics.char_width).abs() < 1e-6);
        assert_eq!(g.last, 1277);
        assert_eq!(g.metrics.margin_width, DEFAULT_MARGIN);
    }

    #[test]
    fn the_row_never_runs_past_either_end() {
        let g = geometry(800, 1180.0, line(0.98, 0.1)).unwrap();
        assert_eq!(g.row.end, 799);
        assert_eq!(g.last, 800);
        let g = geometry(800, 1180.0, line(0.0, 0.1)).unwrap();
        assert_eq!((g.row.start, g.shift), (0, 0.0));
        assert!(geometry(0, 1180.0, line(0.5, 0.1)).is_none());
        // A box narrower than its margins draws at the default width rather than dividing by zero.
        let g = geometry(800, 40.0, line(0.5, 0.1)).unwrap();
        assert_eq!(g.metrics.char_width, DEFAULT_CHAR_WIDTH);
    }

    #[test]
    fn the_strip_zooms_about_the_base_under_the_pointer_down_to_the_widest_base() {
        let v = line(0.5, 0.02);
        let z = zoom_line(v, 0.5, 0.0, 0.25);
        assert!((z.width - 0.01).abs() < 1e-12);
        // The base a quarter of the way across stays a quarter of the way across.
        let before = v.center - v.width / 2.0 + 0.25 * v.width;
        let after = z.center - z.width / 2.0 + 0.25 * z.width;
        assert!((before - after).abs() < 1e-12);
        // Never narrower than a base 48 px wide.
        let min = min_width(1180.0, 6093);
        assert!((min - 1070.0 / 48.0 / 6093.0).abs() < 1e-12);
        assert_eq!(wheel_zoom(line(0.5, min), -30.0, 600.0, 1180.0, 6093).width, min);
        // Zooming out by the wheel's own torque: exp(dz * 0.02), each step capped at 30.
        let out = wheel_zoom(line(0.5, 0.02), 500.0, 645.0, 1180.0, 6093);
        assert!((out.width - 0.02 * 0.6f64.exp()).abs() < 1e-12);
    }

    #[test]
    fn plain_scroll_slides_the_strip_and_stops_at_the_ends() {
        let v = line(0.5, 0.02);
        let p = wheel_pan(v, 107.0, 1180.0, 6093);
        assert!((p.center - (0.5 + 0.1 * 0.02)).abs() < 1e-12, "a tenth of the box moves a tenth of the window");
        assert_eq!(wheel_pan(v, 1e9, 1180.0, 6093).center, 1.0 - 0.01);
        assert_eq!(wheel_pan(v, -1e9, 1180.0, 6093).center, 0.01);
    }

    #[test]
    fn the_strip_hands_back_where_the_letters_stop() {
        assert!(hands_back(HIDE_BASES_BELOW - 0.01));
        assert!(!hands_back(HIDE_BASES_BELOW));
    }

    #[test]
    fn the_strip_slides_to_a_caret_put_outside_it_and_stays_for_one_inside() {
        let length = 6093;
        let v = line(1220.0 / 6093.0, 114.0 / 6093.0);
        let g = geometry(length, 1180.0, v).unwrap();
        assert_eq!(follow_caret(v, &g, 1200, length), None);
        let moved = follow_caret(v, &g, 3000, length).unwrap();
        assert!((moved.center - 3000.5 / 6093.0).abs() < 1e-12);
        assert_eq!(moved.width, v.width);
    }
}
