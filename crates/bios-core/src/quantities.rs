//! How much there is of something: what a piece of DNA or a protein weighs, and the nanograms,
//! picomoles and molecules that weight turns into.
//!
//! Every mass here is an average isotopic mass, which is what a balance and a bench calculation
//! deal in, not the monoisotopic mass a spectrometer peak gives.

/// The Avogadro constant, in molecules per mole. Exact since the 2019 redefinition of the mole
/// (CODATA, SI 9th edition).
pub const AVOGADRO: f64 = 6.022_140_76e23;

// IUPAC standard atomic weights, the conventional values of the 2021 table. Every mass below is
// worked out from these, so a reader can check any of them with a formula and a pencil.
const C: f64 = 12.011;
const H: f64 = 1.008;
const N: f64 = 14.007;
const O: f64 = 15.999;
const P: f64 = 30.973_761_998;
const S: f64 = 32.06;

/// One water, in g/mol: lost when two residues join, gained back at the ends of the finished chain.
pub const WATER_DA: f64 = 2.0 * H + O;

/// The mass of `C`c`H`h`N`n`O`o`S`s, in g/mol.
fn formula_weight(c: u32, h: u32, n: u32, o: u32, s: u32) -> f64 {
    f64::from(c) * C + f64::from(h) * H + f64::from(n) * N + f64::from(o) * O + f64::from(s) * S
}

/// Whether a mass was worked out from the bases or residues themselves, or from an average because
/// only a length was known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MassBasis {
    /// Added up base by base.
    Composition,
    /// A length times a conventional average per base pair or per base.
    Average,
}

impl MassBasis {
    /// How the mass was arrived at, for a reply that has to be traceable.
    pub fn words(self) -> &'static str {
        match self {
            MassBasis::Composition => "the base composition of the sequence given",
            MassBasis::Average => "a conventional average, because only a length was given",
        }
    }
}

// --- DNA ---

/// The conventional average mass of one base pair of double-stranded DNA, in g/mol. Used only when
/// no sequence is known; give the bases and the composition is added up instead.
pub const AVERAGE_BP_DA: f64 = 650.0;

/// The conventional average mass of one nucleotide of single-stranded DNA, in g/mol.
pub const AVERAGE_BASE_DA: f64 = 330.0;

/// What one deoxynucleotide weighs inside a strand, in g/mol: its 5'-monophosphate less the water
/// lost forming the phosphodiester bond to the next one. From the atomic weights above and the
/// nucleotide formulas dAMP C10H14N5O6P, dCMP C9H14N3O7P, dGMP C10H14N5O7P, dTMP C10H15N2O8P,
/// which give 313.21, 289.18, 329.21 and 304.20 — the numbers the published oligonucleotide mass
/// formula is written with.
fn base_weight(base: u8) -> Option<f64> {
    let (c, h, n, o) = match base {
        b'A' | b'a' => (10, 14, 5, 6),
        b'C' | b'c' => (9, 14, 3, 7),
        b'G' | b'g' => (10, 14, 5, 7),
        b'T' | b't' => (10, 15, 2, 8),
        _ => return None,
    };
    Some(formula_weight(c, h, n, o, 0) + P - WATER_DA)
}

/// What the two ends of a synthesised strand add, in g/mol: the water that closes the chain, less
/// the 5'-phosphate the strand does not carry (HPO3). This is the -61.96 term of the published
/// oligonucleotide mass formula, and it is why an oligo is quoted with free 5'-OH and 3'-OH ends.
const STRAND_ENDS_DA: f64 = WATER_DA - (H + P + 3.0 * O);

/// What one strand of these bases weighs, in g/mol, with free 5'-OH and 3'-OH ends. `None` when a
/// letter is not A, C, G or T, since an ambiguous base has no one mass.
pub fn strand_weight(bases: &str) -> Option<f64> {
    if bases.is_empty() {
        return None;
    }
    let mut total = STRAND_ENDS_DA;
    for base in bases.bytes() {
        total += base_weight(base)?;
    }
    Some(total)
}

/// What a double-stranded piece of these bases weighs, in g/mol: this strand plus the one that
/// pairs with it. `None` when a letter is not A, C, G or T.
pub fn duplex_weight(bases: &str) -> Option<f64> {
    let complement = crate::seq::revcomp(bases);
    Some(strand_weight(bases)? + strand_weight(&complement)?)
}

/// What a piece of DNA of this length weighs, in g/mol, from the conventional average alone.
pub fn average_weight(length: u64, double_stranded: bool) -> f64 {
    let per = if double_stranded { AVERAGE_BP_DA } else { AVERAGE_BASE_DA };
    length as f64 * per
}

/// What a piece of DNA weighs, in g/mol, and how that was arrived at: from the bases when they are
/// known and plain, from the average otherwise. `None` for nothing at all.
pub fn dna_weight(bases: Option<&str>, length: u64, double_stranded: bool) -> Option<(f64, MassBasis)> {
    if let Some(bases) = bases
        && let Some(weight) = if double_stranded { duplex_weight(bases) } else { strand_weight(bases) }
    {
        return Some((weight, MassBasis::Composition));
    }
    (length > 0).then(|| (average_weight(length, double_stranded), MassBasis::Average))
}

/// The picomoles in `ng` nanograms of something of this molar mass. `None` for a mass that is not
/// a positive number, which would divide by zero.
pub fn pmol_from_ng(ng: f64, weight: f64) -> Option<f64> {
    (weight > 0.0 && weight.is_finite()).then(|| ng / weight * 1000.0)
}

/// The nanograms in `pmol` picomoles of something of this molar mass.
pub fn ng_from_pmol(pmol: f64, weight: f64) -> f64 {
    pmol * weight / 1000.0
}

/// The molecules in `pmol` picomoles.
pub fn molecules_from_pmol(pmol: f64) -> f64 {
    pmol * 1e-12 * AVOGADRO
}

/// The picomoles that many molecules are.
pub fn pmol_from_molecules(molecules: f64) -> f64 {
    molecules / AVOGADRO * 1e12
}

/// The micromolar concentration of `pmol` picomoles in `volume_ul` microlitres. `None` for no
/// volume. Picomoles per microlitre is already micromolar, so this is a division and nothing else.
pub fn um_from_pmol(pmol: f64, volume_ul: f64) -> Option<f64> {
    (volume_ul > 0.0 && volume_ul.is_finite()).then(|| pmol / volume_ul)
}

/// The microlitres that `pmol` picomoles must be dissolved in to come out at `target_um`
/// micromolar. `None` for a target that is not a positive number.
pub fn volume_for_um(pmol: f64, target_um: f64) -> Option<f64> {
    (target_um > 0.0 && target_um.is_finite()).then(|| pmol / target_um)
}

// --- Protein ---

/// One amino-acid residue inside a chain: the free amino acid less a water, as C, H, N, O and S
/// counts. The twenty the genetic codes translate to; anything else has no settled mass here.
fn residue_formula(aa: char) -> Option<(u32, u32, u32, u32, u32)> {
    Some(match aa.to_ascii_uppercase() {
        'G' => (2, 3, 1, 1, 0),
        'A' => (3, 5, 1, 1, 0),
        'S' => (3, 5, 1, 2, 0),
        'P' => (5, 7, 1, 1, 0),
        'V' => (5, 9, 1, 1, 0),
        'T' => (4, 7, 1, 2, 0),
        'C' => (3, 5, 1, 1, 1),
        'L' => (6, 11, 1, 1, 0),
        'I' => (6, 11, 1, 1, 0),
        'N' => (4, 6, 2, 2, 0),
        'D' => (4, 5, 1, 3, 0),
        'Q' => (5, 8, 2, 2, 0),
        'K' => (6, 12, 2, 1, 0),
        'E' => (5, 7, 1, 3, 0),
        'M' => (5, 9, 1, 1, 1),
        'H' => (6, 7, 3, 1, 0),
        'F' => (9, 9, 1, 1, 0),
        'R' => (6, 12, 4, 1, 0),
        'Y' => (9, 9, 1, 2, 0),
        'W' => (11, 10, 2, 1, 0),
        _ => return None,
    })
}

/// What one amino-acid residue weighs inside a chain, in g/mol. `None` for a letter that is not one
/// of the twenty.
pub fn residue_weight(aa: char) -> Option<f64> {
    let (c, h, n, o, s) = residue_formula(aa)?;
    Some(formula_weight(c, h, n, o, s))
}

/// What a polypeptide weighs, in g/mol: its residues plus the water at its two ends. A trailing
/// stop is not a residue and must be removed first. `None` when a letter is not one of the twenty,
/// since then the chain has no one mass.
pub fn protein_weight(protein: &str) -> Option<f64> {
    if protein.is_empty() {
        return None;
    }
    let mut total = WATER_DA;
    for aa in protein.chars() {
        total += residue_weight(aa)?;
    }
    Some(total)
}

/// Molar absorption at 280 nm, in M-1 cm-1, per tryptophan of a protein in water (Pace, Vajdos,
/// Fee, Grimsley and Gray 1995, "How to measure and predict the molar absorption coefficient of a
/// protein", Protein Science 4:2411-2423).
pub const TRP_E280: f64 = 5500.0;

/// The same, per tyrosine.
pub const TYR_E280: f64 = 1490.0;

/// The same, per cystine: a pair of cysteines joined by a disulfide bond. A free cysteine adds
/// nothing at 280 nm.
pub const CYSTINE_E280: f64 = 125.0;

/// The molar absorption coefficient at 280 nm, in M-1 cm-1, of a protein with these counts.
pub fn extinction_280(trp: usize, tyr: usize, cystines: usize) -> f64 {
    trp as f64 * TRP_E280 + tyr as f64 * TYR_E280 + cystines as f64 * CYSTINE_E280
}

/// How many of each residue a protein holds, each letter once, in the order it first appears.
pub fn composition(protein: &str) -> Vec<(char, usize)> {
    let mut counts: Vec<(char, usize)> = Vec::new();
    for aa in protein.chars().map(|a| a.to_ascii_uppercase()) {
        match counts.iter_mut().find(|(seen, _)| *seen == aa) {
            Some((_, count)) => *count += 1,
            None => counts.push((aa, 1)),
        }
    }
    counts
}

/// How many of one residue a protein holds.
pub fn residue_count(protein: &str, aa: char) -> usize {
    protein.chars().filter(|r| r.to_ascii_uppercase() == aa).count()
}

#[cfg(test)]
mod tests {
    use super::{
        AVOGADRO, MassBasis, WATER_DA, average_weight, composition, dna_weight, duplex_weight, extinction_280, molecules_from_pmol, ng_from_pmol,
        pmol_from_molecules, pmol_from_ng, protein_weight, residue_count, residue_weight, strand_weight, um_from_pmol, volume_for_um,
    };

    /// Rounded to `places`, so a worked-by-hand answer can be compared without chasing the last bit.
    fn at(value: f64, places: i32) -> f64 {
        let scale = 10f64.powi(places);
        (value * scale).round() / scale
    }

    #[test]
    fn a_water_weighs_what_its_atoms_weigh() {
        // H 1.008 x 2 + O 15.999 = 18.015.
        assert_eq!(at(WATER_DA, 3), 18.015);
    }

    #[test]
    fn a_base_in_a_strand_weighs_what_the_published_oligo_formula_says() {
        // The formula every oligo supplier quotes, in g/mol: A 313.21, C 289.18, G 329.21,
        // T 304.20, and -61.96 for the two free ends. Those constants are rounded to two places
        // and were worked from an older atomic-weight table, so Bios's own arithmetic from the
        // IUPAC 2021 weights sits within 0.01 of each of them. dTMP is the one that lands on the
        // boundary: C10H15N2O8P less a water is 304.195 here against the published 304.20.
        let published = [('A', 313.21), ('C', 289.18), ('G', 329.21), ('T', 304.20)];
        for (base, quoted) in published {
            let one = strand_weight(&base.to_string()).unwrap() - (18.015 - 79.979);
            assert!((one - quoted).abs() < 0.01, "{base}: {one} against the published {quoted}");
        }
        // 313.21+289.18+329.21+304.20-61.96 = 1173.84 for one of each base.
        assert!((strand_weight("ACGT").unwrap() - 1173.84).abs() < 0.02);
        // One base of each is its own complement's composition, so a duplex is twice one strand.
        assert_eq!(at(duplex_weight("ACGT").unwrap(), 6), at(2.0 * strand_weight("ACGT").unwrap(), 6));
        assert_eq!(strand_weight("ANGT"), None);
        assert_eq!(strand_weight(""), None);
    }

    #[test]
    fn an_average_is_used_only_when_no_bases_are_known() {
        assert_eq!(dna_weight(None, 1000, true), Some((650_000.0, MassBasis::Average)));
        assert_eq!(dna_weight(None, 1000, false), Some((330_000.0, MassBasis::Average)));
        assert_eq!(dna_weight(Some("ANGT"), 4, true), Some((2600.0, MassBasis::Average)));
        assert_eq!(dna_weight(Some("ACGT"), 4, true).map(|(_, basis)| basis), Some(MassBasis::Composition));
        assert_eq!(dna_weight(None, 0, true), None);
        assert_eq!(average_weight(2686, true), 1_745_900.0);
    }

    #[test]
    fn nanograms_picomoles_and_molecules_convert_both_ways() {
        // 1 ug of 1 kb double-stranded DNA at 650 g/mol per base pair: 1000/650000 nmol = 1.538 pmol.
        let kb = average_weight(1000, true);
        assert_eq!(at(pmol_from_ng(1000.0, kb).unwrap(), 3), 1.538);
        assert_eq!(at(ng_from_pmol(1.538_461_5, kb), 3), 1000.0);
        // A picomole is 1e-12 mol, so it is Avogadro's number divided by a million million.
        assert_eq!(molecules_from_pmol(1.0), AVOGADRO / 1e12);
        assert_eq!(at(pmol_from_molecules(molecules_from_pmol(2.5)), 9), 2.5);
        assert_eq!(pmol_from_ng(1.0, 0.0), None);
    }

    #[test]
    fn picomoles_per_microlitre_are_micromolar() {
        assert_eq!(um_from_pmol(100.0, 50.0), Some(2.0));
        assert_eq!(um_from_pmol(100.0, 0.0), None);
        // 20 nmol of oligo to 100 uM wants 200 uL.
        assert_eq!(volume_for_um(20_000.0, 100.0), Some(200.0));
        assert_eq!(volume_for_um(20_000.0, 0.0), None);
    }

    #[test]
    fn a_residue_weighs_what_its_atoms_weigh() {
        // Gly C2H3NO: 2x12.011 + 3x1.008 + 14.007 + 15.999 = 57.052.
        assert_eq!(at(residue_weight('G').unwrap(), 3), 57.052);
        // Trp C11H10N2O: 132.121 + 10.080 + 28.014 + 15.999 = 186.214.
        assert_eq!(at(residue_weight('W').unwrap(), 3), 186.214);
        // Cys C3H5NOS: 36.033 + 5.040 + 14.007 + 15.999 + 32.060 = 103.139.
        assert_eq!(at(residue_weight('C').unwrap(), 3), 103.139);
        // Leu and Ile are both C6H11NO, so they weigh the same.
        assert_eq!(residue_weight('L'), residue_weight('I'));
        assert_eq!(residue_weight('*'), None);
        assert_eq!(residue_weight('X'), None);
    }

    #[test]
    fn a_protein_weighs_its_residues_plus_one_water() {
        // Gly-Gly by hand: 57.052 x 2 + 18.015 = 132.119.
        assert_eq!(at(protein_weight("GG").unwrap(), 3), 132.119);
        // One tryptophan: 186.214 + 18.015 = 204.229.
        assert_eq!(at(protein_weight("W").unwrap(), 3), 204.229);
        assert_eq!(protein_weight("MA*"), None);
        assert_eq!(protein_weight(""), None);
    }

    #[test]
    fn absorption_at_280_is_pace_1995_arithmetic() {
        assert_eq!(extinction_280(1, 0, 0), 5500.0);
        assert_eq!(extinction_280(0, 1, 0), 1490.0);
        assert_eq!(extinction_280(2, 3, 1), 2.0 * 5500.0 + 3.0 * 1490.0 + 125.0);
        assert_eq!(extinction_280(0, 0, 0), 0.0);
    }

    #[test]
    fn a_composition_counts_every_letter_once() {
        assert_eq!(composition("MAAW"), vec![('M', 1), ('A', 2), ('W', 1)]);
        assert_eq!(residue_count("MAAW", 'A'), 2);
        assert_eq!(residue_count("MAAW", 'C'), 0);
    }
}
