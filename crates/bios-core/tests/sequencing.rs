//! Sanger primers tiled across a construct.
//!
//! Designing these wrongly does not crash: it sends someone a set of primers whose reads leave a
//! hole nobody notices, or one that binds twice and gives a trace of two sequences at once. So
//! every test here names a way the plan could be confidently wrong — a gap passed off as covered,
//! a primer that is not the construct's own bases, a repeat read with one primer — and checks it
//! against pUC19 or a construct built to break it.

use bios_core::primer::{PrimerSearchOptions, find_primer_sites};
use bios_core::primer_design::{DEFAULT_PRIMER_CRITERIA, PrimerCriteria};
use bios_core::seq::revcomp;
use bios_core::sequencing::{DEFAULT_SEQUENCING_SETTINGS, SequencingSettings, StrandPlan, design_sequencing_primers};

/// pUC19 (GenBank L09137), 2686 bp circular.
fn puc19() -> String {
    let path = format!("{}/tests/fixtures/pUC19.gb", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(path).expect("the pUC19 fixture is readable");
    bios_core::genbank::read(&text).expect("pUC19 parses").sequence
}

/// How many bases `start..=end` covers, through the origin when `end < start`.
fn span(start: usize, end: usize, n: usize) -> usize {
    if end >= start { end - start + 1 } else { n - start + end + 1 }
}

/// Every base of the construct the reads of one strand resolve, as a set.
fn resolved(plan: &StrandPlan, n: usize) -> Vec<bool> {
    let mut seen = vec![false; n];
    for read in &plan.reads {
        for k in 0..span(read.read_start, read.read_end, n) {
            if let Some(slot) = seen.get_mut((read.read_start + k) % n) {
                *slot = true;
            }
        }
    }
    seen
}

/// Every primer of a plan is the construct's own bases at its footprint, on the strand it says,
/// and anneals fully and perfectly in exactly one place.
fn check_primers(plan: &StrandPlan, template: &str, circular: bool) {
    let n = template.len();
    for read in &plan.reads {
        let footprint: String =
            (0..span(read.primer.start, read.primer.end, n)).map(|k| template.as_bytes()[(read.primer.start + k) % n] as char).collect();
        let expected = if read.primer.forward { footprint } else { revcomp(&footprint) };
        assert_eq!(read.primer.sequence, expected, "the oligo is the construct's own bases at {}..{}", read.primer.start, read.primer.end);
        assert_eq!(read.primer.forward, plan.forward);
        let options = PrimerSearchOptions { circular, ..PrimerSearchOptions::default() };
        let sites = find_primer_sites(template, &read.primer.sequence, options).unwrap();
        let full = sites.iter().filter(|s| s.mismatches == 0 && s.anneal_length == read.primer.sequence.len()).count();
        assert_eq!(full, 1, "{} binds fully and perfectly in {full} places", read.primer.sequence);
        assert_eq!(read.other_sites, sites.len() - 1);
        // The read begins at the base after the 3' end, on the primer's own strand.
        let three_prime = if read.primer.forward { read.primer.end } else { read.primer.start };
        let begins = if read.primer.forward { (three_prime + 1) % n } else { (three_prime + n - 1) % n };
        assert_eq!(if read.primer.forward { read.read_start } else { read.read_end }, begins);
    }
}

#[test]
fn four_primers_read_the_whole_of_puc19_on_one_strand() {
    let template = puc19();
    let n = template.len();
    let plan = design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).unwrap();
    assert_eq!(plan.requested, 2686);
    assert_eq!(plan.strands.len(), 1, "one strand unless both are asked for");
    let forward = &plan.strands[0];
    assert!(forward.forward);
    // 2686 bases at 800 a read, overlapping 100, is four reads.
    assert_eq!(forward.reads.len(), 4);
    assert_eq!(forward.covered, 2686, "every base of the plasmid");
    assert!(forward.gaps.is_empty(), "{:?}", forward.gaps);
    assert!(plan.rejected.is_empty(), "no candidate had to be refused on pUC19: {:?}", plan.rejected);
    assert!(resolved(forward, n).into_iter().all(|base| base), "every base is inside some read");
    check_primers(forward, &template, true);

    // Recorded: the four oligos this plan picks, 0-based footprints. The first sits just before
    // base 1 (so its read starts there) and the stretch wraps the origin like any other.
    let picked: Vec<(usize, usize, usize, Option<f64>)> =
        forward.reads.iter().map(|r| (r.primer.start, r.primer.end, r.primer.length, r.primer.tm)).collect();
    assert_eq!(picked, vec![(2660, 2682, 23, Some(60.2)), (675, 694, 20, Some(59.9)), (1365, 1388, 24, Some(60.1)), (2046, 2069, 24, Some(60.3))]);
    for read in &forward.reads {
        let tm = read.primer.tm.unwrap();
        assert!((tm - 60.0).abs() <= 3.0, "the melting temperature is the gate: {tm}");
        assert!((18..=25).contains(&read.primer.length));
    }
    // Every read after the first shares at least the overlap asked for with the one before it.
    for read in forward.reads.iter().skip(1) {
        assert!(read.overlap_with_previous >= 100, "{} bases of overlap", read.overlap_with_previous);
    }
    assert_eq!(forward.reads[0].overlap_with_previous, 0, "there is nothing before the first read");
}

#[test]
fn both_strands_read_the_whole_of_puc19_each_way() {
    let template = puc19();
    let n = template.len();
    let settings = SequencingSettings { both_strands: true, ..DEFAULT_SEQUENCING_SETTINGS };
    let plan = design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &settings).unwrap();
    assert_eq!(plan.strands.len(), 2);
    for strand in &plan.strands {
        assert_eq!(strand.reads.len(), 4);
        assert_eq!(strand.covered, 2686);
        assert!(strand.gaps.is_empty(), "{:?}", strand.gaps);
        assert!(resolved(strand, n).into_iter().all(|base| base));
        check_primers(strand, &template, true);
    }
    assert!(plan.strands[0].forward && !plan.strands[1].forward);
    // The bottom strand's primers are the reverse complement of the plasmid at their footprints,
    // which `check_primers` has already held them to, and they read the other way.
    let reverse = &plan.strands[1];
    for read in &reverse.reads {
        assert!(!read.primer.forward);
        assert!(span(read.read_start, read.read_end, n) == 800);
    }
}

#[test]
fn a_range_of_a_plasmid_is_tiled_without_reading_the_rest_of_it() {
    let template = puc19();
    let plan = design_sequencing_primers(&template, true, 399, 1599, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).unwrap();
    assert_eq!((plan.start, plan.end, plan.requested), (399, 1599, 1201));
    let forward = &plan.strands[0];
    // 1201 bases at 800 a read, overlapping 100, is two reads.
    assert_eq!(forward.reads.len(), 2);
    assert_eq!(forward.covered, 1201);
    assert!(forward.gaps.is_empty(), "{:?}", forward.gaps);
    check_primers(forward, &template, true);
    // The first read begins at the first base asked for or just before it, never after.
    let first = &forward.reads[0];
    assert!(first.read_start <= 399, "the first read starts at or before base 400: {}", first.read_start);
    // And the last read reaches the last base asked for.
    let last = forward.reads.last().unwrap();
    assert!(last.read_end >= 1599, "{}", last.read_end);
}

#[test]
fn a_primer_that_would_bind_twice_is_refused_and_the_stretch_it_would_have_read_is_a_gap() {
    // A construct carrying one 600-base stretch of pUC19 twice. No oligo taken from inside that
    // stretch binds once, so the second copy cannot be read with a unique primer at all.
    let puc19 = puc19();
    let repeat = &puc19[0..600];
    let template = format!("{repeat}{}{repeat}{}", &puc19[600..1400], &puc19[1400..2200]);
    assert_eq!(template.len(), 2800);
    let settings = SequencingSettings { read_length: 300, overlap: 50, ..DEFAULT_SEQUENCING_SETTINGS };
    let plan = design_sequencing_primers(&template, true, 0, template.len() - 1, &DEFAULT_PRIMER_CRITERIA, &settings).unwrap();
    let forward = &plan.strands[0];

    assert!(!plan.rejected.is_empty(), "candidates inside the repeat have to be refused");
    let doubled = plan.rejected.iter().filter(|r| r.why.contains("binds fully and perfectly in 2 places")).count();
    assert!(doubled > 0, "{:?}", plan.rejected);
    // Each refused oligo really does occur twice in the construct.
    for refused in plan.rejected.iter().filter(|r| r.why.contains("2 places")) {
        assert_eq!(template.matches(&refused.sequence).count(), 2, "{}", refused.sequence);
    }
    assert!(!forward.gaps.is_empty(), "the repeat cannot be covered and the plan has to say so");
    assert!(forward.covered < plan.requested, "{} of {}", forward.covered, plan.requested);
    // And the gap blames the repeat, not something else: every candidate that met the criteria
    // there bound twice, and the reason says exactly that and nothing about temperature.
    assert!(
        forward.gaps.iter().any(|g| g.why.contains("all bind the construct in more than one place")),
        "the gap names the reason: {:?}",
        forward.gaps.iter().map(|g| &g.why).collect::<Vec<_>>()
    );
    assert!(forward.gaps.iter().all(|g| !g.why.contains("°C target")), "no gap here is the temperature's fault: {:?}", forward.gaps);
    // What was read is still read properly: every primer in the plan binds once.
    check_primers(forward, &template, true);
    // Nothing is claimed as covered that no read reaches.
    let seen = resolved(forward, template.len());
    for gap in &forward.gaps {
        for k in 0..span(gap.start, gap.end, template.len()) {
            assert!(!seen[(gap.start + k) % template.len()], "base {} is in a gap and in a read", gap.start + k);
        }
    }
}

#[test]
fn a_tract_too_at_rich_for_any_primer_is_a_gap_that_says_so() {
    // 402 bases of AAT: no 18 to 25mer of it comes within 3 °C of 60, and the search window is
    // shorter than the tract, so there is nowhere inside it to take a primer from.
    let puc19 = puc19();
    let tract = "AAT".repeat(134);
    let template = format!("{}{tract}{}", &puc19[0..600], &puc19[600..1200]);
    let settings = SequencingSettings { read_length: 200, overlap: 50, ..DEFAULT_SEQUENCING_SETTINGS };
    let plan = design_sequencing_primers(&template, false, 0, template.len() - 1, &DEFAULT_PRIMER_CRITERIA, &settings).unwrap();
    let forward = &plan.strands[0];
    assert_eq!(plan.requested, 1602);
    assert!(!forward.gaps.is_empty(), "the AT tract cannot be read through");

    // Two gaps, and neither of them is silent. The first is the 21 bases the first primer itself
    // sits on: a read begins after its primer, and a linear construct has nothing upstream.
    let head = &forward.gaps[0];
    assert_eq!((head.start, head.end), (0, 20));
    assert!(head.why.contains("need a primer upstream of the stretch asked for"), "{}", head.why);

    // The second is the AT tract (bases 601 to 1002, 1-based), where no 18 to 25mer melts within
    // 3 °C of 60: the closest anywhere the tiler looked is a 25mer at 56 °C. The window it names is
    // the one it actually searched — 149 bases back, one short of the 150-base stride so that every
    // read moves on — and not the 150 that was asked for.
    let tract_gap = &forward.gaps[1];
    assert!(tract_gap.start >= 600 && tract_gap.start < 600 + tract.len(), "the gap is at the tract: {}..{}", tract_gap.start, tract_gap.end);
    assert!(tract_gap.why.contains("no acceptable primer near base 761 (149 bases back, 150 forward)"), "{}", tract_gap.why);
    assert!(tract_gap.why.contains("further from the 60 °C target than the 3 °C allowed"), "{}", tract_gap.why);
    assert!(tract_gap.why.contains("25-base oligo at Tm 56 °C"), "{}", tract_gap.why);
    assert!(!tract_gap.why.contains("bind the construct"), "nothing here was refused for binding: {}", tract_gap.why);

    // The pUC19 halves either side of the tract are read, and what was read was read properly.
    assert_eq!(forward.covered, 1367);
    assert_eq!(forward.covered, plan.requested - forward.gaps.iter().map(|g| g.end - g.start + 1).sum::<usize>());
    check_primers(forward, &template, false);
}

#[test]
fn settings_that_cannot_work_are_refused_with_a_reason() {
    let template = puc19();
    let n = template.len();
    let bad = |settings: SequencingSettings| design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &settings).unwrap_err();
    let overlapping = bad(SequencingSettings { read_length: 300, overlap: 300, ..DEFAULT_SEQUENCING_SETTINGS });
    assert!(overlapping.to_string().contains("never moves on"), "{overlapping}");
    let huge = bad(SequencingSettings { read_length: 5000, ..DEFAULT_SEQUENCING_SETTINGS });
    assert!(huge.to_string().contains("shorten read_length"), "{huge}");
    // A stretch outside the construct, and one through the origin of a linear one.
    assert!(design_sequencing_primers(&template, true, 0, n, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).is_err());
    let wrapped = design_sequencing_primers(&template, false, 2000, 200, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).unwrap_err();
    assert!(wrapped.to_string().contains("linear"), "{wrapped}");
}

#[test]
fn the_overlap_asked_for_is_what_the_reads_share() {
    let template = puc19();
    let n = template.len();
    for overlap in [0usize, 50, 300] {
        let settings = SequencingSettings { read_length: 700, overlap, ..DEFAULT_SEQUENCING_SETTINGS };
        let plan = design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &settings).unwrap();
        let forward = &plan.strands[0];
        assert_eq!(forward.covered, 2686, "overlap {overlap} still covers everything");
        assert!(forward.gaps.is_empty(), "overlap {overlap}: {:?}", forward.gaps);
        for read in forward.reads.iter().skip(1) {
            assert!(read.overlap_with_previous >= overlap, "overlap {overlap}: got {}", read.overlap_with_previous);
        }
        // A wider overlap means more reads, never fewer.
        assert!(forward.reads.len() >= 2686usize.div_ceil(700));
    }
}

#[test]
fn a_plan_is_the_same_answer_every_time_and_touches_nothing() {
    let template = puc19();
    let n = template.len();
    let once = design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).unwrap();
    let twice = design_sequencing_primers(&template, true, 0, n - 1, &DEFAULT_PRIMER_CRITERIA, &DEFAULT_SEQUENCING_SETTINGS).unwrap();
    assert_eq!(once.strands, twice.strands);
    // A tighter temperature band is a different, stricter answer, not an error.
    let strict = PrimerCriteria { max_tm_difference: 1.0, ..DEFAULT_PRIMER_CRITERIA };
    let tight = design_sequencing_primers(&template, true, 0, n - 1, &strict, &DEFAULT_SEQUENCING_SETTINGS).unwrap();
    for read in &tight.strands[0].reads {
        assert!((read.primer.tm.unwrap() - 60.0).abs() <= 1.0, "{:?}", read.primer.tm);
    }
    assert_eq!(tight.strands[0].covered, 2686);
}
