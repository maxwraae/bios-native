//! Joined (spliced) features through edits, ported from the web app's `joinedFeatures.test.ts`
//! in its order and with its numbers.
//!
//! That file also covers three modules that are not ported yet: the address model
//! (`bios/model.ts`), feature addresses (`bios/address.ts`) and block slicing (`concat.ts`). A
//! test that is only about those is kept by name and ignored. A test that also edits keeps every
//! assertion about the edit, and names the ones it had to leave out.

mod support;

use bios_core::edits::{apply_delete, apply_insert, apply_replace};
use bios_core::model::{Feature, Fuzzy, Segment, Strand};
use support::{bases, feature, own, segments, with_locations};

const SEQUENCE: &str = "ATGAAACCCGGGTTTAAACCCGGGTTTAAACCC";

fn joined() -> Feature {
    with_locations(feature("spliced", "CDS", 1, 19, Strand::Forward), &segments(&[(1, 4), (11, 14), (17, 19)]))
}

#[test]
#[ignore = "tests buildModel, span and featsAt of bios/model.ts, which are not ported; it makes no edit"]
fn joined_feature_reads_exclude_introns_preserve_segment_order_and_reverse_complement_the_assembled_transcript() {}

#[test]
fn an_insertion_inside_an_intron_shifts_only_downstream_segments_and_leaves_the_transcript_unchanged() {
    let r = apply_insert(SEQUENCE, &[joined()], 9, "ACAC", false).unwrap();
    let f = &r.features[0];
    assert_eq!(f.segments, segments(&[(1, 4), (15, 18), (21, 23)]));
    assert_eq!(bases(&r.sequence, f, false), bases(SEQUENCE, &joined(), false));
    assert_eq!(r.delta.truncated.len(), 0);
}

#[test]
fn an_insertion_inside_one_exon_extends_that_exon_without_filling_an_intron() {
    let r = apply_insert(SEQUENCE, &[joined()], 4, "AC", false).unwrap();
    let f = &r.features[0];
    assert_eq!(f.segments, segments(&[(1, 6), (13, 16), (19, 21)]));
    assert_eq!(bases(&r.sequence, f, false), format!("{}AC{}{}{}", &SEQUENCE[1..3], &SEQUENCE[3..5], &SEQUENCE[11..15], &SEQUENCE[17..20]));
}

#[test]
fn deleting_an_entire_exon_retains_one_joined_annotation_and_counts_only_the_annotated_bases_removed() {
    let r = apply_delete(SEQUENCE, &[joined()], 12, 15, false).unwrap();
    let f = &r.features[0];
    assert_eq!(r.features.len(), 1);
    assert_eq!(f.segments, segments(&[(1, 4), (13, 15)]));
    assert_eq!(r.delta.truncated.first().map(|t| t.lost_bases), Some(4));
    assert_eq!(bases(&r.sequence, f, false), format!("{}{}", &SEQUENCE[1..5], &SEQUENCE[17..20]));
}

#[test]
fn deleting_every_exon_removes_the_annotation_one_surviving_exon_becomes_a_simple_feature() {
    let r = apply_delete(SEQUENCE, &[joined()], 2, 15, false).unwrap();
    let f = &r.features[0];
    assert_eq!(f.locations(), None);
    assert_eq!(f.start, 3);
    assert_eq!(f.end, 5);
    assert_eq!(bases(&r.sequence, f, false), &SEQUENCE[17..20]);
    assert_eq!(apply_delete(SEQUENCE, &[joined()], 2, 20, false).unwrap().features.len(), 0);
}

#[test]
fn an_interior_replacement_changes_one_exon_retaining_the_other_segments() {
    let r = apply_replace(SEQUENCE, &[joined()], 13, 14, "ACACA", false).unwrap();
    let f = &r.features[0];
    assert_eq!(f.segments, segments(&[(1, 4), (11, 17), (20, 22)]));
    assert_eq!(bases(&r.sequence, f, false), format!("{}{}ACACA{}{}", &SEQUENCE[1..5], &SEQUENCE[11..12], &SEQUENCE[14..15], &SEQUENCE[17..20]));
}

#[test]
#[ignore = "tests byHandle of bios/model.ts and ownToPlus of bios/address.ts, which are not ported; it makes no edit"]
fn literal_addresses_map_to_the_actual_exon_on_both_strands_and_refuse_to_consume_an_intervening_intron() {}

#[test]
fn origin_spanning_joins_retain_their_ordered_pieces_through_a_circular_deletion() {
    let f = Feature { start: 27, end: 8, ..with_locations(joined(), &segments(&[(27, 30), (2, 4), (7, 8)])) };
    let r = apply_delete(SEQUENCE, std::slice::from_ref(&f), 14, 17, true).unwrap();
    assert_eq!(r.features[0].segments, segments(&[(23, 26), (2, 4), (7, 8)]));
    assert_eq!(bases(&r.sequence, &r.features[0], true), bases(SEQUENCE, &f, true));
}

#[test]
#[ignore = "tests sliceBlock and concatBlocks of concat.ts, which are not ported; it makes no edit"]
fn slicing_and_concatenating_blocks_carries_one_joined_feature_with_correctly_offset_segments() {}

#[test]
fn uncertain_segment_markers_survive_edits_and_block_projection_without_becoming_a_verified_full_sequence() {
    let before = Segment { start_fuzzy: Some(Fuzzy::Before), ..Segment::new(1, 4) };
    let uncertain = with_locations(joined(), &[before, Segment { end_fuzzy: Some(Fuzzy::After), ..Segment::new(11, 14) }]);
    let edited = apply_insert(SEQUENCE, std::slice::from_ref(&uncertain), 9, "ACAC", false).unwrap();
    assert_eq!(edited.features[0].segments, [before, Segment { end_fuzzy: Some(Fuzzy::After), ..Segment::new(15, 18) }]);
    assert!(own(&edited.sequence, &edited.features[0], false).unwrap_err().contains("uncertain"));
    // Left out: the web app next slices this feature into a block and concatenates it
    // (sliceBlock and concatBlocks of concat.ts, not ported) and expects the markers at
    // [{3, 6, <}, {13, 16, >}].
    let one = apply_delete(SEQUENCE, &[uncertain], 2, 5, false).unwrap().features.remove(0);
    assert_eq!(one.locations(), None);
    assert_eq!(one.end_fuzzy, Some(Fuzzy::After));
}

#[test]
fn dna_edits_cannot_silently_remap_an_unsupported_feature_expression() {
    let f = Feature { location_unsupported: Some("join(2..5,ACC.1:12..15)".to_string()), ..joined() };
    assert!(apply_insert(SEQUENCE, &[f], 9, "A", false).unwrap_err().to_string().contains("unsupported location"));
    // Left out: the web app also expects sliceBlock of concat.ts, which is not ported, to refuse
    // the same feature.
}
