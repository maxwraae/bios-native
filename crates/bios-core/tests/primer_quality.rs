//! The web app's `editor/primerQuality.test.ts`, in its order and with its sequences.

use bios_core::primer_quality::primer_flags;

#[test]
fn a_clean_primer_has_no_warnings() {
    // Ends in C (clamp), no long runs, not self-complementary.
    let f = primer_flags("atcaatgagtacaggatc");
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    assert!(f.gc_clamp);
}

#[test]
fn flags_a_missing_3_prime_gc_clamp() {
    let f = primer_flags("atgctagcatgacttgaa");
    assert!(!f.gc_clamp);
    assert!(f.warnings.iter().any(|w| w.contains("clamp")));
}

#[test]
fn flags_a_gc_rich_3_prime_end() {
    let f = primer_flags("atatatatatatgcgcgc");
    assert!(f.warnings.iter().any(|w| w.contains("GC-rich")));
}

#[test]
fn flags_a_homopolymer_run() {
    let f = primer_flags("atgcaaaaacgtacgtac");
    assert!(f.long_run >= 5);
    assert!(f.warnings.iter().any(|w| w.contains("run")));
    assert!(f.warnings.contains(&"5× A run".to_string()));
}

#[test]
fn flags_self_complementarity_a_palindrome_hairpins() {
    let f = primer_flags("gaattcgaattc");
    assert!(f.self_comp >= 6);
    assert!(f.warnings.iter().any(|w| w.contains("self-compl")));
}

#[test]
fn is_case_and_junk_insensitive() {
    assert!(primer_flags("ATCAATGAGTACAGGATC").warnings.is_empty());
    let _ = primer_flags("atg-cta n cat");
    let _ = primer_flags("");
}
