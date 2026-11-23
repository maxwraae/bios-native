//! What you are looking at: the open plasmid's name in the top-left corner of the work, its
//! length and shape under it, and the file's own sentence about itself under that. Ported from
//! the web's `editor/PlasmidTitle.tsx` (whole file) and `styles.css` 7019-7128 (`.plasmid-title`
//! and its parts).
//!
//! The name is the display face at 15/1.15, wrapping anywhere for up to two lines and showing the
//! rest under the pointer; the length is the mono face at 10; the description is one muted line
//! that unrolls under the pointer or on a click, which keeps it open.

use bios_core::Construct;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Color32, CornerRadius, FontId, Galley, Painter, Rect, Sense, Ui, pos2, vec2};
use std::sync::Arc;

use crate::theme;

/// `.plasmid-title`: `top: 8px; left: 16px; max-width: min(420px, 38%)`.
const TOP: f32 = 8.0;
const LEFT: f32 = 16.0;
const MAX_W: f32 = 420.0;
/// `.plasmid-title-desc`'s `max-width`, the same open and shut.
const DESC_W: f32 = 200.0;

/// The sentence a file carries about itself: GenBank's DEFINITION, joined across its lines.
pub fn definition(construct: &Construct) -> String {
    let mut words: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in &construct.header {
        if let Some(rest) = line.strip_prefix("DEFINITION") {
            inside = true;
            words.push(rest.trim());
        } else if inside && line.starts_with(' ') {
            words.push(line.trim());
        } else if inside {
            break;
        }
    }
    words.into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// `ownDescription`: the file's sentence, unless it is filler. "." is how an empty DEFINITION is
/// written and SnapGene stamps "synthetic circular DNA." into every export, so those, and a
/// sentence that only repeats the name, are no description at all.
pub fn own_description(construct: &Construct) -> String {
    let raw = definition(construct);
    let bare = raw.trim_end_matches('.').trim();
    if bare.is_empty() || filler(bare) || bare == construct.name {
        return String::new();
    }
    raw
}

/// `^(?:synthetic\s+)?(?:(?:circular|linear)\s+)?(?:dna|sequence)(?:\s+construct)?$`, ignoring case.
fn filler(text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut words: Vec<&str> = lower.split_whitespace().collect();
    if words.first() == Some(&"synthetic") {
        words.remove(0);
    }
    if matches!(words.first(), Some(&"circular") | Some(&"linear")) {
        words.remove(0);
    }
    if words.last() == Some(&"construct") && words.len() > 1 {
        words.pop();
    }
    matches!(words.as_slice(), ["dna"] | ["sequence"])
}

/// `toLocaleString()` in English: 6093 → "6,093".
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// How one part of the title is set.
struct Face {
    font: FontId,
    line_height: f32,
    color: Color32,
    /// `word-break: break-all`: a plasmid name is one token, so it fills the line.
    anywhere: bool,
}

/// `.plasmid-title-name`: the display face at 15/1.15, in ink.
fn name_face() -> Face {
    Face { font: theme::serif(15.0), line_height: 15.0 * 1.15, color: theme::INK, anywhere: true }
}

/// `.plasmid-title-desc`: the interface face at 12/1.35, muted.
fn desc_face() -> Face {
    Face { font: theme::sans(12.0), line_height: 12.0 * 1.35, color: theme::MUTED, anywhere: false }
}

fn wrapped(ui: &Ui, text: &str, face: Face, width: f32, rows: usize) -> Arc<Galley> {
    let Face { font, line_height, color, anywhere } = face;
    let mut job = LayoutJob::single_section(
        text.to_string(),
        TextFormat { font_id: font, color, line_height: Some(line_height), valign: Align::Center, ..TextFormat::default() },
    );
    job.wrap = TextWrapping { max_width: width, max_rows: rows, break_anywhere: anywhere, overflow_character: Some('…') };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The hover ground: the map's own canvas behind the unrolled lines, padding cancelled by a
/// negative margin so the words do not move under the pointer.
fn ground(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect.expand2(vec2(6.0, 2.0)), CornerRadius::same(theme::RADIUS_SM as u8), theme::CANVAS);
}

/// Draws the title in the corner of `rect`, the work's cell.
pub fn show(ui: &mut Ui, rect: Rect, construct: &Construct) {
    if construct.name.is_empty() && construct.is_empty() {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let width = MAX_W.min(rect.width() * 0.38).max(0.0);
    let left = rect.left() + LEFT;
    let mut y = rect.top() + TOP;
    let pinned_id = ui.scope_id().with("plasmid-title-pinned");

    if !construct.name.is_empty() {
        let shut = wrapped(ui, &construct.name, name_face(), width, 2);
        let shut_rect = Rect::from_min_size(pos2(left, y), shut.size());
        let hovered = ui.interact(shut_rect, ui.scope_id().with("plasmid-title-name"), Sense::hover()).on_hover_text(&construct.name).hovered();
        if hovered {
            let open = wrapped(ui, &construct.name, name_face(), width, 8);
            let open_rect = Rect::from_min_size(pos2(left, y), open.size());
            // Unrolled over what is below it, so it is drawn last, on its own layer.
            let top = ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("plasmid-title-open")));
            ground(&top, open_rect);
            top.galley(open_rect.min, open, theme::INK);
        } else {
            painter.galley(shut_rect.min, shut.clone(), theme::INK);
        }
        y = shut_rect.bottom();
    }

    if !construct.is_empty() {
        let shape = if construct.circular { "circular" } else { "linear" };
        let line = format!("{} bp · {shape}", grouped(construct.len()));
        // `margin-top: 1px`, at the mono face's own line.
        let galley = painter.layout_no_wrap(line, theme::mono(10.0), theme::MUTED);
        painter.galley(pos2(left, y + 1.0), galley.clone(), theme::MUTED);
        y += 1.0 + galley.size().y;
    }

    let description = own_description(construct);
    if description.is_empty() {
        return;
    }
    let desc_w = DESC_W.min(width);
    let at = pos2(left, y + 3.0);
    let shut = wrapped(ui, &description, desc_face(), desc_w, 1);
    let shut_rect = Rect::from_min_size(at, shut.size());
    let response = ui.interact(shut_rect, ui.scope_id().with("plasmid-title-desc"), Sense::click());
    let pinned = ui.data(|d| d.get_temp::<String>(pinned_id)).is_some_and(|p| p == description);
    if response.clicked() {
        ui.data_mut(|d| {
            if pinned {
                d.remove::<String>(pinned_id);
            } else {
                d.insert_temp(pinned_id, description.clone());
            }
        });
    }
    if response.hovered() || pinned {
        let open = wrapped(ui, &description, desc_face(), desc_w, 12);
        let open_rect = Rect::from_min_size(at, open.size());
        let top = ui.painter().clone().with_layer_id(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("plasmid-title-desc-open")));
        ground(&top, open_rect);
        top.galley(open_rect.min, open, theme::MUTED);
    } else {
        painter.galley(shut_rect.min, shut, theme::MUTED);
    }
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;

    use super::{grouped, own_description};

    fn with(name: &str, header: &[&str]) -> Construct {
        Construct { name: name.into(), header: header.iter().map(|l| (*l).to_string()).collect(), ..Construct::default() }
    }

    #[test]
    fn the_description_is_the_definition_unless_it_is_filler() {
        let real = with(
            "wrap_demo",
            &["DEFINITION  Circular demo plasmid whose AmpR spans the origin", "            (join syntax).", "ACCESSION   wrap_demo"],
        );
        assert_eq!(own_description(&real), "Circular demo plasmid whose AmpR spans the origin (join syntax).");
        for filler in ["DEFINITION  .", "DEFINITION  synthetic circular DNA.", "DEFINITION  Linear sequence construct", "DEFINITION  pX."] {
            assert_eq!(own_description(&with("pX", &[filler])), "", "{filler}");
        }
        assert_eq!(own_description(&with("pX", &["ACCESSION   pX"])), "");
    }

    #[test]
    fn a_length_is_grouped_in_thousands() {
        assert_eq!(grouped(800), "800");
        assert_eq!(grouped(6093), "6,093");
        assert_eq!(grouped(1234567), "1,234,567");
    }
}
