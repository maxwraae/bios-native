//! Where a feature's 5' cap is drawn in the rows: on every unwrapped piece whose 5' end is in it,
//! as Bios `editor/rows/RowFeatures.tsx` `piecesForRow` asks each piece.

use bios_layout::row_features::{Kind, RowFeature, pieces_for_row};
use bios_layout::rows::Row;

const LENGTH: i64 = 800;

fn wrapped(forward: bool) -> RowFeature<'static> {
    RowFeature { start: 749, end: 99, forward, directional: true, name: "AmpR", locations: None, bases: None, runs: None }
}

fn heads(f: RowFeature, row: Row) -> Vec<bool> {
    pieces_for_row(&[f], row, 10.0, LENGTH, Kind::Feature, None, false).pieces.iter().map(|p| p.show_head).collect()
}

#[test]
fn a_feature_through_the_origin_is_capped_on_both_halves_where_each_has_its_5_prime_end() {
    let first = Row { index: 0, start: 0, end: 59 };
    let last = Row { index: 13, start: 780, end: 799 };
    let at_start = Row { index: 12, start: 720, end: 779 };
    // Forward: 5' is each half's start, so the cap is at 749 and again at the origin.
    assert_eq!(heads(wrapped(true), at_start), [true]);
    assert_eq!(heads(wrapped(true), first), [true]);
    assert_eq!(heads(wrapped(true), last), [false]);
    // Reverse: 5' is each half's end, at 799 and at 99.
    assert_eq!(heads(wrapped(false), last), [true]);
    assert_eq!(heads(wrapped(false), first), [false]);
    assert_eq!(heads(wrapped(false), Row { index: 1, start: 60, end: 119 }), [true]);
}
