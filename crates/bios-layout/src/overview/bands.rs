//! The outer band: what is being ADDED, drawn outside the molecule. Ported from Bios
//! `editor/overview/bands.ts`.
//!
//! This is the one picture that teaches Gibson without a sentence of explanation. The insert is a
//! band standing off the ring over the opening, and its homology arms are drawn at parent
//! coordinates that lie on backbone which SURVIVES, so the arms visibly run past the cuts and lie
//! over DNA that stays. That overlap is not decoration; it is literally what the method is.
//!
//! Pure geometry, no radius: this file answers "which arcs, in fraction space", and the renderer
//! decides where to put them. Everything comes back as `Vec<Piece>` from `split_span`, so a band
//! crossing the origin is already two seam-cut pieces, exactly like every other span on the map.

use super::viewport::{Piece, js_round, split_span};

#[derive(Clone, Debug, PartialEq)]
pub struct OuterBand {
    /// The opening, in PARENT coordinates: removed span is the half-open [up, down).
    pub up: i64,
    pub down: i64,
    /// Homology arms in bp, measured back onto surviving backbone from each cut.
    pub left_arm_bp: i64,
    pub right_arm_bp: i64,
    /// Bases going in. Reported for the label; the body's width follows the OPENING, not this.
    pub insert_bp: i64,
    /// The body: new DNA.
    pub color: String,
    /// The arms. Distinct from the body on purpose: they are backbone you already have.
    pub arm_color: Option<String>,
    pub label: Option<String>,
    /// The fragments, in order 5'→3'; empty when the band has none. Each gets its own stretch of
    /// the body in its own colour, so the band stops being "the insert" and becomes the tubes that
    /// will actually arrive.
    ///
    /// They divide the body by LENGTH RATIO, not by base position: the body's angular width belongs
    /// to the opening (what leaves), and the fragments belong to the insert (what goes in). Those
    /// are two different numbers, and pretending otherwise would draw a band that runs past its
    /// own cuts.
    pub segments: Vec<BandSegment>,
    /// Where the overlaps are and how hot they run, measured in bp ALONG THE INSERT, 0 at the
    /// upstream cut and `insert_bp` at the downstream one. So the two outer homology arms are just
    /// the seams at each end. Empty when the band has none.
    pub seams: Vec<BandSeam>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandSegment {
    pub name: String,
    /// Where it starts and ends ALONG THE INSERT: 0 is the upstream cut, `insert_bp` the downstream
    /// one. Deliberately signed and unclamped: an eBlock carries the homology arm, so the first one
    /// genuinely starts before 0 and the last ends past `insert_bp`, out over backbone that stays.
    ///
    /// Segments MAY overlap each other. That is not a defect to be tidied away; it is why the
    /// pieces join, and it is the thing worth seeing.
    pub from_bp: i64,
    pub to_bp: i64,
    pub color: String,
    /// Which row to draw on (0 when `None`). Overlapping pieces on different rows read as two
    /// tiles, not one smear.
    pub tier: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandSeam {
    /// Distance along the insert, 0 … `insert_bp`.
    pub at_bp: i64,
    pub tm: f64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandLayout {
    /// Overlapping backbone that stays, immediately 5' of the upstream cut.
    pub arm_left: Vec<Piece>,
    /// Over the opening: the part of the band that is new DNA. One run when there are no segments.
    pub body: Vec<Piece>,
    /// One run per piece, in order. Empty when the band has no segments.
    pub segments: Vec<SegmentRun>,
    /// Overlapping backbone that stays, immediately 3' of the downstream cut.
    pub arm_right: Vec<Piece>,
    /// Junctions, as fractions round the molecule, for a tick and a temperature.
    pub seams: Vec<SeamMark>,
    /// True when the arms had to be shortened because the surviving backbone is smaller.
    pub clamped: bool,
}

/// One fragment's stretch of the band.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentRun {
    pub name: String,
    pub color: String,
    pub tier: usize,
    pub pieces: Vec<Piece>,
}

/// One junction, placed round the molecule.
#[derive(Clone, Debug, PartialEq)]
pub struct SeamMark {
    pub frac: f64,
    pub tm: f64,
    pub label: String,
}

/// `x` wrapped into `0..l`. Every caller has already returned on `l <= 0`; the check is here
/// because a remainder by zero panics.
fn modulo(x: i64, l: i64) -> i64 {
    if l > 0 { x.rem_euclid(l) } else { 0 }
}

/// [`modulo`] for a position between two bases.
fn modulo_f(x: f64, l: f64) -> f64 {
    ((x % l) + l) % l
}

/// Bases removed by an opening, going 5'→3' from `up` round to `down`.
pub fn removed_len(up: i64, down: i64, l: i64) -> i64 {
    if down >= up { down - up } else { l - up + down }
}

/// Where the band's three runs sit, in fraction space.
///
/// `min_body_frac` (0.012 when `None`) exists for the pure insertion: nothing is removed, so the
/// body has no angular width of its own and would vanish. A small wedge centred on the cut is the
/// honest drawing: something IS going in there, at a point.
pub fn layout_band(band: &OuterBand, l: i64, min_body_frac: Option<f64>) -> BandLayout {
    let min_body_frac = min_body_frac.unwrap_or(0.012);
    if l <= 0 {
        return BandLayout { arm_left: Vec::new(), body: Vec::new(), segments: Vec::new(), arm_right: Vec::new(), seams: Vec::new(), clamped: false };
    }
    let (up, down) = (band.up, band.down);
    let removed = removed_len(up, down, l);
    let surviving = l - removed;

    // An arm cannot be longer than the backbone that survives to hold it, and the two arms together
    // cannot wrap the molecule: otherwise the band eats its own tail and reads as a full ring.
    let want = band.left_arm_bp + band.right_arm_bp;
    let room = 0.max(surviving);
    let scale = if want > room && want > 0 { room as f64 / want as f64 } else { 1.0 };
    let left_bp = 0.max((band.left_arm_bp as f64 * scale).floor() as i64);
    let right_bp = 0.max((band.right_arm_bp as f64 * scale).floor() as i64);

    let arm_left = if left_bp > 0 { split_span(modulo(up - left_bp, l), modulo(up - 1, l), l) } else { Vec::new() };
    let arm_right = if right_bp > 0 { split_span(modulo(down, l), modulo(down + right_bp - 1, l), l) } else { Vec::new() };

    // The body's extent in bases, whether it comes from the opening or (for a pure insertion) from
    // the minimum wedge that keeps it visible.
    let half = 1.max(js_round((min_body_frac * l as f64) / 2.0) as i64);
    let body_start = if removed > 0 { up } else { modulo(up - half, l) };
    let body_len = if removed > 0 { removed } else { half * 2 };
    let body = split_span(modulo(body_start, l), modulo(body_start + body_len - 1, l), l);

    // ONE MAPPING, from "bp along the insert" to "where round the molecule", and everything drawn on
    // this band goes through it, so a seam can never land off the boundary it names.
    //
    // Inside the insert it is a RATIO: the body's angular width belongs to the opening, the bases
    // belong to the insert, and those are two different numbers. Outside it is 1:1, because out
    // there the band is lying over real parent backbone (the arms) which the map already draws at
    // its own scale. The scale changes at the cut, which is exactly where the molecule changes from
    // "what stays" to "what is being put in".
    let total_bp = if band.insert_bp != 0 { band.insert_bp } else { 1 };
    let at_base = |bp: i64| -> f64 {
        if bp <= 0 {
            return (up + bp) as f64;
        }
        if bp >= total_bp {
            return (down + (bp - total_bp)) as f64;
        }
        body_start as f64 + (body_len * bp) as f64 / total_bp as f64
    };
    let at_frac = |bp: i64| -> f64 { modulo_f(at_base(bp), l as f64) / l as f64 };

    let mut segments = Vec::new();
    for sgm in &band.segments {
        let from = js_round(at_base(sgm.from_bp)) as i64;
        let to = js_round(at_base(sgm.to_bp)) as i64;
        // LENGTH BY WALKING FORWARD, not by subtraction. A segment that crosses the origin has a
        // numerically SMALLER end than start, so `to > from` silently dropped it, which is how a
        // fragment vanished on exactly the constructs where the opening wraps.
        let len = modulo(to - from, l);
        if len > 0 {
            segments.push(SegmentRun {
                name: sgm.name.clone(),
                color: sgm.color.clone(),
                tier: sgm.tier.unwrap_or(0),
                pieces: split_span(modulo(from, l), modulo(from + len - 1, l), l),
            });
        }
    }

    let seams = band.seams.iter().map(|sm| SeamMark { frac: at_frac(sm.at_bp), tm: sm.tm, label: sm.label.clone() }).collect();

    BandLayout { arm_left, body, segments, arm_right, seams, clamped: scale < 1.0 }
}

/// How much of a feature sits inside the opening: 0 (untouched) to 1 (entirely inside).
///
/// Drives the dim: a feature mostly inside the doomed span is leaving with it, and saying so on
/// the map is what makes "1,292 bp comes out" mean something. Both the feature and the span may
/// cross the origin, which is the whole reason this is not `min`/`max` arithmetic.
pub fn overlap_fraction(feat_start: i64, feat_end: i64, up: i64, down: i64, l: i64) -> f64 {
    if l <= 0 {
        return 0.0;
    }
    let in_opening = |i: i64| {
        let x = modulo(i, l);
        if down >= up { x >= up && x < down } else { x >= up || x < down }
    };
    let feat_len = if feat_end >= feat_start { feat_end - feat_start + 1 } else { l - feat_start + feat_end + 1 };
    if feat_len <= 0 {
        return 0.0;
    }
    let mut hit: i64 = 0;
    for k in 0..feat_len {
        if in_opening(feat_start + k) {
            hit += 1;
        }
    }
    hit as f64 / feat_len as f64
}

/// A feature this far inside the opening reads as leaving, and is drawn dim.
pub const DIM_AT: f64 = 0.5;
