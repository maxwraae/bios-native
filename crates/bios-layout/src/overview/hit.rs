//! What is under a point on the map: which gap between two bases, and which feature. The pure
//! half of a press and a drag, as Bios `editor/overview/PlasmidOverview.tsx` answers them:
//! `caretBpAt` for the gap, the transparent shapes `renderRing` and `renderLine` lay over every
//! arc, bar and flag for the feature, and `beginDrag` for where a shift-press extends from.
//!
//! Points are in the frame's own pixels, measured from its top left, as the layouts place things.

use super::line_map::LineMap;
use super::projection::{Point, line_window, point_to_frac_line, point_to_frac_ring};
use super::ring_map::RingMap;
use super::scene::{Flag, Frame};
use super::viewport::{Projection, Viewport, js_round, wrap1};

/// An arc is caught over at least this many pixels across its lane, so that a thin inner lane is
/// still catchable.
pub const MIN_ARC_REACH: f64 = 14.0;

/// What of a feature a point is on. The index is into `MapScene::features`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapHit {
    /// One of its arcs or bars. A name written on the shape is part of the shape.
    Shape(usize),
    /// The chip its name was lifted onto. The leader between the two catches nothing.
    Flag(usize),
}

impl MapHit {
    pub fn feature(self) -> usize {
        match self {
            MapHit::Shape(feature) | MapHit::Flag(feature) => feature,
        }
    }
}

/// The gap nearest a fraction of the molecule: 0 is before the first base and `length` is after
/// the last.
pub fn gap_of(frac: f64, length: i64) -> i64 {
    if length <= 0 {
        return 0;
    }
    (js_round(frac * length as f64) as i64).clamp(0, length)
}

/// The gap a point on the ring reads, by its angle alone: anywhere along a spoke is the same gap,
/// the empty middle included. Just anticlockwise of the origin reads `length` and just clockwise
/// of it reads 0, which are the same place.
pub fn ring_gap_at(map: &RingMap, length: i64, at: Point) -> i64 {
    gap_of(point_to_frac_ring(at.x, at.y, map.center, map.geom), length)
}

/// The gap a point on the line reads, by its x alone, held at the two ends of the molecule.
/// `frame` is the one `map` was laid out in.
pub fn line_gap_at(map: &LineMap, frame: Frame, length: i64, x: f64) -> i64 {
    let window = line_window(Viewport { center: frame.center, width: frame.width, projection: Projection::Line });
    gap_of(point_to_frac_line(x, window.left, frame.width, frame.w, map.left), length)
}

/// The feature a point on the ring is on. What is drawn last is on top, so a flag wins over an
/// arc and a later feature over an earlier one. A dashed connector catches nothing.
pub fn ring_feature_at(map: &RingMap, at: Point) -> Option<MapHit> {
    if let Some(feature) = flag_at(&map.flags, at) {
        return Some(MapHit::Flag(feature));
    }
    let radius = (at.x - map.geom.cx).hypot(at.y - map.geom.cyc);
    let frac = point_to_frac_ring(at.x, at.y, map.center, map.geom);
    let reach = map.thickness.max(MIN_ARC_REACH) / 2.0;
    map.arcs
        .iter()
        .rev()
        .find(|arc| {
            !arc.connector && (radius - arc.radius).abs() <= reach && (arc.width_frac >= 1.0 || wrap1(frac - arc.start_frac) <= arc.width_frac)
        })
        .map(|arc| MapHit::Shape(arc.feature))
}

/// The feature a point on the line is on, by the same rule as [`ring_feature_at`]. A bar is
/// caught over exactly what is drawn of it.
pub fn line_feature_at(map: &LineMap, at: Point) -> Option<MapHit> {
    if let Some(feature) = flag_at(&map.flags, at) {
        return Some(MapHit::Flag(feature));
    }
    let half = map.thickness / 2.0;
    map.bars
        .iter()
        .rev()
        .find(|bar| !bar.connector && at.x >= bar.x0 && at.x <= bar.x1 && (at.y - bar.y).abs() <= half)
        .map(|bar| MapHit::Shape(bar.feature))
}

fn flag_at(flags: &[Flag], at: Point) -> Option<usize> {
    flags.iter().rev().find(|f| at.x >= f.x && at.x <= f.x + f.w && (at.y - f.y).abs() <= f.h / 2.0).map(|f| f.feature)
}

/// Where a shift-press at `gap` on a ring extends the selection to, as an unwrapped position for
/// `drag_selection`: `gap` itself, or the same place one turn back or on, whichever is nearest
/// `anchor`. A press cannot say which way round it means, so it means the short way. Exactly half
/// way round goes anticlockwise.
pub fn ring_extend_to(anchor: i64, gap: i64, length: i64) -> i64 {
    let mut nearest = gap.saturating_sub(length);
    for turned in [gap, gap.saturating_add(length)] {
        if turned.abs_diff(anchor) < nearest.abs_diff(anchor) {
            nearest = turned;
        }
    }
    nearest
}
