//! What is sent to the server for one construct.
//!
//! The expectation below is written by hand from the two things the server's side of this is
//! defined by: the feature shape the Bios web app and server share (0-based, inclusive at both
//! ends, a strand of 1 or -1, qualifiers under `notes`, pieces under `locations` only for a real
//! join) and what the web app's own snapshot of the same file holds. It is not this crate's
//! output copied back.

use bios_core::{genbank, genbank_write};
use bios_sync::Snapshot;
use serde_json::json;

/// GenBank counts from 1; every position below is one more than in the expectation.
const FILE: &str = "\
LOCUS       pTest                     20 bp    DNA     circular SYN 18-NOV-2026
DEFINITION  A plasmid for a test.
FEATURES             Location/Qualifiers
     CDS             3..10
                     /label=\"GFP\"
                     /gene=\"gfp\"
                     /note=\"first\"
                     /note=\"second\"
     promoter        complement(13..16)
                     /label=\"P1\"
     misc_feature    <5..>8
                     /label=\"partial\"
     misc_feature    join(18..20,1..4)
                     /label=\"through the origin\"
     exon            order(2..4,7..9)
                     /label=\"pieces\"
ORIGIN
        1 acgtacgtag acgtacgtag
//
";

#[test]
fn a_snapshot_has_the_shape_the_web_app_sends() {
    let construct = genbank::read(FILE).unwrap();

    let snapshot = Snapshot::of(&construct).unwrap();

    let expected = json!({
        "name": "pTest",
        "text": genbank_write::write(&construct).unwrap(),
        "sequence": "ACGTACGTAGACGTACGTAG",
        "circular": true,
        "features": [
            { "name": "GFP", "type": "CDS", "start": 2, "end": 9, "strand": 1, "notes": { "gene": ["gfp"], "note": ["first", "second"] } },
            { "name": "P1", "type": "promoter", "start": 12, "end": 15, "strand": -1, "notes": {} },
            { "name": "partial", "type": "misc_feature", "start": 4, "end": 7, "strand": 1, "startFuzzy": "<", "endFuzzy": ">", "notes": {} },
            // One contiguous feature that runs through the origin: its end is before its start,
            // and it has no pieces.
            { "name": "through the origin", "type": "misc_feature", "start": 17, "end": 3, "strand": 1, "notes": {} },
            {
                "name": "pieces", "type": "exon", "start": 1, "end": 8, "strand": 1,
                "locations": [{ "start": 1, "end": 3 }, { "start": 6, "end": 8 }],
                "locationOperator": "order",
                "notes": {}
            },
        ],
    });
    assert_eq!(serde_json::to_value(&snapshot).unwrap(), expected);
    // The text is the whole construct: reading it gives back what was sent beside it.
    let back = genbank::read(&snapshot.text).unwrap();
    assert_eq!(back, construct);
    assert!(snapshot.text.contains("DEFINITION  A plasmid for a test."), "what Bios does not interpret goes along unchanged");
}

#[test]
fn a_location_bios_does_not_interpret_is_sent_as_written() {
    let construct = genbank::read(&FILE.replace("order(2..4,7..9)", "J00194.1:100..202")).unwrap();
    let sent = serde_json::to_value(Snapshot::of(&construct).unwrap()).unwrap();
    assert_eq!(sent["features"][4]["locationUnsupported"], "J00194.1:100..202");
    assert_eq!(sent["features"][4]["name"], "pieces");
}

#[test]
fn a_construct_with_no_bases_is_never_made_into_something_to_send() {
    let mut construct = genbank::read(FILE).unwrap();
    construct.sequence.clear();
    assert_eq!(Snapshot::of(&construct), Err(genbank_write::WriteError::Empty));
}
