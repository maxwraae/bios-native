//! The band behind every other row (`.seq-row-band` in Bios `editor/rows/SequenceRows.tsx`).

use bios_layout::rows::{Row, band_for, metrics_for, track};

#[test]
fn every_other_row_is_banded_margin_to_margin_with_its_edges_in_the_middle_of_the_gaps() {
    let m = metrics_for(1000.0, None, None);
    let row = |index| Row { index, start: index as i64 * m.bps_per_row, end: (index as i64 + 1) * m.bps_per_row - 1 };
    assert_eq!(band_for(row(0), 80.0, &m), None, "the first row sits on the paper");
    assert_eq!(band_for(row(2), 80.0, &m), None);
    let (x, y, w, h) = band_for(row(1), 80.0, &m).unwrap();
    assert_eq!(x, -m.margin_width / 2.0);
    assert_eq!(w, m.content_width + m.margin_width);
    assert_eq!(y, -track::ROW_GAP / 2.0);
    assert_eq!(h, 80.0, "as tall as the row it bands, so the next band starts where this one ends");
}
