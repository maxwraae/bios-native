//! The navigator seam at the floor of the editor pane: the whole construct as a rail in the map's
//! own tints, cut sites as hairlines, the window you are looking at as a box with grips, and the
//! readout that travels with it. Ported from Bios `editor/overview/MapMinimap.tsx` and
//! `styles.css` lines 5309-5600 (`.navigator-seam`, `.editor-seam`, `.navigator-*`). Where things
//! sit is decided in `bios_layout::overview::minimap`; what a hand does in
//! `bios_layout::overview::navigate`.
//!
//! The web settles the window over 0.2s when the rail is clicked; here it lands at once.

use bios_layout::overview::minimap::{
    self, BAND, BAND_TOP, CUT_H, CUT_TOP, FRAME_H, GRIP_W, MARK_MAJOR_H, MARK_MINOR_H, MARK_TOP, NavFeature, PAD_X, PAD_Y, TEXT_TOP, TRACK_H,
    WINDOW_MIN_W,
};
use bios_layout::overview::navigate::{self, Grip};
use bios_layout::overview::viewport::Viewport;
use egui::{Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::primitives::shadow;
use crate::theme::{self, Shadow, Weight};

/// A feature on the rail, with the colour its arc is drawn in.
#[derive(Clone, Copy, Debug)]
pub struct Feature {
    pub nav: NavFeature,
    pub color: Color32,
}

/// A press on the window still down: what it took hold of, where, and the window it found.
#[derive(Clone, Copy)]
struct Held {
    grip: Grip,
    x: f32,
    center: f64,
    width: f64,
}

/// `.navigator-pane`'s lift: `0 1px 2px rgb(30 35 40 / 6%)`.
const PANE_SHADOW: &[Shadow] = &[Shadow { y: 1.0, blur: 2.0, spread: 0.0, color: Color32::from_rgba_unmultiplied_const(30, 35, 40, 15) }];

/// Draws the seam in `rect` (the full-bleed band at the pane's floor, `--navigator-h` tall) and
/// moves `vp` as the hand asks. `ring` is the map style's choice, which decides ring or line as
/// the width changes.
pub fn show(ui: &mut Ui, rect: Rect, l: i64, features: &[Feature], cuts: &[i64], vp: &mut Viewport, ring: bool) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme::CELL_SUNKEN);
    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, theme::HAIRLINE));
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, theme::HAIRLINE));
    if l <= 0 {
        return;
    }
    let track = Rect::from_min_max(
        pos2(rect.left() + PAD_X as f32, rect.top() + 1.0 + PAD_Y as f32),
        pos2(rect.right() - PAD_X as f32, rect.top() + 1.0 + (PAD_Y + TRACK_H) as f32),
    );
    let tw = track.width().max(1.0);
    let x = |f: f64| track.left() + (f.clamp(0.0, 1.0) as f32) * tw;
    let band =
        |top: f64, h: f64, l: f64, w: f64| Rect::from_min_max(pos2(x(l), track.top() + top as f32), pos2(x(l + w), track.top() + (top + h) as f32));

    let response = ui.interact(track.expand2(vec2(GRIP_W as f32 / 2.0, 1.0)), ui.scope_id().with("navigator"), Sense::click_and_drag());
    pointer(ui, &response, track, vp, ring);

    let win = minimap::window(*vp);
    if let Some(w) = &win {
        let r = band(0.0, FRAME_H, w.left, w.width);
        shadow(&painter, r, 4.0, PANE_SHADOW);
        painter.rect_filled(r, CornerRadius::same(4), Color32::from_rgba_unmultiplied(255, 255, 255, 148));
    }
    painter.rect_filled(band(BAND_TOP, BAND, 0.0, 1.0), CornerRadius::same(5), Color32::from_rgba_unmultiplied(14, 14, 16, 18));
    let feature_clip = painter.with_clip_rect(band(BAND_TOP, BAND, 0.0, 1.0).intersect(rect));
    let navs: Vec<NavFeature> = features.iter().map(|f| f.nav).collect();
    for seg in minimap::segments(&navs, l) {
        let Some(f) = features.get(seg.feature) else { continue };
        let mut r = band(BAND_TOP, BAND, seg.left, seg.width);
        r.max.x = r.max.x.max(r.min.x + 2.0);
        feature_clip.rect_filled(r, CornerRadius::same(2), theme::mix(f.color, theme::CELL, theme::TINT_RING));
        let cap = Rect::from_min_max(r.min, pos2(r.min.x + 2.0, r.max.y));
        feature_clip.rect_filled(cap, CornerRadius { nw: 2, sw: 2, ne: 0, se: 0 }, f.color);
    }
    for &c in cuts {
        let cx = x(minimap::cut_at(c, l));
        painter.rect_filled(
            Rect::from_min_size(pos2(cx, track.top() + CUT_TOP as f32), vec2(1.0, CUT_H as f32)),
            0.0,
            theme::alpha(theme::CUT_SITE, 0.5),
        );
    }
    if let Some(w) = &win {
        let scrim = theme::alpha(theme::CELL_SUNKEN, 0.72);
        painter.rect_filled(band(CUT_TOP, CUT_H, 0.0, w.left.max(0.0)), 0.0, scrim);
        let right = w.left + w.width;
        painter.rect_filled(band(CUT_TOP, CUT_H, right, 1.0 - right), 0.0, scrim);
    }
    for m in minimap::marks(l) {
        let (h, a) = if m.major { (MARK_MAJOR_H, 1.0) } else { (MARK_MINOR_H, 0.65) };
        painter.rect_filled(
            Rect::from_min_size(pos2(x(m.at), track.top() + MARK_TOP as f32), vec2(1.0, h as f32)),
            0.0,
            theme::alpha(theme::HAIRLINE_STRONG, a),
        );
    }
    let extent = theme::sans(9.5);
    let text_top = track.top() + TEXT_TOP as f32;
    painter.text(pos2(track.left(), text_top), egui::Align2::LEFT_TOP, "1", extent.clone(), theme::MUTED);
    painter.text(pos2(track.right(), text_top), egui::Align2::RIGHT_TOP, minimap::end_extent(l), extent, theme::MUTED);
    if win.is_some() {
        let read = minimap::readout(*vp, l);
        let strong = painter.layout_no_wrap(read.range, theme::sans_weight(11.0, Weight::Heavy), theme::INK);
        let quiet = painter.layout_no_wrap(read.span, theme::sans_weight(11.0, Weight::Medium), theme::MUTED);
        let lw = strong.size().x + quiet.size().x;
        let cx = track.left() + minimap::label_x(*vp, f64::from(tw), f64::from(lw)) as f32;
        let left = cx - lw / 2.0;
        let sw = strong.size().x;
        painter.galley(pos2(left, text_top), strong, theme::INK);
        painter.galley(pos2(left + sw, text_top), quiet, theme::MUTED);
    }
    if let Some(w) = &win {
        let held = ui.data(|d| d.get_temp::<Held>(response.id)).is_some();
        let over = response.hover_pos();
        let single = w.pieces.len() == 1;
        for p in &w.pieces {
            let mut r = band(0.0, FRAME_H, p.left, p.width);
            if r.width() < WINDOW_MIN_W as f32 {
                r.max.x = r.min.x + WINDOW_MIN_W as f32;
            }
            let hovered = over.is_some_and(|o| r.expand2(vec2(GRIP_W as f32 / 2.0, 0.0)).contains(o));
            let edge = if held {
                0.78
            } else if hovered {
                0.62
            } else {
                0.40
            };
            let corner = CornerRadius {
                nw: if p.cut_left { 0 } else { 4 },
                sw: if p.cut_left { 0 } else { 4 },
                ne: if p.cut_right { 0 } else { 4 },
                se: if p.cut_right { 0 } else { 4 },
            };
            painter.rect_stroke(r, corner, Stroke::new(1.0, theme::alpha(theme::INK, edge)), StrokeKind::Inside);
            let grip_ink = theme::alpha(theme::INK, if hovered { 0.72 } else { 0.52 });
            let grip = |cx: f32| {
                let bar = Rect::from_center_size(pos2(cx, r.center().y), vec2(4.0, r.height() + 2.0));
                painter.rect_filled(bar, CornerRadius::same(2), grip_ink);
            };
            if single || p.left_edge {
                grip(r.left());
            }
            if single || p.right_edge {
                grip(r.right());
            }
        }
    }
}

/// What the pointer does on the track: a press on the rail sends the window there; on the window
/// it moves it; on a grip it zooms; the wheel zooms about the pointer.
fn pointer(ui: &Ui, response: &egui::Response, track: Rect, vp: &mut Viewport, ring: bool) {
    let tw = f64::from(track.width().max(1.0));
    let frac = |px: f32| f64::from(px - track.left()) / tw;
    let win = minimap::window(*vp);
    let grip_at = |px: f32| -> Option<Grip> {
        let w = win.as_ref()?;
        let single = w.pieces.len() == 1;
        let half = GRIP_W as f32 / 2.0;
        for p in &w.pieces {
            let l = track.left() + p.left as f32 * track.width();
            let r = (track.left() + (p.left + p.width) as f32 * track.width()).max(l + WINDOW_MIN_W as f32);
            if (single || p.left_edge) && (px - l).abs() <= half {
                return Some(Grip::Left);
            }
            if (single || p.right_edge) && (px - r).abs() <= half {
                return Some(Grip::Right);
            }
            if px >= l && px <= r {
                return Some(Grip::Move);
            }
        }
        None
    };
    if let Some(over) = response.hover_pos() {
        let on_body = over.y <= track.top() + FRAME_H as f32 + 1.0;
        let icon = match grip_at(over.x).filter(|_| on_body) {
            Some(Grip::Left | Grip::Right) => CursorIcon::ResizeHorizontal,
            Some(Grip::Move) => CursorIcon::Grab,
            None => CursorIcon::PointingHand,
        };
        ui.ctx().set_cursor_icon(icon);
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        if zoom != 1.0 {
            *vp = navigate::rail_wheel(*vp, 0.0, -f64::from(zoom).ln() / 0.02, frac(over.x), tw, ring);
        } else if scroll != egui::Vec2::ZERO {
            *vp = navigate::rail_wheel(*vp, -f64::from(scroll.x), -f64::from(scroll.y), frac(over.x), tw, ring);
        }
    }
    let (pressed, down) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
    let Some(at) = response.interact_pointer_pos().filter(|_| down) else {
        ui.data_mut(|d| d.remove::<Held>(response.id));
        return;
    };
    if pressed {
        let on_body = at.y <= track.top() + FRAME_H as f32 + 1.0;
        match grip_at(at.x).filter(|_| on_body) {
            Some(grip) => ui.data_mut(|d| {
                d.insert_temp(response.id, Held { grip, x: at.x, center: vp.center, width: vp.width });
            }),
            None => *vp = navigate::rail_press(*vp, frac(at.x), ring),
        }
        return;
    }
    if let Some(held) = ui.data(|d| d.get_temp::<Held>(response.id)) {
        ui.ctx().set_cursor_icon(if held.grip == Grip::Move { CursorIcon::Grabbing } else { CursorIcon::ResizeHorizontal });
        let dx = f64::from(at.x - held.x) / tw;
        *vp = navigate::rail_drag(*vp, held.grip, dx, held.center, held.width, ring);
    }
}
