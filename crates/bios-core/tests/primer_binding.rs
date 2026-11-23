//! Comparing a saved primer with its stored site, ported from the web app's
//! `primerBinding.test.ts` in its order and with its numbers.

use bios_core::duplex::AlignRunKind;
use bios_core::model::{Fuzzy, LocationOperator, Segment};
use bios_core::primer_binding::{BindingStatus, PrimerBindingInput, compare_primer_binding};

fn fixture() -> PrimerBindingInput<'static> {
    PrimerBindingInput { sequence: "AACGTCAGTGG", bases: "CGTCAGT", start: 2, end: 8, forward: true, circular: false, ..Default::default() }
}

#[test]
fn stored_oligo_is_compared_independently_of_a_changed_template() {
    let exact = compare_primer_binding(&fixture());
    assert_eq!(exact.status, BindingStatus::Exact);
    assert_eq!(exact.three_prime_matched, Some(true));
    assert!(exact.annealing_tm.is_some());
    let changed = compare_primer_binding(&PrimerBindingInput { sequence: "AACGACAGTGG", ..fixture() });
    assert_eq!(changed.bases, "CGTCAGT");
    assert_eq!(changed.template_bases, "CGACAGT");
    assert_eq!(changed.mismatch_positions, [2]);
    assert_eq!(changed.annealing_tm, None);
    assert_eq!(changed.status, BindingStatus::Changed);
    let mismatches: Vec<(&str, i64)> =
        changed.runs.iter().filter(|r| r.kind == AlignRunKind::Mismatch).map(|r| (r.bases.as_str(), r.start_col)).collect();
    assert_eq!(mismatches, [("T", 4)]);
}

#[test]
fn five_prime_tails_are_not_called_mismatches_and_reverse_bases_remain_in_oligo_order() {
    let check = compare_primer_binding(&PrimerBindingInput { bases: "TTTACTGACG", tail_length: 3, forward: false, ..fixture() });
    assert_eq!(check.status, BindingStatus::Exact);
    assert_eq!(check.tail, "TTT");
    assert_eq!(check.annealing_bases, "ACTGACG");
    assert_eq!(check.template_bases, "ACTGACG");
    assert_eq!(check.mismatch_positions, []);
    let runs: Vec<(AlignRunKind, i64, i64, &str)> = check.runs.iter().map(|r| (r.kind, r.start_col, r.step, r.bases.as_str())).collect();
    assert_eq!(runs, [(AlignRunKind::Overhang, 11, -1, "TTT"), (AlignRunKind::Match, 8, -1, "ACTGACG")]);
}

#[test]
fn circular_reverse_comparison_and_tail_drawing_wrap_at_the_origin() {
    let check = compare_primer_binding(&PrimerBindingInput {
        sequence: "AACCCCGT",
        bases: "GGTTAC",
        start: 6,
        end: 1,
        forward: false,
        tail_length: 2,
        circular: true,
        ..Default::default()
    });
    assert_eq!(check.status, BindingStatus::Exact);
    assert_eq!(check.template_bases, "TTAC");
    assert_eq!(check.runs.iter().map(|r| (r.start_col, r.step)).collect::<Vec<_>>(), [(3, -1), (1, -1)]);
}

#[test]
fn template_insertions_and_deletions_are_visible_without_altering_the_oligo() {
    let plain = |sequence: &'static str, bases: &'static str, end: i64| PrimerBindingInput {
        sequence,
        bases,
        start: 0,
        end,
        forward: true,
        circular: false,
        ..Default::default()
    };
    let longer = compare_primer_binding(&plain("ACGATCAG", "ACGTCAG", 7));
    assert_eq!(longer.deletions, 1);
    assert_eq!(longer.insertions, 0);
    assert_eq!(longer.aligned_primer.as_deref(), Some("ACG-TCAG"));
    assert_eq!(longer.aligned_template.as_deref(), Some("ACGATCAG"));
    assert_eq!(longer.three_prime_matched, Some(true));
    let shorter = compare_primer_binding(&plain("ACGTCAG", "ACGATCAG", 6));
    assert_eq!(shorter.insertions, 1);
    assert_eq!(shorter.deletions, 0);
    assert_eq!(shorter.runs.iter().find(|r| r.kind == AlignRunKind::Insertion).map(|r| r.bases.as_str()), Some("A"));
    let after_end = compare_primer_binding(&plain("ACGTCAGT", "ACGTCAG", 7));
    assert_eq!(after_end.deletions, 1);
    assert_eq!(after_end.three_prime_matched, Some(true), "template beyond the oligo does not unpair its matched 3′ nucleotide");
}

#[test]
fn unpaired_3_prime_ends_and_ambiguous_letters_do_not_receive_a_perfect_duplex_tm() {
    let mismatch = compare_primer_binding(&PrimerBindingInput { bases: "CGTCAGC", ..fixture() });
    assert_eq!(mismatch.three_prime_matched, Some(false));
    assert_eq!(mismatch.mismatch_positions, [6]);
    assert_eq!(mismatch.annealing_tm, None);
    let ambiguous = compare_primer_binding(&PrimerBindingInput { bases: "CGNCAGT", ..fixture() });
    assert_eq!(ambiguous.status, BindingStatus::Changed);
    assert_eq!(ambiguous.ambiguous_positions, [2]);
    assert_eq!(ambiguous.mismatch_positions, []);
    assert_eq!(ambiguous.annealing_tm, None);
}

#[test]
fn unbound_oligos_stay_readable_without_invented_coordinates_or_drawing() {
    let check = compare_primer_binding(&PrimerBindingInput { start: -1, end: -1, unbound: true, ..fixture() });
    assert_eq!(check.status, BindingStatus::Unbound);
    assert_eq!(check.bases, fixture().bases);
    assert_eq!(check.template_bases, "");
    assert_eq!(check.three_prime_matched, None);
    assert_eq!(check.runs, []);
}

#[test]
fn invalid_and_oversized_comparisons_are_explicitly_unverified() {
    for patched in [
        PrimerBindingInput { start: -1, ..fixture() },
        PrimerBindingInput { end: 99, ..fixture() },
        PrimerBindingInput { start: 8, end: 2, ..fixture() },
        PrimerBindingInput { bases: "DNA?", ..fixture() },
        PrimerBindingInput { tail_length: 99, ..fixture() },
    ] {
        assert_eq!(compare_primer_binding(&patched).status, BindingStatus::Unverified, "{patched:?}");
    }
    let (template, oligo) = ("A".repeat(2000), "C".repeat(2000));
    let check = compare_primer_binding(&PrimerBindingInput {
        sequence: &template,
        bases: &oligo,
        start: 0,
        end: 1999,
        forward: true,
        circular: false,
        ..Default::default()
    });
    assert_eq!(check.status, BindingStatus::Unverified);
    assert_eq!(check.aligned_primer, None);
    assert_eq!(check.runs, []);
}

#[test]
fn an_inferred_tail_does_not_become_a_verified_binding_or_temperature_claim() {
    let bases = ["GG", fixture().bases].concat();
    let input = PrimerBindingInput { bases: &bases, tail_length: 2, tail_length_inferred: true, ..fixture() };
    let assumed = compare_primer_binding(&input);
    assert_eq!(assumed.status, BindingStatus::Unverified);
    assert_eq!(assumed.annealing_tm, None);
    // The web app's /inferred.*Confirm tail length/.
    let inferred = assumed.summary.find("inferred").expect("the summary says the tail is inferred");
    assert!(assumed.summary[inferred..].contains("Confirm tail length"), "{}", assumed.summary);
    assert_eq!(assumed.bases, input.bases);
    assert_eq!(assumed.runs[0].kind, AlignRunKind::Overhang, "the saved oligo remains drawable");
    assert_eq!(compare_primer_binding(&PrimerBindingInput { tail_length_inferred: false, ..input }).status, BindingStatus::Exact);
}

#[test]
fn uncertain_and_segmented_locations_cannot_claim_a_perfect_contiguous_binding_even_when_the_envelope_matches() {
    let exact = PrimerBindingInput {
        sequence: "ACGTACGT",
        bases: "ACGTACGT",
        tail_length: 0,
        start: 0,
        end: 7,
        forward: true,
        circular: false,
        ..Default::default()
    };
    let pieces = [Segment::new(0, 2), Segment::new(5, 7)];
    for located in [
        PrimerBindingInput { start_fuzzy: Some(Fuzzy::Before), ..exact },
        PrimerBindingInput { end_fuzzy: Some(Fuzzy::After), ..exact },
        PrimerBindingInput { location_operator: Some(LocationOperator::Order), ..exact },
        PrimerBindingInput { locations: &pieces, ..exact },
        PrimerBindingInput { location_unsupported: Some("1^2"), ..exact },
    ] {
        let check = compare_primer_binding(&located);
        assert_eq!(check.bases, exact.bases);
        assert_eq!(check.status, BindingStatus::Unverified, "{located:?}");
        assert_eq!(check.three_prime_matched, None);
        assert_eq!(check.annealing_tm, None);
        assert_eq!(check.aligned_primer, None);
    }
}
