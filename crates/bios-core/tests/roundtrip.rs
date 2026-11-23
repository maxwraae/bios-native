//! Writing a construct and reading it back gives the same construct.

use bios_core::{genbank, genbank_write};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn round_trips(name: &str) {
    let original = genbank::read(&fixture(name)).unwrap();
    let written = genbank_write::write(&original).unwrap();
    let again = genbank::read(&written).unwrap();
    assert_eq!(again.features.len(), original.features.len(), "{name}: feature count");
    for (a, b) in original.features.iter().zip(&again.features) {
        assert_eq!(a, b, "{name}: feature {}", a.name);
    }
    assert_eq!(again, original, "{name}");
    assert_eq!(genbank_write::write(&again).unwrap(), written, "{name}: writing twice gives the same text");
    assert!(written.lines().all(|l| l.chars().count() <= 80), "{name}: a line passed column 80");
}

#[test]
fn puc19_round_trips() {
    round_trips("pUC19.gb");
}

#[test]
fn a_construct_with_a_feature_through_the_origin_round_trips() {
    round_trips("wrap_demo.gb");
}
