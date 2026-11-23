//! Restriction-enzyme tests, ported from the web app's `enzymes.test.ts` in its order and with its
//! numbers.
//!
//! Coordinates are 0-based; `top_cut`/`bottom_cut` are gap coordinates (cut before that base).
//! These synthetic fixtures independently pin cut coordinates, overhangs and unavailable metadata.

use bios_core::enzymes::{COMMON_ENZYMES, CleavageStatus, CutSite, Overhang, OverhangKind, enzymes, find_cut_sites, get_enzyme};
use bios_core::model::Strand;

fn overhang(kind: OverhangKind, length: usize, seq: &str) -> Overhang {
    Overhang { kind, length, seq: seq.to_string() }
}

fn blunt() -> Overhang {
    overhang(OverhangKind::Blunt, 0, "")
}

const NO_CUTS: Vec<CutSite> = Vec::new();

#[test]
fn ecori_leaves_a_4_base_5_prime_aatt_overhang() {
    let seq = ["AAAA", "GAATTC", "AAAA"].concat(); // site at index 4
    let sites = find_cut_sites(&seq, &["EcoRI"], false);
    assert_eq!(sites.len(), 1); // palindromic: scanned once, not double-counted
    let s = &sites[0];
    assert_eq!(s.recognition_start, 4);
    assert_eq!(s.strand, Strand::Forward);
    assert_eq!(s.top_cut, 5); // G^AATTC
    assert_eq!(s.bottom_cut, 9); // CTTAA^G
    assert_eq!(s.overhang, overhang(OverhangKind::FivePrime, 4, "AATT"));
}

#[test]
fn smai_is_a_blunt_cutter() {
    let seq = ["TTTT", "CCCGGG", "TTTT"].concat();
    let sites = find_cut_sites(&seq, &["SmaI"], false);
    assert_eq!(sites.len(), 1);
    let s = &sites[0];
    assert_eq!(s.top_cut, 7); // CCC^GGG
    assert_eq!(s.bottom_cut, 7);
    assert_eq!(s.overhang, blunt());
}

#[test]
fn bsai_type_iis_cuts_downstream_outside_its_recognition_site() {
    let enz = get_enzyme("BsaI").unwrap();
    assert!(enz.type2s);
    let seq = ["AAAAAA", "GGTCTC", "AAAAAAAAAAAA"].concat(); // site at index 6, room downstream
    let sites = find_cut_sites(&seq, &["BsaI"], false);
    assert_eq!(sites.len(), 1);
    let s = &sites[0];
    assert_eq!(s.recognition_start, 6);
    assert_eq!(s.strand, Strand::Forward);
    // GGTCTC(1/5): the cut is past the end of the recognition site.
    assert!(s.top_cut > s.recognition_start + enz.length, "top cut is outside the site");
    assert_eq!(s.top_cut, 13);
    assert_eq!(s.bottom_cut, 17);
    assert_eq!(s.overhang.kind, OverhangKind::FivePrime);
    assert_eq!(s.overhang.length, 4);
}

#[test]
fn bsai_on_the_reverse_strand_cuts_upstream_strand_minus_1() {
    // GAGACC is the reverse complement of GGTCTC: a minus-strand BsaI site.
    let seq = ["AAAAAAAAAAAA", "GAGACC", "AAAAAA"].concat(); // site at index 12
    let sites = find_cut_sites(&seq, &["BsaI"], false);
    assert_eq!(sites.len(), 1);
    let s = &sites[0];
    assert_eq!(s.strand, Strand::Reverse);
    // On the minus strand the cut lands to the LEFT of the recognition site in plus coords.
    assert!(s.top_cut.min(s.bottom_cut) < s.recognition_start, "cut is outside the site");
    assert_eq!(s.overhang.length, 4);
}

#[test]
fn an_origin_spanning_site_on_a_circular_target_is_found_exactly_once() {
    // GAATTC laid across the origin: G A A T at the tail, T C at the head.
    let seq = ["TC", "AAAAAAAAAA", "GAAT"].concat(); // N=16, site starts at index 12
    assert_eq!(seq.len(), 16);
    let circ = find_cut_sites(&seq, &["EcoRI"], true);
    assert_eq!(circ.len(), 1, "not duplicated by the circular double-scan");
    let s = &circ[0];
    assert_eq!(s.recognition_start, 12);
    assert_eq!(s.top_cut, 13);
    assert_eq!(s.bottom_cut, 1); // 12+5 = 17 ≡ 1 (mod 16)
    assert_eq!(s.overhang, overhang(OverhangKind::FivePrime, 4, "AATT"));
}

#[test]
fn linear_never_wraps_the_origin() {
    let seq = ["TC", "AAAAAAAAAA", "GAAT"].concat(); // same fixture, now linear
    let lin = find_cut_sites(&seq, &["EcoRI"], false);
    assert_eq!(lin.len(), 0, "the origin-spanning site does not exist on a linear molecule");
}

#[test]
fn get_enzyme_resolves_canonical_names_and_aliases_case_insensitively() {
    assert_eq!(get_enzyme("EcoRI").unwrap().site, "GAATTC");
    assert_eq!(get_enzyme("ecori").unwrap().name, "EcoRI");
    assert_eq!(get_enzyme("Eco31I").unwrap().name, "BsaI"); // isoschizomer alias
    assert_eq!(get_enzyme("  bsai  ").unwrap().name, "BsaI"); // trimmed
    assert_eq!(get_enzyme("NotARealEnzyme"), None);
}

#[test]
fn find_cut_sites_skips_unknown_enzyme_names_without_throwing() {
    let seq = ["AAAA", "GAATTC", "AAAA"].concat();
    let sites = find_cut_sites(&seq, &["EcoRI", "Nonexistent"], false);
    assert_eq!(sites.len(), 1);
}

#[test]
fn the_enzyme_table_and_common_set_are_populated_and_consistent() {
    assert!(enzymes().len() > 400, "full REBASE-style table is vendored");
    assert!(COMMON_ENZYMES.len() > 50, "a workhorse set is exposed");
    for name in COMMON_ENZYMES.iter() {
        assert!(get_enzyme(name).is_some(), "COMMON_ENZYMES entry {name} resolves");
    }
}

#[test]
fn legacy_recognition_metadata_cannot_manufacture_double_strand_cuts() {
    for (name, seq, status) in [
        ("M.SssI", "AACGTT", CleavageStatus::Methyltransferase),
        ("M.CviPI", "AAGCTT", CleavageStatus::Methyltransferase),
        ("BpuJI", "AAACCCGTTTT", CleavageStatus::Unverified),
        ("PspFI", "AAACCCAGCTTT", CleavageStatus::Unverified),
        // Methyltransferase aliases also occur inside mixed legacy recognition groups.
        ("M.EcoKDam", "AAGATCTT", CleavageStatus::Unverified),
    ] {
        let e = get_enzyme(name).unwrap();
        assert_eq!(e.cleavage_status(), status, "{name}");
        assert_eq!(e.top_snip_offset(), None, "{name}");
        assert_eq!(e.bottom_snip_offset(), None, "{name}");
        assert_eq!(find_cut_sites(seq, &[name], false), NO_CUTS, "{name}");
        assert_eq!(find_cut_sites(seq, &[name], true), NO_CUTS, "{name} circular");
    }
}

#[test]
fn mluci_uses_the_independently_documented_0_4_cut_without_validating_tseci() {
    let e = get_enzyme("MluCI").unwrap();
    assert_eq!(e.cleavage_status(), CleavageStatus::Supported);
    assert!(e.cleavage_source.unwrap_or("").contains("neb.com"));
    assert_eq!(e.aliases, ["MluCI"]);
    let sites = find_cut_sites("CCAATTGG", &["MluCI"], false);
    let site = sites.first();
    assert_eq!(site.map(|s| s.recognition_start), Some(2));
    assert_eq!(site.map(|s| s.top_cut), Some(2));
    assert_eq!(site.map(|s| s.bottom_cut), Some(6));
    assert_eq!(site.map(|s| &s.overhang), Some(&overhang(OverhangKind::FivePrime, 4, "AATT")));
    assert_eq!(get_enzyme("TseCI").map(|e| e.cleavage_status()), Some(CleavageStatus::Unverified));
    assert_eq!(get_enzyme("TseCI").map(|e| e.name), Some("TseCI"));
    assert_eq!(find_cut_sites("CCAATTGG", &["TseCI"], false), NO_CUTS);
}

#[test]
fn known_zero_offsets_and_genuine_blunt_cuts_remain_usable() {
    let sites = find_cut_sites("CCGATCGG", &["MboI"], false);
    let mbo = sites.first();
    assert_eq!(mbo.map(|s| s.top_cut), Some(2));
    assert_eq!(mbo.map(|s| s.bottom_cut), Some(6));
    assert_eq!(mbo.map(|s| &s.overhang), Some(&overhang(OverhangKind::FivePrime, 4, "GATC")));
    let sites = find_cut_sites("CCGATATCGG", &["EcoRV"], false);
    let eco = sites.first();
    assert_eq!(eco.map(|s| s.top_cut), Some(5));
    assert_eq!(eco.map(|s| s.bottom_cut), Some(5));
    assert_eq!(eco.map(|s| &s.overhang), Some(&blunt()));
}

#[test]
fn all_methyltransferase_aliases_and_unverified_entries_have_no_cut_coordinates() {
    let (mut supported, mut unverified, mut methyltransferase) = (0, 0, 0);
    for e in enzymes() {
        match e.cleavage_status() {
            CleavageStatus::Supported => supported += 1,
            CleavageStatus::Unverified => unverified += 1,
            CleavageStatus::Methyltransferase => methyltransferase += 1,
        }
        if e.cleavage_status() != CleavageStatus::Supported {
            assert_eq!(e.top_snip_offset(), None, "{}", e.name);
            assert_eq!(e.bottom_snip_offset(), None, "{}", e.name);
        }
        for alias in e.aliases.iter().filter(|a| a.starts_with("M.")) {
            assert_ne!(get_enzyme(alias).map(|e| e.cleavage_status()), Some(CleavageStatus::Supported), "{alias}");
        }
    }
    assert_eq!((supported, unverified, methyltransferase), (319, 208, 6));
}
