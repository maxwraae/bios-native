//! The scissors dial: how many times an enzyme may cut before its sites are drawn, and from which
//! table. Ported from Bios `editor/EnzymesMenu.tsx` (120-227) and `editor/cutsites.ts`
//! (`CutsiteView`, `ALL_ENZYME_COUNT`), its look from `styles.css` (`.oge-pop-panel`, lines
//! 6790-6802; `.enzymes-*`, lines 8201-8270).
//!
//! The foot's "All enzymes" opens the list panel (`enzymes_panel.rs`); the note under the dial
//! offers to turn the enzyme names back on when they are off and there is something to name.

use bios_core::Construct;
use bios_core::enzymes::{COMMON_ENZYMES, CutSite, enzymes, find_cut_sites, get_enzyme};
use egui::{Align2, Area, CornerRadius, Id, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::primitives::{Face, Material, Segment, group_small, shadow};
use crate::rows::dial_sites;
use crate::theme;

/// How many times an enzyme may cut and still be drawn: none, once, at most twice or three times,
/// or `None` for every site (`CutLimit`).
pub type CutLimit = Option<usize>;

/// Which enzymes the map considers and how many times they may cut (`CutsiteView`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CutsiteView {
    pub limit: CutLimit,
    /// `false` is the ~70 workhorses; `true` every group in the table.
    pub all: bool,
}

/// `CUTSITE_VIEW_DEFAULT`: the common enzymes that cut once.
pub const CUTSITE_VIEW_DEFAULT: CutsiteView = CutsiteView { limit: Some(1), all: false };

impl Default for CutsiteView {
    fn default() -> Self {
        CUTSITE_VIEW_DEFAULT
    }
}

/// The dial, in order: its value, its label and its tooltip.
const LIMITS: [(CutLimit, &str, &str); 5] = [
    (Some(0), "None", "Draw no cut sites"),
    (Some(1), "Unique", "Only enzymes that cut once"),
    (Some(2), "≤2", "Enzymes that cut at most twice"),
    (Some(3), "≤3", "Enzymes that cut at most three times"),
    (None, "All", "Every site the set cuts"),
];

/// The cut sites `view` draws on `c`, without any enzyme asked for by name.
pub fn cut_sites(c: &Construct, view: CutsiteView) -> Vec<CutSite> {
    cut_sites_with(c, view, &[])
}

/// The cut sites `view` draws on `c`, plus every site of each enzyme in `shown`, asked for by name
/// from the Enzymes panel, whatever the dial says (`computeCutSites`, `withNamed`). In position
/// order.
pub fn cut_sites_with(c: &Construct, view: CutsiteView, shown: &[String]) -> Vec<CutSite> {
    if c.sequence.is_empty() {
        return Vec::new();
    }
    let mut sites = dial_view_sites(c, view);
    let already: std::collections::HashSet<&str> = sites.iter().map(|s| s.enzyme).collect();
    let missing: Vec<&str> = shown.iter().filter_map(|n| get_enzyme(n)).map(|e| e.name).filter(|n| !already.contains(n)).collect();
    if missing.is_empty() {
        return sites;
    }
    sites.extend(find_cut_sites(&c.sequence, &missing, c.circular));
    sites.sort_by_key(|s| s.top_cut);
    sites
}

fn dial_view_sites(c: &Construct, view: CutsiteView) -> Vec<CutSite> {
    if view.all {
        let names: Vec<&str> = enzymes().iter().map(|e| e.name).collect();
        dial_sites(&c.sequence, c.circular, &names, view.limit)
    } else {
        dial_sites(&c.sequence, c.circular, &COMMON_ENZYMES, view.limit)
    }
}

/// What the foot says about what is drawn.
pub fn summary(sites: &[CutSite], limit: CutLimit) -> String {
    let mut names: Vec<&str> = sites.iter().map(|s| s.enzyme).collect();
    names.sort_unstable();
    names.dedup();
    let enzymes = names.len();
    let count = sites.len();
    if limit == Some(0) {
        "No cut sites drawn".into()
    } else if count == 0 {
        if limit.is_none() { "Nothing in this set cuts here".into() } else { "Nothing cuts this few times — try All".into() }
    } else if limit == Some(1) {
        format!("{enzymes} {} once", if enzymes == 1 { "enzyme cuts" } else { "enzymes cut" })
    } else {
        format!(
            "{enzymes} {} · {} {}",
            if enzymes == 1 { "enzyme" } else { "enzymes" },
            bios_layout::overview::scene::group_thousands(count as f64),
            if count == 1 { "site" } else { "sites" }
        )
    }
}

/// `.oge-pop-panel.enzymes-panel`'s width: its 300px floor.
const WIDTH: f32 = 300.0;
/// One `.enzymes-dial` row: 6px padding round a 28px group.
const DIAL_H: f32 = 6.0 + 28.0 + 6.0;
/// `.enzymes-foot`: 2px above its rule, 7px padding round an 11.5px line.
const FOOT_H: f32 = 2.0 + 1.0 + 7.0 + 15.0 + 7.0;

/// What the panel did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The dial or the table changed.
    Changed(CutsiteView),
    /// The pointer has been away from the panel and its button long enough: close it.
    Close,
    /// "All enzymes": open the list panel and close this.
    OpenList,
    /// "Enzyme names are off on the map — turn them on".
    NamesOn,
}

/// `.enzymes-note`: 4px above, 8px padding round a 12px line.
const NOTE_H: f32 = 4.0 + 8.0 + 15.0 + 8.0;

/// Draws the open panel, centred over `row_center` with its bottom 10px above `trigger`'s top, as
/// the web places it under the row of pills. `sites` is what is drawn now, for the foot.
pub fn show(ui: &mut Ui, row_center: f32, trigger: Rect, view: CutsiteView, sites: &[CutSite], names_on: bool) -> Option<Outcome> {
    // The names being off only matters when there is something to name.
    let names_off = !names_on && view.limit != Some(0) && !sites.is_empty();
    let height = 4.0 + 2.0 * DIAL_H + if names_off { NOTE_H } else { 0.0 } + FOOT_H;
    let rect = Rect::from_min_size(pos2((row_center - WIDTH / 2.0).round(), trigger.top() - 10.0 - height), vec2(WIDTH, height));
    let mut outcome = None;
    Area::new(Id::unique("enzymes-panel")).order(Order::Foreground).fixed_pos(rect.min).fade_in(false).show(ui.ctx(), |ui| {
        ui.set_min_size(rect.size());
        let painter = ui.painter().clone();
        shadow(&painter, rect, theme::RADIUS, theme::SHADOW_CELL);
        painter.rect_filled(rect, CornerRadius::same(theme::RADIUS as u8), theme::CELL);
        painter.rect_stroke(rect, CornerRadius::same(theme::RADIUS as u8), Stroke::new(1.0, theme::HAIRLINE), egui::StrokeKind::Outside);
        // Swallow presses on the panel's own ground so they do not reach the map under it.
        ui.allocate_rect(rect, Sense::click());

        let limits: Vec<Segment> =
            LIMITS.iter().map(|(v, label, tip)| Segment { face: Face::Label(label), active: *v == view.limit, enabled: true, tip }).collect();
        let all_label = format!("All {}", enzymes().len());
        let sets = [
            Segment { face: Face::Label("Common"), active: !view.all, enabled: true, tip: "The ~70 common cloning enzymes" },
            Segment { face: Face::Label(&all_label), active: view.all, enabled: true, tip: "The full REBASE-style table" },
        ];
        let mut y = rect.top() + 4.0;
        let label_x = rect.left() + 12.0;
        let group_x = |ui: &Ui, label: &str| label_x + crate::primitives::text_width(ui, label, theme::sans(12.0)).max(56.0) + 10.0;
        for (label, which) in [("Cut sites", 0), ("From", 1)] {
            let mid = y + DIAL_H / 2.0;
            painter.text(pos2(label_x, mid), Align2::LEFT_CENTER, label, theme::sans(12.0), theme::MUTED);
            let at = pos2(group_x(ui, label), mid - 14.0);
            if which == 0 {
                if let Some(i) = group_small(ui, at, &limits, Material::Float)
                    && let Some((limit, _, _)) = LIMITS.get(i)
                {
                    outcome = Some(Outcome::Changed(CutsiteView { limit: *limit, ..view }));
                }
            } else if let Some(i) = group_small(ui, at, &sets, Material::Float) {
                outcome = Some(Outcome::Changed(CutsiteView { all: i == 1, ..view }));
            }
            y += DIAL_H;
        }
        if names_off {
            let note = Rect::from_min_size(pos2(rect.left(), y + 4.0), vec2(WIDTH, NOTE_H - 4.0));
            let response = ui.allocate_rect(note, Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            painter.rect_filled(note, 0.0, theme::CELL_HOVER);
            let ink = if response.hovered() { theme::INK } else { theme::MUTED };
            painter.text(
                pos2(label_x, note.center().y),
                Align2::LEFT_CENTER,
                "Enzyme names are off on the map — turn them on",
                theme::sans(12.0),
                ink,
            );
            if response.clicked() {
                outcome = Some(Outcome::NamesOn);
            }
            y += NOTE_H;
        }
        y += 2.0;
        painter.hline(rect.left()..=rect.right(), y + 0.5, Stroke::new(1.0, theme::HAIRLINE));
        let foot_mid = y + 1.0 + (FOOT_H - 3.0) / 2.0;
        painter.text(pos2(label_x, foot_mid), Align2::LEFT_CENTER, summary(sites, view.limit), theme::sans(11.5), theme::MUTED);
        // `.enzymes-search`: the door to the list, pushed right.
        let label = "All enzymes";
        let w = 8.0 + crate::primitives::text_width(ui, label, theme::sans(12.0)) + 6.0 + 13.0 + 8.0;
        let button = Rect::from_min_size(pos2(rect.right() - 12.0 - w, foot_mid - 12.0), vec2(w, 24.0));
        let response = ui.allocate_rect(button, Sense::click());
        if response.hovered() {
            painter.rect_filled(button, CornerRadius::same(theme::RADIUS_SM as u8), theme::CELL_HOVER);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let text = painter.text(pos2(button.left() + 8.0, foot_mid), Align2::LEFT_CENTER, label, theme::sans(12.0), theme::INK);
        crate::icon::paint(&painter, pos2(text.right() + 6.0 + 6.5, foot_mid), 13.0, crate::lucide::ARROW_RIGHT, theme::INK);
        if response.clicked() {
            outcome = Some(Outcome::OpenList);
        }
    });
    if outcome.is_some() {
        return outcome;
    }
    // `onMouseLeave` → close after 140ms, unless the pointer comes back.
    let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p: Pos2| rect.expand(2.0).contains(p) || trigger.contains(p));
    let now = ui.input(|i| i.time);
    let id = Id::unique("enzymes-panel-left");
    if over {
        ui.data_mut(|d| d.remove::<f64>(id));
        return None;
    }
    let left = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    if now - left >= 0.14 {
        ui.data_mut(|d| d.remove::<f64>(id));
        return Some(Outcome::Close);
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
    None
}
