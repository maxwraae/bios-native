//! The projects page: every project as a card with its map, a search over them, the rail's All /
//! Pinned rows with their counts, pins and a hand-made order by dragging, rename, remove from the
//! list and delete on the card, and tiles that make a new project or open a folder. Ported from
//! the web's
//! `projects/ProjectsHome.tsx` (`ProjectCard`, `NewProjectTile`, the grid and its drag model),
//! `projects/ProjectsRail.tsx`, `projects/ProjectMap.tsx` and `projects/projects.css`.
//!
//! Where it differs from the web: a project is a folder anywhere on the computer, so a card also
//! says where its folder is, and the page lists the folders opened lately rather than everything
//! on a server (as a code editor's start page does). Not here, as in the web without its assistant: the "What do you want to make?" box and its
//! examples, invitations and the people on a card, and the Share button. Bios on your own machine
//! has no accounts.

use bios_core::Construct;
use bios_layout::projects::{self, Band, CardMap, Filter, Mark};
use egui::epaint::PathShape;
use egui::{Align2, Color32, CornerRadius, CursorIcon, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, UiBuilder, pos2, vec2};

use crate::theme::{self, Weight};
use crate::{icon, lucide, map, primitives};

/// The map a card draws, worked out once when the card is read.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumb {
    map: CardMap,
    /// Each feature's tint, by the index `map` names it by.
    colors: Vec<Color32>,
}

/// `ProjectMap`'s input: the construct's features as the map families them, drawn as tints.
pub fn thumb(c: &Construct) -> Thumb {
    let scene = map::scene_of(c);
    let spans: Vec<(i64, i64)> = scene.features.iter().map(|f| (f.start, f.end)).collect();
    let colors = scene.features.iter().map(|f| theme::mix(theme::role_color(f.role), theme::CELL_SUNKEN, theme::TINT_RING)).collect();
    Thumb { map: projects::card_map(scene.length, scene.circular, &spans), colors }
}

/// One project, as its card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// What a pin or the order remembers it by: the folder's path.
    pub id: String,
    pub name: String,
    /// "4 constructs · today".
    pub meta: String,
    /// Where the folder is, short: "~/Documents/Bios projects".
    pub place: String,
    /// `None` for a project with nothing in it yet.
    pub thumb: Option<Thumb>,
}

/// What the page shows.
pub struct Model<'a> {
    /// Every project, in the order the page keeps: by hand once arranged, else the latest first.
    pub cards: &'a [Card],
    /// Pinned project ids, in the order they are drawn.
    pub pinned: &'a [String],
    /// The order was made by hand, so the header says so and offers the way back.
    pub arranged: bool,
    pub filter: Filter,
}

/// What was asked for on the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Chosen {
    Open(String),
    Rename {
        id: String,
        name: String,
    },
    /// The card asked first, in place: this is the person's yes to moving the folder to the Trash.
    Delete(String),
    /// Take it off the list; the folder stays where it is.
    Remove(String),
    /// The pinned ids, in their new order.
    Pins(Vec<String>),
    /// Every project id, in the order arranged by hand.
    Order(Vec<String>),
    /// "Sort by recent": forget the order made by hand.
    SortByRecent,
    /// The New project tile: ask where to make it and what to call it.
    NewProject,
    /// The Open folder tile.
    OpenFolder,
    /// "Drop a plasmid to start from one", pressed rather than dropped onto.
    PickFiles,
}

/// A press on a card that may become a drag, and the drag once it has.
struct Drag {
    id: String,
    from: Pos2,
    /// The two bands as the drag has rearranged them, once it is a drag.
    live: Option<(Vec<String>, Vec<String>)>,
}

/// The page's own state: the search, a card being renamed or asked about.
#[derive(Default)]
pub struct ProjectsHome {
    pub query: String,
    renaming: Option<(String, String, bool)>,
    confirming: Option<String>,
    drag: Option<Drag>,
    /// Where each card was drawn last frame, for the drag to land on.
    drawn: Vec<(Band, String, Rect)>,
    bands: Vec<(Band, Rect)>,
}

const HEAD_H: f32 = 59.0;
/// `.rail-head` and `.rail-nav-item`.
const RAIL_HEAD: f32 = 44.0;
const RAIL_ROW: f32 = 32.0;

fn r(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, 255.0) as u8)
}

/// `ProjectsRail`: the wordmark, then All projects and Pinned, each with its count.
pub fn rail(ui: &mut egui::Ui, rect: Rect, filter: Filter, counts: (usize, usize)) -> Option<Filter> {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, theme::RAIL_GROUND);
    painter.text(pos2(rect.left() + 14.0, rect.top() + 12.0 + 14.0), Align2::LEFT_CENTER, "Bios", theme::serif(19.0), theme::INK);
    let mut chosen = None;
    let mut top = rect.top() + RAIL_HEAD + 1.0;
    for (which, label, count) in [(Filter::All, "All projects", counts.0), (Filter::Pinned, "Pinned", counts.1)] {
        let row = Rect::from_min_size(pos2(rect.left() + 4.0, top), vec2(rect.width() - 8.0, RAIL_ROW));
        let response = ui.interact(row, ui.scope_id().with(("projects-filter", label)), Sense::click());
        let active = filter == which;
        primitives::row_ground(&painter, row, response.hovered(), active);
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let font = if active { theme::sans_weight(13.0, Weight::Semibold) } else { theme::sans(13.0) };
        painter.text(pos2(row.left() + 10.0, row.center().y), Align2::LEFT_CENTER, label, font, theme::INK);
        painter.text(pos2(row.right() - 10.0, row.center().y), Align2::RIGHT_CENTER, count.to_string(), theme::mono(11.0), theme::MUTED);
        if response.clicked() {
            chosen = Some(which);
        }
        top = row.bottom() + 2.0;
    }
    let rule = top + 8.0 - 1.0;
    painter.line_segment([pos2(rect.left(), rule), pos2(rect.right(), rule)], Stroke::new(1.0, theme::HAIRLINE));
    chosen
}

/// A 24px button in a card's foot (`.oge-project-card-act`).
fn act(ui: &mut egui::Ui, rect: Rect, glyph: &[icon::Node], tip: &str, on: bool, danger: bool) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("card-act", tip)), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, r(theme::RADIUS_SM), theme::alpha(theme::INK, 0.06));
    }
    let ink = match (hovered, danger, on) {
        (true, true, _) => theme::DANGER_TEXT,
        (true, false, _) | (_, _, true) => theme::INK,
        _ => theme::MUTED,
    };
    icon::paint(ui.painter(), rect.center(), 13.0, glyph, ink);
    response.on_hover_text(tip).clicked()
}

/// `.oge-project-card-yes` / `-no`: half the foot each.
fn answer(ui: &mut egui::Ui, rect: Rect, text: &str, danger: bool) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("card-answer", text)), Sense::click());
    let (fill, edge, ink) = if danger {
        (theme::DANGER_BG, theme::DANGER_BORDER, theme::DANGER_TEXT)
    } else if response.hovered() {
        (theme::ROW_HOVER, theme::HAIRLINE_STRONG, theme::INK)
    } else {
        (Color32::TRANSPARENT, theme::HAIRLINE_STRONG, theme::INK)
    };
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().rect_filled(rect, r(theme::RADIUS_SM), fill);
    ui.painter().rect_stroke(rect, r(theme::RADIUS_SM), Stroke::new(1.0, edge), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, theme::sans(12.0), ink);
    response.clicked()
}

/// A single-line field with no frame of its own. `Some(true)` when Enter or a click away
/// commits it, `Some(false)` on Escape.
fn field(ui: &mut egui::Ui, rect: Rect, text: &mut String, focused: &mut bool, hint: &str, size: f32) -> Option<bool> {
    let edit = egui::TextEdit::singleline(text)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(theme::sans(size))
        .text_color(theme::INK)
        .hint_text(egui::RichText::new(hint).color(theme::MUTED))
        .desired_width(rect.width());
    let response = ui.put(rect, edit);
    if !*focused {
        response.request_focus();
        *focused = true;
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) && (response.has_focus() || response.lost_focus()) {
        return Some(false);
    }
    response.lost_focus().then_some(true)
}

/// Paints the map on a card into the square `art`.
fn paint_thumb(painter: &egui::Painter, art: Rect, thumb: &Thumb) {
    // `.oge-project-map`: 88% of the art, the map's coordinates scaled to it.
    let side = art.width().min(art.height()) * 0.88;
    let scale = side / projects::MAP_SIZE as f32;
    let origin = art.center() - vec2(side, side) / 2.0;
    let at = |x: f64, y: f64| origin + vec2(x as f32 * scale, y as f32 * scale);
    let m = &thumb.map;
    let track = projects::TRACK as f32 * scale;
    if !m.circular {
        let y = m.cy;
        painter.line_segment([at(m.line_x0, y), at(m.line_x0 + m.line_w, y)], Stroke::new(1.0, theme::HAIRLINE_STRONG));
        for mark in &m.marks {
            if let Mark::Bar { feature, left, width } = mark {
                let rect = Rect::from_min_size(at(*left, y - projects::TRACK / 2.0), vec2(*width as f32 * scale, track));
                painter.rect_filled(rect, 0.0, thumb.colors.get(*feature).copied().unwrap_or(theme::HAIRLINE_STRONG));
            }
        }
        return;
    }
    let center = at(m.cx, m.cy);
    let radius = m.r as f32 * scale;
    painter.circle_stroke(center, radius, Stroke::new(1.0, theme::HAIRLINE_STRONG));
    let on_ring = |frac: f64, radius: f32| {
        let theta = frac * std::f64::consts::TAU;
        center + vec2(radius * theta.sin() as f32, -radius * theta.cos() as f32)
    };
    for mark in &m.marks {
        if let Mark::Arc { feature, from, to } = mark {
            let steps = (((to - from) * 160.0).ceil() as usize).clamp(2, 400);
            let points: Vec<Pos2> = (0..=steps).map(|s| on_ring(from + (to - from) * s as f64 / steps as f64, radius)).collect();
            let color = thumb.colors.get(*feature).copied().unwrap_or(theme::HAIRLINE_STRONG);
            painter.add(Shape::Path(PathShape::line(points, Stroke::new(track, color))));
        }
    }
    // The origin mark at 12 o'clock: it says which way the coordinates run.
    painter.line_segment([on_ring(0.0, radius - track), on_ring(0.0, radius + track)], Stroke::new(1.0, theme::MUTED));
}

/// `text` cut to `width` from the front, so the end of a path (the folder it is in) stays.
fn clip_start(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    if primitives::text_width(ui, text, font.clone()) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate = format!("…{}", chars.get(chars.len() - mid..).unwrap_or_default().iter().collect::<String>());
        if primitives::text_width(ui, &candidate, font.clone()) <= width { lo = mid } else { hi = mid - 1 }
    }
    format!("…{}", chars.get(chars.len() - lo..).unwrap_or_default().iter().collect::<String>())
}

/// `NewProjectTile`, and the Open folder tile beside it, drawn the same way.
fn tile(ui: &mut egui::Ui, at: Rect, glyph: &[icon::Node], label: &str) -> bool {
    let response = ui.interact(at, ui.scope_id().with(("project-tile", label)), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        painter.rect_filled(at, r(theme::RADIUS), theme::alpha(theme::INK, 0.03));
    }
    // A dashed outline round the rounded rectangle.
    let corner = theme::RADIUS;
    let mut ring = Vec::new();
    for (c, start) in [
        (pos2(at.right() - corner, at.top() + corner), -90.0f32),
        (pos2(at.right() - corner, at.bottom() - corner), 0.0),
        (pos2(at.left() + corner, at.bottom() - corner), 90.0),
        (pos2(at.left() + corner, at.top() + corner), 180.0),
    ] {
        for s in 0..=6 {
            let a = (start + 15.0 * s as f32).to_radians();
            ring.push(c + vec2(a.cos(), a.sin()) * corner);
        }
    }
    if let Some(first) = ring.first().copied() {
        ring.push(first);
    }
    painter.extend(Shape::dashed_line(&ring, Stroke::new(1.0, theme::HAIRLINE_STRONG), 3.0, 3.0));
    let art = Rect::from_min_size(at.min, vec2(at.width(), at.width()));
    let ink = if response.hovered() { theme::INK } else { theme::MUTED };
    icon::paint(painter, art.center(), 22.0, glyph, ink);
    painter.text(pos2(at.center().x, art.bottom() + 8.0 + 15.0), Align2::CENTER_CENTER, label, theme::sans(13.0), ink);
    response.on_hover_text(label).clicked()
}

impl ProjectsHome {
    /// Draws the page in `rect` and gives back what was asked for.
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, model: &Model<'_>) -> Vec<Chosen> {
        let mut out = Vec::new();
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, theme::CANVAS);

        // The header: the page's name, the order when it was made by hand, the search.
        let head = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD_H));
        painter.line_segment([pos2(head.left(), head.bottom() - 0.5), pos2(head.right(), head.bottom() - 0.5)], Stroke::new(1.0, theme::HAIRLINE));
        painter.text(pos2(head.left() + 20.0, head.center().y), Align2::LEFT_CENTER, "Projects", theme::serif(20.0), theme::INK);
        let find = Rect::from_min_size(pos2(head.right() - 20.0 - 210.0, head.center().y - 15.0), vec2(210.0, 30.0));
        painter.rect_filled(find, r(theme::RADIUS_SM), theme::CELL);
        icon::paint(&painter, pos2(find.left() + 9.0 + 7.0, find.center().y), 14.0, lucide::SEARCH, theme::MUTED);
        let input = Rect::from_min_max(pos2(find.left() + 30.0, find.center().y - 9.0), pos2(find.right() - 9.0, find.center().y + 9.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(theme::sans(13.0))
            .text_color(theme::INK)
            .hint_text(egui::RichText::new("Search projects").color(theme::MUTED))
            .desired_width(input.width());
        let response = ui.put(input, edit);
        if response.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.query.clear();
        }
        if model.arranged {
            let reset = "Sort by recent";
            let reset_w = primitives::text_width(ui, reset, theme::sans(12.0));
            let reset_rect = Rect::from_min_size(pos2(find.left() - 14.0 - reset_w, head.center().y - 10.0), vec2(reset_w, 20.0));
            let response = ui.interact(reset_rect, ui.scope_id().with("projects-order-reset"), Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let ink = if response.hovered() { theme::INK } else { theme::MUTED };
            painter.text(reset_rect.left_center(), Align2::LEFT_CENTER, reset, theme::sans(12.0), ink);
            painter.text(pos2(reset_rect.left() - 8.0, head.center().y), Align2::RIGHT_CENTER, "Your order", theme::sans(12.0), theme::MUTED);
            if response.clicked() {
                out.push(Chosen::SortByRecent);
            }
        }

        // What the filter and the search let through, in the two bands.
        let q = self.query.trim().to_lowercase();
        let matches =
            |card: &Card| (q.is_empty() || card.name.to_lowercase().contains(&q)) && projects::matches_filter(&card.id, model.filter, model.pinned);
        let by_id = |id: &str| model.cards.iter().find(|c| c.id == id);
        let rest_pinned: Vec<String> = model.pinned.iter().filter(|id| by_id(id).is_some_and(matches)).cloned().collect();
        let rest_recent: Vec<String> = model.cards.iter().filter(|c| !model.pinned.contains(&c.id) && matches(c)).map(|c| c.id.clone()).collect();
        let shown = rest_pinned.len() + rest_recent.len();
        let can_drag = q.is_empty() && model.filter == Filter::All && shown > 0;
        let dragging = self.drag.as_ref().and_then(|d| d.live.as_ref().map(|_| d.id.clone()));
        let (pinned_cards, recent_cards) = match self.drag.as_ref().and_then(|d| d.live.clone()) {
            Some(live) => live,
            None => (rest_pinned.clone(), rest_recent.clone()),
        };

        let body = Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max);
        let grid = projects::grid(f64::from(body.width()));
        let mut drawn = Vec::new();
        let mut bands = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("projects-home").show(ui, |ui| {
                let top = ui.min_rect().top();
                let left = body.left() + grid.left as f32;
                let mut y = top + 28.0;
                if model.cards.is_empty() && model.filter == Filter::All {
                    let line = "Drop a plasmid to start from one";
                    let w = primitives::text_width(ui, line, theme::sans(13.0)) + 28.0;
                    let drop = Rect::from_min_size(pos2(left, y), vec2(w, 34.0));
                    let response = ui.interact(drop, ui.scope_id().with("projects-drop"), Sense::click());
                    ui.painter().extend(Shape::dashed_line(
                        &[drop.left_top(), drop.right_top(), drop.right_bottom(), drop.left_bottom(), drop.left_top()],
                        Stroke::new(1.0, theme::HAIRLINE_STRONG),
                        4.0,
                        3.0,
                    ));
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    ui.painter().text(drop.center(), Align2::CENTER_CENTER, line, theme::sans(13.0), theme::MUTED);
                    if response.clicked() {
                        out.push(Chosen::PickFiles);
                    }
                    y = drop.bottom() + 28.0;
                }
                let band_label = |ui: &egui::Ui, y: f32, text: &str| {
                    ui.painter().text(pos2(left, y + 7.0), Align2::LEFT_CENTER, text, theme::sans_weight(12.0, Weight::Medium), theme::MUTED);
                };
                let two_bands = !pinned_cards.is_empty() || dragging.is_some();
                if two_bands {
                    band_label(ui, y, "Pinned");
                    y += 16.0 + 6.0;
                    let count = pinned_cards.len();
                    let height = if count == 0 { grid.card_h as f32 * 0.4 } else { grid.height(count) as f32 };
                    let zone = Rect::from_min_size(pos2(left, y), vec2((body.width() - grid.left as f32 * 2.0).max(grid.card_w as f32), height));
                    bands.push((Band::Pinned, zone));
                    if count == 0 {
                        ui.painter().extend(Shape::dashed_line(
                            &[zone.left_top(), zone.right_top(), zone.right_bottom(), zone.left_bottom(), zone.left_top()],
                            Stroke::new(1.0, theme::INK),
                            4.0,
                            3.0,
                        ));
                        let hint = "Drop a project here to keep it at the top";
                        ui.painter().text(pos2(zone.left() + 18.0, zone.center().y), Align2::LEFT_CENTER, hint, theme::sans(13.0), theme::INK);
                    }
                    for (i, id) in pinned_cards.iter().enumerate() {
                        let (x, dy) = grid.at(i);
                        let at = Rect::from_min_size(pos2(body.left() + x as f32, y + dy as f32), vec2(grid.card_w as f32, grid.card_h as f32));
                        if let Some(card) = by_id(id) {
                            self.card(ui, at, card, true, dragging.as_deref() == Some(id.as_str()), model.pinned, &mut out);
                            drawn.push((Band::Pinned, id.clone(), at));
                        }
                    }
                    y += height + 24.0;
                    band_label(ui, y, "Recent");
                    y += 16.0 + 6.0;
                }
                let new_tile = model.filter == Filter::All;
                let count = recent_cards.len() + if new_tile { 2 } else { 0 };
                let zone_h = (grid.height(count.max(1)) as f32).max(grid.card_h as f32);
                bands.push((Band::Recent, Rect::from_min_size(pos2(left, y), vec2((body.width() - grid.left as f32 * 2.0).max(1.0), zone_h))));
                let mut slot = 0;
                if new_tile {
                    let tiles = [(lucide::PLUS, "New project", Chosen::NewProject), (lucide::FOLDER_OPEN, "Open folder", Chosen::OpenFolder)];
                    for (glyph, label, chosen) in tiles {
                        let (x, dy) = grid.at(slot);
                        let at = Rect::from_min_size(pos2(body.left() + x as f32, y + dy as f32), vec2(grid.card_w as f32, grid.card_h as f32));
                        if tile(ui, at, glyph, label) {
                            out.push(chosen);
                        }
                        slot += 1;
                    }
                }
                for id in &recent_cards {
                    let (x, dy) = grid.at(slot);
                    slot += 1;
                    let at = Rect::from_min_size(pos2(body.left() + x as f32, y + dy as f32), vec2(grid.card_w as f32, grid.card_h as f32));
                    if let Some(card) = by_id(id) {
                        self.card(ui, at, card, false, dragging.as_deref() == Some(id.as_str()), model.pinned, &mut out);
                        drawn.push((Band::Recent, id.clone(), at));
                    }
                }
                y += zone_h;
                if (!model.cards.is_empty() || model.filter != Filter::All) && shown == 0 {
                    let said = if !q.is_empty() {
                        format!("No project matches \u{201c}{}\u{201d}.", self.query.trim())
                    } else {
                        "Nothing pinned yet. Press Pin on a card, or drag it to the top.".to_string()
                    };
                    let at = if model.filter == Filter::All { y + 28.0 } else { top + 28.0 };
                    ui.painter().text(pos2(left, at + 8.0), Align2::LEFT_CENTER, said, theme::sans(13.0), theme::MUTED);
                    y = at + 16.0;
                }
                ui.allocate_rect(Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), y + 40.0)), Sense::hover());
            });
        });
        self.drawn = drawn;
        self.bands = bands;
        self.track_drag(ui, can_drag, &rest_pinned, &rest_recent, model, &mut out);
        out
    }

    /// The drag: a press on a card that moves 6px becomes one; the bands rearrange live under it,
    /// and letting go keeps the new pins, or else the new order (`onGridPointerMove` / `Up`).
    fn track_drag(
        &mut self,
        ui: &egui::Ui,
        can_drag: bool,
        rest_pinned: &[String],
        rest_recent: &[String],
        model: &Model<'_>,
        out: &mut Vec<Chosen>,
    ) {
        let (pos, pressed, down, released) =
            ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released()));
        if pressed && can_drag && self.renaming.is_none() && self.confirming.is_none() {
            // A press on a card's foot buttons is aimed at them, not at the card.
            let on_card = pos.and_then(|p| self.drawn.iter().find(|(_, _, rect)| rect.contains(p) && p.y < rect.bottom() - projects::FOOT_H as f32));
            self.drag = on_card.zip(pos).map(|((_, id, _), from)| Drag { id: id.clone(), from, live: None });
        }
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if down && let Some(p) = pos {
            if drag.live.is_none() && p.distance(drag.from) >= 6.0 {
                drag.live = Some((rest_pinned.to_vec(), rest_recent.to_vec()));
            }
            if let Some((pinned, recent)) = drag.live.as_mut() {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                if let Some((zone, _)) = self.bands.iter().find(|(_, rect)| rect.contains(p)) {
                    let over = self.drawn.iter().find(|(band, _, rect)| band == zone && rect.contains(p)).map(|(_, id, _)| id.as_str());
                    projects::drag_to(pinned, recent, &drag.id, *zone, over);
                }
                ui.ctx().request_repaint();
            }
            return;
        }
        if released || !down {
            let finished = self.drag.take();
            if let Some((pinned, recent)) = finished.and_then(|d| d.live) {
                if pinned != rest_pinned {
                    // Pins not shown under a search stay pinned, after the ones arranged here.
                    let hidden = model.pinned.iter().filter(|id| !rest_pinned.contains(id) && !pinned.contains(id) && !recent.contains(id)).cloned();
                    out.push(Chosen::Pins(pinned.iter().cloned().chain(hidden).collect()));
                } else {
                    out.push(Chosen::Order(pinned.into_iter().chain(recent).collect()));
                }
            }
        }
    }

    /// `ProjectCard`: the map over the foot with the name, the count and the day, and on hover
    /// Pin, Rename and Delete.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut egui::Ui, at: Rect, card: &Card, pinned: bool, lifted: bool, all_pins: &[String], out: &mut Vec<Chosen>) {
        let response = ui.interact(at, ui.scope_id().with(("project-card", &card.id)), Sense::click());
        let hovered = ui.rect_contains_pointer(at) && self.drag.as_ref().is_none_or(|d| d.live.is_none());
        let painter = ui.painter().clone();
        if lifted {
            primitives::shadow(&painter, at, theme::RADIUS, theme::SHADOW_2);
        }
        painter.rect_filled(at, r(theme::RADIUS), if hovered { theme::CELL_HOVER } else { theme::CELL });
        let art = Rect::from_min_size(at.min, vec2(at.width(), at.width()));
        let top_only = CornerRadius { nw: r(theme::RADIUS).nw, ne: r(theme::RADIUS).ne, sw: 0, se: 0 };
        painter.rect_filled(art.shrink(0.5), top_only, theme::CELL_SUNKEN);
        painter.line_segment([art.left_bottom(), art.right_bottom()], Stroke::new(1.0, theme::HAIRLINE));
        match &card.thumb {
            Some(thumb) => paint_thumb(&painter, art, thumb),
            None => {
                painter.text(art.center(), Align2::CENTER_CENTER, "Nothing in it yet", theme::sans(12.0), theme::MUTED);
            }
        }
        painter.rect_stroke(at, r(theme::RADIUS), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
        if pinned {
            let badge = Rect::from_center_size(pos2(at.right() - 8.0 - 10.0, at.top() + 8.0 + 10.0), vec2(20.0, 20.0));
            painter.circle_filled(badge.center(), 10.0, theme::FLOAT);
            icon::paint(&painter, badge.center(), 12.0, lucide::PIN, theme::MUTED);
        }

        let foot = Rect::from_min_max(pos2(at.left() + 10.0, art.bottom() + 8.0), pos2(at.right() - 10.0, at.bottom() - 10.0));
        if self.confirming.as_deref() == Some(card.id.as_str()) {
            let half = (foot.width() - 6.0) / 2.0;
            let yes = Rect::from_min_size(pos2(foot.left(), foot.center().y - 13.0), vec2(half, 26.0));
            let no = Rect::from_min_size(pos2(yes.right() + 6.0, yes.top()), vec2(half, 26.0));
            if answer(ui, yes, "Move to Trash", true) {
                out.push(Chosen::Delete(card.id.clone()));
                self.confirming = None;
            }
            if answer(ui, no, "Keep", false) {
                self.confirming = None;
            }
            return;
        }
        let renaming = self.renaming.as_ref().is_some_and(|(id, _, _)| *id == card.id);
        let acts_w = if hovered && !renaming { 4.0 * 24.0 + 3.0 * 2.0 + 6.0 } else { 0.0 };
        let text_w = foot.width() - acts_w;
        if renaming {
            if let Some((id, draft, focused)) = self.renaming.as_mut() {
                let row = Rect::from_min_size(pos2(foot.left(), foot.top()), vec2(text_w, 18.0));
                match field(ui, row, draft, focused, "", 14.0) {
                    Some(true) => {
                        let next = draft.trim().to_string();
                        if !next.is_empty() && next != card.name {
                            out.push(Chosen::Rename { id: id.clone(), name: next });
                        }
                        self.renaming = None;
                    }
                    Some(false) => self.renaming = None,
                    None => {}
                }
            }
        } else {
            let name = crate::session_nav::clip(ui, &card.name, &theme::sans(14.0), text_w);
            painter.text(pos2(foot.left(), foot.top() + 8.0), Align2::LEFT_CENTER, name, theme::sans(14.0), theme::INK);
        }
        let meta = crate::session_nav::clip(ui, &card.meta, &theme::sans(12.0), text_w);
        painter.text(pos2(foot.left(), foot.top() + 8.0 + 17.0), Align2::LEFT_CENTER, meta, theme::sans(12.0), theme::MUTED);
        let place = clip_start(ui, &card.place, &theme::sans(11.0), foot.width());
        painter.text(pos2(foot.left(), foot.top() + 8.0 + 33.0), Align2::LEFT_CENTER, place, theme::sans(11.0), theme::MUTED);
        let mut acted = false;
        if hovered && !renaming {
            let mut x = foot.right() - 24.0 * 4.0 - 6.0;
            let row_y = foot.center().y - 12.0;
            let pin_tip = if pinned { format!("Unpin {}", card.name) } else { format!("Pin {}", card.name) };
            if act(ui, Rect::from_min_size(pos2(x, row_y), vec2(24.0, 24.0)), lucide::PIN, &pin_tip, pinned, false) {
                acted = true;
                let next = if pinned {
                    all_pins.iter().filter(|id| **id != card.id).cloned().collect()
                } else {
                    all_pins.iter().cloned().chain(std::iter::once(card.id.clone())).collect()
                };
                out.push(Chosen::Pins(next));
            }
            x += 26.0;
            if act(ui, Rect::from_min_size(pos2(x, row_y), vec2(24.0, 24.0)), lucide::PENCIL, &format!("Rename {}", card.name), false, false) {
                acted = true;
                self.renaming = Some((card.id.clone(), card.name.clone(), false));
            }
            x += 26.0;
            if act(ui, Rect::from_min_size(pos2(x, row_y), vec2(24.0, 24.0)), lucide::X, "Remove from list (the folder stays)", false, false) {
                acted = true;
                out.push(Chosen::Remove(card.id.clone()));
            }
            x += 26.0;
            if act(
                ui,
                Rect::from_min_size(pos2(x, row_y), vec2(24.0, 24.0)),
                lucide::TRASH_2,
                &format!("Move {} to the Trash", card.name),
                false,
                true,
            ) {
                acted = true;
                self.confirming = Some(card.id.clone());
            }
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let was_drag = self.drag.as_ref().is_some_and(|d| d.live.is_some());
        if hovered && !acted {
            response.clone().on_hover_text(&card.id);
        }
        if response.clicked() && !acted && !renaming && !was_drag {
            out.push(Chosen::Open(card.id.clone()));
        }
    }
}
