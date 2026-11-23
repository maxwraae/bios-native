//! What a hand does to the one viewport: the map style's zoom jump, the wheel and pinch on the map,
//! and the navigator's rail, window and grips. Ported from Bios `editor/overview/useViewport.ts`
//! (the commit rule and the ring/line jump), `editor/overview/PlasmidOverview.tsx` lines 933-986
//! (the map's wheel) and `editor/overview/MapMinimap.tsx` (the rail's wheel and drags).
//!
//! A zoom does not jump the width: it sets a TARGET the view eases toward every frame
//! ([`zoom_toward`], [`ease_zoom`]), so a wheel tick or a pinch glides in and settles, and rapid
//! input accelerates. Because the map is drawn from the width every frame, easing the zoom is also
//! what makes the circle unroll into the line smoothly instead of stepping.

use super::projection::{line_window, point_to_frac_line};
use super::viewport::{
    MAX_WIDTH, MIN_WIDTH, Projection, RING_TO_LINE, Viewport, clamp, clamp_center, pan_by, projection_for_width, set_center, set_projection,
    set_width,
};

/// The map's window before any file view is applied: a whole ring, origin at the top
/// (`useViewport`'s first state under the editor's default `mapStyle: "circular"`).
pub const OPENING: Viewport = Viewport { center: 0.0, width: 1.0, projection: Projection::Ring };

/// Let the width decide ring or line (`useViewport` `normalize`). `ring` is the map style's
/// choice: picking linear stays a line at every zoom.
pub fn normalize(v: Viewport, ring: bool) -> Viewport {
    let projection = projection_for_width(v.width, ring, v.projection);
    if projection == v.projection { v } else { set_projection(v, projection) }
}

/// The ring and line buttons as zoom jumps (`useViewport`'s projection effect). Ring shows the
/// whole plasmid; line drops to a reading altitude, 0.4 of the molecule, unless already closer.
///
/// This is why a long linear construct opens zoomed: the map starts as a whole ring at the origin
/// ([`OPENING`]), the file's linear style turns it to a line 0.4 wide, and the centre, clamped onto
/// the line, puts the window at the start (a 6,093-base linear construct opens on 1 – 2,437).
pub fn style_jump(v: Viewport, ring: bool) -> Viewport {
    if ring {
        return normalize(set_width(Viewport { projection: Projection::Ring, ..v }, 1.0), ring);
    }
    let w = if v.width > RING_TO_LINE { 0.4 } else { v.width };
    normalize(set_width(Viewport { projection: Projection::Line, ..v }, w), ring)
}

/// The zoom factor one wheel event asks for: `exp(dy · 0.02)`, `dy` held to ±30, so a mouse
/// wheel's few big notches and a trackpad's stream of small ones travel alike.
pub fn wheel_factor(dy: f64) -> f64 {
    (clamp(dy, -30.0, 30.0) * 0.02).exp()
}

/// Pinch or ⌘-wheel on the map, where one step settles. On a line the base under the pointer `x`
/// stays under it; a ring zooms about its centre. `w` is the map's width and `margin` the line's
/// side margin. The map itself eases there ([`zoom_toward`] with [`wheel_anchor`]).
pub fn map_zoom(v: Viewport, dy: f64, x: f64, w: f64, margin: f64, ring: bool) -> Viewport {
    let width = clamp(v.width * wheel_factor(dy), MIN_WIDTH, MAX_WIDTH);
    let Some((anchor, s)) = wheel_anchor(v, x, w, margin) else {
        return normalize(set_width(v, width), ring);
    };
    let center = anchor - s * width + width / 2.0;
    normalize(set_center(Viewport { width, ..v }, center), ring)
}

/// What a zoom over the map holds still: on a line, the base under the pointer `x` and where across
/// the line it sits (0 the left margin, 1 the right); on a ring nothing, it zooms about its centre.
pub fn wheel_anchor(v: Viewport, x: f64, w: f64, margin: f64) -> Option<(f64, f64)> {
    if v.projection != Projection::Line {
        return None;
    }
    let win = line_window(v);
    let anchor = point_to_frac_line(x, win.left, v.width, w, margin);
    Some((anchor, clamp((x - margin) / 1f64.max(w - 2.0 * margin), 0.0, 1.0)))
}

// ── THE EASED ZOOM (`useViewport.ts`) ──────────────────────────────────────────────────────────

/// Easing rate (1/seconds). Higher = snappier. At 22 the view reaches its target in ~180ms: quick
/// enough that the ring→line unroll feels driven, not sluggish, but still eased so it glides in and
/// settles rather than snapping.
pub const ZOOM_RATE: f64 = 22.0;

/// Where an eased zoom is headed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomTarget {
    pub width: f64,
    /// The base held at `s` across a line while the width comes in; `None` zooms about the centre.
    pub anchor_frac: Option<f64>,
    pub s: f64,
}

/// Multiply the zoom target by `factor` (<1 in, >1 out). Accumulates against the live target, when
/// there is one, so rapid input accelerates; the view eases toward it. `anchor` is
/// [`wheel_anchor`]'s, and `min_width` the narrowest window allowed.
pub fn zoom_toward(v: Viewport, live: Option<ZoomTarget>, factor: f64, anchor: Option<(f64, f64)>, min_width: f64) -> ZoomTarget {
    let base = live.map_or(v.width, |t| t.width);
    let width = clamp(base * factor, min_width, MAX_WIDTH);
    ZoomTarget { width, anchor_frac: anchor.map(|a| a.0), s: anchor.map_or(0.5, |a| a.1) }
}

/// The seconds one frame of easing covers: a first frame counts as 1/60, and a long stall as no more
/// than 50 ms, so a hitch does not leap the view. `last` is the previous frame's time, `now` this one's.
pub fn frame_dt(last: Option<f64>, now: f64) -> f64 {
    match last {
        None => 1.0 / 60.0,
        Some(last) => 0.05f64.min((now - last).max(0.0)),
    }
}

/// One frame of the eased zoom, `dt` seconds long: the view and whether it has arrived. A
/// frame-rate-independent exponential ease-out; within 0.3% of the target it lands on it exactly.
/// Anchored on a line, the anchored base stays under the same on-screen spot as the width changes.
pub fn ease_zoom(v: Viewport, t: ZoomTarget, dt: f64, ring: bool) -> (Viewport, bool) {
    let k = 1.0 - (-ZOOM_RATE * dt).exp();
    let mut width = v.width + (t.width - v.width) * k;
    let done = (width - t.width).abs() <= t.width * 0.003;
    if done {
        width = t.width;
    }
    let center = match t.anchor_frac {
        Some(anchor) if v.projection == Projection::Line => anchor - t.s * width + width / 2.0,
        _ => v.center,
    };
    (normalize(Viewport { width, center: clamp_center(center, width, v.projection), ..v }, ring), done)
}

/// Plain scroll on the map: slide the line, turn the ring. `delta` is the dominant axis. Pan
/// distance scales with the visible width so it feels the same at every zoom; the ring has a floor
/// so a full turn stays reachable when zoomed in.
pub fn map_pan(v: Viewport, delta: f64, w: f64, ring: bool) -> Viewport {
    let w = if w > 0.0 { w } else { 1.0 };
    let d = if v.projection == Projection::Line { (delta / w) * v.width } else { (delta / w) * 0.25f64.max(v.width) };
    normalize(pan_by(v, d), ring)
}

/// The dominant axis of a scroll: horizontal when it is the larger.
pub fn dominant(dx: f64, dy: f64) -> f64 {
    if dx.abs() > dy.abs() { dx } else { dy }
}

/// A press on the rail at fraction `at` of its width sends the window there.
pub fn rail_press(v: Viewport, at: f64, ring: bool) -> Viewport {
    normalize(set_center(v, at), ring)
}

/// A wheel over the rail. A dominant sideways scroll pans; otherwise it zooms about the base under
/// the pointer, at fraction `at` of the rail, in the rail's own space.
pub fn rail_wheel(v: Viewport, dx: f64, dy: f64, at: f64, rail_w: f64, ring: bool) -> Viewport {
    let px = if rail_w > 0.0 { rail_w } else { 1.0 };
    if dx.abs() > dy.abs() {
        return normalize(set_center(v, v.center + (dx / px) * v.width), ring);
    }
    let width = clamp(v.width * wheel_factor(dy), MIN_WIDTH, MAX_WIDTH);
    let at = clamp(at, 0.0, 1.0);
    let k = if v.width > 0.0 { width / v.width } else { 1.0 };
    normalize(set_center(Viewport { width, ..v }, at + (v.center - at) * k), ring)
}

/// What a press on the navigator's window took hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// The window's body: it follows the pointer 1:1.
    Move,
    /// An end: that end moves and the other stays put, which is a zoom.
    Left,
    Right,
}

/// A drag on the window, `dx` the travel since the press as a fraction of the whole rail, from the
/// centre and width the press found.
pub fn rail_drag(v: Viewport, grip: Grip, dx: f64, start_center: f64, start_width: f64, ring: bool) -> Viewport {
    match grip {
        Grip::Move => normalize(set_center(v, start_center + dx), ring),
        Grip::Left | Grip::Right => {
            let signed = if grip == Grip::Right { dx } else { -dx };
            let width = clamp(start_width + signed, MIN_WIDTH, MAX_WIDTH);
            normalize(set_center(Viewport { width, ..v }, start_center + dx / 2.0), ring)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::viewport::visible_range;
    use super::*;

    #[test]
    fn a_linear_construct_opens_on_its_first_four_tenths() {
        let v = style_jump(OPENING, false);
        assert_eq!(v.projection, Projection::Line);
        assert!((v.width - 0.4).abs() < 1e-12);
        let r = visible_range(v, 6093);
        // The web's navigator reads "1 – 2,437 · 2.4 kb" for a 6,093-base linear construct on open.
        assert_eq!((r.start_bp + 1, r.end_bp + 1, r.span), (1, 2437, 2437));
    }

    #[test]
    fn a_circular_construct_opens_whole() {
        assert_eq!(style_jump(OPENING, true), OPENING);
    }

    #[test]
    fn the_line_button_keeps_a_closer_zoom() {
        let v = Viewport { center: 0.5, width: 0.1, projection: Projection::Line };
        assert_eq!(style_jump(v, false).width, 0.1);
        // and the ring button always fits the whole ring
        let back = style_jump(v, true);
        assert_eq!((back.width, back.projection), (1.0, Projection::Ring));
    }

    #[test]
    fn a_line_zoom_keeps_the_base_under_the_pointer() {
        let (w, m) = (1000.0, 44.0);
        let v = Viewport { center: 0.5, width: 0.4, projection: Projection::Line };
        let x = 700.0;
        let win = line_window(v);
        let before = point_to_frac_line(x, win.left, v.width, w, m);
        let z = map_zoom(v, -30.0, x, w, m, false);
        assert!(z.width < v.width);
        let after = point_to_frac_line(x, line_window(z).left, z.width, w, m);
        assert!((before - after).abs() < 1e-9, "{before} vs {after}");
    }

    #[test]
    fn zoom_is_held_between_the_floor_and_the_whole() {
        let mut v = Viewport { center: 0.5, width: 0.4, projection: Projection::Line };
        for _ in 0..400 {
            v = map_zoom(v, -30.0, 300.0, 1000.0, 44.0, false);
        }
        assert!((v.width - MIN_WIDTH).abs() < 1e-12);
        for _ in 0..400 {
            v = map_zoom(v, 30.0, 300.0, 1000.0, 44.0, false);
        }
        assert_eq!(v.width, MAX_WIDTH);
    }

    #[test]
    fn a_ring_zoomed_in_far_enough_unrolls_and_comes_back() {
        let mut v = OPENING;
        for _ in 0..3 {
            v = map_zoom(v, -30.0, 0.0, 800.0, 44.0, true);
        }
        assert_eq!(v.projection, Projection::Line);
        for _ in 0..6 {
            v = map_zoom(v, 30.0, 0.0, 800.0, 44.0, true);
        }
        assert_eq!(v.projection, Projection::Ring);
    }

    /// Runs an ease to rest in frames of `dt`, returning the view and how long it took.
    fn settle(mut v: Viewport, t: ZoomTarget, dt: f64, ring: bool) -> (Viewport, f64) {
        let mut took = 0.0;
        for _ in 0..1000 {
            let (next, done) = ease_zoom(v, t, dt, ring);
            v = next;
            took += dt;
            if done {
                break;
            }
        }
        (v, took)
    }

    #[test]
    fn an_eased_zoom_glides_in_and_lands_exactly_on_its_target_in_about_180_ms() {
        let t = zoom_toward(OPENING, None, 0.5, None, MIN_WIDTH);
        assert_eq!(t.width, 0.5);
        let (first, done) = ease_zoom(OPENING, t, 1.0 / 60.0, true);
        assert!(!done && first.width < 1.0 && first.width > 0.5, "one frame is part of the way: {}", first.width);
        let (v, took) = settle(OPENING, t, 1.0 / 60.0, true);
        assert_eq!(v.width, 0.5);
        assert!((0.15..=0.3).contains(&took), "settled in {took} s");
    }

    #[test]
    fn the_ease_is_frame_rate_independent() {
        let t = ZoomTarget { width: 0.2, anchor_frac: None, s: 0.5 };
        let one = ease_zoom(OPENING, t, 0.02, true).0;
        let half = ease_zoom(OPENING, t, 0.01, true).0;
        let two = ease_zoom(half, t, 0.01, true).0;
        assert!((one.width - two.width).abs() < 1e-12, "{} vs {}", one.width, two.width);
    }

    #[test]
    fn rapid_zoom_input_accelerates_against_the_live_target() {
        let a = zoom_toward(OPENING, None, 0.8, None, MIN_WIDTH);
        let b = zoom_toward(OPENING, Some(a), 0.8, None, MIN_WIDTH);
        assert!((b.width - 0.64).abs() < 1e-12);
        let floor = zoom_toward(OPENING, Some(ZoomTarget { width: 0.011, ..a }), 0.5, None, MIN_WIDTH);
        assert_eq!(floor.width, MIN_WIDTH);
    }

    #[test]
    fn an_eased_line_zoom_keeps_the_base_under_the_pointer_every_frame() {
        let (w, m, x) = (1000.0, 44.0, 700.0);
        let mut v = Viewport { center: 0.5, width: 0.4, projection: Projection::Line };
        let anchor = wheel_anchor(v, x, w, m);
        let before = anchor.map(|a| a.0).unwrap_or(f64::NAN);
        let t = zoom_toward(v, None, 0.3, anchor, MIN_WIDTH);
        for _ in 0..30 {
            v = ease_zoom(v, t, 1.0 / 60.0, false).0;
            let after = point_to_frac_line(x, line_window(v).left, v.width, w, m);
            assert!((before - after).abs() < 1e-9, "{before} vs {after}");
        }
    }

    #[test]
    fn an_eased_zoom_unrolls_the_ring_into_a_line_on_the_way() {
        let t = zoom_toward(OPENING, None, 0.25, None, MIN_WIDTH);
        let (v, _) = settle(OPENING, t, 1.0 / 60.0, true);
        assert_eq!((v.width, v.projection), (0.25, Projection::Line));
        let back = settle(v, zoom_toward(v, None, 4.0, None, MIN_WIDTH), 1.0 / 60.0, true).0;
        assert_eq!((back.width, back.projection), (1.0, Projection::Ring));
    }

    #[test]
    fn a_frame_counts_a_sixtieth_first_and_never_more_than_50_ms() {
        assert_eq!(frame_dt(None, 3.0), 1.0 / 60.0);
        assert!((frame_dt(Some(3.0), 3.01) - 0.01).abs() < 1e-12);
        assert_eq!(frame_dt(Some(3.0), 9.0), 0.05);
    }

    #[test]
    fn scroll_slides_the_line_by_its_own_width_and_stops_at_the_end() {
        let v = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
        let p = map_pan(v, 100.0, 1000.0, false);
        assert!((p.center - 0.24).abs() < 1e-12);
        let end = map_pan(v, 1e6, 1000.0, false);
        assert!((end.center - 0.8).abs() < 1e-12);
    }

    #[test]
    fn scroll_turns_the_ring_and_wraps() {
        let v = OPENING;
        let p = map_pan(v, -100.0, 1000.0, true);
        assert!((p.center - 0.9).abs() < 1e-12);
    }

    #[test]
    fn the_rail_sends_the_window_and_its_ends_zoom() {
        let v = Viewport { center: 0.2, width: 0.4, projection: Projection::Line };
        assert!((rail_press(v, 0.5, false).center - 0.5).abs() < 1e-12);
        // dragging the right end out by a tenth widens by a tenth and keeps the left end put
        let d = rail_drag(v, Grip::Right, 0.1, 0.2, 0.4, false);
        assert!((d.width - 0.5).abs() < 1e-12);
        assert!((d.center - d.width / 2.0).abs() < 1e-12);
        let m = rail_drag(v, Grip::Move, 0.1, 0.2, 0.4, false);
        assert!((m.center - 0.3).abs() < 1e-12);
    }

    #[test]
    fn the_rail_wheel_zooms_about_the_pointer() {
        let v = Viewport { center: 0.5, width: 0.4, projection: Projection::Line };
        let z = rail_wheel(v, 0.0, -30.0, 0.6, 1000.0, false);
        let k = z.width / v.width;
        assert!((z.center - (0.6 + (0.5 - 0.6) * k)).abs() < 1e-12);
        let p = rail_wheel(v, 100.0, 0.0, 0.6, 1000.0, false);
        assert!((p.center - 0.54).abs() < 1e-12);
    }
}
