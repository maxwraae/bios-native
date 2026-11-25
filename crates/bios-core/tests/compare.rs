//! Two plasmids compared end to end, against answers that are known exactly because the second
//! plasmid was made from the first: the same sequence, the same sequence turned, the same
//! sequence flipped, and copies carrying one change each at a base this file chose.
//!
//! pUC19 is public (GenBank L09137), 2,686 bp, circular. Every position below is 1-based in the
//! comment and 0-based in the assertion, as `bios-core` counts.

use std::time::Instant;

use bios_core::compare::{Comparison, Diff, DiffKind, Place, ProteinEffect, compare, describe_effect, feature_at, indel_effect, substitution_effect};
use bios_core::seq::revcomp;
use bios_core::{Construct, genbank};

fn puc19() -> Construct {
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/pUC19.gb", env!("CARGO_MANIFEST_DIR"))).unwrap();
    genbank::read(&text).unwrap()
}

/// `seq` read from its base `by` (0-based) on, the bases before it put at the end: the same
/// circular molecule with its origin moved.
fn turned(seq: &str, by: usize) -> String {
    format!("{}{}", &seq[by..], &seq[..by])
}

/// `seq` with the 0-based base `at` replaced.
fn substituted(seq: &str, at: usize, base: char) -> String {
    let mut out = seq.to_string();
    out.replace_range(at..at + 1, &base.to_string());
    out
}

fn circular(a: &str, b: &str) -> Comparison {
    compare(a, true, b, true).unwrap_or_else(|e| panic!("{}", e.reason))
}

fn only(c: &Comparison) -> &Diff {
    assert_eq!(c.diffs.len(), 1, "{:?}", c.diffs);
    &c.diffs[0]
}

#[test]
fn a_plasmid_compared_with_itself_is_identical() {
    let p = puc19();
    let c = circular(&p.sequence, &p.sequence);
    assert!(c.identical());
    assert_eq!(c.diffs, Vec::new());
    assert_eq!((c.a_length, c.b_length, c.matches, c.aligned_columns), (2686, 2686, 2686, 2686));
    assert_eq!(c.identity(), 1.0);
    assert_eq!((c.flipped, c.rotated, c.b_origin), (false, false, 0));
}

#[test]
fn a_plasmid_whose_origin_moved_is_the_same_plasmid_and_the_turn_is_reported() {
    let p = puc19();
    // Read from base 1,001 on: base 1,687 of the turned copy is then base 1 of pUC19.
    let c = circular(&p.sequence, &turned(&p.sequence, 1000));
    assert!(c.identical());
    assert_eq!((c.flipped, c.rotated, c.b_origin), (false, true, 1686));
    assert_eq!(c.identity(), 1.0);
    // The map goes both ways: base 1 of pUC19 is base 1,687 of the copy, and base 1,001 of pUC19
    // is base 1 of it.
    assert_eq!(c.b_of_a(0), Some(1686));
    assert_eq!(c.b_of_a(1000), Some(0));
    assert_eq!(c.a_of_b(0), Some(1000));
}

#[test]
fn a_plasmid_compared_with_its_reverse_complement_is_the_same_plasmid() {
    let p = puc19();
    let c = circular(&p.sequence, &revcomp(&p.sequence));
    assert!(c.identical());
    // Base 1 of pUC19 pairs with base 2,686 of the flipped copy.
    assert_eq!((c.flipped, c.b_origin), (true, 2685));
    assert_eq!(c.identity(), 1.0);
    assert_eq!(c.b_of_a(0), Some(2685));
    assert_eq!(c.b_of_a(2685), Some(0));
}

#[test]
fn one_substitution_is_one_difference_at_the_base_it_was_made_at() {
    let p = puc19();
    // Base 500 is T (inside the lac operator); the copy has A there.
    assert_eq!(&p.sequence[499..500], "T");
    let c = circular(&p.sequence, &substituted(&p.sequence, 499, 'A'));
    assert!(!c.identical());
    assert_eq!((c.a_length, c.b_length, c.matches, c.aligned_columns), (2686, 2686, 2685, 2686));
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Substitution,
            at_a: Place::At { start: 499, end: 499 },
            at_b: Place::At { start: 499, end: 499 },
            a_bases: "T".into(),
            b_bases: "A".into(),
        }
    );
}

#[test]
fn five_inserted_bases_are_one_insertion_placed_after_the_base_they_follow() {
    let p = puc19();
    // GGACA put in after base 1,200: it starts with a base pUC19 does not have at 1,201 and ends
    // with one it does not have at 1,200, so the insertion can only sit in one place.
    let with = format!("{}GGACA{}", &p.sequence[..1200], &p.sequence[1200..]);
    let c = circular(&p.sequence, &with);
    assert_eq!((c.a_length, c.b_length, c.matches, c.aligned_columns), (2686, 2691, 2686, 2691));
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Insertion,
            at_a: Place::After(1199),
            at_b: Place::At { start: 1200, end: 1204 },
            a_bases: String::new(),
            b_bases: "GGACA".into(),
        }
    );
}

#[test]
fn twenty_deleted_bases_are_one_deletion_naming_the_bases_that_went() {
    let p = puc19();
    // Bases 2,000..2,019 taken out (inside AmpR).
    let gone = &p.sequence[1999..2019];
    assert_eq!(gone, "AACGATCAAGGCGAGTTACA");
    let without = format!("{}{}", &p.sequence[..1999], &p.sequence[2019..]);
    let c = circular(&p.sequence, &without);
    assert_eq!((c.a_length, c.b_length, c.matches, c.aligned_columns), (2686, 2666, 2666, 2686));
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Deletion,
            at_a: Place::At { start: 1999, end: 2018 },
            at_b: Place::After(1998),
            a_bases: gone.into(),
            b_bases: String::new(),
        }
    );
}

#[test]
fn a_turned_copy_with_a_change_reports_the_change_on_both_plasmids_own_bases() {
    let p = puc19();
    // The copy is read from base 1,501 on AND has A where pUC19 has T at base 500. Base 500 of
    // pUC19 is base 500 - 1500 + 2686 = 1,686 of the copy.
    let edited = substituted(&p.sequence, 499, 'A');
    let c = circular(&p.sequence, &turned(&edited, 1500));
    assert_eq!((c.flipped, c.rotated, c.b_origin), (false, true, 1186));
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Substitution,
            at_a: Place::At { start: 499, end: 499 },
            at_b: Place::At { start: 1685, end: 1685 },
            a_bases: "T".into(),
            b_bases: "A".into(),
        }
    );
}

#[test]
fn a_flipped_copy_with_a_change_reports_the_change_on_both_plasmids_own_bases() {
    let p = puc19();
    // pUC19's base 500 is base 2,686 - 500 + 1 = 2,187 of the reverse complement, and the base
    // reported there is the one the comparison read: the complement of what the copy stores.
    let flipped = revcomp(&substituted(&p.sequence, 499, 'A'));
    let c = circular(&p.sequence, &flipped);
    assert!(c.flipped);
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Substitution,
            at_a: Place::At { start: 499, end: 499 },
            at_b: Place::At { start: 2186, end: 2186 },
            a_bases: "T".into(),
            b_bases: "A".into(),
        }
    );
}

#[test]
fn three_changes_at_once_are_three_differences() {
    let p = puc19();
    let mut edited = substituted(&p.sequence, 499, 'A');
    edited = format!("{}GGACA{}", &edited[..1200], &edited[1200..]);
    // 1,999 on pUC19 is 2,004 on the copy, which already carries the five extra bases.
    edited = format!("{}{}", &edited[..2004], &edited[2024..]);
    let c = circular(&p.sequence, &edited);
    assert_eq!(c.diffs.len(), 3);
    assert_eq!(c.count(DiffKind::Substitution), 1);
    assert_eq!(c.count(DiffKind::Insertion), 1);
    assert_eq!(c.count(DiffKind::Deletion), 1);
    assert_eq!(c.diffs[0].at_a, Place::At { start: 499, end: 499 });
    assert_eq!(c.diffs[1].at_b, Place::At { start: 1200, end: 1204 });
    assert_eq!(c.diffs[2].at_a, Place::At { start: 1999, end: 2018 });
    assert_eq!(c.diffs[2].at_b, Place::After(2003));
    // 2,686 columns of pUC19 plus the five extra bases.
    assert_eq!((c.aligned_columns, c.matches), (2691, 2665));
}

#[test]
fn two_plasmids_that_are_not_versions_of_each_other_are_refused_rather_than_described() {
    let p = puc19();
    // A sequence of the same length and base composition that shares nothing with pUC19.
    let other: String = (0..2686).map(|i| ["A", "C", "G", "T"][(i * 7 + i / 4) % 4]).collect::<Vec<_>>().concat();
    let failed = compare(&p.sequence, true, &other, true).unwrap_err();
    assert!(failed.reason.contains("do not line up"), "{}", failed.reason);
    assert!(failed.shared < 600, "shared {} bp", failed.shared);
}

#[test]
fn a_sequence_too_short_to_compare_is_refused() {
    let failed = compare("ACGTACGTACGT", true, "ACGTACGTACGT", true).unwrap_err();
    assert!(failed.reason.contains("at least 24 bases"), "{}", failed.reason);
}

#[test]
fn a_substitution_in_a_coding_feature_says_what_it_does_to_the_protein() {
    let p = puc19();
    // lacZ alpha is complement(146..469), so it reads the other way: base 198 is the middle base
    // of its codon 91, ATT (Ile). The copy's C there makes it AGT, Ser.
    let lacz = feature_at(&p.features, p.len(), 197).map(|i| &p.features[i]).unwrap();
    assert_eq!(lacz.name, "lacZ alpha");
    let effect = substitution_effect(&p, lacz, 197, "C").unwrap();
    assert_eq!(
        effect,
        ProteinEffect::Codons {
            first: 91,
            last: 91,
            from_codons: "ATT".into(),
            to_codons: "AGT".into(),
            from_aa: "I".into(),
            to_aa: "S".into(),
            silent: false,
        }
    );
    assert_eq!(describe_effect(&effect), "codon 91 ATT→AGT, changes the protein: Ile91Ser");

    // Base 200 is the third base of codon 90, GGT (Gly): C there gives GGG, still Gly.
    let silent = substitution_effect(&p, lacz, 199, "C").unwrap();
    assert_eq!(
        silent,
        ProteinEffect::Codons {
            first: 90,
            last: 90,
            from_codons: "GGT".into(),
            to_codons: "GGG".into(),
            from_aa: "G".into(),
            to_aa: "G".into(),
            silent: true,
        }
    );
    assert_eq!(describe_effect(&silent), "codon 90 GGT→GGG, silent");
}

#[test]
fn a_length_change_in_a_coding_feature_says_whether_the_frame_shifts() {
    let p = puc19();
    // AmpR is complement(1626..2486): base 2,000 falls in its codon 163.
    let ampr = p.features.iter().find(|f| f.name == "AmpR").unwrap();
    let shift = indel_effect(&p, ampr, 1999, 20, false).unwrap();
    assert_eq!(shift, ProteinEffect::Frameshift { at_codon: 163, bases: 20, added: false });
    assert_eq!(describe_effect(&shift), "20 bp removed at codon 163: shifts the reading frame from there on");

    let kept = indel_effect(&p, ampr, 1999, 21, true).unwrap();
    assert_eq!(kept, ProteinEffect::InFrame { at_codon: 163, codons: 7, added: true });
    assert_eq!(describe_effect(&kept), "7 codons added at codon 163, in frame");
}

#[test]
fn a_non_coding_feature_has_no_protein_to_change() {
    let p = puc19();
    // Base 500 sits in the lac operator, which is not a CDS.
    let at = feature_at(&p.features, p.len(), 499).map(|i| &p.features[i]).unwrap();
    assert_eq!(at.name, "lac operator");
    assert_eq!(substitution_effect(&p, at, 499, "A"), None);
}

/// Bases from a small linear congruential generator: deterministic, and with no repeats to speak
/// of. Used where what is being tested is the shape of the answer, not the sequence.
fn made_up(n: usize, seed: u64) -> String {
    let mut state = seed;
    (0..n)
        .map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ["A", "C", "G", "T"][(state >> 33) as usize % 4]
        })
        .collect::<Vec<_>>()
        .concat()
}

#[test]
fn a_stretch_that_resembles_nothing_is_reported_as_replaced_not_base_by_base() {
    let p = puc19();
    // Bases 801..1800 of pUC19 swapped for a thousand unrelated ones. Aligned base by base, two
    // unrelated kilobases produce hundreds of chance differences; what is true is that the stretch
    // was replaced.
    let instead = made_up(1_000, 99);
    let c = circular(&p.sequence, &format!("{}{instead}{}", &p.sequence[..800], &p.sequence[1800..]));
    assert_eq!(c.diffs.len(), 2, "{:?}", c.diffs.iter().map(|d| (d.kind, d.length())).collect::<Vec<_>>());
    assert_eq!(c.count(DiffKind::Deletion), 1);
    assert_eq!(c.count(DiffKind::Insertion), 1);
    assert!(c.diffs[0].length() > 900 && c.diffs[1].length() > 900, "{:?}", c.diffs);
}

#[test]
fn a_hundred_substitutions_are_a_hundred_differences() {
    let p = puc19();
    let mut edited = p.sequence.clone().into_bytes();
    let places: Vec<usize> = (0..100).map(|i| 10 + i * 25).collect();
    for &at in &places {
        edited[at] = if edited[at] == b'A' { b'C' } else { b'A' };
    }
    let c = circular(&p.sequence, &String::from_utf8(edited).unwrap());
    assert_eq!(c.diffs.len(), 100);
    assert_eq!(c.count(DiffKind::Substitution), 100);
    assert_eq!(c.matches, 2586);
    let reported: Vec<usize> = c.diffs.iter().filter_map(|d| if let Place::At { start, .. } = d.at_a { Some(start) } else { None }).collect();
    assert_eq!(reported, places);
}

#[test]
fn a_linear_plasmid_is_not_turned_and_its_missing_tail_is_one_deletion() {
    let p = puc19();
    let c = compare(&p.sequence, false, &p.sequence[..2_500], false).unwrap();
    assert_eq!((c.rotated, c.flipped, c.b_origin), (false, false, 0));
    assert_eq!(
        only(&c),
        &Diff {
            kind: DiffKind::Deletion,
            at_a: Place::At { start: 2500, end: 2685 },
            at_b: Place::After(2499),
            a_bases: p.sequence[2500..].to_string(),
            b_bases: String::new(),
        }
    );
}

#[test]
fn a_pair_with_more_differences_than_will_be_held_says_so() {
    // 48 kb with a substitution every 20 bases: 2,400 of them, past the 2,000 that are kept.
    let a = made_up(48_000, 31);
    let mut b = a.clone().into_bytes();
    for i in 0..2_400 {
        let at = i * 20 + 7;
        b[at] = if b[at] == b'A' { b'C' } else { b'A' };
    }
    let c = circular(&a, &String::from_utf8(b).unwrap());
    assert_eq!(c.diffs.len(), bios_core::compare::MAX_DIFFS);
    assert!(c.more_diffs);
}

/// A plasmid pair at a real working size, to show the cost. The sequence is made here rather than
/// committed: what is being measured is the time, not the bases.
#[test]
fn a_large_pair_is_compared_in_good_time() {
    // 12 kb from a small linear congruential generator: deterministic, and no repeats to speak of.
    let mut state: u64 = 0x2026_1123;
    let big: String = (0..12_000)
        .map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            ["A", "C", "G", "T"][(state >> 33) as usize % 4]
        })
        .collect::<Vec<_>>()
        .concat();
    // Turned by 7,000, one base changed at 3,000, five bases in after 6,000, twenty out at 9,000.
    let mut edited = substituted(&big, 2999, if &big[2999..3000] == "A" { 'C' } else { 'A' });
    edited = format!("{}GGACA{}", &edited[..6000], &edited[6000..]);
    edited = format!("{}{}", &edited[..9005], &edited[9025..]);
    let turned_edited = turned(&edited, 7000);

    let t0 = Instant::now();
    let c = circular(&big, &turned_edited);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("12 kb vs 11,985 bp circular, origin moved: {ms:.0} ms, {} difference(s)", c.diffs.len());
    assert!(ms < 2_000.0, "took {ms:.0} ms");
    assert_eq!(c.diffs.len(), 3);
    assert_eq!(c.count(DiffKind::Substitution), 1);
    assert_eq!(c.count(DiffKind::Insertion), 1);
    assert_eq!(c.count(DiffKind::Deletion), 1);
    assert_eq!(c.diffs[0].at_a, Place::At { start: 2999, end: 2999 });
    assert_eq!(c.diffs[1].at_a, Place::After(5999));
    assert_eq!(c.diffs[2].at_a, Place::At { start: 9000, end: 9019 });
    assert!(c.rotated);
    assert_eq!((c.a_length, c.b_length), (12_000, 11_985));
}
