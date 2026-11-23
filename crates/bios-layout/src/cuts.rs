//! Restriction cuts in the wrapped rows. Ported from Bios `editor/rows/cutGeometry.ts`, with the
//! row bucketing and the staircase of `editor/rows/RowCutMarks.tsx` and the overhang range of
//! `editor/rows/cutHover.ts`, which the same tests cover.
//!
//! A cut arrives as plain numbers, so this crate needs nothing above it: the caller reads them off
//! `bios_core::enzymes::CutSite`. Both scissions are GAP coordinates, the space before base N,
//! which is exactly where `x_of_base` puts them. The tests need the enzyme table, so they live in
//! `crates/bios-ui/tests/cut_geometry.rs`, the lowest crate that sees both.

use crate::rows::{CutBands, PACK_GAP, Row, pack_lanes, track, x_of_base};

/// One double-strand cleavage, as the rows need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    /// Gap coordinate of the forward-strand scission.
    pub top_cut: i64,
    /// Gap coordinate of the reverse-strand scission.
    pub bottom_cut: i64,
    /// The overhang's length, signed: positive for a 5' overhang, negative for a 3', 0 when blunt.
    pub overhang: i64,
    /// 0-based, plus strand: leftmost base of the recognition match.
    pub recognition_start: i64,
    /// How many bases the enzyme reads.
    pub site_length: i64,
    /// The scissions as offsets from `recognition_start`, before wrapping, when the table has them.
    pub offsets: Option<(i64, i64)>,
}

/// One copy of a cut's staircase, in unwrapped gap coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Projection {
    pub top: i64,
    pub bottom: i64,
}

/// Copies of the short cut staircase that intersect the displayed molecule.
pub fn cut_projections(c: &Cut, length: i64, circular: bool) -> Vec<Projection> {
    if !circular {
        return vec![Projection { top: c.top_cut, bottom: c.bottom_cut }];
    }
    [length.saturating_neg(), 0, length]
        .into_iter()
        .map(|shift| {
            let top = c.top_cut.saturating_add(shift);
            Projection { top, bottom: top.saturating_add(c.overhang) }
        })
        .filter(|p| p.top.max(p.bottom) >= 0 && p.top.min(p.bottom) < length)
        .collect()
}

/// Recognition bases on each side of the origin, as inclusive `(from, to)` ranges; `site_to` may
/// be unwrapped or normalized.
pub fn recognition_pieces(site_from: i64, site_to: i64, length: i64, circular: bool) -> Vec<(i64, i64)> {
    if length <= 0 || site_from < 0 || site_from >= length || site_to < 0 {
        return Vec::new();
    }
    let end = if site_to < site_from && circular { site_to.saturating_add(length) } else { site_to };
    if end < site_from || end - site_from >= length || (!circular && end >= length) {
        return Vec::new();
    }
    if end < length { vec![(site_from, end)] } else { vec![(site_from, length - 1), (0, end - length)] }
}

/// One staircase piece in one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowCut {
    /// Which of the cuts handed to [`cuts_for_row`] this is.
    pub cut: usize,
    pub top_cut: i64,
    pub bottom_cut: i64,
    /// 0-based, inclusive: the bases the enzyme READS, which is not always where it cuts.
    pub site_from: i64,
    pub site_to: i64,
    /// A type IIS enzyme cuts outside the site it reads, so a caret inside it would be a lie.
    pub cuts_outside: bool,
    /// Which lane this cut's name sits in, from the row's own packing. See [`pack_names`].
    pub lane: usize,
    pub draw_top: i64,
    pub draw_bottom: i64,
    /// The gap the name and the pointer target stand on: the top scission, kept inside the row.
    pub anchor: i64,
    pub overhang_delta: i64,
    /// The molecule's length when it is circular.
    pub sequence_length: Option<i64>,
}

/// Short staircase pieces intersecting this row, including circular-origin continuations. Lanes
/// are all 0 until [`pack_names`] has run.
pub fn cuts_for_row(cuts: &[Cut], row: Row, length: i64, circular: bool) -> Vec<RowCut> {
    let mut out = Vec::new();
    // Coincident cleavage pairs share a staircase.
    let mut seen: Vec<(i64, i64)> = Vec::new();
    for (index, c) in cuts.iter().enumerate() {
        for p in cut_projections(c, length, circular) {
            let lo = p.top.min(p.bottom);
            let hi = p.top.max(p.bottom);
            let past = row.end.saturating_add(1);
            if hi < row.start || lo > past || (lo == past && lo != length) {
                continue;
            }
            if seen.contains(&(p.top, p.bottom)) {
                continue;
            }
            seen.push((p.top, p.bottom));
            let site_from = c.recognition_start;
            out.push(RowCut {
                cut: index,
                top_cut: c.top_cut,
                bottom_cut: c.bottom_cut,
                site_from,
                site_to: site_from.saturating_add(c.site_length.max(1) - 1),
                cuts_outside: c.offsets.is_some_and(|(top, bottom)| top.max(bottom) < 0 || top.min(bottom) > c.site_length),
                lane: 0,
                draw_top: p.top,
                draw_bottom: p.bottom,
                anchor: row.start.max(past.min(p.top)),
                overhang_delta: p.bottom.saturating_sub(p.top),
                sequence_length: circular.then_some(length),
            });
        }
    }
    out
}

/// The rows a cut draws in, by the rows its staircase copies reach. Ascending, each once.
pub fn rows_of_cut(c: &Cut, length: i64, circular: bool, bps_per_row: i64, row_count: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    if bps_per_row <= 0 || row_count == 0 {
        return out;
    }
    let last_row = row_count - 1;
    let row_of = |gap: i64| usize::try_from(gap.div_euclid(bps_per_row)).map_or(last_row, |row| row.min(last_row));
    for p in cut_projections(c, length, circular) {
        let lo = p.top.min(p.bottom).max(0);
        let hi = p.top.max(p.bottom).min(length).max(0);
        for row in row_of(lo)..=row_of(hi) {
            if !out.contains(&row) {
                out.push(row);
            }
        }
    }
    out.sort_unstable();
    out
}

/// Roughly how wide a name prints, so lanes can be packed without measuring text.
pub const LABEL_CHAR_PX: f64 = 6.2;

pub fn label_width(name: &str) -> f64 {
    name.encode_utf16().count() as f64 * LABEL_CHAR_PX
}

/// Packs the names of a row's cuts into lanes, lane 0 closest to the bases, and returns how many
/// lanes the row needs. `name` is the enzyme a cut belongs to.
pub fn pack_names<'a>(cuts: &mut [RowCut], row: Row, char_width: f64, name: impl Fn(&RowCut) -> &'a str) -> usize {
    let items: Vec<(f64, f64)> = cuts.iter().map(|c| (x_of_base(c.anchor, row, char_width), label_width(name(c)))).collect();
    let (lanes, count) = pack_lanes(&items, PACK_GAP);
    for (cut, lane) in cuts.iter_mut().zip(lanes) {
        cut.lane = lane;
    }
    count
}

/// The baseline of a cut's name, which stands at [`Staircase::anchor_x`] and reads rightward.
/// `offset_y` is the duplex's top in the row.
pub fn name_baseline(lane: usize, offset_y: f64) -> f64 {
    offset_y - track::CUTSITE_LABELS * lane as f64 - 4.0
}

/// A cut's mark in one row: down through the forward strand at the top scission, across the
/// overhang, down through the reverse strand at the bottom scission. A blunt cutter's two
/// scissions coincide and it is one straight line.
#[derive(Clone, Debug, PartialEq)]
pub struct Staircase {
    /// The line's corners in order, in the row's pixels.
    pub points: Vec<(f64, f64)>,
    pub anchor_x: f64,
}

/// `offset_y` is the duplex's top in the row; `bands` is `cut_bands_for` of the row's tracks.
pub fn staircase(c: &RowCut, row: Row, char_width: f64, bands: CutBands, offset_y: f64) -> Staircase {
    let past = row.end.saturating_add(1);
    // A scission can fall past this row's edge on a cut that straddles the wrap; the across-segment
    // stays inside the row rather than running off into the margin.
    let clip_x = |gap: i64| x_of_base(gap, row, char_width).min(x_of_base(past, row, char_width)).max(0.0);
    let here = |gap: i64| gap >= row.start && gap <= past;
    let top_x = clip_x(c.draw_top);
    let bottom_x = clip_x(c.draw_bottom);
    let (y0, mid, y1) = (offset_y + bands.top_y0, offset_y + bands.mid_y, offset_y + bands.bot_y1);
    let mut points = Vec::with_capacity(4);
    if here(c.draw_top) {
        points.push((top_x, y0));
    }
    points.push((top_x, mid));
    points.push((bottom_x, mid));
    if here(c.draw_bottom) {
        points.push((bottom_x, y1));
    }
    Staircase { points, anchor_x: clip_x(c.anchor) }
}

/// The bases whose pairing a cut destroys, as a 0-based inclusive range. `None` when blunt.
///
/// Circular gaps need the signed overhang width: normalized endpoints alone cannot distinguish a
/// short origin-crossing overhang from its long complement. A wrapped range has `from > to`.
pub fn overhang_of(top: i64, bottom: i64, overhang_delta: Option<i64>, sequence_length: Option<i64>) -> Option<(i64, i64)> {
    if let (Some(n), Some(delta)) = (sequence_length.filter(|&n| n > 0), overhang_delta) {
        if delta == 0 {
            return None;
        }
        if delta.saturating_abs() >= n {
            return Some((0, n - 1));
        }
        let other = top.saturating_add(delta);
        return Some((top.min(other).rem_euclid(n), top.max(other).saturating_sub(1).rem_euclid(n)));
    }
    let from = top.min(bottom);
    let to = top.max(bottom).saturating_sub(1);
    if to < from { None } else { Some((from, to)) }
}
