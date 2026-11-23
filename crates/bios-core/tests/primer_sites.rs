//! Primer sites: Tm scope, the second lap, the bounded inventory, refused settings and an
//! independent oracle, ported from the web app's `primerSites.test.ts` in its order and with its
//! numbers.

use bios_core::primer::{PrimerSearchError, PrimerSearchOptions, PrimerSite, find_primer_sites, search_primer_sites};
use bios_core::seq::revcomp;

const BINDING: &str = "ATGCGTACGTTGACCGATCA";

fn template() -> String {
    ["GGGGGGGGGG", BINDING, "TTTTTTTTTT"].concat()
}

fn sites(target: &str, primer: &str, options: PrimerSearchOptions) -> Vec<PrimerSite> {
    find_primer_sites(target, primer, options).expect("the settings are valid")
}

#[test]
fn mismatched_and_ambiguous_candidates_never_borrow_a_perfect_duplex_tm() {
    let options = PrimerSearchOptions { circular: false, min_tm: 1000.0, ..Default::default() };
    for base in ["A", "N"] {
        let oligo = [&BINDING[..5], base, &BINDING[6..]].concat();
        let found = sites(&template(), &oligo, options);
        let site = found.iter().find(|s| s.start == 10 && s.end == 29 && s.forward).expect("the site");
        assert_eq!(site.tm, None);
        assert_eq!(site.mismatch_positions, [5]);
    }
    assert_eq!(sites(&template(), BINDING, options).len(), 0, "exact duplexes obey the temperature filter");
}

#[test]
fn a_circular_site_cannot_reuse_a_template_base_on_a_second_lap() {
    let sequence = "ACGTAC";
    let oligo = sequence.repeat(5);
    assert_eq!(sites(sequence, &oligo, PrimerSearchOptions { circular: true, min_anchor3: 7, min_tm: -1000.0, ..Default::default() }), []);
    let found = sites(sequence, &oligo, PrimerSearchOptions { circular: true, min_anchor3: 3, min_tm: -1000.0, ..Default::default() });
    assert!(!found.is_empty());
    for site in &found {
        let length = if site.end < site.start { sequence.len() - site.start + site.end + 1 } else { site.end - site.start + 1 };
        assert_eq!(site.anneal_length, length);
        assert!(length <= sequence.len());
        assert_eq!(site.tail_length + length, oligo.len());
    }
}

#[test]
fn bounded_inventory_reports_the_real_count_and_first_genomic_sites_on_both_strands() {
    let sequence = [BINDING, "TT", &revcomp(BINDING), "AA"].concat().repeat(30);
    let options = PrimerSearchOptions { circular: true, ..Default::default() };
    let all = sites(&sequence, BINDING, options);
    for limit in [1, 3, 50, 1000] {
        let result = search_primer_sites(&sequence, BINDING, options, limit).expect("the settings are valid");
        assert_eq!(result.total, all.len());
        assert_eq!(result.truncated, all.len() > limit);
        assert_eq!(result.sites, all[..limit.min(all.len())]);
    }
}

#[test]
fn invalid_search_settings_fail_explicitly_even_on_an_empty_template() {
    // The web app also refuses an anchor of 2.5 or NaN, a mismatch limit of -1 or 1.5, an
    // isolated-mismatch option of "true" and a missing topology. None of those can be written
    // here: the fields are whole numbers that cannot be negative, and booleans.
    for options in [PrimerSearchOptions { min_anchor3: 0, ..Default::default() }, PrimerSearchOptions { min_tm: f64::INFINITY, ..Default::default() }]
    {
        let refused = find_primer_sites("", "", PrimerSearchOptions { circular: false, ..options });
        assert_eq!(refused, Err(PrimerSearchError::Settings));
        assert!(refused.is_err_and(|e| e.to_string().contains("Primer search needs")));
    }
    // And a site limit of -1 or 1.5, likewise.
    for limit in [0, 1001] {
        let refused = search_primer_sites(&template(), BINDING, PrimerSearchOptions { circular: false, ..Default::default() }, limit);
        assert_eq!(refused, Err(PrimerSearchError::SiteLimit));
        assert!(refused.is_err_and(|e| e.to_string().contains("Site limit")));
    }
}

/// What the oracle and the search are compared on: start, end, forward, anneal length, tail
/// length and mismatch positions.
type Geometry = (usize, usize, bool, usize, usize, Vec<usize>);

#[test]
fn candidate_geometry_and_mismatch_offsets_match_an_independent_exhaustive_footprint_oracle() {
    let mut cases = 0;
    let primers = ["ACGTAC", "GACGTA", "TGCATG", "AAACCC", "CGTTCG", "ACGACGTAC"];
    for sequence in ["ACGTACGTAC", "GGGAAACCCG", "CGTTCGATGC", "ACGTNACGTA"] {
        for primer in primers {
            for circular in [false, true] {
                for isolated in [false, true] {
                    let mut expected: Vec<Geometry> = Vec::new();
                    let (n, m, anchor) = (sequence.len() as i64, primer.len() as i64, 3i64);
                    for forward in [true, false] {
                        for terminal in 0..n {
                            let mut best: Option<Geometry> = None;
                            let mut best_score = 0;
                            // Enumerate complete physical footprints instead of walking a query extension.
                            for length in anchor..=n.min(m) {
                                let lo = if forward { terminal - length + 1 } else { terminal };
                                let hi = if forward { terminal } else { terminal + length - 1 };
                                if !circular && (lo < 0 || hi >= n) {
                                    continue;
                                }
                                let plus: String = (0..length).map(|i| char::from(sequence.as_bytes()[((lo + i + n) % n) as usize])).collect();
                                let target = if forward { plus } else { revcomp(&plus) };
                                let query = &primer[(m - length) as usize..];
                                let mismatch: Vec<i64> = query
                                    .bytes()
                                    .zip(target.bytes())
                                    .enumerate()
                                    .filter(|&(_, (b, t))| b != t || !b"ACGT".contains(&b))
                                    .map(|(i, _)| i as i64)
                                    .collect();
                                if mismatch.len() > 3 || mismatch.contains(&(length - 1)) || mismatch.contains(&0) {
                                    continue;
                                }
                                let anchor_errors: Vec<i64> = mismatch.iter().copied().filter(|&i| i >= length - anchor).collect();
                                if anchor_errors.len() > usize::from(isolated)
                                    || anchor_errors.iter().any(|&i| i == 0 || mismatch.contains(&(i - 1)) || mismatch.contains(&(i + 1)))
                                {
                                    continue;
                                }
                                let score = length - 3 * mismatch.len() as i64;
                                if score > best_score {
                                    best_score = score;
                                    best = Some((
                                        ((lo + n) % n) as usize,
                                        (hi % n) as usize,
                                        forward,
                                        length as usize,
                                        (m - length) as usize,
                                        mismatch.iter().map(|&i| (i + m - length) as usize).collect(),
                                    ));
                                }
                            }
                            expected.extend(best);
                        }
                    }
                    expected.sort_by(|a, b| a.0.cmp(&b.0).then(b.2.cmp(&a.2)).then(a.1.cmp(&b.1)));
                    let options =
                        PrimerSearchOptions { circular, min_anchor3: 3, allow_isolated_mismatch: isolated, min_tm: -1000.0, ..Default::default() };
                    let actual: Vec<Geometry> = sites(sequence, primer, options)
                        .into_iter()
                        .map(|s| (s.start, s.end, s.forward, s.anneal_length, s.tail_length, s.mismatch_positions))
                        .collect();
                    assert_eq!(actual, expected, "sequence {sequence} primer {primer} circular {circular} isolated {isolated}");
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 96);
}
