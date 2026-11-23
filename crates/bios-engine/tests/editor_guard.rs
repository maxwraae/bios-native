//! The web app's `editGuard.test.ts` (5 cases), in their order.
//!
//! Named for the loss each case prevents. The guard exists because two real plasmids were
//! destroyed by an empty write.

use bios_core::Construct;
use bios_core::model::{Feature, Segment, Strand};
use bios_engine::editor::guard::{accepts_edit, bases_of};

fn seq(sequence: &str) -> Construct {
    Construct { sequence: sequence.to_string(), ..Construct::default() }
}

#[test]
fn a_real_construct_is_accepted() {
    assert!(accepts_edit(Some(&seq("ACGT"))));
    assert!(accepts_edit(Some(&seq("a"))));
}

#[test]
fn an_editor_that_has_not_been_given_its_sequence_is_not_an_edit() {
    // An editor holds an empty sequence for a beat after it opens, and would hand that back as
    // though everything had been deleted.
    assert!(!accepts_edit(Some(&seq(""))));
}

#[test]
fn a_failed_or_truncated_read_is_not_an_edit() {
    // The web app's three inputs are an empty record, null and undefined.
    assert!(!accepts_edit(Some(&Construct::default())));
    assert!(!accepts_edit(None));
}

#[test]
fn a_non_string_sequence_is_not_trusted_into_a_length_check() {
    // The web app passes a number, a list and an object where the bases should be: the length of
    // a list would be a count of something else, and would pass a careless check. A `Construct`
    // can hold nothing but text there, so those inputs cannot be written. What is left of the
    // case is that only bases count: a construct with a name and a feature but no bases is refused.
    let feature = Feature {
        name: "GFP".to_string(),
        kind: "CDS".to_string(),
        start: 0,
        end: 2,
        strand: Strand::Forward,
        segments: vec![Segment::new(0, 2)],
        notes: Vec::new(),
        start_fuzzy: None,
        end_fuzzy: None,
        location_operator: None,
        location_unsupported: None,
    };
    let no_bases = Construct { name: "12345".to_string(), features: vec![feature], ..Construct::default() };
    assert!(!accepts_edit(Some(&no_bases)));
}

#[test]
fn bases_of_never_throws_on_junk() {
    // The web app's junk is values of the wrong type. Here it is nothing, and a construct of nothing.
    assert_eq!(bases_of(None), "");
    assert_eq!(bases_of(Some(&Construct::default())), "");
    assert_eq!(bases_of(Some(&seq("ACGT"))), "ACGT");
}
