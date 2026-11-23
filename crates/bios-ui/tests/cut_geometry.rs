//! Parity with Bios `editor/rows/cutGeometry.test.ts`: one test here per test there, in the same
//! order, with the same numbers. The geometry is `bios_layout::cuts`; the tests are here because
//! they need the enzyme table in `bios-core`, and `bios-layout` depends on nothing.
//!
//! Two assertions of the first test wait for ports that do not exist yet: the Enzymes list
//! (`enzymeRows`, `groupRows`) and Find (`findMatches`). They are marked where they would stand.

use bios_core::enzymes::{CutSite, enzymes, find_cut_sites};
use bios_core::{Strand, genbank};
use bios_layout::cuts::{cuts_for_row, overhang_of, recognition_pieces};
use bios_layout::rows::Row;
use bios_ui::rows::{cut_of, default_cut_sites, dial_sites};

fn rotated(dna: &str, shift: usize) -> String {
    format!("{}{}", &dna[shift..], &dna[..shift])
}

/// one recognition site produces two enzyme cuts but one Find destination
#[test]
fn one_recognition_site_produces_two_enzyme_cuts_but_one_find_destination() {
    let seq = format!("{}CGAAAAAAATGC{}", "A".repeat(30), "A".repeat(30));
    let sites = find_cut_sites(&seq, &["BcgI"], false);
    assert_eq!(sites.len(), 2);
    // Not ported: `groupRows([row], { filter: "all" })` files BcgI under "twice", once.
    let all: Vec<&str> = enzymes().iter().map(|e| e.name).collect();
    let bcgi = |limit: usize| dial_sites(&seq, false, &all, Some(limit)).iter().filter(|c| c.enzyme == "BcgI").count();
    assert_eq!(bcgi(1), 0);
    assert_eq!(bcgi(2), 2);
    // Not ported: `findMatches(seq, false, "BcgI", "exact")` finds the one recognition site.
    assert_eq!(sites.iter().map(|s| (s.top_cut, s.bottom_cut)).collect::<Vec<_>>(), [(20, 18), (54, 52)]);
    let cuts: Vec<_> = sites.iter().map(cut_of).collect();
    let length = seq.len() as i64;
    let drawn = cuts_for_row(&cuts, Row { index: 0, start: 0, end: length - 1 }, length, false);
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|c| c.cuts_outside));
}

/// a short overhang crossing the circular origin occupies only its own bases
#[test]
fn a_short_overhang_crossing_the_circular_origin_occupies_only_its_own_bases() {
    let dna = "AATTCAAAAG"; // EcoRI reads across the origin; cuts 0 and 4.
    assert!(!find_cut_sites(dna, &["EcoRI"], true).is_empty());
    let n = dna.len() as i64;
    for shift in 0..dna.len() {
        let cut: CutSite = find_cut_sites(&rotated(dna, shift), &["EcoRI"], true).remove(0);
        let (from, to) = overhang_of(cut.top_cut as i64, cut.bottom_cut as i64, Some(4), Some(n)).unwrap();
        let unpaired: Vec<i64> = (0..n).filter(|&i| if from <= to { i >= from && i <= to } else { i >= from || i <= to }).collect();
        assert_eq!(unpaired.len(), 4);
        let own: Vec<i64> = (0..4).map(|j| (cut.top_cut as i64 + j) % n).collect();
        for i in &unpaired {
            assert!(own.contains(i));
        }
        let rows = [Row { index: 0, start: 0, end: 4 }, Row { index: 1, start: 5, end: 9 }];
        let pieces: Vec<_> = rows.iter().flat_map(|&r| cuts_for_row(&[cut_of(&cut)], r, n, true).into_iter().map(move |c| (c, r))).collect();
        let span: i64 =
            pieces.iter().map(|(c, r)| ((r.end + 1).min(c.draw_top.max(c.draw_bottom)) - r.start.max(c.draw_top.min(c.draw_bottom))).max(0)).sum();
        assert_eq!(span, 4, "rotation {shift}");
        assert!(pieces.iter().all(|(c, _)| (c.draw_bottom - c.draw_top).abs() == 4));
    }
}

/// a 3-prime origin overhang is short and reverse-oriented; linear terminal cuts remain visible
#[test]
fn a_3_prime_origin_overhang_is_short_and_reverse_oriented_and_linear_terminal_cuts_remain_visible() {
    assert_eq!(overhang_of(1, 9, Some(-2), Some(10)), Some((9, 0)));
    let cut = find_cut_sites("TTCACTG", &["TspRI"], false).into_iter().find(|c| c.strand == Strand::Reverse).unwrap();
    let row = cuts_for_row(&[cut_of(&cut)], Row { index: 0, start: 0, end: 6 }, 7, false);
    assert_eq!(row.len(), 1);
    assert_eq!(row[0].draw_top, 7);
}

/// recognition highlighting covers every recognition base through every circular origin
#[test]
fn recognition_highlighting_covers_every_recognition_base_through_every_circular_origin() {
    let dna = "AAAAAGAATTCAAAAA";
    let n = dna.len() as i64;
    for origin in 0..dna.len() {
        let turned = rotated(dna, origin);
        let c = find_cut_sites(&turned, &["EcoRI"], true).remove(0);
        let start = c.recognition_start as i64;
        let pieces = recognition_pieces(start, start + 5, n, true);
        let indices: Vec<usize> = pieces.iter().flat_map(|&(from, to)| from..=to).map(|i| i as usize).collect();
        assert_eq!(indices.len(), 6);
        assert_eq!(indices.iter().map(|&i| &turned[i..=i]).collect::<String>(), "GAATTC");
        assert_eq!(recognition_pieces(start, (start + 5) % n, n, true), pieces);
    }
    assert_eq!(recognition_pieces(9, 12, 10, false), []);
    assert_eq!(recognition_pieces(2, 12, 10, true), []);
    assert_eq!(recognition_pieces(-1, 4, 10, true), []);
}

/// Not in the original: the cuts the rows show at rest on the reference plasmid are the ones the
/// web app's rows show on it.
mod private {
    use super::*;

    /// A file in the private fixtures folder, which keeps this repository's layout.
    fn private_fixture(path: &str) -> std::path::PathBuf {
        let root = std::env::var_os("BIOS_PRIVATE_FIXTURES").expect("set BIOS_PRIVATE_FIXTURES to the private fixtures folder");
        std::path::PathBuf::from(root).join(path)
    }

    /// The lab construct's first 240 bases carry MunI, NruI and MluI in the web app's rows.
    #[test]
    #[ignore = "needs BIOS_PRIVATE_FIXTURES: answers recorded from private data"]
    fn the_rows_show_the_workhorse_enzymes_that_cut_once() {
        let path = private_fixture("crates/bios-core/tests/fixtures/lab_construct.gb");
        let construct = genbank::read(&std::fs::read_to_string(path).unwrap()).unwrap();
        let sites = default_cut_sites(&construct);
        let first: Vec<(&str, usize, usize)> = sites.iter().filter(|s| s.top_cut < 240).map(|s| (s.enzyme, s.top_cut, s.bottom_cut)).collect();
        assert_eq!(first, [("MunI", 161, 165), ("NruI", 208, 208), ("MluI", 228, 232)]);
        for site in &sites {
            assert_eq!(sites.iter().filter(|s| s.enzyme == site.enzyme).count(), 1, "{} cuts once", site.enzyme);
        }
    }
}

/// Not in the original: at rest the rows show only the common enzymes that cut once. On pUC19
/// those include the polylinker's, in its order.
#[test]
fn the_rows_show_the_common_enzymes_that_cut_once() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bios-core/tests/fixtures/pUC19.gb");
    let construct = genbank::read(&std::fs::read_to_string(path).unwrap()).unwrap();
    let sites = default_cut_sites(&construct);
    let mcs: Vec<(&str, usize, usize)> =
        sites.iter().filter(|s| (395..455).contains(&s.top_cut)).map(|s| (s.enzyme, s.top_cut, s.bottom_cut)).collect();
    // The polylinker's sites, by where each enzyme cuts the top and bottom strands: EcoRI G^AATTC,
    // whose site starts at 395, cuts the top before 396 and the bottom four bases on; PstI CTGCA^G
    // at 434 leaves its four on the 3' end; SmaI CCC^GGG at 411 is blunt.
    assert_eq!(
        mcs,
        [
            ("EcoRI", 396, 400),
            ("SacI", 406, 402),
            ("KpnI", 412, 408),
            ("Acc65I", 408, 412),
            ("SmaI", 414, 414),
            ("BamHI", 417, 421),
            ("XbaI", 423, 427),
            ("SalI", 429, 433),
            ("PstI", 439, 435),
            ("SphI", 445, 441),
            ("HindIII", 447, 451)
        ]
    );
    for site in &sites {
        assert_eq!(sites.iter().filter(|s| s.enzyme == site.enzyme).count(), 1, "{} cuts once", site.enzyme);
    }
}
