//! Where every base sits in the wrapped sequence view. Ported from Bios `editor/rows/layout.ts`.
//!
//! One coordinate contract: every layer in a row (bases, features, primers, cut marks, rules, the
//! selection) places itself at `x = (bp - row.start) * char_width` and nowhere else.

/// Below this many pixels per base, the letters stop being drawn.
pub const HIDE_BASES_BELOW: f64 = 7.0;
/// Above this, amino acids are drawn.
pub const TRANSLATIONS_ABOVE: f64 = 4.0;
/// Below this, a primer stops printing its own bases.
pub const PRIMER_BASES_BELOW: f64 = 8.0;
/// Rows end on a whole multiple of this many bases.
pub const GROUP: i64 = 10;
/// The margin is never less than this; below it the row numbers have nowhere to go.
pub const MIN_MARGIN: f64 = 50.0;
/// The margin Bios asks for. Half falls either side as padding.
pub const DEFAULT_MARGIN: f64 = 110.0;
/// The character width Bios asks for before justification.
pub const DEFAULT_CHAR_WIDTH: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Bases in a full row. Always positive, always a multiple of [`GROUP`].
    pub bps_per_row: i64,
    /// Pixels per base after justification. Fractional on purpose.
    pub char_width: f64,
    /// Total width reserved for margins; half sits either side of the content.
    pub margin_width: f64,
    /// The drawable width. `bps_per_row * char_width` equals this.
    pub content_width: f64,
}

/// A row of the wrapped view. `start` and `end` are 0-based and inclusive; an empty row has `end = start - 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub index: usize,
    pub start: i64,
    pub end: i64,
}

/// A range clipped to one row, in that row's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub x_start: f64,
    pub width: f64,
    pub start: i64,
    pub end: i64,
    /// The range continued past this row's edge: draw a cut, not a cap.
    pub clipped_left: bool,
    pub clipped_right: bool,
}

/// The widest a base gets: the rows' own zoom stops here (`MAX_CHAR_WIDTH`).
pub const MAX_CHAR_WIDTH: f64 = 48.0;

/// The narrowest a base is asked for by the rows' zoom (`Math.max(0.4, …)`). The rows never stay
/// there: below [`HIDE_BASES_BELOW`] they hand back to the map.
pub const MIN_CHAR_WIDTH: f64 = 0.4;

/// The width a base is asked for after a zoom of `dz`, the wheel's delta (positive zooms out), at
/// the map's own rate (`exp(dz * 0.02)`, each step capped at 30). A width under the letters is
/// allowed: it is what tells the rows to hand back to the map.
pub fn zoom_char_width(char_width: f64, dz: f64) -> f64 {
    (char_width * (-dz.clamp(-30.0, 30.0) * 0.02).exp()).clamp(MIN_CHAR_WIDTH, MAX_CHAR_WIDTH)
}

/// Whether the wrapped rows, at this width a base, have been zoomed out past the letters and hand
/// back to the map (`onZoomedOut`).
pub fn rows_hand_back(char_width: f64) -> bool {
    char_width < HIDE_BASES_BELOW
}

/// How far to scroll a drag held against the rows' top or bottom edge, in pixels this frame
/// (`edgeScroll`): faster the deeper into the band, up to three steps of 16. The bottom band is the
/// deeper because the tool row floats over the foot of the rows. `y` is the pointer's distance from
/// the top of a viewport `height` tall.
pub fn edge_scroll(y: f64, height: f64) -> f64 {
    const TOP: f64 = 32.0;
    const BOTTOM: f64 = 72.0;
    let step = if y < TOP {
        -((TOP - y) / TOP).min(3.0)
    } else if y > height - BOTTOM {
        ((y - (height - BOTTOM)) / BOTTOM).min(3.0)
    } else {
        0.0
    };
    step * 16.0
}

/// How the panel divides into rows. `width` is the panel's full width including the margin.
pub fn metrics_for(width: f64, char_width: Option<f64>, margin_width: Option<f64>) -> Metrics {
    let margin_width = margin_width.unwrap_or(DEFAULT_MARGIN).max(MIN_MARGIN);
    let asked = char_width.unwrap_or(DEFAULT_CHAR_WIDTH);
    let content_width = (width - margin_width).max(0.0);
    // Floor, then snap to ten. The order matters: snapping first would let a row claim bases the panel cannot show.
    let fit = if asked > 0.0 { (content_width / asked).floor() } else { 0.0 };
    let bps_per_row = (((fit / GROUP as f64).round() as i64) * GROUP).max(GROUP);
    let char_width = if content_width > 0.0 { content_width / bps_per_row as f64 } else { asked };
    Metrics { bps_per_row, char_width, margin_width, content_width }
}

/// Every row of a construct, in order. An empty construct still gets one row, holding no bases.
pub fn rows_for(sequence_length: i64, bps_per_row: i64) -> Vec<Row> {
    if bps_per_row <= 0 {
        return Vec::new();
    }
    let total = ((sequence_length + bps_per_row - 1) / bps_per_row).max(1);
    (0..total)
        .map(|index| {
            let start = index * bps_per_row;
            let last = (index + 1) * bps_per_row - 1;
            Row { index: index as usize, start, end: if last < sequence_length { last } else { sequence_length - 1 } }
        })
        .collect()
}

/// Which row a base falls in.
pub fn row_of_base(bp: i64, bps_per_row: i64) -> i64 {
    if bps_per_row > 0 { bp.div_euclid(bps_per_row) } else { 0 }
}

/// The left edge of a base's slot, in its row's pixels. The one conversion everything else uses.
pub fn x_of_base(bp: i64, row: Row, char_width: f64) -> f64 {
    (bp - row.start) as f64 * char_width
}

/// The centre of a base's slot.
pub fn center_of_base(bp: i64, row: Row, char_width: f64) -> f64 {
    (bp - row.start) as f64 * char_width + char_width / 2.0
}

/// Where a click lands, as a caret position: the gap between two bases. It can be one past the
/// row's last base and never more.
pub fn caret_at_x(x: f64, row: Row, char_width: f64) -> i64 {
    if char_width <= 0.0 || x < 0.0 {
        return row.start;
    }
    let into = ((x + char_width / 2.0) / char_width).floor() as i64;
    let pos = into + row.start;
    if pos > row.end + 1 { row.end + 1 } else { pos }
}

/// The base under a point, or `None` past the row's end.
pub fn base_at_x(x: f64, row: Row, char_width: f64) -> Option<i64> {
    if char_width <= 0.0 || x < 0.0 {
        return None;
    }
    let bp = (x / char_width).floor() as i64 + row.start;
    if bp > row.end { None } else { Some(bp) }
}

/// A linear range clipped to one row, or `None` if it misses the row. Unwrap an origin-spanning range first.
pub fn span_in_row(start: i64, end: i64, row: Row, char_width: f64) -> Option<Span> {
    if row.end < row.start {
        return None;
    }
    let from = start.max(row.start);
    let to = end.min(row.end);
    if to < from {
        return None;
    }
    Some(Span {
        x_start: x_of_base(from, row, char_width),
        width: (to - from + 1) as f64 * char_width,
        start: from,
        end: to,
        clipped_left: start < row.start,
        clipped_right: end > row.end,
    })
}

/// A possibly origin-spanning range as one or two linear ranges.
pub fn unwrap(start: i64, end: i64, sequence_length: i64) -> Vec<(i64, i64)> {
    if sequence_length <= 0 {
        return Vec::new();
    }
    if end >= start {
        return vec![(start, end)];
    }
    vec![(start, sequence_length - 1), (0, end)]
}

/// What is legible at this zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regime {
    pub bases: bool,
    pub translations: bool,
    pub primer_bases: bool,
    pub rungs: bool,
}

pub fn regime_for(char_width: f64) -> Regime {
    Regime {
        bases: char_width >= HIDE_BASES_BELOW,
        translations: char_width > TRANSLATIONS_ABOVE,
        primer_bases: char_width >= PRIMER_BASES_BELOW,
        rungs: char_width >= HIDE_BASES_BELOW,
    }
}

/// Track heights, in pixels.
pub mod track {
    pub const SPACER: f64 = 6.0;
    pub const INTERACTIONS: f64 = 20.0;
    pub const SEQUENCE: f64 = 15.0;
    pub const REVERSE_SEQUENCE: f64 = 15.0;
    /// The weld: the channel between the strands, holding only the pairing.
    pub const RUNGS: f64 = 9.0;
    pub const FEATURES: f64 = 17.0;
    pub const FEATURES_MARGIN_BOTTOM: f64 = 5.0;
    /// Air between the bases and the first lane stacked against them. Charged once per side.
    pub const LANE_GAP: f64 = 6.0;
    /// Air outside a forward-strand stack, between it and the row above.
    pub const ABOVE_AIR: f64 = 16.0;
    pub const PARTS: f64 = 15.0;
    pub const PRIMERS: f64 = 18.0;
    pub const PRIMER_LIFT: f64 = 10.0;
    pub const CUTSITE_LABELS: f64 = 15.0;
    pub const TRANSLATIONS: f64 = 17.0;
    pub const ORFS: f64 = 15.0;
    /// Clear air below a row, so one row reads as a block.
    pub const ROW_GAP: f64 = 22.0;
}

/// The stacked tracks of one side of the duplex, and how many lanes each needed in this row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StrandLanes {
    pub features: u32,
    pub parts: u32,
    pub primers: u32,
    /// Pixels of outward room the primer lanes need for what hangs off them.
    pub primer_lift: f64,
    pub translations: u32,
    pub orfs: u32,
}

/// Lanes this row needed. Forward-strand annotations stack upward off the top strand and
/// reverse-strand ones downward off the bottom, so position says which strand a thing is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lanes {
    pub interactions: u32,
    pub cutsite_labels: u32,
    pub above: StrandLanes,
    pub below: StrandLanes,
}

/// Which optional tracks are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tracks {
    /// Both strands, welded. Off, the row is one strand and the weld closes with it.
    pub reverse_sequence: bool,
    /// The distance rules through the duplex.
    pub axis: bool,
}

impl Default for Tracks {
    fn default() -> Self {
        Tracks { reverse_sequence: true, axis: true }
    }
}

/// The outer air a forward-strand stack needs to stay attached to its own bases. 0 when empty.
pub fn above_air(lanes: StrandLanes) -> f64 {
    if lane_block_height(lanes) > 0.0 { track::ABOVE_AIR } else { 0.0 }
}

/// How tall one side's stack is, margins included. 0 when that side stacks nothing.
pub fn lane_block_height(lanes: StrandLanes) -> f64 {
    let mut h = 0.0;
    if lanes.features > 0 {
        h += f64::from(lanes.features) * track::FEATURES + track::FEATURES_MARGIN_BOTTOM;
    }
    if lanes.primers > 0 {
        h += f64::from(lanes.primers) * track::PRIMERS + lanes.primer_lift;
    }
    if lanes.parts > 0 {
        h += f64::from(lanes.parts) * track::PARTS;
    }
    if lanes.orfs > 0 {
        h += f64::from(lanes.orfs) * track::ORFS;
    }
    if lanes.translations > 0 {
        h += f64::from(lanes.translations) * track::TRANSLATIONS;
    }
    if h > 0.0 { h + track::LANE_GAP } else { 0.0 }
}

/// Offset of the DNA band after the annotation lanes above it. Shared by drawing and scrolling.
pub fn duplex_top(lanes: Lanes) -> f64 {
    f64::from(lanes.interactions) * track::INTERACTIONS
        + f64::from(lanes.cutsite_labels) * track::CUTSITE_LABELS
        + above_air(lanes.above)
        + lane_block_height(lanes.above)
}

/// How tall one row is. Height is per row because it depends on how many lanes that row needed.
pub fn row_height(lanes: Lanes, tracks: Tracks) -> f64 {
    let mut h = track::SPACER + track::SEQUENCE;
    if tracks.reverse_sequence {
        h += track::RUNGS + track::REVERSE_SEQUENCE;
    }
    h += duplex_top(lanes);
    h += lane_block_height(lanes.below);
    h + track::ROW_GAP
}

/// Where each band sits inside a row with nothing stacked above it, measured down from the row's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bands {
    /// Baseline for the forward strand's letters.
    pub forward: f64,
    /// Top of the weld. Rungs start here.
    pub rung_top: f64,
    /// Bottom of the weld. Rungs end here.
    pub rung_bottom: f64,
    /// The middle of the weld: the molecule's own centre line.
    pub duplex: f64,
    /// Baseline for the reverse strand's letters.
    pub reverse: f64,
    /// The bottom of the bases.
    pub after_strands: f64,
    /// The top of the first lane stacked below: `after_strands` plus the lane gap.
    pub lanes_top: f64,
    /// Total height for a row with nothing stacked.
    pub height: f64,
}

/// How far below a track's top its letters' baseline falls.
fn baseline(h: f64) -> f64 {
    h - h / 4.0
}

pub fn bands_for(tracks: Tracks) -> Bands {
    let show_reverse = tracks.reverse_sequence;
    let mut y = track::SPACER;
    let forward = y + baseline(track::SEQUENCE);
    y += track::SEQUENCE;
    let rung_top = y;
    if show_reverse {
        y += track::RUNGS;
    }
    let rung_bottom = y;
    let reverse = if show_reverse { y + baseline(track::REVERSE_SEQUENCE) } else { y };
    if show_reverse {
        y += track::REVERSE_SEQUENCE;
    }
    Bands {
        forward,
        rung_top,
        rung_bottom,
        duplex: (rung_top + rung_bottom) / 2.0,
        reverse,
        after_strands: y,
        lanes_top: y + track::LANE_GAP,
        height: y + track::ROW_GAP,
    }
}

/// The horizontal zebra: every other row on a faint band, margin to margin, as `(x, y, width,
/// height)` in the row's own pixels. Its edges fall in the middle of the gap between rows, not at a
/// row's bottom: `row_height` carries the gap below the row, so the band is shifted up by half of
/// it. Bios `editor/rows/SequenceRows.tsx`, the `.seq-row-band` rect (lines 1335-1350).
pub fn band_for(row: Row, row_height: f64, metrics: &Metrics) -> Option<(f64, f64, f64, f64)> {
    (row.index % 2 == 1).then(|| (-metrics.margin_width / 2.0, -track::ROW_GAP / 2.0, metrics.content_width + metrics.margin_width, row_height))
}

/// One distance rule: a vertical line through the duplex on a base's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub bp: i64,
    pub x: f64,
    /// Line width in pixels: 1 for a fifth or a tenth, 2 for a fiftieth.
    pub w: f64,
    /// Strength relative to the tenth: under 1 is a fifth, over 1 a fiftieth.
    pub k: f64,
}

/// The levels as `(every, width, strength)`, heaviest last. The last one that claims a base wins.
pub const RULE_LEVELS: [(i64, f64, f64); 3] = [(5, 1.0, 0.45), (10, 1.0, 1.0), (50, 2.0, 1.6)];

pub fn rules_for(row: Row, char_width: f64) -> Vec<Rule> {
    let mut out = Vec::new();
    for bp in row.start..=row.end {
        // 1-based for the reader: the rule falls before base 10 of the sequence, not before base 9.
        let n = bp + 1;
        let Some(&(_, w, k)) = RULE_LEVELS.iter().rev().find(|(every, _, _)| n % every == 0) else {
            continue;
        };
        out.push(Rule { bp, x: x_of_base(bp, row, char_width), w, k });
    }
    out
}

/// The vertical extents a cut mark spans in a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CutBands {
    pub top_y0: f64,
    pub mid_y: f64,
    pub bot_y1: f64,
}

pub fn cut_bands_for(tracks: Tracks) -> CutBands {
    let b = bands_for(tracks);
    CutBands { top_y0: track::SPACER, mid_y: b.duplex, bot_y1: b.after_strands }
}

/// The default gap `pack_lanes` leaves between two items in one lane.
pub const PACK_GAP: f64 = 6.0;

/// Assigns items, given as `(x, width)`, to lanes so none overlaps another in the same lane.
/// Returns a lane index per item in the order given, and the number of lanes used.
pub fn pack_lanes(items: &[(f64, f64)], gap: f64) -> (Vec<usize>, usize) {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| items[a].0.total_cmp(&items[b].0).then(a.cmp(&b)));
    let mut last_end: Vec<f64> = Vec::new();
    let mut lanes = vec![0; items.len()];
    for i in order {
        let (x, width) = items[i];
        let lane = match last_end.iter().position(|&end| end <= x) {
            Some(lane) => lane,
            None => {
                last_end.push(0.0);
                last_end.len() - 1
            }
        };
        last_end[lane] = x + width + gap;
        lanes[i] = lane;
    }
    (lanes, last_end.len())
}

/// Where a row starts, for every row. `tops[i]` is row i's top and `tops[rows.len()]` the total height.
#[derive(Clone, Debug, PartialEq)]
pub struct RowPlan {
    pub tops: Vec<f64>,
    pub height: f64,
}

pub fn plan_rows(rows: &[Row], lanes_for: impl Fn(Row) -> Lanes, tracks: Tracks) -> RowPlan {
    let mut tops = Vec::with_capacity(rows.len() + 1);
    let mut y = 0.0;
    for &row in rows {
        tops.push(y);
        y += row_height(lanes_for(row), tracks);
    }
    tops.push(y);
    RowPlan { tops, height: y }
}

/// The index of the last row starting at or before `y`.
pub fn row_at(plan: &RowPlan, y: f64) -> usize {
    if plan.tops.len() < 2 {
        return 0;
    }
    let mut lo = 0;
    let mut hi = plan.tops.len() - 2;
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if plan.tops[mid] <= y {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}
