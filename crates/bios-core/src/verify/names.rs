//! Which map a read file is probably about, from its name alone (`verify/matchNames.ts`), and what
//! a row is called when it is one of many (`siblingLabels`, `runLabel`).
//!
//! A GUESS, NOT AN ANSWER. The board draws what this says and the person corrects it with one
//! click. That is why the rules prefer to ABSTAIN: a name that fits two maps equally goes to the
//! tray, where it is visible, rather than into a row, where it would be a confident mistake.
//!
//! Both names are split into tokens on `_ - . + space`, lowercased. The sequencer's own suffixes are
//! dropped from the sample (`S12`, `L001`, `001`, `R1`, the well `A3`), and so is the plate id, a
//! token many samples share and no map mentions. What is left is compared to each map by tokens in
//! common over the LONGER of the two lists, and only a clear winner places the sample.

use std::collections::{BTreeMap, HashMap, HashSet};

/// Where one sample's name puts it.
#[derive(Clone, Debug, PartialEq)]
pub struct NameMatch {
    /// The map's id, or `None` when the name fits nothing or fits two maps equally.
    pub map_id: Option<String>,
    /// 0..1, the winning score.
    pub score: f64,
    /// Why there is no match: `"no fit"` or `"tie"`.
    pub why: Option<&'static str>,
}

fn is_sep(c: char) -> bool {
    matches!(c, '_' | '-' | '.' | '+') || c.is_whitespace()
}

fn strip_ext(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for ext in [".fastq.gz", ".fq.gz", ".fastq", ".fq", ".gbk", ".gb", ".dna", ".fasta", ".fa"] {
        if lower.ends_with(ext) {
            return name.get(..name.len() - ext.len()).unwrap_or(name);
        }
    }
    name
}

fn tokens_of(name: &str) -> Vec<String> {
    strip_ext(name).to_lowercase().split(is_sep).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// `^(s\d+|l\d{3}|00\d|r[12]|i[12]|[a-h](0?[1-9]|1[0-2]))$`, case-insensitive: what a sequencer
/// appends and no map name carries.
fn is_noise(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    let b = t.as_bytes();
    let digits = |s: &[u8]| !s.is_empty() && s.iter().all(u8::is_ascii_digit);
    match b {
        [b's', rest @ ..] if digits(rest) => true,
        [b'l', rest @ ..] if rest.len() == 3 && digits(rest) => true,
        [b'0', b'0', d] if d.is_ascii_digit() => true,
        [b'r' | b'i', b'1' | b'2'] => true,
        [b'a'..=b'h', rest @ ..] => matches!(rest, [b'1'..=b'9'] | [b'0', b'1'..=b'9'] | [b'1', b'0'..=b'2']),
        _ => false,
    }
}

/// The plate id: a token at least three samples share and no map mentions.
fn plate_tokens(samples: &[Vec<String>], maps: &[Vec<String>]) -> HashSet<String> {
    let in_maps: HashSet<&String> = maps.iter().flatten().collect();
    let mut count: HashMap<&String, usize> = HashMap::new();
    for toks in samples {
        for t in toks.iter().collect::<HashSet<_>>() {
            *count.entry(t).or_default() += 1;
        }
    }
    count.into_iter().filter(|(t, n)| *n >= 3 && !in_maps.contains(t) && !is_noise(t)).map(|(t, _)| t.clone()).collect()
}

/// The plate id these files share, as the sequencer wrote it, or `None`.
pub fn plate_of(file_names: &[String], map_names: &[String]) -> Option<String> {
    let samples: Vec<Vec<String>> = file_names.iter().map(|n| tokens_of(n)).collect();
    let maps: Vec<Vec<String>> = map_names.iter().map(|n| tokens_of(n)).collect();
    let found = plate_tokens(&samples, &maps);
    let mut count: BTreeMap<&String, usize> = BTreeMap::new();
    for toks in &samples {
        for t in toks.iter().collect::<HashSet<_>>() {
            if found.contains(t) {
                *count.entry(t).or_default() += 1;
            }
        }
    }
    let best = count.iter().max_by_key(|(_, n)| **n).map(|(t, _)| (*t).clone())?;
    Some(as_written(file_names, &best))
}

/// The token as the first file that carries it spells it.
fn as_written(names: &[String], token: &str) -> String {
    names.iter().find_map(|n| n.split(is_sep).find(|t| t.to_lowercase() == token).map(str::to_string)).unwrap_or_else(|| token.to_string())
}

/// The part of a sample's name that says what it is, without the sequencer's and the plate's.
pub fn family_of(sample: &str, plate: &HashSet<String>) -> Vec<String> {
    tokens_of(sample).into_iter().filter(|t| !is_noise(t) && !plate.contains(t)).collect()
}

/// Matches every sample to the map its name fits best, or to nothing. `maps` is `(id, name)`.
pub fn match_names(samples: &[String], maps: &[(String, String)]) -> HashMap<String, NameMatch> {
    let map_toks: Vec<(String, Vec<String>)> = maps
        .iter()
        .map(|(id, name)| {
            let mut seen = HashSet::new();
            (id.clone(), tokens_of(name).into_iter().filter(|t| seen.insert(t.clone())).collect())
        })
        .collect();
    let sample_toks: Vec<Vec<String>> = samples.iter().map(|s| tokens_of(s)).collect();
    let plate = plate_tokens(&sample_toks, &map_toks.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>());
    let mut out = HashMap::new();
    for (sample, toks) in samples.iter().zip(&sample_toks) {
        let mut seen = HashSet::new();
        let mine: Vec<&String> = toks.iter().filter(|t| !is_noise(t) && !plate.contains(*t) && seen.insert((*t).clone())).collect();
        if mine.is_empty() || map_toks.is_empty() {
            out.insert(sample.clone(), NameMatch { map_id: None, score: 0.0, why: Some("no fit") });
            continue;
        }
        let mut scored: Vec<(&String, f64)> = map_toks
            .iter()
            .map(|(id, t)| {
                let shared = t.iter().filter(|x| mine.contains(x)).count();
                (id, shared as f64 / t.len().max(mine.len()) as f64)
            })
            .collect();
        // Stable, best first, as the web's sort.
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let (best_id, best) = scored[0];
        let next = scored.get(1).map(|s| s.1);
        let found = if best <= 0.0 {
            NameMatch { map_id: None, score: 0.0, why: Some("no fit") }
        } else if next == Some(best) {
            NameMatch { map_id: None, score: best, why: Some("tie") }
        } else {
            NameMatch { map_id: Some(best_id.clone()), score: best, why: None }
        };
        out.insert(sample.clone(), found);
    }
    out
}

/// Why the samples in the tray are still there, in one word, or nothing: a token carried by the
/// unplaced samples, by NO placed sample, and named by no map. It abstains with nothing placed,
/// because there is no contrast to draw the word out of.
pub fn unplaced_hint(unplaced: &[String], placed: &[String], map_names: &[String]) -> Option<String> {
    if unplaced.is_empty() || placed.is_empty() {
        return None;
    }
    let in_maps: HashSet<String> = map_names.iter().flat_map(|n| tokens_of(n)).collect();
    let on_placed: HashSet<String> = placed.iter().flat_map(|n| tokens_of(n)).collect();
    let mut count: HashMap<String, usize> = HashMap::new();
    for name in unplaced {
        for t in tokens_of(name).into_iter().collect::<HashSet<_>>() {
            if is_noise(&t) || in_maps.contains(&t) || on_placed.contains(&t) {
                continue;
            }
            *count.entry(t).or_default() += 1;
        }
    }
    let best = count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.chars().count().cmp(&b.0.chars().count())).then(b.0.cmp(&a.0)))?.0;
    Some(as_written(unplaced, &best))
}

/// A run's label: its file name without `.fastq` / `.fq` / `.gz`.
pub fn run_label(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for ext in [".fastq.gz", ".fq.gz", ".fastq", ".fq"] {
        if lower.ends_with(ext) {
            return name.get(..name.len() - ext.len()).unwrap_or(name).to_string();
        }
    }
    name.to_string()
}

fn label_sep(c: char) -> bool {
    matches!(c, '_' | '-' | '.' | ' ')
}

/// What each row is called when it is one of many named by a machine: the shared head and tail
/// taken off at separators, the middle left (`Aso10_1_A11_S11_L001_001` beside its siblings reads
/// `A11_S11`). Whole labels when that would leave a name empty or two the same.
pub fn sibling_labels(names: &[String]) -> HashMap<String, String> {
    let stems: Vec<(String, Vec<char>)> = names.iter().map(|raw| (raw.clone(), run_label(raw).chars().collect())).collect();
    let full: HashMap<String, String> = stems.iter().map(|(raw, s)| (raw.clone(), s.iter().collect())).collect();
    if stems.len() < 2 {
        return full;
    }
    let texts: Vec<&[char]> = stems.iter().map(|(_, s)| s.as_slice()).collect();
    let first = texts[0];
    let mut head = first.len();
    for t in &texts {
        head = head.min(t.len());
        let mut i = 0;
        while i < head && first[i] == t[i] {
            i += 1;
        }
        head = i;
    }
    while head > 0 && !label_sep(first[head - 1]) {
        head -= 1;
    }
    let mut tail = first.len();
    for t in &texts {
        tail = tail.min(t.len());
        let mut i = 0;
        while i < tail && first[first.len() - 1 - i] == t[t.len() - 1 - i] {
            i += 1;
        }
        tail = i;
    }
    while tail > 0 && !label_sep(first[first.len() - tail]) {
        tail -= 1;
    }
    let tail = tail.min(first.len().saturating_sub(head));
    if head == 0 && tail == 0 {
        return full;
    }
    let mut cut = HashMap::new();
    let mut seen = HashSet::new();
    for (raw, stem) in &stems {
        let end = stem.len().saturating_sub(tail).max(head.min(stem.len()));
        let middle: String = stem.get(head.min(stem.len())..end).unwrap_or_default().iter().collect();
        let middle = middle.trim_matches(label_sep).to_string();
        if middle.is_empty() || !seen.insert(middle.clone()) {
            return full;
        }
        cut.insert(raw.clone(), middle);
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &[&str] = &[
        "Part3+4_Theoretically_optimized_with_EF1A_intron_FULL",
        "Part3+4_Theoretically_optimized_with_synthetic_intron_and_chopped_part_3_FULL",
        "Part3+4_Theoretically_optimized_with_synthetic_intron_and_downstream_part_2_FULL",
        "Part3+4_with_EF1A_in_between_FULL",
        "Part4_FULL",
        "Part4_no_intron_FULL",
        "Part4_with_EF1A_FULL",
        "VWXY_noMismatch__VWXY_EDTR_101bp",
        "qrs_olg7_sp",
        "QRS_OLG7",
        "qrs_olg8_sp",
        "QRS_OLG8",
        "QRS_noMismatch__QRS_101bp_guide_EDTR",
        "GENX2_TRANS_pUC19",
        "pUC19_full_uncut",
        "pUC19_ZZ-1.5_k7_p13_AGGAGAG",
        "pUC19_ZZ-1.5_k7_p20_AAAAGAC",
        "pUC19_ZZ+2.5_k7_p14_AAAGAAC",
        "pUC18_EDTR_101bp_guide_GENX2.dna",
    ];

    const SAMPLES: &[&str] = &[
        "Part3_4_with_EF1A_in_between_aZQ4101_H2_S182",
        "Part3_4_with_EF1A_in_between_aZQ4101_H3_S183",
        "Part3_4_with_EF1A_in_between_aZQ4101_H4_S184",
        "Part4_aZQ4101_G2_S170",
        "Part4_aZQ4101_G3_S171",
        "Part4_aZQ4101_G4_S172",
        "VWXY_noMismatch_aZQ4101_A2_S98",
        "VWXY_noMismatch_aZQ4101_B2_S110",
        "VWXY_noMismatch_aZQ4101_C2_S122",
        "QRS_OLG7_aZQ4101_D2_S134",
        "QRS_OLG7_SP_aZQ4101_E2_S146",
        "QRS_OLG8_aZQ4101_F2_S158",
        "F_ZZ_2_5_AAAGAAC_aZQ4101_A7_S103",
    ];

    fn maps() -> Vec<(String, String)> {
        MAPS.iter().map(|n| (format!("id:{n}"), n.to_string())).collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn pick(m: &HashMap<String, NameMatch>, s: &str) -> Option<String> {
        m.get(s).and_then(|x| x.map_id.clone()).map(|id| id.trim_start_matches("id:").to_string())
    }

    #[test]
    fn a_design_name_buried_in_a_sample_name_finds_its_map() {
        let got = match_names(&strings(SAMPLES), &maps());
        assert_eq!(pick(&got, "Part4_aZQ4101_G2_S170").as_deref(), Some("Part4_FULL"));
        assert_eq!(pick(&got, "QRS_OLG8_aZQ4101_F2_S158").as_deref(), Some("QRS_OLG8"));
    }

    #[test]
    fn the_near_misses_go_the_right_way() {
        let got = match_names(&strings(SAMPLES), &maps());
        assert_eq!(pick(&got, "Part4_aZQ4101_G3_S171").as_deref(), Some("Part4_FULL"));
        assert_eq!(pick(&got, "Part3_4_with_EF1A_in_between_aZQ4101_H2_S182").as_deref(), Some("Part3+4_with_EF1A_in_between_FULL"));
        assert_eq!(pick(&got, "QRS_OLG7_aZQ4101_D2_S134").as_deref(), Some("QRS_OLG7"));
        assert_eq!(pick(&got, "QRS_OLG7_SP_aZQ4101_E2_S146").as_deref(), Some("qrs_olg7_sp"));
    }

    #[test]
    fn a_map_whose_name_carries_more_than_the_samples_still_wins_when_nothing_closer_exists() {
        let got = match_names(&strings(SAMPLES), &maps());
        assert_eq!(pick(&got, "VWXY_noMismatch_aZQ4101_A2_S98").as_deref(), Some("VWXY_noMismatch__VWXY_EDTR_101bp"));
    }

    #[test]
    fn the_plate_id_is_found_and_ignored_not_matched() {
        let plate: HashSet<String> = ["azq4101".to_string()].into();
        assert_eq!(family_of("Part4_aZQ4101_G2_S170", &plate), vec!["part4"]);
        assert_eq!(family_of("QRS_OLG7_SP_aZQ4101_E2_S146", &plate), vec!["qrs", "olg7", "sp"]);
    }

    #[test]
    fn a_name_that_fits_nothing_goes_to_the_pile_and_says_so() {
        let got = match_names(&strings(SAMPLES), &maps());
        assert_eq!(got["F_ZZ_2_5_AAAGAAC_aZQ4101_A7_S103"].map_id.as_deref(), Some("id:pUC19_ZZ+2.5_k7_p14_AAAGAAC"));
        let none = match_names(&strings(&["plate_A01_S1"]), &maps());
        assert_eq!(none["plate_A01_S1"].map_id, None);
        assert_eq!(none["plate_A01_S1"].why, Some("no fit"));
    }

    #[test]
    fn a_tie_is_not_a_match() {
        let tied =
            match_names(&strings(&["genx2_X1_S1", "genx2_X2_S2", "genx2_X3_S3"]), &[("a".into(), "GENX2_A".into()), ("b".into(), "GENX2_B".into())]);
        assert_eq!(tied["genx2_X1_S1"].map_id, None);
        assert_eq!(tied["genx2_X1_S1"].why, Some("tie"));
    }

    #[test]
    fn no_maps_at_all_means_every_sample_is_in_the_pile_not_an_error() {
        assert_eq!(match_names(&strings(&["a_S1"]), &[])["a_S1"].map_id, None);
    }

    const PLACED: &[&str] = &["QRS_OLG7_aZQ4103_A1_S1", "QRS_OLG7_aZQ4103_B1_S13", "QRS_OLG9_aZQ4103_A3_S3", "QRS_OLG11_aZQ4103_A5_S5"];
    const UNPLACED: &[&str] =
        &["VWXY_LNK20v1_aZQ4103_A2_S2", "VWXY_LNK20v1_aZQ4103_B2_S14", "VWXY_LNK20v1_aZQ4103_C2_S26", "VWXY_LNK20v1_aZQ4103_D2_S38"];
    const HINT_MAPS: &[&str] = &["QRS_OLG7", "QRS_OLG9", "QRS_OLG11"];

    #[test]
    fn names_the_design_the_unplaced_samples_share() {
        let said = unplaced_hint(&strings(UNPLACED), &strings(PLACED), &strings(HINT_MAPS)).unwrap();
        assert!(said.to_lowercase() == "vwxy" || said.to_lowercase() == "lnk20v1", "{said}");
    }

    #[test]
    fn never_the_plate_id() {
        assert_ne!(unplaced_hint(&strings(UNPLACED), &strings(PLACED), &strings(HINT_MAPS)).map(|s| s.to_lowercase()).as_deref(), Some("azq4103"));
    }

    #[test]
    fn never_a_map_that_is_here() {
        let said = unplaced_hint(&strings(&["QRS_OLG7_aZQ4103_C1_S25"]), &strings(PLACED), &strings(HINT_MAPS));
        assert_ne!(said.map(|s| s.to_lowercase()).as_deref(), Some("qrs_olg7"));
    }

    #[test]
    fn it_says_the_word_as_the_file_writes_it() {
        let said = unplaced_hint(&strings(UNPLACED), &strings(PLACED), &strings(HINT_MAPS)).unwrap();
        assert_ne!(said, said.to_lowercase());
    }

    #[test]
    fn abstains_when_nothing_is_placed_or_unplaced_or_different() {
        assert_eq!(unplaced_hint(&strings(UNPLACED), &[], &strings(HINT_MAPS)), None);
        assert_eq!(unplaced_hint(&[], &strings(PLACED), &strings(HINT_MAPS)), None);
        assert_eq!(unplaced_hint(&strings(&["QRS_OLG7_aZQ4103_C1_S25"]), &strings(&["QRS_OLG7_aZQ4103_A1_S1"]), &strings(HINT_MAPS)), None);
    }

    #[test]
    fn the_plate_id_names_a_submission() {
        let files = strings(&["QX100_AAV_GENX2_EF1A_aZQ41009_A2_S2", "QX100_AAV_GENX2_PRO_aZQ41009_A7_S7", "Ct_1_aZQ41009_A9_S9"]);
        assert_eq!(plate_of(&files, &[]).as_deref(), Some("aZQ41009"));
    }

    #[test]
    fn sibling_labels_keep_the_well_and_drop_what_all_share() {
        let names = strings(&["Aso10_1_A11_S11_L001_001.fastq.gz", "Aso10_1_B11_S23_L001_001.fastq.gz"]);
        let got = sibling_labels(&names);
        assert_eq!(got[&names[0]], "A11_S11");
        assert_eq!(got[&names[1]], "B11_S23");
        let one = strings(&["x.fastq.gz"]);
        assert_eq!(sibling_labels(&one)[&one[0]], "x");
    }
}
