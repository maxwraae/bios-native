//! A RELATIONSHIP DRAWN ACROSS THE RING'S INTERIOR. Ported from Bios `editor/overview/chords.ts`.
//!
//! Everything else on the map is at or outside the backbone: the lanes, the cut ticks, the
//! coordinate numbers, the label columns. The centre is empty, deliberately so: a leader-line
//! design that crossed it was deleted for being unreadable. So a chord is the one thing that
//! belongs in there, and it is the only drawing on this map that is ABOUT two features rather than
//! about one.
//!
//! It bends toward the centre rather than running straight, for the reason every genome browser
//! bends them: three chords between neighbouring features drawn straight are three overlapping
//! lines, and the same three bent are three distinguishable arcs. The bend is proportional, so a
//! chord across the plasmid dips deep and a chord between neighbours barely leaves the ring.

use super::viewport::js_number;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

/// How far toward the ring's centre a chord is pulled. 0 draws a straight line.
pub const BEND: f64 = 0.55;

/// The point the curve is pulled toward: the midpoint of the two ends, moved toward the centre.
/// `bend` is [`BEND`] when `None`.
///
/// Also where the verb is written, which is why it is public rather than hidden inside the path.
pub fn chord_apex(a: Pt, b: Pt, cx: f64, cy: f64, bend: Option<f64>) -> Pt {
    let bend = bend.unwrap_or(BEND);
    let mx = (a.x + b.x) / 2.0;
    let my = (a.y + b.y) / 2.0;
    Pt { x: cx + (mx - cx) * (1.0 - bend), y: cy + (my - cy) * (1.0 - bend) }
}

/// One quadratic through the interior, from `a` to `b`. `bend` is [`BEND`] when `None`.
pub fn chord_path(a: Pt, b: Pt, cx: f64, cy: f64, bend: Option<f64>) -> String {
    let c = chord_apex(a, b, cx, cy, bend);
    format!("M {} {} Q {} {} {} {}", js_number(a.x), js_number(a.y), js_number(c.x), js_number(c.y), js_number(b.x), js_number(b.y))
}

/// An arrowhead sitting AT `tip`, pointing away from `from`, so it lands on the thing being acted
/// on, whatever direction the chord arrived from. `size` is 6 when `None`.
///
/// Degenerate input (the two points coincident) gets a head pointing straight up rather than a
/// NaN path that silently draws nothing.
pub fn arrow_head(tip: Pt, from: Pt, size: Option<f64>) -> String {
    let [t, l, r] = arrow_head_points(tip, from, size.unwrap_or(6.0));
    format!("M {} {} L {} {} L {} {} Z", js_number(t.x), js_number(t.y), js_number(l.x), js_number(l.y), js_number(r.x), js_number(r.y))
}

/// The three corners of [`arrow_head`]: the tip, then the two back corners.
pub fn arrow_head_points(tip: Pt, from: Pt, size: f64) -> [Pt; 3] {
    let mut dx = tip.x - from.x;
    let mut dy = tip.y - from.y;
    let len = dx.hypot(dy);
    if len < 0.001 {
        dx = 0.0;
        dy = -1.0;
    } else {
        dx /= len;
        dy /= len;
    }
    // Perpendicular, for the two back corners.
    let px = -dy;
    let py = dx;
    let bx = tip.x - dx * size;
    let by = tip.y - dy * size;
    let half = size * 0.45;
    [tip, Pt { x: bx + px * half, y: by + py * half }, Pt { x: bx - px * half, y: by - py * half }]
}

/// The middle of a feature as a fraction of the molecule, wrap included.
///
/// A feature that crosses the origin has `end` before `start`, and averaging those lands on the
/// wrong side of the plasmid: the one case where the obvious arithmetic is silently wrong.
pub fn mid_frac(start: i64, end: i64, l: i64) -> f64 {
    if l <= 0 {
        return 0.0;
    }
    let lf = l as f64;
    let len = if end >= start { end - start } else { l - start + end };
    let mid = (start as f64 + len as f64 / 2.0) % lf;
    (mid + lf) % lf / lf
}
