//! Turning the abstract viewport (`{center, width}` in bp-fractions) into pixels. Ported from Bios
//! `editor/overview/projection.ts`. The two projections share the model and differ ONLY here: a
//! ring and a line are the same window drawn two ways.
//!
//! RING: a "window onto a growing circle." At width=1 the whole ring fits the box (radius R0). As
//! you zoom in, the radius grows (R = R0/width) and the circle's center slides downward off-screen,
//! so the top arc, the part you're looking at, fills the frame and flattens. As it flattens it
//! also RISES to the vertical middle of the frame: at width=1 the arc rides the top (the whole ring
//! is centred); zoomed in it sits at H/2, where a near-flat line is actually usable instead of
//! pinned to the top edge. bp `center` always sits at 12 o'clock; rotating the map moves `center`.
//! This is monotonic and continuous, so zoom never jumps.
//!
//! LINE: the window [center±width/2] mapped straight across the frame.
//!
//! Angles: θ(frac) = (frac - center)·2π, measured clockwise from the top. A point at θ is
//! (cx + R·sinθ, cyc − R·cosθ): θ=0 → top, θ=π/2 → 3 o'clock. That matches how bp numbers run
//! clockwise on every plasmid map.

use super::viewport::{MIN_WIDTH, Viewport, clamp, js_number, js_round, ring_delta, wrap1};

pub const TAU: f64 = std::f64::consts::PI * 2.0;

// ---- MORPH (circle → line as you zoom) ----
//
// Straightness of the map at a given zoom: 0 = full circle, 1 = straight line. Between MORPH_LINE
// and MORPH_CIRC the circle smoothly UNROLLS into the line (smoothstep easing), so ring→line is one
// continuous transform, not a hard swap. Only circular molecules morph; a linear one is always 1.
/// At/above this visible width: still a pure circle.
pub const MORPH_CIRC: f64 = 0.62;
/// At/below this visible width: fully a straight line.
pub const MORPH_LINE: f64 = 0.3;

pub fn straightness(width: f64) -> f64 {
    if width >= MORPH_CIRC {
        return 0.0;
    }
    if width <= MORPH_LINE {
        return 1.0;
    }
    let u = (MORPH_CIRC - width) / (MORPH_CIRC - MORPH_LINE);
    u * u * (3.0 - 2.0 * u) // smoothstep: eases in and out so the unroll has no hard start/stop
}

/// A bare position in the frame, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Where a base sits mid-morph: its true position on the ring, lerped toward where it lands on the
/// straightened line by `t`. The line target is measured the SHORT way around from `center`
/// ([`ring_delta`]), so both halves of the ring unroll off the two ends symmetrically instead of one
/// side whipping across the frame. Off-window bases get line targets beyond the frame and the
/// drawing clips them, which reads exactly as the far side of the ring straightening and sliding away.
///
/// `lane_depth` (0 when `None`) is how deep into the nested feature lanes this point sits, in
/// pixels. On the ring that means a smaller radius; on the line it means further ABOVE the axis,
/// because the line puts the molecule's own axis at the bottom and stacks annotations up from it:
/// the unrolled form of the ring putting backbone, scale and cut sites on the OUTSIDE with features
/// nesting inward. The sign has to match at both ends or a nested feature flips sides the instant
/// the morph finishes; carrying it through the lerp is what keeps a nested feature nested for the
/// whole length of the unroll instead of snapping into place at one end.
///
/// `axis_y` is where the line's axis sits: the middle (`h / 2`) when `None`, for callers that
/// don't place one.
// The parameter list is the original's, kept whole so the two files can be read side by side.
#[allow(clippy::too_many_arguments)]
pub fn morph_point(
    frac: f64,
    center: f64,
    width: f64,
    t: f64,
    w: f64,
    h: f64,
    geom: RingGeom,
    margin_x: f64,
    lane_depth: Option<f64>,
    axis_y: Option<f64>,
) -> Point {
    let lane_depth = lane_depth.unwrap_or(0.0);
    let axis_y = axis_y.unwrap_or(h / 2.0);
    let r = ring_point(frac, center, geom, Some(geom.r - lane_depth));
    let d = ring_delta(center, frac); // (-0.5, 0.5], signed short-way distance
    let x_line = w / 2.0 + (d / width) * (w - 2.0 * margin_x);
    let y_line = axis_y - lane_depth;
    Point { x: r.x + (x_line - r.x) * t, y: r.y + (y_line - r.y) * t }
}

// ---- RING ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingGeom {
    pub cx: f64,
    /// Circle CENTER y (slides below the frame as you zoom in).
    pub cyc: f64,
    /// Current backbone radius.
    pub r: f64,
    /// Radius at width=1 (the fit radius).
    pub r0: f64,
}

// The band kept clear at the top, so the fit-ring's top arc and the labels just outside it never
// rise into the construct's name in the top-left corner: a ring arc rising into a 15px label reads
// as the label being part of the drawing. Two lines of chrome plus air.
const TOP_RESERVE: f64 = 56.0;

// Nothing sits on the right edge, so this is a plain margin, not a reservation. It does not go to
// zero: `ring_geom`'s `m` is proportional (`max(28, min(W,H) * 0.15)`) and collapses to 28px on a
// narrow pane, so this stays as the floor that does not move with the window.
const RIGHT_RESERVE: f64 = 12.0;

/// The mirror of `TOP_RESERVE` at the other end: the bottom margin the map keeps clear inside its
/// own frame, so a label pushed to the floor of the frame still has air under it. The navigator
/// takes its own height below the map and nothing is drawn under it, so this is a margin, not a
/// reservation.
pub const NAV_RESERVE: f64 = 14.0;

pub fn ring_geom(width: f64, w: f64, h: f64, margin: Option<f64>) -> RingGeom {
    // Give the fit-ring real breathing room instead of a flat 28px, so the whole-plasmid view isn't
    // pressed against the frame edges: a proportional margin keeps that air at any pane size.
    let m = margin.unwrap_or_else(|| 28f64.max(w.min(h) * 0.15));
    // Cap the radius so the top arc (y = H/2 − R0 at full view) stays below the name label, and the
    // right arc (x = W/2 + R0) keeps a margin off the frame.
    let r0 = 1f64.max((w.min(h) / 2.0 - m).min(h / 2.0 - TOP_RESERVE).min(w / 2.0 - RIGHT_RESERVE));
    let width = clamp(width, MIN_WIDTH, 1.0);
    let r = r0 / width;
    // The viewed arc (12 o'clock) sits at y = cyc - R. Ride the top at full view (width=1 → y =
    // H/2 - R0, the whole ring centred) and migrate to the vertical middle as you zoom in (width→0 →
    // y = H/2). Zooming still slides the circle centre downward off-screen, just faster than the arc.
    let cyc = h / 2.0 - r0 * width + r;
    RingGeom { cx: w / 2.0, cyc, r, r0 }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub theta: f64,
}

/// Where a bp fraction sits on the ring. `radius` is the backbone's (`geom.r`) when `None`.
pub fn ring_point(frac: f64, center: f64, geom: RingGeom, radius: Option<f64>) -> Pt {
    let radius = radius.unwrap_or(geom.r);
    let theta = (frac - center) * TAU;
    Pt { x: geom.cx + radius * theta.sin(), y: geom.cyc - radius * theta.cos(), theta }
}

/// SVG path for an arc from `start_frac`→`end_frac` at `radius`. Callers pass non-wrapping pieces
/// (see `split_span`), so the sweep is just the sign of the span.
pub fn ring_arc_path(start_frac: f64, end_frac: f64, center: f64, geom: RingGeom, radius: f64) -> String {
    let p0 = ring_point(start_frac, center, geom, Some(radius));
    let p1 = ring_point(end_frac, center, geom, Some(radius));
    let d_theta = (end_frac - start_frac) * TAU;
    let large_arc = if d_theta.abs() > std::f64::consts::PI { 1 } else { 0 };
    let sweep = if d_theta >= 0.0 { 1 } else { 0 };
    format!(
        "M {} {} A {} {} 0 {large_arc} {sweep} {} {}",
        js_number(round(p0.x)),
        js_number(round(p0.y)),
        js_number(round(radius)),
        js_number(round(radius)),
        js_number(round(p1.x)),
        js_number(round(p1.y)),
    )
}

/// A cursor position on the ring → the bp fraction under it (for click-to-centre / rotate).
pub fn point_to_frac_ring(px: f64, py: f64, center: f64, geom: RingGeom) -> f64 {
    let theta = (px - geom.cx).atan2(geom.cyc - py); // inverse of (sinθ, −cosθ)
    wrap1(center + theta / TAU)
}

// ---- LINE ----

/// The line's horizontal inset, both sides. Public because anything drawn IN REGISTER with the
/// line has to inset by the same number, and a copy of it is a coordinate bug waiting for its
/// first edit.
pub const LINE_MARGIN: f64 = 44.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineWindow {
    pub left: f64,
    pub right: f64,
}

/// The on-screen window as fractions, clamped so a line never overhangs an end.
pub fn line_window(vp: Viewport) -> LineWindow {
    let left = clamp(vp.center - vp.width / 2.0, 0.0, 0f64.max(1.0 - vp.width));
    LineWindow { left, right: left + vp.width }
}

pub fn line_x(frac: f64, left: f64, width: f64, w: f64, margin_x: f64) -> f64 {
    margin_x + ((frac - left) / width) * (w - 2.0 * margin_x)
}

pub fn point_to_frac_line(px: f64, left: f64, width: f64, w: f64, margin_x: f64) -> f64 {
    left + ((px - margin_x) / (w - 2.0 * margin_x)) * width
}

fn round(n: f64) -> f64 {
    js_round(n * 100.0) / 100.0
}
