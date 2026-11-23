//! The one panel for a feature, opened at the point it was clicked.
//!
//! Ported from the web app: `editor/FeatureCard.tsx` (the panel and its placement, lines 66-294),
//! `editor/categories.ts` (`CATEGORIES`, `categoryFromType`), `editor/feature/ActsOn.tsx` (the
//! read-only `Acts on` and `ActedOnBy`, lines 89-262) and their rules in `styles.css` (`.feature-card`
//! 8465-8530, `.panel-*` 8826-8901, `.acts*` 8947-9145).
//!
//! There is no Save button: a field lands when it is left, a choice the moment it is made, and
//! the panel stays open on the record that was just written. Escape and ✕ close it.

use bios_core::acts::Action;
use bios_core::{Construct, Feature, Strand};
use egui::epaint::Shadow;
use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Key, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::acts_on::ActsOn;
use crate::icon;
use crate::lucide;
use crate::theme::{self, Weight};

/// `.feature-card { width: 340px }`.
pub const WIDTH: f32 = 340.0;
/// `padding: 12px 13px 11px`.
const PAD: Margin = Margin { left: 13, right: 13, top: 12, bottom: 11 };
/// The panel's own offset from the point it describes (`translate(-50%, calc(-100% - 10px))`).
const CLEAR: f32 = 10.0;
/// How close to the window's edges it may come (`FeatureCard.tsx` `margin`).
const EDGE: f32 = 12.0;

/// The closed vocabulary for what a feature is (`categories.ts`), key and label.
pub const CATEGORIES: [(&str, &str); 10] = [
    ("promoter", "Promoter"),
    ("cds", "CDS"),
    ("terminator", "Terminator"),
    ("origin", "Origin"),
    ("tag", "Tag"),
    ("marker", "Marker"),
    ("linker", "Linker"),
    ("insulator", "Insulator"),
    ("primer", "Primer"),
    ("misc", "Misc"),
];

/// The record the panel writes, its span 1-based as a person reads it (`FeatureEdit`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureEdit {
    pub name: String,
    /// One of [`CATEGORIES`]' keys, or empty when nothing has been said.
    pub category: String,
    pub forward: bool,
    pub start: usize,
    pub end: usize,
    /// Why it is here (`bios-use`).
    pub purpose: String,
    /// What it does (`bios-description`).
    pub description: String,
}

/// The panel while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureCard {
    /// Which of the construct's features this is.
    pub feature: usize,
    /// The point it was opened at, on screen. Taken from the pointer on the first frame.
    pub anchor: Option<Pos2>,
    /// The record as the construct has it.
    pub was: FeatureEdit,
    /// The record as the person has it so far.
    pub edit: FeatureEdit,
    /// What the empty choice of the kind says: the GenBank type, or "unspecified".
    pub type_word: String,
    /// Who said what is written here (`bios-use-source`): `human` or anything else.
    pub source: Option<String>,
    /// What it acts on, and what acts on it.
    pub acts: ActsOn,
    start_text: String,
    end_text: String,
    /// The list of kinds is open.
    picking: bool,
    /// How tall it was last frame, to place it above or below.
    height: f32,
}

/// What the person did this frame. A field left and ✕ pressed at once is both.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Write [`FeatureCard::edit`].
    pub commit: bool,
    pub close: bool,
    /// Write these as everything the feature acts on (`set_feature_actions`).
    pub actions: Option<Vec<Action>>,
}

/// The value of a `bios-<key>: value` note.
fn note<'a>(f: &'a Feature, key: &str) -> Option<&'a str> {
    let prefix = format!("bios-{key}:");
    f.note("note")?.iter().find_map(|n| n.strip_prefix(&prefix)).map(str::trim)
}

impl FeatureCard {
    /// The panel of the construct's `index`th feature.
    pub fn of(c: &Construct, index: usize) -> Option<FeatureCard> {
        let f = c.features.get(index)?;
        let was = FeatureEdit {
            name: f.name.clone(),
            category: note(f, "category").unwrap_or_default().to_string(),
            forward: f.strand != Strand::Reverse,
            start: f.start + 1,
            end: f.end + 1,
            purpose: note(f, "use").unwrap_or_default().to_string(),
            description: note(f, "description").unwrap_or_default().to_string(),
        };
        let kind = note(f, "category").map_or_else(|| f.kind.clone(), str::to_owned);
        let type_word = if kind.is_empty() { "unspecified".to_string() } else { kind.replace('_', " ") };
        Some(FeatureCard {
            feature: index,
            anchor: None,
            start_text: was.start.to_string(),
            end_text: was.end.to_string(),
            edit: was.clone(),
            was,
            type_word,
            source: note(f, "use-source").map(str::to_owned),
            acts: ActsOn::of(c, index),
            picking: false,
            height: 0.0,
        })
    }

    /// The panel re-pointed at the record just written, keeping where it is and what is typed.
    pub fn refreshed(&self, fresh: FeatureCard) -> FeatureCard {
        FeatureCard {
            anchor: self.anchor,
            edit: self.edit.clone(),
            start_text: self.start_text.clone(),
            end_text: self.end_text.clone(),
            height: self.height,
            acts: self.acts.refreshed(fresh.acts.clone()),
            ..fresh
        }
    }

    /// Whether what the person has differs from the record.
    pub fn changed(&self) -> bool {
        self.edit != self.was
    }
}

/// Where the panel's top-left goes for an anchor, a size and a window (`FeatureCard.tsx` `place`):
/// above the point when there is room, below it when there is not, kept inside the window.
pub fn placement(anchor: Pos2, size: egui::Vec2, screen: Rect) -> Pos2 {
    let below = anchor.y - screen.top() - size.y - EDGE < 0.0;
    let half = size.x / 2.0;
    let x = anchor.x.max(screen.left() + half + EDGE).min(screen.right() - half - EDGE);
    if below {
        let y = (screen.top() + EDGE - CLEAR).max(anchor.y.min(screen.bottom() - EDGE - size.y - CLEAR));
        pos2(x - half, y + CLEAR)
    } else {
        pos2(x - half, anchor.y - size.y - CLEAR)
    }
}

/// `.panel-input`: a hairline box at `--radius-sm`, 2px of ink round it while it has the keys.
pub(crate) fn input_frame(painter: &egui::Painter, rect: Rect, focused: bool) {
    let r = CornerRadius::same(theme::RADIUS_SM as u8);
    if focused {
        painter.rect_stroke(rect, r, Stroke::new(2.0, theme::ACCENT), StrokeKind::Middle);
    } else {
        painter.rect_stroke(rect, r, Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
    }
}

fn hint(text: &str, font: &FontId) -> RichText {
    RichText::new(text).font(font.clone()).color(theme::MUTED)
}

/// `.panel-input` at 13px: 5px 7px of padding and a line of 1.45.
const INPUT_FONT: f32 = 13.0;
const INPUT_H: f32 = INPUT_FONT * 1.45 + 10.0 + 2.0;

/// A one-line `.panel-input` in `rect`. Gives the response of the text inside.
pub(crate) fn line_input(ui: &mut Ui, rect: Rect, text: &mut String, id: Id, placeholder: &str) -> egui::Response {
    let font = theme::sans(INPUT_FONT);
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(8, 6)))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(hint(placeholder, &font))
        .align(Align2::LEFT_CENTER);
    let response = ui.put(rect, edit);
    input_frame(ui.painter(), rect, response.has_focus());
    response
}

/// `.panel-area`: two rows tall to start with, growing with what is written.
fn area_input(ui: &mut Ui, text: &mut String, id: Id, placeholder: &str) -> egui::Response {
    let font = theme::sans(INPUT_FONT);
    let width = ui.available_width();
    // `line-height: 1.45`, which egui's own line is shorter than.
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
        let format =
            egui::TextFormat { font_id: theme::sans(INPUT_FONT), color: theme::INK, line_height: Some(INPUT_FONT * 1.45), ..Default::default() };
        let mut job = egui::text::LayoutJob::single_section(text.as_str().to_owned(), format);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let edit = egui::TextEdit::multiline(text)
        .layouter(&mut layouter)
        .id(id)
        .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(8, 6)))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(hint(placeholder, &font))
        .desired_rows(2)
        .min_size(vec2(0.0, 2.0 * INPUT_FONT * 1.45 + 12.0))
        .desired_width(width);
    let response = ui.add(edit);
    input_frame(ui.painter(), response.rect, response.has_focus());
    response
}

/// `.panel-label` / `.acts-label`: 11.5px at 600, muted.
fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::sans_weight(11.5, Weight::Semibold)).color(theme::MUTED));
}

/// Draws the panel and says what the person did. `fallback` is where it opens when it was not
/// opened by the pointer (the feature's place in the rows).
pub fn show(ctx: &egui::Context, card: &mut FeatureCard, fallback: Pos2) -> Outcome {
    let mut outcome = Outcome::default();
    let anchor = *card.anchor.get_or_insert(fallback);
    let screen = ctx.content_rect();
    let size = vec2(WIDTH, card.height.max(1.0));
    let corner = placement(anchor, size, screen);
    let base = Id::unique("feature-card");
    let frame = egui::Frame::new().fill(theme::CELL).corner_radius(theme::RADIUS).inner_margin(PAD).shadow(Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(0, 0, 0, 36),
    });
    let mut kind_rect = Rect::NOTHING;
    // `animation: feature-card-in 0.12s ease-out both`: opacity only.
    let appearing = crate::primitives::entrance(ctx, base, 0.12);
    let shown = egui::Area::new(base).order(egui::Order::Tooltip).fixed_pos(corner).fade_in(false).show(ctx, |ui| {
        ui.set_opacity(appearing);
        frame.show(ui, |ui| {
            let inner = WIDTH - f32::from(PAD.left) - f32::from(PAD.right);
            ui.set_width(inner);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            head(ui, card, base, inner, &mut outcome);
            ui.add_space(8.0);
            kind_rect = kind_row(ui, card, inner, &mut outcome);
            ui.add_space(7.0);
            span_row(ui, card, base, inner, &mut outcome);
            ui.add_space(11.0);
            if let Some(actions) = crate::acts_on::show(ui, &mut card.acts, base, true) {
                outcome.actions = Some(actions);
            }
            for (title, text, id, placeholder) in [
                ("Why it\u{2019}s here", &mut card.edit.purpose, base.with("purpose"), "so it can be seen on a western"),
                (
                    "What it does",
                    &mut card.edit.description,
                    base.with("description"),
                    "an epitope tag; eight residues, recognised by anti-FLAG antibodies",
                ),
            ] {
                ui.add_space(9.0);
                label(ui, title);
                ui.add_space(3.0);
                if area_input(ui, text, id, placeholder).lost_focus() {
                    outcome.commit = true;
                }
            }
            if let Some(source) = &card.source {
                ui.add_space(10.0);
                source_chip(ui, source);
            }
        });
    });
    let height = shown.response.rect.height();
    if (height - card.height).abs() > 0.5 {
        card.height = height;
        ctx.request_repaint();
    }
    if card.picking {
        pick_kind(ctx, card, kind_rect, &mut outcome);
    }
    // Escape closes it when the keys are in it or nowhere (the web listens on the panel itself);
    // what was being typed and not yet left is let go. A field elsewhere keeps its own Escape.
    let [verb, filter] = crate::acts_on::field_ids(base);
    let ours = [base.with("name"), base.with("purpose"), base.with("description"), base.with(("span", 0)), base.with(("span", 1)), verb, filter];
    let elsewhere = ctx.memory(|m| m.focused()).is_some_and(|id| !ours.contains(&id));
    if ctx.input(|i| i.key_pressed(Key::Escape)) && !elsewhere {
        if card.picking {
            card.picking = false;
        } else {
            return Outcome { commit: false, close: true, actions: outcome.actions };
        }
    }
    outcome
}

/// `.feature-card-head`: the name, typeable in place, and ✕.
fn head(ui: &mut Ui, card: &mut FeatureCard, base: Id, inner: f32, outcome: &mut Outcome) {
    let font = theme::sans_weight(15.5, Weight::Semibold);
    let height = 15.5 * 1.25 + 2.0;
    let (row, _) = ui.allocate_exact_size(vec2(inner, height), Sense::hover());
    let close = Rect::from_center_size(pos2(row.right() - 9.0, row.center().y), vec2(18.0, 18.0));
    let name_rect = Rect::from_min_max(pos2(row.left() - 3.0, row.top()), pos2(close.left() - 7.0, row.bottom()));
    let id = base.with("name");
    let hovered = ui.rect_contains_pointer(name_rect);
    let focused = ui.memory(|m| m.has_focus(id));
    if hovered && !focused {
        ui.painter().rect_filled(name_rect, CornerRadius::same(theme::RADIUS_SM as u8), theme::CELL_HOVER);
    }
    let edit = egui::TextEdit::singleline(&mut card.edit.name)
        .id(id)
        .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(3, 1)))
        .font(font)
        .text_color(theme::INK)
        .align(Align2::LEFT_CENTER);
    let name = ui.put(name_rect, edit);
    if name.has_focus() {
        crate::primitives::focus_ring(ui.painter(), name_rect, theme::RADIUS_SM, 1.0);
    }
    if name.lost_focus() {
        outcome.commit = true;
    }
    let x = ui.interact(close, base.with("close"), Sense::click()).on_hover_text("Close");
    if x.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    icon::paint(ui.painter(), close.center(), 14.0, lucide::X, if x.hovered() { theme::INK } else { theme::MUTED });
    if x.clicked() {
        outcome.close = true;
    }
}

/// `.panel-row`: what it is, and which way it reads. Gives the box of the kind's choice.
fn kind_row(ui: &mut Ui, card: &mut FeatureCard, inner: f32, outcome: &mut Outcome) -> Rect {
    let (row, _) = ui.allocate_exact_size(vec2(inner, INPUT_H), Sense::hover());
    // `.panel-seg`: two buttons of 4px 9px round a 13px arrow, inside a hairline.
    let seg_w = 2.0 * (9.0 + 14.0 + 9.0) + 2.0;
    let seg = Rect::from_min_max(pos2(row.right() - seg_w, row.top() + 1.0), pos2(row.right(), row.bottom() - 1.0));
    let kind = Rect::from_min_max(row.min, pos2(seg.left() - 8.0, row.bottom()));
    let response = ui.interact(kind, Id::unique("feature-card").with("kind"), Sense::click());
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if response.clicked() {
        card.picking = !card.picking;
    }
    let painter = ui.painter();
    input_frame(painter, kind, card.picking);
    let shown = CATEGORIES.iter().find(|(key, _)| *key == card.edit.category).map_or(card.type_word.as_str(), |(_, label)| *label);
    painter.with_clip_rect(kind.shrink(1.0)).text(
        pos2(kind.left() + 8.0, kind.center().y),
        Align2::LEFT_CENTER,
        shown,
        theme::sans(INPUT_FONT),
        theme::INK,
    );
    icon::paint(painter, pos2(kind.right() - 14.0, kind.center().y), 13.0, lucide::CHEVRON_DOWN, theme::INK);

    painter.rect_stroke(seg, CornerRadius::same(theme::RADIUS_SM as u8), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Inside);
    let half = (seg.width() - 2.0) / 2.0;
    for (i, (forward, arrow)) in [(true, lucide::ARROW_RIGHT), (false, lucide::ARROW_LEFT)].into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(seg.left() + 1.0 + half * i as f32, seg.top() + 1.0), vec2(half, seg.height() - 2.0));
        let press = ui.interact(r, Id::unique("feature-card").with(("way", i)), Sense::click());
        let on = card.edit.forward == forward;
        let painter = ui.painter();
        if on {
            let corners = if i == 0 { CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 } } else { CornerRadius { nw: 0, sw: 0, ne: 7, se: 7 } };
            painter.rect_filled(r, corners, theme::CELL_ACTIVE);
        }
        icon::paint(painter, r.center(), 14.0, arrow, if on { theme::INK } else { theme::MUTED });
        if press.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if press.clicked() {
            card.edit.forward = forward;
            outcome.commit = true;
        }
    }
    kind
}

/// The `<select>`'s list: "unspecified" (or the GenBank type) first, then the vocabulary.
fn pick_kind(ctx: &egui::Context, card: &mut FeatureCard, under: Rect, outcome: &mut Outcome) {
    let choices: Vec<(&str, String)> =
        std::iter::once(("", card.type_word.clone())).chain(CATEGORIES.iter().map(|(key, label)| (*key, (*label).to_string()))).collect();
    let item_h = 24.0;
    let shown = egui::Area::new(Id::unique("feature-card-kinds"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos2(under.left(), under.bottom() + 4.0))
        .fade_in(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::CELL)
                .stroke(Stroke::new(1.0, theme::HAIRLINE))
                .corner_radius(theme::RADIUS_SM)
                .inner_margin(4.0)
                .shadow(Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_rgba_unmultiplied(30, 35, 40, 20) })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (key, word) in &choices {
                        let (r, response) = ui.allocate_exact_size(vec2(under.width() - 8.0, item_h), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(6), theme::ROW_HOVER);
                        }
                        if *key == card.edit.category {
                            icon::paint(ui.painter(), pos2(r.left() + 10.0, r.center().y), 12.0, lucide::CHECK, theme::INK);
                        }
                        ui.painter().text(pos2(r.left() + 24.0, r.center().y), Align2::LEFT_CENTER, word, theme::sans(INPUT_FONT), theme::INK);
                        if response.clicked() {
                            card.edit.category = (*key).to_string();
                            card.picking = false;
                            outcome.commit = true;
                        }
                    }
                });
        });
    let away = ctx.input(|i| i.pointer.primary_pressed().then(|| i.pointer.interact_pos()).flatten());
    if away.is_some_and(|at| !shown.response.rect.contains(at) && !under.contains(at)) {
        card.picking = false;
    }
}

/// `.panel-span`: start and end, typeable, and the length.
fn span_row(ui: &mut Ui, card: &mut FeatureCard, base: Id, inner: f32, outcome: &mut Outcome) {
    let (row, _) = ui.allocate_exact_size(vec2(inner, INPUT_H), Sense::hover());
    let start = Rect::from_min_size(row.min, vec2(74.0, INPUT_H));
    let dots = theme::sans(INPUT_FONT);
    let dots_w = crate::primitives::text_width(ui, "..", dots.clone());
    let end = Rect::from_min_size(pos2(start.right() + 6.0 + dots_w + 6.0, row.top()), vec2(74.0, INPUT_H));
    let baseline_y = row.center().y;
    ui.painter().text(pos2(start.right() + 6.0, baseline_y), Align2::LEFT_CENTER, "..", dots, theme::MUTED);
    for (rect, which) in [(start, 0), (end, 1)] {
        let text = if which == 0 { &mut card.start_text } else { &mut card.end_text };
        text.retain(|c| c.is_ascii_digit());
        let response = line_input(ui, rect, text, base.with(("span", which)), "");
        if response.lost_focus() {
            let parsed = text.parse::<usize>().ok().filter(|n| *n >= 1);
            let slot = if which == 0 { &mut card.edit.start } else { &mut card.edit.end };
            match parsed {
                Some(n) => *slot = n,
                None => *text = slot.to_string(),
            }
            outcome.commit = true;
        }
    }
    let live = |text: &str, was: usize| text.parse::<usize>().unwrap_or(was);
    let length = (live(&card.end_text, card.edit.end) + 1).saturating_sub(live(&card.start_text, card.edit.start));
    ui.painter().text(pos2(row.right(), baseline_y), Align2::RIGHT_CENTER, format!("{} bp", thousands(length)), theme::sans(12.0), theme::MUTED);
}

/// `.feature-card-source`: who said it. A claim someone made and one that was guessed never weigh
/// the same.
fn source_chip(ui: &mut Ui, source: &str) {
    let human = source == "human";
    let text = if human { "you said this" } else { "suggested" };
    let font = theme::sans_weight(10.5, Weight::Book);
    let width = crate::primitives::text_width(ui, text, font.clone()) + 12.0;
    let (r, _) = ui.allocate_exact_size(vec2(width, 10.5 * 1.25 + 4.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(255), Color32::from_rgba_unmultiplied(0, 0, 0, 10));
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, font, if human { theme::INK } else { theme::MUTED });
}

pub(crate) fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use bios_core::genbank;
    use egui::{Rect, pos2, vec2};

    use super::{FeatureCard, placement};

    const FILE: &str = "LOCUS       t                         40 bp    DNA     circular SYN 01-JAN-2026\n\
FEATURES             Location/Qualifiers\n     CDS             3..12\n                     /label=\"T7 pol\"\n                     \
/note=\"bios-id: a\"\n                     /note=\"bios-act: transcribes > b, construct\"\n                     /note=\"bios-use: drives it\"\n                     \
/note=\"bios-use-source: human\"\n     promoter        complement(20..30)\n                     /label=\"pT7\"\n                     /note=\"bios-id: b\"\n\
ORIGIN\n        1 acgtggatcc gatcaaaaac ccccgggggt tttttacgta\n//\n";

    #[test]
    fn the_panel_reads_the_record_and_both_ends_of_what_acts_on_what() {
        let c = genbank::read(FILE).unwrap();
        let pol = FeatureCard::of(&c, 0).unwrap();
        assert_eq!((pol.was.start, pol.was.end, pol.was.forward), (3, 12, true));
        assert_eq!(pol.was.purpose, "drives it");
        assert_eq!(pol.type_word, "CDS", "nothing said about what it is: the empty choice names the GenBank type");
        assert_eq!(pol.source.as_deref(), Some("human"));
        assert_eq!(pol.acts.statements.len(), 1);
        assert_eq!(pol.acts.statements[0].names, ["pT7", "this construct"]);
        let promoter = FeatureCard::of(&c, 1).unwrap();
        assert!(!promoter.was.forward);
        assert_eq!(promoter.acts.acted_on.iter().map(|(n, v, _)| (n.as_str(), v.as_str())).collect::<Vec<_>>(), [("T7 pol", "transcribes")]);
        assert!(FeatureCard::of(&c, 2).is_none());
    }

    #[test]
    fn it_sits_above_the_point_and_flips_below_when_there_is_no_room() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0));
        let size = vec2(340.0, 321.0);
        // The web, measured: GFP clicked at (1301, 684) put the card's top-left at (1088, 353).
        let above = placement(pos2(1301.0, 684.5), size, screen);
        assert_eq!((above.x, above.y.round()), (1088.0, 354.0));
        let below = placement(pos2(400.0, 100.0), size, screen);
        assert_eq!((below.x, below.y), (230.0, 110.0), "under the point, by its own offset");
    }
}
