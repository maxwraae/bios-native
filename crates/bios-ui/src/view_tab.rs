//! "What's shown on the map": the layer switches, the base colour and the label density, in the
//! popover the sliders button opens. Ported from Bios `leftpanel/ViewTab.tsx`, `editor/layers.ts`
//! and `components/ToggleRow.tsx`, their look from `styles.css` (`.view-tab`, `.view-section-title`,
//! `.base-style-seg`, `.toggle-row`, lines 2992-3096; `.oge-pop-panel`, 6790-6802).
//!
//! The native map honours Features, Enzyme names and Label density, and the rows Features, Enzyme
//! names, Binders, ORFs, Amino acids, Reverse strand, Position ruler, Interactions, Parts and the
//! base colour. Every other row is shown as the web shows it, greyed (`.toggle-row.is-disabled`),
//! because the native map and rows cannot draw it yet; its tooltip says so.

use bios_layout::overview::labels::{LABEL_DENSITY_DEFAULT, LabelDensity};
use egui::{Align2, Area, CornerRadius, CursorIcon, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::primitives::shadow;
use crate::theme::{self, Weight};

/// How the sequence's bases are coloured: a global preference, not a layer. Bios
/// `editor/layers.ts` (`BaseStyle`, `BASE_STYLE_ROWS`), in the order the popover lists them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseStyle {
    /// One quiet grey, so colour lives only where it means something. The default
    /// (`BASE_STYLE_DEFAULT`).
    #[default]
    Calm,
    Muted,
    Vivid,
    /// G and C in one colour, A and T in another.
    GcAt,
    /// The plain ink (`"black"`, labelled None).
    Black,
}

const BASE_STYLES: [BaseStyle; 5] = [BaseStyle::Calm, BaseStyle::Muted, BaseStyle::Vivid, BaseStyle::GcAt, BaseStyle::Black];

/// The layers the native map and rows honour, as the web's workspace holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    /// "Features" (`layerVisibility.features`): the construct's own annotations, primer sites aside.
    pub features: bool,
    /// "ORFs" (`layerVisibility.orfs`): open reading frames derived from the bases. Off at rest.
    pub orfs: bool,
    /// "Amino acids" (`layerVisibility.translations`): every coding feature's protein. Off at rest.
    pub amino_acids: bool,
    /// "Splice-map hits" (`showSpliceScan`): the `..._SM` motif hits a splice scan wrote, hidden
    /// at rest because they would bury the map.
    pub splice_scan: bool,
    /// "Enzyme names" (`layerVisibility.cutsites`).
    pub cut_names: bool,
    pub density: LabelDensity,
    pub base_style: BaseStyle,
    /// "Reverse strand" (`layerVisibility.reverseSequence`): both strands, welded. Rows only.
    pub reverse_strand: bool,
    /// "Position ruler" (`layerVisibility.axis`): the distance rules through the duplex. Rows only.
    pub ruler: bool,
    /// "Interactions" (`layerVisibility.interactions`): what acts on what, across the rows and
    /// through the ring. Off until asked for; stating a relationship turns it on.
    pub interactions: bool,
    /// "Parts" (`layerVisibility.parts`): the lane of parts in the rows. On by default.
    pub parts: bool,
    /// "Binders" (`layerVisibility.primers`): the whole oligo lane in the rows: made primers, held
    /// binders and Find's alignments, and with them the primer ticks on the navigator rail. The
    /// web's map draws no oligos, and neither does this one.
    pub binders: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers {
            features: true,
            orfs: false,
            amino_acids: false,
            splice_scan: false,
            cut_names: true,
            density: LABEL_DENSITY_DEFAULT,
            base_style: BaseStyle::default(),
            reverse_strand: true,
            ruler: true,
            interactions: false,
            parts: true,
            binders: true,
        }
    }
}

impl Layers {
    /// The switch a popover row stands for, when native honours it.
    fn switch(&self, label: &str) -> Option<bool> {
        match label {
            "Features" => Some(self.features),
            "ORFs" => Some(self.orfs),
            "Amino acids" => Some(self.amino_acids),
            "Enzyme names" => Some(self.cut_names),
            "Reverse strand" => Some(self.reverse_strand),
            "Position ruler" => Some(self.ruler),
            "Interactions" => Some(self.interactions),
            "Parts" => Some(self.parts),
            "Binders" => Some(self.binders),
            _ => None,
        }
    }

    /// These layers with the popover row called `label` switched the other way. A row native
    /// does not draw is left as it is.
    pub fn flipped(self, label: &str) -> Layers {
        match label {
            "Features" => Layers { features: !self.features, ..self },
            "ORFs" => Layers { orfs: !self.orfs, ..self },
            "Amino acids" => Layers { amino_acids: !self.amino_acids, ..self },
            "Splice-map hits" => Layers { splice_scan: !self.splice_scan, ..self },
            "Enzyme names" => Layers { cut_names: !self.cut_names, ..self },
            "Reverse strand" => Layers { reverse_strand: !self.reverse_strand, ..self },
            "Position ruler" => Layers { ruler: !self.ruler, ..self },
            "Interactions" => Layers { interactions: !self.interactions, ..self },
            "Parts" => Layers { parts: !self.parts, ..self },
            "Binders" => Layers { binders: !self.binders, ..self },
            _ => self,
        }
    }
}

/// `LAYER_ROWS`, with the web's defaults (`LAYER_DEFAULTS`) for the rows native cannot draw.
const LAYER_ROWS: [(&str, bool); 9] = [
    ("Features", true),
    ("Enzyme names", true),
    ("Binders", true),
    ("Parts", true),
    ("ORFs", false),
    ("Amino acids", false),
    ("Reverse strand", true),
    ("Position ruler", true),
    ("Interactions", false),
];
const BASE_STYLE_ROWS: [&str; 5] = ["Calm", "Muted", "Vivid", "G·C / A·T", "None"];
const LABEL_DENSITY_ROWS: [(LabelDensity, &str); 3] =
    [(LabelDensity::Roomy, "Roomy"), (LabelDensity::Balanced, "Balanced"), (LabelDensity::Dense, "Dense")];

/// Why a row is grey.
const NOT_YET: &str = "Not in this version of Bios yet";

const WIDTH: f32 = 260.0;
const TITLE_H: f32 = 8.0 + 13.0 + 4.0;
const ROW_H: f32 = 8.0 + 16.0 + 8.0;
const PILL_H: f32 = 29.0;
const GAP: f32 = 6.0;

fn seg_h(items: usize) -> f32 {
    let rows = items.div_ceil(2) as f32;
    2.0 + rows * PILL_H + (rows - 1.0).max(0.0) * GAP + 8.0
}

/// What the popover did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Changed(Layers),
    /// A press landed outside the popover and its button.
    Close,
}

/// Draws the popover opening upward from `trigger`, its right edge on the trigger's
/// (`.oge-pop-bottom-right`: `bottom: calc(100% + 6px); right: 0`).
pub fn show(ui: &mut Ui, trigger: Rect, layers: Layers) -> Option<Outcome> {
    show_with(ui, trigger, layers, 0)
}

/// The popover for a construct carrying `splice_hits` splice-map hits: with any, an Annotations
/// section offers to show them, counted (`ViewTab.tsx`, `countSpliceScan`).
pub fn show_with(ui: &mut Ui, trigger: Rect, layers: Layers, splice_hits: usize) -> Option<Outcome> {
    let annotations = if splice_hits > 0 { TITLE_H + ROW_H } else { 0.0 };
    let height = 6.0
        + TITLE_H
        + LAYER_ROWS.len() as f32 * ROW_H
        + TITLE_H
        + seg_h(BASE_STYLE_ROWS.len())
        + TITLE_H
        + seg_h(LABEL_DENSITY_ROWS.len())
        + annotations
        + 6.0;
    let rect = Rect::from_min_size(pos2(trigger.right() - WIDTH, trigger.top() - 6.0 - height), vec2(WIDTH, height));
    let mut outcome = None;
    Area::new(Id::unique("view-tab")).order(Order::Foreground).fixed_pos(rect.min).fade_in(false).show(ui.ctx(), |ui| {
        ui.set_min_size(rect.size());
        let painter = ui.painter().clone();
        shadow(&painter, rect, theme::RADIUS, theme::SHADOW_CELL);
        painter.rect_filled(rect, CornerRadius::same(theme::RADIUS as u8), theme::CELL);
        painter.rect_stroke(rect, CornerRadius::same(theme::RADIUS as u8), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Outside);
        ui.allocate_rect(rect, Sense::click());
        let mut y = rect.top() + 6.0;
        let title = |y: f32, text: &str| {
            let galley = painter.layout_no_wrap(text.to_uppercase(), theme::sans_weight(11.0, Weight::Semibold), theme::MUTED);
            painter.galley(pos2(rect.left() + 14.0, y + 8.0), galley, theme::MUTED);
        };
        title(y, "Show on map");
        y += TITLE_H;
        for (label, default) in LAYER_ROWS {
            let row = Rect::from_min_size(pos2(rect.left(), y), vec2(WIDTH, ROW_H));
            let state = layers.switch(label);
            let honoured = state.is_some();
            let checked = state.unwrap_or(default);
            let response = ui.allocate_rect(row, if honoured { Sense::click() } else { Sense::hover() });
            let response = if honoured { response } else { response.on_hover_text(NOT_YET) };
            if honoured && response.hovered() {
                painter.rect_filled(row, 0.0, theme::CELL_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let ink = if honoured { theme::INK } else { theme::MUTED };
            painter.text(pos2(row.left() + 14.0, row.center().y), Align2::LEFT_CENTER, label, theme::sans(13.0), ink);
            let switch = Rect::from_min_size(pos2(row.right() - 14.0 - 34.0, row.center().y - 9.0), vec2(34.0, 18.0));
            let fade = if honoured { 1.0 } else { 0.5 };
            let track = if checked { theme::ACCENT } else { theme::HAIRLINE_STRONG };
            painter.rect_filled(switch, CornerRadius::same(9), theme::alpha(track, fade));
            let thumb = pos2(switch.left() + if checked { 18.0 } else { 2.0 } + 7.0, switch.center().y);
            shadow(&painter, Rect::from_center_size(thumb, vec2(14.0, 14.0)), 7.0, theme::SHADOW_1);
            painter.circle_filled(thumb, 7.0, theme::alpha(theme::FLOAT, fade));
            if honoured && response.clicked() {
                outcome = Some(Outcome::Changed(layers.flipped(label)));
            }
            y += ROW_H;
        }
        let mut pills = |y: f32, labels: &[&str], on: Option<usize>, enabled: bool| -> Option<usize> {
            let w = (WIDTH - 28.0 - GAP) / 2.0;
            let mut clicked = None;
            for (i, label) in labels.iter().enumerate() {
                let (col, line) = ((i % 2) as f32, (i / 2) as f32);
                let r = Rect::from_min_size(pos2(rect.left() + 14.0 + col * (w + GAP), y + 2.0 + line * (PILL_H + GAP)), vec2(w, PILL_H));
                let response = ui.allocate_rect(r, if enabled { Sense::click() } else { Sense::hover() });
                let response = if enabled { response } else { response.on_hover_text(NOT_YET) };
                let is_on = on == Some(i);
                let hovered = enabled && response.hovered();
                let ground = if is_on {
                    theme::FLOAT
                } else if hovered {
                    theme::CELL_ACTIVE
                } else {
                    theme::CELL_HOVER
                };
                painter.rect_filled(r, CornerRadius::same(8), ground);
                if is_on {
                    painter.rect_stroke(r, CornerRadius::same(8), Stroke::new(1.0, theme::HAIRLINE_STRONG), StrokeKind::Inside);
                }
                let ink = if is_on || hovered { theme::INK } else { theme::MUTED };
                let weight = if is_on { Weight::Semibold } else { Weight::Medium };
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    *label,
                    theme::sans_weight(12.5, weight),
                    if enabled { ink } else { theme::alpha(ink, 0.6) },
                );
                if response.clicked() {
                    clicked = Some(i);
                }
            }
            clicked
        };
        title(y, "Base colour");
        y += TITLE_H;
        let on = BASE_STYLES.iter().position(|s| *s == layers.base_style);
        if let Some(i) = pills(y, &BASE_STYLE_ROWS, on, true)
            && let Some(style) = BASE_STYLES.get(i)
        {
            outcome = Some(Outcome::Changed(Layers { base_style: *style, ..layers }));
        }
        y += seg_h(BASE_STYLE_ROWS.len());
        title(y, "Label density");
        y += TITLE_H;
        let names: Vec<&str> = LABEL_DENSITY_ROWS.iter().map(|(_, l)| *l).collect();
        let on = LABEL_DENSITY_ROWS.iter().position(|(d, _)| *d == layers.density);
        if let Some(i) = pills(y, &names, on, true)
            && let Some((density, _)) = LABEL_DENSITY_ROWS.get(i)
        {
            outcome = Some(Outcome::Changed(Layers { density: *density, ..layers }));
        }
        y += seg_h(LABEL_DENSITY_ROWS.len());
        if splice_hits > 0 {
            title(y, "Annotations");
            y += TITLE_H;
            let row = Rect::from_min_size(pos2(rect.left(), y), vec2(WIDTH, ROW_H));
            let response = ui.allocate_rect(row, Sense::click()).on_hover_text("Algorithmic splice-motif scan results, off by default");
            if response.hovered() {
                painter.rect_filled(row, 0.0, theme::CELL_HOVER);
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            let label = format!("Splice-map hits ({splice_hits})");
            painter.text(pos2(row.left() + 14.0, row.center().y), Align2::LEFT_CENTER, label, theme::sans(13.0), theme::INK);
            let checked = layers.splice_scan;
            let switch = Rect::from_min_size(pos2(row.right() - 14.0 - 34.0, row.center().y - 9.0), vec2(34.0, 18.0));
            painter.rect_filled(switch, CornerRadius::same(9), if checked { theme::ACCENT } else { theme::HAIRLINE_STRONG });
            let thumb = pos2(switch.left() + if checked { 18.0 } else { 2.0 } + 7.0, switch.center().y);
            shadow(&painter, Rect::from_center_size(thumb, vec2(14.0, 14.0)), 7.0, theme::SHADOW_1);
            painter.circle_filled(thumb, 7.0, theme::FLOAT);
            if response.clicked() {
                outcome = Some(Outcome::Changed(Layers { splice_scan: !checked, ..layers }));
            }
        }
    });
    if outcome.is_some() {
        return outcome;
    }
    // The popover closes on a press anywhere else.
    let pressed_outside =
        ui.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !trigger.contains(p)));
    pressed_outside.then_some(Outcome::Close)
}
