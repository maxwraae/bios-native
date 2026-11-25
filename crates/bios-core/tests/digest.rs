//! Digest and ligation answers, recorded against the published pUC19 sequence (GenBank L09137,
//! 2686 bp circular, `tests/fixtures/pUC19.gb`).
//!
//! Every pUC19 number below was worked out from that sequence and the enzymes' published cut
//! positions, not from this code:
//!
//! | enzyme | site, 1-based | cut | top scission | bottom scission | end |
//! |---|---|---|---|---|---|
//! | EcoRI | GAATTC 396..401 | G^AATTC | after 396 | after 400 | 5' AATT (397..400) |
//! | BamHI | GGATCC 417..422 | G^GATCC | after 417 | after 421 | 5' GATC (418..421) |
//! | PstI | CTGCAG 435..440 | CTGCA^G | after 439 | after 435 | 3' TGCA (436..439) |
//! | HindIII | AAGCTT 447..452 | A^AGCTT | after 447 | after 451 | 5' AGCT (448..451) |
//! | SmaI | CCCGGG 412..417 | CCC^GGG | after 414 | after 414 | blunt |
//!
//! Those are the only sites each of those enzymes has in pUC19; NotI (GCGGCCGC) and EcoRV
//! (GATATC) have none at all. BsaI has one, read on the bottom strand: GAGACC at 1766..1771 on
//! the top strand, so GGTCTC(1/5) counted from the bottom strand's 3' side cuts at 1771 - 11 = 1760
//! on the top strand and 1771 - 7 = 1764 on the bottom, which is upstream of and outside its own
//! recognition site.

use std::path::Path;

use bios_core::digest::{DigestError, Fragment, Piece, can_self_close, digest, fragment_features, joins, ligate};
use bios_core::enzymes::OverhangKind;
use bios_core::{Construct, genbank};

fn puc19() -> Construct {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pUC19.gb");
    genbank::read(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A fragment as `(length, 1-based start, 1-based end, through the origin)`.
fn shape(f: &Fragment) -> (usize, usize, usize, bool) {
    (f.length, f.start + 1, f.end + 1, f.wraps_origin)
}

fn ends(f: &Fragment) -> (Option<&'static str>, String, Option<&'static str>, String) {
    let text = |end: &bios_core::digest::FragmentEnd| match &end.overhang {
        None => "as it came".to_string(),
        Some(o) => match o.kind {
            OverhangKind::Blunt => "blunt".to_string(),
            OverhangKind::FivePrime => format!("5' {}", o.seq),
            OverhangKind::ThreePrime => format!("3' {}", o.seq),
        },
    };
    (f.left.enzyme, text(&f.left), f.right.enzyme, text(&f.right))
}

#[test]
fn one_cut_in_a_circular_plasmid_gives_one_linear_fragment() {
    let c = puc19();
    assert_eq!(c.len(), 2686);
    let d = digest(&c.sequence, true, &["EcoRI"]).unwrap();
    assert_eq!(d.cuts.len(), 1);
    let cut = d.cuts.first().unwrap();
    assert_eq!((cut.enzyme, cut.top_cut, cut.bottom_cut, cut.recognition_start + 1), ("EcoRI", 396, 400, 396));
    assert_eq!(cut.overhang.seq, "AATT");
    assert_eq!(cut.overhang.kind, OverhangKind::FivePrime);

    assert_eq!(d.fragments.len(), 1, "one cut, one fragment");
    let f = d.fragments.first().unwrap();
    assert_eq!(shape(f), (2686, 397, 396, true));
    assert_eq!(ends(f), (Some("EcoRI"), "5' AATT".into(), Some("EcoRI"), "5' AATT".into()));
    assert_eq!(f.sequence.len(), 2686);
    // The whole plasmid, read from the cut: the last base before the cut comes last.
    assert!(f.sequence.starts_with("AATTC"));
    assert!(f.sequence.ends_with('G'));
}

#[test]
fn ecori_and_psti_give_2643_and_43() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["EcoRI", "PstI"]).unwrap();
    assert_eq!(d.fragments.len(), 2);
    assert_eq!(d.fragments.iter().map(|f| f.length).collect::<Vec<_>>(), vec![2643, 43], "longest first, as a gel runs them");
    assert_eq!(d.fragments.iter().map(|f| f.length).sum::<usize>(), 2686);
    assert_eq!(d.total, 2686);

    let big = d.fragments.first().unwrap();
    assert_eq!(shape(big), (2643, 440, 396, true));
    assert_eq!(ends(big), (Some("PstI"), "3' TGCA".into(), Some("EcoRI"), "5' AATT".into()));

    let small = d.fragments.get(1).unwrap();
    assert_eq!(shape(small), (43, 397, 439, false));
    assert_eq!(ends(small), (Some("EcoRI"), "5' AATT".into(), Some("PstI"), "3' TGCA".into()));
    assert_eq!(small.sequence, "AATTCGAGCTCGGTACCCGGGGATCCTCTAGAGTCGACCTGCA");
}

#[test]
fn three_enzymes_across_the_polylinker_add_up_to_the_plasmid() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["EcoRI", "BamHI", "HindIII"]).unwrap();
    assert_eq!(d.cuts.iter().map(|cut| (cut.enzyme, cut.top_cut)).collect::<Vec<_>>(), vec![("EcoRI", 396), ("BamHI", 417), ("HindIII", 447)]);
    assert_eq!(d.fragments.iter().map(shape).collect::<Vec<_>>(), vec![(2635, 448, 396, true), (30, 418, 447, false), (21, 397, 417, false)]);
    assert_eq!(d.fragments.iter().map(|f| f.length).sum::<usize>(), 2686);
}

#[test]
fn an_enzyme_with_no_site_is_named_and_cuts_nothing() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["NotI", "EcoRV"]).unwrap();
    assert_eq!(d.non_cutters, vec!["NotI", "EcoRV"]);
    assert!(d.cuts.is_empty());
    assert!(d.uncut_circle, "nothing cut it, so it is still a circle and not a fragment");
    assert!(d.fragments.is_empty());

    let one = digest(&c.sequence, true, &["EcoRI", "NotI"]).unwrap();
    assert_eq!(one.non_cutters, vec!["NotI"]);
    assert_eq!(one.fragments.len(), 1);
}

#[test]
fn a_blunt_cutter_leaves_blunt_ends() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["SmaI"]).unwrap();
    let cut = d.cuts.first().unwrap();
    assert_eq!((cut.top_cut, cut.bottom_cut, cut.recognition_start + 1), (414, 414, 412));
    let f = d.fragments.first().unwrap();
    assert_eq!(shape(f), (2686, 415, 414, true));
    assert_eq!(ends(f), (Some("SmaI"), "blunt".into(), Some("SmaI"), "blunt".into()));
}

#[test]
fn a_type_iis_enzyme_cuts_outside_its_recognition_site() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["BsaI"]).unwrap();
    assert_eq!(d.cuts.len(), 1);
    let cut = d.cuts.first().unwrap();
    assert_eq!(cut.strand, bios_core::Strand::Reverse);
    assert_eq!(cut.recognition_start + 1, 1766, "GAGACC on the top strand is GGTCTC read on the bottom");
    assert_eq!((cut.top_cut, cut.bottom_cut), (1760, 1764), "the cut is upstream of the site, not inside it");
    assert!(cut.top_cut < cut.recognition_start, "a Type IIS enzyme on the bottom strand cuts before its own site");
    assert_eq!(cut.overhang.kind, OverhangKind::FivePrime);
    assert_eq!(cut.overhang.length, 4);

    let two = digest(&c.sequence, true, &["EcoRI", "BsaI"]).unwrap();
    assert_eq!(two.fragments.iter().map(shape).collect::<Vec<_>>(), vec![(1364, 397, 1760, false), (1322, 1761, 396, true)]);
    assert_eq!(two.fragments.iter().map(|f| f.length).sum::<usize>(), 2686);
}

#[test]
fn a_linear_molecule_keeps_the_ends_it_came_with() {
    let c = puc19();
    // The polylinker on its own, as if it had been cut out: EcoRI..HindIII is 51 bp.
    let stretch = c.sequence.get(396..447).unwrap();
    assert_eq!(stretch.len(), 51);
    let d = digest(stretch, false, &["BamHI"]).unwrap();
    assert_eq!(d.fragments.iter().map(shape).collect::<Vec<_>>(), vec![(30, 22, 51, false), (21, 1, 21, false)]);
    let first = d.fragments.iter().find(|f| f.start == 0).unwrap();
    assert_eq!(ends(first), (None, "as it came".into(), Some("BamHI"), "5' GATC".into()));
    let second = d.fragments.iter().find(|f| f.start == 21).unwrap();
    assert_eq!(ends(second), (Some("BamHI"), "5' GATC".into(), None, "as it came".into()));
    // Circular, the same one cut would give one fragment and not two.
    assert_eq!(digest(stretch, true, &["BamHI"]).unwrap().fragments.len(), 1);
}

#[test]
fn a_name_bios_cannot_vouch_for_is_refused_not_guessed_at() {
    let c = puc19();
    assert_eq!(digest(&c.sequence, true, &["NotAnEnzyme"]), Err(DigestError::UnknownEnzyme("NotAnEnzyme".into())));
    let no_enzymes: [&str; 0] = [];
    assert_eq!(digest(&c.sequence, true, &no_enzymes), Err(DigestError::NoEnzymes));
    // An alias resolves to the catalog's own spelling.
    assert_eq!(digest(&c.sequence, true, &["ecori"]).unwrap().cuts.first().unwrap().enzyme, "EcoRI");
    // One enzyme named twice is one enzyme.
    assert_eq!(digest(&c.sequence, true, &["EcoRI", "ecori"]).unwrap().cuts.len(), 1);
}

#[test]
fn features_are_sorted_into_the_ones_that_survive_and_the_ones_cut_through() {
    let c = puc19();
    let d = digest(&c.sequence, true, &["EcoRI", "PstI"]).unwrap();
    let name = |i: &usize| c.features.get(*i).map(|f| f.name.clone()).unwrap_or_default();

    let small = fragment_features(d.fragments.get(1).unwrap(), &c.features, c.len());
    assert!(small.inside.is_empty(), "nothing fits whole in 43 bp of polylinker");
    assert_eq!(
        small.clipped.iter().map(name).collect::<Vec<_>>(),
        vec!["synthetic DNA construct", "lacZ alpha", "MCS"],
        "the 43 bp piece is cut out of these three"
    );

    let big = fragment_features(d.fragments.first().unwrap(), &c.features, c.len());
    let kept: Vec<String> = big.inside.iter().map(|carried| name(&carried.index)).collect();
    assert_eq!(kept, vec!["M13 fwd", "M13 rev", "lac operator", "lac promoter", "CAP binding site", "ori", "AmpR", "AmpR promoter"]);
    assert_eq!(big.clipped.iter().map(name).collect::<Vec<_>>(), vec!["synthetic DNA construct", "lacZ alpha", "MCS"]);
    // M13 fwd is 379..395 on the plasmid; the fragment starts at 440, so it lands 2626 bases in.
    let m13 = big.inside.first().unwrap();
    assert_eq!((m13.start + 1, m13.end + 1), (2626, 2642));
    assert_eq!(2686 - 440 + 379, 2625, "the offset M13 fwd's start moves by");
}

#[test]
fn ends_ligate_only_when_their_single_stranded_bases_pair() {
    let left = |seq: &str, enzyme: &str| {
        let d = digest(seq, false, &[enzyme]).unwrap();
        d.fragments.iter().find(|f| f.left.enzyme.is_some()).unwrap().left.clone()
    };
    let right = |seq: &str, enzyme: &str| {
        let d = digest(seq, false, &[enzyme]).unwrap();
        d.fragments.iter().find(|f| f.right.enzyme.is_some()).unwrap().right.clone()
    };
    let eco = ("AAAAAGAATTCAAAAA", "EcoRI");
    let bam = ("AAAAAGGATCCAAAAA", "BamHI");
    let bgl = ("AAAAAAGATCTAAAAA", "BglII");
    let sma = ("AAAAACCCGGGAAAAA", "SmaI");
    let ecorv = ("AAAAAGATATCAAAAA", "EcoRV");

    assert!(joins(&right(eco.0, eco.1), &left(eco.0, eco.1)), "a cut religates");
    assert!(joins(&right(bam.0, bam.1), &left(bgl.0, bgl.1)), "BamHI and BglII both leave 5' GATC");
    assert!(!joins(&right(eco.0, eco.1), &left(bam.0, bam.1)), "AATT does not pair with GATC");
    assert!(joins(&right(sma.0, sma.1), &left(ecorv.0, ecorv.1)), "any blunt end joins any other");
    assert!(!joins(&right(sma.0, sma.1), &left(eco.0, eco.1)), "blunt does not join an overhang");
}

#[test]
fn turning_a_piece_round_reads_the_other_strand_and_swaps_its_ends() {
    let d = digest("AAAAAGAATTCAAAAAGGATCCAAAAA", false, &["EcoRI", "BamHI"]).unwrap();
    let middle = d.fragments.iter().find(|f| f.left.enzyme == Some("EcoRI") && f.right.enzyme == Some("BamHI")).unwrap();
    let piece = Piece::from(middle);
    let back = piece.flipped();
    assert_eq!(back.sequence, bios_core::seq::revcomp(&piece.sequence));
    assert_eq!(back.left.enzyme, Some("BamHI"));
    assert_eq!(back.right.enzyme, Some("EcoRI"));
    assert_eq!(back.flipped(), piece);
}

/// pUC19 opened EcoRI/HindIII takes the 51 bp polylinker out and keeps 2635 bp; an insert cut out
/// of GAATTC + 15 bases + AAGCTT is 21 bp, so the product is 2656 bp and goes in one way only.
#[test]
fn an_insert_with_two_different_ends_goes_in_one_way_round() {
    let c = puc19();
    let opened = digest(&c.sequence, true, &["EcoRI", "HindIII"]).unwrap();
    assert_eq!(opened.fragments.iter().map(shape).collect::<Vec<_>>(), vec![(2635, 448, 396, true), (51, 397, 447, false)]);
    let backbone = Piece::from(opened.fragments.first().unwrap());
    assert!(!can_self_close(&backbone), "5' AATT cannot close on 5' AGCT");

    let ordered = "GAATTCATGGGTAAACCGTAAAAGCTT";
    let cut = digest(ordered, false, &["EcoRI", "HindIII"]).unwrap();
    let insert = Piece::from(cut.fragments.iter().find(|f| f.left.enzyme.is_some() && f.right.enzyme.is_some()).unwrap());
    assert_eq!(insert.sequence, "AATTCATGGGTAAACCGTAAA");
    assert_eq!(insert.sequence.len(), 21);

    let products = ligate(&backbone, &insert);
    assert_eq!(products.len(), 1, "AATT one side and AGCT the other: one orientation");
    let p = products.first().unwrap();
    assert_eq!(p.sequence.len(), 2656);
    assert!(p.circular);
    assert_eq!((p.insert_start + 1, p.insert_end + 1), (2636, 2656));
    assert_eq!(
        p.junctions.iter().map(|j| (j.at, j.upstream_enzyme, j.downstream_enzyme)).collect::<Vec<_>>(),
        vec![(2635, Some("EcoRI"), Some("EcoRI")), (0, Some("HindIII"), Some("HindIII")),]
    );
    // Both sites are rebuilt, so the product can be cut back open with the same two enzymes.
    let back = digest(&p.sequence, true, &["EcoRI", "HindIII"]).unwrap();
    assert_eq!(back.fragments.iter().map(|f| f.length).collect::<Vec<_>>(), vec![2635, 21]);
}

#[test]
fn a_blunt_insert_goes_in_either_way_round_and_the_vector_can_close_empty() {
    let c = puc19();
    let opened = digest(&c.sequence, true, &["SmaI"]).unwrap();
    let backbone = Piece::from(opened.fragments.first().unwrap());
    assert!(can_self_close(&backbone), "two blunt ends ligate to each other");

    let insert = Piece {
        sequence: "ATGGGTAAACCGTAAGGGG".into(),
        left: bios_core::digest::FragmentEnd::blunt(),
        right: bios_core::digest::FragmentEnd::blunt(),
    };
    let products = ligate(&backbone, &insert);
    assert_eq!(products.len(), 2, "blunt ligation cannot choose a direction");
    assert_eq!(products.iter().map(|p| p.sequence.len()).collect::<Vec<_>>(), vec![2705, 2705]);
    let (forward, reverse) = (products.first().unwrap(), products.get(1).unwrap());
    assert!(forward.sequence.ends_with("ATGGGTAAACCGTAAGGGG"));
    assert!(reverse.sequence.ends_with(&bios_core::seq::revcomp("ATGGGTAAACCGTAAGGGG")));
}

#[test]
fn ends_that_cannot_be_ligated_give_no_product() {
    let c = puc19();
    let opened = digest(&c.sequence, true, &["EcoRI", "HindIII"]).unwrap();
    let backbone = Piece::from(opened.fragments.first().unwrap());
    let blunt =
        Piece { sequence: "ATGGGTAAACCGTAA".into(), left: bios_core::digest::FragmentEnd::blunt(), right: bios_core::digest::FragmentEnd::blunt() };
    assert!(ligate(&backbone, &blunt).is_empty(), "a blunt insert does not go into an overhang-cut vector");
}
