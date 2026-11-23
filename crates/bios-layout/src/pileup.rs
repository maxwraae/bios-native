//! Where the mapper's reads sit in the pileup: each read's pieces in display columns, the reads
//! packed into lanes, and the window over the columns. Ported from the web's
//! `reads/mapperLayout.ts` (`rowSegments`), `reads/pile.ts` (`packRows`) and
//! `reads/pileup/viewport.ts`.

/// One piece of a read: `length` of its characters from `offset`, drawn from column `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub start: usize,
    pub offset: usize,
    pub length: usize,
}

/// A read's pieces in the padded reference's columns. A circular read past the end continues at
/// column 0; a linear read is cut at both ends. Every character is kept.
pub fn row_segments(leading: i64, internal_len: usize, length: usize, circular: bool) -> Vec<Piece> {
    let mut parts = Vec::new();
    if length == 0 {
        return parts;
    }
    let n = length as i64;
    let mut offset: i64 = if circular { 0 } else { (-leading).max(0) };
    let mut guard = 0;
    while (offset as usize) < internal_len && guard < internal_len + 2 {
        guard += 1;
        let raw = leading + offset;
        let start = if circular { raw.rem_euclid(n) } else { raw };
        let take = (internal_len as i64 - offset).min(n - start);
        if take <= 0 {
            break;
        }
        if start >= 0 {
            parts.push(Piece { start: start as usize, offset: offset as usize, length: take as usize });
        }
        offset += take;
    }
    parts
}

/// Reads packed into lanes: no two in one lane overlap (with `gutter` columns between them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pile {
    /// Each item's lane.
    pub lane: Vec<usize>,
    /// The items lane by lane, each lane by start: `order[offset[l]..offset[l + 1]]`.
    pub order: Vec<usize>,
    pub offset: Vec<usize>,
    pub lanes: usize,
}

/// Greedy first fit by start (`packRows`). `ends` are inclusive; an end before its start runs to
/// the end of the reference.
pub fn pack(starts: &[usize], ends: &[usize], length: usize, gutter: usize) -> Pile {
    let n = starts.len().min(ends.len());
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by_key(|&i| (starts[i], i));
    let mut lane = vec![0usize; n];
    let mut lane_end: Vec<i64> = Vec::new();
    for &i in &idx {
        let s = starts[i] as i64;
        let e = if ends[i] < starts[i] { length.saturating_sub(1) as i64 } else { ends[i] as i64 };
        let placed = lane_end.iter().position(|&end| end + (gutter as i64) < s).unwrap_or_else(|| {
            lane_end.push(-1);
            lane_end.len() - 1
        });
        lane[i] = placed;
        if let Some(slot) = lane_end.get_mut(placed) {
            *slot = e;
        }
    }
    let lanes = lane_end.len();
    let mut offset = vec![0usize; lanes + 1];
    for &l in &lane {
        offset[l + 1] += 1;
    }
    for l in 0..lanes {
        offset[l + 1] += offset[l];
    }
    let mut cursor = offset.clone();
    let mut order = vec![0usize; n];
    for &i in &idx {
        let l = lane[i];
        order[cursor[l]] = i;
        cursor[l] += 1;
    }
    Pile { lane, order, offset, lanes }
}

/// The narrowest window, in columns.
pub const MIN_BASES: f64 = 24.0;

/// A window over the columns: the first visible one (fractional) and how many fit across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub from: f64,
    pub bases: f64,
}

impl Window {
    /// The whole reference.
    pub fn whole(length: usize) -> Window {
        Window { from: 0.0, bases: (length as f64).max(MIN_BASES) }
    }

    /// Kept inside the reference.
    pub fn clamp(self, length: usize) -> Window {
        let n = length as f64;
        let bases = self.bases.min(n).max(MIN_BASES.min(n.max(1.0)));
        let from = self.from.min(n - bases).max(0.0);
        Window { from, bases }
    }

    /// Zoomed by `factor` (below 1 is closer) about `anchor`, 0..1 across.
    pub fn zoom_at(self, length: usize, factor: f64, anchor: f64) -> Window {
        let bases = self.bases * factor;
        let at = self.from + anchor * self.bases;
        Window { from: at - anchor * bases, bases }.clamp(length)
    }

    pub fn pan_by(self, length: usize, delta: f64) -> Window {
        Window { from: self.from + delta, bases: self.bases }.clamp(length)
    }

    /// `column` in the middle at `bases` across.
    pub fn centred(length: usize, column: f64, bases: f64) -> Window {
        Window { from: column - bases / 2.0, bases }.clamp(length)
    }

    pub fn x_of(self, column: f64, width: f32) -> f32 {
        ((column - self.from) / self.bases * f64::from(width)) as f32
    }

    pub fn column_at(self, x: f32, width: f32) -> f64 {
        self.from + f64::from(x) / f64::from(width.max(1.0)) * self.bases
    }
}

/// Coverage binned for a strip `bins` wide: each bin's mean, and whether a called column is in it.
pub fn depth_bins(coverage: &[f64], called: &[bool], bins: usize) -> Vec<(f64, bool)> {
    let n = coverage.len();
    if n == 0 || bins == 0 {
        return Vec::new();
    }
    let bins = bins.min(n);
    (0..bins)
        .map(|b| {
            let (lo, hi) = (b * n / bins, ((b + 1) * n / bins).max(b * n / bins + 1).min(n));
            let sum: f64 = coverage[lo..hi].iter().sum();
            let hit = called.get(lo..hi).is_some_and(|c| c.iter().any(|x| *x));
            (sum / (hi - lo) as f64, hit)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_origin_negative_leading_columns_and_gaps_retain_every_read_character() {
        let parts = row_segments(8, 6, 10, true);
        assert_eq!(parts, vec![Piece { start: 8, offset: 0, length: 2 }, Piece { start: 0, offset: 2, length: 4 }]);
        assert_eq!(row_segments(-2, 4, 10, true), vec![Piece { start: 8, offset: 0, length: 2 }, Piece { start: 0, offset: 2, length: 2 }]);
        assert_eq!(row_segments(-2, 4, 10, false), vec![Piece { start: 0, offset: 2, length: 2 }]);
    }

    #[test]
    fn reads_that_do_not_overlap_share_a_lane_and_overlapping_ones_do_not() {
        let pile = pack(&[0, 10, 5], &[4, 14, 9], 100, 2);
        // 0..4 then 10..14 fit one lane (a gutter of 2); 5..9 sits 1 after the first and needs another.
        assert_eq!(pile.lanes, 2);
        assert_eq!(pile.lane, vec![0, 0, 1]);
        assert_eq!(&pile.order[pile.offset[0]..pile.offset[1]], &[0, 1]);
    }

    #[test]
    fn no_two_reads_in_a_lane_overlap_and_every_read_is_placed_once() {
        let starts: Vec<usize> = (0..200).map(|i| (i * 37) % 500).collect();
        let ends: Vec<usize> = starts.iter().map(|s| s + 60).collect();
        let pile = pack(&starts, &ends, 600, 2);
        assert_eq!(pile.order.len(), 200);
        for l in 0..pile.lanes {
            let items = &pile.order[pile.offset[l]..pile.offset[l + 1]];
            for w in items.windows(2) {
                assert!(ends[w[0]] + 2 < starts[w[1]]);
            }
        }
        assert!(pack(&[], &[], 10, 2).order.is_empty());
    }

    #[test]
    fn the_window_stays_inside_and_zoom_keeps_the_anchor_still() {
        let w = Window::whole(1000);
        let z = w.zoom_at(1000, 0.5, 0.5);
        assert!((z.from - 250.0).abs() < 1e-9 && (z.bases - 500.0).abs() < 1e-9);
        assert_eq!(Window { from: -40.0, bases: 10.0 }.clamp(1000), Window { from: 0.0, bases: 24.0 });
        assert_eq!(z.pan_by(1000, 10_000.0).from, 500.0);
        assert!((z.column_at(z.x_of(400.0, 800.0), 800.0) - 400.0).abs() < 1e-6);
    }

    #[test]
    fn depth_bins_average_and_flag_called_columns() {
        let bins = depth_bins(&[1.0, 3.0, 5.0, 7.0], &[false, false, false, true], 2);
        assert_eq!(bins, vec![(2.0, false), (6.0, true)]);
    }
}
