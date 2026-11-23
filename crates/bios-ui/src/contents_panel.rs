//! Contents: what is in the open construct, answered on the construct. Ported from Bios
//! `editor/Inspector.tsx` and `leftpanel/PropertiesTab.tsx` (General, Interactions, Features,
//! Parts and Binders, each a collapsible section whose switch is the map layer's own), its look
//! from `styles.css` (`.prop-*`, 3110-3300; `.prop-inter-*`, 8759-8795; `.prop-binder*`, 3931-3966).
//!
//! A press on a feature selects it and opens its card; on a part or a binder it selects it.
//! Parts and binders are not read from the file here: whoever holds them hands them in
//! ([`Listed`], [`BinderRow`]), so the panel lists what the map draws.

use std::collections::HashMap;

use bios_core::{Construct, Strand};
use bios_layout::overview::display::{is_primer_bind, is_source, is_splice_scan, role_for_feature};
use egui::{Align2, CursorIcon, Rect, Sense, Ui, pos2, vec2};

use crate::side_panel::{self, RowFace};
use crate::theme;
use crate::view_tab::Layers;
use bios_core::acts::{Edge, Graph, Target};

/// A part, as the parts lane holds it: its name and span, 0-based inclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub forward: bool,
}

/// One binder held against the construct: one row per oligo, not per site (`binderRows`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinderRow {
    pub name: String,
    pub bases: String,
    /// Where it binds cleanly, 0-based inclusive, when it binds anywhere.
    pub home: Option<(usize, usize)>,
    pub forward: bool,
    /// How many other places it also binds.
    pub strays: usize,
}

/// What the panel asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Close,
    /// A section's switch flipped a layer.
    Layers(Layers),
    /// Select these bases, 0-based inclusive.
    Select(usize, usize),
    /// Select the construct's `index`th feature and open its card.
    Feature(usize),
    /// Take the `index`th binder off.
    RemoveBinder(usize),
}

/// What the panel remembers while it is open: which sections stand open. Features is open at rest:
/// an all-collapsed panel reads as broken.
#[derive(Clone, Debug, Default)]
pub struct ContentsPanel {
    open: HashMap<&'static str, bool>,
}

/// `1–120`: GenBank counts from 1.
fn range(start: usize, end: usize) -> String {
    format!("{}–{}", start + 1, end + 1)
}

/// The features a list shows, in the order a plasmid reads: by where they start. What the editor
/// does not draw is not listed (`extractHidden`): the whole-molecule `source`, and splice-map hits
/// unless they are asked for. Primer sites and parts are drawn on their own layers.
pub fn listed_features(c: &Construct, show_splice_scan: bool) -> Vec<usize> {
    let shown = |f: &bios_core::Feature| {
        !is_primer_bind(&f.kind) && !bios_core::parts::is_part(f) && !is_source(&f.kind) && (show_splice_scan || !is_splice_scan(&f.name))
    };
    let mut list: Vec<usize> = (0..c.features.len()).filter(|i| c.features.get(*i).is_some_and(shown)).collect();
    list.sort_by_key(|i| c.features.get(*i).map_or(0, |f| f.start));
    list
}

/// Features with no stated relationship, once anything on the construct has one: a promoter with
/// nothing to read it is a finding (`idle`).
pub fn idle(c: &Construct, edges: &[Edge], show_splice_scan: bool) -> Vec<usize> {
    if edges.is_empty() {
        return Vec::new();
    }
    listed_features(c, show_splice_scan).into_iter().filter(|i| !edges.iter().any(|e| e.actor == *i || e.target == Target::Feature(*i))).collect()
}

/// The construct's parts (`/pragma="Teselagen_Part"`), in the order they start, as the Parts
/// section lists them.
pub fn listed_parts(c: &Construct) -> Vec<Listed> {
    let mut parts: Vec<Listed> = c
        .features
        .iter()
        .filter(|f| bios_core::parts::is_part(f))
        .map(|f| Listed { name: f.name.clone(), start: f.start, end: f.end, forward: f.strand != Strand::Reverse })
        .collect();
    parts.sort_by_key(|p| p.start);
    parts
}

impl ContentsPanel {
    fn is_open(&self, key: &'static str) -> bool {
        *self.open.get(key).unwrap_or(&(key == "features"))
    }

    fn flip(&mut self, key: &'static str) {
        let now = self.is_open(key);
        self.open.insert(key, !now);
    }

    /// Draws the panel in `rect` over `c`, with `selection` lighting the row it matches.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        c: &Construct,
        selection: Option<(usize, usize)>,
        layers: Layers,
        parts: &[Listed],
        binders: &[BinderRow],
    ) -> Option<Outcome> {
        let mut outcome = None;
        let closed = side_panel::show(ui, rect, "contents", "Contents", |ui, inner| {
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                egui::ScrollArea::vertical().id_salt("contents-body").auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    outcome = self.body(ui, c, selection, layers, parts, binders);
                    ui.add_space(34.0 + 16.0);
                });
            });
        });
        if closed { Some(Outcome::Close) } else { outcome }
    }

    fn body(
        &mut self,
        ui: &mut Ui,
        c: &Construct,
        selection: Option<(usize, usize)>,
        layers: Layers,
        parts: &[Listed],
        binders: &[BinderRow],
    ) -> Option<Outcome> {
        let mut outcome = None;
        // General.
        if side_panel::header(ui, "General", None, self.is_open("general"), None).toggled {
            self.flip("general");
        }
        if self.is_open("general") {
            general(ui, c);
        }
        side_panel::rule(ui);

        // Interactions: the graph as a list, and the one place its silences show.
        let edges = Graph::of(&c.features).edges;
        let head = side_panel::header(ui, "Interactions", Some(edges.len()), self.is_open("interactions"), Some(layers.interactions));
        if head.toggled {
            self.flip("interactions");
        }
        if head.visibility {
            outcome = Some(Outcome::Layers(Layers { interactions: !layers.interactions, ..layers }));
        }
        if self.is_open("interactions") {
            if edges.is_empty() {
                side_panel::empty(ui, "Nothing has been said about what acts on what. Click a feature and answer “what does this act on?”");
            } else {
                side_panel::body(ui, "interactions", |ui| interactions(ui, c, &edges, layers.splice_scan));
            }
        }
        side_panel::rule(ui);

        // Features.
        let features = listed_features(c, layers.splice_scan);
        let head = side_panel::header(ui, "Features", Some(features.len()), self.is_open("features"), Some(layers.features));
        if head.toggled {
            self.flip("features");
        }
        if head.visibility {
            outcome = Some(Outcome::Layers(Layers { features: !layers.features, ..layers }));
        }
        if self.is_open("features") {
            if features.is_empty() {
                side_panel::empty(ui, "No features on this construct.");
            } else {
                side_panel::body(ui, "features", |ui| {
                    for i in &features {
                        let Some(f) = c.features.get(*i) else { continue };
                        let acts = edges.iter().filter(|e| e.actor == *i || e.target == Target::Feature(*i)).count();
                        let face = RowFace {
                            color: Some(theme::role_color(role_for_feature(&f.kind, &f.name))),
                            name: if f.name.is_empty() { "(unnamed)" } else { &f.name },
                            range: &range(f.start, f.end),
                            reverse: f.strand == Strand::Reverse,
                            selected: selection == Some((f.start, f.end)),
                            acts,
                        };
                        if side_panel::row(ui, &face).clicked() {
                            outcome = Some(Outcome::Feature(*i));
                        }
                    }
                });
            }
        }
        side_panel::rule(ui);

        // Parts.
        let head = side_panel::header(ui, "Parts", Some(parts.len()), self.is_open("parts"), Some(layers.parts));
        if head.toggled {
            self.flip("parts");
        }
        if head.visibility {
            outcome = Some(Outcome::Layers(Layers { parts: !layers.parts, ..layers }));
        }
        if self.is_open("parts") {
            if parts.is_empty() {
                side_panel::empty(ui, "No parts yet.");
            } else {
                side_panel::body(ui, "parts", |ui| {
                    for p in parts {
                        let face = RowFace {
                            color: Some(theme::HAIRLINE_STRONG),
                            name: &p.name,
                            range: &range(p.start, p.end),
                            reverse: !p.forward,
                            selected: selection == Some((p.start, p.end)),
                            acts: 0,
                        };
                        if side_panel::row(ui, &face).clicked() {
                            outcome = Some(Outcome::Select(p.start, p.end));
                        }
                    }
                });
            }
        }
        side_panel::rule(ui);

        // Binders: the oligos held against the construct, which are not in the file.
        let head = side_panel::header(ui, "Binders", Some(binders.len()), self.is_open("binders"), Some(layers.binders));
        if head.toggled {
            self.flip("binders");
        }
        if head.visibility {
            outcome = Some(Outcome::Layers(Layers { binders: !layers.binders, ..layers }));
        }
        if self.is_open("binders") {
            if binders.is_empty() {
                side_panel::empty(ui, "No binders. Select a stretch of the construct, right-click, and pick Binder.");
            } else {
                side_panel::body(ui, "binders", |ui| {
                    for (i, b) in binders.iter().enumerate() {
                        if let Some(o) = binder(ui, b, selection) {
                            outcome = Some(match o {
                                BinderPress::Row => match b.home {
                                    Some((s, e)) => Outcome::Select(s, e),
                                    None => continue,
                                },
                                BinderPress::Remove => Outcome::RemoveBinder(i),
                            });
                        }
                    }
                });
            }
        }
        side_panel::rule(ui);
        outcome
    }
}

/// `.prop-general`: name, length, topology and description, label beside value.
fn general(ui: &mut Ui, c: &Construct) {
    let mut facts: Vec<(&str, String)> = vec![
        ("Name", if c.name.is_empty() { "—".to_string() } else { c.name.clone() }),
        ("Length", format!("{} bp", bios_layout::overview::scene::group_thousands(c.len() as f64))),
        ("Topology", if c.circular { "Circular" } else { "Linear" }.to_string()),
    ];
    let description = crate::plasmid_title::definition(c);
    if !description.is_empty() {
        facts.push(("Description", description));
    }
    let width = ui.available_width();
    let label_w = facts.iter().map(|(l, _)| crate::primitives::text_width(ui, l, theme::sans(11.5))).fold(0.0, f32::max);
    ui.add_space(4.0);
    for (label, value) in facts {
        let value_x = 14.0 + label_w + 12.0;
        let galley = ui.painter().layout(value, theme::sans(12.5), theme::INK, (width - value_x - 12.0).max(40.0));
        let (r, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 5.0), Sense::hover());
        ui.painter().text(pos2(r.left() + 14.0, r.top() + 1.5), Align2::LEFT_TOP, label, theme::sans(11.5), theme::MUTED);
        ui.painter().galley(pos2(r.left() + value_x, r.top()), galley, theme::INK);
    }
    ui.add_space(8.0);
}

/// `.prop-inter-row`: actor, verb, target in three columns, then the features wired to nothing.
fn interactions(ui: &mut Ui, c: &Construct, edges: &[Edge], show_splice_scan: bool) {
    let width = ui.available_width();
    let name = |i: usize| c.features.get(i).map_or("—", |f| f.name.as_str());
    for e in edges {
        let (r, _) = ui.allocate_exact_size(vec2(width, 4.0 + 15.0 + 4.0), Sense::hover());
        let painter = ui.painter();
        let verb = painter.layout_no_wrap(e.verb.clone(), theme::sans(11.0), theme::MUTED);
        let mid = r.center().x;
        let verb_rect = Rect::from_center_size(pos2(mid, r.center().y), verb.size());
        let side = ((r.width() - 12.0 - verb_rect.width() - 12.0) / 2.0).max(0.0);
        let actor_clip = Rect::from_min_size(pos2(r.left() + 6.0, r.top()), vec2(side, r.height()));
        let target_clip = Rect::from_min_size(pos2(r.right() - 6.0 - side, r.top()), vec2(side, r.height()));
        painter.with_clip_rect(actor_clip).text(
            pos2(actor_clip.left(), r.center().y),
            Align2::LEFT_CENTER,
            name(e.actor),
            theme::sans(12.0),
            theme::INK,
        );
        painter.galley(verb_rect.min, verb, theme::MUTED);
        let target = match e.target {
            Target::Feature(i) => name(i),
            Target::Construct => "this construct",
        };
        painter.with_clip_rect(target_clip).text(
            pos2(target_clip.right(), r.center().y),
            Align2::RIGHT_CENTER,
            target,
            theme::sans(12.0),
            theme::INK,
        );
    }
    let idle = idle(c, edges, show_splice_scan);
    if let [one] = idle.as_slice() {
        let n = c.features.get(*one).map_or("", |f| f.name.as_str());
        idle_line(ui, &format!("{} is not wired to anything.", if n.is_empty() { "One feature" } else { n }));
    } else if !idle.is_empty() {
        idle_line(ui, &format!("{} features are not wired to anything.", idle.len()));
    }
}

fn idle_line(ui: &mut Ui, text: &str) {
    let width = ui.available_width();
    ui.add_space(4.0);
    let galley = ui.painter().layout(text.to_string(), theme::sans(11.5), theme::MUTED, width - 12.0);
    let (r, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 12.0), Sense::hover());
    ui.painter().hline(r.left()..=r.right(), r.top() + 0.5, egui::Stroke::new(1.0, theme::HAIRLINE));
    ui.painter().galley(pos2(r.left() + 6.0, r.top() + 6.0), galley, theme::MUTED);
}

enum BinderPress {
    Row,
    Remove,
}

/// A binder: its row, its bases, and the strays and the remove under them (`.prop-binder`).
fn binder(ui: &mut Ui, b: &BinderRow, selection: Option<(usize, usize)>) -> Option<BinderPress> {
    let range_text = b.home.map_or_else(|| "binds nowhere".to_string(), |(s, e)| range(s, e));
    let face = RowFace {
        color: None,
        name: &b.name,
        range: &range_text,
        reverse: b.home.is_some() && !b.forward,
        selected: b.home.is_some() && selection == b.home,
        acts: 0,
    };
    let mut pressed = side_panel::row(ui, &face).clicked().then_some(BinderPress::Row);
    let width = ui.available_width();
    let galley = ui.painter().layout(b.bases.clone(), theme::mono(11.0), theme::MUTED, width - 34.0);
    let (r, _) = ui.allocate_exact_size(vec2(width, galley.size().y + 2.0), Sense::hover());
    ui.painter().galley(pos2(r.left() + 26.0, r.top()), galley, theme::MUTED);
    let (meta, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    if b.strays > 0 {
        let said = format!("also binds {} other place{}", b.strays, if b.strays > 1 { "s" } else { "" });
        ui.painter().text(pos2(meta.left() + 26.0, meta.center().y), Align2::LEFT_CENTER, said, theme::sans(11.0), theme::DANGER_TEXT);
    }
    let remove = Rect::from_min_max(pos2(meta.right() - 60.0, meta.top()), meta.max);
    let response = ui.interact(remove, ui.scope_id().with(("binder-remove", &b.name)), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = if response.hovered() { theme::INK } else { theme::MUTED };
    ui.painter().text(pos2(meta.right() - 12.0, meta.center().y), Align2::RIGHT_CENTER, "remove", theme::sans(11.0), ink);
    if response.clicked() {
        pressed = Some(BinderPress::Remove);
    }
    pressed
}
