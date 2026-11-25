//! ⌘F: Find in the open plasmid, the way a text editor does it. An enzyme name or a sequence; it
//! collects the matches and jumps through them (⏎ next, ⇧⏎ previous), selecting each. It never
//! filters what is drawn. Escape closes it and leaves the hit selected.
//!
//! Ported from the web app's `editor/FindBar.tsx` (the bar 120-230), `editor/findMatches.ts`
//! (`findMatches`, the Exact path 67-132) and the `.find-bar` rules in `styles.css` 8399-8458.
//!
//! Alignment is the DEFAULT, as on the web: an imperfect fit is the normal case a researcher is
//! looking at, and an exact-match tool answers "no" to most real questions.
//!
//! Where native differs from the web (PARITY.md): what the current hit is — its length and strand,
//! or how the alignment fits — is said on a second line INSIDE the bar, in both modes, where the
//! eye already is while typing and stepping. The web captions each alignment on the rows and puts
//! Exact's label in a pill under the bar; both covered the bases and the map they described.

use bios_core::Construct;
use bios_core::enzymes::{find_cut_sites, get_enzyme};
use bios_core::search::{SearchOptions, find_binding_sites};
use bios_core::seq::{clean_seq, revcomp};
use egui::{Align2, CornerRadius, Id, Key, Margin, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::icon;
use crate::lucide;
use crate::primitives::{self, Button, Face, Material, Segment, text_width};
use crate::theme;

/// One place the query was found, its span 0-based on the plus strand (`end < start` wraps).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub start: usize,
    pub end: usize,
    pub forward: bool,
    /// What this landing is: `EcoRI`, or `18 bp · minus strand`.
    pub label: String,
}

/// Every match of `query` in `c`, ascending by start: an enzyme's sites, or where a sequence of at
/// least two bases occurs exactly on either strand (`findMatches`, Exact).
pub fn find_matches(c: &Construct, query: &str) -> Vec<FindMatch> {
    let q = query.trim();
    if q.is_empty() || c.sequence.is_empty() {
        return Vec::new();
    }
    let length = c.len();
    if let Some(enzyme) = get_enzyme(q) {
        let mut out: Vec<FindMatch> = find_cut_sites(&c.sequence, &[enzyme.name], c.circular)
            .into_iter()
            .map(|site| {
                let mut end = site.recognition_start + enzyme.length.saturating_sub(1);
                if end >= length {
                    end -= length;
                }
                FindMatch { start: site.recognition_start, end, forward: site.strand != bios_core::Strand::Reverse, label: enzyme.name.to_string() }
            })
            .collect();
        out.sort_by_key(|m| m.start);
        return out;
    }
    let s = clean_seq(q);
    if s.len() < 2 || s.len() > length {
        return Vec::new();
    }
    // A palindrome is its own reverse complement: the two scans report one site twice.
    let palindromic = revcomp(&s) == s;
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<FindMatch> = find_binding_sites(&c.sequence, &s, SearchOptions { max_mismatches: 0, circular: c.circular })
        .into_iter()
        .filter(|b| !palindromic || seen.insert((b.start, b.end)))
        .map(|b| {
            let mut parts = vec![format!("{} bp", s.len())];
            if b.mismatches > 0 {
                parts.push(format!("{} mismatch{}", b.mismatches, if b.mismatches > 1 { "es" } else { "" }));
            }
            if !b.forward && !palindromic {
                parts.push("minus strand".to_string());
            }
            FindMatch { start: b.start, end: b.end, forward: b.forward, label: parts.join(" · ") }
        })
        .collect();
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.forward.cmp(&a.forward)));
    out
}

/// How a sequence is found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// How does this sequence fit here, mismatches, inserts and overhangs included.
    #[default]
    Alignment,
    Exact,
}

/// What the alignment is doing, for the tally (`FindBar.tsx` `count`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Aligning {
    /// The query is being aligned and nothing for it has come back yet.
    pub pending: bool,
    /// The query is longer than this construct admits: the longest it does, in bases.
    pub too_long: Option<usize>,
    /// The aligner's own words when it could not answer.
    pub error: Option<String>,
    /// The answer stopped at the most it reports, so there may be more.
    pub capped: bool,
}

/// Whether the query names an enzyme: then it is that enzyme's sites in either mode.
pub fn is_enzyme(query: &str) -> bool {
    get_enzyme(query.trim()).is_some()
}

/// Whether the selection's read-out stands down because the Find bar already describes this
/// exact selection (`editor/readoutYield.ts`). Not "Find is open": a selection dragged somewhere
/// else while searching is the person's own question and keeps its numbers. Spans are compared
/// exactly, as stored (an end before the start wraps), because stepping to a hit selects exactly
/// it; yielding on overlap would take the pill from a selection that merely brushes one.
pub fn readout_yields_to_find(selection: Option<(usize, usize)>, hits: &[(usize, usize)]) -> bool {
    selection.is_some_and(|sel| hits.contains(&sel))
}

/// The bar while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindBar {
    pub query: String,
    pub mode: Mode,
    /// Which match is current.
    pub index: usize,
    /// The field takes the keyboard (and selects what is in it) on the next frame.
    focus: bool,
}

impl FindBar {
    pub fn new(query: Option<String>) -> FindBar {
        FindBar { query: query.unwrap_or_default(), mode: Mode::Alignment, index: 0, focus: true }
    }

    /// Opened again while open: the field takes the keyboard again.
    pub fn refocus(&mut self) {
        self.focus = true;
    }

    /// Whether the query goes to the aligner: Alignment mode, and not an enzyme's name.
    pub fn aligns(&self) -> bool {
        self.mode == Mode::Alignment && !is_enzyme(&self.query)
    }

    /// What the tally says (`FindBar.tsx` `count`): the cap when it was hit ("24+"), "aligning…"
    /// while nothing has come back, the aligner's own words when it could not align, and never
    /// "No matches" for a query that was not searched.
    pub fn count(&self, matches: &[FindMatch], aligning: &Aligning) -> String {
        let aligns = self.aligns();
        if self.query.trim().is_empty() {
            String::new()
        } else if let Some(limit) = aligning.too_long.filter(|_| aligns) {
            format!("Find aligns up to {} bases here", crate::selection_readout::thousands(limit))
        } else if !matches.is_empty() {
            format!("{} / {}{}", self.index + 1, matches.len(), if aligns && aligning.capped { "+" } else { "" })
        } else if let Some(error) = aligning.error.as_ref().filter(|_| aligns) {
            error.clone()
        } else if aligns && aligning.pending {
            "aligning…".to_string()
        } else {
            "No matches".to_string()
        }
    }
}

/// What the person did with the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The query changed: find again and go to the first.
    Changed,
    /// Go to the next (1) or the previous (-1) match.
    Step(i32),
    /// The mode was switched.
    Mode(Mode),
    /// Hold the query against the construct as a binder, and close.
    Keep,
    Close,
}

/// `.find-bar-input` and `.find-bar-count`.
const INPUT_W: f32 = 220.0;
const COUNT_W: f32 = 56.0;
const HEIGHT: f32 = 44.0;
/// The second line, saying what the current hit is.
const DETAIL_H: f32 = 22.0;

/// Draws the bar top-centre of `pane`, under its head band, and says what the person did.
pub fn show(ui: &mut Ui, pane: Rect, bar: &mut FindBar, matches: &[FindMatch], aligning: &Aligning) -> Option<Outcome> {
    let segments = [
        Segment {
            face: Face::Label("Alignment"),
            active: bar.mode == Mode::Alignment,
            enabled: true,
            tip: "Alignment — how does this sequence fit here, mismatches, inserts and overhangs included",
        },
        Segment { face: Face::Label("Exact"), active: bar.mode == Mode::Exact, enabled: true, tip: "Exact match only" },
    ];
    let group_w = primitives::group_width(ui, &segments, false);
    let count = bar.count(matches, aligning);
    let count_w = text_width(ui, &count, theme::mono(11.0)).max(COUNT_W);
    // Keep: only once the query has landed somewhere, as an oligo, in Alignment mode.
    let keep = bar.aligns() && aligning.too_long.is_none() && !matches.is_empty();
    let keep_button = Button::label("Keep", Material::Glass);
    let keep_w = if keep { keep_button.width(ui) + 6.0 } else { 0.0 };
    let width = 8.0 + 14.0 + 6.0 + INPUT_W + 6.0 + count_w + 6.0 + keep_w + group_w + 3.0 * (6.0 + 34.0) + 8.0;
    let detail = matches.get(bar.index).map(|m| m.label.as_str()).filter(|l| !l.is_empty());
    let height = HEIGHT + if detail.is_some() { DETAIL_H } else { 0.0 };
    let rect = Rect::from_min_size(pos2(pane.center().x - width / 2.0, pane.top() + theme::HEAD_H + 8.0), vec2(width, height));
    let layer = egui::LayerId::new(egui::Order::Foreground, Id::unique("find-bar"));
    let mut ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect.expand(40.0)));
    let painter = ui.painter().clone();
    // `box-shadow: var(--shadow-cell), 0 0 0 1px var(--hairline)`.
    primitives::shadow(&painter, rect, HEIGHT / 2.0, theme::SHADOW_CELL);
    painter.rect_filled(rect, CornerRadius::same(22), theme::CELL);
    painter.rect_stroke(rect, CornerRadius::same(22), Stroke::new(1.0, theme::HAIRLINE), StrokeKind::Outside);
    let mid = rect.top() + HEIGHT / 2.0;
    let mut x = rect.left() + 8.0;
    icon::paint(&painter, pos2(x + 7.0, mid), 14.0, lucide::SEARCH, theme::MUTED);
    x += 14.0 + 6.0;

    let mut outcome = None;
    let font = theme::sans(13.0);
    let field_rect = Rect::from_min_size(pos2(x, mid - 10.0), vec2(INPUT_W, 20.0));
    let id = Id::unique("find-bar-input");
    let edit = egui::TextEdit::singleline(&mut bar.query)
        .id(id)
        .frame(egui::Frame::NONE.inner_margin(Margin::ZERO))
        .font(font.clone())
        .text_color(theme::INK)
        .hint_text(egui::RichText::new("Find enzyme or sequence…").font(font).color(theme::MUTED))
        .align(Align2::LEFT_CENTER);
    let field = ui.put(field_rect, edit);
    if bar.focus {
        field.request_focus();
        // `inputRef.current?.select()`: what is there is chosen, so typing replaces it.
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(bar.query.chars().count()));
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), id);
        bar.focus = false;
    }
    if field.changed() {
        outcome = Some(Outcome::Changed);
    }
    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        outcome = Some(Outcome::Step(if ui.input(|i| i.modifiers.shift) { -1 } else { 1 }));
        field.request_focus();
    }
    x += INPUT_W + 6.0;
    painter.text(pos2(x + count_w, mid), Align2::RIGHT_CENTER, count, theme::mono(11.0), theme::MUTED);
    x += count_w + 6.0;
    if keep {
        let tip = "Hold this oligo against the construct — it stays drawn while you work, and never reaches the file";
        if keep_button.show_at(&mut ui, pos2(x, mid - 17.0)).on_hover_text(tip).clicked() {
            outcome = Some(Outcome::Keep);
        }
        x += keep_w;
    }
    match primitives::group(&mut ui, pos2(x, mid - 17.0), &segments, Material::Glass, false) {
        Some(0) if bar.mode != Mode::Alignment => outcome = Some(Outcome::Mode(Mode::Alignment)),
        Some(1) if bar.mode != Mode::Exact => outcome = Some(Outcome::Mode(Mode::Exact)),
        _ => {}
    }
    x += group_w + 6.0;
    let buttons: [(&[icon::Node], &str, bool, Option<Outcome>); 3] = [
        (lucide::CHEVRON_UP, "Previous (⇧⏎)", !matches.is_empty(), Some(Outcome::Step(-1))),
        (lucide::CHEVRON_DOWN, "Next (⏎)", !matches.is_empty(), Some(Outcome::Step(1))),
        (lucide::X, "Close (Esc)", true, Some(Outcome::Close)),
    ];
    for (nodes, tip, enabled, then) in buttons {
        let button = Button { enabled, ..Button::icon(nodes, 15.0, Material::Glass) };
        if button.show_at(&mut ui, pos2(x, mid - 17.0)).on_hover_text(tip).clicked() {
            outcome = then;
        }
        x += 34.0 + 6.0;
    }
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        outcome = Some(Outcome::Close);
    }
    // What the current landing is, on the bar's second line, under the field it answers.
    if let Some(detail) = detail {
        let line =
            Rect::from_min_max(pos2(rect.left() + 8.0 + 14.0 + 6.0, rect.top() + HEIGHT - 6.0), pos2(rect.right() - 22.0, rect.bottom() - 6.0));
        let font = theme::mono(11.0);
        let shown = crate::session_nav::clip(&ui, detail, &font, line.width());
        let r = painter.text(line.left_center(), Align2::LEFT_CENTER, shown, font, theme::MUTED);
        ui.interact(r, Id::unique("find-bar-detail"), egui::Sense::hover()).on_hover_text(detail);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use bios_core::Construct;

    use super::{Aligning, FindBar, FindMatch, Mode, find_matches, readout_yields_to_find};

    // `editor/readoutYield.test.ts`, in its order and with its numbers.
    const HITS: [(usize, usize); 2] = [(100, 117), (400, 429)];

    #[test]
    fn the_read_out_yields_on_the_span_find_is_captioning() {
        assert!(readout_yields_to_find(Some((100, 117)), &HITS));
        assert!(readout_yields_to_find(Some((400, 429)), &HITS));
    }

    #[test]
    fn a_selection_dragged_somewhere_else_keeps_its_numbers() {
        assert!(!readout_yields_to_find(Some((900, 940)), &HITS));
    }

    #[test]
    fn a_selection_that_merely_overlaps_a_hit_still_gets_its_numbers() {
        assert!(!readout_yields_to_find(Some((100, 130)), &HITS));
        assert!(!readout_yields_to_find(Some((90, 117)), &HITS));
        assert!(!readout_yields_to_find(Some((105, 110)), &HITS));
    }

    #[test]
    fn with_no_probe_there_is_nothing_to_yield_to() {
        assert!(!readout_yields_to_find(Some((100, 117)), &[]));
    }

    #[test]
    fn no_selection_is_not_a_yield() {
        assert!(!readout_yields_to_find(None, &HITS));
    }

    #[test]
    fn an_origin_spanning_selection_is_compared_literally_not_normalised() {
        assert!(readout_yields_to_find(Some((5900, 40)), &[(5900, 40)]));
        assert!(!readout_yields_to_find(Some((5900, 40)), &[(40, 5900)]));
    }

    fn construct(sequence: &str, circular: bool) -> Construct {
        Construct { name: "t".into(), circular, sequence: sequence.into(), ..Construct::default() }
    }

    #[test]
    fn a_sequence_is_found_on_both_strands_and_says_which() {
        // GGATC on the plus strand at 2, and its reverse complement GATCC at 9: a minus-strand hit.
        let c = construct("AAGGATCAAGATCCAA", false);
        let found = find_matches(&c, "ggatc");
        let summary: Vec<(usize, usize, &str)> = found.iter().map(|m| (m.start, m.end, m.label.as_str())).collect();
        assert_eq!(summary, [(2, 6, "5 bp"), (9, 13, "5 bp · minus strand")]);
    }

    #[test]
    fn a_palindrome_is_one_site_and_names_no_strand() {
        let found = find_matches(&construct("AAGAATTCAA", false), "GAATTC");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "6 bp");
    }

    #[test]
    fn an_enzyme_name_finds_its_sites_and_through_the_origin_of_a_circle() {
        let found = find_matches(&construct("ATTCAAAAAAAAAAGA", true), "EcoRI");
        assert_eq!(found.iter().map(|m| (m.start, m.end, m.label.as_str())).collect::<Vec<_>>(), [(14, 3, "EcoRI")]);
    }

    #[test]
    fn under_two_bases_nothing_is_searched_and_the_tally_says_what_is_true() {
        let c = construct("AAACCCAAACCC", false);
        assert!(find_matches(&c, "a").is_empty());
        assert!(find_matches(&c, "   ").is_empty());
        let mut bar = FindBar::new(Some("AAAC".into()));
        bar.mode = Mode::Exact;
        let none = Aligning::default();
        let found = find_matches(&c, &bar.query);
        assert_eq!(bar.count(&found, &none), "1 / 2");
        bar.index = 1;
        assert_eq!(bar.count(&found, &none), "2 / 2");
        bar.query = "TTTT".into();
        assert_eq!(bar.count(&find_matches(&c, &bar.query), &none), "No matches");
        bar.query.clear();
        assert_eq!(bar.count(&[], &none), "");
    }

    #[test]
    fn alignment_is_the_default_and_its_tally_says_what_is_true_right_now() {
        let mut bar = FindBar::new(Some("ACGTACGTAC".into()));
        assert_eq!(bar.mode, Mode::Alignment);
        assert!(bar.aligns());
        let hit = FindMatch { start: 0, end: 9, forward: true, label: String::new() };
        assert_eq!(bar.count(&[], &Aligning { pending: true, ..Aligning::default() }), "aligning…");
        assert_eq!(bar.count(std::slice::from_ref(&hit), &Aligning { capped: true, ..Aligning::default() }), "1 / 1+");
        assert_eq!(bar.count(&[], &Aligning { error: Some("could not align".into()), ..Aligning::default() }), "could not align");
        assert_eq!(bar.count(&[], &Aligning { too_long: Some(5306), ..Aligning::default() }), "Find aligns up to 5,306 bases here");
        assert_eq!(bar.count(&[], &Aligning::default()), "No matches");
        // An enzyme's name is that enzyme's sites in either mode.
        bar.query = "EcoRI".into();
        assert!(!bar.aligns());
        assert_eq!(bar.count(&[hit], &Aligning { capped: true, ..Aligning::default() }), "1 / 1");
    }
}
