//! The `gibsonTm` half of the web app's `thermodynamics.test.ts`, which `tests/thermodynamics.rs`
//! left as an ignored test while `gibson.ts` was unported.
//!
//! The web app asserts these inside its first two Tm tests: for every oracle row,
//! `|gibsonTm(row.sequence) - row.gibsonTm| < 1e-9`, and for every sequence the second test builds
//! around a code that is not A, C, G or T, `gibsonTm(seq) === null`. The oracle's `gibsonTm` column
//! is the same pinned Biopython run as its `tm` column, read under the Gibson conditions
//! (dnac1=250, dnac2=0, so no Ct/4 and no self-complementary symmetry correction).

use bios_core::gibson::gibson_tm;
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn gibson_tm_matches_pinned_biopython_across_composition_symmetry_salt_and_concentration() {
    let oracle: Value = serde_json::from_str(&fixture("tm-oracle.json")).expect("json");
    assert_eq!(oracle["biopython"], "1.85");
    let cases = oracle["cases"].as_array().expect("cases");
    assert!(cases.len() >= 150);
    for row in cases {
        let sequence = row["sequence"].as_str().expect("sequence");
        let expected = row["gibsonTm"].as_f64().expect("gibsonTm");
        let actual = gibson_tm(sequence);
        assert!(actual.is_some_and(|actual| (actual - expected).abs() < 1e-9), "Gibson convention: {sequence} actual: {actual:?}");
    }
}

#[test]
fn gibson_tm_is_never_given_for_unknown_degenerate_gapped_or_invalid_dna() {
    for code in "RYSWKMBDHVNUX-.*".chars() {
        for size in [0, 6, 20, 100] {
            let seq = format!("{}{code}ACGT", "ACGT".repeat(size));
            assert_eq!(gibson_tm(&seq), None, "{seq}");
        }
    }
}
