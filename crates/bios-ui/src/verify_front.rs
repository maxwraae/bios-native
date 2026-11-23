//! VERIFY'S FRONT DOOR: name the run, drop it in. Ported from the web's `verify/LocalVerify.tsx`
//! and its styles (`.verify-workbench-narrow`, `.verify-h1`, `.lv-name`, `.lv-drop`).
//!
//! One door, and it asks for a run: reads, maps, or both. The drop makes a verification folder and
//! the folder's board is where maps become rows. Nothing is aligned here.

use egui::{Align2, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::primitives;
use crate::theme::{self, Weight};

/// What the front door asks for this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrontAction {
    /// "or choose files…".
    ChooseFiles,
}

/// The door's own state: the name field.
pub struct Front {
    pub name: String,
}

impl Front {
    pub fn new(stamp: String) -> Front {
        Front { name: stamp }
    }

    /// Draws the door in `cell`. `dropping` while files hover over the window; `busy` names what is
    /// happening after a drop; `error` is the last failure.
    pub fn show(&mut self, ui: &mut Ui, cell: Rect, dropping: bool, busy: Option<&str>, error: Option<&str>) -> Option<FrontAction> {
        let painter = ui.painter().with_clip_rect(cell);
        painter.rect_filled(cell, 0.0, theme::CANVAS);
        crate::verify_board::band(ui, cell, "");
        let width = cell.width().min(720.0);
        let left = cell.center().x - width / 2.0 + 24.0;
        let right = cell.center().x + width / 2.0 - 24.0;
        let mut y = cell.top() + 28.0 + theme::HEAD_H;

        painter.text(pos2(left, y), Align2::LEFT_TOP, "Verify", theme::serif(30.0), theme::INK);
        y += 31.5 + 4.0;
        let lede = "Name the run, then drop it in. The reads, the maps, or both. Bios works out which sample is which map and shows you, so you can correct it before anything runs.";
        let galley = ui.fonts_mut(|f| f.layout(lede.to_string(), theme::sans(14.0), theme::MUTED, right - left));
        let lede_h = galley.size().y;
        painter.galley(pos2(left, y), galley, theme::MUTED);
        y += lede_h + 16.0;

        // "Call this run" and the field.
        let label_w = primitives::text_width(ui, "Call this run", theme::sans(12.0));
        painter.text(pos2(left, y + 15.0), Align2::LEFT_CENTER, "Call this run", theme::sans(12.0), theme::MUTED);
        let field = Rect::from_min_max(pos2(left + label_w + 10.0, y), pos2(right, y + 30.0));
        painter.rect_filled(field, CornerRadius::same(theme::RADIUS_SM as u8), theme::FLOAT);
        let edit = egui::TextEdit::singleline(&mut self.name)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(theme::sans(13.0))
            .text_color(theme::INK)
            .desired_width(field.width() - 20.0);
        let inner = Rect::from_min_max(pos2(field.left() + 10.0, field.center().y - 9.0), pos2(field.right() - 10.0, field.center().y + 9.0));
        let response = ui.put(inner, edit);
        let ring = if response.has_focus() { theme::mix(theme::HAIRLINE_STRONG, theme::SANA_GREEN, 0.55) } else { theme::HAIRLINE_STRONG };
        painter.rect_stroke(field, CornerRadius::same(theme::RADIUS_SM as u8), Stroke::new(1.0, ring), StrokeKind::Inside);
        y = field.bottom() + 12.0 + 16.0;

        // The window (`.gwin`) and its dashed drop zone.
        let win = Rect::from_min_max(pos2(left, y), pos2(right, y + 32.0 + 140.0));
        painter.rect_filled(win, CornerRadius::same(theme::RADIUS as u8), theme::CELL);
        painter.rect_stroke(win, CornerRadius::same(theme::RADIUS as u8), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
        let drop = win.shrink(16.0);
        if dropping {
            painter.rect_filled(drop, CornerRadius::same(theme::RADIUS as u8), theme::CELL_HOVER);
        }
        dashed(&painter, drop, if dropping { theme::ACCENT } else { theme::HAIRLINE_STRONG });
        let mid = drop.center().x;
        let mut ty = drop.top() + 30.0;
        painter.text(pos2(mid, ty), Align2::CENTER_TOP, "Drop your sequencing here", theme::sans(14.0), theme::INK);
        ty += 22.0;
        let hint =
            "Reads and maps in one go, as many as came back. Every plasmid already in this project is a candidate too. Mapping runs on this machine.";
        let galley = ui.fonts_mut(|f| {
            let mut job = egui::text::LayoutJob::simple(hint.to_string(), theme::sans(11.0), theme::MUTED, drop.width() - 40.0);
            job.halign = egui::Align::Center;
            f.layout_job(job)
        });
        let hint_h = galley.size().y;
        painter.galley(pos2(mid, ty), galley, theme::MUTED);
        ty += hint_h + 10.0;
        let words = busy.unwrap_or("or choose files…");
        let font = theme::sans_weight(11.0, Weight::Book);
        let bw = primitives::text_width(ui, words, font.clone()) + 18.0;
        let button = Rect::from_center_size(pos2(mid, ty + 11.0), vec2(bw, 22.0));
        let resp = ui.interact(button, ui.scope_id().with("verify-choose"), if busy.is_none() { Sense::click() } else { Sense::hover() });
        painter.rect_filled(button, CornerRadius::same(theme::RADIUS_SM as u8), if resp.hovered() { theme::CELL_HOVER } else { theme::FLOAT });
        painter.rect_stroke(button, CornerRadius::same(theme::RADIUS_SM as u8), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
        painter.text(button.center(), Align2::CENTER_CENTER, words, font, theme::INK);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if let Some(e) = error {
            painter.text(pos2(mid, button.bottom() + 10.0), Align2::CENTER_TOP, e, theme::sans(12.0), theme::DANGER_TEXT);
        }
        resp.clicked().then_some(FrontAction::ChooseFiles)
    }
}

/// A dashed rounded border, as CSS `border: 1px dashed` draws one.
pub fn dashed(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let stroke = Stroke::new(1.0, color);
    let r = theme::RADIUS.min(rect.height() / 2.0);
    let (l, t, rt, b) = (rect.left() + 0.5, rect.top() + 0.5, rect.right() - 0.5, rect.bottom() - 0.5);
    let edges =
        [(pos2(l + r, t), pos2(rt - r, t)), (pos2(rt, t + r), pos2(rt, b - r)), (pos2(rt - r, b), pos2(l + r, b)), (pos2(l, b - r), pos2(l, t + r))];
    for (a, z) in edges {
        painter.add(egui::Shape::dashed_line(&[a, z], stroke, 4.0, 3.0));
    }
    // The corners, solid quarter arcs.
    let corners = [(pos2(l + r, t + r), 180.0f32), (pos2(rt - r, t + r), 270.0), (pos2(rt - r, b - r), 0.0), (pos2(l + r, b - r), 90.0)];
    for (c, start) in corners {
        let pts: Vec<egui::Pos2> = (0..=6)
            .map(|i| {
                let a = (start + 15.0 * i as f32).to_radians();
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }
}
