//! ⌘P: go to a plasmid of the project by typing part of its name. Ported from the web's
//! `explorer/FilePalette.tsx` and `explorer/matchTree.ts` (`flattenFiles`, `rankFiles`), with the
//! palette's look from `styles.css` (`.palette-overlay`, `.file-palette`, `.palette-item`).

use std::cmp::Ordering;

use egui::{Align2, Color32, CornerRadius, CursorIcon, Key, Rect, Sense, Shadow, Stroke, StrokeKind, pos2, vec2};

use crate::theme;
use crate::{icon, lucide};

/// `MAX_RESULTS`: how many the palette draws.
pub const MAX_RESULTS: usize = 100;

/// One plasmid as the palette lists it: its name and the folder it sits in ("" at the top).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: String,
}

/// `rankFiles`: case-insensitive substring, best first: the exact name, then a name that starts
/// with the query, then one that contains it, then a folder path that does. Ties go by name. An
/// empty query keeps everything in its order.
pub fn rank(entries: &[Entry], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut scored: Vec<(usize, i32)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.name.to_lowercase();
            let path = format!("{}/{}", e.dir, e.name).to_lowercase();
            let score = if name == q {
                100
            } else if name.starts_with(&q) {
                80
            } else if name.contains(&q) {
                60
            } else if path.contains(&q) {
                40
            } else {
                return None;
            };
            Some((i, score))
        })
        .collect();
    scored.sort_by(|(a, sa), (b, sb)| sb.cmp(sa).then_with(|| compare(&entries[*a].name, &entries[*b].name)));
    scored.into_iter().map(|(i, _)| i).collect()
}

/// `localeCompare` for names: by letters ignoring case first, then as written.
fn compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// The palette while it is open.
#[derive(Default)]
pub struct QuickOpen {
    pub query: String,
    active: usize,
    focused: bool,
    /// The query `active` belongs to; a new one starts again at the top.
    ranked_for: Option<String>,
}

/// What happened in the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The entry at this index of the list given was chosen.
    Pick(usize),
    Close,
}

impl QuickOpen {
    /// Draws the palette over `area` (the work column) and answers what was done in it.
    pub fn show(&mut self, ui: &mut egui::Ui, area: Rect, entries: &[Entry], project_open: bool) -> Option<Outcome> {
        let ranked: Vec<usize> = rank(entries, &self.query).into_iter().take(MAX_RESULTS).collect();
        if self.ranked_for.as_deref() != Some(self.query.as_str()) {
            self.active = 0;
            self.ranked_for = Some(self.query.clone());
        }
        let mut outcome = None;
        egui::Area::new(egui::Id::unique("file-palette")).order(egui::Order::Foreground).fixed_pos(area.min).fade_in(false).show(ui.ctx(), |ui| {
            let dim = ui.interact(area, ui.scope_id().with("palette-overlay"), Sense::click());
            ui.painter().rect_filled(area, 0.0, Color32::from_rgba_unmultiplied(20, 24, 28, 31));
            let width = 460f32.min(area.width() * 0.84);
            let rows_h = (ranked.len().max(1) as f32) * 33.0 + 12.0;
            let height = (48.0 + rows_h).min(area.height() * 0.62);
            let palette = Rect::from_min_size(pos2(area.center().x - width / 2.0, area.top() + area.height() * 0.11), vec2(width, height));
            let inside = ui.interact(palette, ui.scope_id().with("file-palette"), Sense::click());
            if dim.clicked() && !inside.hovered() {
                outcome = Some(Outcome::Close);
            }
            let radius = CornerRadius::same(theme::RADIUS as u8);
            ui.painter().add(
                Shadow { offset: [0, 24], blur: 60, spread: 0, color: Color32::from_rgba_unmultiplied(30, 35, 40, 70) }.as_shape(palette, radius),
            );
            ui.painter().rect_filled(palette, radius, theme::FLOAT);
            ui.painter().rect_stroke(palette, radius, Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
            let field = Rect::from_min_max(pos2(palette.left() + 16.0, palette.top() + 14.0), pos2(palette.right() - 16.0, palette.top() + 34.0));
            let edit = egui::TextEdit::singleline(&mut self.query)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .font(theme::sans(15.0))
                .text_color(theme::INK)
                .hint_text(egui::RichText::new("Go to plasmid\u{2026}").color(theme::MUTED))
                .desired_width(field.width());
            let response = ui.put(field, edit);
            if !self.focused {
                response.request_focus();
                self.focused = true;
            }
            let rule = palette.top() + 48.0;
            ui.painter().line_segment([pos2(palette.left(), rule), pos2(palette.right(), rule)], Stroke::new(1.0, theme::HAIRLINE));
            let (down, up, enter, escape) =
                ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if down {
                self.active = (self.active + 1).min(ranked.len().saturating_sub(1));
            }
            if up {
                self.active = self.active.saturating_sub(1);
            }
            if enter && let Some(hit) = ranked.get(self.active) {
                outcome = Some(Outcome::Pick(*hit));
            }
            if escape {
                outcome = Some(Outcome::Close);
            }
            let list = Rect::from_min_max(pos2(palette.left() + 6.0, rule + 6.0), pos2(palette.right() - 6.0, palette.bottom() - 6.0));
            let painter = ui.painter().with_clip_rect(list);
            if ranked.is_empty() {
                let said = if project_open { "No matching plasmids" } else { "No project open" };
                painter.text(pos2(list.left() + 10.0, list.top() + 18.0), Align2::LEFT_CENTER, said, theme::sans(13.0), theme::MUTED);
                return;
            }
            for (row, &i) in ranked.iter().enumerate() {
                let item = Rect::from_min_size(pos2(list.left(), list.top() + row as f32 * 33.0), vec2(list.width(), 33.0));
                if item.top() > list.bottom() {
                    break;
                }
                let response = ui.interact(item.intersect(list), ui.scope_id().with(("palette-item", i)), Sense::click());
                if response.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                    self.active = row;
                }
                if row == self.active {
                    painter.rect_filled(item, CornerRadius::same(255), theme::CELL_ACTIVE);
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                icon::paint(&painter, pos2(item.left() + 10.0 + 7.5, item.center().y), 15.0, lucide::FILE_TEXT, theme::MUTED);
                let entry = &entries[i];
                let dir_w =
                    if entry.dir.is_empty() { 0.0 } else { crate::primitives::text_width(ui, &entry.dir, theme::sans(11.0)).min(item.width() * 0.4) };
                let name_room = item.width() - 10.0 - 15.0 - 9.0 - 10.0 - dir_w - if dir_w > 0.0 { 12.0 } else { 0.0 };
                let name = crate::session_nav::clip(ui, &entry.name, &theme::sans(13.0), name_room);
                painter.text(pos2(item.left() + 34.0, item.center().y), Align2::LEFT_CENTER, name, theme::sans(13.0), theme::INK);
                if dir_w > 0.0 {
                    let dir = crate::session_nav::clip(ui, &entry.dir, &theme::sans(11.0), dir_w);
                    painter.text(pos2(item.right() - 10.0, item.center().y), Align2::RIGHT_CENTER, dir, theme::sans(11.0), theme::MUTED);
                }
                if response.clicked() {
                    outcome = Some(Outcome::Pick(i));
                }
            }
        });
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, rank};

    fn entry(dir: &str, name: &str) -> Entry {
        Entry { name: name.into(), dir: dir.into() }
    }

    // matchTree.test.ts's quick-open tests, on its sample folder, in its order.
    fn flat() -> Vec<Entry> {
        vec![entry("subfolder", "mini.gb"), entry("", "gfp.fasta"), entry("", "pUC19.gb"), entry("", "pVector_demo.gb")]
    }

    fn names(entries: &[Entry], order: Vec<usize>) -> Vec<String> {
        order.into_iter().map(|i| entries[i].name.clone()).collect()
    }

    #[test]
    fn rank_files_empty_query_returns_everything_a_query_filters_to_name_or_path_matches() {
        let flat = flat();
        assert_eq!(rank(&flat, "").len(), 4);
        assert_eq!(names(&flat, rank(&flat, "gb")), ["mini.gb", "pUC19.gb", "pVector_demo.gb"]);
        assert!(rank(&flat, "zzz").is_empty());
    }

    #[test]
    fn rank_files_name_prefix_outranks_a_substring_and_a_dir_only_match_still_surfaces() {
        let flat = flat();
        assert_eq!(names(&flat, rank(&flat, "p")), ["pUC19.gb", "pVector_demo.gb", "gfp.fasta"]);
        assert_eq!(names(&flat, rank(&flat, "subfolder")), ["mini.gb"]);
    }

    #[test]
    fn the_exact_name_comes_first() {
        let flat = vec![entry("", "pUC19 copy"), entry("", "pUC19")];
        assert_eq!(names(&flat, rank(&flat, "puc19")), ["pUC19", "pUC19 copy"]);
    }
}
