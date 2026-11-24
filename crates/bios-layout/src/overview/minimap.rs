//! The navigator: the whole construct as a flat rail with the window you are looking at boxed on
//! it. Ported from Bios `editor/overview/MapMinimap.tsx`, its numbers from `styles.css`
//! (`.navigator-*`, lines 5309-5600). Positions here are fractions of the rail's width (0..1) and
//! pixels down from the top of the track; the painter scales them.

use super::scene::group_thousands;
use super::viewport::{Projection, Viewport, WindowPiece, clamp, nice_ticks, split_span, visible_range, window_pieces};

/// The feature rail (`BAND`), 4px down (`BAND_TOP`) so the box can stand proud of it.
pub const BAND: f64 = 9.0;
pub const BAND_TOP: f64 = 4.0;
/// The window box: 4px above and below the band.
pub const FRAME_H: f64 = BAND + 8.0;
/// `.navigator-track`'s height.
pub const TRACK_H: f64 = 42.0;
/// `.navigator-seam`'s padding, and the editor seam's side inset (the map's own line margin).
pub const PAD_Y: f64 = 12.0;
pub const PAD_X: f64 = 44.0;
/// A cut tick and the scrim: 2px down, 13 tall.
pub const CUT_TOP: f64 = 2.0;
pub const CUT_H: f64 = 13.0;
/// A primer tick: half the band, so a forward one on the top edge and a reverse one on the bottom
/// read apart without the band growing, and never narrower than this, a 20-mer being three
/// thousandths of a 6 kb rail.
pub const PRIMER_TICK_H: f64 = BAND / 2.0;
pub const PRIMER_TICK_W: f64 = 3.0;
/// Scale marks under the box.
pub const MARK_TOP: f64 = 20.0;
pub const MARK_MINOR_H: f64 = 2.5;
pub const MARK_MAJOR_H: f64 = 5.0;
/// The extents and the travelling label.
pub const TEXT_TOP: f64 = 28.0;
/// The window's grips: 16 wide, centred on its ends.
pub const GRIP_W: f64 = 16.0;
pub const WINDOW_MIN_W: f64 = 16.0;
/// Past this width the window is the whole rail, so the box has nothing left to say.
pub const FULL: f64 = 0.985;

/// One feature block on the rail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    /// Index into the features handed in.
    pub feature: usize,
    pub left: f64,
    pub width: f64,
}

/// A feature to draw: its span, 0-based inclusive (an end before the start wraps), and its rank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavFeature {
    pub start: i64,
    pub end: i64,
    pub rank: f64,
}

/// Feature blocks along the rail, origin-wrap aware, in rank order so the useful one is painted
/// last and survives the overlap.
pub fn segments(features: &[NavFeature], l: i64) -> Vec<Seg> {
    let mut segs: Vec<(f64, Seg)> = features
        .iter()
        .enumerate()
        .flat_map(|(i, f)| {
            split_span(f.start, f.end, l).into_iter().map(move |p| (f.rank, Seg { feature: i, left: p.start_frac, width: p.width_frac }))
        })
        .collect();
    // Stable, as the web's `sort` is: equal ranks keep their order.
    segs.sort_by(|a, b| a.0.total_cmp(&b.0));
    segs.into_iter().map(|(_, s)| s).collect()
}

/// A primer to mark on the rail: its footprint, 0-based inclusive (an end before the start wraps),
/// and which way it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavPrimer {
    pub start: i64,
    pub end: i64,
    pub forward: bool,
}

/// One primer's tick on the rail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// Index into the primers handed in.
    pub primer: usize,
    pub left: f64,
    pub width: f64,
    pub forward: bool,
}

/// Primer ticks along the rail, origin-wrap aware and left to right. A primer is an oligo, not a
/// stretch of the construct: drawn to scale it is a hair too thin to find, so a tick narrower than
/// `min_width` (the painter's [`PRIMER_TICK_W`] as a fraction of the track) grows about its middle
/// to that, and never off either end of the rail. A primer over the origin is two ticks, one at
/// each end, which is how the wrap reads.
pub fn primer_ticks(primers: &[NavPrimer], l: i64, min_width: f64) -> Vec<Tick> {
    let min = min_width.clamp(0.0, 1.0);
    let mut ticks: Vec<Tick> = primers
        .iter()
        .enumerate()
        .flat_map(|(i, p)| {
            split_span(p.start, p.end, l).into_iter().map(move |s| {
                let width = s.width_frac.max(min);
                Tick { primer: i, left: clamp(s.start_frac + s.width_frac / 2.0 - width / 2.0, 0.0, 1.0 - width), width, forward: p.forward }
            })
        })
        .collect();
    ticks.sort_by(|a, b| a.left.total_cmp(&b.left).then(a.primer.cmp(&b.primer)));
    ticks
}

/// A scale mark: where, as a fraction, and whether it is a major one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: f64,
    pub major: bool,
}

/// The major run [`nice_ticks`] draws on the map's axis, halved for minors.
pub fn marks(l: i64) -> Vec<Mark> {
    let majors = nice_ticks(l);
    let step = majors.first().copied().unwrap_or(0.0);
    let mut out = Vec::new();
    if step <= 0.0 || l <= 0 {
        return out;
    }
    let lf = l as f64;
    let mut x = step / 2.0;
    while x < lf {
        out.push(Mark { at: x / lf, major: ((x / step) % 1.0).abs() < 1e-9 });
        x += step / 2.0;
    }
    out
}

/// A cut's place on the rail.
pub fn cut_at(position: i64, l: i64) -> f64 {
    if l <= 0 { 0.0 } else { position.rem_euclid(l) as f64 / l as f64 }
}

/// What the travelling label says: the first and last base in view, and how much that is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readout {
    /// "1 – 2,437"
    pub range: String,
    /// " · 2.4 kb"
    pub span: String,
}

pub fn readout(vp: Viewport, l: i64) -> Readout {
    let r = visible_range(vp, l);
    let span = if r.span >= 1000 { format!(" · {:.1} kb", r.span as f64 / 1000.0) } else { format!(" · {} bp", r.span) };
    Readout { range: format!("{} – {}", group_thousands((r.start_bp + 1) as f64), group_thousands((r.end_bp + 1) as f64)), span }
}

/// The far extent: "6,093 bp".
pub fn end_extent(l: i64) -> String {
    format!("{} bp", group_thousands(l as f64))
}

/// Where the label's centre sits on a track `track_w` wide: centred on the window, but never off
/// the rail or under the two extents.
pub fn label_x(vp: Viewport, track_w: f64, label_w: f64) -> f64 {
    let left = vp.center - vp.width / 2.0;
    clamp((left + vp.width / 2.0) * track_w, label_w / 2.0 + 16.0, track_w - label_w / 2.0 - 58.0)
}

/// Everything the window draws: nothing at full zoom-out, else one piece, or two when a ring's
/// window straddles the origin. The lift and the two scrims use the unwrapped `left`.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub left: f64,
    pub width: f64,
    pub pieces: Vec<WindowPiece>,
}

pub fn window(vp: Viewport) -> Option<Window> {
    if vp.width >= FULL {
        return None;
    }
    let wrap = vp.projection == Projection::Ring;
    Some(Window { left: vp.center - vp.width / 2.0, width: vp.width, pieces: window_pieces(vp.center, vp.width, wrap) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_six_kilobase_line_reads_as_the_web_shows_it_on_open() {
        let vp = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
        let r = readout(vp, 6093);
        assert_eq!(r.range, "1 – 2,437");
        assert_eq!(r.span, " · 2.4 kb");
        assert_eq!(end_extent(6093), "6,093 bp");
    }

    #[test]
    fn a_short_window_counts_bases() {
        let vp = Viewport { center: 0.5, width: 0.1, projection: Projection::Line };
        assert_eq!(readout(vp, 800).span, " · 80 bp");
    }

    #[test]
    fn the_whole_rail_has_no_window() {
        assert!(window(Viewport { center: 0.5, width: 0.99, projection: Projection::Line }).is_none());
        assert!(window(Viewport { center: 0.5, width: 0.9, projection: Projection::Line }).is_some());
    }

    #[test]
    fn a_ring_window_over_the_origin_is_two_pieces() {
        let w = window(Viewport { center: 0.0, width: 0.2, projection: Projection::Ring });
        assert_eq!(w.map(|w| w.pieces.len()), Some(2));
    }

    #[test]
    fn features_paint_in_rank_order_and_wrap() {
        let f = [NavFeature { start: 0, end: 99, rank: 5.0 }, NavFeature { start: 900, end: 99, rank: 1.0 }];
        let s = segments(&f, 1000);
        assert_eq!(s.iter().map(|s| s.feature).collect::<Vec<_>>(), vec![1, 1, 0]);
    }

    #[test]
    fn a_primer_drawn_to_scale_would_vanish_so_its_tick_has_a_floor() {
        // 20 bases of 6,000: two thousandths of the rail, grown about its middle to the floor.
        let p = [NavPrimer { start: 2990, end: 3009, forward: true }];
        let t = primer_ticks(&p, 6000, 0.01);
        assert_eq!(t.len(), 1);
        assert!((t[0].width - 0.01).abs() < 1e-12);
        assert!((t[0].left - 0.495).abs() < 1e-12);
        // A primer wide enough on its own keeps its own width.
        let wide = primer_ticks(&[NavPrimer { start: 0, end: 599, forward: true }], 6000, 0.01);
        assert_eq!(wide.first().map(|t| (t.left, t.width)), Some((0.0, 0.1)));
    }

    #[test]
    fn a_primer_at_either_end_stays_on_the_rail() {
        let ends = [NavPrimer { start: 0, end: 19, forward: true }, NavPrimer { start: 5980, end: 5999, forward: false }];
        let t = primer_ticks(&ends, 6000, 0.01);
        assert_eq!(t.len(), 2);
        assert!((t[0].left - 0.0).abs() < 1e-12);
        assert!((t[1].left - 0.99).abs() < 1e-12);
        assert!(t.iter().all(|t| t.left >= 0.0 && t.left + t.width <= 1.0 + 1e-12));
    }

    #[test]
    fn a_primer_over_the_origin_is_a_tick_at_each_end() {
        let t = primer_ticks(&[NavPrimer { start: 5990, end: 9, forward: true }], 6000, 0.01);
        assert_eq!(t.len(), 2);
        assert_eq!(t.iter().map(|t| t.primer).collect::<Vec<_>>(), vec![0, 0]);
        assert!((t[0].left - 0.0).abs() < 1e-12);
        assert!((t[1].left - 0.99).abs() < 1e-12);
    }

    #[test]
    fn ticks_read_left_to_right_whatever_order_the_primers_came_in() {
        let p = [
            NavPrimer { start: 4000, end: 4019, forward: true },
            NavPrimer { start: 100, end: 119, forward: false },
            NavPrimer { start: 2000, end: 2019, forward: true },
        ];
        let t = primer_ticks(&p, 6000, 0.01);
        assert_eq!(t.iter().map(|t| t.primer).collect::<Vec<_>>(), vec![1, 2, 0]);
        assert_eq!(t.iter().map(|t| t.forward).collect::<Vec<_>>(), vec![false, true, true]);
        assert!(t.windows(2).all(|w| w[0].left <= w[1].left));
    }

    #[test]
    fn no_primers_is_no_ticks() {
        assert!(primer_ticks(&[], 6000, 0.01).is_empty());
        assert!(primer_ticks(&[NavPrimer { start: 0, end: 19, forward: true }], 0, 0.01).is_empty());
    }

    #[test]
    fn marks_halve_the_major_step() {
        let m = marks(6093);
        // nice_ticks(6093) steps by 2000, so marks every 1000 with every other one major.
        assert_eq!(m.len(), 6);
        assert!(!m[0].major && m[1].major);
        assert!((m[1].at - 2000.0 / 6093.0).abs() < 1e-12);
    }

    #[test]
    fn the_label_keeps_clear_of_the_extents() {
        let vp = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
        assert_eq!(label_x(vp, 1000.0, 100.0), 200.0);
        let start = Viewport { center: 0.01, width: 0.02, projection: Projection::Line };
        assert_eq!(label_x(start, 1000.0, 100.0), 66.0);
        let end = Viewport { center: 0.99, width: 0.02, projection: Projection::Line };
        assert_eq!(label_x(end, 1000.0, 100.0), 892.0);
    }
}
