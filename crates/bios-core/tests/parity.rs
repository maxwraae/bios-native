//! The Rust reader against what Bios itself parsed from the same files.

use bios_core::genbank;
use bios_core::model::Strand;
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn a_feature_through_the_origin_is_kept_and_wraps() {
    let c = genbank::read(&fixture("wrap_demo.gb")).expect("reads");
    assert!(c.circular);
    assert_eq!(c.len(), 800);
    assert_eq!(c.features.len(), 4);
    let amp = &c.features[0];
    assert_eq!((amp.name.as_str(), amp.start, amp.end, amp.strand), ("AmpR", 749, 99, Strand::Forward));
    assert!(amp.wraps());
    assert_eq!(amp.span(c.len()), 151);
    assert_eq!(amp.note("note").expect("notes")[0], "spans the origin - the shape that used to be silently dropped");
    assert_eq!(c.comments, vec!["bios_construct_id: e1bce6ce-3ba7-41a1-bc1f-21012f5d610f"]);
}

/// The lab construct the snapshot was recorded from is not in this repository: it is read from the
/// folder `BIOS_PRIVATE_FIXTURES` names, which keeps this repository's layout.
mod private {
    use super::*;

    fn private_fixture(name: &str) -> String {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        let path = std::path::PathBuf::from(root).join("crates/bios-core/tests/fixtures").join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_lab_construct_matches_the_bios_snapshot_feature_for_feature() {
        let ours = genbank::read(&private_fixture("lab_construct.gb")).expect("reads");
        let theirs: Value = serde_json::from_str(&private_fixture("lab_construct.snapshot.json")).expect("json");

        assert_eq!(ours.name, theirs["name"]);
        assert_eq!(ours.circular, theirs["circular"]);
        assert_eq!(ours.len() as u64, theirs["length"].as_u64().expect("length"));
        assert_eq!(ours.sequence, theirs["sequence"]);

        let expected = theirs["features"].as_array().expect("features");
        assert_eq!(ours.features.len(), expected.len());
        for (f, e) in ours.features.iter().zip(expected) {
            let at = format!("{} {}..{}", f.name, f.start, f.end);
            assert_eq!(f.name, e["name"], "{at}");
            assert_eq!(f.kind, e["type"], "{at}");
            assert_eq!(f.start as u64, e["start"].as_u64().expect("start"), "{at}");
            assert_eq!(f.end as u64, e["end"].as_u64().expect("end"), "{at}");
            assert_eq!(i64::from(f.strand.sign()), e["strand"].as_i64().expect("strand"), "{at}");
            let notes = e["notes"].as_object().expect("notes");
            assert_eq!(f.notes.len(), notes.len(), "{at}: note keys");
            for (key, values) in &f.notes {
                let want: Vec<String> = notes[key].as_array().expect("values").iter().map(|v| v.as_str().expect("string").to_string()).collect();
                assert_eq!(values, &want, "{at}: /{key}");
            }
        }
    }
}
