//! Not in the original (the web has no test for these): the frame of a translation, the span it
//! reads, and where each residue stands over its codon on both strands.

use bios_layout::rows::Row;
use bios_layout::translations::{Coding, Translation, is_coding, residues_for_row, span_bases};

/// The standard code for the few codons these tests read.
fn translate(bases: &str) -> String {
    bases
        .as_bytes()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| match &c[..] {
            b"ATG" => 'M',
            b"AAA" => 'K',
            b"TAG" => '*',
            b"CTA" => 'L',
            b"TTT" => 'F',
            b"CAT" => 'H',
            _ => 'X',
        })
        .collect()
}

fn revcomp(bases: &str) -> String {
    bases
        .chars()
        .rev()
        .map(|b| match b {
            'A' => 'T',
            'T' => 'A',
            'C' => 'G',
            _ => 'C',
        })
        .collect()
}

fn codons(c: Coding, seq: &str) -> Vec<(usize, char)> {
    let Some(bases) = span_bases(c, seq) else { return Vec::new() };
    let protein = translate(&if c.forward { bases.to_string() } else { revcomp(bases) });
    bios_layout::translations::codons(c, &protein)
}

#[test]
fn a_frame_is_said_the_way_a_biologist_says_it_and_steps_in_from_the_reading_end() {
    let forward = Translation { start: 10, end: 30, forward: true, frame: 1 };
    assert_eq!(forward.frame_label(), "+2");
    assert_eq!(forward.reading_span(), Some((11, 30)));
    let reverse = Translation { start: 10, end: 30, forward: false, frame: 2 };
    assert_eq!(reverse.frame_label(), "−3");
    assert_eq!(reverse.reading_span(), Some((10, 28)));
    assert_eq!(Translation { start: 0, end: 1, forward: false, frame: 2 }.reading_span(), None);
}

#[test]
fn only_cds_and_gene_are_coding() {
    assert!(is_coding("CDS") && is_coding("gene"));
    assert!(!is_coding("promoter") && !is_coding("misc_feature"));
}

#[test]
fn a_forward_span_reads_from_its_own_start_and_each_residue_sits_on_its_codons_middle_base() {
    //              0123456789
    let seq = "CCATGAAATAGCC";
    let read = codons(Coding { start: 2, end: 10, forward: true }, seq);
    assert_eq!(read, [(2, 'M'), (5, 'K'), (8, '*')]);
    let row = Row { index: 0, start: 0, end: 12 };
    let residues = residues_for_row(&read, row, 10.0);
    assert_eq!(residues.iter().map(|r| r.x).collect::<Vec<_>>(), [35.0, 65.0, 95.0]);
    assert!(residues[0].start && !residues[1].start);
    assert!(residues[2].stop);
}

#[test]
fn a_reverse_span_reads_from_its_end_backwards() {
    // The bottom strand of 2..10 read 5' to 3' is CTATTTCAT: L, F, H, read from base 10 down.
    let seq = "CCATGAAATAGCC";
    let read = codons(Coding { start: 2, end: 10, forward: false }, seq);
    assert_eq!(read, [(8, 'L'), (5, 'F'), (2, 'H')]);
    // A row that starts at 7 keeps only the residue whose middle base (9) falls in it.
    let residues = residues_for_row(&read, Row { index: 1, start: 7, end: 12 }, 10.0);
    assert_eq!(residues.len(), 1);
    assert_eq!(residues[0].aa, 'L');
    assert_eq!(residues[0].x, 25.0);
}

#[test]
fn the_rows_each_residue_falls_in_agree_with_asking_row_by_row() {
    let seq = "CCATGAAATAGCC";
    let read = codons(Coding { start: 2, end: 10, forward: true }, seq);
    let rows = [Row { index: 0, start: 0, end: 4 }, Row { index: 1, start: 5, end: 9 }, Row { index: 2, start: 10, end: 12 }];
    let placed = bios_layout::translations::residues_in_rows(&read, &rows, 5, 10.0);
    let asked: Vec<(usize, bios_layout::translations::Residue)> =
        rows.iter().flat_map(|row| residues_for_row(&read, *row, 10.0).into_iter().map(|r| (row.index, r))).collect();
    assert_eq!(placed, asked);
    assert_eq!(placed.iter().map(|(row, _)| *row).collect::<Vec<_>>(), [0, 1, 1]);
}

#[test]
fn fewer_than_three_bases_read_nothing() {
    assert!(codons(Coding { start: 0, end: 1, forward: true }, "ATG").is_empty());
    assert!(codons(Coding { start: 5, end: 9, forward: true }, "ATG").is_empty());
}

/// Not in the original either: the rows' zoom and their edge scroll, from `SequenceRows.tsx`.
#[test]
fn the_rows_zoom_at_the_maps_rate_up_to_the_widest_base_and_out_past_the_letters() {
    use bios_layout::rows::{DEFAULT_CHAR_WIDTH, HIDE_BASES_BELOW, MAX_CHAR_WIDTH, MIN_CHAR_WIDTH, rows_hand_back, zoom_char_width};
    let wider = zoom_char_width(DEFAULT_CHAR_WIDTH, -10.0);
    assert!((wider - DEFAULT_CHAR_WIDTH * 0.2f64.exp()).abs() < 1e-9);
    // A step is capped at 30 however hard the wheel turns.
    assert_eq!(zoom_char_width(DEFAULT_CHAR_WIDTH, -500.0), zoom_char_width(DEFAULT_CHAR_WIDTH, -30.0));
    assert_eq!(zoom_char_width(MAX_CHAR_WIDTH, -30.0), MAX_CHAR_WIDTH);
    // Out past the letters, which is what hands the rows back to the map (`onZoomedOut`).
    let past = zoom_char_width(HIDE_BASES_BELOW, 30.0);
    assert!(past < HIDE_BASES_BELOW && rows_hand_back(past));
    assert!(!rows_hand_back(HIDE_BASES_BELOW));
    assert_eq!(zoom_char_width(MIN_CHAR_WIDTH, 30.0), MIN_CHAR_WIDTH);
}

#[test]
fn a_drag_held_against_an_edge_scrolls_faster_the_deeper_it_goes() {
    use bios_layout::rows::edge_scroll;
    assert_eq!(edge_scroll(300.0, 600.0), 0.0, "the middle stays put");
    assert_eq!(edge_scroll(16.0, 600.0), -8.0, "half into the 32px top band: half a step up");
    assert_eq!(edge_scroll(-200.0, 600.0), -48.0, "past the top: three steps at most");
    assert_eq!(edge_scroll(600.0 - 36.0, 600.0), 8.0, "the bottom band is 72 deep");
    assert_eq!(edge_scroll(900.0, 600.0), 48.0);
}
