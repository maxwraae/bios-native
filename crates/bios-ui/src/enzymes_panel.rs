//! The enzymes as a list: the other half of the scissors. Ported from Bios `editor/EnzymesPanel.tsx`
//! (search, the four chips, the groups, `EnzymeItem`), its look from `styles.css` (`.enz-*`, lines
//! 8286-8400). The model is `bios_core::enzyme_list`, which the `list_enzymes` command also reads.
//!
//! What it lists is not what the map draws: an enzyme that does not cut is an answer, and one the
//! dial hides because it cuts twice is the one you came to find. A row with sites puts its enzyme on
//! the map by name (and goes to its first site); pressed again it comes off.

use std::collections::HashMap;

use bios_core::Construct;
use bios_core::enzyme_list::{EnzymeFilter, EnzymeRow, GroupKey, enzyme_rows, group_rows};
use bios_core::enzymes::{CutSite, OverhangKind, enzymes};
use bios_core::model::Strand;
use egui::{Align2, CornerRadius, CursorIcon, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::{Face, Material, Segment, group_small_even, text_width};
use crate::side_panel::{self, ROW_H};
use crate::theme;
use crate::{icon, lucide};

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Close,
    /// Put this enzyme on the map by name, or take it off.
    Toggle(String),
    /// Select these bases, 0-based inclusive: a site's recognition sequence.
    GoTo(usize, usize),
}

/// What the panel remembers while it is open.
#[derive(Clone, Debug, Default)]
pub struct EnzymesPanel {
    pub query: String,
    pub filter: EnzymeFilter,
    open: HashMap<&'static str, bool>,
    /// The whole table against the construct, worked out once per revision of what is drawn.
    rows: Option<(u64, Vec<EnzymeRow>)>,
}

/// Which groups stand open at rest: "Doesn't cut" is most of the table and stays shut.
fn open_at_rest(key: GroupKey) -> bool {
    key != GroupKey::None
}

fn site_span(row: &EnzymeRow, site: &CutSite) -> (usize, usize) {
    (site.recognition_start, site.recognition_start + row.site.len().max(1) - 1)
}

/// Thousands grouped, as `toLocaleString` writes a position.
fn grouped(n: usize) -> String {
    bios_layout::overview::scene::group_thousands(n as f64)
}

impl EnzymesPanel {
    /// The rows for `revision` of `c`.
    fn rows(&mut self, revision: u64, c: &Construct) -> &[EnzymeRow] {
        if self.rows.as_ref().is_none_or(|(r, _)| *r != revision) {
            self.rows = Some((revision, enzyme_rows(&c.sequence, c.circular)));
        }
        self.rows.as_ref().map_or(&[], |(_, rows)| rows.as_slice())
    }

    /// Draws the panel in `rect`. `shown` is what is on the map by name.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, revision: u64, c: &Construct, shown: &[String]) -> Option<Outcome> {
        let rows = self.rows(revision, c).to_vec();
        let mut outcome = None;
        let closed = side_panel::show(ui, rect, "enzymes", "Enzymes", |ui, inner| {
            outcome = self.body(ui, inner, &rows, shown);
        });
        if closed { Some(Outcome::Close) } else { outcome }
    }

    fn body(&mut self, ui: &mut Ui, inner: Rect, rows: &[EnzymeRow], shown: &[String]) -> Option<Outcome> {
        let mut outcome = None;
        // `.enz-tools`: the field, then the chips, 8px in, with a rule under.
        let field = Rect::from_min_size(pos2(inner.left() + 8.0, inner.top() + 8.0), vec2(inner.width() - 16.0, 30.0));
        let painter = ui.painter().clone();
        painter.rect_filled(field, CornerRadius::same(theme::RADIUS_SM as u8), theme::CANVAS);
        icon::paint(&painter, pos2(field.left() + 9.0 + 7.0, field.center().y), 14.0, lucide::SEARCH, theme::MUTED);
        let input = Rect::from_min_max(pos2(field.left() + 30.0, field.center().y - 9.0), pos2(field.right() - 9.0, field.center().y + 9.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(theme::sans(13.0))
            .text_color(theme::INK)
            .hint_text(egui::RichText::new("Name, alias or site…").color(theme::MUTED))
            .desired_width(input.width());
        ui.put(input, edit);
        let all = format!("All {}", enzymes().len());
        let chips = [
            (EnzymeFilter::Common, "Common", "The ~78 cloning workhorses"),
            (EnzymeFilter::All, all.as_str(), "Every enzyme in the table"),
            (EnzymeFilter::TypeIis, "IIS", "Type IIS — cuts outside its site (Golden Gate)"),
            (EnzymeFilter::Blunt, "Blunt", "Leaves no overhang"),
        ];
        let segments: Vec<Segment> =
            chips.iter().map(|(f, label, tip)| Segment { face: Face::Label(label), active: *f == self.filter, enabled: true, tip }).collect();
        let chips_top = field.bottom() + 6.0;
        if let Some(i) = group_small_even(ui, pos2(field.left(), chips_top), field.width(), &segments, Material::Float)
            && let Some((filter, _, _)) = chips.get(i)
        {
            self.filter = *filter;
        }
        let tools_bottom = chips_top + 28.0 + 6.0;
        painter.hline(inner.left()..=inner.right(), tools_bottom + 0.5, Stroke::new(1.0, theme::HAIRLINE));

        let list = Rect::from_min_max(pos2(inner.left(), tools_bottom + 1.0), inner.max);
        let groups = group_rows(rows, &self.query, self.filter, shown);
        let hits: usize = groups.iter().map(|g| g.rows.len()).sum();
        let searching = !self.query.trim().is_empty();
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            ui.set_clip_rect(list.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().id_salt("enzymes-list").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if hits == 0 {
                    let kind = match self.filter {
                        EnzymeFilter::TypeIis => " among Type IIS enzymes",
                        EnzymeFilter::Blunt => " among blunt cutters",
                        _ => "",
                    };
                    let text = if searching {
                        format!("No enzyme matches “{}”{kind}.", self.query.trim())
                    } else {
                        "No enzyme of this kind in the table.".to_string()
                    };
                    side_panel::empty(ui, &text);
                }
                for group in groups.iter().filter(|g| !g.rows.is_empty()) {
                    let key = group.key.key();
                    let open = searching || *self.open.get(key).unwrap_or(&open_at_rest(group.key));
                    if side_panel::header(ui, group.key.label(), Some(group.rows.len()), open, None).toggled {
                        self.open.insert(key, !open);
                    }
                    if open {
                        side_panel::body(ui, key, |ui| {
                            for (i, hit) in &group.rows {
                                let Some(row) = rows.get(*i) else { continue };
                                if let Some(o) = item(ui, row, hit.via, group.key == GroupKey::Shown) {
                                    outcome = Some(o);
                                }
                            }
                        });
                    }
                    side_panel::rule(ui);
                }
                ui.add_space(34.0 + 16.0);
            });
        });
        outcome
    }
}

/// One enzyme: name · class · site · end · where, and its sites under it while it is on the map
/// (`EnzymeItem`).
fn item(ui: &mut Ui, row: &EnzymeRow, via: Option<&str>, on: bool) -> Option<Outcome> {
    let cuts = row.sites.len();
    let where_ = match cuts {
        0 => String::new(),
        1 => row.sites.first().map(|s| grouped(s.recognition_start + 1)).unwrap_or_default(),
        n => format!("{n}×"),
    };
    let hang = if row.type2s { "cuts outside its site".to_string() } else { row.overhang_text.clone() };
    let tip = if cuts > 0 {
        let also = via.map(|v| format!(" (also {v})")).unwrap_or_default();
        format!("{} — {}{also} · {} · {hang}", if on { "Take off the map" } else { "Show on the map" }, row.name, row.site)
    } else {
        format!("{} does not cut this construct", row.name)
    };
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_H), if cuts > 0 { Sense::click() } else { Sense::hover() });
    let response = response.on_hover_text(tip);
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, 0.0, theme::CELL_ACTIVE);
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, theme::CUT_SITE);
    } else if cuts > 0 && response.hovered() {
        painter.rect_filled(rect, 0.0, theme::CELL_HOVER);
    }
    if cuts > 0 && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let y = rect.center().y;
    let mut right = rect.right() - 12.0;
    let mut place = |text: &str, font: egui::FontId, ink: egui::Color32| {
        if !text.is_empty() {
            right = painter.text(pos2(right, y), Align2::RIGHT_CENTER, text, font, ink).left() - 6.0;
        }
    };
    place(&where_, theme::mono(10.5), theme::MUTED);
    if !row.type2s {
        place(&row.overhang_text, theme::mono(10.5), theme::MUTED);
    }
    place(row.caret_site.as_deref().unwrap_or(row.site), theme::mono(10.5), theme::CUT_SITE);
    if row.type2s {
        let w = text_width(ui, "IIS", theme::mono(9.5)) + 10.0;
        let tag = Rect::from_min_size(pos2(right - w, y - 7.0), vec2(w, 14.0));
        painter.rect_filled(tag, CornerRadius::same(7), theme::CELL_ACTIVE);
        painter.text(tag.center(), Align2::CENTER_CENTER, "IIS", theme::mono(9.5), theme::MUTED);
        right = tag.left() - 6.0;
    }
    let left = rect.left() + 14.0;
    let name_clip = Rect::from_min_max(pos2(left, rect.top()), pos2(right.max(left), rect.bottom()));
    let clipped = painter.with_clip_rect(name_clip);
    let name = clipped.text(pos2(left, y), Align2::LEFT_CENTER, row.name, theme::sans(12.5), if cuts > 0 { theme::INK } else { theme::MUTED });
    if let Some(via) = via {
        clipped.text(pos2(name.right() + 6.0, y), Align2::LEFT_CENTER, via, theme::sans(11.0), theme::MUTED);
    }
    let mut outcome = response.clicked().then(|| Outcome::Toggle(row.name.to_string()));
    if on && cuts > 0 {
        let (band, _) = ui.allocate_exact_size(vec2(width, 0.0), Sense::hover());
        let mut bottom = band.bottom();
        for site in &row.sites {
            let (r, response) = ui.allocate_exact_size(vec2(width, 3.0 + 12.0 + 3.0), Sense::click());
            let painter = ui.painter();
            painter.rect_filled(r, 0.0, if response.hovered() { theme::CELL_HOVER } else { theme::CELL_ACTIVE });
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let at = painter.text(
                pos2(r.left() + 28.0, r.center().y),
                Align2::LEFT_CENTER,
                grouped(site.recognition_start + 1),
                theme::mono(10.5),
                theme::INK,
            );
            let hang = match site.overhang.kind {
                OverhangKind::Blunt => "blunt".to_string(),
                OverhangKind::FivePrime => format!("5′ {}", site.overhang.seq),
                OverhangKind::ThreePrime => format!("3′ {}", site.overhang.seq),
            };
            let hang = painter.text(pos2(at.right() + 8.0, r.center().y), Align2::LEFT_CENTER, hang, theme::mono(10.5), theme::MUTED);
            if site.strand == Strand::Reverse {
                painter.text(pos2(hang.right() + 8.0, r.center().y), Align2::LEFT_CENTER, "◂", theme::sans(10.0), theme::MUTED);
            }
            if response.clicked() {
                let (start, end) = site_span(row, site);
                outcome = Some(Outcome::GoTo(start, end));
            }
            bottom = r.bottom();
        }
        let (pad, _) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
        ui.painter().rect_filled(Rect::from_min_max(pos2(pad.left(), bottom), pad.max), 0.0, theme::CELL_ACTIVE);
    }
    outcome
}
