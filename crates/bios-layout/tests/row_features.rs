//! Parity with Bios `editor/rows/RowFeatures.test.ts`: the primer ribbon's geometry, off the
//! screen: which columns pair, which lift, and what a lane costs. One test here per test there, in
//! the same order, with the same assertions and the same numbers.

use bios_core::duplex::{AlignRun, AlignRunKind};
use bios_layout::row_features::{CellKind, Kind, PrimerInsert, RowFeature, cells_from_runs, pieces_for_row, primer_cells};
use bios_layout::rows::{Row, track};

const ROW: Row = Row { index: 0, start: 0, end: 59 };
const SEQ: &str = "ACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT";

/// `primer({})`: the original's default primer, to be changed with struct update syntax.
fn primer() -> RowFeature<'static> {
    RowFeature { start: 10, end: 19, forward: true, directional: true, name: "p", locations: None, bases: None, runs: None }
}

fn kinds(f: &RowFeature, from: i64, to: i64) -> Vec<CellKind> {
    primer_cells(f, Some(SEQ), from, to).iter().map(|c| c.kind).collect()
}

fn primers(features: &[RowFeature]) -> bios_layout::row_features::RowPieces {
    pieces_for_row(features, ROW, 12.0, SEQ.len() as i64, Kind::Primer, Some(SEQ), false)
}

/// a made primer pairs everywhere it reads the template, and a `&` is a gap
#[test]
fn a_made_primer_pairs_everywhere_it_reads_the_template_and_an_ampersand_is_a_gap() {
    let bases = format!("{}&&{}", &SEQ[10..15], &SEQ[17..20]);
    let f = RowFeature { bases: Some(&bases), ..primer() };
    let cells = primer_cells(&f, Some(SEQ), 10, 19);
    use CellKind::{Gap, Paired};
    assert_eq!(cells.iter().map(|c| c.kind).collect::<Vec<_>>(), [Paired, Paired, Paired, Paired, Paired, Gap, Gap, Paired, Paired, Paired]);
    assert_eq!(cells[0].ch, Some('G'), "SEQ[10] in ACGT repeated");
}

/// a reverse primer pairs against the complement, so its own bases read as the bottom strand
#[test]
fn a_reverse_primer_pairs_against_the_complement_so_its_own_bases_read_as_the_bottom_strand() {
    use CellKind::{Change, Paired};
    // SEQ[10..13] is GTAC; the bottom strand there is CATG, read right to left as GTAC.
    let f = RowFeature { start: 10, end: 13, forward: false, bases: Some("GTAC"), ..primer() };
    assert_eq!(kinds(&f, 10, 13), [Paired, Paired, Paired, Paired]);
    // The query's third base, read 5'→3' along the bottom strand, sits over column 11.
    let wrong = RowFeature { start: 10, end: 13, forward: false, bases: Some("GTCC"), ..primer() };
    assert_eq!(kinds(&wrong, 10, 13), [Paired, Change, Paired, Paired]);
}

/// two primers whose ribbons do not overlap still share no lane if the name of one lands on the other
#[test]
fn two_primers_whose_ribbons_do_not_overlap_still_share_no_lane_if_the_name_of_one_lands_on_the_other() {
    let a = RowFeature { name: "a-long-primer-name", start: 30, end: 39, ..primer() };
    let b = RowFeature { name: "b", start: 18, end: 27, ..primer() };
    assert_eq!(primers(&[a, b]).lanes, 2, "the name beside a's 5' end reaches back over b");
    let far = RowFeature { name: "c", start: 0, end: 5, ..primer() };
    assert_eq!(primers(&[a, far]).lanes, 1);
}

/// a lane holding a changed base is a half-line taller, and the ribbon sits below that room on the forward side
#[test]
fn a_lane_holding_a_changed_base_is_a_half_line_taller_and_the_ribbon_sits_below_that_room_on_the_forward_side() {
    let clean = RowFeature { bases: Some(&SEQ[10..20]), ..primer() };
    let changed = RowFeature { start: 30, end: 39, bases: Some("GGGGGGGGGG"), ..primer() };
    let one = primers(&[clean]);
    assert_eq!(one.height, track::PRIMERS);
    assert_eq!(one.lift, 0.0);
    assert_eq!(one.pieces[0].y, 0.0);
    let two = primers(&[clean, changed]);
    assert_eq!(two.lanes, 1, "they do not overlap, so one lane");
    assert_eq!(two.height, track::PRIMERS + track::PRIMER_LIFT);
    assert_eq!(two.lift, track::PRIMER_LIFT);
    assert_eq!(two.pieces[0].y, track::PRIMER_LIFT, "forward: the hang goes up, so the ribbon sits under it");
    let below = primers(&[RowFeature { forward: false, ..changed }]);
    assert_eq!(below.pieces[0].y, 0.0, "reverse: the hang goes down, so the ribbon stays at the top");
}

fn run(kind: AlignRunKind, bases: &str, length: usize, template_length: usize, start_col: i64, step: i64) -> AlignRun {
    let paired = matches!(kind, AlignRunKind::Match | AlignRunKind::Mismatch);
    AlignRun { kind, paired, bases: bases.into(), oligo_start: Some(0), length, template_length, start_col, step, off_template: false }
}

/// an aligned oligo places its tail before the anchor, its deletion on the skipped columns and its insert on a boundary
#[test]
fn an_aligned_oligo_places_its_tail_before_the_anchor_its_deletion_on_the_skipped_columns_and_its_insert_on_a_boundary() {
    use AlignRunKind::{Deletion, Insertion, Match, Mismatch, Overhang};
    let runs = [
        run(Overhang, "GAATTC", 6, 0, 14, 1),
        run(Match, &SEQ[20..25], 5, 5, 20, 1),
        run(Insertion, "TT", 2, 0, 25, 1),
        run(Match, &SEQ[25..28], 3, 3, 25, 1),
        run(Deletion, "", 0, 2, 28, 1),
        run(Mismatch, "G", 1, 1, 30, 1),
        run(Match, &SEQ[31..34], 3, 3, 31, 1),
    ];
    let laid = cells_from_runs(&runs, ROW, SEQ.len() as i64, true);
    assert_eq!(laid.five_at, 14, "the 5' end is the tail's far end");
    assert_eq!(laid.three_at, 33);
    assert_eq!(laid.inserts, [PrimerInsert { edge: 25, bases: "TT".into() }]);
    let kind = |bp: i64| laid.cells.iter().find(|c| c.bp == bp).map(|c| c.kind);
    assert_eq!(kind(14), Some(CellKind::Tail));
    assert_eq!(kind(19), Some(CellKind::Tail));
    assert_eq!(kind(20), Some(CellKind::Paired));
    assert_eq!(kind(28), Some(CellKind::Gap));
    assert_eq!(kind(29), Some(CellKind::Gap));
    assert_eq!(kind(30), Some(CellKind::Change));
    assert_eq!(kind(33), Some(CellKind::Paired));
    assert_eq!(laid.cells.len(), 20, "six tail, five, three, two gaps, one change, three");

    let f = RowFeature { name: "probe", start: 20, end: 33, runs: Some(&runs), ..primer() };
    let got = pieces_for_row(&[f], ROW, 12.0, SEQ.len() as i64, Kind::Primer, Some(SEQ), true);
    assert_eq!(got.pieces.len(), 1);
    assert_eq!(got.pieces[0].start, 14, "the piece begins at the tail, not the footprint");
    assert_eq!(got.pieces[0].end, 33);
    assert_eq!(got.height, track::PRIMERS + track::PRIMERS, "an insert needs a whole line");
}

/// a reverse insert sits on the column edge to the right of where the alignment resumes
#[test]
fn a_reverse_insert_sits_on_the_column_edge_to_the_right_of_where_the_alignment_resumes() {
    use AlignRunKind::{Insertion, Match};
    let runs = [run(Match, "AAAA", 4, 4, 30, -1), run(Insertion, "GG", 2, 0, 26, -1), run(Match, "AAAA", 4, 4, 26, -1)];
    let laid = cells_from_runs(&runs, ROW, SEQ.len() as i64, false);
    assert_eq!(laid.inserts, [PrimerInsert { edge: 27, bases: "GG".into() }]);
    assert_eq!(laid.five_at, 30);
    assert_eq!(laid.three_at, 23);
}
