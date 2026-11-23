//! Where a feature's direction mark goes. Ported from Bios `editor/overview/direction.ts`.
//! Pure geometry, one rule, every renderer.
//!
//! THE RULE: the mark sits at the 5' end of each DRAWN PIECE, pointing the way the feature reads.
//! `>` at the left of a forward feature, `<` at the right of a reverse one.
//!
//! Two decisions are baked in here.
//!
//! 1. THE START, NOT THE END. Drawing a whole feature as an arrow lands the head at 3'. That
//!    answers "which way does this read?" one full annotation after you asked it, and on a feature
//!    wider than the viewport you never reach the answer at all. At 5' you meet the mark where you
//!    start reading, like the play head at the front of a track.
//!
//! 2. PER PIECE, NOT PER FEATURE. A construct that wraps the origin draws one feature as two arcs; a
//!    row view breaks one feature across four lines. Hanging the mark off the piece means every arc
//!    and every row states the direction, so it follows you down the page instead of waiting at an
//!    end you may never scroll to.
//!
//! Direction means ONE thing in this product; that is the whole point of centralising it. Whether
//! a feature gets a mark at all is a separate question: orientation is meaningless for an enhancer
//! or a cassette, and drawing nothing is the honest way to say so.

use super::viewport::js_number;

/// How long the mark is, along the molecule. Matches the row editor's chevron (7px).
pub const HEAD_PX: f64 = 7.0;

/// A length in px as a fraction of the circle at a given radius: arc length over circumference.
pub fn arrow_frac(px: f64, radius: f64) -> f64 {
    px / (2.0 * std::f64::consts::PI * 1f64.max(radius))
}

/// The mark's extent along the molecule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadSpan {
    pub base_frac: f64,
    pub tip_frac: f64,
}

/// The mark's extent along the molecule, as (base, tip) fractions.
///
/// `base` is the piece's 5' edge and `tip` is `head` further along in reading order, so tip is always
/// downstream of base: that is what makes the triangle read as `>` or `<` rather than just sitting
/// there. Callers draw base as the flat side across the band and tip as the apex on the centre line.
pub fn head_span(start_frac: f64, width_frac: f64, head: f64, forward: bool) -> HeadSpan {
    if forward {
        HeadSpan { base_frac: start_frac, tip_frac: start_frac + head }
    } else {
        HeadSpan { base_frac: start_frac + width_frac, tip_frac: start_frac + width_frac - head }
    }
}

/// [`head_span`]'s straight-line twin: a triangle at the 5' edge of a bar, drawn inside it.
pub fn head_path(x0: f64, x1: f64, y: f64, half: f64, forward: bool) -> String {
    let base = if forward { x0 } else { x1 };
    let tip = if forward { x0 + HEAD_PX } else { x1 - HEAD_PX };
    format!("M{} {}L{} {}L{} {}Z", js_number(base), js_number(y - half), js_number(tip), js_number(y), js_number(base), js_number(y + half))
}

/// Is the piece wide enough to carry a mark? Below this the head is most of the shape, which reads as
/// a triangle rather than as a bar with a direction: better to draw the bar and say nothing.
pub fn fits_head(width_along: f64, head: f64) -> bool {
    width_along > head * 1.6
}
