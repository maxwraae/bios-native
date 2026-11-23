//! Two-sided cutters, ported from the web app's `doubleCutters.test.ts` in its order and with its
//! numbers.
//!
//! Hand-transcribed from NEB recognition chart, independently of the vendored REBASE fixture:
//! https://www.neb.com/en-us/tools-and-resources/selection-charts/alphabetized-list-of-recognition-specificities

use bios_core::enzymes::{CleavageStatus, Overhang, OverhangKind, cut_offsets, enzymes, find_cut_sites, get_enzyme};
use bios_core::model::Strand;
use bios_core::seq::revcomp;

struct Case {
    name: &'static str,
    motif: &'static str,
    left: [i64; 2],
    right: [i64; 2],
}

const CASES: [Case; 4] = [
    Case { name: "BcgI", motif: "CGAAAAAAATGC", left: [10, 12], right: [12, 10] },
    Case { name: "BaeI", motif: "ACAAAAGTATC", left: [10, 15], right: [12, 7] },
    Case { name: "CspCI", motif: "CAAAAAAAGTGG", left: [11, 13], right: [12, 10] },
    Case { name: "BsaXI", motif: "ACAAAAACTCC", left: [9, 12], right: [10, 7] },
];
const FLANK: &str = "ATTAATTATATATAATATTAATTATATATAA";

/// The body of the web app's one test per case.
fn both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin(case: &Case) {
    let Case { name, motif, left, right } = case;
    let seq = [FLANK, motif, FLANK].concat();
    let (n, r, l) = (seq.len() as i64, FLANK.len() as i64, motif.len() as i64);
    let forward_pairs = [[r - left[0], r - left[1]], [r + l + right[0], r + l + right[1]]];
    for strand in [Strand::Forward, Strand::Reverse] {
        let dna = if strand == Strand::Forward { seq.clone() } else { revcomp(&seq) };
        let expected = forward_pairs.map(|[top, bottom]| if strand == Strand::Forward { [top, bottom] } else { [n - bottom, n - top] });
        let linear = find_cut_sites(&dna, &[name.to_string(), name.to_lowercase()], false);
        assert_eq!(linear.len(), 2, "{name} {strand:?}");
        for origin in 0..n {
            let rotated = [&dna[origin as usize..], &dna[..origin as usize]].concat();
            let cuts = find_cut_sites(&rotated, &[name], true);
            assert_eq!(cuts.len(), 2);
            for (i, &[top, bottom]) in expected.iter().enumerate() {
                let c = cuts.iter().find(|x| x.cut_index == Some(i + 1)).unwrap();
                let norm = |x: i64| (x - origin).rem_euclid(n) as usize;
                assert_eq!([c.top_cut, c.bottom_cut], [norm(top), norm(bottom)]);
                assert_eq!(c.recognition_start, norm(r));
                assert_eq!(c.strand, strand);
                assert_eq!(c.cuts_per_recognition, Some(2));
                assert_eq!(
                    c.overhang,
                    Overhang {
                        kind: if top < bottom { OverhangKind::FivePrime } else { OverhangKind::ThreePrime },
                        length: (top - bottom).unsigned_abs() as usize,
                        seq: dna[top.min(bottom) as usize..top.max(bottom) as usize].to_string(),
                    }
                );
                let offsets = cut_offsets(c).unwrap();
                assert_eq!(norm(r + offsets.top), c.top_cut);
                assert_eq!(norm(r + offsets.bottom), c.bottom_cut);
            }
        }
    }
}

#[test]
fn bcgi_both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin() {
    both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin(&CASES[0]);
}

#[test]
fn baei_both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin() {
    both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin(&CASES[1]);
}

#[test]
fn cspci_both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin() {
    both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin(&CASES[2]);
}

#[test]
fn bsaxi_both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin() {
    both_cleavage_pairs_strands_overhang_bases_and_every_circular_origin(&CASES[3]);
}

#[test]
fn linear_ends_omit_only_the_unavailable_pair_never_emit_negative_coordinates() {
    let motif = CASES[0].motif;
    let right_only = find_cut_sites(&[motif, FLANK].concat(), &["BcgI"], false);
    let found: Vec<_> = right_only.iter().map(|c| (c.cut_index, c.cuts_per_recognition, c.top_cut, c.bottom_cut)).collect();
    assert_eq!(found, [(Some(2), Some(2), 24, 22)]);
    assert_eq!(find_cut_sites(&[FLANK, motif].concat(), &["BcgI"], false).len(), 1);
    assert_eq!(find_cut_sites(motif, &["BcgI"], false).len(), 0);
}

#[test]
fn palindromic_two_sided_cutters_are_not_scanned_twice_and_aliases_do_not_duplicate_pairs() {
    let cuts = find_cut_sites(&[FLANK, "GCAAAAAAATGC", FLANK].concat(), &["AlfI", "AlfI"], false);
    assert_eq!(cuts.len(), 2);
    assert!(cuts.iter().all(|c| c.strand == Strand::Forward));
    assert_eq!(enzymes().iter().filter(|e| e.cleavage_status() == CleavageStatus::Supported && !e.additional_cuts().is_empty()).count(), 22);
    assert_eq!(get_enzyme("HaeIV").unwrap().cleavage_status(), CleavageStatus::Unverified);
}

#[test]
fn a_reverse_cut_at_the_linear_terminal_gap_stays_at_n_not_zero() {
    let dna = revcomp("CAGTGAA");
    let cuts = find_cut_sites(&dna, &["TspRI"], false);
    let reverse = cuts.iter().find(|c| c.strand == Strand::Reverse).unwrap();
    assert_eq!(reverse.top_cut, dna.len());
    assert_eq!(reverse.bottom_cut, 0);
}

#[test]
fn a_circular_recognition_site_cannot_traverse_the_same_template_bases_twice() {
    assert_eq!(find_cut_sites("GCGC", &["BssHII"], true).len(), 0);
}
