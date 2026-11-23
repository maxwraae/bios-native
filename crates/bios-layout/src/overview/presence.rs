//! The floor under a feature's drawn size: how the map says "something is here" about a thing too
//! small to draw at its true width. Ported from Bios `editor/overview/presence.ts`.
//!
//! THE THREE STATES, and why the middle one is the bug. A 3bp feature on a 6kb plasmid is about
//! a thousandth of the ring. Drawn honestly it lands somewhere under a pixel, and the renderer
//! rounds it to a grey hairline that flickers in and out as you pan, or drops it entirely.
//!
//!   absent      says "nothing here". Wrong, but at least it is a clear claim.
//!   hairline    says nothing at all, while still taking your attention. The worst of the three.
//!   mark        says "something, too small to name". True, and useful.
//!
//! So nothing is ever drawn thinner than [`MIN_MARK_PX`]. What is widened is the DRAWING, never the
//! coordinates: `mark` comes back with the widened span so the caller can style it as the
//! approximation it is rather than passing it off as a measurement. Everything that reports
//! position (labels, the readout, hit-testing) keeps using the true span.
//!
//! PIXELS, NOT FRACTIONS, is the whole reason this is shared. The linear view measures in screen x,
//! the ring in fractions of a turn, and on the ring a lane's circumference shrinks as it nests
//! inward: lane 4 has roughly a fifth of lane 0's. A fixed fractional floor would therefore be a
//! different physical size in every lane and at every zoom, so the caller converts `MIN_MARK_PX`
//! into the fraction that means that many pixels AT ITS OWN RADIUS (`direction::arrow_frac`) and
//! hands it in. That is what keeps a 3bp feature the same size on the ring, on the line, and at
//! both ends of the morph; without it, the transition was the one place small features blinked out
//! and came back.

/// The narrowest anything is ever drawn, in pixels.
///
/// Small enough to still read as "a tick, not a feature" (it must never be mistaken for a span
/// you could name or click along) and comfortably above the sub-pixel rounding that produced the
/// flickering hairline this exists to replace.
pub const MIN_MARK_PX: f64 = 3.0;

/// Is this span (in pixels) too narrow to draw honestly?
pub fn is_mark(x0: f64, x1: f64) -> bool {
    (x1 - x0).abs() < MIN_MARK_PX
}

/// Widen a pixel span to the mark floor, keeping its CENTRE fixed.
///
/// Centred rather than extended rightward so the tick sits where the feature is: growing one edge
/// would shift a 1bp feature up to 3px away from its own coordinate, which at high zoom is a
/// visible lie about position.
pub fn widen_to_mark(x0: f64, x1: f64) -> (f64, f64) {
    let lo = x0.min(x1);
    let hi = x0.max(x1);
    let w = hi - lo;
    if w >= MIN_MARK_PX {
        return (lo, hi);
    }
    let mid = (lo + hi) / 2.0;
    (mid - MIN_MARK_PX / 2.0, mid + MIN_MARK_PX / 2.0)
}

// ---- The seam: ground between one feature and the next ----
//
// Colour in this product names a ROLE, not a type: four hues and a neutral, on purpose. The cost
// of that decision is that neighbours are routinely the same colour: two promoters end to end, or
// the run of `misc_feature` greys on a cloning map. Drawn with butt caps and no edge they fuse into
// one long band, and a construct's parts stop being countable: you cannot see that the amber block
// is two promoters rather than one long one.
//
// The fix is ground left visible rather than more hues: a feature is a cell, the lane is its
// ground, and a hairline of ground runs between any two of them. It costs no ink, adds no colour,
// and works for two greys as well as it does for two greens.
//
// What it must never do is eat a small feature. The seam is given up from the DRAWN span and is
// clamped at the mark floor above, so a 3bp tick keeps its full presence and only features with
// width to spare pay for the seam. It is taken evenly from both ends, so a feature's drawn centre
// stays on its true centre, the same reason `widen_to_mark` is centred.

/// The ground left between two abutting features, in pixels. Each gives up half.
pub const SEAM_PX: f64 = 2.0;

/// Shrink a drawn width to leave half a seam at each end, never going below `floor`.
///
/// Returns the new width. Clamped rather than all-or-nothing so a feature approaching the floor
/// loses the seam gradually instead of snapping back to full width at one particular size.
fn seamed(width: f64, seam: f64, floor: f64) -> f64 {
    width.min(floor).max(width - seam)
}

/// Inset a pixel span by the seam, keeping its centre fixed. For the linear view, which measures
/// features in screen x.
pub fn seam_span(x0: f64, x1: f64) -> (f64, f64) {
    let lo = x0.min(x1);
    let hi = x0.max(x1);
    let w = seamed(hi - lo, SEAM_PX, MIN_MARK_PX);
    let pad = (hi - lo - w) / 2.0;
    (lo + pad, hi - pad)
}

/// A span to draw, in fractions of a turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcSeam {
    pub draw_start: f64,
    pub draw_width: f64,
}

/// The same inset in fractions of a turn, for the ring and the morph.
///
/// `seam_frac` and `floor_frac` are [`SEAM_PX`] and [`MIN_MARK_PX`] already converted to fractions
/// at the caller's radius: see the note at the top on why that conversion cannot live here.
pub fn arc_seam(draw_start: f64, draw_width: f64, seam_frac: f64, floor_frac: f64) -> ArcSeam {
    let w = seamed(draw_width, seam_frac, floor_frac);
    ArcSeam { draw_start: draw_start + (draw_width - w) / 2.0, draw_width: w }
}

/// The span to DRAW, plus whether it had to be widened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcMark {
    pub mark: bool,
    pub draw_start: f64,
    pub draw_width: f64,
}

/// The mark floor in fractions of a turn, for the ring and the morph.
///
/// `floor_frac` is [`MIN_MARK_PX`] already converted to a fraction at the caller's radius: see the
/// note at the top on why that conversion cannot live here. Returns the span to DRAW plus whether
/// it had to be widened, so the caller can style an approximation as one.
pub fn arc_mark(start_frac: f64, width_frac: f64, floor_frac: f64) -> ArcMark {
    // Only a width that IS below the floor is widened; one that cannot be compared is left alone.
    if width_frac < floor_frac {
        // Centred, for the same reason `widen_to_mark` is.
        let mid = start_frac + width_frac / 2.0;
        return ArcMark { mark: true, draw_start: mid - floor_frac / 2.0, draw_width: floor_frac };
    }
    ArcMark { mark: false, draw_start: start_frac, draw_width: width_frac }
}
