//! Ported from the Bios web app's `packages/formats/src/genbankLocations.test.ts`.
//!
//! The original's "part pragmas cannot shift location metadata" test is not ported: it guards
//! the web app's separate parts lane, which this reader does not have.

use bios_core::model::LocationOperator;
use bios_core::{genbank, genbank_write};

const SEQUENCE: &str = "ACGTTGCAAGTCGATCGTACGATC";

fn file(location: &str) -> String {
    format!(
        "LOCUS       test 24 bp DNA circular\nFEATURES             Location/Qualifiers\n     CDS             {location}\n                     /label=\"spliced\"\nORIGIN\n        1 {}\n//\n",
        SEQUENCE.to_lowercase()
    )
}

#[test]
fn location_semantics_survive_reading_and_repeated_writing() {
    for expression in [
        "join(2..5,12..15)",
        "complement(join(2..5,12..15))",
        "order(2..5,12..15)",
        "<2..>15",
        "join(<2..5,12..>15)",
        "complement(order(<2..5,12..>15))",
        "2^3",
        "join(2..5,ACC.1:2..5)",
    ] {
        let first = genbank::read(&file(expression)).unwrap();
        let out = genbank_write::write(&first).unwrap();
        assert!(out.contains(expression), "{expression}:\n{out}");
        let reopened = genbank::read(&out).unwrap();
        assert_eq!(reopened.features, first.features, "{expression}");
    }
}

#[test]
fn equivalent_nested_reverse_joins_normalize_to_one_orientation_without_changing_transcript_order() {
    let a = genbank::read(&file("complement(join(2..5,12..15))")).unwrap();
    let b = genbank::read(&file("join(complement(12..15),complement(2..5))")).unwrap();
    assert_eq!(a.features, b.features);
}

#[test]
fn remote_or_mixed_strand_locations_remain_explicitly_uninterpreted_and_cannot_silently_move() {
    let result = genbank::read(&file("join(2..5,complement(12..15))")).unwrap();
    let f = &result.features[0];
    assert_eq!(f.location_unsupported.as_deref(), Some("join(2..5,complement(12..15))"));
    // The original asserts that writing a moved copy throws. Here the writer always writes the
    // location as it was read, and every edit refuses a feature that has one, so it cannot move.
    let mut moved = result.clone();
    moved.features[0].start += 1;
    assert!(genbank_write::write(&moved).unwrap().contains("join(2..5,complement(12..15))"));
}

#[test]
fn a_surviving_single_order_segment_keeps_its_declared_semantics_when_saved() {
    let read = genbank::read(&file("order(2..5)")).unwrap();
    assert_eq!(read.features[0].location_operator, Some(LocationOperator::Order));
    let out = genbank_write::write(&read).unwrap();
    assert!(out.contains("order(2..5)"));
    let reopened = genbank::read(&out).unwrap();
    assert_eq!(reopened.features[0].location_operator, Some(LocationOperator::Order));
}

#[test]
fn a_join_through_the_origin_is_one_wrapping_feature_however_it_was_written() {
    let joined = genbank::read(&file("join(20..24,1..3)")).unwrap();
    let wrapped = genbank::read(&file("20..3")).unwrap();
    assert_eq!(joined.features, wrapped.features);
    assert_eq!((joined.features[0].start, joined.features[0].end), (19, 2));
    assert_eq!(joined.features[0].segments.len(), 1);
    assert!(genbank_write::write(&wrapped).unwrap().contains("join(20..24,1..3)"));
}
