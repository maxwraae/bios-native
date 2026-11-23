//! Verify's panel: the project's verifications side by side, each a tinted folder of maps, each map
//! with its alignments under it. Ported from the web's `layout/SessionNav.tsx` as it draws a
//! verification folder (`typedFolders`, `nestRuns`, `renderConstruct` with run children) and
//! `layout/sessionConstructs.ts` (`runNote`, `siblingLabels` per map).
//!
//! A folder's name opens its board; a map opens its first file nobody has called; a run opens.

use std::collections::BTreeSet;

use bios_core::verify::runs::Tone;
use egui::{Align2, CursorIcon, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::primitives::text_width;
use crate::session_nav::clip;
use crate::theme;
use crate::{icon, kind_icons, lucide, verify_kit};

/// One alignment under its map.
#[derive(Clone, Debug, PartialEq)]
pub struct NavRun {
    pub name: String,
    pub label: String,
    pub tone: Tone,
    pub note: String,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavMap {
    pub id: String,
    pub name: String,
    pub circular: bool,
    pub runs: Vec<NavRun>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavFolder {
    /// The folder's path in the project.
    pub folder: String,
    pub maps: Vec<NavMap>,
}

/// What is on screen, for the lit row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavOpen {
    Nothing,
    Folder(String),
    Run { folder: String, name: String, map: Option<String> },
}

/// What the person chose in the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavChosen {
    /// The front door: a new run.
    NewRun,
    Folder(String),
    Map {
        folder: String,
        map: String,
    },
    Run {
        folder: String,
        name: String,
        map: String,
    },
}

/// The panel's state: which folders and maps are shut.
#[derive(Default)]
pub struct VerifyNav {
    shut: BTreeSet<String>,
}

const ROW: f32 = 32.0;
const INDENT: f32 = 13.0;

impl VerifyNav {
    /// Draws the list in `rect`, under the sidebar's head (`sidebar`), which holds the project's
    /// name, the way out and the new run.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, folders: &[NavFolder], open: &NavOpen) -> Vec<NavChosen> {
        let mut out = Vec::new();
        let body = rect;
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("verify-nav").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if folders.is_empty() {
                    let text = "No plates yet. Press + and drop the reads that came back.";
                    let g = ui.fonts_mut(|f| f.layout(text.to_string(), theme::sans(13.0), theme::MUTED, body.width() - 40.0));
                    let (r, _) = ui.allocate_exact_size(vec2(body.width(), g.size().y + 20.0), Sense::hover());
                    ui.painter().galley(pos2(r.left() + 14.0, r.top() + 10.0), g, theme::MUTED);
                }
                for f in folders {
                    self.folder(ui, f, open, &mut out);
                }
            });
        });
        out
    }

    fn folder(&mut self, ui: &mut Ui, f: &NavFolder, open: &NavOpen, out: &mut Vec<NavChosen>) {
        let is_open = !self.shut.contains(&f.folder);
        let lit = *open == NavOpen::Folder(f.folder.clone());
        let (row, resp) = row_rect(ui);
        let painter = ui.painter().clone();
        crate::primitives::row_ground(&painter, row, resp.hovered(), lit);
        let chevron = Rect::from_center_size(pos2(row.left() + 4.0 + 10.0, row.center().y), vec2(20.0, 20.0));
        icon::paint(&painter, chevron.center(), 14.0, if is_open { lucide::CHEVRON_DOWN } else { lucide::CHEVRON_RIGHT }, theme::MUTED);
        verify_kit::verification_mark(&painter, pos2(row.left() + 4.0 + 20.0 + 12.0, row.center().y), 16.0);
        let name = f.folder.rsplit('/').next().unwrap_or(&f.folder);
        let shown = clip(ui, name, &theme::sans(13.0), row.right() - row.left() - 56.0);
        painter.text(pos2(row.left() + 4.0 + 20.0 + 24.0 + 6.0, row.center().y), Align2::LEFT_CENTER, shown, theme::sans(13.0), theme::INK);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            let on_chevron = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| chevron.contains(p));
            if on_chevron || lit {
                if is_open {
                    self.shut.insert(f.folder.clone());
                } else {
                    self.shut.remove(&f.folder);
                }
            } else {
                out.push(NavChosen::Folder(f.folder.clone()));
            }
        }
        if !is_open {
            return;
        }
        let kids_top = ui.cursor().top();
        for m in &f.maps {
            self.map(ui, f, m, open, out);
        }
        let kids_bottom = ui.cursor().top();
        if kids_bottom > kids_top {
            let x = ui.max_rect().left() + 4.0 + 13.5;
            ui.painter().line_segment([pos2(x, kids_top), pos2(x, kids_bottom)], Stroke::new(1.0, theme::alpha(theme::PLACE_VERIFY, 0.45)));
        }
    }

    fn map(&mut self, ui: &mut Ui, f: &NavFolder, m: &NavMap, open: &NavOpen, out: &mut Vec<NavChosen>) {
        let key = format!("{}\u{0}{}", f.folder, m.id);
        let kids_open = !m.runs.is_empty() && !self.shut.contains(&key);
        let lit = matches!(open, NavOpen::Run { folder, map, .. } if *folder == f.folder && map.as_deref() == Some(m.id.as_str()));
        let (row, resp) = row_rect(ui);
        let painter = ui.painter().clone();
        crate::primitives::row_ground(&painter, row, resp.hovered(), lit);
        let x = row.left() + 4.0 + INDENT + 12.0;
        let mark = pos2(x + 8.0, row.center().y);
        if lit {
            kind_icons::paint_plasmid_filled(&painter, mark, 15.0, theme::INK);
        } else {
            kind_icons::paint(&painter, mark, 15.0, if m.circular { kind_icons::PLASMID } else { kind_icons::LINEAR }, theme::MUTED);
        }
        let room = row.right() - 8.0 - (x + 24.0) - if m.runs.is_empty() { 0.0 } else { 22.0 };
        let name = crate::session_nav::fit_name(ui, &m.name, &theme::sans(13.0), room);
        painter.text(pos2(x + 24.0, row.center().y), Align2::LEFT_CENTER, name, theme::sans(13.0), theme::INK);
        let toggle = Rect::from_center_size(pos2(row.right() - 8.0 - 9.0, row.center().y), vec2(18.0, 18.0));
        if !m.runs.is_empty() && resp.hovered() {
            icon::paint(&painter, toggle.center(), 13.0, if kids_open { lucide::CHEVRON_DOWN } else { lucide::CHEVRON_RIGHT }, theme::MUTED);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let resp = resp.on_hover_text(&m.name);
        if resp.clicked() {
            let on_toggle = ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| toggle.contains(p));
            if on_toggle && !m.runs.is_empty() {
                if kids_open {
                    self.shut.insert(key.clone());
                } else {
                    self.shut.remove(&key);
                }
            } else {
                out.push(NavChosen::Map { folder: f.folder.clone(), map: m.id.clone() });
            }
        }
        if !kids_open {
            return;
        }
        for r in &m.runs {
            let lit =
                matches!(open, NavOpen::Run { folder, name, map } if *folder == f.folder && *name == r.name && map.as_deref() == Some(m.id.as_str()));
            let (row, resp) = row_rect(ui);
            let painter = ui.painter().clone();
            crate::primitives::row_ground(&painter, row, resp.hovered(), lit);
            let x = row.left() + 4.0 + 2.0 * INDENT + 12.0;
            verify_kit::dot(&painter, pos2(x + 6.0, row.center().y), r.tone, r.stale);
            let note_font = theme::sans(11.0);
            // The name is the row's identity and is kept whole when it can be; the note gets the rest.
            let room = row.right() - 8.0 - (x + 18.0);
            let label_w = text_width(ui, &r.label, theme::sans(13.0)).min(room * 0.7);
            let note = clip(ui, &r.note, &note_font, (room - label_w - 8.0).max(0.0));
            painter.text(pos2(row.right() - 8.0, row.center().y), Align2::RIGHT_CENTER, note, note_font, theme::MUTED);
            let label = clip(ui, &r.label, &theme::sans(13.0), label_w + 1.0);
            painter.text(pos2(x + 18.0, row.center().y), Align2::LEFT_CENTER, label, theme::sans(13.0), theme::INK);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.on_hover_text(&r.name).clicked() {
                out.push(NavChosen::Run { folder: f.folder.clone(), name: r.name.clone(), map: m.id.clone() });
            }
        }
    }
}

fn row_rect(ui: &mut Ui) -> (Rect, egui::Response) {
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
    let row = Rect::from_min_max(pos2(slot.left() + 4.0, slot.top() + 1.0), pos2(slot.right() - 4.0, slot.bottom() - 1.0));
    let resp = ui.interact(row, ui.scope_id().with(("vrow", slot.top() as i32)), Sense::click());
    (row, resp)
}
