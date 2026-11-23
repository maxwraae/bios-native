//! The side panel the Contents and Enzymes lists stand in (`.inspector`), and the two shapes every
//! list in it is made of: a collapsible section with a count and an optional show-on-map switch,
//! and a row whose name has the width. Ported from Bios `editor/Inspector.tsx`,
//! `leftpanel/propParts.tsx` and `styles.css` (`.inspector*`, lines 6944-6995; `.prop-*`,
//! 3110-3300).
//!
//! Only one side panel is on screen at a time: they take the same 290px on the pane's right.

use egui::{Align2, Area, Color32, CornerRadius, CursorIcon, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::primitives::{shadow, text_width};
use crate::theme::{self, Weight};
use crate::{icon, lucide};

/// Which side panel is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Contents,
    Enzymes,
}

/// `.inspector`'s width.
pub const WIDTH: f32 = 290.0;
/// `.inspector-head`: 10px padding round a 23px close button, and its rule.
const HEAD_H: f32 = 10.0 + 23.0 + 10.0 + 1.0;
/// `.prop-section-header`: 9px padding round a 12px label.
pub const SECTION_H: f32 = 9.0 + 15.0 + 9.0;
/// `.prop-row`: 5px padding round a 12.5px name.
pub const ROW_H: f32 = 5.0 + 15.0 + 5.0;
/// `.prop-section-body`'s ceiling: a long list scrolls inside its own section.
const SECTION_BODY_MAX: f32 = 320.0;

/// Where the panel stands in the editor pane: under the head band, 12 from the right, down to the
/// tool row's foot (`top: calc(var(--head-h) + 8px); right: 12px; bottom: floor + air`).
pub fn place(pane: Rect, tools_bottom: f32) -> Rect {
    let top = pane.top() + theme::HEAD_H + 8.0;
    let bottom = tools_bottom.max(top + 120.0);
    Rect::from_min_max(pos2(pane.right() - 12.0 - WIDTH, top), pos2(pane.right() - 12.0, bottom))
}

/// Draws the panel's card and head in `rect`, then its body through `body`, given the body's own
/// rectangle. Returns true when its close button was pressed.
pub fn show(ui: &mut Ui, rect: Rect, id: &str, title: &str, body: impl FnOnce(&mut Ui, Rect)) -> bool {
    let mut closed = false;
    Area::new(Id::unique(("side-panel", id))).order(Order::Middle).fixed_pos(rect.min).fade_in(false).show(ui.ctx(), |ui| {
        ui.set_min_size(rect.size());
        let painter = ui.painter().clone();
        let r = CornerRadius::same(theme::RADIUS as u8);
        shadow(&painter, rect, theme::RADIUS, theme::SHADOW_CELL);
        painter.rect_filled(rect, r, theme::CELL);
        painter.rect_stroke(rect, r, Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Outside);
        // Presses on the card's own ground stay on the card.
        ui.allocate_rect(rect, Sense::click());
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD_H));
        painter.text(
            pos2(head.left() + 14.0, head.center().y - 0.5),
            Align2::LEFT_CENTER,
            title,
            theme::sans_weight(13.0, Weight::Semibold),
            theme::INK,
        );
        let close = Rect::from_center_size(pos2(head.right() - 10.0 - 11.5, head.center().y - 0.5), vec2(23.0, 23.0));
        let response = ui.allocate_rect(close, Sense::click()).on_hover_text("Close");
        let hovered = response.hovered();
        if hovered {
            painter.rect_filled(close, CornerRadius::same(12), theme::CELL_HOVER);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        icon::paint(&painter, close.center(), 15.0, lucide::PANEL_RIGHT_CLOSE, if hovered { theme::INK } else { theme::MUTED });
        closed = response.clicked();
        painter.hline(rect.left()..=rect.right(), head.bottom() - 0.5, Stroke::new(1.0, theme::HAIRLINE));
        let inner = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
            ui.set_clip_rect(inner.shrink2(vec2(0.0, 0.0)).intersect(rect.shrink(1.0)));
            body(ui, inner);
        });
    });
    closed
}

/// What a section's header was pressed for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header {
    pub toggled: bool,
    pub visibility: bool,
}

/// A section's header across the width of `ui`, laid out next: chevron, label, count and, when
/// `visible` is given, the show-on-map switch (`Section`).
pub fn header(ui: &mut Ui, label: &str, count: Option<usize>, open: bool, visible: Option<bool>) -> Header {
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, SECTION_H), Sense::hover());
    let switch_w = if visible.is_some() { 6.0 + 24.0 + 12.0 } else { 0.0 };
    let expand = Rect::from_min_max(row.min, pos2(row.right() - switch_w, row.bottom()));
    let expand_response = ui.interact(expand, ui.scope_id().with(("section", label)), Sense::click());
    let switch = Rect::from_min_max(pos2(expand.right(), row.top()), row.max);
    let switch_response = visible.map(|on| {
        let tip = format!("{} {} on map", if on { "Hide" } else { "Show" }, label.to_lowercase());
        ui.interact(switch, ui.scope_id().with(("section-vis", label)), Sense::click()).on_hover_text(tip)
    });
    let painter = ui.painter();
    if ui.rect_contains_pointer(row) {
        painter.rect_filled(row, 0.0, theme::CELL_HOVER);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let chevron_at = pos2(row.left() + 12.0 + 6.5, row.center().y);
    let chevron_ink = if open { theme::ACCENT } else { theme::MUTED };
    let nodes = if open { lucide::CHEVRON_DOWN } else { lucide::CHEVRON_RIGHT };
    icon::paint(painter, chevron_at, 13.0, nodes, chevron_ink);
    let label_ink = if open { theme::INK } else { theme::MUTED };
    let label_x = row.left() + 12.0 + 13.0 + 6.0;
    painter.text(pos2(label_x, row.center().y), Align2::LEFT_CENTER, label, theme::sans_weight(12.0, Weight::Medium), label_ink);
    if let Some(count) = count {
        let text = count.to_string();
        let w = text_width(ui, &text, theme::mono(11.0)) + 14.0;
        let pill = Rect::from_min_size(pos2(expand.right() - 4.0 - w, row.center().y - 8.0), vec2(w, 16.0));
        painter.rect_filled(pill, CornerRadius::same(8), theme::CELL_ACTIVE);
        painter.text(pill.center(), Align2::CENTER_CENTER, text, theme::mono(11.0), theme::MUTED);
    }
    if let Some(on) = visible {
        let track = Rect::from_min_size(pos2(switch.left() + 6.0, row.center().y - 6.5), vec2(24.0, 13.0));
        painter.rect_filled(track, CornerRadius::same(7), if on { theme::ACCENT } else { theme::HAIRLINE_STRONG });
        let thumb = Rect::from_min_size(pos2(track.left() + if on { 13.0 } else { 2.0 }, track.top() + 2.0), vec2(9.0, 9.0));
        shadow(painter, thumb, 4.5, theme::SHADOW_1);
        painter.circle_filled(thumb.center(), 4.5, Color32::WHITE);
    }
    Header { toggled: expand_response.clicked(), visibility: switch_response.is_some_and(|r| r.clicked()) }
}

/// A section's body: a list with its own scroll once it passes 320px, then 4px under it.
pub fn body<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = egui::ScrollArea::vertical().id_salt(("section-body", id)).max_height(SECTION_BODY_MAX).auto_shrink([false, true]).show(ui, add).inner;
    ui.add_space(4.0);
    out
}

/// What a list row says (`Row`).
#[derive(Clone, Debug, Default)]
pub struct RowFace<'a> {
    /// The colour the thing has on the map; `None` keeps the swatch's place empty.
    pub color: Option<Color32>,
    pub name: &'a str,
    pub range: &'a str,
    pub reverse: bool,
    pub selected: bool,
    /// How many stated relationships touch it; 0 draws nothing.
    pub acts: usize,
}

/// A row in a section's list, laid out next. Returns its response.
pub fn row(ui: &mut Ui, face: &RowFace) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
    let response = response.on_hover_text(face.name);
    let painter = ui.painter();
    if face.selected {
        painter.rect_filled(rect, 0.0, theme::CELL_ACTIVE);
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, theme::ACCENT);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, theme::CELL_HOVER);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let mut x = rect.left() + 14.0;
    if let Some(color) = face.color {
        painter.rect_filled(Rect::from_min_size(pos2(x, rect.center().y - 4.5), vec2(9.0, 9.0)), CornerRadius::same(2), color);
    }
    x += 9.0 + 8.0;
    let mut right = rect.right() - 12.0;
    let range = painter.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, face.range, theme::mono(10.5), theme::MUTED);
    right = range.left() - 8.0;
    if face.acts > 0 {
        let acts = painter.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, format!("⇄{}", face.acts), theme::sans(10.5), theme::MUTED);
        right = acts.left() - 8.0;
    }
    if face.reverse {
        let strand = painter.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, "◂", theme::sans(11.0), theme::MUTED);
        right = strand.left() - 8.0;
    }
    let name = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
    painter.with_clip_rect(name).text(pos2(x, rect.center().y), Align2::LEFT_CENTER, face.name, theme::sans(12.5), theme::INK);
    response
}

/// `.prop-empty`: a muted sentence where a list would be.
pub fn empty(ui: &mut Ui, text: &str) {
    let width = ui.available_width();
    let galley = ui.painter().layout(text.to_string(), theme::sans(12.0), theme::MUTED, (width - 28.0).max(40.0));
    let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 24.0), Sense::hover());
    ui.painter().galley(pos2(rect.left() + 14.0, rect.top() + 10.0), galley, theme::MUTED);
}

/// The hairline under a section (`.prop-section { border-bottom }`).
pub fn rule(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().hline(rect.left()..=rect.right(), rect.center().y, Stroke::new(1.0, theme::HAIRLINE));
}
