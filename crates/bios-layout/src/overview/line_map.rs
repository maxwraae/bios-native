//! The map as a line: where the axis, its scale, every bar and every name sits. Ported from
//! `renderLine` and `layoutLineAnnotations` in Bios `editor/overview/PlasmidOverview.tsx`.
//!
//! Same split as the ring, unrolled. The AXIS carries the molecule (backbone and coordinate scale)
//! and the annotations stack ABOVE it, which is how every published linear map is drawn: the
//! coordinate line under the construct, not through the middle of it. So a deeper lane is a
//! smaller y.
//!
//! Not ported, because the native map does not draw them yet: the base-reading mode a line swaps
//! to when the letters are wide enough to read, the hover card of a cut, point marks, the outer band, the
//! selection, the caret, relation brackets, and anything that answers the pointer.

use std::collections::{HashMap, HashSet};

use super::direction::{HEAD_PX, fits_head};
use super::feature_geometry::{feature_anchor, feature_drawing_pieces};
use super::labels::{
    LabelSpacing, Prioritised, RowItem, bar_available_px, cap_by_priority, fit_label, flag_height, flag_width, group_cuts_by_position, join_enzymes,
    label_spacing, pack_rows,
};
use super::presence::{is_mark, seam_span, widen_to_mark};
use super::projection::{LINE_MARGIN, NAV_RESERVE, Point, line_window, line_x};
use super::scene::{CUT_FONT, Flag, Frame, MapScene, cut_overhang, flag_font, flag_row, group_thousands, lane_depth, lane_inset, nice_step};
use super::viewport::{Projection, Viewport, visible_range};

/// Room kept under the axis: the coordinate numbers, then the title block.
pub const AXIS_RESERVE: f64 = 92.0;

/// One tick hanging off the axis.
#[derive(Clone, Debug, PartialEq)]
pub struct LineTick {
    pub x: f64,
    pub major: bool,
    /// The coordinate under a major tick, centred on `x` with its baseline [`NUMBER_DROP`] below
    /// the axis.
    pub number: Option<String>,
}

/// How far under the axis the baseline of a coordinate number sits.
pub const NUMBER_DROP: f64 = 20.0;

/// One piece of a feature, as a bar in its lane.
#[derive(Clone, Debug, PartialEq)]
pub struct LineBar {
    /// Index into [`MapScene::features`].
    pub feature: usize,
    /// The span to draw, after the mark floor and the seam.
    pub x0: f64,
    pub x1: f64,
    /// The lane's centre line.
    pub y: f64,
    /// Too small to draw at its true width, so widened to a tick and drawn stronger.
    pub mark: bool,
    /// A dashed hairline between two segments of a joined location, not a bar.
    pub connector: bool,
    /// The direction mark: one corner of its base, its tip on the centre line, the other corner.
    pub head: Option<[Point; 3]>,
    /// The name written inside the bar, centred, when the longest legible form of it fits.
    pub name: Option<String>,
}

/// A cut site on the line: a hash across the strand, and its enzymes' name on a leader when it
/// won a row. A cut whose name did not fit still gets its hash.
#[derive(Clone, Debug, PartialEq)]
pub struct LineCut {
    pub x: f64,
    /// The hash runs from `y0` to `y1`, across the axis.
    pub y0: f64,
    pub y1: f64,
    /// The name, centred on its `x`, with its baseline at `y`, and the leader from the top of the
    /// hash to just under it.
    pub label: Option<CutLabel>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CutLabel {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub leader: (Point, Point),
}

/// Everything the line draws, back to front: the axis, the ticks, the bars, the flags.
#[derive(Clone, Debug, PartialEq)]
pub struct LineMap {
    pub axis_y: f64,
    /// The axis runs from `left` to `right`.
    pub left: f64,
    pub right: f64,
    /// How thick a lane's bar is drawn.
    pub thickness: f64,
    /// Type size of a name written inside a bar.
    pub lane_font: f64,
    pub ticks: Vec<LineTick>,
    pub bars: Vec<LineBar>,
    pub flags: Vec<Flag>,
    pub cuts: Vec<LineCut>,
}

/// The numbers every part of the line is placed from.
#[derive(Clone, Copy)]
struct Line {
    sp: LabelSpacing,
    /// Left edge of the window, as a fraction of the molecule.
    window_left: f64,
    width: f64,
    w: f64,
    margin_x: f64,
    axis_y: f64,
    l: i64,
    cut_names: bool,
}

impl Line {
    fn x(&self, frac: f64) -> f64 {
        line_x(frac, self.window_left, self.width, self.w, self.margin_x)
    }

    fn clamp_x(&self, v: f64) -> f64 {
        self.margin_x.max((self.w - self.margin_x).min(v))
    }

    /// Lane 0's centre, just clear of the axis.
    fn lane_base(&self) -> f64 {
        self.axis_y - lane_inset(self.sp)
    }

    fn lane_y(&self, lane: usize) -> f64 {
        self.lane_base() - lane_depth(lane, self.sp)
    }

    fn in_window(&self, frac: f64) -> bool {
        frac >= self.window_left - 1e-6 && frac <= self.window_left + self.width + 1e-6
    }
}

/// Lay out the line for `scene` in `frame`. `measure(text, font_px)` is the advance width of `text`
/// in the interface font.
pub fn layout_line(scene: &MapScene, frame: Frame, mut measure: impl FnMut(&str, f64) -> f64) -> LineMap {
    let sp = label_spacing(frame.density);
    let vp = Viewport { center: frame.center, width: frame.width, projection: Projection::Line };
    let deepest = (0..scene.features.len()).map(|i| scene.lane(i)).max().unwrap_or(0);
    // The axis is placed so the whole drawing sits centred: a construct three lanes deep should
    // not leave a third of the frame empty above it just because the axis is pinned low.
    let stack_up = lane_inset(sp) + lane_depth(deepest, sp) + sp.lane_thickness / 2.0;
    let stack_down = AXIS_RESERVE;
    let axis_y = match frame.axis_at_bottom {
        // A band in a page: the axis sits just above the floor and the features stand on it.
        Some(reserve) => (frame.h - reserve).max(stack_up),
        None => (frame.h - NAV_RESERVE - stack_down).min((frame.h - (stack_up + stack_down)) / 2.0 + stack_up),
    };
    let line = Line {
        sp,
        window_left: line_window(vp).left,
        width: frame.width,
        w: frame.w,
        margin_x: LINE_MARGIN,
        axis_y,
        l: scene.length,
        cut_names: frame.cut_names,
    };
    let mut map = LineMap {
        axis_y,
        left: line.margin_x,
        right: frame.w - line.margin_x,
        thickness: sp.lane_thickness,
        lane_font: sp.lane_font,
        ticks: Vec::new(),
        bars: Vec::new(),
        flags: Vec::new(),
        cuts: Vec::new(),
    };
    if scene.length <= 0 {
        return map;
    }
    map.ticks = line_ticks(&line, vp);
    let drawn = line_bars(&line, scene, &mut measure);
    (map.flags, map.cuts) = line_flags(&line, scene, &drawn, deepest, &mut measure);
    map.bars = drawn.bars;
    map
}

/// Round steps off the span in view, the same rule the ring's scale uses, with minor ticks at a
/// fifth. Both projections divide their ruler identically.
fn line_ticks(line: &Line, vp: Viewport) -> Vec<LineTick> {
    let lf = line.l as f64;
    let range = visible_range(vp, line.l);
    let step = nice_step(range.span as f64 / 7.0);
    let minor = step / 5.0;
    let mut ticks = Vec::new();
    if minor.is_nan() || minor <= 0.0 {
        return ticks;
    }
    let end = (range.start_bp + range.span) as f64;
    let mut bp = (range.start_bp as f64 / minor).ceil() * minor;
    while bp <= end {
        let x = line.x(bp / lf);
        if x >= line.margin_x - 1.0 && x <= line.w - line.margin_x + 1.0 {
            let major = bp % step == 0.0;
            let at = bp % lf;
            let number = major.then(|| group_thousands(if at == 0.0 { 1.0 } else { at }));
            ticks.push(LineTick { x, major, number });
        }
        bp += minor;
    }
    ticks
}

/// What drawing the bars leaves for the pass that flags the names that did not fit.
struct Drawn {
    bars: Vec<LineBar>,
    /// Features that already said their name inside their own bar: they need no flag.
    named_in_bar: HashSet<usize>,
    /// Features drawn at more than mark width: the only ones big enough to be worth flagging.
    drawn_wide: HashSet<usize>,
}

fn line_bars(line: &Line, scene: &MapScene, measure: &mut impl FnMut(&str, f64) -> f64) -> Drawn {
    let sp = line.sp;
    let half = sp.lane_thickness / 2.0;
    let mut drawn = Drawn { bars: Vec::new(), named_in_bar: HashSet::new(), drawn_wide: HashSet::new() };
    for (i, f) in scene.features.iter().enumerate() {
        let y = line.lane_y(scene.lane(i));
        let anchor = feature_anchor(f, line.l);
        for p in feature_drawing_pieces(f, line.l) {
            let true_x0 = line.clamp_x(line.x(p.start_frac));
            let true_x1 = line.clamp_x(line.x(p.start_frac + p.width_frac));
            if true_x1 <= true_x0 {
                continue;
            }
            if p.connector {
                drawn.bars.push(LineBar { feature: i, x0: true_x0, x1: true_x1, y, mark: false, connector: true, head: None, name: None });
                continue;
            }
            // Nothing is drawn thinner than a mark: absent says "nothing here", a mark says
            // "something, too small to name", and a hairline would say nothing at all.
            let mark = is_mark(true_x0, true_x1);
            if !mark {
                drawn.drawn_wide.insert(i);
            }
            // Widened to the floor, then inset by the seam: a bar gives up a hairline at each end
            // so the ground shows between it and its neighbour.
            let (wide0, wide1) = widen_to_mark(true_x0, true_x1);
            let (x0, x1) = seam_span(wide0, wide1);
            // The name goes IN the bar, on the longest form that fits. Only the widest piece of a
            // feature that wraps the origin is named; naming both halves says it twice.
            let name = (!f.name.is_empty() && (p.start_frac + p.width_frac / 2.0 - anchor).abs() < 1e-9)
                .then(|| fit_label(&f.name, bar_available_px(x0, x1, sp.lane_font), sp.lane_font, &mut *measure))
                .flatten();
            if name.is_some() {
                drawn.named_in_bar.insert(i);
            }
            // The direction mark at the 5' end, inside the bar so the extent stays honest.
            let head = (p.show_head && f.directional && fits_head(x1 - x0, HEAD_PX)).then(|| {
                let (base, tip) = if f.forward { (x0, x0 + HEAD_PX) } else { (x1, x1 - HEAD_PX) };
                [Point { x: base, y: y - half }, Point { x: tip, y }, Point { x: base, y: y + half }]
            });
            drawn.bars.push(LineBar { feature: i, x0, x1, y, mark, connector: false, head, name });
        }
    }
    drawn
}

/// A name competing for a row above the lanes: a feature's flag or an enzyme's name.
#[derive(Clone, Debug, PartialEq)]
struct Cand {
    /// The feature, or for a cut the index of its group.
    key: usize,
    /// A cut's name is a bare label on a leader; a feature's is a chip.
    cut: bool,
    text: String,
    /// Width as drawn: the whole chip, or the bare text. Measured, never estimated.
    w: f64,
    half: f64,
    /// The feature's own x, or the cut's: where the leader starts.
    cx: f64,
    /// The name's centre, held inside the frame.
    lx: f64,
    pri: f64,
    /// Top of the feature's own lane bar: where its leader attaches.
    lane_top: f64,
}

impl Cand {
    fn id(&self) -> String {
        format!("{}{}", if self.cut { "c" } else { "f" }, self.key)
    }
}

impl Prioritised for Cand {
    fn pri(&self) -> f64 {
        self.pri
    }
}

/// A feature whose bar was too narrow to hold its own name takes a flag: the linear twin of the
/// ring's column, same chip, same rule. Features that named themselves in place never reach the
/// stack, and a feature that is only a mark stays a tick. Then every cut in the window, grouped by
/// position so isoschizomers share one name.
fn flag_candidates(line: &Line, scene: &MapScene, drawn: &Drawn, measure: &mut impl FnMut(&str, f64) -> f64) -> Vec<Cand> {
    let sp = line.sp;
    let font = flag_font(sp);
    let clamp = |cx: f64, half: f64| (line.margin_x + half).max((line.w - line.margin_x - half).min(cx));
    let mut cand = Vec::new();
    for (i, f) in scene.features.iter().enumerate() {
        if f.name.is_empty() || drawn.named_in_bar.contains(&i) || !drawn.drawn_wide.contains(&i) {
            continue;
        }
        let frac = feature_anchor(f, line.l);
        if !line.in_window(frac) {
            continue;
        }
        let Some(text) = fit_label(&f.name, line.w * 0.32, font, &mut *measure) else { continue };
        let w = flag_width(measure(&text, font), font);
        let half = w / 2.0;
        let cx = line.x(frac);
        cand.push(Cand {
            key: i,
            cut: false,
            text,
            w,
            half,
            cx,
            lx: clamp(cx, half),
            pri: scene.priority(i),
            lane_top: line.lane_y(scene.lane(i)) - sp.lane_thickness / 2.0,
        });
    }
    for (g, group) in group_cuts_by_position(&scene.cuts, None).iter().enumerate() {
        let frac = group.pos as f64 / line.l as f64;
        if !line.in_window(frac) {
            continue;
        }
        let cx = line.x(frac);
        let text = join_enzymes(&group.enzymes);
        let w = measure(&text, CUT_FONT);
        let half = 9f64.max(w / 2.0);
        cand.push(Cand { key: g, cut: true, text, w, half, cx, lx: clamp(cx, half), pri: 0.0, lane_top: line.axis_y - sp.lane_thickness / 2.0 });
    }
    cand
}

/// One stack of rows above the lanes holds every flag and every enzyme name. Names are placed
/// best-first, so the most useful one gets the row nearest the strand and its shortest leader, and
/// a name is dropped only when the stack has genuinely reached the top of the frame.
///
/// The cap is on names, not on existence: a cut that lost the competition for a row still draws
/// its hash.
fn line_flags(line: &Line, scene: &MapScene, drawn: &Drawn, deepest: usize, measure: &mut impl FnMut(&str, f64) -> f64) -> (Vec<Flag>, Vec<LineCut>) {
    const GAP: f64 = 8.0;
    let sp = line.sp;
    let font = flag_font(sp);
    let row_h = flag_row(sp);
    let reach = cut_overhang(sp.lane_thickness);
    // The row nearest the strand has to clear the LANES, not sit a fixed distance off the axis.
    let stack_top = line.lane_y(deepest) - sp.lane_thickness / 2.0;
    let first_row = (line.axis_y - 24.0).min(stack_top - 12.0);
    // Never stack up under the frame's top; beyond that the stack simply stops growing.
    let max_rows = ((first_row - 24.0) / row_h).floor().max(1.0) as usize;

    let cand = flag_candidates(line, scene, drawn, measure);
    let shown = cap_by_priority(&cand, sp.max_labels);
    // With enzyme names off, cuts do not compete for a row; their hashes are drawn regardless.
    let items: Vec<RowItem> =
        shown.iter().filter(|c| line.cut_names || !c.cut).map(|c| RowItem { id: c.id(), x: c.lx, half: c.half, pri: c.pri }).collect();
    let row_of: HashMap<String, usize> = pack_rows(&items, max_rows, Some(GAP)).into_iter().map(|p| (p.id, p.row)).collect();

    let mut flags = Vec::new();
    let mut cuts = Vec::new();
    for c in &cand {
        let y = row_of.get(&c.id()).map(|&row| first_row - row as f64 * row_h);
        if c.cut {
            let top = line.axis_y - reach;
            let label = y.map(|y| CutLabel { text: c.text.clone(), x: c.lx, y, leader: (Point { x: c.cx, y: top }, Point { x: c.lx, y: y + 3.0 }) });
            cuts.push(LineCut { x: c.cx, y0: top, y1: line.axis_y + reach, label });
            continue;
        }
        let Some(y) = y else { continue }; // the stack reached the frame
        let h = flag_height(font);
        flags.push(Flag {
            feature: c.key,
            text: c.text.clone(),
            x: c.lx - c.w / 2.0,
            y,
            w: c.w,
            h,
            font,
            leader: (Point { x: c.cx, y: c.lane_top }, Point { x: c.lx, y: y + h / 2.0 }),
        });
    }
    (flags, cuts)
}
