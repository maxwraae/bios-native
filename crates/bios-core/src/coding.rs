//! How a coding feature says it is translated (reading frame, genetic code and partial ends), and the translation those settings give.
//!
//! Ported from the Bios web app. Text is measured in UTF-16 units and uppercased by the full Unicode rules, as the web app does, so
//! that bases which are not DNA fall into the same codons here as there.

use crate::seq::is_js_whitespace;

pub use crate::genetic_codes_data::GENETIC_CODES;

/// One NCBI genetic code. `amino_acids` and `start_stops` hold one character per codon, the codons in T, C, A, G order at each
/// position; `start_stops` marks an initiation codon "M" and a termination codon "*".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneticCode {
    pub id: u64,
    pub name: &'static str,
    pub amino_acids: &'static str,
    pub start_stops: &'static str,
}

/// How the first codon is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartCodonMode {
    Auto,
    Methionine,
    Ordinary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodingSettings {
    /// Number of leading bases before the first complete codon (GenBank `codon_start` minus 1).
    pub phase: u64,
    pub table_id: u64,
    /// `None` when `bios_start_codon` holds something that is not a mode.
    pub start_mode: Option<StartCodonMode>,
    pub partial_start: bool,
    pub partial_end: bool,
    /// Why no verified translation can be given, when one cannot.
    pub unavailable: Option<String>,
}

/// The NCBI genetic codes Bios translates with, by `transl_table` number.
pub const GENETIC_CODE_IDS: [u64; 27] = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 15, 16, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33];

/// The genetic code with this `transl_table` number, when Bios carries it.
pub fn genetic_code(id: u64) -> Option<&'static GeneticCode> {
    GENETIC_CODES.iter().find(|code| code.id == id)
}

/// The largest whole number the web app can hold exactly; a larger qualifier value is malformed.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn note<'a>(notes: &'a [(String, Vec<String>)], key: &str) -> Option<&'a [String]> {
    notes.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice())
}

/// A qualifier's value when it has exactly one.
fn scalar(values: &[String]) -> Option<&str> {
    match values {
        [only] => Some(only.as_str()),
        _ => None,
    }
}

fn positive_integer(values: &[String]) -> Option<u64> {
    let digits = scalar(values)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Leading zeros carry no value and must not push a small number past what `u64` parses. All
    // zeros leave nothing to parse, which is right: zero is not positive.
    let n: u64 = digits.trim_start_matches('0').parse().ok()?;
    (n <= MAX_SAFE_INTEGER).then_some(n)
}

fn true_value(values: Option<&[String]>) -> bool {
    values.and_then(scalar).is_some_and(|v| v.to_lowercase() == "true")
}

/// Reads saved translation settings from a feature's qualifiers. Malformed or unsupported
/// metadata is never silently replaced: it makes the translation unavailable and says why.
pub fn read_coding_settings(notes: &[(String, Vec<String>)]) -> CodingSettings {
    let start = match note(notes, "codon_start") {
        None => Some(1),
        Some(values) => positive_integer(values),
    };
    let table_id = match note(notes, "transl_table") {
        None => Some(1),
        Some(values) => positive_integer(values),
    };
    let start_mode = match note(notes, "bios_start_codon") {
        None => Some(StartCodonMode::Auto),
        Some(values) => match scalar(values) {
            Some("auto") => Some(StartCodonMode::Auto),
            Some("methionine") => Some(StartCodonMode::Methionine),
            Some("ordinary") => Some(StartCodonMode::Ordinary),
            _ => None,
        },
    };
    let partial =
        note(notes, "note").is_some_and(|values| values.iter().any(|v| v.get(..13).is_some_and(|head| head.eq_ignore_ascii_case("bios-partial:"))));

    let unavailable = if start.is_none_or(|s| s > 3) {
        Some("codon_start must be 1, 2 or 3".to_string())
    } else if table_id.is_none_or(|id| !GENETIC_CODE_IDS.contains(&id)) {
        Some("transl_table is not a supported NCBI genetic code".to_string())
    } else if start_mode.is_none() {
        Some("the first-codon translation setting is invalid".to_string())
    } else if ["transl_except", "ribosomal_slippage", "exception", "pseudo", "pseudogene"].iter().any(|key| note(notes, key).is_some()) {
        Some("the CDS carries translation exceptions or pseudogene metadata".to_string())
    } else if true_value(note(notes, "bios_translation_unavailable")) {
        let reason = note(notes, "bios_translation_unavailable_reason").and_then(scalar);
        // The byte-order mark counts as blank here because it does in the web app.
        let stated = reason.filter(|r| !r.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}').is_empty());
        Some(stated.unwrap_or("the saved coding metadata does not establish a verified translation").to_string())
    } else {
        None
    };

    let start = start.unwrap_or(1);
    CodingSettings {
        phase: start - 1,
        table_id: table_id.unwrap_or(1),
        start_mode,
        partial_start: partial || start > 1 || true_value(note(notes, "bios_cds_partial_start")),
        partial_end: partial || true_value(note(notes, "bios_cds_partial_end")),
        unavailable,
    }
}

/// Why these settings give no translation. An empty reason is no reason, as in the web app.
fn why_unavailable(settings: &CodingSettings) -> Option<&str> {
    settings.unavailable.as_deref().filter(|reason| !reason.is_empty())
}

/// The bases an IUPAC letter stands for, each as its place in a genetic code table's T, C, A, G order.
fn iupac(base: char) -> Option<&'static [usize]> {
    const T: usize = 0;
    const C: usize = 1;
    const A: usize = 2;
    const G: usize = 3;
    Some(match base {
        'A' => &[A],
        'C' => &[C],
        'G' => &[G],
        'T' | 'U' => &[T],
        'R' => &[A, G],
        'Y' => &[C, T],
        'S' => &[C, G],
        'W' => &[A, T],
        'K' => &[G, T],
        'M' => &[A, C],
        'B' => &[C, G, T],
        'D' => &[A, G, T],
        'H' => &[A, C, T],
        'V' => &[A, C, G],
        'N' => &[A, C, G, T],
        _ => return None,
    })
}

/// What each of a codon's three letters may be.
type Letters = [&'static [usize]; 3];

/// The letters of a codon from its characters. `None` when any of them is not an IUPAC base or there are fewer than three. More than
/// three can only come from a character that uppercases to several ("ß" to "SS"); the web app then reads the first three, so this does.
fn letters(codon: impl Iterator<Item = char>) -> Option<Letters> {
    let mut found: Letters = [&[]; 3];
    let mut count = 0;
    for base in codon.flat_map(char::to_uppercase) {
        let bases = iupac(base)?;
        if let Some(slot) = found.get_mut(count) {
            *slot = bases;
        }
        count += 1;
    }
    (count >= 3).then_some(found)
}

/// The letters of a codon given as text, which must be three UTF-16 units long.
fn letters_of(codon: &str) -> Option<Letters> {
    if codon.encode_utf16().count() != 3 {
        return None;
    }
    letters(codon.chars())
}

/// The letters of a codon cut from a sequence. Half of a split surrogate pair is no base.
fn letters_of_units(codon: &[u16]) -> Option<Letters> {
    if codon.len() != 3 {
        return None;
    }
    letters(char::decode_utf16(codon.iter().copied()).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)))
}

/// Where each concrete codon that the letters allow sits in a genetic code table.
fn table_indices(letters: Letters) -> impl Iterator<Item = usize> {
    let [first, second, third] = letters;
    first.iter().flat_map(move |a| second.iter().flat_map(move |b| third.iter().map(move |c| a * 16 + b * 4 + c)))
}

/// Whether every concrete codon the letters allow carries `mark` among the table's start and stop marks.
fn every_codon_is_marked(letters: Option<Letters>, table_id: u64, mark: u8) -> bool {
    let (Some(code), Some(letters)) = (genetic_code(table_id), letters) else {
        return false;
    };
    table_indices(letters).all(|index| code.start_stops.as_bytes().get(index) == Some(&mark))
}

/// Whether the codon initiates translation in this genetic code (the web app defaults to table 1). An ambiguous codon does only when
/// everything it could be does.
pub fn is_start_codon(codon: &str, table_id: u64) -> bool {
    every_codon_is_marked(letters_of(codon), table_id, b'M')
}

/// Whether the codon terminates translation in this genetic code (the web app defaults to table 1). An ambiguous codon does only when
/// everything it could be does.
pub fn is_stop_codon(codon: &str, table_id: u64) -> bool {
    every_codon_is_marked(letters_of(codon), table_id, b'*')
}

fn residue(letters: Option<Letters>, settings: &CodingSettings, first: bool, terminal: bool) -> char {
    let (Some(code), Some(letters)) = (genetic_code(settings.table_id), letters) else {
        return 'X';
    };
    if why_unavailable(settings).is_some() {
        return 'X';
    }
    let mut residues = table_indices(letters).map(|index| {
        let mark = code.start_stops.as_bytes().get(index).copied();
        let auto_start = settings.start_mode == Some(StartCodonMode::Auto) && !settings.partial_start && mark == Some(b'M');
        if first && (settings.start_mode == Some(StartCodonMode::Methionine) || auto_start) {
            'M'
        } else if terminal && !settings.partial_end && mark == Some(b'*') {
            '*'
        } else {
            code.amino_acids.as_bytes().get(index).map_or('X', |aa| char::from(*aa))
        }
    });
    let Some(only) = residues.next() else {
        return 'X';
    };
    if residues.all(|other| other == only) { only } else { 'X' }
}

/// The residue one codon gives: "X" unless every concrete codon it could be gives the same one. `first` and `terminal` say where the
/// codon stands in its CDS (the web app defaults both to false). Context-dependent stops (NCBI 27/28/31) become stops only at an
/// established CDS terminus.
pub fn coding_residue(codon: &str, settings: &CodingSettings, first: bool, terminal: bool) -> char {
    residue(letters_of(codon), settings, first, terminal)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodingTranslation {
    pub settings: CodingSettings,
    /// The bases that were translated: uppercased, white space removed, U read as T, the leading `phase` bases dropped. Half of a
    /// surrogate pair that the phase cut through shows as U+FFFD, since Rust text cannot hold it.
    pub coding_bases: String,
    pub protein: String,
    pub trailing_bases: usize,
    pub valid_start: bool,
    pub valid_stop: bool,
    /// 1-based residue numbers of the stops before the last residue.
    pub internal_stops: Vec<usize>,
    /// Every translated residue is determined; not a claim that a complete protein exists.
    pub verified: bool,
    pub complete: bool,
    pub unavailable: Option<String>,
}

/// Translates a coding sequence as its settings say (the web app defaults to `read_coding_settings` of no qualifiers). Settings that
/// are unavailable give no protein at all.
pub fn translate_coding(bases: &str, settings: CodingSettings) -> CodingTranslation {
    let cleaned: String = bases.to_uppercase().chars().filter(|c| !is_js_whitespace(*c)).map(|c| if c == 'U' { 'T' } else { c }).collect();
    let phase = usize::try_from(settings.phase).unwrap_or(usize::MAX);
    let units: Vec<u16> = cleaned.encode_utf16().skip(phase).collect();
    let trailing_bases = units.len() % 3;
    let count = units.len() / 3;
    let mut protein = String::new();
    if why_unavailable(&settings).is_none() {
        for (i, codon) in units.as_chunks::<3>().0.iter().enumerate() {
            protein.push(residue(letters_of_units(codon), &settings, i == 0, i + 1 == count && trailing_bases == 0));
        }
    }
    let head = units.get(..3).unwrap_or(&units);
    let tail = units.get(units.len().saturating_sub(3)..).unwrap_or(&units);
    let valid_start =
        settings.start_mode == Some(StartCodonMode::Methionine) || every_codon_is_marked(letters_of_units(head), settings.table_id, b'M');
    let valid_stop = trailing_bases == 0 && protein.ends_with('*') && every_codon_is_marked(letters_of_units(tail), settings.table_id, b'*');
    let before_the_stop = protein.chars().count().saturating_sub(usize::from(valid_stop));
    let internal_stops: Vec<usize> = protein.chars().take(before_the_stop).enumerate().filter(|(_, aa)| *aa == '*').map(|(i, _)| i + 1).collect();
    let verified = why_unavailable(&settings).is_none() && count > 0 && !protein.contains('X');
    let complete =
        verified && !settings.partial_start && !settings.partial_end && trailing_bases == 0 && valid_start && valid_stop && internal_stops.is_empty();
    let unavailable = why_unavailable(&settings).map(str::to_string);
    CodingTranslation {
        settings,
        coding_bases: String::from_utf16_lossy(&units),
        protein,
        trailing_bases,
        valid_start,
        valid_stop,
        internal_stops,
        verified,
        complete,
        unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GENETIC_CODE_IDS, GENETIC_CODES, StartCodonMode, coding_residue, is_start_codon, is_stop_codon, read_coding_settings, translate_coding,
    };

    fn notes(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect())).collect()
    }

    #[test]
    fn no_qualifiers_mean_frame_one_of_the_standard_code() {
        let s = read_coding_settings(&[]);
        assert_eq!((s.phase, s.table_id, s.start_mode), (0, 1, Some(StartCodonMode::Auto)));
        assert_eq!((s.partial_start, s.partial_end, s.unavailable), (false, false, None));
    }

    #[test]
    fn a_later_frame_is_a_partial_start() {
        let s = read_coding_settings(&notes(&[("codon_start", &["3"]), ("transl_table", &["11"]), ("bios_start_codon", &["methionine"])]));
        assert_eq!((s.phase, s.table_id, s.start_mode), (2, 11, Some(StartCodonMode::Methionine)));
        assert_eq!((s.partial_start, s.partial_end, s.unavailable), (true, false, None));
    }

    #[test]
    fn malformed_settings_are_reported_in_a_fixed_order() {
        let reason = |pairs: &[(&str, &[&str])]| read_coding_settings(&notes(pairs)).unavailable.unwrap_or_default();
        assert_eq!(reason(&[("codon_start", &["4"]), ("transl_table", &["7"])]), "codon_start must be 1, 2 or 3");
        assert_eq!(reason(&[("codon_start", &["1", "2"])]), "codon_start must be 1, 2 or 3");
        assert_eq!(reason(&[("codon_start", &["0"])]), "codon_start must be 1, 2 or 3");
        assert_eq!(reason(&[("transl_table", &["7"])]), "transl_table is not a supported NCBI genetic code");
        assert_eq!(reason(&[("bios_start_codon", &["always"])]), "the first-codon translation setting is invalid");
        assert_eq!(reason(&[("pseudo", &[])]), "the CDS carries translation exceptions or pseudogene metadata");
        assert_eq!(reason(&[("bios_translation_unavailable", &["TRUE"])]), "the saved coding metadata does not establish a verified translation");
        assert_eq!(
            reason(&[("bios_translation_unavailable", &["true"]), ("bios_translation_unavailable_reason", &["read through"])]),
            "read through"
        );
        assert_eq!(read_coding_settings(&notes(&[("codon_start", &["4"])])).phase, 3);
    }

    #[test]
    fn every_table_the_settings_accept_is_carried_whole() {
        assert_eq!(GENETIC_CODES.iter().map(|code| code.id).collect::<Vec<_>>(), GENETIC_CODE_IDS);
        for code in &GENETIC_CODES {
            assert_eq!((code.amino_acids.len(), code.start_stops.len()), (64, 64), "table {}", code.id);
        }
    }

    #[test]
    fn a_codon_is_read_in_either_case_and_only_when_it_is_three_iupac_bases() {
        let standard = read_coding_settings(&[]);
        let residue = |codon: &str| coding_residue(codon, &standard, false, false);
        assert_eq!((residue("uuu"), residue("gcn"), residue("tar"), residue("AT-"), residue("ATGA"), residue("AT")), ('F', 'A', '*', 'X', 'X', 'X'));
        assert!(is_start_codon("aUg", 1) && !is_start_codon("NTG", 1) && !is_start_codon("ATG", 7));
        assert!(is_stop_codon("tar", 1) && !is_stop_codon("TGA", 2) && !is_stop_codon("", 1));
        // What the web app's uppercasing makes of letters that are not ASCII: "ſ" is an S, and "ß" is two of which the first is read.
        assert_eq!((residue("TCſ"), residue("TCß"), residue("ßAA"), residue("AT🧬")), ('S', 'S', 'X', 'X'));
    }

    #[test]
    fn a_stop_before_the_last_residue_is_internal() {
        let t = translate_coding("ATGTAAGCTTAA", read_coding_settings(&[]));
        assert_eq!((t.protein.as_str(), t.internal_stops, t.valid_stop, t.verified, t.complete), ("M*A*", vec![2], true, true, false));
    }

    #[test]
    fn text_that_is_not_dna_falls_into_the_codons_the_web_app_gives_it() {
        let spaced = translate_coding("atg gcu\u{feff}taa", read_coding_settings(&[]));
        assert_eq!((spaced.coding_bases.as_str(), spaced.protein.as_str(), spaced.complete), ("ATGGCTTAA", "MA*", true));
        // A character outside the basic plane is two units long, so the codon after it starts one base late.
        let wide = translate_coding("ATG🧬GCTTAA", read_coding_settings(&[]));
        assert_eq!((wide.protein.as_str(), wide.trailing_bases, wide.verified), ("MXL", 2, false));
        let cut = translate_coding("🧬ATGTAA", read_coding_settings(&notes(&[("codon_start", &["2"])])));
        assert_eq!((cut.coding_bases.as_str(), cut.protein.as_str(), cut.trailing_bases), ("\u{fffd}ATGTAA", "XV", 1));
    }

    #[test]
    fn a_partial_note_marks_both_ends() {
        let s = read_coding_settings(&notes(&[("note", &["made by hand", "Bios-Partial: 5'"])]));
        assert_eq!((s.partial_start, s.partial_end), (true, true));
    }
}
