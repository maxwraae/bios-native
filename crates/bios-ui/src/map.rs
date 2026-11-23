//! The map: the whole construct at a glance, as a ring when circular and a line when linear.
//!
//! Drawn as the Bios web app draws its map at the whole-molecule view (`renderRing` and
//! `renderLine` in `editor/overview/PlasmidOverview.tsx`). Where everything sits is decided in
//! `bios_layout::overview` (`ring_map`, `line_map`); this file only paints those numbers.
//!
//! [`show`] also draws where the person is (the selection as a band along the molecule's own axis,
//! the caret as a mark across it) and answers the pointer the way that component does
//! (`beginDrag`, `onPointerMove`): the map selects as a text editor does. What is under the
//! pointer is decided in `bios_layout::overview::hit`.

use bios_core::acts::{Graph, Target};
use bios_core::enzymes::CutSite;
use bios_core::{Construct, Feature, Strand};
use bios_layout::overview::arc_text::arc_glyphs;
use bios_layout::overview::bands::{BandLayout, DIM_AT, OuterBand, SeamMark, layout_band, overlap_fraction};
use bios_layout::overview::chords::{Pt as ChordPt, arrow_head_points as arrow, chord_apex, mid_frac};
use bios_layout::overview::display::{FileFeature, MapFeature, display_features};
use bios_layout::overview::flywheel::{self, Flywheel, SpinPhase, Tone};
use bios_layout::overview::hit::{MapHit, gap_of, line_feature_at, line_gap_at, ring_feature_at, ring_gap_at};
use bios_layout::overview::line_map::{LineBar, LineCut, LineMap, NUMBER_DROP, layout_line};
use bios_layout::overview::morph_map::{MorphMap, along_path, layout_morph};
use bios_layout::overview::navigate::{self, ZoomTarget};
use bios_layout::overview::projection::{
    LINE_MARGIN, Point, RingGeom, TAU, line_window, line_x, point_to_frac_line, point_to_frac_ring, ring_point, straightness,
};
use bios_layout::overview::ring_map::{Anchor, ArcLabel, RingArc, RingCut, RingMap, layout_ring};
use bios_layout::overview::scene::{CUT_FONT, Flag, Frame, MapScene};
use bios_layout::overview::selection::ring_step;
use bios_layout::overview::viewport::{MIN_WIDTH, Piece, Projection, Viewport, pan_by, split_span};
use egui::epaint::{QuadraticBezierShape, TextShape};
use egui::{Color32, CornerRadius, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::theme;
use crate::view_tab::Layers;

/// Type size of the numbers on the coordinate scale.
const SCALE_FONT: f32 = 10.0;
/// How far a major and a minor tick reach from the axis.
const TICK_MAJOR: f32 = 6.0;
const TICK_MINOR: f32 = 3.0;

/// The selection on the ring is a band this wide along the axis, whatever a lane's thickness is:
/// it is a range of sequence, not a feature. On the line it reaches [`SELECTION_REACH`] past a
/// lane's half thickness either side of the axis.
const SELECTION_BAND: f32 = 26.0;
const SELECTION_REACH: f64 = 4.0;
/// How strong the selection's blue is on the ring and on the line: its opacity.
const SELECTION_RING: f32 = 0.28;
const SELECTION_LINE: f32 = 0.22;
/// The caret: a line across the axis with a dot on its outer end. On the ring it reaches
/// [`CARET_RING_REACH`] either side of the axis; on the line it has to clear the lane beside the
/// axis to stay visible, so it reaches [`CARET_LINE_REACH`] past a lane's half thickness.
const CARET_WIDTH: f32 = 2.0;
const CARET_DOT: f32 = 2.4;
const CARET_RING_REACH: f64 = 7.0;
const CARET_LINE_REACH: f64 = 9.0;
/// How far the ring may be turned out of the frame, above or below, and still be drawn.
const OFF_FRAME: f64 = 60.0;
/// A few pixels of jitter in a click is still a click: the pointer has to travel further than
/// this, across plus down, before a press becomes a drag.
const CLICK_SLOP: f32 = 3.0;

/// Where the person is in the sequence, for the map to draw. A caret and a selection are never
/// both set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapCursor {
    /// The gap the caret stands in: 0 is before the first base, the length is after the last.
    pub caret: Option<usize>,
    /// The first and last selected base, 0-based. An end before the start runs through the origin.
    pub selection: Option<(usize, usize)>,
}

/// What the pointer did on the map this frame. A gap is counted like [`MapCursor::caret`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapGesture {
    /// The button went down at this gap, anywhere on the map that is not a feature; with shift
    /// held, on a feature's arc or bar too. Also given, once, when a drag begins on a feature:
    /// the gap it began at, from which the drags that follow select.
    Press { gap: usize, shift: bool },
    /// The pointer is at this gap with the button still down, having left where it went down.
    ///
    /// On a ring the gap is unwrapped: it starts from the press's gap and keeps counting past
    /// the length, or below 0, as the pointer goes round, so that
    /// `bios_layout::overview::selection::drag_selection` selects through the origin and takes a
    /// full turn as everything.
    Drag { gap: i64 },
    /// The button went down on a feature's arc or bar, or on the flag carrying its name: the
    /// bases it covers, first and last, 0-based. An end before the start runs through the origin.
    Feature { start: usize, end: usize },
}

/// A press that is still down, kept between frames in the interface's memory.
#[derive(Clone, Copy)]
struct Held {
    /// Where the button went down.
    origin: Pos2,
    /// The gap it went down at.
    anchor: usize,
    /// Whether the pointer has since left that spot.
    moved: bool,
    /// Whether it went down on a feature, and no drag has begun from it yet.
    on_feature: bool,
    /// The last gap read off the map, which [`ring_step`] steps from.
    prev: i64,
    /// Where the pointer is, unwrapped on the ring.
    to: i64,
}

// ── THE OUTER BAND ──────────────────────────────────────────────────────────────────────────
//
// What is being ADDED, drawn outside the molecule: the body over the opening, the homology arms
// running past both cuts onto backbone that stays. The overlap is not decoration — it is what the
// method is, and this is the one picture that teaches Gibson without a sentence.
//
// The numbers are `bios_layout::overview::bands`; this only paints them.

/// How far outside the axis the band's own row sits (`BAND_R` in the web's `PlasmidOverview`).
const BAND_R: f64 = 26.0;
/// How thick the band's bar is.
const BAND_STROKE: f32 = 12.0;
/// What the ring gives up so the band clears the coordinate scale: the band, a label tier, and
/// air. The outer tier is deliberately not reserved for — it only exists when seams stagger.
pub const BAND_RESERVE: f64 = BAND_R + BAND_STROKE as f64 + 16.0;
/// How far the band floats above the axis in line view.
const BAND_LINE_Y: f64 = 30.0;
/// A seam's temperature is written once there is this much room beside it.
const SEAM_ROOM: f64 = 26.0;

/// What is drawn on top of the molecule that is not part of it: a design going in, and the stretch
/// that is leaving.
#[derive(Clone, Copy, Default)]
pub struct Overlay<'a> {
    /// Bands of new DNA. Empty on an ordinary map, and then nothing here costs anything.
    pub bands: &'a [OuterBand],
    /// The opening, half-open `[up, down)`: washed out, and the features mostly inside it drawn
    /// dim, because they are leaving with it.
    pub dim: Option<(i64, i64)>,
}

/// A construct laid out in a rectangle.
struct Plan {
    scene: MapScene,
    frame: Frame,
    laid: Laid,
    /// The viewport's own projection, which mid-unroll decides how the pointer is read.
    projection: Projection,
}

enum Laid {
    Ring(RingMap),
    Line(LineMap),
    /// A ring part way to its line (`renderMorph`).
    Morph(MorphMap),
}

impl Plan {
    #[allow(clippy::too_many_arguments)]
    fn new(
        painter: &Painter,
        rect: Rect,
        c: &Construct,
        view: MapView,
        cuts: &[CutSite],
        layers: Layers,
        overlay: &Overlay,
        axis_at_bottom: Option<f64>,
    ) -> Plan {
        let scene =
            scene_with(c, layers.splice_scan).with_cuts(cuts.iter().map(|s| (i64::try_from(s.top_cut).unwrap_or(0), s.enzyme.to_string())).collect());
        let measure = |text: &str, font_px: f64| f64::from(advance(painter, text, font_px as f32));
        let (w, h) = (f64::from(rect.width()), f64::from(rect.height()));
        let vp = view.vp;
        // A band stands outside the coordinate scale, so the ring comes in to make room for it.
        // One number, read by the renderer and the hit-test alike.
        let band_reserve = if overlay.bands.is_empty() { 0.0 } else { BAND_RESERVE };
        let frame = Frame {
            center: vp.center,
            width: vp.width,
            density: layers.density,
            cut_names: layers.cut_names,
            band_reserve,
            axis_at_bottom,
            ..Frame::ring(w, h)
        };
        // Zoom drives one continuous transform: the circle (t = 0) unrolls into the line (t = 1).
        // The ring and line layouts own the two ends and the morph fills the transition, so there is
        // no snap. Picking the line style keeps a line at every zoom.
        let t = if view.ring { straightness(vp.width) } else { 1.0 };
        let laid = if t <= 0.0 {
            Laid::Ring(layout_ring(&scene, frame, measure))
        } else if t >= 1.0 {
            Laid::Line(layout_line(&scene, frame, measure))
        } else {
            Laid::Morph(layout_morph(&scene, frame, t, measure))
        };
        Plan { scene, frame, laid, projection: vp.projection }
    }

    fn gap_at(&self, at: Point) -> i64 {
        match &self.laid {
            Laid::Ring(map) => ring_gap_at(map, self.scene.length, at),
            Laid::Line(map) => line_gap_at(map, self.frame, self.scene.length, at.x),
            // Mid-unroll the pointer is read the way the web reads it (`fracAt`): off the ring or off
            // the line, whichever the viewport's projection is, not through the morph itself.
            Laid::Morph(map) => {
                let frac = match self.projection {
                    Projection::Ring => point_to_frac_ring(at.x, at.y, map.center, map.geom),
                    Projection::Line => {
                        let window = line_window(Viewport { center: self.frame.center, width: self.frame.width, projection: Projection::Line });
                        point_to_frac_line(at.x, window.left, self.frame.width, self.frame.w, LINE_MARGIN)
                    }
                };
                gap_of(frac, self.scene.length)
            }
        }
    }

    /// The feature under a point. Nothing mid-unroll: the web gives the morph's bands no handlers.
    fn feature_at(&self, at: Point) -> Option<MapHit> {
        match &self.laid {
            Laid::Ring(map) => ring_feature_at(map, at),
            Laid::Line(map) => line_feature_at(map, at),
            Laid::Morph(_) => None,
        }
    }

    /// The bases feature `i` covers, as a gesture.
    fn feature(&self, i: usize) -> Option<MapGesture> {
        let f = self.scene.features.get(i)?;
        Some(MapGesture::Feature { start: usize::try_from(f.start).ok()?, end: usize::try_from(f.end).ok()? })
    }
}

/// The window onto the molecule the map draws through, and whether its style is the ring (which
/// unrolls into a line when zoomed in) or the line (a line at every zoom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub vp: Viewport,
    pub ring: bool,
}

impl MapView {
    /// The whole construct, drawn the way its topology draws it.
    pub fn whole(c: &Construct) -> MapView {
        let vp = if c.circular { navigate::OPENING } else { Viewport { center: 0.5, width: 1.0, projection: Projection::Line } };
        MapView { vp, ring: c.circular }
    }
}

pub fn draw(painter: &Painter, rect: Rect, c: &Construct) {
    draw_view(painter, rect, c, MapView::whole(c), None);
}

/// The map drawn through `view`, with nothing on it: a figure in a page rather than a pane.
///
/// `axis_at_bottom` pins the line's axis that far above the floor instead of centring the whole
/// drawing, which is what a short band in a list needs: centred, a construct several lanes deep
/// puts its features above the top of the band and they are simply not drawn.
pub fn draw_view(painter: &Painter, rect: Rect, c: &Construct, view: MapView, axis_at_bottom: Option<f64>) {
    let overlay = Overlay::default();
    let plan = Plan::new(painter, rect, c, view, &[], Layers::default(), &overlay, axis_at_bottom);
    Paint {
        painter,
        origin: rect.min.to_vec2(),
        scene: &plan.scene,
        frame: plan.frame,
        cursor: MapCursor::default(),
        wires: Wires::default(),
        overlay,
    }
    .map(&plan.laid);
}

/// Draws the whole map with where the person is on it, and says what the pointer did there.
pub fn show(ui: &mut egui::Ui, rect: Rect, c: &Construct, cursor: &MapCursor) -> Option<MapGesture> {
    let mut view = MapView::whole(c);
    show_view(ui, rect, c, cursor, &mut view, &[], Layers::default())
}

/// Draws the map through `view` with where the person is on it, and says what the pointer did
/// there.
///
/// The map selects the way a text editor does. A press drops a caret, a drag selects from where
/// it began, and shift extends what is already selected; the caller holds the selection and makes
/// those changes from the gestures. A press on a feature selects the bases it covers.
///
/// Two wheel gestures, as the web map has (`PlasmidOverview.tsx` 933-986): plain scroll pans (slides
/// the line, turns the ring); a pinch, or ⌘/ctrl with the wheel, zooms, anchored under the pointer on
/// the line. They change `view`.
///
/// `cuts` are the cut sites drawn: a hash across the strand each, named where a row is free.
/// `layers` says whether those names are shown and how densely names stack.
pub fn show_view(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &Construct,
    cursor: &MapCursor,
    view: &mut MapView,
    cuts: &[CutSite],
    layers: Layers,
) -> Option<MapGesture> {
    show_overlay(ui, rect, c, cursor, view, cuts, layers, Overlay::default())
}

/// [`show_view`] with something drawn on top of the molecule: a design going in, and the stretch
/// that is leaving. Build is what has those; everywhere else passes `Overlay::default()`.
///
/// Its zoom eases as the editor's does; the flywheel is the editor's alone ([`show_moving`]).
#[allow(clippy::too_many_arguments)]
pub fn show_overlay(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &Construct,
    cursor: &MapCursor,
    view: &mut MapView,
    cuts: &[CutSite],
    layers: Layers,
    overlay: Overlay<'_>,
) -> Option<MapGesture> {
    let id = ui.scope_id().with("map-motion");
    let mut motion = ui.data(|d| d.get_temp::<Motion>(id)).unwrap_or_default();
    let gesture = show_inner(ui, rect, c, cursor, view, &mut motion, cuts, layers, overlay);
    ui.data_mut(|d| d.insert_temp(id, motion));
    gesture
}

/// [`show_view`] with the map's motion held by the caller: the eased zoom, and the ring as a
/// flywheel when `motion.spin` lets it be one.
#[allow(clippy::too_many_arguments)]
pub fn show_moving(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &Construct,
    cursor: &MapCursor,
    view: &mut MapView,
    motion: &mut Motion,
    cuts: &[CutSite],
    layers: Layers,
) -> Option<MapGesture> {
    show_inner(ui, rect, c, cursor, view, motion, cuts, layers, Overlay::default())
}

#[allow(clippy::too_many_arguments)]
fn show_inner(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &Construct,
    cursor: &MapCursor,
    view: &mut MapView,
    motion: &mut Motion,
    cuts: &[CutSite],
    layers: Layers,
    overlay: Overlay<'_>,
) -> Option<MapGesture> {
    let painter = ui.painter().with_clip_rect(rect);
    let response = ui.interact(rect, ui.scope_id().with("map"), Sense::click_and_drag());
    let now = ui.input(|i| i.time);
    if !c.is_empty() {
        wheel(ui, &response, view, motion, now);
    }
    motion.animate(ui.ctx(), view, now);
    let plan = Plan::new(&painter, rect, c, *view, cuts, layers, &overlay, None);
    let gesture = if motion.press(ui, &response, now) { None } else { pointer(ui, &response, &plan) };
    let wires = if layers.interactions { Wires::of(c, &plan.scene, crate::rows::lit_feature(ui.ctx())) } else { Wires::default() };
    let paint = Paint { painter: &painter, origin: rect.min.to_vec2(), scene: &plan.scene, frame: plan.frame, cursor: *cursor, wires, overlay };
    match &plan.laid {
        Laid::Ring(map) if flywheel::active(motion.fly.phase) => {
            paint.carriage(map, motion);
            if motion.fly.phase == SpinPhase::Gone && rolled_off(ui, rect) {
                motion.fetch(now);
                ui.ctx().request_repaint();
            }
        }
        laid => paint.map(laid),
    }
    gesture
}

/// What the map's motion remembers between frames: the zoom it is easing toward, and the ring as a
/// flywheel (`useViewport.ts` and `useFlywheel.ts`). Time is the interface's clock, in seconds.
#[derive(Clone, Debug)]
pub struct Motion {
    zoom: Option<ZoomTarget>,
    zoom_last: Option<f64>,
    pub fly: Flywheel,
    fly_last: Option<f64>,
    /// Whether the ring may be wound up into a flywheel at all: a full ring the person turns with
    /// their own hand, in the editor.
    pub spin: bool,
    /// Holds the flywheel where it is, for a picture of one moment of it.
    pub frozen: bool,
    /// The flywheel's clock at its last frame: where its sparks are and how its flames flicker.
    pub now: f64,
}

impl Default for Motion {
    fn default() -> Self {
        Motion { zoom: None, zoom_last: None, fly: flywheel::idle(1, 0.0), fly_last: None, spin: false, frozen: false, now: 0.0 }
    }
}

impl Motion {
    /// The editor's motion: the flywheel may take the ring.
    pub fn spinning() -> Self {
        Motion { spin: true, ..Motion::default() }
    }

    /// Hard stop, including a windup: a zoom, a projection change, a style button.
    pub fn stop(&mut self, now: f64) {
        if self.fly.phase == SpinPhase::Idle && self.fly.run.is_none() {
            return;
        }
        self.fly = flywheel::idle(self.fly.seed, now);
        self.fly_last = None;
        self.frozen = false;
    }

    /// A style button wins over a zoom in flight, and takes the ring back.
    pub fn jump(&mut self, now: f64) {
        self.zoom = None;
        self.zoom_last = None;
        self.stop(now);
    }

    /// Flies the view to `width` about `center`, eased like a wheel zoom: the target rides the zoom
    /// as its anchor, so the centre converges on it as the window comes in (`PlasmidOverview`'s
    /// `focus`). `v` is the view as it stands.
    pub fn focus(&mut self, v: Viewport, center: f64, width: f64, now: f64) {
        self.jump(now);
        let factor = if v.width > 0.0 { width / v.width } else { 1.0 };
        self.zoom = Some(navigate::zoom_toward(v, None, factor, Some((center, 0.5)), MIN_WIDTH));
    }

    /// The roll-back button: from gone, the ring rolls back in.
    pub fn fetch(&mut self, now: f64) {
        self.fly = flywheel::fetch(&self.fly, now);
        self.frozen = false;
    }

    /// One frame of motion: the eased zoom one step nearer its target, and the flywheel one step
    /// further. Asks for the next frame while either is moving.
    fn animate(&mut self, ctx: &egui::Context, view: &mut MapView, now: f64) {
        if let Some(target) = self.zoom {
            let dt = navigate::frame_dt(self.zoom_last, now);
            let (vp, done) = navigate::ease_zoom(view.vp, target, dt, view.ring);
            view.vp = vp;
            if done {
                self.zoom = None;
                self.zoom_last = None;
            } else {
                self.zoom_last = Some(now);
                ctx.request_repaint();
            }
        }
        // A zoom or a projection change takes the ring back from it.
        if view.vp.projection != Projection::Ring || straightness(view.vp.width) > 0.0 || !view.ring {
            self.stop(now);
        }
        if !flywheel::active(self.fly.phase) || self.frozen {
            return;
        }
        let dt = navigate::frame_dt(self.fly_last, now);
        let (next, turned) = flywheel::step(&self.fly, dt, now);
        self.fly = next;
        self.now = now;
        if turned != 0.0 {
            view.vp = navigate::normalize(pan_by(view.vp, turned), view.ring);
        }
        if flywheel::active(self.fly.phase) {
            self.fly_last = Some(now);
            ctx.request_repaint();
        } else {
            self.fly_last = None;
        }
    }

    /// A hand on a spinning ring stops it dead, and places no caret; a ring that has broken loose
    /// cannot be caught, and takes no caret either. True when the press was the flywheel's.
    fn press(&mut self, ui: &egui::Ui, response: &Response, now: f64) -> bool {
        if !flywheel::active(self.fly.phase) {
            return false;
        }
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            self.fly = flywheel::press(&self.fly, now);
            self.frozen = false;
        }
        // Until the button is up again the press is the flywheel's, not the start of a drag.
        ui.data_mut(|data| data.remove::<Held>(response.id));
        true
    }
}

/// The wheel and the pinch over the map. Plain scroll pans (slides the line, turns the ring); a
/// pinch, or ⌘/ctrl with the wheel, sets the target the zoom eases toward, anchored under the
/// pointer on the line. A full ring can be wound up into the flywheel; once it owns the ring the
/// hand's scroll is torque, not pan.
fn wheel(ui: &egui::Ui, response: &Response, view: &mut MapView, motion: &mut Motion, now: f64) {
    let Some(over) = response.hover_pos() else {
        return;
    };
    let (scroll, zoom, command) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command || i.modifiers.ctrl));
    let w = f64::from(response.rect.width());
    let x = f64::from(over.x - response.rect.left());
    // egui's scroll is the content's motion; the web's wheel delta is the opposite sign.
    let (dx, dy) = (-f64::from(scroll.x), -f64::from(scroll.y));
    let dz = if zoom != 1.0 {
        // A pinch arrives as a zoom factor; turned back into the wheel's delta, `exp(dy * 0.02)`.
        Some(-f64::from(zoom).ln() / 0.02)
    } else if scroll != Vec2::ZERO && command {
        Some(dy)
    } else {
        None
    };
    if let Some(dz) = dz {
        motion.stop(now);
        let anchor = navigate::wheel_anchor(view.vp, x, w, LINE_MARGIN);
        motion.zoom = Some(navigate::zoom_toward(view.vp, motion.zoom, navigate::wheel_factor(dz), anchor, MIN_WIDTH));
        ui.ctx().request_repaint();
        return;
    }
    if scroll == Vec2::ZERO {
        return;
    }
    let delta = navigate::dominant(dx, dy);
    if view.vp.projection == Projection::Line {
        view.vp = navigate::map_pan(view.vp, delta, w, view.ring);
        return;
    }
    let w = if w > 0.0 { w } else { 1.0 };
    let turns = (delta / w) * 0.25f64.max(view.vp.width);
    let pan = if motion.spin && view.ring && straightness(view.vp.width) <= 0.0 {
        let (next, pan) = flywheel::wheel(&motion.fly, turns, now);
        motion.fly = next;
        if flywheel::active(motion.fly.phase) {
            ui.ctx().request_repaint();
        }
        pan
    } else {
        turns
    };
    if pan != 0.0 {
        view.vp = navigate::normalize(pan_by(view.vp, pan), view.ring);
    }
}

/// The map once the ring has rolled off: one line, and the button that rolls it back
/// (`.overview-rolled`). True when the button was pressed.
fn rolled_off(ui: &mut egui::Ui, rect: Rect) -> bool {
    let line = ui.painter().layout_no_wrap("It rolled off.".to_owned(), theme::sans(13.0), theme::MUTED);
    let label = ui.painter().layout_no_wrap("Go get it".to_owned(), theme::sans_weight(13.0, theme::Weight::Medium), theme::INK);
    let button = vec2(label.size().x + 32.0, label.size().y + 14.0);
    let total = line.size().y + 12.0 + button.y;
    let top = rect.center().y - total / 2.0;
    let painter = ui.painter().with_clip_rect(rect);
    let line_h = line.size().y;
    painter.galley(pos2(rect.center().x - line.size().x / 2.0, top), line, theme::MUTED);
    let at = Rect::from_center_size(pos2(rect.center().x, top + line_h + 12.0 + button.y / 2.0), button);
    let response = ui.interact(at, ui.scope_id().with("ring-fetch"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    let border = if response.hovered() { theme::MUTED } else { theme::HAIRLINE_STRONG };
    painter.rect(at, CornerRadius::same(255), theme::FLOAT, Stroke::new(1.0, border), StrokeKind::Inside);
    painter.galley(at.center() - label.size() / 2.0, label, theme::INK);
    response.clicked()
}

fn hovered_id() -> egui::Id {
    egui::Id::unique("bios-map-hovered")
}

/// The construct's feature the pointer is on over the map, this frame.
pub fn hovered(ctx: &egui::Context) -> Option<usize> {
    ctx.data(|d| d.get_temp::<Option<usize>>(hovered_id())).flatten()
}

/// What the pointer did this frame.
///
/// Nothing is decided at a press on an arc or a bar beyond selecting its feature: a drag that
/// starts there selects bases like any other, and says so with a `Press` at the gap it began at.
/// A flag takes its press whole. Shift on an arc is the map's own press, extending over it.
fn pointer(ui: &egui::Ui, response: &Response, plan: &Plan) -> Option<MapGesture> {
    let local = |at: Pos2| Point { x: f64::from(at.x - response.rect.left()), y: f64::from(at.y - response.rect.top()) };
    let (pressed, down, shift) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift));
    let held_at = response.interact_pointer_pos().filter(|_| down && plan.scene.length > 0);
    // The file feature the pointer is on, for the Interactions layer to light (`setLitFeature`).
    let over =
        response.hover_pos().and_then(|at| plan.feature_at(local(at))).and_then(|hit| plan.scene.features.get(hit.feature())).map(|f| f.source);
    ui.ctx().data_mut(|d| d.insert_temp(hovered_id(), over));
    if held_at.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if let Some(over) = response.hover_pos() {
        ui.ctx().set_cursor_icon(if plan.feature_at(local(over)).is_some() { CursorIcon::PointingHand } else { CursorIcon::Grab });
    }
    let Some(at) = held_at else {
        ui.data_mut(|data| data.remove::<Held>(response.id));
        return None;
    };
    let gap = plan.gap_at(local(at));
    if pressed {
        let anchor = usize::try_from(gap).ok()?;
        let hit = plan.feature_at(local(at));
        if let Some(MapHit::Flag(feature)) = hit {
            ui.data_mut(|data| data.remove::<Held>(response.id));
            return plan.feature(feature);
        }
        let on = hit.filter(|_| !shift).and_then(|hit| plan.feature(hit.feature()));
        // Extending shows its range at once, so it is a drag from the first frame.
        let held = Held { origin: at, anchor, moved: shift, on_feature: on.is_some(), prev: gap, to: gap };
        ui.data_mut(|data| data.insert_temp(response.id, held));
        return on.or(Some(MapGesture::Press { gap: anchor, shift }));
    }
    let mut held = ui.data(|data| data.get_temp::<Held>(response.id))?;
    let travelled = (at.x - held.origin.x).abs() + (at.y - held.origin.y).abs();
    held.moved |= travelled > CLICK_SLOP;
    // Round the ring the pointer's position is carried on, so the origin is not a jump.
    held.to = if matches!(plan.laid, Laid::Ring(_)) { ring_step(held.to, held.prev, gap, plan.scene.length) } else { gap };
    held.prev = gap;
    let begins = held.moved && held.on_feature;
    held.on_feature &= !held.moved;
    ui.data_mut(|data| data.insert_temp(response.id, held));
    if begins {
        // The drag itself follows on the next frame, whether or not the pointer moves again.
        ui.ctx().request_repaint();
        return Some(MapGesture::Press { gap: held.anchor, shift: false });
    }
    held.moved.then_some(MapGesture::Drag { gap: held.to })
}

/// What the map draws of a construct: its features as shown, fused, ranked and given lanes.
pub fn scene_of(c: &Construct) -> MapScene {
    scene_with(c, false)
}

/// The same, with the splice-map hits drawn when they are asked for.
pub fn scene_with(c: &Construct, show_splice_scan: bool) -> MapScene {
    let file: Vec<FileFeature<'_>> = c.features.iter().map(file_feature).collect();
    let mut shown = display_features(&file, show_splice_scan);
    // Parts are not features: the web map draws `sequenceData.features` only, and parts get their
    // own lane in the rows.
    shown.retain(|f| c.features.get(f.source).is_none_or(|g| !bios_core::parts::is_part(g)));
    MapScene::new(&shown, bp(c.len()), c.circular)
}

fn bp(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn file_feature(f: &Feature) -> FileFeature<'_> {
    FileFeature {
        name: &f.name,
        kind: &f.kind,
        start: bp(f.start),
        end: bp(f.end),
        forward: f.strand == Strand::Forward,
        locations: f.locations().map(|segments| segments.iter().map(|s| (bp(s.start), bp(s.end))).collect()),
        location_unsupported: f.location_unsupported.is_some(),
    }
}

/// Advance width of `text` in the interface font, which is what the layout fits names by.
fn advance(painter: &Painter, text: &str, font_px: f32) -> f32 {
    let galley = painter.layout_no_wrap(text.to_owned(), theme::sans(font_px), Color32::PLACEHOLDER);
    galley.rows.first().map_or(0.0, |row| row.row.glyphs.iter().map(|g| g.advance_width).sum())
}

/// The face a feature's name is drawn in. The layout fits names by their width at the regular
/// weight ([`advance`]) and the web app then draws them a step heavier, so this does the same.
fn name_font(size: f32) -> FontId {
    theme::sans_weight(size, theme::Weight::Medium)
}

/// Which edge of a piece of text sits at the point it is placed by.
#[derive(Clone, Copy)]
enum Set {
    Start,
    Middle,
    End,
}

/// One relationship between two drawn features (`MapEdge` in `PlasmidOverview.tsx`).
struct Wire {
    /// The actor and the target, as indices into [`MapScene::features`].
    a: usize,
    b: usize,
    verb: String,
    /// Part of what the pointer is asking about, or nothing is lit.
    lit: bool,
    /// The actor's own colour, or the muted ink.
    color: Color32,
}

/// The Interactions layer on the map: the chords or brackets, and which features step back.
#[derive(Default)]
struct Wires {
    wires: Vec<Wire>,
    /// Per scene feature: greyed back because the lit feature is not wired to it (`arcTone`).
    dim: Vec<bool>,
}

impl Wires {
    /// The construct's relationships between features the map draws. The whole construct is
    /// never an endpoint here, as in the rows.
    fn of(c: &Construct, scene: &MapScene, lit: Option<usize>) -> Wires {
        let graph = Graph::of(&c.features);
        let drawn = |file: usize| scene.features.iter().position(|f| f.source == file);
        let wires = graph
            .edges
            .iter()
            .filter_map(|e| {
                let Target::Feature(t) = e.target else { return None };
                let (a, b) = (drawn(e.actor)?, drawn(t)?);
                let color = c.features.get(e.actor).map_or(theme::MUTED, crate::acts_on::wire_color);
                (a != b).then(|| Wire { a, b, verb: e.verb.clone(), lit: graph.is_lit(lit, e.actor) && graph.is_lit(lit, t), color })
            })
            .collect();
        let dim = scene.features.iter().map(|f| lit.is_some() && !graph.is_lit(lit, f.source)).collect();
        Wires { wires, dim }
    }
}

/// How strong a greyed-back feature is drawn (`opacity` 0.12 for a `dim` tone).
const DIM: f32 = 0.12;

struct Paint<'a> {
    painter: &'a Painter,
    /// Top left of the map's rectangle: the layout's numbers are measured from it.
    origin: Vec2,
    scene: &'a MapScene,
    frame: Frame,
    cursor: MapCursor,
    /// What acts on what, while the Interactions layer is on.
    wires: Wires,
    /// A design going in, and the stretch leaving with the cut.
    overlay: Overlay<'a>,
}

impl Paint<'_> {
    fn map(&self, laid: &Laid) {
        match laid {
            Laid::Ring(map) => self.ring(map),
            Laid::Line(map) => self.line(map),
            Laid::Morph(map) => self.morph(map),
        }
    }

    // ---- the unroll ----

    /// `renderMorph`: the backbone, each band with its arrowhead, and each name along its band.
    fn morph(&self, map: &MorphMap) {
        let points = |run: &[Point]| run.iter().map(|p| self.at(*p)).collect::<Vec<Pos2>>();
        self.painter.add(Shape::line(points(&map.backbone), Stroke::new(1.5, theme::HAIRLINE_STRONG)));
        for arc in &map.arcs {
            for run in &arc.runs {
                self.painter.add(Shape::line(points(run), Stroke::new(map.thickness as f32, self.tint(arc.feature, arc.mark))));
            }
            if let Some(head) = arc.head {
                self.head(arc.feature, head);
            }
        }
        for name in &map.names {
            let (_, ink) = self.colors(name.feature);
            let font = name_font(name.font as f32);
            let whole = self.painter.layout_no_wrap(name.text.clone(), font.clone(), ink);
            let Some(row) = whole.rows.first() else { continue };
            let glyphs = &row.row.glyphs;
            let centres: Vec<f64> = glyphs.iter().map(|g| f64::from(g.pos.x + g.advance_width / 2.0)).collect();
            let total: f64 = glyphs.iter().map(|g| f64::from(g.advance_width)).sum();
            for (glyph, placed) in glyphs.iter().zip(along_path(&name.path, name.len, &centres, total)) {
                let Some(placed) = placed else { continue };
                self.turned_letter(glyph.chr, &font, ink, self.at(Point { x: placed.x, y: placed.y }), placed.angle as f32);
            }
        }
    }

    // ---- the flywheel ----

    /// THE CARRIAGE. The ring and its dial travel together off the map and back; the blur is on
    /// the ring alone so the dial stays sharp. Sparks are thrown into MAP space from wherever the
    /// rim is at that moment, and left behind as it goes. Gone, nothing of the ring is drawn.
    fn carriage(mut self, map: &RingMap, motion: &Motion) {
        let fly = &motion.fly;
        if fly.phase == SpinPhase::Gone {
            return;
        }
        let rpm = flywheel::rpm_of(fly);
        let exit = flywheel::exit_px(self.frame.w, map.geom.r);
        let home = self.origin;
        self.origin += vec2((fly.offset * exit) as f32, 0.0);
        let blur = flywheel::blur_px(rpm) as f32;
        if blur > 0.0 {
            // A toolkit with no blur filter: the ring drawn a few times, a blur's width apart, each
            // faint, so its edges soften the way `filter: blur()` softens them.
            let mut wires = std::mem::take(&mut self.wires);
            for (dx, dy, opacity) in [(0.0, 0.0, 0.45), (blur, 0.0, 0.3), (-blur, 0.0, 0.3), (0.0, blur, 0.3), (0.0, -blur, 0.3)] {
                let mut painter = self.painter.clone();
                painter.set_opacity(opacity);
                let copy = Paint { painter: &painter, origin: self.origin + vec2(dx, dy), wires, ..self };
                copy.ring(map);
                wires = copy.wires;
            }
        } else {
            self.ring(map);
        }
        let (cx, cy, r) = (map.geom.cx, map.geom.cyc, map.geom.r);
        let fire = flywheel::fire(rpm);
        if fire > 0.0 {
            self.flames(cx, cy, r, fire, flywheel::screen_dir(fly.omega), motion.now);
        }
        self.rpm(cx, cy, rpm);
        self.origin = home;
        for p in &fly.sparks {
            let s = flywheel::spark_streak(p, motion.now, cx, cy, r, exit);
            let color = if s.hue == 0 { theme::SANA_AMBER } else { theme::SANA_ORANGE };
            let at = |(x, y): (f64, f64)| self.at(Point { x, y });
            self.painter.line_segment([at(s.head), at(s.tail)], Stroke::new(1.5, theme::alpha(color, s.alpha as f32)));
        }
    }

    /// The ring's own dial while the flywheel has it, in the empty middle. Orange, not amber, for
    /// the hot tone: amber is a signal yellow that does not read as text on paper.
    fn rpm(&self, cx: f64, cy: f64, rpm: f64) {
        let fill = match flywheel::tone(rpm) {
            Tone::Redline | Tone::Ablaze => theme::SANA_SCARLET,
            Tone::Hot => theme::SANA_ORANGE,
            Tone::Quiet => theme::MUTED,
        };
        let at = |y: f64| self.at(Point { x: cx, y: cy + y });
        self.text(at(8.0), Set::Middle, &format!("{}", rpm.round()), theme::mono_weight(44.0, theme::Weight::Semibold), fill);
        // 11px with two pixels between the letters, as the web spaces it.
        let font = theme::sans(11.0);
        let letters: Vec<(char, f32)> = "RPM".chars().map(|ch| (ch, advance(self.painter, &ch.to_string(), 11.0) + 2.0)).collect();
        let mut x = at(30.0).x - letters.iter().map(|l| l.1).sum::<f32>() / 2.0;
        for (ch, w) in letters {
            self.text(pos2(x, at(30.0).y), Set::Start, &ch.to_string(), font.clone(), fill);
            x += w;
        }
    }

    /// THE RIM ON FIRE: a soft glow behind, then three layers of tongues standing off the rim.
    fn flames(&self, cx: f64, cy: f64, r: f64, intensity: f64, dir: f64, now: f64) {
        let color = |c: u8| match c {
            0 => theme::SANA_SCARLET,
            1 => theme::SANA_ORANGE,
            _ => theme::SANA_AMBER,
        };
        // The glow: a radial gradient, as rings of a mesh whose edges carry each stop's colour.
        let stops = flywheel::glow_stops(r, intensity);
        let mut mesh = egui::epaint::Mesh::default();
        const SEGMENTS: u32 = 96;
        for pair in stops.windows(2) {
            let [(r0, c0, a0), (r1, c1, a1)] = [pair[0], pair[1]];
            let base = mesh.vertices.len() as u32;
            for k in 0..=SEGMENTS {
                let th = f64::from(k) / f64::from(SEGMENTS) * TAU;
                let (s, c) = th.sin_cos();
                mesh.colored_vertex(self.at(Point { x: cx + r0 * s, y: cy - r0 * c }), theme::alpha(color(c0), a0 as f32));
                mesh.colored_vertex(self.at(Point { x: cx + r1 * s, y: cy - r1 * c }), theme::alpha(color(c1), a1 as f32));
            }
            for k in 0..SEGMENTS {
                let i = base + 2 * k;
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i + 1, i + 3, i + 2);
            }
        }
        self.painter.add(Shape::mesh(mesh));
        let tongues = flywheel::flame_tongues(cx, cy, r, intensity, dir, now);
        for (li, layer) in flywheel::FLAME_LAYERS.iter().enumerate() {
            let fill = theme::alpha(color(layer.color), layer.opacity as f32);
            for t in tongues.iter().filter(|t| t.layer == li) {
                let quad = |p0: (f64, f64), c: (f64, f64), p1: (f64, f64), u: f64| {
                    let v = 1.0 - u;
                    (v * v * p0.0 + 2.0 * v * u * c.0 + u * u * p1.0, v * v * p0.1 + 2.0 * v * u * c.1 + u * u * p1.1)
                };
                let mut outline = Vec::with_capacity(18);
                for k in 0..=8 {
                    outline.push(quad(t.a, t.c1, t.tip, f64::from(k) / 8.0));
                }
                for k in 1..=8 {
                    outline.push(quad(t.tip, t.c2, t.b, f64::from(k) / 8.0));
                }
                self.painter.add(Shape::convex_polygon(outline.into_iter().map(|(x, y)| self.at(Point { x, y })).collect(), fill, Stroke::NONE));
            }
        }
    }

    fn at(&self, p: Point) -> Pos2 {
        pos2(p.x as f32, p.y as f32) + self.origin
    }

    /// The selection to draw, when it lies on the molecule.
    fn selection(&self) -> Option<(i64, i64)> {
        let (start, end) = self.cursor.selection?;
        let (start, end) = (bp(start), bp(end));
        (start < self.scene.length && end < self.scene.length).then_some((start, end))
    }

    /// The caret to draw: none while a range is selected, which is drawn instead.
    fn caret(&self) -> Option<i64> {
        self.cursor.caret.map(bp).filter(|&caret| self.cursor.selection.is_none() && self.scene.length > 0 && caret <= self.scene.length)
    }

    fn selection_blue(opacity: f32) -> Color32 {
        theme::alpha(theme::SANA_BLUE, opacity)
    }

    /// The caret's line from `from` to `to`, with its dot on `to`.
    fn caret_mark(&self, from: Pos2, to: Pos2) {
        self.painter.line_segment([from, to], Stroke::new(CARET_WIDTH, theme::INK));
        self.painter.circle_filled(to, CARET_DOT, theme::INK);
    }

    fn feature(&self, i: usize) -> Option<&MapFeature> {
        self.scene.features.get(i)
    }

    /// The colour of feature `i`, and the darker twin its name and leader are drawn in.
    /// Greyed back while the Interactions layer asks about something else.
    fn colors(&self, i: usize) -> (Color32, Color32) {
        let (color, ink) = self.base_colors(i);
        (self.fade(i, color), self.fade(i, ink))
    }

    fn base_colors(&self, i: usize) -> (Color32, Color32) {
        self.feature(i).map_or((theme::HAIRLINE_STRONG, theme::INK), |f| (theme::role_color(f.role), theme::role_label_color(f.role)))
    }

    fn fade(&self, i: usize, color: Color32) -> Color32 {
        // Dim for two reasons: it is not wired to what the pointer is on, or it is LEAVING with
        // the opening. A feature mostly inside the doomed span goes with it, and saying so on the
        // map is what makes "694 bp comes out" mean something.
        if self.wires.dim.get(i).copied().unwrap_or(false) || self.leaving(i) { theme::alpha(color, DIM) } else { color }
    }

    /// Is feature `i` mostly inside the opening?
    fn leaving(&self, i: usize) -> bool {
        let Some((up, down)) = self.dim_span() else { return false };
        self.feature(i).is_some_and(|f| overlap_fraction(f.start, f.end, up, down, self.scene.length) >= DIM_AT)
    }

    /// The fill of a band or bar: a tint of the feature's colour, opaque so that nothing under it
    /// prints through the name written on top. A mark is too small to carry a name, so it is stronger.
    fn tint(&self, i: usize, mark: bool) -> Color32 {
        self.fade(i, theme::mix(self.base_colors(i).0, theme::CELL, if mark { theme::TINT_MARK } else { theme::TINT_RING }))
    }

    /// A wire in its actor's colour, faint when it is not part of what is asked.
    fn wire_color(&self, w: &Wire) -> Color32 {
        let color = w.color;
        if w.lit { color } else { theme::alpha(color, 0.1) }
    }

    /// `chords` in `renderRing`: each relationship as a curve through the ring's empty middle,
    /// from the actor's lane to the target's, its arrowhead on the target, its verb at the apex
    /// when the chord is long enough to carry it.
    fn chords(&self, map: &RingMap) {
        let length = self.scene.length;
        let lane_r = |i: usize| map.arcs.iter().find(|a| a.feature == i && !a.connector).map(|a| a.radius);
        for w in &self.wires.wires {
            let (Some(fa), Some(fb), Some(ra), Some(rb)) = (self.feature(w.a), self.feature(w.b), lane_r(w.a), lane_r(w.b)) else { continue };
            let end = |f: &MapFeature, r: f64| {
                let p = ring_point(mid_frac(f.start, f.end, length), map.center, map.geom, Some(r - map.thickness / 2.0 - 2.0));
                ChordPt { x: p.x, y: p.y }
            };
            let (a, b) = (end(fa, ra), end(fb, rb));
            let apex = chord_apex(a, b, map.geom.cx, map.geom.cyc, None);
            let color = self.wire_color(w);
            let at = |p: ChordPt| self.at(Point { x: p.x, y: p.y });
            self.painter.add(QuadraticBezierShape::from_points_stroke(
                [at(a), at(apex), at(b)],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.25, color),
            ));
            self.painter.add(Shape::convex_polygon(arrow(b, apex, 6.0).iter().map(|&p| at(p)).collect(), color, Stroke::NONE));
            if (b.x - a.x).hypot(b.y - a.y) > w.verb.chars().count() as f64 * 6.0 + 14.0 {
                self.text(at(apex) - vec2(0.0, 4.0), Set::Middle, &w.verb, theme::sans(10.5), color);
            }
        }
    }

    /// `edgeArcs` in `renderLine`: the ring's chord becomes a bracket over the stack, because a
    /// line has no interior to cross.
    fn brackets(&self, map: &LineMap) {
        let length = self.scene.length;
        let Some(highest) = map.bars.iter().map(|b| b.y).min_by(f64::total_cmp) else { return };
        let top = highest - map.thickness / 2.0 - 16.0;
        // A feature out of view has no bar, but its lane still has a height: the bracket runs to the
        // edge it lies past, as the web clamps it.
        let lane_of = |i: usize| self.scene.lanes.get(i).copied();
        let bar_y = |test: &dyn Fn(usize) -> bool| map.bars.iter().find(|b| !b.connector && test(b.feature)).map(|b| b.y - map.thickness / 2.0);
        let lane_y = |i: usize| bar_y(&|f| f == i).or_else(|| bar_y(&|f| lane_of(f).is_some() && lane_of(f) == lane_of(i)));
        let held = |x: f64| map.left.max(map.right.min(x));
        for w in &self.wires.wires {
            let (Some(fa), Some(fb), Some(ya), Some(yb)) = (self.feature(w.a), self.feature(w.b), lane_y(w.a), lane_y(w.b)) else { continue };
            let xa = held(self.line_x(map, mid_frac(fa.start, fa.end, length)));
            let xb = held(self.line_x(map, mid_frac(fb.start, fb.end, length)));
            let dir = if xb >= xa { 1.0 } else { -1.0 };
            let r = 5.0_f64.min((xb - xa).abs() / 2.0);
            let color = self.wire_color(w);
            let p = |x: f64, y: f64| self.at(Point { x, y });
            let stroke = Stroke::new(1.25, color);
            self.painter.line_segment([p(xa, ya), p(xa, top + r)], stroke);
            self.painter.add(QuadraticBezierShape::from_points_stroke(
                [p(xa, top + r), p(xa, top), p(xa + dir * r, top)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            self.painter.line_segment([p(xa + dir * r, top), p(xb - dir * r, top)], stroke);
            self.painter.add(QuadraticBezierShape::from_points_stroke(
                [p(xb - dir * r, top), p(xb, top), p(xb, top + r)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            self.painter.line_segment([p(xb, top + r), p(xb, yb)], stroke);
            let head = arrow(ChordPt { x: xb, y: yb }, ChordPt { x: xb, y: top }, 6.0);
            self.painter.add(Shape::convex_polygon(head.iter().map(|q| p(q.x, q.y)).collect(), color, Stroke::NONE));
            self.text(p((xa + xb) / 2.0, top - 4.0), Set::Middle, &w.verb, theme::sans(10.5), color);
        }
    }

    /// Text with its baseline at `at.y`.
    fn text(&self, at: Pos2, set: Set, text: &str, font: FontId, color: Color32) {
        let galley = self.painter.layout_no_wrap(text.to_owned(), font, color);
        let Some(row) = galley.rows.first() else { return };
        let width: f32 = row.row.glyphs.iter().map(|g| g.advance_width).sum();
        let baseline = row.pos.y + row.row.glyphs.first().map_or(0.0, |g| g.pos.y);
        let left = match set {
            Set::Start => at.x,
            Set::Middle => at.x - width / 2.0,
            Set::End => at.x - width,
        };
        self.painter.galley(pos2(left, at.y - baseline), galley, color);
    }

    /// Text whose lowercase letters are centred on `at.y`.
    fn text_middle(&self, at: Pos2, set: Set, text: &str, font: FontId, color: Color32) {
        self.text(at + vec2(0.0, font.size * theme::SANS_MIDDLE), set, text, font, color);
    }

    fn head(&self, i: usize, points: [Point; 3]) {
        self.painter.add(Shape::convex_polygon(points.iter().map(|p| self.at(*p)).collect(), self.colors(i).0, Stroke::NONE));
    }

    /// A name on a chip in the feature's own tint, tied to the feature by a leader in its colour.
    fn flag(&self, flag: &Flag, leader_opacity: f32) {
        let (_, ink) = self.colors(flag.feature);
        self.painter.line_segment([self.at(flag.leader.0), self.at(flag.leader.1)], Stroke::new(1.0, theme::alpha(ink, leader_opacity)));
        let chip = Rect::from_min_size(self.at(Point { x: flag.x, y: flag.y - flag.h / 2.0 }), vec2(flag.w as f32, flag.h as f32));
        self.painter.rect(
            chip,
            CornerRadius::same(3),
            self.tint(flag.feature, false),
            Stroke::new(0.75, theme::alpha(ink, 0.45)),
            StrokeKind::Middle,
        );
        self.text_middle(chip.center(), Set::Middle, &flag.text, name_font(flag.font as f32), ink);
    }

    // ---- ring ----

    fn ring(&self, map: &RingMap) {
        let center = self.at(Point { x: map.geom.cx, y: map.geom.cyc });
        // The selection first, under the axis and its ticks: a band laid over them would hide
        // the one stretch of scale just picked out to measure.
        self.ring_selection(map);
        self.painter.circle_stroke(center, map.axis_r as f32, Stroke::new(1.5, theme::HAIRLINE_STRONG));
        for tick in &map.ticks {
            let stroke = if tick.major { Stroke::new(1.0, theme::MUTED) } else { Stroke::new(0.75, theme::HAIRLINE_STRONG) };
            self.painter.line_segment([self.at(tick.inner), self.at(tick.outer)], stroke);
            if let Some(number) = &tick.number {
                let set = match number.anchor {
                    Anchor::Start => Set::Start,
                    Anchor::Middle => Set::Middle,
                    Anchor::End => Set::End,
                };
                self.text_middle(self.at(number.at), set, &number.text, theme::sans(SCALE_FONT), theme::MUTED);
            }
        }
        self.ring_dim(map);
        self.chords(map);
        for arc in &map.arcs {
            self.arc(map, arc);
        }
        self.ring_bands(map);
        for cut in &map.cuts {
            self.ring_cut(cut);
        }
        self.ring_caret(map);
        for label in &map.arc_labels {
            self.arc_label(map, label);
        }
        for flag in &map.flags {
            self.flag(flag, 0.5);
        }
    }

    /// The selected bases as a band along the axis, centred on it.
    ///
    /// One sweep, on through the origin when the selection runs through it, where the web app
    /// draws the two pieces `split_span` makes of it: two ends of a see-through band meeting at
    /// the origin add up to a line there. Everything selected is the whole circle, as a path
    /// because the toolkit strokes a circle outside its radius.
    fn ring_selection(&self, map: &RingMap) {
        let Some((start, end)) = self.selection() else { return };
        let length = self.scene.length;
        let bases = if end >= start { end - start + 1 } else { length - start + end + 1 };
        let stroke = Stroke::new(SELECTION_BAND, Self::selection_blue(SELECTION_RING));
        let mut points = self.sweep(map, start as f64 / length as f64, bases as f64 / length as f64, map.axis_r);
        if bases >= length {
            // The last point is the first again.
            points.pop();
            self.painter.add(Shape::closed_line(points, stroke));
        } else {
            self.painter.add(Shape::line(points, stroke));
        }
    }

    /// The caret: a line crossing the axis at its gap, the dot on the outer end.
    fn ring_caret(&self, map: &RingMap) {
        let Some(caret) = self.caret() else { return };
        let frac = (caret % self.scene.length) as f64 / self.scene.length as f64;
        let on_axis = ring_point(frac, map.center, map.geom, Some(map.axis_r));
        if on_axis.y > self.frame.h + OFF_FRAME || on_axis.y < -OFF_FRAME {
            return;
        }
        self.caret_mark(
            self.on_ring(map.geom, map.center, frac, map.axis_r - CARET_RING_REACH),
            self.on_ring(map.geom, map.center, frac, map.axis_r + CARET_RING_REACH),
        );
    }

    /// The centre line of an arc, as points close enough together to read as a curve.
    fn arc_points(&self, map: &RingMap, arc: &RingArc) -> Vec<Pos2> {
        self.sweep(map, arc.start_frac, arc.width_frac, arc.radius)
    }

    /// `width_frac` of a turn from `start_frac` at `radius`, as points along it.
    fn sweep(&self, map: &RingMap, start_frac: f64, width_frac: f64, radius: f64) -> Vec<Pos2> {
        let length = width_frac * TAU * radius;
        let steps = ((length / 2.0).ceil().clamp(1.0, 4096.0)) as usize;
        (0..=steps).map(|k| self.on_ring(map.geom, map.center, start_frac + width_frac * k as f64 / steps as f64, radius)).collect()
    }

    fn on_ring(&self, geom: RingGeom, center: f64, frac: f64, radius: f64) -> Pos2 {
        let p = ring_point(frac, center, geom, Some(radius));
        self.at(Point { x: p.x, y: p.y })
    }

    fn arc(&self, map: &RingMap, arc: &RingArc) {
        let points = self.arc_points(map, arc);
        if arc.connector {
            self.painter.extend(Shape::dashed_line(&points, Stroke::new(1.0, self.colors(arc.feature).0), 4.0, 3.0));
            return;
        }
        self.painter.add(Shape::line(points, Stroke::new(map.thickness as f32, self.tint(arc.feature, arc.mark))));
        if let Some(head) = arc.head {
            self.head(arc.feature, head);
        }
    }

    /// A name written along its own arc, letter by letter.
    fn arc_label(&self, map: &RingMap, label: &ArcLabel) {
        let (_, ink) = self.colors(label.feature);
        let font = name_font(label.font as f32);
        let whole = self.painter.layout_no_wrap(label.text.clone(), font.clone(), ink);
        let Some(row) = whole.rows.first() else { return };
        let glyphs = &row.row.glyphs;
        let centres: Vec<f64> = glyphs.iter().map(|g| f64::from(g.pos.x + g.advance_width / 2.0)).collect();
        let total: f64 = glyphs.iter().map(|g| f64::from(g.advance_width)).sum();
        for (glyph, placed) in glyphs.iter().zip(arc_glyphs(label, map.center, map.geom, &centres, total)) {
            self.turned_letter(glyph.chr, &font, ink, self.at(Point { x: placed.x, y: placed.y }), placed.angle as f32);
        }
    }

    /// One letter turned by `angle`, the middle of its lowercase height and of its advance at `at`.
    fn turned_letter(&self, letter: char, font: &FontId, ink: Color32, at: Pos2, angle: f32) {
        let galley = self.painter.layout_no_wrap(letter.to_string(), font.clone(), ink);
        let Some(glyph) = galley.rows.first().and_then(|row| row.row.glyphs.first()) else { return };
        // The galley's top left, measured from `at` before turning: half the advance back, and up
        // from the baseline, which sits below the middle of the lowercase letters.
        let corner = vec2(-glyph.advance_width / 2.0, font.size * theme::SANS_MIDDLE - glyph.pos.y);
        let (sin, cos) = angle.sin_cos();
        let turned = vec2(cos * corner.x - sin * corner.y, sin * corner.x + cos * corner.y);
        self.painter.add(TextShape::new(at + turned, galley, ink).with_angle(angle));
    }

    // ---- line ----

    fn line(&self, map: &LineMap) {
        let axis = |x: f64, drop: f32| self.at(Point { x, y: map.axis_y }) + vec2(0.0, drop);
        // The selection under the axis and its ticks, as on the ring.
        self.line_selection(map);
        self.painter.line_segment([axis(map.left, 0.0), axis(map.right, 0.0)], Stroke::new(1.5, theme::HAIRLINE_STRONG));
        for tick in &map.ticks {
            let (reach, stroke) =
                if tick.major { (TICK_MAJOR, Stroke::new(1.0, theme::MUTED)) } else { (TICK_MINOR, Stroke::new(0.75, theme::HAIRLINE_STRONG)) };
            self.painter.line_segment([axis(tick.x, 0.0), axis(tick.x, reach)], stroke);
            if let Some(number) = &tick.number {
                self.text(axis(tick.x, NUMBER_DROP as f32), Set::Middle, number, theme::sans(SCALE_FONT), theme::MUTED);
            }
        }
        self.line_dim(map);
        self.brackets(map);
        for bar in &map.bars {
            self.bar(map, bar);
        }
        self.line_bands(map);
        for cut in &map.cuts {
            self.line_cut(cut);
        }
        self.line_caret(map);
        for flag in &map.flags {
            self.flag(flag, 0.45);
        }
    }

    /// A cut on the ring: the hash across the backbone, and its name in the side column on a faint
    /// leader from the hash's outer end.
    fn ring_cut(&self, cut: &RingCut) {
        self.painter.line_segment([self.at(cut.tin), self.at(cut.tout)], Stroke::new(1.25, Self::cut_quiet()));
        if let Some(label) = &cut.label {
            let (a, b) = label.leader;
            self.painter.line_segment([self.at(a), self.at(b)], Stroke::new(0.75, theme::alpha(theme::MUTED, 0.4)));
            let set = match label.anchor {
                Anchor::Start => Set::Start,
                Anchor::Middle => Set::Middle,
                Anchor::End => Set::End,
            };
            self.text_middle(self.at(Point { x: label.x, y: label.y }), set, &label.text, theme::sans(CUT_FONT as f32), theme::MUTED);
        }
    }

    /// `CUT_QUIET`: at rest a cut is background, its colour tinted toward the paper.
    fn cut_quiet() -> Color32 {
        theme::mix(theme::CUT_SITE, theme::CELL, 0.55)
    }

    /// A cut: the hash in the quiet cut colour, and its name on a faint leader when it has a row.
    fn line_cut(&self, cut: &LineCut) {
        self.painter
            .line_segment([self.at(Point { x: cut.x, y: cut.y0 }), self.at(Point { x: cut.x, y: cut.y1 })], Stroke::new(1.25, Self::cut_quiet()));
        if let Some(label) = &cut.label {
            let (a, b) = label.leader;
            self.painter.line_segment([self.at(a), self.at(b)], Stroke::new(0.75, theme::alpha(theme::MUTED, 0.4)));
            self.text(self.at(Point { x: label.x, y: label.y }), Set::Middle, &label.text, theme::sans(CUT_FONT as f32), theme::MUTED);
        }
    }

    /// Where a fraction of the molecule falls along the axis; off either end of it when the
    /// fraction is out of view.
    fn line_x(&self, map: &LineMap, frac: f64) -> f64 {
        let window = line_window(Viewport { center: self.frame.center, width: self.frame.width, projection: Projection::Line });
        line_x(frac, window.left, self.frame.width, self.frame.w, map.left)
    }

    /// The selected stretch as a block straddling the axis, never thinner than a hairline and a half.
    fn line_selection(&self, map: &LineMap) {
        let Some((start, end)) = self.selection() else { return };
        let half = map.thickness / 2.0 + SELECTION_REACH;
        let held = |x: f64| map.left.max(map.right.min(x));
        for piece in split_span(start, end, self.scene.length) {
            let x0 = held(self.line_x(map, piece.start_frac));
            let x1 = held(self.line_x(map, piece.start_frac + piece.width_frac));
            let block = Rect::from_min_max(
                self.at(Point { x: x0, y: map.axis_y - half }),
                self.at(Point { x: x0 + (x1 - x0).max(1.5), y: map.axis_y + half }),
            );
            self.painter.rect_filled(block, CornerRadius::same(2), Self::selection_blue(SELECTION_LINE));
        }
    }

    /// The opening, washed over: what is leaving, said on the molecule itself, so that "694 bp
    /// comes out" means something you can see.
    fn ring_dim(&self, map: &RingMap) {
        let Some((up, down)) = self.dim_span() else { return };
        let length = self.scene.length;
        let removed = if down >= up { down - up } else { length - up + down };
        if removed <= 0 {
            return;
        }
        let stroke = Stroke::new(SELECTION_BAND, theme::alpha(theme::CELL, 0.55));
        let points = self.sweep(map, up as f64 / length as f64, removed as f64 / length as f64, map.axis_r);
        self.painter.add(Shape::line(points, stroke));
    }

    /// The same wash, unrolled: straightening the map must not take the picture away.
    fn line_dim(&self, map: &LineMap) {
        let Some((up, down)) = self.dim_span() else { return };
        let length = self.scene.length;
        let half = map.thickness / 2.0 + SELECTION_REACH;
        let held = |x: f64| map.left.max(map.right.min(x));
        for piece in split_span(up, (down - 1 + length) % length, length) {
            let x0 = held(self.line_x(map, piece.start_frac));
            let x1 = held(self.line_x(map, piece.start_frac + piece.width_frac));
            let block = Rect::from_min_max(
                self.at(Point { x: x0, y: map.axis_y - half }),
                self.at(Point { x: x0 + (x1 - x0).max(1.5), y: map.axis_y + half }),
            );
            self.painter.rect_filled(block, CornerRadius::same(2), theme::alpha(theme::CELL, 0.55));
        }
    }

    /// The opening to wash, when there is one that lies on the molecule.
    fn dim_span(&self) -> Option<(i64, i64)> {
        let (up, down) = self.overlay.dim?;
        let length = self.scene.length;
        (length > 0 && up.rem_euclid(length) != down.rem_euclid(length)).then(|| (up.rem_euclid(length), down.rem_euclid(length)))
    }

    /// The band round the ring: the arms on backbone that stays, the body over the opening, one
    /// run per piece to order, and a tick with its temperature at every junction.
    fn ring_bands(&self, map: &RingMap) {
        let length = self.scene.length;
        if length <= 0 {
            return;
        }
        let band_r = map.axis_r + BAND_R;
        // Two tiers, because the pieces OVERLAP: drawn on one row the shared stretch would read as
        // a single longer piece, which is the one thing about an assembly you must not draw.
        let tier_r = |t: usize| band_r + t as f64 * (f64::from(BAND_STROKE) + 2.0);
        for band in self.overlay.bands {
            let laid = layout_band(band, length, None);
            let run = |pieces: &[Piece], color: Color32, tier: usize| {
                for piece in pieces {
                    let points = self.sweep(map, piece.start_frac, piece.width_frac, tier_r(tier));
                    self.painter.add(Shape::line(points, Stroke::new(BAND_STROKE, color)));
                }
            };
            if laid.segments.is_empty() {
                // No design yet: a body and its arms is the honest picture of "something goes
                // here, I have not worked out what to order".
                run(&laid.arm_left, band_arm(band), 0);
                run(&laid.arm_right, band_arm(band), 0);
                run(&laid.body, band_body(band), 0);
            } else {
                // The pieces CARRY the arms — that is what makes them orderable — so the arms are
                // already on screen inside them, and drawing them again would double the picture.
                for segment in &laid.segments {
                    run(&segment.pieces, band_color(&segment.color), segment.tier);
                }
            }
            self.ring_seams(map, &laid, band_r);
        }
    }

    /// A tick at every junction at every zoom — you can always see THAT there is one — and the
    /// temperature once there is room to read it, staggered so neighbours do not collide.
    fn ring_seams(&self, map: &RingMap, laid: &BandLayout, band_r: f64) {
        let outer_tier = laid.segments.iter().any(|s| s.tier > 0);
        let top_r = band_r + if outer_tier { f64::from(BAND_STROKE) + 2.0 } else { 0.0 };
        for (i, seam) in laid.seams.iter().enumerate() {
            let room = seam_room(&laid.seams, i) * TAU * band_r;
            let inner = self.on_ring(map.geom, map.center, seam.frac, band_r - f64::from(BAND_STROKE) / 2.0 - 2.0);
            let outer = self.on_ring(map.geom, map.center, seam.frac, top_r + f64::from(BAND_STROKE) / 2.0 + 2.0);
            self.painter.line_segment([inner, outer], Stroke::new(1.25, theme::alpha(theme::INK, 0.75)));
            if room < SEAM_ROOM {
                continue;
            }
            // Two radii, so junctions a few pixels apart can all say their temperature.
            let label_r = top_r + f64::from(BAND_STROKE) / 2.0 + if i % 2 == 1 { 30.0 } else { 14.0 };
            let lead = self.on_ring(map.geom, map.center, seam.frac, label_r - 7.0);
            let at = self.on_ring(map.geom, map.center, seam.frac, label_r);
            self.painter.line_segment([outer, lead], Stroke::new(0.75, theme::alpha(theme::INK, 0.35)));
            self.halo_text(at, &seam_text(seam));
        }
    }

    /// The ring's band, unrolled: the same spans as bars above the axis.
    fn line_bands(&self, map: &LineMap) {
        let length = self.scene.length;
        if length <= 0 {
            return;
        }
        let band_y = map.axis_y - BAND_LINE_Y;
        let held = |x: f64| map.left.max(map.right.min(x));
        for band in self.overlay.bands {
            let laid = layout_band(band, length, None);
            let run = |pieces: &[Piece], color: Color32, tier: usize| {
                for piece in pieces {
                    let x0 = held(self.line_x(map, piece.start_frac));
                    let x1 = held(self.line_x(map, piece.start_frac + piece.width_frac));
                    let top = band_y - f64::from(BAND_STROKE) / 2.0 - tier as f64 * (f64::from(BAND_STROKE) + 2.0);
                    let bar = Rect::from_min_max(
                        self.at(Point { x: x0, y: top }),
                        self.at(Point { x: x0 + (x1 - x0).max(1.5), y: top + f64::from(BAND_STROKE) }),
                    );
                    self.painter.rect_filled(bar, CornerRadius::same(2), color);
                }
            };
            if laid.segments.is_empty() {
                run(&laid.arm_left, band_arm(band), 0);
                run(&laid.arm_right, band_arm(band), 0);
                run(&laid.body, band_body(band), 0);
            } else {
                for segment in &laid.segments {
                    run(&segment.pieces, band_color(&segment.color), segment.tier);
                }
            }
            self.line_seams(map, &laid, band_y);
        }
    }

    fn line_seams(&self, map: &LineMap, laid: &BandLayout, band_y: f64) {
        for (i, seam) in laid.seams.iter().enumerate() {
            let x = self.line_x(map, seam.frac);
            if x < map.left - 20.0 || x > map.right + 20.0 {
                continue;
            }
            let gap = laid
                .seams
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != i)
                .map(|(_, other)| (self.line_x(map, other.frac) - x).abs())
                .fold(f64::INFINITY, f64::min);
            let top = band_y - f64::from(BAND_STROKE) / 2.0 - 3.0;
            let bottom = band_y + f64::from(BAND_STROKE) / 2.0 + 3.0;
            self.painter
                .line_segment([self.at(Point { x, y: top }), self.at(Point { x, y: bottom })], Stroke::new(1.25, theme::alpha(theme::INK, 0.75)));
            if gap >= SEAM_ROOM {
                let drop = if i % 2 == 1 { 22.0 } else { 8.0 };
                self.halo_text(self.at(Point { x, y: band_y - f64::from(BAND_STROKE) / 2.0 - drop }), &seam_text(seam));
            }
        }
    }

    /// Text painted over whatever is behind it, stroke first. A seam's temperature belongs to its
    /// junction, not to a feature, so it never goes through the map's placement pass and has to
    /// stay legible where a leader or a name runs underneath.
    fn halo_text(&self, at: Pos2, text: &str) {
        let font = theme::sans(10.0);
        for dx in [-1.0f32, 0.0, 1.0] {
            for dy in [-1.0f32, 0.0, 1.0] {
                if dx != 0.0 || dy != 0.0 {
                    self.text_middle(at + vec2(dx, dy), Set::Middle, text, font.clone(), theme::CELL);
                }
            }
        }
        self.text_middle(at, Set::Middle, text, font, theme::alpha(theme::INK, 0.9));
    }

    /// The caret: a line through the axis at its gap, the dot on top.
    fn line_caret(&self, map: &LineMap) {
        let Some(caret) = self.caret() else { return };
        // Not taken round the length as on the ring, which is what the web app does here too and
        // which puts a caret after the last base of a linear molecule before its first.
        let x = self.line_x(map, caret as f64 / self.scene.length as f64);
        if x < map.left - 1e-3 || x > map.right + 1e-3 {
            return;
        }
        let reach = map.thickness / 2.0 + CARET_LINE_REACH;
        self.caret_mark(self.at(Point { x, y: map.axis_y + reach }), self.at(Point { x, y: map.axis_y - reach }));
    }

    fn bar(&self, map: &LineMap, bar: &LineBar) {
        let (color, ink) = self.colors(bar.feature);
        let (left, right) = (self.at(Point { x: bar.x0, y: bar.y }), self.at(Point { x: bar.x1, y: bar.y }));
        if bar.connector {
            self.painter.extend(Shape::dashed_line(&[left, right], Stroke::new(1.0, color), 4.0, 3.0));
            return;
        }
        let half = vec2(0.0, map.thickness as f32 / 2.0);
        let corner = CornerRadius::same(if bar.mark { 1 } else { 2 });
        self.painter.rect_filled(Rect::from_min_max(left - half, right + half), corner, self.tint(bar.feature, bar.mark));
        if let Some(head) = bar.head {
            self.head(bar.feature, head);
        }
        if let Some(name) = &bar.name {
            self.text_middle(left + (right - left) / 2.0, Set::Middle, name, name_font(map.lane_font as f32), ink);
        }
    }
}

/// How far the nearest other seam is, as a fraction of the molecule: the room this one's label
/// has. Staggering means a label only has to clear the one two seams away.
fn seam_room(seams: &[SeamMark], i: usize) -> f64 {
    seams
        .iter()
        .enumerate()
        .filter(|(k, _)| *k != i)
        .map(|(_, other)| (((other.frac - seams.get(i).map_or(0.0, |s| s.frac) + 1.5) % 1.0) - 0.5).abs())
        .fold(1.0, f64::min)
}

fn seam_text(seam: &SeamMark) -> String {
    format!("{:.1}°C", seam.tm)
}

/// A band's body colour, and the arms', as the caller named them. The names are the app's own
/// tokens, so an unknown one falls back rather than drawing nothing.
fn band_body(band: &OuterBand) -> Color32 {
    band_color(&band.color)
}

fn band_arm(band: &OuterBand) -> Color32 {
    band.arm_color.as_deref().map_or(theme::ARM, band_color)
}

/// `#rrggbb`, or one of the app's own colour names.
fn band_color(name: &str) -> Color32 {
    if let Some(hex) = name.strip_prefix('#')
        && hex.len() == 6
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        return Color32::from_rgb((rgb >> 16) as u8, ((rgb >> 8) & 0xff) as u8, (rgb & 0xff) as u8);
    }
    match name {
        "insert" => theme::INSERT,
        "arm" => theme::ARM,
        _ => theme::HAIRLINE_STRONG,
    }
}

#[cfg(test)]
mod tests {
    use super::{Wires, scene_of};

    const FILE: &str = "LOCUS       t                         40 bp    DNA     circular SYN 01-JAN-2026\n\
FEATURES             Location/Qualifiers\n     CDS             3..12\n                     /label=\"pol\"\n                     \
/note=\"bios-id: a\"\n                     /note=\"bios-act: transcribes > b, construct\"\n     promoter        20..30\n                     \
/label=\"pT7\"\n                     /note=\"bios-id: b\"\n     terminator      32..36\n                     /label=\"stop\"\n\
ORIGIN\n        1 acgtggatcc gatcaaaaac ccccgggggt tttttacgta\n//\n";

    #[test]
    fn a_hand_back_flies_the_map_out_to_the_stretch_the_strip_was_on() {
        use bios_layout::overview::navigate::ease_zoom;
        use bios_layout::overview::viewport::{Projection, Viewport};
        let at = Viewport { center: 0.2, width: 0.019, projection: Projection::Line };
        let mut motion = super::Motion::spinning();
        motion.focus(at, 0.3, 0.04, 0.0);
        let target = motion.zoom.unwrap();
        let mut v = at;
        for _ in 0..60 {
            v = ease_zoom(v, target, 1.0 / 60.0, false).0;
        }
        assert!((v.width - 0.04).abs() < 1e-12, "{v:?}");
        assert!((v.center - 0.3).abs() < 1e-9, "the stretch rides the zoom as its anchor: {v:?}");
    }

    #[test]
    fn the_map_wires_feature_to_feature_and_greys_back_what_the_lit_feature_is_not_wired_to() {
        let c = bios_core::genbank::read(FILE).unwrap();
        let scene = scene_of(&c);
        let at_rest = Wires::of(&c, &scene, None);
        assert_eq!(at_rest.wires.len(), 1, "the whole construct is never an endpoint on the map");
        assert_eq!(at_rest.wires[0].verb, "transcribes");
        assert!(at_rest.wires[0].lit && at_rest.dim.iter().all(|d| !d), "with nothing lit, everything is");
        let pointing = Wires::of(&c, &scene, Some(1));
        let dimmed: Vec<&str> = scene.features.iter().zip(&pointing.dim).filter(|(_, d)| **d).map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(dimmed, ["stop"]);
        assert!(!Wires::of(&c, &scene, Some(2)).wires[0].lit, "an edge away from the lit feature steps back");
    }
}
