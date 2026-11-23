//! Sequence maths: complement, GC content, melting temperature, translation and open reading frames.
//!
//! Ported from the Bios web app. Where a function is handed text that is not DNA, it answers as the web app does.

use crate::model::Strand;

/// The complement of one IUPAC base, keeping case. Anything else is returned unchanged.
pub fn complement(base: u8) -> u8 {
    match base {
        b'A' => b'T',
        b'T' | b'U' => b'A',
        b'C' => b'G',
        b'G' => b'C',
        b'R' => b'Y',
        b'Y' => b'R',
        b'K' => b'M',
        b'M' => b'K',
        b'B' => b'V',
        b'V' => b'B',
        b'D' => b'H',
        b'H' => b'D',
        b'a' => b't',
        b't' | b'u' => b'a',
        b'c' => b'g',
        b'g' => b'c',
        b'r' => b'y',
        b'y' => b'r',
        b'k' => b'm',
        b'm' => b'k',
        b'b' => b'v',
        b'v' => b'b',
        b'd' => b'h',
        b'h' => b'd',
        other => other,
    }
}

pub fn revcomp(seq: &str) -> String {
    seq.bytes().rev().map(|b| complement(b) as char).collect()
}

/// The letters of `seq`, uppercased. Digits, spaces and every other character are dropped.
pub fn clean_seq(seq: &str) -> String {
    seq.chars().filter(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase()).collect()
}

/// What the web app's `\s` matches: Unicode white space and the byte-order mark, but not U+0085. Numbered and wrapped sequence text is
/// accepted exactly where the web app accepts it.
pub(crate) fn is_js_whitespace(c: char) -> bool {
    c == '\u{feff}' || (c.is_whitespace() && c != '\u{85}')
}

/// Legacy GC lower bound as a percentage, from 0 to 100: G, C and S count. An empty sequence is 0. Use [`gc_content_range`] for a
/// sequence with ambiguity codes.
pub fn gc_content(seq: &str) -> f64 {
    let upper = seq.to_uppercase();
    // The web app divides by the length in UTF-16 units, so text that is not DNA gets the same figure here as there.
    let len = upper.encode_utf16().count();
    if len == 0 {
        return 0.0;
    }
    let gc = upper.chars().filter(|c| matches!(c, 'G' | 'C' | 'S')).count();
    100.0 * gc as f64 / len as f64
}

/// The lowest and highest GC percentage a sequence can have, from 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GcRange {
    pub min: f64,
    pub max: f64,
}

/// Possible GC percentages for IUPAC DNA; no probability distribution over unknown bases is assumed. `None` for an empty sequence and
/// for one holding anything but IUPAC DNA letters, white space and digits.
pub fn gc_content_range(seq: &str) -> Option<GcRange> {
    let iupac = |c: char| matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T' | 'R' | 'Y' | 'S' | 'W' | 'K' | 'M' | 'B' | 'D' | 'H' | 'V' | 'N');
    if seq.chars().any(|c| !(iupac(c) || is_js_whitespace(c) || c.is_ascii_digit())) {
        return None;
    }
    let s = clean_seq(seq);
    if s.is_empty() {
        return None;
    }
    let (mut lo, mut hi) = (0usize, 0usize);
    for base in s.bytes() {
        if matches!(base, b'G' | b'C' | b'S') {
            lo += 1;
        }
        if !matches!(base, b'A' | b'T' | b'W') {
            hi += 1;
        }
    }
    Some(GcRange { min: 100.0 * lo as f64 / s.len() as f64, max: 100.0 * hi as f64 / s.len() as f64 })
}

/// The web app's `toFixed`, for a value that is finite and not negative. Rust's own formatting sends an exact half to the even digit
/// (12.5 prints as "12") where the web app rounds it up ("13"), and a GC figure such as 1 base in 8 lands exactly on a half.
fn to_fixed(value: f64, digits: u8) -> String {
    let digits = usize::from(digits);
    // Every f64 is a finite decimal of at most 1074 places, so this is the value itself with nothing rounded away, and the first
    // digit dropped below decides the rounding alone.
    let exact = format!("{value:.1074}");
    let (whole, places) = exact.split_once('.').unwrap_or((exact.as_str(), ""));
    let mut kept: Vec<u8> = whole.bytes().chain(places.bytes().take(digits)).collect();
    if places.as_bytes().get(digits).is_some_and(|dropped| *dropped >= b'5') {
        let mut carried = true;
        for digit in kept.iter_mut().rev() {
            if *digit == b'9' {
                *digit = b'0';
            } else {
                // Only digits are here for the values this is given; anything else is left as it is rather than made into nonsense.
                if digit.is_ascii_digit() {
                    *digit += 1;
                }
                carried = false;
                break;
            }
        }
        if carried {
            kept.insert(0, b'1');
        }
    }
    let point = kept.len().saturating_sub(digits);
    let mut out = String::with_capacity(kept.len() + 1);
    for (i, digit) in kept.iter().enumerate() {
        if i == point && digits > 0 {
            out.push('.');
        }
        out.push(char::from(*digit));
    }
    out
}

/// The GC content as text, to `digits` decimal places (the web app defaults to 0). Range endpoints round outward so uncertainty
/// cannot disappear through display rounding.
pub fn format_gc_content(seq: &str, digits: u8) -> String {
    let Some(range) = gc_content_range(seq) else {
        return "GC unavailable".to_string();
    };
    if range.min == range.max {
        return format!("{}% GC", to_fixed(range.min, digits));
    }
    let scale = 10f64.powi(i32::from(digits));
    format!("{}–{}% GC", to_fixed((range.min * scale).floor() / scale, digits), to_fixed((range.max * scale).ceil() / scale, digits))
}

/// The conditions [`tm`] is given when none are chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmDefaults {
    /// Total strand concentration, nM.
    pub concentration_nm: f64,
    /// Sodium concentration, mM.
    pub sodium_mm: f64,
}

pub const DNA_TM_DEFAULTS: TmDefaults = TmDefaults { concentration_nm: 250.0, sodium_mm: 50.0 };
pub const DEFAULT_TM_DESCRIPTION: &str = "Perfect DNA duplex estimate: SantaLucia 1998, 250 nM total strands, 50 mM Na+. Equal strands (Ct/4), or one self-complementary pool (Ct) with symmetry correction. No magnesium, mismatch or gap correction.";

/// Gas constant, cal/(mol·K).
const R_GAS: f64 = 1.987;

/// ΔH (kcal/mol) and ΔS (cal/mol·K) of one nearest-neighbour pair: the SantaLucia 1998 unified parameters.
fn nearest_neighbour(first: u8, second: u8) -> Option<(f64, f64)> {
    Some(match (first, second) {
        (b'A', b'A') | (b'T', b'T') => (-7.9, -22.2),
        (b'A', b'T') => (-7.2, -20.4),
        (b'T', b'A') => (-7.2, -21.3),
        (b'C', b'A') | (b'T', b'G') => (-8.5, -22.7),
        (b'G', b'T') | (b'A', b'C') => (-8.4, -22.4),
        (b'C', b'T') | (b'A', b'G') => (-7.8, -21.0),
        (b'G', b'A') | (b'T', b'C') => (-8.2, -22.2),
        (b'C', b'G') => (-10.6, -27.2),
        (b'G', b'C') => (-9.8, -24.4),
        (b'G', b'G') | (b'C', b'C') => (-8.0, -19.9),
        _ => return None,
    })
}

/// SantaLucia 1998 perfect DNA-duplex Tm, °C, at a total strand concentration in nM and a sodium concentration in mM
/// ([`DNA_TM_DEFAULTS`] when none are chosen). It is checked against recorded Biopython `DNA_NN3` answers.
///
/// Unsupported bases and conditions have no scalar estimate: `None`, never a figure from an unrelated formula. The terms are added in
/// the web app's order, so nothing but the last bit of the platform's logarithm can tell the two apart.
pub fn tm(seq: &str, conc_nm: f64, na_mm: f64) -> Option<f64> {
    let s = clean_seq(seq);
    let bases = s.as_bytes();
    let unsupported = seq.chars().any(|c| !(matches!(c.to_ascii_uppercase(), 'A' | 'C' | 'G' | 'T') || is_js_whitespace(c) || c.is_ascii_digit()));
    if bases.len() < 2 || unsupported || !conc_nm.is_finite() || conc_nm <= 0.0 || !na_mm.is_finite() || na_mm <= 0.0 {
        return None;
    }
    let (ct, na) = (conc_nm * 1e-9, na_mm / 1000.0);
    // A concentration too small to survive the change of unit has become zero, and zero has no logarithm.
    if ct <= 0.0 || na <= 0.0 {
        return None;
    }
    let mut dh = 0.0;
    let mut ds = 0.0;
    for end in [*bases.first()?, *bases.last()?] {
        if matches!(end, b'G' | b'C') {
            dh += 0.1;
            ds += -2.8;
        } else {
            dh += 2.3;
            ds += 4.1;
        }
    }
    for pair in bases.windows(2) {
        let (pair_h, pair_s) = nearest_neighbour(*pair.first()?, *pair.last()?)?;
        dh += pair_h;
        ds += pair_s;
    }
    let self_complementary = s == revcomp(&s);
    if self_complementary {
        ds -= 1.4;
    }
    ds += 0.368 * (bases.len() - 1) as f64 * na.ln();
    let tm_k = (dh * 1000.0) / (ds + R_GAS * (ct / if self_complementary { 1.0 } else { 4.0 }).ln());
    (tm_k.is_finite() && tm_k > 0.0).then_some(tm_k - 273.15)
}

/// Where a base sits in the T, C, A, G order that genetic code tables are written in.
fn base_index(base: u8) -> Option<usize> {
    match base {
        b'T' => Some(0),
        b'C' => Some(1),
        b'A' => Some(2),
        b'G' => Some(3),
        _ => None,
    }
}

/// The standard genetic code, one residue per codon, the codons in T, C, A, G order at each position.
const STANDARD_CODE: &[u8; 64] = b"FFLLSSSSYY**CC*WLLLLPPPPHHQQRRRRIIIMTTTTNNKKSSRRVVVVAAAADDEEGGGG";

/// The residue of one codon in the standard code. Anything that is not three of A, C, G and T is "X".
fn standard_residue(codon: &[u8]) -> char {
    let [first, second, third] = codon else {
        return 'X';
    };
    let (Some(first), Some(second), Some(third)) = (base_index(*first), base_index(*second), base_index(*third)) else {
        return 'X';
    };
    STANDARD_CODE.get(first * 16 + second * 4 + third).map_or('X', |residue| char::from(*residue))
}

/// Translate a DNA sequence to protein (standard code). Unknown codons become "X"; bases past the last whole codon are dropped.
pub fn translate(seq: &str) -> String {
    clean_seq(seq).as_bytes().as_chunks::<3>().0.iter().map(|codon| standard_residue(codon)).collect()
}

/// An open reading frame: an ATG through the first stop in its frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orf {
    /// 1-based inclusive start on the forward strand. For a forward ORF this is the A of its ATG; for a reverse one it is the base
    /// under the last base of its stop codon.
    pub start: usize,
    /// 1-based inclusive end on the forward strand. For a forward ORF this is the last base of its stop codon; for a reverse one it
    /// is the base under the A of its ATG.
    pub end: usize,
    pub strand: Strand,
    /// Protein including the trailing stop as "*".
    pub protein: String,
    /// Length in amino acids, excluding the stop.
    pub aa_length: usize,
}

/// Scan both strands for ORFs (ATG to stop) of at least `min_aa` amino acids, which the web app defaults to 30. Coordinates are
/// 1-based inclusive on the forward strand. The result is ordered by start, the reverse strand first where two share one.
pub fn find_orfs(seq: &str, min_aa: usize) -> Vec<Orf> {
    let fwd = clean_seq(seq);
    let mut orfs = Vec::new();
    scan_orfs(fwd.as_bytes(), Strand::Forward, min_aa, &mut orfs);
    scan_orfs(revcomp(&fwd).as_bytes(), Strand::Reverse, min_aa, &mut orfs);
    orfs.sort_by(|a, b| a.start.cmp(&b.start).then(a.strand.sign().cmp(&b.strand.sign())));
    orfs
}

fn scan_orfs(s: &[u8], strand: Strand, min_aa: usize, orfs: &mut Vec<Orf>) {
    for frame in 0..3 {
        let mut i = frame;
        while i + 3 <= s.len() {
            if s.get(i..i + 3) != Some(b"ATG") {
                i += 3;
                continue;
            }
            let mut j = i;
            let mut protein = String::new();
            let mut stopped = false;
            while let Some(codon) = s.get(j..j + 3) {
                let residue = standard_residue(codon);
                protein.push(residue);
                j += 3;
                if residue == '*' {
                    stopped = true;
                    break;
                }
            }
            // The web app goes on to try every later ATG of the frame. Moving on at once finds the same ORFs: with no stop ahead no
            // later ATG has one either, and an ATG inside a reading too short to keep ends at the same stop with a shorter reading
            // still. Trying them takes time that grows with the square of the length of a sequence made of ATGs.
            if !stopped {
                break;
            }
            let aa_length = protein.len().saturating_sub(1);
            if aa_length >= min_aa {
                // `j` is one past the stop codon on this strand's reading.
                let (start, end) = match strand {
                    Strand::Forward => (i + 1, j),
                    Strand::Reverse => ((s.len() + 1).saturating_sub(j), s.len().saturating_sub(i)),
                };
                orfs.push(Orf { start, end, strand, protein, aa_length });
            }
            i = j;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DNA_TM_DEFAULTS, Orf, clean_seq, find_orfs, format_gc_content, gc_content, gc_content_range, revcomp, tm, to_fixed, translate};
    use crate::coding::genetic_code;
    use crate::model::Strand;

    #[test]
    fn revcomp_reverses_and_complements() {
        assert_eq!(revcomp("ATGCA"), "TGCAT");
        assert_eq!(revcomp("aacN"), "Ngtt");
    }

    #[test]
    fn clean_seq_keeps_only_letters() {
        assert_eq!(clean_seq("  1 acgt nnRY\n61 tt-a*"), "ACGTNNRYTTA");
    }

    #[test]
    fn gc_of_half() {
        assert_eq!(gc_content("ATGC"), 50.0);
        assert_eq!(gc_content("atSw"), 25.0);
        // "ß" uppercases to "SS", and the web app counts both.
        assert_eq!(gc_content("gcß"), 100.0);
        assert_eq!(gc_content(""), 0.0);
    }

    #[test]
    fn white_space_is_what_the_web_app_calls_white_space() {
        let at_defaults = |seq: &str| tm(seq, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm);
        assert_eq!(at_defaults("ACGT\u{feff}AC GT\n"), at_defaults("ACGTACGT"));
        assert!(at_defaults("ACGTACGT").is_some());
        assert_eq!(at_defaults("ACGT\u{85}ACGT"), None);
        assert_eq!(gc_content_range("AC\u{85}GT"), None);
    }

    #[test]
    fn an_exact_half_rounds_up_as_in_the_web_app() {
        assert_eq!(to_fixed(12.5, 0), "13");
        assert_eq!(to_fixed(0.5, 0), "1");
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(6.25, 1), "6.3");
        assert_eq!(to_fixed(100.0, 2), "100.00");
        // One base in eight and one in sixteen are exact halves at their last place.
        assert_eq!(format_gc_content("GATATATA", 0), "13% GC");
        assert_eq!(format_gc_content("GATATATATATATATA", 1), "6.3% GC");
    }

    #[test]
    fn translate_reads_whole_codons_of_the_standard_code() {
        assert_eq!(translate("atg gcc TAA c"), "MA*");
        assert_eq!(translate("ATGNNNAUG"), "MXX");
        let codons: String =
            "TCAG".chars().flat_map(|a| "TCAG".chars().flat_map(move |b| "TCAG".chars().map(move |c| format!("{a}{b}{c}")))).collect();
        assert_eq!(Some(translate(&codons).as_str()), genetic_code(1).map(|code| code.amino_acids));
    }

    #[test]
    fn orfs_are_found_on_both_strands_in_forward_coordinates() {
        let forward = "CCATGGCTTAACC";
        let orf = |start, end, strand| Orf { start, end, strand, protein: "MA*".to_string(), aa_length: 2 };
        assert_eq!(find_orfs(forward, 2), vec![orf(3, 11, Strand::Forward)]);
        assert_eq!(find_orfs(&revcomp(forward), 2), vec![orf(3, 11, Strand::Reverse)]);
        assert_eq!(find_orfs(forward, 3), vec![]);
    }

    #[test]
    fn an_atg_inside_a_reading_too_short_to_keep_is_not_an_orf() {
        assert_eq!(find_orfs("ATGATGTAA", 2).len(), 1);
        assert_eq!(find_orfs("ATGATGTAA", 3), vec![]);
        // Without a stop the web app's scan would read to the end from each of these in turn.
        assert_eq!(find_orfs(&"ATG".repeat(200_000), 1), vec![]);
    }
}
