//! Binders, as the web app's `editor/binders.test.ts` checks them, in its order and with its
//! numbers. The failure mode is not a crash, it is a drawing that lies: a primer drawn over a span
//! it does not anneal to, or an off-target drawn the same as the site you meant. (The web's "binders
//! leave no ink" tests check its OVE projection object, which Bios does not have: a binder here is
//! never part of the construct at all.)

use bios_core::binders::{Binder, annealed_bases, binder_from_bases, binder_from_selection, binders_to_sites, site_label};
use bios_core::seq::revcomp;

// 120bp, homopolymer flanks so nothing binds by accident, non-palindromic anneal.
const SEQ: &str =
    concat!("GGGGGGGGGGGGGGGGGGGG", "ATGCGTACGTTGACCGATCAGGCATTACGGCTATTCAGA", "TTTTTTTTTTTTTTTTTTTT", "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");
const TAIL: &str = "GCTAGCGGATCCGCTAGCGGATCC"; // 24nt of non-homology: a cut site + filler

fn anneal() -> &'static str {
    &SEQ[20..42] // 22nt, starts at 20
}

fn binder(bases: &str) -> Binder {
    Binder { id: "binder-1".into(), name: "p1".into(), bases: bases.into() }
}

fn tailed() -> String {
    format!("{TAIL}{}", anneal())
}

// A tailed primer survives the projection.

#[test]
fn a_gibson_style_tailed_primer_lands_and_lands_where_it_anneals() {
    let sites = binders_to_sites(&[binder(&tailed())], SEQ, false);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].site.start, 20);
    assert_eq!(sites[0].site.tail_length, 24);
    assert_eq!(sites[0].site.anneal_length, 22);
    assert!(sites[0].on_target);
}

#[test]
fn the_drawing_gets_the_annealed_bases_not_the_whole_oligo() {
    let sites = binders_to_sites(&[binder(&tailed())], SEQ, false);
    let s = &sites[0];
    let span = s.site.end - s.site.start + 1;
    assert_eq!(annealed_bases(s).len(), span);
    assert_eq!(annealed_bases(s), anneal());
}

#[test]
fn the_tail_is_named_because_the_map_cannot_draw_it() {
    let sites = binders_to_sites(&[binder(&tailed())], SEQ, false);
    assert!(site_label(&sites[0]).contains("24nt 5' tail"));
}

#[test]
fn an_untailed_primer_is_named_plainly() {
    let sites = binders_to_sites(&[binder(anneal())], SEQ, false);
    assert_eq!(site_label(&sites[0]), "p1");
}

// Tm belongs to the site, not the oligo.

#[test]
fn a_tailed_primers_site_tm_is_the_anneals() {
    let tailed = binders_to_sites(&[binder(&tailed())], SEQ, false);
    let plain = binders_to_sites(&[binder(anneal())], SEQ, false);
    assert_eq!(tailed[0].site.tm, plain[0].site.tm);
}

// A partial landing is not a tail, and must not unseat the real site.

const ONTARGET: &str = "ACTATGCGGCATCAGAGCA"; // 19nt

fn seq2() -> String {
    let chance = &ONTARGET[ONTARGET.len() - 14..]; // its 3' end, dropped somewhere else verbatim
    format!("GGGGGGGGGGGGGGGGGGGG{}TTTTTTTTTTTTTTTTTTTTTTTTTTTTTT{}CCCCCCCCCCCCCCCCCCCC", revcomp(ONTARGET), revcomp(chance))
}

#[test]
fn both_sites_are_found_the_chance_one_is_real_priming_risk() {
    let mut sites = binders_to_sites(&[binder(ONTARGET)], &seq2(), false);
    assert_eq!(sites.len(), 2);
    sites.sort_by_key(|s| std::cmp::Reverse(s.site.anneal_length));
    assert_eq!(sites[0].site.anneal_length, 19);
    assert_eq!(sites[1].site.anneal_length, 14);
    assert_eq!(sites[0].site.mismatches, 0);
    assert_eq!(sites[1].site.mismatches, 0);
}

#[test]
fn the_full_length_site_is_still_the_on_target_and_the_short_one_partial() {
    let sites = binders_to_sites(&[binder(ONTARGET)], &seq2(), false);
    let on = sites.iter().find(|s| s.site.anneal_length == 19).unwrap();
    let chance = sites.iter().find(|s| s.site.anneal_length == 14).unwrap();
    assert!(on.on_target, "the real site must not be demoted by a chance landing");
    assert!(!on.partial);
    assert!(!chance.on_target);
    assert!(chance.partial);
}

#[test]
fn the_labels_tell_them_apart() {
    let labels: Vec<String> = binders_to_sites(&[binder(ONTARGET)], &seq2(), false).iter().map(site_label).collect();
    assert!(labels.iter().any(|l| l == "p1"), "the on-target should be named plainly, got {labels:?}");
    let chance = labels.iter().find(|l| *l != "p1").unwrap();
    assert!(chance.contains("only 14 of 19 nt anneal"));
    assert!(!chance.contains("tail"), "a chance landing is not a tail");
}

#[test]
fn a_designed_tail_is_still_called_a_tail_not_a_partial() {
    let label = site_label(&binders_to_sites(&[binder(&tailed())], SEQ, false)[0]);
    assert!(label.contains("24nt 5' tail"));
    assert!(!label.contains("only"));
}

// Off-target is ambient, not asked for.

#[test]
fn an_oligo_that_binds_twice_has_no_on_target() {
    let twice = format!("{SEQ}{}", anneal());
    let sites = binders_to_sites(&[binder(anneal())], &twice, false);
    assert_eq!(sites.len(), 2);
    assert!(sites.iter().all(|s| !s.on_target));
    assert!(sites.iter().all(|s| site_label(s).contains("also binds here")));
}

#[test]
fn a_reverse_primer_is_projected_onto_the_strand_it_anneals_to() {
    let sites = binders_to_sites(&[binder(&revcomp(anneal()))], SEQ, false);
    assert!(!sites[0].site.forward);
    assert_eq!(sites[0].site.start, 20);
    assert_eq!(sites[0].site.end, 41);
}

// The two doors (`binderFromSelection`, `binderFromBases`).

#[test]
fn a_binder_from_a_selection_is_what_anneals_there() {
    let b = binder_from_selection(SEQ, 20, 41, 3).unwrap();
    assert_eq!(b.bases, revcomp(anneal()));
    assert_eq!((b.id.as_str(), b.name.as_str()), ("binder-3", "binder 3"));
    assert_eq!(binder_from_selection(SEQ, 20, 500, 4), None);
    assert_eq!(binder_from_selection(SEQ, 30, 20, 4), None);
}

#[test]
fn a_binder_from_bases_keeps_only_the_letters() {
    let b = binder_from_bases(" acg t-12 ", Some("  "), 7).unwrap();
    assert_eq!((b.bases.as_str(), b.name.as_str()), ("ACGT", "binder 7"));
    assert_eq!(binder_from_bases("12 -", None, 8), None);
    assert_eq!(binder_from_bases("aa", Some("mine"), 9).unwrap().name, "mine");
}
