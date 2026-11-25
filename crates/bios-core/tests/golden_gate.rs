//! Golden Gate / type IIS assembly.
//!
//! The reference answers here are of two kinds. The enzyme geometry is read straight off the
//! repository's own enzyme table and asserted against it, so a change to the table shows up as a
//! failure here rather than as a wrong overhang on somebody's bench. Everything else is checked
//! against a destination vector built base by base in this file: the two BsaI sites are placed at
//! positions chosen by hand, so each cut, each overhang and the whole product sequence are known
//! before the designer is asked. pUC19 (GenBank L09137) stands in where a real sequence has
//! something to say — it carries exactly two BsmBI sites and exactly one BsaI site.

mod support;

use bios_core::enzymes::{find_cut_sites, get_enzyme};
use bios_core::golden_gate::{GoldenGateDesign, GoldenGateError, GoldenGateInput, Part, design_golden_gate, geometry, sites_within};
use bios_core::model::Strand;
use bios_core::seq::revcomp;
use support::feature;

// ---------------------------------------------------------------------------
// The synthetic destination vector and its parts.
// ---------------------------------------------------------------------------

/// 60 bases of backbone before the stuffer, free of GGTCTC and GAGACC.
const HEAD: &str = "GCTAAAGACAATTACATAACATACACGTCAGCACGAAACTTGTTGGCCCAGTGTGAATCG";
/// 20 bases of stuffer between the two sites.
const STUFFER_BODY: &str = "CTTAAGGGTTAAGTAAGTGT";
/// 50 bases of backbone after the stuffer.
const TAIL: &str = "GATGCATACGCCTTTACTTGCTGTGTCCACCCCATCGGACTGGCATTTTT";

/// Three parts, each written from its first fusion base to its last: AATG→GGAG, GGAG→TTCG,
/// TTCG→CGCT. Thirty, twenty-four and thirty-six bases.
const PART_A: &str = "AATGATTACACTCAGAAACAGAACTCGGAG";
const PART_B: &str = "GGAGGGGTAATTTTGACAGGTTCG";
const PART_C: &str = "TTCGTCACGCAGAGGCGCGCCCTCCTGAAGTGCGCT";

/// A 152 bp destination vector, laid out so that both BsaI sites point outward from the stuffer
/// and so leave with it.
///
/// BsaI is GGTCTC(1/5). A forward site at `r` cuts the top strand at gap `r + 7`; read on the other
/// strand (plus-strand GAGACC) at `r` it cuts at gap `r - 5`. So:
///
/// ```text
/// 0            60   64 65          85     91     97 98        101        151
/// | HEAD (60)  |AATG| T|GAGACC| BODY (20) |GGTCTC| A |CGCT| TAIL (50)      |
///              ^ cut 60, overhang AATG           cut 98, overhang CGCT ^
/// ```
///
/// The cut at gap 60 comes from the reverse site at 65 (65 - 5); the cut at gap 98 from the forward
/// site at 91 (91 + 7). Both sites lie in the arc 60..97, so the arc 98..59 — the backbone — holds
/// none, which is what makes it the backbone.
fn dest() -> String {
    format!("{HEAD}AATGTGAGACC{STUFFER_BODY}GGTCTCACGCT{TAIL}")
}

fn parts() -> Vec<Part> {
    vec![Part::new("Part A", PART_A), Part::new("Part B", PART_B), Part::new("Part C", PART_C)]
}

fn input(parts: Vec<Part>) -> GoldenGateInput {
    GoldenGateInput::new("pGG-dest", dest(), "BsaI", parts)
}

fn designed(input: &GoldenGateInput) -> GoldenGateDesign {
    design_golden_gate(input).unwrap_or_else(|e| panic!("{e}"))
}

fn puc19() -> String {
    let path = format!("{}/tests/fixtures/pUC19.gb", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    bios_core::genbank::read(&text).expect("pUC19").sequence
}

// ---------------------------------------------------------------------------
// The enzyme's geometry, against the table it comes from.
// ---------------------------------------------------------------------------

/// GGTCTC(1/5) is not written down anywhere in Bios: it is the enzyme table's `GGTCTC` with
/// `top_snip_offset: 7, bottom_snip_offset: 11` minus the site's own length. This pins both halves
/// of that, so the table and the subtraction cannot drift apart.
#[test]
fn bsai_geometry_is_the_enzyme_table_minus_the_length_of_the_site() {
    let table = get_enzyme("BsaI").expect("BsaI is in the table");
    assert_eq!((table.site, table.length), ("GGTCTC", 6));
    assert!(table.type2s);
    assert_eq!(table.top_snip_offset(), Some(7));
    assert_eq!(table.bottom_snip_offset(), Some(11));
    assert_eq!(table.additional_cuts(), &[]);

    let g = geometry("BsaI").expect("geometry");
    assert_eq!(g.enzyme, "BsaI");
    assert_eq!(g.site, "GGTCTC");
    assert_eq!(g.spacer, 7 - 6, "the top strand is cut one base past the site");
    assert_eq!(g.overhang_len, 11 - 7, "and the bottom strand four bases further, leaving four");
    assert_eq!(g.notation(), "GGTCTC(1/5)");
    assert_eq!(g.five_prime_tail, "GGTCTCA");
    assert_eq!(g.three_prime_tail, "AGAGACC");
    assert_eq!(g.three_prime_tail, format!("A{}", revcomp(g.site)));
}

/// The same subtraction over the rest of the type IIS enzymes an agent might ask for. SapI leaves
/// THREE bases, not four, and BtgZI cuts ten bases away: both come out of the table unchanged.
#[test]
fn every_golden_gate_enzyme_comes_out_of_the_same_subtraction() {
    let expected = [
        ("BsaI", "BsaI", "GGTCTC(1/5)", 4, "GGTCTCA", "AGAGACC"),
        ("BsmBI", "BsmBI", "CGTCTC(1/5)", 4, "CGTCTCA", "AGAGACG"),
        ("Esp3I", "BsmBI", "CGTCTC(1/5)", 4, "CGTCTCA", "AGAGACG"),
        ("BbsI", "BbsI", "GAAGAC(2/6)", 4, "GAAGACAA", "AAGTCTTC"),
        ("BpiI", "BbsI", "GAAGAC(2/6)", 4, "GAAGACAA", "AAGTCTTC"),
        ("SapI", "SapI", "GCTCTTC(1/4)", 3, "GCTCTTCA", "AGAAGAGC"),
        ("BspQI", "SapI", "GCTCTTC(1/4)", 3, "GCTCTTCA", "AGAAGAGC"),
        ("AarI", "AarI", "CACCTGC(4/8)", 4, "CACCTGCAAAA", "AAAAGCAGGTG"),
        ("BtgZI", "BtgZI", "GCGATG(10/14)", 4, "GCGATGAAAAAAAAAA", "AAAAAAAAAACATCGC"),
    ];
    for (asked, canonical, notation, overhang_len, five, three) in expected {
        let g = geometry(asked).unwrap_or_else(|e| panic!("{asked}: {e}"));
        assert_eq!(g.enzyme, canonical, "{asked}");
        assert_eq!(g.notation(), notation, "{asked}");
        assert_eq!(g.overhang_len, overhang_len, "{asked}");
        assert_eq!(g.five_prime_tail, five, "{asked}");
        assert_eq!(g.three_prime_tail, three, "{asked}");
        // A tail is the site and the spacer; the 3' tail is the spacer and the site read on the
        // other strand. Checked where it matters: digest the piece the tails make and see that the
        // two cuts leave exactly the part's own first and last bases.
        let table = get_enzyme(canonical).expect("in the table");
        assert_eq!(g.five_prime_tail.len(), table.length + usize::try_from(g.spacer).expect("spacer"));
        assert_eq!(g.three_prime_tail, format!("{}{}", "A".repeat(usize::try_from(g.spacer).expect("spacer")), revcomp(g.site)));
        let k = usize::try_from(g.overhang_len).expect("overhang");
        let part = format!("{}CTTAAGGGTTAAGT{}", "ACGT".get(..k).expect("left"), "TGCA".get(..k).expect("right"));
        let piece = format!("{}{part}{}", g.five_prime_tail, g.three_prime_tail);
        let cuts = find_cut_sites(&piece, &[canonical], false);
        let overhangs: Vec<&str> = cuts.iter().map(|c| c.overhang.seq.as_str()).collect();
        assert_eq!(overhangs, [part.get(..k).expect("left"), part.get(part.len() - k..).expect("right")], "{asked}");
    }
}

/// An enzyme the method cannot use says so by name rather than designing something that would not
/// work in the tube.
#[test]
fn an_enzyme_that_cannot_hold_an_assembly_together_is_refused() {
    assert_eq!(geometry("NotAnEnzyme"), Err(GoldenGateError::UnknownEnzyme("NotAnEnzyme".into())));
    // EcoRI cuts inside G^AATTC, so its site stays on both pieces.
    assert_eq!(geometry("EcoRI"), Err(GoldenGateError::NotTypeIIS("EcoRI")));
    // HaeIV is type IIS in the table but its cut positions are not verified there.
    assert_eq!(geometry("HaeIV"), Err(GoldenGateError::CleavageUnverified("HaeIV")));
    // BsaXI cuts on both sides of its site: two cleavage pairs, not one released end.
    assert_eq!(geometry("BsaXI"), Err(GoldenGateError::TwoSidedCutter("BsaXI")));
    // BceSI's site is MMCGAAG, so there is no one sequence to put on a tail.
    assert_eq!(geometry("BceSI"), Err(GoldenGateError::DegenerateSite("BceSI")));
    // BpmI is CTGGAG(16/14): the bottom strand is cut first, so the overhang is 3'.
    assert_eq!(geometry("BpmI"), Err(GoldenGateError::NotAFivePrimeOverhang("BpmI")));
    // MlyI is GAGTC(5/5): blunt, so nothing anneals.
    assert_eq!(geometry("MlyI"), Err(GoldenGateError::NotAFivePrimeOverhang("MlyI")));
}

// ---------------------------------------------------------------------------
// The assembly itself.
// ---------------------------------------------------------------------------

/// The whole answer for the vector drawn in [`dest`]: which arc is the backbone, what each overhang
/// is, the order the overhangs force, and the product base for base.
#[test]
fn three_parts_go_into_the_synthetic_vector_in_the_order_their_overhangs_force() {
    let d = designed(&input(parts()));

    // The backbone is the arc the sites left: 98..59, so 61..98 is what goes.
    assert_eq!(d.backbone.name, "pGG-dest backbone");
    assert_eq!(d.backbone.removed, (61, 98));
    assert_eq!(d.backbone.removed_len, 38);
    assert_eq!(d.backbone.length, 152 - 38 + 4);
    assert!(!d.backbone.through_origin);
    assert_eq!(d.backbone.right_overhang, "AATG", "the cut at gap 60, where the first part joins");
    assert_eq!(d.backbone.left_overhang, "CGCT", "the cut at gap 98, where the last part joins");

    // The parts were given A, B, C and their overhangs happen to agree with that; the order is the
    // overhangs' doing either way.
    assert_eq!(d.order, ["pGG-dest backbone", "Part A", "Part B", "Part C"]);

    // Each piece gives the product its own length less the four bases it shares with the next, so
    // the product is the retained backbone plus 26 + 20 + 32.
    let product = d.product.as_ref().expect("a circular product");
    let expected = format!("{HEAD}{}{}{}CGCT{TAIL}", &PART_A[..26], &PART_B[..20], &PART_C[..32]);
    assert_eq!(product.sequence, expected);
    assert_eq!(product.sequence.len(), 192);
    assert!(product.circular);

    // Four junctions round the circle, each named by the overhang that makes it.
    let junctions: Vec<String> = d.junctions.iter().map(|j| format!("{} {} {} {:?}", j.left, j.right, j.overhang, j.position)).collect();
    assert_eq!(
        junctions,
        [
            "pGG-dest backbone Part A AATG Some((61, 64))",
            "Part A Part B GGAG Some((87, 90))",
            "Part B Part C TTCG Some((107, 110))",
            "Part C pGG-dest backbone CGCT Some((139, 142))",
        ]
    );
    assert!(d.junctions.iter().all(|j| j.unique && !j.palindromic));
    // Every overhang is really at the position the design claims.
    for j in &d.junctions {
        let (start, _) = j.position.expect("a position");
        let at = usize::try_from(start - 1).expect("0-based");
        assert_eq!(product.sequence.get(at..at + 4), Some(j.overhang.as_str()), "{}", j.overhang);
    }

    // Each part is on the product where the design says, overhangs included, so neighbours share
    // four bases.
    let placed: Vec<(&str, usize, usize)> = product.placements.iter().map(|p| (p.name.as_str(), p.start, p.end)).collect();
    assert_eq!(placed, [("Part A", 60, 89), ("Part B", 86, 109), ("Part C", 106, 141)]);

    assert_eq!(d.warnings, Vec::<String>::new(), "this set is clean");
}

/// What goes on the order form: the part as given, with the site and its spacer at the 5' end and
/// the same read backwards at the 3' end, so the enzyme releases the part and keeps the tails.
#[test]
fn each_part_is_quoted_with_the_tails_it_needs() {
    let d = designed(&input(parts()));
    let a = d.pieces.first().expect("Part A");
    assert_eq!(a.name, "Part A");
    assert_eq!(a.five_prime_tail, "GGTCTCA");
    assert_eq!(a.three_prime_tail, "AGAGACC");
    assert_eq!(a.part, PART_A);
    assert_eq!(a.order, format!("GGTCTCA{PART_A}AGAGACC"));
    assert_eq!(a.left_overhang, "AATG");
    assert_eq!(a.right_overhang, "GGAG");
    assert_eq!(a.order.len(), PART_A.len() + 14);

    // And the tails really do leave the part's own first and last four bases as the overhangs: the
    // forward site cuts one base in, four bases wide.
    let g = geometry("BsaI").expect("geometry");
    let cut = g.five_prime_tail.len();
    assert_eq!(a.order.get(cut..cut + 4), Some(a.left_overhang.as_str()));
    let back = a.order.len() - g.three_prime_tail.len() - 4;
    assert_eq!(a.order.get(back..back + 4), Some(a.right_overhang.as_str()));

    assert_eq!(
        d.pieces.iter().map(|p| p.order.clone()).collect::<Vec<_>>(),
        [format!("GGTCTCA{PART_A}AGAGACC"), format!("GGTCTCA{PART_B}AGAGACC"), format!("GGTCTCA{PART_C}AGAGACC"),]
    );
}

/// The vector's own annotations come onto the product at their new positions, the ones that were
/// wholly inside the stuffer go, and one clipped by the opening is kept and marked. A part's own
/// annotations come along too.
#[test]
fn features_cross_the_opening_and_the_parts_bring_their_own() {
    let mut input = input(parts());
    input.vector_features = vec![
        feature("ori", "rep_origin", 0, 39, Strand::Forward),    // wholly before the stuffer
        feature("lacZ stuffer", "CDS", 70, 90, Strand::Forward), // wholly inside it
        feature("spans the opening", "misc_feature", 50, 110, Strand::Forward), // clipped
        feature("AmpR", "CDS", 110, 151, Strand::Reverse),       // wholly after it
    ];
    if let Some(part) = input.parts.get_mut(1) {
        part.features = vec![feature("B's own", "misc_feature", 4, 19, Strand::Forward)];
    }
    let d = designed(&input);
    let product = d.product.expect("a product");

    let kept: Vec<(&str, usize, usize)> = d
        .backbone
        .removed
        .0
        .eq(&61)
        .then(|| product.features.iter().map(|f| (f.name.as_str(), f.start, f.end)).collect::<Vec<_>>())
        .expect("the opening is where the other test says");

    // `ori` is before the opening, so it does not move. `AmpR` was at 110..151 and the opening
    // replaced 38 bases with 78, so it moves 40 on.
    assert!(kept.contains(&("ori", 0, 39)), "{kept:?}");
    assert!(kept.contains(&("AmpR", 150, 191)), "{kept:?}");
    // The clipped one survives with its left half and whatever of its right half the product kept.
    assert_eq!(d.backbone.removed_len, 38);
    let carried: Vec<&str> = product.carried.kept.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(carried, ["ori", "spans the opening", "AmpR"]);
    assert_eq!(product.carried.removed.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["lacZ stuffer"]);
    assert_eq!(product.carried.truncated.iter().map(|t| t.before.name.as_str()).collect::<Vec<_>>(), ["spans the opening"]);

    // One marker per part, and Part B's own feature four bases into Part B, which starts at 86.
    assert!(kept.contains(&("Part A", 60, 89)), "{kept:?}");
    assert!(kept.contains(&("Part B", 86, 109)), "{kept:?}");
    assert!(kept.contains(&("Part C", 106, 141)), "{kept:?}");
    assert!(kept.contains(&("B's own", 90, 105)), "{kept:?}");
}

/// The stuffer may straddle the vector's origin: the same vector rotated so the opening wraps gives
/// the same product, rotated by the same amount.
#[test]
fn the_stuffer_may_run_through_the_origin() {
    let straight = designed(&input(parts()));
    let flat = straight.product.expect("a product").sequence;

    let v = dest();
    let rotated = format!("{}{}", v.get(80..).expect("tail"), v.get(..80).expect("head"));
    let d = designed(&GoldenGateInput::new("pGG-dest", rotated, "BsaI", parts()));
    assert_eq!(d.backbone.removed, (133, 18));
    assert!(d.backbone.through_origin);
    assert_eq!(d.backbone.removed_len, 38);
    assert_eq!(d.order, ["pGG-dest backbone", "Part A", "Part B", "Part C"]);

    // A replacement that wraps starts the product, so the inserted stretch is at the front and the
    // first junction is at base 1.
    assert_eq!(d.junctions.first().and_then(|j| j.position), Some((1, 4)));
    let product = d.product.expect("a product").sequence;
    assert_eq!(product.len(), 192);
    assert_eq!(product, format!("{}{}", flat.get(60..).expect("from the opening"), flat.get(..60).expect("to it")));
    assert_eq!(d.warnings, Vec::<String>::new());
}

/// SapI leaves three bases, not four, and nothing in the design says otherwise.
#[test]
fn sapi_assembles_on_three_base_overhangs() {
    // Built exactly as `dest` is, with SapI's own geometry: GCTCTTC(1/4).
    const HEAD: &str = "CTTGTCTCCAAGTACCCATTTAGTAGACAAATCGTTCCAT";
    const BODY: &str = "CACCAATTCGCTGGTT";
    const TAIL: &str = "GTTGAACTATACGACCGGGGCACACTGCAC";
    const PART: &str = "ATGTCAGTTCCCATTTAGAGGGCT";
    let vector = format!("{HEAD}ATGTGAAGAGC{BODY}GCTCTTCAGCT{TAIL}");
    assert_eq!(vector.len(), 108);

    let d = designed(&GoldenGateInput::new("pSapI-dest", vector, "SapI", vec![Part::new("CDS", PART)]));
    assert_eq!(d.geometry.overhang_len, 3);
    assert_eq!(d.backbone.right_overhang, "ATG");
    assert_eq!(d.backbone.left_overhang, "GCT");
    assert_eq!(d.backbone.removed, (41, 75));
    assert_eq!(d.order, ["pSapI-dest backbone", "CDS"]);
    assert_eq!(
        d.junctions.iter().map(|j| (j.overhang.clone(), j.position)).collect::<Vec<_>>(),
        [("ATG".to_string(), Some((41, 43))), ("GCT".to_string(), Some((62, 64))),]
    );
    let product = d.product.expect("a product");
    assert_eq!(product.sequence, format!("{HEAD}{}GCT{TAIL}", PART.get(..21).expect("contribution")));
    assert_eq!(product.sequence.len(), 94);
    assert_eq!(d.pieces.first().map(|p| p.order.clone()), Some(format!("GCTCTTCA{PART}AGAAGAGC")));
    assert_eq!(d.warnings, Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// The warnings.
// ---------------------------------------------------------------------------

/// A BsaI site inside a part would cut the part apart in the same tube, so it is named with where
/// it is — and so is the product it would leave.
#[test]
fn a_site_inside_a_part_is_named_with_its_position() {
    let mut parts = parts();
    if let Some(b) = parts.get_mut(1) {
        // GGTCTC put at 0-based 6 of Part B, in place of six of its bases, so the part's overhangs
        // and the assembly are untouched.
        b.seq = format!("{}GGTCTC{}", PART_B.get(..6).expect("head"), PART_B.get(12..).expect("tail"));
    }
    let d = designed(&input(parts));
    assert!(d.product.is_some(), "it still assembles; it just gets cut again");
    let said = d.warnings.join("\n");
    assert!(said.contains("Part \"Part B\" carries a BsaI site of its own at 7..12"), "{said}");
    assert!(said.contains("The assembled product still carries 1 BsaI site(s), at 93"), "{said}");
}

/// Two junctions on the same overhang let the pieces swap places, so the order the design reports
/// is only one of several.
#[test]
fn two_junctions_sharing_an_overhang_are_called_ambiguous() {
    // Give Part B the same 3' overhang as Part A, and Part C the matching 5' overhang.
    let b = format!("{}GGAG", PART_B.get(..20).expect("body"));
    let c = format!("GGAG{}", PART_C.get(4..).expect("body"));
    let d = designed(&input(vec![Part::new("Part A", PART_A), Part::new("Part B", b), Part::new("Part C", c)]));
    let said = d.warnings.join("\n");
    assert!(said.contains("Overhang GGAG after \"Part A\" fits \"Part B\" and \"Part C\""), "{said}");
    assert!(said.contains("Overhang GGAG is used by 2 junctions"), "{said}");
    assert!(d.junctions.iter().filter(|j| j.overhang == "GGAG").all(|j| !j.unique));
}

/// An overhang that reads the same on both strands anneals to itself, so the piece after it can go
/// in either way round.
#[test]
fn a_palindromic_overhang_is_reported_against_the_piece_it_lets_flip() {
    // GTAC is its own reverse complement. Put it at the Part A / Part B junction.
    let a = format!("{}GTAC", PART_A.get(..26).expect("body"));
    let b = format!("GTAC{}", PART_B.get(4..).expect("body"));
    let d = designed(&input(vec![Part::new("Part A", a), Part::new("Part B", b), Part::new("Part C", PART_C)]));
    assert_eq!(revcomp("GTAC"), "GTAC");
    let said = d.warnings.join("\n");
    assert!(said.contains("The GTAC junction between \"Part A\" and \"Part B\" at 87..90 reads the same on both strands"), "{said}");
    assert!(said.contains("so those two ends can also anneal the other way round and a part can go in backwards"), "{said}");
    assert!(d.junctions.iter().any(|j| j.overhang == "GTAC" && j.palindromic));
}

/// Overhangs that do not join up give no product, and the warning says which piece the chain
/// stopped at and which overhang had nowhere to go.
#[test]
fn pieces_that_do_not_close_into_a_circle_give_no_product() {
    // Part B's 3' overhang no longer matches Part C's 5' overhang.
    let b = format!("{}ACTA", PART_B.get(..20).expect("body"));
    let d = designed(&input(vec![Part::new("Part A", PART_A), Part::new("Part B", b), Part::new("Part C", PART_C)]));
    assert!(d.product.is_none());
    assert_eq!(d.order, ["pGG-dest backbone", "Part A", "Part B"]);
    assert!(d.junctions.iter().all(|j| j.position.is_none()), "there is no product to place them on");
    let said = d.warnings.join("\n");
    assert!(said.contains("the 3' overhang ACTA of \"Part B\" matches no part's 5' overhang"), "{said}");
    assert!(said.contains("it is not the backbone's CGCT either"), "{said}");
}

/// A part whose overhangs are the right ones the wrong way round is told so, because Bios never
/// turns a part round by itself.
#[test]
fn a_part_that_would_only_fit_backwards_is_told_so() {
    let flipped = revcomp(PART_B);
    let d = designed(&input(vec![Part::new("Part A", PART_A), Part::new("Part B", flipped), Part::new("Part C", PART_C)]));
    assert!(d.product.is_none());
    let said = d.warnings.join("\n");
    assert!(said.contains("\"Part B\" would match if it went in backwards"), "{said}");
}

/// A part nothing in the closed circle leads to is left over, and says so.
#[test]
fn a_part_the_circle_never_reaches_is_reported_as_left_over() {
    let mut parts = parts();
    parts.push(Part::new("Spare", "ACTAGGGTTAAGTAAGTGTGCAA"));
    let d = designed(&input(parts));
    assert!(d.product.is_some(), "the other three still close");
    let said = d.warnings.join("\n");
    assert!(said.contains("\"Spare\" (ACTA…GCAA) is left over"), "{said}");
}

// ---------------------------------------------------------------------------
// Refusals.
// ---------------------------------------------------------------------------

#[test]
fn a_part_too_short_to_hold_two_overhangs_and_a_body_is_refused() {
    let err = design_golden_gate(&input(vec![Part::new("Stub", "AATGCGCT")])).expect_err("too short");
    assert_eq!(err, GoldenGateError::PartTooShort { name: "Stub".into(), length: 8, least: 9 });
    assert!(err.to_string().contains("its first and last bases ARE its two overhangs"), "{err}");
}

#[test]
fn a_linear_destination_vector_is_refused() {
    let mut input = input(parts());
    input.vector_circular = false;
    assert_eq!(design_golden_gate(&input), Err(GoldenGateError::VectorLinear));
    let mut empty = input.clone();
    empty.vector_circular = true;
    empty.vector_seq = String::new();
    assert_eq!(design_golden_gate(&empty), Err(GoldenGateError::VectorEmpty));
    let mut none = input.clone();
    none.vector_circular = true;
    none.parts = Vec::new();
    assert_eq!(design_golden_gate(&none), Err(GoldenGateError::NoParts));
}

/// A vector with only one site cannot be opened, and the message says where the one site cut.
#[test]
fn a_vector_the_enzyme_cuts_once_is_refused_with_the_position() {
    // The second site taken out of the synthetic vector, its six bases replaced.
    let one = dest().replacen("GGTCTC", "GGATCC", 1);
    let err = design_golden_gate(&GoldenGateInput::new("pGG-dest", one, "BsaI", parts())).expect_err("one site");
    assert_eq!(err, GoldenGateError::VectorNotCutTwice { enzyme: "BsaI", cuts: vec![61] });
    assert!(err.to_string().contains("cuts this plasmid once, at 61"), "{err}");
}

/// Two sites pointing INWARD leave their own sites on the big piece, so the only piece the enzyme
/// cannot touch again is the little one BETWEEN the cuts — which is what the design then keeps, and
/// its overhangs are the other way round, so the parts no longer go in.
///
/// This is the honest answer rather than a refusal: that little fragment really is the only
/// site-free piece of that digest. What tells the person their vector is laid out backwards is the
/// overhangs and the dead end, both named.
#[test]
fn a_vector_whose_sites_point_inward_keeps_the_piece_between_them() {
    // The same layout with both sites turned round: GGTCTC before AATG, GAGACC after CGCT.
    let inward = format!("{HEAD}GGTCTCAAATG{STUFFER_BODY}CGCTTGAGACC{TAIL}");
    let d = designed(&GoldenGateInput::new("pIn", inward, "BsaI", parts()));
    // The forward site at 61 cuts at gap 67, the reverse site at 97 cuts at gap 91, and 68..91 is
    // the only arc without a site in it.
    assert_eq!(d.backbone.removed, (92, 67));
    assert!(d.backbone.through_origin);
    assert_eq!(d.backbone.length, 24 + 4);
    assert_eq!((d.backbone.right_overhang.as_str(), d.backbone.left_overhang.as_str()), ("CGCT", "AATG"), "the wrong way round");
    assert!(d.product.is_none());
    let said = d.warnings.join("\n");
    assert!(said.contains("the 3' overhang CGCT of \"pIn backbone\" matches no part's 5' overhang"), "{said}");
}

// ---------------------------------------------------------------------------
// A real sequence.
// ---------------------------------------------------------------------------

/// pUC19 carries exactly two BsmBI sites, and they happen to point outward — so BsmBI really does
/// open it, and the arc the sites are NOT in is the backbone even though it is the shorter piece.
/// Its 5' overhang, CGCG, is its own reverse complement, which the design says out loud.
#[test]
fn puc19_opens_with_bsmbi_at_the_one_arc_free_of_its_sites() {
    let seq = puc19();
    assert_eq!(seq.len(), 2686);
    // "TCCC" + 20 bases + "CGCG": the two overhangs pUC19's digest leaves.
    const PART: &str = "TCCCATCCTAGCCTAGCTACGCGTCGCG";

    let d = designed(&GoldenGateInput::new("pUC19", seq.clone(), "BsmBI", vec![Part::new("linker", PART)]));
    assert_eq!(d.geometry.notation(), "CGTCTC(1/5)");
    // The forward site runs through the origin at 2683 and cuts at gap 3; the reverse site at 51
    // cuts at gap 45. 46..3 holds both sites, so 4..49 is the backbone.
    assert_eq!(d.backbone.removed, (46, 3));
    assert!(d.backbone.through_origin);
    assert_eq!(d.backbone.removed_len, 2644);
    assert_eq!(d.backbone.length, 46);
    assert_eq!(d.backbone.right_overhang, seq.get(45..49).expect("46..49"));
    assert_eq!(d.backbone.right_overhang, "TCCC");
    assert_eq!(d.backbone.left_overhang, seq.get(3..7).expect("4..7"));
    assert_eq!(d.backbone.left_overhang, "CGCG");

    let product = d.product.expect("a product");
    assert_eq!(product.sequence, format!("{}{}", PART.get(..24).expect("contribution"), seq.get(3..45).expect("the retained arc")));
    assert_eq!(product.sequence.len(), 42 + 24, "the 42 bp arc, plus the linker less the overhang it shares");
    assert_eq!(
        d.junctions.iter().map(|j| (j.overhang.clone(), j.position)).collect::<Vec<_>>(),
        [("TCCC".to_string(), Some((1, 4))), ("CGCG".to_string(), Some((25, 28))),]
    );
    // CGCG reads the same both ways, so the linker can go in either way round.
    assert_eq!(revcomp("CGCG"), "CGCG");
    assert!(d.warnings.iter().any(|w| w.contains("The CGCG junction between \"linker\" and \"pUC19 backbone\" at 25..28")), "{:?}", d.warnings);
}

/// pUC19 has exactly one BsaI site and one SapI site, so neither can open it.
#[test]
fn puc19_cannot_be_a_bsai_or_sapi_destination() {
    let seq = puc19();
    let bsai = design_golden_gate(&GoldenGateInput::new("pUC19", seq.clone(), "BsaI", parts())).expect_err("one BsaI site");
    // The site reads GAGACC at 1766 on the plus strand, so BsaI cuts the top strand at 1761.
    assert_eq!(bsai, GoldenGateError::VectorNotCutTwice { enzyme: "BsaI", cuts: vec![1761] });
    let sapi = design_golden_gate(&GoldenGateInput::new("pUC19", seq, "SapI", parts())).expect_err("one SapI site");
    assert_eq!(sapi, GoldenGateError::VectorNotCutTwice { enzyme: "SapI", cuts: vec![691] });
}

/// The site search behind the warnings finds a site at the very end of a linear piece, where the
/// cut itself falls past the end — which is exactly the site that would ruin an assembly.
#[test]
fn a_site_at_the_end_of_a_linear_piece_is_still_found() {
    let bsai = get_enzyme("BsaI").expect("BsaI");
    assert_eq!(sites_within("AAAAGGTCTC", bsai, false), vec![4], "the cut falls four bases past the end");
    assert_eq!(sites_within("GAGACCAAAA", bsai, false), vec![0], "and five bases before the start");
    assert_eq!(sites_within("AAAAAAAAAA", bsai, false), Vec::<i64>::new());
}
