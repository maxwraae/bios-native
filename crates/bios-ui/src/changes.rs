//! The changes column: everything that happened to the open construct, newest first, each with
//! who did it, and a way back to any of them.

use bios_project::{Actor, Entry};
use egui::{Align2, Rect, Sense, Stroke, UiBuilder, pos2, vec2};

use crate::theme;

pub const WIDTH: f32 = 300.0;

pub enum Action {
    /// Go back to how the construct was at this version.
    Restore(u64),
    /// Take back the last change to its sequence or features.
    Undo,
    /// Open the construct at this path: the one this was made from, or one made from it.
    Open(String),
}

/// What the open construct was made from and what was made from it, as paths.
#[derive(Default)]
pub struct Lineage {
    pub made_from: Option<String>,
    pub made_into: Vec<String>,
}

fn when(at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(at).map(|t| t.with_timezone(&chrono::Local).format("%-d %b %H:%M").to_string()).unwrap_or_default()
}

fn who(entry: &Entry) -> (&'static str, egui::Color32) {
    match entry.actor {
        Actor::Human => ("You", theme::INK),
        // An agent's work is marked so it can be told apart at a glance.
        Actor::Agent => ("Agent", theme::feature_color("promoter")),
        Actor::Import => ("Outside Bios", theme::MUTED),
    }
}

/// True when the entry changed the construct's text, which is what there is to go back over.
fn changed_text(entry: &Entry) -> bool {
    entry.base_sha.as_deref().is_some_and(|base| base != entry.result_sha)
}

fn button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, label: &str) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    let fill = if response.hovered() { theme::CELL_ACTIVE } else { theme::CELL };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().rect_stroke(rect, 7.0, Stroke::new(1.0, theme::HAIRLINE_STRONG), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::sans(11.5), theme::INK);
    response.clicked()
}

/// A construct's name as a link. Gives true when it was clicked.
fn link(ui: &mut egui::Ui, at: egui::Pos2, path: &str, width: f32) -> (bool, f32) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let galley = ui.painter().layout_no_wrap(name.to_string(), theme::sans(11.5), theme::INK);
    let size = vec2(galley.size().x.min(width), 18.0);
    let rect = Rect::from_min_size(at, size);
    let response = ui.interact(rect, egui::Id::unique(("changes-link", path)), Sense::click());
    let p = ui.painter().with_clip_rect(rect);
    p.galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, theme::INK);
    if response.hovered() {
        p.line_segment([rect.left_bottom() - vec2(0.0, 2.0), rect.right_bottom() - vec2(0.0, 2.0)], Stroke::new(1.0, theme::INK));
    }
    (response.clicked(), size.x)
}

pub fn show(ui: &mut egui::Ui, rect: Rect, history: &[Entry], lineage: &Lineage) -> Option<Action> {
    let p = ui.painter().clone();
    p.rect_filled(rect, 0.0, theme::CANVAS);
    p.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, theme::HAIRLINE));
    p.text(rect.min + vec2(16.0, 24.0), Align2::LEFT_CENTER, "Changes", theme::serif(18.0), theme::INK);

    let mut action = None;
    if history.iter().any(changed_text) {
        let undo = Rect::from_min_size(pos2(rect.right() - 72.0, rect.top() + 12.0), vec2(56.0, 24.0));
        if button(ui, undo, egui::Id::unique("changes-undo"), "Undo") {
            action = Some(Action::Undo);
        }
    }
    let mut top = rect.top() + 46.0;
    let groups = [("Made from", lineage.made_from.as_slice()), ("Made into", lineage.made_into.as_slice())];
    for (what, paths) in groups.into_iter().filter(|(_, paths)| !paths.is_empty()) {
        p.text(pos2(rect.left() + 16.0, top + 9.0), Align2::LEFT_CENTER, what, theme::sans(11.5), theme::MUTED);
        let mut x = rect.left() + 84.0;
        for path in paths {
            // A second row when the names run out of room.
            if x > rect.left() + 84.0 && x + 60.0 > rect.right() - 16.0 {
                top += 20.0;
                x = rect.left() + 84.0;
            }
            let (clicked, width) = link(ui, pos2(x, top), path, rect.right() - 16.0 - x);
            if clicked {
                action = Some(Action::Open(path.clone()));
            }
            x += width + 12.0;
        }
        top += 22.0;
    }
    p.line_segment([pos2(rect.left(), top), pos2(rect.right(), top)], Stroke::new(1.0, theme::HAIRLINE));

    let list = Rect::from_min_max(pos2(rect.left() + 1.0, top + 1.0), rect.max);
    let newest = history.last().map(|e| e.version);
    ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
        ui.set_clip_rect(list);
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("changes").show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for entry in history.iter().rev() {
                let width = ui.available_width();
                let summary = ui.painter().layout(entry.summary.clone(), theme::sans(12.5), theme::INK, width - 44.0);
                let height = summary.size().y + 34.0;
                let (row, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                let (name, color) = who(entry);
                ui.painter().circle_filled(row.min + vec2(18.0, 17.0), 3.5, color);
                ui.painter().galley(row.min + vec2(32.0, 9.0), summary, theme::INK);
                let meta = format!("{name} · {} · {}", entry.source, when(&entry.at));
                ui.painter().text(pos2(row.left() + 32.0, row.bottom() - 13.0), Align2::LEFT_CENTER, meta, theme::sans(11.0), theme::MUTED);
                ui.painter()
                    .line_segment([row.left_bottom() + vec2(16.0, 0.0), row.right_bottom() - vec2(16.0, 0.0)], Stroke::new(1.0, theme::HAIRLINE));
                // Going back to the newest version would be going nowhere.
                if response.hovered() && Some(entry.version) != newest {
                    let back = Rect::from_min_size(pos2(row.right() - 100.0, row.bottom() - 25.0), vec2(86.0, 20.0));
                    if button(ui, back, egui::Id::unique(("changes-back", entry.version)), "Go back here") {
                        action = Some(Action::Restore(entry.version));
                    }
                }
            }
            ui.add_space(12.0);
        });
    });
    action
}
