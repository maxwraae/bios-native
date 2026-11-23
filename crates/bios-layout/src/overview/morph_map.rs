//! THE UNROLL: a circular map between its ring and its line. Ported from `renderMorph` in Bios
//! `editor/overview/PlasmidOverview.tsx`.
//!
//! Zoom drives one continuous transform: the circle (t = 0) unrolls into the line (t = 1), with
//! `t = straightness(width)`. The polished ring and line layouts own the two ends; this fills the
//! transition so there is no snap. The arc around where you're looking is sampled as a path whose
//! points lerp from their ring positions to their straight-line positions ([`morph_point`]). Only
//! what lies within `half_span` of the centre is drawn (the whole ring at t → 0, just the window at
//! t → 1), so the FAR side of the ring never ghosts back into frame mid-unroll; its ends slide off
//! and the frame clips them.
//!
//! What the web draws mid-unroll and nothing more: the backbone, every feature's band with its mark
//! floor and seam, its arrowhead, and its name written along the band. The coordinate scale, the
//! cut sites, the selection and the flags belong to the two ends and are not drawn in between.

use super::direction::{HEAD_PX, fits_head, head_span};
use super::labels::{fit_label, label_pad, label_spacing};
use super::presence::{MIN_MARK_PX, SEAM_PX, arc_mark, arc_seam};
use super::projection::{LINE_MARGIN, NAV_RESERVE, Point, RingGeom, morph_point, ring_geom};
use super::ring_map::ring_margin_for;
use super::scene::{CUT_FONT, Frame, MapScene, lane_depth, lane_inset};
use super::viewport::{ring_delta, split_span, wrap1};

/// Where the line's axis sits for the unroll to arrive at: low in the frame, with its numbers
/// beneath it and the annotations stacked above (`lineAxisY`).
pub fn line_axis_y(h: f64) -> f64 {
    h - NAV_RESERVE - 92.0
}

/// Points along the backbone, and along each feature's band.
const N: usize = 240;

/// One piece of a feature's band, as unbroken runs of points along its lane's centre line: the pen
/// lifts where the band leaves the drawn span, so a feature straddling the edge is clipped cleanly.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphArc {
    pub feature: usize,
    pub runs: Vec<Vec<Point>>,
    /// Widened to the mark floor: drawn in the stronger tint.
    pub mark: bool,
    /// The arrowhead at its 5' end: base corner, tip, base corner.
    pub head: Option<[Point; 3]>,
}

/// A name written along its own band, the way a `textPath` anchored at its middle sets it.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphName {
    pub feature: usize,
    pub text: String,
    /// The band's visible run, ordered left to right so the letters never come out mirrored.
    pub path: Vec<Point>,
    /// How long that run is on screen.
    pub len: f64,
    pub font: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MorphMap {
    /// How far unrolled: 0 a ring, 1 a line.
    pub t: f64,
    pub geom: RingGeom,
    pub center: f64,
    /// How far either side of the centre is drawn, in fractions of the molecule.
    pub half_span: f64,
    pub backbone: Vec<Point>,
    pub arcs: Vec<MorphArc>,
    pub names: Vec<MorphName>,
    pub thickness: f64,
}

/// Lay out the unroll for `scene` in `frame` at straightness `t`. `measure(text, font_px)` is the
/// advance width of `text` in the interface font.
pub fn layout_morph(scene: &MapScene, frame: Frame, t: f64, mut measure: impl FnMut(&str, f64) -> f64) -> MorphMap {
    let longest_cut = scene.cuts.iter().fold(0.0, |m: f64, (_, name)| m.max(measure(name, CUT_FONT)));
    let geom = ring_geom(frame.width, frame.w, frame.h, ring_margin_for(frame.w, frame.h, longest_cut, frame.band_reserve));
    let center = frame.center;
    let width = frame.width;
    let sp = label_spacing(frame.density);
    let half_span = 0.5 * (1.0 - t) + (width / 2.0) * t; // whole ring at t→0, just the window at t→1
    let axis_y = line_axis_y(frame.h);
    // `mp` maps a base onto the molecule's own axis: that is the backbone at both ends of the lerp.
    // `mp_lane` steps in by the shared lane-0 inset first, so a feature sits the same distance off
    // the axis whether the map is a ring, a line, or halfway between.
    let mp = |frac: f64, off: f64| morph_point(frac, center, width, t, frame.w, frame.h, geom, LINE_MARGIN, Some(off), Some(axis_y));
    let mp_lane = |frac: f64, depth: f64| mp(frac, lane_inset(sp) + depth);
    let shown = |frac: f64| ring_delta(center, frac).abs() <= half_span;

    // How many PIXELS one unit of bp-fraction covers, measured through the actual transform. The
    // unroll compresses the drawn path, so a span sized as "6px worth of circle" would come out at
    // a third of that mid-morph. Rather than model the compression, probe it: step two points
    // either side of the window centre and measure how far apart they actually land.
    let px_per_frac = {
        let probe = 1e-3;
        let a = mp(wrap1(center - probe / 2.0), 0.0);
        let b = mp(wrap1(center + probe / 2.0), 0.0);
        (b.x - a.x).hypot(b.y - a.y) / probe
    };
    let frac_for_px = |px: f64| px / px_per_frac.max(1.0);

    let path_over = |start: f64, span: f64, steps: usize, depth: f64| {
        let mut runs: Vec<Vec<Point>> = Vec::new();
        let mut pen = false;
        for k in 0..=steps {
            let frac = start + span * (k as f64 / steps as f64);
            if !shown(frac) {
                pen = false;
                continue;
            }
            let p = mp_lane(frac, depth);
            match runs.last_mut() {
                Some(run) if pen => run.push(p),
                _ => runs.push(vec![p]),
            }
            pen = true;
        }
        runs.retain(|r| r.len() > 1);
        runs
    };

    // The visible stretch of a feature as ONE contiguous run, plus how long it is on screen: the
    // pen lifts at the clip edge, which is right for drawing and useless for setting a name.
    let path_run = |start: f64, span: f64, steps: usize, depth: f64| -> Option<(Vec<Point>, f64)> {
        let mut pts: Vec<Point> = Vec::new();
        for k in 0..=steps {
            let frac = start + span * (k as f64 / steps as f64);
            if !shown(frac) {
                if !pts.is_empty() {
                    break; // take the first unbroken run and stop
                }
                continue;
            }
            pts.push(mp_lane(frac, depth));
        }
        let (first, last) = (pts.first()?.x, pts.last()?.x);
        if pts.len() < 2 {
            return None;
        }
        // Reverse where the run reads right-to-left, so the glyphs never come out mirrored or upside down.
        if last < first {
            pts.reverse();
        }
        let len = pts.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum();
        Some((pts, len))
    };

    // Backbone: sampled directly in offset-from-centre space so it's always exactly the ±half_span
    // window, never the far side.
    let backbone = (0..=N).map(|k| mp(wrap1(center + (-half_span + (2.0 * half_span * k as f64) / N as f64)), 0.0)).collect();

    let mut arcs = Vec::new();
    let mut names = Vec::new();
    for (i, f) in scene.features.iter().enumerate() {
        let depth = lane_depth(scene.lane(i), sp);
        for pc in split_span(f.start, f.end, scene.length) {
            // The mark floor and the seam, on the morph's own measured scale, so a 3bp feature does
            // not go back to being a hairline for the width of the unroll.
            let marked = arc_mark(pc.start_frac, pc.width_frac, frac_for_px(MIN_MARK_PX));
            let seam = arc_seam(marked.draw_start, marked.draw_width, frac_for_px(SEAM_PX), frac_for_px(MIN_MARK_PX));
            let (draw_start, draw_width) = (seam.draw_start, seam.draw_width);
            let steps = 2usize.max((draw_width * N as f64).round().clamp(0.0, 1e6) as usize);
            // The arrowhead, carried through the unroll like everything else, off the DRAWN span.
            let head_frac = frac_for_px(HEAD_PX);
            let hs = head_span(draw_start, draw_width, head_frac, f.forward);
            let head = (f.directional && fits_head(draw_width, head_frac) && shown(hs.tip_frac)).then(|| {
                [
                    mp_lane(hs.base_frac, depth - sp.lane_thickness / 2.0),
                    mp_lane(hs.tip_frac, depth),
                    mp_lane(hs.base_frac, depth + sp.lane_thickness / 2.0),
                ]
            });
            arcs.push(MorphArc { feature: i, runs: path_over(draw_start, draw_width, steps, depth), mark: marked.mark, head });
            if f.name.is_empty() {
                continue;
            }
            let Some((path, len)) = path_run(pc.start_frac, pc.width_frac, steps, depth) else { continue };
            let Some(text) = fit_label(&f.name, len - 2.0 * label_pad(sp.lane_font), sp.lane_font, &mut measure) else { continue };
            names.push(MorphName { feature: i, text, path, len, font: sp.lane_font });
        }
    }
    MorphMap { t, geom, center, half_span, backbone, arcs, names, thickness: sp.lane_thickness }
}

/// One letter set along a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathGlyph {
    /// Where the middle of the letter's advance sits on the path.
    pub x: f64,
    pub y: f64,
    /// Radians clockwise on screen from lying flat left to right.
    pub angle: f64,
}

/// The letters of a name set along `path` from its middle (`startOffset="50%"`,
/// `text-anchor="middle"`), given where each letter's middle falls along the straight text
/// (`centres`) and how wide the whole text is. A letter that would fall off either end of the path
/// is not set, as a browser leaves it out.
pub fn along_path(path: &[Point], len: f64, centres: &[f64], total: f64) -> Vec<Option<PathGlyph>> {
    centres
        .iter()
        .map(|c| {
            let mut d = len / 2.0 - total / 2.0 + c;
            if d < 0.0 || d > len {
                return None;
            }
            for w in path.windows(2) {
                let seg = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
                if d <= seg && seg > 0.0 {
                    let u = d / seg;
                    return Some(PathGlyph {
                        x: w[0].x + (w[1].x - w[0].x) * u,
                        y: w[0].y + (w[1].y - w[0].y) * u,
                        angle: (w[1].y - w[0].y).atan2(w[1].x - w[0].x),
                    });
                }
                d -= seg;
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::display::MapFeature;
    use super::super::labels::LabelDensity;
    use super::super::projection::{line_x, ring_point, straightness};
    use super::*;

    fn feature(start: i64, end: i64, name: &str) -> MapFeature {
        MapFeature {
            start,
            end,
            name: name.to_string(),
            kind: "CDS".into(),
            forward: true,
            locations: None,
            location_unsupported: false,
            role: super::super::display::FeatureRole::Coding,
            directional: true,
            source: 0,
        }
    }

    fn measure(text: &str, font: f64) -> f64 {
        text.chars().count() as f64 * font * 0.55
    }

    fn frame(width: f64, center: f64) -> Frame {
        Frame { center, width, density: LabelDensity::Roomy, ..Frame::ring(1000.0, 600.0) }
    }

    #[test]
    fn near_the_ring_end_the_backbone_is_the_whole_circle() {
        let scene = MapScene::new(&[], 6000, true);
        let f = frame(0.6199, 0.0);
        let m = layout_morph(&scene, f, straightness(f.width), measure);
        assert!(m.t > 0.0 && m.t < 1e-3);
        assert!((m.half_span - 0.5).abs() < 1e-3);
        // first and last sample are the same place: the far side of the ring, all the way round
        let (Some(a), Some(b)) = (m.backbone.first(), m.backbone.last()) else { panic!("no backbone") };
        assert!((a.x - b.x).abs() < 1.0 && (a.y - b.y).abs() < 1.0);
        let top = ring_point(0.0, 0.0, m.geom, None);
        let Some(mid) = m.backbone.get(N / 2) else { panic!("short backbone") };
        assert!((mid.x - top.x).abs() < 0.5 && (mid.y - top.y).abs() < 0.5, "the centre base is at 12 o'clock");
    }

    #[test]
    fn at_the_line_end_the_backbone_is_the_window_on_the_line_axis() {
        let scene = MapScene::new(&[], 6000, true);
        let f = frame(0.3, 0.25);
        let m = layout_morph(&scene, f, 1.0, measure);
        assert_eq!(m.half_span, 0.15);
        for p in &m.backbone {
            assert!((p.y - line_axis_y(600.0)).abs() < 1e-9);
        }
        let (Some(a), Some(b)) = (m.backbone.first(), m.backbone.last()) else { panic!("no backbone") };
        assert!((a.x - line_x(0.1, 0.1, 0.3, 1000.0, LINE_MARGIN)).abs() < 1e-6);
        assert!((b.x - line_x(0.4, 0.1, 0.3, 1000.0, LINE_MARGIN)).abs() < 1e-6);
    }

    #[test]
    fn a_feature_on_the_far_side_is_clipped_mid_unroll_and_one_in_view_is_named_along_its_band() {
        let scene = MapScene::new(&[feature(5700, 300, "near"), feature(2800, 3200, "far away")], 6000, true);
        let f = frame(0.45, 0.0);
        let m = layout_morph(&scene, f, straightness(0.45), measure);
        assert!(m.half_span < 0.5);
        let near: Vec<&MorphArc> = m.arcs.iter().filter(|a| a.feature == 0).collect();
        assert!(near.iter().any(|a| !a.runs.is_empty()));
        assert!(m.arcs.iter().filter(|a| a.feature == 1).all(|a| a.runs.is_empty()), "the far side never ghosts into frame");
        assert!(m.names.iter().any(|n| n.feature == 0));
        for n in &m.names {
            let (Some(a), Some(b)) = (n.path.first(), n.path.last()) else { panic!("empty name path") };
            assert!(a.x <= b.x, "runs left to right");
        }
    }

    #[test]
    fn letters_set_along_a_path_sit_on_it_centred_on_its_middle() {
        let path = [Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }, Point { x: 100.0, y: 100.0 }];
        let g = along_path(&path, 200.0, &[5.0, 15.0], 20.0);
        let Some(Some(first)) = g.first() else { panic!("no glyph") };
        assert!((first.x - 95.0).abs() < 1e-9 && first.y == 0.0 && first.angle == 0.0);
        let Some(Some(second)) = g.get(1) else { panic!("no glyph") };
        assert!((second.x - 100.0).abs() < 1e-9 && (second.y - 5.0).abs() < 1e-9);
        assert!((second.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(along_path(&path, 200.0, &[-500.0], 20.0), vec![None]);
    }
}
