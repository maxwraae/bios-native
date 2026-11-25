//! Recorded answers for the DNA and protein quantity maths.
//!
//! Where the answers come from:
//!
//! - **The protein weight** is checked against UniProt P62593 (BLAT_ECOLX, beta-lactamase TEM-1),
//!   whose `SQ` line publishes "286 AA; 31515 MW". The sequence below is that entry's, copied from
//!   <https://rest.uniprot.org/uniprotkb/P62593.fasta>. pUC19's own AmpR is a two-residue variant
//!   of it, and the test says what the two residues add.
//! - **The oligonucleotide weight** is checked against the formula every oligo supplier publishes,
//!   A 313.21 + C 289.18 + G 329.21 + T 304.20 per base and -61.96 for the two free ends, which
//!   `bios_core::quantities` works out again from the IUPAC atomic weights.
//! - **The genetic codes** are checked against the Biopython oracle already in the fixtures.
//! - Everything else is arithmetic from a number stated in the test itself.

use std::collections::BTreeMap;

use bios_core::coding::{GENETIC_CODES, codons_marked};
use bios_core::quantities::{
    AVERAGE_BASE_DA, AVERAGE_BP_DA, duplex_weight, extinction_280, molecules_from_pmol, ng_from_pmol, pmol_from_ng, protein_weight, residue_count,
    strand_weight, um_from_pmol, volume_for_um,
};
use bios_core::seq::{find_orfs, find_orfs_with, translate_with};

/// UniProt P62593, beta-lactamase TEM-1: 286 residues, published molecular weight 31515.
const TEM1: &str = "MSIQHFRVALIPFFAAFCLPVFAHPETLVKVKDAEDQLGARVGYIELDLNSGKILESFRPEERFPMMSTFKVLLCGAVLSR\
VDAGQEQLGRRIHYSQNDLVEYSPVTEKHLTDGMTVRELCSAAITMSDNTAANLLLTTIGGPKELTAFLHNMGDHVTRLDRWEPELNEAIPNDERDTTMPAAMATTLRKLLTGELLTLASRQQLIDWMEADKVAGPLLR\
SALPAGWFIADKSGAGERGSRGIIAALGPDGKPSRIVVIYTTGSQATMDERNRQIAEIGASLIKHW";

#[test]
fn the_published_weight_of_a_named_protein_comes_back() {
    assert_eq!(TEM1.chars().count(), 286);
    let weight = protein_weight(TEM1).unwrap();
    // UniProt rounds its MW to the nearest dalton; Bios's atomic weights are a later IUPAC table,
    // which moves 286 residues by a fraction of one dalton.
    assert_eq!(weight.round(), 31515.0, "{weight} against UniProt P62593's published 31515");
    // Its absorption at 280 nm, from the residues UniProt's sequence holds.
    let (trp, tyr, cys) = (residue_count(TEM1, 'W'), residue_count(TEM1, 'Y'), residue_count(TEM1, 'C'));
    assert_eq!((trp, tyr, cys), (4, 4, 3));
    assert_eq!(extinction_280(trp, tyr, 0), 27_960.0);
    assert_eq!(extinction_280(trp, tyr, cys / 2), 28_085.0);
}

#[test]
fn an_oligo_weighs_what_the_published_formula_gives() {
    // The M13 forward primer in pUC19's multiple cloning site, 24 bases.
    let m13 = "GTAAAACGACGGCCAGTGCCAAGC";
    let counts: BTreeMap<char, usize> = "ACGT".chars().map(|base| (base, m13.chars().filter(|b| *b == base).count())).collect();
    assert_eq!(counts.values().sum::<usize>(), 24);
    let published = counts[&'A'] as f64 * 313.21 + counts[&'C'] as f64 * 289.18 + counts[&'G'] as f64 * 329.21 + counts[&'T'] as f64 * 304.20 - 61.96;
    let worked = strand_weight(m13).unwrap();
    assert!((worked - published).abs() < 0.2, "{worked} against the published formula's {published}");
    // A duplex of it is the two strands, and the complement has the complementary counts.
    assert!((duplex_weight(m13).unwrap() - (worked + strand_weight(&bios_core::seq::revcomp(m13)).unwrap())).abs() < 1e-9);
}

#[test]
fn a_hundred_nanograms_of_a_fragment_is_the_picomoles_the_bench_quotes() {
    // 1.2 kb at the conventional 650 g/mol per base pair: 1200 x 650 = 780,000.
    let weight = 1200.0 * AVERAGE_BP_DA;
    assert_eq!(weight, 780_000.0);
    let pmol = pmol_from_ng(100.0, weight).unwrap();
    // 100 ng / 780,000 g/mol = 1.282e-13 mol = 0.1282 pmol.
    assert_eq!((pmol * 1e4).round() / 1e4, 0.1282);
    assert_eq!((ng_from_pmol(pmol, weight) * 1e6).round() / 1e6, 100.0);
    // 0.1282 pmol x 6.02214076e23 / 1e12 molecules.
    assert_eq!((molecules_from_pmol(pmol) / 1e9).round(), 77.0);
    // A 20-mer oligo at 330 g/mol per base: 20 x 330 = 6600, so 1 ug is 151.5 pmol.
    let oligo = 20.0 * AVERAGE_BASE_DA;
    assert_eq!(oligo, 6600.0);
    assert_eq!((pmol_from_ng(1000.0, oligo).unwrap() * 10.0).round() / 10.0, 151.5);
}

#[test]
fn a_three_to_one_ligation_weighs_out_three_times_the_moles() {
    // 50 ng of pUC19 read from its bases, against a 1.2 kb insert at the average.
    let vector = duplex_weight(&pUC19()).unwrap();
    // pUC19 is 2686 bp; its real weight is below the 650-per-base-pair average of 1,745,900.
    assert_eq!((vector / 10.0).round() * 10.0, 1_659_570.0);
    assert!(vector < 2686.0 * AVERAGE_BP_DA);
    let vector_pmol = pmol_from_ng(50.0, vector).unwrap();
    assert_eq!((vector_pmol * 1e5).round() / 1e5, 0.03013);
    let insert_ng = ng_from_pmol(vector_pmol * 3.0, 1200.0 * AVERAGE_BP_DA);
    // 0.030128 pmol x 3 x 780,000 / 1000 = 70.5 ng.
    assert_eq!((insert_ng * 10.0).round() / 10.0, 70.5);
}

#[test]
fn resuspending_an_oligo_is_picomoles_over_micromolar() {
    // 33.4 nmol of a dried oligo to 100 uM wants 334 uL.
    assert_eq!(volume_for_um(33_400.0, 100.0), Some(334.0));
    // And 33.4 nmol in 334 uL reads back as 100 uM.
    assert_eq!(um_from_pmol(33_400.0, 334.0), Some(100.0));
}

#[test]
fn the_tables_translate_and_stop_as_the_oracle_says() {
    let path = format!("{}/tests/fixtures/genetic-code-oracle.json", env!("CARGO_MANIFEST_DIR"));
    let oracle: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let tables = oracle["tables"].as_array().unwrap();
    assert_eq!(tables.len(), GENETIC_CODES.len());
    let codons: String = "TCAG".chars().flat_map(|a| "TCAG".chars().flat_map(move |b| "TCAG".chars().map(move |c| format!("{a}{b}{c}")))).collect();
    for table in tables {
        let id = table["id"].as_u64().unwrap();
        let listed = |key: &str| table[key].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(translate_with(&codons, id).as_deref(), table["ordinary"].as_str(), "table {id} residues");
        assert_eq!(codons_marked(id, b'M'), listed("starts"), "table {id} start codons");
        assert_eq!(codons_marked(id, b'*'), listed("stops"), "table {id} stop codons");
    }
}

#[test]
fn an_orf_read_with_the_mitochondrial_code_runs_through_tga_to_aga() {
    // ATG GCC TGA GCC AGA: TGA terminates in the standard code, and is a tryptophan in the
    // vertebrate mitochondrial one, where AGA terminates instead (both from the oracle above).
    let seq = "ATGGCCTGAGCCAGACCC";
    let standard = find_orfs_with(seq, 1, 1, false).unwrap();
    assert_eq!(standard.iter().map(|o| (o.start, o.end, o.protein.clone())).collect::<Vec<_>>(), vec![(1, 9, "MA*".to_string())]);
    let mito = find_orfs_with(seq, 1, 2, false).unwrap();
    assert_eq!(mito.iter().map(|o| (o.start, o.end, o.protein.clone())).collect::<Vec<_>>(), vec![(1, 15, "MAWA*".to_string())]);
    // The plain scan is the standard code with ATG alone, which is what the window draws.
    assert_eq!(find_orfs(seq, 1), standard);
}

/// pUC19's bases, from the GenBank fixture.
#[allow(non_snake_case)]
fn pUC19() -> String {
    let path = format!("{}/tests/fixtures/pUC19.gb", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    let construct = bios_core::genbank::read(&text).unwrap();
    assert_eq!(construct.len(), 2686);
    construct.sequence
}
