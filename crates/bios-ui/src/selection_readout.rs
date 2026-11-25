//! What is selected, in numbers: one glass pill floating 8px above the selection's highlight,
//! with the two verbs, + Primer and + Feature.
//!
//! Ported from the web app's `editor/SelectionReadout.tsx` (its numbers 52-103, its placement
//! 163-213, the pill 337-371, `PrimerInline` 378-460 and `Verb` 464-477) and the rules in
//! `styles.css` 5837-5974 (`.readout-pill`, `.selection-readout` and its `.sr-*` parts).
//!
//! The pill says three numbers and offers two verbs. + Primer is answered on the pill: a
//! direction pre-guessed from the drag, a name, Enter, and the verdict beside it. + Feature turns
//! the pill into the sheet that names a new feature (`ask.rs`), at the same anchor.

use bios_core::Construct;
use bios_core::primer_quality::primer_flags;
use bios_core::seq::{DNA_TM_DEFAULTS, revcomp, tm};
use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Key, Margin, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::icon;
use crate::lucide;
use crate::primitives::{self, text_width};
use crate::theme::{self, Weight};

/// The band where an occurrence count is worth giving: long enough to be a specific probe, short
/// enough that "does this stretch repeat?" is a real question.
const SPEC: std::ops::RangeInclusive<usize> = 6..=60;
/// The lengths a Tm is given for, and the most it can say and still be a temperature.
const TM_LENGTHS: std::ops::RangeInclusive<usize> = 8..=120;
const TM_CEILING: f64 = 95.0;
/// Up to this a selection is plausibly a primer, so its design liabilities are worth flagging.
const OLIGO: std::ops::RangeInclusive<usize> = 12..=45;
/// `.readout-pill`: 34 high, 16 either side, a gap of 7 between its parts.
const HEIGHT: f32 = 34.0;
const PAD: f32 = 16.0;
const GAP: f32 = 7.0;
/// The bottom of the pane carries the toolbar, so a pill is kept above this band.
const TOOLBAR_BAND: f32 = 120.0;
/// `.sr-verdict.is-warn`, and its dot.
const WARN: Color32 = theme::hex(0xb7791f);
const WARN_DOT: Color32 = theme::hex(0xd79a2b);

/// The selected bases, 5' to 3' on the top strand. `end < start` runs through the origin.
pub fn selected_bases(c: &Construct, start: usize, end: usize) -> String {
    let s = &c.sequence;
    if start <= end {
        s.get(start..=end).unwrap_or_default().to_string()
    } else {
        [s.get(start..).unwrap_or_default(), s.get(..=end).unwrap_or_default()].concat()
    }
}

/// JavaScript's `Math.round`: halves go up, also below zero.
fn js_round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

pub fn thousands(n: usize) -> String {
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

/// The pill's three numbers, worded: `11 bp`, `Tm 31°C` (only where it means something) and
/// `55% GC`.
pub fn numbers(bases: &str) -> Vec<String> {
    let len = bases.len();
    let mut out = vec![format!("{} bp", thousands(len))];
    if TM_LENGTHS.contains(&len)
        && let Some(t) = tm(bases, DNA_TM_DEFAULTS.concentration_nm, DNA_TM_DEFAULTS.sodium_mm).filter(|t| *t <= TM_CEILING)
    {
        out.push(format!("Tm {}°C", js_round(t)));
    }
    let gc = bases.bytes().filter(|b| matches!(b.to_ascii_uppercase(), b'G' | b'C')).count();
    let fraction = if len == 0 { 0.0 } else { gc as f64 / len as f64 };
    out.push(format!("{}% GC", js_round(fraction * 100.0)));
    out
}

/// Overlapping count of `needle` in `hay`: a primer can anneal at overlapping registers.
fn count(hay: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    (0..hay.len()).filter(|&i| hay.get(i..).is_some_and(|rest| rest.starts_with(needle))).count()
}

/// How many places this exact stretch could anneal in the construct: the top strand and its
/// reverse complement, through the origin of a circular one. The selection itself is one.
pub fn anneal_sites(seq: &str, sub: &str, circular: bool) -> usize {
    if seq.is_empty() || sub.is_empty() || sub.len() > seq.len() {
        return 0;
    }
    let f = sub.to_uppercase();
    let wrap = if circular { seq.get(..f.len() - 1).unwrap_or_default() } else { "" };
    let hay = format!("{seq}{wrap}").to_uppercase();
    let mut n = count(&hay, &f);
    let r = revcomp(&f);
    // A palindrome would count its own site twice.
    if r != f {
        n += count(&hay, &r);
    }
    n
}

/// What the primer ask says beside its name: how many places it anneals and whether the design
/// is clean. The words, whether to warn, and the flags for the tooltip.
pub fn verdict(sub: &str, sites: Option<usize>) -> Option<(String, bool, Vec<String>)> {
    let flags = OLIGO.contains(&sub.len()).then(|| primer_flags(sub));
    let clean = flags.as_ref().is_none_or(|f| f.warnings.is_empty());
    let mut words = Vec::new();
    if let Some(sites) = sites {
        words.push(if sites <= 1 { "unique".to_string() } else { format!("{sites} sites") });
    }
    if let Some(f) = &flags {
        let n = f.warnings.len();
        words.push(if clean { "clean".to_string() } else { format!("{n} flag{}", if n > 1 { "s" } else { "" }) });
    }
    if words.is_empty() {
        return None;
    }
    let warn = sites.is_some_and(|s| s > 1) || !clean;
    Some((words.join(" · "), warn, flags.map(|f| f.warnings).unwrap_or_default()))
}

/// The selection the pill is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    pub start: usize,
    pub end: usize,
    pub bases: String,
    /// How many places it anneals, for a stretch in the band where that is worth saying.
    pub sites: Option<usize>,
}

impl Selected {
    pub fn of(c: &Construct, start: usize, end: usize) -> Selected {
        let bases = selected_bases(c, start, end);
        let sites = SPEC.contains(&bases.len()).then(|| anneal_sites(&c.sequence, &bases, c.circular));
        Selected { start, end, bases, sites }
    }
}

/// + Primer while it is being answered on the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimerAsk {
    /// The selection it was opened for; a new selection lets it go.
    pub span: (usize, usize),
    pub forward: bool,
    pub name: String,
    focused: bool,
}

impl PrimerAsk {
    pub fn new(span: (usize, usize), forward: bool) -> PrimerAsk {
        PrimerAsk { span, forward, name: String::new(), focused: false }
    }
}

/// What the person did on the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// + Primer was pressed.
    Primer,
    /// + Feature was pressed.
    Feature,
    /// The primer ask was answered: make it.
    Make { forward: bool, name: String },
    /// The primer ask was let go.
    Cancel,
}

/// Where the pill goes (`SelectionReadout.tsx` `place`): above the highlight's top piece, below
/// it when there is no room, kept off the toolbar band and inside the pane. Without a highlight
/// on screen, bottom-centre of the pane. Gives the pill's top-left.
///
/// Where native differs (PARITY.md): given `air`, the middle of the clear space between the
/// highlight's row and the row above, the pill stands centred in it rather than 8 px above the
/// highlight, so it sits between rows instead of on the lanes stacked over the bases.
pub fn placement(highlight: Option<Rect>, air: Option<f32>, size: egui::Vec2, pane: Rect) -> Pos2 {
    let Some(top) = highlight else {
        return pos2(pane.center().x - size.x / 2.0, pane.bottom() - 16.0 - size.y);
    };
    let gap = 8.0;
    let mut y = air.map_or(top.top() - size.y - gap, |air| air - size.y / 2.0) - pane.top();
    if y < 4.0 {
        y = top.bottom() - pane.top() + gap;
    }
    let max_y = pane.height() - size.y - TOOLBAR_BAND;
    if y > max_y {
        y = max_y.max(4.0);
    }
    let half = size.x / 2.0 + 6.0;
    let x = (top.center().x - pane.left()).min(pane.width() - half).max(half);
    pos2(pane.left() + x - size.x / 2.0, pane.top() + y)
}

/// A part of the pill and how wide it is.
enum Part<'a> {
    Words(String),
    Dot,
    Verb(&'a str, &'a str, Outcome),
    Ask,
}

fn mono() -> FontId {
    theme::mono(12.5)
}

fn verb_font() -> FontId {
    theme::sans_weight(11.5, Weight::Semibold)
}

/// The width of the primer ask: direction, name, ✓ and the verdict.
fn ask_width(ui: &Ui, verdict: Option<&(String, bool, Vec<String>)>) -> f32 {
    let mut w = 4.0 + direction_width(ui) + 6.0 + 108.0 + 6.0 + 34.0;
    if let Some((words, _, _)) = verdict {
        w += 6.0 + 7.0 + 5.0 + text_width(ui, words, theme::mono(11.0));
    }
    w
}

fn direction_width(ui: &Ui) -> f32 {
    (text_width(ui, "3'←5'", theme::mono(10.5)) + 16.0).max(48.0)
}

/// Draws the pill for `selected`, with the primer ask on it when one is open, and says what the
/// person did. `highlight` is the selection's top piece on screen, `pane` the editor's.
pub fn show(
    ui: &mut Ui,
    selected: &Selected,
    highlight: Option<Rect>,
    air: Option<f32>,
    pane: Rect,
    asking: Option<&mut PrimerAsk>,
    feature_verb: bool,
) -> Option<Outcome> {
    let verdict = verdict(&selected.bases, selected.sites);
    let mut parts: Vec<Part> = Vec::new();
    for (i, n) in numbers(&selected.bases).into_iter().enumerate() {
        if i > 0 {
            parts.push(Part::Dot);
        }
        parts.push(Part::Words(n));
    }
    let open = asking.is_some();
    if open {
        parts.push(Part::Ask);
    } else {
        parts.push(Part::Verb("+ Primer", "Make a primer from this selection (⌘⇧P)", Outcome::Primer));
    }
    if feature_verb && !open {
        parts.push(Part::Verb("+ Feature", "Make a feature from this selection (⌘⇧F)", Outcome::Feature));
    }
    let width_of = |ui: &Ui, part: &Part| match part {
        Part::Words(w) => text_width(ui, w, mono()),
        Part::Dot => text_width(ui, "·", mono()),
        Part::Verb(label, ..) => 4.0 + text_width(ui, label, verb_font()) + 22.0 + 2.0,
        Part::Ask => ask_width(ui, verdict.as_ref()),
    };
    let inner: f32 = parts.iter().map(|p| width_of(ui, p)).sum::<f32>() + GAP * (parts.len().saturating_sub(1)) as f32;
    let size = vec2(inner + 2.0 * PAD, HEIGHT);
    // `animation: sr-in 0.14s ease-out`: it fades in, rising 5 px, each time it appears.
    let shown = primitives::entrance(ui.ctx(), Id::unique("selection-readout"), 0.14);
    let rect = Rect::from_min_size(placement(highlight, air, size, pane) + vec2(0.0, 5.0 * (1.0 - shown)), size);
    let mut outcome = None;
    // Above the rows, so a press on a verb or in the name is the pill's and not a caret.
    let layer = egui::LayerId::new(egui::Order::Foreground, Id::unique("selection-readout"));
    let mut pill = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    pill.set_opacity(shown);
    // `backdrop-filter: blur(14px)` is not drawn; what it does over the bases is wash them out to
    // the paper they stand on, so the paper goes under the glass.
    pill.painter().rect_filled(rect, CornerRadius::same(17), theme::CELL);
    primitives::glass(pill.painter(), rect, HEIGHT / 2.0);
    let mut x = rect.left() + PAD;
    let mid = rect.center().y;
    let mut asking = asking;
    for part in &parts {
        let w = width_of(&pill, part);
        match part {
            Part::Words(words) => {
                pill.painter().text(pos2(x, mid), Align2::LEFT_CENTER, words, mono(), theme::INK);
            }
            Part::Dot => {
                pill.painter().text(pos2(x, mid), Align2::LEFT_CENTER, "·", mono(), theme::HAIRLINE_STRONG);
            }
            Part::Verb(label, tip, then) => {
                let r = Rect::from_min_size(pos2(x + 4.0, mid - 11.0), vec2(w - 4.0, 22.0));
                if verb(&mut pill, r, label, tip) {
                    outcome = Some(then.clone());
                }
            }
            Part::Ask => {
                if let Some(ask) = asking.as_deref_mut() {
                    outcome = primer_ask(&mut pill, pos2(x + 4.0, mid), ask, verdict.as_ref()).or(outcome);
                }
            }
        }
        x += w + GAP;
    }
    outcome
}

/// `.sr-verb`: a white pill with a strong hairline, ink and a solid ring on hover.
fn verb(ui: &mut Ui, r: Rect, label: &str, tip: &str) -> bool {
    let response = ui.interact(r, Id::unique(("sr-verb", label)), Sense::click()).on_hover_text(tip);
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    // `transition: background 0.12s ease, border-color 0.12s ease`.
    let lit = primitives::hover_amount(ui, response.id, hovered);
    let painter = ui.painter();
    let fill = Color32::from_rgba_unmultiplied(255, 255, 255, (179.0 + 76.0 * lit).round() as u8);
    painter.rect_filled(r, CornerRadius::same(255), fill);
    painter.rect_stroke(r, CornerRadius::same(255), Stroke::new(1.0, theme::mix(theme::INK, theme::HAIRLINE_STRONG, lit)), StrokeKind::Inside);
    painter.text(r.center(), Align2::CENTER_CENTER, label, verb_font(), theme::INK);
    response.clicked()
}

/// `PrimerInline`: the direction, the name, ✓ and the verdict, starting at `at` (left, middle).
fn primer_ask(ui: &mut Ui, at: Pos2, ask: &mut PrimerAsk, verdict: Option<&(String, bool, Vec<String>)>) -> Option<Outcome> {
    let mut outcome = None;
    let dir_w = direction_width(ui);
    let dir = Rect::from_min_size(pos2(at.x, at.y - 9.5), vec2(dir_w, 19.0));
    let tip = if ask.forward {
        "Forward primer (reads the plus strand) — click for reverse"
    } else {
        "Reverse primer (reads the minus strand) — click for forward"
    };
    // `.sr-dir { cursor: pointer }`.
    let response = ui.interact(dir, Id::unique("sr-dir"), Sense::click()).on_hover_text(tip).on_hover_cursor(CursorIcon::PointingHand);
    if response.clicked() {
        ask.forward = !ask.forward;
    }
    let ink = if ask.forward { theme::INK } else { theme::MUTED };
    let painter = ui.painter();
    painter.rect_filled(dir, CornerRadius::same(255), Color32::from_rgba_unmultiplied(255, 255, 255, 179));
    painter.rect_stroke(
        dir,
        CornerRadius::same(255),
        Stroke::new(1.0, if ask.forward { theme::INK } else { theme::HAIRLINE_STRONG }),
        StrokeKind::Inside,
    );
    painter.text(dir.center(), Align2::CENTER_CENTER, if ask.forward { "5'→3'" } else { "3'←5'" }, theme::mono(10.5), ink);

    // `.sr-primer-name`: 108 wide, white, a pill; the ring goes ink while it has the keys.
    let name = Rect::from_min_size(pos2(dir.right() + 6.0, at.y - 11.0), vec2(108.0, 22.0));
    let font = theme::sans(12.0);
    let edit = egui::TextEdit::singleline(&mut ask.name)
        .id(Id::unique("sr-primer-name"))
        .frame(egui::Frame::NONE.inner_margin(Margin::symmetric(9, 3)))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(egui::RichText::new("name").font(font).color(theme::MUTED))
        .align(Align2::LEFT_CENTER);
    ui.painter().rect_filled(name, CornerRadius::same(255), Color32::WHITE);
    let field = ui.put(name, edit);
    ui.painter().rect_stroke(
        name,
        CornerRadius::same(255),
        Stroke::new(1.0, if field.has_focus() { theme::INK } else { theme::HAIRLINE_STRONG }),
        StrokeKind::Inside,
    );
    if !ask.focused {
        field.request_focus();
        ask.focused = true;
    }
    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        outcome = Some(Outcome::Make { forward: ask.forward, name: ask.name.clone() });
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        outcome = Some(Outcome::Cancel);
    }

    // `.sr-primer-go`: ✓ on ink.
    let go = Rect::from_min_size(pos2(name.right() + 6.0, at.y - 10.0), vec2(34.0, 20.0));
    let response = ui.interact(go, Id::unique("sr-primer-go"), Sense::click()).on_hover_text("Make primer (Enter)");
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    ui.painter().rect_filled(go, CornerRadius::same(255), theme::INK);
    icon::paint(ui.painter(), go.center(), 12.0, lucide::CHECK, Color32::WHITE);
    if response.clicked() {
        outcome = Some(Outcome::Make { forward: ask.forward, name: ask.name.clone() });
    }

    if let Some((words, warn, flags)) = verdict {
        let font = theme::mono(11.0);
        let w = 7.0 + 5.0 + text_width(ui, words, font.clone());
        let r = Rect::from_min_size(pos2(go.right() + 6.0, at.y - 8.0), vec2(w, 16.0));
        let (ink, dot) = if *warn { (WARN, WARN_DOT) } else { (theme::ACCENT, theme::ACCENT) };
        ui.painter().circle_filled(pos2(r.left() + 3.5, at.y), 3.5, dot);
        ui.painter().text(pos2(r.left() + 12.0, at.y), Align2::LEFT_CENTER, words, font, ink);
        // `.sr-verdict { cursor: help }`, whether or not there are flags to say.
        let verdict = ui.interact(r, Id::unique("sr-verdict"), Sense::hover()).on_hover_cursor(CursorIcon::Help);
        if !flags.is_empty() {
            verdict.on_hover_text(flags.join(" · "));
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;
    use egui::{Rect, pos2, vec2};

    use super::{anneal_sites, numbers, placement, selected_bases, verdict};

    fn construct(sequence: &str) -> Construct {
        Construct { name: "t".into(), circular: true, sequence: sequence.into(), ..Construct::default() }
    }

    #[test]
    fn a_selection_through_the_origin_reads_round_it() {
        let c = construct("AACCGGTT");
        assert_eq!(selected_bases(&c, 2, 4), "CCG");
        assert_eq!(selected_bases(&c, 6, 1), "TTAA");
    }

    #[test]
    fn the_pill_gives_a_tm_only_where_it_means_something() {
        let c = construct(&"ACGTACGTAG".repeat(80));
        // The web's own pill on wrap_demo, bases 72 to 82: "11 bp · Tm 31°C · 55% GC".
        assert_eq!(numbers(&selected_bases(&c, 71, 81)), ["11 bp", "Tm 31°C", "55% GC"]);
        assert_eq!(numbers("ACGTAC"), ["6 bp", "50% GC"], "under 8 bases a Tm is not said");
        assert_eq!(numbers(&"ACGTACGTAG".repeat(13)), ["130 bp", "50% GC"], "nor above 120");
        assert_eq!(numbers(&"ACGTACGTAG".repeat(100)), ["1,000 bp", "50% GC"]);
        assert_eq!(numbers("ACGTNACGTACG"), ["12 bp", "50% GC"], "no Tm is worded as nothing, not as a lie");
    }

    #[test]
    fn a_stretch_is_counted_on_both_strands_and_through_the_origin() {
        assert_eq!(anneal_sites("AAAACCCCGGGG", "ACCC", false), 1);
        // Only through the origin of a circle.
        assert_eq!(anneal_sites("GGTAAAACCC", "ACCCG", true), 1);
        assert_eq!(anneal_sites("GGTAAAACCC", "ACCCG", false), 0);
        assert_eq!(anneal_sites("GGTAAAACCC", "CCGG", true), 1, "a palindrome is one site, not two");
        assert_eq!(anneal_sites("CCAAAAAAGG", "GGCC", true), 1);
        assert_eq!(anneal_sites("AAAA", "AA", false), 3, "overlapping registers count");
    }

    #[test]
    fn the_verdict_says_unique_or_how_many_and_whether_the_design_is_clean() {
        assert_eq!(verdict("ACGTAC", Some(1)), Some(("unique".to_string(), false, Vec::new())));
        let (words, warn, flags) = verdict("ACGTACGTAGA", Some(80)).unwrap();
        assert_eq!((words.as_str(), warn), ("80 sites", true));
        assert!(flags.is_empty(), "under 12 bases the design is not judged");
        let (words, warn, _) = verdict("atcaatgagtacaggatc", Some(1)).unwrap();
        assert_eq!((words.as_str(), warn), ("unique · clean", false));
        assert_eq!(verdict(&"A".repeat(200), None), None);
    }

    #[test]
    fn the_pill_floats_above_the_highlight_and_drops_below_it_at_the_top() {
        let pane = Rect::from_min_max(pos2(266.0, 38.0), pos2(1440.0, 900.0));
        let size = vec2(378.0, 34.0);
        let high = Rect::from_min_max(pos2(336.0, 600.0), pos2(500.0, 640.0));
        assert_eq!(placement(Some(high), None, size, pane), pos2(272.0, 558.0), "kept inside the pane's left edge");
        let top = Rect::from_min_max(pos2(700.0, 40.0), pos2(800.0, 80.0));
        assert_eq!(placement(Some(top), None, size, pane).y, 88.0);
        assert_eq!(placement(None, None, size, pane), pos2(664.0, 850.0));
    }

    #[test]
    fn given_the_air_between_rows_the_pill_stands_centred_in_it() {
        let pane = Rect::from_min_max(pos2(266.0, 38.0), pos2(1440.0, 900.0));
        let size = vec2(378.0, 34.0);
        let high = Rect::from_min_max(pos2(700.0, 600.0), pos2(900.0, 640.0));
        // The row above ends, and its air is centred at 560: well clear of the highlight at 600.
        assert_eq!(placement(Some(high), Some(560.0), size, pane).y, 543.0);
        // Air too near the top of the pane: below the highlight, as before.
        let top = Rect::from_min_max(pos2(700.0, 40.0), pos2(800.0, 80.0));
        assert_eq!(placement(Some(top), Some(30.0), size, pane).y, 88.0);
    }
}
