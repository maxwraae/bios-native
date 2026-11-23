//! The one viewport model, shared by both projections. Ported from Bios `editor/overview/viewport.ts`.
//!
//! A plasmid isn't two different things (a ring and a line): it's one sequence looked at through
//! one window, `{center, width}` in bp-FRACTIONS (0..1), drawn either bent into a circle or laid
//! out straight. Everything the user does to zoom or pan is a pure function on this object, and
//! because the map holds it, zoom-out can never strand: it just grows `width` back toward 1.
//!
//! Coordinates are 0-based inclusive: a span from start..end is (end - start + 1) bases, and an
//! origin-spanning span has end < start.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    Ring,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// bp fraction (0..1) sitting at the middle of the view. On a ring this wraps; on a line it's clamped.
    pub center: f64,
    /// Fraction of the whole molecule visible (0<..1). 1 = the entire construct in frame.
    pub width: f64,
    pub projection: Projection,
}

// How far you can zoom IN on the overview before it stops being an overview. Kept as a fraction
// floor; the hand-off to the row view trips well before this on real constructs.
pub const MIN_WIDTH: f64 = 0.01;
pub const MAX_WIDTH: f64 = 1.0;

// Zoom owns the projection: once a circular map is zoomed in past RING_TO_LINE (fraction of the
// molecule in view), a near-flat arc reads worse than a straight line, so it flips ring→line, and
// back to a ring once you zoom out past LINE_TO_RING. The gap between the two is hysteresis: it
// stops the projection flickering when you sit right at the boundary. A LINEAR molecule is always a
// line, at every zoom. This makes "become linear as you zoom in" a consequence of altitude, not a
// mode you set.
pub const RING_TO_LINE: f64 = 0.6;
pub const LINE_TO_RING: f64 = 0.7;

pub fn projection_for_width(width: f64, circular: bool, current: Projection) -> Projection {
    if !circular {
        return Projection::Line;
    }
    if current == Projection::Ring {
        return if width < RING_TO_LINE { Projection::Line } else { Projection::Ring };
    }
    if width > LINE_TO_RING { Projection::Ring } else { Projection::Line }
}

/// `max(lo, min(hi, v))`, in that order: when `lo > hi` the answer is `lo`. Not `f64::clamp`,
/// which panics on exactly that input, and a line window wider than the molecule produces it.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    lo.max(hi.min(v))
}

/// [`clamp`] for base positions, with the same `lo` wins rule.
fn clamp_bp(v: i64, lo: i64, hi: i64) -> i64 {
    lo.max(hi.min(v))
}

/// Wrap a fraction into [0,1).
pub fn wrap1(f: f64) -> f64 {
    ((f % 1.0) + 1.0) % 1.0
}

/// Signed fractional delta a→b on a ring, in (-0.5, 0.5]: the short way round. Used so panning
/// and anchoring across the origin take the nearest path instead of unwinding the long way.
pub fn ring_delta(a: f64, b: f64) -> f64 {
    let mut d = wrap1(b - a);
    if d > 0.5 {
        d -= 1.0;
    }
    d
}

/// A ring center is free (wraps); a line center is clamped so the window can't fall off either end.
pub fn clamp_center(center: f64, width: f64, projection: Projection) -> f64 {
    if projection == Projection::Ring {
        return wrap1(center);
    }
    let half = width / 2.0;
    clamp(center, half, 1.0 - half)
}

pub fn set_center(vp: Viewport, frac: f64) -> Viewport {
    Viewport { center: clamp_center(frac, vp.width, vp.projection), ..vp }
}

pub fn set_width(vp: Viewport, width: f64) -> Viewport {
    let w = clamp(width, MIN_WIDTH, MAX_WIDTH);
    // Re-clamp the center: a line window that was valid at the old width may now overhang an end.
    Viewport { width: w, center: clamp_center(vp.center, w, vp.projection), ..vp }
}

/// Pan by a signed fraction of the sequence. Ring wraps; line clamps (via [`clamp_center`]).
pub fn pan_by(vp: Viewport, d_frac: f64) -> Viewport {
    set_center(vp, vp.center + d_frac)
}

/// Multiply the visible width by `factor` (<1 zooms in, >1 out). If `anchor_frac` is given on a
/// LINE, the bp under that anchor stays put on screen (cursor-anchored zoom); a RING zooms about
/// its center (rotation is how you bring a locus to the top, which is how a plasmid map feels).
pub fn zoom_by(vp: Viewport, factor: f64, anchor_frac: Option<f64>) -> Viewport {
    let width = clamp(vp.width * factor, MIN_WIDTH, MAX_WIDTH);
    let anchor_frac = match anchor_frac {
        Some(anchor_frac) if vp.projection != Projection::Ring => anchor_frac,
        _ => return Viewport { width, center: clamp_center(vp.center, width, vp.projection), ..vp },
    };
    // Keep the anchored bp at the same on-screen position s∈[0,1] across the window.
    let left = vp.center - vp.width / 2.0;
    let s = if vp.width > 0.0 { clamp((anchor_frac - left) / vp.width, 0.0, 1.0) } else { 0.5 };
    let center = anchor_frac - s * width + width / 2.0;
    Viewport { width, center: clamp_center(center, width, Projection::Line), ..vp }
}

/// Switch projection, preserving where you're looking (center/width), re-clamping for the line.
pub fn set_projection(vp: Viewport, projection: Projection) -> Viewport {
    Viewport { projection, center: clamp_center(vp.center, vp.width, projection), ..vp }
}

/// The bp span currently in frame, 0-based inclusive, wrap-aware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleRange {
    pub start_bp: i64,
    pub end_bp: i64,
    /// How many bases are visible (drives the row-view hand-off threshold and tick density).
    pub span: i64,
    /// The window straddles the origin (only possible on a ring).
    pub wraps: bool,
}

pub fn visible_range(vp: Viewport, l: i64) -> VisibleRange {
    let span = clamp_bp(js_round(vp.width * l as f64) as i64, 1, l);
    let left_frac =
        if vp.projection == Projection::Ring { wrap1(vp.center - vp.width / 2.0) } else { clamp(vp.center - vp.width / 2.0, 0.0, 1.0 - vp.width) };
    let start_bp = clamp_bp(js_round(left_frac * l as f64) as i64, 0, 0.max(l - 1));
    // An empty construct has nothing to wrap round. The original's `% 0` is NaN there; a remainder
    // by zero would panic here, so the range stays the one base its `span` of 1 claims.
    let end_bp = (start_bp + span - 1).checked_rem(l).unwrap_or(start_bp);
    VisibleRange { start_bp, end_bp, span, wraps: end_bp < start_bp }
}

/// One on-screen piece of a span, in fractions of the molecule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub start_frac: f64,
    pub width_frac: f64,
    /// True for the piece that ends at the origin seam (for drawing a clean cut).
    pub cut_right: bool,
    /// True for the piece that begins after the origin seam.
    pub cut_left: bool,
}

/// Split a 0-based-inclusive span into on-screen pieces. One piece normally; two when it crosses
/// the origin (end < start). The single source of the origin-wrap rule: features, the minimap
/// window, and selection all go through it.
pub fn split_span(start: i64, end: i64, l: i64) -> Vec<Piece> {
    if l <= 0 {
        return Vec::new();
    }
    let lf = l as f64;
    if end >= start {
        return vec![Piece { start_frac: start as f64 / lf, width_frac: (end - start + 1) as f64 / lf, cut_right: false, cut_left: false }];
    }
    vec![
        Piece { start_frac: start as f64 / lf, width_frac: (l - start) as f64 / lf, cut_right: true, cut_left: false },
        Piece { start_frac: 0.0, width_frac: (end + 1) as f64 / lf, cut_right: false, cut_left: true },
    ]
}

/// One rectangle of the minimap window, in fraction space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPiece {
    pub left: f64,
    pub width: f64,
    pub left_edge: bool,
    pub right_edge: bool,
    pub cut_left: bool,
    pub cut_right: bool,
}

/// The minimap window as 1 or 2 rectangles in fraction space: a circular window that straddles
/// the origin is drawn as two seam-cut pieces so it reads as one wrapped band.
pub fn window_pieces(center: f64, width: f64, wrap: bool) -> Vec<WindowPiece> {
    if width >= 1.0 {
        return vec![WindowPiece { left: 0.0, width: 1.0, left_edge: false, right_edge: false, cut_left: false, cut_right: false }];
    }
    if !wrap {
        let left = clamp(center - width / 2.0, 0.0, 1.0 - width);
        return vec![WindowPiece { left, width, left_edge: true, right_edge: true, cut_left: false, cut_right: false }];
    }
    let l = wrap1(center - width / 2.0);
    let r = l + width;
    if r <= 1.0 {
        return vec![WindowPiece { left: l, width, left_edge: true, right_edge: true, cut_left: false, cut_right: false }];
    }
    vec![
        WindowPiece { left: l, width: 1.0 - l, left_edge: true, right_edge: false, cut_left: false, cut_right: true },
        WindowPiece { left: 0.0, width: r - 1.0, left_edge: false, right_edge: true, cut_left: true, cut_right: false },
    ]
}

/// Evenly spaced tick positions (in bp) at a "nice" 1/2/5×10ⁿ step, ~4 across the whole molecule.
/// The minimap and the overview share it so they agree. Fractional only below 4 bp.
pub fn nice_ticks(l: i64) -> Vec<f64> {
    if l <= 0 {
        return Vec::new();
    }
    let lf = l as f64;
    let target = lf / 4.0;
    let pow = 10f64.powf(target.log10().floor());
    let mut step = 10.0 * pow;
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * pow >= target {
            step = m * pow;
            break;
        }
    }
    let mut ticks = Vec::new();
    let mut x = step;
    while x < lf {
        ticks.push(x);
        x += step;
    }
    ticks
}

// The two row-view hand-off inversions: given our width, what row-view character width shows the
// same span, and back. `client_px` is the pane's content width. Kept here so the round-trip is
// unit-tested next to the model it belongs to.
pub fn char_width_for_width(width: f64, client_px: f64, l: i64) -> f64 {
    let visible_bp = 1f64.max(width * l as f64);
    client_px / visible_bp
}

pub fn width_for_char_width(char_width: f64, client_px: f64, l: i64) -> f64 {
    if char_width <= 0.0 || l <= 0 {
        return 1.0;
    }
    let visible_bp = client_px / char_width;
    clamp(visible_bp / l as f64, MIN_WIDTH, MAX_WIDTH)
}

// ---- Two JavaScript behaviours the map's arithmetic rests on ----

/// JavaScript's `Math.round`: a half rounds toward +∞ (`-2.5` gives `-2`), where `f64::round`
/// rounds it away from zero.
pub(crate) fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// A number as JavaScript prints it inside a template string, which is what the path strings of
/// the original are made of: the shortest digits that read back as the same number, no `-0`, and
/// an exponent only at 1e21 and above or below 1e-6 (ECMAScript `Number::toString`).
pub(crate) fn js_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n == 0.0 {
        return "0".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() };
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let (digits, exponent) = shortest_digits(n.abs());
    let count = digits.len() as i32;
    // How many of the digits sit before the decimal point.
    let point = exponent + 1;
    if (count..=21).contains(&point) {
        format!("{sign}{digits}{}", "0".repeat((point - count) as usize))
    } else if (1..=21).contains(&point) {
        let (whole, part) = digits.split_at_checked(point as usize).unwrap_or((&digits, ""));
        format!("{sign}{whole}.{part}")
    } else if (-5..=0).contains(&point) {
        format!("{sign}0.{}{digits}", "0".repeat(-point as usize))
    } else {
        let (first, rest) = digits.split_at_checked(1).unwrap_or((&digits, ""));
        let dot = if rest.is_empty() { "" } else { "." };
        let mark = if exponent < 0 { '-' } else { '+' };
        format!("{sign}{first}{dot}{rest}e{mark}{}", exponent.abs())
    }
}

/// The shortest run of digits that reads back as `size` (finite and above zero), and the power of
/// ten of the first of them.
fn shortest_digits(size: f64) -> (String, i32) {
    let (mut digits, exponent) = digits_of(&format!("{size:e}"));
    // Two candidates can be equally near, which is when the number is exactly the lower one
    // followed by a 5. JavaScript then takes the one ending in an even digit; Rust takes the upper.
    if let Some(last) = digits.chars().last().and_then(|c| c.to_digit(10)).filter(|last| last % 2 == 1) {
        let mut tie = digits.clone();
        tie.pop();
        tie.extend(char::from_digit(last - 1, 10));
        let lower = tie.clone();
        tie.push('5');
        // No f64 has more than 767 significant decimal digits, so 800 places is the number exactly.
        // Printing one digit past the shortest first spares that work for a number that cannot tie.
        let prints_as_tie = |places: usize| digits_of(&format!("{size:.places$e}")).0.trim_end_matches('0') == tie;
        if prints_as_tie(digits.len()) && prints_as_tie(800) {
            digits = lower;
        }
    }
    (digits, exponent)
}

/// The digits and the exponent of a number printed as `d.ddde±x`.
fn digits_of(scientific: &str) -> (String, i32) {
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific, "0"));
    (mantissa.chars().filter(char::is_ascii_digit).collect(), exponent.parse().unwrap_or(0))
}
