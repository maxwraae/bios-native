//! The coordinate contract of the edit code, and the function that encodes it.
//!
//! Three conventions meet in the edit code and must not be conflated:
//!   - A [`Feature`] is 0-based and inclusive, and `end < start` means it spans the origin of a
//!     circular construct.
//!   - A [`Part`] is 0-based and half-open. An origin-spanning feature is two parts.
//!   - The public functions of [`crate::edits`] take 1-based inclusive positions. That conversion
//!     lives only there.
//!
//! On a simple span `end < start` means "spans the origin". It is never a collapsed range or a
//! tombstone: a feature a deletion consumed is reported as removed, not re-encoded. An earlier
//! version reused `end < start` for that, which silently deleted every origin-spanning feature on
//! the first ordinary edit.

use crate::edits::EditError;
use crate::model::Feature;

/// Whether a feature is a part: a span marked as a reusable piece rather than an annotation,
/// carried in GenBank as `/pragma="Teselagen_Part"` (the convention the web app's GenBank reader
/// follows, and writes back). The web shows parts in their own lane of the rows, never among the
/// features.
pub fn is_part(f: &Feature) -> bool {
    f.note("pragma").is_some_and(|values| values.iter().any(|v| v.trim() == "Teselagen_Part"))
}

/// A 0-based half-open `[start, end)` range.
pub type Part = (usize, usize);

/// Feature to half-open parts. An origin-spanning feature (`end < start`) becomes two parts.
pub fn feature_to_parts(f: &Feature, seq_len: usize, circular: bool) -> Result<Vec<Part>, EditError> {
    let Some(locations) = f.locations() else {
        return Ok(span_to_parts(f.start, f.end, seq_len, circular));
    };
    if locations.iter().any(|p| p.start >= seq_len || p.end >= seq_len || (!circular && p.end < p.start)) {
        return Err(EditError::InvalidSegments { name: f.name.clone() });
    }
    Ok(locations.iter().flat_map(|p| span_to_parts(p.start, p.end, seq_len, circular)).collect())
}

/// One inclusive span to half-open parts: two when it runs through the origin.
pub(crate) fn span_to_parts(start: usize, end: usize, seq_len: usize, circular: bool) -> Vec<Part> {
    // Saturating only so that a hostile `usize::MAX` cannot overflow.
    let past_end = end.saturating_add(1);
    if circular && end < start { vec![(start, seq_len), (0, past_end)] } else { vec![(start, past_end)] }
}

/// Removes `[ws, we)` from every piece, keeping what is left of each in order.
pub(crate) fn cut(pieces: &[Part], ws: usize, we: usize) -> Vec<Part> {
    let mut kept = Vec::with_capacity(pieces.len() + 1);
    for &(s, e) in pieces {
        if e <= ws || s >= we {
            kept.push((s, e));
            continue;
        }
        if s < ws {
            kept.push((s, ws));
        }
        if e > we {
            kept.push((we, e));
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::{cut, span_to_parts};

    #[test]
    fn an_origin_span_is_two_parts_only_on_a_circle() {
        assert_eq!(span_to_parts(8, 2, 10, true), vec![(8, 10), (0, 3)]);
        assert_eq!(span_to_parts(2, 8, 10, true), vec![(2, 9)]);
        assert_eq!(span_to_parts(8, 2, 10, false), vec![(8, 3)]);
    }

    #[test]
    fn a_cut_inside_a_piece_leaves_both_flanks() {
        assert_eq!(cut(&[(0, 10)], 3, 5), vec![(0, 3), (5, 10)]);
        assert_eq!(cut(&[(0, 10)], 0, 10), vec![]);
        assert_eq!(cut(&[(0, 3), (6, 9)], 3, 6), vec![(0, 3), (6, 9)]);
    }
}
