//! Primer design: pairs that amplify a stretch and pairs that install a change.
//!
//! Designing primers wrongly does not crash: it sends someone to the bench with an oligo that
//! amplifies the wrong thing, or nothing. So every test here names a way the design could be
//! confidently wrong — a product that is not the stretch asked for, a Tm that counts a 5' tail,
//! a mutagenic primer that does not carry its change — and checks the property, not a snapshot.

use bios_core::pcr::PcrPrediction;
use bios_core::primer_design::{
    DEFAULT_PRIMER_CRITERIA, DesignError, MAX_MUTAGENESIS_LENGTH, MIN_MUTAGENESIS_FLANK, Mutagenesis, PrimerCriteria, design_mutagenesis_primers,
    design_primer_pairs, pair_product,
};
use bios_core::primer_quality::cross_dimer;
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

/// The criteria with the boundary pins loosened by `slack` bases and nothing else changed.
fn with_slack(slack: usize) -> PrimerCriteria {
    PrimerCriteria { boundary_slack: slack, ..DEFAULT_PRIMER_CRITERIA }
}

#[test]
fn no_slack_is_the_pinned_design_exactly() {
    let template = puc19();
    // The default, written out, has to be the same answer as the default itself: the slack is the
    // one parameter that could silently move every design already recorded.
    assert_eq!(DEFAULT_PRIMER_CRITERIA.boundary_slack, 0);
    for (start, end) in [(600, 1099), (100, 599)] {
        let pinned = design_primer_pairs(&template, true, start, end, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
        assert_eq!(pinned, design_primer_pairs(&template, true, start, end, &with_slack(0), "", "").unwrap());
        for pair in &pinned {
            assert_eq!((pair.forward_moved, pair.reverse_moved), (0, 0));
        }
    }
}

#[test]
fn slack_cools_a_gc_rich_boundary_no_pinned_primer_can_reach() {
    // The same stretch as `a_gc_rich_boundary_cannot_be_cooled_and_the_reason_says_so`: pUC19
    // reads 78% GC from base 101, so the coolest pinned 18-mer there melts at 64.4 °C. Ten bases
    // of outward slack reach AT-richer bases and bring the forward primer to 60.2 °C.
    let template = puc19();
    let best = |slack: usize| design_primer_pairs(&template, true, 100, 599, &with_slack(slack), "", "").unwrap()[0].clone();

    let pinned = best(0);
    assert_eq!((pinned.forward_moved, pinned.reverse_moved), (0, 0));
    assert_eq!(pinned.forward.tm, Some(64.4));

    let loosened = best(10);
    assert_eq!(loosened.forward_moved, 10, "the forward 5' end takes all ten");
    assert_eq!(loosened.reverse_moved, 8);
    assert_eq!(loosened.forward.start, 90, "ten bases outward of 100");
    assert_eq!(loosened.reverse.end, 607, "eight bases outward of 599");
    assert_eq!(loosened.forward.tm, Some(60.2));
    assert_eq!(loosened.reverse.tm, Some(59.7));
    assert_eq!(loosened.tm_difference, 0.5);
    assert!(loosened.score < pinned.score, "{} vs {}", loosened.score, pinned.score);
    assert!(loosened.reason.contains("meets every criterion") || !loosened.reason.contains("the 60 °C target"), "{}", loosened.reason);

    // More slack, used only as far as it pays: the forward end stops at 15 of the 20 allowed.
    let wide = best(20);
    assert_eq!((wide.forward_moved, wide.reverse_moved), (15, 8));
    assert_eq!(wide.forward.tm, Some(59.6));
}

#[test]
fn slack_moves_the_ends_outward_only_so_the_stretch_asked_for_stays_whole() {
    let template = puc19();
    let n = template.len();
    for slack in [0, 1, 5, 25] {
        for pair in design_primer_pairs(&template, true, 600, 1099, &with_slack(slack), "", "").unwrap() {
            assert!(pair.forward_moved <= slack && pair.reverse_moved <= slack);
            assert_eq!(pair.forward.start, 600 - pair.forward_moved, "the forward 5' end only ever moves outward");
            assert_eq!(pair.reverse.end, 1099 + pair.reverse_moved, "and the reverse 5' end the other way");
            // The product holds every base asked for and exactly as many more as were moved.
            match pair_product(&template, true, &pair) {
                PcrPrediction::Product { product, .. } => {
                    assert_eq!(product.sequence.len(), 500 + pair.forward_moved + pair.reverse_moved);
                    assert!(product.sequence.contains(&template[600..=1099]), "the stretch asked for is whole inside the product");
                }
                PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
            }
        }
        assert!(n > 1099);
    }
}

#[test]
fn a_linear_construct_has_no_slack_past_its_own_ends() {
    let template = puc19();
    let n = template.len();
    // The stretch runs to the last base, so the reverse end cannot move at all and the forward end
    // can move only the 30 bases that lie before it.
    let pairs = design_primer_pairs(&template, false, 30, n - 1, &with_slack(50), "", "").unwrap();
    for pair in &pairs {
        assert!(pair.forward_moved <= 30, "{}", pair.forward_moved);
        assert_eq!(pair.reverse_moved, 0, "there is nothing beyond the last base of a linear construct");
    }
}

#[test]
fn slack_may_not_lap_a_circle() {
    let template = puc19();
    let n = template.len();
    // The stretch is the whole construct but ten bases, so the two ends share ten bases of slack
    // however much is asked for: a product that laps the circle is not a product.
    let pairs = design_primer_pairs(&template, true, 10, 9, &with_slack(100), "", "").unwrap();
    for pair in &pairs {
        assert!(pair.forward_moved + pair.reverse_moved <= 10, "{} + {}", pair.forward_moved, pair.reverse_moved);
        match pair_product(&template, true, pair) {
            PcrPrediction::Product { product, .. } => assert!(product.sequence.len() <= n),
            PcrPrediction::Refused { reasons, .. } => panic!("refused: {reasons:?}"),
        }
    }
}

#[test]
fn slack_widens_what_counts_as_a_stretch_long_enough_for_two_primers() {
    let template = puc19();
    // 21 bases cannot hold two 18-mers, and the refusal counts only the stretch itself.
    let short = design_primer_pairs(&template, true, 100, 120, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap_err();
    assert_eq!(short, DesignError::RegionTooShort { needed: 36, have: 21 });
    // Five bases of slack at each end make 31, still too few for two 18-mers, and the message says
    // the number it actually had to work with rather than the stretch alone.
    let wider = design_primer_pairs(&template, true, 100, 120, &with_slack(5), "", "").unwrap_err();
    assert_eq!(wider, DesignError::RegionTooShort { needed: 36, have: 31 });
    // At ten there are 41, which is room, and the stretch asked for is still whole inside the
    // product. A product of 37 to 57 bases is under the floor the annealing rule applies to, so
    // the pair honestly carries no temperature.
    let pairs = design_primer_pairs(&template, true, 100, 120, &with_slack(10), "", "").unwrap();
    for pair in &pairs {
        assert!(pair.forward.start <= 100 && pair.reverse.end >= 120);
        assert!(pair.forward.anneal.len() + pair.reverse.anneal.len() <= 21 + pair.forward_moved + pair.reverse_moved);
        assert_eq!(pair.annealing, None, "a 37-base product is shorter than the annealing rule applies to");
    }
}

#[test]
fn a_pair_that_cannot_be_designed_says_that_slack_would_help_only_when_there_is_none() {
    // The lopsided synthetic template of `a_stretch_too_short_or_a_pair_too_far_apart_is_refused`:
    // 20% GC at one end and 87% at the other, where no length brings the two within 3 °C.
    let lopsided = format!("ATTTAGATTTAGATTTAGATTTAGATTTAG{}GGCCGCAGGCCGCAGGCCGCAGGCCGCAGG", "TGACCTGTAAGCATGCTAGTCCAATGGTCAGTCATGCATG");
    let pinned = design_primer_pairs(&lopsided, false, 0, lopsided.len() - 1, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap_err();
    assert!(pinned.to_string().contains("boundary_slack would let each end move outward"), "{pinned}");
    // The ends are the construct's own ends, so on a linear construct there is nowhere to move to;
    // the message then does not promise something that cannot help.
    let loosened = design_primer_pairs(&lopsided, false, 0, lopsided.len() - 1, &with_slack(20), "", "").unwrap_err();
    assert!(!loosened.to_string().contains("boundary_slack"), "{loosened}");
}

#[test]
fn every_returned_pair_carries_its_cross_dimer_and_an_annealing_temperature() {
    let template = puc19();
    let pairs = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "", "").unwrap();
    for pair in &pairs {
        // The dimer is the two whole oligos against each other, which is what `cross_dimer` says
        // of them on its own.
        assert_eq!(pair.dimer, cross_dimer(&pair.forward.sequence, &pair.reverse.sequence));
        let a = pair.annealing.expect("a 500-base product and two melting primers have one");
        assert_eq!(a.product_length, 500 + pair.forward_moved + pair.reverse_moved);
        assert_eq!(a.primer_tm, pair.forward.tm.unwrap().min(pair.reverse.tm.unwrap()));
        // The rule, recomputed from the parts the pair reports: 0.3 primer + 0.7 product − 14.9.
        let expected = 0.3 * a.primer_tm + 0.7 * a.product_tm - 14.9;
        assert!((a.celsius - expected).abs() < 0.1, "{} vs {expected}", a.celsius);
        // Hotter than the primers would suggest on their own and well under their Tm: the product
        // term is what moves it, which is the whole reason this rule and not "Tm minus five".
        assert!(a.celsius > 55.0 && a.celsius < a.product_tm, "{}", a.celsius);
    }
    // The best pair over this stretch, recorded: Tm 59.4/59.9 °C primers on a 57.4% GC product.
    let best = &pairs[0];
    let a = best.annealing.unwrap();
    assert_eq!((a.primer_tm, a.product_tm, a.celsius), (59.4, 82.1, 60.4));
    assert_eq!(best.warnings, vec!["forward: 5-bp hairpin stem round a 5-base loop".to_string()]);
}

#[test]
fn a_tail_is_part_of_the_oligo_the_dimer_check_sees() {
    let template = puc19();
    // Two tails that are each other's reverse complement: the oligos now dimerise through their
    // tails, 3' end to 3' end, however well the annealing parts behave.
    let pairs = design_primer_pairs(&template, true, 600, 1099, &DEFAULT_PRIMER_CRITERIA, "GGGGGGGGGG", "CCCCCCCCCC").unwrap();
    let best = &pairs[0];
    assert!(best.dimer.longest >= 10, "{}", best.dimer.longest);
    assert!(best.warnings.iter().any(|w| w.contains("cross-complementarity")), "{:?}", best.warnings);
}
