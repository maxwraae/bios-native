//! Melting temperature and GC content against recorded Biopython answers, ported from the web app's
//! `thermodynamics.test.ts` in its order and with its numbers.
//!
//! The web app's first two tests also check `gibsonTm`, which belongs to a module that is not
//! ported yet (`gibson.ts`). Those lines are gathered in the ignored test at the end.

use bios_core::seq::{DNA_TM_DEFAULTS, GcRange, format_gc_content, gc_content_range, revcomp, tm};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `tm(seq)` of the web app: the default conditions.
fn default_tm(seq: &str) -> Option<f64> {
    tm(seq, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm)
}

#[test]
fn perfect_duplex_tm_matches_pinned_biopython_across_composition_symmetry_salt_and_concentration() {
    let oracle: Value = serde_json::from_str(&fixture("tm-oracle.json")).expect("json");
    assert_eq!(oracle["biopython"], "1.85");
    let cases = oracle["cases"].as_array().expect("cases");
    assert!(cases.len() >= 150);
    for row in cases {
        let sequence = row["sequence"].as_str().expect("sequence");
        let concentration_nm = row["concentrationNM"].as_f64().expect("concentrationNM");
        let sodium_mm = row["sodiumMM"].as_f64().expect("sodiumMM");
        let expected = row["tm"].as_f64().expect("tm");
        let actual = tm(sequence, concentration_nm, sodium_mm);
        assert!(actual.is_some_and(|actual| (actual - expected).abs() < 1e-9), "{row} actual: {actual:?}");
        let reversed = tm(&revcomp(sequence), concentration_nm, sodium_mm).expect("the reverse complement has a Tm");
        assert!((reversed - expected).abs() < 1e-9, "{row} reverse complement: {reversed}");
    }
}

#[test]
fn unknown_degenerate_gapped_or_invalid_dna_never_gets_a_scalar_tm() {
    for code in "RYSWKMBDHVNUX-.*".chars() {
        for size in [0, 6, 20, 100] {
            let seq = format!("{}{code}ACGT", "ACGT".repeat(size));
            assert_eq!(default_tm(&seq), None, "{seq}");
        }
    }
    for seq in ["", "A", " 123 "] {
        assert_eq!(default_tm(seq), None);
    }
    assert_eq!(default_tm("1 acgt\n5 CGTA"), default_tm("ACGTCGTA"));
    // The last is the web app's `Number.MIN_VALUE`, the smallest number above zero.
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::from_bits(1)] {
        assert_eq!(tm("ACGTCGTA", bad, 50.0), None);
        assert_eq!(tm("ACGTCGTA", 250.0, bad), None);
    }
}

#[test]
fn gc_bounds_cover_every_concrete_expansion_of_all_iupac_pairs() {
    let alphabet = [
        ("A", "A"),
        ("C", "C"),
        ("G", "G"),
        ("T", "T"),
        ("R", "AG"),
        ("Y", "CT"),
        ("S", "GC"),
        ("W", "AT"),
        ("K", "GT"),
        ("M", "AC"),
        ("B", "CGT"),
        ("D", "AGT"),
        ("H", "ACT"),
        ("V", "ACG"),
        ("N", "ACGT"),
    ];
    for (a, aa) in alphabet {
        for (b, bb) in alphabet {
            let percentages: Vec<f64> =
                aa.chars().flat_map(|x| bb.chars().map(move |y| [x, y].iter().filter(|z| "GC".contains(**z)).count() as f64 * 50.0)).collect();
            let min = percentages.iter().copied().fold(f64::INFINITY, f64::min);
            let max = percentages.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            assert_eq!(gc_content_range(&format!("{a}{b}")), Some(GcRange { min, max }), "{a}{b}");
        }
    }
    assert_eq!(format_gc_content("CGANNNNNNTGC", 0), "33–84% GC");
    assert_eq!(format_gc_content("SW", 0), "50% GC");
    assert_eq!(format_gc_content("N", 0), "0–100% GC");
    assert_eq!(format_gc_content(&format!("{}N", "G".repeat(1000)), 1), "99.9–100.0% GC");
    for seq in ["", "AXT", "AC-GT", "AUG"] {
        assert_eq!(gc_content_range(seq), None);
    }
}

// The Gibson half of the first two tests above (the oracle's `gibsonTm` column) is in
// `tests/gibson_thermodynamics.rs`.
