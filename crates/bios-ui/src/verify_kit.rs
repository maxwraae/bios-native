//! What Verify's screens share: the run chip (the person's call as its colour, the app's finding as
//! its dot), the dot's tones, a small popover list, and the verification folder's mark. From the
//! web's `reads/reads.css` (`.board-chip*`, `.tone-*`) and `verify/Board.tsx` `Chip`.

use bios_core::verify::folder::Call;
use bios_core::verify::runs::Tone;
use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::primitives::text_width;
use crate::theme::{self, Weight};
use crate::{icon, kind_icons, lucide};

/// `.board-chip`: 22 high, mono 11, padded 8, 5 between its parts.
pub const CHIP_H: f32 = 22.0;
const CHIP_PAD: f32 = 8.0;
const CHIP_GAP: f32 = 5.0;

pub fn chip_font() -> FontId {
    theme::mono(11.0)
}

/// The dot's colour for what the app found; `None` for the hollow ring of "not this map".
pub fn tone_color(tone: Tone) -> Option<Color32> {
    match tone {
        Tone::Ok => Some(theme::SANA_GREEN),
        Tone::Changed => Some(theme::SANA_SCARLET),
        Tone::Gaps => Some(theme::SANA_AMBER),
        Tone::Unmapped => None,
    }
}

/// Paints the 6 px finding dot; `faded` for an alignment measured against older bases.
pub fn dot(painter: &egui::Painter, center: Pos2, tone: Tone, faded: bool) {
    let a = if faded { 0.4 } else { 1.0 };
    match tone_color(tone) {
        Some(c) => {
            painter.circle_filled(center, 3.0, theme::alpha(c, a));
        }
        None => {
            painter.circle_stroke(center, 2.25, Stroke::new(1.5, theme::alpha(theme::MUTED, a)));
        }
    }
}

/// What a chip is.
#[derive(Clone, Debug, PartialEq)]
pub enum ChipKind {
    /// Aligned: opens when clicked, wears the call and the dot.
    Stored { call: Option<Call>, tone: Tone, stale: bool, overruled: bool },
    /// Waiting to run, on a map.
    Staged,
    /// Waiting, on no map.
    Loose,
}

/// A chip's colours: ground, border, ink.
fn chip_colors(kind: &ChipKind) -> (Color32, Color32, Color32) {
    match kind {
        ChipKind::Stored { call: Some(Call::Good), .. } => {
            (theme::mix(theme::CELL, theme::SANA_GREEN, 0.18), Color32::TRANSPARENT, theme::mix(theme::INK, theme::SANA_GREEN, 0.70))
        }
        ChipKind::Stored { call: Some(Call::Bad), .. } => {
            (theme::mix(theme::CELL, theme::SANA_SCARLET, 0.14), Color32::TRANSPARENT, theme::SANA_SCARLET)
        }
        ChipKind::Stored { call: Some(Call::Unsure), .. } => {
            (theme::mix(theme::CELL, theme::SANA_BLUE, 0.12), Color32::TRANSPARENT, theme::SANA_BLUE)
        }
        ChipKind::Stored { .. } => (theme::CELL_SUNKEN, Color32::TRANSPARENT, theme::INK),
        ChipKind::Staged | ChipKind::Loose => (Color32::TRANSPARENT, theme::HAIRLINE_STRONG, theme::MUTED),
    }
}

fn mark_of(kind: &ChipKind) -> Option<&'static str> {
    match kind {
        ChipKind::Stored { call: Some(Call::Good), .. } => Some("✓"),
        ChipKind::Stored { call: Some(Call::Bad), .. } => Some("✗"),
        ChipKind::Stored { call: Some(Call::Unsure), .. } => Some("?"),
        _ => None,
    }
}

/// How wide a chip with this label is.
pub fn chip_width(ui: &Ui, label: &str, kind: &ChipKind, removable: bool) -> f32 {
    let mut w = 2.0 * CHIP_PAD + text_width(ui, label, chip_font());
    if mark_of(kind).is_some() {
        w += text_width(ui, "✓", theme::sans(11.0)) + CHIP_GAP;
    }
    if let ChipKind::Stored { overruled, .. } = kind {
        w += 6.0 + CHIP_GAP;
        if *overruled {
            w += 4.0 + CHIP_GAP;
        }
    }
    if removable {
        w += 14.0 + CHIP_GAP;
    }
    w
}

/// What happened to a chip this frame.
#[derive(Default)]
pub struct ChipResponse {
    pub body: Option<Response>,
    pub removed: bool,
}

/// One chip to draw.
pub struct ChipSpec<'a> {
    pub label: &'a str,
    pub kind: &'a ChipKind,
    /// It has a × (staged chips).
    pub removable: bool,
    /// Faded, while it is being dragged.
    pub dimmed: bool,
    /// Its hover text.
    pub title: &'a str,
}

/// Draws one chip in `rect`.
pub fn chip(ui: &mut Ui, rect: Rect, id: egui::Id, spec: &ChipSpec) -> ChipResponse {
    let ChipSpec { label, kind, removable, dimmed, title } = *spec;
    let body = ui.interact(rect, id, Sense::click_and_drag());
    let hovered = body.hovered();
    let (ground, border, mut ink) = chip_colors(kind);
    if dimmed {
        ink = theme::alpha(ink, 0.35);
    }
    let painter = ui.painter().clone();
    let r = CornerRadius::same((CHIP_H / 2.0) as u8);
    painter.rect_filled(rect, r, if dimmed { theme::alpha(ground, 0.35) } else { ground });
    if border != Color32::TRANSPARENT {
        painter.rect_stroke(rect, r, Stroke::new(1.0, border), StrokeKind::Inside);
    }
    let mut x = rect.left() + CHIP_PAD;
    if let Some(mark) = mark_of(kind) {
        let g = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, mark, theme::sans(11.0), ink);
        x = g.right() + CHIP_GAP;
    }
    let name = painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, chip_font(), ink);
    if matches!(kind, ChipKind::Stored { .. }) && hovered {
        painter.line_segment([pos2(name.left(), name.bottom() - 1.0), pos2(name.right(), name.bottom() - 1.0)], Stroke::new(1.0, ink));
    }
    x = name.right() + CHIP_GAP;
    if let ChipKind::Stored { tone, stale, overruled, .. } = kind {
        dot(&painter, pos2(x + 3.0, rect.center().y), *tone, *stale);
        x += 6.0 + CHIP_GAP;
        if *overruled {
            painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, "•", theme::sans(11.0), ink);
            x += 4.0 + CHIP_GAP;
        }
    }
    let mut removed = false;
    if removable {
        let xr = Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(14.0, 14.0));
        let x_resp = ui.interact(xr, id.with("x"), Sense::click());
        if hovered || x_resp.hovered() {
            if x_resp.hovered() {
                painter.rect_filled(xr, CornerRadius::same(7), theme::CELL_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            icon::paint(&painter, xr.center(), 10.0, lucide::X, if x_resp.hovered() { theme::INK } else { theme::MUTED });
        }
        removed = x_resp.clicked();
    }
    if hovered {
        let grab = matches!(kind, ChipKind::Staged | ChipKind::Loose);
        ui.ctx().set_cursor_icon(if grab { CursorIcon::Grab } else { CursorIcon::PointingHand });
    }
    let body = if title.is_empty() { body } else { body.on_hover_text(title) };
    ChipResponse { body: Some(body), removed }
}

/// Lays chips out left to right, wrapping, 5 apart across and 4 down (`.board-chips`). Returns
/// each chip's rect and the bottom of the last line.
pub fn wrap(widths: &[f32], left: f32, top: f32, right: f32) -> (Vec<Rect>, f32) {
    let mut out = Vec::with_capacity(widths.len());
    let (mut x, mut y) = (left, top);
    for &w in widths {
        let w = w.min(right - left);
        if x > left && x + w > right {
            x = left;
            y += CHIP_H + 4.0;
        }
        out.push(Rect::from_min_size(pos2(x, y), vec2(w, CHIP_H)));
        x += w + 5.0;
    }
    let bottom = if widths.is_empty() { top } else { y + CHIP_H };
    (out, bottom)
}

/// The verification folder's mark: a folder with a tick, in Verify's green.
pub fn verification_mark(painter: &egui::Painter, center: Pos2, size: f32) {
    kind_icons::paint(painter, center, size, kind_icons::FOLDER, theme::PLACE_VERIFY);
    let badge = center + vec2(size * 0.22, size * 0.16);
    painter.circle_filled(badge, size * 0.2, theme::RAIL_GROUND);
    icon::paint_with(painter, badge, size * 0.42, lucide::CIRCLE_CHECK, theme::PLACE_VERIFY, 2.4);
}

/// A floating list under `at`, one line per item; returns the clicked item's index, or `Some(None)`
/// when the person clicked elsewhere (close it).
pub fn popover(ui: &Ui, id: &str, at: Pos2, head: &str, items: &[(String, String)], empty: &str, width: f32) -> Option<Option<usize>> {
    let mut chosen = None;
    let area = egui::Area::new(egui::Id::unique(id)).order(egui::Order::Foreground).fixed_pos(at).fade_in(false);
    let shown = area.show(ui.ctx(), |ui| {
        egui::Frame::new().fill(theme::FLOAT).stroke(Stroke::new(1.0, theme::HAIRLINE)).corner_radius(theme::RADIUS_SM).inner_margin(6.0).show(
            ui,
            |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if !head.is_empty() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(head).font(theme::sans_weight(11.0, Weight::Semibold)).color(theme::MUTED));
                    ui.add_space(4.0);
                }
                if items.is_empty() {
                    ui.label(egui::RichText::new(empty).font(theme::sans(12.0)).color(theme::MUTED));
                }
                for (i, (name, note)) in items.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), theme::ROW_HOVER);
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let note_w = text_width(ui, note, theme::sans(11.0));
                    let name_room = (width - note_w - 24.0).max(20.0);
                    let shown = crate::session_nav::clip(ui, name, &theme::sans(13.0), name_room);
                    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, shown, theme::sans(13.0), theme::INK);
                    ui.painter().text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, note, theme::sans(11.0), theme::MUTED);
                    if resp.clicked() {
                        chosen = Some(Some(i));
                    }
                }
            },
        );
    });
    if chosen.is_none() && shown.response.clicked_elsewhere() {
        chosen = Some(None);
    }
    chosen
}
