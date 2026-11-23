//! The Enzymes panel's list, ported from the web app's `editor/enzymeList.test.ts` in its order and
//! with its numbers.

use bios_core::enzyme_list::{EnzymeFilter, EnzymeHit, EnzymeRow, GroupKey, enzyme_rows, group_rows, match_enzyme, name_order};
use std::collections::HashSet;

// One EcoRI site (G^AATTC, 5′ overhang), one SmaI site (CCC^GGG, blunt) and one BsaI site (GGTCTC,
// Type IIS). Everything else in the table either cuts it incidentally or not at all.
const SEQ: &str = "AAAAGAATTCAAAACCCGGGAAAAGGTCTCAAAAAAAAAA";

fn rows() -> Vec<EnzymeRow> {
    enzyme_rows(SEQ, false)
}

fn row<'a>(rows: &'a [EnzymeRow], name: &str) -> &'a EnzymeRow {
    rows.iter().find(|r| r.name == name).unwrap()
}

fn listed(rows: &[EnzymeRow], query: &str, filter: EnzymeFilter, shown: &[&str]) -> HashSet<&'static str> {
    let shown: Vec<String> = shown.iter().map(|s| s.to_string()).collect();
    group_rows(rows, query, filter, &shown).iter().flat_map(|g| g.rows.iter().map(|(i, _)| rows[*i].name)).collect()
}

#[test]
fn every_enzyme_in_the_table_gets_a_row_alphabetically() {
    let rows = rows();
    assert!(rows.len() > 500, "expected the whole table, got {}", rows.len());
    let names: Vec<&str> = rows.iter().map(|r| r.name).collect();
    let mut sorted = names.clone();
    sorted.sort_by(|a, b| name_order(a, b));
    assert_eq!(names, sorted);
}

#[test]
fn a_row_carries_its_sites_on_this_molecule_and_how_it_cuts() {
    let rows = rows();
    let eco = row(&rows, "EcoRI");
    assert_eq!(eco.sites.len(), 1);
    assert_eq!(eco.sites[0].recognition_start, 4);
    assert_eq!(eco.caret_site.as_deref(), Some("G^AATTC"));
    assert_eq!(eco.overhang_text, "5′ AATT");
    assert!(!eco.blunt);
    assert!(!eco.type2s);

    assert!(row(&rows, "SmaI").blunt);
    let bsa = row(&rows, "BsaI");
    assert!(bsa.type2s);
    assert_eq!(bsa.sites.len(), 1);
    // Type IIS cuts outside its site: no caret to place, and no site-intrinsic overhang text.
    assert_eq!(bsa.caret_site, None);
    assert_eq!(bsa.overhang_text, "");
}

#[test]
fn an_enzyme_that_does_not_cut_still_gets_a_row_with_no_sites() {
    assert_eq!(row(&rows(), "NotI").sites.len(), 0);
}

#[test]
fn search_matches_the_name_an_isoschizomer_or_the_bases() {
    let rows = rows();
    let bsa = row(&rows, "BsaI");
    assert_eq!(match_enzyme(bsa, "bsa"), Some(EnzymeHit::default()));
    assert_eq!(match_enzyme(bsa, "Eco31I"), Some(EnzymeHit { via: Some("Eco31I"), by_site: false }));
    let site = Some(EnzymeHit { via: None, by_site: true });
    assert_eq!(match_enzyme(row(&rows, "EcoRI"), "gaattc"), site);
    // A site copied out of a protocol keeps its caret and its spaces.
    assert_eq!(match_enzyme(row(&rows, "EcoRI"), "g^aa ttc"), site);
    assert_eq!(match_enzyme(row(&rows, "EcoRI"), "zzz"), None);
    assert_eq!(match_enzyme(row(&rows, "EcoRI"), ""), Some(EnzymeHit::default()), "an empty query matches everything");
}

#[test]
fn rows_group_by_how_often_they_cut() {
    let rows = rows();
    let groups = group_rows(&rows, "", EnzymeFilter::All, &[]);
    let keys: Vec<&str> = groups.iter().map(|g| g.key.key()).collect();
    assert_eq!(keys, ["shown", "once", "twice", "few", "none"]);
    let names = |k: GroupKey| -> Vec<&str> { groups.iter().find(|g| g.key == k).unwrap().rows.iter().map(|(i, _)| rows[*i].name).collect() };
    let once = names(GroupKey::Once);
    assert!(once.contains(&"EcoRI"), "EcoRI cuts this once");
    assert!(once.contains(&"SmaI"));
    assert!(names(GroupKey::None).contains(&"NotI"));
    // Every row lands in exactly one group.
    let total: usize = groups.iter().map(|g| g.rows.len()).sum();
    assert_eq!(total, rows.len());
}

#[test]
fn the_scope_narrows_to_a_class_of_enzyme() {
    let rows = rows();
    assert!(listed(&rows, "", EnzymeFilter::TypeIis, &[]).contains("BsaI"));
    assert!(!listed(&rows, "", EnzymeFilter::TypeIis, &[]).contains("EcoRI"));
    assert!(listed(&rows, "", EnzymeFilter::Blunt, &[]).contains("SmaI"));
    assert!(!listed(&rows, "", EnzymeFilter::Blunt, &[]).contains("EcoRI"), "EcoRI leaves a 5′ overhang");
}

#[test]
fn the_list_rests_on_the_common_enzymes_not_the_whole_table() {
    let rows = rows();
    let common = listed(&rows, "", EnzymeFilter::default(), &[]);
    assert!(common.contains("EcoRI"), "a workhorse is there");
    assert!(common.len() < rows.len() / 4, "the common set is small ({} of {})", common.len(), rows.len());
    assert_eq!(listed(&rows, "", EnzymeFilter::All, &[]).len(), rows.len());
}

#[test]
fn a_search_looks_at_the_whole_table_whatever_the_list_is_resting_on() {
    let rows = rows();
    // Eco252I is an isoschizomer group outside the common set: searching it must find it rather
    // than answer "nothing", which would be a lie about the table.
    assert!(!listed(&rows, "", EnzymeFilter::Common, &[]).contains("Eco252I"), "it is not in the resting list");
    assert!(listed(&rows, "Eco252I", EnzymeFilter::Common, &[]).contains("Eco252I"), "but searching finds it");
    // A class chip still applies to a search: it says what kind of answer you asked for.
    assert!(!listed(&rows, "Eco252I", EnzymeFilter::TypeIis, &[]).contains("Eco252I"));
}

#[test]
fn what_is_on_the_map_comes_first_whatever_the_scope_says() {
    let rows = rows();
    let groups = group_rows(&rows, "", EnzymeFilter::Blunt, &["ecori".to_string()]);
    let shown: Vec<&str> = groups.iter().find(|g| g.key == GroupKey::Shown).unwrap().rows.iter().map(|(i, _)| rows[*i].name).collect();
    assert_eq!(shown, ["EcoRI"], "a chosen enzyme outranks a resting scope it does not belong to");
    assert!(!groups.iter().filter(|g| g.key != GroupKey::Shown).flat_map(|g| &g.rows).any(|(i, _)| rows[*i].name == "EcoRI"));
}

#[test]
fn but_a_query_narrows_what_is_on_the_map_too() {
    // Searching BsaI and being answered with EcoRI, because EcoRI happens to be on the map, reads
    // as the list being broken. The map is where "still on the map" is said.
    let rows = rows();
    let groups = group_rows(&rows, "BsaI", EnzymeFilter::Common, &["EcoRI".to_string()]);
    assert_eq!(groups.iter().find(|g| g.key == GroupKey::Shown).unwrap().rows.len(), 0);
    assert!(groups.iter().flat_map(|g| &g.rows).any(|(i, _)| rows[*i].name == "BsaI"));
}

#[test]
fn an_empty_sequence_still_lists_the_table_cutting_nothing() {
    let empty = enzyme_rows("", true);
    assert_eq!(empty.len(), rows().len());
    assert!(empty.iter().all(|r| r.sites.is_empty()));
}
