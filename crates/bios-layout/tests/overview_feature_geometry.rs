//! Parity with Bios `editor/featureGeometry.test.ts`: the tests there that are about the map's
//! pieces, in the same order, with the same assertions and the same numbers.
//!
//! Left out, because what they test is not in this crate: "row geometry keeps joined segments and
//! gaps on one lane" (the sequence rows), "opening an intron does not dim the flanking annotation"
//! (`featureOverlapFraction` for joined locations) and "an unverified segmented primer keeps its
//! footprint visible" (the primer track).

use bios_layout::overview::feature_geometry::{feature_anchor, feature_contains, feature_drawing_pieces, feature_pieces};
use bios_layout::overview::labels::{PlainSpan, assign_lanes, coalesce_features};

fn located(start: i64, end: i64, locations: Option<&[(i64, i64)]>) -> PlainSpan {
    PlainSpan { start, end, locations: locations.map(<[(i64, i64)]>::to_vec), ..PlainSpan::default() }
}

fn joined() -> PlainSpan {
    PlainSpan { forward: Some(true), ..located(10, 59, Some(&[(10, 19), (40, 59)])) }
}

/// `spans(f)`: the annotated pieces. `spans(f, true)`: the connectors.
fn spans(f: &PlainSpan, gap: bool) -> Vec<(i64, i64)> {
    let pieces = if gap { feature_drawing_pieces(f, 100).into_iter().filter(|p| p.connector).collect() } else { feature_pieces(f, 100) };
    pieces.iter().map(|p| (p.start, p.end)).collect()
}

/// joined drawings cover the annotated bases only, and put labels inside a segment
#[test]
fn joined_drawings_cover_the_annotated_bases_only_and_put_labels_inside_a_segment() {
    assert_eq!(spans(&joined(), false), [(10, 19), (40, 59)]);
    assert_eq!(spans(&joined(), true), [(20, 39)]);
    for base in 0..100 {
        assert_eq!(feature_contains(&joined(), base, 100), (10..=19).contains(&base) || (40..=59).contains(&base));
    }
    assert_eq!(feature_anchor(&joined(), 100), 0.5);
    assert!(feature_contains(&joined(), (feature_anchor(&joined(), 100) * 100.0).floor() as i64, 100));
}

/// all pieces keep one biological direction marker, including circular splits and reversed joins
#[test]
fn all_pieces_keep_one_biological_direction_marker_including_circular_splits_and_reversed_joins() {
    for feature in [joined(), located(80, 9, None), located(80, 9, Some(&[(80, 89), (0, 9)]))] {
        for forward in [true, false] {
            let pieces = feature_pieces(&PlainSpan { forward: Some(forward), ..feature.clone() }, 100);
            assert_eq!(pieces.iter().filter(|p| p.show_head).count(), 1);
            assert!(pieces[if forward { 0 } else { pieces.len() - 1 }].show_head);
        }
    }
    assert!(spans(&located(80, 9, None), true).is_empty());
    assert_eq!(spans(&located(80, 9, Some(&[(80, 89), (0, 9)])), true), [(90, 99)]);
    assert_eq!(spans(&located(80, 19, Some(&[(80, 89), (10, 19)])), true), [(90, 99), (0, 9)]);
}

/// out-of-order and overlapping segments remain covered, with no connector on annotated bases
#[test]
fn out_of_order_and_overlapping_segments_remain_covered_with_no_connector_on_annotated_bases() {
    let feature = located(70, 44, Some(&[(70, 79), (20, 29), (25, 44)]));
    assert_eq!(spans(&feature, false), [(70, 79), (20, 29), (25, 44)]);
    for piece in feature_drawing_pieces(&feature, 100).iter().filter(|p| p.connector) {
        for base in piece.start..=piece.end {
            assert!(!feature_contains(&feature, base, 100));
        }
    }
    assert!(feature_pieces(&PlainSpan { locations: Some(Vec::new()), ..joined() }, 100).is_empty());
    assert!(feature_pieces(&PlainSpan { locations: Some(vec![(-1, 10)]), ..joined() }, 100).is_empty());
}

/// coalescing cannot merge a joined record with same-name neighbors or discard its source identity
#[test]
fn coalescing_cannot_merge_a_joined_record_with_same_name_neighbors_or_discard_its_source_identity() {
    let feature = PlainSpan { name: Some("Same".to_owned()), kind: Some("CDS".to_owned()), ..joined() };
    let neighbor = PlainSpan { name: Some("Same".to_owned()), kind: Some("CDS".to_owned()), ..located(20, 39, None) };
    let actual = coalesce_features(&[feature.clone(), neighbor.clone()]);
    assert_eq!(actual, [feature, neighbor]);
    // The connector reserves a lane too, so an intronic annotation cannot conceal the join.
    let lanes = assign_lanes(&actual, 100, None);
    assert_ne!(lanes[0], lanes[1]);
}

/// A piece carries its extent as fractions of the molecule too, which is what the map draws from.
#[test]
fn pieces_carry_their_extent_as_fractions() {
    let wrap = feature_pieces(&located(80, 9, None), 100);
    assert_eq!(wrap.iter().map(|p| (p.start_frac, p.width_frac)).collect::<Vec<_>>(), [(0.8, 0.2), (0.0, 0.1)]);
    let gap = feature_drawing_pieces(&joined(), 100)[0];
    assert!(gap.connector && !gap.show_head);
    assert_eq!((gap.start_frac, gap.width_frac), (0.2, 0.2));
}
