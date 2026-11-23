//! What clipping a coding feature does to its reading frame, ported from the web app's
//! `codingEdit.test.ts` in its order and with its numbers.

mod support;

use bios_core::edits::{apply_delete, apply_insert};
use bios_core::model::{Feature, Strand};
use bios_core::seq::revcomp;
use support::{feature, notes, protein_of, segments, strand, with_locations};

fn clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(sign: i8, removed: i64) {
    let sequence = if sign == 1 { "ATGTTGGCTTAA".to_string() } else { revcomp("ATGTTGGCTTAA") };
    let f = Feature {
        notes: notes(&[("translation", &["MLA"]), ("bios_start_codon", &["methionine"]), ("note", &["keep this"])]),
        ..feature("CDS", "CDS", 0, 11, strand(sign))
    };
    let (from, to) = if sign == 1 { (1, removed) } else { (13 - removed, 12) };
    let result = apply_delete(&sequence, std::slice::from_ref(&f), from, to, false).unwrap();
    let translation = protein_of(&result.sequence, &result.features, false);
    assert_eq!(translation.protein, "LA*");
    assert_eq!(translation.settings.phase, ((3 - removed) % 3) as u64);
    assert!(translation.settings.partial_start);
    assert!(!translation.complete);
    assert!(translation.verified);
    assert_eq!(result.features[0].note("translation"), None);
    assert_eq!(result.features[0].note("note"), Some(&["keep this".to_string()][..]));
    assert_eq!(f.note("translation").map(|v| v[0].as_str()), Some("MLA"), "the prior saved record is untouched");
}

#[test]
fn clipping_1_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(1, 1);
}

#[test]
fn clipping_2_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(1, 2);
}

#[test]
fn clipping_3_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(1, 3);
}

#[test]
fn clipping_1_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_minus_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(-1, 1);
}

#[test]
fn clipping_2_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_minus_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(-1, 2);
}

#[test]
fn clipping_3_bases_from_the_cds_5_prime_end_retains_surviving_codons_strand_minus_1() {
    clipping_bases_from_the_cds_5_prime_end_retains_surviving_codons(-1, 3);
}

#[test]
fn joined_clipping_counts_only_exon_bases_and_an_interior_insertion_still_shifts_the_frame() {
    let f = with_locations(feature("CDS", "CDS", 0, 15, Strand::Forward), &segments(&[(0, 1), (6, 15)]));
    let result = apply_delete("ATCCCCGTTGGCTTAA", &[f], 1, 7, false).unwrap();
    assert_eq!(protein_of(&result.sequence, &result.features, false).protein, "LA*");
    let shifted = apply_insert("ATGTTGGCTTAA", &[feature("CDS", "CDS", 0, 11, Strand::Forward)], 5, "A", false).unwrap();
    let t = protein_of(&shifted.sequence, &shifted.features, false);
    assert_eq!(t.settings.phase, 0);
    assert_eq!(t.trailing_bases, 1);
    assert_ne!(t.protein, "MLA*");
}

#[test]
fn clipping_across_the_origin_retains_coding_order_and_distinguishes_the_missing_3_prime_end() {
    let f = feature("CDS", "CDS", 12, 7, Strand::Forward);
    let result = apply_delete("TGGCTTAACCCCATGT", &[f], 13, 2, true).unwrap();
    assert_eq!(protein_of(&result.sequence, &result.features, true).protein, "A*");
    let tail = apply_delete("ATGTTGGCTTAA", &[feature("CDS", "CDS", 0, 11, Strand::Forward)], 12, 12, false).unwrap();
    let t = protein_of(&tail.sequence, &tail.features, false);
    assert_eq!(t.protein, "MLA");
    assert!(t.settings.partial_end);
    assert!(!t.settings.partial_start);
}
