//! The map as a ring: where the axis, its scale, every arc and every name sits. Ported from
//! `renderRing` and `layoutRingAnnotations` in Bios `editor/overview/PlasmidOverview.tsx`.
//!
//! Two rings, and the split is the whole layout. The OUTER one is the molecule itself: the
//! backbone and its coordinate scale, everything that is a POSITION on the DNA. The annotations
//! nest INSIDE it, because a feature is a stretch of that molecule, not a thing orbiting it.
//!
//! Not ported, because the native map does not draw them yet: the hover card of a cut, point marks, the outer
//! band, the selection, the caret, relation chords, and anything that answers the pointer.

use std::collections::HashMap;

use super::direction::{HEAD_PX, arrow_frac, fits_head, head_span};
use super::feature_geometry::{feature_anchor, feature_drawing_pieces, feature_pieces};
use super::labels::{
    ColItem, LabelSpacing, Prioritised, arc_available_px, cap_by_priority, fit_label, flag_height, flag_width, group_cuts_by_position, join_enzymes,
    label_pad, label_spacing, lanes_for_radius, merge_bands, pack_column,
};
use super::presence::{MIN_MARK_PX, SEAM_PX, arc_mark, arc_seam};
use super::projection::{NAV_RESERVE, Point, RingGeom, TAU, ring_geom, ring_point};
use super::scene::{CUT_FONT, Flag, Frame, MapScene, flag_font, flag_row, group_thousands, lane_depth, lane_inset, nice_step};

/// Which way a number on the scale is set against its tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A coordinate printed beside a major tick, upright: you read a coordinate, you don't admire it,
/// and rotated digits at the bottom of a ring are upside down.
#[derive(Clone, Debug, PartialEq)]
pub struct TickNumber {
    /// Vertical middle of the text, and the edge or middle `anchor` names.
    pub at: Point,
    pub text: String,
    pub anchor: Anchor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RingTick {
    /// On the axis.
    pub inner: Point,
    pub outer: Point,
    pub major: bool,
    pub number: Option<TickNumber>,
}

/// One piece of a feature, as a band along its lane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingArc {
    /// Index into [`MapScene::features`].
    pub feature: usize,
    /// The span to draw, after the mark floor and the seam.
    pub start_frac: f64,
    pub width_frac: f64,
    /// The lane's centre line.
    pub radius: f64,
    /// Too small to draw at its true width, so widened to a tick and drawn stronger.
    pub mark: bool,
    /// A dashed hairline between two segments of a joined location, not a band.
    pub connector: bool,
    /// The direction mark: one corner of its base, its tip on the centre line, the other corner.
    pub head: Option<[Point; 3]>,
}

/// A name written along the feature's own arc.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcLabel {
    pub feature: usize,
    pub text: String,
    /// Where the middle of the text sits.
    pub frac: f64,
    /// The circle the middle of the letters rides on.
    pub radius: f64,
    /// True on the top of the ring, where the text follows the way the bases run. On the bottom it
    /// runs the other way so the letters come out the right way up.
    pub upright: bool,
    pub font: f64,
}

/// A cut site on the ring: a hash across the backbone, and its enzymes' name on a leader when the
/// column had room. A cut whose name did not fit still gets its hash.
#[derive(Clone, Debug, PartialEq)]
pub struct RingCut {
    /// The hash, from 6 inside the backbone to 6 outside it.
    pub tin: Point,
    pub tout: Point,
    pub label: Option<RingCutLabel>,
}

/// An enzyme name in a side column: set from `x` toward the ring (`anchor` is the edge at `x`),
/// its middle at `y`, with a leader from the hash's outer end.
#[derive(Clone, Debug, PartialEq)]
pub struct RingCutLabel {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub anchor: Anchor,
    pub leader: (Point, Point),
}

/// Everything the ring draws, back to front: the axis circle, the ticks, the arcs, the names.
#[derive(Clone, Debug, PartialEq)]
pub struct RingMap {
    pub geom: RingGeom,
    /// The viewport's centre, for turning a fraction into a point with [`ring_point`].
    pub center: f64,
    /// The backbone: the axis the scale hangs off.
    pub axis_r: f64,
    /// How thick a lane's band is drawn.
    pub thickness: f64,
    pub ticks: Vec<RingTick>,
    pub arcs: Vec<RingArc>,
    pub arc_labels: Vec<ArcLabel>,
    pub flags: Vec<Flag>,
    pub cuts: Vec<RingCut>,
}

/// The ring has to leave room for what floats beside it. With no cut names to make room for, the
/// margin is a share of the frame's short side, never under 28px and never over 30% of it.
///
/// It must be the same number everywhere [`ring_geom`] is called, or a renderer and a hit-test
/// would disagree about where a base is.
pub fn ring_margin(w: f64, h: f64) -> Option<f64> {
    ring_margin_for_cuts(w, h, 0.0)
}

/// [`ring_margin`] when enzyme names float beside the ring: the margin grows to hold the longest
/// of them, `longest_cut` wide at [`CUT_FONT`], plus 30.
pub fn ring_margin_for_cuts(w: f64, h: f64, longest_cut: f64) -> Option<f64> {
    ring_margin_for(w, h, longest_cut, 0.0)
}

/// [`ring_margin_for_cuts`] with room outside the ring for a band of what is being added.
///
/// The reserve is added to the margin and the cap lifts with it: a band is worth giving ring up
/// for, but only on a construct that has one — paying for it always would shrink every map to
/// serve a rare one.
pub fn ring_margin_for(w: f64, h: f64, longest_cut: f64, band_reserve: f64) -> Option<f64> {
    let lim = w.min(h);
    if lim == 0.0 || lim.is_nan() {
        return None;
    }
    let cut = if longest_cut > 0.0 { longest_cut + 30.0 } else { 0.0 };
    let band = band_reserve.max(0.0);
    let cap = lim * if band > 0.0 { 0.38 } else { 0.3 };
    Some((28f64.max(lim * 0.15).max(cut) + band).min(cap))
}

/// The numbers every part of the ring is placed from.
#[derive(Clone, Copy)]
struct Ring {
    geom: RingGeom,
    center: f64,
    sp: LabelSpacing,
    axis_r: f64,
    /// Lane 0's centre line.
    feature_r: f64,
    w: f64,
    h: f64,
    l: i64,
    cut_names: bool,
}

impl Ring {
    fn point(&self, frac: f64, radius: f64) -> Point {
        let p = ring_point(frac, self.center, self.geom, Some(radius));
        Point { x: p.x, y: p.y }
    }

    fn lane_r(&self, lane: usize) -> f64 {
        self.feature_r - lane_depth(lane, self.sp)
    }

    /// Whether a point at this height is on the part of the ring in the frame.
    fn on_arc(&self, y: f64) -> bool {
        y <= self.h + 60.0 && y >= -60.0
    }

    fn side_of(&self, x: f64) -> f64 {
        if x >= self.geom.cx { 1.0 } else { -1.0 }
    }

    /// Slide a settled label back out onto a circle just outside the backbone, so a fan that
    /// spread downward hugs the curve instead of marching across the middle of the ring.
    fn hug_x(&self, y: f64, side: f64, pad: f64) -> f64 {
        let rr = self.axis_r + pad;
        let dy = y - self.geom.cyc;
        self.geom.cx + side * (rr * rr - dy * dy).max(0.0).sqrt()
    }

    /// Where a long name can actually live. At three o'clock the backbone is at its furthest from
    /// the centre line and there may be no room between it and the frame edge; near the top and
    /// bottom the ring has drawn back in. So an over-long label's ideal y is pushed toward the
    /// nearer pole until the name fits, which is where a printed map puts its long names anyway.
    fn fit_y(&self, y: f64, side: f64, pad: f64, est_w: f64) -> f64 {
        let rr = self.axis_r + pad;
        let room = (if side > 0.0 { self.w - self.geom.cx } else { self.geom.cx }) - est_w - 8.0;
        if room >= rr {
            return y; // fits at any angle
        }
        let need = (rr * rr - room.max(0.0).powi(2)).max(0.0).sqrt();
        let dy = y - self.geom.cyc;
        if dy.abs() >= need {
            return y; // already far enough from the ring's widest point
        }
        self.geom.cyc + if dy >= 0.0 { need } else { -need }
    }
}

/// Lay out the ring for `scene` in `frame`. `measure(text, font_px)` is the advance width of `text`
/// in the interface font.
pub fn layout_ring(scene: &MapScene, frame: Frame, mut measure: impl FnMut(&str, f64) -> f64) -> RingMap {
    let longest_cut = scene.cuts.iter().fold(0.0, |m: f64, (_, name)| m.max(measure(name, CUT_FONT)));
    let geom = ring_geom(frame.width, frame.w, frame.h, ring_margin_for(frame.w, frame.h, longest_cut, frame.band_reserve));
    let sp = label_spacing(frame.density);
    let axis_r = geom.r;
    let ring = Ring {
        geom,
        center: frame.center,
        sp,
        axis_r,
        feature_r: axis_r - lane_inset(sp),
        w: frame.w,
        h: frame.h,
        l: scene.length,
        cut_names: frame.cut_names,
    };
    let mut map = RingMap {
        geom,
        center: frame.center,
        axis_r,
        thickness: sp.lane_thickness,
        ticks: Vec::new(),
        arcs: Vec::new(),
        arc_labels: Vec::new(),
        flags: Vec::new(),
        cuts: Vec::new(),
    };
    if scene.length <= 0 {
        return map;
    }
    // Lanes nest inward, so a ring that is not big enough for six of them has to say so. Features
    // past what fits share the innermost drawable lane rather than disappearing.
    let max_lane = usize::try_from(lanes_for_radius(ring.feature_r, sp, None)).unwrap_or(0);
    let lanes: Vec<usize> = (0..scene.features.len()).map(|i| scene.lane(i).min(max_lane)).collect();

    map.ticks = ring_ticks(&ring, frame.width);
    map.arcs = ring_arcs(&ring, scene, &lanes);
    let named = name_on_arcs(&ring, scene, &lanes, &mut measure);
    (map.flags, map.cuts) = ring_flags(&ring, scene, &lanes, &named, &mut measure);
    map.arc_labels = named.labels;
    map
}

/// The coordinate ring: an axis outside the annotations, ticked and numbered. The step comes from
/// the span in view, so it stays a sensible round number: 1,000 across a whole 6 kb plasmid.
fn ring_ticks(ring: &Ring, width: f64) -> Vec<RingTick> {
    let lf = ring.l as f64;
    let step = nice_step(width.max(1.0) * lf / 7.0);
    let minor = step / 5.0;
    let mut ticks = Vec::new();
    if minor.is_nan() || minor <= 0.0 {
        return ticks;
    }
    let mut bp = 0.0;
    while bp < lf {
        let major = bp % step == 0.0;
        let frac = bp / lf;
        let inner = ring.point(frac, ring.axis_r);
        if ring.on_arc(inner.y) {
            let outer = ring.point(frac, ring.axis_r + if major { 6.0 } else { 3.0 });
            let number = major.then(|| {
                let at = ring.point(frac, ring.axis_r + 12.0);
                let anchor = if at.x > ring.geom.cx + 6.0 {
                    Anchor::Start
                } else if at.x < ring.geom.cx - 6.0 {
                    Anchor::End
                } else {
                    Anchor::Middle
                };
                TickNumber { at, text: group_thousands(if bp == 0.0 { 1.0 } else { bp }), anchor }
            });
            ticks.push(RingTick { inner, outer, major, number });
        }
        bp += minor;
    }
    ticks
}

/// Nested features draw on their own concentric lane rather than painting over each other.
fn ring_arcs(ring: &Ring, scene: &MapScene, lanes: &[usize]) -> Vec<RingArc> {
    let thick = ring.sp.lane_thickness;
    let mut arcs = Vec::new();
    for (i, f) in scene.features.iter().enumerate() {
        let lr = ring.lane_r(lanes.get(i).copied().unwrap_or(0));
        for p in feature_drawing_pieces(f, ring.l) {
            if p.connector {
                arcs.push(RingArc {
                    feature: i,
                    start_frac: p.start_frac,
                    width_frac: p.width_frac,
                    radius: lr,
                    mark: false,
                    connector: true,
                    head: None,
                });
                continue;
            }
            // Widened to the mark floor before drawing. On the ring the arc gets THINNER as it
            // nests inward, so the floor is converted at this lane's own radius to stay a constant
            // number of pixels.
            let marked = arc_mark(p.start_frac, p.width_frac, arrow_frac(MIN_MARK_PX, lr));
            // Then the seam: a hairline of ring left between this feature and the next, so two arcs
            // of the same role stay two arcs.
            let seam = arc_seam(marked.draw_start, marked.draw_width, arrow_frac(SEAM_PX, lr), arrow_frac(MIN_MARK_PX, lr));
            // A mark at the 5' end, pointing the way the feature reads, inside the first few px of
            // the band so a feature never appears to reach further than its coordinates say.
            let head_frac = arrow_frac(HEAD_PX, lr);
            let head = (p.show_head && f.directional && fits_head(seam.draw_width, head_frac)).then(|| {
                let hs = head_span(seam.draw_start, seam.draw_width, head_frac, f.forward);
                [ring.point(hs.base_frac, lr - thick / 2.0), ring.point(hs.tip_frac, lr), ring.point(hs.base_frac, lr + thick / 2.0)]
            });
            arcs.push(RingArc {
                feature: i,
                start_frac: seam.draw_start,
                width_frac: seam.draw_width,
                radius: lr,
                mark: marked.mark,
                connector: false,
                head,
            });
        }
    }
    arcs
}

/// What the pass that writes names along their own arcs leaves for the pass that flags the rest.
struct Named {
    labels: Vec<ArcLabel>,
    /// Features that wanted to name themselves in place and could not, best first.
    flagged: Vec<usize>,
    /// Bands of each side's column already taken by a name written along its arc.
    obstacles: Vec<(f64, ColItem)>,
}

fn overlaps(a0: f64, a1: f64, b0: f64, b1: f64) -> bool {
    a0 < b1 && b0 < a1
}

/// A feature big enough to hold its own name gets it written ALONG ITS ARC instead of out in the
/// column: the single biggest source of air on a printed map.
///
/// A stretch of a lane can only carry one name, and features nest freely, so the arc is a claimed
/// resource: walk the features best-first and give each stretch to the most useful name that wants
/// it. Claims are per lane: two features at the same angle on different lanes sit at different
/// radii, so both can write their name in place.
fn name_on_arcs(ring: &Ring, scene: &MapScene, lanes: &[usize], measure: &mut impl FnMut(&str, f64) -> f64) -> Named {
    let sp = ring.sp;
    let mut named = Named { labels: Vec::new(), flagged: Vec::new(), obstacles: Vec::new() };
    let mut claims: HashMap<usize, Vec<(f64, f64)>> = HashMap::new();

    let mut by_rank: Vec<usize> = (0..scene.features.len()).filter(|&i| scene.features.get(i).is_some_and(|f| !f.name.is_empty())).collect();
    by_rank.sort_by(|&a, &b| scene.priority(b).total_cmp(&scene.priority(a)).then_with(|| a.cmp(&b)));

    for i in by_rank {
        let Some(f) = scene.features.get(i) else { continue };
        let lane = lanes.get(i).copied().unwrap_or(0);
        let fr = ring.lane_r(lane); // this feature's own lane, not the backbone
        let frac = feature_anchor(f, ring.l);
        if !ring.on_arc(ring.point(frac, fr).y) {
            continue; // off the visible arc
        }
        // A feature in two pieces across the origin is left to the column: a name split over the
        // seam reads worse than a leader does.
        let pieces = feature_pieces(f, ring.l);
        let [piece] = pieces.as_slice() else {
            named.flagged.push(i);
            continue;
        };
        // Does the name fit inside the feature's own sweep? Measured, and shortened to fit rather
        // than abandoned: a name shortened on the thing it names beats a full name pointing at it
        // from off in the margin.
        let room = arc_available_px(piece.width_frac, fr, sp.lane_font);
        let Some(text) = fit_label(&f.name, room, sp.lane_font, &mut *measure) else {
            named.flagged.push(i); // nothing legible fits on the arc
            continue;
        };
        let text_px = measure(&text, sp.lane_font);
        let half_frac = (text_px / 2.0 + label_pad(sp.lane_font)) / (TAU * fr);
        let (f0, f1) = (frac - half_frac, frac + half_frac);
        // The ±1 shifts too, so a claim straddling the origin still blocks its true neighbours.
        let taken = claims.get(&lane).is_some_and(|c| {
            c.iter().any(|&(g0, g1)| overlaps(f0, f1, g0, g1) || overlaps(f0 - 1.0, f1 - 1.0, g0, g1) || overlaps(f0 + 1.0, f1 + 1.0, g0, g1))
        });
        if taken {
            named.flagged.push(i); // a better name owns this stretch
            continue;
        }
        claims.entry(lane).or_default().push((f0, f1));

        // Keep the letters upright. Following the arc the way the bases run works on the top of
        // the ring; on the bottom that would hang the text upside down.
        let theta = ((((frac - ring.center) % 1.0) + 1.5) % 1.0 - 0.5) * TAU; // (-π, π] from 12 o'clock
        let upright = theta.cos() > 0.0;
        // ON the lane, centred on the thing it names. The nudge differs for the reversed text on
        // the bottom of the ring, where the middle of the letters lands slightly differently.
        let r = fr + if upright { -0.5 } else { 1.0 };

        // Reserve the band this name occupies so the column has to flow around it. Only the
        // stretch the TEXT covers, not the whole feature's sweep.
        let t_a = ring.point(f0, r);
        let t_b = ring.point(f1, r);
        let (side_a, side_b) = (ring.side_of(t_a.x), ring.side_of(t_b.x));
        let sides = if side_a == side_b { vec![side_a] } else { vec![side_a, side_b] };
        for s in sides {
            let item =
                ColItem { id: format!("arc{i}:{s}"), y: (t_a.y + t_b.y) / 2.0, h: (t_b.y - t_a.y).abs() + sp.feature_row, pri: 0.0, pinned: true };
            named.obstacles.push((s, item));
        }
        named.labels.push(ArcLabel { feature: i, text, frac, radius: r, upright, font: sp.lane_font });
    }
    named
}

/// A name competing for a place in one side's column: a feature's flag or an enzyme's name.
#[derive(Clone, Debug, PartialEq)]
struct Cand {
    /// The feature, or for a cut the index of its group.
    feature: usize,
    /// For a cut, its hash; a cut's name is a bare label on a leader, a feature's is a chip.
    cut: Option<(Point, Point)>,
    text: String,
    /// Width as drawn: the whole chip, or the bare text. Measured, never estimated.
    w: f64,
    /// The height it claims in the column.
    row: f64,
    /// How far outside the backbone it hugs.
    pad: f64,
    pri: f64,
    /// On the feature's own lane: where the leader starts.
    anchor: Point,
    /// Level with the point just outside the backbone: where the label WANTS to sit.
    oy: f64,
    side: f64,
}

impl Cand {
    fn id(&self) -> String {
        format!("{}{}", if self.cut.is_some() { "c" } else { "f" }, self.feature)
    }
}

impl Prioritised for Cand {
    fn pri(&self) -> f64 {
        self.pri
    }
}

/// A feature that could not write its name on its own arc says it on a flag, if it is drawn wide
/// enough to see: if you can see it, you can read what it is. A feature narrower than a mark is a
/// tick whose extent you cannot see, and aiming a leader at it says more about the leader.
fn flag_candidates(ring: &Ring, scene: &MapScene, lanes: &[usize], flagged: &[usize], measure: &mut impl FnMut(&str, f64) -> f64) -> Vec<Cand> {
    let sp = ring.sp;
    let font = flag_font(sp);
    let mut cand = Vec::new();
    for &i in flagged {
        let Some(f) = scene.features.get(i) else { continue };
        let fr = ring.lane_r(lanes.get(i).copied().unwrap_or(0));
        // Measured at the feature's OWN lane radius: a lane nested inward has a shorter circumference.
        let drawn_px = feature_pieces(f, ring.l).iter().fold(0.0, |m: f64, q| m.max(q.width_frac)) * TAU * fr;
        if drawn_px < MIN_MARK_PX {
            continue;
        }
        let frac = feature_anchor(f, ring.l);
        let out = ring.point(frac, ring.feature_r + sp.feature_pad);
        let side = ring.side_of(out.x);
        // Capped at a share of the frame so one pathological name can't take the whole column.
        let Some(text) = fit_label(&f.name, ring.w * 0.32, font, &mut *measure) else { continue };
        let w = flag_width(measure(&text, font), font);
        cand.push(Cand {
            feature: i,
            cut: None,
            text,
            w,
            row: flag_row(sp),
            pad: sp.feature_pad,
            pri: scene.priority(i),
            // The leader starts on the feature's OWN lane: a nested feature's flag has to point at
            // the arc it names, which may be several lanes in.
            anchor: ring.point(frac, fr),
            oy: ring.fit_y(out.y, side, sp.feature_pad, w),
            side,
        });
    }
    // Every cut on the visible arc, grouped by position so isoschizomers share one name.
    for (g, group) in group_cuts_by_position(&scene.cuts, None).iter().enumerate() {
        let frac = group.pos as f64 / ring.l as f64;
        let p = ring.point(frac, ring.feature_r);
        if !ring.on_arc(p.y) {
            continue;
        }
        let out = ring.point(frac, ring.feature_r + sp.cut_pad);
        let side = ring.side_of(out.x);
        let text = join_enzymes(&group.enzymes);
        let w = measure(&text, CUT_FONT);
        cand.push(Cand {
            feature: g,
            cut: Some((ring.point(frac, ring.axis_r - 6.0), ring.point(frac, ring.axis_r + 6.0))),
            text,
            w,
            row: sp.cut_row,
            pad: sp.cut_pad,
            pri: 0.0,
            anchor: p,
            oy: ring.fit_y(out.y, side, sp.cut_pad, w),
            side,
        });
    }
    cand
}

/// One column per side holds every flag. A crowd is spread around its own centre of gravity
/// instead of losing members, and a name only goes missing when the frame is genuinely full.
fn settle_columns(ring: &Ring, shown: &[Cand], obstacles: &[(f64, ColItem)]) -> HashMap<String, f64> {
    let sp = ring.sp;
    let (top, bottom) = (10.0, ring.h - NAV_RESERVE);
    let mut settled = HashMap::new();
    for side in [-1.0, 1.0] {
        let mut items: Vec<ColItem> = shown
            .iter()
            .filter(|c| c.side == side && (ring.cut_names || c.cut.is_none()))
            .map(|c| ColItem { id: c.id(), y: c.oy, h: c.row, pri: c.pri, pinned: false })
            .collect();
        let bands: Vec<ColItem> = obstacles.iter().filter(|(s, _)| *s == side).map(|(_, item)| item.clone()).collect();
        items.extend(merge_bands(&bands, Some(&format!("arcband{side}:"))));
        for p in pack_column(&items, top, bottom, Some(sp.gap)) {
            settled.insert(p.id, p.y);
        }
    }
    settled
}

/// Give every label in a STACK the same outer edge, which is what makes a crowd read as a stack
/// rather than a cascade: the ring's curve would give each a different x and the run would come out
/// as a diagonal staircase. Labels closer than a row apart were packed against each other and
/// belong to one run; the run takes its x from its most outward member, held inside the frame.
fn align_columns(ring: &Ring, shown: &[Cand], settled: &HashMap<String, f64>) -> HashMap<String, f64> {
    let sp = ring.sp;
    let mut col_x = HashMap::new();
    for side in [-1.0, 1.0] {
        let mut col: Vec<(&Cand, f64)> = shown.iter().filter(|c| c.side == side).filter_map(|c| settled.get(&c.id()).map(|y| (c, *y))).collect();
        col.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut run_start = 0;
        while run_start < col.len() {
            let mut run_end = run_start + 1;
            while let (Some(prev), Some(next)) = (col.get(run_end - 1), col.get(run_end))
                && next.1 - prev.1 <= sp.feature_row + sp.gap + 8.0
            {
                run_end += 1;
            }
            let run = col.get(run_start..run_end).unwrap_or_default();
            let outers = run.iter().map(|(c, y)| ring.hug_x(*y, side, c.pad) + side * c.w);
            let x =
                if side < 0.0 { outers.fold(f64::INFINITY, f64::min).max(8.0) } else { outers.fold(f64::NEG_INFINITY, f64::max).min(ring.w - 8.0) };
            for (c, _) in run {
                col_x.insert(c.id(), x);
            }
            run_start = run_end;
        }
    }
    col_x
}

fn ring_flags(
    ring: &Ring,
    scene: &MapScene,
    lanes: &[usize],
    named: &Named,
    measure: &mut impl FnMut(&str, f64) -> f64,
) -> (Vec<Flag>, Vec<RingCut>) {
    let sp = ring.sp;
    let font = flag_font(sp);
    let cand = flag_candidates(ring, scene, lanes, &named.flagged, measure);
    // A ceiling with a name on it: past this many, packing would only shed the tail by accident.
    let shown = cap_by_priority(&cand, sp.max_labels);
    let settled = settle_columns(ring, &shown, &named.obstacles);
    let col_x = align_columns(ring, &shown, &settled);

    let mut flags = Vec::new();
    let mut cuts = Vec::new();
    for c in &cand {
        let Some(&y) = settled.get(&c.id()) else {
            // The column was genuinely full. A cut keeps its hash; a flag is not drawn.
            if let Some((tin, tout)) = c.cut {
                cuts.push(RingCut { tin, tout, label: None });
            }
            continue;
        };
        // `outer` is the edge AWAY from the ring, and it is the one that lines up.
        let outer = col_x.get(&c.id()).copied().unwrap_or_else(|| {
            let hug = ring.hug_x(y, c.side, c.pad);
            if c.side < 0.0 { (hug - c.w).max(8.0) } else { (hug + c.w).min(ring.w - 8.0) }
        });
        // Where the label starts on the ring side: the leader attaches here, not at the aligned
        // margin, so a short name in a wide column still gets a leader that reaches it.
        let inner = outer - c.side * c.w;
        let leader_x = inner - c.side * 4.0;
        if let Some((tin, tout)) = c.cut {
            let anchor = if c.side > 0.0 { Anchor::End } else { Anchor::Start };
            let label = RingCutLabel { text: c.text.clone(), x: outer, y, anchor, leader: (tout, Point { x: leader_x, y }) };
            cuts.push(RingCut { tin, tout, label: Some(label) });
            continue;
        }
        flags.push(Flag {
            feature: c.feature,
            text: c.text.clone(),
            x: outer.min(inner),
            y,
            w: c.w,
            h: flag_height(font),
            font,
            leader: (c.anchor, Point { x: leader_x, y }),
        });
    }
    (flags, cuts)
}
