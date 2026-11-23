//! Parity with Bios `editor/features.test.ts`: the tests there about how a feature is shown, in
//! the same order, with the same assertions.
//!
//! `isDirectional({ type, name })` is `is_directional(kind, name)`. A colour in the web app is a
//! [`FeatureRole`] here, so the test that maps a colour onto a label class checks the role instead.
//!
//! Left out, because what they test is not in this crate: "the row view is told the same direction
//! the map draws" (the row view's arrowhead and the save path) and everything from "the annotation
//! write path" on (`clampFeature`, `mergeForSave`).

use bios_layout::overview::display::{
    FeatureRole, FileFeature, display_features, humanize_name, is_directional, is_primer_bind, is_source, is_splice_scan, role_for_feature,
};

/// things that are read, transcribed or synthesised are directional
#[test]
fn things_that_are_read_transcribed_or_synthesised_are_directional() {
    for kind in ["CDS", "gene", "mRNA", "promoter", "terminator", "polyA_signal", "primer_bind", "exon", "RBS"] {
        assert!(is_directional(kind, "x"), "{kind} should be directional");
    }
}

/// an enhancer is NOT directional — working either way round is what makes it an enhancer
#[test]
fn an_enhancer_is_not_directional() {
    assert!(!is_directional("enhancer", "CMV enhancer"));
}

/// a protein binding site is not directional — the sites are palindromic because the proteins are dimers
#[test]
fn a_protein_binding_site_is_not_directional() {
    assert!(!is_directional("protein_bind", "lac operator"));
}

/// recombination sites are directional — two loxP the same way excise, opposed they invert
#[test]
fn recombination_sites_are_directional() {
    assert!(is_directional("misc_recomb", "loxP"));
}

/// bookkeeping spans have no direction, which is the whole point of the rule
#[test]
fn bookkeeping_spans_have_no_direction() {
    // These are brackets someone drew round a region, not biology.
    for name in ["CASSETTE 1", "cassette 2", "filler", "eblock1", "eblock2", "oligo hyb", "filler sequence"] {
        assert!(!is_directional("misc_feature", name), "{name} should have no direction");
    }
}

/// a directional thing typed as misc_feature is still caught by name
#[test]
fn a_directional_thing_typed_as_misc_feature_is_still_caught_by_name() {
    // Real files put these through as bare misc_feature constantly.
    for name in ["stop", "KOZAK", "T7 promoter", "SV40 poly A", "linker", "primer 3"] {
        assert!(is_directional("misc_feature", name), "{name} should be directional");
    }
}

/// isDirectional never reads the strand flag
#[test]
fn is_directional_never_reads_the_strand_flag() {
    // Direction is about whether orientation MEANS anything, not which way the file recorded it.
    assert!(!is_directional("misc_feature", "filler"));
    assert!(is_directional("CDS", "AmpR"));
}

/// labelRoleClass maps a feature's arc colour onto its label class
#[test]
fn a_feature_is_drawn_in_the_family_of_its_type() {
    assert_eq!(role_for_feature("CDS", "EGFP"), FeatureRole::Coding);
    assert_eq!(role_for_feature("promoter", "hSYN"), FeatureRole::Promoter);
    // An origin is structural, not its own kind of thing: it answers to the same role as the rest
    // of the backbone.
    assert_eq!(role_for_feature("rep_origin", "ori"), FeatureRole::Structural);
}

/// humanizeName turns a GenBank token into something readable, and changes nothing else
#[test]
fn humanize_name_turns_a_genbank_token_into_something_readable_and_changes_nothing_else() {
    assert_eq!(humanize_name("CMV_enhancer"), "CMV enhancer");
    assert_eq!(humanize_name("  bGH   poly_A "), "bGH poly A");
    assert_eq!(humanize_name(""), "");
}

/// well-known directional parts are caught even when a file types them as misc_feature
#[test]
fn well_known_directional_parts_are_caught_even_when_a_file_types_them_as_misc_feature() {
    for name in ["CAG", "WPRE optional", "AAV2 ITR", "IRES", "T2A"] {
        assert!(is_directional("misc_feature", name), "{name} should be directional");
    }
    // ...but the type still wins where it disagrees: this one is an enhancer whatever it is called.
    assert!(!is_directional("enhancer", "CAG enhancer"));
}

// ---- Not in the original: the rules above it has no tests for, held to what the web app draws ----

/// A name is matched as a whole word, the way the web app's `\b…\b` patterns match it.
#[test]
fn a_name_only_counts_as_a_whole_word() {
    assert!(!is_directional("misc_feature", "restart"), "start inside a word");
    assert!(!is_directional("misc_feature", "exon_2"), "an underscore is part of the word");
    assert!(is_directional("misc_feature", "exon 2"));
    assert!(is_directional("misc_feature", "bGH poly(A) signal"));
    assert!(is_directional("misc_feature", "polyA"));
    assert!(!is_directional("misc_feature", "poly  A"), "one space at most");
}

/// A `misc_feature` is coloured by what its name says it is; anything with a real type keeps it.
/// The colours are the ones the web app drew for an intron cassette.
#[test]
fn a_misc_feature_takes_its_family_from_its_name() {
    assert_eq!(role_for_feature("misc_feature", "eblock for new intron"), FeatureRole::Scaffold);
    assert_eq!(role_for_feature("misc_feature", "gibson homology"), FeatureRole::Scaffold);
    assert_eq!(role_for_feature("misc_feature", "e-block 2"), FeatureRole::Scaffold);
    assert_eq!(role_for_feature("misc_feature", "chimeric intron"), FeatureRole::Sage);
    assert_eq!(role_for_feature("misc_feature", "exon"), FeatureRole::Coding);
    assert_eq!(role_for_feature("misc_feature", "stop"), FeatureRole::Signal);
    assert_eq!(role_for_feature("misc_feature", "linker"), FeatureRole::Structural);
    assert_eq!(role_for_feature("protein_bind", "stop"), FeatureRole::Signal);
    assert_eq!(role_for_feature("CDS", "intron-containing gene"), FeatureRole::Coding);
    assert_eq!(role_for_feature("regulatory", "stop"), FeatureRole::Regulatory);
    assert_eq!(role_for_feature("polyA_signal", "bGH poly(A) signal"), FeatureRole::Regulatory);
}

#[test]
fn what_is_never_drawn_on_the_map() {
    assert!(is_source("source") && is_source("Source") && !is_source("misc_feature"));
    assert!(is_primer_bind("primer_bind") && !is_primer_bind("primer"));
    assert!(is_splice_scan("ESE_ESRseq_SM") && is_splice_scan("5'SS_sm ") && !is_splice_scan("SM") && !is_splice_scan("ESE_SM2"));
}

fn file<'a>(name: &'a str, kind: &'a str, start: i64, end: i64) -> FileFeature<'a> {
    FileFeature { name, kind, start, end, forward: true, locations: None, location_unsupported: false }
}

/// The map is handed the shown features only, under their display names, each remembering which of
/// the file's features it draws.
#[test]
fn the_map_is_handed_the_shown_features() {
    let features = [
        file("source", "source", 0, 999),
        file("CMV_enhancer", "enhancer", 10, 300),
        file("BP1_SM", "regulatory", 400, 406),
        file("fwd", "primer_bind", 500, 520),
        file("exon_1", "misc_feature", 600, 700),
    ];
    let shown = display_features(&features, false);
    assert_eq!(shown.iter().map(|f| (f.name.as_str(), f.source)).collect::<Vec<_>>(), [("CMV enhancer", 1), ("exon 1", 4)]);
    // The family is read from the file's own name and the direction from the shown one.
    assert_eq!((shown[1].role, shown[1].directional), (FeatureRole::Structural, true));
    assert_eq!((shown[0].role, shown[0].directional), (FeatureRole::Regulatory, false));

    let with_scan = display_features(&features, true);
    assert_eq!(with_scan.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["CMV enhancer", "BP1 SM", "exon 1"]);
}
