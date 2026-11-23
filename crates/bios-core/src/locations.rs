//! One representation of a feature's location, however it was written in the file.

use crate::edits::EditError;
use crate::model::{Feature, LocationOperator, Segment};

/// The feature's pieces as every part of Bios should see them, or `None` when it is one
/// contiguous interval.
///
/// A piece that runs through the origin is split in two. A plain two-arm join through the origin
/// is how a file writes one contiguous wrapping feature, so it comes back as `None`.
pub fn normalized_feature_locations(feature: &Feature, length: usize, circular: bool) -> Option<Vec<Segment>> {
    let wrapping;
    let source: &[Segment] = match feature.locations() {
        Some(locations) => locations,
        None if circular && feature.end < feature.start && (feature.start_fuzzy.is_some() || feature.end_fuzzy.is_some()) => {
            wrapping = [Segment { start: feature.start, end: feature.end, start_fuzzy: feature.start_fuzzy, end_fuzzy: feature.end_fuzzy }];
            &wrapping
        }
        None => return None,
    };
    let mut parts: Vec<Segment> = Vec::with_capacity(source.len() + 1);
    for p in source {
        if circular && length > 0 && p.end < p.start {
            parts.push(Segment { start: p.start, end: length - 1, start_fuzzy: p.start_fuzzy, end_fuzzy: None });
            parts.push(Segment { start: 0, end: p.end, start_fuzzy: None, end_fuzzy: p.end_fuzzy });
        } else {
            parts.push(*p);
        }
    }
    if parts.len() < 2 {
        return None;
    }
    if let (Some(fuzzy), Some(first)) = (feature.start_fuzzy, parts.first_mut()) {
        first.start_fuzzy.get_or_insert(fuzzy);
    }
    if let (Some(fuzzy), Some(last)) = (feature.end_fuzzy, parts.last_mut()) {
        last.end_fuzzy.get_or_insert(fuzzy);
    }
    if let [head, tail] = parts.as_slice()
        && circular
        && feature.location_operator != Some(LocationOperator::Order)
        && feature.end < feature.start
        && head.start == feature.start
        && Some(head.end) == length.checked_sub(1)
        && tail.start == 0
        && tail.end == feature.end
        && !parts.iter().any(|p| p.start_fuzzy.is_some() || p.end_fuzzy.is_some())
    {
        return None;
    }
    Some(parts)
}

/// Puts a feature's location into the one shape the rest of Bios expects, after reading it or
/// after moving it: a plain join through the origin is a single wrapping span, a piece that runs
/// through the origin is two pieces, and a single span carries its uncertain ends on the feature.
pub fn normalize_location(feature: &mut Feature, length: usize, circular: bool) {
    match normalized_feature_locations(feature, length, circular) {
        Some(pieces) => {
            feature.segments = pieces;
            feature.start_fuzzy = None;
            feature.end_fuzzy = None;
        }
        None => {
            if let [only] = feature.segments.as_slice() {
                feature.start_fuzzy = feature.start_fuzzy.or(only.start_fuzzy);
                feature.end_fuzzy = feature.end_fuzzy.or(only.end_fuzzy);
            }
            feature.segments = vec![Segment::new(feature.start, feature.end)];
        }
    }
}

/// Moves an annotation to a new circular origin without changing its biological order. Every
/// coordinate moves `offset` bases to the right, wrapping at `length`.
pub fn rotate_feature_location(feature: &Feature, length: usize, offset: i64) -> Result<Feature, EditError> {
    if offset == 0 {
        return Ok(feature.clone());
    }
    if unsupported_location(feature).is_some() {
        return Err(EditError::RotateUnsupported);
    }
    // A circle of no bases has nowhere to move a coordinate to, and the remainder below would
    // divide by zero.
    if length == 0 {
        return Ok(feature.clone());
    }
    let at = |n: usize| usize::try_from((n as i128 + i128::from(offset)).rem_euclid(length as i128)).unwrap_or(n);
    let mut moved = feature.clone();
    moved.start = at(feature.start);
    moved.end = at(feature.end);
    if feature.locations().is_some() {
        for p in &mut moved.segments {
            p.start = at(p.start);
            p.end = at(p.end);
        }
    }
    // An `order` is the only operator that says something a plain list of pieces does not.
    moved.location_operator = moved.location_operator.filter(|op| *op == LocationOperator::Order);
    match normalized_feature_locations(&moved, length, true) {
        Some(locations) => {
            moved.segments = locations;
            moved.start_fuzzy = None;
            moved.end_fuzzy = None;
        }
        None => moved.segments = vec![simple_segment(&moved)],
    }
    Ok(moved)
}

/// The one segment a simple feature carries: its own span and uncertain ends.
pub(crate) fn simple_segment(f: &Feature) -> Segment {
    Segment { start: f.start, end: f.end, start_fuzzy: f.start_fuzzy, end_fuzzy: f.end_fuzzy }
}

/// The location text Bios could not interpret, if the feature has one.
pub(crate) fn unsupported_location(f: &Feature) -> Option<&str> {
    f.location_unsupported.as_deref().filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{normalized_feature_locations, rotate_feature_location};
    use crate::edits::EditError;
    use crate::model::{Feature, Fuzzy, LocationOperator, Segment, Strand};

    fn feature(start: usize, end: usize, segments: &[(usize, usize)]) -> Feature {
        Feature {
            name: "f".to_string(),
            kind: "misc_feature".to_string(),
            start,
            end,
            strand: Strand::Forward,
            segments: segments.iter().map(|&(s, e)| Segment::new(s, e)).collect(),
            notes: Vec::new(),
            start_fuzzy: None,
            end_fuzzy: None,
            location_operator: None,
            location_unsupported: None,
        }
    }

    #[test]
    fn a_two_arm_origin_join_is_one_interval_unless_it_is_an_order() {
        let mut f = feature(8, 2, &[(8, 9), (0, 2)]);
        assert_eq!(normalized_feature_locations(&f, 10, true), None);
        assert_eq!(normalized_feature_locations(&f, 10, false), Some(f.segments.clone()));
        f.location_operator = Some(LocationOperator::Order);
        assert_eq!(normalized_feature_locations(&f, 10, true), Some(f.segments.clone()));
    }

    #[test]
    fn an_uncertain_wrapping_span_keeps_its_markers_on_two_pieces() {
        let mut f = feature(8, 2, &[(8, 2)]);
        f.start_fuzzy = Some(Fuzzy::Before);
        let expected = vec![Segment { start: 8, end: 9, start_fuzzy: Some(Fuzzy::Before), end_fuzzy: None }, Segment::new(0, 2)];
        assert_eq!(normalized_feature_locations(&f, 10, true), Some(expected));
        assert_eq!(normalized_feature_locations(&f, 10, false), None);
    }

    #[test]
    fn rotating_splits_a_piece_that_lands_on_the_origin() {
        let f = feature(1, 8, &[(1, 3), (6, 8)]);
        let moved = rotate_feature_location(&f, 10, 3).unwrap();
        assert_eq!((moved.start, moved.end), (4, 1));
        assert_eq!(moved.segments, vec![Segment::new(4, 6), Segment::new(9, 9), Segment::new(0, 1)]);
        assert_eq!(rotate_feature_location(&moved, 10, -3).unwrap().segments, vec![Segment::new(1, 3), Segment::new(6, 6), Segment::new(7, 8)]);
    }

    #[test]
    fn rotating_a_simple_span_keeps_one_segment() {
        let moved = rotate_feature_location(&feature(7, 9, &[(7, 9)]), 10, 2).unwrap();
        assert_eq!((moved.start, moved.end), (9, 1));
        assert_eq!(moved.segments, vec![Segment::new(9, 1)]);
        assert_eq!(rotate_feature_location(&feature(7, 9, &[(7, 9)]), 0, 2).unwrap(), feature(7, 9, &[(7, 9)]));
    }

    #[test]
    fn an_unsupported_location_is_not_rotated() {
        let mut f = feature(1, 3, &[(1, 3)]);
        f.location_unsupported = Some("join(2..4,ACC.1:12..15)".to_string());
        assert_eq!(rotate_feature_location(&f, 10, 0), Ok(f.clone()));
        assert_eq!(rotate_feature_location(&f, 10, 1), Err(EditError::RotateUnsupported));
    }
}
