//! The small sheet that turns a selection into a feature or a primer: a name, which way it
//! reads, and for a feature why it is here, what it serves and what it is (Bios
//! `editor/AskSheet.tsx`). Only the name has to be filled in; nothing hides behind a click, so the
//! chips are on screen at once. Enter lands it from the name field and Escape lets go.

use egui::{Align, Align2, Key, Layout, RichText, Stroke, vec2};

use crate::theme;

pub const WIDTH: f32 = 460.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    Feature,
    Primer,
}

/// What a feature made by hand can be: the word shown, the GenBank type that carries it to any
/// other editor, and the key Bios keeps on the feature itself. The web app's list, less the
/// primer, which has its own verb.
pub const KINDS: [(&str, &str, &str); 9] = [
    ("Promoter", "promoter", "promoter"),
    ("CDS", "CDS", "cds"),
    ("Terminator", "terminator", "terminator"),
    ("Origin", "rep_origin", "origin"),
    ("Tag", "misc_feature", "tag"),
    ("Marker", "CDS", "marker"),
    ("Linker", "misc_feature", "linker"),
    ("Insulator", "regulatory", "insulator"),
    ("Misc", "misc_feature", "misc"),
];

/// Which of [`KINDS`] a feature is: what Bios was told it is, or else the best reading of its
/// GenBank type, for a feature nobody has said anything about.
pub fn kind_of(category: Option<&str>, genbank_type: &str) -> Option<usize> {
    let key = category.map(str::to_lowercase).unwrap_or_else(|| {
        match genbank_type.to_lowercase().as_str() {
            "cds" | "gene" | "mrna" => "cds",
            "promoter" => "promoter",
            "terminator" => "terminator",
            "rep_origin" | "origin" => "origin",
            "regulatory" | "enhancer" => "insulator",
            _ => "misc",
        }
        .to_string()
    });
    KINDS.iter().position(|(_, _, own)| *own == key)
}

/// A pill that is filled when it is the one picked.
pub fn chip(ui: &mut egui::Ui, label: &str, on: bool) -> bool {
    let text = RichText::new(label).font(theme::sans(12.0)).color(if on { theme::CANVAS } else { theme::INK });
    let button =
        egui::Button::new(text).fill(if on { theme::INK } else { theme::CELL }).stroke(Stroke::new(1.0, theme::HAIRLINE_STRONG)).corner_radius(10.0);
    ui.add(button).clicked()
}

/// A feature on the map a new one could serve (`AskTarget`): one with a bios id, so it can be
/// pointed at whatever it is renamed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Its bios id.
    pub id: String,
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// What a new feature serves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Serves {
    Construct,
    /// A feature, by its bios id.
    Feature(String),
}

/// How many of the nearest targets are on screen before "+N more" (`NEAR`).
const NEAR: usize = 7;

/// The sheet while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub what: What,
    pub name: String,
    pub forward: bool,
    /// Which of [`KINDS`] is picked. `None` is unsaid, which lands as `misc_feature`.
    pub kind: Option<usize>,
    /// Why it is here, in a sentence. Empty is unsaid.
    pub why: String,
    /// What it serves. `None` is unsaid.
    pub serves: Option<Serves>,
    /// What it could serve, the nearest to the selection first.
    pub targets: Vec<Target>,
    /// "+N more" was pressed: every target is on screen.
    all_targets: bool,
    /// The name field takes the keyboard on the first frame.
    focused: bool,
}

impl Ask {
    pub fn new(what: What, forward: bool) -> Ask {
        Ask {
            what,
            name: String::new(),
            forward,
            kind: None,
            why: String::new(),
            serves: None,
            targets: Vec::new(),
            all_targets: false,
            focused: false,
        }
    }

    /// The same sheet offering `targets` to serve, the nearest to `span` (first and last base)
    /// first, so the likely answer is under the eye.
    pub fn serving(mut self, mut targets: Vec<Target>, span: (usize, usize)) -> Ask {
        let mid = |a: usize, b: usize| (a as f64 + b as f64) / 2.0;
        let here = mid(span.0, span.1);
        targets.sort_by(|a, b| (mid(a.start, a.end) - here).abs().total_cmp(&(mid(b.start, b.end) - here).abs()));
        self.targets = targets;
        self
    }

    /// The targets on screen: the nearest seven, or all of them once asked or when only one more
    /// would be hidden.
    pub fn shown_targets(&self) -> &[Target] {
        if self.all_targets || self.targets.len() <= NEAR + 1 { &self.targets } else { self.targets.get(..NEAR).unwrap_or(&self.targets) }
    }

    /// The why as it will be saved: one line, no space at its ends.
    pub fn why(&self) -> String {
        self.why.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The category key Bios keeps on the feature, when one was picked.
    pub fn category(&self) -> Option<&'static str> {
        self.kind.and_then(|i| KINDS.get(i)).map(|(_, _, own)| *own)
    }

    /// The name as it will be saved.
    pub fn name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() { "unnamed".to_string() } else { name.to_string() }
    }

    /// The GenBank type the new annotation gets.
    pub fn genbank_type(&self) -> &'static str {
        match self.what {
            What::Primer => "primer_bind",
            What::Feature => self.kind.and_then(|i| KINDS.get(i)).map_or("misc_feature", |(_, kind, _)| kind),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
}

/// Draws the sheet with its top right corner at `corner` and says whether it was settled.
pub fn show(ui: &mut egui::Ui, corner: egui::Pos2, ask: &mut Ask) -> Option<Outcome> {
    let mut outcome = None;
    // In a layer of its own above the sequence, so that a press on the sheet is the sheet's and
    // never a caret in the bases under it.
    let area = egui::Area::new(egui::Id::unique("ask")).order(egui::Order::Foreground).pivot(Align2::RIGHT_TOP).fixed_pos(corner).fade_in(false);
    area.show(ui.ctx(), |ui| {
        egui::Frame::new().fill(theme::CANVAS).stroke(Stroke::new(1.0, theme::HAIRLINE_STRONG)).corner_radius(10.0).inner_margin(12.0).show(
            ui,
            |ui| {
                ui.set_width(WIDTH - 24.0);
                ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                let hint = match ask.what {
                    What::Feature => "Name",
                    What::Primer => "Name this primer",
                };
                let field = ui.add(egui::TextEdit::singleline(&mut ask.name).hint_text(hint).desired_width(f32::INFINITY).font(theme::sans(14.0)));
                if !ask.focused {
                    field.request_focus();
                    ask.focused = true;
                }
                // Enter in a one-line field gives up the focus; that is the person saying "this one".
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    outcome = Some(Outcome::Done);
                }
                if ask.what == What::Feature {
                    ui.add(
                        egui::TextEdit::multiline(&mut ask.why)
                            .hint_text("Why it's here — e.g. so GFP can be seen on a western")
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .font(theme::sans(13.0)),
                    );
                }
                ui.horizontal(|ui| {
                    if chip(ui, "top strand →", ask.forward) {
                        ask.forward = true;
                    }
                    if chip(ui, "← bottom strand", !ask.forward) {
                        ask.forward = false;
                    }
                });
                if ask.what == What::Feature {
                    ui.label(RichText::new("What it serves").font(theme::sans(11.0)).color(theme::MUTED));
                    ui.horizontal_wrapped(|ui| {
                        if chip(ui, "the construct", ask.serves == Some(Serves::Construct)) {
                            ask.serves = if ask.serves == Some(Serves::Construct) { None } else { Some(Serves::Construct) };
                        }
                        let mut picked = None;
                        for t in ask.shown_targets() {
                            let on = ask.serves == Some(Serves::Feature(t.id.clone()));
                            if chip(ui, &t.name, on) {
                                picked = Some(if on { None } else { Some(Serves::Feature(t.id.clone())) });
                            }
                        }
                        if let Some(serves) = picked {
                            ask.serves = serves;
                        }
                        let hidden = ask.targets.len() - ask.shown_targets().len();
                        if hidden > 0 && chip(ui, &format!("+{hidden} more"), false) {
                            ask.all_targets = true;
                        }
                    });
                    ui.label(RichText::new("What it is").font(theme::sans(11.0)).color(theme::MUTED));
                    ui.horizontal_wrapped(|ui| {
                        for (i, (label, _, _)) in KINDS.iter().enumerate() {
                            if chip(ui, label, ask.kind == Some(i)) {
                                ask.kind = if ask.kind == Some(i) { None } else { Some(i) };
                            }
                        }
                    });
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let go = match ask.what {
                        What::Feature => "Make feature",
                        What::Primer => "Add primer",
                    };
                    if chip(ui, go, true) {
                        outcome = Some(Outcome::Done);
                    }
                    if chip(ui, "Cancel", false) {
                        outcome = Some(Outcome::Cancelled);
                    }
                });
            },
        );
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        outcome = Some(Outcome::Cancelled);
    }
    outcome
}

/// One line of text asked for by name: a new name for a plasmid, a folder to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// What is being asked for, shown above the field: `Rename pUC19`.
    pub label: String,
    pub text: String,
    /// The word on the button that lands it: `Rename`.
    pub go: String,
    focused: bool,
}

impl Prompt {
    pub fn new(label: impl Into<String>, text: impl Into<String>, go: impl Into<String>) -> Prompt {
        Prompt { label: label.into(), text: text.into(), go: go.into(), focused: false }
    }

    /// What was typed, without space at its ends.
    pub fn value(&self) -> String {
        self.text.trim().to_string()
    }
}

/// Draws the prompt with its top left corner at `corner` and says whether it was settled.
/// Enter lands it, Escape lets go, and nothing lands while the field is empty.
pub fn prompt(ui: &mut egui::Ui, corner: egui::Pos2, prompt: &mut Prompt) -> Option<Outcome> {
    let mut outcome = None;
    let area = egui::Area::new(egui::Id::unique("prompt")).order(egui::Order::Foreground).pivot(Align2::LEFT_TOP).fixed_pos(corner).fade_in(false);
    area.show(ui.ctx(), |ui| {
        egui::Frame::new().fill(theme::CANVAS).stroke(Stroke::new(1.0, theme::HAIRLINE_STRONG)).corner_radius(10.0).inner_margin(12.0).show(
            ui,
            |ui| {
                ui.set_width(WIDTH - 120.0);
                ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                ui.label(RichText::new(&prompt.label).font(theme::sans(11.0)).color(theme::MUTED));
                let field = ui.add(egui::TextEdit::singleline(&mut prompt.text).desired_width(f32::INFINITY).font(theme::sans(14.0)));
                if !prompt.focused {
                    field.request_focus();
                    prompt.focused = true;
                }
                let filled = !prompt.value().is_empty();
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && filled {
                    outcome = Some(Outcome::Done);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if filled && chip(ui, &prompt.go, true) {
                        outcome = Some(Outcome::Done);
                    }
                    if chip(ui, "Cancel", false) {
                        outcome = Some(Outcome::Cancelled);
                    }
                });
            },
        );
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        outcome = Some(Outcome::Cancelled);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::{Ask, What};

    #[test]
    fn what_it_could_serve_comes_nearest_first_and_the_rest_wait_behind_more() {
        let target = |i: usize| super::Target { id: format!("id{i}"), name: format!("f{i}"), start: i * 100, end: i * 100 + 10 };
        let ask = Ask::new(What::Feature, true).serving((0..10).map(target).collect(), (505, 505));
        let names: Vec<&str> = ask.shown_targets().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["f5", "f4", "f6", "f3", "f7", "f2", "f8"]);
        // Eight fit without a "+1 more" that would only hide one.
        let eight = Ask::new(What::Feature, true).serving((0..8).map(target).collect(), (0, 0));
        assert_eq!(eight.shown_targets().len(), 8);
        let mut said = Ask::new(What::Feature, true);
        said.why = "  so GFP can be\n seen ".into();
        said.kind = Some(4);
        assert_eq!((said.why(), said.category()), ("so GFP can be seen".to_string(), Some("tag")));
    }

    #[test]
    fn only_the_name_has_to_be_filled_in_and_even_that_has_a_fallback() {
        let mut ask = Ask::new(What::Feature, true);
        assert_eq!((ask.name(), ask.genbank_type()), ("unnamed".to_string(), "misc_feature"));
        ask.name = "  GFP ".into();
        ask.kind = Some(1);
        assert_eq!((ask.name(), ask.genbank_type()), ("GFP".to_string(), "CDS"));
        ask.kind = Some(3);
        assert_eq!(ask.genbank_type(), "rep_origin");
        assert_eq!(Ask::new(What::Primer, false).genbank_type(), "primer_bind");
    }
}
