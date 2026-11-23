//! The product of two primers on a template, ported from the web app's `pcr.test.ts` in its order
//! and with its numbers.

use std::collections::HashSet;

use bios_core::pcr::{
    Bindings, PcrContribution, PcrInput, PcrPrediction, PcrPrimer, PcrProduct, PcrProductPrimerSite, TemplateArc, predict_pcr_product,
};
use bios_core::seq::revcomp;

/// The web app's generator: a 32-bit LCG, two of its high bits a base.
fn dna(length: usize) -> String {
    let mut state: u32 = 123456;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            char::from(b"ACGT"[((state >> 28) & 3) as usize])
        })
        .collect()
}

fn sequence() -> String {
    dna(100)
}

fn slice(seq: &str, start: usize, end: usize) -> String {
    if start <= end { seq[start..=end].to_string() } else { [&seq[start..], &seq[..=end]].concat() }
}

fn primer(seq: &str, start: usize, end: usize, forward: bool, tail: &str) -> PcrPrimer {
    let binding = slice(seq, start, end);
    PcrPrimer {
        id: Some(if forward { "primer-F" } else { "primer-R" }.to_string()),
        name: Some(if forward { "Forward" } else { "Reverse" }.to_string()),
        start: start as i64,
        end: end as i64,
        forward,
        bases: [tail, &if forward { binding } else { revcomp(&binding) }].concat(),
        tail_length: tail.len(),
        ..Default::default()
    }
}

fn input_on(seq: &str, [f_start, f_end, r_start, r_end]: [usize; 4], circular: bool) -> PcrInput {
    PcrInput { sequence: seq.to_string(), circular, forward: primer(seq, f_start, f_end, true, ""), reverse: primer(seq, r_start, r_end, false, "") }
}

/// The web app's `input(fStart, fEnd, rStart, rEnd)` on the linear 100-base template.
fn input_at(sites: [usize; 4]) -> PcrInput {
    input_on(&sequence(), sites, false)
}

/// The web app's `input()`.
fn input() -> PcrInput {
    input_at([5, 24, 70, 89])
}

fn change(base: char) -> char {
    if base == 'A' { 'C' } else { 'A' }
}

fn replace(seq: &str, at: usize, base: char) -> String {
    [&seq[..at], base.to_string().as_str(), &seq[at + 1..]].concat()
}

fn base_at(seq: &str, at: usize) -> char {
    char::from(seq.as_bytes()[at])
}

/// `assert.ok(result.ok)`, and what an accepted prediction carries.
fn accepted(result: PcrPrediction) -> (PcrProduct, Vec<String>, Bindings) {
    match result {
        PcrPrediction::Product { product, caveats, bindings } => (*product, caveats, bindings),
        PcrPrediction::Refused { reasons, .. } => panic!("refused: {}", reasons.join(" ")),
    }
}

/// `assert.equal(result.ok, false)`, and the reasons joined as the web app's tests join them.
fn refused(result: PcrPrediction) -> String {
    match result {
        PcrPrediction::Refused { reasons, .. } => reasons.join(" "),
        PcrPrediction::Product { product, .. } => panic!("predicted {}", product.sequence),
    }
}

/// A regular expression of the form `a.*b`.
fn in_order(text: &str, first: &str, then: &str) -> bool {
    text.find(first).is_some_and(|at| text[at + first.len()..].contains(then))
}

#[test]
fn linear_product_uses_the_complete_selected_sites_and_maps_every_base_without_mutating_inputs() {
    let sequence = sequence();
    let request = input();
    let before = request.clone();
    let result = predict_pcr_product(&request);
    let (product, _, bindings) = accepted(result);
    assert_eq!(product.sequence, sequence[5..90]);
    assert_eq!(product.template_map, (0..85).map(|i| Some(i + 5)).collect::<Vec<_>>());
    assert_eq!(product.template_arc, TemplateArc { start: 5, end: 89, length: 85, wraps_origin: false });
    assert_eq!(bindings.forward.status, bios_core::primer_binding::BindingStatus::Exact);
    assert_eq!(bindings.reverse.status, bios_core::primer_binding::BindingStatus::Exact);
    assert_eq!(
        product.primer_sites.forward,
        PcrProductPrimerSite {
            start: 0,
            end: 19,
            forward: true,
            bases: request.forward.bases.clone(),
            tail_length: 0,
            source_tail_length: 0,
            source_id: Some("primer-F".to_string()),
            name: Some("Forward".to_string()),
        }
    );
    assert_eq!([product.primer_sites.reverse.start, product.primer_sites.reverse.end], [65, 84]);
    assert_eq!(product.contributions.len(), product.sequence.len());
    assert_eq!(request, before);
}

#[test]
fn both_5_prime_tails_are_installed_at_product_ends_and_become_part_of_full_primer_binding_sites() {
    let sequence = sequence();
    let mut request = input();
    request.forward = primer(&sequence, 5, 24, true, "GGCC");
    request.reverse = primer(&sequence, 70, 89, false, "ATGACA");
    let (product, _, _) = accepted(predict_pcr_product(&request));
    assert_eq!(product.sequence, ["GGCC", &sequence[5..90], &revcomp("ATGACA")].concat());
    assert!(product.sequence.starts_with(&request.forward.bases));
    assert!(product.sequence.ends_with(&revcomp(&request.reverse.bases)));
    assert_eq!(product.template_map[..4], [None, None, None, None]);
    assert_eq!(product.template_map[product.template_map.len() - 6..], [None; 6]);
    assert_eq!([product.primer_sites.forward.tail_length, product.primer_sites.forward.source_tail_length], [0, 4]);
    assert_eq!([product.primer_sites.reverse.tail_length, product.primer_sites.reverse.source_tail_length], [0, 6]);
}

#[test]
fn internal_primer_mismatches_are_incorporated_from_oligos_on_both_strands() {
    let sequence = sequence();
    let mut request = input();
    request.forward.bases = replace(&request.forward.bases, 5, change(base_at(&request.forward.bases, 5)));
    request.reverse.bases = replace(&request.reverse.bases, 6, change(base_at(&request.reverse.bases, 6)));
    let (product, caveats, bindings) = accepted(predict_pcr_product(&request));
    assert_eq!(product.sequence, [&request.forward.bases, &sequence[25..70], &revcomp(&request.reverse.bases)].concat());
    assert_eq!(product.template_map[5], Some(10), "primer-installed mutation retains source coordinate");
    assert_eq!(bindings.forward.three_prime_matched, Some(true));
    assert_eq!(bindings.reverse.three_prime_matched, Some(true));
    assert_eq!(caveats.iter().filter(|c| c.contains("substitution")).count(), 2);
}

#[test]
fn circular_origin_arcs_include_wrapping_forward_or_reverse_footprints_without_an_arbitrary_extra_lap() {
    let sequence = sequence();
    for [fs, fe, rs, re] in [[70, 89, 10, 29], [90, 9, 20, 39], [70, 89, 95, 14]] {
        let (product, _, _) = accepted(predict_pcr_product(&input_on(&sequence, [fs, fe, rs, re], true)));
        assert_eq!(product.sequence, [&sequence[fs..], &sequence[..=re]].concat());
        assert!(product.template_arc.wraps_origin);
        assert_eq!(product.template_map, (0..product.sequence.len()).map(|i| Some((fs + i) % sequence.len())).collect::<Vec<_>>());
        for segment in &product.segments {
            if let Some(start) = segment.template_start {
                assert!(segment.template_end.expect("a placed segment ends on the template") >= start);
            }
        }
    }
    let (ordinary, _, _) = accepted(predict_pcr_product(&input_on(&sequence, [10, 29, 40, 59], true)));
    assert_eq!(ordinary.sequence.len(), 50);
}

#[test]
fn inverse_pcr_includes_explicit_terminal_overlap_beyond_one_circular_template_length() {
    let seq = dna(1000);
    let (product, caveats, _) = accepted(predict_pcr_product(&input_on(&seq, [100, 120, 90, 110], true)));
    assert_eq!(product.sequence, [&seq[100..], &seq[..111]].concat());
    assert_eq!(product.sequence.len(), 1011);
    assert_eq!(product.template_arc.length, 1011);
    assert_eq!(product.template_map[..11], product.template_map[product.template_map.len() - 11..]);
    assert!(caveats.join(" ").contains("11 repeated terminal"));
}

#[test]
fn consistent_overlapping_primers_merge_one_shared_sequence_including_a_shared_installed_mutation() {
    let sequence = sequence();
    let mut request = input_at([10, 29, 20, 39]);
    let mutation = change(base_at(&sequence, 25));
    request.forward.bases = replace(&request.forward.bases, 15, mutation);
    request.reverse.bases = replace(&request.reverse.bases, 14, base_at(&revcomp(&mutation.to_string()), 0));
    let (product, _, _) = accepted(predict_pcr_product(&request));
    assert_eq!(product.sequence, replace(&sequence[10..40], 15, mutation));
    assert_eq!(product.contributions.iter().filter(|c| **c == PcrContribution::Overlap).count(), 10);
    assert!(product.sequence.starts_with(&request.forward.bases));
    assert!(product.sequence.ends_with(&revcomp(&request.reverse.bases)));
    let tailed = predict_pcr_product(&PcrInput {
        forward: PcrPrimer { bases: ["GG", &request.forward.bases].concat(), tail_length: 2, ..request.forward.clone() },
        reverse: PcrPrimer { bases: ["AAC", &request.reverse.bases].concat(), tail_length: 3, ..request.reverse.clone() },
        ..request.clone()
    });
    let (tailed, _, _) = accepted(tailed);
    assert_eq!(tailed.sequence, ["GG", &product.sequence, &revcomp("AAC")].concat());
    let (same_footprint, _, _) = accepted(predict_pcr_product(&input_at([10, 29, 10, 29])));
    assert_eq!(same_footprint.sequence, sequence[10..30]);
}

#[test]
fn conflicting_overlapping_primer_bases_are_refused_rather_than_selecting_a_winner() {
    let mut request = input_at([10, 29, 20, 39]);
    request.forward.bases = replace(&request.forward.bases, 15, change(base_at(&sequence(), 25)));
    let reasons = refused(predict_pcr_product(&request));
    assert!(reasons.contains("conflicting bases at template position 26"), "{reasons}");
}

#[test]
fn nonoverlapping_internal_indels_install_full_oligos_and_produce_explicit_insertion_deletion_mapping() {
    let sequence = sequence();
    for inserted_forward in [true, false] {
        let mut request = input_at([10, 29, 70, 89]);
        {
            let (inserted, deleted) =
                if inserted_forward { (&mut request.forward, &mut request.reverse) } else { (&mut request.reverse, &mut request.forward) };
            inserted.bases = [&inserted.bases[..8], "A", &inserted.bases[8..]].concat();
            deleted.bases = [&deleted.bases[..7], &deleted.bases[8..]].concat();
        }
        let (product, caveats, bindings) = accepted(predict_pcr_product(&request));
        assert_eq!(product.sequence, [&request.forward.bases, &sequence[30..70], &revcomp(&request.reverse.bases)].concat());
        let (inserted, deleted) = if inserted_forward { (&bindings.forward, &bindings.reverse) } else { (&bindings.reverse, &bindings.forward) };
        assert_eq!(inserted.insertions, 1);
        assert_eq!(deleted.deletions, 1);
        assert_eq!(product.template_map.iter().filter(|p| p.is_none()).count(), 1);
        assert_eq!(product.template_map.iter().flatten().collect::<HashSet<_>>().len(), 79);
        assert_eq!(product.template_map.len(), product.sequence.len());
        assert!(caveats.join(" ").contains("minimum-edit alignment"));
    }
}

#[test]
fn overlapping_indels_and_terminal_alignment_gaps_have_explicit_unsupported_boundaries() {
    let mut request = input_at([10, 29, 20, 39]);
    request.forward.bases = [&request.forward.bases[..8], "A", &request.forward.bases[8..]].concat();
    let overlapping = refused(predict_pcr_product(&request));
    assert!(in_order(&overlapping, "Overlapping", "gaps"), "{overlapping}");
    let seq = "AACGTCGATCGTACGATCGA";
    let mut terminal = input_on(seq, [1, 5, 12, 18], false);
    terminal.forward.bases = "ACGT".to_string();
    let result = refused(predict_pcr_product(&terminal));
    assert!(result.contains("terminal alignment gap"), "{result}");
}

#[test]
fn unbound_inferred_invalid_orientation_3_prime_mismatch_and_invalid_coordinates_return_both_binding_reports() {
    let sequence = sequence();
    let forward_bases = input().forward.bases;
    let last = forward_bases.len() - 1;
    let unpaired_end = [&forward_bases[..last], change(base_at(&forward_bases, last)).to_string().as_str()].concat();
    let ambiguous = ["N", &forward_bases[1..]].concat();
    // A patch to the forward primer, and the alternatives of the web app's regular expression.
    type Patch<'a> = Box<dyn Fn(&mut PcrPrimer) + 'a>;
    let cases: Vec<(Patch, &[&str])> = vec![
        (
            Box::new(|p| {
                p.unbound = true;
                p.start = -1;
                p.end = -1;
            }),
            &["no stored binding"],
        ),
        (Box::new(|p| p.tail_length_inferred = true), &["inferred"]),
        (Box::new(|p| p.forward = false), &["forward-strand"]),
        (Box::new(|p| p.end = sequence.len() as i64), &["unverified"]),
        (Box::new(|p| p.tail_length = 100), &["unverified"]),
        (Box::new(|p| p.bases = unpaired_end.clone()), &["3′ end", "terminal alignment gap"]),
        (Box::new(|p| p.bases = ambiguous.clone()), &["ambiguous or non-ACGT"]),
    ];
    for (patch, expected) in &cases {
        let mut request = input();
        patch(&mut request.forward);
        let result = predict_pcr_product(&request);
        assert!(!result.ok());
        // Both reports are there whatever the outcome: `bindings` is on both variants.
        let Bindings { forward: _, reverse: _ } = result.bindings();
        let reasons = refused(result);
        assert!(expected.iter().any(|wanted| reasons.contains(wanted)), "{reasons}");
    }
    let outward = refused(predict_pcr_product(&input_at([70, 89, 5, 24])));
    assert!(outward.contains("do not face"), "{outward}");
    let nested = refused(predict_pcr_product(&input_at([10, 39, 20, 29])));
    assert!(nested.contains("extends past"), "{nested}");
    let invalid = predict_pcr_product(&PcrInput { sequence: "NOT!DNA".to_string(), ..input() });
    assert!(!invalid.ok());
}
