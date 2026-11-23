//! What this feature does, and to what: the feature panel's `Acts on` and `Acted on by`.
//!
//! Ported from Bios `editor/feature/ActsOn.tsx` (`ActsOn`, `ActedOnBy`) and its rules in
//! `styles.css` (`.acts*`, lines 8947-9145). Three decisions carried over from there:
//!
//! 1. The verb is free text. What the box offers is the verbs already used on this construct, so
//!    the vocabulary grows out of what people say.
//! 2. One verb, many targets. A different verb is a different line.
//! 3. Nothing is saved, because nothing is ever unsaved. A tick lands the moment it is ticked; the
//!    verb lands when you stop typing or leave the box. ⌘Z is the Cancel.
//!
//! What the person decides comes back from [`show`] as the feature's whole list of statements,
//! for the caller to write with the `set_feature_actions` command.

use bios_core::acts::{self, Action, FOR_CONSTRUCT};
use bios_core::{Construct, Feature};
use egui::{Align2, Color32, CornerRadius, CursorIcon, Id, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::theme::{self, Weight};
use crate::{icon, lucide};

/// How long after the last key a typed verb is written: one undo step per pause, not per key.
const SETTLE: f64 = 0.6;
/// `.acts-pick { max-height: 214px }`.
const PICK_MAX_H: f32 = 214.0;
const PICK_ROW_H: f32 = 12.5 * 1.25 + 10.0;

/// A feature that can be pointed at, as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    /// What the statement stores: the feature's Bios id, or `#n` for one that has none yet (the
    /// command gives it one).
    pub key: String,
    pub name: String,
    pub kind: String,
    /// 1-based, as the list prints it.
    pub start: usize,
    pub end: usize,
    pub color: Color32,
}

/// One statement as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub verb: String,
    /// The stored targets: ids, `#n`, or `construct`.
    pub keys: Vec<String>,
    /// What each target is called; "something removed" for an id nothing carries any more.
    pub names: Vec<String>,
    pub colors: Vec<Color32>,
}

/// The statement being written: which line it is (past the end for a new one), and the draft.
#[derive(Clone, Debug, Default, PartialEq)]
struct Draft {
    at: usize,
    verb: String,
    picked: Vec<String>,
    filter: String,
    /// When the verb was last typed in, in the window's seconds, until it is written.
    typed: Option<f64>,
    /// The verb box takes the keys on the first frame.
    focus: bool,
}

/// The section's state while the panel is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActsOn {
    pub statements: Vec<Statement>,
    /// Every other feature on the construct.
    pub picks: Vec<Pick>,
    /// Verbs already used on this construct, offered to the verb box.
    pub verbs: Vec<String>,
    /// Who acts on this feature and how: name, verb and colour. Derived, never typed here.
    pub acted_on: Vec<(String, String, Color32)>,
    draft: Option<Draft>,
}

/// What the panel lists as a feature: not the `source` record, not a primer, not a part.
fn listed(f: &Feature) -> bool {
    !f.kind.eq_ignore_ascii_case("source") && !f.kind.eq_ignore_ascii_case("primer_bind") && !bios_core::parts::is_part(f)
}

/// The colour a relationship is drawn in: its actor's own colour from the file (`/color`), and
/// the muted ink when the file gives none, as the web draws it (`.seq-inter-line`,
/// `.overview-inter-line` fall back to `--ink-muted`).
pub fn wire_color(f: &Feature) -> Color32 {
    f.note("color")
        .and_then(|v| v.first())
        .and_then(|c| c.trim().strip_prefix('#').filter(|h| h.len() == 6).and_then(|h| u32::from_str_radix(h, 16).ok()))
        .map_or(theme::MUTED, theme::hex)
}

fn color_of(f: &Feature) -> Color32 {
    theme::row_feature_color(&f.kind, &f.name)
}

impl ActsOn {
    /// The section for the construct's `index`th feature.
    pub fn of(c: &Construct, index: usize) -> ActsOn {
        let Some(f) = c.features.get(index) else {
            return ActsOn::default();
        };
        let by_id = |id: &str| c.features.iter().find(|g| acts::feature_id(g) == Some(id));
        let statements = acts::actions_of(f)
            .into_iter()
            .map(|a| {
                let (names, colors) = a
                    .targets
                    .iter()
                    .map(|t| match (t.as_str(), by_id(t)) {
                        (FOR_CONSTRUCT, _) => ("this construct".to_string(), theme::HAIRLINE_STRONG),
                        (_, Some(g)) => (g.name.clone(), color_of(g)),
                        (_, None) => ("something removed".to_string(), theme::HAIRLINE_STRONG),
                    })
                    .unzip();
                Statement { verb: a.verb, keys: a.targets, names, colors }
            })
            .collect();
        let picks = c
            .features
            .iter()
            .enumerate()
            .filter(|(i, g)| *i != index && listed(g))
            .map(|(i, g)| Pick {
                key: acts::feature_id(g).map_or_else(|| format!("#{}", i + 1), str::to_owned),
                name: g.name.clone(),
                kind: bios_core::acts::category(g).unwrap_or(&g.kind).replace('_', " "),
                start: g.start + 1,
                end: g.end + 1,
                color: color_of(g),
            })
            .collect();
        let acted_on = acts::acted_on_by(&c.features, index)
            .into_iter()
            .filter_map(|(actor, verb)| c.features.get(actor).map(|g| (g.name.clone(), verb, color_of(g))))
            .collect();
        ActsOn { statements, picks, verbs: acts::Graph::verbs(&c.features), acted_on, draft: None }
    }

    /// The section re-read from the record just written, still open on the line being written.
    /// A target picked before it had an id is picked by the id it was given.
    pub fn refreshed(&self, fresh: ActsOn) -> ActsOn {
        let draft = self.draft.clone().map(|mut d| {
            if let Some(s) = fresh.statements.get(d.at) {
                d.picked = s.keys.clone();
            }
            d.typed = None;
            d.focus = false;
            d
        });
        ActsOn { draft, ..fresh }
    }

    /// Whether a line is open for writing.
    pub fn is_open(&self) -> bool {
        self.draft.is_some()
    }

    fn actions(&self) -> Vec<Action> {
        self.statements.iter().map(|s| Action { verb: s.verb.clone(), targets: s.keys.clone() }).collect()
    }

    /// The draft written back, when it is a statement: a verb AND something it acts on.
    fn commit(&mut self) -> Option<Vec<Action>> {
        let d = self.draft.as_mut()?;
        d.typed = None;
        if d.verb.trim().is_empty() || d.picked.is_empty() {
            return None;
        }
        let (at, line) = (d.at, Action { verb: d.verb.clone(), targets: d.picked.clone() });
        let mut next = self.actions();
        match next.get_mut(at) {
            Some(slot) => *slot = line,
            None => next.push(line),
        }
        Some(next.into_iter().map(|a| a.cleaned()).filter(Action::is_statement).collect())
    }

    /// Opens a line for writing: statement `at`, or a new one.
    pub fn open(&mut self, at: Option<usize>) {
        let from = at.and_then(|i| self.statements.get(i));
        self.draft = Some(Draft {
            at: at.unwrap_or(self.statements.len()),
            verb: from.map(|s| s.verb.clone()).unwrap_or_default(),
            picked: from.map(|s| s.keys.clone()).unwrap_or_default(),
            focus: true,
            ..Draft::default()
        });
    }
}

/// The ids of the section's text boxes, so the panel's Escape knows they are its own.
pub fn field_ids(base: Id) -> [Id; 2] {
    [base.with(("acts", "verb")), base.with(("acts", "filter"))]
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::sans_weight(11.5, Weight::Semibold)).color(theme::MUTED));
}

/// `.acts-chip`: a pill on `--cell-sunken` with a dot of the feature's colour.
fn chip(ui: &mut Ui, name: &str, color: Color32) {
    let font = theme::sans(12.0);
    let width = crate::primitives::text_width(ui, name, font.clone()) + 4.0 + 7.0 + 4.0 + 6.0;
    let (r, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(r, CornerRadius::same(9), theme::CELL_SUNKEN);
    painter.circle_filled(pos2(r.left() + 4.0 + 3.5, r.center().y), 3.5, color);
    painter.text(pos2(r.left() + 4.0 + 7.0 + 4.0, r.center().y), Align2::LEFT_CENTER, name, font, theme::INK);
}

/// `.acts-drop` / `.acts-add` / `.acts-done`: a quiet text button that washes on hover.
fn quiet_button(ui: &mut Ui, at: Rect, id: Id, glyph: Option<&[icon::Node]>, text: &str, ink: Color32) -> egui::Response {
    let response = ui.interact(at, id, Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(at, CornerRadius::same(theme::RADIUS_SM as u8), theme::CELL_HOVER);
    }
    let mut x = at.left() + 6.0;
    if let Some(glyph) = glyph {
        icon::paint(ui.painter(), pos2(x + 6.0, at.center().y), 12.0, glyph, ink);
        x += 12.0 + 5.0;
    }
    if !text.is_empty() {
        ui.painter().text(pos2(x, at.center().y), Align2::LEFT_CENTER, text, theme::sans(13.0), ink);
    }
    response
}

/// Draws the section. Gives the feature's whole new list of statements when the person decided
/// something this frame; `None` when nothing is to be written. `editable` false draws the web's
/// read-only state: no rows to open, no add button.
pub fn show(ui: &mut Ui, acts: &mut ActsOn, base: Id, editable: bool) -> Option<Vec<Action>> {
    let mut out = None;
    let width = ui.available_width();
    label(ui, "Acts on");
    if !acts.statements.is_empty() {
        ui.add_space(4.0);
    }
    let open_at = acts.draft.as_ref().map(|d| d.at);
    let mut clicked_row = None;
    let mut dropped = None;
    for (i, s) in acts.statements.iter().enumerate() {
        let top = ui.cursor().min;
        let inner = ui.allocate_ui(vec2(width - if editable { 26.0 } else { 0.0 }, 0.0), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(7.0, 4.0);
                let mut verb = RichText::new(&s.verb).font(theme::sans_weight(13.0, Weight::Book)).color(theme::INK);
                if open_at == Some(i) {
                    verb = verb.underline();
                }
                ui.label(verb);
                for (name, color) in s.names.iter().zip(&s.colors) {
                    chip(ui, name, *color);
                }
            });
        });
        let body = Rect::from_min_max(top, pos2(top.x + width - if editable { 26.0 } else { 0.0 }, inner.response.rect.bottom() + 6.0));
        ui.add_space(6.0);
        if editable {
            let row = ui.interact(body, base.with(("acts-row", i)), Sense::click());
            if row.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if row.clicked() {
                clicked_row = Some(i);
            }
            let drop = Rect::from_min_size(pos2(body.right() + 2.0, top.y), vec2(24.0, 22.0));
            if quiet_button(ui, drop, base.with(("acts-drop", i)), Some(lucide::X), "", theme::MUTED).on_hover_text("Remove").clicked() {
                dropped = Some(i);
            }
        }
    }
    if let Some(i) = dropped {
        // Taken away at once, and the line being written with it.
        out = Some(acts.actions().into_iter().enumerate().filter(|(j, _)| *j != i).map(|(_, a)| a).collect());
        acts.draft = None;
    } else if let Some(i) = clicked_row {
        if open_at == Some(i) {
            acts.draft = None;
        } else {
            acts.open(Some(i));
        }
    }

    if editable && acts.draft.is_some() {
        if let Some(next) = draft(ui, acts, base, width) {
            out = Some(next);
        }
    } else if editable {
        ui.add_space(3.0);
        let text = if acts.statements.is_empty() { "What does this act on?" } else { "What else does this act on?" };
        let w = crate::primitives::text_width(ui, text, theme::sans(13.0)) + 12.0 + 5.0 + 12.0;
        let (r, _) = ui.allocate_exact_size(vec2(width, 13.0 * 1.25 + 6.0), Sense::hover());
        let at = Rect::from_min_size(pos2(r.left() - 6.0, r.top()), vec2(w, r.height()));
        if quiet_button(ui, at, base.with("acts-add"), Some(lucide::PLUS), text, theme::INK).clicked() {
            acts.open(None);
        }
    }

    if !acts.acted_on.is_empty() {
        ui.add_space(11.0);
        label(ui, "Acted on by");
        ui.add_space(4.0);
        for (name, verb, color) in &acts.acted_on {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(7.0, 4.0);
                chip(ui, name, *color);
                ui.label(RichText::new(verb).font(theme::sans(13.0)).color(theme::MUTED));
            });
            ui.add_space(6.0);
        }
    }
    out
}

/// `.acts-edit`: the verb box, the search and the list to tick, and Done.
fn draft(ui: &mut Ui, acts: &mut ActsOn, base: Id, width: f32) -> Option<Vec<Action>> {
    let mut out = None;
    let [verb_id, filter_id] = field_ids(base);
    let now = ui.input(|i| i.time);
    let h = 13.0 * 1.45 + 12.0;
    ui.add_space(7.0);
    let (verb_rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let others = acts.picks.len();
    let verbs = acts.verbs.clone();
    let d = acts.draft.as_mut()?;
    let before = d.verb.clone();
    let verb = crate::feature_card::line_input(ui, verb_rect, &mut d.verb, verb_id, "what it does \u{2014} transcribes, cuts, represses\u{2026}");
    if d.focus {
        verb.request_focus();
        d.focus = false;
    }
    if d.verb != before {
        d.typed = Some(now);
    }
    let mut chosen = None;
    if verb.has_focus() {
        chosen = suggestions(ui, verb_rect, &d.verb, &verbs);
    }
    let lost = verb.lost_focus();
    if let Some(v) = chosen {
        d.verb = v;
        d.typed = None;
        out = acts.commit();
    } else if lost {
        out = acts.commit();
    } else if let Some(typed) = acts.draft.as_ref().and_then(|d| d.typed) {
        if now - typed >= SETTLE {
            out = acts.commit();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE - (now - typed)));
        }
    }

    ui.add_space(7.0);
    let (head, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let d = acts.draft.as_mut()?;
    let count = format!("{} picked", d.picked.len());
    let count_w = crate::primitives::text_width(ui, &count, theme::sans(12.0));
    let filter_rect = Rect::from_min_max(head.min, pos2(head.right() - count_w - 8.0, head.bottom()));
    crate::feature_card::line_input(ui, filter_rect, &mut d.filter, filter_id, &format!("Search {others} features"));
    ui.painter().text(pos2(head.right(), head.center().y), Align2::RIGHT_CENTER, count, theme::sans(12.0), theme::MUTED);

    ui.add_space(7.0);
    let q = d.filter.trim().to_lowercase();
    let shown: Vec<Pick> =
        acts.picks.iter().filter(|p| q.is_empty() || p.name.to_lowercase().contains(&q) || p.kind.to_lowercase().contains(&q)).cloned().collect();
    let mut toggled = None;
    let picked = d.picked.clone();
    egui::Frame::new().stroke(Stroke::new(1.0, theme::HAIRLINE)).corner_radius(theme::RADIUS_SM).inner_margin(3.0).show(ui, |ui| {
        ui.set_width(width - 8.0);
        egui::ScrollArea::vertical().id_salt(base.with("acts-pick")).max_height(PICK_MAX_H - 8.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for p in &shown {
                if pick_row(ui, base.with(("acts-pick", &p.key)), picked.contains(&p.key), Some(p.color), &p.name, &p.kind, &range(p)) {
                    toggled = Some(p.key.clone());
                }
            }
            if shown.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), PICK_ROW_H + 6.0), Sense::hover());
                ui.painter().text(pos2(r.left() + 5.0, r.center().y), Align2::LEFT_CENTER, "Nothing matches.", theme::sans(12.5), theme::MUTED);
            }
            // The whole molecule is not a span, so it gets the last row and a rule above it.
            ui.add_space(3.0);
            let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
            ui.painter().hline(rule.x_range(), rule.top(), Stroke::new(1.0, theme::HAIRLINE));
            if pick_row(ui, base.with(("acts-pick", FOR_CONSTRUCT)), picked.iter().any(|k| k == FOR_CONSTRUCT), None, "this whole construct", "", "")
            {
                toggled = Some(FOR_CONSTRUCT.to_string());
            }
        });
    });
    if let Some(key) = toggled
        && let Some(d) = acts.draft.as_mut()
    {
        match d.picked.iter().position(|k| *k == key) {
            Some(at) => {
                d.picked.remove(at);
            }
            None => d.picked.push(key),
        }
        // A tick is a decision, not a draft: it lands now.
        out = acts.commit().or(out);
    }

    ui.add_space(7.0);
    let w = crate::primitives::text_width(ui, "Done", theme::sans(13.0)) + 12.0;
    let (r, _) = ui.allocate_exact_size(vec2(width, 13.0 * 1.25 + 6.0), Sense::hover());
    if quiet_button(ui, Rect::from_min_size(pos2(r.left() - 6.0, r.top()), vec2(w, r.height())), base.with("acts-done"), None, "Done", theme::MUTED)
        .clicked()
    {
        if acts.draft.as_ref().is_some_and(|d| d.typed.is_some()) {
            out = acts.commit().or(out);
        }
        acts.draft = None;
    }
    out
}

fn range(p: &Pick) -> String {
    format!("{}..{}", crate::feature_card::thousands(p.start), crate::feature_card::thousands(p.end))
}

/// `.acts-pick-row`: a tick, the feature's colour, its name, its kind and where it is.
fn pick_row(ui: &mut Ui, id: Id, on: bool, color: Option<Color32>, name: &str, kind: &str, range: &str) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), PICK_ROW_H), Sense::hover());
    let response = ui.interact(r, id, Sense::click());
    let painter = ui.painter();
    let corners = CornerRadius::same(theme::RADIUS_SM as u8);
    if on {
        painter.rect_filled(r, corners, theme::CELL_ACTIVE);
    } else if response.hovered() {
        painter.rect_filled(r, corners, theme::CELL_HOVER);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let mut x = r.left() + 5.0;
    let tick = Rect::from_min_size(pos2(x, r.center().y - 6.5), vec2(13.0, 13.0));
    if on {
        painter.rect_filled(tick, CornerRadius::same(3), theme::INK);
        icon::paint(painter, tick.center(), 11.0, lucide::CHECK, theme::CELL);
    } else {
        painter.rect_stroke(tick, CornerRadius::same(3), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
    }
    x = tick.right() + 7.0;
    let dot = Rect::from_min_size(pos2(x, r.center().y - 4.5), vec2(9.0, 9.0));
    match color {
        Some(c) => {
            painter.rect_filled(dot, corners, c);
        }
        None => {
            painter.rect_stroke(dot, corners, Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
        }
    }
    x = dot.right() + 7.0;
    let small = theme::sans(11.5);
    let range_w = if range.is_empty() { 0.0 } else { crate::primitives::text_width(ui, range, small.clone()) };
    let kind_w = if kind.is_empty() { 0.0 } else { crate::primitives::text_width(ui, kind, small.clone()) };
    let right = r.right() - 5.0;
    painter.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, range, small.clone(), theme::MUTED);
    let kind_right = right - if range.is_empty() { 0.0 } else { range_w + 7.0 };
    painter.text(pos2(kind_right, r.center().y), Align2::RIGHT_CENTER, kind, small, theme::MUTED);
    let name_right = kind_right - if kind.is_empty() { 0.0 } else { kind_w + 7.0 };
    let clip = Rect::from_min_max(pos2(x, r.top()), pos2(name_right.max(x), r.bottom()));
    painter.with_clip_rect(clip).text(pos2(x, r.center().y), Align2::LEFT_CENTER, name, theme::sans(12.5), theme::INK);
    response.clicked()
}

/// The verb box's list (`<datalist>`): the construct's own verbs that contain what is typed. Gives
/// the one clicked.
fn suggestions(ui: &mut Ui, under: Rect, typed: &str, verbs: &[String]) -> Option<String> {
    let q = typed.trim().to_lowercase();
    let matching: Vec<&String> = verbs.iter().filter(|v| v.to_lowercase().contains(&q) && v.to_lowercase() != q).collect();
    if matching.is_empty() {
        return None;
    }
    let mut chosen = None;
    egui::Area::new(Id::unique("acts-verbs")).order(egui::Order::Tooltip).fixed_pos(pos2(under.left(), under.bottom() + 2.0)).fade_in(false).show(
        ui.ctx(),
        |ui| {
            egui::Frame::new().fill(theme::CELL).stroke(Stroke::new(1.0, theme::HAIRLINE)).corner_radius(theme::RADIUS_SM).inner_margin(4.0).show(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for v in matching {
                        let (r, response) = ui.allocate_exact_size(vec2(under.width() - 8.0, 24.0), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(6), theme::ROW_HOVER);
                        }
                        ui.painter().text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, v, theme::sans(13.0), theme::INK);
                        // On the press: the box loses the keys before a click completes.
                        if response.is_pointer_button_down_on() {
                            chosen = Some(v.clone());
                        }
                    }
                },
            );
        },
    );
    chosen
}

#[cfg(test)]
mod tests {
    use bios_core::genbank;

    use super::ActsOn;

    const FILE: &str = "LOCUS       t                         40 bp    DNA     circular SYN 01-JAN-2026\n\
FEATURES             Location/Qualifiers\n     CDS             3..12\n                     /label=\"T7 pol\"\n                     \
/note=\"bios-id: a\"\n                     /note=\"bios-act: transcribes > b, gone\"\n     promoter        complement(20..30)\n                     \
/label=\"pT7\"\n                     /note=\"bios-id: b\"\n     misc_feature    32..35\n                     /label=\"spacer\"\n\
ORIGIN\n        1 acgtggatcc gatcaaaaac ccccgggggt tttttacgta\n//\n";

    #[test]
    fn a_tick_writes_the_whole_list_and_a_target_without_an_id_is_named_by_its_place() {
        let c = genbank::read(FILE).unwrap();
        let mut acts = ActsOn::of(&c, 0);
        assert_eq!(acts.statements[0].names, ["pT7", "something removed"]);
        assert_eq!(acts.picks.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["b", "#3"], "the actor is not its own target");
        assert_eq!(acts.verbs, ["transcribes"]);

        acts.open(None);
        acts.draft.as_mut().unwrap().picked.push("#3".into());
        assert!(acts.commit().is_none(), "no verb yet: not a statement, nothing written");
        acts.draft.as_mut().unwrap().verb = "spaces".into();
        let next = acts.commit().unwrap();
        assert_eq!(next.len(), 2);
        assert_eq!((next[1].verb.as_str(), next[1].targets.as_slice()), ("spaces", ["#3".to_string()].as_slice()));
        assert_eq!(next[0].targets, ["b", "gone"], "what was there is written back as it was");

        let promoter = ActsOn::of(&c, 1);
        assert_eq!(promoter.acted_on.iter().map(|(n, v, _)| (n.as_str(), v.as_str())).collect::<Vec<_>>(), [("T7 pol", "transcribes")]);
    }
}
