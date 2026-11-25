//! Hairpins inside one oligo and dimers between two.
//!
//! Every oligo here is synthetic and short enough that the answer was counted by hand before the
//! code was run; the count is written into the test, with the duplex drawn out in the comment that
//! justifies it. A wrong count here sends someone to the bench with a pair that makes
//! primer-dimer instead of product, so none of these numbers may be adjusted to suit the code.

use bios_core::primer_quality::{cross_dimer, hairpins};

/// A 3' dimer: the last six bases of each oligo pair with the last six of the other, and nothing
/// else in either can pair (neither holds a T, so no A pairs at all).
///
/// ```text
/// 5'-AAAAAAAAAAGGCGGC-3'
///              ||||||
/// 3'-          CCGCCGAAAAAAAAAA-5'
/// ```
#[test]
fn six_bases_of_each_three_prime_end_pair_with_the_other() {
    let d = cross_dimer("AAAAAAAAAAGGCGGC", "AAAAAAAAAAGCCGCC");
    assert_eq!(d.longest, 6);
    assert_eq!(d.three_prime_first, 6);
    assert_eq!(d.three_prime_second, 6);
    assert_eq!(d.diagram, format!("5'-AAAAAAAAAAGGCGGC-3'\n{}||||||\n3'-{}CCGCCGAAAAAAAAAA-5'", " ".repeat(13), " ".repeat(10)));
    assert_eq!(
        d.warnings,
        vec!["6-bp cross-complementarity between the two".to_string(), "6-bp dimer on both 3' ends, which the polymerase extends".to_string()]
    );
}

/// The same six bases of complementarity, moved off both 3' ends. It is still worth a look and no
/// longer worth refusing: a duplex a polymerase cannot extend makes no primer-dimer product.
///
/// ```text
/// 5'-     GGCGGCAAAAAAAAAA-3'
///         ||||||
/// 3'-AAAAACCGCCGAAAAA-5'
/// ```
#[test]
fn the_same_six_bases_away_from_both_three_prime_ends_warn_only_once() {
    let d = cross_dimer("GGCGGCAAAAAAAAAA", "AAAAAGCCGCCAAAAA");
    assert_eq!(d.longest, 6);
    assert_eq!(d.three_prime_first, 0, "the first oligo ends in A and the second holds no T");
    assert_eq!(d.three_prime_second, 0);
    assert_eq!(d.warnings, vec!["6-bp cross-complementarity between the two".to_string()]);
}

/// Four pairs, both of them on the 3' ends: under the limit for complementarity anywhere and at
/// the limit for a duplex the polymerase extends.
///
/// ```text
/// 5'-AAAAAAAAAAAAGGCG-3'
///                ||||
/// 3'-            CGCCAAAAAAAAAAAA-5'
/// ```
#[test]
fn four_pairs_on_the_three_prime_ends_warn_although_four_anywhere_would_not() {
    let d = cross_dimer("AAAAAAAAAAAAGGCG", "AAAAAAAAAAAACGCC");
    assert_eq!(d.longest, 4);
    assert_eq!(d.three_prime_first, 4);
    assert_eq!(d.three_prime_second, 4);
    assert_eq!(d.warnings, vec!["4-bp dimer on both 3' ends, which the polymerase extends".to_string()]);
}

#[test]
fn two_oligos_that_cannot_pair_at_all_have_nothing_against_them() {
    // All A against all G: A pairs T and G pairs C, so not one position pairs.
    let d = cross_dimer("AAAAAAAAAAAAAAAA", "GGGGGGGGGGGGGGGG");
    assert_eq!((d.longest, d.three_prime_first, d.three_prime_second), (0, 0, 0));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert_eq!(d.diagram, "", "there is no register to draw");
}

#[test]
fn an_oligo_against_its_own_reverse_complement_pairs_end_to_end() {
    // The extreme case, as a check that nothing is lost at the ends: 12 of 12.
    let d = cross_dimer("ACGTTGCAATCG", "CGATTGCAACGT");
    assert_eq!(d.longest, 12);
    assert_eq!(d.three_prime_first, 12);
    assert_eq!(d.three_prime_second, 12);
}

/// `GGCGG` and `CCGCC` are each other's reverse complement, so they pair five for five round the
/// four Ts between them. The three Ts on the end pair with nothing (the oligo holds no A).
#[test]
fn a_five_base_pair_stem_round_a_four_base_loop_is_found_where_it_sits() {
    let h = hairpins("GGCGGTTTTCCGCCTTT");
    let worst = h.worst.expect("the stem is found");
    assert_eq!(worst.stem, 5);
    assert_eq!(worst.loop_length, 4);
    assert_eq!(worst.stem_5, (0, 4));
    assert_eq!(worst.stem_3, (9, 13));
    assert!(!worst.at_three_prime, "the last three bases are outside the stem");
    assert_eq!(worst.shape, "[GGCGG]TTTT[CCGCC]TTT");
    assert_eq!(h.three_prime, None);
    assert_eq!(h.warnings, vec!["5-bp hairpin stem round a 4-base loop".to_string()]);
}

/// Three pairs only, but they fold the 3'-terminal base, which is the one the polymerase extends
/// from: `GGG` against the `CCC` the oligo ends with, round four Ts.
#[test]
fn a_three_base_pair_stem_on_the_three_prime_end_warns_although_three_elsewhere_would_not() {
    let h = hairpins("GGGTTTTCCC");
    let worst = h.worst.expect("the stem is found");
    assert_eq!((worst.stem, worst.loop_length), (3, 4));
    assert_eq!(worst.stem_5, (0, 2));
    assert_eq!(worst.stem_3, (7, 9));
    assert!(worst.at_three_prime);
    assert_eq!(worst.shape, "[GGG]TTTT[CCC]");
    assert_eq!(h.three_prime.as_ref().map(|p| (p.stem, p.loop_length)), Some((3, 4)));
    assert_eq!(h.warnings, vec!["hairpin pairs the 3' end: 3-bp stem round a 4-base loop".to_string()]);
}

#[test]
fn an_oligo_that_folds_on_nothing_has_no_hairpin() {
    let h = hairpins("AAAAAAAAAAAAAAAA");
    assert_eq!(h.worst, None);
    assert_eq!(h.three_prime, None);
    assert!(h.warnings.is_empty());
}

#[test]
fn a_palindrome_too_short_to_turn_is_not_a_hairpin() {
    // GGGCCC pairs end to end as a DUPLEX, but folded on itself the only pair with three unpaired
    // bases between its arms is the outermost one, and one base pair is not a stem.
    assert_eq!(hairpins("GGGCCC").worst, None);
    // The same six bases with room to turn do fold.
    assert_eq!(hairpins("GGGTTTCCC").worst.map(|h| (h.stem, h.loop_length)), Some((3, 3)));
}

#[test]
fn an_n_is_never_made_to_pair() {
    // Without the N this is a 5-bp stem; with it, the stem breaks either side of the unknown base.
    let h = hairpins("GGCGGTTTTCCGNC");
    assert_eq!(h.worst.map(|p| p.stem), Some(3), "CGG against CCG, the pair the N would have made left out");
    let d = cross_dimer("AAAAAAAAAAGGCGGC", "AAAAAAAAAANCCGCC");
    assert_eq!(d.longest, 5, "the N in the second oligo breaks the sixth pair");
}
