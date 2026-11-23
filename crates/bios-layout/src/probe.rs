//! The caption of each of Find's alignments, in the row the hit starts in. Ported from the Bios web
//! app's `editor/rows/RowProbe.tsx`.
//!
//! The picture of a hit is drawn by the primer lane (its runs, see `row_features::cells_from_runs`).
//! What is left here is the sentence, "22/28 nt aligned · 95% identity · 1 mismatch · Tm 59°C ·
//! would prime", beside the hit it describes, in a lane of its own below everything the strands
//! stack.

use std::collections::BTreeMap;

use bios_core::find_align::{AlignHit, DRAWN_LIMIT, describe_hit, hit_runs, row_segments};

/// Average advance of a caption character at 10px in the UI font: an over-estimate, so a caption
/// erring wide parks slightly left of the edge rather than running its tail off the screen.
pub const CAPTION_CHAR_PX: f64 = 5.2;
/// Baseline of the caption inside its lane.
pub const CAPTION_BASELINE: f64 = 12.0;

/// One hit's caption, in the row its first column falls in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caption {
    /// Which of the hits handed in this describes.
    pub hit: usize,
    pub text: String,
    /// The column inside the row it starts at.
    pub col: usize,
}

/// Every drawn hit's caption, by row index, in the order they were ranked. Only the first
/// [`DRAWN_LIMIT`] are drawn, the same cap the ribbons use.
pub fn probe_by_row(hits: &[AlignHit], sequence_length: usize, circular: bool, bps_per_row: usize) -> BTreeMap<usize, Vec<Caption>> {
    let mut map: BTreeMap<usize, Vec<Caption>> = BTreeMap::new();
    if sequence_length == 0 || bps_per_row == 0 {
        return map;
    }
    for (index, hit) in hits.iter().take(DRAWN_LIMIT).enumerate() {
        let runs = hit_runs(hit, sequence_length, circular);
        // Anchored on the hit's FIRST segment, paired or not: a perfect hit lifts nothing and still
        // has to say "20/20 nt aligned · 100% identity · would prime".
        let Some(first) = row_segments(&runs, bps_per_row, sequence_length, circular).into_iter().min_by_key(|s| (s.row_index, s.col_in_row)) else {
            continue;
        };
        map.entry(first.row_index).or_default().push(Caption { hit: index, text: describe_hit(hit, Some(sequence_length)), col: first.col_in_row });
    }
    map
}

/// Where a caption starts so that the WHOLE of it stays inside the row: clamped against its own
/// measured width, or the half that says whether it would prime is the half cut off.
pub fn caption_x(wanted: f64, text: &str, content_width: f64) -> f64 {
    wanted.min(content_width - text.chars().count() as f64 * CAPTION_CHAR_PX).max(0.0)
}

#[cfg(test)]
mod tests {
    use bios_core::find_align::alignment_hits;

    use super::{caption_x, probe_by_row};

    #[test]
    fn a_caption_sits_in_the_row_its_hit_starts_in_and_never_runs_off_the_right() {
        let t = "GGGGGGGGGGATGCGTACGTTGACCGATCATTTTTTTTTTAAACCCTTTGGGACGTACGT";
        let hits = alignment_hits(t, false, &t[10..40]);
        let rows = probe_by_row(&hits, t.len(), false, 25);
        let first = rows.get(&0).and_then(|r| r.first()).map(|c| (c.hit, c.col));
        assert_eq!(first, Some((0, 10)));
        assert!(rows[&0][0].text.starts_with("30/30 nt aligned"));
        assert!(probe_by_row(&hits, t.len(), false, 0).is_empty());
        assert_eq!(caption_x(500.0, "abcd", 400.0), 400.0 - 4.0 * 5.2);
        assert_eq!(caption_x(500.0, &"x".repeat(200), 400.0), 0.0);
        assert_eq!(caption_x(30.0, "abcd", 400.0), 30.0);
    }
}
