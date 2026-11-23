//! Every NCBI genetic code and the translation of a CDS against recorded Biopython answers, ported
//! from the web app's `coding.test.ts` in its order and with its numbers.

mod support;

use bios_core::coding::{CodingTranslation, GENETIC_CODES, coding_residue, is_start_codon, is_stop_codon, read_coding_settings, translate_coding};
use serde_json::Value;
use support::notes;

fn oracle() -> Value {
    let path = format!("{}/tests/fixtures/genetic-code-oracle.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).expect("json")
}

/// All 64 codons, T, C, A, G at each position.
fn codons() -> Vec<String> {
    "TCAG".chars().flat_map(|a| "TCAG".chars().flat_map(move |b| "TCAG".chars().map(move |c| format!("{a}{b}{c}")))).collect()
}

fn sorted(mut codons: Vec<String>) -> Vec<String> {
    codons.sort();
    codons
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().expect("array").iter().map(|v| v.as_str().expect("string").to_string()).collect()
}

#[test]
fn all_64_codons_initiation_codons_and_termination_codons_agree_with_biopython_for_all_27_ncbi_tables() {
    let oracle = oracle();
    let tables = oracle["tables"].as_array().expect("tables");
    assert_eq!(tables.len(), 27);
    let ids: Vec<u64> = tables.iter().map(|t| t["id"].as_u64().expect("id")).collect();
    assert_eq!(GENETIC_CODES.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
    let codons = codons();
    for (t, id) in tables.iter().zip(ids) {
        let settings = read_coding_settings(&notes(&[("transl_table", &[id.to_string().as_str()])]));
        assert_eq!(codons.iter().map(|c| coding_residue(c, &settings, false, false)).collect::<String>(), t["ordinary"], "table {id}");
        assert_eq!(sorted(codons.iter().filter(|c| is_start_codon(c, id)).cloned().collect()), sorted(strings(&t["starts"])), "table {id}");
        assert_eq!(sorted(codons.iter().filter(|c| is_stop_codon(c, id)).cloned().collect()), sorted(strings(&t["stops"])), "table {id}");
    }
}

#[test]
fn every_alternative_initiator_and_both_partial_5_prime_phases_translate_like_biopython() {
    let oracle = oracle();
    for c in oracle["cdsCases"].as_array().expect("cdsCases") {
        let table = c["table"].as_u64().expect("table").to_string();
        let phase = c["phase"].as_u64().expect("phase");
        let codon_start = (phase + 1).to_string();
        let settings = read_coding_settings(&notes(&[("transl_table", &[table.as_str()]), ("codon_start", &[codon_start.as_str()])]));
        let t = translate_coding(c["dna"].as_str().expect("dna"), settings);
        assert_eq!(t.protein, c["protein"], "{c}");
        assert!(t.verified, "{c}");
        assert!(t.valid_stop, "{c}");
        assert_eq!(t.complete, phase == 0, "{c}");
    }
}

#[test]
fn ambiguous_codons_are_resolved_only_when_every_possibility_gives_the_same_residue() {
    let t = |dna: &str| translate_coding(dna, read_coding_settings(&notes(&[("bios_start_codon", &["ordinary"])])));
    assert_eq!(t("GCNTAR").protein, "A*");
    assert!(t("GCNTAR").verified);
    assert_eq!(t("TANNNN").protein, "XX");
    assert!(!t("TANNNN").verified);
}

#[test]
fn first_codon_overrides_and_dual_coded_stops_have_explicit_context() {
    let t = |dna: &str, pairs: &[(&str, &[&str])]| translate_coding(dna, read_coding_settings(&notes(pairs)));
    assert_eq!(t("GTGGCTTAA", &[("transl_table", &["11"])]).protein, "MA*");
    assert_eq!(t("GTGGCTTAA", &[("transl_table", &["11"]), ("bios_start_codon", &["ordinary"])]).protein, "VA*");
    assert_eq!(t("GTGGCTTAA", &[("bios_start_codon", &["methionine"])]).protein, "MA*");
    assert_eq!(t("ATGTGATGA", &[("transl_table", &["27"])]).protein, "MW*");
    assert_eq!(t("ATGTGATGA", &[("transl_table", &["27"]), ("bios_cds_partial_end", &["true"])]).protein, "MWW");
}

#[test]
fn invalid_and_exceptional_annotations_remain_unverified_without_falling_back_to_standard_translation() {
    let cases: [(&str, &[&str]); 9] = [
        ("codon_start", &[]),
        ("codon_start", &["0"]),
        ("codon_start", &["1", "2"]),
        ("transl_table", &["7"]),
        ("transl_table", &["bad"]),
        ("bios_start_codon", &["bad"]),
        ("transl_except", &["(pos:4..6,aa:Sec)"]),
        ("pseudo", &[""]),
        ("bios_translation_unavailable", &["true"]),
    ];
    for case in cases {
        let t: CodingTranslation = translate_coding("ATGGCTTAA", read_coding_settings(&notes(&[case])));
        assert!(t.unavailable.is_some_and(|reason| !reason.is_empty()), "{case:?}");
        assert_eq!(t.protein, "", "{case:?}");
        assert!(!t.verified, "{case:?}");
    }
}
