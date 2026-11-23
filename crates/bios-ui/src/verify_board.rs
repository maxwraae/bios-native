//! THE BOARD: what matches what, before anything runs, and one click to change it. Ported from the
//! web's `verify/Board.tsx` and `verify/FolderVerify.tsx`, with their styles (`.board*`, `.bw-*`).
//!
//! A row per map, its files as chips: grey staged chips that wait to run, stored chips whose colour
//! is the person's call and whose dot is what the app found. Chips drag between rows and to the
//! tray; × takes one off a map; the foot says the next thing to do.

use bios_core::verify::folder::{Call, RunSummary};
use bios_core::verify::runs::{LineState, box_fails_as_one, disagrees, run_tone, state_of, summary_line};
use egui::{Align2, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::primitives::{self, Button, Face, Material, Segment, Tone, text_width};
use crate::theme::{self, Weight};
use crate::verify_kit::{self, CHIP_H, ChipKind};
use crate::{icon, lucide};

/// A sample waiting to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub name: String,
    /// What the chip says: the part of the name that tells it from its row's others.
    pub label: String,
    /// Its files, R1 first.
    pub files: Vec<String>,
}

/// An alignment on a row.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub run: RunSummary,
    pub label: String,
    /// Measured against bases the map no longer holds.
    pub stale: bool,
}

/// One map and everything measured against it.
#[derive(Clone, Debug, PartialEq)]
pub struct MapRow {
    pub id: String,
    pub name: String,
    pub length: usize,
    pub waiting: Vec<Waiting>,
    pub stored: Vec<Stored>,
}

/// What the board shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardModel {
    /// The verification's name (its folder's last part).
    pub name: String,
    pub rows: Vec<MapRow>,
    /// Samples on no map, labelled by their first file's whole name.
    pub tray: Vec<Waiting>,
    /// The design the tray's samples share that no map is called.
    pub hint: Option<String>,
    /// Plasmids in the project that can become maps here: `(id, name, length)`.
    pub candidates: Vec<(String, String, usize)>,
    /// What a run is doing now, while one is.
    pub running: Option<String>,
    /// Something being done (a copy, a rename), or a failure, under the bar.
    pub busy: Option<String>,
    pub error: Option<String>,
    /// The first uncalled alignment in the order a plate is called, and how many there are.
    pub first_uncalled: Option<(String, Option<String>)>,
    pub uncalled: usize,
    pub good: usize,
    /// "Copied" for a moment after Copy.
    pub copied: bool,
    /// An undo is offered for the last clear.
    pub can_undo_clear: bool,
}

/// What the person did on the board this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum BoardAction {
    /// Upload reads or maps (a file dialog).
    UploadFiles,
    /// Add reads to one map (a file dialog).
    AddReadsTo(String),
    AddMap(String),
    ChangeMap {
        map: String,
        plasmid: String,
    },
    AutoMatch,
    ClearAll,
    UndoClear,
    Rename(String),
    DeleteVerification,
    Place {
        sample: String,
        map: Option<String>,
    },
    Remove(String),
    OpenRun {
        name: String,
        map: Option<String>,
    },
    DeleteRun {
        name: String,
        map: Option<String>,
    },
    Run,
    Cancel,
    /// Open the first file nobody has called.
    Call,
    Copy,
}

/// The board's own state between frames.
#[derive(Default)]
pub struct Board {
    name_draft: Option<String>,
    add_open: bool,
    menu_open: bool,
    confirm_delete: bool,
    confirm_run: Option<(String, Option<String>)>,
    /// The row open to its files' whole names.
    open_row: Option<String>,
    /// The chip being dragged: its sample and whether the press became a drag.
    drag: Option<(String, bool)>,
}

/// Where a chip can land: a row's map, or the tray.
type Target = Option<String>;

impl Board {
    /// Forgets what belonged to the last folder.
    pub fn reset(&mut self) {
        *self = Board::default();
    }

    pub fn show(&mut self, ui: &mut Ui, work: Rect, model: &BoardModel, dropping: bool) -> Vec<BoardAction> {
        let mut out = Vec::new();
        let painter = ui.painter().with_clip_rect(work);
        painter.rect_filled(work, 0.0, theme::CANVAS);
        band(ui, work, &model.name);
        let cell = Rect::from_min_max(pos2(work.left(), work.top() + theme::HEAD_H), work.max);
        if dropping {
            painter.rect_stroke(cell.shrink(1.0), 0.0, Stroke::new(2.0, theme::alpha(theme::SANA_GREEN, 0.5)), egui::StrokeKind::Inside);
        }
        let width = cell.width().min(1040.0);
        // `.verify-workbench`: at most 1040 wide with 24 either side, then the bar and rows inset 24 more.
        let wl = cell.center().x - width / 2.0 + 24.0;
        let f = Frame { cell, wl, wr: wl + width - 48.0, bar_top: cell.top() + 38.0, group_left: wl + 24.0, running: model.running.is_some() };
        self.bar(ui, &painter, &f, model, &mut out);
        let y = self.notice(ui, &painter, &f, model, &mut out);
        let y = self.title(ui, &painter, &f, model, &mut out, y);
        let (wl, wr) = (f.wl, f.wr);

        // The rows, the tray, scrolling under the bar.
        let body = Rect::from_min_max(pos2(wl, y), pos2(wr, cell.bottom()));
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let mut targets: Vec<(Rect, Target)> = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("board-scroll").show(ui, |ui| {
                self.rows(ui, model, &mut out, &mut targets, pointer);
                ui.add_space(110.0);
            });
        });

        self.drag_end(ui, model, &targets, pointer, &mut out);
        foot(ui, f.cell, model, &mut out);
        self.popovers(ui, &f, model, &mut out);
        out
    }

    fn bar(&mut self, ui: &mut Ui, painter: &egui::Painter, f: &Frame, model: &BoardModel, out: &mut Vec<BoardAction>) {
        let (wr, bar_top, group_left, running) = (f.wr, f.bar_top, f.group_left, f.running);
        let segments = [
            Segment { face: Face::Icon(lucide::UPLOAD, 15.0), active: false, enabled: !running, tip: "Upload reads or maps" },
            Segment { face: Face::Icon(lucide::DNA, 15.0), active: self.add_open, enabled: !running, tip: "Add a map from this project" },
            Segment {
                face: Face::Label("Auto-match"),
                active: false,
                enabled: !running && !model.rows.is_empty() && !model.tray.is_empty(),
                tip: if model.rows.is_empty() {
                    "Add plasmid maps to match against"
                } else if model.tray.is_empty() {
                    "All samples are placed"
                } else {
                    "Match unplaced samples to maps by filename"
                },
            },
            Segment {
                face: Face::Icon(lucide::X, 15.0),
                active: false,
                enabled: !running && model.rows.iter().any(|r| !r.waiting.is_empty()),
                tip: "Take every unrun sample off its map",
            },
        ];
        match primitives::group(ui, pos2(group_left, bar_top), &segments, Material::Glass, false) {
            Some(0) => out.push(BoardAction::UploadFiles),
            Some(1) => self.add_open = !self.add_open,
            Some(2) => out.push(BoardAction::AutoMatch),
            Some(3) => out.push(BoardAction::ClearAll),
            _ => {}
        }
        let group_w = primitives::group_width(ui, &segments, false);
        let more_w = if running { Button::label("Cancel", Material::Ghost).width(ui) } else { 34.0 };
        let field = Rect::from_min_max(pos2(group_left + group_w + 12.0, bar_top), pos2(wr - 24.0 - more_w - 12.0, bar_top + 34.0));
        let field_resp = ui.interact(field, ui.scope_id().with("board-field"), Sense::click());
        primitives::glass(painter, field, 17.0);
        icon::paint(painter, pos2(field.left() + 14.0 + 7.0, field.center().y), 14.0, lucide::FOLDER_INPUT, theme::MUTED);
        painter.text(
            pos2(field.left() + 14.0 + 14.0 + 8.0, field.center().y),
            Align2::LEFT_CENTER,
            "Drop reads or maps anywhere",
            theme::mono(12.5),
            theme::MUTED,
        );
        if field_resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if field_resp.clicked() && !running {
            out.push(BoardAction::UploadFiles);
        }
        let more_at = pos2(wr - 24.0 - more_w, bar_top);
        if running {
            if Button::label("Cancel", Material::Ghost).show_at(ui, more_at).clicked() {
                out.push(BoardAction::Cancel);
            }
        } else {
            let r = Rect::from_min_size(more_at, vec2(34.0, 34.0));
            let resp = ui.interact(r, ui.scope_id().with("board-more"), Sense::click()).on_hover_text("More");
            if resp.hovered() {
                painter.rect_filled(r.shrink(4.0), CornerRadius::same(8), theme::CELL_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            icon::paint(painter, r.center(), 16.0, lucide::MORE_HORIZONTAL, theme::MUTED);
            if resp.clicked() {
                self.menu_open = !self.menu_open;
            }
        }
    }

    fn notice(&mut self, ui: &mut Ui, painter: &egui::Painter, f: &Frame, model: &BoardModel, out: &mut Vec<BoardAction>) -> f32 {
        let (wl, bar_top) = (f.wl, f.bar_top);
        let mut y = bar_top + 34.0 + 16.0;
        // The notice: a confirmation, a status or a failure.
        if self.confirm_delete {
            let maps = model.rows.len();
            let runs: usize = model.rows.iter().map(|r| r.stored.len()).sum();
            let text = format!(
                "Delete {}, its {} and {runs} alignment{}?",
                model.name,
                if maps == 1 { "map".to_string() } else { format!("{maps} maps") },
                if runs == 1 { "" } else { "s" }
            );
            let g = painter.text(pos2(wl + 24.0, y + 11.0), Align2::LEFT_CENTER, &text, theme::sans(13.0), theme::INK);
            let yes = Rect::from_min_size(pos2(g.right() + 10.0, y), vec2(38.0, 22.0));
            if text_button(ui, yes, "Yes", true) {
                self.confirm_delete = false;
                out.push(BoardAction::DeleteVerification);
            }
            let no = Rect::from_min_size(pos2(yes.right() + 6.0, y), vec2(34.0, 22.0));
            if text_button(ui, no, "No", false) {
                self.confirm_delete = false;
            }
            y += 22.0 + 12.0;
        }
        let status =
            model.error.as_ref().map(|e| (e, true)).or(model.running.as_ref().map(|r| (r, false))).or(model.busy.as_ref().map(|b| (b, false)));
        if let Some((text, error)) = status {
            painter.text(
                pos2(wl + 24.0, y + 9.0),
                Align2::LEFT_CENTER,
                text,
                theme::sans(12.0),
                if error { theme::DANGER_TEXT } else { theme::MUTED },
            );
            if model.can_undo_clear && !error {
                let w = text_width(ui, text, theme::sans(12.0));
                if text_button(ui, Rect::from_min_size(pos2(wl + 24.0 + w + 10.0, y - 2.0), vec2(44.0, 22.0)), "Undo", false) {
                    out.push(BoardAction::UndoClear);
                }
            }
            y += 18.0 + 12.0;
        } else if model.can_undo_clear {
            let text = "Every unrun sample is off its map.";
            painter.text(pos2(wl + 24.0, y + 9.0), Align2::LEFT_CENTER, text, theme::sans(12.0), theme::MUTED);
            let w = text_width(ui, text, theme::sans(12.0));
            if text_button(ui, Rect::from_min_size(pos2(wl + 24.0 + w + 10.0, y - 2.0), vec2(44.0, 22.0)), "Undo", false) {
                out.push(BoardAction::UndoClear);
            }
            y += 18.0 + 12.0;
        }

        y
    }

    fn title(&mut self, ui: &mut Ui, painter: &egui::Painter, f: &Frame, model: &BoardModel, out: &mut Vec<BoardAction>, y: f32) -> f32 {
        let (wl, wr, running) = (f.wl, f.wr, f.running);
        // The list's head: the name, editable where it stands, and how many maps.
        let head_y = y + 2.0;
        let count = format!("{} map{}", model.rows.len(), if model.rows.len() == 1 { "" } else { "s" });
        painter.text(pos2(wr - 24.0, head_y + 9.0), Align2::RIGHT_CENTER, &count, theme::sans(13.0), theme::MUTED);
        let count_w = text_width(ui, &count, theme::sans(13.0));
        let title = Rect::from_min_max(pos2(wl + 24.0, head_y), pos2(wr - 24.0 - count_w - 10.0, head_y + 18.0));
        let mut draft = self.name_draft.clone().unwrap_or_else(|| model.name.clone());
        let edit = egui::TextEdit::singleline(&mut draft)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(theme::sans_weight(13.0, Weight::Medium))
            .text_color(theme::INK)
            .interactive(!running)
            .desired_width(title.width());
        let resp = ui.put(title, edit);
        if resp.gained_focus() || resp.changed() {
            self.name_draft = Some(draft.clone());
        }
        if resp.lost_focus() {
            let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let Some(wanted) = self.name_draft.take()
                && !escaped
                && !wanted.trim().is_empty()
                && wanted.trim() != model.name
            {
                out.push(BoardAction::Rename(wanted.trim().to_string()));
            }
        }
        head_y + 18.0 + 24.0
    }

    fn drag_end(&mut self, ui: &mut Ui, model: &BoardModel, targets: &[(Rect, Target)], pointer: Option<Pos2>, out: &mut Vec<BoardAction>) {
        // A drag lands where it is released; released over nothing it is a cancel.
        if let Some((sample, dragging)) = self.drag.clone() {
            let released = ui.input(|i| i.pointer.any_released());
            if released {
                if dragging
                    && let Some(p) = pointer
                    && let Some((_, target)) = targets.iter().find(|(r, _)| r.contains(p))
                {
                    out.push(BoardAction::Place { sample, map: target.clone() });
                }
                self.drag = None;
            } else if dragging {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                if let Some(p) = pointer {
                    let label = model
                        .rows
                        .iter()
                        .flat_map(|r| r.waiting.iter())
                        .chain(model.tray.iter())
                        .find(|w| w.name == self.drag.as_ref().map_or("", |d| d.0.as_str()))
                        .map(|w| w.label.clone())
                        .unwrap_or_default();
                    let w = verify_kit::chip_width(ui, &label, &ChipKind::Staged, false);
                    let r = Rect::from_min_size(p + vec2(8.0, 4.0), vec2(w, CHIP_H));
                    let fg = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("drag-chip")));
                    fg.rect_filled(r, CornerRadius::same(11), theme::FLOAT);
                    fg.rect_stroke(r, CornerRadius::same(11), Stroke::new(1.0, theme::HAIRLINE_STRONG), egui::StrokeKind::Inside);
                    fg.text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, label, verify_kit::chip_font(), theme::MUTED);
                }
            }
        }
    }

    fn popovers(&mut self, ui: &mut Ui, f: &Frame, model: &BoardModel, out: &mut Vec<BoardAction>) {
        let (wr, bar_top, group_left) = (f.wr, f.bar_top, f.group_left);
        // The popovers.
        if self.add_open {
            let items: Vec<(String, String)> =
                model.candidates.iter().map(|(_, n, l)| (n.clone(), format!("{} bp", bios_core::verify::grouped(*l as i64)))).collect();
            let at = pos2(group_left, bar_top + 34.0 + 6.0);
            match verify_kit::popover(ui, "board-add-map", at, "Add a map", &items, "no other construct in this project", 280.0) {
                Some(Some(i)) => {
                    self.add_open = false;
                    if let Some((id, ..)) = model.candidates.get(i) {
                        out.push(BoardAction::AddMap(id.clone()));
                    }
                }
                Some(None) => self.add_open = false,
                None => {}
            }
        }
        if self.menu_open {
            let mut items: Vec<(String, String)> = Vec::new();
            let swap = model.rows.len() == 1;
            if swap {
                items.extend(
                    model
                        .candidates
                        .iter()
                        .map(|(_, n, l)| (format!("Change the map to {n}"), format!("{} bp", bios_core::verify::grouped(*l as i64)))),
                );
            }
            items.push(("Delete this verification".to_string(), String::new()));
            let at = pos2(wr - 24.0 - 280.0, bar_top + 34.0 + 6.0);
            match verify_kit::popover(ui, "board-menu", at, "", &items, "", 280.0) {
                Some(Some(i)) => {
                    self.menu_open = false;
                    if i + 1 == items.len() {
                        self.confirm_delete = true;
                    } else if let (Some(row), Some((id, ..))) = (model.rows.first(), model.candidates.get(i)) {
                        out.push(BoardAction::ChangeMap { map: row.id.clone(), plasmid: id.clone() });
                    }
                }
                Some(None) => self.menu_open = false,
                None => {}
            }
        }
    }

    fn rows(&mut self, ui: &mut Ui, model: &BoardModel, out: &mut Vec<BoardAction>, targets: &mut Vec<(Rect, Target)>, pointer: Option<Pos2>) {
        let full = ui.available_rect_before_wrap();
        let (left, right) = (full.left(), full.right());
        let dragging = self.drag.as_ref().filter(|d| d.1).map(|d| d.0.clone());
        if model.rows.is_empty() {
            let text =
                "Drop the maps these reads were meant to be, or pick one from this project. Reads dropped first wait below until a map arrives.";
            let galley = ui.fonts_mut(|f| f.layout(text.to_string(), theme::sans(13.0), theme::MUTED, 52.0 * 7.0));
            let (r, _) = ui.allocate_exact_size(vec2(full.width(), galley.size().y + 26.0), Sense::hover());
            ui.painter().galley(pos2(left + 24.0, r.top() + 8.0), galley, theme::MUTED);
        }
        let show_tray = !model.tray.is_empty() || dragging.is_some();
        let mut pass = Pass { out, targets, pointer, dragging, left, right };
        for (n, row) in model.rows.iter().enumerate() {
            self.map_row(ui, n + 1, row, &mut pass);
        }
        if show_tray {
            self.tray(ui, model, &mut pass);
        }
    }

    fn map_row(&mut self, ui: &mut Ui, n: usize, row: &MapRow, pass: &mut Pass) {
        let (pointer, left, right) = (pass.pointer, pass.left, pass.right);
        let dragging = pass.dragging.clone();
        let dragging = dragging.as_deref();
        let (out, targets) = (&mut *pass.out, &mut *pass.targets);
        let top = ui.cursor().top();
        // Chips: the good ones first, the rest in plate order.
        struct Line<'a> {
            name: &'a str,
            stored: Option<&'a Stored>,
            waiting: Option<&'a Waiting>,
        }
        let mut lines: Vec<Line> = row
            .stored
            .iter()
            .map(|s| Line { name: &s.run.name, stored: Some(s), waiting: None })
            .chain(row.waiting.iter().map(|w| Line { name: &w.name, stored: None, waiting: Some(w) }))
            .collect();
        lines.sort_by(|a, b| bios_core::verify::natural_cmp(a.name, b.name));
        lines.sort_by_key(|l| !l.stored.is_some_and(|s| s.run.call == Some(Call::Good)));
        let kinds: Vec<(String, ChipKind, bool)> = lines
            .iter()
            .map(|l| match (l.stored, l.waiting) {
                (Some(s), _) => (
                    s.label.clone(),
                    ChipKind::Stored { call: s.run.call, tone: run_tone(&s.run), stale: s.stale, overruled: disagrees(&s.run) },
                    false,
                ),
                (None, Some(w)) => (w.label.clone(), ChipKind::Staged, true),
                _ => (String::new(), ChipKind::Staged, false),
            })
            .collect();
        let widths: Vec<f32> = kinds.iter().map(|(l, k, x)| verify_kit::chip_width(ui, l, k, *x)).collect();
        let chips_top = top + 6.0 + 22.0 + 4.0;
        let (rects, chips_bottom) = verify_kit::wrap(&widths, left + 62.0, chips_top, right - 24.0);
        let mut bottom = if lines.is_empty() { chips_top + 22.0 } else { chips_bottom } + 8.0;
        let states: Vec<LineState> = row.stored.iter().map(|s| state_of(&s.run)).collect();
        let fails = box_fails_as_one(&states);
        let why = "Every sample here differs from this map. Open one: if they differ in the same place, it is the map that is wrong, not the clones.";
        let why_galley = fails.then(|| ui.fonts_mut(|f| f.layout(why.to_string(), theme::sans(12.0), theme::MUTED, right - 24.0 - left - 62.0)));
        if let Some(g) = &why_galley {
            bottom += g.size().y + 8.0;
        }
        let open = self.open_row.as_deref() == Some(row.id.as_str()) && !row.stored.is_empty();
        let detail_top = bottom;
        if open {
            bottom += row.stored.len() as f32 * 28.0 + 8.0;
        }
        let rect = Rect::from_min_max(pos2(left, top), pos2(right, bottom));
        let resp = ui.allocate_rect(rect, Sense::hover());
        let over = dragging.is_some() && pointer.is_some_and(|p| rect.contains(p));
        let hovered = resp.hovered() || open;
        let painter = ui.painter().clone();
        if over {
            painter.rect_filled(rect, 0.0, theme::alpha(theme::SANA_GREEN, 0.14));
        } else if hovered {
            painter.rect_filled(rect, 0.0, theme::ROW_HOVER);
        }
        painter.line_segment([pos2(left, rect.bottom() - 0.5), pos2(right, rect.bottom() - 0.5)], Stroke::new(1.0, theme::HAIRLINE));
        targets.push((rect, Some(row.id.clone())));

        // The line: number, name, length, and the row's two buttons.
        let line_mid = top + 6.0 + 11.0;
        painter.text(pos2(left + 24.0 + 24.0, line_mid), Align2::RIGHT_CENTER, n.to_string(), theme::mono(12.5), theme::MUTED);
        let name_font = theme::sans_weight(13.0, Weight::Medium);
        let name_w = text_width(ui, &row.name, name_font.clone());
        let name_rect = Rect::from_min_size(pos2(left + 62.0, line_mid - 11.0), vec2(name_w, 22.0));
        let name_resp =
            ui.interact(name_rect, ui.scope_id().with(("row-name", &row.id)), if row.stored.is_empty() { Sense::hover() } else { Sense::click() });
        painter.text(pos2(name_rect.left(), line_mid), Align2::LEFT_CENTER, &row.name, name_font, theme::INK);
        if name_resp.hovered() && !row.stored.is_empty() {
            painter.line_segment([pos2(name_rect.left(), line_mid + 8.0), pos2(name_rect.right(), line_mid + 8.0)], Stroke::new(1.0, theme::INK));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let name_resp = name_resp.on_hover_text(if row.stored.is_empty() {
            row.name.clone()
        } else {
            format!("open {}: its first file nobody has called", row.name)
        });
        if name_resp.clicked() {
            out.push(BoardAction::OpenRun { name: String::new(), map: Some(row.id.clone()) });
        }
        let bp = format!("{} bp", bios_core::verify::grouped(row.length as i64));
        painter.text(pos2(name_rect.right() + 14.0 + 70.0, line_mid), Align2::RIGHT_CENTER, bp, theme::mono(12.5), theme::INK);
        if hovered {
            let mut x = right - 24.0 - 22.0;
            if !row.stored.is_empty() {
                let t = Rect::from_min_size(pos2(x, line_mid - 11.0), vec2(22.0, 22.0));
                if row_button(
                    ui,
                    t,
                    &format!("toggle-{}", row.id),
                    if open { lucide::CHEVRON_UP } else { lucide::CHEVRON_DOWN },
                    if open { "close" } else { "every file by its whole name" },
                ) {
                    self.open_row = if open { None } else { Some(row.id.clone()) };
                }
                x -= 24.0;
            }
            let a = Rect::from_min_size(pos2(x, line_mid - 11.0), vec2(22.0, 22.0));
            if row_button(ui, a, &format!("add-{}", row.id), lucide::PLUS, &format!("add reads to {}", row.name)) {
                out.push(BoardAction::AddReadsTo(row.id.clone()));
            }
        }

        // The chips.
        if lines.is_empty() {
            painter.text(pos2(left + 62.0, chips_top + 11.0), Align2::LEFT_CENTER, "drop reads here", theme::sans(12.0), theme::MUTED);
        }
        for ((line, (label, kind, removable)), r) in lines.iter().zip(&kinds).zip(&rects) {
            let id = ui.scope_id().with(("chip", &row.id, line.name));
            let dimmed = dragging == Some(line.name);
            let title = match (line.stored, line.waiting) {
                (Some(s), _) => stored_title(s),
                (None, Some(w)) => w.files.join(" + "),
                _ => String::new(),
            };
            let resp = verify_kit::chip(ui, *r, id, &verify_kit::ChipSpec { label, kind, removable: *removable, dimmed, title: &title });
            if resp.removed {
                out.push(BoardAction::Place { sample: line.name.to_string(), map: None });
            }
            if let Some(body) = resp.body {
                if let Some(s) = line.stored {
                    if body.clicked() {
                        out.push(BoardAction::OpenRun { name: s.run.name.clone(), map: s.run.map_id.clone() });
                    }
                } else if !resp.removed {
                    self.track_drag(&body, line.name);
                }
            }
        }
        if let Some(g) = why_galley {
            painter.galley(pos2(left + 62.0, chips_bottom + 8.0), g, theme::MUTED);
        }
        if open {
            self.detail(ui, row, out, detail_top, left, right);
        }
    }

    fn detail(&mut self, ui: &mut Ui, row: &MapRow, out: &mut Vec<BoardAction>, top: f32, left: f32, right: f32) {
        let mut sorted: Vec<&Stored> = row.stored.iter().collect();
        sorted.sort_by(|a, b| bios_core::verify::natural_cmp(&a.run.name, &b.run.name));
        let painter = ui.painter().clone();
        for (i, s) in sorted.iter().enumerate() {
            let y = top + i as f32 * 28.0;
            let line = Rect::from_min_max(pos2(left + 62.0, y), pos2(right - 24.0, y + 28.0));
            verify_kit::dot(&painter, pos2(line.left() + 3.0, line.center().y), run_tone(&s.run), s.stale);
            let file = s.run.read_files.as_ref().and_then(|f| f.first()).cloned().unwrap_or_else(|| s.run.name.clone());
            let name_r = Rect::from_min_size(pos2(line.left() + 14.0, y + 3.0), vec2(text_width(ui, &file, theme::mono(11.0)), 22.0));
            let resp = ui.interact(name_r, ui.scope_id().with(("detail", &row.id, &s.run.name)), Sense::click());
            painter.text(pos2(name_r.left(), line.center().y), Align2::LEFT_CENTER, &file, theme::mono(11.0), theme::INK);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                out.push(BoardAction::OpenRun { name: s.run.name.clone(), map: s.run.map_id.clone() });
            }
            let mut words = summary_line(&s.run);
            if let Some(call) = s.run.call {
                words = format!("{words}   {}", call.word());
            }
            if disagrees(&s.run) {
                words += " · the app found otherwise";
            }
            painter.text(pos2(name_r.right() + 14.0, line.center().y), Align2::LEFT_CENTER, words, theme::sans(11.5), theme::MUTED);
            let key = (s.run.name.clone(), s.run.map_id.clone());
            if self.confirm_run.as_ref() == Some(&key) {
                let yes = Rect::from_min_size(pos2(line.right() - 92.0, y + 3.0), vec2(48.0, 22.0));
                if text_button(ui, yes, "delete", true) {
                    self.confirm_run = None;
                    out.push(BoardAction::DeleteRun { name: key.0.clone(), map: key.1.clone() });
                }
                if text_button(ui, Rect::from_min_size(pos2(line.right() - 40.0, y + 3.0), vec2(40.0, 22.0)), "keep", false) {
                    self.confirm_run = None;
                }
            } else if row_button(
                ui,
                Rect::from_min_size(pos2(line.right() - 22.0, y + 3.0), vec2(22.0, 22.0)),
                &format!("del-{}-{}", row.id, s.run.name),
                lucide::X,
                "delete this alignment",
            ) {
                self.confirm_run = Some(key);
            }
        }
    }

    fn tray(&mut self, ui: &mut Ui, model: &BoardModel, pass: &mut Pass) {
        let (pointer, left, right) = (pass.pointer, pass.left, pass.right);
        let dragging = pass.dragging.clone();
        let dragging = dragging.as_deref();
        let (out, targets) = (&mut *pass.out, &mut *pass.targets);
        let top = ui.cursor().top() + 12.0;
        let kinds: Vec<f32> = model.tray.iter().map(|w| verify_kit::chip_width(ui, &w.label, &ChipKind::Loose, true)).collect();
        let chips_top = top + 2.0 + 18.0 + 8.0;
        let (rects, chips_bottom) = verify_kit::wrap(&kinds, left + 24.0, chips_top, right - 24.0);
        let mut bottom = if model.tray.is_empty() { chips_top } else { chips_bottom + 8.0 };
        let hint = model.hint.as_ref().map(|h| format!("These say {h}. No map here is called that."));
        if hint.is_some() {
            bottom += 22.0;
        }
        let rect = Rect::from_min_max(pos2(left, top), pos2(right, bottom.max(top + 40.0)));
        ui.allocate_rect(Rect::from_min_max(pos2(left, top - 12.0), rect.max), Sense::hover());
        let painter = ui.painter().clone();
        if dragging.is_some() && pointer.is_some_and(|p| rect.contains(p)) {
            painter.rect_filled(rect, 0.0, theme::alpha(theme::SANA_GREEN, 0.14));
        }
        targets.push((rect, None));
        painter.text(pos2(left + 24.0, top + 11.0), Align2::LEFT_CENTER, "Not placed", theme::sans_weight(13.0, Weight::Medium), theme::INK);
        let count = if model.tray.is_empty() {
            "drop a chip here to take it off its map".to_string()
        } else {
            format!("{} file{} · drag to a map", model.tray.len(), if model.tray.len() == 1 { "" } else { "s" })
        };
        let w = text_width(ui, "Not placed", theme::sans_weight(13.0, Weight::Medium));
        painter.text(pos2(left + 24.0 + w + 10.0, top + 11.0), Align2::LEFT_CENTER, count, theme::sans(13.0), theme::MUTED);
        for (w, r) in model.tray.iter().zip(&rects) {
            let id = ui.scope_id().with(("tray", &w.name));
            let title = w.files.join(" + ");
            let resp = verify_kit::chip(
                ui,
                *r,
                id,
                &verify_kit::ChipSpec {
                    label: &w.label,
                    kind: &ChipKind::Loose,
                    removable: true,
                    dimmed: dragging == Some(w.name.as_str()),
                    title: &title,
                },
            );
            if resp.removed {
                out.push(BoardAction::Remove(w.name.clone()));
            } else if let Some(body) = resp.body {
                self.track_drag(&body, &w.name);
            }
        }
        if let Some(h) = hint {
            painter.text(pos2(left + 24.0, chips_bottom + 8.0 + 9.0), Align2::LEFT_CENTER, h, theme::sans(12.0), theme::MUTED);
        }
    }

    fn track_drag(&mut self, body: &egui::Response, name: &str) {
        if body.drag_started() {
            self.drag = Some((name.to_string(), false));
        }
        if body.dragged() && self.drag.as_ref().is_some_and(|d| d.0 == name) && body.drag_delta().length() > 0.0 {
            let moved = body.ctx.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).is_some_and(|(a, b)| a.distance(b) >= 6.0));
            if moved && let Some(d) = self.drag.as_mut() {
                d.1 = true;
            }
        }
    }
}

fn stored_title(s: &Stored) -> String {
    let files = s.run.read_files.as_ref().map_or_else(|| s.run.name.clone(), |f| f.join(" + "));
    let found = match s.run.call {
        Some(call) => format!("you called it {} — the app found: {}", call.word(), s.run.verdict),
        None => s.run.verdict.clone(),
    };
    let mut parts = vec![files, found];
    if s.stale {
        parts.push("aligned against an earlier version of this map".into());
    }
    parts.join(" — ")
}

/// The head band over the board, with the verification as its one tab.
pub fn band(ui: &Ui, work: Rect, name: &str) {
    let painter = ui.painter().with_clip_rect(work);
    let band = Rect::from_min_max(work.min, pos2(work.right(), work.top() + theme::HEAD_H));
    painter.line_segment([pos2(band.left(), band.bottom() - 0.5), pos2(band.right(), band.bottom() - 0.5)], Stroke::new(1.0, theme::HAIRLINE));
    if name.is_empty() {
        return;
    }
    let font = theme::sans_weight(13.0, Weight::Medium);
    let w = text_width(ui, name, font.clone()) + 16.0 + 8.0 + 36.0;
    let tab = Rect::from_min_size(pos2(band.left() + 10.0, band.top() + 8.0), vec2(w, band.height() - 8.0));
    painter.rect_filled(tab, CornerRadius { nw: theme::RADIUS_TAB as u8, ne: theme::RADIUS_TAB as u8, sw: 0, se: 0 }, theme::CANVAS);
    painter.line_segment([tab.left_top() + vec2(4.0, 0.0), tab.right_top() - vec2(4.0, 0.0)], Stroke::new(2.0, theme::PLACE_VERIFY));
    verify_kit::verification_mark(&painter, pos2(tab.left() + 18.0, tab.center().y), 15.0);
    painter.text(pos2(tab.left() + 32.0, tab.center().y), Align2::LEFT_CENTER, name, font, theme::INK);
}

/// A 22-square button on a row (`.board-row-add`), returning whether it was clicked.
fn row_button(ui: &mut Ui, rect: Rect, id: &str, glyph: &[icon::Node], tip: &str) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(theme::RADIUS_SM as u8), theme::FLOAT);
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_SM as u8),
            Stroke::new(1.0, theme::HAIRLINE_STRONG),
            egui::StrokeKind::Inside,
        );
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    icon::paint(ui.painter(), rect.center(), 13.0, glyph, if hovered { theme::INK } else { theme::MUTED });
    resp.on_hover_text(tip).clicked()
}

/// A small word button (`.fv-act`, `.board-chip-yes`).
fn text_button(ui: &mut Ui, rect: Rect, word: &str, danger: bool) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(("word", word, rect.min.x as i32, rect.min.y as i32)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), if danger { theme::DANGER_BG } else { theme::CELL_HOVER });
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, word, theme::sans(12.0), if danger { theme::DANGER_TEXT } else { theme::INK });
    resp.clicked()
}

/// The foot: one verb, and the number it is about.
fn foot(ui: &mut Ui, cell: Rect, model: &BoardModel, out: &mut Vec<BoardAction>) {
    let running = model.running.is_some();
    let verb: Option<(String, BoardAction)> = if running {
        None
    } else {
        let placed: usize = model.rows.iter().map(|r| r.waiting.len()).sum();
        if placed > 0 {
            Some((format!("Run {placed}"), BoardAction::Run))
        } else if model.uncalled > 0 {
            Some((format!("Call {}", model.uncalled), BoardAction::Call))
        } else if model.good > 0 {
            Some((
                if model.copied { "Copied".to_string() } else { format!("Copy {} well{}", model.good, if model.good == 1 { "" } else { "s" }) },
                BoardAction::Copy,
            ))
        } else {
            None
        }
    };
    if let Some((words, action)) = verb {
        let mut b = Button::label(&words, Material::Glass);
        b.tone = Tone::Solid;
        let w = b.width(ui);
        if b.show_at(ui, pos2(cell.center().x - w / 2.0, cell.bottom() - 14.0 - 34.0)).clicked() {
            out.push(action);
        }
    }
}

/// What laying out the rows carries along.
struct Pass<'a> {
    out: &'a mut Vec<BoardAction>,
    targets: &'a mut Vec<(Rect, Target)>,
    pointer: Option<Pos2>,
    dragging: Option<String>,
    left: f32,
    right: f32,
}

/// Where the board's pieces sit this frame.
struct Frame {
    cell: Rect,
    wl: f32,
    wr: f32,
    bar_top: f32,
    group_left: f32,
    running: bool,
}
