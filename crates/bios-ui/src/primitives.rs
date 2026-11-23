//! The web app's pressables, from `design/primitives/button.css`: the ghost button and the solid
//! one, the segmented group, and the glass they take on when they float on the artifact.
//!
//! The web's rule (button.css, top): GLASS IS FOR THINGS FLOATING ON THE ARTIFACT. Outside the one
//! cell a button is a ghost (transparent, a hairline ring, filling with paper on hover); inside it
//! (`.oge-cell .oge-btn`) the same button is glass. Here the caller says which with [`Material`].

use egui::epaint::{CornerRadius, RectShape};
use egui::{Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::icon::{self, Node};
use crate::theme::{self, Shadow, Weight};

/// Where a pressable sits, which decides what it is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    /// Off the artifact: transparent with a hairline ring (`.oge-btn`).
    Ghost,
    /// On the artifact (`.oge-cell .oge-btn`): white at 62%, a white ring at 70%, lifted.
    Glass,
    /// Glass made opaque, for a group floating over letters: the editor pane's tool row
    /// (`.editor-pane > .canvas-tools .oge-btn-group { background: var(--float) }`).
    Float,
}

/// `.oge-btn`'s face: solid for the one action that matters, ghost otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ghost,
    Solid,
}

/// Paints CSS `box-shadow` layers under a rounded rect.
pub fn shadow(painter: &Painter, rect: Rect, radius: f32, layers: &[Shadow]) {
    for layer in layers {
        let r = rect.translate(vec2(0.0, layer.y)).expand(layer.spread);
        let corner = (radius + layer.spread).max(0.0);
        painter.add(RectShape::filled(r, CornerRadius::same(corner.min(255.0) as u8), layer.color).with_blur_width(layer.blur));
    }
}

fn corner(radius: f32, rect: Rect) -> CornerRadius {
    CornerRadius::same(radius.min(rect.height() / 2.0).min(255.0) as u8)
}

/// The glass ground (button.css `.oge-cell .oge-btn, .oge-cell .oge-btn-group`). The backdrop blur
/// is not drawn: what it hides is the map's ink, which at 62% white is already faint.
pub fn glass(painter: &Painter, rect: Rect, radius: f32) {
    glass_over(painter, rect, radius, Color32::from_rgba_unmultiplied(255, 255, 255, 158));
}

/// The glass's ring, highlight and lift over `ground`.
fn glass_over(painter: &Painter, rect: Rect, radius: f32, ground: Color32) {
    let r = corner(radius, rect);
    shadow(painter, rect, radius.min(rect.height() / 2.0), theme::SHADOW_GLASS);
    painter.rect_filled(rect, r, ground);
    painter.rect_stroke(rect, r, Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 179)), StrokeKind::Inside);
    // `inset 0 1px 0 rgb(255 255 255 / 60%)`: a highlight along the inside of the top edge.
    let top = Rect::from_min_max(
        rect.min + vec2(radius.min(rect.height() / 2.0), 1.0),
        pos2(rect.max.x - radius.min(rect.height() / 2.0), rect.min.y + 2.0),
    );
    painter.rect_filled(top, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 153));
}

/// What a button shows.
#[derive(Clone, Copy)]
pub enum Face<'a> {
    Label(&'a str),
    Icon(&'a [Node], f32),
}

/// One `.oge-btn`, standing alone.
pub struct Button<'a> {
    pub face: Face<'a>,
    pub material: Material,
    pub tone: Tone,
    /// `.oge-btn-sm`: 28 high, 12px type.
    pub small: bool,
    pub active: bool,
    pub enabled: bool,
}

impl<'a> Button<'a> {
    pub fn label(text: &'a str, material: Material) -> Self {
        Button { face: Face::Label(text), material, tone: Tone::Ghost, small: false, active: false, enabled: true }
    }

    pub fn icon(nodes: &'a [Node], size: f32, material: Material) -> Self {
        Button { face: Face::Icon(nodes, size), material, tone: Tone::Ghost, small: false, active: false, enabled: true }
    }

    fn height(&self) -> f32 {
        if self.small { 28.0 } else { 34.0 }
    }

    fn font(&self) -> FontId {
        theme::sans_weight(if self.small { 12.0 } else { 13.0 }, Weight::Book)
    }

    /// Its width: an icon button is square (`.oge-btn-icon`), a label is padded 15 (12 small).
    pub fn width(&self, ui: &Ui) -> f32 {
        match self.face {
            Face::Icon(..) => self.height(),
            Face::Label(text) => {
                let pad = if self.small { 12.0 } else { 15.0 };
                text_width(ui, text, self.font()) + 2.0 * pad
            }
        }
    }

    /// Draws the button with its top-left at `at`.
    pub fn show_at(self, ui: &mut Ui, at: Pos2) -> Response {
        let rect = Rect::from_min_size(at, vec2(self.width(ui), self.height()));
        let sense = if self.enabled { Sense::click() } else { Sense::hover() };
        let response = ui.allocate_rect(rect, sense);
        let hovered = self.enabled && response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let lit = if self.active { 1.0 } else { hover_amount(ui, response.id, hovered) };
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        let mut ink = if self.enabled { theme::MUTED } else { theme::alpha(theme::MUTED, 0.4) };
        match (self.tone, self.material) {
            (Tone::Solid, material) => {
                if material == Material::Glass {
                    shadow(painter, rect, radius, theme::SHADOW_1);
                }
                painter.rect_filled(rect, corner(radius, rect), theme::mix(theme::ACCENT_HOVER, theme::ACCENT, lit));
                ink = theme::ON_ACCENT;
            }
            (Tone::Ghost, Material::Ghost) => {
                if lit > 0.0 {
                    painter.rect_filled(rect, corner(radius, rect), theme::alpha(theme::CELL, lit));
                }
                painter.rect_stroke(rect, corner(radius, rect), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
                if self.enabled {
                    ink = theme::mix(theme::INK, theme::MUTED, lit);
                }
            }
            (Tone::Ghost, Material::Glass | Material::Float) => {
                if self.active {
                    shadow(painter, rect, radius, theme::SHADOW_1);
                    painter.rect_filled(rect, corner(radius, rect), theme::FLOAT);
                } else if self.material == Material::Float {
                    glass_over(painter, rect, radius, theme::FLOAT);
                } else {
                    glass(painter, rect, radius);
                }
                if self.enabled {
                    ink = theme::mix(theme::INK, theme::MUTED, lit);
                }
            }
        }
        face(painter, rect, self.face, self.font(), ink);
        response
    }
}

fn face(painter: &Painter, rect: Rect, face: Face, font: FontId, ink: Color32) {
    match face {
        Face::Label(text) => {
            painter.text(rect.center(), Align2::CENTER_CENTER, text, font, ink);
        }
        Face::Icon(nodes, size) => icon::paint(painter, rect.center(), size, nodes, ink),
    }
}

/// How long a control takes to take on its hover state: `transition: … 0.12s` on `.oge-btn`
/// (`design/primitives/button.css` 51) and `.sr-verb` (`styles.css` 5915).
pub const HOVER_SECONDS: f32 = 0.12;

/// CSS's `ease`, `cubic-bezier(0.25, 0.1, 0.25, 1)`: how far along the change is at `t` of its time.
pub fn css_ease(t: f32) -> f32 {
    cubic_bezier((0.25, 0.1), (0.25, 1.0), t)
}

/// CSS's `ease-out`, `cubic-bezier(0, 0, 0.58, 1)`.
pub fn css_ease_out(t: f32) -> f32 {
    cubic_bezier((0.0, 0.0), (0.58, 1.0), t)
}

/// A CSS `cubic-bezier` timing curve through `(x1, y1)` and `(x2, y2)`, read at `t`.
fn cubic_bezier((x1, y1): (f32, f32), (x2, y2): (f32, f32), t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return t.clamp(0.0, 1.0);
    }
    let bezier = |a: f32, b: f32, u: f32| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u) + u.powi(3);
    // The curve's x rises with u, so u is found by halving; y is then read off at that u.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if bezier(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(y1, y2, (lo + hi) / 2.0)
}

/// How far a floating thing is into its entrance, 0 to 1, over `seconds` with `ease-out`: it
/// starts again whenever the thing was not drawn in the pass before, as a CSS animation does
/// each time its element mounts (`sr-in`, `feature-card-in` in `styles.css`).
pub fn entrance(ctx: &egui::Context, id: egui::Id, seconds: f32) -> f32 {
    let key = id.with("entrance");
    let (pass, now) = (ctx.cumulative_pass_nr(), ctx.input(|i| i.time));
    let start = match ctx.data(|d| d.get_temp::<(u64, f64)>(key)) {
        Some((last, start)) if last == pass || last + 1 == pass => start,
        _ => now,
    };
    ctx.data_mut(|d| d.insert_temp(key, (pass, start)));
    let t = ((now - start) as f32 / seconds.max(f32::EPSILON)).min(1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    css_ease_out(t)
}

/// How far into its hover state the control `id` is, 0 to 1, easing over [`HOVER_SECONDS`].
pub fn hover_amount(ui: &Ui, id: egui::Id, hovered: bool) -> f32 {
    ui.ctx().animate_bool_with_time_and_easing(id.with("hover"), hovered, HOVER_SECONDS, css_ease)
}

pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x)
}

/// One segment of an `.oge-btn-group`.
pub struct Segment<'a> {
    pub face: Face<'a>,
    pub active: bool,
    pub enabled: bool,
    /// The tooltip, with its shortcut as the web writes it ("Back ⌘Z").
    pub tip: &'a str,
}

/// The two sizes of `.oge-btn-group`: the standard one, and `.oge-btn-group-sm` (28 high, its
/// segments 24 high, 2 apart, 12px type padded 12).
#[derive(Clone, Copy)]
struct Sizes {
    height: f32,
    segment: f32,
    gap: f32,
    font: f32,
    pad: f32,
}

const STANDARD: Sizes = Sizes { height: 34.0, segment: 30.0, gap: 1.0, font: 13.0, pad: 15.0 };
const SMALL: Sizes = Sizes { height: 28.0, segment: 24.0, gap: 2.0, font: 12.0, pad: 12.0 };

/// The size of one segment (`.oge-btn-group .oge-btn`, and `.oge-btn-group .oge-btn-icon` for an
/// icon: `width: auto; padding: 0 10px`). The editor's own segments are fixed 30×30
/// (`.canvas-tools .oge-btn-group .oge-btn`), which the caller asks for with `square`.
fn segment_width(ui: &Ui, face: Face, square: bool, sizes: Sizes) -> f32 {
    match face {
        Face::Icon(_, size) => {
            if square {
                sizes.segment
            } else {
                size + 20.0
            }
        }
        Face::Label(text) => text_width(ui, text, theme::sans_weight(sizes.font, Weight::Book)) + 2.0 * sizes.pad,
    }
}

fn width_of(ui: &Ui, segments: &[Segment], square: bool, sizes: Sizes) -> f32 {
    let inner: f32 = segments.iter().map(|s| segment_width(ui, s.face, square, sizes)).sum();
    inner + (segments.len().saturating_sub(1)) as f32 * sizes.gap + 4.0 + 2.0
}

/// The width an `.oge-btn-group` of `segments` takes: 2 padding each side, 1 between.
pub fn group_width(ui: &Ui, segments: &[Segment], square: bool) -> f32 {
    width_of(ui, segments, square, STANDARD)
}

/// The width an `.oge-btn-group-sm` of `segments` takes.
pub fn group_small_width(ui: &Ui, segments: &[Segment]) -> f32 {
    width_of(ui, segments, false, SMALL)
}

/// An `.oge-btn-group` with its top-left at `at`: one pill, 34 high, its segments 30 high inside,
/// the chosen one lifting out as a chip. Returns which segment was clicked.
pub fn group(ui: &mut Ui, at: Pos2, segments: &[Segment], material: Material, square: bool) -> Option<usize> {
    group_sized(ui, at, segments, material, square, STANDARD, None)
}

/// An `.oge-btn-group-sm` (with `size="sm"` buttons) with its top-left at `at`.
pub fn group_small(ui: &mut Ui, at: Pos2, segments: &[Segment], material: Material) -> Option<usize> {
    group_sized(ui, at, segments, material, false, SMALL, None)
}

/// A small group `width` wide whose segments share it evenly, in 11px type, so it reads as one
/// control (`.enz-tools .oge-btn-group .oge-btn { flex: 1; font-size: 11px }`).
pub fn group_small_even(ui: &mut Ui, at: Pos2, width: f32, segments: &[Segment], material: Material) -> Option<usize> {
    group_sized(ui, at, segments, material, false, Sizes { font: 11.0, pad: 4.0, ..SMALL }, Some(width))
}

fn group_sized(ui: &mut Ui, at: Pos2, segments: &[Segment], material: Material, square: bool, sizes: Sizes, even: Option<f32>) -> Option<usize> {
    let width = even.unwrap_or_else(|| width_of(ui, segments, square, sizes));
    let even_w = even.map(|w| (w - 6.0 - segments.len().saturating_sub(1) as f32 * sizes.gap) / segments.len().max(1) as f32);
    let rect = Rect::from_min_size(at, vec2(width, sizes.height));
    let radius = sizes.height / 2.0;
    let chip = sizes.segment / 2.0;
    {
        let painter = ui.painter();
        match material {
            Material::Ghost => {
                painter.rect_stroke(rect, corner(radius, rect), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
            }
            Material::Glass => glass(painter, rect, radius),
            Material::Float => glass_over(painter, rect, radius, theme::FLOAT),
        }
    }
    let mut clicked = None;
    // 1px border + 2px padding, then the segments, centred vertically.
    let mut x = rect.min.x + 1.0 + 2.0;
    let top = rect.min.y + (sizes.height - sizes.segment) / 2.0;
    for (i, segment) in segments.iter().enumerate() {
        let w = even_w.unwrap_or_else(|| segment_width(ui, segment.face, square, sizes));
        let r = Rect::from_min_size(pos2(x, top), vec2(w, sizes.segment));
        let sense = if segment.enabled { Sense::click() } else { Sense::hover() };
        let response = ui.allocate_rect(r, sense);
        let hovered = segment.enabled && response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let painter = ui.painter();
        if segment.active {
            match material {
                Material::Glass | Material::Float => {
                    shadow(painter, r, chip, theme::SHADOW_1);
                    painter.rect_filled(r, corner(chip, r), theme::FLOAT);
                }
                Material::Ghost => {
                    painter.rect_filled(r, corner(chip, r), theme::CELL);
                }
            }
        }
        let ink = if !segment.enabled {
            theme::alpha(theme::MUTED, 0.4)
        } else if segment.active {
            theme::INK
        } else {
            theme::mix(theme::INK, theme::MUTED, hover_amount(ui, response.id, hovered))
        };
        face(painter, r, segment.face, theme::sans_weight(sizes.font, Weight::Book), ink);
        let response = if segment.tip.is_empty() { response } else { response.on_hover_text(segment.tip) };
        if response.clicked() {
            clicked = Some(i);
        }
        x += w + sizes.gap;
    }
    clicked
}

/// A list row (`.rail-file`, `.rail-nav-item`): `--row-hover` under the pointer, `--row-active`
/// when it is the open one, at `--radius-sm`.
pub fn row_ground(painter: &Painter, rect: Rect, hovered: bool, active: bool) {
    let fill = if active {
        theme::ROW_ACTIVE
    } else if hovered {
        theme::ROW_HOVER
    } else {
        return;
    };
    painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM as u8), fill);
}

/// The focus ring every `:focus-visible` rule draws: `outline: 2px solid var(--focus)`, offset
/// `offset` (1 on fields, -2 inset on rail rows).
pub fn focus_ring(painter: &Painter, rect: Rect, radius: f32, offset: f32) {
    let r = rect.expand(offset);
    painter.rect_stroke(r, CornerRadius::same((radius + offset).max(0.0) as u8), Stroke::new(2.0, theme::FOCUS), StrokeKind::Outside);
}

#[cfg(test)]
mod tests {
    use super::{css_ease, css_ease_out};

    #[test]
    fn the_hover_eases_as_a_browsers_ease_does() {
        assert_eq!((css_ease(0.0), css_ease(1.0)), (0.0, 1.0));
        // `ease` is most of the way there by half its time: 0.8024 at 0.5, 0.4085 at 0.25.
        assert!((css_ease(0.5) - 0.8024).abs() < 0.002, "{}", css_ease(0.5));
        assert!((css_ease(0.25) - 0.4085).abs() < 0.002, "{}", css_ease(0.25));
        // `ease-out` at half its time.
        assert!((css_ease_out(0.5) - 0.6846).abs() < 0.002, "{}", css_ease_out(0.5));
    }
}
