//! The pieces a feature is drawn as. Ported from Bios `editor/featureGeometry.ts`.
//!
//! A feature is one span, or two when it runs through the origin, or several when the file wrote a
//! joined location. Every view asks here what to draw, so the map, its labels and its lanes agree
//! about which bases a feature covers and which stretch between two segments is only a connector.

use super::labels::{Span, feature_drawing_pieces as drawing_bases, feature_pieces as piece_bases};

/// One piece of a feature as it is drawn, in fractions of the molecule and in bases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawPiece {
    pub start_frac: f64,
    pub width_frac: f64,
    /// 0-based and inclusive.
    pub start: i64,
    pub end: i64,
    /// The one piece that carries the feature's direction mark.
    pub show_head: bool,
    /// A dashed, non-annotated gap between two segments of a joined location.
    pub connector: bool,
}

fn piece(start: i64, end: i64, length: i64, connector: bool) -> DrawPiece {
    let len = length as f64;
    DrawPiece { start_frac: start as f64 / len, width_frac: (end - start + 1) as f64 / len, start, end, show_head: false, connector }
}

/// The bases actually annotated, in stored segment order. Gaps never become filled bars.
pub fn feature_pieces<T: Span>(feature: &T, length: i64) -> Vec<DrawPiece> {
    let mut pieces: Vec<DrawPiece> = piece_bases(feature, length).into_iter().map(|(start, end)| piece(start, end, length, false)).collect();
    let head = if feature.forward() == Some(false) { pieces.last_mut() } else { pieces.first_mut() };
    if let Some(head) = head {
        head.show_head = true;
    }
    pieces
}

/// Dashed, non-annotated gaps connecting consecutive segments, including the circular seam, then
/// the pieces themselves.
pub fn feature_drawing_pieces<T: Span>(feature: &T, length: i64) -> Vec<DrawPiece> {
    let pieces = feature_pieces(feature, length);
    let all = drawing_bases(feature, length);
    let gaps = all.len().saturating_sub(pieces.len());
    let mut out: Vec<DrawPiece> = all.into_iter().take(gaps).map(|(start, end)| piece(start, end, length, true)).collect();
    out.extend(pieces);
    out
}

/// A label or leader must point to a covered base, never the midpoint of an intron.
pub fn feature_anchor<T: Span>(feature: &T, length: i64) -> f64 {
    let mut largest: Option<DrawPiece> = None;
    for p in feature_pieces(feature, length) {
        if largest.is_none_or(|best| p.width_frac > best.width_frac) {
            largest = Some(p);
        }
    }
    largest.map_or(0.0, |p| p.start_frac + p.width_frac / 2.0)
}

pub fn feature_contains<T: Span>(feature: &T, base: i64, length: i64) -> bool {
    feature_pieces(feature, length).iter().any(|p| p.start <= base && p.end >= base)
}
