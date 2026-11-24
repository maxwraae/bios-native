//! Primer design: pairs that amplify a stretch and pairs that install a change.
//!
//! Designing primers wrongly does not crash: it sends someone to the bench with an oligo that
//! amplifies the wrong thing, or nothing. So every test here names a way the design could be
//! confidently wrong — a product that is not the stretch asked for, a Tm that counts a 5' tail,
//! a mutagenic primer that does not carry its change — and checks the property, not a snapshot.

use bios_core::pcr::PcrPrediction;
use bios_core::primer_design::{
    DEFAULT_PRIMER_CRITERIA, DesignError, MAX_MUTAGENESIS_LENGTH, MIN_MUTAGENESIS_FLANK, Mutagenesis, design_mutagenesis_primers,
    design_primer_pairs, pair_product,
};
use bios_core::seq::revcomp;

/// pUC19 (GenBank L09137), the public fixture every DNA test here designs over.
fn puc19() -> String {
    let path = format!("{}/tests/fixtures/pUC19.gb", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).expect("the pUC19 fixture is readable");
    bios_core::genbank::read(&text).expect("pUC19 parses").sequence
}

#[test]
fn a_pair_pins_the_product_to_the_stretch_asked_for() {
    let template = puc19();
    let pairs = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    assert!(!pairs.is_empty() && pairs.len() <= 5);
    for pair in &pairs {
        assert_eq!(pair.forward.start, 600, "the forward primer's 5' end is pinned");
        assert_eq!(pair.reverse.end, 1099, "the reverse primer's 5' end is pinned");
        // The oligo is the template's own bases, forward along the top strand and reverse
        // along the bottom one.
        assert_eq!(pair.forward.anneal, template[600..=pair.forward.end]);
        assert_eq!(pair.reverse.anneal, revcomp(&template[pair.reverse.start..=1099]));
        match pair_product(&template, true, pair) {
            PcrPrediction::Product { product, .. } => {
                assert_eq!(product.sequence, template[600..=1099], "the product is the stretch asked for");
                assert_eq!(product.sequence.len(), 500);
            }
            PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
        }
    }
}

#[test]
fn the_ranking_holds_to_the_criteria_it_names() {
    let template = puc19();
    let pairs = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    let mut scores: Vec<f64> = Vec::new();
    for pair in &pairs {
        let (f, r) = (pair.forward.tm.unwrap(), pair.reverse.tm.unwrap());
        assert!((f - r).abs() <= DEFAULT_PRIMER_CRITERIA.max_tm_difference + 0.05, "{f} vs {r}");
        for p in [&pair.forward, &pair.reverse] {
            assert!((18..=25).contains(&p.length), "{} bases", p.length);
            // A candidate further from the target than the pair may differ is kept, but the
            // reason has to own up to it: that is what makes the list honest.
            let tm = p.tm.unwrap();
            assert!((tm - 60.0).abs() <= 3.0 || pair.reason.contains("the 60 °C target"), "Tm {tm}, reason {}", pair.reason);
        }
        scores.push(pair.score);
    }
    // The best pair is the one that meets the criteria.
    let best = &pairs[0];
    for p in [&best.forward, &best.reverse] {
        let tm = p.tm.unwrap();
        assert!((tm - 60.0).abs() <= 3.0, "the best pair is within tolerance of the target: {tm}");
    }
    assert!(best.reason.ends_with("meets every criterion"), "{}", best.reason);
    assert!(scores.windows(2).all(|w| w[0] <= w[1]), "best first: {scores:?}");
    // Same input, same answer.
    assert_eq!(pairs, design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap());
}

#[test]
fn a_tail_rides_on_the_oligo_and_into_the_product_without_touching_the_tm() {
    let template = puc19();
    let plain = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    let tailed = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "ggaattcc", "aagcttaa").unwrap();
    let (bare, with_tail) = (&plain[0], &tailed[0]);
    assert_eq!(with_tail.forward.anneal, bare.forward.anneal);
    assert_eq!(with_tail.forward.tm, bare.forward.tm, "the tail is not on the template and not in the Tm");
    assert_eq!(with_tail.forward.sequence, format!("GGAATTCC{}", bare.forward.anneal));
    assert_eq!(with_tail.reverse.sequence, format!("AAGCTTAA{}", bare.reverse.anneal));
    assert_eq!(with_tail.forward.length, bare.forward.length + 8);
    match pair_product(&template, true, with_tail) {
        PcrPrediction::Product { product, .. } => {
            assert_eq!(product.sequence, format!("GGAATTCC{}{}", &template[600..=1099], revcomp("AAGCTTAA")));
        }
        PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
    }
}

#[test]
fn a_gc_poor_stretch_keeps_the_length_limits_and_says_the_tm_is_short() {
    // Synthetic: a 20% GC run at each end (the second the reverse complement of the first, so
    // both primers read the same bases and their temperatures match each other exactly) around a
    // GC-rich middle. No 18-25mer at either end reaches 60 °C, and saying so is the honest answer
    // rather than a longer primer.
    let poor = "ATTTAGATTTAGATTTAGATTTAGATTTAG";
    let template = format!("{poor}GCGCATGCCTGCAGGTCGACTCTAGAGGATCCCCGGGTACC{}", revcomp(poor));
    let end = template.len() - 1;
    let pairs = design_primer_pairs(&template, false, 0, end, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    let best = &pairs[0];
    for p in [&best.forward, &best.reverse] {
        assert!((18..=25).contains(&p.length), "the length window is not stretched to reach the target: {} bases", p.length);
        let tm = p.tm.unwrap();
        assert!(tm < 55.0, "a 20% GC end cannot reach 60 °C: {tm}");
    }
    assert_eq!(best.tm_difference, 0.0, "the two ends read the same bases");
    assert!(best.reason.contains("below the 60 °C target"), "{}", best.reason);
    assert!(best.reason.contains("is outside 40–60%"), "{}", best.reason);
}

#[test]
fn a_gc_rich_boundary_cannot_be_cooled_and_the_reason_says_so() {
    // pUC19 reads TCAGGGCGCGTCAGCGGG from base 101: 78% GC, so even the shortest primer allowed
    // melts at 64.4 °C. The 5' end is pinned by the stretch asked for, so the only freedom is
    // length and length only makes it hotter. The pair is still returned — it is the best there
    // is — and its reason says the temperature is above target instead of pretending otherwise.
    let template = puc19();
    let pairs = design_primer_pairs(&template, true, 100, 599, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    let best = &pairs[0];
    assert_eq!(best.forward.length, 18, "the shortest allowed primer is the coolest one here");
    assert_eq!(best.forward.tm, Some(64.4));
    assert!(best.reason.contains("forward Tm 64.4 °C is 4.4 °C above the 60 °C target"), "{}", best.reason);
    assert!(best.reason.contains("forward GC 77.8% is outside 40–60%"), "{}", best.reason);
}

#[test]
fn a_stretch_too_short_or_a_pair_too_far_apart_is_refused_and_says_why() {
    let template = puc19();
    let short = design_primer_pairs(&template, true, 100, 120, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap_err();
    assert_eq!(short, DesignError::RegionTooShort { needed: 36, have: 21 });
    assert!(short.to_string().contains("21 bases long"), "{short}");
    // Synthetic: 20% GC at one end and 87% at the other. No length in the window brings the two
    // temperatures within 3 °C of each other, and there is no honest pair to return.
    let lopsided = format!("ATTTAGATTTAGATTTAGATTTAGATTTAG{}GGCCGCAGGCCGCAGGCCGCAGGCCGCAGG", "TGACCTGTAAGCATGCTAGTCCAATGGTCAGTCATGCATG");
    let apart = design_primer_pairs(&lopsided, false, 0, lopsided.len() - 1, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap_err();
    assert!(matches!(apart, DesignError::NoPair(_)), "{apart:?}");
    assert!(apart.to_string().contains("the closest is"), "{apart}");
}

#[test]
fn a_stretch_through_the_origin_is_designed_on_a_circle_and_refused_on_a_line() {
    let template = puc19();
    let n = template.len();
    let pairs = design_primer_pairs(&template, true, n - 200, 199, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    let best = &pairs[0];
    assert_eq!(best.forward.start, n - 200);
    assert_eq!(best.reverse.end, 199);
    match pair_product(&template, true, best) {
        PcrPrediction::Product { product, .. } => {
            assert_eq!(product.sequence.len(), 400);
            assert_eq!(product.sequence, format!("{}{}", &template[n - 200..], &template[..200]));
        }
        PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
    }
    assert!(design_primer_pairs(&template, false, n - 200, 199, &DEFAULT_PRIMER_CRITERIA, "", "").is_err());
}

#[test]
fn a_substitution_pair_centres_the_mismatch_and_installs_it() {
    let template = puc19();
    let at = 1000;
    let was = &template[at..=at];
    let to = if was == "A" { "G" } else { "A" };
    let d = design_mutagenesis_primers(&template, true, at, &Mutagenesis::substitute(to), &DEFAULT_PRIMER_CRITERIA).unwrap();
    assert_eq!(d.describes, format!("{was}1001{to}"));
    assert_eq!(d.changed_from, was);
    assert_eq!(d.changed_to, to);
    // The change sits in the middle: the flanks differ by at most one base.
    assert!(d.flank_5.abs_diff(d.flank_3) <= 1, "{} vs {}", d.flank_5, d.flank_3);
    assert!(d.flank_5 >= MIN_MUTAGENESIS_FLANK && d.flank_3 >= MIN_MUTAGENESIS_FLANK);
    assert!(d.forward.length <= MAX_MUTAGENESIS_LENGTH);
    assert_eq!(d.reverse.sequence, revcomp(&d.forward.sequence), "the partner is the reverse complement");
    assert!(d.forward.tm.unwrap() >= 60.0, "the flanks grow until the oligo reaches the target: {:?}", d.forward.tm);
    // The oligo is the template with the one base changed.
    assert_eq!(d.forward.sequence, format!("{}{to}{}", &template[at - d.flank_5..at], &template[at + 1..at + 1 + d.flank_3]));
    // The product is the whole construct carrying the change, and nothing else.
    assert_eq!(d.product.len(), template.len());
    assert_eq!(&d.product[at..=at], to);
    assert_eq!(d.product[..at], template[..at]);
    assert_eq!(d.product[at + 1..], template[at + 1..]);
    assert!(d.after_window.contains(&format!("[{to}]")), "{}", d.after_window);
}

#[test]
fn a_substitution_pair_round_trips_through_the_pcr_prediction() {
    let template = puc19();
    let at = 1000;
    let to = if &template[at..=at] == "A" { "G" } else { "A" };
    let d = design_mutagenesis_primers(&template, true, at, &Mutagenesis::substitute(to), &DEFAULT_PRIMER_CRITERIA).unwrap();
    let input = bios_core::pcr::PcrInput {
        sequence: template.clone(),
        circular: true,
        forward: d.forward.as_pcr_primer("forward"),
        reverse: d.reverse.as_pcr_primer("reverse"),
    };
    match bios_core::pcr::predict_pcr_product(&input) {
        PcrPrediction::Product { product, .. } => {
            assert_eq!(product.sequence, d.forward.sequence, "the two oligos agree on the bases they install");
            assert!(product.sequence.contains(to));
        }
        PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
    }
}

#[test]
fn an_insertion_and_a_deletion_carry_their_change_into_the_product() {
    let template = puc19();
    let at = 1200;
    let inserted =
        design_mutagenesis_primers(&template, true, at, &Mutagenesis::insert("GGTACC"), &DEFAULT_PRIMER_CRITERIA).expect("insertion designs");
    assert_eq!(inserted.changed_from, "");
    assert_eq!(inserted.changed_to, "GGTACC");
    assert!(inserted.describes.starts_with("insert 6 bases (GGTACC) before base 1201"), "{}", inserted.describes);
    assert_eq!(inserted.product.len(), template.len() + 6);
    assert_eq!(&inserted.product[at..at + 6], "GGTACC");
    assert!(inserted.forward.sequence.contains("GGTACC"));
    assert_eq!(inserted.forward.sequence, format!("{}GGTACC{}", &template[at - inserted.flank_5..at], &template[at..at + inserted.flank_3]));

    let deleted = design_mutagenesis_primers(&template, true, at, &Mutagenesis::delete(9), &DEFAULT_PRIMER_CRITERIA).expect("deletion designs");
    assert_eq!(deleted.changed_from, template[at..at + 9]);
    assert_eq!(deleted.changed_to, "");
    assert_eq!(deleted.product.len(), template.len() - 9);
    assert_eq!(deleted.product[at..], template[at + 9..]);
    // The oligo joins the two sides with nothing between them.
    assert_eq!(deleted.forward.sequence, format!("{}{}", &template[at - deleted.flank_5..at], &template[at + 9..at + 9 + deleted.flank_3]));
}

#[test]
fn a_mutagenesis_that_cannot_be_designed_says_why() {
    let template = puc19();
    let same =
        design_mutagenesis_primers(&template, true, 1000, &Mutagenesis::substitute(&template[1000..=1000]), &DEFAULT_PRIMER_CRITERIA).unwrap_err();
    assert!(same.to_string().contains("nothing to change"), "{same}");
    let outside = design_mutagenesis_primers(&template, true, template.len(), &Mutagenesis::delete(1), &DEFAULT_PRIMER_CRITERIA);
    assert!(outside.is_err());
    // A linear construct has no bases on the far side of a change at its very end.
    let other = if &template[2..=2] == "A" { "G" } else { "A" };
    let edge = design_mutagenesis_primers(&template, false, 2, &Mutagenesis::substitute(other), &DEFAULT_PRIMER_CRITERIA).unwrap_err();
    assert!(edge.to_string().contains("correct bases on each side"), "{edge}");
}
